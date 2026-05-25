use crate::{
    JSONSourceMap, SourceMap, SourceViewToken, Token, error::Result, sourcemap::SourceMapParts,
};

/// An owned, lifetime-free source map.
///
/// `OwnedSourceMap` is a thin wrapper around `SourceMap<'static>`. It exists
/// so that downstream code that stores sourcemaps as long-lived struct fields
/// can write `Option<OwnedSourceMap>` instead of `Option<SourceMap<'static>>`,
/// keeping the `'static` lifetime annotation out of the public type names.
///
/// Internally this is just `SourceMap<'static>` — same allocation model, same
/// methods. For the zero-copy-parse case, prefer the lifetime-parameterized
/// [`SourceMap<'a>`] directly.
///
/// ```
/// use oxc_sourcemap::{OwnedSourceMap, SourceMap, SourceMapBuilder};
///
/// // From a builder (always owned):
/// let owned: OwnedSourceMap = SourceMapBuilder::default().into_owned_sourcemap();
///
/// // From a JSON string (zero-copy parse, then detach):
/// let json = r#"{"version":3,"sources":[],"names":[],"mappings":""}"#;
/// let owned = OwnedSourceMap::from_json_string(json).unwrap();
///
/// // Use the same accessors as SourceMap<'_>:
/// let _file: Option<&str> = owned.get_file();
/// let _names: Vec<&str> = owned.get_names().collect();
///
/// // Drop into the underlying `&SourceMap<'_>` when needed (e.g. for
/// // serialization, lookup tables, concat builders):
/// let _inner: &SourceMap<'_> = owned.as_source_map();
/// let _json_str = owned.to_json_string();
/// ```
#[derive(Debug, Clone, Default)]
pub struct OwnedSourceMap {
    inner: SourceMap<'static>,
}

impl OwnedSourceMap {
    /// Wrap an already-`'static` `SourceMap`.
    #[inline]
    pub fn new(inner: SourceMap<'static>) -> Self {
        Self { inner }
    }

    /// Borrow as `&SourceMap<'_>` so generic code that works on `SourceMap`
    /// (lookup tables, concat builder, encoder, visualizer) can consume an
    /// `OwnedSourceMap` without an explicit conversion.
    #[inline]
    pub fn as_source_map(&self) -> &SourceMap<'static> {
        &self.inner
    }

    /// Mutable borrow of the inner `SourceMap`. Useful for `set_file` etc.
    #[inline]
    pub fn as_source_map_mut(&mut self) -> &mut SourceMap<'static> {
        &mut self.inner
    }

    /// Unwrap into the inner `SourceMap<'static>`.
    #[inline]
    pub fn into_inner(self) -> SourceMap<'static> {
        self.inner
    }

    // ---------- parse / serialize ----------

    /// Parse a sourcemap from a `JSONSourceMap` (already-deserialized JSON).
    ///
    /// # Errors
    /// Returns VLQ decode errors.
    pub fn from_json(value: JSONSourceMap) -> Result<Self> {
        SourceMap::from_json(value).map(Self::new)
    }

    /// Parse a sourcemap from a JSON string. Uses the zero-copy borrowed-
    /// deserialization path internally, then detaches to `'static` so the
    /// result outlives `value`.
    ///
    /// # Errors
    /// Returns `serde_json` and VLQ decode errors.
    pub fn from_json_string(value: &str) -> Result<Self> {
        SourceMap::from_json_string(value).map(SourceMap::into_owned).map(Self::new)
    }

    pub fn to_json(&self) -> JSONSourceMap {
        self.inner.to_json()
    }

    pub fn to_json_string(&self) -> String {
        self.inner.to_json_string()
    }

    pub fn to_data_url(&self) -> String {
        self.inner.to_data_url()
    }

    // ---------- accessors (delegated) ----------

    pub fn get_file(&self) -> Option<&str> {
        self.inner.get_file()
    }

    pub fn set_file(&mut self, file: &str) {
        self.inner.set_file(file);
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.inner.get_source_root()
    }

    pub fn get_x_google_ignore_list(&self) -> Option<&[u32]> {
        self.inner.get_x_google_ignore_list()
    }

    pub fn set_x_google_ignore_list(&mut self, list: Vec<u32>) {
        self.inner.set_x_google_ignore_list(list);
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.inner.get_debug_id()
    }

    pub fn set_debug_id(&mut self, debug_id: &str) {
        self.inner.set_debug_id(debug_id);
    }

    pub fn get_names(&self) -> impl Iterator<Item = &str> {
        self.inner.get_names()
    }

    pub fn get_sources(&self) -> impl Iterator<Item = &str> {
        self.inner.get_sources()
    }

    pub fn set_sources<S: AsRef<str>, I: IntoIterator<Item = S>>(&mut self, sources: I) {
        self.inner.set_sources(sources);
    }

    pub fn get_source_contents(&self) -> impl Iterator<Item = Option<&str>> {
        self.inner.get_source_contents()
    }

    pub fn set_source_contents(&mut self, contents: Vec<Option<&str>>) {
        self.inner.set_source_contents(contents);
    }

    pub fn get_name(&self, id: u32) -> Option<&str> {
        self.inner.get_name(id)
    }

    pub fn get_source(&self, id: u32) -> Option<&str> {
        self.inner.get_source(id)
    }

    pub fn get_source_content(&self, id: u32) -> Option<&str> {
        self.inner.get_source_content(id)
    }

    pub fn get_source_and_content(&self, id: u32) -> Option<(&str, &str)> {
        self.inner.get_source_and_content(id)
    }

    pub fn get_token(&self, index: u32) -> Option<Token> {
        self.inner.get_token(index)
    }

    pub fn get_tokens(&self) -> impl Iterator<Item = Token> {
        self.inner.get_tokens()
    }

    pub fn get_source_view_token(&self, index: u32) -> Option<SourceViewToken<'_, 'static>> {
        self.inner.get_source_view_token(index)
    }

    pub fn get_source_view_tokens(&self) -> impl Iterator<Item = SourceViewToken<'_, 'static>> {
        self.inner.get_source_view_tokens()
    }

    pub fn generate_lookup_table(&self) -> Vec<&[Token]> {
        self.inner.generate_lookup_table()
    }

    pub fn lookup_token(
        &self,
        lookup_table: &[&[Token]],
        line: u32,
        col: u32,
    ) -> Option<Token> {
        self.inner.lookup_token(lookup_table, line, col)
    }

    pub fn lookup_source_view_token(
        &self,
        lookup_table: &[&[Token]],
        line: u32,
        col: u32,
    ) -> Option<SourceViewToken<'_, 'static>> {
        self.inner.lookup_source_view_token(lookup_table, line, col)
    }

    // ---------- structural ----------

    /// Take the inner parts; same as `SourceMap::into_parts`.
    pub fn into_parts(self) -> SourceMapParts<'static> {
        self.inner.into_parts()
    }

    /// Build from parts; same as `SourceMap::from_parts`.
    pub fn from_parts(parts: SourceMapParts<'static>) -> Self {
        Self::new(SourceMap::from_parts(parts))
    }
}

impl From<SourceMap<'static>> for OwnedSourceMap {
    #[inline]
    fn from(inner: SourceMap<'static>) -> Self {
        Self::new(inner)
    }
}

impl From<OwnedSourceMap> for SourceMap<'static> {
    #[inline]
    fn from(owned: OwnedSourceMap) -> Self {
        owned.into_inner()
    }
}

impl std::ops::Deref for OwnedSourceMap {
    type Target = SourceMap<'static>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for OwnedSourceMap {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a> SourceMap<'a> {
    /// Materialize an owned sourcemap from `self`. Equivalent to
    /// `OwnedSourceMap::new(self.into_owned())`.
    #[inline]
    pub fn into_owned_sourcemap(self) -> OwnedSourceMap {
        OwnedSourceMap::new(self.into_owned())
    }
}
