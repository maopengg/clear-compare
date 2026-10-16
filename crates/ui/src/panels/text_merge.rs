//! Inline two-way merge controller. Only typed native document commands write files.
use crate::{invoke, tr};
use clear_contracts::{
    LineEnding, SaveOptions, SaveResult, Side, TextDiff, TextDocument, TextOptions, text_merge,
};
use leptos::{prelude::*, task::spawn_local};
use std::collections::BTreeSet;
#[derive(Clone, Debug)]
pub struct Context {
    pub diff_id: String,
    pub session: String,
    pub row_id: String,
    pub options: TextOptions,
    pub left: bool,
    pub right: bool,
}
#[derive(Clone, Copy)]
pub struct Controller {
    pub original: RwSignal<Option<TextDiff>>,
    pub context: RwSignal<Option<Context>>,
    pub english: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub dirty: RwSignal<bool>,
    pub document: RwSignal<Option<TextDocument>>,
    pub selected: RwSignal<BTreeSet<usize>>,
    pub undo: RwSignal<Vec<BTreeSet<usize>>>,
    pub redo: RwSignal<Vec<BTreeSet<usize>>>,
}
impl Controller {
    pub fn new(
        original: RwSignal<Option<TextDiff>>,
        context: RwSignal<Option<Context>>,
        english: RwSignal<bool>,
        busy: RwSignal<bool>,
        error: RwSignal<String>,
        dirty: RwSignal<bool>,
    ) -> Self {
        let s = Self {
            original,
            context,
            english,
            busy,
            error,
            dirty,
            document: RwSignal::new(None),
            selected: RwSignal::new(BTreeSet::new()),
            undo: RwSignal::new(Vec::new()),
            redo: RwSignal::new(Vec::new()),
        };
        Effect::watch(
            move || original.with(|d| d.as_ref().map(|d| d.id.clone())),
            move |_, _, _| {
                let old = s.document.get_untracked();
                s.document.set(None);
                s.selected.set(BTreeSet::new());
                s.undo.set(Vec::new());
                s.redo.set(Vec::new());
                if let Some(old) = old {
                    spawn_local(async move {
                        let _ = invoke::<()>(
                            "close_document",
                            serde_json::json!({"documentId":old.id}),
                        )
                        .await;
                    });
                }
            },
            true,
        );
        s
    }
    pub fn target(&self) -> Side {
        self.document
            .with(|d| d.as_ref().map(|d| d.side.clone()).unwrap_or(Side::Right))
    }
    pub fn context(&self) -> Option<Context> {
        self.context.get().filter(|c| {
            self.original
                .with(|d| d.as_ref().is_some_and(|d| d.id == c.diff_id))
        })
    }
    pub fn allowed(&self, target: &Side) -> bool {
        !self.busy.get()
            && self.context().is_some_and(|c| {
                if *target == Side::Left {
                    c.left
                } else {
                    c.right
                }
            })
            && (!self.dirty.get() || self.target() == *target)
    }
    pub fn preview(&self) -> Option<text_merge::Preview> {
        self.original.with(|d| {
            d.as_ref().and_then(|d| {
                text_merge::preview(
                    d,
                    &self.selected.get(),
                    &self.target(),
                    self.document
                        .with(|d| d.as_ref().map(|d| d.line_ending.clone()))
                        .as_ref(),
                )
                .ok()
            })
        })
    }
    pub fn apply(self, hunk: usize, target: Side) {
        if !self.allowed(&target) {
            return;
        }
        let Some(context) = self.context() else {
            return;
        };
        let Some(original) = self.original.get_untracked() else {
            return;
        };
        self.busy.set(true);
        self.error.set(String::new());
        spawn_local(async move {
            let outcome=async {
                let mut document=self.document.get_untracked();
                if document.as_ref().is_some_and(|d|d.side!=target) && let Some(old)=document.take(){invoke::<()>("close_document",serde_json::json!({"documentId":old.id})).await?;self.document.set(None);}
                if document.is_none() {
                    let d=invoke::<TextDocument>("open_document",serde_json::json!({"session":context.session,"rowId":context.row_id,"side":target})).await?;
                    if d.text!=text_merge::text(&original,&target) {
                        let _=invoke::<()>("close_document",serde_json::json!({"documentId":d.id})).await;
                        return Err(tr(self.english.get_untracked(),"文件在比较后发生变化，请重新比较。","File changed since comparison. Compare again.").into());
                    }
                    self.document.set(Some(d.clone()));document=Some(d);
                }
                let d=document.ok_or("Document unavailable")?;
                if d.line_ending==LineEnding::Mixed {return Err(tr(self.english.get_untracked(),"目标包含混合换行，请先在编辑器中明确选择换行格式。","Target has mixed endings. Choose an explicit ending format in the editor first.").into());}
                let old=self.selected.get_untracked();let mut next=old.clone();next.insert(hunk);
                text_merge::preview(&original,&next,&target,Some(&d.line_ending))?;
                invoke::<()>("set_text_merge_dirty",serde_json::json!({"dirty":true})).await?;
                self.undo.update(|h|{h.push(old);if h.len()>128{h.remove(0);}});self.redo.set(Vec::new());self.selected.set(next);self.dirty.set(true);Ok::<_,String>(())
            }.await;
            if let Err(e) = outcome {
                self.error.set(e);
            }
            self.busy.set(false);
        });
    }
    pub fn history(self, redo: bool) {
        if self.busy.get_untracked() {
            return;
        }
        let history = if redo { self.redo } else { self.undo };
        let Some(next) = history.with_untracked(|h| h.last().cloned()) else {
            return;
        };
        self.busy.set(true);
        spawn_local(async move {
            match invoke::<()>(
                "set_text_merge_dirty",
                serde_json::json!({"dirty":!next.is_empty()}),
            )
            .await
            {
                Ok(()) => {
                    history.update(|h| {
                        h.pop();
                    });
                    let old = self.selected.get_untracked();
                    if redo {
                        self.undo.update(|h| h.push(old));
                    } else {
                        self.redo.update(|h| h.push(old));
                    }
                    self.dirty.set(!next.is_empty());
                    self.selected.set(next);
                }
                Err(e) => self.error.set(e),
            }
            self.busy.set(false);
        });
    }
    pub fn discard(self) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        spawn_local(async move {
            match invoke::<()>("set_text_merge_dirty", serde_json::json!({"dirty":false})).await {
                Ok(()) => {
                    self.dirty.set(false);
                    self.selected.set(BTreeSet::new());
                    self.undo.set(Vec::new());
                    self.redo.set(Vec::new());
                    if let Some(d) = self.document.get_untracked() {
                        let _ =
                            invoke::<()>("close_document", serde_json::json!({"documentId":d.id}))
                                .await;
                    }
                    self.document.set(None);
                    self.error.set(String::new());
                }
                Err(e) => self.error.set(e),
            }
            self.busy.set(false);
        });
    }
    pub fn save(self, saved: impl Fn(String) + Copy + Send + Sync + 'static) {
        if self.busy.get_untracked() || !self.dirty.get_untracked() {
            return;
        }
        let Some(document) = self.document.get_untracked() else {
            return;
        };
        let Some(preview) = self.preview() else {
            return;
        };
        let Some(context) = self.context() else {
            return;
        };
        let Some(original) = self.original.get_untracked() else {
            return;
        };
        let source = if document.side == Side::Left {
            Side::Right
        } else {
            Side::Left
        };
        let expected_source = text_merge::text(&original, &source);
        let text = text_merge::text(&preview.diff, &document.side);
        self.busy.set(true);
        self.error.set(String::new());
        spawn_local(async move {
            match invoke::<TextDiff>("text_diff",serde_json::json!({"session":context.session,"rowId":context.row_id,"options":TextOptions::default()})).await {
                Ok(fresh) if text_merge::text(&fresh,&source)==expected_source=>{},
                Ok(_)=>{self.error.set(tr(self.english.get_untracked(),"来源在合并期间发生变化；缓冲区已保留，请放弃后重新比较。","Source changed during merging; buffer retained. Discard and compare again.").into());self.busy.set(false);return;},
                Err(e)=>{self.error.set(e);self.busy.set(false);return;}
            }
            match invoke::<SaveResult>("save_document",serde_json::json!({"documentId":document.id,"text":text,"options":SaveOptions::default()})).await {
                Ok(result)=>{
                    let _=invoke::<()>("close_document",serde_json::json!({"documentId":result.document.id})).await;
                    self.document.set(None);self.selected.set(BTreeSet::new());self.undo.set(Vec::new());self.redo.set(Vec::new());
                    match invoke::<TextDiff>("text_diff",serde_json::json!({"session":context.session,"rowId":context.row_id,"options":context.options})).await {
                        Ok(next)=>{self.context.update(|c|{if let Some(c)=c{c.diff_id=next.id.clone();}});self.original.set(Some(next));},
                        Err(e)=>{self.original.set(None);self.error.set(format!("{} {e}",tr(self.english.get_untracked(),"文件已保存，但重新比较失败：","File saved, but comparison refresh failed:")));}
                    }
                    saved(result.backup);
                    match invoke::<()>("set_text_merge_dirty",serde_json::json!({"dirty":false})).await {Ok(())=>self.dirty.set(false),Err(e)=>self.error.set(e)}
                },Err(e)=>self.error.set(e)
            }
            self.busy.set(false);
        });
    }
}
