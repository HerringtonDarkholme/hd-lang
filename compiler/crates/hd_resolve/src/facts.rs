//! Which decorators of a function attach a value (annotations, "Function
//! Facts"): every decorator line but the ones the compiler reads itself.

use hd_syntax::{NodeRef, SyntaxKind};

use crate::view::Src;

/// The decorators that are not facts: each is read by the compiler.
const COMPILER_DECORATORS: &[&str] = &[
    "derive",
    "error",
    "from",
    "source",
    "intrinsic",
    "annotate",
    "num_suffix",
    "str_prefix",
];

/// The name a decorator line writes, with its expression node.
fn line<'t>(src: &Src<'t>, d: NodeRef<'t>) -> Option<(Option<String>, NodeRef<'t>)> {
    let e = d.children().next()?;
    let callee = match e.kind() {
        SyntaxKind::CallExpr => e.children().next()?,
        _ => e,
    };
    let callee = if callee.kind() == SyntaxKind::TypeArgsExpr {
        callee.children().next()?
    } else {
        callee
    };
    let name = (callee.kind() == SyntaxKind::NameExpr)
        .then(|| src.first_ident(callee))
        .flatten()
        .map(|t| src.text(t).to_owned());
    Some((name, e))
}

/// The expressions of the decorators before `node` that attach a value, in
/// source order.
#[must_use]
pub fn fact_lines<'t>(src: &Src<'t>, node: NodeRef<'t>) -> Vec<NodeRef<'t>> {
    node.children()
        .filter(|c| c.kind() == SyntaxKind::Decorator)
        .filter_map(|d| line(src, d))
        .filter(|(name, _)| {
            name.as_ref()
                .is_none_or(|n| !COMPILER_DECORATORS.contains(&n.as_str()))
        })
        .map(|(_, e)| e)
        .collect()
}
