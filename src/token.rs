use crate::SourceMap;

/// Sentinel value representing an invalid/missing ID for source or name.
/// Used when a token doesn't have an associated source file or name.
pub(crate) const INVALID_ID: u32 = u32::MAX;

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

    /// Construct a `Token` directly from raw u32 ids using `INVALID_ID`
    /// (`u32::MAX`) to mean "absent". Skips the `Option<u32> → u32`
    /// roundtrip for hot decode/concat loops that already track the
    /// sentinel value directly.
    #[inline]
    pub(crate) fn new_raw(
        dst_line: u32,
        dst_col: u32,
        src_line: u32,
        src_col: u32,
        source_id: u32,
        name_id: u32,
    ) -> Self {
        Self { dst_line, dst_col, src_line, src_col, source_id, name_id }
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
///
/// Two lifetimes:
/// * `'sm` — the borrow of the [`SourceMap`] reference itself.
/// * `'data` — the underlying string data inside the [`SourceMap`] (the input
///   JSON buffer, for maps parsed via [`SourceMap::from_json_string`]).
#[derive(Debug, Clone, Copy)]
pub struct SourceViewToken<'sm, 'data> {
    pub(crate) token: Token,
    pub(crate) sourcemap: &'sm SourceMap<'data>,
}

impl<'sm, 'data> SourceViewToken<'sm, 'data> {
    pub fn new(token: Token, sourcemap: &'sm SourceMap<'data>) -> Self {
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

    pub fn get_name(&self) -> Option<&'sm str> {
        if self.token.name_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_name(self.token.name_id)
        }
    }

    pub fn get_source(&self) -> Option<&'sm str> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source(self.token.source_id)
        }
    }

    pub fn get_source_content(&self) -> Option<&'sm str> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source_content(self.token.source_id)
        }
    }

    pub fn get_source_and_content(&self) -> Option<(&'sm str, &'sm str)> {
        if self.token.source_id == INVALID_ID {
            None
        } else {
            self.sourcemap.get_source_and_content(self.token.source_id)
        }
    }

    pub fn to_tuple(&self) -> (Option<&'sm str>, u32, u32, Option<&'sm str>) {
        (self.get_source(), self.get_src_line(), self.get_src_col(), self.get_name())
    }
}
