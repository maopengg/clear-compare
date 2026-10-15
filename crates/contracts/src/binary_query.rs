//! Byte-query validation shared by native operations and the desktop.
use crate::{BinarySearchMode, BinarySearchOptions};
pub fn offset(text: &str) -> Result<u64, String> {
    let value = text.trim();
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.map_err(|_| "Use a nonnegative decimal or 0x hexadecimal byte offset".into())
}
pub fn pattern(options: &BinarySearchOptions) -> Result<Vec<u8>, String> {
    if options.query.len() > 16384 {
        return Err("Byte query exceeds 16 KiB".into());
    }
    if !(1..=256).contains(&options.limit) {
        return Err("Search result limit must be between 1 and 256".into());
    }
    let bytes = match options.mode {
        BinarySearchMode::Hex => {
            let digits = options
                .query
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect::<Vec<_>>();
            if digits.len() % 2 != 0 {
                return Err("Hex query needs complete two-digit bytes".into());
            }
            let mut bytes = Vec::new();
            for pair in digits.as_chunks::<2>().0 {
                let a = pair[0]
                    .to_digit(16)
                    .ok_or("Hex query contains a non-hex character")?;
                let b = pair[1]
                    .to_digit(16)
                    .ok_or("Hex query contains a non-hex character")?;
                bytes.push((a * 16 + b) as u8);
            }
            bytes
        }
        BinarySearchMode::Utf8 => options.query.as_bytes().to_vec(),
        BinarySearchMode::Utf16Le => options
            .query
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
        BinarySearchMode::Utf16Be => options
            .query
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect(),
    };
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err("Search pattern must encode between 1 and 4096 bytes".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    fn options(mode: BinarySearchMode, query: &str) -> BinarySearchOptions {
        BinarySearchOptions {
            mode,
            query: query.into(),
            offset: "0".into(),
            limit: 256,
            side: Side::Left,
        }
    }
    #[test]
    fn explicit_encodings_and_strict_hex_validation() {
        assert_eq!(
            pattern(&options(BinarySearchMode::Hex, "aB 00\nFf")).unwrap(),
            [171, 0, 255]
        );
        assert_eq!(
            pattern(&options(BinarySearchMode::Utf8, "\u{1f600}")).unwrap(),
            [240, 159, 152, 128]
        );
        assert_eq!(
            pattern(&options(BinarySearchMode::Utf16Le, "\u{1f600}")).unwrap(),
            [61, 216, 0, 222]
        );
        assert_eq!(
            pattern(&options(BinarySearchMode::Utf16Be, "\u{1f600}")).unwrap(),
            [216, 61, 222, 0]
        );
        for query in ["", " ", "a", "0xAB", "GG", "aa-bb", "aa\u{a0}bb"] {
            assert!(
                pattern(&options(BinarySearchMode::Hex, query)).is_err(),
                "{query}"
            );
        }
        assert!(pattern(&options(BinarySearchMode::Utf8, &"a".repeat(4097))).is_err());
        assert_eq!(
            pattern(&options(BinarySearchMode::Utf8, &"a".repeat(4096)))
                .unwrap()
                .len(),
            4096
        );
        for limit in [0, 257, usize::MAX] {
            let mut o = options(BinarySearchMode::Utf8, "a");
            o.limit = limit;
            assert!(pattern(&o).is_err());
        }
    }
    #[test]
    fn offsets_preserve_u64_precision_and_reject_bad_input() {
        assert_eq!(offset(" 0X1000 ").unwrap(), 4096);
        assert_eq!(offset("18446744073709551615").unwrap(), u64::MAX);
        for text in ["", "-1", "0x", "xyz", "1.5", "18446744073709551616"] {
            assert!(offset(text).is_err());
        }
    }
}
