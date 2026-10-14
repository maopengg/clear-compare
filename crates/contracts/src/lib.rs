//! Serializable desktop/WASM/CLI contracts. No filesystem or runtime dependencies.
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
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
    pub path: String,
    pub overwrite: bool,
    pub bytes: String,
    pub source_digest: String,
    pub target_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub id: String,
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
