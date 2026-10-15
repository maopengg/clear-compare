//! Bounded live binary pages and streaming, overlapping byte search.
use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path},
    unique_id,
};
use clear_contracts::{
    BinaryLine, BinaryPage, BinarySearchOptions, BinarySearchResult, Entry, Kind, Side,
    binary_query,
};
use std::{
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};
pub const PAGE_BYTES: u64 = 4096;
pub const SEARCH_BYTES: u64 = 64 * 1024 * 1024;
#[derive(PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: Option<SystemTime>,
}
fn stamp(meta: &Metadata) -> Stamp {
    Stamp {
        bytes: meta.len(),
        modified: meta.modified().ok(),
    }
}
fn open(path: &Path, expected: Option<&Entry>) -> Result<(File, Stamp)> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(Error::Invalid(
            "Binary operations require regular files; links are not followed".into(),
        ));
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(Error::Invalid("Expected a regular binary file".into()));
    }
    if let Some(entry) = expected {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|t| t.as_millis().to_string());
        if entry.bytes.parse::<u64>().ok() != Some(metadata.len())
            || entry.modified_ms.is_some() && entry.modified_ms != modified
        {
            return Err(Error::Invalid(
                "Binary source metadata changed. Compare folders again.".into(),
            ));
        }
    }
    Ok((file, stamp(&metadata)))
}
fn stable(file: &File, before: &Stamp) -> Result<()> {
    if stamp(&file.metadata()?) != *before {
        return Err(Error::Invalid("Binary file changed during the read".into()));
    }
    Ok(())
}
fn read(path: &Path, entry: Option<&Entry>, offset: u64) -> Result<(Vec<u8>, u64)> {
    let (mut file, before) = open(path, entry)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    (&mut file).take(PAGE_BYTES).read_to_end(&mut bytes)?;
    stable(&file, &before)?;
    Ok((bytes, before.bytes))
}
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if (32..=126).contains(b) {
                char::from(*b)
            } else {
                '.'
            }
        })
        .collect()
}
fn page_data(
    path: String,
    offset: u64,
    left: Vec<u8>,
    right: Vec<u8>,
    left_size: u64,
    right_size: u64,
    available: [bool; 2],
) -> Result<BinaryPage> {
    let mut lines = Vec::new();
    for start in (0..left.len().max(right.len())).step_by(16) {
        let a = &left[start.min(left.len())..(start + 16).min(left.len())];
        let b = &right[start.min(right.len())..(start + 16).min(right.len())];
        lines.push(BinaryLine {
            offset: offset
                .checked_add(start as u64)
                .ok_or_else(|| Error::Invalid("Offset overflow".into()))?
                .to_string(),
            left_changed: a
                .iter()
                .enumerate()
                .filter_map(|(i, v)| (b.get(i) != Some(v)).then_some(i))
                .collect(),
            right_changed: b
                .iter()
                .enumerate()
                .filter_map(|(i, v)| (a.get(i) != Some(v)).then_some(i))
                .collect(),
            left_hex: hex(a),
            right_hex: hex(b),
            left_ascii: ascii(a),
            right_ascii: ascii(b),
            changed: a != b,
        });
    }
    Ok(BinaryPage {
        id: unique_id(),
        path,
        offset: offset.to_string(),
        left_size: left_size.to_string(),
        right_size: right_size.to_string(),
        left_available: available[0],
        right_available: available[1],
        lines,
    })
}
fn page_offset(text: &str) -> Result<u64> {
    let offset = binary_query::offset(text).map_err(Error::Invalid)?;
    if offset % PAGE_BYTES != 0 {
        return Err(Error::Invalid(
            "Binary page offset must be a 4096-byte boundary".into(),
        ));
    }
    Ok(offset)
}
pub fn page(snapshot: &Snapshot, row_id: &str, offset: &str) -> Result<BinaryPage> {
    let offset = page_offset(offset)?;
    let relative = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let side = |root: &Path, entry: Option<&Entry>| -> Result<(Vec<u8>, u64)> {
        match entry {
            None => Ok((Vec::new(), 0)),
            Some(e) if e.kind == Kind::File && e.error.is_none() => {
                read(&checked_path(root, relative, false)?, Some(e), offset)
            }
            _ => Err(Error::Invalid(
                "Binary viewer requires readable regular files".into(),
            )),
        }
    };
    let (left, l) = side(&snapshot.left, row.left.as_ref())?;
    let (right, r) = side(&snapshot.right, row.right.as_ref())?;
    page_data(
        row.path.clone(),
        offset,
        left,
        right,
        l,
        r,
        [row.left.is_some(), row.right.is_some()],
    )
}
pub fn page_files(left: &Path, right: &Path, offset: &str) -> Result<BinaryPage> {
    let offset = page_offset(offset)?;
    let (a, l) = read(left, None, offset)?;
    let (b, r) = read(right, None, offset)?;
    page_data(
        format!("{} ↔ {}", left.display(), right.display()),
        offset,
        a,
        b,
        l,
        r,
        [true, true],
    )
}
struct Query {
    pattern: Vec<u8>,
    prefix: Vec<usize>,
    start: u64,
    limit: usize,
}
impl Query {
    fn new(options: &BinarySearchOptions) -> Result<Self> {
        let pattern = binary_query::pattern(options).map_err(Error::Invalid)?;
        let start = binary_query::offset(&options.offset).map_err(Error::Invalid)?;
        let mut prefix = vec![0; pattern.len()];
        let mut matched = 0;
        for i in 1..pattern.len() {
            while matched > 0 && pattern[matched] != pattern[i] {
                matched = prefix[matched - 1];
            }
            if pattern[matched] == pattern[i] {
                matched += 1;
            }
            prefix[i] = matched;
        }
        Ok(Self {
            pattern,
            prefix,
            start,
            limit: options.limit,
        })
    }
}
fn scan(
    reader: &mut (impl Read + Seek),
    size: u64,
    q: &Query,
    window: u64,
    cancel: &AtomicBool,
) -> Result<BinarySearchResult> {
    check_cancel(cancel)?;
    if q.start > size {
        return Err(Error::Invalid(
            "Search offset is beyond the file size".into(),
        ));
    }
    let possible = size
        .checked_sub(q.pattern.len() as u64)
        .map(|n| n + 1)
        .unwrap_or_default();
    let end = possible.min(q.start.saturating_add(window)).max(q.start);
    let read_end = end.saturating_add(q.pattern.len() as u64 - 1).min(size);
    let mut result = BinarySearchResult {
        path: String::new(),
        offset: q.start.to_string(),
        searched_until: end.to_string(),
        next_offset: (end < possible).then(|| end.to_string()),
        size: size.to_string(),
        positions: Vec::new(),
        pattern_bytes: q.pattern.len(),
    };
    if q.start >= possible {
        return Ok(result);
    }
    reader.seek(SeekFrom::Start(q.start))?;
    let mut cursor = q.start;
    let mut matched = 0;
    let mut buffer = [0u8; 65536];
    while cursor < read_end {
        check_cancel(cancel)?;
        let length = (read_end - cursor).min(buffer.len() as u64) as usize;
        let n = reader.read(&mut buffer[..length])?;
        if n == 0 {
            return Err(Error::Invalid(
                "Binary file ended before its observed size".into(),
            ));
        }
        for byte in &buffer[..n] {
            while matched > 0 && q.pattern[matched] != *byte {
                matched = q.prefix[matched - 1];
            }
            if q.pattern[matched] == *byte {
                matched += 1;
            }
            cursor += 1;
            if matched == q.pattern.len() {
                let position = cursor - q.pattern.len() as u64;
                if position < end {
                    result.positions.push(position.to_string());
                    if result.positions.len() == q.limit {
                        let next = position + 1;
                        result.searched_until = next.to_string();
                        result.next_offset = (next < possible).then(|| next.to_string());
                        check_cancel(cancel)?;
                        return Ok(result);
                    }
                }
                matched = q.prefix[matched - 1];
            }
        }
    }
    check_cancel(cancel)?;
    Ok(result)
}
fn search_path(
    path: &Path,
    expected: Option<&Entry>,
    label: String,
    options: &BinarySearchOptions,
    cancel: &AtomicBool,
) -> Result<BinarySearchResult> {
    check_cancel(cancel)?;
    let query = Query::new(options)?;
    let (mut file, before) = open(path, expected)?;
    let mut result = scan(&mut file, before.bytes, &query, SEARCH_BYTES, cancel)?;
    stable(&file, &before)?;
    check_cancel(cancel)?;
    result.path = label;
    Ok(result)
}
pub fn search_file(
    path: &Path,
    options: &BinarySearchOptions,
    cancel: &AtomicBool,
) -> Result<BinarySearchResult> {
    search_path(path, None, path.display().to_string(), options, cancel)
}
pub fn search(
    snapshot: &Snapshot,
    row_id: &str,
    options: &BinarySearchOptions,
    cancel: &AtomicBool,
) -> Result<BinarySearchResult> {
    let relative = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown binary entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown binary row".into()))?;
    let (root, entry) = match options.side {
        Side::Left => (&snapshot.left, row.left.as_ref()),
        Side::Right => (&snapshot.right, row.right.as_ref()),
    };
    let entry = entry
        .filter(|e| e.kind == Kind::File && e.error.is_none())
        .ok_or_else(|| Error::Invalid("Choose an existing readable regular file side".into()))?;
    search_path(
        &checked_path(root, relative, false)?,
        Some(entry),
        row.path.clone(),
        options,
        cancel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clear_contracts::BinarySearchMode;
    use std::io::Cursor;
    fn query(pattern: &str, start: u64, limit: usize) -> Query {
        Query::new(&BinarySearchOptions {
            query: pattern.into(),
            mode: BinarySearchMode::Utf8,
            offset: start.to_string(),
            limit,
            side: Side::Left,
        })
        .unwrap()
    }
    #[test]
    fn continuation_keeps_overlaps_and_window_boundary_matches() {
        let bytes = b"aaaaaxyzxyz";
        let mut reader = Cursor::new(bytes);
        let first = scan(
            &mut reader,
            bytes.len() as u64,
            &query("aaa", 0, 1),
            2,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(first.positions, ["0"]);
        assert_eq!(first.next_offset.as_deref(), Some("1"));
        let next = scan(
            &mut reader,
            bytes.len() as u64,
            &query("aaa", 1, 256),
            2,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(next.positions, ["1", "2"]);
        assert_eq!(next.next_offset.as_deref(), Some("3"));
        let first = scan(
            &mut reader,
            bytes.len() as u64,
            &query("xyz", 0, 256),
            6,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(first.positions, ["5"]);
        assert_eq!(first.next_offset.as_deref(), Some("6"));
        let next = scan(
            &mut reader,
            bytes.len() as u64,
            &query("xyz", 6, 256),
            6,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(next.positions, ["8"]);
        assert!(next.next_offset.is_none());
    }
    #[test]
    fn chunk_boundary_eof_cancellation_and_short_reads() {
        let mut bytes = vec![b'.'; 65535];
        bytes.extend_from_slice(b"needle!");
        let mut reader = Cursor::new(&bytes);
        let r = scan(
            &mut reader,
            bytes.len() as u64,
            &query("needle!", 0, 256),
            SEARCH_BYTES,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(r.positions, ["65535"]);
        assert!(r.next_offset.is_none());
        assert!(
            scan(
                &mut reader,
                bytes.len() as u64,
                &query("a", 0, 256),
                SEARCH_BYTES,
                &AtomicBool::new(true)
            )
            .is_err()
        );
        assert!(
            scan(
                &mut reader,
                bytes.len() as u64,
                &query("a", bytes.len() as u64 + 1, 256),
                SEARCH_BYTES,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let end = scan(
            &mut reader,
            bytes.len() as u64,
            &query("a", bytes.len() as u64, 256),
            SEARCH_BYTES,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(end.positions.is_empty());
        assert!(end.next_offset.is_none());
        assert!(
            scan(
                &mut Cursor::new(b"a"),
                10,
                &query("aaa", 0, 256),
                SEARCH_BYTES,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    #[test]
    fn paginated_search_matches_naive_search_for_all_small_binary_inputs() {
        let cancel = AtomicBool::new(false);
        for length in 0..=8 {
            for bits in 0..1u16 << length {
                let bytes = (0..length)
                    .map(|i| if bits & (1 << i) == 0 { b'a' } else { b'b' })
                    .collect::<Vec<_>>();
                for pattern in ["a", "b", "aa", "aba", "bbb"] {
                    let expected = bytes
                        .windows(pattern.len())
                        .enumerate()
                        .filter_map(|(i, w)| (w == pattern.as_bytes()).then(|| i.to_string()))
                        .collect::<Vec<_>>();
                    for window in [1, 3, 9] {
                        for limit in [1, 2, 256] {
                            let mut offset = 0;
                            let mut actual = Vec::new();
                            loop {
                                let r = scan(
                                    &mut Cursor::new(&bytes),
                                    bytes.len() as u64,
                                    &query(pattern, offset, limit),
                                    window,
                                    &cancel,
                                )
                                .unwrap();
                                actual.extend(r.positions);
                                match r.next_offset {
                                    Some(next) => {
                                        let n = next.parse().unwrap();
                                        assert!(n > offset);
                                        offset = n;
                                    }
                                    None => break,
                                }
                            }
                            assert_eq!(actual, expected, "{bytes:?}/{pattern}/{window}/{limit}");
                        }
                    }
                }
            }
        }
    }
}
