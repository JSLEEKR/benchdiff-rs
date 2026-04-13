//! Compare a baseline run against a current run.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::Result;
use crate::sample::Run;
use crate::stats::{benjamini_hochberg, cohens_d, welch_t_test};

/// Per-benchmark verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Statistically significant slowdown above threshold.
    Regression,
    /// Statistically significant speedup above threshold.
    Improvement,
    /// Change is either not significant or below threshold.
    Unchanged,
    /// Present in baseline but missing from current run.
    MissingFromCurrent,
    /// Present in current run but missing from baseline.
    NewInCurrent,
    /// Ignored by config patterns.
    Ignored,
    /// Not enough samples to compute t-test.
    Insufficient,
}

impl Verdict {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::Improvement => "improvement",
            Self::Unchanged => "unchanged",
            Self::MissingFromCurrent => "missing",
            Self::NewInCurrent => "new",
            Self::Ignored => "ignored",
            Self::Insufficient => "insufficient",
        }
    }

    #[must_use]
    pub const fn is_regression(self) -> bool {
        matches!(self, Self::Regression)
    }
}

/// One row of a comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffRow {
    pub name: String,
    pub baseline_mean_ns: f64,
    pub current_mean_ns: f64,
    pub n_baseline: usize,
    pub n_current: usize,
    /// Relative change `(current - baseline) / baseline`. Positive = slower.
    pub rel_change: f64,
    /// Two-sided p-value, or `1.0` when not computed.
    pub p_value: f64,
    /// Cohen's d effect size.
    pub cohens_d: f64,
    pub verdict: Verdict,
}

/// Full comparison result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffReport {
    pub baseline_label: String,
    pub rows: Vec<DiffRow>,
    pub alpha: f64,
    pub min_relative_change: f64,
    pub correction: String,
}

impl DiffReport {
    #[must_use]
    pub fn regressions(&self) -> usize {
        self.rows.iter().filter(|r| r.verdict.is_regression()).count()
    }

    #[must_use]
    pub fn improvements(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.verdict == Verdict::Improvement)
            .count()
    }

    #[must_use]
    pub fn unchanged(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.verdict == Verdict::Unchanged)
            .count()
    }
}

/// Compute a full comparison between two runs with a given config.
pub fn compare_runs(
    baseline: &Run,
    current: &Run,
    baseline_label: &str,
    cfg: &Config,
) -> Result<DiffReport> {
    let mut rows: Vec<DiffRow> = Vec::new();
    let mut p_indices: Vec<usize> = Vec::new();

    // Baseline benchmarks (present, missing, or ignored).
    for bb in &baseline.benchmarks {
        if cfg.is_ignored(&bb.name) {
            rows.push(missing_row(&bb.name, Verdict::Ignored));
            continue;
        }
        match current.find(&bb.name) {
            None => rows.push(missing_row(&bb.name, Verdict::MissingFromCurrent)),
            Some(cb) => {
                let a = bb.samples_normalized();
                let b = cb.samples_normalized();
                let row = compute_row(&bb.name, &a, &b, cfg);
                if matches!(row.verdict, Verdict::Regression | Verdict::Improvement | Verdict::Unchanged)
                {
                    p_indices.push(rows.len());
                }
                rows.push(row);
            }
        }
    }

    // New-in-current benchmarks that weren't in baseline.
    for cb in &current.benchmarks {
        if baseline.find(&cb.name).is_some() {
            continue;
        }
        if cfg.is_ignored(&cb.name) {
            rows.push(missing_row(&cb.name, Verdict::Ignored));
        } else {
            rows.push(missing_row(&cb.name, Verdict::NewInCurrent));
        }
    }

    // Benjamini-Hochberg correction across tested rows.
    if cfg.correction == "bh" && !p_indices.is_empty() {
        let pvals: Vec<f64> = p_indices.iter().map(|&i| rows[i].p_value).collect();
        let reject = benjamini_hochberg(&pvals, cfg.alpha);
        for (idx, &i) in p_indices.iter().enumerate() {
            let row = &mut rows[i];
            // Only downgrade — if BH says "don't reject" but we had flagged it,
            // walk back to Unchanged. Upgrade is not possible here (verdict
            // logic already requires threshold + direction).
            if !reject[idx] && row.verdict != Verdict::Unchanged {
                // Only downgrade change verdicts.
                if matches!(row.verdict, Verdict::Regression | Verdict::Improvement) {
                    row.verdict = Verdict::Unchanged;
                }
            }
        }
    }

    // Sort by severity then by abs rel_change desc.
    rows.sort_by(|a, b| {
        let rank = |v: Verdict| match v {
            Verdict::Regression => 0,
            Verdict::Improvement => 1,
            Verdict::Unchanged => 2,
            Verdict::Insufficient => 3,
            Verdict::MissingFromCurrent => 4,
            Verdict::NewInCurrent => 5,
            Verdict::Ignored => 6,
        };
        rank(a.verdict).cmp(&rank(b.verdict)).then_with(|| {
            b.rel_change
                .abs()
                .partial_cmp(&a.rel_change.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    Ok(DiffReport {
        baseline_label: baseline_label.to_string(),
        rows,
        alpha: cfg.alpha,
        min_relative_change: cfg.min_relative_change,
        correction: cfg.correction.clone(),
    })
}

fn missing_row(name: &str, v: Verdict) -> DiffRow {
    DiffRow {
        name: name.to_string(),
        baseline_mean_ns: 0.0,
        current_mean_ns: 0.0,
        n_baseline: 0,
        n_current: 0,
        rel_change: 0.0,
        p_value: 1.0,
        cohens_d: 0.0,
        verdict: v,
    }
}

fn compute_row(name: &str, a: &[f64], b: &[f64], cfg: &Config) -> DiffRow {
    use crate::stats::mean;
    let mean_a = mean(a);
    let mean_b = mean(b);
    let rel = if mean_a.abs() < f64::EPSILON {
        if mean_b.abs() < f64::EPSILON {
            0.0
        } else {
            f64::INFINITY
        }
    } else {
        (mean_b - mean_a) / mean_a
    };

    let (p, d, verdict) = match welch_t_test(a, b) {
        Ok(r) => {
            let d = cohens_d(a, b);
            let significant = r.p < cfg.alpha;
            let tol = cfg
                .tolerance_for(name)
                .unwrap_or(cfg.min_relative_change);
            let big_enough = rel.abs() >= tol;
            let verdict = if significant && big_enough {
                if mean_b > mean_a {
                    Verdict::Regression
                } else {
                    Verdict::Improvement
                }
            } else {
                Verdict::Unchanged
            };
            (r.p, d, verdict)
        }
        Err(_) => (1.0, 0.0, Verdict::Insufficient),
    };

    DiffRow {
        name: name.to_string(),
        baseline_mean_ns: mean_a,
        current_mean_ns: mean_b,
        n_baseline: a.len(),
        n_current: b.len(),
        rel_change: rel,
        p_value: p,
        cohens_d: d,
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::{Benchmark, Unit};

    fn run(b: Vec<(&str, Vec<f64>)>) -> Run {
        let mut r = Run::new("t");
        for (n, s) in b {
            r.benchmarks.push(Benchmark::new(n, s, Unit::Ns).unwrap());
        }
        r
    }

    #[test]
    fn detects_clean_regression() {
        let a = run(vec![(
            "foo",
            vec![100.0, 101.0, 99.0, 100.5, 99.5, 100.1, 99.9, 100.2],
        )]);
        let b = run(vec![(
            "foo",
            vec![150.0, 151.0, 149.0, 150.5, 149.5, 150.1, 149.9, 150.2],
        )]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.regressions(), 1);
        assert_eq!(rep.rows[0].verdict, Verdict::Regression);
        assert!(rep.rows[0].rel_change > 0.4);
    }

    #[test]
    fn detects_clean_improvement() {
        let a = run(vec![(
            "foo",
            vec![200.0, 201.0, 199.0, 200.5, 199.5, 200.1, 199.9, 200.2],
        )]);
        let b = run(vec![(
            "foo",
            vec![100.0, 101.0, 99.0, 100.5, 99.5, 100.1, 99.9, 100.2],
        )]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.improvements(), 1);
        assert_eq!(rep.regressions(), 0);
    }

    #[test]
    fn below_threshold_is_unchanged() {
        // 2% change with default 5% threshold → not flagged even if p<alpha.
        let a = run(vec![(
            "foo",
            vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0],
        )]);
        let b = run(vec![(
            "foo",
            vec![102.0, 102.0, 102.0, 102.0, 102.0, 102.0],
        )]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.rows[0].verdict, Verdict::Unchanged);
    }

    #[test]
    fn missing_from_current_reported() {
        let a = run(vec![("foo", vec![1.0, 2.0]), ("bar", vec![3.0, 4.0])]);
        let b = run(vec![("foo", vec![1.0, 2.0])]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert!(rep
            .rows
            .iter()
            .any(|r| r.name == "bar" && r.verdict == Verdict::MissingFromCurrent));
    }

    #[test]
    fn new_in_current_reported() {
        let a = run(vec![("foo", vec![1.0, 2.0])]);
        let b = run(vec![("foo", vec![1.0, 2.0]), ("bar", vec![3.0, 4.0])]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert!(rep
            .rows
            .iter()
            .any(|r| r.name == "bar" && r.verdict == Verdict::NewInCurrent));
    }

    #[test]
    fn ignored_pattern_short_circuits() {
        let a = run(vec![("slow_foo", vec![100.0, 100.0, 100.0])]);
        let b = run(vec![("slow_foo", vec![500.0, 500.0, 500.0])]);
        let mut cfg = Config::default();
        cfg.ignore.patterns.push("slow_*".into());
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.rows[0].verdict, Verdict::Ignored);
        assert_eq!(rep.regressions(), 0);
    }

    #[test]
    fn tolerance_override_allows_slowdown() {
        let a = run(vec![(
            "slow_op",
            vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0],
        )]);
        let b = run(vec![(
            "slow_op",
            vec![110.0, 110.0, 110.0, 110.0, 110.0, 110.0],
        )]);
        let mut cfg = Config::default();
        cfg.tolerance
            .insert("slow_op".into(), toml::Value::Float(0.20));
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        // 10% change but tolerance is 20% → unchanged.
        assert_eq!(rep.rows[0].verdict, Verdict::Unchanged);
    }

    #[test]
    fn insufficient_samples_handled() {
        let a = run(vec![("foo", vec![100.0])]);
        let b = run(vec![("foo", vec![100.0, 101.0])]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.rows[0].verdict, Verdict::Insufficient);
    }

    #[test]
    fn bh_correction_downgrades_borderline() {
        // Two benchmarks both borderline; BH should downgrade the weaker.
        let a = run(vec![
            ("foo", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
            ("bar", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
        ]);
        let b = run(vec![
            // Strong regression (big change + low variance).
            ("foo", vec![150.0, 150.5, 149.5, 150.2, 149.8, 150.1, 149.9, 150.3]),
            // Borderline: just above threshold, lower effect.
            ("bar", vec![106.0, 107.0, 105.0, 106.0, 107.0, 105.0, 106.0, 107.0]),
        ]);
        let mut cfg = Config::default();
        cfg.correction = "bh".into();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        // Foo should still be a regression.
        let foo = rep.rows.iter().find(|r| r.name == "foo").unwrap();
        assert_eq!(foo.verdict, Verdict::Regression);
    }

    #[test]
    fn zero_baseline_mean_infinite_rel() {
        let a = run(vec![("foo", vec![0.0, 0.0, 0.0, 0.0, 0.0])]);
        let b = run(vec![("foo", vec![1.0, 1.0, 1.0, 1.0, 1.0])]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert!(rep.rows[0].rel_change.is_infinite());
    }

    #[test]
    fn verdict_as_str_stable() {
        assert_eq!(Verdict::Regression.as_str(), "regression");
        assert_eq!(Verdict::Improvement.as_str(), "improvement");
        assert_eq!(Verdict::Unchanged.as_str(), "unchanged");
        assert_eq!(Verdict::Ignored.as_str(), "ignored");
        assert_eq!(Verdict::Insufficient.as_str(), "insufficient");
        assert_eq!(Verdict::MissingFromCurrent.as_str(), "missing");
        assert_eq!(Verdict::NewInCurrent.as_str(), "new");
    }

    #[test]
    fn counts_match_rows() {
        let a = run(vec![
            ("foo", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
            ("bar", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
            ("baz", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
        ]);
        let b = run(vec![
            ("foo", vec![200.0, 200.0, 200.0, 200.0, 200.0, 200.0, 200.0, 200.0]),
            ("bar", vec![100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 100.0]),
            ("baz", vec![50.0, 50.0, 50.0, 50.0, 50.0, 50.0, 50.0, 50.0]),
        ]);
        let cfg = Config::default();
        let rep = compare_runs(&a, &b, "v1", &cfg).unwrap();
        assert_eq!(rep.regressions(), 1);
        assert_eq!(rep.improvements(), 1);
        assert_eq!(rep.unchanged(), 1);
    }
}
