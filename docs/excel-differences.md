---
title: Differences from Excel
nav_order: 13
---

# Differences from Excel
{: .no_toc }

Cellar follows Excel wherever it can, but it keeps its file format small
and readable. That is why some things work differently, and some are
missing.

1. TOC
{:toc}

## Works differently

| Topic | In Cellar |
|---|---|
| **PivotTables and charts** | Live in the sidebar, not in the cells. They are stored as definitions and always show current data; there is no Refresh. |
| **Conditional formatting** | The rule is a formula in which `_` is the cell being formatted, instead of a formula written for the top-left cell. |
| **Data validation** | A rule covers a whole column. An entry that fails can still be entered on purpose. |
| **Filter** | One column and one condition per sheet. |
| **Tables** | Every table has the same look. There is no total row and no filter buttons in the header. |
| **Colours** | A palette of 15 colours for text and fill. Imported colours snap to the nearest one. |
| **Insert and delete** | Whole rows or columns only; single cells cannot be shifted. |
| **Clear (Delete key)** | Also removes the cell's formatting and comment. Use Edit → Clear to remove just one of them. |
| **Text in cells** | Does not wrap and does not flow into empty neighbours. |
| **Links** | Made with `=HYPERLINK(…)`; opened with Ctrl+click. |
| **Sparklines** | Made with `=SPARKLINE(…)`, as in Google Sheets. |
| **Print** | Opens a PDF of the sheet; you print from the PDF viewer. A4 portrait, no page setup. |
| **Dates typed into cells** | Stay text (`2026-03-15`). Use `DATE(2026, 3, 15)` or `DATEVALUE` to calculate with them. |

## Not available

- Whole-column and whole-row references (`B:B`, `2:2`). Use a range such as
  `B1:B1000`.
- Merged cells, borders, italic, strikethrough, text alignment, wrapped
  text, font and font size.
- Date and custom number formats.
- Colour scales, data bars and icon sets in conditional formatting.
- Filtering by several columns or several values at once.
- Rich text inside a cell, images, shapes and text boxes.
- Macros and VBA.
- Sheet protection, hidden sheets, tab colours.
- Page setup: orientation, margins, headers and footers.
- The `GET` function needs network access, which cannot yet be switched on
  from the window.

## Functions

Cellar has about 160 of Excel's functions; see the
[function reference](functions.md). When you import a workbook, the
[import report](files.md#the-excel-import-report) lists every formula that
uses a function Cellar lacks.

## Going back to Excel

**File → Export → Excel workbook (.xlsx)…** writes values, formulas and
formatting. Not written: PivotTables, charts, tables, conditional formats,
comments and validation rules. `SPARKLINE` cells show `#NAME?` in Excel.
