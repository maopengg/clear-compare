//! Paged stash manager; mutations share reviewed native Git action plans.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Stashes {
    c: Controller,
    page: RwSignal<Option<StashPage>>,
    detail: RwSignal<Option<StashDetail>>,
    patch: RwSignal<Option<GitPatch>>,
    selected: RwSignal<String>,
    area: RwSignal<String>,
    attempted: RwSignal<String>,
    query: RwSignal<String>,
    file_query: RwSignal<String>,
    file_offset: RwSignal<usize>,
    file: RwSignal<String>,
    message: RwSignal<String>,
    include: RwSignal<bool>,
    restore: RwSignal<bool>,
    branch: RwSignal<String>,
    pending: RwSignal<Option<String>>,
}
fn key(state: &RepositoryState) -> String {
    format!("{}/{}", state.repository.id, state.revision)
}
impl Stashes {
    fn load(self, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.attempted.set(key(&state));
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<StashPage>(
                "workspace_stashes",
                serde_json::json!({"repositoryId":state.repository.id,"offset":offset}),
            )
            .await
            {
                Ok(page) => {
                    self.attempted.set(key(&page.state));
                    self.c.accept(page.state.clone());
                    if !page
                        .entries
                        .iter()
                        .any(|e| e.id == self.selected.get_untracked())
                    {
                        self.selected.set(String::new());
                        self.detail.set(None);
                        self.patch.set(None);
                    }
                    self.page.set(Some(page));
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn open(self, id: String, area: String) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<StashDetail>(
                "workspace_stash_detail",
                serde_json::json!({"repositoryId":state.repository.id,"stashId":id,"area":area}),
            )
            .await
            {
                Ok(detail) => {
                    self.selected.set(id);
                    self.area.set(detail.area.clone());
                    self.detail.set(Some(detail));
                    self.patch.set(None);
                    self.file.set(String::new());
                    self.file_offset.set(0);
                    self.file_query.set(String::new());
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn patch(self, file: String, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let (Some(state), Some(detail)) =
            (self.c.selected.get_untracked(), self.detail.get_untracked())
        else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<GitPatch>("workspace_commit_patch",serde_json::json!({"repositoryId":state.repository.id,"detailId":detail.detail.id,"fileId":file,"offset":offset})).await{Ok(p)=>{self.file.set(file);self.patch.set(Some(p));},Err(e)=>self.c.error.set(e)}
            self.c.busy.set(false);
        });
    }
    fn mutable(self) -> bool {
        self.c.selected.with(|s| {
            s.as_ref().is_some_and(|s| {
                s.repository.kind != "bare"
                    && s.repository.head.is_some()
                    && s.repository.conflicts == 0
                    && s.operation.is_none()
            })
        })
    }
    fn run(self, operation: StashOperation) {
        let Some(detail) = self.detail.get_untracked() else {
            return;
        };
        let branching = matches!(&operation, StashOperation::Branch);
        self.pending.set(Some(if branching {
            "branch".into()
        } else {
            "use".into()
        }));
        self.c.run(Action::StashUse {
            id: detail.entry.id,
            operation,
            name: branching.then(|| self.branch.get_untracked()),
            restore_index: self.restore.get_untracked(),
        });
    }
}
fn area(value: &str, en: bool) -> &'static str {
    match value {
        "index" => tr(en, "已暂存修改", "Staged changes"),
        "unstaged" => tr(en, "未暂存修改", "Unstaged changes"),
        "untracked" => tr(en, "未跟踪文件", "Untracked files"),
        _ => tr(en, "全部已跟踪修改", "All tracked changes"),
    }
}
pub fn panel(c: Controller, en: RwSignal<bool>) -> impl IntoView {
    let v = Stashes {
        c,
        page: RwSignal::new(None),
        detail: RwSignal::new(None),
        patch: RwSignal::new(None),
        selected: RwSignal::new(String::new()),
        area: RwSignal::new("tracked".into()),
        attempted: RwSignal::new(String::new()),
        query: RwSignal::new(String::new()),
        file_query: RwSignal::new(String::new()),
        file_offset: RwSignal::new(0),
        file: RwSignal::new(String::new()),
        message: RwSignal::new(String::new()),
        include: RwSignal::new(false),
        restore: RwSignal::new(false),
        branch: RwSignal::new(String::new()),
        pending: RwSignal::new(None),
    };
    let repository = Memo::new(move |_| {
        c.selected
            .with(|s| s.as_ref().map(|s| s.repository.id.clone()))
    });
    Effect::watch(
        move || repository.get(),
        move |_, _, _| {
            c.stashes.set(false);
            v.page.set(None);
            v.detail.set(None);
            v.patch.set(None);
            v.selected.set(String::new());
            v.attempted.set(String::new());
            v.pending.set(None);
        },
        false,
    );
    Effect::watch(
        move || c.comparison.with(|r| r.is_some()),
        move |comparing, _, _| {
            if *comparing {
                c.stashes.set(false);
            }
        },
        false,
    );
    Effect::new(move |_| {
        if !c.stashes.get() {
            v.attempted.set(String::new());
            return;
        }
        if c.busy.get() || c.plan.with(|p| p.is_some()) {
            return;
        }
        if let Some(state) = c.selected.get()
            && v.attempted.get() != key(&state)
        {
            v.load(0);
        }
    });
    Effect::new(move || {
        if v.pending.with(|p| p.is_some())
            && !c.busy.get()
            && let Some(success) = c.completion.get()
        {
            let pending = v.pending.get_untracked();
            v.pending.set(None);
            if success && pending.as_deref() == Some("save") {
                v.message.set(String::new());
            }
            if success && pending.as_deref() == Some("branch") {
                v.branch.set(String::new());
            }
        }
    });
    let files = Memo::new(move |_| {
        v.detail.with(|d| {
            d.as_ref()
                .map(|d| {
                    let q = v.file_query.get().to_lowercase();
                    d.detail
                        .files
                        .iter()
                        .filter(|f| f.path.to_lowercase().contains(&q))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });
    view! {<Show when=move ||c.stashes.get()><section class="git-stashes" aria-label=move ||tr(en.get(),"贮藏管理","Stash manager")>
    <header><strong>{move ||tr(en.get(),"贮藏管理","Stash manager")}</strong><details class="git-stash-create"><summary>{move ||tr(en.get(),"保存当前修改","Save current changes")}</summary><div><input aria-label=move ||tr(en.get(),"贮藏说明","Stash message") placeholder=move ||tr(en.get(),"填写贮藏说明","Enter a stash message") disabled=move ||c.busy.get() prop:value=move ||v.message.get() on:input=move |ev|v.message.set(event_target_value(&ev))/><label><input type="checkbox" disabled=move ||c.busy.get() prop:checked=move ||v.include.get() on:change=move |ev|v.include.set(event_target_checked(&ev))/>{move ||tr(en.get(),"包含未跟踪文件","Include untracked files")}</label><button class="quiet" disabled=move ||c.busy.get()||!v.mutable()||v.message.get().trim().is_empty() on:click=move |_|{v.pending.set(Some("save".into()));c.run(Action::StashSave{message:v.message.get_untracked(),include_untracked:v.include.get_untracked()});}>{move ||tr(en.get(),"保存贮藏","Save stash")}</button></div></details><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.load(v.page.with_untracked(|p|p.as_ref().map(|p|p.offset).unwrap_or(0)))>{move ||tr(en.get(),"刷新贮藏","Refresh stashes")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|c.stashes.set(false)>{move ||tr(en.get(),"返回仓库","Back to repository")}</button></header>
    <div class="git-stash-body"><aside class="git-stash-list"><input aria-label=move ||tr(en.get(),"筛选本页贮藏","Filter this stash page") placeholder=move ||tr(en.get(),"筛选本页说明、作者…","Filter page messages, authors…") prop:value=move ||v.query.get() on:input=move |ev|v.query.set(event_target_value(&ev))/><div class="git-stash-entries"><For each=move ||v.page.with(|p|p.as_ref().map(|p|{let q=v.query.get().to_lowercase();p.entries.iter().filter(|e|format!("{} {} {}",e.message,e.author,e.object).to_lowercase().contains(&q)).cloned().collect::<Vec<_>>()}).unwrap_or_default()) key=|e|e.id.clone() children=move |entry|{let id=entry.id.clone();let selected=entry.id.clone();view!{<button class="git-stash-entry" class:selected=move ||v.selected.get()==selected disabled=move ||c.busy.get() on:click=move |_|v.open(id.clone(),"tracked".into())><strong>{format!("stash@{{{}}}",entry.position)}</strong><span>{entry.message}</span><small>{format!("{} · {}",entry.author,entry.date)}</small><code title=entry.object.clone()>{entry.object.chars().take(12).collect::<String>()}</code></button>}}/><Show when=move ||!c.busy.get()&&v.page.with(|p|p.as_ref().is_some_and(|p|p.entries.is_empty()))><p>{move ||tr(en.get(),"当前没有贮藏记录。","No stash entries.")}</p></Show></div><footer><button class="quiet" disabled=move ||c.busy.get()||v.page.with(|p|p.as_ref().is_none_or(|p|p.offset==0)) on:click=move |_|v.load(v.page.with_untracked(|p|p.as_ref().map(|p|p.offset.saturating_sub(100)).unwrap_or(0)))>"‹"</button><span>{move ||v.page.with(|p|p.as_ref().map(|p|format!("{}–{}",p.offset+usize::from(!p.entries.is_empty()),p.offset+p.entries.len())))}</span><button class="quiet" disabled=move ||c.busy.get()||v.page.with(|p|p.as_ref().is_none_or(|p|!p.has_more)) on:click=move |_|v.load(v.page.with_untracked(|p|p.as_ref().map(|p|p.offset+100).unwrap_or(0)))>"›"</button></footer></aside>
    <main class="git-stash-detail"><Show when=move ||v.detail.with(|d|d.is_some()) fallback=move ||view!{<p class="git-stash-empty">{move ||tr(en.get(),"选择贮藏查看文件与差异。","Select a stash to inspect its files and changes.")}</p>}>
    <header><strong>{move ||v.detail.with(|d|d.as_ref().map(|d|format!("{} · {}",d.entry.message,area(&d.area,en.get()))))}</strong><select aria-label=move ||tr(en.get(),"贮藏比较区域","Stash comparison area") disabled=move ||c.busy.get() prop:value=move ||v.area.get() on:change=move |ev|v.open(v.selected.get_untracked(),event_target_value(&ev))>{["tracked","index","unstaged","untracked"].into_iter().map(move |a|view!{<option value=a disabled=move ||a=="untracked"&&v.detail.with(|d|d.as_ref().is_none_or(|d|d.entry.parents.len()<3))>{move ||area(a,en.get())}</option>}).collect_view()}</select></header>
    <div class="git-stash-actions"><label><input type="checkbox" disabled=move ||c.busy.get() prop:checked=move ||v.restore.get() on:change=move |ev|v.restore.set(event_target_checked(&ev))/>{move ||tr(en.get(),"恢复暂存区状态","Restore index state")}</label>{[(StashOperation::Apply,"应用贮藏","Apply stash"),(StashOperation::Pop,"弹出贮藏","Pop stash"),(StashOperation::Drop,"删除贮藏","Drop stash")].into_iter().map(move |(op,zh,english)|view!{<button class="quiet" disabled=move ||c.busy.get()||!v.mutable() on:click=move |_|v.run(op.clone())>{move ||tr(en.get(),zh,english)}</button>}).collect_view()}<input aria-label=move ||tr(en.get(),"从贮藏创建的分支名","New branch from stash") placeholder=move ||tr(en.get(),"新分支名称","New branch name") disabled=move ||c.busy.get() prop:value=move ||v.branch.get() on:input=move |ev|v.branch.set(event_target_value(&ev))/><button class="quiet" disabled=move ||c.busy.get()||!v.mutable()||v.branch.get().trim().is_empty() on:click=move |_|v.run(StashOperation::Branch)>{move ||tr(en.get(),"从贮藏新建分支","Branch from stash")}</button></div>
    <div class="git-stash-content"><aside><input aria-label=move ||tr(en.get(),"筛选贮藏文件","Filter stash files") placeholder=move ||tr(en.get(),"搜索文件…","Search files…") prop:value=move ||v.file_query.get() on:input=move |ev|{v.file_query.set(event_target_value(&ev));v.file_offset.set(0);}/><div class="git-stash-files"><Show when=move ||files.with(|f|f.is_empty())><p class="git-stash-empty">{move ||tr(en.get(),"当前区域没有匹配的文件变化，可切换比较区域或调整搜索。","No matching changes in this area. Change the area or filter.")}</p></Show><For each=move ||files.with(|f|f.iter().skip(v.file_offset.get()).take(200).cloned().collect::<Vec<_>>()) key=|f|f.id.clone() children=move |f|{let id=f.id.clone();let selected=f.id.clone();let title=f.path.clone();view!{<button class="git-stash-file" title=title class:selected=move ||v.file.get()==selected disabled=move ||c.busy.get() on:click=move |_|v.patch(id.clone(),0)><small>{super::workbench::change_label(&f.status,en.get()).to_owned()}</small><span>{f.path}</span></button>}}/></div><footer><button class="quiet" disabled=move ||v.file_offset.get()==0 on:click=move |_|v.file_offset.update(|n|*n=n.saturating_sub(200))>"‹"</button><span>{move ||format!("{}–{} / {}",v.file_offset.get()+usize::from(files.with(|f|!f.is_empty())),(v.file_offset.get()+200).min(files.with(|f|f.len())),files.with(|f|f.len()))}</span><button class="quiet" disabled=move ||{v.file_offset.get()+200>=files.with(|f|f.len())} on:click=move |_|v.file_offset.update(|n|*n+=200)>"›"</button></footer></aside>
    <section class="git-stash-patch"><header>{move ||v.patch.with(|p|p.as_ref().map(|p|p.path.clone())).unwrap_or_else(||tr(en.get(),"选择文件查看补丁","Select a file to view its patch").into())}</header><div class="git-stash-patch-scroll">{move ||v.patch.with(|p|p.as_ref().map(|p|p.text.lines().map(|line|{let css=if line.starts_with('+')&&!line.starts_with("+++"){"add"}else if line.starts_with('-')&&!line.starts_with("---"){"delete"}else{"context"};view!{<pre class=css>{line.to_owned()}</pre>}}).collect_view()))}</div><Show when=move ||v.patch.with(|p|p.as_ref().is_some_and(|p|p.binary||p.long_lines))><small>{move ||tr(en.get(),"二进制差异或过长行按 Git 补丁限制显示。","Binary changes or long lines use bounded Git patch output.")}</small></Show><footer><button class="quiet" disabled=move ||c.busy.get()||v.patch.with(|p|p.as_ref().is_none_or(|p|p.offset==0)) on:click=move |_|v.patch(v.file.get_untracked(),v.patch.with_untracked(|p|p.as_ref().map(|p|p.offset.saturating_sub(500)).unwrap_or(0)))>"‹"</button><span>{move ||v.patch.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",p.offset+usize::from(p.total_lines>0),(p.offset+500).min(p.total_lines),p.total_lines)))}</span><button class="quiet" disabled=move ||c.busy.get()||v.patch.with(|p|p.as_ref().is_none_or(|p|p.next_offset.is_none())) on:click=move |_|{if let Some(offset)=v.patch.with_untracked(|p|p.as_ref().and_then(|p|p.next_offset)){v.patch(v.file.get_untracked(),offset);}}>"›"</button></footer></section></div>
    </Show></main></div>
    </section></Show>}
}
