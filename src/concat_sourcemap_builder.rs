use std::borrow::Cow;

use crate::{SourceMap, Token, token::TokenChunk};

/// The `ConcatSourceMapBuilder` is a helper to concat sourcemaps.
///
/// Names/sources/sourcesContent are accumulated as [`Cow<'a, str>`], so the builder serves two
/// input shapes without ever copying string bytes during the join:
///
/// * [`add_sourcemap`](Self::add_sourcemap) / [`from_sourcemaps`](Self::from_sourcemaps) **borrow**
///   from input maps that outlive the builder (`Cow::Borrowed`).
/// * [`add_sourcemap_owned`](Self::add_sourcemap_owned) /
///   [`from_owned_sourcemaps`](Self::from_owned_sourcemaps) **consume** owned input maps and
///   **move** their strings in (`Cow::Owned`) — no copy, ideal when the inputs are discarded
///   afterwards (e.g. a bundler joining freshly-rendered modules).
///
/// The ownership decision is deferred to the end:
/// * [`into_sourcemap`](Self::into_sourcemap) moves the accumulated entries straight into a
///   [`SourceMap<'a>`] — zero copy either way.
/// * [`into_owned_sourcemap`](Self::into_owned_sourcemap) detaches to `'static`: entries moved in
///   via `add_sourcemap_owned` are kept as-is, only borrowed ones are copied.
#[derive(Debug, Default)]
pub struct ConcatSourceMapBuilder<'a> {
    pub(crate) names: Vec<Cow<'a, str>>,
    pub(crate) sources: Vec<Cow<'a, str>>,
    pub(crate) source_contents: Vec<Option<Cow<'a, str>>>,
    pub(crate) tokens: Vec<Token>,
    /// The `token_chunks` is used for encode tokens to vlq mappings at parallel.
    pub(crate) token_chunks: Vec<TokenChunk>,
    pub(crate) token_chunk_prev_source_id: u32,
    pub(crate) token_chunk_prev_name_id: u32,
}

impl<'a> ConcatSourceMapBuilder<'a> {
    /// Create new `ConcatSourceMapBuilder` with pre-allocated capacity.
    ///
    /// Allocating capacity before adding sourcemaps with `add_sourcemap` avoids memory copies
    /// and increases performance.
    ///
    /// Alternatively, use `from_sourcemaps`.
    pub fn with_capacity(
        names_len: usize,
        sources_len: usize,
        tokens_len: usize,
        token_chunks_len: usize,
    ) -> Self {
        Self {
            names: Vec::with_capacity(names_len),
            sources: Vec::with_capacity(sources_len),
            source_contents: Vec::with_capacity(sources_len),
            tokens: Vec::with_capacity(tokens_len),
            token_chunks: Vec::with_capacity(token_chunks_len),
            token_chunk_prev_source_id: 0,
            token_chunk_prev_name_id: 0,
        }
    }

    /// Sum the `names` / `sources` / `tokens` lengths across `maps`, to pre-size the builder.
    fn sum_lengths<'s, 'd: 's>(
        maps: impl Iterator<Item = &'s SourceMap<'d>>,
    ) -> (usize, usize, usize) {
        let (mut names, mut sources, mut tokens) = (0, 0, 0);
        for map in maps {
            names += map.names.len();
            sources += map.sources.len();
            tokens += map.tokens.len();
        }
        (names, sources, tokens)
    }

    /// Pad `source_contents` with `None` so it stays index-aligned with `sources`. An input map's
    /// `sourcesContent` may be absent or shorter than its `sources` (it is not normalized on
    /// decode), and contents are indexed by source id — without this, a later map's content would
    /// shift onto an earlier map's source. A no-op when the map's contents already cover its
    /// sources (the common case), so it costs nothing there.
    fn pad_source_contents(&mut self) {
        self.source_contents.resize(self.sources.len(), None);
    }

    /// Create new `ConcatSourceMapBuilder` from an array of `SourceMap`s and line offsets.
    ///
    /// This avoids memory copies versus creating builder with `ConcatSourceMapBuilder::default()`
    /// and then adding sourcemaps individually with `add_sourcemap`.
    ///
    /// # Example
    /// ```no_run
    /// # use oxc_sourcemap::ConcatSourceMapBuilder;
    /// # use oxc_sourcemap::SourceMap;
    /// # let sourcemap1 = SourceMap::default();
    /// # let sourcemap2 = SourceMap::default();
    /// let builder = ConcatSourceMapBuilder::from_sourcemaps(&[
    ///   (&sourcemap1, 0),
    ///   (&sourcemap2, 100),
    /// ]);
    /// let combined_sourcemap = builder.into_sourcemap();
    /// ```
    pub fn from_sourcemaps(sourcemap_and_line_offsets: &[(&'a SourceMap<'_>, u32)]) -> Self {
        let (names_len, sources_len, tokens_len) =
            Self::sum_lengths(sourcemap_and_line_offsets.iter().map(|(sourcemap, _)| *sourcemap));
        let mut builder = Self::with_capacity(
            names_len,
            sources_len,
            tokens_len,
            sourcemap_and_line_offsets.len(),
        );

        for (sourcemap, line_offset) in sourcemap_and_line_offsets.iter().copied() {
            builder.add_sourcemap(sourcemap, line_offset);
        }

        builder
    }

    /// Create a `ConcatSourceMapBuilder` from **owned** `SourceMap`s and line offsets, **moving**
    /// their strings in (no copy).
    ///
    /// The owned counterpart of [`from_sourcemaps`](Self::from_sourcemaps) — see
    /// [`add_sourcemap_owned`](Self::add_sourcemap_owned). This is the fastest way to join owned
    /// maps that are discarded afterwards (e.g. a bundler joining rendered modules): no
    /// name/source/sourcesContent string is copied.
    pub fn from_owned_sourcemaps(sourcemap_and_line_offsets: Vec<(SourceMap<'a>, u32)>) -> Self {
        let (names_len, sources_len, tokens_len) =
            Self::sum_lengths(sourcemap_and_line_offsets.iter().map(|(sourcemap, _)| sourcemap));
        let mut builder = Self::with_capacity(
            names_len,
            sources_len,
            tokens_len,
            sourcemap_and_line_offsets.len(),
        );

        for (sourcemap, line_offset) in sourcemap_and_line_offsets {
            builder.add_sourcemap_owned(sourcemap, line_offset);
        }

        builder
    }

    /// Add a **borrowed** `SourceMap`'s entries to the concatenation, offset by `line_offset`.
    ///
    /// Strings are borrowed from `sourcemap` for `'a` (no allocation), so the result cannot
    /// outlive it. Use [`add_sourcemap_owned`](Self::add_sourcemap_owned) when you own the map and
    /// want to move its strings in instead.
    pub fn add_sourcemap(&mut self, sourcemap: &'a SourceMap<'_>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        // Borrow strings directly from the input map — no allocations. The output `SourceMap`'s
        // lifetime is tied to `'a`, so the borrow checker enforces that input maps outlive it.
        self.sources.extend(sourcemap.get_sources().map(Cow::Borrowed));
        self.source_contents
            .extend(sourcemap.get_source_contents().map(|content| content.map(Cow::Borrowed)));
        self.pad_source_contents();
        self.names.extend(sourcemap.get_names().map(Cow::Borrowed));

        self.add_tokens(&sourcemap.tokens, line_offset, source_offset, name_offset);
    }

    /// Add an **owned** `SourceMap` to the concatenation, **moving** its strings in (no copy),
    /// offset by `line_offset`.
    ///
    /// This is the fastest path when the inputs are owned and discarded after the join: the
    /// name/source/sourcesContent `Cow::Owned` heap buffers are moved across rather than copied,
    /// so the cost is independent of how much `sourcesContent` rides along. Pair with
    /// [`into_sourcemap`](Self::into_sourcemap), which then moves the entries straight into the
    /// result (and, for `'static` inputs, returns a `'static` map without any copy).
    pub fn add_sourcemap_owned(&mut self, sourcemap: SourceMap<'a>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        let parts = sourcemap.into_parts();

        // Move the owned entries in — no string bytes are copied.
        self.sources.extend(parts.sources);
        self.source_contents.extend(parts.source_contents);
        self.pad_source_contents();
        self.names.extend(parts.names);

        self.add_tokens(&parts.tokens, line_offset, source_offset, name_offset);
    }

    /// Append `tokens` to `self.tokens`, translated by `line_offset` / `source_offset` /
    /// `name_offset` so they resolve against the combined `sources` / `names` arrays, and record
    /// the matching [`TokenChunk`]. Shared by `add_sourcemap` (borrowed) and `add_sourcemap_owned`
    /// (owned) — only how the strings get in differs.
    fn add_tokens(
        &mut self,
        tokens: &[Token],
        line_offset: u32,
        source_offset: u32,
        name_offset: u32,
    ) {
        let start = self.tokens.len();
        // The chunk header records the prev-id baseline as it stood *before* this chunk.
        let chunk_prev_source_id = self.token_chunk_prev_source_id;
        let chunk_prev_name_id = self.token_chunk_prev_name_id;

        if start == 0 && line_offset == 0 && source_offset == 0 && name_offset == 0 {
            // Genuinely the first contributing map: no line/source/name offset, and no previous
            // token to dedup against, so every token is unchanged — copy them in one `memcpy`.
            // (A prior map can add sources/names without tokens, leaving `start == 0` while the
            // offsets are non-zero, so all four conditions must hold to skip translation.)
            self.tokens.extend_from_slice(tokens);
        } else {
            self.tokens.reserve(tokens.len());
            let mut tokens = tokens.iter();
            // Boundary dedup: only the first token can equal the previous map's last token (every
            // later token has a distinct generated position), so check it once and drop if equal.
            if let Some(first) = tokens.next() {
                let first = first.translated(line_offset, source_offset, name_offset);
                if self.tokens.last() != Some(&first) {
                    self.tokens.push(first);
                }
            }
            self.tokens.extend(
                tokens.map(|token| token.translated(line_offset, source_offset, name_offset)),
            );
        }

        // The next chunk's VLQ baseline is the last source/name id committed. Scan back from the
        // end of what we just appended — the final token almost always carries both, so this is
        // typically O(1); if this map contributed neither, the previous baseline carries over.
        let mut prev_source_id = chunk_prev_source_id;
        let mut prev_name_id = chunk_prev_name_id;
        let (mut have_source, mut have_name) = (false, false);
        for token in self.tokens[start..].iter().rev() {
            if !have_source && let Some(id) = token.get_source_id() {
                prev_source_id = id;
                have_source = true;
            }
            if !have_name && let Some(id) = token.get_name_id() {
                prev_name_id = id;
                have_name = true;
            }
            if have_source && have_name {
                break;
            }
        }
        self.token_chunk_prev_source_id = prev_source_id;
        self.token_chunk_prev_name_id = prev_name_id;

        // Record the chunk once boundary dedup has settled the actual end index.
        let end = self.tokens.len() as u32;
        let chunk = if start > 0 {
            let prev = &self.tokens[start - 1];
            TokenChunk::new(
                start as u32,
                end,
                prev.get_dst_line(),
                prev.get_dst_col(),
                prev.get_src_line(),
                prev.get_src_col(),
                chunk_prev_name_id,
                chunk_prev_source_id,
            )
        } else {
            TokenChunk::new(0, end, 0, 0, 0, 0, 0, 0)
        };
        self.token_chunks.push(chunk);
    }

    /// Finish, moving the accumulated names/sources/contents straight into a [`SourceMap<'a>`]
    /// (zero copy — the `Cow` vectors are moved, not rebuilt).
    pub fn into_sourcemap(self) -> SourceMap<'a> {
        SourceMap::new(
            None,
            self.names,
            None,
            self.sources,
            self.source_contents,
            self.tokens.into_boxed_slice(),
            Some(self.token_chunks),
        )
    }

    /// Same as [`Self::into_sourcemap`], but detaches to a `'static` [`crate::OwnedSourceMap`].
    /// Entries moved in via [`add_sourcemap_owned`](Self::add_sourcemap_owned) are kept as-is
    /// (no copy); only borrowed entries (from [`add_sourcemap`](Self::add_sourcemap)) are copied.
    #[inline]
    pub fn into_owned_sourcemap(self) -> crate::OwnedSourceMap {
        self.into_sourcemap().into_owned_sourcemap()
    }
}

#[cfg(test)]
fn build_test_inputs() -> [SourceMap<'static>; 3] {
    [
        SourceMap::new(
            None,
            vec![Cow::Borrowed("foo"), Cow::Borrowed("foo2")],
            None,
            vec![Cow::Borrowed("foo.js")],
            vec![],
            vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
        SourceMap::new(
            None,
            vec![Cow::Borrowed("bar")],
            None,
            vec![Cow::Borrowed("bar.js")],
            vec![],
            vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
        SourceMap::new(
            None,
            vec![Cow::Borrowed("abc")],
            None,
            vec![Cow::Borrowed("abc.js")],
            vec![],
            vec![Token::new(1, 2, 2, 2, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
    ]
}

#[cfg(test)]
fn assert_test_result(concat_sm: SourceMap<'_>) {
    let expected = SourceMap::new(
        None,
        vec![
            Cow::Borrowed("foo"),
            Cow::Borrowed("foo2"),
            Cow::Borrowed("bar"),
            Cow::Borrowed("abc"),
        ],
        None,
        vec![Cow::Borrowed("foo.js"), Cow::Borrowed("bar.js"), Cow::Borrowed("abc.js")],
        vec![],
        vec![
            Token::new(1, 1, 1, 1, Some(0), Some(0)),
            Token::new(3, 1, 1, 1, Some(1), Some(2)),
            Token::new(3, 2, 2, 2, Some(2), Some(3)),
        ]
        .into_boxed_slice(),
        None,
    );

    assert_eq!(concat_sm.tokens, expected.tokens);
    assert_eq!(concat_sm.sources, expected.sources);
    assert_eq!(concat_sm.names, expected.names);
    assert_eq!(
        concat_sm.token_chunks,
        Some(vec![
            TokenChunk::new(0, 1, 0, 0, 0, 0, 0, 0,),
            TokenChunk::new(1, 2, 1, 1, 1, 1, 0, 0,),
            TokenChunk::new(2, 3, 3, 1, 1, 1, 2, 1,)
        ])
    );

    assert_eq!(expected.to_json().mappings, concat_sm.to_json().mappings);
}

#[test]
fn test_concat_sourcemap_builder() {
    let [sm1, sm2, sm3] = build_test_inputs();
    let inputs = [(&sm1, 0u32), (&sm2, 2), (&sm3, 2)];
    let mut builder = ConcatSourceMapBuilder::default();
    for (sourcemap, line_offset) in inputs.iter().copied() {
        builder.add_sourcemap(sourcemap, line_offset);
    }
    assert_test_result(builder.into_sourcemap());
}

#[test]
fn test_concat_sourcemap_builder_from_sourcemaps() {
    let [sm1, sm2, sm3] = build_test_inputs();
    let builder = ConcatSourceMapBuilder::from_sourcemaps(&[(&sm1, 0), (&sm2, 2), (&sm3, 2)]);
    assert_test_result(builder.into_sourcemap());
}

#[test]
fn test_concat_sourcemap_builder_add_sourcemap_owned() {
    // Moving owned maps in must produce the same result as borrowing them.
    let [sm1, sm2, sm3] = build_test_inputs();
    let mut builder = ConcatSourceMapBuilder::default();
    builder.add_sourcemap_owned(sm1, 0);
    builder.add_sourcemap_owned(sm2, 2);
    builder.add_sourcemap_owned(sm3, 2);
    assert_test_result(builder.into_sourcemap());
}

#[test]
fn test_concat_sourcemap_builder_from_owned_sourcemaps() {
    let [sm1, sm2, sm3] = build_test_inputs();
    let builder = ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(sm1, 0), (sm2, 2), (sm3, 2)]);
    assert_test_result(builder.into_sourcemap());
}

#[test]
fn test_concat_owned_moves_strings_into_owned_sourcemap() {
    // Two owned maps with owned content; the move-in path must preserve every string and
    // renumber `other`'s ids/lines, ending up as a `'static` map with no data lost.
    let a = SourceMap::new(
        None,
        vec![Cow::Owned("name_a".to_string())],
        None,
        vec![Cow::Owned("a.js".to_string())],
        vec![Some(Cow::Owned("a content".to_string()))],
        vec![Token::new(0, 0, 0, 0, Some(0), Some(0))].into_boxed_slice(),
        None,
    );
    let b = SourceMap::new(
        None,
        vec![Cow::Owned("name_b".to_string())],
        None,
        vec![Cow::Owned("b.js".to_string())],
        vec![Some(Cow::Owned("b content".to_string()))],
        vec![Token::new(0, 0, 0, 0, Some(0), Some(0))].into_boxed_slice(),
        None,
    );

    let owned =
        ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(a, 0), (b, 5)]).into_owned_sourcemap();

    assert_eq!(owned.get_sources().collect::<Vec<_>>(), vec!["a.js", "b.js"]);
    assert_eq!(owned.get_names().collect::<Vec<_>>(), vec!["name_a", "name_b"]);
    assert_eq!(owned.get_source_content(0), Some("a content"));
    assert_eq!(owned.get_source_content(1), Some("b content"));
    // `b`'s token is shifted by the line offset and renumbered onto the combined ids.
    assert_eq!(owned.get_token(1), Some(Token::new(5, 0, 0, 0, Some(1), Some(1))));
}

#[test]
fn test_concat_owned_translates_after_tokenless_map() {
    // A first map that contributes a source but no tokens leaves `self.tokens` empty while the
    // source offset advances; the next map must still renumber its ids (not take the memcpy path).
    let tokenless = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Owned("a.js".to_string())],
        vec![Some(Cow::Owned("a content".to_string()))],
        vec![].into_boxed_slice(),
        None,
    );
    let with_token = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Owned("b.js".to_string())],
        vec![Some(Cow::Owned("b content".to_string()))],
        // Source id 0 within its own map; after concat it must point at "b.js" (combined index 1).
        vec![Token::new(0, 0, 0, 0, Some(0), None)].into_boxed_slice(),
        None,
    );

    let map = ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(tokenless, 0), (with_token, 0)])
        .into_sourcemap();

    assert_eq!(map.get_sources().collect::<Vec<_>>(), vec!["a.js", "b.js"]);
    assert_eq!(map.get_token(0).unwrap().get_source_id(), Some(1));
    assert_eq!(map.get_source_content(1), Some("b content"));
}

#[test]
fn test_concat_owned_pads_missing_source_contents() {
    // First map has a source but no `sourcesContent`; the second map's content must stay attached
    // to its own source rather than shifting onto the first map's source id.
    let no_content = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Owned("a.js".to_string())],
        vec![],
        vec![Token::new(0, 0, 0, 0, Some(0), None)].into_boxed_slice(),
        None,
    );
    let with_content = SourceMap::new(
        None,
        vec![],
        None,
        vec![Cow::Owned("b.js".to_string())],
        vec![Some(Cow::Owned("b content".to_string()))],
        vec![Token::new(0, 0, 0, 0, Some(0), None)].into_boxed_slice(),
        None,
    );

    let map =
        ConcatSourceMapBuilder::from_owned_sourcemaps(vec![(no_content, 0), (with_content, 1)])
            .into_sourcemap();

    assert_eq!(map.get_sources().collect::<Vec<_>>(), vec!["a.js", "b.js"]);
    assert_eq!(map.get_source_content(0), None);
    assert_eq!(map.get_source_content(1), Some("b content"));
}

#[test]
fn test_concat_sourcemap_builder_deduplicates_tokens() {
    // Test that duplicate tokens at concatenation boundaries are removed
    // For tokens to be truly identical after concatenation, they must have:
    // - Same dst_line (after line_offset)
    // - Same dst_col
    // - Same src_line, src_col
    // - Same source_id and name_id (after source_offset/name_offset)

    // This is difficult to create naturally, so we test the scenario where
    // no deduplication should happen (tokens are different)
    let sm1 = SourceMap::new(
        None,
        vec![Cow::Borrowed("name1")],
        None,
        vec![Cow::Borrowed("file1.js")],
        vec![],
        vec![Token::new(1, 1, 1, 1, Some(0), Some(0)), Token::new(2, 5, 2, 5, Some(0), Some(0))]
            .into_boxed_slice(),
        None,
    );

    // sm2 has different source_id/name_id after offset, so won't deduplicate
    let sm2 = SourceMap::new(
        None,
        vec![Cow::Borrowed("name2")],
        None,
        vec![Cow::Borrowed("file2.js")],
        vec![],
        vec![
            Token::new(2, 5, 2, 5, Some(0), Some(0)), // Different source/name after offset
            Token::new(3, 10, 3, 10, Some(0), Some(0)),
        ]
        .into_boxed_slice(),
        None,
    );

    let mut builder = ConcatSourceMapBuilder::default();
    builder.add_sourcemap(&sm1, 0);
    builder.add_sourcemap(&sm2, 0);

    let concat_sm = builder.into_sourcemap();

    // Should have 4 tokens (no deduplication because source_id/name_id differ)
    assert_eq!(concat_sm.tokens.len(), 4);
}
