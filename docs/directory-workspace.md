# Paired directory workspace

The directory comparison UI defaults to a paired expandable outline. A row represents one native relative path. Each side shows its own name, size and modification time; an absent counterpart has a missing label and empty metadata cells. Both sides share a table/scroll container, so expansion, sorting and paging cannot misalign pairs. The central status preserves content-verified, metadata-only, one-sided, error and type-conflict distinctions.

Click a triangle or double-click a directory name to expand/collapse it. Expand all supports individual collapsed exceptions; Collapse all returns to root entries. Sorting is per sibling group, directories before files. Search filters the entire snapshot before collapse/paging, retains ancestor context and opens matching paths; clearing search returns to the manual expansion state. The visible preorder is paged in groups of at most 500 rows. A later page can start below an off-page parent; indentation and the full relative-path tooltip retain context. Disable Folder tree to use the earlier flat, recursive or breadcrumb navigation.

Click either name to focus the pair. Double-click a file or use Compare contents to open the existing aligned text comparator, which replaces the directory surface and provides line numbers, insert/delete colors, intraline ranges, change navigation and literal search. Binary, picture/table viewers and side-specific editors remain available from the focused-row actions. Text decoding/size restrictions still apply; unsupported input produces an error and can be opened with the appropriate specialized viewer. File copying/deletion/renaming continues through selected file IDs and reviewed plans; directory selection does not silently imply recursive writes.

The native `query_folder` command accepts optional `expanded` IDs and `expandAll`. Omit `expanded` for legacy flat queries. With `expandAll=false`, IDs are explicitly open directories; with `expandAll=true`, they are collapsed exceptions. Search opens its matching ancestors independently. Unknown/non-directory IDs are rejected. `FolderPage.outline` carries display-only names, indentation, expansion, filtered-child presence and ancestor-context flags. It does not provide native paths for reconstruction. The outline reuses snapshot evidence; expanding a directory does not rescan file bodies.

The text preview's conditional body is built without subscribing its outer conditional to nested render state. File-reset and size-measurement effects use explicit dependencies. Browser regression reproduced a render-update loop when opening even an empty text comparison. A malformed unbraced pagination comparison in the view macro also rendered attribute source as visible text; the expression is now braced. The browser test checks normal pagination button state, absence of leaked source text and navigation after opening. Successful compilation alone did not expose these defects.

## Evidence and limits

This is an independent implementation guided by the supplied screenshot and the project's existing source. It is not evidence that Beyond Compare was decompiled or that its complete behavior was recovered. Multiple concurrent session tabs, column resizing and broader native interaction acceptance remain separate work.

Core tests cover collapsed/expanded paths, one-sided and empty directories, expand-all exceptions, invalid IDs, ancestor-preserving search and pagination past 500 entries. A captured-metadata fixture also exercises a 105,132-entry snapshot and verifies that entering a 128-file subdirectory returns only those entries; the test records timing without imposing a hardware-dependent time threshold. The optional browser test below loads the built UI into an isolated browser, serves assets through interception without a server, and supplies synthetic bridge responses. It checks paired row positions, missing cells, timestamps, expansion, selection, actual file double-click, populated and empty text previews, legacy flat/breadcrumb navigation, text highlighting, quick/full phases and cancellation. It does not exercise native file dialogs, real filesystem IPC or packaged-client installation. No real project data is used.

```sh
# Normal Rust/Trunk build remains independent of Node.js.
trunk --no-color build --release
# Optional browser QA dependencies stay under ignored target/work.
npm install --prefix target/work/ui-qa --no-save --package-lock=false playwright@1.64.1
NODE_PATH=./target/work/ui-qa/node_modules node scripts/check-directory-ui.cjs
```

On macOS the optional script uses installed Google Chrome with a fresh temporary profile. Set `CLEAR_COMPARE_QA_BROWSER` to an alternate browser executable, or on other systems provision Playwright's Chromium. Screenshots/logs are development artifacts under `target/work`, not user comparison data.

## Compact desktop layout

The shell is bounded to the viewport. A compact application bar holds secondary tools, and a single paired path row holds directory selection and comparison. Search, differences-only filtering and reviewed file operations share one command row. View settings (tree mode, expansion and sorting), comparison settings (SHA-256 and exclusions), text rules and report controls use dismissible menus. Outside click closes menus; Escape closes the current menu and returns keyboard focus to its summary. They overlay the comparison rather than pushing it down.

The directory table uses the remaining height with its own scrolling and sticky column headings. Rows are 23 px and normal interface text is 12 px. Opening text switches the same central surface to the synchronized two-pane comparator, with 12 px / 22 px source text. Returning to folders retains expansion, selection and list scroll position. The comparison basis, counts and paging remain visible in a compact result footer. Secondary workflows open separately and retain their existing review and write safeguards; baseline synchronization remains independent of comparison invalidation.

Design references: [Folder alignment and comparison criteria](https://www.scootersoftware.com/v5help/dir_how_to_compare.html), [paired text panes and synchronized scrolling](https://www.scootersoftware.com/v5help/viewtext.html), and the supplied Beyond Compare screenshot. This is a layout and interaction reference, not a claim of feature parity.

Optional density regression uses 94 synthetic entries and a 160-line text diff. It checks internal scrolling and content height at 1500×1100, 1260×840 and 900×620, secondary menu access, Escape dismissal, language switching, and return to the retained directory state:

```sh
NODE_PATH=./target/work/ui-qa/node_modules node scripts/check-workspace-density.cjs
```

Ordinary LF/CRLF/CR and missing-final-newline labels are hidden by default. Significant paired line-ending differences remain visible; ignored EOL differences retain the existing rule semantics. Display / legend contains a Show all line endings option for inspecting every original ending. This changes presentation only and never rewrites line endings.

## Inline two-way text merge

Local folder text comparisons provide paired ← / → buttons in a fixed center gutter at the first row of each changed block, including blocks continued on a later 500-row page. Each button stages that block from the other side, while unrelated changes remain untouched. The destination header and toolbar show unsaved state. Undo/redo retain up to 128 selections. One destination is edited at a time; save or discard it before changing the other side. Back navigation and other workspace tools stay locked while changes are pending, and native close/quit requests are blocked until they are saved or discarded.

Saving is explicit. It rechecks the source against the compared text, then uses the existing native editor's digest revalidation, verified original-byte backup, encoding/BOM and target newline policy, and journal. After saving, the text pair is read and compared again; remaining format differences are therefore retained. The directory snapshot is marked as needing a new comparison; it is not silently declared up to date. Failed saves retain the buffer. Staged text changes reset stale search/navigation positions; a shrinking merge clamps pagination to the remaining rows. The generic cancel action is disabled during pending merge operations because native file saving is atomic rather than cancellable. Stale targets are rejected before the first staged change, and a previously missing side that has appeared on disk is rejected rather than treated as empty text.

Git/archive comparisons remain read-only in this view. Creating a missing counterpart uses the directory copy workflow; inline merge writes only existing regular files. Mixed-EOL targets require an explicit newline choice in the existing editor before using inline merging. This does not add concurrent editing of both sides or arbitrary typing in the line viewer.

`NODE_PATH=./target/work/ui-qa/node_modules node scripts/check-inline-merge.cjs` exercises both directions, multiple blocks, undo/redo, no write before Save, failure retention, discard, and stale evidence using synthetic bridge replies. Pure contract tests cover insertions/deletions, Unicode, ignored-equal original text, final newlines and pagination. Native editor integration tests verify encoding retention, backups and external-change rejection. Browser tests do not verify native OS close dialogs.

## Directory context menu

Right-click a directory row (or press Shift+F10 / the Context Menu key on its name) for localized file actions. The clicked cell determines the left/right side. Compare contents, edit, hex/table/image comparison, copy to the other side, rename, delete, copy file name and snapshot properties use existing capabilities. Directory rows offer expansion; file-only actions are disabled for directories, links, errors and absent sides. This does not add recursive directory copy/delete, move, arbitrary “open with”, or native Finder properties.

Context copy/delete/rename target the clicked file only, replacing previous checkbox selection. They pass its opaque native ID to the existing preview commands. Rename collects the new basename first. No file changes occur until the separate reviewed plan is explicitly executed. Properties show the comparison snapshot, not fresh filesystem metadata. Outside click, Escape and Tab dismiss the menu; arrow/Home/End keys navigate enabled actions. The default WebView context menu remains available outside the directory results, including editable text fields.

`NODE_PATH=./target/work/ui-qa/node_modules node scripts/check-directory-context.cjs` checks both languages and copy directions, native ID routing, preview-only mutation, stale checkbox isolation, properties, clipboard, disabled actions, dismissal, keyboard focus, viewport clamping and failed previews. It uses synthetic IPC replies; native file-write safeguards remain covered by the core integration tests.

Both merge directions remain together between the text panes while horizontal text scrolling stays independent of the gutter. Vertical scrolling, difference navigation and paging keep the center controls aligned with their changed block. Scrolling over the gutter scrolls the synchronized text panes.
