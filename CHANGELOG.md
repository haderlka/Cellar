# Changelog

## Cellar 0.1.0 — 2026-10-04

First release of Cellar, a desktop spreadsheet built on
[tshts](https://github.com/SamuelSchlesinger/tshts) 0.2.1. tshts' own
history is in its repository.

### Added

- **Desktop GUI** (egui/eframe) for macOS, Windows and Linux: virtualized
  grid, formula bar, name box, sheet tabs, menus, mouse selection, column
  resizing, click-to-insert references while typing formulas, native file
  dialogs, drag & drop to open, Open Recent.
- **Fill handle** with Excel's behaviour: formulas shift, series continue
  (numbers, months, weekdays, quarters, numbered text), copy/series toggle,
  double-click fill down, Auto Fill Options, Ctrl+D / Ctrl+R.
- **PivotTables** in a sidebar with Excel's PivotTable Fields pane, Value
  Field Settings (11 summary functions, 15 *Show Values As* calculations,
  number format), Field Settings (subtotals, sorting, Top 10, item filter),
  report filters and compact/outline/tabular layouts.
- **Charts** (column, line, pie, scatter) and **PivotCharts**, stored as
  definitions in the workbook.
- **Excel converter**: formatting, column widths, charts, PivotTables and
  PivotCharts are imported in addition to values and formulas, and every
  formula is verified against Excel's cached result in an import report.
- **Exports**: Markdown tables (sheet, workbook, selection), every chart as
  PNG or SVG, plus the existing `.xlsx` and CSV exports.
- **Batch commands**: `--convert`, `--export-md`, `--export-charts`.
- **Canonical file layout** for `.cellar` files: sorted, one line per cell,
  default fields omitted, byte-identical on unchanged saves.
- Logo and window icon.

### Changed

- Workbooks use the `.cellar` extension (tshts files open after renaming
  them from `.tshts`; the format is the same).
- The end-to-end scenario tests now drive the editing session directly
  instead of a terminal, so they run on every platform in well under a
  second.
- Months and weekdays sort in calendar order in PivotTables.

### Removed

- The terminal interface (vim-style keys, command palette, ratatui UI) and
  its pseudo-terminal test suite.

### Fixed

- Series detection no longer panics on text with non-ASCII characters
  before a number (e.g. "Würfel1").
- File-error tests no longer depend on the operating system's language.
