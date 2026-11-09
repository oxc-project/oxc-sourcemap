use crate::{SourceMap, Token};

/// Lookups through a cursor mostly move forward along a line, so a lookup scans this many tokens
/// from the previous result before it falls back to binary search.
const FORWARD_SCAN: usize = 8;

/// A stateful cursor for many token lookups in one [`SourceMap`].
///
/// Created by [`SourceMap::lookup_cursor`]. It owns the map's lookup table and remembers the
/// line and token index of its previous result. When the next query is on the same line and every
/// token before that index is before the queried column, the search starts there: a short forward
/// scan, then a binary search over the rest of the line. Otherwise it searches the whole line.
/// This makes runs of mostly-forward queries, such as tracing every token of one map through
/// another when composing sourcemaps, cheaper than independent lookups.
///
/// The previous result only changes where the search starts, never its answer:
/// [`lookup_token`](Self::lookup_token) and [`lookup_token_approx`](Self::lookup_token_approx)
/// return exactly what [`SourceMap::lookup_token`] and [`SourceMap::lookup_token_approx`]
/// return for the same map, in any query order, as long as each line's tokens are sorted by
/// generated column.
///
/// Like those lookups, the cursor relies on that order. Maps decoded from JSON always have it,
/// because decoding sorts out-of-order lines. A map built from tokens, for example with
/// [`SourceMap::new`], must provide it: lookups on a line that is not sorted by column give
/// unspecified results, and the cursor's can differ from the stateless lookups'.
///
/// ```
/// use oxc_sourcemap::SourceMapBuilder;
///
/// let mut builder = SourceMapBuilder::default();
/// let source = builder.add_source_and_content("a.js", "");
/// builder.add_token(0, 2, 0, 0, Some(source), None);
/// builder.add_token(0, 6, 0, 4, Some(source), None);
/// let map = builder.into_sourcemap();
///
/// let mut cursor = map.lookup_cursor();
/// // Clamps to the line's first token.
/// assert_eq!(cursor.lookup_token_approx(0, 0).map(|t| t.get_dst_col()), Some(2));
/// // The last token before the column.
/// assert_eq!(cursor.lookup_token_approx(0, 5).map(|t| t.get_dst_col()), Some(2));
/// assert_eq!(cursor.lookup_token_approx(0, 9).map(|t| t.get_dst_col()), Some(6));
/// // The exact lookup does not clamp.
/// assert_eq!(cursor.lookup_token(0, 0), None);
/// ```
#[derive(Debug, Clone)]
pub struct LookupCursor<'a> {
    lines: Vec<&'a [Token]>,
    /// The line and token index of the previous result.
    last: Option<(u32, usize)>,
}

impl<'a> LookupCursor<'a> {
    pub(crate) fn new(map: &'a SourceMap<'_>) -> Self {
        Self { lines: map.generate_lookup_table(), last: None }
    }

    /// Lookup a token by line and column, like [`SourceMap::lookup_token`].
    ///
    /// Returns the first token at `col` if there is one, else the last token before it. Returns
    /// `None` when `col` is before the line's first token, or the line is empty or missing.
    #[inline]
    pub fn lookup_token(&mut self, line: u32, col: u32) -> Option<Token> {
        let (tokens, next) = self.search(line, col)?;
        let index = if tokens.get(next).is_some_and(|token| token.dst_col == col) {
            next
        } else {
            next.checked_sub(1)?
        };
        self.select(line, tokens, index)
    }

    /// Lookup a token by line and column, like [`SourceMap::lookup_token_approx`].
    ///
    /// Returns the first token at `col` if there is one, else the last token before it, clamped
    /// to the line's first token. Returns `None` when the line is empty or missing.
    #[inline]
    pub fn lookup_token_approx(&mut self, line: u32, col: u32) -> Option<Token> {
        let (tokens, next) = self.search(line, col)?;
        let index = if tokens.get(next).is_some_and(|token| token.dst_col == col) {
            next
        } else {
            next.saturating_sub(1)
        };
        self.select(line, tokens, index)
    }

    /// Returns `line`'s tokens and the index of the first token at or after `col`.
    #[inline]
    fn search(&self, line: u32, col: u32) -> Option<(&'a [Token], usize)> {
        let tokens = *self.lines.get(line as usize)?;
        // The search may start at the previous result only when every token before it is before
        // `col`, so a previous result never changes the answer.
        let mut next = match self.last {
            Some((last_line, index))
                if last_line == line && (index == 0 || tokens[index - 1].dst_col < col) =>
            {
                index
            }
            _ => 0,
        };
        // Bound the scan so distant jumps cannot turn a lookup into a linear search.
        let end = tokens.len().min(next + FORWARD_SCAN);
        while next < end && tokens[next].dst_col < col {
            next += 1;
        }
        if next == end {
            next += tokens[next..].partition_point(|token| token.dst_col < col);
        }
        Some((tokens, next))
    }

    /// Returns the token at `index`, remembering it as the previous result. An empty line has no
    /// token.
    #[inline]
    fn select(&mut self, line: u32, tokens: &[Token], index: usize) -> Option<Token> {
        let token = *tokens.get(index)?;
        self.last = Some((line, index));
        Some(token)
    }
}
