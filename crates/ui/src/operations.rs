//! Bounded status polling: the native worker owns progress and a single final result.
use crate::{invoke, operation_poll_delay};
use clear_contracts::*;
use leptos::prelude::*;
use serde::Serialize;
pub trait FromOutcome: Sized {
    fn from_outcome(value: OperationOutcome) -> Result<Self, String>;
}
impl FromOutcome for FileApplyReply {
    fn from_outcome(value: OperationOutcome) -> Result<Self, String> {
        match value {
            OperationOutcome::File(v) => Ok(v),
            OperationOutcome::Error(e) => Err(e),
            _ => Err("Unexpected recovery result".into()),
        }
    }
}
impl FromOutcome for RecoveryApplyReply {
    fn from_outcome(value: OperationOutcome) -> Result<Self, String> {
        match value {
            OperationOutcome::Recovery(v) => Ok(v),
            OperationOutcome::Error(e) => Err(e),
            _ => Err("Unexpected file-operation result".into()),
        }
    }
}
impl FromOutcome for BidirectionalApplyReply {
    fn from_outcome(value: OperationOutcome) -> Result<Self, String> {
        match value {
            OperationOutcome::Bidirectional(v) => Ok(v),
            OperationOutcome::Error(e) => Err(e),
            _ => Err("Unexpected bidirectional result".into()),
        }
    }
}
impl FromOutcome for BidirectionalRecoveryReply {
    fn from_outcome(value: OperationOutcome) -> Result<Self, String> {
        match value {
            OperationOutcome::BidirectionalRecovery(v) => Ok(v),
            OperationOutcome::Error(e) => Err(e),
            _ => Err("Unexpected coordinated rollback result".into()),
        }
    }
}
pub async fn run<T: FromOutcome>(
    command: &str,
    args: impl Serialize,
    progress: RwSignal<Option<TaskProgress>>,
) -> Result<T, String> {
    progress.set(None);
    let handle = invoke::<TaskHandle>(command, args).await?;
    T::from_outcome(follow(handle, progress).await?)
}
pub async fn follow(
    handle: TaskHandle,
    progress: RwSignal<Option<TaskProgress>>,
) -> Result<OperationOutcome, String> {
    let mut poll_ms = 16;
    loop {
        let next =
            invoke::<TaskProgress>("operation_status", serde_json::json!({"taskId":handle.id}))
                .await?;
        if next.handle.id != handle.id || next.handle.kind != handle.kind {
            return Err("Operation status identity changed".into());
        }
        let terminal = next.phase.terminal();
        progress.set(Some(next));
        if terminal {
            return invoke(
                "take_operation_result",
                serde_json::json!({"taskId":handle.id}),
            )
            .await;
        }
        wasm_bindgen_futures::JsFuture::from(operation_poll_delay(poll_ms))
            .await
            .map_err(|_| "Progress timer unavailable")?;
        poll_ms = (poll_ms * 2).min(250);
    }
}
