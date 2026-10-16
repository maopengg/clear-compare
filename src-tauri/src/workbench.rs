use super::*;
use clear_contracts::workbench::{
    Action, ActionPlan, ActionResult, Catalog, CommitDetail, GitPatch, HistoryPage, HistoryQuery,
    RepositoryState, ScanSettings,
};
fn workspace<'a>(
    state: &'a Backend,
    base: &std::path::Path,
) -> Result<std::sync::MutexGuard<'a, Option<clear_core::workbench::Workspace>>, String> {
    let mut workspace = state
        .workspace
        .lock()
        .map_err(|_| "Workspace unavailable")?;
    if workspace.is_none() {
        *workspace = Some(clear_core::workbench::Workspace::load(base).map_err(|e| e.to_string())?);
    }
    Ok(workspace)
}
#[tauri::command]
pub async fn workspace_history(
    repository_id: String,
    query: HistoryQuery,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<HistoryPage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .history(&repository_id, &query, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_commit_detail(
    repository_id: String,
    reference: String,
    parent_index: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<CommitDetail, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .commit_detail(&repository_id, &reference, parent_index, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_commit_patch(
    repository_id: String,
    detail_id: String,
    file_id: String,
    offset: Option<usize>,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<GitPatch, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let w = workspace(&state, &base)?;
        w.as_ref()
            .ok_or("Workspace unavailable")?
            .commit_patch_page(
                &repository_id,
                &detail_id,
                &file_id,
                offset.unwrap_or(0),
                &state.cancel,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_prepare(
    repository_id: String,
    revision: String,
    action: Action,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<ActionPlan, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .prepare(&repository_id, &revision, action, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_apply(
    plan_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<ActionResult, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .apply(&plan_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_catalog(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Catalog, String> {
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let w = workspace(&state, &base)?;
        {
            let mut catalog = w.as_ref().ok_or("Workspace unavailable")?.catalog.clone();
            catalog.projects = clear_core::sessions::list(&base).map_err(|e| e.to_string())?;
            Ok(catalog)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_scan(
    settings: ScanSettings,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Catalog, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w.scan(settings, &state.cancel).map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_open(
    repository_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<RepositoryState, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .open(&repository_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_favorite(
    repository_id: String,
    favorite: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Catalog, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .favorite(&repository_id, favorite)
            .map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_remove(
    repository_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Catalog, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w.remove(&repository_id).map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
