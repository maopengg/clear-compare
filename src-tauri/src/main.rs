#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bidirectional;
mod native_menu;
mod operations;
mod workbench;
use clear_contracts::*;
use clear_core::compare::Snapshot;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct Backend {
    workspace: Mutex<Option<clear_core::workbench::Workspace>>,
    text_merge_dirty: AtomicBool,
    workspace_dirty: AtomicBool,
    baseline: Mutex<Option<(String, PathBuf)>>,
    bidirectional: Mutex<Option<clear_core::bidirectional::Plan>>,
    bidirectional_recovery: Mutex<Option<clear_core::bidirectional::RecoveryPlan>>,
    operation: Mutex<Option<Arc<operations::DesktopOperation>>>,
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    archive: Mutex<Option<Arc<clear_core::archive::Snapshot>>>,
    git: Mutex<Option<Arc<clear_core::git::Snapshot>>>,
    tree_snapshot: Mutex<Option<Arc<clear_core::tree_snapshot::Snapshot>>>,
    table: Mutex<Option<(String, Arc<TableComparison>)>>,
    table_document: Mutex<Option<Arc<clear_core::table::document::Document>>>,
    plan: Mutex<Option<SyncPlan>>,
    editor_backups: Mutex<BTreeMap<String, PathBuf>>,
    restore: Mutex<Option<(String, clear_core::editing::RestorePlan)>>,
    recovery: Mutex<Option<clear_core::recovery::Plan>>,
    restart: Mutex<Option<clear_core::sync::resume::Plan>>,
    folder_merge: Mutex<Option<clear_core::folder_merge::Plan>>,
    latest_merge: Mutex<Option<(String, Option<String>)>>,
    merge_guards: Mutex<BTreeMap<String, std::collections::BTreeSet<String>>>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    documents: Mutex<BTreeMap<String, Arc<clear_core::editing::OpenDocument>>>,
}

struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Backend {
    fn begin(&self) -> Result<Busy, String> {
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("Another operation is running".into());
        }
        self.cancel.store(false, Ordering::Relaxed);
        Ok(Busy(self.busy.clone()))
    }
    fn snapshot(&self, session: &str) -> Result<Arc<Snapshot>, String> {
        let snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "State unavailable")?
            .clone()
            .ok_or("Compare directories first")?;
        if snapshot.report.session != session {
            return Err("Comparison expired. Compare again.".into());
        }
        Ok(snapshot)
    }
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
async fn compare(
    request: CompareRequest,
    verify_matching: Option<bool>,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _operation_guard = state.begin()?;
    *state
        .table_document
        .lock()
        .map_err(|_| "State unavailable")? = None;
    *state.table.lock().map_err(|_| "State unavailable")? = None;
    // Invalidate the old plan before a new scan, even when that scan fails.
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        if verify_matching.unwrap_or(false) {
            clear_core::compare::compare_matching(&request, &cancel)
        } else {
            clear_core::compare::compare(&request, &cancel)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let report = &snapshot.report;
    let opened = OpenedComparison {
        session: report.session.clone(),
        left: report.left.clone(),
        right: report.right.clone(),
        summary: report.summary.clone(),
        warnings: report.warnings.clone(),
        page: Page {
            total: report.rows.len(),
            rows: report.rows.iter().take(500).cloned().collect(),
        },
    };
    *state.snapshot.lock().map_err(|_| "State unavailable")? = Some(Arc::new(snapshot));
    Ok(opened)
}

#[tauri::command]
fn set_text_merge_dirty(dirty: bool, state: State<'_, Arc<Backend>>) {
    state.text_merge_dirty.store(dirty, Ordering::Release);
}

#[tauri::command]
fn cancel(state: State<'_, Arc<Backend>>) {
    state.cancel.store(true, Ordering::Relaxed);
    if let Ok(slot) = state.operation.lock()
        && let Some(task) = slot.as_ref()
    {
        task.control.cancel();
    }
}

#[tauri::command]
async fn open_document(
    session: String,
    row_id: String,
    side: Side,
    state: State<'_, Arc<Backend>>,
) -> Result<TextDocument, String> {
    let _operation_guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let document = tauri::async_runtime::spawn_blocking(move || {
        clear_core::editing::open(&snapshot, &row_id, side)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = document.view.clone();
    let mut documents = state.documents.lock().map_err(|_| "State unavailable")?;
    if documents.len() >= 8 {
        return Err("Close an editor before opening more documents".into());
    }
    let tokens = clear_contracts::edit_history::conflict_tokens(&view.text);
    if !tokens.is_empty() {
        state
            .merge_guards
            .lock()
            .map_err(|_| "State unavailable")?
            .insert(view.id.clone(), tokens.into_iter().collect());
    }
    documents.insert(view.id.clone(), Arc::new(document));
    Ok(view)
}

#[tauri::command]
fn close_document(document_id: String, state: State<'_, Arc<Backend>>) -> Result<(), String> {
    state
        .merge_guards
        .lock()
        .map_err(|_| "State unavailable")?
        .remove(&document_id);
    state
        .editor_backups
        .lock()
        .map_err(|_| "State unavailable")?
        .remove(&document_id);
    *state.restore.lock().map_err(|_| "State unavailable")? = None;
    state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .remove(&document_id);
    Ok(())
}

#[tauri::command]
async fn save_document(
    document_id: String,
    text: String,
    options: SaveOptions,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SaveResult, String> {
    let _operation_guard = state.begin()?;
    let document = state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .get(&document_id)
        .cloned()
        .ok_or("Editor expired. Reopen the document.")?;
    let tokens = state
        .merge_guards
        .lock()
        .map_err(|_| "State unavailable")?
        .get(&document_id)
        .map(|s| s.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let backup = data_dir(&app)?.join("editor-backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let (updated, result) = tauri::async_runtime::spawn_blocking(move || {
        clear_core::editing::save_merge_aware(&document, &text, &backup, &options, &tokens)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let mut documents = state.documents.lock().map_err(|_| "State unavailable")?;
    documents.remove(&document_id);
    let mut backups = state
        .editor_backups
        .lock()
        .map_err(|_| "State unavailable")?;
    backups.remove(&document_id);
    backups.insert(updated.view.id.clone(), PathBuf::from(&result.backup));
    *state.restore.lock().map_err(|_| "State unavailable")? = None;
    {
        let mut guards = state.merge_guards.lock().map_err(|_| "State unavailable")?;
        if let Some(tokens) = guards.remove(&document_id) {
            guards.insert(updated.view.id.clone(), tokens);
        }
    }
    documents.insert(updated.view.id.clone(), Arc::new(updated));
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    Ok(result)
}

#[tauri::command]
async fn preview_editor_restore(
    document_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<RestorePreview, String> {
    let _operation_guard = state.begin()?;
    let document = state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .get(&document_id)
        .cloned()
        .ok_or("Editor expired")?;
    let backup = state
        .editor_backups
        .lock()
        .map_err(|_| "State unavailable")?
        .get(&document_id)
        .cloned()
        .ok_or("No recent save backup for this editor")?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::editing::preview_restore(&document, &backup)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = plan.view.clone();
    *state.restore.lock().map_err(|_| "State unavailable")? = Some((document_id, plan));
    Ok(view)
}
#[tauri::command]
async fn apply_editor_restore(
    plan_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SaveResult, String> {
    let _operation_guard = state.begin()?;
    let (document_id, plan) = state
        .restore
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview a restore first")?;
    if plan.view.id != plan_id {
        return Err("Restore preview expired".into());
    }
    if !state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .contains_key(&document_id)
    {
        return Err("Editor expired".into());
    }
    let backup = data_dir(&app)?.join("editor-backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let (updated, result) = tauri::async_runtime::spawn_blocking(move || {
        clear_core::editing::apply_restore(&plan, &backup)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let mut documents = state.documents.lock().map_err(|_| "State unavailable")?;
    documents.remove(&document_id);
    state
        .merge_guards
        .lock()
        .map_err(|_| "State unavailable")?
        .remove(&document_id);
    let mut backups = state
        .editor_backups
        .lock()
        .map_err(|_| "State unavailable")?;
    backups.remove(&document_id);
    backups.insert(updated.view.id.clone(), PathBuf::from(&result.backup));
    let tokens = clear_contracts::edit_history::conflict_tokens(&updated.view.text);
    if !tokens.is_empty() {
        state
            .merge_guards
            .lock()
            .map_err(|_| "State unavailable")?
            .insert(updated.view.id.clone(), tokens.into_iter().collect());
    }
    documents.insert(updated.view.id.clone(), Arc::new(updated));
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    Ok(result)
}

#[tauri::command]
fn page(session: String, offset: usize, state: State<'_, Arc<Backend>>) -> Result<Page, String> {
    let snapshot = state.snapshot(&session)?;
    Ok(Page {
        total: snapshot.report.rows.len(),
        rows: snapshot
            .report
            .rows
            .iter()
            .skip(offset)
            .take(500)
            .cloned()
            .collect(),
    })
}

#[tauri::command]
async fn query_folder(
    session: String,
    query: FolderQuery,
    offset: usize,
    expanded: Option<Vec<String>>,
    expand_all: Option<bool>,
    state: State<'_, Arc<Backend>>,
) -> Result<FolderPage, String> {
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(expanded) = expanded {
            clear_core::tree::outline(
                &snapshot,
                &query,
                &expanded,
                expand_all.unwrap_or(false),
                offset,
            )
        } else {
            clear_core::tree::query(&snapshot, &query, offset)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_report_destination(
    format: ReportFormat,
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (label, ext) = match format {
            ReportFormat::Html => ("HTML report", "html"),
            ReportFormat::Json => ("JSON report", "json"),
        };
        app.dialog()
            .file()
            .add_filter(label, &[ext])
            .set_file_name(format!("comparison-{}.{}", clear_core::unique_id(), ext))
            .blocking_save_file()
            .map(|p| p.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn export_report(
    request: ReportRequest,
    state: State<'_, Arc<Backend>>,
) -> Result<ReportResult, String> {
    let _guard = state.begin()?;
    let cancel = state.cancel.clone();
    let job: Box<dyn FnOnce() -> clear_core::Result<ReportResult> + Send> = match request.source {
        ReportSource::Local => {
            let snapshot = state.snapshot(&request.session)?;
            Box::new(move || {
                clear_core::report::export(
                    &snapshot.report,
                    &request,
                    &[snapshot.left.clone(), snapshot.right.clone()],
                    &cancel,
                )
            })
        }
        ReportSource::Archive => {
            let snapshot = archive_snapshot(&state, &request.session)?;
            Box::new(move || {
                clear_core::report::export(
                    &snapshot.report,
                    &request,
                    &snapshot.report_roots(),
                    &cancel,
                )
            })
        }
        ReportSource::Git => {
            let snapshot = git_snapshot(&state, &request.session)?;
            Box::new(move || {
                clear_core::report::export(
                    &snapshot.report,
                    &request,
                    &snapshot.report_roots(),
                    &cancel,
                )
            })
        }
        ReportSource::TreeSnapshot => {
            let snapshot = state
                .tree_snapshot
                .lock()
                .map_err(|_| "State unavailable")?
                .clone()
                .ok_or("Open a tree comparison first")?;
            if snapshot.report.session != request.session {
                return Err("Tree snapshot comparison expired".into());
            }
            Box::new(move || {
                clear_core::report::export(
                    &snapshot.report,
                    &request,
                    &snapshot.report_roots(),
                    &cancel,
                )
            })
        }
    };
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compare_git(
    request: GitCompareRequest,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _guard = state.begin()?;
    *state.git.lock().map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot =
        tauri::async_runtime::spawn_blocking(move || clear_core::git::compare(&request, &cancel))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    let view = OpenedComparison {
        session: snapshot.report.session.clone(),
        left: snapshot.report.left.clone(),
        right: snapshot.report.right.clone(),
        summary: snapshot.report.summary.clone(),
        warnings: snapshot.report.warnings.clone(),
        page: snapshot.page(0, "", false).map_err(|e| e.to_string())?,
    };
    *state.git.lock().map_err(|_| "State unavailable")? = Some(Arc::new(snapshot));
    Ok(view)
}
fn git_snapshot(state: &Backend, session: &str) -> Result<Arc<clear_core::git::Snapshot>, String> {
    let snapshot = state
        .git
        .lock()
        .map_err(|_| "State unavailable")?
        .clone()
        .ok_or("Compare Git revisions first")?;
    if snapshot.report.session != session {
        return Err("Git comparison expired".into());
    }
    Ok(snapshot)
}
#[tauri::command]
async fn git_page(
    session: String,
    offset: usize,
    search: String,
    differences_only: bool,
    state: State<'_, Arc<Backend>>,
) -> Result<Page, String> {
    let snapshot = git_snapshot(&state, &session)?;
    tauri::async_runtime::spawn_blocking(move || snapshot.page(offset, &search, differences_only))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn git_text_diff(
    session: String,
    row_id: String,
    options: TextOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<TextDiff, String> {
    let _guard = state.begin()?;
    let snapshot = git_snapshot(&state, &session)?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || snapshot.text_diff(&row_id, &options, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn capture_tree_snapshot(
    request: TreeSnapshotRequest,
    state: State<'_, Arc<Backend>>,
) -> Result<TreeSnapshotInfo, String> {
    let _guard = state.begin()?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::tree_snapshot::capture(&request, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn compare_tree_snapshots(
    request: ArchiveRequest,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _guard = state.begin()?;
    *state
        .tree_snapshot
        .lock()
        .map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        clear_core::tree_snapshot::compare(&request, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = OpenedComparison {
        session: snapshot.report.session.clone(),
        left: snapshot.report.left.clone(),
        right: snapshot.report.right.clone(),
        summary: snapshot.report.summary.clone(),
        warnings: snapshot.report.warnings.clone(),
        page: snapshot.page(0, "", false).map_err(|e| e.to_string())?,
    };
    *state
        .tree_snapshot
        .lock()
        .map_err(|_| "State unavailable")? = Some(Arc::new(snapshot));
    Ok(view)
}
#[tauri::command]
async fn tree_snapshot_page(
    session: String,
    offset: usize,
    search: String,
    differences_only: bool,
    state: State<'_, Arc<Backend>>,
) -> Result<Page, String> {
    let snapshot = state
        .tree_snapshot
        .lock()
        .map_err(|_| "State unavailable")?
        .clone()
        .ok_or("Open a tree snapshot first")?;
    if snapshot.report.session != session {
        return Err("Tree snapshot comparison expired".into());
    }
    tauri::async_runtime::spawn_blocking(move || snapshot.page(offset, &search, differences_only))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn pick_tree_snapshot(save: bool, app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app
            .dialog()
            .file()
            .add_filter("Clear Compare tree", &["cctree"]);
        let path = if save {
            dialog.set_file_name("snapshot.cctree").blocking_save_file()
        } else {
            dialog.blocking_pick_file()
        };
        path.map(|p| p.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compare_archives(
    request: ArchiveRequest,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _operation_guard = state.begin()?;
    *state.archive.lock().map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        clear_core::archive::compare(&request, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = OpenedComparison {
        session: snapshot.report.session.clone(),
        left: snapshot.report.left.clone(),
        right: snapshot.report.right.clone(),
        summary: snapshot.report.summary.clone(),
        warnings: snapshot.report.warnings.clone(),
        page: snapshot.page(0, "", false).map_err(|e| e.to_string())?,
    };
    *state.archive.lock().map_err(|_| "State unavailable")? = Some(Arc::new(snapshot));
    Ok(view)
}
fn archive_snapshot(
    state: &Backend,
    session: &str,
) -> Result<Arc<clear_core::archive::Snapshot>, String> {
    let snapshot = state
        .archive
        .lock()
        .map_err(|_| "State unavailable")?
        .clone()
        .ok_or("Compare archives first")?;
    if snapshot.report.session != session {
        return Err("Archive comparison expired".into());
    }
    Ok(snapshot)
}
#[tauri::command]
async fn archive_page(
    session: String,
    offset: usize,
    search: String,
    differences_only: bool,
    state: State<'_, Arc<Backend>>,
) -> Result<Page, String> {
    let snapshot = archive_snapshot(&state, &session)?;
    tauri::async_runtime::spawn_blocking(move || snapshot.page(offset, &search, differences_only))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn archive_text_diff(
    session: String,
    row_id: String,
    options: TextOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<TextDiff, String> {
    let _operation_guard = state.begin()?;
    let snapshot = archive_snapshot(&state, &session)?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || snapshot.text_diff(&row_id, &options, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn pick_archive(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("ZIP", &["zip"])
            .blocking_pick_file()
            .map(|p| p.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prepare_folder_merge(
    request: FolderMergeRequest,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<FolderMergePreview, String> {
    let _operation_guard = state.begin()?;
    *state.folder_merge.lock().map_err(|_| "State unavailable")? = None;
    let backup = data_dir(&app)?.join("merge-runs");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::folder_merge::prepare(&request, &backup, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = plan.preview(0);
    *state.folder_merge.lock().map_err(|_| "State unavailable")? = Some(plan);
    Ok(view)
}
#[tauri::command]
fn folder_merge_page(
    plan_id: String,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<FolderMergePreview, String> {
    let guard = state.folder_merge.lock().map_err(|_| "State unavailable")?;
    let plan = guard.as_ref().ok_or("Preview folder merge first")?;
    let view = plan.preview(offset);
    if view.id != plan_id {
        return Err("Folder merge preview expired".into());
    }
    Ok(view)
}
#[tauri::command]
fn resolve_folder_merge(
    plan_id: String,
    row_id: String,
    resolution: FolderResolution,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<FolderMergePreview, String> {
    let _operation_guard = state.begin()?;
    let mut guard = state.folder_merge.lock().map_err(|_| "State unavailable")?;
    let plan = guard.as_mut().ok_or("Preview folder merge first")?;
    if plan.preview(0).id != plan_id {
        return Err("Folder merge preview expired".into());
    }
    plan.resolve(&row_id, resolution)
        .map_err(|e| e.to_string())?;
    Ok(plan.preview(offset))
}
#[tauri::command]
async fn apply_folder_merge(
    plan_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<FolderMergeResult, String> {
    let _operation_guard = state.begin()?;
    let plan = state
        .folder_merge
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview folder merge first")?;
    if plan.preview(0).id != plan_id {
        return Err("Folder merge preview expired".into());
    }
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || clear_core::folder_merge::execute(&plan, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn picture_preview(
    session: String,
    row_id: String,
    options: PictureOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<PicturePreview, String> {
    let _operation_guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::picture::preview(&snapshot, &row_id, &options, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn text_diff(
    session: String,
    row_id: String,
    options: TextOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<TextDiff, String> {
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::text::preview_with_options(&snapshot, &row_id, &options)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_table_document(
    session: String,
    row_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TableDocumentInfo, String> {
    let _guard = state.begin()?;
    *state
        .table_document
        .lock()
        .map_err(|_| "State unavailable")? = None;
    *state.table.lock().map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let doc = tauri::async_runtime::spawn_blocking(move || {
        clear_core::table::document::Document::open(&snapshot, &row_id, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let info = doc.info.clone();
    *state
        .table_document
        .lock()
        .map_err(|_| "State unavailable")? = Some(Arc::new(doc));
    Ok(info)
}
fn table_document(
    state: &Backend,
    id: &str,
) -> Result<Arc<clear_core::table::document::Document>, String> {
    let doc = state
        .table_document
        .lock()
        .map_err(|_| "State unavailable")?
        .clone()
        .ok_or("Open a table document first")?;
    if doc.info.id != id {
        return Err("Table document expired. Reopen it from the file list.".into());
    }
    Ok(doc)
}
#[tauri::command]
async fn table_catalog(
    document_id: String,
    left_html: bool,
    right_html: bool,
    state: State<'_, Arc<Backend>>,
) -> Result<TableDocumentInfo, String> {
    let _guard = state.begin()?;
    let doc = table_document(&state, &document_id)?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || doc.inspect(left_html, right_html, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn close_table_document(document_id: String, state: State<'_, Arc<Backend>>) -> Result<(), String> {
    let _guard = state.begin()?;
    let mut doc = state
        .table_document
        .lock()
        .map_err(|_| "State unavailable")?;
    if doc.as_ref().is_some_and(|d| d.info.id == document_id) {
        *doc = None;
        *state.table.lock().map_err(|_| "State unavailable")? = None;
    }
    Ok(())
}

#[tauri::command]
async fn table_compare(
    document_id: String,
    options: TableOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedTable, String> {
    let _operation_guard = state.begin()?;
    let document = table_document(&state, &document_id)?;
    *state.table.lock().map_err(|_| "State unavailable")? = None;
    let path = document.info.path.clone();
    let cancel = state.cancel.clone();
    let report = tauri::async_runtime::spawn_blocking(move || document.compare(&options, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let id = clear_core::unique_id();
    let total = report.rows.len();
    let view = TableComparison {
        column_map: report.column_map.clone(),
        headers_changed: report.headers_changed,
        columns: report.columns,
        options: report.options.clone(),
        left_tables: report.left_tables.clone(),
        right_tables: report.right_tables.clone(),
        warnings: report.warnings.clone(),
        left_headers: report.left_headers.clone(),
        right_headers: report.right_headers.clone(),
        summary: report.summary.clone(),
        rows: report.rows.iter().take(500).cloned().collect(),
    };
    *state.table.lock().map_err(|_| "State unavailable")? = Some((id.clone(), Arc::new(report)));
    Ok(OpenedTable {
        id,
        path,
        report: view,
        total,
    })
}
#[tauri::command]
fn table_page(
    table_id: String,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<TableRow>, String> {
    let cache = state.table.lock().map_err(|_| "State unavailable")?;
    let (id, report) = cache.as_ref().ok_or("Compare a table first")?;
    if id != &table_id {
        return Err("Table expired".into());
    }
    Ok(report.rows.iter().skip(offset).take(500).cloned().collect())
}

#[tauri::command]
async fn binary_page(
    session: String,
    row_id: String,
    offset: String,
    state: State<'_, Arc<Backend>>,
) -> Result<BinaryPage, String> {
    let _guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::binary::page(&snapshot, &row_id, &offset)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn binary_search(
    session: String,
    row_id: String,
    options: BinarySearchOptions,
    state: State<'_, Arc<Backend>>,
) -> Result<BinarySearchResult, String> {
    let _guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::binary::search(&snapshot, &row_id, &options, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn merge_buffers(
    base: String,
    left: String,
    right: String,
    state: State<'_, Arc<Backend>>,
) -> Result<MergeResult, String> {
    let _operation_guard = state.begin()?;
    let mut result = tauri::async_runtime::spawn_blocking(move || {
        clear_core::merge::three_way(&base, &left, &right)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let id = clear_core::unique_id();
    result.id = Some(id.clone());
    *state.latest_merge.lock().map_err(|_| "State unavailable")? =
        Some((id, result.conflict_token.clone()));
    Ok(result)
}

#[tauri::command]
fn stage_merge_result(
    document_id: String,
    result_id: String,
    text: String,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<String>, String> {
    let _operation_guard = state.begin()?;
    if text.len() > clear_contracts::edit_history::MAX_TEXT_BYTES || text.contains('\0') {
        return Err("Invalid merge text or decoded limit exceeded".into());
    }
    if !state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .contains_key(&document_id)
    {
        return Err("Editor expired".into());
    }
    let (id, token) = state
        .latest_merge
        .lock()
        .map_err(|_| "State unavailable")?
        .clone()
        .ok_or("Generate a merge result first")?;
    if id != result_id {
        return Err("Merge result expired. Generate again.".into());
    }
    let mut guards = state.merge_guards.lock().map_err(|_| "State unavailable")?;
    let tokens = guards.entry(document_id).or_default();
    if tokens.len() >= 2048 {
        return Err("Reopen the resolved editor to reset merge history".into());
    }
    if let Some(token) = token {
        tokens.insert(token);
    }
    Ok(tokens.iter().cloned().collect())
}

#[tauri::command]
async fn refresh_compared_file(
    session: String,
    row_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _guard = state.begin()?;
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let backend = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut stored = backend.snapshot.lock().map_err(|_| "State unavailable")?;
        let snapshot = stored
            .as_mut()
            .filter(|s| s.report.session == session)
            .ok_or("Comparison expired. Compare again.")?;
        let snapshot =
            Arc::get_mut(snapshot).ok_or("Comparison is still in use. Compare again.")?;
        clear_core::compare_refresh::file(snapshot, &row_id, &backend.cancel)
            .map_err(|e| e.to_string())?;
        let r = &snapshot.report;
        Ok(OpenedComparison {
            session: r.session.clone(),
            left: r.left.clone(),
            right: r.right.clone(),
            summary: r.summary.clone(),
            warnings: r.warnings.clone(),
            page: Page {
                total: r.rows.len(),
                rows: Vec::new(),
            },
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn preview_sync(
    session: String,
    row_ids: Vec<String>,
    source_side: Side,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    let _operation_guard = state.begin()?;
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let backup = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::plan_direction(&snapshot, &row_ids, &backup, &cancel, source_side)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.plan.lock().map_err(|_| "State unavailable")? = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn preview_delete(
    session: String,
    row_ids: Vec<String>,
    target_side: Side,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    let _guard = state.begin()?;
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let base = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::plan_delete(&snapshot, &row_ids, &base, &cancel, target_side)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.plan.lock().map_err(|_| "State unavailable")? = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn preview_mirror_files(
    session: String,
    source_side: Side,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    prepare_mirror(session, source_side, false, app, state).await
}

#[tauri::command]
async fn preview_mirror_tree(
    session: String,
    source_side: Side,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    prepare_mirror(session, source_side, true, app, state).await
}

async fn prepare_mirror(
    session: String,
    source_side: Side,
    tree: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    let _guard = state.begin()?;
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let base = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        if tree {
            clear_core::sync::plan_mirror_tree(&snapshot, &base, &cancel, source_side)
        } else {
            clear_core::sync::plan_mirror_files(&snapshot, &base, &cancel, source_side)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.plan.lock().map_err(|_| "State unavailable")? = Some(plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn preview_rename(
    session: String,
    row_id: String,
    target_side: Side,
    new_name: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<SyncPlan, String> {
    let _guard = state.begin()?;
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let base = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::plan_rename(
            &snapshot,
            &row_id,
            &base,
            &cancel,
            target_side,
            std::ffi::OsStr::new(&new_name),
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.plan.lock().map_err(|_| "State unavailable")? = Some(plan.clone());
    Ok(plan)
}
async fn refresh_editor_locations(
    state: &Backend,
    moves: Vec<clear_core::editing::Relocation>,
    affected: Vec<PathBuf>,
) -> Result<(Vec<DocumentLocation>, Vec<String>, Vec<String>), String> {
    let documents = state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let changes = tauri::async_runtime::spawn_blocking(move || {
        let mut changes = Vec::new();
        for original in documents {
            let id = original.view.id.clone();
            if let Some(change) = moves
                .iter()
                .find(|r| original.has_path(&r.root.join(&r.from)))
            {
                match original.relocated(change) {
                    Ok(updated) => changes.push((original, Some(updated), false, None)),
                    Err(e) => changes.push((
                        original,
                        None,
                        true,
                        Some(format!(
                            "Editor location update failed; buffer is retained: {e}"
                        )),
                    )),
                }
            } else if affected.iter().any(|path| original.has_path(path))
                && !original.location_valid()
            {
                changes.push((
                    original,
                    None,
                    true,
                    Some(format!(
                        "Editor {id} source changed; its unsaved buffer is retained."
                    )),
                ));
            }
        }
        changes
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut cache = state.documents.lock().map_err(|_| "State unavailable")?;
    let mut locations = Vec::new();
    let mut stale = Vec::new();
    let mut warnings = Vec::new();
    for (old, updated, invalid, warning) in changes {
        if cache
            .get(&old.view.id)
            .is_none_or(|now| !Arc::ptr_eq(now, &old))
        {
            continue;
        }
        if let Some(next) = updated {
            locations.push(DocumentLocation {
                id: next.view.id.clone(),
                path: next.view.path.clone(),
                absolute_path: next.view.absolute_path.clone(),
            });
            cache.insert(next.view.id.clone(), Arc::new(next));
        }
        if invalid {
            stale.push(old.view.id.clone());
        }
        if let Some(warning) = warning {
            warnings.push(warning);
        }
    }
    Ok((locations, stale, warnings))
}

#[tauri::command]
async fn apply_sync(plan_id: String, state: State<'_, Arc<Backend>>) -> Result<TaskHandle, String> {
    let guard = state.begin()?;
    let plan = state
        .plan
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview a sync plan first")?;
    if plan.id != plan_id {
        return Err("Sync plan expired".into());
    }
    let snapshot = state.snapshot(&plan.session)?;
    operations::launch_file(state.inner().clone(), guard, plan, snapshot)
}

#[tauri::command]
async fn list_sync_history(app: tauri::AppHandle) -> Result<Vec<SyncHistoryEntry>, String> {
    let base = data_dir(&app)?.join("backups");
    tauri::async_runtime::spawn_blocking(move || {
        if !base.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for item in std::fs::read_dir(&base)
            .map_err(|e| e.to_string())?
            .take(2000)
        {
            let item = item.map_err(|e| e.to_string())?;
            if !item.file_type().map_err(|e| e.to_string())?.is_dir() {
                continue;
            }
            let Some(id) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !valid_history_id(&id) {
                continue;
            }
            let Ok(path) = clear_core::compare::checked_path(
                &base,
                &PathBuf::from(&id).join("journal.json"),
                false,
            ) else {
                continue;
            };
            let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let modified_ms = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis().to_string())
                .unwrap_or_default();
            entries.push(SyncHistoryEntry { id, modified_ms });
        }
        entries.sort_by(|a, b| b.id.cmp(&a.id));
        Ok(entries)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn valid_history_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
#[tauri::command]
async fn preview_sync_resume(
    history_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<ResumePreview, String> {
    let _guard = state.begin()?;
    *state.restart.lock().map_err(|_| "State unavailable")? = None;
    if !valid_history_id(&history_id) {
        return Err("Invalid history entry".into());
    }
    let base = data_dir(&app)?.join("backups");
    let journal = clear_core::compare::checked_path(
        &base,
        &PathBuf::from(history_id).join("journal.json"),
        false,
    )
    .map_err(|e| e.to_string())?;
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::resume::preview(&journal, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = plan.view.clone();
    *state.restart.lock().map_err(|_| "State unavailable")? = Some(plan);
    Ok(view)
}
#[tauri::command]
fn resume_page(
    plan_id: String,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<ResumeAction>, String> {
    let cached = state.restart.lock().map_err(|_| "State unavailable")?;
    let plan = cached
        .as_ref()
        .filter(|p| p.view.id == plan_id)
        .ok_or("Resume preview expired")?;
    Ok(plan.page(offset))
}
#[tauri::command]
fn apply_sync_resume(
    plan_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TaskHandle, String> {
    let guard = state.begin()?;
    let plan = state
        .restart
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview a restart first")?;
    if plan.view.id != plan_id {
        return Err("Resume preview expired".into());
    }
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    *state.recovery.lock().map_err(|_| "State unavailable")? = None;
    operations::launch_resume(state.inner().clone(), guard, plan)
}
#[tauri::command]
async fn preview_sync_recovery(
    history_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<RecoveryPreview, String> {
    let _operation_guard = state.begin()?;
    if !valid_history_id(&history_id) {
        return Err("Invalid history entry".into());
    }
    let data = data_dir(&app)?;
    let base = data.join("backups");
    let journal = clear_core::compare::checked_path(
        &base,
        &PathBuf::from(history_id).join("journal.json"),
        false,
    )
    .map_err(|e| e.to_string())?;
    let backup = data.join("recovery-backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::recovery::preview(&journal, &backup, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = plan.view.clone();
    *state.recovery.lock().map_err(|_| "State unavailable")? = Some(plan);
    Ok(view)
}
#[tauri::command]
async fn apply_sync_recovery(
    plan_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TaskHandle, String> {
    let guard = state.begin()?;
    let plan = state
        .recovery
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview recovery first")?;
    if plan.view.id != plan_id {
        return Err("Recovery preview expired".into());
    }
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    operations::launch_recovery(state.inner().clone(), guard, plan)
}

#[tauri::command]
async fn pick_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| path.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn load_session(app: tauri::AppHandle) -> Result<Option<SavedSession>, String> {
    clear_core::sessions::load_last(&data_dir(&app)?).map_err(|e| e.to_string())
}
#[tauri::command]
fn save_session(session: SavedSession, app: tauri::AppHandle) -> Result<(), String> {
    clear_core::sessions::save_last(&data_dir(&app)?, &session).map_err(|e| e.to_string())
}
#[tauri::command]
async fn load_active_project(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Option<LibraryEntry>, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || clear_core::sessions::active(&base))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn activate_project(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<LibraryEntry, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let entry = clear_core::sessions::load(&base, &id)?;
        clear_core::sessions::set_active(&base, Some(&id))?;
        Ok::<_, clear_core::Error>(entry)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn exclusion_for_entry(
    session: String,
    row_id: String,
    current: String,
    state: State<'_, Arc<Backend>>,
) -> Result<String, String> {
    let _guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    clear_core::rules::exclude_entry(&snapshot, &row_id, &current).map_err(|e| e.to_string())
}
#[tauri::command]
fn validate_exclusions(rules: String) -> Result<(), String> {
    clear_core::rules::Rules::parse(&rules)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_sessions(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<LibraryEntry>, String> {
    let _operation_guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || clear_core::sessions::list(&base))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn load_library_session(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<LibraryEntry, String> {
    let _operation_guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || clear_core::sessions::load(&base, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn save_library_session(
    id: Option<String>,
    revision: Option<String>,
    name: String,
    session: SavedSession,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<LibraryEntry, String> {
    let _operation_guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let entry =
            clear_core::sessions::save(&base, id.as_deref(), revision.as_deref(), &name, &session)?;
        clear_core::sessions::set_active(&base, Some(&entry.id))?;
        Ok::<_, clear_core::Error>(entry)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn archive_session(
    id: String,
    revision: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    let _operation_guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::sessions::archive(&base, &id, &revision)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_native_language(english: bool, app: tauri::AppHandle) -> Result<(), String> {
    native_menu::set(&app, english).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Backend::default()))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app|{native_menu::set(app.handle(),false)?;Ok(())})
        .invoke_handler(tauri::generate_handler![
            set_native_language,
            workbench::workspace_catalog,
            workbench::workspace_scan,
            workbench::workspace_open,
            workbench::workspace_favorite,
            workbench::workspace_reference_favorite,
            workbench::workspace_compare_references,
            workbench::workspace_stashes,
            workbench::workspace_remote_probe,
            workbench::workspace_worktrees,
            workbench::workspace_submodules,
            workbench::workspace_submodule_open,
            workbench::workspace_worktree_destination,
            workbench::workspace_worktree_open,
            workbench::workspace_worktree_reveal,
            workbench::workspace_stash_detail,
            workbench::workspace_remove,
            workbench::workspace_prepare,
            workbench::workspace_apply,
            workbench::workspace_history,
            workbench::workspace_file_tree,
            workbench::workspace_file_info,
            workbench::workspace_working_diff,
            workbench::workspace_change_file,
            workbench::workspace_file_exclude,
            workbench::workspace_file_prepare,
            workbench::workspace_file_apply,
            workbench::workspace_file_reveal,
            workbench::workspace_file_settings,
            workbench::workspace_source_open,
            workbench::workspace_source_save,
            workbench::workspace_source_close,
            workbench::workspace_source_dirty,
            workbench::workspace_source_check,
            workbench::workspace_source_create,
            workbench::workspace_source_search,

            workbench::workspace_commit_detail,
            workbench::workspace_commit_children,
            workbench::workspace_commit_patch,
            compare,
            bidirectional::choose_baseline,
            bidirectional::prepare_bidirectional,
            bidirectional::bidirectional_page,
            bidirectional::bidirectional_recovery_page,
            bidirectional::resolve_bidirectional,
            bidirectional::apply_bidirectional,
            bidirectional::preview_bidirectional_recovery,
            bidirectional::apply_bidirectional_recovery,
            compare_archives,
            compare_git,
            export_report,
            pick_report_destination,
            git_page,
            git_text_diff,
            capture_tree_snapshot,
            compare_tree_snapshots,
            tree_snapshot_page,
            pick_tree_snapshot,
            archive_page,
            archive_text_diff,
            pick_archive,
            cancel,
            set_text_merge_dirty,
            page,
            query_folder,
            picture_preview,
            prepare_folder_merge,
            folder_merge_page,
            resolve_folder_merge,
            apply_folder_merge,
            text_diff,
            merge_buffers,
            stage_merge_result,
            binary_page,
            binary_search,
            table_compare,
            open_table_document,
            table_catalog,
            close_table_document,
            table_page,
            refresh_compared_file,
            preview_sync,
            preview_delete,
            preview_rename,
            preview_mirror_files,
            preview_mirror_tree,
            apply_sync,
            operations::active_operation,
            operations::operation_status,
            operations::pause_operation,
            operations::cancel_operation,
            operations::resume_operation,
            operations::take_operation_result,
            pick_directory,
            load_session,
            save_session,
            load_active_project,
            activate_project,
            exclusion_for_entry,
            validate_exclusions,
            list_sessions,
            load_library_session,
            save_library_session,
            archive_session,
            open_document,
            close_document,
            save_document,
            preview_editor_restore,
            apply_editor_restore,
            list_sync_history,
            preview_sync_recovery,
            preview_sync_resume,
            resume_page,
            apply_sync_resume,
            apply_sync_recovery
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && (window.state::<Arc<Backend>>().text_merge_dirty.load(Ordering::Acquire)||window.state::<Arc<Backend>>().workspace_dirty.load(Ordering::Acquire)) {
                api.prevent_close();
                window.app_handle().dialog().message("请先保存或放弃未保存的文件或合并，再关闭应用。 / Save or discard unsaved files and merges before closing.").title("未保存的内容 / Unsaved content").show(|_|{});
            }
        })
        .build(tauri::generate_context!()).expect("desktop runtime failed")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && (app.state::<Arc<Backend>>().text_merge_dirty.load(Ordering::Acquire)||app.state::<Arc<Backend>>().workspace_dirty.load(Ordering::Acquire)) {
                api.prevent_exit();
                app.dialog().message("请先保存或放弃未保存的文件或合并，再退出应用。 / Save or discard unsaved files and merges before quitting.").title("未保存的内容 / Unsaved content").show(|_|{});
            }
        });
}
