//! Integration tests for the Criterion parser. Builds a fixture on the fly
//! so we don't need real Criterion output vendored in.

use benchdiff::parsers::criterion;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn mk(path: &Path, content: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn walks_synthetic_criterion_tree() {
    let dir = tempdir().unwrap();
    mk(
        &dir.path().join("parse/new/sample.json"),
        r#"{"iters":[1,1,1,1,1],"times":[100.0,101.0,99.0,100.5,99.5]}"#,
    );
    mk(
        &dir.path().join("render/new/sample.json"),
        r#"{"iters":[1,1,1,1,1],"times":[500.0,501.0,499.0,500.5,499.5]}"#,
    );
    mk(&dir.path().join("report/index.html"), "<html/>");
    let run = criterion::parse(dir.path()).unwrap();
    assert_eq!(run.benchmarks.len(), 2);
    let names: Vec<&str> = run.benchmarks.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"parse"));
    assert!(names.contains(&"render"));
}

#[test]
fn prefers_sample_over_estimates() {
    let dir = tempdir().unwrap();
    mk(
        &dir.path().join("b/new/sample.json"),
        r#"{"iters":[1,1],"times":[500.0,501.0]}"#,
    );
    mk(
        &dir.path().join("b/new/estimates.json"),
        r#"{"mean":{"point_estimate":99999.0}}"#,
    );
    let run = criterion::parse(dir.path()).unwrap();
    assert_eq!(run.benchmarks.len(), 1);
    assert!((run.benchmarks[0].samples[0] - 500.0).abs() < 1e-9);
}

#[test]
fn falls_back_to_estimates_when_sample_missing() {
    let dir = tempdir().unwrap();
    mk(
        &dir.path().join("b/new/estimates.json"),
        r#"{"mean":{"point_estimate":42.0},"std_dev":{"point_estimate":2.0}}"#,
    );
    let run = criterion::parse(dir.path()).unwrap();
    assert_eq!(run.benchmarks.len(), 1);
    assert!(run.benchmarks[0].samples.iter().any(|v| (v - 42.0).abs() < 1e-9));
}
