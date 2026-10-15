# Table format policies

Table comparison is read-only and compares extracted string values, not source bytes or rendered-page appearance. Folder content status remains a separate result.

## Inputs and native documents

Files are bounded at 4 MiB raw bytes. Text inputs use the UTF-8/BOM-marked UTF-16 decoder; OOXML workbooks remain binary. HTML has an additional 1 MiB decoded limit. Other declared HTML character sets are not auto-decoded.

The desktop captures both source byte buffers in one native document identified by an opaque ID. A verified folder comparison pins the original file digests; opening a changed file fails and requires another folder comparison. Metadata-only folder scans capture current bytes without claiming historical content verification. Later table option changes/listing reuse captured bytes. Reopening refreshes; close/new folder comparison invalidates the document. There is no persisted table workspace or editing.

Auto format recognizes `.xlsx`/`.xlsm`/`.xltx`/`.xltm` and ZIP-signature inputs as OOXML workbooks, `.html`/`.htm` for HTML, `.tsv` for TSV and otherwise CSV. Explicit per-side CSV, TSV, HTML or Excel choices override it. The legacy delimited source uses `TableOptions.delimiter`. Direct string comparison has no filename hints and retains that delimiter default for `auto`.

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

Rows match by position or one zero-based key column. Duplicate/missing key columns are errors, including repeated keys introduced by rowspans. Numeric tolerance is optional finite, nonnegative absolute f64 tolerance; all other cells are exact strings. Multi-column keys, arbitrary renamed-column mappings, legacy Excel formats and table editing remain planned. Formula expressions are not evaluated.

Columns match positionally by default. Enable **Align by header name** / `--align-columns-by-name` to match exact header strings independently of physical order. This requires headers, unique nonblank names on each side, and no data row wider than its header. Names are case-sensitive, without trimming or Unicode normalization; whitespace-only names are rejected. HTML uses the already-extracted static header text. The aligned union follows left header order, then right-only names in right order, with a maximum of 1024 logical columns.

In name mode, `key_column` refers to that zero-based logical union, not the right file's physical column number. The native matcher maps it to the corresponding source index on each side. A key absent on a side with data rows is an error; an empty/missing side may have no key. Duplicate row keys are still rejected after mapping.

JSON retains original per-side row vectors and headers. `column_map` gives each logical name and optional original left/right indices; `changed_columns` uses logical indices. `columns` is the logical union length. Positional mode has an empty column_map, and indices retain positional meaning. `headers_changed` describes schema differences under the chosen mode: reordering the same unique names is equal in name mode; added/removed names remain different even with no data rows. Header differences in positional mode still compare the original arrays. CLI exit codes use this flag as well as row differences.

The UI displays both sides in logical order with numbered headers and source-column tooltips. Missing columns, missing cells in short rows and explicit empty strings remain distinct; missing values are not padded into empty strings. Cell/header display clipping does not change native matching.

`clear-compare table LEFT RIGHT` applies extension hints. Options include:

- `--list-tables`: list native source metadata/catalogs without comparing a selected grid. It can succeed with zero available tables; that does not establish equality.
- `--html`: select table zero on both sides; `--left-html N` / `--right-html N` override a side.
- `--delimited`: disable HTML/extension inference for delimited sources; it can be combined with a side-specific HTML choice for mixed input.
- `--tsv`: use the legacy tab delimiter for delimited sources; use `--delimited --tsv` to force tab parsing of a file named `.html`.
- `--align-columns-by-name`: exact named-column alignment; conflicts with `--no-headers`.
- `--key-column`, `--numeric-tolerance`, `--no-headers`: row/cell policies; name mode uses the logical key index.

Column totals are computed over the entire result before row paging. The result records effective TableOptions, including resolved CSV/TSV/HTML/OOXML formats and selected table/worksheet indices. Comparison exit codes remain 0 equal, 1 different (including headers), 2 invalid/unreadable/unsupported input. A regular empty HTML file has no selectable table and cannot be silently treated as equal to another empty HTML input. A missing side in a folder snapshot is explicitly an empty side, not an empty existing HTML file.

## Excel OOXML worksheets

XLSX/XLSM/XLTX/XLTM use the pinned calamine reader, with an application package preflight and [streaming cell metadata API](https://docs.rs/calamine/0.36.1/calamine/struct.XlsxCellReader.html). No dense library Range is created from declared dimensions. Catalogs accept at most 64 workbook sheets and expose only worksheets with zero-based indices, original names and hidden status. Hidden worksheets may be selected. Chart, macro and dialog sheets are not data worksheets. Macros, external resources, queries and formula evaluation are never executed. Legacy XLS, XLSB, encrypted workbooks and ODS remain unsupported.

Only saved literal/cached values are compared. Ordinary and shared formulas use saved cached results without shared-formula expansion. Missing caches and Excel cell errors return errors rather than equal/empty values. Caches may be stale; recalculate/save in an appropriate spreadsheet application when freshness matters. Formula-text changes with identical cached values do not produce a difference under this policy. Selected workbook sides contribute explicit cached-value/format-limit notes.

Shared/inline strings, including rich-text runs, become text; OOXML xml:space preservation applies. Booleans become TRUE/FALSE, finite numbers use their numeric value's shortest round-trip string form. Currency/percentage formatting, styles, charts, filters and merged-cell expansion are not compared. Recognized dates normalize through the workbook's 1900/1904 date system to YYYY-MM-DDTHH:MM:SS.mmm without a timezone. Elapsed-time formats use duration-days:VALUE; ISO date/duration strings remain stored strings. Nonfinite numbers and date serials outside 0 to 2,958,465 are rejected. Cell types are compared as extracted strings: text can equal a numeric/boolean cell with the same spelling. Numeric tolerance applies to finite numeric strings, not ISO dates or prefixed durations.

The selected grid is anchored at A1 through the last explicit nonempty or empty-string value. Leading/interior absent cells become empty strings in a rectangular worksheet; purely empty styled cells are skipped. Unused rows/columns are not synthesized from dimensions. Duplicate coordinates, more than 30,000 rows, 1,024 columns, 200,000 visited/expanded cells or 8 MiB extracted text are errors. Sparse cells outside these bounds are rejected before result allocation. Excel does not expose distinct missing-vs-empty source-cell provenance; missing sides and columns remain separate.

Classic ZIP central-directory bounds/counts are checked before either ZIP reader is constructed; ZIP64 end records are unsupported. Package preflight accepts at most 512 parts, 8 MiB per expanded part and 32 MiB total expanded bytes, validates decompressed lengths/CRC, rejects case-insensitive duplicate/traversal names and encrypted parts, and bounds XML to depth 64 and one million elements. XML must be UTF-8; DTDs are rejected. Worksheet relationships must target internal XML parts so allocation-hint validation cannot be bypassed through binary part names. Shared-string uniqueCount and shared-formula indices are bounded before library allocation. The reader then streams selected cells under application grid/text budgets. Native source bytes remain frozen; changing rules does not reread files. Content-verified folder opening pins the raw workbook hash.

```sh
clear-compare table ./left.xlsx ./right.csv --list-tables
clear-compare table ./left.xlsx ./right.xlsx --left-sheet 0 --right-sheet 1 --key-column 0
clear-compare table ./left.xlsx ./right.csv --left-sheet 0 --align-columns-by-name --key-column 0 --numeric-tolerance 0.01
```

Sheet arguments conflict with HTML/delimited selection for that side. Effective source options and selected-sheet notes are in comparison JSON; worksheet catalogs are in document/list responses. Existing exit codes remain zero for equal extracted values, one for differences including header structure, and two for errors. Core/CLI tests cover catalogs/selections, shared/inline/rich strings, formulas, date systems, sparse grids, cross-format alignment, frozen/digest-checked sources and resource/error boundaries. Packaged-client interaction/install acceptance remains deferred.
