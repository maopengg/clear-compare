#[path = "source_menu.rs"]
mod source_menu;
// Lazy file tree, multi-document editor and native source search.
use super::{entry_icon, git_actions};
use crate::{invoke, tr};
use clear_contracts::{
    Encoding, Kind, LineEnding, SaveOptions, SaveResult, TextDocument, workbench::*,
};
use leptos::{prelude::*, task::spawn_local};
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(inline_js = "
export function sourceMount(...args){if(!window.ClearCode)throw Error('Editor assets are unavailable');window.ClearCode.mount(...args);}
export function sourceActivate(...args){window.ClearCode?.activate(...args);}
export function sourceReadonly(busy){window.ClearCode?.setReadonly(busy);}
export function sourceLocale(en){window.ClearCode?.setLocale(en);}
export function sourceNavigate(...args){window.ClearCode?.navigate(...args);}
export function sourceCommand(...args){window.ClearCode?.command(...args);}
export function sourceDestroy(id){window.ClearCode?.destroy(id);}
export function sourceFocusSearch(){window.ClearCode?.focusSearch();}
export function sourceQuickOpen(){window.ClearCode?.focusSearch();requestAnimationFrame(()=>{const select=document.querySelector('.source-search-type');if(select){select.value='name';select.dispatchEvent(new Event('change',{bubbles:true}));}const scope=document.querySelector('.source-search-scope');if(scope){scope.value=document.querySelector('.source-toolbar select')?.value?'current':'all';scope.dispatchEvent(new Event('change',{bubbles:true}));}document.querySelector('.source-search-query')?.select();});}
")]
extern "C" {
    #[wasm_bindgen(catch,js_name=sourceMount)]
    fn mount(
        host: &JsValue,
        id: &str,
        text: &str,
        path: &str,
        english: bool,
        change: &JsValue,
        save: &JsValue,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(js_name=sourceActivate)]
    fn activate(host: &JsValue, id: &str);
    #[wasm_bindgen(js_name=sourceReadonly)]
    fn readonly(busy: bool);
    #[wasm_bindgen(js_name=sourceLocale)]
    fn locale(english: bool);
    #[wasm_bindgen(js_name=sourceNavigate)]
    fn navigate(id: &str, offset: usize, length: usize);
    #[wasm_bindgen(js_name=sourceCommand)]
    fn command(id: &str, name: &str);
    #[wasm_bindgen(js_name=sourceDestroy)]
    fn destroy(id: &str);
    #[wasm_bindgen(js_name=sourceFocusSearch)]
    pub fn focus_search();
    #[wasm_bindgen(js_name=sourceQuickOpen)]
    pub fn quick_open();
}
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
#[derive(Clone)]
struct Tab {
    repository: String,
    file: String,
    document: RwSignal<TextDocument>,
    draft: RwSignal<String>,
    saved: RwSignal<String>,
    options: RwSignal<SaveOptions>,
    generation: RwSignal<u64>,
    changed: RwSignal<bool>,
}
impl Tab {
    fn release_text(&self) {
        self.draft.set(String::new());
        self.saved.set(String::new());
        self.document.update(|d| d.text = String::new());
    }
    fn dirty(&self) -> bool {
        self.draft.with(|d| self.saved.with(|s| d != s))
            || self
                .options
                .with(|o| o.encoding.is_some() || o.bom.is_some() || o.line_ending.is_some())
    }
}
#[derive(Clone, Copy)]
struct Controller {
    c: git_actions::Controller,
    english: RwSignal<bool>,
    tabs: RwSignal<Vec<Tab>>,
    active: RwSignal<String>,
    host: NodeRef<leptos::html::Div>,
    tree: RwSignal<BTreeMap<String, FilePage>>,
    expanded: RwSignal<BTreeSet<String>>,
    parent: RwSignal<Option<String>>,
    close: RwSignal<Option<String>>,
    results: RwSignal<Option<SourceResults>>,
    result_offset: RwSignal<usize>,
}
fn key(repo: &str, directory: Option<&str>) -> String {
    format!("{repo}/{}", directory.unwrap_or("root"))
}
impl Controller {
    fn tab(self, id: &str) -> Option<Tab> {
        self.tabs.with(|tabs| {
            tabs.iter()
                .find(|t| t.document.with(|d| d.id == id))
                .cloned()
        })
    }
    fn current(self) -> Option<Tab> {
        self.tab(&self.active.get())
    }
    fn select(self, id: String) {
        self.active.set(id.clone());
        if let Some(host) = self.host.get() {
            activate(host.as_ref(), &id);
        }
    }
    fn dirty(self, tab: Tab) {
        let dirty = tab.dirty();
        tab.generation.update(|v| *v += 1);
        let id = tab.document.with_untracked(|d| d.id.clone());
        let generation = tab.generation.get_untracked();
        spawn_local(async move {
            if let Err(e) = invoke::<()>(
                "workspace_source_dirty",
                serde_json::json!({"documentId":id,"dirty":dirty,"generation":generation}),
            )
            .await
                && self.tab(&id).is_some()
            {
                self.c.error.set(e);
            }
        });
    }
    fn open(
        self,
        repository: String,
        file: String,
        expected: Option<String>,
        position: Option<(usize, usize)>,
    ) {
        if self.c.busy.get_untracked() {
            return;
        }
        if let Some(tab) = self.tabs.with_untracked(|tabs| {
            tabs.iter()
                .find(|t| t.repository == repository && t.file == file)
                .cloned()
        }) {
            if expected.is_some()
                && (tab.dirty()
                    || tab
                        .document
                        .with_untracked(|d| Some(&d.digest) != expected.as_ref()))
            {
                self.c.error.set(
                    tr(
                        self.english.get_untracked(),
                        "请先保存或重新载入已打开文件，再定位搜索结果。",
                        "Save or reload the open file before navigating search results.",
                    )
                    .into(),
                );
                return;
            }
            let id = tab.document.with_untracked(|d| d.id.clone());
            if expected.is_some() {
                self.c.busy.set(true);
                self.c.error.set(String::new());
                spawn_local(async move {
                    match invoke::<WorkspaceDocument>("workspace_source_open", serde_json::json!({"repositoryId":repository,"fileId":file,"expected":expected})).await {
                        Ok(_) => { self.select(id.clone()); if let Some((offset,length))=position { navigate(&id,offset,length); } },
                        Err(e) => self.c.error.set(e),
                    }
                    self.c.busy.set(false);
                });
            } else {
                self.select(id);
            }
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<WorkspaceDocument>(
                "workspace_source_open",
                serde_json::json!({"repositoryId":repository,"fileId":file,"expected":expected}),
            )
            .await
            {
                Ok(view) => {
                    let text = normalize(&view.document.text);
                    let id = view.document.id.clone();
                    let tab = Tab {
                        repository: view.repository_id,
                        file: view.file_id,
                        document: RwSignal::new(view.document),
                        draft: RwSignal::new(text.clone()),
                        saved: RwSignal::new(text.clone()),
                        options: RwSignal::new(SaveOptions::default()),
                        generation: RwSignal::new(0),
                        changed: RwSignal::new(false),
                    };
                    self.tabs.update(|tabs| tabs.push(tab.clone()));
                    self.active.set(id.clone());
                    if let Some(host) = self.host.get() {
                        let t = tab.clone();
                        let change = Closure::<dyn FnMut(String)>::new(move |text| {
                            t.draft.set(text);
                            self.dirty(t.clone());
                        })
                        .into_js_value();
                        let save_id = id.clone();
                        let save =
                            Closure::<dyn FnMut()>::new(move || self.save(save_id.clone(), false))
                                .into_js_value();
                        let path = tab.document.with_untracked(|d| d.path.clone());
                        if let Err(e) = mount(
                            host.as_ref(),
                            &id,
                            &text,
                            &path,
                            self.english.get_untracked(),
                            &change,
                            &save,
                        ) {
                            self.c.error.set(
                                e.as_string()
                                    .unwrap_or_else(|| "Unable to initialize editor".into()),
                            );
                        } else if let Some((offset, length)) = position {
                            navigate(&id, offset, length);
                        }
                    }
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn save(self, id: String, close_after: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(tab) = self.tab(&id) else {
            return;
        };
        if !tab.dirty() {
            if close_after {
                self.close_tab(id, false);
            }
            return;
        }
        tab.generation.update(|v| *v += 1);
        let generation = tab.generation.get_untracked();
        let text = tab.draft.get_untracked();
        let options = tab.options.get_untracked();
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SaveResult>("workspace_source_save",serde_json::json!({"documentId":id,"text":text,"options":options,"generation":generation})).await{Ok(result)=>{tab.saved.set(normalize(&result.document.text));tab.document.set(result.document);tab.options.set(SaveOptions::default());tab.changed.set(false);if tab.dirty(){self.dirty(tab.clone());}},Err(e)=>self.c.error.set(e)}
            self.c.busy.set(false);
            if close_after && !tab.dirty() {
                self.close_tab(id, false);
            }
        });
    }
    fn close_tab(self, id: String, discard: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(tab) = self.tab(&id) else {
            return;
        };
        if tab.dirty() && !discard {
            self.close.set(Some(id));
            return;
        }
        self.c.busy.set(true);
        spawn_local(async move {
            match invoke::<()>(
                "workspace_source_close",
                serde_json::json!({"documentId":id,"discard":discard}),
            )
            .await
            {
                Ok(()) => {
                    destroy(&id);
                    self.tabs
                        .update(|tabs| tabs.retain(|t| t.document.with_untracked(|d| d.id != id)));
                    tab.release_text();
                    self.close.set(None);
                    let next = self.tabs.with_untracked(|tabs| {
                        tabs.last()
                            .map(|t| t.document.with_untracked(|d| d.id.clone()))
                            .unwrap_or_default()
                    });
                    self.select(next);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn reload(self) {
        let Some(tab) = self.current() else {
            return;
        };
        if tab.dirty() {
            self.c.error.set(
                tr(
                    self.english.get_untracked(),
                    "请先保存或明确放弃此标签页的修改，再重新打开文件。",
                    "Save or discard this tab before reopening the file.",
                )
                .into(),
            );
            return;
        }
        let id = tab.document.with_untracked(|d| d.id.clone());
        self.c.busy.set(true);
        spawn_local(async move {
            match invoke::<()>(
                "workspace_source_close",
                serde_json::json!({"documentId":id,"discard":false}),
            )
            .await
            {
                Ok(()) => {
                    destroy(&id);
                    self.tabs
                        .update(|tabs| tabs.retain(|t| t.document.with_untracked(|d| d.id != id)));
                    tab.release_text();
                    self.c.busy.set(false);
                    self.open(tab.repository, tab.file, None, None);
                }
                Err(e) => {
                    self.c.error.set(e);
                    self.c.busy.set(false);
                }
            }
        });
    }
    fn load(self, repository: String, directory: Option<String>, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<FilePage>("workspace_file_tree",serde_json::json!({"repositoryId":repository,"directory":directory,"offset":offset})).await{Ok(page)=>self.tree.update(|t|{t.insert(key(&repository,directory.as_deref()),page);}),Err(e)=>self.c.error.set(e)}
            self.c.busy.set(false);
        });
    }
    fn check(self) {
        if self.c.busy.get_untracked() || self.tabs.with_untracked(|t| t.is_empty()) {
            return;
        }
        let versions = self.tabs.with_untracked(|tabs| {
            tabs.iter()
                .map(|t| {
                    (
                        t.document.with_untracked(|d| d.id.clone()),
                        t.generation.get_untracked(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        });
        spawn_local(async move {
            if let Ok(statuses) =
                invoke::<Vec<DocumentStatus>>("workspace_source_check", serde_json::json!({})).await
            {
                for status in statuses {
                    if let Some(tab) = self.tab(&status.id)
                        && versions.get(&status.id) == Some(&tab.generation.get_untracked())
                    {
                        tab.changed.set(status.changed);
                    }
                }
            }
        });
    }
}
fn search_groups(s: Controller, english: RwSignal<bool>) -> AnyView {
    type FileGroup = (String, Vec<SearchHit>);
    let mut groups = BTreeMap::<String, (String, BTreeMap<String, FileGroup>)>::new();
    if let Some(results) = s.results.get() {
        for hit in results
            .hits
            .into_iter()
            .skip(s.result_offset.get())
            .take(200)
        {
            groups
                .entry(hit.repository_id.clone())
                .or_insert_with(|| (hit.repository.clone(), BTreeMap::new()))
                .1
                .entry(hit.file_id.clone())
                .or_insert_with(|| (hit.path.clone(), Vec::new()))
                .1
                .push(hit);
        }
    }
    groups.into_iter().map(move |(repo, (name, files))| {
        let path = s.c.catalog.with_untracked(|catalog|catalog.repositories.iter().find(|r|r.id==repo).map(|r|r.path.clone()).unwrap_or_default());
        let count = files.len();
        view!{<details class="source-result-project" open><summary title=path.clone()><strong>{name}</strong><span>{path.clone()}</span><small>{format!("{count} {}",tr(english.get(),"个文件（本页）","files (this page)"))}</small></summary>{files.into_values().map(move |(path,hits)|{
            let count=hits.len();
            view!{<details class="source-result-file" open><summary><strong>{path}</strong><small>{format!("{count} {}",tr(english.get(),"处匹配（本页）","matches (this page)"))}</small></summary>{hits.into_iter().map(move |hit|{
                let repo=hit.repository_id.clone();let file=hit.file_id.clone();
                let location=if hit.line==0{tr(english.get(),"打开文件","Open file").to_string()}else{format!("{}:{}",hit.line,hit.column)};
                view!{<button class="source-hit" disabled=move ||s.c.busy.get() on:click=move |_|s.open(repo.clone(),file.clone(),hit.digest.clone(),Some((hit.offset_utf16,hit.length_utf16)))><span>{location}</span><code>{hit.preview}</code></button>}
            }).collect_view()}</details>}
        }).collect_view()}</details>}
    }).collect_view().into_any()
}
fn nodes(
    c: Controller,
    repository: String,
    directory: Option<String>,
    depth: usize,
    menu: source_menu::State,
) -> AnyView {
    let tree_key = key(&repository, directory.as_deref());
    let page_key = tree_key.clone();
    let count_key = tree_key.clone();
    let repo_files = repository.clone();
    let parent_files = directory.clone();
    let repo_next = repository.clone();
    let dir_next = directory.clone();
    view!{<div role="group"><For each=move ||c.tree.with(|t|t.get(&tree_key).map(|p|p.files.clone()).unwrap_or_default()) key=|f|f.id.clone() children=move |file|{let menu_file=file.clone();let keyboard_file=file.clone();let menu_repo=repo_files.clone();let keyboard_repo=repo_files.clone();let folder=file.kind=="directory";let open_parent=parent_files.clone();let id=file.id.clone();let click_id=id.clone();let exp_id=id.clone();let children_id=id.clone();let repo=repo_files.clone();let children_repo=repo.clone();let label=file.name.clone();let path=file.path.clone();let kind=if folder{Kind::Directory}else if file.kind=="link"{Kind::Link}else{Kind::File};view!{<div class="source-tree-node"><button class="source-tree-row" on:contextmenu=move |ev|{ev.prevent_default();menu.show(c,menu_repo.clone(),menu_file.clone(),ev.as_ref(),false);} on:keydown=move |ev|{if ev.key()=="ContextMenu"||(ev.shift_key()&&ev.key()=="F10"){ev.prevent_default();menu.show(c,keyboard_repo.clone(),keyboard_file.clone(),ev.as_ref(),true);}} style=format!("padding-left:{}px",6+depth*14) title=path disabled=move ||c.c.busy.get()||file.kind=="link"||file.kind=="special" on:click=move |_|{if folder{c.parent.set(Some(click_id.clone()));let expand=!c.expanded.with_untracked(|set|set.contains(&click_id));c.expanded.update(|set|{if expand{set.insert(click_id.clone());}else{set.remove(&click_id);}});if expand{c.load(repo.clone(),Some(click_id.clone()),0);}}else{c.parent.set(open_parent.clone());c.open(repo.clone(),click_id.clone(),None,None);}}><span class="source-expand">{move ||if folder{if c.expanded.with(|s|s.contains(&exp_id)){"▾"}else{"▸"}}else{""}}</span>{entry_icon::icon(&kind,&label,false)}<span>{label}</span></button><Show when=move ||folder&&c.expanded.with(|s|s.contains(&children_id))>{nodes(c,children_repo.clone(),Some(id.clone()),depth+1,menu)}</Show></div>}}/>
        <div class="source-tree-paging" style:display=move ||if c.tree.with(|t|t.get(&count_key).is_some_and(|p|p.total>300)){"flex"}else{"none"}><button class="quiet" disabled=move ||c.c.busy.get()||c.tree.with(|t|t.get(&page_key).is_none_or(|p|p.offset==0)) on:click={let repo=repository.clone();let directory=directory.clone();let key=key(&repository,directory.as_deref());move |_|{let offset=c.tree.with_untracked(|t|t.get(&key).map(|p|p.offset.saturating_sub(300)).unwrap_or_default());c.load(repo.clone(),directory.clone(),offset);}}>"‹"</button><button class="quiet" disabled={let key=key(&repository,directory.as_deref());move ||c.c.busy.get()||c.tree.with(|t|t.get(&key).is_none_or(|p|p.offset+300>=p.total))} on:click={let key=key(&repository,directory.as_deref());move |_|{let offset=c.tree.with_untracked(|t|t.get(&key).map(|p|p.offset+300).unwrap_or_default());c.load(repo_next.clone(),dir_next.clone(),offset);}}>"›"</button></div>
    </div>}.into_any()
}
pub fn panel(
    c: git_actions::Controller,
    visible: RwSignal<bool>,
    english: RwSignal<bool>,
    back: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let menu = source_menu::State::new();
    let s = Controller {
        c,
        english,
        tabs: RwSignal::new(Vec::new()),
        active: RwSignal::new(String::new()),
        host: NodeRef::new(),
        tree: RwSignal::new(BTreeMap::new()),
        expanded: RwSignal::new(BTreeSet::new()),
        parent: RwSignal::new(None),
        close: RwSignal::new(None),
        results: RwSignal::new(None),
        result_offset: RwSignal::new(0),
    };
    let repo = Memo::new(move |_| {
        c.selected.with(|r| {
            r.as_ref()
                .filter(|r| r.repository.kind != "bare")
                .map(|r| r.repository.id.clone())
        })
    });
    Effect::new(move |_| {
        let current = repo.get();
        if visible.get()
            && let Some(repo) = current
        {
            s.parent.set(None);
            s.load(repo, None, 0);
        }
    });
    Effect::new(move |_| readonly(c.busy.get()));
    Effect::new(move |_| locale(english.get()));
    let handle = window_event_listener(leptos::ev::focus, move |_| s.check());
    on_cleanup(move || handle.remove());
    let query = RwSignal::new(SourceSearch::default());
    let scope = RwSignal::new("current".to_string());
    Effect::new(move |_| {
        if let Some(id) = repo.get() {
            query.set(c.catalog.with_untracked(|catalog| {
                catalog
                    .file_settings
                    .get(&id)
                    .and_then(|p| p.search.clone())
                    .unwrap_or_default()
            }));
        }
    });
    let save_search = move || {
        let Some(id) = repo.get_untracked() else {
            return;
        };
        if c.busy.get_untracked() {
            return;
        }
        let mut prefs = c
            .catalog
            .with_untracked(|catalog| catalog.file_settings.get(&id).cloned().unwrap_or_default());
        prefs.search = Some(query.get_untracked());
        c.busy.set(true);
        c.error.set(String::new());
        spawn_local(async move {
            match invoke::<FileSettings>(
                "workspace_file_settings",
                serde_json::json!({"repositoryId":id,"settings":prefs}),
            )
            .await
            {
                Ok(value) => c.catalog.update(|catalog| {
                    catalog.file_settings.insert(id, value);
                }),
                Err(e) => c.error.set(e),
            }
            c.busy.set(false);
        });
    };

    let name = RwSignal::new(String::new());
    let folder = RwSignal::new(false);
    let settings_open = RwSignal::new(false);
    let settings = RwSignal::new(FileSettings::default());
    Effect::new(move |_| {
        if visible.get() && repo.get().is_none() && scope.get_untracked() == "current" {
            scope.set("all".into());
        }
    });
    let back_to_git = move || {
        if c.busy.get_untracked() {
            return;
        }
        let Some(id) = repo.get_untracked() else {
            back();
            return;
        };
        c.busy.set(true);
        spawn_local(async move {
            match invoke::<RepositoryState>(
                "workspace_open",
                serde_json::json!({"repositoryId":id}),
            )
            .await
            {
                Ok(state) => c.accept(state),
                Err(e) => c.error.set(e),
            }
            c.busy.set(false);
            back();
        });
    };

    let search = move || {
        if c.busy.get_untracked() {
            return;
        }
        let mut request = query.get_untracked();
        let current = repo.get_untracked();
        request.repositories = if scope.get_untracked() == "all" {
            c.catalog.with_untracked(|c| {
                c.repositories
                    .iter()
                    .filter(|r| r.kind != "bare")
                    .map(|r| r.id.clone())
                    .collect()
            })
        } else {
            current.into_iter().collect()
        };
        request.directory = None;
        request.file = None;
        if scope.get_untracked() == "directory" {
            request.directory = s.parent.get_untracked();
        }
        if scope.get_untracked() == "file"
            && let Some(tab) = s.current()
        {
            request.repositories = vec![tab.repository];
            request.file = Some(tab.file);
        }
        c.busy.set(true);
        c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SourceResults>(
                "workspace_source_search",
                serde_json::json!({"query":request}),
            )
            .await
            {
                Ok(result) => {
                    s.results.set(Some(result));
                    s.result_offset.set(0);
                }
                Err(e) => c.error.set(e),
            }
            c.busy.set(false);
        });
    };
    let create = move || {
        let Some(repo) = repo.get_untracked() else {
            return;
        };
        if c.busy.get_untracked() {
            return;
        }
        let directory = s.parent.get_untracked();
        let value = name.get_untracked();
        let is_folder = folder.get_untracked();
        c.busy.set(true);
        c.error.set(String::new());
        spawn_local(async move {
            match invoke::<WorkspaceFile>("workspace_source_create",serde_json::json!({"repositoryId":repo,"directory":directory,"name":value,"folder":is_folder})).await {Ok(file)=>{name.set(String::new());match invoke::<FilePage>("workspace_file_tree",serde_json::json!({"repositoryId":repo,"directory":directory,"offset":0})).await{Ok(page)=>s.tree.update(|tree|{tree.insert(key(&repo,directory.as_deref()),page);}),Err(e)=>c.error.set(e)}c.busy.set(false);if !is_folder{s.open(repo,file.id,None,None);}return;},Err(e)=>c.error.set(e)}
            c.busy.set(false);
        });
    };
    view! {<section class="source-workspace" style:display=move ||if visible.get(){"flex"}else{"none"} aria-label=move ||tr(english.get(),"文件与搜索","Files and search") on:keydown=move |ev|{if (ev.meta_key()||ev.ctrl_key())&&!ev.alt_key(){if ev.key().eq_ignore_ascii_case("s"){ev.prevent_default();s.save(s.active.get_untracked(),false);}else if ev.shift_key()&&ev.key().eq_ignore_ascii_case("f"){ev.prevent_default();focus_search();}}}>
        <header class="source-toolbar"><select aria-label=move ||tr(english.get(),"文件仓库","File repository") disabled=move ||c.busy.get() prop:value=move ||repo.get().unwrap_or_default() on:change=move |ev|{let id=event_target_value(&ev);if id.is_empty(){c.selected.set(None);return;}c.busy.set(true);spawn_local(async move{match invoke::<RepositoryState>("workspace_open",serde_json::json!({"repositoryId":id})).await{Ok(state)=>c.accept(state),Err(e)=>c.error.set(e)}c.busy.set(false);});}><option value="">{move ||tr(english.get(),"选择仓库","Choose repository")}</option>{move ||c.catalog.with(|c|c.repositories.iter().filter(|r|r.kind!="bare").map(|r|view!{<option value=r.id.clone()>{format!("{} · {}",r.name,r.path)}</option>}).collect_view())}</select><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|back_to_git()>{move ||tr(english.get(),"返回 Git","Back to Git")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{s.check();if let Some(repo)=repo.get_untracked(){s.load(repo,None,0);}}>{move ||tr(english.get(),"刷新文件","Refresh files")}</button><button class="quiet" disabled=move ||repo.get().is_none() on:click=move |_|{if let Some(id)=repo.get_untracked(){settings.set(c.catalog.with_untracked(|c|c.file_settings.get(&id).cloned().unwrap_or_default()));settings_open.update(|v|*v = !*v);}}>{move ||tr(english.get(),"文件规则","File rules")}</button></header>
        <Show when=move ||settings_open.get()><div class="source-rules"><textarea aria-label=move ||tr(english.get(),"仓库文件排除规则","Repository file exclusions") prop:value=move ||settings.with(|s|s.exclusions.clone()) on:input=move |ev|settings.update(|s|s.exclusions=event_target_value(&ev))/><label><input type="checkbox" prop:checked=move ||settings.with(|s|s.show_hidden) on:change=move |ev|settings.update(|s|s.show_hidden=event_target_checked(&ev))/>{move ||tr(english.get(),"显示隐藏文件","Show hidden files")}</label><label><input type="checkbox" prop:checked=move ||settings.with(|s|s.respect_gitignore) on:change=move |ev|settings.update(|s|s.respect_gitignore=event_target_checked(&ev))/>{move ||tr(english.get(),"遵循 Git 忽略规则","Respect Git ignore rules")}</label><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{let Some(id)=repo.get_untracked()else{return;};let mut draft=settings.get_untracked();draft.search=c.catalog.with_untracked(|catalog|catalog.file_settings.get(&id).and_then(|v|v.search.clone()));c.busy.set(true);spawn_local(async move{match invoke::<FileSettings>("workspace_file_settings",serde_json::json!({"repositoryId":id,"settings":draft})).await{Ok(value)=>{c.catalog.update(|c|{c.file_settings.insert(id.clone(),value);});settings_open.set(false);s.parent.set(None);c.busy.set(false);s.load(id,None,0);},Err(e)=>{c.error.set(e);c.busy.set(false);}}});}>{move ||tr(english.get(),"保存规则","Save rules")}</button></div></Show>
        <div class="source-search"><input class="source-search-query" aria-label=move ||tr(english.get(),"全局搜索内容","Global search query") placeholder=move ||tr(english.get(),"搜索名称、路径或文件内容…","Search names, paths or file contents…") prop:value=move ||query.with(|q|q.query.clone()) on:input=move |ev|query.update(|q|q.query=event_target_value(&ev)) on:keydown=move |ev|{if ev.key()=="Enter"{search();}}/><select class="source-search-scope" aria-label=move ||tr(english.get(),"搜索范围","Search scope") prop:value=move ||scope.get() on:change=move |ev|scope.set(event_target_value(&ev))><option value="current">{move ||tr(english.get(),"当前仓库","Current repository")}</option><option value="all">{move ||tr(english.get(),"全部仓库","All repositories")}</option><option value="directory">{move ||tr(english.get(),"选中目录","Selected directory")}</option><option value="file">{move ||tr(english.get(),"当前文件","Current file")}</option></select><select class="source-search-type" aria-label=move ||tr(english.get(),"搜索类型","Search type") prop:value=move ||query.with(|q|q.mode.clone()) on:change=move |ev|query.update(|q|q.mode=event_target_value(&ev))><option value="content">{move ||tr(english.get(),"内容","Contents")}</option><option value="name">{move ||tr(english.get(),"文件名","File name")}</option><option value="path">{move ||tr(english.get(),"完整路径","Full path")}</option></select><button class="quiet" disabled=move ||c.busy.get()||query.with(|q|q.query.is_empty()) on:click=move |_|search()>{move ||tr(english.get(),"搜索文件","Search files")}</button><details><summary>{move ||tr(english.get(),"搜索选项","Search options")}</summary><div>{[("regex","正则","Regex"),("case","区分大小写","Match case"),("word","全词","Whole word"),("multiline","多行","Multiline"),("ignore","遵循 .gitignore","Respect .gitignore")].into_iter().map(move |(key,zh,en)|view!{<label><input type="checkbox" prop:checked=move ||query.with(|q|match key{"regex"=>q.regex,"case"=>q.case_sensitive,"word"=>q.whole_word,"multiline"=>q.multiline,_=>q.respect_gitignore}) on:change=move |ev|{let value=event_target_checked(&ev);query.update(|q|match key{"regex"=>q.regex=value,"case"=>q.case_sensitive=value,"word"=>q.whole_word=value,"multiline"=>q.multiline=value,_=>q.respect_gitignore=value});}/>{move ||tr(english.get(),zh,en)}</label>}).collect_view()}<input aria-label=move ||tr(english.get(),"搜索包含规则","Search include rules") placeholder="*.rs;*.py" prop:value=move ||query.with(|q|q.include.clone()) on:input=move |ev|query.update(|q|q.include=event_target_value(&ev))/><input aria-label=move ||tr(english.get(),"搜索排除规则","Search exclude rules") placeholder="node_modules/;dist/" prop:value=move ||query.with(|q|q.exclusions.clone()) on:input=move |ev|query.update(|q|q.exclusions=event_target_value(&ev))/><button class="quiet" disabled=move ||c.busy.get()||repo.get().is_none() on:click=move |_|save_search()>{move ||tr(english.get(),"保存当前项目搜索设置","Save search settings for this project")}</button></div></details></div>
        {source_menu::panel(s,menu)}
        <div class="source-body"><aside class="source-sidebar"><div class="source-create"><input aria-label=move ||tr(english.get(),"新文件或文件夹名称","New file or folder name") placeholder=move ||tr(english.get(),"新文件或文件夹名称","New file or folder name") prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev))/><label><input type="checkbox" prop:checked=move ||folder.get() on:change=move |ev|folder.set(event_target_checked(&ev))/>{move ||tr(english.get(),"文件夹","Folder")}</label><button class="quiet" disabled=move ||c.busy.get()||repo.get().is_none()||name.get().is_empty() on:click=move |_|create()>{move ||tr(english.get(),"创建","Create")}</button><button class="quiet" on:click=move |_|s.parent.set(None)>{move ||tr(english.get(),"选择根目录","Select root")}</button><small>{move ||s.parent.get().map(|id|format!("{}: {}",tr(english.get(),"创建于","Create in"),s.tree.with(|tree|tree.values().flat_map(|p|p.files.iter()).find(|f|f.id==id).map(|f|f.path.clone()).unwrap_or_default()))).unwrap_or_else(||tr(english.get(),"创建于仓库根目录","Create in repository root").into())}</small></div><div class="source-tree">{move ||repo.get().map(|repo|nodes(s,repo,None,0,menu))}</div></aside>
        <main class="source-editing"><nav class="source-tabs" aria-label=move ||tr(english.get(),"打开的文件","Open files")><For each=move ||s.tabs.get() key=|t|t.document.with_untracked(|d|d.id.clone()) children=move |tab|{let id=tab.document.with_untracked(|d|d.id.clone());let select=id.clone();let close=id.clone();let chosen=id.clone();let metadata=tab.document;view!{<div><button class="quiet" aria-pressed=move ||s.active.get()==chosen title=move ||metadata.with(|d|d.absolute_path.clone()) on:click=move |_|s.select(select.clone())>{move ||metadata.with(|d|d.path.rsplit('/').next().unwrap_or(&d.path).to_string())}{move ||if tab.dirty(){" •"}else{""}}</button><button class="quiet" aria-label=move ||tr(english.get(),"关闭文件标签","Close file tab") disabled=move ||c.busy.get() on:click=move |_|s.close_tab(close.clone(),false)>"×"</button></div>}}/></nav>
        <div class="source-editor-tools"><button class="quiet" disabled=move ||c.busy.get()||s.current().is_none() on:click=move |_|s.save(s.active.get_untracked(),false)>{move ||tr(english.get(),"保存当前文件","Save current file")}</button>{[("undo","撤销","Undo"),("redo","重做","Redo"),("find","文件内查找替换","Find/replace in file"),("goto","跳转行","Go to line")].into_iter().map(move |(name,zh,en)|view!{<button class="quiet" disabled=move ||c.busy.get()||s.active.get().is_empty() on:click=move |_|command(&s.active.get_untracked(),name)>{move ||tr(english.get(),zh,en)}</button>}).collect_view()}<button class="quiet" disabled=move ||c.busy.get()||s.current().is_none() on:click=move |_|s.reload()>{move ||tr(english.get(),"重新载入","Reload")}</button></div>
        <Show when=move ||s.current().is_some_and(|t|t.changed.get())><p class="source-external" role="status">{move ||tr(english.get(),"文件在外部发生变化。未保存内容仍保留；保存前请核对或重新载入。","The file changed outside the editor. Your buffer is retained; review or reload before saving.")}</p></Show>
        <div class="source-code-host" node_ref=s.host></div><Show when=move ||s.tabs.with(|t|t.is_empty())><p class="source-empty">{move ||tr(english.get(),"从左侧文件树或搜索结果打开文件。","Open a file from the tree or search results.")}</p></Show>
        <footer class="source-format">{move ||s.current().map(|tab|{let encoding=tab.clone();let ending=tab.clone();let bom=tab.clone();let eo=encoding.options;let lo=ending.options;let bo=bom.options;view!{<span>{move ||tab.document.with(|d|d.absolute_path.clone())}</span><span class="source-format-meta">{move ||tab.document.with(|d|format!("{} · {} · {}",match d.encoding{Encoding::Utf8=>"UTF-8",Encoding::Utf16Le=>"UTF-16 LE",Encoding::Utf16Be=>"UTF-16 BE"},match d.line_ending{LineEnding::Lf=>"LF",LineEnding::CrLf=>"CRLF",LineEnding::Cr=>"CR",LineEnding::Mixed=>tr(english.get(),"混合换行","Mixed endings")},if d.bom{"BOM"}else{tr(english.get(),"无 BOM","No BOM")}))}</span><select disabled=move ||c.busy.get() aria-label=move ||tr(english.get(),"保存编码","Save encoding") prop:value=move ||eo.with(|o|match o.encoding{Some(Encoding::Utf8)=>"utf8",Some(Encoding::Utf16Le)=>"utf16_le",Some(Encoding::Utf16Be)=>"utf16_be",None=>""}) on:change=move |ev|{let value=event_target_value(&ev);encoding.options.update(|o|o.encoding=match value.as_str(){"utf8"=>Some(Encoding::Utf8),"utf16_le"=>Some(Encoding::Utf16Le),"utf16_be"=>Some(Encoding::Utf16Be),_=>None});s.dirty(encoding.clone());}><option value="">{move ||tr(english.get(),"保留编码","Preserve encoding")}</option><option value="utf8">"UTF-8"</option><option value="utf16_le">"UTF-16 LE"</option><option value="utf16_be">"UTF-16 BE"</option></select><select disabled=move ||c.busy.get() aria-label=move ||tr(english.get(),"保存换行格式","Save line endings") prop:value=move ||lo.with(|o|match o.line_ending{Some(LineEnding::Lf)=>"lf",Some(LineEnding::CrLf)=>"crlf",Some(LineEnding::Cr)=>"cr",_=>""}) on:change=move |ev|{let value=event_target_value(&ev);ending.options.update(|o|o.line_ending=match value.as_str(){"lf"=>Some(LineEnding::Lf),"crlf"=>Some(LineEnding::CrLf),"cr"=>Some(LineEnding::Cr),_=>None});s.dirty(ending.clone());}><option value="">{move ||tr(english.get(),"保留换行","Preserve endings")}</option><option value="lf">"LF"</option><option value="crlf">"CRLF"</option><option value="cr">"CR"</option></select><label>"BOM"<select disabled=move ||c.busy.get() aria-label="BOM" prop:value=move ||bo.with(|o|match o.bom{Some(true)=>"yes",Some(false)=>"no",None=>""}) on:change=move |ev|{bom.options.update(|o|o.bom=match event_target_value(&ev).as_str(){"yes"=>Some(true),"no"=>Some(false),_=>None});s.dirty(bom.clone());}><option value="">{move ||tr(english.get(),"保留","Preserve")}</option><option value="yes">{move ||tr(english.get(),"包含","Include")}</option><option value="no">{move ||tr(english.get(),"不包含","Exclude")}</option></select></label>}})}</footer>
        <Show when=move ||s.close.with(|c|c.is_some())><div class="source-close-review" role="region" aria-label=move ||tr(english.get(),"未保存文件","Unsaved file")><span>{move ||tr(english.get(),"此标签页有未保存修改。","This tab has unsaved changes.")}</span><button class="primary" disabled=move ||c.busy.get() on:click=move |_|{if let Some(id)=s.close.get_untracked(){s.save(id,true);}}>{move ||tr(english.get(),"保存并关闭","Save and close")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if let Some(id)=s.close.get_untracked(){s.close_tab(id,true);}}>{move ||tr(english.get(),"放弃并关闭","Discard and close")}</button><button class="quiet" on:click=move |_|s.close.set(None)>{move ||tr(english.get(),"取消关闭","Cancel close")}</button></div></Show>
        </main></div>
        <Show when=move ||s.results.with(|r|r.is_some())><section class="source-results"><header><strong>{move ||s.results.with(|r|r.as_ref().map(|r|format!("{} {} · {} {} · {} {}{}{}",r.hits.len(),tr(english.get(),"处匹配","matches"),r.searched_files,tr(english.get(),"个文件已搜索","files searched"),r.skipped_files,tr(english.get(),"个文件跳过","files skipped"),if r.truncated{tr(english.get()," · 达到搜索上限"," · search limit reached")}else{""},if r.cancelled{tr(english.get()," · 已取消，保留结果"," · cancelled, results retained")}else{""})).unwrap_or_default())}</strong><button class="quiet" on:click=move |_|s.results.set(None)>{move ||tr(english.get(),"关闭搜索结果","Close search results")}</button></header><details class="source-search-warnings" style:display=move ||if s.results.with(|r|r.as_ref().is_some_and(|r|!r.warnings.is_empty())){"block"}else{"none"}><summary>{move ||tr(english.get(),"搜索警告","Search warnings")}</summary><pre>{move ||s.results.with(|r|r.as_ref().map(|r|r.warnings.join("\n")).unwrap_or_default())}</pre></details><div class="source-result-groups">{move ||search_groups(s,english)}</div><footer><button class="quiet" disabled=move ||s.result_offset.get()==0 on:click=move |_|s.result_offset.update(|o|*o=o.saturating_sub(200))>"‹"</button><button class="quiet" disabled=move ||s.results.with(|r|r.as_ref().is_none_or(|r|s.result_offset.get()+200>=r.hits.len())) on:click=move |_|s.result_offset.update(|o|*o+=200)>"›"</button></footer></section></Show>
    </section>}
}
