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

    /// Try to load from a path if it exists, otherwise return default.
    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(p) if p.exists() => Self::load(p),
            _ => Ok(Self::default()),
        }
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
    #[must_use]
    pub fn tolerance_for(&self, name: &str) -> Option<f64> {
        self.tolerance.get(name).and_then(|v| v.as_float())
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

/// Tiny glob matcher: supports `*` at start, end, or both.
#[must_use]
pub fn glob_match(pattern: &str, name: &str) -> bool {
    match (pattern.starts_with('*'), pattern.ends_with('*')) {
        (true, true) => {
            let inner = &pattern[1..pattern.len() - 1];
            name.contains(inner)
        }
        (true, false) => {
            let suffix = &pattern[1..];
            name.ends_with(suffix)
        }
        (false, true) => {
            let prefix = &pattern[..pattern.len() - 1];
            name.starts_with(prefix)
        }
        (false, false) => pattern == name,
    }
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
    fn ignore_exact() {
        let mut c = Config::default();
        c.ignore.patterns.push("exact".into());
        assert!(c.is_ignored("exact"));
        assert!(!c.is_ignored("not-exact"));
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
