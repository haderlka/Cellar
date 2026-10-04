//! PivotTables in the sidebar, modelled on Excel:
//! - "Create PivotTable" dialog (source range + name)
//! - the "PivotTable Fields" pane: field list with checkboxes and search,
//!   the Filters / Columns / Rows / Values areas with drag & drop, and the
//!   per-field menu (Move Up/Down/…, Move to …, Remove Field, Settings)
//! - Value Field Settings (Summarize Values By / Show Values As / number
//!   format), Field Settings (subtotals, sort, Top 10, item filter) and
//!   PivotTable Options (layout, totals, empty cells, data source)
//! - the rendered table with report-filter dropdowns and the Row/Column
//!   Labels sort-and-filter dropdowns.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use eframe::egui::{
    self, Color32, ComboBox, FontId, Frame, Margin, PopupCloseBehavior, Rect, RichText, Sense,
    Stroke, Ui, pos2, vec2,
};
use cellar::domain::{
    NumberFormat, PivotCellKind, PivotData, PivotField, PivotLayout, PivotOutput, PivotSort,
    PivotSpec, PivotValue, ShowValuesAs, Spreadsheet, Summarize, TopFilter, compute_pivot,
};

use crate::app::GuiApp;

/// One of the four drop areas of the field pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Area {
    Filters,
    Columns,
    Rows,
    Values,
}

impl Area {
    fn title(self) -> &'static str {
        match self {
            Area::Filters => "Filters",
            Area::Columns => "Columns",
            Area::Rows => "Rows",
            Area::Values => "Values",
        }
    }
}

/// What is being dragged in the field pane.
#[derive(Clone, Debug)]
enum DragField {
    /// A field from the field list.
    List(String),
    /// A chip already in an area.
    Chip(Area, usize),
    /// The Σ Values pseudo-field.
    SigmaValues,
}

/// Cached computation for one pivot.
pub struct PivotEntry {
    spec: PivotSpec,
    data_hash: u64,
    pub data: Result<PivotData, String>,
    pub output: Result<PivotOutput, String>,
}

#[derive(Default)]
pub struct PivotUi {
    /// Pivot (index on the active sheet) whose field pane is open.
    pub editing: Option<usize>,
    search: String,
    pub create: Option<CreateDialog>,
    value_settings: Option<ValueSettingsDialog>,
    field_settings: Option<FieldSettingsDialog>,
    options: Option<OptionsDialog>,
    cache: HashMap<(usize, String), Rc<PivotEntry>>,
}

impl PivotUi {
    pub fn any_dialog_open(&self) -> bool {
        self.create.is_some()
            || self.value_settings.is_some()
            || self.field_settings.is_some()
            || self.options.is_some()
    }
}

pub struct CreateDialog {
    source: String,
    name: String,
    error: Option<String>,
}

struct ValueSettingsDialog {
    pivot: usize,
    index: usize,
    draft: PivotValue,
    show_as_tab: bool,
}

struct FieldSettingsDialog {
    pivot: usize,
    area: Area,
    index: usize,
    draft: PivotField,
}

struct OptionsDialog {
    pivot: usize,
    draft: PivotSpec,
    error: Option<String>,
}

// ------------------------------------------------------------ spec edits

fn axis_mut(spec: &mut PivotSpec, area: Area) -> &mut Vec<PivotField> {
    match area {
        Area::Filters => &mut spec.filters,
        Area::Columns => &mut spec.columns,
        Area::Rows => &mut spec.rows,
        Area::Values => unreachable!("values are not PivotFields"),
    }
}

fn area_len(spec: &PivotSpec, area: Area) -> usize {
    match area {
        Area::Filters => spec.filters.len(),
        Area::Columns => spec.columns.len(),
        Area::Rows => spec.rows.len(),
        Area::Values => spec.values.len(),
    }
}

fn default_summarize(data: Option<&PivotData>, field: &str) -> Summarize {
    // Excel: Sum for all-numeric fields, Count otherwise.
    if data.is_some_and(|d| d.is_numeric(field)) {
        Summarize::Sum
    } else {
        Summarize::Count
    }
}

/// Take a field out of Filters/Rows/Columns, returning its settings.
fn take_from_axes(spec: &mut PivotSpec, field: &str) -> Option<PivotField> {
    for area in [Area::Filters, Area::Columns, Area::Rows] {
        let axis = axis_mut(spec, area);
        if let Some(i) = axis.iter().position(|f| f.field == field) {
            return Some(axis.remove(i));
        }
    }
    None
}

/// Place `field` in `to`. A field is in at most one of Filters/Rows/
/// Columns (moving keeps its settings); Values may hold it in addition,
/// even several times, as in Excel. `from` is the chip being moved.
fn place_field(
    spec: &mut PivotSpec,
    data: Option<&PivotData>,
    field: &str,
    to: Area,
    from: Option<(Area, usize)>,
    at: Option<usize>,
) {
    if let Some((Area::Values, i)) = from
        && to != Area::Values
        && i < spec.values.len()
    {
        spec.values.remove(i);
    }
    if to == Area::Values {
        if let Some((Area::Values, i)) = from {
            // Reorder within Values.
            let v = spec.values.remove(i);
            let at = at.unwrap_or(spec.values.len()).min(spec.values.len());
            spec.values.insert(at, v);
            return;
        }
        if let Some((area, _)) = from {
            // Moving an axis chip into Values moves the field.
            axis_mut(spec, area).retain(|f| f.field != field);
        }
        spec.values
            .push(PivotValue::new(field, default_summarize(data, field)));
        return;
    }
    let settings = take_from_axes(spec, field).unwrap_or_else(|| PivotField::new(field));
    let axis = axis_mut(spec, to);
    let at = at.unwrap_or(axis.len()).min(axis.len());
    axis.insert(at, settings);
}

fn remove_field_everywhere(spec: &mut PivotSpec, field: &str) {
    take_from_axes(spec, field);
    spec.values.retain(|v| v.field != field);
}

// ----------------------------------------------------------------- impl

impl GuiApp {
    /// Cached result of pivot `idx` on the active sheet; recomputed when the
    /// definition or the source data change.
    pub fn pivot_entry(&mut self, idx: usize) -> Option<Rc<PivotEntry>> {
        let host = self.app.workbook.active_sheet;
        let spec = self.app.workbook.current_sheet().pivots.get(idx)?.clone();
        let data = PivotData::read(&self.app.workbook, host, &spec.source);
        let mut h = DefaultHasher::new();
        match &data {
            Ok(d) => {
                d.fields.hash(&mut h);
                d.records.hash(&mut h);
            }
            Err(e) => e.hash(&mut h),
        }
        let data_hash = h.finish();
        let key = (host, spec.name.clone());
        if let Some(e) = self.pivots.cache.get(&key)
            && e.spec == spec
            && e.data_hash == data_hash
        {
            return Some(e.clone());
        }
        let output = data
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|d| compute_pivot(&spec, d));
        let entry = Rc::new(PivotEntry {
            spec,
            data_hash,
            data,
            output,
        });
        self.pivots.cache.insert(key, entry.clone());
        Some(entry)
    }

    fn edit_pivot(&mut self, idx: usize, f: impl FnOnce(&mut PivotSpec)) {
        let Some(mut spec) = self.app.workbook.current_sheet().pivots.get(idx).cloned() else {
            return;
        };
        f(&mut spec);
        self.app.replace_pivot(idx, spec);
    }

    pub fn open_create_pivot(&mut self) {
        let ((r0, c0), (r1, c1)) = match self.app.get_selection_range() {
            Some(range) if range.0 != range.1 => range,
            _ => current_region(
                self.app.workbook.current_sheet(),
                self.app.selected_row,
                self.app.selected_col,
            ),
        };
        let label = |r: usize, c: usize| format!("{}{}", Spreadsheet::column_label(c), r + 1);
        self.pivots.create = Some(CreateDialog {
            source: format!("{}:{}", label(r0, c0), label(r1, c1)),
            name: self.app.next_pivot_name(),
            error: None,
        });
        self.show_sidebar = true;
    }

    pub fn pivot_dialogs(&mut self, ctx: &egui::Context) {
        self.create_pivot_dialog(ctx);
        self.value_settings_dialog(ctx);
        self.field_settings_dialog(ctx);
        self.options_dialog(ctx);
    }

    fn create_pivot_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.pivots.create else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("create_pivot")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Create PivotTable");
            ui.add_space(6.0);
            ui.label("Choose the data that you want to analyze");
            egui::Grid::new("create_pivot_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Table/Range:");
                    ui.add(
                        egui::TextEdit::singleline(&mut dlg.source)
                            .hint_text("A1:D100 or Sheet2!A1:D100"),
                    );
                    ui.end_row();
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut dlg.name);
                    ui.end_row();
                });
            ui.label(
                RichText::new("The first row of the range must contain column headers.").weak(),
            );
            if let Some(err) = &dlg.error {
                ui.colored_label(Color32::from_rgb(210, 60, 60), err);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel {
            self.pivots.create = None;
        } else if ok {
            let host = self.app.workbook.active_sheet;
            let dlg = self.pivots.create.as_mut().expect("open");
            let name = dlg.name.trim().to_string();
            let taken = self
                .app
                .workbook
                .current_sheet()
                .pivots
                .iter()
                .any(|p| p.name == name);
            match PivotData::read(&self.app.workbook, host, dlg.source.trim()) {
                _ if name.is_empty() => dlg.error = Some("Enter a name".into()),
                _ if taken => {
                    dlg.error = Some(format!("A PivotTable named {} already exists", name))
                }
                Err(e) => dlg.error = Some(e),
                Ok(d) if d.records.is_empty() => {
                    dlg.error =
                        Some("The range needs a header row and at least one data row".into())
                }
                Ok(_) => {
                    let spec = PivotSpec::new(name, dlg.source.trim().to_uppercase());
                    self.pivots.create = None;
                    self.app.add_pivot(spec);
                    self.pivots.editing = Some(self.app.workbook.current_sheet().pivots.len() - 1);
                }
            }
        }
    }

    // ------------------------------------------------------ field pane

    /// Excel's "PivotTable Fields" task pane for pivot `idx`.
    pub fn field_pane(&mut self, ui: &mut Ui, idx: usize) {
        let Some(entry) = self.pivot_entry(idx) else {
            self.pivots.editing = None;
            return;
        };
        let spec = entry.spec.clone();
        let data = entry.data.as_ref().ok();
        let mut new_spec = spec.clone();
        let mut open_value_settings: Option<usize> = None;
        let mut open_field_settings: Option<(Area, usize)> = None;

        ui.horizontal(|ui| {
            ui.heading("PivotTable Fields");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    self.pivots.editing = None;
                }
            });
        });
        ui.label(RichText::new(&spec.name).weak());
        if let Err(e) = &entry.data {
            ui.colored_label(Color32::from_rgb(210, 60, 60), e);
        }

        // -- field list
        ui.horizontal(|ui| {
            ui.label("Choose fields to add to report:");
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.pivots.search)
                .hint_text("Search")
                .desired_width(f32::INFINITY),
        );
        let fields: Vec<String> = data.map(|d| d.fields.clone()).unwrap_or_default();
        let search = self.pivots.search.to_lowercase();
        let (_, dropped_on_list) = ui.dnd_drop_zone::<DragField, _>(Frame::NONE, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("pivot_fields")
                .max_height(170.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for field in fields.iter().filter(|f| f.to_lowercase().contains(&search)) {
                        let mut checked = spec.uses_field(field);
                        // egui can't nest a clickable widget inside a drag
                        // source, so the checkbox toggles and the name drags.
                        let toggled = ui
                            .horizontal(|ui| {
                                let mut toggled = ui.checkbox(&mut checked, "").changed();
                                let label = ui
                                    .add(
                                        egui::Label::new(field.as_str())
                                            .sense(Sense::click_and_drag()),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::Grab);
                                label.dnd_set_drag_payload(DragField::List(field.clone()));
                                if label.clicked() {
                                    checked = !checked;
                                    toggled = true;
                                }
                                toggled
                            })
                            .inner;
                        if toggled {
                            if checked {
                                // Excel: numbers go to Values, text to Rows.
                                let to = if data.is_some_and(|d| d.is_numeric(field)) {
                                    Area::Values
                                } else {
                                    Area::Rows
                                };
                                place_field(&mut new_spec, data, field, to, None, None);
                            } else {
                                remove_field_everywhere(&mut new_spec, field);
                            }
                        }
                    }
                });
        });
        // Dragging a chip back onto the list removes it (like dragging out).
        if let Some(payload) = dropped_on_list {
            match payload.as_ref() {
                DragField::Chip(Area::Values, i) => {
                    if *i < new_spec.values.len() {
                        new_spec.values.remove(*i);
                    }
                }
                DragField::Chip(area, i) => {
                    let axis = axis_mut(&mut new_spec, *area);
                    if *i < axis.len() {
                        axis.remove(*i);
                    }
                }
                _ => {}
            }
        }

        ui.add_space(4.0);
        ui.label("Drag fields between areas below:");
        let width = (ui.available_width() - 8.0) / 2.0;
        egui::Grid::new(("pivot_areas", idx))
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for row in [[Area::Filters, Area::Columns], [Area::Rows, Area::Values]] {
                    for area in row {
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.label(RichText::new(area.title()).strong());
                            let action = self.area_box(ui, &spec, area, width);
                            match action {
                                AreaAction::None => {}
                                AreaAction::Drop(payload, at) => {
                                    apply_drop(&mut new_spec, data, payload, area, at)
                                }
                                AreaAction::Menu(cmd) => apply_menu(
                                    &mut new_spec,
                                    data,
                                    area,
                                    cmd,
                                    &mut open_value_settings,
                                    &mut open_field_settings,
                                ),
                            }
                        });
                    }
                    ui.end_row();
                }
            });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("PivotTable Options…").clicked() {
                self.pivots.options = Some(OptionsDialog {
                    pivot: idx,
                    draft: spec.clone(),
                    error: None,
                });
            }
            if ui.button("Insert PivotChart…").clicked() {
                self.open_pivot_chart_dialog(&spec);
            }
        });

        if let Some(v) = open_value_settings
            && let Some(draft) = new_spec.values.get(v).cloned()
        {
            self.pivots.value_settings = Some(ValueSettingsDialog {
                pivot: idx,
                index: v,
                draft,
                show_as_tab: false,
            });
        }
        if let Some((area, i)) = open_field_settings
            && area != Area::Values
            && let Some(draft) = axis_mut(&mut new_spec.clone(), area).get(i).cloned()
        {
            self.pivots.field_settings = Some(FieldSettingsDialog {
                pivot: idx,
                area,
                index: i,
                draft,
            });
        }
        // Show what's being dragged next to the pointer.
        if let Some(payload) = egui::DragAndDrop::payload::<DragField>(ui.ctx())
            && let Some(pos) = ui.ctx().pointer_hover_pos()
        {
            let label = match payload.as_ref() {
                DragField::List(f) => f.clone(),
                DragField::Chip(Area::Values, i) => spec
                    .values
                    .get(*i)
                    .map(|v| v.display_name())
                    .unwrap_or_default(),
                DragField::Chip(area, i) => axis_mut(&mut spec.clone(), *area)
                    .get(*i)
                    .map(|f| f.field.clone())
                    .unwrap_or_default(),
                DragField::SigmaValues => "Σ Values".to_string(),
            };
            egui::Area::new(egui::Id::new("pivot_drag_preview"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pos + vec2(12.0, 8.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(label);
                    });
                });
        }
        if new_spec != spec {
            self.app.replace_pivot(idx, new_spec);
        }
    }

    /// One area box with its chips; returns what the user did.
    fn area_box(&mut self, ui: &mut Ui, spec: &PivotSpec, area: Area, width: f32) -> AreaAction {
        let mut action = AreaAction::None;
        let chips: Vec<String> = match area {
            Area::Filters => spec.filters.iter().map(|f| f.field.clone()).collect(),
            Area::Columns => spec.columns.iter().map(|f| f.field.clone()).collect(),
            Area::Rows => spec.rows.iter().map(|f| f.field.clone()).collect(),
            Area::Values => spec.values.iter().map(|v| v.display_name()).collect(),
        };
        let sigma_here = spec.values.len() > 1
            && ((area == Area::Rows && spec.options.values_on_rows)
                || (area == Area::Columns && !spec.options.values_on_rows));
        let frame = Frame::group(ui.style()).inner_margin(Margin::same(4));
        let (resp, dropped) = ui.dnd_drop_zone::<DragField, _>(frame, |ui| {
            ui.set_min_size(vec2(width - 10.0, 70.0));
            ui.set_max_width(width - 10.0);
            let mut hover_index = None;
            for (i, name) in chips.iter().enumerate() {
                let r = chip_menu(
                    ui,
                    name,
                    area,
                    i,
                    chips.len(),
                    DragField::Chip(area, i),
                    |cmd| action = AreaAction::Menu(cmd),
                );
                if let Some(p) = ui.ctx().pointer_hover_pos()
                    && p.y < r.rect.center().y
                    && hover_index.is_none()
                {
                    hover_index = Some(i);
                }
            }
            if sigma_here {
                let resp = chip_button(
                    ui,
                    RichText::new("Σ Values").italics(),
                    DragField::SigmaValues,
                );
                egui::Popup::menu(&resp)
                    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let other = if area == Area::Rows {
                            "Move to Column Labels"
                        } else {
                            "Move to Row Labels"
                        };
                        if ui.button(other).clicked() {
                            action = AreaAction::Menu(ChipCmd::SigmaToggle);
                            ui.close();
                        }
                    });
            }
            hover_index
        });
        if let Some(payload) = dropped {
            let at = resp.inner;
            action = AreaAction::Drop((*payload).clone(), at);
        }
        action
    }

    // ---------------------------------------------------- table render

    /// The pivot card body: report filters and the table.
    pub fn draw_pivot(&mut self, ui: &mut Ui, idx: usize) {
        let Some(entry) = self.pivot_entry(idx) else {
            return;
        };
        let spec = entry.spec.clone();
        let data = match &entry.data {
            Ok(d) => d,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(210, 60, 60), e);
                return;
            }
        };
        let mut new_spec = spec.clone();

        // Report filters, above the table like Excel.
        for (i, f) in spec.filters.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&f.field).strong());
                let items = data.items(&f.field);
                let shown =
                    items.len() - f.hidden_items.iter().filter(|h| items.contains(h)).count();
                let caption = match shown {
                    n if n == items.len() => "(All)".to_string(),
                    1 => items
                        .iter()
                        .find(|it| !f.hidden_items.contains(it))
                        .cloned()
                        .unwrap_or_default(),
                    0 => "(None)".to_string(),
                    _ => "(Multiple Items)".to_string(),
                };
                ui.menu_button(caption, |ui| {
                    item_checklist(ui, &items, &mut new_spec.filters[i].hidden_items);
                });
            });
        }

        match &entry.output {
            Err(e) => {
                ui.colored_label(Color32::from_rgb(210, 60, 60), e);
            }
            Ok(_) if spec.rows.is_empty() && spec.columns.is_empty() && spec.values.is_empty() => {
                let r = ui.add(
                    egui::Label::new(
                        RichText::new(format!("{}\n\nTo build a report, choose fields from the PivotTable Field List.", spec.name))
                            .weak(),
                    )
                    .sense(Sense::click()),
                );
                if r.clicked() {
                    self.pivots.editing = Some(idx);
                }
            }
            Ok(out) => {
                egui::ScrollArea::horizontal()
                    .id_salt(("pivot_table", idx))
                    .show(ui, |ui| {
                        paint_table(ui, out, &spec, data, &mut new_spec, idx);
                    });
            }
        }
        if new_spec != spec {
            self.app.replace_pivot(idx, new_spec);
        }
    }

    // ---------------------------------------------------- dialogs

    fn value_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.pivots.value_settings else {
            return;
        };
        let spec = self
            .app
            .workbook
            .current_sheet()
            .pivots
            .get(dlg.pivot)
            .cloned();
        let Some(spec) = spec else {
            self.pivots.value_settings = None;
            return;
        };
        let base_fields: Vec<String> = spec
            .rows
            .iter()
            .chain(&spec.columns)
            .map(|f| f.field.clone())
            .collect();
        let host = self.app.workbook.active_sheet;
        let data = PivotData::read(&self.app.workbook, host, &spec.source).ok();
        let mut ok = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("value_field_settings")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Value Field Settings");
            // Body scrolls so OK/Cancel stay on screen in small windows.
            let body_h = (ui.ctx().content_rect().height() - 180.0).max(200.0);
            egui::ScrollArea::vertical().id_salt("dialog_body").max_height(body_h).show(ui, |ui| {
            ui.label(format!("Source Name: {}", dlg.draft.field));
            ui.horizontal(|ui| {
                ui.label("Custom Name:");
                let mut name = dlg.draft.display_name();
                if ui.text_edit_singleline(&mut name).changed() {
                    let default = PivotValue { name: None, ..dlg.draft.clone() }.display_name();
                    dlg.draft.name = (name != default && !name.is_empty()).then_some(name);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut dlg.show_as_tab, false, "Summarize Values By");
                ui.selectable_value(&mut dlg.show_as_tab, true, "Show Values As");
            });
            ui.separator();
            if !dlg.show_as_tab {
                ui.label("Summarize value field by");
                ui.label(RichText::new("Choose the type of calculation that you want to use to summarize data from the selected field").weak());
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for how in Summarize::ALL {
                        let before = dlg.draft.summarize;
                        if ui.selectable_value(&mut dlg.draft.summarize, how, how.label()).clicked()
                            && dlg.draft.name.as_deref() == Some(&PivotValue { name: None, summarize: before, ..dlg.draft.clone() }.display_name())
                        {
                            dlg.draft.name = None;
                        }
                    }
                });
            } else {
                ui.label("Show values as");
                ComboBox::from_id_salt("show_as")
                    .width(260.0)
                    .selected_text(dlg.draft.show_as.label())
                    .show_ui(ui, |ui| {
                        for s in ShowValuesAs::ALL {
                            ui.selectable_value(&mut dlg.draft.show_as, s, s.label());
                        }
                    });
                let needs_field = dlg.draft.show_as.needs_base_field();
                let needs_item = dlg.draft.show_as.needs_base_item();
                ui.add_enabled_ui(needs_field, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label("Base field:");
                            egui::ScrollArea::vertical().id_salt("base_field").max_height(120.0).show(ui, |ui| {
                                for f in &base_fields {
                                    ui.selectable_value(&mut dlg.draft.base_field, Some(f.clone()), f);
                                }
                            });
                        });
                        ui.add_enabled_ui(needs_item, |ui| {
                            ui.vertical(|ui| {
                                ui.label("Base item:");
                                egui::ScrollArea::vertical().id_salt("base_item").max_height(120.0).show(ui, |ui| {
                                    let mut items = vec!["(previous)".to_string(), "(next)".to_string()];
                                    if let (Some(d), Some(bf)) = (&data, &dlg.draft.base_field) {
                                        items.extend(d.items(bf));
                                    }
                                    for it in items {
                                        ui.selectable_value(&mut dlg.draft.base_item, Some(it.clone()), it);
                                    }
                                });
                            });
                        });
                    });
                });
                if needs_field && dlg.draft.base_field.is_none() {
                    ui.colored_label(Color32::from_rgb(220, 140, 30), "Choose a base field (it must be in Rows or Columns).");
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Number Format:");
                number_format_picker(ui, &mut dlg.draft.number_format);
            });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            let dlg = self.pivots.value_settings.take().expect("open");
            self.edit_pivot(dlg.pivot, |s| {
                if dlg.index < s.values.len() {
                    s.values[dlg.index] = dlg.draft;
                }
            });
        } else if cancel {
            self.pivots.value_settings = None;
        }
    }

    fn field_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.pivots.field_settings else {
            return;
        };
        let Some(spec) = self
            .app
            .workbook
            .current_sheet()
            .pivots
            .get(dlg.pivot)
            .cloned()
        else {
            self.pivots.field_settings = None;
            return;
        };
        let host = self.app.workbook.active_sheet;
        let items = PivotData::read(&self.app.workbook, host, &spec.source)
            .map(|d| d.items(&dlg.draft.field))
            .unwrap_or_default();
        let values: Vec<String> = spec.values.iter().map(|v| v.display_name()).collect();
        let mut ok = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("field_settings")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Field Settings");
            // Body scrolls so OK/Cancel stay on screen in small windows.
            let body_h = (ui.ctx().content_rect().height() - 180.0).max(200.0);
            egui::ScrollArea::vertical()
                .id_salt("dialog_body")
                .max_height(body_h)
                .show(ui, |ui| {
                    ui.label(format!("Source Name: {}", dlg.draft.field));
                    ui.separator();
                    if dlg.area != Area::Filters {
                        ui.label(RichText::new("Subtotals").strong());
                        ui.radio_value(&mut dlg.draft.subtotals, true, "Automatic");
                        ui.radio_value(&mut dlg.draft.subtotals, false, "None");
                        ui.separator();
                        ui.label(RichText::new("Sort").strong());
                        sort_options(ui, &mut dlg.draft.sort, &values);
                        ui.separator();
                        ui.label(RichText::new("Value Filters: Top 10").strong());
                        top_filter_editor(ui, &mut dlg.draft.top_filter, &values);
                        ui.separator();
                    }
                    ui.label(RichText::new("Items").strong());
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            item_checklist(ui, &items, &mut dlg.draft.hidden_items);
                        });
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            let dlg = self.pivots.field_settings.take().expect("open");
            self.edit_pivot(dlg.pivot, |s| {
                let axis = axis_mut(s, dlg.area);
                if dlg.index < axis.len() {
                    axis[dlg.index] = dlg.draft;
                }
            });
        } else if cancel {
            self.pivots.field_settings = None;
        }
    }

    fn options_dialog(&mut self, ctx: &egui::Context) {
        let Some(dlg) = &mut self.pivots.options else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("pivot_options")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("PivotTable Options");
            // Body scrolls so OK/Cancel stay on screen in small windows.
            let body_h = (ui.ctx().content_rect().height() - 180.0).max(200.0);
            egui::ScrollArea::vertical()
                .id_salt("dialog_body")
                .max_height(body_h)
                .show(ui, |ui| {
                    egui::Grid::new("pivot_opts_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("PivotTable Name:");
                            ui.text_edit_singleline(&mut dlg.draft.name);
                            ui.end_row();
                            ui.label("Data source:");
                            ui.text_edit_singleline(&mut dlg.draft.source);
                            ui.end_row();
                        });
                    ui.separator();
                    let o = &mut dlg.draft.options;
                    ui.label(RichText::new("Report Layout").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut o.layout, PivotLayout::Compact, "Compact Form");
                        ui.radio_value(&mut o.layout, PivotLayout::Outline, "Outline Form");
                        ui.radio_value(&mut o.layout, PivotLayout::Tabular, "Tabular Form");
                    });
                    ui.add_enabled(
                        o.layout != PivotLayout::Compact,
                        egui::Checkbox::new(&mut o.repeat_item_labels, "Repeat All Item Labels"),
                    );
                    ui.separator();
                    ui.label(RichText::new("Subtotals").strong());
                    let all_off = dlg
                        .draft
                        .rows
                        .iter()
                        .chain(&dlg.draft.columns)
                        .all(|f| !f.subtotals);
                    let mut mode = if all_off {
                        0
                    } else if dlg.draft.options.subtotals_at_top {
                        2
                    } else {
                        1
                    };
                    let before = mode;
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut mode, 0, "Do Not Show Subtotals");
                        ui.radio_value(&mut mode, 1, "Show at Bottom");
                        ui.add_enabled(
                            dlg.draft.options.layout != PivotLayout::Tabular,
                            egui::RadioButton::new(mode == 2, "Show at Top"),
                        )
                        .clicked()
                        .then(|| mode = 2);
                    });
                    if mode != before {
                        for f in dlg
                            .draft
                            .rows
                            .iter_mut()
                            .chain(dlg.draft.columns.iter_mut())
                        {
                            f.subtotals = mode != 0;
                        }
                        dlg.draft.options.subtotals_at_top = mode == 2;
                    }
                    let o = &mut dlg.draft.options;
                    ui.separator();
                    ui.label(RichText::new("Totals & Filters").strong());
                    ui.checkbox(&mut o.grand_totals_rows, "Show grand totals for rows");
                    ui.checkbox(&mut o.grand_totals_columns, "Show grand totals for columns");
                    ui.separator();
                    ui.label(RichText::new("Layout & Format").strong());
                    ui.horizontal(|ui| {
                        ui.label("For empty cells show:");
                        ui.add(egui::TextEdit::singleline(&mut o.empty_cells).desired_width(80.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Show Σ Values in:");
                        ui.radio_value(&mut o.values_on_rows, false, "Columns");
                        ui.radio_value(&mut o.values_on_rows, true, "Rows");
                    });
                    if let Some(err) = &dlg.error {
                        ui.colored_label(Color32::from_rgb(210, 60, 60), err);
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ok = ui.button("OK").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            let dlg = self.pivots.options.as_mut().expect("open");
            let name = dlg.draft.name.trim().to_string();
            let clash = self
                .app
                .workbook
                .current_sheet()
                .pivots
                .iter()
                .enumerate()
                .any(|(i, p)| i != dlg.pivot && p.name == name);
            let host = self.app.workbook.active_sheet;
            if name.is_empty() || clash {
                dlg.error = Some("The name must be unique and not empty".into());
            } else if let Err(e) =
                PivotData::read(&self.app.workbook, host, dlg.draft.source.trim())
            {
                dlg.error = Some(e);
            } else {
                let dlg = self.pivots.options.take().expect("open");
                let mut spec = dlg.draft;
                spec.name = name;
                spec.source = spec.source.trim().to_uppercase();
                self.app.replace_pivot(dlg.pivot, spec);
            }
        } else if cancel {
            self.pivots.options = None;
        }
    }
}

// --------------------------------------------------------- chip menus

#[derive(Clone, Copy, Debug)]
enum ChipCmd {
    Up(usize),
    Down(usize),
    First(usize),
    Last(usize),
    MoveTo(usize, Area),
    Remove(usize),
    Settings(usize),
    SigmaToggle,
}

enum AreaAction {
    None,
    Drop(DragField, Option<usize>),
    Menu(ChipCmd),
}

/// The dropdown Excel shows on a field in an area.
/// A full-width field button with a dropdown arrow, like Excel's area
/// chips. Clicking opens its menu; dragging carries `payload`.
fn chip_button(ui: &mut Ui, text: RichText, payload: DragField) -> egui::Response {
    let resp = ui.add(
        egui::Button::new(text)
            .min_size(vec2(ui.available_width(), 22.0))
            .sense(Sense::click_and_drag()),
    );
    resp.dnd_set_drag_payload(payload);
    let color = ui.visuals().text_color();
    let a = pos2(resp.rect.max.x - 14.0, resp.rect.center().y - 2.0);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![a, a + vec2(8.0, 0.0), a + vec2(4.0, 4.5)],
        color,
        Stroke::NONE,
    ));
    resp
}

fn chip_menu(
    ui: &mut Ui,
    name: &str,
    area: Area,
    i: usize,
    len: usize,
    payload: DragField,
    mut act: impl FnMut(ChipCmd),
) -> egui::Response {
    let resp = chip_button(ui, RichText::new(name), payload);
    egui::Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut item = |ui: &mut Ui, enabled: bool, label: &str, cmd: ChipCmd| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    act(cmd);
                    ui.close();
                }
            };
            item(ui, i > 0, "Move Up", ChipCmd::Up(i));
            item(ui, i + 1 < len, "Move Down", ChipCmd::Down(i));
            item(ui, i > 0, "Move to Beginning", ChipCmd::First(i));
            item(ui, i + 1 < len, "Move to End", ChipCmd::Last(i));
            ui.separator();
            item(
                ui,
                area != Area::Filters,
                "Move to Report Filter",
                ChipCmd::MoveTo(i, Area::Filters),
            );
            item(
                ui,
                area != Area::Rows,
                "Move to Row Labels",
                ChipCmd::MoveTo(i, Area::Rows),
            );
            item(
                ui,
                area != Area::Columns,
                "Move to Column Labels",
                ChipCmd::MoveTo(i, Area::Columns),
            );
            item(
                ui,
                area != Area::Values,
                "Move to Values",
                ChipCmd::MoveTo(i, Area::Values),
            );
            ui.separator();
            item(ui, true, "Remove Field", ChipCmd::Remove(i));
            ui.separator();
            let settings = if area == Area::Values {
                "Value Field Settings…"
            } else {
                "Field Settings…"
            };
            item(ui, true, settings, ChipCmd::Settings(i));
        });
    resp
}

fn apply_menu(
    spec: &mut PivotSpec,
    data: Option<&PivotData>,
    area: Area,
    cmd: ChipCmd,
    value_settings: &mut Option<usize>,
    field_settings: &mut Option<(Area, usize)>,
) {
    let len = area_len(spec, area);
    let reorder = |spec: &mut PivotSpec, from: usize, to: usize| {
        if area == Area::Values {
            let v = spec.values.remove(from);
            spec.values.insert(to.min(spec.values.len()), v);
        } else {
            let axis = axis_mut(spec, area);
            let f = axis.remove(from);
            axis.insert(to.min(axis.len()), f);
        }
    };
    match cmd {
        ChipCmd::Up(i) if i > 0 => reorder(spec, i, i - 1),
        ChipCmd::Down(i) if i + 1 < len => reorder(spec, i, i + 1),
        ChipCmd::First(i) => reorder(spec, i, 0),
        ChipCmd::Last(i) => reorder(spec, i, len - 1),
        ChipCmd::MoveTo(i, to) => {
            let field = if area == Area::Values {
                spec.values[i].field.clone()
            } else {
                axis_mut(spec, area)[i].field.clone()
            };
            place_field(spec, data, &field, to, Some((area, i)), None);
        }
        ChipCmd::Remove(i) => {
            if area == Area::Values {
                spec.values.remove(i);
            } else {
                axis_mut(spec, area).remove(i);
            }
        }
        ChipCmd::Settings(i) => {
            if area == Area::Values {
                *value_settings = Some(i);
            } else {
                *field_settings = Some((area, i));
            }
        }
        ChipCmd::SigmaToggle => spec.options.values_on_rows = !spec.options.values_on_rows,
        _ => {}
    }
}

fn apply_drop(
    spec: &mut PivotSpec,
    data: Option<&PivotData>,
    payload: DragField,
    to: Area,
    at: Option<usize>,
) {
    match payload {
        DragField::List(field) => place_field(spec, data, &field, to, None, at),
        DragField::Chip(from, i) => {
            if from == to {
                // Reorder within the area.
                let len = area_len(spec, from);
                if i < len {
                    let target = at.unwrap_or(len).min(len);
                    let target = if target > i { target - 1 } else { target };
                    if from == Area::Values {
                        let v = spec.values.remove(i);
                        spec.values.insert(target.min(spec.values.len()), v);
                    } else {
                        let axis = axis_mut(spec, from);
                        let f = axis.remove(i);
                        axis.insert(target.min(axis.len()), f);
                    }
                }
            } else if i < area_len(spec, from) {
                let field = if from == Area::Values {
                    spec.values[i].field.clone()
                } else {
                    axis_mut(spec, from)[i].field.clone()
                };
                place_field(spec, data, &field, to, Some((from, i)), at);
            }
        }
        DragField::SigmaValues => match to {
            Area::Rows => spec.options.values_on_rows = true,
            Area::Columns => spec.options.values_on_rows = false,
            _ => {}
        },
    }
}

// ------------------------------------------------------ shared widgets

/// "(Select All)" plus one checkbox per item; edits the hidden-item list.
fn item_checklist(ui: &mut Ui, items: &[String], hidden: &mut Vec<String>) {
    let mut all = items.iter().all(|i| !hidden.contains(i));
    if ui.checkbox(&mut all, "(Select All)").changed() {
        if all {
            hidden.retain(|h| !items.contains(h));
        } else {
            for it in items {
                if !hidden.contains(it) {
                    hidden.push(it.clone());
                }
            }
        }
    }
    egui::ScrollArea::vertical()
        .id_salt("item_checklist")
        .max_height(220.0)
        .show(ui, |ui| {
            for it in items {
                let mut on = !hidden.contains(it);
                if ui.checkbox(&mut on, it).changed() {
                    if on {
                        hidden.retain(|h| h != it);
                    } else {
                        hidden.push(it.clone());
                    }
                }
            }
        });
}

fn sort_options(ui: &mut Ui, sort: &mut PivotSort, values: &[String]) {
    let mut kind = match sort {
        PivotSort::Ascending => 0,
        PivotSort::Descending => 1,
        PivotSort::Manual => 2,
        PivotSort::ByValue {
            descending: false, ..
        } => 3,
        PivotSort::ByValue {
            descending: true, ..
        } => 4,
    };
    let mut by = match sort {
        PivotSort::ByValue { value, .. } => *value,
        _ => 0,
    };
    ui.radio_value(&mut kind, 2, "Manual (data source order)");
    ui.radio_value(&mut kind, 0, "Ascending (A to Z) by field labels");
    ui.radio_value(&mut kind, 1, "Descending (Z to A) by field labels");
    ui.add_enabled_ui(!values.is_empty(), |ui| {
        ui.radio_value(&mut kind, 3, "Ascending by value");
        ui.radio_value(&mut kind, 4, "Descending by value");
        if kind >= 3 {
            ComboBox::from_id_salt("sort_by_value")
                .selected_text(values.get(by).cloned().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for (i, v) in values.iter().enumerate() {
                        ui.selectable_value(&mut by, i, v);
                    }
                });
        }
    });
    *sort = match kind {
        0 => PivotSort::Ascending,
        1 => PivotSort::Descending,
        2 => PivotSort::Manual,
        k => PivotSort::ByValue {
            value: by,
            descending: k == 4,
        },
    };
}

fn top_filter_editor(ui: &mut Ui, filter: &mut Option<TopFilter>, values: &[String]) {
    let mut on = filter.is_some();
    ui.add_enabled_ui(!values.is_empty(), |ui| {
        if ui.checkbox(&mut on, "Show only").changed() {
            *filter = on.then_some(TopFilter {
                top: true,
                count: 10,
                by_value: 0,
            });
        }
        if let Some(f) = filter {
            ui.horizontal(|ui| {
                ComboBox::from_id_salt("top_bottom")
                    .width(80.0)
                    .selected_text(if f.top { "Top" } else { "Bottom" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.top, true, "Top");
                        ui.selectable_value(&mut f.top, false, "Bottom");
                    });
                ui.add(egui::DragValue::new(&mut f.count).range(1..=10_000));
                ui.label("Items by");
                ComboBox::from_id_salt("top_by")
                    .selected_text(values.get(f.by_value).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, v) in values.iter().enumerate() {
                            ui.selectable_value(&mut f.by_value, i, v);
                        }
                    });
            });
        }
    });
}

fn number_format_picker(ui: &mut Ui, fmt: &mut Option<NumberFormat>) {
    let label = match fmt {
        None | Some(NumberFormat::General) => "General".to_string(),
        Some(NumberFormat::Number {
            decimals,
            thousands_sep,
        }) => {
            format!(
                "Number ({} dp{})",
                decimals,
                if *thousands_sep {
                    ", 1,000 separator"
                } else {
                    ""
                }
            )
        }
        Some(NumberFormat::Currency { symbol, decimals }) => {
            format!("Currency {} ({} dp)", symbol, decimals)
        }
        Some(NumberFormat::Percentage { decimals }) => format!("Percentage ({} dp)", decimals),
    };
    ComboBox::from_id_salt("value_number_format")
        .width(220.0)
        .selected_text(label)
        .show_ui(ui, |ui| {
            ui.selectable_value(fmt, None, "General");
            for (label, f) in crate::app::number_formats().into_iter().skip(1) {
                ui.selectable_value(fmt, Some(f.clone()), label);
            }
        });
}

/// The contiguous non-empty block around (row, col), like Excel's
/// "current region" used to prefill the Create PivotTable range.
pub fn current_region(
    sheet: &Spreadsheet,
    row: usize,
    col: usize,
) -> ((usize, usize), (usize, usize)) {
    let filled = |r: usize, c: usize| {
        sheet
            .cells
            .get(&(r, c))
            .is_some_and(|cd| !cd.value.is_empty())
    };
    let (mut r0, mut c0, mut r1, mut c1) = (row, col, row, col);
    for _ in 0..10_000 {
        let mut grew = false;
        if r0 > 0 && (c0..=c1).any(|c| filled(r0 - 1, c)) {
            r0 -= 1;
            grew = true;
        }
        if r1 + 1 < sheet.rows && (c0..=c1).any(|c| filled(r1 + 1, c)) {
            r1 += 1;
            grew = true;
        }
        if c0 > 0 && (r0..=r1).any(|r| filled(r, c0 - 1)) {
            c0 -= 1;
            grew = true;
        }
        if c1 + 1 < sheet.cols && (r0..=r1).any(|r| filled(r, c1 + 1)) {
            c1 += 1;
            grew = true;
        }
        if !grew {
            break;
        }
    }
    ((r0, c0), (r1, c1))
}

// ------------------------------------------------------------ painting

const PT_ROW_H: f32 = 21.0;
const PT_FONT: f32 = 12.5;

/// Paint the pivot output Excel-style and add the Row/Column Labels
/// dropdowns (sort + filter of the first field on that axis).
fn paint_table(
    ui: &mut Ui,
    out: &PivotOutput,
    spec: &PivotSpec,
    data: &PivotData,
    new_spec: &mut PivotSpec,
    idx: usize,
) {
    let font = FontId::proportional(PT_FONT);
    let dark = ui.visuals().dark_mode;
    let text = ui.visuals().text_color();
    let header_bg = if dark {
        Color32::from_rgb(38, 58, 84)
    } else {
        Color32::from_rgb(221, 235, 247)
    };
    let line = if dark {
        Color32::from_gray(70)
    } else {
        Color32::from_rgb(155, 194, 230)
    };
    let ncols = out.table.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![40.0f32; ncols];
    ui.fonts_mut(|f| {
        for row in &out.table {
            for (c, cell) in row.iter().enumerate() {
                let w = f
                    .layout_no_wrap(cell.text.clone(), font.clone(), text)
                    .size()
                    .x
                    + 16.0
                    + cell.indent as f32 * 12.0;
                // Header cells with a dropdown need room for the arrow.
                widths[c] = widths[c].max(
                    w + if cell.kind == PivotCellKind::Header {
                        14.0
                    } else {
                        0.0
                    },
                );
            }
        }
    });
    let total_w: f32 = widths.iter().sum();
    let total_h = out.table.len() as f32 * PT_ROW_H;
    let (rect, _) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    let xs: Vec<f32> = widths
        .iter()
        .scan(rect.min.x, |x, w| {
            let x0 = *x;
            *x += w;
            Some(x0)
        })
        .collect();

    let mut dropdowns: Vec<(Rect, bool)> = Vec::new();
    for (r, row) in out.table.iter().enumerate() {
        let y = rect.min.y + r as f32 * PT_ROW_H;
        let is_header = r < out.header_rows;
        let bold_row = row
            .iter()
            .any(|c| matches!(c.kind, PivotCellKind::Subtotal | PivotCellKind::Grand));
        let grand_row = row.first().is_some_and(|c| c.kind == PivotCellKind::Grand);
        if is_header {
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.min.x, y), vec2(total_w, PT_ROW_H)),
                0.0,
                header_bg,
            );
        }
        if grand_row {
            painter.hline(rect.x_range(), y, Stroke::new(1.5, line));
        }
        for (c, cell) in row.iter().enumerate() {
            if cell.text.is_empty() {
                continue;
            }
            let cell_rect = Rect::from_min_size(pos2(xs[c], y), vec2(widths[c], PT_ROW_H));
            let bold = is_header || bold_row || cell.kind == PivotCellKind::Grand;
            let galley = painter.layout_no_wrap(cell.text.clone(), font.clone(), text);
            let x = if cell.number.is_some() {
                cell_rect.max.x - 6.0 - galley.size().x
            } else {
                cell_rect.min.x + 6.0 + cell.indent as f32 * 12.0
            };
            let pos = pos2(x, cell_rect.center().y - galley.size().y / 2.0);
            if bold {
                painter.galley(pos + vec2(0.6, 0.0), galley.clone(), text);
            }
            painter.galley(pos, galley, text);
            // Row Labels / Column Labels get Excel's sort & filter dropdown.
            let is_rows_caption =
                r + 1 == out.header_rows && c < out.label_cols && !spec.rows.is_empty();
            let is_cols_caption = r == 0 && c == out.label_cols && !spec.columns.is_empty();
            if cell.kind == PivotCellKind::Header && (is_rows_caption || is_cols_caption) {
                dropdowns.push((cell_rect, is_rows_caption));
                let a = pos2(cell_rect.max.x - 11.0, cell_rect.center().y - 2.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![a, a + vec2(7.0, 0.0), a + vec2(3.5, 4.0)],
                    text,
                    Stroke::NONE,
                ));
            }
        }
    }
    painter.hline(
        rect.x_range(),
        rect.min.y + out.header_rows as f32 * PT_ROW_H,
        Stroke::new(1.0, line),
    );

    // Dropdown menus for the axis captions.
    let values: Vec<String> = spec.values.iter().map(|v| v.display_name()).collect();
    for (i, (r, is_rows)) in dropdowns.into_iter().enumerate() {
        let resp = ui.interact(r, egui::Id::new(("pivot_caption", idx, i)), Sense::click());
        egui::Popup::menu(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(220.0);
                let axis = if is_rows {
                    &mut new_spec.rows
                } else {
                    &mut new_spec.columns
                };
                let field_id = egui::Id::new(("pivot_caption_field", idx, is_rows));
                let mut which: usize = ui
                    .data(|d| d.get_temp(field_id))
                    .unwrap_or(0)
                    .min(axis.len().saturating_sub(1));
                if axis.len() > 1 {
                    ui.horizontal(|ui| {
                        ui.label("Select field:");
                        ComboBox::from_id_salt(("caption_field", idx, is_rows))
                            .selected_text(axis[which].field.clone())
                            .show_ui(ui, |ui| {
                                for (k, f) in axis.iter().enumerate() {
                                    ui.selectable_value(&mut which, k, &f.field);
                                }
                            });
                    });
                    ui.data_mut(|d| d.insert_temp(field_id, which));
                }
                let Some(field) = axis.get_mut(which) else {
                    return;
                };
                if ui.button("Sort A to Z").clicked() {
                    field.sort = PivotSort::Ascending;
                }
                if ui.button("Sort Z to A").clicked() {
                    field.sort = PivotSort::Descending;
                }
                ui.menu_button("More Sort Options", |ui| {
                    sort_options(ui, &mut field.sort, &values)
                });
                ui.menu_button("Value Filters", |ui| {
                    top_filter_editor(ui, &mut field.top_filter, &values)
                });
                ui.separator();
                let items = data.items(&field.field);
                item_checklist(ui, &items, &mut field.hidden_items);
            });
    }
}
