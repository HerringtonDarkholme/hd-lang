//! Narrow queries over the checker's completed facts, for optional analysis.
//!
//! Each query answers something only checking knows: the item a call
//! selected, a binding's checked type, reads and mutability, a type's
//! disposal protocol, and statement reachability. Names, spans and written
//! forms are syntax; the consumer reads them from the tree. Raw compiler
//! identifiers never leave the view.

use std::marker::PhantomData;

use hd_base::{DefId, NodeIdx};
use hd_resolve::Src;
use hd_syntax::NodeRef;
use hd_types::Prim;

use crate::comparison::{
    Binding, Comparison, DiscardedExpression, Mutability, ParameterWrite, SourceFacts,
    VariantPattern,
};

/// An opaque body identity, valid only while its checked view is borrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyRef<'v> {
    def: DefId,
    borrow: PhantomData<&'v SourceFacts>,
}

/// A selected declaration identity. Raw compiler IDs never leave the view.
///
/// ```compile_fail
/// use hd_check::analysis::{CallView, DeclRef};
/// fn escape<'v>(call: CallView<'v>) -> DeclRef<'static> {
///     call.selected_declaration().unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclRef<'v> {
    def: DefId,
    prelude_debug: bool,
    borrow: PhantomData<&'v SourceFacts>,
}

impl DeclRef<'_> {
    #[must_use]
    pub const fn is_prelude_debug(self) -> bool {
        self.prelude_debug
    }
}

/// One checked ordering comparison.
#[derive(Clone, Copy)]
pub struct ComparisonView<'v> {
    fact: &'v Comparison,
    src: Src<'v>,
}

impl<'v> ComparisonView<'v> {
    /// The binary expression: left operand, operator, right operand.
    #[must_use]
    pub fn node(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.fact.node)
    }

    /// The adjusted checked type of the left or right operand, when it is
    /// a primitive.
    #[must_use]
    pub const fn primitive(self, right: bool) -> Option<Prim> {
        if right {
            self.fact.right
        } else {
            self.fact.left
        }
    }
}

/// Final read and permission facts for one binding.
#[derive(Clone, Copy)]
pub struct BindingView<'v> {
    binding: &'v Binding,
    src: Src<'v>,
}

impl<'v> BindingView<'v> {
    #[must_use]
    pub const fn owner(self) -> BodyRef<'v> {
        BodyRef {
            def: self.binding.body,
            borrow: PhantomData,
        }
    }

    /// The syntax the binding was declared by.
    #[must_use]
    pub fn node(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.binding.node)
    }

    /// The syntax whose first identifier is the binding's name.
    #[must_use]
    pub fn name_node(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.binding.name)
    }

    #[must_use]
    pub const fn is_read(self) -> bool {
        self.binding.read
    }

    #[must_use]
    pub const fn is_unbound_disposable(self) -> bool {
        self.binding.unbound_disposable
    }

    #[must_use]
    pub fn mutability(self) -> Option<MutabilityView<'v>> {
        self.binding.mutability.as_ref().map(|fact| MutabilityView {
            fact,
            src: self.src,
        })
    }

    /// Whether `self` was declared before `other` in the same body.
    #[must_use]
    pub fn precedes(self, other: Self) -> bool {
        self.binding.body == other.binding.body
            && self.binding.local.raw() < other.binding.local.raw()
    }
}

/// Checked properties of a written `let mut` binding.
#[derive(Clone, Copy)]
pub struct MutabilityView<'v> {
    fact: &'v Mutability,
    src: Src<'v>,
}

impl<'v> MutabilityView<'v> {
    #[must_use]
    pub const fn is_primitive(self) -> bool {
        self.fact.primitive
    }

    #[must_use]
    pub const fn mutable_address_taken(self) -> bool {
        self.fact.address_taken
    }

    /// The binding's pattern, where `mut` is written.
    #[must_use]
    pub fn pattern(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.fact.pattern)
    }
}

/// The first mutation of a `mut` parameter in one direct body statement.
#[derive(Clone, Copy)]
pub struct ParameterWriteView<'v> {
    write: &'v ParameterWrite,
    src: Src<'v>,
}

impl<'v> ParameterWriteView<'v> {
    /// The parameter's name; `self` for a `mut self` receiver.
    #[must_use]
    pub fn name(self) -> &'v str {
        &self.write.name
    }

    /// The function body block.
    #[must_use]
    pub fn body(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.write.body)
    }

    /// The body's direct statement that contains the write.
    #[must_use]
    pub fn statement(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.write.statement)
    }

    /// The syntax recorded as the mutation itself.
    #[must_use]
    pub fn write(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.write.write)
    }
}

/// Resolved variant layout and the sub-pattern written for each field.
#[derive(Clone, Copy)]
pub struct PatternView<'v> {
    pattern: &'v VariantPattern,
    src: Src<'v>,
}

impl<'v> PatternView<'v> {
    #[must_use]
    pub fn node(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.pattern.node)
    }

    #[must_use]
    pub fn variant(self) -> &'v str {
        &self.pattern.variant
    }

    #[must_use]
    pub fn fields(self) -> &'v [String] {
        &self.pattern.fields
    }

    /// Each field position's written sub-pattern, in field order.
    pub fn subpatterns(self) -> impl Iterator<Item = Option<NodeRef<'v>>> + 'v {
        let tree = &self.src.parse.tree;
        self.pattern
            .subs
            .iter()
            .map(move |sub| sub.map(|sub| tree.node(sub)))
    }
}

/// A discarded expression statement and the checked facts about it.
#[derive(Clone, Copy)]
pub struct CallView<'v> {
    discarded: &'v DiscardedExpression,
    src: Src<'v>,
}

impl<'v> CallView<'v> {
    #[must_use]
    pub fn node(self) -> NodeRef<'v> {
        self.src.parse.tree.node(self.discarded.node)
    }

    #[must_use]
    pub const fn selected_declaration(self) -> Option<DeclRef<'v>> {
        match self.discarded.selected {
            Some(def) => Some(DeclRef {
                def,
                prelude_debug: self.discarded.prelude_debug,
                borrow: PhantomData,
            }),
            None => None,
        }
    }

    #[must_use]
    pub const fn is_unbound_disposable(self) -> bool {
        self.discarded.unbound_disposable
    }

    /// For a `std.format.debug` call by a bare name, whether the bare name
    /// `debug` resolves to `std.format.debug` here.
    #[must_use]
    pub const fn standard_debug(self) -> bool {
        self.discarded.standard_debug
    }

    /// For a `std.format.debug` call by a bare name, whether the bare name
    /// `dbg` resolves to `std.format.dbg` here.
    #[must_use]
    pub const fn standard_dbg(self) -> bool {
        self.discarded.standard_dbg
    }
}

/// The completed checker facts of one module. Tooling cannot retain it.
#[derive(Clone, Copy)]
pub struct CheckedView<'v> {
    src: Src<'v>,
    facts: Option<&'v SourceFacts>,
}

impl<'v> CheckedView<'v> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(src: Src<'v>, facts: Option<&'v SourceFacts>) -> Self {
        Self { src, facts }
    }

    fn all<T>(self, family: fn(&SourceFacts) -> &[T]) -> &'v [T] {
        self.facts.map_or(&[], family)
    }

    pub fn comparisons(self) -> impl Iterator<Item = ComparisonView<'v>> + 'v {
        let src = self.src;
        self.all(|f| &f.comparisons)
            .iter()
            .map(move |fact| ComparisonView { fact, src })
    }

    pub fn bindings(self) -> impl Iterator<Item = BindingView<'v>> + 'v {
        let src = self.src;
        self.all(|f| &f.bindings)
            .iter()
            .map(move |binding| BindingView { binding, src })
    }

    /// The statements no execution reaches.
    pub fn unreachable(self) -> impl Iterator<Item = NodeRef<'v>> + 'v {
        let tree = &self.src.parse.tree;
        self.all(|f| &f.unreachable)
            .iter()
            .map(move |&node: &NodeIdx| tree.node(node))
    }

    pub fn parameter_writes(self) -> impl Iterator<Item = ParameterWriteView<'v>> + 'v {
        let src = self.src;
        self.all(|f| &f.parameter_writes)
            .iter()
            .map(move |write| ParameterWriteView { write, src })
    }

    pub fn patterns(self) -> impl Iterator<Item = PatternView<'v>> + 'v {
        let src = self.src;
        self.all(|f| &f.patterns)
            .iter()
            .map(move |pattern| PatternView { pattern, src })
    }

    pub fn discarded(self) -> impl Iterator<Item = CallView<'v>> + 'v {
        let src = self.src;
        self.all(|f| &f.discarded)
            .iter()
            .map(move |discarded| CallView { discarded, src })
    }
}
