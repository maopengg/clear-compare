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
    Picture {
        left: PathBuf,
        right: PathBuf,
        #[arg(long, default_value_t = 0)]
        tolerance: u8,
        #[arg(long)]
        ignore_alpha: bool,
    },
    RecoverSync {
        journal: PathBuf,
        #[arg(long)]
        backup_dir: PathBuf,
        #[arg(long)]
        apply: bool,
    },
    RestoreEditor {
        target: PathBuf,
        backup: PathBuf,
        #[arg(long)]
        backup_dir: PathBuf,
        #[arg(long)]
        apply: bool,
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
            } else if snapshot
                .report
                .rows
                .iter()
                .any(|r| r.status != clear_contracts::Status::Same)
            {
                1
            } else {
                0
            })
        }
        Command::Picture {
            left,
            right,
            tolerance,
            ignore_alpha,
        } => {
            let result = clear_core::picture::compare_paths(
                Some(&left),
                Some(&right),
                &clear_contracts::PictureOptions {
                    tolerance,
                    ignore_alpha,
                },
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&result.summary)?);
            Ok(i32::from(result.summary.changed_pixels != "0"))
        }
        Command::RecoverSync {
            journal,
            backup_dir,
            apply,
        } => {
            let plan = clear_core::recovery::preview(&journal, &backup_dir, &cancel)?;
            if apply {
                let result = clear_core::recovery::execute(&plan, &cancel)?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                Ok(if result.actions.iter().all(|a| a.state == "completed") {
                    0
                } else {
                    2
                })
            } else {
                println!("{}", serde_json::to_string_pretty(&plan.view)?);
                Ok(0)
            }
        }
        Command::RestoreEditor {
            target,
            backup,
            backup_dir,
            apply,
        } => {
            let document = clear_core::editing::open_file(&target)?;
            let plan = clear_core::editing::preview_restore(&document, &backup)?;
            if apply {
                let (_, result) = clear_core::editing::apply_restore(&plan, &backup_dir)?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&plan.view)?);
            }
            Ok(0)
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
