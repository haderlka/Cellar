//! Markdown table export: the computed cell values (formula results, not
//! formulas) with no number formats or styling. The first row of the
//! exported area becomes the table header. Columns are padded so the raw
//! Markdown is readable too.

use unicode_width::UnicodeWidthStr;

use crate::domain::models::{Spreadsheet, Workbook};

/// Bounding box of the non-empty cells, if any.
pub fn used_range(sheet: &Spreadsheet) -> Option<((usize, usize), (usize, usize))> {
    let mut it = sheet.cells.iter().filter(|(_, cd)| !cd.value.trim().is_empty()).map(|(k, _)| *k);
    let first = it.next()?;
    let (mut r0, mut c0, mut r1, mut c1) = (first.0, first.1, first.0, first.1);
    for (r, c) in it {
        r0 = r0.min(r);
        c0 = c0.min(c);
        r1 = r1.max(r);
        c1 = c1.max(c);
    }
    Some(((r0, c0), (r1, c1)))
}

/// Escape a value for a table cell: pipes would split the cell and line
/// breaks would end the row.
fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('|', "\\|").replace("\r\n", "<br>").replace('\n', "<br>")
}

/// A Markdown table of the rectangle (r0, c0)–(r1, c1); the first row is the
/// header.
pub fn range_to_markdown(sheet: &Spreadsheet, (r0, c0): (usize, usize), (r1, c1): (usize, usize)) -> String {
    let rows: Vec<Vec<String>> = (r0..=r1)
        .map(|r| (c0..=c1).map(|c| escape(&sheet.get_cell(r, c).value)).collect())
        .collect();
    let ncols = c1 - c0 + 1;
    let widths: Vec<usize> = (0..ncols)
        .map(|c| rows.iter().map(|row| row[c].width()).max().unwrap_or(0).max(3))
        .collect();
    let line = |cells: &[String]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(v, w)| format!("{}{}", v, " ".repeat(w - v.width())))
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    let mut out = line(&rows[0]);
    let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    out.push_str(&line(&rule));
    for row in &rows[1..] {
        out.push_str(&line(row));
    }
    out
}

/// The used area of one sheet as a Markdown table ("" when empty).
pub fn sheet_to_markdown(sheet: &Spreadsheet) -> String {
    match used_range(sheet) {
        Some((a, b)) => range_to_markdown(sheet, a, b),
        None => String::new(),
    }
}

/// Every non-empty sheet as `## Sheet name` followed by its table.
pub fn workbook_to_markdown(wb: &Workbook) -> String {
    let mut out = String::new();
    for (name, sheet) in wb.sheet_names.iter().zip(&wb.sheets) {
        let table = sheet_to_markdown(sheet);
        if table.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("## {}\n\n{}", name, table));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CellData, CellFormat, NumberFormat};

    fn sheet() -> Spreadsheet {
        let mut s = Spreadsheet::default();
        let mut put = |r, c, v: &str, f: Option<&str>| {
            s.cells.insert(
                (r, c),
                CellData { value: v.into(), formula: f.map(String::from), ..CellData::default() },
            );
        };
        put(1, 1, "Item", None);
        put(1, 2, "Price", None);
        put(2, 1, "A|B", None);
        put(2, 2, "1234.5", None);
        put(3, 2, "2469", Some("=C3*2"));
        s
    }

    #[test]
    fn exports_used_range_with_values_not_formulas_or_formats() {
        let mut s = sheet();
        // A currency format must not leak into the export.
        s.cells.get_mut(&(2, 2)).unwrap().format = Some(CellFormat {
            number_format: NumberFormat::Currency { symbol: "$".into(), decimals: 2 },
            ..CellFormat::default()
        });
        assert_eq!(
            sheet_to_markdown(&s),
            "| Item | Price  |\n\
             | ---- | ------ |\n\
             | A\\|B | 1234.5 |\n\
             |      | 2469   |\n"
        );
    }

    #[test]
    fn workbook_export_has_a_heading_per_non_empty_sheet() {
        let mut wb = Workbook::default();
        *wb.current_sheet_mut() = sheet();
        wb.add_sheet("Empty".into());
        let md = workbook_to_markdown(&wb);
        assert!(md.starts_with("## Sheet1\n\n| Item"));
        assert!(!md.contains("Empty"));
    }

    #[test]
    fn empty_sheet_exports_nothing() {
        assert_eq!(sheet_to_markdown(&Spreadsheet::default()), "");
    }
}
