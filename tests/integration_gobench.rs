//! Integration tests for the Go bench parser using a real captured run.

use benchdiff::parsers::gobench;

const FIXTURE: &str = include_str!("fixtures/gobench_basic.txt");

#[test]
fn parses_real_gobench_output() {
    let run = gobench::parse_str(FIXTURE).unwrap();
    assert!(!run.benchmarks.is_empty());
    assert!(run.benchmarks.iter().all(|b| !b.samples.is_empty()));
}

#[test]
fn real_fixture_has_expected_names() {
    let run = gobench::parse_str(FIXTURE).unwrap();
    let names: Vec<&str> = run.benchmarks.iter().map(|b| b.name.as_str()).collect();
    assert!(names.iter().any(|n| n.starts_with("Benchmark")));
}

#[test]
fn aggregated_count_produces_multiple_samples() {
    let run = gobench::parse_str(FIXTURE).unwrap();
    // At least one benchmark should have more than one sample (since fixture
    // uses -count=3).
    assert!(run.benchmarks.iter().any(|b| b.samples.len() > 1));
}
