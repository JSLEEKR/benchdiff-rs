//! `benchdiff` binary entry point.
#![forbid(unsafe_code)]
#![deny(warnings)]

use clap::Parser;

use benchdiff::cli::{run, Cli};
use benchdiff::error::Error as BdError;

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            // Use `{err}` (not `{err:#}`) on purpose: every `benchdiff::Error`
            // variant whose `#[from]` source is interesting (`Io`, `Json`,
            // `Toml`) already embeds `{source}` in its `Display`. anyhow's
            // alternate formatter walks `#[source]` and re-prints it, which
            // would emit the same message twice (with a stray `: ` joiner).
            // The non-alternate form prints `Display` once, which is exactly
            // the message we want.
            eprintln!("benchdiff error: {err}");
            std::process::exit(classify_exit_code(&err));
        }
    }
}

/// Map an error into one of benchdiff's documented exit codes.
///
/// Uses the structured `benchdiff::Error` enum via `anyhow` downcasting
/// rather than matching substrings of the `Display` output. The old
/// substring-based approach was locale-sensitive (Windows emits translated
/// I/O error messages) and silently misclassified anything it didn't
/// recognize.
fn classify_exit_code(err: &anyhow::Error) -> i32 {
    // Walk the error chain looking for a benchdiff::Error.
    if let Some(bd) = err.downcast_ref::<BdError>() {
        return match bd {
            BdError::Parse { .. }
            | BdError::UnknownFormat
            | BdError::Json { .. }
            | BdError::Toml { .. }
            | BdError::InvalidNumber { .. } => 3,
            BdError::Io { .. }
            | BdError::BaselineNotFound { .. }
            | BdError::InputTooLarge { .. }
            | BdError::LineTooLong { .. } => 4,
            BdError::InsufficientSamples { .. }
            | BdError::EmptyInput
            | BdError::NonFinite { .. } => 5,
            BdError::InvalidLabel { .. } | BdError::Config(_) => 2,
        };
    }
    // Fallback: plain I/O errors surfaced without our wrapper still get 4.
    if err.downcast_ref::<std::io::Error>().is_some() {
        return 4;
    }
    2
}
