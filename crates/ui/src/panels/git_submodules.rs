//! Recursive submodule status and explicit reviewed registration / pinned checkout.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Modules {
    c: Controller,
    list: RwSignal<Option<SubmoduleList>>,
    query: RwSignal<String>,
    offset: RwSignal<usize>,
    pending: RwSignal<bool>,
    expected: RwSignal<u64>,
}
impl Modules {
    fn locked(self) -> bool {
        self.c.busy.get() || self.c.plan.with(|p| p.is_some())
    }
    fn load(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(s) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SubmoduleList>(
                "workspace_submodules",
                serde_json::json!({"repositoryId":s.repository.id}),
            )
            .await
            {
                Ok(l) => {
                    self.c.accept(l.state.clone());
                    self.list.set(Some(l));
                    self.offset.set(0)
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn run(self, a: Action) {
        if self.locked() {
            return;
        }
        self.pending.set(true);
        self.expected
            .set(self.c.epoch.get_untracked().wrapping_add(1));
        self.c.run(a);
    }
    fn open(self, id: String) {
        if self.locked() {
            return;
        }
        let Some(s) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<WorktreeOpen>(
                "workspace_submodule_open",
                serde_json::json!({"repositoryId":s.repository.id,"submoduleId":id}),
            )
            .await
            {
                Ok(result) => {
                    let mut catalog = result.catalog;
                    catalog.projects = self.c.catalog.with_untracked(|c| c.projects.clone());
                    self.c.catalog.set(catalog);
                    self.c.plan.set(None);
                    self.c.accept(result.state);
                    self.c.submodules.set(false)
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
}
fn status(en: bool, value: &str) -> &'static str {
    match value {
        "current" => tr(en, "与父项目一致", "At recorded commit"),
        "different_commit" => tr(en, "提交与记录不同", "Different commit"),
        "dirty" => tr(en, "有本地修改", "Local changes"),
        "conflicted" => tr(en, "Gitlink 冲突", "Gitlink conflict"),
        "uninitialized" => tr(en, "未初始化", "Not initialized"),
        "missing_configuration" => tr(en, "配置或 Gitlink 缺失", "Missing config or gitlink"),
        _ => tr(en, "需检查", "Needs attention"),
    }
}
fn issue_text(en: bool, value: &str) -> String {
    match value {
        "Uninitialized submodule directory contains files" => tr(
            en,
            "未初始化目录中已有文件，请先处理。",
            "Uninitialized submodule directory contains files",
        ),
        "Submodule Git metadata cannot be a symbolic link" => tr(
            en,
            "子模块 Git 数据不能是符号链接。",
            "Submodule Git metadata cannot be a symbolic link",
        ),
        "Submodule repository mapping is invalid" => tr(
            en,
            "子模块仓库位置无效，请刷新检查。",
            "Submodule repository mapping is invalid",
        ),
        _ => value,
    }
    .to_string()
}
fn row(e: SubmoduleEntry, v: Modules, en: RwSignal<bool>) -> impl IntoView {
    let id = StoredValue::new(e.id.clone());
    let disabled =
        e.issue.is_some() || matches!(e.status.as_str(), "conflicted" | "missing_configuration");
    let state = StoredValue::new(e.status.clone());
    let configured = StoredValue::new(e.configured_url.clone());
    let actual = StoredValue::new(e.actual_url.clone());
    let primary = actual
        .get_value()
        .or(configured.get_value())
        .unwrap_or_else(|| e.declared_url.clone());
    let issue = StoredValue::new(e.issue.clone());
    view! {<tr><td><strong style=format!("padding-left:{}px",e.depth*12) title=e.name>{e.path}</strong><small>{move ||if e.registered{tr(en.get(),"已注册","Registered")}else{tr(en.get(),"未注册","Not registered")}}</small></td><td><span class:warning=e.status!="current">{move ||status(en.get(),&state.get_value())}</span><small>{move ||format!("{} {}",e.changes,tr(en.get(),"处修改/忽略项","changes / ignored items"))}</small><Show when=move ||issue.with_value(|i|i.is_some())><p class="git-submodule-issue">{move ||issue.with_value(|i|i.as_ref().map(|i|issue_text(en.get(),i)))}</p></Show></td><td><label>{move ||tr(en.get(),"父项目记录","Recorded")}<code title=e.expected.clone().unwrap_or_default()>{e.expected.as_ref().map(|s|s.chars().take(8).collect::<String>()).unwrap_or_else(||"—".into())}</code></label><label>"HEAD"<code title=e.head.clone().unwrap_or_default()>{e.head.as_ref().map(|s|s.chars().take(8).collect::<String>()).unwrap_or_else(||"—".into())}</code></label></td><td><code class="git-submodule-url" title=primary.clone()>{primary.clone()}</code><details class="git-submodule-addresses"><summary>{move ||tr(en.get(),"地址详情","URL details")}</summary><small>{move ||tr(en.get(),"声明地址","Declared URL")}</small><code>{e.declared_url}</code><Show when=move ||configured.with_value(|v|v.is_some())><small>{move ||tr(en.get(),"注册地址","Registered URL")}</small><code>{configured.get_value()}</code></Show><Show when=move ||actual.with_value(|v|v.is_some())><small>{move ||tr(en.get(),"当前 origin / 首个远程","Current origin / first remote")}</small><code>{actual.get_value()}</code></Show></details></td><td><button class="quiet" disabled=move ||v.locked()||!e.initialized on:click=move |_|v.open(id.get_value())>{move ||tr(en.get(),"打开仓库","Open repository")}</button><button class="quiet" disabled=move ||v.locked()||disabled on:click=move |_|v.run(Action::InitializeSubmodules{id:Some(id.get_value())})>{move ||tr(en.get(),"初始化注册…","Initialize registration…")}</button><button class="quiet" disabled=move ||v.locked()||disabled on:click=move |_|v.run(Action::UpdateSubmodules{id:Some(id.get_value()),recursive:false})>{move ||tr(en.get(),"更新…","Update…")}</button><button class="quiet" disabled=move ||v.locked()||disabled on:click=move |_|v.run(Action::UpdateSubmodules{id:Some(id.get_value()),recursive:true})>{move ||tr(en.get(),"递归更新…","Update recursively…")}</button></td></tr>}
}
pub fn panel(c: Controller, en: RwSignal<bool>) -> impl IntoView {
    let v = Modules {
        c,
        list: RwSignal::new(None),
        query: RwSignal::new(String::new()),
        offset: RwSignal::new(0),
        pending: RwSignal::new(false),
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
            c.submodules.set(false);
            v.list.set(None);
            v.query.set(String::new());
            v.pending.set(false)
        },
        false,
    );
    Effect::watch(
        move || {
            c.comparison.with(|r| r.is_some())
                || c.stashes.get()
                || c.remotes.get()
                || c.worktrees.get()
        },
        move |other, _, _| {
            if *other {
                c.submodules.set(false)
            }
        },
        false,
    );
    Effect::watch(
        move || c.submodules.get(),
        move |open, _, _| {
            if *open && !c.busy.get_untracked() {
                v.load();
            }
        },
        false,
    );
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
                v.load()
            }
        }
    });
    let filtered = Memo::new(move |_| {
        let q = v.query.get().to_lowercase();
        v.list.with(|l| {
            l.as_ref()
                .map(|l| {
                    l.entries
                        .iter()
                        .filter(|e| {
                            format!("{} {} {}", e.path, e.name, e.declared_url)
                                .to_lowercase()
                                .contains(&q)
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    view! {<Show when=move ||c.submodules.get()><section class="git-submodules" aria-label=move ||tr(en.get(),"子模块管理","Submodule manager")><header><strong>{move ||tr(en.get(),"子模块管理","Submodules")}</strong><input aria-label=move ||tr(en.get(),"筛选子模块","Filter submodules") placeholder=move ||tr(en.get(),"路径、名称或地址…","Path, name or URL…") prop:value=move ||v.query.get() on:input=move |ev|{v.query.set(event_target_value(&ev));v.offset.set(0)}/><button class="quiet" disabled=move ||v.locked() on:click=move |_|v.load()>{move ||tr(en.get(),"刷新子模块","Refresh submodules")}</button><button class="quiet" disabled=move ||v.locked()||v.list.with(|l|l.as_ref().is_none_or(|l|l.entries.is_empty())) on:click=move |_|v.run(Action::InitializeSubmodules{id:None})>{move ||tr(en.get(),"初始化全部注册…","Initialize all registrations…")}</button><button class="quiet" disabled=move ||v.locked()||v.list.with(|l|l.as_ref().is_none_or(|l|l.entries.is_empty())) on:click=move |_|v.run(Action::UpdateSubmodules{id:None,recursive:false})>{move ||tr(en.get(),"更新全部…","Update all…")}</button><button class="quiet" disabled=move ||v.locked()||v.list.with(|l|l.as_ref().is_none_or(|l|l.entries.is_empty())) on:click=move |_|v.run(Action::UpdateSubmodules{id:None,recursive:true})>{move ||tr(en.get(),"递归更新全部…","Update all recursively…")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if v.pending.get_untracked(){c.plan.set(None);c.completion_epoch.set(c.epoch.get_untracked());c.completion.set(Some(false));}c.submodules.set(false)}>{move ||tr(en.get(),"返回仓库","Back to repository")}</button></header>
    <p>{move ||tr(en.get(),"初始化注册只保存本机配置；更新会初始化、按父项目记录的提交检出。递归更新包含后续发现的子模块。不会强制覆盖修改；失败或取消可能留下已完成步骤，请刷新检查。","Registration only writes local config. Updates initialize and check out commits recorded by the parent; recursive updates include newly discovered modules. Changes are not forced over. Failure or cancellation may leave completed steps; refresh to inspect.")}</p>
    <div class="git-submodule-scroll"><table><thead><tr><th>{move ||tr(en.get(),"子模块","Submodule")}</th><th>{move ||tr(en.get(),"状态","Status")}</th><th>{move ||tr(en.get(),"提交","Commits")}</th><th>{move ||tr(en.get(),"地址","URLs")}</th><th>{move ||tr(en.get(),"操作","Actions")}</th></tr></thead><tbody><For each=move ||filtered.with(|e|e.iter().skip(v.offset.get()).take(200).cloned().collect::<Vec<_>>()) key=|e|e.id.clone() children=move |e|row(e,v,en)/></tbody></table><Show when=move ||v.list.with(|l|l.is_some())&&filtered.with(|e|e.is_empty())><p>{move ||tr(en.get(),"没有匹配的子模块。","No matching submodules.")}</p></Show></div><footer><span>{move ||filtered.with(|e|format!("{}–{} / {}",v.offset.get()+usize::from(!e.is_empty()),(v.offset.get()+200).min(e.len()),e.len()))}</span><button class="quiet" aria-label=move ||tr(en.get(),"上一页子模块","Previous submodules") disabled=move ||v.offset.get()==0 on:click=move |_|v.offset.update(|n|*n=n.saturating_sub(200))>"‹"</button><button class="quiet" aria-label=move ||tr(en.get(),"下一页子模块","Next submodules") disabled=move ||filtered.with(|e|v.offset.get()+200>=e.len()) on:click=move |_|v.offset.update(|n|*n+=200)>"›"</button></footer></section></Show>}
}
