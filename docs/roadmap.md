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
