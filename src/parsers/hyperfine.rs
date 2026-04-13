//! Parser for `hyperfine --export-json` output.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::parsers::read_bounded;
use crate::sample::{Benchmark, Run, Unit};

#[derive(Debug, Deserialize)]
struct HyperfineRoot {
    results: Vec<HyperfineResult>,
}

#[derive(Debug, Deserialize)]
struct HyperfineResult {
    command: String,
    #[serde(default)]
    times: Vec<f64>,
    #[serde(default)]
    mean: Option<f64>,
    #[serde(default)]
    stddev: Option<f64>,
}

pub fn parse_file(path: &Path) -> Result<Run> {
    let text = read_bounded(path)?;
    parse_str(&text).map_err(|e| match e {
        Error::Parse { message, .. } => Error::parse("hyperfine", message),
        other => other,
    })
}

pub fn parse_str(text: &str) -> Result<Run> {
    let root: HyperfineRoot = serde_json::from_str(text).map_err(|e| Error::Parse {
        format: "hyperfine".into(),
        message: e.to_string(),
    })?;
    let mut run = Run::new("hyperfine");
    for r in root.results {
        // hyperfine times are in seconds; we record as seconds + Unit::S so
        // downstream ns normalization handles it consistently.
        let samples = if r.times.is_empty() {
            // Fall back to mean (+/- stddev synth) if times are missing.
            match (r.mean, r.stddev) {
                (Some(m), Some(sd)) => vec![m - sd, m, m + sd],
                (Some(m), None) => vec![m, m],
                _ => continue,
            }
        } else {
            r.times
        };
        for v in &samples {
            if !v.is_finite() {
                return Err(Error::NonFinite {
                    name: r.command.clone(),
                });
            }
        }
        run.benchmarks
            .push(Benchmark::new(r.command, samples, Unit::S)?);
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_commands() {
        let t = r#"{
            "results": [
                {"command":"cmd-a","times":[0.10,0.11,0.09,0.10,0.105],"mean":0.101,"stddev":0.005},
                {"command":"cmd-b","times":[0.20,0.21,0.19,0.20,0.205],"mean":0.201,"stddev":0.005}
            ]
        }"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 2);
        assert_eq!(run.benchmarks[0].name, "cmd-a");
        assert_eq!(run.benchmarks[0].samples.len(), 5);
        assert_eq!(run.benchmarks[0].unit, Unit::S);
    }

    #[test]
    fn missing_times_falls_back_to_mean() {
        let t = r#"{
            "results": [
                {"command":"cmd","times":[],"mean":0.5,"stddev":0.1}
            ]
        }"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].samples.len(), 3);
    }

    #[test]
    fn missing_times_and_stddev_uses_mean_twice() {
        let t = r#"{
            "results": [
                {"command":"cmd","times":[],"mean":0.5}
            ]
        }"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].samples.len(), 2);
    }

    #[test]
    fn missing_everything_skipped() {
        let t = r#"{ "results": [ {"command":"cmd","times":[]} ] }"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 0);
    }

    #[test]
    fn empty_results() {
        let t = r#"{ "results": [] }"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 0);
    }

    #[test]
    fn malformed_json_rejected() {
        let err = parse_str("not json").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn non_finite_overflow_handled() {
        // serde_json either rejects 1e400 at parse time (returning Error::Parse)
        // or parses it as infinity (returning Error::NonFinite). Either is fine;
        // the important thing is we don't silently produce a NaN/Inf benchmark.
        let t = r#"{ "results":[{"command":"x","times":[0.1,1e400,0.1]}] }"#;
        let res = parse_str(t);
        match res {
            Err(Error::Parse { .. } | Error::NonFinite { .. }) => {}
            Ok(run) => {
                for b in &run.benchmarks {
                    for s in &b.samples {
                        assert!(s.is_finite(), "leaked non-finite sample");
                    }
                }
            }
            Err(other) => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn unit_is_seconds() {
        let t = r#"{"results":[{"command":"x","times":[0.1,0.2]}]}"#;
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].unit, Unit::S);
    }
}
