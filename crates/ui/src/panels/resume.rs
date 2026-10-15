use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    preview: RwSignal<Option<ResumePreview>>,
    offset: RwSignal<usize>,
    page: impl Fn(bool) + Copy + Send + Sync + 'static,
    apply: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {<Show when=move ||preview.with(|p|p.is_some())><div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="resume-title">
        <h2 id="resume-title">{move ||tr(english.get(),"检查同步续跑计划","Review operation restart")}</h2>
        <p>{move ||tr(english.get(),"沿用原操作的范围、方向与排除规则。已完成动作会验证当前状态与备份；剩余动作执行前重新校验。每次重试保存独立备份，已登记的残留临时文件会先保留内容再清理。","Keep the original scope, direction and exclusions. Verify completed actions and backups, revalidate remaining writes, and use independent retry backups. Registered leftover work files are retained before cleanup.")}</p>
        <p class="backup-path">{move ||preview.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"原比较另一目录","Other original comparison directory"),p.source)))}</p>
        <p class="backup-path">{move ||preview.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"执行目录","Operation directory"),p.target)))}</p>
        <p class="backup-path">{move ||preview.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"原日志","Original journal"),p.journal)))}</p>
        <p>{move ||preview.with(|p|p.as_ref().map(|p|format!("{} {} · {} {} · {} {}",tr(english.get(),"已验证完成","Verified complete"),p.satisfied,tr(english.get(),"待续跑","Remaining"),p.pending,tr(english.get(),"保留临时文件","Retain work files"),p.cleanup_files)))}</p>
        <div class="plan-actions"><For each=move ||preview.with(|p|p.as_ref().map(|p|p.actions.clone().into_iter().enumerate().collect::<Vec<_>>()).unwrap_or_default()) key=|(i,_)|*i children=move |(_,a)|view!{<div><span>{a.path}<small>{format!(" · {}",a.operation)}</small></span><strong>{move ||match a.state.as_str(){"verified_complete"=>tr(english.get(),"已验证完成","Verified complete"),"finish_staged_rename"=>tr(english.get(),"完成中断重命名","Finish staged rename"),"backup_then_remove_internal_temporary"=>tr(english.get(),"先保留内容再清理","Retain contents + cleanup"),_=>tr(english.get(),"重新校验后执行","Revalidate + execute")}}</strong></div>}/></div>
        <div class="pagination"><span>{move ||preview.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",offset.get()+usize::from(p.total>0),(offset.get()+p.actions.len()).min(p.total),p.total)))}</span><button class="quiet" disabled=move ||busy.get()||offset.get()==0 on:click=move |_|page(false)>{move ||tr(english.get(),"上一页","Previous")}</button><button class="quiet" disabled=move ||busy.get()||preview.with(|p|p.as_ref().is_none_or(|p|offset.get()+p.actions.len()>=p.total)) on:click=move |_|page(true)>{move ||tr(english.get(),"下一页","Next")}</button></div>
        <p class="backup-path">{move ||preview.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"本次备份目录","Retry backup directory"),p.backup_root)))}</p>
        <details><summary>{move ||tr(english.get(),"原生校验提示","Native validation notes")}</summary><p>{move ||preview.with(|p|p.as_ref().map(|p|p.warnings.join(" · ")))}</p></details>
        <footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|preview.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move ||busy.get() on:click=move |_|apply()>{move ||tr(english.get(),"执行已预览的续跑","Execute reviewed restart")}</button></footer>
    </section></div></Show>}
}
