//! Submodule of `state` — see state/mod.rs.
//!
//! Excel's fill handle: extend a source range down, up, right or left.
//! Formulas are copied with their relative references shifted (several
//! source formulas repeat in turn), values continue a detected series
//! (1, 2 → 3, 4; Jan → Feb; Item1 → Item2) or are copied, and formats come
//! along. Dragging back into the source clears the cells left behind.

use super::*;
use crate::domain::services::{AutofillPattern, FormulaEvaluator};

/// Excel's Auto Fill Options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    /// "Copy Cells": repeat the source (formulas still shift).
    Copy,
    /// "Fill Series": continue number/date/text sequences.
    Series,
    /// "Fill Formatting Only".
    FormatsOnly,
    /// "Fill Without Formatting": like Series but keep the targets' formats.
    WithoutFormatting,
}

impl FillMode {
    pub const ALL: [FillMode; 4] = [Self::Copy, Self::Series, Self::FormatsOnly, Self::WithoutFormatting];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Copy => "Copy Cells",
            Self::Series => "Fill Series",
            Self::FormatsOnly => "Fill Formatting Only",
            Self::WithoutFormatting => "Fill Without Formatting",
        }
    }
}

pub type CellRange = ((usize, usize), (usize, usize));

/// What a fill would write: new cell contents, or `None` to clear.
#[derive(Debug, Default)]
pub struct FillPlan {
    pub cells: Vec<(usize, usize, Option<CellData>)>,
    /// Targets skipped because the shifted formula would be circular.
    pub skipped: usize,
}

/// The cells added when the fill handle of `source` is dragged to `to`.
/// Excel extends along the axis where the pointer is further outside the
/// source. Returns `None` while the pointer is still inside the source and
/// below/right of its last row/column (nothing to do).
pub fn fill_target(source: CellRange, to: (usize, usize)) -> Option<FillTarget> {
    let ((r0, c0), (r1, c1)) = source;
    let (r, c) = to;
    let down = r.saturating_sub(r1);
    let up = r0.saturating_sub(r);
    let right = c.saturating_sub(c1);
    let left = c0.saturating_sub(c);
    let vertical = down.max(up);
    let horizontal = right.max(left);
    if vertical == 0 && horizontal == 0 {
        // Inside the source: dragging back up/left shrinks it (clears).
        if r < r1 && (r1 - r) >= (c1 - c.min(c1)) && r >= r0 {
            return Some(FillTarget::Clear(((r + 1, c0), (r1, c1))));
        }
        if c < c1 && c >= c0 {
            return Some(FillTarget::Clear(((r0, c + 1), (r1, c1))));
        }
        return None;
    }
    Some(if vertical >= horizontal {
        if down > 0 { FillTarget::Fill(((r1 + 1, c0), (r, c1))) } else { FillTarget::Fill(((r, c0), (r0 - 1, c1))) }
    } else if right > 0 {
        FillTarget::Fill(((r0, c1 + 1), (r1, c)))
    } else {
        FillTarget::Fill(((r0, c), (r1, c0 - 1)))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillTarget {
    /// Cells to fill from the source.
    Fill(CellRange),
    /// Part of the source to clear (handle dragged back inside).
    Clear(CellRange),
}

impl App {
    /// Excel's default when dragging: a single number is copied (Ctrl
    /// switches to a series); everything else fills a series.
    pub fn default_fill_mode(&self, source: CellRange) -> FillMode {
        let ((r0, c0), (r1, c1)) = source;
        let single_number = r0 == r1 && c0 == c1 && {
            let cd = self.workbook.current_sheet().get_cell(r0, c0);
            cd.formula.is_none() && cd.value.trim().parse::<f64>().is_ok()
        };
        if single_number { FillMode::Copy } else { FillMode::Series }
    }

    /// Work out what filling `target` from `source` would write, without
    /// changing anything (the GUI previews the last value while dragging).
    pub fn plan_fill(&self, source: CellRange, target: CellRange, mode: FillMode) -> FillPlan {
        let ((sr0, sc0), (sr1, sc1)) = source;
        let ((tr0, tc0), (tr1, tc1)) = target;
        let sheet = self.workbook.current_sheet();
        let evaluator = FormulaEvaluator::new(sheet);
        let vertical = tc0 >= sc0 && tc1 <= sc1;
        let forward = if vertical { tr0 > sr1 } else { tc0 > sc1 };
        let mut plan = FillPlan::default();

        // One "line" per column (vertical fill) or row (horizontal fill).
        let lines: Vec<usize> = if vertical { (sc0..=sc1).collect() } else { (sr0..=sr1).collect() };
        for line in lines {
            let src_pos: Vec<(usize, usize)> = if vertical {
                (sr0..=sr1).map(|r| (r, line)).collect()
            } else {
                (sc0..=sc1).map(|c| (line, c)).collect()
            };
            // Targets nearest the source first.
            let mut tgt_pos: Vec<(usize, usize)> = if vertical {
                (tr0..=tr1).map(|r| (r, line)).collect()
            } else {
                (tc0..=tc1).map(|c| (line, c)).collect()
            };
            if !forward {
                tgt_pos.reverse();
            }
            let srcs: Vec<CellData> = src_pos.iter().map(|&(r, c)| sheet.get_cell(r, c)).collect();
            let n = srcs.len();
            // A series only when every source cell is a plain, non-empty value.
            let pattern = (mode != FillMode::Copy
                && srcs.iter().all(|c| c.formula.is_none() && c.spill_anchor.is_none() && !c.value.is_empty()))
            .then(|| AutofillPattern::detect_series(&srcs.iter().map(|c| c.value.clone()).collect::<Vec<_>>()))
            .filter(AutofillPattern::is_series);

            for (k, &(tr, tc)) in tgt_pos.iter().enumerate() {
                let i = if forward { k % n } else { n - 1 - (k % n) };
                let src = &srcs[i];
                let existing = sheet.cells.get(&(tr, tc)).cloned();
                let new_cell = match mode {
                    FillMode::FormatsOnly => {
                        let mut cell = existing.clone().unwrap_or_default();
                        cell.format = src.format.clone();
                        (cell.format.is_some() || !cell.value.is_empty() || cell.formula.is_some()).then_some(cell)
                    }
                    _ => {
                        let mut cell = if let (Some(f), None) = (&src.formula, src.spill_anchor) {
                            let (sr, sc) = src_pos[i];
                            let adjusted = evaluator.adjust_formula_references(f, tr as i32 - sr as i32, tc as i32 - sc as i32);
                            if evaluator.would_create_circular_reference(&adjusted, (tr, tc)) {
                                plan.skipped += 1;
                                continue;
                            }
                            // Same clock rationale as the autofill/paste paths.
                            let value = crate::domain::parser::with_recalc_clock(
                                crate::domain::parser::now_serial(),
                                || evaluator.evaluate_formula(&adjusted),
                            );
                            CellData { value, formula: Some(adjusted), ..CellData::default() }
                        } else if let Some(p) = &pattern {
                            let idx = if forward { (n + k) as i64 } else { -(k as i64) - 1 };
                            CellData { value: p.generate_at(idx), ..CellData::default() }
                        } else {
                            CellData { value: src.value.clone(), ..CellData::default() }
                        };
                        cell.format = if mode == FillMode::WithoutFormatting {
                            existing.as_ref().and_then(|e| e.format.clone())
                        } else {
                            src.format.clone()
                        };
                        cell.comment = existing.as_ref().and_then(|e| e.comment.clone());
                        let empty = cell.value.is_empty() && cell.formula.is_none() && cell.format.is_none() && cell.comment.is_none();
                        (!empty).then_some(cell)
                    }
                };
                if new_cell.is_none() && existing.is_none() {
                    continue;
                }
                plan.cells.push((tr, tc, new_cell));
            }
        }
        plan
    }

    /// Fill `target` from `source` as one undo step. Returns the number of
    /// cells written.
    pub fn fill(&mut self, source: CellRange, target: CellRange, mode: FillMode) -> usize {
        let plan = self.plan_fill(source, target, mode);
        let n = self.apply_plan(plan.cells);
        self.status_message = Some(match (n, plan.skipped) {
            (0, 0) => "Nothing to fill".to_string(),
            (n, 0) => format!("Filled {} cell(s) ({})", n, mode.label()),
            (n, s) => format!("Filled {} cell(s) ({}); {} skipped: would create a circular reference", n, mode.label(), s),
        });
        n
    }

    /// Clear `range` (fill handle dragged back inside the selection).
    pub fn clear_range(&mut self, range: CellRange) {
        let ((r0, c0), (r1, c1)) = range;
        let cells = (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c, None))).collect();
        self.apply_plan(cells);
    }

    /// Excel's Ctrl+D / Ctrl+R: copy the selection's first row down (or
    /// first column right) through the rest of the selection.
    pub fn fill_down_or_right(&mut self, down: bool) {
        let Some(((r0, c0), (r1, c1))) = self.get_selection_range() else {
            // Single cell: Excel copies from the cell above / to the left.
            let (r, c) = (self.selected_row, self.selected_col);
            if down && r > 0 {
                self.fill(((r - 1, c), (r - 1, c)), ((r, c), (r, c)), FillMode::Copy);
            } else if !down && c > 0 {
                self.fill(((r, c - 1), (r, c - 1)), ((r, c), (r, c)), FillMode::Copy);
            }
            return;
        };
        if down && r1 > r0 {
            self.fill(((r0, c0), (r0, c1)), ((r0 + 1, c0), (r1, c1)), FillMode::Copy);
        } else if !down && c1 > c0 {
            self.fill(((r0, c0), (r1, c0)), ((r0, c0 + 1), (r1, c1)), FillMode::Copy);
        }
    }

    fn apply_plan(&mut self, cells: Vec<(usize, usize, Option<CellData>)>) -> usize {
        let mut batch = Vec::new();
        let mut writes = Vec::new();
        let mut clears = Vec::new();
        for (r, c, new) in cells {
            let old = self.workbook.current_sheet().cells.get(&(r, c)).cloned();
            if old == new {
                continue;
            }
            batch.push(UndoAction::CellModified { row: r, col: c, old_cell: old, new_cell: new.clone() });
            match new {
                Some(cell) => writes.push((r, c, cell)),
                None => clears.push((r, c)),
            }
        }
        let n = batch.len();
        if !writes.is_empty() {
            self.workbook.write_cells_on_active(writes);
        }
        if !clears.is_empty() {
            self.workbook.clear_cells_on_active(clears);
        }
        if !batch.is_empty() {
            self.record_action(UndoAction::Batch(batch));
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(cells: &[(usize, usize, &str)]) -> App {
        let mut app = App::default();
        for &(r, c, v) in cells {
            let cd = if v.starts_with('=') {
                CellData { formula: Some(v.into()), ..CellData::default() }
            } else {
                CellData { value: v.into(), ..CellData::default() }
            };
            app.set_cell_with_undo(r, c, cd);
        }
        app.recalc_all();
        app
    }

    fn val(app: &App, r: usize, c: usize) -> String {
        app.workbook.current_sheet().get_cell(r, c).value
    }

    fn formula(app: &App, r: usize, c: usize) -> Option<String> {
        app.workbook.current_sheet().get_cell(r, c).formula
    }

    #[test]
    fn target_follows_the_dominant_direction() {
        let src = ((2, 2), (3, 3));
        assert_eq!(fill_target(src, (8, 4)), Some(FillTarget::Fill(((4, 2), (8, 3)))));
        assert_eq!(fill_target(src, (0, 3)), Some(FillTarget::Fill(((0, 2), (1, 3)))));
        assert_eq!(fill_target(src, (3, 6)), Some(FillTarget::Fill(((2, 4), (3, 6)))));
        assert_eq!(fill_target(src, (2, 0)), Some(FillTarget::Fill(((2, 0), (3, 1)))));
        assert_eq!(fill_target(src, (3, 3)), None);
        assert_eq!(fill_target(src, (2, 3)), Some(FillTarget::Clear(((3, 2), (3, 3)))));
    }

    #[test]
    fn formulas_shift_and_overwrite_targets() {
        let mut app = app_with(&[(0, 0, "1"), (1, 0, "2"), (2, 0, "3"), (0, 1, "=A1*10"), (1, 1, "old")]);
        app.fill(((0, 1), (0, 1)), ((1, 1), (2, 1)), FillMode::Series);
        assert_eq!(formula(&app, 1, 1).as_deref(), Some("=A2*10"));
        assert_eq!(formula(&app, 2, 1).as_deref(), Some("=A3*10"));
        assert_eq!(val(&app, 2, 1), "30");
        // One undo step restores everything.
        app.undo();
        assert_eq!(val(&app, 1, 1), "old");
        assert_eq!(formula(&app, 2, 1), None);
    }

    #[test]
    fn series_and_copy_modes() {
        let mut app = app_with(&[(0, 0, "1"), (1, 0, "3"), (0, 1, "Jan"), (0, 2, "7")]);
        app.fill(((0, 0), (1, 0)), ((2, 0), (3, 0)), FillMode::Series);
        assert_eq!((val(&app, 2, 0), val(&app, 3, 0)), ("5".into(), "7".into()));
        app.fill(((0, 1), (0, 1)), ((1, 1), (2, 1)), FillMode::Series);
        assert_eq!(val(&app, 2, 1), "Mar");
        // A single number copies by default, counts with Fill Series.
        assert_eq!(app.default_fill_mode(((0, 2), (0, 2))), FillMode::Copy);
        app.fill(((0, 2), (0, 2)), ((1, 2), (1, 2)), FillMode::Copy);
        assert_eq!(val(&app, 1, 2), "7");
        app.fill(((0, 2), (0, 2)), ((1, 2), (2, 2)), FillMode::Series);
        assert_eq!(val(&app, 2, 2), "9");
    }

    #[test]
    fn filling_up_continues_backwards() {
        let mut app = app_with(&[(5, 0, "10"), (6, 0, "20")]);
        app.fill(((5, 0), (6, 0)), ((3, 0), (4, 0)), FillMode::Series);
        assert_eq!((val(&app, 3, 0), val(&app, 4, 0)), ("-10".into(), "0".into()));
    }

    #[test]
    fn multiple_formulas_repeat_in_turn_and_formats_copy() {
        let mut app = app_with(&[(0, 0, "=1+1"), (1, 0, "=2*2")]);
        let bold = crate::domain::CellFormat {
            style: crate::domain::CellStyle { bold: true, ..Default::default() },
            ..Default::default()
        };
        app.workbook.current_sheet_mut().cells.get_mut(&(1, 0)).unwrap().format = Some(bold.clone());
        app.fill(((0, 0), (1, 0)), ((2, 0), (4, 0)), FillMode::Series);
        assert_eq!(formula(&app, 2, 0).as_deref(), Some("=1+1"));
        assert_eq!(formula(&app, 3, 0).as_deref(), Some("=2*2"));
        assert_eq!(app.workbook.current_sheet().get_cell(3, 0).format, Some(bold));
        app.fill(((0, 0), (1, 0)), ((2, 0), (2, 0)), FillMode::WithoutFormatting);
        assert_eq!(app.workbook.current_sheet().get_cell(2, 0).format, None);
    }

    #[test]
    fn ctrl_d_copies_the_first_row_down() {
        let mut app = app_with(&[(0, 0, "5"), (0, 1, "=A1+1")]);
        app.selection_start = Some((0, 0));
        app.selection_end = Some((2, 1));
        app.fill_down_or_right(true);
        assert_eq!(val(&app, 2, 0), "5");
        assert_eq!(formula(&app, 2, 1).as_deref(), Some("=A3+1"));
    }

    #[test]
    fn dragging_back_clears() {
        let mut app = app_with(&[(0, 0, "1"), (1, 0, "2"), (2, 0, "3")]);
        app.clear_range(((1, 0), (2, 0)));
        assert_eq!(val(&app, 1, 0), "");
        assert!(!app.workbook.current_sheet().cells.contains_key(&(2, 0)));
    }
}
