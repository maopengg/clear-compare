//! Linked worktree manager; all mutations use captured IDs and common Git review.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Trees {
    c: Controller,
    list: RwSignal<Option<WorktreeList>>,
    attempted: RwSignal<String>,
    query: RwSignal<String>,
    offset: RwSignal<usize>,
    destination: RwSignal<Option<WorktreeDestination>>,
    folder: RwSignal<String>,
    mode: RwSignal<String>,
    branch: RwSignal<String>,
    reason: RwSignal<String>,
    pending: RwSignal<bool>,
    pending_create: RwSignal<bool>,
    creating: RwSignal<bool>,
    expected: RwSignal<u64>,
}
impl Trees {
    fn locked(self) -> bool {
        self.c.busy.get() || self.c.plan.with(|p| p.is_some())
    }
    fn load(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.attempted.set(state.repository.id.clone());
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<WorktreeList>(
                "workspace_worktrees",
                serde_json::json!({"repositoryId":state.repository.id}),
            )
            .await
            {
                Ok(list) => {
                    let mut catalog = list.catalog.clone();
                    catalog.projects = self.c.catalog.with_untracked(|c| c.projects.clone());
                    self.c.catalog.set(catalog);
                    self.c.accept(list.state.clone());
                    self.list.set(Some(list));
                    self.offset.set(0)
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn choose(self) {
        if self.locked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<Option<WorktreeDestination>>(
                "workspace_worktree_destination",
                serde_json::json!({"repositoryId":state.repository.id}),
            )
            .await
            {
                Ok(Some(d)) => self.destination.set(Some(d)),
                Ok(None) => {}
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn run(self, action: Action) {
        if self.locked() {
            return;
        }
        self.pending.set(true);
        self.pending_create
            .set(matches!(&action, Action::CreateWorktree { .. }));
        self.expected
            .set(self.c.epoch.get_untracked().wrapping_add(1));
        self.c.run(action);
    }
    fn create(self) {
        let Some(d) = self.destination.get_untracked() else {
            return;
        };
        let mode = self.mode.get_untracked();
        self.run(Action::CreateWorktree {
            destination_id: d.id,
            folder: self.folder.get_untracked(),
            reference_id: self.c.worktree_start.get_untracked(),
            new_branch: (mode == "new").then(|| self.branch.get_untracked()),
            detach: mode == "detached",
        });
    }
    fn open(self, id: String, reveal: bool) {
        if self.locked() {
            return;
        }
        let Some(s) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            let args = serde_json::json!({"repositoryId":s.repository.id,"worktreeId":id});
            if reveal {
                if let Err(e) = invoke::<()>("workspace_worktree_reveal", args).await {
                    self.c.error.set(e)
                }
            } else {
                match invoke::<WorktreeOpen>("workspace_worktree_open", args).await {
                    Ok(result) => {
                        self.c.catalog.set(result.catalog);
                        self.c.plan.set(None);
                        self.c.accept(result.state);
                        self.c.worktrees.set(false)
                    }
                    Err(e) => self.c.error.set(e),
                }
            }
            self.c.busy.set(false);
        });
    }
}
fn entry(e: WorktreeEntry, v: Trees, en: RwSignal<bool>) -> impl IntoView {
    let id = StoredValue::new(e.id.clone());
    let locked = e.locked.is_some();
    let protected = e.main || e.current || !e.available;
    view! {<tr><td><strong title=e.path.clone()>{e.path.clone()}</strong><small>{move ||if e.main{tr(en.get(),"主工作区","Main worktree")}else if e.current{tr(en.get(),"当前工作区","Current worktree")}else{tr(en.get(),"关联工作区","Linked worktree")}}</small></td><td><span>{e.branch.as_deref().unwrap_or("").trim_start_matches("refs/heads/").to_string()}</span><Show when=move ||e.detached><span>{move ||tr(en.get(),"分离 HEAD","Detached HEAD")}</span></Show><code>{e.head.as_ref().map(|h|h.chars().take(8).collect::<String>()).unwrap_or_default()}</code></td><td><Show when=move ||e.bare><span>{move ||tr(en.get(),"裸仓库","Bare repository")}</span></Show><Show when=move ||!e.available><span>{move ||tr(en.get(),"路径不可用","Path unavailable")}</span></Show>{e.locked.map(move |reason|view!{<span title=reason>{move ||tr(en.get(),"已锁定","Locked")}</span>})}{e.prunable.map(move |reason|view!{<span title=reason>{move ||tr(en.get(),"失效记录","Stale registration")}</span>})}</td><td><button class="quiet" disabled=move ||v.locked()||!e.available on:click=move |_|v.open(id.get_value(),false)>{move ||tr(en.get(),"打开","Open")}</button><button class="quiet" disabled=move ||v.locked()||!e.available on:click=move |_|v.open(id.get_value(),true)>{move ||tr(en.get(),"系统中显示","Show in file manager")}</button><button class="quiet" disabled=move ||v.locked()||protected on:click=move |_|v.run(Action::SetWorktreeLock{id:id.get_value(),reason:(!locked).then(||v.reason.get_untracked())})>{move ||if locked{tr(en.get(),"解锁","Unlock")}else{tr(en.get(),"锁定","Lock")}}</button><button class="quiet" disabled=move ||v.locked()||protected||locked on:click=move |_|v.run(Action::RemoveWorktree{id:id.get_value()})>{move ||tr(en.get(),"移除工作区","Remove worktree")}</button></td></tr>}
}
pub fn panel(c: Controller, en: RwSignal<bool>) -> impl IntoView {
    let v = Trees {
        c,
        list: RwSignal::new(None),
        attempted: RwSignal::new(String::new()),
        query: RwSignal::new(String::new()),
        offset: RwSignal::new(0),
        destination: RwSignal::new(None),
        folder: RwSignal::new(String::new()),
        mode: RwSignal::new("new".into()),
        branch: RwSignal::new(String::new()),
        reason: RwSignal::new(String::new()),
        pending: RwSignal::new(false),
        pending_create: RwSignal::new(false),
        creating: RwSignal::new(false),
        expected: RwSignal::new(0),
    };
    let repository = Memo::new(move |_| {
        c.selected.with(|s| {
            s.as_ref()
                .map(|s| s.repository.id.clone())
                .unwrap_or_default()
        })
    });
    Effect::watch(
        move || repository.get(),
        move |_, _, _| {
            c.worktrees.set(false);
            c.worktree_start.set(None);
            v.list.set(None);
            v.attempted.set(String::new());
            v.destination.set(None);
            v.folder.set(String::new());
            v.branch.set(String::new());
            v.pending.set(false);
            v.creating.set(false);
        },
        false,
    );
    Effect::new(move || {
        if c.worktrees.get() && !c.busy.get() && v.attempted.get() != repository.get() {
            v.load();
        }
    });
    Effect::new(move || {
        if !v.pending.get() {
            return;
        }
        if v.expected.get() != c.epoch.get() {
            v.pending.set(false);
            return;
        }
        if !c.busy.get()
            && v.expected.get() == c.completion_epoch.get()
            && let Some(ok) = c.completion.get()
        {
            v.pending.set(false);
            if ok {
                if v.pending_create.get_untracked() {
                    v.folder.set(String::new());
                }
                v.load();
            }
        }
    });
    Effect::watch(
        move || (c.worktrees.get(), c.worktree_start.get()),
        move |(visible, start), _, _| {
            if *visible && start.is_some() {
                v.creating.set(true);
            }
        },
        false,
    );
    let filtered = Memo::new(move |_| {
        let q = v.query.get().to_lowercase();
        v.list.with(|l| {
            l.as_ref()
                .map(|l| {
                    l.entries
                        .iter()
                        .filter(|e| {
                            format!("{} {}", e.path, e.branch.as_deref().unwrap_or(""))
                                .to_lowercase()
                                .contains(&q)
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    view! {<Show when=move ||c.worktrees.get()><section class="git-worktrees" aria-label=move ||tr(en.get(),"工作区管理","Worktree manager")>
    <header><strong>{move ||tr(en.get(),"关联工作区（Worktree）","Linked worktrees")}</strong><button class="quiet" disabled=move ||v.locked() on:click=move |_|v.load()>{move ||tr(en.get(),"刷新工作区","Refresh worktrees")}</button><button class="quiet" disabled=move ||v.locked()||v.list.with(|l|l.as_ref().is_none_or(|l|!l.entries.iter().any(|e|e.prunable.is_some()&&e.locked.is_none()))) on:click=move |_|v.run(Action::PruneWorktrees)>{move ||tr(en.get(),"清理失效记录…","Prune stale registrations…")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if v.pending.get_untracked(){c.plan.set(None);c.completion_epoch.set(c.epoch.get_untracked());c.completion.set(Some(false));}c.worktrees.set(false)}>{move ||tr(en.get(),"返回仓库","Back to repository")}</button></header>
    <div class="git-worktrees-tools"><input aria-label=move ||tr(en.get(),"筛选工作区","Filter worktrees") placeholder=move ||tr(en.get(),"按路径或分支筛选…","Filter path or branch…") prop:value=move ||v.query.get() on:input=move |ev|{v.query.set(event_target_value(&ev));v.offset.set(0);}/><label>{move ||tr(en.get(),"锁定说明","Lock reason")}<input aria-label=move ||tr(en.get(),"工作区锁定说明","Worktree lock reason") disabled=move ||v.locked() prop:value=move ||v.reason.get() on:input=move |ev|v.reason.set(event_target_value(&ev))/></label><span>{move ||v.list.with(|l|l.as_ref().map(|l|format!("{} {} · {} {}",l.entries.len(),tr(en.get(),"个工作区","worktrees"),l.entries.iter().filter(|e|e.prunable.is_some()&&e.locked.is_none()).count(),tr(en.get(),"条可清理记录","prunable registrations"))))}</span></div>
    <details class="git-worktree-create" prop:open=move ||v.creating.get()><summary>{move ||tr(en.get(),"创建工作区","Create worktree")}</summary><div><button class="quiet" disabled=move ||v.locked() on:click=move |_|v.choose()>{move ||tr(en.get(),"选择父目录…","Choose parent directory…")}</button><code>{move ||v.destination.with(|d|d.as_ref().map(|d|d.path.clone()).unwrap_or_default())}</code><label>{move ||tr(en.get(),"新文件夹名称","New folder name")}<input aria-label=move ||tr(en.get(),"工作区文件夹名称","Worktree folder name") disabled=move ||v.locked() prop:value=move ||v.folder.get() on:input=move |ev|v.folder.set(event_target_value(&ev))/></label><label>{move ||tr(en.get(),"起始版本","Starting revision")}<select aria-label=move ||tr(en.get(),"工作区起始版本","Worktree starting revision") disabled=move ||v.locked() prop:value=move ||c.worktree_start.get().unwrap_or_default() on:change=move |ev|{let s=event_target_value(&ev);c.worktree_start.set((!s.is_empty()).then_some(s))}><option value="">"HEAD"</option>{move ||c.selected.with(|s|s.as_ref().map(|s|s.references.iter().map(|r|view!{<option value=r.id.clone()>{r.full_name.clone()}</option>}).collect_view()))}</select></label><label>{move ||tr(en.get(),"检出方式","Checkout mode")}<select aria-label=move ||tr(en.get(),"工作区检出方式","Worktree checkout mode") disabled=move ||v.locked() prop:value=move ||v.mode.get() on:change=move |ev|v.mode.set(event_target_value(&ev))><option value="new">{move ||tr(en.get(),"创建新分支","Create new branch")}</option><option value="existing">{move ||tr(en.get(),"已有本地分支","Existing local branch")}</option><option value="detached">{move ||tr(en.get(),"分离 HEAD","Detached HEAD")}</option></select></label><Show when=move ||v.mode.get()=="new"><label>{move ||tr(en.get(),"新分支名称","New branch name")}<input aria-label=move ||tr(en.get(),"工作区新分支名称","New worktree branch name") disabled=move ||v.locked() prop:value=move ||v.branch.get() on:input=move |ev|v.branch.set(event_target_value(&ev))/></label></Show><button class="primary" disabled=move ||v.locked()||v.destination.with(|d|d.is_none())||v.folder.get().trim().is_empty()||(v.mode.get()=="new"&&v.branch.get().trim().is_empty()) on:click=move |_|v.create()>{move ||tr(en.get(),"检查并创建工作区","Review and create worktree")}</button></div></details>
    <p class="git-worktree-note">{move ||tr(en.get(),"移除会删除所选工作区目录，保留分支与提交；修改、未跟踪或忽略文件及打开的编辑标签会阻止移除。清理只删除失效注册，已锁定记录保留。","Removal deletes the selected checkout directory, retaining branches and commits. Changes, untracked/ignored files and open editor tabs block removal. Pruning removes stale registrations, retaining locked ones.")}</p>
    <div class="git-worktree-scroll"><table><thead><tr><th>{move ||tr(en.get(),"路径","Path")}</th><th>{move ||tr(en.get(),"分支 / 提交","Branch / commit")}</th><th>{move ||tr(en.get(),"状态","Status")}</th><th>{move ||tr(en.get(),"操作","Actions")}</th></tr></thead><tbody><For each=move ||filtered.with(|e|e.iter().skip(v.offset.get()).take(200).cloned().collect::<Vec<_>>()) key=|e|(e.id.clone(),e.available) children=move |e|entry(e,v,en)/></tbody></table><Show when=move ||v.list.with(|l|l.is_some())&&filtered.with(|e|e.is_empty())><p>{move ||tr(en.get(),"没有匹配的工作区。","No matching worktrees.")}</p></Show></div>
    <footer><span>{move ||filtered.with(|e|format!("{}–{} / {}",v.offset.get()+usize::from(!e.is_empty()),(v.offset.get()+200).min(e.len()),e.len()))}</span><button class="quiet" aria-label=move ||tr(en.get(),"上一页工作区","Previous worktrees") disabled=move ||v.offset.get()==0 on:click=move |_|v.offset.update(|n|*n=n.saturating_sub(200))>"‹"</button><button class="quiet" aria-label=move ||tr(en.get(),"下一页工作区","Next worktrees") disabled=move ||filtered.with(|e|v.offset.get()+200>=e.len()) on:click=move |_|v.offset.update(|n|*n+=200)>"›"</button></footer>
    </section></Show>}
}
