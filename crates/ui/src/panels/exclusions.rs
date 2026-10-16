//! Editable project-local exclusion patterns, committed explicitly.
use crate::tr;
use leptos::prelude::*;
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    visible: RwSignal<bool>,
    rules: RwSignal<String>,
    apply: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let draft = RwSignal::new(String::new());
    let pattern = RwSignal::new(String::new());
    Effect::watch(
        move || visible.get(),
        move |open, _, _| {
            if *open {
                draft.set(rules.get_untracked());
                pattern.set(String::new());
            }
        },
        false,
    );
    let add = move || {
        let value = pattern.get_untracked();
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        let rule = if value.starts_with('-') {
            value.to_owned()
        } else {
            format!("-{value}")
        };
        let mut values = draft
            .get_untracked()
            .split([';', '\n'])
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !values.contains(&rule) {
            values.push(rule);
        }
        draft.set(values.join(";\n"));
        pattern.set(String::new());
    };
    view! {<Show when=move||visible.get()><div class="modal-backdrop exclusions-backdrop"><section class="exclusions-dialog" role="dialog" aria-modal="true" aria-label=move||tr(english.get(),"项目排除规则","Project exclusions")>
        <header><strong>{move||tr(english.get(),"项目排除规则","Project exclusions")}</strong><button class="quiet" disabled=move||busy.get() on:click=move|_|visible.set(false)>{move||tr(english.get(),"取消","Cancel")}</button></header>
        <p>{move||tr(english.get(),"只影响当前项目的比较和同步，不删除磁盘文件。./ 表示从比较根目录匹配，目录以 / 结尾；支持 *.log 等通配符。","Applies only to this project’s comparison and synchronization; no files are deleted. ./ anchors at the comparison root, / marks a folder; wildcards such as *.log are supported.")}</p>
        <div class="exclusion-add"><input aria-label=move||tr(english.get(),"新增排除规则","New exclusion rule") placeholder="*.log / .venv/ / ./docs/" disabled=move||busy.get() prop:value=move||pattern.get() on:input=move|ev|pattern.set(event_target_value(&ev)) on:keydown=move|ev|if ev.key()=="Enter"&&!ev.is_composing(){ev.prevent_default();add();}/><button class="quiet" disabled=move||busy.get()||pattern.get().trim().is_empty() on:click=move|_|add()>{move||tr(english.get(),"添加","Add")}</button></div>
        <div class="exclusion-list">{move||draft.get().split([';','\n']).map(str::trim).filter(|r|!r.is_empty()).enumerate().map(|(index,rule)|{let rule=rule.to_owned();let aria=rule.clone();view!{<div class="exclusion-item"><code>{rule}</code><button class="quiet" disabled=move||busy.get() aria-label=move||format!("{} {aria}",tr(english.get(),"移除排除规则","Remove exclusion")) on:click=move|_|{let values=draft.get_untracked().split([';','\n']).map(str::trim).filter(|r|!r.is_empty()).enumerate().filter(|(i,_)|*i!=index).map(|(_,r)|r.to_owned()).collect::<Vec<_>>();draft.set(values.join(";\n"));}>{move||tr(english.get(),"移除","Remove")}</button></div>}}).collect_view()}</div>
        <details><summary>{move||tr(english.get(),"批量编辑规则","Edit all patterns")}</summary><textarea aria-label="Exclusion rule draft" disabled=move||busy.get() prop:value=move||draft.get() on:input=move|ev|draft.set(event_target_value(&ev))/></details>
        <footer><span>{move||tr(english.get(),"应用后保存到当前项目；已有比较会按新规则重新扫描。","Applying saves to the current project and rescans an existing comparison.")}</span><button class="primary" disabled=move||busy.get() on:click=move|_|apply(draft.get_untracked())>{move||tr(english.get(),"应用并保存","Apply and save")}</button></footer>
    </section></div></Show>}
}
