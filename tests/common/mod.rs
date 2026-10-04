//! Test harness for Cellar's end-to-end scenario tests.
//!
//! Drives the same editing session the GUI uses ([`cellar::App`]): cells
//! are typed in through the normal commit path (formula evaluation, cycle
//! checks, undo), sheet commands run through `execute_command`, and values
//! are read back from the workbook. No window is opened, so the tests run
//! headless and deterministically.
//!
//! The scenarios (and the idea of cross-checking whole spreadsheet models
//! against a pure-Rust ground truth) come from tshts.

#![allow(dead_code)]

pub mod scenarios;

use cellar::application::{App, AppMode};
use cellar::domain::{format_cell_value, Spreadsheet};

pub struct Harness {
    pub app: App,
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

impl Harness {
    pub fn new() -> Self {
        Self { app: App::default() }
    }

    /// Move the cursor to `addr` ("B7" or "Sheet2!B7"), creating the sheet
    /// if a qualified address names one that doesn't exist yet.
    pub fn goto(&mut self, addr: &str) {
        let (sheet, cell) = match addr.rsplit_once('!') {
            Some((s, c)) => (Some(s.trim_matches('\'')), c),
            None => (None, addr),
        };
        if let Some(name) = sheet {
            let idx = match self.app.workbook.sheet_names.iter().position(|n| n == name) {
                Some(i) => i,
                None => {
                    self.app.workbook.add_sheet(name.to_string());
                    self.app.workbook.sheets.len() - 1
                }
            };
            self.app.switch_to_sheet(idx);
        }
        let (r, c) = Spreadsheet::parse_cell_reference(&cell.replace('$', ""))
            .unwrap_or_else(|| panic!("bad cell address {:?}", addr));
        let sheet = self.app.workbook.current_sheet_mut();
        sheet.rows = sheet.rows.max(r + 50);
        sheet.cols = sheet.cols.max(c + 5);
        self.app.clear_selection();
        self.app.selected_row = r;
        self.app.selected_col = c;
    }

    /// Type `content` into `addr` and commit it, as a user would.
    pub fn enter(&mut self, addr: &str, content: &str) {
        self.goto(addr);
        self.app.input = content.to_string();
        self.app.cursor_position = self.app.input.chars().count();
        self.app.mode = AppMode::Editing;
        self.app.finish_editing();
    }

    /// Run a sheet command (e.g. `format currency`) on the cursor cell.
    pub fn command(&mut self, cmd: &str) {
        self.app.command_input = cmd.to_string();
        self.app.mode = AppMode::CommandPalette;
        self.app.execute_command();
    }

    /// Recalculate every formula, like the user's "Recalculate all".
    pub fn recalc(&mut self) {
        self.app.recalc_all();
    }

    /// Raw (unformatted) value at `addr`.
    pub fn value(&mut self, addr: &str) -> String {
        self.goto(addr);
        let (r, c) = (self.app.selected_row, self.app.selected_col);
        self.app.workbook.current_sheet().get_cell(r, c).value
    }

    /// Text of row `row` (1-indexed) as displayed, number formats applied,
    /// cells separated by ` | `.
    pub fn displayed_row(&self, row: usize) -> String {
        let sheet = self.app.workbook.current_sheet();
        (0..sheet.cols)
            .map(|c| {
                let cell = sheet.get_cell(row - 1, c);
                match &cell.format {
                    Some(f) => format_cell_value(&cell.value, f),
                    None => cell.value,
                }
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}
