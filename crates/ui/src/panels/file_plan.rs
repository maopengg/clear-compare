//! Reviewed native copy/deletion plans. Selection mode is not execution authorization.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    plan: RwSignal<Option<SyncPlan>>,
    apply: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let limit = RwSignal::new(500usize);
    let identity = Memo::new(move |_| plan.with(|p| p.as_ref().map(|p| p.id.clone())));
    Effect::new(move |_| {
        identity.get();
        limit.set(500);
    });
    let deletion = move || {
        plan.with(|p| {
            p.as_ref()
                .is_some_and(|p| matches!(p.policy, FilePlanPolicy::DeleteSelected { .. }))
        })
    };
    let renaming = move || {
        plan.with(|p| {
            p.as_ref()
                .is_some_and(|p| matches!(p.policy, FilePlanPolicy::RenameSelected { .. }))
        })
    };
    view! {<Show when=move ||plan.with(|p|p.is_some())><div class="modal-backdrop"><section class="sync-dialog" class:deletion-plan=deletion role="dialog" aria-modal="true" aria-labelledby="sync-title">
        <h2 id="sync-title">{move ||if deletion(){tr(english.get(),"检查文件删除计划","Review file deletion")}else if renaming(){tr(english.get(),"检查文件重命名","Review file rename")}else{tr(english.get(),"检查复制计划","Review file copy")}}</h2>
        <p>{move ||plan.with(|p|p.as_ref().map(|p|match &p.policy {
            FilePlanPolicy::SelectedCopy=>if p.source_side==Side::Right {tr(english.get(),"从右侧复制到左侧，目标独有文件保留。覆盖前先验证备份。","Copy right to left; destination-only files remain. Backups are verified before overwrite.")}else{tr(english.get(),"从左侧复制到右侧，目标独有文件保留。覆盖前先验证备份。","Copy left to right; destination-only files remain. Backups are verified before overwrite.")},
            FilePlanPolicy::RenameSelected{target_side}=>if *target_side==Side::Left {tr(english.get(),"重命名左侧的一个普通文件，不覆盖已有名称。原始内容会先完成校验备份。","Rename one regular file on the left without overwriting an existing name. Original contents are backed up and verified first.")}else{tr(english.get(),"重命名右侧的一个普通文件，不覆盖已有名称。原始内容会先完成校验备份。","Rename one regular file on the right without overwriting an existing name. Original contents are backed up and verified first.")},
            FilePlanPolicy::DeleteSelected{target_side}=>if *target_side==Side::Left {tr(english.get(),"删除左侧选中的普通文件。每个文件会先完成校验备份；目录不会移除。","Delete selected regular files on the left. Each file is backed up and verified first; directories remain.")}else{tr(english.get(),"删除右侧选中的普通文件。每个文件会先完成校验备份；目录不会移除。","Delete selected regular files on the right. Each file is backed up and verified first; directories remain.")},
        }))}</p>
        <p class="backup-path">{move ||plan.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"目标目录","Target directory"),p.target)))}</p>
        <p>{move ||plan.with(|p|p.as_ref().map(|p|format!("{} {}",p.actions.len(),tr(english.get(),"个文件","files"))))}</p>
        <div class="plan-actions"><For each=move ||plan.with(|p|p.as_ref().map(|p|p.actions.iter().take(limit.get()).cloned().map(|a|(p.id.clone(),a)).collect::<Vec<_>>()).unwrap_or_default()) key=|(id,a)|(id.clone(),a.row_id.clone()) children=move |(_,a)|view! {<div><span>{a.path}{a.rename_to.map(|r|format!(" → {}",r.destination))}<small>{format!(" · {} bytes",a.bytes)}</small></span><strong>{move ||match a.operation {FileOperation::Rename=>tr(english.get(),"备份后重命名","Backup + rename"),FileOperation::Delete=>tr(english.get(),"备份后删除","Backup + delete"),FileOperation::Copy=>if a.target_existed {tr(english.get(),"覆盖并备份","Overwrite + backup")}else{tr(english.get(),"新增","Create")}}}</strong></div>}/></div>
        <Show when=move ||plan.with(|p|p.as_ref().is_some_and(|p|p.actions.len()>limit.get()))><button class="quiet" on:click=move |_|limit.update(|n|*n+=500)>{move ||tr(english.get(),"再显示 500 个动作","Show 500 more actions")}</button></Show>
        <p class="backup-path">{move ||plan.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"备份位置","Backup location"),p.backup_root)))}</p>
        <footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|plan.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class=move ||if deletion(){"primary danger"}else{"primary"} disabled=move ||busy.get() on:click=move |_|apply()>{move ||if deletion(){tr(english.get(),"执行已预览的删除","Execute reviewed deletion")}else if renaming(){tr(english.get(),"执行已预览的重命名","Execute reviewed rename")}else{tr(english.get(),"执行已预览的复制","Execute reviewed copy")}}</button></footer>
    </section></div></Show>}
}
