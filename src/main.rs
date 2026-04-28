use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

mod archive;
mod model;
mod output;
mod parser;

use output::{print_bench_report, print_module_summaries, print_report};
use parser::{benchmark_archive, module_summaries, parse_archive_report};

#[derive(Debug, Parser)]
#[command(name = "ckan-meta-rs")]
#[command(about = "Experimental CKAN metadata archive parser and benchmark tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse a CKAN metadata archive and report timing information.
    Parse {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
        archive: PathBuf,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,

        /// Number of parse errors to show in the terminal report.
        #[arg(long, default_value_t = 8)]
        max_errors: usize,
    },

    /// Parse the same archive repeatedly and report timing statistics.
    Bench {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
        archive: PathBuf,

        /// Number of measured runs.
        #[arg(long, default_value_t = 10)]
        runs: usize,

        /// Number of warmup runs to discard before measuring.
        #[arg(long, default_value_t = 2)]
        warmups: usize,

        /// Emit machine-readable JSON instead of a terminal report.
        #[arg(long)]
        json: bool,
    },

    /// Emit per-module summaries for compatibility comparison work.
    Modules {
        /// Path to a CKAN metadata .zip, .tar.gz archive, or extracted directory.
        archive: PathBuf,

        /// Emit one JSON object per line.
        #[arg(long)]
        json_lines: bool,

        /// Emit one pretty-printed JSON array.
        #[arg(long)]
        json: bool,

        /// Limit the number of module rows emitted.
        #[arg(long)]
        limit: Option<usize>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Parse {
            archive,
            json,
            max_errors,
        } => parse_archive(archive, json, max_errors),
        Command::Bench {
            archive,
            runs,
            warmups,
            json,
        } => bench(archive, runs, warmups, json),
        Command::Modules {
            archive,
            json_lines,
            json,
            limit,
        } => modules(archive, json, json_lines, limit),
    }
}

fn parse_archive(archive: PathBuf, json: bool, max_errors: usize) -> Result<()> {
    let report = parse_archive_report(archive)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report, max_errors);
    }

    Ok(())
}

fn bench(archive: PathBuf, runs: usize, warmups: usize, json: bool) -> Result<()> {
    let report = benchmark_archive(archive, runs, warmups)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_bench_report(&report);
    }

    Ok(())
}

fn modules(archive: PathBuf, json: bool, json_lines: bool, limit: Option<usize>) -> Result<()> {
    let modules = module_summaries(archive, limit)?;
    print_module_summaries(&modules, json, json_lines)
}
