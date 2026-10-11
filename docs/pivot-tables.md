---
title: PivotTables
nav_order: 8
---

# PivotTables
{: .no_toc }

1. TOC
{:toc}

A PivotTable summarises a list of data: revenue by region and month, count
of orders by customer, and so on. In Cellar, PivotTables live in the
**sidebar** on the right, not in the cells, and update by themselves when
the data changes.

## Create a PivotTable

Your data needs a header row: one title per column.

1. Click a cell inside the data (or select it).
2. Choose **Data → PivotTable…**, or click **+ PivotTable** at the top of
   the sidebar.
3. Check the **Table/Range** (for example `A1:D100`, or `Sheet2!A1:D100`
   for data on another sheet) and the **Name**.
4. Click **OK**.

The sidebar now shows the **PivotTable Fields** pane.

## Choose the fields

The pane lists every column of your data. Below it are four areas:

| Area | What goes there |
|---|---|
| **Rows** | Fields whose values become the rows, for example Region |
| **Columns** | Fields whose values become the columns, for example Month |
| **Values** | The numbers to summarise, for example Revenue |
| **Filters** | Fields to filter the whole report by |

- **Tick a field** in the list to add it. Text fields go to Rows, numbers
  to Values.
- **Drag** a field from the list into an area, or from one area to
  another.
- Click a field in an area to open its **menu**: Move Up, Move Down, Move
  to Beginning or End, Move to Row Labels, Column Labels, Report Filter or
  Values, Remove Field, and its settings.
- Use the **search** field above the list to find a field by name.

Click **Done** to close the pane. To open it again, click **Fields** on the
PivotTable's card.

## Change how values are summarised

Click a field in **Values** and choose **Value Field Settings…**.

- **Summarize Values By:** Sum, Count, Average, Max, Min, Product, Count
  Numbers, StdDev, StdDevp, Var, Varp.
- **Show Values As:** No calculation, % of grand total, % of row or column
  total, % of parent total, difference from, % difference from, running
  total, rank, index and more. Some need a **Base field** and **Base
  item**.
- **Custom Name** for the column, and the **Number Format**.

## Sort, filter and subtotals

Click a field in **Rows** or **Columns** and choose **Field Settings…**,
or use the dropdown beside **Row Labels** or **Column Labels** in the
table.

- **Sort:** A to Z, Z to A, by a value (largest first, for example), or
  manual (the order of the data).
- **Filter items:** untick the items to hide.
- **Top 10:** show only the top or bottom N items by a value.
- **Subtotals:** automatic or none.

Fields in **Filters** get a dropdown above the table to choose which items
the whole report includes.

Months and weekdays sort in calendar order.

## Layout and totals

Click **PivotTable Options…** in the Fields pane.

- **Report Layout:** Compact Form, Outline Form or Tabular Form, and
  whether to repeat item labels.
- **Subtotals** at the top or bottom of each group.
- **Grand totals** for rows and for columns.
- What to show **for empty cells**.
- Whether several value fields are shown in columns or in rows (Σ Values).
- The **data source** range, if your data has grown.

## Using results in formulas

`GETPIVOTDATA` reads one value from a PivotTable:

```
=GETPIVOTDATA("Sum of Revenue", "PivotTable1", "Region", "East")
```

You do not need to type it:

- While typing a formula, **click a value** in the PivotTable. Cellar
  inserts the call.
- Or right-click a value and choose **Copy GETPIVOTDATA formula**.

The PivotTable is identified by its name. A PivotTable on another sheet is
written as `"Sheet2!PivotTable1"`. Renaming a PivotTable updates the
formulas.

The same click works in the
[conditional formatting dialog](formatting.md#what-you-can-compare-with),
to format cells that are above or below a PivotTable value.

## PivotCharts

Click **PivotChart** on a PivotTable's card, or **Insert PivotChart…** in
the Fields pane. See [Charts](charts.md#pivotcharts).

## Rename or delete

- **Rename:** PivotTable Options… → PivotTable Name.
- **Delete:** click **Delete** on the card. Its PivotCharts are deleted
  too. Ctrl+Z brings them back.

## PivotTables from Excel

Importing an `.xlsx` file converts its PivotTables into live Cellar
PivotTables in the sidebar. Excel's static copy of their output stays in
the cells; the import report says where, so you can delete it.
