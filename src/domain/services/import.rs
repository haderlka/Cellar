//! Text table formats → sheets: delimited text (CSV, TSV, semicolon- or
//! pipe-separated), Markdown pipe tables and JSON / JSON Lines. Pure
//! parsing; reading the file and picking the format happen in
//! `infrastructure::import`.
//!
//! Every value is imported as text, never as a formula: imported files are
//! untrusted, and a leading `=` would otherwise run on load (for example
//! `GET()`). The user can turn a cell into a formula in the formula bar.

use crate::domain::models::{CellData, Spreadsheet};

/// A parsed table and, when the source names it, its sheet name.
pub type NamedSheet = (Option<String>, Spreadsheet);

/// Decode a text file: UTF-8 (with or without BOM), UTF-16 with BOM (Excel's
/// "Unicode Text"), and anything else as Windows-1252/Latin-1, which is what
/// older Excel versions write for "CSV" on Windows.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&p| if le { u16::from_le_bytes(p) } else { u16::from_be_bytes(p) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

/// Windows-1252 byte → char (Latin-1 except for 0x80–0x9F).
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Guess the field separator from the first lines: the candidate that
/// appears the same (non-zero) number of times on every line wins, then the
/// most frequent one. Separators inside double quotes don't count. Falls
/// back to a comma.
pub fn sniff_delimiter(text: &str) -> u8 {
    const CANDIDATES: [u8; 4] = *b",\t;|";
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(10).collect();
    if lines.is_empty() {
        return b',';
    }
    let count = |line: &str, d: u8| {
        let mut quoted = false;
        line.bytes()
            .filter(|&b| {
                if b == b'"' {
                    quoted = !quoted;
                }
                !quoted && b == d
            })
            .count()
    };
    let mut best = (false, 0usize, b',');
    for d in CANDIDATES {
        let counts: Vec<usize> = lines.iter().map(|l| count(l, d)).collect();
        let min = *counts.iter().min().unwrap_or(&0);
        if min == 0 {
            continue;
        }
        let consistent = counts.iter().all(|&c| c == counts[0]);
        // Candidates are in preference order, so ties keep the earlier one.
        if (consistent, min) > (best.0, best.1) {
            best = (consistent, min, d);
        }
    }
    best.2
}

/// Parse delimited text. Rows may have different widths.
pub fn parse_delimited(text: &str, delimiter: u8) -> Result<Spreadsheet, String> {
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| format!("Failed to read row {}: {}", i + 1, e))?;
        rows.push(record.iter().map(str::to_string).collect());
    }
    Ok(sheet_from_rows(rows))
}

/// Every GitHub-style pipe table in a Markdown document, one sheet each.
/// A `#` heading right before a table names its sheet, so files written by
/// File → Export → Markdown (all sheets) come back with their sheet names.
pub fn parse_markdown_tables(text: &str) -> Vec<NamedSheet> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with('#') {
            let title = line.trim_start_matches('#').trim();
            heading = (!title.is_empty()).then(|| title.to_string());
            i += 1;
            continue;
        }
        let is_table_start = line.contains('|')
            && lines.get(i + 1).is_some_and(|next| is_separator_row(next));
        if !is_table_start {
            i += 1;
            continue;
        }
        let mut rows = vec![split_markdown_row(line)];
        i += 2;
        while i < lines.len() && lines[i].contains('|') {
            rows.push(split_markdown_row(lines[i]));
            i += 1;
        }
        out.push((heading.take(), sheet_from_rows(rows)));
    }
    out
}

/// `|---|:--:|` (outer pipes optional).
fn is_separator_row(line: &str) -> bool {
    let cells = split_markdown_row(line);
    !cells.is_empty()
        && line.contains('-')
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        })
}

/// Split a table row on unescaped pipes and undo the export's escaping
/// (`\|`, `\\`, `<br>` for line breaks).
fn split_markdown_row(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some(next @ ('|' | '\\')) => cur.push(next),
                Some(next) => {
                    cur.push('\\');
                    cur.push(next);
                }
                None => cur.push('\\'),
            },
            '|' => cells.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    // Text after the last pipe is a cell only when the row has no closing pipe.
    if !cur.trim().is_empty() {
        cells.push(cur);
    }
    cells
        .into_iter()
        .map(|c| c.trim().replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n"))
        .collect()
}

/// JSON tables:
/// - an array of objects → a header row of their keys (in file order), one
///   row per object;
/// - an array of arrays → one row per inner array;
/// - an object whose values are such arrays → one sheet per key;
/// - a single object → a two-column key/value table.
///
/// Nested objects and arrays inside a cell are kept as JSON text.
pub fn parse_json_tables(text: &str) -> Result<Vec<NamedSheet>, String> {
    let value: Json = serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
    match value {
        Json::Arr(items) => Ok(vec![(None, json_array_sheet(&items))]),
        Json::Obj(fields) if !fields.is_empty() && fields.iter().all(|(_, v)| matches!(v, Json::Arr(_))) => Ok(fields
            .into_iter()
            .map(|(k, v)| match v {
                Json::Arr(items) => (Some(k), json_array_sheet(&items)),
                _ => unreachable!(),
            })
            .collect()),
        Json::Obj(fields) => {
            let rows = fields.iter().map(|(k, v)| vec![k.clone(), v.cell()]).collect();
            Ok(vec![(None, sheet_from_rows(rows))])
        }
        _ => Err("JSON must be an array or an object to become a table".to_string()),
    }
}

/// JSON Lines / NDJSON: one JSON value per line, as one table.
pub fn parse_json_lines(text: &str) -> Result<Spreadsheet, String> {
    let items = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("Invalid JSON on line {}: {}", i + 1, e)))
        .collect::<Result<Vec<Json>, String>>()?;
    Ok(json_array_sheet(&items))
}

fn json_array_sheet(items: &[Json]) -> Spreadsheet {
    if items.iter().any(|v| matches!(v, Json::Obj(_))) {
        // Header: keys in the order they first appear across the objects.
        let mut header: Vec<&str> = Vec::new();
        for item in items {
            if let Json::Obj(fields) = item {
                for (k, _) in fields {
                    if !header.contains(&k.as_str()) {
                        header.push(k);
                    }
                }
            }
        }
        let mut rows = vec![header.iter().map(|k| k.to_string()).collect()];
        for item in items {
            rows.push(match item {
                Json::Obj(fields) => header
                    .iter()
                    .map(|k| fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.cell()).unwrap_or_default())
                    .collect(),
                other => vec![other.cell()],
            });
        }
        sheet_from_rows(rows)
    } else {
        let rows = items
            .iter()
            .map(|item| match item {
                Json::Arr(cells) => cells.iter().map(Json::cell).collect(),
                other => vec![other.cell()],
            })
            .collect();
        sheet_from_rows(rows)
    }
}

/// A JSON value that keeps object keys in file order (`serde_json::Map`
/// sorts them, and the column order of a table matters).
enum Json {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// The cell text: scalars as values, nested structures as JSON.
    fn cell(&self) -> String {
        match self {
            Json::Null => String::new(),
            Json::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Json::Num(n) | Json::Str(n) => n.clone(),
            nested => nested.to_json(),
        }
    }

    fn to_json(&self) -> String {
        let quote = |s: &str| serde_json::to_string(s).unwrap_or_default();
        match self {
            Json::Null => "null".to_string(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => n.clone(),
            Json::Str(s) => quote(s),
            Json::Arr(items) => format!("[{}]", items.iter().map(Json::to_json).collect::<Vec<_>>().join(",")),
            Json::Obj(fields) => format!(
                "{{{}}}",
                fields.iter().map(|(k, v)| format!("{}:{}", quote(k), v.to_json())).collect::<Vec<_>>().join(",")
            ),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Json {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Json, E> {
                Ok(Json::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Json, E> {
                Ok(Json::Num(n.to_string()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Json, E> {
                Ok(Json::Num(n.to_string()))
            }
            fn visit_f64<E>(self, n: f64) -> Result<Json, E> {
                Ok(Json::Num(serde_json::Number::from_f64(n).map(|n| n.to_string()).unwrap_or_default()))
            }
            fn visit_str<E>(self, s: &str) -> Result<Json, E> {
                Ok(Json::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<Json, E> {
                Ok(Json::Str(s))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Json::Arr(items))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut fields: Vec<(String, Json)> = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Json>()? {
                    // A repeated key keeps its first position, last value.
                    match fields.iter_mut().find(|(f, _)| *f == k) {
                        Some(slot) => slot.1 = v,
                        None => fields.push((k, v)),
                    }
                }
                Ok(Json::Obj(fields))
            }
        }
        d.deserialize_any(V)
    }
}

/// A sheet holding `rows` as plain values from A1, a little larger than the
/// data so there is room to keep typing.
pub fn sheet_from_rows(rows: Vec<Vec<String>>) -> Spreadsheet {
    let mut sheet = Spreadsheet::default();
    let mut max_row = 0;
    let mut max_col = 0;
    let mut cells = Vec::new();
    for (r, row) in rows.into_iter().enumerate() {
        for (c, value) in row.into_iter().enumerate() {
            if value.is_empty() {
                continue;
            }
            max_row = max_row.max(r);
            max_col = max_col.max(c);
            cells.push((r, c, CellData { value, ..CellData::default() }));
        }
    }
    if !cells.is_empty() {
        sheet.rows = sheet.rows.max(max_row + 5);
        sheet.cols = sheet.cols.max(max_col + 5);
    }
    // One bulk write; the workbook's dependency graph is rebuilt on load.
    sheet.set_many(cells);
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(sheet: &Spreadsheet, r: usize, c: usize) -> String {
        sheet.get_cell(r, c).value
    }

    #[test]
    fn decodes_bom_utf16_and_latin1() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFName"), "Name");
        assert_eq!(decode_text(b"\xFF\xFEa\x00\t\x00b\x00"), "a\tb");
        assert_eq!(decode_text(b"Stra\xDFe \x80"), "Straße €");
    }

    #[test]
    fn sniffs_common_separators() {
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3"), b',');
        assert_eq!(sniff_delimiter("a;b;c\n1,5;2,5;3"), b';');
        assert_eq!(sniff_delimiter("a\tb\n1\t2"), b'\t');
        assert_eq!(sniff_delimiter("a|b\n1|2"), b'|');
        // A comma inside quotes doesn't count.
        assert_eq!(sniff_delimiter("\"x, y\";z\n\"1, 2\";3"), b';');
        assert_eq!(sniff_delimiter("single column"), b',');
    }

    #[test]
    fn parses_semicolon_csv_as_text_not_formulas() {
        let sheet = parse_delimited("Name;Total\nA;=1+1\n", b';').unwrap();
        assert_eq!(value(&sheet, 0, 1), "Total");
        assert_eq!(value(&sheet, 1, 1), "=1+1");
        assert!(sheet.get_cell(1, 1).formula.is_none());
    }

    #[test]
    fn markdown_export_round_trips_with_sheet_names() {
        let md = "## Sales\n\n| Item | Note |\n| ---- | :--: |\n| Pen  | a \\| b |\n| Ink  | x<br>y |\n\n\
                  ## Costs\n\n|A|B|\n|-|-|\n|1|2|\n";
        let tables = parse_markdown_tables(md);
        assert_eq!(tables.len(), 2);
        let (name, sheet) = &tables[0];
        assert_eq!(name.as_deref(), Some("Sales"));
        assert_eq!(value(sheet, 0, 0), "Item");
        assert_eq!(value(sheet, 1, 1), "a | b");
        assert_eq!(value(sheet, 2, 1), "x\ny");
        assert_eq!(tables[1].0.as_deref(), Some("Costs"));
        assert_eq!(value(&tables[1].1, 1, 1), "2");
    }

    #[test]
    fn markdown_without_outer_pipes_and_without_tables() {
        let tables = parse_markdown_tables("a | b\n--|--\n1 | 2\n");
        assert_eq!(value(&tables[0].1, 1, 1), "2");
        assert!(parse_markdown_tables("just | text\nno separator").is_empty());
    }

    #[test]
    fn json_array_of_objects_keeps_key_order_and_nested_values() {
        let tables =
            parse_json_tables(r#"[{"z": 1, "a": true}, {"a": null, "b": {"x": [1]}}]"#).unwrap();
        let sheet = &tables[0].1;
        assert_eq!(
            (value(sheet, 0, 0), value(sheet, 0, 1), value(sheet, 0, 2)),
            ("z".into(), "a".into(), "b".into())
        );
        assert_eq!(value(sheet, 1, 1), "TRUE");
        assert_eq!(value(sheet, 2, 1), "");
        assert_eq!(value(sheet, 2, 2), r#"{"x":[1]}"#);
    }

    #[test]
    fn json_object_of_arrays_is_one_sheet_per_key() {
        let tables = parse_json_tables(r#"{"Q1": [[1, 2]], "Q2": [{"n": 3}]}"#).unwrap();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].0.as_deref(), Some("Q1"));
        assert_eq!(value(&tables[0].1, 0, 1), "2");
        assert_eq!(value(&tables[1].1, 1, 0), "3");
        assert!(parse_json_tables("42").is_err());
    }

    #[test]
    fn json_lines() {
        let sheet = parse_json_lines("{\"a\": 1}\n\n{\"a\": 2, \"b\": \"x\"}\n").unwrap();
        assert_eq!(value(&sheet, 0, 1), "b");
        assert_eq!(value(&sheet, 2, 0), "2");
        assert!(parse_json_lines("{\"a\": 1}\nnot json").unwrap_err().contains("line 2"));
    }
}
