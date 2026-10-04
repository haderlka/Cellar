//! Infrastructure layer providing external service integrations.
//!
//! This module contains implementations for external concerns like
//! file I/O, persistence, and other system-level operations.

pub mod atomic;
pub mod canonical_json;
pub mod chart_image;
pub mod persistence;
pub mod fetcher;
pub mod recent;
pub mod autosave;
pub mod sidecar;
pub mod xlsx;
pub mod xlsx_convert;
pub mod xlsx_extras;
pub mod xlsx_pivot;

pub use persistence::*;
