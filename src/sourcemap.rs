use std::borrow::Cow;

use crate::{
    SourceViewToken,
    decode::{JSONSourceMap, decode, decode_from_string, decode_from_string_borrowed},
    encode::{encode, encode_to_string},
    error::Result,
    token::{Token, TokenChunk},
};

/// A parsed source map.
///
/// All string-typed fields (`names`, `sources`, `sources_content`, `file`,
/// `source_root`, `debug_id`) are stored as offsets into a single contiguous
/// [`Box<str>`] buffer ([`SourceMap::buf`]). This collapses what was
/// previously an `Arc<str>` per string into one allocation per `SourceMap`,
/// regardless of how many names/sources/contents it holds. The accessors
/// continue to return `&str`; the borrow is into the map's own buffer, so
/// no lifetime parameter is needed.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// All string data concatenated. `StrRef::start`/`end` are byte offsets
    /// into this buffer.
    pub(crate) buf: Box<str>,
    pub(crate) file: OptionalStrRef,
    pub(crate) source_root: OptionalStrRef,
    pub(crate) debug_id: OptionalStrRef,
    pub(crate) names: Box<[StrRef]>,
    pub(crate) sources: Box<[StrRef]>,
    pub(crate) source_contents: Box<[OptionalStrRef]>,
    pub(crate) tokens: Box<[Token]>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
    /// Identifies third-party sources (such as framework code or bundler-generated code), allowing developers to avoid code that they don't want to see or step through, without having to configure this beforehand.
    /// The `x_google_ignoreList` field refers to the `sources` array, and lists the indices of all the known third-party sources in that source map.
    /// When parsing the source map, developer tools can use this to determine sections of the code that the browser loads and runs that could be automatically ignore-listed.
    pub(crate) x_google_ignore_list: Option<Vec<u32>>,
}

/// A reference to a substring of [`SourceMap::buf`], represented as
/// `start..end` byte offsets. 8 bytes total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct StrRef {
    pub start: u32,
    pub end: u32,
}

impl StrRef {
    #[inline]
    pub(crate) fn resolve(self, buf: &str) -> &str {
        // SAFETY: ranges are always populated from `buf.len()` checkpoints,
        // and the buffer is immutable once assembled.
        unsafe { buf.get_unchecked(self.start as usize..self.end as usize) }
    }

    #[inline]
    #[expect(dead_code)]
    pub(crate) fn len(self) -> u32 {
        self.end - self.start
    }
}

/// An optional `StrRef`, packed into 8 bytes via the sentinel `start = u32::MAX`.
/// Lets `Box<[OptionalStrRef]>` stay tight (8 bytes/entry) instead of the
/// 12-byte `Option<StrRef>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionalStrRef {
    start: u32,
    end: u32,
}

impl OptionalStrRef {
    pub(crate) const NONE: Self = Self { start: u32::MAX, end: 0 };

    #[inline]
    pub(crate) fn some(r: StrRef) -> Self {
        debug_assert!(r.start != u32::MAX);
        Self { start: r.start, end: r.end }
    }

    #[inline]
    pub(crate) fn as_option(self) -> Option<StrRef> {
        if self.start == u32::MAX {
            None
        } else {
            Some(StrRef { start: self.start, end: self.end })
        }
    }

    #[inline]
    pub(crate) fn resolve<'a>(self, buf: &'a str) -> Option<&'a str> {
        self.as_option().map(|r| r.resolve(buf))
    }

    #[inline]
    pub(crate) fn is_some(self) -> bool {
        self.start != u32::MAX
    }
}

impl Default for OptionalStrRef {
    #[inline]
    fn default() -> Self {
        Self::NONE
    }
}

impl SourceMap {
    /// Construct a `SourceMap` from owned components. Strings are copied
    /// into the internal buffer; offsets are recorded.
    pub fn new(
        file: Option<&str>,
        names: Vec<&str>,
        source_root: Option<&str>,
        sources: Vec<&str>,
        source_contents: Vec<Option<&str>>,
        tokens: Box<[Token]>,
        token_chunks: Option<Vec<TokenChunk>>,
    ) -> Self {
        let mut interner = crate::sourcemap_builder::StringInterner::default();
        let file = file.map(|s| interner.intern_unique(s)).map_or(OptionalStrRef::NONE, Into::into);
        let source_root =
            source_root.map(|s| interner.intern_unique(s)).map_or(OptionalStrRef::NONE, Into::into);
        let names: Box<[StrRef]> = names.into_iter().map(|s| interner.intern_unique(s)).collect();
        let sources: Box<[StrRef]> =
            sources.into_iter().map(|s| interner.intern_unique(s)).collect();
        let source_contents: Box<[OptionalStrRef]> = source_contents
            .into_iter()
            .map(|opt| match opt {
                Some(s) => interner.intern_unique(s).into(),
                None => OptionalStrRef::NONE,
            })
            .collect();
        Self {
            buf: interner.into_buf(),
            file,
            source_root,
            debug_id: OptionalStrRef::NONE,
            names,
            sources,
            source_contents,
            tokens,
            token_chunks,
            x_google_ignore_list: None,
        }
    }

    /// Convert the vlq sourcemap to to `SourceMap`.
    /// # Errors
    ///
    /// The `serde_json` deserialize Error.
    pub fn from_json(value: JSONSourceMap) -> Result<Self> {
        decode(value)
    }

    /// Convert the vlq sourcemap string to `SourceMap`.
    /// # Errors
    ///
    /// The `serde_json` deserialize Error.
    pub fn from_json_string(value: &str) -> Result<Self> {
        decode_from_string(value)
    }

    /// Convert `SourceMap` to vlq sourcemap.
    pub fn to_json(&self) -> JSONSourceMap {
        encode(self)
    }

    /// Replace this map's `tokens` array, leaving every other field (and
    /// the underlying string buffer) untouched. Useful for token-level
    /// transforms like line shifting where the strings don't change.
    ///
    /// Drops any existing `token_chunks` since they would now reference
    /// invalid positions.
    pub fn set_tokens(&mut self, tokens: Box<[Token]>) {
        self.tokens = tokens;
        self.token_chunks = None;
    }

    /// Convert `SourceMap` to vlq sourcemap string.
    pub fn to_json_string(&self) -> String {
        encode_to_string(self)
    }

    /// Convert `SourceMap` to vlq sourcemap data url.
    pub fn to_data_url(&self) -> String {
        let base_64_str = base64_simd::STANDARD.encode_to_string(self.to_json_string().as_bytes());
        format!("data:application/json;charset=utf-8;base64,{base_64_str}")
    }

    pub fn get_file(&self) -> Option<&str> {
        self.file.resolve(&self.buf)
    }

    pub fn set_file(&mut self, file: &str) {
        let mut rebuild = SourceMapRebuild::from(self);
        rebuild.file = rebuild.builder.intern(file).into();
        *self = rebuild.into_sourcemap();
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.source_root.resolve(&self.buf)
    }

    pub fn get_x_google_ignore_list(&self) -> Option<&[u32]> {
        self.x_google_ignore_list.as_deref()
    }

    /// Set `x_google_ignoreList`.
    pub fn set_x_google_ignore_list(&mut self, x_google_ignore_list: Vec<u32>) {
        self.x_google_ignore_list = Some(x_google_ignore_list);
    }

    pub fn set_debug_id(&mut self, debug_id: &str) {
        let mut rebuild = SourceMapRebuild::from(self);
        rebuild.debug_id = rebuild.builder.intern(debug_id).into();
        *self = rebuild.into_sourcemap();
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.debug_id.resolve(&self.buf)
    }

    pub fn get_names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(|r| r.resolve(&self.buf))
    }

    /// Adjust `sources`.
    pub fn set_sources<S: AsRef<str>, I: IntoIterator<Item = S>>(&mut self, sources: I) {
        let mut rebuild = SourceMapRebuild::from(self);
        rebuild.sources = sources
            .into_iter()
            .map(|s| rebuild.builder.intern(s.as_ref()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        *self = rebuild.into_sourcemap();
    }

    pub fn get_sources(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().map(|r| r.resolve(&self.buf))
    }

    /// Adjust `source_content`.
    pub fn set_source_contents(&mut self, source_contents: Vec<Option<&str>>) {
        let mut rebuild = SourceMapRebuild::from(self);
        rebuild.source_contents = source_contents
            .into_iter()
            .map(|opt| match opt {
                Some(s) => rebuild.builder.intern(s).into(),
                None => OptionalStrRef::NONE,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        *self = rebuild.into_sourcemap();
    }

    pub fn get_source_contents(&self) -> impl Iterator<Item = Option<&str>> {
        self.source_contents.iter().map(|r| r.resolve(&self.buf))
    }

    pub fn get_token(&self, index: u32) -> Option<Token> {
        self.tokens.get(index as usize).copied()
    }

    pub fn get_source_view_token(&self, index: u32) -> Option<SourceViewToken<'_>> {
        self.tokens.get(index as usize).copied().map(|token| SourceViewToken::new(token, self))
    }

    /// Get raw tokens.
    pub fn get_tokens(&self) -> impl Iterator<Item = Token> {
        self.tokens.iter().copied()
    }

    /// Get source view tokens. See [`SourceViewToken`] for more information.
    pub fn get_source_view_tokens(&self) -> impl Iterator<Item = SourceViewToken<'_>> {
        self.tokens.iter().map(|&token| SourceViewToken::new(token, self))
    }

    pub fn get_name(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(|r| r.resolve(&self.buf))
    }

    pub fn get_source(&self, id: u32) -> Option<&str> {
        self.sources.get(id as usize).map(|r| r.resolve(&self.buf))
    }

    pub fn get_source_content(&self, id: u32) -> Option<&str> {
        self.source_contents.get(id as usize).and_then(|r| r.resolve(&self.buf))
    }

    pub fn get_source_and_content(&self, id: u32) -> Option<(&str, &str)> {
        let source = self.get_source(id)?;
        let content = self.get_source_content(id)?;
        Some((source, content))
    }

    /// Generate a lookup table, it will be used at `lookup_token` or `lookup_source_view_token`.
    pub fn generate_lookup_table(&self) -> Vec<LineLookupTable<'_>> {
        // The dst line/dst col always has increasing order.
        if let Some(last_token) = self.tokens.last() {
            let mut table = vec![&self.tokens[..0]; last_token.dst_line as usize + 1];
            let mut prev_start_idx = 0u32;
            let mut prev_dst_line = 0u32;
            for (idx, token) in self.tokens.iter().enumerate() {
                if token.dst_line != prev_dst_line {
                    table[prev_dst_line as usize] = &self.tokens[prev_start_idx as usize..idx];
                    prev_start_idx = idx as u32;
                    prev_dst_line = token.dst_line;
                }
            }
            table[prev_dst_line as usize] = &self.tokens[prev_start_idx as usize..];
            table
        } else {
            vec![]
        }
    }

    /// Lookup a token by line and column, it will used at remapping.
    pub fn lookup_token(
        &self,
        lookup_table: &[LineLookupTable],
        line: u32,
        col: u32,
    ) -> Option<Token> {
        // If the line is greater than the number of lines in the lookup table, it hasn't corresponding origin token.
        if line >= lookup_table.len() as u32 {
            return None;
        }
        let token = greatest_lower_bound(lookup_table[line as usize], &(line, col), |token| {
            (token.dst_line, token.dst_col)
        })?;
        Some(*token)
    }

    /// Lookup a token by line and column, it will used at remapping. See `SourceViewToken`.
    pub fn lookup_source_view_token(
        &self,
        lookup_table: &[LineLookupTable],
        line: u32,
        col: u32,
    ) -> Option<SourceViewToken<'_>> {
        self.lookup_token(lookup_table, line, col).map(|token| SourceViewToken::new(token, self))
    }
}

impl From<StrRef> for OptionalStrRef {
    #[inline]
    fn from(r: StrRef) -> Self {
        Self::some(r)
    }
}

/// In-place rebuilder used by the `set_*` methods. Since string data lives
/// in a single immutable `Box<str>`, any mutation of a string-typed field
/// requires assembling a fresh buffer with the existing strings re-interned.
pub(crate) struct SourceMapRebuild {
    pub(crate) builder: crate::sourcemap_builder::StringInterner,
    pub(crate) file: OptionalStrRef,
    pub(crate) source_root: OptionalStrRef,
    pub(crate) debug_id: OptionalStrRef,
    pub(crate) names: Box<[StrRef]>,
    pub(crate) sources: Box<[StrRef]>,
    pub(crate) source_contents: Box<[OptionalStrRef]>,
    pub(crate) tokens: Box<[Token]>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
    pub(crate) x_google_ignore_list: Option<Vec<u32>>,
}

impl SourceMapRebuild {
    fn from(sm: &SourceMap) -> Self {
        let mut builder = crate::sourcemap_builder::StringInterner::default();
        let file = reintern_optional(&mut builder, sm.file, &sm.buf);
        let source_root = reintern_optional(&mut builder, sm.source_root, &sm.buf);
        let debug_id = reintern_optional(&mut builder, sm.debug_id, &sm.buf);
        let names = sm
            .names
            .iter()
            .map(|r| builder.intern(r.resolve(&sm.buf)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let sources = sm
            .sources
            .iter()
            .map(|r| builder.intern(r.resolve(&sm.buf)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let source_contents = sm
            .source_contents
            .iter()
            .map(|r| reintern_optional(&mut builder, *r, &sm.buf))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            builder,
            file,
            source_root,
            debug_id,
            names,
            sources,
            source_contents,
            tokens: sm.tokens.clone(),
            token_chunks: sm.token_chunks.clone(),
            x_google_ignore_list: sm.x_google_ignore_list.clone(),
        }
    }

    fn into_sourcemap(self) -> SourceMap {
        SourceMap {
            buf: self.builder.into_buf(),
            file: self.file,
            source_root: self.source_root,
            debug_id: self.debug_id,
            names: self.names,
            sources: self.sources,
            source_contents: self.source_contents,
            tokens: self.tokens,
            token_chunks: self.token_chunks,
            x_google_ignore_list: self.x_google_ignore_list,
        }
    }
}

fn reintern_optional(
    builder: &mut crate::sourcemap_builder::StringInterner,
    r: OptionalStrRef,
    buf: &str,
) -> OptionalStrRef {
    match r.resolve(buf) {
        Some(s) => builder.intern(s).into(),
        None => OptionalStrRef::NONE,
    }
}

/// A zero-copy parsed source map, holding `Cow` views into an input JSON
/// buffer.
///
/// Most usage should prefer the owned [`SourceMap`] — it has no lifetime
/// parameter and packs all strings into a single buffer. `BorrowedSourceMap`
/// exists for the niche case where you want to parse a sourcemap and
/// immediately read from it without paying for a string-copy into the
/// owned representation:
///
/// ```
/// # use oxc_sourcemap::BorrowedSourceMap;
/// # fn read(_: &str) {}
/// let json = r#"{"version":3,"sources":[],"names":[],"mappings":""}"#;
/// let borrowed = BorrowedSourceMap::from_json_string(json).unwrap();
/// for token in borrowed.get_tokens() {
///     // ... read tokens, no string allocations beyond what serde_json did ...
/// }
/// // Promote to owned if you need to store it past `json`'s lifetime:
/// let owned = borrowed.into_owned();
/// ```
#[derive(Debug, Clone, Default)]
pub struct BorrowedSourceMap<'a> {
    pub(crate) file: Option<Cow<'a, str>>,
    pub(crate) source_root: Option<Cow<'a, str>>,
    pub(crate) debug_id: Option<Cow<'a, str>>,
    pub(crate) names: Vec<Cow<'a, str>>,
    pub(crate) sources: Vec<Cow<'a, str>>,
    pub(crate) source_contents: Vec<Option<Cow<'a, str>>>,
    pub(crate) tokens: Box<[Token]>,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
    pub(crate) x_google_ignore_list: Option<Vec<u32>>,
}

impl<'a> BorrowedSourceMap<'a> {
    /// Parse a sourcemap JSON string without copying strings into an owned
    /// buffer. Each name / source / sourcesContent entry becomes a
    /// `Cow::Borrowed` view into `value` when no JSON escapes are present,
    /// or a `Cow::Owned` `String` when serde_json had to unescape.
    ///
    /// # Errors
    /// Returns `serde_json` and VLQ decode errors.
    pub fn from_json_string(value: &'a str) -> Result<Self> {
        decode_from_string_borrowed(value)
    }

    /// Copy every string into a single owned [`SourceMap::buf`] and return
    /// the owned form. `Cow::Owned` entries are moved without copying their
    /// `String` data; only `Cow::Borrowed` entries trigger a `memcpy`.
    pub fn into_owned(self) -> SourceMap {
        let mut interner = crate::sourcemap_builder::StringInterner::default();
        let file = match self.file {
            Some(c) => interner.intern_unique(&c).into(),
            None => OptionalStrRef::NONE,
        };
        let source_root = match self.source_root {
            Some(c) => interner.intern_unique(&c).into(),
            None => OptionalStrRef::NONE,
        };
        let debug_id = match self.debug_id {
            Some(c) => interner.intern_unique(&c).into(),
            None => OptionalStrRef::NONE,
        };
        let names: Box<[StrRef]> =
            self.names.into_iter().map(|c| interner.intern_unique(&c)).collect();
        let sources: Box<[StrRef]> =
            self.sources.into_iter().map(|c| interner.intern_unique(&c)).collect();
        let source_contents: Box<[OptionalStrRef]> = self
            .source_contents
            .into_iter()
            .map(|opt| match opt {
                Some(c) => interner.intern_unique(&c).into(),
                None => OptionalStrRef::NONE,
            })
            .collect();
        SourceMap {
            buf: interner.into_buf(),
            file,
            source_root,
            debug_id,
            names,
            sources,
            source_contents,
            tokens: self.tokens,
            token_chunks: self.token_chunks,
            x_google_ignore_list: self.x_google_ignore_list,
        }
    }

    pub fn get_file(&self) -> Option<&str> {
        self.file.as_deref()
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.source_root.as_deref()
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.debug_id.as_deref()
    }

    pub fn get_x_google_ignore_list(&self) -> Option<&[u32]> {
        self.x_google_ignore_list.as_deref()
    }

    pub fn get_name(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(AsRef::as_ref)
    }

    pub fn get_source(&self, id: u32) -> Option<&str> {
        self.sources.get(id as usize).map(AsRef::as_ref)
    }

    pub fn get_source_content(&self, id: u32) -> Option<&str> {
        self.source_contents.get(id as usize).and_then(|opt| opt.as_deref())
    }

    pub fn get_source_and_content(&self, id: u32) -> Option<(&str, &str)> {
        let source = self.get_source(id)?;
        let content = self.get_source_content(id)?;
        Some((source, content))
    }

    pub fn get_names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(AsRef::as_ref)
    }

    pub fn get_sources(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().map(AsRef::as_ref)
    }

    pub fn get_source_contents(&self) -> impl Iterator<Item = Option<&str>> {
        self.source_contents.iter().map(|opt| opt.as_deref())
    }

    pub fn get_token(&self, index: u32) -> Option<Token> {
        self.tokens.get(index as usize).copied()
    }

    pub fn get_tokens(&self) -> impl Iterator<Item = Token> {
        self.tokens.iter().copied()
    }
}

type LineLookupTable<'a> = &'a [Token];

fn greatest_lower_bound<'a, T, K: Ord, F: Fn(&'a T) -> K>(
    slice: &'a [T],
    key: &K,
    map: F,
) -> Option<&'a T> {
    let mut idx = match slice.binary_search_by_key(key, &map) {
        Ok(index) => index,
        Err(index) => {
            // If there is no match, then we know for certain that the index is where we should
            // insert a new token, and that the token directly before is the greatest lower bound.
            return slice.get(index.checked_sub(1)?);
        }
    };

    // If we get an exact match, then we need to continue looking at previous tokens to see if
    // they also match. We use a linear search because the number of exact matches is generally
    // very small, and almost certainly smaller than the number of tokens before the index.
    for i in (0..idx).rev() {
        if map(&slice[i]) == *key {
            idx = i;
        } else {
            break;
        }
    }
    slice.get(idx)
}

#[test]
fn test_sourcemap_lookup_token() {
    let input = r#"{
        "version": 3,
        "sources": ["coolstuff.js"],
        "sourceRoot": "x",
        "names": ["x","alert"],
        "mappings": "AAAA,GAAIA,GAAI,EACR,IAAIA,GAAK,EAAG,CACVC,MAAM"
    }"#;
    let sm = SourceMap::from_json_string(input).unwrap();
    let lookup_table = sm.generate_lookup_table();
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 0).unwrap().to_tuple(),
        (Some("coolstuff.js"), 0, 0, None)
    );
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 3).unwrap().to_tuple(),
        (Some("coolstuff.js"), 0, 4, Some("x"))
    );
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 24).unwrap().to_tuple(),
        (Some("coolstuff.js"), 2, 8, None)
    );

    // Lines continue out to infinity
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 1000).unwrap().to_tuple(),
        (Some("coolstuff.js"), 2, 8, None)
    );

    assert!(sm.lookup_source_view_token(&lookup_table, 1000, 0).is_none());
}

#[test]
fn test_mut_sourcemap() {
    let mut sm = SourceMap::default();
    sm.set_file("index.js");
    sm.set_sources(vec!["foo.js"]);
    sm.set_source_contents(vec![Some("foo")]);

    assert_eq!(sm.get_file(), Some("index.js"));
    assert_eq!(sm.get_source(0), Some("foo.js"));
    assert_eq!(sm.get_source_content(0), Some("foo"));
}
