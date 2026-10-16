# Paired directory workspace

The directory comparison UI defaults to a paired expandable outline. A row represents one native relative path. Each side shows its own name, size and modification time; an absent counterpart has a missing label and empty metadata cells. Both sides share a table/scroll container, so expansion, sorting and paging cannot misalign pairs. The central status preserves content-verified, metadata-only, one-sided, error and type-conflict distinctions.

Click a triangle or double-click a directory name to expand/collapse it. Expand all supports individual collapsed exceptions; Collapse all returns to root entries. Sorting is per sibling group, directories before files. Search filters the entire snapshot before collapse/paging, retains ancestor context and opens matching paths; clearing search returns to the manual expansion state. The visible preorder is paged in groups of at most 500 rows. A later page can start below an off-page parent; indentation and the full relative-path tooltip retain context. Disable Folder tree to use the earlier flat, recursive or breadcrumb navigation.

Click either name to focus the pair. Double-click a file or use Compare contents to open the existing aligned text comparator, which scrolls into view and provides line numbers, insert/delete colors, intraline ranges, change navigation and literal search. Binary, picture/table viewers and side-specific editors remain available from the focused-row actions. Text decoding/size restrictions still apply; unsupported input produces an error and can be opened with the appropriate specialized viewer. File copying/deletion/renaming continues through selected file IDs and reviewed plans; directory selection does not silently imply recursive writes.

The native `query_folder` command accepts optional `expanded` IDs and `expandAll`. Omit `expanded` for legacy flat queries. With `expandAll=false`, IDs are explicitly open directories; with `expandAll=true`, they are collapsed exceptions. Search opens its matching ancestors independently. Unknown/non-directory IDs are rejected. `FolderPage.outline` carries display-only names, indentation, expansion, filtered-child presence and ancestor-context flags. It does not provide native paths for reconstruction. The outline reuses snapshot evidence; expanding a directory does not rescan file bodies.

The text preview's conditional body is built without subscribing its outer conditional to nested render state. File-reset and size-measurement effects use explicit dependencies. Browser regression reproduced a render-update loop when opening even an empty text comparison. A malformed unbraced pagination comparison in the view macro also rendered attribute source as visible text; the expression is now braced. The browser test checks normal pagination button state, absence of leaked source text and navigation after opening. Successful compilation alone did not expose these defects.

## Evidence and limits

This is an independent implementation guided by the supplied screenshot and the project's existing source. It is not evidence that Beyond Compare was decompiled or that its complete behavior was recovered. Tabbed workspaces, column resizing and broader native interaction acceptance remain separate work.

Core tests cover collapsed/expanded paths, one-sided and empty directories, expand-all exceptions, invalid IDs, ancestor-preserving search and pagination past 500 entries. A captured-metadata fixture also exercises a 105,132-entry snapshot and verifies that entering a 128-file subdirectory returns only those entries; the test records timing without imposing a hardware-dependent time threshold. The optional browser test below loads the built UI into an isolated browser, serves assets through interception without a server, and supplies synthetic bridge responses. It checks paired row positions, missing cells, timestamps, expansion, selection, actual file double-click, populated and empty text previews, legacy flat/breadcrumb navigation, text highlighting, quick/full phases and cancellation. It does not exercise native file dialogs, real filesystem IPC or packaged-client installation. No real project data is used.

```sh
# Normal Rust/Trunk build remains independent of Node.js.
trunk --no-color build --release
# Optional browser QA dependencies stay under ignored target/work.
npm install --prefix target/work/ui-qa --no-save --package-lock=false playwright@1.64.1
NODE_PATH=./target/work/ui-qa/node_modules node scripts/check-directory-ui.cjs
```

On macOS the optional script uses installed Google Chrome with a fresh temporary profile. Set `CLEAR_COMPARE_QA_BROWSER` to an alternate browser executable, or on other systems provision Playwright's Chromium. Screenshots/logs are development artifacts under `target/work`, not user comparison data.
