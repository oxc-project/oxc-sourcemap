use std::sync::Arc;

use crate::SourceMap;

/// Sentinel value representing an invalid/missing ID for source or name.
/// Used when a token doesn't have an associated source file or name.
pub(crate) const INVALID_ID: u32 = u32::MAX;

/// Struct of Arrays storage for tokens, improving memory locality
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens {
    pub(crate) dst_lines: Vec<u32>,
    pub(crate) dst_cols: Vec<u32>,
    pub(crate) src_lines: Vec<u32>,
    pub(crate) src_cols: Vec<u32>,
    pub(crate) source_ids: Vec<u32>,
    pub(crate) name_ids: Vec<u32>,
}

impl Tokens {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            dst_lines: Vec::with_capacity(capacity),
            dst_cols: Vec::with_capacity(capacity),
            src_lines: Vec::with_capacity(capacity),
            src_cols: Vec::with_capacity(capacity),
            source_ids: Vec::with_capacity(capacity),
            name_ids: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, token: Token) {
        self.dst_lines.push(token.dst_line);
        self.dst_cols.push(token.dst_col);
        self.src_lines.push(token.src_line);
        self.src_cols.push(token.src_col);
        self.source_ids.push(token.source_id);
        self.name_ids.push(token.name_id);
    }

    pub fn push_raw(
        &mut self,
        dst_line: u32,
        dst_col: u32,
        src_line: u32,
        src_col: u32,
        source_id: Option<u32>,
        name_id: Option<u32>,
    ) {
        self.dst_lines.push(dst_line);
        self.dst_cols.push(dst_col);
        self.src_lines.push(src_line);
        self.src_cols.push(src_col);
        self.source_ids.push(source_id.unwrap_or(INVALID_ID));
        self.name_ids.push(name_id.unwrap_or(INVALID_ID));
    }

    pub fn get(&self, index: usize) -> Option<Token> {
        if index >= self.len() {
            return None;
        }
        Some(Token {
            dst_line: self.dst_lines[index],
            dst_col: self.dst_cols[index],
            src_line: self.src_lines[index],
            src_col: self.src_cols[index],
            source_id: self.source_ids[index],
            name_id: self.name_ids[index],
        })
    }

    pub fn len(&self) -> usize {
        self.dst_lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dst_lines.is_empty()
    }

    pub fn iter(&self) -> TokensIter<'_> {
        TokensIter { tokens: self, index: 0 }
    }

    pub fn last(&self) -> Option<Token> {
        if self.is_empty() {
            None
        } else {
            self.get(self.len() - 1)
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        self.dst_lines.reserve(additional);
        self.dst_cols.reserve(additional);
        self.src_lines.reserve(additional);
        self.src_cols.reserve(additional);
        self.source_ids.reserve(additional);
        self.name_ids.reserve(additional);
    }

    pub fn shrink_to_fit(&mut self) {
        self.dst_lines.shrink_to_fit();
        self.dst_cols.shrink_to_fit();
        self.src_lines.shrink_to_fit();
        self.src_cols.shrink_to_fit();
        self.source_ids.shrink_to_fit();
        self.name_ids.shrink_to_fit();
    }

    pub fn extend_from_slice(&mut self, tokens: &[Token]) {
        self.reserve(tokens.len());
        for token in tokens {
            self.push(*token);
        }
    }
}

pub struct TokensIter<'a> {
    tokens: &'a Tokens,
    index: usize,
}

impl<'a> Iterator for TokensIter<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some(token)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.tokens.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for TokensIter<'a> {
    fn len(&self) -> usize {
        self.tokens.len() - self.index
    }
}

/// The `Token` is used to generate vlq `mappings`.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Token {
    pub(crate) dst_line: u32,
    pub(crate) dst_col: u32,
    pub(crate) src_line: u32,
    pub(crate) src_col: u32,
    source_id: u32,
    name_id: u32,
}

impl Token {
    pub fn new(
        dst_line: u32,
        dst_col: u32,
        src_line: u32,
        src_col: u32,
        source_id: Option<u32>,
        name_id: Option<u32>,
    ) -> Self {
        Self {
            dst_line,
            dst_col,
            src_line,
            src_col,
            source_id: source_id.unwrap_or(INVALID_ID),
            name_id: name_id.unwrap_or(INVALID_ID),
        }
    }

    pub fn get_dst_line(&self) -> u32 {
        self.dst_line
    }

    pub fn get_dst_col(&self) -> u32 {
        self.dst_col
    }

    pub fn get_src_line(&self) -> u32 {
        self.src_line
    }

    pub fn get_src_col(&self) -> u32 {
        self.src_col
    }

    pub fn get_name_id(&self) -> Option<u32> {
        if self.name_id == INVALID_ID { None } else { Some(self.name_id) }
    }

    pub fn get_source_id(&self) -> Option<u32> {
        if self.source_id == INVALID_ID { None } else { Some(self.source_id) }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenChunk {
    pub start: u32,
    pub end: u32,
    pub prev_dst_line: u32,
    pub prev_dst_col: u32,
    pub prev_src_line: u32,
    pub prev_src_col: u32,
    pub prev_name_id: u32,
    pub prev_source_id: u32,
}

impl TokenChunk {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        start: u32,
        end: u32,
        prev_dst_line: u32,
        prev_dst_col: u32,
        prev_src_line: u32,
        prev_src_col: u32,
        prev_name_id: u32,
        prev_source_id: u32,
    ) -> Self {
        Self {
            start,
            end,
            prev_dst_line,
            prev_dst_col,
            prev_src_line,
            prev_src_col,
            prev_name_id,
            prev_source_id,
        }
    }
}

/// The `SourceViewToken` provider extra `source` and `source_content` value.
#[derive(Debug, Clone, Copy)]
pub struct SourceViewToken<'a> {
    pub(crate) token: Token,
    pub(crate) sourcemap: &'a SourceMap,
}

impl<'a> SourceViewToken<'a> {
    pub fn new(token: Token, sourcemap: &'a SourceMap) -> Self {
        Self { token, sourcemap }
    }

    pub fn get_dst_line(&self) -> u32 {
        self.token.dst_line
    }

    pub fn get_dst_col(&self) -> u32 {
        self.token.dst_col
    }

    pub fn get_src_line(&self) -> u32 {
        self.token.src_line
    }

    pub fn get_src_col(&self) -> u32 {
        self.token.src_col
    }

    pub fn get_name_id(&self) -> Option<u32> {
        if self.token.name_id == INVALID_ID { None } else { Some(self.token.name_id) }
    }

    pub fn get_source_id(&self) -> Option<u32> {
        if self.token.source_id == INVALID_ID { None } else { Some(self.token.source_id) }
    }

    pub fn get_name(&self) -> Option<&Arc<str>> {
        if self.token.name_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_name(self.token.name_id)
        }
    }

    pub fn get_source(&self) -> Option<&Arc<str>> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source(self.token.source_id)
        }
    }

    pub fn get_source_content(&self) -> Option<&Arc<str>> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source_content(self.token.source_id)
        }
    }

    pub fn get_source_and_content(&self) -> Option<(&Arc<str>, &Arc<str>)> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source_and_content(self.token.source_id)
        }
    }

    pub fn to_tuple(&self) -> (Option<&Arc<str>>, u32, u32, Option<&Arc<str>>) {
        (self.get_source(), self.get_src_line(), self.get_src_col(), self.get_name())
    }
}
