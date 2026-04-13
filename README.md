# benchdiff-rs

[![license](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](LICENSE)
[![rust](https://img.shields.io/badge/rust-2021-orange.svg?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![version](https://img.shields.io/badge/version-1.0.0-green.svg?style=for-the-badge)](CHANGELOG.md)
[![status](https://img.shields.io/badge/status-stable-brightgreen.svg?style=for-the-badge)](#)
[![ci-ready](https://img.shields.io/badge/ci-ready-success.svg?style=for-the-badge&logo=githubactions)](#ci-integration)

> **Statistical benchmark regression detector for Criterion, Go bench, and hyperfine.**
> Parses benchmark output, runs Welch's two-sample t-test against a saved
> baseline, gates CI on significant slowdowns, and emits rich markdown reports.
> Single Rust binary. No runtime dependencies.

---

## Why This Exists

Benchmark tools are great at producing numbers. They are bad at telling you
whether the number is different enough to care.

Teams typically do one of these things in CI:

1. **Eyeball the percentages.** "12% slower, probably noise." Sometimes that
   "noise" is a real 12% loss that compounds across releases.
2. **Gate on a fixed threshold.** Any change > 5% fails CI. This catches
   big regressions but is horribly flaky when the noise floor itself is 8%.
3. **Ignore benchmarks entirely** and hope no one notices until a customer
   complains about latency.

None of these test whether the change is **statistically significant**. That's
what Welch's t-test is for: it tells you the probability that two samples
came from distributions with the same mean, under the realistic assumption
that the two groups may have different variances.

`benchdiff-rs` wraps the statistics in a CLI that:

- **Parses** Criterion, Go bench, hyperfine, and generic CSV.
- **Saves baselines** as labelled JSON files under `.benchdiff/baselines/`.
- **Compares** a new run to a baseline using Welch's t-test + Cohen's d.
- **Gates CI** with a clean exit code.
- **Reports** in text / Markdown / JSON — drop the Markdown into a PR comment.

And it does all of this as a single 2-ish MB Rust binary with no runtime.

## Features

- **Multi-format input** — Criterion `sample.json` / `estimates.json`
  directory trees, Go `-bench` stdout, hyperfine `--export-json`, and a
  generic `name,value,unit` CSV.
- **Welch's two-sample t-test** with Welch-Satterthwaite degrees of freedom,
  computed via a regularized incomplete beta function (no `statrs` needed).
- **Cohen's d** effect size so you see magnitude, not just significance.
- **Benjamini-Hochberg FDR correction** (`--correction bh`) for when you
  have lots of benchmarks and don't want to chase false positives.
- **Minimum relative change threshold** (default 5%) so statistically
  significant but meaningless changes don't trip CI.
- **Per-benchmark tolerance overrides** and glob-based ignore patterns.
- **Cross-platform.** Handles Windows CRLF line endings in all text parsers.
  Uses `PathBuf` throughout; no path concatenation.
- **Safe input handling.** Files > 256 MB are rejected up front. Single
  lines > 1 MB are rejected. Baseline labels are sanitized against path
  traversal.
- **Three report formats.** Rich terminal table, GitHub-flavored Markdown
  with emoji verdicts, and JSON for downstream tooling.
- **No `unsafe` code.** `#![forbid(unsafe_code)]` enforced.

## Install

### From source

```bash
git clone https://github.com/JSLEEKR/benchdiff-rs
cd benchdiff-rs
cargo install --path .
```

### Via cargo

```bash
cargo install --git https://github.com/JSLEEKR/benchdiff-rs
```

This installs the `benchdiff` binary into `~/.cargo/bin`.

## Quick start

```bash
# 1. Run your benchmarks however you usually do.
cargo bench                                  # produces target/criterion/
go test -bench=. -count=5 > bench.txt        # Go bench
hyperfine --export-json bench.json "./my-cmd"

# 2. Save the current run as a baseline.
benchdiff save target/criterion --label v1.0.0
benchdiff save bench.txt        --label v1.0.0
benchdiff save bench.json       --label v1.0.0

# 3. Later, after changes, compare.
benchdiff compare target/criterion --against v1.0.0 --output markdown \
    --out benchdiff-report.md

# 4. CI gating.
benchdiff compare bench.txt --against v1.0.0 --allow-regressions 0
# exit 0 → all clean; exit 1 → at least one real regression.
```

## Usage

### `benchdiff save`

```text
benchdiff save <INPUT> --label <LABEL> [--format auto|criterion|gobench|hyperfine|csv]
```

- `<INPUT>` can be a directory (Criterion default: `target/criterion`), a
  single JSON file (`hyperfine --export-json`), a Go bench stdout file, or
  a CSV file with columns `name,value,unit`.
- `--label` is the name under which the baseline is stored. Must match
  `[A-Za-z0-9._+\-]+` and cannot start with `.`.
- `--format` defaults to `auto`: directories are assumed to be Criterion,
  `.csv` files are CSV, `.json` files are peeked at to tell hyperfine from
  Criterion, and everything else is Go bench text.

### `benchdiff compare`

```text
benchdiff compare <INPUT> --against <LABEL> \
    [--alpha 0.05] [--min-change 0.05] \
    [--correction none|bh] \
    [--output text|markdown|json] [--out <FILE>] \
    [--allow-regressions N]
```

- `--alpha` — significance level for the t-test. Default `0.05`.
- `--min-change` — minimum absolute relative change to flag. Default `0.05`
  (5%). Even if a change is statistically significant, it is not reported
  as a regression unless it also exceeds this threshold.
- `--correction bh` — apply Benjamini-Hochberg FDR correction across all
  compared benchmarks. Downgrades marginal rejections. Useful when you have
  hundreds of benchmarks.
- `--output` — report format. `text` prints a comfy-table; `markdown` is
  GitHub-flavored with emoji; `json` is machine-readable.
- `--allow-regressions N` — allow up to N regressions before exiting 1.
  Handy when you're landing a deliberately-slower refactor and know exactly
  which benches will change.

### `benchdiff list`

Prints all baselines in the current baseline directory.

### `benchdiff init`

Prints a starter `benchdiff.toml` template to stdout.

### `benchdiff summary`

```text
benchdiff summary <INPUT> --against <LABEL> --top 10
```

Like `compare` but shows only the top-N biggest relative changes. Useful
when your benchmark suite has hundreds of benches.

### `benchdiff inspect`

Debug aid. Parses an input file and prints the detected benchmarks without
doing any comparison.

## Statistical background

### Welch's two-sample t-test

For two samples A (baseline) and B (current):

```
mean_a = (1/n_a) Σ a_i
mean_b = (1/n_b) Σ b_i
var_a  = (1/(n_a-1)) Σ (a_i - mean_a)²    [Bessel-corrected]
var_b  = (1/(n_b-1)) Σ (b_i - mean_b)²

se = sqrt(var_a/n_a + var_b/n_b)
t  = (mean_b - mean_a) / se
```

Welch-Satterthwaite degrees of freedom:

```
df = (var_a/n_a + var_b/n_b)² /
     ( (var_a/n_a)²/(n_a-1) + (var_b/n_b)²/(n_b-1) )
```

The two-sided p-value is computed via the identity linking the t-distribution
and the regularized incomplete beta:

```
p = I_x(df/2, 1/2)    where    x = df / (df + t²)
```

The incomplete beta is evaluated with Lentz's continued fraction (see
Numerical Recipes §6.4). This means **no dependency on `statrs`** for core
math — `benchdiff` links only `serde`, `clap`, `comfy-table`, and friends.

### Cohen's d

Effect size using the pooled standard deviation:

```
pooled_sd = sqrt(((n_a-1)*var_a + (n_b-1)*var_b) / (n_a + n_b - 2))
d         = (mean_b - mean_a) / pooled_sd
```

Reported in every row so you can distinguish "statistically significant but
tiny" from "a meaningful change".

### Why a minimum relative change?

With enough samples, **any** difference becomes statistically significant.
A 0.4% slowdown at `p < 1e-20` is probably not a release blocker. The
`--min-change` flag (default 5%) filters out statistically-real-but-trivial
changes. You can override it globally, per config, or per benchmark via
`[tolerance]` in `benchdiff.toml`.

### Multiple comparison correction

Testing 100 benchmarks at α=0.05 expects 5 false positives by chance. If
you want to bound the false-discovery rate instead, pass `--correction bh`
for Benjamini-Hochberg. This is conservative about calling small changes
regressions when you have many tests.

### Edge cases

| Situation                               | Verdict          |
|-----------------------------------------|------------------|
| `n < 2` in either sample                | `insufficient`   |
| Both variances zero, equal means        | `unchanged`      |
| Both variances zero, different means    | real change, `p=0` |
| Benchmark in baseline but not current   | `missing`        |
| Benchmark in current but not baseline   | `new`            |
| Benchmark name matches an ignore glob   | `ignored`        |

## Config file

`benchdiff.toml` in the working directory (or passed via `--config`):

```toml
# Significance level for the t-test.
alpha = 0.05

# Minimum absolute relative change (e.g. 0.05 = 5%) to flag.
min_relative_change = 0.05

# "none" (default) or "bh" for Benjamini-Hochberg FDR.
correction = "none"

# Where to save / load baselines.
baseline_dir = ".benchdiff/baselines"

[ignore]
# Glob-lite patterns: `*` as prefix, suffix, or both.
patterns = ["experimental_*", "*_legacy", "*bench*"]

[tolerance]
# Per-benchmark overrides: allowed relative change for this bench only.
"parse_huge_json" = 0.15
"slow_encode"     = 0.20
```

### Generating a starter config

```bash
benchdiff init > benchdiff.toml
```

## CI integration

### GitHub Actions (Criterion)

```yaml
name: bench-regression
on: [pull_request]

jobs:
  benchdiff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install --git https://github.com/JSLEEKR/benchdiff-rs

      # Load baseline from default branch.
      - uses: actions/cache@v4
        with:
          path: .benchdiff
          key: benchdiff-${{ github.base_ref }}

      - run: cargo bench
      - run: |
          benchdiff compare target/criterion \
              --against main \
              --output markdown \
              --out benchdiff-report.md \
              --allow-regressions 0

      - uses: actions/upload-artifact@v4
        with:
          name: benchdiff-report
          path: benchdiff-report.md
```

### GitHub Actions (Go bench)

```yaml
- run: go test -bench=. -count=5 -benchmem > bench.txt
- run: benchdiff compare bench.txt --against main --output markdown --out report.md
```

### GitLab CI

```yaml
benchdiff:
  script:
    - cargo install --git https://github.com/JSLEEKR/benchdiff-rs
    - cargo bench
    - benchdiff compare target/criterion --against main --allow-regressions 0
  artifacts:
    paths:
      - benchdiff-report.md
```

## Exit codes

| code | meaning                                 |
|------|-----------------------------------------|
| 0    | No regressions (or within `--allow-regressions`) |
| 1    | One or more regressions detected        |
| 2    | Usage / CLI error                       |
| 3    | Parse error                             |
| 4    | I/O error                               |
| 5    | Statistical error (insufficient samples)|

## Input format details

### Criterion

`benchdiff-rs` walks a directory tree and looks for `<bench>/new/sample.json`
first (raw per-iteration timings) and falls back to `<bench>/new/estimates.json`
(synthesizing three samples from `mean ± std_dev`). Criterion's `report/`
subdirectory is explicitly skipped.

### Go bench

Parses standard `go test -bench=.` output line by line. `-count=N`
invocations aggregate into N samples per benchmark. Subtest names like
`BenchmarkEncodeJSON/small` are preserved verbatim; the trailing `-8`
GOMAXPROCS marker is stripped.

### hyperfine

Reads the JSON from `hyperfine --export-json`. The `times` array (in seconds)
becomes the sample vector. When `times` is empty, falls back to synthesizing
three points from `mean ± stddev`.

### Generic CSV

```csv
name,value,unit
parse_small,120,ns
parse_small,122,ns
parse_large,4520,ns
parse_large,4510,ns
```

Header is optional. Units: `ns`, `us`, `ms`, `s`, `ops`, `bytes`. Rows with
the same `name` aggregate into a single benchmark; mixing units within one
benchmark is an error.

## Security notes

- **No network I/O.** `benchdiff` never dials the internet.
- **No shelling out.** All parsing happens in-process.
- **No `unsafe` code.** Enforced at the crate level.
- **Bounded inputs.** Files larger than 256 MB and individual lines longer
  than 1 MB are rejected with a clear error.
- **Path traversal hardening.** Baseline labels are validated against a
  strict character allow-list and cannot start with `.`.
- **`NaN` / `Infinity` are rejected** at parse time — they cannot smuggle
  themselves through the t-test.

## Windows notes

- All text parsers accept both LF and CRLF line endings.
- Paths use `PathBuf` exclusively; no string concatenation.
- Baseline label validation blocks every character that is illegal in a
  Windows filename (`/ \ : * ? " < > |`).

## Development

```bash
git clone https://github.com/JSLEEKR/benchdiff-rs
cd benchdiff-rs
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## License

[MIT](LICENSE) © 2026 JSLEEKR.

## Acknowledgements

- Welch's t-test and the Welch-Satterthwaite df formula.
- Numerical Recipes in C §6.4 for Lentz's continued fraction.
- Howard Hinnant's date algorithms for epoch conversion.
- Criterion.rs, hyperfine, and the Go testing package for the benchmark
  output formats this tool consumes.
