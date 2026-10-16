# Clear Compare interface

## Scene

A developer checks project changes on a desktop monitor in a normally lit workspace, reading paths and source text before copying files. A light, restrained workspace keeps attention on the differences.

## Structure

One fixed-height desktop window. A 36 px application bar, one paired path row and one compact command row precede the main comparison surface. Rules, sorting and reports open in dismissible menus; secondary workflows open separately. They do not permanently consume list space. The aligned directory table fills the remaining height, with its own scrolling and sticky headings. Name, size and modification time remain separate for each side. The default outline expands native directory IDs in place, retains ancestors under search and shows missing counterparts explicitly.

Double-clicking a file replaces the directory surface with a full-height synchronized text comparison. Returning to folders retains the directory DOM, expansion, selection and scroll position. Comparison basis, summary and paging share a compact footer. Advanced baseline synchronization remains available independently of the comparison snapshot so completed-run evidence survives invalidation. The synchronization plan appears only after the user asks to preview selected files. Local text differences expose per-block directional merge arrows, bounded undo/redo and explicit target-side Save/Discard controls in the comparison view. Unsaved merges retain their buffer and block navigation and native close/quit until resolved.

## Tokens

CSS tokens live in `crates/ui/style.css`. The accent is `oklch(.45 .086 230)`. Primary surfaces are neutral white, supporting surfaces carry a small blue chroma, and body text uses a dark neutral. Status labels combine words with color.

## Typography and controls

System sans-serif for the workspace; system monospace for file sizes and source text. Normal UI and source text use 12 px; directory rows are 23 px and source lines 22 px. Compact controls share radii, spacing, disabled states and focus outlines. Paths are truncated in lists and exposed through titles; notices wrap long paths.

## Loading and empty states

Folder comparisons publish a size-only result before optional SHA-256 verification. Modification times are displayed but never determine comparison status; equal-sized files remain unverified until their contents are checked. New sessions use metadata mode, and saved sessions retain their choice. The result basis, scan/verification stage and stop-and-keep-list affordance stay explicit. Other busy operations have a status line and cancellation affordance. Once comparison or file content is available, task feedback moves below the content into a compact bottom status area; initial empty scans retain their upper status line. The routine symbolic-link policy notice is omitted from the results footer, while actionable scan warnings remain visible. The empty state explains directory selection and copy direction. Read and sync errors remain visible. The UI never labels all files equal when comparison has failed.

## Responsiveness

The desktop minimum width is 900 pixels. The file list is bounded and paged, and source text scrolls within the diff panel. Reduced-motion preferences disable animations.

Directory rows use recognizable blue folder silhouettes (closed/open), folded document icons and extension cues for code, pictures and archives. The size column displays a dash for folders/links, decimal KB/MB/GB/TB for files, and exact-byte/type tooltips. Narrow ← / → columns execute a single-file copy immediately, without a preview dialog; the source remains. Native planning, revalidation, verified backups and journaling still apply. Only the affected pair is reread with SHA-256, then directory status/counts and the current query are refreshed while expansion and scrolling remain. Native application menu labels follow the interface language; the Mac bundle declares Simplified Chinese and English localization support.

Projects use a compact picker, save button and exclusion count in the application bar. The project manager opens a searchable list beside the current project's paths, name and exclusions; it does not permanently consume comparison space. It reuses the revision-aware local session library. Project switching, manager close and comparison commit named settings; unsaved edits have a header marker. An independent exclusion dialog supports adding/removing patterns and bulk editing. Right-click exclusion resolves opaque native entry IDs into escaped root-anchored patterns, commits the active profile, invalidates old comparison plans and rescans. Startup migrates legacy last-session settings once and restores the active project. Invalid patterns and revision conflicts retain drafts; opening projects does not modify compared files.
