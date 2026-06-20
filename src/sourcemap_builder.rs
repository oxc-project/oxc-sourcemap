use std::borrow::Cow;

use rustc_hash::FxHashMap;

use crate::{
    SourceMap,
    token::{Token, TokenChunk},
};

/// Helper to build a [`SourceMap`].
///
/// The builder **borrows** the names, sources and source contents you add for its lifetime `'a`,
/// so building allocates essentially nothing beyond the tokens vector (the dedup maps key by
/// `&'a str`, not owned copies).
///
/// The ownership decision is deferred to the end:
/// * [`into_sourcemap`](Self::into_sourcemap) returns a borrowed [`SourceMap<'a>`] — zero copy.
/// * [`into_owned_sourcemap`](Self::into_owned_sourcemap) copies the strings once into a
///   `'static` [`crate::OwnedSourceMap`].
#[derive(Debug, Default)]
pub struct SourceMapBuilder<'a> {
    pub(crate) file: Option<&'a str>,
    pub(crate) names_map: FxHashMap<&'a str, u32>,
    pub(crate) names: Vec<&'a str>,
    pub(crate) sources: Vec<&'a str>,
    pub(crate) sources_map: FxHashMap<&'a str, u32>,
    pub(crate) source_contents: Vec<Option<&'a str>>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
}

impl<'a> SourceMapBuilder<'a> {
    /// Add a name, deduplicating. The name is borrowed for `'a` (no allocation).
    pub fn add_name(&mut self, name: &'a str) -> u32 {
        if let Some(&id) = self.names_map.get(name) {
            return id;
        }
        let count = self.names.len() as u32;
        self.names_map.insert(name, count);
        self.names.push(name);
        count
    }

    /// Add a source and its content, deduplicating on the source path.
    /// Both are borrowed for `'a` (no allocation). Use this if `source` may be a duplicate.
    pub fn add_source_and_content(&mut self, source: &'a str, source_content: &'a str) -> u32 {
        if let Some(&id) = self.sources_map.get(source) {
            return id;
        }
        let count = self.sources.len() as u32;
        self.sources_map.insert(source, count);
        self.sources.push(source);
        self.source_contents.push(Some(source_content));
        count
    }

    /// Add a source and its content without deduplicating (skips the hash lookup when sources
    /// are unique).
    ///
    /// The source name and source content are borrowed for `'a` — neither is copied.
    pub fn set_source_and_content(&mut self, source: &'a str, source_content: &'a str) -> u32 {
        let count = self.sources.len() as u32;
        self.sources.push(source);
        self.source_contents.push(Some(source_content));
        count
    }

    /// Add item to `SourceMap::tokens`.
    pub fn add_token(
        &mut self,
        dst_line: u32,
        dst_col: u32,
        src_line: u32,
        src_col: u32,
        src_id: Option<u32>,
        name_id: Option<u32>,
    ) {
        self.tokens.push(Token::new(dst_line, dst_col, src_line, src_col, src_id, name_id));
    }

    /// Set the generated file name. Borrowed for `'a` (no allocation).
    pub fn set_file(&mut self, file: &'a str) {
        self.file = Some(file);
    }

    /// Set the `SourceMap::token_chunks` to make the sourcemap to vlq mapping at parallel.
    pub fn set_token_chunks(&mut self, token_chunks: Vec<TokenChunk>) {
        self.token_chunks = Some(token_chunks);
    }

    /// Finish, borrowing the names/sources/contents for `'a` (zero copy).
    pub fn into_sourcemap(mut self) -> SourceMap<'a> {
        // Trade performance for memory.
        // The tokens array take enormously large amount of data,
        // which is not ideal for large applications.
        self.names.shrink_to_fit();
        self.sources.shrink_to_fit();
        // For checker.ts, capacity for `tokens` before and after are 262144 and 171174 respectively.
        self.tokens.shrink_to_fit();
        if let Some(c) = self.token_chunks.as_mut() {
            c.shrink_to_fit()
        }
        SourceMap::new(
            self.file.map(Cow::Borrowed),
            self.names.into_iter().map(Cow::Borrowed).collect(),
            None,
            self.sources.into_iter().map(Cow::Borrowed).collect(),
            self.source_contents.into_iter().map(|content| content.map(Cow::Borrowed)).collect(),
            self.tokens.into_boxed_slice(),
            self.token_chunks,
        )
    }

    /// Same as [`Self::into_sourcemap`], but copies the strings once into an owned
    /// [`crate::OwnedSourceMap`] so callers can store the result without spelling out `'static`.
    #[inline]
    pub fn into_owned_sourcemap(mut self) -> crate::OwnedSourceMap {
        self.names.shrink_to_fit();
        self.sources.shrink_to_fit();
        self.tokens.shrink_to_fit();
        if let Some(c) = self.token_chunks.as_mut() {
            c.shrink_to_fit()
        }
        crate::OwnedSourceMap::new(SourceMap::new(
            self.file.map(|file| Cow::Owned(file.to_owned())),
            self.names.into_iter().map(|name| Cow::Owned(name.to_owned())).collect(),
            None,
            self.sources.into_iter().map(|source| Cow::Owned(source.to_owned())).collect(),
            self.source_contents
                .into_iter()
                .map(|content| content.map(|content| Cow::Owned(content.to_owned())))
                .collect(),
            self.tokens.into_boxed_slice(),
            self.token_chunks,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build() {
        let mut builder = SourceMapBuilder::default();
        builder.set_source_and_content("baz.js", "");
        builder.add_name("x");
        builder.set_file("file");

        let sm = builder.into_sourcemap();
        assert_eq!(sm.get_source(0), Some("baz.js"));
        assert_eq!(sm.get_name(0), Some("x"));
        assert_eq!(sm.get_file(), Some("file"));

        let expected = r#"{"version":3,"file":"file","names":["x"],"sources":["baz.js"],"sourcesContent":[""],"mappings":""}"#;
        assert_eq!(expected, sm.to_json_string());
    }

    #[test]
    fn dedup() {
        let mut builder = SourceMapBuilder::default();
        let id_a = builder.add_name("foo");
        let id_b = builder.add_name("bar");
        let id_a_again = builder.add_name("foo");
        let id_b_again = builder.add_name("bar");
        assert_eq!(id_a, id_a_again);
        assert_eq!(id_b, id_b_again);
        assert_ne!(id_a, id_b);

        let src_a = builder.add_source_and_content("a.js", "content a");
        let src_b = builder.add_source_and_content("b.js", "content b");
        let src_a_again = builder.add_source_and_content("a.js", "different content (ignored)");
        assert_eq!(src_a, src_a_again);
        assert_ne!(src_a, src_b);

        let sm = builder.into_sourcemap();
        assert_eq!(sm.get_names().collect::<Vec<_>>(), vec!["foo", "bar"]);
        assert_eq!(sm.get_sources().collect::<Vec<_>>(), vec!["a.js", "b.js"]);
        // Source content for the first add wins; the second add returns the
        // existing id without overwriting.
        assert_eq!(sm.get_source_content(src_a), Some("content a"));
    }

    #[test]
    fn add_token_and_chunks() {
        let mut builder = SourceMapBuilder::default();
        let name_id = builder.add_name("n");
        let source_id = builder.add_source_and_content("s.js", "src");
        builder.add_token(0, 0, 0, 0, Some(source_id), Some(name_id));
        builder.add_token(0, 4, 0, 4, Some(source_id), None);
        builder.set_token_chunks(vec![TokenChunk::new(0, 2, 0, 0, 0, 0, 0, 0)]);

        let sm = builder.into_sourcemap();
        assert!(sm.token_chunks.is_some());
        assert_eq!(sm.get_tokens().count(), 2);
        assert_eq!(sm.get_token(0), Some(Token::new(0, 0, 0, 0, Some(0), Some(0))));
    }

    #[test]
    fn into_owned_sourcemap() {
        let mut builder = SourceMapBuilder::default();
        builder.set_file("f.js");
        let name_id = builder.add_name("n");
        let source_id = builder.add_source_and_content("s.js", "src");
        builder.add_token(0, 0, 0, 0, Some(source_id), Some(name_id));
        builder.set_token_chunks(vec![TokenChunk::new(0, 1, 0, 0, 0, 0, 0, 0)]);

        let owned = builder.into_owned_sourcemap();
        assert_eq!(owned.get_file(), Some("f.js"));
        assert_eq!(owned.get_name(0), Some("n"));
        assert_eq!(owned.get_source(0), Some("s.js"));
        assert_eq!(owned.get_source_content(0), Some("src"));
        assert_eq!(owned.get_tokens().count(), 1);
    }

    #[test]
    fn into_owned_sourcemap_without_chunks() {
        // No token chunks set: exercises the `None` branch of the chunk shrink.
        let owned = SourceMapBuilder::default().into_owned_sourcemap();
        assert_eq!(owned.get_tokens().count(), 0);
        assert!(owned.as_source_map().token_chunks.is_none());
    }
}
