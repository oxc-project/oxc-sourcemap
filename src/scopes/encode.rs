//! Encoding of a [`ScopeInfo`] into the `"scopes"` and `"ranges"` fields.

use super::*;
use crate::error::{Error, Result};

/// The encoded fields of a [`ScopeInfo`].
pub(crate) struct EncodedScopeInfo {
    /// One entry per source.
    pub scopes: Vec<Option<String>>,
    /// A single segment with all ranges, or `None` when there are no ranges.
    pub ranges: Option<String>,
}

/// Shifts applied while encoding, used to append one map's scope information to a concatenated
/// map: generated lines move by `line`, source indices by `source` and name indices by `name`.
///
/// Validation applies to the unshifted values.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Offsets {
    pub line: u32,
    pub source: u32,
    pub name: u32,
}

/// Encode `info`, checking that it is well-formed.
///
/// Name indices are checked against `names_len` when it is known.
pub(crate) fn encode_scope_info(
    info: &ScopeInfo,
    names_len: Option<usize>,
    offsets: Offsets,
) -> Result<EncodedScopeInfo> {
    let mut encoder = Encoder { info, names_len, offsets, flat: Vec::new() };
    let scopes = encoder.scopes()?;
    let ranges = encoder.ranges()?;
    Ok(EncodedScopeInfo { scopes, ranges })
}

struct Encoder<'i> {
    info: &'i ScopeInfo,
    names_len: Option<usize>,
    offsets: Offsets,
    /// Per source, the pre-order list of its scopes, to resolve range definitions.
    flat: Vec<Vec<&'i OriginalScope>>,
}

#[derive(Default)]
struct ScopeState {
    position: PositionState,
    name: u32,
    kind: u32,
    variable: u32,
}

#[derive(Default)]
struct RangeState {
    position: PositionState,
    definition_source: u32,
    definition_scope: u32,
}

impl<'i> Encoder<'i> {
    fn scopes(&mut self) -> Result<Vec<Option<String>>> {
        let info = self.info;
        let mut result = Vec::with_capacity(info.scopes.len());
        for scopes in &info.scopes {
            let mut flat = Vec::new();
            let encoded = match scopes {
                Some(scopes) => {
                    let mut out = Output::default();
                    let mut state = ScopeState::default();
                    for scope in scopes {
                        self.scope(scope, &mut state, &mut out, &mut flat)?;
                    }
                    Some(out.0)
                }
                None => None,
            };
            result.push(encoded);
            self.flat.push(flat);
        }
        Ok(result)
    }

    fn scope(
        &self,
        scope: &'i OriginalScope,
        state: &mut ScopeState,
        out: &mut Output,
        flat: &mut Vec<&'i OriginalScope>,
    ) -> Result<()> {
        flat.push(scope);

        let (line, column) = state.position.advance(scope.start)?;
        let mut flags = 0;
        if scope.name.is_some() {
            flags |= SCOPE_FLAG_HAS_NAME;
        }
        if scope.kind.is_some() {
            flags |= SCOPE_FLAG_HAS_KIND;
        }
        if scope.is_stack_frame {
            flags |= SCOPE_FLAG_IS_STACK_FRAME;
        }
        out.item(TAG_ORIGINAL_SCOPE_START);
        out.unsigned(flags);
        out.unsigned(u64::from(line));
        out.unsigned(u64::from(column));
        if let Some(name) = scope.name {
            out.relative(&mut state.name, self.name(name)?);
        }
        if let Some(kind) = scope.kind {
            out.relative(&mut state.kind, self.name(kind)?);
        }

        if !scope.variables.is_empty() {
            out.item(TAG_ORIGINAL_SCOPE_VARIABLES);
            for &variable in &scope.variables {
                out.relative(&mut state.variable, self.name(variable)?);
            }
        }

        for child in &scope.children {
            self.scope(child, state, out, flat)?;
        }

        let (line, column) = state.position.advance(scope.end)?;
        out.item(TAG_ORIGINAL_SCOPE_END);
        out.unsigned(u64::from(line));
        out.unsigned(u64::from(column));
        Ok(())
    }

    fn ranges(&self) -> Result<Option<String>> {
        if self.info.ranges.is_empty() {
            return Ok(None);
        }
        let mut out = Output::default();
        let mut state = RangeState::default();
        for range in &self.info.ranges {
            self.range(range, &mut state, &mut out)?;
        }
        Ok(Some(out.0))
    }

    /// Items are written in the order of the spec grammar: start, bindings, call site, sub-range
    /// bindings, child ranges and end.
    fn range(
        &self,
        range: &GeneratedRange,
        state: &mut RangeState,
        out: &mut Output,
    ) -> Result<()> {
        let start = self.generated(range.start)?;
        let end = self.generated(range.end)?;
        let definition =
            range.definition.map(|definition| self.definition(definition)).transpose()?;

        let (line, column) = state.position.advance(start)?;
        let mut flags = 0;
        if line != 0 {
            flags |= RANGE_FLAG_HAS_LINE;
        }
        if definition.is_some() {
            flags |= RANGE_FLAG_HAS_DEFINITION;
        }
        match range.stack_frame_type {
            StackFrameType::None => {}
            StackFrameType::Original => flags |= RANGE_FLAG_IS_STACK_FRAME,
            StackFrameType::Hidden => flags |= RANGE_FLAG_IS_STACK_FRAME | RANGE_FLAG_IS_HIDDEN,
        }
        out.item(TAG_GENERATED_RANGE_START);
        out.unsigned(flags);
        if line != 0 {
            out.unsigned(u64::from(line));
        }
        out.unsigned(u64::from(column));
        if let Some(definition) = range.definition {
            let source = self.source(definition.source_index)?;
            let source_offset = i64::from(source) - i64::from(state.definition_source);
            out.signed(source_offset);
            // The scope index is relative within the same source, absolute otherwise.
            if source_offset == 0 {
                out.signed(i64::from(definition.scope_index) - i64::from(state.definition_scope));
            } else {
                out.unsigned(u64::from(definition.scope_index));
            }
            state.definition_source = source;
            state.definition_scope = definition.scope_index;
        }

        if !range.bindings.is_empty() {
            let Some(definition) = definition else {
                return Err(Error::InvalidScopes("range has bindings but no definition"));
            };
            if range.bindings.len() != definition.variables.len() {
                return Err(Error::InvalidScopes(
                    "range bindings do not match the variables of its definition",
                ));
            }
            out.item(TAG_GENERATED_RANGE_BINDINGS);
            for bindings in &range.bindings {
                let initial = bindings.first().filter(|binding| binding.from == range.start);
                out.unsigned(self.binding_expression(initial.and_then(|b| b.expression))?);
            }
        }

        if let Some(call_site) = range.call_site {
            let Some(definition) = definition else {
                return Err(Error::InvalidScopes("range has a call site but no definition"));
            };
            if !definition.is_stack_frame {
                return Err(Error::InvalidScopes(
                    "inlined range's definition is not a stack frame",
                ));
            }
            if range.stack_frame_type != StackFrameType::None {
                return Err(Error::InvalidScopes("inlined range is a stack frame"));
            }
            // The absolute form is the only one Chrome DevTools decodes so far; the relative `J`
            // form is decoded but not emitted.
            out.item(TAG_GENERATED_RANGE_CALL_SITE);
            out.unsigned(u64::from(self.source(call_site.source_index)?));
            out.unsigned(u64::from(call_site.line));
            out.unsigned(u64::from(call_site.column));
        }

        for (variable_index, bindings) in range.bindings.iter().enumerate() {
            // The bindings item covers a binding that starts with the range. Everything else,
            // including a variable that only becomes available later, needs a sub-range binding.
            let skip = usize::from(bindings.first().is_some_and(|b| b.from == range.start));
            let sub_ranges = &bindings[skip..];
            if sub_ranges.is_empty() {
                continue;
            }
            out.item(TAG_GENERATED_RANGE_SUB_RANGE_BINDING);
            out.unsigned(variable_index as u64);
            let mut position = PositionState::at(start);
            for binding in sub_ranges {
                if binding.from > range.end {
                    return Err(Error::InvalidScopes("binding starts after the end of its range"));
                }
                let (line, column) = position.advance(self.generated(binding.from)?)?;
                out.unsigned(u64::from(line));
                out.unsigned(u64::from(column));
                out.unsigned(self.binding_expression(binding.expression)?);
            }
        }

        for child in &range.children {
            self.range(child, state, out)?;
        }

        let (line, column) = state.position.advance(end)?;
        out.item(TAG_GENERATED_RANGE_END);
        if line != 0 {
            out.unsigned(u64::from(line));
        }
        out.unsigned(u64::from(column));
        Ok(())
    }

    fn definition(&self, definition: ScopeRef) -> Result<&'i OriginalScope> {
        self.flat
            .get(definition.source_index as usize)
            .and_then(|scopes| scopes.get(definition.scope_index as usize))
            .copied()
            .ok_or(Error::InvalidScopes("range definition refers to a missing scope"))
    }

    /// Validate a name index and shift it.
    fn name(&self, index: u32) -> Result<u32> {
        if self.names_len.is_some_and(|len| index as usize >= len) {
            return Err(Error::BadNameReference(index));
        }
        index.checked_add(self.offsets.name).ok_or(Error::BadNameReference(index))
    }

    /// Validate a source index and shift it.
    fn source(&self, index: u32) -> Result<u32> {
        if index as usize >= self.info.scopes.len() {
            return Err(Error::BadSourceReference(index));
        }
        index.checked_add(self.offsets.source).ok_or(Error::BadSourceReference(index))
    }

    /// Shift a generated position.
    fn generated(&self, position: Position) -> Result<Position> {
        let line = position
            .line
            .checked_add(self.offsets.line)
            .ok_or(Error::InvalidScopes("generated line overflows"))?;
        Ok(Position::new(line, position.column))
    }

    /// Binding expressions are 1-based, with `0` for unavailable.
    fn binding_expression(&self, expression: Option<u32>) -> Result<u64> {
        expression.map_or(Ok(0), |index| Ok(u64::from(self.name(index)?) + 1))
    }
}

/// The previous position, for encoding the next one relatively.
#[derive(Default)]
struct PositionState {
    line: u32,
    column: u32,
}

impl PositionState {
    fn at(position: Position) -> Self {
        Self { line: position.line, column: position.column }
    }

    /// Returns the line delta and the column, which is relative when the line does not change.
    fn advance(&mut self, position: Position) -> Result<(u32, u32)> {
        if position < Position::new(self.line, self.column) {
            return Err(Error::InvalidScopes(
                "scopes and ranges must be sorted, nested and non-overlapping",
            ));
        }
        let line = position.line - self.line;
        let column = if line == 0 { position.column - self.column } else { position.column };
        self.line = position.line;
        self.column = position.column;
        Ok((line, column))
    }
}

#[derive(Default)]
struct Output(String);

impl Output {
    /// Start an item with its tag.
    fn item(&mut self, tag: u64) {
        if !self.0.is_empty() {
            self.0.push(',');
        }
        self.unsigned(tag);
    }

    fn unsigned(&mut self, mut value: u64) {
        const BASE64: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        loop {
            let digit = (value & 0b1_1111) as usize;
            value >>= 5;
            if value == 0 {
                self.0.push(char::from(BASE64[digit]));
                return;
            }
            self.0.push(char::from(BASE64[digit | 0b10_0000]));
        }
    }

    fn signed(&mut self, value: i64) {
        let magnitude = value.unsigned_abs();
        self.unsigned((magnitude << 1) | u64::from(value < 0));
    }

    /// Write `index` relative to the previous one, and remember it.
    fn relative(&mut self, previous: &mut u32, index: u32) {
        self.signed(i64::from(index) - i64::from(*previous));
        *previous = index;
    }
}
