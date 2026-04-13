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

/// Format a nanosecond count with a human-friendly unit.
#[must_use]
pub fn fmt_ns(ns: f64) -> String {
    if !ns.is_finite() {
        return "—".to_string();
    }
    let abs = ns.abs();
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
#[must_use]
pub fn fmt_pct(r: f64) -> String {
    if !r.is_finite() {
        return "∞".to_string();
    }
    format!("{:+.2}%", r * 100.0)
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
    fn output_parse_variants() {
        assert_eq!(Output::parse("text").unwrap(), Output::Text);
        assert_eq!(Output::parse("TXT").unwrap(), Output::Text);
        assert_eq!(Output::parse("markdown").unwrap(), Output::Markdown);
        assert_eq!(Output::parse("md").unwrap(), Output::Markdown);
        assert_eq!(Output::parse("json").unwrap(), Output::Json);
        assert!(Output::parse("xml").is_err());
    }
}
