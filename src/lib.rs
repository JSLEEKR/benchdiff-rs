//! benchdiff — statistical benchmark regression detector.
//!
//! Parses benchmark output from Criterion, `go test -bench`, hyperfine,
//! and generic CSV; compares to a saved baseline using Welch's two-sample
//! t-test; emits text / JSON / Markdown reports; gates CI on significant
//! slowdowns.

#![forbid(unsafe_code)]
#![deny(warnings)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod baseline;
pub mod cli;
pub mod compare;
pub mod config;
pub mod error;
pub mod parsers;
pub mod report;
pub mod sample;
pub mod stats;

pub use baseline::Baseline;
pub use compare::{compare_runs, DiffReport, DiffRow, Verdict};
pub use config::Config;
pub use error::Error;
pub use parsers::{detect_format, parse_any, Format};
pub use sample::{Benchmark, Run, Unit};

/// Library version, taken from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
