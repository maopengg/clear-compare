# Cooperative file-operation tasks

Reviewed file application and rollback now run as bounded native background tasks. Preview and the underlying verified backup/journal formats remain unchanged. `apply_sync` and `apply_sync_recovery` consume the reviewed plan ID and return a native `TaskHandle` instead of keeping the command open until all work finishes.

## Desktop workflow

After application starts, the progress panel shows verified completed-action counts, current relative path and tracked file-stream read/write byte counts. Reads include repeated hashes and post-write verification; writes include temporary data and backups. These are work counters, not a percentage or an estimate of remaining bytes. Metadata/journal bytes and later editor-location refresh reads are not included. Display paths/error text are bounded and clipped with an indicator; numbers cross IPC as decimal strings.

**Pause** requests a cooperative checkpoint. **Waiting for a safe pause point** remains distinct from **Paused**; an OS call, final mutation validation or journal flush can delay acknowledgment. **Continue** also withdraws a pending pause request. **Cancel this task** targets its exact task ID and wakes a paused worker. Previously completed writes remain applied and can be reviewed in operation history.

The frontend polls small status snapshots at 250 ms intervals only while following an operation; it receives the full result once. Startup checks for an uncollected operation in the same native app process, allowing reconnection after a WebView reload or a lost IPC response. **Connect file task** retries that lookup; a progress-panel reconnect control is also available after monitoring stops. Draft persistence across WebView reload is not provided by this feature.

Task state and its pending result stay in one native slot. The operation admission guard remains held until the terminal result is collected, preventing another write from racing result publication or losing an earlier uncollected result. The terminal result is consumed once by exact ID. After collection a later operation replaces the slot and older task IDs expire. No task server, watcher or extra progress thread is started.

## Shared native control

`clear_core::task::Control` is independent of Tauri and owns a cancellation token, pause condition variable, small progress state and opaque identity. A synchronous worker explicitly enters its one-shot `run` scope. The scoped thread-local binding is restored on normal return, errors and unwinding; it is not propagated implicitly into spawned threads. Only cancellation checks using that scope's exact token can pause. Independent noncancellable post-commit verification tokens remain nonpausing.

Existing hash/copy/backup loops cooperate at bounded chunk checkpoints. A pause waits on a condition variable rather than spinning; cancellation wakes it, with a short timeout also checking direct cancellation-token changes. Counters saturate instead of wrapping. A running mutation can still honor cancellation before the filesystem commit; cancellation after commit does not suppress mandatory result verification.

Mutations use an explicit nonpausing guard. Before entering, the worker can pause; after entering it refreshes native paths, relevant digests/permissions/backups and prepared content before writing. A new pause remains requested until the next safe checkpoint. Copy, deletion, rename, directory mutation and their rollback share this rule. A generation counter detects a pause during complete-scope preflight and repeats that preflight before creating a journal. Thus a long pause does not turn old validation into permission to overwrite a newly changed file.

`cancelled` action states are now distinguished from failures even when cancellation occurs inside an action. Recovery also reconciles cancelled actions: unchanged originals are skipped, while verifiable staged rename/deletion states can still be recovered. Earlier journals retain their existing read compatibility. A terminal task phase is not substituted for journal evidence; full action results and backups remain authoritative for recovery.

## IPC contract

- `active_operation` returns an uncollected handle or null, without starting work.
- `operation_status(task_id)` returns a small `TaskProgress` snapshot.
- `pause_operation(task_id)`, `resume_operation(task_id)` and `cancel_operation(task_id)` control only the matching current task. Terminal controls cannot cancel a later unrelated operation.
- `take_operation_result(task_id)` returns exactly one tagged file/recovery/error outcome after the terminal phase is published.
- `TaskPhase` distinguishes running, pause_requested, paused, cancel_requested, completed, failed and cancelled.

The busy guard continues to protect other native operations while this task or its result is outstanding. Closing an editor remains supported; editor-location updates still avoid reopening a closed document and retain in-memory unsaved buffers during ordinary operations. The final file/recovery outcome includes location/staleness notices as before.

## Limits and validation

This is live-task pause/continue and same-process reconnection. Task handles, pause flags and outcomes are not durable across a native process restart. New file-operation journals support separate [reviewed forward restart](operation-resume.md) after process exit, using persisted original intent and per-attempt artifacts; operation history also retains reviewed rollback. Startup never automatically resumes writes. Old journals without forward context support their existing rollback only. Partial-recovery resume and redo remain planned. Final validation and OS calls are cooperative boundaries, not interruptible transactions. Closed/crashed processes can leave the documented running/partial journal states.

Core tests coordinate real worker threads to verify acknowledged pause, resume, cancellation wakeup, cancellation inside mutation guards, bounded progress, token isolation, scope restoration after panic and expired preflight evidence after a pause. Native registry tests verify admission until result collection, exact task IDs, once-only results and expiration. Normal file-operation/recovery compatibility tests continue. Packaged-client interaction and installation acceptance remain deferred until the planned feature implementation is complete.


## Coordinated two-root tasks

Common-baseline bidirectional apply and coordinated rollback use the same one-slot task registry and return tagged bidirectional outcomes. Their total includes the native action count plus profile commit/restoration. Child operations share the parent control token, progress and pause semantics. Scoped mutation validators additionally recheck both root identities at inner commit boundaries, including after a pause during file/backup streaming. Validators do not propagate implicitly to spawned threads and restore their stack on errors/unwind. See [bidirectional evidence and partial-operation limits](bidirectional-sync.md).
