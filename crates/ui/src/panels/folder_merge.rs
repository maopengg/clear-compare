//! Baseline-aware folder merge, reviewed choices, and separate empty output.
use crate::{invoke, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
pub struct Signals {
    pub visible: RwSignal<bool>,
    pub preview: RwSignal<Option<FolderMergePreview>>,
    pub left: RwSignal<String>,
    pub right: RwSignal<String>,
    pub exclusions: RwSignal<String>,
    pub error: RwSignal<String>,
    pub message: RwSignal<String>,
}
fn choice(value: &str) -> FolderResolution {
    match value {
        "combine" => FolderResolution::Combine,
        "base" => FolderResolution::Base,
        "left" => FolderResolution::Left,
        "right" => FolderResolution::Right,
        "delete" => FolderResolution::Delete,
        "auto" => FolderResolution::Auto,
        _ => FolderResolution::Conflict,
    }
}
fn value(resolution: &FolderResolution) -> &'static str {
    match resolution {
        FolderResolution::Combine => "combine",
        FolderResolution::Base => "base",
        FolderResolution::Left => "left",
        FolderResolution::Right => "right",
        FolderResolution::Delete => "delete",
        FolderResolution::Auto => "auto",
        FolderResolution::Conflict => "conflict",
    }
}
pub fn panel(english: RwSignal<bool>, busy: RwSignal<bool>, signals: Signals) -> impl IntoView {
    let Signals {
        visible,
        preview,
        left,
        right,
        exclusions,
        error,
        message,
    } = signals;
    let two_way = RwSignal::new(false);
    let base = RwSignal::new(String::new());
    let output = RwSignal::new(String::new());
    let offset = RwSignal::new(0usize);
    let render_generation = RwSignal::new(0usize);
    let pick = move |baseline: bool| {
        busy.set(true);
        spawn_local(async move {
            match invoke::<Option<String>>("pick_directory", serde_json::json!({})).await {
                Ok(Some(p)) => {
                    if baseline {
                        base.set(p);
                    } else {
                        output.set(p);
                    }
                    preview.set(None);
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let prepare = move || {
        busy.set(true);
        error.set(String::new());
        preview.set(None);
        let request = FolderMergeRequest {
            two_way: two_way.get_untracked(),
            base: base.get_untracked(),
            left: left.get_untracked(),
            right: right.get_untracked(),
            output: output.get_untracked(),
            exclusions: exclusions.get_untracked(),
        };
        spawn_local(async move {
            match invoke::<FolderMergePreview>(
                "prepare_folder_merge",
                serde_json::json!({"request":request}),
            )
            .await
            {
                Ok(p) => {
                    offset.set(0);
                    preview.set(Some(p));
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let page = move |forward: bool| {
        let Some(p) = preview.get_untracked() else {
            return;
        };
        let next = if forward {
            offset.get_untracked() + 500
        } else {
            offset.get_untracked().saturating_sub(500)
        };
        busy.set(true);
        spawn_local(async move {
            match invoke::<FolderMergePreview>(
                "folder_merge_page",
                serde_json::json!({"planId":p.id,"offset":next}),
            )
            .await
            {
                Ok(p) => {
                    preview.set(Some(p));
                    offset.set(next);
                }
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let resolve = move |id: String, resolution: FolderResolution| {
        let Some(p) = preview.get_untracked() else {
            return;
        };
        busy.set(true);
        let start = offset.get_untracked();
        spawn_local(async move {
            match invoke::<FolderMergePreview>("resolve_folder_merge",serde_json::json!({"planId":p.id,"rowId":id,"resolution":resolution,"offset":start})).await {Ok(p)=>preview.set(Some(p)),Err(e)=>{error.set(e);render_generation.update(|n|*n+=1);}}
            busy.set(false);
        });
    };
    let apply = move || {
        let Some(p) = preview.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match invoke::<FolderMergeResult>(
                "apply_folder_merge",
                serde_json::json!({"planId":p.id}),
            )
            .await
            {
                Ok(r) => {
                    let completed = r.actions.iter().filter(|a| a.state == "completed").count();
                    message.set(format!(
                        "{} {} · {}",
                        tr(
                            english.get_untracked(),
                            "目录输出已完成条目：",
                            "Folder output completed entries:"
                        ),
                        completed,
                        r.journal
                    ));
                    if let Some(a) = r.actions.iter().find(|a| a.error.is_some()) {
                        error.set(format!(
                            "{}: {}",
                            a.path,
                            a.error.as_deref().unwrap_or_default()
                        ));
                    }
                }
                Err(e) => error.set(e),
            }
            preview.set(None);
            busy.set(false);
        });
    };
    view! {
        <Show when=move ||visible.get()><section class="folder-merge-panel"><header><strong>{move ||tr(english.get(),"目录合并","Folder merge")}</strong><button class="quiet" on:click=move |_|visible.set(false)>{move ||tr(english.get(),"收起","Collapse")}</button></header>
            <p>{move ||tr(english.get(),"使用上方的左右路径和排除规则。基础、左右、输出及日志目录必须互不包含。输出目录必须为空；输入目录保持不变。","Use the left/right paths and exclusions above. Base, inputs, output and journal directories must be separate. Output must be empty; all three input trees are preserved.")}</p>
            <label class="check-label"><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||two_way.get() on:change=move |ev|{two_way.set(event_target_checked(&ev));preview.set(None);}/>{move ||tr(english.get(),"两方合并（无共同基础）","Two-way merge (no common baseline)")}</label><div class="merge-root-fields"><label>{move ||tr(english.get(),"共同基础版本目录","Common baseline directory")}<span><input disabled=move ||busy.get() ||two_way.get() prop:value=move ||base.get() on:input=move |ev|{base.set(event_target_value(&ev));preview.set(None);}/><button class="quiet" disabled=move ||busy.get() ||two_way.get() on:click=move |_|pick(true)>"…"</button></span></label><label>{move ||tr(english.get(),"独立空输出目录","Separate empty output directory")}<span><input disabled=move ||busy.get() prop:value=move ||output.get() on:input=move |ev|{output.set(event_target_value(&ev));preview.set(None);}/><button class="quiet" disabled=move ||busy.get() on:click=move |_|pick(false)>"…"</button></span></label></div>
            <button class="primary" disabled=move ||busy.get() ||(!two_way.get() &&base.get().is_empty()) ||output.get().is_empty() on:click=move |_|prepare()>{move ||tr(english.get(),"分析并预览目录合并","Analyze and preview folder merge")}</button>
            <Show when=move ||preview.get().is_some()><p class="backup-path">{move ||preview.get().map(|p|format!("{} → {} · {} {} / {} {}",if p.two_way{tr(english.get(),"无共同基础","No common baseline").to_string()}else{p.base},p.output,p.conflicts,tr(english.get(),"个未解决冲突","unresolved conflicts"),p.total,tr(english.get(),"个条目","entries")))}</p><p>{move ||tr(english.get(),"采用某侧的目录版本会应用到其整个子树；省略只影响输出，不会删除输入。","Choosing a directory version applies to its entire subtree. Omission affects output only; input files are never deleted.")}</p>
                <div class="folder-merge-rows"><For each=move ||preview.get().map(|p|p.rows.into_iter().map(|r|((p.id.clone(),render_generation.get()),r)).collect::<Vec<_>>()).unwrap_or_default() key=|(id,row)|(id.clone(),row.id.clone()) children=move |(_,row)|{let id=row.id.clone();let initial=value(&row.resolution);let automatic=row.classification=="auto_merged_text";let combined=row.classification=="combined_directory";view!{<div><span>{row.path}<small>{row.reason}</small></span><select aria-label="Folder merge resolution" disabled=move ||busy.get() prop:value=initial on:change=move |ev|resolve(id.clone(),choice(&event_target_value(&ev)))><Show when=move ||combined><option value="combine">{move ||tr(english.get(),"保留子项选择","Keep child choices")}</option></Show><option value="conflict" disabled=true>{move ||tr(english.get(),"未解决冲突","Unresolved conflict")}</option><option value="left">{move ||tr(english.get(),"采用左侧","Use left")}</option><option value="right">{move ||tr(english.get(),"采用右侧","Use right")}</option><option value="base">{move ||tr(english.get(),"采用基础版本","Use baseline")}</option><option value="delete">{move ||tr(english.get(),"从输出省略","Omit from output")}</option><Show when=move ||automatic><option value="auto">{move ||tr(english.get(),"自动文本合并","Automatic text merge")}</option></Show></select></div>}}/></div>
                <footer><button class="quiet" disabled=move ||busy.get() ||offset.get()==0 on:click=move |_|page(false)>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||preview.get().map(|p|format!("{}–{} / {}",offset.get()+usize::from(p.total>0),(offset.get()+500).min(p.total),p.total))}</span><button class="quiet" disabled=move ||busy.get() ||preview.get().is_none_or(|p|offset.get()+500>=p.total) on:click=move |_|page(true)>{move ||tr(english.get(),"下一页","Next")}</button><button class="primary" disabled=move ||busy.get() ||preview.get().is_none_or(|p|p.conflicts>0) on:click=move |_|apply()>{move ||tr(english.get(),"生成已预览的输出","Create reviewed output")}</button></footer>
            </Show>
        </section></Show>
    }
}
