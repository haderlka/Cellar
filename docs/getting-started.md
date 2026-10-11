---
title: Getting started
nav_order: 2
---

# Getting started
{: .no_toc }

1. TOC
{:toc}

## Install Cellar

### Download a release

Go to the [releases page](https://github.com/haderlka/Cellar/releases) and
pick the file for your system.

| System | File | How to install |
|---|---|---|
| Windows | `cellar-…-windows-x64.exe` | Run it. There is no installer; keep the file wherever you like. |
| macOS | `cellar-…-macos-universal.dmg` | Open it and drag Cellar to Applications. |
| Linux | `cellar-…-linux-x64.tar.gz` | Unpack it and run `cellar`. |

On macOS the first start may be blocked because the app is not signed by
Apple. Right-click the app and choose **Open**, or run this once in
Terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Cellar.app
```

### Build from source

Cellar is written in Rust. Install Rust with [rustup](https://rustup.rs),
then:

```bash
git clone https://github.com/haderlka/Cellar.git
cd Cellar
cargo run --release
```

The first build takes a few minutes. Afterwards start
`target/release/cellar` (`cellar.exe` on Windows) directly.

- **Windows** needs the Visual Studio C++ Build Tools.
- **Linux** needs the usual GUI development packages, for example on
  Debian or Ubuntu: `libxkbcommon-dev libgtk-3-dev libssl-dev`.

## The window

From top to bottom:

| Part | What it is for |
|---|---|
| **Menu bar** | File, Edit, Format, Formulas, Data, Sheet, View, Help. Every item shows its keyboard shortcut. |
| **Name box** (left of the formula bar) | Shows the selected cell or range. Type `B12`, `A1:C5`, `Sheet2!A1` or a name and press Enter to go there. |
| **Formula bar** | Shows what is in the selected cell: its value, or its formula. You can edit here too. |
| **Grid** | The cells. Column letters across the top, row numbers down the side. |
| **Sidebar** (right) | PivotTables and charts of the sheet. Hide or show it with **View → Sidebar**. |
| **Sheet tabs** | One tab per sheet; **+** adds a sheet. |
| **Status bar** | Messages on the left. When you select several cells, their Sum, Average and Count on the right. |

## Your first workbook

1. Click cell **A1** and type `Item`. Press **Tab** to move right, type
   `Price`, press **Enter**.
2. Fill in a few rows: an item name in column A, a number in column B.
3. Click the cell under your prices and press **Alt+=** (AutoSum). Cellar
   proposes `=SUM(B2:B4)`. Press **Enter**.
4. Select the price cells and choose **Format → Number Format → Currency**.
5. Press **Ctrl+S**. Cellar asks where to save the `.cellar` file.

That file is plain text. Open it in a text editor to see one line per cell.

## Opening an Excel file

1. Choose **File → Open…** (or **File → Import → Excel workbook…**) and pick
   the `.xlsx` file. You can also drop the file onto the window.
2. Read the **import report**. It lists every formula that gives a different
   result in Cellar than Excel had saved, usually because it uses a function
   Cellar does not have.
3. Choose **File → Save As…** to write the `.cellar` file.

More about files: [Files](files.md).

## Getting help inside Cellar

**Help → Keyboard Shortcuts & Formulas** (F1) lists every command with its
shortcut. While you type a formula, Cellar suggests functions and shows
their arguments; see [Formulas](formulas.md#formula-help-while-typing).
