//! File-tree context actions. Paths and writes remain native capabilities.
use super::*;
#[wasm_bindgen(inline_js = "
let sourceMenuAnchor;
export function sourceMenuPosition(event,keyboard){sourceMenuAnchor=event.currentTarget;const r=sourceMenuAnchor.getBoundingClientRect();return [keyboard?r.left:event.clientX,keyboard?r.bottom:event.clientY];}
export function sourceMenuFocus(){requestAnimationFrame(()=>{const m=document.querySelector('.source-context-menu');if(!m)return;const r=m.getBoundingClientRect();m.style.left=Math.max(4,Math.min(r.left,innerWidth-r.width-4))+'px';m.style.top=Math.max(4,Math.min(r.top,innerHeight-r.height-4))+'px';m.querySelector('button:not(:disabled)')?.focus();});}
export function sourceMenuRestore(){if(sourceMenuAnchor?.isConnected)sourceMenuAnchor.focus();}
export function sourceMenuKey(event){const b=[...event.currentTarget.querySelectorAll('button:not(:disabled)')];const i=b.indexOf(document.activeElement);let n;if(event.key==='ArrowDown')n=(i+1)%b.length;else if(event.key==='ArrowUp')n=(i+b.length-1)%b.length;else if(event.key==='Home')n=0;else if(event.key==='End')n=b.length-1;else return;event.preventDefault();b[n]?.focus();}
export async function sourceCopy(text){await navigator.clipboard.writeText(text);}
")]
extern "C" {
    #[wasm_bindgen(js_name=sourceMenuPosition)]
    fn position(event: &JsValue, keyboard: bool) -> js_sys::Array;
    #[wasm_bindgen(js_name=sourceMenuFocus)]
    fn focus();
    #[wasm_bindgen(js_name=sourceMenuRestore)]
    fn restore();
    #[wasm_bindgen(js_name=sourceMenuKey)]
    fn keypress(event: &JsValue);
    #[wasm_bindgen(js_name=sourceCopy)]
    fn copy(text: &str) -> js_sys::Promise;
}
#[derive(Clone)]
pub(super) struct Target {
    repository: String,
    file: WorkspaceFile,
    x: i32,
    y: i32,
}
#[derive(Clone)]
struct Draft {
    target: Target,
    operation: String,
    name: String,
    repository: String,
    directory: Option<String>,
    page: Option<FilePage>,
    plan: Option<SourceFilePlan>,
}
#[derive(Clone, Copy)]
pub(super) struct State {
    menu: RwSignal<Option<Target>>,
    draft: RwSignal<Option<Draft>>,
    notice: RwSignal<String>,
}
impl State {
    pub fn new() -> Self {
        Self {
            menu: RwSignal::new(None),
            draft: RwSignal::new(None),
            notice: RwSignal::new(String::new()),
        }
    }
    pub fn show(
        self,
        c: Controller,
        repository: String,
        file: WorkspaceFile,
        event: &JsValue,
        keyboard: bool,
    ) {
        if c.c.busy.get_untracked() {
            return;
        }
        let xy = position(event, keyboard);
        self.menu.set(Some(Target {
            repository,
            file,
            x: xy.get(0).as_f64().unwrap_or_default() as i32,
            y: xy.get(1).as_f64().unwrap_or_default() as i32,
        }));
        focus();
    }
    fn dismiss(self) {
        self.menu.set(None);
        restore();
    }
    fn action(self, s: Controller, target: Target, action: &str) {
        self.dismiss();
        if s.c.busy.get_untracked() {
            return;
        }
        if action == "open" {
            s.open(target.repository, target.file.id, None, None);
            return;
        }
        if ["rename", "copy", "move", "trash"].contains(&action) {
            self.draft.set(Some(Draft {
                repository: target.repository.clone(),
                name: target.file.name.clone(),
                target,
                operation: action.into(),
                directory: None,
                page: None,
                plan: None,
            }));
            if matches!(action, "copy" | "move") {
                self.directory(s, None);
            }
            return;
        }
        s.c.busy.set(true);
        s.c.error.set(String::new());
        let action = action.to_string();
        spawn_local(async move {
            if action == "exclude" {
                match invoke::<FileSettings>(
                    "workspace_file_exclude",
                    serde_json::json!({"repositoryId":target.repository,"fileId":target.file.id}),
                )
                .await
                {
                    Ok(settings) => {
                        s.c.catalog.update(|c| {
                            c.file_settings.insert(target.repository.clone(), settings);
                        });
                        s.tree.set(BTreeMap::new());
                        s.expanded.set(BTreeSet::new());
                        s.parent.set(None);
                        s.c.busy.set(false);
                        s.load(target.repository, None, 0);
                        return;
                    }
                    Err(e) => {
                        s.c.error.set(e);
                        s.c.busy.set(false);
                        return;
                    }
                }
            }
            let result = if action == "reveal" {
                invoke::<()>(
                    "workspace_file_reveal",
                    serde_json::json!({"repositoryId":target.repository,"fileId":target.file.id}),
                )
                .await
            } else {
                match invoke::<SourceFileInfo>(
                    "workspace_file_info",
                    serde_json::json!({"repositoryId":target.repository,"fileId":target.file.id}),
                )
                .await
                {
                    Ok(info) => {
                        let value = match action.as_str() {
                            "name" => info.name,
                            "relative" => info.relative_path,
                            _ => info.absolute_path,
                        };
                        wasm_bindgen_futures::JsFuture::from(copy(&value))
                            .await
                            .map(|_| ())
                            .map_err(|_| {
                                tr(
                                    s.english.get_untracked(),
                                    "无法写入剪贴板",
                                    "Could not write to clipboard",
                                )
                                .to_string()
                            })
                    }
                    Err(e) => Err(e),
                }
            };
            if let Err(e) = result {
                s.c.error.set(e);
            }
            s.c.busy.set(false);
        });
    }
    fn directory(self, s: Controller, directory: Option<String>) {
        self.directory_page(s, directory, 0);
    }
    fn directory_page(self, s: Controller, directory: Option<String>, offset: usize) {
        if s.c.busy.get_untracked() {
            return;
        }
        let Some(draft) = self.draft.get_untracked() else {
            return;
        };
        s.c.busy.set(true);
        s.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<FilePage>("workspace_file_tree",serde_json::json!({"repositoryId":draft.repository,"directory":directory,"offset":offset})).await{
                Ok(page)=>self.draft.update(|d|{if let Some(d)=d{d.directory=directory;d.page=Some(page);d.plan=None;}}),Err(e)=>s.c.error.set(e)
            }
            s.c.busy.set(false);
        });
    }
    fn prepare(self, s: Controller) {
        if s.c.busy.get_untracked() {
            return;
        }
        let Some(draft) = self.draft.get_untracked() else {
            return;
        };
        let action = match draft.operation.as_str() {
            "rename" => SourceFileAction::Rename { name: draft.name },
            "copy" => SourceFileAction::Copy {
                repository: draft.repository,
                directory: draft.directory,
            },
            "move" => SourceFileAction::Move {
                repository: draft.repository,
                directory: draft.directory,
            },
            _ => SourceFileAction::Trash,
        };
        s.c.busy.set(true);
        s.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SourceFilePlan>("workspace_file_prepare",serde_json::json!({"repositoryId":draft.target.repository,"fileId":draft.target.file.id,"action":action})).await{
                Ok(plan)=>self.draft.update(|d|{if let Some(d)=d{d.plan=Some(plan);}}),Err(e)=>s.c.error.set(e)
            }
            s.c.busy.set(false);
        });
    }
    fn apply(self, s: Controller) {
        if s.c.busy.get_untracked() {
            return;
        }
        let Some(plan) = self
            .draft
            .with_untracked(|d| d.as_ref().and_then(|d| d.plan.clone()))
        else {
            return;
        };
        s.c.busy.set(true);
        s.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SourceFileResult>(
                "workspace_file_apply",
                serde_json::json!({"planId":plan.id}),
            )
            .await
            {
                Ok(result) => {
                    self.notice.set(format!(
                        "{} · {}",
                        tr(
                            s.english.get_untracked(),
                            "操作完成，日志",
                            "Completed, journal"
                        ),
                        result.journal
                    ));
                    self.draft.set(None);
                    s.tree.set(BTreeMap::new());
                    s.expanded.set(BTreeSet::new());
                    s.parent.set(None);
                    s.results.set(None);
                    s.c.busy.set(false);
                    if let Some(repo) = s.c.selected.get_untracked() {
                        s.load(repo.repository.id, None, 0);
                    }
                    return;
                }
                Err(e) => {
                    s.c.error.set(e);
                    self.draft.update(|d| {
                        if let Some(d) = d {
                            d.plan = None;
                        }
                    });
                }
            }
            s.c.busy.set(false);
        });
    }
}
fn title(action: &str, en: bool) -> &'static str {
    match action {
        "open" => tr(en, "打开文件", "Open file"),
        "name" => tr(en, "复制文件名", "Copy name"),
        "relative" => tr(en, "复制相对路径", "Copy relative path"),
        "path" => tr(en, "复制完整路径", "Copy full path"),
        "reveal" => tr(en, "在系统文件管理器中显示", "Show in file manager"),
        "exclude" => tr(en, "排除此项", "Exclude this item"),
        "rename" => tr(en, "重命名…", "Rename…"),
        "copy" => tr(en, "复制到…", "Copy to…"),
        "move" => tr(en, "移动到…", "Move to…"),
        _ => tr(en, "移到废纸篓…", "Move to trash…"),
    }
}
pub(super) fn panel(s: Controller, m: State) -> impl IntoView {
    let english = s.english;
    view! {
        <Show when=move ||m.menu.with(|v|v.is_some())><div class="source-context-backdrop" on:pointerdown=move |_|m.dismiss() on:contextmenu=move |ev|{ev.prevent_default();m.dismiss();}>{move ||m.menu.get().map(|target|{
            let regular=matches!(target.file.kind.as_str(),"file"|"directory");
            view!{<div class="source-context-menu" role="menu" aria-label=move ||tr(english.get(),"文件操作","File actions") style=format!("left:{}px;top:{}px",target.x,target.y) on:pointerdown=move |ev|ev.stop_propagation() on:keydown=move |ev|{if ev.key()=="Escape"{ev.prevent_default();m.dismiss();}else{keypress(ev.as_ref());}}>{["open","name","relative","path","reveal","exclude","rename","copy","move","trash"].into_iter().filter(|a|*a!="open"||target.file.kind=="file").map(|action|{let target=target.clone();view!{<button role="menuitem" disabled=move ||s.c.busy.get()||!regular on:click=move |_|m.action(s,target.clone(),action)>{move ||title(action,english.get())}</button>}}).collect_view()}</div>}
        })}</div></Show>
        <Show when=move ||m.draft.with(|v|v.is_some())><section class="source-file-review" role="region" aria-label=move ||tr(english.get(),"文件操作确认","Review file operation")>
            <header><strong>{move ||m.draft.with(|d|d.as_ref().map(|d|format!("{} · {}",title(&d.operation,english.get()),d.target.file.path)).unwrap_or_default())}</strong><button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|{m.draft.set(None);restore();}>{move ||tr(english.get(),"取消文件操作","Cancel file operation")}</button></header>
            <Show when=move ||m.draft.with(|d|d.as_ref().is_some_and(|d|d.operation=="rename"))><input aria-label=move ||tr(english.get(),"新的文件名","New file name") prop:value=move ||m.draft.with(|d|d.as_ref().map(|d|d.name.clone()).unwrap_or_default()) disabled=move ||s.c.busy.get() on:input=move |ev|m.draft.update(|d|{if let Some(d)=d{d.name=event_target_value(&ev);d.plan=None;}})/></Show>
            <Show when=move ||m.draft.with(|d|d.as_ref().is_some_and(|d|matches!(d.operation.as_str(),"copy"|"move")))><div class="source-target-picker"><select aria-label=move ||tr(english.get(),"目标仓库","Destination repository") disabled=move ||s.c.busy.get() prop:value=move ||m.draft.with(|d|d.as_ref().map(|d|d.repository.clone()).unwrap_or_default()) on:change=move |ev|{m.draft.update(|d|{if let Some(d)=d{d.repository=event_target_value(&ev);d.directory=None;d.page=None;d.plan=None;}});m.directory(s,None);}>{move ||s.c.catalog.with(|c|c.repositories.iter().filter(|r|r.kind!="bare").map(|r|view!{<option value=r.id.clone()>{format!("{} · {}",r.name,r.path)}</option>}).collect_view())}</select><span>{move ||m.draft.with(|d|d.as_ref().and_then(|d|d.page.as_ref().map(|p|if p.path.is_empty(){tr(english.get(),"仓库根目录","Repository root").to_string()}else{p.path.clone()})).unwrap_or_default())}</span><button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|m.directory(s,None)>{move ||tr(english.get(),"根目录","Root")}</button><button class="quiet" disabled=move ||s.c.busy.get()||m.draft.with(|d|d.as_ref().is_none_or(|d|d.directory.is_none())) on:click=move |_|{let parent=m.draft.with_untracked(|d|d.as_ref().and_then(|d|d.page.as_ref().and_then(|p|p.parent.clone())));m.directory(s,parent);}>{move ||tr(english.get(),"上级目录","Parent folder")}</button><div>{move ||m.draft.get().and_then(|d|d.page).map(|page|page.files.into_iter().filter(|f|f.kind=="directory").map(|file|view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|m.directory(s,Some(file.id.clone()))>{file.name}</button>}).collect_view())}</div><button class="quiet" disabled=move ||s.c.busy.get()||m.draft.with(|d|d.as_ref().and_then(|d|d.page.as_ref()).is_none_or(|p|p.offset==0)) on:click=move |_|{if let Some(d)=m.draft.get_untracked(){let offset=d.page.as_ref().map(|p|p.offset.saturating_sub(300)).unwrap_or(0);m.directory_page(s,d.directory,offset);}}>{move ||tr(english.get(),"上一页目录","Previous folder page")}</button><button class="quiet" disabled=move ||s.c.busy.get()||m.draft.with(|d|d.as_ref().and_then(|d|d.page.as_ref()).is_none_or(|p|p.offset+300>=p.total)) on:click=move |_|{if let Some(d)=m.draft.get_untracked(){let offset=d.page.as_ref().map(|p|p.offset+300).unwrap_or(0);m.directory_page(s,d.directory,offset);}}>{move ||tr(english.get(),"下一页目录","Next folder page")}</button></div></Show>
            <p>{move ||tr(english.get(),"操作针对磁盘内容；已有目标不会覆盖。移动、重命名、删除前请关闭该项内打开的文件。","Operations use disk contents and never overwrite existing targets. Close files inside the item before moving, renaming or deleting.")}</p>
            {move ||m.draft.with(|d|d.as_ref().and_then(|d|d.plan.clone())).map(|plan|view!{<div class="source-file-summary"><div>{plan.source}</div><div>{plan.destination.unwrap_or_else(||tr(english.get_untracked(),"系统废纸篓","System trash").into())}</div><span>{format!("{} {} · {} {} · {} B",plan.files,tr(english.get(),"个文件","files"),plan.directories,tr(english.get(),"个目录","folders"),plan.bytes)}</span></div>})}
            <button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|m.prepare(s)>{move ||tr(english.get(),"预览文件操作","Preview file operation")}</button><button class="primary" disabled=move ||s.c.busy.get()||m.draft.with(|d|d.as_ref().is_none_or(|d|d.plan.is_none())) on:click=move |_|m.apply(s)>{move ||tr(english.get(),"执行文件操作","Apply file operation")}</button>
        </section></Show>
        <Show when=move ||!m.notice.get().is_empty()><div class="source-file-notice" role="status"><span>{move ||m.notice.get()}</span><button class="quiet" on:click=move |_|m.notice.set(String::new())>"×"</button></div></Show>
    }
}
