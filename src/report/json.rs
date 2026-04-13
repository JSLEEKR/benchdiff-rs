//! JSON report (machine-readable).

use crate::compare::DiffReport;
use crate::error::{Error, Result};

pub fn render(report: &DiffReport) -> Result<String> {
    serde_json::to_string_pretty(report).map_err(|e| Error::Parse {
        format: "json-report".into(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{DiffReport, DiffRow, Verdict};

    fn sample() -> DiffReport {
        DiffReport {
            baseline_label: "v1".into(),
            alpha: 0.05,
            min_relative_change: 0.05,
            correction: "none".into(),
            rows: vec![DiffRow {
                name: "foo".into(),
                baseline_mean_ns: 100.0,
                current_mean_ns: 150.0,
                n_baseline: 10,
                n_current: 10,
                rel_change: 0.5,
                p_value: 0.001,
                cohens_d: 3.0,
                verdict: Verdict::Regression,
            }],
        }
    }

    #[test]
    fn renders_pretty_json() {
        let s = render(&sample()).unwrap();
        assert!(s.contains("\"baseline_label\""));
        assert!(s.contains("\"foo\""));
        assert!(s.contains("\"Regression\""));
    }

    #[test]
    fn round_trips_through_serde() {
        let s = render(&sample()).unwrap();
        let back: DiffReport = serde_json::from_str(&s).unwrap();
        assert_eq!(back.rows.len(), 1);
        assert_eq!(back.baseline_label, "v1");
    }
}
