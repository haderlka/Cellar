//! Scenario tests: whole spreadsheet models cross-checked against an
//! independent pure-Rust implementation.
//!
//! ## Why this exists
//!
//! Unit tests verify that `=SUM(A1:A3)` returns the right number. They do
//! not catch *semantic drift* — e.g., "this DCF model produces enterprise
//! value 4% off because PMT's sign convention differs from what an analyst
//! would expect." These scenarios (written for tshts, which Cellar is built
//! on) catch exactly that.
//!
//! ## How it works
//!
//! Each scenario implements three things:
//!
//! 1. **`compute(inputs)`** — a pure-Rust ground-truth calculation.
//! 2. **`populate(harness, inputs)`** — types the same model into a
//!    spreadsheet (literals + formulas) through Cellar's editing session.
//! 3. **`checks(output)`** — `(cell-address, expected-value)` pairs derived
//!    from the ground truth.
//!
//! The runner computes the truth, populates the sheet, recalculates
//! everything, and compares each checked cell within its tolerance.
//!
//! Scenarios should NOT hard-code expected values from a Cellar run — they
//! DERIVE them from `compute()` so the model and the engine have to agree.

use crate::common::Harness;

/// One assertion in a scenario run.
#[derive(Debug, Clone)]
pub struct CellCheck {
    /// Human-readable label shown in failure messages, e.g.
    /// "enterprise_value" or "month_3_balance". Keep concise.
    pub label: String,
    /// A1-style cell address, e.g. "F12" or "Sheet2!A1".
    pub cell: String,
    /// Ground-truth expected numeric value (from the Rust model).
    pub expected: f64,
    /// Absolute tolerance for the comparison. Use ≥ 1e-6 for plain
    /// arithmetic; loosen to ~0.01 for cells that go through
    /// `=ROUND(...)` or display formatting.
    pub tolerance: f64,
}

impl CellCheck {
    pub fn new(label: impl Into<String>, cell: impl Into<String>, expected: f64) -> Self {
        Self {
            label: label.into(),
            cell: cell.into(),
            expected,
            tolerance: 1e-6,
        }
    }

    pub fn with_tolerance(mut self, tol: f64) -> Self {
        self.tolerance = tol;
        self
    }
}

/// A characteristic spreadsheet use case wired up as a Rust
/// ground-truth model + a Cellar population routine + a list of
/// post-recalc cell assertions.
pub trait Scenario {
    /// Inputs that parameterize the model. The framework calls
    /// [`Scenario::default_inputs`] today; future runs may sweep over
    /// inputs to fuzz the model.
    type Inputs: Clone;
    /// Ground-truth output of the Rust model. Opaque to the framework;
    /// only [`Scenario::checks`] reads it.
    type Output;

    /// Short display name used in failure messages and test logging.
    fn name(&self) -> &'static str;

    /// Default inputs — typically the example that the scenario is
    /// motivated by. Implementations may expose more for ad-hoc sweeps.
    fn default_inputs(&self) -> Self::Inputs;

    /// Compute the expected outputs in pure Rust.
    ///
    /// This is the GROUND TRUTH. If Cellar disagrees, either Cellar has a
    /// bug or this Rust model has a bug — both are valid findings.
    fn compute(&self, inputs: &Self::Inputs) -> Self::Output;

    /// Populate the spreadsheet with literals and formulas matching the
    /// model.
    fn populate(&self, h: &mut Harness, inputs: &Self::Inputs);

    /// Build the list of cell checks from the ground-truth output.
    /// Each check ties a spreadsheet cell back to a Rust-computed value.
    fn checks(&self, output: &Self::Output) -> Vec<CellCheck>;

    /// Optional rendered-text checks — for cells where the user-visible
    /// rendering (currency-formatted, percent-formatted, etc.) is what
    /// matters and the raw numeric value is only an internal detail.
    /// Default empty: most scenarios only care about numeric correctness.
    /// Implementations that test number formatting override this.
    fn rendered_text_checks(&self, _output: &Self::Output) -> Vec<RenderedTextCheck> {
        Vec::new()
    }
}

/// Asserts that a row's displayed text (number formats applied) contains
/// `expected_substring`. Use for currency / percent checks, where the raw
/// value doesn't show the formatting.
#[derive(Debug, Clone)]
pub struct RenderedTextCheck {
    pub label: String,
    /// Spreadsheet row, 1-indexed (matches A1 notation).
    pub row: u16,
    /// Substring expected somewhere in the row's rendered text.
    pub expected_substring: String,
}

impl RenderedTextCheck {
    pub fn new(label: impl Into<String>, row: u16, expected_substring: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            row,
            expected_substring: expected_substring.into(),
        }
    }
}

/// Run a scenario end-to-end against a fresh harness.
///
/// Panics with a structured multi-line message if any check fails.
pub fn run<S: Scenario>(s: &S) {
    let inputs = s.default_inputs();
    let truth = s.compute(&inputs);
    let mut h = Harness::new();
    s.populate(&mut h, &inputs);
    h.recalc();

    let all_checks = s.checks(&truth);
    let mut failures = Vec::new();
    for check in &all_checks {
        match read_cell_numeric(&mut h, &check.cell) {
            Ok(v) => {
                let delta = (v - check.expected).abs();
                if delta > check.tolerance {
                    failures.push(format!(
                        "  {label} ({cell}): expected {expected:.6}, Cellar shows \
                         {actual:.6} (|Δ|={delta:.6}, tol={tol})",
                        label = check.label,
                        cell = check.cell,
                        expected = check.expected,
                        actual = v,
                        delta = delta,
                        tol = check.tolerance,
                    ));
                }
            }
            Err(msg) => failures.push(format!(
                "  {label} ({cell}): expected {expected:.6}, read error: {msg}",
                label = check.label,
                cell = check.cell,
                expected = check.expected,
                msg = msg,
            )),
        }
    }

    let render_checks = s.rendered_text_checks(&truth);
    for check in &render_checks {
        let row = h.displayed_row(check.row as usize);
        if !row.contains(&check.expected_substring) {
            failures.push(format!(
                "  {label} (row {row}): expected to contain {expected:?}, \
                 row displays as {actual:?}",
                label = check.label,
                row = check.row,
                expected = check.expected_substring,
                actual = row.trim(),
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "[scenario:{name}] {failed} of {total} checks failed:\n{detail}",
            name = s.name(),
            failed = failures.len(),
            total = all_checks.len() + render_checks.len(),
            detail = failures.join("\n"),
        );
    }
}

// ---------- Helpers used by the scenarios ----------

/// Move the cursor to `addr` (`"A1"`, `"Sheet2!A1"`).
pub fn goto_cell(h: &mut Harness, addr: &str) {
    h.goto(addr);
}

/// Numeric value at `addr`, or an error if the cell isn't a number.
pub fn read_cell_numeric(h: &mut Harness, addr: &str) -> Result<f64, String> {
    let v = h.value(addr);
    v.trim().parse::<f64>().map_err(|_| format!("not a number: {:?}", v))
}

/// Type a literal or formula into `addr` and commit it.
pub fn enter_cell(h: &mut Harness, addr: &str, content: &str) {
    h.enter(addr, content);
}

/// Bulk-populate a list of (address, content) pairs.
pub fn enter_cells(h: &mut Harness, cells: &[(&str, &str)]) {
    for (addr, content) in cells {
        enter_cell(h, addr, content);
    }
}

// ---------- Numeric helpers shared by scenarios ----------

/// Wrap an A1-style address (single cell `"A2"` or range `"A2:B6"`)
/// in fully-absolute references: `"A2" → "$A$2"`, `"A2:B6" → "$A$2:$B$6"`.
/// Idempotent. Scenarios reach for this instead of hand-formatting
/// because `format!("${}", range)` produces a single literal `$`
/// followed by `range`, which is NOT absolute on both endpoints —
/// a subtle gotcha that bit the `lookup` and `currency` scenarios
/// during development.
pub fn abs_range(addr: &str) -> String {
    addr.split(':').map(abs_cell).collect::<Vec<_>>().join(":")
}

/// Wrap a single A1-style cell address in fully-absolute references.
/// Idempotent.
pub fn abs_cell(cell: &str) -> String {
    let bytes = cell.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'$') { i += 1; }
    let col_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let col_end = i;
    if bytes.get(i) == Some(&b'$') { i += 1; }
    let col_letters = &cell[col_start..col_end];
    let row_digits = &cell[i..];
    format!("${}${}", col_letters, row_digits)
}

/// Format an `f64` for inclusion in a Cellar formula. Avoids scientific
/// notation, which the parser doesn't accept, and keeps full precision.
pub fn lit(x: f64) -> String {
    // {:?} on f64 prints the shortest decimal that round-trips, in
    // standard notation for normal-range numbers. Falls back to a fixed
    // representation for very large/small magnitudes to avoid scientific
    // notation in formulas.
    if x.is_finite() && x.abs() < 1e15 && (x == 0.0 || x.abs() >= 1e-4) {
        format!("{:?}", x)
    } else {
        format!("{:.12}", x)
    }
}

pub mod amortization;
pub mod bond;
pub mod break_even;
pub mod budgeting;
pub mod commission;
pub mod compound;
pub mod currency;
pub mod dcf;
pub mod formatting;
pub mod inventory;
pub mod leaderboard;
pub mod lookup;
pub mod pipeline;
pub mod portfolio;
pub mod quarterly_forecast;
pub mod regression;
pub mod schedule;
pub mod sensitivity;
pub mod tax;

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn abs_cell_anchors_both_parts() {
        assert_eq!(abs_cell("A1"), "$A$1");
        assert_eq!(abs_cell("AA10"), "$AA$10");
        assert_eq!(abs_cell("$A$1"), "$A$1");
        assert_eq!(abs_cell("$A1"), "$A$1");
        assert_eq!(abs_cell("A$1"), "$A$1");
    }

    #[test]
    fn abs_range_anchors_both_endpoints() {
        assert_eq!(abs_range("A2:B6"), "$A$2:$B$6");
        assert_eq!(abs_range("A1"), "$A$1");
        assert_eq!(abs_range("$A$2:$B$6"), "$A$2:$B$6");
        assert_eq!(abs_range("$A2:B$6"), "$A$2:$B$6");
    }
}
