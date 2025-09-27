//! Ported and modified from <https://github.com/getsentry/rust-sourcemap/blob/9.1.0/src/encoder.rs>

use json_escape_simd::{escape_into, escape_into_generic};

use crate::JSONSourceMap;
use crate::{SourceMap, token::TokenChunk, soa_tokens::SoaTokens};

pub fn encode(sourcemap: &SourceMap) -> JSONSourceMap {
    JSONSourceMap {
        file: sourcemap.get_file().map(ToString::to_string),
        mappings: {
            let mut mappings = String::with_capacity(estimate_mappings_length(sourcemap));
            serialize_sourcemap_mappings(sourcemap, &mut mappings);
            mappings
        },
        source_root: sourcemap.get_source_root().map(ToString::to_string),
        sources: sourcemap.sources.iter().map(ToString::to_string).collect(),
        sources_content: Some(
            sourcemap
                .source_contents
                .iter()
                .map(|v| v.as_ref().map(|item| item.to_string()))
                .collect(),
        ),
        names: sourcemap.names.iter().map(ToString::to_string).collect(),
        debug_id: sourcemap.get_debug_id().map(ToString::to_string),
        x_google_ignore_list: sourcemap.get_x_google_ignore_list().map(|x| x.to_vec()),
    }
}

pub fn encode_to_string(sourcemap: &SourceMap) -> String {
    // Worst-case capacity accounting:
    // - escape_into / escape_into_generic may write up to (len * 2 + 2) for each string
    // - include commas between items and constant JSON punctuation/keys
    let mut max_segments = 0usize;

    // {"version":3,
    max_segments += 13;

    // Optional "file":"...",
    if let Some(file) = sourcemap.get_file() {
        max_segments += 8 /* "file":" */ + file.as_ref().len() + 2 /* ", */;
    }

    // Optional "sourceRoot":"...",
    if let Some(source_root) = sourcemap.get_source_root() {
        max_segments += 14 /* "sourceRoot":" */ + source_root.len() + 2 /* ", */;
    }

    // "names":[
    max_segments += 9;
    let names_count = sourcemap.names.len();
    let names_len_sum: usize = sourcemap.names.iter().map(|s| s.len()).sum();
    max_segments += 2 * names_len_sum + 2 * names_count; // worst-case escaped items
    if names_count > 0 {
        max_segments += names_count - 1; // commas between items
    }

    // ],"sources":[
    max_segments += 13;
    let sources_count = sourcemap.sources.len();
    let sources_len_sum: usize = sourcemap.sources.iter().map(|s| s.len()).sum();
    max_segments += 2 * sources_len_sum + 2 * sources_count; // worst-case escaped items
    if sources_count > 0 {
        max_segments += sources_count - 1; // commas between items
    }

    // ],"sourcesContent":[
    max_segments += 20;
    let sc_count = sourcemap.source_contents.len();
    let sc_len_sum: usize = sourcemap
        .source_contents
        .iter()
        .map(|v| v.as_ref().map_or(/*"null"*/ 4, |s| s.len()))
        .sum();
    max_segments += 2 * sc_len_sum + 2 * sc_count; // worst-case escaped items
    if sc_count > 0 {
        max_segments += sc_count - 1; // commas between items
    }

    // Optional ],"x_google_ignoreList":[
    if let Some(x_google_ignore_list) = &sourcemap.x_google_ignore_list {
        max_segments += 25; // ],"x_google_ignoreList":[

        debug_assert!(
            x_google_ignore_list.iter().all(|&v| v < 10000),
            "x_google_ignore_list values must be < 10000"
        );
        let ig_count = x_google_ignore_list.len();
        // guess 4 digits per item
        max_segments += 4 * ig_count;
    }

    // ],"mappings":"
    max_segments += 14;
    max_segments += estimate_mappings_length(sourcemap);

    // Optional ,"debugId":"..."
    if let Some(debug_id) = sourcemap.get_debug_id() {
        max_segments += 13 /* ,"debugId":" */ + debug_id.len();
    }

    // "}
    max_segments += 2;
    let mut contents = PreAllocatedString::new(max_segments);

    contents.push("{\"version\":3,");
    if let Some(file) = sourcemap.get_file() {
        contents.push("\"file\":\"");
        contents.push(file.as_ref());
        contents.push("\",");
    }

    if let Some(source_root) = sourcemap.get_source_root() {
        contents.push("\"sourceRoot\":\"");
        contents.push(source_root);
        contents.push("\",");
    }

    contents.push("\"names\":[");
    contents.push_list(sourcemap.names.iter(), escape_into_generic);

    contents.push("],\"sources\":[");
    contents.push_list(sourcemap.sources.iter(), escape_into_generic);

    // Quote `source_content` in parallel
    let source_contents = &sourcemap.source_contents;
    contents.push("],\"sourcesContent\":[");
    contents.push_list(source_contents.iter().map(|v| v.as_deref().unwrap_or("null")), escape_into);

    if let Some(x_google_ignore_list) = &sourcemap.x_google_ignore_list {
        contents.push("],\"x_google_ignoreList\":[");
        contents.push_list(x_google_ignore_list.iter(), |s, output| {
            output.extend_from_slice(s.to_string().as_bytes());
        });
    }

    contents.push("],\"mappings\":\"");
    serialize_sourcemap_mappings(sourcemap, &mut contents.buf);

    if let Some(debug_id) = sourcemap.get_debug_id() {
        contents.push("\",\"debugId\":\"");
        contents.push(debug_id);
    }

    contents.push("\"}");

    // Check we calculated number of segments required correctly
    debug_assert!(contents.len() <= max_segments);

    contents.consume()
}

fn estimate_mappings_length(sourcemap: &SourceMap) -> usize {
    sourcemap
        .token_chunks
        .as_ref()
        .map(|chunks| {
            chunks.iter().map(|t| (t.end - t.start) * 10).sum::<u32>() as usize
                + chunks.last().map_or(0, |t| t.prev_dst_line as usize)
        })
        .unwrap_or_else(|| {
            sourcemap.tokens.len() * 10
                + sourcemap.tokens.last().map_or(0, |t| t.get_dst_line() as usize)
        })
}

fn serialize_sourcemap_mappings(sm: &SourceMap, output: &mut String) {
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

fn serialize_mappings(tokens: &SoaTokens, token_chunk: &TokenChunk, output: &mut String) {
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

    let mut prev_token = if start == 0 { None } else { tokens.get(start as usize - 1) };

    for i in start as usize..end as usize {
        let Some(token) = tokens.get(i) else { continue };
        // Max length of a single VLQ encoding is 7 bytes. Max number of calls to `encode_vlq_diff` is 5.
        // Also need 1 byte for each line number difference, or 1 byte if no line num difference.
        // Reserve this amount of capacity in `rv` early, so can skip bounds checks in code below.
        // As well as skipping the bounds checks, this also removes a function call to
        // `alloc::raw_vec::RawVec::grow_one` for every byte that's pushed.
        // https://godbolt.org/z/44G8jjss3
        const MAX_TOTAL_VLQ_BYTES: usize = 5 * MAX_VLQ_BYTES;

        let num_line_breaks = token.get_dst_line() - prev_dst_line;
        if num_line_breaks != 0 {
            output.reserve(MAX_TOTAL_VLQ_BYTES + num_line_breaks as usize);
            // SAFETY: We have reserved sufficient capacity for `num_line_breaks` bytes
            unsafe { push_bytes_unchecked(output, b';', num_line_breaks) };
            prev_dst_col = 0;
            prev_dst_line += num_line_breaks;
        } else if let Some(ref prev) = prev_token {
            if *prev == token {
                continue;
            }
            output.reserve(MAX_TOTAL_VLQ_BYTES + 1);
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

/// A helper for pre-allocate string buffer.
///
/// Pre-allocate a Cow<'a, str> buffer, and push the segment into it.
/// Finally, convert it to a pre-allocated length String.
struct PreAllocatedString {
    buf: String,
    len: usize,
}

impl PreAllocatedString {
    fn new(max_segments: usize) -> Self {
        Self { buf: String::with_capacity(max_segments), len: 0 }
    }

    #[inline]
    fn push(&mut self, s: &str) {
        self.len += s.len();
        self.buf.push_str(s);
    }

    #[inline]
    fn push_list<S, I>(&mut self, mut iter: I, encode: impl Fn(S, &mut Vec<u8>))
    where
        I: Iterator<Item = S>,
    {
        let Some(first) = iter.next() else {
            return;
        };
        encode(first, self.as_mut_vec());

        for other in iter {
            self.push(",");
            encode(other, self.as_mut_vec());
        }
    }

    fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        // SAFETY: we are sure that the string is not shared
        unsafe { self.buf.as_mut_vec() }
    }

    #[inline]
    fn consume(self) -> String {
        self.buf
    }

    fn len(&self) -> usize {
        self.buf.len()
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
        "x_google_ignoreList": [0, 1]
    }"#;
    let sm = SourceMap::from_json_string(input).unwrap();
    let sm2 = SourceMap::from_json_string(&sm.to_json_string()).unwrap();

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
    let sm2 = SourceMap::from_json_string(&sm.to_json_string()).unwrap();

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
