use std::sync::Arc;

use crate::{
    SourceViewToken,
    decode::{JSONSourceMap, decode, decode_from_string},
    encode::{encode, encode_to_string},
    error::Result,
    token::{Token, TokenChunk, Tokens},
};

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    pub(crate) file: Option<Arc<str>>,
    pub(crate) names: Vec<Arc<str>>,
    pub(crate) source_root: Option<String>,
    pub(crate) sources: Vec<Arc<str>>,
    pub(crate) source_contents: Vec<Option<Arc<str>>>,
    pub(crate) tokens: Tokens,
    pub(crate) token_chunks: Option<Vec<TokenChunk>>,
    /// Identifies third-party sources (such as framework code or bundler-generated code), allowing developers to avoid code that they don't want to see or step through, without having to configure this beforehand.
    /// The `x_google_ignoreList` field refers to the `sources` array, and lists the indices of all the known third-party sources in that source map.
    /// When parsing the source map, developer tools can use this to determine sections of the code that the browser loads and runs that could be automatically ignore-listed.
    pub(crate) x_google_ignore_list: Option<Vec<u32>>,
    pub(crate) debug_id: Option<String>,
}

impl SourceMap {
    pub fn new(
        file: Option<Arc<str>>,
        names: Vec<Arc<str>>,
        source_root: Option<String>,
        sources: Vec<Arc<str>>,
        source_contents: Vec<Option<Arc<str>>>,
        tokens: Tokens,
        token_chunks: Option<Vec<TokenChunk>>,
    ) -> Self {
        Self {
            file,
            names,
            source_root,
            sources,
            source_contents,
            tokens,
            token_chunks,
            x_google_ignore_list: None,
            debug_id: None,
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

    /// Convert `SourceMap` to vlq sourcemap string.
    pub fn to_json_string(&self) -> String {
        encode_to_string(self)
    }

    /// Convert `SourceMap` to vlq sourcemap data url.
    pub fn to_data_url(&self) -> String {
        let base_64_str = base64_simd::STANDARD.encode_to_string(self.to_json_string().as_bytes());
        format!("data:application/json;charset=utf-8;base64,{base_64_str}")
    }

    pub fn get_file(&self) -> Option<&Arc<str>> {
        self.file.as_ref()
    }

    pub fn set_file(&mut self, file: &str) {
        self.file = Some(file.into());
    }

    pub fn get_source_root(&self) -> Option<&str> {
        self.source_root.as_deref()
    }

    pub fn get_x_google_ignore_list(&self) -> Option<&[u32]> {
        self.x_google_ignore_list.as_deref()
    }

    /// Set `x_google_ignoreList`.
    pub fn set_x_google_ignore_list(&mut self, x_google_ignore_list: Vec<u32>) {
        self.x_google_ignore_list = Some(x_google_ignore_list);
    }

    pub fn set_debug_id(&mut self, debug_id: &str) {
        self.debug_id = Some(debug_id.into());
    }

    pub fn get_debug_id(&self) -> Option<&str> {
        self.debug_id.as_deref()
    }

    pub fn get_names(&self) -> impl Iterator<Item = &Arc<str>> {
        self.names.iter()
    }

    /// Adjust `sources`.
    pub fn set_sources(&mut self, sources: Vec<&str>) {
        self.sources = sources.into_iter().map(Into::into).collect();
    }

    pub fn get_sources(&self) -> impl Iterator<Item = &Arc<str>> {
        self.sources.iter()
    }

    /// Adjust `source_content`.
    pub fn set_source_contents(&mut self, source_contents: Vec<Option<&str>>) {
        self.source_contents =
            source_contents.into_iter().map(|v| v.map(Arc::from)).collect::<Vec<_>>();
    }

    pub fn get_source_contents(&self) -> impl Iterator<Item = Option<&Arc<str>>> {
        self.source_contents.iter().map(|item| item.as_ref())
    }

    pub fn get_token(&self, index: u32) -> Option<Token> {
        self.tokens.get(index as usize)
    }

    pub fn get_source_view_token(&self, index: u32) -> Option<SourceViewToken<'_>> {
        self.tokens.get(index as usize).map(|token| SourceViewToken::new(token, self))
    }

    /// Get raw tokens.
    pub fn get_tokens(&self) -> impl Iterator<Item = Token> + '_ {
        self.tokens.iter()
    }

    /// Get source view tokens. See [`SourceViewToken`] for more information.
    pub fn get_source_view_tokens(&self) -> impl Iterator<Item = SourceViewToken<'_>> {
        self.tokens.iter().map(|token| SourceViewToken::new(token, self))
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

    pub fn get_source_and_content(&self, id: u32) -> Option<(&Arc<str>, &Arc<str>)> {
        let source = self.get_source(id)?;
        let content = self.get_source_content(id)?;
        Some((source, content))
    }

    /// Generate a lookup table, it will be used at `lookup_token` or `lookup_source_view_token`.
    pub fn generate_lookup_table(&self) -> Vec<LineLookupTable> {
        // The dst line/dst col always has increasing order.
        if let Some(last_token) = self.tokens.last() {
            let mut table = vec![LineLookupTable { tokens: &self.tokens, start: 0, end: 0 }; last_token.dst_line as usize + 1];
            let mut prev_start_idx = 0u32;
            let mut prev_dst_line = 0u32;
            for idx in 0..self.tokens.len() {
                let dst_line = self.tokens.dst_lines[idx];
                if dst_line != prev_dst_line {
                    table[prev_dst_line as usize] = LineLookupTable {
                        tokens: &self.tokens,
                        start: prev_start_idx as usize,
                        end: idx,
                    };
                    prev_start_idx = idx as u32;
                    prev_dst_line = dst_line;
                }
            }
            table[prev_dst_line as usize] = LineLookupTable {
                tokens: &self.tokens,
                start: prev_start_idx as usize,
                end: self.tokens.len(),
            };
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
        let table_entry = lookup_table[line as usize];
        greatest_lower_bound_token(table_entry.tokens, table_entry.start, table_entry.end, (line, col))
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

#[derive(Debug, Clone, Copy)]
pub struct LineLookupTable<'a> {
    tokens: &'a Tokens,
    start: usize,
    end: usize,
}

fn greatest_lower_bound_token(
    tokens: &Tokens,
    start: usize,
    end: usize,
    key: (u32, u32),
) -> Option<Token> {
    if start >= end {
        return None;
    }

    // Binary search for the key
    let mut left = start;
    let mut right = end;

    while left < right {
        let mid = left + (right - left) / 2;
        let mid_key = (tokens.dst_lines[mid], tokens.dst_cols[mid]);

        match mid_key.cmp(&key) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid,
            std::cmp::Ordering::Equal => {
                // Found exact match, but we need the first occurrence
                right = mid;
                while right > start && (tokens.dst_lines[right - 1], tokens.dst_cols[right - 1]) == key {
                    right -= 1;
                }
                return tokens.get(right);
            }
        }
    }

    // No exact match, return the greatest lower bound
    if left > start {
        tokens.get(left - 1)
    } else {
        None
    }
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
        (Some(&"coolstuff.js".into()), 0, 0, None)
    );
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 3).unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 0, 4, Some(&"x".into()))
    );
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 24).unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 2, 8, None)
    );

    // Lines continue out to infinity
    assert_eq!(
        sm.lookup_source_view_token(&lookup_table, 0, 1000).unwrap().to_tuple(),
        (Some(&"coolstuff.js".into()), 2, 8, None)
    );

    assert!(sm.lookup_source_view_token(&lookup_table, 1000, 0).is_none());
}

#[test]
fn test_sourcemap_source_view_token() {
    let mut tokens = Tokens::new();
    tokens.push(Token::new(1, 1, 1, 1, Some(0), Some(0)));
    let sm = SourceMap::new(
        None,
        vec!["foo".into()],
        None,
        vec!["foo.js".into()],
        vec![],
        tokens,
        None,
    );
    let mut source_view_tokens = sm.get_source_view_tokens();
    assert_eq!(
        source_view_tokens.next().unwrap().to_tuple(),
        (Some(&"foo.js".into()), 1, 1, Some(&"foo".into()))
    );
}

#[test]
fn test_mut_sourcemap() {
    let mut sm = SourceMap::default();
    sm.set_file("index.js");
    sm.set_sources(vec!["foo.js"]);
    sm.set_source_contents(vec![Some("foo")]);

    assert_eq!(sm.get_file().map(|s| s.as_ref()), Some("index.js"));
    assert_eq!(sm.get_source(0).map(|s| s.as_ref()), Some("foo.js"));
    assert_eq!(sm.get_source_content(0).map(|s| s.as_ref()), Some("foo"));
}
