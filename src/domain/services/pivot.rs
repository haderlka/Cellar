//! PivotTable engine: reads a source range, aggregates it per a
//! [`PivotSpec`], and lays the result out the way Excel does (compact,
//! outline or tabular form, subtotals, grand totals, "Show Values As").
//! Also produces PivotChart data from the same aggregation.
//!
//! Pure domain code: input is a `Workbook`, output is plain data that the
//! GUI renders.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::domain::models::{
    format_cell_value, CellFormat, NumberFormat, PivotField, PivotLayout, PivotSort, PivotSpec,
    ShowValuesAs, Spreadsheet, Summarize, Workbook, BLANK_ITEM,
};

/// Records of a pivot's source range: header names plus data rows.
#[derive(Debug, Clone, Default)]
pub struct PivotData {
    pub fields: Vec<String>,
    pub records: Vec<Vec<String>>,
}

impl PivotData {
    /// Read `source` (header row + data) resolving unqualified refs on sheet
    /// `host`. Fully blank rows are skipped; blank headers become
    /// "Column X" so every field has a name.
    pub fn read(wb: &Workbook, host: usize, source: &str) -> Result<Self, String> {
        let sheet = wb.sheets.get(host).ok_or("Source sheet missing")?;
        Self::read_on(Some(wb), sheet, source)
    }

    /// Like [`read`](Self::read) with the host sheet given directly. A
    /// sheet-qualified source needs `wb`.
    pub fn read_on(wb: Option<&Workbook>, host: &Spreadsheet, source: &str) -> Result<Self, String> {
        let src = SourceRange::parse(source)?;
        let sheet = match &src.sheet {
            Some(name) => {
                let wb = wb.ok_or_else(|| format!("Sheet '{}' not found", name))?;
                let idx = wb
                    .sheet_names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(name))
                    .ok_or_else(|| format!("Sheet '{}' not found", name))?;
                wb.sheets.get(idx).ok_or("Source sheet missing")?
            }
            None => host,
        };
        let ((r0, c0), (r1, c1)) = (src.start, src.end);
        let (last_r, last_c) = sheet.last_cell();
        let (r1, c1) = (r1.min(last_r.max(r0)), c1.min(last_c.max(c0)));
        let value = |r: usize, c: usize| sheet.cells.get(&(r, c)).map(|cd| cd.value.clone()).unwrap_or_default();

        let mut fields: Vec<String> = Vec::new();
        for c in c0..=c1 {
            let mut name = value(r0, c).trim().to_string();
            if name.is_empty() {
                name = format!("Column {}", Spreadsheet::column_label(c));
            }
            // Excel appends 2, 3, … to duplicate header names.
            let base = name.clone();
            let mut n = 2;
            while fields.contains(&name) {
                name = format!("{}{}", base, n);
                n += 1;
            }
            fields.push(name);
        }
        let records = ((r0 + 1)..=r1)
            .map(|r| (c0..=c1).map(|c| value(r, c)).collect::<Vec<_>>())
            .filter(|rec| rec.iter().any(|v| !v.trim().is_empty()))
            .collect();
        Ok(Self { fields, records })
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f == name)
    }

    /// Distinct item labels of a field, sorted A→Z (`(blank)` last).
    pub fn items(&self, field: &str) -> Vec<String> {
        let Some(i) = self.field_index(field) else { return Vec::new() };
        let set: HashSet<String> = self.records.iter().map(|r| item_label(&r[i])).collect();
        let mut items: Vec<String> = set.into_iter().collect();
        items.sort_by(|a, b| compare_labels(a, b));
        items
    }

    /// True when every non-blank value is a number. Excel's checkbox adds
    /// such fields to Values (Sum); others go to Rows / get Count.
    pub fn is_numeric(&self, field: &str) -> bool {
        let Some(i) = self.field_index(field) else { return false };
        let mut any = false;
        for r in &self.records {
            let v = r[i].trim();
            if v.is_empty() {
                continue;
            }
            if v.parse::<f64>().is_err() {
                return false;
            }
            any = true;
        }
        any
    }
}

/// A pivot's source range, parsed: optional sheet name plus the corners.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRange {
    pub sheet: Option<String>,
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl SourceRange {
    pub fn parse(source: &str) -> Result<Self, String> {
        let (sheet, range) = match source.rsplit_once('!') {
            Some((name, range)) => (Some(name.trim().trim_matches('\'').replace("''", "'")), range),
            None => (None, source),
        };
        let range = range.trim().replace('$', "").to_uppercase();
        let (a, b) = range.split_once(':').ok_or("The source must be a range like A1:D20")?;
        let (Some((r0, c0)), Some((r1, c1))) =
            (Spreadsheet::parse_cell_reference(a), Spreadsheet::parse_cell_reference(b))
        else {
            return Err(format!("Invalid source range '{}'", source));
        };
        Ok(Self { sheet, start: (r0.min(r1), c0.min(c1)), end: (r0.max(r1), c0.max(c1)) })
    }
}

fn item_label(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() { BLANK_ITEM.to_string() } else { t.to_string() }
}

/// Excel's built-in custom lists, which it uses when sorting PivotTable
/// items so months and weekdays come out in calendar order. English and
/// German spellings.
const CUSTOM_LISTS: &[&[&str]] = &[
    &["sun", "mon", "tue", "wed", "thu", "fri", "sat"],
    &["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"],
    &["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"],
    &["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"],
    &["mo", "di", "mi", "do", "fr", "sa", "so"],
    &["montag", "dienstag", "mittwoch", "donnerstag", "freitag", "samstag", "sonntag"],
    &["jan", "feb", "mär", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "dez"],
    &["januar", "februar", "märz", "april", "mai", "juni", "juli", "august", "september", "oktober", "november", "dezember"],
];

/// Position of both labels in the same custom list, if they share one.
fn custom_list_positions(a: &str, b: &str) -> Option<(usize, usize)> {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    CUSTOM_LISTS.iter().find_map(|list| {
        Some((list.iter().position(|x| *x == a)?, list.iter().position(|x| *x == b)?))
    })
}

/// Numbers numerically, month/weekday names in calendar order, other text
/// case-insensitively, `(blank)` last.
pub fn compare_labels(a: &str, b: &str) -> Ordering {
    match (a == BLANK_ITEM, b == BLANK_ITEM) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    if let Some((x, y)) = custom_list_positions(a, b) {
        return x.cmp(&y);
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        _ => a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)),
    }
}

// ------------------------------------------------------------ aggregation

#[derive(Debug, Clone, Copy)]
struct Acc {
    count: usize,
    nums: usize,
    sum: f64,
    sumsq: f64,
    product: f64,
    min: f64,
    max: f64,
}

impl Default for Acc {
    fn default() -> Self {
        Self { count: 0, nums: 0, sum: 0.0, sumsq: 0.0, product: 1.0, min: f64::INFINITY, max: f64::NEG_INFINITY }
    }
}

impl Acc {
    fn add(&mut self, raw: &str) {
        let t = raw.trim();
        if t.is_empty() {
            return;
        }
        self.count += 1;
        if let Ok(x) = t.parse::<f64>() {
            self.nums += 1;
            self.sum += x;
            self.sumsq += x * x;
            self.product *= x;
            self.min = self.min.min(x);
            self.max = self.max.max(x);
        }
    }

    fn result(&self, how: Summarize) -> Option<f64> {
        let n = self.nums as f64;
        let var = |pop: bool| {
            let d = if pop { n } else { n - 1.0 };
            (d > 0.0).then(|| ((self.sumsq - self.sum * self.sum / n) / d).max(0.0))
        };
        match how {
            Summarize::Count => (self.count > 0).then_some(self.count as f64),
            _ if self.count == 0 => None,
            Summarize::Sum => Some(self.sum),
            Summarize::CountNumbers => Some(n),
            Summarize::Average => (self.nums > 0).then(|| self.sum / n),
            Summarize::Max => (self.nums > 0).then_some(self.max),
            Summarize::Min => (self.nums > 0).then_some(self.min),
            Summarize::Product => (self.nums > 0).then_some(self.product),
            Summarize::Var => var(false),
            Summarize::VarP => var(true),
            Summarize::StdDev => var(false).map(f64::sqrt),
            Summarize::StdDevP => var(true).map(f64::sqrt),
        }
    }
}

type Path = Vec<String>;

/// Which kind of output cell, so the GUI can style it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotCellKind {
    Blank,
    /// Captions such as "Row Labels", "Column Labels", value names.
    Header,
    /// Row or column item label.
    Item,
    /// Subtotal label or value.
    Subtotal,
    /// Grand total label or value.
    Grand,
    /// A plain aggregated value.
    Value,
}

#[derive(Debug, Clone)]
pub struct PivotCell {
    pub text: String,
    pub kind: PivotCellKind,
    /// Indentation level (compact form).
    pub indent: u8,
    /// Numeric result, for right-alignment.
    pub number: Option<f64>,
    /// For value cells: how GETPIVOTDATA addresses the cell.
    pub reference: Option<PivotRef>,
}

impl PivotCell {
    fn text(text: impl Into<String>, kind: PivotCellKind) -> Self {
        Self { text: text.into(), kind, indent: 0, number: None, reference: None }
    }
    fn blank() -> Self {
        Self::text("", PivotCellKind::Blank)
    }
}

/// PivotChart data: categories from the row axis, one series per column
/// item × value field. Subtotals and grand totals are excluded, as in
/// Excel.
#[derive(Debug, Clone, Default)]
pub struct PivotChartData {
    pub categories: Vec<String>,
    pub series: Vec<(String, Vec<Option<f64>>)>,
}

#[derive(Debug, Clone, Default)]
pub struct PivotOutput {
    pub table: Vec<Vec<PivotCell>>,
    /// Number of leading header rows in `table`.
    pub header_rows: usize,
    /// Number of leading row-label columns in `table`.
    pub label_cols: usize,
    pub chart: PivotChartData,
}

#[derive(Debug, Clone)]
enum Slot {
    /// An item; `parent` items have children (and show the subtotal on
    /// their own line when subtotals are at the top).
    Item { path: Path, parent: bool },
    /// A subtotal line/column after a group's children.
    Subtotal { path: Path },
    Grand,
}

impl Slot {
    fn agg_path(&self) -> &[String] {
        match self {
            Slot::Item { path, .. } | Slot::Subtotal { path } => path,
            Slot::Grand => &[],
        }
    }
}

struct Engine<'a> {
    spec: &'a PivotSpec,
    acc: HashMap<(Path, Path), Vec<Acc>>,
    /// Ordered children per axis: prefix → item labels.
    row_children: HashMap<Path, Vec<String>>,
    col_children: HashMap<Path, Vec<String>>,
}

impl Engine<'_> {
    fn raw(&self, rp: &[String], cp: &[String], v: usize) -> Option<f64> {
        let how = self.spec.values.get(v)?.summarize;
        self.acc.get(&(rp.to_vec(), cp.to_vec()))?.get(v)?.result(how)
    }

    /// Value after "Show Values As".
    fn shown(&self, rp: &[String], cp: &[String], v: usize) -> Option<f64> {
        let x = self.raw(rp, cp, v)?;
        let pv = &self.spec.values[v];
        let ratio = |d: Option<f64>| d.filter(|d| *d != 0.0).map(|d| x / d);
        // Base field → (is_row_axis, level).
        let base = pv.base_field.as_deref().and_then(|bf| {
            self.spec
                .rows
                .iter()
                .position(|f| f.field == bf)
                .map(|k| (true, k))
                .or_else(|| self.spec.columns.iter().position(|f| f.field == bf).map(|k| (false, k)))
        });
        let with_axis = |is_row: bool, path: Path| -> Option<f64> {
            if is_row { self.raw(&path, cp, v) } else { self.raw(rp, &path, v) }
        };
        match pv.show_as {
            ShowValuesAs::NoCalculation => Some(x),
            ShowValuesAs::PercentOfGrandTotal => ratio(self.raw(&[], &[], v)),
            ShowValuesAs::PercentOfColumnTotal => ratio(self.raw(&[], cp, v)),
            ShowValuesAs::PercentOfRowTotal => ratio(self.raw(rp, &[], v)),
            ShowValuesAs::PercentOfParentRowTotal => {
                if rp.is_empty() { Some(1.0) } else { ratio(self.raw(&rp[..rp.len() - 1], cp, v)) }
            }
            ShowValuesAs::PercentOfParentColumnTotal => {
                if cp.is_empty() { Some(1.0) } else { ratio(self.raw(rp, &cp[..cp.len() - 1], v)) }
            }
            ShowValuesAs::Index => {
                let (g, rt, ct) = (self.raw(&[], &[], v)?, self.raw(rp, &[], v)?, self.raw(&[], cp, v)?);
                (rt * ct != 0.0).then(|| x * g / (rt * ct))
            }
            other => {
                let (is_row, k) = base?;
                let path = if is_row { rp } else { cp };
                if path.len() <= k {
                    // Cell is above the base field's level.
                    return (other == ShowValuesAs::PercentOfParentTotal).then_some(1.0);
                }
                let children = if is_row { &self.row_children } else { &self.col_children };
                let siblings = children.get(&path[..k].to_vec())?;
                let at = |item: &str| -> Option<f64> {
                    let mut p = path.to_vec();
                    p[k] = item.to_string();
                    with_axis(is_row, p)
                };
                let pos = siblings.iter().position(|s| *s == path[k])?;
                match other {
                    ShowValuesAs::PercentOfParentTotal => ratio(with_axis(is_row, path[..=k].to_vec())),
                    ShowValuesAs::RunningTotal | ShowValuesAs::PercentRunningTotal => {
                        let run: f64 = siblings[..=pos].iter().filter_map(|s| at(s)).sum();
                        if other == ShowValuesAs::RunningTotal {
                            Some(run)
                        } else {
                            let total: f64 = siblings.iter().filter_map(|s| at(s)).sum();
                            (total != 0.0).then(|| run / total)
                        }
                    }
                    ShowValuesAs::RankAscending | ShowValuesAs::RankDescending => {
                        let asc = other == ShowValuesAs::RankAscending;
                        let better = siblings
                            .iter()
                            .filter_map(|s| at(s))
                            .filter(|y| if asc { *y < x } else { *y > x })
                            .count();
                        Some(better as f64 + 1.0)
                    }
                    _ => {
                        // % Of / Difference From / % Difference From.
                        let base_item = match pv.base_item.as_deref() {
                            Some("(previous)") => siblings.get(pos.checked_sub(1)?)?.clone(),
                            Some("(next)") => siblings.get(pos + 1)?.clone(),
                            Some(item) => item.to_string(),
                            None => return None,
                        };
                        let b = at(&base_item);
                        let is_base = base_item == path[k];
                        match other {
                            ShowValuesAs::PercentOf => ratio(b),
                            ShowValuesAs::DifferenceFrom => (!is_base).then(|| x - b.unwrap_or(0.0)),
                            _ => {
                                if is_base { None } else { b.filter(|b| *b != 0.0).map(|b| (x - b) / b) }
                            }
                        }
                    }
                }
            }
        }
    }

    fn format(&self, v: usize, x: f64) -> String {
        let pv = &self.spec.values[v];
        let fmt = match (&pv.number_format, pv.show_as) {
            (Some(f), _) => f.clone(),
            (None, s) if s.is_percentage() => NumberFormat::Percentage { decimals: 2 },
            _ => NumberFormat::General,
        };
        let raw = if fmt == NumberFormat::General {
            // Excel's General: up to ~10 significant decimals, no noise.
            let rounded = (x * 1e10).round() / 1e10;
            format!("{}", rounded)
        } else {
            x.to_string()
        };
        format_cell_value(&raw, &CellFormat { number_format: fmt, ..CellFormat::default() })
    }
}

/// The aggregation plus the row and column lines it is shown on.
struct Prepared<'a> {
    engine: Engine<'a>,
    row_slots: Vec<Slot>,
    col_slots: Vec<Slot>,
}

fn prepare<'a>(spec: &'a PivotSpec, data: &PivotData) -> Result<Prepared<'a>, String> {
    let idx = |name: &str| data.field_index(name).ok_or_else(|| format!("Field '{}' is not in the source data", name));
    let filter_idx = spec
        .filters
        .iter()
        .chain(&spec.rows)
        .chain(&spec.columns)
        .map(|f| Ok((idx(&f.field)?, &f.hidden_items)))
        .collect::<Result<Vec<_>, String>>()?;
    let row_idx = spec.rows.iter().map(|f| idx(&f.field)).collect::<Result<Vec<_>, _>>()?;
    let col_idx = spec.columns.iter().map(|f| idx(&f.field)).collect::<Result<Vec<_>, _>>()?;
    let val_idx = spec.values.iter().map(|v| idx(&v.field)).collect::<Result<Vec<_>, _>>()?;
    let (nr, nc, nv) = (row_idx.len(), col_idx.len(), val_idx.len());

    // First-appearance order per field, for "Manual" (data source) sort.
    let mut first_seen: HashMap<(usize, String), usize> = HashMap::new();
    let mut acc: HashMap<(Path, Path), Vec<Acc>> = HashMap::new();
    for (ri, rec) in data.records.iter().enumerate() {
        if filter_idx.iter().any(|(i, hidden)| hidden.contains(&item_label(&rec[*i]))) {
            continue;
        }
        let rk: Path = row_idx.iter().map(|&i| item_label(&rec[i])).collect();
        let ck: Path = col_idx.iter().map(|&i| item_label(&rec[i])).collect();
        for (&i, label) in row_idx.iter().zip(&rk).chain(col_idx.iter().zip(&ck)) {
            first_seen.entry((i, label.clone())).or_insert(ri);
        }
        for i in 0..=nr {
            for j in 0..=nc {
                let e = acc.entry((rk[..i].to_vec(), ck[..j].to_vec())).or_insert_with(|| vec![Acc::default(); nv]);
                for (v, &vi) in val_idx.iter().enumerate() {
                    e[v].add(&rec[vi]);
                }
            }
        }
    }

    let mut engine = Engine { spec, acc, row_children: HashMap::new(), col_children: HashMap::new() };
    engine.row_children = build_children(&engine, true, &row_idx, &first_seen);
    engine.col_children = build_children(&engine, false, &col_idx, &first_seen);

    let opts = &spec.options;
    let row_slots = axis_slots(&engine.row_children, &spec.rows.iter().map(|f| f.subtotals).collect::<Vec<_>>(), nr, true, opts.layout, opts.subtotals_at_top, opts.grand_totals_columns);
    let col_slots = axis_slots(&engine.col_children, &spec.columns.iter().map(|f| f.subtotals).collect::<Vec<_>>(), nc, false, PivotLayout::Tabular, false, opts.grand_totals_rows);
    Ok(Prepared { engine, row_slots, col_slots })
}

/// Compute a pivot's output. Errors are user-facing messages (bad range,
/// missing field).
pub fn compute_pivot(spec: &PivotSpec, data: &PivotData) -> Result<PivotOutput, String> {
    let Prepared { engine, row_slots, col_slots } = prepare(spec, data)?;
    let (nr, nc, nv) = (spec.rows.len(), spec.columns.len(), spec.values.len());
    let opts = &spec.options;

    // Expand Σ Values onto its axis.
    let values_on_rows = opts.values_on_rows && nv > 1;
    let vals_per_col = if values_on_rows || nv == 0 { vec![None] } else { (0..nv).map(Some).collect() };
    let col_leaves: Vec<(&Slot, Option<usize>)> = col_slots
        .iter()
        .flat_map(|s| vals_per_col.iter().map(move |v| (s, *v)))
        .collect();
    // Without any values Excel shows just the item labels.
    let col_leaves = if nv == 0 { Vec::new() } else { col_leaves };

    let compact = opts.layout == PivotLayout::Compact;
    let label_cols = if compact { 1 } else { nr.max(1) };
    let show_values_header = !values_on_rows && nv > 1;
    let value_caption = |v: usize| spec.values[v].display_name();

    // ---- header rows
    let mut table: Vec<Vec<PivotCell>> = Vec::new();
    let header_levels = nc + usize::from(show_values_header);
    let header_levels = header_levels.max(1);
    if nc > 0 {
        // Top row: value caption (single value) + "Column Labels".
        let mut row = vec![PivotCell::blank(); label_cols + col_leaves.len()];
        if nv == 1 {
            row[0] = PivotCell::text(value_caption(0), PivotCellKind::Header);
        }
        if let Some(first) = row.get_mut(label_cols) {
            *first = PivotCell::text(
                if opts.layout == PivotLayout::Tabular {
                    spec.columns.iter().map(|f| f.field.clone()).collect::<Vec<_>>().join(" / ")
                } else {
                    "Column Labels".to_string()
                },
                PivotCellKind::Header,
            );
        }
        table.push(row);
    }
    for level in 0..header_levels {
        let last = level + 1 == header_levels;
        let mut row = vec![PivotCell::blank(); label_cols];
        if last {
            if compact {
                if nr > 0 {
                    row[0] = PivotCell::text("Row Labels", PivotCellKind::Header);
                } else if values_on_rows {
                    row[0] = PivotCell::text("Values", PivotCellKind::Header);
                }
            } else {
                for (k, f) in spec.rows.iter().enumerate() {
                    row[k] = PivotCell::text(f.field.clone(), PivotCellKind::Header);
                }
            }
        }
        let mut prev: Option<Path> = None;
        for (slot, v) in &col_leaves {
            let is_value_level = show_values_header && level == header_levels - 1;
            let cell = if nc == 0 {
                // No column fields: header is the value name.
                match v {
                    Some(v) => PivotCell::text(value_caption(*v), PivotCellKind::Header),
                    None => PivotCell::text("Total", PivotCellKind::Header),
                }
            } else if is_value_level {
                let kind = if matches!(slot, Slot::Grand) { PivotCellKind::Grand } else { PivotCellKind::Header };
                let name = v.map(value_caption).unwrap_or_default();
                let text = if matches!(slot, Slot::Grand) { format!("Total {}", name) } else { name };
                PivotCell::text(text, kind)
            } else {
                match slot {
                    Slot::Item { path, .. } => {
                        let key = path[..=level.min(path.len() - 1)].to_vec();
                        let shown = level < path.len() && prev.as_ref() != Some(&key);
                        prev = Some(key);
                        if shown { PivotCell::text(path[level].clone(), PivotCellKind::Item) } else { PivotCell::blank() }
                    }
                    Slot::Subtotal { path } => {
                        let shown = level + 1 == path.len() && prev.as_ref() != Some(&[path.clone(), vec!["\u{1}".into()]].concat());
                        prev = Some([path.clone(), vec!["\u{1}".into()]].concat());
                        if shown { PivotCell::text(format!("{} Total", path[level]), PivotCellKind::Subtotal) } else { PivotCell::blank() }
                    }
                    Slot::Grand => {
                        let shown = level == 0 && prev.as_deref() != Some(&["\u{2}".to_string()][..]);
                        prev = Some(vec!["\u{2}".into()]);
                        if shown { PivotCell::text("Grand Total", PivotCellKind::Grand) } else { PivotCell::blank() }
                    }
                }
            };
            row.push(cell);
        }
        table.push(row);
    }
    let header_rows = table.len();

    // ---- body
    let vals_per_row: Vec<Option<usize>> = if values_on_rows { (0..nv).map(Some).collect() } else { vec![None] };
    let mut prev_path: Path = Vec::new();
    for slot in &row_slots {
        for (vi, rv) in vals_per_row.iter().enumerate() {
            // Values on rows: the slot's own line, then one line per value.
            let lines: Vec<Option<usize>> = if values_on_rows && vi == 0 { vec![None, *rv] } else { vec![*rv] };
            for line_val in lines {
                let header_line = values_on_rows && line_val.is_none();
                let mut row = vec![PivotCell::blank(); label_cols];
                let kind = match slot {
                    Slot::Item { .. } => PivotCellKind::Item,
                    Slot::Subtotal { .. } => PivotCellKind::Subtotal,
                    Slot::Grand => PivotCellKind::Grand,
                };
                let depth = slot.agg_path().len().saturating_sub(1);
                if let Some(v) = line_val.filter(|_| values_on_rows) {
                    // Value-name line under its item.
                    let col = if compact { 0 } else { label_cols - 1 };
                    let name = if matches!(slot, Slot::Grand) { format!("Total {}", value_caption(v)) } else { value_caption(v) };
                    row[col] = PivotCell { text: name, kind, indent: if compact { depth as u8 + 1 } else { 0 }, number: None, reference: None };
                } else {
                    match slot {
                        Slot::Grand => {
                            if nr > 0 || !values_on_rows {
                                row[0] = PivotCell::text(if nr > 0 { "Grand Total" } else { "Total" }, PivotCellKind::Grand);
                            }
                        }
                        Slot::Subtotal { path } => {
                            let col = if compact { 0 } else { path.len() - 1 };
                            row[col] = PivotCell {
                                text: format!("{} Total", path.last().cloned().unwrap_or_default()),
                                kind,
                                indent: if compact { depth as u8 } else { 0 },
                                number: None,
                                reference: None,
                            };
                        }
                        Slot::Item { path, .. } => {
                            if compact {
                                row[0] = PivotCell { text: path[depth].clone(), kind, indent: depth as u8, number: None, reference: None };
                            } else if opts.layout == PivotLayout::Outline {
                                row[depth] = PivotCell::text(path[depth].clone(), kind);
                                if opts.repeat_item_labels {
                                    for (k, label) in path.iter().enumerate().take(depth) {
                                        row[k] = PivotCell::text(label.clone(), kind);
                                    }
                                }
                            } else {
                                // Tabular: parents on the first line of their group.
                                for (k, label) in path.iter().enumerate() {
                                    let new_group = prev_path.len() <= k || prev_path[..=k] != path[..=k];
                                    if new_group || opts.repeat_item_labels || k == depth {
                                        row[k] = PivotCell::text(label.clone(), kind);
                                    }
                                }
                            }
                            prev_path = path.clone();
                        }
                    }
                }
                // Value cells.
                let parent_without_values = matches!(slot, Slot::Item { parent: true, path } if !(opts.subtotals_at_top && spec.rows[path.len() - 1].subtotals))
                    || header_line;
                for (cslot, cv) in &col_leaves {
                    let v = line_val.or(*cv);
                    let cell = match v {
                        Some(v) if !parent_without_values => {
                            let value_kind = match (slot, cslot) {
                                (Slot::Grand, _) | (_, Slot::Grand) => PivotCellKind::Grand,
                                (Slot::Subtotal { .. }, _) | (_, Slot::Subtotal { .. }) | (Slot::Item { parent: true, .. }, _) => PivotCellKind::Subtotal,
                                _ => PivotCellKind::Value,
                            };
                            match engine.shown(slot.agg_path(), cslot.agg_path(), v) {
                                Some(x) => PivotCell {
                                    text: engine.format(v, x),
                                    kind: value_kind,
                                    indent: 0,
                                    number: Some(x),
                                    reference: Some(PivotRef::new(spec, v, slot.agg_path(), cslot.agg_path())),
                                },
                                None => PivotCell::text(opts.empty_cells.clone(), value_kind),
                            }
                        }
                        _ => PivotCell::blank(),
                    };
                    row.push(cell);
                }
                table.push(row);
            }
        }
    }

    // ---- PivotChart data
    let mut chart = PivotChartData::default();
    let leaf_rows: Vec<&Path> = row_slots
        .iter()
        .filter_map(|s| match s {
            Slot::Item { path, parent: false } => Some(path),
            _ => None,
        })
        .collect();
    let empty: Path = Vec::new();
    let row_keys: Vec<&Path> = if nr == 0 { vec![&empty] } else { leaf_rows };
    chart.categories = row_keys.iter().map(|p| if p.is_empty() { "Total".to_string() } else { p.join(" / ") }).collect();
    let leaf_cols: Vec<&Path> = col_slots
        .iter()
        .filter_map(|s| match s {
            Slot::Item { path, parent: false } => Some(path),
            _ => None,
        })
        .collect();
    let col_keys: Vec<&Path> = if nc == 0 { vec![&empty] } else { leaf_cols };
    for cp in &col_keys {
        for v in 0..nv {
            let name = match (cp.is_empty(), nv > 1) {
                (true, _) => value_caption(v),
                (false, false) => cp.join(" / "),
                (false, true) => format!("{} - {}", cp.join(" / "), value_caption(v)),
            };
            let points = row_keys.iter().map(|rp| super::chart_data::plottable(engine.shown(rp, cp, v))).collect();
            chart.series.push((name, points));
        }
    }

    Ok(PivotOutput { table, header_rows, label_cols, chart })
}

/// A value cell's address in GETPIVOTDATA terms: the value field plus the
/// row and column items leading to it (none for grand totals).
#[derive(Debug, Clone, PartialEq)]
pub struct PivotRef {
    /// Index into `PivotSpec::values`.
    pub value: usize,
    /// (field, item) pairs, row fields first.
    pub items: Vec<(String, String)>,
}

impl PivotRef {
    fn new(spec: &PivotSpec, value: usize, rows: &[String], cols: &[String]) -> Self {
        let pairs = |fields: &[PivotField], items: &[String]| -> Vec<(String, String)> {
            fields.iter().zip(items).map(|(f, i)| (f.field.clone(), i.clone())).collect()
        };
        let mut items = pairs(&spec.rows, rows);
        items.extend(pairs(&spec.columns, cols));
        Self { value, items }
    }

    /// `GETPIVOTDATA("Sum of Revenue","PivotTable1","Region","East")`,
    /// without the leading `=`. Numeric items are written as numbers.
    /// `sheet` names the pivot's sheet for a formula on another sheet
    /// (`"Sheet2!PivotTable1"`).
    pub fn formula(&self, spec: &PivotSpec, sheet: Option<&str>) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let item = |s: &str| match s.parse::<f64>() {
            Ok(x) if x.is_finite() && !s.starts_with(['+', '.']) && s.trim() == s => s.to_string(),
            _ => quote(s),
        };
        let mut args = vec![
            quote(&spec.values.get(self.value).map(|v| v.display_name()).unwrap_or_default()),
            quote(&match sheet {
                Some(sheet) => format!("{}!{}", sheet, spec.name),
                None => spec.name.clone(),
            }),
        ];
        for (field, it) in &self.items {
            args.push(quote(field));
            args.push(item(it));
        }
        format!("GETPIVOTDATA({})", args.join(","))
    }
}

fn item_matches(label: &str, wanted: &str) -> bool {
    let wanted = wanted.trim();
    if label.to_lowercase() == wanted.to_lowercase() {
        return true;
    }
    matches!((label.parse::<f64>(), wanted.parse::<f64>()), (Ok(a), Ok(b)) if a == b)
}

/// GETPIVOTDATA: the value `data_field` (its name, e.g. "Sum of Revenue",
/// or its source field) shows where the row and column items in `items`
/// meet. Fields left out mean their total. As in Excel, only values the
/// PivotTable shows can be retrieved: a hidden item, a subtotal or grand
/// total that is switched off, or an empty cell is an error.
pub fn get_pivot_data(
    spec: &PivotSpec,
    data: &PivotData,
    data_field: &str,
    items: &[(String, String)],
) -> Result<f64, String> {
    let data_field = data_field.trim();
    let v = spec
        .values
        .iter()
        .position(|pv| pv.display_name().eq_ignore_ascii_case(data_field))
        .or_else(|| spec.values.iter().position(|pv| pv.field.eq_ignore_ascii_case(data_field)))
        .ok_or_else(|| format!("'{}' is not a value field of {}", data_field, spec.name))?;
    let mut row_want: Vec<Option<&str>> = vec![None; spec.rows.len()];
    let mut col_want: Vec<Option<&str>> = vec![None; spec.columns.len()];
    for (field, item) in items {
        let find = |axis: &[PivotField]| axis.iter().position(|f| f.field.eq_ignore_ascii_case(field.trim()));
        let want = match (find(&spec.rows), find(&spec.columns)) {
            (Some(k), _) => &mut row_want[k],
            (None, Some(k)) => &mut col_want[k],
            _ => return Err(format!("'{}' is not a row or column field of {}", field, spec.name)),
        };
        if want.replace(item).is_some() {
            return Err(format!("'{}' is given twice", field));
        }
    }
    let Prepared { engine, row_slots, col_slots } = prepare(spec, data)?;
    // Parent lines in compact/outline form show values only when their
    // subtotal is shown at the top of the group.
    let shows_values = |s: &Slot| match s {
        Slot::Item { parent: true, path } => {
            spec.options.subtotals_at_top && spec.rows[path.len() - 1].subtotals
        }
        _ => true,
    };
    let pick = |slots: &[Slot], want: &[Option<&str>]| -> Result<Path, String> {
        let depth = want.iter().rposition(Option::is_some).map_or(0, |k| k + 1);
        let mut found: Vec<&[String]> = slots
            .iter()
            .filter(|s| shows_values(s))
            .map(Slot::agg_path)
            .filter(|p| {
                p.len() == depth
                    && p.iter().zip(want).all(|(label, w)| w.is_none_or(|w| item_matches(label, w)))
            })
            .collect();
        found.sort();
        found.dedup();
        match found.as_slice() {
            [one] => Ok(one.to_vec()),
            [] => Err("The PivotTable doesn't show that item".to_string()),
            _ => Err("More than one item matches; name the outer fields too".to_string()),
        }
    };
    let rp = pick(&row_slots, &row_want)?;
    let cp = pick(&col_slots, &col_want)?;
    engine.shown(&rp, &cp, v).ok_or_else(|| "The PivotTable shows no value there".to_string())
}

/// A PivotTable found by name, for GETPIVOTDATA.
pub struct NamedPivot<'a> {
    pub spec: &'a PivotSpec,
    /// The sheet holding the pivot; an unqualified source is on it.
    pub sheet: &'a Spreadsheet,
    /// That sheet's name when the pivot was found through the workbook.
    pub sheet_name: Option<&'a str>,
}

impl NamedPivot<'_> {
    pub fn read(&self, wb: Option<&Workbook>) -> Result<PivotData, String> {
        PivotData::read_on(wb, self.sheet, &self.spec.source)
    }

    /// The source range's corners as formula references, the first one
    /// sheet-qualified when the source isn't on the formula's own sheet.
    pub fn source_range(&self) -> Option<(String, String)> {
        let src = SourceRange::parse(&self.spec.source).ok()?;
        let a1 = |(r, c): (usize, usize)| format!("{}{}", Spreadsheet::column_label(c), r + 1);
        let start = match src.sheet.as_deref().or(self.sheet_name) {
            Some(sheet) => format!("'{}'!{}", sheet.replace('\'', "''"), a1(src.start)),
            None => a1(src.start),
        };
        Some((start, a1(src.end)))
    }
}

/// Split a pivot name as written in GETPIVOTDATA into a sheet and the
/// pivot's name when it is qualified (`Sheet2!PivotTable1`,
/// `'My Data'!PivotTable1`). Sheet and pivot names may both contain `!`, so
/// every split point is tried against the workbook's sheet names.
pub fn split_pivot_name<'n>(sheet_names: &[String], name: &'n str) -> Option<(usize, &'n str)> {
    name.match_indices('!').find_map(|(i, _)| {
        let sheet = unquote_sheet(&name[..i]);
        let idx = sheet_names.iter().position(|n| n.eq_ignore_ascii_case(&sheet))?;
        Some((idx, &name[i + 1..]))
    })
}

/// Find the PivotTable `name` (`PivotTable1`, or `Sheet2!PivotTable1`):
/// on the formula's own sheet first, then on the others in tab order.
pub fn find_pivot<'a>(wb: Option<&'a Workbook>, host: &'a Spreadsheet, name: &str) -> Option<NamedPivot<'a>> {
    let name = name.trim();
    let on = |sheet: &'a Spreadsheet, sheet_name: Option<&'a str>, pivot: &str| {
        sheet
            .pivots
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(pivot.trim()))
            .map(|spec| NamedPivot { spec, sheet, sheet_name })
    };
    if let Some(wb) = wb
        && let Some((i, pivot)) = split_pivot_name(&wb.sheet_names, name)
        && let Some(found) = on(&wb.sheets[i], Some(&wb.sheet_names[i]), pivot)
    {
        return Some(found);
    }
    if let Some(found) = on(host, None, name) {
        return Some(found);
    }
    let wb = wb?;
    wb.sheets.iter().zip(&wb.sheet_names).find_map(|(s, n)| on(s, Some(n), name))
}

/// Follow a sheet rename in GETPIVOTDATA calls that name their pivot as
/// `old!PivotTable1`. Returns `None` when nothing changed.
pub fn rename_sheet_in_pivot_names(formula: &str, old: &str, new: &str) -> Option<String> {
    map_getpivotdata_names(formula, |name| {
        name.match_indices('!')
            .find(|(i, _)| unquote_sheet(&name[..*i]).eq_ignore_ascii_case(old))
            .map(|(i, _)| format!("{}!{}", new, &name[i + 1..]))
    })
}

fn unquote_sheet(s: &str) -> String {
    s.trim().trim_matches('\'').replace("''", "'")
}

/// Rewrite the pivot names that GETPIVOTDATA calls in `formula` pass as a
/// string literal (their second argument; item names and other text are
/// left alone). `f` gets each name and returns its replacement, if any.
/// Returns `None` when nothing changed.
pub fn map_getpivotdata_names(formula: &str, mut f: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(formula.len());
    let mut last = 0;
    for (start, end) in pivot_name_literals(formula) {
        let name = formula[start..end].replace("\"\"", "\"");
        if let Some(new) = f(&name) {
            out.push_str(&formula[last..start]);
            out.push_str(&new.replace('"', "\"\""));
            last = end;
        }
    }
    if last == 0 {
        return None;
    }
    out.push_str(&formula[last..]);
    Some(out)
}

/// Byte ranges of the contents of every GETPIVOTDATA call's second
/// argument, when that argument is a string literal.
fn pivot_name_literals(formula: &str) -> Vec<(usize, usize)> {
    const NAME: &str = "GETPIVOTDATA";
    let b = formula.as_bytes();
    // Index of the closing quote of the literal opening at `open`.
    let close = |open: usize| -> Option<usize> {
        let mut i = open + 1;
        while i < b.len() {
            if b[i] == b'"' {
                if b.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                return Some(i);
            }
            i += 1;
        }
        None
    };
    let skip_ws = |mut i: usize| {
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'.';
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'"' {
            i = close(i).map_or(b.len(), |c| c + 1);
            continue;
        }
        let call = b.len() - i >= NAME.len()
            && b[i..i + NAME.len()].eq_ignore_ascii_case(NAME.as_bytes())
            && (i == 0 || !is_ident(b[i - 1]));
        if !call {
            i += 1;
            continue;
        }
        i += NAME.len();
        let mut j = skip_ws(i);
        if b.get(j) != Some(&b'(') {
            continue;
        }
        // Skip the first argument up to its top-level comma.
        j += 1;
        let mut depth = 0usize;
        while j < b.len() {
            match b[j] {
                b'"' => {
                    j = close(j).map_or(b.len(), |c| c + 1);
                    continue;
                }
                b'(' | b'{' => depth += 1,
                b')' | b'}' if depth == 0 => break,
                b')' | b'}' => depth -= 1,
                b',' if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        if b.get(j) != Some(&b',') {
            continue;
        }
        let open = skip_ws(j + 1);
        if b.get(open) == Some(&b'"')
            && let Some(c) = close(open)
        {
            spans.push((open + 1, c));
        }
        // Calls nested in the arguments are found as the scan goes on.
    }
    spans
}

/// Ordered child items for every prefix on one axis, applying each field's
/// sort and Top-N filter.
fn build_children(
    engine: &Engine,
    rows: bool,
    field_idx: &[usize],
    first_seen: &HashMap<(usize, String), usize>,
) -> HashMap<Path, Vec<String>> {
    let fields = if rows { &engine.spec.rows } else { &engine.spec.columns };
    let mut sets: HashMap<Path, Vec<String>> = HashMap::new();
    for (rp, cp) in engine.acc.keys() {
        let (path, other) = if rows { (rp, cp) } else { (cp, rp) };
        if !other.is_empty() || path.is_empty() {
            continue;
        }
        let parent = path[..path.len() - 1].to_vec();
        let list = sets.entry(parent).or_default();
        let label = path.last().unwrap().clone();
        if !list.contains(&label) {
            list.push(label);
        }
    }
    let total_of = |parent: &Path, label: &str, v: usize| -> Option<f64> {
        let mut p = parent.clone();
        p.push(label.to_string());
        if rows { engine.raw(&p, &[], v) } else { engine.raw(&[], &p, v) }
    };
    for (parent, items) in sets.iter_mut() {
        let level = parent.len();
        let Some(field) = fields.get(level) else { continue };
        let fi = field_idx[level];
        let by_value = |items: &mut Vec<String>, v: usize, desc: bool| {
            items.sort_by(|a, b| {
                let (x, y) = (total_of(parent, a, v), total_of(parent, b, v));
                let o = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
                if desc { o.reverse() } else { o }
            })
        };
        match &field.sort {
            PivotSort::Ascending => items.sort_by(|a, b| compare_labels(a, b)),
            PivotSort::Descending => items.sort_by(|a, b| compare_labels(b, a)),
            PivotSort::Manual => items.sort_by_key(|l| first_seen.get(&(fi, l.clone())).copied().unwrap_or(usize::MAX)),
            PivotSort::ByValue { value, descending } => by_value(items, *value, *descending),
        }
        if let Some(top) = &field.top_filter {
            let mut ranked = items.clone();
            by_value(&mut ranked, top.by_value, top.top);
            ranked.truncate(top.count);
            items.retain(|i| ranked.contains(i));
        }
    }
    sets
}

/// Lines (rows) or columns of one axis in display order.
fn axis_slots(
    children: &HashMap<Path, Vec<String>>,
    subtotals: &[bool],
    depth: usize,
    is_rows: bool,
    layout: PivotLayout,
    subtotals_at_top: bool,
    grand: bool,
) -> Vec<Slot> {
    fn walk(
        prefix: &Path,
        children: &HashMap<Path, Vec<String>>,
        subtotals: &[bool],
        depth: usize,
        top: bool,
        out: &mut Vec<Slot>,
    ) {
        let Some(items) = children.get(prefix) else { return };
        for label in items {
            let mut path = prefix.clone();
            path.push(label.clone());
            let parent = path.len() < depth;
            if !parent {
                out.push(Slot::Item { path, parent: false });
                continue;
            }
            if top {
                // The parent's own line carries its subtotal.
                out.push(Slot::Item { path: path.clone(), parent: true });
                walk(&path, children, subtotals, depth, top, out);
            } else {
                walk(&path, children, subtotals, depth, top, out);
                if subtotals[path.len() - 1] {
                    out.push(Slot::Subtotal { path });
                }
            }
        }
    }
    let mut out = Vec::new();
    if depth > 0 {
        // Rows in compact/outline form get a header line per parent item;
        // tabular rows and all columns put subtotals after the children.
        let header_lines = is_rows && layout != PivotLayout::Tabular;
        if header_lines && subtotals_at_top {
            walk(&Vec::new(), children, subtotals, depth, true, &mut out);
        } else if header_lines {
            // Parent header line (no values) + children + bottom subtotal.
            fn walk_bottom(prefix: &Path, children: &HashMap<Path, Vec<String>>, subtotals: &[bool], depth: usize, out: &mut Vec<Slot>) {
                let Some(items) = children.get(prefix) else { return };
                for label in items {
                    let mut path = prefix.clone();
                    path.push(label.clone());
                    if path.len() < depth {
                        out.push(Slot::Item { path: path.clone(), parent: true });
                        walk_bottom(&path, children, subtotals, depth, out);
                        if subtotals[path.len() - 1] {
                            out.push(Slot::Subtotal { path });
                        }
                    } else {
                        out.push(Slot::Item { path, parent: false });
                    }
                }
            }
            walk_bottom(&Vec::new(), children, subtotals, depth, &mut out);
        } else {
            walk(&Vec::new(), children, subtotals, depth, false, &mut out);
        }
    }
    if grand || depth == 0 {
        out.push(Slot::Grand);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{PivotField, PivotValue};

    fn data() -> PivotData {
        let rows = [
            ("North", "Jan", "100"),
            ("South", "Jan", "50"),
            ("North", "Feb", "200"),
            ("South", "Feb", "70"),
            ("West", "Jan", "30"),
        ];
        PivotData {
            fields: vec!["Region".into(), "Month".into(), "Revenue".into()],
            records: rows.iter().map(|(a, b, c)| vec![a.to_string(), b.to_string(), c.to_string()]).collect(),
        }
    }

    fn texts(out: &PivotOutput) -> Vec<Vec<String>> {
        out.table.iter().map(|r| r.iter().map(|c| c.text.clone()).collect()).collect()
    }

    fn spec(rows: &[&str], cols: &[&str], vals: &[(&str, Summarize)]) -> PivotSpec {
        let mut s = PivotSpec::new("PivotTable1", "A1:C6");
        s.rows = rows.iter().map(|f| PivotField::new(*f)).collect();
        s.columns = cols.iter().map(|f| PivotField::new(*f)).collect();
        s.values = vals.iter().map(|(f, how)| PivotValue::new(*f, *how)).collect();
        s
    }

    #[test]
    fn rows_only_sum_with_grand_total() {
        let out = compute_pivot(&spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]), &data()).unwrap();
        assert_eq!(
            texts(&out),
            vec![
                vec!["Row Labels", "Sum of Revenue"],
                vec!["North", "300"],
                vec!["South", "120"],
                vec!["West", "30"],
                vec!["Grand Total", "450"],
            ]
        );
        assert_eq!(out.chart.categories, vec!["North", "South", "West"]);
        assert_eq!(out.chart.series[0].1, vec![Some(300.0), Some(120.0), Some(30.0)]);
    }

    #[test]
    fn rows_and_columns_cross_tab() {
        let out = compute_pivot(&spec(&["Region"], &["Month"], &[("Revenue", Summarize::Sum)]), &data()).unwrap();
        assert_eq!(
            texts(&out),
            vec![
                vec!["Sum of Revenue", "Column Labels", "", ""],
                vec!["Row Labels", "Jan", "Feb", "Grand Total"],
                vec!["North", "100", "200", "300"],
                vec!["South", "50", "70", "120"],
                vec!["West", "30", "", "30"],
                vec!["Grand Total", "180", "270", "450"],
            ]
        );
        assert_eq!(out.chart.series.len(), 2);
        assert_eq!(out.chart.series[1].0, "Feb");
        assert_eq!(out.chart.series[1].1, vec![Some(200.0), Some(70.0), None]);
    }

    #[test]
    fn nested_rows_compact_subtotals_on_top() {
        let out = compute_pivot(&spec(&["Region", "Month"], &[], &[("Revenue", Summarize::Sum)]), &data()).unwrap();
        let t = texts(&out);
        assert_eq!(t[1], vec!["North", "300"]);
        assert_eq!(t[2], vec!["Jan", "100"]);
        assert_eq!(out.table[2][0].indent, 1);
        assert_eq!(t.last().unwrap(), &vec!["Grand Total", "450"]);
        assert_eq!(out.chart.categories[0], "North / Jan");
    }

    #[test]
    fn tabular_layout_puts_subtotals_below() {
        let mut s = spec(&["Region", "Month"], &[], &[("Revenue", Summarize::Sum)]);
        s.options.layout = PivotLayout::Tabular;
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!(t[0], vec!["Region", "Month", "Sum of Revenue"]);
        assert_eq!(t[1], vec!["North", "Jan", "100"]);
        assert_eq!(t[2], vec!["", "Feb", "200"]);
        assert_eq!(t[3], vec!["North Total", "", "300"]);
    }

    #[test]
    fn multiple_values_get_a_values_header_row() {
        let out = compute_pivot(
            &spec(&["Region"], &[], &[("Revenue", Summarize::Sum), ("Revenue", Summarize::Count)]),
            &data(),
        )
        .unwrap();
        let t = texts(&out);
        assert_eq!(t[0], vec!["Row Labels", "Sum of Revenue", "Count of Revenue"]);
        assert_eq!(t[1], vec!["North", "300", "2"]);
    }

    #[test]
    fn filters_hide_items() {
        let mut s = spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]);
        s.filters.push(PivotField { hidden_items: vec!["Feb".into()], ..PivotField::new("Month") });
        s.rows[0].hidden_items.push("West".into());
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!(t, vec![vec!["Row Labels", "Sum of Revenue"], vec!["North", "100"], vec!["South", "50"], vec!["Grand Total", "150"]]);
    }

    #[test]
    fn show_values_as_percent_of_grand_total_and_running_total() {
        let mut s = spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]);
        s.values[0].show_as = ShowValuesAs::PercentOfGrandTotal;
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!(t[1][1], "66.67%");
        assert_eq!(t[4][1], "100.00%");

        s.values[0].show_as = ShowValuesAs::RunningTotal;
        s.values[0].base_field = Some("Region".into());
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!((t[1][1].as_str(), t[2][1].as_str(), t[3][1].as_str()), ("300", "420", "450"));
    }

    #[test]
    fn difference_from_previous_and_rank() {
        let mut s = spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]);
        s.values[0].show_as = ShowValuesAs::DifferenceFrom;
        s.values[0].base_field = Some("Region".into());
        s.values[0].base_item = Some("(previous)".into());
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!((t[1][1].as_str(), t[2][1].as_str(), t[3][1].as_str()), ("", "-180", "-90"));

        s.values[0].show_as = ShowValuesAs::RankDescending;
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!((t[1][1].as_str(), t[2][1].as_str(), t[3][1].as_str()), ("1", "2", "3"));
    }

    #[test]
    fn sort_by_value_and_top_filter() {
        let mut s = spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]);
        s.rows[0].sort = PivotSort::ByValue { value: 0, descending: false };
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!((t[1][0].as_str(), t[3][0].as_str()), ("West", "North"));

        s.rows[0].sort = PivotSort::Ascending;
        s.rows[0].top_filter = Some(crate::domain::models::TopFilter { top: true, count: 2, by_value: 0 });
        let t = texts(&compute_pivot(&s, &data()).unwrap());
        assert_eq!(t.len(), 4, "{:?}", t);
        assert_eq!((t[1][0].as_str(), t[2][0].as_str()), ("North", "South"));
    }

    #[test]
    fn aggregations() {
        let d = data();
        let get = |how| {
            let t = texts(&compute_pivot(&spec(&[], &[], &[("Revenue", how)]), &d).unwrap());
            t[1][1].clone()
        };
        assert_eq!(get(Summarize::Average), "90");
        assert_eq!(get(Summarize::Max), "200");
        assert_eq!(get(Summarize::Min), "30");
        assert_eq!(get(Summarize::Count), "5");
        assert_eq!(get(Summarize::Product), "2100000000");
        assert_eq!(get(Summarize::VarP), "3560");
    }

    #[test]
    fn months_and_weekdays_sort_in_calendar_order() {
        let mut v = vec!["Mar", "Jan", "Dec", "Feb"];
        v.sort_by(|a, b| compare_labels(a, b));
        assert_eq!(v, vec!["Jan", "Feb", "Mar", "Dec"]);
        let mut v = vec!["Freitag", "Montag", "Sonntag"];
        v.sort_by(|a, b| compare_labels(a, b));
        assert_eq!(v, vec!["Montag", "Freitag", "Sonntag"]);
        let mut v = vec!["10", "9", "apple", "Banana"];
        v.sort_by(|a, b| compare_labels(a, b));
        assert_eq!(v, vec!["9", "10", "apple", "Banana"]);
    }

    #[test]
    fn missing_field_is_reported() {
        let err = compute_pivot(&spec(&["Nope"], &[], &[]), &data()).unwrap_err();
        assert!(err.contains("Nope"));
    }

    fn lookup(s: &PivotSpec, value: &str, items: &[(&str, &str)]) -> Result<f64, String> {
        let items: Vec<(String, String)> = items.iter().map(|(f, i)| (f.to_string(), i.to_string())).collect();
        get_pivot_data(s, &data(), value, &items)
    }

    #[test]
    fn get_pivot_data_reads_items_totals_and_cross_tabs() {
        let s = spec(&["Region"], &["Month"], &[("Revenue", Summarize::Sum)]);
        assert_eq!(lookup(&s, "Sum of Revenue", &[("Region", "North")]), Ok(300.0));
        assert_eq!(lookup(&s, "sum of revenue", &[("region", "north"), ("Month", "Feb")]), Ok(200.0));
        assert_eq!(lookup(&s, "Revenue", &[("Month", "Jan")]), Ok(180.0), "source field name works too");
        assert_eq!(lookup(&s, "Sum of Revenue", &[]), Ok(450.0), "grand total");
        assert!(lookup(&s, "Sum of Revenue", &[("Region", "West"), ("Month", "Feb")]).is_err(), "empty cell");
        assert!(lookup(&s, "Sum of Revenue", &[("Region", "East")]).is_err());
        assert!(lookup(&s, "Count of Revenue", &[]).is_err());
        assert!(lookup(&s, "Sum of Revenue", &[("Revenue", "1")]).is_err(), "not a row/column field");
    }

    #[test]
    fn get_pivot_data_follows_what_the_pivot_shows() {
        let mut s = spec(&["Region", "Month"], &[], &[("Revenue", Summarize::Sum)]);
        assert_eq!(lookup(&s, "Sum of Revenue", &[("Region", "South")]), Ok(120.0), "subtotal");
        assert_eq!(lookup(&s, "Sum of Revenue", &[("Region", "West"), ("Month", "Jan")]), Ok(30.0));
        // "Jan" alone is ambiguous: it appears under several regions.
        assert!(lookup(&s, "Sum of Revenue", &[("Month", "Jan")]).is_err());
        assert_eq!(lookup(&s, "Sum of Revenue", &[("Month", "Feb"), ("Region", "South")]), Ok(70.0));

        s.rows[0].subtotals = false;
        assert!(lookup(&s, "Sum of Revenue", &[("Region", "South")]).is_err(), "subtotal switched off");
        s.options.grand_totals_columns = false;
        assert!(lookup(&s, "Sum of Revenue", &[]).is_err(), "grand total switched off");
        s.rows[0].hidden_items = vec!["North".into()];
        assert!(lookup(&s, "Sum of Revenue", &[("Region", "North"), ("Month", "Jan")]).is_err(), "hidden item");

        let mut s = spec(&["Region"], &[], &[("Revenue", Summarize::Sum)]);
        s.values[0].show_as = ShowValuesAs::PercentOfGrandTotal;
        assert_eq!(lookup(&s, "Sum of Revenue", &[("Region", "West")]), Ok(30.0 / 450.0), "shown value");
    }

    #[test]
    fn value_cells_carry_their_getpivotdata_formula() {
        let s = spec(&["Region"], &["Month"], &[("Revenue", Summarize::Sum)]);
        let out = compute_pivot(&s, &data()).unwrap();
        let formulas: Vec<String> = out
            .table
            .iter()
            .flatten()
            .filter_map(|c| c.reference.as_ref().map(|r| r.formula(&s, None)))
            .collect();
        assert!(formulas.contains(
            &r#"GETPIVOTDATA("Sum of Revenue","PivotTable1","Region","North","Month","Feb")"#.to_string()
        ));
        assert!(formulas.contains(&r#"GETPIVOTDATA("Sum of Revenue","PivotTable1")"#.to_string()));
        // Every formula reproduces the value it was taken from.
        for cell in out.table.iter().flatten() {
            let Some(r) = &cell.reference else { continue };
            let v = s.values[r.value].display_name();
            assert_eq!(get_pivot_data(&s, &data(), &v, &r.items).ok(), cell.number);
        }
        let numeric = PivotRef { value: 0, items: vec![("Year".into(), "2024".into()), ("Code".into(), "007".into())] };
        assert_eq!(numeric.formula(&s, None), r#"GETPIVOTDATA("Sum of Revenue","PivotTable1","Year",2024,"Code",007)"#);
        let grand = PivotRef { value: 0, items: Vec::new() };
        assert_eq!(grand.formula(&s, Some("Q1 Data")), r#"GETPIVOTDATA("Sum of Revenue","Q1 Data!PivotTable1")"#);
        assert!(item_matches("007", "7"));
    }

    #[test]
    fn only_getpivotdata_pivot_names_are_rewritten() {
        let upper = |n: &str| Some(n.to_uppercase());
        // Item names, other functions' text and the value field stay.
        assert_eq!(
            map_getpivotdata_names(r#"=IF(A1="p",GETPIVOTDATA("p", "p" ,"p","p"),"p")"#, upper).as_deref(),
            Some(r#"=IF(A1="p",GETPIVOTDATA("p", "P" ,"p","p"),"p")"#)
        );
        // A nested call in an argument, a computed first argument, escapes.
        assert_eq!(
            map_getpivotdata_names(
                r#"=getpivotdata(CONCAT("a",",b"),"x",F,GETPIVOTDATA("v","y""z"))"#,
                upper
            )
            .as_deref(),
            Some(r#"=getpivotdata(CONCAT("a",",b"),"X",F,GETPIVOTDATA("v","Y""Z"))"#)
        );
        // A name from a cell isn't a literal; MYGETPIVOTDATA isn't the call.
        assert_eq!(map_getpivotdata_names(r#"=GETPIVOTDATA("v",A1)"#, upper), None);
        assert_eq!(map_getpivotdata_names(r#"=XGETPIVOTDATA("v","x")"#, upper), None);
        assert_eq!(map_getpivotdata_names(r#"=GETPIVOTDATA("v","x"#, upper), None, "unterminated");

        assert_eq!(
            rename_sheet_in_pivot_names(r#"=GETPIVOTDATA("Sum of X","'Old Data'!P","Region","Old Data!P")"#, "old data", "New")
                .as_deref(),
            Some(r#"=GETPIVOTDATA("Sum of X","New!P","Region","Old Data!P")"#)
        );
        let sheets = vec!["a!b".to_string(), "Data".to_string()];
        assert_eq!(split_pivot_name(&sheets, "a!b!Sales!Q1"), Some((0, "Sales!Q1")));
        assert_eq!(split_pivot_name(&sheets, "'data'!P"), Some((1, "P")));
        assert_eq!(split_pivot_name(&sheets, "Sales!Q1"), None);
    }

    #[test]
    fn reads_source_with_duplicate_and_blank_headers() {
        let mut wb = Workbook::default();
        let sheet = wb.current_sheet_mut();
        for (c, h) in ["A", "A", ""].iter().enumerate() {
            sheet.cells.insert((0, c), crate::domain::CellData { value: h.to_string(), ..Default::default() });
        }
        sheet.cells.insert((1, 0), crate::domain::CellData { value: "x".into(), ..Default::default() });
        let d = PivotData::read(&wb, 0, "A1:C3").unwrap();
        assert_eq!(d.fields, vec!["A", "A2", "Column C"]);
        assert_eq!(d.records.len(), 1, "blank row 3 is skipped");
    }
}
