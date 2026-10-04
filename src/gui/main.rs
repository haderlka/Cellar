//! Cellar — a desktop spreadsheet that keeps workbooks as readable,
//! diff- and merge-friendly text. Built with egui/eframe on the tshts
//! calculation engine (see the `cellar` library crate).
//!
//! Runs on macOS, Windows and Linux.
//!
//! Usage:
//!   cellar [FILE]                         open a .cellar, .xlsx or .csv file
//!
//! Batch commands (no window):
//!   cellar --convert IN.xlsx [OUT.cellar] convert Excel → .cellar (with report)
//!   cellar --export-md IN [OUT.md]        all sheets as Markdown tables (values only)
//!   cellar --export-charts IN [DIR] [--svg]
//!                                         every chart as PNG (or SVG) into DIR
//! IN may be a .cellar or .xlsx file.

// Windows: build as a GUI program so double-clicking cellar.exe doesn't open
// a console window. The batch commands attach to the console they were
// started from instead (`attach_parent_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod charts;
mod dialogs;
mod grid;
mod open_files;
mod pivot_ui;
mod shortcuts;
mod sidebar;

use eframe::egui;
use cellar::infrastructure::{atomic, chart_image, fetcher, xlsx_convert, FileRepository};

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("--convert" | "--export-md" | "--export-charts")
    ) {
        attach_parent_console();
    }
    match args.first().map(String::as_str) {
        Some("--convert") => std::process::exit(convert_cli(&args[1..])),
        Some("--export-md") => std::process::exit(export_md_cli(&args[1..])),
        Some("--export-charts") => std::process::exit(export_charts_cli(&args[1..])),
        _ => {}
    }

    // GET() needs the HTTP fetcher and CSV export the atomic file writer;
    // the domain layer reaches both through traits installed here.
    fetcher::install_as_http_fetcher();
    atomic::install_as_file_writer();
    // macOS delivers files opened from Finder as events, not arguments.
    open_files::install();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Cellar")
            .with_app_id("cellar")
            .with_icon(app::window_icon())
            .with_inner_size([1100.0, 700.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    let file = args.into_iter().find(|a| !a.starts_with("--"));
    eframe::run_native(
        "cellar",
        options,
        Box::new(move |cc| Ok(Box::new(app::GuiApp::new(cc, file)))),
    )
}

/// Windows GUI programs start without a console, so `println!` from the
/// batch commands would go nowhere. Attach to the console of the shell that
/// started us (if any); redirected output (`> file`, pipes) is unaffected.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 call with no pointers; failure (no parent
    // console) just leaves output unattached.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// `--convert IN [OUT.cellar]`: batch conversion for scripts. An `.xlsx`
/// input is converted and its verification report printed; a `.cellar`
/// input is rewritten in the canonical layout. Exit code 0 on success (even
/// with formula differences, which are listed), 1 on failure.
fn convert_cli(args: &[String]) -> i32 {
    let Some(input) = args.first() else {
        eprintln!("usage: cellar --convert IN.xlsx [OUT.cellar]");
        return 1;
    };
    let output = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| app::sibling_with_extension(input, "cellar"));
    let result = if input.to_lowercase().ends_with(".xlsx") {
        xlsx_convert::convert_xlsx_file(input, &output).map(|report| print!("{}", report.summary()))
    } else {
        load_any(input).and_then(|wb| FileRepository::save_workbook(&wb, &output).map(|_| ()))
    };
    match result {
        Ok(()) => {
            println!("Saved {}", output);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

/// Load a .cellar or (converting it) an .xlsx file for the batch commands.
fn load_any(path: &str) -> Result<cellar::domain::Workbook, String> {
    if path.to_lowercase().ends_with(".xlsx") {
        xlsx_convert::convert_xlsx(path).map(|(wb, _)| wb)
    } else {
        FileRepository::load_workbook(path).map(|(wb, _)| wb)
    }
}

/// `--export-md IN [OUT.md]`: every non-empty sheet as a Markdown table of
/// computed values (no formulas, no formatting).
fn export_md_cli(args: &[String]) -> i32 {
    let Some(input) = args.first() else {
        eprintln!("usage: cellar --export-md IN [OUT.md]");
        return 1;
    };
    let output = args.get(1).cloned().unwrap_or_else(|| app::sibling_with_extension(input, "md"));
    let result = load_any(input).and_then(|wb| {
        let md = cellar::domain::workbook_to_markdown(&wb);
        atomic::atomic_write(&output, md.as_bytes()).map_err(|e| e.to_string())
    });
    match result {
        Ok(()) => {
            println!("Saved {}", output);
            0
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}

/// `--export-charts IN [DIR] [--svg]`: every chart of every sheet as an image.
fn export_charts_cli(args: &[String]) -> i32 {
    let svg = args.iter().any(|a| a == "--svg");
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let Some(input) = positional.first() else {
        eprintln!("usage: cellar --export-charts IN [DIR] [--svg]");
        return 1;
    };
    let dir = positional.get(1).map(|d| std::path::PathBuf::from(d.as_str())).unwrap_or_else(|| {
        let p = std::path::Path::new(input.as_str());
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        p.with_file_name(format!("{} charts", stem))
    });
    let result = load_any(input).and_then(|wb| chart_image::export_all_charts(&wb, &dir, if svg { "svg" } else { "png" }));
    match result {
        Ok(report) => {
            for p in &report.written {
                println!("Saved {}", p.display());
            }
            for (chart, why) in &report.failed {
                eprintln!("skipped {}: {}", chart, why);
            }
            i32::from(report.written.is_empty() && !report.failed.is_empty())
        }
        Err(e) => {
            eprintln!("error: {}", e);
            1
        }
    }
}
