//! Table workspace panel; native operations stay behind typed IPC.
use crate::{label, status_class, tr};
use clear_contracts::*;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub table_visible: RwSignal<bool>,
    pub table: RwSignal<Option<OpenedTable>>,
    pub table_id: RwSignal<String>,
    pub table_tsv: RwSignal<bool>,
    pub table_headers: RwSignal<bool>,
    pub table_key: RwSignal<String>,
    pub table_tolerance: RwSignal<String>,
    pub table_offset: RwSignal<usize>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    open_table: impl Fn(String) + Copy + Send + Sync + 'static,
    paginate_table: impl Fn(bool) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        table_visible,
        table,
        table_id,
        table_tsv,
        table_headers,
        table_key,
        table_tolerance,
        table_offset,
    } = signals;
    view! {
        <Show when=move ||table_visible.get()><section class="table-panel"><header><strong>{move ||table.get().map(|t|t.path)}</strong><button class="quiet" on:click=move |_|{table_visible.set(false);table.set(None);}>{move ||tr(english.get(),"关闭表格预览","Close table preview")}</button></header><div class="table-options"><label><input type="checkbox" prop:checked=move ||table_tsv.get() on:change=move |ev|table_tsv.set(event_target_checked(&ev))/>"TSV"</label><label><input type="checkbox" prop:checked=move ||table_headers.get() on:change=move |ev|table_headers.set(event_target_checked(&ev))/>{move ||tr(english.get(),"首行是表头","Header row")}</label><label>{move ||tr(english.get(),"主键列（从 0 开始，可留空）","Key column (zero-based, optional)")}<input aria-label="Key column" prop:value=move ||table_key.get() on:input=move |ev|table_key.set(event_target_value(&ev))/></label><label>{move ||tr(english.get(),"数值绝对容差（可留空）","Absolute numeric tolerance (optional)")}<input aria-label="Numeric tolerance" prop:value=move ||table_tolerance.get() on:input=move |ev|table_tolerance.set(event_target_value(&ev))/></label><button class="quiet" disabled=move ||busy.get() on:click=move |_|open_table(table_id.get_untracked())>{move ||tr(english.get(),"应用规则重新比较","Compare with these rules")}</button></div><p>{move ||table.get().map(|t|format!("{} {} · {} {} · {} {} · {} {}",tr(english.get(),"一致","Same"),t.report.summary.same,tr(english.get(),"修改","Changed"),t.report.summary.changed,tr(english.get(),"仅左","Left only"),t.report.summary.left_only,tr(english.get(),"仅右","Right only"),t.report.summary.right_only))}</p><Show when=move ||table.get().is_some_and(|t|t.report.left_headers!=t.report.right_headers)><p class="warnings">{move ||tr(english.get(),"两侧表头不同；当前按列位置比较，请检查列顺序。","Headers differ; columns are compared by position. Check column order.")}</p></Show><div class="table-scroll"><table><thead><tr><th>{move ||tr(english.get(),"主键 / 行号","Key / row")}</th><th>{move ||table.get().map(|t|format!("{}: {}",tr(english.get(),"左","Left"),t.report.left_headers.join(" | ")))}</th><th>{move ||table.get().map(|t|format!("{}: {}",tr(english.get(),"右","Right"),t.report.right_headers.join(" | ")))}</th><th>{move ||tr(english.get(),"状态","Status")}</th></tr></thead><tbody><For each=move ||table.get().map(|t|t.report.rows.into_iter().map(|r|(t.id.clone(),r)).collect::<Vec<_>>()).unwrap_or_default() key=|(id,r)|(id.clone(),r.key.clone()) children=move |(_,row)|{let status=row.status.clone();let css=status_class(&status);view!{<tr class=css><td>{row.key}</td><td><div class="table-cells">{row.left.map(|cells|cells.into_iter().enumerate().map(|(i,c)|view!{<span class=if row.changed_columns.contains(&i){"cell changed"}else{"cell"}>{c}</span>}).collect_view())}</div></td><td><div class="table-cells">{row.right.map(|cells|cells.into_iter().enumerate().map(|(i,c)|view!{<span class=if row.changed_columns.contains(&i){"cell changed"}else{"cell"}>{c}</span>}).collect_view())}</div></td><td>{move ||label(&status,english.get())}</td></tr>}}/></tbody></table></div><footer><button class="quiet" disabled=move ||busy.get() ||table_offset.get()==0 on:click=move |_|paginate_table(false)>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||table.get().map(|t|format!("{}–{} / {}",table_offset.get()+usize::from(t.total>0),(table_offset.get()+500).min(t.total),t.total))}</span><button class="quiet" disabled=move ||busy.get() ||table.get().is_none_or(|t|table_offset.get()+500>=t.total) on:click=move |_|paginate_table(true)>{move ||tr(english.get(),"下一页","Next")}</button></footer></section></Show>
    }
}
