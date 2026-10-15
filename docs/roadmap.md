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
