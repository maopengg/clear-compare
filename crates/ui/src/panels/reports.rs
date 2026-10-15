//! Export full native comparison snapshots rather than the visible page.
use crate::{invoke, tr};
use clear_contracts::{ReportFormat, ReportRequest, ReportResult, ReportSource};
use leptos::{prelude::*, task::spawn_local};
pub fn controls(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    context: impl Fn() -> Option<(ReportSource, String)> + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let format = RwSignal::new(ReportFormat::Html);
    let differences = RwSignal::new(false);
    let notice = RwSignal::new(String::new());
    let export = move || {
        let Some((source, session)) = context() else {
            return;
        };
        let format = format.get_untracked();
        let differences_only = differences.get_untracked();
        let en = english.get_untracked();
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match invoke::<Option<String>>(
                "pick_report_destination",
                serde_json::json!({"format":format}),
            )
            .await
            {
                Ok(Some(destination)) => {
                    let request = ReportRequest {
                        source,
                        format,
                        session,
                        destination,
                        english: en,
                        differences_only,
                    };
                    match invoke::<ReportResult>(
                        "export_report",
                        serde_json::json!({"request":request}),
                    )
                    .await
                    {
                        Ok(result) => {
                            let _ = notice.try_set(format!(
                                "{}: {} · {} {} · {} B",
                                tr(en, "报告已保存", "Report saved"),
                                result.path,
                                result.rows,
                                tr(en, "行", "rows"),
                                result.bytes
                            ));
                        }
                        Err(e) => error.set(e),
                    }
                }
                Ok(None) => {}
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    view! {<div class="report-controls"><label>{move ||tr(english.get(),"报告格式","Report format")}<select disabled=move ||busy.get() prop:value=move ||match format.get(){ReportFormat::Html=>"html",ReportFormat::Json=>"json"} on:change=move |ev|format.set(if event_target_value(&ev)=="json"{ReportFormat::Json}else{ReportFormat::Html})><option value="html">"HTML"</option><option value="json">"JSON"</option></select></label><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||differences.get() on:change=move |ev|differences.set(event_target_checked(&ev))/>{move ||tr(english.get(),"仅导出差异（含未验证/错误）","Differences only (including unverified/errors)")}</label><button class="quiet" disabled=move ||busy.get() ||context().is_none() on:click=move |_|export()>{move ||tr(english.get(),"导出完整比较报告…","Export comparison report…")}</button><span>{move ||tr(english.get(),"不受当前搜索/分页影响；保存到来源目录外的新文件。","Independent of search/page; choose a new file outside source directories.")}</span></div><Show when=move ||!notice.get().is_empty()><p class="report-notice" role="status">{move ||notice.get()}</p></Show>}
}
