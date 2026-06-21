//! `spacescan` library crate.
//!
//! The binary in `main.rs` is a thin shell around this library: it parses CLI
//! flags, runs a scan, and drives either the text report or the interactive
//! TUI. All of the reusable logic lives in the modules below so it can be unit
//! tested from the external `tests/` directory.

pub mod bench;
pub mod cli;
pub mod constants;
pub mod deletion;
pub mod export;
pub mod format;
pub mod metric;
pub mod node;
pub mod picker;
pub mod reclaim;
pub mod report;
pub mod reveal;
pub mod runner;
pub mod scan;
pub mod stats;
pub mod theme;
pub mod tui;
pub mod util;
