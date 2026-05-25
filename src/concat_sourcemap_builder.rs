use crate::{
    SourceMap, Token,
    sourcemap::{OptionalStrRef, StrRef},
    sourcemap_builder::StringInterner,
    token::TokenChunk,
};

/// The `ConcatSourceMapBuilder` is a helper to concat sourcemaps.
#[derive(Debug, Default)]
pub struct ConcatSourceMapBuilder {
    interner: StringInterner,
    names: Vec<StrRef>,
    sources: Vec<StrRef>,
    source_contents: Vec<OptionalStrRef>,
    tokens: Vec<Token>,
    token_chunks: Vec<TokenChunk>,
    token_chunk_prev_source_id: u32,
    token_chunk_prev_name_id: u32,
}

impl ConcatSourceMapBuilder {
    /// Create new `ConcatSourceMapBuilder` with pre-allocated capacity.
    pub fn with_capacity(
        names_len: usize,
        sources_len: usize,
        tokens_len: usize,
        token_chunks_len: usize,
    ) -> Self {
        Self {
            interner: StringInterner::default(),
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
    pub fn from_sourcemaps(sourcemap_and_line_offsets: &[(&SourceMap, u32)]) -> Self {
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

    pub fn add_sourcemap(&mut self, sourcemap: &SourceMap, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;
        let start_token_idx = self.tokens.len() as u32;

        // Capture prev_name_id and prev_source_id before they get updated during token mapping
        let chunk_prev_name_id = self.token_chunk_prev_name_id;
        let chunk_prev_source_id = self.token_chunk_prev_source_id;

        // Copy each name/source/sources_content into the builder's buffer.
        self.sources.extend(sourcemap.get_sources().map(|s| self.interner.intern_unique(s)));
        self.source_contents.extend(sourcemap.get_source_contents().map(|opt| match opt {
            Some(s) => self.interner.intern_unique(s).into(),
            None => OptionalStrRef::NONE,
        }));
        self.names.reserve(sourcemap.names.len());
        self.names.extend(sourcemap.get_names().map(|s| self.interner.intern_unique(s)));

        // Extend `tokens`, skipping the first token if it duplicates the last existing one.
        self.tokens.reserve(sourcemap.tokens.len());
        for (i, token) in sourcemap.get_tokens().enumerate() {
            let new_token = Token::new(
                token.get_dst_line() + line_offset,
                token.get_dst_col(),
                token.get_src_line(),
                token.get_src_col(),
                token.get_source_id().map(|x| {
                    self.token_chunk_prev_source_id = x + source_offset;
                    self.token_chunk_prev_source_id
                }),
                token.get_name_id().map(|x| {
                    self.token_chunk_prev_name_id = x + name_offset;
                    self.token_chunk_prev_name_id
                }),
            );
            if i == 0 && self.tokens.last() == Some(&new_token) {
                continue;
            }
            self.tokens.push(new_token);
        }

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

    pub fn into_sourcemap(self) -> SourceMap {
        SourceMap {
            buf: self.interner.into_buf(),
            file: OptionalStrRef::NONE,
            source_root: OptionalStrRef::NONE,
            debug_id: OptionalStrRef::NONE,
            names: self.names.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            source_contents: self.source_contents.into_boxed_slice(),
            tokens: self.tokens.into_boxed_slice(),
            token_chunks: Some(self.token_chunks),
            x_google_ignore_list: None,
        }
    }
}

#[cfg(test)]
fn build_test_inputs() -> [SourceMap; 3] {
    [
        SourceMap::new(
            None,
            vec!["foo", "foo2"],
            None,
            vec!["foo.js"],
            vec![],
            vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
        SourceMap::new(
            None,
            vec!["bar"],
            None,
            vec!["bar.js"],
            vec![],
            vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
        SourceMap::new(
            None,
            vec!["abc"],
            None,
            vec!["abc.js"],
            vec![],
            vec![Token::new(1, 2, 2, 2, Some(0), Some(0))].into_boxed_slice(),
            None,
        ),
    ]
}

#[cfg(test)]
fn assert_test_result(concat_sm: SourceMap) {
    let expected_tokens: Box<[Token]> = vec![
        Token::new(1, 1, 1, 1, Some(0), Some(0)),
        Token::new(3, 1, 1, 1, Some(1), Some(2)),
        Token::new(3, 2, 2, 2, Some(2), Some(3)),
    ]
    .into_boxed_slice();

    assert_eq!(concat_sm.tokens, expected_tokens);
    let names: Vec<&str> = concat_sm.get_names().collect();
    assert_eq!(names, vec!["foo", "foo2", "bar", "abc"]);
    let sources: Vec<&str> = concat_sm.get_sources().collect();
    assert_eq!(sources, vec!["foo.js", "bar.js", "abc.js"]);
    assert_eq!(
        concat_sm.token_chunks,
        Some(vec![
            TokenChunk::new(0, 1, 0, 0, 0, 0, 0, 0,),
            TokenChunk::new(1, 2, 1, 1, 1, 1, 0, 0,),
            TokenChunk::new(2, 3, 3, 1, 1, 1, 2, 1,)
        ])
    );

    // Verify mapping serialization is the same as a baseline built via the new() ctor.
    let expected = SourceMap::new(
        None,
        vec!["foo", "foo2", "bar", "abc"],
        None,
        vec!["foo.js", "bar.js", "abc.js"],
        vec![],
        expected_tokens,
        None,
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
fn test_concat_sourcemap_builder_deduplicates_tokens() {
    let sm1 = SourceMap::new(
        None,
        vec!["name1"],
        None,
        vec!["file1.js"],
        vec![],
        vec![Token::new(1, 1, 1, 1, Some(0), Some(0)), Token::new(2, 5, 2, 5, Some(0), Some(0))]
            .into_boxed_slice(),
        None,
    );

    let sm2 = SourceMap::new(
        None,
        vec!["name2"],
        None,
        vec!["file2.js"],
        vec![],
        vec![Token::new(2, 5, 2, 5, Some(0), Some(0)), Token::new(3, 10, 3, 10, Some(0), Some(0))]
            .into_boxed_slice(),
        None,
    );

    let mut builder = ConcatSourceMapBuilder::default();
    builder.add_sourcemap(&sm1, 0);
    builder.add_sourcemap(&sm2, 0);

    let concat_sm = builder.into_sourcemap();
    assert_eq!(concat_sm.tokens.len(), 4);
}
