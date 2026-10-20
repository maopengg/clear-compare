//! Cross-file replacement review; all file paths and changes come from native plans.
use super::*;
#[wasm_bindgen(inline_js = "
export function batchRefresh(id,expected,text){return window.ClearCode?.refresh(id,expected,text)===true;}
let anchor;
export function batchFocus(){anchor=document.activeElement;requestAnimationFrame(()=>document.querySelector('.source-replace-dialog button:not(:disabled)')?.focus());}
export function batchRestore(){if(anchor?.isConnected)anchor.focus();}
export function batchKey(event){if(event.key!=='Tab')return;const items=[...event.currentTarget.querySelectorAll('button:not(:disabled),input:not(:disabled),textarea:not(:disabled),select:not(:disabled)')].filter(e=>e.getClientRects().length);if(!items.length)return;const first=items[0],last=items.at(-1);if(event.shiftKey&&(document.activeElement===first||!event.currentTarget.contains(document.activeElement))){event.preventDefault();last.focus();}else if(!event.shiftKey&&(document.activeElement===last||!event.currentTarget.contains(document.activeElement))){event.preventDefault();first.focus();}}
let nextListener=0;const listeners=new Map();
export function batchListen(callback){const id=++nextListener,state={callback,stop:null};listeners.set(id,state);Promise.resolve(window.__TAURI__?.event?.listen?.('source-replace-progress',event=>state.callback?.(event.payload))||(()=>{})).then(stop=>{if(listeners.has(id))state.stop=stop;else stop();}).catch(()=>{});return id;}
export function batchUnlisten(id){const state=listeners.get(id);if(state){state.callback=null;state.stop?.();listeners.delete(id);}}
")]
extern "C" {
    #[wasm_bindgen(js_name=batchRefresh)]
    fn refresh(id: &str, expected: &str, text: &str) -> bool;
    #[wasm_bindgen(js_name=batchFocus)]
    fn focus();
    #[wasm_bindgen(js_name=batchRestore)]
    fn restore();
    #[wasm_bindgen(js_name=batchKey)]
    fn key(event: &JsValue);
    #[wasm_bindgen(js_name=batchListen)]
    fn listen(callback: &JsValue) -> f64;
    #[wasm_bindgen(js_name=batchUnlisten)]
    fn unlisten(id: f64);
}
#[derive(Clone, Copy)]
struct Review {
    s: Controller,
    preview: RwSignal<Option<SourceReplacePreview>>,
    selected: RwSignal<BTreeSet<String>>,
    patch: RwSignal<Option<GitPatch>>,
    file: RwSignal<String>,
    result: RwSignal<Option<SourceReplaceResult>>,
    consumed: RwSignal<bool>,
    offset: RwSignal<usize>,
    progress: RwSignal<Option<SourceReplaceProgress>>,
}
fn issue(en: bool, value: &str) -> &str {
    match value {
        "unsaved_buffer" => tr(
            en,
            "有未保存草稿，请先保存或关闭",
            "Unsaved buffer; save or close it first",
        ),
        "git_conflict" => tr(
            en,
            "冲突文件，请使用三方冲突编辑器",
            "Git conflict; use the conflict resolver",
        ),
        "read_only" => tr(en, "文件为只读", "Read-only file"),
        "mixed_line_endings" => tr(
            en,
            "混合换行，请明确选择转换格式后重新预览",
            "Mixed endings; choose a conversion policy and preview again",
        ),
        "unchanged" => tr(en, "替换后内容不变", "No content change"),
        "completed" => tr(en, "已完成", "Completed"),
        "cancelled" => tr(
            en,
            "已取消；已完成项保留",
            "Cancelled; completed files retained",
        ),
        "partial" => tr(en, "部分完成", "Partially completed"),
        "failed" | "failed_or_uncertain" => tr(
            en,
            "执行失败，请核对恢复记录",
            "Failed; inspect recovery records",
        ),
        "uncertain" => tr(
            en,
            "结果需核对，请检查文件和恢复记录",
            "Result needs inspection; check files and recovery records",
        ),
        _ => value,
    }
}
fn describe(en: bool, text: String) -> String {
    if en {
        return text;
    }
    for (native, localized) in [
        ("Replacement plan expired", "替换计划已失效，请重新预览。"),
        (
            "This file has no applicable replacement",
            "这个文件没有可执行的替换内容。",
        ),
        (
            "Replacement target changed after preview",
            "文件在预览后发生变化，请重新预览。",
        ),
        (
            "matching unsaved source buffer",
            "存在未保存草稿，请先保存或关闭，再重新预览。",
        ),
        (
            "Repository or file rules changed since replacement preview",
            "仓库状态或文件规则已改变，请重新预览。",
        ),
        (
            "Replacement scan was truncated",
            "搜索达到限制，请缩小范围后重新预览。",
        ),
        (
            "Replacement preview exceeded its time limit",
            "预览超时，请缩小搜索范围。",
        ),
        (
            "Replacement preview exceeds 64 MiB",
            "替换预览超过 64 MiB，请缩小搜索范围。",
        ),
        (
            "Replacement exceeds the text size limit",
            "替换后的文件超过文本大小限制。",
        ),
        (
            "Replacement exceeds the per-file match limit",
            "单文件匹配超过 20 万处，请缩小匹配范围。",
        ),
        (
            "Unclosed replacement capture reference",
            "正则替换的分组引用缺少右花括号。",
        ),
        (
            "Replacement refers to an unknown capture group",
            "正则替换引用了不存在的分组。",
        ),
        (
            "A matching file changed during replacement preview",
            "文件在预览期间发生变化，请重新预览。",
        ),
        (
            "Invalid replacement diff page",
            "差异页码无效，请重新预览。",
        ),
        (
            "Replacement requires content search",
            "请使用内容搜索；替换内容不能超过 65536 字节或含空字符，并须选择有效换行格式。",
        ),
        (
            "Select applicable replacement files without duplicates",
            "请选择可替换的文件，且不能重复选择。",
        ),
        (
            "Replacement file changed or could not be inspected after saving",
            "保存后文件已变化或无法检查，请核对文件和恢复记录。",
        ),
        (
            "Saved replacement digest differs from the preview",
            "保存内容与预览不一致，请核对文件和恢复记录。",
        ),
    ] {
        if text.contains(native) {
            return localized.into();
        }
    }
    text
}
impl Review {
    fn close(self) {
        if !self.s.c.busy.get_untracked() {
            self.preview.set(None);
            restore();
        }
    }
    fn patch(self, file: String, offset: usize) {
        if self.s.c.busy.get_untracked() || self.consumed.get_untracked() {
            return;
        }
        let Some(preview) = self.preview.get_untracked() else {
            return;
        };
        self.s.c.busy.set(true);
        self.s.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<GitPatch>(
                "workspace_source_replace_patch",
                serde_json::json!({"planId":preview.id,"fileId":file,"offset":offset}),
            )
            .await
            {
                Ok(patch) => {
                    self.file.set(file);
                    self.patch.set(Some(patch));
                }
                Err(error) => self
                    .s
                    .c
                    .error
                    .set(describe(self.s.english.get_untracked(), error)),
            }
            self.s.c.busy.set(false);
        });
    }
    fn apply(self) {
        if self.s.c.busy.get_untracked() || self.consumed.get_untracked() {
            return;
        }
        let Some(preview) = self.preview.get_untracked() else {
            return;
        };
        let selected = self.selected.get_untracked();
        if selected.is_empty() {
            return;
        }
        if self.s.tabs.with_untracked(|tabs| {
            tabs.iter().any(|tab| {
                tab.dirty()
                    && preview.files.iter().any(|file| {
                        selected.contains(&file.id)
                            && file.repository_id == tab.repository
                            && file.file_id == tab.file
                    })
            })
        }) {
            self.s.c.error.set(
                tr(
                    self.s.english.get_untracked(),
                    "所选文件中有未保存草稿，请先保存或关闭，再重新预览。",
                    "Selected files have unsaved buffers; save or close them and preview again.",
                )
                .into(),
            );
            return;
        }
        self.consumed.set(true);
        self.s.c.busy.set(true);
        self.s.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<SourceReplaceResult>(
                "workspace_source_replace_apply",
                serde_json::json!({"planId":preview.id,"selected":selected}),
            )
            .await
            {
                Ok(result) => {
                    for view in &result.updated_documents {
                        if let Some(tab) = self.s.tab(&view.document.id) {
                            if tab.dirty() {
                                tab.changed.set(true);
                                continue;
                            }
                            let text = normalize(&view.document.text);
                            if !refresh(&view.document.id, &tab.saved.get_untracked(), &text) {
                                tab.changed.set(true);
                                continue;
                            }
                            tab.draft.set(text.clone());
                            tab.saved.set(text);
                            tab.document.set(view.document.clone());
                            tab.options.set(SaveOptions::default());
                            tab.changed.set(false);
                            self.s.dirty(tab);
                        }
                    }
                    if !result.outcomes.is_empty() {
                        self.s.results.set(None);
                    }
                    self.result.set(Some(result));
                }
                Err(error) => self
                    .s
                    .c
                    .error
                    .set(describe(self.s.english.get_untracked(), error)),
            }
            self.s.c.busy.set(false);
            self.s.check();
        });
    }
}
fn files(r: Review) -> impl IntoView {
    let en = r.s.english;
    view! {<div class="source-replace-files"><For each=move ||r.preview.with(|preview|preview.as_ref().map(|preview|preview.files.iter().skip(r.offset.get()).take(200).cloned().collect::<Vec<_>>()).unwrap_or_default()) key=|file|file.id.clone() children=move |file|{
        let file=StoredValue::new(file); let check=file; let open=file;
        view! {<div class="source-replace-file"><input type="checkbox" aria-label=move ||file.with_value(|file|format!("{} {}",tr(en.get(),"选择","Select"),file.path)) disabled=move ||r.s.c.busy.get()||r.consumed.get()||file.with_value(|file|file.issue.is_some()) prop:checked=move ||file.with_value(|file|r.selected.with(|set|set.contains(&file.id))) on:change=move |event|{let selected=event_target_checked(&event);r.selected.update(|set|check.with_value(|file|{if selected{set.insert(file.id.clone());}else{set.remove(&file.id);}}));}/><button class="quiet" disabled=move ||r.s.c.busy.get()||r.consumed.get()||file.with_value(|file|file.issue.is_some()) on:click=move |_|open.with_value(|file|r.patch(file.id.clone(),0))><strong>{file.with_value(|file|file.path.clone())}</strong><small>{file.with_value(|file|format!("{} · {} {} · {} → {} B · {} / {}",file.repository,file.matches,tr(en.get(),"处匹配","matches"),file.before_bytes,file.after_bytes,match file.encoding {Encoding::Utf8=>"UTF-8",Encoding::Utf16Le=>"UTF-16 LE",Encoding::Utf16Be=>"UTF-16 BE"},match file.line_ending {LineEnding::Lf=>"LF",LineEnding::CrLf=>"CRLF",LineEnding::Cr=>"CR",LineEnding::Mixed=>tr(en.get(),"混合换行","Mixed endings")}))}</small></button><span>{move ||file.with_value(|file|{
            if let Some(code)=&file.issue{return issue(en.get(),code).to_string();}
            r.result.with(|result|result.as_ref().map(|result|result.outcomes.iter().find(|outcome|outcome.id==file.id).map(|outcome|issue(en.get(),&outcome.status).to_string()).unwrap_or_else(||tr(en.get(),"未执行","Not run").into())).unwrap_or_default())
        })}</span></div>}
    }/></div>}
}
pub(super) fn panel(
    s: Controller,
    query: RwSignal<SourceSearch>,
    scope: RwSignal<String>,
) -> impl IntoView {
    let replacement = RwSignal::new(String::new());
    let ending = RwSignal::new(String::new());
    let en = s.english;
    let r = Review {
        s,
        preview: RwSignal::new(None),
        selected: RwSignal::new(BTreeSet::new()),
        patch: RwSignal::new(None),
        file: RwSignal::new(String::new()),
        result: RwSignal::new(None),
        consumed: RwSignal::new(false),
        offset: RwSignal::new(0),
        progress: RwSignal::new(None),
    };
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |value| {
        if let Ok(progress) = serde_wasm_bindgen::from_value::<SourceReplaceProgress>(value)
            && r.preview.with_untracked(|preview| {
                preview
                    .as_ref()
                    .is_some_and(|preview| preview.id == progress.plan_id)
            })
        {
            r.progress.set(Some(progress));
        }
    })
    .into_js_value();
    let listener = listen(&callback);
    on_cleanup(move || unlisten(listener));
    let prepare = move || {
        if s.c.busy.get_untracked() {
            return;
        }
        let request = match search_request(s, query.get_untracked(), &scope.get_untracked()) {
            Ok(query) => SourceReplaceRequest {
                query,
                replacement: replacement.get_untracked(),
                line_ending: match ending.get_untracked().as_str() {
                    "lf" => Some(LineEnding::Lf),
                    "crlf" => Some(LineEnding::CrLf),
                    "cr" => Some(LineEnding::Cr),
                    _ => None,
                },
            },
            Err(error) => {
                s.c.error.set(error);
                return;
            }
        };
        s.c.busy.set(true);
        s.c.error.set(String::new());
        r.preview.set(None);
        r.patch.set(None);
        r.result.set(None);
        r.consumed.set(false);
        r.offset.set(0);
        r.progress.set(None);
        spawn_local(async move {
            match invoke::<SourceReplacePreview>(
                "workspace_source_replace_preview",
                serde_json::json!({"request":request}),
            )
            .await
            {
                Ok(preview) => {
                    r.selected.set(
                        preview
                            .files
                            .iter()
                            .filter(|file| file.issue.is_none())
                            .map(|file| file.id.clone())
                            .collect(),
                    );
                    r.preview.set(Some(preview));
                    focus();
                }
                Err(error) => s.c.error.set(describe(en.get_untracked(), error)),
            }
            s.c.busy.set(false);
        });
    };
    view! {
        <details class="source-batch-controls" on:keydown=move |event|{if(event.meta_key()||event.ctrl_key())&&event.key().eq_ignore_ascii_case("s"){event.prevent_default();event.stop_propagation();}}><summary>{move ||tr(en.get(),"批量替换","Replace across files")}</summary><div><textarea aria-label=move ||tr(en.get(),"批量替换内容","Batch replacement text") placeholder=move ||tr(en.get(),"替换为…（可留空删除匹配）","Replace with… (leave empty to delete matches)") prop:value=move ||replacement.get() on:input=move |event|replacement.set(event_target_value(&event))/><label>{move ||tr(en.get(),"换行格式","Line endings")}<select aria-label=move ||tr(en.get(),"批量替换换行格式","Batch replacement line endings") prop:value=move ||ending.get() on:change=move |event|ending.set(event_target_value(&event))><option value="">{move ||tr(en.get(),"保持原格式；混合换行需另选","Preserve; choose a policy for mixed endings")}</option><option value="lf">"LF"</option><option value="crlf">"CRLF"</option><option value="cr">"CR"</option></select></label><span>{move ||tr(en.get(),"使用上方搜索范围和规则；正则替换支持 $1、${name} 和 $$。","Uses the search scope and rules above. Regex replacement supports $1, ${name} and $$.")}</span><button class="quiet" disabled=move ||s.c.busy.get()||query.with(|query|query.mode!="content"||query.query.is_empty()) on:click=move |_|prepare()>{move ||tr(en.get(),"预览批量替换","Preview replacements")}</button></div></details>
        <Show when=move ||r.preview.with(|preview|preview.is_some())><div class="source-replace-backdrop"></div><section class="source-replace-dialog" role="dialog" aria-modal="true" aria-label=move ||tr(en.get(),"批量替换预览","Replacement preview") on:keydown=move |event|{event.stop_propagation();if event.key()=="Escape"{r.close();}else if(event.meta_key()||event.ctrl_key())&&event.key().eq_ignore_ascii_case("s"){event.prevent_default();}else{key(event.as_ref());}}>
            <header><strong>{move ||tr(en.get(),"批量替换预览","Replacement preview")}</strong><span>{move ||r.preview.with(|preview|preview.as_ref().map(|preview|format!("{} {} · {} {} · {} {}",preview.files.len(),tr(en.get(),"个命中文件","matching files"),preview.searched_files,tr(en.get(),"个已搜索文件","files searched"),preview.skipped_files,tr(en.get(),"个非文本/超限/无法读取文件已跳过","non-text/oversized/unreadable files skipped"))).unwrap_or_default())}</span><button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|r.close()>{move ||tr(en.get(),"关闭","Close")}</button></header>
            <div class="source-replace-summary"><code>{move ||r.preview.with(|preview|preview.as_ref().map(|preview|preview.request.query.query.clone()).unwrap_or_default())}</code><span>" → "</span><code>{move ||r.preview.with(|preview|preview.as_ref().map(|preview|preview.request.replacement.clone()).unwrap_or_default())}</code><span>{move ||tr(en.get(),"逐文件复查并备份；中途取消保留已完成项。","Each file is rechecked and backed up; cancellation retains completed files.")}</span></div>
            <Show when=move ||!s.c.error.get().is_empty()><p class="source-replace-error" role="status">{move ||s.c.error.get()}</p></Show>
            <Show when=move ||r.preview.with(|preview|preview.as_ref().is_some_and(|preview|!preview.warnings.is_empty()))><details><summary>{move ||tr(en.get(),"扫描提示","Scan warnings")}</summary><pre>{move ||r.preview.with(|preview|preview.as_ref().map(|preview|preview.warnings.join("\n")).unwrap_or_default())}</pre></details></Show>
            <div class="source-replace-body"><aside><div class="source-replace-selection"><button class="quiet" disabled=move ||s.c.busy.get()||r.consumed.get() on:click=move |_|r.selected.set(r.preview.with_untracked(|preview|preview.as_ref().map(|preview|preview.files.iter().filter(|file|file.issue.is_none()).map(|file|file.id.clone()).collect()).unwrap_or_default()))>{move ||tr(en.get(),"全选可替换文件","Select applicable files")}</button><button class="quiet" disabled=move ||s.c.busy.get()||r.consumed.get() on:click=move |_|r.selected.set(BTreeSet::new())>{move ||tr(en.get(),"清空选择","Clear selection")}</button></div>{files(r)}<footer><span>{move ||r.preview.with(|preview|preview.as_ref().map(|preview|format!("{}–{} / {}",r.offset.get()+usize::from(!preview.files.is_empty()),(r.offset.get()+200).min(preview.files.len()),preview.files.len())).unwrap_or_default())}</span><button class="quiet" disabled=move ||r.offset.get()==0 on:click=move |_|r.offset.update(|offset|*offset=offset.saturating_sub(200))>"‹"</button><button class="quiet" disabled=move ||r.preview.with(|preview|preview.as_ref().is_none_or(|preview|r.offset.get()+200>=preview.files.len())) on:click=move |_|r.offset.update(|offset|*offset+=200)>"›"</button></footer></aside><main><Show when=move ||r.patch.with(|patch|patch.is_some()) fallback=move ||view!{<p>{move ||tr(en.get(),"选择文件查看将要写入的差异。","Select a file to inspect its proposed changes.")}</p>}><strong>{move ||r.patch.with(|patch|patch.as_ref().map(|patch|patch.path.clone()).unwrap_or_default())}</strong><div class="source-replace-diff">{move ||r.patch.get().map(|patch|patch.text.lines().map(|line|{let class=if line.starts_with('+')&&!line.starts_with("+++"){"insert"}else if line.starts_with('-')&&!line.starts_with("---"){"delete"}else{""};view!{<pre class=class>{line.to_owned()}</pre>}}).collect_view())}</div><footer><span>{move ||r.patch.with(|patch|patch.as_ref().map(|patch|format!("{}–{} / {}",patch.offset+usize::from(patch.total_lines>0),(patch.offset+500).min(patch.total_lines),patch.total_lines)).unwrap_or_default())}</span><button class="quiet" disabled=move ||s.c.busy.get()||r.consumed.get()||r.patch.with(|patch|patch.as_ref().is_none_or(|patch|patch.offset==0)) on:click=move |_|r.patch(r.file.get_untracked(),r.patch.with_untracked(|patch|patch.as_ref().map(|patch|patch.offset.saturating_sub(500)).unwrap_or(0)))>"‹"</button><button class="quiet" disabled=move ||s.c.busy.get()||r.consumed.get()||r.patch.with(|patch|patch.as_ref().is_none_or(|patch|patch.next_offset.is_none())) on:click=move |_|if let Some(offset)=r.patch.with_untracked(|patch|patch.as_ref().and_then(|patch|patch.next_offset)){r.patch(r.file.get_untracked(),offset)}>"›"</button><span>{move ||r.patch.with(|patch|patch.as_ref().is_some_and(|patch|patch.long_lines)).then_some(tr(en.get(),"超长行仅预览前 8192 字符","Long lines preview the first 8192 characters"))}</span></footer></Show></main></div>
            <Show when=move ||r.result.with(|result|result.is_some())><div class="source-replace-result" role="status"><strong>{move ||r.result.with(|result|result.as_ref().map(|result|issue(en.get(),&result.status).to_string()).unwrap_or_default())}</strong><span>{move ||r.result.with(|result|result.as_ref().and_then(|result|result.issue.clone()).map(|text|describe(en.get(),text)).unwrap_or_default())}</span><code>{move ||r.result.with(|result|result.as_ref().map(|result|result.journal.clone()).unwrap_or_default())}</code></div></Show>
            <footer><span>{move ||r.selected.with(|selected|format!("{} {}",selected.len(),tr(en.get(),"个文件已选","files selected")))}</span><span class="source-replace-progress" role="status">{move ||r.progress.with(|progress|progress.as_ref().map(|progress|format!("{} {} / {} · {}",tr(en.get(),"已处理","Processed"),progress.processed,progress.total,progress.path)).unwrap_or_default())}</span><Show when=move ||s.c.busy.get()><button class="quiet" on:click=move |_|spawn_local(async{let _=invoke::<()>("cancel",serde_json::json!({})).await;})>{move ||tr(en.get(),"取消当前任务","Cancel current task")}</button></Show><button class="primary" disabled=move ||s.c.busy.get()||r.consumed.get()||r.selected.with(BTreeSet::is_empty) on:click=move |_|r.apply()>{move ||tr(en.get(),"替换所选文件","Replace selected files")}</button></footer>
        </section></Show>
    }
}
