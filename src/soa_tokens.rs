use crate::token::Token;

/// Struct-of-Arrays token storage for better cache locality on partial field access.
/// Stores token fields in separate arrays instead of an array of structs.
#[derive(Debug, Clone, Default)]
pub struct SoaTokens {
    /// Destination line numbers
    dst_lines: Box<[u32]>,
    /// Destination column numbers
    dst_cols: Box<[u32]>,
    /// Source line numbers
    src_lines: Box<[u32]>,
    /// Source column numbers
    src_cols: Box<[u32]>,
    /// Source file IDs
    source_ids: Box<[u32]>,
    /// Name IDs
    name_ids: Box<[u32]>,
    /// Number of tokens
    len: usize,
}

impl SoaTokens {
    /// Create SoA tokens from a slice of Token structs
    pub fn from_tokens(tokens: &[Token]) -> Self {
        if tokens.is_empty() {
            return Self::default();
        }

        let len = tokens.len();
        let mut dst_lines = Vec::with_capacity(len);
        let mut dst_cols = Vec::with_capacity(len);
        let mut src_lines = Vec::with_capacity(len);
        let mut src_cols = Vec::with_capacity(len);
        let mut source_ids = Vec::with_capacity(len);
        let mut name_ids = Vec::with_capacity(len);

        for token in tokens {
            dst_lines.push(token.get_dst_line());
            dst_cols.push(token.get_dst_col());
            src_lines.push(token.get_src_line());
            src_cols.push(token.get_src_col());
            source_ids.push(token.get_source_id().unwrap_or(u32::MAX));
            name_ids.push(token.get_name_id().unwrap_or(u32::MAX));
        }

        Self {
            dst_lines: dst_lines.into_boxed_slice(),
            dst_cols: dst_cols.into_boxed_slice(),
            src_lines: src_lines.into_boxed_slice(),
            src_cols: src_cols.into_boxed_slice(),
            source_ids: source_ids.into_boxed_slice(),
            name_ids: name_ids.into_boxed_slice(),
            len,
        }
    }

    /// Get the number of tokens
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if there are no tokens
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get a token by index
    pub fn get(&self, index: usize) -> Option<Token> {
        if index >= self.len {
            return None;
        }

        Some(Token::new(
            self.dst_lines[index],
            self.dst_cols[index],
            self.src_lines[index],
            self.src_cols[index],
            if self.source_ids[index] == u32::MAX { None } else { Some(self.source_ids[index]) },
            if self.name_ids[index] == u32::MAX { None } else { Some(self.name_ids[index]) },
        ))
    }

    /// Get the last token
    pub fn last(&self) -> Option<Token> {
        if self.is_empty() { None } else { self.get(self.len - 1) }
    }

    /// Get destination line for a token (optimized for lookup table generation)
    #[cfg(test)]
    pub fn get_dst_line(&self, index: usize) -> Option<u32> {
        if index >= self.len { None } else { Some(self.dst_lines[index]) }
    }

    /// Get destination line and column for a token (optimized for binary search)
    #[cfg(test)]
    pub fn get_dst_pos(&self, index: usize) -> Option<(u32, u32)> {
        if index >= self.len { None } else { Some((self.dst_lines[index], self.dst_cols[index])) }
    }

    /// Create an iterator over tokens
    pub fn iter(&self) -> SoaTokenIterator<'_> {
        SoaTokenIterator { tokens: self, index: 0 }
    }

}

/// Iterator over SoA tokens
pub struct SoaTokenIterator<'a> {
    tokens: &'a SoaTokens,
    index: usize,
}

impl<'a> Iterator for SoaTokenIterator<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some(token)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.tokens.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for SoaTokenIterator<'a> {
    fn len(&self) -> usize {
        self.tokens.len - self.index
    }
}

impl<'a> std::iter::FusedIterator for SoaTokenIterator<'a> {}

/// Enable indexing with usize
impl std::ops::Index<usize> for SoaTokens {
    type Output = Token;

    fn index(&self, index: usize) -> &Self::Output {
        // This is a bit of a hack - we can't return a reference to a Token
        // that doesn't exist in memory, so we panic if out of bounds.
        // In practice, use get() for safe access.
        if index >= self.len {
            panic!("index out of bounds: the len is {} but the index is {}", self.len, index);
        }

        // This is not ideal but maintains compatibility
        // The Token is constructed on stack and immediately leaked
        // This is safe but not recommended for frequent use
        panic!("Cannot return reference to temporary Token. Use get() instead.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soa_tokens_basic() {
        let tokens = vec![
            Token::new(0, 0, 0, 0, Some(0), Some(0)),
            Token::new(0, 5, 0, 5, Some(0), Some(1)),
            Token::new(1, 0, 1, 0, Some(1), None),
        ];

        let soa = SoaTokens::from_tokens(&tokens);

        assert_eq!(soa.len(), 3);
        assert!(!soa.is_empty());

        for (i, expected) in tokens.iter().enumerate() {
            assert_eq!(soa.get(i), Some(*expected));
        }

        assert_eq!(soa.get(3), None);
    }

    #[test]
    fn test_soa_tokens_iterator() {
        let tokens = vec![
            Token::new(0, 0, 0, 0, Some(0), Some(0)),
            Token::new(0, 5, 0, 5, Some(0), Some(1)),
            Token::new(1, 0, 1, 0, Some(1), None),
        ];

        let soa = SoaTokens::from_tokens(&tokens);
        let collected: Vec<_> = soa.iter().collect();

        assert_eq!(collected, tokens);
    }

    #[test]
    fn test_soa_tokens_empty() {
        let soa = SoaTokens::from_tokens(&[]);
        assert!(soa.is_empty());
        assert_eq!(soa.len(), 0);
        assert_eq!(soa.get(0), None);
        assert_eq!(soa.last(), None);
    }

    #[test]
    fn test_soa_tokens_optimized_access() {
        let tokens = vec![
            Token::new(0, 10, 0, 0, Some(0), Some(0)),
            Token::new(1, 20, 1, 5, Some(0), None),
            Token::new(2, 30, 2, 10, Some(1), Some(1)),
        ];

        let soa = SoaTokens::from_tokens(&tokens);

        assert_eq!(soa.get_dst_line(0), Some(0));
        assert_eq!(soa.get_dst_line(1), Some(1));
        assert_eq!(soa.get_dst_line(2), Some(2));
        assert_eq!(soa.get_dst_line(3), None);

        assert_eq!(soa.get_dst_pos(0), Some((0, 10)));
        assert_eq!(soa.get_dst_pos(1), Some((1, 20)));
        assert_eq!(soa.get_dst_pos(2), Some((2, 30)));
    }
}
