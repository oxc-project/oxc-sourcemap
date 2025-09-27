use std::sync::Arc;

use crate::{SourceMap, Token, token::TokenChunk};

/// The `ConcatSourceMapBuilder` is a helper to concat sourcemaps.
#[derive(Debug, Default)]
pub struct ConcatSourceMapBuilder {
    pub(crate) names: Vec<Arc<str>>,
    pub(crate) sources: Vec<Arc<str>>,
    pub(crate) source_contents: Vec<Option<Arc<str>>>,
    pub(crate) tokens: Vec<Token>,
    /// The `token_chunks` is used for encode tokens to vlq mappings at parallel.
    pub(crate) token_chunks: Vec<TokenChunk>,
    pub(crate) token_chunk_prev_source_id: u32,
    pub(crate) token_chunk_prev_name_id: u32,
}

impl ConcatSourceMapBuilder {
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
    /// ```
    /// let builder = ConcatSourceMapBuilder::from_sourcemaps(&[
    ///   (&sourcemap1, 0),
    ///   (&sourcemap2, 100),
    /// ]);
    /// let combined_sourcemap = builder.into_sourcemap();
    /// ```
    pub fn from_sourcemaps(sourcemap_and_line_offsets: &[(&SourceMap, u32)]) -> Self {
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

    pub fn add_sourcemap(&mut self, sourcemap: &SourceMap, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        // Add `token_chunks`, See `TokenChunk`.
        if let Some(last_token) = self.tokens.last() {
            self.token_chunks.push(TokenChunk::new(
                self.tokens.len() as u32,
                self.tokens.len() as u32 + sourcemap.tokens.len() as u32,
                last_token.get_dst_line(),
                last_token.get_dst_col(),
                last_token.get_src_line(),
                last_token.get_src_col(),
                self.token_chunk_prev_name_id,
                self.token_chunk_prev_source_id,
            ));
        } else {
            self.token_chunks.push(TokenChunk::new(
                0,
                sourcemap.tokens.len() as u32,
                0,
                0,
                0,
                0,
                0,
                0,
            ));
        }

        // Extend `sources` and `source_contents`.
        self.sources.extend(sourcemap.get_sources().map(Arc::clone));

        // Clone `Arc` instead of generating a new `Arc` and copying string data because
        // source texts are generally long strings. Cost of copying a large string is higher
        // than cloning an `Arc`.
        self.source_contents.extend(sourcemap.source_contents.iter().cloned());

        // Extend `names`.
        self.names.reserve(sourcemap.names.len());
        self.names.extend(sourcemap.get_names().map(Arc::clone));

        // Extend `tokens`.
        self.tokens.reserve(sourcemap.tokens.len());
        let tokens = sourcemap.get_tokens().map(|token| {
            Token::new(
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
            )
        });
        self.tokens.extend(tokens);
    }

    pub fn into_sourcemap(self) -> SourceMap {
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

#[test]
fn test_concat_sourcemap_builder() {
    run_test(|sourcemap_and_line_offsets| {
        let mut builder = ConcatSourceMapBuilder::default();
        for (sourcemap, line_offset) in sourcemap_and_line_offsets.iter().copied() {
            builder.add_sourcemap(sourcemap, line_offset);
        }
        builder
    });
}

#[test]
fn test_concat_sourcemap_builder_from_sourcemaps() {
    run_test(ConcatSourceMapBuilder::from_sourcemaps);
}

#[cfg(test)]
fn run_test<F>(create_builder: F)
where
    F: Fn(&[(&SourceMap, u32)]) -> ConcatSourceMapBuilder,
{
    let sm1 = SourceMap::new(
        None,
        vec!["foo".into(), "foo2".into()],
        None,
        vec!["foo.js".into()],
        vec![],
        vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
        None,
    );
    let sm2 = SourceMap::new(
        None,
        vec!["bar".into()],
        None,
        vec!["bar.js".into()],
        vec![],
        vec![Token::new(1, 1, 1, 1, Some(0), Some(0))].into_boxed_slice(),
        None,
    );
    let sm3 = SourceMap::new(
        None,
        vec!["abc".into()],
        None,
        vec!["abc.js".into()],
        vec![],
        vec![Token::new(1, 2, 2, 2, Some(0), Some(0))].into_boxed_slice(),
        None,
    );

    let builder = create_builder(&[(&sm1, 0), (&sm2, 2), (&sm3, 2)]);

    let sm = SourceMap::new(
        None,
        vec!["foo".into(), "foo2".into(), "bar".into(), "abc".into()],
        None,
        vec!["foo.js".into(), "bar.js".into(), "abc.js".into()],
        vec![],
        vec![
            Token::new(1, 1, 1, 1, Some(0), Some(0)),
            Token::new(3, 1, 1, 1, Some(1), Some(2)),
            Token::new(3, 2, 2, 2, Some(2), Some(3)),
        ].into_boxed_slice(),
        None,
    );
    let concat_sm = builder.into_sourcemap();

    assert_eq!(concat_sm.tokens, sm.tokens);
    assert_eq!(concat_sm.sources, sm.sources);
    assert_eq!(concat_sm.names, sm.names);
    assert_eq!(
        concat_sm.token_chunks,
        Some(vec![
            TokenChunk::new(0, 1, 0, 0, 0, 0, 0, 0,),
            TokenChunk::new(1, 2, 1, 1, 1, 1, 0, 0,),
            TokenChunk::new(2, 3, 3, 1, 1, 1, 2, 1,)
        ])
    );

    assert_eq!(sm.to_json().mappings, concat_sm.to_json().mappings);
}
