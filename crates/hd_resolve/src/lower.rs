//! Use resolution and header lowering for one folder (resolution-and-
//! interfaces.md §4.9, §4.10 steps 1 to 4): item heads, use declarations
//! in every form, the export worklist with `pub use` chains, module scopes
//! with the prelude, and every header lowered to `hd_types`.
//!
//! The same code serves two modes. While `FolderIface(F)` runs, uses of
//! sibling modules resolve through F's own heads and a worklist. Once F's
//! interface is frozen, `ModulePrep` and `Body` resolve them through its
//! export index instead.

use hd_base::{HashMap, HashSet};
use std::cell::RefCell;
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
    Export, Field, FnSig, FolderIface, Generic, HeadKind, ImplKind, Item, ItemData, Lookup, Names,
    TraitData, Variant, fill_trait_args, fill_type_args, show_ty,
};
use crate::known::KnownItems;
use crate::variance::{self, Seen};
use crate::view::Src;

mod decls;
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
    ("std.hash", &["Hash"]),
    ("std.iter", &["Iterator", "Iterable"]),
    ("std.console", &["Console", "ConsoleError", "println"]),
    ("std.task", &["Suspend"]),
];

/// The prelude names of test code (`module.prelude.test-assertions`): bound only
/// in test code, but no declaration of a module may take their place.
const TEST_PRELUDE: &[(&str, &[&str])] = &[("std.testing", &["it", "assert", "assert_equal"])];

/// Whether `name` is a prelude name (`module.prelude.no-shadow`).
#[must_use]
pub fn is_prelude_name(name: &str) -> bool {
    PRELUDE.iter().any(|(_, list)| list.contains(&name))
}

/// Whether `name` is a prelude name of test code only.
#[must_use]
pub fn is_test_prelude_name(name: &str) -> bool {
    TEST_PRELUDE.iter().any(|(_, list)| list.contains(&name))
}

/// The declaration that a prelude name of test code names
/// (`module.prelude.test-assertions`): an unbound registration or assertion in test code.
#[must_use]
pub fn test_prelude_item(names: &Names<'_>, name: &str) -> Option<DefId> {
    TEST_PRELUDE
        .iter()
        .find(|(_, list)| list.contains(&name))
        .map(|(module, _)| names.item(module, name))
}

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
    /// Dotted text already owned by the world, when it indexes module nodes.
    fn module_path_node(&self, _module: PathId) -> Option<&str> {
        None
    }
    /// A known module's folder without rendering a dotted path. Small worlds
    /// may keep the text-only implementation; the project driver indexes nodes.
    fn module_folder_node(&self, names: &Names<'_>, module: PathId) -> Option<u32> {
        if let Some(text) = self.module_path_node(module) {
            self.module_folder(text)
        } else {
            self.module_folder(&names.module_text(module))
        }
    }
    /// Whether a `use` may name the module: it exists and is not its own
    /// program (`module.path.main-no-use`).
    fn usable(&self, module: &str) -> bool;
    /// Whether `module` is test-only code (a test module or an integration
    /// test module), which non-test code may not use
    /// (`module.test.non-test-use.test-module`).
    fn test_only(&self, module: &str) -> bool;
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>>;
    /// Holds (`true`) or resumes the recording of the reads that follow,
    /// in a world that records them ([`Recorded`]): binding a use reads
    /// quietly, since its reads count only once a lookup consults a name
    /// it binds (#248).
    fn hold_reads(&self, _held: bool) {}
}

/// A [`World`] that records what one folder's header lowering reads of
/// it (#248): each module path it asks about, found or not, and each
/// folder whose interface it opens. A use's own reads are held while it
/// binds, and count once a lookup consults a name it binds
/// ([`build_folder`] reads them again then). `FolderIface(F)` keys F's
/// interface by these reads, not by every folder F's uses reach
/// (`crates/hd_cache/DESIGN.md` §5.3).
pub struct Recorded<'a> {
    world: &'a dyn World,
    reads: std::sync::Mutex<Reads>,
    held: std::sync::atomic::AtomicBool,
}

/// What a [`Recorded`] world was asked, sorted, without repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    /// Each module path asked for its folder, its use, or its test role.
    pub modules: Vec<String>,
    /// Each folder whose interface was opened, by folder ID.
    pub folders: Vec<u32>,
}

impl<'a> Recorded<'a> {
    #[must_use]
    pub fn new(world: &'a dyn World) -> Self {
        Self {
            world,
            reads: std::sync::Mutex::default(),
            held: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The reads so far.
    #[must_use]
    pub fn reads(&self) -> Reads {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Reads> {
        self.reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn module(&self, module: &str) {
        if self.held() {
            return;
        }
        let mut r = self.lock();
        if let Err(at) = r.modules.binary_search_by(|m| m.as_str().cmp(module)) {
            r.modules.insert(at, module.to_owned());
        }
    }
}

impl World for Recorded<'_> {
    fn module_path_node(&self, module: PathId) -> Option<&str> {
        self.world.module_path_node(module)
    }
    fn module_folder_node(&self, names: &Names<'_>, module: PathId) -> Option<u32> {
        if let Some(text) = self.world.module_path_node(module) {
            self.module(text);
        } else {
            self.module(&names.module_text(module));
        }
        self.world.module_folder_node(names, module)
    }
    fn module_folder(&self, module: &str) -> Option<u32> {
        self.module(module);
        self.world.module_folder(module)
    }
    fn usable(&self, module: &str) -> bool {
        self.module(module);
        self.world.usable(module)
    }
    fn test_only(&self, module: &str) -> bool {
        self.module(module);
        self.world.test_only(module)
    }
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>> {
        if !self.held() {
            let mut r = self.lock();
            if let Err(at) = r.folders.binary_search(&folder) {
                r.folders.insert(at, folder);
            }
        }
        self.world.iface(folder)
    }
    fn hold_reads(&self, held: bool) {
        self.held.store(held, std::sync::atomic::Ordering::Relaxed);
    }
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
    /// The path's first segment as written, before `path` resolved it.
    pub written_root: String,
    /// The path before its roots were rewritten, for a missing module's
    /// message when its absolute path names compiler-owned test code.
    written_path: String,
    /// A synthetic doc-test module sees only the package's library view.
    doc_test: bool,
}

impl UseDecl {
    #[must_use]
    pub fn module(&self) -> String {
        self.path.join(".")
    }

    fn missing_module(&self, module: &str) -> String {
        let written = self.doc_test
            || self
                .path
                .iter()
                .any(|s| matches!(s.as_str(), "$tests" | "$tasks"));
        let path = if written { &self.written_path } else { module };
        let mut message = format!("no module named `{path}`");
        if self.doc_test && self.written_root == "pkg" {
            message.push_str(
                "; doc tests see only library modules, not the executable's declarations",
            );
        }
        message
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
                    .filter(|t| {
                        matches!(src.tkind(*t), Some(TokenKind::Ident | TokenKind::RawIdent))
                    })
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
    // `use dep.NAME` binds `NAME`, the final component as written
    // (`names.use.final-component`), not the root of the package it names,
    // which another package of the same name may change
    // (`module.dep.key-name`).
    let alias = match (&alias, &group, path.as_slice()) {
        (None, None, [root, name]) if root == "dep" => Some(name.clone()),
        _ => alias,
    };
    let written_root = path.first().cloned().unwrap_or_default();
    let written_path = path.join(".");
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
        written_root,
        written_path,
        doc_test: roots.doc_test,
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

/// A block-local declaration's place (`crates/hd_tir/DESIGN.md` "Local items
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

/// The span of a declaration's name, else of the whole declaration.
#[must_use]
pub fn head_name_span(src: &Src<'_>, h: &Head<'_>) -> Span {
    decl_kind(src, h.node)
        .and_then(|(_, kw)| {
            h.node
                .name(&src.parse.tokens)
                .or_else(|| src.name_after(h.node, kw))
        })
        .map_or_else(|| src.span(h.node), |t| src.token_span(t))
}

/// The name token of a member of a trait, an implementation or a block: a
/// function or an associated type.
fn member_name(src: &Src<'_>, f: NodeRef<'_>) -> Option<hd_base::TokenIdx> {
    f.name(&src.parse.tokens).or_else(|| {
        (f.kind() == SyntaxKind::FnDecl)
            .then(|| src.name_after(f, TokenKind::KwFn))
            .flatten()
    })
}

/// Heads of the module's own items: the top-level ones, each followed by
/// the block-local declarations of its bodies (`crates/hd_tir/DESIGN.md` "Local
/// items lift to hidden module items"). With `tests`, as test code is
/// checked, the items of its `tests:` block follow (`names.tests.module-items`).
#[must_use]
pub fn heads<'t>(names: &Names<'_>, src: &Src<'t>, module: &str, tests: bool) -> Vec<Head<'t>> {
    let mut out = Vec::new();
    let mut impl_names: HashMap<String, u32> = HashMap::default();
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
                name: src.symbol(t, names.syms),
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
                src.symbol(t, names.syms),
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
    /// For a REPL input (`f1c55020:future-work/compiler/repl-plan.md` §2.2): the session scope its own
    /// scope is nested in. Every top-level declaration and `use` of the
    /// input is then exported, so later inputs see it whatever its `pub`.
    pub session: Option<Arc<SessionScope>>,
}

/// The names a REPL input sees besides its own: `src/lib.hd`'s scope, its
/// private declarations included (`cli.repl.package.lib`), with each
/// earlier input's top-level names nested in it, the newest last.
#[derive(Clone, Debug, Default)]
pub struct SessionScope {
    pub scope: ModuleScope,
    pub kinds: Kinds,
    /// What declared each of those names: `src/lib.hd` or `repl#N`
    /// (`cli.repl.redeclare.binding`, `cli.repl.redeclare.type`).
    pub declared_by: HashMap<Symbol, String>,
    /// The paths of the modules the session holds besides the input:
    /// `src/lib.hd`'s and the earlier inputs'. A session counts as one
    /// module wherever a rule compares modules, so an implementation in
    /// the input may have an owner declared in one of them
    /// ([`impl_placement`]).
    pub modules: Vec<String>,
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
    /// Names no use can reach through this folder: each public name whose
    /// `pub use` chain returns to a `pub use` it passed
    /// (`module.pub-use.chain.loop`), as (module path, name), sorted. A
    /// use through another folder reads them as a loop, quietly.
    pub loop_names: Vec<(String, Symbol)>,
    /// For each REPL input ([`ModIn::session`]): the modules its `use`s
    /// bind, as (module path, local name, used module path), sorted.
    pub module_uses: Vec<(String, String, String)>,
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
    // Module identity is the package/module trie node, never an item node or
    // an hd_project::ModuleTable row. Text is shared only at World/blob boundaries.
    module_keys: HashMap<Arc<str>, PathId>,
    module_paths: HashMap<PathId, Arc<str>>,
    own: HashMap<(PathId, Symbol), (DefId, HeadKind, bool)>,
    /// The `struct` declarations of the modules being lowered.
    struct_defs: HashSet<DefId>,
    pub_uses: HashMap<(PathId, Symbol), (PathId, Symbol)>,
    memo: RefCell<HashMap<(PathId, Symbol), Res>>,
    visiting: RefCell<HashSet<(PathId, Symbol)>>,
    /// Re-export loops already reported, each as its cycle's sorted nodes,
    /// so a loop is one error (`module.pub-use.chain.loop`).
    loops_reported: RefCell<HashSet<Vec<(PathId, Symbol)>>>,
    /// Associated type names of this folder's traits, from their heads.
    trait_assoc: HashMap<DefId, Vec<Symbol>>,
    /// The declared variance markers of the folder's own declarations.
    variances: HashMap<DefId, Vec<i8>>,
    /// Whether each generic parameter of the folder's own declarations is
    /// a row parameter, from their heads.
    row_params: HashMap<DefId, Vec<bool>>,
    /// This folder's aliases, each lowered after the aliases it names:
    /// whether each generic parameter is a row parameter, and the body.
    aliases: RefCell<HashMap<DefId, (Vec<bool>, Ty)>>,
    seeds: HashMap<DefId, Item>,
    /// The declared parameters of the traits of the modules being lowered,
    /// lowered before any other header, so a trait reference fills its
    /// defaults whatever the declaration order, and a private trait (which
    /// a frozen interface leaves out) is filled as while it was built.
    own_traits: HashMap<DefId, Vec<Generic>>,
    /// The declared parameters of the folder's own aliases and of its data
    /// types, enums and newtypes that declare a default, so a written type
    /// fills its omitted trailing arguments from them (`fill_type_args`).
    /// They are recorded by [`lower_aliases`], each after the aliases and
    /// data types its declaration names.
    own_types: RefCell<HashMap<DefId, Vec<Generic>>>,
    /// The supertraits of the traits of the modules being lowered, as
    /// trait-value types over each trait's `Self`, lowered before any
    /// other header, so a projection or a binding finds the trait that
    /// declares its associated type (trait.binding.name-reach) whatever
    /// the declaration order.
    own_supers: HashMap<DefId, Vec<Ty>>,
    /// The REPL inputs being lowered ([`ModIn::session`]): no `use` names
    /// one, yet each exports its own names.
    inputs: HashSet<String>,
    /// For each REPL input being lowered: the other modules of its
    /// session ([`SessionScope::modules`]).
    session_modules: HashMap<String, Vec<String>>,
}

impl Resolver<'_, '_> {
    fn intern_module(&mut self, path: &str) -> PathId {
        if let Some(&module) = self.module_keys.get(path) {
            return module;
        }
        let module = self.cx.names.module(path);
        let text: Arc<str> = path.into();
        self.module_keys.insert(Arc::clone(&text), module);
        self.module_paths.insert(module, text);
        module
    }

    fn module_text(&self, module: PathId) -> &str {
        &self.module_paths[&module]
    }

    fn module_exists(&self, module: &str) -> bool {
        self.cx.world.usable(module)
    }

    fn export(&self, module: &str, name: Symbol) -> Res {
        let Some(f) = self
            .cx
            .world
            .module_folder(module)
            .filter(|_| self.cx.world.usable(module) || self.inputs.contains(module))
        else {
            return Err(Code::UnknownModule);
        };
        let frozen = |i: &FolderIface| {
            i.export(module, name)
                .map(|e| (e.def, e.kind))
                .ok_or(if i.is_private(module, name) {
                    Code::PrivateImport
                } else if i.loop_names.contains(&(module.to_owned(), name)) {
                    Code::ReExportLoop
                } else {
                    Code::UnknownImport
                })
        };
        if f != self.cx.folder {
            let Some(i) = self.cx.world.iface(f) else {
                // A test-only module has no interface outside a test
                // build; naming it from non-test code is test-only-use,
                // not unknown-module (`module.test.non-test-use`).
                if self.cx.world.test_only(module) {
                    return Err(Code::TestOnlyUse);
                }
                return Err(Code::UnknownModule);
            };
            return frozen(&i);
        }
        if let Some(i) = &self.frozen {
            return frozen(i);
        }
        // An unfrozen folder normally supplies every module. If a caller
        // supplies a subset, preserve its previous absent-head lookup result.
        let Some(&module) = self.module_keys.get(module) else {
            return Err(Code::UnknownImport);
        };
        let key = (module, name);
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
            if !self.visiting.borrow_mut().insert(key) {
                return Err(Code::ReExportLoop);
            }
            let r = self.export(self.module_text(*m), *n);
            self.visiting.borrow_mut().remove(&key);
            r
        } else {
            Err(Code::UnknownImport)
        };
        self.memo.borrow_mut().insert(key, r);
        r
    }

    /// Whether `module` lives in the folder being lowered: only then did
    /// the lookup above follow this folder's own `pub use` chain.
    fn is_own_folder(&self, module: &str) -> bool {
        self.cx.world.module_folder(module) == Some(self.cx.folder)
    }

    /// The `pub use` cycle reachable from `key`, if following the chain
    /// returns to a link it passed (`module.pub-use.chain.loop`): the
    /// cycle's nodes in a canonical order, whatever link the walk starts
    /// from.
    fn export_cycle(&self, key: (PathId, Symbol)) -> Option<Vec<(PathId, Symbol)>> {
        let mut at = key;
        let mut seen: HashMap<(PathId, Symbol), usize> = HashMap::default();
        let mut chain: Vec<(PathId, Symbol)> = Vec::new();
        while chain.len() <= self.pub_uses.len() {
            if let Some(&start) = seen.get(&at) {
                let mut cycle = chain[start..].to_vec();
                cycle.sort_by(|a, b| {
                    (self.module_text(a.0), self.cx.names.text(a.1))
                        .cmp(&(self.module_text(b.0), self.cx.names.text(b.1)))
                });
                return Some(cycle);
            }
            seen.insert(at, chain.len());
            let next = *self.pub_uses.get(&at)?;
            chain.push(at);
            at = next;
        }
        None
    }

    /// Reports a re-export loop once per cycle, at `span`. A loop through
    /// another folder was reported when its interface was built, so its
    /// uses stay quiet here.
    fn report_loop(&self, diags: &mut DiagBuf, span: hd_base::Span, module: &str, name: Symbol) {
        if !self.is_own_folder(module) {
            return;
        }
        let cycle = self
            .module_keys
            .get(module)
            .and_then(|&module| self.export_cycle((module, name)));
        let fresh = match cycle {
            Some(cycle) => self.loops_reported.borrow_mut().insert(cycle),
            // No cycle in this folder's chain: report, as before.
            None => true,
        };
        if fresh {
            let msg = format!("re-export loop through `{}`", self.cx.names.text(name));
            diags.error(Code::ReExportLoop, span, &msg);
        }
    }

    /// Whether `d` is a `struct` declaration (`data.struct.decl`).
    fn is_struct(&self, d: DefId) -> bool {
        self.struct_defs.contains(&d) || self.item(d).is_some_and(|i| i.is_struct)
    }

    /// An item of another folder or a seed, for kinds and alias bodies.
    fn item(&self, d: DefId) -> Option<Item> {
        if let Some(i) = self.seeds.get(&d) {
            return Some(i.clone());
        }
        let module = self.cx.names.module_node(d);
        let f = self.cx.world.module_folder_node(&self.cx.names, module)?;
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

    /// The number of generic parameters a trait declares.
    fn trait_arity(&self, tr: DefId) -> Option<usize> {
        if let Some(g) = self.own_traits.get(&tr) {
            return Some(g.iter().filter(|g| !g.row).count());
        }
        self.item(tr)
            .map(|item| item.generics.iter().filter(|g| !g.row).count())
    }

    /// A written type constructor's arguments with the omitted trailing
    /// ones filled from the declaration's defaults
    /// (`types.generic.default.written`): `Outcome[string]` is
    /// `Result[string, AppError]` for `type Outcome[T, E = AppError]`.
    ///
    /// The second result names the first type parameter left without an
    /// argument that has no default (`types.generic.default.written-missing`);
    /// the third is the number of parameters the declaration has
    /// (`types.generic.too-long`).
    fn fill_type_args(&self, def: DefId, args: &[Ty]) -> (Vec<Ty>, Option<String>, Option<usize>) {
        let pool = self.cx.names.pool;
        let fill = |generics: &[Generic]| {
            let filled = fill_type_args(pool, def, generics, args);
            let missing = generics
                .iter()
                .skip(filled.len())
                .find(|g| !g.row && g.default.is_none())
                .map(|g| self.cx.names.text(g.name).to_owned());
            (filled, missing, Some(generics.len()))
        };
        if let Some(g) = self.own_types.borrow().get(&def) {
            return fill(g);
        }
        if let Some(i) = self.seeds.get(&def) {
            return fill(&i.generics);
        }
        let module = self.cx.names.module_node(def);
        let iface = self
            .cx
            .world
            .module_folder_node(&self.cx.names, module)
            .and_then(|f| {
                if f == self.cx.folder {
                    self.frozen.clone()
                } else {
                    self.cx.world.iface(f)
                }
            });
        match iface.as_ref().and_then(|i| i.item(def)) {
            Some(item) => fill(&item.generics),
            None => (args.to_vec(), None, None),
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
    let mut kinds = Kinds::default();
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
    // An interface build notes the names its lowering looks up, which
    // decide the uses whose reads key it (`reread_uses`).
    if r.frozen.is_none() {
        scope.track();
    }
    bind_uses(r, &mut scope, &mut kinds, uses, diags);
    if let Some(outer) = &m.session {
        let seen = scope.looked_up.take();
        scope = outer.scope.overlay(&scope);
        scope.looked_up = seen;
        for (d, k) in &outer.kinds {
            kinds.entry(*d).or_insert(*k);
        }
    }
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
        // And no module name twice inside it (`names.module.unique`).
        if inner.lookup(h.name).is_some() {
            let msg = format!("`{name}` is already declared in the `tests:` block");
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
    r.cx.world.hold_reads(true);
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
                // `module.rt.std-only.error`.
                UseRootError::StdOnly => (
                    Code::StdOnlyUse,
                    format!(
                        "`{}` is a runtime module, which only the standard library may use",
                        u.path.iter().take(3).cloned().collect::<Vec<_>>().join(".")
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
                    let msg = u.missing_module(&module);
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
                        Err(Code::ReExportLoop) => {
                            r.report_loop(diags, g.span, &module, sym);
                            let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
                            bind_poison(scope, local, use_origin(u), row);
                        }
                        Err(code) => {
                            let msg = export_failure(code, &g.name, &module);
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
                        let msg = format!(
                            "'{last}' names both the module '{last}' and a declaration of module '{}'; rename one",
                            u.written_root
                        );
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
                    let msg = u.missing_module(&parent);
                    diags.error(Code::UnknownModule, u.span, &msg);
                    bind_poison(scope, local, use_origin(u), row);
                } else {
                    let code = r
                        .export(&parent, names.syms.intern(last))
                        .err()
                        .unwrap_or(Code::UnknownImport);
                    if code == Code::ReExportLoop {
                        r.report_loop(diags, u.span, &parent, names.syms.intern(last));
                    } else {
                        let msg = export_failure(code, last, &parent);
                        diags.error(code, u.span, &msg);
                    }
                    bind_poison(scope, local, use_origin(u), row);
                }
            }
        }
    }
    r.cx.world.hold_reads(false);
}

/// Asks the world again what [`bind_uses`] asked it for each use of
/// `uses` that binds a name of `seen`, so a recording world
/// ([`Recorded`]) counts those reads.
fn reread_uses(r: &Resolver<'_, '_>, uses: &[UseDecl], seen: &HashSet<Symbol>) {
    let names = &r.cx.names;
    let local = |name: &str, alias: &Option<String>| {
        seen.contains(&names.syms.intern(alias.as_deref().unwrap_or(name)))
    };
    for u in uses.iter().filter(|u| u.root_error.is_none()) {
        let module = u.module();
        if let Some(group) = &u.group {
            let mut asked = false;
            for g in group.iter().filter(|g| local(&g.name, &g.alias)) {
                if !asked {
                    asked = true;
                    let _ = r.module_exists(&module);
                }
                let _ = r.export(&module, names.syms.intern(&g.name));
            }
        } else if let Some((last, parent)) = u.path.split_last()
            && local(last, &u.alias)
        {
            let _ = r.module_exists(&module);
            if !parent.is_empty() {
                let parent = parent.join(".");
                let _ = r.export(&parent, names.syms.intern(last));
                let _ = r.module_exists(&parent);
            }
        }
    }
}

/// The names `u` exports from its module `m`, each as (local name,
/// module, declared name): the grouped names of a `pub use`. A REPL input
/// ([`ModIn::session`]) exports every name its uses bind to a declaration,
/// so a single use's name reaches later inputs as a grouped one's does. A
/// single use that names a module binds no declaration.
fn reexports(r: &Resolver<'_, '_>, m: &ModIn<'_>, u: &UseDecl) -> Vec<(Symbol, String, Symbol)> {
    let names = &r.cx.names;
    let session = m.session.is_some();
    if let Some(group) = &u.group {
        if !(u.public || session) {
            return Vec::new();
        }
        let module = u.module();
        return group
            .iter()
            .map(|g| {
                let local = names.syms.intern(g.alias.as_deref().unwrap_or(&g.name));
                (local, module.clone(), names.syms.intern(&g.name))
            })
            .collect();
    }
    let Some((last, parent)) = u.path.split_last() else {
        return Vec::new();
    };
    if !session || u.root_error.is_some() || parent.is_empty() || r.module_exists(&u.module()) {
        return Vec::new();
    }
    let local = names
        .syms
        .intern(u.alias.as_deref().unwrap_or(last.as_str()));
    vec![(local, parent.join("."), names.syms.intern(last))]
}

/// What the user reads when `module` does not give `name` to an import.
fn export_failure(code: Code, name: &str, module: &str) -> String {
    match code {
        Code::PrivateImport => format!("'{name}' is private to module '{module}'"),
        Code::UnknownModule => format!("no module named '{module}'"),
        Code::ReExportLoop => format!("re-export loop through `{name}`"),
        Code::TestOnlyUse => format!("`{module}` is test-only code, which only test code may use"),
        _ => format!("module '{module}' declares no '{name}'"),
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
    let test_module = r.cx.world.test_only(&m.path);
    for (module, list) in PRELUDE
        .iter()
        .chain(TEST_PRELUDE.iter().filter(|_| test_module))
    {
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
    // Whole test modules have their test names bound above. An ordinary
    // module does not bind them, yet its declarations cannot take one
    // (`module.prelude.no-shadow`). Their declaring module is the source.
    if test_module {
        return;
    }
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
/// An associated type member: its name, its def, its value in an
/// implementation, and its bounds in a trait (`trait.assoc.bound-decl`).
type AssocDecl = (Symbol, DefId, Option<Ty>, Vec<Ty>);

/// A bound's trait and its arguments, defaults filled.
type BoundRef = (DefId, TyList);

/// The type parameters in scope while lowering one header.
#[derive(Clone, Default)]
struct Gen {
    /// Type parameters in scope: name, type, and each bound's trait with
    /// its (default-filled) arguments.
    tys: Vec<(Symbol, Ty, Vec<BoundRef>)>,
    rows: Vec<(Symbol, RowParamRef)>,
    /// Where each type parameter's name is declared.
    decls: Vec<(Symbol, Span)>,
    /// The header declares no row parameter (`req.row.param.no-data`).
    type_kinded: bool,
    self_ty: Option<Ty>,
    self_trait: Option<DefId>,
    /// Type parameters written with a bound list. Nothing reads it now:
    /// `mut T` is an error for every type parameter
    /// (types.value.no-mut.written).
    bounded: Vec<Ty>,
    /// Type parameters of the list being lowered whose bounds are not
    /// lowered yet: whether one is bounded by `Tuple` is not known.
    pending: Vec<Ty>,
}

impl Gen {
    /// Adds one header's bindings while retaining the enclosing scope's
    /// vectors and bounds. Signature lowering appends entries; only the
    /// bounds of newly appended type parameters are filled in place.
    fn with_scope<R>(&mut self, lower: impl FnOnce(&mut Self) -> R) -> R {
        let lengths = (
            self.tys.len(),
            self.rows.len(),
            self.decls.len(),
            self.bounded.len(),
            self.pending.len(),
        );
        let context = (self.type_kinded, self.self_ty, self.self_trait);
        let value = lower(self);
        self.tys.truncate(lengths.0);
        self.rows.truncate(lengths.1);
        self.decls.truncate(lengths.2);
        self.bounded.truncate(lengths.3);
        self.pending.truncate(lengths.4);
        (self.type_kinded, self.self_ty, self.self_trait) = context;
        value
    }
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
    /// Type parameters already reported as used in a row: one error per
    /// parameter, at its first use.
    kind_reported: HashSet<u32>,
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

    /// Whether the module belongs to the standard library, which is the
    /// one package allowed to implement a sealed trait.
    fn in_std(&self) -> bool {
        self.module.split('.').next() == Some("std")
    }

    fn sym(&self, text: &str) -> Symbol {
        self.names.syms.intern(text)
    }

    fn sym_token(&self, token: hd_base::TokenIdx) -> Symbol {
        self.src.symbol(token, self.names.syms)
    }

    fn import_hint(&self, name: &str) -> String {
        if self.in_std() {
            return String::new();
        }
        let world = self.r.cx.world;
        // Core shares std's root folder and its public export index.
        // Prelude has child modules, so its own folder is only re-exports.
        let core = self.names.module_node(self.names.known.list);
        world
            .module_folder_node(&self.names, core)
            .and_then(|folder| world.iface(folder))
            .map_or_else(String::new, |iface| {
                crate::iface::std_import_hint(&self.names, &iface.exports, name)
            })
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
    fn is_poisoned(&self, sym: Symbol) -> bool {
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
        let (first, token) = segs.first()?;
        let first_sym = self.sym_token(*token);
        if self.is_poisoned(first_sym) {
            return None;
        }
        // A local declaration in scope here shadows the module's names.
        if segs.len() == 1
            && let Some(l) = local_at(self.locals, first_sym, self.at)
        {
            return Some((l.def, l.kind));
        }
        let b = self.scope.lookup(first_sym);
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
        // A `use` of a module brings in that module alone, not its children
        // (`module.path.no-child-import`, `module.path.no-std-child-import`);
        // only a root module's name leads to them.
        if segs.len() > 2 && module.contains('.') {
            let msg = format!("`{first}` does not bring in the child modules of `{module}`");
            self.diags.error(Code::UnknownImport, span, &msg);
            return None;
        }
        for (s, _) in &segs[1..segs.len() - 1] {
            module.push('.');
            module.push_str(s);
        }
        let (last, token) = segs.last()?;
        let last_sym = self.sym_token(*token);
        match self.r.export(&module, last_sym) {
            Ok(x) => Some(x),
            Err(Code::ReExportLoop) => {
                let r = self.r;
                r.report_loop(&mut *self.diags, span, &module, last_sym);
                None
            }
            Err(code) => {
                let msg = export_failure(code, last, &module);
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
                        let name = self.sym_token(t);
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
        if segs
            .first()
            .is_some_and(|s| self.is_poisoned(self.sym_token(s.1)))
        {
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
                let msg = format!("no trait named `{name}`{}", self.import_hint(&name));
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
        if let Some(max) = self.r.trait_arity(def)
            && args.len() > max
        {
            self.too_many_args(def, max, args.len(), span);
            return None;
        }
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
    /// supertraits (trait.dyn.binding.complete.reference-values, req.key.binding.complete).
    fn require_complete(&mut self, t: Ty, span: Span) {
        if let Some(msg) = crate::assoc::incomplete_message(&self.names, self.r, t) {
            self.diags.error(Code::TraitNotDynamicallySafe, span, &msg);
        }
    }

    fn trait_of(&self, t: Ty) -> Option<DefId> {
        self.names.pool.trait_value(t).map(|(def, _)| def)
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
            if segs.len() == 1 && self.type_param_in_row(c, &segs[0].0, gn) {
                continue;
            }
            if segs
                .first()
                .is_some_and(|s| self.is_poisoned(self.sym_token(s.1)))
            {
                continue;
            }
            let found = self.resolve_path(&segs, self.src.span(c));
            if let Some((def, HeadKind::Alias)) = found {
                self.alias_in_row(c, def, gn, &mut data);
                continue;
            }
            if let Some(t) = self.trait_value_from(c, gn, None, found) {
                self.require_complete(t, self.src.span(c));
                let t = crate::assoc::with_super_bindings(self.names.pool.types(), self.r, t);
                data.keys.push(t);
            }
        }
        self.names.pool.row(&data)
    }

    /// `req.row.param.unmarked`, `req.row.param.no-data.error`: a
    /// type-kinded parameter written in a row. A function's or
    /// implementation's parameter gets a fix-it adding `$`. Reports
    /// whether `name` is such a parameter.
    fn type_param_in_row(&mut self, c: NodeRef<'_>, name: &str, gn: &Gen) -> bool {
        let Some(decl) = gn
            .decls
            .iter()
            .rev()
            .find(|(s, _)| self.names.text(*s) == name)
            .map(|(_, sp)| *sp)
        else {
            return false;
        };
        if !self.kind_reported.insert(decl.lo) {
            return true;
        }
        let span = self.src.span(c);
        if gn.type_kinded {
            let msg = format!(
                "`{name}` is a type parameter, and a data type, enum, trait or newtype declares no row parameter"
            );
            self.diags.error(Code::GenericKindMismatch, span, &msg);
        } else {
            let msg =
                format!("`{name}` is a type parameter used in a row: declare it as `${name}`");
            self.diags.error(Code::GenericKindMismatch, span, &msg);
            let at = Span {
                hi: decl.lo,
                ..decl
            };
            self.diags.add_fix(
                &format!("declare `{name}` as a row parameter"),
                hd_diag::FixSafety::Exact,
                &[(at, "$")],
            );
        }
        true
    }

    /// An alias written in a row (`req.row.alias.expand`): a row alias
    /// stands for its keys, which the row flattens in. A type alias names
    /// a type, never a key (`req.row.alias.type-alias.key`).
    fn alias_in_row(&mut self, c: NodeRef<'_>, def: DefId, gn: &Gen, data: &mut RowData) {
        let (args, _, _) = self.type_args(c, gn);
        let span = self.src.span(c);
        let t = self.expand_alias(def, &args, span);
        let name = self.names.display_name(def).to_owned();
        match self.names.pool.get(t) {
            TyData::Row(_) => data.keys.push(t),
            TyData::Poison => {}
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
        (kind, gn, referent): (HeadKind, &Gen, bool),
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
        // A written type omits its defaulted trailing arguments
        // (`types.generic.default.written`, `types.generic.default.bare`).
        let filled;
        let args = if matches!(
            kind,
            HeadKind::Alias | HeadKind::Data | HeadKind::Enum | HeadKind::Newtype
        ) {
            let (f, missing, arity) = self.r.fill_type_args(def, args);
            // `types.generic.too-long`: no more arguments than parameters.
            if let Some(max) = arity.filter(|max| args.len() > *max) {
                self.too_many_args(def, max, args.len(), span);
                return Ty::POISON;
            }
            // A parameter left without an argument or a default
            // (`types.generic.default.written-missing`).
            if let Some(param) = missing {
                let msg = format!(
                    "`{}` is written without its parameter `{param}`, which has no default",
                    self.names.display_name(def)
                );
                self.diags.error(Code::PartialGenericArguments, span, &msg);
                return Ty::POISON;
            }
            filled = f;
            filled.as_slice()
        } else {
            args
        };
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
                // Each argument has the kind of its parameter
                // (`grammar.type.row-argument.kind`).
                if let Some((i, row)) = args
                    .iter()
                    .zip(self.generic_rows(def))
                    .enumerate()
                    .find_map(|(i, (a, row))| {
                        (matches!(pool.get(*a), TyData::Row(_)) != row).then_some((i, row))
                    })
                {
                    let name = self.names.display_name(def);
                    let msg = if row {
                        format!(
                            "argument {} of `{name}` is a row parameter: write it after `$`",
                            i + 1
                        )
                    } else {
                        format!(
                            "argument {} of `{name}` is a type parameter, not a row",
                            i + 1
                        )
                    };
                    self.diags.error(Code::GenericKindMismatch, span, &msg);
                    return Ty::POISON;
                }
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
                let at = Span {
                    hi: span.lo,
                    ..span
                };
                let (message, prefix) = if referent {
                    ("insert `dyn` before the trait name", "dyn ")
                } else {
                    ("insert `&dyn` before the trait name", "&dyn ")
                };
                self.diags
                    .add_fix(message, hd_diag::FixSafety::Exact, &[(at, prefix)]);
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

    /// `types.generic.too-long`: a written type or trait with more
    /// arguments than its declaration has parameters.
    fn too_many_args(&mut self, def: DefId, max: usize, got: usize, span: Span) {
        let msg = format!(
            "`{}` takes {max} type arguments, not {got}",
            self.names.display_name(def)
        );
        self.diags.error(Code::ArgumentCount, span, &msg);
    }

    fn named(&mut self, n: NodeRef<'_>, gn: &Gen, referent: bool) -> Ty {
        let segs = self.segments(n);
        let span = self.src.span(n);
        let Some((first, ft)) = segs.first().cloned() else {
            return Ty::POISON;
        };
        if segs.len() == 1 {
            if self.src.tkind(ft) == Some(TokenKind::KwSelfType) {
                let Some(self_ty) = gn.self_ty else {
                    // `Self` outside a trait or impl body names nothing
                    // (`names.self-type.outside`).
                    let msg = "no type named `Self` here: `Self` names the implementing type inside a trait or impl";
                    self.diags.error(Code::UnknownType, span, msg);
                    return Ty::POISON;
                };
                return self_ty;
            }
            if let Some((_, t, _)) = gn
                .tys
                .iter()
                .rev()
                .find(|(s, _, _)| self.names.text(*s) == first)
            {
                return *t;
            }
            if gn.rows.iter().any(|(s, _)| self.names.text(*s) == first) {
                let msg = format!(
                    "`{first}` is a row parameter, not a type: write it in a row after `$`, as in `$ {first}`"
                );
                self.diags.error(Code::GenericKindMismatch, span, &msg);
                return Ty::POISON;
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
        if self.is_poisoned(self.sym_token(ft)) {
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
                let msg = format!("no type named `{first}`{}", self.import_hint(&first));
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
        self.ctor(def, (kind, gn, referent), &args, row, span)
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

    fn ty(&mut self, n: Option<NodeRef<'_>>, gn: &Gen) -> Ty {
        self.ty_at(n, gn, false)
    }

    // `dyn Tr` describes a referent or an inherent implementation target;
    // it is never a sized value type in its own right.
    fn ty_at(&mut self, n: Option<NodeRef<'_>>, gn: &Gen, referent: bool) -> Ty {
        let Some(n) = n else { return Ty::VOID };
        let pool = self.names.pool;
        let first_ty = |me: &mut Self, n: NodeRef<'_>| -> Ty {
            let c = n.children().find(|c| c.kind().is_type());
            me.ty(c, gn)
        };
        match n.kind() {
            SyntaxKind::NamedType => self.named(n, gn, referent),
            SyntaxKind::ImplType => {
                self.diags.error(
                    Code::TypeMismatch,
                    self.src.span(n),
                    "`impl Trait` must be the whole return type of a named function",
                );
                Ty::POISON
            }
            SyntaxKind::OptionalType => {
                let t = first_ty(self, n);
                pool.intern_ty(&TyData::Option(t))
            }
            SyntaxKind::MutType => {
                let t = first_ty(self, n);
                // A primitive has no `mut` form (types.prim.no-mut.error),
                // nor has a type parameter (types.value.no-mut.written); the
                // checker rejects the others but `Suspend[T]`.
                match pool.get(t) {
                    TyData::Prim(_) => {
                        self.diags.error(
                            Code::MutOnPrimitive,
                            self.src.span(n),
                            "a primitive type has no `mut` form",
                        );
                        t
                    }
                    TyData::Param(_) => {
                        self.diags.error(
                            Code::MutOnStruct,
                            self.src.span(n),
                            "a type parameter has no `mut` form",
                        );
                        t
                    }
                    _ => pool.intern_ty(&TyData::Mut(t)),
                }
            }
            SyntaxKind::ParenType => {
                self.ty_at(n.children().find(|c| c.kind().is_type()), gn, referent)
            }
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
                if !referent {
                    self.diags.error(
                        Code::TraitUsedAsType,
                        self.src.span(n),
                        "a trait value needs a reference type, `&dyn` or `&mut dyn`",
                    );
                    return Ty::POISON;
                }
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
                            inputs = if c.kind() == SyntaxKind::RestType {
                                // `types.tuple.rest.list`: a rest element
                                // is a `List`.
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
                let name = self.sym_token(t);
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
                // supertraits (`crates/hd_check/DESIGN.md` (trait solver) §4.1).
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
                if decls.is_empty() {
                    // No bound declares the name: an associated-type
                    // error at the projection, not a fabricated member
                    // with knock-on errors below
                    // (`trait.assoc.projection`).
                    if cands.is_empty() {
                        self.gap("a projection on a type without a bound naming it");
                    } else {
                        let msg = format!(
                            "`{}` names no associated type that any bound declares",
                            self.names.text(name)
                        );
                        self.diags
                            .error(Code::UnknownAssociatedType, self.src.span(n), &msg);
                    }
                    return Ty::POISON;
                }
                let (trait_, args, assoc) = decls[0];
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
            SyntaxKind::RefType => {
                // `ref.type.forms`: the reference's own `mut` says whether it
                // writes. A `mut` operand is the checker's `mut-on-struct`,
                // except the legacy `mut Suspend[T]`, whose `mut` a
                // reference drops.
                let mutable = n
                    .direct_token(&self.src.parse.tokens, TokenKind::KwMut)
                    .is_some();
                let inner = self.ty_at(n.children().find(|c| c.kind().is_type()), gn, true);
                let suspend = self.names.known.suspend;
                let inner = match pool.get(inner) {
                    TyData::Mut(i) if matches!(pool.get(i), TyData::Adt { def, .. } if def == suspend) => {
                        i
                    }
                    _ => inner,
                };
                pool.intern_ty(&TyData::Ref { inner, mutable })
            }
            _ => {
                self.gap("this type form");
                Ty::POISON
            }
        }
    }

    /// `req.row.param.no-data.marked`: the generic parameters of a data
    /// type, an enum, a trait or a newtype are type-kinded, so none is
    /// marked `$`.
    fn type_kinded(&mut self, gl: Option<NodeRef<'_>>, gn: &mut Gen) {
        gn.type_kinded = true;
        if self.r.frozen.is_some() {
            return;
        }
        let toks = &self.src.parse.tokens;
        for g in gl.iter().flat_map(|gl| gl.children()) {
            if g.kind() == SyntaxKind::GenericParameter
                && g.direct_token(toks, TokenKind::Dollar).is_some()
            {
                self.diags.error(
                    Code::GenericKindMismatch,
                    self.src.span(g),
                    "a data type, enum, trait or newtype declares no row parameter: drop the `$`",
                );
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
            let name = self.sym_token(t);
            let name_span = self.src.token_span(t);
            if self.r.frozen.is_none() {
                self.prelude_shadow(*g, self.src.text(t));
            }
            // No type parameter reuses the name of one in scope: an
            // enclosing implementation's or trait's, or an earlier one of
            // this list (`names.type-param.no-redeclare`).
            if self.r.frozen.is_none()
                && (gn.tys.iter().any(|(s, _, _)| *s == name)
                    || gn.rows.iter().any(|(s, _)| *s == name))
            {
                let msg = format!(
                    "`{}` is already a type parameter in scope",
                    self.src.text(t)
                );
                self.diags
                    .error(Code::DuplicateBinding, self.src.span(*g), &msg);
            }
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            self.names.paths.set_param_name(
                hd_base::PathId::from_raw(owner.raw()),
                index,
                self.src.text(t),
            );
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
                gn.decls.push((name, name_span));
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
                let me = (!row)
                    .then(|| gn.tys[start..].iter().find(|(s, _, _)| *s == g.name))
                    .flatten()
                    .map(|(_, t, _)| *t);
                for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                    // A bound that names no trait is already reported. It
                    // stays as poison, so that the parameter satisfies
                    // every trait and adds no second error.
                    let bound = self.trait_value(b, gn, me).unwrap_or(Ty::POISON);
                    // `Structure` is named only inside a template, never as
                    // a parameter bound (`annot.structure.named-positions`).
                    if self.r.frozen.is_none()
                        && self
                            .names
                            .pool
                            .trait_value(bound)
                            .is_some_and(|(def, _)| def == self.names.known.structure)
                    {
                        self.diags.error(
                            Code::StructureOutsideTemplate,
                            self.src.span(b),
                            "`Structure` may be named only inside a template",
                        );
                    }
                    bounds.push(bound);
                }
                if let Some(me) = me {
                    let resolved: Vec<Ty> = bounds
                        .iter()
                        .copied()
                        .filter(|b| *b != Ty::POISON)
                        .collect();
                    self.bound_clashes(me, &resolved, self.src.span(bl));
                }
            }
            if !row
                && Src::child(node, SyntaxKind::BoundList).is_some()
                && let Some((_, t, _)) = gn.tys[start..].iter().find(|(s, _, _)| *s == g.name)
            {
                let t = *t;
                gn.bounded.push(t);
            }
            g.bound = bounds
                .iter()
                .find(|b| **b != Ty::POISON)
                .and_then(|b| self.trait_of(*b));
            if !row && let Some(slot) = gn.tys[start..].iter_mut().find(|(s, _, _)| *s == g.name) {
                slot.2 = bounds
                    .iter()
                    .filter_map(|b| self.names.pool.trait_value(*b))
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
    fn result_required(&mut self, f: NodeRef<'_>, required: bool) {
        if required && self.r.frozen.is_none() && Src::type_child(f).is_none() {
            self.diags.error(
                Code::MissingResultType,
                self.src.span(f),
                "this declaration must state its result type: write `-> T`",
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
    fn sig(&mut self, f: NodeRef<'_>, owner: DefId, gn: &mut Gen) -> FnSig {
        gn.with_scope(|gn| self.sig_in_scope(f, owner, gn))
    }

    fn sig_in_scope(&mut self, f: NodeRef<'_>, owner: DefId, gn: &mut Gen) -> FnSig {
        let generics = self.generics(
            Src::child(f, SyntaxKind::GenericParameterList),
            owner,
            0,
            gn,
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
                if let Some(token) = p.direct_token(toks, TokenKind::KwSelfValue) {
                    let st = gn.self_ty.unwrap_or(Ty::POISON);
                    let pool = self.names.pool;
                    // `data.struct.mut-self`: a struct's `mut self` is its own
                    // changeable copy, of the struct type itself.
                    let own_copy =
                        matches!(pool.get(st), TyData::Adt { def, .. } if self.r.is_struct(def));
                    let mutable = p.direct_token(toks, TokenKind::KwMut).is_some();
                    // `fn.method.ref-self`: `&self` is `&Self` and
                    // `&mut self` is `&mut Self`, for every kind of type.
                    let st = if p.direct_token(toks, TokenKind::Amp).is_some() {
                        pool.intern_ty(&TyData::Ref { inner: st, mutable })
                    } else if mutable && !own_copy {
                        pool.intern_ty(&TyData::Mut(st))
                    } else {
                        st
                    };
                    params.push((self.sym_token(token), st));
                    defaults.push(false);
                    continue;
                }
                if p.direct_token(toks, TokenKind::Ellipsis).is_some() {
                    variadic = true;
                }
                let token = p.name(toks);
                let name = token.map_or_else(|| i.to_string(), |t| self.src.text(t).to_owned());
                let sym = token.map_or_else(|| self.sym(&name), |t| self.sym_token(t));
                if self.r.frozen.is_none() {
                    self.prelude_shadow(p, &name);
                    // `names.type-param.no-redeclare`: no parameter reuses
                    // the name of a type parameter in scope, the
                    // function's own or an enclosing one's.
                    if gn.tys.iter().any(|(s, _, _)| *s == sym)
                        || gn.rows.iter().any(|(s, _)| *s == sym)
                    {
                        let msg = format!("`{name}` is a type parameter in scope");
                        self.diags
                            .error(Code::DuplicateBinding, self.src.span(p), &msg);
                    }
                    // `names.fn.params.scope`: value parameters have
                    // distinct names.
                    if params.iter().any(|(s, _)| *s == sym) {
                        let msg = format!("`{name}` names an earlier parameter");
                        self.diags
                            .error(Code::DuplicateBinding, self.src.span(p), &msg);
                    }
                }
                let ty = self.ty(Src::type_child(p), gn);
                // `fn.vararg.type.kinds`: a vararg's type is a `List`, a
                // tuple type, or a type parameter bounded by `Tuple`,
                // under `mut` as elsewhere.
                if p.direct_token(toks, TokenKind::Ellipsis).is_some() {
                    let inner = match self.names.pool.get(ty) {
                        TyData::Mut(i) => i,
                        _ => ty,
                    };
                    let list = self.names.known.list;
                    let is_list = matches!(self.names.pool.get(inner), TyData::Adt { def, .. } if def == list);
                    if !is_list && inner != Ty::POISON && !self.inputs_tuple(inner, gn) {
                        self.diags.error(
                            Code::TypeMismatch,
                            self.src.span(p),
                            "a vararg's type must be a `List`, a tuple type, or a type parameter bounded by `Tuple`",
                        );
                    }
                }
                params.push((sym, ty));
                defaults.push(Src::child(p, SyntaxKind::DefaultValue).is_some());
            }
        }
        let ret = match Src::type_child(f) {
            Some(n) if n.kind() == SyntaxKind::ImplType => {
                let bound = Src::child(n, SyntaxKind::NamedType)
                    .and_then(|n| self.trait_value(n, gn, None))
                    .unwrap_or(Ty::POISON);
                let args: Vec<Ty> = gn
                    .decls
                    .iter()
                    .filter_map(|(name, _)| {
                        if let Some((_, r)) = gn.rows.iter().find(|(s, _)| s == name) {
                            return Some(self.names.pool.intern_ty(&TyData::Row(
                                self.names.pool.row(&hd_types::RowData {
                                    keys: vec![],
                                    params: vec![*r],
                                }),
                            )));
                        }
                        gn.tys
                            .iter()
                            .find(|(s, _, _)| s == name)
                            .map(|(_, t, _)| *t)
                    })
                    .collect();
                self.names.pool.intern_ty(&TyData::Opaque {
                    owner,
                    site: 0,
                    args: self.names.pool.list(&args),
                    bound,
                })
            }
            n => self.ty(n, gn),
        };
        let row = self.row(Src::child(f, SyntaxKind::RequirementRow), gn);
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
                    let token = f.name(&self.src.parse.tokens);
                    let name = token.map_or_else(|| i.to_string(), |t| self.src.text(t).to_owned());
                    let ty = self.ty(Src::type_child(f), gn);
                    out.push(Field {
                        name: token.map_or_else(|| self.sym(&name), |t| self.sym_token(t)),
                        ty,
                        public: self.src.is_pub(f),
                        has_default: Src::child(f, SyntaxKind::DefaultValue).is_some(),
                        embedded: false,
                    });
                }
                SyntaxKind::EmbeddedField => {
                    let tn = Src::type_child(f);
                    let token = tn
                        .map(|t| self.segments(t))
                        .and_then(|s| s.last().map(|x| x.1));
                    let ty = self.ty(tn, gn);
                    out.push(Field {
                        name: token.map_or_else(|| self.sym(""), |t| self.sym_token(t)),
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

    /// Whether each generic parameter of a declaration is a row parameter.
    fn generic_rows(&self, def: DefId) -> Vec<bool> {
        match self.r.row_params.get(&def) {
            Some(rows) => rows.clone(),
            None => self
                .r
                .item(def)
                .map(|i| i.generics.iter().map(|g| g.row).collect())
                .unwrap_or_default(),
        }
    }

    /// The field nodes under `holder`, in the order [`Self::fields`] lowers
    /// them.
    fn field_nodes(holder: Option<NodeRef<'_>>) -> impl Iterator<Item = NodeRef<'_>> {
        holder.into_iter().flat_map(NodeRef::children).filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::DataField | SyntaxKind::Parameter | SyntaxKind::EmbeddedField
            )
        })
    }

    /// The shape rules of a data type's fields: names are unique
    /// (`data.field.unique`, `data.embed.unique`), and a type embeds at
    /// most three fields (`data.embed.width`).
    fn data_fields(&mut self, holder: Option<NodeRef<'_>>, fields: &[Field]) {
        let mut first: HashMap<Symbol, usize> = HashMap::default();
        let mut embedded = 0;
        for (i, (node, f)) in Self::field_nodes(holder).zip(fields).enumerate() {
            let span = self.src.span(node);
            let name = self.names.text(f.name).to_owned();
            if let Some(&j) = first.get(&f.name) {
                let code = if f.embedded || fields[j].embedded {
                    Code::DuplicateEmbeddedField
                } else {
                    Code::DuplicateField
                };
                let msg = format!("the field `{name}` is declared twice");
                self.diags.error(code, span, &msg);
            } else {
                first.insert(f.name, i);
            }
            if f.embedded {
                embedded += 1;
                if embedded == 4 {
                    let msg =
                        format!("`{name}` is a fourth embedded field; a type embeds at most three");
                    self.diags.error(Code::TooManyEmbeddedFields, span, &msg);
                }
            }
        }
    }

    /// A named parameter under `holder` that repeats an earlier one of the
    /// same list (`data.field.unique`); `what` names the list in the error.
    fn repeated_names(&mut self, holder: Option<NodeRef<'_>>, what: &str) {
        let mut seen: HashSet<String> = HashSet::default();
        for p in Self::field_nodes(holder) {
            let Some(pt) = p.name(&self.src.parse.tokens) else {
                continue;
            };
            let name = self.src.text(pt).to_owned();
            if !seen.insert(name.clone()) {
                let msg = format!("{what} `{name}` is declared twice");
                self.diags
                    .error(Code::DuplicateField, self.src.span(p), &msg);
            }
        }
    }

    /// A shared parameter whose type names a type parameter
    /// (`data.shared.type-parameter`) is an error when `report` is set, and
    /// its type becomes poison either way, so later stages stay silent.
    fn shared_type_parameters(
        &mut self,
        shared: &mut [Field],
        shared_node: Option<NodeRef<'_>>,
        report: bool,
    ) {
        for (n, f) in Self::field_nodes(shared_node).zip(shared.iter_mut()) {
            if !self.names.pool.has_param(f.ty) {
                continue;
            }
            f.ty = Ty::POISON;
            if report {
                let name = self.names.text(f.name).to_owned();
                let msg = format!(
                    "the shared field `{name}` names a type parameter, but shared data has one copy per variant"
                );
                self.diags
                    .error(Code::SharedDataTypeParameter, self.src.span(n), &msg);
            }
        }
    }

    /// An enum's variants are unique (`data.enum.unique`), and a variant's
    /// named payload parameter does not repeat a named shared parameter
    /// (`data.shared.payload-names`).
    fn enum_names(
        &mut self,
        shared: &[Field],
        shared_node: Option<NodeRef<'_>>,
        block: Option<NodeRef<'_>>,
    ) {
        let shared_named: HashSet<Symbol> = Self::field_nodes(shared_node)
            .zip(shared)
            .filter(|(n, _)| n.name(&self.src.parse.tokens).is_some())
            .map(|(_, f)| f.name)
            .collect();
        self.repeated_names(shared_node, "the shared parameter");
        let mut seen = HashSet::default();
        for v in block
            .iter()
            .flat_map(|b| b.children())
            .filter(|c| c.kind() == SyntaxKind::EnumVariant)
        {
            let Some(t) = v.name(&self.src.parse.tokens) else {
                continue;
            };
            let text = self.src.text(t).to_owned();
            if !seen.insert(text.clone()) {
                let msg = format!("the variant `{text}` is declared twice");
                self.diags
                    .error(Code::DuplicateVariant, self.src.span(v), &msg);
            }
            self.repeated_names(
                Src::child(v, SyntaxKind::ParameterList),
                "the payload parameter",
            );
            for p in Self::field_nodes(Src::child(v, SyntaxKind::ParameterList)) {
                let Some(pt) = p.name(&self.src.parse.tokens) else {
                    continue;
                };
                let name = self.src.text(pt).to_owned();
                if shared_named.contains(&self.sym(&name)) {
                    let msg = format!(
                        "the payload parameter `{name}` repeats a shared parameter of the enum"
                    );
                    self.diags
                        .error(Code::DuplicateField, self.src.span(p), &msg);
                }
            }
        }
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
        (self_ty, gn): (Ty, &mut Gen),
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

    /// `result_required`: each method declares its result type, as a
    /// trait's methods and a trait implementation's do
    /// (`fn.decl.result-required-pub`).
    fn members(
        &mut self,
        block: Option<NodeRef<'_>>,
        owner: DefId,
        public: bool,
        result_required: bool,
        gn: &mut Gen,
        out: &mut Vec<Item>,
    ) -> (Vec<(Symbol, DefId)>, Vec<AssocDecl>) {
        let mut methods = Vec::new();
        let mut assoc = Vec::new();
        for f in block.iter().flat_map(|b| b.children()) {
            match f.kind() {
                SyntaxKind::FnDecl => {
                    let Some(t) = member_name(&self.src, f) else {
                        continue;
                    };
                    let text = self.src.text(t).to_owned();
                    let name = self.sym_token(t);
                    let def = self.names.member(owner, PathKind::Member, &text);
                    self.result_required(f, result_required || self.src.is_pub(f));
                    if result_required
                        && Src::type_child(f).is_some_and(|n| n.kind() == SyntaxKind::ImplType)
                    {
                        self.diags.error(
                            Code::TypeMismatch,
                            self.src.span(f),
                            "a trait method cannot return `impl Trait` yet",
                        );
                    }
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
                    let mut bounds = Vec::new();
                    if let Some(bl) = Src::child(f, SyntaxKind::BoundList) {
                        for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                            if let Some(t) = self.trait_value(b, gn, gn.self_ty) {
                                bounds.push(t);
                            }
                        }
                    }
                    assoc.push((self.sym_token(t), def, ty, bounds));
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
        let mut seen = HashSet::default();
        let mut bad = Vec::new();
        for f in block.iter().flat_map(|b| b.children()) {
            if !matches!(
                f.kind(),
                SyntaxKind::FnDecl | SyntaxKind::AssociatedTypeDecl
            ) {
                continue;
            }
            let Some(t) = member_name(&self.src, f) else {
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

    /// The literal marker of one decorator, resolved through the module's
    /// scope rather than matched by its written first segment.
    fn literal_marker(&self, d: NodeRef<'_>) -> Option<u8> {
        let def = crate::facts::decorator_def(
            &self.src,
            &self.names,
            self.scope,
            &|module, name| self.r.export(module, name).ok().map(|x| x.0),
            d,
        )?;
        if def == self.names.known.num_suffix {
            Some(crate::iface::LITERAL_SUFFIX)
        } else if def == self.names.known.str_prefix {
            Some(crate::iface::LITERAL_PREFIX)
        } else {
            None
        }
    }

    /// `@num_suffix` or `@str_prefix` before a function.
    fn literal_fn(&self, n: NodeRef<'_>) -> u8 {
        n.children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .find_map(|d| self.literal_marker(d))
            .unwrap_or(0)
    }

    fn intrinsic(&self, n: NodeRef<'_>) -> Option<Symbol> {
        self.decorators(n)
            .into_iter()
            .find(|(d, _)| d == "intrinsic")
            .and_then(|(_, a)| a.first().map(|k| self.sym(k)))
    }

    /// Reports a user `impl` of the sealed trait `trait_`, on the `impl`
    /// (`trait.sealed.user-impl`).
    fn sealed_impl_rejected(&mut self, imp: NodeRef<'_>, trait_: DefId) {
        if self.r.frozen.is_some() {
            return;
        }
        let msg = format!(
            "`{}` is sealed: only the compiler and the standard library implement it",
            self.names.display_name(trait_)
        );
        self.diags
            .error(Code::SealedTraitImplementation, self.src.span(imp), &msg);
    }

    fn poisoned_impl(h: &Head<'_>, self_ty: Ty, generics: Vec<Generic>, out: &mut Vec<Item>) {
        let mut item = Item::new(
            h.def,
            h.name,
            true,
            ItemData::Impl {
                trait_: DefId::NONE,
                trait_args: TyList::EMPTY,
                self_ty,
                methods: vec![],
                assoc: vec![],
                by: None,
                kind: ImplKind::Poison,
            },
        );
        item.generics = generics;
        out.push(item);
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
        // The rejected implementation is left out, so nothing else is
        // reported about it.
        if target.kind() == SyntaxKind::MutType {
            if self.r.frozen.is_none() {
                self.diags.error(
                    Code::MutableImplTarget,
                    self.src.span(target),
                    "an implementation target cannot be a `mut` view",
                );
            }
            return;
        }
        let self_ty = self.ty_at(Some(target), &gn, true);
        gn.self_ty = Some(self_ty);
        let toks: Vec<_> = n.direct_tokens().collect();
        let by = toks
            .iter()
            .position(|t| self.src.tkind(*t) == Some(TokenKind::Ident) && self.src.text(*t) == "by")
            .and_then(|i| toks.get(i + 1))
            .map(|t| self.src.text(*t).to_owned());
        // `annot.block.structure-use`: `by Structure` must name
        // `std.structure.Structure`, imported with `use` like any other
        // name.
        if by.as_deref() == Some("Structure")
            && self.r.frozen.is_none()
            && !names_std_structure(&self.names, self.scope)
        {
            let at = toks
                .iter()
                .position(|t| {
                    self.src.tkind(*t) == Some(TokenKind::Ident) && self.src.text(*t) == "Structure"
                })
                .map_or_else(|| self.src.span(n), |i| self.src.token_span(toks[i]));
            self.diags.error(
                Code::UnknownTrait,
                at,
                "`Structure` is unknown without `use std.structure.Structure`",
            );
        }
        let bare_param = matches!(pool.get(self_ty), TyData::Param(p) if p.owner == h.def);
        // An inherent implementation cannot target a tuple or a transparent
        // alias, whichever package owns the type it names
        // (`trait.own.inherent.tuple-alias`).
        if trait_node.is_none() && by.is_none() && self.tuple_or_alias_target(target, self_ty, n) {
            if self.r.frozen.is_none() {
                self.diags.error(
                    Code::InvalidImplTarget,
                    self.src.span(n),
                    "an inherent implementation cannot target a tuple or a transparent alias",
                );
            }
            return;
        }
        let (trait_, trait_args, header_bindings) = match trait_node {
            None => (DefId::NONE, TyList::EMPTY, Vec::new()),
            Some(tn) => {
                let segs = self.segments(tn);
                if segs
                    .first()
                    .is_some_and(|s| self.is_poisoned(self.sym_token(s.1)))
                {
                    Self::poisoned_impl(h, self_ty, generics, out);
                    return;
                }
                let found = self.resolve_path(&segs, self.src.span(tn));
                // `Suspend[T]` is the compiler's suspension type, not a
                // trait (`req.protocol.builtin`).
                if let Some((def, kind)) = found
                    && kind != HeadKind::Trait
                    && self.names.known.is_sealed(def)
                    && !self.in_std()
                {
                    if self.r.frozen.is_none() {
                        let msg = format!(
                            "no trait named `{}`: it is a built-in type, not a trait",
                            self.names.display_name(def)
                        );
                        self.diags
                            .error(Code::UnknownTrait, self.src.span(tn), &msg);
                    }
                    return;
                }
                let Some(tv) = self.trait_value_from(tn, &gn, Some(self_ty), found) else {
                    Self::poisoned_impl(h, self_ty, generics, out);
                    return;
                };
                match pool.get(tv) {
                    TyData::TraitValue {
                        def,
                        args,
                        bindings,
                    } => (def, args, bindings),
                    _ => return,
                }
            }
        };
        // Only the compiler and the standard library implement a sealed
        // trait (`trait.sealed.user-impl`). The rejected implementation
        // is left out, so nothing else is reported about it.
        if self.names.known.is_sealed(trait_) && !self.in_std() {
            self.sealed_impl_rejected(n, trait_);
            return;
        }
        // A target is a type constructor, never a bare type parameter
        // (`trait.target.bare-parameter`). A derivation template is no
        // implementation (`trait.target.template`), and a standard-library
        // numeric family is the one exception (`trait.target.numeric-family`).
        if bare_param
            && by.as_deref() != Some("Structure")
            && !(self.in_std() && self.numeric_family(&generics))
        {
            if self.r.frozen.is_none() {
                self.diags.error(
                    Code::BareParameterImplTarget,
                    self.src.span(target),
                    "an implementation target must start with a type constructor, not a bare type parameter",
                );
            }
            return;
        }
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
        // The package and module that may declare the implementation
        // (`trait.own.*`). The rejected implementation is left out, so no
        // overlap or body error follows it.
        let session = self
            .r
            .session_modules
            .get(self.module)
            .map_or(&[][..], Vec::as_slice);
        if let Some(code) = impl_placement(
            &self.names,
            (self.module, session),
            (trait_, trait_args, self_ty),
            kind,
        ) {
            if self.r.frozen.is_none() {
                let name = self.names.display_name(h.def);
                let msg = if code == Code::NonlocalImpl {
                    format!("`{name}` belongs in the module of its trait or target")
                } else {
                    format!("`{name}` is in a package that owns neither its trait nor its target")
                };
                self.diags.error(code, self.src.span(n), &msg);
            }
            return;
        }
        if self.r.frozen.is_none() {
            let head = ImplHead {
                target,
                trait_args,
                self_ty,
            };
            self.impl_head_rules(h, &generics, &head, trait_node.is_some());
        }
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
        let (methods, assoc_decls) = self.members(
            block,
            h.def,
            trait_ != DefId::NONE,
            trait_ != DefId::NONE,
            &mut gn,
            out,
        );
        if trait_ == DefId::NONE
            && self.r.frozen.is_none()
            && let Some(b) = block
        {
            self.impl_variance(h.def, &generics, (self_ty, &mut gn), b);
        }
        let mut assoc: Vec<(DefId, Ty)> = header_bindings;
        for (name, _, ty, _) in assoc_decls {
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
        it.fills_defaults = self.fills_defaults(h.node);
        it.skips = self.skips(h.node, &gn);
        out.push(it);
    }

    /// Whether every type parameter is bounded by exactly one of `Num`,
    /// `Integer` and `Float`: the standard library's numeric families
    /// (`trait.target.numeric-family`).
    fn numeric_family(&self, generics: &[Generic]) -> bool {
        let known = &self.names.known;
        generics.iter().filter(|g| !g.row).all(|g| {
            matches!(g.bounds.as_slice(), [b]
                if self.trait_of(*b).is_some_and(|t| [known.num, known.integer, known.float].contains(&t)))
        })
    }

    /// Whether the target of an inherent implementation is a tuple or a
    /// transparent alias. A primitive name is no alias, whatever the
    /// prelude binds it to.
    fn tuple_or_alias_target(
        &mut self,
        target: NodeRef<'_>,
        self_ty: Ty,
        imp: NodeRef<'_>,
    ) -> bool {
        let segs = self.segments(target);
        let alias = target.kind() == SyntaxKind::NamedType
            && matches!(segs.as_slice(), [(first, _)]
                if !Prim::ALL.iter().any(|p| p.name() == first)
                    && self.resolve_path(&segs, self.src.span(imp)).is_some_and(|(_, k)| k == HeadKind::Alias));
        alias || matches!(self.names.pool.get(self_ty), TyData::Tuple { .. })
    }

    /// What an implementation head must satisfy: no trait value type or
    /// row extension as a target (`trait.target.trait-value.error`,
    /// `trait.target.row-argument.extension`), and every type parameter
    /// constrained by the head (`trait.overlap.constrained-head`,
    /// `trait.overlap.head-projection`).
    fn impl_head_rules(
        &mut self,
        h: &Head<'_>,
        generics: &[Generic],
        head: &ImplHead<'_>,
        has_trait: bool,
    ) {
        let pool = self.names.pool;
        let span = self.src.span(h.node);
        let args: Vec<Ty> = pool.list_items(head.trait_args).to_vec();
        let roots: Vec<Ty> = args.iter().copied().chain([head.self_ty]).collect();
        let mut impl_target = head.self_ty;
        while let TyData::Ref { inner, .. } = pool.get(impl_target) {
            impl_target = inner;
        }
        if has_trait && matches!(pool.get(impl_target), TyData::TraitValue { .. }) {
            self.diags.error(
                Code::TraitValueImplTarget,
                self.src.span(head.target),
                "a trait value type is never an implementation target",
            );
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
        let mut constrained: HashSet<u16> = HashSet::default();
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
                if self.is_poisoned(sym) {
                    continue;
                }
                let Some(Binding {
                    kind: BindingKind::Item,
                    value,
                }) = self.scope.lookup(sym)
                else {
                    let hint = if self.scope.lookup(sym).is_none() {
                        self.import_hint(&a)
                    } else {
                        String::new()
                    };
                    let msg = format!("no trait named `{a}`{hint}");
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
                let params: Vec<Ty> = (0..generics.len())
                    .map(|i| {
                        let index = u16::try_from(i).unwrap_or(u16::MAX);
                        pool.intern_ty(&TyData::Param(ParamRef { owner: def, index }))
                    })
                    .collect();
                // The type's own parameters, moved to the implementation's.
                let own = |t: Ty| {
                    pool.subst(t, &|p: ParamRef| {
                        (p.owner == h.def).then(|| params.get(usize::from(p.index)).copied())?
                    })
                };
                let mut gs = Vec::new();
                for (g, &param) in generics.iter().zip(&params) {
                    let mut g2 = Generic::plain(g.name);
                    if !g.row {
                        // The bounds the type declares on the parameter
                        // stay (`annot.bound.declared`), then `T < X`
                        // unless one of them already is it.
                        let derived = pool.intern_ty(&TyData::TraitValue {
                            def: tr,
                            args: self.r.fill_trait_args(tr, TyList::EMPTY, Some(param)),
                            bindings: vec![],
                        });
                        g2.bounds = g.bounds.iter().map(|b| own(*b)).collect();
                        if !g2.bounds.contains(&derived) {
                            g2.bounds.push(derived);
                        }
                        g2.bound = g.bound.or(Some(tr));
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
    /// (`crates/hd_mono/DESIGN.md` §13.14). `fields` are each variant's members (a data
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
        let cause_ref = pool.intern_ty(&TyData::Ref {
            inner: error_ty,
            mutable: false,
        });
        let cause_ty = pool.intern_ty(&TyData::Option(cause_ref));
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
        self.names
            .paths
            .set_param_name(hd_base::PathId::from_raw(h.def.raw()), 0, "Self");
        let mut gn = Gen {
            self_ty: Some(self_ty),
            self_trait: Some(h.def),
            ..Gen::default()
        };
        let gl = Src::child(h.node, SyntaxKind::GenericParameterList);
        self.type_kinded(gl, &mut gn);
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
                    // A supertrait bound lists a trait once
                    // (`trait.super.unique`).
                    if self.r.frozen.is_none() && supers.contains(&t) {
                        let msg = format!(
                            "the supertrait `{}` is listed twice",
                            show_ty(&self.names, t)
                        );
                        self.diags
                            .error(Code::DuplicateSupertrait, self.src.span(b), &msg);
                    }
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
                let sig = self.sig(n, h.def, &mut Gen::default());
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Fn(sig));
                it.intrinsic = self.intrinsic(n);
                it.literal_fn = self.literal_fn(n);
                it.has_facts =
                    !crate::facts::fact_lines(&self.src, n, |d| self.literal_marker(d).is_some())
                        .is_empty();
                out.push(it);
            }
            HeadKind::Data => {
                let mut gn = Gen::default();
                self.type_kinded(gl, &mut gn);
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let fields = self.fields(block, &gn);
                if self.r.frozen.is_none() {
                    self.field_variance(h.def, &generics, block, &fields);
                    self.data_fields(block, &fields);
                }
                self.derived(h, &generics, out);
                self.error_impls(h, &generics, &[fields.as_slice()], out);
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Data(fields));
                it.is_struct = n
                    .direct_token(&self.src.parse.tokens, TokenKind::KwStruct)
                    .is_some();
                it.generics = generics;
                it.targets = self.annotate_mask(n);
                it.fact_pattern = self.annotate_pattern(n, &gn);
                out.push(it);
            }
            HeadKind::Enum => {
                let mut gn = Gen::default();
                self.type_kinded(gl, &mut gn);
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let shared_params = Src::child(n, SyntaxKind::ParameterList);
                let nodes: Vec<NodeRef<'_>> = shared_params
                    .iter()
                    .flat_map(|pl| pl.children())
                    .filter(|c| c.kind() == SyntaxKind::Parameter)
                    .collect();
                self.parameter_default_order(&nodes, false);
                let mut shared = self.fields(shared_params, &gn);
                let check = self.r.frozen.is_none();
                let holder = Src::child(n, SyntaxKind::ParameterList);
                self.shared_type_parameters(&mut shared, holder, check);
                if check {
                    self.field_variance(h.def, &generics, holder, &shared);
                    self.enum_names(&shared, holder, block);
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
                        name: self.sym_token(t),
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
                it.fact_pattern = self.annotate_pattern(n, &gn);
                out.push(it);
            }
            HeadKind::Trait => {
                let (self_ty, mut gn, generics) = self.trait_generics(h);
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
                let (methods, assoc_decls) =
                    self.members(block, h.def, h.public, true, &mut gn, out);
                let mut assoc = Vec::new();
                for (name, def, ty, bounds) in assoc_decls {
                    out.push(Item::new(
                        def,
                        name,
                        h.public,
                        ItemData::AssocType {
                            owner: h.def,
                            bounds,
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
                self.type_kinded(gl, &mut gn);
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
        if let Some(item) = out.iter_mut().rev().find(|item| item.def == h.def) {
            self.declaration_docs(n, &mut item.docs, &[]);
        }
    }

    /// Attached docs use the same normalization and decorator handling as `hd_doc`.
    fn declaration_docs(
        &self,
        node: NodeRef<'_>,
        out: &mut Vec<(Vec<String>, String)>,
        path: &[String],
    ) {
        let first = node.first_token();
        let head = node.direct_tokens().next().unwrap_or(first);
        let mut parts = vec![hd_syntax::doc_text(
            self.src.text,
            &self.src.parse.tokens,
            first,
        )];
        if head != first {
            parts.push(hd_syntax::doc_text(
                self.src.text,
                &self.src.parse.tokens,
                head,
            ));
        }
        parts.retain(|part| !part.is_empty());
        let text = parts.join("\n");
        if !text.is_empty() {
            out.push((path.to_vec(), text));
        }
        for holder in node
            .children()
            .filter(|n| matches!(n.kind(), SyntaxKind::Block | SyntaxKind::ParameterList))
        {
            for (index, child) in holder.children().enumerate() {
                if !matches!(
                    child.kind(),
                    SyntaxKind::DataField
                        | SyntaxKind::Parameter
                        | SyntaxKind::EmbeddedField
                        | SyntaxKind::EnumVariant
                ) {
                    continue;
                }
                let name = if child.kind() == SyntaxKind::EmbeddedField {
                    Src::type_child(child)
                        .map(|t| self.segments(t))
                        .and_then(|s| s.last().map(|x| x.0.clone()))
                        .unwrap_or_default()
                } else {
                    child
                        .name(&self.src.parse.tokens)
                        .map_or_else(|| index.to_string(), |t| self.src.text(t).to_owned())
                };
                let mut next = path.to_vec();
                next.push(name);
                self.declaration_docs(child, out, &next);
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
        self.r
            .own_types
            .borrow_mut()
            .insert(h.def, generics.clone());
        let mut it = Item::new(h.def, h.name, h.public, ItemData::Alias(t));
        it.generics = generics;
        out.push(it);
    }

    /// The folder declarations (by index in `index`) that the part `scan`
    /// of the declaration `n` names, other than through its own generic
    /// parameters.
    fn named_in_order(
        &mut self,
        (n, scan): (NodeRef<'_>, NodeRef<'_>),
        index: &HashMap<DefId, usize>,
    ) -> Vec<usize> {
        let params: Vec<&str> = Src::child(n, SyntaxKind::GenericParameterList)
            .iter()
            .flat_map(|gl| gl.children())
            .filter(|g| g.kind() == SyntaxKind::GenericParameter)
            .filter_map(|g| g.name(&self.src.parse.tokens))
            .map(|t| self.src.text(t))
            .collect();
        let mut out = Vec::new();
        for c in scan
            .descendants()
            .filter(|c| c.kind() == SyntaxKind::NamedType)
        {
            let segs = self.segments(c);
            let Some((first, token)) = segs.first() else {
                continue;
            };
            if (segs.len() == 1 && params.contains(&first.as_str()))
                || self.is_poisoned(self.sym_token(*token))
            {
                continue;
            }
            if let Some((d, _)) = self.resolve_path(&segs, self.src.span(c))
                && let Some(&i) = index.get(&d)
            {
                out.push(i);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The declared parameters of a data type, enum or newtype, recorded
    /// for the written types that count or fill them
    /// (`types.generic.too-long`, `types.generic.default.written`). The item's own
    /// lowering reports their diagnostics, so these are dropped.
    fn record_defaults(&mut self, h: &Head<'_>) {
        self.at = self.src.span(h.node).lo;
        self.outer = h
            .local
            .map(|l| enclosing_generics(&self.src, l.top, self.at))
            .unwrap_or_default();
        let gl = Src::child(h.node, SyntaxKind::GenericParameterList);
        let generics = self.generics(gl, h.def, 0, &mut Gen::default());
        self.r.own_types.borrow_mut().insert(h.def, generics);
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

/// Whether `h` is a data type, enum or newtype, whose parameters are
/// recorded before any signature is lowered.
fn declares_params(h: &Head<'_>) -> bool {
    matches!(h.kind, HeadKind::Data | HeadKind::Enum | HeadKind::Newtype)
}

/// Lowers every alias of the folder before any other header, each after
/// the aliases its declaration names, whatever the declaration order
/// (`crates/hd_resolve/DESIGN.md`, "Row aliases and context types in
/// interfaces"). Every data type, enum and newtype takes part in that
/// order, for its generic parameters alone: a written type counts its
/// arguments against them (`types.generic.too-long`) and fills the omitted
/// ones from their defaults (`types.generic.default.written`), so an alias
/// that names it comes after the aliases its defaults name.
/// An alias that expands to itself is `alias-cycle`, reported once per
/// cycle, on its declaration that comes first in the source
/// (`types.alias.cycle.reported`). Returns each module's alias items and
/// the first unsupported form met.
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
                .filter(|h| h.kind == HeadKind::Alias || declares_params(h))
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
                kind_reported: HashSet::default(),
                locals: &locals[mi],
                at: mods[mi].src.span(h.node).lo,
                outer: Vec::new(),
            };
            let scan = match h.kind {
                HeadKind::Alias => Some(h.node),
                _ => Src::child(h.node, SyntaxKind::GenericParameterList),
            };
            scan.map_or_else(Vec::new, |scan| low.named_in_order((h.node, scan), &index))
        })
        .collect();
    let is_alias = |i: usize| sites[i].1.kind == HeadKind::Alias;
    let mut out: Vec<Vec<Item>> = (0..mods.len()).map(|_| Vec::new()).collect();
    let mut unsupported = None;
    for comp in components(&edges) {
        let cyclic = comp.iter().any(|&i| is_alias(i))
            && (comp.len() > 1 || edges[comp[0]].contains(&comp[0]));
        let first = comp.iter().copied().filter(|&i| is_alias(i)).min();
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
            if cyclic && Some(i) == first {
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
            let mut dropped = DiagBuf::default();
            let alias = h.kind == HeadKind::Alias;
            let mut low = Lower {
                r,
                names: r.cx.names,
                src,
                module: &mods[mi].path,
                scope,
                kinds: &*kinds,
                diags: if alias { diags } else { &mut dropped },
                unsupported: None,
                kind_reported: HashSet::default(),
                locals: &locals[mi],
                at: 0,
                outer: Vec::new(),
            };
            if alias {
                low.alias_item(h, cyclic, &mut out[mi]);
                if unsupported.is_none() {
                    unsupported = low.unsupported.take();
                }
            } else {
                low.record_defaults(h);
            }
        }
    }
    (out, unsupported)
}

/// The written head of an implementation, as `impl_head_rules` reads it.
struct ImplHead<'t> {
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
        TyData::Option(i) | TyData::Mut(i) | TyData::Ref { inner: i, .. } => kids.push(i),
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
                .filter_map(|t| pool.trait_value(*t))
                .filter_map(|(def, _)| index.get(&def).copied())
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

/// The placement error of an implementation head, if it is outside its
/// owners (`trait.own.*`): `OrphanImpl` when no owner is in the impl's
/// package, `NonlocalImpl` when one is but not in this module. An
/// inherent implementation is owned by its target alone. `None` for the
/// standard library, and for what is no implementation of its own: a
/// template or a derivation block.
///
/// `session` holds the modules that count as `module` too: for a REPL
/// input, `src/lib.hd` and the earlier inputs, since a session is one
/// module ([`SessionScope::modules`]); empty for any other module.
#[must_use]
pub fn impl_placement(
    names: &Names<'_>,
    (module, session): (&str, &[String]),
    (trait_, trait_args, self_ty): (DefId, TyList, Ty),
    kind: ImplKind,
) -> Option<Code> {
    let pool = names.pool;
    let package = module.split('.').next().unwrap_or("");
    if package == "std" {
        return None;
    }
    let module = names.module(module);
    let session: Vec<PathId> = session.iter().map(|m| names.module(m)).collect();
    let package = names.package_node(DefId::from_raw(module.raw()));
    let ctor_module = |t: Ty| -> Option<PathId> {
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        match pool.get(t) {
            TyData::Adt { def, .. } | TyData::TraitValue { def, .. } => {
                Some(names.module_node(def))
            }
            TyData::Prim(_)
            | TyData::Tuple { .. }
            | TyData::Option(_)
            | TyData::Fn { .. }
            | TyData::Ref { .. } => Some(names.module("std.core")),
            _ => None,
        }
    };
    let mut owners = Vec::new();
    if trait_ == DefId::NONE {
        // `trait.own.inherent`, `trait.own.module.inherent-target`: only
        // a written inherent block, and only the target's own module.
        if kind != ImplKind::Written {
            return None;
        }
        owners.extend(ctor_module(self_ty));
    } else {
        if !kind.is_impl() {
            return None;
        }
        owners.push(names.module_node(trait_));
        owners.extend(ctor_module(self_ty));
        owners.extend(
            pool.list_items(trait_args)
                .iter()
                .copied()
                .filter_map(ctor_module),
        );
    }
    if owners.iter().any(|o| *o == module || session.contains(o)) {
        return None;
    }
    let local = owners
        .iter()
        .any(|o| names.package_node(DefId::from_raw(o.raw())) == package);
    Some(if local {
        Code::NonlocalImpl
    } else {
        Code::OrphanImpl
    })
}

/// Whether a syntax node is `impl X by Structure`, a derivation block with
/// no trait (`annot.no-trait.form`).
#[must_use]
pub fn traitless_block(src: &Src<'_>, n: NodeRef<'_>) -> bool {
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

/// Whether `a` and `b` are declared in the same module, where all the
/// modules of `session` count as one: for a REPL input, the input itself,
/// `src/lib.hd` and the earlier inputs ([`SessionScope::modules`]). Empty
/// for a program and any other module.
fn same_module(names: &Names<'_>, session: &[PathId], a: DefId, b: DefId) -> bool {
    let (x, y) = (names.module_node(a), names.module_node(b));
    x == y || (session.contains(&x) && session.contains(&y))
}

/// Whether an item is a template whose trait is declared in another module
/// (`annot.template.module`), a session counting as one module.
#[must_use]
pub fn misplaced_template(names: &Names<'_>, session: &[PathId], it: &Item) -> bool {
    matches!(
        &it.data,
        ItemData::Impl { trait_, kind: ImplKind::Template | ImplKind::TupleTemplate, .. }
            if *trait_ != DefId::NONE && !same_module(names, session, it.def, *trait_)
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
        let mut by_def =
            HashMap::with_capacity_and_hasher(items.len(), hd_base::FixedState::default());
        let mut by_target: HashMap<DefId, Vec<usize>> = HashMap::default();
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
/// the items of the block's module; a target in another module of `session`
/// is not among them and goes unchecked here. Rejected blocks stay out of
/// the derivation checks.
#[must_use]
pub fn misplaced_block(
    names: &Names<'_>,
    session: &[PathId],
    items: &[Item],
    it: &Item,
) -> Option<&'static str> {
    misplaced_block_in(names, session, &|d| items.iter().find(|i| i.def == d), it)
}

/// `misplaced_block`, with `item` finding an item of `module` by `DefId`.
fn misplaced_block_in<'a>(
    names: &Names<'_>,
    session: &[PathId],
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
    if !same_module(names, session, it.def, def) {
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
/// an unresolved name is skipped: its name already has an error. A line
/// that breaks a rule derives nothing: its error replaces every later
/// error for its implementations (`trait.derive.related.replaces`), in the
/// interface and in every later lowering of the module, which sees the
/// same lines. `report` is false for those later lowerings, whose errors
/// the interface already holds. Returns whether any implementation was
/// dropped.
fn related_derives(
    names: &Names<'_>,
    items: &mut Vec<Item>,
    index: &ItemIndex,
    heads: &[Head<'_>],
    scope: &ModuleScope,
    src: &Src<'_>,
    diags: &mut DiagBuf,
    report: bool,
) -> bool {
    let known = names.known;
    let mut broken: Vec<(DefId, Vec<DefId>)> = Vec::new();
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
                if report {
                    let mut missing: Vec<_> = traits
                        .iter()
                        .flat_map(|&t| needed_traits(known, t))
                        .filter(|t| !traits.contains(t))
                        .map(|t| names.display_name(t))
                        .collect();
                    missing.sort_unstable();
                    missing.dedup();
                    let detail = if missing.is_empty() {
                        String::new()
                    } else {
                        format!("; missing `{}`", missing.join("`, `"))
                    };
                    diags.error(
                        Code::MixedDerivedLaw,
                        src.span(d),
                        &format!("`@derive` must list the traits a derived trait needs, and cannot mix with hand-written related traits{detail}"),
                    );
                }
                broken.push((h.def, traits));
            }
        }
    }
    if broken.is_empty() {
        return false;
    }
    items.retain(|it| {
        let ItemData::Impl {
            trait_,
            self_ty,
            kind: ImplKind::Derived,
            ..
        } = &it.data
        else {
            return true;
        };
        let TyData::Adt { def, .. } = names.pool.get(*self_ty) else {
            return true;
        };
        !broken
            .iter()
            .any(|(ty, traits)| *ty == def && traits.contains(trait_))
    });
    true
}

/// Whether `Structure` in `scope` names `std.structure.Structure`
/// (`annot.block.structure-use`).
#[must_use]
pub fn names_std_structure(names: &Names<'_>, scope: &ModuleScope) -> bool {
    matches!(
        scope.lookup(names.syms.intern("Structure")),
        Some(Binding {
            kind: BindingKind::Item,
            value,
        }) if DefId::from_raw(value) == names.known.structure
    )
}

/// The derivation template of `trait_` (`annot.template.form`), among a
/// module's own items, then the interfaces it sees.
/// A template declared outside its trait's module is rejected
/// (`misplaced-derivation`) and derives nothing.
#[must_use]
pub fn template_of(
    names: &Names<'_>,
    session: &[PathId],
    lookup: &Lookup<'_>,
    trait_: DefId,
) -> Option<DefId> {
    let is = |it: &&Item| {
        matches!(&it.data, ItemData::Impl { trait_: t, kind: ImplKind::Template, .. } if *t == trait_)
            && !misplaced_template(names, session, it)
    };
    lookup
        .own
        .iter()
        .find(is)
        .or_else(|| lookup.ifaces.iter().find_map(|f| f.items.impls().find(is)))
        .map(|it| it.def)
}

/// The `@derive` line of `ty`'s declaration that names `trait_`, if one
/// resolves: its span, for `underivable-trait` (`annot.derive.other`,
/// `annot.derive.error-trait`).
#[must_use]
pub fn derive_span(
    names: &Names<'_>,
    scope: &ModuleScope,
    src: &Src<'_>,
    heads: &[Head<'_>],
    ty: DefId,
    trait_: DefId,
) -> Option<Span> {
    for h in heads.iter().filter(|h| h.def == ty) {
        for d in h
            .node
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
        {
            let (name, args) = decorator_line(src, d)?;
            if name != "derive" {
                continue;
            }
            for a in args {
                let resolved = match scope.lookup(names.syms.intern(&a)) {
                    Some(Binding {
                        kind: BindingKind::Item,
                        value,
                    }) => DefId::from_raw(value),
                    _ => continue,
                };
                if resolved == trait_ {
                    return Some(src.span(d));
                }
            }
        }
    }
    None
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
/// `annot.template.unique`, `annot.block.module`, `annot.block.newtype.error`,
/// `annot.line.placement-blocks`, `annot.no-trait.*`), each reported once on
/// the block or member. `extra` holds the targets of blocks that sit in
/// another module of the session, and `blocked` the types that another
/// module of the session gives a derivation block with no trait.
fn placement(
    names: &Names<'_>,
    session: &[PathId],
    (items, extra, mut blocked): (&[Item], &[Item], HashSet<DefId>),
    index: &ItemIndex,
    heads: &[Head<'_>],
    src: &Src<'_>,
    diags: &mut DiagBuf,
) {
    // The traits that already have their one template of each kind.
    let mut templated: HashSet<(DefId, bool)> = HashSet::default();
    // The types that already have their one derivation block with no trait.
    for h in heads.iter().filter(|h| h.kind == HeadKind::Impl) {
        let Some(it) = index.get(items, h.def) else {
            continue;
        };
        let ItemData::Impl {
            trait_,
            by,
            self_ty,
            kind,
            ..
        } = &it.data
        else {
            continue;
        };
        let msg = if misplaced_template(names, session, it) {
            Some("a template belongs in the module that declares its trait")
        } else {
            misplaced_block_in(
                names,
                session,
                &|d| {
                    index
                        .get(items, d)
                        .or_else(|| extra.iter().find(|i| i.def == d))
                },
                it,
            )
        };
        if let Some(msg) = msg {
            diags.error(Code::MisplacedDerivation, src.span(h.node), msg);
        }
        // A trait has at most one template and one tuple template, so a
        // second is an error (`annot.template.unique`,
        // `annot.template.tuple.rules`); a misplaced one counts for nothing.
        if matches!(kind, ImplKind::Template | ImplKind::TupleTemplate)
            && msg.is_none()
            && !templated.insert((*trait_, *kind == ImplKind::TupleTemplate))
        {
            let msg = if *kind == ImplKind::TupleTemplate {
                "a trait has at most one tuple template"
            } else {
                "a trait has at most one template"
            };
            diags.error(Code::OverlappingImpl, src.span(h.node), msg);
        }
        let traitless = *trait_ == DefId::NONE && by.is_some_and(|b| names.text(b) == "Structure");
        // A type has at most one derivation block with no trait, so a
        // second is an error (`annot.no-trait.unique`). A misplaced block
        // is already one error and counts for nothing.
        if traitless
            && msg.is_none()
            && let TyData::Adt { def, .. } = names.pool.get(*self_ty)
            && !blocked.insert(def)
        {
            diags.error(
                Code::OverlappingImpl,
                src.span(h.node),
                "a type has at most one derivation block with no trait",
            );
        }
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

/// The modules of the REPL input `module`'s session, the input included, as
/// paths; empty for any other module.
fn session_paths(
    names: &Names<'_>,
    module: &str,
    sessions: &HashMap<String, Vec<String>>,
) -> Vec<PathId> {
    let Some(mods) = sessions.get(module) else {
        return Vec::new();
    };
    std::iter::once(module)
        .chain(mods.iter().map(String::as_str))
        .map(|m| names.module(m))
        .collect()
}

/// The declarations, in other modules of the session, that the derivation
/// blocks of `items` target: a block's target is checked where it is
/// declared (`annot.block.newtype.error`, `annot.no-trait.target`).
fn session_targets(
    r: &Resolver<'_, '_>,
    items: &[Item],
    index: &ItemIndex,
    session: &[PathId],
) -> Vec<Item> {
    let names = &r.cx.names;
    let mut out: Vec<Item> = Vec::new();
    if session.is_empty() {
        return out;
    }
    for it in items {
        let ItemData::Impl { self_ty, .. } = &it.data else {
            continue;
        };
        let TyData::Adt { def, .. } = names.pool.get(*self_ty) else {
            continue;
        };
        if index.get(items, def).is_some()
            || out.iter().any(|i| i.def == def)
            || !derivation_block(names, it)
        {
            continue;
        }
        let Some(iface) =
            r.cx.world
                .module_folder_node(names, names.module_node(def))
                .and_then(|f| r.cx.world.iface(f))
        else {
            continue;
        };
        if let Some(target) = iface.item(def) {
            out.push(target.clone());
        }
    }
    out
}

/// The types to which an earlier module of the REPL input `module`'s
/// session gives its one derivation block with no trait, so a block in the
/// input is a second one (`annot.no-trait.unique`,
/// `cli.repl.redefine.impl`). Empty for any other module.
fn session_blocks(r: &Resolver<'_, '_>, module: &str) -> HashSet<DefId> {
    let names = &r.cx.names;
    let mut out = HashSet::default();
    for m in r.session_modules.get(module).into_iter().flatten() {
        let Some(iface) =
            r.cx.world
                .module_folder(m)
                .and_then(|f| r.cx.world.iface(f))
        else {
            continue;
        };
        let node = names.module(m);
        for it in iface.items.impls() {
            if let ItemData::Impl {
                trait_,
                by: Some(by),
                self_ty,
                ..
            } = &it.data
                && *trait_ == DefId::NONE
                && names.text(*by) == "Structure"
                && names.module_node(it.def) == node
                && let TyData::Adt { def, .. } = names.pool.get(*self_ty)
            {
                out.insert(def);
            }
        }
    }
    out
}

/// The diagnostics a use of the folder's modules reports when it names
/// nothing (`USE_CODES`), in the order [`build_folder`] meets them:
/// `HeaderCheck(F)` reports them (#248), so the interface entry, keyed
/// by the folders F's headers read, holds none. They depend on every
/// folder F's uses reach, and on F's own uses.
pub fn use_diags(cx: &Cx<'_>, mods: &[ModIn<'_>], diags: &mut DiagBuf) {
    let names = &cx.names;
    let all_heads: Vec<Vec<Head<'_>>> = mods
        .iter()
        .map(|m| heads(names, &m.src, &m.path, m.tests))
        .collect();
    let all_uses: Vec<Vec<UseDecl>> = mods.iter().map(|m| use_decls(&m.src, &m.roots)).collect();
    let r = resolver(cx, mods, &all_heads, &all_uses, None);
    for ((m, hs), us) in mods.iter().zip(&all_heads).zip(&all_uses) {
        let mut scoping = DiagBuf::default();
        let _ = scope_of(&r, m, hs, us, &mut scoping);
        diags.append_if(&scoping, &is_use_code);
    }
}

/// The codes a `use` that names nothing reports while module scopes are
/// built: [`use_diags`]' codes, which `build_folder` leaves out.
/// `re-export-loop` is not one: it depends only on the folder's own
/// source, and its interface reports it once per loop.
pub const USE_CODES: [Code; 6] = [
    Code::UnknownImport,
    Code::PrivateImport,
    Code::UnknownModule,
    Code::AmbiguousImport,
    Code::TestOnlyUse,
    Code::CyclicTestDependency,
];

fn is_use_code(code: Code) -> bool {
    USE_CODES.contains(&code)
}

/// The resolver of one folder's lowering: its own heads and `pub use`
/// links, over `frozen` once its interface is built.
fn resolver<'a, 'b>(
    cx: &'b Cx<'a>,
    mods: &[ModIn<'_>],
    all_heads: &[Vec<Head<'_>>],
    all_uses: &[Vec<UseDecl>],
    frozen: Option<Arc<FolderIface>>,
) -> Resolver<'a, 'b> {
    let names = &cx.names;
    let mut r = Resolver {
        cx,
        frozen,
        module_keys: HashMap::default(),
        module_paths: HashMap::default(),
        own: HashMap::default(),
        struct_defs: HashSet::default(),
        pub_uses: HashMap::default(),
        memo: RefCell::new(HashMap::default()),
        visiting: RefCell::new(HashSet::default()),
        loops_reported: RefCell::new(HashSet::default()),
        trait_assoc: HashMap::default(),
        variances: HashMap::default(),
        row_params: HashMap::default(),
        aliases: RefCell::new(HashMap::default()),
        seeds: HashMap::default(),
        own_traits: HashMap::default(),
        own_types: RefCell::new(HashMap::default()),
        own_supers: HashMap::default(),
        inputs: mods
            .iter()
            .filter(|m| m.session.is_some())
            .map(|m| m.path.clone())
            .collect(),
        session_modules: mods
            .iter()
            .filter_map(|m| Some((m.path.clone(), m.session.as_ref()?.modules.clone())))
            .collect(),
    };
    for (m, hs) in mods.iter().zip(all_heads) {
        let module = r.intern_module(&m.path);
        for h in hs {
            if h.kind == HeadKind::Impl {
                continue;
            }
            // A local declaration is no module member (`names.local-type.static`),
            // and no other module sees a test item (`names.tests.inside-only`).
            if h.local.is_none() && h.test.is_none() {
                r.own.entry((module, h.name)).or_insert((
                    h.def,
                    h.kind,
                    h.public || m.session.is_some(),
                ));
            }
            if h.kind == HeadKind::Data
                && h.node
                    .direct_token(&m.src.parse.tokens, TokenKind::KwStruct)
                    .is_some()
            {
                r.struct_defs.insert(h.def);
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
                let rows = gl
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::GenericParameter)
                    .map(|g| g.direct_token(toks, TokenKind::Dollar).is_some())
                    .collect();
                r.row_params.insert(h.def, rows);
            }
            if h.kind == HeadKind::Trait {
                let assoc = Src::child(h.node, SyntaxKind::Block)
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::AssociatedTypeDecl)
                    .filter_map(|c| c.name(&m.src.parse.tokens))
                    .map(|t| m.src.symbol(t, names.syms))
                    .collect();
                r.trait_assoc.insert(h.def, assoc);
            }
        }
        for s in &m.seeds {
            if let Some(k) = s.kind() {
                r.own
                    .entry((module, s.name))
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
    for (m, us) in mods.iter().zip(all_uses) {
        let from = r.module_keys[m.path.as_str()];
        for u in us {
            for (local, module, name) in reexports(&r, m, u) {
                let target = r.intern_module(&module);
                r.pub_uses.insert((from, local), (target, name));
            }
        }
    }
    r
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
    let mut r = resolver(cx, mods, &all_heads, &all_uses, frozen);
    let mut out = FolderOut {
        modules: Vec::new(),
        exports: Vec::new(),
        private_names: Vec::new(),
        loop_names: Vec::new(),
        module_uses: Vec::new(),
    };
    let mut scopes: Vec<ModScopes> = mods
        .iter()
        .zip(&all_heads)
        .zip(&all_uses)
        .map(|((m, hs), us)| {
            // An interface leaves the use diagnostics to `HeaderCheck`
            // ([`use_diags`]).
            let mut scoping = DiagBuf::default();
            let (scope, mut kinds) = scope_of(&r, m, hs, us, &mut scoping);
            diags.append_if(&scoping, &|c| r.frozen.is_some() || !is_use_code(c));
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
    // A use bound above may have filled the memo of this folder's own
    // `pub use` chains while its reads were held: they are read again,
    // recorded, when a lookup needs them.
    r.memo.borrow_mut().clear();
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
                    kind_reported: HashSet::default(),
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
        let mut index = ItemIndex::new(names, &items);
        if r.frozen.is_none() {
            let session = session_paths(names, &m.path, &r.session_modules);
            let extra = session_targets(&r, &items, &index, &session);
            let blocked = session_blocks(&r, &m.path);
            placement(
                names,
                &session,
                (&items, &extra, blocked),
                &index,
                hs,
                &m.src,
                diags,
            );
        }
        let dropped = related_derives(
            names,
            &mut items,
            &index,
            hs,
            &scope,
            &m.src,
            diags,
            r.frozen.is_none(),
        );
        if dropped {
            // Broken `@derive` lines drop their derived implementations
            // above, moving every later item: rebuild their index.
            index = ItemIndex::new(names, &items);
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
    drop_extra_methods(&r, &mut out.modules);
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
                kind_reported: HashSet::default(),
                locals: &[],
                at: 0,
                outer: Vec::new(),
            };
            low.check_module_decorators(&items, &o.blocks);
        }
        decls::check(&r, mods, &all_heads, &out.modules, diags);
        let mut seen = HashSet::default();
        for (m, us) in mods.iter().zip(&all_uses) {
            let module = r.module_keys[m.path.as_str()];
            let mut publics: Vec<Symbol> = r
                .own
                .iter()
                .filter(|((mp, _), (_, _, p))| *mp == module && *p)
                .map(|((_, n), _)| *n)
                .collect();
            for u in us {
                publics.extend(reexports(&r, m, u).into_iter().map(|(local, _, _)| local));
            }
            for name in publics {
                if !seen.insert((module, name)) {
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
                        out.loop_names.push((m.path.clone(), name));
                        let span = us
                            .iter()
                            .find(|u| u.public)
                            .map_or(m.src.span(m.src.root()), |u| u.span);
                        r.report_loop(diags, span, &m.path, name);
                    }
                    Err(_) => {}
                }
            }
        }
        out.exports.sort_by(|a, b| {
            (a.module.as_str(), names.text(a.name)).cmp(&(b.module.as_str(), names.text(b.name)))
        });
        for (m, o) in mods.iter().zip(&out.modules) {
            if m.session.is_none() {
                continue;
            }
            let sc = &o.scope;
            for ((&name, b), origin) in sc.name.iter().zip(&sc.binding).zip(&sc.origin) {
                if b.kind != BindingKind::Module || *origin != Origin::Use {
                    continue;
                }
                if let Some(target) = sc.modules.get(b.value as usize) {
                    // A name an earlier input left in the session is in
                    // this scope too; only the input's own uses are its
                    // record, so a redeclaration names the first input.
                    let inherited = m.session.as_ref().is_some_and(|outer| {
                        outer.scope.lookup(name).is_some_and(|ob| {
                            ob.kind == BindingKind::Module
                                && outer.scope.modules.get(ob.value as usize) == Some(target)
                        })
                    });
                    if inherited {
                        continue;
                    }
                    let local = names.text(name).to_owned();
                    out.module_uses
                        .push((m.path.clone(), local, target.clone()));
                }
            }
        }
        out.module_uses.sort();
        out.private_names = r
            .own
            .iter()
            .filter(|(_, (_, _, public))| !public)
            .map(|(&(module, name), _)| (r.module_text(module).to_owned(), name))
            .collect();
        out.private_names
            .sort_by(|a, b| (a.0.as_str(), names.text(a.1)).cmp(&(b.0.as_str(), names.text(b.1))));
        out.loop_names
            .sort_by(|a, b| (a.0.as_str(), names.text(a.1)).cmp(&(b.0.as_str(), names.text(b.1))));
        // A use read quietly while it bound (`bind_uses`); what the
        // lowering made of a name it binds depends on those reads, so
        // they count for each name a lookup consulted.
        for ((m, us), o) in mods.iter().zip(&all_uses).zip(&out.modules) {
            let seen = o.scope.looked_up();
            reread_uses(&r, us, &seen);
            if m.tests {
                reread_uses(&r, &test_use_decls(&m.src, &m.roots), &seen);
            }
        }
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
    let mut out = HashMap::default();
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
                kind_reported: HashSet::default(),
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

/// Leaves out of a written or delegating implementation each method its
/// trait does not declare (spec 09 `trait.impl.extra-methods`): such a
/// method is no part of the trait implementation, so a later call to it is
/// the ordinary unknown method. The checker reports it once, where its body
/// is checked.
fn drop_extra_methods(r: &Resolver<'_, '_>, modules: &mut [ModOut]) {
    let mut drops: Vec<(usize, usize, Vec<DefId>)> = Vec::new();
    for (mi, m) in modules.iter().enumerate() {
        for (ii, it) in m.items.iter().enumerate() {
            let ItemData::Impl {
                trait_,
                methods,
                kind: ImplKind::Written | ImplKind::Delegated,
                ..
            } = &it.data
            else {
                continue;
            };
            if *trait_ == DefId::NONE {
                continue;
            }
            let found = modules
                .iter()
                .flat_map(|m| m.items.iter())
                .find(|i| i.def == *trait_)
                .map(|i| i.data.clone())
                .or_else(|| r.item(*trait_).map(|i| i.data));
            let Some(ItemData::Trait(td)) = found else {
                continue;
            };
            let extra: Vec<DefId> = methods
                .iter()
                .filter(|(n, _)| !td.methods.iter().any(|(t, _)| t == n))
                .map(|(_, d)| *d)
                .collect();
            if !extra.is_empty() {
                drops.push((mi, ii, extra));
            }
        }
    }
    for (mi, ii, extra) in drops {
        if let ItemData::Impl { methods, .. } = &mut modules[mi].items[ii].data {
            methods.retain(|(_, d)| !extra.contains(d));
        }
    }
}

/// The members a delegating implementation `impl Tr for C by E` takes
/// from its part (spec 09 "Delegation", `crates/hd_check/DESIGN.md` (trait solver) §3.10), once every
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
/// A `@derive` on a newtype takes every method of its trait, receiver or
/// not, from the base type the same way, over the newtype's base in place
/// of a field (`trait.derive.newtype.base`; `hd_check::newtype` writes the
/// bodies, which wrap and unwrap the `Self` positions).
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
                by,
                kind: kind @ (ImplKind::Delegated | ImplKind::Derived),
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
            let ca = pool.list_items(cargs);
            let at_args = |t: Ty| {
                pool.subst(t, &|q: ParamRef| {
                    (q.owner == c)
                        .then(|| ca.get(q.index as usize).copied())
                        .flatten()
                })
            };
            // A delegating implementation takes the methods with a receiver
            // from an embedded field. A derived newtype takes every method
            // from its base type (trait.derive.newtype.base).
            let (part, receivers_only) = match (kind, by, item(c).map(|i| i.data)) {
                (ImplKind::Delegated, Some(by), Some(ItemData::Data(fields))) => {
                    let Some(field) = fields.iter().find(|f| f.embedded && f.name == *by) else {
                        continue;
                    };
                    (at_args(field.ty), true)
                }
                (ImplKind::Derived, _, Some(ItemData::Newtype(inner))) => (at_args(inner), false),
                _ => continue,
            };
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
                if receivers_only && sig.params.first().is_none_or(|p| names.text(p.0) != "self") {
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
    let mut out = HashMap::default();
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
                kind_reported: HashSet::default(),
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

#[cfg(test)]
mod prelude_tests {
    use super::{PRELUDE, Src, TEST_PRELUDE, use_decls};

    fn assert_exports(text: &str, table: &[(&str, &[&str])]) {
        let parsed = hd_syntax::parse(text.as_bytes());
        assert!(parsed.is_ok(), "{:?}", parsed.diagnostics);
        let src = Src {
            parse: &parsed,
            text,
            file: hd_base::FileId::from_raw(0),
        };
        let mut exports = Vec::new();
        for declaration in use_decls(&src, &hd_project::UseRoots::default()) {
            if !declaration.public {
                continue;
            }
            assert!(declaration.root_error.is_none());
            assert!(declaration.alias.is_none());
            let mut path = declaration.path;
            if let Some(group) = declaration.group {
                let module = path.join(".");
                for name in group {
                    assert!(name.alias.is_none());
                    exports.push((module.clone(), name.name));
                }
            } else {
                let name = path.pop().expect("an exported name");
                exports.push((path.join("."), name));
            }
        }
        let mut expected: Vec<_> = table
            .iter()
            .flat_map(|(module, names)| {
                names
                    .iter()
                    .map(|name| ((*module).to_owned(), (*name).to_owned()))
            })
            .collect();
        exports.sort();
        expected.sort();
        assert_eq!(exports, expected);
    }

    #[test]
    fn ordinary_prelude_matches_its_hd_exports() {
        assert_exports(include_str!("../../../lib/std/prelude.hd"), PRELUDE);
    }

    #[test]
    fn test_prelude_matches_its_hd_exports() {
        assert_exports(
            include_str!("../../../lib/std/prelude/testing.hd"),
            TEST_PRELUDE,
        );
    }
}

#[cfg(test)]
mod module_key_tests {
    use super::{Cx, Recorded, Resolver, World, resolver};
    use crate::{FolderIface, HeadKind, KnownItems, Names};
    use hd_base::{DefId, PathId};
    use hd_diag::Code;
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::InternPool;
    use std::sync::Arc;

    struct Modules;

    impl World for Modules {
        fn module_folder(&self, module: &str) -> Option<u32> {
            match module {
                "app" | "app.a" | "app.z" => Some(0),
                "dep.mod" => Some(1),
                _ => None,
            }
        }

        fn usable(&self, module: &str) -> bool {
            self.module_folder(module).is_some()
        }

        fn test_only(&self, _module: &str) -> bool {
            false
        }

        fn iface(&self, _folder: u32) -> Option<Arc<FolderIface>> {
            None
        }
    }

    fn with_resolver(test: impl FnOnce(&mut Resolver<'_, '_>)) {
        let pool = InternPool::new();
        let paths = PathTable::new();
        let syms = ShardedInterner::default();
        let known = KnownItems::new(&paths);
        let names = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
        };
        let cx = Cx {
            names,
            folder: 0,
            world: &Modules,
        };
        test(&mut resolver(&cx, &[], &[], &[], None));
    }

    #[test]
    fn declaring_module_and_package_keep_distinct_identity() {
        with_resolver(|r| {
            let names = &r.cx.names;
            let a = names.item("first.deep.mod", "Same");
            let b = names.item("second.deep.mod", "Same");
            let member = names.member(
                names.local_scope(a, 1),
                hd_intern::PathKind::Member,
                "field",
            );
            assert_eq!(names.module_node(member), names.module_node(a));
            assert_ne!(names.module_node(a), names.module_node(b));
            assert_eq!(names.package_node(member), names.module("first"));
            assert_ne!(names.package_node(a), names.package_node(b));
            assert_eq!(
                names.module_text(names.module_node(member)),
                "first.deep.mod"
            );
            assert_eq!(names.module_of(member), "first.deep.mod");
            assert_eq!(names.module_text(PathId::NONE), "");
        });
    }

    #[test]
    fn node_lookup_keeps_text_worlds_and_recorded_reads() {
        with_resolver(|r| {
            struct Indexed {
                app: PathId,
            }
            impl World for Indexed {
                fn module_folder(&self, _: &str) -> Option<u32> {
                    panic!("node lookup must not fall back to text");
                }
                fn module_path_node(&self, node: PathId) -> Option<&str> {
                    (node == self.app).then_some("app.a")
                }
                fn module_folder_node(&self, _: &Names<'_>, node: PathId) -> Option<u32> {
                    (node == self.app).then_some(0)
                }
                fn usable(&self, _: &str) -> bool {
                    true
                }
                fn test_only(&self, _: &str) -> bool {
                    false
                }
                fn iface(&self, _: u32) -> Option<Arc<FolderIface>> {
                    None
                }
            }
            let names = &r.cx.names;
            let app = names.module("app.a");
            let missing = names.module("missing");
            assert_eq!(Modules.module_folder_node(names, app), Some(0));
            assert_eq!(Modules.module_folder_node(names, missing), None);
            let world = Indexed { app };
            let recorded = Recorded::new(&world);
            assert_eq!(recorded.module_folder_node(names, app), Some(0));
            assert_eq!(recorded.module_folder_node(names, app), Some(0));
            assert_eq!(recorded.reads().modules, vec!["app.a"]);
            recorded.hold_reads(true);
            assert_eq!(recorded.module_folder_node(names, missing), None);
            assert_eq!(recorded.reads().modules, vec!["app.a"]);
            recorded.hold_reads(false);
            assert_eq!(recorded.module_folder_node(names, missing), None);
            assert_eq!(recorded.reads().modules, vec!["app.a", "missing"]);
        });
    }

    #[test]
    fn keys_preserve_module_identity_and_export_failures() {
        with_resolver(|r| {
            let z = r.intern_module("app.z");
            let a = r.intern_module("app.a");
            assert_eq!(r.intern_module("app.z"), z);
            assert_ne!(a, z);
            assert_eq!(r.module_text(a), "app.a");
            assert_eq!(r.module_keys.len(), 2);
            let name = r.cx.names.syms.intern("visible");
            let def = r.cx.names.item("app.a", "visible");
            r.own.insert((a, name), (def, HeadKind::Fn, true));
            r.pub_uses.insert((z, name), (a, name));
            assert_eq!(r.export("app.z", name), Ok((def, HeadKind::Fn)));
            let hidden = r.cx.names.syms.intern("hidden");
            r.own
                .insert((a, hidden), (DefId::NONE, HeadKind::Fn, false));
            assert_eq!(r.export("app.a", hidden), Err(Code::PrivateImport));
            assert_eq!(r.export("app.z", hidden), Err(Code::UnknownImport));
            assert_eq!(r.export("app", hidden), Err(Code::UnknownImport));
            assert_eq!(r.export("missing", hidden), Err(Code::UnknownModule));
            assert_eq!(r.export("dep.mod", hidden), Err(Code::UnknownModule));
        });
    }

    #[test]
    fn cycle_keys_are_canonical_by_text_not_intern_order() {
        with_resolver(|r| {
            let z = r.intern_module("app.z");
            let a = r.intern_module("app.a");
            let name = r.cx.names.syms.intern("value");
            let ka = (a, name);
            let kz = (z, name);
            r.pub_uses.insert(ka, kz);
            r.pub_uses.insert(kz, ka);
            assert_eq!(r.export_cycle(kz), Some(vec![ka, kz]));
            assert_eq!(r.export_cycle(ka), Some(vec![ka, kz]));
            assert_eq!(r.export("app.z", name), Err(Code::ReExportLoop));
            assert!(r.visiting.borrow().is_empty());
            assert_ne!(a, PathId::NONE);
        });
    }
}
