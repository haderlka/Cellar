---
title: Formatting
nav_order: 4
---

# Formatting
{: .no_toc }

1. TOC
{:toc}

Formatting changes how a cell looks, never what it contains. Select the
cells first, then choose a command from the **Format** menu.

## Number formats

**Format → Number Format** offers:

| Format | Example | Shortcut |
|---|---|---|
| General | `1234.5` as typed | Ctrl+Shift+~ |
| Number | `1,234.56` | Ctrl+Shift+! |
| Number without separator | `1234.56` | |
| Integer | `1,235` | |
| Currency | `$1,234.56` or `1,234.56 €` | Ctrl+Shift+$ |
| Percent | `12%` or `12.34%` | Ctrl+Shift+% |

To change the number of decimals, set **Format → Decimal places** and click
**Apply**. Number, currency and percent formats keep their kind; General
cells become plain numbers.

The currency symbol sits where its countries put it: `$1,234.56`,
`1,234.56 €`, `CHF 1,234.56`.

## Bold, underline and colours

- **Format → Bold** (Ctrl+B) and **Underline** (Ctrl+U).
- **Format → Text Color** and **Fill Color** offer a palette of 15 colours,
  and **None** to remove the colour.

## Format Painter

Format Painter copies the formatting of one cell onto others.

1. Select the cell whose formatting you want.
2. Choose **Format → Format Painter**.
3. Click a cell, or drag over a range. They take the formatting; their
   content stays.

Press **Esc** to cancel before step 3.

## Conditional formatting

A conditional format changes the look of cells depending on their value,
for example a red fill for every amount over 1,000. The formatting updates
by itself when values change.

### Add a rule

1. Select the cells the rule is for: one cell, a range, or whole columns
   (click the column letters).
2. Choose **Format → Conditional Formatting…**.
3. Under **New rule**, check **Applies to**. It is written as in a formula:

   | You enter | The rule covers |
   |---|---|
   | `C1` | One cell |
   | `C1:D5` | A range |
   | `B:B` or `B` | The whole column B, however long it grows |
   | `B:D` | Columns B to D |
   | `C1:C9, E1:E9` | Several of these, separated by commas |

   To pick the cells with the mouse, click into the field and then drag
   over them in the grid.
4. Choose the condition under **Format cells if**:

   | Condition | Formats cells that… |
   |---|---|
   | Value is greater than, less than, at least, at most, equal to, not equal to, between | compare that way with what you enter |
   | Text contains | contain the text |
   | Cell is blank / is not blank | are empty or not |
   | Above / Below the column's average | are above or below the average of the rule's cells |
   | Duplicate values / Unique values | appear more than once, or only once, among the rule's cells |
   | Top values / Bottom values | are among the N largest or smallest of the rule's cells |
   | Formula | make your own formula true |

5. Choose the **Format**: bold, underline, text colour, fill colour. The
   preview shows the result.
6. Click **Add Rule**, then **OK**.

### What you can compare with

In the value field you can enter:

| You enter | Meaning |
|---|---|
| `100` | The number 100 |
| `late` | The text "late" |
| `C3` | The cell C3 |
| `Limits!B2` | A cell on another sheet |
| `GETPIVOTDATA(…)` or `AVERAGE(B1:B50)` | The result of a function |
| `=TaxRate * 2` | Anything after `=`: a name or a calculation |
| `"Q1"` | Text that would otherwise read as a cell |

Instead of typing a cell, **click it in the grid** (click into the value
field first if you last used **Applies to**). You can switch to another
sheet and click there. Clicking a value in a **PivotTable** in the
sidebar inserts its `GETPIVOTDATA` formula.

The **Rule** line under the form shows how Cellar reads your entry, so you
can check that `C3` was taken as a cell and not as text.

### Compare with a cell in the same row

Often each row has its own target: highlight *Actual* where it is below
*Target* in the same row.

1. Choose "Value is less than" and enter a cell of the target column, for
   example `C3`.
2. Tick **Compare with that cell's column in each row**.

Now every cell is compared with column C of its own row, not always with
C3.

### Your own formula

Choose **Formula** and write a condition. In it, `_` stands for the cell
being formatted and `ROW()` for its row number:

| Formula | Formats cells that… |
|---|---|
| `_ > AVERAGE(B1:B100)` | are above the average of B1:B100 |
| `AND(_ >= 10, _ < 20)` | are from 10 up to, but not including, 20 |
| `LEN(_) > 30` | hold more than 30 characters |
| `_ < INDIRECT("C" & ROW())` | are below column C in the same row |

### Change or remove rules

Open **Format → Conditional Formatting…** again. Every rule of the sheet is
listed with the cells it applies to, and with **Edit** and **Remove**. Later rules are applied on top of
earlier ones. Nothing changes until you click **OK**.

### Good to know

- Numeric conditions apply to numbers only, so a column's title is not
  formatted along with its values.
- A rule follows its cells when rows or columns are inserted or deleted.
- Average, duplicates and top N look at the rule's own cells. For a whole
  column they read rows 1 to 1000, or the sheet's size if it is larger, and
  several columns are each looked at on their own.

## Column widths and row heights

- **Drag** the border between two column letters or two row numbers.
- **Double-click** a column border to fit the column to its content
  (**Format → AutoFit Column Width** does the same for the selection).
- Double-click a row border to reset the row to the standard height.
- Select several whole columns or rows first to resize them together.

Text does not wrap and does not flow into the next cell; a cell shows as
much as fits.

## Hiding rows and columns

**Format → Hide & Unhide** hides the selected rows (Ctrl+9) or columns
(Ctrl+0). To bring them back, select across the hidden ones and choose
**Unhide** (Ctrl+Shift+9 or Ctrl+Shift+0). With nothing hidden in the
selection, Unhide shows everything.

## Tables

**Format → Format as Table…** gives a block of data a header row and banded
rows, and lets formulas refer to its columns by name. See
[Tables](data.md#tables).
