//! Serializable desktop/WASM/CLI contracts. No filesystem or runtime dependencies.
use serde::{Deserialize, Serialize};
pub mod binary_query;
pub mod edit_history;
pub mod merge_edit;
pub mod text_view;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Same,
    Changed,
    LeftOnly,
    RightOnly,
    MetadataMatch,
    TypeConflict,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Directory,
    Link,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    /// Decimal string: lossless across JavaScript and WASM boundaries.
    pub bytes: String,
    pub modified_ms: Option<String>,
    pub digest: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub directory_summary: Option<Summary>,
    pub status: Status,
    pub left: Option<Entry>,
    pub right: Option<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRequest {
    pub left: String,
    pub right: String,
    pub exclusions: String,
    pub content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub request: CompareRequest,
    pub english: bool,
    #[serde(default)]
    pub view: SessionView,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub same: usize,
    pub changed: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub unverified: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    #[serde(default)]
    pub policy: Option<ComparisonPolicy>,
    pub session: String,
    pub left: String,
    pub right: String,
    pub summary: Summary,
    pub rows: Vec<Row>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub rows: Vec<Row>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenedComparison {
    pub session: String,
    pub left: String,
    pub right: String,
    pub summary: Summary,
    pub warnings: Vec<String>,
    pub page: Page,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    #[serde(default)]
    pub intraline_limited: bool,
    #[serde(default)]
    pub changes: Vec<TextRange>,
    pub tag: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
    /// Original right-side text for lines equal under normalization.
    pub right_text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TextOptions {
    pub ignore_case: bool,
    pub ignore_whitespace: bool,
    pub ignore_line_endings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDiff {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub intraline_limited: bool,
    pub path: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocument {
    pub id: String,
    pub absolute_path: String,
    pub path: String,
    pub side: Side,
    pub text: String,
    pub encoding: Encoding,
    pub bom: bool,
    pub line_ending: LineEnding,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveResult {
    pub document: TextDocument,
    pub backup: String,
}

fn legacy_file_plan_version() -> u32 {
    1
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    #[default]
    Copy,
    Delete,
    Rename,
    CreateDirectory,
    RemoveDirectory,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FilePlanPolicy {
    #[default]
    SelectedCopy,
    DeleteSelected {
        target_side: Side,
    },
    RenameSelected {
        target_side: Side,
    },
    MirrorFiles,
    MirrorTree,
    Reconcile {
        target_side: Side,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum FilePermissions {
    Unix { mode: u32 },
    ReadOnly { value: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePaths {
    pub destination: String,
    pub destination_native: NativePath,
    pub temporary_native: NativePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryIdentity {
    pub platform: String,
    pub volume: String,
    pub index: String,
    pub birth: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory_identity: Option<DirectoryIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename_to: Option<RenamePaths>,
    #[serde(default)]
    pub operation: FileOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_permissions: Option<FilePermissions>,
    pub row_id: String,
    #[serde(default)]
    pub relative_native: Option<NativePath>,
    pub path: String,
    #[serde(alias = "overwrite")]
    pub target_existed: bool,
    pub bytes: String,
    pub source_digest: Option<String>,
    pub target_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub target: String,
    #[serde(default = "legacy_file_plan_version")]
    pub version: u32,
    #[serde(default)]
    pub policy: FilePlanPolicy,
    pub id: String,
    #[serde(default)]
    pub target_native: Option<NativePath>,
    #[serde(default)]
    pub source_side: Side,
    pub session: String,
    pub actions: Vec<FileAction>,
    pub backup_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub path: String,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<String>,
    pub plan: SyncPlan,
    pub actions: Vec<ActionResult>,
    pub journal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    #[serde(default)]
    pub conflict_versions: Vec<MergeConflict>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub conflict_token: Option<String>,
    pub text: String,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryLine {
    #[serde(default)]
    pub left_changed: Vec<usize>,
    #[serde(default)]
    pub right_changed: Vec<usize>,
    pub offset: String,
    pub left_hex: String,
    pub right_hex: String,
    pub left_ascii: String,
    pub right_ascii: String,
    pub changed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryPage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub left_available: bool,
    #[serde(default)]
    pub right_available: bool,
    pub path: String,
    pub offset: String,
    pub left_size: String,
    pub right_size: String,
    pub lines: Vec<BinaryLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableOptions {
    /// Exact unique header names; logical columns follow left then right-only order.
    #[serde(default)]
    pub align_columns_by_name: bool,
    #[serde(default)]
    pub left_source: TableSource,
    #[serde(default)]
    pub right_source: TableSource,
    pub delimiter: String,
    pub headers: bool,
    /// Zero-based key column (logical union index in name mode). Duplicate keys are rejected.
    pub key_column: Option<usize>,
    /// Absolute numeric tolerance; enabled explicitly, finite and nonnegative.
    pub numeric_tolerance: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    pub key: String,
    pub left: Option<Vec<String>>,
    pub right: Option<Vec<String>>,
    pub changed_columns: Vec<usize>,
    pub status: Status,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableComparison {
    #[serde(default)]
    pub column_map: Vec<TableColumn>,
    #[serde(default)]
    pub headers_changed: bool,
    #[serde(default)]
    pub columns: usize,
    #[serde(default)]
    pub options: Option<TableOptions>,
    #[serde(default)]
    pub left_tables: Vec<HtmlTableInfo>,
    #[serde(default)]
    pub right_tables: Vec<HtmlTableInfo>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub left_headers: Vec<String>,
    pub right_headers: Vec<String>,
    pub rows: Vec<TableRow>,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub left: Option<usize>,
    pub right: Option<usize>,
}
impl TableComparison {
    pub fn source_column(&self, logical: usize, left: bool) -> Option<usize> {
        if self.column_map.is_empty() {
            Some(logical)
        } else {
            self.column_map
                .get(logical)
                .and_then(|c| if left { c.left } else { c.right })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenedTable {
    pub id: String,
    pub path: String,
    pub report: TableComparison,
    pub total: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveOptions {
    pub encoding: Option<Encoding>,
    pub line_ending: Option<LineEnding>,
    pub bom: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePreview {
    pub id: String,
    pub target: String,
    pub backup: String,
    pub original_digest: String,
    pub current_digest: String,
    pub bytes: String,
}

/// Lossless durable paths. Paths from another operating system are not executed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "encoding", content = "value", rename_all = "snake_case")]
pub enum NativePath {
    Unix(Vec<u8>),
    Windows(Vec<u16>),
    Utf8(String),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAction {
    pub path: String,
    pub operation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPreview {
    pub id: String,
    pub source_journal: String,
    pub target: String,
    pub backup_root: String,
    pub actions: Vec<RecoveryAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeAction {
    pub path: String,
    pub operation: String,
    pub state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumePreview {
    pub id: String,
    pub journal: String,
    pub source: String,
    pub target: String,
    pub backup_root: String,
    pub satisfied: usize,
    pub pending: usize,
    pub cleanup_files: usize,
    pub total: usize,
    pub actions: Vec<ResumeAction>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResult {
    pub plan: RecoveryPreview,
    pub actions: Vec<ActionResult>,
    pub journal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryEntry {
    pub id: String,
    pub modified_ms: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    #[default]
    Path,
    Status,
    LeftSize,
    RightSize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderQuery {
    pub parent_id: Option<String>,
    pub recursive: bool,
    pub search: String,
    pub differences_only: bool,
    pub sort: FolderSort,
    pub descending: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub id: String,
    pub label: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderPage {
    pub page: Page,
    pub breadcrumbs: Vec<Breadcrumb>,
    #[serde(default)]
    pub outline: std::collections::BTreeMap<String, OutlineNode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineNode {
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
    pub has_children: bool,
    pub context_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionView {
    pub search: String,
    pub differences_only: bool,
    pub recursive: bool,
    pub sort: FolderSort,
    pub descending: bool,
    pub reverse_sync: bool,
    pub text_options: TextOptions,
}
impl Default for SessionView {
    fn default() -> Self {
        Self {
            search: String::new(),
            differences_only: true,
            recursive: true,
            sort: FolderSort::Path,
            descending: false,
            reverse_sync: false,
            text_options: TextOptions::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub updated_ms: String,
    pub revision: String,
    pub session: SavedSession,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PictureOptions {
    pub tolerance: u8,
    pub ignore_alpha: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PixelBounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PictureSummary {
    pub left_dimensions: Option<(u32, u32)>,
    pub right_dimensions: Option<(u32, u32)>,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub total_pixels: String,
    pub changed_pixels: String,
    pub max_channel_delta: u8,
    pub bounds: Option<PixelBounds>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PicturePreview {
    pub path: String,
    pub summary: PictureSummary,
    pub preview_width: u32,
    pub preview_height: u32,
    pub left_png: Option<String>,
    pub right_png: Option<String>,
    pub difference_png: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMergeRequest {
    #[serde(default)]
    pub two_way: bool,
    pub base: String,
    pub left: String,
    pub right: String,
    pub output: String,
    pub exclusions: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FolderResolution {
    Combine,
    Base,
    Left,
    Right,
    Delete,
    Auto,
    Conflict,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMergeRow {
    pub id: String,
    pub path: String,
    pub classification: String,
    pub resolution: FolderResolution,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMergePreview {
    pub id: String,
    pub two_way: bool,
    pub base: String,
    pub left: String,
    pub right: String,
    pub output: String,
    pub backup_root: String,
    pub conflicts: usize,
    pub total: usize,
    pub rows: Vec<FolderMergeRow>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMergeResult {
    pub plan: FolderMergePreview,
    pub actions: Vec<ActionResult>,
    pub journal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRequest {
    pub left: String,
    pub right: String,
    pub exclusions: String,
    pub content: bool,
}

/// Export a content-free tree inventory into a new file outside the source tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSnapshotRequest {
    pub root: String,
    pub destination: String,
    pub exclusions: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSnapshotInfo {
    pub version: u32,
    pub captured_ms: String,
    pub source_name: String,
    pub entries: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitSource {
    #[default]
    Commit,
    Index,
    WorkingTree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCompareRequest {
    /// Include nonignored untracked paths only when a working-tree source is selected.
    #[serde(default)]
    pub include_untracked: bool,
    #[serde(default)]
    pub left_source: GitSource,
    #[serde(default)]
    pub right_source: GitSource,
    pub repository: String,
    pub left_revision: String,
    pub right_revision: String,
    pub exclusions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub id: usize,
    pub left: String,
    pub base: String,
    pub right: String,
}

/// UTF-8 byte range in an original, unnormalized line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportSource {
    Local,
    Archive,
    TreeSnapshot,
    Git,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportFormat {
    Html,
    Json,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportRequest {
    pub source: ReportSource,
    pub format: ReportFormat,
    pub session: String,
    pub destination: String,
    pub english: bool,
    pub differences_only: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportResult {
    pub path: String,
    pub session: String,
    pub rows: usize,
    pub bytes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationBasis {
    Sha256,
    SizeAndTime,
    SizeOnly,
    GitObjectIdentity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonPolicy {
    pub exclusions: String,
    pub verification: VerificationBasis,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum TableSource {
    #[default]
    Auto,
    Delimited,
    Csv,
    Tsv,
    Html {
        table: usize,
    },
    Xlsx {
        sheet: usize,
    },
}
impl Default for TableOptions {
    fn default() -> Self {
        Self {
            align_columns_by_name: false,
            left_source: TableSource::Auto,
            right_source: TableSource::Auto,
            delimiter: ",".into(),
            headers: true,
            key_column: None,
            numeric_tolerance: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlTableInfo {
    pub index: usize,
    pub caption: String,
    pub element_id: String,
    pub rows: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDocumentInfo {
    #[serde(default)]
    pub left_xlsx: bool,
    #[serde(default)]
    pub right_xlsx: bool,
    #[serde(default)]
    pub left_sheets: Vec<WorksheetInfo>,
    #[serde(default)]
    pub right_sheets: Vec<WorksheetInfo>,
    pub left_tsv: bool,
    pub right_tsv: bool,
    pub left_tables: Vec<HtmlTableInfo>,
    pub right_tables: Vec<HtmlTableInfo>,
    pub left_error: Option<String>,
    pub right_error: Option<String>,
    pub id: String,
    pub path: String,
    pub left_html: bool,
    pub right_html: bool,
    pub left_available: bool,
    pub right_available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinarySearchMode {
    Hex,
    Utf8,
    Utf16Le,
    Utf16Be,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinarySearchOptions {
    pub query: String,
    pub mode: BinarySearchMode,
    pub offset: String,
    pub limit: usize,
    pub side: Side,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinarySearchResult {
    pub path: String,
    pub offset: String,
    pub searched_until: String,
    pub next_offset: Option<String>,
    pub size: String,
    pub positions: Vec<String>,
    pub pattern_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentLocation {
    pub id: String,
    pub path: String,
    pub absolute_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileApplyReply {
    pub result: SyncResult,
    pub locations: Vec<DocumentLocation>,
    pub stale_documents: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryApplyReply {
    pub result: RecoveryResult,
    pub locations: Vec<DocumentLocation>,
    pub stale_documents: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPhase {
    Running,
    PauseRequested,
    Paused,
    CancelRequested,
    Completed,
    Failed,
    Cancelled,
}
impl TaskPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    File,
    Recovery,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHandle {
    pub id: String,
    pub kind: OperationKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub handle: TaskHandle,
    pub phase: TaskPhase,
    pub stage: String,
    pub current_path: Option<String>,
    pub completed_actions: usize,
    pub total_actions: usize,
    pub bytes_read: String,
    pub bytes_written: String,
    pub elapsed_ms: String,
    pub error: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OperationOutcome {
    File(FileApplyReply),
    Recovery(RecoveryApplyReply),
    Bidirectional(BidirectionalApplyReply),
    BidirectionalRecovery(BidirectionalRecoveryReply),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorksheetInfo {
    pub index: usize,
    pub name: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineInfo {
    pub path: String,
    pub id: String,
    pub revision: String,
    pub entries: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalRow {
    pub baseline: Option<BidirectionalValue>,
    pub left: Option<BidirectionalValue>,
    pub right: Option<BidirectionalValue>,
    pub id: String,
    pub path: String,
    pub state: String,
    pub action: String,
    pub winner: Option<Side>,
    pub can_left: bool,
    pub can_right: bool,
    pub subtree: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalPreview {
    pub can_prefer_left: bool,
    pub can_prefer_right: bool,
    pub id: String,
    pub baseline: BaselineInfo,
    pub backup_root: String,
    pub rows: Vec<BidirectionalRow>,
    pub total: usize,
    pub conflicts: usize,
    pub blocked: usize,
    pub actions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalResult {
    #[serde(default)]
    pub details_in_journal: bool,
    pub baseline_info: BaselineInfo,
    pub started_journals: Vec<String>,
    pub intent_digest: String,
    pub id: String,
    pub journal: String,
    pub baseline: String,
    pub baseline_updated: bool,
    pub phase: String,
    pub planned_journals: Vec<String>,
    pub operations: Vec<SyncResult>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalRecoveryPreview {
    pub total: usize,
    pub rows: Vec<BidirectionalRecoveryRow>,
    pub id: String,
    pub journal: String,
    pub baseline: String,
    pub restore_baseline: bool,
    pub operations: Vec<RecoveryPreview>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalRecoveryResult {
    #[serde(default)]
    pub details_in_journal: bool,
    pub journal: String,
    pub baseline_restored: bool,
    pub operations: Vec<RecoveryResult>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct BidirectionalApplyReply {
    pub result: BidirectionalResult,
    pub locations: Vec<DocumentLocation>,
    pub stale_documents: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct BidirectionalRecoveryReply {
    pub result: BidirectionalRecoveryResult,
    pub locations: Vec<DocumentLocation>,
    pub stale_documents: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalRecoveryRow {
    pub path: String,
    pub operation: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalValue {
    pub kind: String,
    pub digest: Option<String>,
}
