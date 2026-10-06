//! The parts of an `.xlsx` package that `calamine` doesn't expose: cell
//! formatting (number formats, bold/underline, font and fill colors),
//! column widths, charts, and pivot-table locations.
//!
//! Everything here is best effort. A malformed or unusual part is skipped
//! rather than failing the import, because the cell values and formulas
//! (read by calamine) are what matter; formatting is a bonus.

use std::collections::HashMap;
use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::xlsx_pivot::{parse_pivot, ImportedPivot};
use crate::domain::{
    CellFormat, CellStyle, ChartSeries, ChartSpec, ChartType, NumberFormat, Spreadsheet,
    TerminalColor,
};

/// Formatting and charts for one worksheet.
#[derive(Debug, Default)]
pub struct SheetExtras {
    /// Format per cell, keyed by (row, col). Only non-default formats.
    pub formats: HashMap<(usize, usize), CellFormat>,
    /// Column widths in characters, keyed by column index.
    pub column_widths: HashMap<usize, usize>,
    /// Custom row heights in points, keyed by row index.
    pub row_heights: HashMap<usize, usize>,
    pub charts: Vec<ChartSpec>,
    /// Pivot tables on this sheet: converted, or (name, reason) when they
    /// couldn't be.
    pub pivots: Vec<Result<ImportedPivot, (String, String)>>,
}

/// Read formatting, widths, charts and pivot names for every sheet,
/// keyed by sheet name.
pub fn read_extras(path: &str) -> Result<HashMap<String, SheetExtras>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("xlsx zip: {}", e))?;

    let workbook_xml = read_part(&mut zip, "xl/workbook.xml").ok_or("xlsx: missing workbook.xml")?;
    let workbook_rels = read_rels(&mut zip, "xl/workbook.xml");
    let xfs = read_part(&mut zip, "xl/styles.xml")
        .map(|xml| parse_styles(&xml))
        .unwrap_or_default();

    // Pivot sources may be a defined name or an Excel table name.
    let mut names = parse_defined_names(&workbook_xml);
    let sheets = parse_sheet_list(&workbook_xml);
    for (sheet_name, rid) in &sheets {
        let Some(sheet_path) = workbook_rels.get(rid) else { continue };
        for (target, rel_type) in read_rels_typed(&mut zip, sheet_path) {
            if rel_type.ends_with("/table")
                && let Some(xml) = read_part(&mut zip, &target)
                && let Some((table, range)) = parse_table(&xml)
            {
                names.insert(table, format!("{}!{}", quote_sheet(sheet_name), range));
            }
        }
    }

    let mut out = HashMap::new();
    for (name, rid) in sheets {
        let Some(sheet_path) = workbook_rels.get(&rid) else { continue };
        let Some(sheet_xml) = read_part(&mut zip, sheet_path) else { continue };
        let mut extras = SheetExtras::default();
        parse_sheet(&sheet_xml, &xfs, &mut extras);

        // Charts hang off the sheet via sheet → drawing → chart rels.
        // Pivot tables are related to the sheet directly.
        let sheet_rels = read_rels_typed(&mut zip, sheet_path);
        for (target, rel_type) in &sheet_rels {
            if rel_type.ends_with("/drawing") {
                for (chart_path, chart_type) in read_rels_typed(&mut zip, target) {
                    if !chart_type.ends_with("/chart") {
                        continue;
                    }
                    if let Some(xml) = read_part(&mut zip, &chart_path)
                        && let Some(chart) = parse_chart(&xml, &name)
                    {
                        extras.charts.push(chart);
                    }
                }
            } else if rel_type.ends_with("/pivotTable")
                && let Some(def) = read_part(&mut zip, target)
            {
                let cache = read_rels_typed(&mut zip, target)
                    .into_iter()
                    .find(|(_, t)| t.ends_with("/pivotCacheDefinition"))
                    .and_then(|(path, _)| read_part(&mut zip, &path));
                let pivot_name = first_attr(&def, b"pivotTableDefinition", b"name").unwrap_or_else(|| "PivotTable".into());
                let resolve = |n: &str| names.get(n).cloned();
                extras.pivots.push(match cache {
                    Some(cache) => parse_pivot(&def, &cache, &name, &resolve).map_err(|e| (pivot_name, e)),
                    None => Err((pivot_name, "its pivot cache is missing".into())),
                });
            }
        }
        out.insert(name, extras);
    }
    Ok(out)
}

// ---------------------------------------------------------------- package

fn read_part<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, path: &str) -> Option<String> {
    let mut f = zip.by_name(path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Relationship id → resolved part path for `part`.
fn read_rels<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    part: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(xml) = read_part(zip, &rels_path(part)) else { return map };
    let mut reader = Reader::from_str(&xml);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if let (Some(id), Some(target)) = (attr(&e, b"Id"), attr(&e, b"Target")) {
                    map.insert(id, resolve(part, &target));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    map
}

/// (resolved target path, relationship type) pairs for `part`.
fn read_rels_typed<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    part: &str,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(xml) = read_part(zip, &rels_path(part)) else { return out };
    let mut reader = Reader::from_str(&xml);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if let (Some(target), Some(ty)) = (attr(&e, b"Target"), attr(&e, b"Type")) {
                    if attr(&e, b"TargetMode").as_deref() == Some("External") {
                        continue;
                    }
                    out.push((resolve(part, &target), ty));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// `xl/worksheets/sheet1.xml` → `xl/worksheets/_rels/sheet1.xml.rels`.
fn rels_path(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{}/_rels/{}.rels", dir, file),
        None => format!("_rels/{}.rels", part),
    }
}

/// Resolve a relationship target against the directory of `part`.
fn resolve(part: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segs: Vec<&str> = part.split('/').collect();
    segs.pop(); // the part's own file name
    for seg in target.split('/') {
        match seg {
            ".." => {
                segs.pop();
            }
            "." | "" => {}
            s => segs.push(s),
        }
    }
    segs.join("/")
}

/// Sheet names with their relationship ids, in workbook order.
fn parse_sheet_list(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut reader = Reader::from_str(xml);
    while let Ok(ev) = reader.read_event() {
        match ev {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                // The id attribute is namespaced (`r:id`); match on local name.
                let rid = e.attributes().flatten().find_map(|a| {
                    (a.key.local_name().as_ref() == b"id").then(|| unescape(&a.value))
                });
                if let (Some(name), Some(rid)) = (attr(&e, b"name"), rid) {
                    out.push((name, rid));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// `<definedName name="X">Sheet1!$A$1:$C$9</definedName>` → X → range.
fn parse_defined_names(xml: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"definedName" => current = attr(&e, b"name"),
            Ok(Event::Text(t)) => {
                if let Some(n) = current.take() {
                    out.insert(n, unescape(t.as_ref()).replace('$', ""));
                }
            }
            Ok(Event::End(_)) => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Excel table part → (table name, ref).
fn parse_table(xml: &str) -> Option<(String, String)> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"table" => {
                return Some((attr(&e, b"name").or_else(|| attr(&e, b"displayName"))?, attr(&e, b"ref")?));
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// First `attr` of the first `element` in `xml`.
fn first_attr(xml: &str, element: &[u8], key: &[u8]) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == element => return attr(&e, key),
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn quote_sheet(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

// ----------------------------------------------------------------- styles

/// Resolved `cellXfs` entries, indexed by a cell's `s` attribute.
/// `None` means "default format" (nothing worth storing).
fn parse_styles(xml: &str) -> Vec<Option<CellFormat>> {
    #[derive(Default, Clone)]
    struct Font {
        bold: bool,
        underline: bool,
        color: Option<TerminalColor>,
    }
    let mut num_fmts: HashMap<u32, String> = HashMap::new();
    let mut fonts: Vec<Font> = Vec::new();
    let mut fills: Vec<Option<TerminalColor>> = Vec::new();
    let mut xf_raw: Vec<(u32, usize, usize)> = Vec::new();

    let mut section = Vec::<Vec<u8>>::new();
    let mut in_font = false;
    let mut in_fill = false;
    let mut reader = Reader::from_str(xml);
    loop {
        let ev = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(ev) => ev,
        };
        let (e, is_empty) = match &ev {
            Event::Start(e) => (e.clone(), false),
            Event::Empty(e) => (e.clone(), true),
            Event::End(e) => {
                match e.local_name().as_ref() {
                    b"font" => in_font = false,
                    b"fill" => in_fill = false,
                    _ => {}
                }
                section.pop();
                continue;
            }
            _ => continue,
        };
        let name = e.local_name().as_ref().to_vec();
        let parent = section.last().map(|v| v.as_slice());
        match name.as_slice() {
            b"numFmt" => {
                if let (Some(id), Some(code)) = (attr(&e, b"numFmtId"), attr(&e, b"formatCode"))
                    && let Ok(id) = id.parse()
                {
                    num_fmts.insert(id, code);
                }
            }
            b"font" if parent == Some(b"fonts") => {
                fonts.push(Font::default());
                in_font = !is_empty;
            }
            b"b" if in_font => {
                if let Some(f) = fonts.last_mut() {
                    f.bold = attr_true(&e);
                }
            }
            b"u" if in_font => {
                if let Some(f) = fonts.last_mut() {
                    f.underline = attr(&e, b"val").as_deref() != Some("none");
                }
            }
            b"color" if in_font => {
                if let Some(f) = fonts.last_mut() {
                    // Black / near-black is the default text color.
                    f.color = rgb_attr(&e).filter(|c| *c != TerminalColor::Black);
                }
            }
            b"fill" if parent == Some(b"fills") => {
                fills.push(None);
                in_fill = !is_empty;
            }
            b"fgColor" if in_fill => {
                if let Some(f) = fills.last_mut() {
                    *f = rgb_attr(&e).filter(|c| *c != TerminalColor::White);
                }
            }
            b"xf" if parent == Some(b"cellXfs") => {
                let num = |k: &[u8]| attr(&e, k).and_then(|v| v.parse().ok()).unwrap_or(0);
                xf_raw.push((num(b"numFmtId"), num(b"fontId") as usize, num(b"fillId") as usize));
            }
            _ => {}
        }
        if !is_empty {
            section.push(name);
        }
    }

    xf_raw
        .into_iter()
        .map(|(num_fmt_id, font_id, fill_id)| {
            let font = fonts.get(font_id).cloned().unwrap_or_default();
            let fmt = CellFormat {
                number_format: number_format(num_fmt_id, num_fmts.get(&num_fmt_id)),
                style: CellStyle {
                    bold: font.bold,
                    underline: font.underline,
                    fg_color: font.color,
                    bg_color: fills.get(fill_id).cloned().flatten(),
                },
            };
            (fmt != CellFormat::default()).then_some(fmt)
        })
        .collect()
}

/// Map an Excel number format onto Cellar's simpler model. Date and time
/// formats map to General: the importer already renders date cells as
/// ISO strings.
fn number_format(id: u32, custom: Option<&String>) -> NumberFormat {
    let code = match (id, custom) {
        (_, Some(code)) => code.as_str(),
        (1, _) => "0",
        (2, _) => "0.00",
        (3, _) => "#,##0",
        (4, _) => "#,##0.00",
        (5 | 6, _) => "$#,##0",
        (7 | 8, _) => "$#,##0.00",
        (9, _) => "0%",
        (10, _) => "0.00%",
        (37 | 38, _) => "#,##0",
        (39 | 40, _) => "#,##0.00",
        (44, _) => "$#,##0.00",
        _ => return NumberFormat::General,
    };
    // Only the first section (positive numbers) matters for our model.
    // Strip quoted literals and bracketed parts so `[Red]` or `"kg"` don't
    // look like date letters.
    let section = code.split(';').next().unwrap_or("");
    let mut bare = String::new();
    let mut symbol = None;
    let mut chars = section.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let lit: String = chars.by_ref().take_while(|&c| c != '"').collect();
                if symbol.is_none() && lit.chars().any(is_currency) {
                    symbol = Some(lit.trim().to_string());
                }
            }
            '[' => {
                // `[$€-407]` carries a currency symbol; `[Red]` is a color.
                let inner: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if let Some(rest) = inner.strip_prefix('$') {
                    let sym = rest.split('-').next().unwrap_or("").to_string();
                    if !sym.is_empty() && symbol.is_none() {
                        symbol = Some(sym);
                    }
                }
            }
            '\\' => {
                if let Some(next) = chars.next()
                    && symbol.is_none()
                    && is_currency(next)
                {
                    symbol = Some(next.to_string());
                }
            }
            c if is_currency(c) => {
                if symbol.is_none() {
                    symbol = Some(c.to_string());
                }
            }
            c => bare.push(c),
        }
    }
    let lower = bare.to_lowercase();
    if lower.contains(['y', 'd', 'h', 's']) || (lower.contains('m') && !lower.contains('0')) {
        return NumberFormat::General;
    }
    let decimals = bare
        .split_once('.')
        .map(|(_, frac)| frac.chars().take_while(|c| *c == '0' || *c == '#').count() as u32)
        .unwrap_or(0);
    if bare.contains('%') {
        NumberFormat::Percentage { decimals }
    } else if let Some(symbol) = symbol {
        NumberFormat::Currency { symbol, decimals }
    } else if bare.contains('0') || bare.contains('#') {
        NumberFormat::Number { decimals, thousands_sep: bare.contains(',') }
    } else {
        NumberFormat::General
    }
}

fn is_currency(c: char) -> bool {
    matches!(c, '$' | '€' | '£' | '¥' | '₹' | '₽' | '₩' | '₺' | '₣')
}

fn rgb_attr(e: &BytesStart) -> Option<TerminalColor> {
    let hex = attr(e, b"rgb")?;
    // ARGB ("FFRRGGBB") or RGB.
    // Checked first: byte slicing below would panic inside a multi-byte
    // character of a malformed attribute.
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let hex = if hex.len() == 8 { &hex[2..] } else { hex.as_str() };
    if hex.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(TerminalColor::nearest(p(0)?, p(2)?, p(4)?))
}

// ------------------------------------------------------------------ sheet

fn parse_sheet(xml: &str, xfs: &[Option<CellFormat>], out: &mut SheetExtras) {
    let mut reader = Reader::from_str(xml);
    loop {
        let e = match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => e,
            Ok(Event::Eof) | Err(_) => break,
            _ => continue,
        };
        match e.local_name().as_ref() {
            b"col" => {
                let num = |k: &[u8]| attr(&e, k).and_then(|v| v.parse::<f64>().ok());
                if let (Some(min), Some(max), Some(width)) = (num(b"min"), num(b"max"), num(b"width")) {
                    // Excel writes one <col> spanning to column 16384 for
                    // "all remaining columns". That's a sheet default, not
                    // a per-column width; storing it would add hundreds of
                    // noise entries to the .cellar file.
                    let (min, max) = (min as usize, max as usize);
                    if min >= 1 && max >= min && max - min < 256 && attr(&e, b"hidden").as_deref() != Some("1") {
                        let chars = width.round().max(3.0) as usize;
                        for c in min..=max {
                            out.column_widths.insert(c - 1, chars);
                        }
                    }
                }
            }
            b"row" => {
                let row = attr(&e, b"r").and_then(|v| v.parse::<usize>().ok());
                let ht = attr(&e, b"ht").and_then(|v| v.parse::<f64>().ok());
                if attr(&e, b"customHeight").as_deref() == Some("1")
                    && let (Some(row), Some(ht)) = (row, ht)
                    && row >= 1
                    && ht.is_finite()
                {
                    let pt = (ht.round() as usize).clamp(1, Spreadsheet::MAX_ROW_HEIGHT);
                    if pt != Spreadsheet::DEFAULT_ROW_HEIGHT {
                        out.row_heights.insert(row - 1, pt);
                    }
                }
            }
            b"c" => {
                let fmt = attr(&e, b"s")
                    .and_then(|s| s.parse::<usize>().ok())
                    .and_then(|s| xfs.get(s).cloned().flatten());
                if let (Some(fmt), Some(r)) = (fmt, attr(&e, b"r"))
                    && let Some(pos) = Spreadsheet::parse_cell_reference(&r)
                {
                    out.formats.insert(pos, fmt);
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ chart

fn parse_chart(xml: &str, host_sheet: &str) -> Option<ChartSpec> {
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut chart_type: Option<ChartType> = None;
    let mut title = String::new();
    let mut series: Vec<ChartSeries> = Vec::new();
    let mut categories: Option<String> = None;
    // PivotChart: `<c:pivotSource><c:name>[Book.xlsx]Sheet!PivotTable1`.
    let mut pivot: Option<String> = None;
    // Per-series: which child we're in and the latest formula/cached text.
    let mut text = String::new();

    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_vec();
                if chart_type.is_none() {
                    chart_type = chart_kind(&name);
                }
                if name == b"ser" {
                    series.push(ChartSeries { name: None, values: String::new() });
                }
                stack.push(name);
                text.clear();
            }
            Ok(Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_vec();
                if chart_type.is_none() {
                    chart_type = chart_kind(&name);
                }
                // A bar chart with barDir="bar" is horizontal; we render
                // both directions as columns.
            }
            Ok(Event::Text(t)) => text.push_str(&unescape(t.as_ref())),
            Ok(Event::GeneralRef(r)) => text.push_str(&entity(r.as_ref())),
            Ok(Event::End(_)) => {
                let Some(name) = stack.pop() else { continue };
                let has = |n: &[u8]| stack.iter().any(|s| s.as_slice() == n);
                match name.as_slice() {
                    // Chart title: <chart><title>…<a:t>text</a:t>. Axis
                    // titles sit under <valAx>/<catAx>, so require the
                    // title's parent to be <chart>.
                    b"t" if has(b"title") && !has(b"ser") => {
                        let pos = stack.iter().rposition(|s| s.as_slice() == b"title");
                        if pos.is_some_and(|p| p > 0 && stack[p - 1].as_slice() == b"chart") {
                            title.push_str(&text);
                        }
                    }
                    b"name" if has(b"pivotSource") => {
                        let n = text.rsplit('!').next().unwrap_or(&text).trim().to_string();
                        if !n.is_empty() {
                            pivot = Some(n);
                        }
                    }
                    b"f" if has(b"ser") => {
                        let r = clean_ref(&text, host_sheet);
                        if let Some(s) = series.last_mut() {
                            if has(b"tx") {
                                s.name.get_or_insert(r);
                            } else if has(b"val") || has(b"yVal") {
                                s.values = r;
                            } else if (has(b"cat") || has(b"xVal")) && categories.is_none() {
                                categories = Some(r);
                            }
                        }
                    }
                    // Cached series name text beats the cell reference.
                    b"v" if has(b"ser") && has(b"tx") => {
                        if let Some(s) = series.last_mut() {
                            s.name = Some(text.clone());
                        }
                    }
                    _ => {}
                }
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    series.retain(|s| !s.values.is_empty());
    if let Some(p) = pivot {
        // The pivot supplies the data; the cached series ranges point at
        // its old static output and are dropped.
        if title.is_empty() {
            title = p.clone();
        }
        return Some(ChartSpec { title, chart_type: chart_type.unwrap_or_default(), pivot: Some(p), categories: None, series: Vec::new() });
    }
    if series.is_empty() {
        return None;
    }
    if title.is_empty() {
        title = series
            .first()
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| "Chart".to_string());
    }
    Some(ChartSpec { title, chart_type: chart_type.unwrap_or_default(), pivot: None, categories, series })
}

fn chart_kind(name: &[u8]) -> Option<ChartType> {
    match name {
        b"barChart" | b"bar3DChart" => Some(ChartType::Bar),
        b"lineChart" | b"line3DChart" | b"areaChart" | b"area3DChart" | b"stockChart"
        | b"radarChart" => Some(ChartType::Line),
        b"pieChart" | b"pie3DChart" | b"doughnutChart" | b"ofPieChart" => Some(ChartType::Pie),
        b"scatterChart" | b"bubbleChart" => Some(ChartType::Scatter),
        _ => None,
    }
}

/// `Sheet1!$B$2:$B$5` → `B2:B5` when `Sheet1` is the host sheet, else
/// `Sheet1!B2:B5`. Quoted sheet names keep their quotes.
fn clean_ref(r: &str, host_sheet: &str) -> String {
    let r = r.trim().trim_start_matches('(').trim_end_matches(')').replace('$', "");
    match r.rsplit_once('!') {
        Some((sheet, range)) => {
            let bare = sheet.trim_matches('\'').replace("''", "'");
            if bare == host_sheet { range.to_string() } else { r.clone() }
        }
        None => r,
    }
}

// -------------------------------------------------------------------- xml

fn attr(e: &BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| unescape(&a.value))
}

/// `<b/>`, `<b val="1"/>`, `<b val="true"/>` are on; `val="0"` is off.
fn attr_true(e: &BytesStart) -> bool {
    !matches!(attr(e, b"val").as_deref(), Some("0") | Some("false"))
}

pub(super) fn unescape_pub(raw: &[u8]) -> String {
    unescape(raw)
}

fn unescape(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    if !s.contains('&') {
        return s.into_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_ref();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        match rest[amp + 1..].find(';') {
            Some(semi) => {
                out.push_str(&entity(&rest.as_bytes()[amp + 1..amp + 1 + semi]));
                rest = &rest[amp + semi + 2..];
            }
            None => {
                out.push_str(&rest[amp..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    match name.as_ref() {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        n => n
            .strip_prefix("#x")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_else(|| format!("&{};", n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats_map_to_cellar_model() {
        let f = |code: &str| number_format(164, Some(&code.to_string()));
        assert_eq!(f("0.00%"), NumberFormat::Percentage { decimals: 2 });
        assert_eq!(f("#,##0.00"), NumberFormat::Number { decimals: 2, thousands_sep: true });
        assert_eq!(f("0"), NumberFormat::Number { decimals: 0, thousands_sep: false });
        assert_eq!(
            f("[$€-407] #,##0.00;[Red]-#,##0.00"),
            NumberFormat::Currency { symbol: "€".into(), decimals: 2 }
        );
        assert_eq!(f("\"$\"#,##0"), NumberFormat::Currency { symbol: "$".into(), decimals: 0 });
        assert_eq!(f("yyyy-mm-dd"), NumberFormat::General);
        assert_eq!(f("h:mm"), NumberFormat::General);
        assert_eq!(number_format(4, None), NumberFormat::Number { decimals: 2, thousands_sep: true });
        assert_eq!(number_format(14, None), NumberFormat::General);
    }

    #[test]
    fn styles_resolve_fonts_and_fills() {
        let xml = r#"<styleSheet>
          <numFmts count="1"><numFmt numFmtId="164" formatCode="0.0%"/></numFmts>
          <fonts count="2">
            <font><sz val="11"/><color theme="1"/></font>
            <font><b/><u/><color rgb="FFFF0000"/></font>
          </fonts>
          <fills count="3">
            <fill><patternFill patternType="none"/></fill>
            <fill><patternFill patternType="gray125"/></fill>
            <fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill>
          </fills>
          <cellStyleXfs count="1"><xf numFmtId="0" fontId="1" fillId="2"/></cellStyleXfs>
          <cellXfs count="3">
            <xf numFmtId="0" fontId="0" fillId="0"/>
            <xf numFmtId="164" fontId="1" fillId="2" applyFont="1"/>
            <xf numFmtId="3" fontId="0" fillId="0"/>
          </cellXfs>
        </styleSheet>"#;
        let xfs = parse_styles(xml);
        assert_eq!(xfs.len(), 3, "cellStyleXfs must not leak into cellXfs");
        assert!(xfs[0].is_none());
        let f = xfs[1].as_ref().unwrap();
        assert_eq!(f.number_format, NumberFormat::Percentage { decimals: 1 });
        assert!(f.style.bold && f.style.underline);
        assert_eq!(f.style.fg_color, Some(TerminalColor::Red));
        assert_eq!(f.style.bg_color, Some(TerminalColor::Yellow));
        assert_eq!(
            xfs[2].as_ref().unwrap().number_format,
            NumberFormat::Number { decimals: 0, thousands_sep: true }
        );
    }

    #[test]
    fn sheet_reads_widths_and_cell_styles() {
        let xfs = vec![None, Some(CellFormat { style: CellStyle { bold: true, ..Default::default() }, ..Default::default() })];
        let xml = r#"<worksheet><cols><col min="1" max="2" width="15.7" customWidth="1"/>
            <col min="3" max="16384" width="9"/></cols>
            <sheetData><row r="1" ht="30" customHeight="1"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1" s="0"/></row>
            <row r="2" ht="15.75"/></sheetData></worksheet>"#;
        let mut out = SheetExtras::default();
        parse_sheet(xml, &xfs, &mut out);
        assert_eq!(out.column_widths.get(&0), Some(&16));
        assert_eq!(out.column_widths.get(&1), Some(&16));
        assert_eq!(out.column_widths.len(), 2, "the to-16384 default span is skipped");
        assert_eq!(out.row_heights, HashMap::from([(0, 30)]), "only custom heights are read");
        assert!(out.formats.get(&(0, 0)).unwrap().style.bold);
        assert!(!out.formats.contains_key(&(0, 1)));
    }

    #[test]
    fn chart_definition_is_extracted() {
        let xml = r#"<c:chartSpace xmlns:c="c" xmlns:a="a"><c:chart>
          <c:title><c:tx><c:rich><a:p><a:r><a:t>Sales &amp; Costs</a:t></a:r></a:p></c:rich></c:tx></c:title>
          <c:plotArea><c:lineChart><c:grouping val="standard"/>
            <c:ser><c:tx><c:strRef><c:f>Data!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>Revenue</c:v></c:pt></c:strCache></c:strRef></c:tx>
              <c:cat><c:strRef><c:f>Data!$A$2:$A$5</c:f></c:strRef></c:cat>
              <c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>
            <c:ser><c:val><c:numRef><c:f>'Other Sheet'!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>
          </c:lineChart>
          <c:valAx><c:title><c:tx><c:rich><a:p><a:r><a:t>EUR</a:t></a:r></a:p></c:rich></c:tx></c:title></c:valAx>
          </c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart(xml, "Data").unwrap();
        assert_eq!(c.title, "Sales & Costs");
        assert_eq!(c.chart_type, ChartType::Line);
        assert_eq!(c.categories.as_deref(), Some("A2:A5"));
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[0].name.as_deref(), Some("Revenue"));
        assert_eq!(c.series[0].values, "B2:B5");
        assert_eq!(c.series[1].values, "'Other Sheet'!C2:C5");
    }

    #[test]
    fn resolves_relative_targets() {
        assert_eq!(resolve("xl/workbook.xml", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("xl/worksheets/sheet1.xml", "../drawings/drawing1.xml"), "xl/drawings/drawing1.xml");
        assert_eq!(resolve("xl/workbook.xml", "/xl/worksheets/sheet2.xml"), "xl/worksheets/sheet2.xml");
        assert_eq!(rels_path("xl/worksheets/sheet1.xml"), "xl/worksheets/_rels/sheet1.xml.rels");
    }
}
