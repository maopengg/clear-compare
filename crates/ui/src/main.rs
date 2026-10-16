mod operations;
mod panels;
mod preferences;
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "export function installWorkspaceMenus(){
 document.addEventListener('click',event=>{
  const inside=event.target.closest('details[name=workspace-menu]');
  document.querySelectorAll('details[name=workspace-menu][open]').forEach(menu=>{
   if(menu!==inside || event.target.closest('.app-menu button'))menu.open=false;
  });
 });
 document.addEventListener('keydown',event=>{
  if(event.key!=='Escape')return;
  const menu=document.querySelector('details[name=workspace-menu][open]');
  if(menu){menu.open=false;menu.querySelector('summary').focus();event.preventDefault();}
 });
} export function operationDelay(){return operationPollDelay(16);} export function operationPollDelay(ms){return new Promise(resolve=>setTimeout(resolve,ms));} export function desktopInvoke(command,args){ if(!window.__TAURI__) return Promise.reject('Please open the installed Clear Compare desktop app.'); return window.__TAURI__.core.invoke(command,args); }")]
extern "C" {
    #[wasm_bindgen(js_name = installWorkspaceMenus)]
    fn install_workspace_menus();
    #[wasm_bindgen(js_name = operationDelay)]
    fn operation_delay() -> js_sys::Promise;
    #[wasm_bindgen(js_name = operationPollDelay)]
    fn operation_poll_delay(milliseconds: u32) -> js_sys::Promise;
    #[wasm_bindgen(js_name = desktopInvoke)]
    fn desktop_invoke(command: &str, args: JsValue) -> js_sys::Promise;
}

async fn invoke<T: DeserializeOwned>(command: &str, args: impl Serialize) -> Result<T, String> {
    let value = serde_wasm_bindgen::to_value(&args).map_err(|e| e.to_string())?;
    let result = wasm_bindgen_futures::JsFuture::from(desktop_invoke(command, value))
        .await
        .map_err(|e| {
            e.as_string()
                .or_else(|| {
                    js_sys::Reflect::get(&e, &JsValue::from_str("message"))
                        .ok()
                        .and_then(|v| v.as_string())
                })
                .unwrap_or_else(|| format!("{e:?}"))
        })?;
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
        Some(e) if e.kind != Kind::File => "—".into(),
        Some(e) => {
            let n = e.bytes.parse::<u64>().unwrap_or_default();
            let unit = if n >= 1_000_000_000_000 {
                Some((1_000_000_000_000_f64, "TB"))
            } else if n >= 1_000_000_000 {
                Some((1_000_000_000_f64, "GB"))
            } else if n >= 1_000_000 {
                Some((1_000_000_f64, "MB"))
            } else if n >= 1000 {
                Some((1000_f64, "KB"))
            } else {
                None
            };
            unit.map(|(divisor, unit)| format!("{:.1} {unit}", n as f64 / divisor))
                .unwrap_or_else(|| format!("{n} B"))
        }
    }
}

#[component]
fn App() -> impl IntoView {
    let operation_progress = RwSignal::new(None::<TaskProgress>);
    let bidi = panels::bidirectional::Signals::new();
    let restart = RwSignal::new(None::<ResumePreview>);
    let restart_offset = RwSignal::new(0usize);
    let git_visible = RwSignal::new(false);
    let archive_visible = RwSignal::new(false);
    let folder_merge_visible = RwSignal::new(false);
    let folder_merge_preview = RwSignal::new(None::<FolderMergePreview>);
    let library = RwSignal::new(None::<Vec<LibraryEntry>>);
    let session_name = RwSignal::new(String::new());
    let active_library = RwSignal::new(None::<(String, String)>);
    let project_saved = RwSignal::new(String::new());
    let project_store = panels::projects::Store {
        active: active_library,
        name: session_name,
        saved: project_saved,
        library,
    };
    let exclusions_visible = RwSignal::new(false);

    let english = RwSignal::new(false);
    let help_visible = RwSignal::new(false);
    let stored_theme = preferences::load();
    let theme_error = RwSignal::new(stored_theme.as_ref().err().cloned().unwrap_or_default());
    let theme = RwSignal::new(stored_theme.unwrap_or_default());
    let left = RwSignal::new(String::new());
    let right = RwSignal::new(String::new());
    let exclusions = RwSignal::new("-./.git/;-node_modules/;-__pycache__/;-.DS_Store".to_string());
    let content = RwSignal::new(false);
    let compare_stage = RwSignal::new(0u8);
    let compare_cancelled = RwSignal::new(false);
    let comparison_verified = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let home_visible = RwSignal::new(true);
    let message = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let comparison = RwSignal::new(None::<OpenedComparison>);
    let rows = RwSignal::new(Vec::<Row>::new());
    let outline = RwSignal::new(std::collections::BTreeMap::<String, OutlineNode>::new());
    let tree_mode = RwSignal::new(true);
    let expanded_dirs = RwSignal::new(BTreeSet::<String>::new());
    let expand_all = RwSignal::new(false);
    let focused_row = RwSignal::new(None::<String>);
    let offset = RwSignal::new(0usize);
    let search = RwSignal::new(String::new());
    let differences = RwSignal::new(true);
    let parent_id = RwSignal::new(None::<String>);
    let breadcrumbs = RwSignal::new(Vec::<Breadcrumb>::new());
    let recursive = RwSignal::new(true);
    let folder_sort = RwSignal::new(String::from("path"));
    let descending = RwSignal::new(false);
    let visible_total = RwSignal::new(0usize);
    let reverse_sync = RwSignal::new(false);
    let file_operation = RwSignal::new("copy".to_string());
    let rename_name = RwSignal::new(String::new());
    let selected = RwSignal::new(BTreeSet::<String>::new());
    let diff = RwSignal::new(None::<TextDiff>);
    let text_context = RwSignal::new(None::<panels::text_merge::Context>);
    let text_merge_dirty = RwSignal::new(false);
    let ignore_case = RwSignal::new(false);
    let ignore_space = RwSignal::new(false);
    let ignore_endings = RwSignal::new(false);
    let plan = RwSignal::new(None::<SyncPlan>);
    let editor = RwSignal::new(None::<TextDocument>);
    let draft = RwSignal::new(String::new());
    let saved_text = RwSignal::new(String::new());
    let text_history = RwSignal::new(clear_contracts::edit_history::History::default());
    let merge_tokens = RwSignal::new(Vec::<String>::new());
    let format_encoding = RwSignal::new(String::new());
    let format_ending = RwSignal::new(String::new());
    let format_bom = RwSignal::new(String::new());
    let can_restore = RwSignal::new(false);
    let history = RwSignal::new(None::<Vec<SyncHistoryEntry>>);
    let recovery = RwSignal::new(None::<RecoveryPreview>);
    let restore_preview = RwSignal::new(None::<RestorePreview>);
    let table_visible = RwSignal::new(false);
    let table = RwSignal::new(None::<OpenedTable>);
    let table_document = RwSignal::new(None::<TableDocumentInfo>);
    let table_left_mode = RwSignal::new("auto".to_string());
    let table_right_mode = RwSignal::new("auto".to_string());
    let table_left_index = RwSignal::new("0".to_string());
    let table_right_index = RwSignal::new("0".to_string());
    let table_headers = RwSignal::new(true);
    let table_align_names = RwSignal::new(false);
    let table_key = RwSignal::new(String::new());
    let table_tolerance = RwSignal::new(String::new());
    let table_offset = RwSignal::new(0usize);
    let picture = RwSignal::new(None::<PicturePreview>);
    let picture_id = RwSignal::new(String::new());
    let picture_tolerance = RwSignal::new(String::from("0"));
    let picture_ignore_alpha = RwSignal::new(false);
    let binary = RwSignal::new(None::<BinaryPage>);
    let binary_id = RwSignal::new(String::new());
    let merge_visible = RwSignal::new(false);
    let merge_base = RwSignal::new(String::new());
    let merge_left = RwSignal::new(String::new());
    let merge_right = RwSignal::new(String::new());
    let merge_result = RwSignal::new(None::<MergeResult>);

    let change_draft = move |value: String| {
        let text = clear_contracts::edit_history::normalize(&value);
        let mut outcome = Ok(());
        text_history.update(|h| outcome = h.record(&text));
        match outcome {
            Ok(()) => draft.set(text),
            Err(e) => {
                error.set(e);
                draft.set(text_history.with_untracked(|h| h.current().to_string()));
            }
        }
    };
    let reset_draft = move |value: String| {
        let text = clear_contracts::edit_history::normalize(&value);
        text_history.update(|h| h.reset(&text));
        saved_text.set(text.clone());
        draft.set(text);
        merge_tokens.set(clear_contracts::edit_history::conflict_tokens(
            &draft.get_untracked(),
        ));
    };
    let project_snapshot = Signal::derive(move || SavedSession {
        request: CompareRequest {
            left: left.get(),
            right: right.get(),
            exclusions: exclusions.get(),
            content: content.get(),
        },
        english: english.get(),
        view: SessionView {
            search: search.get(),
            differences_only: differences.get(),
            recursive: recursive.get(),
            sort: match folder_sort.get().as_str() {
                "status" => FolderSort::Status,
                "left_size" => FolderSort::LeftSize,
                "right_size" => FolderSort::RightSize,
                _ => FolderSort::Path,
            },
            descending: descending.get(),
            reverse_sync: reverse_sync.get(),
            text_options: TextOptions {
                ignore_case: ignore_case.get(),
                ignore_whitespace: ignore_space.get(),
                ignore_line_endings: ignore_endings.get(),
            },
        },
    });
    let current_session = move || project_snapshot.get_untracked();
    let project_dirty = Memo::new(move |_| {
        active_library.get().is_some()
            && panels::projects::signature(&session_name.get(), &project_snapshot.get())
                != project_saved.get()
    });
    let apply_session = move |saved: SavedSession| {
        left.set(saved.request.left);
        right.set(saved.request.right);
        exclusions.set(saved.request.exclusions);
        content.set(saved.request.content);
        english.set(saved.english);
        search.set(saved.view.search);
        differences.set(saved.view.differences_only);
        recursive.set(saved.view.recursive);
        folder_sort.set(
            match saved.view.sort {
                FolderSort::Path => "path",
                FolderSort::Status => "status",
                FolderSort::LeftSize => "left_size",
                FolderSort::RightSize => "right_size",
            }
            .into(),
        );
        descending.set(saved.view.descending);
        reverse_sync.set(saved.view.reverse_sync);
        ignore_case.set(saved.view.text_options.ignore_case);
        ignore_space.set(saved.view.text_options.ignore_whitespace);
        ignore_endings.set(saved.view.text_options.ignore_line_endings);
    };
    spawn_local(async move {
        match invoke::<Option<LibraryEntry>>("load_active_project", serde_json::json!({})).await {
            Ok(Some(entry)) => {
                apply_session(entry.session.clone());
                project_store.accept(&entry);
            }
            Ok(None) => {
                match invoke::<Option<SavedSession>>("load_session", serde_json::json!({})).await {
                    Ok(Some(saved)) => apply_session(saved),
                    Ok(None) => {}
                    Err(e) => error.set(e),
                }
            }
            Err(e) => error.set(e),
        }
    });

    Effect::watch(
        move || english.get(),
        move |en, _, _| {
            let en = *en;
            spawn_local(async move {
                if let Err(e) =
                    invoke::<()>("set_native_language", serde_json::json!({"english":en})).await
                {
                    error.set(e);
                }
            });
        },
        true,
    );

    let compare_table_document = move || {
        let Some(doc) = table_document.get_untracked() else {
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
        let source = |mode: String, index: String| -> Result<TableSource, String> {
            match mode.as_str() {
                "html" => index
                    .parse::<usize>()
                    .map(|table| TableSource::Html { table })
                    .map_err(|_| "HTML table index must be a nonnegative integer".into()),
                "xlsx" => index
                    .parse::<usize>()
                    .map(|sheet| TableSource::Xlsx { sheet })
                    .map_err(|_| "Worksheet index must be a nonnegative integer".into()),
                "delimited" => Ok(TableSource::Delimited),
                "csv" => Ok(TableSource::Csv),
                "tsv" => Ok(TableSource::Tsv),
                _ => Ok(TableSource::Auto),
            }
        };
        let left_source = match source(
            table_left_mode.get_untracked(),
            table_left_index.get_untracked(),
        ) {
            Ok(v) => v,
            Err(e) => {
                error.set(e);
                return;
            }
        };
        let right_source = match source(
            table_right_mode.get_untracked(),
            table_right_index.get_untracked(),
        ) {
            Ok(v) => v,
            Err(e) => {
                error.set(e);
                return;
            }
        };
        let options = TableOptions {
            left_source,
            right_source,
            delimiter: ",".into(),
            headers: table_headers.get_untracked(),
            align_columns_by_name: table_align_names.get_untracked(),
            key_column,
            numeric_tolerance: if tolerance.is_empty() {
                None
            } else {
                Some(tolerance)
            },
        };

        busy.set(true);
        error.set(String::new());
        table.set(None);
        spawn_local(async move {
            match invoke::<OpenedTable>(
                "table_compare",
                serde_json::json!({"documentId":doc.id,"options":options}),
            )
            .await
            {
                Ok(value) => {
                    table_document.update(|info| {
                        if let Some(info) = info
                            && info.id == doc.id
                        {
                            if !value.report.left_tables.is_empty() {
                                info.left_tables = value.report.left_tables.clone();
                                info.left_error = None;
                            }
                            if !value.report.right_tables.is_empty() {
                                info.right_tables = value.report.right_tables.clone();
                                info.right_error = None;
                            }
                        }
                    });
                    table.set(Some(value));
                    table_offset.set(0);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let open_table = move |id: String| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        table_visible.set(true);
        table.set(None);
        table_document.set(None);
        table_left_mode.set("auto".into());
        table_right_mode.set("auto".into());
        table_left_index.set("0".into());
        table_right_index.set("0".into());
        spawn_local(async move {
            match invoke::<TableDocumentInfo>(
                "open_table_document",
                serde_json::json!({"session":c.session,"rowId":id}),
            )
            .await
            {
                Ok(info) => {
                    table_document.set(Some(info));
                    busy.set(false);
                    compare_table_document();
                }
                Err(e) => {
                    error.set(e);
                    busy.set(false);
                }
            }
        });
    };
    let table_catalog = move || {
        let Some(doc) = table_document.get_untracked() else {
            return;
        };
        let left_html = table_left_mode.get_untracked() == "html"
            || (table_left_mode.get_untracked() == "auto" && doc.left_html);
        let right_html = table_right_mode.get_untracked() == "html"
            || (table_right_mode.get_untracked() == "auto" && doc.right_html);
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<TableDocumentInfo>("table_catalog",serde_json::json!({"documentId":doc.id,"leftHtml":left_html,"rightHtml":right_html})).await{Ok(info)=>table_document.set(Some(info)),Err(e)=>error.set(e)}
            busy.set(false);
        });
    };
    let close_table = move || {
        let doc = table_document.get_untracked();
        table_document.set(None);
        table.set(None);
        table_visible.set(false);
        if let Some(doc) = doc {
            spawn_local(async move {
                let _ = invoke::<()>(
                    "close_table_document",
                    serde_json::json!({"documentId":doc.id}),
                )
                .await;
            });
        }
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
    let open_picture = move |id: String| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        let tolerance = match picture_tolerance.get_untracked().parse::<u8>() {
            Ok(t) => t,
            Err(_) => {
                error.set(
                    tr(
                        english.get_untracked(),
                        "图片容差必须在 0–255 之间。",
                        "Picture tolerance must be 0–255.",
                    )
                    .into(),
                );
                return;
            }
        };
        let options = PictureOptions {
            tolerance,
            ignore_alpha: picture_ignore_alpha.get_untracked(),
        };
        busy.set(true);
        error.set(String::new());
        picture.set(None);
        picture_id.set(id.clone());
        spawn_local(async move {
            match invoke::<PicturePreview>(
                "picture_preview",
                serde_json::json!({"session":c.session,"rowId":id,"options":options}),
            )
            .await
            {
                Ok(p) => picture.set(Some(p)),
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
        if binary_id.get_untracked() != id {
            binary.set(None);
        }
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
    let build_query = move || FolderQuery {
        parent_id: parent_id.get_untracked(),
        recursive: recursive.get_untracked(),
        search: search.get_untracked(),
        differences_only: differences.get_untracked(),
        sort: match folder_sort.get_untracked().as_str() {
            "status" => FolderSort::Status,
            "left_size" => FolderSort::LeftSize,
            "right_size" => FolderSort::RightSize,
            _ => FolderSort::Path,
        },
        descending: descending.get_untracked(),
    };
    let load_folder = move |new_offset: usize| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        let query = build_query();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<FolderPage>(
                "query_folder",
                serde_json::json!({"session":c.session,"query":query,"offset":new_offset,"expanded":if tree_mode.get_untracked(){Some(expanded_dirs.get_untracked().into_iter().collect::<Vec<_>>())}else{None},"expandAll":expand_all.get_untracked()}),
            )
            .await
            {
                Ok(result) => {
                    outline.set(result.outline);
                    focused_row.set(None);
                    rows.set(result.page.rows);
                    visible_total.set(result.page.total);
                    offset.set(new_offset);
                    breadcrumbs.set(result.breadcrumbs);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let browse_folder = move |id: Option<String>| {
        parent_id.set(id);
        recursive.set(false);
        diff.set(None);
        binary.set(None);
        picture.set(None);
        table.set(None);
        table_visible.set(false);
        load_folder(0);
    };
    let toggle_directory = move |id: String| {
        if busy.get_untracked() {
            return;
        }
        if !tree_mode.get_untracked() {
            browse_folder(Some(id));
            return;
        }
        expanded_dirs.update(|dirs| {
            if !dirs.remove(&id) {
                dirs.insert(id);
            }
        });
        load_folder(0);
    };
    let open_text = move |id: String| {
        if busy.get_untracked() {
            return;
        }
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        diff.set(None);
        text_context.set(None);
        let merge_row = rows.with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned());
        let text_options = TextOptions {
            ignore_case: ignore_case.get_untracked(),
            ignore_whitespace: ignore_space.get_untracked(),
            ignore_line_endings: ignore_endings.get_untracked(),
        };
        spawn_local(async move {
            match invoke::<TextDiff>(
                "text_diff",
                serde_json::json!({"session":c.session,"rowId":id,"options":text_options}),
            )
            .await
            {
                Ok(d) => {
                    text_context.set(merge_row.map(|row| panels::text_merge::Context {
                        diff_id: d.id.clone(),
                        session: c.session.clone(),
                        row_id: id.clone(),
                        options: text_options.clone(),
                        left: row.left.is_some_and(|e| e.kind == Kind::File),
                        right: row.right.is_some_and(|e| e.kind == Kind::File),
                    }));
                    diff.set(Some(d));
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let reset = move || {
        bidi.preview.set(None);
        bidi.recovery.set(None);
        file_operation.set("copy".into());
        comparison.set(None);
        rows.set(Vec::new());
        outline.set(std::collections::BTreeMap::new());
        expanded_dirs.set(BTreeSet::new());
        expand_all.set(false);
        focused_row.set(None);
        parent_id.set(None);
        breadcrumbs.set(Vec::new());
        visible_total.set(0);
        selected.set(BTreeSet::new());
        diff.set(None);
        binary.set(None);
        picture.set(None);
        table.set(None);
        table_visible.set(false);
        table_document.set(None);
        plan.set(None);
        folder_merge_preview.set(None);
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
        let verify_differences = differences.get_untracked();
        reset();
        busy.set(true);
        error.set(String::new());
        offset.set(0);
        compare_cancelled.set(false);
        compare_stage.set(1);
        comparison_verified.set(false);
        spawn_local(async move {
            if let Err(e) = project_store.persist(current_session()).await {
                error.set(e);
                compare_stage.set(0);
                busy.set(false);
                return;
            }
            // Publish the metadata list before doing any full-content scan. Each
            // phase has a fresh native snapshot; no metadata digest is reused.
            let passes: &[bool] = if request.content || verify_differences {
                &[false, true]
            } else {
                &[false]
            };
            for &verify in passes {
                if compare_cancelled.get_untracked() {
                    break;
                }
                compare_stage.set(if verify { 2 } else { 1 });
                let mut phase_request = request.clone();
                phase_request.content = verify && request.content;
                let result = async {
                    let opened = invoke::<OpenedComparison>(
                        "compare",
                        serde_json::json!({ "request": phase_request, "verifyMatching": verify && !request.content }),
                    )
                    .await?;
                    let result = invoke::<FolderPage>(
                        "query_folder",
                        serde_json::json!({
                            "session": opened.session, "query": build_query(), "offset": 0,
                            "expanded": if tree_mode.get_untracked(){Some(Vec::<String>::new())}else{None}, "expandAll": false
                        }),
                    )
                    .await?;
                    left.set(opened.left.clone());
                    right.set(opened.right.clone());
                    comparison_verified.set(verify);
                    comparison.set(Some(opened));
                    outline.set(result.outline);
                    rows.set(result.page.rows);
                    visible_total.set(result.page.total);
                    breadcrumbs.set(result.breadcrumbs);
                    Ok::<(), String>(())
                }
                .await;
                if let Err(e) = result {
                    error.set(e);
                    break;
                }
                if !verify && (request.content || verify_differences) {
                    // Let the WebView paint the quick result before verification.
                    let _ = wasm_bindgen_futures::JsFuture::from(operation_delay()).await;
                }
            }
            if comparison.get_untracked().is_some() {
                let saved = current_session();
                if let Err(e) =
                    invoke::<()>("save_session", serde_json::json!({ "session": saved })).await
                {
                    error.set(e);
                }
            }
            compare_stage.set(0);
            busy.set(false);
        });
    };
    let change_exclusions = move |rules: String| {
        if busy.get_untracked() {
            return;
        }
        let rescan = comparison.get_untracked().is_some();
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result = async {
                invoke::<()>("validate_exclusions", serde_json::json!({"rules":rules})).await?;
                let mut candidate = current_session();
                candidate.request.exclusions = rules.clone();
                project_store.persist(candidate.clone()).await?;
                if active_library.get_untracked().is_none()
                    && !candidate.request.left.trim().is_empty()
                    && !candidate.request.right.trim().is_empty()
                {
                    invoke::<()>("save_session", serde_json::json!({"session":candidate})).await?;
                }
                reset();
                exclusions.set(rules);
                exclusions_visible.set(false);
                Ok::<(), String>(())
            }
            .await;
            busy.set(false);
            match result {
                Ok(()) => {
                    if rescan {
                        run_compare();
                    } else {
                        message.set(
                            tr(
                                english.get_untracked(),
                                "排除规则已应用；未命名项目请保存为项目。",
                                "Exclusions applied; save unnamed work as a project.",
                            )
                            .into(),
                        );
                    }
                }
                Err(e) => error.set(e),
            }
        });
    };
    let exclude_entry = move |id: String| {
        if busy.get_untracked() {
            return;
        }
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            let result=invoke::<String>("exclusion_for_entry",serde_json::json!({"session":c.session,"rowId":id,"current":exclusions.get_untracked()})).await;
            busy.set(false);
            match result {
                Ok(rules) => change_exclusions(rules),
                Err(e) => error.set(e),
            }
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
        let new_offset = if next {
            offset.get_untracked().saturating_add(500)
        } else {
            offset.get_untracked().saturating_sub(500)
        };
        load_folder(new_offset);
    };
    let update_editor_locations = move |locations: Vec<DocumentLocation>, stale: Vec<String>| {
        for location in locations {
            editor.update(|value| {
                if let Some(doc) = value
                    && doc.id == location.id
                {
                    doc.path = location.path;
                    doc.absolute_path = location.absolute_path;
                }
            });
        }
        if let Some(doc) = editor.get_untracked()
            && stale.contains(&doc.id)
        {
            let dirty = draft.with_untracked(|t| saved_text.with_untracked(|saved| t != saved))
                || !format_encoding.get_untracked().is_empty()
                || !format_ending.get_untracked().is_empty()
                || !format_bom.get_untracked().is_empty();
            if !dirty {
                editor.set(None);
                reset_draft(String::new());
                can_restore.set(false);
                spawn_local(async move {
                    let _ =
                        invoke::<()>("close_document", serde_json::json!({"documentId":doc.id}))
                            .await;
                });
            }
        }
    };
    let preview = move || {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        plan.set(None);
        let ids = selected.get_untracked().into_iter().collect::<Vec<_>>();
        spawn_local(async move {
            let operation = file_operation.get_untracked();
            let (command, args) = match operation.as_str() {
                "mirror_left_to_right"
                | "mirror_right_to_left"
                | "mirror_tree_left_to_right"
                | "mirror_tree_right_to_left" => (
                    if operation.contains("_tree_") {
                        "preview_mirror_tree"
                    } else {
                        "preview_mirror_files"
                    },
                    serde_json::json!({"session":c.session,"sourceSide":if operation.ends_with("left_to_right") {Side::Left} else {Side::Right}}),
                ),
                "rename_left" | "rename_right" => {
                    if ids.len() != 1 {
                        error.set(
                            tr(
                                english.get_untracked(),
                                "重命名需要恰好选择一个文件。",
                                "Select exactly one file to rename.",
                            )
                            .into(),
                        );
                        busy.set(false);
                        return;
                    }
                    (
                        "preview_rename",
                        serde_json::json!({"session":c.session,"rowId":ids[0],"targetSide":if operation=="rename_left"{Side::Left}else{Side::Right},"newName":rename_name.get_untracked()}),
                    )
                }
                "delete_left" => (
                    "preview_delete",
                    serde_json::json!({"session":c.session,"rowIds":ids,"targetSide":Side::Left}),
                ),
                "delete_right" => (
                    "preview_delete",
                    serde_json::json!({"session":c.session,"rowIds":ids,"targetSide":Side::Right}),
                ),
                _ => (
                    "preview_sync",
                    serde_json::json!({"session":c.session,"rowIds":ids,"sourceSide":if reverse_sync.get_untracked(){Side::Right}else{Side::Left}}),
                ),
            };
            match invoke::<SyncPlan>(command, args).await {
                Ok(p) => plan.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let complete_file = move |reply: FileApplyReply| {
        update_editor_locations(reply.locations, reply.stale_documents);
        let result = reply.result;
        if !reply.warnings.is_empty() {
            error.set(reply.warnings.join(" · "));
        }
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
        file_operation.set("copy".into());
        selected.set(BTreeSet::new());
        diff.set(None);

        plan.set(None);
    };
    let apply = move || {
        let Some(p) = plan.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match operations::run::<FileApplyReply>(
                "apply_sync",
                serde_json::json!({"planId":p.id}),
                operation_progress,
            )
            .await
            {
                Ok(reply) => complete_file(reply),
                Err(e) => {
                    error.set(e);
                    reset();
                }
            }
            plan.set(None);
            busy.set(false);
        });
    };

    let quick_copy = move |id: String, source: Side| {
        if busy.get_untracked() {
            return;
        }
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        message.set(String::new());
        plan.set(None);
        selected.set(BTreeSet::new());
        spawn_local(async move {
            let mut attempted = false;
            let result=async{
                let p=invoke::<SyncPlan>("preview_sync",serde_json::json!({"session":c.session,"rowIds":[id.clone()],"sourceSide":source})).await?;
                if p.session!=c.session ||p.source_side!=source ||p.policy!=FilePlanPolicy::SelectedCopy ||p.actions.len()!=1 ||p.actions[0].row_id!=id ||p.actions[0].operation!=FileOperation::Copy{return Err("Unexpected quick-copy plan".into());}
                attempted=true;
                let reply=operations::run::<FileApplyReply>("apply_sync",serde_json::json!({"planId":p.id}),operation_progress).await?;
                update_editor_locations(reply.locations,reply.stale_documents);
                if !reply.warnings.is_empty(){error.set(reply.warnings.join(" · "));}
                let action=reply.result.actions.first();
                let completed=reply.result.actions.len()==1&&action.is_some_and(|a|a.state=="completed"&&a.error.is_none());
                let failure=action.and_then(|a|a.error.clone()).unwrap_or_else(||tr(english.get_untracked(),"合并未完成；请检查操作历史。","Merge did not complete; inspect operation history.").into());
                let refreshed=invoke::<OpenedComparison>("refresh_compared_file",serde_json::json!({"session":c.session,"rowId":id})).await?;
                let page=invoke::<FolderPage>("query_folder",serde_json::json!({"session":c.session,"query":build_query(),"offset":offset.get_untracked(),"expanded":if tree_mode.get_untracked(){Some(expanded_dirs.get_untracked().into_iter().collect::<Vec<_>>())}else{None},"expandAll":expand_all.get_untracked()})).await?;
                comparison.set(Some(refreshed));outline.set(page.outline);rows.set(page.page.rows);visible_total.set(page.page.total);breadcrumbs.set(page.breadcrumbs);focused_row.set(None);
                if completed{message.set(format!("{} · {}",tr(english.get_untracked(),"文件已合并，来源文件保留","File merged; source retained"),reply.result.journal));}else{error.set(failure);}
                Ok::<(),String>(())
            }.await;
            if let Err(e) = result {
                if attempted {
                    reset();
                    error.set(format!(
                        "{} {e}",
                        tr(
                            english.get_untracked(),
                            "合并或刷新未完成，请重新比较。",
                            "Merge or refresh did not complete; compare again."
                        )
                    ));
                } else {
                    error.set(e);
                }
            }
            busy.set(false);
        });
    };

    let editor_dirty = move || {
        draft.with_untracked(|t| saved_text.with_untracked(|saved| t != saved))
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
                    reset_draft(document.text.clone());
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
                    change_draft(result.document.text.clone());
                    saved_text.set(clear_contracts::edit_history::normalize(
                        &result.document.text,
                    ));
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
                    reset_draft(result.document.text.clone());
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
    let complete_recovery = move |reply: RecoveryApplyReply| {
        update_editor_locations(reply.locations, reply.stale_documents);
        let result = reply.result;
        if !reply.warnings.is_empty() {
            error.set(reply.warnings.join(" · "));
        }
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
        reset();
        message.set(format!(
            "{} {}/{} · {}",
            tr(english.get_untracked(), "恢复完成", "Recovered"),
            completed,
            result.actions.len(),
            result.journal
        ));

        recovery.set(None);
    };
    let apply_recovery = move || {
        let Some(p) = recovery.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match operations::run::<RecoveryApplyReply>(
                "apply_sync_recovery",
                serde_json::json!({"planId":p.id}),
                operation_progress,
            )
            .await
            {
                Ok(reply) => complete_recovery(reply),
                Err(e) => {
                    error.set(e);
                    reset();
                }
            }
            recovery.set(None);
            busy.set(false);
        });
    };
    let preview_restart = move |id: String| {
        busy.set(true);
        error.set(String::new());
        restart.set(None);
        recovery.set(None);
        restart_offset.set(0);
        spawn_local(async move {
            match invoke::<ResumePreview>(
                "preview_sync_resume",
                serde_json::json!({"historyId":id}),
            )
            .await
            {
                Ok(p) => restart.set(Some(p)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let page_restart = move |next: bool| {
        let Some(p) = restart.get_untracked() else {
            return;
        };
        let offset = if next {
            restart_offset.get_untracked().saturating_add(500)
        } else {
            restart_offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<Vec<ResumeAction>>(
                "resume_page",
                serde_json::json!({"planId":p.id,"offset":offset}),
            )
            .await
            {
                Ok(rows) => {
                    restart.update(|value| {
                        if let Some(v) = value
                            && v.id == p.id
                        {
                            v.actions = rows;
                        }
                    });
                    restart_offset.set(offset);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let apply_restart = move || {
        let Some(p) = restart.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match operations::run::<FileApplyReply>(
                "apply_sync_resume",
                serde_json::json!({"planId":p.id}),
                operation_progress,
            )
            .await
            {
                Ok(reply) => complete_file(reply),
                Err(e) => {
                    error.set(e);
                    reset();
                }
            }
            restart.set(None);
            busy.set(false);
        });
    };
    let complete_bidi = move |reply: BidirectionalApplyReply| {
        update_editor_locations(reply.locations, reply.stale_documents);
        reset();
        if let Some(e) = &reply.result.error {
            error.set(e.clone());
        } else if !reply.warnings.is_empty() {
            error.set(reply.warnings.join(" · "));
        }
        message.set(format!(
            "{} · {}",
            tr(
                english.get_untracked(),
                "双向同步结果",
                "Bidirectional result"
            ),
            reply.result.journal
        ));
        bidi.baseline.set(Some(reply.result.baseline_info.clone()));
        bidi.run.set(Some(reply.result));
    };
    let recover_bidi = move |reply: BidirectionalRecoveryReply| {
        update_editor_locations(reply.locations, reply.stale_documents);
        reset();
        if let Some(e) = &reply.result.error {
            error.set(e.clone());
        } else if !reply.warnings.is_empty() {
            error.set(reply.warnings.join(" · "));
        }
        bidi.baseline.set(None);
        message.set(format!(
            "{} · {}",
            tr(
                english.get_untracked(),
                "协调回退结果",
                "Coordinated rollback result"
            ),
            reply.result.journal
        ));
        bidi.undo.set(Some(reply.result));
    };
    let reconnect_operation = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match invoke::<Option<TaskHandle>>("active_operation", serde_json::json!({})).await {
                Ok(Some(handle)) => {
                    busy.set(true);
                    match operations::follow(handle, operation_progress).await {
                        Ok(OperationOutcome::File(reply)) => complete_file(reply),
                        Ok(OperationOutcome::Recovery(reply)) => complete_recovery(reply),
                        Ok(OperationOutcome::Bidirectional(reply)) => complete_bidi(reply),
                        Ok(OperationOutcome::BidirectionalRecovery(reply)) => recover_bidi(reply),
                        Ok(OperationOutcome::Error(e)) | Err(e) => {
                            error.set(e);
                            reset();
                            recovery.set(None);
                        }
                    }
                    busy.set(false);
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    reconnect_operation();
    let refresh_library = move || {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<Vec<LibraryEntry>>("list_sessions", serde_json::json!({})).await {
                Ok(entries) => library.set(Some(entries)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let save_library = move |update: bool| {
        let (id, revision) = if update {
            let Some((id, revision)) = active_library.get_untracked() else {
                return;
            };
            (Some(id), Some(revision))
        } else {
            (None, None)
        };
        let args = serde_json::json!({"id":id,"revision":revision,"name":session_name.get_untracked(),"session":current_session()});
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<LibraryEntry>("save_library_session", args).await {
                Ok(entry) => {
                    project_store.accept(&entry);
                    message.set(
                        tr(
                            english.get_untracked(),
                            "项目配置已保存。",
                            "Project settings saved.",
                        )
                        .into(),
                    );
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let open_library = move |entry: LibraryEntry| {
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "先保存或重置编辑内容，再切换项目。",
                    "Save or reset the editor before switching projects.",
                )
                .into(),
            );
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            if let Err(e) = project_store.persist(current_session()).await {
                error.set(e);
                busy.set(false);
                return;
            }
            match invoke::<LibraryEntry>("activate_project", serde_json::json!({"id":entry.id}))
                .await
            {
                Ok(entry) => {
                    reset();
                    apply_session(entry.session.clone());
                    project_store.accept(&entry);
                    library.set(None);
                    if let Some(d) = editor.get_untracked() {
                        let _ =
                            invoke::<()>("close_document", serde_json::json!({"documentId":d.id}))
                                .await;
                    }
                    editor.set(None);
                    reset_draft(String::new());
                    can_restore.set(false);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let close_projects = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match project_store.persist(current_session()).await {
                Ok(()) => library.set(None),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let new_project = move || {
        if busy.get_untracked() {
            return;
        }
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "请先保存或重置编辑内容，再新建项目。",
                    "Save or reset edits before creating a project.",
                )
                .into(),
            );
            return;
        }
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match project_store.persist(current_session()).await {
                Ok(()) => {
                    reset();
                    active_library.set(None);
                    session_name.set(String::new());
                    project_saved.set(String::new());
                    left.set(String::new());
                    right.set(String::new());
                    exclusions.set("-./.git/;-node_modules/;-__pycache__/;-.DS_Store".into());
                    search.set(String::new());
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let archive_library = move |entry: LibraryEntry| {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<()>(
                "archive_session",
                serde_json::json!({"id":entry.id,"revision":entry.revision}),
            )
            .await
            {
                Ok(()) => {
                    library.update(|entries| {
                        if let Some(entries) = entries {
                            entries.retain(|e| e.id != entry.id);
                        }
                    });
                    if active_library
                        .get_untracked()
                        .is_some_and(|(id, _)| id == entry.id)
                    {
                        active_library.set(None);
                    }
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let load_merge = move |result: MergeResult| {
        if editor_dirty() {
            error.set(
                tr(
                    english.get_untracked(),
                    "先保存或重置编辑内容。",
                    "Save or reset the editor first.",
                )
                .into(),
            );
            return;
        }
        let Some(document) = editor.get_untracked() else {
            return;
        };
        let Some(id) = result.id else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<Vec<String>>(
                "stage_merge_result",
                serde_json::json!({"documentId":document.id,"resultId":id,"text":result.text}),
            )
            .await
            {
                Ok(tokens) => {
                    merge_tokens.set(tokens);
                    change_draft(result.text);
                }
                Err(e) => error.set(e),
            }
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
        reset_draft(String::new());
        spawn_local(async move {
            let _ = invoke::<()>(
                "close_document",
                serde_json::json!({"documentId":document.id}),
            )
            .await;
        });
    };

    view! {
                <main class=move ||if comparison.get().is_some(){if diff.get().is_some(){"workspace compact-workspace has-comparison active-text"}else{"workspace compact-workspace has-comparison"}}else{if diff.get().is_some(){"workspace compact-workspace active-text"}else{"workspace compact-workspace"}} class:workbench-home=move ||home_visible.get() data-theme=move ||theme.get().value()>
                    <header class="topbar">
                        <div class="identity"><span class="mark" aria-hidden="true">"⇄"</span><strong>"Clear Compare"</strong><button class="quiet" disabled=move ||busy.get()||text_merge_dirty.get()||editor_dirty() on:click=move |_|{diff.set(None);git_visible.set(false);archive_visible.set(false);folder_merge_visible.set(false);merge_visible.set(false);if editor.get_untracked().is_some(){close_editor();}home_visible.set(true);}>{move ||tr(english.get(),"首页","Home")}</button><button class="quiet project-picker" disabled=move||busy.get()||text_merge_dirty.get() title=move||session_name.get() on:click=move|_|refresh_library()>{move||format!("{}：{}{} ▾",tr(english.get(),"项目","Project"),if active_library.get().is_some(){session_name.get()}else{tr(english.get(),"未保存项目","Unsaved project").into()},if project_dirty.get(){" •"}else{""})}</button><button class="quiet" disabled=move||busy.get()||text_merge_dirty.get() on:click=move|_|{if active_library.get_untracked().is_some(){save_library(true);}else{refresh_library();}}>{move||tr(english.get(),"保存项目","Save project")}</button><button class="quiet" disabled=move||busy.get()||text_merge_dirty.get() on:click=move|_|exclusions_visible.set(true)>{move||format!("{} {}",tr(english.get(),"排除","Exclude"),exclusions.get().split([';','\n']).filter(|r|!r.trim().is_empty()).count())}</button></div>
                        <div class="top-actions" inert=move ||text_merge_dirty.get()><details name="workspace-menu" class="app-menu"><summary>{move ||tr(english.get(),"工具","Tools")}</summary><div class="menu-body"><button class="quiet" aria-expanded=move ||help_visible.get() aria-controls="help-panel" on:click=move |_|help_visible.update(|v| *v = !*v)>{move ||tr(english.get(),"帮助 / 主题","Help / theme")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|{diff.set(None);git_visible.set(true);}>"Git"</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|{diff.set(None);archive_visible.set(true);}>{move ||tr(english.get(),"归档 / 快照","Archives / snapshots")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|folder_merge_visible.set(!folder_merge_visible.get_untracked())>{move ||tr(english.get(),"目录合并","Folder merge")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|refresh_library()>{move ||tr(english.get(),"项目管理","Projects")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|load_history()>{move ||tr(english.get(),"操作历史","Operation history")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|reconnect_operation()>{move ||tr(english.get(),"连接文件任务","Connect file task")}</button></div></details>{panels::bidirectional::panel(english,busy,error,comparison,operation_progress,bidi,complete_bidi,recover_bidi,reset)}<span class="local-label">{move || tr(english.get(),"本地处理 · 自动备份","Local processing · Verified backups")}</span>
                        <button class="quiet" disabled=move||busy.get() on:click=move |_| english.update(|v| *v = !*v)>{move || if english.get() { "中文" } else { "English" }}</button></div>
                    </header>
                    {panels::exclusions::panel(english,busy,exclusions_visible,exclusions,change_exclusions)}
                    {panels::help::panel(english,help_visible,theme,theme_error)}
                    {panels::sessions::panel(english,busy,panels::sessions::Signals{library,session_name,active_library,left,right,exclusions,dirty:project_dirty},refresh_library,save_library,open_library,archive_library,new_project,close_projects,reset,move||exclusions_visible.set(true),pick)}
                    <section class="path-section" aria-label="Comparison directories">
                        <div class="path-pair">
                            <div class="path-field"><label for="left">{move || tr(english.get(),"左侧 · 来源","Left · Source")}</label><div class="path-input"><input id="left" prop:value=move || left.get() disabled=move || busy.get() placeholder=move || tr(english.get(),"选择来源文件夹","Choose source directory") on:input=move |ev| {reset();left.set(event_target_value(&ev));}/><button class="browse" aria-label=move||tr(english.get(),"选择左侧文件夹","Choose source directory") disabled=move || busy.get() on:click=move |_| pick(true)>"…"</button></div></div>
                            <div class="path-direction" aria-hidden="true">"→"</div>
                            <div class="path-field"><label for="right">{move || tr(english.get(),"右侧 · 目标","Right · Destination")}</label><div class="path-input"><input id="right" prop:value=move || right.get() disabled=move || busy.get() placeholder=move || tr(english.get(),"选择目标文件夹","Choose destination directory") on:input=move |ev| {reset();right.set(event_target_value(&ev));}/><button class="browse" aria-label=move||tr(english.get(),"选择右侧文件夹","Choose destination directory") disabled=move || busy.get() on:click=move |_| pick(false)>"…"</button></div></div>
                        </div>
                        <button class="primary" disabled=move || busy.get() || left.get().trim().is_empty() || right.get().trim().is_empty() on:click=move |_| run_compare()>{move || tr(english.get(),"比较文件夹","Compare directories")}</button>
                        <details name="workspace-menu" class="comparison-settings"><summary>{move ||tr(english.get(),"比较设置","Compare settings")}</summary><div class="menu-body">                <div class="compare-options"><label class="check-label"><input type="checkbox" prop:checked=move || content.get() disabled=move || busy.get() on:change=move |ev| {reset();content.set(event_target_checked(&ev));}/>{move || tr(english.get(),"列表显示后完整校验内容（SHA-256，较慢）","Verify all contents after listing (SHA-256, slower)")}</label>

                        </div>
                        <p class="compare-hint">{move ||tr(english.get(),"先按文件大小列出结果；修改时间不参与比较。大小相同仍标为未验证内容，完整校验后确定是否一致。","List results using file size; modification times do not affect comparison. Equal sizes remain unverified until content verification.")}</p>
                        <div class="rules"><button class="quiet" disabled=move||busy.get() on:click=move|_|exclusions_visible.set(true)>{move||tr(english.get(),"管理项目排除规则…","Manage project exclusions…")}</button></div></div></details>
                    </section>
                    {panels::git::panel(english,busy,git_visible,error,diff)}
                    {panels::archives::panel(english,busy,archive_visible,error,diff)}
                    {panels::folder_merge::panel(english,busy,panels::folder_merge::Signals{visible:folder_merge_visible,preview:folder_merge_preview,left,right,exclusions,error,message})}
                    <Show when=move || !error.get().is_empty()><div class="notice error-notice" role="alert">{move || error.get()}</div></Show>
                    <Show when=move || !message.get().is_empty()><div class="notice" role="status">{move || message.get()}<br/>{move || tr(english.get(),"请重新比较以查看最新状态。备份与操作日志已保留。","Compare again to refresh. Backups and operation journal are retained.")}</div></Show>
                    <div class="workspace-progress" class:bottom-progress=move ||home_visible.get()||comparison.with(|c|c.is_some()) ||diff.with(|d|d.is_some()) ||editor.with(|d|d.is_some()) ||picture.with(|p|p.is_some()) ||binary.with(|b|b.is_some()) ||table.with(|t|t.is_some())>
                    {panels::tasks::panel(english,busy,error,operation_progress,reconnect_operation)}

                    <Show when=move || busy.get()><div class="progress" role="status"><span class="pulse"></span>{move || match compare_stage.get() {
                        1 => tr(english.get(),"正在快速扫描目录（按文件大小，忽略修改时间）…","Scanning directories by file size (ignoring modification time)…"),
                        2 => tr(english.get(),"列表已显示，正在校验文件内容；确认一致后自动隐藏…","List available; verifying contents and hiding confirmed equal files…"),
                        _ => tr(english.get(),"正在处理，请稍候…","Working, please wait…"),
                    }}<button class="quiet" disabled=move ||text_merge_dirty.get() on:click=move |_| {compare_cancelled.set(true);spawn_local(async move { let _ = invoke::<()>("cancel",serde_json::json!({})).await; });}>{move ||if compare_stage.get() == 2 {tr(english.get(),"停止校验，保留列表","Stop verification, keep list")} else {tr(english.get(),"取消任务","Cancel task")}}</button></div></Show>
                    </div>
                    <Show when=move ||home_visible.get()&&!git_visible.get()&&!archive_visible.get()&&!folder_merge_visible.get()&&diff.get().is_none()>{panels::workbench::panel(english,busy,error,move ||home_visible.set(false),refresh_library,move |entry|{home_visible.set(false);open_library(entry);})}</Show>
                    <div class="directory-host" style:display=move ||if home_visible.get(){"none"}else{""}><Show when=move || comparison.get().is_some() fallback=move || view! {
                        <section class="empty"><div class="empty-symbol" aria-hidden="true">"⇄"</div><h1>{move || tr(english.get(),"看清差异，再同步","See the difference. Then sync.")}</h1><p>{move || tr(english.get(),"选择两个独立的文件夹，比较内容，检查计划，然后复制你选择的文件。","Choose two separate directories, compare their contents, review a plan, then copy selected files.")}</p><div class="empty-notes"><span>{move || tr(english.get(),"目标独有文件保留","Destination-only files retained")}</span><span>{move || tr(english.get(),"覆盖前验证备份","Backups verified before overwrite")}</span></div></section>
                    }>
                        <div class="filterbar"><details name="workspace-menu" class="view-settings"><summary>{move ||tr(english.get(),"视图","View")}</summary><div class="menu-body">                <div class="outline-toolbar"><label><input type="checkbox" prop:checked=move ||tree_mode.get() disabled=move ||busy.get() on:change=move |ev|{tree_mode.set(event_target_checked(&ev));parent_id.set(None);focused_row.set(None);load_folder(0);}/>{move ||tr(english.get(),"目录树","Folder tree")}</label><button class="quiet" disabled=move ||busy.get() ||!tree_mode.get() on:click=move |_|{expand_all.set(true);expanded_dirs.set(BTreeSet::new());load_folder(0);}>{move ||tr(english.get(),"全部展开","Expand all")}</button><button class="quiet" disabled=move ||busy.get() ||!tree_mode.get() on:click=move |_|{expand_all.set(false);expanded_dirs.set(BTreeSet::new());load_folder(0);}>{move ||tr(english.get(),"全部折叠","Collapse all")}</button><label>{move ||tr(english.get(),"排序","Sort")}<select disabled=move ||busy.get() prop:value=move ||folder_sort.get() on:change=move |ev|{folder_sort.set(event_target_value(&ev));load_folder(0);}><option value="path">{move ||tr(english.get(),"名称","Name")}</option><option value="status">{move ||tr(english.get(),"状态","Status")}</option><option value="left_size">{move ||tr(english.get(),"左侧大小","Left size")}</option><option value="right_size">{move ||tr(english.get(),"右侧大小","Right size")}</option></select></label><span>{move ||tr(english.get(),"左右同行对应 · ? 表示内容未验证 · 文件夹状态汇总子项","Aligned pairs · ? means content unverified · folder status includes descendants")}</span></div>
        </div></details>                <details name="workspace-menu" class="comparison-tools"><summary>{move ||tr(english.get(),"规则 / 报告","Rules / reports")}</summary><div class="menu-body">
                        <div class="text-options"><button class="quiet" on:click=move |_|merge_visible.set(!merge_visible.get_untracked())>{move || tr(english.get(),"三方文本合并","Three-way text merge")}</button><span>{move || tr(english.get(),"文本比较规则（打开文件时应用）","Text rules (applied when opening a file)")}</span><label><input type="checkbox" prop:checked=move || ignore_case.get() on:change=move |ev|ignore_case.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略大小写","Ignore case")}</label><label><input type="checkbox" prop:checked=move || ignore_space.get() on:change=move |ev|ignore_space.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略空白","Ignore whitespace")}</label><label><input type="checkbox" prop:checked=move || ignore_endings.get() on:change=move |ev|ignore_endings.set(event_target_checked(&ev))/>{move || tr(english.get(),"忽略换行风格","Ignore line ending style")}</label></div>
                        {panels::reports::controls(english,busy,error,move ||comparison.with(|c|c.as_ref().map(|c|(ReportSource::Local,c.session.clone()))))}

                        </div></details>
        <input aria-label="Search comparison paths" disabled=move ||busy.get() placeholder=move || tr(english.get(),"搜索路径…","Search paths…") prop:value=move || search.get() on:input=move |ev| search.set(event_target_value(&ev)) on:keydown=move |ev|{if ev.key()=="Enter"{load_folder(0);}}/><button class="quiet" disabled=move ||busy.get() on:click=move |_|load_folder(0)>{move ||tr(english.get(),"搜索","Search")}</button><label class="check-label"><input type="checkbox" prop:checked=move || differences.get() disabled=move ||busy.get() on:change=move |ev|{differences.set(event_target_checked(&ev));if differences.get_untracked() && !comparison_verified.get_untracked(){run_compare();}else{load_folder(0);}}/>{move || tr(english.get(),"只看差异","Differences only")}</label><label class="check-label"><input type="checkbox" prop:checked=move ||reverse_sync.get() disabled=move ||busy.get() ||file_operation.get()!="copy" on:change=move |ev|{reverse_sync.set(event_target_checked(&ev));selected.set(BTreeSet::new());plan.set(None);}/>{move ||tr(english.get(),"右 → 左","Right → left")}</label><label class="check-label">{move ||tr(english.get(),"操作","Operation")}<select aria-label="File operation" disabled=move ||busy.get() prop:value=move ||file_operation.get() on:change=move |e|{file_operation.set(event_target_value(&e));rename_name.set(String::new());selected.set(BTreeSet::new());plan.set(None);}><option value="copy">{move ||tr(english.get(),"复制","Copy")}</option><option value="delete_left">{move ||tr(english.get(),"删除左侧文件","Delete left files")}</option><option value="delete_right">{move ||tr(english.get(),"删除右侧文件","Delete right files")}</option><option value="mirror_left_to_right">{move ||tr(english.get(),"文件镜像：左 → 右","File mirror: left → right")}</option><option value="mirror_right_to_left">{move ||tr(english.get(),"文件镜像：右 → 左","File mirror: right → left")}</option><option value="mirror_tree_left_to_right">{move ||tr(english.get(),"目录镜像：左 → 右","Tree mirror: left → right")}</option><option value="mirror_tree_right_to_left">{move ||tr(english.get(),"目录镜像：右 → 左","Tree mirror: right → left")}</option><option value="rename_left">{move ||tr(english.get(),"重命名左侧文件","Rename left file")}</option><option value="rename_right">{move ||tr(english.get(),"重命名右侧文件","Rename right file")}</option></select></label><Show when=move ||file_operation.get().starts_with("rename_")><label class="check-label">{move ||tr(english.get(),"新文件名","New file name")}<input aria-label="New file name" prop:value=move ||rename_name.get() disabled=move ||busy.get() on:input=move |e|{rename_name.set(event_target_value(&e));plan.set(None);}/></label></Show><span class="selection-count">{move || format!("{} {}",selected.get().len(),tr(english.get(),"个文件已选","files selected"))}</span><button class="primary" disabled=move || (!file_operation.get().starts_with("mirror_") && selected.get().is_empty()) || busy.get() on:click=move |_| preview()>{move || tr(english.get(),if file_operation.get().starts_with("mirror_") {"预览整个比较的镜像…"}else if file_operation.get()=="copy" {"预览复制 →"}else if file_operation.get().starts_with("rename_"){"预览重命名…"}else{"预览删除…"},if file_operation.get().starts_with("mirror_") {"Preview whole-comparison mirror…"}else if file_operation.get()=="copy" {"Preview copy →"}else if file_operation.get().starts_with("rename_"){"Preview rename…"}else{"Preview deletion…"})}</button></div>
                        <Show when=move ||file_operation.get().starts_with("mirror_")><p class="warnings">{move ||tr(english.get(),"镜像覆盖整个比较根目录，使用本次比较记录的排除规则，不受搜索、分页或勾选影响。目标独有普通文件会先备份再删除；目录镜像会创建缺失目录、移除清空后的多余目录，含排除项的目录保留。文件镜像保留空目录。需要内容校验。","Mirror mode covers the entire comparison roots under captured exclusions, regardless of search, pages or selection. Destination-only regular files are backed up and deleted. Tree mirror creates missing directories and removes emptied extras, preserving excluded-entry ancestors; file mirror retains empty directories. Requires content verification.")}</p></Show>
                        <Show when=move ||!tree_mode.get()>{panels::folders::controls(english,busy,panels::folders::Signals{parent_id,breadcrumbs,recursive,sort:folder_sort,descending},browse_folder,load_folder)}</Show>

                        {panels::directory::panel(panels::directory::Signals{english,busy,comparison,rows,outline,selected,focused:focused_row,operation:file_operation,reverse:reverse_sync},toggle_directory,open_text,open_editor,move |id,kind|match kind {"binary"=>open_binary(id,"0".into()),"table"=>open_table(id),_=>open_picture(id)},move ||plan.set(None),move |id,side,action,name|{
                            if busy.get_untracked(){return;}
                            if action=="quick_copy"{quick_copy(id,side);return;}
                            if action=="exclude"{exclude_entry(id);return;}
                            selected.set(BTreeSet::from([id]));plan.set(None);
                            match action {"copy"=>{file_operation.set("copy".into());reverse_sync.set(side==Side::Right);},"rename"=>{file_operation.set(if side==Side::Left{"rename_left"}else{"rename_right"}.into());rename_name.set(name);},_=>file_operation.set(if side==Side::Left{"delete_left"}else{"delete_right"}.into())}
                            preview();
                        })}

                        <Show when=move || comparison.with(|c|c.as_ref().is_some_and(|c|c.warnings.iter().any(|note|note!="Symbolic links are listed but not followed, compared or synchronized.")))><p class="warnings">{move || comparison.with(|c|c.as_ref().map(|c|c.warnings.iter().filter(|note|note.as_str()!="Symbolic links are listed but not followed, compared or synchronized.").cloned().collect::<Vec<_>>().join(" · ")))}</p></Show>
    <div class="results-status">                    <p class="comparison-basis" title=move ||tr(english.get(),"快速扫描只按文件大小判断；修改时间不参与比较。大小相同不代表内容一致，请使用完整内容校验。","Quick scans compare file sizes only and ignore modification times. Equal sizes do not prove equal contents; use full content verification.")>{move ||if comparison_verified.get() {
                            tr(english.get(),"内容比较 · SHA-256","Content comparison · SHA-256")
                        } else {
                            tr(english.get(),"大小比较 · 忽略时间 · 内容未校验","Size comparison · time ignored · content unverified")
                        }}</p>
                        <div class="summary-bar"><span class="stat">{move ||tr(english.get(),"统计","Summary")}</span>{move || comparison.get().map(|c| { let s=c.summary; view! {<span class="stat changed">{format!("{} {}",tr(english.get(),"修改","Changed"),s.changed)}</span><span class="stat left">{format!("{} {}",tr(english.get(),"仅左","Left only"),s.left_only)}</span><span class="stat right">{format!("{} {}",tr(english.get(),"仅右","Right only"),s.right_only)}</span><span class="stat same">{format!("{} {}",tr(english.get(),"一致","Same"),s.same)}</span><span class="stat">{format!("{} {} · {} {}",tr(english.get(),"未验证","Unverified"),s.unverified,tr(english.get(),"错误","Errors"),s.errors)}</span>} })}</div>                    <div class="pagination"><span>{move || format!("{}–{} / {}",offset.get()+usize::from(visible_total.get()>0),(offset.get()+500).min(visible_total.get()),visible_total.get())}</span><button class="quiet" disabled=move || offset.get()==0 || busy.get() on:click=move |_| paginate(false)>{move || tr(english.get(),"上一页","Previous")}</button><button class="quiet" disabled=move || busy.get() || (offset.get()+500>=visible_total.get()) on:click=move |_| paginate(true)>{move || tr(english.get(),"下一页","Next")}</button></div></div>
                    </Show></div>
                    {panels::text::panel(panels::text::Signals{english,busy,error,dirty:text_merge_dirty,context:text_context,diff},move |backup|{plan.set(None);comparison_verified.set(false);message.set(format!("{} {}",tr(english.get_untracked(),"合并已保存；原始内容备份：","Merge saved; original backup:"),backup));},move |d|{merge_left.set(d.lines.iter().filter(|l|l.old_line.is_some()).map(|l|l.text.clone()).collect::<String>());merge_right.set(d.lines.iter().filter(|l|l.new_line.is_some()).map(|l|l.right_text.clone().unwrap_or_else(||l.text.clone())).collect::<String>());merge_result.set(None);merge_visible.set(true);})}
                    {panels::picture::panel(english,busy,panels::picture::Signals{picture,picture_id,picture_tolerance,picture_ignore_alpha},open_picture)}
                    {panels::table::panel(english,busy,panels::table::Signals{table_visible, table, table_document, table_left_mode,table_right_mode,table_left_index,table_right_index, table_headers, table_align_names, table_key, table_tolerance, table_offset}, compare_table_document, paginate_table,table_catalog,close_table)}
                    {panels::binary::panel(english,busy,panels::binary::Signals{binary, binary_id, comparison, error}, open_binary)}
                    {panels::merge::panel(english,busy,panels::merge::Signals{merge_visible, merge_base, merge_left, merge_right, merge_result, editor, draft, saved_text, error},load_merge)}
                    {panels::editor::panel(english,busy,panels::editor::Signals{editor, draft, saved_text, text_history, merge_tokens, error, format_encoding, format_ending, format_bom, can_restore}, close_editor, save_editor, preview_restore,change_draft)}
                    {panels::resume::panel(english,busy,restart,restart_offset,page_restart,apply_restart)}
                    {panels::history::panel(english,busy,history,recovery,preview_recovery,apply_recovery,preview_restart)}
                    <Show when=move ||restore_preview.get().is_some()><div class="modal-backdrop"><section class="sync-dialog" role="dialog" aria-modal="true" aria-labelledby="restore-title"><h2 id="restore-title">{move ||tr(english.get(),"检查恢复计划","Review restore plan")}</h2><p>{move ||tr(english.get(),"恢复上次保存前的原始字节。恢复前的当前内容也会备份；文件若发生外部修改，恢复将被阻止。","Restore the original bytes from before the last save. Current contents are backed up first; external changes block restoration.")}</p><p class="backup-path">{move ||restore_preview.get().map(|p|format!("{} ← {} · {} bytes",p.target,p.backup,p.bytes))}</p><footer><button class="quiet" disabled=move ||busy.get() on:click=move |_|restore_preview.set(None)>{move ||tr(english.get(),"返回","Back")}</button><button class="primary" disabled=move ||busy.get() on:click=move |_|apply_restore()>{move ||tr(english.get(),"执行已预览的恢复","Execute reviewed restore")}</button></footer></section></div></Show>
                    {panels::file_plan::panel(english,busy,plan,comparison,apply)}

                    <footer class="statusbar"><span>{move || tr(english.get(),"UTF-8 / UTF-16 预览 · 不跟随符号链接 · 不自动删除","UTF-8 / UTF-16 preview · Links not followed · No automatic deletion")}</span><span>"Clear Compare · MIT"</span></footer>
                </main>
            }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
    install_workspace_menus();
    if let Some(placeholder) = document().get_element_by_id("boot-placeholder") {
        placeholder.remove();
    }
}
