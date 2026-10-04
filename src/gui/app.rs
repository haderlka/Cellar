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
use cellar::infrastructure::{recent, xlsx, xlsx_convert, FileRepository};

use crate::dialogs::Dialogs;
use crate::grid::GridState;

pub const FORMULA_BAR_ID: &str = "formula_bar";

/// An in-progress cell edit. The text is shared by the in-cell editor and
/// the formula bar; `in_cell` says which one has the caret.
pub struct Edit {
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
    ImportExcel,
    Quit,
}

pub struct GuiApp {
    pub app: App,
    pub edit: Option<Edit>,
    pub grid: GridState,
    pub dialogs: Dialogs,
    pub pivots: crate::pivot_ui::PivotUi,
    pub show_sidebar: bool,
    /// Suggested path for the first "Save" after importing a non-.cellar
    /// file (xlsx/csv): same folder, same name, `.cellar` extension.
    pub suggested_save: Option<PathBuf>,
    name_box: String,
    name_box_editing: bool,
    confirm: Option<Guarded>,
    allow_close: bool,
    window_title: String,
}

impl GuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<String>) -> Self {
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = true);
        let mut gui = Self {
            app: App::default(),
            edit: None,
            grid: GridState::default(),
            dialogs: Dialogs::default(),
            pivots: Default::default(),
            show_sidebar: true,
            suggested_save: None,
            name_box: String::new(),
            name_box_editing: false,
            confirm: None,
            allow_close: false,
            window_title: String::new(),
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
        match extension(&path).as_str() {
            "xlsx" | "xlsm" => self.import_excel(path),
            "csv" | "tsv" | "txt" => match CsvExporter::import_from_csv(&p) {
                Ok(sheet) => {
                    let mut wb = Workbook::from_spreadsheet(sheet);
                    wb.build_dep_graph_from_scratch();
                    self.load_foreign(wb, &path);
                }
                Err(e) => self.app.status_message = Some(format!("Could not open {}: {}", p, e)),
            },
            _ => {
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

    pub fn import_excel(&mut self, path: PathBuf) {
        let p = path.to_string_lossy().to_string();
        match xlsx_convert::convert_xlsx(&p) {
            Ok((wb, report)) => {
                self.load_foreign(wb, &path);
                self.app.status_message = Some(format!(
                    "Imported {} — save it as .cellar to keep working in text format",
                    file_name(&path)
                ));
                self.dialogs.import_report = Some(report);
            }
            Err(e) => self.app.status_message = Some(format!("Excel import failed: {}", e)),
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
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Spreadsheets", &["cellar", "xlsx", "xlsm", "csv"])
                    .add_filter("Cellar workbook", &["cellar"])
                    .add_filter("Excel workbook", &["xlsx", "xlsm"])
                    .add_filter("CSV", &["csv"])
                    .pick_file()
                {
                    self.open_path(path);
                }
            }
            Guarded::OpenPath(path) => self.open_path(path),
            Guarded::ImportExcel => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Excel workbook", &["xlsx", "xlsm"])
                    .pick_file()
                {
                    self.import_excel(path);
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
        self.edit = Some(Edit { row, col, text, in_cell, focus_pending: true, ref_start: None });
        self.grid.scroll_to_cursor = true;
    }

    /// Write the pending edit through Cellar's normal commit path (formula
    /// evaluation, cycle checks, undo) and move the cursor.
    pub fn commit_edit(&mut self, mv: EditMove) {
        let Some(edit) = self.edit.take() else { return };
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
        self.edit = None;
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
        let sheet = self.app.workbook.current_sheet_mut();
        if row + 1 >= sheet.rows {
            sheet.rows = row + 50;
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
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);

        // Global shortcuts (work while typing too).
        if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::S))) {
            self.save_as();
        } else if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S))) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::O))) {
            self.guarded(ctx, Guarded::Open);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::N))) {
            self.guarded(ctx, Guarded::New);
        }

        if self.edit.is_some() {
            self.handle_edit_keys(ctx, typing);
            return;
        }
        if typing || self.dialogs.any_open() || self.pivots.any_dialog_open() {
            return;
        }

        if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::Z)))
            || ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Y)))
        {
            self.app.redo();
        } else if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Z))) {
            self.app.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::B))) {
            self.app.toggle_bold();
        }
        // Excel: Ctrl+D fills down, Ctrl+R fills right.
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::D))) {
            self.app.fill_down_or_right(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::R))) {
            self.app.fill_down_or_right(false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::U))) {
            self.app.toggle_underline();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::A))) {
            let sheet = self.app.workbook.current_sheet();
            self.app.selection_start = Some((0, 0));
            self.app.selection_end = Some((sheet.rows - 1, sheet.cols - 1));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Home))) {
            self.set_cursor(0, 0);
            self.grid.scroll_to_cursor = true;
        }

        let mut start_text: Option<String> = None;
        let mut clipboard_op = None;
        let (mods, pressed) = ctx.input_mut(|i| {
            for ev in &i.events {
                match ev {
                    egui::Event::Copy => clipboard_op = Some("copy"),
                    egui::Event::Cut => clipboard_op = Some("cut"),
                    egui::Event::Paste(_) => clipboard_op = Some("paste"),
                    egui::Event::Text(t) if !i.modifiers.command => {
                        start_text.get_or_insert_with(String::new).push_str(t);
                    }
                    _ => {}
                }
            }
            let keys = [
                Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight, Key::Enter,
                Key::Tab, Key::F2, Key::Delete, Key::Backspace, Key::PageUp, Key::PageDown,
                Key::Home, Key::Escape,
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
        for key in pressed {
            match key {
                Key::ArrowUp => self.move_cursor(-1, 0, shift),
                Key::ArrowDown => self.move_cursor(1, 0, shift),
                Key::ArrowLeft => self.move_cursor(0, -1, shift),
                Key::ArrowRight => self.move_cursor(0, 1, shift),
                Key::PageUp => self.move_cursor(-25, 0, shift),
                Key::PageDown => self.move_cursor(25, 0, shift),
                Key::Enter => self.move_cursor(if shift { -1 } else { 1 }, 0, false),
                Key::Tab => self.move_cursor(0, if shift { -1 } else { 1 }, false),
                Key::Home => {
                    let r = self.app.selected_row;
                    self.set_cursor(r, 0);
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
                if ui.add(Button::new("New").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::N)))).clicked() {
                    self.guarded(ctx, Guarded::New);
                }
                if ui.add(Button::new("Open…").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::O)))).clicked() {
                    self.guarded(ctx, Guarded::Open);
                }
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
                if ui.button("Import Excel workbook…").clicked() {
                    self.guarded(ctx, Guarded::ImportExcel);
                }
                ui.separator();
                if ui.add(Button::new("Save").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))).clicked() {
                    self.save();
                }
                if ui.add(Button::new("Save As…").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)))).clicked() {
                    self.save_as();
                }
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
                if ui.button("Quit").clicked() {
                    self.guarded(ctx, Guarded::Quit);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo").clicked() {
                    self.app.undo();
                }
                if ui.button("Redo").clicked() {
                    self.app.redo();
                }
                ui.separator();
                if ui.button("Cut").clicked() {
                    self.app.cut_selection();
                }
                if ui.button("Copy").clicked() {
                    self.app.copy_selection();
                }
                if ui.button("Paste").clicked() {
                    self.app.paste();
                }
                if ui.button("Clear contents").clicked() {
                    self.app.clear_selection_contents();
                }
                ui.separator();
                if ui.button("Copy selection as Markdown table").clicked() {
                    self.copy_selection_markdown(ui.ctx());
                }
                ui.separator();
                if ui.add(Button::new("Fill Down").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::D)))).clicked() {
                    self.app.fill_down_or_right(true);
                }
                if ui.add(Button::new("Fill Right").shortcut_text(ctx.format_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::R)))).clicked() {
                    self.app.fill_down_or_right(false);
                }
                if ui.button("Fill selection (continue series)").clicked() {
                    self.app.autofill_selection();
                }
                ui.separator();
                self.row_col_buttons(ui);
            });
            ui.menu_button("Format", |ui| self.format_menu(ui));
            ui.menu_button("Data", |ui| {
                if ui.button("Sort column A-Z").clicked() {
                    self.app.sort_column_asc();
                }
                if ui.button("Sort column Z-A").clicked() {
                    self.app.sort_column_desc();
                }
                ui.separator();
                if ui.button("PivotTable…").clicked() {
                    self.open_create_pivot();
                }
                if ui.button("Insert chart…").clicked() {
                    self.open_chart_dialog(None);
                }
                ui.separator();
                if ui.button("Recalculate all").clicked() {
                    self.app.recalc_all();
                }
            });
            ui.menu_button("Sheet", |ui| {
                if ui.button("New sheet").clicked() {
                    self.run_command("sheet new");
                }
                if ui.button("Rename sheet…").clicked() {
                    self.dialogs.open_rename(&self.app);
                }
                let can_delete = self.app.workbook.sheets.len() > 1;
                if ui.add_enabled(can_delete, Button::new("Delete sheet")).clicked() {
                    self.run_command("sheet delete");
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut self.show_sidebar, "Sidebar (PivotTables & charts)");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts & formulas").clicked() {
                    self.dialogs.help = true;
                }
                ui.separator();
                if ui.button("About Cellar").clicked() {
                    self.dialogs.about = true;
                }
            });
        });
    }

    pub fn row_col_buttons(&mut self, ui: &mut egui::Ui) {
        if ui.button("Insert row above").clicked() {
            self.app.insert_row();
        }
        if ui.button("Delete row").clicked() {
            self.app.delete_row();
        }
        if ui.button("Insert column left").clicked() {
            self.app.insert_col();
        }
        if ui.button("Delete column").clicked() {
            self.app.delete_col();
        }
    }

    fn format_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Bold").clicked() {
            self.app.toggle_bold();
        }
        if ui.button("Underline").clicked() {
            self.app.toggle_underline();
        }
        ui.separator();
        ui.menu_button("Number format", |ui| {
            for (label, fmt) in number_formats() {
                if ui.button(label).clicked() {
                    self.app.set_selection_format(fmt);
                }
            }
        });
        ui.menu_button("Text color", |ui| self.color_choices(ui, false));
        ui.menu_button("Fill color", |ui| self.color_choices(ui, true));
        ui.separator();
        if ui.button("Auto-fit column width").clicked() {
            let c = self.app.selected_col;
            self.app.workbook.current_sheet_mut().auto_resize_column(c);
            self.app.dirty = true;
        }
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
            let resp = ui.add(
                TextEdit::singleline(&mut shown)
                    .id(id)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("Value or =formula"),
            );
            if resp.gained_focus() {
                match &mut self.edit {
                    Some(e) => e.in_cell = false,
                    None => {
                        self.edit = Some(Edit {
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
                    self.commit_edit(EditMove::Stay);
                    self.app.snapshot_view_state_to_active_sheet();
                    self.app.switch_to_sheet(idx);
                    self.app.restore_view_state_from_active_sheet();
                    self.grid = GridState::default();
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

        self.handle_keys(&ctx);

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
        ("Currency ($)", NumberFormat::Currency { symbol: "$".into(), decimals: 2 }),
        ("Currency (€)", NumberFormat::Currency { symbol: "€".into(), decimals: 2 }),
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
