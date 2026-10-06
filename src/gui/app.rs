//! Main window: menus, formula bar, sheet tabs, status bar,
//! keyboard handling and file operations. The grid, charts and dialogs live
//! in sibling modules as further `impl GuiApp` blocks.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Button, Color32, Context, Key, KeyboardShortcut, Layout, Modifiers, RichText,
    TextEdit, ViewportCommand,
};
use cellar::application::{App, AppMode};
use cellar::domain::{
    range_to_markdown, sheet_to_markdown, workbook_to_markdown, CsvExporter, NumberFormat, Spreadsheet,
    TerminalColor, Workbook,
};
use cellar::infrastructure::import::{self, ImportFormat};
use cellar::infrastructure::{recent, xlsx, FileRepository};

use crate::dialogs::Dialogs;
use crate::shortcuts::Action;
use crate::grid::GridState;

pub const FORMULA_BAR_ID: &str = "formula_bar";

/// An in-progress cell edit. The text is shared by the in-cell editor and
/// the formula bar; `in_cell` says which one has the caret.
pub struct Edit {
    /// The sheet the edited cell is on. While typing a formula the user can
    /// show other sheets to click references there.
    pub sheet: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub in_cell: bool,
    /// Focus the editor (and move the caret to the end) next frame.
    pub focus_pending: bool,
    /// Byte offset where the last clicked-in reference starts, so a second
    /// click replaces it instead of appending (Excel's "point mode").
    pub ref_start: Option<usize>,
}

/// Actions that would discard unsaved changes and so ask first.
#[derive(Clone, PartialEq)]
pub enum Guarded {
    New,
    Open,
    /// Reopen a file from File → Open Recent.
    OpenPath(PathBuf),
    /// File → Import → a format.
    Import(ImportFormat),
    Quit,
}

pub struct GuiApp {
    pub app: App,
    pub edit: Option<Edit>,
    pub grid: GridState,
    pub dialogs: Dialogs,
    pub pivots: crate::pivot_ui::PivotUi,
    pub show_sidebar: bool,
    /// A Delete clicked on a sidebar card, applied once the sidebar is drawn.
    pub sidebar_delete: Option<crate::sidebar::SidebarDelete>,
    /// Suggested path for the first "Save" after importing a non-.cellar
    /// file (Excel, CSV, …): same folder, same name, `.cellar` extension.
    pub suggested_save: Option<PathBuf>,
    name_box: String,
    name_box_editing: bool,
    confirm: Option<Guarded>,
    /// Excel's Insert/Delete dialog (Ctrl+Shift+= / Ctrl+-) when the
    /// selection isn't whole rows or columns: (insert?, entire rows?).
    structure_dialog: Option<(bool, bool)>,
    allow_close: bool,
    window_title: String,
    /// Value of Format → Decimal places (2 until the user changes it).
    decimal_places: u32,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<String>) -> Self {
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::open_files::set_context(&cc.egui_ctx);
        let mut gui = Self {
            app: App::default(),
            edit: None,
            grid: GridState::default(),
            dialogs: Dialogs::default(),
            pivots: Default::default(),
            show_sidebar: true,
            sidebar_delete: None,
            suggested_save: None,
            name_box: String::new(),
            name_box_editing: false,
            confirm: None,
            structure_dialog: None,
            allow_close: false,
            window_title: String::new(),
            decimal_places: 2,
        };
        if let Some(path) = file {
            gui.open_path(PathBuf::from(path));
        }
        gui
    }

    // ------------------------------------------------------------ files

    pub fn open_path(&mut self, path: PathBuf) {
        // Absolute, so the recent-files entry works from anywhere.
        let path = std::path::absolute(&path).unwrap_or(path);
        let p = path.to_string_lossy().to_string();
        if !path.exists() {
            self.app.status_message = Some(format!("{} no longer exists", p));
            recent::remove(&p);
            return;
        }
        match ImportFormat::from_path(&path) {
            Some(format) => self.import(path, format),
            None => {
                let result = FileRepository::load_workbook(&p);
                if let Err(e) = &result {
                    self.app.status_message = Some(format!("Could not open {}: {}", p, e));
                    return;
                }
                self.reset_view();
                self.app.set_load_workbook_result(result);
                self.suggested_save = None;
            }
        }
    }

    /// Load a workbook that came from another format. It has no `.cellar`
    /// file yet, so Save asks where to put one.
    fn load_foreign(&mut self, wb: Workbook, source: &Path) {
        self.reset_view();
        self.app.set_load_workbook_result(Ok((wb, source.to_string_lossy().to_string())));
        self.app.filename = None;
        self.app.dirty = true;
        self.suggested_save = Some(source.with_extension("cellar"));
    }

    /// Open a file saved by another program (see File → Import).
    pub fn import(&mut self, path: PathBuf, format: ImportFormat) {
        match import::import_file(&path, format) {
            Ok(imported) => {
                self.load_foreign(imported.workbook, &path);
                self.app.status_message = Some(format!(
                    "Imported {} — save it as .cellar to keep working in text format",
                    file_name(&path)
                ));
                self.dialogs.import_report = imported.report;
            }
            Err(e) => {
                self.app.status_message = Some(format!("Could not import {}: {}", file_name(&path), e))
            }
        }
    }

    fn reset_view(&mut self) {
        self.edit = None;
        self.grid = GridState::default();
        self.app.clear_selection();
    }

    pub fn save(&mut self) {
        match self.app.filename.clone() {
            Some(f) if f.to_lowercase().ends_with(".cellar") => self.save_to(PathBuf::from(f)),
            _ => self.save_as(),
        }
    }

    pub fn save_as(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Cellar workbook", &["cellar"]);
        let current = self.app.filename.clone().map(PathBuf::from).or(self.suggested_save.clone());
        if let Some(path) = current {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                dialog = dialog.set_directory(dir);
            }
            dialog = dialog.set_file_name(file_name(&path.with_extension("cellar")));
        } else {
            dialog = dialog.set_file_name("spreadsheet.cellar");
        }
        if let Some(mut path) = dialog.save_file() {
            if extension(&path) != "cellar" {
                path.set_extension("cellar");
            }
            self.save_to(path);
        }
    }

    fn save_to(&mut self, path: PathBuf) {
        self.commit_edit(EditMove::Stay);
        self.app.snapshot_view_state_to_active_sheet();
        let p = path.to_string_lossy().to_string();
        match FileRepository::save_workbook(&self.app.workbook, &p) {
            Ok(_) => {
                recent::add(&p);
                self.app.filename = Some(p);
                self.app.dirty = false;
                self.suggested_save = None;
                self.app.status_message = Some(format!("Saved {}", file_name(&path)));
            }
            Err(e) => self.app.status_message = Some(format!("Save failed: {}", e)),
        }
    }

    fn export_xlsx(&mut self) {
        let name = self.base_name() + ".xlsx";
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Excel workbook", &["xlsx"])
            .set_file_name(name)
            .save_file()
        {
            let p = path.to_string_lossy().to_string();
            self.app.status_message = Some(match xlsx::save_xlsx(&self.app.workbook, &p) {
                Ok(()) => format!("Exported {}", file_name(&path)),
                Err(e) => format!("Export failed: {}", e),
            });
        }
    }

    fn export_csv(&mut self) {
        let name = self.base_name() + ".csv";
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name(name)
            .save_file()
        {
            let p = path.to_string_lossy().to_string();
            self.app.status_message =
                Some(match CsvExporter::export_to_csv(self.app.workbook.current_sheet(), &p) {
                    Ok(_) => format!("Exported current sheet to {}", file_name(&path)),
                    Err(e) => format!("Export failed: {}", e),
                });
        }
    }

    /// Markdown table(s) of the computed values, no formatting.
    fn export_markdown(&mut self, all_sheets: bool) {
        let wb = &self.app.workbook;
        let text = if all_sheets {
            workbook_to_markdown(wb)
        } else {
            sheet_to_markdown(wb.current_sheet())
        };
        if text.is_empty() {
            self.app.status_message = Some("Nothing to export: the sheet is empty".into());
            return;
        }
        let name = if all_sheets {
            self.base_name()
        } else {
            format!("{} - {}", self.base_name(), wb.sheet_names[wb.active_sheet])
        };
        if let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .set_file_name(format!("{}.md", name))
            .save_file()
        {
            if path.extension().is_none() {
                path.set_extension("md");
            }
            self.app.status_message = Some(match cellar::infrastructure::atomic::atomic_write(&path.to_string_lossy(), text.as_bytes()) {
                Ok(_) => format!("Exported {}", file_name(&path)),
                Err(e) => format!("Export failed: {}", e),
            });
        }
    }

    fn copy_selection_markdown(&mut self, ctx: &Context) {
        let (a, b) = self.selection_or_cursor();
        let text = range_to_markdown(self.app.workbook.current_sheet(), a, b);
        ctx.copy_text(text);
        self.app.status_message = Some("Copied the selection as a Markdown table".into());
    }

    fn base_name(&self) -> String {
        self.app
            .filename
            .clone()
            .map(PathBuf::from)
            .or(self.suggested_save.clone())
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "spreadsheet".to_string())
    }

    /// Run `action`, first asking to save if there are unsaved changes.
    pub fn guarded(&mut self, ctx: &Context, action: Guarded) {
        if self.app.dirty {
            self.confirm = Some(action);
        } else {
            self.run_guarded(ctx, action);
        }
    }

    fn run_guarded(&mut self, ctx: &Context, action: Guarded) {
        match action {
            Guarded::New => {
                self.app = App::default();
                self.reset_view();
                self.suggested_save = None;
            }
            Guarded::Open => {
                let mut all = vec!["cellar"];
                all.extend(ImportFormat::all_extensions());
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("All supported files", &all)
                    .add_filter("Cellar workbook", &["cellar"]);
                for format in ImportFormat::ALL {
                    dialog = dialog.add_filter(format.name(), format.extensions());
                }
                if let Some(path) = dialog.pick_file() {
                    self.open_path(path);
                }
            }
            Guarded::OpenPath(path) => self.open_path(path),
            Guarded::Import(format) => {
                if let Some(path) =
                    rfd::FileDialog::new().add_filter(format.name(), format.extensions()).pick_file()
                {
                    self.import(path, format);
                }
            }
            Guarded::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    // ---------------------------------------------------------- editing

    pub fn cell_input_text(&self, row: usize, col: usize) -> String {
        let cell = self.app.workbook.current_sheet().get_cell(row, col);
        cell.formula.unwrap_or(cell.value)
    }

    pub fn start_edit(&mut self, initial: Option<String>, in_cell: bool) {
        let (row, col) = (self.app.selected_row, self.app.selected_col);
        let cell = self.app.workbook.current_sheet().get_cell(row, col);
        if let Some(anchor) = cell.spill_anchor {
            self.app.status_message = Some(format!(
                "This cell is filled by the formula in {}{}; edit that cell instead",
                Spreadsheet::column_label(anchor.1),
                anchor.0 + 1
            ));
            return;
        }
        let text = initial.unwrap_or_else(|| self.cell_input_text(row, col));
        let sheet = self.app.workbook.active_sheet;
        self.edit = Some(Edit { sheet, row, col, text, in_cell, focus_pending: true, ref_start: None });
        self.grid.scroll_to_cursor = true;
    }

    /// Write the pending edit through Cellar's normal commit path (formula
    /// evaluation, cycle checks, undo) and move the cursor.
    pub fn commit_edit(&mut self, mv: EditMove) {
        let Some(edit) = self.edit.take() else { return };
        // A formula picked across sheets is entered on its own sheet.
        self.show_sheet(edit.sheet);
        self.app.clear_selection();
        self.app.selected_row = edit.row;
        self.app.selected_col = edit.col;
        self.app.input = edit.text;
        self.app.cursor_position = self.app.input.chars().count();
        self.app.mode = AppMode::Editing;
        match mv {
            EditMove::Down | EditMove::Stay => self.app.finish_editing(),
            EditMove::Up => self.app.finish_editing_move_up(),
            EditMove::Right => self.app.finish_editing_move_right(),
            EditMove::Left => self.app.finish_editing_move_left(),
        }
        if mv == EditMove::Stay {
            self.app.selected_row = edit.row;
            self.app.selected_col = edit.col;
        }
        self.app.mode = AppMode::Normal;
        self.ensure_sheet_size();
        self.grid.scroll_to_cursor = true;
    }

    pub fn cancel_edit(&mut self) {
        if let Some(edit) = self.edit.take() {
            self.show_sheet(edit.sheet);
        }
    }

    /// Show sheet `idx`, keeping each sheet's cursor and scroll position.
    fn show_sheet(&mut self, idx: usize) {
        if idx == self.app.workbook.active_sheet || idx >= self.app.workbook.sheets.len() {
            return;
        }
        self.app.snapshot_view_state_to_active_sheet();
        self.app.switch_to_sheet(idx);
        self.app.restore_view_state_from_active_sheet();
        self.grid = GridState::default();
    }

    pub fn run_command(&mut self, cmd: &str) {
        self.commit_edit(EditMove::Stay);
        self.app.command_input = cmd.to_string();
        self.app.mode = AppMode::CommandPalette;
        self.app.execute_command();
        self.app.mode = AppMode::Normal;
    }

    /// Move the cursor, optionally extending the selection (Shift).
    pub fn move_cursor(&mut self, drow: isize, dcol: isize, extend: bool) {
        let (r0, c0) = (self.app.selected_row, self.app.selected_col);
        if extend {
            let anchor = self.app.selection_start.unwrap_or((r0, c0));
            let end = self.app.selection_end.unwrap_or((r0, c0));
            let r = (end.0 as isize + drow).max(0) as usize;
            let c = (end.1 as isize + dcol).max(0) as usize;
            self.grow_to(r, c);
            let sheet = self.app.workbook.current_sheet();
            let (r, c) = (r.min(sheet.rows - 1), c.min(sheet.cols - 1));
            self.app.selection_start = Some(anchor);
            self.app.selection_end = Some((r, c));
            self.grid.scroll_target = Some((r, c));
        } else {
            let r = (r0 as isize + drow).max(0) as usize;
            let c = (c0 as isize + dcol).max(0) as usize;
            self.set_cursor(r, c);
        }
        self.grid.scroll_to_cursor = true;
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.grow_to(row, col);
        let sheet = self.app.workbook.current_sheet();
        self.app.selected_row = row.min(sheet.rows - 1);
        self.app.selected_col = col.min(sheet.cols - 1);
        self.app.clear_selection();
        self.grid.scroll_target = None;
    }

    /// Sheets have a fixed size in Cellar; grow it when the user walks past
    /// the edge so the grid feels unbounded.
    pub(crate) fn grow_to(&mut self, row: usize, col: usize) {
        // Excel's row limit: the Name Box accepts any row number, and the
        // grid, Select All and copy all scale with the sheet's size.
        const MAX_ROWS: usize = Spreadsheet::MAX_ROWS;
        let sheet = self.app.workbook.current_sheet_mut();
        if row + 1 >= sheet.rows && sheet.rows < MAX_ROWS {
            sheet.rows = (row + 50).min(MAX_ROWS);
        }
        if col + 1 >= sheet.cols && col < 1024 {
            sheet.cols = (col + 5).min(1024);
        }
    }

    fn ensure_sheet_size(&mut self) {
        let (r, c) = (self.app.selected_row, self.app.selected_col);
        self.grow_to(r, c);
    }

    pub fn selection_or_cursor(&self) -> ((usize, usize), (usize, usize)) {
        let cur = (self.app.selected_row, self.app.selected_col);
        self.app.get_selection_range().unwrap_or((cur, cur))
    }

    // --------------------------------------------------------- keyboard

    fn handle_keys(&mut self, ctx: &Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let modal_open = self.dialogs.any_open() || self.pivots.any_dialog_open() || self.structure_dialog.is_some();

        // Shortcut table (see shortcuts.rs). Bindings with more modifiers
        // are tried first so Ctrl+Shift+9 isn't taken as Ctrl+9.
        let mut bindings: Vec<(Action, KeyboardShortcut)> = Action::ALL
            .iter()
            .filter(|a| !a.bound_elsewhere())
            .flat_map(|a| a.bindings(ctx).into_iter().map(move |k| (*a, k)))
            .collect();
        bindings.sort_by_key(|(_, k)| {
            std::cmp::Reverse(
                [k.modifiers.ctrl, k.modifiers.shift, k.modifiers.alt, k.modifiers.command, k.modifiers.mac_cmd]
                    .iter()
                    .filter(|m| **m)
                    .count(),
            )
        });
        let editing = self.edit.is_some();
        for (action, shortcut) in &bindings {
            let allowed = action.works_while_typing()
                || (editing && matches!(action, Action::InsertDate | Action::InsertTime))
                || (!typing && !editing && !modal_open);
            if allowed && ctx.input_mut(|i| i.consume_shortcut(shortcut)) {
                self.do_action(ctx, *action);
            }
        }

        if self.edit.is_some() {
            self.handle_edit_keys(ctx, typing);
            return;
        }
        if typing || modal_open {
            return;
        }

        let mut start_text: Option<String> = None;
        let mut clipboard_op = None;
        let (mods, pressed) = ctx.input_mut(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Copy => clipboard_op = Some("copy"),
                    egui::Event::Cut => clipboard_op = Some("cut"),
                    egui::Event::Paste(_) => clipboard_op = Some("paste"),
                    // Typing starts an edit; chords with Ctrl/⌘ don't, and
                    // neither does a space typed with Shift (Select Row).
                    egui::Event::Text(t)
                        if !i.modifiers.command && !i.modifiers.ctrl && !(i.modifiers.shift && t == " ") =>
                    {
                        start_text.get_or_insert_with(String::new).push_str(t);
                    }
                    _ => {}
                }
            }
            let keys = [
                Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight, Key::Enter,
                Key::Tab, Key::F2, Key::Delete, Key::Backspace, Key::PageUp, Key::PageDown,
                Key::Home, Key::End, Key::Escape,
            ];
            let pressed: Vec<Key> = keys.into_iter().filter(|k| i.key_pressed(*k)).collect();
            // Tab would otherwise move keyboard focus to the formula bar.
            i.consume_key(Modifiers::NONE, Key::Tab);
            i.consume_key(Modifiers::SHIFT, Key::Tab);
            (i.modifiers, pressed)
        });

        match clipboard_op {
            Some("copy") => self.app.copy_selection(),
            Some("cut") => self.app.cut_selection(),
            Some("paste") => {
                self.app.paste();
                self.ensure_sheet_size();
            }
            _ => {}
        }
        let shift = mods.shift;
        // Excel: Ctrl+Arrow (⌘+Arrow on Mac) jumps to the edge of the data.
        let jump = mods.command;
        for key in pressed {
            match key {
                Key::ArrowUp if jump => self.jump_edge(-1, 0, shift),
                Key::ArrowDown if jump => self.jump_edge(1, 0, shift),
                Key::ArrowLeft if jump => self.jump_edge(0, -1, shift),
                Key::ArrowRight if jump => self.jump_edge(0, 1, shift),
                Key::ArrowUp => self.move_cursor(-1, 0, shift),
                Key::ArrowDown => self.move_cursor(1, 0, shift),
                Key::ArrowLeft => self.move_cursor(0, -1, shift),
                Key::ArrowRight => self.move_cursor(0, 1, shift),
                Key::PageUp => self.move_cursor(-25, 0, shift),
                Key::PageDown => self.move_cursor(25, 0, shift),
                Key::Enter => self.move_cursor(if shift { -1 } else { 1 }, 0, false),
                Key::Tab => self.move_cursor(0, if shift { -1 } else { 1 }, false),
                Key::Home if jump => {
                    self.set_cursor(0, 0);
                    self.grid.scroll_to_cursor = true;
                }
                Key::Home => {
                    let r = self.app.selected_row;
                    self.set_cursor(r, 0);
                    self.grid.scroll_to_cursor = true;
                }
                Key::End if jump => {
                    let (r, c) = self.last_used_cell();
                    self.set_cursor(r, c);
                    self.grid.scroll_to_cursor = true;
                }
                Key::F2 => self.start_edit(None, true),
                Key::Delete | Key::Backspace => self.app.clear_selection_contents(),
                Key::Escape => self.app.dismiss_transients(),
                _ => {}
            }
        }
        if let Some(text) = start_text {
            self.start_edit(Some(text), true);
        }
    }

    fn handle_edit_keys(&mut self, ctx: &Context, typing: bool) {
        // Excel: Ctrl+Enter puts the entry into every selected cell.
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
            ctx.memory_mut(|m| {
                m.surrender_focus(egui::Id::new(FORMULA_BAR_ID));
                m.surrender_focus(egui::Id::new(crate::grid::CELL_EDITOR_ID));
            });
            let home = self.edit.as_ref().is_some_and(|e| e.sheet == self.app.workbook.active_sheet);
            let range = self.app.get_selection_range().filter(|_| home);
            let at = self.edit.as_ref().map(|e| (e.row, e.col));
            self.commit_edit(EditMove::Stay);
            if let (Some(range), Some(at)) = (range, at) {
                self.app.fill_block_from(at, range);
                self.app.selection_start = Some(range.0);
                self.app.selection_end = Some(range.1);
            }
            return;
        }
        let (enter, tab, esc, shift) = ctx.input_mut(|i| {
            let shift = i.modifiers.shift;
            let enter = i.consume_key(Modifiers::NONE, Key::Enter)
                || i.consume_key(Modifiers::SHIFT, Key::Enter);
            let tab = i.consume_key(Modifiers::NONE, Key::Tab)
                || i.consume_key(Modifiers::SHIFT, Key::Tab);
            let esc = i.consume_key(Modifiers::NONE, Key::Escape);
            (enter, tab, esc, shift)
        });
        if esc || enter || tab {
            // Hand the keyboard back to the grid; otherwise the formula
            // bar keeps focus after Enter and arrows stop navigating.
            ctx.memory_mut(|m| {
                m.surrender_focus(egui::Id::new(FORMULA_BAR_ID));
                m.surrender_focus(egui::Id::new(crate::grid::CELL_EDITOR_ID));
            });
        }
        if esc {
            self.cancel_edit();
        } else if enter {
            self.commit_edit(if shift { EditMove::Up } else { EditMove::Down });
        } else if tab {
            self.commit_edit(if shift { EditMove::Left } else { EditMove::Right });
        } else if !typing {
            // Editor lost focus (e.g. a menu click): arrows commit and move.
            let dir = ctx.input(|i| {
                [
                    (Key::ArrowUp, EditMove::Up),
                    (Key::ArrowDown, EditMove::Down),
                    (Key::ArrowLeft, EditMove::Left),
                    (Key::ArrowRight, EditMove::Right),
                ]
                .into_iter()
                .find(|(k, _)| i.key_pressed(*k))
                .map(|(_, d)| d)
            });
            if let Some(d) = dir {
                self.commit_edit(d);
            }
        }
    }

    // --------------------------------------------------------------- ui

    fn menu_bar(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                self.menu_item(ui, ctx, Action::New);
                self.menu_item(ui, ctx, Action::Open);
                ui.menu_button("Open Recent", |ui| {
                    let files = recent::load();
                    if files.is_empty() {
                        ui.label(RichText::new("No recently opened files").weak());
                    }
                    for (i, f) in files.iter().enumerate() {
                        let path = PathBuf::from(f);
                        let label = format!("{}  {}", i + 1, file_name(&path));
                        if ui.button(label).on_hover_text(f).clicked() {
                            self.guarded(ctx, Guarded::OpenPath(path));
                            ui.close();
                        }
                    }
                    if !files.is_empty() {
                        ui.separator();
                        if ui.button("Clear Recently Opened").clicked() {
                            recent::clear();
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Import", |ui| {
                    for format in ImportFormat::ALL {
                        // Separators between spreadsheets, delimited text
                        // and structured text, like the Export menu.
                        if matches!(format, ImportFormat::Csv | ImportFormat::Markdown) {
                            ui.separator();
                        }
                        if ui.button(format!("{}…", format.label())).clicked() {
                            self.guarded(ctx, Guarded::Import(format));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                self.menu_item(ui, ctx, Action::Save);
                self.menu_item(ui, ctx, Action::SaveAs);
                ui.separator();
                ui.menu_button("Export", |ui| {
                    if ui.button("Excel workbook (.xlsx)…").clicked() {
                        self.export_xlsx();
                    }
                    if ui.button("CSV (current sheet)…").clicked() {
                        self.export_csv();
                    }
                    ui.separator();
                    if ui.button("Markdown table (current sheet)…").clicked() {
                        self.export_markdown(false);
                    }
                    if ui.button("Markdown (all sheets)…").clicked() {
                        self.export_markdown(true);
                    }
                    ui.separator();
                    if ui.button("All charts as PNG…").clicked() {
                        self.export_all_charts("png");
                    }
                    if ui.button("All charts as SVG…").clicked() {
                        self.export_all_charts("svg");
                    }
                });
                ui.separator();
                self.menu_item(ui, ctx, Action::Quit);
            });
            ui.menu_button("Edit", |ui| {
                for a in [Action::Undo, Action::Redo] {
                    self.menu_item(ui, ctx, a);
                }
                ui.separator();
                for a in [Action::Cut, Action::Copy, Action::Paste, Action::ClearContents, Action::CopyMarkdown] {
                    self.menu_item(ui, ctx, a);
                }
                ui.separator();
                for a in [Action::FillDown, Action::FillRight, Action::FillSeries] {
                    self.menu_item(ui, ctx, a);
                }
                ui.separator();
                for a in [Action::InsertCells, Action::DeleteCells] {
                    self.menu_item(ui, ctx, a);
                }
                self.row_col_buttons(ui, ctx);
                ui.separator();
                for a in [Action::SelectRow, Action::SelectColumn, Action::SelectAll] {
                    self.menu_item(ui, ctx, a);
                }
                ui.separator();
                for a in [Action::InsertDate, Action::InsertTime] {
                    self.menu_item(ui, ctx, a);
                }
            });
            ui.menu_button("Format", |ui| self.format_menu(ui, ctx));
            ui.menu_button("Data", |ui| {
                self.menu_item(ui, ctx, Action::AutoSum);
                ui.separator();
                self.menu_item(ui, ctx, Action::SortAscending);
                self.menu_item(ui, ctx, Action::SortDescending);
                ui.separator();
                self.menu_item(ui, ctx, Action::PivotTable);
                self.menu_item(ui, ctx, Action::InsertChart);
                ui.separator();
                self.menu_item(ui, ctx, Action::Recalculate);
            });
            ui.menu_button("Sheet", |ui| {
                for a in [Action::NewSheet, Action::RenameSheet, Action::DeleteSheet] {
                    self.menu_item(ui, ctx, a);
                }
                ui.separator();
                for a in [Action::NextSheet, Action::PreviousSheet] {
                    self.menu_item(ui, ctx, a);
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_sidebar, Action::ToggleSidebar.label());
                ui.separator();
                for a in [Action::ZoomIn, Action::ZoomOut, Action::ZoomReset] {
                    self.menu_item(ui, ctx, a);
                }
            });
            ui.menu_button("Help", |ui| {
                self.menu_item(ui, ctx, Action::Help);
                ui.separator();
                self.menu_item(ui, ctx, Action::About);
            });
        });
    }

    /// A menu button for `action`, with its shortcut shown on the right.
    pub fn menu_item(&mut self, ui: &mut egui::Ui, ctx: &Context, action: Action) {
        let enabled = match action {
            Action::DeleteSheet => self.app.workbook.sheets.len() > 1,
            Action::NextSheet => self.app.workbook.active_sheet + 1 < self.app.workbook.sheets.len(),
            Action::PreviousSheet => self.app.workbook.active_sheet > 0,
            _ => true,
        };
        let button = Button::new(action.label()).shortcut_text(action.shortcut_text(ctx));
        if ui.add_enabled(enabled, button).clicked() {
            self.do_action(ctx, action);
        }
    }

    pub fn row_col_buttons(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        for a in [Action::InsertRows, Action::DeleteRows, Action::InsertColumns, Action::DeleteColumns] {
            self.menu_item(ui, ctx, a);
        }
    }

    fn format_menu(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        self.menu_item(ui, ctx, Action::Bold);
        self.menu_item(ui, ctx, Action::Underline);
        ui.separator();
        ui.menu_button("Number Format", |ui| {
            for a in [Action::FormatGeneral, Action::FormatNumber, Action::FormatCurrency, Action::FormatPercent] {
                self.menu_item(ui, ctx, a);
            }
            ui.separator();
            for (label, fmt) in number_formats().into_iter().skip(1) {
                if ui.button(label).clicked() {
                    self.app.set_selection_format(fmt);
                }
            }
        });
        self.decimal_places(ui);
        ui.menu_button("Text Color", |ui| self.color_choices(ui, false));
        ui.menu_button("Fill Color", |ui| self.color_choices(ui, true));
        ui.separator();
        ui.menu_button("Hide & Unhide", |ui| {
            for a in [Action::HideRows, Action::UnhideRows, Action::HideColumns, Action::UnhideColumns] {
                self.menu_item(ui, ctx, a);
            }
        });
        self.menu_item(ui, ctx, Action::AutoFitColumn);
    }

    /// Run a menu command / shortcut.
    pub fn do_action(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::New => self.guarded(ctx, Guarded::New),
            Action::Open => self.guarded(ctx, Guarded::Open),
            Action::Save => self.save(),
            Action::SaveAs => self.save_as(),
            Action::Quit => self.guarded(ctx, Guarded::Quit),
            Action::Undo => {
                self.commit_edit(EditMove::Stay);
                self.app.undo();
            }
            Action::Redo => {
                self.commit_edit(EditMove::Stay);
                self.app.redo();
            }
            Action::Cut => self.app.cut_selection(),
            Action::Copy => self.app.copy_selection(),
            Action::Paste => {
                self.app.paste();
                self.ensure_sheet_size();
            }
            Action::ClearContents => self.app.clear_selection_contents(),
            Action::CopyMarkdown => self.copy_selection_markdown(ctx),
            Action::FillDown => self.app.fill_down_or_right(true),
            Action::FillRight => self.app.fill_down_or_right(false),
            Action::FillSeries => self.app.autofill_selection(),
            Action::InsertCells => self.insert_or_delete(true),
            Action::DeleteCells => self.insert_or_delete(false),
            Action::InsertRows => self.apply_structure(true, true),
            Action::DeleteRows => self.apply_structure(false, true),
            Action::InsertColumns => self.apply_structure(true, false),
            Action::DeleteColumns => self.apply_structure(false, false),
            Action::SelectRow => {
                let ((r0, _), (r1, _)) = self.selection_or_cursor();
                let last = self.app.workbook.current_sheet().cols - 1;
                self.app.selection_start = Some((r0, 0));
                self.app.selection_end = Some((r1, last));
            }
            Action::SelectColumn => {
                let ((_, c0), (_, c1)) = self.selection_or_cursor();
                let last = self.app.workbook.current_sheet().rows - 1;
                self.app.selection_start = Some((0, c0));
                self.app.selection_end = Some((last, c1));
            }
            Action::SelectAll => {
                let sheet = self.app.workbook.current_sheet();
                self.app.selection_start = Some((0, 0));
                self.app.selection_end = Some((sheet.rows - 1, sheet.cols - 1));
            }
            Action::InsertDate | Action::InsertTime => self.insert_now(action == Action::InsertDate),
            Action::Bold => self.app.toggle_bold(),
            Action::Underline => self.app.toggle_underline(),
            Action::FormatGeneral => self.app.set_selection_format(NumberFormat::General),
            Action::FormatNumber => {
                self.app.set_selection_format(NumberFormat::Number { decimals: 2, thousands_sep: true })
            }
            Action::FormatCurrency => {
                self.app.set_selection_format(NumberFormat::Currency { symbol: "$".into(), decimals: 2 })
            }
            Action::FormatPercent => self.app.set_selection_format(NumberFormat::Percentage { decimals: 0 }),
            Action::HideRows | Action::UnhideRows | Action::HideColumns | Action::UnhideColumns => {
                self.hide_unhide(action)
            }
            Action::AutoFitColumn => {
                let ((_, c0), (_, c1)) = self.selection_or_cursor();
                for c in c0..=c1.min(c0 + 200) {
                    self.app.workbook.current_sheet_mut().auto_resize_column(c);
                }
                self.app.dirty = true;
            }
            Action::AutoSum => self.auto_sum(),
            Action::SortAscending => self.app.sort_column_asc(),
            Action::SortDescending => self.app.sort_column_desc(),
            Action::PivotTable => self.open_create_pivot(),
            Action::InsertChart => self.open_chart_dialog(None),
            Action::Recalculate => self.app.recalc_all(),
            Action::NewSheet => self.run_command("sheet new"),
            Action::NextSheet | Action::PreviousSheet => {
                let n = self.app.workbook.sheets.len();
                let cur = self.app.workbook.active_sheet;
                let to = if action == Action::NextSheet { (cur + 1).min(n - 1) } else { cur.saturating_sub(1) };
                if to != cur {
                    self.commit_edit(EditMove::Stay);
                    self.app.snapshot_view_state_to_active_sheet();
                    self.app.switch_to_sheet(to);
                    self.app.restore_view_state_from_active_sheet();
                    self.grid = GridState::default();
                }
            }
            Action::RenameSheet => self.dialogs.open_rename(&self.app),
            Action::DeleteSheet => self.run_command("sheet delete"),
            Action::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Action::ZoomIn => ctx.set_zoom_factor((ctx.zoom_factor() * 1.1).min(3.0)),
            Action::ZoomOut => ctx.set_zoom_factor((ctx.zoom_factor() / 1.1).max(0.5)),
            Action::ZoomReset => ctx.set_zoom_factor(1.0),
            Action::Help => self.dialogs.help = true,
            Action::About => self.dialogs.about = true,
        }
    }

    /// Excel's Ctrl+Shift+= / Ctrl+-: whole rows or columns selected are
    /// inserted/deleted directly; otherwise the Insert/Delete dialog asks.
    fn insert_or_delete(&mut self, insert: bool) {
        let ((r0, c0), (r1, c1)) = self.selection_or_cursor();
        let sheet = self.app.workbook.current_sheet();
        let whole_rows = c0 == 0 && c1 + 1 >= sheet.cols;
        let whole_cols = r0 == 0 && r1 + 1 >= sheet.rows;
        if whole_rows || whole_cols {
            self.apply_structure(insert, whole_rows);
        } else {
            self.structure_dialog = Some((insert, true));
        }
    }

    /// Insert/delete as many rows (or columns) as the selection spans.
    fn apply_structure(&mut self, insert: bool, rows: bool) {
        self.commit_edit(EditMove::Stay);
        let ((r0, c0), (r1, c1)) = self.selection_or_cursor();
        match (insert, rows) {
            (true, true) => self.app.insert_rows(r0, r1 - r0 + 1),
            (false, true) => self.app.delete_rows(r0, r1 - r0 + 1),
            (true, false) => self.app.insert_cols(c0, c1 - c0 + 1),
            (false, false) => self.app.delete_cols(c0, c1 - c0 + 1),
        }
    }

    fn structure_dialog_ui(&mut self, ctx: &Context) {
        let Some((insert, mut rows)) = self.structure_dialog else { return };
        let mut done = None;
        egui::Modal::new(egui::Id::new("insert_delete")).show(ctx, |ui| {
            ui.heading(if insert { "Insert" } else { "Delete" });
            ui.radio_value(&mut rows, true, "Entire row");
            ui.radio_value(&mut rows, false, "Entire column");
            ui.label(
                RichText::new("Cellar inserts and deletes whole rows or columns; shifting single cells isn't supported.")
                    .weak()
                    .small(),
            );
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        self.structure_dialog = Some((insert, rows));
        match done {
            Some(true) => {
                self.structure_dialog = None;
                self.apply_structure(insert, rows);
            }
            Some(false) => self.structure_dialog = None,
            None => {}
        }
    }

    /// Excel's Ctrl+9 / Ctrl+0 and their Shift variants.
    fn hide_unhide(&mut self, action: Action) {
        let ((r0, c0), (r1, c1)) = self.selection_or_cursor();
        match action {
            Action::HideRows => self.app.hidden_rows.extend(r0..=r1),
            Action::HideColumns => self.app.hidden_cols.extend(c0..=c1),
            Action::UnhideRows => {
                // Unhide within the selection; with nothing hidden there,
                // unhide everything.
                let before = self.app.hidden_rows.len();
                self.app.hidden_rows.retain(|r| !(r0..=r1).contains(r));
                if self.app.hidden_rows.len() == before {
                    self.app.hidden_rows.clear();
                }
            }
            Action::UnhideColumns => {
                let before = self.app.hidden_cols.len();
                self.app.hidden_cols.retain(|c| !(c0..=c1).contains(c));
                if self.app.hidden_cols.len() == before {
                    self.app.hidden_cols.clear();
                }
            }
            _ => return,
        }
        self.app.dirty = true;
    }

    /// Excel's Alt+=: start a SUM over the numbers directly above the
    /// cursor (or to its left), ready to confirm with Enter.
    fn auto_sum(&mut self) {
        self.commit_edit(EditMove::Stay);
        let (r, c) = (self.app.selected_row, self.app.selected_col);
        let sheet = self.app.workbook.current_sheet();
        let is_num = |r: usize, c: usize| sheet.get_cell(r, c).value.trim().parse::<f64>().is_ok();
        let label = |r: usize, c: usize| format!("{}{}", Spreadsheet::column_label(c), r + 1);
        let mut top = r;
        while top > 0 && is_num(top - 1, c) {
            top -= 1;
        }
        let formula = if top < r {
            format!("=SUM({}:{})", label(top, c), label(r - 1, c))
        } else {
            let mut left = c;
            while left > 0 && is_num(r, left - 1) {
                left -= 1;
            }
            if left < c { format!("=SUM({}:{})", label(r, left), label(r, c - 1)) } else { "=SUM()".to_string() }
        };
        self.start_edit(Some(formula), true);
    }

    /// Excel's Ctrl+; (date) and Ctrl+Shift+; (time): typed into the cell
    /// being edited, or start an edit with it.
    fn insert_now(&mut self, date: bool) {
        let serial = cellar::domain::parser::now_serial();
        let text = if date {
            let (y, m, d) = cellar::domain::parser::serial_to_date_pub(serial);
            format!("{:04}-{:02}-{:02}", y, m, d)
        } else {
            let mins = ((serial.fract() * 1440.0).round() as u32) % 1440;
            format!("{:02}:{:02}", mins / 60, mins % 60)
        };
        match &mut self.edit {
            Some(e) => {
                e.text.push_str(&text);
                e.focus_pending = true;
            }
            None => self.start_edit(Some(text), true),
        }
    }

    /// Excel's Ctrl+Arrow: jump to the edge of the current block of data,
    /// or to the next filled cell, or to the sheet's edge.
    fn jump_edge(&mut self, dr: isize, dc: isize, extend: bool) {
        let sheet = self.app.workbook.current_sheet();
        let filled = |r: usize, c: usize| sheet.cells.get(&(r, c)).is_some_and(|cd| !cd.value.is_empty());
        let start = if extend {
            self.app.selection_end.unwrap_or((self.app.selected_row, self.app.selected_col))
        } else {
            (self.app.selected_row, self.app.selected_col)
        };
        let (max_r, max_c) = (sheet.rows - 1, sheet.cols - 1);
        let step = |(r, c): (usize, usize)| -> Option<(usize, usize)> {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            (nr >= 0 && nc >= 0 && nr as usize <= max_r && nc as usize <= max_c).then_some((nr as usize, nc as usize))
        };
        let mut pos = start;
        match step(pos) {
            None => {}
            Some(next) if filled(pos.0, pos.1) && filled(next.0, next.1) => {
                // Inside a block: run to its last filled cell.
                pos = next;
                while let Some(n) = step(pos).filter(|n| filled(n.0, n.1)) {
                    pos = n;
                }
            }
            Some(next) => {
                // At an edge or in a gap: go to the next filled cell.
                pos = next;
                while !filled(pos.0, pos.1) {
                    match step(pos) {
                        Some(n) => pos = n,
                        None => break,
                    }
                }
            }
        }
        if extend {
            let anchor = self.app.selection_start.unwrap_or((self.app.selected_row, self.app.selected_col));
            self.app.selection_start = Some(anchor);
            self.app.selection_end = Some(pos);
            self.grid.scroll_target = Some(pos);
        } else {
            self.set_cursor(pos.0, pos.1);
        }
        self.grid.scroll_to_cursor = true;
    }

    /// Excel's Ctrl+End target: the last used row and column.
    fn last_used_cell(&self) -> (usize, usize) {
        let sheet = self.app.workbook.current_sheet();
        sheet
            .cells
            .iter()
            .filter(|(_, cd)| !cd.value.is_empty() || cd.formula.is_some())
            .fold((0, 0), |(mr, mc), ((r, c), _)| (mr.max(*r), mc.max(*c)))
    }

    fn color_choices(&mut self, ui: &mut egui::Ui, background: bool) {
        if ui.button("None").clicked() {
            if background {
                self.app.set_selection_bg_color(None);
            } else {
                self.app.set_selection_fg_color(None);
            }
        }
        for color in TerminalColor::ALL {
            let (r, g, b) = color.rgb();
            let label = RichText::new(format!("■  {:?}", color)).color(Color32::from_rgb(r, g, b));
            if ui.button(label).clicked() {
                if background {
                    self.app.set_selection_bg_color(Some(color));
                } else {
                    self.app.set_selection_fg_color(Some(color));
                }
            }
        }
    }

    fn formula_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Name box: shows the cursor/selection, accepts "B12" or "A1:C5".
            if !self.name_box_editing {
                self.name_box = self.selection_label();
            }
            let nb = ui.add(
                TextEdit::singleline(&mut self.name_box)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace),
            );
            if nb.gained_focus() {
                self.name_box_editing = true;
            }
            if nb.lost_focus() {
                self.name_box_editing = false;
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    let target = self.name_box.trim().to_uppercase();
                    self.goto(&target);
                }
            }
            ui.label(RichText::new("fx").italics().weak());

            let (row, col) = match &self.edit {
                Some(e) => (e.row, e.col),
                None => (self.app.selected_row, self.app.selected_col),
            };
            let mut shown = match &self.edit {
                Some(e) => e.text.clone(),
                None => self.cell_input_text(row, col),
            };
            let id = egui::Id::new(FORMULA_BAR_ID);
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let color = ui.visuals().text_color();
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                ui.fonts_mut(|f| f.layout_job(crate::grid::formula_layout_job(buf.as_str(), font.clone(), color)))
            };
            let resp = ui.add(
                TextEdit::singleline(&mut shown)
                    .id(id)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .layouter(&mut layouter)
                    .hint_text("Value or =formula"),
            );
            if resp.gained_focus() {
                match &mut self.edit {
                    Some(e) => e.in_cell = false,
                    None => {
                        self.edit = Some(Edit {
                            sheet: self.app.workbook.active_sheet,
                            row,
                            col,
                            text: shown.clone(),
                            in_cell: false,
                            focus_pending: false,
                            ref_start: None,
                        })
                    }
                }
            }
            if resp.changed()
                && let Some(e) = &mut self.edit
            {
                e.text = shown;
                e.ref_start = None;
            }
            if let Some(e) = &mut self.edit
                && !e.in_cell
                && e.focus_pending
            {
                e.focus_pending = false;
                resp.request_focus();
                move_caret_to_end(ui.ctx(), id, e.text.chars().count());
            }
        });
    }

    /// "Decimal places: [n] Apply", like Excel's Format Cells: sets the
    /// number of decimals for every selected cell.
    fn decimal_places(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Decimal places:");
            ui.add(egui::DragValue::new(&mut self.decimal_places).range(0..=cellar::domain::MAX_DECIMALS).speed(0.05));
            if ui.button("Apply").clicked() {
                self.app.set_selection_decimals(self.decimal_places);
                ui.close();
            }
        });
    }

    fn selection_label(&self) -> String {
        let cur = |r: usize, c: usize| format!("{}{}", Spreadsheet::column_label(c), r + 1);
        match self.app.get_selection_range() {
            Some(((r0, c0), (r1, c1))) if (r0, c0) != (r1, c1) => {
                format!("{}:{}", cur(r0, c0), cur(r1, c1))
            }
            _ => cur(self.app.selected_row, self.app.selected_col),
        }
    }

    fn goto(&mut self, target: &str) {
        let (a, b) = target.split_once(':').unwrap_or((target, target));
        match (Spreadsheet::parse_cell_reference(a), Spreadsheet::parse_cell_reference(b)) {
            (Some((r0, c0)), Some((r1, c1))) => {
                self.commit_edit(EditMove::Stay);
                self.set_cursor(r0, c0);
                if (r0, c0) != (r1, c1) {
                    self.grow_to(r1, c1);
                    self.app.selection_start = Some((r0, c0));
                    self.app.selection_end = Some((r1, c1));
                }
                self.grid.scroll_to_cursor = true;
            }
            _ => self.app.status_message = Some(format!("Not a cell reference: {}", target)),
        }
    }

    fn sheet_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let active = self.app.workbook.active_sheet;
            for (idx, name) in self.app.workbook.sheet_names.clone().iter().enumerate() {
                let tab = ui.selectable_label(idx == active, format!("  {}  ", name));
                if tab.clicked() && idx != active {
                    match &mut self.edit {
                        // Typing a formula: show the sheet to click references
                        // on it (Excel's point mode); the formula bar keeps
                        // the text.
                        Some(e) if e.text.starts_with('=') => {
                            e.in_cell = false;
                            e.focus_pending = true;
                            e.ref_start = None;
                        }
                        _ => self.commit_edit(EditMove::Stay),
                    }
                    self.show_sheet(idx);
                }
                if tab.double_clicked() {
                    self.dialogs.open_rename(&self.app);
                }
                tab.context_menu(|ui| {
                    if ui.button("Rename…").clicked() {
                        if idx != self.app.workbook.active_sheet {
                            self.app.switch_to_sheet(idx);
                        }
                        self.dialogs.open_rename(&self.app);
                    }
                    let can_delete = self.app.workbook.sheets.len() > 1;
                    if ui.add_enabled(can_delete, Button::new("Delete")).clicked() {
                        if idx != self.app.workbook.active_sheet {
                            self.app.switch_to_sheet(idx);
                        }
                        self.run_command("sheet delete");
                    }
                });
            }
            if ui.button("+").on_hover_text("New sheet").clicked() {
                self.run_command("sheet new");
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(msg) = &self.app.status_message {
                ui.label(msg);
            } else {
                ui.label(RichText::new("Ready").weak());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.app.get_selection_range().is_some()
                    && let Some((sum, avg, count)) = self.app.get_selection_stats()
                    && count > 0
                {
                    ui.label(format!("Sum: {}   Average: {}   Count: {}", fmt_num(sum), fmt_num(avg), count));
                }
            });
        });
    }

    fn confirm_dialog(&mut self, ctx: &Context) {
        let Some(action) = self.confirm.clone() else { return };
        let mut close = false;
        egui::Modal::new(egui::Id::new("confirm_discard")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label("Do you want to save your changes first?");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.save();
                    if !self.app.dirty {
                        close = true;
                        self.run_guarded(ctx, action.clone());
                    }
                }
                if ui.button("Don't save").clicked() {
                    close = true;
                    self.run_guarded(ctx, action.clone());
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.confirm = None;
        }
    }

    fn update_title(&mut self, ctx: &Context) {
        let name = self
            .app
            .filename
            .as_ref()
            .map(|f| file_name(Path::new(f)))
            .or_else(|| self.suggested_save.as_ref().map(|p| file_name(p)))
            .unwrap_or_else(|| "Untitled".to_string());
        let title = format!("{}{} — Cellar", name, if self.app.dirty { " •" } else { "" });
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.app.dirty {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm = Some(Guarded::Quit);
        }
        // Files dropped onto the window open like File → Open.
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
            if self.app.dirty {
                self.app.status_message =
                    Some("Save or discard your changes before opening another file".to_string());
            } else {
                self.open_path(path);
            }
        }
        // Files opened from Finder (macOS). The window holds one workbook,
        // so open the first; ask to save unsaved changes first.
        if let Some(path) = crate::open_files::take().into_iter().next() {
            self.guarded(&ctx, Guarded::OpenPath(path));
        }

        self.handle_keys(&ctx);
        // Excel zooms with Ctrl + mouse wheel (⌘ + scroll on Mac).
        let zoom = ctx.input(|i| i.zoom_delta());
        if zoom != 1.0 {
            ctx.set_zoom_factor((ctx.zoom_factor() * zoom).clamp(0.5, 3.0));
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(&ctx, ui));
        egui::Panel::top("formula_bar").show(ui, |ui| {
            ui.add_space(2.0);
            self.formula_bar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("tabs").show(ui, |ui| self.sheet_tabs(ui));
        if self.show_sidebar {
            self.sidebar(ui);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.show_grid(ui));

        self.show_dialogs(&ctx);
        self.structure_dialog_ui(&ctx);
        self.confirm_dialog(&ctx);
        self.update_title(&ctx);
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum EditMove {
    Stay,
    Up,
    Down,
    Left,
    Right,
}

/// The Cellar logo (rendered from `assets/logo.svg` by
/// `cargo run --example render_icons`).
pub const LOGO_PNG: &[u8] = include_bytes!("../../assets/icon-256.png");

pub fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(LOGO_PNG).unwrap_or_default()
}

pub fn number_formats() -> Vec<(&'static str, NumberFormat)> {
    vec![
        ("General", NumberFormat::General),
        ("Number (1234.56)", NumberFormat::Number { decimals: 2, thousands_sep: false }),
        ("Number (1,234.56)", NumberFormat::Number { decimals: 2, thousands_sep: true }),
        ("Integer (1,235)", NumberFormat::Number { decimals: 0, thousands_sep: true }),
        ("Currency ($1,234.56)", NumberFormat::Currency { symbol: "$".into(), decimals: 2 }),
        ("Currency (1,234.56 €)", NumberFormat::Currency { symbol: "€".into(), decimals: 2 }),
        ("Percent (12%)", NumberFormat::Percentage { decimals: 0 }),
        ("Percent (12.34%)", NumberFormat::Percentage { decimals: 2 }),
    ]
}

pub fn move_caret_to_end(ctx: &Context, id: egui::Id, chars: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
        let end = egui::text::CCursor::new(chars);
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
        state.store(ctx, id);
    }
}

pub fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{:.4}", n).trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// `dir/name.xlsx` → `dir/name.<ext>`.
pub fn sibling_with_extension(path: &str, ext: &str) -> String {
    Path::new(path).with_extension(ext).to_string_lossy().to_string()
}
