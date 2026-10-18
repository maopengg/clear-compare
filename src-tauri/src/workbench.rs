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

#[tauri::command]
pub async fn workspace_file_tree(
    repository_id: String,
    directory: Option<String>,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FilePage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .file_tree(&repository_id, directory.as_deref(), offset, &state.cancel)
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_file_settings(
    repository_id: String,
    settings: clear_contracts::workbench::FileSettings,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FileSettings, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .file_settings_persisted(&repository_id, settings, &base, &state.cancel)
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_open(
    repository_id: String,
    file_id: String,
    expected: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorkspaceDocument, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_open(&repository_id, &file_id, expected.as_deref(), &state.cancel)
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_save(
    document_id: String,
    text: String,
    options: clear_contracts::SaveOptions,
    generation: u64,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::SaveResult, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_save(
                &document_id,
                &text,
                &options,
                &base.join("source-backups"),
                generation,
                &state.cancel,
            )
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_close(
    document_id: String,
    discard: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        w.source_close(&document_id, discard)
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_dirty(
    document_id: String,
    dirty: bool,
    generation: u64,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    if dirty {
        state.workspace_dirty.store(true, Ordering::Release);
    }
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_dirty(&document_id, dirty, generation)
            .map_err(|e| e.to_string());
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_check(
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<clear_contracts::workbench::DocumentStatus>, String> {
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w.source_check(&state.cancel).map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_create(
    repository_id: String,
    directory: Option<String>,
    name: String,
    folder: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorkspaceFile, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_create(
                &repository_id,
                directory.as_deref(),
                &name,
                folder,
                &state.cancel,
            )
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_source_search(
    query: clear_contracts::workbench::SourceSearch,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceResults, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut workspace = workspace(&state, &base)?;
        let w = workspace.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_search(&query, &state.cancel)
            .map_err(|e| e.to_string())?;
        state
            .workspace_dirty
            .store(w.has_dirty_sources(), Ordering::Release);
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_info(
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceFileInfo, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .source_file_info(&repository_id, &file_id, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_file_prepare(
    repository_id: String,
    file_id: String,
    action: clear_contracts::workbench::SourceFileAction,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceFilePlan, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .source_file_prepare(&repository_id, &file_id, action, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
fn recycle_source_file(path: &std::path::Path) -> clear_core::Result<()> {
    let context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    let context = {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut context = context;
        context.set_delete_method(DeleteMethod::NsFileManager);
        context
    };
    context.delete(path).map_err(|e| {
        clear_core::Error::Invalid(format!(
            "System trash failed; no permanent-delete fallback: {e}"
        ))
    })
}
#[tauri::command]
pub async fn workspace_file_apply(
    plan_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceFileResult, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .source_file_apply(
                &plan_id,
                &base.join("file-operations"),
                &state.cancel,
                recycle_source_file,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_file_reveal(
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .source_file_path(&repository_id, &file_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = std::process::Command::new("open");
            c.arg("-R").arg(&path);
            c
        };
        #[cfg(windows)]
        let mut command = {
            let mut c = std::process::Command::new("explorer.exe");
            let mut argument = std::ffi::OsString::from("/select,");
            argument.push(&path);
            c.arg(argument);
            c
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut command = {
            let mut c = std::process::Command::new("xdg-open");
            c.arg(path.parent().ok_or("File parent unavailable")?);
            c
        };
        let status = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("System file manager could not reveal this item".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_exclude(
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FileSettings, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .source_file_exclude(&repository_id, &file_id, &base, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_working_diff(
    repository_id: String,
    file_id: String,
    area: String,
    diff_id: Option<String>,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorkingDiff, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .working_diff(
                &repository_id,
                &file_id,
                &area,
                diff_id.as_deref(),
                offset,
                &state.cancel,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_change_file(
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorkspaceFile, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .change_file(&repository_id, &file_id, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
