//! Typed Git controls shared by the repository toolbar and changed-file rows.
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};

#[derive(Clone, Copy)]
pub struct Controller {
    pub epoch: RwSignal<u64>,
    pub completion_epoch: RwSignal<u64>,
    pub worktrees: RwSignal<bool>,
    pub worktree_start: RwSignal<Option<String>>,
    pub remotes: RwSignal<bool>,
    pub stashes: RwSignal<bool>,
    pub comparison: RwSignal<Option<ReferenceComparison>>,
    pub selected: RwSignal<Option<RepositoryState>>,
    pub catalog: RwSignal<Catalog>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub plan: RwSignal<Option<ActionPlan>>,
    pub log: RwSignal<String>,
    pub completion: RwSignal<Option<bool>>,
}
impl Controller {
    pub(super) fn accept(self, state: RepositoryState) {
        self.catalog.update(|c| {
            c.reference_settings.insert(
                state.repository.id.clone(),
                state.reference_settings.clone(),
            );
            if let Some(repo) = c
                .repositories
                .iter_mut()
                .find(|r| r.id == state.repository.id)
            {
                *repo = state.repository.clone();
            }
        });
        self.selected.set(Some(state));
    }
    async fn execute(self, plan: ActionPlan) {
        let epoch = self.epoch.get_untracked();
        if self.selected.with_untracked(|s| {
            s.as_ref()
                .is_none_or(|s| s.repository.id != plan.repository_id)
        }) {
            self.error.set(
                "Repository selection changed; review the action again in its repository".into(),
            );
            self.completion_epoch.set(epoch);
            self.completion.set(Some(false));
            self.busy.set(false);
            return;
        }
        match invoke::<ActionResult>("workspace_apply", serde_json::json!({"planId":plan.id})).await
        {
            Ok(result) => {
                let recovery = result
                    .backup_reference
                    .map(|r| format!("\n{r}"))
                    .unwrap_or_default();
                let config = result
                    .recovery_path
                    .map(|p| format!("\n{p}"))
                    .unwrap_or_default();
                self.log.set(format!("{}{recovery}{config}", result.output));
                self.accept(result.state);
                self.completion_epoch.set(epoch);
                self.completion.set(Some(true));
            }
            Err(e) => {
                self.error.set(e);
                self.completion_epoch.set(epoch);
                self.completion.set(Some(false));
                // Failed merges can still change the index and enter a conflict state.
                if let Ok(state) = invoke::<RepositoryState>(
                    "workspace_open",
                    serde_json::json!({"repositoryId":plan.repository_id}),
                )
                .await
                {
                    self.accept(state);
                }
            }
        }
        self.busy.set(false);
    }
    pub fn run(self, action: Action) {
        if self.busy.get_untracked() {
            return;
        }
        let Some(state) = self.selected.get_untracked() else {
            return;
        };
        self.epoch.update(|n| *n = n.wrapping_add(1));
        let epoch = self.epoch.get_untracked();
        self.busy.set(true);
        self.completion.set(None);
        self.error.set(String::new());
        self.plan.set(None);
        spawn_local(async move {
            match invoke::<ActionPlan>("workspace_prepare", serde_json::json!({"repositoryId":state.repository.id,"revision":state.revision,"action":action})).await {
                Ok(plan) if plan.dangerous => { self.plan.set(Some(plan)); self.busy.set(false); }
                Ok(plan) => self.execute(plan).await,
                Err(e) => { self.error.set(e); self.completion_epoch.set(epoch); self.completion.set(Some(false)); self.busy.set(false); }
            }
        });
    }
}
pub fn panel(c: Controller, english: RwSignal<bool>) -> impl IntoView {
    let message = RwSignal::new(String::new());
    let amend = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let reference = RwSignal::new(String::new());
    let operation = RwSignal::new("create_branch".to_string());
    let remote = RwSignal::new(String::new());
    let pull_mode = RwSignal::new("ff_only".to_string());
    let run_branch = move || {
        let reference = reference.get_untracked();
        let name = name.get_untracked();
        let action = match operation.get_untracked().as_str() {
            "create_branch" => Action::CreateBranch {
                name,
                start: (!reference.is_empty()).then_some(reference),
            },
            "switch" => Action::Switch {
                reference,
                detach: false,
            },
            "detach" => Action::Switch {
                reference,
                detach: true,
            },
            "rename" => Action::RenameBranch {
                name: reference,
                new_name: name,
            },
            "delete" => Action::DeleteBranch {
                name: reference,
                force: false,
            },
            "force_delete" => Action::DeleteBranch {
                name: reference,
                force: true,
            },
            "merge" => Action::Merge { reference },
            "rebase" => Action::Rebase { reference },
            "cherry_pick" => Action::CherryPick { reference },
            "revert" => Action::Revert { reference },
            "tag" => Action::CreateTag {
                name,
                reference,
                message: None,
            },
            "delete_tag" => Action::DeleteTag { name: reference },
            "upstream" => Action::SetUpstream {
                branch: name,
                upstream: Some(reference),
            },
            _ => return,
        };
        c.run(action);
    };
    view! {
        <section class="git-action-controls" aria-label=move ||tr(english.get(),"Git 操作","Git actions")>
            <div class="git-quick-actions">
                {super::git_branches::panel(c,english)}
                <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|c.run(Action::Fetch {remote:None,prune:true})>{move ||tr(english.get(),"获取全部远程更新","Fetch all remotes")}</button>
                <details data-git-commit><summary>{move ||tr(english.get(),"提交","Commit")}</summary><div class="git-action-form">
                    <textarea aria-label=move ||tr(english.get(),"提交说明","Commit message") placeholder=move ||tr(english.get(),"填写提交说明；仅提交已暂存的内容","Enter a message; commit staged changes only") prop:value=move ||message.get() on:input=move |ev|message.set(event_target_value(&ev))/>
                    <label><input type="checkbox" prop:checked=move ||amend.get() on:change=move |ev|amend.set(event_target_checked(&ev))/>{move ||tr(english.get(),"修改最后一次提交","Amend last commit")}</label>
                    <button class="primary" disabled=move ||c.busy.get()||message.get().trim().is_empty() on:click=move |_|c.run(Action::Commit {message:message.get_untracked(),amend:amend.get_untracked()})>{move ||tr(english.get(),"提交已暂存文件","Commit staged files")}</button>
                </div></details>
                <details><summary>{move ||tr(english.get(),"分支与版本操作","Branch and revision actions")}</summary><div class="git-action-form">
                    <select aria-label=move ||tr(english.get(),"版本操作","Revision action") prop:value=move ||operation.get() on:change=move |ev|operation.set(event_target_value(&ev))>
                        {[("create_branch","新建分支","New branch"),("switch","切换分支","Switch branch"),("detach","检出提交或标签","Checkout revision or tag"),("rename","重命名分支","Rename branch"),("delete","删除已合并分支","Delete merged branch"),("force_delete","强制删除分支","Force delete branch"),("merge","合并到当前分支","Merge into current branch"),("rebase","当前分支变基到此版本","Rebase current branch onto revision"),("cherry_pick","拣选提交","Cherry-pick commit"),("revert","撤销提交并生成反向提交","Revert commit"),("tag","新建轻量标签","New lightweight tag"),("delete_tag","删除标签","Delete tag"),("upstream","设置上游分支","Set upstream")].into_iter().map(move |(v,zh,en)|view!{<option value=v>{move ||tr(english.get(),zh,en)}</option>}).collect_view()}
                    </select>
                    <input aria-label=move ||tr(english.get(),"分支或提交","Branch or revision") placeholder=move ||tr(english.get(),"已有分支、标签或提交；新分支可留空","Existing branch, tag or revision; optional for new branch") prop:value=move ||reference.get() on:input=move |ev|reference.set(event_target_value(&ev))/>
                    <input aria-label=move ||tr(english.get(),"新名称或本地分支","New name or local branch") placeholder=move ||tr(english.get(),"新名称／设置上游的本地分支","New name / local branch for upstream") prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev))/>
                    <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|run_branch()>{move ||tr(english.get(),"执行版本操作","Run revision action")}</button>
                </div></details>
                <details><summary>{move ||tr(english.get(),"拉取与推送","Pull and push")}</summary><div class="git-action-form">
                    <input aria-label=move ||tr(english.get(),"远程名称","Remote name") placeholder="origin" prop:value=move ||remote.get() on:input=move |ev|remote.set(event_target_value(&ev))/>
                    <input aria-label=move ||tr(english.get(),"远程操作分支","Remote action branch") placeholder=move ||tr(english.get(),"分支名称","Branch name") prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev))/>
                    <select aria-label=move ||tr(english.get(),"拉取模式","Pull mode") prop:value=move ||pull_mode.get() on:change=move |ev|pull_mode.set(event_target_value(&ev))><option value="ff_only">{move ||tr(english.get(),"仅快进","Fast-forward only")}</option><option value="merge">{move ||tr(english.get(),"合并","Merge")}</option><option value="rebase">{move ||tr(english.get(),"变基","Rebase")}</option></select>
                    <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|c.run(Action::Pull {remote:remote.get_untracked(),branch:name.get_untracked(),mode:pull_mode.get_untracked()})>{move ||tr(english.get(),"拉取","Pull")}</button>
                    <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|c.run(Action::Push {remote:remote.get_untracked(),branch:name.get_untracked(),set_upstream:true,force_with_lease:false})>{move ||tr(english.get(),"推送并设置上游","Push and set upstream")}</button>
                </div></details>
                <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{c.comparison.set(None);c.remotes.set(false);c.worktrees.set(false);c.stashes.set(true);}>{move ||tr(english.get(),"贮藏管理","Stash manager")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{c.comparison.set(None);c.stashes.set(false);c.worktrees.set(false);c.remotes.set(true);}>{move ||tr(english.get(),"远程管理","Remote manager")}</button>
                <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{c.comparison.set(None);c.stashes.set(false);c.remotes.set(false);c.worktrees.set(true);}>{move ||tr(english.get(),"工作区管理","Worktrees")}</button>
            </div>
            <Show when=move ||c.selected.with(|s|s.as_ref().is_some_and(|s|s.operation.is_some()))>
                <div class="git-quick-actions"><span>{move ||tr(english.get(),"进行中的 Git 操作","Git operation in progress")}</span>
                    {[("continue","继续","Continue"),("skip","跳过","Skip"),("abort","中止","Abort")].into_iter().map(move |(kind,zh,en)|view!{<button class="quiet" disabled=move ||c.busy.get()||c.selected.with(|s|s.as_ref().is_none_or(|s|kind=="skip"&&s.operation.as_deref()==Some("merge"))) on:click=move |_|{if let Some(operation)=c.selected.get_untracked().and_then(|s|s.operation){c.run(match kind {"continue"=>Action::Continue{operation},"skip"=>Action::Skip{operation},_=>Action::Abort{operation}})}}>{move ||tr(english.get(),zh,en)}</button>}).collect_view()}
                </div>
            </Show>
            <Show when=move ||c.plan.with(|p|p.is_some())><section class="git-action-review" role="region" aria-label=move ||tr(english.get(),"确认 Git 操作","Review Git action")><strong>{move ||tr(english.get(),"请确认操作范围；历史或文件可能改变","Review the action; history or files may change")}</strong><code>{move ||c.plan.with(|p|p.as_ref().map(|p|format!("git {}",p.arguments.join(" "))).unwrap_or_default())}</code><code>{move ||c.plan.with(|p|p.as_ref().and_then(|p|p.backup_reference.clone()).unwrap_or_default())}</code><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{c.plan.set(None);c.completion_epoch.set(c.epoch.get_untracked());c.completion.set(Some(false));}>{move ||tr(english.get(),"取消操作","Cancel action")}</button><button class="primary" disabled=move ||c.busy.get() on:click=move |_|{if let Some(plan)=c.plan.get_untracked(){c.plan.set(None);c.busy.set(true);spawn_local(c.execute(plan));}}>{move ||tr(english.get(),"确认执行","Confirm action")}</button></section></Show>
            <Show when=move ||!c.log.get().is_empty()><details class="git-action-log"><summary>{move ||tr(english.get(),"上次 Git 操作结果","Last Git action result")}</summary><pre>{move ||c.log.get()}</pre></details></Show>
        </section>
    }
}
