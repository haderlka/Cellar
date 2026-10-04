//! Submodule of `state` — see state/mod.rs.
//!
//! PivotTable definitions on the active sheet. Each change is one
//! snapshot-undo step. PivotCharts reference pivots by name, so renames
//! and deletions are applied to them as well.

use super::*;
use crate::domain::PivotSpec;

impl App {
    /// First unused `PivotTable<N>` name on the active sheet.
    pub fn next_pivot_name(&self) -> String {
        let sheet = self.workbook.current_sheet();
        (1..)
            .map(|n| format!("PivotTable{}", n))
            .find(|name| !sheet.pivots.iter().any(|p| &p.name == name))
            .unwrap_or_default()
    }

    pub fn add_pivot(&mut self, pivot: PivotSpec) {
        let name = pivot.name.clone();
        self.with_snapshot_undo("add pivot table", |app| {
            app.workbook.current_sheet_mut().pivots.push(pivot);
        });
        self.status_message = Some(format!("{} added", name));
    }

    pub fn replace_pivot(&mut self, index: usize, pivot: PivotSpec) {
        let Some(old) = self.workbook.current_sheet().pivots.get(index) else { return };
        if *old == pivot {
            return;
        }
        let old_name = old.name.clone();
        self.with_snapshot_undo("edit pivot table", |app| {
            let sheet = app.workbook.current_sheet_mut();
            for chart in sheet.charts.iter_mut().filter(|c| c.pivot.as_deref() == Some(&old_name)) {
                chart.pivot = Some(pivot.name.clone());
            }
            sheet.pivots[index] = pivot;
        });
    }

    /// Remove a pivot and the PivotCharts built on it.
    pub fn remove_pivot(&mut self, index: usize) {
        let Some(name) = self.workbook.current_sheet().pivots.get(index).map(|p| p.name.clone()) else { return };
        self.with_snapshot_undo("remove pivot table", |app| {
            let sheet = app.workbook.current_sheet_mut();
            sheet.pivots.remove(index);
            sheet.charts.retain(|c| c.pivot.as_deref() != Some(&name));
        });
        self.status_message = Some(format!("{} removed", name));
    }
}
