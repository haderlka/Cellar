//! Submodule of `state`: the tools behind the GUI's Data, Edit and Format
//! dialogs. Sort, remove duplicates, text to columns, goal seek, paste
//! special, format painter, tables, names, conditional formats, data
//! validation, filter, tracing and sheet duplicate/move.
//!
//! None of these add anything to the file format: they edit cells, or the
//! tables, names, rules and view state the workbook already stores.

use super::*;
use crate::domain::{CellStyle, ConditionalFormat, Table};
use std::cmp::Ordering;

/// A sort level: the column and whether it sorts ascending.
pub type SortKey = (usize, bool);

/// What Paste Special takes from the copied cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    All,
    /// Results only; the target keeps its formatting.
    Values,
    /// Formulas and constants; the target keeps its formatting.
    Formulas,
    /// Number format, bold, colours; the target keeps its content.
    Formats,
}

impl PasteMode {
    pub const ALL: [PasteMode; 4] = [PasteMode::All, PasteMode::Values, PasteMode::Formulas, PasteMode::Formats];

    pub fn label(self) -> &'static str {
        match self {
            PasteMode::All => "All",
            PasteMode::Values => "Values",
            PasteMode::Formulas => "Formulas",
            PasteMode::Formats => "Formats",
        }
    }
}

/// Formatting picked up by the format painter.
#[derive(Debug, Clone)]
pub struct FormatBlock {
    pub rows: usize,
    pub cols: usize,
    formats: HashMap<(usize, usize), CellFormat>,
}

/// Whether `value` passes a filter. The criteria reads like SUMIF's:
/// `=text` must equal the value, `<>text` must not, `>`, `>=`, `<`, `<=`
/// compare (numbers as numbers), and anything else must be contained in
/// the value. Case is ignored.
pub fn filter_matches(value: &str, criteria: &str) -> bool {
    let (value_lc, numeric) = (value.to_lowercase(), value.trim().parse::<f64>().ok());
    // Numbers compare with numbers and text with text; a number is never
    // "greater than" a word.
    let compare = |rest: &str| match (numeric, rest.trim().parse::<f64>().ok()) {
        (Some(v), Some(limit)) => v.partial_cmp(&limit),
        (None, None) if !value.is_empty() => Some(value_lc.as_str().cmp(rest.to_lowercase().as_str())),
        _ => None,
    };
    let equal = |rest: &str| match (numeric, rest.trim().parse::<f64>().ok()) {
        (Some(v), Some(wanted)) => v == wanted,
        _ => value_lc == rest.to_lowercase(),
    };
    if let Some(rest) = criteria.strip_prefix(">=") {
        compare(rest).is_some_and(Ordering::is_ge)
    } else if let Some(rest) = criteria.strip_prefix("<=") {
        compare(rest).is_some_and(Ordering::is_le)
    } else if let Some(rest) = criteria.strip_prefix("<>") {
        !equal(rest)
    } else if let Some(rest) = criteria.strip_prefix('>') {
        compare(rest).is_some_and(Ordering::is_gt)
    } else if let Some(rest) = criteria.strip_prefix('<') {
        compare(rest).is_some_and(Ordering::is_lt)
    } else if let Some(rest) = criteria.strip_prefix('=') {
        equal(rest)
    } else {
        value_lc.contains(&criteria.to_lowercase())
    }
}

/// Split the arguments of a function call at its top-level commas.
fn split_arguments(args: &str) -> Vec<&str> {
    let (mut parts, mut start, mut depth, mut in_string) = (Vec::new(), 0, 0usize, false);
    for (i, ch) in args.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => depth = depth.saturating_sub(1),
            ',' if !in_string && depth == 0 => {
                parts.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(args[start..].trim());
    parts
}

/// Checks a name for a named range or table: letters, digits, `_` and `.`,
/// not starting with a digit, and not something that reads as a cell.
pub fn valid_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_');
    if !first_ok || !chars.all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
        return Err("Names start with a letter or _ and contain only letters, digits, _ and .".to_string());
    }
    if Spreadsheet::parse_cell_reference(&name.to_uppercase()).is_some() {
        return Err(format!("{} is a cell reference", name));
    }
    if matches!(name.to_uppercase().as_str(), "TRUE" | "FALSE") {
        return Err(format!("{} is a reserved word", name));
    }
    Ok(())
}

/// Excel's sort order: numbers, then text (ignoring case), blanks last
/// whichever the direction.
fn sort_cmp(a: &str, b: &str, ascending: bool) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => {
            let order = match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)),
            };
            if ascending { order } else { order.reverse() }
        }
    }
}

fn plain(value: String) -> CellData {
    CellData { value, ..CellData::default() }
}

fn is_blank(cell: &CellData) -> bool {
    cell.value.is_empty() && cell.formula.is_none() && cell.format.is_none() && cell.comment.is_none()
}

impl App {
    /// The selection, or the cursor cell when nothing is selected.
    pub fn selection_or_cursor(&self) -> CellRange {
        let cur = (self.selected_row, self.selected_col);
        self.get_selection_range().unwrap_or((cur, cur))
    }

    /// The range a Data command works on: the selection when it spans
    /// several cells, otherwise the block of data around the cursor.
    pub fn data_range(&self) -> CellRange {
        match self.get_selection_range() {
            Some((a, b)) if a != b => (a, b),
            _ => self.workbook.current_sheet().current_region(self.selected_row, self.selected_col),
        }
    }

    /// Last row of `range` that holds anything, so whole-column selections
    /// don't walk a million empty rows.
    fn last_used_row(&self, ((r0, c0), (r1, c1)): CellRange) -> Option<usize> {
        self.workbook
            .current_sheet()
            .cells
            .iter()
            .filter(|&(&(r, c), cd)| (r0..=r1).contains(&r) && (c0..=c1).contains(&c) && !is_blank(cd))
            .map(|(&(r, _), _)| r)
            .max()
    }

    /// Whether the first row of `range` looks like column titles: it is a
    /// table's header row, or holds only text above rows with numbers, or
    /// is the only bold row.
    pub fn guess_header(&self, ((r0, c0), (r1, c1)): CellRange) -> bool {
        let sheet = self.workbook.current_sheet();
        if sheet.tables.iter().any(|t| t.top_row == r0 && t.left_col <= c1 && t.right_col >= c0) {
            return true;
        }
        if r1 <= r0 {
            return false;
        }
        let cell = |r: usize, c: usize| sheet.cells.get(&(r, c));
        let numeric = |r: usize, c: usize| cell(r, c).is_some_and(|cd| cd.value.trim().parse::<f64>().is_ok());
        let bold = |r: usize| {
            let mut filled = (c0..=c1).filter_map(|c| cell(r, c)).filter(|cd| !cd.value.is_empty()).peekable();
            filled.peek().is_some() && filled.all(|cd| cd.format.as_ref().is_some_and(|f| f.style.bold))
        };
        let first_is_text = (c0..=c1).any(|c| cell(r0, c).is_some_and(|cd| !cd.value.is_empty()))
            && !(c0..=c1).any(|c| numeric(r0, c));
        let last = self.last_used_row(((r0, c0), (r1, c1))).unwrap_or(r0).min(r0 + 200);
        let numbers_below = (r0 + 1..=last).any(|r| (c0..=c1).any(|c| numeric(r, c)));
        (first_is_text && numbers_below) || (bold(r0) && !bold(r0 + 1))
    }

    /// Sort the rows of `range` by `keys` (first key first). Formulas that
    /// point at a sorted row follow it. Returns the number of rows sorted.
    pub fn sort_range(&mut self, range: CellRange, has_header: bool, keys: &[SortKey]) -> usize {
        let ((r0, c0), (_, c1)) = range;
        let first = r0 + usize::from(has_header);
        let Some(last) = self.last_used_row(range).filter(|&l| l > first) else {
            self.status_message = Some("Nothing to sort".to_string());
            return 0;
        };
        let keys: Vec<SortKey> = keys.iter().copied().filter(|(c, _)| (c0..=c1).contains(c)).collect();
        if keys.is_empty() {
            return 0;
        }
        let sheet = self.workbook.current_sheet();
        // Spilled values belong to their formula; it spills again where it lands.
        let stored = |r: usize, c: usize| sheet.cells.get(&(r, c)).filter(|cd| cd.spill_anchor.is_none()).cloned();
        let rows: Vec<Vec<Option<CellData>>> =
            (first..=last).map(|r| (c0..=c1).map(|c| stored(r, c)).collect()).collect();
        let value = |row: &[Option<CellData>], col: usize| row[col - c0].as_ref().map_or("", |cd| cd.value.as_str()).to_string();
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&a, &b| {
            keys.iter()
                .map(|&(col, asc)| sort_cmp(&value(&rows[a], col), &value(&rows[b], col), asc))
                .find(|o| *o != Ordering::Equal)
                .unwrap_or(Ordering::Equal)
        });
        let row_map: HashMap<usize, usize> = order.iter().enumerate().map(|(new, &old)| (first + old, first + new)).collect();
        let evaluator = FormulaEvaluator::new(sheet);
        let mut plan = Vec::with_capacity(rows.len() * (c1 - c0 + 1));
        for (new, &old) in order.iter().enumerate() {
            for (i, cell) in rows[old].iter().enumerate() {
                let mut cell = cell.clone();
                if let Some(cd) = &mut cell
                    && let Some(f) = &cd.formula
                {
                    cd.formula = Some(evaluator.remap_row_references(f, &row_map, last));
                }
                plan.push((first + new, c0 + i, cell));
            }
        }
        self.apply_plan(plan);
        self.status_message = Some(format!("Sorted {} row(s)", rows.len()));
        rows.len()
    }

    /// Sort A→Z / Z→A: the data around the cursor (or the selection) by
    /// the cursor's column, keeping a header row in place.
    pub fn sort_selection(&mut self, ascending: bool) {
        let range = self.data_range();
        let col = self.selected_col.clamp(range.0.1, range.1.1);
        let has_header = self.guess_header(range);
        self.sort_range(range, has_header, &[(col, ascending)]);
    }

    /// Delete the rows of `range` that repeat an earlier row in every one
    /// of `cols` (case ignored); the rows below move up within the range.
    /// Returns how many were removed.
    pub fn remove_duplicates(&mut self, range: CellRange, has_header: bool, cols: &[usize]) -> usize {
        let ((r0, c0), (_, c1)) = range;
        let first = r0 + usize::from(has_header);
        let Some(last) = self.last_used_row(range).filter(|&l| l >= first) else { return 0 };
        let sheet = self.workbook.current_sheet();
        let mut seen = HashSet::new();
        let kept: Vec<usize> = (first..=last)
            .filter(|&r| {
                let key: Vec<String> = cols.iter().map(|&c| sheet.get_cell(r, c).value.to_lowercase()).collect();
                seen.insert(key)
            })
            .collect();
        let removed = last - first + 1 - kept.len();
        if removed == 0 {
            self.status_message = Some("No duplicate rows found".to_string());
            return 0;
        }
        let evaluator = FormulaEvaluator::new(sheet);
        let mut plan = Vec::new();
        for (i, target) in (first..=last).enumerate() {
            for c in c0..=c1 {
                let cell = kept.get(i).and_then(|&source| {
                    let mut cd = sheet.cells.get(&(source, c)).filter(|cd| cd.spill_anchor.is_none()).cloned()?;
                    if let Some(f) = &cd.formula {
                        cd.formula = Some(evaluator.adjust_formula_references(f, target as i32 - source as i32, 0));
                    }
                    Some(cd)
                });
                plan.push((target, c, cell));
            }
        }
        self.apply_plan(plan);
        self.status_message = Some(format!("Removed {} duplicate row(s); {} remain", removed, kept.len()));
        removed
    }

    /// Split the text in the first column of `range` at `delimiter` into
    /// the columns to its right. Formulas are left alone. Returns how many
    /// cells were split.
    pub fn text_to_columns(&mut self, range: CellRange, delimiter: &str) -> usize {
        let ((r0, c0), (r1, _)) = range;
        if delimiter.is_empty() {
            return 0;
        }
        let mut plan = Vec::new();
        let mut split = 0;
        let mut widest = 1;
        let sheet = self.workbook.current_sheet();
        for (r, c) in self.stored_cells_in(((r0, c0), (r1, c0))) {
            let cell = sheet.get_cell(r, c);
            if cell.formula.is_some() || cell.spill_anchor.is_some() {
                continue;
            }
            // Runs of spaces count as one, as when splitting names.
            let parts: Vec<&str> = if delimiter == " " {
                cell.value.split_whitespace().collect()
            } else {
                cell.value.split(delimiter).map(str::trim).collect()
            };
            if parts.len() < 2 {
                continue;
            }
            split += 1;
            widest = widest.max(parts.len());
            for (i, part) in parts.iter().enumerate() {
                let existing = sheet.get_cell(r, c0 + i);
                plan.push((r, c0 + i, Some(CellData { value: part.to_string(), formula: None, spill_anchor: None, ..existing })));
            }
        }
        if split > 0 {
            self.workbook.current_sheet_mut().grow_to_fit(r1, c0 + widest - 1);
            self.apply_plan(plan);
        }
        self.status_message = Some(format!("Split {} cell(s) into up to {} columns", split, widest));
        split
    }

    /// Find the value of `input` that makes the formula in `target` equal
    /// `goal`, and enter it (one undo step). The search runs on a copy of
    /// the workbook.
    pub fn goal_seek(&mut self, target: (usize, usize), goal: f64, input: (usize, usize)) -> Result<f64, String> {
        let label = |(r, c): (usize, usize)| format!("{}{}", Spreadsheet::column_label(c), r + 1);
        let sheet = self.workbook.current_sheet();
        if sheet.get_cell(target.0, target.1).formula.is_none() {
            return Err(format!("{} must contain a formula", label(target)));
        }
        let input_cell = sheet.get_cell(input.0, input.1);
        if input_cell.formula.is_some() {
            return Err(format!("{} must contain a value, not a formula", label(input)));
        }
        let mut scratch = self.workbook.clone();
        let mut error = |x: f64| -> Result<f64, String> {
            scratch.set_cell_on_active(input.0, input.1, plain(x.to_string()));
            let shown = scratch.current_sheet().get_cell(target.0, target.1).value;
            shown.trim().parse::<f64>().map(|v| v - goal).map_err(|_| format!("{} is not a number ({})", label(target), shown))
        };
        let tolerance = 1e-9 * goal.abs().max(1.0);
        let start = input_cell.value.trim().parse::<f64>().unwrap_or(0.0);
        // Secant steps, falling back to bisection once the goal is bracketed.
        let (mut x0, mut x1) = (start, if start == 0.0 { 1.0 } else { start * 1.1 });
        let (mut e0, mut e1) = (error(x0)?, error(x1)?);
        let mut bracket: Option<(f64, f64, f64)> = None;
        let mut found = (e0.abs() <= tolerance).then_some(x0);
        for _ in 0..200 {
            if found.is_some() {
                break;
            }
            if e1.abs() <= tolerance {
                found = Some(x1);
                break;
            }
            if e0.signum() != e1.signum() {
                bracket = Some((x0, e0, x1));
            }
            let slope = (e1 - e0) / (x1 - x0);
            let mut next = if slope.abs() > 1e-300 { x1 - e1 / slope } else { x1 + (x1 - x0) * 2.0 };
            if let Some((a, ea, b)) = bracket {
                let (lo, hi) = (a.min(b), a.max(b));
                if !(next > lo && next < hi) {
                    next = (a + b) / 2.0;
                }
                let e_next = error(next)?;
                // Keep the half that still straddles the goal.
                bracket = Some(if ea.signum() != e_next.signum() { (a, ea, next) } else { (next, e_next, b) });
                (x0, e0, x1, e1) = (x1, e1, next, e_next);
                if (hi - lo).abs() <= 1e-13 * lo.abs().max(hi.abs()).max(1.0) {
                    found = Some(next);
                }
            } else {
                if !next.is_finite() {
                    break;
                }
                (x0, e0) = (x1, e1);
                x1 = next;
                e1 = error(x1)?;
            }
        }
        let Some(x) = found else {
            return Err("Goal Seek could not find a solution".to_string());
        };
        // Drop float noise: 4.999999999999 is entered as 5.
        let x = format!("{:.10}", x).parse::<f64>().unwrap_or(x);
        self.set_cell_with_undo(input.0, input.1, CellData { value: x.to_string(), ..input_cell });
        Ok(x)
    }

    /// Paste the copied cells at the cursor, taking only what `mode` names
    /// and optionally swapping rows and columns.
    pub fn paste_special(&mut self, mode: PasteMode, transpose: bool) {
        self.refresh_clipboard_from_system();
        let Some(clipboard) = self.clipboard.clone() else {
            self.status_message = Some("Paste Special needs cells copied in Cellar".to_string());
            return;
        };
        let (dest_row, dest_col) = (self.selected_row, self.selected_col);
        let place = |ro: usize, co: usize| if transpose { (dest_row + co, dest_col + ro) } else { (dest_row + ro, dest_col + co) };
        let (last_row, last_col) = clipboard.cells.iter().fold((dest_row, dest_col), |(mr, mc), (ro, co, _)| {
            let (r, c) = place(*ro, *co);
            (mr.max(r), mc.max(c))
        });
        self.workbook.current_sheet_mut().grow_to_fit(last_row, last_col);
        let sheet = self.workbook.current_sheet();
        let names = self.workbook.named_ranges.clone();
        let plan: Vec<(usize, usize, Option<CellData>)> =
            crate::domain::parser::with_recalc_clock(crate::domain::parser::now_serial(), || {
                let evaluator = FormulaEvaluator::for_workbook(&self.workbook, sheet, &names);
                clipboard
                    .cells
                    .iter()
                    .filter_map(|(ro, co, cell)| {
                        let (r, c) = place(*ro, *co);
                        if r >= sheet.rows || c >= sheet.cols {
                            return None;
                        }
                        let existing = sheet.get_cell(r, c);
                        // The formula moved from where it was copied to here.
                        let formula = cell.formula.as_ref().map(|f| {
                            evaluator.adjust_formula_references(
                                f,
                                r as i32 - (clipboard.source_row + ro) as i32,
                                c as i32 - (clipboard.source_col + co) as i32,
                            )
                        });
                        let with_formula = |format: Option<CellFormat>, comment: Option<String>| match &formula {
                            Some(f) if evaluator.would_create_circular_reference(f, (r, c)) => None,
                            Some(f) => Some(CellData {
                                value: evaluator.evaluate_formula(f),
                                formula: Some(f.clone()),
                                format,
                                comment,
                                spill_anchor: None,
                            }),
                            None => Some(CellData { value: cell.value.clone(), formula: None, format, comment, spill_anchor: None }),
                        };
                        let new = match mode {
                            PasteMode::All => with_formula(cell.format.clone(), cell.comment.clone())?,
                            PasteMode::Formulas => with_formula(existing.format.clone(), existing.comment.clone())?,
                            PasteMode::Values => CellData { value: cell.value.clone(), formula: None, spill_anchor: None, ..existing },
                            PasteMode::Formats => {
                                if existing.spill_anchor.is_some() {
                                    return None;
                                }
                                CellData { format: cell.format.clone(), ..existing }
                            }
                        };
                        Some((r, c, (!is_blank(&new)).then_some(new)))
                    })
                    .collect()
            });
        let n = plan.len();
        self.apply_plan(plan);
        self.status_message = Some(format!(
            "Pasted {} of {} cell(s){}",
            mode.label().to_lowercase(),
            n,
            if transpose { ", transposed" } else { "" }
        ));
    }

    /// Pick up the formatting of `range` for the format painter.
    pub fn copy_formats(&self, range: CellRange) -> FormatBlock {
        let ((r0, c0), (r1, c1)) = range;
        let formats: HashMap<(usize, usize), CellFormat> = self
            .stored_cells_in(range)
            .into_iter()
            .filter_map(|(r, c)| Some(((r - r0, c - c0), self.workbook.current_sheet().cells.get(&(r, c))?.format.clone()?)))
            .collect();
        // A whole row or column is as big as its formatted part.
        let (used_r, used_c) = formats.keys().fold((0, 0), |(mr, mc), &(r, c)| (mr.max(r), mc.max(c)));
        FormatBlock { rows: (r1 - r0 + 1).min(used_r + 1), cols: (c1 - c0 + 1).min(used_c + 1), formats }
    }

    /// Give `target` the formatting in `block`, repeated to fill it; a
    /// single target cell takes the whole block. Contents are kept.
    pub fn apply_formats(&mut self, block: &FormatBlock, target: CellRange) -> usize {
        const MAX_CELLS: usize = 1_000_000;
        let ((r0, c0), (mut r1, mut c1)) = target;
        if (r0, c0) == (r1, c1) {
            (r1, c1) = (r0 + block.rows - 1, c0 + block.cols - 1);
        }
        if (r1 - r0 + 1).saturating_mul(c1 - c0 + 1) > MAX_CELLS {
            self.status_message = Some("That range is too large to format at once".to_string());
            return 0;
        }
        self.workbook.current_sheet_mut().grow_to_fit(r1, c1);
        let sheet = self.workbook.current_sheet();
        let mut plan = Vec::new();
        for r in r0..=r1.min(sheet.rows - 1) {
            for c in c0..=c1.min(sheet.cols - 1) {
                let format = block.formats.get(&((r - r0) % block.rows, (c - c0) % block.cols)).cloned();
                let existing = sheet.get_cell(r, c);
                if existing.format != format && existing.spill_anchor.is_none() {
                    let cell = CellData { format, ..existing };
                    plan.push((r, c, (!is_blank(&cell)).then_some(cell)));
                }
            }
        }
        let n = self.apply_plan(plan);
        self.status_message = Some(format!("Formatting applied to {} cell(s)", n));
        n
    }

    /// Edit → Clear → Formats: back to General, no bold or colours.
    pub fn clear_selection_formats(&mut self) {
        self.clear_selection_part(|cell| cell.format = None, "formatting");
    }

    /// Edit → Clear → Comments.
    pub fn clear_selection_comments(&mut self) {
        self.clear_selection_part(|cell| cell.comment = None, "comments");
    }

    fn clear_selection_part(&mut self, clear: impl Fn(&mut CellData), what: &str) {
        let sheet = self.workbook.current_sheet();
        let plan: Vec<(usize, usize, Option<CellData>)> = self
            .stored_cells_in(self.selection_or_cursor())
            .into_iter()
            .filter_map(|(r, c)| {
                let mut cell = sheet.get_cell(r, c);
                if cell.spill_anchor.is_some() {
                    return None;
                }
                clear(&mut cell);
                Some((r, c, (!is_blank(&cell)).then_some(cell)))
            })
            .collect();
        let n = self.apply_plan(plan);
        self.status_message = Some(format!("Cleared {} from {} cell(s)", what, n));
    }

    // ------------------------------------------------------------ tables

    /// A table name no sheet uses yet: Table1, Table2, …
    pub fn next_table_name(&self) -> String {
        (1..)
            .map(|n| format!("Table{}", n))
            .find(|n| !self.table_name_taken(n))
            .unwrap_or_default()
    }

    fn table_name_taken(&self, name: &str) -> bool {
        self.workbook.sheets.iter().flat_map(|s| &s.tables).any(|t| t.name.eq_ignore_ascii_case(name))
    }

    /// Column titles for a table over `range`: the first row's text, with
    /// blanks named Column1… and repeats numbered so each is unique.
    fn table_headers(&self, ((r0, c0), (_, c1)): CellRange) -> Vec<String> {
        let sheet = self.workbook.current_sheet();
        let mut headers: Vec<String> = Vec::new();
        for (i, c) in (c0..=c1).enumerate() {
            let text = sheet.get_cell(r0, c).value.trim().to_string();
            let base = if text.is_empty() { format!("Column{}", i + 1) } else { text };
            let mut name = base.clone();
            let mut n = 2;
            while headers.iter().any(|h| h.eq_ignore_ascii_case(&name)) {
                name = format!("{}{}", base, n);
                n += 1;
            }
            headers.push(name);
        }
        headers
    }

    /// Format `range` as a table: its first row holds the column titles,
    /// and formulas can read columns as `Name[Title]`.
    pub fn add_table(&mut self, range: CellRange, name: &str) -> Result<(), String> {
        let ((r0, c0), (r1, c1)) = range;
        valid_name(name)?;
        if self.table_name_taken(name) || self.workbook.named_ranges.contains_key(&name.to_uppercase()) {
            return Err(format!("The name {} is already used", name));
        }
        if r1 == r0 {
            return Err("A table needs a header row and at least one row below it".to_string());
        }
        let overlaps = |t: &Table| t.top_row <= r1 && t.bottom_row >= r0 && t.left_col <= c1 && t.right_col >= c0;
        if self.workbook.current_sheet().tables.iter().any(overlaps) {
            return Err("A table can't overlap another table".to_string());
        }
        let table = Table {
            name: name.to_string(),
            top_row: r0,
            left_col: c0,
            bottom_row: r1,
            right_col: c1,
            headers: self.table_headers(range),
        };
        self.with_snapshot_undo(&format!("Create table '{}'", name), |app| {
            app.workbook.current_sheet_mut().tables.push(table);
            app.workbook.rebuild_table_names();
        });
        self.status_message = Some(format!("Created table '{}'", name));
        Ok(())
    }

    /// Turn the table back into ordinary cells; the cells stay.
    pub fn remove_table(&mut self, name: &str) -> bool {
        if !self.workbook.current_sheet().tables.iter().any(|t| t.name == name) {
            return false;
        }
        self.with_snapshot_undo(&format!("Remove table '{}'", name), |app| {
            app.workbook.current_sheet_mut().tables.retain(|t| t.name != name);
            app.workbook.rebuild_table_names();
        });
        self.status_message = Some(format!("Removed table '{}' (its cells are unchanged)", name));
        true
    }

    /// Rename a table on the active sheet and/or change the cells it
    /// covers. Formulas using `Old[Title]` are not rewritten.
    pub fn update_table(&mut self, name: &str, new_name: &str, range: CellRange) -> Result<(), String> {
        let ((r0, c0), (r1, c1)) = range;
        if !new_name.eq_ignore_ascii_case(name) {
            valid_name(new_name)?;
            if self.table_name_taken(new_name) || self.workbook.named_ranges.contains_key(&new_name.to_uppercase()) {
                return Err(format!("The name {} is already used", new_name));
            }
        }
        let overlaps = |t: &Table| t.name != name && t.top_row <= r1 && t.bottom_row >= r0 && t.left_col <= c1 && t.right_col >= c0;
        if self.workbook.current_sheet().tables.iter().any(overlaps) {
            return Err("A table can't overlap another table".to_string());
        }
        let headers = self.table_headers(range);
        self.with_snapshot_undo(&format!("Change table '{}'", name), |app| {
            if let Some(t) = app.workbook.current_sheet_mut().tables.iter_mut().find(|t| t.name == name) {
                *t = Table { name: new_name.to_string(), top_row: r0, left_col: c0, bottom_row: r1, right_col: c1, headers };
            }
            app.workbook.rebuild_table_names();
        });
        Ok(())
    }

    // ------------------------------------------------------------- names

    /// Named ranges and formulas, sorted; the names tables register for
    /// themselves (`Table`, `Table[Column]`) are left out.
    pub fn user_names(&self) -> Vec<(String, String)> {
        let mut names: Vec<(String, String)> = self
            .workbook
            .named_ranges
            .iter()
            .filter(|(k, _)| !k.contains('[') && !self.table_name_taken(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        names.sort();
        names
    }

    /// Define (or redefine) a name for a range, constant or formula.
    pub fn define_name(&mut self, name: &str, value: &str) -> Result<(), String> {
        valid_name(name)?;
        let value = value.trim().trim_start_matches('=').trim().to_string();
        if value.is_empty() {
            return Err("A name needs a range or value to refer to".to_string());
        }
        if self.table_name_taken(name) {
            return Err(format!("{} is the name of a table", name));
        }
        let name = name.to_string();
        self.with_snapshot_undo(&format!("Name '{}' = {}", name, value), |app| app.workbook.set_name(&name, &value));
        Ok(())
    }

    pub fn delete_name(&mut self, name: &str) -> bool {
        if !self.workbook.named_ranges.contains_key(&name.to_uppercase()) {
            return false;
        }
        let name = name.to_string();
        self.with_snapshot_undo(&format!("Remove name '{}'", name), |app| {
            app.workbook.remove_name(&name);
        });
        true
    }

    /// The selection as a name's value: `Sheet1!A1:B5`.
    pub fn selection_reference(&self) -> String {
        let (a, b) = self.selection_or_cursor();
        let cell = |(r, c): (usize, usize)| Spreadsheet::format_cell_reference(r, c, false, false);
        let sheet = crate::domain::format_sheet_name(&self.workbook.sheet_names[self.workbook.active_sheet]);
        if a == b { format!("{}!{}", sheet, cell(a)) } else { format!("{}!{}:{}", sheet, cell(a), cell(b)) }
    }

    /// Where `target` points: a cell or range (`B5`, `A1:C9`,
    /// `Sheet2!B5`), a named range or a table. `None` for anything else.
    pub fn resolve_reference(&self, target: &str) -> Option<(usize, CellRange)> {
        let target = target.trim();
        let wb = &self.workbook;
        if let Some((idx, t)) =
            wb.sheets.iter().enumerate().find_map(|(i, s)| s.tables.iter().find(|t| t.name.eq_ignore_ascii_case(target)).map(|t| (i, t)))
        {
            return Some((idx, ((t.top_row, t.left_col), (t.bottom_row, t.right_col))));
        }
        if let Some(value) = wb.named_ranges.get(&target.to_uppercase())
            && !value.eq_ignore_ascii_case(target)
        {
            return self.resolve_reference(value);
        }
        let (sheet, cells) = match target.rsplit_once('!') {
            Some((name, cells)) => {
                let name = name.trim_matches('\'');
                (wb.sheet_names.iter().position(|n| n.eq_ignore_ascii_case(name))?, cells)
            }
            None => (wb.active_sheet, target),
        };
        let cells = cells.replace('$', "").to_uppercase();
        let (a, b) = cells.split_once(':').unwrap_or((&cells, &cells));
        let (a, b) = (Spreadsheet::parse_cell_reference(a)?, Spreadsheet::parse_cell_reference(b)?);
        Some((sheet, ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))))
    }

    // ------------------------------------------- conditional formatting

    /// Replace the active sheet's conditional-format rules (one undo step).
    pub fn set_conditional_formats(&mut self, rules: Vec<ConditionalFormat>) {
        let sheet_idx = self.workbook.active_sheet;
        let old = self.workbook.current_sheet().conditional_formats.clone();
        if old == rules {
            return;
        }
        let sheet = self.workbook.current_sheet_mut();
        sheet.conditional_formats = rules.clone();
        sheet.cf_cache.lock().unwrap().clear();
        self.record_action(UndoAction::ConditionalFormatsReplaced { sheet_idx, old, new: rules });
    }

    /// Add one rule per column of the selection.
    pub fn add_conditional_format(&mut self, columns: std::ops::RangeInclusive<usize>, predicate: &str, style: CellStyle) {
        let mut rules = self.workbook.current_sheet().conditional_formats.clone();
        rules.extend(columns.map(|column| ConditionalFormat { column, predicate: predicate.to_string(), style: style.clone() }));
        self.set_conditional_formats(rules);
    }

    // -------------------------------------------------- data validation

    /// Set or clear the rule for what may be typed into `col`: a formula
    /// without `=` in which `_` stands for the entry, e.g. `_ > 0`.
    pub fn set_validation(&mut self, col: usize, predicate: Option<String>) {
        let changed = match predicate.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
            Some(p) => self.validations.insert(col, p.clone()) != Some(p),
            None => self.validations.remove(&col).is_some(),
        };
        if changed {
            self.dirty = true;
            crate::infrastructure::autosave::mark_dirty();
        }
    }

    /// The rule `input` breaks when typed into `(row, col)`, if any.
    /// Blank entries and formulas are always accepted.
    pub fn validation_error(&self, row: usize, col: usize, input: &str) -> Option<String> {
        let rule = self.validations.get(&col)?;
        let ok = input.trim().is_empty()
            || input.starts_with('=')
            || crate::domain::with_recalc_context(&self.workbook, || {
                self.workbook.current_sheet().predicate_holds_at(rule, input.trim(), Some(row))
            });
        (!ok).then(|| rule.clone())
    }

    /// The entries a list rule on `col` allows, for the in-cell dropdown:
    /// the values of `OR(_ = "a", _ = "b")`, or the cells of the range in
    /// `COUNTIF(range, _) > 0`. `None` for any other rule.
    pub fn validation_choices(&self, col: usize) -> Option<Vec<String>> {
        let rule = self.validations.get(&col)?.trim();
        if let Some(source) = rule.strip_prefix("COUNTIF(").and_then(|r| r.strip_suffix(", _) > 0")) {
            let (sheet, ((r0, c0), (r1, c1))) = self.resolve_reference(source)?;
            let sheet = &self.workbook.sheets[sheet];
            let (last_row, last_col) = sheet.last_cell();
            let mut seen = HashSet::new();
            return Some(
                (r0..=r1.min(last_row))
                    .flat_map(|r| (c0..=c1.min(last_col)).map(move |c| (r, c)))
                    .filter_map(|pos| sheet.cells.get(&pos).map(|cd| cd.value.clone()))
                    .filter(|v| !v.is_empty() && seen.insert(v.to_lowercase()))
                    .take(500)
                    .collect(),
            );
        }
        let items = match rule.strip_prefix("OR(").and_then(|r| r.strip_suffix(')')) {
            Some(args) => split_arguments(args),
            None => vec![rule],
        };
        items
            .into_iter()
            .map(|item| {
                let value = item.strip_prefix("_ = ")?;
                match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                    Some(text) => Some(text.replace("\"\"", "\"")),
                    None => value.parse::<f64>().ok().map(|_| value.to_string()),
                }
            })
            .collect()
    }

    // ------------------------------------------------------------ filter

    /// Hide the rows of `first..=last` whose value in `col` doesn't pass
    /// `criteria` (see [`filter_matches`]). Rows hidden before are shown.
    pub fn apply_filter_rows(&mut self, col: usize, criteria: &str, first: usize, last: usize) {
        let sheet = self.workbook.current_sheet();
        self.hidden_rows = (first..=last).filter(|&r| !filter_matches(&sheet.get_cell(r, col).value, criteria)).collect();
        let shown = (last + 1).saturating_sub(first) - self.hidden_rows.len();
        self.filter_column = Some(col);
        self.filter_value = Some(criteria.to_string());
        self.dirty = true;
        self.status_message = Some(format!("Filter: {} of {} row(s) shown", shown, (last + 1).saturating_sub(first)));
    }

    /// Distinct values of `col` in rows `first..=last`, sorted like a sort
    /// would (numbers, text, blank last).
    pub fn distinct_values(&self, col: usize, first: usize, last: usize) -> Vec<String> {
        let sheet = self.workbook.current_sheet();
        let mut seen = HashSet::new();
        let mut values: Vec<String> =
            (first..=last).map(|r| sheet.get_cell(r, col).value).filter(|v| seen.insert(v.to_lowercase())).collect();
        values.sort_by(|a, b| sort_cmp(a, b, true));
        values
    }

    // ------------------------------------------------------------- trace

    /// The ranges the cursor cell's formula reads, with the sheet for
    /// those on another sheet.
    pub fn direct_precedents(&self) -> Vec<(Option<String>, CellRange)> {
        let cell = self.workbook.current_sheet().get_cell(self.selected_row, self.selected_col);
        let active = &self.workbook.sheet_names[self.workbook.active_sheet];
        cell.formula
            .map(|f| crate::domain::formula_references(&f))
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.sheet.filter(|s| !s.eq_ignore_ascii_case(active)), r.range))
            .collect()
    }

    /// The cells whose formulas read the cursor cell, with the sheet for
    /// those on another sheet. Sorted.
    pub fn direct_dependents(&self) -> Vec<(Option<String>, (usize, usize))> {
        let wb = &self.workbook;
        let Some(&sheet_id) = wb.sheet_ids.get(wb.active_sheet) else { return Vec::new() };
        let node = (sheet_id, self.selected_row, self.selected_col);
        let mut cells: Vec<(Option<String>, (usize, usize))> = wb
            .graph
            .dependents
            .get(&node)
            .into_iter()
            .flatten()
            .filter(|d| **d != node)
            .map(|&(id, r, c)| ((id != sheet_id).then(|| wb.sheet_name_of(id).unwrap_or("?").to_string()), (r, c)))
            .collect();
        cells.sort();
        cells
    }

    // ------------------------------------------------------------ sheets

    /// Add a copy of the active sheet after it and show the copy.
    pub fn duplicate_sheet(&mut self) {
        self.snapshot_view_state_to_active_sheet();
        let index = self.workbook.active_sheet;
        let label = format!("Duplicate sheet '{}'", self.workbook.sheet_names[index]);
        self.with_snapshot_undo(&label, |app| {
            if let Some(copy) = app.workbook.duplicate_sheet(index) {
                app.workbook.active_sheet = copy;
            }
        });
        self.clear_selection();
        self.invalidate_cross_sheet_state();
        self.restore_view_state_from_active_sheet();
        self.status_message = Some(format!("Added '{}'", self.workbook.sheet_names[self.workbook.active_sheet]));
    }

    /// Move sheet `from` to position `to` in the tab order.
    pub fn move_sheet(&mut self, from: usize, to: usize) {
        let n = self.workbook.sheets.len();
        if from >= n || to >= n || from == to {
            return;
        }
        self.snapshot_view_state_to_active_sheet();
        let label = format!("Move sheet '{}'", self.workbook.sheet_names[from]);
        self.with_snapshot_undo(&label, |app| {
            app.workbook.move_sheet(from, to);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(cells: &[(usize, usize, &str)]) -> App {
        let mut app = App::default();
        for &(r, c, v) in cells {
            app.selected_row = r;
            app.selected_col = c;
            app.mode = AppMode::Editing;
            app.input = v.to_string();
            app.finish_editing();
        }
        app.selected_row = 0;
        app.selected_col = 0;
        app
    }

    fn column(app: &App, col: usize, rows: std::ops::Range<usize>) -> Vec<String> {
        rows.map(|r| app.workbook.current_sheet().get_cell(r, col).value).collect()
    }

    #[test]
    fn sort_keeps_the_header_and_moves_whole_rows() {
        let mut app = app_with(&[
            (0, 0, "Name"), (0, 1, "Qty"), (0, 2, "Double"),
            (1, 0, "pear"), (1, 1, "30"), (1, 2, "=B2*2"),
            (2, 0, "Apple"), (2, 1, "10"), (2, 2, "=B3*2"),
            (3, 0, "fig"), (3, 1, "20"), (3, 2, "=B4*2"),
        ]);
        app.selected_row = 2;
        app.selected_col = 1;
        assert!(app.guess_header(app.data_range()));
        app.sort_selection(true);
        assert_eq!(column(&app, 0, 0..4), ["Name", "Apple", "fig", "pear"]);
        assert_eq!(column(&app, 2, 1..4), ["20", "40", "60"], "formulas follow their row");
        assert_eq!(app.workbook.current_sheet().get_cell(1, 2).formula.as_deref(), Some("=B2*2"));

        app.undo();
        assert_eq!(column(&app, 0, 0..4), ["Name", "pear", "Apple", "fig"]);
    }

    #[test]
    fn sort_by_two_keys_puts_blanks_last_in_both_directions() {
        let mut app = app_with(&[
            (0, 0, "b"), (0, 1, "2"),
            (1, 0, "a"), (1, 1, "2"),
            (2, 0, "c"), (2, 1, "1"),
            (3, 0, "d"),
        ]);
        let range = ((0, 0), (3, 1));
        app.sort_range(range, false, &[(1, false), (0, true)]);
        assert_eq!(column(&app, 0, 0..4), ["a", "b", "c", "d"]);
        app.sort_range(range, false, &[(1, true)]);
        assert_eq!(column(&app, 0, 0..4), ["c", "a", "b", "d"]);
    }

    #[test]
    fn remove_duplicates_keeps_first_rows_and_closes_the_gaps() {
        let mut app = app_with(&[
            (0, 0, "Key"), (0, 1, "V"),
            (1, 0, "a"), (1, 1, "1"),
            (2, 0, "A"), (2, 1, "2"),
            (3, 0, "b"), (3, 1, "=10+1"),
            (4, 0, "a"), (4, 1, "4"),
        ]);
        let removed = app.remove_duplicates(((0, 0), (4, 1)), true, &[0]);
        assert_eq!(removed, 2);
        assert_eq!(column(&app, 0, 0..5), ["Key", "a", "b", "", ""]);
        assert_eq!(column(&app, 1, 0..3), ["V", "1", "11"]);
        assert!(!app.workbook.current_sheet().cells.contains_key(&(4, 0)));
        // Both columns must match when both are compared.
        app.undo();
        assert_eq!(app.remove_duplicates(((0, 0), (4, 1)), true, &[0, 1]), 0);
    }

    #[test]
    fn text_to_columns_splits_to_the_right() {
        let mut app = app_with(&[(0, 0, "a, b,c"), (1, 0, "single"), (2, 0, "John  Smith")]);
        assert_eq!(app.text_to_columns(((0, 0), (1, 0)), ","), 1);
        assert_eq!((0..3).map(|c| app.workbook.current_sheet().get_cell(0, c).value).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(app.text_to_columns(((2, 0), (2, 0)), " "), 1);
        assert_eq!(app.workbook.current_sheet().get_cell(2, 1).value, "Smith");
    }

    #[test]
    fn goal_seek_solves_rising_and_falling_formulas() {
        let mut app = app_with(&[(0, 0, "1"), (0, 1, "=A1*A1+A1"), (1, 0, "3"), (1, 1, "=100-4*A2")]);
        let x = app.goal_seek((0, 1), 30.0, (0, 0)).unwrap();
        assert!((x - 5.0).abs() < 1e-6, "{}", x);
        assert_eq!(app.workbook.current_sheet().get_cell(0, 1).value, "30");
        let x = app.goal_seek((1, 1), 0.0, (1, 0)).unwrap();
        assert_eq!(x, 25.0);
        assert!(app.goal_seek((0, 0), 1.0, (1, 0)).is_err(), "target needs a formula");
        app.undo();
        assert_eq!(app.workbook.current_sheet().get_cell(1, 0).value, "3");
    }

    #[test]
    fn paste_special_values_formats_and_transpose() {
        let mut app = app_with(&[(0, 0, "2"), (0, 1, "=A1*10"), (5, 0, "x")]);
        app.selected_row = 0;
        app.selected_col = 1;
        app.toggle_bold();
        app.selection_start = Some((0, 0));
        app.selection_end = Some((0, 1));
        app.copy_selection();
        app.clear_selection();

        app.selected_row = 2;
        app.selected_col = 0;
        app.paste_special(PasteMode::Values, false);
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.get_cell(2, 1).value, "20");
        assert_eq!(sheet.get_cell(2, 1).formula, None);
        assert_eq!(sheet.get_cell(2, 1).format, None, "values leave formatting alone");

        app.selected_row = 5;
        app.selected_col = 0;
        app.paste_special(PasteMode::Formats, true);
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.get_cell(5, 0).value, "x", "formats keep the content");
        assert!(sheet.get_cell(6, 0).format.unwrap().style.bold, "transposed: B1 lands one row down");

        app.selected_row = 8;
        app.selected_col = 0;
        app.paste_special(PasteMode::Formulas, false);
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.get_cell(8, 1).formula.as_deref(), Some("=A9*10"));
        assert_eq!(sheet.get_cell(8, 1).value, "20");
        assert_eq!(sheet.get_cell(8, 1).format, None);
    }

    #[test]
    fn format_painter_tiles_and_clear_formats_undoes_it() {
        let mut app = app_with(&[(0, 0, "1"), (3, 0, "a"), (4, 0, "b")]);
        app.toggle_bold();
        let block = app.copy_formats(((0, 0), (0, 0)));
        assert_eq!(app.apply_formats(&block, ((3, 0), (4, 1))), 4);
        let sheet = app.workbook.current_sheet();
        assert!(sheet.get_cell(4, 0).format.unwrap().style.bold);
        assert!(sheet.get_cell(4, 1).format.unwrap().style.bold, "empty cells are formatted too");

        app.selection_start = Some((3, 0));
        app.selection_end = Some((4, 1));
        app.clear_selection_formats();
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.get_cell(3, 0), CellData { value: "a".into(), ..CellData::default() });
        assert!(!sheet.cells.contains_key(&(4, 1)), "a cell left with nothing is removed");
    }

    #[test]
    fn tables_register_column_names_and_follow_inserted_rows() {
        let mut app = app_with(&[(1, 0, "Item"), (1, 1, "Price"), (2, 0, "a"), (2, 1, "5"), (3, 0, "b"), (3, 1, "7")]);
        app.add_table(((1, 0), (3, 1)), "Sales").unwrap();
        assert!(app.add_table(((3, 1), (5, 2)), "Other").is_err(), "overlap");
        assert!(app.add_table(((8, 0), (9, 1)), "sales").is_err(), "duplicate name");
        assert_eq!(app.workbook.named_ranges.get("SALES[PRICE]").map(String::as_str), Some("B3:B4"));
        app.selected_row = 0;
        app.selected_col = 3;
        app.mode = AppMode::Editing;
        app.input = "=SUM(Sales[Price])".to_string();
        app.finish_editing();
        assert_eq!(app.workbook.current_sheet().get_cell(0, 3).value, "12");

        app.insert_rows(0, 2);
        let t = app.workbook.current_sheet().tables[0].clone();
        assert_eq!((t.top_row, t.bottom_row), (3, 5));
        app.insert_cols(1, 1);
        let t = app.workbook.current_sheet().tables[0].clone();
        assert_eq!((t.left_col, t.right_col, t.headers.len()), (0, 2, 3));
        assert_eq!(app.workbook.named_ranges.get("SALES[PRICE]").map(String::as_str), Some("C5:C6"), "names follow the table");
        assert_eq!(app.workbook.current_sheet().get_cell(2, 4).value, "12", "the formula moved and still sums the column");

        assert_eq!(app.resolve_reference("sales"), Some((0, ((3, 0), (5, 2)))));
        assert!(app.remove_table("Sales"));
        assert!(app.workbook.named_ranges.is_empty());
        app.undo();
        assert_eq!(app.workbook.current_sheet().tables.len(), 1);
    }

    #[test]
    fn deleting_a_column_drops_its_conditional_format() {
        let mut app = app_with(&[(0, 0, "1"), (0, 1, "2"), (0, 2, "3")]);
        let style = CellStyle { bold: true, ..CellStyle::default() };
        app.add_conditional_format(1..=2, "_ > 1", style);
        assert!(app.workbook.current_sheet().conditional_style_for(0, 1).is_some());
        app.delete_cols(1, 1);
        let rules = &app.workbook.current_sheet().conditional_formats;
        assert_eq!(rules.iter().map(|r| r.column).collect::<Vec<_>>(), [1]);
        app.undo();
        app.undo();
        assert!(app.workbook.current_sheet().conditional_formats.is_empty());
    }

    #[test]
    fn conditional_format_compares_with_cells_on_this_and_other_sheets() {
        let mut app = app_with(&[(0, 0, "5"), (1, 0, "50"), (2, 2, "10")]);
        app.workbook.add_sheet("Limits".to_string());
        app.workbook.sheets[1].set_cell(0, 1, CellData { value: "40".into(), ..CellData::default() });
        let bold = CellStyle { bold: true, ..CellStyle::default() };
        app.add_conditional_format(0..=0, "_ < C3", bold.clone());
        app.add_conditional_format(0..=0, "_ > Limits!B1", CellStyle { underline: true, ..CellStyle::default() });
        let styles = |app: &App| {
            crate::domain::with_recalc_context(&app.workbook, || {
                let sheet = app.workbook.current_sheet();
                (sheet.display_style(0, 0), sheet.display_style(1, 0))
            })
        };
        let (small, big) = styles(&app);
        assert!(small.bold && !small.underline, "5 < C3");
        assert!(!big.bold && big.underline, "50 > Limits!B1");

        // Changing the compared cells changes the formatting.
        app.set_cell_with_undo(2, 2, CellData { value: "1".into(), ..CellData::default() });
        assert!(!styles(&app).0.bold);
        app.workbook.active_sheet = 1;
        app.set_cell_with_undo(0, 1, CellData { value: "99".into(), ..CellData::default() });
        app.workbook.active_sheet = 0;
        assert!(!styles(&app).1.underline, "the other sheet's edit is seen");
    }

    #[test]
    fn names_resolve_for_go_to_and_reject_cell_like_names() {
        let mut app = app_with(&[(0, 0, "1")]);
        app.selection_start = Some((1, 1));
        app.selection_end = Some((3, 2));
        let reference = app.selection_reference();
        assert_eq!(reference, "Sheet1!B2:C4");
        app.define_name("Rates", &reference).unwrap();
        assert_eq!(app.resolve_reference("rates"), Some((0, ((1, 1), (3, 2)))));
        assert_eq!(app.resolve_reference("$c$2"), Some((0, ((1, 2), (1, 2)))));
        assert_eq!(app.resolve_reference("Nope!A1"), None);
        assert!(app.define_name("A1", "B2").is_err());
        assert!(app.define_name("2x", "B2").is_err());
        assert_eq!(app.user_names(), [("RATES".to_string(), "Sheet1!B2:C4".to_string())]);
        assert!(app.delete_name("rates"));
        assert!(app.user_names().is_empty());
    }

    #[test]
    fn validation_checks_typed_values_only() {
        let mut app = App::default();
        app.set_validation(1, Some("AND(_ >= 0, _ <= 10)".to_string()));
        assert!(app.dirty);
        assert_eq!(app.validation_error(0, 1, "5"), None);
        assert_eq!(app.validation_error(0, 1, "11").as_deref(), Some("AND(_ >= 0, _ <= 10)"));
        assert_eq!(app.validation_error(0, 1, ""), None);
        assert_eq!(app.validation_error(0, 1, "=99"), None);
        assert_eq!(app.validation_error(0, 0, "11"), None);
        app.set_validation(1, None);
        assert_eq!(app.validation_error(0, 1, "11"), None);
    }

    #[test]
    fn validation_reads_limits_and_lists_from_cells() {
        let mut app = app_with(&[(0, 5, "Open"), (1, 5, "Closed"), (2, 5, "open"), (0, 6, "100"), (1, 6, "200")]);
        app.workbook.add_sheet("Lists".to_string());
        app.workbook.sheets[1].set_cell(0, 0, CellData { value: "Red".into(), ..CellData::default() });
        // A limit in a cell, and one in the entry's own row.
        app.set_validation(0, Some("_ <= G1".to_string()));
        assert_eq!(app.validation_error(0, 0, "150").as_deref(), Some("_ <= G1"));
        app.set_validation(0, Some("_ <= INDIRECT(\"G\" & ROW())".to_string()));
        assert_eq!(app.validation_error(1, 0, "150"), None, "row 2 allows up to G2");
        assert!(app.validation_error(0, 0, "150").is_some());

        app.set_validation(1, Some("COUNTIF(F1:F9, _) > 0".to_string()));
        assert_eq!(app.validation_choices(1), Some(vec!["Open".to_string(), "Closed".to_string()]));
        assert_eq!(app.validation_error(0, 1, "closed"), None);
        assert!(app.validation_error(0, 1, "Pending").is_some());
        app.set_validation(1, Some("COUNTIF(Lists!A1:A5, _) > 0".to_string()));
        assert_eq!(app.validation_choices(1), Some(vec!["Red".to_string()]));
        assert_eq!(app.validation_error(0, 1, "Red"), None, "a list on another sheet");

        app.set_validation(2, Some("OR(_ = \"a, b\", _ = 3, _ = \"say \"\"hi\"\"\")".to_string()));
        assert_eq!(app.validation_choices(2), Some(vec!["a, b".to_string(), "3".to_string(), "say \"hi\"".to_string()]));
        app.set_validation(2, Some("_ = \"only\"".to_string()));
        assert_eq!(app.validation_choices(2), Some(vec!["only".to_string()]));
        assert_eq!(app.validation_choices(0), None, "not a list rule");
    }

    #[test]
    fn filter_criteria_compare_like_sumif() {
        for (value, criteria, expected) in [
            ("12", ">10", true),
            ("9", ">10", false),
            ("10", ">=10", true),
            ("9.5", "<10", true),
            ("abc", ">10", false),
            ("", "<10", false),
            ("pear", ">fig", true),
            ("Apple", "<>apple", false),
            ("pear", "<>apple", true),
            ("10.0", "=10", true),
            ("Apple pie", "apple", true),
            ("Apple pie", "=apple", false),
        ] {
            assert_eq!(filter_matches(value, criteria), expected, "{:?} {:?}", value, criteria);
        }
    }

    #[test]
    fn a_table_name_stands_for_its_data_rows() {
        let mut app = app_with(&[(0, 0, "Item"), (0, 1, "Unit Price"), (1, 0, "a"), (1, 1, "5"), (2, 0, "b"), (2, 1, "7"), (5, 5, "=SUM(Sales)")]);
        app.add_table(((0, 0), (2, 1)), "Sales").unwrap();
        assert_eq!(app.workbook.current_sheet().get_cell(5, 5).value, "12");
        app.selected_row = 6;
        app.selected_col = 5;
        app.mode = AppMode::Editing;
        app.input = "=SUM(Sales[Unit Price])".to_string();
        app.finish_editing();
        assert_eq!(app.workbook.current_sheet().get_cell(6, 5).value, "12");
        assert!(app.user_names().is_empty(), "table names are not listed as defined names");
        assert!(app.define_name("sales", "A1").is_err());
        assert!(app.add_table(((8, 0), (8, 1)), "Flat").is_err(), "a header alone is not a table");

        app.update_table("Sales", "Orders", ((0, 0), (2, 1))).unwrap();
        assert!(!app.workbook.named_ranges.contains_key("SALES"), "the old name is gone");
        assert_eq!(app.workbook.named_ranges.get("ORDERS").map(String::as_str), Some("A2:B3"));
        app.remove_table("Orders");
        assert!(app.workbook.named_ranges.is_empty());
    }

    #[test]
    fn value_filter_keeps_the_header_row() {
        let mut app = app_with(&[(0, 0, "Fruit"), (1, 0, "Apple"), (2, 0, "Pineapple"), (3, 0, "apple")]);
        assert_eq!(app.distinct_values(0, 1, 3), ["Apple", "Pineapple"]);
        app.apply_filter_rows(0, "=apple", 1, 3);
        assert_eq!(app.hidden_rows, HashSet::from([2]));
        app.apply_filter_rows(0, "apple", 1, 3);
        assert!(app.hidden_rows.is_empty());
        assert_eq!(app.filter_value.as_deref(), Some("apple"));
    }

    #[test]
    fn trace_lists_direct_precedents_and_dependents() {
        let mut app = app_with(&[(0, 0, "1"), (1, 0, "=A1+1"), (2, 0, "=A2+SUM(A1:A2)")]);
        assert_eq!(app.direct_dependents(), [(None, (1, 0)), (None, (2, 0))]);
        app.selected_row = 2;
        assert_eq!(app.direct_precedents(), [(None, ((1, 0), (1, 0))), (None, ((0, 0), (1, 0)))]);
    }

    #[test]
    fn duplicate_and_move_sheet() {
        let mut app = app_with(&[(0, 0, "7"), (0, 1, "=A1*2")]);
        app.frozen_rows = 1;
        app.duplicate_sheet();
        assert_eq!(app.workbook.sheet_names, ["Sheet1", "Sheet1 (2)"]);
        assert_eq!(app.workbook.active_sheet, 1);
        assert_eq!(app.frozen_rows, 1, "the copy keeps the view");
        app.set_cell_with_undo(0, 0, CellData { value: "10".into(), ..CellData::default() });
        assert_eq!(app.workbook.current_sheet().get_cell(0, 1).value, "20", "the copy calculates on its own");
        assert_eq!(app.workbook.sheets[0].get_cell(0, 1).value, "14");

        app.move_sheet(1, 0);
        assert_eq!(app.workbook.sheet_names, ["Sheet1 (2)", "Sheet1"]);
        assert_eq!(app.workbook.active_sheet, 0, "still showing the copy");
        app.set_cell_with_undo(0, 0, CellData { value: "3".into(), ..CellData::default() });
        assert_eq!(app.workbook.current_sheet().get_cell(0, 1).value, "6");
        app.undo();
        app.undo();
        assert_eq!(app.workbook.sheet_names, ["Sheet1", "Sheet1 (2)"]);
    }
}
