# Reviewed forward restart after process exit

File copy, selected deletion, same-directory rename, file mirror and tree mirror now persist the context needed to preview and restart an interrupted original operation. This is distinct from live-task pause/continue. Nothing resumes automatically on application startup. Existing journals without the new context remain eligible for their documented rollback, but cannot be restarted forward.

## Review and execute

Open Operation history and choose **Preview restart** for the original operation. The native backend reads its known local backup set; the UI receives an opaque preview ID and pages of at most 500 action labels. Review the actual target, other comparison directory, original journal, new attempt backup directory and counts:

- Verified complete: current native identity, bytes/basic permissions and relevant backup or directory record substantiate the original action. A journal state alone is insufficient.
- Retry: original target state and required source evidence remain compatible with the original plan.
- Finish staged rename: the original file remains at the journaled native temporary name with its original identity/contents and a verified original backup.
- Retain internal work file: a surviving copy work file has a registered native identity. Its current contents are backed up and verified before that exact file is removed.

Applying consumes that exact cached preview. The operation-set lock and a fresh inspection must confirm the journal, action classifications and work-file evidence have not changed. Live progress, pause/continue and cancellation use the existing task controller. Earlier completed actions remain completed when a later action fails. Reopen the original history to review another attempt or rollback.

CLI preview is read-only; applying always performs a fresh preview:

```sh
clear-compare resume-sync ./backups/PLAN/journal.json
clear-compare resume-sync ./backups/PLAN/journal.json --offset 500
clear-compare resume-sync ./backups/PLAN/journal.json --apply
clear-compare recover-sync ./backups/PLAN/journal.json --backup-dir ./rollback-backups --apply
```

`--offset` is preview-only. Exit status is zero for a preview or fully completed application, and two for rejection or incomplete application. A completed operation with no remaining work is not offered as another forward attempt.

## Persistent evidence and backups

New original runs write `intent.json` before their first action. The bounded version-one context holds the original plan, lossless native roots/relative paths, comparison policy, captured row metadata and available native identity/basic-permission pins. The journal references its SHA-256. Context includes no file bodies; ordinary verified backups and deliberately retained work files contain bytes. A maximum 128 MiB context is accepted. Native identity capability is required for forward restart; ordinary operations can still run on supported paths where that extra proof cannot be captured.

Each operation set contains `operation.lock`. Original execution, forward inspection/application and rollback coordinate through this exclusive native file lock. A crashed process releases its open lock handle. Locks coordinate application participants; they do not prevent external programs from changing source files. Legacy rollback can read sets created before lock files existed.

Original artifacts stay in place. Each reviewed retry creates `attempts/ATTEMPT/`, saves the exact previous journal as `prior-journal.json`, and stores new action backups/identity records there. Per-action `backup_namespace` selects its artifact root, so later retries never overwrite earlier backup files. Completed actions retain their original namespace. A staged rename retains its already-verified backup. The original `journal.json` continues to describe all actions and references the attempts; rollback reads each action's namespace and reverses tree dependencies as usual.

Copy work files use an operation-specific prefix, but the prefix is insufficient proof of ownership. Before writing data, an immutable `temporary-records/` entry records the action, attempt, native relative path and file identity. Restart compares surviving files against these identities. Committed copies are recognized through the recorded file identity now present at the destination plus expected bytes/basic permissions. A different file containing identical bytes does not qualify. Registered surviving partial files are retained under the new attempt's `retained-temporaries/`, with `retained.json` describing their native names, size, digest and identity. A crash during retention may leave both copies; subsequent review preserves the earlier attempt and rechecks any remaining live work file. Unregistered or replaced lookalikes are never deleted by this mechanism.

Metadata and directory entries are flushed using the existing platform-aware helpers. The journal is bounded to 64 MiB; a retry reserves state/error growth with a 63 MiB serialized preflight and fewer than 1,000 prior attempts. Work-file records are bounded to four times the scan entry limit and 256 MiB total metadata. These are resource limits, not guarantees of acceptable latency for every filesystem.

## Revalidation and limits

Roots must remain at their original canonical native locations with the same recorded directory identities/basic permissions. Required original sources must retain their identity, bytes and basic permissions. Pending targets must retain their original identity/content state. Completed deletions and overwritten copies require their original verified backups; directory actions require the correct identity/permission records. The new context also preserves basic permissions for rollback of an overwritten copy. Legacy sets retain their existing metadata limits. Display labels are rebuilt from native paths rather than used as filesystem input.

Selected operations revalidate their selected paths. Mirror policies additionally validate the original full comparison scope against the projected completed actions, including exclusions and directory layout. Only registered, identity-matching internal copy work files on the operation target are temporarily omitted from that inventory. New unrelated entries can invalidate mirror intent. An excluded child appearing under a directory scheduled for removal prevents restart rather than being removed.

On Windows, regular-file proof uses volume/file ID without creation time because filesystem name tunneling can alter creation time during rename/replacement; directory identity retains the existing creation-time check. See Microsoft's [creation-time documentation](https://learn.microsoft.com/en-us/dotnet/api/system.io.filesysteminfo.creationtimeutc). Unix uses the existing device/inode and available birth-time proof. Identity reuse and filesystem-specific semantics remain subject to the platform limitations described in the tree-mirror contract.

No journal signing or authenticity claim is made: context hashes detect mismatched evidence, not deliberate coordinated rewriting of an entire backup set. Keep known local journals and their artifacts together, on the original OS. Multi-file execution is not a filesystem transaction or universal protection against concurrent/adversarial mutation and hardware failure. Conflicting external changes require manual review or reviewed rollback. Recovery-journal forward resume/redo, folder-merge resume, baseline-aware bidirectional synchronization and full metadata preservation remain planned.

## Validation

Core tests kill and reap a real child process paused after a partial copy, then reopen the journal in the parent, retain its partial work file, finish a tree mirror and roll back through the original history. Other tests cover unconfirmed committed copies/directories, staged and case-only rename, selected deletion/rename on either side, replaced roots/identical-byte files, changed source/target/backup evidence, lock contention, expired previews, new excluded children, namespace/context tampering, basic permissions and native non-Unicode names where supported. CLI tests exercise preview/apply and original-history rollback. This is core/backend validation; packaged-client installation and interaction acceptance remain deferred until planned feature implementation is complete.
