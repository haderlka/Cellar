//! The spreadsheet grid: a custom-painted, virtualized table inside a
//! `ScrollArea`. Only visible cells are drawn, so large sheets stay fast.
//! Column/row headers are painted at the viewport edge so they stay put
//! while the body scrolls.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui,
};
use eframe::egui::text::{LayoutJob, TextFormat};
use cellar::domain::{
    format_cell_value, formula_references, CellData, CellStyle, FormulaRef, Spreadsheet, TerminalColor, Workbook,
};
use std::collections::HashSet;

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

/// Pixels of a row `pt` points tall; the default 15 pt row is `ROW_H`.
pub fn row_px(pt: usize) -> f32 {
    (pt as f32 * ROW_H / Spreadsheet::DEFAULT_ROW_HEIGHT as f32).max(4.0)
}

fn px_to_pt(px: f32) -> usize {
    (px * Spreadsheet::DEFAULT_ROW_HEIGHT as f32 / ROW_H).round().max(2.0) as usize
}

/// Colours of the references in a formula, in order of appearance (as
/// Excel cycles blue, red, purple, green…). Readable on both themes.
const REF_COLORS: [Color32; 8] = [
    Color32::from_rgb(52, 120, 246),
    Color32::from_rgb(220, 60, 60),
    Color32::from_rgb(150, 80, 200),
    Color32::from_rgb(30, 150, 80),
    Color32::from_rgb(225, 120, 20),
    Color32::from_rgb(0, 150, 170),
    Color32::from_rgb(200, 50, 150),
    Color32::from_rgb(140, 110, 40),
];

/// The references in `formula`, each with its colour. A reference that
/// appears twice keeps one colour.
pub fn colored_refs(formula: &str) -> Vec<(FormulaRef, Color32)> {
    let mut seen: Vec<(Option<String>, CellRange)> = Vec::new();
    formula_references(formula)
        .into_iter()
        .map(|r| {
            let key = (r.sheet.as_ref().map(|s| s.to_lowercase()), r.range);
            let i = seen.iter().position(|k| *k == key).unwrap_or_else(|| {
                seen.push(key);
                seen.len() - 1
            });
            (r, REF_COLORS[i % REF_COLORS.len()])
        })
        .collect()
}

/// `text` laid out with its references coloured, for the cell editor and
/// the formula bar. Text that isn't a formula is laid out plainly.
pub fn formula_layout_job(text: &str, font: FontId, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut pos = 0;
    if text.starts_with('=') {
        for (r, ref_color) in colored_refs(text) {
            job.append(&text[pos..r.span.start], 0.0, TextFormat::simple(font.clone(), color));
            job.append(&text[r.span.clone()], 0.0, TextFormat::simple(font.clone(), ref_color));
            pos = r.span.end;
        }
    }
    job.append(&text[pos..], 0.0, TextFormat::simple(font, color));
    job
}

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
    /// button, formula suggestions). Presses on them must not reach the
    /// grid underneath.
    pub overlays: Vec<Rect>,
    /// The grid's viewport on screen this frame.
    screen: Rect,
}

impl Default for GridState {
    fn default() -> Self {
        Self {
            scroll_to_cursor: false,
            scroll_target: None,
            drag: None,
            last_fill: None,
            overlays: Vec::new(),
            screen: Rect::NOTHING,
        }
    }
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
    RowResize { row: usize, start_px: f32, start_y: f32 },
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
    /// The bottom edge of a row header.
    RowBorder(usize),
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
    /// Shown rows with a custom height, as (display index, pixels),
    /// sorted. Every other row is `ROW_H` tall.
    tall: Vec<(usize, f32)>,
    /// `extra[k]`: how much taller than `ROW_H` the rows `tall[..k]` are
    /// together; `extra[tall.len()]` is the total.
    extra: Vec<f32>,
    /// Frozen panes: columns `..frozen_cols` and display rows
    /// `..frozen_idx` stay put while the rest scrolls under them.
    frozen_cols: usize,
    frozen_idx: usize,
}

impl Layout {
    fn new(sheet: &Spreadsheet, gui: &GuiApp, available: egui::Vec2) -> Self {
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
        let mut layout =
            Self { col_x, visible_rows, rows: sheet.rows, tall: Vec::new(), extra: vec![0.0], frozen_cols: 0, frozen_idx: 0 };
        let mut tall: Vec<(usize, f32)> = sheet
            .row_heights
            .iter()
            .filter_map(|(&r, &pt)| Some((layout.index_of(r)?, row_px(pt))))
            .collect();
        tall.sort_by_key(|&(idx, _)| idx);
        let mut sum = 0.0;
        for &(_, h) in &tall {
            sum += h - ROW_H;
            layout.extra.push(sum);
        }
        layout.tall = tall;
        // Panes frozen so large that nothing could scroll are shown unfrozen.
        let frozen_cols = gui.app.frozen_cols.min(layout.cols());
        if layout.col_x[frozen_cols] < (available.x - ROW_HEADER_W) * 0.8 {
            layout.frozen_cols = frozen_cols;
        }
        let frozen_idx = match &layout.visible_rows {
            Some(v) => v.partition_point(|&r| r < gui.app.frozen_rows),
            None => gui.app.frozen_rows.min(layout.rows),
        };
        if layout.row_y(frozen_idx) < (available.y - HEADER_H) * 0.8 {
            layout.frozen_idx = frozen_idx;
        }
        layout
    }

    fn frozen_w(&self) -> f32 {
        self.col_x[self.frozen_cols]
    }

    fn frozen_h(&self) -> f32 {
        self.row_y(self.frozen_idx)
    }

    /// Screen area of one of the four panes the freeze lines divide the
    /// body into. Cells are clipped to their pane.
    fn pane(&self, screen: Rect, frozen_row: bool, frozen_col: bool) -> Rect {
        let (bx, by) = (screen.min.x + ROW_HEADER_W, screen.min.y + HEADER_H);
        let (fx, fy) = (bx + self.frozen_w(), by + self.frozen_h());
        Rect::from_min_max(
            pos2(if frozen_col { bx } else { fx }, if frozen_row { by } else { fy }),
            pos2(if frozen_col { fx } else { screen.max.x.max(fx) }, if frozen_row { fy } else { screen.max.y.max(fy) }),
        )
    }

    fn pane_of(&self, screen: Rect, row_idx: usize, col: usize) -> Rect {
        self.pane(screen, row_idx < self.frozen_idx, col < self.frozen_cols)
    }

    /// Body-relative position under screen point `p`: frozen panes don't
    /// scroll, so they map straight from the screen.
    fn body_pos(&self, origin: Pos2, screen: Rect, p: Pos2) -> (f32, f32) {
        let (sx, sy) = (p.x - screen.min.x - ROW_HEADER_W, p.y - screen.min.y - HEADER_H);
        (
            if sx < self.frozen_w() { sx } else { p.x - origin.x - ROW_HEADER_W },
            if sy < self.frozen_h() { sy } else { p.y - origin.y - HEADER_H },
        )
    }

    /// The parts of `range` in each pane, as (pane, screen rect).
    fn range_parts(&self, origin: Pos2, screen: Rect, ((r0, c0), (r1, c1)): CellRange) -> Vec<(Rect, Rect)> {
        let mut parts = Vec::new();
        let Some((a, b)) = self.display_span(r0.min(r1), r0.max(r1)) else { return parts };
        let Some(last_col) = self.cols().checked_sub(1) else { return parts };
        let (ca, cb) = (c0.min(c1), c0.max(c1).min(last_col));
        if ca > last_col {
            return parts;
        }
        let split = |lo: usize, hi: usize, at: usize| {
            let frozen = (lo < at).then(|| (lo, hi.min(at.saturating_sub(1))));
            let scrolling = (hi >= at).then(|| (lo.max(at), hi));
            [frozen, scrolling]
        };
        for (ra, rb) in split(a, b, self.frozen_idx).into_iter().flatten() {
            for (xa, xb) in split(ca, cb, self.frozen_cols).into_iter().flatten() {
                let rect = self.cell_rect(origin, screen, ra, xa).union(self.cell_rect(origin, screen, rb, xb));
                parts.push((self.pane_of(screen, ra, xa), rect));
            }
        }
        parts
    }

    /// Top of display row `idx`, relative to the body (`idx` may be
    /// `row_count()`, giving the total height).
    fn row_y(&self, idx: usize) -> f32 {
        let k = self.tall.partition_point(|&(i, _)| i < idx);
        idx as f32 * ROW_H + self.extra[k]
    }

    fn row_h(&self, idx: usize) -> f32 {
        match self.tall.binary_search_by_key(&idx, |&(i, _)| i) {
            Ok(k) => self.tall[k].1,
            Err(_) => ROW_H,
        }
    }

    /// Display row at body-relative y (clamped).
    fn row_idx_at(&self, y: f32) -> usize {
        let y = y.max(0.0);
        let top = |k: usize| self.tall[k].0 as f32 * ROW_H + self.extra[k];
        // Number of tall rows starting at or above y.
        let (mut lo, mut hi) = (0, self.tall.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if top(mid) <= y { lo = mid + 1 } else { hi = mid }
        }
        let idx = match lo.checked_sub(1) {
            None => (y / ROW_H) as usize,
            Some(k) => {
                let (i, h) = self.tall[k];
                let top = i as f32 * ROW_H + self.extra[k];
                if y < top + h { i } else { i + 1 + ((y - top - h) / ROW_H) as usize }
            }
        };
        idx.min(self.row_count().saturating_sub(1))
    }

    fn height(&self) -> f32 {
        self.row_y(self.row_count())
    }

    /// First and last shown display rows within `r0..=r1`.
    fn display_span(&self, r0: usize, r1: usize) -> Option<(usize, usize)> {
        match &self.visible_rows {
            None => (r0 < self.rows).then(|| (r0, r1.min(self.rows - 1))),
            Some(v) => {
                let a = v.partition_point(|&r| r < r0);
                let b = v.partition_point(|&r| r <= r1);
                (a < b).then(|| (a, b - 1))
            }
        }
    }

    fn row_count(&self) -> usize {
        self.visible_rows.as_ref().map_or(self.rows, Vec::len)
    }

    /// Row shown at display index `idx`. Callers clamp `idx` to
    /// `row_count()`, which is 0 when every row is hidden, so fall back to
    /// the last visible row (or `idx` itself) instead of indexing past the end.
    fn row_at(&self, idx: usize) -> usize {
        self.visible_rows.as_ref().map_or(idx, |v| v.get(idx).or(v.last()).copied().unwrap_or(idx))
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

    /// Screen rect of display row `row_idx`, column `col`. Frozen rows
    /// and columns are placed from the viewport, the rest from the
    /// scrolled content's `origin`.
    fn cell_rect(&self, origin: Pos2, screen: Rect, row_idx: usize, col: usize) -> Rect {
        let x0 = if col < self.frozen_cols { screen.min.x } else { origin.x };
        let y0 = if row_idx < self.frozen_idx { screen.min.y } else { origin.y };
        Rect::from_min_size(
            pos2(x0 + ROW_HEADER_W + self.col_x[col], y0 + HEADER_H + self.row_y(row_idx)),
            vec2(self.col_x[col + 1] - self.col_x[col], self.row_h(row_idx)),
        )
    }
}

impl GuiApp {
    pub fn show_grid(&mut self, ui: &mut Ui) {
        self.clamp_selection();
        let sheet_idx = self.app.workbook.active_sheet;
        let layout = Layout::new(self.app.workbook.current_sheet(), self, ui.available_size());
        let content = vec2(
            ROW_HEADER_W + layout.width(),
            HEADER_H + layout.height(),
        );

        egui::ScrollArea::both()
            .id_salt(("grid", sheet_idx))
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                ui.set_min_size(content);
                let origin = ui.min_rect().min;
                let screen = viewport.translate(origin.to_vec2());
                self.grid.screen = screen;
                let cell_rect = |row_idx: usize, col: usize| layout.cell_rect(origin, screen, row_idx, col);

                // Keep the cursor visible after keyboard moves. Expand the
                // target by the header (and frozen pane) size so it isn't
                // hidden under them. Frozen cells are always in view.
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
                        let pad_x = if c < layout.frozen_cols { 0.0 } else { ROW_HEADER_W + layout.frozen_w() };
                        let pad_y = if idx < layout.frozen_idx { 0.0 } else { HEADER_H + layout.frozen_h() };
                        let padded = Rect::from_min_max(
                            rect.min - vec2(pad_x, pad_y),
                            rect.max + vec2(if pad_x > 0.0 { 4.0 } else { 0.0 }, if pad_y > 0.0 { 4.0 } else { 0.0 }),
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
                self.validation_dropdown(ui, &layout, &cell_rect, screen);
                self.cell_hover(ui, &layout, origin, screen, &response);
                response.context_menu(|ui| self.grid_context_menu(ui));
            });
    }

    /// Keep the selection inside the sheet. Deleting columns/rows, undo and
    /// redo shrink the sheet without touching the selection, and the grid
    /// indexes column geometry by the selection's columns.
    fn clamp_selection(&mut self) {
        // Tracer arrows belong to the cell they were asked for.
        let at = (self.app.workbook.active_sheet, (self.app.selected_row, self.app.selected_col));
        if self.tools.trace.as_ref().is_some_and(|t| (t.sheet, t.cell) != at) {
            self.tools.trace = None;
        }
        let sheet = self.app.workbook.current_sheet();
        let (last_row, last_col) = (sheet.rows.saturating_sub(1), sheet.cols.saturating_sub(1));
        let clamp = |p: &mut Option<(usize, usize)>| {
            if let Some((r, c)) = p {
                *r = (*r).min(last_row);
                *c = (*c).min(last_col);
            }
        };
        clamp(&mut self.app.selection_start);
        clamp(&mut self.app.selection_end);
        self.app.selected_row = self.app.selected_row.min(last_row);
        self.app.selected_col = self.app.selected_col.min(last_col);
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
        let corner = cell_rect(idx, c1).max;
        // Not when the corner is scrolled under a frozen pane.
        let pane = layout.pane_of(self.grid.screen, idx, c1);
        pane.expand(HANDLE).contains(corner).then(|| Rect::from_center_size(corner, vec2(HANDLE, HANDLE)))
    }

    fn range_rect(layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect, ((ra, ca), (rb, cb)): CellRange) -> Option<Rect> {
        let (ia, ib) = (layout.index_of(ra.min(rb))?, layout.index_of(ra.max(rb))?);
        let last_col = layout.cols().checked_sub(1)?;
        Some(cell_rect(ia, ca.min(cb).min(last_col)).union(cell_rect(ib, ca.max(cb).min(last_col))))
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

    /// Excel's in-cell dropdown: when the cursor's column only allows a
    /// list of values (Data → Data Validation), a button beside the cell
    /// offers them.
    fn validation_dropdown(&mut self, ui: &mut Ui, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect, screen: Rect) {
        if self.edit.is_some() || self.grid.drag.is_some() {
            return;
        }
        let (row, col) = (self.app.selected_row, self.app.selected_col);
        let Some(choices) = self.app.validation_choices(col) else { return };
        let Some(idx) = layout.index_of(row).filter(|_| col < layout.cols()) else { return };
        let cell = cell_rect(idx, col);
        if !layout.pane_of(screen, idx, col).intersects(cell) {
            return;
        }
        let rect = Rect::from_min_size(cell.right_top() + vec2(1.0, 0.0), vec2(18.0, cell.height()));
        let resp = ui.put(rect, egui::Button::new(""));
        self.grid.overlays.push(rect);
        let a = rect.center() + vec2(-3.5, -2.0);
        let arrow = vec![a, a + vec2(7.0, 0.0), a + vec2(3.5, 4.0)];
        ui.painter().add(egui::Shape::convex_polygon(arrow, ui.visuals().text_color(), Stroke::NONE));
        let mut chosen = None;
        egui::Popup::menu(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for choice in &choices {
                    if ui.button(choice).clicked() {
                        chosen = Some(choice.clone());
                        ui.close();
                    }
                }
            });
        });
        if let Some(value) = chosen {
            self.start_edit(Some(value), true);
            self.commit_edit(EditMove::Stay);
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

        let wb = &self.app.workbook;
        let sheet = wb.current_sheet();
        if layout.cols() == 0 || layout.row_count() == 0 {
            return;
        }
        // Visible rows and columns: the frozen ones, then those scrolled
        // into view beside them.
        let (fc, fr) = (layout.frozen_cols, layout.frozen_idx);
        let c0 = layout.col_at(viewport.min.x + layout.frozen_w() - ROW_HEADER_W).max(fc);
        let c1 = layout.col_at(viewport.max.x - ROW_HEADER_W);
        let r0 = layout.row_idx_at(viewport.min.y + layout.frozen_h() - HEADER_H).max(fr);
        let r1 = (layout.row_idx_at(viewport.max.y) + 2).min(layout.row_count());
        let row_groups = [(true, 0..fr), (false, r0..r1.max(r0))];
        let col_groups = [(true, 0..fc), (false, c0..(c1 + 1).max(c0))];
        let body_top = screen.min.y + HEADER_H;
        let body_left = screen.min.x + ROW_HEADER_W;
        let (fx, fy) = (body_left + layout.frozen_w(), body_top + layout.frozen_h());

        // Cells.
        let found: HashSet<(usize, usize)> = match &self.tools.find {
            Some(_) => self.app.find_replace_results.iter().copied().collect(),
            None => HashSet::new(),
        };
        let has_tables = !sheet.tables.is_empty();
        let look = CellLook { font: &font, text, dark, show_formulas: self.tools.show_formulas };
        // Conditional formats may compare with other sheets and
        // PivotTables, which need the workbook to resolve.
        cellar::domain::with_recalc_context(wb, || {
            for (frozen_row, rows) in &row_groups {
                for (frozen_col, cols) in &col_groups {
                    let pane = painter.with_clip_rect(layout.pane(screen, *frozen_row, *frozen_col));
                    for idx in rows.clone() {
                        let row = layout.row_at(idx);
                        for col in cols.clone() {
                            let rect = cell_rect(idx, col);
                            if rect.width() <= 0.0 {
                                continue;
                            }
                            match sheet.cells.get(&(row, col)) {
                                Some(cd) => {
                                    let style = sheet.display_style(row, col);
                                    let chart = if look.show_formulas { None } else { sparkline(wb, cd) };
                                    draw_cell(&pane, rect, cd, &style, chart.is_some(), &look);
                                    if let Some((values, columns)) = chart {
                                        let color = style.fg_color.as_ref().map_or(REF_COLORS[0], rgb);
                                        draw_sparkline(&pane, rect, &values, columns, color);
                                    }
                                }
                                // Table banding covers cells that hold nothing.
                                None if has_tables => {
                                    if let Some(fill) = sheet.table_style_for(row, col).and_then(|s| s.bg_color) {
                                        pane.rect_filled(rect.shrink(0.5), 0.0, rgb(&fill));
                                    }
                                }
                                None => {}
                            }
                            if found.contains(&(row, col)) {
                                pane.rect_filled(rect.shrink(0.5), 0.0, Color32::from_rgb(255, 210, 0).gamma_multiply(0.35));
                            }
                        }
                    }
                }
            }
        });

        // Grid lines. Frozen edges sit at fixed positions; scrolled ones
        // are clipped to their side of the freeze line.
        let line = Stroke::new(1.0, grid_line);
        let scrolled_cols = painter.with_clip_rect(Rect::from_min_max(pos2(fx, screen.min.y), screen.max));
        for col in 0..=fc {
            painter.vline(body_left + layout.col_x[col], body_top..=screen.max.y, line);
        }
        for col in c0..=(c1 + 1).min(layout.cols()) {
            scrolled_cols.vline(origin.x + ROW_HEADER_W + layout.col_x[col], body_top..=screen.max.y, line);
        }
        let scrolled_rows = painter.with_clip_rect(Rect::from_min_max(pos2(screen.min.x, fy), screen.max));
        for idx in 0..=fr {
            painter.hline(body_left..=screen.max.x, body_top + layout.row_y(idx), line);
        }
        for idx in r0..=r1 {
            scrolled_rows.hline(body_left..=screen.max.x, origin.y + HEADER_H + layout.row_y(idx), line);
        }

        // Selection, cursor, and formula-reference highlight, each drawn
        // pane by pane so nothing shows through a frozen pane.
        let body = painter.with_clip_rect(Rect::from_min_max(pos2(body_left, body_top), screen.max));
        let outline = |range: CellRange, fill: Color32, stroke: Stroke| {
            for (pane, rect) in layout.range_parts(origin, screen, range) {
                let p = painter.with_clip_rect(pane);
                p.rect_filled(rect, 0.0, fill);
                p.rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
            }
        };
        if let Some(range) = self.app.get_selection_range() {
            outline(range, v.selection.bg_fill.gamma_multiply(0.35), Stroke::new(1.0, accent));
        }
        if let Some(Drag::RefPick { anchor, current }) = self.grid.drag {
            let blue = Color32::from_rgb(40, 110, 230);
            outline((anchor, current), blue.gamma_multiply(0.12), Stroke::new(1.5, blue));
        }
        for (range, color) in self.formula_highlights(layout) {
            outline(range, color.gamma_multiply(0.10), Stroke::new(1.5, color));
        }
        let cursor = (self.app.selected_row, self.app.selected_col);
        outline((cursor, cursor), Color32::TRANSPARENT, Stroke::new(2.0, accent));
        self.paint_trace(layout, &body, &outline, cell_rect);
        if let Some(h) = self.handle_rect(layout, cell_rect) {
            body.rect_filled(h.expand(1.0), 0.0, bg);
            body.rect_filled(h, 0.0, accent);
        }

        // Sticky headers.
        let ((sr0, sc0), (sr1, sc1)) = self.selection_or_cursor();
        let header_font = FontId::proportional(12.0);
        let col_header = Rect::from_min_max(screen.min, pos2(screen.max.x, screen.min.y + HEADER_H));
        painter.rect_filled(col_header, 0.0, header_bg);
        for (frozen, cols) in &col_groups {
            let (left, right, base) =
                if *frozen { (body_left, fx, screen.min.x) } else { (fx, screen.max.x, origin.x) };
            let hp = painter.with_clip_rect(Rect::from_min_max(pos2(left, screen.min.y), pos2(right, screen.max.y)));
            for col in cols.clone() {
                let x0 = base + ROW_HEADER_W + layout.col_x[col];
                let x1 = base + ROW_HEADER_W + layout.col_x[col + 1];
                if x1 <= x0 {
                    continue;
                }
                let r = Rect::from_min_max(pos2(x0, screen.min.y), pos2(x1, screen.min.y + HEADER_H));
                if (sc0..=sc1).contains(&col) {
                    hp.rect_filled(r, 0.0, header_hl);
                }
                hp.vline(x1, r.y_range(), line);
                hp.text(r.center(), Align2::CENTER_CENTER, Spreadsheet::column_label(col), header_font.clone(), text);
                if self.app.filter_column == Some(col) && self.app.filter_value.is_some() {
                    // A funnel marks the filtered column.
                    let c = pos2(x1 - 9.0, r.center().y);
                    let funnel = vec![c + vec2(-4.5, -4.0), c + vec2(4.5, -4.0), c + vec2(0.0, 1.0)];
                    hp.add(egui::Shape::convex_polygon(funnel, accent, Stroke::NONE));
                    hp.line_segment([c, c + vec2(0.0, 5.0)], Stroke::new(1.6, accent));
                }
            }
        }
        let row_header = Rect::from_min_max(screen.min, pos2(screen.min.x + ROW_HEADER_W, screen.max.y));
        painter.rect_filled(row_header, 0.0, header_bg);
        for (frozen, rows) in &row_groups {
            let (top, bottom, base) =
                if *frozen { (body_top, fy, screen.min.y) } else { (fy, screen.max.y, origin.y) };
            let rp = painter.with_clip_rect(Rect::from_min_max(pos2(screen.min.x, top), pos2(screen.max.x, bottom)));
            for idx in rows.clone() {
                let row = layout.row_at(idx);
                let y0 = base + HEADER_H + layout.row_y(idx);
                let r = Rect::from_min_size(pos2(screen.min.x, y0), vec2(ROW_HEADER_W, layout.row_h(idx)));
                if (sr0..=sr1).contains(&row) {
                    rp.rect_filled(r, 0.0, header_hl);
                }
                rp.hline(r.x_range(), r.max.y, line);
                rp.text(r.center(), Align2::CENTER_CENTER, (row + 1).to_string(), header_font.clone(), text);
            }
        }
        let corner = Rect::from_min_size(screen.min, vec2(ROW_HEADER_W, HEADER_H));
        painter.rect_filled(corner, 0.0, header_bg);
        painter.hline(screen.x_range(), screen.min.y + HEADER_H, line);
        painter.vline(screen.min.x + ROW_HEADER_W, screen.y_range(), line);

        // Freeze lines, across the headers too.
        let freeze = Stroke::new(1.0, if dark { Color32::from_gray(130) } else { Color32::from_gray(120) });
        if fc > 0 {
            painter.vline(fx, screen.y_range(), freeze);
        }
        if fr > 0 {
            painter.hline(screen.x_range(), fy, freeze);
        }
    }

    /// Formulas → Trace Precedents / Dependents: arrows into and out of
    /// the traced cell, like Excel's blue tracer arrows.
    fn paint_trace(
        &self,
        layout: &Layout,
        body: &Painter,
        outline: &dyn Fn(CellRange, Color32, Stroke),
        cell_rect: &dyn Fn(usize, usize) -> Rect,
    ) {
        let Some(trace) = &self.tools.trace else { return };
        if trace.sheet != self.app.workbook.active_sheet {
            return;
        }
        let blue = Color32::from_rgb(52, 120, 246);
        let stroke = Stroke::new(1.5, blue);
        let center = |(r, c): (usize, usize)| {
            let idx = layout.index_of(r)?;
            (c < layout.cols()).then(|| cell_rect(idx, c).center())
        };
        let arrow = |from: (usize, usize), to: (usize, usize)| {
            if let (Some(a), Some(b)) = (center(from), center(to))
                && a != b
            {
                body.circle_filled(a, 3.0, blue);
                body.arrow(a, b - a, stroke);
            }
        };
        for range in &trace.precedents {
            if range.0 != range.1 {
                outline(*range, Color32::TRANSPARENT, Stroke::new(1.0, blue));
            }
            arrow(range.0, trace.cell);
        }
        for dependent in &trace.dependents {
            arrow(trace.cell, *dependent);
        }
    }

    /// Hovering a cell shows its comment, and where its link goes.
    fn cell_hover(&mut self, ui: &Ui, layout: &Layout, origin: Pos2, screen: Rect, response: &egui::Response) {
        if self.grid.drag.is_some() || self.edit.is_some() || !response.hovered() {
            return;
        }
        let Some(pos) = ui.ctx().pointer_hover_pos() else { return };
        if self.grid.overlays.iter().any(|r| r.contains(pos)) {
            return;
        }
        let Hit::Cell(row, col) = self.hit_test(layout, origin, screen, pos) else { return };
        if self.tools.painter.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Copy);
        }
        let Some(cd) = self.app.workbook.current_sheet().cells.get(&(row, col)) else { return };
        let mut lines: Vec<String> = cd.comment.iter().cloned().collect();
        if let Some(url) = self.hyperlink_url(cd) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            let key = if ui.ctx().os() == egui::os::OperatingSystem::Mac { "Cmd" } else { "Ctrl" };
            lines.push(format!("{}\n{}+click to open", url, key));
        }
        if lines.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("cell_hover"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos + vec2(14.0, 16.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(320.0);
                    for line in lines {
                        ui.label(line);
                    }
                });
            });
    }

    /// The address a `=HYPERLINK(address, …)` cell opens.
    pub fn hyperlink_url(&self, cd: &CellData) -> Option<String> {
        let formula = cd.formula.as_deref().filter(|f| is_hyperlink(f))?;
        // The first argument: up to the first comma outside quotes and brackets.
        let args = &formula["=HYPERLINK(".len()..];
        let (mut depth, mut in_string, mut end) = (0usize, false, args.len());
        for (i, ch) in args.char_indices() {
            match ch {
                '"' => in_string = !in_string,
                '(' if !in_string => depth += 1,
                ')' | ',' if !in_string && depth == 0 => {
                    end = i;
                    break;
                }
                ')' if !in_string => depth -= 1,
                _ => {}
            }
        }
        let wb = &self.app.workbook;
        let evaluator = cellar::domain::FormulaEvaluator::for_workbook(wb, wb.current_sheet(), &wb.named_ranges);
        let url = evaluator.evaluate_formula(&format!("={}", &args[..end]));
        (!url.is_empty() && !url.starts_with('#')).then_some(url)
    }

    fn hit_test(&self, layout: &Layout, origin: Pos2, screen: Rect, p: Pos2) -> Hit {
        let in_header_row = p.y < screen.min.y + HEADER_H;
        let in_header_col = p.x < screen.min.x + ROW_HEADER_W;
        if !in_header_row && !in_header_col {
            let cell_rect = |row_idx: usize, col: usize| layout.cell_rect(origin, screen, row_idx, col);
            if self.handle_rect(layout, &cell_rect).is_some_and(|h| h.expand(3.0).contains(p)) {
                return Hit::FillHandle;
            }
        }
        let (bx, by) = layout.body_pos(origin, screen, p);
        let row_idx = layout.row_idx_at(by);
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
            (false, true) => {
                // Grab the border within 3px of a row edge.
                let top = layout.row_y(row_idx);
                if (top + layout.row_h(row_idx) - by).abs() <= 3.0 {
                    Hit::RowBorder(layout.row_at(row_idx))
                } else if row_idx > 0 && (by - top).abs() <= 3.0 {
                    Hit::RowBorder(layout.row_at(row_idx - 1))
                } else {
                    Hit::RowHeader(layout.row_at(row_idx))
                }
            }
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
        if hovering && matches!(hit, Hit::RowBorder(_)) || matches!(self.grid.drag, Some(Drag::RowResize { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
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
            let open_link = ui.input(|i| i.modifiers.command) && self.edit.is_none();
            self.on_press(hit, shift, open_link, layout);
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
            if delta != vec2(0.0, 0.0) && !matches!(drag, Drag::Resize { .. } | Drag::RowResize { .. }) {
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
                    for c in self.resize_targets(col, false) {
                        self.app.workbook.current_sheet_mut().auto_resize_column(c);
                    }
                    self.app.dirty = true;
                }
                // Text is one line, so AutoFit is the default height.
                Hit::RowBorder(row) => {
                    for r in self.resize_targets(row, true) {
                        self.app.workbook.current_sheet_mut().set_row_height(r, Spreadsheet::DEFAULT_ROW_HEIGHT);
                    }
                    self.app.dirty = true;
                }
                _ => {}
            }
        }
    }

    fn on_press(&mut self, hit: Hit, shift: bool, open_link: bool, layout: &Layout) {
        // While typing a formula, clicking a cell inserts its reference.
        let active = self.app.workbook.active_sheet;
        let picking_ref = self.edit.as_ref().is_some_and(|e| {
            e.text.starts_with('=')
                && !matches!(hit, Hit::Cell(r, c) if (active, r, c) == (e.sheet, e.row, e.col))
        });
        if picking_ref && let Hit::Cell(r, c) = hit {
            self.grid.drag = Some(Drag::RefPick { anchor: (r, c), current: (r, c) });
            return;
        }
        // A dialog asking for a reference takes the cells clicked or
        // dragged over, like Excel's range boxes.
        if let Hit::Cell(r, c) = hit
            && self.edit.is_none()
            && self.tools.picking().is_some()
        {
            self.grid.drag = Some(Drag::RefPick { anchor: (r, c), current: (r, c) });
            return;
        }
        if open_link
            && let Hit::Cell(r, c) = hit
            && let Some(url) = self.app.workbook.current_sheet().cells.get(&(r, c)).and_then(|cd| self.hyperlink_url(cd))
        {
            self.open_link(&url);
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
            Hit::RowBorder(row) => {
                let start_px = layout.index_of(row).map_or(ROW_H, |idx| layout.row_h(idx));
                self.grid.drag = Some(Drag::RowResize { row, start_px, start_y: f32::NAN });
            }
            Hit::FillHandle => {
                let source = self.selection_or_cursor();
                self.grid.drag = Some(Drag::Fill { source, end: source.1 });
            }
        }
    }

    /// Rows (or columns) a border drag on `index` resizes: every selected
    /// whole row (column) when `index` is one of them, as in Excel.
    fn resize_targets(&self, index: usize, rows: bool) -> std::ops::RangeInclusive<usize> {
        let sheet = self.app.workbook.current_sheet();
        if let Some(((r0, c0), (r1, c1))) = self.app.get_selection_range() {
            let whole_rows = c0 == 0 && c1 + 1 >= sheet.cols;
            let whole_cols = r0 == 0 && r1 + 1 >= sheet.rows;
            // Capped: Select All on a big sheet shouldn't store a million heights.
            if rows && whole_rows && (r0..=r1).contains(&index) {
                return r0..=r1.min(r0 + 10_000);
            }
            if !rows && whole_cols && (c0..=c1).contains(&index) {
                return c0..=c1;
            }
        }
        index..=index
    }

    /// Ranges on the shown sheet that the formula being edited (or the
    /// cursor cell's formula) reads, each with its reference colour.
    fn formula_highlights(&self, layout: &Layout) -> Vec<(CellRange, Color32)> {
        let wb = &self.app.workbook;
        let (formula, home) = match &self.edit {
            Some(e) if e.text.starts_with('=') => (e.text.as_str(), e.sheet),
            Some(_) => return Vec::new(),
            None => match wb
                .current_sheet()
                .cells
                .get(&(self.app.selected_row, self.app.selected_col))
                .and_then(|c| c.formula.as_deref())
            {
                Some(f) => (f, wb.active_sheet),
                None => return Vec::new(),
            },
        };
        let shown = &wb.sheet_names[wb.active_sheet];
        let last_col = layout.cols().saturating_sub(1);
        colored_refs(formula)
            .into_iter()
            .filter(|(r, _)| match &r.sheet {
                None => home == wb.active_sheet,
                Some(name) => name.eq_ignore_ascii_case(shown),
            })
            .filter_map(|(r, color)| {
                let ((r0, c0), (r1, c1)) = r.range;
                let (a, b) = layout.display_span(r0, r1)?;
                (c0 <= last_col).then(|| (((layout.row_at(a), c0), (layout.row_at(b), c1.min(last_col))), color))
            })
            .collect()
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
            Hit::RowHeader(r) | Hit::RowBorder(r) => Some((r, self.app.selected_col)),
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
                for c in self.resize_targets(col, false) {
                    self.app.workbook.current_sheet_mut().set_column_width(c, chars);
                }
            }
            Drag::RowResize { row, start_px, start_y } => {
                if start_y.is_nan() {
                    self.grid.drag = Some(Drag::RowResize { row, start_px, start_y: pos.y });
                    return;
                }
                let pt = px_to_pt(start_px + pos.y - start_y);
                for r in self.resize_targets(row, true) {
                    self.app.workbook.current_sheet_mut().set_row_height(r, pt);
                }
            }
        }
    }

    fn on_release(&mut self, drag: Drag, toggle_mode: bool) {
        match drag {
            Drag::Resize { .. } | Drag::RowResize { .. } => self.app.dirty = true,
            // Format Painter: the cells just clicked or dragged over take
            // the picked-up formatting.
            Drag::Cells { .. } | Drag::Rows { .. } | Drag::Cols { .. } => {
                if let Some(block) = self.tools.painter.take() {
                    let target = self.selection_or_cursor();
                    self.app.apply_formats(&block, target);
                }
            }
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
                if self.edit.is_none() {
                    self.pick_for_dialog((a, b));
                    return;
                }
                let reference = if a == b { label(a) } else { format!("{}:{}", label(a), label(b)) };
                let prefix = self.reference_sheet_prefix().unwrap_or_default();
                self.insert_reference(&format!("{}{}", prefix, reference));
            }
        }
    }

    /// `Sheet2!` when the formula being typed belongs to another sheet than
    /// the one shown, so references clicked here name their sheet.
    pub fn reference_sheet_prefix(&self) -> Option<String> {
        let edit = self.edit.as_ref()?;
        let wb = &self.app.workbook;
        (edit.sheet != wb.active_sheet)
            .then(|| format!("{}!", cellar::domain::format_sheet_name(&wb.sheet_names[wb.active_sheet])))
    }

    /// Point mode: put a clicked reference (a cell, a range, or a
    /// PivotTable value's GETPIVOTDATA call) into the formula being typed.
    pub fn insert_reference(&mut self, reference: &str) {
        let Some(edit) = &mut self.edit else { return };
        // A second click right after the first replaces the reference
        // instead of appending ("=A1" → "=B2").
        if let Some(start) = edit.ref_start
            && start <= edit.text.len()
        {
            edit.text.truncate(start);
        } else if edit.text.ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == ')') {
            edit.text.push('+');
        }
        edit.ref_start = Some(edit.text.len());
        edit.text.push_str(reference);
        edit.focus_pending = true;
    }

    fn cell_editor(&mut self, ui: &mut Ui, layout: &Layout, cell_rect: &dyn Fn(usize, usize) -> Rect, screen: Rect) {
        let Some(edit) = &mut self.edit else { return };
        // On another sheet (picking references there) the formula bar edits.
        if !edit.in_cell || edit.sheet != self.app.workbook.active_sheet {
            return;
        }
        let Some(idx) = layout.index_of(edit.row) else { return };
        if edit.col >= layout.cols() {
            return;
        }
        let rect = cell_rect(idx, edit.col);
        // Not while scrolled out of view or under a frozen pane.
        if !screen.intersects(rect) || !layout.pane_of(screen, idx, edit.col).intersects(rect) {
            return;
        }
        // Grow the editor to fit the text, like Excel does.
        let font = FontId::proportional(FONT_SIZE);
        let text_w = ui.fonts_mut(|f| f.layout_no_wrap(edit.text.clone(), font.clone(), Color32::WHITE).size().x);
        let rect = Rect::from_min_size(rect.min, vec2(rect.width().max(text_w + 16.0), rect.height()));
        let id = egui::Id::new(CELL_EDITOR_ID);
        let color = ui.visuals().text_color();
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            ui.fonts_mut(|f| f.layout_job(formula_layout_job(buf.as_str(), font.clone(), color)))
        };
        let resp = ui.put(
            rect,
            TextEdit::singleline(&mut edit.text)
                .id(id)
                .layouter(&mut layouter)
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
            let caret = edit.caret.take().unwrap_or(edit.text.chars().count());
            move_caret_to_end(ui.ctx(), id, caret);
        }
        self.formula_assist(ui.ctx(), id, rect);
    }

    fn grid_context_menu(&mut self, ui: &mut Ui) {
        use crate::shortcuts::Action;
        let ctx = ui.ctx().clone();
        for a in [Action::Cut, Action::Copy, Action::Paste, Action::PasteSpecial, Action::ClearContents, Action::CopyMarkdown] {
            self.menu_item(ui, &ctx, a);
        }
        ui.separator();
        for a in [Action::InsertCells, Action::DeleteCells] {
            self.menu_item(ui, &ctx, a);
        }
        self.row_col_buttons(ui, &ctx);
        ui.separator();
        for a in [Action::SortAscending, Action::SortDescending, Action::FilterBySelection, Action::ClearFilter] {
            self.menu_item(ui, &ctx, a);
        }
        ui.separator();
        for a in [Action::EditComment, Action::InsertHyperlink, Action::FormatPainter] {
            self.menu_item(ui, &ctx, a);
        }
        ui.separator();
        for a in [Action::InsertChart, Action::PivotTable] {
            self.menu_item(ui, &ctx, a);
        }
    }
}

/// How cells are drawn this frame.
struct CellLook<'a> {
    font: &'a FontId,
    text: Color32,
    dark: bool,
    /// View → Show Formulas: formula text instead of results.
    show_formulas: bool,
}

fn draw_cell(painter: &Painter, rect: Rect, cd: &CellData, style: &CellStyle, hide_text: bool, look: &CellLook) {
    if let Some(bg) = &style.bg_color {
        painter.rect_filled(rect.shrink(0.5), 0.0, rgb(bg));
    }
    if cd.comment.is_some() {
        // Excel's red corner marks a cell with a comment.
        let c = rect.right_top() + vec2(-0.5, 0.5);
        let corner = vec![c + vec2(-6.0, 0.0), c, c + vec2(0.0, 6.0)];
        painter.add(egui::Shape::convex_polygon(corner, Color32::from_rgb(210, 50, 50), Stroke::NONE));
    }
    let formula = cd.formula.as_deref().filter(|_| look.show_formulas);
    let shown = match (formula, &cd.format) {
        (Some(f), _) => f.to_string(),
        (None, Some(fmt)) => format_cell_value(&cd.value, fmt),
        (None, None) => cd.value.clone(),
    };
    if shown.is_empty() || hide_text {
        return;
    }
    let is_error = formula.is_none() && is_error_value(&cd.value);
    let numeric = formula.is_none() && cd.value.trim().parse::<f64>().is_ok();
    let link = formula.is_none() && cd.formula.as_deref().is_some_and(is_hyperlink);
    let color = match (&style.fg_color, &style.bg_color) {
        _ if is_error => Color32::from_rgb(210, 50, 50),
        // Black text would vanish on the dark theme's background.
        (Some(TerminalColor::Black), None) if look.dark => look.text,
        (Some(fg), _) => rgb(fg),
        (None, None) if link && look.dark => Color32::from_rgb(110, 170, 255),
        (None, _) if link => Color32::from_rgb(20, 90, 200),
        // Pale fills are designed for dark text.
        (None, Some(_)) => Color32::BLACK,
        (None, None) => look.text,
    };
    let galley = painter.layout_no_wrap(shown, look.font.clone(), color);
    let size = galley.size();
    let x = if numeric || is_error {
        rect.max.x - 4.0 - size.x
    } else {
        rect.min.x + 4.0
    };
    // Bottom-aligned like Excel: tall rows keep the text on their last line.
    let pos = pos2(x, rect.max.y - (ROW_H + size.y) / 2.0);
    let clip = painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
    if style.bold {
        // egui's default font has no bold face; overdraw for weight.
        clip.galley(pos + vec2(0.6, 0.0), galley.clone(), color);
    }
    if style.underline || link {
        let y = pos.y + size.y - 1.0;
        clip.hline(pos.x..=pos.x + size.x, y, Stroke::new(1.0, color));
    }
    clip.galley(pos, galley, color);
}

fn is_hyperlink(formula: &str) -> bool {
    formula.get(..11).is_some_and(|start| start.eq_ignore_ascii_case("=HYPERLINK("))
}

/// The numbers a `=SPARKLINE(range, …)` cell charts, and whether as
/// columns (`"column"` or `"bar"`) instead of a line. The chart is drawn
/// from the range itself; the cell's value is only a text rendering.
fn sparkline(wb: &Workbook, cd: &CellData) -> Option<(Vec<f64>, bool)> {
    let formula = cd.formula.as_deref()?;
    if !formula.get(..11).is_some_and(|start| start.eq_ignore_ascii_case("=SPARKLINE(")) {
        return None;
    }
    let reference = formula_references(formula).into_iter().next()?;
    let sheet = match &reference.sheet {
        Some(name) => &wb.sheets[wb.sheet_names.iter().position(|n| n.eq_ignore_ascii_case(name))?],
        None => wb.current_sheet(),
    };
    let ((r0, c0), (r1, c1)) = reference.range;
    let (last_row, last_col) = sheet.last_cell();
    let values: Vec<f64> = (r0..=r1.min(last_row))
        .flat_map(|r| (c0..=c1.min(last_col)).map(move |c| (r, c)))
        .take(2000)
        .filter_map(|pos| sheet.cells.get(&pos)?.value.trim().parse().ok())
        .collect();
    let lower = formula.to_lowercase();
    (!values.is_empty()).then(|| (values, lower.contains("\"column\"") || lower.contains("\"bar\"")))
}

fn draw_sparkline(painter: &Painter, rect: Rect, values: &[f64], columns: bool, color: Color32) {
    let area = rect.shrink2(vec2(4.0, 3.0));
    if area.width() <= 2.0 || area.height() <= 2.0 {
        return;
    }
    let (mut lo, mut hi) = values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    if columns {
        // Columns grow from zero.
        (lo, hi) = (lo.min(0.0), hi.max(0.0));
    }
    let span = if hi > lo { hi - lo } else { 1.0 };
    let y = |v: f64| area.max.y - ((v - lo) / span) as f32 * area.height();
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    if columns {
        let step = area.width() / values.len() as f32;
        for (i, v) in values.iter().enumerate() {
            let x = area.min.x + i as f32 * step;
            let bar = Rect::from_two_pos(pos2(x + step * 0.1, y(0.0)), pos2(x + step * 0.9, y(*v)));
            let fill = if *v < 0.0 { Color32::from_rgb(210, 60, 60) } else { color };
            painter.rect_filled(bar, 0.0, fill);
        }
    } else {
        let step = area.width() / (values.len().max(2) - 1) as f32;
        let points: Vec<Pos2> = values.iter().enumerate().map(|(i, v)| pos2(area.min.x + i as f32 * step, y(*v))).collect();
        if let [only] = points[..] {
            painter.circle_filled(only, 2.0, color);
        } else {
            painter.add(egui::Shape::line(points, Stroke::new(1.5, color)));
        }
    }
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
