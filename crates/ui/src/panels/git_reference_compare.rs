//! Captured branch/index/working-tree comparisons inside the repository workbench.
use super::git_actions::Controller;
use crate::{invoke, label, status_class, tr};
use clear_contracts::{workbench::*, *};
use leptos::{prelude::*, task::spawn_local};
#[derive(Clone, Copy)]
struct Compare {
    c: Controller,
    opened: RwSignal<Option<OpenedComparison>>,
    page: RwSignal<Option<Page>>,
    search: RwSignal<String>,
    differences: RwSignal<bool>,
    offset: RwSignal<usize>,
    diff: RwSignal<Option<TextDiff>>,
    text_offset: RwSignal<usize>,
    options: RwSignal<TextOptions>,
    file: RwSignal<String>,
    epoch: RwSignal<usize>,
}
impl Compare {
    fn load(self) {
        if self.c.busy.get_untracked() {
            return;
        }
        let (Some(state), Some(request)) = (
            self.c.selected.get_untracked(),
            self.c.comparison.get_untracked(),
        ) else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        spawn_local(async move {
            let result=async {
                let state=invoke::<RepositoryState>("workspace_open",serde_json::json!({"repositoryId":state.repository.id})).await?;
                self.c.accept(state.clone());
                invoke::<OpenedComparison>("workspace_compare_references",serde_json::json!({"repositoryId":state.repository.id,"revision":state.revision,"request":request})).await
            }.await;
            match result {
                Ok(view) => {
                    self.page.set(Some(view.page.clone()));
                    self.opened.set(Some(view));
                    self.offset.set(0);
                    self.search.set(String::new());
                    self.differences.set(true);
                    self.diff.set(None);
                    self.file.set(String::new());
                    self.text_offset.set(0);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn query(self, offset: usize) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(view) = self.opened.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        let args = serde_json::json!({"session":view.session,"offset":offset,"search":self.search.get_untracked(),"differencesOnly":self.differences.get_untracked()});
        spawn_local(async move {
            match invoke::<Page>("git_page", args).await {
                Ok(page) => {
                    self.page.set(Some(page));
                    self.offset.set(offset);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
    fn text(self, id: String) {
        if self.c.busy.get_untracked() {
            return;
        }
        let Some(view) = self.opened.get_untracked() else {
            return;
        };
        self.c.busy.set(true);
        self.c.error.set(String::new());
        let args = serde_json::json!({"session":view.session,"rowId":id,"options":self.options.get_untracked()});
        spawn_local(async move {
            match invoke::<TextDiff>("git_text_diff", args).await {
                Ok(diff) => {
                    self.file.set(id);
                    self.diff.set(Some(diff));
                    self.epoch.update(|n| *n += 1);
                    self.text_offset.set(0);
                }
                Err(e) => self.c.error.set(e),
            }
            self.c.busy.set(false);
        });
    }
}
pub fn panel(c: Controller, english: RwSignal<bool>) -> impl IntoView {
    let v = Compare {
        c,
        opened: RwSignal::new(None),
        page: RwSignal::new(None),
        search: RwSignal::new(String::new()),
        differences: RwSignal::new(true),
        offset: RwSignal::new(0),
        diff: RwSignal::new(None),
        text_offset: RwSignal::new(0),
        options: RwSignal::new(TextOptions::default()),
        file: RwSignal::new(String::new()),
        epoch: RwSignal::new(0),
    };
    let repository = Memo::new(move |_| {
        c.selected
            .with(|s| s.as_ref().map(|s| s.repository.id.clone()))
    });
    Effect::watch(
        move || repository.get(),
        move |_, _, _| {
            c.comparison.set(None);
            v.opened.set(None);
            v.page.set(None);
            v.diff.set(None);
        },
        false,
    );
    Effect::watch(
        move || c.comparison.get(),
        move |request, _, _| {
            if request.is_some() {
                v.load();
            }
        },
        false,
    );
    let rows = Memo::new(move |_| {
        v.diff.with(|d| {
            d.as_ref()
                .map(|d| text_view::align(&d.lines))
                .unwrap_or_default()
        })
    });
    let left_scroll = NodeRef::<leptos::html::Div>::new();
    let right_scroll = NodeRef::<leptos::html::Div>::new();
    let visible_rows = move || {
        let start = v.text_offset.get();
        let epoch = v.epoch.get();
        (start..(start + 500).min(rows.with(|r| r.len())))
            .map(|i| (epoch, i))
            .collect::<Vec<_>>()
    };
    let side = move |right: bool| {
        view! {
            <div class="git-reference-side" node_ref=if right{right_scroll}else{left_scroll} tabindex="0" aria-label=move ||if right{tr(english.get(),"右侧比较文本","Right comparison text")}else{tr(english.get(),"左侧比较文本","Left comparison text")} on:scroll=move |_|{
                let (source,target)=if right{(right_scroll,left_scroll)}else{(left_scroll,right_scroll)};
                if let (Some(source),Some(target))=(source.get(),target.get())&&source.scroll_top()!=target.scroll_top(){target.set_scroll_top(source.scroll_top());}
            }><For each=visible_rows key=|n|*n children=move |(_,i)|{
                let pair=rows.with(|r|r.get(i).cloned());
                let line=v.diff.with(|d|d.as_ref().and_then(|d|pair.as_ref().and_then(|p|if right{p.right}else{p.left}).and_then(|n|d.lines.get(n)).cloned()));
                view!{<div class="git-reference-render-row">{super::text::cell(line.as_ref(),right,false,false,false,english.get())}</div>}
            }/></div>
        }
    };
    view! {<Show when=move ||c.comparison.with(|r|r.is_some())><section class="git-reference-compare" aria-label=move ||tr(english.get(),"分支与工作区比较","Reference comparison")>
      <header><strong>{move ||tr(english.get(),"版本比较（只读）","Version comparison (read only)")}</strong><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|c.comparison.set(None)>{move ||tr(english.get(),"返回仓库","Back to repository")}</button></header>
      <div class="git-reference-sources"><label>{move ||tr(english.get(),"左侧版本","Left revision")}<select aria-label=move ||tr(english.get(),"比较左侧引用","Left comparison reference") disabled=move ||c.busy.get() prop:value=move ||c.comparison.with(|r|r.as_ref().map(|r|r.left_reference_id.clone()).unwrap_or_default()) on:change=move |ev|{let id=event_target_value(&ev);c.comparison.update(|r|{if let Some(r)=r{r.left_reference_id=id;}});}>{move ||c.selected.with(|s|s.as_ref().map(|s|s.references.iter().filter(|r|!r.name.ends_with("/HEAD")).map(|r|view!{<option value=r.id.clone()>{r.full_name.clone()}</option>}).collect_view()))}</select></label>
      <span>"↔"</span><label>{move ||tr(english.get(),"右侧来源","Right source")}<select aria-label=move ||tr(english.get(),"比较右侧来源","Right comparison source") disabled=move ||c.busy.get() prop:value=move ||c.comparison.with(|r|r.as_ref().map(|r|match &r.target{CompareTarget::Current=>"current".into(),CompareTarget::WorkingTree{..}=>"worktree".into(),CompareTarget::Index=>"index".into(),CompareTarget::Reference{id}=>id.clone()}).unwrap_or_default()) on:change=move |ev|{let id=event_target_value(&ev);c.comparison.update(|r|{if let Some(r)=r{r.target=match id.as_str(){"current"=>CompareTarget::Current,"worktree"=>CompareTarget::WorkingTree{include_untracked:false},"index"=>CompareTarget::Index,_=>CompareTarget::Reference{id}};}});}>
      <option value="current">{move ||tr(english.get(),"当前提交 HEAD","Current HEAD")}</option><Show when=move ||c.selected.with(|s|s.as_ref().is_some_and(|s|s.repository.kind!="bare"))><option value="worktree">{move ||tr(english.get(),"工作区文件","Working-tree files")}</option><option value="index">{move ||tr(english.get(),"暂存区","Index")}</option></Show>{move ||c.selected.with(|s|s.as_ref().map(|s|s.references.iter().filter(|r|!r.name.ends_with("/HEAD")).map(|r|view!{<option value=r.id.clone()>{r.full_name.clone()}</option>}).collect_view()))}</select></label>
      <Show when=move ||c.comparison.with(|r|r.as_ref().is_some_and(|r|matches!(r.target,CompareTarget::WorkingTree{..})))><label><input type="checkbox" disabled=move ||c.busy.get() prop:checked=move ||c.comparison.with(|r|r.as_ref().is_some_and(|r|matches!(r.target,CompareTarget::WorkingTree{include_untracked:true}))) on:change=move |ev|{let include_untracked=event_target_checked(&ev);c.comparison.update(|r|{if let Some(r)=r{r.target=CompareTarget::WorkingTree{include_untracked};}});}/>{move ||tr(english.get(),"包含未跟踪文件","Include untracked")}</label></Show>
      <button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.load()>{move ||tr(english.get(),"重新比较","Compare again")}</button></div>
      <Show when=move ||v.opened.with(|o|o.is_some())><details class="git-reference-capture"><summary>{move ||tr(english.get(),"已捕获的比较来源与限制","Captured sources and limitations")}</summary><code>{move ||v.opened.with(|o|o.as_ref().map(|o|format!("{} ↔ {}",o.left,o.right)))}</code><p>{move ||tr(english.get(),"提交和暂存区固定到捕获对象；工作区按磁盘原始字节比较，打开预览前复查。未保存的编辑不包含在内。链接不跟随，子模块不展开。","Commits and index entries are pinned. Working-tree bytes are rechecked before preview; unsaved edits are excluded. Links are not followed and submodules are not expanded.")}</p></details></Show>
      <div class="git-reference-body"><aside><div class="git-reference-filter"><input aria-label=move ||tr(english.get(),"筛选比较路径","Filter comparison paths") placeholder=move ||tr(english.get(),"搜索路径…","Search paths…") prop:value=move ||v.search.get() on:input=move |ev|v.search.set(event_target_value(&ev)) on:keydown=move |ev|{if ev.key()=="Enter"{v.query(0);}}/><button class="quiet" disabled=move ||c.busy.get() on:click=move |_|v.query(0)>{move ||tr(english.get(),"搜索","Search")}</button><label><input type="checkbox" disabled=move ||c.busy.get() prop:checked=move ||v.differences.get() on:change=move |ev|{v.differences.set(event_target_checked(&ev));v.query(0);}/>{move ||tr(english.get(),"只看差异","Differences only")}</label></div>
      <div class="git-reference-files"><Show when=move ||!c.busy.get()&&v.page.with(|p|p.as_ref().is_some_and(|p|p.rows.is_empty()))><p class="git-reference-empty">{move ||tr(english.get(),"没有符合条件的文件。可关闭“只看差异”或调整搜索。","No matching files. Show equal files or change the search.")}</p></Show><For each=move ||v.page.with(|p|p.as_ref().map(|p|p.rows.clone()).unwrap_or_default()) key=|r|r.id.clone() children=move |row|{let id=row.id.clone();let selected=row.id.clone();let preview=row.status!=Status::Error&&row.left.as_ref().or(row.right.as_ref()).is_some_and(|e|e.kind==Kind::File);let status=row.status;let css=status_class(&status);let title=row.path.clone();view!{<button class="git-reference-file" class:selected=move ||v.file.get()==selected disabled=move ||c.busy.get()||!preview title=title on:click=move |_|v.text(id.clone())><span>{row.path}</span><small class=css>{move ||label(&status,english.get())}</small></button>}}/></div>
      <footer><button class="quiet" disabled=move ||c.busy.get()||v.offset.get()==0 on:click=move |_|v.query(v.offset.get_untracked().saturating_sub(500))>"‹"</button><span>{move ||v.page.with(|p|p.as_ref().map(|p|format!("{}–{} / {}",v.offset.get()+usize::from(p.total>0),(v.offset.get()+500).min(p.total),p.total)))}</span><button class="quiet" disabled=move ||c.busy.get()||v.page.with(|p|p.as_ref().is_none_or(|p|v.offset.get()+500>=p.total)) on:click=move |_|v.query(v.offset.get_untracked()+500)>"›"</button></footer></aside>
      <main class="git-reference-text"><header><strong>{move ||v.diff.with(|d|d.as_ref().map(|d|d.path.clone())).unwrap_or_else(||tr(english.get(),"选择文件查看内容差异","Select a file to view changes").into())}</strong></header><div class="git-reference-columns"><span>{move ||tr(english.get(),"左侧版本","Left revision")}</span><span>{move ||tr(english.get(),"右侧来源","Right source")}</span></div>
      <div class="git-reference-text-scroll">{side(false)}{side(true)}</div>
      <footer><button class="quiet" disabled=move ||v.text_offset.get()==0 on:click=move |_|v.text_offset.update(|n|*n=n.saturating_sub(500))>"‹"</button><span>{move ||format!("{}–{} / {}",v.text_offset.get()+usize::from(rows.with(|r|!r.is_empty())),(v.text_offset.get()+500).min(rows.with(|r|r.len())),rows.with(|r|r.len()))}</span><button class="quiet" disabled=move ||{v.text_offset.get()+500>=rows.with(|r|r.len())} on:click=move |_|v.text_offset.update(|n|*n+=500)>"›"</button></footer></main></div>
      <Show when=move ||c.busy.get()><div class="git-reference-progress" role="status">{move ||tr(english.get(),"正在读取比较结果…","Reading comparison…")}</div></Show>
    </section></Show>}
}
