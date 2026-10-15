# Git commit, index and working-tree comparison

The Git adapter compares locally available commits, captured index records and physical working-tree files in any left/right combination. Each side explicitly selects `commit`, `index` or `working_tree` in `GitCompareRequest`; omitted source fields default to `commit`. The revision field is used only for a commit source. `include_untracked` defaults to false and is valid only with a working-tree side.

Commit sources resolve to full object IDs before tree loading. Index sources use Git's [ls-files formatted output](https://git-scm.com/docs/git-ls-files) with cached entries, root-relative raw NUL-terminated names, object mode/type/ID/size and stage number. Git handles split and sparse indexes; the adapter synthesizes parent directories for aggregation. The repository selector can point at a subdirectory without limiting the index to that subtree. Linked worktrees use their own index and protect both per-worktree and common Git storage from report exports. Bare repositories support commit sources but reject index sources.

## Captured index semantics

The index listing is read once per comparison, including when both sides select it. Each retained regular file points to the object ID captured by that command. Later staging, working-tree edits and branch movement do not change the existing snapshot. A new comparison refreshes it. Text previews read the pinned blob, not a path in the worktree. An object removed/corrupted after capture produces an explicit preview error; there is no working-tree or network fallback.

The displayed `Index @ <SHA-256>` identifies the complete captured formatted listing, before application exclusions. It is not a checksum of the `.git/index` file or a new hash of all blob contents. Git object identity determines file equality; executable-mode changes count as modifications. Symbolic links remain unverified and are not followed; submodules are explicit unsupported-entry errors. LFS pointers remain pointer files.

Index stage zero is the ordinary merged entry. Stages 1–3 are grouped into one path with an explicit unresolved-conflict error; the viewer does not silently choose base/ours/theirs. Text preview is disabled until the conflict is resolved and the index is compared again. Index entries can therefore be incomplete and cannot automatically be treated as a committable tree.

[Intent-to-add](https://git-scm.com/docs/git-diff-index#Documentation/git-diff-index.txt---ita-invisible-in-index) records are represented by their indexed empty blobs, matching Git's visible-index representation. Their working-tree bodies are not staged content and are not read. The UI/report includes a warning explaining this behavior. This is an index-record viewer; it does not reproduce every porcelain `git diff --cached` policy flag.

## Physical working-tree sources

Working-tree mode enumerates the index's tracked names, reads their current filesystem state and optionally includes nonignored untracked paths. The index names/modes are read without fetching or requiring their blobs. `include_untracked` uses Git's standard ignore sources for untracked enumeration; tracked files remain included even when an ignore rule matches. Scope is rooted at the full worktree even when the repository selector points to a subdirectory. Application exclusions filter the bounded listing and observed entries, including directory-only rules; they do not prune Git's own enumeration. Empty directories are not independently listed by Git. Embedded untracked repositories and unsupported file kinds are errors rather than expanded content.

Regular files are streamed in 64 KiB chunks with cancellation, size/mtime/Unix-permission checks before and after reading, a content SHA-256 and a Git blob hash using the repository's SHA-1 or SHA-256 storage format. No objects are written. Raw blob identity deliberately does **not** run clean/smudge filters, textconv, CRLF normalization, LFS resolution or other attributes. A working file can therefore differ from the index even when porcelain Git diff would normalize it to equality. On Unix, executable bits are used when core.filemode is true; otherwise the indexed executable mode is retained where available. On systems without native Unix executable bits, indexed executable modes are retained. core.symlinks=false index-link placeholders remain unverified links.

This is a physical-file comparison, not a porcelain status implementation. Missing tracked files remain missing even when skip-worktree or sparse-checkout marks them; the viewer does not substitute index contents for off-cone files. Assume-unchanged flags do not suppress actual reads. Unmerged index stages define candidate paths/mode hints but never replace the working file's bytes. Selecting the index itself still reports its unresolved stages as errors.

The scan is sequential and does not freeze the entire worktree at one instant. A comparison retains digests/metadata, not every file body. Text previews re-read at most 4 MiB and verify both the captured digest/blob identity and metadata; external edits require another comparison. The digest check also rejects same-size, time-preserving edits. Parent components and final symbolic links are not followed. Working paths cannot address .git administrative components; Windows device/unsupported names are rejected following the [Win32 naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file). Read failures and obstructing parent paths remain explicit errors/type conflicts; symlinks remain unverified. These checks do not guarantee safety against adversarial filesystem races.

Two working-tree sides reuse one capture; both therefore describe the same observed per-file state without a second scan. Report exports retain those observed results and do not re-read sources. Existing source/worktree/common-Git-directory protection also applies to working-tree reports. A bare repository cannot supply a working-tree source. Fresh/unborn worktrees can compare against themselves without a commit; including their never-staged files requires include_untracked.

## Commands and limits

```sh
# Defaults to HEAD on each commit side; the flag overrides that side's revision.
clear-compare git-compare ./repository --right-index
clear-compare git-compare ./repository HEAD~1 HEAD --right-index
clear-compare git-compare ./repository --left-index --right-index

# Raw tracked working files versus index; optional nonignored untracked files.
clear-compare git-compare ./repository --left-index --right-worktree
clear-compare git-compare ./repository --right-worktree --include-untracked

# LEFT/RIGHT remain positional; an index side ignores its supplied revision string.
clear-compare report HEAD ignored ./staged.json --source git --repository ./repository --right-index --format json
clear-compare report HEAD ignored ./working.json --source git --repository ./repository --right-worktree --include-untracked --format json
```

Git comparison exits 0 for verified-equal regular files, 1 for differences/unverified entries, and 2 for invalid inputs, recorded comparison errors or command failures. Reports can be created even when differences/errors produce a nonzero exit; inspect JSON/stdout and stderr. Git source flags are rejected for non-Git report sources. Index and working-tree flags on the same side conflict; include-untracked without a working-tree source is rejected. Exclusions use the same root-relative/any-level semantics as commit comparison and apply before synthesized index directories are added.

Each Git command has a 15-second deadline, cancellation, a bounded stdout reader and a 64 KiB stderr cap. Listing output is capped at 64 MiB, the index at 600,000 raw stage records and 200,000 retained entries including synthesized directories, and the combined comparison at 200,000 paths. Text preview is limited to 4 MiB encoded bytes and the shared text decoder/comparison budgets. Unexpanded sparse-directory modes, malformed records and unsupported filenames fail explicitly. Missing local objects never trigger downloads. If a Git version does not support the required command/format fields, the error is returned rather than substituting a different comparison.

The runner removes inherited Git configuration/redirection/trace environment, disables optional locks/replacement objects/fsmonitor/prompts/transports, and requires `--no-lazy-fetch`. It invokes argument arrays without a shell. No application command checks out files, stages changes, writes index/tree objects, commits or runs textconv/clean/smudge hooks. Source indexes and working-tree files are not modified by comparison. Working-tree enumeration treats nonempty Git diagnostics as failures rather than silently accepting partial scopes; known sparse-index expansion advice is suppressed only for the invoked commands, without changing repository configuration. Object garbage collection and adversarial concurrent repository mutation can still invalidate future reads.

Difftool/mergetool registration, SVN and write integrations remain planned. Packaged-client interaction and installation acceptance is deferred until planned feature implementation is complete.
