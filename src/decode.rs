/// Port from https://github.com/getsentry/rust-sourcemap/blob/9.1.0/src/decoder.rs
/// It is a helper for decoding VLQ sourcemap strings to `SourceMap`.
use std::borrow::Cow;

use crate::error::{Error, Result};
use crate::token::INVALID_ID;
use crate::{SourceMap, Token};

/// See <https://github.com/tc39/source-map/blob/1930e58ffabefe54038f7455759042c6e3dd590e/source-map-rev3.md>.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JSONSourceMap {
    /// The version field, must be 3.
    #[serde(deserialize_with = "deserialize_version")]
    pub version: u32,
    /// An optional name of the generated code that this source map is associated with.
    pub file: Option<String>,
    /// A string with the encoded mapping data.
    pub mappings: String,
    /// An optional source root, useful for relocating source files on a server or removing repeated values in the "sources" entry.
    /// This value is prepended to the individual entries in the "source" field.
    pub source_root: Option<String>,
    /// A list of original sources used by the "mappings" entry.
    pub sources: Vec<String>,
    /// An optional list of source content, useful when the "source" can't be hosted.
    /// The contents are listed in the same order as the sources in line 5. "null" may be used if some original sources should be retrieved by name.
    pub sources_content: Option<Vec<Option<String>>>,
    /// A list of symbol names used by the "mappings" entry.
    #[serde(default)]
    pub names: Vec<String>,
    /// An optional field containing the debugId for this sourcemap.
    pub debug_id: Option<String>,
    /// Identifies third-party sources (such as framework code or bundler-generated code), allowing developers to avoid code that they don't want to see or step through, without having to configure this beforehand.
    /// The `x_google_ignoreList` field refers to the `sources` array, and lists the indices of all the known third-party sources in that source map.
    /// When parsing the source map, developer tools can use this to determine sections of the code that the browser loads and runs that could be automatically ignore-listed.
    #[serde(rename = "x_google_ignoreList", alias = "ignoreList")]
    pub x_google_ignore_list: Option<Vec<u32>>,
}

fn deserialize_version<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let version = u32::deserialize(deserializer)?;
    if version != 3 {
        return Err(serde::de::Error::custom(format!("unsupported source map version: {version}")));
    }
    Ok(version)
}

pub fn decode(json: JSONSourceMap) -> Result<SourceMap<'static>> {
    validate_x_google_ignore_list(json.x_google_ignore_list.as_deref(), json.sources.len())?;

    let tokens = decode_mapping(&json.mappings, json.names.len(), json.sources.len())?;
    Ok(SourceMap {
        file: json.file.map(Cow::Owned),
        names: json.names.into_iter().map(Cow::Owned).collect(),
        source_root: json.source_root.map(Cow::Owned),
        sources: json.sources.into_iter().map(Cow::Owned).collect(),
        source_contents: json
            .sources_content
            .map(|content| content.into_iter().map(|c| c.map(Cow::Owned)).collect())
            .unwrap_or_default(),
        tokens: tokens.into_boxed_slice(),
        token_chunks: None,
        x_google_ignore_list: json.x_google_ignore_list,
        debug_id: json.debug_id.map(Cow::Owned),
    })
}

/// Private deserialization shape that borrows directly from the JSON input
/// when no escapes are present. The lifetime `'a` is the input buffer's
/// lifetime: each `Cow::Borrowed` is a zero-copy slice into that buffer, and
/// only escaped strings (which serde_json must unescape into a fresh `String`)
/// land in `Cow::Owned`.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BorrowedJSONSourceMap<'a> {
    #[serde(deserialize_with = "deserialize_version")]
    #[expect(dead_code)]
    version: u32,
    #[serde(borrow)]
    file: Option<Cow<'a, str>>,
    #[serde(borrow)]
    mappings: Cow<'a, str>,
    #[serde(borrow)]
    source_root: Option<Cow<'a, str>>,
    #[serde(borrow)]
    sources: Vec<Cow<'a, str>>,
    #[serde(borrow)]
    sources_content: Option<Vec<Option<Cow<'a, str>>>>,
    #[serde(borrow, default)]
    names: Vec<Cow<'a, str>>,
    #[serde(borrow)]
    debug_id: Option<Cow<'a, str>>,
    #[serde(rename = "x_google_ignoreList", alias = "ignoreList")]
    x_google_ignore_list: Option<Vec<u32>>,
}

pub fn decode_from_string(value: &str) -> Result<SourceMap<'_>> {
    let json: BorrowedJSONSourceMap<'_> = serde_json::from_str(value)?;

    validate_x_google_ignore_list(json.x_google_ignore_list.as_deref(), json.sources.len())?;

    let tokens = decode_mapping(&json.mappings, json.names.len(), json.sources.len())?;

    Ok(SourceMap {
        file: json.file,
        names: json.names,
        source_root: json.source_root,
        sources: json.sources,
        source_contents: json.sources_content.unwrap_or_default(),
        tokens: tokens.into_boxed_slice(),
        token_chunks: None,
        x_google_ignore_list: json.x_google_ignore_list,
        debug_id: json.debug_id,
    })
}

fn validate_x_google_ignore_list(ignore_list: Option<&[u32]>, sources_len: usize) -> Result<()> {
    if let Some(ignore_list) = ignore_list {
        for &idx in ignore_list {
            if idx as usize >= sources_len {
                return Err(Error::BadSourceReference(idx));
            }
        }
    }
    Ok(())
}

fn decode_mapping(mapping: &str, names_len: usize, sources_len: usize) -> Result<Vec<Token>> {
    let mapping = mapping.as_bytes();

    let mut tokens: Vec<Token> = Vec::with_capacity(estimate_token_capacity(mapping));

    let mut dst_line = 0u32;
    let mut dst_col = 0u32;
    let mut src_id = 0;
    let mut src_line = 0;
    let mut src_col = 0;
    let mut name_id = 0;

    // Source map segments are delta-encoded and interpreted relative to previous values.
    //
    // Segment arity:
    // * 1 field: generated column
    // * 4 fields: generated column, source id, source line, source column
    // * 5 fields: 4-field segment + name id
    //
    // Delimiters:
    // * `,` separates segments on the same generated line
    // * `;` advances generated line and resets generated column
    let mut cursor = 0usize;
    let mut nums = [0i64; 5];
    while cursor < mapping.len() {
        match mapping[cursor] {
            b',' => {
                // Empty segment, skip.
                cursor += 1;
            }
            b';' => {
                // New destination line. Destination columns are line-relative.
                dst_line = dst_line.wrapping_add(1);
                dst_col = 0;
                cursor += 1;
            }
            _ => {
                let nums_len = parse_vlq_segment_into(mapping, &mut cursor, &mut nums)?;

                // `nums[0]` is always generated column delta.
                let new_dst_col = i64::from(dst_col) + nums[0];
                if new_dst_col < 0 {
                    return Err(Error::BadSegmentSize(0)); // Negative column
                }
                dst_col = new_dst_col as u32;

                let mut src = INVALID_ID;
                let mut name = INVALID_ID;

                if nums_len > 1 {
                    if nums_len != 4 && nums_len != 5 {
                        return Err(Error::BadSegmentSize(nums_len as u32));
                    }

                    // Source/name fields are also delta-encoded.
                    let new_src_id = i64::from(src_id) + nums[1];
                    if new_src_id < 0 || new_src_id >= sources_len as i64 {
                        return Err(Error::BadSourceReference(src_id));
                    }
                    src_id = new_src_id as u32;
                    src = src_id;

                    let new_src_line = i64::from(src_line) + nums[2];
                    if new_src_line < 0 {
                        return Err(Error::BadSegmentSize(0)); // Negative line
                    }
                    src_line = new_src_line as u32;

                    let new_src_col = i64::from(src_col) + nums[3];
                    if new_src_col < 0 {
                        return Err(Error::BadSegmentSize(0)); // Negative column
                    }
                    src_col = new_src_col as u32;

                    if nums_len > 4 {
                        name_id = (i64::from(name_id) + nums[4]) as u32;
                        if name_id >= names_len as u32 {
                            return Err(Error::BadNameReference(name_id));
                        }
                        name = name_id;
                    }
                }

                tokens.push(Token::new(
                    dst_line,
                    dst_col,
                    src_line,
                    src_col,
                    if src == INVALID_ID { None } else { Some(src) },
                    if name == INVALID_ID { None } else { Some(name) },
                ));
            }
        }
    }

    Ok(tokens)
}

// Align B64 lookup table on 64-byte boundary for better cache performance
#[repr(align(64))]
struct Aligned64([i8; 256]);

/// VLQ decode table. Entry semantics:
/// * `-1` — segment (`,`) or line (`;`) delimiter, terminates a value scan.
/// * `0..=63` — base64 payload; bit 5 (value >= 32) is the VLQ continuation flag.
///
/// Bytes outside the base64 alphabet decode as `63`, i.e. exactly like `'/'`
/// (payload 31 with the continuation bit set). This preserves the historical
/// lenient behavior: invalid characters are consumed as continuation bytes and
/// surface as `VlqLeftover`/`VlqOverflow` or garbage values, never a panic.
static B64_DECODE: Aligned64 = build_b64_decode();

const fn build_b64_decode() -> Aligned64 {
    let mut table = [63i8; 256];
    let mut i = 0;
    while i < 26 {
        table[(b'A' + i) as usize] = i as i8;
        table[(b'a' + i) as usize] = 26 + i as i8;
        i += 1;
    }
    let mut i = 0;
    while i < 10 {
        table[(b'0' + i) as usize] = 52 + i as i8;
        i += 1;
    }
    table[b'+' as usize] = 62;
    table[b'/' as usize] = 63;
    table[b',' as usize] = -1;
    table[b';' as usize] = -1;
    Aligned64(table)
}

/// Pick a `Vec<Token>` capacity for `decode_mapping`.
///
/// Two regimes:
/// * Small mappings (< 256 bytes) — exact `,`/`;` count. The scan itself is
///   cheap on a few hundred bytes, and getting capacity *exact* lets the
///   trailing `Vec::into_boxed_slice` in `decode_from_string` skip its
///   shrink-realloc. That realloc dominates the per-iteration cost on tiny
///   benchmarks (32-byte fixtures), so an over-estimate would regress them.
/// * Larger mappings — `len / 4 + 1`. The scan would itself become the
///   dominant cost (~15-20 µs on the xlarge perf fixture). The heuristic is
///   close to typical sourcemap density (~4-5 bytes per segment); `Vec::push`
///   handles under-estimates via geometric growth, and the trailing
///   `into_boxed_slice` shrinks any over-allocation back to exact size.
fn estimate_token_capacity(mapping: &[u8]) -> usize {
    const EXACT_SCAN_THRESHOLD: usize = 256;
    if mapping.len() < EXACT_SCAN_THRESHOLD {
        let mut n = 1usize;
        for &b in mapping {
            if b == b',' || b == b';' {
                n += 1;
            }
        }
        n
    } else {
        mapping.len() / 4 + 1
    }
}

/// Parse one VLQ segment at `cursor` and stop at `,` / `;` / end-of-input.
///
/// Returns the number of decoded values in the segment. Values are written into
/// `rv` for the first 5 fields (the maximum valid segment size). If the segment
/// contains more than 5 fields, we keep parsing and counting so the caller can
/// reject it with `BadSegmentSize` carrying the true field count.
fn parse_vlq_segment_into(mapping: &[u8], cursor: &mut usize, rv: &mut [i64; 5]) -> Result<usize> {
    let mut rv_len = 0usize;
    while let Some(value) = next_vlq(mapping, cursor)? {
        if rv_len < rv.len() {
            rv[rv_len] = value;
        }
        rv_len += 1;
    }
    if rv_len == 0 {
        return Err(Error::VlqNoValues);
    }
    Ok(rv_len)
}

/// Decode one VLQ value starting at `*cursor`.
///
/// Returns `Ok(None)` without consuming anything when positioned at a
/// delimiter (`,` / `;`) or end of input; otherwise consumes the value's bytes.
#[inline(always)]
fn next_vlq(mapping: &[u8], cursor: &mut usize) -> Result<Option<i64>> {
    let Some(&byte) = mapping.get(*cursor) else { return Ok(None) };
    let first = i64::from(B64_DECODE.0[byte as usize]);
    if first < 0 {
        return Ok(None);
    }
    *cursor += 1;
    if first < 32 {
        // No continuation bit: a single-byte value, the dominant case in real
        // mappings (small line/column deltas).
        return Ok(Some(decode_sign(first)));
    }

    let mut cur = first & 0b11111;
    let mut shift = 5u32;
    loop {
        let Some(&byte) = mapping.get(*cursor) else {
            // Input ended while a continuation was pending.
            return Err(Error::VlqLeftover);
        };
        let enc = i64::from(B64_DECODE.0[byte as usize]);
        if enc < 0 {
            // Delimiter while a continuation was pending.
            return Err(Error::VlqLeftover);
        }
        *cursor += 1;
        // VLQ shift grows by 5 bits per continuation byte. Bail out before
        // `payload << shift` could overflow i64: the largest safe shift for a
        // 5-bit value is 62, and `shift` only ever takes multiples of 5, so
        // the first offending shift is 65.
        if shift > 62 {
            return Err(Error::VlqOverflow);
        }
        cur |= (enc & 0b11111) << shift;
        shift += 5;
        if enc < 32 {
            return Ok(Some(decode_sign(cur)));
        }
    }
}

/// VLQ stores the sign in the low bit; remaining bits are the magnitude.
/// Branchless sign-magnitude decode: `-0` collapses to `0`.
#[inline(always)]
fn decode_sign(cur: i64) -> i64 {
    let mag = cur >> 1;
    let sign = cur & 1;
    (mag ^ -sign) + sign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_sourcemap() {
        let input = r#"{
            "version": 3,
            "sources": ["coolstuff.js"],
            "sourceRoot": "x",
            "names": ["x","alert"],
            "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
            "x_google_ignoreList": [0]
        }"#;
        let sm = SourceMap::from_json_string(input).unwrap();
        assert_eq!(sm.get_source_root(), Some("x"));
        assert_eq!(sm.get_x_google_ignore_list(), Some(&[0][..]));
        let mut iter = sm.get_source_view_tokens().filter(|token| token.get_name_id().is_some());
        assert_eq!(iter.next().unwrap().to_tuple(), (Some("coolstuff.js"), 0, 4, Some("x")));
        assert_eq!(iter.next().unwrap().to_tuple(), (Some("coolstuff.js"), 1, 4, Some("x")));
        assert_eq!(iter.next().unwrap().to_tuple(), (Some("coolstuff.js"), 2, 2, Some("alert")));
        assert!(iter.next().is_none());
    }

    #[test]
    fn decode_from_json_value() {
        // `SourceMap::from_json` / `decode` consumes an owned `JSONSourceMap`,
        // a separate path from the borrowed `from_json_string` / `decode_from_string`.
        let json = SourceMap::from_json_string(
            r#"{
                "version": 3,
                "file": "f.js",
                "sourceRoot": "r",
                "names": ["n"],
                "sources": ["a.js"],
                "sourcesContent": ["c"],
                "mappings": "AAAAA",
                "debugId": "d",
                "x_google_ignoreList": [0]
            }"#,
        )
        .unwrap()
        .to_json();
        let sm = SourceMap::from_json(json).unwrap();
        assert_eq!(sm.get_file(), Some("f.js"));
        assert_eq!(sm.get_source_root(), Some("r"));
        assert_eq!(sm.get_debug_id(), Some("d"));
        assert_eq!(sm.get_x_google_ignore_list(), Some(&[0][..]));
        assert_eq!(sm.get_source_content(0), Some("c"));
        assert_eq!(sm.get_name(0), Some("n"));
    }

    #[test]
    fn decode_sourcemap_optional_field() {
        let input = r#"{
            "version": 3,
            "names": [],
            "sources": [],
            "sourcesContent": [null],
            "mappings": ""
        }"#;
        SourceMap::from_json_string(input).expect("should success");
    }

    #[test]
    fn decode_unsupported_version() {
        let input = r#"{"version": 2, "names": [], "sources": [], "mappings": ""}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadJson(_)));
    }

    #[test]
    fn decode_mapping_bad_segment_size() {
        let input = r#"{"version":3,"names":[],"sources":[],"sourcesContent":[],"mappings":"AA"}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadSegmentSize(2)));
    }

    #[test]
    fn decode_mapping_vlq_leftover() {
        let input = r#"{"version":3,"names":[],"sources":[],"sourcesContent":[],"mappings":"g"}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::VlqLeftover));
    }

    #[test]
    fn decode_mapping_vlq_overflow() {
        // A long run of continuation bytes overflows the VLQ shift accumulator.
        let mappings = "g".repeat(14);
        let input =
            format!(r#"{{"version":3,"names":[],"sources":["a.js"],"mappings":"{mappings}"}}"#);
        let err = SourceMap::from_json_string(&input).unwrap_err();
        assert!(matches!(err, Error::VlqOverflow));
    }

    #[test]
    fn decode_mapping_bad_source_reference() {
        // 4-field segment references source id 0, but there are no sources.
        let input = r#"{"version":3,"names":[],"sources":[],"mappings":"AAAA"}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadSourceReference(_)));
    }

    #[test]
    fn decode_mapping_bad_name_reference() {
        // 5-field segment references name id 0, but there are no names.
        let input = r#"{"version":3,"names":[],"sources":["a.js"],"mappings":"AAAAA"}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadNameReference(0)));
    }

    #[test]
    fn decode_ignore_list_bad_source_reference() {
        // `x_google_ignoreList` references a source index that does not exist.
        let input =
            r#"{"version":3,"names":[],"sources":[],"mappings":"","x_google_ignoreList":[3]}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadSourceReference(3)));
    }

    #[test]
    fn decode_owned_propagates_errors() {
        // The owned `decode` path (`SourceMap::from_json`) must surface the same
        // validation errors as the borrowed path: a bad ignore-list index...
        let bad_ignore_list = JSONSourceMap {
            version: 3,
            file: None,
            mappings: String::new(),
            source_root: None,
            sources: vec![],
            sources_content: None,
            names: vec![],
            debug_id: None,
            x_google_ignore_list: Some(vec![3]),
        };
        assert!(matches!(SourceMap::from_json(bad_ignore_list), Err(Error::BadSourceReference(3))));

        // ...and a mapping that references a source with no sources declared.
        let bad_mapping = JSONSourceMap {
            version: 3,
            file: None,
            mappings: "AAAA".to_string(),
            source_root: None,
            sources: vec![],
            sources_content: None,
            names: vec![],
            debug_id: None,
            x_google_ignore_list: None,
        };
        assert!(matches!(SourceMap::from_json(bad_mapping), Err(Error::BadSourceReference(_))));
    }

    #[test]
    fn decode_invalid_char_behaves_like_slash() {
        // Bytes outside the base64 alphabet decode as payload 31 with the
        // continuation bit set — exactly like '/'. A map using '!' must
        // therefore produce the same tokens as the same map using '/'.
        let make = |mappings: &str| {
            format!(r#"{{"version":3,"names":[],"sources":[],"mappings":"{mappings}"}}"#)
        };
        let bang_json = make("gD,!C");
        let slash_json = make("gD,/C");
        let bang = SourceMap::from_json_string(&bang_json).unwrap();
        let slash = SourceMap::from_json_string(&slash_json).unwrap();
        let tokens: Vec<Token> = bang.get_tokens().collect();
        assert_eq!(tokens, slash.get_tokens().collect::<Vec<Token>>());
        assert_eq!(tokens[0].get_dst_col(), 48);
        assert_eq!(tokens[1].get_dst_col(), 1);
    }

    #[test]
    fn decode_dangling_continuation_before_delimiter_is_leftover() {
        // 13 continuation bytes stay under the shift-overflow bound, so ending
        // the segment there must surface VlqLeftover, not VlqOverflow.
        let mappings = format!("{},A", "g".repeat(13));
        let input = format!(r#"{{"version":3,"names":[],"sources":[],"mappings":"{mappings}"}}"#);
        let err = SourceMap::from_json_string(&input).unwrap_err();
        assert!(matches!(err, Error::VlqLeftover));
    }

    #[test]
    fn decode_oversized_segments_report_exact_field_count() {
        for (mappings, expected) in [("AAAAAA", 6u32), ("AAAAAAA", 7u32)] {
            let input =
                format!(r#"{{"version":3,"names":[],"sources":[],"mappings":"{mappings}"}}"#);
            let err = SourceMap::from_json_string(&input).unwrap_err();
            assert!(matches!(err, Error::BadSegmentSize(n) if n == expected));
        }
    }

    #[test]
    fn decode_parse_error_beats_segment_validation() {
        // The whole segment is VLQ-parsed before any field validation, so a
        // dangling continuation reports VlqLeftover even when the decoded
        // fields would also fail the column check ("Dg": delta -1) or the
        // segment-size check ("AAAAAg": 6 fields).
        for mappings in ["Dg", "AAAAAg"] {
            let input =
                format!(r#"{{"version":3,"names":[],"sources":[],"mappings":"{mappings}"}}"#);
            let err = SourceMap::from_json_string(&input).unwrap_err();
            assert!(matches!(err, Error::VlqLeftover), "mappings {mappings:?}: {err:?}");
        }
    }

    #[test]
    fn decode_negative_column_beats_size_check() {
        // "DA" decodes to two fields with a negative generated column; the
        // column check runs before the field-count check.
        let input = r#"{"version":3,"names":[],"sources":[],"mappings":"DA"}"#;
        let err = SourceMap::from_json_string(input).unwrap_err();
        assert!(matches!(err, Error::BadSegmentSize(0)));
    }

    #[test]
    fn parse_vlq_segment_empty_is_no_values() {
        // Directly exercise the defensive `VlqNoValues` branch: with the cursor
        // already at the end of the input, no values can be decoded. This state
        // is unreachable through `decode_mapping` (which only calls the parser
        // on a non-delimiter byte), so it is covered here.
        let mut cursor = 0;
        let mut out = [0i64; 5];
        let err = parse_vlq_segment_into(b"", &mut cursor, &mut out).unwrap_err();
        assert!(matches!(err, Error::VlqNoValues));
    }
}
