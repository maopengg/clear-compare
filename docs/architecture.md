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
