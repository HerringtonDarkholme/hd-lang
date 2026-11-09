//! Optional checked facts for tools: only what needs checker state. Names,
//! spans and written forms are read from syntax by the consumer, and no
//! diagnostic or policy lives here.

use hd_base::{DefId, LocalId, NodeIdx};
use hd_types::Prim;

/// One ordering comparison: its binary node and each operand's checked
/// primitive type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub node: NodeIdx,
    pub left: Option<Prim>,
    pub right: Option<Prim>,
}

/// One source binding after body checking has recorded every read and capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub body: DefId,
    pub local: LocalId,
    /// The syntax the binding was declared by.
    pub node: NodeIdx,
    /// The syntax whose first identifier is the binding's name: `node`, or
    /// a recursive closure's binding pattern.
    pub name: NodeIdx,
    pub read: bool,
    /// The binding used ordinary `:=` or `let` for a value that implements a
    /// disposal protocol.
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

/// A checked change to a `mut` parameter or `mut self` copy. The consumer
/// groups the changes by the body statement they sit in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterWrite {
    /// The function body block.
    pub body: NodeIdx,
    /// The `Parameter` node of the changed parameter.
    pub parameter: NodeIdx,
    /// The syntax recorded as the change itself.
    pub write: NodeIdx,
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
    /// The discarded value implements a disposal protocol.
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
    /// The statements after a diverging one in their suite, before dead
    /// code can be lowered away.
    pub unreachable: Vec<NodeIdx>,
    pub parameter_writes: Vec<ParameterWrite>,
    pub discarded: Vec<DiscardedExpression>,
}

impl SourceFacts {
    pub fn extend(&mut self, other: Self) {
        self.comparisons.extend(other.comparisons);
        self.bindings.extend(other.bindings);
        self.patterns.extend(other.patterns);
        self.unreachable.extend(other.unreachable);
        self.parameter_writes.extend(other.parameter_writes);
        self.discarded.extend(other.discarded);
    }
}
