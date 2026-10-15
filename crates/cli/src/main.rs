use clap::{Parser, Subcommand, ValueEnum};
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

#[derive(Clone, Copy, ValueEnum)]
enum ReportKind {
    Local,
    Archive,
    Snapshot,
    Git,
}
#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Html,
    Json,
}

#[derive(Subcommand)]
enum Command {
    Report {
        left: String,
        right: String,
        destination: String,
        #[arg(long, value_enum, default_value = "local")]
        source: ReportKind,
        #[arg(long, value_enum, default_value = "html")]
        format: OutputFormat,
        #[arg(long)]
        repository: Option<String>,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        metadata_only: bool,
        #[arg(long)]
        differences_only: bool,
        #[arg(long)]
        english: bool,
    },
    Compare {
        left: String,
        right: String,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        metadata_only: bool,
    },
    GitCompare {
        repository: String,
        left_revision: String,
        right_revision: String,
        #[arg(long, default_value = "")]
        exclude: String,
    },
    Snapshot {
        root: String,
        destination: String,
        #[arg(long, default_value = "")]
        exclude: String,
    },
    Snapshots {
        left: String,
        right: Option<String>,
        #[arg(long, default_value = "")]
        exclude: String,
    },
    Archives {
        left: String,
        right: Option<String>,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        metadata_only: bool,
    },
    MergeFolders {
        base: String,
        left: String,
        right: String,
        output: String,
        #[arg(long, default_value = "")]
        exclude: String,
        #[arg(long)]
        journal_dir: PathBuf,
        #[arg(long = "choose")]
        choices: Vec<String>,
        #[arg(long)]
        apply: bool,
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
        html: bool,
        #[arg(long, conflicts_with = "html")]
        delimited: bool,
        #[arg(long)]
        list_tables: bool,
        #[arg(long)]
        left_html: Option<usize>,
        #[arg(long)]
        right_html: Option<usize>,
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
        Command::Report {
            left,
            right,
            destination,
            source,
            format,
            repository,
            exclude,
            metadata_only,
            differences_only,
            english,
        } => {
            use clear_contracts::{ReportFormat, ReportSource};
            if repository.is_some() && !matches!(source, ReportKind::Git) {
                return Err(clear_core::Error::Invalid(
                    "--repository only applies to --source git".into(),
                ));
            }

            let (report, roots, source) = match source {
                ReportKind::Local => {
                    let snapshot = clear_core::compare::compare(
                        &CompareRequest {
                            left,
                            right,
                            exclusions: exclude,
                            content: !metadata_only,
                        },
                        &cancel,
                    )?;
                    let roots = vec![snapshot.left.clone(), snapshot.right.clone()];
                    (snapshot.report, roots, ReportSource::Local)
                }
                ReportKind::Archive => {
                    let snapshot = clear_core::archive::compare(
                        &clear_contracts::ArchiveRequest {
                            left,
                            right: if right == "-" { String::new() } else { right },
                            exclusions: exclude,
                            content: !metadata_only,
                        },
                        &cancel,
                    )?;
                    let roots = snapshot.report_roots();
                    (snapshot.report, roots, ReportSource::Archive)
                }
                ReportKind::Snapshot => {
                    let snapshot = clear_core::tree_snapshot::compare(
                        &clear_contracts::ArchiveRequest {
                            left,
                            right: if right == "-" { String::new() } else { right },
                            exclusions: exclude,
                            content: false,
                        },
                        &cancel,
                    )?;
                    let roots = snapshot.report_roots();
                    (snapshot.report, roots, ReportSource::TreeSnapshot)
                }
                ReportKind::Git => {
                    if metadata_only {
                        return Err(clear_core::Error::Invalid(
                            "Git report uses object identity; --metadata-only is not applicable"
                                .into(),
                        ));
                    }
                    let repository = repository.ok_or_else(|| {
                        clear_core::Error::Invalid("Git reports require --repository".into())
                    })?;
                    let snapshot = clear_core::git::compare(
                        &clear_contracts::GitCompareRequest {
                            repository,
                            left_revision: left,
                            right_revision: right,
                            exclusions: exclude,
                        },
                        &cancel,
                    )?;
                    let roots = snapshot.report_roots();
                    (snapshot.report, roots, ReportSource::Git)
                }
            };
            let result = clear_core::report::export(
                &report,
                &clear_contracts::ReportRequest {
                    source,
                    format: match format {
                        OutputFormat::Html => ReportFormat::Html,
                        OutputFormat::Json => ReportFormat::Json,
                    },
                    session: report.session.clone(),
                    destination,
                    english,
                    differences_only,
                },
                &roots,
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(if report.summary.errors > 0 {
                2
            } else if report
                .rows
                .iter()
                .any(|r| r.status != clear_contracts::Status::Same)
            {
                1
            } else {
                0
            })
        }

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
        Command::GitCompare {
            repository,
            left_revision,
            right_revision,
            exclude,
        } => {
            let snapshot = clear_core::git::compare(
                &clear_contracts::GitCompareRequest {
                    repository,
                    left_revision,
                    right_revision,
                    exclusions: exclude,
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
        Command::Snapshot {
            root,
            destination,
            exclude,
        } => {
            let info = clear_core::tree_snapshot::capture(
                &clear_contracts::TreeSnapshotRequest {
                    root,
                    destination,
                    exclusions: exclude,
                },
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&info)?);
            Ok(if info.errors > 0 { 2 } else { 0 })
        }
        Command::Snapshots {
            left,
            right,
            exclude,
        } => {
            let paired = right.is_some();
            let snapshot = clear_core::tree_snapshot::compare(
                &clear_contracts::ArchiveRequest {
                    left,
                    right: right.unwrap_or_default(),
                    exclusions: exclude,
                    content: false,
                },
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&snapshot.report)?);
            Ok(if snapshot.report.summary.errors > 0 {
                2
            } else if paired
                && snapshot
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
        Command::Archives {
            left,
            right,
            exclude,
            metadata_only,
        } => {
            let paired = right.is_some();
            let snapshot = clear_core::archive::compare(
                &clear_contracts::ArchiveRequest {
                    left,
                    right: right.unwrap_or_default(),
                    exclusions: exclude,
                    content: !metadata_only,
                },
                &cancel,
            )?;
            println!("{}", serde_json::to_string_pretty(&snapshot.report)?);
            Ok(if snapshot.report.summary.errors > 0 {
                2
            } else if paired
                && snapshot
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
        Command::MergeFolders {
            base,
            left,
            right,
            output,
            exclude,
            journal_dir,
            choices,
            apply,
        } => {
            let mut plan = clear_core::folder_merge::prepare(
                &clear_contracts::FolderMergeRequest {
                    two_way: base == "-",
                    base,
                    left,
                    right,
                    output,
                    exclusions: exclude,
                },
                &journal_dir,
                &cancel,
            )?;
            for item in choices {
                let (path, choice) = item.rsplit_once('=').ok_or_else(|| {
                    clear_core::Error::Invalid(
                        "Use --choose relative/path=left|right|base|delete|auto".into(),
                    )
                })?;
                let resolution = match choice {
                    "combine" => clear_contracts::FolderResolution::Combine,
                    "left" => clear_contracts::FolderResolution::Left,
                    "right" => clear_contracts::FolderResolution::Right,
                    "base" => clear_contracts::FolderResolution::Base,
                    "delete" => clear_contracts::FolderResolution::Delete,
                    "auto" => clear_contracts::FolderResolution::Auto,
                    _ => {
                        return Err(clear_core::Error::Invalid(
                            "Invalid folder merge choice".into(),
                        ));
                    }
                };
                plan.resolve_path(std::path::Path::new(path), resolution)?;
            }
            if apply {
                let result = clear_core::folder_merge::execute(&plan, &cancel)?;
                println!("{}", serde_json::to_string_pretty(&result)?);
                Ok(
                    if result
                        .actions
                        .iter()
                        .all(|a| matches!(a.state.as_str(), "completed" | "omitted"))
                    {
                        0
                    } else {
                        2
                    },
                )
            } else {
                let preview = plan.preview(0);
                let conflicts = preview.conflicts;
                println!("{}", serde_json::to_string_pretty(&preview)?);
                Ok(i32::from(conflicts > 0))
            }
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
            html,
            delimited,
            list_tables,
            left_html,
            right_html,
            no_headers,
            key_column,
            numeric_tolerance,
        } => {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let document =
                clear_core::table::document::Document::from_paths(&left, &right, &cancel)?;
            let source = |selected: Option<usize>| {
                selected
                    .or(if html { Some(0) } else { None })
                    .map(|table| clear_contracts::TableSource::Html { table })
                    .unwrap_or(if delimited {
                        clear_contracts::TableSource::Delimited
                    } else {
                        clear_contracts::TableSource::Auto
                    })
            };
            if list_tables {
                let info = document.inspect(
                    matches!(source(left_html), clear_contracts::TableSource::Html { .. })
                        || (matches!(source(left_html), clear_contracts::TableSource::Auto)
                            && document.info.left_html),
                    matches!(
                        source(right_html),
                        clear_contracts::TableSource::Html { .. }
                    ) || (matches!(source(right_html), clear_contracts::TableSource::Auto)
                        && document.info.right_html),
                    &cancel,
                )?;
                println!("{}", serde_json::to_string_pretty(&info)?);
                return Ok(if info.left_error.is_some() || info.right_error.is_some() {
                    2
                } else {
                    0
                });
            }
            let result = document.compare(
                &clear_contracts::TableOptions {
                    left_source: source(left_html),
                    right_source: source(right_html),
                    delimiter: if tsv { "\t" } else { "," }.into(),
                    headers: !no_headers,
                    key_column,
                    numeric_tolerance,
                },
                &cancel,
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
