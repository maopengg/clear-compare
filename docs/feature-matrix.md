# Feature matrix

Status is based on implemented behavior; platform/build validation is recorded separately in release notes.

| Plan module | Current implementation | Remaining scope |
|---|---|---|
| F01 Workspace | One window, persisted path/rule settings | Tabs, session library, workspace restore |
| F02 Folder comparison | Recursive bounded scan, metadata or SHA-256, paged file list | Tree aggregation, live refresh, broader criteria |
| F03 Rules | Root-relative and any-level exclusions, semicolon/newline import | Inclusion priority, attributes, content filters |
| F04 Operations | Verified regular-file copy with overwrite backups | Move, delete, rename, attribute/time preservation |
| F05 Synchronization | Reviewed left-to-right or right-to-left copy, stop-on-error, journal, cancellation, persisted copy history and reviewed rollback | Mirror, baseline-aware bidirectional policies, pause, crash resume and recovery redo |
| F06 Text compare | UTF-8/UTF-16 line diff, original-text paired view, bounded computation | Intraline differences, alignment controls and navigation |
| F07 Editing | Side-specific editor, encoding/BOM/newline retention or explicit conversion, digest-checked atomic save, verified backup and reviewed last-save restore | Editor undo/redo history, persistent backup browser and block-copy workflow |
| F08 Text rules | Case, within-line whitespace and line-ending normalization | Syntax filters, regex rules and manual alignment |
| F09 Text merge | Three-way merge buffers, conflict markers, load result into editor; JSON CLI | Conflict navigation and resolution controls, multi-file merge workflows |
| F10 Folder merge | Planned | Baseline-aware folder merging |
| F11 Table compare | CSV/TSV strict parser, positional/key row alignment, numeric tolerance, paged paired cells and CLI | Excel/HTML, column-name mapping, table editing and richer tolerances |
| F12 Binary compare | Streaming hash comparison and bounded 4 KiB hex/ASCII paired pages | Hex editing, byte alignment and search |
| F13 Picture compare | Planned | Pixel diff, overlay and tolerance |
| F14 Conversion | Planned | Document converters and format policies |
| F15 Other viewers | Planned | Patch, clipboard, media/version metadata |
| F16 Archives/snapshots | Planned | Archive adapters and tree snapshots |
| F17 Remote filesystems | Planned | FTP/FTPS/SFTP/network adapters |
| F18 Cloud storage | Planned | WebDAV/S3/Dropbox/OneDrive |
| F19 Automation | JSON compare and copy-preview/apply CLI | Scripts, reports, scheduled workflows |
| F20 Version control | Planned | Git/SVN integration |
| F21 System integration | Native folder dialog | Drag/drop, file associations, context menus |
| F22 Platform specifics | Local filesystem only | ACL, registry, device and metadata adapters |
| F23 Preferences/help | Chinese/English switch and persisted basic settings | Themes, shortcut settings, updater and comprehensive help |

The long-term project remains the complete plan, not only this preview. A green build does not establish full feature parity or three-platform runtime validation.
