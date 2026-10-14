use clap::{Parser, Subcommand};
use clear_contracts::CompareRequest;
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[derive(Parser)]
#[command(
    version,
    about = "Local file comparison, text merging and verified directional copy"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Compare {
        left: String,
        right: String,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        metadata_only: bool,
    },
    Table {
        left: PathBuf,
        right: PathBuf,
        #[arg(long)]
        tsv: bool,
        #[arg(long)]
        no_headers: bool,
        #[arg(long)]
        key_column: Option<usize>,
        #[arg(long)]
        numeric_tolerance: Option<String>,
    },
    Merge {
        base: PathBuf,
        left: PathBuf,
        right: PathBuf,
    },
    Diff {
        left: PathBuf,
        right: PathBuf,
        #[arg(long)]
        ignore_case: bool,
        #[arg(long)]
        ignore_whitespace: bool,
        #[arg(long)]
        ignore_line_endings: bool,
    },
    Sync {
        left: String,
        right: String,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        backup_dir: PathBuf,
        #[arg(long)]
        reverse: bool,
        #[arg(long)]
        apply: bool,
    },
}

fn run(cli: Cli) -> clear_core::Result<i32> {
    let cancel = AtomicBool::new(false);
    match cli.command {
        Command::Compare {
            left,
            right,
            exclude,
            metadata_only,
        } => {
            let snapshot = clear_core::compare::compare(
                &CompareRequest {
                    left,
                    right,
                    exclusions: exclude,
                    content: !metadata_only,
                },
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&snapshot.report)?);
            Ok(if snapshot.report.summary.errors > 0 {
                2
            } else if snapshot.report.summary.changed
                + snapshot.report.summary.left_only
                + snapshot.report.summary.right_only
                + snapshot.report.summary.unverified
                > 0
            {
                1
            } else {
                0
            })
        }
        Command::Table {
            left,
            right,
            tsv,
            no_headers,
            key_column,
            numeric_tolerance,
        } => {
            let result = clear_core::table::compare(
                &clear_core::text::read_text(&left)?,
                &clear_core::text::read_text(&right)?,
                &clear_contracts::TableOptions {
                    delimiter: if tsv { "\t" } else { "," }.into(),
                    headers: !no_headers,
                    key_column,
                    numeric_tolerance,
                },
            )?;
            let changed =
                result.summary.changed + result.summary.left_only + result.summary.right_only > 0
                    || result.left_headers != result.right_headers;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(i32::from(changed))
        }
        Command::Merge { base, left, right } => {
            let result = clear_core::merge::three_way(
                &clear_core::text::read_text(&base)?,
                &clear_core::text::read_text(&left)?,
                &clear_core::text::read_text(&right)?,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(i32::from(result.conflicts > 0))
        }
        Command::Diff {
            left,
            right,
            ignore_case,
            ignore_whitespace,
            ignore_line_endings,
        } => {
            let diff = clear_core::text::compare_strings(
                "text",
                &clear_core::text::read_text(&left)?,
                &clear_core::text::read_text(&right)?,
                &clear_contracts::TextOptions {
                    ignore_case,
                    ignore_whitespace,
                    ignore_line_endings,
                },
            )?;
            let different = diff.lines.iter().any(|l| l.tag != "equal");
            println!("{}", serde_json::to_string_pretty(&diff)?);
            Ok(i32::from(different))
        }
        Command::Sync {
            left,
            right,
            exclude,
            backup_dir,
            reverse,
            apply,
        } => {
            let s = clear_core::compare::compare(
                &CompareRequest {
                    left,
                    right,
                    exclusions: exclude,
                    content: true,
                },
                &cancel,
            )?;
            let ids = s
                .report
                .rows
                .iter()
                .filter(|r| {
                    (r.status == clear_contracts::Status::Changed
                        || r.status
                            == if reverse {
                                clear_contracts::Status::RightOnly
                            } else {
                                clear_contracts::Status::LeftOnly
                            })
                        && (if reverse { &r.right } else { &r.left })
                            .as_ref()
                            .is_some_and(|e| e.kind == clear_contracts::Kind::File)
                })
                .map(|r| r.id.clone())
                .collect::<Vec<_>>();
            let plan = clear_core::sync::plan_direction(
                &s,
                &ids,
                &backup_dir,
                &cancel,
                if reverse {
                    clear_contracts::Side::Right
                } else {
                    clear_contracts::Side::Left
                },
            )?;
            if !apply {
                println!("{}", serde_json::to_string_pretty(&plan)?);
                return Ok(0);
            }
            let result = clear_core::sync::execute(&s, &plan, &cancel)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(if result.actions.iter().all(|a| a.state == "completed") {
                0
            } else {
                2
            })
        }
    }
}

fn main() {
    let code = match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            2
        }
    };
    std::process::exit(code);
}
