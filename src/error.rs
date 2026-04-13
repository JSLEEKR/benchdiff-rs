//! Error types for benchdiff.

use std::path::PathBuf;
use thiserror::Error;

/// All fallible operations in benchdiff return this error type.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error for {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON parse error in {path:?}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("TOML parse error in {path:?}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("parse error ({format}): {message}")]
    Parse { format: String, message: String },

    #[error("unknown benchmark format — tried: criterion, gobench, hyperfine, csv")]
    UnknownFormat,

    #[error("no benchmarks found in input")]
    EmptyInput,

    #[error("invalid baseline label {label:?}: {reason}")]
    InvalidLabel { label: String, reason: String },

    #[error("baseline {label:?} not found in {dir:?}")]
    BaselineNotFound { label: String, dir: PathBuf },

    #[error("insufficient samples for {name:?}: need >= {need}, got {got}")]
    InsufficientSamples {
        name: String,
        need: usize,
        got: usize,
    },

    #[error("invalid numeric value for {field}: {value}")]
    InvalidNumber { field: String, value: String },

    #[error("input file too large: {bytes} bytes (limit {limit})")]
    InputTooLarge { bytes: u64, limit: u64 },

    #[error("line too long in {format}: {bytes} bytes")]
    LineTooLong { format: String, bytes: usize },

    #[error("NaN or infinity in benchmark value ({name:?})")]
    NonFinite { name: String },

    #[error("config error: {0}")]
    Config(String),
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }

    pub fn parse(format: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Parse {
            format: format.into(),
            message: message.into(),
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
