//! Optional checked source facts for tools. No diagnostic or policy lives here.

use hd_base::Span;
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
