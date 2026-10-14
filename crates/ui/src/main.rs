mod panels;
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
    let reverse_sync = RwSignal::new(false);
    let selected = RwSignal::new(BTreeSet::<String>::new());
    let diff = RwSignal::new(None::<TextDiff>);
    let diff_limit = RwSignal::new(500usize);
    let ignore_case = RwSignal::new(false);
    let ignore_space = RwSignal::new(false);
    let ignore_endings = RwSignal::new(false);
    let plan = RwSignal::new(None::<SyncPlan>);
    let editor = RwSignal::new(None::<TextDocument>);
    let draft = RwSignal::new(String::new());
    let format_encoding = RwSignal::new(String::new());
    let format_ending = RwSignal::new(String::new());
    let format_bom = RwSignal::new(String::new());
    let can_restore = RwSignal::new(false);
    let history = RwSignal::new(None::<Vec<SyncHistoryEntry>>);
    let recovery = RwSignal::new(None::<RecoveryPreview>);
    let restore_preview = RwSignal::new(None::<RestorePreview>);
    let table_visible = RwSignal::new(false);
    let table = RwSignal::new(None::<OpenedTable>);
    let table_id = RwSignal::new(String::new());
    let table_tsv = RwSignal::new(false);
    let table_headers = RwSignal::new(true);
    let table_key = RwSignal::new(String::new());
    let table_tolerance = RwSignal::new(String::new());
    let table_offset = RwSignal::new(0usize);
    let binary = RwSignal::new(None::<BinaryPage>);
    let binary_id = RwSignal::new(String::new());
    let merge_visible = RwSignal::new(false);
    let merge_base = RwSignal::new(String::new());
    let merge_left = RwSignal::new(String::new());
    let merge_right = RwSignal::new(String::new());
    let merge_result = RwSignal::new(None::<MergeResult>);

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

    let open_table = move |id: String| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        let key = table_key.get_untracked();
        let key_column = if key.is_empty() {
            None
        } else {
            match key.parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => {
                    error.set(
                        tr(
                            english.get_untracked(),
                            "主键列必须是从 0 开始的列号",
                            "Key column must be a zero-based column number",
                        )
                        .into(),
                    );
                    return;
                }
            }
        };
        let tolerance = table_tolerance.get_untracked();
        let options = TableOptions {
            delimiter: if table_tsv.get_untracked() { "\t" } else { "," }.into(),
            headers: table_headers.get_untracked(),
            key_column,
            numeric_tolerance: if tolerance.is_empty() {
                None
            } else {
                Some(tolerance)
            },
        };
        busy.set(true);
        error.set(String::new());
        table_visible.set(true);
        table_id.set(id.clone());
        spawn_local(async move {
            match invoke::<OpenedTable>(
                "table_compare",
                serde_json::json!({"session":c.session,"rowId":id,"options":options}),
            )
            .await
            {
                Ok(value) => {
                    table.set(Some(value));
                    table_offset.set(0);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let paginate_table = move |forward: bool| {
        let Some(t) = table.get_untracked() else {
            return;
        };
        let next = if forward {
            table_offset.get_untracked() + 500
        } else {
            table_offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<Vec<TableRow>>(
                "table_page",
                serde_json::json!({"tableId":t.id,"offset":next}),
            )
            .await
            {
                Ok(rows) => {
                    table.update(|t| {
                        if let Some(t) = t {
                            t.report.rows = rows;
                        }
                    });
                    table_offset.set(next);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let open_binary = move |id: String, offset: String| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        binary.set(None);
        binary_id.set(id.clone());
        spawn_local(async move {
            match invoke::<BinaryPage>(
                "binary_page",
                serde_json::json!({"session":c.session,"rowId":id,"offset":offset}),
            )
            .await
            {
                Ok(p) => binary.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let reset = move || {
        comparison.set(None);
        rows.set(Vec::new());
        selected.set(BTreeSet::new());
        diff.set(None);
        binary.set(None);
        table.set(None);
        table_visible.set(false);
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
                serde_json::json!({"session":c.session,"rowIds":ids,"sourceSide":if reverse_sync.get_untracked(){Side::Right}else{Side::Left}}),
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

    let editor_dirty = move || {
        editor
            .get_untracked()
            .is_some_and(|d| d.text != draft.get_untracked())
            || !format_encoding.get_untracked().is_empty()
            || !format_ending.get_untracked().is_empty()
            || !format_bom.get_untracked().is_empty()
    };
    let open_editor = move |row_id: String, side: Side| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "请先保存或重置当前编辑内容。",
                    "Save or reset the current editor before opening another file.",
                )
                .into(),
            );
            return;
        }
        let old = editor.get_untracked();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            if let Some(old) = old {
                let _ =
                    invoke::<()>("close_document", serde_json::json!({"documentId":old.id})).await;
            }
            match invoke::<TextDocument>(
                "open_document",
                serde_json::json!({"session":c.session,"rowId":row_id,"side":side}),
            )
            .await
            {
                Ok(document) => {
                    can_restore.set(false);
                    draft.set(document.text.clone());
                    editor.set(Some(document));
                }
                Err(e) => {
                    editor.set(None);
                    error.set(e);
                }
            }
            busy.set(false);
        });
    };
    let save_editor = move || {
        let Some(document) = editor.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        let text = draft.get_untracked();
        let options = SaveOptions {
            encoding: match format_encoding.get_untracked().as_str() {
                "utf8" => Some(Encoding::Utf8),
                "utf16_le" => Some(Encoding::Utf16Le),
                "utf16_be" => Some(Encoding::Utf16Be),
                _ => None,
            },
            line_ending: match format_ending.get_untracked().as_str() {
                "lf" => Some(LineEnding::Lf),
                "crlf" => Some(LineEnding::CrLf),
                "cr" => Some(LineEnding::Cr),
                _ => None,
            },
            bom: match format_bom.get_untracked().as_str() {
                "yes" => Some(true),
                "no" => Some(false),
                _ => None,
            },
        };
        spawn_local(async move {
            match invoke::<SaveResult>(
                "save_document",
                serde_json::json!({"documentId":document.id,"text":text,"options":options}),
            )
            .await
            {
                Ok(result) => {
                    draft.set(result.document.text.clone());
                    editor.set(Some(result.document));
                    format_encoding.set(String::new());
                    format_ending.set(String::new());
                    format_bom.set(String::new());
                    can_restore.set(true);
                    reset();
                    message.set(format!(
                        "{} {}",
                        tr(
                            english.get_untracked(),
                            "文件已保存；原始内容备份：",
                            "File saved; original backup:"
                        ),
                        result.backup
                    ));
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let preview_restore = move || {
        let Some(d) = editor.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<RestorePreview>(
                "preview_editor_restore",
                serde_json::json!({"documentId":d.id}),
            )
            .await
            {
                Ok(p) => restore_preview.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let apply_restore = move || {
        let Some(p) = restore_preview.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<SaveResult>("apply_editor_restore", serde_json::json!({"planId":p.id}))
                .await
            {
                Ok(result) => {
                    draft.set(result.document.text.clone());
                    editor.set(Some(result.document));
                    can_restore.set(true);
                    reset();
                    message.set(format!(
                        "{} {}",
                        tr(
                            english.get_untracked(),
                            "已恢复；恢复前内容备份：",
                            "Restored; previous contents backed up:"
                        ),
                        result.backup
                    ));
                }
                Err(e) => error.set(e),
            }
            restore_preview.set(None);
            busy.set(false);
        });
    };
    let load_history = move || {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<Vec<SyncHistoryEntry>>("list_sync_history", serde_json::json!({})).await
            {
                Ok(entries) => history.set(Some(entries)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let preview_recovery = move |id: String| {
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "先保存或重置编辑内容，再预览恢复。",
                    "Save or reset the editor before previewing recovery.",
                )
                .into(),
            );
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<RecoveryPreview>(
                "preview_sync_recovery",
                serde_json::json!({"historyId":id}),
            )
            .await
            {
                Ok(p) => recovery.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let apply_recovery = move || {
        let Some(p) = recovery.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<RecoveryResult>(
                "apply_sync_recovery",
                serde_json::json!({"planId":p.id}),
            )
            .await
            {
                Ok(result) => {
                    let completed = result
                        .actions
                        .iter()
                        .filter(|a| a.state == "completed")
                        .count();
                    if let Some(failed) = result.actions.iter().find(|a| a.error.is_some()) {
                        error.set(format!(
                            "{}: {}",
                            failed.path,
                            failed.error.as_deref().unwrap_or_default()
                        ));
                    }
                    if let Some(d) = editor.get_untracked() {
                        let _ =
                            invoke::<()>("close_document", serde_json::json!({"documentId":d.id}))
                                .await;
                    }
                    editor.set(None);
                    draft.set(String::new());
                    can_restore.set(false);
                    reset();
                    message.set(format!(
                        "{} {}/{} · {}",
                        tr(english.get_untracked(), "恢复完成", "Recovered"),
                        completed,
                        result.actions.len(),
                        result.journal
                    ));
                }
                Err(e) => error.set(e),
            }
            recovery.set(None);
            busy.set(false);
        });
    };
    let close_editor = move || {
        let Some(document) = editor.get_untracked() else {
            return;
        };
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "有未保存的修改，请先保存或重置。",
                    "Unsaved changes: save or reset before closing.",
                )
                .into(),
            );
            return;
        }
        editor.set(None);
        spawn_local(async move {
            let _ = invoke::<()>(
                "close_document",
                serde_json::json!({"documentId":document.id}),
            )
            .await;
        });
    };

    view! {
        <main class="workspace">
            <header class="topbar">
                <div class="identity"><span class="mark" aria-hidden="true">"⇄"</span><strong>"Clear Compare"</strong><span class="version">"0.1 preview"</span></div>
                <div class="top-actions"><button class="quiet" disabled=move ||busy.get() on:click=move |_|load_history()>{move ||tr(english.get(),"操作历史","Operation history")}</button><span class="local-label">{move || tr(english.get(),"本地处理 · 自动备份","Local processing · Verified backups")}</span>
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
                <div class="text-options"><button class="quiet" on:click=move |_|merge_visible.set(!merge_visible.get_untracked())>{move || tr(english.get(),"三方文本合并","Three-way text merge")}</button><span>{move || tr(english.get(),"文本比较规则（打开文件时应用）","Text rules (applied when opening a file)")}</span><label><input type="checkbox" prop:checked=move || ignore_case.get() on:change=move |ev|ignore_case.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略大小写","Ignore case")}</label><label><input type="checkbox" prop:checked=move || ignore_space.get() on:change=move |ev|ignore_space.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略空白","Ignore whitespace")}</label><label><input type="checkbox" prop:checked=move || ignore_endings.get() on:change=move |ev|ignore_endings.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略换行风格","Ignore line ending style")}</label></div><div class="filterbar"><input aria-label="Filter current page" placeholder=move || tr(english.get(),"筛选当前页路径…","Filter paths on this page…") prop:value=move || search.get() on:input=move |ev| search.set(event_target_value(&ev))/><label class="check-label"><input type="checkbox" prop:checked=move || differences.get() on:change=move |ev| differences.set(event_target_checked(&ev))/>{move || tr(english.get(),"只看差异","Differences only")}</label><label class="check-label"><input type="checkbox" prop:checked=move ||reverse_sync.get() disabled=move ||busy.get() on:change=move |ev|{reverse_sync.set(event_target_checked(&ev));selected.set(BTreeSet::new());plan.set(None);}/>{move ||tr(english.get(),"右 → 左","Right → left")}</label><span class="selection-count">{move || format!("{} {}",selected.get().len(),tr(english.get(),"个文件已选","files selected"))}</span><button class="primary" disabled=move || selected.get().is_empty() || busy.get() on:click=move |_| preview()>{move || tr(english.get(),"预览同步 →","Preview sync →")}</button></div>
                <section class="file-list" aria-label="Comparison results"><div class="file-header"><span></span><span>{move || tr(english.get(),"相对路径","Relative path")}</span><span>{move || tr(english.get(),"左侧大小","Left size")}</span><span>{move || tr(english.get(),"右侧大小","Right size")}</span><span>{move || tr(english.get(),"状态","Status")}</span><span>{move || tr(english.get(),"编辑","Edit")}</span></div>
                    <div class="file-scroll"><For each=move || { let query=search.get().to_lowercase(); rows.get().into_iter().filter(|r| (!differences.get() || r.status != Status::Same) && r.path.to_lowercase().contains(&query)).collect::<Vec<_>>() } key=|r| r.id.clone() children=move |row| {
                        let id=row.id.clone(); let select_id=id.clone(); let check_id=id.clone(); let open_id=id.clone();
                        let eligible_left=matches!(row.status,Status::Changed|Status::LeftOnly) && row.left.as_ref().is_some_and(|e| e.kind==Kind::File);let eligible_right=matches!(row.status,Status::Changed|Status::RightOnly) && row.right.as_ref().is_some_and(|e|e.kind==Kind::File);
                        let can_left=row.left.as_ref().is_some_and(|e|e.kind==Kind::File && e.error.is_none());
                        let can_right=row.right.as_ref().is_some_and(|e|e.kind==Kind::File && e.error.is_none());
                        let edit_left=id.clone();let edit_right=id.clone();let hex_id=id.clone();let table_row_id=id.clone();
                        let left_size=size(row.left.as_ref()); let right_size=size(row.right.as_ref()); let state=row.status.clone(); let css=status_class(&state); let path=row.path.clone();
                        let detail=row.left.as_ref().and_then(|e| e.error.clone()).or_else(|| row.right.as_ref().and_then(|e| e.error.clone())).unwrap_or_else(|| path.clone());
                        view! {<div class="file-row" title=detail><input type="checkbox" aria-label=format!("Select {}",path) disabled=move || !(if reverse_sync.get(){eligible_right}else{eligible_left}) || busy.get() prop:checked=move || selected.get().contains(&check_id) on:change=move |ev| { let checked=event_target_checked(&ev);selected.update(|s| {if checked {s.insert(select_id.clone());} else {s.remove(&select_id);}});plan.set(None); }/>
                            <button class="file-name" disabled=move || busy.get() on:click=move |_| {
                                let Some(c)=comparison.get_untracked() else {return;}; let id=open_id.clone();busy.set(true);error.set(String::new());diff.set(None);diff_limit.set(500);
                                spawn_local(async move {match invoke::<TextDiff>("text_diff",serde_json::json!({"session":c.session,"rowId":id,"options":{"ignore_case":ignore_case.get_untracked(),"ignore_whitespace":ignore_space.get_untracked(),"ignore_line_endings":ignore_endings.get_untracked()}})).await {Ok(d)=>diff.set(Some(d)),Err(e)=>error.set(e)} busy.set(false);});
                            }>{path}</button><span class="file-size">{left_size}</span><span class="file-size">{right_size}</span><span class=format!("status {css}")>{move || label(&state,english.get())}</span><span class="row-edits"><button class="quiet" disabled=move ||busy.get() || !(can_left || can_right) on:click=move |_|open_table(table_row_id.clone())>{move ||tr(english.get(),"表","CSV")}</button><button class="quiet" disabled=move || busy.get() || !(can_left || can_right) on:click=move |_|open_binary(hex_id.clone(),"0".into())>"Hex"</button><button class="quiet" disabled=move || busy.get() || !can_left on:click=move |_| open_editor(edit_left.clone(),Side::Left)>{move || tr(english.get(),"左","L")}</button><button class="quiet" disabled=move || busy.get() || !can_right on:click=move |_| open_editor(edit_right.clone(),Side::Right)>{move || tr(english.get(),"右","R")}</button></span></div>}
                    }/></div>
                </section>
                <div class="pagination"><span>{move || comparison.get().map(|c| format!("{}–{} / {}",offset.get()+1,(offset.get()+500).min(c.page.total),c.page.total)).unwrap_or_default()}</span><button class="quiet" disabled=move || offset.get()==0 || busy.get() on:click=move |_| paginate(false)>{move || tr(english.get(),"上一页","Previous")}</button><button class="quiet" disabled=move || busy.get() || comparison.get().is_none_or(|c| offset.get()+500>=c.page.total) on:click=move |_| paginate(true)>{move || tr(english.get(),"下一页","Next")}</button></div>
                <Show when=move || comparison.get().is_some_and(|c| !c.warnings.is_empty())><p class="warnings">{move || comparison.get().map(|c| c.warnings.join(" · "))}</p></Show>
            </Show>
            <Show when=move || diff.get().is_some()><section class="diff-panel"><header><button class="quiet" on:click=move |_|{if let Some(d)=diff.get_untracked(){merge_left.set(d.lines.iter().filter(|l|l.old_line.is_some()).map(|l|l.text.clone()).collect::<String>());merge_right.set(d.lines.iter().filter(|l|l.new_line.is_some()).map(|l|l.right_text.clone().unwrap_or_else(||l.text.clone())).collect::<String>());merge_result.set(None);merge_visible.set(true);}}>{move || tr(english.get(),"载入合并面板","Load into merge panel")}</button><strong>{move || diff.get().map(|d|d.path)}</strong><button class="quiet" on:click=move |_| diff.set(None)>{move || tr(english.get(),"关闭预览","Close preview")}</button></header><div class="diff-headings"><span>{move || tr(english.get(),"左侧 · 来源","Left · Source")}</span><span>{move || tr(english.get(),"右侧 · 目标","Right · Destination")}</span></div><div class="diff-scroll"><For each=move || diff.get().map(|d|d.lines.into_iter().take(diff_limit.get()).enumerate().collect::<Vec<_>>()).unwrap_or_default() key=|(i,_)| *i children=move |(_,line)| {
                let left_text=if line.tag=="insert" {String::new()} else {line.text.clone()};let right_text=if line.tag=="delete" {String::new()} else {line.right_text.unwrap_or(line.text)};
                view! {<div class=format!("diff-line {}",line.tag)><div class="diff-cell"><span class="line-number">{line.old_line.map(|n|n.to_string())}</span><pre>{left_text}</pre></div><div class="diff-cell"><span class="line-number">{line.new_line.map(|n|n.to_string())}</span><pre>{right_text}</pre></div></div>}
            }/></div><Show when=move || diff.get().is_some_and(|d|d.lines.len()>diff_limit.get())><button class="quiet" on:click=move |_| diff_limit.update(|n|*n+=500)>{move || tr(english.get(),"再显示 500 行","Show 500 more lines")}</button></Show></section></Show>
            {panels::table::panel(english,busy,panels::table::Signals{table_visible, table, table_id, table_tsv, table_headers, table_key, table_tolerance, table_offset}, open_table, paginate_table)}
            {panels::binary::panel(english,busy,panels::binary::Signals{binary, binary_id}, open_binary)}
            {panels::merge::panel(english,busy,panels::merge::Signals{merge_visible, merge_base, merge_left, merge_right, merge_result, editor, draft, error})}
            {panels::editor::panel(english,busy,panels::editor::Signals{editor, draft, format_encoding, format_ending, format_bom, can_restore}, close_editor, save_editor, preview_restore)}
            {panels::history::panel(english,busy,history,recovery,preview_recovery,apply_recovery)}
            <Show when=move ||restore_preview.get().is_some()><div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="restore-title"><h2 id="restore-title">{move ||tr(english.get(),"检查恢复计划","Review restore plan")}</h2><p>{move ||tr(english.get(),"恢复上次保存前的原始字节。恢复前的当前内容也会备份；文件若发生外部修改，恢复将被阻止。","Restore the original bytes from before the last save. Current contents are backed up first; external changes block restoration.")}</p><p class="backup-path">{move ||restore_preview.get().map(|p|format!("{} ← {} · {} bytes",p.target,p.backup,p.bytes))}</p><footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|restore_preview.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move ||busy.get() on:click=move |_|apply_restore()>{move ||tr(english.get(),"执行已预览的恢复","Execute reviewed restore")}</button></footer></section></div></Show>
            <Show when=move || plan.get().is_some()><div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="sync-title"><h2 id="sync-title">{move || tr(english.get(),"检查同步计划","Review synchronization plan")}</h2><p>{move || tr(english.get(),if plan.get().is_some_and(|p|p.source_side==Side::Right){"从右侧复制到左侧。不会删除目标独有文件。所有覆盖文件将先备份。"}else{"从左侧复制到右侧。不会删除目标独有文件。所有覆盖文件将先备份。"},if plan.get().is_some_and(|p|p.source_side==Side::Right){"Copy right to left. Destination-only files are retained. Overwritten files are backed up first."}else{"Copy left to right. Destination-only files are retained. Overwritten files are backed up first."})}</p><div class="plan-actions"><For each=move || plan.get().map(|p|p.actions).unwrap_or_default() key=|a|a.row_id.clone() children=move |a| view! {<div><span>{a.path}</span><strong>{move || if a.overwrite {tr(english.get(),"覆盖并备份","Overwrite + backup")} else {tr(english.get(),"新增","Create")}}</strong></div>}/></div><p class="backup-path">{move || plan.get().map(|p|format!("{}: {}",tr(english.get(),"备份位置","Backup location"),p.backup_root))}</p><footer><button class="quiet" disabled=move || busy.get() on:click=move |_| plan.set(None)>{move || tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move || busy.get() on:click=move |_| apply()>{move || tr(english.get(),"执行已预览的同步","Execute reviewed plan")}</button></footer></section></div></Show>
            <footer class="statusbar"><span>{move || tr(english.get(),"UTF-8 / UTF-16 预览 · 不跟随符号链接 · 不自动删除","UTF-8 / UTF-16 preview · Links not followed · No automatic deletion")}</span><span>"Clear Compare · MIT"</span></footer>
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
