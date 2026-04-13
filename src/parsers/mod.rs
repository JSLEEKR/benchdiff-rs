//! Format detection and dispatch.

use std::path::Path;

use crate::error::{Error, Result};
use crate::sample::Run;

pub mod criterion;
pub mod csv;
pub mod gobench;
pub mod hyperfine;

/// Known input formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Criterion,
    GoBench,
    Hyperfine,
    Csv,
}

impl Format {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Criterion => "criterion",
            Self::GoBench => "gobench",
            Self::Hyperfine => "hyperfine",
            Self::Csv => "csv",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "criterion" => Ok(Self::Criterion),
            "gobench" | "go-bench" | "go" => Ok(Self::GoBench),
            "hyperfine" => Ok(Self::Hyperfine),
            "csv" => Ok(Self::Csv),
            _ => Err(Error::parse("format", format!("unknown format: {s}"))),
        }
    }
}

/// Maximum allowed input file size (256 MB).
pub const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum allowed single-line length in text parsers (1 MB).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Guess the format of a path.
///
/// Rules:
/// - directory containing `estimates.json` or `sample.json` subtree → criterion
/// - file ending `.json` with `results` key → hyperfine
/// - file ending `.json` with `mean` + `estimate` → criterion estimates.json
/// - file ending `.csv` → csv
/// - otherwise assume go-bench text
pub fn detect_format(path: &Path) -> Result<Format> {
    if path.is_dir() {
        return Ok(Format::Criterion);
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "csv" {
        return Ok(Format::Csv);
    }
    if ext == "json" {
        // Peek at JSON to distinguish hyperfine vs criterion.
        let text = read_bounded(path)?;
        let lower = text.to_ascii_lowercase();
        if lower.contains("\"results\"") && lower.contains("\"times\"") {
            return Ok(Format::Hyperfine);
        }
        if lower.contains("\"mean\"") || lower.contains("\"estimate\"") {
            return Ok(Format::Criterion);
        }
        return Err(Error::UnknownFormat);
    }
    // Default for text files: go bench.
    Ok(Format::GoBench)
}

/// Parse an input path with an explicit or auto-detected format.
pub fn parse_any(path: &Path, format: Option<Format>) -> Result<Run> {
    let fmt = match format {
        Some(f) => f,
        None => detect_format(path)?,
    };
    match fmt {
        Format::Criterion => criterion::parse(path),
        Format::GoBench => gobench::parse_file(path),
        Format::Hyperfine => hyperfine::parse_file(path),
        Format::Csv => csv::parse_file(path),
    }
}

/// Read a text file, rejecting files larger than [`MAX_INPUT_BYTES`].
pub fn read_bounded(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    if meta.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge {
            bytes: meta.len(),
            limit: MAX_INPUT_BYTES,
        });
    }
    std::fs::read_to_string(path).map_err(|e| Error::io(path, e))
}

/// Split text into lines, trimming a trailing `\r` so Windows CRLF files parse.
#[must_use]
pub fn lines_crlf(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn format_as_str() {
        assert_eq!(Format::Criterion.as_str(), "criterion");
        assert_eq!(Format::GoBench.as_str(), "gobench");
        assert_eq!(Format::Hyperfine.as_str(), "hyperfine");
        assert_eq!(Format::Csv.as_str(), "csv");
    }

    #[test]
    fn format_parse_accepts_variants() {
        assert_eq!(Format::parse("criterion").unwrap(), Format::Criterion);
        assert_eq!(Format::parse("go-bench").unwrap(), Format::GoBench);
        assert_eq!(Format::parse("GoBench").unwrap(), Format::GoBench);
        assert_eq!(Format::parse("go").unwrap(), Format::GoBench);
        assert_eq!(Format::parse("hyperfine").unwrap(), Format::Hyperfine);
        assert_eq!(Format::parse("csv").unwrap(), Format::Csv);
    }

    #[test]
    fn format_parse_rejects_unknown() {
        assert!(Format::parse("magic").is_err());
    }

    #[test]
    fn detect_csv_by_ext() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.csv");
        std::fs::write(&p, "name,value,unit\n").unwrap();
        assert_eq!(detect_format(&p).unwrap(), Format::Csv);
    }

    #[test]
    fn detect_hyperfine_by_content() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench.json");
        std::fs::write(
            &p,
            r#"{"results":[{"command":"x","times":[0.1],"mean":0.1}]}"#,
        )
        .unwrap();
        assert_eq!(detect_format(&p).unwrap(), Format::Hyperfine);
    }

    #[test]
    fn detect_criterion_by_content() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("estimates.json");
        std::fs::write(&p, r#"{"mean":{"estimate":1.0}}"#).unwrap();
        assert_eq!(detect_format(&p).unwrap(), Format::Criterion);
    }

    #[test]
    fn detect_gobench_default_for_txt() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench.txt");
        std::fs::write(&p, "BenchmarkFoo-8 1 1 ns/op\n").unwrap();
        assert_eq!(detect_format(&p).unwrap(), Format::GoBench);
    }

    #[test]
    fn detect_directory_is_criterion() {
        let dir = tempdir().unwrap();
        assert_eq!(detect_format(dir.path()).unwrap(), Format::Criterion);
    }

    #[test]
    fn lines_crlf_handles_both_endings() {
        let lines = lines_crlf("a\r\nb\nc\r\n");
        assert_eq!(lines, vec!["a", "b", "c", ""]);
    }

    #[test]
    fn read_bounded_rejects_missing() {
        assert!(read_bounded(Path::new("/definitely/not/real.json")).is_err());
    }
}
