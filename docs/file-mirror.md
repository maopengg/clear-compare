# Reviewed regular-file mirror

File mirror makes included regular-file contents follow the explicitly chosen source side. It copies new/changed source files and backs up/deletes target-only regular files. It covers the entire captured comparison under its exclusions, independently of search, current folder, paging or selected rows. Empty directories are retained in this policy. Use the separate [directory-tree mirror](tree-mirror.md) policy to include directory creation/removal. Baseline-aware bidirectional synchronization remains planned.

## Desktop

1. Compare independent local folders with content verification enabled.
2. Choose **File mirror: left → right** or **File mirror: right → left** in the operation menu. These are separate from ordinary selected copy; changing mode clears selection and old previews.
3. Preview the entire comparison. The dialog identifies the actual destination root, direction, copy and deletion counts, individual actions and backup directory. Use Show more for further actions after the initial 500.
4. Apply the exact reviewed plan ID. Copies run before deletions. Cancellation or any error stops later actions; earlier writes can remain. Compare again and retain the journal/backups for review or recovery.

Mirror mode is not saved in named sessions and resets to Copy after application. A metadata-only comparison, unreadable entry, included link/special file or file/directory type conflict blocks the whole preview. Do not treat ignored files or unsupported entries as absent. Excluded subtrees remain outside the operation, including when their parent directories exist only at the destination.

## Validation and journals

The native planner selects every changed/source-only regular file and every target-only regular file from the captured content-verified snapshot. Empty-only differences report that there are no regular-file changes; they do not create an empty journal. All ordinary copy/deletion digest, path, exclusion, backup and permission checks apply.

Before returning a preview and again before creating its execution journal, both complete included trees are re-scanned with SHA-256. Names, entry kinds and contents must still match the captured inventory, including files originally equal. New entries, missing entries, changed equal files or newly unreadable entries expire the plan. Excluded changes do not expire it. Individual actions continue to revalidate immediately before their writes. Deletion additionally rechecks source-counterpart absence before and after its verified backup so a newly appeared source entry blocks removal.

Version-four plans use `policy.mode = mirror_files` and `source_side`. Their actions may mix `copy` and `delete`; the native execution path verifies that the action set matches the complete captured regular-file scope. Native source/destination paths stay in the cached snapshot; display strings are not execution inputs. New journal input/output limits and backup placement follow [file operations](file-operations.md).

Each action is journaled as running before mutation. Recovery can preview a mixed copy/deletion journal after restart, restore verified overwritten/deleted bytes or remove verified copied additions after taking current-file backups. Recreated deleted paths and external content changes block unsafe rollback. Backup sets must remain with their original journals on the original OS. No automatic resume or redo is provided.

Comparison and revalidation are live filesystem scans, not locked snapshots. Other processes can still change files during execution. The operation is not an atomic transaction and does not continuously delete newly created files. Directory additions/removal, preserving complete filesystem metadata, links, type replacement and cross-volume move are outside this policy. Copy may create needed parent directories; recovery retains them. Final packaged-client interaction and installation acceptance remain deferred.

## CLI

```sh
# Preview only; direction and existing backup base are required.
clear-compare mirror-files ./left ./right --source left --exclude='./private/;keep.txt' --backup-dir ./backups
# Apply performs a new scan and plan, rather than trusting exported JSON.
clear-compare mirror-files ./left ./right --source right --backup-dir ./backups --apply
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./recovery-backups --apply
```

Successful preview/application exits 0. Invalid, expired, empty or unsupported plans and failed/cancelled application exit 2. All arguments and JSON fields retain the existing native-path rules. JSON is inspectable operation output, not a command script to import and execute.
