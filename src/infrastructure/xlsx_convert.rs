//! Excel → `.cellar` conversion with a verification report.
//!
//! [`load_xlsx`](super::xlsx::load_xlsx) reads values and formulas. This
//! module adds what it skips (formatting, column widths, charts — see
//! [`super::xlsx_extras`]) and then checks the result: every formula is
//! recalculated with the Cellar engine and compared against the value Excel
//! cached in the file. Differences usually mean a function Cellar doesn't
//! implement (shows as `#NAME?`) or one that behaves differently, and are
//! listed in the [`ConversionReport`] so the user knows which cells to check.

use std::collections::HashMap;

use crate::domain::{Spreadsheet, Workbook};

/// Cap on mismatches kept per sheet; the total is still counted.
const MAX_LISTED_MISMATCHES: usize = 500;

#[derive(Debug, Clone)]
pub struct FormulaMismatch {
    /// Cell address such as `B7`.
    pub cell: String,
    pub formula: String,
    /// Value Excel cached in the file.
    pub excel: String,
    /// Value Cellar computes.
    pub cellar: String,
}

#[derive(Debug, Clone, Default)]
pub struct SheetReport {
    pub name: String,
    pub cells: usize,
    pub formulas: usize,
    pub formatted_cells: usize,
    pub charts: usize,
    pub mismatch_count: usize,
    pub mismatches: Vec<FormulaMismatch>,
}

#[derive(Debug, Clone, Default)]
pub struct ConversionReport {
    pub source: String,
    pub sheets: Vec<SheetReport>,
    /// Notes about pivots and things that were not converted.
    pub warnings: Vec<String>,
    pub pivots_converted: usize,
}

impl ConversionReport {
    pub fn total_mismatches(&self) -> usize {
        self.sheets.iter().map(|s| s.mismatch_count).sum()
    }

    /// Multi-line plain-text summary (used by the CLI and copied by the GUI).
    pub fn summary(&self) -> String {
        let mut s = format!("Converted {}\n", self.source);
        for sh in &self.sheets {
            s.push_str(&format!(
                "  {}: {} cells, {} formulas, {} formatted, {} charts, {} formula differences\n",
                sh.name, sh.cells, sh.formulas, sh.formatted_cells, sh.charts, sh.mismatch_count
            ));
            for m in &sh.mismatches {
                s.push_str(&format!(
                    "    {}  {}  Excel: {}  Cellar: {}\n",
                    m.cell, m.formula, m.excel, m.cellar
                ));
            }
            if sh.mismatch_count > sh.mismatches.len() {
                s.push_str(&format!(
                    "    … and {} more\n",
                    sh.mismatch_count - sh.mismatches.len()
                ));
            }
        }
        for w in &self.warnings {
            s.push_str(&format!("  warning: {}\n", w));
        }
        s
    }
}

/// Read a spreadsheet file (.xlsx, .xlsm, .xlsb, .xls, .ods) into a recalculated
/// workbook; formatting, charts and PivotTables come from .xlsx/.xlsm only.
pub fn convert_xlsx(path: &str) -> Result<(Workbook, ConversionReport), String> {
    // The readers (calamine and our XML parsing) see untrusted input; some
    // malformed files make calamine panic (e.g. overflowing cell
    // references). Report those as a damaged file instead of crashing.
    std::panic::catch_unwind(|| convert_xlsx_unguarded(path))
        .unwrap_or_else(|_| Err(format!("{} could not be read; the file may be damaged", path)))
}

fn convert_xlsx_unguarded(path: &str) -> Result<(Workbook, ConversionReport), String> {
    let mut wb = super::xlsx::load_xlsx(path)?;
    let mut report = ConversionReport { source: path.to_string(), ..Default::default() };

    // Formatting, charts and PivotTables are read from the Office Open XML
    // parts; .xls, .xlsb and .ods bring values and formulas only.
    let is_ooxml = [".xlsx", ".xlsm"].iter().any(|ext| path.to_lowercase().ends_with(ext));
    let extras = if is_ooxml {
        super::xlsx_extras::read_extras(path)
    } else {
        report.warnings.push(
            "Values and formulas were imported; formatting, charts and PivotTables are only \
             read from .xlsx files."
                .to_string(),
        );
        Ok(HashMap::new())
    };
    match extras {
        Ok(mut extras) => {
            for (idx, name) in wb.sheet_names.clone().iter().enumerate() {
                let Some(ex) = extras.remove(name) else { continue };
                let sheet = &mut wb.sheets[idx];
                for ((r, c), fmt) in ex.formats {
                    if let Some(cell) = sheet.cells.get_mut(&(r, c)) {
                        cell.format = Some(fmt);
                    }
                }
                for (c, w) in ex.column_widths {
                    if c < sheet.cols {
                        sheet.column_widths.insert(c, w);
                    }
                }
                sheet.charts = ex.charts;
                for pivot in ex.pivots {
                    match pivot {
                        Ok(p) => {
                            report.pivots_converted += 1;
                            report.warnings.push(format!(
                                "PivotTable \"{}\" on sheet \"{}\" is now a live PivotTable in the sidebar. \
                                 Excel's static copy of its output is still in {}; delete those cells if you \
                                 don't need them.",
                                p.spec.name, name, if p.location.is_empty() { "the sheet".to_string() } else { p.location.clone() }
                            ));
                            sheet.pivots.push(p.spec);
                        }
                        Err((pivot, why)) => report.warnings.push(format!(
                            "PivotTable \"{}\" on sheet \"{}\" could not be converted ({}); it was imported \
                             as static values. Recreate it with Data > PivotTable.",
                            pivot, name, why
                        )),
                    }
                }
            }
        }
        Err(e) => report
            .warnings
            .push(format!("Formatting and charts could not be read ({}); values were imported.", e)),
    }

    // Snapshot Excel's cached formula results, then recalc everything.
    let mut excel_values: HashMap<(usize, usize, usize), String> = HashMap::new();
    for (idx, sheet) in wb.sheets.iter().enumerate() {
        for (&(r, c), cd) in &sheet.cells {
            if cd.formula.is_some() {
                excel_values.insert((idx, r, c), cd.value.clone());
            }
        }
    }
    recalc_everything(&mut wb, &mut report);

    for (idx, sheet) in wb.sheets.iter().enumerate() {
        let mut sr = SheetReport {
            name: wb.sheet_names[idx].clone(),
            cells: sheet.cells.len(),
            formulas: sheet.cells.values().filter(|c| c.formula.is_some()).count(),
            formatted_cells: sheet.cells.values().filter(|c| c.format.is_some()).count(),
            charts: sheet.charts.len(),
            ..Default::default()
        };
        let mut keys: Vec<_> = sheet.cells.keys().copied().collect();
        keys.sort();
        for (r, c) in keys {
            let cd = &sheet.cells[&(r, c)];
            let (Some(formula), Some(excel)) = (&cd.formula, excel_values.get(&(idx, r, c))) else {
                continue;
            };
            if values_match(excel, &cd.value) {
                continue;
            }
            sr.mismatch_count += 1;
            if sr.mismatches.len() < MAX_LISTED_MISMATCHES {
                sr.mismatches.push(FormulaMismatch {
                    cell: format!("{}{}", Spreadsheet::column_label(c), r + 1),
                    formula: formula.clone(),
                    excel: excel.clone(),
                    cellar: cd.value.clone(),
                });
            }
        }
        report.sheets.push(sr);
    }
    Ok((wb, report))
}

/// Convert `input` (.xlsx) and save it as `output` (.cellar).
pub fn convert_xlsx_file(input: &str, output: &str) -> Result<ConversionReport, String> {
    let (wb, report) = convert_xlsx(input)?;
    super::FileRepository::save_workbook(&wb, output)?;
    Ok(report)
}

fn recalc_everything(wb: &mut Workbook, report: &mut ConversionReport) {
    for (idx, sheet) in wb.sheets.iter().enumerate() {
        let name = wb.sheet_names[idx].clone();
        for (&(r, c), cd) in &sheet.cells {
            if cd.formula.is_some() {
                wb.dirty.insert((name.clone(), r, c));
            }
        }
    }
    wb.build_dep_graph_from_scratch();
    wb.rebuild_cross_sheet_deps();
    if let Err(e) = wb.recalc_via_graph_result() {
        report.warnings.push(format!("Recalculation: {}", e));
    }
}

/// Excel's cached value vs. the Cellar result, allowing for float noise,
/// date rendering (ISO string vs. serial) and error spelling.
fn values_match(excel: &str, ours: &str) -> bool {
    let (excel, ours) = (excel.trim(), ours.trim());
    if excel == ours || excel.eq_ignore_ascii_case(ours) {
        return true;
    }
    // Both errors: calamine spells them differently (`#Div0!`).
    if excel.starts_with('#') && ours.starts_with('#') {
        return true;
    }
    if let (Ok(a), Ok(b)) = (excel.parse::<f64>(), ours.parse::<f64>()) {
        return (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    }
    // Empty-string results: Excel caches nothing, Cellar may give "" or 0.
    if excel.is_empty() && (ours.is_empty() || ours == "0") {
        return true;
    }
    // The importer renders Excel dates as ISO; Cellar may return a serial.
    if let Ok(serial) = ours.parse::<f64>()
        && excel.len() >= 10
        && excel.as_bytes()[4] == b'-'
    {
        let (y, m, d) = crate::domain::parser::serial_to_date_pub(serial);
        return excel.starts_with(&format!("{:04}-{:02}-{:02}", y, m, d));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_comparison_tolerates_representation_noise() {
        assert!(values_match("0.30000000000000004", "0.3"));
        assert!(values_match("TRUE", "true"));
        assert!(values_match("#Div0!", "#DIV/0!"));
        assert!(values_match("", ""));
        assert!(!values_match("42", "#NAME?"));
        assert!(!values_match("1", "2"));
    }
}
