//! Ported and modified from <https://github.com/getsentry/rust-sourcemap/blob/9.1.0/src/encoder.rs>

use std::fmt::Write;

use json_escape_simd::escape_into;

use crate::JSONSourceMap;
use crate::{SourceMap, Token, token::TokenChunk};

pub fn encode(sourcemap: &SourceMap<'_>) -> JSONSourceMap {
    let has_source_contents = sourcemap.source_contents.iter().any(|v| v.is_some());
    JSONSourceMap {
        version: 3,
        file: sourcemap.get_file().map(ToString::to_string),
        mappings: {
            let mut mappings = String::with_capacity(estimate_mappings_length(sourcemap));
            serialize_sourcemap_mappings(sourcemap, &mut mappings);
            mappings
        },
        source_root: sourcemap.get_source_root().map(ToString::to_string),
        sources: sourcemap.sources.iter().map(ToString::to_string).collect(),
        sources_content: if has_source_contents {
            Some(
                sourcemap
                    .source_contents
                    .iter()
                    .map(|v| v.as_ref().map(|item| item.to_string()))
                    .collect(),
            )
        } else {
            None
        },
        names: sourcemap.names.iter().map(ToString::to_string).collect(),
        debug_id: sourcemap.get_debug_id().map(ToString::to_string),
        x_google_ignore_list: sourcemap.get_x_google_ignore_list().map(|x| x.to_vec()),
    }
}

pub fn encode_to_string(sourcemap: &SourceMap<'_>) -> String {
    let mut capacity = 0usize;

    // {"version":3,
    capacity += 13;

    // Calculate string lengths in a single pass for better cache locality
    let names_count = sourcemap.names.len();
    let sources_count = sourcemap.sources.len();
    // Accumulate total string bytes across all collections
    let mut total_string_bytes = 0usize;

    for name in &sourcemap.names {
        total_string_bytes += name.len();
    }

    for source in &sourcemap.sources {
        total_string_bytes += source.len();
    }

    // Single pass over source_contents to check existence and accumulate byte lengths
    let (has_source_contents, sc_bytes) =
        sourcemap.source_contents.iter().fold((false, 0usize), |(has_some, bytes), content| {
            match content {
                Some(s) => (true, bytes + s.len()),
                None => (has_some, bytes + 4), // "null"
            }
        });
    let sc_count = if has_source_contents { sourcemap.source_contents.len() } else { 0 };
    if has_source_contents {
        total_string_bytes += sc_bytes;
    }

    let string_count = names_count
        + sources_count
        + sc_count
        + usize::from(sourcemap.get_file().is_some())
        + usize::from(sourcemap.get_source_root().is_some())
        + usize::from(sourcemap.get_debug_id().is_some());
    let reserve_escapes_individually = total_string_bytes > INDIVIDUAL_ESCAPE_RESERVE_THRESHOLD;

    // Optional "file":"...",
    if let Some(file) = sourcemap.get_file() {
        capacity += 8 /* "file": */
            + estimated_escaped_json_string_len(file, reserve_escapes_individually)
            + 1 /* , */;
    }

    // Optional "sourceRoot":"...",
    if let Some(source_root) = sourcemap.get_source_root() {
        capacity += 14 /* "sourceRoot": */
            + estimated_escaped_json_string_len(source_root, reserve_escapes_individually)
            + 1 /* , */;
    }

    capacity += 9 + 13; // "names":[ + ],"sources":[
    if has_source_contents {
        capacity += 20; // ],"sourcesContent":[
    }
    if reserve_escapes_individually {
        capacity += total_string_bytes;
    } else {
        capacity += total_string_bytes * 6 + ESCAPE_SIMD_PADDING * usize::from(string_count > 0);
    }
    capacity += 2 * (names_count + sources_count + sc_count); // quotes around each item

    // Commas between array items
    let comma_count = names_count.saturating_sub(1)
        + sources_count.saturating_sub(1)
        + sc_count.saturating_sub(1);
    capacity += comma_count;

    // Optional ],"x_google_ignoreList":[
    if let Some(x_google_ignore_list) = &sourcemap.x_google_ignore_list {
        capacity += 25; // ],"x_google_ignoreList":[

        let ig_count = x_google_ignore_list.len();
        capacity += 10 * ig_count;
    }

    // ],"mappings":"
    capacity += 14;
    capacity += estimate_mappings_length(sourcemap);

    // Optional ,"debugId":<escaped>
    if let Some(debug_id) = sourcemap.get_debug_id() {
        capacity += 12 /* ,"debugId": */
            + estimated_escaped_json_string_len(debug_id, reserve_escapes_individually);
    }

    // "} (closing quote of mappings + closing brace)
    capacity += 2;
    let mut contents = JsonStringBuffer::new(capacity, reserve_escapes_individually);

    contents.push_str("{\"version\":3,");
    if let Some(file) = sourcemap.get_file() {
        contents.push_str("\"file\":");
        contents.push_escaped(file);
        contents.push_str(",");
    }

    if let Some(source_root) = sourcemap.get_source_root() {
        contents.push_str("\"sourceRoot\":");
        contents.push_escaped(source_root);
        contents.push_str(",");
    }

    contents.push_str("\"names\":[");
    contents.push_list(sourcemap.names.iter(), |s, out| out.push_escaped(s));

    contents.push_str("],\"sources\":[");
    contents.push_list(sourcemap.sources.iter(), |s, out| out.push_escaped(s));

    if has_source_contents {
        let source_contents = &sourcemap.source_contents;
        contents.push_str("],\"sourcesContent\":[");
        contents.push_list(source_contents.iter(), |v, output| match v {
            Some(s) => output.push_escaped(s),
            None => output.push_str("null"),
        });
    }

    if let Some(x_google_ignore_list) = &sourcemap.x_google_ignore_list {
        contents.push_str("],\"x_google_ignoreList\":[");
        contents.push_list(x_google_ignore_list.iter(), |s, output| {
            write!(output.as_string_mut(), "{s}").unwrap();
        });
    }

    contents.push_str("],\"mappings\":\"");
    serialize_sourcemap_mappings(sourcemap, contents.as_string_mut());
    contents.push_str("\"");

    if let Some(debug_id) = sourcemap.get_debug_id() {
        contents.push_str(",\"debugId\":");
        contents.push_escaped(debug_id);
    }

    contents.push_str("}");

    contents.into_string()
}

// `json_escape_simd::escape_into` writes into spare capacity and the crate's
// own `escape` helper allocates `len * 6 + 32 + 3`. Keep the same padding when
// we reserve immediately before escaping a string.
const ESCAPE_SIMD_PADDING: usize = 32 + 3;
const INDIVIDUAL_ESCAPE_RESERVE_THRESHOLD: usize = 4096;

fn estimated_escaped_json_string_len(value: &str, reserve_escapes_individually: bool) -> usize {
    if reserve_escapes_individually { value.len() + 2 } else { value.len() * 6 + 2 }
}

fn worst_case_escape_spare_capacity(value: &str) -> usize {
    value.len().saturating_mul(6).saturating_add(ESCAPE_SIMD_PADDING)
}

fn exact_escape_spare_capacity(value: &str) -> usize {
    let mut len = 2usize; // surrounding quotes
    for b in value.bytes() {
        len += match b {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            0x00..=0x1f => 6,
            _ => 1,
        };
    }
    len + ESCAPE_SIMD_PADDING
}

fn estimate_mappings_length(sourcemap: &SourceMap<'_>) -> usize {
    sourcemap
        .token_chunks
        .as_ref()
        .map(|chunks| {
            // Increased from 10 to 12 to account for worst-case VLQ encoding and separators
            // Add prev_dst_line for each chunk as those become semicolons
            chunks
                .iter()
                .map(|t| (t.end - t.start) as usize * 12 + t.prev_dst_line as usize)
                .sum::<usize>()
        })
        .unwrap_or_else(|| {
            sourcemap.tokens.len() * 12 + sourcemap.tokens.last().map_or(0, |t| t.dst_line as usize)
        })
}

fn serialize_sourcemap_mappings(sm: &SourceMap<'_>, output: &mut String) {
    if let Some(token_chunks) = sm.token_chunks.as_ref() {
        token_chunks.iter().for_each(|token_chunk| {
            serialize_mappings(&sm.tokens, token_chunk, output);
        })
    } else {
        serialize_mappings(
            &sm.tokens,
            &TokenChunk::new(0, sm.tokens.len() as u32, 0, 0, 0, 0, 0, 0),
            output,
        );
    }
}

// Max length of a single VLQ encoding
const MAX_VLQ_BYTES: usize = 7;

fn serialize_mappings(tokens: &[Token], token_chunk: &TokenChunk, output: &mut String) {
    let TokenChunk {
        start,
        end,
        mut prev_dst_line,
        mut prev_dst_col,
        mut prev_src_line,
        mut prev_src_col,
        mut prev_name_id,
        mut prev_source_id,
    } = *token_chunk;

    let mut prev_token = if start == 0 { None } else { Some(&tokens[start as usize - 1]) };

    for token in &tokens[start as usize..end as usize] {
        // Max length of a single VLQ encoding is 7 bytes. Max number of calls to `encode_vlq_diff` is 5.
        // Also need 1 byte for each line number difference, or 1 byte if no line num difference.
        // Reserve this amount of capacity in `rv` early, so can skip bounds checks in code below.
        // As well as skipping the bounds checks, this also removes a function call to
        // `alloc::raw_vec::RawVec::grow_one` for every byte that's pushed.
        // https://godbolt.org/z/44G8jjss3
        const MAX_TOTAL_VLQ_BYTES: usize = 5 * MAX_VLQ_BYTES;

        let num_line_breaks = token.get_dst_line() - prev_dst_line;
        if num_line_breaks != 0 {
            let required = MAX_TOTAL_VLQ_BYTES + num_line_breaks as usize;
            if output.capacity() - output.len() < required {
                output.reserve(required);
            }
            // SAFETY: We have reserved sufficient capacity for `num_line_breaks` bytes
            unsafe { push_bytes_unchecked(output, b';', num_line_breaks) };
            prev_dst_col = 0;
            prev_dst_line += num_line_breaks;
        } else if prev_token.is_some() {
            let required = MAX_TOTAL_VLQ_BYTES + 1;
            if output.capacity() - output.len() < required {
                output.reserve(required);
            }
            // SAFETY: We have reserved sufficient capacity for 1 byte
            unsafe { push_byte_unchecked(output, b',') };
        }

        // SAFETY: We have reserved enough capacity above to satisfy safety contract
        // of `encode_vlq_diff` for all calls below
        unsafe {
            encode_vlq_diff(output, token.get_dst_col(), prev_dst_col);
            prev_dst_col = token.get_dst_col();

            if let Some(source_id) = token.get_source_id() {
                encode_vlq_diff(output, source_id, prev_source_id);
                prev_source_id = source_id;
                encode_vlq_diff(output, token.get_src_line(), prev_src_line);
                prev_src_line = token.get_src_line();
                encode_vlq_diff(output, token.get_src_col(), prev_src_col);
                prev_src_col = token.get_src_col();
                if let Some(name_id) = token.get_name_id() {
                    encode_vlq_diff(output, name_id, prev_name_id);
                    prev_name_id = name_id;
                }
            }
        }

        prev_token = Some(token);
    }
}

/// Encode diff as VLQ and push encoding into `out`.
/// Will push between 1 byte (num = 0) and 7 bytes (num = -u32::MAX).
///
/// # SAFETY
/// Caller must ensure at least 7 bytes spare capacity in `out`,
/// as this function does not perform any bounds checks.
#[inline]
unsafe fn encode_vlq_diff(out: &mut String, a: u32, b: u32) {
    unsafe {
        encode_vlq(out, i64::from(a) - i64::from(b));
    }
}

// Align chars lookup table on 64 so occupies a single cache line
#[repr(align(64))]
struct Aligned64([u8; 64]);

static B64_CHARS: Aligned64 = Aligned64([
    b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M', b'N', b'O', b'P',
    b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', b'a', b'b', b'c', b'd', b'e', b'f',
    b'g', b'h', b'i', b'j', b'k', b'l', b'm', b'n', b'o', b'p', b'q', b'r', b's', b't', b'u', b'v',
    b'w', b'x', b'y', b'z', b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'+', b'/',
]);

/// Encode number as VLQ and push encoding into `out`.
/// Will push between 1 byte (num = 0) and 7 bytes (num = -u32::MAX).
///
/// # SAFETY
/// Caller must ensure at least 7 bytes spare capacity in `out`,
/// as this function does not perform any bounds checks.
unsafe fn encode_vlq(out: &mut String, num: i64) {
    unsafe {
        let mut num = if num < 0 { ((-num) << 1) + 1 } else { num << 1 };

        // Breaking out of loop early when have reached last char (rather than conditionally adding
        // 32 for last char within the loop) removes 3 instructions from the loop.
        // https://godbolt.org/z/Es4Pavh9j
        // This translates to a 16% speed-up for VLQ encoding.
        let mut digit;
        loop {
            digit = num & 0b11111;
            num >>= 5;
            if num == 0 {
                break;
            }

            let b = B64_CHARS.0[digit as usize + 32];
            // SAFETY:
            // * This loop can execute a maximum of 7 times, and on last turn will exit before getting here.
            //   Caller promises there are at least 7 bytes spare capacity in `out` at start. We only
            //   push 1 byte on each turn, so guaranteed there is at least 1 byte capacity in `out` here.
            // * All values in `B64_CHARS` lookup table are ASCII bytes.
            push_byte_unchecked(out, b);
        }

        let b = B64_CHARS.0[digit as usize];
        // SAFETY:
        // * The loop above pushes max 6 bytes. Caller promises there are at least 7 bytes spare capacity
        //   in `out` at start. So guaranteed there is at least 1 byte capacity in `out` here.
        // * All values in `B64_CHARS` lookup table are ASCII bytes.
        push_byte_unchecked(out, b);
    }
}

/// Push a byte to `out` without bounds checking.
///
/// # SAFETY
/// * `out` must have at least 1 byte spare capacity.
/// * `b` must be an ASCII byte (i.e. not `>= 128`).
//
// `#[inline(always)]` to ensure that `len` is stored in a register during `encode_vlq`'s loop.
#[expect(clippy::inline_always)]
#[inline(always)]
unsafe fn push_byte_unchecked(out: &mut String, b: u8) {
    unsafe {
        debug_assert!(out.len() < out.capacity());
        debug_assert!(b.is_ascii());

        let out = out.as_mut_vec();
        let len = out.len();
        let ptr = out.as_mut_ptr().add(len);
        ptr.write(b);
        out.set_len(len + 1);
    }
}

/// Push a byte to `out` a number of times without bounds checking.
///
/// # SAFETY
/// * `out` must have at least `repeats` bytes spare capacity.
/// * `b` must be an ASCII byte (i.e. not `>= 128`).
#[inline]
unsafe fn push_bytes_unchecked(out: &mut String, b: u8, repeats: u32) {
    unsafe {
        debug_assert!(out.capacity() - out.len() >= repeats as usize);
        debug_assert!(b.is_ascii());

        let out = out.as_mut_vec();
        let len = out.len();
        let mut ptr = out.as_mut_ptr().add(len);
        for _ in 0..repeats {
            ptr.write(b);
            ptr = ptr.add(1);
        }
        out.set_len(len + repeats as usize);
    }
}

/// Helper around the JSON output buffer.
///
/// Small maps keep one aggregate worst-case escape reserve for speed. Larger
/// maps reserve closer to the final JSON size, then grow per string only when
/// the SIMD escaper needs more spare capacity.
struct JsonStringBuffer {
    inner: String,
    reserve_escapes_individually: bool,
}

impl JsonStringBuffer {
    fn new(capacity: usize, reserve_escapes_individually: bool) -> Self {
        Self { inner: String::with_capacity(capacity), reserve_escapes_individually }
    }

    #[inline]
    fn push_str(&mut self, s: &str) {
        self.inner.push_str(s);
    }

    #[inline]
    fn push_escaped(&mut self, s: &str) {
        if self.reserve_escapes_individually {
            let spare = self.inner.capacity() - self.inner.len();
            let worst_case = worst_case_escape_spare_capacity(s);
            if spare < worst_case {
                let required = exact_escape_spare_capacity(s);
                if spare < required {
                    self.inner.reserve(required);
                }
            }
        }
        escape_into(s, self.as_mut_vec());
    }

    #[inline]
    fn push_list<S, I>(&mut self, mut iter: I, encode: impl Fn(S, &mut Self))
    where
        I: Iterator<Item = S>,
    {
        let Some(first) = iter.next() else {
            return;
        };
        encode(first, self);

        for other in iter {
            self.inner.push(',');
            encode(other, self);
        }
    }

    #[inline]
    fn as_string_mut(&mut self) -> &mut String {
        &mut self.inner
    }

    fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        // SAFETY: we are sure that the string is not shared
        unsafe { self.inner.as_mut_vec() }
    }

    #[inline]
    fn into_string(self) -> String {
        self.inner
    }
}

#[test]
fn test_encode() {
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM",
        "x_google_ignoreList": [0]
    }"#;
    let sm = SourceMap::from_json_string(input).unwrap();
    let encoded = sm.to_json_string();
    let sm2 = SourceMap::from_json_string(&encoded).unwrap();

    for (tok1, tok2) in sm.get_tokens().zip(sm2.get_tokens()) {
        assert_eq!(tok1, tok2);
    }

    // spellchecker:off
    let input = r#"{
        "version": 3,
        "file": "index.js",
        "names": [
            "text",
            "text"
        ],
        "sources": [
            "../../hmr.js",
            "../../main.js",
            "../../index.html"
        ],
        "sourcesContent": [
            "export const foo = 'hello'\n\ntext('.hmr', foo)\n\nfunction text(el, text) {\n  document.querySelector(el).textContent = text\n}\n\nimport.meta.hot?.accept((mod) =\u003E {\n  if (mod) {\n    text('.hmr', mod.foo)\n  }\n})\n",
            "import './hmr.js'\n\ntext('.app', 'hello')\n\nfunction text(el, text) {\n  document.querySelector(el).textContent = text\n}\n",
            "\u003Ch1\u003EHMR Full Bundle Mode\u003C/h1\u003E\n\n\u003Cdiv class=\"app\"\u003E\u003C/div\u003E\n\u003Cdiv class=\"hmr\"\u003E\u003C/div\u003E\n\n\u003Cscript type=\"module\" src=\"./main.js\"\u003E\u003C/script\u003E\n"
        ],
        "mappings": ";;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;AAAA,MAAa,MAAM;AAEnBA,OAAK,QAAQ,IAAI;AAEjB,SAASA,OAAK,IAAI,QAAM;AACtB,UAAS,cAAc,GAAG,CAAC,cAAcA;;SAG1B,QAAQ,QAAQ;AAC/B,KAAI,KAAK;AACP,SAAK,QAAQ,IAAI,IAAI;;EAEvB;;;;;;;ACVF,KAAK,QAAQ,QAAQ;AAErB,SAAS,KAAK,IAAI,QAAM;AACtB,UAAS,cAAc,GAAG,CAAC,cAAcC"
    }"#;
    // spellchecker:on
    let sm = SourceMap::from_json_string(input).unwrap();
    let encoded = sm.to_json_string();
    let sm2 = SourceMap::from_json_string(&encoded).unwrap();

    for (tok1, tok2) in sm.get_tokens().zip(sm2.get_tokens()) {
        assert_eq!(tok1, tok2);
    }
}

#[test]
fn test_encode_escape_string() {
    // '\0' should be escaped.
    let mut sm = SourceMap::new(
        None,
        vec!["name_length_greater_than_16_\0".into()],
        None,
        vec!["\0".into()],
        vec![Some("emoji-👀-\0".into())],
        vec![].into_boxed_slice(),
        None,
    );
    sm.set_x_google_ignore_list(vec![0]);
    sm.set_debug_id("56431d54-c0a6-451d-8ea2-ba5de5d8ca2e");
    assert_eq!(
        sm.to_json_string(),
        r#"{"version":3,"names":["name_length_greater_than_16_\u0000"],"sources":["\u0000"],"sourcesContent":["emoji-👀-\u0000"],"x_google_ignoreList":[0],"mappings":"","debugId":"56431d54-c0a6-451d-8ea2-ba5de5d8ca2e"}"#
    );
}

#[test]
fn test_vlq_encode_diff() {
    // Most import tests here are that with maximum values, `encode_vlq_diff` pushes maximum of 7 bytes.
    // This invariant is essential to safety of `encode_vlq_diff`.
    #[rustfmt::skip]
    const FIXTURES: &[(u32, u32, &str)] = &[
        (0,           0, "A"),
        (1,           0, "C"),
        (2,           0, "E"),
        (15,          0, "e"),
        (16,          0, "gB"),
        (511,         0, "+f"),
        (512,         0, "ggB"),
        (16_383,      0, "+/f"),
        (16_384,      0, "gggB"),
        (524_287,     0, "+//f"),
        (524_288,     0, "ggggB"),
        (16_777_215,  0, "+///f"),
        (16_777_216,  0, "gggggB"),
        (536_870_911, 0, "+////f"),
        (536_870_912, 0, "ggggggB"),
        (u32::MAX,    0, "+/////H"), // 7 bytes

        (0, 1,           "D"),
        (0, 2,           "F"),
        (0, 15,          "f"),
        (0, 16,          "hB"),
        (0, 511,         "/f"),
        (0, 512,         "hgB"),
        (0, 16_383,      "//f"),
        (0, 16_384,      "hggB"),
        (0, 524_287,     "///f"),
        (0, 524_288,     "hgggB"),
        (0, 16_777_215,  "////f"),
        (0, 16_777_216,  "hggggB"),
        (0, 536_870_911, "/////f"),
        (0, 536_870_912, "hgggggB"),
        (0, u32::MAX,    "//////H"), // 7 bytes
    ];

    for (a, b, res) in FIXTURES.iter().copied() {
        let mut out = String::with_capacity(MAX_VLQ_BYTES);
        // SAFETY: `out` has 7 bytes spare capacity
        unsafe { encode_vlq_diff(&mut out, a, b) };
        assert_eq!(&out, res);
    }
}

#[test]
fn test_encode_all_sources_content_null() {
    let sm = SourceMap::new(
        None,
        vec![],
        None,
        vec!["a.js".into(), "b.js".into()],
        vec![None, None],
        vec![].into_boxed_slice(),
        None,
    );
    let json = sm.to_json_string();
    assert!(
        !json.contains("sourcesContent"),
        "sourcesContent should be omitted when all items are None"
    );

    let json_map = encode(&sm);
    assert!(json_map.sources_content.is_none());

    let sm = SourceMap::new(
        None,
        vec![],
        None,
        vec!["a.js".into(), "b.js".into()],
        vec![Some("content".into()), None],
        vec![].into_boxed_slice(),
        None,
    );
    let json = sm.to_json_string();
    assert!(
        json.contains("sourcesContent"),
        "sourcesContent should be present when at least one item is Some"
    );
    assert!(
        json.contains(r#""sourcesContent":["content",null]"#),
        "None source_contents should be encoded as raw null, not quoted \"null\""
    );

    let json_map = encode(&sm);
    assert!(json_map.sources_content.is_some());
}

#[test]
fn test_encode_escape_file_and_source_root() {
    let sm = SourceMap::new(
        Some("file\0name.js".into()),
        vec![],
        Some("root\0path".into()),
        vec![],
        vec![],
        vec![].into_boxed_slice(),
        None,
    );
    let json = sm.to_json_string();
    assert!(
        json.contains(r#""file":"file\u0000name.js""#),
        "file field should have \\0 escaped: {json}"
    );
    assert!(
        json.contains(r#""sourceRoot":"root\u0000path""#),
        "sourceRoot field should have \\0 escaped: {json}"
    );
    // Verify the output is valid JSON by round-tripping
    SourceMap::from_json_string(&json).unwrap();
}

#[test]
fn test_encode_escape_debug_id() {
    let mut sm = SourceMap::default();
    // A debug_id containing JSON-special characters must be escaped, otherwise
    // the output is malformed JSON.
    sm.set_debug_id("id-with-\"quote\"-and-\\backslash");
    let json = sm.to_json_string();
    // Round-trip must succeed (would fail if the quote isn't escaped).
    let roundtripped = SourceMap::from_json_string(&json).unwrap();
    assert_eq!(roundtripped.get_debug_id(), Some("id-with-\"quote\"-and-\\backslash"));
}
