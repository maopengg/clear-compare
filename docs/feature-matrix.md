# Feature matrix

Status is based on implemented behavior; platform/build validation is recorded separately in release notes.

| Plan module | Current implementation | Remaining scope |
|---|---|---|
| F01 Workspace | One window, persisted path/rule settings | Tabs, session library, workspace restore |
| F02 Folder comparison | Recursive bounded scan, metadata or SHA-256, paged file list | Tree aggregation, live refresh, broader criteria |
| F03 Rules | Root-relative and any-level exclusions, semicolon/newline import | Inclusion priority, attributes, content filters |
| F04 Operations | Verified regular-file copy with overwrite backups | Move, delete, rename, attribute/time preservation |
| F05 Synchronization | Reviewed left-to-right copy, stop-on-error, journal, cancellation | Mirror, bidirectional policies, pause and recovery |
| F06 Text compare | UTF-8 line diff, paired read-only view | Intraline differences, alignment and navigation |
| F07 Editing | Planned | Editing, undo/redo, block copying, external-change-safe saving |
| F08 Text rules | Planned | Whitespace/case/syntax normalization and alignment |
| F09 Text merge | Planned | Two/three-way merge and conflict output |
| F10 Folder merge | Planned | Baseline-aware folder merging |
| F11 Table compare | Planned | CSV/TSV/Excel/HTML, key alignment and tolerances |
| F12 Binary compare | Streaming hash comparison only | Hex viewing/editing and alignment |
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
