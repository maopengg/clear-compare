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
