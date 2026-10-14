//! Binary workspace panel; native operations stay behind typed IPC.
use crate::tr;
use clear_contracts::*;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub binary: RwSignal<Option<BinaryPage>>,
    pub binary_id: RwSignal<String>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    open_binary: impl Fn(String, String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals { binary, binary_id } = signals;
    view! {
        <Show when=move ||binary.get().is_some()><section class="binary-panel"><header><strong>{move ||binary.get().map(|p|format!("{} · {} / {} bytes",p.path,p.left_size,p.right_size))}</strong><button class="quiet" on:click=move |_|binary.set(None)>{move ||tr(english.get(),"关闭二进制预览","Close binary preview")}</button></header><div class="binary-scroll"><For each=move ||binary.get().map(|p|p.lines).unwrap_or_default() key=|l|l.offset.clone() children=move |line|view! {<div class=if line.changed{"binary-line changed"}else{"binary-line"}><span>{format!("{:08X}",line.offset.parse::<u64>().unwrap_or_default())}</span><pre>{line.left_hex}</pre><pre>{line.left_ascii}</pre><pre>{line.right_hex}</pre><pre>{line.right_ascii}</pre></div>}/></div><footer><button class="quiet" disabled=move ||busy.get() ||binary.get().is_none_or(|p|p.offset=="0") on:click=move |_|{if let Some(p)=binary.get_untracked(){let n=p.offset.parse::<u64>().unwrap_or_default().saturating_sub(4096);open_binary(binary_id.get_untracked(),n.to_string());}}>{move ||tr(english.get(),"上一页","Previous")}</button><span>{move ||binary.get().map(|p|format!("Offset {} · 4096 bytes/page",p.offset))}</span><button class="quiet" disabled=move ||busy.get() ||binary.get().is_none_or(|p|{let n=p.offset.parse::<u64>().unwrap_or_default();n.saturating_add(4096)>=p.left_size.parse::<u64>().unwrap_or_default().max(p.right_size.parse::<u64>().unwrap_or_default())}) on:click=move |_|{if let Some(p)=binary.get_untracked() && let Some(n)=p.offset.parse::<u64>().unwrap_or_default().checked_add(4096){open_binary(binary_id.get_untracked(),n.to_string());}}>{move ||tr(english.get(),"下一页","Next")}</button></footer></section></Show>
    }
}
