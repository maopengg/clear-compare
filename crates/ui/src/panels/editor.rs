//! Editor workspace panel; native operations stay behind typed IPC.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub editor: RwSignal<Option<TextDocument>>,
    pub draft: RwSignal<String>,
    pub saved_text: RwSignal<String>,
    pub text_history: RwSignal<clear_contracts::edit_history::History>,
    pub merge_tokens: RwSignal<Vec<String>>,
    pub error: RwSignal<String>,
    pub format_encoding: RwSignal<String>,
    pub format_ending: RwSignal<String>,
    pub format_bom: RwSignal<String>,
    pub can_restore: RwSignal<bool>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    close_editor: impl Fn() + Copy + Send + Sync + 'static,
    save_editor: impl Fn() + Copy + Send + Sync + 'static,
    preview_restore: impl Fn() + Copy + Send + Sync + 'static,
    change: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        editor,
        draft,
        saved_text,
        text_history,
        merge_tokens,
        error,
        format_encoding,
        format_ending,
        format_bom,
        can_restore,
    } = signals;
    let dirty = move || {
        draft.with(|t| saved_text.with(|saved| t != saved))
            || !format_encoding.get().is_empty()
            || !format_ending.get().is_empty()
            || !format_bom.get().is_empty()
    };
    let composing = RwSignal::new(false);
    let undo = move |redo: bool| {
        let mut next = None;
        text_history.update(|h| {
            if if redo { h.redo() } else { h.undo() } {
                next = Some(h.current().to_string());
            }
        });
        if let Some(text) = next {
            draft.set(text);
        }
    };
    let unresolved = move || {
        draft.with(|text| {
            merge_tokens.with(|tokens| {
                clear_contracts::edit_history::has_any_conflict(text, tokens)
                    || (!tokens.is_empty()
                        && clear_contracts::edit_history::generic_markers(text)
                            > saved_text.with(|saved| {
                                clear_contracts::edit_history::generic_markers(saved)
                            }))
            })
        })
    };
    view! {
        <Show when=move || editor.get().is_some()>
            <section class="editor-panel">
                <header><strong>{move ||editor.with(|e|e.as_ref().map(|d|format!("{} · {} · {}",d.absolute_path,match d.encoding{Encoding::Utf8=>"UTF-8",Encoding::Utf16Le=>"UTF-16 LE",Encoding::Utf16Be=>"UTF-16 BE"},match d.line_ending{LineEnding::Lf=>"LF",LineEnding::CrLf=>"CRLF",LineEnding::Cr=>"CR",LineEnding::Mixed=>"Mixed"})))}</strong><span>{move ||if dirty(){tr(english.get(),"未保存","Unsaved")}else{tr(english.get(),"已保存","Saved")}}</span></header>
                <div class="save-options">
                    <label>{move ||tr(english.get(),"编码","Encoding")}<select disabled=move ||busy.get() prop:value=move ||format_encoding.get() on:change=move |ev|format_encoding.set(event_target_value(&ev))><option value="">{move ||tr(english.get(),"保留原格式","Preserve original")}</option><option value="utf8">"UTF-8"</option><option value="utf16_le">"UTF-16 LE"</option><option value="utf16_be">"UTF-16 BE"</option></select></label>
                    <label>{move ||tr(english.get(),"换行","Line endings")}<select disabled=move ||busy.get() prop:value=move ||format_ending.get() on:change=move |ev|format_ending.set(event_target_value(&ev))><option value="">{move ||tr(english.get(),"保留原格式","Preserve original")}</option><option value="lf">"LF"</option><option value="crlf">"CRLF"</option><option value="cr">"CR"</option></select></label>
                    <label>"BOM"<select disabled=move ||busy.get() prop:value=move ||format_bom.get() on:change=move |ev|format_bom.set(event_target_value(&ev))><option value="">{move ||tr(english.get(),"保留（UTF-16 必须有）","Preserve (required for UTF-16)")}</option><option value="yes">{move ||tr(english.get(),"包含","Include")}</option><option value="no">{move ||tr(english.get(),"不包含（仅 UTF-8）","Exclude (UTF-8 only)")}</option></select></label>
                </div>
                <Show when=move ||editor.get().is_some_and(|d|d.line_ending==LineEnding::Mixed)><p class="warnings">{move ||tr(english.get(),"文件包含混合换行；保存时请明确选择 LF、CRLF 或 CR。","Mixed line endings: explicitly choose LF, CRLF or CR before saving.")}</p></Show>
                <div class="editor-tools"><button class="quiet" disabled=move ||busy.get() ||composing.get() ||!text_history.with(|h|h.can_undo()) on:click=move |_|undo(false)>{move ||tr(english.get(),"撤销文本","Undo text")}</button><button class="quiet" disabled=move ||busy.get() ||composing.get() ||!text_history.with(|h|h.can_redo()) on:click=move |_|undo(true)>{move ||tr(english.get(),"重做文本","Redo text")}</button><Show when=unresolved><span class="unresolved-note">{move ||tr(english.get(),"请先处理生成的冲突标记，才能保存。","Resolve generated conflict markers before saving.")}</span></Show></div>
                <textarea class="text-editor" aria-label="Text editor" spellcheck="false" disabled=move ||busy.get() prop:value=move ||draft.get() on:compositionstart=move |_|composing.set(true) on:compositionend=move |ev|{composing.set(false);change(event_target_value(&ev));} on:input=move |ev|{let text=event_target_value(&ev);if composing.get_untracked(){if text.len()<=clear_contracts::edit_history::MAX_TEXT_BYTES{draft.set(clear_contracts::edit_history::normalize(&text));}else{error.set("Editor text exceeds decoded limit".into());}}else{change(text);}} on:keydown=move |ev|{if ev.is_composing() ||composing.get_untracked() ||ev.alt_key(){return;}
                    if ev.ctrl_key() ||ev.meta_key(){match ev.key().to_lowercase().as_str(){"z"=>{ev.prevent_default();undo(ev.shift_key());},"y"=>{ev.prevent_default();undo(true);},_=>{}}}}/>
                <footer>
                    <button class="quiet" disabled=move ||busy.get() on:click=move |_|close_editor()>{move ||tr(english.get(),"关闭编辑","Close editor")}</button>
                    <button class="quiet" disabled=move ||busy.get() on:click=move |_|{change(saved_text.get_untracked());format_encoding.set(String::new());format_ending.set(String::new());format_bom.set(String::new());}>{move ||tr(english.get(),"重置内容和格式","Reset buffer and format")}</button>
                    <button class="quiet" disabled=move ||busy.get() || !can_restore.get() ||dirty() on:click=move |_|preview_restore()>{move ||tr(english.get(),"预览恢复上次保存","Preview undo last save")}</button>
                    <button class="primary" disabled=move ||busy.get() ||composing.get() ||unresolved() || !dirty() on:click=move |_|save_editor()>{move ||tr(english.get(),"备份并保存","Backup and save")}</button>
                </footer>
            </section>
        </Show>
    }
}
