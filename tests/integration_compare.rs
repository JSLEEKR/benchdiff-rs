//! End-to-end: save → compare flow via the library API.

use benchdiff::baseline::Baseline;
use benchdiff::compare::{compare_runs, Verdict};
use benchdiff::config::Config;
use benchdiff::sample::{Benchmark, Run, Unit};
use tempfile::tempdir;

fn make_run(benches: &[(&str, &[f64])]) -> Run {
    let mut r = Run::new("test");
    for (n, s) in benches {
        r.benchmarks
            .push(Benchmark::new(*n, s.to_vec(), Unit::Ns).unwrap());
    }
    r
}

#[test]
fn save_and_compare_round_trip() {
    let dir = tempdir().unwrap();
    let baseline_run = make_run(&[
        (
            "parse",
            &[100.0, 101.0, 99.0, 100.5, 99.5, 100.1, 99.9, 100.2],
        ),
        (
            "render",
            &[500.0, 501.0, 499.0, 500.5, 499.5, 500.1, 499.9, 500.2],
        ),
    ]);
    Baseline::new("v1", baseline_run.clone())
        .save(dir.path())
        .unwrap();

    let current = make_run(&[
        (
            "parse",
            &[100.0, 101.0, 99.0, 100.5, 99.5, 100.1, 99.9, 100.2],
        ),
        (
            "render",
            &[600.0, 601.0, 599.0, 600.5, 599.5, 600.1, 599.9, 600.2],
        ),
    ]);

    let loaded = Baseline::load(dir.path(), "v1").unwrap();
    let cfg = Config::default();
    let report = compare_runs(&loaded.run, &current, "v1", &cfg).unwrap();

    let render_row = report.rows.iter().find(|r| r.name == "render").unwrap();
    assert_eq!(render_row.verdict, Verdict::Regression);
    assert!(render_row.rel_change > 0.15);

    let parse_row = report.rows.iter().find(|r| r.name == "parse").unwrap();
    assert_eq!(parse_row.verdict, Verdict::Unchanged);
}

#[test]
fn config_alpha_override_changes_verdict() {
    // Change is real but α very strict → no rejection.
    let a = make_run(&[(
        "foo",
        &[100.0, 100.5, 99.5, 100.2, 99.8, 100.1, 99.9, 100.0],
    )]);
    let b = make_run(&[(
        "foo",
        &[108.0, 108.5, 107.5, 108.2, 107.8, 108.1, 107.9, 108.0],
    )]);

    let mut cfg = Config::default();
    cfg.alpha = 1e-20;
    let report = compare_runs(&a, &b, "v1", &cfg).unwrap();
    assert_eq!(report.rows[0].verdict, Verdict::Unchanged);
}

#[test]
fn listing_empty_dir_returns_empty() {
    let dir = tempdir().unwrap();
    let ls = Baseline::list(dir.path()).unwrap();
    assert!(ls.is_empty());
}
