//! Named comparison configurations, with explicit loading and revision-aware updates.
use crate::tr;
use clear_contracts::LibraryEntry;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub library: RwSignal<Option<Vec<LibraryEntry>>>,
    pub session_name: RwSignal<String>,
    pub active_library: RwSignal<Option<(String, String)>>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    refresh: impl Fn() + Copy + Send + Sync + 'static,
    save: impl Fn(bool) + Copy + Send + Sync + 'static,
    open: impl Fn(LibraryEntry) + Copy + Send + Sync + 'static,
    archive: impl Fn(LibraryEntry) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        library,
        session_name,
        active_library,
    } = signals;
    view! {
        <Show when=move ||library.get().is_some()><section class="session-panel">
            <header><strong>{move ||tr(english.get(),"比较会话库","Comparison session library")}</strong><button class="quiet" disabled=move ||busy.get() on:click=move |_|refresh()>{move ||tr(english.get(),"刷新","Refresh")}</button><button class="quiet" on:click=move |_|library.set(None)>{move ||tr(english.get(),"关闭","Close")}</button></header>
            <p>{move ||tr(english.get(),"保存路径、排除规则和视图设置。打开会话只加载配置；归档后配置文件保留在本机会话归档目录。","Save paths, exclusions and view settings. Opening only loads configuration; archived files are retained in the local session archive directory.")}</p>
            <div class="session-actions"><input aria-label="Session name" maxlength="64" placeholder=move ||tr(english.get(),"会话名称","Session name") disabled=move ||busy.get() prop:value=move ||session_name.get() on:input=move |ev|session_name.set(event_target_value(&ev))/><button class="quiet" disabled=move ||busy.get() ||session_name.get().trim().is_empty() on:click=move |_|save(false)>{move ||tr(english.get(),"另存新会话","Save as new")}</button><button class="primary" disabled=move ||busy.get() ||active_library.get().is_none() ||session_name.get().trim().is_empty() on:click=move |_|save(true)>{move ||tr(english.get(),"更新当前会话","Update current session")}</button></div>
            <Show when=move ||library.get().is_some_and(|e|e.is_empty())><p>{move ||tr(english.get(),"尚未保存会话。先填写两个比较路径，再命名保存。","No saved sessions. Fill both comparison paths, then save with a name.")}</p></Show>
            <div class="session-list"><For each=move ||library.get().unwrap_or_default() key=|e|(e.id.clone(),e.revision.clone()) children=move |entry|{let load=entry.clone();let archived=entry.clone();let id=entry.id.clone();view!{<div class=move ||if active_library.get().is_some_and(|(active,_)|active==id){"session-entry active"}else{"session-entry"}><div><strong>{entry.name}</strong><p>{entry.session.request.left}<br/>{entry.session.request.right}</p></div><button class="quiet" disabled=move ||busy.get() on:click=move |_|open(load.clone())>{move ||tr(english.get(),"打开","Open")}</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|archive(archived.clone())>{move ||tr(english.get(),"归档","Archive")}</button></div>}}/></div>
        </section></Show>
    }
}
