/// Port from https://github.com/getsentry/rust-sourcemap/blob/9.1.0/src/decoder.rs
/// It is a helper for decode vlq soucemap string to `SourceMap`.
use std::sync::Arc;

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

pub fn decode(json: JSONSourceMap) -> Result<SourceMap> {
    // Validate x_google_ignore_list indices
    if let Some(ref ignore_list) = json.x_google_ignore_list {
        for &idx in ignore_list {
            if idx >= json.sources.len() as u32 {
                return Err(Error::BadSourceReference(idx));
            }
        }
    }

    let tokens = decode_mapping(&json.mappings, json.names.len(), json.sources.len())?;
    Ok(SourceMap {
        file: json.file.map(Arc::from),
        names: json.names.into_iter().map(Arc::from).collect(),
        source_root: json.source_root,
        sources: json.sources.into_iter().map(Arc::from).collect(),
        source_contents: json
            .sources_content
            .map(|content| content.into_iter().map(|c| c.map(Arc::from)).collect())
            .unwrap_or_default(),
        tokens: tokens.into_boxed_slice(),
        token_chunks: None,
        x_google_ignore_list: json.x_google_ignore_list,
        debug_id: json.debug_id,
    })
}

pub fn decode_from_string(value: &str) -> Result<SourceMap> {
    decode(serde_json::from_str(value)?)
}

fn decode_mapping(mapping: &str, names_len: usize, sources_len: usize) -> Result<Vec<Token>> {
    let mapping = mapping.as_bytes();

    // Upper-bound token estimate: each `,` and `;` can delimit at most one segment.
    let mut estimated_tokens = 1usize;
    for &byte in mapping {
        if byte == b',' || byte == b';' {
            estimated_tokens += 1;
        }
    }
    let mut tokens = Vec::with_capacity(estimated_tokens);

    let mut dst_line = 0u32;
    let mut dst_col = 0u32;
    let mut src_id = 0;
    let mut src_line = 0;
    let mut src_col = 0;
    let mut name_id = 0;

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

#[rustfmt::skip]
static B64: Aligned64 = Aligned64([ -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 62, -1, -1, -1, 63, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, -1, -1, -1, -1, -1, -1, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, -1, -1, -1, -1, -1, -1, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1 ]);

fn parse_vlq_segment_into(mapping: &[u8], cursor: &mut usize, rv: &mut [i64; 5]) -> Result<usize> {
    let mut cur = 0i64;
    let mut shift = 0u32;
    let mut rv_len = 0usize;

    while *cursor < mapping.len() {
        let c = mapping[*cursor];
        if c == b',' || c == b';' {
            break;
        }

        // SAFETY: B64 is a 256-element lookup table, and c is a u8 (0-255)
        let enc = unsafe { i64::from(*B64.0.get_unchecked(c as usize)) };
        let val = enc & 0b11111;
        let cont = enc >> 5;

        // Check if shift would overflow before applying
        if shift >= 64 {
            return Err(Error::VlqOverflow);
        }

        // For large shifts, check if the value would fit in 32 bits when decoded
        if shift <= 62 {
            cur = cur.wrapping_add(val << shift);
        } else {
            // Beyond 62 bits of shift, we'd overflow i64
            return Err(Error::VlqOverflow);
        }

        *cursor += 1;
        shift += 5;

        if cont == 0 {
            let sign = cur & 1;
            cur >>= 1;
            if sign != 0 {
                cur = -cur;
            }
            if rv_len < rv.len() {
                rv[rv_len] = cur;
            }
            rv_len += 1;
            cur = 0;
            shift = 0;
        }
    }

    if cur != 0 || shift != 0 {
        Err(Error::VlqLeftover)
    } else if rv_len == 0 {
        Err(Error::VlqNoValues)
    } else {
        Ok(rv_len)
    }
}

#[test]
fn test_decode_sourcemap() {
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
    assert_eq!(
        iter.next().unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 0, 4, Some(&"x".into()))
    );
    assert_eq!(
        iter.next().unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 1, 4, Some(&"x".into()))
    );
    assert_eq!(
        iter.next().unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 2, 2, Some(&"alert".into()))
    );
    assert!(iter.next().is_none());
}

#[test]
fn test_decode_sourcemap_optional_filed() {
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
fn test_decode_mapping_bad_segment_size() {
    let input = r#"{
        "version": 3,
        "names": [],
        "sources": [],
        "sourcesContent": [],
        "mappings": "AA"
    }"#;

    let err = SourceMap::from_json_string(input).unwrap_err();
    assert!(matches!(err, Error::BadSegmentSize(2)));
}

#[test]
fn test_decode_mapping_vlq_leftover() {
    let input = r#"{
        "version": 3,
        "names": [],
        "sources": [],
        "sourcesContent": [],
        "mappings": "g"
    }"#;

    let err = SourceMap::from_json_string(input).unwrap_err();
    assert!(matches!(err, Error::VlqLeftover));
}
