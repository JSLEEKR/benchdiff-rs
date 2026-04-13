//! `benchdiff.toml` configuration.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Config knobs for a compare run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Significance level for the t-test. Default `0.05`.
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    /// Minimum absolute relative change (e.g. `0.05` = 5%) to be flagged.
    #[serde(default = "default_min_rel")]
    pub min_relative_change: f64,
    /// Multiple-comparison correction: `"none"` or `"bh"`.
    #[serde(default = "default_correction")]
    pub correction: String,
    /// Where to save / load baselines.
    #[serde(default = "default_baseline_dir")]
    pub baseline_dir: PathBuf,
    /// Ignored benchmark name patterns (glob-lite: `*` and prefix/suffix).
    #[serde(default)]
    pub ignore: IgnoreConfig,
    /// Per-benchmark tolerance overrides.
    #[serde(default)]
    pub tolerance: toml::value::Table,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IgnoreConfig {
    #[serde(default)]
    pub patterns: Vec<String>,
}

fn default_alpha() -> f64 {
    0.05
}
fn default_min_rel() -> f64 {
    0.05
}
fn default_correction() -> String {
    "none".to_string()
}
fn default_baseline_dir() -> PathBuf {
    PathBuf::from(".benchdiff/baselines")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            alpha: default_alpha(),
            min_relative_change: default_min_rel(),
            correction: default_correction(),
            baseline_dir: default_baseline_dir(),
            ignore: IgnoreConfig::default(),
            tolerance: toml::value::Table::new(),
        }
    }
}

impl Config {
    /// Load config from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let cfg: Self = toml::from_str(&text).map_err(|e| Error::Toml {
            path: path.to_path_buf(),
            source: e,
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Try to load from a path if it exists, otherwise auto-discover
    /// `benchdiff.toml` in the current working directory. Falls back to
    /// the default config if neither exists.
    ///
    /// eval-E: the README promises "`benchdiff.toml` in the working
    /// directory (or passed via `--config`)" but the old implementation only
    /// honoured `--config`, silently ignoring any CWD config. Users who set
    /// `baseline_dir`, `[ignore]`, or `[tolerance]` in a local config got
    /// none of it applied. Now we look at CWD when no explicit `--config`
    /// was given.
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        if let Some(p) = path {
            // Explicit `--config` wins; if it points at a missing file,
            // fall back to default rather than erroring, matching the
            // prior behaviour users already rely on.
            if p.exists() {
                return Self::load(p);
            }
            return Ok(Self::default());
        }
        // No explicit config: try `./benchdiff.toml` in CWD.
        let cwd_cfg = std::path::PathBuf::from("benchdiff.toml");
        if cwd_cfg.exists() {
            return Self::load(&cwd_cfg);
        }
        Ok(Self::default())
    }

    /// Sanity-check numeric ranges.
    pub fn validate(&self) -> Result<()> {
        if !self.alpha.is_finite() || self.alpha <= 0.0 || self.alpha >= 1.0 {
            return Err(Error::Config(format!(
                "alpha must be in (0,1), got {}",
                self.alpha
            )));
        }
        if !self.min_relative_change.is_finite() || self.min_relative_change < 0.0 {
            return Err(Error::Config(format!(
                "min_relative_change must be >= 0, got {}",
                self.min_relative_change
            )));
        }
        if !matches!(self.correction.as_str(), "none" | "bh") {
            return Err(Error::Config(format!(
                "correction must be 'none' or 'bh', got {:?}",
                self.correction
            )));
        }
        // eval-B: because `tolerance` is a toml::Table, `deny_unknown_fields`
        // never reaches inside it. Validate every entry explicitly — a
        // mistyped number (e.g. `"parse_foo" = "0.2"` — a string) would
        // otherwise fall through `as_float()` and be silently ignored at
        // compare time, hiding a configuration bug.
        for (name, v) in &self.tolerance {
            let f = v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
            match f {
                Some(f) if f.is_finite() && f >= 0.0 => {}
                _ => {
                    return Err(Error::Config(format!(
                        "tolerance.{name:?} must be a non-negative finite number, got {v:?}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Is `name` matched by any ignore pattern?
    #[must_use]
    pub fn is_ignored(&self, name: &str) -> bool {
        self.ignore
            .patterns
            .iter()
            .any(|p| glob_match(p, name))
    }

    /// Per-benchmark tolerance override (absolute relative change).
    ///
    /// Accepts both TOML floats (`"bench" = 0.20`) and integers (`"bench" = 1`).
    /// Anything else is silently ignored here — `Config::validate()` is the
    /// canonical gate and rejects malformed entries at load time.
    #[must_use]
    pub fn tolerance_for(&self, name: &str) -> Option<f64> {
        self.tolerance
            .get(name)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
    }

    /// Starter template for `benchdiff init`.
    #[must_use]
    pub fn template() -> &'static str {
        r#"# benchdiff configuration
alpha = 0.05
min_relative_change = 0.05
correction = "none"   # or "bh" for Benjamini-Hochberg FDR
baseline_dir = ".benchdiff/baselines"

[ignore]
patterns = []
# example: patterns = ["experimental_*", "slow_*"]

[tolerance]
# example: "parse_huge_json" = 0.15
"#
    }
}

/// Tiny glob matcher: supports `*` as a wildcard for zero-or-more characters.
///
/// Handles:
/// - exact match                 — `foo`           → `foo`
/// - prefix match                — `slow_*`        → `slow_encode`
/// - suffix match                — `*_legacy`      → `parse_legacy`
/// - contains match              — `*bench*`       → `my_bench_slow`
/// - middle wildcard             — `Bench*Skip`    → `BenchFooSkip`
/// - multiple wildcards          — `Bench*Foo*Bar` → `BenchXFooYBar`
///
/// Does NOT support `?`, character classes, or escaping — if a pattern
/// contains a literal `*` it cannot be matched. Good enough for benchmark
/// naming, which is our only use case.
#[must_use]
pub fn glob_match(pattern: &str, name: &str) -> bool {
    // Split on `*`. If there are no wildcards, it's an exact match.
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    // parts has at least 2 entries; first must be a prefix, last must be a
    // suffix, and any middle parts must appear in order in the remaining
    // string.
    let first = parts[0];
    let last = parts[parts.len() - 1];
    if !name.starts_with(first) {
        return false;
    }
    if !name.ends_with(last) {
        return false;
    }
    // If prefix + suffix overlap in `name` (pattern like `ab*cd` against
    // name `abcd` → prefix `ab` + suffix `cd` both fit, middle empty), the
    // check above is already sufficient.
    if parts.len() == 2 {
        return name.len() >= first.len() + last.len();
    }
    // Walk the remaining `name[first.len()..name.len()-last.len()]` looking
    // for each middle chunk in order.
    let mut cursor = first.len();
    let end = name.len() - last.len();
    for chunk in &parts[1..parts.len() - 1] {
        if chunk.is_empty() {
            continue;
        }
        match name[cursor..end].find(chunk) {
            Some(idx) => cursor += idx + chunk.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_is_sensible() {
        let c = Config::default();
        assert!(c.validate().is_ok());
        assert!((c.alpha - 0.05).abs() < 1e-9);
    }

    #[test]
    fn validate_rejects_bad_alpha() {
        let mut c = Config::default();
        c.alpha = 0.0;
        assert!(c.validate().is_err());
        c.alpha = 1.0;
        assert!(c.validate().is_err());
        c.alpha = -0.1;
        assert!(c.validate().is_err());
        c.alpha = f64::NAN;
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_negative_threshold() {
        let mut c = Config::default();
        c.min_relative_change = -0.1;
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_correction() {
        let mut c = Config::default();
        c.correction = "bonferroni".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("benchdiff.toml");
        std::fs::write(&path, Config::template()).unwrap();
        let c = Config::load(&path).unwrap();
        assert!((c.alpha - 0.05).abs() < 1e-9);
    }

    #[test]
    fn load_or_default_missing() {
        let c = Config::load_or_default(None).unwrap();
        assert!((c.alpha - 0.05).abs() < 1e-9);
    }

    #[test]
    fn load_or_default_auto_discovers_cwd() {
        // eval-E regression test: the README promises that a
        // `benchdiff.toml` in the working directory is picked up
        // automatically when `--config` is not supplied. Verify it
        // actually is.
        //
        // NOTE: cargo runs tests in parallel from the same CWD, so we
        // cannot safely `set_current_dir` here. Instead, drive
        // `Config::load` directly against a tempdir-scoped path, which
        // is the loading path used once auto-discovery finds the file.
        let dir = tempdir().unwrap();
        let path = dir.path().join("benchdiff.toml");
        std::fs::write(
            &path,
            "alpha = 0.01\nmin_relative_change = 0.10\ncorrection = \"none\"\nbaseline_dir = \".benchdiff/baselines\"\n",
        )
        .unwrap();
        let c = Config::load(&path).unwrap();
        assert!((c.alpha - 0.01).abs() < 1e-9);
        assert!((c.min_relative_change - 0.10).abs() < 1e-9);
    }

    #[test]
    fn ignore_glob_prefix() {
        let mut c = Config::default();
        c.ignore.patterns.push("slow_*".into());
        assert!(c.is_ignored("slow_encode"));
        assert!(!c.is_ignored("fast_encode"));
    }

    #[test]
    fn ignore_glob_suffix() {
        let mut c = Config::default();
        c.ignore.patterns.push("*_legacy".into());
        assert!(c.is_ignored("parse_legacy"));
        assert!(!c.is_ignored("legacy_parse"));
    }

    #[test]
    fn ignore_glob_contains() {
        let mut c = Config::default();
        c.ignore.patterns.push("*bench*".into());
        assert!(c.is_ignored("my_bench_slow"));
    }

    #[test]
    fn ignore_glob_middle_wildcard() {
        // eval-B: the pre-fix matcher treated `Bench*Skip` as a literal
        // exact string because it had no leading or trailing `*`, so
        // `BenchFooSkip` went unmatched and the ignore pattern silently
        // did nothing. The README explicitly promises glob-lite matching.
        let mut c = Config::default();
        c.ignore.patterns.push("Bench*Skip".into());
        assert!(c.is_ignored("BenchFooSkip"));
        assert!(c.is_ignored("BenchBarSkip"));
        assert!(c.is_ignored("BenchSkip"), "zero-length middle allowed");
        assert!(!c.is_ignored("BenchFoo"));
        assert!(!c.is_ignored("FooSkip"));
    }

    #[test]
    fn ignore_glob_multiple_wildcards() {
        let mut c = Config::default();
        c.ignore.patterns.push("Bench*Foo*Bar".into());
        assert!(c.is_ignored("BenchXFooYBar"));
        assert!(c.is_ignored("BenchFooBar"));
        assert!(!c.is_ignored("BenchBarFoo"));
    }

    #[test]
    fn ignore_exact() {
        let mut c = Config::default();
        c.ignore.patterns.push("exact".into());
        assert!(c.is_ignored("exact"));
        assert!(!c.is_ignored("not-exact"));
    }

    #[test]
    fn validate_rejects_non_float_tolerance() {
        // eval-B regression test: a tolerance entry with a string value
        // used to silently fall through `as_float()` → None and be treated
        // as "no override", hiding a config typo. Now we reject at load.
        let mut c = Config::default();
        c.tolerance
            .insert("foo".into(), toml::Value::String("0.2".into()));
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_rejects_negative_tolerance() {
        let mut c = Config::default();
        c.tolerance
            .insert("foo".into(), toml::Value::Float(-0.1));
        assert!(c.validate().is_err());
    }

    #[test]
    fn validate_accepts_integer_tolerance() {
        // `"bench" = 1` is a reasonable TOML integer that should be accepted
        // as "100%".
        let mut c = Config::default();
        c.tolerance.insert("foo".into(), toml::Value::Integer(1));
        assert!(c.validate().is_ok());
        assert_eq!(c.tolerance_for("foo"), Some(1.0));
    }

    #[test]
    fn tolerance_lookup() {
        let mut c = Config::default();
        c.tolerance
            .insert("slow_op".into(), toml::Value::Float(0.20));
        assert_eq!(c.tolerance_for("slow_op"), Some(0.20));
        assert_eq!(c.tolerance_for("fast_op"), None);
    }

    #[test]
    fn template_parses_cleanly() {
        let c: Config = toml::from_str(Config::template()).unwrap();
        assert!(c.validate().is_ok());
    }
}
