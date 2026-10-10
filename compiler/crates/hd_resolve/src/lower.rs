//! Use resolution and header lowering for one folder (resolution-and-
//! interfaces.md §4.9, §4.10 steps 1 to 4): item heads, use declarations
//! in every form, the export worklist with `pub use` chains, module scopes
//! with the prelude, and every header lowered to `hd_types`.
//!
//! The same code serves two modes. While `FolderIface(F)` runs, uses of
//! sibling modules resolve through F's own heads and a worklist. Once F's
//! interface is frozen, `ModulePrep` and `Body` resolve them through its
//! export index instead.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hd_base::{DefId, NotImplemented, PathId, Span, Stage, StageResult, Symbol};
use hd_diag::{Code, DiagBuf};

use hd_intern::PathKind;
use hd_project::{UseRootError, UseRoots};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::{
    Inputs, InternPool, ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList,
};

use crate::iface::{
    Export, Field, FnSig, FolderIface, Generic, HeadKind, ImplKind, Item, ItemData, Names,
    TraitData, Variant, fill_trait_args, show_ty,
};
use crate::known::KnownItems;
use crate::variance::{self, Seen};
use crate::view::Src;

mod decorators;
use crate::{Binding, BindingKind, ModuleScope, Origin};

/// The prelude (spec/lang/10-modules.md#prelude) as fixed uses: origin
/// module and names. `lib/std/prelude.hd` re-exports exactly these; a test
/// checks the two agree. The `std.testing` row is test code only.
pub const PRELUDE: &[(&str, &[&str])] = &[
    (
        "std.core",
        &[
            "never", "bool", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "f32",
            "f64", "char", "string", "void", "List", "Map", "Any", "AnyVal", "AnyRef", "Option",
            "Result", "panic",
        ],
    ),
    ("std.format", &["Display", "Debug", "debug", "dbg"]),
    ("std.cmp", &["Eq", "PartialOrd", "Ord", "Ordering"]),
    ("std.hash", &["Hash", "Hasher"]),
    ("std.iter", &["Iterator", "Iterable"]),
    ("std.console", &["Console", "ConsoleError", "println"]),
    ("std.task", &["Suspend", "Poll", "PollContext", "Waker"]),
];

/// The prelude names of test code (`module.prelude.test-only`): bound only
/// in test code, but no declaration of a module may take their place.
const TEST_PRELUDE: &[(&str, &[&str])] = &[("std.testing", &["it"])];

/// The modules the prelude's names come from: each module's folder gets an
/// edge to theirs (`module.prelude.fixed-uses`).
#[must_use]
pub fn prelude_modules() -> Vec<&'static str> {
    PRELUDE.iter().map(|p| p.0).collect()
}

/// The outside of one folder: which folder holds a module, and the frozen
/// interfaces of other folders.
pub trait World {
    fn module_folder(&self, module: &str) -> Option<u32>;
    /// Whether a `use` may name the module: it exists and is not its own
    /// program (`module.path.main-no-use`).
    fn usable(&self, module: &str) -> bool;
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>>;
}

/// One name of a use: the declaration, its local name, where it is.
#[derive(Clone, Debug)]
pub struct UseName {
    pub name: String,
    pub alias: Option<String>,
    pub span: Span,
}

/// A use declaration with its root (`pkg`, `std`, `dep.NAME`, `self`,
/// `super`) rewritten to absolute module path segments.
#[derive(Clone, Debug)]
pub struct UseDecl {
    pub public: bool,
    pub path: Vec<String>,
    /// `use M.{a, b as c}`; `None` for a single use.
    pub group: Option<Vec<UseName>>,
    pub alias: Option<String>,
    pub span: Span,
    /// Why the path's root names no module path; `path` is then as written.
    pub root_error: Option<UseRootError>,
}

impl UseDecl {
    #[must_use]
    pub fn module(&self) -> String {
        self.path.join(".")
    }
}

/// The top-level use declarations of a file, in order. The uses of its
/// `tests:` block are [`test_use_decls`].
#[must_use]
pub fn use_decls(src: &Src<'_>, roots: &UseRoots) -> Vec<UseDecl> {
    src.root()
        .children()
        .filter(|c| c.kind() == SyntaxKind::UseDecl)
        .map(|item| use_decl(src, item, roots, false))
        .collect()
}

/// The items of a file's `tests:` block (`grammar.tests.item-forms`).
pub fn tests_items<'t>(src: &Src<'t>) -> impl Iterator<Item = NodeRef<'t>> {
    src.root()
        .children()
        .filter(|c| c.kind() == SyntaxKind::TestsBlock)
        .flat_map(hd_syntax::NodeRef::children)
        .filter(|b| b.kind() == SyntaxKind::Block)
        .flat_map(hd_syntax::NodeRef::children)
}

/// The use declarations of a file's `tests:` block, in order. They are
/// test code, and only the block sees their names (`names.tests.inside-only`).
#[must_use]
pub fn test_use_decls(src: &Src<'_>, roots: &UseRoots) -> Vec<UseDecl> {
    tests_items(src)
        .filter(|c| c.kind() == SyntaxKind::UseDecl)
        .map(|item| use_decl(src, item, roots, true))
        .collect()
}

/// One use declaration; `tests` when it is in a `tests:` block.
fn use_decl(src: &Src<'_>, item: NodeRef<'_>, roots: &UseRoots, tests: bool) -> UseDecl {
    let mut path: Vec<String> = Vec::new();
    let mut alias = None;
    let mut public = false;
    let mut after_as = false;
    let mut seen_use = false;
    for t in item.direct_tokens() {
        match src.tkind(t) {
            Some(TokenKind::KwPub) => public = true,
            Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfValue) => {
                let text = src.text(t);
                if !seen_use && text == "use" {
                    seen_use = true;
                } else if text == "as" {
                    after_as = true;
                } else if after_as {
                    alias = Some(text.to_owned());
                } else {
                    path.push(text.to_owned());
                }
            }
            _ => {}
        }
    }
    let group = Src::child(item, SyntaxKind::UseGroup).map(|g| {
        g.children()
            .filter(|c| c.kind() == SyntaxKind::UseItem)
            .filter_map(|ui| {
                let toks: Vec<_> = ui
                    .direct_tokens()
                    .filter(|t| src.tkind(*t) == Some(TokenKind::Ident))
                    .collect();
                let first = *toks.first()?;
                let alias = (toks.len() >= 3 && src.text(toks[1]) == "as")
                    .then(|| src.text(toks[2]).to_owned());
                Some(UseName {
                    name: src.text(first).to_owned(),
                    alias,
                    span: src.span(ui),
                })
            })
            .collect()
    });
    // A `tests:` block's use is test code, which may name a dev
    // dependency (`module.test.dev-dependency.in-tests`).
    let absolute = if tests {
        roots.absolute_in_tests(&path)
    } else {
        roots.absolute(&path)
    };
    let (path, root_error) = match absolute {
        Ok(abs) => (abs, None),
        Err(e) => (path, Some(e)),
    };
    UseDecl {
        public,
        path,
        group,
        alias,
        span: src.span(item),
        root_error,
    }
}

/// One own item's head and its node.
#[derive(Clone, Copy)]
pub struct Head<'t> {
    pub name: Symbol,
    pub def: DefId,
    pub kind: HeadKind,
    pub node: NodeRef<'t>,
    pub public: bool,
    /// Where a block-local declaration is visible; `None` at the top level.
    pub local: Option<LocalSite<'t>>,
    /// For an item of a `tests:` block, or a local declaration inside one,
    /// the block: test code, which only the block sees
    /// (`names.tests.inside-only`).
    pub test: Option<NodeRef<'t>>,
}

/// A block-local declaration's place (checking-and-tir.md "Local items
/// lift to hidden module items"): it is visible from its first byte to
/// the end of its enclosing suite (`names.local-type.name`,
/// `names.local-impl.extent`).
#[derive(Clone, Copy)]
pub struct LocalSite<'t> {
    pub lo: u32,
    pub hi: u32,
    /// The enclosing suite.
    pub suite: hd_base::NodeIdx,
    /// The top-level declaration whose text holds it, which its anchors
    /// count from.
    pub top: NodeRef<'t>,
}

/// A block-local declaration as name lookup sees it, with no syntax.
#[derive(Clone, Copy, Debug)]
pub struct LocalItem {
    pub def: DefId,
    pub name: Symbol,
    pub kind: HeadKind,
    pub lo: u32,
    pub hi: u32,
    pub suite: hd_base::NodeIdx,
}

impl LocalItem {
    /// Whether byte `at` is in this declaration's extent.
    #[must_use]
    pub fn covers(&self, at: u32) -> bool {
        self.lo <= at && at < self.hi
    }
}

/// The block-local declarations among `heads`.
#[must_use]
pub fn local_items(heads: &[Head<'_>]) -> Vec<LocalItem> {
    heads
        .iter()
        .filter_map(|h| {
            let l = h.local?;
            Some(LocalItem {
                def: h.def,
                name: h.name,
                kind: h.kind,
                lo: l.lo,
                hi: l.hi,
                suite: l.suite,
            })
        })
        .collect()
}

/// The local type or trait `name` visible at byte `at`: the innermost
/// declaration whose extent holds `at` (`names.local-type.name`,
/// `names.local-type.not-visible`). Impls have no name to find.
#[must_use]
pub fn local_at(locals: &[LocalItem], name: Symbol, at: u32) -> Option<&LocalItem> {
    locals
        .iter()
        .filter(|l| l.kind != HeadKind::Impl && l.name == name && l.covers(at))
        .max_by_key(|l| l.lo)
}

/// The stable segment of an impl: its header's tokens, `impl` to the colon.
fn impl_segment(src: &Src<'_>, n: NodeRef<'_>) -> String {
    let stop = Src::child(n, SyntaxKind::Block).map(|b| src.first(b).raw());
    let mut parts = Vec::new();
    let mut started = false;
    for t in src.tokens(n) {
        if stop.is_some_and(|s| t.raw() >= s) {
            break;
        }
        if src.tkind(t) == Some(TokenKind::KwImpl) {
            started = true;
        }
        if started {
            parts.push(src.text(t));
        }
    }
    while parts.last() == Some(&":") {
        parts.pop();
    }
    parts.join(" ")
}

/// The head kind of a declaration node and the keyword before its name.
fn decl_kind(src: &Src<'_>, n: NodeRef<'_>) -> Option<(HeadKind, TokenKind)> {
    Some(match n.kind() {
        SyntaxKind::FnDecl => (HeadKind::Fn, TokenKind::KwFn),
        SyntaxKind::DataDecl => (HeadKind::Data, TokenKind::KwData),
        SyntaxKind::EnumDecl => (HeadKind::Enum, TokenKind::KwEnum),
        SyntaxKind::TraitDecl => (HeadKind::Trait, TokenKind::KwTrait),
        SyntaxKind::TypeDecl => {
            let k = if n.direct_token(&src.parse.tokens, TokenKind::Eq).is_some() {
                HeadKind::Alias
            } else {
                HeadKind::Newtype
            };
            (k, TokenKind::KwType)
        }
        SyntaxKind::ImplDecl => (HeadKind::Impl, TokenKind::KwImpl),
        _ => return None,
    })
}

/// Heads of the module's own items: the top-level ones, each followed by
/// the block-local declarations of its bodies (checking-and-tir.md "Local
/// items lift to hidden module items"). With `tests`, as test code is
/// checked, the items of its `tests:` block follow (`names.tests.module-items`).
#[must_use]
pub fn heads<'t>(names: &Names<'_>, src: &Src<'t>, module: &str, tests: bool) -> Vec<Head<'t>> {
    let mut out = Vec::new();
    let mut impl_names: HashMap<String, u32> = HashMap::new();
    let top = src.root().children().map(|n| (n, None));
    let in_tests = src
        .root()
        .children()
        .filter(|c| tests && c.kind() == SyntaxKind::TestsBlock)
        .flat_map(|b| {
            Src::child(b, SyntaxKind::Block)
                .into_iter()
                .flat_map(hd_syntax::NodeRef::children)
                .map(move |n| (n, Some(b)))
        });
    for (n, holder) in top.chain(in_tests) {
        let Some((kind, kw)) = decl_kind(src, n) else {
            continue;
        };
        let h = if kind == HeadKind::Impl {
            let mut seg = impl_segment(src, n);
            let count = impl_names.entry(seg.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                seg = format!("{seg} #{count}");
            }
            let def = DefId::from_raw(
                names
                    .paths
                    .intern(names.module(module), PathKind::Impl, &seg)
                    .raw(),
            );
            Head {
                name: names.syms.intern(&seg),
                def,
                kind: HeadKind::Impl,
                node: n,
                public: true,
                local: None,
                test: holder,
            }
        } else {
            let Some(t) = n.name(&src.parse.tokens).or_else(|| src.name_after(n, kw)) else {
                continue;
            };
            let text = src.text(t);
            Head {
                name: names.syms.intern(text),
                def: names.item(module, text),
                kind,
                node: n,
                public: src.is_pub(n),
                local: None,
                test: holder,
            }
        };
        out.push(h);
        for (owner, f) in body_nodes(names, src, &[h]) {
            local_heads(
                names,
                src,
                (owner, f),
                (holder.unwrap_or(n), holder),
                &mut out,
            );
        }
    }
    out
}

/// The block-local type, trait and impl declarations of the body of
/// function `f`, in source order, each followed by those of its own
/// method bodies. A declaration's path is a child of its scope
/// ([`Names::local_scope`]), numbered by its place among the body's local
/// declarations (Q-R23.1). Local functions are closures, not items.
fn local_heads<'t>(
    names: &Names<'_>,
    src: &Src<'t>,
    (owner, f): (DefId, NodeRef<'t>),
    (top, holder): (NodeRef<'t>, Option<NodeRef<'t>>),
    out: &mut Vec<Head<'t>>,
) {
    let Some(body) = Src::child(f, SyntaxKind::Block) else {
        return;
    };
    let mut found = Vec::new();
    local_decls(body, &mut found);
    for (i, (n, suite)) in found.into_iter().enumerate() {
        let Some((kind, kw)) = decl_kind(src, n) else {
            continue;
        };
        // A local derivation block with no trait is an error of its own
        // (`annot.no-trait.local`), never an impl.
        if kind == HeadKind::Impl && traitless_block(src, n) {
            continue;
        }
        let scope = names.local_scope(owner, i);
        let (name, def) = if kind == HeadKind::Impl {
            let seg = impl_segment(src, n);
            (
                names.syms.intern(&seg),
                names.member(scope, PathKind::Impl, &seg),
            )
        } else {
            let Some(t) = n.name(&src.parse.tokens).or_else(|| src.name_after(n, kw)) else {
                continue;
            };
            let text = src.text(t);
            (
                names.syms.intern(text),
                names.member(scope, PathKind::Item, text),
            )
        };
        let h = Head {
            name,
            def,
            kind,
            node: n,
            public: false,
            local: Some(LocalSite {
                lo: src.span(n).lo,
                hi: src.span(suite).hi,
                suite: suite.index(),
                top,
            }),
            test: holder,
        };
        out.push(h);
        for (m, mf) in body_nodes(names, src, &[h]) {
            local_heads(names, src, (m, mf), (top, holder), out);
        }
    }
}

/// The type parameter names (and `Self`, inside an impl or trait) of the
/// declarations of `top` that enclose the declaration starting at `at`.
fn enclosing_generics(src: &Src<'_>, top: NodeRef<'_>, at: u32) -> Vec<String> {
    let mut out = Vec::new();
    let mut n = top;
    while src.span(n).lo < at {
        if decl_kind(src, n).is_some() {
            if matches!(n.kind(), SyntaxKind::ImplDecl | SyntaxKind::TraitDecl) {
                out.push("Self".to_owned());
            }
            for g in Src::child(n, SyntaxKind::GenericParameterList)
                .iter()
                .flat_map(|gl| gl.children())
            {
                if let Some(t) = g.name(&src.parse.tokens) {
                    out.push(src.text(t).to_owned());
                }
            }
        }
        let Some(c) = n.children().find(|c| {
            let s = src.span(*c);
            s.lo <= at && at < s.hi
        }) else {
            break;
        };
        n = c;
    }
    out
}

/// The local declarations under `n` with their suites, outside nested
/// local declarations (whose methods are bodies of their own). A local
/// function and a closure belong to the enclosing body.
fn local_decls<'t>(n: NodeRef<'t>, out: &mut Vec<(NodeRef<'t>, NodeRef<'t>)>) {
    for c in n.children() {
        let decl = matches!(
            c.kind(),
            SyntaxKind::DataDecl
                | SyntaxKind::EnumDecl
                | SyntaxKind::TraitDecl
                | SyntaxKind::TypeDecl
                | SyntaxKind::ImplDecl
        );
        if decl && n.kind() == SyntaxKind::Block {
            out.push((c, n));
        } else {
            local_decls(c, out);
        }
    }
}

/// The kind of every item a scope can name.
pub type Kinds = HashMap<DefId, HeadKind>;

/// One module of the folder being resolved.
pub struct ModIn<'t> {
    pub path: String,
    /// What the roots of its use paths name.
    pub roots: UseRoots,
    pub src: Src<'t>,
    /// Compiler-supplied items of this module (`seed`).
    pub seeds: Vec<Item>,
    /// Whether its test code is checked (`hd check --tests`, `hd test`):
    /// the items of its `tests:` block are then items of the module, which
    /// only the block sees. A folder interface never holds them.
    pub tests: bool,
}

/// One module's result: all its items (private ones included), its scope
/// and the kinds of the names the scope binds.
pub struct ModOut {
    pub items: Vec<Item>,
    pub scope: ModuleScope,
    /// With [`ModIn::tests`], the scope inside its `tests:` block: the
    /// module's scope with the block's uses and items nested in it
    /// (`names.tests.sees-module`, `names.tests.shadow`).
    pub tests: Option<ModuleScope>,
    /// The diagnostics of its `tests:` block's uses and item headers,
    /// which no folder interface reports.
    pub test_diags: DiagBuf,
    pub kinds: Kinds,
    /// Declaration-relative positions of the items (`anchor`).
    pub anchors: crate::anchor::Anchors,
    /// The syntax nodes of the module's derivation blocks.
    pub blocks: Vec<hd_base::NodeIdx>,
}

pub struct FolderOut {
    pub modules: Vec<ModOut>,
    pub exports: Vec<Export>,
    /// The interface's private-name index: each top-level declaration
    /// without `pub`, as (module path, name), sorted (§4.9).
    pub private_names: Vec<(String, Symbol)>,
}

/// What one resolution needs from the run.
pub struct Cx<'a> {
    pub names: Names<'a>,
    pub folder: u32,
    pub world: &'a dyn World,
}

type Res = Result<(DefId, HeadKind), Code>;

struct Resolver<'a, 'b> {
    cx: &'b Cx<'a>,
    frozen: Option<Arc<FolderIface>>,
    own: HashMap<(String, Symbol), (DefId, HeadKind, bool)>,
    pub_uses: HashMap<(String, Symbol), (String, Symbol)>,
    memo: RefCell<HashMap<(String, Symbol), Res>>,
    visiting: RefCell<HashSet<(String, Symbol)>>,
    /// Associated type names of this folder's traits, from their heads.
    trait_assoc: HashMap<DefId, Vec<Symbol>>,
    /// The declared variance markers of the folder's own declarations.
    variances: HashMap<DefId, Vec<i8>>,
    /// This folder's aliases, each lowered after the aliases it names:
    /// whether each generic parameter is a row parameter, and the body.
    aliases: RefCell<HashMap<DefId, (Vec<bool>, Ty)>>,
    seeds: HashMap<DefId, Item>,
    /// The declared parameters of the traits of the modules being lowered,
    /// lowered before any other header, so a trait reference fills its
    /// defaults whatever the declaration order, and a private trait (which
    /// a frozen interface leaves out) is filled as while it was built.
    own_traits: HashMap<DefId, Vec<Generic>>,
    /// The supertraits of the traits of the modules being lowered, as
    /// trait-value types over each trait's `Self`, lowered before any
    /// other header, so a projection or a binding finds the trait that
    /// declares its associated type (trait.binding.name-reach) whatever
    /// the declaration order.
    own_supers: HashMap<DefId, Vec<Ty>>,
}

impl Resolver<'_, '_> {
    fn module_exists(&self, module: &str) -> bool {
        self.cx.world.usable(module)
    }

    fn export(&self, module: &str, name: Symbol) -> Res {
        let Some(f) = self
            .cx
            .world
            .module_folder(module)
            .filter(|_| self.cx.world.usable(module))
        else {
            return Err(Code::UnknownModule);
        };
        let frozen = |i: &FolderIface| {
            i.export(module, name)
                .map(|e| (e.def, e.kind))
                .ok_or(if i.is_private(module, name) {
                    Code::PrivateImport
                } else {
                    Code::UnknownImport
                })
        };
        if f != self.cx.folder {
            let i = self.cx.world.iface(f).ok_or(Code::UnknownModule)?;
            return frozen(&i);
        }
        if let Some(i) = &self.frozen {
            return frozen(i);
        }
        let key = (module.to_owned(), name);
        if let Some(r) = self.memo.borrow().get(&key) {
            return *r;
        }
        let r = if let Some(&(d, k, public)) = self.own.get(&key) {
            if public {
                Ok((d, k))
            } else {
                Err(Code::PrivateImport)
            }
        } else if let Some((m, n)) = self.pub_uses.get(&key) {
            if !self.visiting.borrow_mut().insert(key.clone()) {
                return Err(Code::ReExportLoop);
            }
            let r = self.export(m, *n);
            self.visiting.borrow_mut().remove(&key);
            r
        } else {
            Err(Code::UnknownImport)
        };
        self.memo.borrow_mut().insert(key, r);
        r
    }

    /// An item of another folder or a seed, for kinds and alias bodies.
    fn item(&self, d: DefId) -> Option<Item> {
        if let Some(i) = self.seeds.get(&d) {
            return Some(i.clone());
        }
        let module = self.cx.names.module_of(d);
        let f = self.cx.world.module_folder(&module)?;
        let iface = if f == self.cx.folder {
            self.frozen.clone()?
        } else {
            self.cx.world.iface(f)?
        };
        iface.item(d).cloned()
    }

    /// A trait reference's arguments with the omitted trailing ones filled
    /// from the trait's defaults (`iface::fill_trait_args`): `impl Add for
    /// Money` implements `Add[Money]` (`expr.op.trait.rhs-self`), and
    /// `T < Add` is `T < Add[T]`.
    fn fill_trait_args(&self, tr: DefId, args: TyList, self_ty: Option<Ty>) -> TyList {
        let pool = self.cx.names.pool;
        if let Some(g) = self.own_traits.get(&tr) {
            return fill_trait_args(pool, tr, g, args, self_ty);
        }
        match self.item(tr) {
            Some(item) => fill_trait_args(pool, tr, &item.generics, args, self_ty),
            None => args,
        }
    }

    /// A trait's direct supertraits as trait-value types over its `Self`.
    fn supers_of(&self, tr: DefId) -> Vec<Ty> {
        if let Some(v) = self.own_supers.get(&tr) {
            return v.clone();
        }
        match self.item(tr).map(|i| i.data) {
            Some(ItemData::Trait(t)) => t.supers,
            _ => Vec::new(),
        }
    }

    fn trait_has_assoc(&self, tr: DefId, name: Symbol) -> bool {
        if let Some(v) = self.trait_assoc.get(&tr) {
            return v.contains(&name);
        }
        match self.item(tr).map(|i| i.data) {
            Some(ItemData::Trait(t)) => t.assoc.iter().any(|a| a.0 == name),
            _ => false,
        }
    }
}

impl crate::assoc::TraitView for Resolver<'_, '_> {
    fn supers(&self, trait_: DefId) -> Vec<Ty> {
        self.supers_of(trait_)
    }
    fn own_assoc(&self, trait_: DefId, name: &str) -> Option<DefId> {
        let names = &self.cx.names;
        self.trait_has_assoc(trait_, names.syms.intern(name))
            .then(|| names.member(trait_, PathKind::Member, name))
    }
    fn own_assocs(&self, trait_: DefId) -> Vec<DefId> {
        let names = &self.cx.names;
        let syms = match self.trait_assoc.get(&trait_) {
            Some(v) => v.clone(),
            None => match self.item(trait_).map(|i| i.data) {
                Some(ItemData::Trait(t)) => t.assoc.iter().map(|a| a.0).collect(),
                _ => Vec::new(),
            },
        };
        syms.into_iter()
            .map(|s| names.member(trait_, PathKind::Member, names.text(s)))
            .collect()
    }
}

fn bind_item(scope: &mut ModuleScope, name: Symbol, def: DefId, origin: Origin, row: u32) {
    scope.bind(
        name,
        Binding {
            kind: BindingKind::Item,
            value: def.raw(),
        },
        origin,
        row,
    );
}

/// A name a failed `use` would have bound: later references stay quiet.
fn bind_poison(scope: &mut ModuleScope, name: Symbol, origin: Origin, row: u32) {
    scope.bind(
        name,
        Binding {
            kind: BindingKind::Poison,
            value: 0,
        },
        origin,
        row,
    );
}

/// Poisons every name a whole `use` would have bound.
fn poison_use(scope: &mut ModuleScope, names: &Names<'_>, u: &UseDecl, row: u32) {
    if let Some(group) = &u.group {
        for g in group {
            let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
            bind_poison(scope, local, use_origin(u), row);
        }
    } else if let Some(last) = u.path.last() {
        let local = names
            .syms
            .intern(u.alias.as_deref().unwrap_or(last.as_str()));
        bind_poison(scope, local, use_origin(u), row);
    }
}

fn use_origin(u: &UseDecl) -> Origin {
    if u.public {
        Origin::PubUse
    } else {
        Origin::Use
    }
}

fn scope_of(
    r: &Resolver<'_, '_>,
    m: &ModIn<'_>,
    heads: &[Head<'_>],
    uses: &[UseDecl],
    diags: &mut DiagBuf,
) -> (ModuleScope, Kinds) {
    let mut scope = ModuleScope::default();
    let mut kinds = Kinds::new();
    for h in heads {
        // A local declaration's name is in no module scope
        // (`names.local-type.static`); lowering finds it by position. Only
        // its `tests:` block sees a test item (`names.tests.inside-only`).
        if h.kind != HeadKind::Impl && h.local.is_none() && h.test.is_none() {
            bind_item(&mut scope, h.name, h.def, Origin::Own, u32::MAX);
        }
        kinds.insert(h.def, h.kind);
    }
    for s in &m.seeds {
        if let Some(k) = s.kind()
            && scope.lookup(s.name).is_none()
        {
            bind_item(&mut scope, s.name, s.def, Origin::Own, u32::MAX);
            kinds.insert(s.def, k);
        }
    }
    bind_uses(r, &mut scope, &mut kinds, uses, diags);
    prelude(r, m, heads, &mut scope, &mut kinds, diags);
    (scope, kinds)
}

/// The scope inside a module's `tests:` block (spec 03 "Tests Blocks"):
/// the module's scope with the block's items and uses nested in it, so
/// the block sees every module name (`names.tests.sees-module`) and its
/// own names win inside it (`names.tests.shadow`).
fn test_scope_of(
    r: &Resolver<'_, '_>,
    src: &Src<'_>,
    heads: &[Head<'_>],
    (scope, kinds): (&ModuleScope, &mut Kinds),
    uses: &[UseDecl],
    diags: &mut DiagBuf,
) -> ModuleScope {
    let mut inner = ModuleScope::default();
    for h in heads
        .iter()
        .filter(|h| h.test.is_some() && h.local.is_none() && h.kind != HeadKind::Impl)
    {
        let span = src.span(h.node);
        let name = r.cx.names.text(h.name);
        if h.public {
            let msg = format!("`{name}` is an item of a `tests:` block, which cannot be `pub`");
            diags.error(Code::PublicTestItem, span, &msg);
        }
        // A test item is a module item, so it repeats no module name
        // declared outside the block (`names.tests.unique`).
        let outside = scope
            .index
            .get(&h.name)
            .map(|&row| scope.origin[row as usize]);
        if matches!(outside, Some(Origin::Own | Origin::Use | Origin::PubUse)) {
            let msg = format!("the module already declares `{name}` outside its `tests:` block");
            diags.error(Code::DuplicateModuleName, span, &msg);
            continue;
        }
        bind_item(&mut inner, h.name, h.def, Origin::Own, u32::MAX);
    }
    for u in uses.iter().filter(|u| u.public) {
        let msg = "a use of a `tests:` block cannot be `pub`";
        diags.error(Code::PublicTestItem, u.span, msg);
    }
    bind_uses(r, &mut inner, kinds, uses, diags);
    scope.overlay(&inner)
}

/// Binds the names of `uses` in `scope`, and reports each use that names
/// nothing, whose names stay quiet (poisoned).
fn bind_uses(
    r: &Resolver<'_, '_>,
    scope: &mut ModuleScope,
    kinds: &mut Kinds,
    uses: &[UseDecl],
    diags: &mut DiagBuf,
) {
    let names = &r.cx.names;
    for (row, u) in uses.iter().enumerate() {
        let row = u32::try_from(row).unwrap_or(u32::MAX);
        let module = u.module();
        if let Some(e) = u.root_error {
            let dep = u.path.get(1).map_or("", String::as_str);
            let (code, msg) = match e {
                UseRootError::AboveRoot => (
                    Code::UnknownModule,
                    "the path goes above the root".to_owned(),
                ),
                UseRootError::UnknownDependency => (
                    Code::UnknownModule,
                    format!("`dep.{dep}` names no dependency of this package"),
                ),
                UseRootError::UnknownRoot => (
                    Code::UnknownModule,
                    format!(
                        "`{}` is no use root; a path starts with `pkg`, `std`, `dep`, `self` or `super`",
                        u.path.first().map_or("", String::as_str)
                    ),
                ),
                // `module.test.non-test-use.dev-dependency`,
                // `cli.dep.dev-use.remove-first`.
                UseRootError::TestOnly => (
                    Code::TestOnlyUse,
                    format!(
                        "`dep.{dep}` is a dev dependency, which only test code may use; to use it here, run `hd remove {dep}`, then `hd add {dep} PATH@VERSION` to put it in [dependencies]"
                    ),
                ),
                // `module.test.cyclic-dev-unit`.
                UseRootError::CyclicTest => (
                    Code::CyclicTestDependency,
                    format!(
                        "`dep.{dep}` depends on this package, so a unit test may not use it; use it from an integration test under tests/"
                    ),
                ),
            };
            diags.error(code, u.span, &msg);
            poison_use(scope, names, u, row);
            continue;
        }
        if let Some(group) = &u.group {
            {
                if !r.module_exists(&module) {
                    let msg = format!("no module named `{module}`");
                    diags.error(Code::UnknownModule, u.span, &msg);
                    poison_use(scope, names, u, row);
                    continue;
                }
                for g in group {
                    let sym = names.syms.intern(&g.name);
                    match r.export(&module, sym) {
                        Ok((d, k)) => {
                            let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
                            let origin = if u.public {
                                Origin::PubUse
                            } else {
                                Origin::Use
                            };
                            bind_item(scope, local, d, origin, row);
                            kinds.insert(d, k);
                        }
                        Err(code) => {
                            let msg = format!("{} `{}` in `{module}`", code.as_str(), g.name);
                            diags.error(code, g.span, &msg);
                            let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
                            bind_poison(scope, local, use_origin(u), row);
                        }
                    }
                }
            }
        } else {
            {
                let Some((last, parent)) = u.path.split_last() else {
                    continue;
                };
                let parent = parent.join(".");
                let local = names
                    .syms
                    .intern(u.alias.as_deref().unwrap_or(last.as_str()));
                let as_decl = (!parent.is_empty())
                    .then(|| r.export(&parent, names.syms.intern(last)))
                    .and_then(Result::ok);
                if r.module_exists(&module) {
                    if as_decl.is_some() {
                        let msg = format!("`{module}` is ambiguous");
                        diags.error(Code::AmbiguousImport, u.span, &msg);
                        bind_poison(scope, local, use_origin(u), row);
                        continue;
                    }
                    let idx = u32::try_from(scope.modules.len()).unwrap_or(u32::MAX);
                    scope.modules.push(module.clone());
                    scope.bind(
                        local,
                        Binding {
                            kind: BindingKind::Module,
                            value: idx,
                        },
                        Origin::Use,
                        row,
                    );
                } else if let Some((d, k)) = as_decl {
                    bind_item(scope, local, d, Origin::Use, row);
                    kinds.insert(d, k);
                } else if !r.module_exists(&parent) {
                    let msg = format!("no module named `{parent}`");
                    diags.error(Code::UnknownModule, u.span, &msg);
                    bind_poison(scope, local, use_origin(u), row);
                } else {
                    let code = r
                        .export(&parent, names.syms.intern(last))
                        .err()
                        .unwrap_or(Code::UnknownImport);
                    let msg = format!("{} `{last}` in `{parent}`", code.as_str());
                    diags.error(code, u.span, &msg);
                    bind_poison(scope, local, use_origin(u), row);
                }
            }
        }
    }
}

/// Binds each prelude name that `scope` leaves free, and reports each
/// own declaration that shadows one (`names.prelude.shadow`).
fn prelude(
    r: &Resolver<'_, '_>,
    m: &ModIn<'_>,
    heads: &[Head<'_>],
    scope: &mut ModuleScope,
    kinds: &mut Kinds,
    diags: &mut DiagBuf,
) {
    let names = &r.cx.names;
    for (module, list) in PRELUDE {
        if !r.module_exists(module) {
            continue;
        }
        for name in *list {
            let sym = names.syms.intern(name);
            let Ok((d, k)) = r.export(module, sym) else {
                continue;
            };
            match scope.lookup(sym) {
                None => {
                    bind_item(scope, sym, d, Origin::Prelude, u32::MAX);
                    kinds.insert(d, k);
                }
                Some(b) if b.kind == BindingKind::Item && b.value == d.raw() => {}
                Some(_) => {
                    let span = heads
                        .iter()
                        .find(|h| h.name == sym && h.local.is_none() && h.test.is_none())
                        .map_or(m.src.span(m.src.root()), |h| m.src.span(h.node));
                    let msg = format!("`{name}` shadows a prelude name");
                    diags.error(Code::PreludeNameShadow, span, &msg);
                }
            }
            // A local type declaration is subject to the same rule
            // (`names.local-type.prelude`).
            for h in heads
                .iter()
                .filter(|h| h.name == sym && h.local.is_some() && h.kind != HeadKind::Impl)
            {
                let msg = format!("`{name}` shadows a prelude name");
                diags.error(Code::PreludeNameShadow, m.src.span(h.node), &msg);
            }
        }
    }
    // The test names are not bound here, yet a declaration of the module
    // cannot take one (`module.prelude.no-shadow`). The module that
    // declares them is the prelude's source.
    for (module, list) in TEST_PRELUDE {
        if m.path == *module {
            continue;
        }
        for name in *list {
            let sym = names.syms.intern(name);
            for h in heads
                .iter()
                .filter(|h| h.name == sym && h.local.is_none() && h.test.is_none())
            {
                let msg = format!("`{name}` shadows a prelude name");
                diags.error(Code::PreludeNameShadow, m.src.span(h.node), &msg);
            }
        }
    }
}

/// An associated type declared in a trait or bound in an impl body.
type AssocDecl = (Symbol, DefId, Option<Ty>);

/// A bound's trait and its arguments, defaults filled.
type BoundRef = (DefId, TyList);

/// The type parameters in scope while lowering one header.
#[derive(Clone, Default)]
struct Gen {
    /// Type parameters in scope: name, type, and each bound's trait with
    /// its (default-filled) arguments.
    tys: Vec<(Symbol, Ty, Vec<BoundRef>)>,
    rows: Vec<(Symbol, RowParamRef)>,
    self_ty: Option<Ty>,
    self_trait: Option<DefId>,
    /// Type parameters written with a bound list (types.generic.no-mut-t).
    bounded: Vec<Ty>,
    /// Type parameters of the list being lowered whose bounds are not
    /// lowered yet: whether one is bounded by `Tuple` is not known.
    pending: Vec<Ty>,
}

struct Lower<'a, 'r, 'x> {
    r: &'a Resolver<'r, 'x>,
    names: Names<'r>,
    src: Src<'a>,
    module: &'a str,
    scope: &'a ModuleScope,
    kinds: &'a Kinds,
    diags: &'a mut DiagBuf,
    unsupported: Option<String>,
    /// The module's block-local declarations.
    locals: &'a [LocalItem],
    /// The byte where the header being lowered is declared: the local
    /// declarations whose extent holds it are in scope.
    at: u32,
    /// For a local declaration: the type parameters (and `Self`) of the
    /// declarations around it.
    outer: Vec<String>,
}

impl Lower<'_, '_, '_> {
    fn gap(&mut self, what: impl Into<String>) {
        self.unsupported.get_or_insert_with(|| what.into());
    }

    fn sym(&self, text: &str) -> Symbol {
        self.names.syms.intern(text)
    }

    fn segments(&self, n: NodeRef<'_>) -> Vec<(String, hd_base::TokenIdx)> {
        n.direct_tokens()
            .filter(|t| {
                matches!(
                    self.src.tkind(*t),
                    Some(TokenKind::Ident | TokenKind::RawIdent | TokenKind::KwSelfType)
                )
            })
            .map(|t| (self.src.text(t).to_owned(), t))
            .collect()
    }

    /// Whether a name was bound by a failed `use` (an earlier error).
    fn is_poisoned(&self, name: &str) -> bool {
        let sym = self.sym(name);
        local_at(self.locals, sym, self.at).is_none()
            && self
                .scope
                .lookup(sym)
                .is_some_and(|b| b.kind == BindingKind::Poison)
    }

    /// The declaration a (possibly qualified) name reaches.
    fn resolve_path(
        &mut self,
        segs: &[(String, hd_base::TokenIdx)],
        span: Span,
    ) -> Option<(DefId, HeadKind)> {
        let (first, _) = segs.first()?;
        if self.is_poisoned(first) {
            return None;
        }
        // A local declaration in scope here shadows the module's names.
        if segs.len() == 1
            && let Some(l) = local_at(self.locals, self.sym(first), self.at)
        {
            return Some((l.def, l.kind));
        }
        let b = self.scope.lookup(self.sym(first));
        if segs.len() == 1 {
            return match b {
                Some(Binding {
                    kind: BindingKind::Item,
                    value,
                }) => {
                    let d = DefId::from_raw(value);
                    let k = self
                        .kinds
                        .get(&d)
                        .copied()
                        .or_else(|| self.r.item(d).and_then(|i| i.kind()))?;
                    Some((d, k))
                }
                _ => None,
            };
        }
        let Some(Binding {
            kind: BindingKind::Module,
            value,
        }) = b
        else {
            let msg = format!("`{first}` is not defined");
            self.diags.error(Code::UnknownName, span, &msg);
            return None;
        };
        let mut module = self.scope.modules.get(value as usize)?.clone();
        for (s, _) in &segs[1..segs.len() - 1] {
            module.push('.');
            module.push_str(s);
        }
        let (last, _) = segs.last()?;
        match self.r.export(&module, self.sym(last)) {
            Ok(x) => Some(x),
            Err(code) => {
                let msg = format!("{} `{last}` in `{module}`", code.as_str());
                self.diags.error(code, span, &msg);
                None
            }
        }
    }

    fn type_args(
        &mut self,
        n: NodeRef<'_>,
        gn: &Gen,
    ) -> (Vec<Ty>, Vec<(Symbol, Ty)>, Option<RowId>) {
        let mut args = Vec::new();
        let mut bindings = Vec::new();
        let mut row = None;
        if let Some(al) = Src::child(n, SyntaxKind::TypeArgumentList) {
            for a in al.children() {
                match a.kind() {
                    SyntaxKind::AssociatedTypeBinding => {
                        let Some(t) = a.name(&self.src.parse.tokens) else {
                            continue;
                        };
                        let name = self.sym(self.src.text(t));
                        let ty = self.ty(Src::type_child(a), gn);
                        bindings.push((name, ty));
                    }
                    // A row argument keeps its position: an alias's row
                    // parameter takes it (`req.row.alias.generic.use`).
                    SyntaxKind::RequirementRow => {
                        let r = self.row(Some(a), gn);
                        row = Some(r);
                        args.push(self.names.pool.intern_ty(&TyData::Row(r)));
                    }
                    k if k.is_type() => args.push(self.ty(Some(a), gn)),
                    _ => {}
                }
            }
        }
        (args, bindings, row)
    }

    /// A trait reference (a bound, a supertrait, an impl's trait, a `dyn`
    /// or a row key) as a trait-value type, its omitted trailing arguments
    /// filled from their defaults with `self_ty` as `Self`
    /// (`types.generic.default.written`). `self_ty` is `None` for a `dyn`
    /// type and a row key.
    fn trait_value(&mut self, n: NodeRef<'_>, gn: &Gen, self_ty: Option<Ty>) -> Option<Ty> {
        let segs = self.segments(n);
        let span = self.src.span(n);
        if segs.first().is_some_and(|s| self.is_poisoned(&s.0)) {
            return None;
        }
        let found = self.resolve_path(&segs, span);
        self.trait_value_from(n, gn, self_ty, found)
    }

    /// [`Self::trait_value`] once its name has resolved to `found`.
    fn trait_value_from(
        &mut self,
        n: NodeRef<'_>,
        gn: &Gen,
        self_ty: Option<Ty>,
        found: Option<(DefId, HeadKind)>,
    ) -> Option<Ty> {
        let segs = self.segments(n);
        let span = self.src.span(n);
        let Some((def, kind)) = found else {
            let name = segs.last().map_or("", |s| s.0.as_str()).to_owned();
            if segs.len() == 1 {
                let msg = format!("no trait named `{name}`");
                self.diags.error(Code::UnknownTrait, span, &msg);
            }
            return None;
        };
        if kind != HeadKind::Trait {
            let msg = format!(
                "no trait named `{}`: not a trait",
                segs.last().map_or("", |s| s.0.as_str())
            );
            self.diags.error(Code::UnknownTrait, span, &msg);
            return None;
        }
        let (mut args, written, _) = self.type_args(n, gn);
        args.retain(|a| !matches!(self.names.pool.get(*a), TyData::Row(_)));
        let args = self
            .r
            .fill_trait_args(def, self.names.pool.list(&args), self_ty);
        // A binding names an associated type the trait declares or
        // reaches through its supertraits (trait.binding.name-reach).
        let written = written
            .into_iter()
            .map(|(name, t)| (self.names.text(name).to_owned(), t))
            .collect();
        let (bindings, bad) = crate::assoc::resolve_bindings(
            self.names.pool.types(),
            self.r,
            (def, args, self_ty),
            written,
        );
        for b in bad {
            let (code, msg) = b.diagnostic(self.names.display_name(def));
            self.diags.error(code, span, &msg);
        }
        Some(self.names.pool.intern_ty(&TyData::TraitValue {
            def,
            args,
            bindings,
        }))
    }

    /// Two bounds of one list fix one associated type
    /// (`trait.binding.once.error`, `trait.binding.super.conflict`).
    fn bound_clashes(&mut self, self_ty: Ty, bounds: &[Ty], span: Span) {
        let pool = self.names.pool.types();
        for c in crate::assoc::bound_clashes(pool, self.r, self_ty, bounds) {
            let msg = crate::assoc::clash_message(&self.names, &c);
            self.diags
                .error(Code::DuplicateAssociatedBinding, span, &msg);
        }
    }

    /// A trait value type binds every associated type of its trait and
    /// supertraits (trait.dyn.binding.complete, req.key.binding.complete).
    fn require_complete(&mut self, t: Ty, span: Span) {
        if let Some(msg) = crate::assoc::incomplete_message(&self.names, self.r, t) {
            self.diags.error(Code::TraitNotDynamicallySafe, span, &msg);
        }
    }

    fn trait_of(&self, t: Ty) -> Option<DefId> {
        match self.names.pool.get(t) {
            TyData::TraitValue { def, .. } => Some(def),
            _ => None,
        }
    }

    fn row(&mut self, n: Option<NodeRef<'_>>, gn: &Gen) -> RowId {
        let Some(n) = n else { return RowId::EMPTY };
        let mut data = RowData::default();
        for c in n.children() {
            if c.kind() != SyntaxKind::NamedType {
                continue;
            }
            let segs = self.segments(c);
            if segs.len() == 1
                && let Some((_, p)) = gn
                    .rows
                    .iter()
                    .find(|(s, _)| self.names.text(*s) == segs[0].0)
            {
                data.params.push(*p);
                continue;
            }
            if segs.first().is_some_and(|s| self.is_poisoned(&s.0)) {
                continue;
            }
            let found = self.resolve_path(&segs, self.src.span(c));
            if let Some((def, HeadKind::Alias)) = found {
                self.alias_in_row(c, def, gn, &mut data);
                continue;
            }
            if let Some(t) = self.trait_value_from(c, gn, None, found) {
                self.require_complete(t, self.src.span(c));
                data.keys.push(t);
            }
        }
        self.names.pool.row(&data)
    }

    /// An alias written in a row (`req.row.alias.expand`): a row alias
    /// stands for its keys, which the row flattens in; an alias of one
    /// trait value is that key (`req.row.alias.one-key`).
    fn alias_in_row(&mut self, c: NodeRef<'_>, def: DefId, gn: &Gen, data: &mut RowData) {
        let (args, _, _) = self.type_args(c, gn);
        let span = self.src.span(c);
        let t = self.expand_alias(def, &args, span);
        let pool = self.names.pool;
        let name = self.names.display_name(def).to_owned();
        match pool.get(t) {
            TyData::Row(_) | TyData::TraitValue { .. } => data.keys.push(t),
            TyData::Poison => {}
            TyData::Mut(inner) if matches!(pool.get(inner), TyData::TraitValue { .. }) => {
                let msg = format!(
                    "`{name}` is `{}`, but a key has no `mut`: its trait decides the provider's access",
                    show_ty(&self.names, t)
                );
                self.diags.error(Code::MutAliasKey, span, &msg);
            }
            _ => {
                let msg = format!("no trait named `{name}`: not a trait");
                self.diags.error(Code::UnknownTrait, span, &msg);
            }
        }
    }

    /// An alias's body, and whether each of its generic parameters is a
    /// row parameter.
    fn alias_body(&self, def: DefId) -> Option<(Vec<bool>, Ty)> {
        if let Some(a) = self.r.aliases.borrow().get(&def) {
            return Some(a.clone());
        }
        match self.r.item(def) {
            Some(Item {
                data: ItemData::Alias(t),
                generics,
                ..
            }) => Some((generics.iter().map(|g| g.row).collect(), t)),
            _ => None,
        }
    }

    /// An alias applied to `args`: its body with them substituted, after
    /// each argument's kind is checked against its parameter's
    /// (`req.row.alias.generic.kind`, `req.row.slot.dollar`).
    fn expand_alias(&mut self, def: DefId, args: &[Ty], span: Span) -> Ty {
        let pool = self.names.pool;
        let Some((rows, body)) = self.alias_body(def) else {
            self.gap("an alias used before its declaration in the same folder");
            return Ty::POISON;
        };
        for (a, row) in args.iter().zip(&rows) {
            let is_row = matches!(pool.get(*a), TyData::Row(_));
            if is_row != *row {
                let name = self.names.display_name(def);
                let msg = if *row {
                    format!("`{name}` takes a row here: write it after `$`, as in `{name}[$ Key]`")
                } else {
                    format!("`{name}` takes a type here, not a row")
                };
                self.diags.error(Code::GenericKindMismatch, span, &msg);
                return Ty::POISON;
            }
        }
        pool.subst(body, &|p: ParamRef| {
            (p.owner == def)
                .then(|| args.get(p.index as usize).copied())
                .flatten()
        })
    }

    fn ctor(
        &mut self,
        def: DefId,
        (kind, gn): (HeadKind, &Gen),
        args: &[Ty],
        row: Option<RowId>,
        span: Span,
    ) -> Ty {
        let pool = self.names.pool;
        if self
            .names
            .declared_in(def, self.names.known.function_module)
        {
            let name = self.names.paths.segment(PathId::from_raw(def.raw()));
            if name == "Fn" || name == "SuspendFn" {
                let inputs = args.first().copied().unwrap_or(Ty::POISON);
                if !self.inputs_tuple(inputs, gn) {
                    let msg = format!(
                        "the inputs of `{name}` are a tuple type or a type parameter bounded by `Tuple`, as in `{name}[(A, B), O, $()]`"
                    );
                    self.diags.error(Code::GenericKindMismatch, span, &msg);
                    return Ty::POISON;
                }
                // Interning gives a tuple its parameter-per-element form,
                // a rest element the `List` it is (`fn.type.vararg-rest`).
                return pool.intern_ty(&TyData::Fn {
                    params: pool.list(&[inputs]),
                    result: args.get(1).copied().unwrap_or(Ty::VOID),
                    row: row.unwrap_or(RowId::EMPTY),
                    suspends: name == "SuspendFn",
                    inputs: Inputs::Tuple,
                });
            }
        }
        match kind {
            HeadKind::Alias => {
                let t = self.expand_alias(def, args, span);
                // A row alias is row-kinded (`req.row.alias.type-or-key`).
                if matches!(pool.get(t), TyData::Row(_)) {
                    let name = self.names.display_name(def);
                    let msg = format!(
                        "`{name}` is a row alias, not a type: write it in a row after `$`, as in `$ {name}`"
                    );
                    self.diags.error(Code::GenericKindMismatch, span, &msg);
                    return Ty::POISON;
                }
                t
            }
            HeadKind::Data | HeadKind::Enum | HeadKind::Newtype => {
                let args: Vec<Ty> = args
                    .iter()
                    .copied()
                    .filter(|a| !matches!(pool.get(*a), TyData::Row(_)))
                    .collect();
                pool.intern_ty(&TyData::Adt {
                    def,
                    args: pool.list(&args),
                })
            }
            HeadKind::Trait => {
                let msg = format!("`{}` is a trait, not a type", self.names.display_name(def));
                self.diags.error(Code::TraitUsedAsType, span, &msg);
                Ty::POISON
            }
            HeadKind::Fn | HeadKind::Impl => {
                let msg = format!(
                    "no type named `{}`: not a type",
                    self.names.display_name(def)
                );
                self.diags.error(Code::UnknownType, span, &msg);
                Ty::POISON
            }
        }
    }

    fn named(&mut self, n: NodeRef<'_>, gn: &Gen) -> Ty {
        let segs = self.segments(n);
        let span = self.src.span(n);
        let Some((first, ft)) = segs.first().cloned() else {
            return Ty::POISON;
        };
        if segs.len() == 1 {
            if self.src.tkind(ft) == Some(TokenKind::KwSelfType) {
                return gn.self_ty.unwrap_or(Ty::POISON);
            }
            if let Some((_, t, _)) = gn
                .tys
                .iter()
                .rev()
                .find(|(s, _, _)| self.names.text(*s) == first)
            {
                return *t;
            }
            if first == "never" {
                return Ty::NEVER;
            }
            if first == "void" {
                return Ty::VOID;
            }
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == first) {
                return Ty::prim(*p);
            }
        }
        if self.is_poisoned(&first) {
            return Ty::POISON;
        }
        let Some((def, kind)) = self.resolve_path(&segs, span) else {
            // Outer type parameters would become the local declaration's
            // own (`names.local-type.refs`); not carried yet.
            if segs.len() == 1 && self.outer.contains(&first) {
                self.gap("a local declaration that names an enclosing type parameter");
                return Ty::POISON;
            }
            if segs.len() == 1 {
                let msg = format!("no type named `{first}`");
                self.diags.error(Code::UnknownType, span, &msg);
            }
            return Ty::POISON;
        };
        let (args, bindings, row) = self.type_args(n, gn);
        // Only a trait has associated types (trait.binding.non-trait).
        if matches!(kind, HeadKind::Data | HeadKind::Enum | HeadKind::Newtype)
            && let Some((name, _)) = bindings.first()
        {
            let msg = format!(
                "`{}` is not a trait, so it has no associated type `{}`",
                self.names.display_name(def),
                self.names.text(*name)
            );
            self.diags.error(Code::UnknownAssociatedType, span, &msg);
        }
        self.ctor(def, (kind, gn), &args, row, span)
    }

    /// Whether `t` may be the inputs argument of `Fn` or `SuspendFn`: a
    /// tuple type, or a type parameter bounded by `Tuple`
    /// (`fn.type.ctor.inputs-tuple`).
    fn inputs_tuple(&self, t: Ty, gn: &Gen) -> bool {
        match self.names.pool.get(t) {
            TyData::Tuple { .. } | TyData::Poison => true,
            TyData::Param(_) => {
                gn.pending.contains(&t)
                    || gn.tys.iter().any(|(_, p, bounds)| {
                        *p == t && bounds.iter().any(|(def, _)| *def == self.names.known.tuple)
                    })
            }
            _ => false,
        }
    }

    /// Whether a `mut` type wraps the written name `Self`, which is no
    /// declared type parameter (types.param.mut-self).
    fn names_self(&self, n: NodeRef<'_>) -> bool {
        n.children().find(|c| c.kind().is_type()).is_some_and(|c| {
            c.kind() == SyntaxKind::NamedType
                && self.segments(c).first().is_some_and(|s| s.0 == "Self")
        })
    }

    fn ty(&mut self, n: Option<NodeRef<'_>>, gn: &Gen) -> Ty {
        let Some(n) = n else { return Ty::VOID };
        let pool = self.names.pool;
        let first_ty = |me: &mut Self, n: NodeRef<'_>| -> Ty {
            let c = n.children().find(|c| c.kind().is_type());
            me.ty(c, gn)
        };
        match n.kind() {
            SyntaxKind::NamedType => self.named(n, gn),
            SyntaxKind::OptionalType => {
                let t = first_ty(self, n);
                pool.intern_ty(&TyData::Option(t))
            }
            SyntaxKind::MutType => {
                let t = first_ty(self, n);
                // Neither a primitive nor a tuple has a `mut` form
                // (types.prim.no-mut.error, types.tuple.no-mut).
                match pool.get(t) {
                    TyData::Prim(_) => {
                        self.diags.error(
                            Code::MutOnPrimitive,
                            self.src.span(n),
                            "a primitive type has no `mut` form",
                        );
                        t
                    }
                    TyData::Tuple { .. } => {
                        self.diags.error(
                            Code::MutOnTuple,
                            self.src.span(n),
                            "a tuple type has no `mut` form",
                        );
                        t
                    }
                    TyData::Param(_) if !gn.bounded.contains(&t) && !self.names_self(n) => {
                        self.diags.error(
                            Code::MutOnTypeParameter,
                            self.src.span(n),
                            "`mut` on an unconstrained type parameter; use a `< mut Any` bound",
                        );
                        t
                    }
                    _ => pool.intern_ty(&TyData::Mut(t)),
                }
            }
            SyntaxKind::ParenType => first_ty(self, n),
            SyntaxKind::RestType => first_ty(self, n),
            SyntaxKind::TupleType => {
                let mut elems = Vec::new();
                let mut rest = None;
                for c in n.children().filter(|c| c.kind().is_type()) {
                    if c.kind() == SyntaxKind::RestType {
                        let t = first_ty(self, c);
                        // `types.tuple.rest.list`: a rest element is a `List`.
                        let list = self.names.known.list;
                        if !matches!(pool.get(t), TyData::Adt { def, .. } if def == list)
                            && t != Ty::POISON
                        {
                            self.diags.error(
                                Code::TypeMismatch,
                                self.src.span(c),
                                "a rest element's type must be a `List`",
                            );
                        }
                        rest = Some(t);
                    } else {
                        elems.push(self.ty(Some(c), gn));
                    }
                }
                pool.intern_ty(&TyData::Tuple {
                    elems: pool.list(&elems),
                    rest,
                })
            }
            SyntaxKind::DynType => {
                let Some(c) = Src::child(n, SyntaxKind::NamedType) else {
                    return Ty::POISON;
                };
                self.trait_value(c, gn, None).map_or(Ty::POISON, |t| {
                    let t = crate::assoc::with_super_bindings(self.names.pool.types(), self.r, t);
                    self.require_complete(t, self.src.span(n));
                    t
                })
            }
            SyntaxKind::FunctionType => {
                let arrow = n
                    .direct_token(&self.src.parse.tokens, TokenKind::Arrow)
                    .map(hd_base::TokenIdx::raw);
                let suspends = n
                    .direct_token(&self.src.parse.tokens, TokenKind::Bang)
                    .is_some();
                let mut params = Vec::new();
                let mut result = Ty::VOID;
                let mut row = RowId::EMPTY;
                let mut inputs = Inputs::Fixed;
                for c in n.children() {
                    if c.kind() == SyntaxKind::RequirementRow {
                        row = self.row(Some(c), gn);
                    } else if c.kind().is_type() {
                        let after = arrow.is_some_and(|a| self.src.first(c).raw() > a);
                        let t = self.ty(Some(c), gn);
                        if after {
                            result = t;
                        } else {
                            // `fn.type.vararg-rest`: a rest element is a
                            // `List`, and the inputs' last.
                            inputs = if c.kind() == SyntaxKind::RestType {
                                Inputs::Rest
                            } else {
                                Inputs::Fixed
                            };
                            params.push(t);
                        }
                    }
                }
                pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    row,
                    suspends,
                    inputs,
                })
            }
            SyntaxKind::ProjectionType => {
                let base_node = n.children().find(|c| c.kind().is_type());
                let base = self.ty(base_node, gn);
                let Some(t) = n.name(&self.src.parse.tokens) else {
                    return Ty::POISON;
                };
                let name = self.sym(self.src.text(t));
                // `Self::X` inside a trait leaves the trait's arguments
                // implicit (each use instantiates them); `T::X` names the
                // bound's arguments, defaults filled, so `T::Out` under
                // `T < Add` is `<T as Add[T]>::Out`.
                let mut cands: Vec<BoundRef> = Vec::new();
                if Some(base) == gn.self_ty
                    && let Some(tr) = gn.self_trait
                {
                    cands.push((tr, TyList::EMPTY));
                }
                if let Some((_, _, bounds)) = gn.tys.iter().find(|(_, t, _)| *t == base) {
                    cands.extend(bounds.iter().copied());
                }
                // The projection names the trait that declares the
                // associated type, which a bound may reach through its
                // supertraits (trait-solver.md §4.1).
                let mut decls: Vec<(DefId, TyList, DefId)> = Vec::new();
                for &(tr, a) in &cands {
                    for d in self.assoc_decls(tr, a, Some(base), name) {
                        if !decls.iter().any(|x| x.2 == d.2) {
                            decls.push(d);
                        }
                    }
                }
                if decls.len() > 1 {
                    let msg = format!(
                        "`{}` names an associated type that two bounds declare",
                        self.names.text(name)
                    );
                    self.diags
                        .error(Code::AmbiguousAssociatedType, self.src.span(n), &msg);
                }
                let found = decls.first().copied().or_else(|| {
                    cands.first().map(|&(tr, a)| {
                        let d = self
                            .names
                            .member(tr, PathKind::Member, self.names.text(name));
                        (tr, a, d)
                    })
                });
                let Some((trait_, args, assoc)) = found else {
                    self.gap("a projection on a type without a bound naming it");
                    return Ty::POISON;
                };
                pool.intern_ty(&TyData::Assoc {
                    assoc,
                    trait_,
                    self_ty: base,
                    args,
                })
            }
            SyntaxKind::InferType => Ty::POISON,
            SyntaxKind::RequirementRow => {
                self.gap("a requirement row in type position");
                Ty::POISON
            }
            SyntaxKind::ContextType => self.context_ty(n, gn),
            _ => {
                self.gap("this type form");
                Ty::POISON
            }
        }
    }

    /// A generic parameter list: names first, so bounds may name later
    /// parameters, then bounds and defaults.
    fn generics(
        &mut self,
        gl: Option<NodeRef<'_>>,
        owner: DefId,
        offset: u16,
        gn: &mut Gen,
    ) -> Vec<Generic> {
        let Some(gl) = gl else { return Vec::new() };
        let params: Vec<NodeRef<'_>> = gl
            .children()
            .filter(|c| c.kind() == SyntaxKind::GenericParameter)
            .collect();
        let mut out = Vec::new();
        let start = gn.tys.len();
        for (i, g) in params.iter().enumerate() {
            let Some(t) = g.name(&self.src.parse.tokens) else {
                continue;
            };
            let name = self.sym(self.src.text(t));
            if self.r.frozen.is_none() {
                self.prelude_shadow(*g, self.src.text(t));
            }
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            let row = g
                .direct_token(&self.src.parse.tokens, TokenKind::Dollar)
                .is_some();
            if row {
                gn.rows.push((name, RowParamRef { owner, index }));
            } else {
                let t = self
                    .names
                    .pool
                    .intern_ty(&TyData::Param(ParamRef { owner, index }));
                gn.tys.push((name, t, Vec::new()));
                gn.pending.push(t);
            }
            let mut generic = Generic::plain(name);
            let toks = &self.src.parse.tokens;
            if g.direct_token(toks, TokenKind::Plus).is_some() {
                generic.variance = 1;
            } else if g.direct_token(toks, TokenKind::Minus).is_some() {
                generic.variance = -1;
            }
            out.push((generic, *g, row));
        }
        let mut done = Vec::new();
        let mut defaulted = false;
        for (position, (mut g, node, row)) in out.into_iter().enumerate() {
            self.generic_default_rules(&params, position, row, &mut defaulted);
            let mut bounds = Vec::new();
            if let Some(bl) = Src::child(node, SyntaxKind::BoundList) {
                g.mut_bound = bl
                    .direct_token(&self.src.parse.tokens, TokenKind::KwMut)
                    .is_some();
                let me = (!row)
                    .then(|| gn.tys[start..].iter().find(|(s, _, _)| *s == g.name))
                    .flatten()
                    .map(|(_, t, _)| *t);
                for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                    if let Some(t) = self.trait_value(b, gn, me) {
                        bounds.push(t);
                    }
                }
                if let Some(me) = me {
                    self.bound_clashes(me, &bounds, self.src.span(bl));
                }
            }
            if !row
                && Src::child(node, SyntaxKind::BoundList).is_some()
                && let Some((_, t, _)) = gn.tys[start..].iter().find(|(s, _, _)| *s == g.name)
            {
                let t = *t;
                gn.bounded.push(t);
            }
            g.bound = bounds.first().and_then(|b| self.trait_of(*b));
            if !row && let Some(slot) = gn.tys[start..].iter_mut().find(|(s, _, _)| *s == g.name) {
                slot.2 = bounds
                    .iter()
                    .filter_map(|b| match self.names.pool.get(*b) {
                        TyData::TraitValue { def, args, .. } => Some((def, args)),
                        _ => None,
                    })
                    .collect();
            }
            if let Some((_, t, _)) = gn.tys[start..].iter().find(|(s, _, _)| *s == g.name) {
                let t = *t;
                gn.pending.retain(|p| *p != t);
            }
            g.bounds = bounds;
            if let Some(d) = Src::child(node, SyntaxKind::TypeDefault) {
                let c = d
                    .children()
                    .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow);
                if let Some(c) = c {
                    g.default = Some(self.ty(Some(c), gn));
                }
            }
            g.row = row;
            done.push(g);
        }
        done
    }

    /// `types.generic.default.order`, `.later`, `.kind`: the
    /// parameter at `position` of a generic list follows a defaulted one
    /// only with a default of its own, and a default names neither its own
    /// parameter nor a later one. `defaulted`: an earlier parameter had one.
    fn generic_default_rules(
        &mut self,
        params: &[NodeRef<'_>],
        position: usize,
        row: bool,
        defaulted: &mut bool,
    ) {
        let node = params[position];
        let Some(default) = Src::child(node, SyntaxKind::TypeDefault) else {
            if *defaulted {
                let msg = format!(
                    "`{}` follows a parameter with a default, so it needs one too",
                    self.src.text(
                        node.name(&self.src.parse.tokens)
                            .unwrap_or(self.src.first(node))
                    )
                );
                self.diags
                    .error(Code::DefaultOrder, self.src.span(node), &msg);
            }
            return;
        };
        *defaulted = true;
        let toks = &self.src.parse.tokens;
        // `types.generic.default.kind`: a row after `$` for a row parameter,
        // a type for a type parameter.
        let row_default = default.children().any(|c| {
            c.kind() == SyntaxKind::RequirementRow
                && c.direct_token(toks, TokenKind::Dollar).is_some()
        });
        if row_default != row {
            let (has, wants) = if row {
                ("a type", "row")
            } else {
                ("a row", "type")
            };
            let msg = format!("the default is {has}, but its parameter is a {wants} parameter");
            self.diags
                .error(Code::GenericKindMismatch, self.src.span(default), &msg);
        }
        let later: Vec<&str> = params[position..]
            .iter()
            .filter_map(|p| p.name(toks))
            .map(|t| self.src.text(t))
            .collect();
        for c in default
            .descendants()
            .filter(|c| c.kind() == SyntaxKind::NamedType)
        {
            let segs = self.segments(c);
            if let [(first, _)] = segs.as_slice()
                && later.contains(&first.as_str())
            {
                let msg =
                    format!("a default cannot name `{first}`, its own parameter or a later one");
                self.diags
                    .error(Code::BindingNotYetVisible, self.src.span(c), &msg);
            }
        }
    }

    /// `fn.default.order-final-function`, `data.shared.default.order`:
    /// after the first parameter with a default, every following
    /// parameter has one. A variadic parameter never needs one; a final
    /// parameter of function type does not either, when `final_fn`.
    fn parameter_default_order(&mut self, params: &[NodeRef<'_>], final_fn: bool) {
        let toks = &self.src.parse.tokens;
        let mut defaulted = false;
        for (i, p) in params.iter().enumerate() {
            if Src::child(*p, SyntaxKind::DefaultValue).is_some() {
                defaulted = true;
                continue;
            }
            let exempt = p.direct_token(toks, TokenKind::Ellipsis).is_some()
                || (final_fn
                    && i + 1 == params.len()
                    && Src::type_child(*p).is_some_and(|t| t.kind() == SyntaxKind::FunctionType));
            if defaulted && !exempt {
                let name = p.name(toks).map_or("a parameter", |t| self.src.text(t));
                let msg =
                    format!("`{name}` follows a parameter with a default, so it needs one too");
                self.diags
                    .error(Code::DefaultOrder, self.src.span(*p), &msg);
            }
        }
    }

    /// `fn.decl.result-required-pub`: a public function, a public inherent
    /// method, and a trait method or the method of a trait implementation
    /// declare their result type.
    fn result_required(&mut self, f: NodeRef<'_>, public: bool) {
        if public && self.r.frozen.is_none() && Src::type_child(f).is_none() {
            self.diags.error(
                Code::MissingResultType,
                self.src.span(f),
                "a public declaration states its result type: write `-> T`",
            );
        }
    }

    /// `module.prelude.no-shadow`: a parameter or a type parameter does not
    /// bind a prelude name.
    fn prelude_shadow(&mut self, node: NodeRef<'_>, name: &str) {
        let prelude = PRELUDE
            .iter()
            .any(|(module, list)| list.contains(&name) && self.r.module_exists(module));
        if prelude {
            let msg = format!("`{name}` shadows a prelude name");
            self.diags
                .error(Code::PreludeNameShadow, self.src.span(node), &msg);
        }
    }

    /// `fn.vararg.final`: a vararg is the last positional parameter.
    fn vararg_final(&mut self, params: &[NodeRef<'_>]) {
        let toks = &self.src.parse.tokens;
        for p in params.iter().rev().skip(1) {
            if p.direct_token(toks, TokenKind::Ellipsis).is_some() {
                let name = p.name(toks).map_or("a parameter", |t| self.src.text(t));
                let msg = format!("`{name}` is a vararg, so it must be the last parameter");
                self.diags
                    .error(Code::NonfinalVararg, self.src.span(*p), &msg);
            }
        }
    }

    /// A function header: generics, parameters, result, row, `!`.
    fn sig(&mut self, f: NodeRef<'_>, owner: DefId, gn: &Gen) -> FnSig {
        let mut gn = gn.clone();
        let generics = self.generics(
            Src::child(f, SyntaxKind::GenericParameterList),
            owner,
            0,
            &mut gn,
        );
        let mut params = Vec::new();
        let mut defaults = Vec::new();
        let mut variadic = false;
        if let Some(pl) = Src::child(f, SyntaxKind::ParameterList) {
            let nodes: Vec<NodeRef<'_>> = pl
                .children()
                .filter(|c| c.kind() == SyntaxKind::Parameter)
                .collect();
            self.parameter_default_order(&nodes, true);
            self.vararg_final(&nodes);
            for (i, p) in nodes.into_iter().enumerate() {
                let toks = &self.src.parse.tokens;
                if p.direct_token(toks, TokenKind::KwSelfValue).is_some() {
                    let st = gn.self_ty.unwrap_or(Ty::POISON);
                    let st = if p.direct_token(toks, TokenKind::KwMut).is_some() {
                        self.names.pool.intern_ty(&TyData::Mut(st))
                    } else {
                        st
                    };
                    params.push((self.sym("self"), st));
                    defaults.push(false);
                    continue;
                }
                if p.direct_token(toks, TokenKind::Ellipsis).is_some() {
                    variadic = true;
                }
                let name = p
                    .name(toks)
                    .map_or_else(|| i.to_string(), |t| self.src.text(t).to_owned());
                if self.r.frozen.is_none() {
                    self.prelude_shadow(p, &name);
                }
                let ty = self.ty(Src::type_child(p), &gn);
                params.push((self.sym(&name), ty));
                defaults.push(Src::child(p, SyntaxKind::DefaultValue).is_some());
            }
        }
        let ret = self.ty(Src::type_child(f), &gn);
        let row = self.row(Src::child(f, SyntaxKind::RequirementRow), &gn);
        let suspends = f
            .direct_token(&self.src.parse.tokens, TokenKind::Bang)
            .is_some();
        FnSig {
            generics,
            params,
            defaults,
            ret,
            row,
            suspends,
            variadic,
        }
    }

    fn fields(&mut self, holder: Option<NodeRef<'_>>, gn: &Gen) -> Vec<Field> {
        let mut out = Vec::new();
        let Some(holder) = holder else { return out };
        for (i, f) in holder.children().enumerate() {
            match f.kind() {
                SyntaxKind::DataField | SyntaxKind::Parameter => {
                    let name = f
                        .name(&self.src.parse.tokens)
                        .map_or_else(|| i.to_string(), |t| self.src.text(t).to_owned());
                    let ty = self.ty(Src::type_child(f), gn);
                    out.push(Field {
                        name: self.sym(&name),
                        ty,
                        public: self.src.is_pub(f),
                        has_default: Src::child(f, SyntaxKind::DefaultValue).is_some(),
                        embedded: false,
                    });
                }
                SyntaxKind::EmbeddedField => {
                    let tn = Src::type_child(f);
                    let name = tn
                        .map(|t| self.segments(t))
                        .and_then(|s| s.last().map(|x| x.0.clone()))
                        .unwrap_or_default();
                    let ty = self.ty(tn, gn);
                    out.push(Field {
                        name: self.sym(&name),
                        ty,
                        public: self.src.is_pub(f),
                        has_default: false,
                        embedded: true,
                    });
                }
                _ => {}
            }
        }
        out
    }

    /// A declaration's declared parameter variances (types.variance.list,
    /// types.variance.map for the built-in collections).
    fn variances(&self, def: DefId) -> Vec<i8> {
        let known = self.names.known;
        match def {
            d if d == known.list => vec![1],
            d if d == known.map => vec![0, 1],
            _ if self.r.variances.contains_key(&def) => self.r.variances[&def].clone(),
            _ => self
                .r
                .item(def)
                .map(|i| i.generics.iter().map(|g| g.variance).collect())
                .unwrap_or_default(),
        }
    }

    /// Checks the fields under `holder` against the declared markers of
    /// `owner`'s parameters (types.variance.verified): each field is a
    /// positive position, an embedded one an invariant position.
    fn field_variance(
        &mut self,
        owner: DefId,
        generics: &[Generic],
        holder: Option<NodeRef<'_>>,
        fields: &[Field],
    ) {
        let Some(holder) = holder else { return };
        let nodes = holder.children().filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::DataField | SyntaxKind::Parameter | SyntaxKind::EmbeddedField
            )
        });
        let mut bad = Vec::new();
        for (node, f) in nodes.zip(fields) {
            let mut seen = vec![Seen::default(); generics.len()];
            let pol = i8::from(!f.embedded);
            let var_of = |d: DefId| self.variances(d);
            variance::walk(self.names.pool, f.ty, pol, owner, &var_of, &mut seen);
            if let Some(g) = Self::broken(generics, &seen) {
                bad.push((self.src.span(node), g));
            }
        }
        for (span, g) in bad {
            self.variance_error(span, g);
        }
    }

    /// The first parameter whose occurrences break its marker.
    fn broken(generics: &[Generic], seen: &[Seen]) -> Option<(Symbol, i8)> {
        generics
            .iter()
            .zip(seen)
            .find(|(g, s)| s.breaks(g.variance))
            .map(|(g, _)| (g.name, g.variance))
    }

    fn variance_error(&mut self, span: Span, (name, marker): (Symbol, i8)) {
        let msg = format!(
            "`{}` is declared {}, but it occurs in a {} position here",
            self.names.text(name),
            if marker > 0 {
                "covariant (`+`)"
            } else {
                "contravariant (`-`)"
            },
            if marker > 0 {
                "negative or invariant"
            } else {
                "positive or invariant"
            }
        );
        self.diags.error(Code::InvalidVariance, span, &msg);
    }

    /// The instance methods of an inherent impl, checked by the variance
    /// each impl parameter has in the target (types.variance.surface,
    /// types.variance.target.check).
    fn impl_variance(
        &mut self,
        def: DefId,
        generics: &[Generic],
        (self_ty, gn): (Ty, &Gen),
        block: NodeRef<'_>,
    ) {
        let pool = self.names.pool;
        let TyData::Adt { def: target, args } = pool.get(self_ty) else {
            return;
        };
        // The built-in collections' markers describe their readonly views
        // only (types.variance.list, types.variance.map).
        if target == self.names.known.list || target == self.names.known.map {
            return;
        }
        let declared = self.variances(target);
        if declared.iter().all(|v| *v == 0) {
            return;
        }
        // Each impl parameter's derived variance in the target.
        let mut seen = vec![Seen::default(); generics.len()];
        for (a, v) in pool.list_items(args).iter().copied().zip(&declared) {
            let var_of = |d: DefId| self.variances(d);
            variance::walk(pool, a, *v, def, &var_of, &mut seen);
        }
        let derived: Vec<Generic> = generics
            .iter()
            .zip(&seen)
            .map(|(g, s)| Generic {
                variance: s.derived(),
                ..g.clone()
            })
            .collect();
        if derived.iter().all(|g| g.variance == 0) {
            return;
        }
        let mut bad = Vec::new();
        for f in block.children().filter(|c| c.kind() == SyntaxKind::FnDecl) {
            let Some(pl) = Src::child(f, SyntaxKind::ParameterList) else {
                continue;
            };
            let toks = &self.src.parse.tokens;
            let params: Vec<NodeRef<'_>> = pl
                .children()
                .filter(|c| c.kind() == SyntaxKind::Parameter)
                .collect();
            if params
                .first()
                .is_none_or(|p| p.direct_token(toks, TokenKind::KwSelfValue).is_none())
            {
                continue;
            }
            let Some(name) = f.name(toks) else {
                continue;
            };
            let text = self.src.text(name).to_owned();
            let mdef = self.names.member(def, PathKind::Member, &text);
            let mut probe = std::mem::take(self.diags);
            let sig = self.sig(f, mdef, gn);
            std::mem::swap(self.diags, &mut probe);
            let mut seen = vec![Seen::default(); generics.len()];
            let var_of = |d: DefId| self.variances(d);
            for (_, t) in sig.params.iter().skip(1) {
                variance::walk(pool, *t, -1, def, &var_of, &mut seen);
            }
            variance::walk(pool, sig.ret, 1, def, &var_of, &mut seen);
            for k in pool.row_data(sig.row).keys {
                variance::walk(pool, k, 0, def, &var_of, &mut seen);
            }
            if let Some(g) = Self::broken(&derived, &seen) {
                bad.push((self.src.span(f), g));
            }
        }
        for (span, g) in bad {
            self.variance_error(span, g);
        }
    }

    fn decorators(&self, n: NodeRef<'_>) -> Vec<(String, Vec<String>)> {
        n.children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .filter_map(|d| decorator_line(&self.src, d))
            .collect()
    }

    fn members(
        &mut self,
        block: Option<NodeRef<'_>>,
        owner: DefId,
        public: bool,
        gn: &Gen,
        out: &mut Vec<Item>,
    ) -> (Vec<(Symbol, DefId)>, Vec<AssocDecl>) {
        let mut methods = Vec::new();
        let mut assoc = Vec::new();
        for f in block.iter().flat_map(|b| b.children()) {
            match f.kind() {
                SyntaxKind::FnDecl => {
                    let Some(t) = f
                        .name(&self.src.parse.tokens)
                        .or_else(|| self.src.name_after(f, TokenKind::KwFn))
                    else {
                        continue;
                    };
                    let text = self.src.text(t).to_owned();
                    let name = self.sym(&text);
                    let def = self.names.member(owner, PathKind::Member, &text);
                    self.result_required(f, public || self.src.is_pub(f));
                    let sig = self.sig(f, def, gn);
                    let mut it = Item::new(
                        def,
                        name,
                        public || self.src.is_pub(f),
                        ItemData::Method {
                            owner,
                            sig,
                            has_body: Src::child(f, SyntaxKind::Block).is_some(),
                        },
                    );
                    it.intrinsic = self.intrinsic(f);
                    out.push(it);
                    methods.push((name, def));
                }
                SyntaxKind::AssociatedTypeDecl => {
                    let Some(t) = f.name(&self.src.parse.tokens) else {
                        continue;
                    };
                    let text = self.src.text(t).to_owned();
                    let def = self.names.member(owner, PathKind::Member, &text);
                    let ty = Src::type_child(f).map(|c| self.ty(Some(c), gn));
                    assoc.push((self.sym(&text), def, ty));
                }
                _ => {}
            }
        }
        (methods, assoc)
    }

    /// A trait or trait implementation declares each member once
    /// (trait.member.unique). Error: `duplicate-trait-member`.
    fn duplicate_members(&mut self, block: Option<NodeRef<'_>>) {
        if self.r.frozen.is_some() {
            return;
        }
        let mut seen = HashSet::new();
        let mut bad = Vec::new();
        for f in block.iter().flat_map(|b| b.children()) {
            if !matches!(
                f.kind(),
                SyntaxKind::FnDecl | SyntaxKind::AssociatedTypeDecl
            ) {
                continue;
            }
            let Some(t) = f
                .name(&self.src.parse.tokens)
                .or_else(|| self.src.name_after(f, TokenKind::KwFn))
            else {
                continue;
            };
            let text = self.src.text(t).to_owned();
            if !seen.insert(text.clone()) {
                bad.push((self.src.span(f), text));
            }
        }
        for (span, text) in bad {
            let msg = format!("`{text}` is declared twice");
            self.diags.error(Code::DuplicateTraitMember, span, &msg);
        }
    }

    /// `@num_suffix` or `@str_prefix` before a function.
    fn literal_fn(&self, n: NodeRef<'_>) -> u8 {
        self.decorators(n)
            .iter()
            .find_map(|(d, _)| match d.as_str() {
                "num_suffix" => Some(crate::iface::LITERAL_SUFFIX),
                "str_prefix" => Some(crate::iface::LITERAL_PREFIX),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn intrinsic(&self, n: NodeRef<'_>) -> Option<Symbol> {
        self.decorators(n)
            .into_iter()
            .find(|(d, _)| d == "intrinsic")
            .and_then(|(_, a)| a.first().map(|k| self.sym(k)))
    }

    fn impl_item(&mut self, h: &Head<'_>, out: &mut Vec<Item>) {
        let n = h.node;
        let pool = self.names.pool;
        let mut gn = Gen::default();
        let generics = self.generics(
            Src::child(n, SyntaxKind::GenericParameterList),
            h.def,
            0,
            &mut gn,
        );
        let tys: Vec<NodeRef<'_>> = n.children().filter(|c| c.kind().is_type()).collect();
        let (trait_node, target) = match tys.as_slice() {
            [t] => (None, *t),
            [tr, t, ..] => (Some(*tr), *t),
            [] => return,
        };
        // An implementation target has no outer `mut` (trait.target.no-mut).
        if target.kind() == SyntaxKind::MutType && self.r.frozen.is_none() {
            self.diags.error(
                Code::MutableImplTarget,
                self.src.span(target),
                "an implementation target cannot be a `mut` view",
            );
        }
        let self_ty = self.ty(Some(target), &gn);
        gn.self_ty = Some(self_ty);
        let toks: Vec<_> = n.direct_tokens().collect();
        let by = toks
            .iter()
            .position(|t| self.src.tkind(*t) == Some(TokenKind::Ident) && self.src.text(*t) == "by")
            .and_then(|i| toks.get(i + 1))
            .map(|t| self.src.text(*t).to_owned());
        let (trait_, trait_args, header_bindings) = match trait_node {
            None => (DefId::NONE, TyList::EMPTY, Vec::new()),
            Some(tn) => match self.trait_value(tn, &gn, Some(self_ty)) {
                Some(tv) => match pool.get(tv) {
                    TyData::TraitValue {
                        def,
                        args,
                        bindings,
                    } => (def, args, bindings),
                    _ => return,
                },
                None => return,
            },
        };
        // The compiler-implemented sealed traits take no written
        // implementation (types.sealed.no-impl).
        if self.r.frozen.is_none() && by.is_none() && self.names.known.is_sealed(trait_) {
            let msg = format!(
                "`{}` is implemented by the compiler only",
                self.names.display_name(trait_)
            );
            self.diags
                .error(Code::SealedTraitImplementation, self.src.span(n), &msg);
        }
        if self.r.frozen.is_none() {
            let head = ImplHead {
                has_trait: trait_node.is_some(),
                by: by.is_some(),
                target,
                trait_args,
                self_ty,
            };
            self.impl_head_rules(h, &generics, &head);
        }
        let bare_param = matches!(pool.get(self_ty), TyData::Param(p) if p.owner == h.def);
        let tuple_bound = bare_param
            && generics
                .first()
                .is_some_and(|g| g.bound.is_some_and(|b| b == self.names.known.tuple));
        let kind = match by.as_deref() {
            Some("Structure") if tuple_bound => ImplKind::TupleTemplate,
            Some("Structure") if bare_param => ImplKind::Template,
            Some("Structure") => ImplKind::Derivation,
            Some(_) => ImplKind::Delegated,
            None => ImplKind::Written,
        };
        if self.r.frozen.is_none() {
            // A header without a trait never delegates
            // (trait.by.no-trait.error).
            if trait_ == DefId::NONE && kind == ImplKind::Delegated {
                self.diags.error(
                    Code::InvalidDelegation,
                    self.src.span(n),
                    "an implementation without a trait has nothing to delegate; only `by Structure` may follow it",
                );
            }
            // A delegating implementation takes its associated types from
            // the part (trait.by.assoc-binding).
            if trait_ != DefId::NONE && kind == ImplKind::Delegated {
                for c in Src::child(n, SyntaxKind::Block)
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::AssociatedTypeDecl)
                {
                    self.diags.error(
                        Code::InvalidDelegation,
                        self.src.span(c),
                        "a delegating implementation takes its associated types from the part, so it binds none",
                    );
                }
            }
        }
        // A block with no trait writes metadata only and declares no
        // members (`annot.no-trait.not-inherent`).
        let block = if trait_ == DefId::NONE && by.as_deref() == Some("Structure") {
            None
        } else {
            Src::child(n, SyntaxKind::Block)
        };
        if trait_ != DefId::NONE {
            self.duplicate_members(block);
        }
        let (methods, assoc_decls) = self.members(block, h.def, trait_ != DefId::NONE, &gn, out);
        if trait_ == DefId::NONE
            && self.r.frozen.is_none()
            && let Some(b) = block
        {
            self.impl_variance(h.def, &generics, (self_ty, &gn), b);
        }
        let mut assoc: Vec<(DefId, Ty)> = header_bindings;
        for (name, _, ty) in assoc_decls {
            if trait_ != DefId::NONE {
                assoc.push((
                    self.names
                        .member(trait_, PathKind::Member, self.names.text(name)),
                    ty.unwrap_or(Ty::POISON),
                ));
            }
        }
        let mut it = Item::new(
            h.def,
            h.name,
            true,
            ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                methods,
                assoc,
                by: by.map(|b| self.sym(&b)),
                kind,
            },
        );
        it.generics = generics;
        out.push(it);
    }

    /// What an implementation head must satisfy: no trait value type or
    /// row extension as a target (`trait.target.trait-value.error`,
    /// `trait.target.row-argument.extension`), no inherent target that is a
    /// tuple or a transparent alias (`trait.own.inherent.tuple-alias`), and
    /// every type parameter constrained by the head
    /// (`trait.overlap.constrained-head`, `trait.overlap.head-projection`).
    fn impl_head_rules(&mut self, h: &Head<'_>, generics: &[Generic], head: &ImplHead<'_>) {
        let pool = self.names.pool;
        let span = self.src.span(h.node);
        let args: Vec<Ty> = pool.list_items(head.trait_args).to_vec();
        let roots: Vec<Ty> = args.iter().copied().chain([head.self_ty]).collect();
        if head.has_trait && matches!(pool.get(head.self_ty), TyData::TraitValue { .. }) {
            self.diags.error(
                Code::TraitValueImplTarget,
                self.src.span(head.target),
                "a trait value type is never an implementation target",
            );
        }
        if !head.has_trait && !head.by {
            // A primitive name is no alias, whatever the prelude binds it to.
            let segs = self.segments(head.target);
            let alias = head.target.kind() == SyntaxKind::NamedType
                && matches!(segs.as_slice(), [(first, _)]
                    if !Prim::ALL.iter().any(|p| p.name() == first)
                        && self.resolve_path(&segs, span).is_some_and(|(_, k)| k == HeadKind::Alias));
            if alias || matches!(pool.get(head.self_ty), TyData::Tuple { .. }) {
                self.diags.error(
                    Code::InvalidImplTarget,
                    span,
                    "an inherent implementation cannot target a tuple or a transparent alias",
                );
            }
        }
        if roots.iter().any(|t| {
            ty_any(pool, *t, &mut |d| match d {
                TyData::Fn { row, .. } | TyData::Row(row) => {
                    let r = pool.row_data(row);
                    !r.params.is_empty() && !r.keys.is_empty()
                }
                _ => false,
            })
        }) {
            self.diags.error(
                Code::InvalidImplTarget,
                span,
                "a row argument in an implementation head cannot extend a row parameter",
            );
        }
        let own = |d: &TyData| matches!(d, TyData::Param(p) if p.owner == h.def);
        let projects = roots.iter().any(|t| {
            ty_any(pool, *t, &mut |d| {
                matches!(d, TyData::Assoc { self_ty, .. } if ty_any(pool, self_ty, &mut |x| own(&x)))
            })
        });
        if projects {
            self.diags.error(
                Code::UnconstrainedImplParameter,
                span,
                "an implementation head cannot project one of its type parameters",
            );
            return;
        }
        let mut constrained: HashSet<u16> = HashSet::new();
        let note = |t: Ty, set: &mut HashSet<u16>| {
            ty_any(pool, t, &mut |d| {
                if let TyData::Param(p) = d
                    && p.owner == h.def
                {
                    set.insert(p.index);
                }
                false
            });
        };
        for t in &roots {
            note(*t, &mut constrained);
        }
        // An associated-type binding in the bound of a constrained
        // parameter constrains the parameter it names.
        loop {
            let before = constrained.len();
            for (i, g) in generics.iter().enumerate() {
                if g.row || !constrained.contains(&u16::try_from(i).unwrap_or(u16::MAX)) {
                    continue;
                }
                for b in &g.bounds {
                    if let TyData::TraitValue { bindings, .. } = pool.get(*b) {
                        for (_, t) in bindings {
                            note(t, &mut constrained);
                        }
                    }
                }
            }
            if constrained.len() == before {
                break;
            }
        }
        for (i, g) in generics.iter().enumerate() {
            if !g.row && !constrained.contains(&u16::try_from(i).unwrap_or(u16::MAX)) {
                let msg = format!(
                    "the type parameter `{}` appears in neither the trait arguments nor the target",
                    self.names.text(g.name)
                );
                self.diags
                    .error(Code::UnconstrainedImplParameter, span, &msg);
            }
        }
    }

    /// `@derive(X, ...)`: the head `impl[T < X, ...] X for D[T, ...]`
    /// (`annot.derive.means`, derived bounds).
    fn derived(&mut self, h: &Head<'_>, generics: &[Generic], out: &mut Vec<Item>) {
        let pool = self.names.pool;
        for (d, args) in self.decorators(h.node) {
            if d != "derive" {
                continue;
            }
            for a in args {
                let sym = self.sym(&a);
                if self.is_poisoned(&a) {
                    continue;
                }
                let Some(Binding {
                    kind: BindingKind::Item,
                    value,
                }) = self.scope.lookup(sym)
                else {
                    let msg = format!("no trait named `{a}`");
                    self.diags
                        .error(Code::UnknownTrait, self.src.span(h.node), &msg);
                    continue;
                };
                let tr = DefId::from_raw(value);
                let seg = format!("derive {a} for {}", self.names.text(h.name));
                let def = DefId::from_raw(
                    self.names
                        .paths
                        .intern(self.names.module(self.module), PathKind::Impl, &seg)
                        .raw(),
                );
                let mut gs = Vec::new();
                let mut params = Vec::new();
                for (i, g) in generics.iter().enumerate() {
                    let index = u16::try_from(i).unwrap_or(u16::MAX);
                    let param = pool.intern_ty(&TyData::Param(ParamRef { owner: def, index }));
                    params.push(param);
                    let mut g2 = Generic::plain(g.name);
                    if !g.row {
                        g2.bound = Some(tr);
                        g2.bounds = vec![pool.intern_ty(&TyData::TraitValue {
                            def: tr,
                            args: self.r.fill_trait_args(tr, TyList::EMPTY, Some(param)),
                            bindings: vec![],
                        })];
                    }
                    g2.row = g.row;
                    gs.push(g2);
                }
                let self_ty = pool.intern_ty(&TyData::Adt {
                    def: h.def,
                    args: pool.list(&params),
                });
                let mut it = Item::new(
                    def,
                    self.sym(&seg),
                    true,
                    ItemData::Impl {
                        trait_: tr,
                        trait_args: self.r.fill_trait_args(tr, TyList::EMPTY, Some(self_ty)),
                        self_ty,
                        methods: vec![],
                        assoc: vec![],
                        by: None,
                        kind: ImplKind::Derived,
                    },
                );
                it.generics = gs;
                out.push(it);
            }
        }
    }

    /// `@error` on a data type or an enum: the heads of the generated
    /// `impl Display`, `impl Error` and one `impl From[P]` per `@from`
    /// member (spec 14 `annot.error.generates`), each an ordinary
    /// implementation with one method, whose body the checker writes
    /// (codegen.md §13.14). `fields` are each variant's members (a data
    /// type's fields as its one variant), over the declaration's
    /// parameters. A generic error type gets the generated bounds
    /// (`annot.error.bound.*`).
    fn error_impls(
        &mut self,
        h: &Head<'_>,
        generics: &[Generic],
        fields: &[&[Field]],
        out: &mut Vec<Item>,
    ) {
        use crate::error_type::{Generated, Message, interpolated, label, shape};
        let Some(shape) = shape(&self.src, h.node) else {
            return;
        };
        let pool = self.names.pool;
        let known = self.names.known;
        let param_of = |t: Ty| match pool.get(t) {
            TyData::Param(p) if p.owner == h.def => Some(usize::from(p.index)),
            _ => None,
        };
        // Which roles each parameter plays: the type of an interpolated, a
        // transparent, or a cause member.
        let n = generics.len();
        let (mut shown, mut transparent, mut cause) =
            (vec![false; n], vec![false; n], vec![false; n]);
        for (v, fs) in shape.variants.iter().zip(fields) {
            if v.members.len() != fs.len() {
                continue;
            }
            match v.message {
                Message::Text(msg) => {
                    let used = interpolated(&self.src, msg);
                    for f in *fs {
                        if used.contains(&label(&self.names, f))
                            && let Some(i) = param_of(f.ty)
                        {
                            shown[i] = true;
                        }
                    }
                }
                Message::Transparent => {
                    if let Some(i) = fs.first().and_then(|f| param_of(f.ty)) {
                        transparent[i] = true;
                    }
                }
                Message::Absent => {}
            }
            if let Some(c) = v.cause() {
                let t = fs[c].ty;
                let inner = match pool.get(t) {
                    TyData::Option(x) => x,
                    _ => t,
                };
                if let Some(i) = param_of(inner) {
                    cause[i] = true;
                }
            }
        }
        let display_bounds: Vec<Vec<DefId>> = (0..n)
            .map(|i| {
                if shown[i] || transparent[i] {
                    vec![known.display]
                } else {
                    vec![]
                }
            })
            .collect();
        let error_bounds: Vec<Vec<DefId>> = (0..n)
            .map(|i| {
                if cause[i] || transparent[i] {
                    vec![known.error]
                } else if shown[i] {
                    vec![known.display, known.inspectable]
                } else {
                    vec![known.inspectable]
                }
            })
            .collect();
        let error_ty = pool.intern_ty(&TyData::TraitValue {
            def: known.error,
            args: TyList::EMPTY,
            bindings: vec![],
        });
        let cause_ty = pool.intern_ty(&TyData::Option(error_ty));
        let mut heads = Vec::new();
        for g in shape.generated() {
            heads.push(match g {
                Generated::Display => (g, known.display, None, display_bounds.clone()),
                Generated::Error => (g, known.error, None, error_bounds.clone()),
                Generated::From { variant, member } => {
                    let Some(f) = fields.get(variant).and_then(|fs| fs.get(member)) else {
                        continue;
                    };
                    // A `@from` of a bare type parameter generates no `From`
                    // (`annot.error.from.type-parameter`, already reported).
                    if param_of(f.ty).is_some() || pool.has_poison(f.ty) {
                        continue;
                    }
                    (g, known.from, Some(f.ty), vec![vec![]; n])
                }
            });
        }
        for (g, tr, arg, bounds) in heads {
            let def = g.def(&self.names, h.def);
            let mut gs = Vec::new();
            let mut params = Vec::new();
            for (i, decl) in generics.iter().enumerate() {
                let index = u16::try_from(i).unwrap_or(u16::MAX);
                let param = pool.intern_ty(&TyData::Param(ParamRef { owner: def, index }));
                params.push(param);
                let mut g2 = Generic::plain(decl.name);
                g2.row = decl.row;
                if !decl.row {
                    g2.bound = bounds[i].first().copied();
                    g2.bounds = bounds[i]
                        .iter()
                        .map(|&b| {
                            pool.intern_ty(&TyData::TraitValue {
                                def: b,
                                args: self.r.fill_trait_args(b, TyList::EMPTY, Some(param)),
                                bindings: vec![],
                            })
                        })
                        .collect();
                }
                gs.push(g2);
            }
            let own = |t: Ty| {
                pool.subst(t, &|p: ParamRef| {
                    (p.owner == h.def).then(|| params.get(usize::from(p.index)).copied())?
                })
            };
            let self_ty = pool.intern_ty(&TyData::Adt {
                def: h.def,
                args: pool.list(&params),
            });
            let arg = arg.map(own);
            let trait_args = self.r.fill_trait_args(
                tr,
                arg.map_or(TyList::EMPTY, |a| pool.list(&[a])),
                Some(self_ty),
            );
            let method = g.method();
            let mdef = self.names.member(def, PathKind::Member, method);
            let sig = match (g, arg) {
                (Generated::Display, _) => {
                    FnSig::simple(vec![], vec![(self.sym("self"), self_ty)], Ty::STRING)
                }
                (Generated::Error, _) => {
                    FnSig::simple(vec![], vec![(self.sym("self"), self_ty)], cause_ty)
                }
                (Generated::From { .. }, a) => FnSig::simple(
                    vec![],
                    vec![(self.sym("value"), a.unwrap_or(Ty::POISON))],
                    self_ty,
                ),
            };
            let methods = if g == Generated::Error && !shape.has_cause() {
                Vec::new()
            } else {
                out.push(Item::new(
                    mdef,
                    self.sym(method),
                    true,
                    ItemData::Method {
                        owner: def,
                        sig,
                        has_body: true,
                    },
                ));
                vec![(self.sym(method), mdef)]
            };
            let seg = g.segment(self.names.display_name(h.def));
            let mut it = Item::new(
                def,
                self.sym(&seg),
                true,
                ItemData::Impl {
                    trait_: tr,
                    trait_args,
                    self_ty,
                    methods,
                    assoc: vec![],
                    by: None,
                    kind: ImplKind::Error,
                },
            );
            it.generics = gs;
            out.push(it);
        }
    }

    /// A trait's `Self` (its parameter 0), the scope of its header, and
    /// its declared parameters.
    fn trait_generics(&mut self, h: &Head<'_>) -> (Ty, Gen, Vec<Generic>) {
        let self_ty = self.names.pool.intern_ty(&TyData::Param(ParamRef {
            owner: h.def,
            index: 0,
        }));
        let mut gn = Gen {
            self_ty: Some(self_ty),
            self_trait: Some(h.def),
            ..Gen::default()
        };
        let gl = Src::child(h.node, SyntaxKind::GenericParameterList);
        let generics = self.generics(gl, h.def, 1, &mut gn);
        (self_ty, gn, generics)
    }

    /// A trait declaration's supertrait list, as trait-value types over
    /// its `Self`.
    fn supers(&mut self, n: NodeRef<'_>, gn: &Gen, self_ty: Ty) -> Vec<Ty> {
        let mut supers = Vec::new();
        if let Some(bl) = Src::child(n, SyntaxKind::BoundList) {
            for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                if let Some(t) = self.trait_value(b, gn, Some(self_ty)) {
                    supers.push(t);
                }
            }
            self.bound_clashes(self_ty, &supers, self.src.span(bl));
        }
        supers
    }

    /// The declarations of the associated type `name` that `tr[args]`
    /// declares or reaches through its supertraits
    /// (`assoc::assoc_decls`).
    fn assoc_decls(
        &self,
        tr: DefId,
        args: TyList,
        self_ty: Option<Ty>,
        name: Symbol,
    ) -> Vec<(DefId, TyList, DefId)> {
        let pool = self.names.pool.types();
        crate::assoc::assoc_decls(pool, self.r, tr, args, self_ty, self.names.text(name))
    }

    fn item(&mut self, h: &Head<'_>, out: &mut Vec<Item>) {
        let n = h.node;
        // The local declarations in scope are those visible where this one
        // is declared, itself included (`names.local-type.refs`).
        self.at = self.src.span(n).lo;
        self.outer = h
            .local
            .map(|l| enclosing_generics(&self.src, l.top, self.at))
            .unwrap_or_default();
        let block = Src::child(n, SyntaxKind::Block);
        let gl = Src::child(n, SyntaxKind::GenericParameterList);
        match h.kind {
            HeadKind::Fn => {
                self.result_required(n, h.public);
                let sig = self.sig(n, h.def, &Gen::default());
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Fn(sig));
                it.intrinsic = self.intrinsic(n);
                it.literal_fn = self.literal_fn(n);
                it.has_facts = !crate::facts::fact_lines(&self.src, n).is_empty();
                out.push(it);
            }
            HeadKind::Data => {
                let mut gn = Gen::default();
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let fields = self.fields(block, &gn);
                if self.r.frozen.is_none() {
                    self.field_variance(h.def, &generics, block, &fields);
                }
                self.derived(h, &generics, out);
                self.error_impls(h, &generics, &[fields.as_slice()], out);
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Data(fields));
                it.generics = generics;
                it.targets = self.annotate_mask(n);
                it.typed_fact = self.annotate_typed(n);
                out.push(it);
            }
            HeadKind::Enum => {
                let mut gn = Gen::default();
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let shared_params = Src::child(n, SyntaxKind::ParameterList);
                let nodes: Vec<NodeRef<'_>> = shared_params
                    .iter()
                    .flat_map(|pl| pl.children())
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .collect();
                self.parameter_default_order(&nodes, false);
                let shared = self.fields(shared_params, &gn);
                let check = self.r.frozen.is_none();
                if check {
                    let holder = Src::child(n, SyntaxKind::ParameterList);
                    self.field_variance(h.def, &generics, holder, &shared);
                }
                let mut variants = Vec::new();
                for v in block
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::EnumVariant)
                {
                    let Some(t) = v.name(&self.src.parse.tokens) else {
                        continue;
                    };
                    let text = self.src.text(t).to_owned();
                    let fields = self.fields(Src::child(v, SyntaxKind::ParameterList), &gn);
                    if check {
                        let holder = Src::child(v, SyntaxKind::ParameterList);
                        self.field_variance(h.def, &generics, holder, &fields);
                    }
                    variants.push(Variant {
                        name: self.sym(&text),
                        def: self.names.member(h.def, PathKind::Variant, &text),
                        fields,
                    });
                }
                self.derived(h, &generics, out);
                let payloads: Vec<&[Field]> =
                    variants.iter().map(|v| v.fields.as_slice()).collect();
                self.error_impls(h, &generics, &payloads, out);
                let mut it =
                    Item::new(h.def, h.name, h.public, ItemData::Enum { shared, variants });
                it.generics = generics;
                it.targets = self.annotate_mask(n);
                it.typed_fact = self.annotate_typed(n);
                out.push(it);
            }
            HeadKind::Trait => {
                let (self_ty, gn, generics) = self.trait_generics(h);
                // A trait's parameters are invariant (types.variance.trait-params).
                if self.r.frozen.is_none()
                    && let Some(gl) = gl
                {
                    for (g, node) in generics.iter().zip(
                        gl.children()
                            .filter(|c| c.kind() == SyntaxKind::GenericParameter),
                    ) {
                        if g.variance != 0 {
                            let msg = format!(
                                "trait parameter `{}` is invariant and takes no marker",
                                self.names.text(g.name)
                            );
                            self.diags
                                .error(Code::InvalidVariance, self.src.span(node), &msg);
                        }
                    }
                }
                let supers = self.supers(n, &gn, self_ty);
                self.duplicate_members(block);
                let (methods, assoc_decls) = self.members(block, h.def, h.public, &gn, out);
                let mut assoc = Vec::new();
                for (name, def, ty) in assoc_decls {
                    out.push(Item::new(
                        def,
                        name,
                        h.public,
                        ItemData::AssocType {
                            owner: h.def,
                            bounds: Vec::new(),
                            default: ty,
                        },
                    ));
                    assoc.push((name, def));
                }
                let mut it = Item::new(
                    h.def,
                    h.name,
                    h.public,
                    ItemData::Trait(TraitData {
                        methods,
                        assoc,
                        supers,
                    }),
                );
                it.generics = generics;
                out.push(it);
            }
            HeadKind::Impl => self.impl_item(h, out),
            HeadKind::Alias => self.alias_item(h, false, out),
            HeadKind::Newtype => {
                let mut gn = Gen::default();
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let t = self.ty(n.children().find(|c| c.kind().is_type()), &gn);
                // `@derive` on a newtype names implementations from the
                // base type's (`trait.derive.newtype`).
                self.derived(h, &generics, out);
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Newtype(t));
                it.generics = generics;
                out.push(it);
            }
        }
    }

    /// A transparent alias (`types.alias.same`), or a row alias, whose
    /// right side is a row after `$` (`req.row.alias.dollar`), stored
    /// expanded (`req.row.alias.expand.first`). A member of an alias
    /// cycle keeps a poisoned body (`types.alias.cycle`).
    fn alias_item(&mut self, h: &Head<'_>, cyclic: bool, out: &mut Vec<Item>) {
        let n = h.node;
        self.at = self.src.span(n).lo;
        self.outer = h
            .local
            .map(|l| enclosing_generics(&self.src, l.top, self.at))
            .unwrap_or_default();
        let mut gn = Gen::default();
        let gl = Src::child(n, SyntaxKind::GenericParameterList);
        let generics = self.generics(gl, h.def, 0, &mut gn);
        let body = n.children().find(|c| c.kind().is_type());
        let t = match body {
            _ if cyclic => Ty::POISON,
            Some(b) if b.kind() == SyntaxKind::RequirementRow => {
                if b.direct_token(&self.src.parse.tokens, TokenKind::Dollar)
                    .is_some()
                {
                    let r = self.row(Some(b), &gn);
                    self.names.pool.intern_ty(&TyData::Row(r))
                } else {
                    // `req.row.alias.dollar.missing`
                    let msg = format!(
                        "a row alias's right side is a row written after `$`: write `type {} = $ ...`",
                        self.names.text(h.name)
                    );
                    self.diags
                        .error(Code::GenericKindMismatch, self.src.span(b), &msg);
                    Ty::POISON
                }
            }
            b => self.ty(b, &gn),
        };
        let rows = generics.iter().map(|g| g.row).collect();
        self.r.aliases.borrow_mut().insert(h.def, (rows, t));
        let mut it = Item::new(h.def, h.name, h.public, ItemData::Alias(t));
        it.generics = generics;
        out.push(it);
    }

    /// The folder aliases (by index in `index`) that the declaration `n`
    /// names, other than through its own generic parameters.
    fn named_aliases(&mut self, n: NodeRef<'_>, index: &HashMap<DefId, usize>) -> Vec<usize> {
        let params: Vec<&str> = Src::child(n, SyntaxKind::GenericParameterList)
            .iter()
            .flat_map(|gl| gl.children())
            .filter(|g| g.kind() == SyntaxKind::GenericParameter)
            .filter_map(|g| g.name(&self.src.parse.tokens))
            .map(|t| self.src.text(t))
            .collect();
        let mut out = Vec::new();
        for c in n
            .descendants()
            .filter(|c| c.kind() == SyntaxKind::NamedType)
        {
            let segs = self.segments(c);
            let Some((first, _)) = segs.first() else {
                continue;
            };
            if (segs.len() == 1 && params.contains(&first.as_str())) || self.is_poisoned(first) {
                continue;
            }
            if let Some((d, HeadKind::Alias)) = self.resolve_path(&segs, self.src.span(c))
                && let Some(&i) = index.get(&d)
            {
                out.push(i);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `$.Context[$ Row]` (`req.context.row`): a context type indexed by
    /// its row, which expands like any row (`req.row.alias.expand.first`).
    fn context_ty(&mut self, n: NodeRef<'_>, gn: &Gen) -> Ty {
        let Some(c) = n.children().find(|c| c.kind().is_type()) else {
            return Ty::POISON;
        };
        let has_dollar = c.kind() == SyntaxKind::RequirementRow
            && c.direct_token(&self.src.parse.tokens, TokenKind::Dollar)
                .is_some();
        if !has_dollar {
            // The brackets are a row slot (`req.context.dollar`).
            let msg = format!(
                "`$.Context[...]` takes a row written after `$`, as in `$.Context[$ {}]`",
                self.src.text(self.src.first(c))
            );
            self.diags
                .error(Code::GenericKindMismatch, self.src.span(c), &msg);
            return Ty::POISON;
        }
        let row = self.row(Some(c), gn);
        let pool = self.names.pool;
        if !pool.row_data(row).params.is_empty() {
            self.diags.error(
                Code::RowParameterInContext,
                self.src.span(c),
                "`$.Context[...]` takes only a concrete row, not a row parameter",
            );
            return Ty::POISON;
        }
        pool.intern_ty(&TyData::Context(row))
    }
}

/// Lowers every alias of the folder before any other header, each after
/// the aliases its declaration names, whatever the declaration order
/// (resolution-and-interfaces.md, "Row aliases and context types in
/// interfaces"). An alias that expands to itself is `alias-cycle`,
/// reported once per cycle, on its declaration that comes first in the
/// source (`types.alias.cycle.reported`). Returns each module's alias
/// items and the first unsupported form met.
fn lower_aliases(
    r: &Resolver<'_, '_>,
    mods: &[ModIn<'_>],
    all_heads: &[Vec<Head<'_>>],
    scopes: &mut [ModScopes],
    diags: &mut DiagBuf,
) -> (Vec<Vec<Item>>, Option<String>) {
    let mut sites: Vec<(usize, &Head<'_>)> = Vec::new();
    for (mi, hs) in all_heads.iter().enumerate() {
        sites.extend(
            hs.iter()
                .filter(|h| h.kind == HeadKind::Alias)
                .map(|h| (mi, h)),
        );
    }
    let index: HashMap<DefId, usize> = sites
        .iter()
        .enumerate()
        .map(|(i, (_, h))| (h.def, i))
        .collect();
    let locals: Vec<Vec<LocalItem>> = all_heads.iter().map(|hs| local_items(hs)).collect();
    let mut scratch = DiagBuf::default();
    let edges: Vec<Vec<usize>> = sites
        .iter()
        .map(|&(mi, h)| {
            let s = &scopes[mi];
            let scope = match (h.test, &s.tests) {
                (Some(_), Some(tests)) => tests,
                _ => &s.scope,
            };
            let mut low = Lower {
                r,
                names: r.cx.names,
                src: mods[mi].src,
                module: &mods[mi].path,
                scope,
                kinds: &s.kinds,
                diags: &mut scratch,
                unsupported: None,
                locals: &locals[mi],
                at: mods[mi].src.span(h.node).lo,
                outer: Vec::new(),
            };
            low.named_aliases(h.node, &index)
        })
        .collect();
    let mut out: Vec<Vec<Item>> = (0..mods.len()).map(|_| Vec::new()).collect();
    let mut unsupported = None;
    for comp in components(&edges) {
        let cyclic = comp.len() > 1 || edges[comp[0]].contains(&comp[0]);
        let first = comp.iter().copied().min().unwrap_or(0);
        for &i in &comp {
            let (mi, h) = sites[i];
            let ModScopes {
                scope,
                kinds,
                tests,
                test_diags,
            } = &mut scopes[mi];
            let (scope, diags): (&ModuleScope, &mut DiagBuf) = match (h.test, tests.as_ref()) {
                (Some(_), Some(tests)) => (tests, test_diags),
                _ => (&*scope, &mut *diags),
            };
            let src = mods[mi].src;
            if cyclic && i == first {
                let names = r.cx.names;
                let others: Vec<String> = comp
                    .iter()
                    .filter(|&&j| j != i)
                    .map(|&j| format!("`{}`", names.text(sites[j].1.name)))
                    .collect();
                let msg = if others.is_empty() {
                    format!("`{}` expands to itself", names.text(h.name))
                } else {
                    format!(
                        "`{}` expands to itself through {}",
                        names.text(h.name),
                        others.join(", ")
                    )
                };
                diags.error(Code::AliasCycle, src.span(h.node), &msg);
            }
            let mut low = Lower {
                r,
                names: r.cx.names,
                src,
                module: &mods[mi].path,
                scope,
                kinds: &*kinds,
                diags,
                unsupported: None,
                locals: &locals[mi],
                at: 0,
                outer: Vec::new(),
            };
            low.alias_item(h, cyclic, &mut out[mi]);
            if unsupported.is_none() {
                unsupported = low.unsupported.take();
            }
        }
    }
    (out, unsupported)
}

/// The written head of an implementation, as `impl_head_rules` reads it.
struct ImplHead<'t> {
    has_trait: bool,
    by: bool,
    target: NodeRef<'t>,
    trait_args: TyList,
    self_ty: Ty,
}

/// Whether `pred` holds of `t` or of any type inside it.
fn ty_any(pool: &InternPool, t: Ty, pred: &mut dyn FnMut(TyData) -> bool) -> bool {
    let d = pool.get(t);
    if pred(d.clone()) {
        return true;
    }
    let mut kids: Vec<Ty> = Vec::new();
    match d {
        TyData::Adt { args, .. } => kids.extend(pool.list_items(args)),
        TyData::Tuple { elems, rest } => {
            kids.extend(pool.list_items(elems));
            kids.extend(rest);
        }
        TyData::Option(i) | TyData::Mut(i) => kids.push(i),
        TyData::Fn {
            params,
            result,
            row,
            ..
        } => {
            kids.extend(pool.list_items(params));
            kids.push(result);
            kids.extend(pool.row_data(row).keys.iter());
        }
        TyData::TraitValue { args, bindings, .. } => {
            kids.extend(pool.list_items(args));
            kids.extend(bindings.iter().map(|b| b.1));
        }
        TyData::Assoc { self_ty, args, .. } => {
            kids.push(self_ty);
            kids.extend(pool.list_items(args));
        }
        TyData::Row(r) | TyData::Context(r) => kids.extend(pool.row_data(r).keys.iter()),
        _ => {}
    }
    kids.into_iter().any(|k| ty_any(pool, k, pred))
}

/// `trait.super.acyclic`, `trait.super.cycle-report`: the supertrait graph
/// of the folder's traits has no cycle. A cycle is one error, on its trait
/// that comes first by module and then by source position.
fn supertrait_cycles(
    r: &Resolver<'_, '_>,
    mods: &[ModIn<'_>],
    all_heads: &[Vec<Head<'_>>],
    diags: &mut DiagBuf,
) {
    let mut sites: Vec<(usize, &Head<'_>)> = Vec::new();
    for (mi, hs) in all_heads.iter().enumerate() {
        sites.extend(
            hs.iter()
                .filter(|h| h.kind == HeadKind::Trait)
                .map(|h| (mi, h)),
        );
    }
    let index: HashMap<DefId, usize> = sites
        .iter()
        .enumerate()
        .map(|(i, (_, h))| (h.def, i))
        .collect();
    let pool = r.cx.names.pool;
    let edges: Vec<Vec<usize>> = sites
        .iter()
        .map(|(_, h)| {
            r.own_supers
                .get(&h.def)
                .into_iter()
                .flatten()
                .filter_map(|t| match pool.get(*t) {
                    TyData::TraitValue { def, .. } => index.get(&def).copied(),
                    _ => None,
                })
                .collect()
        })
        .collect();
    for comp in components(&edges) {
        if comp.len() < 2 && !edges[comp[0]].contains(&comp[0]) {
            continue;
        }
        let first = comp.iter().copied().min().unwrap_or(0);
        let (mi, h) = sites[first];
        let names = r.cx.names;
        let others: Vec<String> = comp
            .iter()
            .filter(|&&j| j != first)
            .map(|&j| format!("`{}`", names.text(sites[j].1.name)))
            .collect();
        let msg = if others.is_empty() {
            format!("`{}` is its own supertrait", names.text(h.name))
        } else {
            format!(
                "`{}` is its own supertrait through {}",
                names.text(h.name),
                others.join(", ")
            )
        };
        diags.error(Code::SupertraitCycle, mods[mi].src.span(h.node), &msg);
    }
}

/// The strongly connected components of a graph (Tarjan's algorithm),
/// each after every component it reaches: dependencies first. Members of
/// a component are in index order.
fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    struct St<'e> {
        edges: &'e [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn visit(st: &mut St<'_>, v: usize) {
        st.index[v] = Some(st.next);
        st.low[v] = st.next;
        st.next += 1;
        st.stack.push(v);
        st.on[v] = true;
        for &w in &st.edges[v] {
            match st.index[w] {
                None => {
                    visit(st, w);
                    st.low[v] = st.low[v].min(st.low[w]);
                }
                Some(iw) if st.on[w] => st.low[v] = st.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(st.low[v]) == st.index[v] {
            let mut comp = Vec::new();
            while let Some(w) = st.stack.pop() {
                st.on[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            comp.sort_unstable();
            st.out.push(comp);
        }
    }
    let n = edges.len();
    let mut st = St {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if st.index[v].is_none() {
            visit(&mut st, v);
        }
    }
    st.out
}

/// The placement error of one item, if it is an impl outside its owning
/// module: `OrphanImpl` when no owner is in the impl's package,
/// `NonlocalImpl` otherwise. `None` for everything else. Coherence calls
/// this too, to leave rejected impls out of the overlap check.
#[must_use]
pub fn misplaced_impl(names: &Names<'_>, module: &str, it: &Item) -> Option<Code> {
    let pool = names.pool;
    let package = module.split('.').next().unwrap_or("");
    if package == "std" {
        return None;
    }
    let ctor_module = |t: Ty| -> Option<String> {
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        match pool.get(t) {
            TyData::Adt { def, .. } | TyData::TraitValue { def, .. } => Some(names.module_of(def)),
            TyData::Prim(_) | TyData::Tuple { .. } | TyData::Option(_) | TyData::Fn { .. } => {
                Some("std.core".to_owned())
            }
            _ => None,
        }
    };
    let ItemData::Impl {
        trait_,
        trait_args,
        self_ty,
        kind,
        ..
    } = &it.data
    else {
        return None;
    };
    if *trait_ == DefId::NONE || !kind.is_impl() {
        return None;
    }
    let mut owners = vec![names.module_of(*trait_)];
    owners.extend(ctor_module(*self_ty));
    owners.extend(
        pool.list_items(*trait_args)
            .iter()
            .copied()
            .filter_map(ctor_module),
    );
    if owners.iter().any(|o| o == module) {
        return None;
    }
    let local = owners.iter().any(|o| o.split('.').next() == Some(package));
    Some(if local {
        Code::NonlocalImpl
    } else {
        Code::OrphanImpl
    })
}

/// Whether a syntax node is `impl X by Structure`, a derivation block with
/// no trait (`annot.no-trait.form`).
fn traitless_block(src: &Src<'_>, n: NodeRef<'_>) -> bool {
    if n.kind() != SyntaxKind::ImplDecl || n.children().filter(|c| c.kind().is_type()).count() != 1
    {
        return false;
    }
    let words: Vec<&str> = n.direct_tokens().map(|t| src.text(t)).collect();
    words
        .iter()
        .position(|w| *w == "by")
        .and_then(|i| words.get(i + 1))
        .is_some_and(|w| *w == "Structure")
}

/// Whether an item is a template whose trait is declared in another module
/// (`annot.template.module`).
#[must_use]
pub fn misplaced_template(names: &Names<'_>, module: &str, it: &Item) -> bool {
    matches!(
        &it.data,
        ItemData::Impl { trait_, kind: ImplKind::Template | ImplKind::TupleTemplate, .. }
            if *trait_ != DefId::NONE && names.module_of(*trait_) != module
    )
}

/// One module's items by `DefId`, and its impls by the type they target,
/// built once per module so the per-item checks look items up instead of
/// scanning them (#118). Lists keep item order, so every report keeps its
/// order.
pub(crate) struct ItemIndex {
    by_def: HashMap<DefId, usize>,
    by_target: HashMap<DefId, Vec<usize>>,
}

impl ItemIndex {
    pub(crate) fn new(names: &Names<'_>, items: &[Item]) -> Self {
        let mut by_def = HashMap::with_capacity(items.len());
        let mut by_target: HashMap<DefId, Vec<usize>> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            by_def.entry(it.def).or_insert(i);
            if let ItemData::Impl { self_ty, .. } = &it.data
                && let TyData::Adt { def, .. } = names.pool.get(*self_ty)
            {
                by_target.entry(def).or_default().push(i);
            }
        }
        ItemIndex { by_def, by_target }
    }

    /// The item `d`, the first with that `DefId` as a scan would find it.
    pub(crate) fn get<'a>(&self, items: &'a [Item], d: DefId) -> Option<&'a Item> {
        self.by_def.get(&d).and_then(|&i| items.get(i))
    }

    /// The impls whose target is the data type or enum `d`, in item order.
    pub(crate) fn impls_for<'a>(
        &self,
        items: &'a [Item],
        d: DefId,
    ) -> impl Iterator<Item = &'a Item> {
        self.by_target
            .get(&d)
            .into_iter()
            .flatten()
            .filter_map(|&i| items.get(i))
    }
}

/// Why a derivation block is rejected, if it is: it is declared outside the
/// module of its target (`annot.block.module`, `annot.no-trait.module`),
/// targets a newtype (`annot.block.newtype.error`) or, with no trait, any
/// target but a data type or enum with an unbounded header
/// (`annot.no-trait.target`, `annot.no-trait.generic.error`). `items` are
/// the items of `module`. Rejected blocks stay out of the derivation checks.
#[must_use]
pub fn misplaced_block(
    names: &Names<'_>,
    module: &str,
    items: &[Item],
    it: &Item,
) -> Option<&'static str> {
    misplaced_block_in(names, module, &|d| items.iter().find(|i| i.def == d), it)
}

/// `misplaced_block`, with `item` finding an item of `module` by `DefId`.
fn misplaced_block_in<'a>(
    names: &Names<'_>,
    module: &str,
    item: &dyn Fn(DefId) -> Option<&'a Item>,
    it: &Item,
) -> Option<&'static str> {
    let pool = names.pool;
    let ItemData::Impl {
        trait_,
        self_ty,
        by,
        ..
    } = &it.data
    else {
        return None;
    };
    let traitless = *trait_ == DefId::NONE && by.is_some_and(|b| names.text(b) == "Structure");
    if !derivation_block(names, it) {
        return None;
    }
    let TyData::Adt { def, args } = pool.get(*self_ty) else {
        return traitless.then_some("a derivation block with no trait targets a data type or enum");
    };
    if names.module_of(def) != module {
        return Some("a derivation block belongs in the module that declares its target");
    }
    let target = item(def)?;
    match &target.data {
        ItemData::Newtype(_) => {
            return Some("a derivation block cannot target a newtype; derive it with `@derive`");
        }
        ItemData::Data(_) | ItemData::Enum { .. } => {}
        _ if traitless => {
            return Some("a derivation block with no trait targets a data type or enum");
        }
        _ => {}
    }
    if traitless {
        let own = pool.list_items(args);
        let plain = it.generics.len() == target.generics.len()
            && own.len() == it.generics.len()
            && it.generics.iter().enumerate().all(|(i, g)| {
                let index = u16::try_from(i).unwrap_or(u16::MAX);
                g.bound.is_none()
                    && g.bounds.is_empty()
                    && !g.mut_bound
                    && g.default.is_none()
                    && own.get(i).is_some_and(|a| {
                        pool.get(*a)
                            == TyData::Param(ParamRef {
                                owner: it.def,
                                index,
                            })
                    })
            });
        if !plain {
            return Some(
                "a derivation block with no trait declares the target's own parameters, without bounds",
            );
        }
    }
    None
}

/// One decorator line as its name and its argument texts (`@derive(Eq, Hash)`
/// gives `derive` and `Eq`, `Hash`). `None` for a form with no name.
fn decorator_line(src: &Src<'_>, d: NodeRef<'_>) -> Option<(String, Vec<String>)> {
    let e = d.children().next()?;
    let (name_node, args) = match e.kind() {
        SyntaxKind::CallExpr => (e.children().next(), Src::child(e, SyntaxKind::ArgumentList)),
        _ => (Some(e), None),
    };
    let name = name_node.and_then(|x| src.first_ident(x))?;
    let mut list = Vec::new();
    for a in args.iter().flat_map(|l| l.children()) {
        let text: String = src.tokens(a).map(|t| src.text(t)).collect();
        list.push(text.trim_matches('"').to_owned());
    }
    Some((src.text(name).to_owned(), list))
}

/// The traits that a derived trait needs in the same `@derive` list
/// (`trait.derive.related.same-list`): `Hash` and `PartialOrd` need `Eq`, and
/// `Ord` needs `Eq` and `PartialOrd`.
fn needed_traits(known: &KnownItems, tr: DefId) -> Vec<DefId> {
    if tr == known.hash || tr == known.partial_ord {
        vec![known.eq]
    } else if tr == known.ord {
        vec![known.eq, known.partial_ord]
    } else {
        Vec::new()
    }
}

/// Whether two traits are related (`trait.derive.related`): `Hash`,
/// `PartialOrd` and `Ord` each relate to `Eq`, and `Ord` also to `PartialOrd`.
fn related_traits(known: &KnownItems, a: DefId, b: DefId) -> bool {
    let pair = |x: DefId, y: DefId| (a == x && b == y) || (a == y && b == x);
    pair(known.hash, known.eq)
        || pair(known.partial_ord, known.eq)
        || pair(known.ord, known.eq)
        || pair(known.ord, known.partial_ord)
}

/// The related-trait rules of derivation (`trait.derive.related.same-list`,
/// `trait.derive.related.no-mix`), each `@derive` line reported once. A line
/// breaks a rule when it lacks a needed trait, or when one of its traits is
/// related to a trait the module hand-writes for the same type. A line with
/// an unresolved name is skipped: its name already has an error.
fn related_derives(
    names: &Names<'_>,
    items: &[Item],
    index: &ItemIndex,
    heads: &[Head<'_>],
    scope: &ModuleScope,
    src: &Src<'_>,
    diags: &mut DiagBuf,
) {
    let known = names.known;
    for h in heads {
        let written: Vec<DefId> = index
            .impls_for(items, h.def)
            .filter_map(|i| match &i.data {
                ItemData::Impl {
                    trait_,
                    kind: ImplKind::Written | ImplKind::Delegated,
                    ..
                } if *trait_ != DefId::NONE => Some(*trait_),
                _ => None,
            })
            .collect();
        for d in h
            .node
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
        {
            let Some((name, args)) = decorator_line(src, d) else {
                continue;
            };
            if name != "derive" {
                continue;
            }
            let traits: Option<Vec<DefId>> = args
                .iter()
                .map(|a| match scope.lookup(names.syms.intern(a)) {
                    Some(Binding {
                        kind: BindingKind::Item,
                        value,
                    }) => Some(DefId::from_raw(value)),
                    _ => None,
                })
                .collect();
            let Some(traits) = traits else {
                continue;
            };
            let short = traits
                .iter()
                .any(|&t| needed_traits(known, t).iter().any(|n| !traits.contains(n)));
            let mixed = traits
                .iter()
                .any(|&t| written.iter().any(|&w| related_traits(known, t, w)));
            if short || mixed {
                diags.error(
                    Code::MixedDerivedLaw,
                    src.span(d),
                    "`@derive` must list the traits a derived trait needs, and cannot mix with hand-written related traits",
                );
            }
        }
    }
}

/// Whether an impl item is a derivation block: `by Structure` on a trait, or
/// with no trait (`annot.no-trait.form`). This is the one test for a block.
pub(crate) fn derivation_block(names: &Names<'_>, it: &Item) -> bool {
    let ItemData::Impl {
        trait_, kind, by, ..
    } = &it.data
    else {
        return false;
    };
    let traitless = *trait_ == DefId::NONE && by.is_some_and(|b| names.text(b) == "Structure");
    traitless || *kind == ImplKind::Derivation
}

/// The syntax nodes of the derivation blocks among a module's heads.
fn derivation_nodes(
    names: &Names<'_>,
    items: &[Item],
    index: &ItemIndex,
    heads: &[Head<'_>],
) -> Vec<hd_base::NodeIdx> {
    heads
        .iter()
        .filter(|h| h.kind == HeadKind::Impl)
        .filter(|h| {
            index
                .get(items, h.def)
                .is_some_and(|it| derivation_block(names, it))
        })
        .map(|h| h.node.index())
        .collect()
}

/// The placement rules of derivation syntax (`annot.template.module`,
/// `annot.block.module`, `annot.block.newtype.error`,
/// `annot.line.placement-blocks`, `annot.no-trait.*`), each reported once on
/// the block or member.
fn placement(
    names: &Names<'_>,
    module: &str,
    items: &[Item],
    index: &ItemIndex,
    heads: &[Head<'_>],
    src: &Src<'_>,
    diags: &mut DiagBuf,
) {
    for h in heads.iter().filter(|h| h.kind == HeadKind::Impl) {
        let Some(it) = index.get(items, h.def) else {
            continue;
        };
        let ItemData::Impl { trait_, by, .. } = &it.data else {
            continue;
        };
        let msg = if misplaced_template(names, module, it) {
            Some("a template belongs in the module that declares its trait")
        } else {
            misplaced_block_in(names, module, &|d| index.get(items, d), it)
        };
        if let Some(msg) = msg {
            diags.error(Code::MisplacedDerivation, src.span(h.node), msg);
        }
        let traitless = *trait_ == DefId::NONE && by.is_some_and(|b| names.text(b) == "Structure");
        let block = derivation_block(names, it);
        for m in Src::child(h.node, SyntaxKind::Block)
            .into_iter()
            .flat_map(NodeRef::children)
        {
            let what = match m.kind() {
                SyntaxKind::Derivation if !block => "a member line belongs in a derivation block",
                SyntaxKind::FnDecl | SyntaxKind::AssociatedTypeDecl if traitless => {
                    "a derivation block with no trait holds only member lines"
                }
                _ => continue,
            };
            diags.error(Code::MisplacedDerivation, src.span(m), what);
        }
    }
    // A derivation block with no trait in a local scope
    // (`annot.no-trait.local`): any such impl not at the module's top level.
    let top: Vec<_> = src.root().children().map(NodeRef::index).collect();
    for n in src.root().descendants() {
        if traitless_block(src, n) && !top.contains(&n.index()) {
            diags.error(
                Code::MisplacedDerivation,
                src.span(n),
                "a derivation block with no trait cannot be local",
            );
        }
    }
}

/// The owning-module rule (`trait.impl.module`, §4.9 "Orphan and coherence
/// inputs"): an impl lives in the module of its trait, of its target's
/// outer constructor, or of a trait argument's outer constructor. std may
/// implement for built-in targets in any of its modules. Reports every
/// misplaced impl of one module (see `misplaced_impl`).
fn ownership(
    names: &Names<'_>,
    module: &str,
    items: &[Item],
    heads: &[Head<'_>],
    src: &Src<'_>,
    diags: &mut DiagBuf,
) {
    let head_of: HashMap<DefId, &Head<'_>> = heads.iter().rev().map(|h| (h.def, h)).collect();
    for it in items {
        let Some(code) = misplaced_impl(names, module, it) else {
            continue;
        };
        let span = head_of
            .get(&it.def)
            .map_or(src.span(src.root()), |h| src.span(h.node));
        let what = if code == Code::NonlocalImpl {
            "nonlocal-impl"
        } else {
            "orphan-impl"
        };
        let msg = format!(
            "{what}: `{}` belongs in the module of its trait or target",
            names.display_name(it.def)
        );
        diags.error(code, span, &msg);
    }
}

/// Resolves and lowers the modules of one folder (§4.10 steps 1 to 4).
/// Without `frozen`, `mods` must be every module of the folder: sibling
/// uses resolve through their heads, and the folder's exports are
/// computed. With `frozen`, sibling uses read its export index, and `mods`
/// may be any subset.
pub fn build_folder(
    cx: &Cx<'_>,
    mods: &[ModIn<'_>],
    frozen: Option<Arc<FolderIface>>,
    stage: Stage,
    diags: &mut DiagBuf,
) -> StageResult<FolderOut> {
    let names = &cx.names;
    let all_heads: Vec<Vec<Head<'_>>> = mods
        .iter()
        .map(|m| heads(names, &m.src, &m.path, m.tests))
        .collect();
    let all_uses: Vec<Vec<UseDecl>> = mods.iter().map(|m| use_decls(&m.src, &m.roots)).collect();
    let mut r = Resolver {
        cx,
        frozen,
        own: HashMap::new(),
        pub_uses: HashMap::new(),
        memo: RefCell::new(HashMap::new()),
        visiting: RefCell::new(HashSet::new()),
        trait_assoc: HashMap::new(),
        variances: HashMap::new(),
        aliases: RefCell::new(HashMap::new()),
        seeds: HashMap::new(),
        own_traits: HashMap::new(),
        own_supers: HashMap::new(),
    };
    for (m, hs) in mods.iter().zip(&all_heads) {
        for h in hs {
            if h.kind == HeadKind::Impl {
                continue;
            }
            // A local declaration is no module member (`names.local-type.static`),
            // and no other module sees a test item (`names.tests.inside-only`).
            if h.local.is_none() && h.test.is_none() {
                r.own
                    .entry((m.path.clone(), h.name))
                    .or_insert((h.def, h.kind, h.public));
            }
            if let Some(gl) = Src::child(h.node, SyntaxKind::GenericParameterList) {
                let toks = &m.src.parse.tokens;
                let vs = gl
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::GenericParameter)
                    .map(|g| {
                        if g.direct_token(toks, TokenKind::Plus).is_some() {
                            1
                        } else if g.direct_token(toks, TokenKind::Minus).is_some() {
                            -1
                        } else {
                            0
                        }
                    })
                    .collect();
                r.variances.insert(h.def, vs);
            }
            if h.kind == HeadKind::Trait {
                let assoc = Src::child(h.node, SyntaxKind::Block)
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::AssociatedTypeDecl)
                    .filter_map(|c| c.name(&m.src.parse.tokens))
                    .map(|t| names.syms.intern(m.src.text(t)))
                    .collect();
                r.trait_assoc.insert(h.def, assoc);
            }
        }
        for s in &m.seeds {
            if let Some(k) = s.kind() {
                r.own
                    .entry((m.path.clone(), s.name))
                    .or_insert((s.def, k, s.public));
                r.seeds.insert(s.def, s.clone());
                if let (ItemData::Alias(t), HeadKind::Alias) = (&s.data, k) {
                    let rows = s.generics.iter().map(|g| g.row).collect();
                    r.aliases.borrow_mut().insert(s.def, (rows, *t));
                }
                if let ItemData::Trait(t) = &s.data {
                    r.trait_assoc
                        .insert(s.def, t.assoc.iter().map(|a| a.0).collect());
                }
            }
        }
    }
    for (m, us) in mods.iter().zip(&all_uses) {
        for u in us.iter().filter(|u| u.public) {
            let module = u.module();
            for g in u.group.iter().flatten() {
                let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
                r.pub_uses.insert(
                    (m.path.clone(), local),
                    (module.clone(), names.syms.intern(&g.name)),
                );
            }
        }
    }
    let mut out = FolderOut {
        modules: Vec::new(),
        exports: Vec::new(),
        private_names: Vec::new(),
    };
    let mut scopes: Vec<ModScopes> = mods
        .iter()
        .zip(&all_heads)
        .zip(&all_uses)
        .map(|((m, hs), us)| {
            let (scope, mut kinds) = scope_of(&r, m, hs, us, diags);
            let mut test_diags = DiagBuf::default();
            let tests = m.tests.then(|| {
                let uses = test_use_decls(&m.src, &m.roots);
                test_scope_of(&r, &m.src, hs, (&scope, &mut kinds), &uses, &mut test_diags)
            });
            ModScopes {
                scope,
                kinds,
                tests,
                test_diags,
            }
        })
        .collect();
    r.own_traits = own_trait_generics(&r, mods, &all_heads, &scopes);
    // Twice: the first round finds each supertrait; the second resolves
    // the bindings that name a supertrait's supertrait's associated type.
    r.own_supers = own_trait_supers(&r, mods, &all_heads, &scopes);
    r.own_supers = own_trait_supers(&r, mods, &all_heads, &scopes);
    let (alias_items, alias_gap) = lower_aliases(&r, mods, &all_heads, &mut scopes, diags);
    let mut unsupported = alias_gap;
    for (((m, hs), s), mut items) in mods.iter().zip(&all_heads).zip(scopes).zip(alias_items) {
        let ModScopes {
            scope,
            kinds,
            tests,
            mut test_diags,
        } = s;
        {
            let locals = local_items(hs);
            // A test item's header resolves in its block's scope, and its
            // diagnostics are test code's.
            let parts = [
                (Some(&scope), &mut *diags, false),
                (tests.as_ref(), &mut test_diags, true),
            ];
            for (scope, diags, test) in parts {
                let Some(scope) = scope else { continue };
                let mut low = Lower {
                    r: &r,
                    names: cx.names,
                    src: m.src,
                    module: &m.path,
                    scope,
                    kinds: &kinds,
                    diags,
                    unsupported: None,
                    locals: &locals,
                    at: 0,
                    outer: Vec::new(),
                };
                // The aliases are lowered already (`lower_aliases`).
                for h in hs
                    .iter()
                    .filter(|h| h.test.is_some() == test && h.kind != HeadKind::Alias)
                {
                    low.item(h, &mut items);
                }
                if unsupported.is_none() {
                    unsupported = low.unsupported.take();
                }
            }
        }
        let mut have: HashSet<DefId> = items.iter().map(|i| i.def).collect();
        for s in &m.seeds {
            if have.insert(s.def) {
                items.push(s.clone());
            }
        }
        // A compiler-supplied member of a trait the module's source
        // declares (`Inspectable.downcast`) joins that trait's methods.
        for s in &m.seeds {
            let ItemData::Method { owner, .. } = &s.data else {
                continue;
            };
            if let Some(ItemData::Trait(t)) = items
                .iter_mut()
                .find(|i| i.def == *owner)
                .map(|i| &mut i.data)
                && !t.methods.iter().any(|(_, d)| *d == s.def)
            {
                t.methods.push((s.name, s.def));
            }
        }
        let index = ItemIndex::new(names, &items);
        ownership(names, &m.path, &items, hs, &m.src, diags);
        if r.frozen.is_none() {
            placement(names, &m.path, &items, &index, hs, &m.src, diags);
            related_derives(names, &items, &index, hs, &scope, &m.src, diags);
        }
        let anchors = crate::anchor::collect(names, &m.src, hs, &items, &index);
        let blocks = derivation_nodes(names, &items, &index, hs);
        out.modules.push(ModOut {
            items,
            scope,
            tests,
            test_diags,
            kinds,
            anchors,
            blocks,
        });
    }
    delegations(&r, &mut out.modules);
    if r.frozen.is_none() {
        let items: HashMap<DefId, &Item> = out
            .modules
            .iter()
            .flat_map(|m| m.items.iter())
            .map(|i| (i.def, i))
            .collect();
        supertrait_cycles(&r, mods, &all_heads, diags);
        for (m, o) in mods.iter().zip(&out.modules) {
            let mut low = Lower {
                r: &r,
                names: cx.names,
                src: m.src,
                module: &m.path,
                scope: &o.scope,
                kinds: &o.kinds,
                diags,
                unsupported: None,
                locals: &[],
                at: 0,
                outer: Vec::new(),
            };
            low.check_module_decorators(&items, &o.blocks);
        }
        let mut seen = HashSet::new();
        for (m, us) in mods.iter().zip(&all_uses) {
            let mut publics: Vec<Symbol> = r
                .own
                .iter()
                .filter(|((mp, _), (_, _, p))| mp == &m.path && *p)
                .map(|((_, n), _)| *n)
                .collect();
            for u in us.iter().filter(|u| u.public) {
                for g in u.group.iter().flatten() {
                    publics.push(names.syms.intern(g.alias.as_deref().unwrap_or(&g.name)));
                }
            }
            for name in publics {
                if !seen.insert((m.path.clone(), name)) {
                    continue;
                }
                match r.export(&m.path, name) {
                    Ok((def, kind)) => out.exports.push(Export {
                        module: m.path.clone(),
                        name,
                        def,
                        kind,
                    }),
                    Err(Code::ReExportLoop) => {
                        let span = us
                            .iter()
                            .find(|u| u.public)
                            .map_or(m.src.span(m.src.root()), |u| u.span);
                        let msg = format!("re-export loop through `{}`", names.text(name));
                        diags.error(Code::ReExportLoop, span, &msg);
                    }
                    Err(_) => {}
                }
            }
        }
        out.exports.sort_by(|a, b| {
            (a.module.as_str(), names.text(a.name)).cmp(&(b.module.as_str(), names.text(b.name)))
        });
        out.private_names = r
            .own
            .iter()
            .filter(|(_, (_, _, public))| !public)
            .map(|(key, _)| key.clone())
            .collect();
        out.private_names
            .sort_by(|a, b| (a.0.as_str(), names.text(a.1)).cmp(&(b.0.as_str(), names.text(b.1))));
    }
    match unsupported {
        Some(what) => Err(NotImplemented::new(stage, what)),
        None => Ok(out),
    }
}

/// The declared parameters of every trait of `mods`, by trait, so their
/// headers fill trait-argument defaults in any declaration order.
/// The main lowering reports their diagnostics, so these are dropped.
fn own_trait_generics(
    r: &Resolver<'_, '_>,
    mods: &[ModIn<'_>],
    all_heads: &[Vec<Head<'_>>],
    scopes: &[ModScopes],
) -> HashMap<DefId, Vec<Generic>> {
    let mut out = HashMap::new();
    let mut scratch = DiagBuf::default();
    for ((m, hs), s) in mods.iter().zip(all_heads).zip(scopes) {
        let locals = local_items(hs);
        for h in hs.iter().filter(|h| h.kind == HeadKind::Trait) {
            let scope = match (h.test, &s.tests) {
                (Some(_), Some(tests)) => tests,
                _ => &s.scope,
            };
            let mut low = Lower {
                r,
                names: r.cx.names,
                src: m.src,
                module: &m.path,
                scope,
                kinds: &s.kinds,
                diags: &mut scratch,
                unsupported: None,
                locals: &locals,
                at: m.src.span(h.node).lo,
                outer: Vec::new(),
            };
            let (_, _, generics) = low.trait_generics(h);
            out.entry(h.def).or_insert(generics);
        }
    }
    out
}

/// The members a delegating implementation `impl Tr for C by E` takes
/// from its part (spec 09 "Delegation", trait-solver.md §3.10), once every
/// item of the folder is lowered:
///
/// - each associated type of `Tr` is bound to the projection
///   `<F as Tr[A]>::Name`, where `F` is the embedded field `E`'s type at
///   `C`'s arguments (trait.by.assoc-types); normalizing it is an ordinary
///   projection;
/// - each method of `Tr` with a receiver that the body does not write
///   gets a forwarding method, with the trait method's signature at
///   `Self = C[...]` (trait.by.generated), whose body the checker writes
///   as `Tr::m(self.E, arguments...)` (`hd_check::delegate`).
///
/// An implementation whose part is not an embedded field of a data type
/// takes nothing: the checker reports it (`invalid-delegation`).
fn delegations(r: &Resolver<'_, '_>, modules: &mut [ModOut]) {
    let names = &r.cx.names;
    let pool = names.pool;
    let item = |d: DefId| -> Option<Item> {
        modules
            .iter()
            .flat_map(|m| m.items.iter())
            .find(|i| i.def == d)
            .cloned()
            .or_else(|| r.item(d))
    };
    let mut found: Vec<Taken> = Vec::new();
    for (mi, m) in modules.iter().enumerate() {
        for (ii, it) in m.items.iter().enumerate() {
            let ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                methods,
                assoc,
                by: Some(by),
                kind: ImplKind::Delegated,
            } = &it.data
            else {
                continue;
            };
            let (trait_, trait_args, self_ty) = (*trait_, *trait_args, *self_ty);
            let TyData::Adt {
                def: c,
                args: cargs,
            } = pool.get(self_ty)
            else {
                continue;
            };
            let Some(ItemData::Data(fields)) = item(c).map(|i| i.data) else {
                continue;
            };
            let Some(field) = fields.iter().find(|f| f.embedded && f.name == *by) else {
                continue;
            };
            let ca = pool.list_items(cargs);
            let part = pool.subst(field.ty, &|q: ParamRef| {
                (q.owner == c)
                    .then(|| ca.get(q.index as usize).copied())
                    .flatten()
            });
            let Some(ItemData::Trait(td)) = item(trait_).map(|i| i.data) else {
                continue;
            };
            let binds: Vec<(DefId, Ty)> = td
                .assoc
                .iter()
                .filter(|(_, a)| !assoc.iter().any(|(x, _)| x == a))
                .map(|&(_, a)| {
                    (
                        a,
                        pool.intern_ty(&TyData::Assoc {
                            assoc: a,
                            trait_,
                            self_ty: part,
                            args: trait_args,
                        }),
                    )
                })
                .collect();
            let targs = pool.list_items(trait_args);
            let mut fwd = Vec::new();
            for &(name, tm) in &td.methods {
                if methods.iter().any(|(w, _)| *w == name) {
                    continue;
                }
                let Some(sig) = item(tm).and_then(|i| i.sig().cloned()) else {
                    continue;
                };
                if sig.params.first().is_none_or(|p| names.text(p.0) != "self") {
                    continue;
                }
                let def = names.member(it.def, PathKind::Member, names.text(name));
                // The trait's `Self` and parameters at the implementation's,
                // the method's own parameters as the forwarder's.
                let at = |q: ParamRef| {
                    if q.owner == trait_ {
                        if q.index == 0 {
                            Some(self_ty)
                        } else {
                            targs.get(usize::from(q.index) - 1).copied()
                        }
                    } else if q.owner == tm {
                        Some(pool.intern_ty(&TyData::Param(ParamRef {
                            owner: def,
                            index: q.index,
                        })))
                    } else {
                        None
                    }
                };
                let ty = |t: Ty| pool.subst(t, &at);
                let generics = sig
                    .generics
                    .iter()
                    .map(|g| Generic {
                        bounds: g.bounds.iter().map(|b| ty(*b)).collect(),
                        default: g.default.map(ty),
                        ..g.clone()
                    })
                    .collect();
                let fsig = FnSig {
                    generics,
                    params: sig.params.iter().map(|(n, t)| (*n, ty(*t))).collect(),
                    defaults: sig.defaults.clone(),
                    ret: ty(sig.ret),
                    row: pool.subst_row(sig.row, &at),
                    suspends: sig.suspends,
                    variadic: sig.variadic,
                };
                fwd.push(Item::new(
                    def,
                    name,
                    true,
                    ItemData::Method {
                        owner: it.def,
                        sig: fsig,
                        has_body: true,
                    },
                ));
            }
            found.push(Taken {
                at: (mi, ii),
                binds,
                forwarders: fwd,
            });
        }
    }
    for t in found {
        let items = &mut modules[t.at.0].items;
        if let ItemData::Impl { methods, assoc, .. } = &mut items[t.at.1].data {
            assoc.extend(t.binds);
            methods.extend(t.forwarders.iter().map(|f| (f.name, f.def)));
        }
        items.extend(t.forwarders);
    }
}

/// What one delegating implementation takes from its part: where it is
/// (module, item), its associated-type bindings and its forwarding
/// methods.
struct Taken {
    at: (usize, usize),
    binds: Vec<(DefId, Ty)>,
    forwarders: Vec<Item>,
}

/// The supertraits of the traits of `mods` (`Resolver::own_supers`).
fn own_trait_supers(
    r: &Resolver<'_, '_>,
    mods: &[ModIn<'_>],
    all_heads: &[Vec<Head<'_>>],
    scopes: &[ModScopes],
) -> HashMap<DefId, Vec<Ty>> {
    let mut out = HashMap::new();
    let mut scratch = DiagBuf::default();
    for ((m, hs), s) in mods.iter().zip(all_heads).zip(scopes) {
        let locals = local_items(hs);
        for h in hs.iter().filter(|h| h.kind == HeadKind::Trait) {
            let scope = match (h.test, &s.tests) {
                (Some(_), Some(tests)) => tests,
                _ => &s.scope,
            };
            let mut low = Lower {
                r,
                names: r.cx.names,
                src: m.src,
                module: &m.path,
                scope,
                kinds: &s.kinds,
                diags: &mut scratch,
                unsupported: None,
                locals: &locals,
                at: m.src.span(h.node).lo,
                outer: Vec::new(),
            };
            let (self_ty, gn, _) = low.trait_generics(h);
            let supers = low.supers(h.node, &gn, self_ty);
            out.entry(h.def).or_insert(supers);
        }
    }
    out
}

/// One module's scopes while its folder resolves: its own, and with test
/// code, its `tests:` block's, whose diagnostics are kept apart.
struct ModScopes {
    scope: ModuleScope,
    kinds: Kinds,
    tests: Option<ModuleScope>,
    test_diags: DiagBuf,
}

/// The function nodes with bodies, by item: own functions and the methods
/// of impls.
#[must_use]
pub fn body_nodes<'t>(
    names: &Names<'_>,
    src: &Src<'t>,
    heads: &[Head<'t>],
) -> Vec<(DefId, NodeRef<'t>)> {
    let mut out = Vec::new();
    for h in heads {
        match h.kind {
            HeadKind::Fn => out.push((h.def, h.node)),
            // An impl's methods, and a trait's default method bodies.
            // A block with no trait declares no members.
            HeadKind::Impl if traitless_block(src, h.node) => {}
            HeadKind::Impl | HeadKind::Trait => {
                for f in Src::child(h.node, SyntaxKind::Block)
                    .iter()
                    .flat_map(|b| b.children())
                {
                    if f.kind() == SyntaxKind::FnDecl
                        && let Some(t) = f
                            .name(&src.parse.tokens)
                            .or_else(|| src.name_after(f, TokenKind::KwFn))
                    {
                        out.push((names.member(h.def, PathKind::Member, src.text(t)), f));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
