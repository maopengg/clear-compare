use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(
    inline_js = "let registryAnchor;export function registryFocus(){registryAnchor=document.activeElement;requestAnimationFrame(()=>document.querySelector('.repository-registration input')?.focus());}export function registryRestore(){if(registryAnchor?.isConnected)registryAnchor.focus();else document.querySelector('.workbench-toolbar button')?.focus();}"
)]
extern "C" {
    #[wasm_bindgen(js_name=registryFocus)]
    fn focus();
    #[wasm_bindgen(js_name=registryRestore)]
    fn restore();
}
#[derive(Clone, Copy)]
pub struct Registration {
    c: Controller,
    opened: RwSignal<bool>,
    previous: RwSignal<Option<Repository>>,
    path: RwSignal<String>,
    plan: RwSignal<Option<RepositoryRegistration>>,
}
pub fn controller(c: Controller) -> Registration {
    Registration {
        c,
        opened: RwSignal::new(false),
        previous: RwSignal::new(None),
        path: RwSignal::new(String::new()),
        plan: RwSignal::new(None),
    }
}
fn describe(en: bool, error: String) -> String {
    let zh = match error.as_str() {
        "Original repository is still available; add the other copy separately" => {
            "原仓库仍可访问，请使用“添加仓库”收录另一个副本。"
        }
        "Repository is already registered" => "这个仓库已收录，请直接在列表中打开。",
        "Close this repository's source tabs before relocating it" => {
            "请先关闭这个仓库的源文件标签，再重新定位。"
        }
        "Close this repository's source tabs before removing it" => {
            "请先关闭这个仓库的源文件标签，再移除收录。"
        }
        "Close destination repository source tabs before registering it" => {
            "请先关闭目标仓库的源文件标签。"
        }
        "Select a different repository location" => "请选择移动后的新仓库位置。",
        "Repository kinds differ; add it separately" => "仓库类型不同，请单独添加。",
        "Repository location changed after review" => "仓库位置在检查后发生变化，请重新检查。",
        "Repository catalogue changed; review again"
        | "Repository registration expired; review again" => "收录信息已变化，请重新检查。",
        "Select an absolute repository root" => "请输入仓库根目录的绝对路径。",
        "Repository registration limit reached" => "收录数量已达上限，请先移除不需要的仓库。",
        _ => return error,
    };
    if en { error } else { zh.into() }
}
impl Registration {
    pub fn open_path(self, path: String) {
        self.open(None);
        self.path.set(path);
    }
    pub fn open(self, previous: Option<Repository>) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.previous.set(previous);
        self.path.set(String::new());
        self.plan.set(None);
        self.opened.set(true);
        self.c.error.set(String::new());
        focus();
    }
    pub fn hide(self) {
        if !self.c.busy.get_untracked() {
            self.opened.set(false);
            self.plan.set(None);
        }
    }
    pub fn close(self) {
        if !self.c.busy.get_untracked() && self.opened.get_untracked() {
            self.hide();
            restore();
        }
    }
    fn prepare(self, en: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        self.plan.set(None);
        let path = self.path.get_untracked();
        let previous = self
            .previous
            .with_untracked(|p| p.as_ref().map(|p| p.id.clone()));
        spawn_local(async move {
            match invoke::<RepositoryRegistration>(
                "workspace_registration_prepare",
                serde_json::json!({"path":path,"previous":previous}),
            )
            .await
            {
                Ok(plan) => self.plan.set(Some(plan)),
                Err(error) => self.c.error.set(describe(en, error)),
            };
            self.c.busy.set(false);
        });
    }
    fn apply(self, en: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(plan) = self.plan.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<Catalog>(
                "workspace_registration_apply",
                serde_json::json!({"planId":plan.id}),
            )
            .await
            {
                Ok(catalog) => {
                    self.c.catalog.set(catalog);
                    self.c.selected.set(None);
                    self.c.plan.set(None);
                    self.opened.set(false);
                    restore();
                }
                Err(error) => {
                    self.plan.set(None);
                    self.c.error.set(describe(en, error));
                }
            };
            self.c.busy.set(false);
        });
    }
    fn remove(self, en: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(previous) = self.previous.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<Catalog>(
                "workspace_remove",
                serde_json::json!({"repositoryId":previous.id}),
            )
            .await
            {
                Ok(catalog) => {
                    self.c.catalog.set(catalog);
                    if self.c.selected.with_untracked(|s| {
                        s.as_ref().is_some_and(|s| s.repository.id == previous.id)
                    }) {
                        self.c.selected.set(None);
                    }
                    self.c.plan.set(None);
                    self.opened.set(false);
                    restore();
                }
                Err(error) => self.c.error.set(describe(en, error)),
            };
            self.c.busy.set(false);
        });
    }
}
pub fn panel(r: Registration, en: RwSignal<bool>) -> impl IntoView {
    view! {<Show when=move ||r.opened.get()><section class="repository-registration" aria-label=move ||tr(en.get(),"仓库收录设置","Repository registration") on:keydown=move |event|{if event.key()=="Escape"{event.prevent_default();r.close();}}>
        <header><strong>{move ||if r.previous.with(Option::is_some){tr(en.get(),"管理仓库收录","Manage repository registration")}else{tr(en.get(),"添加已有 Git 仓库","Add existing Git repository")}}</strong><button class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|r.close()>{move ||tr(en.get(),"关闭","Close")}</button></header>
        <Show when=move ||r.previous.with(Option::is_some)><p><span>{move ||tr(en.get(),"原位置：","Original: ")}</span><code>{move ||r.previous.with(|p|p.as_ref().map(|p|p.path.clone()).unwrap_or_default())}</code></p></Show>
        <form on:submit=move |event|{event.prevent_default();r.prepare(en.get_untracked());}><label>{move ||tr(en.get(),"仓库根目录","Repository root")}<input disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"仓库根目录","Repository root") prop:value=move ||r.path.get() on:input=move |event|{r.path.set(event_target_value(&event));r.plan.set(None);}/></label><button type="button" class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|{r.c.busy.set(true);spawn_local(async move{match invoke::<Option<String>>("pick_directory",serde_json::json!({})).await{Ok(Some(path))=>{r.path.set(path);r.plan.set(None);},Ok(None)=>{},Err(error)=>r.c.error.set(error)};r.c.busy.set(false);});}>{move ||tr(en.get(),"选择目录…","Choose directory…")}</button><button class="quiet" type="submit" disabled=move ||r.c.busy.get()||r.path.get().trim().is_empty()>{move ||tr(en.get(),"检查仓库","Inspect repository")}</button></form>
        <p>{move ||tr(en.get(),"仅收录此仓库，不递归扫描。重新定位用于原位置已失效的仓库；确认后转移收藏、分支偏好和文件规则，磁盘文件保持原样。","Registers this repository without recursive discovery. Relocation transfers favorites, branch preferences and file rules from an unavailable location; disk files stay in place.")}</p>
        <Show when=move ||r.plan.with(Option::is_some)><div class="repository-registration-review" role="region" aria-label=move ||tr(en.get(),"确认仓库收录","Review repository registration")><code>{move ||r.plan.with(|p|p.as_ref().map(|p|p.repository.path.clone()).unwrap_or_default())}</code><span>{move ||r.plan.with(|p|p.as_ref().map(|p|format!("{} · {}",p.repository.branch,p.repository.match_key.as_deref().unwrap_or(tr(en.get(),"无远程地址","No remote")))).unwrap_or_default())}</span><Show when=move ||r.plan.with(|p|p.as_ref().is_some_and(|p|p.previous.as_ref().is_some_and(|old|old.match_key!=p.repository.match_key)))><strong>{move ||tr(en.get(),"远程地址与原记录不同，请核对后再确认。","The remote differs from the original record. Verify it before confirming.")}</strong></Show><Show when=move ||r.plan.with(|p|p.as_ref().is_some_and(|p|p.existing.is_some()))><strong>{move ||tr(en.get(),"新位置已收录：合并为一条记录；原记录的文件规则和分支偏好优先。","Destination is already registered: combine records; original file rules and branch preferences take precedence.")}</strong></Show><button class="primary" disabled=move ||r.c.busy.get() on:click=move |_|r.apply(en.get_untracked())>{move ||if r.previous.with(Option::is_some){tr(en.get(),"确认重新定位","Confirm relocation")}else{tr(en.get(),"确认收录","Confirm registration")}}</button></div></Show>
        <Show when=move ||r.previous.with(Option::is_some)><button class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|r.remove(en.get_untracked())>{move ||tr(en.get(),"移除收录（保留磁盘文件）","Remove registration (keep disk files)")}</button></Show>
    </section></Show>}
}
