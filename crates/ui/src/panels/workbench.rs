//! Home for comparison projects and captured Git repositories.
use super::{git_actions, git_changes, git_history, source};
use crate::{invoke, tr};
use clear_contracts::{LibraryEntry, workbench::*};
use leptos::{prelude::*, task::spawn_local};
fn kind(kind: &str, en: bool) -> &str {
    match kind {
        "worktree" => "worktree",
        "submodule" => tr(en, "子模块", "Submodule"),
        "bare" => tr(en, "裸仓库", "Bare repository"),
        _ => tr(en, "仓库", "Repository"),
    }
}
pub(super) fn change_label(code: &str, en: bool) -> &str {
    match code {
        "M" => tr(en, "已修改", "Modified"),
        "A" => tr(en, "新增", "Added"),
        "D" => tr(en, "已删除", "Deleted"),
        "R" => tr(en, "重命名", "Renamed"),
        "C" => tr(en, "复制", "Copied"),
        "U" => tr(en, "冲突", "Conflict"),
        "?" => tr(en, "未跟踪", "Untracked"),
        "T" => tr(en, "类型变化", "Type changed"),
        _ => "—",
    }
}
#[derive(Clone, Copy)]
pub struct Navigation {
    pub files: RwSignal<bool>,
    pub home: RwSignal<bool>,
    pub library: RwSignal<Option<Vec<LibraryEntry>>>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    navigation: Navigation,
    compare: impl Fn() + Copy + Send + Sync + 'static,
    manage: impl Fn() + Copy + Send + Sync + 'static,
    open_project: impl Fn(LibraryEntry) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let files_visible = navigation.files;
    let catalog = RwSignal::new(Catalog::default());
    let projects = RwSignal::new(Vec::<LibraryEntry>::new());
    let selected = RwSignal::new(None::<RepositoryState>);
    let actions = git_actions::Controller {
        selected,
        catalog,
        busy,
        error,
        plan: RwSignal::new(None),
        log: RwSignal::new(String::new()),
    };
    let history_visible = RwSignal::new(false);
    let editor_request = RwSignal::new(None);
    let query = RwSignal::new(String::new());
    let mode = RwSignal::new("all".to_string());
    let root = RwSignal::new(String::new());
    let roots = RwSignal::new(Vec::<String>::new());
    let exclusions = RwSignal::new(String::new());
    let settings_open = RwSignal::new(false);
    let load = move || {
        spawn_local(async move {
            match invoke::<Catalog>("workspace_catalog", serde_json::json!({})).await {
                Ok(c) => {
                    if !settings_open.get_untracked() {
                        roots.set(c.settings.roots.clone());
                        exclusions.set(c.settings.exclusions.join("\n"));
                    }
                    projects.set(c.projects.clone());
                    catalog.set(c);
                }
                Err(e) => error.set(e),
            }
        })
    };
    Effect::new(move |_| {
        let _ = navigation.library.get();
        if navigation.home.get() {
            load();
        }
    });
    let scan = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        selected.set(None);
        let settings = ScanSettings {
            roots: roots.get_untracked(),
            exclusions: exclusions
                .get_untracked()
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        };
        spawn_local(async move {
            match invoke::<Catalog>("workspace_scan", serde_json::json!({"settings":settings}))
                .await
            {
                Ok(c) => {
                    catalog.set(c);
                    settings_open.set(false);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let browse = move || {
        spawn_local(async move {
            match invoke::<Option<String>>("pick_directory", serde_json::json!({})).await {
                Ok(Some(path)) => {
                    roots.update(|r| {
                        if !r.contains(&path) {
                            r.push(path);
                        }
                    });
                    settings_open.set(true);
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
        })
    };
    let open = move |id: String| {
        if busy.get_untracked() {
            return;
        }
        actions.plan.set(None);
        actions.log.set(String::new());
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<RepositoryState>(
                "workspace_open",
                serde_json::json!({"repositoryId":id}),
            )
            .await
            {
                Ok(r) => {
                    catalog.update(|c| {
                        if let Some(old) =
                            c.repositories.iter_mut().find(|p| p.id == r.repository.id)
                        {
                            *old = r.repository.clone();
                        }
                    });
                    selected.set(Some(r));
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let favorite = move |id: String, value: bool| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match invoke::<Catalog>(
                "workspace_favorite",
                serde_json::json!({"repositoryId":id,"favorite":value}),
            )
            .await
            {
                Ok(c) => catalog.set(c),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let repositories = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        let mode = mode.get();
        let mut r = catalog.with(|c| {
            c.repositories
                .iter()
                .filter(|r| {
                    (mode != "favorites" || r.favorite)
                        && (mode != "recent" || !r.last_opened.is_empty())
                        && (q.is_empty()
                            || format!(
                                "{} {} {} {}",
                                r.name,
                                r.path,
                                r.branch,
                                r.match_key.as_deref().unwrap_or_default()
                            )
                            .to_lowercase()
                            .contains(&q))
                })
                .cloned()
                .collect::<Vec<_>>()
        });
        if mode == "recent" {
            r.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
        } else {
            r.sort_by(|a, b| {
                b.favorite
                    .cmp(&a.favorite)
                    .then(a.match_key.cmp(&b.match_key))
                    .then(a.name.cmp(&b.name))
            });
        }
        r
    });
    view! {<section class="workbench" class:source-active=move ||files_visible.get() on:keydown=move |ev|{if (ev.meta_key()||ev.ctrl_key())&&!ev.alt_key()&&((ev.shift_key()&&ev.key().eq_ignore_ascii_case("f"))||ev.key().eq_ignore_ascii_case("p")){ev.prevent_default();if !files_visible.get_untracked(){files_visible.set(true);}
    if ev.key().eq_ignore_ascii_case("p"){source::quick_open();}else{source::focus_search();}}} aria-label=move ||tr(english.get(),"项目工作台","Project workspace")>
            <header class="workbench-heading"><div><strong>{move ||tr(english.get(),"工作台","Workspace")}</strong><span>{move ||format!("{} {} · {} {}",projects.with(|p|p.len()),tr(english.get(),"个比较项目","comparison projects"),catalog.with(|c|c.repositories.len()),tr(english.get(),"个 Git 仓库","Git repositories"))}</span></div><button class="quiet" disabled=move ||busy.get() on:click=move |_|manage()>{move ||tr(english.get(),"管理比较项目","Manage comparison projects")}</button><button class="primary" disabled=move ||busy.get() on:click=move |_|compare()>{move ||tr(english.get(),"文件夹比较","Folder comparison")}</button></header>
            <div class="workbench-toolbar"><button class="quiet" on:click=move |_|{files_visible.set(true);source::focus_search();}>{move ||tr(english.get(),"文件 / 全局搜索","Files / global search")}</button><nav aria-label=move ||tr(english.get(),"项目分类","Project categories")>{[ ("all","全部","All"),("recent","最近","Recent"),("favorites","收藏","Favorites"),("projects","比较项目","Comparisons"),("repositories","Git 仓库","Git repositories")].into_iter().map(move |(value,zh,en)|view!{<button class="quiet" aria-pressed=move ||mode.get()==value on:click=move |_|{mode.set(value.into());selected.set(None);}>{move ||tr(english.get(),zh,en)}</button>}).collect_view()}</nav><input aria-label=move ||tr(english.get(),"搜索项目与仓库","Search projects and repositories") placeholder=move ||tr(english.get(),"搜索名称、路径、远程地址、分支…","Search name, path, remote or branch…") prop:value=move ||query.get() on:input=move |ev|query.set(event_target_value(&ev))/><button class="quiet" disabled=move ||busy.get() on:click=move |_|browse()>{move ||tr(english.get(),"添加扫描目录","Add scan directory")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|settings_open.update(|v|*v = !*v)>{move ||tr(english.get(),"扫描设置","Scan settings")}</button><button class="quiet" disabled=move ||busy.get()||roots.with(|r|r.is_empty()) on:click=move |_|scan()>{move ||tr(english.get(),"重新扫描","Rescan")}</button></div>
            <Show when=move ||settings_open.get()><section class="workbench-scan" aria-label=move ||tr(english.get(),"扫描设置","Scan settings")><div class="workbench-root-input"><input aria-label=move ||tr(english.get(),"扫描目录路径","Scan directory path") placeholder=move ||tr(english.get(),"输入一个目录，再点击添加","Enter a directory, then Add") prop:value=move ||root.get() on:input=move |ev|root.set(event_target_value(&ev))/><button class="quiet" disabled=move ||busy.get()||root.get().trim().is_empty() on:click=move |_|{let value=root.get_untracked().trim().to_string();roots.update(|r|{if !r.contains(&value){r.push(value);}});root.set(String::new());}>{move ||tr(english.get(),"添加","Add")}</button></div><For each=move ||roots.get() key=|r|r.clone() children=move |path|{let remove=path.clone();view!{<div class="workbench-root"><code>{path}</code><button class="quiet" disabled=move ||busy.get() on:click=move |_|roots.update(|r|r.retain(|p|p!=&remove))>{move ||tr(english.get(),"移除目录","Remove directory")}</button></div>}}/><label>{move ||tr(english.get(),"跳过的目录名称（每行一个；.git 内部始终跳过）","Directory names to skip (one per line; .git internals are always skipped)")}<textarea prop:value=move ||exclusions.get() on:input=move |ev|exclusions.set(event_target_value(&ev))/></label><button class="primary" disabled=move ||busy.get()||roots.with(|r|r.is_empty()) on:click=move |_|scan()>{move ||tr(english.get(),"保存并扫描","Save and scan")}</button></section></Show>
            <Show when=move ||catalog.with(|c|!c.warnings.is_empty()||c.truncated)><details class="workbench-warnings"><summary>{move ||tr(english.get(),"扫描警告","Scan warnings")}</summary><pre>{move ||catalog.with(|c|format!("{}{}",if c.truncated{tr(english.get(),"扫描达到数量或深度上限，请缩小范围。\n","Scan limit reached; narrow the scope.\n")}else{""},c.warnings.join("\n")))}</pre></details></Show>
            <div class="workbench-body" class:files-view=move ||files_visible.get()><aside class="workbench-list">
                <Show when=move ||mode.get()=="all"||mode.get()=="projects"><h2>{move ||tr(english.get(),"比较项目","Comparison projects")}</h2><For each=move ||projects.with(|p|p.iter().filter(|p|query.get().is_empty()||p.name.to_lowercase().contains(&query.get().to_lowercase())).cloned().collect::<Vec<_>>()) key=|p|p.id.clone() children=move |p|{let entry=p.clone();view!{<button class="workbench-project" disabled=move ||busy.get() on:click=move |_|open_project(entry.clone())><strong>{p.name}</strong><span>{p.session.request.left}</span><span>{p.session.request.right}</span></button>}}/><Show when=move ||projects.with(|p|p.is_empty())><p class="workbench-empty">{move ||tr(english.get(),"尚无已保存的比较项目。","No saved comparison projects.")}</p></Show></Show>
                <Show when=move ||mode.get()!="projects"><h2>{move ||tr(english.get(),"Git 仓库","Git repositories")}</h2><For each=move ||repositories.get() key=|r|format!("{}:{}:{}:{}:{}:{}",r.id,r.favorite,r.branch,r.changed,r.available,r.last_opened) children=move |r|{
                    let open_id=r.id.clone();let favorite_id=r.id.clone();let chosen_id=r.id.clone();let is_favorite=r.favorite;let name=r.name.clone();let key=r.match_key.clone();let matched=key.is_some();
                    view!{<article class="workbench-repository" class:selected=move ||selected.with(|s|s.as_ref().is_some_and(|s|s.repository.id==chosen_id))><button class="workbench-repository-open" disabled=move ||busy.get() on:click=move |_|open(open_id.clone())><strong>{name}<small>{move ||kind(&r.kind,english.get()).to_string()}</small></strong><span title=r.path.clone()>{r.path.clone()}</span><span>{format!("{} · {} {} · {} {} · ↑{} ↓{}",r.branch,r.changed,tr(english.get_untracked(),"处变更","changes"),r.conflicts,tr(english.get_untracked(),"冲突","conflicts"),r.ahead,r.behind)}</span><Show when=move ||matched><span class="workbench-match">{key.clone()}</span></Show><Show when=move ||!r.available><span class="workbench-unavailable">{move ||tr(english.get(),"待验证或路径失效","Needs verification or unavailable")}</span></Show></button><button class="quiet workbench-star" aria-label=move ||if is_favorite{tr(english.get(),"取消收藏仓库","Unfavorite repository")}else{tr(english.get(),"收藏仓库","Favorite repository")} disabled=move ||busy.get() on:click=move |_|favorite(favorite_id.clone(),!is_favorite)>{if is_favorite{"★"}else{"☆"}}</button></article>}
                }/><Show when=move ||repositories.with(|r|r.is_empty())><p class="workbench-empty">{move ||tr(english.get(),"添加扫描目录，发现你的 Git 仓库。扫描不会拉取或修改代码。","Add a scan directory to discover Git repositories. Scanning does not fetch or modify code.")}</p></Show></Show>
            </aside><main class="workbench-detail">{source::panel(actions,files_visible,english,editor_request,move ||files_visible.set(false))}<div class="git-panels" style:display=move ||if files_visible.get(){"none"}else{"flex"}><Show when=move ||selected.with(|s|s.is_some()) fallback=move ||view!{<section class="workbench-welcome"><strong>{move ||tr(english.get(),"选择项目或仓库开始","Choose a project or repository")}</strong><p>{move ||tr(english.get(),"比较项目保留独立规则；同一远程地址的仓库自动归组，本地副本与 worktree 分别保留。","Comparison projects keep independent rules. Repositories sharing a remote are grouped while local copies and worktrees remain separate.")}</p></section>}>
                <nav class="git-workspace-tabs"><button class="quiet" aria-pressed=move ||!history_visible.get() on:click=move |_|history_visible.set(false)>{move ||tr(english.get(),"工作区","Working tree")}</button><button class="quiet" aria-pressed=move ||history_visible.get() on:click=move |_|history_visible.set(true)>{move ||tr(english.get(),"提交历史","History")}</button></nav>
                {git_actions::panel(actions, english)}
                {git_history::panel(actions,history_visible,english)}
                {git_changes::panel(actions,history_visible,files_visible,editor_request,english)}
            </Show></div></main></div>
        </section>}
}
