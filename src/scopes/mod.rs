//! Scope and binding information from the ECMA-426
//! [Scopes proposal](https://github.com/tc39/ecma426/blob/main/proposals/scopes.md).
//!
//! The encoding follows the draft spec (<https://tc39.es/ecma426/branch/proposal-scopes/>):
//!
//! * `"scopes"` is a list with one entry per `sources` entry. Each entry is a string of encoded
//!   [`OriginalScope`] trees, or `null` when there is no scope information for that source.
//! * `"ranges"` is a list of strings ("segments"), each encoding consecutive top-level
//!   [`GeneratedRange`] trees. Every segment starts from a fresh decoder state, which lets tools
//!   concatenate segments without re-encoding them.
//!
//! All names (scope names, kinds, variables and binding expressions) are indices into the source
//! map's `names` array, the same way [`Token`](crate::Token) refers to names.

mod decode;
mod encode;

pub(crate) use decode::decode_scope_info;
pub(crate) use encode::{Offsets, encode_scope_info};

/// `B`: start of an original scope.
const TAG_ORIGINAL_SCOPE_START: u64 = 0x1;
/// `C`: end of an original scope.
const TAG_ORIGINAL_SCOPE_END: u64 = 0x2;
/// `D`: variables of the current original scope.
const TAG_ORIGINAL_SCOPE_VARIABLES: u64 = 0x3;
/// `E`: start of a generated range.
const TAG_GENERATED_RANGE_START: u64 = 0x4;
/// `F`: end of a generated range.
const TAG_GENERATED_RANGE_END: u64 = 0x5;
/// `G`: binding expressions of the current generated range.
const TAG_GENERATED_RANGE_BINDINGS: u64 = 0x6;
/// `H`: sub-range bindings for one variable of the current generated range.
const TAG_GENERATED_RANGE_SUB_RANGE_BINDING: u64 = 0x7;
/// `I`: absolute call site of the current (inlined) generated range.
const TAG_GENERATED_RANGE_CALL_SITE: u64 = 0x8;
/// `J`: call site relative to the definition of an enclosing generated range.
const TAG_GENERATED_RANGE_RELATIVE_CALL_SITE: u64 = 0x9;

const SCOPE_FLAG_HAS_NAME: u64 = 0x1;
const SCOPE_FLAG_HAS_KIND: u64 = 0x2;
const SCOPE_FLAG_IS_STACK_FRAME: u64 = 0x4;

const RANGE_FLAG_HAS_LINE: u64 = 0x1;
const RANGE_FLAG_HAS_DEFINITION: u64 = 0x2;
const RANGE_FLAG_IS_STACK_FRAME: u64 = 0x4;
const RANGE_FLAG_IS_HIDDEN: u64 = 0x8;

/// A zero-based line and column.
///
/// Whether it refers to an original source or to the generated code depends on context.
/// Positions order by line first, then column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

impl Position {
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// A zero-based line and column in the original source at `source_index` in `sources`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OriginalPosition {
    pub source_index: u32,
    pub line: u32,
    pub column: u32,
}

impl OriginalPosition {
    pub const fn new(source_index: u32, line: u32, column: u32) -> Self {
        Self { source_index, line, column }
    }
}

/// A lexical scope in an original source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OriginalScope {
    /// Where the scope starts (inclusive).
    pub start: Position,
    /// Where the scope ends (exclusive).
    pub end: Position,
    /// Index into `names` of the scope's original name, such as a function or class name.
    pub name: Option<u32>,
    /// Index into `names` of a label for the scope, purely informative for debuggers.
    ///
    /// For JavaScript-like languages the spec suggests `Global`, `Script`/`Module`, `Function`,
    /// `Class` and `Block`.
    pub kind: Option<u32>,
    /// Whether running this scope adds a frame to the call stack (functions, methods, ...).
    pub is_stack_frame: bool,
    /// Indices into `names` of the variables this scope declares.
    pub variables: Vec<u32>,
    /// Nested scopes, sorted and non-overlapping, within `start..end`.
    pub children: Vec<OriginalScope>,
}

/// Refers to an [`OriginalScope`]: the `scope_index`-th scope in a pre-order traversal of the
/// scope trees of the source at `source_index`.
///
/// Resolve it with [`ScopeInfo::original_scope`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ScopeRef {
    pub source_index: u32,
    pub scope_index: u32,
}

impl ScopeRef {
    pub const fn new(source_index: u32, scope_index: u32) -> Self {
        Self { source_index, scope_index }
    }
}

/// Whether a [`GeneratedRange`] is a function in the generated code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum StackFrameType {
    /// The range is not a JavaScript/Wasm function, e.g. a block or an inlined function body.
    #[default]
    None,
    /// The range is a JavaScript/Wasm function that shows up in stack traces.
    Original,
    /// The range is a function inserted by a compiler, e.g. a block scope outlined into a
    /// function. Debuggers hide its frames in translated stack traces.
    Hidden,
}

/// The JavaScript expression that retrieves a variable's value, starting at `from`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    /// The generated position from which this binding applies, until the next binding of the same
    /// variable or the end of the range.
    pub from: Position,
    /// Index into `names` of the expression, or `None` if the variable is unavailable.
    pub expression: Option<u32>,
}

impl Binding {
    pub const fn new(from: Position, expression: Option<u32>) -> Self {
        Self { from, expression }
    }
}

/// A range of generated code, which may correspond to an [`OriginalScope`].
///
/// A range is not necessarily a lexical scope of the generated code: an inlined function body or
/// a concatenated bundle can also be a range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedRange {
    /// Where the range starts (inclusive).
    pub start: Position,
    /// Where the range ends (exclusive).
    pub end: Position,
    /// The original scope this range corresponds to.
    pub definition: Option<ScopeRef>,
    /// Whether this range is a function in the generated code.
    pub stack_frame_type: StackFrameType,
    /// For an inlined function body: where the inlined function was called in the original source.
    ///
    /// A range with a call site must have a `definition` whose scope `is_stack_frame`, and its
    /// `stack_frame_type` must be [`StackFrameType::None`].
    pub call_site: Option<OriginalPosition>,
    /// How to retrieve the value of each variable of the `definition` scope in this range.
    ///
    /// Either empty, or one entry per variable of the definition scope, in the same order. Each
    /// entry lists bindings by increasing `from`, all within the range. When encoding, an empty
    /// list means the variable is unavailable in the whole range. Decoded entries always start
    /// with a binding at `start`, whose `expression` is `None` while the variable is unavailable.
    pub bindings: Vec<Vec<Binding>>,
    /// Nested ranges, sorted and non-overlapping, within `start..end`.
    pub children: Vec<GeneratedRange>,
}

/// The decoded `"scopes"` and `"ranges"` of a source map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeInfo {
    /// One entry per `sources` entry: the top-level scopes of that source, or `None` when the
    /// source has no scope information.
    pub scopes: Vec<Option<Vec<OriginalScope>>>,
    /// The top-level generated ranges, sorted and non-overlapping.
    pub ranges: Vec<GeneratedRange>,
}

impl ScopeInfo {
    /// Resolve a [`ScopeRef`], such as a [`GeneratedRange::definition`], to its scope.
    pub fn original_scope(&self, scope: ScopeRef) -> Option<&OriginalScope> {
        let roots = self.scopes.get(scope.source_index as usize)?.as_deref()?;
        let mut remaining = scope.scope_index;
        let mut stack: Vec<&OriginalScope> = roots.iter().rev().collect();
        while let Some(current) = stack.pop() {
            if remaining == 0 {
                return Some(current);
            }
            remaining -= 1;
            stack.extend(current.children.iter().rev());
        }
        None
    }
}

/// How [`SourceMap::decode_scope_info`](crate::SourceMap::decode_scope_info) treats malformed
/// scope information.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ScopeDecodeMode {
    /// Decode like a spec-compliant consumer, which never fails.
    ///
    /// Items that cannot be used are dropped: references to missing names, sources or scopes
    /// become `None`, and misplaced items are ignored. Variable indices are kept as-is even when
    /// they are out of bounds, so bindings still line up with variables. A string that does not
    /// parse discards its contents: the source's entry becomes `None`, or all ranges are dropped.
    #[default]
    Lax,
    /// Fail with an [`Error`](crate::Error) on any malformed or inconsistent item.
    Strict,
}

#[cfg(test)]
mod tests;
