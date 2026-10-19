//! Native project/workspace contracts. Display paths are never file capabilities.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanSettings {
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Remote {
    #[serde(default)]
    pub fetch_urls: Vec<String>,
    #[serde(default)]
    pub push_urls: Vec<String>,
    #[serde(default)]
    pub push_local: bool,
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
    pub reference_settings: std::collections::BTreeMap<String, ReferenceSettings>,
    #[serde(default)]
    pub file_settings: std::collections::BTreeMap<String, FileSettings>,
    #[serde(default)]
    pub projects: Vec<crate::LibraryEntry>,
    pub settings: ScanSettings,
    pub repositories: Vec<Repository>,
    pub warnings: Vec<String>,
    pub scanned_directories: usize,
    pub truncated: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ReferenceSettings {
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FileSettings {
    pub exclusions: String,
    pub respect_gitignore: bool,
    pub show_hidden: bool,
    pub search: Option<SourceSearch>,
}
impl Default for FileSettings {
    fn default() -> Self {
        Self {
            exclusions: String::new(),
            respect_gitignore: true,
            show_hidden: false,
            search: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub bytes: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceFileAction {
    Rename {
        name: String,
    },
    Copy {
        repository: String,
        directory: Option<String>,
    },
    Move {
        repository: String,
        directory: Option<String>,
    },
    Trash,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFilePlan {
    pub id: String,
    pub operation: String,
    pub source: String,
    pub destination: Option<String>,
    pub files: usize,
    pub directories: usize,
    pub bytes: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFileInfo {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFileResult {
    pub journal: String,
    pub entry: Option<WorkspaceFile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilePage {
    pub parent: Option<String>,
    pub path: String,
    pub files: Vec<WorkspaceFile>,
    pub offset: usize,
    pub total: usize,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SourceSearch {
    pub repositories: Vec<String>,
    pub directory: Option<String>,
    pub file: Option<String>,
    pub query: String,
    pub mode: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub multiline: bool,
    pub include: String,
    pub exclusions: String,
    pub respect_gitignore: bool,
}
impl Default for SourceSearch {
    fn default() -> Self {
        Self {
            repositories: Vec::new(),
            directory: None,
            file: None,
            query: String::new(),
            mode: "content".into(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
            multiline: false,
            include: String::new(),
            exclusions: String::new(),
            respect_gitignore: true,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub file_id: String,
    pub repository_id: String,
    pub repository: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub offset_utf16: usize,
    pub length_utf16: usize,
    pub preview: String,
    pub digest: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceResults {
    pub hits: Vec<SearchHit>,
    pub searched_files: usize,
    pub skipped_files: usize,
    pub truncated: bool,
    pub cancelled: bool,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceDocument {
    pub repository_id: String,
    pub file_id: String,
    pub document: crate::TextDocument,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentStatus {
    pub id: String,
    pub changed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub id: String,
    pub path: String,
    pub original_path: Option<String>,
    pub index: String,
    pub worktree: String,
    pub untracked: bool,
    pub conflicted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub reference_settings: ReferenceSettings,
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
    #[serde(default)]
    pub snapshot: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphCommit {
    pub commit: Commit,
    pub column: usize,
    pub width: usize,
    pub edges: Vec<GraphEdge>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryPage {
    pub snapshot: String,
    pub offset: usize,
    pub has_more: bool,
    pub commits: Vec<GraphCommit>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitFile {
    pub id: String,
    pub path: String,
    pub original_path: Option<String>,
    pub status: String,
    pub old_mode: String,
    pub new_mode: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitDetail {
    pub id: String,
    pub commit: Commit,
    pub message: String,
    pub parent: Option<String>,
    pub files: Vec<CommitFile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitChildren {
    pub target: String,
    pub snapshot: String,
    pub offset: usize,
    pub total: usize,
    pub children: Vec<String>,
    pub next_offset: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitPatch {
    pub path: String,
    pub text: String,
    pub binary: bool,
    pub offset: usize,
    pub total_lines: usize,
    pub next_offset: Option<usize>,
    pub long_lines: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkingLine {
    pub id: Option<String>,
    pub kind: String,
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub text: String,
    pub no_newline: bool,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkingHunk {
    pub id: String,
    pub before: usize,
    pub after: usize,
    pub continued: bool,
    pub lines: Vec<WorkingLine>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkingDiff {
    pub id: String,
    pub repository_id: String,
    pub file_id: String,
    pub revision: String,
    pub path: String,
    pub area: String,
    pub partial: bool,
    pub reason: Option<String>,
    pub hunks: Vec<WorkingHunk>,
    pub raw: String,
    pub offset: usize,
    pub total_lines: usize,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    CreateAndSwitch {
        name: String,
        start: Option<String>,
    },
    CheckoutRemote {
        reference: String,
        name: String,
    },
    SwitchAndUpdate {
        reference: String,
        name: Option<String>,
        mode: String,
    },
    SwitchAndRebase {
        reference: String,
        name: Option<String>,
    },
    UpdateCurrent {
        mode: String,
    },
    PushBranch {
        branch: String,
        remote: String,
        target: String,
        set_upstream: bool,
        force_with_lease: bool,
    },
    StageSelection {
        diff_id: String,
        hunks: Vec<String>,
        lines: Vec<String>,
    },
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
    StashUse {
        id: String,
        operation: StashOperation,
        name: Option<String>,
        restore_index: bool,
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
    ClearRemotePush {
        name: String,
    },
    RemoveRemote {
        name: String,
    },
    CreateWorktree {
        destination_id: String,
        folder: String,
        reference_id: Option<String>,
        new_branch: Option<String>,
        detach: bool,
    },
    RemoveWorktree {
        id: String,
    },
    PruneWorktrees,
    SetWorktreeLock {
        id: String,
        reason: Option<String>,
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
    #[serde(default)]
    pub recovery_path: Option<String>,
    pub output: String,
    pub backup_reference: Option<String>,
    pub state: RepositoryState,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub id: String,
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub main: bool,
    pub current: bool,
    pub available: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorktreeList {
    pub state: RepositoryState,
    pub catalog: Catalog,
    pub entries: Vec<WorktreeEntry>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorktreeDestination {
    pub id: String,
    pub path: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorktreeOpen {
    pub state: RepositoryState,
    pub catalog: Catalog,
}

/// Reference IDs come from the captured repository state, never from UI paths.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompareTarget {
    Current,
    Reference { id: String },
    WorkingTree { include_untracked: bool },
    Index,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReferenceComparison {
    pub left_reference_id: String,
    pub target: CompareTarget,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StashEntry {
    pub id: String,
    pub position: usize,
    pub object: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub parents: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StashPage {
    pub state: RepositoryState,
    pub entries: Vec<StashEntry>,
    pub offset: usize,
    pub has_more: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StashDetail {
    pub entry: StashEntry,
    pub area: String,
    pub detail: CommitDetail,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StashOperation {
    Apply,
    Pop,
    Drop,
    Branch,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteProbe {
    pub remote: String,
    pub push: bool,
    pub references: usize,
    pub default_branch: Option<String>,
    pub elapsed_ms: String,
}
