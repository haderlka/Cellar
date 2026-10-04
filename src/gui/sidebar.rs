//! The right-hand sidebar: PivotTables (each followed by its PivotCharts)
//! and regular charts, as cards. While a PivotTable is being edited the
//! sidebar shows Excel's "PivotTable Fields" pane above that pivot.

use eframe::egui::{self, RichText, Ui};

use crate::app::GuiApp;

impl GuiApp {
    pub fn sidebar(&mut self, root: &mut Ui) {
        egui::Panel::right("sidebar")
            .resizable(true)
            .default_size(440.0)
            .min_size(300.0)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Analysis");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Hide")
                            .on_hover_text("Show again with View > Sidebar")
                            .clicked()
                        {
                            self.show_sidebar = false;
                        }
                        if ui
                            .button("+ Chart")
                            .on_hover_text("Chart the selected cells")
                            .clicked()
                        {
                            self.open_chart_dialog(None);
                        }
                        if ui
                            .button("+ PivotTable")
                            .on_hover_text("Summarize the selected data")
                            .clicked()
                        {
                            self.open_create_pivot();
                        }
                    });
                });
                ui.separator();

                let pivot_count = self.app.workbook.current_sheet().pivots.len();
                if self.pivots.editing.is_some_and(|i| i >= pivot_count) {
                    self.pivots.editing = None;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(i) = self.pivots.editing {
                            // Editing: field pane, then just this pivot and its charts.
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.field_pane(ui, i);
                            });
                            ui.add_space(6.0);
                            self.pivot_card(ui, i);
                            self.pivot_chart_cards(ui, i);
                            return;
                        }
                        let chart_count = self.app.workbook.current_sheet().charts.len();
                        if pivot_count == 0 && chart_count == 0 {
                            ui.add_space(12.0);
                            ui.label(
                                RichText::new(
                                    "Nothing here yet.\n\n\
                                 + PivotTable summarizes a data range (select it first, with its \
                                 header row).\n\n\
                                 + Chart plots the selected cells (labels in the first column, \
                                 numbers next to it).\n\n\
                                 A PivotChart is created from a PivotTable's card.",
                                )
                                .weak(),
                            );
                            return;
                        }
                        for i in 0..pivot_count {
                            self.pivot_card(ui, i);
                            self.pivot_chart_cards(ui, i);
                        }
                        // Charts not tied to a pivot on this sheet.
                        let pivot_names: Vec<String> = self
                            .app
                            .workbook
                            .current_sheet()
                            .pivots
                            .iter()
                            .map(|p| p.name.clone())
                            .collect();
                        for ci in 0..chart_count {
                            let linked = self.app.workbook.current_sheet().charts[ci]
                                .pivot
                                .as_ref()
                                .is_some_and(|p| pivot_names.contains(p));
                            if !linked {
                                self.chart_card(ui, ci);
                            }
                        }
                    });
            });
    }

    fn pivot_chart_cards(&mut self, ui: &mut Ui, pivot: usize) {
        let Some(name) = self
            .app
            .workbook
            .current_sheet()
            .pivots
            .get(pivot)
            .map(|p| p.name.clone())
        else {
            return;
        };
        let charts: Vec<usize> = self
            .app
            .workbook
            .current_sheet()
            .charts
            .iter()
            .enumerate()
            .filter(|(_, c)| c.pivot.as_deref() == Some(&name))
            .map(|(i, _)| i)
            .collect();
        for ci in charts {
            self.chart_card(ui, ci);
        }
    }

    fn pivot_card(&mut self, ui: &mut Ui, i: usize) {
        let Some(spec) = self.app.workbook.current_sheet().pivots.get(i).cloned() else {
            return;
        };
        let editing = self.pivots.editing == Some(i);
        let mut delete = false;
        let mut chart = false;
        let mut fields = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Buttons first (right-aligned) so a long name truncates
            // instead of running underneath them.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    delete = ui
                        .small_button("Delete")
                        .on_hover_text("Delete the PivotTable and its PivotCharts")
                        .clicked();
                    chart = ui.small_button("PivotChart").clicked();
                    if !editing {
                        fields = ui
                            .small_button("Fields")
                            .on_hover_text("Show the PivotTable Fields pane")
                            .clicked();
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&spec.name).strong()).truncate());
                    });
                });
            });
            ui.label(RichText::new(format!("Data: {}", spec.source)).weak().small());
            self.draw_pivot(ui, i);
        });
        ui.add_space(6.0);
        if fields {
            self.pivots.editing = Some(i);
        }
        if chart {
            self.open_pivot_chart_dialog(&spec);
        }
        if delete {
            self.pivots.editing = None;
            self.app.remove_pivot(i);
        }
    }
}
