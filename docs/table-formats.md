# Table format policies

Table comparison is read-only and compares extracted string values, not source bytes or rendered-page appearance. Folder content status remains a separate result.

## Inputs and native documents

Files are bounded at 4 MiB raw bytes and decoded as UTF-8 or BOM-marked UTF-16 using the common editor decoder. HTML has an additional 1 MiB decoded limit. Other declared HTML character sets are not auto-decoded.

The desktop captures both source texts in one native document identified by an opaque ID. A verified folder comparison pins the original file digests; opening a changed file fails and requires another folder comparison. Metadata-only folder scans capture current text without claiming historical content verification. Later table option changes/listing reuse captured text. Reopening refreshes; close/new folder comparison invalidates the document. There is no persisted table workspace or editing.

Auto format uses `.html`/`.htm` for HTML, `.tsv` for TSV and otherwise CSV. Explicit per-side CSV, TSV or HTML choices override it. The legacy delimited source uses `TableOptions.delimiter`. Direct string comparison has no filename hints and retains that delimiter default for `auto`.

## HTML extraction

The parser repairs common HTML5 syntax such as omitted cell/row endings and decodes HTML entities. It runs as a native static parser, never as executable browser content. It does not fetch resources, evaluate scripts/CSS, resolve image values or compute form-control values. CSS visibility is not inferred; static `hidden` attributes and template contents are omitted. Script/style text is excluded across namespaces.

Catalogs list up to 64 HTML-namespace tables in normalized document order, including nested tables as independent choices. Captions and element IDs are labels; DOM row counts exclude rows synthesized by spans. Parent table extraction omits nested-table text and rows. Select indices are zero-based and stable within the captured document.

[HTML's table model](https://html.spec.whatwg.org/multipage/tables.html#processing-model-1) defines header/body/footer group ordering and cell spans. This extractor orders thead before body/direct rows and tfoot last. Covered grid slots repeat the anchor value. Rowspan zero extends through its row group, including span-implied rows; positive spans may add implied rows within that group. Colspan zero becomes one; values cap at 1000 columns and 65534 rows before application budgets. Unlike browser error recovery, malformed span integers and overlapping cells return errors rather than guessing values. A cell cannot silently overwrite a previously occupied slot.

ASCII layout whitespace, including line/block boundaries, collapses to spaces with outer layout whitespace removed. NBSP and other Unicode characters remain data. Presentation attributes/colgroup styling are not compared. The selected grid is rectangular and missing slots become empty strings; CSV trailing missing columns remain distinct from explicit empty cells. Only the first logical row is treated as headers when enabled; hierarchical header interpretation is not implemented.

## Budgets

- HTML: 1 MiB decoded text, 100,000 allocated nodes, 256 traversal levels, 64 tables; node/time/cancel checks between small parse chunks and after completion.
- Parsing has a 2-second elapsed budget checked at chunk boundaries; it is not an OS-enforced real-time limit, and allocations can temporarily overshoot within a chunk.
- Selected grid: 30,000 expanded rows, 1024 columns, 200,000 slots and 8 MiB expanded string data. Repeated span values count toward the text budget.
- Delimited inputs: 4 MiB decoded text, 30,000 logical rows, 1024 columns and 200,000 cells. Quoted line breaks are part of a cell.
- Native responses page by 500 rows. The desktop displays 25 columns per window and at most 8192 Unicode characters per cell with an explicit clipping label; the result/CLI retain full values.

A budget failure is an error, not equality. The native single-document/result caches are replaced rather than accumulated.

## Matching and CLI

Matching remains positional or by one zero-based key column. Duplicate/missing key columns are errors, including repeated keys introduced by rowspans. Numeric tolerance is optional finite, nonnegative absolute f64 tolerance; all other cells are exact strings. Header differences are reported separately and produce a CLI difference exit code. Multi-column keys, column-name mapping, Excel and table editing remain planned. Formula expressions are not evaluated.

`clear-compare table LEFT RIGHT` applies extension hints. Options include:

- `--list-tables`: list native source metadata/catalogs without comparing a selected grid. It can succeed with zero available tables; that does not establish equality.
- `--html`: select table zero on both sides; `--left-html N` / `--right-html N` override a side.
- `--delimited`: disable HTML/extension inference for delimited sources; it can be combined with a side-specific HTML choice for mixed input.
- `--tsv`: use the legacy tab delimiter for delimited sources; use `--delimited --tsv` to force tab parsing of a file named `.html`.
- `--key-column`, `--numeric-tolerance`, `--no-headers`: existing row/cell policies.

Column totals are computed over the entire result before row paging. The result records effective TableOptions, including resolved CSV/TSV/HTML formats and selected HTML indices. Comparison exit codes remain 0 equal, 1 different (including headers), 2 invalid/unreadable/unsupported input. A regular empty HTML file has no selectable table and cannot be silently treated as equal to another empty HTML input. A missing side in a folder snapshot is explicitly an empty side, not an empty existing HTML file.
