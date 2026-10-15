# Common-baseline bidirectional synchronization

Bidirectional reconciliation compares two local trees with their last common content state. It uses SHA-256 and directory presence rather than timestamps to infer additions, modifications and deletions. It supports regular files, nested directories and empty directories under one fixed exclusion policy. Links, unreadable entries and unsupported kinds block preparation. Type replacement remains an explicit blocker requiring separate resolution; it is never silently implemented as recursive deletion.

## Desktop workflow

1. Compare independent directories with content verification enabled. For the first baseline, bring the included trees into agreement through a reviewed directional operation or other deliberate edits.
2. Expand **Bidirectional sync and common baseline** and choose **Create common baseline**. Creation requires equal included contents and directory presence, including empty directories. Save the `.ccbase` file outside both trees. Existing baseline files are never overwritten by initialization.
3. On subsequent runs, select that baseline and preview bidirectional sync. Roots, their native identities and the exact exclusions must match. Source data is revalidated against the folder comparison.
4. Review both directions, deletions and baseline/left/right metadata. Unresolved/type conflicts appear before ordinary rows. Each page contains at most 500 rows, including unchanged/previously deleted entries that inform the baseline. Choose individual winners or an explicit whole-conflict preference. A directory winner applies to its entire branch.
5. Execute that exact native preview ID. Each target uses verified backups, directory identity records and the existing action journal. Live progress, safe pause/continue, cancellation and same-process reconnection use the shared task registry. Open editors receive both sides' affected paths; clean affected documents close and drafts remain for review.
6. Only when both included trees match the reviewed final inventory does the baseline advance. Review the returned `run.json` location. **Preview latest bidirectional rollback** or **Choose run.json to preview coordinated rollback** restores both sides' applied actions in reverse dependency order and restores the exact previous baseline bytes after successful file recovery.

The baseline and its sibling `.lock` file must remain together. Selecting a file in the UI passes only a cached native selection ID to later commands; the frontend never supplies reconstructed action paths. Every resolution creates a new preview token, invalidating the previous token. Recompare and prepare again after cancellation, failure or external changes.

## Classification and conflict policy

- Equal current file hashes, equal directory presence or common absence retain that common state. Independently arriving at identical bytes needs no copy; the reviewed run can still acknowledge this state in a new baseline revision.
- An addition or modification on only one side propagates to the other. A deletion propagates only if the opposite side still matches the old baseline. Two edits of the same file and delete-versus-modify require a winner.
- When both sides retain a directory, descendants reconcile independently. Directory fingerprints include the included descendants, so deleting a directory while the opposite branch changed produces one branch conflict rather than treating an unchanged directory marker as unchanged contents.
- A directory choice includes all its descendant actions. This prevents accepting a parent deletion while leaving contradictory child choices active. Deletion of a modified branch is possible only as an explicit chosen winner, with verified backups before file removal.
- Excluded entries are not copied or deleted. Their existing ancestors cannot be removed. An inferred deletion of such an ancestor becomes a conflict whose deletion winner is unavailable; preserving the present branch can recreate the included directory on the other side. A newly added branch containing only excluded children can propagate its visible empty directory without copying the exclusions.
- File/directory type conflicts are displayed as blocked, including cases where only one side changed type. Resolve those types deliberately and compare again. Missing files, empty strings and directories are distinct states; metadata equality never supplies a content digest.

Content rules do not synchronize owner, ACLs, timestamps, attributes or hard-link relationships. Basic permissions remain part of the underlying copy/backup/recovery implementation, but permission-only changes do not count as content changes in this baseline policy. Rename detection is not inferred: path additions/deletions reconcile under their normal evidence, with case/name aliases rejected by native write checks.

## Persistent format and ordering

Version-one `.ccbase` metadata includes a generated ID, revision incremented by successful forward runs, last run ID, lossless native roots, native root identity/basic-permission evidence, exact exclusions and an inventory of directory markers/regular-file SHA-256 hashes. Coordinated rollback restores the exact previous revision as well as its contents; distinct last-run IDs distinguish subsequent branches. It contains no file bodies. Maximum baseline size is 64 MiB and a common inventory has at most 200,000 entries. Hierarchy, native paths, duplicate entries and digests are validated on reading. Baselines are local to their original OS/root identities; a recreated root is not silently accepted.

A sibling lock coordinates native participants across profile revision replacements. Original execution and coordinated rollback hold this lock, plus an exclusive operation-set lock. A crashed process releases open lock handles. Readers do not create profile locks or journals; explicit initialization creates the lock. File locks coordinate application participants rather than locking other programs out of the compared trees.

For a supplied backup base, one run uses `.bidirectional/RUN/`:

- `baseline-before.json` retains the exact original bytes; `baseline-after.json` holds the reviewed next revision before target writes begin.
- Bounded `intent.json` pins the baseline reference/digest, desired common inventory and at most one child plan per target. `run.json` references its SHA-256 and records started/planned child journals, results, errors and baseline-commit state. Run intent/input bounds are 128 MiB; oversized evidence fails before target mutations.
- Child plans retain normal `BACKUP_BASE/CHILD/journal.json` histories and artifacts. Version-six `reconcile` plans allow ordered mixed copy/deletion/directory actions for one explicit target. Source direction must be the opposite side. Directory creation precedes file work; empty-directory removal follows descendant deletion. Each child keeps existing immutable restart intent and per-attempt backup support.
- The coordinator validates the whole captured scope before starting, then uses the shared native commit checks for every child action. Scoped mutation validators recheck both root identities at inner write boundaries, including after acknowledged pauses and during coordinated recovery.
- Final verification scans both included trees against the exact desired path/kind/hash inventory. Baseline replacement occurs under a safe nonpausing commit guard only after that verification. Errors/cancellation before the baseline commit leave the previous baseline as the reference and record partial action results. Cancellation arriving after a verified commit cannot retroactively undo it. A filesystem error during baseline replacement/verification can require inspection of both baseline artifacts and the run journal.

This is a multi-step operation, not a transaction or a locked filesystem snapshot. Earlier actions can remain after failure. A fresh baseline-based preview can reconcile already-completed common states and remaining work; it does not pretend that a previous incomplete run was atomic. Individually restarting/recovering a child does not advance or restore the common baseline. Prefer coordinated rollback when reverting an entire run.

Coordinated rollback refuses a baseline with a newer revision or changed bytes and revalidates the run, baseline and each child preview before writing. Missing started-child journals require manual review; unstarted children are skipped. Current baseline bytes are retained under the run's recovery history before rollback. If only part of rollback completes, the baseline remains unchanged and all recovery journals/backups must be retained for review. Forward resume of partially completed recovery/redo remains planned.

Hashes detect inconsistent evidence, not deliberate coordinated rewriting of an entire unsigned backup set. Keep known local profiles/journals with their artifacts. Application identity checks and flushed files/directory entries do not provide universal protection against power loss, filesystem-specific identity reuse or adversarial races across all storage systems.

## CLI

Commands preview by default; `--apply` is required for writes. Backup directories must already exist and be outside both compared roots.

```sh
clear-compare baseline ./left ./right ./common.ccbase --exclude='-node_modules/;-keep'
clear-compare baseline ./left ./right ./common.ccbase --exclude='-node_modules/;-keep' --apply
clear-compare bidirectional ./left ./right --baseline ./common.ccbase --backup-dir ./backups --exclude='-node_modules/;-keep'
clear-compare bidirectional ./left ./right --baseline ./common.ccbase --backup-dir ./backups --exclude='-node_modules/;-keep' --prefer right --apply
clear-compare recover-bidirectional ./backups/.bidirectional/RUN/run.json --backup-dir ./recovery-backups
clear-compare recover-bidirectional ./backups/.bidirectional/RUN/run.json --backup-dir ./recovery-backups --apply
```

CLI preview/output is JSON. Desktop terminal replies set `details_in_journal` and retain child journal references while omitting full action arrays; authoritative full records stay in the native run/child journals. CLI output retains the full actions. `--offset 500` pages sync or recovery rows and is preview-only. `--prefer left|right` explicitly resolves eligible conflicts across the complete scope in one pass; unsupported type replacement and excluded-ancestor deletion remain blocked. Zero means a valid conflict-free preview or successful application/recovery; two means blocked/unresolved preview, invalid input or incomplete application. No automatic startup synchronization or scheduled writes are enabled.

## Validation

Core/CLI tests cover independent edits, additions/deletions, empty directories, divergent edits, delete/modify branch choices, excluded-child protection, exact baseline rollback, later revision refusal, scope/profile expiry, root replacement, lock contention, blocked type replacement, whole-scope conflict preference and pagination. Cooperative thread tests cancel after one side completes and verify both partial rollback and unchanged baseline. Unix tests replace roots during paused forward/recovery streams and assert the replacement tree and profile are not overwritten. Existing operation/recovery compatibility tests continue. Packaged-client interaction and installation acceptance remain deferred until planned feature implementation is complete; green builds do not establish full reference-product parity.
