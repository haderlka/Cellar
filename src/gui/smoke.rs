//! Headless frames through the whole window: the grid paints with frozen
//! panes, tables, comments, links and sparklines, every tool dialog opens
//! and draws, and keys reach the dialogs. Nothing is checked pixel by
//! pixel; this catches panics and wiring mistakes, not layout.

use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers, Rect};

use crate::app::{EditMove, GuiApp};
use crate::shortcuts::Action;

struct Window {
    ctx: egui::Context,
    gui: GuiApp,
}

impl Window {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let gui = GuiApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()), None);
        Self { ctx, gui }
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = self.ctx.run_ui(input, |ui| eframe::App::ui(&mut self.gui, ui, &mut frame));
        // Nothing is rendered, so the font texture goes nowhere.
        output.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(Vec::new());
        }
    }

    fn key(&mut self, key: Key) {
        let press = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![press(true)]);
        self.frame(vec![press(false)]);
    }

    fn enter(&mut self, row: usize, col: usize, text: &str) {
        self.gui.set_cursor(row, col);
        self.gui.start_edit(Some(text.to_string()), true);
        self.gui.commit_edit(EditMove::Stay);
    }

    fn value(&self, row: usize, col: usize) -> String {
        self.gui.app.workbook.current_sheet().get_cell(row, col).value
    }

    /// A small price list with a header row.
    fn with_data() -> Self {
        let mut w = Self::new();
        let rows = [("Item", "Price", "Qty"), ("pear", "3", "10"), ("apple", "1.5", "40"), ("fig", "9", "2"), ("apple", "1.5", "40")];
        for (r, (a, b, c)) in rows.iter().enumerate() {
            w.enter(r, 0, a);
            w.enter(r, 1, b);
            w.enter(r, 2, c);
        }
        w.gui.set_cursor(1, 1);
        w
    }
}

#[test]
fn grid_paints_panes_tables_comments_links_and_sparklines() {
    let mut w = Window::with_data();
    w.gui.app.add_table(((0, 0), (4, 2)), "Stock").unwrap();
    w.gui.app.add_conditional_format(1..=1, "_ > 2", Default::default());
    w.enter(6, 0, "=HYPERLINK(\"https://example.com\", \"site\")");
    w.enter(6, 1, "=SPARKLINE(B2:B5)");
    w.enter(6, 2, "=SPARKLINE(C2:C5, \"column\")");
    w.enter(7, 0, "=SUM(Stock[Price])");
    assert_eq!(w.value(6, 0), "site");
    assert_eq!(w.value(7, 0), "15");
    w.gui.set_cursor(6, 0);
    let link = w.gui.app.workbook.current_sheet().get_cell(6, 0);
    assert_eq!(w.gui.hyperlink_url(&link).as_deref(), Some("https://example.com"));
    w.gui.app.set_cell_comment(Some("checked".to_string()));

    // Freeze the header row and first column, then scroll both ways.
    w.gui.set_cursor(1, 1);
    w.gui.do_action(&w.ctx.clone(), Action::FreezePanes);
    assert_eq!((w.gui.app.frozen_rows, w.gui.app.frozen_cols), (1, 1));
    w.frames(2);
    w.gui.set_cursor(90, 20);
    w.gui.grid.scroll_to_cursor = true;
    w.frames(3);
    w.gui.app.selection_start = Some((0, 0));
    w.gui.app.selection_end = Some((95, 22));
    w.frames(2);

    w.gui.set_cursor(7, 0);
    w.gui.grid.scroll_to_cursor = true;
    w.gui.do_action(&w.ctx.clone(), Action::TracePrecedents);
    w.gui.do_action(&w.ctx.clone(), Action::ShowFormulas);
    w.frames(2);
    assert!(w.gui.tools.trace.is_some());
    w.gui.set_cursor(1, 1);
    w.gui.do_action(&w.ctx.clone(), Action::TraceDependents);
    assert!(w.gui.app.status_message.as_deref().is_some_and(|m| m.contains("A8")), "{:?}", w.gui.app.status_message);
    w.frames(2);

    // A filter hides rows; the frozen header stays.
    w.gui.set_cursor(2, 0);
    w.gui.do_action(&w.ctx.clone(), Action::FilterBySelection);
    assert_eq!(w.gui.app.hidden_rows.len(), 2, "pear and fig are hidden: {:?}", w.gui.app.hidden_rows);
    w.frames(2);
    w.gui.do_action(&w.ctx.clone(), Action::ClearFilter);
    assert!(w.gui.app.hidden_rows.is_empty());

    // Panes too large to leave anything scrolling fall back to unfrozen.
    w.gui.app.frozen_rows = 500;
    w.gui.app.frozen_cols = 500;
    w.frames(2);
}

#[test]
fn every_tool_dialog_opens_draws_and_closes() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    let modal = [
        Action::GoTo,
        Action::EditComment,
        Action::PasteSpecial,
        Action::InsertHyperlink,
        Action::FormatAsTable,
        Action::DataValidation,
        Action::SortDialog,
        Action::RemoveDuplicates,
        Action::TextToColumns,
        Action::GoalSeek,
    ];
    for action in modal {
        w.gui.do_action(&ctx, action);
        assert!(w.gui.tools.any_open(), "{:?} opens a dialog", action);
        w.frames(3);
        w.key(Key::Escape);
        assert!(!w.gui.tools.any_open(), "Esc closes {:?}", action);
    }
    // Windows that stay open beside the grid.
    for action in [Action::ConditionalFormatting, Action::Filter, Action::NameManager, Action::Find, Action::Replace] {
        w.gui.do_action(&ctx, action);
        w.frames(3);
    }
    assert!(w.gui.tools.find.is_some());
    w.gui.tools = Default::default();
    w.gui.do_action(&ctx, Action::Find);
    w.frames(2);
    w.key(Key::Escape);
    w.frames(1);
    assert!(w.gui.tools.find.is_none(), "Esc closes Find");
    w.gui.tools = Default::default();
    w.gui.dialogs.help = true;
    w.frames(2);
}

#[test]
fn sort_and_duplicates_dialogs_act_on_enter() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    w.gui.do_action(&ctx, Action::RemoveDuplicates);
    w.frames(2);
    w.key(Key::Enter);
    assert_eq!(w.value(4, 0), "", "the second apple row is gone");
    w.gui.set_cursor(2, 1);
    w.gui.do_action(&ctx, Action::SortDialog);
    w.frames(2);
    w.key(Key::Enter);
    assert_eq!((1..4).map(|r| w.value(r, 0)).collect::<Vec<_>>(), ["apple", "pear", "fig"], "by price, header kept");
    assert_eq!(w.value(0, 0), "Item");
    w.gui.do_action(&ctx, Action::SortDescending);
    assert_eq!(w.value(1, 0), "fig");
}

#[test]
fn conditional_format_dialog_takes_a_clicked_cell() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    w.gui.set_cursor(1, 1);
    w.gui.do_action(&ctx, Action::ConditionalFormatting);
    w.frames(2);
    assert!(w.gui.pick_for_dialog(((2, 2), (2, 2))), "the click goes to the dialog");
    assert_eq!((w.gui.app.selected_row, w.gui.app.selected_col), (1, 1), "and doesn't move the cursor");
    w.frames(2);
    // On another sheet the reference names its sheet.
    w.gui.do_action(&ctx, Action::NewSheet);
    w.frames(1);
    assert!(w.gui.pick_for_dialog(((0, 1), (0, 1))));
    w.frames(2);
    w.gui.tools = Default::default();
    assert!(!w.gui.pick_for_dialog(((0, 0), (0, 0))), "no dialog, an ordinary click");
}

#[test]
fn reference_dialogs_take_dragged_cells_and_lists_get_a_dropdown() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    for action in [Action::GoalSeek, Action::FormatAsTable, Action::DataValidation, Action::NameManager] {
        w.gui.do_action(&ctx, action);
        w.frames(2);
        assert!(w.gui.pick_for_dialog(((1, 0), (3, 1))), "{:?} takes the dragged cells", action);
        w.frames(2);
        w.gui.tools = Default::default();
    }
    // A list rule on column A: the cursor cell shows a dropdown button.
    w.gui.app.set_validation(0, Some("COUNTIF(A2:A4, _) > 0".to_string()));
    assert_eq!(w.gui.app.validation_choices(0).map(|c| c.len()), Some(3));
    w.gui.set_cursor(6, 0);
    w.frames(3);
    w.enter(6, 0, "fig");
    assert_eq!(w.value(6, 0), "fig");
    w.gui.start_edit(Some("plum".to_string()), true);
    w.gui.commit_edit(EditMove::Stay);
    assert!(w.gui.tools.invalid.is_some(), "not in the list");
}

#[test]
fn find_dialog_searches_as_typed_and_steps_with_enter() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    w.gui.set_cursor(0, 0);
    w.gui.do_action(&ctx, Action::Find);
    w.frames(2);
    w.frame(vec![Event::Text("apple".to_string())]);
    w.frames(2);
    assert_eq!(w.gui.app.find_replace_results, [(2, 0), (4, 0)]);
    assert_eq!((w.gui.app.selected_row, w.gui.app.selected_col), (2, 0), "jumps to the first match");
    w.key(Key::Enter);
    w.frames(1);
    assert_eq!(w.gui.app.selected_row, 4);
    w.key(Key::Enter);
    w.frames(1);
    assert_eq!(w.gui.app.selected_row, 2, "wraps around");
}

#[test]
fn tab_inserts_the_highlighted_function() {
    let mut w = Window::new();
    w.gui.start_edit(Some("=SU".to_string()), true);
    w.frames(3);
    w.key(Key::ArrowDown);
    w.key(Key::Tab);
    w.frames(1);
    let edit = w.gui.edit.as_ref().expect("still editing");
    assert_eq!(edit.text, "=SUM(", "SUBSTITUTE is first, SUM second");
    w.frames(2);
}

#[test]
fn an_entry_breaking_validation_is_held_back() {
    let mut w = Window::new();
    w.gui.app.set_validation(0, Some("_ > 0".to_string()));
    w.gui.set_cursor(0, 0);
    w.gui.start_edit(Some("-5".to_string()), true);
    w.gui.commit_edit(EditMove::Down);
    assert!(w.gui.tools.invalid.is_some());
    assert_eq!(w.value(0, 0), "");
    w.frames(2);
    w.key(Key::Enter);
    assert_eq!(w.gui.edit.as_ref().map(|e| e.text.as_str()), Some("-5"), "Retry goes back to editing");
    w.gui.cancel_edit();
    w.enter(0, 0, "5");
    assert_eq!(w.value(0, 0), "5");
}

#[test]
fn name_box_goes_to_names_and_sheet_tabs_reorder() {
    let mut w = Window::with_data();
    let ctx = w.ctx.clone();
    w.gui.app.selection_start = Some((1, 1));
    w.gui.app.selection_end = Some((4, 1));
    w.gui.name_selection("Prices");
    w.gui.set_cursor(0, 0);
    w.gui.goto("prices");
    assert_eq!(w.gui.app.get_selection_range(), Some(((1, 1), (4, 1))));

    w.gui.do_action(&ctx, Action::DuplicateSheet);
    w.frames(2);
    assert_eq!(w.gui.app.workbook.sheet_names, ["Sheet1", "Sheet1 (2)"]);
    w.gui.do_action(&ctx, Action::MoveSheetLeft);
    assert_eq!(w.gui.app.workbook.sheet_names, ["Sheet1 (2)", "Sheet1"]);
    w.gui.goto("Sheet1!C3");
    assert_eq!(w.gui.app.workbook.active_sheet, 1);
    assert_eq!((w.gui.app.selected_row, w.gui.app.selected_col), (2, 2));
    w.frames(2);
}
