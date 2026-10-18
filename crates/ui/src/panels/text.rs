//! A bounded, horizontally and vertically shared-scroll text workspace.
use super::text_merge::{Context, Controller};
use crate::tr;
use clear_contracts::{DiffLine, Side, TextDiff, text_view};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
pub(super) fn cell(
    line: Option<&DiffLine>,
    right: bool,
    ending_changed: bool,
    show_endings: bool,
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
    view! {<div class=if focused{format!("{css} diff-focus")}else{css.to_owned()}><span class="line-number">{number.map(|n|n.to_string())}</span><pre>{pieces.into_iter().map(|(text,changed)|if changed{view!{<mark>{text}</mark>}.into_any()}else{view!{<span>{text}</span>}.into_any()}).collect_view()}</pre><small class="intraline-limit">{truncated.then_some(tr(english,"8192 字后截断","Truncated after 8192 chars"))}</small><small class="intraline-limit">{limited.then_some(tr(english,"整行提示","Whole-line hint"))}</small>{(line.is_some() && (show_endings || ending_changed)).then(||view!{<span class=if ending_changed{"line-ending changed"}else{"line-ending"}>{text_view::ending(text)}</span>})}</div>}
}
#[derive(Clone, Copy)]
pub struct Signals {
    pub english: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub dirty: RwSignal<bool>,
    pub context: RwSignal<Option<Context>>,
    pub diff: RwSignal<Option<TextDiff>>,
}
pub fn panel(
    s: Signals,
    saved: impl Fn(String) + Copy + Send + Sync + 'static,
    load: impl Fn(TextDiff) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let english = s.english;
    let original_diff = s.diff;
    let merge = Controller::new(s.diff, s.context, s.english, s.busy, s.error, s.dirty);
    let preview = Memo::new(move |_| merge.preview());
    let diff = Memo::new(move |_| preview.with(|p| p.as_ref().map(|p| p.diff.clone())));
    let show_endings = RwSignal::new(false);
    let offset = RwSignal::new(0usize);
    let current = RwSignal::new(None::<usize>);
    let query = RwSignal::new(String::new());
    let hits = RwSignal::new(Vec::<usize>::new());
    let hit = RwSignal::new(None::<usize>);
    let notice = RwSignal::new(String::new());
    let scroll = NodeRef::<leptos::html::Div>::new();
    let other_scroll = NodeRef::<leptos::html::Div>::new();
    let width = RwSignal::new(0i32);
    let gutter_top = RwSignal::new(0i32);
    let rows = Memo::new(move |_| {
        diff.with(|d| {
            d.as_ref()
                .map(|d| text_view::align(&d.lines))
                .unwrap_or_default()
        })
    });
    let hunks = Memo::new(move |_| rows.with(|r| text_view::hunks(r)));
    Effect::watch(
        move || original_diff.with(|d| d.as_ref().map(|d| d.id.clone())),
        move |_, _, _| {
            offset.set(0);
            current.set(None);
            hits.set(Vec::new());
            hit.set(None);
            notice.set(String::new());
            let opening = original_diff.with_untracked(|d| d.is_some());
            let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
                let target = if opening {
                    document().get_element_by_id("text-comparison")
                } else {
                    document()
                        .query_selector(".outline-row.focused .entry-name")
                        .ok()
                        .flatten()
                };
                if let Some(node) = target.and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok()) {
                    let _ = node.focus();
                }
            });
            let _ = window().request_animation_frame(callback.unchecked_ref());
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
                gutter_top.set(node.scroll_top());
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
            gutter_top.set(from.scroll_top());
            if from.scroll_top() != to.scroll_top() {
                to.set_scroll_top(from.scroll_top());
            }
            if from.scroll_left() != to.scroll_left() {
                to.set_scroll_left(from.scroll_left());
            }
        }
    };
    Effect::watch(
        move || {
            (
                rows.with(|r| r.len()),
                offset.get(),
                english.get(),
                show_endings.get(),
            )
        },
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
    Effect::watch(
        move || diff.with(|d| d.as_ref().map(|d| d.id.clone())),
        move |_, _, _| {
            current.set(None);
            hits.set(Vec::new());
            hit.set(None);
            let last_page = rows.with_untracked(|r| r.len().saturating_sub(1) / 500 * 500);
            offset.update(|page| *page = (*page).min(last_page));
        },
        false,
    );
    let pane = move |right: bool| {
        view! {
            <div node_ref=if right{other_scroll}else{scroll} class="diff-scroll shared-diff" tabindex="0" aria-label=move ||if right{tr(english.get(),"右侧文本","Right text")}else{tr(english.get(),"左侧文本","Left text")} on:focusin=move |_|merge.focused.set(if right{Side::Right}else{Side::Left}) on:mousedown=move |_|merge.focused.set(if right{Side::Right}else{Side::Left}) on:scroll=move |_|sync(right)><div class="text-pane-content" style:min-width=move ||format!("{}px",width.get())><div class="diff-column-head">{move ||if right{tr(english.get(),"右侧 · 目标","Right · Destination")}else{tr(english.get(),"左侧 · 来源","Left · Source")}}<strong>{move ||(merge.side_dirty(&if right{Side::Right}else{Side::Left})).then_some(tr(english.get()," · 未保存"," · Unsaved"))}</strong></div>
            <For each=move ||diff.with(|d|d.as_ref().map(|d|rows.with(|r|r.iter().enumerate().skip(offset.get()).take(500).map(|(i,r)|(d.id.clone(),i,r.clone())).collect::<Vec<_>>())).unwrap_or_default()) key=|(id,i,_)|(id.clone(),*i) children=move |(_,i,row)|{
                let (line,ending_changed)=diff.with_untracked(|d|d.as_ref().map(|d|{let l=row.left.map(|n|&d.lines[n]);let r=row.right.map(|n|&d.lines[n]);(if right{r.cloned()}else{l.cloned()},text_view::significant_ending(l,r))})).unwrap_or_default();
                view!{<div class="diff-render-row">{move ||cell(line.as_ref(),right,ending_changed,show_endings.get(),current.get()==Some(i),english.get())}</div>}
            }/></div></div>
        }
    };
    let gutter = move || {
        view! {
            <div class="merge-gutter" role="group" aria-label=move ||tr(english.get(),"差异合并","Merge changes") on:wheel:capture=move |ev|{
                ev.prevent_default();
                if let Some(node)=scroll.get(){node.set_scroll_top(node.scroll_top()+ev.delta_y() as i32);sync(false);}
            }>
                <div class="merge-gutter-heading" aria-hidden="true">"← →"</div>
                <div class="merge-gutter-viewport"><div class="merge-gutter-content" style:transform=move ||format!("translateY(-{}px)",gutter_top.get())>
                    <For each=move ||preview.with(|p|p.as_ref().map(|p|p.hunks.iter().filter(|(i,id)|**i>=offset.get() && **i<offset.get()+500 && (**i==offset.get() || **i==0 || p.hunks.get(&(**i-1))!=Some(id))).map(|(i,id)|(p.diff.id.clone(),*i,*id)).collect::<Vec<_>>()).unwrap_or_default()) key=|(diff,i,hunk)|(diff.clone(),*i,*hunk) children=move |(_,i,hunk)|view!{
                        <div class="merge-gutter-row" style:top=move ||format!("{}px",i.saturating_sub(offset.get())*22)>
                            {[false,true].into_iter().map(move |right|view!{
                                <button class="merge-arrow" aria-label=move ||if right{tr(english.get(),"将此差异合并到右侧","Merge this change into right")}else{tr(english.get(),"将此差异合并到左侧","Merge this change into left")} title=move ||if right{tr(english.get(),"左 → 右：将此差异合并到右侧缓冲区","Left → right: merge this change into the right buffer")}else{tr(english.get(),"右 → 左：将此差异合并到左侧缓冲区","Right → left: merge this change into the left buffer")} disabled=move ||!merge.allowed(&if right{Side::Right}else{Side::Left}) on:focus=move |_|{
                                    if let Some(node)=scroll.get(){let y=(i.saturating_sub(offset.get_untracked())*22) as i32;if y<node.scroll_top() || y+48>node.scroll_top()+node.client_height(){jump(i);}}
                                } on:click=move |_|merge.apply(hunk,if right{Side::Right}else{Side::Left})>{if right{"→"}else{"←"}}</button>
                            }).collect_view()}
                        </div>
                    }/>
                </div></div>
            </div>
        }
    };
    view! {<Show when=move ||diff.with(|d|d.is_some())>{move ||untrack(move ||view!{<section id="text-comparison" class="diff-panel" tabindex="0" aria-label="Text comparison" on:keydown=move |ev|{if !ev.is_composing() && (ev.ctrl_key() || ev.meta_key()) && !ev.alt_key() && !ev.shift_key() && ev.key().eq_ignore_ascii_case("s"){ev.prevent_default();ev.stop_propagation();merge.save(Some(merge.focused.get_untracked()),saved);}else if !ev.is_composing() && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() && ev.key()=="F7"{ev.prevent_default();navigate(!ev.shift_key());}}><header><button class="quiet" disabled=move ||merge.busy.get() ||merge.dirty.get() on:click=move |_|{if let Some(d)=diff.get_untracked(){load(d);}}>{move ||tr(english.get(),"载入合并面板","Load into merge panel")}</button><strong>{move ||diff.with(|d|d.as_ref().map(|d|d.path.clone()))}</strong><button class="quiet" disabled=move ||merge.busy.get() ||merge.dirty.get() on:click=move |_|original_diff.set(None)>{move ||tr(english.get(),"返回目录","Back to folders")}</button></header>
        <div class="text-navigation"><button class="quiet" disabled=move ||!hunks.with(|h|h.iter().any(|n|current.get().is_some_and(|c|*n<c))) on:click=move |_|navigate(false)>{move ||tr(english.get(),"上一差异","Previous change")}</button><button class="quiet" disabled=move ||!hunks.with(|h|h.iter().any(|n|current.get().is_none_or(|c|*n>c))) on:click=move |_|navigate(true)>{move ||tr(english.get(),"下一差异","Next change")}</button><span>{move ||format!("{} {}",hunks.with(|h|h.len()),if merge.dirty.get(){tr(english.get(),"段未合并","remaining groups")}else{tr(english.get(),"组差异","change groups")})}</span><input aria-label="Find literal text" prop:value=move ||query.get() on:input=move |ev|{query.set(event_target_value(&ev));hits.set(Vec::new());hit.set(None);} on:keydown=move |ev|{if ev.key()=="Enter" && !ev.is_composing(){search();}}/><button class="quiet" on:click=move |_|search()>{move ||tr(english.get(),"查找原文","Find literal text")}</button><button class="quiet" disabled=move ||hit.get().is_none_or(|i|i==0) on:click=move |_|{let i=hit.get_untracked().unwrap_or_default().saturating_sub(1);if let Some(row)=hits.with_untracked(|h|h.get(i).copied()){hit.set(Some(i));jump(row);}}>{move ||tr(english.get(),"上一匹配","Previous match")}</button><button class="quiet" disabled=move ||hit.get().is_none_or(|i|i+1>=hits.with(|h|h.len())) on:click=move |_|{let i=hit.get_untracked().unwrap_or_default()+1;if let Some(row)=hits.with_untracked(|h|h.get(i).copied()){hit.set(Some(i));jump(row);}}>{move ||tr(english.get(),"下一匹配","Next match")}</button><span>{move ||format!("{} / {}",hit.get().map(|i|i+1).unwrap_or_default(),hits.with(|h|h.len()))}</span></div>
        <div class="text-merge-bar"><Show when=move ||merge.context().is_some() fallback=move ||view!{<span>{move ||tr(english.get(),"只读版本比较；合并请打开本地目录中的文件。","Read-only version comparison; open local directory files to merge.")}</span>}>
        <button class="quiet" disabled=move ||merge.busy.get() ||merge.undo.with(|h|h.is_empty()) on:click=move |_|merge.history(false)>{move ||tr(english.get(),"撤销合并","Undo merge")}</button><button class="quiet" disabled=move ||merge.busy.get() ||merge.redo.with(|h|h.is_empty()) on:click=move |_|merge.history(true)>{move ||tr(english.get(),"重做合并","Redo merge")}</button>
        <span class="merge-state">{move ||if merge.dirty.get(){format!("{}{}{}",if merge.side_dirty(&Side::Left){tr(english.get(),"左侧未保存","Left unsaved")}else{""},if merge.side_dirty(&Side::Left)&&merge.side_dirty(&Side::Right){" · "}else{""},if merge.side_dirty(&Side::Right){tr(english.get(),"右侧未保存","Right unsaved")}else{""})}else{tr(english.get(),"← / → 双向合并；⌘S / Ctrl+S 保存当前侧，按钮保存两侧。","Merge either way; ⌘S / Ctrl+S saves the focused pane, button saves both.").into()}}</span>
        <button class="quiet" disabled=move ||merge.busy.get() ||!merge.dirty.get() on:click=move |_|merge.discard()>{move ||tr(english.get(),"放弃合并","Discard merge")}</button><button class="primary" title=move ||tr(english.get(),"保存左右两侧所有未保存修改；⌘S / Ctrl+S 只保存当前侧。","Save all unsaved changes on both sides; ⌘S / Ctrl+S saves the focused pane only.") disabled=move ||merge.busy.get() ||!merge.dirty.get() on:click=move |_|merge.save(None,saved)>{move ||tr(english.get(),"保存两侧","Save both")}</button>
        </Show></div>
        <details class="text-legend"><summary>{move ||tr(english.get(),"显示 / 说明","Display / legend")}</summary><p class="text-view-note"><label class="line-ending-option"><input type="checkbox" prop:checked=move ||show_endings.get() on:change=move |ev|show_endings.set(event_target_checked(&ev))/>{move ||tr(english.get(),"显示所有行尾标记","Show all line endings")}</label>{move ||tr(english.get(),"修改组按行序对齐；下划线标记行内变化。默认只提示换行格式差异。LF / CRLF / CR 为行尾，∅ 表示无末尾换行。查找区分大小写，覆盖全部原文。","Changed groups align by line order; underlines mark intraline changes. Only differing endings are shown by default. LF / CRLF / CR identify endings; ∅ means no final newline. Search is case-sensitive across all original text.")}</p></details><Show when=move ||diff.with(|d|d.as_ref().is_some_and(|d|d.intraline_limited))><p class="text-view-note">{move ||tr(english.get(),"部分行内比较超出预算，已降级为整行提示；原始行差异仍保留。","Some intraline comparisons exceeded their budget and use whole-line hints; original line differences remain.")}</p></Show><Show when=move ||!notice.get().is_empty()><p role="alert">{move ||notice.get()}</p></Show>
        <div class="text-panes">{pane(false)}{gutter()}{pane(true)}</div><footer class="text-pagination"><button class="quiet" disabled=move ||offset.get()==0 on:click=move |_|{offset.update(|n|*n=n.saturating_sub(500));if let Some(n)=scroll.get(){n.set_scroll_top(0);}}>{move ||tr(english.get(),"上一页","Previous page")}</button><span>{move ||format!("{}–{} / {}",offset.get()+usize::from(rows.with(|r|!r.is_empty())),(offset.get()+500).min(rows.with(|r|r.len())),rows.with(|r|r.len()))}</span><button class="quiet" disabled=move ||{offset.get()+500>=rows.with(|r|r.len())} on:click=move |_|{offset.update(|n|*n+=500);if let Some(n)=scroll.get(){n.set_scroll_top(0);}}>{move ||tr(english.get(),"下一页","Next page")}</button></footer>
    </section>})}</Show>}
}
