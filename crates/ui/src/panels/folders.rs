//! Native directory navigation and global query controls.
use crate::tr;
use clear_contracts::Breadcrumb;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub parent_id: RwSignal<Option<String>>,
    pub breadcrumbs: RwSignal<Vec<Breadcrumb>>,
    pub recursive: RwSignal<bool>,
    pub sort: RwSignal<String>,
    pub descending: RwSignal<bool>,
}
pub fn controls(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    browse: impl Fn(Option<String>) + Copy + Send + Sync + 'static,
    load: impl Fn(usize) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        parent_id,
        breadcrumbs,
        recursive,
        sort,
        descending,
    } = signals;
    view! {
        <div class="folder-controls">
            <nav aria-label="Directory breadcrumbs"><button class="quiet" disabled=move ||busy.get() on:click=move |_|browse(None)>{move ||tr(english.get(),"根目录","Root")}</button><For each=move ||breadcrumbs.get() key=|b|b.id.clone() children=move |crumb|{let id=crumb.id.clone();view!{<span>"/"</span><button class="quiet" disabled=move ||busy.get() on:click=move |_|browse(Some(id.clone()))>{crumb.label}</button>}}/></nav>
            <label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||recursive.get() on:change=move |ev|{recursive.set(event_target_checked(&ev));load(0);}/>{move ||tr(english.get(),"包含子目录","Include subdirectories")}</label>
            <label>{move ||tr(english.get(),"排序","Sort")}<select disabled=move ||busy.get() prop:value=move ||sort.get() on:change=move |ev|{sort.set(event_target_value(&ev));load(0);}><option value="path">{move ||tr(english.get(),"路径","Path")}</option><option value="status">{move ||tr(english.get(),"状态","Status")}</option><option value="left_size">{move ||tr(english.get(),"左侧大小","Left size")}</option><option value="right_size">{move ||tr(english.get(),"右侧大小","Right size")}</option></select></label>
            <label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||descending.get() on:change=move |ev|{descending.set(event_target_checked(&ev));load(0);}/>{move ||tr(english.get(),"降序","Descending")}</label>
            <span>{move ||if parent_id.get().is_none(){tr(english.get(),"整个比较范围","Entire comparison")}else{tr(english.get(),"当前目录范围","Current directory scope")}}</span>
        </div>
    }
}
