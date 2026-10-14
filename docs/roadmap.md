# Roadmap and evidence

The complete scope is in [development-plan.md](development-plan.md). This log tracks actual progress, not projected feature completion.

## P0 and P1 initial implementation

- Rust workspace separates contracts, native core, CLI, Leptos CSR interface and Tauri integration.
- MIT license, contribution and security documents, feature matrix and CI/release workflows created.
- Rust/WASM toolchain and Trunk prepared.
- Folder comparison and explicit exclusions implemented.
- UTF-8 text preview, file selection, copy preview and verified-backup copy implemented.
- Initial tests cover nested exclusions, target-only preservation, verified backups, external modification rejection, overlapping roots, cancellation and symlink escape rejection.

## Validation still required before a release claim

- WASM and native desktop compilation.
- Packaged app launch and end-to-end synthetic directory comparison/copy.
- GitHub push and CI results.
- Windows/Linux package builds and runtime acceptance.
- Performance baselines and full P0 feature decomposition.

## Remaining long-term milestones

P2: editing, encoding and large-directory performance. P3: durable synchronization policies, recovery and automation. P4: advanced merge and specialized viewers. P5: archives and remote adapters. P6: cloud, VCS and system integrations. P7: full platform release acceptance.
