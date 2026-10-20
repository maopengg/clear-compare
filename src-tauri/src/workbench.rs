use super::*;
use clear_contracts::workbench::{
    Action, ActionPlan, ActionResult, Catalog, CommitChildren, CommitDetail, GitPatch, HistoryPage,
    HistoryQuery, RepositoryState, ScanSettings,
};
use tauri::Emitter;
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
pub async fn workspace_rebase_todo(
    repository_id: String,
    revision: String,
    upstream: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<clear_contracts::workbench::RebaseTodoItem>, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .interactive_rebase_todo(&repository_id, &revision, &upstream, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_commit_children(
    repository_id: String,
    detail_id: String,
    offset: usize,
    snapshot: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<CommitChildren, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .commit_children(
                &repository_id,
                &detail_id,
                offset,
                snapshot.as_deref(),
                &state.cancel,
            )
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
pub async fn workspace_source_replace_preview(
    request: clear_contracts::workbench::SourceReplaceRequest,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceReplacePreview, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .source_replace_preview(request, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_source_replace_patch(
    plan_id: String,
    file_id: String,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::GitPatch, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .source_replace_patch(&plan_id, &file_id, offset, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_source_replace_apply(
    plan_id: String,
    selected: Vec<String>,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SourceReplaceResult, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let events = app.clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .source_replace_apply_progress(
                &plan_id,
                &selected,
                &base.join("source-replacements"),
                &state.cancel,
                |progress| {
                    let _ = events.emit_to("main", "source-replace-progress", progress);
                },
            )
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

#[tauri::command]
pub async fn workspace_reference_favorite(
    repository_id: String,
    reference_id: String,
    favorite: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::ReferenceSettings, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .favorite_reference(
                &repository_id,
                &reference_id,
                favorite,
                &base,
                &state.cancel,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_compare_references(
    repository_id: String,
    revision: String,
    request: clear_contracts::workbench::ReferenceComparison,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<OpenedComparison, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = {
            let mut w = workspace(&state, &base)?;
            w.as_mut()
                .ok_or("Workspace unavailable")?
                .compare_references(&repository_id, &revision, &request, &state.cancel)
                .map_err(|e| e.to_string())?
        };
        let view = OpenedComparison {
            session: snapshot.report.session.clone(),
            left: snapshot.report.left.clone(),
            right: snapshot.report.right.clone(),
            summary: snapshot.report.summary.clone(),
            warnings: snapshot.report.warnings.clone(),
            page: snapshot.page(0, "", true).map_err(|e| e.to_string())?,
        };
        *state.git.lock().map_err(|_| "State unavailable")? = Some(Arc::new(snapshot));
        Ok(view)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_stashes(
    repository_id: String,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::StashPage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .stashes(&repository_id, offset, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_stash_detail(
    repository_id: String,
    stash_id: String,
    area: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::StashDetail, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .stash_detail(&repository_id, &stash_id, &area, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_remote_probe(
    repository_id: String,
    remote: String,
    push: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RemoteProbe, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .probe_remote(&repository_id, &remote, push, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_worktrees(
    repository_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorktreeList, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .worktrees(&repository_id, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_worktree_destination(
    repository_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Option<clear_contracts::workbench::WorktreeDestination>, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(chosen) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = chosen.into_path().map_err(|e| e.to_string())?;
        let path = path.to_str().ok_or("Worktree parent must be Unicode")?;
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .worktree_destination(&repository_id, path, &state.cancel)
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_worktree_open(
    repository_id: String,
    worktree_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorktreeOpen, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .open_worktree(&repository_id, &worktree_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_worktree_reveal(
    repository_id: String,
    worktree_id: String,
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
            .worktree_path(&repository_id, &worktree_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        let mut command = std::process::Command::new("open");
        #[cfg(windows)]
        let mut command = std::process::Command::new("explorer.exe");
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut command = std::process::Command::new("xdg-open");
        let status = command
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("System file manager could not open this worktree".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_submodules(
    repository_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::SubmoduleList, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .submodules(&repository_id, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn workspace_submodule_open(
    repository_id: String,
    submodule_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::WorktreeOpen, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        let w = w.as_mut().ok_or("Workspace unavailable")?;
        let result = w
            .open_submodule(&repository_id, &submodule_id, &state.cancel)
            .map_err(|e| e.to_string())?;
        w.save(&base).map_err(|e| e.to_string())?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_conflict_open(
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::ConflictDocument, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace(&state, &base)?
            .as_mut()
            .ok_or("Workspace unavailable")?
            .conflict_open(&repository_id, &file_id, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_revision_open(
    repository_id: String,
    commit: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RevisionTree, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .revision_open(&repository_id, &commit, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_revision_tree(
    repository_id: String,
    session: String,
    directory: Option<String>,
    offset: usize,
    filter: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RevisionTree, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .revision_tree(
                &repository_id,
                &session,
                directory.as_deref(),
                offset,
                &filter,
                &state.cancel,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_revision_content(
    repository_id: String,
    session: String,
    file_id: String,
    mode: String,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RevisionContent, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .revision_content(
                &repository_id,
                &session,
                &file_id,
                &mode,
                offset,
                &state.cancel,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_revision_local_diff(
    repository_id: String,
    session: String,
    file_id: String,
    options: clear_contracts::TextOptions,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RevisionLocalDiff, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .revision_local_diff(&repository_id, &session, &file_id, &options, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_history_open(
    source: clear_contracts::workbench::FileHistorySource,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FileHistoryPage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .file_history_open(source, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_history_page(
    history_id: String,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FileHistoryPage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .file_history_page(&history_id, offset, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_blame(
    history_id: String,
    target: clear_contracts::workbench::TraceTarget,
    offset: usize,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::FileBlamePage, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .file_blame(&history_id, &target, offset, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_history_version(
    history_id: String,
    target: clear_contracts::workbench::TraceTarget,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<clear_contracts::workbench::RevisionFocus, String> {
    let _guard = state.begin()?;
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .file_history_version(&history_id, &target, &state.cancel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workspace_file_history_close(
    history_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    let base = data_dir(&app)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut w = workspace(&state, &base)?;
        w.as_mut()
            .ok_or("Workspace unavailable")?
            .file_history_close(&history_id)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
