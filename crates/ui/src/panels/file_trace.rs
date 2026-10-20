//! Shared committed-file history, attribution and captured-version reader.
use super::git_actions::Controller;
use crate::{invoke, tr};
use clear_contracts::workbench::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;
#[wasm_bindgen(inline_js = "
let traceAnchor;
export function traceRemember(){traceAnchor=document.activeElement;}
export function traceFocus(){requestAnimationFrame(()=>document.querySelector('.file-trace>header button:not(:disabled)')?.focus());}
export function traceRestore(){requestAnimationFrame(()=>{if(traceAnchor?.isConnected)traceAnchor.focus();});}
export function traceTime(raw,en){const date=new Date(Number(raw)*1000);return Number.isNaN(date.valueOf())?raw:date.toLocaleString(en?'en-US':'zh-CN',{timeZone:'UTC',year:'numeric',month:'2-digit',day:'2-digit'})+' UTC';}
")]
extern "C" {
    #[wasm_bindgen(js_name=traceRemember)]
    fn remember();
    #[wasm_bindgen(js_name=traceFocus)]
    fn focus();
    #[wasm_bindgen(js_name=traceRestore)]
    fn restore();
    #[wasm_bindgen(js_name=traceTime)]
    fn time(raw: &str, en: bool) -> String;
}
pub(super) fn open(request: RwSignal<Option<FileHistorySource>>, source: FileHistorySource) {
    remember();
    request.set(Some(source));
}
#[derive(Clone, Copy)]
struct Trace {
    c: Controller,
    request: RwSignal<Option<FileHistorySource>>,
    detail_request: RwSignal<Option<(String, String)>>,
    files: RwSignal<bool>,
    history: RwSignal<bool>,
    english: RwSignal<bool>,
    page: RwSignal<Option<FileHistoryPage>>,
    target: RwSignal<TraceTarget>,
    target_label: RwSignal<String>,
    target_commit: RwSignal<String>,
    blocked: RwSignal<bool>,
    blame: RwSignal<Option<FileBlamePage>>,
    blame_target: RwSignal<TraceTarget>,
    version: RwSignal<Option<RevisionFocus>>,
    content: RwSignal<Option<RevisionContent>>,
    mode: RwSignal<String>,
}
fn target_label(v: Trace) -> String {
    if v.target
        .with(|target| matches!(target, TraceTarget::Anchor))
    {
        tr(v.english.get(), "打开时的提交版本", "Captured revision").into()
    } else {
        v.target_label.get()
    }
}
fn describe(en: bool, error: String) -> String {
    if en {
        return error;
    }
    for (native, zh) in [
        (
            "File history graph changed",
            "历史边界已改变，请重新打开文件历史。",
        ),
        (
            "File history graph metadata",
            "Git 历史边界元数据不可用或超过限制，请核对仓库。",
        ),
        (
            "Historical common repository mapping changed",
            "仓库共享目录映射已变化，请重新扫描。",
        ),
        ("Operation cancelled", "已取消读取；原内容保留。"),
        ("File history expired", "文件历史已失效，请重新打开。"),
        (
            "Blame line selection expired",
            "该行的追溯信息已失效，请重新读取这一页。",
        ),
        (
            "Unknown file history commit",
            "文件历史选择已失效，请重新选择提交。",
        ),
        ("File has no committed version", "文件尚无提交版本。"),
        (
            "File is absent from this historical commit",
            "该文件在所选提交中不存在。",
        ),
        (
            "File is deleted at this commit",
            "该提交中此文件已删除，请选择更早的版本。",
        ),
        (
            "Line blame requires UTF-8 text",
            "逐行追溯需要 UTF-8 文本；其他格式仍可查看文件历史和版本。",
        ),
        (
            "Line blame requires a regular committed text file",
            "逐行追溯需要已提交的普通文本文件。",
        ),
        (
            "Line blame exceeds 4 MiB",
            "逐行追溯超过 4 MiB 限制；可查看文件历史和分页版本。",
        ),
        (
            "Line blame exceeds 100000 physical lines",
            "逐行追溯超过 100000 个 Git 物理行限制。",
        ),
        (
            "File history currently requires Unicode paths",
            "文件历史目前需要 Unicode 路径。",
        ),
        (
            "Blame contents differ",
            "追溯内容与固定的 Git 对象不一致，请重新读取。",
        ),
        (
            "Blame response is incomplete",
            "Git 追溯响应不完整，请重新读取。",
        ),
        (
            "Historical repository identity changed",
            "仓库身份已变化，请重新扫描。",
        ),
        (
            "Historical repository mapping changed",
            "仓库映射已变化，请重新扫描。",
        ),
        ("Historical tree expired", "历史版本已失效，请重新打开。"),
    ] {
        if error.contains(native) {
            return zh.into();
        }
    }
    error
}
impl Trace {
    fn close(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        let page = self.page.get_untracked();
        self.c.busy.set(true);
        spawn_local(async move {
            let result = if let Some(page) = page {
                invoke::<()>(
                    "workspace_file_history_close",
                    serde_json::json!({"historyId":page.id}),
                )
                .await
            } else {
                Ok(())
            };
            match result {
                Ok(()) => {
                    self.request.set(None);
                    restore();
                }
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
        });
    }
    fn fail(self, error: String) {
        self.c
            .error
            .set(describe(self.english.get_untracked(), error));
    }
    fn load(self, source: FileHistorySource) {
        if self.c.busy.get_untracked() {
            return;
        }
        self.c.busy.set(true);
        self.c.error.set(String::new());
        focus();
        spawn_local(async move {
            match invoke::<FileHistoryPage>(
                "workspace_file_history_open",
                serde_json::json!({"source":source}),
            )
            .await
            {
                Ok(page) => {
                    if let Some(old) = self.page.get_untracked()
                        && old.id != page.id
                    {
                        let _ = invoke::<()>(
                            "workspace_file_history_close",
                            serde_json::json!({"historyId":old.id}),
                        )
                        .await;
                    }
                    self.target_commit.set(page.anchor.clone());
                    self.blocked.set(false);
                    self.page.set(Some(page));
                    self.target.set(TraceTarget::Anchor);
                    self.target_label.set(
                        tr(
                            self.english.get_untracked(),
                            "打开时的提交版本",
                            "Captured revision",
                        )
                        .into(),
                    );
                    self.blame.set(None);
                    self.version.set(None);
                    self.content.set(None);
                    self.mode.set("history".into());
                }
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
            focus();
        });
    }
    fn page(self, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(page) = self.page.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<FileHistoryPage>(
                "workspace_file_history_page",
                serde_json::json!({"historyId":page.id,"offset":offset}),
            )
            .await
            {
                Ok(page) => self.page.set(Some(page)),
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
        });
    }
    fn blame(self, target: TraceTarget, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(page) = self.page.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<FileBlamePage>(
                "workspace_file_blame",
                serde_json::json!({"historyId":page.id,"target":target,"offset":offset}),
            )
            .await
            {
                Ok(blame) => {
                    self.blame_target.set(target);
                    self.blame.set(Some(blame));
                    self.mode.set("blame".into());
                }
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
        });
    }
    fn version(self, target: TraceTarget) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(page) = self.page.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            let result=async{
                let version=invoke::<RevisionFocus>("workspace_file_history_version",serde_json::json!({"historyId":page.id,"target":target})).await?;
                let mut content=invoke::<RevisionContent>("workspace_revision_content",serde_json::json!({"repositoryId":version.tree.repository_id,"session":version.tree.id,"fileId":version.entry.id,"mode":"auto","offset":0})).await?;
                if content.mode=="text" && version.line>500 {
                    let offset=version.line.saturating_sub(1)/500*500;
                    content=invoke::<RevisionContent>("workspace_revision_content",serde_json::json!({"repositoryId":version.tree.repository_id,"session":version.tree.id,"fileId":version.entry.id,"mode":"text","offset":offset})).await?;
                }
                Ok::<_,String>((version,content))
            }.await;
            match result {
                Ok((version, content)) => {
                    self.version.set(Some(version));
                    self.content.set(Some(content));
                    self.mode.set("version".into());
                }
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
        });
    }
    fn content(self, mode: String, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(version) = self.version.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RevisionContent>("workspace_revision_content",serde_json::json!({"repositoryId":version.tree.repository_id,"session":version.tree.id,"fileId":version.entry.id,"mode":mode,"offset":offset})).await{Ok(content)=>self.content.set(Some(content)),Err(error)=>self.fail(error)}
            self.c.busy.set(false);
        });
    }
    fn details(self, commit: String) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(page) = self.page.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            let result = async {
                let state = invoke::<RepositoryState>(
                    "workspace_open",
                    serde_json::json!({"repositoryId":page.repository_id}),
                )
                .await?;
                invoke::<()>(
                    "workspace_file_history_close",
                    serde_json::json!({"historyId":page.id}),
                )
                .await?;
                Ok::<_, String>(state)
            }
            .await;
            match result {
                Ok(state) => {
                    self.c.accept(state);
                    self.files.set(false);
                    self.history.set(true);
                    self.request.set(None);
                    self.detail_request.set(Some((page.repository_id, commit)));
                }
                Err(error) => self.fail(error),
            }
            self.c.busy.set(false);
        });
    }
}
fn history_entries(v: Trace) -> impl IntoView {
    view! {<div class="file-trace-commits"><For each=move ||v.page.with(|page|page.as_ref().map(|page|page.entries.clone()).unwrap_or_default()) key=|entry|entry.id.clone() children=move |entry|{
        let entry=StoredValue::new(entry);
        view!{<button class="file-trace-commit" disabled=move ||v.c.busy.get() title=entry.with_value(|entry|format!("{} · {} · {}",entry.commit.id,entry.commit.date,entry.path)) class:selected=move ||entry.with_value(|entry|v.target.with(|target|matches!(target,TraceTarget::Commit{id} if *id==entry.id))) on:click=move |_|entry.with_value(|entry|{v.target.set(TraceTarget::Commit{id:entry.id.clone()});v.target_commit.set(entry.commit.id.clone());v.blocked.set(entry.deleted);v.target_label.set(format!("{} · {}",entry.commit.id.chars().take(8).collect::<String>(),entry.path));})>
          <strong>{entry.with_value(|entry|entry.commit.subject.clone())}</strong><small>{entry.with_value(|entry|format!("{} · {}",entry.commit.id.chars().take(8).collect::<String>(),entry.commit.author))}</small><span>{entry.with_value(|entry|entry.path.clone())}{move ||entry.with_value(|entry|entry.deleted.then_some(tr(v.english.get(),"（已删除）"," (deleted)")))}</span><Show when=move ||entry.with_value(|entry|entry.previous_path.is_some())><span>{entry.with_value(|entry|format!("{} → {}",entry.previous_path.as_deref().unwrap_or_default(),entry.path))}</span></Show>
        </button>}
    }/></div>}
}
fn blame_view(v: Trace) -> impl IntoView {
    view! {<div class="file-trace-blame" tabindex="0" aria-label=move ||tr(v.english.get(),"逐行追溯结果","Blame results")><table><thead><tr><th>{move ||tr(v.english.get(),"提交","Commit")}</th><th>{move ||tr(v.english.get(),"作者","Author")}</th><th class="trace-date">{move ||tr(v.english.get(),"日期","Date")}</th><th>{move ||tr(v.english.get(),"行","Line")}</th><th>{move ||tr(v.english.get(),"内容","Contents")}</th></tr></thead><tbody><For each=move ||v.blame.with(|page|page.as_ref().map(|page|page.lines.clone()).unwrap_or_default()) key=|line|line.id.clone() children=move |line|{
        let line=StoredValue::new(line);let commit=line;
        view!{<tr class:selected=move ||line.with_value(|line|v.target.with(|target|matches!(target,TraceTarget::Line{id} if *id==line.id))) on:click=move |_|line.with_value(|line|{v.target.set(TraceTarget::Line{id:line.id.clone()});v.target_commit.set(line.commit.clone());v.blocked.set(false);v.target_label.set(format!("{} · {}:{}",line.commit.chars().take(8).collect::<String>(),line.original_path,line.original_line));}) title=line.with_value(|line|format!("{} · {} {} · {} · {}:{}",line.subject,line.author,line.email,line.timezone,line.original_path,line.original_line))>
          <td><button class="quiet" aria-label=move ||line.with_value(|line|format!("{} {}",tr(v.english.get(),"查看该行提交","Show line commit"),line.line)) disabled=move ||v.c.busy.get() on:click=move |event|{event.stop_propagation();commit.with_value(|line|v.details(line.commit.clone()));}>{line.with_value(|line|line.commit.chars().take(8).collect::<String>())}</button>{move ||line.with_value(|line|line.boundary.then_some(view!{<span title=tr(v.english.get(),"追溯边界：根提交或浅克隆边界","Attribution boundary: root commit or shallow history")>"*"</span>}))}</td>
          <td>{line.with_value(|line|line.author.clone())}</td><td class="trace-date">{move ||line.with_value(|line|time(&line.timestamp,v.english.get()))}</td><td><button class="quiet" aria-label=move ||line.with_value(|line|format!("{} {}",tr(v.english.get(),"选择追溯行","Select blame line"),line.line)) on:click=move |_|line.with_value(|line|{v.target.set(TraceTarget::Line{id:line.id.clone()});v.target_commit.set(line.commit.clone());v.blocked.set(false);v.target_label.set(format!("{} · {}:{}",line.commit.chars().take(8).collect::<String>(),line.original_path,line.original_line));})>{line.with_value(|line|line.line)}</button></td><td><code>{line.with_value(|line|line.text.clone())}</code>{move ||line.with_value(|line|line.long_line.then_some(view!{<span class="file-trace-long">{tr(v.english.get(),"（前 8192 字符）"," (first 8192 characters)")}</span>}))}</td>
        </tr>}
    }/></tbody></table></div>}
}
pub fn panel(
    c: Controller,
    request: RwSignal<Option<FileHistorySource>>,
    detail_request: RwSignal<Option<(String, String)>>,
    files: RwSignal<bool>,
    history: RwSignal<bool>,
    english: RwSignal<bool>,
) -> impl IntoView {
    let v = Trace {
        c,
        request,
        detail_request,
        files,
        history,
        english,
        page: RwSignal::new(None),
        target: RwSignal::new(TraceTarget::Anchor),
        target_label: RwSignal::new(String::new()),
        target_commit: RwSignal::new(String::new()),
        blocked: RwSignal::new(false),
        blame: RwSignal::new(None),
        blame_target: RwSignal::new(TraceTarget::Anchor),
        version: RwSignal::new(None),
        content: RwSignal::new(None),
        mode: RwSignal::new("history".into()),
    };
    Effect::new(move |_| {
        if let Some(source) = request.get() {
            v.load(source);
        }
    });
    let actionable = move || {
        v.page
            .with(|page| page.as_ref().is_some_and(|page| page.status == "tracked"))
            && !v.blocked.get()
    };
    view! {<Show when=move ||request.with(Option::is_some)><section class="file-trace" role="region" aria-label=move ||tr(english.get(),"文件历史与追溯","File history and blame")>
      <header><strong>{move ||tr(english.get(),"文件历史 / 逐行追溯","File history / blame")}</strong><span title=move ||v.page.with(|page|page.as_ref().map(|page|page.path.clone()).unwrap_or_default())>{move ||v.page.with(|page|page.as_ref().map(|page|page.path.clone()).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.close()>{move ||tr(english.get(),"返回原视图","Back to previous view")}</button></header>
      <div class="file-trace-capture"><code title=move ||v.page.with(|page|page.as_ref().map(|page|page.anchor.clone()).unwrap_or_default())>{move ||v.page.with(|page|page.as_ref().map(|page|page.anchor.chars().take(12).collect::<String>()).unwrap_or_default())}</code><span>{move ||tr(english.get(),"固定已提交版本；不含磁盘和未保存草稿。Git 行号按 LF 分隔。","Pinned committed version; disk changes and unsaved buffers excluded. Git lines use LF.")}</span><Show when=move ||v.page.with(|page|page.as_ref().is_some_and(|page|page.shallow||page.truncated))><strong>{move ||v.page.with(|page|page.as_ref().map(|page|format!("{} {}",if page.shallow{tr(english.get(),"浅克隆，历史可能不完整。","Shallow repository; history may be incomplete.")}else{""},if page.truncated{tr(english.get(),"仅前 5000 个提交。","First 5000 commits only.")}else{""})).unwrap_or_default())}</strong></Show></div>
      <div class="file-trace-actions"><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|{v.target.set(TraceTarget::Anchor);v.target_commit.set(v.page.with_untracked(|page|page.as_ref().map(|page|page.anchor.clone()).unwrap_or_default()));v.blocked.set(false);v.target_label.set(tr(english.get_untracked(),"打开时的提交版本","Captured revision").into());}>{move ||tr(english.get(),"打开时版本","Captured revision")}</button><span title=move ||target_label(v)>{move ||target_label(v)}</span><button class="quiet" disabled=move ||c.busy.get()||!actionable() on:click=move |_|v.blame(v.target.get_untracked(),0)>{move ||tr(english.get(),"追溯所选版本","Blame selected version")}</button><button class="quiet" disabled=move ||c.busy.get()||!actionable() on:click=move |_|v.version(v.target.get_untracked())>{move ||tr(english.get(),"查看所选版本","View selected version")}</button><button class="quiet" disabled=move ||c.busy.get()||v.target_commit.with(String::is_empty) on:click=move |_|{let commit=v.target_commit.get_untracked();if !commit.is_empty(){v.details(commit);}}>{move ||tr(english.get(),"查看提交详情","Show commit details")}</button></div>
      <Show when=move ||v.page.with(|page|page.as_ref().is_some_and(|page|page.status!="tracked"))><p class="file-trace-notice" role="status">{move ||v.page.with(|page|match page.as_ref().map(|page|page.status.as_str()){Some("unborn")=>tr(english.get(),"仓库尚无提交；保留当前草稿后可继续编辑。","Repository has no commits; continue editing your existing buffer."),_=>tr(english.get(),"文件不在打开时的提交中；未跟踪文件没有该版本的追溯信息。","File is not in the captured commit; untracked files have no attribution at this revision.")})}</p></Show>
      <div class="file-trace-body"><aside>{history_entries(v)}<footer><span>{move ||v.page.with(|page|page.as_ref().map(|page|format!("{}–{} / {}",page.offset+usize::from(page.total>0),(page.offset+100).min(page.total),page.total)).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||v.page.with(|page|page.as_ref().is_none_or(|page|page.offset==0)) on:click=move |_|if let Some(page)=v.page.get_untracked(){v.page(page.offset.saturating_sub(100));}>"‹"</button><button class="quiet" disabled=move ||c.busy.get()||v.page.with(|page|page.as_ref().is_none_or(|page|page.offset+100>=page.total)) on:click=move |_|if let Some(page)=v.page.get_untracked(){v.page(page.offset+100);}>"›"</button></footer></aside>
        <main><nav><button class="quiet" disabled=move ||v.blame.with(Option::is_none) aria-pressed=move ||v.mode.get()=="blame" on:click=move |_|v.mode.set("blame".into())>{move ||tr(english.get(),"逐行追溯","Blame")}</button><button class="quiet" disabled=move ||v.content.with(Option::is_none) aria-pressed=move ||v.mode.get()=="version" on:click=move |_|v.mode.set("version".into())>{move ||tr(english.get(),"历史正文","Historical contents")}</button><code>{move ||if v.mode.get()=="blame"{v.blame.with(|page|page.as_ref().map(|page|format!("{} · {}",page.commit.chars().take(12).collect::<String>(),page.path)).unwrap_or_default())}else{v.version.with(|version|version.as_ref().map(|version|format!("{} · {}",version.tree.commit.chars().take(12).collect::<String>(),version.entry.path)).unwrap_or_default())}}</code></nav>
          <Show when=move ||v.mode.get()=="blame"><Show when=move ||v.blame.with(|page|page.as_ref().is_some_and(|page|page.total==0))><p class="file-trace-notice">{move ||tr(english.get(),"空文件没有可追溯的行。","Empty file has no lines to attribute.")}</p></Show>{blame_view(v)}<footer><span>{move ||v.blame.with(|page|page.as_ref().map(|page|format!("{}–{} / {}",page.offset+usize::from(page.total>0),(page.offset+200).min(page.total),page.total)).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||v.blame.with(|page|page.as_ref().is_none_or(|page|page.offset==0)) on:click=move |_|if let Some(page)=v.blame.get_untracked(){v.blame(v.blame_target.get_untracked(),page.offset.saturating_sub(200));}>"‹"</button><button class="quiet" disabled=move ||c.busy.get()||v.blame.with(|page|page.as_ref().is_none_or(|page|page.next_offset.is_none())) on:click=move |_|if let Some(offset)=v.blame.with_untracked(|page|page.as_ref().and_then(|page|page.next_offset)){v.blame(v.blame_target.get_untracked(),offset);}>"›"</button></footer></Show>
          <Show when=move ||v.mode.get()=="version"><div class="file-trace-version-controls"><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.content("auto".into(),0)>{move ||tr(english.get(),"原文","Text")}</button><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.content("hex".into(),0)>{move ||tr(english.get(),"十六进制","Hexadecimal")}</button><span>{move ||v.content.with(|content|content.as_ref().is_some_and(|content|content.long_lines)).then_some(tr(english.get(),"超长行仅前 8192 字符","Long lines show first 8192 characters"))}</span></div>
            <div class="file-trace-version" tabindex="0" aria-label=move ||tr(english.get(),"固定版本正文","Pinned version contents")>{move ||v.content.get().map(|content|if content.mode=="hex"{view!{<pre>{content.text}</pre>}.into_any()}else{view!{<div>{content.text.split('\n').enumerate().map(|(i,line)|{let number=content.offset+i+1;view!{<div class="git-revision-line" class:trace-line-focus=move ||v.version.with(|version|version.as_ref().is_some_and(|version|version.line==number))><span>{number}</span><code>{line.to_owned()}</code></div>}}).collect_view()}</div>}.into_any()})}</div>
            <footer><span>{move ||v.content.with(|content|content.as_ref().map(|content|format!("{}–{} / {} {}",content.offset+usize::from(content.total>0),(content.offset+if content.mode=="hex"{4096}else{500}).min(content.total),content.total,if content.mode=="hex"{tr(english.get(),"字节","bytes")}else{tr(english.get(),"行","lines")})).unwrap_or_default())}</span><button class="quiet" disabled=move ||c.busy.get()||v.content.with(|content|content.as_ref().is_none_or(|content|content.offset==0)) on:click=move |_|if let Some(content)=v.content.get_untracked(){let step=if content.mode=="hex"{4096}else{500};v.content(content.mode,content.offset.saturating_sub(step));}>"‹"</button><button class="quiet" disabled=move ||c.busy.get()||v.content.with(|content|content.as_ref().is_none_or(|content|content.next_offset.is_none())) on:click=move |_|if let Some(content)=v.content.get_untracked()&&let Some(offset)=content.next_offset{v.content(content.mode,offset);}>"›"</button></footer>
          </Show>
          <Show when=move ||v.mode.get()=="history"><p class="file-trace-empty">{move ||tr(english.get(),"左侧选择提交，然后查看历史正文或逐行追溯。追溯行可定位到它的原始提交与旧文件名。","Select a commit on the left, then view its contents or blame. Each attributed line links to its original commit and filename.")}</p></Show>
        </main></div><Show when=move ||c.busy.get()><footer class="file-trace-progress" role="status"><span>{move ||tr(english.get(),"正在读取文件历史…","Reading file history…")}</span><button class="quiet" on:click=move |_|spawn_local(async{let _=invoke::<()>("cancel",serde_json::json!({})).await;})>{move ||tr(english.get(),"取消读取","Cancel read")}</button></footer></Show>
    </section></Show>}
}
