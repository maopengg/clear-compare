//! The existing revision-aware session library presented as named local projects.
use crate::tr;
use clear_contracts::LibraryEntry;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub library: RwSignal<Option<Vec<LibraryEntry>>>,
    pub session_name: RwSignal<String>,
    pub active_library: RwSignal<Option<(String, String)>>,
    pub left: RwSignal<String>,
    pub right: RwSignal<String>,
    pub exclusions: RwSignal<String>,
    pub dirty: Memo<bool>,
}
#[allow(clippy::too_many_arguments)]
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    s: Signals,
    refresh: impl Fn() + Copy + Send + Sync + 'static,
    save: impl Fn(bool) + Copy + Send + Sync + 'static,
    open: impl Fn(LibraryEntry) + Copy + Send + Sync + 'static,
    archive: impl Fn(LibraryEntry) + Copy + Send + Sync + 'static,
    new: impl Fn() + Copy + Send + Sync + 'static,
    close: impl Fn() + Copy + Send + Sync + 'static,
    changed: impl Fn() + Copy + Send + Sync + 'static,
    edit_rules: impl Fn() + Copy + Send + Sync + 'static,
    pick: impl Fn(bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let filter = RwSignal::new(String::new());
    let can_save = move || {
        !s.session_name.get().trim().is_empty()
            && !s.left.get().trim().is_empty()
            && !s.right.get().trim().is_empty()
    };
    view! {<Show when=move||s.library.get().is_some()><section class="session-panel project-manager" role="dialog" aria-modal="true" aria-label=move||tr(english.get(),"项目管理","Projects")>
        <header><strong>{move||tr(english.get(),"项目管理","Projects")}</strong><button class="quiet" disabled=move||busy.get() on:click=move|_|new()>{move||tr(english.get(),"新建项目","New project")}</button><button class="quiet" disabled=move||busy.get() on:click=move|_|refresh()>{move||tr(english.get(),"刷新列表","Refresh list")}</button><button class="quiet" disabled=move||busy.get() on:click=move|_|close()>{move||tr(english.get(),"关闭","Close")}</button></header>
        <div class="project-manager-body"><aside><input aria-label=move||tr(english.get(),"搜索项目","Search projects") placeholder=move||tr(english.get(),"搜索项目…","Search projects…") prop:value=move||filter.get() on:input=move|ev|filter.set(event_target_value(&ev))/>
            <div class="project-list"><For each=move||{let q=filter.get().to_lowercase();s.library.get().unwrap_or_default().into_iter().filter(|e|q.is_empty()||e.name.to_lowercase().contains(&q)).collect::<Vec<_>>()} key=|e|(e.id.clone(),e.revision.clone()) children=move|entry|{
                let selected=entry.id.clone();let load=entry.clone();let archived=entry.clone();
                view!{<div class=move||if s.active_library.get().is_some_and(|(id,_)|id==selected){"project-entry active"}else{"project-entry"}>
                    <button class="project-open" disabled=move||busy.get() on:click=move|_|open(load.clone())><strong>{entry.name}</strong><small title=entry.session.request.left.clone()>{entry.session.request.left.clone()}</small><small title=entry.session.request.right.clone()>{entry.session.request.right.clone()}</small><span>{move||tr(english.get(),"打开项目","Open project")}</span></button>
                    <button class="quiet project-archive" disabled=move||busy.get() on:click=move|_|archive(archived.clone())>{move||tr(english.get(),"归档配置","Archive settings")}</button>
                </div>}
            }/></div>
            <Show when=move||s.library.get().is_some_and(|v|v.is_empty())><p>{move||tr(english.get(),"尚无项目。为当前比较命名并保存，或新建项目。","No projects yet. Name and save this comparison, or create a project.")}</p></Show>
        </aside><div class="project-details"><h2>{move||if s.active_library.get().is_some(){tr(english.get(),"当前项目","Current project")}else{tr(english.get(),"未保存项目","Unsaved project")}}</h2>
            <label>{move||tr(english.get(),"项目名称","Project name")}<input aria-label=move||tr(english.get(),"项目名称","Project name") maxlength="64" disabled=move||busy.get() prop:value=move||s.session_name.get() on:input=move|ev|s.session_name.set(event_target_value(&ev))/></label>
            <label>{move||tr(english.get(),"左侧文件夹","Left folder")}<input aria-label=move||tr(english.get(),"项目左侧文件夹","Project left folder") disabled=move||busy.get() prop:value=move||s.left.get() on:input=move|ev|{changed();s.left.set(event_target_value(&ev));}/><button class="quiet" disabled=move||busy.get() on:click=move|_|pick(true)>{move||tr(english.get(),"选择左侧文件夹…","Choose left folder…")}</button></label>
            <label>{move||tr(english.get(),"右侧文件夹","Right folder")}<input aria-label=move||tr(english.get(),"项目右侧文件夹","Project right folder") disabled=move||busy.get() prop:value=move||s.right.get() on:input=move|ev|{changed();s.right.set(event_target_value(&ev));}/><button class="quiet" disabled=move||busy.get() on:click=move|_|pick(false)>{move||tr(english.get(),"选择右侧文件夹…","Choose right folder…")}</button></label>
            <div class="project-rules-heading"><strong>{move||tr(english.get(),"本项目排除规则","Project exclusions")}</strong><button class="quiet" disabled=move||busy.get() on:click=move|_|edit_rules()>{move||tr(english.get(),"添加 / 编辑排除项","Add / edit exclusions")}</button></div>
            <div class="project-rule-summary">{move||s.exclusions.get().split([';','\n']).map(str::trim).filter(|r|!r.is_empty()).map(|r|view!{<code>{r.to_owned()}</code>}).collect_view()}</div>
            <p>{move||tr(english.get(),"每个项目独立保存路径、排除规则、比较与视图设置。切换项目及关闭此面板前保存当前项目；打开项目只加载配置，不修改目录文件。归档只移走配置。","Each project stores paths, exclusions, comparison and view settings independently. Switching projects or closing this panel saves the current project. Opening only loads settings; archiving affects configuration only.")}</p>
            <footer><span>{move||if s.dirty.get(){tr(english.get(),"有未保存的配置修改","Settings changed")}else{tr(english.get(),"本地保存","Stored locally")}}</span><button class="quiet" disabled=move||busy.get()||!can_save() on:click=move|_|save(false)>{move||tr(english.get(),"另存为新项目","Save as new project")}</button><button class="primary" disabled=move||busy.get()||!can_save() on:click=move|_|save(s.active_library.get_untracked().is_some())>{move||tr(english.get(),"保存项目","Save project")}</button></footer>
        </div></div>
    </section></Show>}
}
