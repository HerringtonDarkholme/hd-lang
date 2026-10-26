//! Optional checked source facts for tools. No diagnostic or policy lives here.

use hd_base::{DefId, LocalId, Span};
use hd_syntax::TokenKind;
use hd_types::Prim;

/// An operand as it was checked, before lowering can erase its source form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operand {
    pub span: Span,
    pub primitive: Option<Prim>,
    /// An unsuffixed integer zero, including its accepted radix spellings.
    pub unsuffixed_zero: bool,
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
    pub mutability: Option<Mutability>,
}

/// Checked source properties of a written `let mut` binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mutability {
    pub primitive: bool,
    pub annotation_writable: bool,
    pub address_taken: bool,
    /// The written keyword and following whitespace, if present.
    pub token: Option<Span>,
}

/// A checked binding directly occupying one enum payload position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadBinding {
    pub span: Span,
    pub position: usize,
    pub name: String,
    pub bare: bool,
}

/// The resolved payload layout and source bindings of an enum pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantPattern {
    pub span: Span,
    pub variant: String,
    pub fields: Vec<String>,
    pub bindings: Vec<PayloadBinding>,
}

/// Ephemeral observations requested by tooling, never cached with checked bodies.
#[derive(Clone, Debug, Default)]
pub struct SourceFacts {
    pub comparisons: Vec<Comparison>,
    pub bindings: Vec<Binding>,
    pub patterns: Vec<VariantPattern>,
}

impl SourceFacts {
    pub fn extend(&mut self, other: Self) {
        self.comparisons.extend(other.comparisons);
        self.bindings.extend(other.bindings);
        self.patterns.extend(other.patterns);
    }
}
