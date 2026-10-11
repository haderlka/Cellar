//! A sheet as a PDF, for File → Export → PDF and Print.
//!
//! Hand-written like the .xlsx export: A4 portrait pages, the sheet's own
//! column widths and row heights, grid lines, fills, bold and colours, with
//! a page footer. Columns that don't fit go on further pages (down, then
//! over, as Excel prints). Text uses the built-in Helvetica, which covers
//! Western European characters; anything else prints as `?`.

use crate::domain::{format_cell_value, CellStyle, Spreadsheet, TerminalColor};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 36.0;
const FOOTER_H: f32 = 18.0;
const FONT_SIZE: f32 = 9.0;
/// Points per character of column width at this font size.
const CHAR_W: f32 = 5.4;

/// Helvetica advance widths for ASCII 32..=126, in 1/1000 of the font size.
/// Digits and number punctuation are the same in Helvetica-Bold, so
/// right-aligned numbers line up in both.
const WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278,
    500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469,
    556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// The byte Helvetica's WinAnsi encoding uses for `ch`, or `?`.
fn win_ansi(ch: char) -> u8 {
    match ch {
        ' '..='~' => ch as u8,
        '\u{a0}'..='\u{ff}' => ch as u32 as u8,
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '‰' => 0x89,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        _ => b'?',
    }
}

fn text_width(text: &str, bold: bool) -> f32 {
    let units: f32 = text
        .chars()
        .map(|ch| match win_ansi(ch) {
            b @ 32..=126 => WIDTHS[(b - 32) as usize] as f32,
            _ => 556.0,
        })
        .sum();
    // Bold letters run a little wider; digits don't.
    let numeric = text.chars().all(|c| c.is_ascii_digit() || " .,-+%$()".contains(c));
    units * FONT_SIZE / 1000.0 * if bold && !numeric { 1.06 } else { 1.0 }
}

/// `text` as a PDF string literal.
fn pdf_string(text: &str) -> String {
    let mut out = String::from("(");
    for ch in text.chars() {
        match win_ansi(ch) {
            b @ (b'(' | b')' | b'\\') => {
                out.push('\\');
                out.push(b as char);
            }
            b @ 32..=126 => out.push(b as char),
            b => out.push_str(&format!("\\{:03o}", b)),
        }
    }
    out.push(')');
    out
}

fn rgb(color: &TerminalColor) -> String {
    let (r, g, b) = color.rgb();
    format!("{:.3} {:.3} {:.3}", r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Split `sizes` into runs that each fit in `available`. A single item
/// larger than the page gets a run of its own.
fn paginate(sizes: &[(usize, f32)], available: f32) -> Vec<Vec<(usize, f32)>> {
    let mut pages: Vec<Vec<(usize, f32)>> = Vec::new();
    let mut used = 0.0;
    for &(index, size) in sizes {
        let size = size.min(available);
        if pages.is_empty() || used + size > available + 0.01 {
            pages.push(Vec::new());
            used = 0.0;
        }
        pages.last_mut().expect("just pushed").push((index, size));
        used += size;
    }
    pages
}

/// Render the used part of `sheet` (hidden rows and columns left out).
/// `None` when the sheet has nothing to print.
pub fn sheet_to_pdf(sheet: &Spreadsheet, title: &str) -> Option<Vec<u8>> {
    let used = |cd: &crate::domain::CellData| !cd.value.is_empty();
    let (last_row, last_col) =
        sheet.cells.iter().filter(|(_, cd)| used(cd)).fold(None, |acc: Option<(usize, usize)>, (&(r, c), _)| {
            Some(acc.map_or((r, c), |(mr, mc)| (mr.max(r), mc.max(c))))
        })?;
    let view = &sheet.view_state;
    let cols: Vec<(usize, f32)> = (0..=last_col)
        .filter(|c| !view.hidden_cols.contains(c))
        .map(|c| (c, sheet.get_column_width(c) as f32 * CHAR_W + 8.0))
        .collect();
    let rows: Vec<(usize, f32)> = (0..=last_row)
        .filter(|r| !view.hidden_rows.contains(r))
        .map(|r| (r, sheet.get_row_height(r) as f32))
        .collect();
    let col_pages = paginate(&cols, PAGE_W - 2.0 * MARGIN);
    let row_pages = paginate(&rows, PAGE_H - 2.0 * MARGIN - FOOTER_H);
    let total = col_pages.len() * row_pages.len();

    let mut streams = Vec::with_capacity(total);
    for page_cols in &col_pages {
        for page_rows in &row_pages {
            let footer = format!("{} - Page {} of {}", title, streams.len() + 1, total);
            streams.push(page_content(sheet, page_rows, page_cols, &footer));
        }
    }
    Some(assemble(&streams))
}

fn page_content(sheet: &Spreadsheet, rows: &[(usize, f32)], cols: &[(usize, f32)], footer: &str) -> String {
    let mut fills = String::new();
    let mut text = String::new();
    let width: f32 = cols.iter().map(|c| c.1).sum();
    let height: f32 = rows.iter().map(|r| r.1).sum();
    let top = PAGE_H - MARGIN;

    let mut y = top;
    for &(row, h) in rows {
        y -= h;
        let mut x = MARGIN;
        for &(col, w) in cols {
            let style: CellStyle = sheet.display_style(row, col);
            if let Some(bg) = &style.bg_color {
                fills.push_str(&format!("{} rg {:.2} {:.2} {:.2} {:.2} re f\n", rgb(bg), x, y, w, h));
            }
            if let Some(cd) = sheet.cells.get(&(row, col)) {
                let sparkline = cd.formula.as_ref().is_some_and(|f| f.to_uppercase().starts_with("=SPARKLINE("));
                let shown = match &cd.format {
                    Some(format) => format_cell_value(&cd.value, format),
                    None => cd.value.clone(),
                };
                if !shown.is_empty() && !sparkline {
                    let color = match (&style.fg_color, &style.bg_color) {
                        (Some(fg), _) => rgb(fg),
                        _ => "0 0 0".to_string(),
                    };
                    let numeric = cd.value.trim().parse::<f64>().is_ok();
                    let tw = text_width(&shown, style.bold);
                    let tx = if numeric { x + w - 3.0 - tw } else { x + 3.0 };
                    let ty = y + 4.0;
                    // Clipped to the cell, as on screen.
                    text.push_str(&format!(
                        "q {:.2} {:.2} {:.2} {:.2} re W n BT /{} {} Tf {} rg {:.2} {:.2} Td {} Tj ET",
                        x + 1.0,
                        y,
                        w - 2.0,
                        h,
                        if style.bold { "F2" } else { "F1" },
                        FONT_SIZE,
                        color,
                        tx,
                        ty,
                        pdf_string(&shown)
                    ));
                    if style.underline {
                        text.push_str(&format!(" {} RG 0.5 w {:.2} {:.2} m {:.2} {:.2} l S", color, tx, ty - 1.2, tx + tw, ty - 1.2));
                    }
                    text.push_str(" Q\n");
                }
            }
            x += w;
        }
    }

    let mut grid = String::from("0.78 G 0.25 w\n");
    let mut x = MARGIN;
    grid.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x, top, x, top - height));
    for &(_, w) in cols {
        x += w;
        grid.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x, top, x, top - height));
    }
    let mut y = top;
    grid.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", MARGIN, y, MARGIN + width, y));
    for &(_, h) in rows {
        y -= h;
        grid.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", MARGIN, y, MARGIN + width, y));
    }

    let footer_x = (PAGE_W - text_width(footer, false) * 8.0 / FONT_SIZE) / 2.0;
    let footer = format!("BT /F1 8 Tf 0.45 g {:.2} {:.2} Td {} Tj ET\n", footer_x, MARGIN - 4.0, pdf_string(footer));
    format!("{}{}{}{}", fills, grid, text, footer)
}

/// Wrap the page content streams in a PDF file.
fn assemble(streams: &[String]) -> Vec<u8> {
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        String::new(), // the page tree, filled in below
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut kids = Vec::new();
    for stream in streams {
        let page = objects.len() + 1;
        kids.push(format!("{} 0 R", page));
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>",
            PAGE_W,
            PAGE_H,
            page + 1
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), stream));
    }
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len());

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CellData, Workbook};

    fn sheet(cells: &[(usize, usize, &str)]) -> Workbook {
        let mut wb = Workbook::default();
        for &(r, c, v) in cells {
            wb.set_cell_on_active(r, c, CellData { value: v.to_string(), ..CellData::default() });
        }
        wb
    }

    #[test]
    fn empty_sheet_has_nothing_to_print() {
        assert!(sheet_to_pdf(Workbook::default().current_sheet(), "x").is_none());
    }

    #[test]
    fn pages_follow_rows_and_columns_and_xref_points_at_objects() {
        // 120 rows of 15 pt need three pages; a column far to the right
        // (13 columns, 10 per page) needs a second set.
        let mut cells: Vec<(usize, usize, &str)> = (0..120).map(|r| (r, 0, "row")).collect();
        cells.push((0, 12, "far (right) \\ €"));
        let wb = sheet(&cells);
        let pdf = sheet_to_pdf(wb.current_sheet(), "Book").unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/Count 6"), "3 row pages x 2 column pages");
        assert!(text.contains("(far \\(right\\) \\\\ \\200)"), "escaped, euro in WinAnsi");
        assert!(text.contains("(Book - Page 6 of 6)"));

        // Every xref entry is the offset of "N 0 obj".
        let xref = text.rfind("startxref\n").unwrap();
        let start: usize = text[xref + 10..].lines().next().unwrap().parse().unwrap();
        let entries: Vec<&str> = text[start..].lines().skip(3).take_while(|l| l.ends_with("n ")).collect();
        assert_eq!(entries.len(), 4 + 12);
        for (i, entry) in entries.iter().enumerate() {
            let offset: usize = entry[..10].parse().unwrap();
            assert!(text[offset..].starts_with(&format!("{} 0 obj", i + 1)), "object {}", i + 1);
        }
    }

    #[test]
    fn numbers_are_right_aligned_by_helvetica_widths() {
        assert_eq!(text_width("1,234.50", false), text_width("1,234.50", true));
        assert!((text_width("100", false) - 3.0 * 0.556 * FONT_SIZE).abs() < 1e-4);
    }
}
