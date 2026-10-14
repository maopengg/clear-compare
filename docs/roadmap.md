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
