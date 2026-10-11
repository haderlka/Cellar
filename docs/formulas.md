---
title: Formulas
nav_order: 5
---

# Formulas
{: .no_toc }

1. TOC
{:toc}

## Writing a formula

A formula starts with `=`. Cellar uses Excel's syntax:

```
=B2*C2
=SUM(A1:A10)
=IF(B2>0, "yes", "no")
=XLOOKUP("West", A2:A9, C2:C9)
=SORT(UNIQUE(A2:A100))
```

Press **Enter** to finish. The cell shows the result; the formula bar shows
the formula. Cellar recalculates automatically whenever a cell it depends
on changes. **Formulas → Calculate Now** (F9) recalculates everything, for
example to refresh `NOW()` or `RAND()`.

Operators: `+ - * / ^` for arithmetic, `&` to join text, `= <> < > <= >=`
to compare.

## References

| You write | It means |
|---|---|
| `B2` | One cell |
| `A1:C10` | A range |
| `$B$2`, `B$2`, `$B2` | A reference that stays fixed when the formula is copied or filled |
| `Sheet2!A1` | A cell on another sheet |
| `'My Sheet'!A1:B5` | A sheet whose name has spaces |
| `TaxRate` | A [name](#names) |
| `Sales[Price]` | A column of a [table](data.md#tables) |
| `Sales` | All data rows of a table |

Whole-column references such as `B:B` are not supported. Use a range like
`B1:B1000`.
{: .warning }

### Clicking instead of typing

While you type a formula, **click a cell** or **drag over a range** to
insert its reference. Click again to replace it with another.

To refer to another sheet, click that sheet's **tab** while typing, then
click the cells there. Cellar writes `Sheet2!B4` for you. Press Enter and
you are back at the formula's cell.

### Seeing what a formula reads

Select or edit a formula cell: every range it reads is outlined in the
grid, each reference in its own colour, matching the colours in the
formula.

## Formula help while typing

- **Suggestions.** As you type a name, Cellar lists the functions, names
  and tables that start with it. Use the **arrow keys** to choose and
  **Tab** to insert, or click an entry. A short description of the
  highlighted function is shown below the list.
- **Arguments.** Inside a function's brackets, Cellar shows its arguments
  and marks the one you are typing:
  `VLOOKUP(lookup_value, table, col_index, [range_lookup])`. Arguments in
  square brackets are optional.

## AutoSum

Select the cell under (or to the right of) a run of numbers and press
**Alt+=** (**Formulas → AutoSum**). Cellar proposes a `SUM` over the
numbers; press Enter to accept.

## Names

A name stands for a range, a value or a calculation, and makes formulas
readable: `=Price * (1 + TaxRate)`.

### Name the selection

Select the cells, click the **name box**, type a new name and press
**Enter**.

### Name Manager

**Formulas → Name Manager…** (Ctrl+F3) lists all names.

- **Define a name:** type the name and what it refers to
  (`Sheet1!B2:B20`, or a value such as `0.19`), then **Save Name**. Click
  **Use Selection**, or click or drag in the grid, to fill in a range.
- **Edit** loads a name into the form; saving replaces it.
- **Go To** selects the range a name refers to.
- **Delete** removes it.

Names start with a letter or `_` and contain letters, digits, `_` and `.`.
A name cannot look like a cell (`A1`).

## Dynamic arrays

A formula that returns several values fills the cells below and beside it.
This is called *spilling*.

```
=SORT(A2:A9)
=UNIQUE(B2:B100)
=FILTER(A2:C50, C2:C50>100)
=SEQUENCE(5)
```

Only the first cell holds the formula; edit that one. If other content is
in the way, the cell shows `#SPILL!`.

`LET` names intermediate results inside one formula, and `LAMBDA` defines
your own function, used with `MAP`, `REDUCE`, `BYROW` and the like:

```
=LET(total, SUM(B2:B10), total / COUNT(B2:B10))
=MAP(A2:A5, LAMBDA(x, x * 2))
```

## Tracing

- **Formulas → Trace Precedents** draws arrows from the cells the selected
  formula reads.
- **Formulas → Trace Dependents** draws arrows to the cells whose formulas
  read the selected cell.

Cells on other sheets are listed in the status bar. The arrows disappear
when you move the cursor or press **Esc**.

## Show Formulas

**Formulas → Show Formulas** (Ctrl+`) shows every formula in the grid
instead of its result. Choose it again to switch back.

## Links and in-cell charts

Two functions are drawn specially by the grid:

- `=HYPERLINK("https://example.com", "text")` makes a link. Ctrl+click
  opens it. See [Links](editing.md#links).
- `=SPARKLINE(B2:M2)` draws a small line chart in the cell.
  `=SPARKLINE(B2:M2, "column")` draws columns. See
  [Sparklines](charts.md#sparklines).

## Errors

| Error | Meaning |
|---|---|
| `#DIV/0!` | Division by zero |
| `#VALUE!` | A value of the wrong kind, such as text where a number is needed |
| `#NAME?` | An unknown function or name |
| `#REF!` | A reference to a cell that no longer exists |
| `#N/A` | A lookup found nothing |
| `#NUM!` | A number that cannot be computed |
| `#SPILL!` | No room for a formula's results |

`IFERROR(formula, fallback)` replaces any error with a fallback value.

A formula that would refer to itself, directly or through other cells, is
rejected when you enter it.

## PivotTable values in formulas

`GETPIVOTDATA` reads a value from a PivotTable. While typing a formula,
click a value in a PivotTable in the sidebar to insert the call. See
[PivotTables](pivot-tables.md#using-results-in-formulas).

All functions are listed in the [function reference](functions.md).
