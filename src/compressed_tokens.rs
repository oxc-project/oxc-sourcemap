use crate::token::Token;

/// Compressed token storage using delta encoding to reduce memory usage.
/// Tokens are stored as deltas from the previous token, with variable-length encoding.
#[derive(Debug, Clone, Default)]
pub struct CompressedTokens {
    /// First token stored in full
    first_token: Option<Token>,
    /// Compressed delta data
    data: Box<[u8]>,
    /// Number of tokens
    count: usize,
    /// Index for faster random access: stores byte offset every N tokens
    /// This allows O(1) positioning for random access
    index: Box<[IndexEntry]>,
}

#[derive(Debug, Clone, Copy)]
struct IndexEntry {
    /// Byte offset in data array
    offset: u32,
    /// Token at this position (for delta calculation)
    token: Token,
}

/// How often to create index entries (every N tokens)
const INDEX_INTERVAL: usize = 256;

/// Header byte format (2 bits per field):
/// - Bits 0-1: dst_line format
/// - Bits 2-3: dst_col format
/// - Bits 4-5: src_line format
/// - Bits 6-7: src_col format
///
/// Format values:
/// - 00: i8 delta
/// - 01: i16 delta
/// - 10: i32 delta
/// - 11: u32 absolute value
#[derive(Debug, Clone, Copy)]
struct HeaderByte(u8);

impl HeaderByte {
    const I8_DELTA: u8 = 0b00;
    const I16_DELTA: u8 = 0b01;
    const I32_DELTA: u8 = 0b10;
    const U32_ABSOLUTE: u8 = 0b11;

    fn new() -> Self {
        Self(0)
    }

    fn set_dst_line_format(&mut self, format: u8) {
        self.0 = (self.0 & !0b11) | (format & 0b11);
    }

    fn set_dst_col_format(&mut self, format: u8) {
        self.0 = (self.0 & !0b1100) | ((format & 0b11) << 2);
    }

    fn set_src_line_format(&mut self, format: u8) {
        self.0 = (self.0 & !0b110000) | ((format & 0b11) << 4);
    }

    fn set_src_col_format(&mut self, format: u8) {
        self.0 = (self.0 & !0b11000000) | ((format & 0b11) << 6);
    }

    fn dst_line_format(&self) -> u8 {
        self.0 & 0b11
    }

    fn dst_col_format(&self) -> u8 {
        (self.0 >> 2) & 0b11
    }

    fn src_line_format(&self) -> u8 {
        (self.0 >> 4) & 0b11
    }

    fn src_col_format(&self) -> u8 {
        (self.0 >> 6) & 0b11
    }
}

impl CompressedTokens {
    /// Create compressed tokens from a slice of tokens
    pub fn from_tokens(tokens: &[Token]) -> Self {
        if tokens.is_empty() {
            return Self { first_token: None, data: Box::new([]), count: 0, index: Box::new([]) };
        }

        let first_token = tokens[0];
        let mut data = Vec::with_capacity(tokens.len() * 8); // Estimate ~8 bytes per token
        let mut index = Vec::with_capacity((tokens.len() / INDEX_INTERVAL) + 1);

        // Add first index entry
        index.push(IndexEntry { offset: 0, token: first_token });

        let mut prev_token = first_token;

        // Compress remaining tokens
        for (i, &token) in tokens.iter().enumerate().skip(1) {
            // Create index entry every INDEX_INTERVAL tokens
            if i % INDEX_INTERVAL == 0 {
                index.push(IndexEntry { offset: data.len() as u32, token });
            }

            // Compress token as delta from previous
            compress_token_delta(&mut data, prev_token, token);
            prev_token = token;
        }

        Self {
            first_token: Some(first_token),
            data: data.into_boxed_slice(),
            count: tokens.len(),
            index: index.into_boxed_slice(),
        }
    }

    /// Get the number of tokens
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if there are no tokens
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Get a token by index
    pub fn get(&self, index: usize) -> Option<Token> {
        if index >= self.count {
            return None;
        }

        if index == 0 {
            return self.first_token;
        }

        // Find nearest index entry
        let index_pos = index / INDEX_INTERVAL;
        let index_entry = &self.index[index_pos.min(self.index.len() - 1)];

        // Start from index entry
        let mut current_token = index_entry.token;
        let mut data_pos = index_entry.offset as usize;
        let start_token_index = index_pos * INDEX_INTERVAL;

        // Decompress tokens from index entry to target
        for _ in start_token_index..index {
            let (next_token, bytes_read) =
                decompress_token_delta(&self.data[data_pos..], current_token);
            current_token = next_token;
            data_pos += bytes_read;
        }

        Some(current_token)
    }

    /// Create an iterator over tokens
    pub fn iter(&self) -> CompressedTokenIterator<'_> {
        CompressedTokenIterator {
            tokens: self,
            index: 0,
            current_token: self.first_token,
            data_pos: 0,
        }
    }

    /// Convert back to a Vec of tokens (for compatibility)
    pub fn to_vec(&self) -> Vec<Token> {
        self.iter().collect()
    }
}

/// Iterator over compressed tokens
pub struct CompressedTokenIterator<'a> {
    tokens: &'a CompressedTokens,
    index: usize,
    current_token: Option<Token>,
    data_pos: usize,
}

impl<'a> Iterator for CompressedTokenIterator<'a> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.tokens.count {
            return None;
        }

        if self.index == 0 {
            self.index += 1;
            return self.current_token;
        }

        if let Some(current) = self.current_token {
            let (next_token, bytes_read) =
                decompress_token_delta(&self.tokens.data[self.data_pos..], current);
            self.current_token = Some(next_token);
            self.data_pos += bytes_read;
            self.index += 1;
            Some(next_token)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.tokens.count - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for CompressedTokenIterator<'a> {
    fn len(&self) -> usize {
        self.tokens.count - self.index
    }
}

/// Compress a token as delta from previous token
fn compress_token_delta(data: &mut Vec<u8>, prev: Token, token: Token) {
    let mut header = HeaderByte::new();

    // Calculate deltas
    let dst_line_delta = token.get_dst_line() as i64 - prev.get_dst_line() as i64;
    let dst_col_delta = token.get_dst_col() as i64 - prev.get_dst_col() as i64;
    let src_line_delta = token.get_src_line() as i64 - prev.get_src_line() as i64;
    let src_col_delta = token.get_src_col() as i64 - prev.get_src_col() as i64;

    // Determine formats and set header
    let dst_line_format = get_field_format(dst_line_delta);
    let dst_col_format = get_field_format(dst_col_delta);
    let src_line_format = get_field_format(src_line_delta);
    let src_col_format = get_field_format(src_col_delta);

    header.set_dst_line_format(dst_line_format);
    header.set_dst_col_format(dst_col_format);
    header.set_src_line_format(src_line_format);
    header.set_src_col_format(src_col_format);

    // Write header first
    data.push(header.0);

    // Encode fields
    encode_field_with_format(data, dst_line_delta, dst_line_format);
    encode_field_with_format(data, dst_col_delta, dst_col_format);
    encode_field_with_format(data, src_line_delta, src_line_format);
    encode_field_with_format(data, src_col_delta, src_col_format);

    // Encode source_id and name_id with special handling for INVALID_ID
    encode_optional_id_delta(data, prev.get_source_id(), token.get_source_id());
    encode_optional_id_delta(data, prev.get_name_id(), token.get_name_id());
}

/// Decompress a token from delta data
fn decompress_token_delta(data: &[u8], prev: Token) -> (Token, usize) {
    let mut pos = 0;

    // Read header
    let header = HeaderByte(data[pos]);
    pos += 1;

    // Decode fields
    let (dst_line, bytes) =
        decode_field_delta(&data[pos..], prev.get_dst_line(), header.dst_line_format());
    pos += bytes;

    let (dst_col, bytes) =
        decode_field_delta(&data[pos..], prev.get_dst_col(), header.dst_col_format());
    pos += bytes;

    let (src_line, bytes) =
        decode_field_delta(&data[pos..], prev.get_src_line(), header.src_line_format());
    pos += bytes;

    let (src_col, bytes) =
        decode_field_delta(&data[pos..], prev.get_src_col(), header.src_col_format());
    pos += bytes;

    // Decode optional IDs
    let (source_id, bytes) = decode_optional_id_delta(&data[pos..], prev.get_source_id());
    pos += bytes;

    let (name_id, bytes) = decode_optional_id_delta(&data[pos..], prev.get_name_id());
    pos += bytes;

    let token = Token::new(dst_line, dst_col, src_line, src_col, source_id, name_id);
    (token, pos)
}

/// Get the format for a field delta
fn get_field_format(delta: i64) -> u8 {
    if delta >= -128 && delta <= 127 {
        HeaderByte::I8_DELTA
    } else if delta >= -32768 && delta <= 32767 {
        HeaderByte::I16_DELTA
    } else if delta >= i32::MIN as i64 && delta <= i32::MAX as i64 {
        HeaderByte::I32_DELTA
    } else {
        HeaderByte::U32_ABSOLUTE
    }
}

/// Encode a field with the given format
fn encode_field_with_format(data: &mut Vec<u8>, delta: i64, format: u8) {
    match format {
        HeaderByte::I8_DELTA => {
            data.push(delta as i8 as u8);
        }
        HeaderByte::I16_DELTA => {
            let bytes = (delta as i16).to_le_bytes();
            data.extend_from_slice(&bytes);
        }
        HeaderByte::I32_DELTA => {
            let bytes = (delta as i32).to_le_bytes();
            data.extend_from_slice(&bytes);
        }
        HeaderByte::U32_ABSOLUTE => {
            // Store as absolute value
            let value = (delta as i64 + i32::MIN as i64) as u32;
            data.extend_from_slice(&value.to_le_bytes());
        }
        _ => unreachable!(),
    }
}

/// Decode a field delta
fn decode_field_delta(data: &[u8], prev_value: u32, format: u8) -> (u32, usize) {
    match format {
        HeaderByte::I8_DELTA => {
            let delta = data[0] as i8 as i32;
            ((prev_value as i32 + delta) as u32, 1)
        }
        HeaderByte::I16_DELTA => {
            let bytes = [data[0], data[1]];
            let delta = i16::from_le_bytes(bytes) as i32;
            ((prev_value as i32 + delta) as u32, 2)
        }
        HeaderByte::I32_DELTA => {
            let bytes = [data[0], data[1], data[2], data[3]];
            let delta = i32::from_le_bytes(bytes);
            ((prev_value as i32 + delta) as u32, 4)
        }
        HeaderByte::U32_ABSOLUTE => {
            let bytes = [data[0], data[1], data[2], data[3]];
            (u32::from_le_bytes(bytes), 4)
        }
        _ => unreachable!(),
    }
}

/// Encode optional ID with special handling for INVALID_ID
fn encode_optional_id_delta(data: &mut Vec<u8>, prev: Option<u32>, current: Option<u32>) {
    match (prev, current) {
        (None, None) => data.push(0), // Both invalid
        (None, Some(id)) => {
            data.push(1); // Was invalid, now valid
            data.extend_from_slice(&id.to_le_bytes());
        }
        (Some(_), None) => data.push(2), // Was valid, now invalid
        (Some(prev_id), Some(curr_id)) => {
            let delta = curr_id as i32 - prev_id as i32;
            if delta >= -127 && delta <= 127 {
                data.push(3); // Small delta
                data.push(delta as i8 as u8);
            } else {
                data.push(4); // Large delta
                data.extend_from_slice(&delta.to_le_bytes());
            }
        }
    }
}

/// Decode optional ID delta
fn decode_optional_id_delta(data: &[u8], prev: Option<u32>) -> (Option<u32>, usize) {
    match data[0] {
        0 => (None, 1), // Both invalid
        1 => {
            // Was invalid, now valid
            let bytes = [data[1], data[2], data[3], data[4]];
            (Some(u32::from_le_bytes(bytes)), 5)
        }
        2 => (None, 1), // Was valid, now invalid
        3 => {
            // Small delta
            let delta = data[1] as i8 as i32;
            let id = (prev.unwrap() as i32 + delta) as u32;
            (Some(id), 2)
        }
        4 => {
            // Large delta
            let bytes = [data[1], data[2], data[3], data[4]];
            let delta = i32::from_le_bytes(bytes);
            let id = (prev.unwrap() as i32 + delta) as u32;
            (Some(id), 5)
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress() {
        let tokens = vec![
            Token::new(0, 0, 0, 0, Some(0), Some(0)),
            Token::new(0, 5, 0, 5, Some(0), Some(0)),
            Token::new(1, 0, 1, 0, Some(0), None),
            Token::new(1, 10, 1, 10, Some(1), Some(1)),
        ];

        let compressed = CompressedTokens::from_tokens(&tokens);

        // Test individual access
        for (i, &expected) in tokens.iter().enumerate() {
            assert_eq!(compressed.get(i), Some(expected));
        }

        // Test iterator
        let decompressed: Vec<_> = compressed.iter().collect();
        assert_eq!(decompressed, tokens);
    }

    #[test]
    fn test_empty_tokens() {
        let compressed = CompressedTokens::from_tokens(&[]);
        assert!(compressed.is_empty());
        assert_eq!(compressed.len(), 0);
        assert_eq!(compressed.get(0), None);
    }
}
