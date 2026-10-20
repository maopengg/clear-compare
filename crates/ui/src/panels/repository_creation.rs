use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(
    inline_js = "let creationAnchor;export function creationFocus(){creationAnchor=document.activeElement;requestAnimationFrame(()=>document.querySelector('.repository-creation select')?.focus());}export function creationRestore(){if(creationAnchor?.isConnected)creationAnchor.focus();else document.querySelector('.workbench-toolbar button')?.focus();}"
)]
extern "C" {
    #[wasm_bindgen(js_name=creationFocus)]
    fn focus();
    #[wasm_bindgen(js_name=creationRestore)]
    fn restore();
}
#[derive(Clone, Copy)]
pub struct Creation {
    c: Controller,
    opened: RwSignal<bool>,
    mode: RwSignal<String>,
    parent: RwSignal<String>,
    name: RwSignal<String>,
    url: RwSignal<String>,
    branch: RwSignal<String>,
    depth: RwSignal<String>,
    bare: RwSignal<bool>,
    plan: RwSignal<Option<RepositoryCreationPlan>>,
    result: RwSignal<Option<RepositoryCreationResult>>,
    running: RwSignal<bool>,
    elapsed: RwSignal<u64>,
}
pub fn controller(c: Controller) -> Creation {
    Creation {
        c,
        opened: RwSignal::new(false),
        mode: RwSignal::new("clone".into()),
        parent: RwSignal::new(String::new()),
        name: RwSignal::new(String::new()),
        url: RwSignal::new(String::new()),
        branch: RwSignal::new(String::new()),
        depth: RwSignal::new(String::new()),
        bare: RwSignal::new(false),
        plan: RwSignal::new(None),
        result: RwSignal::new(None),
        running: RwSignal::new(false),
        elapsed: RwSignal::new(0),
    }
}
fn describe(en: bool, error: String) -> String {
    let zh = match error.as_str() {
        "Use a single portable repository folder name" => {
            "请输入单个跨平台目录名，不使用路径分隔符或系统保留名称。"
        }
        "Select an absolute repository parent directory" => "请选择已有父目录的绝对路径。",
        "Repository destination already exists" => {
            "目标位置已存在，请更换目录名；不会覆盖已有目录。"
        }
        "Repository parent changed after review" => "父目录在检查后发生变化，请重新检查。",
        "Local clone source changed after review" => "本地来源仓库在检查后发生变化，请重新检查。",
        "Repository staging directory changed" => "临时目录身份发生变化，已停止发布目标目录。",
        "Repository creation expired; review again" => "创建计划已失效，请重新检查。",
        "Repositories cannot be created inside Git metadata" => "不能在 Git 管理目录内部创建仓库。",
        "Use a Git URL without embedded credentials, query or fragment" => {
            "请使用不含内嵌密码、查询参数或片段的 Git 地址；认证使用本机凭据。"
        }
        "Unsupported clone URL protocol" => {
            "克隆支持 HTTPS、HTTP、SSH、Git 地址或绝对本地仓库路径。"
        }
        "Use an absolute local source path or Git URL" => {
            "请输入 Git 地址或本地来源仓库的绝对路径。"
        }
        "Invalid initial branch or clone reference" => "分支或标签名称无效。",
        "Clone depth must be 1–1000000" => "浅克隆深度必须为 1–1000000 的整数。",
        "Repository registration limit reached" => "收录数量已达上限，请先移除不需要的仓库。",
        "Operation cancelled" => "操作已取消。",
        _ => return error,
    };
    if en { error } else { zh.into() }
}
impl Creation {
    pub fn open(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.opened.set(true);
        self.plan.set(None);
        self.result.set(None);
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
    fn changed(self) {
        self.plan.set(None);
    }
    fn prepare(self, en: bool) {
        if self.c.busy.get_untracked() {
            return;
        }
        let operation = if self.mode.get_untracked() == "init" {
            RepositoryCreation::Init {
                branch: self.branch.get_untracked(),
                bare: self.bare.get_untracked(),
            }
        } else {
            let value = self.depth.get_untracked();
            let depth = if value.trim().is_empty() {
                None
            } else {
                match value.parse::<u32>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        self.c
                            .error
                            .set(describe(en, "Clone depth must be 1–1000000".into()));
                        return;
                    }
                }
            };
            RepositoryCreation::Clone {
                url: self.url.get_untracked(),
                branch: (!self.branch.get_untracked().is_empty())
                    .then(|| self.branch.get_untracked()),
                bare: self.bare.get_untracked(),
                depth,
            }
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        self.plan.set(None);
        spawn_local(async move {
            match invoke::<RepositoryCreationPlan>("workspace_creation_prepare",serde_json::json!({"parent":self.parent.get_untracked(),"name":self.name.get_untracked(),"operation":operation})).await{Ok(plan)=>self.plan.set(Some(plan)),Err(error)=>self.c.error.set(describe(en,error))};
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
        self.running.set(true);
        self.c.error.set(String::new());
        self.plan.set(None);
        let start = js_sys::Date::now();
        spawn_local(async move {
            match invoke::<RepositoryCreationResult>(
                "workspace_creation_apply",
                serde_json::json!({"planId":plan.id}),
            )
            .await
            {
                Ok(result) => {
                    if let Some(catalog) = result.catalog.clone() {
                        self.c.catalog.set(catalog);
                    }
                    self.result.set(Some(result));
                }
                Err(error) => self.c.error.set(describe(en, error)),
            };
            self.elapsed.set((js_sys::Date::now() - start) as u64);
            self.running.set(false);
            self.c.busy.set(false);
        });
    }
}
pub fn panel(
    r: Creation,
    en: RwSignal<bool>,
    register: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    view! {<Show when=move ||r.opened.get()><section class="repository-creation repository-registration" aria-label=move ||tr(en.get(),"克隆或初始化仓库","Clone or initialize repository") on:keydown=move |event|{if event.key()=="Escape"{event.prevent_default();r.close();}}>
    <header><strong>{move ||tr(en.get(),"克隆 / 初始化 Git 仓库","Clone / initialize Git repository")}</strong><button class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|r.close()>{move ||tr(en.get(),"关闭","Close")}</button></header>
    <details class="repository-creation-options" prop:open=move ||r.result.with(Option::is_none)><summary>{move ||tr(en.get(),"创建参数","Creation options")}</summary><form on:submit=move |event|{event.prevent_default();r.prepare(en.get_untracked());}>
    <select disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"创建方式","Creation method") prop:value=move ||r.mode.get() on:change=move |event|{let mode=event_target_value(&event);r.branch.set(if mode=="init"{"main".into()}else{String::new()});r.mode.set(mode);r.changed();}><option value="clone">{move ||tr(en.get(),"克隆仓库","Clone repository")}</option><option value="init">{move ||tr(en.get(),"初始化仓库","Initialize repository")}</option></select>
    <label>{move ||tr(en.get(),"父目录","Parent directory")}<input disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"新仓库父目录","New repository parent") prop:value=move ||r.parent.get() on:input=move |event|{r.parent.set(event_target_value(&event));r.changed();}/></label><button type="button" class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|{r.c.busy.set(true);spawn_local(async move{match invoke::<Option<String>>("pick_directory",serde_json::json!({})).await{Ok(Some(parent))=>{r.parent.set(parent);r.changed();},Ok(None)=>{},Err(error)=>r.c.error.set(error)}r.c.busy.set(false);});}>{move ||tr(en.get(),"选择父目录…","Choose parent…")}</button>
    <label>{move ||tr(en.get(),"目录名","Folder name")}<input disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"新仓库目录名","New repository folder name") prop:value=move ||r.name.get() on:input=move |event|{r.name.set(event_target_value(&event));r.changed();}/></label>
    <Show when=move ||r.mode.get()=="clone"><label>{move ||tr(en.get(),"Git 地址 / 本地来源","Git URL / local source")}<input disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"克隆来源","Clone source") prop:value=move ||r.url.get() on:input=move |event|{r.url.set(event_target_value(&event));r.changed();}/></label></Show>
    <label>{move ||if r.mode.get()=="init"{tr(en.get(),"初始分支","Initial branch")}else{tr(en.get(),"分支 / 标签（可选）","Branch / tag (optional)")}}<input disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"创建分支或标签","Creation branch or tag") prop:value=move ||r.branch.get() on:input=move |event|{r.branch.set(event_target_value(&event));r.changed();}/></label>
    <Show when=move ||r.mode.get()=="clone"><label>{move ||tr(en.get(),"浅克隆深度（留空为完整）","Shallow depth (empty for full)")}<input type="number" min="1" max="1000000" disabled=move ||r.c.busy.get() aria-label=move ||tr(en.get(),"浅克隆深度","Shallow clone depth") prop:value=move ||r.depth.get() on:input=move |event|{r.depth.set(event_target_value(&event));r.changed();}/></label></Show>
    <label class="repository-creation-bare"><input type="checkbox" disabled=move ||r.c.busy.get() prop:checked=move ||r.bare.get() on:change=move |event|{r.bare.set(event_target_checked(&event));r.changed();}/>{move ||tr(en.get(),"裸仓库（无工作区）","Bare repository (no working tree)")}</label><button class="quiet" type="submit" disabled=move ||r.c.busy.get()||r.parent.get().is_empty()||r.name.get().is_empty()>{move ||tr(en.get(),"检查创建计划","Inspect creation plan")}</button></form></details>
    <p>{move ||tr(en.get(),"目标必须不存在。先在独立临时目录执行，再发布并收录；失败或取消保留文件和日志。认证使用本机 Git/SSH/凭据配置。","Destination must not exist. Runs in a separate staging directory, then publishes and registers. Failures or cancellation retain files and logs. Uses local Git/SSH/credential configuration.")}</p>
    <Show when=move ||r.plan.with(Option::is_some)><div class="repository-registration-review" role="region" aria-label=move ||tr(en.get(),"确认创建仓库","Review repository creation")><code>{move ||r.plan.with(|p|p.as_ref().map(|p|p.destination.clone()).unwrap_or_default())}</code><span>{move ||r.plan.with(|p|p.as_ref().map(|p|match &p.operation{RepositoryCreation::Init{branch,bare}=>format!("{} · {branch}{}",tr(en.get(),"初始化","Initialize"),if *bare{tr(en.get()," · 裸仓库"," · Bare")}else{""}),RepositoryCreation::Clone{url,branch,bare,depth}=>format!("{} · {}{}{}",url,branch.as_deref().unwrap_or(tr(en.get(),"默认分支","Default branch")),if *bare{tr(en.get()," · 裸仓库"," · Bare")}else{""},depth.map(|n|format!(" · {} {n}",tr(en.get(),"深度","Depth"))).unwrap_or_default())}).unwrap_or_default())}</span><button class="primary" disabled=move ||r.c.busy.get() on:click=move |_|r.apply(en.get_untracked())>{move ||tr(en.get(),"确认执行创建","Confirm repository creation")}</button></div></Show>
    <Show when=move ||r.result.with(Option::is_some)><div class="repository-creation-result" role="status"><strong>{move ||r.result.with(|v|v.as_ref().map(|v|match v.status.as_str(){"registered"=>tr(en.get(),"仓库已创建并收录","Repository created and registered"),"created_unregistered"=>tr(en.get(),"仓库已创建，收录未完成","Repository created; registration incomplete"),"cancelled"=>tr(en.get(),"已取消，保留临时文件","Cancelled; staging files retained"),_=>tr(en.get(),"创建未完成，保留临时文件","Creation incomplete; staging files retained")}).unwrap_or_default())}</strong><span>{move ||format!("{} ms",r.elapsed.get())}</span><code>{move ||r.result.with(|v|v.as_ref().map(|v|v.path.clone()).unwrap_or_default())}</code><p>{move ||r.result.with(|v|v.as_ref().and_then(|v|v.issue.clone()).map(|e|describe(en.get(),e)).unwrap_or_default())}</p><Show when=move ||r.result.with(|v|v.as_ref().is_some_and(|v|v.status=="created_unregistered"))><button class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|{let path=r.result.with_untracked(|v|v.as_ref().map(|v|v.path.clone()).unwrap_or_default());r.close();register(path);}>{move ||tr(en.get(),"收录已创建的仓库","Register the created repository")}</button></Show><button class="quiet" disabled=move ||r.c.busy.get() on:click=move |_|{r.result.set(None);r.plan.set(None);}>{move ||tr(en.get(),"再次创建","Create another")}</button><details><summary>{move ||tr(en.get(),"执行日志","Execution log")}</summary><code>{move ||r.result.with(|v|v.as_ref().map(|v|v.journal.clone()).unwrap_or_default())}</code><pre>{move ||r.result.with(|v|v.as_ref().map(|v|v.output.clone()).unwrap_or_default())}</pre></details></div></Show>
    <Show when=move ||r.running.get()><footer role="status"><span>{move ||tr(en.get(),"正在创建仓库，现有内容保持可见…","Creating repository; existing contents remain visible…")}</span><button class="quiet" on:click=move |_|spawn_local(async{let _=invoke::<()>("cancel",serde_json::json!({})).await;})>{move ||tr(en.get(),"取消创建","Cancel creation")}</button></footer></Show>
    </section></Show>}
}
