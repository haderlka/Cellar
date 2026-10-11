---
title: Sheets and view
nav_order: 10
---

# Sheets and view
{: .no_toc }

1. TOC
{:toc}

## Sheets

A workbook holds one or more sheets. Their tabs are at the bottom of the
window.

| To do this | Do this |
|---|---|
| Show a sheet | Click its tab, or press Ctrl+PgDn / Ctrl+PgUp |
| Add a sheet | Click **+** after the tabs, or **Sheet → Insert Sheet** (Shift+F11) |
| Rename | Double-click the tab, or **Sheet → Rename Sheet…** |
| Duplicate | **Sheet → Duplicate Sheet**. The copy is named like "Budget (2)". |
| Reorder | Drag a tab sideways, or **Sheet → Move Sheet Left / Right** |
| Delete | **Sheet → Delete Sheet**. Ctrl+Z brings it back. |

Right-clicking a tab offers the same commands.

Renaming a sheet updates every formula that refers to it. Formulas that
referred to a deleted sheet show `#REF!`.

## Freeze panes

Frozen rows and columns stay in place while the rest of the sheet scrolls,
so titles remain visible.

| Command (View menu) | Freezes |
|---|---|
| **Freeze Panes at Cursor** | Everything above and to the left of the selected cell |
| **Freeze Top Row** | Row 1 |
| **Freeze First Column** | Column A |
| **Unfreeze Panes** | Nothing |

Example: to keep row 1 and column A visible, select **B2** and choose
**Freeze Panes at Cursor**. A darker line marks the frozen area.

## Show Formulas

**View → Show Formulas** (Ctrl+`) displays formulas instead of results in
every cell. Useful for checking a sheet.

## Sidebar

The sidebar on the right holds the sheet's PivotTables and charts. Drag its
left edge to resize it. **View → Sidebar** hides or shows it.

## Zoom

Hold **Ctrl** (Cmd on macOS) and turn the mouse wheel, or use **View → Zoom
In**, **Zoom Out** and **Reset Zoom**.

## Hidden rows and columns

See [Hiding rows and columns](formatting.md#hiding-rows-and-columns).

## What is remembered

Frozen panes, hidden rows and columns, the filter and validation rules are
saved with each sheet in the workbook file. Zoom and the sidebar's size are
not.
