#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clear_contracts::*;
use clear_core::compare::Snapshot;
use serde::{Deserialize, Serialize};
use std::{
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
    let guard = state.begin()?;
    // Invalidate the old plan before a new scan, even when that scan fails.
    *state.plan.lock().map_err(|_| "State unavailable")? = None;
    let cancel = state.cancel.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
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
    state: State<'_, Backend>,
) -> Result<TextDiff, String> {
    let snapshot = state.snapshot(&session)?;
    tauri::async_runtime::spawn_blocking(move || clear_core::text::preview(&snapshot, &row_id))
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
    let guard = state.begin()?;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let backup = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
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
    let cancel = state.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
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

#[derive(Serialize, Deserialize)]
struct SavedSession {
    request: CompareRequest,
    english: bool,
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
            preview_sync,
            apply_sync,
            pick_directory,
            load_session,
            save_session
        ])
        .run(tauri::generate_context!())
        .expect("desktop runtime failed");
}
