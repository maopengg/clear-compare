# Comparison report format (version 1)

Reports contain historical comparison metadata, not file bodies. The source snapshot is not rescanned during export. Names are display labels paired with opaque row IDs; reports are not filesystem operation plans or lossless tree snapshots. Use `.cctree` inventories when native path encoding matters.

## Supported sources and formats

- Desktop and CLI: local directories, classic ZIP/directory trees, directory snapshots/current directories, local Git committed trees.
- HTML: standalone UTF-8, Chinese or English labels, no scripts/external resources, escaped dynamic values, original status/warnings and metadata. File timestamps are shown as Unix milliseconds. Directory sizes aggregate retained regular files, not disk allocation.
- JSON: UTF-8 with schema `clear-compare-report` and integer version `1`. Validate both before consuming a report from an unknown version.
- Text, table, picture and synchronization-operation report exports remain planned.

## JSON fields

| Field | Meaning |
|---|---|
| schema, version | Report schema identity and version |
| source | `local`, `archive`, `tree_snapshot` or `git` |
| policy | Original exclusion string and verification basis; null for an older result without recorded policy |
| filtered | Whether rows with status `same` were omitted |
| session | Opaque comparison ID; not a timestamp or a reusable file capability |
| left, right | Original source labels (paths or resolved Git revision labels) |
| summary | Full comparison statistics, unchanged by row filtering |
| rows | Complete retained rows, not the GUI page/search/directory subset |
| warnings | Original snapshot limitations and diagnostics |

`policy.verification` is `sha256`, `size_and_time`, `size_only` or `git_object_identity`. Matching metadata never establishes file content equality. Git equality uses object identity and executable mode, not a fresh SHA-256 scan. An imported tree inventory may already omit entries under its capture rules; the current comparison policy cannot recover those entries.

Rows retain `id`, display `path`, `status`, optional left/right entry metadata and optional descendant summary. Status values are `same`, `changed`, `left_only`, `right_only`, `metadata_match`, `type_conflict`, `error`. A differences-only export omits only `same` rows. File sizes, modification milliseconds and output-byte counts are decimal strings; counts/row indices are bounded integers. Entries include kind, size, optional modified time/digest and optional read error.

## Export semantics

Output is limited to 128 MiB and written through a sibling temporary file. The final filename must be new and outside protected input directories. Existing files and dangling destination symlinks are refused. Cancellation/failure cleans temporary output; this is a single-file export, not a transactional snapshot of the original filesystem.

Git root protection includes actual worktree and Git/common-storage paths even when the selected directory is a subdirectory or a linked worktree. Reports can be placed beside ZIP or snapshot files, but never overwrite those source files.

CLI syntax is `clear-compare report LEFT RIGHT DESTINATION`. Use `--source local|archive|snapshot|git`, `--format html|json`, `--exclude`, `--metadata-only`, `--differences-only`, `--english`. Git requires `--repository` and rejects `--metadata-only`. A `-` right source browses archive/snapshot input alone; use `./-` for a directory literally named `-`.

The CLI prints the saved path, session, row count and byte count as JSON. Exit `0` means a saved report with verified equality/empty results; `1` means saved results contain differences or unverified entries; `2` means comparison errors or export failure. A report containing source-read errors can still be saved and return `2`; inspect JSON stdout and stderr. Invalid arguments cannot produce an output file.
