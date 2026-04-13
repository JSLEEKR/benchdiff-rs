//! Integration tests for the CSV parser + end-to-end compare flow.

use benchdiff::compare::compare_runs;
use benchdiff::config::Config;
use benchdiff::parsers::csv;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/csv_smoke.csv");
    p
}

#[test]
fn parses_real_csv() {
    let run = csv::parse_file(&fixture()).unwrap();
    assert_eq!(run.benchmarks.len(), 2);
    for b in &run.benchmarks {
        assert_eq!(b.samples.len(), 5);
    }
}

#[test]
fn identical_runs_are_unchanged() {
    let run = csv::parse_file(&fixture()).unwrap();
    let cfg = Config::default();
    let rep = compare_runs(&run, &run, "baseline", &cfg).unwrap();
    assert_eq!(rep.regressions(), 0);
    assert_eq!(rep.improvements(), 0);
}
