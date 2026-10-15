//! One bounded background operation slot; task IDs are native and results consumed once.
use super::*;
pub(super) struct DesktopOperation {
    pub control: Arc<clear_core::task::Control>,
    result: Mutex<Option<OperationOutcome>>,
    consumed: AtomicBool,
    guard: Mutex<Option<Busy>>,
}
impl Drop for DesktopOperation {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl DesktopOperation {
    fn publish(&self, outcome: OperationOutcome, phase: TaskPhase, error: Option<&str>) {
        *self.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(outcome);
        self.control.finish(phase, error);
    }
}
fn start(
    state: &Backend,
    kind: OperationKind,
    total: usize,
    guard: Busy,
) -> Result<Arc<DesktopOperation>, String> {
    let mut slot = state
        .operation
        .lock()
        .map_err(|_| "Task state unavailable")?;
    if slot
        .as_ref()
        .is_some_and(|task| !task.consumed.load(Ordering::Acquire))
    {
        return Err("Collect the previous operation result before starting another".into());
    }
    let task = Arc::new(DesktopOperation {
        control: clear_core::task::Control::new(kind, total, state.cancel.clone()),
        result: Mutex::new(None),
        consumed: AtomicBool::new(false),
        guard: Mutex::new(Some(guard)),
    });
    *slot = Some(task.clone());
    Ok(task)
}
fn get(state: &Backend, id: &str) -> Result<Arc<DesktopOperation>, String> {
    state
        .operation
        .lock()
        .map_err(|_| "Task state unavailable")?
        .as_ref()
        .filter(|t| t.control.handle().id == id)
        .cloned()
        .ok_or_else(|| "Operation task expired".into())
}
fn phase(actions: &[ActionResult]) -> TaskPhase {
    if actions.iter().all(|a| a.state == "completed") {
        TaskPhase::Completed
    } else if actions.iter().any(|a| a.state == "cancelled") {
        TaskPhase::Cancelled
    } else {
        TaskPhase::Failed
    }
}
pub(super) fn launch_file(
    state: Arc<Backend>,
    guard: Busy,
    plan: SyncPlan,
    snapshot: Arc<Snapshot>,
) -> Result<TaskHandle, String> {
    let task = start(&state, OperationKind::File, plan.actions.len(), guard)?;
    let handle = task.control.handle();
    tauri::async_runtime::spawn(async move {
        let cancel = state.cancel.clone();
        let controller = task.control.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            controller.run(|| {
                let affected = clear_core::sync::affected_paths(&snapshot, &plan)?;
                let result = clear_core::sync::execute(&snapshot, &plan, &cancel)?;
                let moves = clear_core::sync::relocations(&snapshot, &result)?;
                Ok((result, moves, affected))
            })
        })
        .await;
        let (outcome, phase, error) = match result {
            Ok(Ok((result, moves, affected))) => {
                let phase = phase(&result.actions);
                let error = result.actions.iter().find_map(|a| a.error.clone());
                let (locations,stale_documents,warnings)=super::refresh_editor_locations(&state,moves,affected).await.unwrap_or_else(|e|(Vec::new(),Vec::new(),vec![format!("Operation finished but editor locations need review; buffers retained: {e}")]));
                (
                    OperationOutcome::File(FileApplyReply {
                        result,
                        locations,
                        stale_documents,
                        warnings,
                    }),
                    phase,
                    error,
                )
            }
            Ok(Err(e)) => {
                let phase = if matches!(e, clear_core::Error::Cancelled) {
                    TaskPhase::Cancelled
                } else {
                    TaskPhase::Failed
                };
                let e = e.to_string();
                (OperationOutcome::Error(e.clone()), phase, Some(e))
            }
            Err(e) => {
                let e = format!("Operation worker failed: {e}");
                (
                    OperationOutcome::Error(e.clone()),
                    TaskPhase::Failed,
                    Some(e),
                )
            }
        };
        task.publish(outcome, phase, error.as_deref());
    });
    Ok(handle)
}
pub(super) fn launch_recovery(
    state: Arc<Backend>,
    guard: Busy,
    plan: clear_core::recovery::Plan,
) -> Result<TaskHandle, String> {
    let task = start(
        &state,
        OperationKind::Recovery,
        plan.view.actions.len(),
        guard,
    )?;
    let handle = task.control.handle();
    tauri::async_runtime::spawn(async move {
        let cancel = state.cancel.clone();
        let controller = task.control.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            controller.run(|| {
                let affected = plan.affected_paths();
                let result = clear_core::recovery::execute(&plan, &cancel)?;
                let moves = plan.relocations(&result);
                Ok((result, moves, affected))
            })
        })
        .await;
        let (outcome, phase, error) = match result {
            Ok(Ok((result, moves, affected))) => {
                let phase = phase(&result.actions);
                let error = result.actions.iter().find_map(|a| a.error.clone());
                let (locations,stale_documents,warnings)=super::refresh_editor_locations(&state,moves,affected).await.unwrap_or_else(|e|(Vec::new(),Vec::new(),vec![format!("Recovery finished but editor locations need review; buffers retained: {e}")]));
                (
                    OperationOutcome::Recovery(RecoveryApplyReply {
                        result,
                        locations,
                        stale_documents,
                        warnings,
                    }),
                    phase,
                    error,
                )
            }
            Ok(Err(e)) => {
                let phase = if matches!(e, clear_core::Error::Cancelled) {
                    TaskPhase::Cancelled
                } else {
                    TaskPhase::Failed
                };
                let e = e.to_string();
                (OperationOutcome::Error(e.clone()), phase, Some(e))
            }
            Err(e) => {
                let e = format!("Recovery worker failed: {e}");
                (
                    OperationOutcome::Error(e.clone()),
                    TaskPhase::Failed,
                    Some(e),
                )
            }
        };
        task.publish(outcome, phase, error.as_deref());
    });
    Ok(handle)
}
#[tauri::command]
pub(super) fn active_operation(
    state: State<'_, Arc<Backend>>,
) -> Result<Option<TaskHandle>, String> {
    Ok(state
        .operation
        .lock()
        .map_err(|_| "Task state unavailable")?
        .as_ref()
        .filter(|t| !t.consumed.load(Ordering::Acquire))
        .map(|t| t.control.handle()))
}
#[tauri::command]
pub(super) fn operation_status(
    task_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<TaskProgress, String> {
    Ok(get(&state, &task_id)?.control.status())
}
#[tauri::command]
pub(super) fn pause_operation(
    task_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    get(&state, &task_id)?
        .control
        .pause()
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub(super) fn resume_operation(
    task_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    get(&state, &task_id)?
        .control
        .resume()
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub(super) fn cancel_operation(
    task_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<(), String> {
    get(&state, &task_id)?.control.cancel();
    Ok(())
}
#[tauri::command]
pub(super) fn take_operation_result(
    task_id: String,
    state: State<'_, Arc<Backend>>,
) -> Result<OperationOutcome, String> {
    take(&state, &task_id)
}
fn take(state: &Backend, task_id: &str) -> Result<OperationOutcome, String> {
    let task = get(state, task_id)?;
    if !task.control.status().phase.terminal() {
        return Err("Operation is still running".into());
    }
    let result = task
        .result
        .lock()
        .map_err(|_| "Task result unavailable")?
        .take()
        .ok_or("Operation result already collected")?;
    task.guard
        .lock()
        .map_err(|_| "Task guard unavailable")?
        .take();
    task.consumed.store(true, Ordering::Release);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operation_slot_retains_one_result_and_requires_the_exact_live_task_id() {
        let state = Backend::default();
        let guard = state.begin().unwrap();
        let task = start(&state, OperationKind::File, 1, guard).unwrap();
        let id = task.control.handle().id;
        assert!(state.begin().is_err());
        assert!(take(&state, &id).is_err());
        assert!(get(&state, "foreign").is_err());
        task.publish(
            OperationOutcome::Error("stopped".into()),
            TaskPhase::Failed,
            Some("stopped"),
        );
        assert!(state.begin().is_err());
        assert!(take(&state, "foreign").is_err());
        assert!(matches!(take(&state,&id).unwrap(),OperationOutcome::Error(e) if e=="stopped"));
        assert!(take(&state, &id).is_err());
        let guard = state.begin().unwrap();
        let next = start(&state, OperationKind::Recovery, 1, guard).unwrap();
        assert_ne!(next.control.handle().id, id);
        assert!(get(&state, &id).is_err());
    }
}
