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

Only verified matching file digests are reported as content-equal. A matching directory row means the directory exists on both sides; it is not a recursive subtree equality assertion. Metadata mode deliberately labels matching metadata as unverified. I/O failures are errors, never matches.

## Write lifecycle

Select eligible left-side regular files → create immutable backend plan → review plan in UI → consume plan ID once → revalidate roots and hashes → write temporary sibling → verify copied digest → verify old-file backup → recheck target → replace → verify result → update journal.

Preview creates no destination file. Execution may create destination parent directories. Destination-only files and excluded files are preserved. A failed action stops subsequent actions; the journal shows completed, failed and pending items. A cancellation cannot undo actions already completed.

## Resource limits

- A maximum of 200,000 scanned entries per root in the preview release.
- At most 500 directory entries in an IPC page.
- File hashes and copy operations use 64 KiB buffers.
- Text preview accepts UTF-8 up to 2 MiB and 30,000 lines per file; unsupported files return an explicit error.
- The UI initially renders 500 text diff lines and can show more on request.
- Only one scan/plan/write task runs at a time. Native work is dispatched off the UI thread.

## Persistence

The current comparison settings are kept under Tauri's application data directory. Backups are grouped by a unique plan ID, with `files/` and `journal.json`. No telemetry is sent. Backend paths and journals must be redacted before public bug reports.

## Remaining work

Restore automation, metadata preservation, durable directory fsync, large-scale indexed scanning, session libraries, broader encodings, advanced merging, remote adapters and converters remain on the roadmap. The initial architecture leaves these separate from the UI and desktop runtime.

## Specialized workspace panels

Leptos editing, merge, table and binary panels are separate modules in `crates/ui/src/panels`. Each consumes typed signals and operation callbacks. Native paths and file mutation remain in `clear-core` and explicitly authorized Tauri commands.

Directional synchronization resolves source/target roots from the immutable plan's `source_side`. It does not copy or reverse the entire directory snapshot. Changing direction clears the UI selection and requires a new preview. Backups always belong to the destination being replaced.

## Copy recovery

Sync journals include the destination root and relative entry paths in native byte/wide-unit encoding. UI display labels never reconstruct filesystem inputs. Recovery rejects paths from a different operating system, symlink traversals, mismatched journal placement, modified current files and invalid original backups.

Recovery previews own a native plan with pinned expected digests. Tauri resolves history IDs under its private backup directory and consumes exact cached preview tokens. CLI preview is read-only; `--apply` generates a fresh plan. Recovery streams contents to verified current-file backups before replacement or removal and persists action states in a separate journal. The original copy journal and backups remain intact.

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

## Git committed-object adapter

The optional Git adapter invokes installed Git directly with typed argument arrays. Revisions are pinned with [`rev-parse --verify --end-of-options`](https://git-scm.com/docs/git-rev-parse); [`ls-tree -r -t -z -l --full-tree`](https://git-scm.com/docs/git-ls-tree) supplies raw tree names, modes, full object IDs and sizes. Filename bytes are preserved on Unix; names unsupported on the host fail explicitly. Text previews read pinned blobs using [`cat-file blob`](https://git-scm.com/docs/git-cat-file), without checkout, textconv, smudge filters or application filesystem writes. Git object identity, rather than timestamps or a new SHA-256 read, determines file equality.

The command runner removes inherited Git redirection/config/trace environment, disables optional locks and replacement objects, overrides fsmonitor, disables prompts and all transport protocols, and passes [`--no-lazy-fetch`](https://git-scm.com/docs/git). Unsupported Git versions fail instead of falling back to fetching. Pipe readers cap stdout/stderr independently while a worker polls cancellation and a 15-second deadline; failure kills/reaps the child and joins readers. Tree output and path counts are bounded; preview blobs remain limited to 4 MiB. Submodules are explicit errors, symlinks stay unverified, LFS pointers are not dereferenced, and there is no working-tree/index/write or recovery capability.
