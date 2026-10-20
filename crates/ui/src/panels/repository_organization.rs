//! Presentation grouping over native repository identities and saved assignments.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(
    inline_js = "let organizationAnchor;export function organizationFocus(){organizationAnchor=document.activeElement;requestAnimationFrame(()=>document.querySelector('.organization-settings input')?.focus());}export function organizationRestore(){if(organizationAnchor?.isConnected)organizationAnchor.focus();}"
)]
extern "C" {
    #[wasm_bindgen(js_name=organizationFocus)]
    fn focus();
    #[wasm_bindgen(js_name=organizationRestore)]
    fn restore();
}
#[derive(Clone, PartialEq, Eq)]
pub struct Group {
    pub key: String,
    pub title: String,
    pub kind: &'static str,
    pub members: Vec<Repository>,
}
fn identity(r: &Repository, o: &RepositoryOrganization) -> (String, String, &'static str) {
    match o.assignments.get(&r.id) {
        Some(RepositoryAssignment::Manual { id }) if o.groups.contains_key(id) => {
            (format!("manual:{id}"), o.groups[id].clone(), "manual")
        }
        Some(RepositoryAssignment::Separate) => {
            (format!("separate:{}", r.id), r.name.clone(), "separate")
        }
        _ => {
            if let Some(key) = &r.match_key {
                (format!("remote:{key}"), key.clone(), "remote")
            } else {
                (format!("common:{}", r.common_id), r.name.clone(), "local")
            }
        }
    }
}
pub fn group_name(r: &Repository, o: &RepositoryOrganization) -> String {
    identity(r, o).1
}
fn order(a: &Repository, b: &Repository, view: &RepositoryListView) -> Ordering {
    let primary = match view.sort {
        RepositorySort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        RepositorySort::Path => a.path.cmp(&b.path),
        RepositorySort::Recent => a.last_opened.cmp(&b.last_opened),
        RepositorySort::Changes => a.changed.cmp(&b.changed),
    };
    (if view.favorites_first {
        b.favorite.cmp(&a.favorite)
    } else {
        Ordering::Equal
    })
    .then(if view.descending {
        primary.reverse()
    } else {
        primary
    })
    .then(a.path.cmp(&b.path))
    .then(a.id.cmp(&b.id))
}
pub fn groups(
    mut repositories: Vec<Repository>,
    o: &RepositoryOrganization,
    recent: bool,
) -> Vec<Group> {
    let mut view = o.view.clone();
    if recent {
        view.sort = RepositorySort::Recent;
        view.descending = true;
        view.favorites_first = false;
    }
    repositories.sort_by(|a, b| order(a, b, &view));
    if !view.grouped {
        return vec![Group {
            key: "flat".into(),
            title: String::new(),
            kind: "flat",
            members: repositories,
        }];
    }
    let mut groups = BTreeMap::<String, Group>::new();
    for r in repositories {
        let (key, title, kind) = identity(&r, o);
        groups
            .entry(key.clone())
            .or_insert(Group {
                key,
                title,
                kind,
                members: vec![],
            })
            .members
            .push(r);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    for group in &mut groups {
        if group.kind == "local" {
            let primary = group
                .members
                .iter()
                .filter(|r| r.kind != "worktree")
                .min_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)))
                .or_else(|| {
                    group
                        .members
                        .iter()
                        .min_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)))
                });
            if let Some(primary) = primary {
                group.title = primary.name.clone();
            }
        }
    }
    groups.sort_by(|a, b| {
        let favorite = if view.favorites_first {
            b.members
                .iter()
                .any(|r| r.favorite)
                .cmp(&a.members.iter().any(|r| r.favorite))
        } else {
            Ordering::Equal
        };
        let primary = match view.sort {
            RepositorySort::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            RepositorySort::Path => a
                .members
                .iter()
                .map(|r| &r.path)
                .min()
                .cmp(&b.members.iter().map(|r| &r.path).min()),
            RepositorySort::Recent => a
                .members
                .iter()
                .map(|r| &r.last_opened)
                .max()
                .cmp(&b.members.iter().map(|r| &r.last_opened).max()),
            RepositorySort::Changes => a
                .members
                .iter()
                .map(|r| r.changed as u64)
                .sum::<u64>()
                .cmp(&b.members.iter().map(|r| r.changed as u64).sum::<u64>()),
        };
        favorite
            .then(if view.descending {
                primary.reverse()
            } else {
                primary
            })
            .then(a.key.cmp(&b.key))
    });
    groups
}
#[derive(Clone, Copy)]
pub struct Organization {
    c: Controller,
    opened: RwSignal<bool>,
    selected: RwSignal<BTreeSet<String>>,
    query: RwSignal<String>,
    offset: RwSignal<usize>,
    target: RwSignal<String>,
    edit: RwSignal<String>,
    name: RwSignal<String>,
    view: RwSignal<RepositoryListView>,
}
pub fn controller(c: Controller) -> Organization {
    Organization {
        c,
        opened: RwSignal::new(false),
        selected: RwSignal::new(BTreeSet::new()),
        query: RwSignal::new(String::new()),
        offset: RwSignal::new(0),
        target: RwSignal::new("automatic".into()),
        edit: RwSignal::new(String::new()),
        name: RwSignal::new(String::new()),
        view: RwSignal::new(RepositoryListView::default()),
    }
}
fn describe(en: bool, error: String) -> String {
    let zh = match error.as_str() {
        "Group name must contain 1–80 printable characters" => "分组名称需要 1–80 个可显示字符。",
        "Repository group name is already used" => "这个分组名称已存在。",
        "Unknown repository group" => "分组已变化，请重新选择。",
        "Select existing repositories before assigning a group" => {
            "请选择仍然收录的仓库，再设置分组。"
        }
        "Repository organization limit reached" => {
            "分组设置达到数量上限，请删除无用分组或恢复自动归组。"
        }
        "Operation cancelled" => "操作已取消。",
        _ => return error,
    };
    if en { error } else { zh.into() }
}
impl Organization {
    pub fn open(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.view.set(
            self.c
                .catalog
                .with_untracked(|c| c.organization.view.clone()),
        );
        self.opened.set(true);
        self.selected.set(BTreeSet::new());
        self.offset.set(0);
        self.c.error.set(String::new());
        focus();
    }
    pub fn hide(self) {
        if !self.c.busy.get_untracked() {
            self.opened.set(false);
        }
    }
    fn close(self) {
        if !self.c.busy.get_untracked() && self.opened.get_untracked() {
            self.hide();
            restore();
        }
    }
    fn run(self, action: OrganizeRepositories, en: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<Catalog>("workspace_organize", serde_json::json!({"action":action}))
                .await
            {
                Ok(catalog) => {
                    match action {
                        OrganizeRepositories::SetView { .. } => {
                            self.view.set(catalog.organization.view.clone())
                        }
                        OrganizeRepositories::CreateGroup { name } => {
                            if let Some((id, value)) = catalog
                                .organization
                                .groups
                                .iter()
                                .find(|(_, value)| **value == name.trim())
                            {
                                self.edit.set(id.clone());
                                self.name.set(value.clone());
                                self.target.set(format!("manual:{id}"));
                            }
                        }
                        OrganizeRepositories::DeleteGroup { .. } => {
                            self.edit.set(String::new());
                            self.name.set(String::new());
                        }
                        OrganizeRepositories::RenameGroup { id, .. } => {
                            self.name.set(
                                catalog
                                    .organization
                                    .groups
                                    .get(&id)
                                    .cloned()
                                    .unwrap_or_default(),
                            );
                        }
                        _ => {}
                    }
                    if self.target.with_untracked(|v| {
                        v.strip_prefix("manual:")
                            .is_some_and(|id| !catalog.organization.groups.contains_key(id))
                    }) {
                        self.target.set("automatic".into());
                    }
                    self.selected.update(|ids| {
                        ids.retain(|id| catalog.repositories.iter().any(|r| r.id == *id))
                    });
                    self.c.catalog.set(catalog);
                }
                Err(error) => self.c.error.set(describe(en, error)),
            };
            self.c.busy.set(false);
        });
    }
    fn assign(self, en: bool) {
        let value = self.target.get_untracked();
        let assignment = if value == "separate" {
            RepositoryAssignment::Separate
        } else if let Some(id) = value.strip_prefix("manual:") {
            RepositoryAssignment::Manual { id: id.into() }
        } else {
            RepositoryAssignment::Automatic
        };
        self.run(
            OrganizeRepositories::Assign {
                repositories: self.selected.get_untracked().into_iter().collect(),
                assignment,
            },
            en,
        );
    }
}
pub fn panel(o: Organization, en: RwSignal<bool>) -> impl IntoView {
    let matches = Memo::new(move |_| {
        let q = o.query.get().to_lowercase();
        o.c.catalog.with(|c| {
            c.repositories
                .iter()
                .filter(|r| {
                    q.is_empty()
                        || format!("{} {} {}", r.name, r.path, group_name(r, &c.organization))
                            .to_lowercase()
                            .contains(&q)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let page = Memo::new(move |_| {
        matches.with(|r| {
            r.iter()
                .skip(o.offset.get())
                .take(100)
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    view! {<Show when=move ||o.opened.get()><section class="organization-settings repository-registration" aria-label=move ||tr(en.get(),"Git 仓库分组与排序","Git repository groups and ordering") on:keydown=move |ev|{if ev.key()=="Escape"{ev.prevent_default();o.close();}}><header><strong>{move ||tr(en.get(),"Git 仓库分组与排序","Git repository groups and ordering")}</strong><button class="quiet" disabled=move ||o.c.busy.get() on:click=move |_|o.close()>{move ||tr(en.get(),"关闭","Close")}</button></header>
    <div class="organization-body"><div class="organization-preferences"><form on:submit=move |ev|{ev.prevent_default();o.run(OrganizeRepositories::SetView{view:o.view.get_untracked()},en.get_untracked());}><label>{move ||tr(en.get(),"排序依据","Sort by")}<select disabled=move ||o.c.busy.get() aria-label=move ||tr(en.get(),"仓库排序依据","Repository sort key") prop:value=move ||match o.view.get().sort{RepositorySort::Name=>"name",RepositorySort::Path=>"path",RepositorySort::Recent=>"recent",RepositorySort::Changes=>"changes"} on:change=move |ev|{let sort=match event_target_value(&ev).as_str(){"path"=>RepositorySort::Path,"recent"=>RepositorySort::Recent,"changes"=>RepositorySort::Changes,_=>RepositorySort::Name};o.view.update(|v|v.sort=sort);}>
    <option value="name">{move ||tr(en.get(),"名称","Name")}</option><option value="path">{move ||tr(en.get(),"路径","Path")}</option><option value="recent">{move ||tr(en.get(),"最近打开时间","Last opened")}</option><option value="changes">{move ||tr(en.get(),"变更数量","Change count")}</option></select></label>
    <label><input type="checkbox" disabled=move ||o.c.busy.get() prop:checked=move ||o.view.get().descending on:change=move |ev|o.view.update(|v|v.descending=event_target_checked(&ev))/>{move ||tr(en.get(),"降序","Descending")}</label><label><input type="checkbox" disabled=move ||o.c.busy.get() prop:checked=move ||o.view.get().favorites_first on:change=move |ev|o.view.update(|v|v.favorites_first=event_target_checked(&ev))/>{move ||tr(en.get(),"收藏优先","Favorites first")}</label><label><input type="checkbox" disabled=move ||o.c.busy.get() prop:checked=move ||o.view.get().grouped on:change=move |ev|o.view.update(|v|v.grouped=event_target_checked(&ev))/>{move ||tr(en.get(),"按项目分组显示","Group repositories by project")}</label><button class="quiet" type="submit" disabled=move ||o.c.busy.get()>{move ||tr(en.get(),"保存显示设置","Save list preferences")}</button><span role="status">{move ||(o.view.get()!=o.c.catalog.with(|c|c.organization.view.clone())).then_some(tr(en.get(),"显示设置尚未保存","List preferences not saved"))}</span></form>
    <form on:submit=move |ev|{ev.prevent_default();let name=o.name.get_untracked();let id=o.edit.get_untracked();o.run(if id.is_empty(){OrganizeRepositories::CreateGroup{name}}else{OrganizeRepositories::RenameGroup{id,name}},en.get_untracked());}><select disabled=move ||o.c.busy.get() aria-label=move ||tr(en.get(),"编辑自定义分组","Edit custom group") prop:value=move ||o.edit.get() on:change=move |ev|{let id=event_target_value(&ev);o.name.set(o.c.catalog.with_untracked(|c|c.organization.groups.get(&id).cloned().unwrap_or_default()));o.edit.set(id);}><option value="">{move ||tr(en.get(),"新建自定义分组","New custom group")}</option>{move ||o.c.catalog.with(|c|c.organization.groups.iter().map(|(id,name)|view!{<option value=id.clone()>{name.clone()}</option>}).collect_view())}</select><input placeholder=move ||tr(en.get(),"分组名称","Group name") aria-label=move ||tr(en.get(),"自定义分组名称","Custom group name") maxlength="80" disabled=move ||o.c.busy.get() prop:value=move ||o.name.get() on:input=move |ev|o.name.set(event_target_value(&ev))/><button class="quiet" type="submit" disabled=move ||o.c.busy.get()||o.name.get().trim().is_empty()>{move ||if o.edit.get().is_empty(){tr(en.get(),"创建分组","Create group")}else{tr(en.get(),"重命名分组","Rename group")}}</button><button class="quiet" type="button" disabled=move ||o.c.busy.get()||o.edit.get().is_empty() on:click=move |_|o.run(OrganizeRepositories::DeleteGroup{id:o.edit.get_untracked()},en.get_untracked())>{move ||tr(en.get(),"删除分组并恢复自动归组","Delete group and restore automatic grouping")}</button><button class="quiet" type="button" disabled=move ||o.c.busy.get() on:click=move |_|o.run(OrganizeRepositories::RestoreAutomatic,en.get_untracked())>{move ||tr(en.get(),"全部恢复自动归组","Restore automatic grouping for all")}</button></form></div>
    <div class="organization-members"><div><input type="search" placeholder=move ||tr(en.get(),"筛选名称、路径或分组…","Filter name, path or group…") aria-label=move ||tr(en.get(),"筛选分组仓库","Filter group repositories") disabled=move ||o.c.busy.get() prop:value=move ||o.query.get() on:input=move |ev|{o.query.set(event_target_value(&ev));o.offset.set(0);}/><button class="quiet" disabled=move ||o.c.busy.get() on:click=move |_|o.selected.update(|ids|ids.extend(page.with_untracked(|p|p.iter().map(|r|r.id.clone()).collect::<Vec<_>>())) )>{move ||tr(en.get(),"选择本页","Select this page")}</button><button class="quiet" disabled=move ||o.c.busy.get() on:click=move |_|o.selected.set(matches.with_untracked(|rows|rows.iter().map(|r|r.id.clone()).collect()))>{move ||tr(en.get(),"选择全部匹配","Select all matches")}</button><button class="quiet" disabled=move ||o.c.busy.get() on:click=move |_|o.selected.set(BTreeSet::new())>{move ||tr(en.get(),"清空选择","Clear selection")}</button></div><div class="organization-repository-list"> <For each=move ||page.get() key=|r|r.id.clone() children=move |r|{let id=r.id.clone();let toggle=id.clone();let path=r.path.clone();let member=r.clone();view!{<label title=path.clone()><input type="checkbox" aria-label=path.clone() disabled=move ||o.c.busy.get() prop:checked=move ||o.selected.with(|ids|ids.contains(&id)) on:change=move |ev|o.selected.update(|ids|{if event_target_checked(&ev){ids.insert(toggle.clone());}else{ids.remove(&toggle);}})/><span>{r.name}</span><code>{path.clone()}</code><small class="organization-current-group">{move ||o.c.catalog.with(|c|match c.organization.assignments.get(&member.id){Some(RepositoryAssignment::Separate)=>tr(en.get(),"单独显示","Separate").into(),Some(RepositoryAssignment::Manual{id})=>c.organization.groups.get(id).cloned().unwrap_or_default(),_=>format!("{} · {}",tr(en.get(),"自动","Auto"),member.match_key.as_deref().unwrap_or(tr(en.get(),"本地关联","Local identity")))})}</small></label>}}/></div><footer><span>{move ||format!("{}–{} / {} · {} {}",o.offset.get()+usize::from(matches.with(|r|!r.is_empty())),(o.offset.get()+100).min(matches.with(Vec::len)),matches.with(Vec::len),o.selected.with(BTreeSet::len),tr(en.get(),"个已选","selected"))}</span><button class="quiet" disabled=move ||o.c.busy.get()||o.offset.get()==0 on:click=move |_|o.offset.update(|v|*v=v.saturating_sub(100))>"‹"</button><button class="quiet" disabled=move ||o.c.busy.get()||(o.offset.get()+100>=matches.with(Vec::len)) on:click=move |_|o.offset.update(|v|*v+=100)>"›"</button></footer>
    <div><select disabled=move ||o.c.busy.get() aria-label=move ||tr(en.get(),"所选仓库的分组","Group for selected repositories") prop:value=move ||o.target.get() on:change=move |ev|o.target.set(event_target_value(&ev))><option value="automatic">{move ||tr(en.get(),"自动：优先远程地址","Automatic: remote first")}</option><option value="separate">{move ||tr(en.get(),"各自单独显示","Keep each repository separate")}</option>{move ||o.c.catalog.with(|c|c.organization.groups.iter().map(|(id,name)|view!{<option value=format!("manual:{id}")>{name.clone()}</option>}).collect_view())}</select><button class="quiet" disabled=move ||o.c.busy.get()||o.selected.with(BTreeSet::is_empty) on:click=move |_|o.assign(en.get_untracked())>{move ||tr(en.get(),"应用到所选仓库","Apply to selected repositories")}</button></div></div></div><p>{move ||tr(en.get(),"分组只影响首页显示，不更改远程地址、不合并仓库或文件规则。删除自定义分组会恢复自动归组。","Grouping only changes home display, never remotes, repositories or file rules. Deleting a custom group restores automatic grouping.")}</p></section></Show>}
}
