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
    start_file(state, guard, plan.actions.len(), move |cancel| {
        let affected = clear_core::sync::affected_paths(&snapshot, &plan)?;
        let result = clear_core::sync::execute(&snapshot, &plan, cancel)?;
        let moves = clear_core::sync::relocations(&snapshot, &result)?;
        Ok((result, moves, affected))
    })
}
pub(super) fn launch_resume(
    state: Arc<Backend>,
    guard: Busy,
    plan: clear_core::sync::resume::Plan,
) -> Result<TaskHandle, String> {
    let total = plan.view.pending + plan.view.satisfied + plan.view.cleanup_files;
    start_file(state, guard, total, move |cancel| {
        let result = clear_core::sync::resume::execute(&plan, cancel)?;
        let (affected, moves) = plan.effects(&result)?;
        Ok((result, moves, affected))
    })
}
type FileWork = (
    SyncResult,
    Vec<clear_core::editing::Relocation>,
    Vec<PathBuf>,
);
fn start_file(
    state: Arc<Backend>,
    guard: Busy,
    total: usize,
    work: impl FnOnce(&AtomicBool) -> clear_core::Result<FileWork> + Send + 'static,
) -> Result<TaskHandle, String> {
    let task = start(&state, OperationKind::File, total, guard)?;
    let handle = task.control.handle();
    tauri::async_runtime::spawn(async move {
        let cancel = state.cancel.clone();
        let controller = task.control.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || controller.run(|| work(&cancel))).await;
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

pub(super) enum BidirectionalWork {
    Apply(Box<clear_core::bidirectional::Plan>),
    Recover(Box<clear_core::bidirectional::RecoveryPlan>),
}
pub(super) fn launch_bidirectional(
    state: Arc<Backend>,
    guard: Busy,
    work: BidirectionalWork,
) -> Result<TaskHandle, String> {
    let (kind, total) = match &work {
        BidirectionalWork::Apply(plan) => (OperationKind::File, plan.view.actions + 1),
        BidirectionalWork::Recover(plan) => (OperationKind::Recovery, plan.view.total + 1),
    };
    let task = start(&state, kind, total, guard)?;
    let handle = task.control.handle();
    tauri::async_runtime::spawn(async move {
        enum Completed {
            Apply(Box<BidirectionalResult>, Vec<PathBuf>),
            Recover(BidirectionalRecoveryResult, Vec<PathBuf>),
        }
        let cancel = state.cancel.clone();
        let controller = task.control.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            controller.run(|| -> clear_core::Result<Completed> {
                match work {
                    BidirectionalWork::Apply(plan) => {
                        let affected = plan.affected_paths()?;
                        Ok(Completed::Apply(
                            Box::new(clear_core::bidirectional::execute(&plan, &cancel)?),
                            affected,
                        ))
                    }
                    BidirectionalWork::Recover(plan) => {
                        let affected = plan.affected_paths();
                        Ok(Completed::Recover(
                            clear_core::bidirectional::execute_recovery(&plan, &cancel)?,
                            affected,
                        ))
                    }
                }
            })
        })
        .await;
        let (outcome, phase, error) = match result {
            Ok(Ok(done)) => {
                let (affected, error, complete) = match &done {
                    Completed::Apply(r, p) => (
                        p.clone(),
                        r.error.clone(),
                        r.baseline_updated && r.error.is_none(),
                    ),
                    Completed::Recover(r, p) => (
                        p.clone(),
                        r.error.clone(),
                        r.baseline_restored && r.error.is_none(),
                    ),
                };
                let phase = if complete {
                    TaskPhase::Completed
                } else if state.cancel.load(Ordering::Relaxed) {
                    TaskPhase::Cancelled
                } else {
                    TaskPhase::Failed
                };
                let (locations, stale_documents, warnings) =
                    super::refresh_editor_locations(&state, Vec::new(), affected)
                        .await
                        .unwrap_or_else(|e| {
                            (
                                Vec::new(),
                                Vec::new(),
                                vec![format!(
                                    "Operation finished; review retained editor buffers: {e}"
                                )],
                            )
                        });
                let outcome = match done {
                    Completed::Apply(mut result, _) => {
                        compact_bidirectional(&mut result);
                        OperationOutcome::Bidirectional(BidirectionalApplyReply {
                            result: *result,
                            locations,
                            stale_documents,
                            warnings,
                        })
                    }
                    Completed::Recover(mut result, _) => {
                        result.details_in_journal = true;
                        for child in &mut result.operations {
                            child.actions.clear();
                            child.plan.actions.clear();
                        }
                        OperationOutcome::BidirectionalRecovery(BidirectionalRecoveryReply {
                            result,
                            locations,
                            stale_documents,
                            warnings,
                        })
                    }
                };
                (outcome, phase, error)
            }
            Ok(Err(e)) => {
                let phase = if matches!(e, clear_core::Error::Cancelled) {
                    TaskPhase::Cancelled
                } else {
                    TaskPhase::Failed
                };
                let error = e.to_string();
                (OperationOutcome::Error(error.clone()), phase, Some(error))
            }
            Err(e) => {
                let error = format!("Bidirectional worker failed: {e}");
                (
                    OperationOutcome::Error(error.clone()),
                    TaskPhase::Failed,
                    Some(error),
                )
            }
        };
        task.publish(outcome, phase, error.as_deref());
    });
    Ok(handle)
}

fn compact_bidirectional(result: &mut BidirectionalResult) {
    result.details_in_journal = true;
    for child in &mut result.operations {
        child.actions.clear();
        child.plan.actions.clear();
    }
}

#[cfg(test)]
mod bidirectional_wire_tests {
    use super::*;
    #[test]
    fn terminal_bidirectional_replies_are_small_but_keep_failure_and_journal_evidence() {
        let actions=(0..5000).map(|i|serde_json::json!({"row_id":i.to_string(),"path":format!("file-{i:04}-{}","x".repeat(120)),"target_existed":false,"bytes":"1","source_digest":"a".repeat(64),"target_digest":null})).collect::<Vec<_>>();
        let states = actions
            .iter()
            .map(|a| serde_json::json!({"path":a["path"],"state":"completed","error":null}))
            .collect::<Vec<_>>();
        let mut result:BidirectionalResult=serde_json::from_value(serde_json::json!({"baseline_info":{"path":"/example/common.ccbase","id":"a-b","revision":"1","entries":5000},"started_journals":["/example/child/journal.json"],"intent_digest":"a".repeat(64),"id":"a-b","journal":"/example/run.json","baseline":"/example/common.ccbase","baseline_updated":false,"phase":"failed","planned_journals":["/example/child/journal.json"],"operations":[{"plan":{"id":"child","session":"session","target":"/example/right","actions":actions,"backup_root":"/example/child"},"actions":states,"journal":"/example/child/journal.json"}],"error":"Source changed; baseline was not advanced"})).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() > 1024 * 1024);
        compact_bidirectional(&mut result);
        let packet = serde_json::to_vec(&result).unwrap();
        assert!(packet.len() < 8192);
        let received: BidirectionalResult = serde_json::from_slice(&packet).unwrap();
        assert!(received.details_in_journal);
        assert!(!received.baseline_updated);
        assert_eq!(received.phase, "failed");
        assert_eq!(
            received.error.as_deref(),
            Some("Source changed; baseline was not advanced")
        );
        assert_eq!(
            received.operations[0].journal,
            "/example/child/journal.json"
        );
        assert_eq!(received.baseline_info.entries, 5000);
    }
}
