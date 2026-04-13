//! Integration tests for the hyperfine parser.

use benchdiff::parsers::hyperfine;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/hyperfine_two_cmds.json");
    p
}

#[test]
fn parses_real_hyperfine_json() {
    let run = hyperfine::parse_file(&fixture()).unwrap();
    assert_eq!(run.benchmarks.len(), 2);
}

#[test]
fn both_commands_have_ten_samples() {
    let run = hyperfine::parse_file(&fixture()).unwrap();
    for b in &run.benchmarks {
        assert_eq!(b.samples.len(), 10);
    }
}

#[test]
fn commands_are_named_verbatim() {
    let run = hyperfine::parse_file(&fixture()).unwrap();
    let names: Vec<&str> = run.benchmarks.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"grep pattern input.txt"));
    assert!(names.contains(&"rg pattern input.txt"));
}
