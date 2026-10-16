# Clear Compare interface

## Scene

A developer checks project changes on a desktop monitor in a normally lit workspace, reading paths and source text before copying files. A light, restrained workspace keeps attention on the differences.

## Structure

One desktop window. A small application header, paired path inputs, rules, summary, paged results and an optional side-by-side text panel. The synchronization plan appears only after the user asks to preview selected files.

## Tokens

CSS tokens live in `crates/ui/style.css`. The accent is `oklch(.45 .086 230)`. Primary surfaces are neutral white, supporting surfaces carry a small blue chroma, and body text uses a dark neutral. Status labels combine words with color.

## Typography and controls

System sans-serif for the workspace; system monospace for file sizes and source text. Compact controls share radii, spacing, disabled states and focus outlines. Paths are truncated in lists and exposed through titles; notices wrap long paths.

## Loading and empty states

Folder comparisons publish a metadata result before optional SHA-256 verification. New sessions use metadata mode, and saved sessions retain their choice. The result basis, scan/verification stage and stop-and-keep-list affordance stay explicit. Other busy operations have a status line and cancellation affordance. The empty state explains directory selection and copy direction. Read and sync errors remain visible. The UI never labels all files equal when comparison has failed.

## Responsiveness

The desktop minimum width is 900 pixels. The file list is bounded and paged, and source text scrolls within the diff panel. Reduced-motion preferences disable animations.
