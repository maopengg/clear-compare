use crate::{invoke, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    progress: RwSignal<Option<TaskProgress>>,
    reconnect: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let control = move |resume: bool| {
        if let Some(p) = progress.get_untracked() {
            spawn_local(async move {
                if let Err(e) = invoke::<()>(
                    if resume {
                        "resume_operation"
                    } else {
                        "pause_operation"
                    },
                    serde_json::json!({"taskId":p.handle.id}),
                )
                .await
                {
                    error.set(e);
                }
            });
        }
    };
    view! {<Show when=move ||progress.with(|p|p.is_some())><section class="progress" aria-label="File operation progress"><div role="status" aria-live="polite">
        <strong>{move ||progress.with(|p|p.as_ref().map(|p|match p.phase {
            TaskPhase::Running=>tr(english.get(),"正在执行文件任务","File operation running"),
            TaskPhase::PauseRequested=>tr(english.get(),"等待安全暂停点…","Waiting for a safe pause point…"),
            TaskPhase::Paused=>tr(english.get(),"已暂停","Paused"),
            TaskPhase::CancelRequested=>tr(english.get(),"正在取消并记录结果…","Cancelling and recording results…"),
            TaskPhase::Completed=>tr(english.get(),"文件任务已完成","File operation completed"),
            TaskPhase::Cancelled=>tr(english.get(),"文件任务已取消，已完成动作保留","Cancelled; completed changes remain"),
            TaskPhase::Failed=>tr(english.get(),"文件任务停止，请检查结果","Stopped; review operation results"),
        }))}</strong>
        <span>{move ||progress.with(|p|p.as_ref().map(|p|format!(" · {}/{} · {} {} B · {} {} B",p.completed_actions,p.total_actions,tr(english.get(),"读取（含校验）","Read (including verification)"),p.bytes_read,tr(english.get(),"写入（含备份）","Written (including backups)"),p.bytes_written)))}</span>
        <p>{move ||progress.with(|p|p.as_ref().and_then(|p|p.current_path.clone()))}</p>
        <Show when=move ||progress.with(|p|p.as_ref().is_some_and(|p|p.phase==TaskPhase::PauseRequested))><small>{move ||tr(english.get(),"最终校验、替换和日志写入完成后才能暂停。","Final validation, replacement and journaling finish before pausing.")}</small></Show>
    </div>
    <Show when=move ||progress.with(|p|p.as_ref().is_some_and(|p|matches!(p.phase,TaskPhase::Running)))><button class="quiet" on:click=move |_|control(false)>{move ||tr(english.get(),"暂停","Pause")}</button></Show>
    <Show when=move ||progress.with(|p|p.as_ref().is_some_and(|p|matches!(p.phase,TaskPhase::PauseRequested|TaskPhase::Paused)))><button class="quiet" on:click=move |_|control(true)>{move ||tr(english.get(),"继续","Continue")}</button></Show>
    <Show when=move ||progress.with(|p|p.as_ref().is_some_and(|p|!p.phase.terminal()))><button class="quiet" on:click=move |_|{if let Some(p)=progress.get_untracked(){spawn_local(async move{if let Err(e)=invoke::<()>("cancel_operation",serde_json::json!({"taskId":p.handle.id})).await{error.set(e);}});}}>{move ||tr(english.get(),"取消此任务","Cancel this task")}</button></Show>
    <button class="quiet" disabled=move ||busy.get() on:click=move |_|reconnect()>{move ||tr(english.get(),"重新连接任务","Reconnect task")}</button>
    </section></Show>}
}
