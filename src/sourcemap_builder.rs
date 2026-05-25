use std::borrow::Cow;

use rustc_hash::FxHashMap;

use crate::{
    SourceMap,
    token::{Token, TokenChunk},
};

/// The `SourceMapBuilder` is a helper to generate sourcemap.
///
/// All strings added via the builder are owned by the builder; the resulting
/// [`SourceMap`] is therefore [`SourceMap<'static>`].
#[derive(Debug, Default)]
pub struct SourceMapBuilder {
    pub(crate) file: Option<Cow<'static, str>>,
    pub(crate) names_map: FxHashMap<Cow<'static, str>, u32>,
    pub(crate) names: Vec<Cow<'static, str>>,
    pub(crate) sources: Vec<Cow<'static, str>>,
    pub(crate) sources_map: FxHashMap<Cow<'static, str>, u32>,
    pub(crate) source_contents: Vec<Option<Cow<'static, str>>>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
}

impl SourceMapBuilder {
    /// Add item to `SourceMap::name`.
    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.names_map.get(name) {
            return id;
        }
        let count = self.names.len() as u32;
        let name: Cow<'static, str> = Cow::Owned(name.to_owned());
        self.names_map.insert(name.clone(), count);
        self.names.push(name);
        count
    }

    /// Add item to `SourceMap::sources` and `SourceMap::source_contents`.
    /// If `source` maybe duplicate, please use it.
    pub fn add_source_and_content(&mut self, source: &str, source_content: &str) -> u32 {
        if let Some(&id) = self.sources_map.get(source) {
            return id;
        }
        let count = self.sources.len() as u32;
        let source: Cow<'static, str> = Cow::Owned(source.to_owned());
        self.sources_map.insert(source.clone(), count);
        self.sources.push(source);
        self.source_contents.push(Some(Cow::Owned(source_content.to_owned())));
        count
    }

    /// Add item to `SourceMap::sources` and `SourceMap::source_contents`.
    /// If `source` hasn't duplicate，it will avoid extra hash calculation.
    pub fn set_source_and_content(&mut self, source: &str, source_content: &str) -> u32 {
        let count = self.sources.len() as u32;
        self.sources.push(Cow::Owned(source.to_owned()));
        self.source_contents.push(Some(Cow::Owned(source_content.to_owned())));
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

    pub fn set_file(&mut self, file: &str) {
        self.file = Some(Cow::Owned(file.to_owned()));
    }

    /// Set the `SourceMap::token_chunks` to make the sourcemap to vlq mapping at parallel.
    pub fn set_token_chunks(&mut self, token_chunks: Vec<TokenChunk>) {
        self.token_chunks = Some(token_chunks);
    }

    pub fn into_sourcemap(mut self) -> SourceMap<'static> {
        // Trade performance for memory.
        // The tokens array take enormously large amount of data,
        // which is not ideal for large applications.
        self.names_map.shrink_to_fit();
        self.names.shrink_to_fit();
        self.sources.shrink_to_fit();
        self.sources_map.shrink_to_fit();
        // For checker.ts, capacity for `tokens` before and after are 262144 and 171174 respectively.
        self.tokens.shrink_to_fit();
        if let Some(c) = self.token_chunks.as_mut() {
            c.shrink_to_fit()
        }
        SourceMap::new(
            self.file,
            self.names,
            None,
            self.sources,
            self.source_contents,
            self.tokens.into_boxed_slice(),
            self.token_chunks,
        )
    }
}

#[test]
fn test_sourcemap_builder() {
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
