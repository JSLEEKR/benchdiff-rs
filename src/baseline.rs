//! Baseline save / load.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::sample::Run;

/// A labelled snapshot of a benchmark run, persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Schema version (starts at 1).
    pub schema: u32,
    pub label: String,
    /// RFC3339-ish timestamp. Informational only.
    pub created_at: String,
    pub run: Run,
}

impl Baseline {
    #[must_use]
    pub fn new(label: impl Into<String>, run: Run) -> Self {
        Self {
            schema: 1,
            label: label.into(),
            created_at: now_iso_utc(),
            run,
        }
    }

    /// Validate that `label` is safe to use as a filename.
    pub fn validate_label(label: &str) -> Result<()> {
        if label.is_empty() {
            return Err(Error::InvalidLabel {
                label: label.into(),
                reason: "label must be non-empty".into(),
            });
        }
        if label.len() > 128 {
            return Err(Error::InvalidLabel {
                label: label.into(),
                reason: "label too long (max 128)".into(),
            });
        }
        for c in label.chars() {
            let ok = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+');
            if !ok {
                return Err(Error::InvalidLabel {
                    label: label.into(),
                    reason: format!("character {c:?} not allowed in label"),
                });
            }
        }
        if label.starts_with('.') {
            return Err(Error::InvalidLabel {
                label: label.into(),
                reason: "label cannot start with '.'".into(),
            });
        }
        Ok(())
    }

    /// Build the JSON file path for a given label under `dir`.
    pub fn path_for(dir: &Path, label: &str) -> Result<PathBuf> {
        Self::validate_label(label)?;
        Ok(dir.join(format!("{label}.json")))
    }

    /// Persist this baseline to disk at `dir/<label>.json`.
    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        Self::validate_label(&self.label)?;
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let path = dir.join(format!("{}.json", self.label));
        let json = serde_json::to_string_pretty(self).map_err(|e| Error::Json {
            path: path.clone(),
            source: e,
        })?;
        std::fs::write(&path, json).map_err(|e| Error::io(&path, e))?;
        Ok(path)
    }

    /// Load a baseline by label.
    pub fn load(dir: &Path, label: &str) -> Result<Self> {
        Self::validate_label(label)?;
        let path = dir.join(format!("{label}.json"));
        if !path.exists() {
            return Err(Error::BaselineNotFound {
                label: label.into(),
                dir: dir.to_path_buf(),
            });
        }
        let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
        let b: Self =
            serde_json::from_str(&text).map_err(|e| Error::json(path.clone(), e))?;
        if b.schema != 1 {
            return Err(Error::parse(
                "baseline",
                format!("unsupported schema version {}", b.schema),
            ));
        }
        Ok(b)
    }

    /// List all `*.json` labels in `dir`. Missing dir → empty list.
    pub fn list(dir: &Path) -> Result<Vec<String>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
        for e in entries {
            let e = e.map_err(|err| Error::io(dir, err))?;
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }
}

/// Tiny RFC3339-ish UTC timestamp without pulling `chrono`.
#[must_use]
pub fn now_iso_utc() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    // Convert to broken-down UTC manually (safe for years 1970..).
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Epoch seconds → (year, month, day, hour, minute, second). UTC. Proleptic
/// Gregorian. Good for 1970-01-01 onward.
#[must_use]
pub fn epoch_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400) as u32;
    let h = time_of_day / 3600;
    let mi = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Algorithm from Howard Hinnant's date library.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{Benchmark, Unit};
    use tempfile::tempdir;

    fn sample_run() -> Run {
        let mut r = Run::new("test");
        r.benchmarks
            .push(Benchmark::new("foo", vec![100.0, 101.0, 99.0, 100.5], Unit::Ns).unwrap());
        r.benchmarks
            .push(Benchmark::new("bar", vec![5.0, 5.1, 4.9, 5.0], Unit::Us).unwrap());
        r
    }

    #[test]
    fn validate_label_accepts_normal() {
        assert!(Baseline::validate_label("v1.0.0").is_ok());
        assert!(Baseline::validate_label("main-abc123").is_ok());
        assert!(Baseline::validate_label("release_2026-04-13").is_ok());
    }

    #[test]
    fn validate_label_rejects_empty() {
        assert!(Baseline::validate_label("").is_err());
    }

    #[test]
    fn validate_label_rejects_dot_prefix() {
        assert!(Baseline::validate_label(".hidden").is_err());
    }

    #[test]
    fn validate_label_rejects_path_chars() {
        for bad in &["v1/2", "v1\\2", "v1:2", "v1*2", "v1?2", "v1 2"] {
            assert!(Baseline::validate_label(bad).is_err(), "bad: {bad}");
        }
    }

    #[test]
    fn validate_label_rejects_too_long() {
        let long = "a".repeat(200);
        assert!(Baseline::validate_label(&long).is_err());
    }

    #[test]
    fn path_for_builds_expected() {
        let p = Baseline::path_for(Path::new("/tmp"), "v1.0.0").unwrap();
        assert!(p.to_string_lossy().ends_with("v1.0.0.json"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let b = Baseline::new("v1", sample_run());
        let path = b.save(dir.path()).unwrap();
        assert!(path.exists());
        let loaded = Baseline::load(dir.path(), "v1").unwrap();
        assert_eq!(loaded.label, "v1");
        assert_eq!(loaded.run.benchmarks.len(), 2);
        assert_eq!(loaded.schema, 1);
    }

    #[test]
    fn load_missing_errors() {
        let dir = tempdir().unwrap();
        let err = Baseline::load(dir.path(), "nope").unwrap_err();
        assert!(matches!(err, Error::BaselineNotFound { .. }));
    }

    #[test]
    fn list_empty_missing_dir() {
        let labels = Baseline::list(Path::new("/nonexistent-benchdiff-test")).unwrap();
        assert!(labels.is_empty());
    }

    #[test]
    fn list_returns_sorted_labels() {
        let dir = tempdir().unwrap();
        Baseline::new("v2", sample_run()).save(dir.path()).unwrap();
        Baseline::new("v1", sample_run()).save(dir.path()).unwrap();
        Baseline::new("v10", sample_run())
            .save(dir.path())
            .unwrap();
        let labels = Baseline::list(dir.path()).unwrap();
        assert_eq!(labels, vec!["v1", "v10", "v2"]);
    }

    #[test]
    fn epoch_conversion_known() {
        // 2000-01-01T00:00:00Z is the canonical Y2K epoch 946684800.
        let (y, mo, d, h, mi, s) = epoch_to_ymdhms(946_684_800);
        assert_eq!((y, mo, d, h, mi, s), (2000, 1, 1, 0, 0, 0));
    }

    #[test]
    fn epoch_conversion_leap_day() {
        // 2024-02-29T12:34:56Z = 1709210096.
        let (y, mo, d, h, mi, s) = epoch_to_ymdhms(1_709_210_096);
        assert_eq!((y, mo, d, h, mi, s), (2024, 2, 29, 12, 34, 56));
    }

    #[test]
    fn epoch_conversion_1970() {
        let (y, mo, d, h, mi, s) = epoch_to_ymdhms(0);
        assert_eq!((y, mo, d, h, mi, s), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn now_iso_has_shape() {
        let s = now_iso_utc();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }
}
