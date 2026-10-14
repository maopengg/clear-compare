//! Merge workspace panel; native operations stay behind typed IPC.
use crate::{invoke, tr};
use clear_contracts::*;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, Copy)]
pub struct Signals {
    pub merge_visible: RwSignal<bool>,
    pub merge_base: RwSignal<String>,
    pub merge_left: RwSignal<String>,
    pub merge_right: RwSignal<String>,
    pub merge_result: RwSignal<Option<MergeResult>>,
    pub editor: RwSignal<Option<TextDocument>>,
    pub draft: RwSignal<String>,
    pub error: RwSignal<String>,
}
pub fn panel(english: RwSignal<bool>, busy: RwSignal<bool>, signals: Signals) -> impl IntoView {
    let Signals {
        merge_visible,
        merge_base,
        merge_left,
        merge_right,
        merge_result,
        editor,
        draft,
        error,
    } = signals;
    view! {
        <Show when=move || merge_visible.get()><section class="merge-panel"><header><strong>{move || tr(english.get(),"三方文本合并","Three-way text merge")}</strong><button class="quiet" on:click=move |_|merge_visible.set(false)>{move || tr(english.get(),"收起","Collapse")}</button></header><p>{move || tr(english.get(),"填写共同基础版本及左右版本。结果先保留在缓冲区；冲突用标记展示，可手动处理后载入已打开的编辑器。","Enter the common base and both versions. The result stays in a buffer; resolve conflict markers before loading it into an open editor.")}</p><div class="merge-inputs"><label>{move || tr(english.get(),"基础版本","Base")}<textarea aria-label="Merge base" spellcheck="false" prop:value=move ||merge_base.get() on:input=move |ev|{merge_base.set(event_target_value(&ev));merge_result.set(None);}/></label><label>{move || tr(english.get(),"左侧版本","Left")}<textarea aria-label="Merge left" spellcheck="false" prop:value=move ||merge_left.get() on:input=move |ev|{merge_left.set(event_target_value(&ev));merge_result.set(None);}/></label><label>{move || tr(english.get(),"右侧版本","Right")}<textarea aria-label="Merge right" spellcheck="false" prop:value=move ||merge_right.get() on:input=move |ev|{merge_right.set(event_target_value(&ev));merge_result.set(None);}/></label></div><button class="primary" disabled=move ||busy.get() on:click=move |_|{busy.set(true);error.set(String::new());let args=serde_json::json!({"base":merge_base.get_untracked(),"left":merge_left.get_untracked(),"right":merge_right.get_untracked()});spawn_local(async move {match invoke::<MergeResult>("merge_buffers",args).await{Ok(value)=>merge_result.set(Some(value)),Err(e)=>error.set(e)}busy.set(false);});}>{move || tr(english.get(),"生成合并结果","Generate merge result")}</button><Show when=move ||merge_result.get().is_some()><p>{move ||merge_result.get().map(|m|format!("{}: {}",tr(english.get(),"冲突组数","Conflict groups"),m.conflicts))}</p><textarea class="text-editor" aria-label="Merge result" spellcheck="false" prop:value=move ||merge_result.get().map(|m|m.text).unwrap_or_default() on:input=move |ev|merge_result.update(|m|{if let Some(m)=m{m.text=event_target_value(&ev);}})/><button class="quiet" disabled=move ||busy.get() ||editor.get().is_none() ||editor.get().is_some_and(|d|d.text!=draft.get()) on:click=move |_|{if let Some(m)=merge_result.get_untracked(){draft.set(m.text);}}>{move || tr(english.get(),"载入已打开的编辑器","Load into open editor")}</button></Show></section></Show>
    }
}
