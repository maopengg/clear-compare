//! Serializable desktop/WASM/CLI contracts. No filesystem or runtime dependencies.
use serde::{Deserialize, Serialize};
pub mod edit_history;
pub mod merge_edit;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyAction {
    pub row_id: String,
    #[serde(default)]
    pub relative_native: Option<NativePath>,
    pub path: String,
    pub overwrite: bool,
    pub bytes: String,
    pub source_digest: String,
    pub target_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub id: String,
    #[serde(default)]
    pub target_native: Option<NativePath>,
    #[serde(default)]
    pub source_side: Side,
    pub session: String,
    pub actions: Vec<CopyAction>,
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
    pub offset: String,
    pub left_hex: String,
    pub right_hex: String,
    pub left_ascii: String,
    pub right_ascii: String,
    pub changed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryPage {
    pub path: String,
    pub offset: String,
    pub left_size: String,
    pub right_size: String,
    pub lines: Vec<BinaryLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableOptions {
    pub delimiter: String,
    pub headers: bool,
    /// Zero-based key column. Duplicate keys are rejected.
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
    pub left_headers: Vec<String>,
    pub right_headers: Vec<String>,
    pub rows: Vec<TableRow>,
    pub summary: Summary,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCompareRequest {
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
