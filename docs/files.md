---
title: Files
nav_order: 11
---

# Files
{: .no_toc }

1. TOC
{:toc}

## Open and save

| Command | Shortcut | What it does |
|---|---|---|
| **File → New** | Ctrl+N | Starts an empty workbook |
| **File → Open…** | Ctrl+O | Opens a `.cellar` file or any format Cellar can import |
| **File → Open Recent** | | The last ten workbooks |
| **File → Save** | Ctrl+S | Saves the `.cellar` file |
| **File → Save As…** | F12 | Saves under a new name |

You can also drop a file onto the window, or start Cellar with a file:
`cellar Budget.cellar`.

The title bar shows a dot after the name while there are unsaved changes.
Cellar asks before discarding them.

Cellar always saves as `.cellar`. A file opened from another format has no
`.cellar` file yet, so the first Save asks where to put it.

## Import

**File → Import** (or simply **File → Open…**) reads these formats:

| Format | Extensions | What comes in |
|---|---|---|
| Excel workbook | `.xlsx` `.xlsm` | Values, formulas, formatting, charts, PivotTables |
| Excel binary / 97–2003 | `.xlsb` `.xls` | Values and formulas |
| OpenDocument (LibreOffice, Google Sheets download) | `.ods` | Values and formulas |
| CSV / delimited text | `.csv` `.txt` | Separator detected: comma, semicolon, tab or bar |
| Tab-separated | `.tsv` `.tab` | |
| Markdown tables | `.md` | One sheet per table, named after the heading above it |
| JSON | `.json` | An array of records or rows; an object of arrays gives one sheet per key |
| JSON Lines | `.jsonl` `.ndjson` | One record per line |

Text files may be UTF-8, UTF-16 or Windows-1252. Values from text formats
come in as values, never as formulas.

### The Excel import report

After importing an Excel or OpenDocument file, Cellar recalculates every
formula and compares the result with the value the file had saved. The
report lists:

- per sheet, how many cells, formulas, formatted cells and charts came in;
- every formula that computes **differently** in Cellar, usually because
  it uses a function Cellar does not have (shown as `#NAME?`). Click a cell
  in the list to go to it.

From Excel workbooks Cellar brings over values, formulas, sheets, named
ranges, number formats, bold and underline, colours, column widths, row
heights, charts, PivotTables (as live PivotTables) and PivotCharts.

## Export

**File → Export** writes:

| Choice | Result |
|---|---|
| **Excel workbook (.xlsx)…** | Values, formulas and formatting |
| **CSV (current sheet)…** | The sheet's values |
| **Markdown table (current sheet)…** / **Markdown (all sheets)…** | The computed values as Markdown tables |
| **All charts as PNG…** / **as SVG…** | Every chart of the workbook, into a folder |
| **PDF (current sheet)…** | The sheet as A4 pages |

**Edit → Copy as Markdown Table** copies just the selection.

## Print and PDF

**File → Print…** (Ctrl+P) writes the current sheet as a PDF and opens it
in your PDF viewer or browser. Print it from there.

The PDF uses A4 portrait pages with the sheet's column widths, row heights,
grid lines, colours and bold text, and a footer with the page number.
Columns that do not fit go onto further pages. Hidden rows and columns are
left out.

The PDF supports Western European characters. Others are printed as `?`.
{: .note }

## The .cellar format

A `.cellar` file is JSON in a fixed layout: keys sorted, cells sorted by
position, one line per cell, and nothing written that is at its default.

```json
"cells": [
  [0, 0, {"format": {"style": {"bold": true}}, "value": "Region"}],
  [1, 2, {"value": "1200.5"}],
  [7, 2, {"formula": "=SUM(C2:C7)", "value": "5821.5"}]
],
```

Each cell is `[row, column, content]`, counted from 0. Formula cells also
store their last result, so the file is readable without Cellar.

Besides its cells, a sheet stores column widths, row heights and, when
present, short entries for:

| Entry | Holds |
|---|---|
| `tables` | Name, range and column titles of each table |
| `conditional_formats` | Cells, rule and style of each conditional format |
| `view_state` | Frozen panes, hidden rows and columns, the filter, validation rules |
| `pivots` | PivotTable definitions (fields and settings, never results) |
| `charts` | Chart definitions (type and ranges, never images) |

### Working with git

Because the file is stable text:

- `git diff` shows exactly which cells changed.
- Two people editing different cells can merge without conflicts.
- Saving an unchanged workbook changes nothing in the file.

Commit the `.cellar` file like any source file.

## Batch commands

The same program converts and exports without opening a window:

```bash
cellar --convert Budget.xlsx                  # writes Budget.cellar, prints the report
cellar --convert data.csv                     # any importable format works
cellar --export-md Budget.cellar              # writes Budget.md (all sheets)
cellar --export-charts Budget.cellar          # PNGs into "Budget charts/"
cellar --export-charts Budget.xlsx out --svg  # SVGs; works on .xlsx too
```

## Uninstall

`cellar --uninstall` lists what would be removed: the program, the `cellar`
terminal command if you linked one, and what Cellar stored for itself
(recent files, clipboard cache, on macOS the app's preferences). Your
workbooks are not touched. Quit Cellar, then remove it all with:

```bash
cellar --uninstall --yes
```

On macOS, if `cellar` is not on your `PATH`, run
`/Applications/Cellar.app/Contents/MacOS/cellar --uninstall --yes`.
