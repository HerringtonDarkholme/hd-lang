//! Borrowed views exposed only while optional post-check analysis runs.
//!
//! The view owns no compiler identifiers or inferred state. It exposes only
//! the narrow, completed observations used by the first lint rules.

use std::marker::PhantomData;

use hd_base::{DefId, FileId, Span};
use hd_resolve::Src;
use hd_syntax::{NodeRef, SyntaxKind};
use hd_types::Prim;

use crate::comparison::{
    Binding, DiscardedExpression, Mutability, Operand, SourceFacts, Statement, VariantPattern,
};

/// An expression operand in one checked comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceExpr {
    comparison: usize,
    right: bool,
}

impl SourceExpr {
    #[doc(hidden)]
    #[must_use]
    pub const fn comparison(comparison: usize, right: bool) -> Self {
        Self { comparison, right }
    }
}

/// A checked local binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceBinding(usize);

impl SourceBinding {
    #[doc(hidden)]
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
}

/// A checked source statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceStatement(usize);

impl SourceStatement {
    #[doc(hidden)]
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
}

/// A checked enum payload pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePattern(usize);

impl SourcePattern {
    #[doc(hidden)]
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
}

/// A discarded source expression whose selected callee may be queried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceCall(usize);

impl SourceCall {
    #[doc(hidden)]
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        Self(index)
    }
}

/// An opaque body identity, valid only while its checked view is borrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyRef<'v> {
    def: DefId,
    borrow: PhantomData<&'v SourceFacts>,
}

/// An adjusted checked type, deliberately limited to the primitive fact the
/// current lint surface needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeRef<'v> {
    primitive: Option<Prim>,
    borrow: PhantomData<&'v SourceFacts>,
}

impl TypeRef<'_> {
    #[must_use]
    pub const fn primitive(self) -> Option<Prim> {
        self.primitive
    }
}

/// A selected declaration identity. Raw compiler IDs never leave the view.
///
/// ```compile_fail
/// use hd_check::analysis::{CheckedView, DeclRef, SourceCall};
/// fn escape<'v>(view: CheckedView<'v>, call: SourceCall) -> DeclRef<'static> {
///     view.selected_declaration(call).unwrap()
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

/// Source shape and checked type of one comparison operand.
#[derive(Clone, Copy)]
pub struct ExpressionView<'v> {
    operand: &'v Operand,
}

impl ExpressionView<'_> {
    #[must_use]
    pub const fn span(self) -> Span {
        self.operand.span
    }

    #[must_use]
    pub const fn is_unsuffixed_zero(self) -> bool {
        self.operand.unsuffixed_zero
    }
}

/// Final read and permission facts for one binding.
#[derive(Clone, Copy)]
pub struct BindingView<'v> {
    binding: &'v Binding,
}

impl<'v> BindingView<'v> {
    #[must_use]
    pub const fn owner(self) -> BodyRef<'v> {
        BodyRef {
            def: self.binding.body,
            borrow: PhantomData,
        }
    }

    #[must_use]
    pub fn name(self) -> &'v str {
        &self.binding.name
    }

    #[must_use]
    pub const fn span(self) -> Span {
        self.binding.span
    }

    #[must_use]
    pub const fn is_read(self) -> bool {
        self.binding.read
    }

    #[must_use]
    pub fn mutability(self) -> Option<MutabilityView<'v>> {
        self.binding
            .mutability
            .as_ref()
            .map(|fact| MutabilityView { fact })
    }
}

/// Checked properties of a written `let mut` binding.
#[derive(Clone, Copy)]
pub struct MutabilityView<'v> {
    fact: &'v Mutability,
}

impl MutabilityView<'_> {
    #[must_use]
    pub const fn is_primitive(self) -> bool {
        self.fact.primitive
    }

    #[must_use]
    pub const fn annotation_is_writable(self) -> bool {
        self.fact.annotation_writable
    }

    #[must_use]
    pub const fn mutable_address_taken(self) -> bool {
        self.fact.address_taken
    }

    #[must_use]
    pub const fn token(self) -> Option<Span> {
        self.fact.token
    }
}

/// Final reachability and body ownership for one statement.
#[derive(Clone, Copy)]
pub struct StatementView<'v> {
    statement: &'v Statement,
}

impl<'v> StatementView<'v> {
    #[must_use]
    pub const fn owner(self) -> BodyRef<'v> {
        BodyRef {
            def: self.statement.body,
            borrow: PhantomData,
        }
    }

    #[must_use]
    pub const fn span(self) -> Span {
        self.statement.span
    }

    #[must_use]
    pub const fn is_reachable(self) -> bool {
        self.statement.reachable
    }
}

/// Resolved variant layout and source payload bindings.
#[derive(Clone, Copy)]
pub struct PatternView<'v> {
    pattern: &'v VariantPattern,
}

impl<'v> PatternView<'v> {
    #[must_use]
    pub const fn span(self) -> Span {
        self.pattern.span
    }

    #[must_use]
    pub fn variant(self) -> &'v str {
        &self.pattern.variant
    }

    #[must_use]
    pub fn fields(self) -> &'v [String] {
        &self.pattern.fields
    }

    #[must_use]
    pub fn bindings(self) -> &'v [crate::comparison::PayloadBinding] {
        &self.pattern.bindings
    }
}

/// Source shape of an expression statement's direct call.
#[derive(Clone, Copy)]
pub struct CallView<'v> {
    discarded: &'v DiscardedExpression,
}

impl CallView<'_> {
    #[must_use]
    pub const fn span(self) -> Span {
        self.discarded.span
    }

    #[must_use]
    pub const fn name(self) -> Option<Span> {
        match &self.discarded.call {
            Some(call) => call.name,
            None => None,
        }
    }

    #[must_use]
    pub const fn positional(self) -> usize {
        match &self.discarded.call {
            Some(call) => call.positional,
            None => 0,
        }
    }

    #[must_use]
    pub const fn named(self) -> usize {
        match &self.discarded.call {
            Some(call) => call.named,
            None => 0,
        }
    }

    #[must_use]
    pub const fn has_spread(self) -> bool {
        match &self.discarded.call {
            Some(call) => call.spread,
            None => false,
        }
    }

    #[must_use]
    pub const fn after_spread(self) -> usize {
        match &self.discarded.call {
            Some(call) => call.after_spread,
            None => 0,
        }
    }
}

/// The completed compiler state visible during one analysis callback.
#[derive(Clone, Copy)]
pub struct CheckedView<'v> {
    source: SourceView<'v>,
    facts: Option<&'v SourceFacts>,
}

impl<'v> CheckedView<'v> {
    /// Construct the callback view from the checked module's frozen source and
    /// optional completed facts. Tooling cannot retain this view.
    #[doc(hidden)]
    #[must_use]
    pub fn from_source(source: Src<'v>, facts: Option<&'v SourceFacts>) -> Self {
        Self {
            source: SourceView { source },
            facts,
        }
    }

    #[must_use]
    pub const fn source(self) -> SourceView<'v> {
        self.source
    }

    #[must_use]
    pub fn expression(self, source: SourceExpr) -> Option<ExpressionView<'v>> {
        let comparison = self.facts?.comparisons.get(source.comparison)?;
        Some(ExpressionView {
            operand: if source.right {
                &comparison.right
            } else {
                &comparison.left
            },
        })
    }

    #[must_use]
    pub fn adjusted_type(self, source: SourceExpr) -> Option<TypeRef<'v>> {
        Some(TypeRef {
            primitive: self.expression(source)?.operand.primitive,
            borrow: PhantomData,
        })
    }

    #[must_use]
    pub fn binding(self, source: SourceBinding) -> Option<BindingView<'v>> {
        self.facts?
            .bindings
            .get(source.0)
            .map(|binding| BindingView { binding })
    }

    #[must_use]
    pub fn statement(self, source: SourceStatement) -> Option<StatementView<'v>> {
        self.facts?
            .statements
            .get(source.0)
            .map(|statement| StatementView { statement })
    }

    #[must_use]
    pub fn pattern(self, source: SourcePattern) -> Option<PatternView<'v>> {
        self.facts?
            .patterns
            .get(source.0)
            .map(|pattern| PatternView { pattern })
    }

    #[must_use]
    pub fn call(self, source: SourceCall) -> Option<CallView<'v>> {
        self.facts?
            .discarded
            .get(source.0)
            .map(|discarded| CallView { discarded })
    }

    #[must_use]
    pub fn selected_declaration(self, source: SourceCall) -> Option<DeclRef<'v>> {
        let discarded = self.facts?.discarded.get(source.0)?;
        Some(DeclRef {
            def: discarded.selected?,
            prelude_debug: discarded.prelude_debug,
            borrow: PhantomData,
        })
    }
}

/// Syntax and text for the source snapshot whose module was checked.
#[derive(Clone, Copy)]
pub struct SourceView<'v> {
    source: Src<'v>,
}

impl<'v> SourceView<'v> {
    #[must_use]
    pub const fn file(self) -> FileId {
        self.source.file
    }

    #[must_use]
    pub fn root(self) -> NodeRef<'v> {
        self.source.root()
    }

    #[must_use]
    pub fn kind(self, node: NodeRef<'_>) -> SyntaxKind {
        node.kind()
    }

    #[must_use]
    pub fn span(self, node: NodeRef<'_>) -> Span {
        self.source.span(node)
    }

    #[must_use]
    pub const fn text(self) -> &'v str {
        self.source.text
    }

    #[must_use]
    pub fn text_at(self, span: Span) -> Option<&'v str> {
        (span.file == self.source.file)
            .then(|| self.source.text.get(span.lo as usize..span.hi as usize))
            .flatten()
    }
}
