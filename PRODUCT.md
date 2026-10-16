# Product

## Register

product

## Platform

web

The interface is a desktop WebView, packaged for macOS, Windows and Linux.

## Users

Developers managing multiple local Git repositories, inspecting and editing project files, and comparing or synchronizing local project directories. The initial acceptance scenario is selective code synchronization with directory and filename exclusions.

## Product Purpose

An open-source multi-project desktop workbench combining visual Git, file comparison and merge, and basic code editing. The full Git and editor scope is under development, tracked in docs/workbench-acceptance.md. Remote filesystem and cloud storage connections are outside the product scope.

## Positioning

Inspect differences and synchronize local files. Batch copies, deletes and renames use a reviewed plan. Explicit per-file direction arrows copy immediately, with native validation, verified backups and a journal.

## Brand Personality

Simple, modern desktop interface. Chinese by default, with an English switch.

## Design Principles

- Make source, destination and operation direction explicit.
- Keep content equality and incomplete verification visibly distinct.
- Review batch and destructive operations before execution; make immediate single-file direction arrows explicit.
- Put comparison results and readable text differences at the center of the workspace.

## Accessibility and Inclusion

Use semantic controls, visible focus states and textual state labels. Implement keyboard and reduced-motion support; do not claim a formal accessibility conformance level before testing.
