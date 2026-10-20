//! Full-height immutable repository browsing, with direct single-file local comparison.
use super::History;
use crate::{invoke, tr};
use clear_contracts::{workbench::*, *};
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Browser {
    h: History,
    english: RwSignal<bool>,
    tree: RwSignal<Option<RevisionTree>>,
    selected: RwSignal<Option<RevisionEntry>>,
    content: RwSignal<Option<RevisionContent>>,
    local: RwSignal<Option<RevisionLocalDiff>>,
    filter: RwSignal<String>,
    applied: RwSignal<String>,
    diff_offset: RwSignal<usize>,
}
fn describe(en: bool, error: String) -> String {
    if en {
        return error;
    }
    for (native, zh) in [
        (
            "Historical tree expired",
            "历史浏览会话已失效，请返回历史后重新打开。",
        ),
        (
            "Historical repository identity changed",
            "仓库身份已变化，请重新扫描。",
        ),
        (
            "Historical repository mapping changed",
            "仓库映射已变化，请重新扫描。",
        ),
        (
            "Historical repository is unavailable",
            "历史仓库不可用，请重新打开。",
        ),
        (
            "Historical blob exceeds 256 MiB",
            "历史文件超过 256 MiB 查看限制。",
        ),
        (
            "Historical text view exceeds 4 MiB",
            "文本超过 4 MiB，请使用分页十六进制查看。",
        ),
        (
            "Historical text comparison exceeds 4 MiB",
            "文本比较超过 4 MiB 限制。",
        ),
        (
            "Historical tree exceeds 50000",
            "历史浏览已达到 50000 个条目限制，请重新打开所需版本。",
        ),
        (
            "Historical blob content does not match",
            "历史对象内容与身份不符，无法可信地预览。",
        ),
        (
            "Bare repositories have no local working file",
            "裸仓库没有可比较的本地工作文件。",
        ),
        (
            "Local counterpart is not a regular file",
            "本地对应项不是普通文件，无法进行文本比较。",
        ),
        (
            "Local counterpart changed during comparison",
            "本地文件在比较期间发生变化，请重新比较。",
        ),
        (
            "Local counterpart appeared during comparison",
            "本地文件在比较期间出现，请重新比较。",
        ),
        (
            "Binary or unsupported text encoding",
            "文件为二进制或编码不受支持，请使用十六进制查看。",
        ),
        (
            "Binary text is not editable",
            "文件包含二进制内容，请使用十六进制查看。",
        ),
        (
            "Use UTF-8 or BOM-marked UTF-16",
            "文本查看需要 UTF-8 或带 BOM 的 UTF-16，可切换十六进制。",
        ),
    ] {
        if error.contains(native) {
            return zh.into();
        }
    }
    error
}
impl Browser {
    fn open(self, commit: String) {
        if self.h.c.busy.get_untracked() {
            return;
        }
        let Some(state) = self.h.c.selected.get_untracked() else {
            return;
        };
        self.h.c.busy.set(true);
        self.h.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RevisionTree>(
                "workspace_revision_open",
                serde_json::json!({"repositoryId":state.repository.id,"commit":commit}),
            )
            .await
            {
                Ok(tree) => {
                    if self.h.c.selected.with_untracked(|state| {
                        state
                            .as_ref()
                            .is_some_and(|state| state.repository.id == tree.repository_id)
                    }) {
                        self.tree.set(Some(tree));
                        self.selected.set(None);
                        self.content.set(None);
                        self.local.set(None);
                        self.filter.set(String::new());
                        self.applied.set(String::new());
                    }
                }
                Err(error) => self
                    .h
                    .c
                    .error
                    .set(describe(self.english.get_untracked(), error)),
            }
            self.h.c.busy.set(false);
        });
    }
    fn directory(self, directory: Option<String>, offset: usize, filter: String) {
        if self.h.c.busy.get_untracked() {
            return;
        }
        let Some(tree) = self.tree.get_untracked() else {
            return;
        };
        self.h.c.busy.set(true);
        self.h.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RevisionTree>("workspace_revision_tree",serde_json::json!({"repositoryId":tree.repository_id,"session":tree.id,"directory":directory,"offset":offset,"filter":filter})).await {
                Ok(page)=>{self.tree.set(Some(page));self.filter.set(filter.clone());self.applied.set(filter);}
                Err(error)=>self.h.c.error.set(describe(self.english.get_untracked(),error)),
            }
            self.h.c.busy.set(false);
        });
    }
    fn select(self, entry: RevisionEntry) {
        if self.h.c.busy.get_untracked() {
            return;
        }
        match entry.kind.as_str() {
            "directory" => self.directory(Some(entry.id), 0, String::new()),
            "submodule" => {
                self.selected.set(Some(entry));
                self.content.set(None);
                self.local.set(None);
            }
            _ => self.read(entry, "auto".into(), 0),
        }
    }
    fn read(self, entry: RevisionEntry, mode: String, offset: usize) {
        if self.h.c.busy.get_untracked() {
            return;
        }
        let Some(tree) = self.tree.get_untracked() else {
            return;
        };
        self.h.c.busy.set(true);
        self.h.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RevisionContent>("workspace_revision_content",serde_json::json!({"repositoryId":tree.repository_id,"session":tree.id,"fileId":entry.id,"mode":mode,"offset":offset})).await {
                Ok(content)=>{self.selected.set(Some(entry));self.content.set(Some(content));self.local.set(None);}
                Err(error)=>self.h.c.error.set(describe(self.english.get_untracked(),error)),
            }
            self.h.c.busy.set(false);
        });
    }
    fn compare(self) {
        if self.h.c.busy.get_untracked() {
            return;
        }
        let (Some(tree), Some(file)) = (self.tree.get_untracked(), self.selected.get_untracked())
        else {
            return;
        };
        self.h.c.busy.set(true);
        self.h.c.error.set(String::new());
        spawn_local(async move {
            match invoke::<RevisionLocalDiff>("workspace_revision_local_diff",serde_json::json!({"repositoryId":tree.repository_id,"session":tree.id,"fileId":file.id,"options":TextOptions::default()})).await {
                Ok(local)=>{self.local.set(Some(local));self.diff_offset.set(0);}
                Err(error)=>self.h.c.error.set(describe(self.english.get_untracked(),error)),
            }
            self.h.c.busy.set(false);
        });
    }
}
fn entries(v: Browser) -> impl IntoView {
    view! {<div class="git-revision-entries"><Show when=move ||v.tree.with(|tree|tree.as_ref().is_some_and(|tree|tree.entries.is_empty()))><p>{move ||tr(v.english.get(),"此目录没有符合筛选条件的条目。","No entries match the filter in this directory.")}</p></Show><For each=move ||v.tree.with(|tree|tree.as_ref().map(|tree|tree.entries.clone()).unwrap_or_default()) key=|entry|entry.id.clone() children=move |entry|{
        let file=StoredValue::new(entry);let kind=file.with_value(|entry|match entry.kind.as_str(){"directory"=>Kind::Directory,"link"=>Kind::Link,"submodule"=>Kind::Other,_=>Kind::File});
        view!{<button class="git-revision-entry" disabled=move ||v.h.c.busy.get() class:selected=move ||file.with_value(|file|v.selected.with(|selected|selected.as_ref().is_some_and(|selected|selected.id==file.id))) title=file.with_value(|file|format!("{} · {} · {}",file.path,file.mode,file.object)) on:click=move |_|file.with_value(|file|v.select(file.clone()))>{super::super::entry_icon::icon(&kind,&file.with_value(|file|file.name.clone()),false)}<span>{file.with_value(|file|file.name.clone())}</span><small>{move ||file.with_value(|file|match file.kind.as_str(){"directory"=>"—".into(),"submodule"=>tr(v.english.get(),"子模块","Submodule").into(),"link"=>tr(v.english.get(),"链接","Link").into(),_=>format!("{} B",file.bytes)})}</small></button>}
    }/></div>}
}
pub(super) fn panel(
    h: History,
    english: RwSignal<bool>,
    files_visible: RwSignal<bool>,
    editor_request: RwSignal<Option<(String, String)>>,
) -> impl IntoView {
    let v = Browser {
        h,
        english,
        tree: RwSignal::new(None),
        selected: RwSignal::new(None),
        content: RwSignal::new(None),
        local: RwSignal::new(None),
        filter: RwSignal::new(String::new()),
        applied: RwSignal::new(String::new()),
        diff_offset: RwSignal::new(0),
    };
    Effect::watch(
        move || h.revision.get(),
        move |commit, _, _| {
            if let Some(commit) = commit {
                v.open(commit.clone());
            }
        },
        false,
    );
    let rows = Memo::new(move |_| {
        v.local.with(|local| {
            local
                .as_ref()
                .map(|local| text_view::align(&local.diff.lines))
                .unwrap_or_default()
        })
    });
    let left = NodeRef::<leptos::html::Div>::new();
    let right = NodeRef::<leptos::html::Div>::new();
    let side = move |is_right: bool| {
        view! {<div class="git-revision-diff-side" node_ref=if is_right{right}else{left} tabindex="0" aria-label=move ||if is_right{tr(english.get(),"本地磁盘内容","Local disk contents")}else{tr(english.get(),"历史提交内容","Historical commit contents")} on:scroll=move |_|{let (source,target)=if is_right{(right,left)}else{(left,right)};if let(Some(source),Some(target))=(source.get(),target.get())&&source.scroll_top()!=target.scroll_top(){target.set_scroll_top(source.scroll_top());}}>
        {move ||rows.with(|rows|rows.iter().skip(v.diff_offset.get()).take(500).map(|pair|{let line=v.local.with(|local|local.as_ref().and_then(|local|if is_right{pair.right}else{pair.left}.and_then(|index|local.diff.lines.get(index))).cloned());view!{<div class="git-reference-render-row">{super::super::text::cell(line.as_ref(),is_right,false,false,false,english.get())}</div>}}).collect_view())}</div>}
    };
    view! {<Show when=move ||h.revision.with(|commit|commit.is_some())><section class="git-revision" aria-label=move ||tr(english.get(),"提交文件浏览","Repository at revision")>
        <header><strong>{move ||tr(english.get(),"提交文件（只读）","Files at revision (read only)")}</strong><code title=move ||v.tree.with(|tree|tree.as_ref().map(|tree|tree.commit.clone()).unwrap_or_default())>{move ||v.tree.with(|tree|tree.as_ref().map(|tree|tree.commit.chars().take(12).collect::<String>()).unwrap_or_default())}</code><span>{move ||tr(english.get(),"不检出；显示该版本全部跟踪条目。","No checkout; shows tracked entries at this revision.")}</span><button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|h.revision.set(None)>{move ||tr(english.get(),"返回提交历史","Back to commit history")}</button></header>
        <div class="git-revision-body"><aside><nav><button class="quiet" disabled=move ||h.c.busy.get()||v.tree.with(|tree|tree.as_ref().is_none_or(|tree|tree.directory.is_none())) on:click=move |_|v.directory(None,0,String::new())>{move ||tr(english.get(),"根目录","Root")}</button><button class="quiet" disabled=move ||h.c.busy.get()||v.tree.with(|tree|tree.as_ref().is_none_or(|tree|tree.directory.is_none())) on:click=move |_|v.directory(v.tree.with_untracked(|tree|tree.as_ref().and_then(|tree|tree.parent.clone())),0,String::new())>{move ||tr(english.get(),"上一级","Up")}</button><code>{move ||v.tree.with(|tree|tree.as_ref().map(|tree|if tree.path.is_empty(){"/".into()}else{tree.path.clone()}).unwrap_or_default())}</code></nav>
        <div class="git-revision-filter"><input aria-label=move ||tr(english.get(),"筛选历史目录文件","Filter historical directory") placeholder=move ||tr(english.get(),"筛选当前目录…","Filter this directory…") prop:value=move ||v.filter.get() on:input=move |event|v.filter.set(event_target_value(&event)) on:keydown=move |event|{if event.key()=="Enter"{v.directory(v.tree.with_untracked(|tree|tree.as_ref().and_then(|tree|tree.directory.clone())),0,v.filter.get_untracked());}}/><button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|v.directory(v.tree.with_untracked(|tree|tree.as_ref().and_then(|tree|tree.directory.clone())),0,v.filter.get_untracked())>{move ||tr(english.get(),"筛选","Filter")}</button></div>
        {entries(v)}<footer><span>{move ||v.tree.with(|tree|tree.as_ref().map(|tree|format!("{}–{} / {}",tree.offset+usize::from(tree.total>0),(tree.offset+300).min(tree.total),tree.total)).unwrap_or_default())}</span><button class="quiet" disabled=move ||h.c.busy.get()||v.tree.with(|tree|tree.as_ref().is_none_or(|tree|tree.offset==0)) on:click=move |_|if let Some(tree)=v.tree.get_untracked(){v.directory(tree.directory,tree.offset.saturating_sub(300),v.applied.get_untracked());}>"‹"</button><button class="quiet" disabled=move ||h.c.busy.get()||v.tree.with(|tree|tree.as_ref().is_none_or(|tree|tree.offset+300>=tree.total)) on:click=move |_|if let Some(tree)=v.tree.get_untracked(){v.directory(tree.directory,tree.offset+300,v.applied.get_untracked());}>"›"</button></footer></aside>
        <main><header><strong>{move ||v.selected.with(|file|file.as_ref().map(|file|file.path.clone()).unwrap_or_else(||tr(english.get(),"选择文件查看该提交的内容","Choose a file to view its committed contents").into()))}</strong><Show when=move ||v.selected.with(|file|file.as_ref().is_some_and(|file|file.kind!="submodule"))><button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|if let Some(file)=v.selected.get_untracked(){v.read(file,"auto".into(),0);}>{move ||tr(english.get(),"查看原文","View contents")}</button><button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|if let Some(file)=v.selected.get_untracked(){v.read(file,"hex".into(),0);}>{move ||tr(english.get(),"十六进制","Hexadecimal")}</button></Show><button class="quiet" disabled=move ||h.c.busy.get()||h.c.selected.with(|state|state.as_ref().is_none_or(|state|state.repository.kind=="bare"))||v.selected.with(|file|file.as_ref().is_none_or(|file|file.kind!="file"))||v.content.with(|content|content.as_ref().is_none_or(|content|content.encoding.is_none())) on:click=move |_|v.compare()>{move ||tr(english.get(),"与本地文件比较","Compare with local file")}</button><Show when=move ||v.local.with(|local|local.as_ref().is_some_and(|local|local.local_file_id.is_some()))><button class="quiet" disabled=move ||h.c.busy.get() on:click=move |_|if let (Some(tree),Some(id))=(v.tree.get_untracked(),v.local.with_untracked(|local|local.as_ref().and_then(|local|local.local_file_id.clone()))){editor_request.set(Some((tree.repository_id,id)));files_visible.set(true);}>{move ||tr(english.get(),"编辑本地文件","Edit local file")}</button></Show></header>
        <Show when=move ||v.selected.with(|file|file.is_some())><div class="git-revision-metadata">
          <code title=move ||v.selected.with(|file|file.as_ref().map(|file|file.object.clone()).unwrap_or_default())>{move ||v.selected.with(|file|file.as_ref().map(|file|format!("{} · {}",file.mode,file.object.chars().take(12).collect::<String>())).unwrap_or_default())}</code>
          <span>{move ||v.selected.with(|file|file.as_ref().map(|file|match file.kind.as_str(){"submodule"=>tr(english.get(),"子模块提交引用；不展开或下载。","Submodule commit reference; not expanded or downloaded."),"link"=>tr(english.get(),"符号链接目标内容；不跟随链接。","Symbolic-link target bytes; link is not followed."),_=>""}).unwrap_or_default())}</span>
          <span>{move ||v.content.with(|content|content.as_ref().and_then(|content|content.encoding.as_ref().map(|encoding|format!("{} · {}",match encoding{Encoding::Utf8=>"UTF-8",Encoding::Utf16Le=>"UTF-16 LE",Encoding::Utf16Be=>"UTF-16 BE"},match content.line_ending {Some(LineEnding::Lf)=>"LF",Some(LineEnding::CrLf)=>"CRLF",Some(LineEnding::Cr)=>"CR",_=>tr(english.get(),"混合换行","Mixed endings")}))).unwrap_or_default())}</span>
        </div></Show>
        <Show when=move ||v.local.with(|local|local.is_none()) fallback=move ||view!{<div class="git-revision-diff-labels"><span>{move ||tr(english.get(),"历史提交","Historical commit")}</span><span>{move ||v.local.with(|local|if local.as_ref().is_some_and(|local|local.local_missing){tr(english.get(),"本地文件不存在","Local file missing")}else{tr(english.get(),"本地磁盘（不含未保存草稿）","Local disk (unsaved buffers excluded)")})}</span></div><div class="git-revision-diff">{side(false)}{side(true)}</div><footer><span>{move ||format!("{}–{} / {}",v.diff_offset.get()+usize::from(rows.with(|rows|!rows.is_empty())),(v.diff_offset.get()+500).min(rows.with(|rows|rows.len())),rows.with(|rows|rows.len()))}</span><button class="quiet" disabled=move ||v.diff_offset.get()==0 on:click=move |_|v.diff_offset.update(|offset|*offset=offset.saturating_sub(500))>"‹"</button><button class="quiet" disabled=move ||{v.diff_offset.get()+500>=rows.with(|rows|rows.len())} on:click=move |_|v.diff_offset.update(|offset|*offset+=500)>"›"</button></footer>}>
          <div class="git-revision-content" tabindex="0" aria-label=move ||tr(english.get(),"历史文件正文","Historical file contents")><Show when=move ||v.content.with(|content|content.is_some()) fallback=move ||view!{<p>{move ||v.selected.with(|file|if file.as_ref().is_some_and(|file|file.kind=="submodule"){tr(english.get(),"上方为子模块对象信息。选择普通文件查看正文。","Submodule object shown above. Select a regular file to view its contents.")}else{tr(english.get(),"在左侧展开目录并选择文件。","Open a directory on the left and choose a file.")})}</p>}>
            {move ||v.content.get().map(|content|if content.mode=="hex"{view!{<pre>{content.text}</pre>}.into_any()}else{view!{<div>{content.text.split('\n').enumerate().map(|(i,line)|view!{<div class="git-revision-line"><span>{content.offset+i+1}</span><code>{line.to_string()}</code></div>}).collect_view()}</div>}.into_any()})}</Show></div>
          <footer><span>{move ||v.content.with(|content|content.as_ref().map(|content|format!("{}–{} / {} {}",content.offset+usize::from(content.total>0),(content.offset+if content.mode=="hex"{4096}else{500}).min(content.total),content.total,if content.mode=="hex"{tr(english.get(),"字节","bytes")}else{tr(english.get(),"行","lines")})).unwrap_or_default())}</span><span>{move ||v.content.with(|content|content.as_ref().is_some_and(|content|content.long_lines)).then_some(tr(english.get(),"超长行仅显示前 8192 字符","Long lines show the first 8192 characters"))}</span><button class="quiet" disabled=move ||h.c.busy.get()||v.content.with(|content|content.as_ref().is_none_or(|content|content.offset==0)) on:click=move |_|if let (Some(file),Some(content))=(v.selected.get_untracked(),v.content.get_untracked()){let step=if content.mode=="hex"{4096}else{500};v.read(file,content.mode,content.offset.saturating_sub(step));}>"‹"</button><button class="quiet" disabled=move ||h.c.busy.get()||v.content.with(|content|content.as_ref().is_none_or(|content|content.next_offset.is_none())) on:click=move |_|if let (Some(file),Some(content))=(v.selected.get_untracked(),v.content.get_untracked())&&let Some(offset)=content.next_offset{v.read(file,content.mode,offset);}>"›"</button></footer>
        </Show></main></div><Show when=move ||h.c.busy.get()><footer class="git-revision-progress" role="status"><span>{move ||tr(english.get(),"正在读取历史对象…","Reading historical objects…")}</span><button class="quiet" on:click=move |_|spawn_local(async{let _=invoke::<()>("cancel",serde_json::json!({})).await;})>{move ||tr(english.get(),"取消","Cancel")}</button></footer></Show>
    </section></Show>}
}
