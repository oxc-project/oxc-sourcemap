use std::borrow::Cow;

use crate::{SourceMap, SourceMapParts, Token, token::TokenChunk};

/// The `ConcatSourceMapBuilder` is a helper to concat sourcemaps.
///
/// The lifetime `'a` is the lifetime of the input source maps borrowed during
/// `add_sourcemap` / `from_sourcemaps`: every name/source/sourcesContent
/// string in the concatenated result is a [`Cow::Borrowed`] view into one of
/// the input maps, so concatenation does no string allocations at all. The
/// resulting [`SourceMap<'a>`] cannot outlive its inputs.
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
        // Calculate length of `Vec`s required
        let mut names_len = 0;
        let mut sources_len = 0;
        let mut tokens_len = 0;
        for (sourcemap, _) in sourcemap_and_line_offsets {
            names_len += sourcemap.names.len();
            sources_len += sourcemap.sources.len();
            tokens_len += sourcemap.tokens.len();
        }

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

    pub fn add_sourcemap(&mut self, sourcemap: &'a SourceMap<'_>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;
        let start_token_idx = self.tokens.len() as u32;

        // Capture prev_name_id and prev_source_id before they get updated during token mapping
        let chunk_prev_name_id = self.token_chunk_prev_name_id;
        let chunk_prev_source_id = self.token_chunk_prev_source_id;

        // Borrow strings directly from the input map — no allocations.
        // The output `SourceMap`'s lifetime is tied to `'a`, so the
        // borrow checker enforces that input maps outlive it.
        self.sources.extend(sourcemap.get_sources().map(Cow::Borrowed));
        self.source_contents
            .extend(sourcemap.get_source_contents().map(|opt| opt.map(Cow::Borrowed)));
        self.names.reserve(sourcemap.names.len());
        self.names.extend(sourcemap.get_names().map(Cow::Borrowed));

        // Append every input token to `self.tokens`, translated by `line_offset`,
        // `source_offset`, and `name_offset` so its references resolve against
        // this builder's combined `sources` / `names` arrays.
        //
        // Two pieces of bookkeeping ride along:
        //
        //  1. **Boundary dedup.** If the first input token, after translation,
        //     equals the last token already in `self.tokens`, we drop it. This
        //     collapses the duplicate that appears when two adjacent input
        //     sourcemaps name the same generated position. Only the first
        //     iteration can match — every later token has a distinct
        //     `dst_line` / `dst_col` — so the check is hoisted out of the main
        //     loop into a separate `iter.next()` branch.
        //
        //  2. **Running prev-id state.** `self.token_chunk_prev_source_id` /
        //     `_prev_name_id` track the last source/name id we *committed*, so
        //     the next call's `TokenChunk` header can use them as the VLQ
        //     delta baseline. We accumulate the updates in plain locals
        //     (`last_source_id` / `last_name_id`) inside this call and write
        //     them back to `self.*` once at the end — that keeps the hot loop
        //     register-resident instead of doing `&mut self` stores per token.
        //     Crucially, the locals are only updated when a token is actually
        //     pushed; the dedup-dropped first token must not advance them, or
        //     the next chunk's baseline would diverge from the tokens that
        //     reached the output.
        let mut last_source_id = self.token_chunk_prev_source_id;
        let mut last_name_id = self.token_chunk_prev_name_id;

        self.tokens.reserve(sourcemap.tokens.len());

        // First iteration: build the translated token, check it against
        // `self.tokens.last()` for boundary dedup, and only on a successful
        // push advance the running prev-id state.
        let mut iter = sourcemap.get_tokens();
        if let Some(first) = iter.next() {
            let new_token = translate_token(first, line_offset, source_offset, name_offset);
            if self.tokens.last() != Some(&new_token) {
                update_prev_ids(new_token, &mut last_source_id, &mut last_name_id);
                self.tokens.push(new_token);
            }
        }

        // Remaining tokens are always pushed (no dedup against the boundary),
        // so the prev-id advance happens unconditionally for any token that
        // carries a source/name id.
        for token in iter {
            let new_token = translate_token(token, line_offset, source_offset, name_offset);
            update_prev_ids(new_token, &mut last_source_id, &mut last_name_id);
            self.tokens.push(new_token);
        }

        // Flush the locals back to `self` so the next `add_sourcemap` call
        // sees the prev-id state from the tokens we actually committed.
        self.token_chunk_prev_source_id = last_source_id;
        self.token_chunk_prev_name_id = last_name_id;

        // Add `token_chunks` after tokens are added so we know the actual end index
        let end_token_idx = self.tokens.len() as u32;

        if start_token_idx > 0 {
            // Not the first sourcemap - use previous token's state
            let prev_token = &self.tokens[start_token_idx as usize - 1];
            self.token_chunks.push(TokenChunk::new(
                start_token_idx,
                end_token_idx,
                prev_token.get_dst_line(),
                prev_token.get_dst_col(),
                prev_token.get_src_line(),
                prev_token.get_src_col(),
                chunk_prev_name_id,
                chunk_prev_source_id,
            ));
        } else {
            // First sourcemap - use zeros
            self.token_chunks.push(TokenChunk::new(0, end_token_idx, 0, 0, 0, 0, 0, 0));
        }
    }

    /// Owned counterpart of [`add_sourcemap`](Self::add_sourcemap): moves the input's strings in via [`SourceMap::into_parts`], so the result is `'static` with no `into_owned` copy.
    pub fn add_sourcemap_owned(&mut self, sourcemap: SourceMap<'static>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        let SourceMapParts { names, sources, source_contents, tokens, .. } = sourcemap.into_parts();
        self.sources.extend(sources);
        self.source_contents.extend(source_contents);
        self.names.extend(names);

        self.append_tokens(&tokens, line_offset, source_offset, name_offset);
    }

    /// Translate/dedup/chunk `tokens` into the builder; mirrors the token loop in `add_sourcemap`.
    fn append_tokens(
        &mut self,
        tokens: &[Token],
        line_offset: u32,
        source_offset: u32,
        name_offset: u32,
    ) {
        let start_token_idx = self.tokens.len() as u32;
        let chunk_prev_name_id = self.token_chunk_prev_name_id;
        let chunk_prev_source_id = self.token_chunk_prev_source_id;

        let mut last_source_id = self.token_chunk_prev_source_id;
        let mut last_name_id = self.token_chunk_prev_name_id;

        self.tokens.reserve(tokens.len());

        let mut iter = tokens.iter().copied();
        if let Some(first) = iter.next() {
            let new_token = translate_token(first, line_offset, source_offset, name_offset);
            if self.tokens.last() != Some(&new_token) {
                update_prev_ids(new_token, &mut last_source_id, &mut last_name_id);
                self.tokens.push(new_token);
            }
        }

        for token in iter {
            let new_token = translate_token(token, line_offset, source_offset, name_offset);
            update_prev_ids(new_token, &mut last_source_id, &mut last_name_id);
            self.tokens.push(new_token);
        }

        self.token_chunk_prev_source_id = last_source_id;
        self.token_chunk_prev_name_id = last_name_id;

        let end_token_idx = self.tokens.len() as u32;

        if start_token_idx > 0 {
            let prev_token = &self.tokens[start_token_idx as usize - 1];
            self.token_chunks.push(TokenChunk::new(
                start_token_idx,
                end_token_idx,
                prev_token.get_dst_line(),
                prev_token.get_dst_col(),
                prev_token.get_src_line(),
                prev_token.get_src_col(),
                chunk_prev_name_id,
                chunk_prev_source_id,
            ));
        } else {
            self.token_chunks.push(TokenChunk::new(0, end_token_idx, 0, 0, 0, 0, 0, 0));
        }
    }

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
}

fn translate_token(token: Token, line_offset: u32, source_offset: u32, name_offset: u32) -> Token {
    Token::new(
        token.get_dst_line() + line_offset,
        token.get_dst_col(),
        token.get_src_line(),
        token.get_src_col(),
        token.get_source_id().map(|id| id + source_offset),
        token.get_name_id().map(|id| id + name_offset),
    )
}

fn update_prev_ids(token: Token, last_source_id: &mut u32, last_name_id: &mut u32) {
    if let Some(source_id) = token.get_source_id() {
        *last_source_id = source_id;
    }
    if let Some(name_id) = token.get_name_id() {
        *last_name_id = name_id;
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
fn test_concat_sourcemap_builder_owned_matches_borrowed() {
    // The owned path must produce byte-identical output to the borrowed path.
    let [b1, b2, b3] = build_test_inputs();
    let mut borrowed = ConcatSourceMapBuilder::default();
    for (sm, off) in [(&b1, 0u32), (&b2, 2), (&b3, 2)] {
        borrowed.add_sourcemap(sm, off);
    }
    let borrowed = borrowed.into_sourcemap();

    let [o1, o2, o3] = build_test_inputs();
    let mut owned = ConcatSourceMapBuilder::default();
    for (sm, off) in [(o1, 0u32), (o2, 2), (o3, 2)] {
        owned.add_sourcemap_owned(sm, off);
    }
    let owned = owned.into_sourcemap();

    assert_eq!(owned.tokens, borrowed.tokens);
    assert_eq!(owned.sources, borrowed.sources);
    assert_eq!(owned.names, borrowed.names);
    assert_eq!(owned.source_contents, borrowed.source_contents);
    assert_eq!(owned.token_chunks, borrowed.token_chunks);
    assert_eq!(owned.to_json().mappings, borrowed.to_json().mappings);
    assert_test_result(owned);
}

#[test]
fn test_concat_sourcemap_builder_owned_matches_borrowed_dedup_and_sentinel() {
    // `None` ids exercise the sentinel passthrough; the offsets make map2's first
    // translated token equal map1's last, exercising the boundary-dedup drop.
    fn inputs() -> [SourceMap<'static>; 2] {
        [
            SourceMap::new(
                None,
                vec![],
                None,
                vec![Cow::Borrowed("a.js")],
                vec![],
                vec![Token::new(0, 0, 0, 0, None, None), Token::new(5, 2, 1, 3, None, None)]
                    .into_boxed_slice(),
                None,
            ),
            SourceMap::new(
                None,
                vec![],
                None,
                vec![Cow::Borrowed("b.js")],
                vec![],
                vec![Token::new(0, 2, 1, 3, None, None), Token::new(2, 0, 2, 0, None, None)]
                    .into_boxed_slice(),
                None,
            ),
        ]
    }

    let [b1, b2] = inputs();
    let mut borrowed = ConcatSourceMapBuilder::default();
    borrowed.add_sourcemap(&b1, 0);
    borrowed.add_sourcemap(&b2, 5);
    let borrowed = borrowed.into_sourcemap();

    let [o1, o2] = inputs();
    let mut owned = ConcatSourceMapBuilder::default();
    owned.add_sourcemap_owned(o1, 0);
    owned.add_sourcemap_owned(o2, 5);
    let owned = owned.into_sourcemap();

    // 4 input tokens minus the deduplicated boundary token.
    assert_eq!(borrowed.tokens.len(), 3);
    assert_eq!(owned.tokens, borrowed.tokens);
    assert_eq!(owned.sources, borrowed.sources);
    assert_eq!(owned.token_chunks, borrowed.token_chunks);
    assert_eq!(owned.to_json().mappings, borrowed.to_json().mappings);
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
