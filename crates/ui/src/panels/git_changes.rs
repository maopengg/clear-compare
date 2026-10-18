//! Grouped, paged changed files and captured hunk/line staging.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy)]
struct Changes {
    c: Controller,
    english: RwSignal<bool>,
    query: RwSignal<String>,
    category: RwSignal<String>,
    tree: RwSignal<bool>,
    offset: RwSignal<usize>,
    selected: RwSignal<BTreeSet<String>>,
    file: RwSignal<String>,
    area: RwSignal<String>,
    diff: RwSignal<Option<WorkingDiff>>,
    lines: RwSignal<BTreeSet<String>>,
    attempted: RwSignal<String>,
}
impl Changes {
    fn key(self, file: &str, area: &str, revision: &str) -> String {
        format!("{file}/{area}/{revision}")
    }
    fn load(self, file: String, area: String, offset: usize, fresh: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        self.file.set(file.clone());
        self.area.set(area.clone());
        self.attempted.set(self.key(&file, &area, &state.revision));
        let token = if fresh {
            None
        } else {
            self.diff
                .with_untracked(|d| d.as_ref().map(|d| d.id.clone()))
        };
        spawn_local(async move {
            match invoke::<WorkingDiff>("workspace_working_diff",serde_json::json!({"repositoryId":state.repository.id,"fileId":file,"area":area,"diffId":token,"offset":offset})).await{
            Ok(diff)=>{if fresh{self.lines.set(BTreeSet::new());}self.diff.set(Some(diff));},Err(e)=>self.c.error.set(e)
        }
            self.c.busy.set(false);
        });
    }
    fn editable(self) -> bool {
        self.diff.with(|d| {
            d.as_ref().is_some_and(|d| {
                d.partial
                    && d.file_id == self.file.get()
                    && d.area == self.area.get()
                    && self.c.selected.with(|s| {
                        s.as_ref().is_some_and(|s| {
                            s.repository.id == d.repository_id && s.revision == d.revision
                        })
                    })
            })
        })
    }
    fn stage(self, hunks: Vec<String>, lines: Vec<String>) {
        if !self.editable() {
            return;
        }
        if let Some(diff) = self.diff.get_untracked() {
            self.c.run(Action::StageSelection {
                diff_id: diff.id,
                hunks,
                lines,
            });
        }
    }
    fn selection(self) -> Vec<String> {
        self.selected.get_untracked().into_iter().collect()
    }
    fn staged_selection(self) -> Vec<String> {
        self.c.selected.with(|state| {
            state
                .as_ref()
                .map(|state| {
                    self.selected.with(|ids| {
                        state
                            .changes
                            .iter()
                            .filter(|change| ids.contains(&change.id) && member(change, "index"))
                            .map(|change| change.id.clone())
                            .collect()
                    })
                })
                .unwrap_or_default()
        })
    }
    fn editor(self, visible: RwSignal<bool>, request: RwSignal<Option<(String, String)>>) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        let file = self.file.get_untracked();
        if file.is_empty() {
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<WorkspaceFile>(
                "workspace_change_file",
                serde_json::json!({"repositoryId":state.repository.id,"fileId":file}),
            )
            .await
            {
                Ok(file) => {
                    request.set(Some((state.repository.id, file.id)));
                    visible.set(true);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
}
fn member(c: &Change, group: &str) -> bool {
    match group {
        "conflicts" => c.conflicted,
        "untracked" => c.untracked && !c.conflicted,
        "index" => !c.conflicted && !c.untracked && c.index != ".",
        _ => !c.conflicted && !c.untracked && c.worktree != ".",
    }
}
fn group_title(group: &str, en: bool) -> &'static str {
    match group {
        "index" => tr(en, "已暂存", "Staged"),
        "untracked" => tr(en, "未跟踪", "Untracked"),
        "conflicts" => tr(en, "冲突", "Conflicts"),
        _ => tr(en, "未暂存", "Unstaged"),
    }
}
fn reason(code: &str, en: bool) -> &'static str {
    match code {
        "binary" => tr(
            en,
            "二进制差异，请使用整文件暂存或取消暂存。",
            "Binary difference; stage or unstage the whole file.",
        ),
        "rename" => tr(
            en,
            "重命名差异当前使用整文件操作。",
            "Use whole-file actions for renamed paths.",
        ),
        "conflict" => tr(
            en,
            "文件有冲突，请编辑解决后暂存；此处仅显示原始差异。",
            "Resolve conflicts before staging; raw difference is shown here.",
        ),
        "unsupported_mode" => tr(
            en,
            "链接、子模块或特殊类型使用整文件操作。",
            "Use whole-file actions for links, submodules and special types.",
        ),
        "head" => tr(
            en,
            "HEAD 对工作区为只读；逐块操作请选择未暂存或已暂存。",
            "HEAD to working tree is read-only. Select unstaged or staged for partial actions.",
        ),
        _ => tr(
            en,
            "该区域没有文本差异，可能只有权限等元数据变化。",
            "No text difference in this area; metadata may differ.",
        ),
    }
}
fn row(s: Changes, c: Change, group: &'static str, leaf: bool) -> AnyView {
    let id = c.id.clone();
    let choose = id.clone();
    let checked = id.clone();
    let highlight = id.clone();
    let stage = id.clone();
    let unstage = id.clone();
    let discard = id.clone();
    let area = if group == "index" {
        "index"
    } else {
        "worktree"
    };
    let label = if leaf {
        c.path.rsplit('/').next().unwrap_or(&c.path).to_string()
    } else {
        c.path.clone()
    };
    let state = super::workbench::change_label(
        if group == "index" {
            &c.index
        } else {
            &c.worktree
        },
        s.english.get_untracked(),
    )
    .to_string();
    view!{<tr class="git-change-row" aria-selected=move ||s.file.get()==highlight&&s.area.get()==area><td><input type="checkbox" aria-label=format!("{} {}",tr(s.english.get_untracked(),"选择","Select"),c.path) prop:checked=move ||s.selected.with(|v|v.contains(&checked)) on:change=move |ev|{let value=event_target_checked(&ev);s.selected.update(|v|{if value&&v.len()<10000{v.insert(choose.clone());}else{v.remove(&choose);}});}/></td><td><button class="git-change-open" title=c.path.clone() on:click=move |_|s.load(id.clone(),area.into(),0,true) disabled=move ||s.c.busy.get()>{label}</button><small>{state}</small></td><td>{if group!="index"{Some(view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|s.c.run(Action::Stage{files:vec![stage.clone()]})>{move ||tr(s.english.get(),"暂存","Stage")}</button>})}else{None}}{if group=="index"{Some(view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|s.c.run(Action::Unstage{files:vec![unstage.clone()]})>{move ||tr(s.english.get(),"取消暂存","Unstage")}</button>})}else{None}}{if group=="worktree"{Some(view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|s.c.run(Action::Discard{files:vec![discard.clone()]})>{move ||tr(s.english.get(),"放弃修改","Discard")}</button>})}else{None}}</td></tr>}.into_any()
}
#[derive(Default)]
struct Tree {
    folders: BTreeMap<String, Tree>,
    files: Vec<Change>,
}
fn tree_view(s: Changes, tree: Tree, group: &'static str) -> AnyView {
    view!{<div>{tree.folders.into_iter().map(move |(name,tree)|view!{<details class="git-change-folder" open><summary>{name}</summary>{tree_view(s,tree,group)}</details>}).collect_view()}<table class="workbench-changes"><tbody>{tree.files.into_iter().map(move |c|row(s,c,group,true)).collect_view()}</tbody></table></div>}.into_any()
}
fn files(s: Changes, changes: Vec<Change>, group: &'static str) -> AnyView {
    if !s.tree.get() {
        return view!{<table class="workbench-changes"><tbody>{changes.into_iter().map(move |c|row(s,c,group,false)).collect_view()}</tbody></table>}.into_any();
    }
    let mut tree = Tree::default();
    for c in changes {
        let parts = c
            .path
            .split('/')
            .take(9)
            .map(String::from)
            .collect::<Vec<_>>();
        let mut node = &mut tree;
        for folder in parts.iter().take(parts.len().saturating_sub(1)) {
            node = node.folders.entry(folder.clone()).or_default();
        }
        node.files.push(c);
    }
    tree_view(s, tree, group)
}
pub fn panel(
    c: Controller,
    history: RwSignal<bool>,
    source_visible: RwSignal<bool>,
    editor_request: RwSignal<Option<(String, String)>>,
    english: RwSignal<bool>,
) -> impl IntoView {
    let s = Changes {
        c,
        english,
        query: RwSignal::new(String::new()),
        category: RwSignal::new("all".into()),
        tree: RwSignal::new(false),
        offset: RwSignal::new(0),
        selected: RwSignal::new(BTreeSet::new()),
        file: RwSignal::new(String::new()),
        area: RwSignal::new("worktree".into()),
        diff: RwSignal::new(None),
        lines: RwSignal::new(BTreeSet::new()),
        attempted: RwSignal::new(String::new()),
    };
    let repo = Memo::new(move |_| {
        c.selected
            .with(|r| r.as_ref().map(|s| s.repository.id.clone()))
    });
    Effect::new(move |_| {
        repo.get();
        s.file.set(String::new());
        s.diff.set(None);
        s.lines.set(BTreeSet::new());
        s.selected.set(BTreeSet::new());
        s.offset.set(0);
    });
    Effect::new(move |_| {
        s.query.get();
        s.category.get();
        s.offset.set(0);
    });
    Effect::new(move |_| {
        if c.busy.get() {
            return;
        }
        let Some(state) = c.selected.get() else {
            return;
        };
        let file = s.file.get();
        if file.is_empty() {
            return;
        }
        let area = s.area.get();
        let key = s.key(&file, &area, &state.revision);
        if key != s.attempted.get_untracked() {
            if state.changes.iter().any(|change| change.id == file) {
                s.load(file, area, 0, true);
            } else {
                s.file.set(String::new());
                s.diff.set(None);
                s.lines.set(BTreeSet::new());
            }
        }
    });
    let filtered = Memo::new(move |_| {
        let needle = s.query.get().to_lowercase();
        let category = s.category.get();
        c.selected.with(|state| {
            state
                .as_ref()
                .map(|s1| {
                    s1.changes
                        .iter()
                        .filter(|change| {
                            change.path.to_lowercase().contains(&needle)
                                && (category == "all" || member(change, &category))
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    Effect::new(move |_| {
        let total = filtered.with(|v| v.len());
        let offset = s.offset.get();
        if offset > 0 && offset >= total {
            s.offset.set(total.saturating_sub(1) / 200 * 200);
        }
    });
    Effect::new(move |_| {
        let valid = c.selected.with(|s| {
            s.as_ref()
                .map(|s| {
                    s.changes
                        .iter()
                        .map(|c| c.id.clone())
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default()
        });
        s.selected.update(|ids| ids.retain(|id| valid.contains(id)));
    });
    view! {<section class="git-changes" style:display=move ||if history.get(){"none"}else{"flex"} aria-label=move ||tr(english.get(),"Git 变更与差异","Git changes and differences")>
    <header class="workbench-repo-heading"><div><strong>{move ||c.selected.with(|s|s.as_ref().map(|s|s.repository.name.clone()).unwrap_or_default())}</strong><span>{move ||c.selected.with(|s|s.as_ref().map(|s|s.repository.path.clone()).unwrap_or_default())}</span></div><span>{move ||c.selected.with(|s|s.as_ref().map(|s|format!("{} · ↑{} ↓{}",s.repository.branch,s.repository.ahead,s.repository.behind)).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if let Some(state)=c.selected.get_untracked(){c.busy.set(true);c.error.set(String::new());spawn_local(async move{match invoke::<RepositoryState>("workspace_open",serde_json::json!({"repositoryId":state.repository.id})).await{Ok(state)=>c.accept(state),Err(e)=>c.error.set(e)}c.busy.set(false);});}}>{move ||tr(english.get(),"刷新仓库","Refresh repository")}</button></header>
    <div class="git-changes-body"><aside class="git-change-files"><div class="git-change-filters"><input aria-label=move ||tr(english.get(),"筛选变更文件","Filter changed files") placeholder=move ||tr(english.get(),"筛选路径…","Filter paths…") prop:value=move ||s.query.get() on:input=move |ev|s.query.set(event_target_value(&ev))/><select aria-label=move ||tr(english.get(),"变更分组","Change group") prop:value=move ||s.category.get() on:change=move |ev|s.category.set(event_target_value(&ev))>{["all","worktree","index","untracked","conflicts"].into_iter().map(move |group|view!{<option value=group>{move ||if group=="all"{tr(english.get(),"全部变更","All changes")}else{group_title(group,english.get())}}</option>}).collect_view()}</select><label><input type="checkbox" prop:checked=move ||s.tree.get() on:change=move |ev|s.tree.set(event_target_checked(&ev))/>{move ||tr(english.get(),"目录树","Tree")}</label></div>
    <div class="git-change-bulk"><span>{move ||format!("{} {}",s.selected.with(|v|v.len()),tr(english.get(),"项已选","selected"))}</span><button class="quiet" on:click=move |_|{let ids=filtered.with_untracked(|v|v.iter().skip(s.offset.get_untracked()).take(200).map(|c|c.id.clone()).collect::<Vec<_>>());s.selected.update(|v|{for id in ids{if v.len()<10000{v.insert(id);}}});}>{move ||tr(english.get(),"选择本页","Select page")}</button><button class="quiet" on:click=move |_|s.selected.set(BTreeSet::new())>{move ||tr(english.get(),"清空选择","Clear selection")}</button><button class="quiet" disabled=move ||c.busy.get()||s.selected.with(|v|v.is_empty()) on:click=move |_|c.run(Action::Stage{files:s.selection()})>{move ||tr(english.get(),"暂存所选","Stage selected")}</button><button class="quiet" disabled=move ||c.busy.get()||s.staged_selection().is_empty() on:click=move |_|c.run(Action::Unstage{files:s.staged_selection()})>{move ||tr(english.get(),"取消暂存所选","Unstage selected")}</button></div>
    <div class="git-change-scroll">{move ||{let values=filtered.get();let page=values.iter().skip(s.offset.get()).take(200).cloned().collect::<Vec<_>>();["worktree","index","untracked","conflicts"].into_iter().filter_map(move |group|{let count=values.iter().filter(|c|member(c,group)).count();let page=page.iter().filter(|c|member(c,group)).cloned().collect::<Vec<_>>();if page.is_empty(){None}else{Some(view!{<details class="git-change-group" data-area=group open><summary>{format!("{} · {count}",group_title(group,english.get()))}</summary>{files(s,page,group)}</details>})}}).collect_view()}}</div>
    <footer class="git-change-pagination"><span>{move ||format!("{}–{} / {}",if filtered.with(|v|v.is_empty()){0}else{s.offset.get()+1},(s.offset.get()+200).min(filtered.with(|v|v.len())),filtered.with(|v|v.len()))}</span><button class="quiet" disabled=move ||s.offset.get()==0 on:click=move |_|s.offset.update(|v|*v=v.saturating_sub(200))>"‹"</button><button class="quiet" disabled=move ||{s.offset.get()+200>=filtered.with(|v|v.len())} on:click=move |_|s.offset.update(|v|*v+=200)>"›"</button></footer>
    <details class="git-repo-extra"><summary>{move ||tr(english.get(),"分支、标签和远程信息","Branches, tags and remotes")}</summary>{move ||c.selected.get().map(|state|view!{<ul class="workbench-refs">{state.references.into_iter().map(|r|view!{<li><strong>{r.name}</strong><span>{r.upstream}</span><span>{r.tracking}</span></li>}).collect_view()}</ul>{state.remotes.into_iter().map(|r|view!{<div class="workbench-remote"><strong>{r.name}</strong><code>{r.fetch}</code><code>{r.push}</code></div>}).collect_view()}})}</details></aside>
    <main class="git-working-diff"><header><strong>{move ||s.diff.with(|d|d.as_ref().map(|d|d.path.clone()).unwrap_or_else(||tr(english.get(),"选择文件查看差异","Select a file to view differences").into()))}</strong><select aria-label=move ||tr(english.get(),"差异区域","Difference area") prop:value=move ||s.area.get() disabled=move ||c.busy.get()||s.file.get().is_empty() on:change=move |ev|s.load(s.file.get_untracked(),event_target_value(&ev),0,true)><option value="worktree">{move ||tr(english.get(),"未暂存：暂存区 → 工作区","Unstaged: index → working tree")}</option><option value="index">{move ||tr(english.get(),"已暂存：HEAD → 暂存区","Staged: HEAD → index")}</option><option value="head">{move ||tr(english.get(),"HEAD → 工作区","HEAD → working tree")}</option></select><button class="quiet" disabled=move ||c.busy.get()||s.file.get().is_empty() on:click=move |_|s.load(s.file.get_untracked(),s.area.get_untracked(),0,true)>{move ||tr(english.get(),"刷新差异","Refresh diff")}</button><button class="quiet" disabled=move ||c.busy.get()||s.file.get().is_empty() on:click=move |_|s.editor(source_visible,editor_request)>{move ||tr(english.get(),"编辑工作区文件","Edit working file")}</button></header>
    <div class="git-selection-tools"><span>{move ||format!("{} {}",s.lines.with(|v|v.len()),tr(english.get(),"行已选","lines selected"))}</span><button class="quiet" disabled=move ||c.busy.get()||!s.editable()||s.lines.with(|v|v.is_empty()) on:click=move |_|s.stage(vec![],s.lines.get_untracked().into_iter().collect())>{move ||if s.area.get()=="index"{tr(english.get(),"取消暂存选中行","Unstage selected lines")}else{tr(english.get(),"暂存选中行","Stage selected lines")}}</button><button class="quiet" on:click=move |_|s.lines.set(BTreeSet::new())>{move ||tr(english.get(),"清空选中行","Clear selected lines")}</button></div>
    <div class="git-diff-scroll">{move ||s.diff.get().map(|diff|{let issue=diff.reason.clone();let partial=diff.partial;view!{{issue.map(|code|view!{<p class="git-working-note">{move ||reason(&code,english.get())}</p>})}<Show when=move ||!s.editable()&&diff.partial><p class="git-working-note">{move ||tr(english.get(),"此差异已过期或未对应当前区域，请刷新后再操作。","This diff is stale or belongs to another area. Refresh before staging.")}</p></Show>{diff.hunks.into_iter().map(|hunk|{let id=hunk.id.clone();view!{<section class="git-working-hunk"><header><code>{format!("@@ -{} +{} @@{}",hunk.before,hunk.after,if hunk.continued{tr(english.get_untracked(),"（续页）"," (continued)")}else{""})}</code><button class="quiet" disabled=move ||c.busy.get()||!s.editable() on:click=move |_|s.stage(vec![id.clone()],vec![])>{move ||if s.area.get()=="index"{tr(english.get(),"取消暂存此块","Unstage hunk")}else{tr(english.get(),"暂存此块","Stage hunk")}}</button></header>{hunk.lines.into_iter().map(|line|{let is_change=line.id.is_some();let id=line.id.clone().unwrap_or_default();let change=id.clone();view!{<div class=format!("git-working-line {}",line.kind)><input type="checkbox" style:visibility=if is_change&&partial{"visible"}else{"hidden"} aria-label=format!("{} {} / {}",tr(english.get_untracked(),"选择差异行","Select changed line"),line.before.map(|n|n.to_string()).unwrap_or_default(),line.after.map(|n|n.to_string()).unwrap_or_default()) disabled=move ||c.busy.get()||id.is_empty()||!s.editable() prop:checked=move ||s.lines.with(|v|v.contains(&change)) on:change={let id=line.id.unwrap_or_default();move |ev|{let checked=event_target_checked(&ev);s.lines.update(|v|{if checked{v.insert(id.clone());}else{v.remove(&id);}});}}/><span>{line.before.map(|n|n.to_string()).unwrap_or_default()}</span><span>{line.after.map(|n|n.to_string()).unwrap_or_default()}</span><code>{line.text}</code><Show when=move ||line.no_newline><small>{move ||tr(english.get(),"文件末尾无换行","No final newline")}</small></Show><Show when=move ||line.truncated><small>{move ||tr(english.get(),"该行预览已截断；操作包含完整行","Line preview truncated; staging includes the full line")}</small></Show></div>}}).collect_view()}</section>}}).collect_view()}<pre>{diff.raw}</pre>}})}</div>
    <footer class="git-change-pagination"><span>{move ||s.diff.with(|d|d.as_ref().map(|d|format!("{}–{} / {}",if d.total_lines==0{0}else{d.offset+1},(d.offset+500).min(d.total_lines),d.total_lines)).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||s.diff.with(|d|d.as_ref().is_none_or(|d|d.offset==0)) on:click=move |_|{if let Some(d)=s.diff.get_untracked(){s.load(d.file_id,d.area,d.offset.saturating_sub(500),false);}}>"‹"</button><button class="quiet" disabled=move ||c.busy.get()||s.diff.with(|d|d.as_ref().is_none_or(|d|d.next_offset.is_none())) on:click=move |_|{if let Some(d)=s.diff.get_untracked()&&let Some(next)=d.next_offset{s.load(d.file_id,d.area,next,false);}}>"›"</button></footer>
    </main></div></section>}
}
