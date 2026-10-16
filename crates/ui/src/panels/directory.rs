//! Aligned two-sided directory outline. Every operation uses native snapshot IDs.
use super::directory_menu::{self, Target};
use crate::{label, size, tr};
use clear_contracts::*;
use leptos::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy)]
pub struct Signals {
    pub english: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub comparison: RwSignal<Option<OpenedComparison>>,
    pub rows: RwSignal<Vec<Row>>,
    pub outline: RwSignal<BTreeMap<String, OutlineNode>>,
    pub selected: RwSignal<BTreeSet<String>>,
    pub focused: RwSignal<Option<String>>,
    pub operation: RwSignal<String>,
    pub reverse: RwSignal<bool>,
}
pub(super) fn modified(entry: Option<&Entry>, english: bool) -> String {
    let Some(ms) = entry
        .and_then(|e| e.modified_ms.as_ref())
        .and_then(|n| n.parse::<f64>().ok())
    else {
        return "—".into();
    };
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    date.to_locale_string(
        if english { "en-GB" } else { "zh-CN" },
        &wasm_bindgen::JsValue::UNDEFINED,
    )
    .into()
}
fn side(
    row: Row,
    node: Option<OutlineNode>,
    right: bool,
    s: Signals,
    toggle: impl Fn(String) + Copy + Send + Sync + 'static,
    open: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let entry = if right {
        row.right.as_ref()
    } else {
        row.left.as_ref()
    };
    let exists = entry.is_some();
    let directory = entry.is_some_and(|e| e.kind == Kind::Directory);
    let expanded = node.as_ref().is_some_and(|n| n.expanded);
    let expandable = node.as_ref().is_some_and(|n| n.has_children);
    let depth = node.as_ref().map_or(0, |n| n.depth);
    let name = node
        .as_ref()
        .map(|n| n.name.clone())
        .unwrap_or_else(|| row.path.clone());
    let id = row.id.clone();
    let click_id = id.clone();
    let double_id = id.clone();
    let key_id = id.clone();
    let entry = entry.cloned();
    let bytes = if exists {
        size(entry.as_ref())
    } else {
        String::new()
    };
    let error = entry
        .as_ref()
        .and_then(|e| e.error.clone())
        .unwrap_or_else(|| row.path.clone());
    view! {
        <td class=if exists {"directory-name"} else {"directory-name absent"} title=error>
            <span hidden=exists class="missing-entry">{move ||tr(s.english.get(),"— 缺失","— Missing")}</span>
                <div hidden=!exists class="directory-name-content" style:padding-left=format!("{}px",depth*16)>
                    <button class="tree-toggle" disabled=move ||s.busy.get() || !expandable aria-label=move ||if expanded {tr(s.english.get(),"折叠目录","Collapse folder")} else {tr(s.english.get(),"展开目录","Expand folder")} aria-expanded=directory.then_some(expanded.to_string()) on:click=move |_|toggle(id.clone())>{if directory {if expanded {"▾"} else {"▸"}} else {""}}</button>
                    <span class=if directory {"entry-icon folder-icon"} else {"entry-icon file-icon"} aria-hidden="true">{if directory {"▰"} else {"▤"}}</span>
                    <button class="entry-name" disabled=move ||s.busy.get() on:click=move |_|s.focused.set(Some(click_id.clone())) on:dblclick=move |_|{if directory {toggle(double_id.clone())} else {open(double_id.clone())}} on:keydown=move |ev|{if ev.key()=="Enter" {ev.prevent_default();if directory {toggle(key_id.clone())} else {open(key_id.clone())}}}>{name.clone()}</button>
                </div>
        </td>
        <td class="directory-size">{bytes}</td>
        <td class="directory-time">{move ||if exists {modified(entry.as_ref(),s.english.get())} else {String::new()}}</td>
    }
}
pub fn panel(
    s: Signals,
    toggle: impl Fn(String) + Copy + Send + Sync + 'static,
    open: impl Fn(String) + Copy + Send + Sync + 'static,
    edit: impl Fn(String, Side) + Copy + Send + Sync + 'static,
    inspect: impl Fn(String, &'static str) + Copy + Send + Sync + 'static,
    selection_changed: impl Fn() + Copy + Send + Sync + 'static,
    file_action: impl Fn(String, Side, &'static str, String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let menu = RwSignal::new(None::<Target>);
    Effect::watch(
        move || {
            (
                s.comparison.with(|c| c.as_ref().map(|c| c.session.clone())),
                s.busy.get(),
            )
        },
        move |_, _, _| menu.set(None),
        false,
    );
    let focused = Signal::derive(move || {
        s.rows.with(|rows| {
            rows.iter()
                .find(|r| Some(&r.id) == s.focused.get().as_ref())
                .cloned()
        })
    });
    let open_focused = move || {
        if let Some(row) = focused.get_untracked() {
            open(row.id)
        }
    };
    view! {
        <section class="directory-workspace" aria-label="Side-by-side directory comparison">
            <div class="directory-roots"><span title=move ||s.comparison.with(|c|c.as_ref().map(|c|c.left.clone()))>{move ||s.comparison.with(|c|c.as_ref().map(|c|c.left.clone()))}</span><span title=move ||s.comparison.with(|c|c.as_ref().map(|c|c.right.clone()))>{move ||s.comparison.with(|c|c.as_ref().map(|c|c.right.clone()))}</span></div>
            <div class="directory-scroll" on:contextmenu=move |ev|ev.prevent_default() on:scroll=move |_|menu.set(None)><table class="directory-table"><colgroup><col class="selection-col"/><col class="name-col"/><col class="size-col"/><col class="time-col"/><col class="state-col"/><col class="name-col"/><col class="size-col"/><col class="time-col"/></colgroup><thead><tr><th aria-label="Selection"></th><th>{move ||tr(s.english.get(),"左侧名称","Left name")}</th><th>{move ||tr(s.english.get(),"大小","Size")}</th><th>{move ||tr(s.english.get(),"修改时间","Modified")}</th><th>{move ||tr(s.english.get(),"比较","Compare")}</th><th>{move ||tr(s.english.get(),"右侧名称","Right name")}</th><th>{move ||tr(s.english.get(),"大小","Size")}</th><th>{move ||tr(s.english.get(),"修改时间","Modified")}</th></tr></thead>
            <tbody><For each=move ||s.rows.get() key=move |r|(s.comparison.with_untracked(|c|c.as_ref().map(|c|c.session.clone())),r.id.clone(),s.outline.with_untracked(|o|o.get(&r.id).map(|n|(n.expanded,n.depth,n.has_children,n.context_only)))) children=move |row|{
                let node=s.outline.with_untracked(|nodes|nodes.get(&row.id).cloned());
                let title=row.path.clone(); let state=row.status.clone(); let title_state=state.clone(); let focus_id=row.id.clone(); let select_id=row.id.clone();let checked_id=row.id.clone();
                let directory=row.left.as_ref().or(row.right.as_ref()).is_some_and(|e|e.kind==Kind::Directory);
                let can_left=row.left.as_ref().is_some_and(|e|e.kind==Kind::File && e.error.is_none());let can_right=row.right.as_ref().is_some_and(|e|e.kind==Kind::File && e.error.is_none());
                let eligible_left=can_left && matches!(row.status,Status::Changed|Status::LeftOnly);let eligible_right=can_right && matches!(row.status,Status::Changed|Status::RightOnly);
                let state_class=crate::status_class(&state);
                let context_row=row.clone();let keyboard_row=row.clone(); let left=row.clone(); let right=row;
                view! {<tr on:contextmenu=move |ev|{ev.prevent_default();if !s.busy.get_untracked(){s.focused.set(Some(context_row.id.clone()));menu.set(Some(Target::mouse(context_row.clone(),&ev)));directory_menu::focus_menu();}} on:keydown=move |ev|{if ev.key()=="ContextMenu" || (ev.key()=="F10" && ev.shift_key()){ev.prevent_default();if !s.busy.get_untracked(){s.focused.set(Some(keyboard_row.id.clone()));menu.set(Some(Target::keyboard(keyboard_row.clone(),&ev)));directory_menu::focus_menu();}}} class=move ||format!("outline-row {} {}",state_class,if s.focused.get().as_ref()==Some(&focus_id){"focused"}else{""}) title=title>
                    <td><input type="checkbox" aria-label=move ||tr(s.english.get(),"选择文件","Select file") disabled=move ||s.busy.get() || !(match s.operation.get().as_str(){"delete_left"|"rename_left"=>can_left,"delete_right"|"rename_right"=>can_right,"mirror_left_to_right"|"mirror_right_to_left"|"mirror_tree_left_to_right"|"mirror_tree_right_to_left"=>false,_=>if s.reverse.get(){eligible_right}else{eligible_left}}) prop:checked=move ||s.selected.with(|ids|ids.contains(&checked_id)) on:change=move |ev|{let checked=event_target_checked(&ev);s.selected.update(|ids|{if checked{ids.insert(select_id.clone());}else{ids.remove(&select_id);}});selection_changed();}/></td>
                    {side(left,node.clone(),false,s,toggle,open)}
                    <td class="directory-state"><span class=format!("pair-state {state_class}") title=move ||label(&title_state,s.english.get())>{move ||if directory {label(&state,s.english.get())} else {match state {Status::Same=>"=",Status::Changed=>"≠",Status::LeftOnly=>"←",Status::RightOnly=>"→",Status::MetadataMatch=>"?",_=>"!"}}}</span></td>
                    {side(right,node,true,s,toggle,open)}
                </tr>}
            }/></tbody></table></div>
            <div class="directory-actions"><span>{move ||focused.get().map(|r|r.path).unwrap_or_else(||tr(s.english.get(),"单击选择；双击文件查看逐行差异，点击三角展开目录。","Select a name; double-click a file for line differences, or use the triangle to expand a folder.").into())}</span>
                <button class="quiet" disabled=move ||s.busy.get() ||!focused.get().is_some_and(|r|r.left.as_ref().or(r.right.as_ref()).is_some_and(|e|e.kind==Kind::File)) on:click=move |_|open_focused()>{move ||tr(s.english.get(),"查看内容差异","Compare contents")}</button>
                <button class="quiet" disabled=move ||s.busy.get() ||!focused.get().is_some_and(|r|r.left.is_some_and(|e|e.kind==Kind::File)) on:click=move |_|{if let Some(r)=focused.get_untracked(){edit(r.id,Side::Left)}}>{move ||tr(s.english.get(),"编辑左侧","Edit left")}</button>
                <button class="quiet" disabled=move ||s.busy.get() ||!focused.get().is_some_and(|r|r.right.is_some_and(|e|e.kind==Kind::File)) on:click=move |_|{if let Some(r)=focused.get_untracked(){edit(r.id,Side::Right)}}>{move ||tr(s.english.get(),"编辑右侧","Edit right")}</button>
                <button class="quiet" disabled=move ||s.busy.get() ||focused.get().is_none() on:click=move |_|{if let Some(r)=focused.get_untracked(){inspect(r.id,"binary")}}>"Hex"</button>
                <button class="quiet" disabled=move ||s.busy.get() ||focused.get().is_none() on:click=move |_|{if let Some(r)=focused.get_untracked(){inspect(r.id,"table")}}>{move ||tr(s.english.get(),"表格","Table")}</button>
                <button class="quiet" disabled=move ||s.busy.get() ||focused.get().is_none() on:click=move |_|{if let Some(r)=focused.get_untracked(){inspect(r.id,"picture")}}>{move ||tr(s.english.get(),"图片","Picture")}</button>
            </div>
            {directory_menu::panel(s,menu,toggle,open,edit,inspect,file_action)}
        </section>
    }
}
