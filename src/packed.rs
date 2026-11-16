//! A source map that holds its mappings in [`Mappings`]: see [`PackedSourceMap`].

use std::borrow::Cow;

use crate::{
    Mappings, MappingsIter, SourceMap,
    decode::{JSONSourceMap, decode_packed, decode_packed_from_string},
    encode::encode_packed_to_string,
    error::Result,
};

/// A source map that holds its mappings in [`Mappings`], about 3 bytes for each token.
///
/// A [`SourceMap`] holds its mappings as decoded [`Token`](crate::Token)s, 24 bytes each. That
/// is the right shape for lookups, but most maps in a build are only held for most of their
/// life: the map of a module waits for its chunk, and the map of a chunk waits to be written.
/// `PackedSourceMap` is for those maps. It converts to and from JSON and joins with other
/// maps without decoding its tokens, and decodes them only when asked.
///
/// Create one with [`SourceMap::into_packed`], or parse JSON straight into it with
/// [`from_json_string`](Self::from_json_string). Get a [`SourceMap`] back with
/// [`unpack`](Self::unpack) or [`into_sourcemap`](Self::into_sourcemap), or write JSON straight
/// from it with [`to_json_string`](Self::to_json_string).
///
/// The names, the sources and the other strings are held as in [`SourceMap`].
#[derive(Debug, Clone, Default)]
pub struct PackedSourceMap<'a> {
    /// The map without its tokens.
    strings: SourceMap<'a>,
    mappings: Mappings,
}

impl<'a> PackedSourceMap<'a> {
    pub(crate) fn from_raw(strings: SourceMap<'a>, mappings: Mappings) -> Self {
        debug_assert!(strings.tokens.is_empty());
        Self { strings, mappings }
    }

    /// Parse a source map JSON string, like [`SourceMap::from_json_string`]. The mappings are
    /// written straight into [`Mappings`].
    ///
    /// # Errors
    ///
    /// Returns an error for invalid JSON, an unsupported version, malformed mappings,
    /// or invalid source or name references.
    pub fn from_json_string(value: &'a str) -> Result<Self> {
        decode_packed_from_string(value)
    }

    /// Convert from a [`JSONSourceMap`], like [`SourceMap::from_json`]. The mappings are
    /// written straight into [`Mappings`].
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported version, malformed mappings, or invalid source or
    /// name references.
    pub fn from_json(value: JSONSourceMap) -> Result<PackedSourceMap<'static>> {
        decode_packed(value)
    }

    /// Serialize to JSON, like [`SourceMap::to_json_string`], without decoding the tokens.
    pub fn to_json_string(&self) -> String {
        encode_packed_to_string(&self.strings, &self.mappings)
    }

    /// The mappings.
    pub fn mappings(&self) -> &Mappings {
        &self.mappings
    }

    /// Decode the tokens, in the order of the `mappings` field.
    pub fn tokens(&self) -> MappingsIter<'_> {
        self.mappings.tokens()
    }

    /// The number of tokens.
    pub fn token_count(&self) -> usize {
        self.mappings.len()
    }

    /// Create a map from its fields. `mappings` refers to `sources` and `names` by index.
    pub fn new(
        file: Option<Cow<'a, str>>,
        names: Vec<Cow<'a, str>>,
        source_root: Option<Cow<'a, str>>,
        sources: Vec<Option<Cow<'a, str>>>,
        source_contents: Vec<Option<Cow<'a, str>>>,
        mappings: Mappings,
    ) -> Self {
        let strings = SourceMap::new(
            file,
            names,
            source_root,
            sources,
            source_contents,
            Box::default(),
            None,
        );
        Self { strings, mappings }
    }

    /// Serialize to a `data:` URL, like [`SourceMap::to_data_url`].
    pub fn to_data_url(&self) -> String {
        let base_64_str = base64_simd::STANDARD.encode_to_string(self.to_json_string().as_bytes());
        format!("data:application/json;charset=utf-8;base64,{base_64_str}")
    }

    pub fn get_file(&self) -> Option<&str> {
        self.strings.get_file()
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.strings.get_source_root()
    }

    pub fn get_names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.strings.get_names()
    }

    pub fn get_sources(&self) -> impl ExactSizeIterator<Item = Option<&str>> {
        self.strings.get_sources()
    }

    pub fn get_source_contents(&self) -> impl ExactSizeIterator<Item = Option<&str>> {
        self.strings.get_source_contents()
    }

    pub fn get_ignore_list(&self) -> Option<&[u32]> {
        self.strings.get_ignore_list()
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.strings.get_debug_id()
    }

    /// The map without its tokens: `file`, `names`, `sources`, `sourcesContent` and the other
    /// fields that are not mappings.
    pub fn strings(&self) -> &SourceMap<'a> {
        &self.strings
    }

    /// Set the `file` field.
    pub fn set_file(&mut self, file: &str) {
        self.strings.set_file(file);
    }

    /// Set the `sources` field. See [`SourceMap::set_sources`].
    pub fn set_sources<S: AsRef<str>, I: IntoIterator<Item = Option<S>>>(&mut self, sources: I) {
        self.strings.set_sources(sources);
    }

    /// Set the `sourcesContent` field. See [`SourceMap::set_source_contents`].
    pub fn set_source_contents(&mut self, source_contents: Vec<Option<&str>>) {
        self.strings.set_source_contents(source_contents);
    }

    /// Set the `ignoreList` field.
    pub fn set_ignore_list(&mut self, ignore_list: Vec<u32>) {
        self.strings.set_ignore_list(ignore_list);
    }

    /// Set the `debugId` field.
    pub fn set_debug_id(&mut self, debug_id: &str) {
        self.strings.set_debug_id(debug_id);
    }

    /// Decode into a [`SourceMap`] that borrows the strings of this map.
    pub fn unpack(&self) -> SourceMap<'_> {
        let strings = &self.strings;
        SourceMap {
            file: strings.file.as_deref().map(Cow::Borrowed),
            names: strings.names.iter().map(|name| Cow::Borrowed(&**name)).collect(),
            source_root: strings.source_root.as_deref().map(Cow::Borrowed),
            sources: strings.sources.iter().map(|s| s.as_deref().map(Cow::Borrowed)).collect(),
            source_contents: strings
                .source_contents
                .iter()
                .map(|s| s.as_deref().map(Cow::Borrowed))
                .collect(),
            tokens: self.tokens().collect(),
            token_chunks: None,
            ignore_list: strings.ignore_list.clone(),
            debug_id: strings.debug_id.as_deref().map(Cow::Borrowed),
        }
    }

    /// Decode into a [`SourceMap`], moving the strings.
    pub fn into_sourcemap(self) -> SourceMap<'a> {
        let tokens = self.tokens().collect();
        SourceMap { tokens, ..self.strings }
    }

    /// Detach from borrowed input, like [`SourceMap::into_owned`].
    pub fn into_owned(self) -> PackedSourceMap<'static> {
        PackedSourceMap { strings: self.strings.into_owned(), mappings: self.mappings }
    }
}

impl<'a> SourceMap<'a> {
    /// Pack the mappings of this map. See [`PackedSourceMap`].
    ///
    /// The strings are moved. An unmapped token keeps only its generated position.
    pub fn into_packed(mut self) -> PackedSourceMap<'a> {
        let mappings = Mappings::from_tokens(self.tokens.iter().copied());
        self.tokens = Box::default();
        // Token chunks are ranges of token indices, for serializing the tokens in parallel.
        self.token_chunks = None;
        PackedSourceMap { strings: self, mappings }
    }
}

impl<'a> From<SourceMap<'a>> for PackedSourceMap<'a> {
    fn from(map: SourceMap<'a>) -> Self {
        map.into_packed()
    }
}

impl<'a> From<PackedSourceMap<'a>> for SourceMap<'a> {
    fn from(map: PackedSourceMap<'a>) -> Self {
        map.into_sourcemap()
    }
}

/// Joins [`PackedSourceMap`]s, like [`ConcatSourceMapBuilder`](crate::ConcatSourceMapBuilder)
/// joins [`SourceMap`]s.
///
/// The mappings are joined as bytes, see [`Mappings::append`]: the tokens are not decoded, and
/// the result is as compact as its inputs.
///
/// [`add_sourcemap`](Self::add_sourcemap) borrows the strings of a map that outlives the
/// builder, and [`add_sourcemap_owned`](Self::add_sourcemap_owned) moves them in. Neither
/// copies a string.
#[derive(Debug, Default)]
pub struct PackedConcatSourceMapBuilder<'a> {
    names: Vec<Cow<'a, str>>,
    sources: Vec<Option<Cow<'a, str>>>,
    source_contents: Vec<Option<Cow<'a, str>>>,
    mappings: Mappings,
}

impl<'a> PackedConcatSourceMapBuilder<'a> {
    /// Create a builder with room for `names_len` names, `sources_len` sources and
    /// `mappings_byte_len` bytes of mappings: the sum of [`Mappings::byte_len`] of the maps to
    /// add.
    pub fn with_capacity(names_len: usize, sources_len: usize, mappings_byte_len: usize) -> Self {
        Self {
            names: Vec::with_capacity(names_len),
            sources: Vec::with_capacity(sources_len),
            source_contents: Vec::with_capacity(sources_len),
            // A token at the start of each map may be written again, a few bytes longer.
            mappings: Mappings::with_byte_capacity(mappings_byte_len + mappings_byte_len / 64),
        }
    }

    /// Create a builder from maps and the generated line that each one starts at.
    pub fn from_sourcemaps(sourcemap_and_line_offsets: &[(&'a PackedSourceMap<'_>, u32)]) -> Self {
        let (names_len, sources_len, byte_len) = sourcemap_and_line_offsets.iter().fold(
            (0, 0, 0),
            |(names, sources, bytes), (map, _)| {
                (
                    names + map.strings.names.len(),
                    sources + map.strings.sources.len(),
                    bytes + map.mappings.byte_len() + 16,
                )
            },
        );
        let mut builder = Self::with_capacity(names_len, sources_len, byte_len);
        for &(sourcemap, line_offset) in sourcemap_and_line_offsets {
            builder.add_sourcemap(sourcemap, line_offset);
        }
        builder
    }

    /// Add a map whose generated code starts at line `line_offset`, borrowing its strings.
    pub fn add_sourcemap(&mut self, sourcemap: &'a PackedSourceMap<'_>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        let strings = &sourcemap.strings;
        self.sources.extend(strings.get_sources().map(|source| source.map(Cow::Borrowed)));
        self.source_contents
            .extend(strings.get_source_contents().map(|content| content.map(Cow::Borrowed)));
        self.pad_source_contents();
        self.names.extend(strings.get_names().map(Cow::Borrowed));

        self.mappings.append(&sourcemap.mappings, line_offset, source_offset, name_offset);
    }

    /// Add a map whose generated code starts at line `line_offset`, moving its strings in.
    pub fn add_sourcemap_owned(&mut self, sourcemap: PackedSourceMap<'a>, line_offset: u32) {
        let source_offset = self.sources.len() as u32;
        let name_offset = self.names.len() as u32;

        let PackedSourceMap { strings, mappings } = sourcemap;
        let parts = strings.into_parts();
        self.sources.extend(parts.sources);
        self.source_contents.extend(parts.source_contents);
        self.pad_source_contents();
        self.names.extend(parts.names);

        self.mappings.append(&mappings, line_offset, source_offset, name_offset);
    }

    /// Contents are indexed by source id, and a map may have fewer contents than sources.
    /// Pad with `None` so that the contents of the next map line up with its sources.
    fn pad_source_contents(&mut self) {
        self.source_contents.resize(self.sources.len(), None);
    }

    /// Finish, moving the names, sources and contents into a [`PackedSourceMap`].
    pub fn into_sourcemap(mut self) -> PackedSourceMap<'a> {
        self.mappings.shrink_to_fit();
        let strings = SourceMap::new(
            None,
            self.names,
            None,
            self.sources,
            self.source_contents,
            Box::default(),
            None,
        );
        PackedSourceMap { strings, mappings: self.mappings }
    }
}
