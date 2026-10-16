//! A bounded, horizontally and vertically shared-scroll text workspace.
use crate::tr;
use clear_contracts::{DiffLine, TextDiff, text_view};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
fn cell(
    line: Option<&DiffLine>,
    right: bool,
    ending_changed: bool,
    focused: bool,
    english: bool,
) -> impl IntoView + use<> {
    let text = line
        .map(|l| {
            if right {
                l.right_text.as_deref().unwrap_or(&l.text)
            } else {
                &l.text
            }
        })
        .unwrap_or("");
    let (pieces, truncated) =
        text_view::preview(text, line.map(|l| l.changes.as_slice()).unwrap_or_default());
    let number = line.and_then(|l| if right { l.new_line } else { l.old_line });
    let css = if line.is_some_and(|l| l.tag != "equal") {
        if right {
            "diff-cell insert"
        } else {
            "diff-cell delete"
        }
    } else {
        "diff-cell"
    };
    let limited = line.is_some_and(|l| l.intraline_limited);
    view! {<div class=if focused{format!("{css} diff-focus")}else{css.to_owned()}><span class="line-number">{number.map(|n|n.to_string())}</span><pre>{pieces.into_iter().map(|(text,changed)|if changed{view!{<mark>{text}</mark>}.into_any()}else{view!{<span>{text}</span>}.into_any()}).collect_view()}</pre><small class="intraline-limit">{truncated.then_some(tr(english,"8192 字后截断","Truncated after 8192 chars"))}</small><small class="intraline-limit">{limited.then_some(tr(english,"整行提示","Whole-line hint"))}</small><span class=if ending_changed{"line-ending changed"}else{"line-ending"}>{line.map(|_|text_view::ending(text))}</span></div>}
}
pub fn panel(
    english: RwSignal<bool>,
    diff: RwSignal<Option<TextDiff>>,
    load: impl Fn(TextDiff) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let offset = RwSignal::new(0usize);
    let current = RwSignal::new(None::<usize>);
    let query = RwSignal::new(String::new());
    let hits = RwSignal::new(Vec::<usize>::new());
    let hit = RwSignal::new(None::<usize>);
    let notice = RwSignal::new(String::new());
    let scroll = NodeRef::<leptos::html::Div>::new();
    let other_scroll = NodeRef::<leptos::html::Div>::new();
    let width = RwSignal::new(0i32);
    let rows = Memo::new(move |_| {
        diff.with(|d| {
            d.as_ref()
                .map(|d| text_view::align(&d.lines))
                .unwrap_or_default()
        })
    });
    let hunks = Memo::new(move |_| rows.with(|r| text_view::hunks(r)));
    Effect::watch(
        move || diff.with(|d| d.as_ref().map(|d| d.id.clone())),
        move |_, _, _| {
            offset.set(0);
            current.set(None);
            hits.set(Vec::new());
            hit.set(None);
            notice.set(String::new());
            if diff.with_untracked(|d| d.is_some()) {
                let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                    if let Some(node) = document().get_element_by_id("text-comparison") {
                        node.scroll_into_view();
                    }
                });
                let _ = window().request_animation_frame(callback.unchecked_ref());
            }
        },
        true,
    );
    let jump = move |row: usize| {
        offset.set(row / 500 * 500);
        current.set(Some(row));
        if let Some(node) = scroll.get() {
            let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                let top = (row % 500).saturating_sub(3) as i32 * 22;
                node.set_scroll_top(top);
                if let Some(other) = other_scroll.get() {
                    other.set_scroll_top(top);
                }
            });
            let _ = window().request_animation_frame(callback.unchecked_ref());
        }
    };
    let navigate = move |forward: bool| {
        let targets = hunks.get_untracked();
        let focused = current.get_untracked();
        let target = if forward {
            targets
                .iter()
                .copied()
                .find(|n| focused.is_none_or(|c| *n > c))
        } else {
            targets
                .iter()
                .rev()
                .copied()
                .find(|n| focused.is_some_and(|c| *n < c))
        };
        if let Some(target) = target {
            jump(target);
        }
    };
    let search = move || {
        let result = diff.with(|d| {
            d.as_ref()
                .map(|d| rows.with(|r| text_view::matches(&d.lines, r, &query.get_untracked())))
        });
        if let Some(result) = result {
            match result {
                Ok(found) => {
                    hit.set(None);
                    if let Some(first) = found.first() {
                        jump(*first);
                        hit.set(Some(0));
                    }
                    hits.set(found);
                    notice.set(String::new());
                }
                Err(e) => notice.set(e),
            }
        }
    };

    let sync = move |right: bool| {
        let (from, to) = if right {
            (other_scroll.get(), scroll.get())
        } else {
            (scroll.get(), other_scroll.get())
        };
        if let (Some(from), Some(to)) = (from, to) {
            if from.scroll_top() != to.scroll_top() {
                to.set_scroll_top(from.scroll_top());
            }
            if from.scroll_left() != to.scroll_left() {
                to.set_scroll_left(from.scroll_left());
            }
        }
    };
    Effect::watch(
        move || (rows.with(|r| r.len()), offset.get(), english.get()),
        move |_, _, _| {
            width.set(0);
            let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                if let (Some(l), Some(r)) = (scroll.get(), other_scroll.get()) {
                    width.set(l.scroll_width().max(r.scroll_width()));
                }
            });
            let _ = window().request_animation_frame(callback.unchecked_ref());
        },
        true,
    );
    let pane = move |right: bool| {
        view! {
            <div node_ref=if right{other_scroll}else{scroll} class="diff-scroll shared-diff" on:scroll=move |_|sync(right)><div class="text-pane-content" style:min-width=move ||format!("{}px",width.get())><div class="diff-column-head">{move ||if right{tr(english.get(),"右侧 · 目标","Right · Destination")}else{tr(english.get(),"左侧 · 来源","Left · Source")}}</div>
            <For each=move ||diff.with(|d|d.as_ref().map(|d|rows.with(|r|r.iter().enumerate().skip(offset.get()).take(500).map(|(i,r)|(d.id.clone(),i,r.clone())).collect::<Vec<_>>())).unwrap_or_default()) key=|(id,i,_)|(id.clone(),*i) children=move |(_,i,row)|{
                let (line,ending_changed)=diff.with_untracked(|d|d.as_ref().map(|d|{let l=row.left.map(|n|&d.lines[n]);let r=row.right.map(|n|&d.lines[n]);(if right{r.cloned()}else{l.cloned()},text_view::significant_ending(l,r))})).unwrap_or_default();
                view!{<div>{move ||cell(line.as_ref(),right,ending_changed,current.get()==Some(i),english.get())}</div>}
            }/></div></div>
        }
    };
    view! {<Show when=move ||diff.with(|d|d.is_some())>{move ||untrack(move ||view!{<section id="text-comparison" class="diff-panel" tabindex="0" aria-label="Text comparison" on:keydown=move |ev|{if !ev.is_composing() && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() && ev.key()=="F7"{ev.prevent_default();navigate(!ev.shift_key());}}><header><button class="quiet" on:click=move |_|{if let Some(d)=diff.get_untracked(){load(d);}}>{move ||tr(english.get(),"载入合并面板","Load into merge panel")}</button><strong>{move ||diff.with(|d|d.as_ref().map(|d|d.path.clone()))}</strong><button class="quiet" on:click=move |_|diff.set(None)>{move ||tr(english.get(),"关闭预览","Close preview")}</button></header>
        <div class="text-navigation"><button class="quiet" disabled=move ||!hunks.with(|h|h.iter().any(|n|current.get().is_some_and(|c|*n<c))) on:click=move |_|navigate(false)>{move ||tr(english.get(),"上一差异","Previous change")}</button><button class="quiet" disabled=move ||!hunks.with(|h|h.iter().any(|n|current.get().is_none_or(|c|*n>c))) on:click=move |_|navigate(true)>{move ||tr(english.get(),"下一差异","Next change")}</button><span>{move ||format!("{} {}",hunks.with(|h|h.len()),tr(english.get(),"组差异","change groups"))}</span><input aria-label="Find literal text" prop:value=move ||query.get() on:input=move |ev|{query.set(event_target_value(&ev));hits.set(Vec::new());hit.set(None);} on:keydown=move |ev|{if ev.key()=="Enter" && !ev.is_composing(){search();}}/><button class="quiet" on:click=move |_|search()>{move ||tr(english.get(),"查找原文","Find literal text")}</button><button class="quiet" disabled=move ||hit.get().is_none_or(|i|i==0) on:click=move |_|{let i=hit.get_untracked().unwrap_or_default().saturating_sub(1);if let Some(row)=hits.with_untracked(|h|h.get(i).copied()){hit.set(Some(i));jump(row);}}>{move ||tr(english.get(),"上一匹配","Previous match")}</button><button class="quiet" disabled=move ||hit.get().is_none_or(|i|i+1>=hits.with(|h|h.len())) on:click=move |_|{let i=hit.get_untracked().unwrap_or_default()+1;if let Some(row)=hits.with_untracked(|h|h.get(i).copied()){hit.set(Some(i));jump(row);}}>{move ||tr(english.get(),"下一匹配","Next match")}</button><span>{move ||format!("{} / {}",hit.get().map(|i|i+1).unwrap_or_default(),hits.with(|h|h.len()))}</span></div>
        <p class="text-view-note">{move ||tr(english.get(),"修改组按行序对齐；下划线标记行内变化。LF / CRLF / CR 为行尾，∅ 表示无末尾换行。查找区分大小写，覆盖全部原文。","Changed groups align by line order; underlines mark intraline changes. LF / CRLF / CR identify endings; ∅ means no final newline. Search is case-sensitive across all original text.")}</p><Show when=move ||diff.with(|d|d.as_ref().is_some_and(|d|d.intraline_limited))><p class="text-view-note">{move ||tr(english.get(),"部分行内比较超出预算，已降级为整行提示；原始行差异仍保留。","Some intraline comparisons exceeded their budget and use whole-line hints; original line differences remain.")}</p></Show><Show when=move ||!notice.get().is_empty()><p role="alert">{move ||notice.get()}</p></Show>
        <div class="text-panes">{pane(false)}{pane(true)}</div><footer class="text-pagination"><button class="quiet" disabled=move ||offset.get()==0 on:click=move |_|{offset.update(|n|*n=n.saturating_sub(500));if let Some(n)=scroll.get(){n.set_scroll_top(0);}}>{move ||tr(english.get(),"上一页","Previous page")}</button><span>{move ||format!("{}–{} / {}",offset.get()+usize::from(rows.with(|r|!r.is_empty())),(offset.get()+500).min(rows.with(|r|r.len())),rows.with(|r|r.len()))}</span><button class="quiet" disabled=move ||{offset.get()+500>=rows.with(|r|r.len())} on:click=move |_|{offset.update(|n|*n+=500);if let Some(n)=scroll.get(){n.set_scroll_top(0);}}>{move ||tr(english.get(),"下一页","Next page")}</button></footer>
    </section>})}</Show>}
}
