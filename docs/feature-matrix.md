# Feature matrix

Status is based on implemented behavior; platform/build validation is recorded separately in release notes.

| Plan module | Current implementation | Remaining scope |
|---|---|---|
| F01 Workspace | One window, versioned named session library, revision-aware updates/archive, restored path/rule/view settings | Tabs, comparison-result/workspace restore, archived-session browser |
| F02 Folder comparison | Recursive bounded scan, metadata or SHA-256, subtree status/size aggregation, native directory navigation, scope-wide search/sort and paged file list | Live refresh, broader criteria, explicit expansion tree and performance baselines |
| F03 Rules | Root-relative and any-level exclusions, semicolon/newline import | Inclusion priority, attributes, content filters |
| F04 Operations | Verified regular-file copy with overwrite backups | Move, delete, rename, attribute/time preservation |
| F05 Synchronization | Reviewed left-to-right or right-to-left copy, stop-on-error, journal, cancellation, persisted copy history and reviewed rollback | Mirror, baseline-aware bidirectional policies, pause, crash resume and recovery redo |
| F06 Text compare | UTF-8/UTF-16 line diff, rule-aware Unicode-scalar intraline hints with explicit fallbacks, original-text ordinal row alignment, shared scrolling, fixed paging, difference navigation and literal row search | Semantic/manual alignment, syntax highlighting and broader large-file/performance validation |
| F07 Editing | Side-specific editor, encoding/BOM/newline retention or explicit conversion, digest-checked atomic save, verified backup, reviewed last-save restore and bounded UTF-8 splice undo/redo | Persistent backup browser, richer editing/selection and block-copy workflow |
| F08 Text rules | Case, within-line whitespace and line-ending normalization | Syntax filters, regex rules and manual alignment |
| F09 Text merge | Three-way merge buffers, tagged conflict provenance, native normal-save gate, conflict navigation/original-version choices, bounded undo/redo, editor loading and JSON CLI | Working-copy export and richer merge editing |
| F10 Folder merge | Two/three-way independent output, baseline/content classification, deletion/modify and probable rename/modify conflicts, auto text merge, paged version choices, source/output validation, durable journals and CLI | Advanced rename analysis, existing-output/in-place policies, resume/cleanup and richer per-file merge editing |
| F11 Table compare | CSV/TSV and bounded static HTML5 tables, independent source formats/table indices, merged-cell grids, frozen native documents, positional/key row alignment, exact header-name column alignment with explicit missing columns, numeric tolerance, row/column paging and CLI | Excel, arbitrary renamed-column mapping, table editing and richer tolerances |
| F12 Binary compare | Streaming hash comparison, bounded 4 KiB hex/ASCII pages with byte differences, decimal/hex navigation, overlapping hex/UTF-8/UTF-16 streaming search with continuation and CLI | Hex editing/replacement, inserted-byte alignment and broader source adapters |
| F13 Picture compare | PNG/JPEG/BMP/WebP RGBA8 pixel diff, EXIF orientation, tolerance, bounds, aligned previews, overlay/heatmap and CLI statistics | Additional formats/animation, ICC/perceptual comparison, alignment controls, pixel inspector and export |
| F14 Conversion | Planned | Document converters and format policies |
| F15 Other viewers | Planned | Patch, clipboard, media/version metadata |
| F16 Archives/snapshots | Read-only classic ZIP Stored/Deflate browsing, ZIP/directory comparison, exclusions, member SHA-256, paged search and bounded text preview; content-free versioned tree export and historical metadata comparisons | Archive updates and nested/other formats |
| F17 Remote filesystems | Planned | FTP/FTPS/SFTP/network adapters |
| F18 Cloud storage | Planned | WebDAV/S3/Dropbox/OneDrive |
| F19 Automation | JSON compare and copy-preview/apply CLI; self-contained HTML/versioned JSON reports for directory, ZIP, snapshot and Git comparisons | Scripts, specialized-viewer/operation reports and scheduled workflows |
| F20 Version control | Read-only local Git commit/index comparison, pinned revisions/index object IDs, unmerged-stage errors, split/sparse and linked-worktree indexes, executable-mode differences, paged search, bounded blob text previews and JSON CLI | Working-tree filesystem comparison, Git diff/merge-tool integration, SVN and traditional interfaces |
| F21 System integration | Native folder dialog | Drag/drop, file associations, context menus |
| F22 Platform specifics | Local filesystem only | ACL, registry, device and metadata adapters |
| F23 Preferences/help | Chinese/English switch, persisted basic settings, per-client system/light/dark themes and searchable bilingual offline workflow/shortcut help | Custom shortcuts, updater and comprehensive integration documentation |

The long-term project remains the complete plan, not only this preview. A green build does not establish full feature parity or three-platform runtime validation.
