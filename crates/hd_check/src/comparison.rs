//! Optional checked source facts for tools. No diagnostic or policy lives here.

use hd_base::{DefId, LocalId, NodeIdx, Span};
use hd_syntax::TokenKind;
use hd_types::Prim;

/// An operand's source node and checked primitive type; its written form is
/// read from syntax by the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operand {
    pub node: NodeIdx,
    pub primitive: Option<Prim>,
}

/// One source ordering comparison; consumers decide whether it needs advice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub span: Span,
    pub operator: TokenKind,
    pub left: Operand,
    pub right: Operand,
}

/// One source binding after body checking has recorded every read and capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub body: DefId,
    pub local: LocalId,
    pub name: String,
    pub span: Span,
    pub read: bool,
    /// The binding used ordinary `:=` or `let` for a value that implements a
    /// disposal protocol. Policy and diagnostics stay in `hd_lint`.
    pub unbound_disposable: bool,
    pub mutability: Option<Mutability>,
}

/// Checked properties of a written `let mut` binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mutability {
    pub primitive: bool,
    pub address_taken: bool,
    /// The binding's pattern, where `mut` is written.
    pub pattern: NodeIdx,
}

/// The resolved payload layout of an enum pattern: the variant's field
/// names and, per field position, the sub-pattern written for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantPattern {
    pub node: NodeIdx,
    pub variant: String,
    pub fields: Vec<String>,
    pub subs: Vec<Option<NodeIdx>>,
}

/// A statement's source reachability, before dead code can be lowered away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub body: DefId,
    pub span: Span,
    pub reachable: bool,
}

/// The first checked change to a `mut` parameter or `mut self` copy in one
/// direct statement of its function body. Whether a later use observes it
/// is read from syntax by the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterWrite {
    pub body: NodeIdx,
    pub statement: NodeIdx,
    pub write: NodeIdx,
    pub name: String,
}

/// An implicit expression statement; explicit `_ := ...` is not a discard
/// here. Its call shape is read from syntax by the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscardedExpression {
    pub node: NodeIdx,
    /// The concrete item selected by checking, when the call has one.
    pub selected: Option<DefId>,
    /// Declaration identity, including qualified calls and type arguments.
    pub prelude_debug: bool,
    /// The discarded value implements a disposal protocol. Policy and
    /// diagnostics stay in `hd_lint`.
    pub unbound_disposable: bool,
    /// For a `std.format.debug` call by a bare name: whether the bare names
    /// `debug` and `dbg` resolve to their std.format declarations here.
    /// Otherwise both are false.
    pub standard_debug: bool,
    pub standard_dbg: bool,
}

/// Ephemeral observations requested by tooling, never cached with checked bodies.
#[derive(Clone, Debug, Default)]
pub struct SourceFacts {
    pub comparisons: Vec<Comparison>,
    pub bindings: Vec<Binding>,
    pub patterns: Vec<VariantPattern>,
    pub statements: Vec<Statement>,
    pub parameter_writes: Vec<ParameterWrite>,
    pub discarded: Vec<DiscardedExpression>,
}

impl SourceFacts {
    pub fn extend(&mut self, other: Self) {
        self.comparisons.extend(other.comparisons);
        self.bindings.extend(other.bindings);
        self.patterns.extend(other.patterns);
        self.statements.extend(other.statements);
        self.parameter_writes.extend(other.parameter_writes);
        self.discarded.extend(other.discarded);
    }
}
