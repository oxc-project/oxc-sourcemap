use rustc_hash::FxHashMap;

use crate::{
    SourceMap,
    sourcemap::{OptionalStrRef, StrRef},
    token::{Token, TokenChunk},
};

/// Interns strings into a single growing buffer.
///
/// Each [`intern`](StringInterner::intern) call appends the string to the
/// buffer and returns a [`StrRef`] (start/end byte offsets). Lookup-by-content
/// is done via a side `FxHashMap<Box<str>, StrRef>` (the `Box<str>` keys are
/// the only per-string allocations during building; they're dropped at
/// `into_buf` time).
#[derive(Debug, Default)]
pub(crate) struct StringInterner {
    buf: String,
    intern_map: FxHashMap<Box<str>, StrRef>,
}

impl StringInterner {
    /// Intern `s`, returning a `StrRef` pointing into the eventual buffer.
    /// Identical strings are deduplicated.
    pub(crate) fn intern(&mut self, s: &str) -> StrRef {
        if let Some(&r) = self.intern_map.get(s) {
            return r;
        }
        let start = self.buf.len() as u32;
        self.buf.push_str(s);
        let end = self.buf.len() as u32;
        let r = StrRef { start, end };
        self.intern_map.insert(Box::from(s), r);
        r
    }

    /// Intern `s` without deduplicating (skip the hashmap lookup).
    ///
    /// Use when the caller already knows the string is unique.
    pub(crate) fn intern_unique(&mut self, s: &str) -> StrRef {
        let start = self.buf.len() as u32;
        self.buf.push_str(s);
        let end = self.buf.len() as u32;
        StrRef { start, end }
    }

    /// Consume the interner and return the final buffer.
    pub(crate) fn into_buf(self) -> Box<str> {
        self.buf.into_boxed_str()
    }
}

/// The `SourceMapBuilder` is a helper to generate sourcemap.
#[derive(Debug, Default)]
pub struct SourceMapBuilder {
    pub(crate) interner: StringInterner,
    pub(crate) file: OptionalStrRef,
    pub(crate) names: Vec<StrRef>,
    pub(crate) names_id_map: FxHashMap<Box<str>, u32>,
    pub(crate) sources: Vec<StrRef>,
    pub(crate) sources_id_map: FxHashMap<Box<str>, u32>,
    pub(crate) source_contents: Vec<OptionalStrRef>,
    pub(crate) tokens: Vec<Token>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
}

impl SourceMapBuilder {
    /// Add item to `SourceMap::name`.
    pub fn add_name(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.names_id_map.get(name) {
            return id;
        }
        let count = self.names.len() as u32;
        let r = self.interner.intern(name);
        self.names.push(r);
        self.names_id_map.insert(Box::from(name), count);
        count
    }

    /// Add item to `SourceMap::sources` and `SourceMap::source_contents`.
    /// If `source` maybe duplicate, please use it.
    pub fn add_source_and_content(&mut self, source: &str, source_content: &str) -> u32 {
        if let Some(&id) = self.sources_id_map.get(source) {
            return id;
        }
        let count = self.sources.len() as u32;
        let r = self.interner.intern(source);
        self.sources.push(r);
        let cr = self.interner.intern(source_content);
        self.source_contents.push(cr.into());
        self.sources_id_map.insert(Box::from(source), count);
        count
    }

    /// Add item to `SourceMap::sources` and `SourceMap::source_contents`.
    /// If `source` hasn't duplicate，it will avoid extra hash calculation.
    pub fn set_source_and_content(&mut self, source: &str, source_content: &str) -> u32 {
        let count = self.sources.len() as u32;
        let r = self.interner.intern_unique(source);
        self.sources.push(r);
        let cr = self.interner.intern(source_content);
        self.source_contents.push(cr.into());
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
        self.file = self.interner.intern(file).into();
    }

    /// Set the `SourceMap::token_chunks` to make the sourcemap to vlq mapping at parallel.
    pub fn set_token_chunks(&mut self, token_chunks: Vec<TokenChunk>) {
        self.token_chunks = Some(token_chunks);
    }

    pub fn into_sourcemap(mut self) -> SourceMap {
        // The tokens array takes the bulk of the memory; shrink to fit so
        // the final SourceMap doesn't carry the builder's growth slack.
        self.tokens.shrink_to_fit();
        if let Some(c) = self.token_chunks.as_mut() {
            c.shrink_to_fit()
        }
        // Drop the dedup maps so their `Box<str>` keys are freed; the
        // strings still live in the interner's buffer.
        drop(self.names_id_map);
        drop(self.sources_id_map);
        SourceMap {
            buf: self.interner.into_buf(),
            file: self.file,
            source_root: OptionalStrRef::NONE,
            debug_id: OptionalStrRef::NONE,
            names: self.names.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            source_contents: self.source_contents.into_boxed_slice(),
            tokens: self.tokens.into_boxed_slice(),
            token_chunks: self.token_chunks,
            x_google_ignore_list: None,
        }
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
