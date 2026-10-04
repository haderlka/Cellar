//! Deterministic, diff- and merge-friendly JSON layout for `.cellar` files.
//!
//! `serde_json::to_string_pretty` on a `Workbook` has two problems for
//! version control:
//!
//! 1. `HashMap`-backed fields (cells, column widths, named ranges,
//!    validations) serialize in hash-iteration order, which changes from
//!    run to run — so saving an unchanged workbook rewrites the whole file.
//! 2. Pretty-printing every nested value puts each cell over ~15 lines, so
//!    two people editing neighbouring cells collide in a merge.
//!
//! This writer fixes both: object keys are sorted, the `cells` array is
//! sorted by (row, col), default fields inside cells are dropped, each
//! cell is written on a single line, and anything else nested
//! `INLINE_DEPTH` or deeper is inlined too (so a pivot or chart definition
//! spans a few short lines, one per setting). The result is one line per
//! cell:
//!
//! ```text
//!       [0, 0, {"value": "Region"}],
//!       [1, 2, {"formula": "=SUM(C2:C9)", "value": "1200.5"}],
//! ```
//!
//! Fields at their default value are left out of the file: inside cells by
//! `strip_defaults` below, everywhere else by `skip_serializing_if` on the
//! model (paired with `#[serde(default)]` so loading fills them back in).
//!
//! The output is still plain JSON: it loads through the normal `serde_json`
//! path and any JSON tool can process it.

use serde::Serialize;
use serde_json::Value;

/// Containers at this nesting depth or deeper are written on one line.
/// Depth 0 is the root object; a sheet's pivot/chart entry
/// (`root.sheets[i].pivots[j]`) sits at depth 4, so its settings (depth 5)
/// get one line each. Cell entries are always inlined regardless of depth.
const INLINE_DEPTH: usize = 5;

/// Serialize `value` into the canonical layout described in the module docs.
pub fn to_canonical_string<T: Serialize>(value: &T) -> Result<String, String> {
    let mut v = serde_json::to_value(value).map_err(|e| format!("Serialization failed: {}", e))?;
    normalize(&mut v);
    let mut out = String::new();
    write_value(&v, 0, false, &mut out);
    out.push('\n');
    Ok(out)
}

/// Sort every `cells` array by (row, col) and strip default-valued fields
/// from the cell payloads. Everything stripped deserializes back to its
/// default (`Option` → `None`, `#[serde(default)]` on the style types).
fn normalize(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(cells)) = map.get_mut("cells") {
                cells.sort_by_key(|c| (coord(c, 0), coord(c, 1)));
                for cell in cells.iter_mut() {
                    if let Some(data) = cell.get_mut(2) {
                        strip_defaults(data);
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                normalize(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize(item);
            }
        }
        _ => {}
    }
}

/// Drop nulls, `false` flags, the `General` number format, and objects
/// left empty by that — `{"format": {"style": {"bold": false, …}}}`
/// collapses to nothing.
fn strip_defaults(v: &mut Value) {
    let Value::Object(map) = v else { return };
    for (_, child) in map.iter_mut() {
        strip_defaults(child);
    }
    map.retain(|key, field| match field {
        Value::Null | Value::Bool(false) => false,
        Value::Object(o) => !o.is_empty(),
        Value::String(s) => !(key == "number_format" && s == "General"),
        _ => true,
    });
}

fn coord(cell: &Value, idx: usize) -> u64 {
    cell.get(idx).and_then(Value::as_u64).unwrap_or(u64::MAX)
}

/// Object entries in a stable order: numeric keys (column widths,
/// validations) numerically, everything else lexically.
fn sorted_entries(map: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|(a, _), (b, _)| match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    });
    entries
}

/// `inline` forces single-line output (used for every cell entry).
fn write_value(v: &Value, depth: usize, inline: bool, out: &mut String) {
    let inline = inline || depth >= INLINE_DEPTH;
    match v {
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Array(items) if inline => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(item, depth + 1, true, out);
            }
            out.push(']');
        }
        Value::Object(map) if inline => {
            out.push('{');
            for (i, (k, val)) in sorted_entries(map).into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_string(k, out);
                out.push_str(": ");
                write_value(val, depth + 1, true, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(depth + 1, out);
                write_value(item, depth + 1, false, out);
            }
            newline(depth, out);
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, val)) in sorted_entries(map).into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(depth + 1, out);
                write_string(k, out);
                out.push_str(": ");
                if k == "cells"
                    && let Value::Array(cells) = val
                    && !cells.is_empty()
                {
                    // One line per cell, whatever the nesting depth.
                    out.push('[');
                    for (j, cell) in cells.iter().enumerate() {
                        if j > 0 {
                            out.push(',');
                        }
                        newline(depth + 2, out);
                        write_value(cell, depth + 2, true, out);
                    }
                    newline(depth + 1, out);
                    out.push(']');
                } else {
                    write_value(val, depth + 1, false, out);
                }
            }
            newline(depth, out);
            out.push('}');
        }
        Value::String(s) => write_string(s, out),
        other => out.push_str(&other.to_string()),
    }
}

fn write_string(s: &str, out: &mut String) {
    // serde_json's string serializer handles escaping; it cannot fail.
    out.push_str(&serde_json::to_string(s).unwrap_or_default());
}

fn newline(depth: usize, out: &mut String) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CellData, Workbook};

    fn sample() -> Workbook {
        let mut wb = Workbook::default();
        let sheet = wb.current_sheet_mut();
        for (r, c, v) in [(5, 1, "b"), (0, 0, "a"), (5, 0, "c"), (2, 3, "d")] {
            sheet.cells.insert(
                (r, c),
                CellData { value: v.to_string(), ..CellData::default() },
            );
        }
        sheet.column_widths.insert(10, 12);
        sheet.column_widths.insert(2, 20);
        wb
    }

    #[test]
    fn output_is_stable_across_saves() {
        let a = to_canonical_string(&sample()).unwrap();
        let b = to_canonical_string(&sample()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn cells_are_sorted_one_per_line_without_nulls() {
        let out = to_canonical_string(&sample()).unwrap();
        let cell_lines: Vec<&str> = out
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('[') && l.contains("\"value\""))
            .collect();
        assert_eq!(
            cell_lines,
            vec![
                r#"[0, 0, {"value": "a"}],"#,
                r#"[2, 3, {"value": "d"}],"#,
                r#"[5, 0, {"value": "c"}],"#,
                r#"[5, 1, {"value": "b"}]"#,
            ]
        );
    }

    #[test]
    fn default_format_fields_are_omitted_and_round_trip() {
        use crate::domain::{CellFormat, CellStyle, NumberFormat, TerminalColor};
        let mut wb = Workbook::default();
        let bold = CellFormat {
            number_format: NumberFormat::General,
            style: CellStyle { bold: true, fg_color: Some(TerminalColor::Red), ..Default::default() },
        };
        let sheet = wb.current_sheet_mut();
        sheet.cells.insert((0, 0), CellData { value: "x".into(), format: Some(bold.clone()), ..Default::default() });
        sheet.cells.insert((0, 1), CellData { value: "y".into(), format: Some(CellFormat::default()), ..Default::default() });
        let out = to_canonical_string(&wb).unwrap();
        assert!(out.contains(r#"[0, 0, {"format": {"style": {"bold": true, "fg_color": "Red"}}, "value": "x"}]"#), "{}", out);
        assert!(out.contains(r#"[0, 1, {"value": "y"}]"#), "{}", out);
        let back: Workbook = serde_json::from_str(&out).unwrap();
        assert_eq!(back.sheets[0].get_cell(0, 0).format, Some(bold));
        assert_eq!(back.sheets[0].get_cell(0, 1).format, None);
    }

    #[test]
    fn pivot_definitions_get_one_line_per_setting() {
        use crate::domain::{PivotField, PivotSpec, PivotValue, Summarize};
        let mut wb = sample();
        let mut p = PivotSpec::new("PivotTable1", "A1:C9");
        p.rows.push(PivotField::new("Region"));
        p.values.push(PivotValue::new("Revenue", Summarize::Sum));
        wb.current_sheet_mut().pivots.push(p.clone());
        let out = to_canonical_string(&wb).unwrap();
        assert!(out.contains("\n          \"rows\": [{\"field\": \"Region\"}],\n"), "{}", out);
        assert!(out.contains("\n          \"values\": [{\"field\": \"Revenue\"}]\n"), "{}", out);
        let back: Workbook = serde_json::from_str(&out).unwrap();
        assert_eq!(back.sheets[0].pivots, vec![p]);
    }

    #[test]
    fn default_workbook_and_sheet_fields_are_omitted() {
        let out = to_canonical_string(&Workbook::default()).unwrap();
        for key in [
            "iterative_calc", "iter_max", "iter_epsilon", "named_ranges", "rows", "cols",
            "column_widths", "default_column_width", "conditional_formats", "tables", "view_state",
        ] {
            assert!(!out.contains(&format!("\"{}\"", key)), "{} written: {}", key, out);
        }
    }

    #[test]
    fn non_default_workbook_and_sheet_fields_round_trip() {
        let mut wb = Workbook { iterative_calc: true, iter_max: 50, iter_epsilon: 1e-9, ..Default::default() };
        let sheet = wb.current_sheet_mut();
        sheet.rows = 101;
        sheet.cols = 27;
        sheet.default_column_width = 10;
        sheet.view_state.frozen_rows = 1;
        let out = to_canonical_string(&wb).unwrap();
        assert!(out.contains("\"frozen_rows\": 1") && !out.contains("frozen_cols"), "{}", out);
        let back: Workbook = serde_json::from_str(&out).unwrap();
        assert!(back.iterative_calc);
        assert_eq!((back.iter_max, back.iter_epsilon), (50, 1e-9));
        let s = &back.sheets[0];
        assert_eq!((s.rows, s.cols, s.default_column_width), (101, 27, 10));
        assert_eq!(s.view_state.frozen_rows, 1);
    }

    #[test]
    fn numeric_keys_sort_numerically() {
        let out = to_canonical_string(&sample()).unwrap();
        let w2 = out.find("\"2\": 20").unwrap();
        let w10 = out.find("\"10\": 12").unwrap();
        assert!(w2 < w10);
    }

    #[test]
    fn round_trips_through_serde() {
        let out = to_canonical_string(&sample()).unwrap();
        let back: Workbook = serde_json::from_str(&out).unwrap();
        assert_eq!(back.sheets[0].cells.len(), 4);
        assert_eq!(back.sheets[0].get_cell(5, 1).value, "b");
        assert_eq!(back.sheets[0].get_cell(5, 1).formula, None);
    }
}
