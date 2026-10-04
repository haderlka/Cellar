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
    let (last_r, last_c) = sheet.last_cell();
    let (r0, r1) = (r0.min(r1), r0.max(r1));
    let (c0, c1) = (c0.min(c1), c0.max(c1));
    let (r1, c1) = (r1.min(last_r.max(r0)), c1.min(last_c.max(c0)));
    // Cap so a whole-column reference can't stall rendering.
    let mut out = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            out.push(sheet.cells.get(&(r, c)).map(|cd| cd.value.clone()).unwrap_or_default());
            if out.len() >= 10_000 {
                return out;
            }
        }
    }
    out
}

pub fn numbers(values: &[String]) -> Vec<Option<f64>> {
    values.iter().map(|v| plottable(v.trim().parse::<f64>().ok())).collect()
}

/// Largest magnitude a chart plots. Axis code (egui_plot, plotters) works
/// with the span `max - min` plus padding, which overflows to infinity
/// near `f64::MAX` and makes tick generation loop forever.
pub const PLOT_LIMIT: f64 = 1e300;

/// A value as a chart can draw it: NaN/infinite are gaps (Rust also parses
/// "NaN" and "inf" text), huge magnitudes are clamped to `PLOT_LIMIT`.
pub fn plottable(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite()).map(|x| x.clamp(-PLOT_LIMIT, PLOT_LIMIT))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_numbers_skip_nan_and_clamp_huge_values() {
        let v: Vec<String> = ["1", "NaN", "inf", "-infinity", "1e308", "-1e308", "x", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            numbers(&v),
            vec![Some(1.0), None, None, None, Some(PLOT_LIMIT), Some(-PLOT_LIMIT), None, None]
        );
    }
}
