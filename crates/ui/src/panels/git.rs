//! Local committed Git trees; working copies are never changed.
use crate::{invoke, label, status_class, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    visible: RwSignal<bool>,
    error: RwSignal<String>,
    diff: RwSignal<Option<TextDiff>>,
) -> impl IntoView {
    let repository = RwSignal::new(String::new());
    let left = RwSignal::new("HEAD~1".to_string());
    let right = RwSignal::new("HEAD".to_string());
    let exclusions = RwSignal::new(String::new());
    let search = RwSignal::new(String::new());
    let differences = RwSignal::new(false);
    let comparison = RwSignal::new(None::<OpenedComparison>);
    let page = RwSignal::new(None::<Page>);
    let offset = RwSignal::new(0usize);
    let reset = move || {
        comparison.set(None);
        page.set(None);
        diff.set(None);
    };
    let compare = move || {
        let request = GitCompareRequest {
            repository: repository.get_untracked(),
            left_revision: left.get_untracked(),
            right_revision: right.get_untracked(),
            exclusions: exclusions.get_untracked(),
        };
        busy.set(true);
        error.set(String::new());
        reset();
        spawn_local(async move {
            match invoke::<OpenedComparison>("compare_git", serde_json::json!({"request":request}))
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
            match invoke::<Page>("git_page", args).await {
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
        <Show when=move ||visible.get()><section class="archive-panel"><header><strong>{move ||tr(english.get(),"Git 提交版本比较","Git commit comparison")}</strong><button class="quiet" on:click=move |_|visible.set(false)>{move ||tr(english.get(),"收起","Collapse")}</button></header>
            <p>{move ||tr(english.get(),"比较本地已有的两个提交、分支或标签。不会检出、提交或下载对象；工作区及暂存区的修改不参与比较。需要支持 --no-lazy-fetch 的 Git。","Compare two locally available commits, branches or tags. No checkout, commit or object download occurs; working-tree and index changes are excluded. Requires Git supporting --no-lazy-fetch.")}</p>
            <div class="archive-controls"><label>{move ||tr(english.get(),"仓库目录","Repository")}<input disabled=move ||busy.get() prop:value=move ||repository.get() on:input=move |ev|{repository.set(event_target_value(&ev));reset();}/></label><button class="quiet" disabled=move ||busy.get() on:click=move |_|{busy.set(true);spawn_local(async move {match invoke::<Option<String>>("pick_directory",serde_json::json!({})).await{Ok(Some(p))=>{repository.set(p);reset();},Ok(None)=>{},Err(e)=>error.set(e)}busy.set(false);});}>{move ||tr(english.get(),"选择仓库…","Choose repository…")}</button></div>
            <div class="archive-sources"><label>{move ||tr(english.get(),"左侧修订","Left revision")}<input disabled=move ||busy.get() prop:value=move ||left.get() on:input=move |ev|{left.set(event_target_value(&ev));reset();}/></label><label>{move ||tr(english.get(),"右侧修订","Right revision")}<input disabled=move ||busy.get() prop:value=move ||right.get() on:input=move |ev|{right.set(event_target_value(&ev));reset();}/></label></div>
            <label class="archive-exclusions">{move ||tr(english.get(),"排除规则","Exclusions")}<input disabled=move ||busy.get() prop:value=move ||exclusions.get() on:input=move |ev|{exclusions.set(event_target_value(&ev));reset();}/></label>
            <div class="archive-controls"><button class="primary" disabled=move ||busy.get() ||repository.get().is_empty() ||left.get().is_empty() ||right.get().is_empty() on:click=move |_|compare()>{move ||tr(english.get(),"比较修订版本","Compare revisions")}</button></div>
            <Show when=move ||comparison.get().is_some()>{super::reports::controls(english,busy,error,move ||comparison.with(|c|c.as_ref().map(|c|(ReportSource::Git,c.session.clone()))))}<p>{move ||comparison.with(|c|c.as_ref().map(|c|format!("{} ↔ {}",c.left,c.right)))}</p><p>{move ||tr(english.get(),"一致按 Git 对象身份判定；可执行位变化也显示为修改。符号链接不跟随，子模块不展开，LFS 仅比较指针文件。","Equality uses Git object identity; executable-mode changes also count as changed. Links are not followed, submodules are not expanded, and LFS comparisons show pointer files.")}</p>
                <div class="archive-controls"><input aria-label="Search Git files" disabled=move ||busy.get() prop:value=move ||search.get() on:input=move |ev|search.set(event_target_value(&ev))/><button class="quiet" disabled=move ||busy.get() on:click=move |_|query(0)>{move ||tr(english.get(),"搜索全部路径","Search all paths")}</button><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||differences.get() on:change=move |ev|{differences.set(event_target_checked(&ev));query(0);}/>{move ||tr(english.get(),"只看差异","Differences only")}</label></div>
                <div class="archive-rows"><For each=move ||page.get().map(|p|p.rows).unwrap_or_default() key=|r|r.id.clone() children=move |row|{let id=row.id.clone();let state=row.status.clone();let css=format!("status {}",status_class(&state));let preview=row.left.as_ref().or(row.right.as_ref()).is_some_and(|e|e.kind==Kind::File)&&state!=Status::Error;view!{<div><button class="file-name" disabled=move ||busy.get() ||!preview on:click=move |_|{let Some(c)=comparison.get_untracked() else{return;};busy.set(true);error.set(String::new());diff.set(None);let args=serde_json::json!({"session":c.session,"rowId":id,"options":TextOptions::default()});spawn_local(async move {match invoke::<TextDiff>("git_text_diff",args).await{Ok(d)=>diff.set(Some(d)),Err(e)=>error.set(e)}busy.set(false);});}>{row.path}</button><span class=css>{move ||label(&state,english.get())}</span><span class="archive-error">{row.left.and_then(|e|e.error).or_else(||row.right.and_then(|e|e.error))}</span></div>}}/></div>
                <footer><button class="quiet" disabled=move ||busy.get() ||offset.get()==0 on:click=move |_|query(offset.get_untracked().saturating_sub(500))>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||page.get().map(|p|format!("{}–{} / {}",offset.get()+usize::from(p.total>0),(offset.get()+500).min(p.total),p.total))}</span><button class="quiet" disabled=move ||busy.get() ||page.get().is_none_or(|p|offset.get()+500>=p.total) on:click=move |_|query(offset.get_untracked()+500)>{move ||tr(english.get(),"下一页","Next")}</button></footer>
            </Show>
        </section></Show>
    }
}
