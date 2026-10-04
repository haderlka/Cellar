//! Submodule of `state` — see state/mod.rs.
//!
//! Persisted chart definitions on the active sheet. Each change is one
//! snapshot-undo step, like other structural edits.

use super::*;
use crate::domain::ChartSpec;

impl App {
    pub fn add_chart(&mut self, chart: ChartSpec) {
        let title = chart.title.clone();
        self.with_snapshot_undo("add chart", |app| {
            app.workbook.current_sheet_mut().charts.push(chart);
        });
        self.status_message = Some(format!("Chart \"{}\" added", title));
    }

    pub fn replace_chart(&mut self, index: usize, chart: ChartSpec) {
        if index >= self.workbook.current_sheet().charts.len() {
            return;
        }
        self.with_snapshot_undo("edit chart", |app| {
            app.workbook.current_sheet_mut().charts[index] = chart;
        });
    }

    pub fn remove_chart(&mut self, index: usize) {
        if index >= self.workbook.current_sheet().charts.len() {
            return;
        }
        self.with_snapshot_undo("remove chart", |app| {
            app.workbook.current_sheet_mut().charts.remove(index);
        });
        self.status_message = Some("Chart removed".to_string());
    }
}
