//! CLI surface via clap derive.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::baseline::Baseline;
use crate::compare::compare_runs;
use crate::config::Config;
use crate::error::Error;
use crate::parsers::{parse_any, Format};
use crate::report::{self, Output};

#[derive(Parser, Debug)]
#[command(
    name = "benchdiff",
    about = "Statistical benchmark regression detector for Criterion, Go bench, and hyperfine",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Path to a `benchdiff.toml` config file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Directory where baselines live (overrides config).
    #[arg(long, global = true)]
    pub baseline_dir: Option<PathBuf>,

    /// Be quieter.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Save a benchmark run as a labelled baseline.
    Save {
        /// Input file or directory.
        input: PathBuf,
        /// Label under which to store this baseline (e.g. `v1.0.0`).
        #[arg(long)]
        label: String,
        /// Force an explicit parser instead of auto-detecting.
        #[arg(long)]
        format: Option<String>,
    },

    /// Compare a current run against a stored baseline.
    Compare {
        /// Current benchmark input.
        input: PathBuf,
        /// Baseline label to compare against.
        #[arg(long)]
        against: String,
        /// Force an explicit parser instead of auto-detecting.
        #[arg(long)]
        format: Option<String>,
        /// Significance level override.
        #[arg(long)]
        alpha: Option<f64>,
        /// Minimum relative change to flag (e.g. `0.05` = 5%).
        #[arg(long)]
        min_change: Option<f64>,
        /// Multiple-comparison correction (`none` or `bh`).
        #[arg(long)]
        correction: Option<String>,
        /// Output format.
        #[arg(long, default_value = "text")]
        output: String,
        /// Write report to a file instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Allow up to N regressions without failing.
        #[arg(long, default_value_t = 0)]
        allow_regressions: u32,
    },

    /// List saved baselines.
    List,

    /// Print a `benchdiff.toml` template.
    Init,

    /// Show the top-N most-changed benchmarks vs a baseline.
    Summary {
        input: PathBuf,
        #[arg(long)]
        against: String,
        #[arg(long, default_value_t = 10)]
        top: usize,
        #[arg(long)]
        format: Option<String>,
    },

    /// Print benchmark metadata from a parsed input (debug aid).
    Inspect {
        input: PathBuf,
        #[arg(long)]
        format: Option<String>,
    },
}

/// Entry point — map the `Cli` struct into an action and run it.
///
/// Returns `Ok(exit_code)` so `main` can translate to process exit.
#[allow(clippy::too_many_lines)]
pub fn run(cli: Cli) -> anyhow::Result<i32> {
    // Resolve config first.
    let mut cfg = Config::load_or_default(cli.config.as_deref())?;
    if let Some(dir) = &cli.baseline_dir {
        cfg.baseline_dir = dir.clone();
    }

    match cli.command {
        Command::Save {
            input,
            label,
            format,
        } => {
            Baseline::validate_label(&label)?;
            let fmt = parse_format(format.as_deref())?;
            let run = parse_any(&input, fmt)?;
            run.require_non_empty()?;
            let baseline = Baseline::new(label.clone(), run);
            let path = baseline.save(&cfg.baseline_dir)?;
            if !cli.quiet {
                println!(
                    "saved baseline {:?} with {} benchmark(s) to {}",
                    label,
                    baseline.run.benchmarks.len(),
                    path.display()
                );
            }
            Ok(0)
        }

        Command::Compare {
            input,
            against,
            format,
            alpha,
            min_change,
            correction,
            output,
            out,
            allow_regressions,
        } => {
            if let Some(a) = alpha {
                cfg.alpha = a;
            }
            if let Some(m) = min_change {
                cfg.min_relative_change = m;
            }
            if let Some(c) = correction {
                cfg.correction = c;
            }
            cfg.validate()?;

            let fmt = parse_format(format.as_deref())?;
            let current = parse_any(&input, fmt)?;
            current.require_non_empty()?;
            let baseline = Baseline::load(&cfg.baseline_dir, &against)?;

            let report = compare_runs(&baseline.run, &current, &against, &cfg)?;
            let out_fmt = Output::parse(&output)?;
            let rendered = report::render(&report, out_fmt)?;
            write_output(out.as_deref(), &rendered)?;

            let regs = report.regressions() as u32;
            if regs > allow_regressions {
                if !cli.quiet {
                    eprintln!(
                        "benchdiff: {regs} regression(s) found (allowed: {allow_regressions})"
                    );
                }
                return Ok(1);
            }
            Ok(0)
        }

        Command::List => {
            let labels = Baseline::list(&cfg.baseline_dir)?;
            if labels.is_empty() {
                if !cli.quiet {
                    println!("no baselines in {}", cfg.baseline_dir.display());
                }
            } else {
                for l in labels {
                    println!("{l}");
                }
            }
            Ok(0)
        }

        Command::Init => {
            print!("{}", Config::template());
            Ok(0)
        }

        Command::Summary {
            input,
            against,
            top,
            format,
        } => {
            let fmt = parse_format(format.as_deref())?;
            let current = parse_any(&input, fmt)?;
            let baseline = Baseline::load(&cfg.baseline_dir, &against)?;
            let report = compare_runs(&baseline.run, &current, &against, &cfg)?;
            let mut rows = report.rows.clone();
            rows.sort_by(|a, b| {
                b.rel_change
                    .abs()
                    .partial_cmp(&a.rel_change.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            rows.truncate(top);
            let small = crate::compare::DiffReport {
                rows,
                ..report
            };
            let rendered = report::render(&small, Output::Text)?;
            println!("{rendered}");
            Ok(0)
        }

        Command::Inspect { input, format } => {
            let fmt = parse_format(format.as_deref())?;
            let run = parse_any(&input, fmt)?;
            println!("source: {}", run.source);
            println!("benchmarks: {}", run.benchmarks.len());
            for b in &run.benchmarks {
                println!("  {:<40}  n={:<5}  unit={}", b.name, b.n(), b.unit.as_str());
            }
            Ok(0)
        }
    }
}

fn parse_format(opt: Option<&str>) -> Result<Option<Format>, Error> {
    match opt {
        None => Ok(None),
        Some("auto") => Ok(None),
        Some(s) => Ok(Some(Format::parse(s)?)),
    }
}

fn write_output(path: Option<&Path>, content: &str) -> Result<(), Error> {
    match path {
        Some(p) => std::fs::write(p, content).map_err(|e| Error::io(p, e)),
        None => {
            print!("{content}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_structure_parses() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_format_auto() {
        assert!(parse_format(Some("auto")).unwrap().is_none());
        assert!(parse_format(None).unwrap().is_none());
        assert_eq!(
            parse_format(Some("gobench")).unwrap().unwrap(),
            Format::GoBench
        );
    }

    #[test]
    fn parse_format_rejects_unknown() {
        assert!(parse_format(Some("magic")).is_err());
    }
}
