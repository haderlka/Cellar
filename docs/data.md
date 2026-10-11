---
title: Tables and data tools
nav_order: 7
---

# Tables and data tools
{: .no_toc }

1. TOC
{:toc}

Most commands on this page work on a **block of data**: the cells around
the cursor up to the first empty row and column. Click any cell inside your
data and choose the command; select a range first if you want something
else.

## Tables

A table is a named block of data with a header row. It is drawn with a
coloured header and banded rows, and formulas can read its columns by
name.

### Create a table

1. Click a cell inside your data. The first row must hold the column
   titles.
2. Choose **Format → Format as Table…** (Ctrl+T).
3. Check the **Name** and the **Cells**. To change the cells, type a range
   or drag over them in the grid.
4. Click **OK**.

### Use a table in formulas

| Formula | Reads |
|---|---|
| `=SUM(Sales[Price])` | The Price column of the table Sales |
| `=AVERAGE(Sales[Unit Price])` | A column whose title has a space |
| `=COUNTA(Sales)` | All data rows of the table |

Formula help suggests table names as you type.

### Grow, change or remove a table

- **Grow:** type in the row directly below the table. The table extends to
  include it, and formulas using its columns follow.
- **Change:** click a cell of the table and choose **Format → Format as
  Table…** again to rename it or change its cells.
- **Remove:** in the same dialog, click **Convert to Range**. The cells
  stay; only the table is removed.

Every table looks the same; a table's style cannot be changed. A cell's own
fill colour wins over the table's banding.

## Sort

### Quick sort

Click a cell in the column to sort by and choose **Data → Sort A to Z** or
**Sort Z to A**. The whole block of data is sorted by that column; a header
row stays in place.

Numbers come before text, and blank cells always come last.

### Sort by several columns

1. Click a cell in your data and choose **Data → Sort…**.
2. Tick or untick **My data has headers**.
3. Choose the column to **Sort by** and the order.
4. Click **Add Level** for a second column, used where the first is equal.
5. Click **Sort**.

Whole rows move together, and formulas keep pointing at their own row.

## Filter

A filter hides the rows that do not match, without deleting them.

A sheet has one filter: one column and one condition.
{: .note }

### Filter by a value

- **Fastest:** right-click a cell and choose **Filter by Selected Cell's
  Value**. Only rows with that value in that column stay visible.
- **From a list:** choose **Data → Filter…** (Ctrl+Shift+L), pick the
  **Column**, and click a value in the list. Type in the search field to
  shorten the list.

### Filter by a condition

In **Data → Filter…**, next to **Show rows that**, choose a comparison and
type a value:

| Choice | Keeps rows that… |
|---|---|
| contain | contain the text |
| equal / do not equal | match exactly, or do not |
| are greater than, less than, at least, at most | compare that way; numbers as numbers |

Click **Apply**.

### Clear the filter

Choose **Data → Clear Filter**, or **Clear Filter** in the dialog. A funnel
in the column header marks the filtered column.

The filter is not applied again when data changes afterwards. Apply it
again to refresh.

## Remove duplicates

1. Click a cell in your data and choose **Data → Remove Duplicates…**.
2. Tick the columns that must match for two rows to count as duplicates.
3. Click **Remove Duplicates**.

The first row of each set is kept; the rows below move up. Capitals are
ignored when comparing.

## Text to columns

Splits text such as `Smith, John` into separate columns.

1. Select the cells in one column.
2. Choose **Data → Text to Columns…**.
3. Choose where to split: comma, semicolon, tab, space, or your own text.
   The dialog previews the first cell.
4. Click **Split**.

The pieces go into the columns to the right and replace what is there, so
make room first. Cells with formulas are left alone.

## Data validation

Validation restricts what can be typed into a column.

A rule always covers a **whole column**.
{: .note }

### Set a rule

1. Click a cell in the column and choose **Data → Data Validation…**.
2. Under **Allow**, choose:

   | Choice | Accepts |
   |---|---|
   | Any value | Everything; this removes the rule |
   | Whole number | Whole numbers within limits |
   | Decimal number | Numbers within limits |
   | One of a list | Only the listed values |
   | Text length | Text of a certain length |
   | Formula | Entries that make your formula true; `_` is the entry |

3. Fill in the limits or the list, and click **OK**.

Limits can be numbers or cells (`F1`). Click a cell in the grid to fill it
in.

### Lists and the dropdown

For **One of a list**, either type the values separated by commas
(`Open, Closed, On hold`) or give a range that holds them (`F1:F9`, or
`Lists!A2:A20` on another sheet). Drag over the range in the grid to fill
it in.

Cells of a column with a list rule show a **dropdown button** beside the
selected cell. Click it and choose a value.

### When an entry breaks the rule

Cellar asks what to do:

- **Retry** goes back to editing.
- **Enter Anyway** accepts the value this once, for example for a column
  title.
- **Cancel** discards the entry.

Blank cells and formulas are always accepted. Validation checks what you
type; it does not check pasted or filled cells.

## Goal Seek

Goal Seek finds the input value that makes a formula reach a result you
want. Example: which price gives a profit of 10,000?

1. Choose **Data → Goal Seek…**.
2. **Set cell:** the cell with the formula.
3. **To value:** the result you want.
4. **By changing cell:** the input cell the formula depends on.
5. Click **OK**.

Click cells in the grid to fill in the two cell fields. Cellar enters the
value it finds into the input cell; **Ctrl+Z** restores the old one.
