//! Which decorators of a function attach a value (annotations, "Function
//! Facts"): every decorator line but the ones the compiler reads itself.

use hd_base::{DefId, PathId, Span, Symbol};
use hd_syntax::{NodeRef, SyntaxKind};

use crate::view::Src;
use crate::{BindingKind, ModuleScope, Names};

/// The decorators that are not facts: each is read by the compiler.
const COMPILER_DECORATORS: &[&str] = &[
    "derive",
    "error",
    "from",
    "source",
    "intrinsic",
    "annotate",
    "host",
    "cfg",
];

/// A decorator line's callee, without its call and type-argument wrappers.
fn callee(d: NodeRef<'_>) -> Option<NodeRef<'_>> {
    let e = d.children().next()?;
    let callee = match e.kind() {
        SyntaxKind::CallExpr => e.children().next()?,
        _ => e,
    };
    if callee.kind() == SyntaxKind::TypeArgsExpr {
        callee.children().next()
    } else {
        Some(callee)
    }
}

/// The name a decorator line writes, with its expression node.
fn line<'t>(src: &Src<'t>, d: NodeRef<'t>) -> Option<(Option<String>, NodeRef<'t>)> {
    let e = d.children().next()?;
    let callee = callee(d)?;
    let name = (callee.kind() == SyntaxKind::NameExpr)
        .then(|| src.first_ident(callee))
        .flatten()
        .map(|t| src.text(t).to_owned());
    Some((name, e))
}

/// The identifier segments of a name or field-path expression.
fn segments(src: &Src<'_>, n: NodeRef<'_>) -> Vec<String> {
    match n.kind() {
        SyntaxKind::NameExpr => vec![src.text(src.first(n)).to_owned()],
        SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
            let mut v = n
                .children()
                .next()
                .map(|b| segments(src, b))
                .unwrap_or_default();
            v.push(src.text(src.last(n)).to_owned());
            v
        }
        _ => vec![],
    }
}

/// Resolves a decorator's callee through the module's scope and its exports.
/// `export` looks up one name in a module; `None` means the path does not
/// resolve. Unlike the ordinary expression checker, this emits no error: an
/// unresolved decorator is checked there as an ordinary expression.
#[must_use]
pub fn decorator_def(
    src: &Src<'_>,
    names: &Names<'_>,
    scope: &ModuleScope,
    export: &dyn Fn(&str, Symbol) -> Option<DefId>,
    d: NodeRef<'_>,
) -> Option<DefId> {
    let segs = segments(src, callee(d)?);
    let first = segs.first()?;
    let binding = scope.lookup(names.syms.intern(first))?;
    match (binding.kind, segs.len()) {
        (BindingKind::Item, 1) => Some(DefId::from_raw(binding.value)),
        (BindingKind::Module, _) if segs.len() >= 2 => {
            let base = scope.modules.get(binding.value as usize)?.clone();
            // A `use` of a module brings in that module alone, not its
            // children. A root module's name is the exception.
            if segs.len() > 2 && base.contains('.') {
                return None;
            }
            let module = if segs.len() == 2 {
                base
            } else {
                format!("{base}.{}", segs[1..segs.len() - 1].join("."))
            };
            export(&module, names.syms.intern(segs.last()?))
        }
        _ => None,
    }
}

/// The expressions of the decorators before `node` that attach a value, in
/// source order. `compiler` marks decorator nodes the compiler consumes.
#[must_use]
pub fn fact_lines<'t>(
    src: &Src<'t>,
    node: NodeRef<'t>,
    mut compiler: impl FnMut(NodeRef<'t>) -> bool,
) -> Vec<NodeRef<'t>> {
    node.children()
        .filter(|c| c.kind() == SyntaxKind::Decorator)
        .filter(|d| !compiler(*d))
        .filter_map(|d| line(src, d))
        .filter(|(name, _)| {
            name.as_ref()
                .is_none_or(|n| !COMPILER_DECORATORS.contains(&n.as_str()))
        })
        .map(|(_, e)| e)
        .collect()
}

/// Whether an expression of this form can never have a list type: a
/// literal, string, tuple, map, data literal, closure, comparison or
/// logical expression. A member line's right side of such a form is an
/// `invalid-member-line` error (`annot.line.right.not-list`) found from
/// syntax alone; any other form is typed.
#[must_use]
pub fn never_a_list(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LiteralExpr
            | SyntaxKind::StringExpr
            | SyntaxKind::TupleExpr
            | SyntaxKind::MapExpr
            | SyntaxKind::DataExpr
            | SyntaxKind::ClosureExpr
            | SyntaxKind::ComparisonExpr
            | SyntaxKind::LogicalAndExpr
            | SyntaxKind::LogicalOrExpr
    )
}

/// Resolved declaration metadata requested by read-only tooling, never cached advice.
#[derive(Clone, Debug, Default)]
pub struct DerivationFacts {
    pub decorators: Vec<BlockDecorator>,
    pub blocks: Vec<DerivationBlock>,
    pub derived: Vec<Derived>,
}

impl DerivationFacts {
    pub fn extend(&mut self, other: Self) {
        self.decorators.extend(other.decorators);
        self.blocks.extend(other.blocks);
        self.derived.extend(other.derived);
    }
}

/// One decorator's occurrence and surrounding derivation block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockDecorator {
    pub span: Span,
    pub value: Option<Span>,
    pub first: Option<Span>,
    pub marker: bool,
    pub valid: bool,
}

/// A written member line: member name, extension mode, and right-side tokens.
pub type MemberLines = Vec<(String, bool, Vec<String>)>;

/// Nominal targets and configured lines of a resolved derivation block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivationBlock {
    pub folder: u32,
    pub target: DefId,
    pub package: PathId,
    pub trait_: DefId,
    pub trait_name: String,
    pub target_name: String,
    pub lines: MemberLines,
    pub span: Span,
}

/// An ordinary derive opt-in, independent of any advice about nearby blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derived {
    pub folder: u32,
    pub target: DefId,
    pub package: PathId,
    pub trait_: DefId,
    pub name: String,
}
