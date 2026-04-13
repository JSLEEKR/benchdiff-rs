//! Generic CSV parser. Columns: `name,value,unit`.
//!
//! Multiple rows with the same name aggregate into a single benchmark. The
//! unit must be consistent within a benchmark.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::parsers::{lines_crlf, read_bounded, MAX_LINE_BYTES};
use crate::sample::{Benchmark, Run, Unit};

pub fn parse_file(path: &Path) -> Result<Run> {
    let text = read_bounded(path)?;
    parse_str(&text)
}

pub fn parse_str(text: &str) -> Result<Run> {
    let mut agg: BTreeMap<String, (Vec<f64>, Unit)> = BTreeMap::new();

    let lines = lines_crlf(text);
    let mut iter = lines.iter().peekable();

    // Optional header: `name,value,unit`.
    if let Some(first) = iter.peek() {
        let trimmed = first.trim().to_ascii_lowercase();
        if trimmed.starts_with("name") && trimmed.contains("value") && trimmed.contains("unit") {
            iter.next();
        }
    }

    let mut lineno = 1usize;
    for raw in iter {
        lineno += 1;
        if raw.len() > MAX_LINE_BYTES {
            return Err(Error::LineTooLong {
                format: "csv".into(),
                bytes: raw.len(),
            });
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.splitn(3, ',').collect();
        if fields.len() != 3 {
            return Err(Error::parse(
                "csv",
                format!("line {lineno}: expected 3 columns"),
            ));
        }
        let name = fields[0].trim().to_string();
        if name.is_empty() {
            return Err(Error::parse("csv", format!("line {lineno}: empty name")));
        }
        let value: f64 = fields[1].trim().parse().map_err(|_| Error::InvalidNumber {
            field: "value".into(),
            value: fields[1].to_string(),
        })?;
        if !value.is_finite() {
            return Err(Error::NonFinite { name: name.clone() });
        }
        let unit = Unit::parse(fields[2])?;

        let entry = agg.entry(name).or_insert_with(|| (Vec::new(), unit));
        if entry.1 != unit {
            return Err(Error::parse(
                "csv",
                format!("line {lineno}: unit changed within a benchmark"),
            ));
        }
        entry.0.push(value);
    }

    let mut run = Run::new("csv");
    for (name, (samples, unit)) in agg {
        run.benchmarks.push(Benchmark::new(name, samples, unit)?);
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_rows() {
        let t = "name,value,unit\nfoo,100,ns\nfoo,101,ns\nbar,5,ms\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 2);
    }

    #[test]
    fn no_header_required() {
        let t = "foo,100,ns\nfoo,101,ns\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        assert_eq!(run.benchmarks[0].samples.len(), 2);
    }

    #[test]
    fn unit_mismatch_rejected() {
        let t = "foo,100,ns\nfoo,0.1,ms\n";
        assert!(parse_str(t).is_err());
    }

    #[test]
    fn bad_number_rejected() {
        let t = "foo,abc,ns\n";
        assert!(parse_str(t).is_err());
    }

    #[test]
    fn bad_unit_rejected() {
        let t = "foo,100,furlongs\n";
        assert!(parse_str(t).is_err());
    }

    #[test]
    fn missing_columns_rejected() {
        let t = "foo,100\n";
        assert!(parse_str(t).is_err());
    }

    #[test]
    fn empty_name_rejected() {
        let t = ",100,ns\n";
        assert!(parse_str(t).is_err());
    }

    #[test]
    fn comments_and_blanks_skipped() {
        let t = "# comment\nfoo,100,ns\n\nfoo,101,ns\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].samples.len(), 2);
    }

    #[test]
    fn handles_crlf() {
        let t = "foo,100,ns\r\nfoo,101,ns\r\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].samples.len(), 2);
    }

    #[test]
    fn nonfinite_rejected() {
        // "inf" parses as f64::INFINITY in Rust's FromStr for f64.
        let t = "foo,inf,ns\n";
        let err = parse_str(t).unwrap_err();
        assert!(matches!(err, Error::NonFinite { .. }));
    }
}
