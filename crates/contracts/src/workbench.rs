//! Native project/workspace contracts. Display paths are never file capabilities.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanSettings {
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub fetch: String,
    pub push: String,
    pub match_key: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub common_id: String,
    pub match_key: Option<String>,
    pub branch: String,
    pub head: Option<String>,
    pub changed: usize,
    pub conflicts: usize,
    pub ahead: usize,
    pub behind: usize,
    pub favorite: bool,
    pub last_opened: String,
    pub available: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub projects: Vec<crate::LibraryEntry>,
    pub settings: ScanSettings,
    pub repositories: Vec<Repository>,
    pub warnings: Vec<String>,
    pub scanned_directories: usize,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub path: String,
    pub original_path: Option<String>,
    pub index: String,
    pub worktree: String,
    pub untracked: bool,
    pub conflicted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reference {
    pub id: String,
    pub name: String,
    pub full_name: String,
    pub target: String,
    pub kind: String,
    pub upstream: String,
    pub tracking: String,
    pub current: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryState {
    pub repository: Repository,
    pub revision: String,
    pub operation: Option<String>,
    pub changes: Vec<Change>,
    pub references: Vec<Reference>,
    pub remotes: Vec<Remote>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub date: String,
    pub subject: String,
    pub decorations: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub reference: Option<String>,
    pub search: String,
    pub author: String,
    pub since: String,
    pub until: String,
    pub path: Option<String>,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Stage {
        files: Vec<String>,
    },
    Unstage {
        files: Vec<String>,
    },
    Discard {
        files: Vec<String>,
    },
    Commit {
        message: String,
        amend: bool,
    },
    CreateBranch {
        name: String,
        start: Option<String>,
    },
    Switch {
        reference: String,
        detach: bool,
    },
    RenameBranch {
        name: String,
        new_name: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    SetUpstream {
        branch: String,
        upstream: Option<String>,
    },
    Merge {
        reference: String,
    },
    Rebase {
        reference: String,
    },
    CherryPick {
        reference: String,
    },
    Revert {
        reference: String,
    },
    Reset {
        reference: String,
        mode: String,
    },
    Continue {
        operation: String,
    },
    Abort {
        operation: String,
    },
    Skip {
        operation: String,
    },
    Fetch {
        remote: Option<String>,
        prune: bool,
    },
    Pull {
        remote: String,
        branch: String,
        mode: String,
    },
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force_with_lease: bool,
    },
    StashSave {
        message: String,
        include_untracked: bool,
    },
    StashApply {
        index: usize,
        pop: bool,
    },
    StashDrop {
        index: usize,
    },
    StashBranch {
        name: String,
        index: usize,
    },
    CreateTag {
        name: String,
        reference: String,
        message: Option<String>,
    },
    DeleteTag {
        name: String,
    },
    PushTag {
        remote: String,
        name: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    SetRemote {
        name: String,
        url: String,
        push: bool,
    },
    RemoveRemote {
        name: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionPlan {
    pub id: String,
    pub repository_id: String,
    pub revision: String,
    pub arguments: Vec<String>,
    pub dangerous: bool,
    pub backup_reference: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionResult {
    pub output: String,
    pub backup_reference: Option<String>,
    pub state: RepositoryState,
}
