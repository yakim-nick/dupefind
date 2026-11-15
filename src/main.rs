use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use dupefind::{Options, Report, find_duplicates};

/// Find duplicate files by content.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Directories to scan (files inside are compared across all of them).
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Ignore files smaller than this many bytes.
    #[arg(long, default_value_t = 1)]
    min_size: u64,

    /// Print the result as JSON instead of text.
    #[arg(long)]
    json: bool,

    /// Exit with status 1 when duplicates are found (useful in scripts and CI).
    #[arg(long)]
    fail_on_duplicates: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let report = find_duplicates(
        &cli.paths,
        &Options {
            min_size: cli.min_size,
        },
    );

    for error in &report.errors {
        eprintln!("warning: {error}");
    }
    if cli.json {
        print_json(&report);
    } else {
        print_text(&report);
    }

    if cli.fail_on_duplicates && !report.groups.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn print_text(report: &Report) {
    for (i, group) in report.groups.iter().enumerate() {
        println!(
            "Group {}: {} files, {} bytes each ({} bytes wasted)",
            i + 1,
            group.files.len(),
            group.size,
            group.wasted_bytes()
        );
        for file in &group.files {
            println!("  {}", file.display());
        }
    }
    println!(
        "{} duplicate group(s), {} bytes reclaimable, {} file(s) scanned",
        report.groups.len(),
        report.wasted_bytes(),
        report.scanned
    );
}

fn print_json(report: &Report) {
    let value = serde_json::json!({
        "scanned": report.scanned,
        "wasted_bytes": report.wasted_bytes(),
        "groups": report.groups,
        "errors": report.errors,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("report is serialisable")
    );
}
