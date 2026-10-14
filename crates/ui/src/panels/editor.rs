//! Editor workspace panel; native operations stay behind typed IPC.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub editor: RwSignal<Option<TextDocument>>,
    pub draft: RwSignal<String>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    close_editor: impl Fn() + Copy + Send + Sync + 'static,
    save_editor: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals { editor, draft } = signals;
    view! {
        <Show when=move || editor.get().is_some()><section class="editor-panel"><header><strong>{move || editor.get().map(|d|format!("{} · {:?} · {:?}",d.absolute_path,d.encoding,d.line_ending))}</strong><span>{move || editor.get().map(|d|if d.text==draft.get(){tr(english.get(),"已保存","Saved")}else{tr(english.get(),"未保存","Unsaved")})}</span></header><textarea class="text-editor" aria-label="Text editor" spellcheck="false" disabled=move || busy.get() prop:value=move || draft.get() on:input=move |ev|draft.set(event_target_value(&ev))/><footer><button class="quiet" disabled=move || busy.get() on:click=move |_|close_editor()>{move || tr(english.get(),"关闭编辑","Close editor")}</button><button class="quiet" disabled=move || busy.get() on:click=move |_|{if let Some(d)=editor.get_untracked(){draft.set(d.text);}}>{move || tr(english.get(),"重置内容","Reset buffer")}</button><button class="primary" disabled=move || busy.get() || editor.get().is_none_or(|d|d.text==draft.get()) on:click=move |_|save_editor()>{move || tr(english.get(),"备份并保存","Backup and save")}</button></footer></section></Show>
    }
}
