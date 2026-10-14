#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    table: Mutex<Option<(String, Arc<TableComparison>)>>,
    plan: Mutex<Option<SyncPlan>>,
    editor_backups: Mutex<BTreeMap<String, PathBuf>>,
    restore: Mutex<Option<(String, clear_core::editing::RestorePlan)>>,
    recovery: Mutex<Option<clear_core::recovery::Plan>>,
    folder_merge: Mutex<Option<clear_core::folder_merge::Plan>>,
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
    state: State<'_, Backend>,
) -> Result<OpenedComparison, String> {
    let _operation_guard = state.begin()?;
    // Invalidate the old plan before a new scan, even when that scan fails.
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        clear_core::compare::compare(&request, &cancel)
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
fn cancel(state: State<'_, Backend>) {
    state.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn open_document(
    session: String,
    row_id: String,
    side: Side,
    state: State<'_, Backend>,
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
    documents.insert(view.id.clone(), Arc::new(document));
    Ok(view)
}

#[tauri::command]
fn close_document(document_id: String, state: State<'_, Backend>) -> Result<(), String> {
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
    state: State<'_, Backend>,
) -> Result<SaveResult, String> {
    let _operation_guard = state.begin()?;
    let document = state
        .documents
        .lock()
        .map_err(|_| "State unavailable")?
        .get(&document_id)
        .cloned()
        .ok_or("Editor expired. Reopen the document.")?;
    let backup = data_dir(&app)?.join("editor-backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let (updated, result) = tauri::async_runtime::spawn_blocking(move || {
        clear_core::editing::save_with_options(&document, &text, &backup, &options)
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
    documents.insert(updated.view.id.clone(), Arc::new(updated));
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    Ok(result)
}

#[tauri::command]
async fn preview_editor_restore(
    document_id: String,
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    let mut backups = state
        .editor_backups
        .lock()
        .map_err(|_| "State unavailable")?;
    backups.remove(&document_id);
    backups.insert(updated.view.id.clone(), PathBuf::from(&result.backup));
    documents.insert(updated.view.id.clone(), Arc::new(updated));
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    Ok(result)
}

#[tauri::command]
fn page(session: String, offset: usize, state: State<'_, Backend>) -> Result<Page, String> {
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
    state: State<'_, Backend>,
) -> Result<FolderPage, String> {
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || clear_core::tree::query(&snapshot, &query, offset))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn prepare_folder_merge(
    request: FolderMergeRequest,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
async fn table_compare(
    session: String,
    row_id: String,
    options: TableOptions,
    state: State<'_, Backend>,
) -> Result<OpenedTable, String> {
    let _operation_guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let (path, report) = tauri::async_runtime::spawn_blocking(move || {
        let (path, left, right) = clear_core::text::load_pair(&snapshot, &row_id)?;
        Ok::<_, clear_core::Error>((path, clear_core::table::compare(&left, &right, &options)?))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let id = clear_core::unique_id();
    let total = report.rows.len();
    let view = TableComparison {
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
) -> Result<BinaryPage, String> {
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::binary::page(&snapshot, &row_id, &offset)
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
    state: State<'_, Backend>,
) -> Result<MergeResult, String> {
    let _operation_guard = state.begin()?;
    tauri::async_runtime::spawn_blocking(move || clear_core::merge::three_way(&base, &left, &right))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn preview_sync(
    session: String,
    row_ids: Vec<String>,
    source_side: Side,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
) -> Result<SyncPlan, String> {
    let _operation_guard = state.begin()?;
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
async fn apply_sync(plan_id: String, state: State<'_, Backend>) -> Result<SyncResult, String> {
    let _operation_guard = state.begin()?;
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
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::execute(&snapshot, &plan, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
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
async fn preview_sync_recovery(
    history_id: String,
    app: tauri::AppHandle,
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
) -> Result<RecoveryResult, String> {
    let _operation_guard = state.begin()?;
    let plan = state
        .recovery
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview recovery first")?;
    if plan.view.id != plan_id {
        return Err("Recovery preview expired".into());
    }
    let cancel = state.cancel.clone();
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    tauri::async_runtime::spawn_blocking(move || clear_core::recovery::execute(&plan, &cancel))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
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
async fn list_sessions(
    app: tauri::AppHandle,
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
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
    state: State<'_, Backend>,
) -> Result<LibraryEntry, String> {
    let _operation_guard = state.begin()?;
    let base = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        clear_core::sessions::save(&base, id.as_deref(), revision.as_deref(), &name, &session)
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
    state: State<'_, Backend>,
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

fn main() {
    tauri::Builder::default()
        .manage(Backend::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            compare,
            cancel,
            page,
            query_folder,
            picture_preview,
            prepare_folder_merge,
            folder_merge_page,
            resolve_folder_merge,
            apply_folder_merge,
            text_diff,
            merge_buffers,
            binary_page,
            table_compare,
            table_page,
            preview_sync,
            apply_sync,
            pick_directory,
            load_session,
            save_session,
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
            apply_sync_recovery
        ])
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}
