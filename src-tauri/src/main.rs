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
    plan: Mutex<Option<SyncPlan>>,
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
        clear_core::editing::save(&document, &text, &backup)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let mut documents = state.documents.lock().map_err(|_| "State unavailable")?;
    documents.remove(&document_id);
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
    app: tauri::AppHandle,
    state: State<'_, Backend>,
) -> Result<SyncPlan, String> {
    let _operation_guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let backup = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        clear_core::sync::plan(&snapshot, &row_ids, &backup, &cancel)
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
    let path = data_dir(&app)?.join("session.json");
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn save_session(session: SavedSession, app: tauri::AppHandle) -> Result<(), String> {
    let root = data_dir(&app)?;
    let temporary = root.join("session.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&session).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, root.join("session.json")).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .manage(Backend::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            compare,
            cancel,
            page,
            text_diff,
            merge_buffers,
            binary_page,
            preview_sync,
            apply_sync,
            pick_directory,
            load_session,
            save_session,
            open_document,
            close_document,
            save_document
        ])
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}
