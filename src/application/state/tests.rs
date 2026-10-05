use super::*;
use crate::domain::CellData;

#[test]
fn test_app_default() {
    let app = App::default();
    assert_eq!(app.selected_row, 0);
    assert_eq!(app.selected_col, 0);
    assert_eq!(app.scroll_row, 0);
    assert_eq!(app.scroll_col, 0);
    assert!(matches!(app.mode, AppMode::Normal));
    assert!(app.input.is_empty());
    assert_eq!(app.cursor_position, 0);
    assert!(app.filename.is_none());
    assert!(app.status_message.is_none());
    assert!(app.filename_input.is_empty());
}

#[test]
fn test_cross_sheet_auto_recalc() {
    // Sheet2!A1 = Sheet1!A1 + 10. Editing Sheet1!A1 should auto-update
    // Sheet2!A1 without a manual F5.
    let mut app = App::default();
    app.workbook.add_sheet("Sheet2".to_string());
    // Sheet1!A1 = 5
    app.workbook.active_sheet = 0;
    app.set_cell_with_undo(0, 0, CellData {
        value: "5".to_string(),
        formula: None,
        format: None,
        comment: None,
    spill_anchor: None,
    });
    // Sheet2!A1 = =Sheet1!A1 + 10  (evaluates to 15)
    app.workbook.active_sheet = 1;
    app.set_cell_with_undo(0, 0, CellData {
        value: "15".to_string(),
        formula: Some("=Sheet1!A1 + 10".to_string()),
        format: None,
        comment: None,
    spill_anchor: None,
    });
    assert_eq!(app.workbook.sheets[1].get_cell(0, 0).value, "15");

    // Now change Sheet1!A1 to 20. Sheet2!A1 should auto-update to 30.
    app.workbook.active_sheet = 0;
    app.set_cell_with_undo(0, 0, CellData {
        value: "20".to_string(),
        formula: None,
        format: None,
        comment: None,
    spill_anchor: None,
    });
    assert_eq!(app.workbook.sheets[1].get_cell(0, 0).value, "30");
}

#[test]
fn test_cross_sheet_cycle_rejected() {
    let mut app = App::default();
    app.workbook.add_sheet("Sheet2".to_string());
    // Sheet1!A1 = =Sheet2!A1
    app.workbook.active_sheet = 0;
    app.start_editing();
    app.input = "=Sheet2!A1".to_string();
    app.cursor_position = app.input.chars().count();
    app.finish_editing();
    // Sheet2!A1 = =Sheet1!A1 — should be rejected (cross-sheet cycle).
    app.workbook.active_sheet = 1;
    app.start_editing();
    app.input = "=Sheet1!A1".to_string();
    app.cursor_position = app.input.chars().count();
    app.finish_editing();
    // The reject path returns early without writing the formula, so
    // Sheet2!A1 stays empty/uninitialized.
    let cell = app.workbook.sheets[1].get_cell(0, 0);
    assert!(cell.formula.is_none(), "expected cross-sheet cycle rejected, got formula={:?}", cell.formula);
}

#[test]
fn test_cross_sheet_chain_propagates() {
    // Three-link chain: Sheet1!A1 → Sheet2!A1 → Sheet3!A1.
    let mut app = App::default();
    app.workbook.add_sheet("Sheet2".to_string());
    app.workbook.add_sheet("Sheet3".to_string());

    app.workbook.active_sheet = 0;
    app.set_cell_with_undo(0, 0, CellData {
        value: "1".to_string(),
        formula: None,
        format: None,
        comment: None,
    spill_anchor: None,
    });
    app.workbook.active_sheet = 1;
    app.set_cell_with_undo(0, 0, CellData {
        value: "2".to_string(),
        formula: Some("=Sheet1!A1 + 1".to_string()),
        format: None,
        comment: None,
    spill_anchor: None,
    });
    app.workbook.active_sheet = 2;
    app.set_cell_with_undo(0, 0, CellData {
        value: "3".to_string(),
        formula: Some("=Sheet2!A1 + 1".to_string()),
        format: None,
        comment: None,
    spill_anchor: None,
    });
    assert_eq!(app.workbook.sheets[2].get_cell(0, 0).value, "3");

    // Bump the head of the chain.
    app.workbook.active_sheet = 0;
    app.set_cell_with_undo(0, 0, CellData {
        value: "10".to_string(),
        formula: None,
        format: None,
        comment: None,
    spill_anchor: None,
    });
    assert_eq!(app.workbook.sheets[1].get_cell(0, 0).value, "11");
    assert_eq!(app.workbook.sheets[2].get_cell(0, 0).value, "12");
}

#[test]
fn test_smoke_end_to_end_flow() {
    // High-level sanity check exercising the key flows wired by the
    // recent refactors. Exercises the App API only.
    let mut app = App::default();
    assert!(!app.dirty);
    assert!(!app.should_quit);

    // Start an edit and commit a value via the normal Editing flow.
    app.start_editing();
    app.input = "12".to_string();
    app.cursor_position = 2;
    app.finish_editing();
    assert_eq!(app.workbook.current_sheet().get_cell(0, 0).value, "12");
    assert!(app.dirty);

    // A formula with an absolute reference round-trips through autofill.
    app.selected_row = 0;
    app.selected_col = 1;
    app.start_editing();
    app.input = "=A1*$B$5".to_string();
    app.cursor_position = app.input.chars().count();
    app.finish_editing();
    let evaluator = crate::domain::FormulaEvaluator::new(app.workbook.current_sheet());
    // $-anchored part survives an autofill row shift.
    let shifted = evaluator.adjust_formula_references("=A1*$B$5", 1, 0);
    assert_eq!(shifted, "=A2*$B$5");

    // Dirty-aware quit prompts.
    app.dirty = true;
    app.request_quit();
    assert!(matches!(app.mode, AppMode::ConfirmDiscard));
    assert!(!app.should_quit);
    app.confirm_pending_action();
    assert!(app.should_quit);

    // Esc-dismiss clears transient state.
    let mut app2 = App::default();
    app2.search_results.push((1, 1));
    app2.status_message = Some("noise".to_string());
    app2.dismiss_transients();
    assert!(app2.search_results.is_empty());
    assert!(app2.status_message.is_none());

    // recalc_all is callable and idempotent.
    app2.recalc_all();
    assert_eq!(
        app2.status_message.as_deref(),
        Some("Recalculated all formulas")
    );
}

#[test]
fn test_vlookup_basic() {
    use crate::domain::FormulaEvaluator;
    let mut sheet = crate::domain::Spreadsheet::default();
    // Single-column lookup
    for (i, v) in ["a", "b", "c", "d"].iter().enumerate() {
        sheet.set_cell(i, 0, crate::domain::CellData {
            value: v.to_string(), formula: None, format: None, comment: None,
        spill_anchor: None,
        });
    }
    let evaluator = FormulaEvaluator::new(&sheet);
    assert_eq!(
        evaluator.evaluate_formula("=VLOOKUP(\"c\", A1:A4, 1, 0)"),
        "c"
    );
}

#[test]
fn test_app_mode_transitions() {
    let mut app = App::default();
    
    // Normal -> Editing -> Normal
    assert!(matches!(app.mode, AppMode::Normal));
    app.start_editing();
    assert!(matches!(app.mode, AppMode::Editing));
    app.finish_editing();
    assert!(matches!(app.mode, AppMode::Normal));
    
    // Normal -> SaveAs -> Normal
    app.start_save_as();
    assert!(matches!(app.mode, AppMode::SaveAs));
    app.cancel_filename_input();
    assert!(matches!(app.mode, AppMode::Normal));
    
    // Normal -> LoadFile -> Normal
    app.start_load_file();
    assert!(matches!(app.mode, AppMode::LoadFile));
    app.cancel_filename_input();
    assert!(matches!(app.mode, AppMode::Normal));
}

#[test]
fn test_status_message_handling() {
    let mut app = App::default();
    
    // Initially no status message
    assert!(app.status_message.is_none());
    
    // Save success sets status message
    app.set_save_result(Ok("test.cellar".to_string()));
    assert!(app.status_message.is_some());
    
    // Starting save dialog clears status message
    app.start_save_as();
    assert!(app.status_message.is_none());
    
    // Load failure sets status message
    app.set_load_workbook_result(Err("Error".to_string()));
    assert!(app.status_message.is_some());
    
    // Starting load dialog clears status message
    app.start_load_file();
    assert!(app.status_message.is_none());
}

#[test]
fn test_selection_functionality() {
    let mut app = App::default();
    
    // Initially no selection
    assert!(app.get_selection_range().is_none());
    assert!(!app.is_cell_selected(0, 0));
    
    // Start selection
    app.start_selection();
    assert_eq!(app.get_selection_range(), Some(((0, 0), (0, 0))));
    assert!(app.is_cell_selected(0, 0));
    
    // Update selection
    app.update_selection(1, 2);
    assert_eq!(app.get_selection_range(), Some(((0, 0), (1, 2))));
    assert!(app.is_cell_selected(0, 1));
    assert!(app.is_cell_selected(1, 2));
    assert!(!app.is_cell_selected(2, 0));
    
    // Clear selection
    app.clear_selection();
    assert!(app.get_selection_range().is_none());
    assert!(!app.is_cell_selected(0, 0));
}

#[test]
fn test_viewport_and_scrolling() {
    let mut app = App::default();
    
    // Test initial viewport size
    assert_eq!(app.viewport_rows, 20);
    assert_eq!(app.viewport_cols, 8);
    
    // Test updating viewport size
    app.update_viewport_size(15, 10);
    assert_eq!(app.viewport_rows, 15);
    assert_eq!(app.viewport_cols, 10);
    
    // Test ensure_cursor_visible - cursor within viewport
    app.selected_row = 5;
    app.selected_col = 3;
    app.scroll_row = 0;
    app.scroll_col = 0;
    app.ensure_cursor_visible();
    assert_eq!(app.scroll_row, 0);  // No need to scroll
    assert_eq!(app.scroll_col, 0);
    
    // Test ensure_cursor_visible - cursor beyond bottom/right
    app.selected_row = 20;  // Beyond viewport (15 rows)
    app.selected_col = 12;  // Beyond viewport (10 cols)
    app.ensure_cursor_visible();
    assert_eq!(app.scroll_row, 6);  // 20 - 15 + 1 = 6
    assert_eq!(app.scroll_col, 3);  // 12 - 10 + 1 = 3
    
    // Test ensure_cursor_visible - cursor before top/left
    app.selected_row = 2;
    app.selected_col = 1;
    app.ensure_cursor_visible();
    assert_eq!(app.scroll_row, 2);  // Scroll to show cursor
    assert_eq!(app.scroll_col, 1);
}

#[test]
fn test_selection_stats() {
    let mut app = App::default();
    app.set_cell_with_undo(0, 0, CellData { value: "10".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
    app.set_cell_with_undo(1, 0, CellData { value: "20".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
    app.set_cell_with_undo(2, 0, CellData { value: "30".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

    app.selection_start = Some((0, 0));
    app.selection_end = Some((2, 0));

    let stats = app.get_selection_stats();
    assert!(stats.is_some());
    let (sum, avg, count) = stats.unwrap();
    assert_eq!(sum, 60.0);
    assert_eq!(avg, 20.0);
    assert_eq!(count, 3);
}

#[test]
fn test_selection_stats_single_cell() {
    let mut app = App::default();
    app.set_cell_with_undo(0, 0, CellData { value: "10".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

    app.selection_start = Some((0, 0));
    app.selection_end = Some((0, 0));

    // Single-cell selections now also publish stats (SUM=value,
    // AVG=value, COUNT=1) — useful for the user (the value of the
    // cell the cursor sits on) and required by the scenario test
    // framework's status-bar value reads.
    let (sum, avg, count) = app.get_selection_stats()
        .expect("single-cell selection should yield stats");
    assert_eq!(sum, 10.0);
    assert_eq!(avg, 10.0);
    assert_eq!(count, 1);
}

#[test]
fn test_selection_stats_no_numbers() {
    let mut app = App::default();
    app.set_cell_with_undo(0, 0, CellData { value: "hello".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
    app.set_cell_with_undo(1, 0, CellData { value: "world".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

    app.selection_start = Some((0, 0));
    app.selection_end = Some((1, 0));

    // No numeric values should return None
    assert!(app.get_selection_stats().is_none());
}

#[test]
fn test_batch_undo() {
    let mut app = App::default();
    app.set_cell_with_undo(0, 0, CellData { value: "A".to_string(), formula: None, format: None, comment: None, spill_anchor: None });
    app.set_cell_with_undo(1, 0, CellData { value: "B".to_string(), formula: None, format: None, comment: None, spill_anchor: None });

    // Cut = batch undo of clearing cells
    app.selection_start = Some((0, 0));
    app.selection_end = Some((1, 0));
    app.cut_selection();

    assert!(app.workbook.current_sheet().get_cell(0, 0).value.is_empty());
    assert!(app.workbook.current_sheet().get_cell(1, 0).value.is_empty());

    // Single undo should restore both cells
    app.undo();

    assert_eq!(app.workbook.current_sheet().get_cell(0, 0).value, "A");
    assert_eq!(app.workbook.current_sheet().get_cell(1, 0).value, "B");
}

#[test]
fn test_terminal_color_from_name() {
    assert_eq!(TerminalColor::from_name("red"), Some(TerminalColor::Red));
    assert_eq!(TerminalColor::from_name("Blue"), Some(TerminalColor::Blue));
    assert_eq!(TerminalColor::from_name("lightgreen"), Some(TerminalColor::LightGreen));
    assert_eq!(TerminalColor::from_name("CYAN"), Some(TerminalColor::Cyan));
    assert_eq!(TerminalColor::from_name("invalid"), None);
}

#[test]
fn test_parse_column_label() {
    use crate::domain::Spreadsheet;
    assert_eq!(Spreadsheet::parse_column_label("A"), Some(0));
    assert_eq!(Spreadsheet::parse_column_label("B"), Some(1));
    assert_eq!(Spreadsheet::parse_column_label("Z"), Some(25));
    assert_eq!(Spreadsheet::parse_column_label("AA"), Some(26));
    assert_eq!(Spreadsheet::parse_column_label("a"), Some(0));
    assert_eq!(Spreadsheet::parse_column_label(""), None);
    assert_eq!(Spreadsheet::parse_column_label("1"), None);
}


#[test]
fn undo_and_redo_of_a_cell_edit_apply_to_the_sheet_it_was_made_on() {
    let mut app = App::default();
    app.workbook.add_sheet("Sheet2".to_string());
    app.set_cell_with_undo(0, 0, CellData { value: "on sheet 1".into(), ..CellData::default() });
    app.switch_to_sheet(1);
    app.set_cell_with_undo(0, 0, CellData { value: "on sheet 2".into(), ..CellData::default() });
    app.switch_to_sheet(0);

    // The last edit was on Sheet2: undo goes there and clears it, leaving
    // Sheet1 alone.
    app.undo();
    assert_eq!(app.workbook.active_sheet, 1);
    assert_eq!(app.workbook.sheets[1].get_cell(0, 0).value, "");
    assert_eq!(app.workbook.sheets[0].get_cell(0, 0).value, "on sheet 1");

    app.switch_to_sheet(0);
    app.redo();
    assert_eq!(app.workbook.active_sheet, 1);
    assert_eq!(app.workbook.sheets[1].get_cell(0, 0).value, "on sheet 2");
    assert_eq!(app.workbook.sheets[0].get_cell(0, 0).value, "on sheet 1");
}

/// GETPIVOTDATA reads a PivotTable from a formula and follows changes to
/// the source data (including data computed by formulas), to the pivot's
/// definition and name, and to its removal.
#[test]
fn getpivotdata_tracks_data_and_pivot_changes() {
    use crate::domain::{PivotField, PivotSpec, PivotValue, Summarize};
    fn enter(app: &mut App, sheet: usize, row: usize, col: usize, text: &str) {
        app.switch_to_sheet(sheet);
        app.selected_row = row;
        app.selected_col = col;
        app.input = text.to_string();
        app.mode = AppMode::Editing;
        app.finish_editing();
    }
    let value = |app: &App, sheet: usize, row: usize, col: usize| app.workbook.sheets[sheet].get_cell(row, col).value;

    let mut app = App::default();
    app.workbook.add_sheet("Report".to_string());
    let rows = [["Region", "Revenue", "Units"], ["North", "100", "1"], ["South", "50", "2"], ["North", "=C4*10", "7"]];
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            enter(&mut app, 0, r, c, text);
        }
    }
    app.switch_to_sheet(0);
    let mut spec = PivotSpec::new("PivotTable1", "A1:B10");
    spec.rows = vec![PivotField::new("Region")];
    spec.values = vec![PivotValue::new("Revenue", Summarize::Sum)];
    app.add_pivot(spec.clone());

    enter(&mut app, 1, 0, 0, r#"=GETPIVOTDATA("Sum of Revenue","PivotTable1","Region","North")"#);
    enter(&mut app, 1, 1, 0, r#"=GETPIVOTDATA("Revenue","PivotTable1")"#);
    assert_eq!(value(&app, 1, 0, 0), "170");
    assert_eq!(value(&app, 1, 1, 0), "220");

    // A source value, a value computed by a formula, and a new row.
    enter(&mut app, 0, 1, 1, "150");
    assert_eq!(value(&app, 1, 0, 0), "220");
    enter(&mut app, 0, 3, 2, "8");
    assert_eq!(value(&app, 1, 0, 0), "230");
    enter(&mut app, 0, 4, 0, "North");
    enter(&mut app, 0, 4, 1, "5");
    assert_eq!(value(&app, 1, 0, 0), "235");

    // A formula inside the pivot's own source would be circular.
    enter(&mut app, 0, 5, 1, r#"=GETPIVOTDATA("Sum of Revenue","PivotTable1")"#);
    assert!(app.workbook.sheets[0].get_cell(5, 1).formula.is_none());

    // Renaming the pivot keeps the formulas working; undo restores both.
    app.switch_to_sheet(0);
    let mut renamed = app.workbook.sheets[0].pivots[0].clone();
    renamed.name = "Sales".to_string();
    app.replace_pivot(0, renamed);
    let formula = app.workbook.sheets[1].get_cell(0, 0).formula.unwrap();
    assert!(formula.contains(r#""Sheet1!Sales""#), "{formula}");
    assert_eq!(value(&app, 1, 0, 0), "235");
    app.undo();
    assert!(app.workbook.sheets[1].get_cell(0, 0).formula.unwrap().contains(r#""PivotTable1""#));

    // Hiding an item recalculates, and makes it unreachable like Excel.
    app.switch_to_sheet(0);
    let mut hidden = app.workbook.sheets[0].pivots[0].clone();
    hidden.rows[0].hidden_items = vec!["South".to_string()];
    app.replace_pivot(0, hidden);
    assert_eq!(value(&app, 1, 1, 0), "235");

    app.remove_pivot(0);
    assert_eq!(value(&app, 1, 0, 0), "#REF!");
}


/// Renaming a sheet keeps everything that names it working: cell formulas,
/// a PivotTable whose source is on it, GETPIVOTDATA calls that name a pivot
/// as `Sheet!Pivot`, and chart ranges.
#[test]
fn renaming_a_sheet_keeps_references_to_it_working() {
    use crate::domain::{ChartSeries, ChartSpec, PivotField, PivotSpec, PivotValue, Summarize};
    fn enter(app: &mut App, sheet: usize, row: usize, col: usize, text: &str) {
        app.switch_to_sheet(sheet);
        app.selected_row = row;
        app.selected_col = col;
        app.input = text.to_string();
        app.mode = AppMode::Editing;
        app.finish_editing();
    }
    let value = |app: &App, sheet: usize, row: usize, col: usize| app.workbook.sheets[sheet].get_cell(row, col).value;

    let mut app = App::default();
    app.workbook.add_sheet("Pivots".to_string());
    app.workbook.add_sheet("Report".to_string());
    let rows = [["Region", "Revenue"], ["North", "100"], ["South", "50"], ["North", "20"]];
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row.iter().enumerate() {
            enter(&mut app, 0, r, c, text);
        }
    }
    // The pivot lives on "Pivots" and reads "Sheet1".
    app.switch_to_sheet(1);
    let mut spec = PivotSpec::new("PivotTable1", "Sheet1!A1:B4");
    spec.rows = vec![PivotField::new("Region")];
    spec.values = vec![PivotValue::new("Revenue", Summarize::Sum)];
    app.add_pivot(spec);
    app.workbook.sheets[1].charts.push(ChartSpec {
        title: "Revenue".into(),
        chart_type: Default::default(),
        pivot: None,
        categories: Some("Sheet1!A2:A4".into()),
        series: vec![ChartSeries { name: Some("Sheet1!B1".into()), values: "Sheet1!B2:B4".into() }],
    });

    enter(&mut app, 2, 0, 0, "=Sheet1!B2*2");
    enter(&mut app, 2, 1, 0, r#"=GETPIVOTDATA("Sum of Revenue","Pivots!PivotTable1","Region","North")"#);
    assert_eq!(value(&app, 2, 0, 0), "200");
    assert_eq!(value(&app, 2, 1, 0), "120");

    app.switch_to_sheet(0);
    assert!(app.workbook.rename_sheet("Raw data".to_string()));
    app.switch_to_sheet(1);
    assert!(app.workbook.rename_sheet("Summary".to_string()));

    let pivots = &app.workbook.sheets[1];
    assert_eq!(pivots.pivots[0].source, "'Raw data'!A1:B4");
    let chart = &pivots.charts[0];
    assert_eq!(chart.categories.as_deref(), Some("'Raw data'!A2:A4"));
    assert_eq!(chart.series[0].name.as_deref(), Some("'Raw data'!B1"));
    assert_eq!(chart.series[0].values, "'Raw data'!B2:B4");
    assert!(
        app.workbook.sheets[2].get_cell(1, 0).formula.unwrap().contains(r#""Summary!PivotTable1""#),
        "GETPIVOTDATA's sheet-qualified pivot name follows the rename"
    );

    // Still live after the renames.
    enter(&mut app, 0, 1, 1, "300");
    assert_eq!(value(&app, 2, 0, 0), "600");
    assert_eq!(value(&app, 2, 1, 0), "320");
}


/// Renaming, adding and removing pivots never points a GETPIVOTDATA formula
/// at a different pivot or rewrites anything but the pivot's name.
#[test]
fn pivot_renames_keep_getpivotdata_formulas_on_their_pivot() {
    use crate::domain::{PivotField, PivotSpec, PivotValue, Summarize};
    fn enter(app: &mut App, sheet: usize, row: usize, col: usize, text: &str) {
        app.switch_to_sheet(sheet);
        app.selected_row = row;
        app.selected_col = col;
        app.input = text.to_string();
        app.mode = AppMode::Editing;
        app.finish_editing();
    }
    fn pivot(name: &str, value: &str) -> PivotSpec {
        let mut spec = PivotSpec::new(name, "A1:B4");
        spec.rows = vec![PivotField::new("Dept")];
        spec.values = vec![PivotValue::new(value, Summarize::Sum)];
        spec
    }
    let formula = |app: &App, sheet: usize, row: usize| app.workbook.sheets[sheet].get_cell(row, 0).formula.unwrap();
    let value = |app: &App, sheet: usize, row: usize| app.workbook.sheets[sheet].get_cell(row, 0).value;

    // Two data sheets with a pivot each; a report sheet in between.
    let mut app = App::default();
    app.workbook.add_sheet("Report".to_string());
    app.workbook.add_sheet("Other".to_string());
    for sheet in [0, 2] {
        let k = if sheet == 0 { "1" } else { "1000" };
        let rows = [["Dept", "Amount"], ["Sales", k], ["Ops", k], ["Sales", k]];
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                enter(&mut app, sheet, r, c, text);
            }
        }
    }
    app.switch_to_sheet(2);
    app.add_pivot(pivot("Sales", "Amount"));
    // The report uses "Sales" plainly; only Other has a pivot by that name.
    // The item is also called "Sales".
    enter(&mut app, 1, 0, 0, r#"=GETPIVOTDATA("Sum of Amount","Sales","Dept","Sales")"#);
    assert_eq!(value(&app, 1, 0), "2000");

    // A new "Sales" pivot on the first sheet would be found first; the
    // report keeps reading Other's pivot.
    app.switch_to_sheet(0);
    app.add_pivot(pivot("Sales", "Amount"));
    assert_eq!(formula(&app, 1, 0), r#"=GETPIVOTDATA("Sum of Amount","Other!Sales","Dept","Sales")"#);
    assert_eq!(value(&app, 1, 0), "2000");

    // Renaming Other's pivot follows it; the "Sales" item stays.
    app.switch_to_sheet(2);
    app.replace_pivot(0, PivotSpec { name: "Revenue".into(), ..app.workbook.sheets[2].pivots[0].clone() });
    assert_eq!(formula(&app, 1, 0), r#"=GETPIVOTDATA("Sum of Amount","Other!Revenue","Dept","Sales")"#);
    assert_eq!(value(&app, 1, 0), "2000");

    // A plain name used on the pivot's own sheet stays plain.
    enter(&mut app, 2, 9, 0, r#"=GETPIVOTDATA("Sum of Amount","Revenue")"#);
    app.switch_to_sheet(2);
    app.replace_pivot(0, PivotSpec { name: "Income".into(), ..app.workbook.sheets[2].pivots[0].clone() });
    assert_eq!(formula(&app, 2, 9), r#"=GETPIVOTDATA("Sum of Amount","Income")"#);
    assert_eq!(value(&app, 2, 9), "3000");

    // Removing the pivot: #REF!, not the first sheet's same-named pivot.
    app.switch_to_sheet(0);
    app.replace_pivot(0, PivotSpec { name: "Income".into(), ..app.workbook.sheets[0].pivots[0].clone() });
    app.switch_to_sheet(2);
    app.remove_pivot(0);
    assert_eq!(value(&app, 1, 0), "#REF!");
    assert_eq!(value(&app, 2, 9), "#REF!");
    app.undo();
    assert_eq!(value(&app, 1, 0), "2000");
    assert_eq!(value(&app, 2, 9), "3000");

    assert_eq!(app.next_pivot_name(), "PivotTable1");
    app.workbook.sheets[2].pivots[0].name = "pivottable1".into();
    assert_eq!(app.next_pivot_name(), "PivotTable2");
}
