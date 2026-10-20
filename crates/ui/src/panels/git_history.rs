#[path = "git_revision.rs"]
mod revision;
// Commit graph, pinned details and contextual Git actions.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(inline_js = "
let historyAnchor;
let rebaseAnchor;
export function historyRebaseFocus(){rebaseAnchor=document.activeElement;requestAnimationFrame(()=>document.querySelector('.git-rebase-review button:not(:disabled)')?.focus());}
export function historyRebaseRestore(){if(rebaseAnchor?.isConnected)rebaseAnchor.focus();}
export function historyRebaseKey(event){if(event.key!=='Tab')return;const items=[...event.currentTarget.querySelectorAll('button:not(:disabled),select:not(:disabled),textarea:not(:disabled)')].filter(e=>e.getClientRects().length);if(!items.length)return;const first=items[0],last=items[items.length-1];if(event.shiftKey&&(document.activeElement===first||!event.currentTarget.contains(document.activeElement))){event.preventDefault();last.focus();}else if(!event.shiftKey&&(document.activeElement===last||!event.currentTarget.contains(document.activeElement))){event.preventDefault();first.focus();}}
export function historyMenuPosition(event,keyboard){historyAnchor=event.currentTarget.querySelector?.('.git-commit-title')||event.currentTarget;const r=historyAnchor.getBoundingClientRect();return [keyboard?r.left:event.clientX,keyboard?r.bottom:event.clientY];}
export function historyMenuFocus(){requestAnimationFrame(()=>{const m=document.querySelector('.git-history-menu');if(!m)return;const r=m.getBoundingClientRect();m.style.left=Math.max(4,Math.min(r.left,innerWidth-r.width-4))+'px';m.style.top=Math.max(4,Math.min(r.top,innerHeight-r.height-4))+'px';m.querySelector('button:not(:disabled)')?.focus();});}
export function historyMenuKey(event){const buttons=[...event.currentTarget.querySelectorAll('button:not(:disabled)')];const i=buttons.indexOf(document.activeElement);let n;if(event.key==='ArrowDown')n=(i+1)%buttons.length;else if(event.key==='ArrowUp')n=(i+buttons.length-1)%buttons.length;else if(event.key==='Home')n=0;else if(event.key==='End')n=buttons.length-1;else return;event.preventDefault();buttons[n]?.focus();}
export function historyMenuRestore(){if(historyAnchor?.isConnected)historyAnchor.focus();}
export async function historyCopy(text){if(navigator.clipboard?.writeText){await navigator.clipboard.writeText(text);return;}const input=document.createElement('textarea');input.value=text;input.style.cssText='position:fixed;left:-10000px';document.body.append(input);input.select();try{if(!document.execCommand('copy'))throw Error('Clipboard unavailable');}finally{input.remove();}}
")]
extern "C" {
    #[wasm_bindgen(js_name=historyRebaseFocus)]
    fn rebase_focus();
    #[wasm_bindgen(js_name=historyRebaseRestore)]
    fn rebase_restore();
    #[wasm_bindgen(js_name=historyRebaseKey)]
    fn rebase_key(event: &JsValue);
    #[wasm_bindgen(js_name=historyMenuPosition)]
    fn menu_position(event: &JsValue, keyboard: bool) -> js_sys::Array;
    #[wasm_bindgen(js_name=historyMenuFocus)]
    fn menu_focus();
    #[wasm_bindgen(js_name=historyMenuKey)]
    fn menu_key(event: &JsValue);
    #[wasm_bindgen(js_name=historyMenuRestore)]
    fn menu_restore();
    #[wasm_bindgen(js_name=historyCopy)]
    fn copy(text: &str) -> js_sys::Promise;
}
#[derive(Clone, Copy)]
struct History {
    c: Controller,
    query: RwSignal<HistoryQuery>,
    page: RwSignal<Option<HistoryPage>>,
    detail: RwSignal<Option<CommitDetail>>,
    children: RwSignal<Option<CommitChildren>>,
    patch: RwSignal<Option<GitPatch>>,
    menu: RwSignal<Option<(Commit, i32, i32)>>,
    file_id: RwSignal<String>,
    rebase: RwSignal<Option<Vec<RebaseTodoItem>>>,
    rebase_upstream: RwSignal<String>,
    revision: RwSignal<Option<String>>,
}
impl History {
    fn open_rebase(self, upstream: String) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        let repository_id = state.repository.id;
        let revision = state.revision;
        spawn_local(async move {
            match invoke::<Vec<RebaseTodoItem>>(
                "workspace_rebase_todo",
                serde_json::json!({"repositoryId":repository_id,"revision":revision,"upstream":upstream}),
            ).await {
                Ok(todo) => {
                    self.rebase_upstream.set(upstream);
                    if self.c.selected.with_untracked(|state| state.as_ref().is_some_and(|state| state.repository.id == repository_id)) {
                        self.rebase.set(Some(todo));
                        rebase_focus();
                    }
                }
                Err(error) => self.c.error.set(error),
            }
            self.c.busy.set(false);
        });
    }
    fn load(self, offset: usize, reset: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(repo) = self.c.selected.get_untracked() else {
            return;
        };
        let mut query = self.query.get_untracked();
        query.offset = offset;
        query.snapshot = if reset {
            None
        } else {
            self.page.get_untracked().map(|p| p.snapshot)
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        self.menu.set(None);
        spawn_local(async move {
            match invoke::<HistoryPage>(
                "workspace_history",
                serde_json::json!({"repositoryId":repo.repository.id,"query":query}),
            )
            .await
            {
                Ok(page) => {
                    self.page.set(Some(page));
                    self.detail.set(None);
                    self.children.set(None);
                    self.patch.set(None);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn select(self, reference: String, parent: usize) {
        self.select_details(reference, parent, false);
    }
    fn select_details(self, reference: String, parent: usize, find_children: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(repo) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        self.menu.set(None);
        spawn_local(async move {
            let mut loaded = false;
            match invoke::<CommitDetail>("workspace_commit_detail",serde_json::json!({"repositoryId":repo.repository.id,"reference":reference,"parentIndex":parent})).await {
                Ok(detail)=>{self.detail.set(Some(detail));self.children.set(None);self.patch.set(None);loaded=true;}
                Err(e)=>self.c.error.set(e),
            }
            self.c.busy.set(false);
            if loaded && find_children {
                self.children(0, true);
            }
        });
    }
    fn children(self, offset: usize, refresh: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(repo) = self.c.selected.get_untracked() else {
            return;
        };
        let Some(detail) = self.detail.get_untracked() else {
            return;
        };
        let snapshot = if refresh {
            None
        } else {
            self.children.get_untracked().map(|p| p.snapshot)
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<CommitChildren>("workspace_commit_children", serde_json::json!({"repositoryId":repo.repository.id,"detailId":detail.id,"offset":offset,"snapshot":snapshot})).await {
                Ok(page) if page.target == detail.commit.id => self.children.set(Some(page)),
                Ok(_) => self.c.error.set("Child commit selection changed; open details again".into()),
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn file(self, id: String, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(repo) = self.c.selected.get_untracked() else {
            return;
        };
        let Some(detail) = self.detail.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<GitPatch>("workspace_commit_patch",serde_json::json!({"repositoryId":repo.repository.id,"detailId":detail.id,"fileId":id,"offset":offset})).await {
                Ok(patch)=>{self.file_id.set(id);self.patch.set(Some(patch));},Err(e)=>self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn context(self, commit: Commit, event: &JsValue, keyboard: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let p = menu_position(event, keyboard);
        self.menu.set(Some((
            commit,
            p.get(0).as_f64().unwrap_or_default() as i32,
            p.get(1).as_f64().unwrap_or_default() as i32,
        )));
        menu_focus();
    }
}
fn child_navigation(h: History, english: RwSignal<bool>) -> impl IntoView {
    view! {<section class="git-child-navigation" aria-label=move ||tr(english.get(),"子提交导航","Child commit navigation")>
        <button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|h.children(0,true)>{move ||tr(english.get(),"查找子提交","Find child commits")}</button>
        <Show when=move ||h.children.with(|p|p.is_some())>
            <span>{move ||h.children.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",p.offset+usize::from(p.total>0),p.offset+p.children.len(),p.total)).unwrap_or_default())}</span>
            <button class="quiet" aria-label=move ||tr(english.get(),"上一页子提交","Previous child commits") disabled=move ||h.c.busy.get()||h.children.with(|p|p.as_ref().is_none_or(|p|p.offset==0)) on:click=move |_|{if let Some(p)=h.children.get_untracked(){h.children(p.offset.saturating_sub(100),false)}}>"‹"</button>
            <button class="quiet" aria-label=move ||tr(english.get(),"下一页子提交","Next child commits") disabled=move ||h.c.busy.get()||h.children.with(|p|p.as_ref().is_none_or(|p|p.next_offset.is_none())) on:click=move |_|{if let Some(offset)=h.children.get_untracked().and_then(|p|p.next_offset){h.children(offset,false)}}>"›"</button>
            <div class="git-child-links">{move ||h.children.get().map(|p|p.children.into_iter().map(move |hash|{let id=hash.clone();view!{<button class="quiet" title=hash.clone() aria-label=format!("{} {hash}",tr(english.get(),"打开子提交","Open child commit")) disabled=move ||h.c.busy.get() on:click=move |_|h.select(id.clone(),0)>{format!("↓ {}",hash.chars().take(8).collect::<String>())}</button>}}).collect_view())}</div>
            <Show when=move ||h.children.with(|p|p.as_ref().is_some_and(|p|p.total==0))><span>{move ||tr(english.get(),"当前分支、远程分支、标签与 HEAD 中未找到直接子提交。","No direct children reachable from local branches, remote branches, tags or HEAD.")}</span></Show>
        </Show>
    </section>}
}
fn graph(row: GraphCommit) -> impl IntoView {
    let x = 12 + row.column * 14;
    let width = (row.width * 14 + 12).max(28);
    view! {<svg class="git-graph" width=width height="38" viewBox=format!("0 0 {width} 38") aria-hidden="true"><path class="git-graph-edge" d=format!("M{x},0 L{x},18")/>{row.edges.into_iter().map(move |e|{let from=12+e.from*14;let to=12+e.to*14;let start=if e.from==row.column{18}else{0};view!{<path class="git-graph-edge" d=format!("M{from},{start} C{from},28 {to},28 {to},38")/>}}).collect_view()}<circle class="git-graph-node" cx=x cy="18" r="3"/></svg>}
}
fn commit_files(files: Vec<CommitFile>, h: History, english: RwSignal<bool>) -> impl IntoView {
    let files = StoredValue::new(files);
    let query = RwSignal::new(String::new());
    let offset = RwSignal::new(0usize);
    let filtered = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        files.with_value(|files| {
            files
                .iter()
                .filter(|f| q.is_empty() || f.path.to_lowercase().contains(&q))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    view! {<div class="git-commit-files" aria-label=move ||tr(english.get(),"提交变更文件","Commit changed files")>
        <input aria-label=move ||tr(english.get(),"筛选提交文件","Filter commit files") placeholder=move ||tr(english.get(),"筛选文件路径…","Filter file paths…") prop:value=move ||query.get() on:input=move |ev|{query.set(event_target_value(&ev));offset.set(0);}/>
        <For each=move ||filtered.with(|files|files.iter().skip(offset.get()).take(200).cloned().collect::<Vec<_>>()) key=|f|f.id.clone() children=move |file|{let id=file.id.clone();let title=file.original_path.map(|p|format!("{p} → {}",file.path)).unwrap_or_else(||file.path.clone());view!{<button title=title class="git-commit-file" disabled=move ||h.c.busy.get() on:click=move |_|h.file(id.clone(),0)><strong>{file.status}</strong><span>{file.path}</span></button>}}/>
        <footer class="git-file-paging"><span>{move ||filtered.with(|f|format!("{}–{} / {}",offset.get()+usize::from(!f.is_empty()),(offset.get()+200).min(f.len()),f.len()))}</span><button class="quiet" aria-label=move ||tr(english.get(),"上一页提交文件","Previous commit files") disabled=move ||offset.get()==0 on:click=move |_|offset.update(|n|*n=n.saturating_sub(200))>"‹"</button><button class="quiet" aria-label=move ||tr(english.get(),"下一页提交文件","Next commit files") disabled=move ||filtered.with(|f|offset.get()+200>=f.len()) on:click=move |_|offset.update(|n|*n+=200)>"›"</button></footer>
    </div>}
}

fn rebase_row(h: History, item: RebaseTodoItem, english: RwSignal<bool>) -> impl IntoView {
    let id = StoredValue::new(item.commit);
    let current = Memo::new(move |_| {
        h.rebase.with(|items| {
            items.as_ref().and_then(|items| {
                items
                    .iter()
                    .position(|item| id.with_value(|id| item.commit == *id))
                    .map(|index| (index, items[index].clone()))
            })
        })
    });
    let action = move || {
        current.with(|item| {
            item.as_ref()
                .map(|(_, item)| item.action)
                .unwrap_or(RebaseStepKind::Pick)
        })
    };
    let change_action = move |value: String| {
        let kind = match value.as_str() {
            "reword" => RebaseStepKind::Reword,
            "edit" => RebaseStepKind::Edit,
            "squash" => RebaseStepKind::Squash,
            "fixup" => RebaseStepKind::Fixup,
            "drop" => RebaseStepKind::Drop,
            _ => RebaseStepKind::Pick,
        };
        h.rebase.update(|items| {
            if let Some(item) = items.as_mut().and_then(|items| {
                items
                    .iter_mut()
                    .find(|item| id.with_value(|id| item.commit == *id))
            }) {
                item.action = kind;
            }
        });
    };
    view! {<li class="git-rebase-item">
        <div class="git-rebase-order">
            <button class="quiet" aria-label=move ||tr(english.get(),"上移提交","Move commit up") disabled=move ||current.with(|item|item.as_ref().is_none_or(|(index,_)|*index==0)) on:click=move |_|{if let Some((index,_))=current.get_untracked()&&index>0{h.rebase.update(|items|if let Some(items)=items{items.swap(index,index-1)})}}>"↑"</button>
            <button class="quiet" aria-label=move ||tr(english.get(),"下移提交","Move commit down") disabled=move ||current.with(|item|item.as_ref().is_none_or(|(index,_)|h.rebase.with(|items|items.as_ref().is_none_or(|items|index+1>=items.len())))) on:click=move |_|{if let Some((index,_))=current.get_untracked(){h.rebase.update(|items|if let Some(items)=items&&index+1<items.len(){items.swap(index,index+1)})}}>"↓"</button>
        </div>
        <div class="git-rebase-commit"><code>{id.with_value(|id|id.chars().take(10).collect::<String>())}</code><span>{item.subject}</span></div>
        <select aria-label=move ||tr(english.get(),"提交操作","Commit action") prop:value=move ||match action(){RebaseStepKind::Pick=>"pick",RebaseStepKind::Reword=>"reword",RebaseStepKind::Edit=>"edit",RebaseStepKind::Squash=>"squash",RebaseStepKind::Fixup=>"fixup",RebaseStepKind::Drop=>"drop"} on:change=move |ev|change_action(event_target_value(&ev))>
            <option value="pick">{move ||tr(english.get(),"保留提交","Pick")}</option>
            <option value="reword">{move ||tr(english.get(),"修改说明","Reword")}</option>
            <option value="edit">{move ||tr(english.get(),"暂停编辑","Edit")}</option>
            <option value="squash">{move ||tr(english.get(),"合并并保留说明","Squash")}</option>
            <option value="fixup">{move ||tr(english.get(),"合并并丢弃说明","Fixup")}</option>
            <option value="drop">{move ||tr(english.get(),"丢弃提交","Drop")}</option>
        </select>
        <Show when=move ||action()==RebaseStepKind::Reword><textarea aria-label=move ||format!("{} {}",tr(english.get(),"提交说明","Commit message"),id.with_value(|id|id.chars().take(8).collect::<String>())) prop:value=move ||current.with(|item|item.as_ref().map(|(_,item)|item.message.clone()).unwrap_or_default()) on:input=move |ev|{let value=event_target_value(&ev);h.rebase.update(|items|if let Some(item)=items.as_mut().and_then(|items|items.iter_mut().find(|item|id.with_value(|id|item.commit==*id))){item.message=value;})}/></Show>
    </li>}
}

fn rebase_editor(h: History, english: RwSignal<bool>) -> impl IntoView {
    view! {<Show when=move ||h.rebase.with(|todo|todo.is_some())>
        <div class="git-rebase-backdrop" on:click=move |_|{}></div>
        <section class="git-rebase-review" role="dialog" aria-modal="true" aria-label=move ||tr(english.get(),"交互式变基序列","Interactive rebase sequence") on:keydown=move |ev|{if ev.key()=="Escape"{ev.stop_propagation();h.rebase.set(None);rebase_restore();}else{rebase_key(ev.as_ref());}}>
            <header><div><h2>{move ||tr(english.get(),"交互式变基","Interactive rebase")}</h2><p>{move ||tr(english.get(),"按从旧到新的顺序排列；选中基点本身不改写。合并提交按 Git 默认行为省略。执行前创建恢复引用。","Ordered oldest first; the selected base is not rewritten. Merge commits are omitted, following Git's default behavior. A recovery reference is created before execution.")}</p></div><code>{move ||h.rebase_upstream.get().chars().take(12).collect::<String>()}</code></header>
            <Show when=move ||!h.c.error.get().is_empty()><p class="git-rebase-error">{move ||h.c.error.get()}</p></Show>
            <ol><For each=move ||h.rebase.get().unwrap_or_default() key=|item|item.commit.clone() children=move |item|rebase_row(h,item,english)/></ol>
            <footer><span>{move ||h.rebase.with(|todo|format!("{} {}",todo.as_ref().map_or(0,Vec::len),tr(english.get(),"个提交；暂停编辑时可修改内容并 amend 后继续","commits; at an edit stop, edit and amend the commit before continuing")))}</span><button class="quiet" on:click=move |_|{h.rebase.set(None);rebase_restore();}>{move ||tr(english.get(),"取消","Cancel")}</button><button class="primary" disabled=move ||h.c.busy.get()||h.rebase.with(|todo|todo.as_ref().is_none_or(Vec::is_empty)) on:click=move |_|{let todo=h.rebase.get_untracked().unwrap_or_default();let upstream=h.rebase_upstream.get_untracked();h.c.run(Action::InteractiveRebase{upstream,todo});}>{move ||tr(english.get(),"预览执行计划","Review execution plan")}</button></footer>
        </section>
    </Show>}
}

pub fn panel(
    c: Controller,
    active: RwSignal<bool>,
    english: RwSignal<bool>,
    files_visible: RwSignal<bool>,
    editor_request: RwSignal<Option<(String, String)>>,
) -> impl IntoView {
    let h = History {
        c,
        query: RwSignal::new(HistoryQuery::default()),
        page: RwSignal::new(None),
        detail: RwSignal::new(None),
        children: RwSignal::new(None),
        patch: RwSignal::new(None),
        menu: RwSignal::new(None),
        file_id: RwSignal::new(String::new()),
        rebase: RwSignal::new(None),
        rebase_upstream: RwSignal::new(String::new()),
        revision: RwSignal::new(None),
    };
    let name = RwSignal::new(String::new());
    let current = Memo::new(move |_| {
        c.selected.with(|s| {
            s.as_ref()
                .map(|s| (s.repository.id.clone(), s.repository.head.clone()))
        })
    });
    let last_repository = RwSignal::new(String::new());
    Effect::new(move |_| {
        let repository = current.get().map(|r| r.0).unwrap_or_default();
        if repository != last_repository.get_untracked() {
            last_repository.set(repository);
            h.revision.set(None);
            h.rebase.set(None);
            h.page.set(None);
            h.detail.set(None);
            h.children.set(None);
            h.patch.set(None);
            h.menu.set(None);
        }
        if active.get() {
            h.load(0, true);
        }
    });
    Effect::new(move |_| {
        if c.plan
            .with(|plan| plan.as_ref().is_some_and(|plan| plan.rebase_todo.is_some()))
        {
            h.rebase.set(None);
            rebase_restore();
        }
    });
    let close = move || {
        h.menu.set(None);
        menu_restore();
    };
    view! {<section class="git-history" style:display=move ||if active.get(){"flex"}else{"none"} aria-label=move ||tr(english.get(),"提交历史","Commit history") on:click=move |_|h.menu.set(None) on:keydown=move |ev|{if ev.key()=="Escape"{close();}}>{rebase_editor(h,english)}<div class="git-history-original" style:display=move ||if h.revision.with(|revision|revision.is_some()){"none"}else{"contents"}>
        <div class="git-history-filters"><input aria-label=move ||tr(english.get(),"历史分支或标签","History branch or tag") placeholder=move ||tr(english.get(),"全部分支与标签","All branches and tags") prop:value=move ||h.query.with(|q|q.reference.clone().unwrap_or_default()) on:input=move |ev|{let v=event_target_value(&ev);h.query.update(|q|q.reference=(!v.is_empty()).then_some(v));}/><input aria-label=move ||tr(english.get(),"提交说明或哈希","Commit message or hash") placeholder=move ||tr(english.get(),"提交说明或哈希","Commit message or hash") prop:value=move ||h.query.with(|q|q.search.clone()) on:input=move |ev|h.query.update(|q|q.search=event_target_value(&ev))/><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|h.load(0,true)>{move ||tr(english.get(),"查询历史","Search history")}</button><Show when=move ||h.detail.with(|d|d.is_some())><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|if let Some(detail)=h.detail.get_untracked(){h.revision.set(Some(detail.commit.id));}>{move ||tr(english.get(),"浏览此提交文件","Browse files at revision")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|if let Some(detail)=h.detail.get_untracked(){h.open_rebase(detail.commit.id)}>{move ||tr(english.get(),"交互式变基…","Interactive rebase…")}</button></Show><details><summary>{move ||tr(english.get(),"更多筛选","More filters")}</summary><div class="git-history-more"><input aria-label=move ||tr(english.get(),"提交作者","Commit author") placeholder=move ||tr(english.get(),"作者或邮箱","Author or email") prop:value=move ||h.query.with(|q|q.author.clone()) on:input=move |ev|h.query.update(|q|q.author=event_target_value(&ev))/><input aria-label=move ||tr(english.get(),"历史文件路径","History file path") placeholder=move ||tr(english.get(),"仓库内相对路径","Repository relative path") prop:value=move ||h.query.with(|q|q.path.clone().unwrap_or_default()) on:input=move |ev|{let v=event_target_value(&ev);h.query.update(|q|q.path=(!v.is_empty()).then_some(v));}/><label>{move ||tr(english.get(),"起始日期","Since")}<input type="date" prop:value=move ||h.query.with(|q|q.since.clone()) on:input=move |ev|h.query.update(|q|q.since=event_target_value(&ev))/></label><label>{move ||tr(english.get(),"截止日期","Until")}<input type="date" prop:value=move ||h.query.with(|q|q.until.split('T').next().unwrap_or_default().to_string()) on:input=move |ev|{let value=event_target_value(&ev);h.query.update(|q|q.until=if value.is_empty(){value}else{format!("{value}T23:59:59")});}/></label></div></details></div>
        <div class="git-history-body"><div class="git-commits"><table><thead><tr><th title=move ||tr(english.get(),"筛选时仅绘制可见提交间的直接连接；父提交可在详情中查看","Filters show direct connections between visible commits; inspect parents in commit details")>{move ||tr(english.get(),"关系","Graph")}</th><th>{move ||tr(english.get(),"提交说明","Commit")}</th><th>{move ||tr(english.get(),"作者","Author")}</th><th>{move ||tr(english.get(),"日期","Date")}</th></tr></thead><tbody>{move ||h.page.get().map(|p|p.commits.into_iter().map(move |row|{let commit=row.commit.clone();let id=commit.id.clone();let menu_commit=commit.clone();let key_commit=commit.clone();let chosen=commit.id.clone();let short=commit.id.chars().take(8).collect::<String>();view!{<tr on:contextmenu=move |ev|{ev.prevent_default();h.context(menu_commit.clone(),ev.as_ref(),false);} class:selected=move ||h.detail.with(|d|d.as_ref().is_some_and(|d|d.commit.id==chosen))><td>{graph(row)}</td><td><button class="git-commit-title" disabled=move ||c.busy.get() on:click=move |_|h.select(id.clone(),0) on:keydown=move |ev|{if ev.key()=="ContextMenu"||(ev.shift_key()&&ev.key()=="F10"){ev.prevent_default();h.context(key_commit.clone(),ev.as_ref(),true);}}><span>{commit.subject}</span><small>{format!("{short} {}",commit.decorations)}</small></button></td><td title=commit.email>{commit.author}</td><td title=commit.date.clone()>{commit.date.chars().take(10).collect::<String>()}</td></tr>}}).collect_view())}</tbody></table><Show when=move ||h.page.with(|p|p.as_ref().is_some_and(|p|p.commits.is_empty()))><p class="workbench-empty">{move ||tr(english.get(),"没有匹配的提交。","No matching commits.")}</p></Show></div>
        <aside class="git-commit-detail"><Show when=move ||h.detail.with(|d|d.is_some()) fallback=move ||view!{<p class="workbench-empty">{move ||tr(english.get(),"选择提交查看说明和变更文件；右键打开提交操作。","Select a commit for details and files. Right-click for commit actions.")}</p>}>
            {move ||h.detail.get().map(|detail|{let target=detail.commit.id.clone();let parents=detail.commit.parents.clone();view!{<div class="git-commit-metadata"><code>{detail.commit.id.clone()}</code><span>{format!("{} <{}> · {}",detail.commit.author,detail.commit.email,detail.commit.date)}</span><pre>{detail.message}</pre><label>{move ||tr(english.get(),"与父提交比较","Compare with parent")}<select aria-label=move ||tr(english.get(),"比较父提交","Compare parent") disabled=move ||c.busy.get()||parents.is_empty() on:change=move |ev|h.select(target.clone(),event_target_value(&ev).parse().unwrap_or(0))>{detail.commit.parents.iter().enumerate().map(|(i,p)|view!{<option value=i selected=detail.parent.as_ref()==Some(p)>{format!("{} · {}",i+1,p.chars().take(8).collect::<String>())}</option>}).collect_view()}</select></label><div class="git-parent-navigation">{detail.commit.parents.into_iter().map(move |parent|{let id=parent.clone();view!{<button class="quiet" disabled=move ||c.busy.get() on:click=move |_|h.select(id.clone(),0)>{format!("↑ {}",parent.chars().take(8).collect::<String>())}</button>}}).collect_view()}</div></div>{child_navigation(h,english)}{commit_files(detail.files,h,english)}}})}
        </Show></aside></div>
                <Show when=move ||h.patch.with(|p|p.is_some())><section class="git-history-patch"><header><strong>{move ||h.patch.with(|p|p.as_ref().map(|p|p.path.clone()).unwrap_or_default())}</strong><button class="quiet" on:click=move |_|h.patch.set(None)>{move ||tr(english.get(),"关闭差异","Close diff")}</button></header><div>{move ||h.patch.get().map(|p|p.text.lines().map(|line|{let css=if line.starts_with('+')&&!line.starts_with("+++"){"insert"}else if line.starts_with('-')&&!line.starts_with("---"){"delete"}else if line.starts_with("@@"){"hunk"}else{""};view!{<pre class=css>{line.to_string()}</pre>}}).collect_view())}</div><footer class="git-patch-paging"><span>{move ||h.patch.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",p.offset+usize::from(p.total_lines>0),(p.offset+500).min(p.total_lines),p.total_lines)).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||h.patch.with(|p|p.as_ref().is_none_or(|p|p.offset==0)) on:click=move |_|h.file(h.file_id.get_untracked(),h.patch.get_untracked().map(|p|p.offset.saturating_sub(500)).unwrap_or_default())>{move ||tr(english.get(),"上一页差异","Previous diff page")}</button><button class="quiet" disabled=move ||c.busy.get()||h.patch.with(|p|p.as_ref().is_none_or(|p|p.next_offset.is_none())) on:click=move |_|{if let Some(offset)=h.patch.get_untracked().and_then(|p|p.next_offset){h.file(h.file_id.get_untracked(),offset)}}>{move ||tr(english.get(),"下一页差异","Next diff page")}</button><span>{move ||h.patch.with(|p|p.as_ref().is_some_and(|p|p.long_lines)).then_some(tr(english.get(),"超长行仅预览前 8192 字符","Long lines preview the first 8192 characters"))}</span></footer></section></Show>
        <footer class="git-history-paging"><span>{move ||h.page.with(|p|p.as_ref().map(|p|format!("{}–{}",p.offset+usize::from(!p.commits.is_empty()),p.offset+p.commits.len())).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||h.page.with(|p|p.as_ref().is_none_or(|p|p.offset==0)) on:click=move |_|h.load(h.page.get_untracked().map(|p|p.offset.saturating_sub(100)).unwrap_or_default(),false)>{move ||tr(english.get(),"上一页提交","Previous commits")}</button><button class="quiet" disabled=move ||c.busy.get()||h.page.with(|p|p.as_ref().is_none_or(|p|!p.has_more)) on:click=move |_|h.load(h.page.get_untracked().map(|p|p.offset+100).unwrap_or_default(),false)>{move ||tr(english.get(),"下一页提交","Next commits")}</button></footer>
        <Show when=move ||h.menu.with(|m|m.is_some())><div class="git-history-menu-dismiss" on:click=move |_|close() on:contextmenu=move |ev|{ev.prevent_default();close();}></div>{move ||h.menu.get().map(|(commit,x,y)|{let hash=commit.id.clone();let copy_hash=hash.clone();let child_hash=hash.clone();let rebase_hash=hash.clone();let browse_hash=hash.clone();view!{<div class="git-history-menu" role="menu" aria-label=move ||tr(english.get(),"提交操作","Commit actions") style=format!("left:{x}px;top:{y}px") on:click=move |ev|ev.stop_propagation() on:keydown=move |ev|{ev.stop_propagation();if ev.key()=="Escape"{close();}else{menu_key(ev.as_ref());}}><strong>{commit.id.chars().take(12).collect::<String>()}</strong><button role="menuitem" on:click=move |_|{let value=copy_hash.clone();spawn_local(async move{match wasm_bindgen_futures::JsFuture::from(copy(&value)).await{Ok(_)=>close(),Err(_)=>c.error.set(tr(english.get_untracked(),"无法复制提交哈希","Unable to copy commit hash").into())}});}>{move ||tr(english.get(),"复制提交哈希","Copy revision hash")}</button><button role="menuitem" disabled=move ||c.busy.get() on:click=move |_|{close();h.revision.set(Some(browse_hash.clone()));}>{move ||tr(english.get(),"浏览此提交的仓库","Show repository at revision")}</button><button role="menuitem" disabled=move ||c.busy.get() on:click=move |_|{close();h.open_rebase(rebase_hash.clone());}>{move ||tr(english.get(),"从此处交互式变基…","Interactive rebase from here…")}</button>{[("cherry_pick","拣选提交","Cherry-pick"),("revert","反向提交","Revert commit"),("checkout","检出此提交","Checkout revision"),("soft","重置到此处（保留暂存修改）","Reset here (soft)"),("mixed","重置到此处（保留工作区修改）","Reset here (mixed)"),("hard","重置到此处（丢弃工作区修改）","Reset here (hard)")].into_iter().map(move |(action,zh,en)|{let hash=hash.clone();view!{<button role="menuitem" disabled=move ||c.busy.get()||(action=="hard"&&c.selected.with(|s|s.as_ref().is_some_and(|s|s.changes.iter().any(|c|c.untracked||c.conflicted)))) on:click=move |_|{close();c.run(match action{"cherry_pick"=>Action::CherryPick{reference:hash.clone()},"revert"=>Action::Revert{reference:hash.clone()},"checkout"=>Action::Switch{reference:hash.clone(),detach:true},mode=>Action::Reset{reference:hash.clone(),mode:mode.into()}});}>{move ||tr(english.get(),zh,en)}</button>}}).collect_view()}<button role="menuitem" disabled=move ||c.busy.get() on:click=move |_|{close();h.select_details(child_hash.clone(),0,true);}>{move ||tr(english.get(),"跳转子提交…","Go to child commit…")}</button><input aria-label=move ||tr(english.get(),"从提交创建的名称","Name to create from commit") placeholder=move ||tr(english.get(),"新分支或标签名称","New branch or tag name") prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev))/>{[(false,"从此提交新建分支","New branch from commit"),(true,"从此提交新建标签","New tag from commit")].into_iter().map(move |(tag,zh,en)|{let hash=commit.id.clone();view!{<button role="menuitem" disabled=move ||c.busy.get()||name.get().trim().is_empty() on:click=move |_|{close();c.run(if tag{Action::CreateTag{name:name.get_untracked(),reference:hash.clone(),message:None}}else{Action::CreateBranch{name:name.get_untracked(),start:Some(hash.clone())}});}>{move ||tr(english.get(),zh,en)}</button>}}).collect_view()}<button role="menuitem" on:click=move |_|close()>{move ||tr(english.get(),"关闭菜单","Close menu")}</button></div>}})}</Show>
    </div>{revision::panel(h,english,files_visible,editor_request)}</section>}
}
