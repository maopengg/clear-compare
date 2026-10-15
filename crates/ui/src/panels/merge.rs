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
    pub saved_text: RwSignal<String>,
    pub error: RwSignal<String>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    load: impl Fn(MergeResult) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        merge_visible,
        merge_base,
        merge_left,
        merge_right,
        merge_result,
        editor,
        draft,
        saved_text,
        error,
    } = signals;

    let history = RwSignal::new(edit_history::History::default());
    let selected = RwSignal::new(0usize);
    let composing = RwSignal::new(false);
    let output = NodeRef::<leptos::html::Textarea>::new();
    let blocks = Memo::new(move |_| {
        merge_result.with(|m| match m {
            Some(m) => match &m.conflict_token {
                Some(t) => merge_edit::blocks(&m.text, t),
                None => Ok(Vec::new()),
            },
            None => Ok(Vec::new()),
        })
    });
    Effect::new(move |_| {
        let count = blocks.with(|b| b.as_ref().map(|b| b.len()).unwrap_or_default());
        if selected.get_untracked() >= count {
            selected.set(count.saturating_sub(1));
        }
    });
    let active = move || {
        blocks.with(|b| {
            b.as_ref()
                .ok()
                .and_then(|b| b.get(selected.get()).or_else(|| b.last()).cloned())
        })
    };
    let change = move |text: String| {
        let mut r = Ok(());
        history.update(|h| r = h.record(&text));
        match r {
            Ok(()) => merge_result.update(|m| {
                if let Some(m) = m {
                    m.text = text;
                }
            }),
            Err(e) => {
                error.set(e);
                let previous = history.with(|h| h.current().to_string());
                if let Some(node) = output.get() {
                    node.set_value(&previous);
                }
                merge_result.update(|m| {
                    if let Some(m) = m {
                        m.text = previous;
                    }
                });
            }
        }
    };
    let undo = move |redo: bool| {
        let mut text = None;
        history.update(|h| {
            if if redo { h.redo() } else { h.undo() } {
                text = Some(h.current().to_string());
            }
        });
        if let Some(text) = text {
            merge_result.update(|m| {
                if let Some(m) = m {
                    m.text = text;
                }
            });
        }
    };
    let jump = move |position: usize| {
        let Some(block) = blocks.with(|b| b.as_ref().ok().and_then(|b| b.get(position).cloned()))
        else {
            return;
        };
        selected.set(position);
        let range =
            merge_result.with(|m| m.as_ref().map(|m| merge_edit::selection(&m.text, &block)));
        if let (Some(Ok((start, end))), Some(node)) = (range, output.get()) {
            let _ = node.focus();
            let _ = node.set_selection_range(start, end);
            node.set_scroll_top((block.line.saturating_sub(2) * 20) as i32);
        }
    };
    let choose = move |choice: merge_edit::Choice| {
        let Some(block) = active() else {
            return;
        };
        let next = merge_result.with(|m| {
            m.as_ref().and_then(|m| {
                m.conflict_token.as_ref().map(|t| {
                    merge_edit::resolve(&m.text, t, &m.conflict_versions, block.id, choice)
                })
            })
        });
        match next {
            Some(Ok(text)) => {
                error.set(String::new());
                change(text);
            }
            Some(Err(e)) => error.set(e),
            None => {}
        }
    };
    view! {
                <Show when=move || merge_visible.get()><section class="merge-panel"><header><strong>{move || tr(english.get(),"三方文本合并","Three-way text merge")}</strong><button class="quiet" on:click=move |_|merge_visible.set(false)>{move || tr(english.get(),"收起","Collapse")}</button></header><p>{move || tr(english.get(),"填写共同基础版本及左右版本。结果先保留在缓冲区；冲突用标记展示，可手动处理后载入已打开的编辑器。","Enter the common base and both versions. The result stays in a buffer; resolve conflict markers before loading it into an open editor.")}</p><div class="merge-inputs"><label>{move || tr(english.get(),"基础版本","Base")}<textarea disabled=move ||busy.get() aria-label="Merge base" spellcheck="false" prop:value=move ||merge_base.get() on:input=move |ev|{merge_base.set(event_target_value(&ev));merge_result.set(None);history.update(|h|h.reset(""));selected.set(0);}/></label><label>{move || tr(english.get(),"左侧版本","Left")}<textarea disabled=move ||busy.get() aria-label="Merge left" spellcheck="false" prop:value=move ||merge_left.get() on:input=move |ev|{merge_left.set(event_target_value(&ev));merge_result.set(None);history.update(|h|h.reset(""));selected.set(0);}/></label><label>{move || tr(english.get(),"右侧版本","Right")}<textarea disabled=move ||busy.get() aria-label="Merge right" spellcheck="false" prop:value=move ||merge_right.get() on:input=move |ev|{merge_right.set(event_target_value(&ev));merge_result.set(None);history.update(|h|h.reset(""));selected.set(0);}/></label></div><button class="primary" disabled=move ||busy.get() on:click=move |_|{busy.set(true);error.set(String::new());let args=serde_json::json!({"base":merge_base.get_untracked(),"left":merge_left.get_untracked(),"right":merge_right.get_untracked()});spawn_local(async move {match invoke::<MergeResult>("merge_buffers",args).await{Ok(value)=>{history.update(|h|h.reset(&value.text));selected.set(0);composing.set(false);merge_result.set(Some(value));},Err(e)=>error.set(e)}busy.set(false);});}>{move || tr(english.get(),"生成合并结果","Generate merge result")}</button><Show when=move ||merge_result.get().is_some()><p>{move ||merge_result.get().map(|m|format!("{}: {} · {}: {}",tr(english.get(),"原始冲突组","Original conflict groups"),m.conflicts,tr(english.get(),"剩余标记","Remaining markers"),m.conflict_token.as_ref().map(|t|m.text.matches(&clear_contracts::edit_history::conflict_tag(t)).count()).unwrap_or_default()))}</p><Show when=move ||blocks.with(|b|b.is_err())><p class="merge-marker-error" role="alert">{move ||blocks.get().err()}</p></Show>
        <div class="merge-conflict-toolbar">
        <button class="quiet" disabled=move ||busy.get() ||composing.get() ||active().is_none() ||selected.get()==0 on:click=move |_|jump(selected.get_untracked().saturating_sub(1))>{move ||tr(english.get(),"上一冲突","Previous conflict")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() ||active().is_none() on:click=move |_|{let count=blocks.with(|b|b.as_ref().map(|b|b.len()).unwrap_or_default());jump((selected.get_untracked()+1).min(count.saturating_sub(1)));}>{move ||tr(english.get(),"下一冲突","Next conflict")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() ||active().is_none() on:click=move |_|{let position=blocks.with(|b|b.as_ref().ok().and_then(|b|active().and_then(|a|b.iter().position(|b|b.id==a.id)))).unwrap_or_default();jump(position);}>{move ||tr(english.get(),"定位当前冲突","Locate current conflict")}</button>
        <span>{move ||active().map(|b|format!("#{} · {} {}",b.id+1,tr(english.get(),"行","line"),b.line))}</span>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() ||!history.with(|h|h.can_undo()) on:click=move |_|undo(false)>{move ||tr(english.get(),"撤销","Undo")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() ||!history.with(|h|h.can_redo()) on:click=move |_|undo(true)>{move ||tr(english.get(),"重做","Redo")}</button></div>
        <Show when=move ||active().is_some()><div class="merge-conflict-choices">
        <button class="quiet" disabled=move ||busy.get() ||composing.get() on:click=move |_|choose(merge_edit::Choice::Left)>{move ||tr(english.get(),"使用左侧原文","Use original left")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() on:click=move |_|choose(merge_edit::Choice::Base)>{move ||tr(english.get(),"使用基础原文","Use original base")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() on:click=move |_|choose(merge_edit::Choice::Right)>{move ||tr(english.get(),"使用右侧原文","Use original right")}</button>
        <button class="quiet" disabled=move ||busy.get() ||composing.get() on:click=move |_|choose(merge_edit::Choice::Both)>{move ||tr(english.get(),"左后右（原文拼接）","Left then right (exact)")}</button></div>
        <p>{move ||tr(english.get(),"选择会替换当前冲突组，保留原文换行；拼接不自动增加换行。已手动修改的冲突组请手动完成，或撤销后再选择。","Choices preserve original line endings; concatenation adds no newline. Finish manually edited conflicts by hand, or undo edits before choosing a version.")}</p></Show>
        <textarea node_ref=output disabled=move ||busy.get() class="text-editor merge-output" wrap="off" aria-label="Merge result" spellcheck="false" prop:value=move ||merge_result.with(|m|m.as_ref().map(|m|m.text.clone()).unwrap_or_default()) on:compositionstart=move |_|composing.set(true) on:compositionend=move |ev|{composing.set(false);change(event_target_value(&ev));} on:input=move |ev|{let text=event_target_value(&ev);if composing.get_untracked(){if text.len()<=edit_history::MAX_TEXT_BYTES{merge_result.update(|m|{if let Some(m)=m{m.text=text;}});}}else{change(text);}} on:keydown=move |ev|{if ev.is_composing() ||composing.get_untracked() ||ev.alt_key(){return;}
    if ev.ctrl_key() ||ev.meta_key(){match ev.key().to_lowercase().as_str(){"z"=>{ev.prevent_default();undo(ev.shift_key());},"y"=>{ev.prevent_default();undo(true);},_=>{}}}}/><button class="quiet" disabled=move ||busy.get() ||composing.get() ||editor.get().is_none() ||draft.with(|t|saved_text.with(|saved|t!=saved)) on:click=move |_|{if let Some(m)=merge_result.get_untracked(){load(m);}}>{move || tr(english.get(),"载入已打开的编辑器","Load into open editor")}</button></Show></section></Show>
            }
}
