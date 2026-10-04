//! Resolving a [`ChartSpec`] to the numbers it plots: from its cell ranges,
//! or, for a PivotChart, from its PivotTable's result. Shared by the GUI
//! (drawing) and the chart image exporter.

use crate::domain::models::{ChartSpec, Spreadsheet, Workbook};

use super::pivot::{compute_pivot, PivotData};

/// Category labels and named numeric series (`None` = no number there).
#[derive(Debug, Clone, Default)]
pub struct ChartData {
    pub categories: Vec<String>,
    pub series: Vec<(String, Vec<Option<f64>>)>,
}

/// Values of a range like `B2:B9`, `B2` or `'Other sheet'!B2:B9`, in
/// row-major order. Unqualified refs resolve on sheet `host`.
pub fn read_range(wb: &Workbook, host: usize, reference: &str) -> Vec<String> {
    let (sheet_idx, range) = match reference.rsplit_once('!') {
        Some((name, range)) => {
            let name = name.trim().trim_matches('\'').replace("''", "'");
            match wb.sheet_names.iter().position(|n| n.eq_ignore_ascii_case(&name)) {
                Some(i) => (i, range),
                None => return Vec::new(),
            }
        }
        None => (host, reference),
    };
    let range = range.trim().replace('$', "").to_uppercase();
    let (a, b) = range.split_once(':').unwrap_or((&range, &range));
    let (Some((r0, c0)), Some((r1, c1))) =
        (Spreadsheet::parse_cell_reference(a), Spreadsheet::parse_cell_reference(b))
    else {
        return Vec::new();
    };
    let Some(sheet) = wb.sheets.get(sheet_idx) else { return Vec::new() };
    // Cap so a whole-column reference can't stall rendering.
    let mut out = Vec::new();
    for r in r0.min(r1)..=r0.max(r1) {
        for c in c0.min(c1)..=c0.max(c1) {
            out.push(sheet.cells.get(&(r, c)).map(|cd| cd.value.clone()).unwrap_or_default());
            if out.len() >= 10_000 {
                return out;
            }
        }
    }
    out
}

pub fn numbers(values: &[String]) -> Vec<Option<f64>> {
    values.iter().map(|v| v.trim().parse::<f64>().ok()).collect()
}

/// Data for a range-based (non-pivot) chart on sheet `host`.
pub fn range_chart_data(wb: &Workbook, host: usize, spec: &ChartSpec) -> ChartData {
    let categories = spec.categories.as_deref().map(|r| read_range(wb, host, r)).unwrap_or_default();
    let series = spec
        .series
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let name = s
                .name
                .as_deref()
                .map(|n| {
                    // A name may itself be a cell reference (from Excel).
                    if Spreadsheet::parse_cell_reference(&n.replace('$', "")).is_some() || n.contains('!') {
                        read_range(wb, host, n).into_iter().next().unwrap_or_else(|| n.to_string())
                    } else {
                        n.to_string()
                    }
                })
                .unwrap_or_else(|| format!("Series {}", i + 1));
            (name, numbers(&read_range(wb, host, &s.values)))
        })
        .collect();
    ChartData { categories, series }
}

/// Data for any chart on sheet `host`, computing its PivotTable if needed.
pub fn resolve_chart_data(wb: &Workbook, host: usize, spec: &ChartSpec) -> Result<ChartData, String> {
    let Some(pivot) = &spec.pivot else {
        return Ok(range_chart_data(wb, host, spec));
    };
    let sheet = wb.sheets.get(host).ok_or("Sheet missing")?;
    let p = sheet
        .pivots
        .iter()
        .find(|p| &p.name == pivot)
        .ok_or_else(|| format!("PivotTable {} not found", pivot))?;
    let data = PivotData::read(wb, host, &p.source)?;
    let out = compute_pivot(p, &data)?;
    if out.chart.series.is_empty() {
        return Err("Add a field to the PivotTable's Values area to chart it.".into());
    }
    Ok(ChartData { categories: out.chart.categories, series: out.chart.series })
}
