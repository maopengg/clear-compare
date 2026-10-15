use super::*;
use clear_core::bidirectional as core;
#[tauri::command]
pub(super) async fn choose_baseline(
    session: String,
    create: bool,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Option<BaselineInfo>, String> {
    let _guard = state.begin()?;
    *state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let cancel = state.cancel.clone();
    let selection = tauri::async_runtime::spawn_blocking(
        move || -> Result<Option<(PathBuf, BaselineInfo)>, String> {
            let dialog = app
                .dialog()
                .file()
                .add_filter("Clear Compare baseline", &["ccbase", "json"]);
            let path = if create {
                dialog
                    .set_file_name("comparison.ccbase")
                    .blocking_save_file()
            } else {
                dialog.blocking_pick_file()
            };
            let Some(path) = path else {
                return Ok(None);
            };
            let path = path.into_path().map_err(|e| e.to_string())?;
            let info = if create {
                core::capture(&snapshot, &path, true, &cancel)
            } else {
                core::inspect(&snapshot, &path, &cancel)
            }
            .map_err(|e| e.to_string())?;
            let native_path = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
            Ok(Some((native_path, info)))
        },
    )
    .await
    .map_err(|e| e.to_string())??;
    if let Some((path, info)) = selection {
        *state.baseline.lock().map_err(|_| "State unavailable")? = Some((info.id.clone(), path));
        Ok(Some(info))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub(super) async fn prepare_bidirectional(
    session: String,
    baseline_id: String,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<BidirectionalPreview, String> {
    let _guard = state.begin()?;
    *state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")? = None;
    let snapshot = state.snapshot(&session)?;
    let path = state
        .baseline
        .lock()
        .map_err(|_| "State unavailable")?
        .as_ref()
        .filter(|(id, _)| *id == baseline_id)
        .map(|(_, p)| p.clone())
        .ok_or("Select a baseline first")?;
    let backup = data_dir(&app)?.join("backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        core::prepare(snapshot, &path, &backup, &cancel)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if plan.view.baseline.id != baseline_id {
        return Err("Baseline identity changed; select it again".into());
    }
    let view = plan.view.clone();
    *state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")? = Some(plan);
    Ok(view)
}
#[tauri::command]
pub(super) fn bidirectional_page(
    plan_id: String,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<BidirectionalRow>, String> {
    let guard = state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")?;
    let plan = guard
        .as_ref()
        .filter(|p| p.view.id == plan_id)
        .ok_or("Bidirectional preview expired")?;
    Ok(plan.page(offset))
}
#[tauri::command]
pub(super) async fn resolve_bidirectional(
    plan_id: String,
    row_id: Option<String>,
    winner: Side,
    state: State<'_, Arc<Backend>>,
) -> Result<BidirectionalPreview, String> {
    let _guard = state.begin()?;
    let mut plan = state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview bidirectional sync first")?;
    if plan.view.id != plan_id {
        return Err("Bidirectional preview expired".into());
    }
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || -> clear_core::Result<core::Plan> {
        if let Some(row) = row_id {
            plan.resolve(&row, winner, &cancel)?;
        } else {
            plan.prefer(winner, &cancel)?;
        }
        Ok(plan)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let view = plan.view.clone();
    *state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")? = Some(plan);
    Ok(view)
}
#[tauri::command]
pub(super) fn apply_bidirectional(
    plan_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TaskHandle, String> {
    let guard = state.begin()?;
    let plan = state
        .bidirectional
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview bidirectional sync first")?;
    if plan.view.id != plan_id {
        return Err("Bidirectional preview expired".into());
    }
    state.snapshot(plan.session())?;
    operations::launch_bidirectional(
        state.inner().clone(),
        guard,
        operations::BidirectionalWork::Apply(Box::new(plan)),
    )
}
#[tauri::command]
pub(super) async fn preview_bidirectional_recovery(
    run_id: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, Arc<Backend>>,
) -> Result<Option<BidirectionalRecoveryPreview>, String> {
    let _guard = state.begin()?;
    *state
        .bidirectional_recovery
        .lock()
        .map_err(|_| "State unavailable")? = None;
    let base = data_dir(&app)?;
    let backup = base.join("recovery-backups");
    std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
    let cancel = state.cancel.clone();
    let plan = tauri::async_runtime::spawn_blocking(
        move || -> Result<Option<core::RecoveryPlan>, String> {
            let path = if let Some(id) = run_id {
                if !valid_history_id(&id) {
                    return Err("Invalid bidirectional history ID".into());
                }
                clear_core::compare::checked_path(
                    &base.join("backups"),
                    &PathBuf::from(".bidirectional").join(id).join("run.json"),
                    false,
                )
                .map_err(|e| e.to_string())?
            } else {
                let chosen = app
                    .dialog()
                    .file()
                    .set_directory(base.join("backups/.bidirectional"))
                    .add_filter("Bidirectional run.json", &["json"])
                    .blocking_pick_file();
                let Some(chosen) = chosen else {
                    return Ok(None);
                };
                chosen.into_path().map_err(|e| e.to_string())?
            };
            core::preview_recovery(&path, &backup, &cancel)
                .map(Some)
                .map_err(|e| e.to_string())
        },
    )
    .await
    .map_err(|e| e.to_string())??;
    if let Some(plan) = plan {
        let view = plan.view.clone();
        *state
            .bidirectional_recovery
            .lock()
            .map_err(|_| "State unavailable")? = Some(plan);
        Ok(Some(view))
    } else {
        Ok(None)
    }
}
#[tauri::command]
pub(super) fn apply_bidirectional_recovery(
    plan_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TaskHandle, String> {
    let guard = state.begin()?;
    let plan = state
        .bidirectional_recovery
        .lock()
        .map_err(|_| "State unavailable")?
        .take()
        .ok_or("Preview bidirectional rollback first")?;
    if plan.view.id != plan_id {
        return Err("Bidirectional rollback expired".into());
    }
    operations::launch_bidirectional(
        state.inner().clone(),
        guard,
        operations::BidirectionalWork::Recover(Box::new(plan)),
    )
}

#[tauri::command]
pub(super) fn bidirectional_recovery_page(
    plan_id: String,
    offset: usize,
    state: State<'_, Arc<Backend>>,
) -> Result<Vec<BidirectionalRecoveryRow>, String> {
    let guard = state
        .bidirectional_recovery
        .lock()
        .map_err(|_| "State unavailable")?;
    let plan = guard
        .as_ref()
        .filter(|p| p.view.id == plan_id)
        .ok_or("Coordinated rollback preview expired")?;
    Ok(plan.page(offset))
}
