//! Persisted file-operation history and reviewed rollback.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    history: RwSignal<Option<Vec<SyncHistoryEntry>>>,
    recovery: RwSignal<Option<RecoveryPreview>>,
    preview: impl Fn(String) + Copy + Send + Sync + 'static,
    apply: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let limit = RwSignal::new(500usize);
    view! {
        <Show when=move ||history.get().is_some() && recovery.get().is_none()>
            <section class="history-panel"><header><strong>{move ||tr(english.get(),"文件操作历史","File operation history")}</strong><button class="quiet" on:click=move |_|history.set(None)>{move ||tr(english.get(),"关闭","Close")}</button></header>
            <p>{move ||tr(english.get(),"选择操作以检查恢复计划。可恢复覆盖或删除的文件；复制新增文件将先备份再移除。","Review recovery for an operation. Overwritten/deleted files can be restored; copied additions are backed up before removal.")}</p>
            <Show when=move ||history.get().is_some_and(|h|h.is_empty())><p>{move ||tr(english.get(),"暂无本机文件操作历史。","No local file-operation history yet.")}</p></Show>
            <div class="history-list"><For each=move ||history.get().unwrap_or_default() key=|e|e.id.clone() children=move |entry|{let id=entry.id.clone();let date=entry.modified_ms.parse::<f64>().ok().filter(|ms|ms.is_finite() && ms.abs()<=8.64e15).map(|ms|js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms)).to_iso_string().as_string().unwrap_or_default()).unwrap_or_default();view!{<div><span>{date}<br/>{entry.id}</span><button class="quiet" disabled=move ||busy.get() on:click=move |_|{limit.set(500);preview(id.clone());}>{move ||tr(english.get(),"预览恢复","Preview recovery")}</button></div>}}/></div>
            </section>
        </Show>
        <Show when=move ||recovery.get().is_some()>
            <div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="recovery-title"><h2 id="recovery-title">{move ||tr(english.get(),"检查文件恢复计划","Review file recovery")}</h2>
                <p>{move ||tr(english.get(),"恢复已应用的动作。已有文件发生修改、或删除路径已被重新创建时会停止；恢复前备份需要替换或移除的当前文件。","Recover applied actions. Modified files or recreated deletion targets stop recovery; current files are backed up before replacement/removal.")}</p>
                <p class="backup-path">{move ||recovery.get().map(|p|format!("{}: {}",tr(english.get(),"目标","Target"),p.target))}</p>
                <p class="backup-path">{move ||recovery.get().map(|p|format!("{}: {}",tr(english.get(),"恢复前备份","Recovery backup"),p.backup_root))}</p>
                <p>{move ||recovery.get().map(|p|format!("{} {}",p.actions.len(),tr(english.get(),"个动作","actions")))}</p>
                <div class="plan-actions"><For each=move ||recovery.get().map(|p|p.actions.into_iter().take(limit.get()).enumerate().collect::<Vec<_>>()).unwrap_or_default() key=|(i,_)|*i children=move |(_,a)|{let removal=a.operation=="remove_created";let deleted=a.operation=="restore_deleted";let renamed=a.operation=="rename_back";let remove_dir=a.operation=="remove_created_directory";let restore_dir=a.operation=="restore_removed_directory";view!{<div><span>{a.path}</span><strong>{move ||if remove_dir{tr(english.get(),"仅移除本次创建且仍为空的目录","Remove verified created directory only if empty")}else if restore_dir{tr(english.get(),"恢复目录与基本权限","Restore directory + basic permissions")}else if renamed{tr(english.get(),"回退重命名（不覆盖现有路径）","Reverse rename (no overwrite)")}else if removal{tr(english.get(),"备份后移除新增文件","Backup + remove created file")}else if deleted{tr(english.get(),"恢复已删除文件（不覆盖现有路径）","Restore deleted file (no overwrite)")}else{tr(english.get(),"备份后恢复旧内容","Backup + restore original")}}</strong></div>}}/></div>
                <Show when=move ||recovery.get().is_some_and(|p|p.actions.len()>limit.get())><button class="quiet" on:click=move |_|limit.update(|n|*n+=500)>{move ||tr(english.get(),"再显示 500 个动作","Show 500 more actions")}</button></Show>
                <footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|recovery.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move ||busy.get() on:click=move |_|apply()>{move ||tr(english.get(),"执行已预览的恢复","Execute reviewed recovery")}</button></footer>
            </section></div>
        </Show>
    }
}
