# CLAUDE.md

Guidance for Claude Code when working in this repository.

Cellar is a desktop spreadsheet (egui/eframe) whose workbooks are saved as
readable, diff- and merge-friendly JSON. It is built on
[tshts](https://github.com/SamuelSchlesinger/tshts): the formula language,
calculation engine, workbook model, editing session and Excel/CSV I/O come
from tshts; the GUI, PivotTables, charts, canonical file layout, Excel
converter extensions, exports and fill handle are Cellar's. Keep the
credits in README.md and LICENSE intact.

## Commands

```bash
cargo run --release               # build and start the app
cargo test                        # unit tests + tests/scenarios.rs
cargo clippy --all-targets        # must stay warning-free (CI uses -D warnings)
cargo bench --bench calc_engine   # recalculation benchmarks
cargo run --example render_icons  # regenerate assets/icon-*.png and icon.ico from assets/logo.svg
```

Do not commit or push; the maintainer reviews changes first.

## Layout

```
src/
├── domain/                  # pure logic, no I/O (see Layer dependencies)
│   ├── models/              # Spreadsheet, Workbook, CellData, CellStyle/CellFormat, ChartSpec,
│   │                        #   PivotSpec (pivot.rs), WorkbookGraph (dep_graph.rs), refs,
│   │                        #   formula_refs.rs (references + positions in formula text)
│   ├── parser/              # formula lexer, recursive-descent parser, AST evaluator,
│   │   └── registry_fns/    #   built-in functions by category (numeric, string, date, …)
│   └── services/            # FormulaEvaluator, recalc executors, CsvExporter,
│                            #   pivot.rs (PivotTable engine), chart_data.rs (resolve chart data),
│                            #   markdown.rs (table export), import.rs (CSV/TSV/Markdown/JSON
│                            #   text → sheets), autofill_pattern.rs (series detection)
├── application/state/       # App: the editing session used by the GUI
│                            #   mod.rs (App, AppMode), undo.rs, editing.rs, clipboard.rs,
│                            #   fill.rs (fill handle), formatting.rs, charts.rs, pivots.rs,
│                            #   navigation.rs, search.rs, io.rs, command/ (sheet commands)
├── infrastructure/          # all I/O
│   ├── persistence.rs       #   load/save workbooks
│   ├── canonical_json.rs    #   the one-line-per-cell .cellar layout
│   ├── xlsx.rs              #   .xlsx import (calamine) / export (hand-written)
│   ├── xlsx_extras.rs       #   .xlsx formatting, widths, charts, pivots (quick-xml)
│   ├── xlsx_pivot.rs        #   Excel PivotTable → PivotSpec
│   ├── xlsx_convert.rs      #   Excel/ODS → Cellar conversion + verification report
│   ├── import.rs            #   ImportFormat: File → Import / Open of non-.cellar files
│   ├── chart_image.rs       #   charts → PNG/SVG (plotters)
│   ├── app_dirs.rs          #   where Cellar stores its own files (config, cache)
│   ├── uninstall.rs         #   `cellar --uninstall`: data, terminal links, program
│   ├── recent.rs, sidecar.rs (rich clipboard), fetcher.rs (GET), atomic.rs, autosave.rs
└── gui/                     # binary `cellar`
    ├── main.rs              #   startup, window icon, batch commands
    ├── app.rs               #   GuiApp: menus, formula bar, tabs, keys, files
    ├── grid.rs              #   virtualized grid, selection, fill handle, cell editor
    ├── sidebar.rs           #   PivotTable and chart cards
    ├── pivot_ui.rs          #   PivotTable Fields pane, settings dialogs, table rendering
    ├── charts.rs            #   chart drawing (egui_plot), chart dialog, export
    ├── dialogs.rs           #   import report, rename, help, about
    └── open_files.rs        #   macOS: files opened from Finder (Apple Event handler)
tests/
├── scenarios.rs             # end-to-end financial models vs. Rust ground truth
└── common/                  # Harness (drives App) + scenarios/
assets/                      # logo.svg, rendered icons, icon.ico (embedded by build.rs)
docs/release.md              # how to cut a release (.github/workflows/release.yml)
```

## Invariants

**Layer dependencies.** Domain must not use infrastructure. The two things
it needs from outside (HTTP for `GET()`, atomic file writes for CSV export)
go through `domain::services::HttpFetcher` / `FileWriter`, whose
implementations `src/gui/main.rs` installs at startup. Tests that need them
install them too (`enable_network_for_test`, `install_file_writer_for_test`).

**Mutations go through the workbook.** `Workbook::set_cell_on_active`,
`clear_cell_on_active`, `write_cells_on_active`, `clear_cells_on_active`
write, update the dependency graph and recalculate. `Spreadsheet::set_cell`
alone skips propagation. From the GUI use `App` methods
(`set_cell_with_undo`, `set_many_with_undo`, `fill`, `add_pivot`, …) so the
change is undoable; whole-workbook edits use `with_snapshot_undo`.

**Editing a cell from the GUI** sets `App::input`, `AppMode::Editing` and
calls `finish_editing*`, which evaluates the formula, rejects cycles and
records undo. Sheet operations (new/rename/delete sheet, formats) run
through `App::execute_command` with the command text in `command_input`.

**File format.** `.cellar` files are serde JSON of `Workbook`, written by
`canonical_json::to_canonical_string` (sorted keys, cells sorted and one per
line, default fields dropped). Values at their default are never written:
every persisted field with a default gets `#[serde(default)]` (or
`default = "fn"`) plus `skip_serializing_if` (`Vec::is_empty`,
`Option::is_none`, `models::is_default`, or an `is_default_x` fn for
non-`Default` values), so files only hold what the user changed and old
files keep loading. This applies to new fields and new structs too.
Renames need a migration step in `domain::models::migrate_workbook_json`.

**App data.** Everything Cellar stores for itself (settings, caches,
state) goes under `infrastructure::app_dirs::config_dir()` or `cache_dir()`,
so `cellar --uninstall` removes it. A location that can't live there must be
added to `app_dirs::data_locations`. `app_dirs::BUNDLE_ID` must match
`CFBundleIdentifier` in `.github/workflows/release.yml`.

**PivotTables and charts are definitions.** `PivotSpec` / `ChartSpec` are
stored per sheet; results are computed when shown (`services::pivot`,
`services::chart_data`). PivotCharts reference their pivot by name;
renaming or removing a pivot updates/removes its charts (`App::replace_pivot`,
`App::remove_pivot`).

**Formulas.** Built-ins are registered per category in
`domain/parser/registry_fns/`; special forms (IF, IFERROR, LET, LAMBDA,
MAP…) live in `domain/parser/evaluator/`. Functions take `&[Value]` and
return `Result<Value, String>`. Excel error values are AST nodes
(`Expr::ErrorLit`) that propagate through calculations; formula adjustment
on paste/fill/insert emits `#REF!` instead of clamping.

**Calculation engine** (from tshts). One workbook-wide `WorkbookGraph` keyed
by `(SheetId, row, col)` tracks same- and cross-sheet dependencies.
`Workbook::recalc_via_graph_result` drains the dirty set, levels the
affected cells topologically and runs them with `SequentialExecutor` or,
when a level has at least `CELLAR_PAR_THRESHOLD` cells (default 512),
`ParallelExecutor` (rayon). `NOW`/`TODAY` share one clock per recalc;
`INDIRECT`/`OFFSET` are re-seeded when their recorded targets change; `GET`
runs serially. Cycles fail unless `Workbook::iterative_calc` is on;
non-converging cycles become `#NUM!`.

**GUI notes.** egui 0.36: the app implements `eframe::App::ui`; panels are
`egui::Panel::{top,bottom,right}` shown inside the root `Ui`. Widgets drawn
over the grid (cell editor, Auto Fill button) register their rects in
`GridState::overlays` so presses don't fall through. Draggable chips use
`Response::dnd_set_drag_payload` on a click-and-drag button, because egui
can't nest clickable widgets inside `dnd_drag_source`. egui's default font
lacks some symbols (→, ▾, ✕); use words or painted shapes instead.

## Tests

- Unit tests live next to the code (`#[cfg(test)]` modules, sibling
  `tests.rs`).
- `tests/scenarios.rs` runs whole models (DCF, amortization, tax brackets,
  FIFO inventory…) typed in through `tests/common::Harness`, which drives
  `App` exactly like the GUI, and checks every result against a pure-Rust
  implementation. Derive expected values from the scenario's `compute()`,
  never from a Cellar run.
- The GUI itself has no automated tests; check UI changes by running the app.
