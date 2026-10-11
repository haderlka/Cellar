//! Dialogs for the Edit, Format, Data and Formulas menus: find and
//! replace, go to, comments, paste special, links, conditional formatting,
//! tables, filter, data validation, names, sort, remove duplicates, text
//! to columns and goal seek. The work itself is done by `App` (see
//! `application/state/tools.rs`); these collect the choices.

use eframe::egui::{self, Color32, ComboBox, Context, Key, RichText, TextEdit};
use cellar::application::{valid_name, CellRange, FormatBlock, PasteMode, SortKey};
use cellar::domain::{CellStyle, ConditionalFormat, Spreadsheet, TerminalColor};

use crate::app::{Edit, EditMove, GuiApp};
use crate::grid::rgb;

/// Tracer arrows for the cell at `cell` (Formulas → Trace …).
pub struct Trace {
    pub sheet: usize,
    pub cell: (usize, usize),
    pub precedents: Vec<CellRange>,
    pub dependents: Vec<(usize, usize)>,
}

/// An entry that breaks its column's validation rule, waiting for the
/// user to retry, enter it anyway or give up.
pub struct InvalidEntry {
    edit: Edit,
    rule: String,
    mv: EditMove,
}

impl InvalidEntry {
    pub fn new(edit: Edit, rule: String, mv: EditMove) -> Self {
        Self { edit, rule, mv }
    }
}

pub struct FindDialog {
    query: String,
    replace: String,
    show_replace: bool,
    focus: bool,
    /// What the results on screen were searched for: query, match case,
    /// regex, sheet.
    searched: Option<(String, bool, bool, usize)>,
}

#[derive(Clone, Copy, PartialEq)]
enum FindStep {
    Next,
    Previous,
    Replace,
    ReplaceAll,
}

/// How a rule compares the cell (conditional formatting) or the entry
/// (data validation).
#[derive(Clone, Copy, PartialEq)]
enum Compare {
    Greater,
    Less,
    AtLeast,
    AtMost,
    Equal,
    NotEqual,
    Between,
}

impl Compare {
    const ALL: [Compare; 7] =
        [Compare::Greater, Compare::Less, Compare::AtLeast, Compare::AtMost, Compare::Equal, Compare::NotEqual, Compare::Between];

    fn label(self) -> &'static str {
        match self {
            Compare::Greater => "greater than",
            Compare::Less => "less than",
            Compare::AtLeast => "at least",
            Compare::AtMost => "at most",
            Compare::Equal => "equal to",
            Compare::NotEqual => "not equal to",
            Compare::Between => "between",
        }
    }

    /// The comparison of `subject` as formula text.
    fn formula(self, subject: &str, a: &str, b: &str) -> String {
        match self {
            Compare::Greater => format!("{} > {}", subject, a),
            Compare::Less => format!("{} < {}", subject, a),
            Compare::AtLeast => format!("{} >= {}", subject, a),
            Compare::AtMost => format!("{} <= {}", subject, a),
            Compare::Equal => format!("{} = {}", subject, a),
            Compare::NotEqual => format!("{} <> {}", subject, a),
            Compare::Between => format!("AND({} >= {}, {} <= {})", subject, a, subject, b),
        }
    }
}

/// A number or text typed into a rule, as formula text.
fn literal(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        Err("Enter a value to compare with".to_string())
    } else if text.parse::<f64>().is_ok() {
        Ok(text.to_string())
    } else {
        Ok(format!("\"{}\"", text.replace('"', "\"\"")))
    }
}

/// What a rule compares with: a number, a cell (`C3`, `Limits!B2`), a
/// function call (`GETPIVOTDATA(…)`, `AVERAGE(B:B)`), anything after `=`
/// (names, calculations), otherwise text. Quotes force text: `"Q1"`.
fn operand(text: &str) -> Result<String, String> {
    let text = text.trim();
    if let Some(formula) = text.strip_prefix('=') {
        return if formula.trim().is_empty() { literal("") } else { Ok(formula.trim().to_string()) };
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return literal(&text[1..text.len() - 1].replace("\"\"", "\""));
    }
    let is_cell = Spreadsheet::parse_qualified_reference(text).is_some();
    let is_call = text.ends_with(')')
        && text.split_once('(').is_some_and(|(name, _)| cellar::domain::builtin_function_names().contains(&name.trim().to_uppercase().as_str()));
    if is_cell || is_call { Ok(text.to_string()) } else { literal(text) }
}

/// The field of a rule form a clicked cell goes into.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    A,
    B,
}

fn number(text: &str) -> Result<String, String> {
    let text = text.trim();
    text.parse::<f64>().map(|_| text.to_string()).map_err(|_| format!("\"{}\" is not a number", text))
}

/// `C3` as "column C of the row being checked": `INDIRECT("C" & ROW())`.
fn same_row_reference(cell: &str) -> Option<String> {
    let (sheet, _, col, _, _) = Spreadsheet::parse_qualified_reference(cell)?;
    let sheet = sheet.map(|name| format!("{}!", cellar::domain::format_sheet_name(&name))).unwrap_or_default();
    Some(format!("INDIRECT(\"{}{}\" & ROW())", sheet, Spreadsheet::column_label(col)))
}

/// An ordering comparison of the cell with `a` (and `b`), only between
/// values of one kind: Excel counts any text as greater than any number,
/// which would format a column's title along with its numbers. Against a
/// cell or formula only numbers are compared, so two titles in one row
/// don't count either; text can be ordered with a formula rule.
fn ordered(compare: Compare, a: &str, b: &str) -> String {
    let kind = if a.parse::<f64>().is_ok() {
        "ISNUMBER(_)".to_string()
    } else if a.starts_with('"') {
        "NOT(ISNUMBER(_))".to_string()
    } else {
        format!("ISNUMBER(_), ISNUMBER({})", a)
    };
    format!("AND({}, {})", kind, compare.formula("_", a, b))
}

/// A limit of a validation rule: a number, or a cell or formula giving one.
fn limit(text: &str) -> Result<String, String> {
    match operand(text)? {
        value if value.starts_with('"') => Err(format!("\"{}\" is not a number or a cell", text.trim())),
        value => Ok(value),
    }
}

/// The range a validation list is read from, when the entry is one
/// (`F1:F9`, `Lists!A2:A20`) or follows `=` (a name).
fn list_source(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(source) = text.strip_prefix('=') {
        return Some(source.trim().to_string());
    }
    let (a, b) = text.split_once(':')?;
    let is_cell = |t: &str| Spreadsheet::parse_qualified_reference(t.trim()).is_some();
    (is_cell(a) && is_cell(b)).then(|| text.to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum CfKind {
    Value(Compare),
    Contains,
    Blank,
    NotBlank,
    AboveAverage,
    BelowAverage,
    Duplicate,
    Unique,
    Top,
    Bottom,
    Custom,
}

impl CfKind {
    fn label(self) -> String {
        match self {
            CfKind::Value(c) => format!("Value is {}", c.label()),
            CfKind::Contains => "Text contains".to_string(),
            CfKind::Blank => "Cell is blank".to_string(),
            CfKind::NotBlank => "Cell is not blank".to_string(),
            CfKind::AboveAverage => "Above the column's average".to_string(),
            CfKind::BelowAverage => "Below the column's average".to_string(),
            CfKind::Duplicate => "Duplicate values".to_string(),
            CfKind::Unique => "Unique values".to_string(),
            CfKind::Top => "Top values".to_string(),
            CfKind::Bottom => "Bottom values".to_string(),
            CfKind::Custom => "Formula (_ is the cell)".to_string(),
        }
    }

    fn all() -> Vec<CfKind> {
        let mut kinds: Vec<CfKind> = Compare::ALL.into_iter().map(CfKind::Value).collect();
        kinds.extend([
            CfKind::Contains,
            CfKind::Blank,
            CfKind::NotBlank,
            CfKind::AboveAverage,
            CfKind::BelowAverage,
            CfKind::Duplicate,
            CfKind::Unique,
            CfKind::Top,
            CfKind::Bottom,
            CfKind::Custom,
        ]);
        kinds
    }

    /// The rule as a predicate for `column`. `same_row` compares with the
    /// named cell's column in each cell's own row; `rows` is how far down
    /// the rules that look at the whole column read.
    fn predicate(self, a: &str, b: &str, same_row: bool, column: usize, rows: usize) -> Result<String, String> {
        let side = |text: &str| -> Result<String, String> {
            let value = operand(text)?;
            if !same_row {
                return Ok(value);
            }
            same_row_reference(&value).ok_or_else(|| format!("\"{}\" is not a cell; the same row needs one, e.g. C3", text.trim()))
        };
        let whole_column = format!("{c}1:{c}{rows}", c = Spreadsheet::column_label(column));
        let count = |text: &str| match text.trim().parse::<usize>() {
            Ok(n) if n > 0 => Ok(n),
            _ => Err("Enter how many values, e.g. 10".to_string()),
        };
        Ok(match self {
            CfKind::Value(c @ (Compare::Equal | Compare::NotEqual)) => c.formula("_", &side(a)?, ""),
            CfKind::Value(Compare::Between) => ordered(Compare::Between, &side(a)?, &side(b)?),
            CfKind::Value(c) => ordered(c, &side(a)?, ""),
            CfKind::Contains if a.is_empty() => return Err("Enter the text to look for".to_string()),
            CfKind::Contains => format!("ISNUMBER(SEARCH(\"{}\", _))", a.replace('"', "\"\"")),
            CfKind::Blank => "_ = \"\"".to_string(),
            CfKind::NotBlank => "_ <> \"\"".to_string(),
            CfKind::AboveAverage => format!("AND(ISNUMBER(_), _ > AVERAGE({}))", whole_column),
            CfKind::BelowAverage => format!("AND(ISNUMBER(_), _ < AVERAGE({}))", whole_column),
            CfKind::Duplicate => format!("AND(_ <> \"\", COUNTIF({}, _) > 1)", whole_column),
            CfKind::Unique => format!("AND(_ <> \"\", COUNTIF({}, _) = 1)", whole_column),
            CfKind::Top => format!("AND(ISNUMBER(_), _ >= LARGE({}, {}))", whole_column, count(a)?),
            CfKind::Bottom => format!("AND(ISNUMBER(_), _ <= SMALL({}, {}))", whole_column, count(a)?),
            CfKind::Custom if a.trim().is_empty() => return Err("Enter a formula, e.g. _ > AVERAGE(B1:B100)".to_string()),
            CfKind::Custom => a.trim().trim_start_matches('=').to_string(),
        })
    }
}

pub struct ConditionalDialog {
    /// The sheet the rules belong to; other sheets can be shown meanwhile
    /// to click cells on them.
    sheet: usize,
    /// Where the next clicked cell or PivotTable value goes.
    field: Field,
    rules: Vec<ConditionalFormat>,
    columns: String,
    kind: CfKind,
    a: String,
    b: String,
    /// Compare each cell with the named cell's column in its own row.
    same_row: bool,
    style: CellStyle,
    /// Index of the rule the form is editing, or a new rule.
    editing: Option<usize>,
    error: Option<String>,
}

pub struct TableDialog {
    sheet: usize,
    /// The table being changed, or a new one.
    existing: Option<String>,
    name: String,
    range: String,
    error: Option<String>,
}

pub struct FilterDialog {
    range: CellRange,
    col: usize,
    has_header: bool,
    /// How the typed text is compared; `None` is "contains".
    compare: Option<Compare>,
    contains: String,
    search: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Allow {
    Any,
    Whole,
    Decimal,
    List,
    TextLength,
    Custom,
}

impl Allow {
    const ALL: [Allow; 6] = [Allow::Any, Allow::Whole, Allow::Decimal, Allow::List, Allow::TextLength, Allow::Custom];

    fn label(self) -> &'static str {
        match self {
            Allow::Any => "Any value",
            Allow::Whole => "Whole number",
            Allow::Decimal => "Decimal number",
            Allow::List => "One of a list",
            Allow::TextLength => "Text length",
            Allow::Custom => "Formula (_ is the entry)",
        }
    }
}

pub struct ValidationDialog {
    sheet: usize,
    col: usize,
    allow: Allow,
    compare: Compare,
    a: String,
    b: String,
    /// Where the next clicked cell goes.
    field: Field,
    error: Option<String>,
}

impl ValidationDialog {
    fn predicate(&self) -> Result<Option<String>, String> {
        let compare = |subject: &str| -> Result<String, String> {
            let b = if self.compare == Compare::Between { limit(&self.b)? } else { String::new() };
            Ok(self.compare.formula(subject, &limit(&self.a)?, &b))
        };
        Ok(Some(match self.allow {
            Allow::Any => return Ok(None),
            Allow::Whole => format!("AND(ISNUMBER(_), _ = INT(_), {})", compare("_")?),
            Allow::Decimal => format!("AND(ISNUMBER(_), {})", compare("_")?),
            Allow::TextLength => compare("LEN(_)")?,
            Allow::List => {
                if let Some(source) = list_source(&self.a) {
                    return Ok(Some(format!("COUNTIF({}, _) > 0", source)));
                }
                let items: Vec<String> =
                    self.a.split(',').map(str::trim).filter(|i| !i.is_empty()).map(|i| literal(i).map(|o| format!("_ = {}", o))).collect::<Result<_, _>>()?;
                match items.as_slice() {
                    [] => return Err("Enter the allowed values, separated by commas, or a range".to_string()),
                    [one] => one.clone(),
                    _ => format!("OR({})", items.join(", ")),
                }
            }
            Allow::Custom if self.a.trim().is_empty() => return Err("Enter a formula, e.g. _ <= TODAY()".to_string()),
            Allow::Custom => self.a.trim().trim_start_matches('=').to_string(),
        }))
    }
}

#[derive(Default)]
pub struct NamesDialog {
    name: String,
    value: String,
    error: Option<String>,
}

pub struct SortDialog {
    range: CellRange,
    has_header: bool,
    keys: Vec<SortKey>,
}

pub struct DuplicatesDialog {
    range: CellRange,
    has_header: bool,
    cols: Vec<(usize, bool)>,
}

pub struct SplitDialog {
    range: CellRange,
    delimiter: String,
    other: String,
}

pub struct GoalDialog {
    sheet: usize,
    target: String,
    goal: String,
    input: String,
    /// Where the next clicked cell goes: the changing cell (B) or the
    /// formula cell (A).
    field: Field,
    error: Option<String>,
}

#[derive(Default)]
pub struct Tools {
    pub find: Option<FindDialog>,
    /// View → Show Formulas.
    pub show_formulas: bool,
    /// Formatting picked up by the Format Painter, for the next cells
    /// clicked.
    pub painter: Option<FormatBlock>,
    pub trace: Option<Trace>,
    pub invalid: Option<InvalidEntry>,
    goto: Option<String>,
    comment: Option<String>,
    paste: Option<(PasteMode, bool)>,
    link: Option<(String, String)>,
    conditional: Option<ConditionalDialog>,
    table: Option<TableDialog>,
    filter: Option<FilterDialog>,
    validation: Option<ValidationDialog>,
    names: Option<NamesDialog>,
    sort: Option<SortDialog>,
    duplicates: Option<DuplicatesDialog>,
    split: Option<SplitDialog>,
    goal: Option<GoalDialog>,
}

impl Tools {
    /// True while a dialog that takes keyboard input is open, so grid
    /// shortcuts don't fire underneath it. Find stays open beside the grid.
    pub fn any_open(&self) -> bool {
        self.invalid.is_some()
            || self.goto.is_some()
            || self.comment.is_some()
            || self.paste.is_some()
            || self.link.is_some()
            || self.conditional.is_some()
            || self.table.is_some()
            || self.filter.is_some()
            || self.validation.is_some()
            || self.names.is_some()
            || self.sort.is_some()
            || self.duplicates.is_some()
            || self.split.is_some()
            || self.goal.is_some()
    }
}

/// Which references a dialog taking clicked cells wants.
pub enum PickScope {
    /// Cells of this sheet; ones clicked elsewhere name their sheet.
    Sheet(usize),
    /// Always with the sheet's name (named ranges).
    Workbook,
}

impl Tools {
    /// The dialog that takes the cells clicked or dragged over in the
    /// grid as a reference, like Excel's range boxes, if one is open.
    pub fn picking(&self) -> Option<PickScope> {
        if let Some(sheet) = self.picking_sheet() {
            Some(PickScope::Sheet(sheet))
        } else if let Some(dlg) = &self.goal {
            Some(PickScope::Sheet(dlg.sheet))
        } else if let Some(dlg) = &self.table {
            Some(PickScope::Sheet(dlg.sheet))
        } else {
            self.names.as_ref().map(|_| PickScope::Workbook)
        }
    }

    /// The sheet of an open rule dialog (conditional format, validation),
    /// whose fields also take a clicked PivotTable value.
    pub fn picking_sheet(&self) -> Option<usize> {
        match (&self.conditional, &self.validation) {
            (Some(dlg), _) => Some(dlg.sheet),
            (None, Some(dlg)) if dlg.allow != Allow::Any => Some(dlg.sheet),
            _ => None,
        }
    }

    /// Put a clicked reference into the field of the dialog that asked.
    pub fn pick_reference(&mut self, reference: &str) {
        // Fields for one cell take the first cell of a dragged range.
        let cell = reference.split_once(':').map_or(reference, |(first, _)| first);
        if let Some(dlg) = &mut self.conditional {
            match (dlg.kind, dlg.field) {
                // A formula is being written: add to it.
                (CfKind::Custom, _) => dlg.a.push_str(reference),
                (CfKind::Value(Compare::Between), Field::B) => dlg.b = format!("={}", cell),
                _ => dlg.a = format!("={}", cell),
            }
            dlg.error = None;
        } else if let Some(dlg) = &mut self.validation {
            match (dlg.allow, dlg.field) {
                (Allow::Custom, _) => dlg.a.push_str(reference),
                (Allow::List, _) => dlg.a = format!("={}", reference),
                (_, Field::B) if dlg.compare == Compare::Between => dlg.b = format!("={}", cell),
                _ => dlg.a = format!("={}", cell),
            }
            dlg.error = None;
        } else if let Some(dlg) = &mut self.goal {
            match dlg.field {
                Field::A => dlg.target = cell.to_string(),
                Field::B => dlg.input = cell.to_string(),
            }
            dlg.error = None;
        } else if let Some(dlg) = &mut self.table {
            dlg.range = reference.to_string();
            dlg.error = None;
        } else if let Some(dlg) = &mut self.names {
            dlg.value = reference.to_string();
        }
    }
}

fn cell_label((row, col): (usize, usize)) -> String {
    format!("{}{}", Spreadsheet::column_label(col), row + 1)
}

fn range_label((a, b): CellRange) -> String {
    if a == b { cell_label(a) } else { format!("{}:{}", cell_label(a), cell_label(b)) }
}

/// OK / Cancel row; Enter and Esc work too. `Some(true)` is OK.
fn ok_cancel(ui: &mut egui::Ui, ok: &str) -> Option<bool> {
    let mut result = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(ok).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            result = Some(true);
        }
        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
            result = Some(false);
        }
    });
    result
}

fn error_label(ui: &mut egui::Ui, error: &Option<String>) {
    if let Some(e) = error {
        ui.colored_label(Color32::from_rgb(210, 60, 60), e);
    }
}

fn color_combo(ui: &mut egui::Ui, id: &str, color: &mut Option<TerminalColor>) {
    let name = |c: &Option<TerminalColor>| c.as_ref().map_or("None".to_string(), |c| format!("{:?}", c));
    ComboBox::from_id_salt(id).selected_text(name(color)).width(110.0).show_ui(ui, |ui| {
        ui.selectable_value(color, None, "None");
        for c in TerminalColor::ALL {
            let label = RichText::new(format!("■  {:?}", c)).color(rgb(&c));
            ui.selectable_value(color, Some(c), label);
        }
    });
}

/// Bold / underline / colours of a conditional format.
fn style_editor(ui: &mut egui::Ui, style: &mut CellStyle) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut style.bold, "Bold");
        ui.checkbox(&mut style.underline, "Underline");
        ui.label("Text");
        color_combo(ui, "cf_text", &mut style.fg_color);
        ui.label("Fill");
        color_combo(ui, "cf_fill", &mut style.bg_color);
    });
}

/// A sample of text in `style`.
fn style_sample(ui: &mut egui::Ui, style: &CellStyle) {
    let mut text = RichText::new("AaBb 123");
    if style.bold {
        text = text.strong();
    }
    if style.underline {
        text = text.underline();
    }
    text = match (&style.fg_color, &style.bg_color) {
        (Some(fg), _) => text.color(rgb(fg)),
        (None, Some(_)) => text.color(Color32::BLACK),
        (None, None) => text,
    };
    if let Some(bg) = &style.bg_color {
        text = text.background_color(rgb(bg));
    }
    ui.label(text);
}

fn parse_columns(text: &str) -> Option<std::ops::RangeInclusive<usize>> {
    let text = text.trim().to_uppercase();
    let (a, b) = text.split_once(':').unwrap_or((&text, &text));
    let (a, b) = (Spreadsheet::parse_column_label(a.trim())?, Spreadsheet::parse_column_label(b.trim())?);
    Some(a.min(b)..=a.max(b))
}

impl GuiApp {
    pub fn tool_dialogs(&mut self, ctx: &Context) {
        self.find_dialog(ctx);
        self.goto_dialog(ctx);
        self.comment_dialog(ctx);
        self.paste_special_dialog(ctx);
        self.link_dialog(ctx);
        self.conditional_dialog(ctx);
        self.table_dialog(ctx);
        self.filter_dialog(ctx);
        self.validation_dialog(ctx);
        self.names_dialog(ctx);
        self.sort_dialog(ctx);
        self.duplicates_dialog(ctx);
        self.split_dialog(ctx);
        self.goal_dialog(ctx);
        self.invalid_entry_dialog(ctx);
    }

    /// Title of column `col` in dialogs: its header text, or "Column B".
    fn column_title(&self, range: CellRange, has_header: bool, col: usize) -> String {
        let header = self.app.workbook.current_sheet().get_cell(range.0.0, col).value;
        if has_header && !header.trim().is_empty() {
            format!("{} ({})", header.trim(), Spreadsheet::column_label(col))
        } else {
            format!("Column {}", Spreadsheet::column_label(col))
        }
    }

    /// A range typed into a dialog, on the sheet shown.
    fn parse_range(&self, text: &str) -> Result<CellRange, String> {
        match self.app.resolve_reference(text) {
            Some((sheet, range)) if sheet == self.app.workbook.active_sheet => Ok(range),
            Some(_) => Err("The range must be on this sheet".to_string()),
            None => Err(format!("\"{}\" is not a range", text.trim())),
        }
    }

    // ------------------------------------------------- find and replace

    pub fn open_find(&mut self, replace: bool) {
        self.commit_edit(EditMove::Stay);
        match &mut self.tools.find {
            Some(dlg) => {
                dlg.show_replace |= replace;
                dlg.focus = true;
            }
            None => {
                self.tools.find = Some(FindDialog {
                    query: String::new(),
                    replace: String::new(),
                    show_replace: replace,
                    focus: true,
                    searched: None,
                })
            }
        }
    }

    /// Move the cursor to the next (or previous) match, wrapping around.
    /// `inclusive` lets the cursor's own cell count as the next match.
    fn find_step(&mut self, forward: bool, inclusive: bool) {
        let results = &self.app.find_replace_results;
        let cursor = (self.app.selected_row, self.app.selected_col);
        let target = if forward {
            results.iter().find(|&&p| p > cursor || (inclusive && p == cursor)).or(results.first())
        } else {
            results.iter().rev().find(|&&p| p < cursor).or(results.last())
        };
        if let Some(&(r, c)) = target {
            self.set_cursor(r, c);
            self.grid.scroll_to_cursor = true;
        }
    }

    fn find_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.find.take() else { return };
        let mut open = true;
        let mut close = false;
        let mut step = None;
        let total = self.app.find_replace_results.len();
        let cursor = (self.app.selected_row, self.app.selected_col);
        let at = self.app.find_replace_results.iter().position(|&p| p == cursor);
        egui::Window::new(if dlg.show_replace { "Find and Replace" } else { "Find" })
            .id(egui::Id::new("find_replace"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                egui::Grid::new("find_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Find");
                    let resp = ui.add(TextEdit::singleline(&mut dlg.query).desired_width(240.0));
                    if std::mem::take(&mut dlg.focus) {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        step = Some(if ui.input(|i| i.modifiers.shift) { FindStep::Previous } else { FindStep::Next });
                        resp.request_focus();
                    }
                    ui.end_row();
                    if dlg.show_replace {
                        ui.label("Replace with");
                        ui.add(TextEdit::singleline(&mut dlg.replace).desired_width(240.0));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.app.search_case_sensitive, "Match case");
                    ui.checkbox(&mut self.app.search_regex, "Regular expression");
                });
                ui.horizontal(|ui| {
                    if ui.button("Find Next").clicked() {
                        step = Some(FindStep::Next);
                    }
                    if ui.button("Find Previous").clicked() {
                        step = Some(FindStep::Previous);
                    }
                    if dlg.show_replace {
                        if ui.button("Replace").clicked() {
                            step = Some(FindStep::Replace);
                        }
                        if ui.button("Replace All").clicked() {
                            step = Some(FindStep::ReplaceAll);
                        }
                    } else if ui.button("Replace…").clicked() {
                        dlg.show_replace = true;
                    }
                });
                let status = match (dlg.query.is_empty(), total, at) {
                    (true, _, _) => "Searches values and formulas on this sheet".to_string(),
                    (false, 0, _) => "No matches".to_string(),
                    (false, n, Some(i)) => format!("{} of {} matches", i + 1, n),
                    (false, n, None) => format!("{} matches", n),
                };
                ui.label(RichText::new(status).weak());
                close = ui.input(|i| i.key_pressed(Key::Escape));
            });
        if !open || close {
            self.app.find_replace_results.clear();
            return;
        }

        // Search again when the query or options change, and before every
        // step so edits made meanwhile are seen. The engine's search moves
        // the cursor to the first match; stepping from the cursor is ours.
        let key = (dlg.query.clone(), self.app.search_case_sensitive, self.app.search_regex, self.app.workbook.active_sheet);
        let typed = dlg.searched.as_ref().is_none_or(|s| s.0 != key.0);
        if dlg.searched.as_ref() != Some(&key) || step.is_some() {
            self.app.find_replace_search = dlg.query.clone();
            self.app.find_replace_search();
            (self.app.selected_row, self.app.selected_col) = cursor;
            dlg.searched = Some(key);
            if typed && step.is_none() && !dlg.query.is_empty() {
                self.find_step(true, true);
            }
        }
        match step {
            Some(FindStep::Next) => self.find_step(true, false),
            Some(FindStep::Previous) => self.find_step(false, false),
            Some(FindStep::Replace) => {
                // Like Excel: the first press goes to a match, the next replaces it.
                match self.app.find_replace_results.iter().position(|&p| p == cursor) {
                    Some(i) => {
                        self.app.find_replace_index = i;
                        self.app.find_replace_replace = dlg.replace.clone();
                        self.app.replace_current();
                        self.app.clear_selection();
                        self.grid.scroll_to_cursor = true;
                    }
                    None => self.find_step(true, true),
                }
            }
            Some(FindStep::ReplaceAll) => {
                self.app.find_replace_replace = dlg.replace.clone();
                self.app.replace_all();
                self.app.find_replace_search();
                (self.app.selected_row, self.app.selected_col) = cursor;
            }
            None => {}
        }
        self.tools.find = Some(dlg);
    }

    // ------------------------------------------------------------- go to

    pub fn open_goto(&mut self) {
        self.commit_edit(EditMove::Stay);
        self.tools.goto = Some(String::new());
    }

    fn goto_dialog(&mut self, ctx: &Context) {
        let Some(mut text) = self.tools.goto.take() else { return };
        let mut go: Option<String> = None;
        let mut done = None;
        let mut names = self.app.user_names();
        for (sheet, name) in self.app.workbook.sheets.iter().zip(&self.app.workbook.sheet_names) {
            for t in &sheet.tables {
                let range = ((t.top_row, t.left_col), (t.bottom_row, t.right_col));
                names.push((t.name.clone(), format!("table on {} ({})", name, range_label(range))));
            }
        }
        egui::Modal::new(egui::Id::new("go_to")).show(ctx, |ui| {
            ui.heading("Go To");
            let resp = ui.add(TextEdit::singleline(&mut text).hint_text("B12, A1:C5, Sheet2!A1 or a name").desired_width(300.0));
            resp.request_focus();
            if !names.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("Names and tables").weak());
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for (name, value) in &names {
                        if ui.selectable_label(false, format!("{}   {}", name, value)).clicked() {
                            go = Some(name.clone());
                        }
                    }
                });
            }
            done = ok_cancel(ui, "Go");
        });
        if done == Some(true) && go.is_none() {
            go = Some(text.clone());
        }
        match (go, done) {
            (Some(target), _) if !target.trim().is_empty() => self.goto(&target),
            (None, None) => self.tools.goto = Some(text),
            _ => {}
        }
    }

    // ---------------------------------------------------------- comments

    pub fn open_comment(&mut self) {
        self.commit_edit(EditMove::Stay);
        let cell = self.app.workbook.current_sheet().get_cell(self.app.selected_row, self.app.selected_col);
        self.tools.comment = Some(cell.comment.unwrap_or_default());
    }

    fn comment_dialog(&mut self, ctx: &Context) {
        let Some(mut text) = self.tools.comment.take() else { return };
        let mut save = None;
        egui::Modal::new(egui::Id::new("comment")).show(ctx, |ui| {
            ui.heading(format!("Comment on {}", cell_label((self.app.selected_row, self.app.selected_col))));
            ui.add(TextEdit::multiline(&mut text).desired_rows(4).desired_width(320.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    save = Some(true);
                }
                if ui.button("Delete Comment").clicked() {
                    text.clear();
                    save = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    save = Some(false);
                }
            });
        });
        match save {
            Some(true) => {
                let text = text.trim();
                self.app.set_cell_comment((!text.is_empty()).then(|| text.to_string()));
            }
            Some(false) => {}
            None => self.tools.comment = Some(text),
        }
    }

    // ----------------------------------------------------- paste special

    pub fn open_paste_special(&mut self) {
        self.commit_edit(EditMove::Stay);
        self.tools.paste = Some((PasteMode::All, false));
    }

    fn paste_special_dialog(&mut self, ctx: &Context) {
        let Some((mut mode, mut transpose)) = self.tools.paste else { return };
        let mut done = None;
        egui::Modal::new(egui::Id::new("paste_special")).show(ctx, |ui| {
            ui.heading("Paste Special");
            for m in PasteMode::ALL {
                ui.radio_value(&mut mode, m, m.label());
            }
            ui.separator();
            ui.checkbox(&mut transpose, "Transpose (swap rows and columns)");
            done = ok_cancel(ui, "Paste");
        });
        self.tools.paste = done.is_none().then_some((mode, transpose));
        if done == Some(true) {
            self.app.paste_special(mode, transpose);
            self.grow_to(self.app.selected_row, self.app.selected_col);
        }
    }

    // -------------------------------------------------------------- links

    pub fn open_link(&mut self, url: &str) {
        let known = ["http://", "https://", "mailto:", "file://"].iter().any(|p| url.to_lowercase().starts_with(p));
        // A bare "example.com" is a web address.
        let url = if known { url.to_string() } else { format!("https://{}", url) };
        self.app.status_message = Some(format!("Opening {}", url));
        self.window_ctx.open_url(egui::OpenUrl::new_tab(url));
    }

    pub fn open_link_dialog(&mut self) {
        self.commit_edit(EditMove::Stay);
        let cell = self.app.workbook.current_sheet().get_cell(self.app.selected_row, self.app.selected_col);
        let url = self.hyperlink_url(&cell).unwrap_or_default();
        self.tools.link = Some((url, cell.value));
    }

    fn link_dialog(&mut self, ctx: &Context) {
        let Some((mut url, mut text)) = self.tools.link.take() else { return };
        let mut done = None;
        egui::Modal::new(egui::Id::new("insert_link")).show(ctx, |ui| {
            ui.heading("Insert Link");
            egui::Grid::new("link_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Address");
                ui.add(TextEdit::singleline(&mut url).hint_text("https://…").desired_width(300.0));
                ui.end_row();
                ui.label("Text to display");
                ui.add(TextEdit::singleline(&mut text).hint_text("(the address)").desired_width(300.0));
                ui.end_row();
            });
            ui.label(RichText::new("Entered as a =HYPERLINK(…) formula.").weak().small());
            done = ok_cancel(ui, "OK");
        });
        match done {
            Some(true) if !url.trim().is_empty() => {
                let quote = |s: &str| format!("\"{}\"", s.trim().replace('"', "\"\""));
                let formula = if text.trim().is_empty() {
                    format!("=HYPERLINK({})", quote(&url))
                } else {
                    format!("=HYPERLINK({}, {})", quote(&url), quote(&text))
                };
                self.start_edit(Some(formula), true);
                self.commit_edit(EditMove::Stay);
            }
            Some(_) => {}
            None => self.tools.link = Some((url, text)),
        }
    }

    // ---------------------------------------------- conditional formats

    pub fn open_conditional(&mut self) {
        self.commit_edit(EditMove::Stay);
        let ((_, c0), (_, c1)) = self.selection_or_cursor();
        let columns = if c0 == c1 {
            Spreadsheet::column_label(c0)
        } else {
            format!("{}:{}", Spreadsheet::column_label(c0), Spreadsheet::column_label(c1))
        };
        self.tools.conditional = Some(ConditionalDialog {
            sheet: self.app.workbook.active_sheet,
            field: Field::A,
            rules: self.app.workbook.current_sheet().conditional_formats.clone(),
            columns,
            kind: CfKind::Value(Compare::Greater),
            a: String::new(),
            b: String::new(),
            same_row: false,
            style: CellStyle { bg_color: Some(TerminalColor::LightRed), ..CellStyle::default() },
            editing: None,
            error: None,
        });
    }

    fn conditional_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.conditional.take() else { return };
        let mut open = true;
        let mut done = None;
        // Rules that look at the whole column read this far down.
        let rows = self.app.workbook.sheets.get(dlg.sheet).map_or(1000, |s| s.rows.max(1000));
        egui::Window::new("Conditional Formatting").open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            ui.label(RichText::new("Rules on this sheet").strong());
            if dlg.rules.is_empty() {
                ui.label(RichText::new("No rules yet. Later rules are applied on top of earlier ones.").weak());
            }
            let mut remove = None;
            egui::Grid::new("cf_rules").num_columns(4).striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
                for (i, rule) in dlg.rules.iter().enumerate() {
                    ui.label(format!("Column {}", Spreadsheet::column_label(rule.column)));
                    ui.monospace(&rule.predicate);
                    style_sample(ui, &rule.style);
                    ui.horizontal(|ui| {
                        if ui.small_button("Edit").clicked() {
                            dlg.editing = Some(i);
                            dlg.columns = Spreadsheet::column_label(rule.column);
                            dlg.kind = CfKind::Custom;
                            dlg.same_row = false;
                            dlg.a = rule.predicate.clone();
                            dlg.style = rule.style.clone();
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                dlg.rules.remove(i);
                dlg.editing = None;
            }

            ui.separator();
            ui.label(RichText::new(if dlg.editing.is_some() { "Edit rule" } else { "New rule" }).strong());
            egui::Grid::new("cf_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Columns");
                ui.add(TextEdit::singleline(&mut dlg.columns).hint_text("B or B:D").desired_width(80.0));
                ui.end_row();
                ui.label("Format cells if");
                ui.horizontal(|ui| {
                    ComboBox::from_id_salt("cf_kind").selected_text(dlg.kind.label()).width(190.0).show_ui(ui, |ui| {
                        for kind in CfKind::all() {
                            ui.selectable_value(&mut dlg.kind, kind, kind.label());
                        }
                    });
                    match dlg.kind {
                        CfKind::Blank
                        | CfKind::NotBlank
                        | CfKind::AboveAverage
                        | CfKind::BelowAverage
                        | CfKind::Duplicate
                        | CfKind::Unique => {}
                        CfKind::Top | CfKind::Bottom => {
                            ui.add(TextEdit::singleline(&mut dlg.a).hint_text("10").desired_width(50.0));
                            ui.label("values");
                        }
                        CfKind::Value(Compare::Between) => {
                            if ui.add(TextEdit::singleline(&mut dlg.a).hint_text("1 or C3").desired_width(90.0)).has_focus() {
                                dlg.field = Field::A;
                            }
                            ui.label("and");
                            if ui.add(TextEdit::singleline(&mut dlg.b).hint_text("10 or C4").desired_width(90.0)).has_focus() {
                                dlg.field = Field::B;
                            }
                        }
                        CfKind::Custom => {
                            ui.add(TextEdit::singleline(&mut dlg.a).hint_text("_ > AVERAGE(B:B)").desired_width(220.0));
                        }
                        _ => {
                            ui.add(TextEdit::singleline(&mut dlg.a).hint_text("100, text or C3").desired_width(170.0));
                            dlg.field = Field::A;
                        }
                    }
                });
                ui.end_row();
                if matches!(dlg.kind, CfKind::Value(_)) {
                    ui.label("");
                    ui.checkbox(&mut dlg.same_row, "Compare with that cell's column in each row")
                        .on_hover_text("With C3: every cell is compared with column C of its own row, not always with C3.");
                    ui.end_row();
                }
                // How the entry is read, so a cell isn't mistaken for text.
                ui.label("Rule");
                let first = parse_columns(&dlg.columns).map_or(0, |cols| *cols.start());
                let same_row = dlg.same_row && matches!(dlg.kind, CfKind::Value(_));
                match dlg.kind.predicate(&dlg.a, &dlg.b, same_row, first, rows) {
                    Ok(predicate) => ui.add(egui::Label::new(RichText::new(predicate).monospace()).wrap()),
                    Err(_) => ui.label(RichText::new("…").weak()),
                };
                ui.end_row();
                ui.label("Format");
                style_editor(ui, &mut dlg.style);
                ui.end_row();
                ui.label("Preview");
                style_sample(ui, &dlg.style);
                ui.end_row();
            });
            ui.label(
                RichText::new(
                    "Compare with a number, text, a cell (C3, Limits!B2) or a formula such as GETPIVOTDATA(…). \
                     Click a cell in the grid or a value in a PivotTable to compare with it; \
                     put text that looks like a cell in quotes (\"Q1\"). In the rule, _ is the cell being formatted \
                     and ROW() its row. A rule covers its whole column.",
                )
                .weak()
                .small(),
            );
            error_label(ui, &dlg.error);
            if ui.button(if dlg.editing.is_some() { "Update Rule" } else { "Add Rule" }).clicked() {
                let same_row = dlg.same_row && matches!(dlg.kind, CfKind::Value(_));
                let new: Result<Vec<ConditionalFormat>, String> = parse_columns(&dlg.columns)
                    .ok_or_else(|| format!("\"{}\" is not a column", dlg.columns))
                    .and_then(|cols| {
                        cols.map(|column| {
                            let predicate = dlg.kind.predicate(&dlg.a, &dlg.b, same_row, column, rows)?;
                            Ok(ConditionalFormat { column, predicate, style: dlg.style.clone() })
                        })
                        .collect()
                    });
                match new {
                    Ok(new) => {
                        match dlg.editing.take() {
                            Some(i) if i < dlg.rules.len() => {
                                dlg.rules.splice(i..=i, new);
                            }
                            _ => dlg.rules.extend(new),
                        }
                        dlg.error = None;
                        dlg.a.clear();
                        dlg.b.clear();
                    }
                    Err(e) => dlg.error = Some(e),
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                // Back to the rules' sheet, in case another was shown to
                // click cells on it.
                self.show_sheet(dlg.sheet);
                self.app.set_conditional_formats(dlg.rules);
            }
            None if open => self.tools.conditional = Some(dlg),
            _ => {}
        }
    }

    /// While a dialog is asking for a reference, the cells clicked or
    /// dragged over in the grid are the answer. Returns whether they were
    /// used.
    pub fn pick_for_dialog(&mut self, range: CellRange) -> bool {
        let Some(scope) = self.tools.picking() else { return false };
        let wb = &self.app.workbook;
        let same_sheet = matches!(scope, PickScope::Sheet(owner) if owner == wb.active_sheet);
        let reference = if same_sheet {
            range_label(range)
        } else {
            format!("{}!{}", cellar::domain::format_sheet_name(&wb.sheet_names[wb.active_sheet]), range_label(range))
        };
        self.tools.pick_reference(&reference);
        true
    }

    // ------------------------------------------------------------ tables

    pub fn open_table(&mut self) {
        self.commit_edit(EditMove::Stay);
        let existing = self.app.workbook.current_sheet().table_at(self.app.selected_row, self.app.selected_col);
        self.tools.table = Some(match existing {
            Some(t) => TableDialog {
                sheet: self.app.workbook.active_sheet,
                existing: Some(t.name.clone()),
                name: t.name.clone(),
                range: range_label(((t.top_row, t.left_col), (t.bottom_row, t.right_col))),
                error: None,
            },
            None => TableDialog {
                sheet: self.app.workbook.active_sheet,
                existing: None,
                name: self.app.next_table_name(),
                range: range_label(self.app.data_range()),
                error: None,
            },
        });
    }

    fn table_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.table.take() else { return };
        let mut done = None;
        let mut remove = false;
        let mut open = true;
        let title = if dlg.existing.is_some() { "Table" } else { "Format as Table" };
        egui::Window::new(title).id(egui::Id::new("format_as_table")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("table_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(TextEdit::singleline(&mut dlg.name).desired_width(200.0));
                ui.end_row();
                ui.label("Cells");
                ui.add(TextEdit::singleline(&mut dlg.range).hint_text("A1:D20").desired_width(200.0));
                ui.end_row();
            });
            ui.label(
                RichText::new(
                    "Drag over the cells in the grid to choose them. The first row holds the column titles. \
                     Formulas can read a column as Name[Title] and all the rows as Name; \
                     the table grows when you type in the row below it.",
                )
                .weak()
                .small(),
            );
            error_label(ui, &dlg.error);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                }
                if dlg.existing.is_some() && ui.button("Convert to Range").on_hover_text("Remove the table; its cells stay").clicked() {
                    remove = true;
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        if !open {
            return;
        }
        if remove || done == Some(true) {
            self.show_sheet(dlg.sheet);
        }
        if remove {
            self.app.remove_table(dlg.existing.as_deref().unwrap_or_default());
            return;
        }
        match done {
            Some(true) => {
                let name = dlg.name.trim().to_string();
                let result = self.parse_range(&dlg.range).and_then(|range| match &dlg.existing {
                    Some(old) => self.app.update_table(old, &name, range),
                    None => self.app.add_table(range, &name),
                });
                if let Err(e) = result {
                    dlg.error = Some(e);
                    self.tools.table = Some(dlg);
                }
            }
            Some(false) => {}
            None => self.tools.table = Some(dlg),
        }
    }

    // ------------------------------------------------------------ filter

    /// The rows a filter at the cursor covers: its table, or the data
    /// around it.
    fn filter_range(&self) -> CellRange {
        let (row, col) = (self.app.selected_row, self.app.selected_col);
        match self.app.workbook.current_sheet().table_at(row, col) {
            Some(t) => ((t.top_row, t.left_col), (t.bottom_row, t.right_col)),
            None => self.app.data_range(),
        }
    }

    pub fn open_filter(&mut self) {
        self.commit_edit(EditMove::Stay);
        let range = self.filter_range();
        self.tools.filter = Some(FilterDialog {
            range,
            col: self.app.selected_col.clamp(range.0.1, range.1.1),
            has_header: self.app.guess_header(range),
            compare: None,
            contains: String::new(),
            search: String::new(),
        });
    }

    fn apply_filter(&mut self, range: CellRange, has_header: bool, col: usize, criteria: &str) {
        let first = range.0.0 + usize::from(has_header);
        self.app.apply_filter_rows(col, criteria, first, range.1.0);
        // Keep the cursor on a row that is still shown.
        if self.app.hidden_rows.contains(&self.app.selected_row) {
            let shown = (range.0.0..=range.1.0).find(|r| !self.app.hidden_rows.contains(r)).unwrap_or(range.0.0);
            self.set_cursor(shown, col);
        }
        self.grid.scroll_to_cursor = true;
    }

    /// Show only the rows with the cursor cell's value in its column.
    pub fn filter_by_selection(&mut self) {
        self.commit_edit(EditMove::Stay);
        let range = self.filter_range();
        let col = self.app.selected_col;
        let value = self.app.workbook.current_sheet().get_cell(self.app.selected_row, col).value;
        let has_header = self.app.guess_header(range);
        self.apply_filter(range, has_header, col, &format!("={}", value));
    }

    pub fn clear_filter(&mut self) {
        self.app.clear_filter();
        self.app.dirty = true;
    }

    fn filter_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.filter.take() else { return };
        let mut open = true;
        let mut apply: Option<String> = None;
        let mut clear = false;
        let mut close = false;
        let ((r0, c0), (r1, c1)) = dlg.range;
        let first = r0 + usize::from(dlg.has_header);
        let values = self.app.distinct_values(dlg.col, first, r1);
        egui::Window::new("Filter").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("filter_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Column");
                ComboBox::from_id_salt("filter_col").selected_text(self.column_title(dlg.range, dlg.has_header, dlg.col)).width(220.0).show_ui(
                    ui,
                    |ui| {
                        for c in c0..=c1 {
                            ui.selectable_value(&mut dlg.col, c, self.column_title(dlg.range, dlg.has_header, c));
                        }
                    },
                );
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut dlg.has_header, "My data has headers");
                ui.end_row();
                ui.label("Show rows that");
                ui.horizontal(|ui| {
                    let label = |c: Option<Compare>| match c {
                        None => "contain".to_string(),
                        Some(Compare::Equal) => "equal".to_string(),
                        Some(Compare::NotEqual) => "do not equal".to_string(),
                        Some(c) => format!("are {}", c.label()),
                    };
                    ComboBox::from_id_salt("filter_compare").selected_text(label(dlg.compare)).width(130.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut dlg.compare, None, label(None));
                        for c in Compare::ALL.into_iter().filter(|c| *c != Compare::Between) {
                            ui.selectable_value(&mut dlg.compare, Some(c), label(Some(c)));
                        }
                    });
                    let resp = ui.add(TextEdit::singleline(&mut dlg.contains).desired_width(110.0));
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if (ui.button("Apply").clicked() || enter) && !dlg.contains.is_empty() {
                        // The filter's criteria reads like SUMIF's: ">100".
                        let text = dlg.contains.trim();
                        apply = Some(match dlg.compare {
                            None => text.trim_start_matches(['=', '<', '>']).to_string(),
                            Some(Compare::Greater) => format!(">{}", text),
                            Some(Compare::Less) => format!("<{}", text),
                            Some(Compare::AtLeast) => format!(">={}", text),
                            Some(Compare::AtMost) => format!("<={}", text),
                            Some(Compare::NotEqual) => format!("<>{}", text),
                            Some(Compare::Equal | Compare::Between) => format!("={}", text),
                        });
                    }
                });
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Or show only one value ({} in rows {}-{}):", values.len(), first + 1, r1 + 1)).weak());
            ui.add(TextEdit::singleline(&mut dlg.search).hint_text("Search values").desired_width(f32::INFINITY));
            let needle = dlg.search.to_lowercase();
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                for value in values.iter().filter(|v| v.to_lowercase().contains(&needle)).take(500) {
                    let label = if value.is_empty() { "(Blanks)" } else { value.as_str() };
                    if ui.selectable_label(false, label).clicked() {
                        apply = Some(format!("={}", value));
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Clear Filter").clicked() {
                    clear = true;
                }
                close = ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
            });
        });
        if clear {
            self.clear_filter();
        } else if let Some(criteria) = apply {
            self.apply_filter(dlg.range, dlg.has_header, dlg.col, &criteria);
        } else if open && !close {
            self.tools.filter = Some(dlg);
        }
    }

    // --------------------------------------------------- data validation

    pub fn open_validation(&mut self) {
        self.commit_edit(EditMove::Stay);
        let col = self.app.selected_col;
        let current = self.app.validations.get(&col).cloned();
        self.tools.validation = Some(ValidationDialog {
            sheet: self.app.workbook.active_sheet,
            col,
            allow: if current.is_some() { Allow::Custom } else { Allow::Whole },
            compare: Compare::Between,
            a: current.unwrap_or_default(),
            b: String::new(),
            field: Field::A,
            error: None,
        });
    }

    fn validation_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.validation.take() else { return };
        let mut done = None;
        let mut open = true;
        let label = Spreadsheet::column_label(dlg.col);
        // The rules shown and set are those of the dialog's sheet; another
        // sheet may be on screen to click cells there.
        let on_its_sheet = dlg.sheet == self.app.workbook.active_sheet;
        let current = if on_its_sheet {
            self.app.validations.get(&dlg.col).cloned()
        } else {
            self.app.workbook.sheets.get(dlg.sheet).and_then(|s| s.view_state.validations.get(&dlg.col).cloned())
        };
        egui::Window::new(format!("Data Validation: column {}", label))
            .id(egui::Id::new("data_validation"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                match &current {
                    Some(rule) => ui.add(egui::Label::new(format!("Current rule: {}", rule)).wrap()),
                    None => ui.label(RichText::new("No rule: anything can be entered.").weak()),
                };
                ui.add_space(4.0);
                egui::Grid::new("validation_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Allow");
                    ComboBox::from_id_salt("validation_allow").selected_text(dlg.allow.label()).width(200.0).show_ui(ui, |ui| {
                        for allow in Allow::ALL {
                            if ui.selectable_value(&mut dlg.allow, allow, allow.label()).clicked() {
                                dlg.a.clear();
                                dlg.b.clear();
                            }
                        }
                    });
                    ui.end_row();
                    match dlg.allow {
                        Allow::Any => {}
                        Allow::Whole | Allow::Decimal | Allow::TextLength => {
                            ui.label(if dlg.allow == Allow::TextLength { "Length is" } else { "Value is" });
                            ui.horizontal(|ui| {
                                ComboBox::from_id_salt("validation_compare").selected_text(dlg.compare.label()).width(110.0).show_ui(ui, |ui| {
                                    for c in Compare::ALL {
                                        ui.selectable_value(&mut dlg.compare, c, c.label());
                                    }
                                });
                                if ui.add(TextEdit::singleline(&mut dlg.a).hint_text("1 or F1").desired_width(80.0)).has_focus() {
                                    dlg.field = Field::A;
                                }
                                if dlg.compare == Compare::Between {
                                    ui.label("and");
                                    if ui.add(TextEdit::singleline(&mut dlg.b).hint_text("10 or F2").desired_width(80.0)).has_focus() {
                                        dlg.field = Field::B;
                                    }
                                } else {
                                    dlg.field = Field::A;
                                }
                            });
                            ui.end_row();
                        }
                        Allow::List => {
                            ui.label("Values");
                            ui.add(TextEdit::singleline(&mut dlg.a).hint_text("Open, Closed, On hold  or  F1:F9").desired_width(260.0));
                            ui.end_row();
                        }
                        Allow::Custom => {
                            ui.label("Formula");
                            ui.add(TextEdit::singleline(&mut dlg.a).hint_text("_ <= TODAY()").desired_width(260.0));
                            ui.end_row();
                        }
                    }
                    if dlg.allow != Allow::Any {
                        ui.label("Rule");
                        match dlg.predicate() {
                            Ok(Some(rule)) => ui.add(egui::Label::new(RichText::new(rule).monospace()).wrap()),
                            _ => ui.label(RichText::new("…").weak()),
                        };
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new(
                        "Limits can be numbers or cells, and a list can be typed or read from a range: click or \
                         drag in the grid to fill them in. A list gives the column's cells a dropdown. \
                         Checked when a value is typed into the column; blank cells and formulas are always \
                         accepted, and an entry that fails can still be entered on purpose.",
                    )
                    .weak()
                    .small(),
                );
                error_label(ui, &dlg.error);
                done = ok_cancel(ui, "OK");
            });
        match done {
            _ if !open => {}
            Some(true) => match dlg.predicate() {
                Ok(rule) => {
                    self.show_sheet(dlg.sheet);
                    self.app.set_validation(dlg.col, rule.clone());
                    self.app.status_message = Some(match rule {
                        Some(_) => format!("Validation set for column {}", label),
                        None => format!("Validation removed from column {}", label),
                    });
                }
                Err(e) => {
                    dlg.error = Some(e);
                    self.tools.validation = Some(dlg);
                }
            },
            Some(false) => {}
            None => self.tools.validation = Some(dlg),
        }
    }

    /// Ask what to do with an entry that breaks its validation rule.
    fn invalid_entry_dialog(&mut self, ctx: &Context) {
        let Some(invalid) = self.tools.invalid.take() else { return };
        #[derive(PartialEq)]
        enum Choice {
            Retry,
            Enter,
            Cancel,
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::new("invalid_entry")).show(ctx, |ui| {
            ui.heading("This value doesn't match the validation rule");
            ui.label(format!(
                "\"{}\" in {} breaks the rule for column {}:",
                invalid.edit.text,
                cell_label((invalid.edit.row, invalid.edit.col)),
                Spreadsheet::column_label(invalid.edit.col)
            ));
            ui.monospace(&invalid.rule);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Retry").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    choice = Some(Choice::Retry);
                }
                if ui.button("Enter Anyway").clicked() {
                    choice = Some(Choice::Enter);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        match choice {
            Some(Choice::Retry) => {
                let mut edit = invalid.edit;
                edit.focus_pending = true;
                self.edit = Some(edit);
            }
            Some(Choice::Enter) => {
                self.edit = Some(invalid.edit);
                self.commit_edit_unchecked(invalid.mv);
            }
            Some(Choice::Cancel) => {}
            None => self.tools.invalid = Some(invalid),
        }
    }

    // ------------------------------------------------------------- names

    pub fn open_names(&mut self) {
        self.commit_edit(EditMove::Stay);
        self.tools.names = Some(NamesDialog { value: self.app.selection_reference(), ..NamesDialog::default() });
    }

    fn names_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.names.take() else { return };
        let mut open = true;
        let mut delete = None;
        let mut go = None;
        let names = self.app.user_names();
        egui::Window::new("Name Manager").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
            if names.is_empty() {
                ui.label(RichText::new("No names yet. A name stands for a range or value in formulas: =SUM(Sales).").weak());
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                egui::Grid::new("names").num_columns(3).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (name, value) in &names {
                        ui.strong(name);
                        ui.monospace(value);
                        ui.horizontal(|ui| {
                            if ui.small_button("Go To").clicked() {
                                go = Some(name.clone());
                            }
                            if ui.small_button("Edit").clicked() {
                                dlg.name = name.clone();
                                dlg.value = value.clone();
                            }
                            if ui.small_button("Delete").clicked() {
                                delete = Some(name.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.label(RichText::new("Define a name").strong());
            egui::Grid::new("name_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(TextEdit::singleline(&mut dlg.name).hint_text("TaxRate").desired_width(160.0));
                ui.end_row();
                ui.label("Refers to");
                ui.horizontal(|ui| {
                    ui.add(TextEdit::singleline(&mut dlg.value).hint_text("Sheet1!B2:B20 or 0.19").desired_width(220.0));
                    if ui.small_button("Use Selection").clicked() {
                        dlg.value = self.app.selection_reference();
                    }
                });
                ui.end_row();
            });
            ui.label(RichText::new("Click or drag over cells in the grid to refer to them.").weak().small());
            error_label(ui, &dlg.error);
            if ui.button("Save Name").clicked() {
                dlg.error = self.app.define_name(dlg.name.trim(), &dlg.value).err();
                if dlg.error.is_none() {
                    dlg.name.clear();
                }
            }
        });
        if let Some(name) = delete {
            self.app.delete_name(&name);
        }
        if let Some(name) = go {
            self.goto(&name);
        } else if open {
            self.tools.names = Some(dlg);
        }
    }

    // -------------------------------------------------------------- sort

    pub fn open_sort(&mut self) {
        self.commit_edit(EditMove::Stay);
        let range = self.app.data_range();
        self.tools.sort = Some(SortDialog {
            range,
            has_header: self.app.guess_header(range),
            keys: vec![(self.app.selected_col.clamp(range.0.1, range.1.1), true)],
        });
    }

    fn sort_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.sort.take() else { return };
        let mut done = None;
        let ((_, c0), (_, c1)) = dlg.range;
        egui::Modal::new(egui::Id::new("sort")).show(ctx, |ui| {
            ui.heading(format!("Sort {}", range_label(dlg.range)));
            ui.checkbox(&mut dlg.has_header, "My data has headers");
            ui.add_space(4.0);
            let mut remove = None;
            let levels = dlg.keys.len();
            egui::Grid::new("sort_levels").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                for (i, (col, ascending)) in dlg.keys.iter_mut().enumerate() {
                    ui.label(if i == 0 { "Sort by" } else { "Then by" });
                    ComboBox::from_id_salt(("sort_col", i)).selected_text(self.column_title(dlg.range, dlg.has_header, *col)).width(190.0).show_ui(
                        ui,
                        |ui| {
                            for c in c0..=c1 {
                                ui.selectable_value(col, c, self.column_title(dlg.range, dlg.has_header, c));
                            }
                        },
                    );
                    ComboBox::from_id_salt(("sort_order", i))
                        .selected_text(if *ascending { "A to Z, smallest first" } else { "Z to A, largest first" })
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(ascending, true, "A to Z, smallest first");
                            ui.selectable_value(ascending, false, "Z to A, largest first");
                        });
                    if levels > 1 && ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                dlg.keys.remove(i);
            }
            if dlg.keys.len() < c1 - c0 + 1 && ui.button("Add Level").clicked() {
                let unused = (c0..=c1).find(|c| !dlg.keys.iter().any(|(k, _)| k == c)).unwrap_or(c0);
                dlg.keys.push((unused, true));
            }
            done = ok_cancel(ui, "Sort");
        });
        match done {
            Some(true) => {
                self.app.sort_range(dlg.range, dlg.has_header, &dlg.keys);
            }
            Some(false) => {}
            None => self.tools.sort = Some(dlg),
        }
    }

    // ------------------------------------------------- remove duplicates

    pub fn open_duplicates(&mut self) {
        self.commit_edit(EditMove::Stay);
        let range = self.app.data_range();
        self.tools.duplicates = Some(DuplicatesDialog {
            range,
            has_header: self.app.guess_header(range),
            cols: (range.0.1..=range.1.1).map(|c| (c, true)).collect(),
        });
    }

    fn duplicates_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.duplicates.take() else { return };
        let mut done = None;
        egui::Modal::new(egui::Id::new("remove_duplicates")).show(ctx, |ui| {
            ui.heading(format!("Remove Duplicates in {}", range_label(dlg.range)));
            ui.checkbox(&mut dlg.has_header, "My data has headers");
            ui.add_space(4.0);
            ui.label("Rows are duplicates when these columns match:");
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for (col, checked) in &mut dlg.cols {
                    ui.checkbox(checked, self.column_title(dlg.range, dlg.has_header, *col));
                }
            });
            ui.label(RichText::new("The first of each set of duplicates is kept; rows below move up.").weak().small());
            done = ok_cancel(ui, "Remove Duplicates");
        });
        match done {
            Some(true) => {
                let cols: Vec<usize> = dlg.cols.iter().filter(|(_, on)| *on).map(|(c, _)| *c).collect();
                if cols.is_empty() {
                    self.app.status_message = Some("Choose at least one column to compare".to_string());
                    self.tools.duplicates = Some(dlg);
                } else {
                    self.app.remove_duplicates(dlg.range, dlg.has_header, &cols);
                }
            }
            Some(false) => {}
            None => self.tools.duplicates = Some(dlg),
        }
    }

    // --------------------------------------------------- text to columns

    pub fn open_split(&mut self) {
        self.commit_edit(EditMove::Stay);
        let ((r0, c0), (r1, _)) = self.selection_or_cursor();
        self.tools.split = Some(SplitDialog { range: ((r0, c0), (r1, c0)), delimiter: ",".to_string(), other: String::new() });
    }

    fn split_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.split.take() else { return };
        let mut done = None;
        let ((r0, c0), (r1, _)) = dlg.range;
        let sheet = self.app.workbook.current_sheet();
        let sample = (r0..=r1.min(r0 + 200)).map(|r| sheet.get_cell(r, c0).value).find(|v| !v.is_empty()).unwrap_or_default();
        egui::Modal::new(egui::Id::new("text_to_columns")).show(ctx, |ui| {
            ui.heading(format!("Text to Columns: {}", range_label(dlg.range)));
            ui.label("Split each cell at:");
            for (label, d) in [("Comma", ","), ("Semicolon", ";"), ("Tab", "\t"), ("Space", " ")] {
                ui.radio_value(&mut dlg.delimiter, d.to_string(), label);
            }
            ui.horizontal(|ui| {
                let other = !matches!(dlg.delimiter.as_str(), "," | ";" | "\t" | " ");
                if ui.radio(other, "Other:").clicked() {
                    dlg.delimiter = dlg.other.clone();
                }
                if ui.add(TextEdit::singleline(&mut dlg.other).desired_width(50.0)).changed() {
                    dlg.delimiter = dlg.other.clone();
                }
            });
            if !sample.is_empty() && !dlg.delimiter.is_empty() {
                let parts: Vec<&str> = if dlg.delimiter == " " {
                    sample.split_whitespace().collect()
                } else {
                    sample.split(dlg.delimiter.as_str()).map(str::trim).collect()
                };
                ui.add_space(4.0);
                ui.label(RichText::new(format!("First cell becomes: {}", parts.join("  |  "))).weak());
            }
            ui.label(RichText::new("The pieces go into the columns to the right, replacing what is there.").weak().small());
            done = ok_cancel(ui, "Split");
        });
        match done {
            Some(true) => {
                self.app.text_to_columns(dlg.range, &dlg.delimiter);
            }
            Some(false) => {}
            None => self.tools.split = Some(dlg),
        }
    }

    // --------------------------------------------------------- goal seek

    pub fn open_goal_seek(&mut self) {
        self.commit_edit(EditMove::Stay);
        self.tools.goal = Some(GoalDialog {
            sheet: self.app.workbook.active_sheet,
            target: cell_label((self.app.selected_row, self.app.selected_col)),
            goal: String::new(),
            input: String::new(),
            field: Field::B,
            error: None,
        });
    }

    fn goal_dialog(&mut self, ctx: &Context) {
        let Some(mut dlg) = self.tools.goal.take() else { return };
        let mut done = None;
        let mut open = true;
        egui::Window::new("Goal Seek").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("goal_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Set cell");
                if ui.add(TextEdit::singleline(&mut dlg.target).hint_text("the formula, e.g. B10").desired_width(160.0)).has_focus() {
                    dlg.field = Field::A;
                }
                ui.end_row();
                ui.label("To value");
                ui.add(TextEdit::singleline(&mut dlg.goal).desired_width(160.0));
                ui.end_row();
                ui.label("By changing cell");
                if ui.add(TextEdit::singleline(&mut dlg.input).hint_text("an input, e.g. B2").desired_width(160.0)).has_focus() {
                    dlg.field = Field::B;
                }
                ui.end_row();
            });
            let asked = if dlg.field == Field::A { "the formula cell" } else { "the cell to change" };
            ui.label(RichText::new(format!("Click a cell in the grid to choose {}.", asked)).weak().small());
            error_label(ui, &dlg.error);
            done = ok_cancel(ui, "OK");
        });
        match done {
            _ if !open => {}
            Some(true) => {
                self.show_sheet(dlg.sheet);
                let cell = |text: &str| {
                    Spreadsheet::parse_cell_reference(&text.trim().replace('$', "").to_uppercase())
                        .ok_or_else(|| format!("\"{}\" is not a cell on this sheet", text.trim()))
                };
                let result = cell(&dlg.target).and_then(|target| {
                    let goal = number(&dlg.goal)?.parse::<f64>().map_err(|e| e.to_string())?;
                    let input = cell(&dlg.input)?;
                    self.app.goal_seek(target, goal, input).map(|x| (input, x))
                });
                match result {
                    Ok((input, x)) => {
                        self.app.status_message = Some(format!("Goal Seek: {} = {}", cell_label(input), crate::app::fmt_num(x)));
                    }
                    Err(e) => {
                        dlg.error = Some(e);
                        self.tools.goal = Some(dlg);
                    }
                }
            }
            Some(false) => {}
            None => self.tools.goal = Some(dlg),
        }
    }

    // ------------------------------------------------------------- trace

    /// Show arrows from the cells the cursor's formula reads
    /// (`precedents`) or to the cells that read it.
    pub fn trace(&mut self, precedents: bool) {
        self.commit_edit(EditMove::Stay);
        let sheet = self.app.workbook.active_sheet;
        let cell = (self.app.selected_row, self.app.selected_col);
        let mut trace = self
            .tools
            .trace
            .take()
            .filter(|t| t.sheet == sheet && t.cell == cell)
            .unwrap_or(Trace { sheet, cell, precedents: Vec::new(), dependents: Vec::new() });
        let qualify = |sheet: &Option<String>, label: String| match sheet {
            Some(s) => format!("{}!{}", s, label),
            None => label,
        };
        let labels: Vec<String> = if precedents {
            let found = self.app.direct_precedents();
            trace.precedents = found.iter().filter(|(s, _)| s.is_none()).map(|(_, r)| *r).collect();
            found.iter().map(|(s, r)| qualify(s, range_label(*r))).collect()
        } else {
            let found = self.app.direct_dependents();
            trace.dependents = found.iter().filter(|(s, _)| s.is_none()).map(|(_, c)| *c).collect();
            found.iter().map(|(s, c)| qualify(s, cell_label(*c))).collect()
        };
        let what = if precedents { "precedents" } else { "dependents" };
        self.app.status_message = Some(if labels.is_empty() {
            format!("{} has no {}", cell_label(cell), what)
        } else {
            let more = if labels.len() > 12 { format!(" and {} more", labels.len() - 12) } else { String::new() };
            format!("{} of {}: {}{}", if precedents { "Precedents" } else { "Dependents" }, cell_label(cell), labels[..labels.len().min(12)].join(", "), more)
        });
        self.tools.trace = Some(trace);
    }

    // --------------------------------------------------- format painter

    pub fn toggle_format_painter(&mut self) {
        self.commit_edit(EditMove::Stay);
        if self.tools.painter.take().is_none() {
            self.tools.painter = Some(self.app.copy_formats(self.selection_or_cursor()));
            self.app.status_message = Some("Format Painter: click or drag over the cells to format (Esc cancels)".to_string());
        } else {
            self.app.status_message = None;
        }
    }

    /// Give a name typed into the Name Box to the selection.
    pub fn name_selection(&mut self, name: &str) {
        let reference = self.app.selection_reference();
        self.app.status_message = Some(match valid_name(name).and_then(|_| self.app.define_name(name, &reference)) {
            Ok(()) => format!("Named {} as {}", reference, name),
            Err(e) => format!("\"{}\" is not a cell, range or name. {}", name, e),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_are_built_from_the_dialog_choices() {
        let rule = |kind: CfKind, a: &str, b: &str| kind.predicate(a, b, false, 1, 1000);
        assert_eq!(rule(CfKind::Value(Compare::Greater), "100", "").unwrap(), "AND(ISNUMBER(_), _ > 100)");
        assert_eq!(rule(CfKind::Value(Compare::Equal), "say \"hi\"", "").unwrap(), "_ = \"say \"\"hi\"\"\"");
        assert_eq!(rule(CfKind::Value(Compare::Between), "1", "=B1").unwrap(), "AND(ISNUMBER(_), AND(_ >= 1, _ <= B1))");
        assert_eq!(rule(CfKind::Contains, "late", "").unwrap(), "ISNUMBER(SEARCH(\"late\", _))");
        assert!(rule(CfKind::Value(Compare::Less), " ", "").is_err());
        assert_eq!(parse_columns("d:b"), Some(1..=3));
        assert_eq!(parse_columns("1"), None);
        // Cells and function calls are recognised without "=".
        assert_eq!(rule(CfKind::Value(Compare::Less), "C3", "").unwrap(), "AND(ISNUMBER(_), ISNUMBER(C3), _ < C3)");
        assert_eq!(rule(CfKind::Value(Compare::NotEqual), "Limits!B2", "").unwrap(), "_ <> Limits!B2");
        assert_eq!(
            rule(CfKind::Value(Compare::Equal), "GETPIVOTDATA(\"Sum of Revenue\", \"PivotTable1\")", "").unwrap(),
            "_ = GETPIVOTDATA(\"Sum of Revenue\", \"PivotTable1\")"
        );
        assert_eq!(rule(CfKind::Value(Compare::Equal), "=TaxRate * 2", "").unwrap(), "_ = TaxRate * 2");
        // Quotes keep cell-like text as text; other words are text anyway.
        assert_eq!(rule(CfKind::Value(Compare::Equal), "\"Q1\"", "").unwrap(), "_ = \"Q1\"");
        assert_eq!(rule(CfKind::Value(Compare::Equal), "Open (late)", "").unwrap(), "_ = \"Open (late)\"");
        assert_eq!(rule(CfKind::Value(Compare::Greater), "m", "").unwrap(), "AND(NOT(ISNUMBER(_)), _ > \"m\")");
        // The same row instead of one fixed cell.
        let same_row = |a: &str| CfKind::Value(Compare::Less).predicate(a, "", true, 1, 1000);
        assert_eq!(
            same_row("C3").unwrap(),
            "AND(ISNUMBER(_), ISNUMBER(INDIRECT(\"C\" & ROW())), _ < INDIRECT(\"C\" & ROW()))"
        );
        assert!(same_row("Plan!$D$9").unwrap().contains("INDIRECT(\"Plan!D\" & ROW())"));
        assert!(same_row("100").is_err(), "the same row needs a cell");
        assert_eq!(rule(CfKind::Top, "3", "").unwrap(), "AND(ISNUMBER(_), _ >= LARGE(B1:B1000, 3))");
        assert!(rule(CfKind::Top, "0", "").is_err());
    }

    #[test]
    fn rules_apply_to_numbers_not_to_the_column_title() {
        use cellar::domain::{CellData, Workbook};
        let mut wb = Workbook::default();
        for (r, (b, c)) in [("Price", "Target"), ("3", "5"), ("9", "5"), ("3", "2"), ("20", "50")].iter().enumerate() {
            wb.set_cell_on_active(r, 1, CellData { value: b.to_string(), ..CellData::default() });
            wb.set_cell_on_active(r, 2, CellData { value: c.to_string(), ..CellData::default() });
        }
        let sheet = wb.current_sheet();
        let holds = |kind: CfKind, a: &str, same_row: bool| -> Vec<bool> {
            let rule = kind.predicate(a, "", same_row, 1, 1000).unwrap();
            (0..5).map(|r| sheet.predicate_holds_at(&rule, &sheet.get_cell(r, 1).value, Some(r))).collect()
        };
        assert_eq!(holds(CfKind::Value(Compare::Greater), "5", false), [false, false, true, false, true], "not the title");
        assert_eq!(holds(CfKind::Value(Compare::Less), "C2", false), [false, true, false, true, false], "always C2");
        assert_eq!(holds(CfKind::Value(Compare::Less), "C2", true), [false, true, false, false, true], "each row's own C");
        assert_eq!(holds(CfKind::AboveAverage, "", false), [false, false, true, false, true]);
        assert_eq!(holds(CfKind::Duplicate, "", false), [false, true, false, true, false]);
        assert_eq!(holds(CfKind::Unique, "", false), [true, false, true, false, true]);
        assert_eq!(holds(CfKind::Top, "2", false), [false, false, true, false, true]);
        assert_eq!(holds(CfKind::Bottom, "2", false), [false, true, false, true, false]);
    }

    #[test]
    fn validation_rules_hold_for_good_entries_only() {
        let sheet = Spreadsheet::default();
        let rule = |allow, compare, a: &str, b: &str| {
            ValidationDialog { sheet: 0, col: 0, allow, compare, a: a.into(), b: b.into(), field: Field::A, error: None }
                .predicate()
                .unwrap()
                .unwrap()
        };
        let whole = rule(Allow::Whole, Compare::Between, "1", "10");
        assert!(sheet.predicate_holds(&whole, "7"));
        assert!(!sheet.predicate_holds(&whole, "7.5"));
        assert!(!sheet.predicate_holds(&whole, "11"));
        assert!(!sheet.predicate_holds(&whole, "seven"));
        let list = rule(Allow::List, Compare::Equal, "Open, Closed, 3", "");
        assert_eq!(list, "OR(_ = \"Open\", _ = \"Closed\", _ = 3)");
        assert!(sheet.predicate_holds(&list, "Closed"));
        assert!(sheet.predicate_holds(&list, "3"));
        assert!(!sheet.predicate_holds(&list, "Pending"));
        let length = rule(Allow::TextLength, Compare::AtMost, "3", "");
        assert!(sheet.predicate_holds(&length, "abc"));
        assert!(!sheet.predicate_holds(&length, "abcd"));
        // Limits and lists can come from cells.
        assert_eq!(rule(Allow::Decimal, Compare::AtMost, "F1", ""), "AND(ISNUMBER(_), _ <= F1)");
        assert_eq!(rule(Allow::List, Compare::Equal, "Lists!A2:A20", ""), "COUNTIF(Lists!A2:A20, _) > 0");
        assert_eq!(rule(Allow::List, Compare::Equal, "=Statuses", ""), "COUNTIF(Statuses, _) > 0");
        let bad = ValidationDialog { sheet: 0, col: 0, allow: Allow::Whole, compare: Compare::AtMost, a: "ten".into(), b: String::new(), field: Field::A, error: None };
        assert!(bad.predicate().is_err());
        let any = ValidationDialog { allow: Allow::Any, ..bad };
        assert_eq!(any.predicate(), Ok(None));
    }
}
