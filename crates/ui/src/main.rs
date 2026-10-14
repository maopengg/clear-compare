use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(
    inline_js = "export function desktopInvoke(command,args){ if(!window.__TAURI__) return Promise.reject('Please open the installed Clear Compare desktop app.'); return window.__TAURI__.core.invoke(command,args); }"
)]
extern "C" {
    #[wasm_bindgen(js_name = desktopInvoke)]
    fn desktop_invoke(command: &str, args: JsValue) -> js_sys::Promise;
}

async fn invoke<T: DeserializeOwned>(command: &str, args: impl Serialize) -> Result<T, String> {
    let value = serde_wasm_bindgen::to_value(&args).map_err(|e| e.to_string())?;
    let result = wasm_bindgen_futures::JsFuture::from(desktop_invoke(command, value))
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    serde_wasm_bindgen::from_value(result).map_err(|e| e.to_string())
}

fn tr(english: bool, zh: &'static str, en: &'static str) -> &'static str {
    if english { en } else { zh }
}
fn label(status: &Status, en: bool) -> &'static str {
    match status {
        Status::Same => tr(en, "一致", "Same"),
        Status::Changed => tr(en, "已修改", "Changed"),
        Status::LeftOnly => tr(en, "仅左侧", "Left only"),
        Status::RightOnly => tr(en, "仅右侧", "Right only"),
        Status::MetadataMatch => tr(en, "未验证内容", "Unverified"),
        Status::TypeConflict => tr(en, "类型冲突", "Type conflict"),
        Status::Error => tr(en, "读取失败", "Read error"),
    }
}
fn status_class(status: &Status) -> &'static str {
    match status {
        Status::Same => "same",
        Status::Changed => "changed",
        Status::LeftOnly => "left",
        Status::RightOnly => "right",
        Status::MetadataMatch => "unverified",
        _ => "error",
    }
}
fn size(entry: Option<&Entry>) -> String {
    match entry {
        None => "—".into(),
        Some(e) if e.kind == Kind::Directory => "DIR".into(),
        Some(e) if e.kind == Kind::Link => "LINK".into(),
        Some(e) => {
            let n = e.bytes.parse::<u64>().unwrap_or_default();
            if n >= 1_048_576 {
                format!("{:.1} MiB", n as f64 / 1_048_576.)
            } else if n >= 1024 {
                format!("{:.1} KiB", n as f64 / 1024.)
            } else {
                format!("{n} B")
            }
        }
    }
}

#[component]
fn App() -> impl IntoView {
    let english = RwSignal::new(false);
    let left = RwSignal::new(String::new());
    let right = RwSignal::new(String::new());
    let exclusions = RwSignal::new("-./.git/;-node_modules/;-__pycache__/;-.DS_Store".to_string());
    let content = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let comparison = RwSignal::new(None::<OpenedComparison>);
    let rows = RwSignal::new(Vec::<Row>::new());
    let offset = RwSignal::new(0usize);
    let search = RwSignal::new(String::new());
    let differences = RwSignal::new(true);
    let selected = RwSignal::new(BTreeSet::<String>::new());
    let diff = RwSignal::new(None::<TextDiff>);
    let diff_limit = RwSignal::new(500usize);
    let plan = RwSignal::new(None::<SyncPlan>);

    spawn_local(async move {
        match invoke::<Option<SavedSession>>("load_session", serde_json::json!({})).await {
            Ok(Some(saved)) => {
                left.set(saved.request.left);
                right.set(saved.request.right);
                exclusions.set(saved.request.exclusions);
                content.set(saved.request.content);
                english.set(saved.english);
            }
            Ok(None) => {}
            Err(e) => error.set(e),
        }
    });

    let reset = move || {
        comparison.set(None);
        rows.set(Vec::new());
        selected.set(BTreeSet::new());
        diff.set(None);
        plan.set(None);
        message.set(String::new());
    };
    let run_compare = move || {
        if busy.get_untracked() {
            return;
        }
        let request = CompareRequest {
            left: left.get_untracked(),
            right: right.get_untracked(),
            exclusions: exclusions.get_untracked(),
            content: content.get_untracked(),
        };
        reset();
        busy.set(true);
        error.set(String::new());
        offset.set(0);
        spawn_local(async move {
            match invoke::<OpenedComparison>("compare", serde_json::json!({ "request": request }))
                .await
            {
                Ok(opened) => {
                    left.set(opened.left.clone());
                    right.set(opened.right.clone());
                    rows.set(opened.page.rows.clone());
                    comparison.set(Some(opened));
                    let saved = SavedSession {
                        request,
                        english: english.get_untracked(),
                    };
                    if let Err(e) =
                        invoke::<()>("save_session", serde_json::json!({ "session": saved })).await
                    {
                        error.set(e);
                    }
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let pick = move |is_left: bool| {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<Option<String>>("pick_directory", serde_json::json!({})).await {
                Ok(Some(path)) => {
                    reset();
                    if is_left {
                        left.set(path)
                    } else {
                        right.set(path)
                    }
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let paginate = move |next: bool| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        let new_offset = if next {
            offset.get_untracked() + 500
        } else {
            offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<Page>(
                "page",
                serde_json::json!({"session":c.session,"offset":new_offset}),
            )
            .await
            {
                Ok(page) => {
                    rows.set(page.rows);
                    offset.set(new_offset);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let preview = move || {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        let ids = selected.get_untracked().into_iter().collect::<Vec<_>>();
        spawn_local(async move {
            match invoke::<SyncPlan>(
                "preview_sync",
                serde_json::json!({"session":c.session,"rowIds":ids}),
            )
            .await
            {
                Ok(p) => plan.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let apply = move || {
        let Some(p) = plan.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<SyncResult>("apply_sync", serde_json::json!({"planId":p.id})).await {
                Ok(result) => {
                    let completed = result
                        .actions
                        .iter()
                        .filter(|a| a.state == "completed")
                        .count();
                    message.set(format!(
                        "{} {completed}/{} · {}",
                        tr(english.get_untracked(), "已完成", "Completed"),
                        result.actions.len(),
                        result.journal
                    ));
                    if let Some(failed) = result.actions.iter().find(|a| a.error.is_some()) {
                        error.set(format!(
                            "{}: {}",
                            failed.path,
                            failed.error.clone().unwrap_or_default()
                        ));
                    }
                    comparison.set(None);
                    rows.set(Vec::new());
                    selected.set(BTreeSet::new());
                    diff.set(None);
                }
                Err(e) => error.set(e),
            }
            plan.set(None);
            busy.set(false);
        });
    };

    view! {
        <main class="workspace">
            <header class="topbar">
                <div class="identity"><span class="mark" aria-hidden="true">"⇄"</span><strong>"Clear Compare"</strong><span class="version">"0.1 preview"</span></div>
                <div class="top-actions"><span class="local-label">{move || tr(english.get(),"本地处理 · 自动备份","Local processing · Verified backups")}</span>
                <button class="quiet" on:click=move |_| english.update(|v| *v = !*v)>{move || if english.get() { "中文" } else { "English" }}</button></div>
            </header>
            <section class="path-section" aria-label="Comparison directories">
                <div class="path-pair">
                    <div class="path-field"><label for="left">{move || tr(english.get(),"左侧 · 来源","Left · Source")}</label><div class="path-input"><input id="left" prop:value=move || left.get() disabled=move || busy.get() placeholder=move || tr(english.get(),"选择来源文件夹","Choose source directory") on:input=move |ev| {reset();left.set(event_target_value(&ev));}/><button class="browse" aria-label="Choose source directory" disabled=move || busy.get() on:click=move |_| pick(true)>"…"</button></div></div>
                    <div class="path-direction" aria-hidden="true">"→"</div>
                    <div class="path-field"><label for="right">{move || tr(english.get(),"右侧 · 目标","Right · Destination")}</label><div class="path-input"><input id="right" prop:value=move || right.get() disabled=move || busy.get() placeholder=move || tr(english.get(),"选择目标文件夹","Choose destination directory") on:input=move |ev| {reset();right.set(event_target_value(&ev));}/><button class="browse" aria-label="Choose destination directory" disabled=move || busy.get() on:click=move |_| pick(false)>"…"</button></div></div>
                </div>
                <div class="compare-options"><label class="check-label"><input type="checkbox" prop:checked=move || content.get() disabled=move || busy.get() on:change=move |ev| {reset();content.set(event_target_checked(&ev));}/>{move || tr(english.get(),"校验文件内容（SHA-256）","Verify contents (SHA-256)")}</label>
                    <button class="primary" disabled=move || busy.get() || left.get().trim().is_empty() || right.get().trim().is_empty() on:click=move |_| run_compare()>{move || tr(english.get(),"比较文件夹","Compare directories")}</button>
                </div>
                <details class="rules"><summary>{move || tr(english.get(),"排除规则","Exclusion rules")}</summary><p>{move || tr(english.get(),"分号或换行分隔；./ 从根目录匹配，目录以 / 结尾。","Separate with semicolons or newlines; ./ anchors at root, / marks a directory.")}</p><textarea aria-label="Exclusion rules" prop:value=move || exclusions.get() disabled=move || busy.get() on:input=move |ev| {reset();exclusions.set(event_target_value(&ev));}/></details>
            </section>
            <Show when=move || !error.get().is_empty()><div class="notice error-notice" role="alert">{move || error.get()}</div></Show>
            <Show when=move || !message.get().is_empty()><div class="notice" role="status">{move || message.get()}<br/>{move || tr(english.get(),"请重新比较以查看最新状态。备份与操作日志已保留。","Compare again to refresh. Backups and operation journal are retained.")}</div></Show>
            <Show when=move || busy.get()><div class="progress" role="status"><span class="pulse"></span>{move || tr(english.get(),"正在处理，请稍候…","Working, please wait…")}<button class="quiet" on:click=move |_| spawn_local(async move { let _ = invoke::<()>("cancel",serde_json::json!({})).await; })>{move || tr(english.get(),"取消任务","Cancel task")}</button></div></Show>
            <Show when=move || comparison.get().is_some() fallback=move || view! {
                <section class="empty"><div class="empty-symbol" aria-hidden="true">"⇄"</div><h1>{move || tr(english.get(),"看清差异，再同步","See the difference. Then sync.")}</h1><p>{move || tr(english.get(),"选择两个独立的文件夹，比较内容，检查计划，然后复制你选择的文件。","Choose two separate directories, compare their contents, review a plan, then copy selected files.")}</p><div class="empty-notes"><span>{move || tr(english.get(),"目标独有文件保留","Destination-only files retained")}</span><span>{move || tr(english.get(),"覆盖前验证备份","Backups verified before overwrite")}</span></div></section>
            }>
                <div class="summary-bar">{move || comparison.get().map(|c| { let s=c.summary; view! {<span class="stat changed">{format!("{} {}",tr(english.get(),"修改","Changed"),s.changed)}</span><span class="stat left">{format!("{} {}",tr(english.get(),"仅左","Left only"),s.left_only)}</span><span class="stat right">{format!("{} {}",tr(english.get(),"仅右","Right only"),s.right_only)}</span><span class="stat same">{format!("{} {}",tr(english.get(),"一致","Same"),s.same)}</span><span class="stat">{format!("{} {} · {} {}",tr(english.get(),"未验证","Unverified"),s.unverified,tr(english.get(),"错误","Errors"),s.errors)}</span>} })}</div>
                <div class="filterbar"><input aria-label="Filter current page" placeholder=move || tr(english.get(),"筛选当前页路径…","Filter paths on this page…") prop:value=move || search.get() on:input=move |ev| search.set(event_target_value(&ev))/><label class="check-label"><input type="checkbox" prop:checked=move || differences.get() on:change=move |ev| differences.set(event_target_checked(&ev))/>{move || tr(english.get(),"只看差异","Differences only")}</label><span class="selection-count">{move || format!("{} {}",selected.get().len(),tr(english.get(),"个文件已选","files selected"))}</span><button class="primary" disabled=move || selected.get().is_empty() || busy.get() on:click=move |_| preview()>{move || tr(english.get(),"预览同步 →","Preview sync →")}</button></div>
                <section class="file-list" aria-label="Comparison results"><div class="file-header"><span></span><span>{move || tr(english.get(),"相对路径","Relative path")}</span><span>{move || tr(english.get(),"左侧大小","Left size")}</span><span>{move || tr(english.get(),"右侧大小","Right size")}</span><span>{move || tr(english.get(),"状态","Status")}</span></div>
                    <div class="file-scroll"><For each=move || { let query=search.get().to_lowercase(); rows.get().into_iter().filter(|r| (!differences.get() || r.status != Status::Same) && r.path.to_lowercase().contains(&query)).collect::<Vec<_>>() } key=|r| r.id.clone() children=move |row| {
                        let id=row.id.clone(); let select_id=id.clone(); let check_id=id.clone(); let open_id=id.clone();
                        let eligible=matches!(row.status,Status::Changed|Status::LeftOnly) && row.left.as_ref().is_some_and(|e| e.kind==Kind::File);
                        let left_size=size(row.left.as_ref()); let right_size=size(row.right.as_ref()); let state=row.status.clone(); let css=status_class(&state); let path=row.path.clone();
                        let detail=row.left.as_ref().and_then(|e| e.error.clone()).or_else(|| row.right.as_ref().and_then(|e| e.error.clone())).unwrap_or_else(|| path.clone());
                        view! {<div class="file-row" title=detail><input type="checkbox" aria-label=format!("Select {}",path) disabled=move || !eligible || busy.get() prop:checked=move || selected.get().contains(&check_id) on:change=move |ev| { let checked=event_target_checked(&ev);selected.update(|s| {if checked {s.insert(select_id.clone());} else {s.remove(&select_id);}});plan.set(None); }/>
                            <button class="file-name" disabled=move || busy.get() on:click=move |_| {
                                let Some(c)=comparison.get_untracked() else {return;}; let id=open_id.clone();busy.set(true);error.set(String::new());diff.set(None);diff_limit.set(500);
                                spawn_local(async move {match invoke::<TextDiff>("text_diff",serde_json::json!({"session":c.session,"rowId":id})).await {Ok(d)=>diff.set(Some(d)),Err(e)=>error.set(e)} busy.set(false);});
                            }>{path}</button><span class="file-size">{left_size}</span><span class="file-size">{right_size}</span><span class=format!("status {css}")>{move || label(&state,english.get())}</span></div>}
                    }/></div>
                </section>
                <div class="pagination"><span>{move || comparison.get().map(|c| format!("{}–{} / {}",offset.get()+1,(offset.get()+500).min(c.page.total),c.page.total)).unwrap_or_default()}</span><button class="quiet" disabled=move || offset.get()==0 || busy.get() on:click=move |_| paginate(false)>{move || tr(english.get(),"上一页","Previous")}</button><button class="quiet" disabled=move || busy.get() || comparison.get().is_none_or(|c| offset.get()+500>=c.page.total) on:click=move |_| paginate(true)>{move || tr(english.get(),"下一页","Next")}</button></div>
                <Show when=move || comparison.get().is_some_and(|c| !c.warnings.is_empty())><p class="warnings">{move || comparison.get().map(|c| c.warnings.join(" · "))}</p></Show>
            </Show>
            <Show when=move || diff.get().is_some()><section class="diff-panel"><header><strong>{move || diff.get().map(|d|d.path)}</strong><button class="quiet" on:click=move |_| diff.set(None)>{move || tr(english.get(),"关闭预览","Close preview")}</button></header><div class="diff-headings"><span>{move || tr(english.get(),"左侧 · 来源","Left · Source")}</span><span>{move || tr(english.get(),"右侧 · 目标","Right · Destination")}</span></div><div class="diff-scroll"><For each=move || diff.get().map(|d|d.lines.into_iter().take(diff_limit.get()).enumerate().collect::<Vec<_>>()).unwrap_or_default() key=|(i,_)| *i children=move |(_,line)| {
                let left_text=if line.tag=="insert" {String::new()} else {line.text.clone()};let right_text=if line.tag=="delete" {String::new()} else {line.text};
                view! {<div class=format!("diff-line {}",line.tag)><div class="diff-cell"><span class="line-number">{line.old_line.map(|n|n.to_string())}</span><pre>{left_text}</pre></div><div class="diff-cell"><span class="line-number">{line.new_line.map(|n|n.to_string())}</span><pre>{right_text}</pre></div></div>}
            }/></div><Show when=move || diff.get().is_some_and(|d|d.lines.len()>diff_limit.get())><button class="quiet" on:click=move |_| diff_limit.update(|n|*n+=500)>{move || tr(english.get(),"再显示 500 行","Show 500 more lines")}</button></Show></section></Show>
            <Show when=move || plan.get().is_some()><div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="sync-title"><h2 id="sync-title">{move || tr(english.get(),"检查同步计划","Review synchronization plan")}</h2><p>{move || tr(english.get(),"从左侧复制到右侧。不会删除目标独有文件。所有覆盖文件将先备份。","Copy left to right. Destination-only files are retained. Overwritten files are backed up first.")}</p><div class="plan-actions"><For each=move || plan.get().map(|p|p.actions).unwrap_or_default() key=|a|a.row_id.clone() children=move |a| view! {<div><span>{a.path}</span><strong>{move || if a.overwrite {tr(english.get(),"覆盖并备份","Overwrite + backup")} else {tr(english.get(),"新增","Create")}}</strong></div>}/></div><p class="backup-path">{move || plan.get().map(|p|format!("{}: {}",tr(english.get(),"备份位置","Backup location"),p.backup_root))}</p><footer><button class="quiet" disabled=move || busy.get() on:click=move |_| plan.set(None)>{move || tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move || busy.get() on:click=move |_| apply()>{move || tr(english.get(),"执行已预览的同步","Execute reviewed plan")}</button></footer></section></div></Show>
            <footer class="statusbar"><span>{move || tr(english.get(),"UTF-8 预览 · 不跟随符号链接 · 不自动删除","UTF-8 preview · Links not followed · No automatic deletion")}</span><span>"Clear Compare · MIT"</span></footer>
        </main>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
    if let Some(placeholder) = document().get_element_by_id("boot-placeholder") {
        placeholder.remove();
    }
}
