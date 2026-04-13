//! Parser for `go test -bench=.` stdout.
//!
//! A bench line looks like:
//!
//! ```text
//! BenchmarkFoo-8           1000000       123.4 ns/op     456 B/op    7 allocs/op
//! ```
//!
//! The first number is iterations (`b.N`), which we ignore. The second is the
//! per-iteration time. `-count=N` invocations produce N lines per benchmark;
//! we aggregate those as samples.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::parsers::{lines_crlf, read_bounded, MAX_LINE_BYTES};
use crate::sample::{Benchmark, Run, Unit};

/// Parse a file.
pub fn parse_file(path: &Path) -> Result<Run> {
    let text = read_bounded(path)?;
    parse_str(&text)
}

/// Parse from an in-memory string.
pub fn parse_str(text: &str) -> Result<Run> {
    let mut agg: BTreeMap<String, (Vec<f64>, Unit)> = BTreeMap::new();

    for line in lines_crlf(text) {
        if line.len() > MAX_LINE_BYTES {
            return Err(Error::LineTooLong {
                format: "gobench".into(),
                bytes: line.len(),
            });
        }
        let trimmed = line.trim();
        if !trimmed.starts_with("Benchmark") {
            continue;
        }
        if let Some((name, value, unit)) = parse_line(trimmed)? {
            if !value.is_finite() {
                return Err(Error::NonFinite { name });
            }
            let entry = agg.entry(name.clone()).or_insert_with(|| (Vec::new(), unit));
            // If unit changes mid-run, reject (shouldn't happen in real output).
            if entry.1 != unit {
                return Err(Error::parse(
                    "gobench",
                    format!("unit changed mid-run for benchmark {name:?}"),
                ));
            }
            entry.0.push(value);
        }
    }

    let mut run = Run::new("go-bench");
    for (name, (samples, unit)) in agg {
        if samples.is_empty() {
            continue;
        }
        run.benchmarks.push(Benchmark::new(name, samples, unit)?);
    }
    Ok(run)
}

/// Parse one bench line. Returns `Ok(None)` for lines that don't match.
#[allow(clippy::similar_names)]
fn parse_line(line: &str) -> Result<Option<(String, f64, Unit)>> {
    let mut it = line.split_ascii_whitespace();
    let name = match it.next() {
        Some(n) if n.starts_with("Benchmark") => n.to_string(),
        _ => return Ok(None),
    };
    // Strip trailing `-N` (GOMAXPROCS marker).
    let name = strip_gomaxprocs(&name);

    // Next field is iteration count — must parse as integer.
    let iters = match it.next() {
        Some(s) => s,
        None => return Ok(None),
    };
    if iters.parse::<u64>().is_err() {
        return Ok(None);
    }

    // Scan remaining tokens for a "<number> <unit>/op" pair where unit is ns/us/ms.
    // Collect into Vec first so we can index.
    let rest: Vec<&str> = it.collect();
    let mut i = 0;
    while i + 1 < rest.len() {
        let val = rest[i];
        let unit_tok = rest[i + 1];
        if let Ok(v) = val.parse::<f64>() {
            if let Some(unit) = match_time_unit(unit_tok) {
                return Ok(Some((name, v, unit)));
            }
        }
        i += 1;
    }
    Ok(None)
}

fn strip_gomaxprocs(name: &str) -> String {
    // `BenchmarkFoo-8` → `BenchmarkFoo`. Only strip when trailing `-<digits>`.
    if let Some(idx) = name.rfind('-') {
        let tail = &name[idx + 1..];
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            return name[..idx].to_string();
        }
    }
    name.to_string()
}

fn match_time_unit(tok: &str) -> Option<Unit> {
    match tok {
        "ns/op" | "ns" => Some(Unit::Ns),
        "us/op" | "μs/op" | "us" => Some(Unit::Us),
        "ms/op" | "ms" => Some(Unit::Ms),
        "s/op" => Some(Unit::S),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_bench_line() {
        let t = "BenchmarkFoo-8   1000000   123.4 ns/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        let b = &run.benchmarks[0];
        assert_eq!(b.name, "BenchmarkFoo");
        assert_eq!(b.unit, Unit::Ns);
        assert!((b.samples[0] - 123.4).abs() < 1e-9);
    }

    #[test]
    fn aggregates_count_repeats() {
        let t = "\
BenchmarkFoo-8 1000 100 ns/op
BenchmarkFoo-8 1000 101 ns/op
BenchmarkFoo-8 1000 99 ns/op
";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        assert_eq!(run.benchmarks[0].samples.len(), 3);
    }

    #[test]
    fn parses_multiple_benchmarks() {
        let t = "\
BenchmarkFoo-8 1000 100 ns/op
BenchmarkBar-8 1000 200 ns/op
";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 2);
    }

    #[test]
    fn preserves_subtest_names() {
        let t = "BenchmarkFoo/case1-8 1000 100 ns/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].name, "BenchmarkFoo/case1");
    }

    #[test]
    fn ignores_non_bench_lines() {
        let t = "\
goos: linux
goarch: amd64
pkg: example.com/x
BenchmarkFoo-8 1000 100 ns/op
PASS
ok      example.com/x    1.234s
";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
    }

    #[test]
    fn handles_us_unit() {
        let t = "BenchmarkFoo-8 1000 1.5 us/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].unit, Unit::Us);
    }

    #[test]
    fn handles_ms_unit() {
        let t = "BenchmarkFoo-8 1000 1.5 ms/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].unit, Unit::Ms);
    }

    #[test]
    fn handles_s_unit() {
        let t = "BenchmarkFoo-8 1000 1.5 s/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks[0].unit, Unit::S);
    }

    #[test]
    fn handles_crlf_line_endings() {
        let t = "BenchmarkFoo-8 1000 100 ns/op\r\nBenchmarkBar-8 1000 200 ns/op\r\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 2);
    }

    #[test]
    fn strip_gomaxprocs_works() {
        assert_eq!(strip_gomaxprocs("BenchmarkFoo-8"), "BenchmarkFoo");
        assert_eq!(strip_gomaxprocs("BenchmarkFoo-16"), "BenchmarkFoo");
        assert_eq!(
            strip_gomaxprocs("BenchmarkFoo/case-1-8"),
            "BenchmarkFoo/case-1"
        );
        assert_eq!(strip_gomaxprocs("BenchmarkFoo"), "BenchmarkFoo");
    }

    #[test]
    fn ignores_unparseable_trailing_fields() {
        let t = "BenchmarkFoo-8 1000 100 ns/op 32 B/op 1 allocs/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 1);
        assert!((run.benchmarks[0].samples[0] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn empty_input_yields_empty_run() {
        let run = parse_str("").unwrap();
        assert_eq!(run.benchmarks.len(), 0);
    }

    #[test]
    fn bad_iters_line_ignored() {
        // Line doesn't have iteration count parseable as u64.
        let t = "BenchmarkFoo-8 xx 100 ns/op\n";
        let run = parse_str(t).unwrap();
        assert_eq!(run.benchmarks.len(), 0);
    }

    #[test]
    fn nonfinite_error_reports_real_name() {
        // eval-A regression test: the pre-fix code reported the length of
        // the aggregated samples vec (as a string!) instead of the benchmark
        // name when NaN/inf was encountered.
        let t = "BenchmarkFoo-8 1000 NaN ns/op\n";
        let err = parse_str(t).unwrap_err();
        match err {
            Error::NonFinite { name } => {
                assert_eq!(name, "BenchmarkFoo");
            }
            other => panic!("expected NonFinite, got {other:?}"),
        }
    }

    #[test]
    fn nonfinite_inf_reports_real_name() {
        let t = "BenchmarkBar-8 1000 inf ns/op\n";
        let err = parse_str(t).unwrap_err();
        match err {
            Error::NonFinite { name } => {
                assert_eq!(name, "BenchmarkBar");
            }
            other => panic!("expected NonFinite, got {other:?}"),
        }
    }
}
