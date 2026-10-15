# Reviewed directory-tree mirror

Tree mirror extends [regular-file mirror](file-mirror.md) to include missing and surplus directories. It operates on the complete content-verified local comparison under its captured exclusions, independently of the displayed folder/search/page/selection. Both left-to-right and right-to-left directions are explicit.

## Workflow and scope

Choose **Tree mirror: left → right** or **Tree mirror: right → left**, preview, review the actual source/destination roots and all four operation counts, then execute the exact native plan ID. The dialog lists directory actions individually and identifies retained directories. Mirror mode is not saved in named sessions. History provides a separate reviewed rollback.

The supported tree consists of readable regular files and real directories. Missing directories, including empty ones, are created parent-first. New/changed files are copied with the existing digest/backup checks, target-only files are backed up and deleted, then surplus directories are removed child-first **only after they are empty**. There is no recursive `remove_dir_all` call on comparison inputs. The source tree remains untouched.

Excluded entries remain outside the operation. A target-only directory containing an excluded child, or a retained descendant directory, remains in place with a preview notice. Its other included children can still be mirrored. A newly appeared excluded child can change the removal layout and expire the old plan before any write; another arrival during execution causes the final empty-directory removal to fail, preserving it. Excluded-file ancestors may therefore remain visible as destination-only directory differences after mirror.

Links, special files, unreadable entries and file/directory type conflicts block the preview. Existing names that alias a proposed directory creation on an insensitive filesystem are refused rather than renamed implicitly. Metadata-complete replication, type replacement and directory rename/move remain planned. Common-baseline bidirectional reconciliation is available through its [separate workflow](bidirectional-sync.md).

## Identity, backup and journaling

Version-five `mirror_tree` plans use `create_directory` and `remove_directory` alongside `copy` and `delete`. The native executor checks the complete action sequence against the captured tree, including dependency order, and revalidates both complete included inventories before writing. Directory removal pins basic permissions and native filesystem identity in the plan. Unix identity uses device/inode plus creation time where available; Windows uses volume/file index and creation time. Identity numbers are encoded as decimal strings across IPC.

A removed empty directory has no file body to copy. Before removal its verified metadata record is flushed under `directory-metadata/<native-path-hash>.json` inside the private operation backup root. The record contains version, lossless native relative path, native identity and basic permissions. The original directory must still match that record and remain empty immediately before `remove_dir`.

A newly created directory is flushed, then its identity/permissions are recorded under `directory-created/<native-path-hash>.json` before that action is marked completed. This record proves which directory rollback may clean up. No marker files are left in the compared trees. Missing/malformed ownership records block automatic cleanup; an interruption between mkdir and its durable record can require manual review of the retained directory. Unsupported/unavailable native identities fail explicitly.

Directory records use flat hashed keys to avoid collisions between metadata filenames and valid source directory names. Each read is limited to 64 KiB and validates its recorded native path. Ordinary file bodies use the existing verified backup tree. Journals retain the existing 32 MiB new-plan and 64 MiB recovery-input limits, per-action running/completed/failed states, cancellation and stop-on-error behavior. Metadata/identity records are integrity checks against the plan/current state, not signed authenticity proofs.

## Rollback order and protection

Version-five rollback reverses action dependency order: recreate removed parent directories before their children/files, restore/delete file changes, then remove directories created by the original operation from deepest to shallowest. Version-one through four journal order/semantics remain compatible.

Recovery previews verify every original directory record. Recreated removed-directory paths are never overwritten. Cleanup of a created directory requires its original recorded identity/basic permissions, and all current children must be independently scheduled for safe removal. Unexpected/excluded external children block cleanup before recovery starts; checks repeat before execution and each removal. Native `remove_dir` still refuses a nonempty directory if another entry arrives later.

Recovery creates a new metadata/ownership backup set and journal. Removed directories are recreated with their recorded basic permissions, default current owner/group and newly created timestamps/identity. Original owner/group, ACLs, extended attributes and timestamps are not fully preserved. Newly mirrored directories initially use OS defaults. Native identifiers can be reused after deletion on some filesystems; identity plus creation time improves replacement detection but is not a universal race or storage guarantee.

Scans are live and do not lock other processes. Neither the original multi-action mirror nor its rollback is an atomic transaction. Cancellation/errors can leave earlier actions applied. New original journals support [reviewed forward restart](operation-resume.md). Startup never automatically applies it; resuming/redoing a partially applied directory rollback remains planned. Retain complete backup sets for manual review when recovery refuses an inconsistent state.

## CLI

```sh
# Preview only; existing backup base and explicit source direction are required.
clear-compare mirror-tree ./left ./right --source left --exclude='./private/;keep.txt' --backup-dir ./backups
# Apply scans and creates a fresh plan.
clear-compare mirror-tree ./left ./right --source right --backup-dir ./backups --apply
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./recovery-backups
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./recovery-backups --apply
```

Successful preview/application exits 0. Invalid, empty, stale, unsupported or failed/cancelled operations exit 2. Empty means no supported included actions; excluded-entry ancestors may remain. Exported plan JSON is not imported as an executable script. Native filenames remain lossless on their original OS. Packaged-client interaction and installation acceptance remain deferred until planned feature implementation is complete.
