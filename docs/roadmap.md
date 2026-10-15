# Roadmap and evidence

The complete scope is in [development-plan.md](development-plan.md). This log tracks actual progress, not projected feature completion.

## P0 and P1 initial implementation

- Rust workspace separates contracts, native core, CLI, Leptos CSR interface and Tauri integration.
- MIT license, contribution and security documents, feature matrix and CI/release workflows created.
- Rust/WASM toolchain and Trunk prepared.
- Folder comparison and explicit exclusions implemented.
- UTF-8 text preview, file selection, copy preview and verified-backup copy implemented.
- Initial tests cover nested exclusions, target-only preservation, verified backups, external modification rejection, overlapping roots, cancellation and symlink escape rejection.

## Verified on macOS Apple silicon

- Eight focused tests passed across rules, comparison, text preview and synchronization.
- Core, CLI and desktop passed Clippy with warnings treated as errors.
- Leptos WASM release build and native desktop debug package built.
- Packaged app opened and completed synthetic directory comparison, UTF-8 diff preview, two-file reviewed copy and journal persistence.
- Filesystem checks confirmed overwrite backup contents, excluded-file preservation and destination-only preservation.
- A startup CSP defect was identified in the real packaged app, corrected and retested.

These results validate the preview workflows only; they do not prove the full plan or Windows/Linux runtime behavior.

## Validation still required before a release claim

- Release-mode packages and three-platform build verification.
- Further UI workflows, keyboard/permission checks and failure recovery.
- GitHub CI results.
- Windows/Linux package builds and runtime acceptance.
- Performance baselines and full P0 feature decomposition.

## Validation sequence

Per project direction, further packaged-client runtime and installation tests are deferred until the planned feature implementation is complete. Continue focused core tests, WASM compilation and static checks during development. Final Windows/macOS/Linux installation, upgrade and runtime acceptance remain mandatory release gates.

The optimized macOS application and DMG were built and exercised on synthetic files before this change in sequence. A portable backup-flush fix subsequently passed core/WASM CI on macOS, Windows and Linux (run 38549416382). Existing local preview artifacts are development outputs; no release tag has been published.

## Remaining long-term milestones

P2: editing, encoding and large-directory performance. P3: durable synchronization policies, recovery and automation. P4: advanced merge and specialized viewers. P5: archives and remote adapters. P6: cloud, VCS and system integrations. P7: full platform release acceptance.

## P2 text workflows and initial specialized viewer

- UTF-8 and BOM-marked UTF-16 LE/BE editing retains BOM and uniform line-ending style.
- Saves verify the current digest, create a verified backup and persist a prepared/completed manifest, then atomically replace and verify the result.
- Display-only case, whitespace and newline normalization preserves original text on both sides.
- Three-way merging combines independent changes and exposes competing changes with base/left/right markers. Results remain buffers until explicitly saved through the editor.
- Hex/ASCII comparison reads fixed 4 KiB pages, independently of file size.
- Focused tests cover encoding retention, external edits, malformed inputs, normalization, final newline significance, adjacent edits, deletion, conflicting insertions and binary page boundaries.
- Core/CLI tests and compiler/static checks continue; the new desktop interaction and installation acceptance are deferred per the agreed validation sequence.

## CSV/TSV comparison

- Strict quoting retains quoted commas, escaped quotes, field newlines and empty cells.
- Optional unique keys align rows independently of order; duplicate keys and malformed records are explicit errors.
- Optional finite nonnegative absolute numeric tolerance is compared using floating-point values. Columns remain positional; differing headers are visible.
- Native table state retains the full bounded report while IPC/UI pages contain at most 500 rows.
- Parser, keyed comparison, missing rows, duplicate keys and invalid-tolerance tests pass locally.
- Windows CI exposed an editor-save access error; the backup source handle is now closed before replacement and error stages are labeled. Three-platform CI must confirm the correction.

## Directional copy and UI module boundaries

- Left-to-right and right-to-left copy use the same digest/backup/journal checks. Reverse-copy tests verify right-only additions and backup of the replaced left-side content.
- Editing, merge, table and binary views are separated into dedicated modules. Table refresh keys include their native report ID; binary reloads remount the preview so stale keyed rows are not reused.
- Three-platform core/WASM CI run 38552812481 passed after the editor handle correction. These checks are distinct from postponed packaged-client installation acceptance.

## Save format controls and reviewed editor restoration

- Save options explicitly convert UTF-8 / UTF-16 LE/BE, BOM and LF/CRLF/CR style. Mixed endings require an explicit choice; BOM-less UTF-16 output is rejected.
- A restore preview pins verified original bytes and the current editor digest. Apply revalidates the current file and creates a backup of its current contents before replacing it. Restoration retains the original bytes, including mixed line endings.
- Desktop restoration consumes a native cached preview token and only accesses the editor's native last-save backup. CLI `restore-editor` can reopen a matching completed manifest across process restarts; preview is the default, `--apply` writes.
- New tests cover format conversion, byte-exact restoration, reopen, restoration backup contents, tampered backup rejection and external edits after preview.
- Three-platform core/WASM CI run 38553600582 passed for the preceding directional-copy and panel changes. Final packaged-client acceptance remains deferred.

## Journaled synchronization recovery

- New copy plans persist lossless native destination/entry paths. Backup storage requires Unicode paths; native entry names round-trip without using display labels.
- Recovery reopens a persisted journal after snapshot/process lifetime, reconciles completed/running/failed copies and previews restore/removal actions. Current content must match the copied digest; original overwrite backups must match their original digest.
- Each recovery action backs up current content before replacement/removal and writes a separate state journal; errors stop later actions. Empty directories created during copy are retained.
- Desktop operation history and CLI `recover-sync` expose reviewed recovery. Older journals lacking native path metadata are rejected explicitly. Recovery redo and automatic crash resume remain planned.
- Tests cover overwrite/create rollback, reverse direction, running-state reconciliation, modified targets, changed backups, native-path round trips and process-level preview/apply behavior. Linux additionally verifies actual non-Unicode filenames; macOS filesystems reject the synthetic invalid UTF-8 filename, so its encoding round trip is tested without creating that filename.
- Preceding save-format/restoration changes passed three-platform core/WASM CI run 38554500982. New packaged-client workflows are compiled but runtime/installation acceptance stays deferred.

## Directory aggregation, navigation and global queries

- Directory status reflects differing/erroneous/unverified scanned descendants; byte totals sum listed regular files under the active rules. A directory with a changed empty child is visibly changed even when file counters remain zero.
- Scope-wide search, difference filtering and stable path/status/numeric-size sorting happen before pagination. Directory IDs and native path indexes provide child browsing and breadcrumbs.
- The UI distinguishes overall comparison counters from the current directory/query scope. Search can apply by button or Enter, and page totals represent the filtered result.
- Tests find a difference after more than 500 equal rows, verify directory aggregate counts/bytes, native breadcrumb scope, unverified descendants and numeric size ordering. CLI now exits1 for empty-directory-only differences.
- Previous copy-recovery changes passed three-platform core/WASM CI run 38555848042. Final packaged-client installation and runtime acceptance remain deferred.

## Named session library

- Named configurations persist comparison paths/rules, language and view/text options; opening loads configuration without starting filesystem work. Directory scope resets to root because snapshot IDs are ephemeral.
- Versioned session files are atomically persisted with revision checks and a nonblocking storage lock for cooperating processes. Archiving retains files under `sessions/trash`.
- Last-session persistence now uses bounded regular-file reads and atomic replacement. Legacy settings default the added view fields.
- Tests cover reopen/update/archive, stale revisions, legacy defaults, invalid IDs, symlink rejection and lock contention. Previous directory-query changes passed three-platform CI run 38557214016.

## Picture comparison

- Native PNG/JPEG/BMP/WebP decoding uses input/dimension/pixel limits and EXIF orientation. Pixels compare as RGBA8 at top-left alignment, with explicit channel tolerance and alpha-channel policy.
- Counts, maximum channel delta and changed bounds come from the full canvas. Aligned thumbnails, opacity overlay and red changed-pixel buckets provide bounded GUI visualization; CLI returns statistics and equality/difference/error exit codes.
- Tests cover exact tolerance boundaries, alpha policy, missing dimensions, one-pixel differences surviving thumbnail reduction, oversized canvas rejection, malformed data, generated JPEG EXIF orientation and BMP CLI behavior.
- Resolved dependency declarations and packaged license notices were refreshed for the codecs and other resolved packages. Prior session-library changes passed three-platform CI run 38558364667.

## Baseline-aware folder merge

- Two-way union and three-way baseline classification handle independent changes, matching changes, directory composition, competing additions, deletion/modify and probable unique-content rename/modify conflicts.
- Conflict-free UTF-8/UTF-16 text changes auto-merge with left-side encoding/BOM/newline preservation; other cases require explicit version choice or omission. Directory choices apply to entire subtrees.
- GUI previews are paged and consume native cached plan IDs; CLI preview is read-only, supports explicit relative-path choices, and writes only with `--apply`. Every remaining conflict blocks output.
- Output must be an independent empty directory. Execution revalidates all inputs, creates files without clobbering external files, keeps manifests/generated text/journals outside output, and verifies the resulting tree.
- Tests verify independent text edits and additions, source preservation, deleted-directory/edited-child conflicts, subtree resolution, probable rename/modify conflicts, changed baseline rejection, nonempty-output protection, two-way competing additions and CLI preview/apply.
- Previous picture changes passed three-platform CI run 38560311740. New desktop interactions are compiled; packaged-client acceptance remains deferred.

## Editor text history and generated-conflict save gate

- Shared UTF-8 splice history supports bounded undo/redo and branch editing without full-document snapshots per keystroke. Toolbar and keyboard controls include input-method composition boundaries. LF editing buffers are compared against a normalized saved baseline while original file newline format is retained by native encoding.
- Merge conflict markers carry unique tags. Native staging registers editor provenance; save rejects remaining tagged markers or introduced conflict labels before filesystem writes. Provenance transfers when saves rotate document IDs, protecting undo across save points. Existing tagged working copies are recognized on open.
- Text file bytes remain bounded at 4 MiB; decoded text can use 8 MiB so UTF-16 expansion does not artificially block valid saves.
- Tests cover Unicode byte boundaries, undo/redo branching, history pruning, tag/separator discrimination and blocked saves before/after a resolved save. Previous folder-merge changes passed three-platform CI run 38570516680.
- The original projectless temporary directory was removed externally; development logs now reside under ignored `target/work`. Packaged-client interaction/installation acceptance is still deferred.

## Read-only archive comparison

- Classic ZIP Stored/Deflate trees can be browsed singly or compared with ZIP/local directory trees, including implicit directories, shared exclusions, descendant aggregation, paged search and UTF-8/UTF-16 member text previews.
- Central-directory preflight, container fingerprints, decoded size/count/ratio budgets, duplicate/unsafe-path rejection and CRC failures prevent malformed or unverified data from appearing equal. No extraction or source writes occur. ZIP64, multidisk, encrypted and unsupported compression formats are not supported.
- Tests cover ZIP/directory equality and changes, exclusions, metadata-only uncertainty, rewritten containers, traversal names, duplicates, expansion limits, cancellation, CRC corruption and CLI exit codes. Local tests and both native/WASM Clippy checks passed; builds are recorded with the commit.
- Previous editor-history changes passed three-platform CI run 38670528131. Packaged-client interaction/install acceptance remains deferred until all planned features are implemented.

## Content-free directory snapshots

- CLI and desktop export versioned `.cctree` inventories into new files outside their source trees. Inventories capture exclusions, names/types/sizes/times/errors without file bodies or digests. Export never replaces existing files; cancellation cleans temporary output.
- Browsing, paged search, snapshot/snapshot and snapshot/current-directory comparison preserve metadata uncertainty. Snapshot sources cannot be edited, synchronized or restored. Portable Unicode components and same-OS native names remain distinct from display labels.
- Tests cover removed original directories, empty/excluded directories, metadata changes, missing entries, refusal of content verification, no-clobber/source containment, cancellation, invalid schemas, forged digest fields, traversal/duplicates/missing ancestors, native-name collisions, symlink non-following and CLI exit codes.
- Archive changes passed three-platform CI run 38672279805.
- macOS does not create arbitrary invalid UTF-8 filenames on this volume; the reader test uses a synthetic native-name inventory to check lossless identity independently of host filesystem restrictions. GUI/installer acceptance remains deferred.

## Read-only Git revision comparison

- Local commits, branches and tags resolve to immutable full IDs; trees compare by object identity and executable mode, with shared exclusions, directory aggregation, paged search, bounded blob text diff and CLI exit codes. No checkout, index/worktree writes, lazy fetch, textconv or LFS resolution occurs.
- Git subprocesses use argument arrays, isolated Git environment, disabled transports/prompts/fsmonitor, bounded stdout/stderr, cancellation and a 15-second deadline. Git must support `--no-lazy-fetch`; the adapter fails explicitly otherwise.
- Real temporary-repository tests verify commit equality/changes/additions, dirty worktree/index preservation, branch movement after comparison, excluded subtrees, invalid revisions, filenames containing spaces/tabs, oversize text, missing objects without fetching, symlinks, executable-mode changes and CLI return codes. Client GUI/installer acceptance remains deferred.
- Tree-snapshot changes passed three-platform CI run 38672977224. Git working-tree/index comparison, difftool/mergetool registration and SVN integration remain planned.

## Structured merge conflict controls

- Merge buffers now support stable conflict groups, previous/next/current navigation, original left/base/right selection, exact left-then-right concatenation, bounded undo/redo and keyboard controls with composition handling.
- Shared parsing rejects malformed, duplicated, nested or incomplete markers. Resolution rejects manually edited blocks, preserves exact original line endings/final-newline absence and does not discard surrounding text. DOM selection uses UTF-16 offsets.
- Tests cover all version choices, Unicode offsets, shifted multi-group positions, malformed markers, manual-edit protection, undo restoring guarded tags, original no-final-newline fragments and oversize generated output. The existing native save/undo regression also exercises the new marker form.
- Previous Git changes passed three-platform CI run 38673619729. Local automated checks and WASM/native builds are recorded with this commit; client GUI/installer acceptance stays deferred.

## Intraline text differences and navigation

- Text comparison now returns original-byte intraline spans for ordinal pairs in replacement groups, with explicit per-line/result budget fallbacks. The equal/delete/insert sequence and all original bytes remain available to the CLI and merge inputs.
- A separate shared-scroll text panel provides 500-row aligned pages, change-group navigation (F7 / Shift+F7 while focused), case-sensitive literal row search across all results and visible original EOL markers. Long display lines are capped at 8192 characters with a truncation label.
- Tests cover emoji/multibyte ranges, whitespace/case/EOL rules, contextual lowercase and expansions, CRLF-only changes, budget fallback, original-text reconstruction, cross-page alignment/search, invalid/overlapping spans and Unicode-safe clipping.
- Structured-merge changes passed three-platform CI run 38674396092. New local tests/lints/builds are recorded with this commit; interactive client/installer validation remains deferred.

## Full comparison reports

- Native/CLI export of HTML and versioned JSON covers complete local, ZIP/directory, tree-snapshot and committed Git results, independent of GUI scope/search/page. An optional differences filter retains unverified/error/type-conflict rows without rewriting the original summary.
- Frozen policies record exclusion input and SHA-256, size/time, size-only or Git-object verification basis. HTML escapes paths, warnings, errors and metadata; no source text, executable script or external resource is embedded.
- Export is bounded, cancellable and no-clobber, with temporary-file cleanup and source/repository path protection. Git roots include linked-worktree common storage and actual top-level roots discovered from subdirectories.
- Tests cover HTML injection payloads, JSON policy/schema/summary, rows beyond 500, changed source files after comparison, uncertainty/error filtering, overwrite/source containment, symlink aliases, stale sessions, cancellation and byte budgets. CLI tests verify saved results and exit codes, including committed Git snapshots with dirty working copies.
- Text-navigation changes passed three-platform CI run 38675843060. New local tests/lints/builds are recorded with this commit; GUI/installer verification remains deferred.

## Static HTML table support and native table documents

- HTML/HTM tables can be listed and independently selected on both sides, including comparisons with CSV/TSV. Extension hints and explicit format overrides are separate; HTML5 repair handles common omitted endings/entities, with defined static text/merged-cell policies.
- Native table documents capture regular-file text once, reject changed verified inputs and support repeatable rule/format changes without rereading sources. Data is not sent to the UI as raw HTML. Document/result IDs expire on replacement/close/new folder comparison.
- Resource checks cover decoded bytes, DOM nodes/depth/time, table count, expanded rows/columns/cells/text, with cancellation checks. Ambiguous overlap and invalid span values are errors. UI adds column windows and explicit long-cell clipping.
- Tests cover entities, whitespace/NBSP, inert/hidden/foreign-script content, nested tables, row-group ordering, zero/positive row spans, column spans, overlaps, limits, missing sides, UTF-16, HTML/CSV/TSV format selection, immutable input reuse and CLI catalogs/exit codes. Dependency declarations and third-party notices were refreshed.
- Comparison-report changes passed three-platform CI run 38677696889. New local automated checks/builds are recorded with this commit; client GUI/installation acceptance stays deferred.

## Byte-level binary navigation and streaming search

- Paired 4 KiB pages expose exact differing byte indices, explicit side availability and a fresh rendering identity. Desktop decimal/hex navigation and selected-match highlights retain 64-bit precision through string offsets.
- Hex, UTF-8 and explicit UTF-16 LE/BE byte patterns support overlapping matches and KMP streaming across read/page boundaries. Each call covers at most 64 MiB candidate starts and 256 matches; explicit continuation preserves overlapping matches and scan-boundary lookahead.
- Native reads reject nonregular/link inputs and observed metadata changes; these are bounded live reads, not immutable content snapshots. UI range labels and CLI next_offset distinguish an empty batch from a completed search. CLI hex exit 0 means a successful page read, not whole-file equality.
- Tests cover exhaustive small binary inputs against a naive matcher with multiple scan windows/result limits, read-block boundaries, premature EOF, cancellation, large offsets, byte differences, stale metadata, missing sides, links, encodings and process-level CLI semantics.
- Previous static HTML table changes passed three-platform core/WASM CI run 38681551239. Packaged-client interaction and installation acceptance remains deferred.

## Presentation themes and offline workflow help

- Added per-client System/Light/Dark themes using shared semantic colors. System responds through prefers-color-scheme; theme choice uses a bounded versioned WebView storage record with explicit failure feedback. Presentation records contain no paths or compared contents.
- Added searchable Chinese/English offline help covering existing folder/rule, copy/recovery, text/editor/merge, table, binary, picture, archive/snapshot/Git, session/report and keyboard workflows. Help identifies unsupported functions and the preview status.
- Top-bar actions wrap instead of assuming one fixed-height row. Native semantic details/summary controls keep help topics keyboard-operable without a modal focus trap. Primary hover styling now uses semantic colors in both themes.
- Calculated dark-theme foreground/background contrast exceeds 4.5:1 for normal/muted text, selected rows, primary buttons and green/amber/red status labels. This calculation is not a formal accessibility or WebView runtime acceptance claim; packaged-client visual/interaction acceptance stays deferred.
- Preceding binary changes passed three-platform core/WASM CI run 38682764125.

## Named table-column alignment

- Added optional exact header-name alignment across CSV/TSV/static HTML inputs, preserving original row/header arrays and exposing a logical-to-source map. Logical order follows left headers then right-only names, bounded to 1024 columns.
- Row keys map independently to each source column. Ambiguous/blank headers, rows wider than their headers, invalid logical keys and absent keys on nonempty sides reject the comparison. Existing positional behavior stays the default.
- Reordered identical name sets compare equal under this option; schema additions/removals remain differences, including header-only tables. Missing columns, short-row missing cells and explicit empty strings remain distinct. UI rows/headers share mapped ordering and column paging; CLI exposes the option and schema flag.
- Focused cases cover reordered/keyed numeric rows, exact-name sensitivity, missing/empty distinctions, malformed schemas, logical limits, mixed HTML/CSV and CLI exit codes. Core/contracts/CLI tests pass locally; UI and native validation are tracked separately from postponed runtime/install acceptance.
- Previous themes/offline-help changes passed three-platform core/WASM CI run 38683146761.

## Captured Git index sources

- Each Git comparison side can select a commit or captured index. Default requests retain commit behavior. Native formatted ls-files output provides modes, object IDs/sizes and stages; previews reuse pinned blobs without reading working-tree paths. Both index sides share one listing.
- Unmerged stage groups produce errors, intent-to-add uses indexed empty blobs with explicit warnings, and index display fingerprints identify the listing rather than index-file bytes. Root-scoped exclusions and synthesized directory aggregation use existing contracts.
- CLI source flags support comparison and HTML/JSON reports. Subdirectory selectors use the complete worktree index; linked worktrees use their own index. Bare indexes reject explicitly. No write-tree, staging, checkout, network, text conversion or source-file writes are introduced.
- Focused tests cover staged-versus-working bytes, frozen previews after later staging, additions/removals, executable modes, exclusions, intent-to-add, unmerged stages, empty/unborn repositories, split/sparse indexes, linked worktrees, SHA-256 object IDs, submodules and CLI/report exit codes. Final client interaction/install acceptance remains deferred.
- Preceding named table-column changes passed three-platform core/WASM CI run 38683576053.

## Raw Git working-tree comparison

- Added a third typed Git source for physical working files, with optional nonignored untracked inclusion. Tracked scope comes from index names/modes without requiring index blobs; source combinations include commit/index/worktree on either side and fresh/unborn worktrees.
- Native streaming computes raw Git SHA-1/SHA-256 blob identity and an independent content SHA-256. Metadata checks surround reads; previews verify the captured content even after same-size/time-preserving edits. No clean/smudge/EOL filters, object writes, index updates, checkout or network operations are introduced.
- Physical missing paths remain absent under sparse checkout; executable modes honor core.filemode with indexed fallback where native bits are unavailable. Links/ancestors are not followed; actual parent types, read errors, unsupported kinds and embedded repositories are explicit.
- Tests exercise tracked/untracked/ignored scope, deletions, raw CRLF/filter behavior, modified previews, both object formats, large streamed files, type obstructions, links/FIFOs, sparse and linked worktrees, report containment and CLI/source-flag contracts. New sha1 dependency is pinned with packaged notices refreshed.
- Prior captured-index changes passed three-platform core/WASM CI run 38684273075. Current packaged-client interaction and installation acceptance remains deferred.

## Reviewed regular-file deletion and journal compatibility

- Added explicit side-specific selected-file deletion, verified byte/basic-permission backups before unlink, cancellation, stop-on-error and operation history/recovery. Recreated paths are never overwritten by deletion rollback; absent subdirectories can be recreated.
- Version-two file plans tag policies/action kinds, retain lossless paths, show actual targets and validate consistency/32 MiB pending-journal bounds before execution. Version-one copy journals retain the old overwrite-field alias and digest semantics; recovery displays derive from native paths.
- Shared copy/deletion plan review is a separate Leptos panel with bounded initial action rendering. Operation switches clear selection, deletion mode is not session-persisted, and failed native previews invalidate old cached plans. CLI deletion requires an explicit side and literal paths; preview is read-only.
- Journal/backup/mutation paths add Unix directory flushes. Permission/byte checks, no-clobber restoration and interrupted-state reconciliation remain explicit; full metadata, directory deletion, mirror policies, automatic resume and OS trash remain planned.
- Tests cover either side, unchanged/target-only/empty files, exclusions, stale inputs, cancellation/partial completion, re-created paths, tampered backups/permissions, interrupted states, legacy copy journals, label projection, native names, policy/budget failures and CLI preview/apply/recovery. Packaged-client runtime/install acceptance stays deferred.
- Previous raw Git working-tree changes passed three-platform core/WASM CI run 38685653307.

## Reviewed regular-file rename and editor location tracking

- Added desktop and CLI previews for a single same-directory regular-file rename on either side, with explicit new names, verified original backups and version-three native-path plans. Existing destinations, links, excluded names and forged staging paths are rejected.
- Uses native no-replace moves through a journal-specific temporary name, including case-only changes. Normal rename retains the original file's identity/timestamps. Recovery verifies the original backup and current destination/staged file, backs up current bytes and renames back without overwriting a recreated original name. Staged recovery preserves an unrelated destination file.
- Open editors follow completed rename/rollback locations without replacing drafts, undo history or saved baselines. Native last-save restoration remains bound after app-controlled rename. Copy/deletion/recovery now invalidate affected clean editor views while retaining dirty buffers; native exact-name checks prevent old-case editor saves from reversing external renames.
- Unicode canonical-spelling checks require case-sensitive NFC equivalence plus actual filesystem identity when raw names differ. Native paths are never normalized for writes and normalization-sensitive filesystems retain distinct names. The resolved Unicode normalization dependency and packaged notices were added.
- Focused tests cover both sides, nested/Unicode names, case-only round trips, native identity/timestamp/basic permissions, collisions/hard links, cancellation/stale inputs, changed backups, staged/interrupted rollback, foreign destination preservation, editor relocation and actual last-save restoration. Linux adds non-Unicode native filenames. Local core/contracts/CLI tests, strict native/WASM Clippy, WASM output and desktop debug build passed. Earlier deletion changes passed three-platform CI run 38687969206; rename CI is checked separately after push.
- Cross-directory moves, bulk/directory renames and automatic resume remain planned. Packaged-client interaction and installation acceptance remain deferred until planned feature development is complete.

## Whole-comparison regular-file mirror

- Added explicit left-to-right/right-to-left file-mirror modes and CLI `mirror-files --source`. The native plan includes every changed/source-only regular-file copy and target-only backed-up deletion across the captured roots, independent of UI folder/search/paging/selection. The preview shows actual source/destination roots, action counts and each operation; mirror mode is not session-persisted.
- Requires SHA-256 comparison and rejects included links, special files, errors and type conflicts. Preview and execution re-scan both complete included inventories, including initially equal files; stale contents, new names and missing entries invalidate the plan before journal creation. Excluded changes remain outside scope.
- Version-four mixed copy/deletion plans use the existing verified-backup, per-action journal, cancellation and stop-on-error machinery. Copies precede deletions. Deletion checks that no source counterpart has appeared before/after backup. The native executor rejects omitted/mistyped scope actions. Existing history recovery can reverse mixed actions with current-file backups and no overwrite of recreated deleted paths.
- Tests cover both directions, changes/additions/deletions, excluded files/subtrees, retained empty directories, mixed rollback, stale equal files, new entries, recreated source counterparts, malformed scopes, metadata-only/type/link refusal, cancellation and CLI preview/apply/recovery. Core/contracts/CLI tests, legacy copy/deletion/recovery/rename compatibility checks, strict native/WASM Clippy, WASM output and desktop debug builds passed locally. Previous rename changes passed three-platform CI run 38691286759.
- Empty-directory replication/removal and exact directory-tree mirror, baseline-aware bidirectional synchronization, pause/resume and recovery redo remain planned. Live scans are not locked snapshots and multi-file execution is not a transaction. Packaged-client runtime/installation tests remain deferred.

## Directory-tree mirror and identity-checked directory recovery

- Added explicit whole-comparison tree-mirror modes and CLI `mirror-tree --source`. Version-five plans include parent-first directory creation (including empty directories), existing verified copy/deletion actions, and child-first removal of emptied surplus directories. The native executor validates the complete dependency order before any journal/output creation.
- Direct-child preflight retains directories needed by excluded/unplanned descendants and lists those retained ancestors in preview notes. New excluded children that change the removal layout expire the old plan. Native directory removals pin filesystem identity and basic permissions, verify flushed metadata records and use only nonrecursive empty-directory removal. Directory-creation aliases on insensitive filesystems are refused before execution.
- Successful mkdir records native identity/permissions in the private backup set; compared trees receive no marker files. Unix records use device/inode/optional creation time and Windows uses winapi-util volume/file-index/creation-time information. Decimal strings preserve identity values across IPC. Flat native-path hashes avoid metadata-name collisions. Missing ownership records and unsupported/replaced identities block automatic cleanup.
- Version-five recovery reverses dependency order, recreating removed parents before files and cleaning created directories last. Original records, recreated paths, created identities and unexpected children are checked during preview, again before recovery starts and before directory mutation. Current directory metadata is backed up in the recovery set; removed basic permissions are restored. Earlier file/rename journals retain their original order and compatibility.
- Local tests cover both directions, nested/empty directories, full file-content/tree equality after mirror, dependency-ordered rollback, exclusions and retained ancestors, stale layouts, original/created directory replacement, external children before/after preview, malformed order/identity records, missing ownership, running-state reconciliation, permission restoration, cancellation, case-sensitive/insensitive aliases and CLI preview/apply/recovery. Linux additionally exercises non-Unicode directory names through identity records. The full core/contracts/CLI run passed 195 local cases; targeted compatibility and strict native/WASM Clippy plus WASM/desktop debug builds passed after final checks. Previous file-mirror work passed three-platform CI run 38692155429.
- Updated README, architecture, help, feature matrix and detailed tree-mirror contracts. The direct Windows identity helper dependency was already resolved/licensed; metadata/notices were checked. Type replacement, standalone directory operations, full ownership/ACL/time replication, baseline sync and partial-recovery resume remain planned. Packaged-client interaction/installation acceptance remains deferred.

## Background operation handles, cooperative pause and progress

- File application and rollback now return native task handles immediately. An Arc-owned desktop backend keeps one bounded operation/result slot, validates exact task IDs, supports once-only terminal results and holds write admission until result collection. The UI polls small snapshots, reconnects to same-process uncollected work at startup, and exposes explicit reconnect and task-specific cancellation controls.
- Added core synchronous-worker task scopes with pause/continue/cancellation, a condition-variable pause acknowledgment, current path/stage, verified-action counts and saturating tracked file-stream byte counters. Pause-requested and paused are distinct. Scoped bindings are restored after errors/panics and do not propagate implicitly to spawned threads; independent noncancellable verification tokens never pause.
- Native mutation guards revalidate paths, source/target/prepared-file/backup digests and permissions after a possible pause, then defer further pause through writes and mandatory verification. A generation counter repeats interrupted complete-scope preflight before journal creation. Directory operations and all recovery kinds use the same guarded transition. Cancellation still interrupts pre-commit checks; post-commit verification remains mandatory.
- Cancelled action states are now recorded for in-action cancellation and included in recovery reconciliation, including a staged rename. Existing plain CLI cancellation/journal behavior is preserved. Task-level outcomes retain the existing editor location/staleness updates; failed or disconnected monitoring invalidates stale comparison views without clearing unsaved editor buffers.
- Full core/contracts/CLI tests passed locally, followed by focused controller/registry tests. Six concurrency/safety tests cover pause acknowledgment/resume, paused cancellation wakeup, deferred pause during mutation, cancellation within mutation, post-pause preflight expiry, token isolation and scope restoration after panic. A native backend unit test covers busy admission until collection, foreign IDs, once-only results and task expiration without launching a client. Strict native/WASM Clippy, WASM output and desktop debug builds passed. Earlier tree-mirror changes passed three-platform CI run 38694371967.
- Updated README, help, architecture, feature matrix and operation-task contracts. Same-process reconnection is implemented; native restart/forward crash resume, partial-recovery resume/redo and baseline-aware bidirectional sync remain planned. Packaged-client interaction and installation acceptance remain deferred.


## Reviewed forward restart after native process exit

- New sync operations persist original native roots, selected/full scope, policy, identity/basic-permission pins and immutable plan context; the journal records its SHA-256. Existing legacy rollback remains available without inventing missing forward evidence.
- Added exclusive operation-set locks, exact preview revalidation, reconciliation of committed-but-unconfirmed actions, staged native rename continuation, and independent retry backup namespaces. Original history retains all action dependencies and can roll back after retries.
- Copy work files are registered by native identity before data writes. A reviewed restart retains verified partial bytes before removing only owned temporary files. Identical-byte foreign replacements and unregistered lookalikes are rejected/preserved.
- Added CLI `resume-sync`, three validated desktop commands, cached opaque preview IDs and bilingual paged history review integrated with live task progress/pause/cancel. Updated current feature matrix and detailed restart contract. Backend/core tests include an actual killed child process and rollback through the original history; packaged-client runtime/install acceptance is still deferred.
- Baseline-aware bidirectional policies, recovery resume/redo, folder-merge resume, full metadata and remaining plan modules are still required for final completion. Historical entries above describe scope at their original milestones; the feature matrix is current.

## Excel OOXML worksheet comparison

- Added pinned calamine/quick-xml dependencies, package/XML allocation preflight and streaming cached-value extraction. No dense range is allocated from untrusted dimensions; raw/expanded bytes, coordinates, XML hints and text are bounded.
- Native documents freeze bytes with folder digest pins. Worksheet catalogs expose names, zero-based selection and hidden status; row/column alignment, keys, tolerance and paging reuse the table engine.
- Added desktop selectors/catalogs, CLI --left-sheet/--right-sheet, bilingual help and documented value/date/formula-cache policies. Missing caches, cell errors and malformed/oversized inputs reject comparison. Formula evaluation, display formatting, legacy formats and editing remain planned.
- Tests cover strings, boolean/sparse grids, shared formula caches, 1900/1904 dates, cross-format keys/tolerance, source freezing/digest expiry, CLI and resource/error boundaries. Runtime/install acceptance remains deferred.
