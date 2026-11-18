//! Decoding of the `"scopes"` and `"ranges"` fields into a [`ScopeInfo`].

use std::borrow::Cow;

use super::*;
use crate::error::{Error, Result};

/// Decode the raw `"scopes"` and `"ranges"` fields of a source map.
///
/// Returns `None` when both fields are absent. Otherwise, `scopes` has exactly one entry per
/// source.
pub(crate) fn decode_scope_info(
    scopes: Option<&[Option<Cow<'_, str>>]>,
    ranges: Option<&[Cow<'_, str>]>,
    names_len: usize,
    sources_len: usize,
    mode: ScopeDecodeMode,
) -> Result<Option<ScopeInfo>> {
    if scopes.is_none() && ranges.is_none() {
        return Ok(None);
    }

    let mut decoder =
        Decoder { mode, names_len, sources_len, flat: Vec::with_capacity(sources_len) };

    let encoded_scopes = scopes.unwrap_or_default();
    if encoded_scopes.len() > sources_len {
        decoder.report(Error::InvalidScopes("\"scopes\" has more entries than \"sources\""))?;
    }
    let mut decoded_scopes = Vec::with_capacity(sources_len);
    for index in 0..sources_len {
        let decoded = match encoded_scopes.get(index) {
            Some(Some(encoded)) => decoder.source_scopes(encoded)?,
            _ => {
                decoder.flat.push(Vec::new());
                None
            }
        };
        decoded_scopes.push(decoded);
    }

    let ranges = match ranges {
        Some(segments) => decoder.ranges(segments)?,
        None => Vec::new(),
    };

    Ok(Some(ScopeInfo { scopes: decoded_scopes, ranges }))
}

/// What range decoding needs to know about an original scope, indexed in pre-order.
struct FlatScope {
    start: Position,
    variables_len: usize,
}

struct Decoder {
    mode: ScopeDecodeMode,
    names_len: usize,
    sources_len: usize,
    /// Per source, the pre-order list of its decoded scopes.
    flat: Vec<Vec<FlatScope>>,
}

/// A generated range whose end item has not been seen yet.
struct OpenRange {
    range: GeneratedRange,
    /// Sub-range bindings are applied when the range ends, so they work regardless of whether
    /// they come before or after the bindings item.
    sub_range_bindings: Vec<(u64, Vec<Binding>)>,
    has_bindings: bool,
}

impl Decoder {
    /// Report a recoverable problem: an error in strict mode, ignored in lax mode.
    fn report(&self, error: Error) -> Result<()> {
        match self.mode {
            ScopeDecodeMode::Strict => Err(error),
            ScopeDecodeMode::Lax => Ok(()),
        }
    }

    fn source_scopes(&mut self, encoded: &str) -> Result<Option<Vec<OriginalScope>>> {
        match self.try_source_scopes(encoded) {
            Ok((roots, flat)) => {
                self.flat.push(flat);
                Ok(Some(roots))
            }
            Err(error) if self.mode == ScopeDecodeMode::Strict => Err(error),
            // Per the spec, a string that fails to parse contributes no scopes.
            Err(_) => {
                self.flat.push(Vec::new());
                Ok(None)
            }
        }
    }

    fn try_source_scopes(&self, encoded: &str) -> Result<(Vec<OriginalScope>, Vec<FlatScope>)> {
        let mut roots = Vec::new();
        let mut flat = Vec::new();
        // Open scopes, each with its index into `flat`.
        let mut stack: Vec<(OriginalScope, usize)> = Vec::new();

        let mut position = PositionAccumulator::default();
        let mut name = 0;
        let mut kind = 0;
        let mut variable = 0;

        let mut values = Vec::new();
        for item in items(encoded) {
            parse_item(item, &mut values)?;
            let mut fields = Fields::new(&values);
            let Some(tag) = fields.next() else {
                self.report(Error::InvalidScopes("empty item"))?;
                continue;
            };
            match tag {
                TAG_ORIGINAL_SCOPE_START => {
                    let flags = fields.unsigned()?;
                    let start = position.accumulate(fields.unsigned()?, fields.unsigned()?)?;
                    let name = if flags & SCOPE_FLAG_HAS_NAME != 0 {
                        self.relative_name(&mut name, fields.signed()?)?
                    } else {
                        None
                    };
                    let kind = if flags & SCOPE_FLAG_HAS_KIND != 0 {
                        self.relative_name(&mut kind, fields.signed()?)?
                    } else {
                        None
                    };
                    // Any further values are reserved for future extensions.
                    let scope = OriginalScope {
                        start,
                        end: start,
                        name,
                        kind,
                        is_stack_frame: flags & SCOPE_FLAG_IS_STACK_FRAME != 0,
                        variables: Vec::new(),
                        children: Vec::new(),
                    };
                    stack.push((scope, flat.len()));
                    flat.push(FlatScope { start, variables_len: 0 });
                }
                TAG_ORIGINAL_SCOPE_END => {
                    let end = position.accumulate(fields.unsigned()?, fields.unsigned()?)?;
                    let Some((mut scope, _)) = stack.pop() else {
                        return Err(Error::InvalidScopes("scope end without a matching start"));
                    };
                    scope.end = end;
                    match stack.last_mut() {
                        Some((parent, _)) => parent.children.push(scope),
                        None => roots.push(scope),
                    }
                }
                TAG_ORIGINAL_SCOPE_VARIABLES => {
                    let Some((scope, flat_index)) = stack.last_mut() else {
                        self.report(Error::InvalidScopes("variables outside of a scope"))?;
                        continue;
                    };
                    if !scope.variables.is_empty() {
                        self.report(Error::InvalidScopes("duplicate variables item"))?;
                    }
                    if !fields.has_next() {
                        self.report(Error::InvalidScopes("variables item without variables"))?;
                    }
                    while let Some(value) = fields.next() {
                        let index = self.accumulate_index(&mut variable, signed(value))?;
                        if index as usize >= self.names_len {
                            self.report(Error::BadNameReference(index))?;
                        }
                        // Out-of-bounds indices are kept so bindings stay aligned with variables.
                        scope.variables.push(index);
                    }
                    flat[*flat_index].variables_len = scope.variables.len();
                }
                // Vendor extensions and unknown items are ignored.
                _ => {}
            }
        }

        if !stack.is_empty() {
            return Err(Error::InvalidScopes("scope start without a matching end"));
        }
        Ok((roots, flat))
    }

    fn ranges(&self, segments: &[Cow<'_, str>]) -> Result<Vec<GeneratedRange>> {
        let mut result: Vec<GeneratedRange> = Vec::new();
        for segment in segments {
            match self.try_segment(segment) {
                Ok(mut ranges) => {
                    if let (Some(previous), Some(next)) = (result.last(), ranges.first())
                        && next.start < previous.end
                    {
                        self.report(Error::InvalidScopes(
                            "range segment starts before the previous segment ends",
                        ))?;
                    }
                    result.append(&mut ranges);
                }
                Err(error) if self.mode == ScopeDecodeMode::Strict => return Err(error),
                // Per the spec, a segment that fails to parse discards all ranges.
                Err(_) => return Ok(Vec::new()),
            }
        }
        Ok(result)
    }

    fn try_segment(&self, segment: &str) -> Result<Vec<GeneratedRange>> {
        let mut result = Vec::new();
        let mut stack: Vec<OpenRange> = Vec::new();

        let mut position = PositionAccumulator::default();
        let mut definition_source = 0;
        let mut definition_scope = 0;

        let mut values = Vec::new();
        for item in items(segment) {
            parse_item(item, &mut values)?;
            let mut fields = Fields::new(&values);
            let Some(tag) = fields.next() else {
                self.report(Error::InvalidScopes("empty item"))?;
                continue;
            };
            match tag {
                TAG_GENERATED_RANGE_START => {
                    let flags = fields.unsigned()?;
                    let line =
                        if flags & RANGE_FLAG_HAS_LINE != 0 { fields.unsigned()? } else { 0 };
                    let start = position.accumulate(line, fields.unsigned()?)?;
                    let definition = if flags & RANGE_FLAG_HAS_DEFINITION != 0 {
                        let source_offset = fields.signed()?;
                        let source =
                            self.accumulate_index(&mut definition_source, source_offset)?;
                        // The scope index is relative within the same source, but absolute after
                        // switching to a different source.
                        let scope_offset = if source_offset == 0 {
                            fields.signed()?
                        } else {
                            definition_scope = 0;
                            i64::try_from(fields.unsigned()?).map_err(|_| Error::VlqOverflow)?
                        };
                        let scope = self.accumulate_index(&mut definition_scope, scope_offset)?;
                        self.resolve_definition(source, scope)?
                    } else {
                        None
                    };
                    let is_stack_frame = flags & RANGE_FLAG_IS_STACK_FRAME != 0;
                    let is_hidden = flags & RANGE_FLAG_IS_HIDDEN != 0;
                    let stack_frame_type = match (is_stack_frame, is_hidden) {
                        (true, true) => StackFrameType::Hidden,
                        (true, false) => StackFrameType::Original,
                        (false, is_hidden) => {
                            if is_hidden {
                                self.report(Error::InvalidScopes(
                                    "hidden range that is not a stack frame",
                                ))?;
                            }
                            StackFrameType::None
                        }
                    };
                    // Any further values are reserved for future extensions.
                    stack.push(OpenRange {
                        range: GeneratedRange {
                            start,
                            end: start,
                            definition,
                            stack_frame_type,
                            ..GeneratedRange::default()
                        },
                        sub_range_bindings: Vec::new(),
                        has_bindings: false,
                    });
                }
                TAG_GENERATED_RANGE_END => {
                    // The line is only present when it changes, which is told apart by the
                    // number of values.
                    let (line, column) = match fields.remaining() {
                        1 => (0, fields.unsigned()?),
                        2 => (fields.unsigned()?, fields.unsigned()?),
                        _ => {
                            return Err(Error::InvalidScopes(
                                "range end must have one or two values",
                            ));
                        }
                    };
                    let end = position.accumulate(line, column)?;
                    let Some(open) = stack.pop() else {
                        return Err(Error::InvalidScopes("range end without a matching start"));
                    };
                    let mut range = open.range;
                    range.end = end;
                    for (variable_index, mut bindings) in open.sub_range_bindings {
                        match usize::try_from(variable_index)
                            .ok()
                            .and_then(|index| range.bindings.get_mut(index))
                        {
                            Some(variable_bindings) => variable_bindings.append(&mut bindings),
                            None => self.report(Error::InvalidScopes(
                                "sub-range binding for a variable without a binding",
                            ))?,
                        }
                    }
                    match stack.last_mut() {
                        Some(parent) => parent.range.children.push(range),
                        None => result.push(range),
                    }
                }
                TAG_GENERATED_RANGE_BINDINGS => {
                    let Some(open) = stack.last_mut() else {
                        self.report(Error::InvalidScopes("bindings outside of a range"))?;
                        continue;
                    };
                    if open.has_bindings {
                        self.report(Error::InvalidScopes("duplicate bindings item"))?;
                    }
                    open.has_bindings = true;
                    let start = open.range.start;
                    let mut bindings = Vec::with_capacity(fields.remaining());
                    while let Some(value) = fields.next() {
                        bindings.push(vec![Binding::new(start, self.binding_expression(value)?)]);
                    }
                    if self.mode == ScopeDecodeMode::Strict {
                        let variables_len = open
                            .range
                            .definition
                            .map(|definition| self.flat_scope(definition).variables_len);
                        if variables_len != Some(bindings.len()) {
                            return Err(Error::InvalidScopes(
                                "range bindings do not match the variables of its definition",
                            ));
                        }
                    }
                    open.range.bindings = bindings;
                }
                TAG_GENERATED_RANGE_SUB_RANGE_BINDING => {
                    let Some(open) = stack.last_mut() else {
                        self.report(Error::InvalidScopes("sub-range binding outside of a range"))?;
                        continue;
                    };
                    let variable_index = fields.unsigned()?;
                    // Each sub-range is relative to the previous one, starting at the range start.
                    let mut from = PositionAccumulator::at(open.range.start);
                    let mut bindings = Vec::new();
                    while let Some(line) = fields.next() {
                        let from = from.accumulate(line, fields.unsigned()?)?;
                        bindings
                            .push(Binding::new(from, self.binding_expression(fields.unsigned()?)?));
                    }
                    if bindings.is_empty() {
                        return Err(Error::InvalidScopes("sub-range binding without bindings"));
                    }
                    if open.sub_range_bindings.iter().any(|(index, _)| *index == variable_index) {
                        self.report(Error::InvalidScopes(
                            "duplicate sub-range binding for a variable",
                        ))?;
                    }
                    open.sub_range_bindings.push((variable_index, bindings));
                }
                TAG_GENERATED_RANGE_CALL_SITE => {
                    let source = to_u32(fields.unsigned()?)?;
                    let line = to_u32(fields.unsigned()?)?;
                    let column = to_u32(fields.unsigned()?)?;
                    let Some(open) = stack.last_mut() else {
                        self.report(Error::InvalidScopes("call site outside of a range"))?;
                        continue;
                    };
                    if source as usize >= self.sources_len {
                        self.report(Error::BadSourceReference(source))?;
                        continue;
                    }
                    self.set_call_site(open, OriginalPosition::new(source, line, column))?;
                }
                TAG_GENERATED_RANGE_RELATIVE_CALL_SITE => {
                    let relative = match fields.remaining() {
                        3 => RelativeCallSite::LineColumn {
                            parent_index: fields.unsigned()?,
                            line_offset: fields.unsigned()?,
                            column: fields.unsigned()?,
                        },
                        2 => RelativeCallSite::LineColumn {
                            parent_index: 0,
                            line_offset: fields.unsigned()?,
                            column: fields.unsigned()?,
                        },
                        1 => RelativeCallSite::Column { column_offset: fields.unsigned()? },
                        _ => {
                            return Err(Error::InvalidScopes(
                                "relative call site must have one to three values",
                            ));
                        }
                    };
                    let parent_index = match relative {
                        RelativeCallSite::LineColumn { parent_index, .. } => parent_index,
                        RelativeCallSite::Column { .. } => 0,
                    };
                    // The call site is relative to the definition of an enclosing range,
                    // counting only ranges with a definition, nearest first.
                    let Some((open, parents)) = stack.split_last_mut() else {
                        self.report(Error::InvalidScopes("call site outside of a range"))?;
                        continue;
                    };
                    let parent = usize::try_from(parent_index).ok().and_then(|parent_index| {
                        parents
                            .iter()
                            .rev()
                            .filter_map(|parent| parent.range.definition)
                            .nth(parent_index)
                    });
                    let Some(parent) = parent else {
                        self.report(Error::InvalidScopes(
                            "relative call site without an enclosing definition",
                        ))?;
                        continue;
                    };
                    let parent_start = self.flat_scope(parent).start;
                    let call_site = match relative {
                        RelativeCallSite::Column { column_offset } => OriginalPosition::new(
                            parent.source_index,
                            parent_start.line,
                            to_u32(u64::from(parent_start.column) + column_offset)?,
                        ),
                        RelativeCallSite::LineColumn { line_offset, column, .. } => {
                            OriginalPosition::new(
                                parent.source_index,
                                to_u32(u64::from(parent_start.line) + line_offset)?,
                                to_u32(column)?,
                            )
                        }
                    };
                    self.set_call_site(open, call_site)?;
                }
                // Vendor extensions and unknown items are ignored.
                _ => {}
            }
        }

        if !stack.is_empty() {
            return Err(Error::InvalidScopes("range start without a matching end"));
        }
        Ok(result)
    }

    fn set_call_site(&self, open: &mut OpenRange, call_site: OriginalPosition) -> Result<()> {
        if open.range.call_site.is_some() {
            self.report(Error::InvalidScopes("duplicate call site item"))?;
        }
        open.range.call_site = Some(call_site);
        Ok(())
    }

    fn flat_scope(&self, scope: ScopeRef) -> &FlatScope {
        // Definitions are only created by `resolve_definition`, which checks both indices.
        &self.flat[scope.source_index as usize][scope.scope_index as usize]
    }

    fn resolve_definition(&self, source: u32, scope: u32) -> Result<Option<ScopeRef>> {
        let Some(scopes) = self.flat.get(source as usize) else {
            self.report(Error::BadSourceReference(source))?;
            return Ok(None);
        };
        if scope as usize >= scopes.len() {
            self.report(Error::InvalidScopes("range definition refers to a missing scope"))?;
            return Ok(None);
        }
        Ok(Some(ScopeRef::new(source, scope)))
    }

    /// Apply a relative index to an accumulator. Negative results clamp to zero.
    fn accumulate_index(&self, accumulator: &mut i64, offset: i64) -> Result<u32> {
        let mut index = accumulator.checked_add(offset).ok_or(Error::VlqOverflow)?;
        if index < 0 {
            self.report(Error::InvalidScopes("relative index is negative"))?;
            index = 0;
        }
        *accumulator = index;
        u32::try_from(index).map_err(|_| Error::VlqOverflow)
    }

    /// Apply a relative name index, dropping references outside `names`.
    fn relative_name(&self, accumulator: &mut i64, offset: i64) -> Result<Option<u32>> {
        let index = self.accumulate_index(accumulator, offset)?;
        if index as usize >= self.names_len {
            self.report(Error::BadNameReference(index))?;
            return Ok(None);
        }
        Ok(Some(index))
    }

    /// Resolve a 1-based binding expression index, where `0` means unavailable.
    fn binding_expression(&self, value: u64) -> Result<Option<u32>> {
        let Some(index) = value.checked_sub(1) else { return Ok(None) };
        match u32::try_from(index) {
            Ok(index) if (index as usize) < self.names_len => Ok(Some(index)),
            _ => {
                self.report(Error::BadNameReference(u32::try_from(index).unwrap_or(u32::MAX)))?;
                Ok(None)
            }
        }
    }
}

/// The forms of a `J` item, relative to the start of an enclosing range's definition.
#[derive(Clone, Copy)]
enum RelativeCallSite {
    /// Same line as the definition start, column offset from its column.
    Column { column_offset: u64 },
    /// Line offset from the definition start of the `parent_index`-th enclosing definition, with
    /// an absolute column.
    LineColumn { parent_index: u64, line_offset: u64, column: u64 },
}

/// Tracks a position encoded as consecutive relative increments.
#[derive(Default)]
struct PositionAccumulator {
    line: u32,
    column: u32,
}

impl PositionAccumulator {
    fn at(position: Position) -> Self {
        Self { line: position.line, column: position.column }
    }

    /// Advance by `line` lines. The column is relative when staying on the same line, absolute
    /// otherwise.
    fn accumulate(&mut self, line: u64, column: u64) -> Result<Position> {
        if line == 0 {
            self.column = to_u32(u64::from(self.column) + column)?;
        } else {
            self.line = to_u32(u64::from(self.line) + line)?;
            self.column = to_u32(column)?;
        }
        Ok(Position::new(self.line, self.column))
    }
}

fn to_u32(value: u64) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::VlqOverflow)
}

/// Split an encoded string into its comma-separated items. An empty string has no items.
fn items(encoded: &str) -> impl Iterator<Item = &str> {
    (!encoded.is_empty()).then(|| encoded.split(',')).into_iter().flatten()
}

/// The raw (unsigned) VLQ values of one item, starting with its tag.
struct Fields<'v> {
    values: &'v [u64],
    index: usize,
}

impl<'v> Fields<'v> {
    fn new(values: &'v [u64]) -> Self {
        Self { values, index: 0 }
    }

    fn next(&mut self) -> Option<u64> {
        let value = self.values.get(self.index).copied()?;
        self.index += 1;
        Some(value)
    }

    fn has_next(&self) -> bool {
        self.index < self.values.len()
    }

    fn remaining(&self) -> usize {
        self.values.len() - self.index
    }

    fn unsigned(&mut self) -> Result<u64> {
        self.next().ok_or(Error::InvalidScopes("item is missing a value"))
    }

    fn signed(&mut self) -> Result<i64> {
        self.unsigned().map(signed)
    }
}

/// Interpret a raw VLQ value as signed: the sign is stored in the lowest bit.
fn signed(value: u64) -> i64 {
    // `value >> 1` has its top bit clear, so the cast cannot wrap.
    let magnitude = (value >> 1) as i64;
    if value & 1 == 1 { -magnitude } else { magnitude }
}

/// Decode all base64 VLQ values of `item` into `values` (cleared first), without sign handling.
fn parse_item(item: &str, values: &mut Vec<u64>) -> Result<()> {
    values.clear();
    let mut bytes = item.bytes();
    while let Some(byte) = bytes.next() {
        let mut digit = base64_value(byte)?;
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            let payload = u64::from(digit & 0b1_1111);
            if shift >= u64::BITS || (payload << shift) >> shift != payload {
                return Err(Error::VlqOverflow);
            }
            value |= payload << shift;
            if digit & 0b10_0000 == 0 {
                break;
            }
            shift += 5;
            // A continuation bit on the last digit of the item.
            let Some(byte) = bytes.next() else { return Err(Error::VlqLeftover) };
            digit = base64_value(byte)?;
        }
        values.push(value);
    }
    Ok(())
}

fn base64_value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::InvalidScopes("invalid base64 character")),
    }
}
