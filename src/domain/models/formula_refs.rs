//! Cell and range references in a formula's text, with their positions.
//! The GUI uses them to outline the cells a formula reads and to colour
//! the references in the editor, as Excel does.

use super::Spreadsheet;

/// Top-left and bottom-right cell, as (row, col).
type Area = ((usize, usize), (usize, usize));

/// A cell or range reference found in formula text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaRef {
    /// Byte range in the formula text, including a `Sheet2!` prefix.
    pub span: std::ops::Range<usize>,
    /// Sheet named by a prefix; `None` means the formula's own sheet.
    pub sheet: Option<String>,
    /// Top-left and bottom-right cell, as (row, col). Whole columns and
    /// rows (`A:B`, `3:5`) extend to the edge of Excel's grid.
    pub range: ((usize, usize), (usize, usize)),
}

/// References in `formula`, in order of appearance. Text inside string
/// literals, function names (`LOG10(`) and names that only look like
/// references are skipped. Defined names and table references are not
/// resolved.
pub fn formula_references(formula: &str) -> Vec<FormulaRef> {
    let b = formula.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i = skip_quoted(b, i, b'"');
            continue;
        }
        // References start at a word boundary.
        if i > 0 && is_word(b[i - 1]) {
            i += 1;
            continue;
        }
        let mut sheet = None;
        let mut at = i;
        if c == b'\'' {
            let end = skip_quoted(b, i, b'\'');
            if b.get(end) != Some(&b'!') {
                i = end;
                continue;
            }
            sheet = Some(formula[i + 1..end - 1].replace("''", "'"));
            at = end + 1;
        } else if is_word(c) {
            let end = word_end(b, i);
            if b.get(end) == Some(&b'!') {
                sheet = Some(formula[i..end].to_string());
                at = end + 1;
            }
        } else if c != b'$' {
            i += 1;
            continue;
        }
        match parse_area(b, at) {
            Some((end, range)) => {
                out.push(FormulaRef { span: i..end, sheet, range });
                i = end;
            }
            None => i = word_end(b, at).max(i + 1),
        }
    }
    out
}

/// Letters, digits, `_` and `.` (and any non-ASCII byte) continue a name.
fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c >= 0x80
}

fn word_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (is_word(b[i]) || b[i] == b'$') {
        i += 1;
    }
    i
}

/// Index just past a `"…"` or `'…'` literal starting at `i` (doubled
/// quotes are escapes), or the end of the text when it isn't closed.
fn skip_quoted(b: &[u8], i: usize, q: u8) -> usize {
    let mut j = i + 1;
    while j < b.len() {
        if b[j] == q {
            if b.get(j + 1) == Some(&q) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

#[derive(Clone, Copy)]
enum Part {
    Cell(usize, usize),
    Col(usize),
    Row(usize),
}

/// `A1`, `$A$1`, `A`, `$C` or `12` at `i`.
fn parse_part(b: &[u8], mut i: usize) -> Option<(usize, Part)> {
    let dollar = |i: &mut usize| {
        if b.get(*i) == Some(&b'$') {
            *i += 1;
        }
    };
    dollar(&mut i);
    let letters = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col_str = std::str::from_utf8(&b[letters..i]).ok()?;
    if col_str.len() > 3 {
        return None;
    }
    if !col_str.is_empty() {
        dollar(&mut i);
    }
    let digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let row_str = std::str::from_utf8(&b[digits..i]).ok()?;
    if row_str.len() > 7 {
        return None;
    }
    let col = match col_str {
        "" => None,
        s => Some(Spreadsheet::parse_column_label(s).filter(|&c| c < Spreadsheet::MAX_COLS)?),
    };
    let row = match row_str {
        "" => None,
        s => Some(s.parse::<usize>().ok().filter(|&r| (1..=Spreadsheet::MAX_ROWS).contains(&r))? - 1),
    };
    let part = match (row, col) {
        (Some(r), Some(c)) => Part::Cell(r, c),
        (None, Some(c)) => Part::Col(c),
        (Some(r), None) => Part::Row(r),
        (None, None) => return None,
    };
    Some((i, part))
}

/// A reference at `i`: a cell, `cell:cell`, `col:col` or `row:row`.
fn parse_area(b: &[u8], i: usize) -> Option<(usize, Area)> {
    let (end, first) = parse_part(b, i)?;
    let second = (b.get(end) == Some(&b':')).then(|| parse_part(b, end + 1)).flatten();
    let last_row = Spreadsheet::MAX_ROWS - 1;
    let last_col = Spreadsheet::MAX_COLS - 1;
    let (end, a, z) = match (first, second) {
        (Part::Cell(r0, c0), Some((e, Part::Cell(r1, c1)))) => (e, (r0, c0), (r1, c1)),
        (Part::Col(c0), Some((e, Part::Col(c1)))) => (e, (0, c0), (last_row, c1)),
        (Part::Row(r0), Some((e, Part::Row(r1)))) => (e, (r0, 0), (r1, last_col)),
        (Part::Cell(r, c), _) => (end, (r, c), (r, c)),
        _ => return None,
    };
    // `LOG10(` is a function, `A1B` a name.
    if b.get(end).is_some_and(|&c| is_word(c) || matches!(c, b'(' | b'$' | b'!' | b'[')) {
        return None;
    }
    Some((end, ((a.0.min(z.0), a.1.min(z.1)), (a.0.max(z.0), a.1.max(z.1)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(f: &str) -> Vec<(&str, Option<String>, Area)> {
        formula_references(f).into_iter().map(|r| (&f[r.span], r.sheet, r.range)).collect()
    }

    #[test]
    fn cells_and_ranges() {
        assert_eq!(
            refs("=A1+SUM($B$2:C3)*b4"),
            vec![
                ("A1", None, ((0, 0), (0, 0))),
                ("$B$2:C3", None, ((1, 1), (2, 2))),
                ("b4", None, ((3, 1), (3, 1))),
            ]
        );
    }

    #[test]
    fn reversed_range_is_normalised() {
        assert_eq!(refs("=SUM(C3:A1)")[0].2, ((0, 0), (2, 2)));
    }

    #[test]
    fn sheet_prefixes() {
        assert_eq!(
            refs("=Sheet2!A1+'My Sheet'!B2:B5+'It''s'!C1"),
            vec![
                ("Sheet2!A1", Some("Sheet2".into()), ((0, 0), (0, 0))),
                ("'My Sheet'!B2:B5", Some("My Sheet".into()), ((1, 1), (4, 1))),
                ("'It''s'!C1", Some("It's".into()), ((0, 2), (0, 2))),
            ]
        );
    }

    #[test]
    fn whole_columns_and_rows() {
        let r = refs("=SUM(A:B)+SUM(3:4)");
        assert_eq!(r[0].2, ((0, 0), (Spreadsheet::MAX_ROWS - 1, 1)));
        assert_eq!(r[1].2, ((2, 0), (3, Spreadsheet::MAX_COLS - 1)));
    }

    #[test]
    fn skips_things_that_only_look_like_references() {
        assert!(refs("=LOG10(100)+\"A1\"&TRUE+Table1[Col]+MyName+1.5+ATAN2(1,2)").is_empty());
        assert!(refs("=A1B+XFE1+A0").is_empty());
    }

    #[test]
    fn incomplete_range_while_typing_keeps_the_cell() {
        assert_eq!(refs("=SUM(A1:"), vec![("A1", None, ((0, 0), (0, 0)))]);
    }

    #[test]
    fn non_ascii_text_is_safe() {
        assert_eq!(refs("=Ä1+\"€\"&B1"), vec![("B1", None, ((0, 1), (0, 1)))]);
    }
}
