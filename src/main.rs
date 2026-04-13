//! `benchdiff` binary entry point.
#![forbid(unsafe_code)]
#![deny(warnings)]

use clap::Parser;

use benchdiff::cli::{run, Cli};

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("benchdiff error: {err:#}");
            let chain = format!("{err:#}");
            let code = if chain.contains("parse") {
                3
            } else if chain.contains("I/O") || chain.contains("no such file") {
                4
            } else if chain.contains("insufficient") {
                5
            } else {
                2
            };
            std::process::exit(code);
        }
    }
}
