---
title: Charts
nav_order: 9
---

# Charts
{: .no_toc }

1. TOC
{:toc}

Charts are shown in the **sidebar** on the right. They follow the cells
they are drawn from: change a number and the chart changes.

## Insert a chart

1. Select the data, including its header row and the labels in the first
   column.
2. Choose **Data → Insert Chart…** (Alt+F1), or click **+ Chart** at the
   top of the sidebar.
3. Check what Cellar worked out from your selection:

   | Field | Meaning |
   |---|---|
   | **Title** | The heading above the chart |
   | **Type** | Bar, Line, Pie or Scatter |
   | **Labels range** | The cells with the category names, for example `A2:A10` (optional) |
   | **Series** | One line per data series: a name and its values range, for example `B2:B10` |

4. Use **+ Add series** or **Remove** to change the series. The dialog
   shows a preview.
5. Click **Insert**.

Ranges can be on another sheet: `Sheet2!B2:B10`.

## Change, export or delete a chart

Each chart has its own card in the sidebar with three buttons:

- **Edit** opens the dialog again.
- **Export…** saves the chart as a PNG or SVG image.
- **Delete** removes it.

**File → Export → All charts as PNG…** (or SVG) saves every chart of the
workbook into a folder.

## PivotCharts

A PivotChart draws the result of a [PivotTable](pivot-tables.md): its row
items become the categories, and its columns and values become the series.
It follows the PivotTable's fields, filters and sorting.

1. Click **PivotChart** on the PivotTable's card in the sidebar.
2. Choose the title and type (Bar, Line or Pie) and click **Insert**.

The PivotChart appears below its PivotTable.

## Sparklines

A sparkline is a tiny chart inside a cell, drawn by a formula:

| Formula | Draws |
|---|---|
| `=SPARKLINE(B2:M2)` | A line through the values |
| `=SPARKLINE(B2:M2, "column")` | One column per value; negative values in red |

Put one at the end of each row to show its trend. The sparkline takes the
cell's text colour if you set one.

Excel has no `SPARKLINE` function, so these cells show `#NAME?` after an
export to `.xlsx`.
{: .note }
