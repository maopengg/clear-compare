//! Independent read-only ZIP/directory comparison workspace.
use crate::{invoke, label, status_class, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    visible: RwSignal<bool>,
    error: RwSignal<String>,
    diff: RwSignal<Option<TextDiff>>,
    diff_limit: RwSignal<usize>,
) -> impl IntoView {
    let left = RwSignal::new(String::new());
    let right = RwSignal::new(String::new());
    let exclusions = RwSignal::new(String::new());
    let content = RwSignal::new(true);
    let search = RwSignal::new(String::new());
    let differences = RwSignal::new(false);
    let comparison = RwSignal::new(None::<OpenedComparison>);
    let page = RwSignal::new(None::<Page>);
    let offset = RwSignal::new(0usize);
    let pick = move |is_left: bool, folder: bool| {
        busy.set(true);
        spawn_local(async move {
            let command = if folder {
                "pick_directory"
            } else {
                "pick_archive"
            };
            match invoke::<Option<String>>(command, serde_json::json!({})).await {
                Ok(Some(path)) => {
                    if is_left {
                        left.set(path);
                    } else {
                        right.set(path);
                    }
                    comparison.set(None);
                    page.set(None);
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let compare = move || {
        let request = ArchiveRequest {
            left: left.get_untracked(),
            right: right.get_untracked(),
            exclusions: exclusions.get_untracked(),
            content: content.get_untracked(),
        };
        busy.set(true);
        error.set(String::new());
        comparison.set(None);
        page.set(None);
        diff.set(None);
        spawn_local(async move {
            match invoke::<OpenedComparison>(
                "compare_archives",
                serde_json::json!({"request":request}),
            )
            .await
            {
                Ok(c) => {
                    page.set(Some(c.page.clone()));
                    comparison.set(Some(c));
                    offset.set(0);
                    search.set(String::new());
                    differences.set(false);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let query = move |start: usize| {
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        busy.set(true);
        let args = serde_json::json!({"session":c.session,"offset":start,"search":search.get_untracked(),"differencesOnly":differences.get_untracked()});
        spawn_local(async move {
            match invoke::<Page>("archive_page", args).await {
                Ok(p) => {
                    page.set(Some(p));
                    offset.set(start);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    view! {
        <Show when=move ||visible.get()><section class="archive-panel"><header><strong>{move ||tr(english.get(),"ZIP / 目录只读比较","Read-only ZIP / directory comparison")}</strong><button class="quiet" on:click=move |_|visible.set(false)>{move ||tr(english.get(),"收起","Collapse")}</button></header>
            <p>{move ||tr(english.get(),"选择 ZIP 或目录；右侧留空可浏览左侧内容。内容校验会解码并计算 SHA-256，不会解压到磁盘或修改来源。","Choose ZIP files or directories; leave the right side empty to browse. Content verification decodes and hashes members without extracting to disk or modifying sources.")}</p>
            <div class="archive-sources"><label>{move ||tr(english.get(),"左侧来源","Left source")}<input disabled=move ||busy.get() prop:value=move ||left.get() on:input=move |ev|{left.set(event_target_value(&ev));comparison.set(None);page.set(None);}/><span><button class="quiet" disabled=move ||busy.get() on:click=move |_|pick(true,false)>"ZIP…"</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|pick(true,true)>{move ||tr(english.get(),"目录…","Directory…")}</button></span></label><label>{move ||tr(english.get(),"右侧来源（可留空）","Right source (optional)")}<input disabled=move ||busy.get() prop:value=move ||right.get() on:input=move |ev|{right.set(event_target_value(&ev));comparison.set(None);page.set(None);}/><span><button class="quiet" disabled=move ||busy.get() on:click=move |_|pick(false,false)>"ZIP…"</button><button class="quiet" disabled=move ||busy.get() on:click=move |_|pick(false,true)>{move ||tr(english.get(),"目录…","Directory…")}</button></span></label></div>
            <label class="archive-exclusions">{move ||tr(english.get(),"排除规则","Exclusions")}<input disabled=move ||busy.get() prop:value=move ||exclusions.get() on:input=move |ev|{exclusions.set(event_target_value(&ev));comparison.set(None);page.set(None);}/></label>
            <div class="archive-controls"><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||content.get() on:change=move |ev|{content.set(event_target_checked(&ev));comparison.set(None);page.set(None);}/>{move ||tr(english.get(),"校验成员内容（SHA-256）","Verify member contents (SHA-256)")}</label><button class="primary" disabled=move ||busy.get() ||left.get().is_empty() on:click=move |_|compare()>{move ||tr(english.get(),"浏览 / 比较","Browse / compare")}</button></div>
            <Show when=move ||comparison.get().is_some()><p>{move ||comparison.with(|c|c.as_ref().map(|c|format!("{} {} · {} {} · {} {} · {} {}",tr(english.get(),"一致","Same"),c.summary.same,tr(english.get(),"修改","Changed"),c.summary.changed,tr(english.get(),"未验证","Unverified"),c.summary.unverified,tr(english.get(),"错误","Errors"),c.summary.errors)))}</p><div class="archive-controls"><input aria-label="Search archive members" disabled=move ||busy.get() prop:value=move ||search.get() on:input=move |ev|search.set(event_target_value(&ev))/><button class="quiet" disabled=move ||busy.get() on:click=move |_|query(0)>{move ||tr(english.get(),"搜索全部成员","Search all members")}</button><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||differences.get() on:change=move |ev|{differences.set(event_target_checked(&ev));query(0);}/>{move ||tr(english.get(),"只看差异","Differences only")}</label></div>
                <div class="archive-rows"><For each=move ||page.get().map(|p|p.rows).unwrap_or_default() key=|r|r.id.clone() children=move |row|{let id=row.id.clone();let state=row.status.clone();let css=format!("status {}",status_class(&state));let can_preview=row.left.as_ref().or(row.right.as_ref()).is_some_and(|e|e.kind==Kind::File) &&state!=Status::Error;view!{<div><button class="file-name" disabled=move ||busy.get() ||!can_preview on:click=move |_|{let Some(c)=comparison.get_untracked() else{return;};let id=id.clone();busy.set(true);error.set(String::new());diff.set(None);diff_limit.set(500);spawn_local(async move {match invoke::<TextDiff>("archive_text_diff",serde_json::json!({"session":c.session,"rowId":id,"options":TextOptions::default()})).await {Ok(d)=>diff.set(Some(d)),Err(e)=>error.set(e)}busy.set(false);});}>{row.path}</button><span class=css>{move ||label(&state,english.get())}</span><span class="archive-error">{row.left.and_then(|e|e.error).or_else(||row.right.and_then(|e|e.error))}</span></div>}}/></div>
                <footer><button class="quiet" disabled=move ||busy.get() ||offset.get()==0 on:click=move |_|query(offset.get_untracked().saturating_sub(500))>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||page.get().map(|p|format!("{}–{} / {}",offset.get()+usize::from(p.total>0),(offset.get()+500).min(p.total),p.total))}</span><button class="quiet" disabled=move ||busy.get() ||page.get().is_none_or(|p|offset.get()+500>=p.total) on:click=move |_|query(offset.get_untracked()+500)>{move ||tr(english.get(),"下一页","Next")}</button></footer>
            </Show>
        </section></Show>
    }
}
