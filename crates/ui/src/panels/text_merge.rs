//! Independent left/right buffers; typed native document commands own all writes.
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub left: BTreeSet<usize>,
    pub right: BTreeSet<usize>,
}
impl Selection {
    fn side(&self, side: &Side) -> &BTreeSet<usize> {
        if *side == Side::Left {
            &self.left
        } else {
            &self.right
        }
    }
    fn side_mut(&mut self, side: &Side) -> &mut BTreeSet<usize> {
        if *side == Side::Left {
            &mut self.left
        } else {
            &mut self.right
        }
    }
}
#[derive(Clone, Copy)]
pub struct Controller {
    pub original: RwSignal<Option<TextDiff>>,
    pub context: RwSignal<Option<Context>>,
    pub english: RwSignal<bool>,
    pub busy: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub dirty: RwSignal<bool>,
    pub left_document: RwSignal<Option<TextDocument>>,
    pub right_document: RwSignal<Option<TextDocument>>,
    pub selected: RwSignal<Selection>,
    pub committed: RwSignal<Selection>,
    pub undo: RwSignal<Vec<Selection>>,
    pub redo: RwSignal<Vec<Selection>>,
    pub focused: RwSignal<Side>,
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
            left_document: RwSignal::new(None),
            right_document: RwSignal::new(None),
            selected: RwSignal::new(Selection::default()),
            committed: RwSignal::new(Selection::default()),
            undo: RwSignal::new(Vec::new()),
            redo: RwSignal::new(Vec::new()),
            focused: RwSignal::new(Side::Right),
        };
        Effect::watch(
            move || original.with(|d| d.as_ref().map(|d| d.id.clone())),
            move |_, _, _| {
                s.close_documents();
                s.selected.set(Selection::default());
                s.committed.set(Selection::default());
                s.undo.set(Vec::new());
                s.redo.set(Vec::new());
            },
            true,
        );
        s
    }
    fn document(&self, side: &Side) -> RwSignal<Option<TextDocument>> {
        if *side == Side::Left {
            self.left_document
        } else {
            self.right_document
        }
    }
    fn close_documents(self) {
        for signal in [self.left_document, self.right_document] {
            let old = signal.get_untracked();
            signal.set(None);
            if let Some(old) = old {
                spawn_local(async move {
                    let _ =
                        invoke::<()>("close_document", serde_json::json!({"documentId":old.id}))
                            .await;
                });
            }
        }
    }
    pub fn side_dirty(&self, side: &Side) -> bool {
        self.selected
            .with(|s| self.committed.with(|c| s.side(side) != c.side(side)))
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
    }
    fn make_preview(
        &self,
        original: &TextDiff,
        selected: &Selection,
    ) -> Result<text_merge::Preview, String> {
        let left = self
            .left_document
            .with(|d| d.as_ref().map(|d| d.line_ending.clone()));
        let right = self
            .right_document
            .with(|d| d.as_ref().map(|d| d.line_ending.clone()));
        text_merge::preview_both(
            original,
            &selected.left,
            &selected.right,
            left.as_ref(),
            right.as_ref(),
        )
    }
    pub fn preview(&self) -> Option<text_merge::Preview> {
        self.original.with(|d| {
            d.as_ref()
                .and_then(|d| self.selected.with(|s| self.make_preview(d, s).ok()))
        })
    }
    async fn guard(self) -> Result<(), String> {
        let dirty = self.selected.get_untracked() != self.committed.get_untracked();
        invoke::<()>("set_text_merge_dirty", serde_json::json!({"dirty":dirty})).await?;
        self.dirty.set(dirty);
        Ok(())
    }
    pub fn apply(self, hunk: usize, target: Side) {
        if !self.allowed(&target) {
            return;
        }
        let (Some(context), Some(original)) = (self.context(), self.original.get_untracked())
        else {
            return;
        };
        self.focused.set(target.clone());
        self.busy.set(true);
        self.error.set(String::new());
        spawn_local(async move {
            let result=async {
                let signal=self.document(&target);
                if signal.get_untracked().is_none() {
                    let d=invoke::<TextDocument>("open_document",serde_json::json!({"session":context.session,"rowId":context.row_id,"side":target})).await?;
                    if d.text!=text_merge::text(&original,&target) {
                        let _=invoke::<()>("close_document",serde_json::json!({"documentId":d.id})).await;
                        return Err(tr(self.english.get_untracked(),"文件在比较后发生变化，请重新比较。","File changed since comparison. Compare again.").into());
                    }
                    signal.set(Some(d));
                }
                let d=signal.get_untracked().ok_or("Document unavailable")?;
                if d.line_ending==LineEnding::Mixed {return Err(tr(self.english.get_untracked(),"目标包含混合换行，请先在编辑器中明确选择换行格式。","Target has mixed endings. Choose an explicit ending format in the editor first.").into());}
                let old=self.selected.get_untracked();let mut next=old.clone();
                next.side_mut(&target).insert(hunk);
                self.make_preview(&original,&next)?;
                invoke::<()>("set_text_merge_dirty",serde_json::json!({"dirty":next!=self.committed.get_untracked()})).await?;
                self.undo.update(|h|{h.push(old);if h.len()>128{h.remove(0);}});self.redo.set(Vec::new());self.selected.set(next);self.dirty.set(self.selected.get_untracked()!=self.committed.get_untracked());Ok::<_,String>(())
            }.await;
            if let Err(e) = result {
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
                serde_json::json!({"dirty":next!=self.committed.get_untracked()}),
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
                    self.selected.set(next);
                    self.dirty
                        .set(self.selected.get_untracked() != self.committed.get_untracked());
                }
                Err(e) => self.error.set(e),
            }
            self.busy.set(false);
        });
    }
    async fn refresh(self) {
        let Some(context) = self.context() else {
            return;
        };
        match invoke::<TextDiff>("text_diff",serde_json::json!({"session":context.session,"rowId":context.row_id,"options":context.options})).await {
            Ok(next)=>{self.context.update(|c|{if let Some(c)=c{c.diff_id=next.id.clone();}});self.original.set(Some(next));},
            Err(e)=>{self.original.set(None);self.error.set(format!("{} {e}",tr(self.english.get_untracked(),"重新比较失败：","Comparison refresh failed:")));}
        }
    }
    pub fn discard(self) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        spawn_local(async move {
            match invoke::<()>("set_text_merge_dirty", serde_json::json!({"dirty":false})).await {
                Ok(()) => {
                    self.selected.set(self.committed.get_untracked());
                    self.dirty.set(false);
                    self.undo.set(Vec::new());
                    self.redo.set(Vec::new());
                    self.error.set(String::new());
                    self.refresh().await;
                }
                Err(e) => self.error.set(e),
            }
            self.busy.set(false);
        });
    }
    async fn validate_sources(self, context: &Context, original: &TextDiff) -> Result<(), String> {
        let fresh=invoke::<TextDiff>("text_diff",serde_json::json!({"session":context.session,"rowId":context.row_id,"options":TextOptions::default()})).await?;
        for side in [Side::Left, Side::Right] {
            let expected = self
                .document(&side)
                .with_untracked(|d| d.as_ref().map(|d| d.text.clone()))
                .unwrap_or_else(|| text_merge::text(original, &side));
            if text_merge::text(&fresh, &side) != expected {
                return Err(tr(self.english.get_untracked(),"来源或目标在合并期间发生变化；未保存缓冲区已保留，请检查后重新比较。","Source or target changed during merging; unsaved buffers retained. Review and compare again.").into());
            }
        }
        Ok(())
    }
    /// None saves both dirty sides; Some saves only that pane. A partial failure
    /// preserves the other buffer and records which document already committed.
    pub fn save(self, side: Option<Side>, saved: impl Fn(String) + Copy + Send + Sync + 'static) {
        if self.busy.get_untracked() || !self.dirty.get_untracked() {
            return;
        }
        let (Some(context), Some(original)) = (self.context(), self.original.get_untracked())
        else {
            return;
        };
        let Some(preview) = self.preview() else {
            return;
        };
        self.busy.set(true);
        self.error.set(String::new());
        spawn_local(async move {
            let outcome=async {
                for target in [Side::Left,Side::Right] {
                    if side.as_ref().is_some_and(|s|*s!=target) || !self.side_dirty(&target){continue;}
                    self.validate_sources(&context,&original).await?;
                    let signal=self.document(&target);
                    let document=signal.get_untracked().ok_or("Document unavailable")?;
                    let text=text_merge::text(&preview.diff,&target);
                    let result=invoke::<SaveResult>("save_document",serde_json::json!({"documentId":document.id,"text":text,"options":SaveOptions::default()})).await?;
                    signal.set(Some(result.document));
                    self.committed.update(|c|*c.side_mut(&target)=self.selected.get_untracked().side(&target).clone());
                    self.undo.set(Vec::new());self.redo.set(Vec::new());saved(result.backup);
                }
                self.guard().await?;
                if !self.dirty.get_untracked(){self.refresh().await;}
                Ok::<_,String>(())
            }.await;
            if let Err(e) = outcome {
                let _ = self.guard().await;
                self.error.set(e);
            }
            self.busy.set(false);
        });
    }
}
