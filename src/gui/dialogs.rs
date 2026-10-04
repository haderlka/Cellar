//! Dialog windows: Excel import report, sheet rename, help. The chart
//! dialog lives in `charts.rs`, the PivotTable dialogs in `pivot_ui.rs`.

use eframe::egui::{self, RichText};
use cellar::application::App;
use cellar::domain::Spreadsheet;
use cellar::infrastructure::xlsx_convert::ConversionReport;

use crate::app::GuiApp;
use crate::charts::ChartDialog;

#[derive(Default)]
pub struct Dialogs {
    pub import_report: Option<ConversionReport>,
    pub chart: Option<ChartDialog>,
    pub rename: Option<String>,
    pub help: bool,
    pub about: bool,
    logo: Option<egui::TextureHandle>,
}

impl Dialogs {
    /// True while a dialog that takes keyboard input is open, so grid
    /// shortcuts don't fire underneath it.
    pub fn any_open(&self) -> bool {
        self.chart.is_some() || self.rename.is_some()
    }

    pub fn open_rename(&mut self, app: &App) {
        self.rename = Some(app.workbook.sheet_names[app.workbook.active_sheet].clone());
    }
}

impl GuiApp {
    pub fn show_dialogs(&mut self, ctx: &egui::Context) {
        self.chart_dialog(ctx);
        self.pivot_dialogs(ctx);
        self.import_report_dialog(ctx);
        self.rename_dialog(ctx);
        self.help_window(ctx);
        self.about_window(ctx);
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.dialogs.about {
            return;
        }
        let logo = self
            .dialogs
            .logo
            .get_or_insert_with(|| {
                let icon = crate::app::window_icon();
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [icon.width as usize, icon.height as usize],
                    &icon.rgba,
                );
                ctx.load_texture("cellar-logo", image, egui::TextureOptions::LINEAR)
            })
            .clone();
        egui::Window::new("About Cellar")
            .open(&mut self.dialogs.about)
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(&logo).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                    ui.heading("Cellar");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.add_space(6.0);
                    ui.label(
                        "A spreadsheet whose workbooks are plain, readable text: \
                         easy to version, diff and merge.",
                    );
                });
                ui.separator();
                ui.label(RichText::new("Built on tshts").strong());
                ui.label(
                    "Cellar's formula language, calculation engine, workbook model, \
                     Excel/CSV import and export, undo and clipboard come from tshts \
                     by Samuel Schlesinger (MIT licence).",
                );
                ui.hyperlink("https://github.com/SamuelSchlesinger/tshts");
                ui.add_space(4.0);
                ui.label(RichText::new("Libraries").strong());
                ui.label(
                    "egui/eframe, egui_plot, rfd, calamine, quick-xml, plotters, \
                     serde, rayon, reqwest, arboard and others: see Cargo.toml.",
                );
            });
    }

    fn import_report_dialog(&mut self, ctx: &egui::Context) {
        let Some(report) = &self.dialogs.import_report else { return };
        let mut open = true;
        let mut save = false;
        let mut jump: Option<(usize, String)> = None;
        let total = report.total_mismatches();
        egui::Window::new("Excel import")
            .open(&mut open)
            .collapsible(false)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.label(format!("Converted {}", report.source));
                ui.add_space(6.0);
                egui::Grid::new("import_sheets").striped(true).num_columns(6).show(ui, |ui| {
                    for h in ["Sheet", "Cells", "Formulas", "Formatted", "Charts", "Differences"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for s in &report.sheets {
                        ui.label(&s.name);
                        ui.label(s.cells.to_string());
                        ui.label(s.formulas.to_string());
                        ui.label(s.formatted_cells.to_string());
                        ui.label(s.charts.to_string());
                        if s.mismatch_count == 0 {
                            ui.label("✔ none");
                        } else {
                            ui.colored_label(egui::Color32::from_rgb(220, 140, 30), s.mismatch_count.to_string());
                        }
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                if total == 0 {
                    ui.label("Every formula gives the same result in Cellar as in Excel.");
                } else {
                    ui.label(format!(
                        "{} formula(s) compute differently in Cellar. Usually a function Cellar doesn't \
                         support (#NAME?). Click a cell to go to it.",
                        total
                    ));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        egui::Grid::new("mismatches").striped(true).num_columns(4).show(ui, |ui| {
                            for h in ["Cell", "Formula", "Excel", "Cellar"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for (si, s) in report.sheets.iter().enumerate() {
                                for m in &s.mismatches {
                                    let label = if report.sheets.len() > 1 {
                                        format!("{}!{}", s.name, m.cell)
                                    } else {
                                        m.cell.clone()
                                    };
                                    if ui.link(label).clicked() {
                                        jump = Some((si, m.cell.clone()));
                                    }
                                    ui.monospace(&m.formula);
                                    ui.label(&m.excel);
                                    ui.label(&m.cellar);
                                    ui.end_row();
                                }
                            }
                        });
                    });
                }
                for w in &report.warnings {
                    ui.colored_label(egui::Color32::from_rgb(220, 140, 30), format!("⚠ {}", w));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save as .cellar…").clicked() {
                        save = true;
                    }
                    if ui.button("Copy report").clicked() {
                        ui.ctx().copy_text(report.summary());
                    }
                });
            });
        if let Some((sheet, cell)) = jump {
            self.app.switch_to_sheet(sheet);
            if let Some((r, c)) = Spreadsheet::parse_cell_reference(&cell) {
                self.set_cursor(r, c);
                self.grid.scroll_to_cursor = true;
            }
        }
        if save {
            self.save_as();
            if !self.app.dirty {
                self.dialogs.import_report = None;
            }
        }
        if !open {
            self.dialogs.import_report = None;
        }
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = &mut self.dialogs.rename else { return };
        let mut done = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("rename_sheet")).show(ctx, |ui| {
            ui.heading("Rename sheet");
            let resp = ui.text_edit_singleline(name);
            resp.request_focus();
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = true;
            }
            ui.horizontal(|ui| {
                if ui.button("Rename").clicked() {
                    done = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if done {
            let name = self.dialogs.rename.take().unwrap_or_default();
            let name = name.trim();
            if !name.is_empty() {
                self.run_command(&format!("rename {}", name));
            }
        } else if cancel {
            self.dialogs.rename = None;
        }
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        use crate::shortcuts::Action;
        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        let cmd = if mac { "⌘" } else { "Ctrl+" };
        let editing: Vec<(String, &str)> = vec![
            ("Type".into(), "Start editing the cell (replaces its content)"),
            ("F2 / double-click".into(), "Edit the cell's current content"),
            ("Enter / Tab".into(), "Commit and move down / right (Shift = back)"),
            (format!("{}Enter", cmd), "Commit into every selected cell"),
            ("Esc".into(), "Cancel the edit"),
            ("While typing =…".into(), "Click or drag cells to insert references"),
            ("Fill handle".into(), "Drag the square at the selection's corner; double-click fills down"),
        ];
        let moving: Vec<(String, &str)> = vec![
            ("Arrows, PgUp / PgDn".into(), "Move (Shift extends the selection)"),
            (format!("{}Arrow", cmd), "Jump to the edge of the data (Shift extends)"),
            ("Home".into(), "Start of the row"),
            (format!("{}Home / {}End", cmd, cmd), "First cell / last used cell"),
        ];
        egui::Window::new("Keyboard Shortcuts & Formulas")
            .open(&mut self.dialogs.help)
            .default_width(560.0)
            .default_height(560.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new("Cellar uses Excel's shortcuts.").weak());
                    let section = |ui: &mut egui::Ui, title: &str, rows: &[(String, &str)]| {
                        ui.add_space(6.0);
                        ui.strong(title);
                        egui::Grid::new(title).striped(true).num_columns(2).min_col_width(170.0).show(ui, |ui| {
                            for (k, d) in rows {
                                ui.monospace(k);
                                ui.label(*d);
                                ui.end_row();
                            }
                        });
                    };
                    section(ui, "Editing", &editing);
                    section(ui, "Moving & selecting", &moving);
                    let commands: Vec<(String, &str)> = Action::ALL
                        .iter()
                        .map(|a| (a.shortcut_text(ctx), a.label()))
                        .filter(|(k, _)| !k.is_empty())
                        .collect();
                    section(ui, "Commands", &commands);
                    ui.separator();
                    ui.label(
                        "Formulas use Excel syntax: =SUM(A1:A10), =IF(B2>0, \"yes\", \"no\"), \
                         =VLOOKUP(…), =XLOOKUP(…), =Sheet2!A1, dynamic arrays like =SORT(A2:A9).",
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Workbooks are saved as .cellar: plain JSON with one line per cell, sorted, \
                             so they diff and merge cleanly in git.",
                        )
                        .weak(),
                    );
                });
            });
    }
}
