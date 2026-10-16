//! Explicit project commits keep profile identity, revision and settings together.
use crate::invoke;
use clear_contracts::{LibraryEntry, SavedSession};
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Store {
    pub active: RwSignal<Option<(String, String)>>,
    pub name: RwSignal<String>,
    pub saved: RwSignal<String>,
    pub library: RwSignal<Option<Vec<LibraryEntry>>>,
}
pub fn signature(name: &str, session: &SavedSession) -> String {
    serde_json::json!({"name":name,"session":session}).to_string()
}
impl Store {
    pub fn accept(self, entry: &LibraryEntry) {
        self.name.set(entry.name.clone());
        self.active
            .set(Some((entry.id.clone(), entry.revision.clone())));
        self.saved.set(signature(&entry.name, &entry.session));
        self.library.update(|entries| {
            if let Some(entries) = entries {
                entries.retain(|e| e.id != entry.id);
                entries.insert(0, entry.clone());
            }
        });
    }
    pub async fn persist(self, session: SavedSession) -> Result<(), String> {
        let Some((id, revision)) = self.active.get_untracked() else {
            return Ok(());
        };
        let name = self.name.get_untracked();
        if signature(&name, &session) == self.saved.get_untracked() {
            return Ok(());
        }
        let entry = invoke::<LibraryEntry>(
            "save_library_session",
            serde_json::json!({"id":id,"revision":revision,"name":name,"session":session}),
        )
        .await?;
        self.accept(&entry);
        Ok(())
    }
}
