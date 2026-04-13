//! Core sample / benchmark / run types.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Unit of a benchmark measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Nanoseconds.
    Ns,
    /// Microseconds.
    Us,
    /// Milliseconds.
    Ms,
    /// Seconds.
    S,
    /// Operations per second (higher = better — inverted vs time).
    Ops,
    /// Bytes (e.g. memory, allocation size — higher = worse).
    Bytes,
}

impl Unit {
    /// Convert a raw value in this unit to nanoseconds-equivalent.
    ///
    /// For non-time units (`Ops`, `Bytes`) the value is returned unchanged —
    /// comparisons still make sense as long as baseline and current use
    /// the same unit.
    #[must_use]
    pub fn to_ns(self, value: f64) -> f64 {
        match self {
            Self::Ns => value,
            Self::Us => value * 1_000.0,
            Self::Ms => value * 1_000_000.0,
            Self::S => value * 1_000_000_000.0,
            Self::Ops | Self::Bytes => value,
        }
    }

    /// Is this a time unit (lower = better) or throughput/size?
    #[must_use]
    pub const fn is_time(self) -> bool {
        matches!(self, Self::Ns | Self::Us | Self::Ms | Self::S)
    }

    /// Short display string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ns => "ns",
            Self::Us => "us",
            Self::Ms => "ms",
            Self::S => "s",
            Self::Ops => "ops",
            Self::Bytes => "bytes",
        }
    }

    /// Parse a unit from a string (case-insensitive).
    pub fn parse(s: &str) -> Result<Self> {
        let lower = s.trim().to_ascii_lowercase();
        match lower.as_str() {
            "ns" | "nanosecond" | "nanoseconds" | "ns/op" => Ok(Self::Ns),
            "us" | "μs" | "microsecond" | "microseconds" => Ok(Self::Us),
            "ms" | "millisecond" | "milliseconds" => Ok(Self::Ms),
            "s" | "sec" | "second" | "seconds" => Ok(Self::S),
            "ops" | "ops/s" | "throughput" => Ok(Self::Ops),
            "bytes" | "b" | "b/op" => Ok(Self::Bytes),
            other => Err(Error::parse("unit", format!("unknown unit {other:?}"))),
        }
    }
}

/// A single benchmark with its raw per-iteration samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Benchmark {
    pub name: String,
    /// Raw samples in the recorded unit (not yet normalized).
    pub samples: Vec<f64>,
    pub unit: Unit,
}

impl Benchmark {
    /// Construct, validating finiteness.
    pub fn new(name: impl Into<String>, samples: Vec<f64>, unit: Unit) -> Result<Self> {
        let name = name.into();
        for s in &samples {
            if !s.is_finite() {
                return Err(Error::NonFinite { name });
            }
        }
        Ok(Self {
            name,
            samples,
            unit,
        })
    }

    /// Samples converted to nanoseconds-equivalent (or passthrough).
    #[must_use]
    pub fn samples_normalized(&self) -> Vec<f64> {
        self.samples.iter().map(|&v| self.unit.to_ns(v)).collect()
    }

    /// Number of samples.
    #[must_use]
    pub fn n(&self) -> usize {
        self.samples.len()
    }
}

/// A full benchmark run (one snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub benchmarks: Vec<Benchmark>,
    /// Informational only — e.g. "criterion" or "go-bench".
    pub source: String,
}

impl Run {
    #[must_use]
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            benchmarks: Vec::new(),
            source: source.into(),
        }
    }

    /// Find a benchmark by exact name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Benchmark> {
        self.benchmarks.iter().find(|b| b.name == name)
    }

    /// Ensure non-empty (for strict paths).
    pub fn require_non_empty(&self) -> Result<()> {
        if self.benchmarks.is_empty() {
            Err(Error::EmptyInput)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_to_ns_basic() {
        assert!((Unit::Ns.to_ns(5.0) - 5.0).abs() < 1e-9);
        assert!((Unit::Us.to_ns(1.0) - 1_000.0).abs() < 1e-9);
        assert!((Unit::Ms.to_ns(1.0) - 1_000_000.0).abs() < 1e-9);
        assert!((Unit::S.to_ns(1.0) - 1_000_000_000.0).abs() < 1e-9);
        assert!((Unit::Ops.to_ns(42.0) - 42.0).abs() < 1e-9);
        assert!((Unit::Bytes.to_ns(42.0) - 42.0).abs() < 1e-9);
    }

    #[test]
    fn unit_is_time() {
        assert!(Unit::Ns.is_time());
        assert!(Unit::Ms.is_time());
        assert!(Unit::S.is_time());
        assert!(!Unit::Ops.is_time());
        assert!(!Unit::Bytes.is_time());
    }

    #[test]
    fn unit_parse_accepts_variants() {
        assert_eq!(Unit::parse("ns").unwrap(), Unit::Ns);
        assert_eq!(Unit::parse("NS").unwrap(), Unit::Ns);
        assert_eq!(Unit::parse("ns/op").unwrap(), Unit::Ns);
        assert_eq!(Unit::parse("microseconds").unwrap(), Unit::Us);
        assert_eq!(Unit::parse("ms").unwrap(), Unit::Ms);
        assert_eq!(Unit::parse("  s  ").unwrap(), Unit::S);
        assert_eq!(Unit::parse("ops").unwrap(), Unit::Ops);
        assert_eq!(Unit::parse("bytes").unwrap(), Unit::Bytes);
    }

    #[test]
    fn unit_parse_rejects_unknown() {
        assert!(Unit::parse("furlongs").is_err());
    }

    #[test]
    fn unit_as_str() {
        assert_eq!(Unit::Ns.as_str(), "ns");
        assert_eq!(Unit::Us.as_str(), "us");
        assert_eq!(Unit::Ms.as_str(), "ms");
        assert_eq!(Unit::S.as_str(), "s");
        assert_eq!(Unit::Ops.as_str(), "ops");
        assert_eq!(Unit::Bytes.as_str(), "bytes");
    }

    #[test]
    fn benchmark_rejects_nan() {
        let r = Benchmark::new("x", vec![1.0, f64::NAN], Unit::Ns);
        assert!(matches!(r, Err(Error::NonFinite { .. })));
    }

    #[test]
    fn benchmark_rejects_infinity() {
        let r = Benchmark::new("x", vec![f64::INFINITY], Unit::Ns);
        assert!(r.is_err());
    }

    #[test]
    fn benchmark_normalized_samples_convert() {
        let b = Benchmark::new("x", vec![1.0, 2.0], Unit::Us).unwrap();
        let ns = b.samples_normalized();
        assert!((ns[0] - 1000.0).abs() < 1e-9);
        assert!((ns[1] - 2000.0).abs() < 1e-9);
    }

    #[test]
    fn run_find_and_empty() {
        let mut run = Run::new("test");
        assert!(run.require_non_empty().is_err());
        run.benchmarks
            .push(Benchmark::new("a", vec![1.0], Unit::Ns).unwrap());
        assert!(run.require_non_empty().is_ok());
        assert!(run.find("a").is_some());
        assert!(run.find("b").is_none());
    }
}
