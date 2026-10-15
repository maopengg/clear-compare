//! Table workspace panel; native operations stay behind typed IPC.
use crate::{label, status_class, tr};
use clear_contracts::*;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub table_visible: RwSignal<bool>,
    pub table: RwSignal<Option<OpenedTable>>,
    pub table_document: RwSignal<Option<TableDocumentInfo>>,
    pub table_left_mode: RwSignal<String>,
    pub table_right_mode: RwSignal<String>,
    pub table_left_index: RwSignal<String>,
    pub table_right_index: RwSignal<String>,
    pub table_headers: RwSignal<bool>,
    pub table_key: RwSignal<String>,
    pub table_tolerance: RwSignal<String>,
    pub table_offset: RwSignal<usize>,
}

fn display_cell(text: String, english: RwSignal<bool>) -> impl IntoView {
    let end = text.char_indices().nth(8192).map(|(n, _)| n);
    let clipped = end.is_some();
    let shown = if let Some(end) = end {
        text[..end].to_string()
    } else {
        text
    };
    view! {<span>{shown}</span>{clipped.then(||view!{<small class="cell-clipped">{move ||tr(english.get(),"截断","Clipped")}</small>})}}
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    compare: impl Fn() + Copy + Send + Sync + 'static,
    paginate_table: impl Fn(bool) + Copy + Send + Sync + 'static,
    catalog: impl Fn() + Copy + Send + Sync + 'static,
    close: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        table_visible,
        table,
        table_document,
        table_left_mode,
        table_right_mode,
        table_left_index,
        table_right_index,
        table_headers,
        table_key,
        table_tolerance,
        table_offset,
    } = signals;

    let column = RwSignal::new(0usize);
    let columns = Memo::new(move |_| {
        table.with(|t| t.as_ref().map(|t| t.report.columns).unwrap_or_default())
    });
    Effect::new(move |_| {
        let n = columns.get();
        if column.get_untracked() >= n {
            column.set(n.saturating_sub(1) / 25 * 25);
        }
    });
    let html_mode = move |left: bool| {
        let mode = if left {
            table_left_mode.get()
        } else {
            table_right_mode.get()
        };
        mode == "html"
            || (mode == "auto"
                && table_document.with(|d| {
                    d.as_ref()
                        .is_some_and(|d| if left { d.left_html } else { d.right_html })
                }))
    };
    view! {
            <Show when=move ||table_visible.get()><section class="table-panel"><header><strong>{move ||table_document.with(|d|d.as_ref().map(|d|d.path.clone()))}</strong><button class="quiet" disabled=move ||busy.get() on:click=move |_|close()>{move ||tr(english.get(),"关闭表格预览","Close table preview")}</button></header><p>{move ||tr(english.get(),"比较使用原生端固定的原文；更改规则不会重读文件。需要刷新时，请重新打开目录列表中的表格。HTML 仅读取静态单元格文本。","Rules use frozen native source text without rereading files. Reopen from the file list to refresh. HTML uses static cell text only.")}</p>
    <div class="table-source-options">
    <label>{move ||tr(english.get(),"左侧格式","Left format")}<select disabled=move ||busy.get() prop:value=move ||table_left_mode.get() on:change=move |ev|{table_left_mode.set(event_target_value(&ev));table.set(None);}><option value="auto">{move ||tr(english.get(),"按扩展名","By extension")}</option><option value="csv">"CSV"</option><option value="tsv">"TSV"</option><option value="html">"HTML"</option></select></label>
    <label>{move ||tr(english.get(),"左侧表序号","Left table index")}<input disabled=move ||busy.get() type="number" min="0" max="63" prop:value=move ||table_left_index.get() on:input=move |ev|{table_left_index.set(event_target_value(&ev));table_left_mode.set("html".into());table.set(None);}/></label>
    <label>{move ||tr(english.get(),"右侧格式","Right format")}<select disabled=move ||busy.get() prop:value=move ||table_right_mode.get() on:change=move |ev|{table_right_mode.set(event_target_value(&ev));table.set(None);}><option value="auto">{move ||tr(english.get(),"按扩展名","By extension")}</option><option value="csv">"CSV"</option><option value="tsv">"TSV"</option><option value="html">"HTML"</option></select></label>
    <label>{move ||tr(english.get(),"右侧表序号","Right table index")}<input disabled=move ||busy.get() type="number" min="0" max="63" prop:value=move ||table_right_index.get() on:input=move |ev|{table_right_index.set(event_target_value(&ev));table_right_mode.set("html".into());table.set(None);}/></label>
    <button class="quiet" disabled=move ||busy.get() ||table_document.with(|d|d.is_none()) on:click=move |_|catalog()>{move ||tr(english.get(),"列出 HTML 表格","List HTML tables")}</button></div>
    <Show when=move ||html_mode(true)||html_mode(false)><div class="html-table-catalog"><div><Show when=move ||html_mode(true)><strong>{move ||tr(english.get(),"左侧 HTML 表格（序号从 0 开始）","Left HTML tables (zero-based)")}</strong><p>{move ||table_document.with(|d|d.as_ref().and_then(|d|d.left_error.clone()))}</p><For each=move ||table_document.get().map(|d|d.left_tables).unwrap_or_default() key=|t|t.index children=move |info|view!{<button class="quiet" disabled=move ||busy.get() on:click=move |_|{table_left_index.set(info.index.to_string());table_left_mode.set("html".into());table.set(None);}>{move ||format!("{} · {} · #{} · {} {}",info.index,info.caption,info.element_id,info.rows,tr(english.get(),"DOM 行","DOM rows"))}</button>}/></Show></div><div><Show when=move ||html_mode(false)><strong>{move ||tr(english.get(),"右侧 HTML 表格（序号从 0 开始）","Right HTML tables (zero-based)")}</strong><p>{move ||table_document.with(|d|d.as_ref().and_then(|d|d.right_error.clone()))}</p><For each=move ||table_document.get().map(|d|d.right_tables).unwrap_or_default() key=|t|t.index children=move |info|view!{<button class="quiet" disabled=move ||busy.get() on:click=move |_|{table_right_index.set(info.index.to_string());table_right_mode.set("html".into());table.set(None);}>{move ||format!("{} · {} · #{} · {} {}",info.index,info.caption,info.element_id,info.rows,tr(english.get(),"DOM 行","DOM rows"))}</button>}/></Show></div></div></Show>
    <div class="table-options"><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||table_headers.get() on:change=move |ev|{table_headers.set(event_target_checked(&ev));table.set(None);}/>{move ||tr(english.get(),"首行是表头","Header row")}</label><label>{move ||tr(english.get(),"主键列（从 0 开始，可留空）","Key column (zero-based, optional)")}<input disabled=move ||busy.get() aria-label="Key column" prop:value=move ||table_key.get() on:input=move |ev|{table_key.set(event_target_value(&ev));table.set(None);}/></label><label>{move ||tr(english.get(),"数值绝对容差（可留空）","Absolute numeric tolerance (optional)")}<input disabled=move ||busy.get() aria-label="Numeric tolerance" prop:value=move ||table_tolerance.get() on:input=move |ev|{table_tolerance.set(event_target_value(&ev));table.set(None);}/></label><button class="quiet" disabled=move ||busy.get() ||table_document.with(|d|d.is_none()) on:click=move |_|compare()>{move ||tr(english.get(),"应用规则重新比较","Compare with these rules")}</button></div><p>{move ||table.get().map(|t|format!("{} {} · {} {} · {} {} · {} {}",tr(english.get(),"一致","Same"),t.report.summary.same,tr(english.get(),"修改","Changed"),t.report.summary.changed,tr(english.get(),"仅左","Left only"),t.report.summary.left_only,tr(english.get(),"仅右","Right only"),t.report.summary.right_only))}</p><Show when=move ||table.get().is_some_and(|t|t.report.left_headers!=t.report.right_headers)><p class="warnings">{move ||tr(english.get(),"两侧表头不同；当前按列位置比较，请检查列顺序。","Headers differ; columns are compared by position. Check column order.")}</p></Show><p class="warnings">{move ||table.with(|t|t.as_ref().map(|t|t.report.warnings.join(" · ")))}</p><div class="table-column-pages"><button class="quiet" disabled=move ||busy.get() ||column.get()==0 on:click=move |_|column.update(|c|*c=c.saturating_sub(25))>{move ||tr(english.get(),"前 25 列","Previous 25 columns")}</button><span>{move ||format!("{} {}–{} / {}",tr(english.get(),"列","Columns"),column.get()+usize::from(columns.get()>0),(column.get()+25).min(columns.get()),columns.get())}</span><button class="quiet" disabled=move ||busy.get() ||(column.get()+25>=columns.get()) on:click=move |_|column.update(|c|*c+=25)>{move ||tr(english.get(),"后 25 列","Next 25 columns")}</button></div><p>{move ||tr(english.get(),"每页最多显示 25 列；长单元格显示前 8192 个字符并标注截断，原始数据保留在比较结果中。","At most 25 columns per page. Long cells show the first 8192 characters with a clipping label; original result values are retained.")}</p><div class="table-scroll"><table><thead><tr><th>{move ||tr(english.get(),"主键 / 行号","Key / row")}</th><th>{move ||table.get().map(|t|format!("{}: {}",tr(english.get(),"左","Left"),t.report.left_headers.iter().skip(column.get()).take(25).cloned().collect::<Vec<_>>().join(" | ")))}</th><th>{move ||table.get().map(|t|format!("{}: {}",tr(english.get(),"右","Right"),t.report.right_headers.iter().skip(column.get()).take(25).cloned().collect::<Vec<_>>().join(" | ")))}</th><th>{move ||tr(english.get(),"状态","Status")}</th></tr></thead><tbody><For each=move ||table.get().map(|t|t.report.rows.into_iter().map(|r|(t.id.clone(),column.get(),r)).collect::<Vec<_>>()).unwrap_or_default() key=|(id,col,r)|(id.clone(),*col,r.key.clone()) children=move |(_,_,row)|{let status=row.status.clone();let css=status_class(&status);view!{<tr class=css><td>{row.key}</td><td><div class="table-cells">{row.left.map(|cells|cells.into_iter().enumerate().skip(column.get_untracked()).take(25).map(|(i,c)|view!{<span class=if row.changed_columns.contains(&i){"cell changed"}else{"cell"} title=move ||format!("{} {}",tr(english.get(),"列","Column"),i)>{display_cell(c,english)}</span>}).collect_view())}</div></td><td><div class="table-cells">{row.right.map(|cells|cells.into_iter().enumerate().skip(column.get_untracked()).take(25).map(|(i,c)|view!{<span class=if row.changed_columns.contains(&i){"cell changed"}else{"cell"} title=move ||format!("{} {}",tr(english.get(),"列","Column"),i)>{display_cell(c,english)}</span>}).collect_view())}</div></td><td>{move ||label(&status,english.get())}</td></tr>}}/></tbody></table></div><footer><button class="quiet" disabled=move ||busy.get() ||table.with(|t|t.is_none()) ||table_offset.get()==0 on:click=move |_|paginate_table(false)>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||table.get().map(|t|format!("{}–{} / {}",table_offset.get()+usize::from(t.total>0),(table_offset.get()+500).min(t.total),t.total))}</span><button class="quiet" disabled=move ||busy.get() ||table.get().is_none_or(|t|table_offset.get()+500>=t.total) on:click=move |_|paginate_table(true)>{move ||tr(english.get(),"下一页","Next")}</button></footer></section></Show>
        }
}
