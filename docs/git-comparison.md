# Git commit and index comparison

The Git adapter compares two locally available commit trees, a commit and the captured index, or the captured index on both sides. Each side explicitly selects `commit` or `index` in `GitCompareRequest`; omitted source fields default to `commit`. The revision field is used only for a commit source.

Commit sources resolve to full object IDs before tree loading. Index sources use Git's [ls-files formatted output](https://git-scm.com/docs/git-ls-files) with cached entries, root-relative raw NUL-terminated names, object mode/type/ID/size and stage number. Git handles split and sparse indexes; the adapter synthesizes parent directories for aggregation. The repository selector can point at a subdirectory without limiting the index to that subtree. Linked worktrees use their own index and protect both per-worktree and common Git storage from report exports. Bare repositories support commit sources but reject index sources.

## Captured index semantics

The index listing is read once per comparison, including when both sides select it. Each retained regular file points to the object ID captured by that command. Later staging, working-tree edits and branch movement do not change the existing snapshot. A new comparison refreshes it. Text previews read the pinned blob, not a path in the worktree. An object removed/corrupted after capture produces an explicit preview error; there is no working-tree or network fallback.

The displayed `Index @ <SHA-256>` identifies the complete captured formatted listing, before application exclusions. It is not a checksum of the `.git/index` file or a new hash of all blob contents. Git object identity determines file equality; executable-mode changes count as modifications. Symbolic links remain unverified and are not followed; submodules are explicit unsupported-entry errors. LFS pointers remain pointer files.

Index stage zero is the ordinary merged entry. Stages 1–3 are grouped into one path with an explicit unresolved-conflict error; the viewer does not silently choose base/ours/theirs. Text preview is disabled until the conflict is resolved and the index is compared again. Index entries can therefore be incomplete and cannot automatically be treated as a committable tree.

[Intent-to-add](https://git-scm.com/docs/git-diff-index#Documentation/git-diff-index.txt---ita-invisible-in-index) records are represented by their indexed empty blobs, matching Git's visible-index representation. Their working-tree bodies are not staged content and are not read. The UI/report includes a warning explaining this behavior. This is an index-record viewer; it does not reproduce every porcelain `git diff --cached` policy flag.

## Commands and limits

```sh
# Defaults to HEAD on each commit side; the flag overrides that side's revision.
clear-compare git-compare ./repository --right-index
clear-compare git-compare ./repository HEAD~1 HEAD --right-index
clear-compare git-compare ./repository --left-index --right-index

# LEFT/RIGHT remain positional; an index side ignores its supplied revision string.
clear-compare report HEAD ignored ./staged.json --source git --repository ./repository --right-index --format json
```

Git comparison exits 0 for verified-equal regular files, 1 for differences/unverified entries, and 2 for invalid inputs, recorded comparison errors or command failures. Reports can be created even when differences/errors produce a nonzero exit; inspect JSON/stdout and stderr. Index flags are rejected for non-Git report sources. Exclusions use the same root-relative/any-level semantics as commit comparison and apply before synthesized index directories are added.

Each Git command has a 15-second deadline, cancellation, a bounded stdout reader and a 64 KiB stderr cap. Listing output is capped at 64 MiB, the index at 600,000 raw stage records and 200,000 retained entries including synthesized directories, and the combined comparison at 200,000 paths. Text preview is limited to 4 MiB encoded bytes and the shared text decoder/comparison budgets. Unexpanded sparse-directory modes, malformed records and unsupported filenames fail explicitly. Missing local objects never trigger downloads. If a Git version does not support the required command/format fields, the error is returned rather than substituting a different comparison.

The runner removes inherited Git configuration/redirection/trace environment, disables optional locks/replacement objects/fsmonitor/prompts/transports, and requires `--no-lazy-fetch`. It invokes argument arrays without a shell. No application command checks out files, stages changes, writes index/tree objects, commits or runs textconv/smudge hooks. Source indexes and working-tree files are not modified by comparison. Object garbage collection and adversarial concurrent repository mutation can still invalidate future reads.

Working-tree/index filesystem comparison, difftool/mergetool registration, SVN and write integrations remain planned. Packaged-client interaction and installation acceptance is deferred until planned feature implementation is complete.
