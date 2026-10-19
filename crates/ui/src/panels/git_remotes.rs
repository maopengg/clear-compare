//! Repository-local remote settings; credentials remain in native Git configuration.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::Action;
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Remotes {
    c: Controller,
    selected: RwSignal<String>,
    form: RwSignal<Option<String>>,
    name: RwSignal<String>,
    url: RwSignal<String>,
    pending: RwSignal<Option<String>>,
    expected: RwSignal<u64>,
    probe: RwSignal<Option<RemoteProbe>>,
}
impl Remotes {
    fn chosen(self) -> Option<Remote> {
        self.c.selected.with(|s| {
            s.as_ref().and_then(|s| {
                s.remotes
                    .iter()
                    .find(|r| r.name == self.selected.get())
                    .or(s.remotes.first())
                    .cloned()
            })
        })
    }
    fn begin(self, kind: &str) {
        if self.c.busy.get_untracked() || self.form.with_untracked(|f| f.is_some()) {
            return;
        }
        let target = self.chosen();
        self.name.set(if kind == "add" {
            self.c.selected.with_untracked(|s| {
                if s.as_ref()
                    .is_some_and(|s| s.remotes.iter().any(|r| r.name == "origin"))
                {
                    "upstream".into()
                } else {
                    "origin".into()
                }
            })
        } else {
            target.map(|r| r.name).unwrap_or_default()
        });
        self.url.set(String::new());
        self.form.set(Some(kind.into()));
    }
    fn submit(self) {
        let Some(kind) = self.form.get_untracked() else {
            return;
        };
        let action = if kind == "add" {
            Action::AddRemote {
                name: self.name.get_untracked(),
                url: self.url.get_untracked(),
            }
        } else {
            Action::SetRemote {
                name: self.name.get_untracked(),
                url: self.url.get_untracked(),
                push: kind == "push",
            }
        };
        self.expected
            .set(self.c.epoch.get_untracked().wrapping_add(1));
        self.pending.set(Some(kind));
        self.c.run(action);
    }
    fn run(self, remove: bool) {
        let Some(remote) = self.chosen() else {
            return;
        };
        self.expected
            .set(self.c.epoch.get_untracked().wrapping_add(1));
        self.pending.set(Some(if remove {
            "remove".into()
        } else {
            "clear".into()
        }));
        self.c.run(if remove {
            Action::RemoveRemote { name: remote.name }
        } else {
            Action::ClearRemotePush { name: remote.name }
        });
    }
    fn probe(self, push: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let (Some(state), Some(remote)) = (self.c.selected.get_untracked(), self.chosen()) else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RemoteProbe>("workspace_remote_probe",serde_json::json!({"repositoryId":state.repository.id,"remote":remote.name,"push":push})).await{Ok(p)=>self.probe.set(Some(p)),Err(e)=>self.c.error.set(e)}
            self.c.busy.set(false);
        });
    }
    fn refresh(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.c.selected.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RepositoryState>(
                "workspace_open",
                serde_json::json!({"repositoryId":state.repository.id}),
            )
            .await
            {
                Ok(s) => {
                    self.c.accept(s);
                    self.probe.set(None);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn cancel(self) {
        if self.pending.with_untracked(|p| p.is_some())
            && self.c.plan.with_untracked(|p| p.is_some())
        {
            self.c.plan.set(None);
            self.c.completion_epoch.set(self.c.epoch.get_untracked());
            self.c.completion.set(Some(false));
        }
        self.form.set(None);
        self.url.set(String::new());
    }
}
fn addresses(remote: &Remote, push: bool) -> Vec<String> {
    let values = if push {
        &remote.push_urls
    } else {
        &remote.fetch_urls
    };
    if values.is_empty() {
        vec![if push {
            remote.push.clone()
        } else {
            remote.fetch.clone()
        }]
    } else {
        values.clone()
    }
}
pub fn panel(c: Controller, en: RwSignal<bool>) -> impl IntoView {
    let v = Remotes {
        c,
        selected: RwSignal::new(String::new()),
        form: RwSignal::new(None),
        name: RwSignal::new(String::new()),
        url: RwSignal::new(String::new()),
        pending: RwSignal::new(None),
        expected: RwSignal::new(0),
        probe: RwSignal::new(None),
    };
    let repository = Memo::new(move |_| {
        c.selected
            .with(|s| s.as_ref().map(|s| s.repository.id.clone()))
    });
    Effect::watch(
        move || repository.get(),
        move |_, _, _| {
            c.remotes.set(false);
            v.selected.set(String::new());
            v.form.set(None);
            v.pending.set(None);
            v.url.set(String::new());
            v.probe.set(None);
        },
        false,
    );
    Effect::watch(
        move || c.comparison.with(|r| r.is_some()) || c.stashes.get(),
        move |other, _, _| {
            if *other {
                c.remotes.set(false);
            }
        },
        false,
    );
    Effect::new(move || {
        if v.pending.with(|p| p.is_some()) && v.expected.get() != c.epoch.get() {
            v.pending.set(None);
            return;
        }
        if v.pending.with(|p| p.is_some())
            && !c.busy.get()
            && v.expected.get() == c.completion_epoch.get()
            && let Some(success) = c.completion.get()
        {
            let kind = v.pending.get_untracked();
            v.pending.set(None);
            if success {
                if kind.as_deref() == Some("add") {
                    v.selected.set(v.name.get_untracked());
                }
                v.form.set(None);
                v.url.set(String::new());
                v.probe.set(None);
            }
        }
    });
    view! {<Show when=move ||c.remotes.get()><section class="git-remotes" aria-label=move ||tr(en.get(),"远程管理","Remote manager")>
    <header><strong>{move ||tr(en.get(),"远程管理","Remote manager")}</strong><button class="quiet" disabled=move ||c.busy.get()||v.form.with(|f|f.is_some()) on:click=move |_|v.begin("add")>{move ||tr(en.get(),"添加远程","Add remote")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.refresh()>{move ||tr(en.get(),"刷新远程","Refresh remotes")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if v.pending.with_untracked(|p|p.is_some())&&c.plan.with_untracked(|p|p.is_some()){c.plan.set(None);c.completion_epoch.set(c.epoch.get_untracked());c.completion.set(Some(false));}c.remotes.set(false);}>{move ||tr(en.get(),"返回仓库","Back to repository")}</button></header>
    <div class="git-remotes-body"><aside><For each=move ||c.selected.with(|s|s.as_ref().map(|s|s.remotes.clone()).unwrap_or_default()) key=|r|(r.name.clone(),r.fetch.clone()) children=move |r|{let name=r.name.clone();let selected=r.name.clone();view!{<button class="git-remote-entry" class:selected=move ||v.chosen().is_some_and(|r|r.name==selected) disabled=move ||c.busy.get()||v.form.with(|f|f.is_some()) on:click=move |_|{v.selected.set(name.clone());v.probe.set(None);} ><strong>{r.name}</strong><span>{r.fetch}</span></button>}}/><Show when=move ||c.selected.with(|s|s.as_ref().is_some_and(|s|s.remotes.is_empty()))><p class="git-remotes-empty">{move ||tr(en.get(),"尚未配置远程。","No remotes configured.")}</p></Show></aside>
    <main><Show when=move ||v.form.with(|f|f.is_some())><section class="git-remote-form" aria-label=move ||tr(en.get(),"编辑远程配置","Edit remote settings")><header><strong>{move ||v.form.with(|f|match f.as_deref(){Some("add")=>tr(en.get(),"添加远程","Add remote"),Some("push")=>tr(en.get(),"替换首个推送地址","Replace first push URL"),_=>tr(en.get(),"替换首个获取地址","Replace first fetch URL")})}</strong><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.cancel()>{move ||tr(en.get(),"取消编辑","Cancel edit")}</button></header><label>{move ||tr(en.get(),"远程名称","Remote name")}<input aria-label=move ||tr(en.get(),"配置远程名称","Configured remote name") disabled=move ||c.busy.get()||c.plan.with(|p|p.is_some())||v.form.with(|f|f.as_deref()!=Some("add")) prop:value=move ||v.name.get() on:input=move |ev|v.name.set(event_target_value(&ev))/></label><label>{move ||tr(en.get(),"新的完整地址","New complete URL")}<input aria-label=move ||tr(en.get(),"新远程地址","New remote URL") autocomplete="off" spellcheck="false" disabled=move ||c.busy.get()||c.plan.with(|p|p.is_some()) prop:value=move ||v.url.get() on:input=move |ev|v.url.set(event_target_value(&ev))/></label><p>{move ||tr(en.get(),"填写完整的新地址；其他地址保留。列表中的 *** 表示已隐藏的凭据或查询参数。","Enter the complete new URL; other URLs are retained. *** hides credentials or query parameters in displayed URLs.")}</p><button class="primary" disabled=move ||c.busy.get()||c.plan.with(|p|p.is_some())||v.name.get().trim().is_empty()||v.url.get().trim().is_empty() on:click=move |_|v.submit()>{move ||tr(en.get(),"检查并保存远程配置","Review and save remote")}</button></section></Show>
    <Show when=move ||v.chosen().is_some()><section class="git-remote-detail"><header><strong>{move ||v.chosen().map(|r|r.name)}</strong><button class="quiet" disabled=move ||c.busy.get()||v.form.with(|f|f.is_some()) on:click=move |_|v.run(true)>{move ||tr(en.get(),"删除远程","Remove remote")}</button></header>
    {[false,true].into_iter().map(move |push|view!{<section class="git-remote-addresses"><header><strong>{move ||if push{tr(en.get(),"推送地址","Push URLs")}else{tr(en.get(),"获取地址","Fetch URLs")}}</strong><button class="quiet" disabled=move ||c.busy.get()||v.form.with(|f|f.is_some()) on:click=move |_|v.begin(if push{"push"}else{"fetch"})>{move ||tr(en.get(),"修改地址","Change URL")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.probe(push)>{move ||tr(en.get(),"检查连接","Check connection")}</button></header><div>{move ||v.chosen().map(|r|addresses(&r,push).into_iter().map(|url|view!{<code>{url}</code>}).collect_view())}</div>{if push{Some(view!{<button class="quiet" disabled=move ||c.busy.get()||v.form.with(|f|f.is_some())||v.chosen().is_none_or(|r|!r.push_local) on:click=move |_|v.run(false)>{move ||tr(en.get(),"移除仓库内独立推送地址","Clear repository push URLs")}</button><small>{move ||tr(en.get(),"移除后使用 Git 配置实际解析出的推送地址；继承的配置保持不变。","Clearing uses the push URLs resolved by Git; inherited configuration remains unchanged.")}</small>})}else{None}}</section>}).collect_view()}
    <div class="git-remote-network"><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if let Some(r)=v.chosen(){c.run(Action::Fetch{remote:Some(r.name),prune:false});}}>{move ||tr(en.get(),"获取此远程","Fetch this remote")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{if let Some(r)=v.chosen(){c.run(Action::Fetch{remote:Some(r.name),prune:true});}}>{move ||tr(en.get(),"获取并清理失效引用","Fetch and prune")}</button></div>
    </section></Show>
    <Show when=move ||v.probe.with(|p|p.is_some())><p class="git-remote-probe" role="status">{move ||v.probe.with(|p|p.as_ref().map(|p|format!("{} · {} · {} {} · {} ms{}",tr(en.get(),"上次连接检查成功","Last connection check succeeded"),if p.push{tr(en.get(),"推送地址","Push URL")}else{tr(en.get(),"获取地址","Fetch URL")},p.references,tr(en.get(),"条引用","references"),p.elapsed_ms,p.default_branch.as_ref().map(|b|format!(" · {b}")).unwrap_or_default())))}</p></Show>
    <p class="git-remotes-note">{move ||tr(en.get(),"连接检查只列出引用，不拉取对象。认证沿用本机 Git、SSH 和凭据配置；配置变更与删除会先确认并保存恢复信息。","Connection checks list references without fetching objects. Authentication uses your Git/SSH credentials. Configuration changes and removal are reviewed and backed up.")}</p>
    </main></div></section></Show>}
}
