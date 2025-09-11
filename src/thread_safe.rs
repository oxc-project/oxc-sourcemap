use crate::{JSONSourceMap, SourceMap, Token, TokenChunk, error::Result};
use std::rc::Rc;
use std::sync::Arc;

/// Thread-safe version of SourceMap that uses Arc internally for thread safety
#[derive(Debug, Clone)]
pub struct ThreadSafeSourceMap {
    file: Option<Arc<str>>,
    names: Vec<Arc<str>>,
    source_root: Option<String>,
    sources: Vec<Arc<str>>,
    source_contents: Vec<Option<Arc<str>>>,
    tokens: Vec<Token>,
    token_chunks: Option<Vec<TokenChunk>>,
    x_google_ignore_list: Option<Vec<u32>>,
    debug_id: Option<String>,
}

impl ThreadSafeSourceMap {
    /// Create a new ThreadSafeSourceMap from a SourceMap by converting Rc to Arc
    pub fn from_sourcemap(sourcemap: SourceMap) -> Self {
        Self {
            file: sourcemap.file.map(|rc| Arc::from(rc.as_ref())),
            names: sourcemap.names.into_iter().map(|rc| Arc::from(rc.as_ref())).collect(),
            source_root: sourcemap.source_root,
            sources: sourcemap.sources.into_iter().map(|rc| Arc::from(rc.as_ref())).collect(),
            source_contents: sourcemap
                .source_contents
                .into_iter()
                .map(|opt| opt.map(|rc| Arc::from(rc.as_ref())))
                .collect(),
            tokens: sourcemap.tokens,
            token_chunks: sourcemap.token_chunks,
            x_google_ignore_list: sourcemap.x_google_ignore_list,
            debug_id: sourcemap.debug_id,
        }
    }

    /// Create from a JSONSourceMap
    pub fn from_json(value: JSONSourceMap) -> Result<Self> {
        Ok(Self::from_sourcemap(SourceMap::from_json(value)?))
    }

    /// Create from a JSON string
    pub fn from_json_string(value: &str) -> Result<Self> {
        Ok(Self::from_sourcemap(SourceMap::from_json_string(value)?))
    }

    /// Convert back to a regular SourceMap (creates new Rc allocations)
    pub fn to_sourcemap(&self) -> SourceMap {
        SourceMap {
            file: self.file.as_ref().map(|arc| Rc::from(arc.as_ref())),
            names: self.names.iter().map(|arc| Rc::from(arc.as_ref())).collect(),
            source_root: self.source_root.clone(),
            sources: self.sources.iter().map(|arc| Rc::from(arc.as_ref())).collect(),
            source_contents: self
                .source_contents
                .iter()
                .map(|opt| opt.as_ref().map(|arc| Rc::from(arc.as_ref())))
                .collect(),
            tokens: self.tokens.clone(),
            token_chunks: self.token_chunks.clone(),
            x_google_ignore_list: self.x_google_ignore_list.clone(),
            debug_id: self.debug_id.clone(),
        }
    }

    /// Convert to JSON
    pub fn to_json(&self) -> JSONSourceMap {
        self.to_sourcemap().to_json()
    }

    /// Convert to JSON string
    pub fn to_json_string(&self) -> String {
        self.to_sourcemap().to_json_string()
    }

    /// Convert to data URL
    pub fn to_data_url(&self) -> String {
        self.to_sourcemap().to_data_url()
    }

    // Replicate the main accessors from SourceMap
    pub fn get_file(&self) -> Option<&Arc<str>> {
        self.file.as_ref()
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.source_root.as_deref()
    }

    pub fn get_x_google_ignore_list(&self) -> Option<&[u32]> {
        self.x_google_ignore_list.as_deref()
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.debug_id.as_deref()
    }

    pub fn get_names(&self) -> impl Iterator<Item = &Arc<str>> {
        self.names.iter()
    }

    pub fn get_sources(&self) -> impl Iterator<Item = &Arc<str>> {
        self.sources.iter()
    }

    pub fn get_source_contents(&self) -> impl Iterator<Item = Option<&Arc<str>>> {
        self.source_contents.iter().map(|item| item.as_ref())
    }

    pub fn get_token(&self, index: u32) -> Option<Token> {
        self.tokens.get(index as usize).copied()
    }

    pub fn get_tokens(&self) -> impl Iterator<Item = Token> + '_ {
        self.tokens.iter().copied()
    }

    pub fn get_name(&self, id: u32) -> Option<&Arc<str>> {
        self.names.get(id as usize)
    }

    pub fn get_source(&self, id: u32) -> Option<&Arc<str>> {
        self.sources.get(id as usize)
    }

    pub fn get_source_content(&self, id: u32) -> Option<&Arc<str>> {
        self.source_contents.get(id as usize).and_then(|item| item.as_ref())
    }
}

impl From<SourceMap> for ThreadSafeSourceMap {
    fn from(sourcemap: SourceMap) -> Self {
        Self::from_sourcemap(sourcemap)
    }
}

/// Wrapper for Arc<ThreadSafeSourceMap> for convenient sharing
#[derive(Debug, Clone)]
pub struct SharedSourceMap(Arc<ThreadSafeSourceMap>);

impl SharedSourceMap {
    pub fn new(sourcemap: SourceMap) -> Self {
        Self(Arc::new(ThreadSafeSourceMap::from_sourcemap(sourcemap)))
    }

    pub fn from_thread_safe(thread_safe: ThreadSafeSourceMap) -> Self {
        Self(Arc::new(thread_safe))
    }
}

impl std::ops::Deref for SharedSourceMap {
    type Target = ThreadSafeSourceMap;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
