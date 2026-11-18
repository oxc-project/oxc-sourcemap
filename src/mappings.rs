//! Compact storage for the mappings of a source map.
//!
//! A [`Token`] is 24 bytes. A build holds most of its maps for much longer than it reads them,
//! so [`Mappings`] stores tokens the way the `mappings` field of a source map does, as deltas
//! from the previous token, in a byte stream of about 3 bytes for each token.
//!
//! Each token is a tag byte followed by the deltas it needs, as zigzag LEB128 varints:
//!
//! | Tag bit | Meaning |
//! |---|---|
//! | 0 | the generated line changed; its delta follows the column |
//! | 1 | the token is mapped; original position deltas follow |
//! | 2 | the token is named; the name delta comes last |
//! | 3 | same source and original line as the previous mapped token; both deltas are omitted |
//! | 4-7 | generated column delta + 1 when the delta is in `0..15`, else `0` and the delta follows the tag |
//!
//! The generated column is relative to the previous token on the same line, and to `0` on a new
//! line. The source and the original position are relative to the previous mapped token. The
//! name is relative to the previous named token. These are the rules of the `mappings` field, so
//! a stream converts to and from VLQ one token at a time, and two streams join by copying bytes:
//! only the first token, the first mapped token and the first named token of the appended
//! stream are relative to state that the join changes.

use std::iter::FusedIterator;

use crate::token::{INVALID_ID, Token};

pub(crate) const TAG_LINE: u8 = 1;
pub(crate) const TAG_MAPPED: u8 = 2;
pub(crate) const TAG_NAMED: u8 = 4;
/// Same source and original line as the previous mapped token.
pub(crate) const TAG_SAME_SOURCE_LINE: u8 = 8;

const NONE: u32 = u32::MAX;

/// A tag byte and at most six varints. A delta of two `u32`s takes at most five bytes, and
/// no `i64` takes more than ten, so [`Mappings::write`] never writes past this many bytes.
const MAX_TOKEN_BYTES: usize = 1 + 6 * 10;

/// The values that the deltas of the next token are relative to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct State {
    dst_line: u32,
    dst_col: u32,
    source_id: u32,
    src_line: u32,
    src_col: u32,
    name_id: u32,
}

/// The tokens of a source map, stored as deltas in about 3 bytes per token instead of 24.
///
/// Add tokens with [`push`](Self::push), read them back with [`tokens`](Self::tokens), and join
/// two sets with [`append`](Self::append). Tokens are read in the order they were added, and only
/// from the start: use a [`LookupTable`] to find the token at a position.
///
/// An unmapped token keeps only its generated position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mappings {
    bytes: Vec<u8>,
    len: u32,
    /// Byte offset of the first mapped token, or [`NONE`].
    first_mapped: u32,
    /// Byte offset of the first named token, or [`NONE`].
    first_named: u32,
    /// [`TAG_MAPPED`] and [`TAG_NAMED`] of the last token.
    last_tag: u8,
    /// The state after the last token.
    end: State,
}

impl Default for Mappings {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            len: 0,
            first_mapped: NONE,
            first_named: NONE,
            last_tag: 0,
            end: State::default(),
        }
    }
}

/// One token as stored: its tag and deltas.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawToken {
    pub tag: u8,
    pub dst_line: i64,
    pub dst_col: i64,
    pub source_id: i64,
    pub src_line: i64,
    pub src_col: i64,
    pub name_id: i64,
}

impl Mappings {
    /// Create empty mappings with room for about `tokens` tokens.
    pub fn with_capacity(tokens: usize) -> Self {
        // Real maps need about three bytes for each token.
        Self { bytes: Vec::with_capacity(tokens.saturating_mul(3)), ..Self::default() }
    }

    /// Create empty mappings with room for `bytes` bytes of encoded tokens.
    /// See [`byte_len`](Self::byte_len).
    pub fn with_byte_capacity(bytes: usize) -> Self {
        Self { bytes: Vec::with_capacity(bytes), ..Self::default() }
    }

    /// Encode `tokens`.
    pub fn from_tokens<I: IntoIterator<Item = Token>>(tokens: I) -> Self {
        let tokens = tokens.into_iter();
        let mut mappings = Self::with_capacity(tokens.size_hint().0);
        for token in tokens {
            mappings.push(token);
        }
        mappings.shrink_to_fit();
        mappings
    }

    /// The number of tokens.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The size of the encoded tokens in bytes.
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Reserve room for `bytes` more bytes of encoded tokens.
    pub fn reserve_bytes(&mut self, bytes: usize) {
        self.bytes.reserve(bytes);
    }

    /// Release unused capacity. Call this before holding mappings that are complete.
    pub fn shrink_to_fit(&mut self) {
        self.bytes.shrink_to_fit();
    }

    /// The generated line of the last token, or `0` when empty.
    pub(crate) fn last_dst_line(&self) -> u32 {
        self.end.dst_line
    }

    /// The last token.
    pub fn last(&self) -> Option<Token> {
        if self.len == 0 {
            return None;
        }
        let end = &self.end;
        let mut token = Token::unmapped(end.dst_line, end.dst_col);
        if self.last_tag & TAG_MAPPED != 0 {
            token.source_id = end.source_id;
            token.src_line = end.src_line;
            token.src_col = end.src_col;
        }
        if self.last_tag & TAG_NAMED != 0 {
            token.name_id = end.name_id;
        }
        Some(token)
    }

    /// Add a token after the last one.
    #[inline]
    pub fn push(&mut self, token: Token) {
        // The usual token of generated code: on the same generated line as the last token, a
        // few columns along, and from the same line of the same source. It is two bytes.
        let end = &mut self.end;
        let dst_col = token.dst_col.wrapping_sub(end.dst_col);
        let src_col = delta(token.src_col, end.src_col);
        if token.dst_line == end.dst_line
            && token.source_id == end.source_id
            && token.src_line == end.src_line
            && token.name_id == INVALID_ID
            && dst_col < 15
            && (-64..64).contains(&src_col)
            && self.first_mapped != NONE
        {
            end.dst_col = token.dst_col;
            end.src_col = token.src_col;
            let tag = ((dst_col as u8 + 1) << 4) | TAG_MAPPED | TAG_SAME_SOURCE_LINE;
            let zigzag = ((src_col << 1) ^ (src_col >> 63)) as u8;
            self.bytes.extend_from_slice(&[tag, zigzag]);
            self.last_tag = TAG_MAPPED;
            self.len += 1;
            return;
        }
        self.push_any(token);
    }

    /// Add a token of any shape after the last one.
    fn push_any(&mut self, token: Token) {
        let (dst_line, dst_col, source, name) = deltas(&mut self.end, token);
        self.write(dst_line, dst_col, source, name);
    }

    /// Write one token given as deltas. The caller keeps [`Self::end`] up to date,
    /// see [`set_end`](Self::set_end).
    #[inline(always)]
    pub(crate) fn write(
        &mut self,
        dst_line: i64,
        dst_col: i64,
        source: Option<(i64, i64, i64)>,
        name: Option<i64>,
    ) {
        let at = self.bytes.len();
        self.bytes.reserve(MAX_TOKEN_BYTES);
        // SAFETY: `MAX_TOKEN_BYTES` bytes are reserved after `at`.
        let tag = unsafe {
            let start = self.bytes.as_mut_ptr().add(at);
            let (end, tag) = encode(start, dst_line, dst_col, source, name);
            self.bytes.set_len(at + end.offset_from_unsigned(start));
            tag
        };
        if tag & TAG_MAPPED != 0 && self.first_mapped == NONE {
            self.first_mapped = at as u32;
        }
        if tag & TAG_NAMED != 0 && self.first_named == NONE {
            self.first_named = at as u32;
        }
        self.last_tag = tag & (TAG_MAPPED | TAG_NAMED);
        self.len += 1;
    }

    /// Set the state after the last token, for a caller of [`write`](Self::write).
    /// The source and name are the previous values when no token was mapped or named.
    pub(crate) fn set_end(
        &mut self,
        dst_line: u32,
        dst_col: u32,
        source_id: u32,
        src_line: u32,
        src_col: u32,
        name_id: u32,
    ) {
        self.end = State { dst_line, dst_col, source_id, src_line, src_col, name_id };
    }

    /// Add the tokens of `other` after the last token, with `line_offset` added to their
    /// generated lines, `source_offset` to their source ids and `name_offset` to their name ids.
    ///
    /// The first token of `other` is dropped when it equals the last token of `self`.
    ///
    /// All but at most three tokens of `other` are copied as bytes, without decoding them.
    pub fn append(
        &mut self,
        other: &Mappings,
        line_offset: u32,
        source_offset: u32,
        name_offset: u32,
    ) {
        if other.len == 0 {
            return;
        }
        let len = self.len;
        let bytes = other.bytes.as_slice();
        let mut pos = 0;

        // The first token of `other` is relative to the zero state, so its deltas are its values.
        let raw = read_raw(bytes, &mut pos);
        let mut first = Token::unmapped(raw.dst_line as u32 + line_offset, raw.dst_col as u32);
        if raw.tag & TAG_MAPPED != 0 {
            first.source_id = raw.source_id as u32 + source_offset;
            first.src_line = raw.src_line as u32;
            first.src_col = raw.src_col as u32;
        }
        if raw.tag & TAG_NAMED != 0 {
            first.name_id = raw.name_id as u32 + name_offset;
        }
        let duplicate = self.last() == Some(first);
        if !duplicate {
            self.push(first);
        }

        // The first mapped and the first named token are relative to the zero state too.
        // When either is the first token, it is already handled.
        let patch = |at: u32| (at != NONE && at != 0).then_some(at as usize);
        let (patch_mapped, patch_named) = (patch(other.first_mapped), patch(other.first_named));
        let patches = match (patch_mapped, patch_named) {
            (Some(mapped), Some(named)) if mapped == named => [Some(mapped), None],
            (Some(mapped), Some(named)) => [Some(mapped.min(named)), Some(mapped.max(named))],
            (at, None) | (None, at) => [at, None],
        };

        self.bytes.reserve(bytes.len() - pos + 2 * MAX_TOKEN_BYTES);
        for at in patches.into_iter().flatten() {
            self.bytes.extend_from_slice(&bytes[pos..at]);
            pos = at;
            let raw = read_raw(bytes, &mut pos);
            let source = (raw.tag & TAG_MAPPED != 0).then(|| {
                if patch_mapped == Some(at) {
                    let end = &self.end;
                    (
                        raw.source_id + i64::from(source_offset) - i64::from(end.source_id),
                        raw.src_line - i64::from(end.src_line),
                        raw.src_col - i64::from(end.src_col),
                    )
                } else {
                    (raw.source_id, raw.src_line, raw.src_col)
                }
            });
            let name = (raw.tag & TAG_NAMED != 0).then(|| {
                if patch_named == Some(at) {
                    raw.name_id + i64::from(name_offset) - i64::from(self.end.name_id)
                } else {
                    raw.name_id
                }
            });
            self.write(raw.dst_line, raw.dst_col, source, name);
        }
        self.bytes.extend_from_slice(&bytes[pos..]);

        self.len = len + other.len - u32::from(duplicate);
        self.last_tag = other.last_tag;
        self.end.dst_line = other.end.dst_line + line_offset;
        self.end.dst_col = other.end.dst_col;
        if other.first_mapped != NONE {
            self.end.source_id = other.end.source_id + source_offset;
            self.end.src_line = other.end.src_line;
            self.end.src_col = other.end.src_col;
        }
        if other.first_named != NONE {
            self.end.name_id = other.end.name_id + name_offset;
        }
    }

    /// Decode the tokens, in the order they were added.
    pub fn tokens(&self) -> MappingsIter<'_> {
        MappingsIter { raw: self.raw_tokens(), state: State::default() }
    }

    /// Read the tokens as stored.
    pub(crate) fn raw_tokens(&self) -> RawTokens<'_> {
        RawTokens { pos: self.bytes.as_ptr(), remaining: self.len, bytes: std::marker::PhantomData }
    }
}

impl FromIterator<Token> for Mappings {
    fn from_iter<I: IntoIterator<Item = Token>>(tokens: I) -> Self {
        Self::from_tokens(tokens)
    }
}

impl Extend<Token> for Mappings {
    fn extend<I: IntoIterator<Item = Token>>(&mut self, tokens: I) {
        for token in tokens {
            self.push(token);
        }
    }
}

impl From<Vec<Token>> for Mappings {
    fn from(tokens: Vec<Token>) -> Self {
        Self::from_tokens(tokens)
    }
}

impl From<Box<[Token]>> for Mappings {
    fn from(tokens: Box<[Token]>) -> Self {
        Self::from_tokens(tokens)
    }
}

impl From<&[Token]> for Mappings {
    fn from(tokens: &[Token]) -> Self {
        Self::from_tokens(tokens.iter().copied())
    }
}

impl<'a> IntoIterator for &'a Mappings {
    type Item = Token;
    type IntoIter = MappingsIter<'a>;

    fn into_iter(self) -> MappingsIter<'a> {
        self.tokens()
    }
}

/// A token as deltas: generated line, generated column, source (id, line, column), name.
type Deltas = (i64, i64, Option<(i64, i64, i64)>, Option<i64>);

/// The deltas of `token` from the state `end`, which becomes the state after `token`.
#[inline(always)]
fn deltas(end: &mut State, token: Token) -> Deltas {
    let dst_line = delta(token.dst_line, end.dst_line);
    if dst_line != 0 {
        end.dst_line = token.dst_line;
        end.dst_col = 0;
    }
    let dst_col = delta(token.dst_col, end.dst_col);
    end.dst_col = token.dst_col;

    let source = if token.source_id == INVALID_ID {
        None
    } else {
        let deltas = (
            delta(token.source_id, end.source_id),
            delta(token.src_line, end.src_line),
            delta(token.src_col, end.src_col),
        );
        end.source_id = token.source_id;
        end.src_line = token.src_line;
        end.src_col = token.src_col;
        Some(deltas)
    };
    let name = if token.name_id == INVALID_ID {
        None
    } else {
        let name = delta(token.name_id, end.name_id);
        end.name_id = token.name_id;
        Some(name)
    };
    (dst_line, dst_col, source, name)
}

/// Encode one token given as deltas at `start`. Returns the position after it, and its tag.
///
/// # SAFETY
/// `start` must be valid for writes of [`MAX_TOKEN_BYTES`] bytes.
#[inline(always)]
unsafe fn encode(
    start: *mut u8,
    dst_line: i64,
    dst_col: i64,
    source: Option<(i64, i64, i64)>,
    name: Option<i64>,
) -> (*mut u8, u8) {
    let mut tag = 0u8;
    // SAFETY: the tag is one byte, followed by at most six varints of at most ten bytes each.
    unsafe {
        let mut end = start.add(1);
        if (0..15).contains(&dst_col) {
            tag |= (dst_col as u8 + 1) << 4;
        } else {
            end = write_varint(end, dst_col);
        }
        if dst_line != 0 {
            tag |= TAG_LINE;
            end = write_varint(end, dst_line);
        }
        if let Some((source_id, src_line, src_col)) = source {
            tag |= TAG_MAPPED;
            if source_id == 0 && src_line == 0 {
                tag |= TAG_SAME_SOURCE_LINE;
            } else {
                end = write_varint(end, source_id);
                end = write_varint(end, src_line);
            }
            end = write_varint(end, src_col);
        }
        if let Some(name) = name {
            tag |= TAG_NAMED;
            end = write_varint(end, name);
        }
        start.write(tag);
        (end, tag)
    }
}

#[inline(always)]
fn delta(value: u32, prev: u32) -> i64 {
    i64::from(value) - i64::from(prev)
}

/// Write `value` as a zigzag LEB128 varint at `out`. Returns the position after it.
///
/// # SAFETY
/// `out` must be valid for writes of ten bytes.
#[inline(always)]
unsafe fn write_varint(mut out: *mut u8, value: i64) -> *mut u8 {
    let mut zigzag = ((value << 1) ^ (value >> 63)) as u64;
    // SAFETY: a `u64` has at most ten groups of seven bits, and the caller provides ten bytes.
    unsafe {
        while zigzag >= 0x80 {
            out.write(zigzag as u8 | 0x80);
            out = out.add(1);
            zigzag >>= 7;
        }
        out.write(zigzag as u8);
        out.add(1)
    }
}

/// Read a varint at `*pos` and move `*pos` past it.
///
/// # SAFETY
/// `*pos` must point at a varint written by [`write_varint`].
#[inline(always)]
unsafe fn read_varint(pos: &mut *const u8) -> i64 {
    // SAFETY: the caller guarantees a complete varint at `*pos`.
    unsafe {
        let mut byte = pos.read();
        *pos = pos.add(1);
        let mut zigzag = u64::from(byte & 0x7f);
        if byte & 0x80 != 0 {
            let mut shift = 7;
            loop {
                byte = pos.read();
                *pos = pos.add(1);
                zigzag |= u64::from(byte & 0x7f) << shift;
                shift += 7;
                if byte & 0x80 == 0 {
                    break;
                }
            }
        }
        ((zigzag >> 1) as i64) ^ -((zigzag & 1) as i64)
    }
}

/// Read the token at `*pos` and move `*pos` past it.
///
/// # SAFETY
/// `*pos` must point at a token written by [`Mappings::write`].
#[inline(always)]
unsafe fn read_raw_at(pos: &mut *const u8) -> RawToken {
    // SAFETY: the caller guarantees a complete token at `*pos`, so the tag and every varint
    // that the tag announces are there to read.
    unsafe {
        let tag = pos.read();
        *pos = pos.add(1);
        let mut raw = RawToken {
            tag,
            dst_line: 0,
            dst_col: 0,
            source_id: 0,
            src_line: 0,
            src_col: 0,
            name_id: 0,
        };
        raw.dst_col = match tag >> 4 {
            0 => read_varint(pos),
            small => i64::from(small) - 1,
        };
        if tag & TAG_LINE != 0 {
            raw.dst_line = read_varint(pos);
        }
        if tag & TAG_MAPPED != 0 {
            if tag & TAG_SAME_SOURCE_LINE == 0 {
                raw.source_id = read_varint(pos);
                raw.src_line = read_varint(pos);
            }
            raw.src_col = read_varint(pos);
        }
        if tag & TAG_NAMED != 0 {
            raw.name_id = read_varint(pos);
        }
        raw
    }
}

/// Read the token at byte offset `*pos` of `bytes` and move `*pos` past it.
///
/// `bytes` are the bytes of [`Mappings`], and `*pos` is the start of a token.
#[inline]
fn read_raw(bytes: &[u8], pos: &mut usize) -> RawToken {
    assert!(*pos < bytes.len());
    // SAFETY: `Mappings::bytes` only holds whole tokens written by `Mappings::write`, and the
    // callers pass the offset of the first token, or an offset that `write` recorded.
    unsafe {
        let start = bytes.as_ptr();
        let mut at = start.add(*pos);
        let raw = read_raw_at(&mut at);
        *pos = at.offset_from_unsigned(start);
        raw
    }
}

/// Iterator over the tokens of [`Mappings`] as stored.
#[derive(Debug, Clone)]
pub(crate) struct RawTokens<'a> {
    /// The start of the next token.
    pos: *const u8,
    remaining: u32,
    bytes: std::marker::PhantomData<&'a [u8]>,
}

// SAFETY: `RawTokens` only reads the bytes that its lifetime borrows, like `&[u8]`.
unsafe impl Send for RawTokens<'_> {}
// SAFETY: as above.
unsafe impl Sync for RawTokens<'_> {}

impl Iterator for RawTokens<'_> {
    type Item = RawToken;

    #[inline(always)]
    fn next(&mut self) -> Option<RawToken> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        // SAFETY: `Mappings::bytes` holds `Mappings::len` whole tokens written by
        // `Mappings::write`, and `remaining` counts the ones not read yet.
        Some(unsafe { read_raw_at(&mut self.pos) })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining as usize, Some(self.remaining as usize))
    }
}

/// Iterator over the tokens of [`Mappings`]. Created by [`Mappings::tokens`].
#[derive(Debug, Clone)]
pub struct MappingsIter<'a> {
    raw: RawTokens<'a>,
    state: State,
}

/// The stream was written from `u32` values, so every delta leads back to a `u32`.
#[inline(always)]
fn apply(prev: u32, delta: i64) -> u32 {
    (i64::from(prev) + delta) as u32
}

impl Iterator for MappingsIter<'_> {
    type Item = Token;

    #[inline]
    fn next(&mut self) -> Option<Token> {
        let raw = self.raw.next()?;
        let state = &mut self.state;
        if raw.tag & TAG_LINE != 0 {
            state.dst_line = apply(state.dst_line, raw.dst_line);
            state.dst_col = 0;
        }
        state.dst_col = apply(state.dst_col, raw.dst_col);

        let mut token = Token::unmapped(state.dst_line, state.dst_col);
        if raw.tag & TAG_MAPPED != 0 {
            state.source_id = apply(state.source_id, raw.source_id);
            state.src_line = apply(state.src_line, raw.src_line);
            state.src_col = apply(state.src_col, raw.src_col);
            token.source_id = state.source_id;
            token.src_line = state.src_line;
            token.src_col = state.src_col;
        }
        if raw.tag & TAG_NAMED != 0 {
            state.name_id = apply(state.name_id, raw.name_id);
            token.name_id = state.name_id;
        }
        Some(token)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.raw.size_hint()
    }
}

impl ExactSizeIterator for MappingsIter<'_> {}
impl FusedIterator for MappingsIter<'_> {}

/// The tokens of a source map, decoded and grouped by generated line, for position lookups.
///
/// Created by [`SourceMap::generate_lookup_table`](crate::SourceMap::generate_lookup_table).
/// The table owns its tokens, 24 bytes each: drop it when the lookups are done.
#[derive(Debug, Clone, Default)]
pub struct LookupTable {
    tokens: Vec<Token>,
    /// The index of the first token of each generated line, and the token count at the end.
    line_starts: Vec<u32>,
}

impl LookupTable {
    /// The generated line and column of the tokens always have increasing order.
    pub(crate) fn new(mappings: &Mappings) -> Self {
        if mappings.is_empty() {
            return Self::default();
        }
        let mut tokens = Vec::with_capacity(mappings.len());
        let mut line_starts = Vec::with_capacity(mappings.last_dst_line() as usize + 2);
        line_starts.push(0);
        for token in mappings.tokens() {
            // Lines without tokens are empty ranges.
            while (line_starts.len() as u32) <= token.dst_line {
                line_starts.push(tokens.len() as u32);
            }
            tokens.push(token);
        }
        line_starts.push(tokens.len() as u32);
        Self { tokens, line_starts }
    }

    /// The number of generated lines, up to the last line with a token.
    pub fn len(&self) -> usize {
        self.line_starts.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tokens of generated line `line`, or `None` past the last line with a token.
    #[inline]
    pub fn get(&self, line: usize) -> Option<&[Token]> {
        let start = *self.line_starts.get(line)? as usize;
        let end = *self.line_starts.get(line + 1)? as usize;
        Some(&self.tokens[start..end])
    }

    /// All tokens, in generated order.
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(tokens: &[Token]) -> Mappings {
        let mappings = Mappings::from_tokens(tokens.iter().copied());
        assert_eq!(mappings.len(), tokens.len());
        assert_eq!(mappings.tokens().len(), tokens.len());
        assert_eq!(mappings.tokens().collect::<Vec<_>>(), tokens);
        assert_eq!(mappings.last(), tokens.last().copied());
        mappings
    }

    #[test]
    fn extreme_tokens_round_trip() {
        round_trip(&[]);
        round_trip(&[Token::new(
            u32::MAX - 1,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            Some(u32::MAX - 1),
            None,
        )]);
        round_trip(&[
            Token::unmapped(0, 0),
            Token::new(0, 14, 7, 3, Some(0), Some(2)),
            Token::new(0, 15, 7, 9, Some(0), None),
            Token::new(0, 30, 7, 2, Some(0), Some(0)),
            Token::new(0, 4_000_000, 7, 0, Some(6_000), Some(u32::MAX - 1)),
            Token::unmapped(0, 4_000_001),
            Token::new(3, 0, 0, 0, Some(0), Some(1)),
            Token::new(3, 9_000_000, u32::MAX, u32::MAX, Some(1), None),
            Token::new(u32::MAX, u32::MAX, 0, 0, Some(u32::MAX - 1), Some(0)),
            // Not in generated order.
            Token::new(2, 5, 1, 1, Some(0), None),
            Token::new(2, 1, 1, 0, Some(0), None),
        ]);
    }

    #[test]
    fn small_tokens_are_small() {
        let mappings = round_trip(&[
            Token::new(0, 0, 0, 0, Some(0), None),
            Token::new(0, 4, 0, 4, Some(0), None),
            Token::unmapped(0, 9),
        ]);
        assert_eq!(mappings.byte_len(), 2 + 2 + 1);
    }

    /// Appending must equal pushing the translated tokens one at a time.
    fn check_append(parts: &[(&[Token], u32, u32, u32)]) {
        let mut appended = Mappings::default();
        let mut pushed = Mappings::default();
        for &(tokens, line, source, name) in parts {
            appended.append(&Mappings::from_tokens(tokens.iter().copied()), line, source, name);
            for (i, token) in tokens.iter().enumerate() {
                let token = token.translated(line, source, name);
                if i == 0 && pushed.last() == Some(token) {
                    continue;
                }
                pushed.push(token);
            }
            assert_eq!(appended, pushed);
            assert_eq!(appended.last(), pushed.last());
        }
        assert_eq!(appended.tokens().collect::<Vec<_>>(), pushed.tokens().collect::<Vec<_>>());
    }

    #[test]
    fn append() {
        let a = [
            Token::new(0, 0, 3, 4, Some(1), Some(2)),
            Token::new(0, 5, 3, 9, Some(1), None),
            Token::new(2, 1, 8, 0, Some(0), Some(0)),
        ];
        // First token is neither mapped nor named; the first mapped and first named differ.
        let b = [
            Token::unmapped(0, 2),
            Token::unmapped(1, 0),
            Token::new(1, 4, 0, 0, Some(0), None),
            Token::new(1, 9, 0, 6, Some(0), Some(0)),
            Token::new(4, 0, 2, 0, Some(1), Some(3)),
        ];
        // First mapped token is also the first named token, but not the first token.
        let c = [Token::unmapped(0, 0), Token::new(0, 3, 5, 5, Some(2), Some(1))];
        let unmapped = [Token::unmapped(0, 1), Token::unmapped(3, 0)];

        check_append(&[(&a, 0, 0, 0)]);
        check_append(&[(&a, 0, 0, 0), (&b, 3, 2, 3), (&c, 8, 4, 7), (&a, 9, 7, 9)]);
        check_append(&[(&b, 5, 1, 1), (&unmapped, 10, 3, 4), (&b, 14, 3, 4), (&[], 20, 5, 8)]);
        check_append(&[(&unmapped, 0, 0, 0), (&unmapped, 4, 0, 0), (&c, 8, 0, 0)]);
        check_append(&[(&[], 0, 0, 0), (&c, 2, 1, 1), (&c, 2, 1, 1)]);
    }

    #[test]
    fn append_drops_duplicate_boundary_token() {
        let a = [Token::new(0, 0, 0, 0, Some(0), None), Token::new(1, 0, 1, 0, Some(0), Some(0))];
        let b = [Token::new(0, 0, 1, 0, Some(0), Some(0)), Token::new(0, 6, 1, 6, Some(0), None)];
        let mut mappings = Mappings::from_tokens(a);
        mappings.append(&Mappings::from_tokens(b), 1, 0, 0);
        assert_eq!(mappings.len(), 3);
        check_append(&[(&a, 0, 0, 0), (&b, 1, 0, 0), (&b[..1], 1, 0, 0), (&a, 1, 0, 0)]);
    }

    #[test]
    fn lookup_table_lines() {
        let table = LookupTable::new(&Mappings::from_tokens([
            Token::unmapped(1, 0),
            Token::unmapped(1, 4),
            Token::unmapped(3, 2),
        ]));
        assert_eq!(table.len(), 4);
        assert_eq!(table.get(0), Some(&[][..]));
        assert_eq!(table.get(1).map(<[Token]>::len), Some(2));
        assert_eq!(table.get(2), Some(&[][..]));
        assert_eq!(table.get(3).map(<[Token]>::len), Some(1));
        assert_eq!(table.get(4), None);
        assert!(LookupTable::new(&Mappings::default()).is_empty());
    }
}
