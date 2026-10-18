//! Searchable reference chooser and validated branch workflows.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(inline_js = "
export function branchFocus(){requestAnimationFrame(()=>{const p=document.querySelector('.git-branch-popover');const a=document.querySelector('.git-branch-entry');if(!p||!a)return;const r=a.getBoundingClientRect();p.style.left=Math.max(4,Math.min(r.left,innerWidth-p.offsetWidth-4))+'px';p.style.top=Math.max(4,Math.min(r.bottom+4,innerHeight-p.offsetHeight-4))+'px';const field=p.querySelector('.git-branch-form input:not([type=checkbox]),.git-branch-form select,.git-branch-form textarea')||p.querySelector('.git-branch-form button.primary')||p.querySelector('.git-branch-search');field?.focus();if(typeof field?.select==='function')field.select();});}
export function branchRestore(){document.querySelector('.git-branch-entry')?.focus();}
export function branchReviewFocus(){requestAnimationFrame(()=>document.querySelector('.git-action-review button')?.focus());}
export function branchCommit(){const d=document.querySelector('[data-git-commit]');if(d){d.open=true;requestAnimationFrame(()=>d.querySelector('textarea')?.focus());}}
export function branchVisible(){return !!document.querySelector('.git-panels')?.getClientRects().length;}
export function branchKeys(ev){const p=ev.currentTarget;const focusable=[...p.querySelectorAll('input,button,select,textarea,summary')].filter(e=>!e.disabled&&e.getClientRects().length);if(ev.key==='Tab'){const first=focusable[0],last=focusable.at(-1);if(ev.shiftKey&&document.activeElement===first){ev.preventDefault();last?.focus();}else if(!ev.shiftKey&&document.activeElement===last){ev.preventDefault();first?.focus();}return;}const input=ev.target.closest('input,textarea,select');if(input){if(input.matches('.git-branch-search')&&ev.key==='ArrowDown'){ev.preventDefault();p.querySelector('.git-branch-choose')?.focus();}return;}let buttons;if(ev.key==='ArrowRight'){buttons=[...p.querySelectorAll('.git-branch-actions button:not(:disabled)')];ev.preventDefault();buttons[0]?.focus();return;}if(ev.target.closest('.git-branch-ref-list'))buttons=[...p.querySelectorAll('.git-branch-choose')].filter(e=>e.getClientRects().length);else buttons=[...p.querySelectorAll('.git-branch-actions button:not(:disabled)')];const i=buttons.indexOf(document.activeElement);let n;if(ev.key==='ArrowDown')n=(i+1)%buttons.length;else if(ev.key==='ArrowUp')n=(i+buttons.length-1)%buttons.length;else if(ev.key==='Home')n=0;else if(ev.key==='End')n=buttons.length-1;else return;ev.preventDefault();buttons[n]?.focus();}
")]
extern "C" {
    #[wasm_bindgen(js_name=branchFocus)]
    fn focus();
    #[wasm_bindgen(js_name=branchRestore)]
    fn restore();
    #[wasm_bindgen(js_name=branchReviewFocus)]
    fn review_focus();
    #[wasm_bindgen(js_name=branchCommit)]
    fn commit_focus();
    #[wasm_bindgen(js_name=branchVisible)]
    fn visible() -> bool;
    #[wasm_bindgen(js_name=branchKeys)]
    fn keys(event: &JsValue);
}
#[derive(Clone)]
struct Form {
    kind: String,
    reference: Option<Reference>,
    name: String,
    start: String,
    remote: String,
    target: String,
    upstream: String,
    mode: String,
    force: bool,
    checkout: bool,
    message: String,
}
#[derive(Clone, Copy)]
struct Menu {
    c: Controller,
    english: RwSignal<bool>,
    open: RwSignal<bool>,
    query: RwSignal<String>,
    chosen: RwSignal<String>,
    form: RwSignal<Option<Form>>,
    pending: RwSignal<bool>,
}
impl Menu {
    fn close(self) {
        self.open.set(false);
        restore();
    }
    fn choose(self, reference: Reference) {
        self.chosen.set(reference.id);
        self.form.set(None);
    }
    fn selected(self) -> Option<Reference> {
        self.c.selected.with(|s| {
            s.as_ref().and_then(|s| {
                s.references
                    .iter()
                    .find(|r| r.id == self.chosen.get())
                    .cloned()
            })
        })
    }
    fn can_checkout(self) -> bool {
        self.c.selected.with(|s| {
            s.as_ref()
                .is_some_and(|s| s.repository.kind != "bare" && s.operation.is_none())
        })
    }
    fn current(self) -> Option<Reference> {
        self.c.selected.with(|s| {
            s.as_ref().and_then(|s| {
                s.references
                    .iter()
                    .find(|r| r.current && r.kind == "local")
                    .cloned()
            })
        })
    }
    fn launch(self, action: Action) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.pending.set(true);
        self.c.run(action);
    }
    fn favorite(self, reference: Reference) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        let favorite = !state
            .reference_settings
            .favorites
            .contains(&reference.full_name);
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<ReferenceSettings>("workspace_reference_favorite",serde_json::json!({"repositoryId":state.repository.id,"referenceId":reference.id,"favorite":favorite})).await{
            Ok(prefs)=>{self.c.catalog.update(|c|{c.reference_settings.insert(state.repository.id.clone(),prefs.clone());});self.c.selected.update(|s|{if let Some(s)=s&&s.repository.id==state.repository.id{s.reference_settings=prefs;}});},Err(e)=>self.c.error.set(e)
        }
            self.c.busy.set(false);
        });
    }
    fn make_form(self, kind: &str, reference: Option<Reference>) {
        let state = self.c.selected.get_untracked();
        let current = state
            .as_ref()
            .and_then(|s| s.references.iter().find(|r| r.current && r.kind == "local"));
        let first_remote = state
            .as_ref()
            .and_then(|s| s.remotes.first())
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let name = reference
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let mut remote = first_remote;
        let mut target = if kind == "push" {
            reference
                .as_ref()
                .or(current)
                .map(|r| r.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mut branch_name = name.clone();
        if let Some(r) = reference.as_ref()
            && r.kind == "remote"
            && let Some(state) = &state
            && let Some(found) = state
                .remotes
                .iter()
                .filter(|remote| r.name.starts_with(&format!("{}/", remote.name)))
                .max_by_key(|r| r.name.len())
        {
            remote = found.name.clone();
            branch_name = r.name[found.name.len() + 1..].into();
        }
        if kind == "push"
            && let Some(local) = reference.as_ref().or(current)
            && let Some(state) = &state
            && let Some(upstream) = state
                .references
                .iter()
                .find(|r| r.kind == "remote" && r.name == local.upstream)
            && let Some(found) = state
                .remotes
                .iter()
                .filter(|r| upstream.name.starts_with(&format!("{}/", r.name)))
                .max_by_key(|r| r.name.len())
        {
            remote = found.name.clone();
            target = upstream.name[found.name.len() + 1..].into();
        }
        let upstream = reference
            .as_ref()
            .and_then(|r| {
                state.as_ref().and_then(|s| {
                    s.references
                        .iter()
                        .find(|remote| remote.kind == "remote" && remote.name == r.upstream)
                        .map(|r| r.full_name.clone())
                })
            })
            .unwrap_or_default();
        let start = reference
            .as_ref()
            .map(|r| r.full_name.clone())
            .unwrap_or_default();
        self.form.set(Some(Form {
            kind: kind.into(),
            reference,
            name: if matches!(kind, "new" | "tag") {
                String::new()
            } else {
                branch_name
            },
            start,
            remote,
            target,
            upstream,
            mode: "ff_only".into(),
            force: false,
            checkout: state.as_ref().is_some_and(|s| s.repository.kind != "bare"),
            message: String::new(),
        }));
        self.open.set(true);
        focus();
    }
    fn submit(self) {
        let Some(d) = self.form.get_untracked() else {
            return;
        };
        let reference = d.reference.as_ref();
        let action = match d.kind.as_str() {
            "new" => {
                if d.checkout {
                    Action::CreateAndSwitch {
                        name: d.name,
                        start: (!d.start.is_empty()).then_some(d.start),
                    }
                } else {
                    Action::CreateBranch {
                        name: d.name,
                        start: (!d.start.is_empty()).then_some(d.start),
                    }
                }
            }
            "detach" => Action::Switch {
                reference: d.start,
                detach: true,
            },
            "tag" => Action::CreateTag {
                name: d.name,
                reference: if d.start.is_empty() {
                    "HEAD".into()
                } else {
                    d.start
                },
                message: (!d.message.trim().is_empty()).then_some(d.message),
            },
            "checkout" => {
                let Some(r) = reference else {
                    return;
                };
                Action::CheckoutRemote {
                    reference: r.full_name.clone(),
                    name: d.name,
                }
            }
            "update" => {
                let Some(r) = reference else {
                    return;
                };
                Action::SwitchAndUpdate {
                    reference: r.full_name.clone(),
                    name: (r.kind == "remote").then_some(d.name),
                    mode: d.mode,
                }
            }
            "rebase" => {
                let Some(r) = reference else {
                    return;
                };
                Action::SwitchAndRebase {
                    reference: r.full_name.clone(),
                    name: (r.kind == "remote").then_some(d.name),
                }
            }
            "rename" => {
                let Some(r) = reference else {
                    return;
                };
                Action::RenameBranch {
                    name: r.name.clone(),
                    new_name: d.name,
                }
            }
            "delete" => {
                let Some(r) = reference else {
                    return;
                };
                Action::DeleteBranch {
                    name: r.name.clone(),
                    force: d.force,
                }
            }
            "upstream" => {
                let Some(r) = reference else {
                    return;
                };
                Action::SetUpstream {
                    branch: r.name.clone(),
                    upstream: (!d.upstream.is_empty()).then_some(d.upstream),
                }
            }
            "push" => {
                let Some(r) = reference else {
                    return;
                };
                Action::PushBranch {
                    branch: r.name.clone(),
                    remote: d.remote,
                    target: d.target,
                    set_upstream: true,
                    force_with_lease: d.force,
                }
            }
            "push_tag" => {
                let Some(r) = reference else {
                    return;
                };
                Action::PushTag {
                    remote: d.remote,
                    name: r.name.clone(),
                }
            }
            _ => return,
        };
        self.launch(action);
    }
    fn label(self) -> String {
        self.c.selected.with(|s| {
            s.as_ref()
                .map(|s| {
                    if s.repository.branch == "(detached)" {
                        format!(
                            "{} · {}",
                            tr(self.english.get(), "分离 HEAD", "Detached HEAD"),
                            s.repository
                                .head
                                .as_deref()
                                .unwrap_or("")
                                .chars()
                                .take(8)
                                .collect::<String>()
                        )
                    } else {
                        s.repository.branch.clone()
                    }
                })
                .unwrap_or_default()
        })
    }
}
fn form_title(kind: &str, en: bool) -> &'static str {
    match kind {
        "new" => tr(en, "新建分支", "New branch"),
        "checkout" => tr(en, "检出远程跟踪分支", "Checkout remote tracking branch"),
        "detach" => tr(en, "检出标签或提交", "Checkout tag or revision"),
        "tag" => tr(en, "新建标签", "New tag"),
        "update" => tr(en, "切换并更新", "Checkout and update"),
        "rebase" => tr(
            en,
            "切换并变基到当前分支",
            "Checkout and rebase onto current branch",
        ),
        "rename" => tr(en, "重命名分支", "Rename branch"),
        "delete" => tr(en, "删除分支", "Delete branch"),
        "upstream" => tr(en, "设置上游分支", "Set upstream branch"),
        "push_tag" => tr(en, "推送标签", "Push tag"),
        _ => tr(en, "推送分支", "Push branch"),
    }
}
fn action_button(
    m: Menu,
    label: impl Fn() -> &'static str + Copy + Send + Sync + 'static,
    enabled: impl Fn() -> bool + Copy + Send + Sync + 'static,
    action: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {<button class="quiet" disabled=move ||m.c.busy.get()||!enabled() on:click=move |_|action()>{label}</button>}
}
fn reference_actions(m: Menu, reference: Reference) -> AnyView {
    let r = RwSignal::new(reference);
    let en = m.english;
    let local = r.with_untracked(|r| r.kind == "local");
    let remote = r.with_untracked(|r| r.kind == "remote");
    let tag = r.with_untracked(|r| r.kind == "tag");
    view!{<div class="git-reference-actions"><strong>{r.with_untracked(|r|r.full_name.clone())}</strong><code>{r.with_untracked(|r|r.target.clone())}</code><small>{r.with_untracked(|r|r.upstream.clone())}</small>
 {if local{Some(action_button(m,move ||tr(en.get(),"切换到此分支","Checkout branch"),move ||m.can_checkout()&&!r.with(|r|r.current),move ||m.launch(Action::Switch{reference:r.with_untracked(|r|r.name.clone()),detach:false})))}else{None}}
 {if remote{Some(action_button(m,move ||tr(en.get(),"检出为本地跟踪分支…","Checkout as local tracking branch…"),move ||m.can_checkout(),move ||m.make_form("checkout",Some(r.get_untracked()))))}else{None}}
 {if tag{Some(action_button(m,move ||tr(en.get(),"检出此标签（分离 HEAD）","Checkout tag (detached HEAD)"),move ||m.can_checkout(),move ||m.launch(Action::Switch{reference:r.with_untracked(|r|r.full_name.clone()),detach:true})))}else{None}}
 {action_button(m,move ||tr(en.get(),"与当前提交比较","Compare with current commit"),move ||m.c.selected.with(|s|s.as_ref().is_some_and(|s|s.repository.head.is_some())),move ||{m.c.comparison.set(Some(ReferenceComparison{left_reference_id:r.with_untracked(|r|r.id.clone()),target:CompareTarget::Current}));m.close();})}
 {action_button(m,move ||tr(en.get(),"与工作区比较","Compare with working tree"),move ||m.c.selected.with(|s|s.as_ref().is_some_and(|s|s.repository.kind!="bare")),move ||{m.c.comparison.set(Some(ReferenceComparison{left_reference_id:r.with_untracked(|r|r.id.clone()),target:CompareTarget::WorkingTree{include_untracked:false}}));m.close();})}
 {action_button(m,move ||tr(en.get(),"从此版本新建分支…","New branch from this revision…"),||true,move ||m.make_form("new",Some(r.get_untracked())))}
 {action_button(m,move ||tr(en.get(),"从此版本新建标签…","New tag from this revision…"),||true,move ||m.make_form("tag",Some(r.get_untracked())))}
 {if local||remote{Some(action_button(m,move ||tr(en.get(),"切换并更新…","Checkout and update…"),move ||m.can_checkout()&&(remote||r.with(|r|!r.upstream.is_empty())),move ||m.make_form("update",Some(r.get_untracked()))))}else{None}}
 {if local||remote{Some(action_button(m,move ||tr(en.get(),"切换并变基到当前分支…","Checkout and rebase onto current branch…"),move ||m.can_checkout()&&!r.with(|r|r.current)&&m.c.selected.with(|s|s.as_ref().is_some_and(|s|s.repository.head.is_some())),move ||m.make_form("rebase",Some(r.get_untracked()))))}else{None}}
 {action_button(m,move ||tr(en.get(),"合并到当前分支","Merge into current branch"),move ||m.can_checkout()&&!r.with(|r|r.current)&&m.current().is_some(),move ||m.launch(Action::Merge{reference:r.with_untracked(|r|r.full_name.clone())}))}
 {action_button(m,move ||tr(en.get(),"当前分支变基到此版本","Rebase current branch onto this revision"),move ||m.can_checkout()&&!r.with(|r|r.current)&&m.current().is_some(),move ||m.launch(Action::Rebase{reference:r.with_untracked(|r|r.full_name.clone())}))}
 {if local{Some(view!{<div>{action_button(m,move ||tr(en.get(),"推送此分支…","Push this branch…"),||true,move ||m.make_form("push",Some(r.get_untracked())))}{action_button(m,move ||tr(en.get(),"设置或取消上游…","Set or unset upstream…"),||true,move ||m.make_form("upstream",Some(r.get_untracked())))}{action_button(m,move ||tr(en.get(),"重命名此分支…","Rename this branch…"),||true,move ||m.make_form("rename",Some(r.get_untracked())))}{action_button(m,move ||tr(en.get(),"删除此分支…","Delete this branch…"),move ||!r.with(|r|r.current),move ||m.make_form("delete",Some(r.get_untracked())))}</div>})}else{None}}
 {if tag{Some(view!{<div>{action_button(m,move ||tr(en.get(),"推送此标签…","Push this tag…"),||true,move ||m.make_form("push_tag",Some(r.get_untracked())))}{action_button(m,move ||tr(en.get(),"删除此标签","Delete this tag"),||true,move ||m.launch(Action::DeleteTag{name:r.with_untracked(|r|r.name.clone())}))}</div>})}else{None}}
 </div>}.into_any()
}
fn form(m: Menu) -> AnyView {
    let en = m.english;
    view!{<section class="git-branch-form"><header><strong>{move ||m.form.with(|f|f.as_ref().map(|f|form_title(&f.kind,en.get())).unwrap_or_default())}</strong><button class="quiet" disabled=move ||m.c.busy.get() on:click=move |_|m.form.set(None)>{move ||tr(en.get(),"取消","Cancel")}</button></header><p>{move ||m.form.with(|f|f.as_ref().and_then(|f|f.reference.as_ref().map(|r|r.full_name.clone())).unwrap_or_default())}</p>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|matches!(f.kind.as_str(),"new"|"checkout"|"rename"|"tag")||matches!(f.kind.as_str(),"update"|"rebase")&&f.reference.as_ref().is_some_and(|r|r.kind=="remote")))><label>{move ||tr(en.get(),"分支或标签名称","Branch or tag name")}<input aria-label=move ||tr(en.get(),"分支或标签名称","Branch or tag name") prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.name.clone()).unwrap_or_default()) disabled=move ||m.c.busy.get() on:input=move |ev|m.form.update(|f|{if let Some(f)=f{f.name=event_target_value(&ev);}})/></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|matches!(f.kind.as_str(),"new"|"tag"|"detach")))><label>{move ||tr(en.get(),"起始版本（可留空）","Starting revision (optional)")}<input aria-label=move ||tr(en.get(),"起始版本","Starting revision") prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.start.clone()).unwrap_or_default()) disabled=move ||m.c.busy.get() on:input=move |ev|m.form.update(|f|{if let Some(f)=f{f.start=event_target_value(&ev);}})/></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="new"))><label><input type="checkbox" disabled=move ||m.c.busy.get()||m.c.selected.with(|s|s.as_ref().is_none_or(|s|s.repository.kind=="bare")) prop:checked=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.checkout)) on:change=move |ev|m.form.update(|f|{if let Some(f)=f{f.checkout=event_target_checked(&ev);}})/>{move ||tr(en.get(),"创建后切换","Checkout after creating")}</label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="tag"))><label>{move ||tr(en.get(),"附注（留空为轻量标签）","Annotation (empty for lightweight tag)")}<textarea aria-label=move ||tr(en.get(),"标签附注","Tag annotation") prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.message.clone()).unwrap_or_default()) disabled=move ||m.c.busy.get() on:input=move |ev|m.form.update(|f|{if let Some(f)=f{f.message=event_target_value(&ev);}})/></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|matches!(f.kind.as_str(),"delete"|"push")))><label><input type="checkbox" disabled=move ||m.c.busy.get() prop:checked=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.force)) on:change=move |ev|m.form.update(|f|{if let Some(f)=f{f.force=event_target_checked(&ev);}})/>{move ||if m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="push")){tr(en.get(),"强制推送（带租约）","Force push with lease")}else{tr(en.get(),"允许删除未合并分支","Allow deleting an unmerged branch")}}</label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|matches!(f.kind.as_str(),"push"|"push_tag")))><label>{move ||tr(en.get(),"目标远程","Target remote")}<select aria-label=move ||tr(en.get(),"目标远程","Target remote") disabled=move ||m.c.busy.get() prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.remote.clone()).unwrap_or_default()) on:change=move |ev|m.form.update(|f|{if let Some(f)=f{f.remote=event_target_value(&ev);}})>{move ||m.c.selected.with(|s|s.as_ref().map(|s|s.remotes.iter().map(|r|view!{<option value=r.name.clone()>{r.name.clone()}</option>}).collect_view()))}</select></label><small>{move ||m.c.selected.with(|s|s.as_ref().and_then(|s|s.remotes.iter().find(|r|m.form.with(|f|f.as_ref().is_some_and(|f|f.remote==r.name))).map(|r|r.push.clone())).unwrap_or_default())}</small></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="push"))><label>{move ||tr(en.get(),"远程目标分支","Remote target branch")}<input aria-label=move ||tr(en.get(),"远程目标分支","Remote target branch") prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.target.clone()).unwrap_or_default()) disabled=move ||m.c.busy.get() on:input=move |ev|m.form.update(|f|{if let Some(f)=f{f.target=event_target_value(&ev);}})/></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="upstream"))><label>{move ||tr(en.get(),"上游分支","Upstream branch")}<select aria-label=move ||tr(en.get(),"上游分支","Upstream branch") disabled=move ||m.c.busy.get() prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.upstream.clone()).unwrap_or_default()) on:change=move |ev|m.form.update(|f|{if let Some(f)=f{f.upstream=event_target_value(&ev);}})><option value="">{move ||tr(en.get(),"不跟踪上游","No upstream")}</option>{move ||m.c.selected.with(|s|s.as_ref().map(|s|s.references.iter().filter(|r|r.kind=="remote"&&!r.name.ends_with("/HEAD")).map(|r|view!{<option value=r.full_name.clone()>{r.name.clone()}</option>}).collect_view()))}</select></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="update"))><label>{move ||tr(en.get(),"更新模式","Update mode")}<select aria-label=move ||tr(en.get(),"更新模式","Update mode") disabled=move ||m.c.busy.get() prop:value=move ||m.form.with(|f|f.as_ref().map(|f|f.mode.clone()).unwrap_or_default()) on:change=move |ev|m.form.update(|f|{if let Some(f)=f{f.mode=event_target_value(&ev);}})><option value="ff_only">{move ||tr(en.get(),"仅快进","Fast-forward only")}</option><option value="merge">{move ||tr(en.get(),"合并","Merge")}</option><option value="rebase">{move ||tr(en.get(),"变基","Rebase")}</option></select></label></Show>
 <Show when=move ||m.form.with(|f|f.as_ref().is_some_and(|f|f.kind=="rebase"))><p>{move ||format!("{} {}",tr(en.get(),"切换后变基到当前分支：","After checkout, rebase onto current branch:"),m.label())}</p></Show>
 <button class="primary" disabled=move ||m.c.busy.get()||m.form.with(|f|f.as_ref().is_none_or(|f|matches!(f.kind.as_str(),"new"|"rename"|"tag"|"checkout")&&f.name.trim().is_empty()||f.kind=="detach"&&f.start.trim().is_empty()||matches!(f.kind.as_str(),"push"|"push_tag")&&f.remote.is_empty()||f.kind=="push"&&f.target.trim().is_empty())) on:click=move |_|m.submit()>{move ||tr(en.get(),"执行分支操作","Run branch action")}</button>
 </section>}.into_any()
}
pub fn panel(c: Controller, english: RwSignal<bool>) -> impl IntoView {
    let m = Menu {
        c,
        english,
        open: RwSignal::new(false),
        query: RwSignal::new(String::new()),
        chosen: RwSignal::new(String::new()),
        form: RwSignal::new(None),
        pending: RwSignal::new(false),
    };
    let repository = Memo::new(move |_| {
        c.selected
            .with(|s| s.as_ref().map(|s| s.repository.id.clone()))
    });
    Effect::new(move |_| {
        repository.get();
        m.open.set(false);
        m.form.set(None);
        m.query.set(String::new());
        m.chosen.set(String::new());
        m.pending.set(false);
    });
    Effect::new(move |_| {
        if !m.pending.get() {
            return;
        }
        if c.plan.with(|p| p.is_some()) {
            m.open.set(false);
            review_focus();
            return;
        }
        if !c.busy.get()
            && let Some(success) = c.completion.get()
        {
            m.pending.set(false);
            if success {
                m.open.set(false);
                m.form.set(None);
                restore();
            } else {
                m.open.set(true);
                focus();
            }
        }
    });
    let keyboard = window_event_listener(leptos::ev::keydown, move |ev| {
        if (ev.meta_key() || ev.ctrl_key())
            && !ev.alt_key()
            && ev.key().eq_ignore_ascii_case("k")
            && visible()
        {
            ev.prevent_default();
            if ev.shift_key() {
                if let Some(current) = m.current() {
                    m.make_form("push", Some(current));
                } else {
                    m.open.set(true);
                    focus();
                }
            } else {
                m.close();
                commit_focus();
            }
        }
    });
    on_cleanup(move || keyboard.remove());
    view! {<div class="git-branch-control"><button class="quiet git-branch-entry" aria-label=move ||tr(english.get(),"分支菜单","Branch menu") aria-expanded=move ||m.open.get() disabled=move ||c.busy.get() on:click=move |_|{m.open.update(|v|*v = !*v);if m.open.get_untracked(){focus();}}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4v8m0-4h4a4 4 0 0 0 4-4"/><circle cx="4" cy="3" r="2"/><circle cx="4" cy="13" r="2"/><circle cx="12" cy="3" r="2"/></svg>{move ||m.label()}" ▾"</button>
    <Show when=move ||m.open.get()><div class="git-branch-backdrop" on:pointerdown=move |_|m.close() on:contextmenu=move |ev|{ev.prevent_default();m.close();}><section class="git-branch-popover" role="dialog" aria-modal="true" aria-label=move ||tr(english.get(),"分支与版本","Branches and revisions") on:pointerdown=move |ev|ev.stop_propagation() on:contextmenu=move |ev|ev.prevent_default() on:keydown=move |ev|{if ev.key()=="Escape"{ev.prevent_default();if m.form.with_untracked(|f|f.is_some()){m.form.set(None);focus();}else{m.close();}}else{keys(ev.as_ref());}}>
    <header><input class="git-branch-search" aria-label=move ||tr(english.get(),"搜索分支与操作","Search branches and actions") placeholder=move ||tr(english.get(),"搜索分支、标签或操作…","Search branches, tags or actions…") prop:value=move ||m.query.get() on:input=move |ev|m.query.set(event_target_value(&ev))/><button class="quiet" aria-label=move ||tr(english.get(),"关闭分支菜单","Close branch menu") on:click=move |_|m.close()>"×"</button></header>
    <div class="git-branch-quick">{[("new","新建分支…","New branch…"),("tag","新建标签…","New tag…"),("detach","检出标签或提交…","Checkout tag or revision…"),("commit","提交…","Commit…"),("push","推送当前分支…","Push current branch…"),("update","更新当前分支","Update current branch")].into_iter().map(move |(kind,zh,en)|view!{<button class="quiet" style:display=move ||if tr(english.get(),zh,en).to_lowercase().contains(&m.query.get().to_lowercase())||m.query.get().is_empty(){"inline-block"}else{"none"} disabled=move ||c.busy.get()||(matches!(kind,"detach"|"update"|"commit")&&!m.can_checkout())||(kind=="push"&&m.current().is_none())||(kind=="update"&&m.current().is_none_or(|r|r.upstream.is_empty())) on:click=move |_|{match kind{"commit"=>{m.close();commit_focus();},"push"=>m.make_form("push",m.current()),"update"=>m.launch(Action::UpdateCurrent{mode:"ff_only".into()}),_=>m.make_form(kind,None)}}>{move ||tr(english.get(),zh,en)}</button>}).collect_view()}</div>
    <Show when=move ||!c.error.get().is_empty()><p class="git-branch-error" role="status">{move ||c.error.get()}</p></Show>
    <div class="git-branch-content"><div class="git-branch-ref-list">{["recent","favorites","local","remote","tag"].into_iter().map(move |group|view!{<details class="git-branch-group" data-kind=group open><summary>{move ||match group{"recent"=>tr(english.get(),"最近使用","Recent"),"favorites"=>tr(english.get(),"收藏","Favorites"),"local"=>tr(english.get(),"本地分支","Local branches"),"remote"=>tr(english.get(),"远程分支","Remote branches"),_=>tr(english.get(),"标签","Tags")}}</summary><For each=move ||c.selected.with(|s|s.as_ref().map(|s|{let q=m.query.get().to_lowercase();let mut refs=s.references.iter().filter(|r|!r.name.ends_with("/HEAD")&&(r.name.to_lowercase().contains(&q)||r.full_name.to_lowercase().contains(&q))&&match group{"recent"=>s.reference_settings.recent.contains(&r.full_name),"favorites"=>s.reference_settings.favorites.contains(&r.full_name),_=>r.kind==group}).cloned().collect::<Vec<_>>();if group=="recent"{refs.sort_by_key(|r|s.reference_settings.recent.iter().position(|n|n==&r.full_name));}refs.truncate(200);refs}).unwrap_or_default()) key=|r|r.full_name.clone() children=move |reference|{let chosen=reference.clone();let right=reference.clone();let focus_id=reference.id.clone();let select=reference.id.clone();let star=reference.clone();let starred=reference.full_name.clone();view!{<div class="git-branch-row" class:selected=move ||m.chosen.get()==select><button class="git-branch-choose" title=reference.full_name.clone() on:click=move |_|m.choose(chosen.clone()) on:focus=move |_|m.chosen.set(focus_id.clone()) on:contextmenu=move |ev|{ev.prevent_default();m.choose(right.clone());}><span>{if reference.current{"● "}else{""}}{reference.name}</span><small>{reference.upstream}</small></button><button class="quiet git-reference-star" aria-label=move ||if c.selected.with(|s|s.as_ref().is_some_and(|s|s.reference_settings.favorites.contains(&starred))){tr(english.get(),"取消收藏引用","Unfavorite reference")}else{tr(english.get(),"收藏引用","Favorite reference")} disabled=move ||c.busy.get() on:click=move |_|m.favorite(star.clone())>{move ||if c.selected.with(|s|s.as_ref().is_some_and(|s|s.reference_settings.favorites.contains(&reference.full_name))){"★"}else{"☆"}}</button></div>}}/></details>}).collect_view()}<small class="git-branch-limit">{move ||tr(english.get(),"每组最多显示 200 项，可搜索定位更多引用。","Up to 200 references per group; search to find others.")}</small></div>
    <aside class="git-branch-actions">{move ||if m.form.with(|f|f.is_some()){form(m)}else if let Some(reference)=m.selected(){reference_actions(m,reference)}else{view!{<p>{move ||tr(english.get(),"选择分支或标签查看操作。最近使用和收藏按仓库保存。","Select a branch or tag for actions. Recent and favorite references are saved per repository.")}</p>}.into_any()}}</aside></div></section></div></Show></div>}
}
