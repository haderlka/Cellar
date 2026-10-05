//! Dialog windows: Excel import report, sheet rename, help. The chart
//! dialog lives in `charts.rs`, the PivotTable dialogs in `pivot_ui.rs`.

use eframe::egui::{self, RichText};
use cellar::application::App;
use cellar::domain::Spreadsheet;
use cellar::infrastructure::xlsx_convert::ConversionReport;

use crate::app::GuiApp;
use crate::charts::ChartDialog;

const REPO_URL: &str = "https://github.com/haderlka/Cellar";
const COFFEE_URL: &str = "https://buymeacoffee.com/haderlka";

/// Buy Me a Coffee's yellow button, drawn rather than loaded from their
/// CDN so the About window works offline. egui's font has no cup glyph,
/// so the cup is painted.
fn coffee_button(ui: &mut egui::Ui) -> egui::Response {
    let font = egui::FontId::proportional(15.0);
    let ink = egui::Color32::BLACK;
    let galley = ui.painter().layout_no_wrap("Buy me a coffee".to_owned(), font, ink);
    let size = egui::vec2(galley.size().x + 52.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if response.hovered() {
        egui::Color32::from_rgb(255, 228, 51)
    } else {
        egui::Color32::from_rgb(255, 221, 0)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, fill);

    // Cup: body, handle and two wisps of steam.
    let stroke = egui::Stroke::new(1.6, ink);
    let base = egui::pos2(rect.min.x + 14.0, rect.center().y + 7.0);
    let cup = [
        base + egui::vec2(0.0, -11.0),
        base + egui::vec2(12.0, -11.0),
        base + egui::vec2(10.5, 0.0),
        base + egui::vec2(1.5, 0.0),
    ];
    painter.add(egui::Shape::convex_polygon(cup.to_vec(), egui::Color32::WHITE, stroke));
    painter.circle_stroke(base + egui::vec2(13.0, -6.5), 3.0, stroke);
    for dx in [3.5, 8.0] {
        let x = base.x + dx;
        painter.line_segment([egui::pos2(x, base.y - 14.0), egui::pos2(x + 1.5, base.y - 17.0)], stroke);
        painter.line_segment([egui::pos2(x + 1.5, base.y - 17.0), egui::pos2(x, base.y - 20.0)], stroke);
    }

    painter.galley(egui::pos2(rect.min.x + 38.0, rect.center().y - galley.size().y / 2.0), galley, ink);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(COFFEE_URL)
}

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
                ui.label(RichText::new("Source code").strong());
                ui.label("Cellar is free and open source. Report issues or follow development on GitHub:");
                ui.hyperlink(REPO_URL);
                ui.add_space(4.0);
                ui.label(RichText::new("Support Cellar").strong());
                ui.label("If Cellar saves you time, you can support its development with a coffee:");
                ui.add_space(2.0);
                if coffee_button(ui).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(COFFEE_URL));
                }
                ui.add_space(4.0);
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
        egui::Window::new("Import report")
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
