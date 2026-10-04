//! Submodule of `state` — see state/mod.rs.

use super::*;

impl App {
    /// Stored cells inside `range`, row-major. Sheets are sparse; walking
    /// the rectangle instead would be a billion lookups for Select All on
    /// a large sheet.
    fn stored_cells_in(&self, ((r0, c0), (r1, c1)): ((usize, usize), (usize, usize))) -> Vec<(usize, usize)> {
        let mut keys: Vec<(usize, usize)> = self
            .workbook
            .current_sheet()
            .cells
            .keys()
            .filter(|&&(r, c)| (r0..=r1).contains(&r) && (c0..=c1).contains(&c))
            .copied()
            .collect();
        keys.sort_unstable();
        keys
    }

    /// The selection's copyable cells (relative positions) and its text as
    /// TSV, which stops at the last row/column holding a value.
    fn collect_for_clipboard(
        &self,
        range: ((usize, usize), (usize, usize)),
    ) -> (Vec<(usize, usize, CellData)>, String) {
        let ((start_row, start_col), _) = range;
        let sheet = self.workbook.current_sheet();
        let mut cells = Vec::new();
        let (mut last_row, mut last_col) = (start_row, start_col);
        for (row, col) in self.stored_cells_in(range) {
            let cell = sheet.get_cell(row, col);
            if !cell.value.is_empty() {
                last_row = last_row.max(row);
                last_col = last_col.max(col);
            }
            // Skip spill ghosts: their value is derived from an anchor
            // formula and they have no formula of their own, so pasting
            // them as-is would produce inert duplicates of the anchor's
            // top-left value. The anchor (when included in the range)
            // carries the formula and will re-spill at the destination.
            if cell.spill_anchor.is_some() {
                continue;
            }
            if !cell.value.is_empty() || cell.formula.is_some() {
                cells.push((row - start_row, col - start_col, cell));
            }
        }

        // Sentinel-prefixed TSV for system clipboard.
        let mut tsv = String::from(crate::infrastructure::sidecar::SENTINEL);
        for row in start_row..=last_row {
            for col in start_col..=last_col {
                if col > start_col {
                    tsv.push('\t');
                }
                if let Some(cell) = sheet.cells.get(&(row, col)) {
                    tsv.push_str(&cell.value);
                }
            }
            tsv.push('\n');
        }
        (cells, tsv)
    }

    pub fn copy_selection(&mut self) {
        let range = if let Some(range) = self.get_selection_range() {
            range
        } else {
            ((self.selected_row, self.selected_col), (self.selected_row, self.selected_col))
        };
        let ((start_row, start_col), (end_row, end_col)) = range;
        let (cells, tsv) = self.collect_for_clipboard(range);
        let count = (end_row - start_row + 1) * (end_col - start_col + 1);
        if let Ok(mut board) = arboard::Clipboard::new() {
            let _ = board.set_text(tsv);
        }
        // Sidecar JSON for formula round-trip. Skipped in tests for the same
        // state-isolation reason as the read path.
        if !cfg!(test) {
            crate::infrastructure::sidecar::write(cells.clone(), start_row, start_col);
        }

        self.clipboard = Some(ClipboardData {
            cells,
            source_row: start_row,
            source_col: start_col,
        });
        self.status_message = Some(format!("Copied {} cell(s)", count));
    }

    pub fn cut_selection(&mut self) {
        let range = if let Some(range) = self.get_selection_range() {
            range
        } else {
            ((self.selected_row, self.selected_col), (self.selected_row, self.selected_col))
        };
        let ((start_row, start_col), _) = range;
        let (cells, tsv) = self.collect_for_clipboard(range);
        let count = cells.len();

        // Symmetric with copy_selection: also push the cut region to the
        // system clipboard so an external paste after `dd`/`x` gets the
        // cut contents instead of stale data from the previous copy.
        if let Ok(mut board) = arboard::Clipboard::new() {
            let _ = board.set_text(tsv);
        }
        if !cfg!(test) {
            crate::infrastructure::sidecar::write(cells.clone(), start_row, start_col);
        }

        self.clipboard = Some(ClipboardData {
            cells,
            source_row: start_row,
            source_col: start_col,
        });
        self.clear_range_with_undo(range);
        self.status_message = Some(format!("Cut {} cell(s)", count));
    }

    /// Clear every cell in the selection (or the cursor cell when nothing
    /// is selected) as a single undo step. Formats and comments go too,
    /// matching `cut_selection`. Used by the GUI's Delete key.
    pub fn clear_selection_contents(&mut self) {
        let range = self.get_selection_range().unwrap_or((
            (self.selected_row, self.selected_col),
            (self.selected_row, self.selected_col),
        ));
        self.clear_range_with_undo(range);
    }

    fn clear_range_with_undo(&mut self, range: ((usize, usize), (usize, usize))) {
        // Clear the cells and notify cross-sheet listeners so any
        // formula on another sheet that referenced these now goes stale.
        // Route the clears through `clear_cells_on_active` so the dirty
        // set is populated (single workbook call, one cross-sheet pass).
        let positions = self.stored_cells_in(range);
        let batch: Vec<UndoAction> = positions
            .iter()
            .map(|&(row, col)| UndoAction::CellModified {
                row,
                col,
                old_cell: Some(self.workbook.current_sheet().get_cell(row, col)),
                new_cell: None,
            })
            .collect();
        if !positions.is_empty() {
            self.workbook.clear_cells_on_active(positions);
        }
        if !batch.is_empty() {
            self.record_action(UndoAction::Batch(batch));
        }
    }

    pub fn paste(&mut self) {
        // If the system clipboard has our sentinel header, load the matching
        // sidecar JSON (preserves formulas/formats/comments). Otherwise fall
        // back to internal clipboard, then plain-text TSV.
        // Skipped in tests because $HOME and ~/.cache/Cellar persist between
        // test runs and would otherwise leak state between cases.
        if !cfg!(test)
            && let Ok(mut board) = arboard::Clipboard::new()
                && let Ok(text) = board.get_text()
                    && crate::infrastructure::sidecar::strip_sentinel(&text).is_some()
                        && let Some(payload) = crate::infrastructure::sidecar::read() {
                            let cb = ClipboardData {
                                cells: payload.cells,
                                source_row: payload.source_row,
                                source_col: payload.source_col,
                                                };
                            self.clipboard = Some(cb);
                        }
        let clipboard = if let Some(ref cb) = self.clipboard {
            cb.clone()
        } else {
            // Tests don't touch the system clipboard (cross-test contamination).
            if !cfg!(test)
                && let Ok(mut board) = arboard::Clipboard::new()
                    && let Ok(text) = board.get_text()
                        && !text.is_empty() {
                            let body = crate::infrastructure::sidecar::strip_sentinel(&text)
                                .unwrap_or(&text);
                            self.paste_tsv(body);
                            return;
                        }
            self.status_message = Some("Nothing to paste".to_string());
            return;
        };

        let dest_row = self.selected_row;
        let dest_col = self.selected_col;
        // Grow the sheet to fit the block; only cells past Excel's grid are
        // left out (the sheet's current size used to clip the paste).
        let (max_row_off, max_col_off) = clipboard
            .cells
            .iter()
            .fold((0, 0), |(mr, mc), (r, c, _)| (mr.max(*r), mc.max(*c)));
        self.workbook
            .current_sheet_mut()
            .grow_to_fit(dest_row + max_row_off, dest_col + max_col_off);

        // Compute all new cells first (evaluator borrows spreadsheet immutably).
        // One clock snapshot for the whole paste so any pasted =NOW() cells
        // agree on time, and so they match what the auto-recalc immediately
        // following will use.
        let new_cells: Vec<_> = crate::domain::parser::with_recalc_clock(
            crate::domain::parser::now_serial(),
            || {
                let evaluator = crate::domain::FormulaEvaluator::new(self.workbook.current_sheet());
                clipboard.cells.iter().filter_map(|(row_off, col_off, cell)| {
                    let target_row = dest_row + row_off;
                    let target_col = dest_col + col_off;
                    if target_row >= self.workbook.current_sheet().rows || target_col >= self.workbook.current_sheet().cols {
                        return None;
                    }
                    let new_cell = if let Some(ref formula) = cell.formula {
                        let row_offset = target_row as i32 - (clipboard.source_row + row_off) as i32;
                        let col_offset = target_col as i32 - (clipboard.source_col + col_off) as i32;
                        let adjusted = evaluator.adjust_formula_references(formula, row_offset, col_offset);
                        let value = evaluator.evaluate_formula(&adjusted);
                        CellData { value, formula: Some(adjusted), format: cell.format.clone(), comment: cell.comment.clone(), spill_anchor: None }
                    } else {
                        cell.clone()
                    };
                    Some((target_row, target_col, new_cell))
                }).collect()
            },
        );

        // Now apply changes (mutably borrows spreadsheet) — collect the
        // undo batch, then push all writes through `set_many` in one shot
        // so dependents recalc just once.
        let mut batch = Vec::new();
        let mut writes: Vec<(usize, usize, CellData)> = Vec::with_capacity(new_cells.len());
        for (target_row, target_col, new_cell) in &new_cells {
            let old = if self
                .workbook
                .current_sheet()
                .cells
                .contains_key(&(*target_row, *target_col))
            {
                Some(self.workbook.current_sheet().get_cell(*target_row, *target_col))
            } else {
                None
            };
            batch.push(UndoAction::CellModified {
                row: *target_row,
                col: *target_col,
                old_cell: old,
                new_cell: Some(new_cell.clone()),
            });
            writes.push((*target_row, *target_col, new_cell.clone()));
        }
        // Single workbook API call handles same-sheet recalc + cross-sheet
        // propagation for the whole paste batch.
        self.workbook.write_cells_on_active(writes);
        if !batch.is_empty() {
            self.record_action(UndoAction::Batch(batch));
        }
        self.status_message = Some(paste_status("Pasted", new_cells.len(), clipboard.cells.len() - new_cells.len()));
    }

    fn paste_tsv(&mut self, text: &str) {
        let dest_row = self.selected_row;
        let dest_col = self.selected_col;
        // Same as `paste`: grow to fit, skip only what lies past Excel's grid.
        let (mut last_row, mut last_col) = (dest_row, dest_col);
        for (row_offset, line) in text.lines().enumerate().filter(|(_, l)| !l.is_empty()) {
            let n = line.split('\t').count();
            last_row = last_row.max(dest_row + row_offset);
            last_col = last_col.max(dest_col + n - 1);
        }
        self.workbook.current_sheet_mut().grow_to_fit(last_row, last_col);
        let mut outside = 0usize;
        let mut batch = Vec::new();
        let mut writes: Vec<(usize, usize, CellData)> = Vec::new();

        for (row_offset, line) in text.lines().enumerate() {
            if line.is_empty() { continue; }
            for (col_offset, value) in line.split('\t').enumerate() {
                let target_row = dest_row + row_offset;
                let target_col = dest_col + col_offset;
                if target_row >= self.workbook.current_sheet().rows || target_col >= self.workbook.current_sheet().cols {
                    outside += 1;
                    continue;
                }
                let old = if self.workbook.current_sheet().cells.contains_key(&(target_row, target_col)) {
                    Some(self.workbook.current_sheet().get_cell(target_row, target_col))
                } else {
                    None
                };
                // If the pasted cell starts with `=`, treat it as a formula
                // and evaluate it at the destination. References are NOT
                // adjusted: this path is only reached when the source was
                // not Cellar (no sentinel/sidecar), so we have no original
                // anchor to shift against. Excel and Sheets behave the same
                // way for plain-text formula paste. The Cellar→Cellar path in
                // `paste()` handles relative adjustment properly.
                let new_cell = if value.starts_with('=') {
                    // Cycle check: pasted text bypasses finish_editing_in_direction,
                    // so a `=A1` pasted into A1 would otherwise be accepted.
                    if !self.iterative_calc {
                        let names = self.workbook.named_ranges.clone();
                        let evaluator = FormulaEvaluator::for_workbook(
                            &self.workbook,
                            self.workbook.current_sheet(),
                            &names,
                        );
                        let same = evaluator.would_create_circular_reference(
                            value,
                            (target_row, target_col),
                        );
                        let precedents = evaluator.extract_qualified_refs(value);
                        let sheet_name = self
                            .workbook
                            .sheet_names[self.workbook.active_sheet]
                            .clone();
                        let cross = self.workbook.would_create_cross_sheet_cycle(
                            &sheet_name,
                            target_row,
                            target_col,
                            &precedents,
                        );
                        if same || cross {
                            // Skip this one cell; keep going. Surfacing via
                            // status_message at end of paste.
                            continue;
                        }
                    }
                    let evaluator = FormulaEvaluator::new(self.workbook.current_sheet());
                    // Same clock-publish rationale as the multi-cell paste path.
                    let evaluated = crate::domain::parser::with_recalc_clock(
                        crate::domain::parser::now_serial(),
                        || evaluator.evaluate_formula(value),
                    );
                    CellData {
                        value: evaluated,
                        formula: Some(value.to_string()),
                        format: None,
                        comment: None,
                        spill_anchor: None,
                    }
                } else {
                    CellData {
                        value: value.to_string(),
                        formula: None,
                        format: None,
                        comment: None,
                        spill_anchor: None,
                    }
                };
                batch.push(UndoAction::CellModified {
                    row: target_row,
                    col: target_col,
                    old_cell: old,
                    new_cell: Some(new_cell.clone()),
                });
                writes.push((target_row, target_col, new_cell));
            }
        }
        let count = writes.len();
        // Single workbook API call handles same-sheet recalc + cross-sheet
        // propagation for the whole paste batch.
        self.workbook.write_cells_on_active(writes);
        if !batch.is_empty() {
            self.record_action(UndoAction::Batch(batch));
        }
        self.status_message = Some(paste_status("Pasted from system clipboard:", count, outside));
    }

    pub fn insert_row(&mut self) {
        let insert_at = self.selected_row;
        let sheet_idx = self.workbook.active_sheet;
        // Routes through Workbook so cross-sheet refs to this sheet adjust
        // too (e.g. `Sheet2!A5` shifts to `Sheet2!A6` on insertion above A5).
        self.workbook.insert_row_on_active(insert_at);
        self.record_action(UndoAction::RowInserted { sheet_idx, at: insert_at });
        self.status_message = Some(format!("Inserted row at {}", insert_at + 1));
    }

    pub fn delete_row(&mut self) {
        let delete_at = self.selected_row;
        let sheet_idx = self.workbook.active_sheet;
        // Snapshot pre-delete: structural cross-sheet shifts can't be
        // reconstructed from the deleted row alone.
        let pre = Box::new(self.workbook.clone());
        self.workbook.delete_row_on_active(delete_at);
        if self.selected_row >= self.workbook.current_sheet().rows {
            self.selected_row = self.workbook.current_sheet().rows.saturating_sub(1);
        }
        self.record_action(UndoAction::RowDeleted { sheet_idx, at: delete_at, pre });
        self.status_message = Some(format!("Deleted row {}", delete_at + 1));
    }

    pub fn insert_col(&mut self) {
        let insert_at = self.selected_col;
        let sheet_idx = self.workbook.active_sheet;
        self.workbook.insert_col_on_active(insert_at);
        self.record_action(UndoAction::ColInserted { sheet_idx, at: insert_at });
        self.status_message = Some(format!("Inserted column at {}", crate::domain::Spreadsheet::column_label(insert_at)));
    }

    pub fn delete_col(&mut self) {
        let delete_at = self.selected_col;
        let sheet_idx = self.workbook.active_sheet;
        let pre = Box::new(self.workbook.clone());
        self.workbook.delete_col_on_active(delete_at);
        if self.selected_col >= self.workbook.current_sheet().cols {
            self.selected_col = self.workbook.current_sheet().cols.saturating_sub(1);
        }
        self.record_action(UndoAction::ColDeleted { sheet_idx, at: delete_at, pre });
        self.status_message = Some(format!("Deleted column {}", crate::domain::Spreadsheet::column_label(delete_at)));
    }

    /// Insert `count` rows above row `at` as one undo step (Excel inserts
    /// as many rows as are selected).
    pub fn insert_rows(&mut self, at: usize, count: usize) {
        let count = count.max(1);
        self.with_snapshot_undo("insert rows", |app| {
            for _ in 0..count {
                app.workbook.insert_row_on_active(at);
            }
        });
        self.status_message = Some(format!("Inserted {} row(s) at {}", count, at + 1));
    }

    /// Delete rows `at..at+count` as one undo step.
    pub fn delete_rows(&mut self, at: usize, count: usize) {
        let count = count.max(1).min(self.workbook.current_sheet().rows.saturating_sub(at));
        self.with_snapshot_undo("delete rows", |app| {
            for _ in 0..count {
                app.workbook.delete_row_on_active(at);
            }
        });
        let last = self.workbook.current_sheet().rows.saturating_sub(1);
        self.selected_row = self.selected_row.min(last);
        self.status_message = Some(format!("Deleted {} row(s) from {}", count, at + 1));
    }

    /// Insert `count` columns left of column `at` as one undo step.
    pub fn insert_cols(&mut self, at: usize, count: usize) {
        let count = count.max(1);
        self.with_snapshot_undo("insert columns", |app| {
            for _ in 0..count {
                app.workbook.insert_col_on_active(at);
            }
        });
        self.status_message = Some(format!(
            "Inserted {} column(s) at {}",
            count,
            crate::domain::Spreadsheet::column_label(at)
        ));
    }

    /// Delete columns `at..at+count` as one undo step.
    pub fn delete_cols(&mut self, at: usize, count: usize) {
        let count = count.max(1).min(self.workbook.current_sheet().cols.saturating_sub(at));
        self.with_snapshot_undo("delete columns", |app| {
            for _ in 0..count {
                app.workbook.delete_col_on_active(at);
            }
        });
        let last = self.workbook.current_sheet().cols.saturating_sub(1);
        self.selected_col = self.selected_col.min(last);
        self.status_message = Some(format!(
            "Deleted {} column(s) from {}",
            count,
            crate::domain::Spreadsheet::column_label(at)
        ));
    }

}

/// Status line after a paste, naming any cells past Excel's grid.
fn paste_status(prefix: &str, pasted: usize, outside: usize) -> String {
    if outside == 0 {
        format!("{} {} cell(s)", prefix, pasted)
    } else {
        format!(
            "{} {} cell(s); {} beyond the {} x {} grid were left out",
            prefix,
            pasted,
            outside,
            crate::domain::Spreadsheet::MAX_ROWS,
            crate::domain::Spreadsheet::MAX_COLS
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CellData;

    fn block_3x3() -> App {
        let mut app = App::default();
        for r in 0..3 {
            for c in 0..3 {
                app.set_cell_with_undo(r, c, CellData { value: format!("{}{}", r, c), ..CellData::default() });
            }
        }
        app.selection_start = Some((0, 0));
        app.selection_end = Some((2, 2));
        app.copy_selection();
        app.clear_selection();
        app
    }

    #[test]
    fn paste_at_the_sheet_edge_grows_the_sheet_instead_of_dropping_cells() {
        let mut app = block_3x3();
        let (last_row, last_col) = (app.workbook.current_sheet().rows - 1, app.workbook.current_sheet().cols - 1);
        app.selected_row = last_row;
        app.selected_col = last_col;
        app.paste();
        let sheet = app.workbook.current_sheet();
        for r in 0..3 {
            for c in 0..3 {
                assert_eq!(sheet.get_cell(last_row + r, last_col + c).value, format!("{}{}", r, c));
            }
        }
        assert_eq!((sheet.rows, sheet.cols), (last_row + 3, last_col + 3));
        assert_eq!(app.status_message.as_deref(), Some("Pasted 9 cell(s)"));

        // One undo step removes the whole paste.
        app.undo();
        assert_eq!(app.workbook.current_sheet().get_cell(last_row + 2, last_col + 2).value, "");
    }

    #[test]
    fn paste_past_excels_grid_leaves_out_only_the_cells_beyond_it() {
        let mut app = block_3x3();
        app.workbook.current_sheet_mut().rows = Spreadsheet::MAX_ROWS;
        app.selected_row = Spreadsheet::MAX_ROWS - 1;
        app.selected_col = 0;
        app.paste();
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.rows, Spreadsheet::MAX_ROWS);
        assert_eq!(sheet.get_cell(Spreadsheet::MAX_ROWS - 1, 2).value, "02");
        assert!(!sheet.cells.keys().any(|&(r, _)| r >= Spreadsheet::MAX_ROWS));
        let status = app.status_message.clone().unwrap();
        assert!(status.starts_with("Pasted 3 cell(s); 6 beyond"), "{}", status);
    }

    #[test]
    fn plain_text_paste_at_the_sheet_edge_grows_the_sheet() {
        let mut app = App::default();
        let (last_row, last_col) = (app.workbook.current_sheet().rows - 1, app.workbook.current_sheet().cols - 1);
        app.selected_row = last_row;
        app.selected_col = last_col;
        app.paste_tsv("a\tb\tc\n1\t2\t3\n");
        let sheet = app.workbook.current_sheet();
        assert_eq!(sheet.get_cell(last_row, last_col + 2).value, "c");
        assert_eq!(sheet.get_cell(last_row + 1, last_col + 2).value, "3");
        assert_eq!(app.status_message.as_deref(), Some("Pasted from system clipboard: 6 cell(s)"));
    }

    #[test]
    fn insert_and_delete_several_rows_and_columns_undo_as_one_step() {
        let mut app = App::default();
        app.set_cell_with_undo(2, 2, CellData { value: "x".into(), ..CellData::default() });
        app.insert_rows(1, 3);
        assert_eq!(app.workbook.current_sheet().get_cell(5, 2).value, "x");
        app.insert_cols(0, 2);
        assert_eq!(app.workbook.current_sheet().get_cell(5, 4).value, "x");
        app.undo();
        app.undo();
        assert_eq!(app.workbook.current_sheet().get_cell(2, 2).value, "x");
        app.delete_rows(0, 2);
        assert_eq!(app.workbook.current_sheet().get_cell(0, 2).value, "x");
        app.delete_cols(0, 2);
        assert_eq!(app.workbook.current_sheet().get_cell(0, 0).value, "x");
    }

    #[test]
    fn test_copy_paste_single_cell() {
        let mut app = App::default();
        app.set_cell_with_undo(0, 0, CellData { value: "Hello".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

        // Copy A1
        app.selected_row = 0;
        app.selected_col = 0;
        app.copy_selection();
        assert!(app.clipboard.is_some());

        // Paste to B2
        app.selected_row = 1;
        app.selected_col = 1;
        app.paste();

        assert_eq!(app.workbook.current_sheet().get_cell(1, 1).value, "Hello");
    }

    #[test]
    fn test_copy_paste_range() {
        let mut app = App::default();
        app.set_cell_with_undo(0, 0, CellData { value: "A".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
        app.set_cell_with_undo(0, 1, CellData { value: "B".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
        app.set_cell_with_undo(1, 0, CellData { value: "C".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
        app.set_cell_with_undo(1, 1, CellData { value: "D".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

        // Select A1:B2
        app.selection_start = Some((0, 0));
        app.selection_end = Some((1, 1));
        app.copy_selection();

        // Paste to C3
        app.selected_row = 2;
        app.selected_col = 2;
        app.paste();

        assert_eq!(app.workbook.current_sheet().get_cell(2, 2).value, "A");
        assert_eq!(app.workbook.current_sheet().get_cell(2, 3).value, "B");
        assert_eq!(app.workbook.current_sheet().get_cell(3, 2).value, "C");
        assert_eq!(app.workbook.current_sheet().get_cell(3, 3).value, "D");
    }

    #[test]
    fn test_cut_paste() {
        let mut app = App::default();
        app.set_cell_with_undo(0, 0, CellData { value: "Move me".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

        app.selected_row = 0;
        app.selected_col = 0;
        app.cut_selection();

        // Original cell should be cleared
        assert!(app.workbook.current_sheet().get_cell(0, 0).value.is_empty());

        // Paste to new location
        app.selected_row = 2;
        app.selected_col = 2;
        app.paste();

        assert_eq!(app.workbook.current_sheet().get_cell(2, 2).value, "Move me");
    }

    #[test]
    fn test_paste_nothing() {
        let mut app = App::default();
        app.paste(); // Should not crash
        assert!(app.status_message.as_ref().unwrap().contains("Nothing to paste"));
    }

    #[test]
    fn test_copy_paste_formula_adjusts_refs() {
        let mut app = App::default();
        app.set_cell_with_undo(0, 0, CellData { value: "10".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
        app.set_cell_with_undo(0, 1, CellData {
            value: "20".to_string(),
            formula: Some("=A1*2".to_string()),
            format: None,
            comment: None,
        spill_anchor: None,
        });

        // Copy B1 (has formula =A1*2)
        app.selected_row = 0;
        app.selected_col = 1;
        app.copy_selection();

        // Paste to B2 (should adjust to =A2*2)
        app.selected_row = 1;
        app.selected_col = 1;
        app.paste();

        let pasted = app.workbook.current_sheet().get_cell(1, 1);
        assert!(pasted.formula.is_some());
        assert_eq!(pasted.formula.unwrap(), "=A2*2");
    }

}
