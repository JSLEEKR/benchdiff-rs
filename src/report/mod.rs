//! Report rendering.

pub mod json;
pub mod markdown;
pub mod text;

use crate::compare::DiffReport;
use crate::error::Result;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Text,
    Markdown,
    Json,
}

impl Output {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "text" | "txt" => Ok(Self::Text),
            "markdown" | "md" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            other => Err(crate::error::Error::parse(
                "output",
                format!("unknown output format: {other}"),
            )),
        }
    }
}

/// Render a diff report in the chosen format.
pub fn render(report: &DiffReport, out: Output) -> Result<String> {
    match out {
        Output::Text => Ok(text::render(report)),
        Output::Markdown => Ok(markdown::render(report)),
        Output::Json => json::render(report),
    }
}

/// Threshold at which we treat a saturated sentinel value as "effectively
/// infinite" for display purposes. `compare::INF_SENTINEL` is `1e300`; we
/// pick a comfortably lower threshold so any computation producing a very
/// large finite number also gets rendered as `∞` in human output.
const DISPLAY_INF_THRESHOLD: f64 = 1e100;

/// Format a nanosecond count with a human-friendly unit.
#[must_use]
pub fn fmt_ns(ns: f64) -> String {
    if !ns.is_finite() {
        return "—".to_string();
    }
    let abs = ns.abs();
    if abs >= DISPLAY_INF_THRESHOLD {
        return "∞".to_string();
    }
    if abs >= 1e9 {
        format!("{:.3} s", ns / 1e9)
    } else if abs >= 1e6 {
        format!("{:.3} ms", ns / 1e6)
    } else if abs >= 1e3 {
        format!("{:.3} us", ns / 1e3)
    } else {
        format!("{ns:.3} ns")
    }
}

/// Format a relative change (`0.05` → `+5.00%`).
///
/// Non-finite and saturation sentinels (`>= 1e100`) both render as `∞` /
/// `-∞` so human output stays readable even when the underlying JSON
/// carries a large finite placeholder for "effectively infinite".
#[must_use]
pub fn fmt_pct(r: f64) -> String {
    if !r.is_finite() {
        return "∞".to_string();
    }
    if r.abs() >= DISPLAY_INF_THRESHOLD {
        return if r > 0.0 { "+∞".to_string() } else { "-∞".to_string() };
    }
    format!("{:+.2}%", r * 100.0)
}

/// Format Cohen's d, collapsing the saturation sentinel to `∞` / `-∞`.
#[must_use]
pub fn fmt_d(d: f64) -> String {
    if !d.is_finite() {
        return "∞".to_string();
    }
    if d.abs() >= DISPLAY_INF_THRESHOLD {
        return if d > 0.0 { "∞".to_string() } else { "-∞".to_string() };
    }
    format!("{d:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_ns_ranges() {
        assert_eq!(fmt_ns(5.0), "5.000 ns");
        assert_eq!(fmt_ns(5_000.0), "5.000 us");
        assert_eq!(fmt_ns(5_000_000.0), "5.000 ms");
        assert_eq!(fmt_ns(5_000_000_000.0), "5.000 s");
    }

    #[test]
    fn fmt_ns_nonfinite() {
        assert_eq!(fmt_ns(f64::NAN), "—");
        assert_eq!(fmt_ns(f64::INFINITY), "—");
    }

    #[test]
    fn fmt_pct_sign() {
        assert_eq!(fmt_pct(0.05), "+5.00%");
        assert_eq!(fmt_pct(-0.05), "-5.00%");
        assert_eq!(fmt_pct(0.0), "+0.00%");
    }

    #[test]
    fn fmt_pct_infinite() {
        assert_eq!(fmt_pct(f64::INFINITY), "∞");
    }

    #[test]
    fn fmt_pct_saturation_sentinel() {
        // eval-B: compare::INF_SENTINEL (1e300) must render as `+∞`, not
        // as a 300-digit percentage string.
        assert_eq!(fmt_pct(1.0e300), "+∞");
        assert_eq!(fmt_pct(-1.0e300), "-∞");
    }

    #[test]
    fn fmt_ns_saturation_sentinel() {
        assert_eq!(fmt_ns(1.0e300), "∞");
    }

    #[test]
    fn fmt_d_basic_and_infinite() {
        assert_eq!(fmt_d(1.2345), "1.23");
        assert_eq!(fmt_d(f64::INFINITY), "∞");
        assert_eq!(fmt_d(1.0e300), "∞");
        assert_eq!(fmt_d(-1.0e300), "-∞");
    }

    #[test]
    fn output_parse_variants() {
        assert_eq!(Output::parse("text").unwrap(), Output::Text);
        assert_eq!(Output::parse("TXT").unwrap(), Output::Text);
        assert_eq!(Output::parse("markdown").unwrap(), Output::Markdown);
        assert_eq!(Output::parse("md").unwrap(), Output::Markdown);
        assert_eq!(Output::parse("json").unwrap(), Output::Json);
        assert!(Output::parse("xml").is_err());
    }
}
