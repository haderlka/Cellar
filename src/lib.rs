//! Cellar — the spreadsheet engine behind the Cellar desktop app.
//!
//! Layers (clean architecture, inherited from tshts):
//! - [`domain`]: cell/sheet/workbook models, the formula parser and
//!   evaluator, the dependency-graph recalc engine, pivot tables, exports.
//! - [`application`]: the editing session ([`App`]): undo/redo, clipboard,
//!   fill, formatting, sheet commands.
//! - [`infrastructure`]: file I/O — `.cellar` files, Excel import/export,
//!   CSV, chart images, HTTP for `GET()`.
//!
//! The GUI lives in `src/gui/` (binary `cellar`).

pub mod domain;
pub mod application;
pub mod infrastructure;

pub use domain::*;
pub use application::*;
