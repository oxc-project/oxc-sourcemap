//! Pre-encoded source map chunks (the esbuild model).
//!
//! A [`SourceMapChunk`] is a single source map's `mappings` already serialized to VLQ bytes,
//! plus the small amount of delta-carry state needed to splice it into a larger map without
//! touching the bulk of the bytes. It mirrors esbuild's `sourcemap.Chunk`.
//!
//! The point is to make concatenation ("stitching") cost O(modules) instead of O(tokens):
//! [`stitch_chunks`] re-encodes only the *first* segment of each chunk (and, if needed, the
//! first symbol name) against the running state of the previous chunk, then copies the rest of
//! the chunk's bytes verbatim. This is what lets a bundler join already-encoded per-module maps
//! without rebuilding a combined `Vec<Token>` or re-running the serializer over every token.
//!
//! ## Invariant
//!
//! Stitching relies on **every chunk's first token carrying a source** (so the source-delta
//! carry is well-defined at the seam). This is exactly what a code generator produces — it emits
//! a mapping for the start of the file — and it is the same assumption esbuild documents in
//! `AppendSourceMapChunk`. Maps whose first token has no source are not the target of this path
//! (they would need the slower token concat).

use crate::{SourceMap, Token, encode::encode_vlq_diff};

/// Max number of bytes a single VLQ value encodes to (matches `encode.rs`).
const MAX_VLQ_BYTES: usize = 7;

/// The running delta-carry state of a source map's `mappings` stream.
///
/// Every VLQ field is encoded as a delta against the previous segment; this holds the absolute
/// values those deltas are measured from. `generated_line` is not itself encoded as VLQ (line
/// breaks are the `;` separators) but is tracked so seams know how many `;` to emit.
///
/// All fields are plain `u32` "last real value" carries initialized to `0` — mirroring the
/// serializer's `prev_*` locals and [`crate::token::TokenChunk`]'s `prev_*` fields. The
/// missing-id sentinel never enters here: tokens without a source/name simply don't advance the
/// corresponding carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceMapState {
    pub generated_line: u32,
    pub generated_column: u32,
    pub source_id: u32,
    pub source_line: u32,
    pub source_column: u32,
    pub name_id: u32,
}

/// A single source map's `mappings`, pre-encoded to VLQ, ready to be stitched into a larger map.
///
/// Produced by [`SourceMap::to_chunk`]; consumed by [`stitch_chunks`]. Holds only the metadata
/// the stitch needs — the bulk of `mappings` is copied byte-for-byte.
#[derive(Debug, Clone)]
pub struct SourceMapChunk {
    /// The VLQ `mappings`, encoded relative to a zero start state and to this map's own
    /// (module-local) source/name ids.
    mappings: String,
    /// The first token, kept so the seam can re-encode the first segment against the previous
    /// chunk's end state. `None` for a token-less map (contributes no mappings).
    first_token: Option<Token>,
    /// Byte offset in `mappings` where the first segment ends (i.e. where the verbatim tail that
    /// can be copied unchanged begins).
    first_segment_end: u32,
    /// `(start, end, local_name_id)` of the first symbol name's VLQ in `mappings`, if any. Only
    /// needs rebasing when it lies in the tail (`start >= first_segment_end`); a name inside the
    /// first segment is handled by re-encoding that segment.
    first_name: Option<(u32, u32, u32)>,
    /// End-of-chunk carry. `generated_line/column` come from the last token. `source_*` / `name_id`
    /// are the *last real* values emitted (mirrors the concat builder's reverse scan); `None` when
    /// the chunk emitted no source / no name, so the previous carry flows through unchanged.
    end_generated_line: u32,
    end_generated_column: u32,
    end_source: Option<(u32, u32, u32)>,
    end_name_id: Option<u32>,
    /// Lengths used to advance the global source/name offsets between chunks.
    sources_len: u32,
    names_len: u32,
}

/// Reserve space for one VLQ value and append `a - b` (as the serializer encodes it).
#[inline]
fn push_vlq_diff(out: &mut String, a: u32, b: u32) {
    out.reserve(MAX_VLQ_BYTES);
    // SAFETY: just reserved `MAX_VLQ_BYTES` (7) spare bytes, satisfying `encode_vlq_diff`.
    unsafe { encode_vlq_diff(out, a, b) };
}

/// Append one token's segment to `out`, advancing `state`. Byte-for-byte identical to the
/// serializer in `encode.rs`. Returns the `(start, end, local_name_id)` byte range of the name
/// VLQ if this token emitted a name.
fn emit_token(
    out: &mut String,
    state: &mut SourceMapState,
    token: Token,
    has_prev: bool,
) -> Option<(u32, u32, u32)> {
    let num_line_breaks = token.get_dst_line() - state.generated_line;
    if num_line_breaks != 0 {
        for _ in 0..num_line_breaks {
            out.push(';');
        }
        state.generated_column = 0;
        state.generated_line = token.get_dst_line();
    } else if has_prev {
        out.push(',');
    }

    push_vlq_diff(out, token.get_dst_col(), state.generated_column);
    state.generated_column = token.get_dst_col();

    let mut name_range = None;
    if let Some(source_id) = token.get_source_id() {
        push_vlq_diff(out, source_id, state.source_id);
        state.source_id = source_id;
        push_vlq_diff(out, token.get_src_line(), state.source_line);
        state.source_line = token.get_src_line();
        push_vlq_diff(out, token.get_src_col(), state.source_column);
        state.source_column = token.get_src_col();
        if let Some(name_id) = token.get_name_id() {
            let start = out.len() as u32;
            push_vlq_diff(out, name_id, state.name_id);
            state.name_id = name_id;
            name_range = Some((start, out.len() as u32, name_id));
        }
    }
    name_range
}

impl SourceMap<'_> {
    /// Encode this map's `mappings` to a self-contained [`SourceMapChunk`] (relative to a zero
    /// start state and this map's own source/name ids), capturing the metadata needed to stitch
    /// it into a larger map. The inverse of building tokens then serializing — here the bytes are
    /// produced once and never re-walked.
    pub fn to_chunk(&self) -> SourceMapChunk {
        let mut mappings = String::with_capacity(self.tokens.len() * 12);
        let mut state = SourceMapState::default();

        let mut first_token = None;
        let mut first_segment_end = 0;
        let mut first_name = None;
        let mut end_source = None;
        let mut end_name_id = None;

        for (i, token) in self.tokens.iter().enumerate() {
            if i == 0 {
                first_token = Some(*token);
            }
            let name_range = emit_token(&mut mappings, &mut state, *token, i > 0);
            if i == 0 {
                first_segment_end = mappings.len() as u32;
            }
            if first_name.is_none()
                && let Some(range) = name_range
            {
                first_name = Some(range);
            }
            // Track the *last real* source/name, mirroring the concat builder's reverse scan.
            if token.get_source_id().is_some() {
                end_source = Some((state.source_id, state.source_line, state.source_column));
            }
            if token.get_name_id().is_some() {
                end_name_id = Some(state.name_id);
            }
        }

        SourceMapChunk {
            mappings,
            first_token,
            first_segment_end,
            first_name,
            end_generated_line: state.generated_line,
            end_generated_column: state.generated_column,
            end_source,
            end_name_id,
            sources_len: self.sources.len() as u32,
            names_len: self.names.len() as u32,
        }
    }
}

/// Stitch pre-encoded chunks into a single `mappings` string, each offset by its generated-line
/// `line_offset`. Byte-for-byte equal to concatenating the same maps with
/// [`crate::ConcatSourceMapBuilder`] and serializing — but it re-encodes only each chunk's first
/// segment (and first tail name) and copies the rest verbatim.
///
/// See the module-level invariant: each chunk's first token must carry a source.
pub fn stitch_chunks(chunks: &[(&SourceMapChunk, u32)]) -> String {
    let capacity: usize =
        chunks.iter().map(|(c, offset)| c.mappings.len() + *offset as usize).sum();
    let mut out = String::with_capacity(capacity);

    let mut prev = SourceMapState::default();
    let mut source_offset = 0u32;
    let mut name_offset = 0u32;
    let mut has_prev = false;

    for (chunk, line_offset) in chunks {
        let line_offset = *line_offset;
        if let Some(first_token) = chunk.first_token {
            // Re-encode the first segment against the previous chunk's end state. The seam's `;`
            // gap and all source/name deltas fall out of `emit_token` exactly as the serializer
            // would compute them over a combined token stream.
            let global_first = first_token.translated(line_offset, source_offset, name_offset);
            emit_token(&mut out, &mut prev, global_first, has_prev);

            // Append the verbatim tail. Its deltas are relative to the (module-local) first token;
            // because the global offset is a constant shift, those deltas are unchanged — except
            // the first symbol name, which (when it lives in the tail) is the first name overall
            // and so must be rebased against the running name carry.
            let tail_start = chunk.first_segment_end as usize;
            match chunk.first_name {
                Some((name_start, name_end, local_name))
                    if name_start >= chunk.first_segment_end =>
                {
                    out.push_str(&chunk.mappings[tail_start..name_start as usize]);
                    push_vlq_diff(&mut out, local_name + name_offset, prev.name_id);
                    out.push_str(&chunk.mappings[name_end as usize..]);
                }
                _ => out.push_str(&chunk.mappings[tail_start..]),
            }

            // Advance the running carry to this chunk's end, translated to global ids. Only fields
            // the chunk actually emitted advance; otherwise the previous carry flows through.
            prev.generated_line = chunk.end_generated_line + line_offset;
            prev.generated_column = chunk.end_generated_column;
            if let Some((source_id, source_line, source_column)) = chunk.end_source {
                prev.source_id = source_id + source_offset;
                prev.source_line = source_line;
                prev.source_column = source_column;
            }
            if let Some(name_id) = chunk.end_name_id {
                prev.name_id = name_id + name_offset;
            }
            has_prev = true;
        }

        source_offset += chunk.sources_len;
        name_offset += chunk.names_len;
    }

    out
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::ConcatSourceMapBuilder;

    /// Build a map whose tokens all carry source 0 (the stitch invariant), with the given
    /// `(dst_line, dst_col, src_line, src_col, name_id)` rows.
    fn map(names: &[&str], rows: &[(u32, u32, u32, u32, Option<u32>)]) -> SourceMap<'static> {
        let tokens = rows
            .iter()
            .map(|&(dl, dc, sl, sc, name)| Token::new(dl, dc, sl, sc, Some(0), name))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        SourceMap::new(
            None,
            names.iter().map(|n| Cow::Owned((*n).to_owned())).collect(),
            None,
            vec![Cow::Owned("file.js".to_owned())],
            vec![],
            tokens,
            None,
        )
    }

    /// The oracle: concat + serialize via the trusted token path.
    fn oracle(maps: &[(&SourceMap<'static>, u32)]) -> String {
        ConcatSourceMapBuilder::from_sourcemaps(maps).into_sourcemap().to_json().mappings
    }

    fn stitched(maps: &[(&SourceMap<'static>, u32)]) -> String {
        let chunks: Vec<SourceMapChunk> = maps.iter().map(|(m, _)| m.to_chunk()).collect();
        let refs: Vec<(&SourceMapChunk, u32)> =
            chunks.iter().zip(maps.iter()).map(|(c, (_, off))| (c, *off)).collect();
        stitch_chunks(&refs)
    }

    fn assert_matches(maps: &[(&SourceMap<'static>, u32)]) {
        assert_eq!(stitched(maps), oracle(maps));
    }

    #[test]
    fn single_chunk_roundtrips() {
        let m =
            map(&["a", "b"], &[(0, 0, 0, 0, None), (0, 6, 0, 6, Some(0)), (1, 0, 1, 0, Some(1))]);
        assert_eq!(m.to_chunk().mappings, m.to_json().mappings);
        assert_matches(&[(&m, 0)]);
    }

    #[test]
    fn two_chunks_no_names() {
        let m0 = map(&[], &[(0, 0, 0, 0, None), (0, 6, 0, 6, None), (0, 12, 0, 12, None)]);
        let m1 = map(&[], &[(0, 0, 0, 0, None), (1, 0, 1, 0, None)]);
        assert_matches(&[(&m0, 0), (&m1, 2)]);
    }

    #[test]
    fn names_in_first_segment_and_tail() {
        // m0: first token has a name (handled by re-encoding the first segment).
        let m0 = map(&["x", "y"], &[(0, 0, 0, 0, Some(0)), (0, 4, 0, 4, Some(1))]);
        // m1: first name lives in the tail (first token has none) — exercises the tail rebase.
        let m1 = map(&["z"], &[(0, 0, 0, 0, None), (0, 5, 0, 5, Some(0)), (1, 2, 1, 2, None)]);
        assert_matches(&[(&m0, 0), (&m1, 3)]);
    }

    #[test]
    fn three_chunks_mixed() {
        let m0 = map(&["foo"], &[(0, 0, 0, 0, Some(0)), (2, 1, 2, 1, None)]);
        let m1 = map(
            &["bar", "baz"],
            &[(0, 0, 0, 0, None), (0, 7, 1, 0, Some(0)), (0, 12, 1, 5, Some(1))],
        );
        let m2 = map(&[], &[(0, 0, 0, 0, None), (1, 4, 3, 2, None)]);
        assert_matches(&[(&m0, 0), (&m1, 4), (&m2, 6)]);
    }

    #[test]
    fn fuzz_against_oracle() {
        // Deterministic LCG so failures reproduce; no external rand dependency.
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };

        for _ in 0..2000 {
            let num_modules = 1 + next() % 5;
            let mut owned_maps: Vec<SourceMap<'static>> = Vec::new();
            let mut offsets: Vec<u32> = Vec::new();
            let mut line_cursor = 0u32;

            for _ in 0..num_modules {
                let num_names = next() % 4;
                let names: Vec<String> = (0..num_names).map(|n| format!("n{n}")).collect();
                let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();

                let num_tokens = 1 + next() % 8;
                let mut rows = Vec::new();
                let mut dl = 0u32;
                let mut dc = 0u32;
                for t in 0..num_tokens {
                    // Generated positions must be non-decreasing (line, then column).
                    if next() % 3 == 0 {
                        dl += 1 + next() % 2;
                        dc = next() % 8;
                    } else {
                        dc += 1 + next() % 8;
                    }
                    let sl = next() % 50;
                    let sc = next() % 50;
                    // First token must carry a source (the invariant); give it no name so both the
                    // first-segment-name and tail-name paths get exercised across iterations.
                    let name = if t == 0 || num_names == 0 {
                        None
                    } else if next() % 2 == 0 {
                        Some(next() % num_names)
                    } else {
                        None
                    };
                    rows.push((dl, dc, sl, sc, name));
                }

                owned_maps.push(map(&name_refs, &rows));
                offsets.push(line_cursor);
                // Next module starts strictly after this one (joined by a newline in a bundler).
                line_cursor += dl + 1 + next() % 3;
            }

            let maps: Vec<(&SourceMap<'static>, u32)> =
                owned_maps.iter().zip(offsets.iter().copied()).collect();
            assert_eq!(stitched(&maps), oracle(&maps), "mismatch for inputs: {maps:?}");
        }
    }
}
