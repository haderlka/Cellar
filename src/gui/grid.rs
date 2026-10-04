//! The spreadsheet grid: a custom-painted, virtualized table inside a
//! `ScrollArea`. Only visible cells are drawn, so large sheets stay fast.
//! Column/row headers are painted at the viewport edge so they stay put
//! while the body scrolls.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui,
};
use cellar::domain::{format_cell_value, CellData, CellStyle, Spreadsheet, TerminalColor};

use cellar::application::{fill_target, CellRange, FillMode, FillTarget};

use crate::app::{move_caret_to_end, EditMove, GuiApp};

pub const ROW_H: f32 = 22.0;
pub const HEADER_H: f32 = 22.0;
pub const ROW_HEADER_W: f32 = 52.0;
/// Pixels per "character" of Cellar column width.
pub const CHAR_W: f32 = 7.5;
const FONT_SIZE: f32 = 13.0;
pub const CELL_EDITOR_ID: &str = "cell_editor";

pub fn col_px(chars: usize) -> f32 {
    (chars as f32 * CHAR_W + 10.0).max(24.0)
}

#[derive(Default)]
pub struct GridState {
    /// Scroll so the cursor cell (or `scroll_target`) is visible.
    pub scroll_to_cursor: bool,
    /// Cell to keep visible instead of the cursor (end of a Shift-extended
    /// selection).
    pub scroll_target: Option<(usize, usize)>,
    drag: Option<Drag>,
    /// The last fill-handle fill, for the Auto Fill Options button. Valid
    /// while nothing else has been undone/done and the filled range is
    /// still selected.
    last_fill: Option<LastFill>,
    /// Widgets drawn over the grid last frame (cell editor, Auto Fill
    /// button). Presses on them must not reach the grid underneath.
    overlays: Vec<Rect>,
}

#[derive(Clone, Copy)]
struct LastFill {
    source: CellRange,
    target: CellRange,
    mode: FillMode,
    undo_len: usize,
}

/// Side of the square fill handle at the selection's bottom-right corner.
const HANDLE: f32 = 7.0;

#[derive(Clone, Copy)]
enum Drag {
    Cells { anchor: (usize, usize) },
    Rows { anchor: usize },
    Cols { anchor: usize },
    Resize { col: usize, start_px: f32, start_x: f32 },
    /// Clicking cells while typing a formula inserts references.
    RefPick { anchor: (usize, usize), current: (usize, usize) },
    /// Dragging the fill handle of `source` towards `end`.
    Fill { source: CellRange, end: (usize, usize) },
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Corner,
    ColHeader(usize),
    ColBorder(usize),
    RowHeader(usize),
    Cell(usize, usize),
    FillHandle,
}

/// Pixel geometry of the active sheet.
struct Layout {
    /// Left edge of each column relative to the body; `col_x[cols]` is the
    /// total width. Hidden columns have zero width.
    col_x: Vec<f32>,
    /// Visible rows in display order (hidden rows removed), or `None` when
    /// nothing is hidden and display index == row.
    visible_rows: Option<Vec<usize>>,
    rows: usize,
}

impl Layout {
    fn new(sheet: &Spreadsheet, gui: &GuiApp) -> Self {
        let mut col_x = Vec::with_capacity(sheet.cols + 1);
        let mut x = 0.0;
        for c in 0..sheet.cols {
            col_x.push(x);
            if !gui.app.hidden_cols.contains(&c) {
                x += col_px(sheet.get_column_width(c));
            }
        }
        col_x.push(x);
        let visible_rows = (!gui.app.hidden_rows.is_empty())
            .then(|| (0..sheet.rows).filter(|r| !gui.app.hidden_rows.contains(r)).collect());
        Self { col_x, visible_rows, rows: sheet.rows }
    }

    fn row_count(&self) -> usize {
        self.visible_rows.as_ref().map_or(self.rows, Vec::len)
    }

    fn row_at(&self, idx: usize) -> usize {
        self.visible_rows.as_ref().map_or(idx, |v| v[idx])
    }

    fn index_of(&self, row: usize) -> Option<usize> {
        match &self.visible_rows {
            Some(v) => v.binary_search(&row).ok(),
            None => (row < self.rows).then_some(row),
        }
    }

    fn cols(&self) -> usize {
        self.col_x.len() - 1
    }

    /// Column containing body-relative x (clamped).
    fn col_at(&self, x: f32) -> usize {
        let i = self.col_x.partition_point(|&cx| cx <= x);
        i.saturating_sub(1).min(self.cols().saturating_sub(1))
    }

    fn width(&self) -> f32 {
        *self.col_x.last().unwrap_or(&0.0)
    }
}

impl GuiApp {
    pub fn show_grid(&mut self, ui: &mut Ui) {
        let sheet_idx = self.app.workbook.active_sheet;
        let layout = Layout::new(self.app.workbook.current_sheet(), self);
        let content = vec2(
            ROW_HEADER_W + layout.width(),
            HEADER_H + layout.row_count() as f32 * ROW_H,
        );

        egui::ScrollArea::both()
            .id_salt(("grid", sheet_idx))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_size(content);
                let origin = ui.min_rect().min;
                let screen = viewport.translate(origin.to_vec2());
                let cell_rect = |row_idx: usize, col: usize| {
                    Rect::from_min_size(
                        pos2(
                            origin.x + ROW_HEADER_W + layout.col_x[col],
                            origin.y + HEADER_H + row_idx as f32 * ROW_H,
                        ),
                        vec2(layout.col_x[col + 1] - layout.col_x[col], ROW_H),
                    )
                };

                // Keep the cursor visible after keyboard moves. Expand the
                // target by the header size so it isn't hidden under them.
                if self.grid.scroll_to_cursor {
                    self.grid.scroll_to_cursor = false;
                    let (r, c) = self
                        .grid
                        .scroll_target
                        .unwrap_or((self.app.selected_row, self.app.selected_col));
                    if let Some(idx) = layout.index_of(r)
                        && c < layout.cols()
                    {
                        let rect = cell_rect(idx, c);
                        let padded = Rect::from_min_max(
                            rect.min - vec2(ROW_HEADER_W, HEADER_H),
                            rect.max + vec2(4.0, 4.0),
                        );
                        ui.scroll_to_rect(padded, None);
                    }
                }

                let response = ui.interact(screen, ui.id().with("grid_body"), Sense::click_and_drag());
                let overlays = std::mem::take(&mut self.grid.overlays);
                self.paint(ui, &layout, origin, screen, viewport, &cell_rect);
                self.interact(ui, &layout, origin, screen, &response, &overlays);
                self.cell_editor(ui, &layout, &cell_rect, screen);
                self.fill_drag_preview(ui, &layout, &cell_rect);
                self.auto_fill_options(ui, &layout, &cell_rect);
                response.context_menu(|ui| self.grid_context_menu(ui));
            });
    }

    /// Screen rect of the fill handle (bottom-right of the selection), when
    /// it is visible and usable.
    fn handle_rect(&self, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect) -> Option<Rect> {
        if self.edit.is_some() {
            return None;
        }
        let ((_, _), (r1, c1)) = self.selection_or_cursor();
        let idx = layout.index_of(r1)?;
        if c1 >= layout.cols() {
            return None;
        }
        Some(Rect::from_center_size(cell_rect(idx, c1).max, vec2(HANDLE, HANDLE)))
    }

    fn range_rect(layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect, ((ra, ca), (rb, cb)): CellRange) -> Option<Rect> {
        let (ia, ib) = (layout.index_of(ra.min(rb))?, layout.index_of(ra.max(rb))?);
        Some(cell_rect(ia, ca.min(cb)).union(cell_rect(ib, ca.max(cb))))
    }

    /// While dragging the fill handle: outline the range being filled (or
    /// grey out the cells being cleared) and show the value the last cell
    /// will get, like Excel's fill tooltip.
    fn fill_drag_preview(&mut self, ui: &mut Ui, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect) {
        let Some(Drag::Fill { source, end }) = self.grid.drag else { return };
        let Some(target) = fill_target(source, end) else { return };
        let painter = ui.painter();
        let grey = Color32::from_gray(110);
        match target {
            FillTarget::Fill(t) => {
                let all = ((source.0.0.min(t.0.0), source.0.1.min(t.0.1)), (source.1.0.max(t.1.0), source.1.1.max(t.1.1)));
                if let Some(r) = Self::range_rect(layout, cell_rect, all) {
                    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                    for w in pts.windows(2) {
                        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], Stroke::new(1.5, grey), 4.0, 3.0));
                    }
                }
                // Preview of the value furthest from the source.
                let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.alt);
                let mode = self.drag_fill_mode(source, ctrl);
                let plan = self.app.plan_fill(source, t, mode);
                let far = if t.0 > source.1 || t.0.1 > source.1.1 { t.1 } else { t.0 };
                let label = plan
                    .cells
                    .iter()
                    .find(|(r, c, _)| (*r, *c) == far)
                    .and_then(|(_, _, cell)| cell.as_ref().map(|c| c.value.clone()))
                    .unwrap_or_default();
                if let Some(pos) = ui.ctx().pointer_hover_pos()
                    && !label.is_empty()
                {
                    egui::Area::new(egui::Id::new("fill_preview"))
                        .order(egui::Order::Tooltip)
                        .fixed_pos(pos + vec2(14.0, 14.0))
                        .interactable(false)
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label(label);
                            });
                        });
                }
            }
            FillTarget::Clear(t) => {
                if let Some(r) = Self::range_rect(layout, cell_rect, t) {
                    painter.rect_filled(r, 0.0, grey.gamma_multiply(0.35));
                }
            }
        }
    }

    /// Default fill mode, switched between copy and series by Ctrl
    /// (Option on macOS), as in Excel.
    fn drag_fill_mode(&self, source: CellRange, toggle: bool) -> FillMode {
        match (self.app.default_fill_mode(source), toggle) {
            (FillMode::Copy, true) => FillMode::Series,
            (_, true) => FillMode::Copy,
            (m, false) => m,
        }
    }

    /// Excel's "Auto Fill Options" button after a fill: redo the fill as
    /// Copy Cells / Fill Series / Fill Formatting Only / Fill Without
    /// Formatting.
    fn auto_fill_options(&mut self, ui: &mut Ui, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect) {
        let Some(lf) = self.grid.last_fill else { return };
        let union = ((lf.source.0.0.min(lf.target.0.0), lf.source.0.1.min(lf.target.0.1)), (lf.source.1.0.max(lf.target.1.0), lf.source.1.1.max(lf.target.1.1)));
        if self.app.undo_stack.len() != lf.undo_len || self.app.get_selection_range() != Some(union) || self.edit.is_some() {
            self.grid.last_fill = None;
            return;
        }
        let Some(r) = Self::range_rect(layout, cell_rect, union) else { return };
        let rect = Rect::from_min_size(r.max + vec2(4.0, 2.0), vec2(74.0, 20.0));
        let resp = ui.put(rect, egui::Button::new(egui::RichText::new("Auto Fill").small()));
        self.grid.overlays.push(rect);
        let color = ui.visuals().text_color();
        let a = pos2(rect.max.x - 11.0, rect.center().y - 2.0);
        ui.painter().add(egui::Shape::convex_polygon(vec![a, a + vec2(7.0, 0.0), a + vec2(3.5, 4.0)], color, Stroke::NONE));
        let mut chosen = None;
        egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            for m in FillMode::ALL {
                if ui.radio(lf.mode == m, m.label()).clicked() {
                    chosen = Some(m);
                    ui.close();
                }
            }
        });
        if let Some(m) = chosen
            && m != lf.mode
        {
            self.app.undo();
            let n = self.app.fill(lf.source, lf.target, m);
            self.app.selection_start = Some(union.0);
            self.app.selection_end = Some(union.1);
            self.grid.last_fill = (n > 0).then_some(LastFill { mode: m, undo_len: self.app.undo_stack.len(), ..lf });
        }
    }

    fn paint(
        &self,
        ui: &Ui,
        layout: &Layout,
        origin: Pos2,
        screen: Rect,
        viewport: Rect,
        cell_rect: &dyn Fn(usize, usize) -> Rect,
    ) {
        let painter = ui.painter_at(screen);
        let v = ui.visuals();
        let dark = v.dark_mode;
        let bg = if dark { Color32::from_gray(30) } else { Color32::WHITE };
        let grid_line = if dark { Color32::from_gray(55) } else { Color32::from_gray(222) };
        let header_bg = if dark { Color32::from_gray(42) } else { Color32::from_gray(243) };
        let header_hl = v.selection.bg_fill.gamma_multiply(0.45);
        let text = v.text_color();
        let accent = v.selection.stroke.color;
        let font = FontId::proportional(FONT_SIZE);
        painter.rect_filled(screen, 0.0, bg);

        let sheet = self.app.workbook.current_sheet();
        if layout.cols() == 0 || layout.row_count() == 0 {
            return;
        }
        let c0 = layout.col_at(viewport.min.x - ROW_HEADER_W);
        let c1 = layout.col_at(viewport.max.x - ROW_HEADER_W);
        let r0 = ((viewport.min.y - HEADER_H).max(0.0) / ROW_H) as usize;
        let r1 = ((viewport.max.y / ROW_H).ceil() as usize + 1).min(layout.row_count());

        // Cells.
        for idx in r0..r1 {
            let row = layout.row_at(idx);
            for col in c0..=c1 {
                let rect = cell_rect(idx, col);
                if rect.width() <= 0.0 {
                    continue;
                }
                if let Some(cd) = sheet.cells.get(&(row, col)) {
                    let cf = sheet.conditional_style_for(row, col);
                    draw_cell(&painter, rect, cd, cf, &font, text, dark);
                }
            }
        }

        // Grid lines.
        let body_top = screen.min.y + HEADER_H;
        let body_left = screen.min.x + ROW_HEADER_W;
        for col in c0..=c1 + 1 {
            if col > layout.cols() {
                break;
            }
            let x = origin.x + ROW_HEADER_W + layout.col_x[col];
            painter.vline(x, body_top..=screen.max.y, Stroke::new(1.0, grid_line));
        }
        for idx in r0..=r1 {
            let y = origin.y + HEADER_H + idx as f32 * ROW_H;
            painter.hline(body_left..=screen.max.x, y, Stroke::new(1.0, grid_line));
        }

        // Selection, cursor, and formula-reference highlight.
        let body = painter.with_clip_rect(Rect::from_min_max(pos2(body_left, body_top), screen.max));
        let range_rect = |(ra, ca): (usize, usize), (rb, cb): (usize, usize)| -> Option<Rect> {
            let (ia, ib) = (layout.index_of(ra.min(rb))?, layout.index_of(ra.max(rb))?);
            Some(cell_rect(ia, ca.min(cb)).union(cell_rect(ib, ca.max(cb))))
        };
        if let Some((a, b)) = self.app.get_selection_range()
            && let Some(rect) = range_rect(a, b)
        {
            body.rect_filled(rect, 0.0, v.selection.bg_fill.gamma_multiply(0.35));
            body.rect_stroke(rect, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
        }
        if let Some(Drag::RefPick { anchor, current }) = self.grid.drag
            && let Some(rect) = range_rect(anchor, current)
        {
            let blue = Color32::from_rgb(40, 110, 230);
            body.rect_filled(rect, 0.0, blue.gamma_multiply(0.12));
            body.rect_stroke(rect, 0.0, Stroke::new(1.5, blue), StrokeKind::Inside);
        }
        if let Some(idx) = layout.index_of(self.app.selected_row)
            && self.app.selected_col < layout.cols()
        {
            let rect = cell_rect(idx, self.app.selected_col);
            body.rect_stroke(rect, 0.0, Stroke::new(2.0, accent), StrokeKind::Inside);
        }
        if let Some(h) = self.handle_rect(layout, cell_rect) {
            body.rect_filled(h.expand(1.0), 0.0, bg);
            body.rect_filled(h, 0.0, accent);
        }

        // Sticky headers.
        let ((sr0, sc0), (sr1, sc1)) = self.selection_or_cursor();
        let header_font = FontId::proportional(12.0);
        let col_header = Rect::from_min_max(screen.min, pos2(screen.max.x, screen.min.y + HEADER_H));
        painter.rect_filled(col_header, 0.0, header_bg);
        let hp = painter.with_clip_rect(Rect::from_min_max(pos2(body_left, screen.min.y), screen.max));
        for col in c0..=c1 {
            let x0 = origin.x + ROW_HEADER_W + layout.col_x[col];
            let x1 = origin.x + ROW_HEADER_W + layout.col_x[col + 1];
            if x1 <= x0 {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, screen.min.y), pos2(x1, screen.min.y + HEADER_H));
            if (sc0..=sc1).contains(&col) {
                hp.rect_filled(r, 0.0, header_hl);
            }
            hp.vline(x1, r.y_range(), Stroke::new(1.0, grid_line));
            hp.text(r.center(), Align2::CENTER_CENTER, Spreadsheet::column_label(col), header_font.clone(), text);
        }
        let row_header = Rect::from_min_max(screen.min, pos2(screen.min.x + ROW_HEADER_W, screen.max.y));
        painter.rect_filled(row_header, 0.0, header_bg);
        let rp = painter.with_clip_rect(Rect::from_min_max(pos2(screen.min.x, body_top), screen.max));
        for idx in r0..r1 {
            let row = layout.row_at(idx);
            let y0 = origin.y + HEADER_H + idx as f32 * ROW_H;
            let r = Rect::from_min_size(pos2(screen.min.x, y0), vec2(ROW_HEADER_W, ROW_H));
            if (sr0..=sr1).contains(&row) {
                rp.rect_filled(r, 0.0, header_hl);
            }
            rp.hline(r.x_range(), y0 + ROW_H, Stroke::new(1.0, grid_line));
            rp.text(r.center(), Align2::CENTER_CENTER, (row + 1).to_string(), header_font.clone(), text);
        }
        let corner = Rect::from_min_size(screen.min, vec2(ROW_HEADER_W, HEADER_H));
        painter.rect_filled(corner, 0.0, header_bg);
        painter.hline(screen.x_range(), screen.min.y + HEADER_H, Stroke::new(1.0, grid_line));
        painter.vline(screen.min.x + ROW_HEADER_W, screen.y_range(), Stroke::new(1.0, grid_line));
    }

    fn hit_test(&self, layout: &Layout, origin: Pos2, screen: Rect, p: Pos2) -> Hit {
        let in_header_row = p.y < screen.min.y + HEADER_H;
        let in_header_col = p.x < screen.min.x + ROW_HEADER_W;
        if !in_header_row && !in_header_col {
            let cell_rect = |row_idx: usize, col: usize| {
                Rect::from_min_size(
                    pos2(origin.x + ROW_HEADER_W + layout.col_x[col], origin.y + HEADER_H + row_idx as f32 * ROW_H),
                    vec2(layout.col_x[col + 1] - layout.col_x[col], ROW_H),
                )
            };
            if self.handle_rect(layout, &cell_rect).is_some_and(|h| h.expand(3.0).contains(p)) {
                return Hit::FillHandle;
            }
        }
        let bx = p.x - origin.x - ROW_HEADER_W;
        let by = p.y - origin.y - HEADER_H;
        let row_idx = ((by / ROW_H).max(0.0) as usize).min(layout.row_count().saturating_sub(1));
        match (in_header_row, in_header_col) {
            (true, true) => Hit::Corner,
            (true, false) => {
                let col = layout.col_at(bx);
                // Grab the border within 4px of a column edge.
                if (layout.col_x[col + 1] - bx).abs() <= 4.0 {
                    Hit::ColBorder(col)
                } else if col > 0 && (bx - layout.col_x[col]).abs() <= 4.0 {
                    Hit::ColBorder(col - 1)
                } else {
                    Hit::ColHeader(col)
                }
            }
            (false, true) => Hit::RowHeader(layout.row_at(row_idx)),
            (false, false) => Hit::Cell(layout.row_at(row_idx), layout.col_at(bx)),
        }
    }

    fn interact(
        &mut self,
        ui: &Ui,
        layout: &Layout,
        origin: Pos2,
        screen: Rect,
        response: &egui::Response,
        overlays: &[Rect],
    ) {
        let (pressed, secondary, down, released, shift, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers.shift,
                i.pointer.interact_pos(),
            )
        });
        let Some(pos) = pos else { return };
        let hit = self.hit_test(layout, origin, screen, pos);
        let hovering = response.hovered() && !overlays.iter().any(|r| r.contains(pos));

        if hovering && matches!(hit, Hit::ColBorder(_)) || matches!(self.grid.drag, Some(Drag::Resize { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if hovering && hit == Hit::FillHandle || matches!(self.grid.drag, Some(Drag::Fill { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        if secondary && hovering
            && let Hit::Cell(r, c) = hit
            && !self.app.is_cell_selected(r, c)
        {
            self.commit_edit(EditMove::Stay);
            self.set_cursor(r, c);
        }

        if pressed && hovering {
            self.on_press(hit, shift, layout);
        } else if down && let Some(drag) = self.grid.drag {
            self.on_drag(drag, hit, pos, layout);
            // Auto-scroll while dragging past the edge.
            let edge = 24.0;
            let mut delta = vec2(0.0, 0.0);
            if pos.y > screen.max.y - edge {
                delta.y = -ROW_H;
            } else if pos.y < screen.min.y + HEADER_H {
                delta.y = ROW_H;
            }
            if pos.x > screen.max.x - edge {
                delta.x = -40.0;
            } else if pos.x < screen.min.x + ROW_HEADER_W {
                delta.x = 40.0;
            }
            if delta != vec2(0.0, 0.0) && !matches!(drag, Drag::Resize { .. }) {
                ui.scroll_with_delta(delta);
            }
        }
        if released && let Some(drag) = self.grid.drag.take() {
            let toggle = ui.input(|i| i.modifiers.command || i.modifiers.alt);
            self.on_release(drag, toggle);
        }

        if response.double_clicked() && hovering {
            match hit {
                Hit::Cell(..) if self.edit.is_none() => self.start_edit(None, true),
                Hit::FillHandle => self.fill_down_to_neighbour(),
                Hit::ColBorder(col) => {
                    self.app.workbook.current_sheet_mut().auto_resize_column(col);
                    self.app.dirty = true;
                }
                _ => {}
            }
        }
    }

    fn on_press(&mut self, hit: Hit, shift: bool, layout: &Layout) {
        // While typing a formula, clicking a cell inserts its reference.
        let picking_ref = self
            .edit
            .as_ref()
            .is_some_and(|e| e.text.starts_with('=') && !matches!(hit, Hit::Cell(r, c) if (r, c) == (e.row, e.col)));
        if picking_ref && let Hit::Cell(r, c) = hit {
            self.grid.drag = Some(Drag::RefPick { anchor: (r, c), current: (r, c) });
            return;
        }
        self.commit_edit(EditMove::Stay);
        let sheet = self.app.workbook.current_sheet();
        let (last_row, last_col) = (sheet.rows - 1, sheet.cols - 1);
        match hit {
            Hit::Cell(r, c) => {
                if shift {
                    let anchor = self.app.selection_start.unwrap_or((self.app.selected_row, self.app.selected_col));
                    self.app.selection_start = Some(anchor);
                    self.app.selection_end = Some((r, c));
                } else {
                    self.set_cursor(r, c);
                    self.grid.drag = Some(Drag::Cells { anchor: (r, c) });
                }
            }
            Hit::ColHeader(c) => {
                self.set_cursor(self.app.selected_row, c);
                self.app.selection_start = Some((0, c));
                self.app.selection_end = Some((last_row, c));
                self.grid.drag = Some(Drag::Cols { anchor: c });
            }
            Hit::RowHeader(r) => {
                self.set_cursor(r, self.app.selected_col);
                self.app.selection_start = Some((r, 0));
                self.app.selection_end = Some((r, last_col));
                self.grid.drag = Some(Drag::Rows { anchor: r });
            }
            Hit::Corner => {
                self.app.selection_start = Some((0, 0));
                self.app.selection_end = Some((last_row, last_col));
            }
            Hit::ColBorder(col) => {
                let start_px = layout.col_x[col + 1] - layout.col_x[col];
                self.grid.drag = Some(Drag::Resize { col, start_px, start_x: f32::NAN });
            }
            Hit::FillHandle => {
                let source = self.selection_or_cursor();
                self.grid.drag = Some(Drag::Fill { source, end: source.1 });
            }
        }
    }

    /// Double-click on the fill handle: fill down as far as the data in
    /// the neighbouring column goes (left column first, as in Excel).
    fn fill_down_to_neighbour(&mut self) {
        let source = self.selection_or_cursor();
        let ((r0, c0), (r1, c1)) = source;
        let sheet = self.app.workbook.current_sheet();
        let filled = |r: usize, c: usize| sheet.cells.get(&(r, c)).is_some_and(|cd| !cd.value.is_empty());
        let neighbour = [c0.checked_sub(1), Some(c1 + 1)]
            .into_iter()
            .flatten()
            .find(|&c| c < sheet.cols && (filled(r0, c) || filled(r1 + 1, c)));
        let Some(nc) = neighbour else { return };
        let mut end = r1;
        while end + 1 < sheet.rows && filled(end + 1, nc) {
            end += 1;
        }
        if end > r1 {
            self.apply_fill(source, ((r1 + 1, c0), (end, c1)), self.app.default_fill_mode(source));
        }
    }

    fn apply_fill(&mut self, source: CellRange, target: CellRange, mode: FillMode) {
        self.grow_to(target.1.0, target.1.1);
        let n = self.app.fill(source, target, mode);
        let union = ((source.0.0.min(target.0.0), source.0.1.min(target.0.1)), (source.1.0.max(target.1.0), source.1.1.max(target.1.1)));
        self.app.selection_start = Some(union.0);
        self.app.selection_end = Some(union.1);
        self.grid.last_fill = (n > 0).then_some(LastFill { source, target, mode, undo_len: self.app.undo_stack.len() });
    }

    fn on_drag(&mut self, drag: Drag, hit: Hit, pos: Pos2, _layout: &Layout) {
        let sheet = self.app.workbook.current_sheet();
        let (last_row, last_col) = (sheet.rows - 1, sheet.cols - 1);
        let cell = match hit {
            Hit::Cell(r, c) => Some((r, c)),
            Hit::FillHandle => None,
            Hit::ColHeader(c) | Hit::ColBorder(c) => Some((self.app.selected_row, c)),
            Hit::RowHeader(r) => Some((r, self.app.selected_col)),
            Hit::Corner => None,
        };
        match drag {
            Drag::Cells { anchor } => {
                if let Some(end) = cell
                    && end != anchor
                {
                    self.app.selection_start = Some(anchor);
                    self.app.selection_end = Some(end);
                }
            }
            Drag::Cols { anchor } => {
                if let Some((_, c)) = cell {
                    self.app.selection_start = Some((0, anchor));
                    self.app.selection_end = Some((last_row, c));
                }
            }
            Drag::Rows { anchor } => {
                if let Some((r, _)) = cell {
                    self.app.selection_start = Some((anchor, 0));
                    self.app.selection_end = Some((r, last_col));
                }
            }
            Drag::RefPick { anchor, .. } => {
                if let Some(current) = cell {
                    self.grid.drag = Some(Drag::RefPick { anchor, current });
                }
            }
            Drag::Fill { source, .. } => {
                if let Some(end) = cell {
                    // Grow the sheet while dragging past its last row/column.
                    self.grow_to(end.0 + 1, end.1 + 1);
                    self.grid.drag = Some(Drag::Fill { source, end });
                }
            }
            Drag::Resize { col, start_px, start_x } => {
                if start_x.is_nan() {
                    self.grid.drag = Some(Drag::Resize { col, start_px, start_x: pos.x });
                    return;
                }
                let px = (start_px + pos.x - start_x).max(20.0);
                let chars = ((px - 10.0) / CHAR_W).round().max(1.0) as usize;
                self.app.workbook.current_sheet_mut().set_column_width(col, chars);
            }
        }
    }

    fn on_release(&mut self, drag: Drag, toggle_mode: bool) {
        match drag {
            Drag::Resize { .. } => self.app.dirty = true,
            Drag::Fill { source, end } => match fill_target(source, end) {
                Some(FillTarget::Fill(t)) => {
                    let mode = self.drag_fill_mode(source, toggle_mode);
                    self.apply_fill(source, t, mode);
                }
                Some(FillTarget::Clear(t)) => {
                    self.app.clear_range(t);
                    // What remains of the source stays selected.
                    let ((r0, c0), (r1, c1)) = source;
                    let rest = if t.0.0 > r0 { ((r0, c0), (t.0.0 - 1, c1)) } else { ((r0, c0), (r1, t.0.1 - 1)) };
                    self.app.selection_start = Some(rest.0);
                    self.app.selection_end = Some(rest.1);
                }
                None => {}
            },
            Drag::RefPick { anchor, current } => {
                let label = |(r, c): (usize, usize)| format!("{}{}", Spreadsheet::column_label(c), r + 1);
                let (a, b) = (
                    (anchor.0.min(current.0), anchor.1.min(current.1)),
                    (anchor.0.max(current.0), anchor.1.max(current.1)),
                );
                let reference = if a == b { label(a) } else { format!("{}:{}", label(a), label(b)) };
                if let Some(edit) = &mut self.edit {
                    // A second click right after the first replaces the
                    // reference instead of appending ("=A1" → "=B2").
                    if let Some(start) = edit.ref_start
                        && start <= edit.text.len()
                    {
                        edit.text.truncate(start);
                    } else if edit.text.ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == ')') {
                        edit.text.push('+');
                    }
                    edit.ref_start = Some(edit.text.len());
                    edit.text.push_str(&reference);
                    edit.focus_pending = true;
                }
            }
            _ => {}
        }
    }

    fn cell_editor(&mut self, ui: &mut Ui, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect, screen: Rect) {
        let Some(edit) = &mut self.edit else { return };
        if !edit.in_cell {
            return;
        }
        let Some(idx) = layout.index_of(edit.row) else { return };
        if edit.col >= layout.cols() {
            return;
        }
        let rect = cell_rect(idx, edit.col);
        if !screen.intersects(rect) {
            return;
        }
        // Grow the editor to fit the text, like Excel does.
        let font = FontId::proportional(FONT_SIZE);
        let text_w = ui.fonts_mut(|f| f.layout_no_wrap(edit.text.clone(), font.clone(), Color32::WHITE).size().x);
        let rect = Rect::from_min_size(rect.min, vec2(rect.width().max(text_w + 16.0), rect.height()));
        let id = egui::Id::new(CELL_EDITOR_ID);
        let resp = ui.put(
            rect,
            TextEdit::singleline(&mut edit.text)
                .id(id)
                .font(font)
                .margin(vec2(3.0, 2.0))
                .desired_width(rect.width()),
        );
        if resp.changed() {
            edit.ref_start = None;
        }
        self.grid.overlays.push(rect);
        if edit.focus_pending {
            edit.focus_pending = false;
            resp.request_focus();
            move_caret_to_end(ui.ctx(), id, edit.text.chars().count());
        }
    }

    fn grid_context_menu(&mut self, ui: &mut Ui) {
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
        if ui.button("Copy as Markdown table").clicked() {
            let (a, b) = self.selection_or_cursor();
            let text = cellar::domain::range_to_markdown(self.app.workbook.current_sheet(), a, b);
            ui.ctx().copy_text(text);
            self.app.status_message = Some("Copied the selection as a Markdown table".into());
        }
        ui.separator();
        self.row_col_buttons(ui);
        ui.separator();
        if ui.button("Insert chart…").clicked() {
            self.open_chart_dialog(None);
        }
        if ui.button("PivotTable…").clicked() {
            self.open_create_pivot();
        }
    }
}

fn draw_cell(
    painter: &Painter,
    rect: Rect,
    cd: &CellData,
    conditional: Option<CellStyle>,
    font: &FontId,
    default_text: Color32,
    dark: bool,
) {
    let mut style = cd.format.as_ref().map(|f| f.style.clone()).unwrap_or_default();
    if let Some(cf) = conditional {
        style.bold |= cf.bold;
        style.underline |= cf.underline;
        if cf.fg_color.is_some() {
            style.fg_color = cf.fg_color;
        }
        if cf.bg_color.is_some() {
            style.bg_color = cf.bg_color;
        }
    }
    if let Some(bg) = &style.bg_color {
        painter.rect_filled(rect.shrink(0.5), 0.0, rgb(bg));
    }
    let shown = match &cd.format {
        Some(fmt) => format_cell_value(&cd.value, fmt),
        None => cd.value.clone(),
    };
    if shown.is_empty() {
        return;
    }
    let is_error = is_error_value(&cd.value);
    let numeric = cd.value.trim().parse::<f64>().is_ok();
    let color = match (&style.fg_color, &style.bg_color) {
        _ if is_error => Color32::from_rgb(210, 50, 50),
        // Black text would vanish on the dark theme's background.
        (Some(TerminalColor::Black), None) if dark => default_text,
        (Some(fg), _) => rgb(fg),
        // Pale fills are designed for dark text.
        (None, Some(_)) => Color32::BLACK,
        (None, None) => default_text,
    };
    let galley = painter.layout_no_wrap(shown, font.clone(), color);
    let size = galley.size();
    let x = if numeric || is_error {
        rect.max.x - 4.0 - size.x
    } else {
        rect.min.x + 4.0
    };
    let pos = pos2(x, rect.center().y - size.y / 2.0);
    let clip = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
    if style.bold {
        // egui's default font has no bold face; overdraw for weight.
        clip.galley(pos + vec2(0.6, 0.0), galley.clone(), color);
    }
    if style.underline {
        let y = pos.y + size.y - 1.0;
        clip.hline(pos.x..=pos.x + size.x, y, Stroke::new(1.0, color));
    }
    clip.galley(pos, galley, color);
}

fn is_error_value(v: &str) -> bool {
    matches!(
        v,
        "#REF!" | "#N/A" | "#DIV/0!" | "#VALUE!" | "#NAME?" | "#NUM!" | "#NULL!" | "#SPILL!" | "#CIRC!"
    )
}

pub fn rgb(c: &TerminalColor) -> Color32 {
    let (r, g, b) = c.rgb();
    Color32::from_rgb(r, g, b)
}
