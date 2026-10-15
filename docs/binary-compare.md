# Binary comparison and byte search

The local-folder workspace opens regular files as paired hex/ASCII pages. Each page contains at most 4096 bytes per side and 16 bytes per line. Underlined hex bytes differ from the corresponding byte on the other side; bytes beyond a shorter file are differences, including a zero byte versus a missing byte. Each refreshed page has a new rendering identity. The folder's SHA-256 comparison remains the mechanism for verified whole-file equality.

## Navigation and search

The desktop accepts decimal and `0x` hexadecimal offsets without JavaScript number conversion. Go opens the containing 4096-byte page. Previous/Next moves a page. Search chooses an existing file side and one explicit byte encoding:

- Hex: complete pairs of hexadecimal digits, with optional ASCII whitespace. No `0x` prefixes, wildcards or separators other than whitespace.
- UTF-8: the exact encoded text, case-sensitive.
- UTF-16 LE/BE: exact encoded code units, including surrogate pairs; no BOM is implicitly added. To find a BOM, use hex bytes.

Queries must encode 1–4096 bytes; query input is limited to 16 KiB UTF-8. Search examines byte positions, including odd offsets for UTF-16, without Unicode normalization or character-boundary restrictions. Matches can overlap and can cross 64 KiB read blocks or 4096-byte display pages. Clicking a match opens its containing page and highlights its bytes on the searched side. A multi-page match remains highlighted when navigating to its adjacent page.

Each invocation scans up to 64 MiB of candidate start positions and returns at most 256 matches. The displayed half-open range `[offset, searched_until)` identifies examined candidate positions. A pattern-length lookahead permits matches that cross the scan boundary. Continue search uses `next_offset`; reaching the result limit resumes one byte after the last match, preserving overlaps. Each continuation replaces the displayed batch. Changing the query, encoding, side or initial offset clears results and selected-match highlighting. Search results are scoped to the comparison and entry.

An empty batch can still have `next_offset`: it means no match in that batch, with more positions remaining. A null cursor means there are no remaining candidate starts for that pattern. Starting at EOF or in the tail shorter than the pattern produces no matches; an offset beyond EOF is rejected.

## Command line

```sh
# Both successful equal and different pages exit 0; inspect per-byte changes.
clear-compare hex ./old.bin ./new.bin --offset 0x1000

# JSON result, including string offsets and an optional continuation cursor.
clear-compare binary-search ./file.bin 'DE AD BE EF' --mode hex --offset 0 --limit 256
clear-compare binary-search ./file.bin 'text' --mode utf8
clear-compare binary-search ./file.bin 'text' --mode utf16-le
```

`hex` requires a 4096-byte boundary and exits 0 after a successful page read; empty pages beyond EOF are allowed. This exit code makes no whole-file equality claim. `binary-search` exits 0 if its batch contains any matches, 1 if that batch has no matches, and 2 on errors. Always inspect `next_offset` before treating a batch as a complete search. Offsets and sizes are decimal strings in JSON to preserve 64-bit precision.

## Live-read consistency and limits

Binary pages and searches read the live files. Before reading, native operations reject symbolic links/nonregular files. Folder-workspace operations additionally check the observed size and modification time against the cached folder entry. After each bounded read/search, size and full modification time are checked again. An observed change rejects the result and asks for another comparison. Cancellation is checked between reads; no filesystem writes occur.

These metadata checks are not an immutable snapshot or a whole-file digest check. Concurrent edits preserving size/time, changes between invocations, and adversarial filesystem races can escape detection. Search results describe the bytes read during that invocation; a continuation does not pin the file version from the preceding batch. The two sides of a page are read sequentially. The interface does not infer whole-file equality from a page, a search or metadata checks. Search is supported for local folder entries and direct CLI file paths; archive/Git/snapshot byte viewers are still planned.

Hex editing, replacement and inserted-byte alignment remain planned. Final packaged-client interaction and installation checks remain deferred until feature development is complete.
