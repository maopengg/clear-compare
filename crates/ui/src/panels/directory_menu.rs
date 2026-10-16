//! File-specific context actions. Mutations use the existing native preview workflow.
use super::directory::Signals;
use crate::{label, size, tr};
use clear_contracts::*;
use leptos::{prelude::*, task::spawn_local};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "
let previous;
export function directoryAnchor(event,keyboard){
 const cell=event.target.closest('td');const rect=event.target.getBoundingClientRect();
 previous=cell?.parentElement.querySelectorAll('.entry-name')[cell.dataset.side==='right'?1:0]||event.target;
 return [cell&&cell.dataset.side==='right'?1:0,keyboard?rect.left:event.clientX,keyboard?rect.bottom:event.clientY];
}
export function focusDirectoryMenu(){
 requestAnimationFrame(()=>{const menu=document.querySelector('.file-context-menu');if(!menu)return;
 const rect=menu.getBoundingClientRect();menu.style.left=Math.max(4,Math.min(rect.left,innerWidth-rect.width-4))+'px';menu.style.top=Math.max(4,Math.min(rect.top,innerHeight-rect.height-4))+'px';
 (menu.querySelector('input')||menu.querySelector('button:not(:disabled)'))?.focus();});
}
export function restoreDirectoryFocus(){if(previous?.isConnected)previous.focus();}
export function directoryMenuKey(event){
 if(event.target.matches('input'))return;
 const items=[...event.target.closest('.file-context-menu').querySelectorAll('button:not(:disabled)')];const index=items.indexOf(document.activeElement);
 let next;if(event.key==='ArrowDown')next=(index+1)%items.length;else if(event.key==='ArrowUp')next=(index+items.length-1)%items.length;else if(event.key==='Home')next=0;else if(event.key==='End')next=items.length-1;else return;
 event.preventDefault();items[next]?.focus();
}
export async function copyDirectoryText(value){
 if(navigator.clipboard?.writeText){await navigator.clipboard.writeText(value);return;}
 const input=document.createElement('textarea');input.value=value;input.style.cssText='position:fixed;left:-10000px';document.body.append(input);input.select();
 try{if(!document.execCommand('copy'))throw new Error('Clipboard unavailable');}finally{input.remove();}
}
")]
extern "C" {
    #[wasm_bindgen(js_name=directoryAnchor)]
    fn anchor(event: &JsValue, keyboard: bool) -> js_sys::Array;
    #[wasm_bindgen(js_name=focusDirectoryMenu)]
    pub fn focus_menu();
    #[wasm_bindgen(js_name=restoreDirectoryFocus)]
    fn restore_focus();
    #[wasm_bindgen(js_name=directoryMenuKey)]
    fn menu_key(event: &JsValue);
    #[wasm_bindgen(js_name=copyDirectoryText)]
    fn copy_text(value: &str) -> js_sys::Promise;
}
#[derive(Clone)]
pub struct Target {
    row: Row,
    right: bool,
    x: f64,
    y: f64,
}
impl Target {
    fn from_event(row: Row, event: &JsValue, keyboard: bool) -> Self {
        let a = anchor(event, keyboard);
        Self {
            row,
            right: a.get(0).as_f64() == Some(1.0),
            x: a.get(1).as_f64().unwrap_or(8.0),
            y: a.get(2).as_f64().unwrap_or(8.0),
        }
    }
    pub fn mouse(row: Row, event: &web_sys::MouseEvent) -> Self {
        Self::from_event(row, event.as_ref(), false)
    }
    pub fn keyboard(row: Row, event: &web_sys::KeyboardEvent) -> Self {
        Self::from_event(row, event.as_ref(), true)
    }
    fn side(&self) -> Side {
        if self.right { Side::Right } else { Side::Left }
    }
}
#[derive(Clone, Copy)]
enum Action {
    Open,
    Edit,
    Binary,
    Table,
    Picture,
    Copy,
    Rename,
    Delete,
    Name,
    Properties,
}
impl Action {
    fn title(self, en: bool, right: bool, directory: bool) -> &'static str {
        match self {
            Self::Open => {
                if directory {
                    tr(en, "展开 / 折叠目录", "Expand / collapse folder")
                } else {
                    tr(en, "比较内容", "Compare contents")
                }
            }
            Self::Edit => {
                if right {
                    tr(en, "编辑右侧文件", "Edit right file")
                } else {
                    tr(en, "编辑左侧文件", "Edit left file")
                }
            }
            Self::Binary => tr(en, "十六进制比较", "Hex comparison"),
            Self::Table => tr(en, "表格比较", "Table comparison"),
            Self::Picture => tr(en, "图片比较", "Picture comparison"),
            Self::Copy => {
                if right {
                    tr(en, "复制到左侧…", "Copy to left…")
                } else {
                    tr(en, "复制到右侧…", "Copy to right…")
                }
            }
            Self::Rename => tr(en, "重命名…", "Rename…"),
            Self::Delete => {
                if right {
                    tr(en, "删除右侧文件…", "Delete right file…")
                } else {
                    tr(en, "删除左侧文件…", "Delete left file…")
                }
            }
            Self::Name => tr(en, "复制文件名", "Copy file name"),
            Self::Properties => tr(en, "属性", "Properties"),
        }
    }
}
pub fn panel(
    s: Signals,
    target: RwSignal<Option<Target>>,
    toggle: impl Fn(String) + Copy + Send + Sync + 'static,
    open: impl Fn(String) + Copy + Send + Sync + 'static,
    edit: impl Fn(String, Side) + Copy + Send + Sync + 'static,
    inspect: impl Fn(String, &'static str) + Copy + Send + Sync + 'static,
    file_action: impl Fn(String, Side, &'static str, String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let rename = RwSignal::new(false);
    let properties = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let clipboard_error = RwSignal::new(false);
    Effect::watch(
        move || target.get().map(|t| (t.row.id, t.right)),
        move |_, _, _| {
            rename.set(false);
            properties.set(false);
            clipboard_error.set(false);
        },
        false,
    );
    let close = move || {
        target.set(None);
        restore_focus();
    };
    view! { {move ||target.get().map(|t|{
        let right=t.right;let row=t.row.clone();let filename=row.path.rsplit('/').next().unwrap_or(&row.path).to_string();
        let entry=if right {row.right.as_ref()} else {row.left.as_ref()};
        let directory=entry.is_some_and(|e|e.kind==Kind::Directory);
        let file=entry.is_some_and(|e|e.kind==Kind::File && e.error.is_none());
        let opposite=if right {row.left.as_ref()}else{row.right.as_ref()};
        let paired_files=file && opposite.is_none_or(|e|e.kind==Kind::File && e.error.is_none());
        let copy=paired_files && matches!(row.status,Status::Changed|Status::LeftOnly|Status::RightOnly);
        let stored=StoredValue::new(t.clone());
        view!{
            <div class="file-context-backdrop" on:pointerdown=move |_|close() on:contextmenu=move |ev|{ev.prevent_default();close();}></div>
            <div class="file-context-menu" role="menu" aria-label=move ||tr(s.english.get(),"文件操作","File actions") style=format!("left:{}px;top:{}px",t.x,t.y) on:contextmenu=move |ev|ev.prevent_default() on:keydown=move |ev|{if ev.key()=="Escape" ||ev.key()=="Tab"{ev.prevent_default();close();}else{menu_key(ev.as_ref());}}>
                <div class="file-context-heading">{if right {tr(s.english.get_untracked(),"右侧","Right")}else{tr(s.english.get_untracked(),"左侧","Left")}}" · "{filename.clone()}</div>
                { [Action::Open,Action::Edit,Action::Binary,Action::Table,Action::Picture,Action::Copy,Action::Rename,Action::Delete,Action::Name,Action::Properties].into_iter().map(move |action|{
                    let disabled=match action {Action::Open=>!directory&&!paired_files,Action::Edit|Action::Rename|Action::Delete=>!file,Action::Binary|Action::Table|Action::Picture=>!paired_files,Action::Copy=>!copy,_=>false};
                    view!{<button role="menuitem" disabled=disabled class=if matches!(action,Action::Copy|Action::Name){"context-group-start"}else{""} on:click=move |_|{
                        let t=stored.get_value();let side=t.side();let id=t.row.id;
                        match action {
                            Action::Rename=>{name.set(t.row.path.rsplit('/').next().unwrap_or(&t.row.path).into());rename.set(true);properties.set(false);focus_menu();},
                            Action::Properties=>{properties.set(true);rename.set(false);focus_menu();},
                            Action::Name=>{let value=t.row.path.rsplit('/').next().unwrap_or(&t.row.path).to_string();spawn_local(async move {if wasm_bindgen_futures::JsFuture::from(copy_text(&value)).await.is_ok(){close();}else{clipboard_error.set(true);}});},
                            _=>{close();match action {Action::Open=>if directory{toggle(id)}else{open(id)},Action::Edit=>edit(id,side),Action::Binary=>inspect(id,"binary"),Action::Table=>inspect(id,"table"),Action::Picture=>inspect(id,"picture"),Action::Copy=>file_action(id,side,"copy",String::new()),Action::Delete=>file_action(id,side,"delete",String::new()),_=>{}}}
                        }
                    }>{move ||action.title(s.english.get(),right,directory)}</button>}
                }).collect_view()}
                <Show when=move ||rename.get()><div class="context-detail"><label>{move ||tr(s.english.get(),"新文件名","New file name")}<input aria-label=move ||tr(s.english.get(),"新文件名","New file name") prop:value=move ||name.get() on:input=move |ev|name.set(event_target_value(&ev))/></label><button role="menuitem" disabled=move ||name.get().trim().is_empty() on:click=move |_|{let t=stored.get_value();let side=t.side();let value=name.get_untracked();close();file_action(t.row.id,side,"rename",value);}>{move ||tr(s.english.get(),"预览重命名…","Preview rename…")}</button></div></Show>
                <Show when=move ||properties.get()><div class="context-detail" role="region" aria-label=move ||tr(s.english.get(),"属性","Properties")><p class="context-path">{row.path.clone()}</p><p>{move ||label(&stored.get_value().row.status,s.english.get())}</p><p>{move ||tr(s.english.get(),"本次比较记录","Comparison snapshot")}</p>{[false,true].into_iter().map(move |r|{let row=stored.get_value().row;let e=if r{row.right}else{row.left};let bytes=format!("{} · {}",e.as_ref().map(|e|super::entry_icon::kind_label(&e.kind,s.english.get_untracked())).unwrap_or("—"),size(e.as_ref()));let time=super::directory::modified(e.as_ref(),s.english.get_untracked());view!{<p>{if r{tr(s.english.get_untracked(),"右侧","Right")}else{tr(s.english.get_untracked(),"左侧","Left")}}": "{if e.is_none(){tr(s.english.get_untracked(),"缺失","Missing").into()}else{format!("{bytes} · {time}")}}</p>}}).collect_view()}</div></Show>
                <Show when=move ||clipboard_error.get()><p class="context-detail" role="alert">{move ||tr(s.english.get(),"无法写入剪贴板，请重试。","Unable to copy to clipboard. Please retry.")}</p></Show>
            </div>
        }
    })} }
}
