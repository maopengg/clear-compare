//! Three-way materials share the source tab's sole editable buffer and undo history.
use super::*;
use clear_contracts::merge_edit::{self, Choice};
use std::sync::Arc;
fn update(s: Controller, id: &str, text: &str) -> bool {
    if s.c.busy.get_untracked() {
        return false;
    }
    match replace(id, &normalize(text)) {
        Ok(()) => {
            s.c.error.set(String::new());
            true
        }
        Err(_) => {
            s.c.error.set(
                tr(
                    s.english.get_untracked(),
                    "无法更新结果编辑器。",
                    "Unable to update the result editor.",
                )
                .into(),
            );
            false
        }
    }
}
fn choose(s: Controller, id: &str, material: &ConflictDocument, block: usize, choice: Choice) {
    let Some(tab) = s.tab(id) else {
        return;
    };
    let Some(merge) = &material.suggestion else {
        return;
    };
    let Some(token) = &merge.conflict_token else {
        return;
    };
    match merge_edit::resolve(&tab.draft.get_untracked(),token,&merge.conflict_versions,block,choice) {
        Ok(text)=>{update(s,id,&text);},
        Err(_)=>s.c.error.set(tr(s.english.get_untracked(),"此块的标记或内容已手动修改。请撤销该修改后再选择版本，或继续手动编辑。","This block was manually edited. Undo that edit before choosing a version, or finish it manually.").into())
    }
}
fn snippet(text: &str) -> String {
    let mut end = text.len().min(16000);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let value = text[..end].lines().take(80).collect::<Vec<_>>().join("\n");
    if value.len() < text.trim_end_matches(['\n', '\r']).len() {
        format!("{value}\n…")
    } else {
        value
    }
}
fn version(s: Controller, id: String, material: Arc<ConflictDocument>, side: u8) -> AnyView {
    let version = match side {
        1 => &material.base,
        2 => &material.current,
        _ => &material.incoming,
    };
    let label = move || match side {
        1 => tr(s.english.get(), "共同基础", "Base"),
        2 => tr(s.english.get(), "当前版本", "Current"),
        _ => tr(s.english.get(), "传入版本", "Incoming"),
    };
    let text = version.as_ref().and_then(|v| v.text.clone());
    let hash = version
        .as_ref()
        .map(|v| v.object.chars().take(10).collect::<String>())
        .unwrap_or_default();
    let use_text = text.clone();
    view!{<section class="conflict-version"><header><strong>{label}</strong><code title=version.as_ref().map(|v|v.object.clone()).unwrap_or_default()>{hash}</code>{use_text.map(move |text|view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|{update(s,&id,&text);}>{move ||tr(s.english.get(),"采用整侧","Use entire version")}</button>})}</header><pre tabindex="0" aria-label=label>{move ||text.clone().unwrap_or_else(||tr(s.english.get(),"该阶段不存在此文件。","This file is absent in this stage.").into())}</pre></section>}.into_any()
}
fn details(s: Controller, id: String, material: ConflictDocument) -> AnyView {
    let Some(tab) = s.tab(&id) else {
        return ().into_any();
    };
    let material = Arc::new(material);
    let rebase = material.operation.as_deref() == Some("rebase");
    let expanded = RwSignal::new(true);
    let generated = RwSignal::new(false);
    let index = RwSignal::new(0usize);
    let merge = material.clone();
    let blocks = Memo::new(move |_| {
        let draft = tab.draft.get();
        merge
            .suggestion
            .as_ref()
            .and_then(|m| m.conflict_token.as_ref())
            .map(|token| merge_edit::blocks(&draft, token))
            .unwrap_or_else(|| Ok(Vec::new()))
    });
    Effect::new(move |_| {
        let count = blocks.with(|v| v.as_ref().map(Vec::len).unwrap_or(0));
        if index.get_untracked() >= count {
            index.set(count.saturating_sub(1));
        }
    });
    let generate = material.clone();
    let generate_id = id.clone();
    let save_id = id.clone();
    let versions = [1, 2, 3]
        .into_iter()
        .map(|side| version(s, id.clone(), material.clone(), side))
        .collect_view();
    let current_id = id.clone();
    let block_material = material.clone();
    view!{<section class="source-conflict" aria-label=move ||tr(s.english.get(),"Git 三方冲突","Git three-way conflict")>
        <header class="conflict-toolbar"><strong title=material.path.clone()>{move ||tr(s.english.get(),"三方冲突","Three-way conflict")}</strong><span class="conflict-caption">{move ||if rebase{tr(s.english.get(),"变基：当前版本是目标分支，传入版本是正在应用的提交。","Rebase: current is the target; incoming is the commit being replayed.")}else{tr(s.english.get(),"结果与普通编辑标签共用缓冲区，可撤销；保存不会自动继续 Git 操作。","Result shares the source buffer and undo history. Saving does not continue Git automatically.")}}</span><button class="quiet" on:click=move |_|expanded.update(|v|*v = !*v)>{move ||if expanded.get(){tr(s.english.get(),"折叠材料","Collapse versions")}else{tr(s.english.get(),"展开材料","Expand versions")}}</button><button class="primary" disabled=move ||s.c.busy.get() on:click=move |_|s.save_mode(save_id.clone(),false,true)>{move ||tr(s.english.get(),"保存并标记已解决","Save and mark resolved")}</button></header>
        <div class="conflict-versions" style:display=move ||if expanded.get(){"grid"}else{"none"}>{versions}</div>
        <div class="conflict-toolbar"><button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|{if let Some(merge)=&generate.suggestion && update(s,&generate_id,&merge.text){generated.set(true);index.set(0);}}>{move ||tr(s.english.get(),"生成三方草稿","Generate three-way draft")}</button><span role="status">{move ||if !generated.get(){tr(s.english.get(),"可直接编辑原结果，或生成草稿后逐块选择。","Edit the existing result, or generate a draft for block choices.").to_string()}else{match blocks.get(){Ok(v)=>format!("{} {}",v.len(),tr(s.english.get(),"个三方标记块；保存前请检查结果","tagged blocks; review before saving")),Err(_)=>tr(s.english.get(),"标记已手动修改，请手动完成或撤销。","Markers were edited; finish manually or undo.").into()}}}</span>
            <button class="quiet" disabled=move ||index.get()==0 on:click=move |_|index.update(|i|*i=i.saturating_sub(1))>{move ||tr(s.english.get(),"上一冲突","Previous conflict")}</button><button class="quiet" disabled=move ||blocks.get().map(|v|index.get().saturating_add(1)>=v.len()).unwrap_or(true) on:click=move |_|index.update(|i|*i+=1)>{move ||tr(s.english.get(),"下一冲突","Next conflict")}</button>
        </div>
        {move ||{
            let remaining=blocks.get().unwrap_or_default();
            let Some(block)=remaining.get(index.get().min(remaining.len().saturating_sub(1))) else{return ().into_any();};
            let block=block.clone();
            let Some(conflict)=block_material.suggestion.as_ref().and_then(|m|m.conflict_versions.iter().find(|c|c.id==block.id)) else{return ().into_any();};
            let choices=[(Choice::Left,"采用当前","Use current"),(Choice::Base,"采用基础","Use base"),(Choice::Right,"采用传入","Use incoming"),(Choice::Both,"保留双方","Keep both")];
            let buttons=choices.into_iter().map(|(choice,zh,en)|{let m=block_material.clone();let id=current_id.clone();view!{<button class="quiet" disabled=move ||s.c.busy.get() on:click=move |_|choose(s,&id,&m,block.id,choice)>{move ||tr(s.english.get(),zh,en)}</button>}}).collect_view();
            let focus_id=current_id.clone();let range=block.range.clone();
            view!{<details class="conflict-block"><summary>{format!("{} {} · {} {}",tr(s.english.get(),"冲突块","Conflict"),block.id+1,tr(s.english.get(),"结果行","Result line"),block.line)}</summary><div class="conflict-block-text"><div><small>{tr(s.english.get(),"共同基础","Base")}</small><pre>{snippet(&conflict.base)}</pre></div><div><small>{tr(s.english.get(),"当前版本","Current")}</small><pre>{snippet(&conflict.left)}</pre></div><div><small>{tr(s.english.get(),"传入版本","Incoming")}</small><pre>{snippet(&conflict.right)}</pre></div></div></details><div class="conflict-toolbar conflict-choices">{buttons}<button class="quiet" on:click=move |_|{if let Some(tab)=s.tab(&focus_id){let draft=tab.draft.get_untracked();if range.end<=draft.len()&&draft.is_char_boundary(range.start)&&draft.is_char_boundary(range.end){navigate(&focus_id,draft[..range.start].encode_utf16().count(),draft[range.clone()].encode_utf16().count());}}}>{move ||tr(s.english.get(),"定位结果","Locate in result")}</button></div>}.into_any()
        }}
    </section>}.into_any()
}
pub(super) fn panel(s: Controller) -> impl IntoView {
    view! {<div class="source-conflict-slot">{move ||{
        let id=s.active.get();
        s.conflicts.with(|m|m.get(&id).cloned()).map(|m|details(s,id,m))
    }}</div>}
}

pub(super) fn describe(en: bool, error: String) -> String {
    if en {
        return error;
    }
    let message = if error.contains("Conflict stages or Git operation changed") {
        "冲突版本或 Git 操作已变化。请重新载入文件并检查 Git 状态；现有草稿仍保留。"
    } else if error.contains("Saved conflict result changed") {
        "已保存的结果在外部发生变化，请检查后重新保存或载入。"
    } else if error.contains("Unresolved conflict markers remain") {
        "结果中仍有冲突标记，请完成选择或手动编辑后再标记已解决。"
    } else if error.contains("Save the conflict result before") {
        "请先保存合并结果，再标记已解决。"
    } else if error.contains("Save or reload the existing source buffer") {
        "请先保存或重新载入已打开的文件，再进入三方冲突编辑。"
    } else {
        return error;
    };
    if let Some((_, path)) = error.split_once("Conflict recovery: ") {
        format!("{message}\n恢复材料：{path}")
    } else {
        message.into()
    }
}
