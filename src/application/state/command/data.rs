//! Helper methods for data-shaping commands extracted from the
//! `execute_command` dispatcher: pivot, table create, chart, goalseek.
//!
//! These are the longer arm bodies that benefit from being broken out;
//! the trivial one-line arms (`["bold"] => self.toggle_bold()`) stay
//! inline in the dispatcher.

use super::super::{App, parse_range};
use crate::domain::{CellData, FormulaEvaluator, Spreadsheet};

impl App {
    /// `:table create A1:D10 [name=Sales]` — see [`App::add_table`].
    pub(super) fn cmd_table_create(&mut self, range_str: &str, opts: &[&str], trimmed: &str) {
        let Some((start, end)) = parse_range(range_str) else {
            self.status_message = Some(format!("Invalid range: {}", range_str));
            return;
        };
        let mut name = format!("Table{}", self.workbook.current_sheet().tables.len() + 1);
        // Recover case-preserved opts from the original command so
        // `name=Mine` is stored as "Mine", not "mine".
        let original_opts: Vec<&str> = trimmed.split_whitespace().skip(3).collect();
        for (i, opt) in opts.iter().enumerate() {
            if opt.starts_with("name=") {
                if let Some(orig) = original_opts.get(i)
                    && let Some(n) = orig.strip_prefix("name=")
                {
                    name = n.to_string();
                    continue;
                }
                if let Some(n) = opt.strip_prefix("name=") {
                    name = n.to_string();
                }
            }
        }
        if let Err(e) = self.add_table((start, end), &name) {
            self.status_message = Some(e);
        }
    }

    /// `:goalseek TARGET_CELL EXPECTED INPUT_CELL` — see [`App::goal_seek`].
    pub(super) fn cmd_goalseek(&mut self, target: &str, expected: &str, input: &str) {
        let target_pos = Spreadsheet::parse_cell_reference(target);
        let input_pos = Spreadsheet::parse_cell_reference(input);
        let expected_v: f64 = expected.parse().unwrap_or(0.0);
        let (Some(t), Some(i)) = (target_pos, input_pos) else {
            self.status_message = Some("goalseek: bad cell reference".to_string());
            return;
        };
        self.status_message = Some(match self.goal_seek(t, expected_v, i) {
            Ok(v) => format!("Goal seek: {} = {:.6}", input, v),
            Err(e) => e,
        });
    }
}

impl App {
    /// `:pivot <source> <target> [row=COL] [value=COL] [agg=sum|count|avg|min|max]`
    ///
    /// Writes a one-key pivot table to `target` whose cells are formula
    /// references back into the source range, so subsequent edits to the
    /// source data auto-refresh the pivot.
    pub(super) fn cmd_pivot(&mut self, source: &str, target_str: &str, opts: &[&str]) {
        let (Some((s, e)), Some(t)) = (
            parse_range(source),
            Spreadsheet::parse_cell_reference(target_str),
        ) else {
            self.status_message = Some("pivot: bad source range or target".to_string());
            return;
        };

        let mut row_col: Option<usize> = None;
        let mut val_col: Option<usize> = None;
        let mut agg = "sum".to_string();
        for o in opts {
            if let Some(c) = o.strip_prefix("row=") {
                row_col = Spreadsheet::parse_column_label(c);
            } else if let Some(c) = o.strip_prefix("value=") {
                val_col = Spreadsheet::parse_column_label(c);
            } else if let Some(a) = o.strip_prefix("agg=") {
                agg = a.to_string();
            }
        }
        let Some(row_col) = row_col else {
            self.status_message = Some("pivot: row=COL required".to_string());
            return;
        };
        let val_col = val_col.unwrap_or(row_col);

        // Collect distinct row-keys preserving sort order. The pivot values
        // are written as formulas (`=SUMIF(...)`) so they auto-update when
        // source data changes.
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        {
            let sheet = self.workbook.current_sheet();
            for r in s.0..=e.0 {
                let k = sheet.get_cell(r, row_col).value;
                if !k.is_empty() {
                    keys.insert(k);
                }
            }
        }
        let row_col_label = Spreadsheet::column_label(row_col);
        let val_col_label = Spreadsheet::column_label(val_col);
        let source_range = format!("{}{}:{}{}", row_col_label, s.0 + 1, row_col_label, e.0 + 1);
        let sum_range = format!("{}{}:{}{}", val_col_label, s.0 + 1, val_col_label, e.0 + 1);
        let agg_formula = |key: &str| -> String {
            let key_esc = key.replace('"', "\"\"");
            match agg.as_str() {
                "count" => format!("=COUNTIF({}, \"{}\")", source_range, key_esc),
                "avg" | "average" => {
                    format!("=AVERAGEIF({}, \"{}\", {})", source_range, key_esc, sum_range)
                }
                "min" => format!("=MIN(IF({}=\"{}\", {}))", source_range, key_esc, sum_range),
                "max" => format!("=MAX(IF({}=\"{}\", {}))", source_range, key_esc, sum_range),
                _ => format!("=SUMIF({}, \"{}\", {})", source_range, key_esc, sum_range),
            }
        };

        let mut rows: Vec<(usize, usize, CellData)> = Vec::new();
        rows.push((
            t.0,
            t.1,
            CellData {
                value: "Key".to_string(),
                formula: None,
                format: None,
                comment: None,
                spill_anchor: None,
            },
        ));
        rows.push((
            t.0,
            t.1 + 1,
            CellData {
                value: agg.clone(),
                formula: None,
                format: None,
                comment: None,
                spill_anchor: None,
            },
        ));
        for (i, k) in keys.iter().enumerate() {
            rows.push((
                t.0 + 1 + i,
                t.1,
                CellData {
                    value: k.clone(),
                    formula: None,
                    format: None,
                    comment: None,
                    spill_anchor: None,
                },
            ));
            let formula = agg_formula(k);
            // Evaluate immediately so the cell shows its initial value;
            // set_many will rebuild deps so it tracks.
            let evaluator = FormulaEvaluator::for_workbook(
                &self.workbook,
                self.workbook.current_sheet(),
                &self.workbook.named_ranges,
            );
            // Publish a clock for the same NOW()/TODAY() consistency reason
            // as the editing/autofill/paste paths.
            let initial = crate::domain::parser::with_recalc_clock(
                crate::domain::parser::now_serial(),
                || evaluator.evaluate_formula(&formula),
            );
            rows.push((
                t.0 + 1 + i,
                t.1 + 1,
                CellData {
                    value: initial,
                    formula: Some(formula),
                    format: None,
                    comment: None,
                    spill_anchor: None,
                },
            ));
        }
        self.set_many_with_undo(rows);
        self.status_message = Some(format!(
            "Pivot written to {}{}: {} groups (auto-refreshes via formulas)",
            Spreadsheet::column_label(t.1),
            t.0 + 1,
            keys.len()
        ));
    }
}
