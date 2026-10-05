//! Submodule of `state` — see state/mod.rs.
//!
//! PivotTable definitions on the active sheet. Each change is one
//! snapshot-undo step. PivotCharts reference pivots by name, so renames
//! and deletions are applied to them as well.
//!
//! GETPIVOTDATA formulas name pivots too. A plain name is looked up on the
//! formula's own sheet first, then on the other sheets in tab order, so
//! adding, renaming or removing a pivot could quietly point a formula at a
//! different pivot. Before each change the affected names are rewritten
//! (qualified as `Sheet!Pivot` where needed) so every formula keeps
//! reading the pivot it read before.

use super::*;
use crate::domain::{find_pivot, map_getpivotdata_names, PivotSpec, Workbook};

/// Where the pivot name `name`, used in a formula on sheet `sheet`, points:
/// (sheet index, pivot index).
fn resolve(wb: &Workbook, sheet: usize, name: &str) -> Option<(usize, usize)> {
    let found = find_pivot(Some(wb), &wb.sheets[sheet], name)?;
    let owner = match found.sheet_name {
        Some(n) => wb.sheet_names.iter().position(|s| s == n)?,
        None => sheet,
    };
    let index = wb.sheets[owner].pivots.iter().position(|p| std::ptr::eq(p, found.spec))?;
    Some((owner, index))
}

impl App {
    /// First unused `PivotTable<N>` name on the active sheet.
    pub fn next_pivot_name(&self) -> String {
        let sheet = self.workbook.current_sheet();
        (1..)
            .map(|n| format!("PivotTable{}", n))
            .find(|name| !sheet.pivots.iter().any(|p| p.name.eq_ignore_ascii_case(name)))
            .unwrap_or_default()
    }

    pub fn add_pivot(&mut self, pivot: PivotSpec) {
        let name = pivot.name.clone();
        self.with_snapshot_undo("add pivot table", |app| {
            app.pin_pivot_name(&pivot.name);
            app.workbook.current_sheet_mut().pivots.push(pivot);
            app.refresh_pivot_formulas();
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
            if old_name != pivot.name {
                if !old_name.eq_ignore_ascii_case(&pivot.name) {
                    app.pin_pivot_name(&pivot.name);
                }
                app.retarget_pivot_refs(index, Some(&pivot.name));
            }
            let sheet = app.workbook.current_sheet_mut();
            for chart in sheet.charts.iter_mut().filter(|c| c.pivot.as_deref() == Some(&old_name)) {
                chart.pivot = Some(pivot.name.clone());
            }
            sheet.pivots[index] = pivot;
            app.refresh_pivot_formulas();
        });
    }

    /// Remove a pivot and the PivotCharts built on it.
    pub fn remove_pivot(&mut self, index: usize) {
        let Some(name) = self.workbook.current_sheet().pivots.get(index).map(|p| p.name.clone()) else { return };
        self.with_snapshot_undo("remove pivot table", |app| {
            app.retarget_pivot_refs(index, None);
            let sheet = app.workbook.current_sheet_mut();
            sheet.pivots.remove(index);
            sheet.charts.retain(|c| c.pivot.as_deref() != Some(&name));
            app.refresh_pivot_formulas();
        });
        self.status_message = Some(format!("{} removed", name));
    }

    /// Recalculate GETPIVOTDATA formulas after a pivot definition changed.
    fn refresh_pivot_formulas(&mut self) {
        if let Err(e) = self.workbook.refresh_pivot_formulas() {
            self.status_message = Some(format!("Recalc: {}", e));
        }
    }

    /// Rewrite the pivot names of GETPIVOTDATA formulas: `f(wb, sheet,
    /// name)` gets each name with the index of the formula's sheet and
    /// returns its replacement, if any.
    fn rewrite_pivot_refs(&mut self, f: impl Fn(&Workbook, usize, &str) -> Option<String>) {
        let wb = &self.workbook;
        let mut changes = Vec::new();
        for (s, sheet) in wb.sheets.iter().enumerate() {
            for (&pos, cd) in &sheet.cells {
                if let Some(new) = cd.formula.as_deref().and_then(|formula| {
                    map_getpivotdata_names(formula, |name| f(wb, s, name))
                }) {
                    changes.push((s, pos, new));
                }
            }
        }
        for (s, pos, formula) in changes {
            if let Some(cd) = self.workbook.sheets[s].cells.get_mut(&pos) {
                cd.formula = Some(formula);
            }
        }
    }

    /// Before a pivot named `name` appears on the active sheet: formulas
    /// that use `name` plainly for a pivot on another sheet get that
    /// sheet's name, so the new pivot can't take them over.
    fn pin_pivot_name(&mut self, name: &str) {
        self.rewrite_pivot_refs(|wb, s, used| {
            if !used.trim().eq_ignore_ascii_case(name) {
                return None;
            }
            let (owner, _) = resolve(wb, s, used).filter(|(owner, _)| *owner != s)?;
            Some(format!("{}!{}", wb.sheet_names[owner], used.trim()))
        });
    }

    /// Formulas reading pivot `index` of the active sheet: point them at its
    /// new name, or for a removal, name its sheet so they show #REF!
    /// instead of falling through to a same-named pivot elsewhere. Names
    /// used from other sheets are written `Sheet!Pivot`.
    fn retarget_pivot_refs(&mut self, index: usize, new_name: Option<&str>) {
        let host = self.workbook.active_sheet;
        let Some(old) = self.workbook.sheets[host].pivots.get(index).map(|p| p.name.clone()) else { return };
        let name = new_name.unwrap_or(&old).to_string();
        self.rewrite_pivot_refs(|wb, s, used| {
            if resolve(wb, s, used) != Some((host, index)) {
                return None;
            }
            Some(if s == host && new_name.is_some() {
                name.clone()
            } else {
                format!("{}!{}", wb.sheet_names[host], name)
            })
        });
    }
}
