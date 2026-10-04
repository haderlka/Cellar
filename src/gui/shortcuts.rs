//! Every menu command and its keyboard shortcut, in one table.
//!
//! Shortcuts follow Excel (Windows/Linux) and Excel for Mac, where ⌘
//! replaces Ctrl for the common commands. Menus, the keyboard handler and
//! the Help window all read this table, so a shortcut shown in a menu is
//! always the one that works.

use eframe::egui::{self, Context, Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    New,
    Open,
    ImportExcel,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    ClearContents,
    CopyMarkdown,
    FillDown,
    FillRight,
    FillSeries,
    InsertCells,
    DeleteCells,
    InsertRows,
    DeleteRows,
    InsertColumns,
    DeleteColumns,
    SelectRow,
    SelectColumn,
    SelectAll,
    InsertDate,
    InsertTime,
    Bold,
    Underline,
    FormatGeneral,
    FormatNumber,
    FormatCurrency,
    FormatPercent,
    HideRows,
    UnhideRows,
    HideColumns,
    UnhideColumns,
    AutoFitColumn,
    AutoSum,
    SortAscending,
    SortDescending,
    PivotTable,
    InsertChart,
    Recalculate,
    NewSheet,
    NextSheet,
    PreviousSheet,
    RenameSheet,
    DeleteSheet,
    ToggleSidebar,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Help,
    About,
}

const CMD: Modifiers = Modifiers::COMMAND;
const CTRL: Modifiers = Modifiers::CTRL;
const SHIFT: Modifiers = Modifiers::SHIFT;
const ALT: Modifiers = Modifiers::ALT;

fn ks(mods: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(mods, key)
}

fn is_mac(ctx: &Context) -> bool {
    ctx.os() == egui::os::OperatingSystem::Mac
}

impl Action {
    /// Every action, grouped the way the menus are.
    pub const ALL: [Action; 55] = [
        Action::New,
        Action::Open,
        Action::ImportExcel,
        Action::Save,
        Action::SaveAs,
        Action::Quit,
        Action::Undo,
        Action::Redo,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::ClearContents,
        Action::CopyMarkdown,
        Action::FillDown,
        Action::FillRight,
        Action::FillSeries,
        Action::InsertCells,
        Action::DeleteCells,
        Action::InsertRows,
        Action::DeleteRows,
        Action::InsertColumns,
        Action::DeleteColumns,
        Action::SelectRow,
        Action::SelectColumn,
        Action::SelectAll,
        Action::InsertDate,
        Action::InsertTime,
        Action::Bold,
        Action::Underline,
        Action::FormatGeneral,
        Action::FormatNumber,
        Action::FormatCurrency,
        Action::FormatPercent,
        Action::HideRows,
        Action::UnhideRows,
        Action::HideColumns,
        Action::UnhideColumns,
        Action::AutoFitColumn,
        Action::AutoSum,
        Action::SortAscending,
        Action::SortDescending,
        Action::PivotTable,
        Action::InsertChart,
        Action::Recalculate,
        Action::NewSheet,
        Action::NextSheet,
        Action::PreviousSheet,
        Action::RenameSheet,
        Action::DeleteSheet,
        Action::ToggleSidebar,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomReset,
        Action::Help,
        Action::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New",
            Action::Open => "Open…",
            Action::ImportExcel => "Import Excel workbook…",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::Quit => "Quit",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::ClearContents => "Clear Contents",
            Action::CopyMarkdown => "Copy as Markdown Table",
            Action::FillDown => "Fill Down",
            Action::FillRight => "Fill Right",
            Action::FillSeries => "Fill Series",
            Action::InsertCells => "Insert…",
            Action::DeleteCells => "Delete…",
            Action::InsertRows => "Insert Rows Above",
            Action::DeleteRows => "Delete Rows",
            Action::InsertColumns => "Insert Columns Left",
            Action::DeleteColumns => "Delete Columns",
            Action::SelectRow => "Select Row",
            Action::SelectColumn => "Select Column",
            Action::SelectAll => "Select All",
            Action::InsertDate => "Insert Today's Date",
            Action::InsertTime => "Insert Current Time",
            Action::Bold => "Bold",
            Action::Underline => "Underline",
            Action::FormatGeneral => "General",
            Action::FormatNumber => "Number (1,234.56)",
            Action::FormatCurrency => "Currency ($1,234.56)",
            Action::FormatPercent => "Percent (12%)",
            Action::HideRows => "Hide Rows",
            Action::UnhideRows => "Unhide Rows",
            Action::HideColumns => "Hide Columns",
            Action::UnhideColumns => "Unhide Columns",
            Action::AutoFitColumn => "AutoFit Column Width",
            Action::AutoSum => "AutoSum",
            Action::SortAscending => "Sort A to Z",
            Action::SortDescending => "Sort Z to A",
            Action::PivotTable => "PivotTable…",
            Action::InsertChart => "Insert Chart…",
            Action::Recalculate => "Calculate Now",
            Action::NewSheet => "Insert Sheet",
            Action::NextSheet => "Next Sheet",
            Action::PreviousSheet => "Previous Sheet",
            Action::RenameSheet => "Rename Sheet…",
            Action::DeleteSheet => "Delete Sheet",
            Action::ToggleSidebar => "Sidebar (PivotTables & Charts)",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::ZoomReset => "Reset Zoom",
            Action::Help => "Keyboard Shortcuts & Formulas",
            Action::About => "About Cellar",
        }
    }

    /// Key bindings that trigger the action. The first one is shown in
    /// menus. Alternatives cover keyboard layouts (a German keyboard has a
    /// separate + key, types = with Shift+0, and Excel DE uses Ctrl+. for
    /// the date).
    pub fn bindings(self, ctx: &Context) -> Vec<KeyboardShortcut> {
        let mac = is_mac(ctx);
        match self {
            Action::New => vec![ks(CMD, Key::N)],
            Action::Open => vec![ks(CMD, Key::O)],
            Action::Save => vec![ks(CMD, Key::S)],
            Action::SaveAs if mac => vec![ks(CMD | SHIFT, Key::S)],
            Action::SaveAs => vec![ks(Modifiers::NONE, Key::F12), ks(CMD | SHIFT, Key::S)],
            Action::Quit if mac => vec![ks(CMD, Key::Q)],
            Action::Undo => vec![ks(CMD, Key::Z)],
            Action::Redo => vec![ks(CMD, Key::Y), ks(CMD | SHIFT, Key::Z)],
            // Cut/Copy/Paste arrive as clipboard events; shown, not bound.
            Action::Cut => vec![ks(CMD, Key::X)],
            Action::Copy => vec![ks(CMD, Key::C)],
            Action::Paste => vec![ks(CMD, Key::V)],
            Action::ClearContents => vec![ks(Modifiers::NONE, Key::Delete)],
            Action::FillDown => vec![ks(CMD, Key::D)],
            Action::FillRight => vec![ks(CMD, Key::R)],
            Action::InsertCells => vec![
                ks(CMD | SHIFT, Key::Equals),
                ks(CMD, Key::Plus),
                ks(CMD | SHIFT, Key::Plus),
            ],
            Action::DeleteCells => vec![ks(CMD, Key::Minus)],
            Action::SelectRow => vec![ks(SHIFT, Key::Space)],
            Action::SelectColumn => vec![ks(CTRL, Key::Space)],
            Action::SelectAll => vec![ks(CMD, Key::A)],
            Action::InsertDate => vec![ks(CTRL, Key::Semicolon), ks(CTRL, Key::Period)],
            Action::InsertTime => vec![ks(CTRL | SHIFT, Key::Semicolon), ks(CTRL | SHIFT, Key::Period)],
            Action::Bold => vec![ks(CMD, Key::B)],
            Action::Underline => vec![ks(CMD, Key::U)],
            Action::FormatGeneral => vec![ks(CTRL | SHIFT, Key::Backtick)],
            Action::FormatNumber => vec![ks(CTRL | SHIFT, Key::Num1)],
            Action::FormatCurrency => vec![ks(CTRL | SHIFT, Key::Num4)],
            Action::FormatPercent => vec![ks(CTRL | SHIFT, Key::Num5)],
            Action::HideRows => vec![ks(CTRL, Key::Num9)],
            Action::UnhideRows => vec![ks(CTRL | SHIFT, Key::Num9)],
            Action::HideColumns => vec![ks(CTRL, Key::Num0)],
            Action::UnhideColumns => vec![ks(CTRL | SHIFT, Key::Num0)],
            Action::AutoSum if mac => vec![ks(CMD | SHIFT, Key::T)],
            Action::AutoSum => vec![ks(ALT, Key::Equals), ks(ALT | SHIFT, Key::Num0)],
            Action::InsertChart => vec![ks(ALT, Key::F1)],
            Action::Recalculate => vec![ks(Modifiers::NONE, Key::F9)],
            Action::NewSheet => vec![ks(SHIFT, Key::F11)],
            Action::NextSheet => vec![ks(CTRL, Key::PageDown)],
            Action::PreviousSheet => vec![ks(CTRL, Key::PageUp)],
            Action::Help => vec![ks(Modifiers::NONE, Key::F1)],
            _ => Vec::new(),
        }
    }

    /// Text shown next to the menu item.
    pub fn shortcut_text(self, ctx: &Context) -> String {
        let mac = is_mac(ctx);
        // Excel names these by the character, not the digit key.
        let chord = |mods: &str, ch: &str| {
            if mac { format!("{}{}", mods.replace("Ctrl+", "⌃").replace("Shift+", "⇧"), ch) } else { format!("{}{}", mods, ch) }
        };
        let show = |a: Action| a.bindings(ctx).first().map(|k| ctx.format_shortcut(k)).unwrap_or_default();
        match self {
            Action::Quit if !mac => "Alt+F4".into(),
            Action::FormatGeneral => chord("Ctrl+Shift+", "~"),
            Action::FormatNumber => chord("Ctrl+Shift+", "!"),
            Action::FormatCurrency => chord("Ctrl+Shift+", "$"),
            Action::FormatPercent => chord("Ctrl+Shift+", "%"),
            Action::InsertDate => chord("Ctrl+", ";"),
            Action::InsertTime => chord("Ctrl+Shift+", ";"),
            Action::AutoSum if !mac => "Alt+=".into(),
            Action::InsertCells => if mac { "⌘⇧=".into() } else { "Ctrl+Shift+=".into() },
            Action::DeleteCells => if mac { "⌘-".into() } else { "Ctrl+-".into() },
            // Excel: select the rows/columns first, then insert/delete.
            Action::InsertRows => format!("{}, {}", show(Action::SelectRow), Action::InsertCells.shortcut_text(ctx)),
            Action::DeleteRows => format!("{}, {}", show(Action::SelectRow), Action::DeleteCells.shortcut_text(ctx)),
            Action::InsertColumns => format!("{}, {}", show(Action::SelectColumn), Action::InsertCells.shortcut_text(ctx)),
            Action::DeleteColumns => format!("{}, {}", show(Action::SelectColumn), Action::DeleteCells.shortcut_text(ctx)),
            Action::ZoomIn | Action::ZoomOut => if mac { "⌘ + Scroll".into() } else { "Ctrl + Mouse Wheel".into() },
            other => show(other),
        }
    }

    /// Actions whose keys are handled outside the shortcut table:
    /// clipboard events, and Delete (which also accepts Backspace).
    pub fn bound_elsewhere(self) -> bool {
        matches!(self, Action::Cut | Action::Copy | Action::Paste | Action::ClearContents)
    }

    /// Actions that work even while a cell or text field is being edited.
    pub fn works_while_typing(self) -> bool {
        matches!(self, Action::New | Action::Open | Action::Save | Action::SaveAs | Action::Quit | Action::Help)
    }
}
