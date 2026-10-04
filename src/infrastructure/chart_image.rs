//! Render charts to image files (PNG or SVG) without a window, using
//! `plotters`. PNG text uses the Ubuntu font bundled with egui, so no
//! system fonts are needed; SVG text names a generic `sans-serif` font
//! that every viewer has.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Once;

use plotters::coord::Shift;
use plotters::prelude::*;
use plotters::style::text_anchor::{HPos, Pos, VPos};

use crate::domain::{resolve_chart_data, ChartData, ChartSpec, ChartType, Workbook};

/// Default image size in pixels.
pub const DEFAULT_SIZE: (u32, u32) = (1200, 700);

/// Tableau 10, the same palette the GUI uses.
pub const SERIES_RGB: [(u8, u8, u8); 8] = [
    (78, 121, 167),
    (242, 142, 43),
    (225, 87, 89),
    (118, 183, 178),
    (89, 161, 79),
    (237, 201, 72),
    (176, 122, 161),
    (255, 157, 167),
];

const FONT: &str = "sans-serif";

fn color(i: usize) -> RGBColor {
    let (r, g, b) = SERIES_RGB[i % SERIES_RGB.len()];
    RGBColor(r, g, b)
}

fn ensure_font() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Can only fail on a malformed font; the bundled one is fine.
        let _ = plotters::style::register_font(FONT, FontStyle::Normal, epaint_default_fonts::UBUNTU_LIGHT);
    });
}

fn err<E: std::fmt::Debug>(e: E) -> String {
    format!("chart rendering failed: {:?}", e)
}

/// Write `data` drawn as `spec` to `path`. The format follows the file
/// extension: `.svg` → SVG, anything else → PNG.
pub fn export_chart(spec: &ChartSpec, data: &ChartData, path: &Path, size: (u32, u32)) -> Result<(), String> {
    if data.series.iter().all(|(_, v)| v.iter().all(Option::is_none)) {
        return Err(format!("\"{}\" has no numeric data", spec.title));
    }
    ensure_font();
    let svg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    if svg {
        let root = SVGBackend::new(path, size).into_drawing_area();
        draw(&root, spec, data)?;
        root.present().map_err(err)
    } else {
        let root = BitMapBackend::new(path, size).into_drawing_area();
        draw(&root, spec, data)?;
        root.present().map_err(err)
    }
}

fn draw<DB: DrawingBackend>(root: &DrawingArea<DB, Shift>, spec: &ChartSpec, data: &ChartData) -> Result<(), String> {
    root.fill(&WHITE).map_err(err)?;
    let root = root.margin(16, 8, 8, 8);
    let area = root.titled(&spec.title, (FONT, 30).into_font().color(&BLACK)).map_err(err)?;
    match spec.chart_type {
        ChartType::Pie => draw_pie(&area, data),
        ChartType::Scatter => draw_scatter(&area, data),
        _ => draw_category(&area, spec.chart_type, data),
    }
}

fn y_range(data: &ChartData, include_zero: bool) -> (f64, f64) {
    let vals = data.series.iter().flat_map(|(_, v)| v.iter().flatten().copied());
    let (mut lo, mut hi) = vals.fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), y| (l.min(y), h.max(y)));
    if include_zero {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < f64::EPSILON {
        return (lo - 1.0, hi + 1.0);
    }
    let pad = (hi - lo) * 0.06;
    (if include_zero && lo == 0.0 { 0.0 } else { lo - pad }, hi + pad)
}

fn fmt_tick(v: &f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    if r.fract() == 0.0 { format!("{}", r as i64) } else { format!("{}", r) }
}

/// Column and line charts: one slot per category.
fn draw_category<DB: DrawingBackend>(area: &DrawingArea<DB, Shift>, kind: ChartType, data: &ChartData) -> Result<(), String> {
    let n = data.series.iter().map(|(_, v)| v.len()).max().unwrap_or(0).max(1);
    let (lo, hi) = y_range(data, kind == ChartType::Bar);
    let mut chart = ChartBuilder::on(area)
        .margin(20)
        .x_label_area_size(56)
        .y_label_area_size(80)
        .build_cartesian_2d(-0.5f64..(n as f64 - 0.5), lo..hi)
        .map_err(err)?;
    chart
        .configure_mesh()
        .disable_x_mesh()
        .x_labels(0)
        .y_label_formatter(&fmt_tick)
        .label_style((FONT, 18))
        .light_line_style(WHITE.mix(0.0))
        .bold_line_style(BLACK.mix(0.12))
        .draw()
        .map_err(err)?;

    let count = data.series.len().max(1) as f64;
    let width = 0.8 / count;
    for (si, (name, values)) in data.series.iter().enumerate() {
        let c = color(si);
        let anno = if kind == ChartType::Bar {
            chart
                .draw_series(values.iter().enumerate().filter_map(|(i, v)| {
                    let x0 = i as f64 - 0.4 + si as f64 * width;
                    v.map(|y| Rectangle::new([(x0 + width * 0.04, lo.max(0.0).min(y)), (x0 + width * 0.96, y)], c.filled()))
                }))
                .map_err(err)?
        } else {
            let pts: Vec<(f64, f64)> = values.iter().enumerate().filter_map(|(i, v)| v.map(|y| (i as f64, y))).collect();
            chart
                .draw_series(pts.iter().map(|p| Circle::new(*p, 4, c.filled())))
                .map_err(err)?;
            chart.draw_series(LineSeries::new(pts, c.stroke_width(3))).map_err(err)?
        };
        anno.label(name.clone())
            .legend(move |(x, y)| Rectangle::new([(x, y - 7), (x + 14, y + 7)], c.filled()));
    }

    // Category labels, thinned when crowded, drawn under each slot.
    let step = n.div_ceil(30);
    let style = TextStyle::from((FONT, 18).into_font()).pos(Pos::new(HPos::Center, VPos::Top));
    for i in (0..n).step_by(step) {
        let label = data.categories.get(i).cloned().unwrap_or_else(|| (i + 1).to_string());
        let (px, py) = chart.backend_coord(&(i as f64, lo));
        area.draw(&Text::new(label, (px, py + 8), style.clone())).map_err(err)?;
    }
    legend(&mut chart, SeriesLabelPosition::UpperRight)
}

fn legend<'a, DB: DrawingBackend + 'a, CT: plotters::coord::CoordTranslate>(
    chart: &mut ChartContext<'a, DB, CT>,
    position: SeriesLabelPosition,
) -> Result<(), String> {
    chart
        .configure_series_labels()
        .position(position)
        .background_style(WHITE.mix(0.9))
        .border_style(BLACK.mix(0.25))
        .label_font((FONT, 18))
        .draw()
        .map_err(err)
}

fn draw_scatter<DB: DrawingBackend>(area: &DrawingArea<DB, Shift>, data: &ChartData) -> Result<(), String> {
    // X from the category range when it's numeric, else the point index.
    let xs: Vec<f64> = (0..data.series.iter().map(|(_, v)| v.len()).max().unwrap_or(0))
        .map(|i| data.categories.get(i).and_then(|c| c.trim().parse().ok()).unwrap_or(i as f64))
        .collect();
    let (xlo, xhi) = xs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| (l.min(*x), h.max(*x)));
    let (xlo, xhi) = if xlo.is_finite() && xhi > xlo { (xlo, xhi) } else { (0.0, 1.0) };
    let pad = (xhi - xlo) * 0.05;
    let (lo, hi) = y_range(data, false);
    let mut chart = ChartBuilder::on(area)
        .margin(20)
        .x_label_area_size(56)
        .y_label_area_size(80)
        .build_cartesian_2d((xlo - pad)..(xhi + pad), lo..hi)
        .map_err(err)?;
    chart
        .configure_mesh()
        .x_label_formatter(&fmt_tick)
        .y_label_formatter(&fmt_tick)
        .label_style((FONT, 18))
        .light_line_style(WHITE.mix(0.0))
        .bold_line_style(BLACK.mix(0.12))
        .draw()
        .map_err(err)?;
    for (si, (name, values)) in data.series.iter().enumerate() {
        let c = color(si);
        chart
            .draw_series(values.iter().zip(&xs).filter_map(|(v, x)| v.map(|y| Circle::new((*x, y), 5, c.filled()))))
            .map_err(err)?
            .label(name.clone())
            .legend(move |(x, y)| Circle::new((x + 7, y), 5, c.filled()));
    }
    // Scatter data usually rises to the right, leaving this corner free.
    legend(&mut chart, SeriesLabelPosition::LowerRight)
}

fn draw_pie<DB: DrawingBackend>(area: &DrawingArea<DB, Shift>, data: &ChartData) -> Result<(), String> {
    let values = data.series.first().map(|(_, v)| v.as_slice()).unwrap_or(&[]);
    let slices: Vec<(String, f64)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let v = (*v).filter(|v| *v > 0.0)?;
            Some((data.categories.get(i).cloned().unwrap_or_else(|| (i + 1).to_string()), v))
        })
        .collect();
    let total: f64 = slices.iter().map(|(_, v)| v).sum();
    if total <= 0.0 {
        return Err("a pie chart needs positive values".into());
    }
    let (w, h) = area.dim_in_pixel();
    let radius = (h as f64 / 2.0 - 30.0).min(w as f64 * 0.3).max(20.0);
    let center = (radius + 40.0, h as f64 / 2.0);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (_, v)) in slices.iter().enumerate() {
        let sweep = v / total * std::f64::consts::TAU;
        let steps = ((sweep / std::f64::consts::TAU) * 180.0).ceil().max(2.0) as usize;
        let mut pts = vec![(center.0 as i32, center.1 as i32)];
        for s in 0..=steps {
            let a = angle + sweep * s as f64 / steps as f64;
            pts.push(((center.0 + radius * a.cos()) as i32, (center.1 + radius * a.sin()) as i32));
        }
        area.draw(&Polygon::new(pts.clone(), color(i).filled())).map_err(err)?;
        area.draw(&PathElement::new(pts, WHITE.stroke_width(2))).map_err(err)?;
        angle += sweep;
    }
    // Legend with percentages.
    let x = (center.0 + radius + 40.0) as i32;
    let mut y = (center.1 - slices.len() as f64 * 15.0).max(10.0) as i32;
    for (i, (label, v)) in slices.iter().enumerate() {
        area.draw(&Rectangle::new([(x, y), (x + 16, y + 16)], color(i).filled())).map_err(err)?;
        area.draw(&Text::new(format!("{}  {:.1}%", label, v / total * 100.0), (x + 26, y - 2), (FONT, 20))).map_err(err)?;
        y += 30;
    }
    Ok(())
}

/// A filename-safe version of `s`.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_.()&+,'".contains(c) { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() { "chart".into() } else { trimmed }
}

/// Outcome of exporting every chart in a workbook.
#[derive(Debug, Default)]
pub struct ChartExportReport {
    pub written: Vec<PathBuf>,
    /// (chart, reason) for charts that couldn't be rendered.
    pub failed: Vec<(String, String)>,
}

/// Export every chart on every sheet into `dir` as `<sheet> - <title>.<ext>`.
pub fn export_all_charts(wb: &Workbook, dir: &Path, ext: &str) -> Result<ChartExportReport, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    let mut report = ChartExportReport::default();
    let mut used: HashSet<String> = HashSet::new();
    for (si, sheet) in wb.sheets.iter().enumerate() {
        for chart in &sheet.charts {
            let base = sanitize(&format!("{} - {}", wb.sheet_names[si], chart.title));
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{} ({})", base, n);
                n += 1;
            }
            let path = dir.join(format!("{}.{}", name, ext));
            let label = format!("{} / {}", wb.sheet_names[si], chart.title);
            match resolve_chart_data(wb, si, chart).and_then(|d| export_chart(chart, &d, &path, DEFAULT_SIZE)) {
                Ok(()) => report.written.push(path),
                Err(e) => report.failed.push((label, e)),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChartSeries;

    fn data() -> ChartData {
        ChartData {
            categories: vec!["North".into(), "South".into(), "West".into()],
            series: vec![
                ("Jan".into(), vec![Some(1200.5), Some(950.0), Some(640.0)]),
                ("Feb".into(), vec![Some(1310.25), None, Some(700.75)]),
            ],
        }
    }

    fn spec(kind: ChartType) -> ChartSpec {
        ChartSpec {
            title: "Revenue".into(),
            chart_type: kind,
            pivot: None,
            categories: None,
            series: vec![ChartSeries { name: None, values: "B2:B4".into() }],
        }
    }

    #[test]
    fn renders_every_chart_type_to_png_and_svg() {
        let dir = tempfile::tempdir().unwrap();
        for kind in [ChartType::Bar, ChartType::Line, ChartType::Pie, ChartType::Scatter] {
            for ext in ["png", "svg"] {
                let path = dir.path().join(format!("{:?}.{}", kind, ext));
                export_chart(&spec(kind), &data(), &path, (600, 400)).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                if ext == "png" {
                    assert_eq!(&bytes[1..4], b"PNG", "{:?}", kind);
                } else {
                    let svg = String::from_utf8(bytes).unwrap();
                    assert!(svg.contains("<svg") && svg.contains("Revenue"), "{:?}", kind);
                }
            }
        }
    }

    #[test]
    fn empty_data_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let empty = ChartData { categories: vec![], series: vec![("x".into(), vec![None])] };
        assert!(export_chart(&spec(ChartType::Bar), &empty, &dir.path().join("x.png"), (100, 100)).is_err());
    }

    #[test]
    fn file_names_are_safe_and_unique() {
        assert_eq!(sanitize("Sales / Q1: 2026?"), "Sales _ Q1_ 2026_");
        assert_eq!(sanitize("..."), "chart");
    }
}
