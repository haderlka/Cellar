---
title: Entering and editing
nav_order: 3
---

# Entering and editing
{: .no_toc }

1. TOC
{:toc}

## Typing into cells

| To do this | Do this |
|---|---|
| Replace a cell's content | Select the cell and start typing |
| Edit the existing content | Press **F2** or double-click the cell |
| Finish and move down | **Enter** (Shift+Enter moves up) |
| Finish and move right | **Tab** (Shift+Tab moves left) |
| Put the entry into every selected cell | **Ctrl+Enter** |
| Cancel | **Esc** |

Numbers are right-aligned, text is left-aligned. An entry that starts with
`=` is a formula; see [Formulas](formulas.md).

**Edit → Insert Today's Date** (Ctrl+;) and **Insert Current Time**
(Ctrl+Shift+;) type the date or time into the cell.

## Moving and selecting

| Keys or mouse | What happens |
|---|---|
| Arrow keys, PgUp, PgDn | Move one cell or one screen |
| Ctrl+Arrow | Jump to the edge of the block of data |
| Home | First column of the row |
| Ctrl+Home, Ctrl+End | First cell, last used cell |
| Shift with any of these | Extend the selection |
| Drag with the mouse | Select a range |
| Click a column letter or row number | Select the whole column or row |
| Click the corner above row 1 | Select everything |
| Shift+Space, Ctrl+Space, Ctrl+A | Select row, column, all |

Type a cell, range or name into the **name box** and press Enter to go
there. **Edit → Go To…** (Ctrl+G or F5) does the same and lists your names
and tables.

## Fill handle

The small square at the bottom-right corner of the selection is the fill
handle.

- **Drag it** down or across to copy the cells. Formulas are copied with
  their references shifted.
- If the cells hold a **series**, dragging continues it: 1, 2 gives 3, 4.
  Jan gives Feb. Mon gives Tue. Q1 gives Q2. Item1 gives Item2.
- Hold **Ctrl** (Option on macOS) while dragging to switch between copying
  and continuing the series.
- **Double-click** it to fill down as far as the neighbouring column has
  data.
- After a fill, the **Auto Fill** button next to the cells lets you change
  your mind: Copy Cells, Fill Series, Fill Formatting Only, Fill Without
  Formatting.
- Drag the handle **back into** the selection to clear cells.

**Edit → Fill Down** (Ctrl+D) and **Fill Right** (Ctrl+R) copy the first
row or column of the selection into the rest.

## Copy, cut and paste

**Ctrl+C**, **Ctrl+X**, **Ctrl+V** work as usual. Formulas are pasted with
their references shifted. Copying also puts the values on the system
clipboard, so you can paste into other programs, and you can paste text
from other programs into Cellar.

### Paste Special

**Edit → Paste Special…** (Ctrl+Alt+V) pastes only part of what you copied:

| Choice | What is pasted |
|---|---|
| All | Everything, like a normal paste |
| Values | The results, without formulas. The target keeps its formatting. |
| Formulas | Formulas and values. The target keeps its formatting. |
| Formats | Number format, bold, colours. The target keeps its content. |
| Transpose | Tick this to swap rows and columns |

**Edit → Paste Values** (Ctrl+Shift+V) pastes the values directly.

Paste Special works with cells copied in Cellar, not with text copied from
another program.
{: .note }

### Copy as Markdown

**Edit → Copy as Markdown Table** copies the selection as a Markdown table,
ready to paste into a README, an issue or a chat.

## Clearing

- **Delete** (or **Edit → Clear → Clear Contents**) empties the selected
  cells, including their formatting and comments.
- **Edit → Clear → Clear Formats** removes only the formatting.
- **Edit → Clear → Clear Comments** removes only the comments.

## Inserting and deleting rows and columns

Select whole rows or columns (click their headers), then press
**Ctrl+Shift+=** to insert or **Ctrl+-** to delete. If only cells are
selected, Cellar asks whether you mean rows or columns.

The same commands are in the **Edit** menu and in the right-click menu:
Insert Rows Above, Delete Rows, Insert Columns Left, Delete Columns.

Formulas, tables and conditional formats follow the rows and columns that
move.

## Undo and redo

**Ctrl+Z** undoes, **Ctrl+Y** redoes. Almost everything can be undone,
including sorts, table changes and sheet deletion.

## Find and Replace

**Edit → Find…** (Ctrl+F) opens a small window that stays open next to the
grid.

1. Type what you are looking for. Cellar searches as you type and
   highlights every match on the sheet.
2. Press **Enter** or **Find Next** to step through the matches.
   Shift+Enter steps backwards.
3. Tick **Match case** to distinguish capitals, or **Regular expression**
   to search with a pattern.

**Edit → Replace…** (Ctrl+H) adds a "Replace with" field.

- **Replace** changes the match under the cursor and moves to the next. The
  first press goes to a match; the next one replaces it.
- **Replace All** changes every match at once, as one undo step.

Replace also works inside formulas: replacing `SUM` with `MAX` rewrites the
formulas and recalculates them. A cell is skipped when only its *result*
contains the text.

## Comments

A comment is a note attached to a cell.

1. Select the cell and choose **Edit → Edit Comment…** (Shift+F2), or
   right-click the cell.
2. Type the note and click **OK**.

A red corner marks the cell. Hover over it to read the comment. Open the
dialog again to change it, or click **Delete Comment**.

## Links

**Edit → Insert Link…** (Ctrl+K) asks for an address and the text to show.
It enters a formula such as:

```
=HYPERLINK("https://example.com", "Our site")
```

The cell is shown in blue and underlined. **Ctrl+click** (Cmd+click on
macOS) opens the address. Hovering shows where the link goes.

## Right-click menu

Right-clicking a cell offers the most common commands: cut, copy, paste,
Paste Special, insert and delete, sort, filter by the cell's value, comment,
link, Format Painter, chart and PivotTable.
