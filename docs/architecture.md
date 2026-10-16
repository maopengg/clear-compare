# Architecture

## Boundaries

| Layer | Responsibility | Dependencies |
|---|---|---|
| contracts | Serializable DTOs, status and operation contracts | serde |
| core | Rules, native paths, streaming hashes, text diff, plans, backups and journals | contracts and focused Rust libraries |
| cli | Command parsing and machine-readable output | core and contracts |
| desktop | Native dialogs, IPC authorization, single-operation state, persistence | Tauri and core |
| ui | Leptos signals, paging, selection, diff and plan review | contracts and WASM bindings |

The core does not depend on Tauri or Leptos. Original native paths stay in the backend snapshot, indexed by opaque IDs. Display strings do not determine filesystem paths. Content sizes and timestamps cross IPC as decimal strings. Leptos calls a narrow Tauri bridge and receives only bounded pages.

## Compare lifecycle

Validate canonical roots → reject overlapping roots → parse exclusions → walk without following links → collect metadata → optionally hash contents → classify files → retain native snapshot → return summary and first page.

Only verified matching file digests are reported as content-equal. Directory status aggregates included descendants; errors and unverified contents prevent a content-equal directory status. Excluded/unscanned contents are outside that assertion. Metadata mode deliberately labels matching metadata as unverified. I/O failures are errors, never matches.

## Write lifecycle

Select eligible source-side regular files → create immutable backend plan → review plan in UI → consume plan ID once → revalidate roots and hashes → write temporary sibling → verify copied digest → verify old-file backup → recheck target → replace → verify result → update journal.

Preview creates no destination file. Execution may create destination parent directories. Copy policies preserve destination-only files; explicit selected-deletion policies remove only reviewed regular files after verified backups. Exclusions still apply. A failed action stops subsequent actions; the journal shows completed, failed and pending items. A cancellation cannot undo actions already completed.

## Resource limits

- A maximum of 200,000 scanned entries per root in the preview release.
- At most 500 directory entries in an IPC page.
- File hashes, copy, deletion backups and recovery use 64 KiB buffers.
- New pending file-operation journals must fit 32 MiB; imported recovery journals are limited to 64 MiB.
- Text workflows support UTF-8 and BOM-marked UTF-16 with 4 MiB encoded/8 MiB decoded text and 30,000 lines per side.
- Text comparison uses 500 aligned rows per page; file-plan review initially renders 500 actions and can show more.
- Only one scan/plan/write task runs at a time. Native work is dispatched off the UI thread.

## Persistence

The current comparison settings are kept under Tauri's application data directory. Backups are grouped by a unique plan ID, with `files/` and `journal.json`. No telemetry is sent. Backend paths and journals must be redacted before public bug reports.

## Remaining work

Recovery-journal resume/redo, full metadata preservation, large-scale performance baselines, workspace restoration, broader encodings, further merge/viewer capabilities and converters remain on the roadmap. The initial architecture leaves these separate from the UI and desktop runtime.

## Specialized workspace panels

Leptos editing, merge, table and binary panels are separate modules in `crates/ui/src/panels`. Each consumes typed signals and operation callbacks. Native paths and file mutation remain in `clear-core` and explicitly authorized Tauri commands.

Directional synchronization resolves source/target roots from the immutable plan's `source_side`. It does not copy or reverse the entire directory snapshot. Changing direction clears the UI selection and requires a new preview. Backups always belong to the destination being replaced.

## File-operation plans and recovery

Sync journals include the destination root and relative entry paths in native byte/wide-unit encoding. UI display labels never reconstruct filesystem inputs. Recovery rejects paths from a different operating system, symlink traversals, mismatched journal placement, modified current files and invalid original backups.

Recovery previews own a native plan with pinned expected digests. Tauri resolves history IDs under its private backup directory and consumes exact cached preview tokens. CLI preview is read-only; `--apply` generates a fresh plan. Recovery streams contents to verified current-file backups before replacement or removal and persists action states in a separate journal. The original operation journal and backups remain intact. Deletion recovery pins absence, verifies original bytes/basic permissions and persists without overwriting a recreated path. Native relative paths generate recovery display labels rather than trusting imported labels. Version-two copy/deletion plans and version-one copy journals retain read compatibility; version-three rename plans additionally pin destination and plan-specific temporary native paths. Native no-replace rename uses two same-directory steps so case-only changes work on insensitive filesystems without overwriting another file. Recovery reconciles staged files and reverses the name with new current-file backups. Editor relocation changes native/cache and IPC location fields only; saved baselines, draft text and undo history stay intact. Canonically equivalent Unicode spellings are accepted only when the OS resolves them to the same regular file, without case folding or replacing native paths. See [file operations](file-operations.md) for limits and reconciliation. Journals/backups/mutations add Unix directory flushes; platform/storage limitations still preclude a universal power-loss guarantee.

## Directory scope queries

The immutable snapshot indexes native directory paths. Directory states and regular-file byte totals propagate from scanned descendants; exclusions, errors and unverified entries remain visible in that aggregation. Query inputs use directory IDs, not display-path reconstruction. Native filtering and stable sorting run across the selected scope before a maximum-500-row response is cloned for IPC. Descendant aggregation walks ancestor paths rather than rescanning the full row set for each directory.

## Picture comparison

Native image decoding is bounded by input size, dimensions and pixel count. The decoder applies [EXIF orientation](https://docs.rs/image/0.25.10/image/enum.DynamicImage.html#method.apply_orientation) before RGBA8 comparison. Pixel statistics use top-left-aligned full images, while IPC carries normalized PNG previews of at most 1024 pixels per side. The heatmap aggregates changed pixels into preview buckets so a single changed source pixel is retained. Browser image elements display only generated data URLs; image files and paths are not exposed through generic filesystem permissions.

## Folder merge output planning

The merge planner scans a shared baseline and both input trees, classifies changes using verified content and ancestor changes, and owns native paths plus pinned source entries. Two-way mode uses a virtual empty baseline without creating any directory in the input trees. Manual directory choices cascade to their entire subtree. The GUI resolves opaque row IDs; CLI choices match explicit relative paths against scanned entries and never create paths from display labels.

Execution refuses unresolved conflicts and nonempty/overlapping output. It revalidates all input trees before writing, streams copied files into sibling temporary files, verifies hashes and persists without replacing existing files. Generated text is preserved separately with its digest in a private run manifest. Final output is rescanned against the planned inventory and digests. Input trees are never written, and partial output is reported through durable action states.

## Read-only ZIP trees

Archive comparison owns an independent native snapshot of ZIP/directory sources. A bounded classic central-directory preflight runs before the ZIP library parses metadata. Unsafe names, duplicate members, ambiguous ancestors, ZIP64 and multidisk archives are rejected. Stored/Deflate member data is streamed through SHA-256 with CRC validation and declared-size, entry-count, aggregate-expansion and ratio limits. Unsupported/encrypted/special entries remain errors; links are unverified. Exclusions run before member decoding.

The compressed container is fingerprinted before/after comparison and again around bounded text preview. ZIP members are never extracted or written. Metadata-only mode never upgrades a size match to content equality. Native snapshots own relative paths/member indices; UI receives opaque IDs and at most 500 rows per request. This adapter has no update, extraction, nested-archive or sync capability.

## Content-free tree snapshots

A separate versioned `.cctree` inventory records paths, entry kinds, size/time/error metadata and capture exclusions. It has no content/hash fields and cannot participate in text preview, sync, merge or recovery. Export scans without opening file bodies, writes a bounded temporary file outside the source tree, syncs it and persists without replacing an existing destination. A live scan is not a filesystem-consistent point-in-time backup.

Portable path components are validated independently of host separators; nonportable names use lossless OS encoding and are rejected on incompatible systems. Import caps file size and entry count, rejects unknown fields/versions, invalid metadata, duplicate/traversing paths and missing/non-directory ancestors before applying exclusions. Historical matching sizes/timestamps stay unverified; metadata changes are distinct from proven content changes. Native snapshots return at most 500 rows and have independent opaque sessions.

## Git commit/index/working-tree adapter

The optional Git adapter invokes installed Git directly with typed argument arrays. Revisions are pinned with [`rev-parse --verify --end-of-options`](https://git-scm.com/docs/git-rev-parse); [`ls-tree -r -t -z -l --full-tree`](https://git-scm.com/docs/git-ls-tree) supplies raw tree names, modes, full object IDs and sizes. Filename bytes are preserved on Unix; names unsupported on the host fail explicitly. Text previews read pinned blobs using [`cat-file blob`](https://git-scm.com/docs/git-cat-file), without checkout, textconv, smudge filters or application filesystem writes. Git object identity, rather than timestamps or a new SHA-256 read, determines file equality.

The command runner removes inherited Git redirection/config/trace environment, disables optional locks and replacement objects, overrides fsmonitor, disables prompts and all transport protocols, and passes [`--no-lazy-fetch`](https://git-scm.com/docs/git). Unsupported Git versions fail instead of falling back to fetching. Pipe readers cap stdout/stderr independently while a worker polls cancellation and a 15-second deadline; failure kills/reaps the child and joins readers. Tree output and path counts are bounded; preview blobs remain limited to 4 MiB. Submodules are explicit errors, symlinks stay unverified, LFS pointers are not dereferenced, and writes and recovery are not implemented. Working-tree sources enumerate tracked/optional-untracked Git paths and read regular files natively with parent/link checks, streaming raw blob/content digests, before/after metadata checks and guarded bounded previews. They honor executable-mode policy while deliberately skipping clean/smudge/EOL normalization and index substitutions for sparse missing files. Index sources use bounded cached formatted ls-files output with raw names, object metadata and stage numbers; unmerged stages become explicit errors. Both index sides share one captured listing and later previews use pinned blob IDs. Split/sparse index handling stays in Git; synthesized parent directories feed the existing aggregation. See [Git comparison semantics](git-comparison.md) for intent-to-add, identity labels and source limitations.

## Structured text-conflict editing

Generated merge results carry original left/base/right fragments and stable per-group IDs in addition to native conflict provenance. All four markers include the result tag and group ID. Shared contract-side parsing validates order, identity, completeness and uniqueness before exposing byte ranges; DOM selection converts those ranges to UTF-16 units. Ordinary Markdown separators and unrelated tags are not navigable conflicts.

Version selection replaces exactly one unchanged generated block with original bytes. It rejects manually changed blocks instead of discarding edits, preserves original final-newline absence and CR/LF bytes, and concatenates both sides without inventing a separator. Buffered edits/resolutions share the bounded splice history and IME composition boundaries. Undo can reintroduce markers, so native save provenance remains authoritative across editor staging/saves. Generated results are capped at 8 MiB before staging; installer and real-WebView interaction verification remain deferred.

## Bounded intraline text view

Line diff retains its original equal/delete/insert sequence for CLI consumers and source reconstruction. Replacement groups receive paired-by-position scalar differences mapped back to original UTF-8 byte ranges. Mapping follows the same whitespace-before-contextual-lowercase-before-EOL normalization order as line comparison, retaining expansions and original text. Per-pair byte/scalar caps and a shared scalar/time budget fall back to explicit whole-line hints; per-line and result-level flags identify that loss of precision.

The WASM view builds immutable row indices, aligns contiguous change groups by ordinal position, and computes hunk/search targets over the complete result before choosing a 500-row page. A fresh comparison identity prevents keyed row reuse across different previews of the same path. The two panes synchronize both axes and share a measured content width so the shorter side does not clamp the longer side. End-of-line badges expose significant changes invisible in ordinary text layout while ignored ending styles remain plain. Rendering clips only the displayed body after 8192 Unicode scalars and labels it; the underlying source model and merge reconstruction stay complete. Highlight ranges are validated/clipped before rendering as escaped text nodes. Unicode source reconstruction, search, paging and rendering invariants are exercised in native contract/core tests; real WebView acceptance remains deferred.

## Comparison report export

Native snapshots retain the exclusion string and a typed verification basis when they are created. Directory, archive, tree-snapshot and Git report commands resolve opaque comparison IDs against those cached snapshots rather than trusting a frontend page or current settings. Export runs behind the shared operation guard and bounded worker. Git protection paths come from actual worktree, per-worktree Git and common Git directories, including when the requested repository path is a subdirectory.

HTML streams escaped text into a self-contained, script-free document with a restrictive CSP. Versioned JSON borrows matching rows instead of cloning their strings, records policy/source and retains the full summary even when same rows are omitted. Metadata uncertainty, errors, original warnings and directory aggregation remain visible. Neither format reads file bodies or rescans sources. Reports are review/integration artifacts, not restore instructions.

Writes use a bounded 128 MiB stream, cancellation checks, a synced sibling temporary file and no-clobber persistence. Existing files, dangling symlinks and destinations under protected roots are rejected. UI export controls capture session/options before the dialog and safely discard a completion notice if its panel was closed during the operation. Packaged-client acceptance remains deferred.

## Frozen multi-format table documents

Table opening resolves a folder snapshot entry ID to checked native paths, reads bounded regular files and verifies original digests when the folder snapshot has them. Captured decoded text stays in one bounded native document slot; the UI receives an opaque document ID, format hints and static HTML catalogs. Reapplying formats/indices/rules uses that immutable text, and closing or starting a new folder comparison releases the cached document/results.

HTML uses the pinned [html5ever parser](https://docs.rs/html5ever/0.39.0/html5ever/) with a [scraper static tree sink](https://docs.rs/scraper/0.27.0/scraper/). Parsing feeds small chunks and checks node/time/cancellation budgets; traversal checks depth without recursive application walking. Scripts are never executed and resources are never fetched. The extractor enumerates bounded table catalogs and expands only the selected table, detecting overlap and limiting dimensions, cells and duplicated text. Nested-table content stays separate, and column/header/key/tolerance comparison reuses the existing string-table engine.

The native result slot owns the complete comparison, returning at most 500 rows. The UI renders 25 columns per window and clips long displayed cells while retaining the original model. Source formats and table choices are independent per side; metadata and errors belong to the pinned document, not to later filesystem contents. Real WebView acceptance remains deferred.

## Whole-comparison regular-file mirror

A version-four mirror policy combines existing verified copy/deletion actions with an explicit source side. Native planning covers the captured roots independently of frontend scope/filter/selection and checks a SHA-256 inventory before preview and execution. It rejects unsupported entries/type conflicts instead of treating them as absent; both complete path sets and originally equal file digests are revalidated. Individual target-only deletions also require source absence immediately before and after backup. Copies precede deletions and share one journal/stop-on-error loop. Existing mixed-action recovery and editor invalidation apply. Empty directory differences are retained explicitly in the regular-file policy. The separate version-five tree policy extends directory handling; baseline synchronization remains planned. See [file mirror](file-mirror.md).

## Directory-tree mirror and recovery ordering

Version-five tree mirror composes parent-first directory creation, verified file copies/deletions and child-first empty-directory removal. Native planning enumerates direct children to retain ancestors required by excluded/unplanned entries; the complete action sequence and captured content inventory are revalidated before execution. Removals pin native directory identity/basic permissions and create path-keyed flat metadata records, while successful mkdir records the created identity outside comparison roots. No recursive input deletion or in-tree marker is used.

Recovery reverses version-five action dependencies and verifies ownership/metadata records before touching directories. It rejects recreated originals, replaced created directories and unexpected children, with fresh preflight before recovery and per-action checks. Basic permissions are restored; owner/ACL/timestamp replication and automatic partial-recovery resume remain planned. Unix identities use device/inode and optional birth time; Windows uses the pinned winapi-util handle-information API. Decimal identity fields avoid JavaScript precision loss. See [tree mirror](tree-mirror.md) for interrupted-record and filesystem limitations.

## Bounded asynchronous file tasks

Tauri file-application/recovery commands now return opaque task handles immediately. An Arc-owned backend holds one task/result slot and keeps write admission until the exact terminal result is collected once. The UI polls small status snapshots and reconnects to uncollected same-process work on startup. Full outcomes retain existing editor-location updates and journal results.

The core task scope binds an exact cancellation token to cooperative pause checkpoints on the synchronous worker thread. Native mutation guards defer pause through fresh final validation/write/mandatory verification. Pausable complete-scope preflight repeats after an acknowledged-pause generation change. Scope drop restores prior bindings even after unwinding. The controller creates no service or progress thread; byte counters track instrumented file streams rather than claiming a completion percentage. See [operation tasks](operation-tasks.md) for states, persistence limits and tests.


## Durable forward restart

New file operations persist immutable bounded context and its journal digest before mutation. `sync::durable` owns native identity evidence, operation locks, registered copy work files and artifact namespaces. `sync::resume` reconstructs the original native scope, reconciles action states against actual files/backups and applies only an exact reviewed preview. Each retry retains previous journals and new backups under an independent attempt directory; original history rollback follows per-action namespaces. Full-scope mirror revalidation projects verified completed actions and ignores only owned target work files until retention/cleanup. Native IPC caches opaque restart previews and executes through the shared background task registry; UI history exposes paged bilingual review. CLI uses the same core. See [forward-restart evidence, ordering and limits](operation-resume.md). No automatic startup write or recovery redo is implied.

## Excel table extraction

Frozen table documents retain bounded native byte buffers. Text formats share the existing decoder; table::xlsx validates package expansion/XML allocation hints before a pinned calamine streaming cell reader extracts saved values. Declared dimensions never allocate a dense library range. Actual positions/grid/text are bounded; missing formula caches/cell errors remain distinct from values. Worksheet catalogs and date-system normalization feed existing string-based key/name alignment and tolerance. CLI and desktop share the adapter and contracts; UI receives indices and paged results without filesystem access. See the [format policies](table-formats.md).


## Common-baseline reconciliation

`bidirectional::{baseline,planner,execute,recovery}` coordinates two target-specific version-six sync plans. Content-only native inventories and subtree fingerprints distinguish independent changes from delete/modify conflicts; explicit branch winners preserve dependency consistency. The common profile is advanced only after both actual trees match the reviewed final inventory, with a profile-sidecar lock, immutable run intent and retained before/after baselines. Existing file/directory journals remain the mutation/backup authority. Coordinated rollback validates original run/profile evidence, reverses each target and restores the previous profile bytes only after recovery succeeds. Newer profile revisions block old rollback.

`task::with_mutation_guard` composes scoped root-identity validators with inner native commit boundaries. Its thread-local stack is synchronous, token-specific, nestable and restored on scope exit/unwind; it creates no service or thread. Both ordinary and paused inner actions recheck the coordinated roots before writes. Desktop typed IPC caches native selections/plans, pages review rows and runs apply/rollback through the existing operation registry. CLI calls the same core. See [workflow, formats and failure limits](bidirectional-sync.md).

Bidirectional desktop terminal outcomes retain status, profile metadata and child journal references, marking full action details as journal-resident. The bridge omits the large action arrays; CLI/core journals retain complete evidence. Native serialization tests exercise large input results and verify a compact reply preserves failure state and references.

## Paired directory outline

The folder workspace now defaults to a native, ancestor-preserving preorder with explicit open IDs or expand-all/collapse exceptions. `query_folder` retains flat-query compatibility and returns bounded outline metadata alongside each page. The UI renders one aligned table for both roots, independent per-side metadata, explicit absent entries and native-ID selection/actions. Double-clicking a file opens the existing line/intraline text comparator. See [directory workspace](directory-workspace.md) for contracts and the distinction between mocked browser checks and native acceptance.
