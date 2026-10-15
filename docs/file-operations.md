# Reviewed file copy, deletion, rename and recovery

The local folder workspace supports directional regular-file copy and explicit selected-file deletion or single-file same-directory rename on either side. These operations use native cached paths, a reviewed plan, content checks, verified backups and a state journal. Directory removal, recursive deletion, cross-directory move, bulk/directory rename, mirror and baseline-aware bidirectional policies remain planned.

## Desktop workflow

1. Compare independent directories. Choose Copy, Delete left/right files or Rename left/right file in the operation selector. Changing the operation clears selection and any visible plan. The reverse-direction checkbox applies only to copying.
2. Select readable regular files on the relevant side. Deletion can select unchanged or target-only files as well as changed files; turn off Differences only to see unchanged files. Directories and links cannot be selected for deletion or rename. Rename requires exactly one file and one new leaf name.
3. Preview the plan. It identifies the actual target directory, side, each relative name/size, action type and backup location. The viewer initially renders 500 actions and can show more. Selection alone does not write or remove anything.
4. Execute that exact native plan ID. Source/target checks run again; errors/cancellation stop later actions. Earlier completed actions remain completed. Compare again after applying or encountering a partial failure.
5. Open operation history to preview rollback. Review its actual target and action list before applying. Deletion rollback restores absent files without overwriting anything that has since appeared at those paths.

Plans are consumed once. Native preview failure invalidates an older cached plan. Successful/partially completed application clears the local comparison and returns the operation selector to Copy. Destructive mode is not saved in named sessions. Open editors follow successful renames and their rollback without changing IDs, saved text, drafts or undo history. Last-save restoration remains bound after an app-controlled rename. Other file changes invalidate affected editor locations: clean affected editors close, dirty buffers remain with a warning and require review/reopen before saving. Native digest and filename checks still prevent saving over external edits or silently reversing an external case-only rename.

## Deletion ordering

For each selected regular file, planning verifies the current contents against any captured folder digest, checks recorded size/time and pins a fresh SHA-256 plus basic permissions. Execution:

- Revalidates the target root, opaque entry/native path, exclusions, regular-file type, digest and basic permissions.
- Creates a new backup under `backups/<plan-id>/files/<native-relative-path>`; copies in 64 KiB chunks with cancellation, preserves basic permission bits/read-only state and flushes the file.
- Verifies backup contents and permissions, flushes directory entries on Unix, rechecks cancellation and the original target, then removes that file.
- Verifies absence even if cancellation arrives just after removal, records completion and stops before subsequent actions if cancelled.

The journal is written before an action enters `running`, then updated after completion/failure. A failing or incomplete backup prevents removal. No directory, counterpart file or unselected destination-only file is removed. New plan roots use private Unix directory permissions. Native roots and existing backup bases must remain canonical and nonoverlapping with compared roots.

Basic permissions mean Unix mode bits or the read-only flag on other systems. Owner/group, ACLs, extended attributes, filesystem flags, original timestamps and hard-link relationships are not preserved as a complete metadata snapshot. Windows read-only/locked files can fail removal; the app does not silently clear those attributes. OS trash/recycle-bin integration is not implemented. These are application backups, not trash entries.

## Rename ordering and interruption

Rename supports one readable regular file within its existing directory, on either side. The counterpart remains untouched. The new name cannot traverse directories, match an exclusion or occupy an existing name, including a hard link to the source. Windows reserved names are rejected. Lossless native names are retained; the CLI also accepts native non-Unicode names where the filesystem supports them.

Planning pins bytes/basic permissions, the new name and a same-directory `.clear-compare-rename-<plan-id>` temporary name. Execution verifies the original backup just as deletion does, then performs two native no-replace renames: original to temporary, then temporary to new. This allows case-only changes on case-insensitive filesystems while refusing a name created by another process. There is no copying/unlinking or overwriting fallback if the filesystem cannot perform the native operation. Ordinary native rename retains the file itself, including its timestamps; the separate application backup still has only the documented basic metadata guarantees.

Cancellation or failure between the two steps may leave the file at the recorded temporary name. History recovery verifies that staged or destination file and the original backup, then previews a rename back. It backs up the current live file before moving it, refuses recreated original names and preserves unrelated destination files when restoring a staged source. A completed rollback can preserve the original file's identity/timestamps. Changed/missing live files, occupied temporary names or modified backups block recovery; this is not automatic resume or backup-based reconstruction of a vanished live file.

Exact-name checks remain case-sensitive even on an insensitive filesystem. Canonically equivalent Unicode spellings are accepted only when filesystem lookup resolves both to the same regular file; this does not conflate distinct files on a normalization-sensitive filesystem. Full platform/filesystem runtime acceptance remains deferred.

## Journals and compatibility

`SyncResult` contains the immutable `plan`, per-action states and journal path. Copy/deletion plans use `plan.version = 2`:

- `policy.mode = selected_copy`, with `source_side`, or `delete_selected`, with an explicit `target_side`. `source_side` is ignored for deletion.
- Each `FileAction.operation` is `copy` or `delete`; a plan cannot mix action kinds under these policies.
- `target_existed` records prior target presence. Copy has a source digest and optional prior-target digest; deletion has no source digest and requires a prior-target digest/basic permissions.
- `target_native` and `relative_native` carry lossless native paths. `target` and `path` are display labels, never filesystem inputs.

Rename plans use version 3 and `policy.mode = rename_selected` with explicit `target_side`, exactly one `operation = rename`, equal original source/target digests, basic permissions, and `rename_to` native destination/temporary paths. These paths must be distinct leaf names under the original parent; the temporary name must match the plan identity. Display labels remain nonauthoritative. Copy/deletion actions have no rename paths.

Version-one copy journals remain readable: absent version/policy/operation default to the earlier copy semantics, and `overwrite` is accepted as the old alias of target_existed. Journals lacking native paths retain the existing explicit manual-recovery limitation. Unsupported versions, inconsistent policies/digests/permissions, duplicate targets and mismatched backup placement are rejected. A new pending journal must fit 32 MiB before execution starts; persisted recovery input remains limited to 64 MiB. Error text is clipped after 64 KiB plus an indicator, leaving room for state changes. This preflight also applies to new copy plans.

Recovery derives displayed action names from decoded native relative paths, so old or altered journal labels cannot misrepresent the paths in its preview. Journals and backups must remain together at their original location and on the original OS. Content checks validate the selected backup; the format is not a cryptographic authenticity/signature mechanism. Use operation history or known local backup sets.

## Recovery behavior

Copy rollback retains the existing rules: restore a verified overwritten backup or back up/remove a digest-matching copied addition. Deletion rollback expects an absent path and restores a verified original backup with its recorded basic permissions. It uses no-clobber persistence and never overwrites a recreated file, including one containing the same bytes. Missing subdirectories can be recreated; the original selected target root must already exist. Restoration retains original bytes, with newly created filesystem timestamps.

A `running`/`failed` entry is reconciled against current state. If the original file still exists unchanged, that action has nothing to recover. If deletion left the path absent and its original backup is intact, it can be restored. Other content states, modified backups/permissions, symlink traversal and conflicting path types block recovery. There is no automatic redo/resume or recursive directory cleanup. A multi-file operation or rollback is not a transaction; earlier changes can remain after an error.

Files and journals are explicitly flushed; on Unix directory entries are also flushed for journals, backups and mutations. This is not a universal guarantee against power loss, storage hardware failures or adversarial filesystem races across all supported systems. Keep backups until the result has been reviewed.

## Command line

```sh
# Preview exact relative paths; a side, path(s) and an existing backup base are required.
clear-compare delete ./left ./right --side right --path sub/file.txt --path old.bin --backup-dir ./backups
# Apply re-scans and generates a fresh plan; it does not execute previously exported JSON.
clear-compare delete ./left ./right --side right --path sub/file.txt --backup-dir ./backups --apply
# Preview/apply one same-directory native rename; new-name is a leaf, not a path.
clear-compare rename ./left ./right --side left --path sub/old.txt --new-name New.txt --backup-dir ./backups
clear-compare rename ./left ./right --side left --path sub/old.txt --new-name New.txt --backup-dir ./backups --apply
# Reopen the resulting journal to preview/apply rollback.
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./recovery-backups
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./recovery-backups --apply
```

Paths are literal native relative paths, without wildcard expansion; a leading ./ is accepted. Parent traversal, absolute/drive-relative paths, missing/excluded entries and directories are rejected. CLI parsing requires explicit target side and at least one --path. Preview/application exits 0 when successful and 2 for invalid input or failed/cancelled actions. The copy command's existing comparison/selection behavior remains unchanged. Packaged-client interaction and installation validation remain deferred until planned feature development is complete.
