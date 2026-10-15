use crate::{invoke, operations, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
pub struct Signals {
    pub baseline: RwSignal<Option<BaselineInfo>>,
    pub preview: RwSignal<Option<BidirectionalPreview>>,
    pub offset: RwSignal<usize>,
    pub run: RwSignal<Option<BidirectionalResult>>,
    pub recovery: RwSignal<Option<BidirectionalRecoveryPreview>>,
    pub recovery_offset: RwSignal<usize>,
    pub undo: RwSignal<Option<BidirectionalRecoveryResult>>,
}
impl Signals {
    pub fn new() -> Self {
        Self {
            baseline: RwSignal::new(None),
            preview: RwSignal::new(None),
            offset: RwSignal::new(0),
            run: RwSignal::new(None),
            recovery: RwSignal::new(None),
            recovery_offset: RwSignal::new(0),
            undo: RwSignal::new(None),
        }
    }
}
fn action(row: &BidirectionalRow, en: bool) -> String {
    let target = if row.winner == Some(Side::Left) {
        tr(en, "右侧", "right")
    } else {
        tr(en, "左侧", "left")
    };
    match row.action.as_str() {
        "copy" => format!("{} {target}", tr(en, "复制到", "Copy to")),
        "delete" => format!("{} {target}", tr(en, "备份后删除", "Back up + delete on")),
        "create_directory" => format!("{} {target}", tr(en, "创建目录于", "Create directory on")),
        "remove_directory" => format!(
            "{} {target}",
            tr(en, "移除清空目录于", "Remove emptied directory on")
        ),
        "keep_common" => tr(en, "保留共同状态", "Keep common state").into(),
        "review_parent_branch" => tr(en, "先处理父目录冲突", "Resolve parent branch first").into(),
        _ => tr(
            en,
            "需要选择或处理类型冲突",
            "Choose a winner or resolve type conflict",
        )
        .into(),
    }
}
fn values(row: &BidirectionalRow, en: bool) -> String {
    let label = |value: &Option<BidirectionalValue>| match value {
        None => tr(en, "不存在", "absent").to_owned(),
        Some(v) if v.kind == "directory" => tr(en, "目录", "directory").to_owned(),
        Some(v) => format!(
            "{} {}",
            tr(en, "文件", "file"),
            v.digest.as_deref().unwrap_or("?")
        ),
    };
    format!(
        "{}: {} · {}: {} · {}: {}",
        tr(en, "基线", "Base"),
        label(&row.baseline),
        tr(en, "左", "Left"),
        label(&row.left),
        tr(en, "右", "Right"),
        label(&row.right)
    )
}
#[allow(clippy::too_many_arguments)]
pub fn panel(
    en: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    comparison: RwSignal<Option<OpenedComparison>>,
    progress: RwSignal<Option<TaskProgress>>,
    s: Signals,
    complete: impl Fn(BidirectionalApplyReply) + Copy + Send + Sync + 'static,
    recovered: impl Fn(BidirectionalRecoveryReply) + Copy + Send + Sync + 'static,
    failed: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let choose = move |create: bool| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        s.preview.set(None);
        spawn_local(async move {
            match invoke::<Option<BaselineInfo>>(
                "choose_baseline",
                serde_json::json!({"session":c.session,"create":create}),
            )
            .await
            {
                Ok(Some(info)) => s.baseline.set(Some(info)),
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let prepare = move || {
        let (Some(c), Some(b)) = (comparison.get_untracked(), s.baseline.get_untracked()) else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        s.preview.set(None);
        s.offset.set(0);
        spawn_local(async move {
            match invoke::<BidirectionalPreview>(
                "prepare_bidirectional",
                serde_json::json!({"session":c.session,"baselineId":b.id}),
            )
            .await
            {
                Ok(view) => s.preview.set(Some(view)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let resolve = move |row: Option<String>, winner: Side| {
        let Some(p) = s.preview.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<BidirectionalPreview>(
                "resolve_bidirectional",
                serde_json::json!({"planId":p.id,"rowId":row,"winner":winner}),
            )
            .await
            {
                Ok(view) => {
                    s.preview.set(Some(view));
                    s.offset.set(0);
                }
                Err(e) => {
                    error.set(e);
                    s.preview.set(None);
                }
            }
            busy.set(false);
        });
    };
    let page = move |next: bool| {
        let Some(p) = s.preview.get_untracked() else {
            return;
        };
        let offset = if next {
            s.offset.get_untracked().saturating_add(500)
        } else {
            s.offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<Vec<BidirectionalRow>>(
                "bidirectional_page",
                serde_json::json!({"planId":p.id,"offset":offset}),
            )
            .await
            {
                Ok(rows) => {
                    s.preview.update(|v| {
                        if let Some(v) = v {
                            v.rows = rows;
                        }
                    });
                    s.offset.set(offset);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let apply = move || {
        let Some(p) = s.preview.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match operations::run::<BidirectionalApplyReply>(
                "apply_bidirectional",
                serde_json::json!({"planId":p.id}),
                progress,
            )
            .await
            {
                Ok(reply) => complete(reply),
                Err(e) => {
                    error.set(e);
                    failed();
                }
            }
            s.preview.set(None);
            busy.set(false);
        });
    };
    let recovery = move |id: Option<String>| {
        busy.set(true);
        error.set(String::new());
        s.recovery.set(None);
        s.recovery_offset.set(0);
        spawn_local(async move {
            match invoke::<Option<BidirectionalRecoveryPreview>>(
                "preview_bidirectional_recovery",
                serde_json::json!({"runId":id}),
            )
            .await
            {
                Ok(view) => s.recovery.set(view),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let recovery_page = move |next: bool| {
        let Some(p) = s.recovery.get_untracked() else {
            return;
        };
        let offset = if next {
            s.recovery_offset.get_untracked().saturating_add(500)
        } else {
            s.recovery_offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<Vec<BidirectionalRecoveryRow>>(
                "bidirectional_recovery_page",
                serde_json::json!({"planId":p.id,"offset":offset}),
            )
            .await
            {
                Ok(rows) => {
                    s.recovery.update(|v| {
                        if let Some(v) = v {
                            v.rows = rows;
                        }
                    });
                    s.recovery_offset.set(offset);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let undo = move || {
        let Some(p) = s.recovery.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match operations::run::<BidirectionalRecoveryReply>(
                "apply_bidirectional_recovery",
                serde_json::json!({"planId":p.id}),
                progress,
            )
            .await
            {
                Ok(reply) => recovered(reply),
                Err(e) => {
                    error.set(e);
                    failed();
                }
            }
            s.recovery.set(None);
            busy.set(false);
        });
    };
    view! {
    <details class="history-panel bidirectional-panel"><summary>{move||tr(en.get(),"双向同步与共同基线","Bidirectional sync and common baseline")}</summary>
    <p>{move||tr(en.get(),"先比较并校验内容。首次基线只能从相同的两侧目录建立；以后按基线识别独立修改与删除。冲突须明确选择保留侧，目录选择作用于整个分支。类型替换需先另行处理。","Compare with content verification first. Initialize a baseline only from equal trees; later runs identify independent edits and deletions. Choose conflict winners explicitly; directory choices affect the entire branch. Resolve type replacement separately.")}</p>
    <div class="pagination"><button class="quiet" disabled=move||busy.get()||comparison.with(|c|c.is_none()) on:click=move|_|choose(true)>{move||tr(en.get(),"建立共同基线…","Create common baseline…")}</button><button class="quiet" disabled=move||busy.get()||comparison.with(|c|c.is_none()) on:click=move|_|choose(false)>{move||tr(en.get(),"选择已有基线…","Select baseline…")}</button><button class="quiet" disabled=move||busy.get()||comparison.with(|c|c.is_none())||s.baseline.with(|b|b.is_none()) on:click=move|_|prepare()>{move||tr(en.get(),"预览双向同步","Preview bidirectional sync")}</button><button class="quiet" disabled=move||busy.get() on:click=move|_|recovery(None)>{move||tr(en.get(),"选择 run.json 预览协调回退…","Choose run.json to preview coordinated rollback…")}</button></div>
    <p class="backup-path">{move||s.baseline.with(|b|b.as_ref().map(|b|format!("{} · {} {} · {} {}",b.path,tr(en.get(),"修订","revision"),b.revision,b.entries,tr(en.get(),"项","entries")) ))}</p>
    <Show when=move||s.preview.with(|p|p.is_some())><section class="sync-dialog" aria-label="Bidirectional sync preview"><h3>{move||tr(en.get(),"核对两侧动作与冲突","Review actions and conflicts on both sides")}</h3><p>{move||s.preview.with(|p|p.as_ref().map(|p|format!("{} {} · {} {} · {} {}",p.actions,tr(en.get(),"个写入动作","writes"),p.conflicts,tr(en.get(),"个待选择冲突","unresolved conflicts"),p.blocked,tr(en.get(),"个类型阻断","blocked type conflicts"))))}</p>
    <p>{move||tr(en.get(),"每侧执行前会备份并校验。只有两侧都达到预览结果，基线才会更新；基线提交前取消或失败不会推进基线，已完成动作保留备份。","Each side uses verified backups and revalidation. Advance the baseline only after both trees match the reviewed result. Cancellation/failure before baseline commit retains the previous baseline and completed-action backups.")}</p>
    <div class="pagination"><button class="quiet" disabled=move||busy.get()||s.preview.with(|p|p.as_ref().is_none_or(|p|!p.can_prefer_left)) on:click=move|_|resolve(None,Side::Left)>{move||tr(en.get(),"所有冲突采用左侧","Use left for all conflicts")}</button><button class="quiet" disabled=move||busy.get()||s.preview.with(|p|p.as_ref().is_none_or(|p|!p.can_prefer_right)) on:click=move|_|resolve(None,Side::Right)>{move||tr(en.get(),"所有冲突采用右侧","Use right for all conflicts")}</button></div>
    <div class="plan-actions"><For each=move||s.preview.with(|p|p.as_ref().map(|p|p.rows.clone()).unwrap_or_default()) key=|r|format!("{}:{}:{}:{:?}",r.id,r.state,r.action,r.winner) children=move|r|{let id=r.id.clone();let right_id=id.clone();let conflict=matches!(r.state.as_str(),"conflict"|"resolved");let left=r.can_left;let right=r.can_right;let branch=r.subtree;let descriptor=r.clone();let label=move||action(&descriptor,en.get());let metadata=r.clone();let values=move||values(&metadata,en.get());view!{<div><span>{r.path}<small>{values}</small><small>{label}{move||if branch{tr(en.get()," · 整个分支"," · entire branch")}else{""}}</small></span>{conflict.then(||view!{<span><button class="quiet" disabled=move||busy.get()||!left on:click=move|_|resolve(Some(id.clone()),Side::Left)>{move||tr(en.get(),"保留左侧","Keep left")}</button><button class="quiet" disabled=move||busy.get()||!right on:click=move|_|resolve(Some(right_id.clone()),Side::Right)>{move||tr(en.get(),"保留右侧","Keep right")}</button></span>})}</div>}}/></div>
    <div class="pagination"><button class="quiet" disabled=move||busy.get()||s.offset.get()==0 on:click=move|_|page(false)>{move||tr(en.get(),"上一页","Previous")}</button><span>{move||s.preview.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",s.offset.get()+usize::from(p.total>0),(s.offset.get()+p.rows.len()).min(p.total),p.total)))}</span><button class="quiet" disabled=move||busy.get()||s.preview.with(|p|p.as_ref().is_none_or(|p|s.offset.get()+p.rows.len()>=p.total)) on:click=move|_|page(true)>{move||tr(en.get(),"下一页","Next")}</button></div>
    <p class="backup-path">{move||s.preview.with(|p|p.as_ref().map(|p|p.backup_root.clone()))}</p><button class="primary" disabled=move||busy.get()||s.preview.with(|p|p.as_ref().is_none_or(|p|p.conflicts+p.blocked>0)) on:click=move|_|apply()>{move||tr(en.get(),"执行已预览的双向同步","Execute reviewed bidirectional sync")}</button></section></Show>
    <Show when=move||s.recovery.with(|p|p.is_some())><section class="sync-dialog" aria-label="Bidirectional rollback preview"><h3>{move||tr(en.get(),"核对协调回退","Review coordinated rollback")}</h3><p class="backup-path">{move||s.recovery.with(|p|p.as_ref().map(|p|format!("{} · {}",p.journal,p.baseline)))}</p><p>{move||tr(en.get(),"回退两侧已应用动作，并恢复原基线。若基线已有更新的修订，将拒绝旧运行的回退。部分回退失败需保留全部日志手动检查。","Roll back applied actions on both sides and restore the previous baseline. A newer baseline revision blocks older rollback. Retain all journals for review if rollback only partly succeeds.")}</p><div class="plan-actions"><For each=move||s.recovery.with(|p|p.as_ref().map(|p|p.rows.clone()).unwrap_or_default()) key=|r|format!("{}:{}",r.target,r.path) children=move|r|view!{<div><span>{r.path}<small>{r.target}</small></span><strong>{r.operation}</strong></div>}/></div>
    <div class="pagination"><button class="quiet" disabled=move||busy.get()||s.recovery_offset.get()==0 on:click=move|_|recovery_page(false)>{move||tr(en.get(),"上一页","Previous")}</button><span>{move||s.recovery.with(|p|p.as_ref().map(|p|format!("{} / {}",s.recovery_offset.get()+p.rows.len(),p.total)))}</span><button class="quiet" disabled=move||busy.get()||s.recovery.with(|p|p.as_ref().is_none_or(|p|s.recovery_offset.get()+p.rows.len()>=p.total)) on:click=move|_|recovery_page(true)>{move||tr(en.get(),"下一页","Next")}</button><button class="primary" disabled=move||busy.get() on:click=move|_|undo()>{move||tr(en.get(),"执行已预览的协调回退","Execute reviewed coordinated rollback")}</button></div></section></Show>
    <button class="quiet" disabled=move||busy.get()||s.run.with(|r|r.is_none()) on:click=move|_|recovery(s.run.get_untracked().map(|r|r.id))>{move||tr(en.get(),"预览最近双向同步的协调回退","Preview latest bidirectional rollback")}</button>
    <Show when=move||s.run.with(|r|r.is_some())><p class="backup-path">{move||s.run.with(|r|r.as_ref().map(|r|format!("{} · {}",r.phase,r.journal)))}</p></Show><Show when=move||s.undo.with(|r|r.is_some())><p class="backup-path">{move||s.undo.with(|r|r.as_ref().map(|r|r.journal.clone()))}</p></Show>
    </details>
    }
}
