//! Reviewed native file-operation plans. Selection mode is not execution authorization.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    plan: RwSignal<Option<SyncPlan>>,
    comparison: RwSignal<Option<OpenedComparison>>,
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
            p.as_ref().is_some_and(|p| {
                matches!(
                    p.policy,
                    FilePlanPolicy::DeleteSelected { .. }
                        | FilePlanPolicy::MirrorFiles
                        | FilePlanPolicy::MirrorTree
                )
            })
        })
    };
    let mirroring = move || {
        plan.with(|p| {
            p.as_ref().is_some_and(|p| {
                matches!(
                    p.policy,
                    FilePlanPolicy::MirrorFiles | FilePlanPolicy::MirrorTree
                )
            })
        })
    };
    let tree_mirroring = move || {
        plan.with(|p| {
            p.as_ref()
                .is_some_and(|p| p.policy == FilePlanPolicy::MirrorTree)
        })
    };
    let renaming = move || {
        plan.with(|p| {
            p.as_ref()
                .is_some_and(|p| matches!(p.policy, FilePlanPolicy::RenameSelected { .. }))
        })
    };
    let source_label = move || {
        plan.with(|value| {
            let p = value.as_ref()?;
            if !matches!(
                p.policy,
                FilePlanPolicy::SelectedCopy
                    | FilePlanPolicy::MirrorFiles
                    | FilePlanPolicy::MirrorTree
            ) {
                return None;
            }
            comparison.with(|value| {
                let c = value.as_ref().filter(|c| c.session == p.session)?;
                Some(format!(
                    "{}: {}",
                    tr(english.get(), "源目录", "Source directory"),
                    if p.source_side == Side::Left {
                        &c.left
                    } else {
                        &c.right
                    }
                ))
            })
        })
    };
    view! {<Show when=move ||plan.with(|p|p.is_some())><div class="modal-backdrop"><section class="sync-dialog" class:deletion-plan=deletion role="dialog" aria-modal="true" aria-labelledby="sync-title">
        <h2 id="sync-title">{move ||if tree_mirroring(){tr(english.get(),"检查整个比较的目录镜像","Review whole-comparison tree mirror")}else if mirroring(){tr(english.get(),"检查整个比较的文件镜像","Review whole-comparison file mirror")}else if deletion(){tr(english.get(),"检查文件删除计划","Review file deletion")}else if renaming(){tr(english.get(),"检查文件重命名","Review file rename")}else{tr(english.get(),"检查复制计划","Review file copy")}}</h2>
        <p>{move ||plan.with(|p|p.as_ref().map(|p|match &p.policy {
            FilePlanPolicy::MirrorTree=>tr(english.get(),"镜像整个比较范围的普通文件与目录，包括创建空目录和移除清空后的目标独有目录。所有删除均有校验备份或目录元数据记录；排除项及其必需父目录保留。目录身份变更、链接和类型冲突阻止执行。","Mirror included regular files and directories across the entire comparison, including empty-directory creation/removal. Deletions have verified backups or directory metadata records. Exclusions and their required ancestors remain. Changed directory identities, links and type conflicts block execution."),
            FilePlanPolicy::MirrorFiles=>if p.source_side==Side::Right {tr(english.get(),"右侧为源，镜像普通文件到左侧。目标独有文件会备份后删除；空目录保留，排除项保留。范围包括整个比较，与显示筛选无关。","Mirror regular files from right to left. Destination-only files are backed up and deleted; empty directories and exclusions remain. Covers the entire comparison regardless of display filters.")}else{tr(english.get(),"左侧为源，镜像普通文件到右侧。目标独有文件会备份后删除；空目录保留，排除项保留。范围包括整个比较，与显示筛选无关。","Mirror regular files from left to right. Destination-only files are backed up and deleted; empty directories and exclusions remain. Covers the entire comparison regardless of display filters.")},
            FilePlanPolicy::Reconcile{..}=>tr(english.get(),"共同基线协调操作，按预览目标执行。","Common-baseline reconciliation for the reviewed target."),
            FilePlanPolicy::SelectedCopy=>if p.source_side==Side::Right {tr(english.get(),"从右侧复制到左侧，目标独有文件保留。覆盖前先验证备份。","Copy right to left; destination-only files remain. Backups are verified before overwrite.")}else{tr(english.get(),"从左侧复制到右侧，目标独有文件保留。覆盖前先验证备份。","Copy left to right; destination-only files remain. Backups are verified before overwrite.")},
            FilePlanPolicy::RenameSelected{target_side}=>if *target_side==Side::Left {tr(english.get(),"重命名左侧的一个普通文件，不覆盖已有名称。原始内容会先完成校验备份。","Rename one regular file on the left without overwriting an existing name. Original contents are backed up and verified first.")}else{tr(english.get(),"重命名右侧的一个普通文件，不覆盖已有名称。原始内容会先完成校验备份。","Rename one regular file on the right without overwriting an existing name. Original contents are backed up and verified first.")},
            FilePlanPolicy::DeleteSelected{target_side}=>if *target_side==Side::Left {tr(english.get(),"删除左侧选中的普通文件。每个文件会先完成校验备份；目录不会移除。","Delete selected regular files on the left. Each file is backed up and verified first; directories remain.")}else{tr(english.get(),"删除右侧选中的普通文件。每个文件会先完成校验备份；目录不会移除。","Delete selected regular files on the right. Each file is backed up and verified first; directories remain.")},
        }))}</p>
        <p class="backup-path">{source_label}</p>
        <p class="backup-path">{move ||plan.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"目标目录","Target directory"),p.target)))}</p>
        <p>{move ||plan.with(|p|p.as_ref().map(|p|format!("{} {}",p.actions.len(),tr(english.get(),"个动作","actions"))))}</p>
        <Show when=mirroring><p class="warnings">{move ||plan.with(|p|p.as_ref().map(|p|format!("{} {} · {} {}",tr(english.get(),"复制","Copy"),p.actions.iter().filter(|a|a.operation==FileOperation::Copy).count(),tr(english.get(),"备份后删除","Backup + delete"),p.actions.iter().filter(|a|a.operation==FileOperation::Delete).count())))}</p></Show>
        <Show when=tree_mirroring><p>{move ||plan.with(|p|p.as_ref().map(|p|format!("{} {} · {} {}",tr(english.get(),"创建目录","Create directories"),p.actions.iter().filter(|a|a.operation==FileOperation::CreateDirectory).count(),tr(english.get(),"移除空目录","Remove empty directories"),p.actions.iter().filter(|a|a.operation==FileOperation::RemoveDirectory).count())))}</p></Show>
        <Show when=move ||plan.with(|p|p.as_ref().is_some_and(|p|!p.warnings.is_empty()))><details><summary>{move ||tr(english.get(),"保留目录提示","Retained directories")}</summary><ul>{move ||plan.with(|p|p.as_ref().map(|p|p.warnings.iter().take(limit.get()).cloned().map(|note|view!{<li>{note}</li>}).collect_view()))}</ul></details></Show>
        <div class="plan-actions"><For each=move ||plan.with(|p|p.as_ref().map(|p|p.actions.iter().take(limit.get()).cloned().map(|a|(p.id.clone(),a)).collect::<Vec<_>>()).unwrap_or_default()) key=|(id,a)|(id.clone(),a.row_id.clone()) children=move |(_,a)|view! {<div><span>{a.path}{a.rename_to.map(|r|format!(" → {}",r.destination))}<small>{format!(" · {} bytes",a.bytes)}</small></span><strong>{move ||match a.operation {FileOperation::CreateDirectory=>tr(english.get(),"创建目录","Create directory"),FileOperation::RemoveDirectory=>tr(english.get(),"记录元数据后移除空目录","Record metadata + remove empty directory"),FileOperation::Rename=>tr(english.get(),"备份后重命名","Backup + rename"),FileOperation::Delete=>tr(english.get(),"备份后删除","Backup + delete"),FileOperation::Copy=>if a.target_existed {tr(english.get(),"覆盖并备份","Overwrite + backup")}else{tr(english.get(),"新增","Create")}}}</strong></div>}/></div>
        <Show when=move ||plan.with(|p|p.as_ref().is_some_and(|p|p.actions.len().max(p.warnings.len())>limit.get()))><button class="quiet" on:click=move |_|limit.update(|n|*n+=500)>{move ||tr(english.get(),"再显示 500 个动作","Show 500 more actions")}</button></Show>
        <p class="backup-path">{move ||plan.with(|p|p.as_ref().map(|p|format!("{}: {}",tr(english.get(),"备份位置","Backup location"),p.backup_root)))}</p>
        <footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|plan.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class=move ||if deletion(){"primary danger"}else{"primary"} disabled=move ||busy.get() on:click=move |_|apply()>{move ||if tree_mirroring(){tr(english.get(),"执行已预览的目录镜像","Execute reviewed tree mirror")}else if mirroring(){tr(english.get(),"执行已预览的文件镜像","Execute reviewed file mirror")}else if deletion(){tr(english.get(),"执行已预览的删除","Execute reviewed deletion")}else if renaming(){tr(english.get(),"执行已预览的重命名","Execute reviewed rename")}else{tr(english.get(),"执行已预览的复制","Execute reviewed copy")}}</button></footer>
    </section></div></Show>}
}
