//! Parser for Criterion.rs output.
//!
//! Criterion writes per-benchmark directories under `target/criterion/`:
//!
//! ```text
//! target/criterion/<group>/<name>/new/sample.json    -- raw samples
//!                                     estimates.json -- summary only
//! ```
//!
//! We prefer `sample.json` (which contains `iters` and `times` arrays). If
//! only `estimates.json` is available we synthesize samples from the mean.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::parsers::read_bounded;
use crate::sample::{Benchmark, Run, Unit};

#[derive(Debug, Deserialize)]
struct SampleJson {
    #[serde(default)]
    iters: Vec<f64>,
    #[serde(default)]
    times: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct EstimatesJson {
    mean: Option<EstimatePoint>,
    #[serde(default)]
    std_dev: Option<EstimatePoint>,
}

#[derive(Debug, Deserialize)]
struct EstimatePoint {
    #[serde(alias = "point_estimate")]
    estimate: Option<f64>,
    #[serde(default)]
    lower_bound: Option<f64>,
    #[serde(default)]
    upper_bound: Option<f64>,
}

impl EstimatePoint {
    fn value(&self) -> Option<f64> {
        self.estimate.or(self.lower_bound).or(self.upper_bound)
    }
}

/// Parse either a directory (walked recursively) or a single `estimates.json`
/// or `sample.json` file.
pub fn parse(path: &Path) -> Result<Run> {
    if path.is_dir() {
        parse_dir(path)
    } else {
        let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let name = stem_from_path(path);
        match fname {
            "sample.json" => {
                let b = parse_sample_file(path, &name)?;
                Ok(one(b))
            }
            "estimates.json" => {
                let b = parse_estimates_file(path, &name)?;
                Ok(one(b))
            }
            _ => Err(Error::parse(
                "criterion",
                format!("unsupported criterion file: {fname}"),
            )),
        }
    }
}

fn one(b: Benchmark) -> Run {
    let mut r = Run::new("criterion");
    r.benchmarks.push(b);
    r
}

fn parse_dir(dir: &Path) -> Result<Run> {
    let mut run = Run::new("criterion");
    walk(dir, &mut run)?;
    Ok(run)
}

fn walk(dir: &Path, run: &mut Run) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for e in entries {
        let e = e.map_err(|err| Error::io(dir, err))?;
        let p = e.path();
        if p.is_dir() {
            // Skip `report/` directories which don't have samples.
            if p.file_name().and_then(|s| s.to_str()) == Some("report") {
                continue;
            }
            // Prefer `new/sample.json`, then `new/estimates.json`.
            let new_dir = p.join("new");
            if new_dir.is_dir() {
                let sample = new_dir.join("sample.json");
                let est = new_dir.join("estimates.json");
                let name = bench_name_from_path(&p);
                if sample.is_file() {
                    run.benchmarks.push(parse_sample_file(&sample, &name)?);
                    continue;
                }
                if est.is_file() {
                    run.benchmarks.push(parse_estimates_file(&est, &name)?);
                    continue;
                }
            }
            walk(&p, run)?;
        }
    }
    Ok(())
}

fn bench_name_from_path(dir: &Path) -> String {
    // Use parent-chain path components relative to criterion root heuristically.
    dir.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unnamed")
        .to_string()
}

fn stem_from_path(path: &Path) -> String {
    path.parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("criterion")
        .to_string()
}

fn parse_sample_file(path: &Path, name: &str) -> Result<Benchmark> {
    let text = read_bounded(path)?;
    let s: SampleJson = serde_json::from_str(&text).map_err(|e| Error::json(path, e))?;
    if s.times.is_empty() {
        return Err(Error::parse(
            "criterion",
            format!("sample.json has no times: {}", path.display()),
        ));
    }
    // Normalize by iteration count if present (same length as times).
    let samples: Vec<f64> = if s.iters.len() == s.times.len() {
        s.times
            .iter()
            .zip(s.iters.iter())
            .map(|(t, i)| if *i > 0.0 { t / i } else { *t })
            .collect()
    } else {
        s.times.clone()
    };
    for v in &samples {
        if !v.is_finite() {
            return Err(Error::NonFinite {
                name: name.to_string(),
            });
        }
    }
    Benchmark::new(name, samples, Unit::Ns)
}

fn parse_estimates_file(path: &Path, name: &str) -> Result<Benchmark> {
    let text = read_bounded(path)?;
    let e: EstimatesJson = serde_json::from_str(&text).map_err(|err| Error::json(path, err))?;
    let mean = e
        .mean
        .as_ref()
        .and_then(EstimatePoint::value)
        .ok_or_else(|| {
            Error::parse("criterion", format!("no mean in {}", path.display()))
        })?;
    let sd = e
        .std_dev
        .as_ref()
        .and_then(EstimatePoint::value)
        .unwrap_or(0.0);
    // Synthesize 3 samples: mean-sd, mean, mean+sd. Mark in benchmark name.
    let samples = vec![mean - sd, mean, mean + sd];
    for v in &samples {
        if !v.is_finite() {
            return Err(Error::NonFinite {
                name: name.to_string(),
            });
        }
    }
    Benchmark::new(name, samples, Unit::Ns)
}

/// Expose a directory walk helper for callers that want to merge runs.
pub fn walk_for_tests(dir: &Path) -> Result<Run> {
    parse_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn mk(path: PathBuf, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn sample_file_parses() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/new/sample.json");
        mk(
            p.clone(),
            r#"{"iters":[1,1,1],"times":[100.0,101.0,99.0]}"#,
        );
        let run = parse(&p).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        assert_eq!(run.benchmarks[0].samples.len(), 3);
    }

    #[test]
    fn sample_file_divides_by_iters() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/new/sample.json");
        mk(
            p.clone(),
            r#"{"iters":[10,10,10],"times":[1000.0,1010.0,990.0]}"#,
        );
        let run = parse(&p).unwrap();
        assert!((run.benchmarks[0].samples[0] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn estimates_file_synthesizes() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/new/estimates.json");
        mk(
            p.clone(),
            r#"{
                "mean":{"point_estimate":100.0,"lower_bound":95.0,"upper_bound":105.0},
                "median":{"point_estimate":100.0},
                "std_dev":{"point_estimate":2.5}
            }"#,
        );
        let run = parse(&p).unwrap();
        assert_eq!(run.benchmarks[0].samples.len(), 3);
        assert!((run.benchmarks[0].samples[1] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn dir_walk_collects_benchmarks() {
        let dir = tempdir().unwrap();
        mk(
            dir.path().join("bench1/new/sample.json"),
            r#"{"iters":[1],"times":[100.0]}"#,
        );
        mk(
            dir.path().join("bench2/new/sample.json"),
            r#"{"iters":[1],"times":[200.0]}"#,
        );
        // Should be ignored:
        mk(dir.path().join("report/index.html"), "<html/>");
        let run = parse(dir.path()).unwrap();
        assert_eq!(run.benchmarks.len(), 2);
    }

    #[test]
    fn dir_walk_prefers_sample_over_estimates() {
        let dir = tempdir().unwrap();
        mk(
            dir.path().join("bench/new/sample.json"),
            r#"{"iters":[1,1],"times":[500.0,501.0]}"#,
        );
        mk(
            dir.path().join("bench/new/estimates.json"),
            r#"{"mean":{"point_estimate":99999.0}}"#,
        );
        let run = parse(dir.path()).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        assert!((run.benchmarks[0].samples[0] - 500.0).abs() < 1e-9);
    }

    #[test]
    fn estimates_only_fallback() {
        let dir = tempdir().unwrap();
        mk(
            dir.path().join("bench/new/estimates.json"),
            r#"{"mean":{"point_estimate":42.0}}"#,
        );
        let run = parse(dir.path()).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
    }

    #[test]
    fn malformed_sample_rejected() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/new/sample.json");
        mk(p.clone(), "not json");
        assert!(parse(&p).is_err());
    }

    #[test]
    fn empty_times_rejected() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/new/sample.json");
        mk(p.clone(), r#"{"iters":[],"times":[]}"#);
        assert!(parse(&p).is_err());
    }

    #[test]
    fn unknown_file_rejected() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bench/random.json");
        mk(p.clone(), "{}");
        assert!(parse(&p).is_err());
    }
}
