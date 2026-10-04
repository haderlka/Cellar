//! Charts: the side panel listing the active sheet's charts, rendering
//! (bar / line / scatter through egui_plot, pie drawn directly), and the
//! insert/edit dialog. Charts are stored as [`ChartSpec`] definitions in
//! the sheet, so they re-read their source ranges every frame.

use std::f32::consts::TAU;

use eframe::egui::{self, pos2, vec2, Color32, ComboBox, Mesh, RichText, Sense, Stroke, Ui};
use egui_plot::{Bar, BarChart, Legend, Line, Plot, PlotPoints, Points};
use cellar::domain::{numbers, range_chart_data, ChartData, ChartSeries, ChartSpec, ChartType, PivotSpec, Spreadsheet};
use cellar::infrastructure::chart_image;

use crate::app::GuiApp;

/// Tableau 10 — distinguishable in light and dark themes.
const SERIES_COLORS: [Color32; 8] = [
    Color32::from_rgb(78, 121, 167),
    Color32::from_rgb(242, 142, 43),
    Color32::from_rgb(225, 87, 89),
    Color32::from_rgb(118, 183, 178),
    Color32::from_rgb(89, 161, 79),
    Color32::from_rgb(237, 201, 72),
    Color32::from_rgb(176, 122, 161),
    Color32::from_rgb(255, 157, 167),
];

fn series_color(i: usize) -> Color32 {
    SERIES_COLORS[i % SERIES_COLORS.len()]
}

/// State of the insert/edit chart window.
pub struct ChartDialog {
    /// `Some(i)` when editing chart `i` of the active sheet.
    pub index: Option<usize>,
    pub spec: ChartSpec,
    pub categories: String,
}

pub fn draw_chart_data(ui: &mut Ui, chart_type: ChartType, data: &ChartData, id: egui::Id, height: f32) {
    let cats = data.categories.clone();
    let series = data.series.clone();
    if series.iter().all(|(_, v)| v.iter().all(Option::is_none)) {
        ui.label(RichText::new("No numeric data in the chart's ranges.").weak());
        return;
    }
    if chart_type == ChartType::Pie {
        draw_pie(ui, &cats, &series[0].1, height);
        return;
    }

    let n_points = series.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
    let labels = cats.clone();
    let plot = Plot::new(id)
        .height(height)
        .legend(Legend::default().follow_insertion_order(true))
        .allow_scroll(false)
        .allow_zoom(false)
        .allow_drag(false)
        .show_grid([false, true]);
    let numeric_x = numbers(&cats);
    let plot = if chart_type == ChartType::Scatter && numeric_x.iter().all(Option::is_some) && !cats.is_empty() {
        plot
    } else {
        // One grid mark per category (thinned when there are many) so
        // every bar/point gets its label.
        let step = (n_points as f64 / 20.0).ceil().max(1.0);
        plot.x_grid_spacer(move |input| {
            let (lo, hi) = input.bounds;
            let (first, last) = ((lo / step).ceil() as i64, (hi / step).floor() as i64);
            (first..=last.max(first - 1))
                .map(|i| egui_plot::GridMark { value: i as f64 * step, step_size: step })
                .collect()
        })
        .custom_x_axes(vec![
            // egui_plot hides x labels closer than 60pt apart by default;
            // category names are short, so allow them much closer.
            egui_plot::AxisHints::new_x().label_spacing(24.0..=36.0).formatter(move |mark, _range| {
                let i = mark.value.round();
                if (mark.value - i).abs() < 1e-6 && i >= 0.0 && (i as usize) < n_points {
                    labels.get(i as usize).cloned().unwrap_or_else(|| (i as usize + 1).to_string())
                } else {
                    String::new()
                }
            }),
        ])
    };
    plot.show(ui, move |plot_ui| {
        let count = series.len().max(1) as f64;
        let width = 0.8 / count;
        for (si, (name, values)) in series.into_iter().enumerate() {
            let color = series_color(si);
            match chart_type {
                ChartType::Bar | ChartType::Pie => {
                    let bars = values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| {
                            let x = i as f64 + (si as f64 - (count - 1.0) / 2.0) * width;
                            v.map(|y| Bar::new(x, y).width(width * 0.92).fill(color))
                        })
                        .collect();
                    plot_ui.bar_chart(BarChart::new(name, bars).color(color));
                }
                ChartType::Line => {
                    let pts: Vec<[f64; 2]> = values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| v.map(|y| [i as f64, y]))
                        .collect();
                    plot_ui.line(Line::new(name.clone(), PlotPoints::new(pts.clone())).color(color).width(2.0));
                    plot_ui.points(Points::new(name, PlotPoints::new(pts)).color(color).radius(3.0));
                }
                ChartType::Scatter => {
                    let pts: Vec<[f64; 2]> = values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, v)| {
                            let x = numeric_x.get(i).copied().flatten().unwrap_or(i as f64);
                            v.map(|y| [x, y])
                        })
                        .collect();
                    plot_ui.points(Points::new(name, PlotPoints::new(pts)).color(color).radius(4.0));
                }
            }
        }
    });
}

fn draw_pie(ui: &mut Ui, cats: &[String], values: &[Option<f64>], height: f32) {
    let slices: Vec<(String, f64)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let v = (*v)?;
            (v > 0.0).then(|| (cats.get(i).cloned().unwrap_or_else(|| format!("{}", i + 1)), v))
        })
        .collect();
    let total: f64 = slices.iter().map(|(_, v)| v).sum();
    if total <= 0.0 {
        ui.label(RichText::new("A pie chart needs positive values.").weak());
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    let radius = (rect.height() / 2.0 - 8.0).min(rect.width() * 0.3).max(10.0);
    let center = pos2(rect.min.x + radius + 10.0, rect.center().y);
    let mut mesh = Mesh::default();
    let mut angle = -TAU / 4.0;
    for (i, (_, v)) in slices.iter().enumerate() {
        let sweep = (*v / total) as f32 * TAU;
        let steps = ((sweep / TAU) * 96.0).ceil().max(2.0) as usize;
        let color = series_color(i);
        let c = mesh.vertices.len() as u32;
        mesh.colored_vertex(center, color);
        for s in 0..=steps {
            let a = angle + sweep * s as f32 / steps as f32;
            mesh.colored_vertex(center + radius * vec2(a.cos(), a.sin()), color);
        }
        for s in 0..steps as u32 {
            mesh.add_triangle(c, c + 1 + s, c + 2 + s);
        }
        angle += sweep;
    }
    painter.add(mesh);
    painter.circle_stroke(center, radius, Stroke::new(1.0, ui.visuals().window_stroke.color));

    // Legend with percentages.
    let mut y = rect.min.y + 8.0;
    let x = center.x + radius + 16.0;
    for (i, (label, v)) in slices.iter().enumerate() {
        if y > rect.max.y - 14.0 {
            break;
        }
        painter.rect_filled(egui::Rect::from_min_size(pos2(x, y + 2.0), vec2(10.0, 10.0)), 2.0, series_color(i));
        painter.text(
            pos2(x + 16.0, y),
            egui::Align2::LEFT_TOP,
            format!("{}  {:.1}%", label, v / total * 100.0),
            egui::FontId::proportional(12.0),
            ui.visuals().text_color(),
        );
        y += 18.0;
    }
}

/// A chart definition guessed from the selection: a text first column
/// becomes the categories, a text first row the series names.
fn chart_from_selection(gui: &GuiApp) -> ChartSpec {
    let sheet = gui.app.workbook.current_sheet();
    let ((r0, c0), (r1, c1)) = gui.selection_or_cursor();
    let is_num = |r: usize, c: usize| sheet.get_cell(r, c).value.trim().parse::<f64>().is_ok();
    let is_blank = |r: usize, c: usize| sheet.get_cell(r, c).value.trim().is_empty();
    let label = |r: usize, c: usize| format!("{}{}", Spreadsheet::column_label(c), r + 1);
    let range = |ra: usize, ca: usize, rb: usize, cb: usize| {
        if (ra, ca) == (rb, cb) { label(ra, ca) } else { format!("{}:{}", label(ra, ca), label(rb, cb)) }
    };

    let first_col_is_text = c1 > c0 && ((r0 + 1)..=r1).any(|r| !is_num(r, c0) && !is_blank(r, c0));
    // Series are the columns holding numbers; other text columns (e.g. a
    // "Month" next to "Region") can't be plotted.
    let value_cols: Vec<usize> = (c0..=c1)
        .filter(|&c| !(first_col_is_text && c == c0))
        .filter(|&c| (r0..=r1).any(|r| is_num(r, c)))
        .collect();
    let has_header = r1 > r0 && value_cols.iter().any(|&c| !is_num(r0, c) && !is_blank(r0, c));
    let data_r0 = if has_header { r0 + 1 } else { r0 };

    let series = value_cols
        .iter()
        .map(|&c| ChartSeries {
            name: has_header.then(|| sheet.get_cell(r0, c).value.clone()).filter(|n| !n.is_empty()),
            values: range(data_r0, c, r1, c),
        })
        .collect::<Vec<_>>();
    let title = if has_header && first_col_is_text {
        let t = sheet.get_cell(r0, c0).value.clone();
        if t.is_empty() { "Chart".to_string() } else { t }
    } else if let Some(name) = series.first().and_then(|s| s.name.clone()) {
        name
    } else {
        "Chart".to_string()
    };
    ChartSpec {
        title,
        chart_type: ChartType::Bar,
        pivot: None,
        categories: first_col_is_text.then(|| range(data_r0, c0, r1, c0)),
        series,
    }
}

impl GuiApp {
    pub fn open_chart_dialog(&mut self, index: Option<usize>) {
        let spec = match index {
            Some(i) => self.app.workbook.current_sheet().charts[i].clone(),
            None => chart_from_selection(self),
        };
        self.show_sidebar = true;
        self.dialogs.chart = Some(ChartDialog {
            index,
            categories: spec.categories.clone().unwrap_or_default(),
            spec,
        });
    }

    /// Excel's Insert PivotChart: a chart whose data is the pivot's result.
    pub fn open_pivot_chart_dialog(&mut self, pivot: &PivotSpec) {
        let title = match pivot.values.as_slice() {
            [v] => v.display_name(),
            _ => pivot.name.clone(),
        };
        self.show_sidebar = true;
        self.dialogs.chart = Some(ChartDialog {
            index: None,
            categories: String::new(),
            spec: ChartSpec {
                title,
                chart_type: ChartType::Bar,
                pivot: Some(pivot.name.clone()),
                categories: None,
                series: Vec::new(),
            },
        });
    }

    /// Resolve a chart's data: from its PivotTable or from its ranges.
    pub fn chart_data(&mut self, spec: &ChartSpec) -> Result<ChartData, String> {
        let host = self.app.workbook.active_sheet;
        let Some(pivot) = &spec.pivot else {
            return Ok(range_chart_data(&self.app.workbook, host, spec));
        };
        let idx = self
            .app
            .workbook
            .current_sheet()
            .pivots
            .iter()
            .position(|p| &p.name == pivot)
            .ok_or_else(|| format!("PivotTable {} not found", pivot))?;
        let entry = self.pivot_entry(idx).ok_or("PivotTable missing")?;
        let out = entry.output.as_ref().map_err(Clone::clone)?;
        if out.chart.series.is_empty() {
            return Err("Add a field to the PivotTable's Values area to chart it.".into());
        }
        Ok(ChartData { categories: out.chart.categories.clone(), series: out.chart.series.clone() })
    }

    /// Save one chart as an image; the format follows the chosen extension.
    pub fn export_chart_file(&mut self, spec: &ChartSpec) {
        let data = match self.chart_data(spec) {
            Ok(d) => d,
            Err(e) => {
                self.app.status_message = Some(format!("Can't export \"{}\": {}", spec.title, e));
                return;
            }
        };
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .add_filter("SVG image", &["svg"])
            .set_file_name(format!("{}.png", spec.title.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")))
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        self.app.status_message = Some(match chart_image::export_chart(spec, &data, &path, chart_image::DEFAULT_SIZE) {
            Ok(()) => format!("Saved chart to {}", path.display()),
            Err(e) => e,
        });
    }

    /// Save every chart of every sheet into a folder as PNG or SVG files.
    pub fn export_all_charts(&mut self, ext: &str) {
        if self.app.workbook.sheets.iter().all(|s| s.charts.is_empty()) {
            self.app.status_message = Some("There are no charts to export".into());
            return;
        }
        let Some(dir) = rfd::FileDialog::new().set_title("Choose a folder for the chart images").pick_folder() else {
            return;
        };
        self.app.status_message = Some(match chart_image::export_all_charts(&self.app.workbook, &dir, ext) {
            Ok(r) if r.failed.is_empty() => format!("Exported {} chart(s) to {}", r.written.len(), dir.display()),
            Ok(r) => format!(
                "Exported {} chart(s); skipped {}: {}",
                r.written.len(),
                r.failed.len(),
                r.failed.iter().map(|(c, e)| format!("{} ({})", c, e)).collect::<Vec<_>>().join("; ")
            ),
            Err(e) => e,
        });
    }

    /// One chart card in the sidebar.
    pub fn chart_card(&mut self, ui: &mut Ui, i: usize) {
        let host = self.app.workbook.active_sheet;
        let Some(spec) = self.app.workbook.current_sheet().charts.get(i).cloned() else { return };
        let mut edit = false;
        let mut delete = false;
        let mut export = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Buttons first (right-aligned) so a long title truncates
            // instead of running underneath them.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    delete = ui.small_button("Delete").clicked();
                    export = ui.small_button("Export…").on_hover_text("Save as PNG or SVG").clicked();
                    edit = ui.small_button("Edit").clicked();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&spec.title).strong()).truncate());
                    });
                });
            });
            if let Some(p) = &spec.pivot {
                ui.label(RichText::new(format!("PivotChart · {}", p)).weak().small());
            }
            match self.chart_data(&spec) {
                Ok(data) => draw_chart_data(ui, spec.chart_type, &data, egui::Id::new(("chart", host, i)), 220.0),
                Err(e) => {
                    ui.label(RichText::new(e).weak());
                }
            }
        });
        ui.add_space(6.0);
        if export {
            self.export_chart_file(&spec);
        }
        if edit {
            self.open_chart_dialog(Some(i));
        }
        if delete {
            self.app.remove_chart(i);
        }
    }

    pub fn chart_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.dialogs.chart.take() else { return };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let is_pivot = dlg.spec.pivot.is_some();

        // The preview's data is resolved before the window borrows the
        // dialog state.
        let mut preview = dlg.spec.clone();
        if !is_pivot {
            preview.categories = (!dlg.categories.trim().is_empty()).then(|| dlg.categories.trim().to_string());
            preview.series.retain(|s| !s.values.trim().is_empty());
        }
        let preview_data = if !is_pivot && preview.series.is_empty() {
            Err("Add at least one values range.".to_string())
        } else {
            self.chart_data(&preview)
        };

        let title = match (dlg.index.is_some(), is_pivot) {
            (true, _) => "Edit chart",
            (false, true) => "Insert PivotChart",
            (false, false) => "Insert chart",
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                egui::Grid::new("chart_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Title");
                    ui.text_edit_singleline(&mut dlg.spec.title);
                    ui.end_row();
                    ui.label("Type");
                    // Excel PivotCharts can't be XY (scatter) charts.
                    let types: &[ChartType] = if is_pivot {
                        &[ChartType::Bar, ChartType::Line, ChartType::Pie]
                    } else {
                        &[ChartType::Bar, ChartType::Line, ChartType::Pie, ChartType::Scatter]
                    };
                    ComboBox::from_id_salt("chart_type")
                        .selected_text(type_name(dlg.spec.chart_type))
                        .show_ui(ui, |ui| {
                            for t in types {
                                ui.selectable_value(&mut dlg.spec.chart_type, *t, type_name(*t));
                            }
                        });
                    ui.end_row();
                    if let Some(p) = &dlg.spec.pivot {
                        ui.label("Data");
                        ui.label(format!("PivotTable {}", p));
                        ui.end_row();
                    } else {
                        ui.label("Labels range");
                        ui.add(egui::TextEdit::singleline(&mut dlg.categories).hint_text("e.g. A2:A10 (optional)"));
                        ui.end_row();
                    }
                });
                if is_pivot {
                    ui.label(
                        RichText::new(
                            "Row items become the categories, column items x values the series. \
                             The chart follows the PivotTable's fields, filters and sorting.",
                        )
                        .weak(),
                    );
                } else {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Series").strong());
                    let mut remove = None;
                    egui::Grid::new("chart_series").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                        ui.label(RichText::new("Name").weak());
                        ui.label(RichText::new("Values range").weak());
                        ui.end_row();
                        for (i, s) in dlg.spec.series.iter_mut().enumerate() {
                            let mut name = s.name.clone().unwrap_or_default();
                            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0)).changed() {
                                s.name = (!name.is_empty()).then_some(name);
                            }
                            ui.add(egui::TextEdit::singleline(&mut s.values).desired_width(160.0).hint_text("B2:B10"));
                            if ui.small_button("Remove").clicked() {
                                remove = Some(i);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(i) = remove {
                        dlg.spec.series.remove(i);
                    }
                    if ui.button("+ Add series").clicked() {
                        dlg.spec.series.push(ChartSeries { name: None, values: String::new() });
                    }
                }
                ui.separator();
                match &preview_data {
                    Ok(data) => draw_chart_data(ui, dlg.spec.chart_type, data, egui::Id::new("chart_preview"), 200.0),
                    Err(e) => {
                        ui.label(RichText::new(e).weak());
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ok = preview_data.is_ok();
                    if ui.add_enabled(ok, egui::Button::new(if dlg.index.is_some() { "Save" } else { "Insert" })).clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            let mut spec = dlg.spec;
            if !is_pivot {
                spec.categories = preview.categories;
                spec.series = preview.series;
            }
            match dlg.index {
                Some(i) => self.app.replace_chart(i, spec),
                None => self.app.add_chart(spec),
            }
        } else if !(cancel || !open) {
            self.dialogs.chart = Some(dlg);
        }
    }
}

fn type_name(t: ChartType) -> &'static str {
    match t {
        ChartType::Bar => "Column",
        ChartType::Line => "Line",
        ChartType::Pie => "Pie",
        ChartType::Scatter => "Scatter",
    }
}
