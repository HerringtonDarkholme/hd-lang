//! Which decorators of a function attach a value (annotations, "Function
//! Facts"): every decorator line but the ones the compiler reads itself.

use hd_base::{DefId, FolderId, NodeIdx, PathId, Symbol};
use hd_syntax::{NodeRef, SyntaxKind};

use crate::view::Src;
use crate::{Binding, BindingKind, ImplKind, Item, ItemData, Lookup, ModuleScope, Names};
use hd_types::TyData;

/// The decorators that are not facts: each is read by the compiler.
const COMPILER_DECORATORS: &[&str] = &[
    "derive",
    "error",
    "from",
    "source",
    "intrinsic",
    "host",
    "cfg",
    "auto_closing",
    "track_caller",
];

/// A decorator line's callee, without its call and type-argument wrappers.
fn callee(d: NodeRef<'_>) -> Option<NodeRef<'_>> {
    callee_of(d.children().next()?)
}

/// Expression `e` without its call and type-argument wrappers.
#[must_use]
pub fn callee_of(e: NodeRef<'_>) -> Option<NodeRef<'_>> {
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

/// A decorator line whose callee is a plain name: `@name`, `@name(...)` or
/// `@name::[T](...)`.
pub(crate) struct DecoParts<'t> {
    pub callee: NodeRef<'t>,
    pub args: Option<NodeRef<'t>>,
    /// The callee carries type arguments.
    pub typed: bool,
    /// The line calls nothing: `@name`.
    pub bare: bool,
}

pub(crate) fn decorator_parts(d: NodeRef<'_>) -> Option<DecoParts<'_>> {
    let e = d.children().next()?;
    let bare = e.kind() != SyntaxKind::CallExpr;
    let written = if bare { e } else { e.children().next()? };
    let callee = callee(d)?;
    (callee.kind() == SyntaxKind::NameExpr).then(|| DecoParts {
        callee,
        args: if bare {
            None
        } else {
            Src::child(e, SyntaxKind::ArgumentList)
        },
        typed: written.kind() == SyntaxKind::TypeArgsExpr,
        bare,
    })
}

/// Whether a decorator line is one the compiler consumes as a marker of
/// its target (`@intrinsic`, `@from`, `@source`, `@auto_closing`) rather
/// than a value it attaches.
#[must_use]
pub fn is_marker(src: &Src<'_>, d: NodeRef<'_>) -> bool {
    matches!(
        line(src, d),
        Some((Some(name), _)) if matches!(name.as_str(), "intrinsic" | "from" | "source" | "auto_closing" | "track_caller")
    )
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

/// The names a path expression writes, outermost first: `a`, `a.b` or
/// `m.T.V`. A chain
/// whose base is not a name (`make().helper`) names no path, so it has
/// none: its last link alone would name an unrelated top-level `helper`.
/// A loop down the chain, so a long `a.b.c...` takes no stack per link.
#[must_use]
pub fn name_path<'t>(src: &Src<'t>, n: NodeRef<'_>) -> Vec<&'t str> {
    let mut at = n;
    let mut out = Vec::new();
    loop {
        match at.kind() {
            SyntaxKind::NameExpr => break,
            SyntaxKind::FieldExpr | SyntaxKind::PathExpr => {
                out.push(src.text(src.last(at)));
                let Some(base) = at.children().next() else {
                    return Vec::new();
                };
                at = base;
            }
            _ => return Vec::new(),
        }
    }
    out.push(src.text(src.first(at)));
    out.reverse();
    out
}

/// The declaration the names `path` reach from `scope`: an item it binds,
/// or an export of a module it binds, which `export` looks up. `None` when
/// the path does not resolve; this emits no error.
#[must_use]
pub fn path_def(
    names: &Names<'_>,
    scope: &ModuleScope,
    export: &dyn Fn(&str, Symbol) -> Option<DefId>,
    path: &[&str],
) -> Option<DefId> {
    let (&first, rest) = path.split_first()?;
    let binding = scope.lookup(names.syms.intern(first))?;
    match (binding.kind, rest) {
        (BindingKind::Item, []) => Some(DefId::from_raw(binding.value)),
        (BindingKind::Module, [.., last]) => {
            let base = scope.modules.get(binding.value as usize)?;
            // A `use` of a module brings in that module alone, not its
            // children. A root module's name is the exception.
            let module = match &rest[..rest.len() - 1] {
                [] => base.clone(),
                _ if base.contains('.') => return None,
                inner => format!("{base}.{}", inner.join(".")),
            };
            export(&module, names.syms.intern(last))
        }
        _ => None,
    }
}

/// Resolves a decorator's callee through the module's scope and its
/// exports ([`path_def`]). An unresolved decorator is checked as an
/// ordinary expression, which reports it.
#[must_use]
pub fn decorator_def(
    src: &Src<'_>,
    names: &Names<'_>,
    scope: &ModuleScope,
    export: &dyn Fn(&str, Symbol) -> Option<DefId>,
    d: NodeRef<'_>,
) -> Option<DefId> {
    path_def(names, scope, export, &name_path(src, callee(d)?))
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

/// The narrow resolution read-only tooling asks of a checked module: what
/// one written name binds in the module's scope, which of its declarations
/// are derivation blocks, and the module's own `@derive` implementations.
/// Lowering collects nothing for it; the derivation lints read written
/// names and decorators from syntax and resolve them here.
#[derive(Clone, Copy)]
pub struct DeclView<'v> {
    names: Names<'v>,
    folder: FolderId,
    scope: &'v ModuleScope,
    lookup: &'v Lookup<'v>,
    blocks: &'v [NodeIdx],
}

impl<'v> DeclView<'v> {
    #[must_use]
    pub const fn new(
        names: Names<'v>,
        folder: FolderId,
        scope: &'v ModuleScope,
        lookup: &'v Lookup<'v>,
        blocks: &'v [NodeIdx],
    ) -> Self {
        Self {
            names,
            folder,
            scope,
            lookup,
            blocks,
        }
    }

    /// Whether the declaration at node `n` is a derivation block, as the
    /// module's lowering resolved it (`annot.no-trait.form`). Cost: a scan
    /// of the module's blocks, usually none or a few.
    #[must_use]
    pub fn derivation_block(&self, n: NodeIdx) -> bool {
        self.blocks.contains(&n)
    }

    /// The item a single written name binds in the module's scope, as a
    /// one-segment type name resolves.
    #[must_use]
    pub fn item_named(&self, name: &str) -> Option<(DefId, &'v Item)> {
        let Some(Binding {
            kind: BindingKind::Item,
            value,
        }) = self.scope.lookup(self.names.syms.intern(name))
        else {
            return None;
        };
        let def = DefId::from_raw(value);
        Some((def, self.lookup.item(def)?))
    }

    /// The package node declaring `def`.
    #[must_use]
    pub fn package_of(&self, def: DefId) -> PathId {
        self.names.package_node(def)
    }

    /// An item's declared name, as messages write it.
    #[must_use]
    pub fn display_name(&self, def: DefId) -> &str {
        self.names.display_name(def)
    }

    /// The `@derive` implementations of nominal types that the module's
    /// folder declares, as (target, trait) pairs: the module's own items,
    /// then its folder's interface, which may repeat them. Cost: one pass
    /// over the module's items and the folder's implementations.
    pub fn derived(&self) -> impl Iterator<Item = (DefId, DefId)> + '_ {
        let folder = self.lookup.folder(self.folder);
        self.lookup
            .own
            .iter()
            .chain(folder.into_iter().flat_map(|f| f.items.impls()))
            .filter_map(|item| match &item.data {
                ItemData::Impl {
                    trait_,
                    self_ty,
                    kind: ImplKind::Derived,
                    ..
                } => match self.names.pool.get(*self_ty) {
                    TyData::Adt { def, .. } => Some((def, *trait_)),
                    _ => None,
                },
                _ => None,
            })
    }
}
