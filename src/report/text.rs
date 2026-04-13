//! Rich terminal output via `comfy-table`.

use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};

use crate::compare::{DiffReport, Verdict};
use crate::report::{fmt_ns, fmt_pct};

#[must_use]
pub fn render(report: &DiffReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "benchdiff: compared against baseline {:?} (alpha={}, threshold={}%, correction={})\n\n",
        report.baseline_label,
        report.alpha,
        report.min_relative_change * 100.0,
        report.correction,
    ));

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            "benchmark",
            "baseline",
            "current",
            "Δ",
            "p",
            "d",
            "verdict",
        ]);

    for row in &report.rows {
        let verdict_str = match row.verdict {
            Verdict::Regression => "[!] regression",
            Verdict::Improvement => "[+] improvement",
            Verdict::Unchanged => "    unchanged",
            Verdict::MissingFromCurrent => "    missing",
            Verdict::NewInCurrent => "    new",
            Verdict::Ignored => "    ignored",
            Verdict::Insufficient => "    insufficient",
        };
        let p = if row.p_value >= 0.9999 {
            "—".to_string()
        } else if row.p_value < 0.0001 {
            "<1e-4".to_string()
        } else {
            format!("{:.4}", row.p_value)
        };
        let d = if row.cohens_d.is_finite() {
            format!("{:.2}", row.cohens_d)
        } else {
            "∞".to_string()
        };
        table.add_row(vec![
            row.name.clone(),
            fmt_ns(row.baseline_mean_ns),
            fmt_ns(row.current_mean_ns),
            fmt_pct(row.rel_change),
            p,
            d,
            verdict_str.to_string(),
        ]);
    }
    out.push_str(&table.to_string());
    out.push('\n');
    out.push_str(&format!(
        "\nsummary: {} regression(s), {} improvement(s), {} unchanged\n",
        report.regressions(),
        report.improvements(),
        report.unchanged(),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{DiffRow, Verdict};

    fn sample_report() -> DiffReport {
        DiffReport {
            baseline_label: "v1.0.0".into(),
            alpha: 0.05,
            min_relative_change: 0.05,
            correction: "none".into(),
            rows: vec![
                DiffRow {
                    name: "fast".into(),
                    baseline_mean_ns: 100.0,
                    current_mean_ns: 150.0,
                    n_baseline: 10,
                    n_current: 10,
                    rel_change: 0.5,
                    p_value: 0.0001,
                    cohens_d: 3.0,
                    verdict: Verdict::Regression,
                },
                DiffRow {
                    name: "slow".into(),
                    baseline_mean_ns: 200.0,
                    current_mean_ns: 100.0,
                    n_baseline: 10,
                    n_current: 10,
                    rel_change: -0.5,
                    p_value: 0.0001,
                    cohens_d: -3.0,
                    verdict: Verdict::Improvement,
                },
            ],
        }
    }

    #[test]
    fn renders_contains_benchmark_names() {
        let s = render(&sample_report());
        assert!(s.contains("fast"));
        assert!(s.contains("slow"));
        assert!(s.contains("regression"));
        assert!(s.contains("improvement"));
    }

    #[test]
    fn renders_contains_summary() {
        let s = render(&sample_report());
        assert!(s.contains("1 regression"));
        assert!(s.contains("1 improvement"));
    }

    #[test]
    fn renders_baseline_label() {
        let s = render(&sample_report());
        assert!(s.contains("v1.0.0"));
    }
}
