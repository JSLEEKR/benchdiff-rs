# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-04-13

### Added
- Multi-format benchmark parsers: Criterion (`sample.json` /
  `estimates.json` directory walk), Go bench stdout, hyperfine
  `--export-json`, and a generic `name,value,unit` CSV.
- Welch's two-sample t-test with Welch-Satterthwaite degrees of freedom,
  implemented on top of a regularized incomplete beta function (Lentz's
  continued fraction) — no dependency on `statrs`.
- Cohen's d effect size reporting per benchmark.
- Benjamini-Hochberg FDR correction (`--correction bh`).
- Per-benchmark tolerance overrides and glob-based ignore patterns via
  `benchdiff.toml`.
- CLI commands: `save`, `compare`, `list`, `init`, `summary`, `inspect`.
- Three report formats: rich terminal table (comfy-table), GitHub-flavored
  Markdown with emoji verdicts, and machine-readable JSON.
- CI gating via clean exit codes and `--allow-regressions N`.
- Cross-platform line-ending handling (LF + CRLF) in all text parsers.
- Input safety: 256 MB file cap, 1 MB line cap, `NaN`/`Infinity` rejection
  at parse time, baseline label validation against path traversal.
- `#![forbid(unsafe_code)]` enforced crate-wide.

[1.0.0]: https://github.com/JSLEEKR/benchdiff-rs/releases/tag/v1.0.0
