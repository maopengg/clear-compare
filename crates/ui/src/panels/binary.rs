//! Bounded byte viewing and search; all file reads use opaque native IDs.
use crate::{invoke, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
pub struct Signals {
    pub binary: RwSignal<Option<BinaryPage>>,
    pub binary_id: RwSignal<String>,
    pub comparison: RwSignal<Option<OpenedComparison>>,
    pub error: RwSignal<String>,
}
#[derive(Clone)]
struct Hit {
    offset: u64,
    bytes: usize,
    left: bool,
}
fn hex_cell(
    text: String,
    changed: Vec<usize>,
    offset: u64,
    left: bool,
    hit: RwSignal<Option<Hit>>,
) -> impl IntoView {
    let bytes = text.split_ascii_whitespace().enumerate().map(|(i, byte)| {
        let changed = changed.contains(&i);
        let byte = byte.to_string();
        view! {<span class:byte-changed=changed class:byte-hit=move ||hit.with(|h|h.as_ref().is_some_and(|h|h.left==left && offset.saturating_add(i as u64)>=h.offset && offset.saturating_add(i as u64)<h.offset.saturating_add(h.bytes as u64)))>{byte}" "</span>}
    }).collect_view();
    view! {<pre>{bytes}</pre>}
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    open_binary: impl Fn(String, String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        binary,
        binary_id,
        comparison,
        error,
    } = signals;
    let query = RwSignal::new(String::new());
    let mode = RwSignal::new("hex".to_string());
    let left = RwSignal::new(true);
    let start = RwSignal::new("0".to_string());
    let jump = RwSignal::new("0".to_string());
    let result = RwSignal::<Option<BinarySearchResult>>::new(None);
    let hit = RwSignal::<Option<Hit>>::new(None);
    let scope = Memo::new(move |_| {
        (
            comparison.with(|c| c.as_ref().map(|c| c.session.clone())),
            binary_id.get(),
        )
    });
    Effect::new(move |_| {
        scope.get();
        result.set(None);
        hit.set(None);
        start.set("0".into());
        jump.set("0".into());
    });
    Effect::new(move |_| {
        if let Some(p) = binary.get() {
            if !p.left_available && left.get_untracked() {
                left.set(false);
            } else if !p.right_available && !left.get_untracked() {
                left.set(true);
            }
        }
    });
    Effect::new(move |_| {
        query.get();
        mode.get();
        left.get();
        start.get();
        result.set(None);
        hit.set(None);
    });
    let search = move |continuation: bool| {
        if busy.get_untracked() {
            return;
        }
        let Some(c) = comparison.get_untracked() else {
            return;
        };
        let offset = if continuation {
            let Some(next) = result.get_untracked().and_then(|r| r.next_offset) else {
                return;
            };
            next
        } else {
            start.get_untracked()
        };
        let options = BinarySearchOptions {
            query: query.get_untracked(),
            mode: match mode.get_untracked().as_str() {
                "utf8" => BinarySearchMode::Utf8,
                "utf16le" => BinarySearchMode::Utf16Le,
                "utf16be" => BinarySearchMode::Utf16Be,
                _ => BinarySearchMode::Hex,
            },
            offset,
            limit: 256,
            side: if left.get_untracked() {
                Side::Left
            } else {
                Side::Right
            },
        };
        if let Err(e) = binary_query::pattern(&options)
            .and_then(|_| binary_query::offset(&options.offset).map(|_| ()))
        {
            error.set(e);
            return;
        }
        busy.set(true);
        error.set(String::new());
        result.set(None);
        hit.set(None);
        let id = binary_id.get_untracked();
        spawn_local(async move {
            match invoke::<BinarySearchResult>(
                "binary_search",
                serde_json::json!({"session":c.session,"rowId":id,"options":options}),
            )
            .await
            {
                Ok(r) => result.set(Some(r)),
                Err(e) => error.set(e),
            }
            busy.set(false);
        });
    };
    let navigate = move || match binary_query::offset(&jump.get_untracked()) {
        Ok(n) => open_binary(binary_id.get_untracked(), (n / 4096 * 4096).to_string()),
        Err(e) => error.set(e),
    };
    view! {
        <Show when=move ||binary.get().is_some()>
            <section class="binary-panel">
                <header><strong>{move ||binary.get().map(|p|format!("{} · {} / {} bytes",p.path,p.left_size,p.right_size))}</strong>
                    <button class="quiet" disabled=move ||busy.get() on:click=move |_|{binary.set(None);result.set(None);hit.set(None);}>
                        {move ||tr(english.get(),"关闭二进制预览","Close binary preview")}
                    </button>
                </header>
                <div class="binary-tools">
                    <label>{move ||tr(english.get(),"字节位置","Byte offset")}<input aria-label="Byte offset" prop:value=move ||jump.get() disabled=move ||busy.get() on:input=move |e|jump.set(event_target_value(&e))/></label>
                    <button class="quiet" disabled=move ||busy.get() on:click=move |_|navigate()>{move ||tr(english.get(),"跳转","Go")}</button>
                    <label>{move ||tr(english.get(),"查找侧","Search side")}<select prop:value=move ||if left.get(){"left"}else{"right"} disabled=move ||busy.get() on:change=move |e|left.set(event_target_value(&e)=="left")>
                        <option value="left" disabled=move ||binary.get().is_none_or(|p|!p.left_available)>{move ||tr(english.get(),"左侧","Left")}</option>
                        <option value="right" disabled=move ||binary.get().is_none_or(|p|!p.right_available)>{move ||tr(english.get(),"右侧","Right")}</option>
                    </select></label>
                    <label>{move ||tr(english.get(),"字节编码","Byte encoding")}<select prop:value=move ||mode.get() disabled=move ||busy.get() on:change=move |e|mode.set(event_target_value(&e))>
                        <option value="hex">"Hex"</option><option value="utf8">"UTF-8"</option><option value="utf16le">"UTF-16 LE"</option><option value="utf16be">"UTF-16 BE"</option>
                    </select></label>
                    <label>{move ||tr(english.get(),"查找内容","Find bytes")}<input aria-label="Find bytes" maxlength="16384" prop:value=move ||query.get() disabled=move ||busy.get() on:input=move |e|query.set(event_target_value(&e))/></label>
                    <label>{move ||tr(english.get(),"起始字节","Start offset")}<input aria-label="Search start offset" prop:value=move ||start.get() disabled=move ||busy.get() on:input=move |e|start.set(event_target_value(&e))/></label>
                    <button class="quiet" disabled=move ||busy.get() on:click=move |_|search(false)>{move ||tr(english.get(),"查找","Find")}</button>
                    <button class="quiet" disabled=move ||busy.get() || result.get().is_none_or(|r|r.next_offset.is_none()) on:click=move |_|search(true)>{move ||tr(english.get(),"继续查找","Continue search")}</button>
                </div>
                <p class="binary-note">{move ||tr(english.get(),"位置支持十进制或 0x 十六进制。每次查找最多扫描 64 MiB 候选位置、返回 256 个结果；支持重叠匹配。实时读取并核对大小与修改时间，不代表整个文件内容一致。","Offsets accept decimal or 0x hex. Each search covers up to 64 MiB of candidate offsets and 256 overlapping matches. Live reads check size and modification time; they do not establish whole-file equality.")}</p>
                <Show when=move ||result.get().is_some()>
                    <div class="binary-results" aria-live="polite">
                        <span>{move ||result.get().map(|r|format!("[{}, {}) · {} {} · {}",r.offset,r.searched_until,r.positions.len(),tr(english.get(),"个匹配","matches"),if r.next_offset.is_some(){tr(english.get(),"还有范围待查","More range remains")}else{tr(english.get(),"已查至末尾","Reached end")}))}</span>
                        <For each=move ||result.get().map(|r|r.positions).unwrap_or_default() key=|n|n.clone() children=move |n| {
                            let offset = n.parse::<u64>().unwrap_or_default();
                            view! {<button class="quiet" disabled=move ||busy.get() on:click=move |_|{
                                if let Some(r)=result.get_untracked(){hit.set(Some(Hit{offset,bytes:r.pattern_bytes,left:left.get_untracked()}));open_binary(binary_id.get_untracked(),(offset/4096*4096).to_string());}
                            }>{format!("0x{offset:X}")}</button>}
                        }/>
                    </div>
                </Show>
                <div class="binary-scroll"><For each=move ||binary.get().map(|p|p.lines.into_iter().map(|l|(p.id.clone(),l)).collect::<Vec<_>>()).unwrap_or_default() key=|(id,l)|(id.clone(),l.offset.clone()) children=move |(_,line)| {
                    let offset = line.offset.parse::<u64>().unwrap_or_default();
                    view! {<div class=if line.changed{"binary-line changed"}else{"binary-line"}>
                        <span>{format!("{offset:08X}")}</span>
                        {hex_cell(line.left_hex,line.left_changed,offset,true,hit)}<pre>{line.left_ascii}</pre>
                        {hex_cell(line.right_hex,line.right_changed,offset,false,hit)}<pre>{line.right_ascii}</pre>
                    </div>}
                }/></div>
                <footer>
                    <button class="quiet" disabled=move ||busy.get() ||binary.get().is_none_or(|p|p.offset=="0") on:click=move |_|{if let Some(p)=binary.get_untracked(){let n=p.offset.parse::<u64>().unwrap_or_default().saturating_sub(4096);open_binary(binary_id.get_untracked(),n.to_string());}}>{move ||tr(english.get(),"上一页","Previous")}</button>
                    <span>{move ||binary.get().map(|p|format!("Offset {} · 4096 bytes/page",p.offset))}</span>
                    <button class="quiet" disabled=move ||busy.get() ||binary.get().is_none_or(|p|{let n=p.offset.parse::<u64>().unwrap_or_default();n.saturating_add(4096)>=p.left_size.parse::<u64>().unwrap_or_default().max(p.right_size.parse::<u64>().unwrap_or_default())}) on:click=move |_|{if let Some(p)=binary.get_untracked() && let Some(n)=p.offset.parse::<u64>().unwrap_or_default().checked_add(4096){open_binary(binary_id.get_untracked(),n.to_string());}}>{move ||tr(english.get(),"下一页","Next")}</button>
                </footer>
            </section>
        </Show>
    }
}
