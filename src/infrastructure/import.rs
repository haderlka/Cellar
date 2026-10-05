//! Opening tables saved by other programs: the formats behind
//! File → Import (and File → Open for anything that isn't `.cellar`).
//! Spreadsheet files go through `xlsx_convert` (calamine), text formats
//! through `domain::services::import`.

use std::path::Path;

use crate::domain::import as text;
use crate::domain::Workbook;
use crate::infrastructure::xlsx_convert::{self, ConversionReport};

/// Text files larger than this are refused instead of exhausting memory.
const MAX_TEXT_FILE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportFormat {
    /// .xlsx, .xlsm, .xlsb, .xls
    Excel,
    /// .ods (LibreOffice, OpenOffice, Google Sheets download)
    OpenDocument,
    /// .csv and other delimited text; the separator is detected.
    Csv,
    /// .tsv / .tab: always tab-separated.
    Tsv,
    /// .md: every pipe table becomes a sheet.
    Markdown,
    /// .json (array of records or rows).
    Json,
    /// .jsonl / .ndjson: one record per line.
    JsonLines,
}

impl ImportFormat {
    /// In File → Import order.
    pub const ALL: [ImportFormat; 7] = [
        ImportFormat::Excel,
        ImportFormat::OpenDocument,
        ImportFormat::Csv,
        ImportFormat::Tsv,
        ImportFormat::Markdown,
        ImportFormat::Json,
        ImportFormat::JsonLines,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ImportFormat::Excel => "Excel workbook",
            ImportFormat::OpenDocument => "OpenDocument spreadsheet",
            ImportFormat::Csv => "CSV / delimited text",
            ImportFormat::Tsv => "Tab-separated text",
            ImportFormat::Markdown => "Markdown tables",
            ImportFormat::Json => "JSON",
            ImportFormat::JsonLines => "JSON Lines",
        }
    }

    /// Lowercase, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ImportFormat::Excel => &["xlsx", "xlsm", "xlsb", "xls"],
            ImportFormat::OpenDocument => &["ods"],
            ImportFormat::Csv => &["csv", "txt"],
            ImportFormat::Tsv => &["tsv", "tab"],
            ImportFormat::Markdown => &["md", "markdown"],
            ImportFormat::Json => &["json"],
            ImportFormat::JsonLines => &["jsonl", "ndjson"],
        }
    }

    /// Menu label, e.g. "Excel workbook (.xlsx, .xlsm, .xlsb, .xls)".
    pub fn label(self) -> String {
        let exts: Vec<String> = self.extensions().iter().map(|e| format!(".{}", e)).collect();
        format!("{} ({})", self.name(), exts.join(", "))
    }

    pub fn from_path(path: &Path) -> Option<ImportFormat> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        Self::ALL.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Every importable extension, for the Open dialog's filter.
    pub fn all_extensions() -> Vec<&'static str> {
        Self::ALL.iter().flat_map(|f| f.extensions().iter().copied()).collect()
    }
}

/// A workbook read from another format. Spreadsheet files also carry the
/// conversion report (formula differences, what wasn't carried over).
pub struct Imported {
    pub workbook: Workbook,
    pub report: Option<ConversionReport>,
}

/// Read `path` as `format`. The workbook has no `.cellar` file yet.
pub fn import_file(path: &Path, format: ImportFormat) -> Result<Imported, String> {
    let p = path.to_string_lossy();
    let mut format = format;
    if matches!(format, ImportFormat::Excel | ImportFormat::OpenDocument) {
        match xlsx_convert::convert_xlsx(&p) {
            Ok((workbook, report)) => return Ok(Imported { workbook, report: Some(report) }),
            Err(e) => match sniff_misnamed(path) {
                // Web exports often save CSV or tab-separated text as `.xls`.
                Misnamed::Text => format = ImportFormat::Csv,
                Misnamed::Markup => {
                    return Err(format!(
                        "this is a web page (HTML/XML) saved with a .{} extension, not a real \
                         spreadsheet; open it in Excel or a browser and save it as .xlsx or .csv",
                        path.extension().unwrap_or_default().to_string_lossy()
                    ));
                }
                Misnamed::No => {
                    return Err(format!(
                        "not a readable {} — the file may be damaged or in a different format ({})",
                        format.name(),
                        e
                    ));
                }
            },
        }
    }
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAX_TEXT_FILE_BYTES
    {
        return Err(format!("file too large: {} bytes (limit {})", meta.len(), MAX_TEXT_FILE_BYTES));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let content = text::decode_text(&bytes);
    let sheets: Vec<text::NamedSheet> = match format {
        ImportFormat::Csv => vec![(None, text::parse_delimited(&content, text::sniff_delimiter(&content))?)],
        ImportFormat::Tsv => vec![(None, text::parse_delimited(&content, b'\t')?)],
        ImportFormat::Markdown => {
            let tables = text::parse_markdown_tables(&content);
            if tables.is_empty() {
                return Err("no Markdown tables found".to_string());
            }
            tables
        }
        ImportFormat::Json => text::parse_json_tables(&content)?,
        ImportFormat::JsonLines => vec![(None, text::parse_json_lines(&content)?)],
        ImportFormat::Excel | ImportFormat::OpenDocument => unreachable!(),
    };
    Ok(Imported { workbook: workbook_from_sheets(sheets), report: None })
}

enum Misnamed {
    /// Plain text: import it as delimited text.
    Text,
    /// HTML or XML: can't be imported, but say what it is.
    Markup,
    /// A real (if damaged) spreadsheet, or unreadable.
    No,
}

/// What a file that failed to open as a spreadsheet really is, judging
/// from its first bytes.
fn sniff_misnamed(path: &Path) -> Misnamed {
    use std::io::Read;
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(4096).read_to_end(&mut head));
    // Zip (.xlsx, .ods) and OLE (.xls) signatures: a damaged spreadsheet.
    if read.is_err() || head.is_empty() || head.starts_with(b"PK\x03\x04") || head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Misnamed::No;
    }
    let text = text::decode_text(&head);
    let lower = text.trim_start().to_lowercase();
    if ["<html", "<!doctype", "<?xml", "<table", "<meta"].iter().any(|t| lower.starts_with(t)) || lower.contains("<html") {
        return Misnamed::Markup;
    }
    if text.contains('\0') || text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) {
        return Misnamed::No;
    }
    Misnamed::Text
}

/// Read `path` in the format its extension names.
pub fn import_path(path: &Path) -> Result<Imported, String> {
    let format = ImportFormat::from_path(path).ok_or_else(|| {
        format!("{} is not a format Cellar can open", path.display())
    })?;
    import_file(path, format)
}

/// One sheet per table; unnamed tables are Sheet1, Sheet2, …, and names
/// are made unique and valid.
fn workbook_from_sheets(sheets: Vec<text::NamedSheet>) -> Workbook {
    let mut wb = Workbook {
        sheets: Vec::new(),
        sheet_names: Vec::new(),
        sheet_ids: Vec::new(),
        next_sheet_id: 0,
        ..Workbook::default()
    };
    for (i, (name, sheet)) in sheets.into_iter().enumerate() {
        let name = unique_sheet_name(name.as_deref(), i, &wb.sheet_names);
        wb.add_sheet(name);
        *wb.sheets.last_mut().expect("just added") = sheet;
    }
    if wb.sheets.is_empty() {
        wb.add_sheet("Sheet1".to_string());
    }
    wb.active_sheet = 0;
    wb.build_dep_graph_from_scratch();
    wb
}

/// Excel's sheet-name rules: at most 31 characters, none of `[]:*?/\`.
fn unique_sheet_name(name: Option<&str>, index: usize, taken: &[String]) -> String {
    let cleaned: String = name
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    let cleaned = cleaned.trim().trim_matches('\'').to_string();
    let base = if cleaned.is_empty() { format!("Sheet{}", index + 1) } else { cleaned };
    let is_taken = |n: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if !is_taken(&base) {
        return base;
    }
    (2..)
        .map(|k| format!("{} ({})", base.chars().take(26).collect::<String>(), k))
        .find(|n| !is_taken(n))
        .expect("unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(ext: &str, content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(ext).tempfile().unwrap();
        f.write_all(content).unwrap();
        f
    }

    #[test]
    fn formats_by_extension() {
        assert_eq!(ImportFormat::from_path(Path::new("a.XLS")), Some(ImportFormat::Excel));
        assert_eq!(ImportFormat::from_path(Path::new("a.ods")), Some(ImportFormat::OpenDocument));
        assert_eq!(ImportFormat::from_path(Path::new("a.tab")), Some(ImportFormat::Tsv));
        assert_eq!(ImportFormat::from_path(Path::new("a.ndjson")), Some(ImportFormat::JsonLines));
        assert_eq!(ImportFormat::from_path(Path::new("a.cellar")), None);
        assert_eq!(ImportFormat::Excel.label(), "Excel workbook (.xlsx, .xlsm, .xlsb, .xls)");
    }

    #[test]
    fn tsv_keeps_commas_inside_fields() {
        let f = write_temp(".tsv", b"Name\tAmount\nSmith, J.\t1,5\n");
        let wb = import_path(f.path()).unwrap().workbook;
        assert_eq!(wb.sheets[0].get_cell(1, 0).value, "Smith, J.");
        assert_eq!(wb.sheets[0].get_cell(1, 1).value, "1,5");
    }

    #[test]
    fn markdown_tables_become_named_unique_sheets() {
        let f = write_temp(".md", b"## Data\n|a|\n|-|\n|1|\n\n## Data\n|b|\n|-|\n|2|\n\n|c|\n|-|\n");
        let wb = import_path(f.path()).unwrap().workbook;
        assert_eq!(wb.sheet_names, ["Data", "Data (2)", "Sheet3"]);
        assert_eq!(wb.sheet_ids.iter().map(|id| id.0).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(wb.sheets[1].get_cell(1, 0).value, "2");
        assert!(import_path(write_temp(".md", b"# no tables").path()).is_err());
    }

    #[test]
    fn misnamed_and_damaged_spreadsheets() {
        // CSV text saved as .xls (common for web exports) is imported as text.
        let csv = write_temp(".xls", b"a;b
1;2
");
        assert_eq!(import_path(csv.path()).unwrap().workbook.sheets[0].get_cell(1, 1).value, "2");
        // HTML saved as .xls is explained, not imported.
        let html = write_temp(".xls", b"<html><table><tr><td>1</td></tr></table></html>");
        assert!(import_path(html.path()).err().unwrap().contains("web page"));
        // A broken zip is reported as a damaged file.
        let broken = write_temp(".ods", b"PKgarbage");
        assert!(import_path(broken.path()).err().unwrap().contains("not a readable OpenDocument"));
        let empty = write_temp(".xlsx", b"");
        assert!(import_path(empty.path()).err().unwrap().contains("not a readable Excel"));
        assert!(import_path(Path::new("does-not-exist.csv")).is_err());
    }

    #[test]
    fn json_numbers_compute() {
        let f = write_temp(".json", br#"[{"n": 2}, {"n": 3.5}]"#);
        let wb = import_path(f.path()).unwrap().workbook;
        let eval = crate::domain::FormulaEvaluator::new(&wb.sheets[0]);
        assert_eq!(eval.evaluate_formula("=SUM(A2:A3)"), "5.5");
    }
}
