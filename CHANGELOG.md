# Changelog

## Unreleased

### Added

- **File → Import** submenu, next to Export, for every format Cellar can
  read. New formats: Excel `.xlsb` and `.xls`, OpenDocument `.ods`
  (formulas are translated from OpenFormula), tab-separated `.tsv`/`.tab`,
  Markdown tables (`.md`, round-trips File → Export → Markdown), JSON and
  JSON Lines. File → Open, double-click and `cellar --convert` accept them
  all.

### Changed

- **CSV import** detects the separator (`,` `;` tab `|`), so semicolon
  CSVs from European Excel no longer land in one column, and reads
  UTF-16 and Windows-1252 files instead of failing on them.
- **`.tsv` files** were split on commas; they are now split on tabs.
- File → Import Excel workbook… moved to File → Import → Excel workbook….

## Cellar 0.1.1 — 2026-10-04

Bug-fix release: crashes, freezes and data loss found by reviewing and
fuzzing the whole application.

### Fixed

- **Deleting a chart or PivotTable** from the sidebar crashed the app.
- **Grid crashes**: after deleting columns (or undoing an insert) while a
  selection reached the last column, and when every row was hidden.
- **Formulas that crashed the app**: one-argument functions called without
  arguments (`=SIN()`, `=HOUR()`, `=UNICHAR()` and 16 more), `REPLACE`
  with a start past the end of the text, and dates far outside Excel's
  range (`=YEAR(10^300)`). A bug in any built-in function now yields an
  error in that cell instead of closing Cellar.
- **Formulas that exhausted memory or never finished**: `REPT`, `&`,
  `CONCAT`, `TEXTJOIN` and `SUBSTITUTE` now stop at Excel's 32,767-character
  text limit (`#VALUE!`), `FIXED`/`DOLLAR` at 127 decimals, and
  `COMBIN` with huge arguments returns `#NUM!`.
- **Charts** with very large values, or text such as `NaN`/`inf`, hung the
  image export; values are now clamped and non-numbers skipped.
- **Undo/redo after switching sheets** changed the same cell on the wrong
  sheet; it now returns to the sheet where the edit was made.
- **Paste near the last row or column** silently dropped the cells that
  didn't fit; the sheet now grows to fit (up to Excel's grid).
- **Case-insensitive find & replace** crashed on characters whose lowercase
  form has a different length (`ẞ`, `İ`).
- **Large selections**: Select All with copy, cut, Delete, Bold, Copy as
  Markdown or the status-bar sum walked every empty cell and could freeze
  or run out of memory; they now only visit cells that hold data.
- **Opening hand-edited or damaged files**: sheets with zero rows/columns,
  absurd sizes, column widths or number formats, cells outside Excel's
  grid, overlong cell references, malformed colours in `.xlsx` styles, and
  `.xlsx` files that crash the reader now load safely or report an error.
  A workbook that fails to parse now shows the real reason instead of
  "missing field `cells`".
- **Typing a far-away address** in the Name Box could grow a sheet to
  billions of rows; sheets now stop at Excel's 1,048,576 rows.
- **PivotTable and chart ranges** like `A1:Z1000000` stop at the last used
  cell instead of reading a million empty rows every frame.
- The **Edit chart** window and the **PivotTable Fields** pane could act on
  the wrong chart or PivotTable after a delete, undo or sheet switch.

### Added

- **About Cellar** links to the GitHub repository and has a Buy Me a
  Coffee button.

## Cellar 0.1.0 — 2026-10-04

First release of Cellar, a desktop spreadsheet built on
[tshts](https://github.com/SamuelSchlesinger/tshts) 0.2.1. tshts' own
history is in its repository.

### Added

- **Desktop GUI** (egui/eframe) for macOS, Windows and Linux: virtualized
  grid, formula bar, name box, sheet tabs, menus, mouse selection, column
  resizing, click-to-insert references while typing formulas, native file
  dialogs, drag & drop to open, Open Recent. On macOS, `.cellar` files
  open from Finder by double-click.
- **`cellar --uninstall`** removes the program and everything Cellar stored
  for itself (recent files, clipboard cache); workbooks stay.
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
- **Excel keyboard shortcuts**, shown next to every menu item: Insert/Delete
  (Ctrl+Shift+= / Ctrl+-, with Excel's Insert/Delete dialog), Shift+Space /
  Ctrl+Space selection, Ctrl+Arrow jumps, Ctrl+End, Ctrl+Enter, AutoSum
  (Alt+=), date/time (Ctrl+; / Ctrl+Shift+;), number-format shortcuts,
  hide/unhide rows and columns, F9, F12, Shift+F11, Ctrl+PgUp/PgDn, Alt+F1.
  Zoom moved to Ctrl + mouse wheel, as in Excel.

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
