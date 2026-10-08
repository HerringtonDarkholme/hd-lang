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

/// The compiler-implemented sealed traits: a written implementation of one
/// is `sealed-trait-implementation`, and its methods are the compiler's.
pub const SEALED_TRAIT_PATHS: [&str; 4] = [
    "std/core/AnyVal",
    "std/core/AnyRef",
    "std/structure/Structure",
    "std/function/Tuple",
];
use hd_intern::PathKind;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::{ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList};

use crate::iface::{
    Export, Field, FnSig, FolderIface, Generic, HeadKind, ImplKind, Item, ItemData, Names,
    TraitData, Variant,
};
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
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>>;
}

/// One name of a use: the declaration, its local name, where it is.
#[derive(Clone, Debug)]
pub struct UseName {
    pub name: String,
    pub alias: Option<String>,
    pub span: Span,
}

/// A use declaration with `pkg`, `self` and `super` rewritten to absolute
/// segments.
#[derive(Clone, Debug)]
pub struct UseDecl {
    pub public: bool,
    pub path: Vec<String>,
    /// `use M.{a, b as c}`; `None` for a single use.
    pub group: Option<Vec<UseName>>,
    pub alias: Option<String>,
    pub span: Span,
    /// A relative path that moved above its root.
    pub bad_root: bool,
}

impl UseDecl {
    #[must_use]
    pub fn module(&self) -> String {
        self.path.join(".")
    }
}

/// The use declarations of a file, in order.
#[must_use]
pub fn use_decls(src: &Src<'_>, package: &str, module: &str) -> Vec<UseDecl> {
    let mut out = Vec::new();
    // A `tests:` block's uses join the module's scope (a phase-1
    // simplification: the spec scopes them to the block).
    let in_tests = src
        .root()
        .children()
        .filter(|c| c.kind() == SyntaxKind::TestsBlock)
        .flat_map(hd_syntax::NodeRef::children)
        .filter(|b| b.kind() == SyntaxKind::Block)
        .flat_map(hd_syntax::NodeRef::children);
    for item in src
        .root()
        .children()
        .chain(in_tests)
        .filter(|c| c.kind() == SyntaxKind::UseDecl)
    {
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
        let mut bad_root = false;
        match path.first().map(String::as_str) {
            Some("pkg") => package.clone_into(&mut path[0]),
            Some("self") => {
                let mut abs: Vec<String> = module.split('.').map(str::to_owned).collect();
                abs.extend(path.drain(1..));
                path = abs;
            }
            Some("super") => {
                let mut abs: Vec<String> = module.split('.').map(str::to_owned).collect();
                let mut rest = path.drain(..).peekable();
                while rest.peek().map(String::as_str) == Some("super") {
                    rest.next();
                    abs.pop();
                    if abs.is_empty() {
                        bad_root = true;
                    }
                }
                abs.extend(rest);
                path = abs;
            }
            _ => {}
        }
        out.push(UseDecl {
            public,
            path,
            group,
            alias,
            span: src.span(item),
            bad_root,
        });
    }
    out
}

/// One own item's head and its node.
#[derive(Clone, Copy)]
pub struct Head<'t> {
    pub name: Symbol,
    pub def: DefId,
    pub kind: HeadKind,
    pub node: NodeRef<'t>,
    pub public: bool,
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

/// Heads of the module's own top-level items.
#[must_use]
pub fn heads<'t>(names: &Names<'_>, src: &Src<'t>, module: &str) -> Vec<Head<'t>> {
    let mut out = Vec::new();
    let mut impl_names: HashMap<String, u32> = HashMap::new();
    for n in src.root().children() {
        let (kind, kw) = match n.kind() {
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
            SyntaxKind::ImplDecl => {
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
                out.push(Head {
                    name: names.syms.intern(&seg),
                    def,
                    kind: HeadKind::Impl,
                    node: n,
                    public: true,
                });
                continue;
            }
            _ => continue,
        };
        let Some(t) = n.name(&src.parse.tokens).or_else(|| src.name_after(n, kw)) else {
            continue;
        };
        let text = src.text(t);
        out.push(Head {
            name: names.syms.intern(text),
            def: names.item(module, text),
            kind,
            node: n,
            public: src.is_pub(n),
        });
    }
    out
}

/// The kind of every item a scope can name.
pub type Kinds = HashMap<DefId, HeadKind>;

/// One module of the folder being resolved.
pub struct ModIn<'t> {
    pub path: String,
    pub src: Src<'t>,
    /// Compiler-supplied items of this module (`seed`).
    pub seeds: Vec<Item>,
}

/// One module's result: all its items (private ones included), its scope
/// and the kinds of the names the scope binds.
pub struct ModOut {
    pub items: Vec<Item>,
    pub scope: ModuleScope,
    pub kinds: Kinds,
    /// Declaration-relative positions of the items (`anchor`).
    pub anchors: crate::anchor::Anchors,
}

pub struct FolderOut {
    pub modules: Vec<ModOut>,
    pub exports: Vec<Export>,
}

/// What one resolution needs from the run.
pub struct Cx<'a> {
    pub names: Names<'a>,
    pub package: &'a str,
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
    /// This folder's aliases, lowered in module order.
    aliases: RefCell<HashMap<DefId, (usize, Ty)>>,
    seeds: HashMap<DefId, Item>,
}

impl Resolver<'_, '_> {
    fn module_exists(&self, module: &str) -> bool {
        self.cx.world.module_folder(module).is_some()
    }

    fn export(&self, module: &str, name: Symbol) -> Res {
        let Some(f) = self.cx.world.module_folder(module) else {
            return Err(Code::UnknownModule);
        };
        if f != self.cx.folder {
            let i = self.cx.world.iface(f).ok_or(Code::UnknownModule)?;
            return i
                .export(module, name)
                .map(|e| (e.def, e.kind))
                .ok_or(Code::UnknownImport);
        }
        if let Some(i) = &self.frozen {
            return i
                .export(module, name)
                .map(|e| (e.def, e.kind))
                .ok_or(Code::UnknownImport);
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

    /// An impl head's trait arguments with the trait's omitted trailing
    /// arguments filled from their defaults (`types.type-args.default`):
    /// `impl Add for Money` implements `Add[Money]`
    /// (`expr.op.trait.rhs-self`). Parameter 0 is `Self`.
    fn default_trait_args(&self, tr: DefId, args: TyList, self_ty: Ty) -> TyList {
        let pool = self.cx.names.pool;
        let Some(item) = self.item(tr) else {
            return args;
        };
        let mut v = pool.list_items(args);
        if v.len() >= item.generics.len() {
            return args;
        }
        for g in &item.generics[v.len()..] {
            let Some(d) = g.default else {
                return args;
            };
            let known = v.clone();
            let t = pool.subst(d, &|p: ParamRef| {
                if p.owner != tr {
                    return None;
                }
                if p.index == 0 {
                    Some(self_ty)
                } else {
                    known.get(p.index as usize - 1).copied()
                }
            });
            v.push(t);
        }
        pool.list(&v)
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

fn scope_of(
    r: &Resolver<'_, '_>,
    m: &ModIn<'_>,
    heads: &[Head<'_>],
    uses: &[UseDecl],
    diags: &mut DiagBuf,
) -> (ModuleScope, Kinds) {
    let names = &r.cx.names;
    let mut scope = ModuleScope::default();
    let mut kinds = Kinds::new();
    for h in heads {
        if h.kind != HeadKind::Impl {
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
    for (row, u) in uses.iter().enumerate() {
        let row = u32::try_from(row).unwrap_or(u32::MAX);
        let module = u.module();
        if u.bad_root {
            diags.error(
                Code::UnknownModule,
                u.span,
                "unknown-module: above the root",
            );
            continue;
        }
        if let Some(group) = &u.group {
            {
                if !r.module_exists(&module) {
                    let msg = format!("unknown-module `{module}`");
                    diags.error(Code::UnknownModule, u.span, &msg);
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
                            bind_item(&mut scope, local, d, origin, row);
                            kinds.insert(d, k);
                        }
                        Err(code) => {
                            let msg = format!("{} `{}` in `{module}`", code.as_str(), g.name);
                            diags.error(code, g.span, &msg);
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
                        let msg = format!("ambiguous-import `{module}`");
                        diags.error(Code::AmbiguousImport, u.span, &msg);
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
                    bind_item(&mut scope, local, d, Origin::Use, row);
                    kinds.insert(d, k);
                } else if !r.module_exists(&parent) {
                    let msg = format!("unknown-module `{parent}`");
                    diags.error(Code::UnknownModule, u.span, &msg);
                } else {
                    let code = r
                        .export(&parent, names.syms.intern(last))
                        .err()
                        .unwrap_or(Code::UnknownImport);
                    let msg = format!("{} `{last}` in `{parent}`", code.as_str());
                    diags.error(code, u.span, &msg);
                }
            }
        }
    }
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
                    bind_item(&mut scope, sym, d, Origin::Prelude, u32::MAX);
                    kinds.insert(d, k);
                }
                Some(b) if b.kind == BindingKind::Item && b.value == d.raw() => {}
                Some(_) => {
                    let span = heads
                        .iter()
                        .find(|h| h.name == sym)
                        .map_or(m.src.span(m.src.root()), |h| m.src.span(h.node));
                    let msg = format!("prelude-name-shadow `{name}`");
                    diags.error(Code::PreludeNameShadow, span, &msg);
                }
            }
        }
    }
    (scope, kinds)
}

/// An associated type declared in a trait or bound in an impl body.
type AssocDecl = (Symbol, DefId, Option<Ty>);

/// The type parameters in scope while lowering one header.
#[derive(Clone, Default)]
struct Gen {
    tys: Vec<(Symbol, Ty, Vec<DefId>)>,
    rows: Vec<(Symbol, RowParamRef)>,
    self_ty: Option<Ty>,
    self_trait: Option<DefId>,
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

    /// The declaration a (possibly qualified) name reaches.
    fn resolve_path(
        &mut self,
        segs: &[(String, hd_base::TokenIdx)],
        span: Span,
    ) -> Option<(DefId, HeadKind)> {
        let (first, _) = segs.first()?;
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
            let msg = format!("unknown-name `{first}`");
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
                    SyntaxKind::RequirementRow => row = Some(self.row(Some(a), gn)),
                    k if k.is_type() => args.push(self.ty(Some(a), gn)),
                    _ => {}
                }
            }
        }
        (args, bindings, row)
    }

    /// A trait reference (a bound, a supertrait, an impl's trait, a `dyn`
    /// or a row key) as a trait-value type.
    fn trait_value(&mut self, n: NodeRef<'_>, gn: &Gen) -> Option<Ty> {
        let segs = self.segments(n);
        let span = self.src.span(n);
        let found = self.resolve_path(&segs, span);
        let Some((def, kind)) = found else {
            let name = segs.last().map_or("", |s| s.0.as_str()).to_owned();
            if segs.len() == 1 {
                let msg = format!("unknown-trait `{name}`");
                self.diags.error(Code::UnknownTrait, span, &msg);
            }
            return None;
        };
        if kind != HeadKind::Trait {
            let msg = format!(
                "unknown-trait `{}`: not a trait",
                segs.last().map_or("", |s| s.0.as_str())
            );
            self.diags.error(Code::UnknownTrait, span, &msg);
            return None;
        }
        let (args, bindings, _) = self.type_args(n, gn);
        let bindings = bindings
            .into_iter()
            .map(|(name, t)| {
                (
                    self.names
                        .member(def, PathKind::Member, self.names.text(name)),
                    t,
                )
            })
            .collect();
        Some(self.names.pool.intern_ty(&TyData::TraitValue {
            def,
            args: self.names.pool.list(&args),
            bindings,
        }))
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
            if let Some(t) = self.trait_value(c, gn) {
                data.keys.push(t);
            }
        }
        self.names.pool.row(&data)
    }

    fn ctor(
        &mut self,
        def: DefId,
        kind: HeadKind,
        args: &[Ty],
        row: Option<RowId>,
        span: Span,
    ) -> Ty {
        let pool = self.names.pool;
        if self.names.module_of(def) == "std.function" {
            let name = self.names.paths.segment(PathId::from_raw(def.raw()));
            if name == "Fn" || name == "SuspendFn" {
                let params = match args.first().map(|a| pool.get(*a)) {
                    Some(TyData::Tuple { elems, .. }) => elems,
                    Some(_) => pool.list(&args[..1]),
                    None => TyList::EMPTY,
                };
                return pool.intern_ty(&TyData::Fn {
                    params,
                    result: args.get(1).copied().unwrap_or(Ty::VOID),
                    row: row.unwrap_or(RowId::EMPTY),
                    suspends: name == "SuspendFn",
                });
            }
        }
        match kind {
            HeadKind::Alias => {
                let body =
                    self.r
                        .aliases
                        .borrow()
                        .get(&def)
                        .copied()
                        .or_else(|| match self.r.item(def) {
                            Some(Item {
                                data: ItemData::Alias(t),
                                generics,
                                ..
                            }) => Some((generics.len(), t)),
                            _ => None,
                        });
                let Some((_, body)) = body else {
                    self.gap("an alias used before its declaration in the same folder");
                    return Ty::POISON;
                };
                pool.subst(body, &|p: ParamRef| {
                    (p.owner == def)
                        .then(|| args.get(p.index as usize).copied())
                        .flatten()
                })
            }
            HeadKind::Data | HeadKind::Enum | HeadKind::Newtype => pool.intern_ty(&TyData::Adt {
                def,
                args: pool.list(args),
            }),
            HeadKind::Trait => {
                let msg = format!("trait-used-as-type `{}`", self.names.path(def));
                self.diags.error(Code::TraitUsedAsType, span, &msg);
                Ty::POISON
            }
            HeadKind::Fn | HeadKind::Impl => {
                let msg = format!("unknown-type `{}`: not a type", self.names.path(def));
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
            if let Some(p) = Prim::ALL.iter().find(|p| p.name() == first) {
                return Ty::prim(*p);
            }
        }
        let Some((def, kind)) = self.resolve_path(&segs, span) else {
            if segs.len() == 1 {
                let msg = format!("unknown-type `{first}`");
                self.diags.error(Code::UnknownType, span, &msg);
            }
            return Ty::POISON;
        };
        let (args, _, row) = self.type_args(n, gn);
        self.ctor(def, kind, &args, row, span)
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
                            "mut-on-primitive: a primitive type has no `mut` form",
                        );
                        t
                    }
                    TyData::Tuple { .. } => {
                        self.diags.error(
                            Code::MutOnTuple,
                            self.src.span(n),
                            "mut-on-tuple: a tuple type has no `mut` form",
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
                        rest = Some(first_ty(self, c));
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
                self.trait_value(c, gn).unwrap_or(Ty::POISON)
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
                for c in n.children() {
                    if c.kind() == SyntaxKind::RequirementRow {
                        row = self.row(Some(c), gn);
                    } else if c.kind().is_type() {
                        let after = arrow.is_some_and(|a| self.src.first(c).raw() > a);
                        let t = self.ty(Some(c), gn);
                        if after {
                            result = t;
                        } else {
                            params.push(t);
                        }
                    }
                }
                pool.intern_ty(&TyData::Fn {
                    params: pool.list(&params),
                    result,
                    row,
                    suspends,
                })
            }
            SyntaxKind::ProjectionType => {
                let base_node = n.children().find(|c| c.kind().is_type());
                let base = self.ty(base_node, gn);
                let Some(t) = n.name(&self.src.parse.tokens) else {
                    return Ty::POISON;
                };
                let name = self.sym(self.src.text(t));
                let mut cands: Vec<DefId> = Vec::new();
                if Some(base) == gn.self_ty
                    && let Some(tr) = gn.self_trait
                {
                    cands.push(tr);
                }
                if let Some((_, _, bounds)) = gn.tys.iter().find(|(_, t, _)| *t == base) {
                    cands.extend(bounds.iter().copied());
                }
                let tr = cands
                    .iter()
                    .copied()
                    .find(|tr| self.r.trait_has_assoc(*tr, name))
                    .or_else(|| cands.first().copied());
                let Some(trait_) = tr else {
                    self.gap("a projection on a type without a bound naming it");
                    return Ty::POISON;
                };
                pool.intern_ty(&TyData::Assoc {
                    assoc: self
                        .names
                        .member(trait_, PathKind::Member, self.names.text(name)),
                    trait_,
                    self_ty: base,
                    args: TyList::EMPTY,
                })
            }
            SyntaxKind::InferType => Ty::POISON,
            SyntaxKind::RequirementRow => {
                self.gap("a requirement row in type position");
                Ty::POISON
            }
            SyntaxKind::ContextType => {
                self.gap("a context type in a header");
                Ty::POISON
            }
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
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            let row = g
                .direct_token(&self.src.parse.tokens, TokenKind::Dollar)
                .is_some();
            if row {
                gn.rows.push((name, RowParamRef { owner, index }));
            } else {
                gn.tys.push((
                    name,
                    self.names
                        .pool
                        .intern_ty(&TyData::Param(ParamRef { owner, index })),
                    Vec::new(),
                ));
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
        for (mut g, node, row) in out {
            let mut bounds = Vec::new();
            if let Some(bl) = Src::child(node, SyntaxKind::BoundList) {
                g.mut_bound = bl
                    .direct_token(&self.src.parse.tokens, TokenKind::KwMut)
                    .is_some();
                for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                    if let Some(t) = self.trait_value(b, gn) {
                        bounds.push(t);
                    }
                }
            }
            g.bound = bounds.first().and_then(|b| self.trait_of(*b));
            if !row && let Some(slot) = gn.tys[start..].iter_mut().find(|(s, _, _)| *s == g.name) {
                slot.2 = bounds
                    .iter()
                    .filter_map(|b| match self.names.pool.get(*b) {
                        TyData::TraitValue { def, .. } => Some(def),
                        _ => None,
                    })
                    .collect();
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
            for (i, p) in pl
                .children()
                .filter(|c| c.kind() == SyntaxKind::Parameter)
                .enumerate()
            {
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
        match self.names.path(def).as_str() {
            "std/core/List" => vec![1],
            "std/core/Map" => vec![0, 1],
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
            "invalid-variance: `{}` is declared {}, but it occurs in a {} position here",
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
        if matches!(
            self.names.path(target).as_str(),
            "std/core/List" | "std/core/Map"
        ) {
            return;
        }
        let declared = self.variances(target);
        if declared.iter().all(|v| *v == 0) {
            return;
        }
        // Each impl parameter's derived variance in the target.
        let mut seen = vec![Seen::default(); generics.len()];
        for (a, v) in pool.list_items(args).into_iter().zip(&declared) {
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
        let mut out = Vec::new();
        for d in n.children().filter(|c| c.kind() == SyntaxKind::Decorator) {
            let Some(e) = d.children().next() else {
                continue;
            };
            let (name_node, args) = match e.kind() {
                SyntaxKind::CallExpr => {
                    (e.children().next(), Src::child(e, SyntaxKind::ArgumentList))
                }
                _ => (Some(e), None),
            };
            let Some(name) = name_node.and_then(|x| self.src.first_ident(x)) else {
                continue;
            };
            let mut list = Vec::new();
            for a in args.iter().flat_map(|l| l.children()) {
                let text: String = self.src.tokens(a).map(|t| self.src.text(t)).collect();
                list.push(text.trim_matches('"').to_owned());
            }
            out.push((self.src.text(name).to_owned(), list));
        }
        out
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
            let msg = format!("duplicate-trait-member: `{text}` is declared twice");
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
                "mutable-impl-target: an implementation target cannot be a `mut` view",
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
            Some(tn) => match self.trait_value(tn, &gn) {
                Some(tv) => match pool.get(tv) {
                    TyData::TraitValue {
                        def,
                        args,
                        bindings,
                    } => (def, self.r.default_trait_args(def, args, self_ty), bindings),
                    _ => return,
                },
                None => return,
            },
        };
        // The compiler-implemented sealed traits take no written
        // implementation (types.sealed.no-impl).
        if self.r.frozen.is_none()
            && by.is_none()
            && SEALED_TRAIT_PATHS.contains(&self.names.path(trait_).as_str())
        {
            let msg = format!(
                "sealed-trait-implementation: `{}` is implemented by the compiler only",
                self.names.path(trait_)
            );
            self.diags
                .error(Code::SealedTraitImplementation, self.src.span(n), &msg);
        }
        let bare_param = matches!(pool.get(self_ty), TyData::Param(p) if p.owner == h.def);
        let tuple_bound = bare_param
            && generics.first().is_some_and(|g| {
                g.bound
                    .is_some_and(|b| self.names.path(b) == "std/function/Tuple")
            });
        let kind = match by.as_deref() {
            Some("Structure") if tuple_bound => ImplKind::TupleTemplate,
            Some("Structure") if bare_param => ImplKind::Template,
            Some("Structure") => ImplKind::Derivation,
            Some(_) => ImplKind::Delegated,
            None => ImplKind::Written,
        };
        let block = Src::child(n, SyntaxKind::Block);
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
                let Some(Binding {
                    kind: BindingKind::Item,
                    value,
                }) = self.scope.lookup(sym)
                else {
                    let msg = format!("unknown-trait `{a}`");
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
                let tv = pool.intern_ty(&TyData::TraitValue {
                    def: tr,
                    args: TyList::EMPTY,
                    bindings: vec![],
                });
                let mut gs = Vec::new();
                let mut params = Vec::new();
                for (i, g) in generics.iter().enumerate() {
                    let index = u16::try_from(i).unwrap_or(u16::MAX);
                    params.push(pool.intern_ty(&TyData::Param(ParamRef { owner: def, index })));
                    let mut g2 = Generic::plain(g.name);
                    if !g.row {
                        g2.bound = Some(tr);
                        g2.bounds = vec![tv];
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
                        trait_args: TyList::EMPTY,
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

    fn item(&mut self, h: &Head<'_>, out: &mut Vec<Item>) {
        let n = h.node;
        let pool = self.names.pool;
        let block = Src::child(n, SyntaxKind::Block);
        let gl = Src::child(n, SyntaxKind::GenericParameterList);
        match h.kind {
            HeadKind::Fn => {
                let sig = self.sig(n, h.def, &Gen::default());
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Fn(sig));
                it.intrinsic = self.intrinsic(n);
                it.literal_fn = self.literal_fn(n);
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
                let mut it = Item::new(h.def, h.name, h.public, ItemData::Data(fields));
                it.generics = generics;
                it.targets = self.annotate_mask(n);
                out.push(it);
            }
            HeadKind::Enum => {
                let mut gn = Gen::default();
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let shared = self.fields(Src::child(n, SyntaxKind::ParameterList), &gn);
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
                let mut it =
                    Item::new(h.def, h.name, h.public, ItemData::Enum { shared, variants });
                it.generics = generics;
                it.targets = self.annotate_mask(n);
                out.push(it);
            }
            HeadKind::Trait => {
                let self_ty = pool.intern_ty(&TyData::Param(ParamRef {
                    owner: h.def,
                    index: 0,
                }));
                let mut gn = Gen {
                    self_ty: Some(self_ty),
                    self_trait: Some(h.def),
                    ..Gen::default()
                };
                let generics = self.generics(gl, h.def, 1, &mut gn);
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
                                "invalid-variance: trait parameter `{}` is invariant and takes no marker",
                                self.names.text(g.name)
                            );
                            self.diags
                                .error(Code::InvalidVariance, self.src.span(node), &msg);
                        }
                    }
                }
                let mut supers = Vec::new();
                if let Some(bl) = Src::child(n, SyntaxKind::BoundList) {
                    for b in bl.children().filter(|c| c.kind() == SyntaxKind::NamedType) {
                        if let Some(t) = self.trait_value(b, &gn) {
                            supers.push(t);
                        }
                    }
                }
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
            HeadKind::Alias | HeadKind::Newtype => {
                let mut gn = Gen::default();
                let generics = self.generics(gl, h.def, 0, &mut gn);
                let body = n.children().find(|c| c.kind().is_type());
                if body.is_some_and(|b| b.kind() == SyntaxKind::RequirementRow) {
                    self.gap("a row alias");
                    return;
                }
                let t = self.ty(body, &gn);
                let data = if h.kind == HeadKind::Alias {
                    self.r
                        .aliases
                        .borrow_mut()
                        .insert(h.def, (generics.len(), t));
                    ItemData::Alias(t)
                } else {
                    ItemData::Newtype(t)
                };
                let mut it = Item::new(h.def, h.name, h.public, data);
                it.generics = generics;
                out.push(it);
            }
        }
    }
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
            .into_iter()
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
    for it in items {
        let Some(code) = misplaced_impl(names, module, it) else {
            continue;
        };
        let span = heads
            .iter()
            .find(|h| h.def == it.def)
            .map_or(src.span(src.root()), |h| src.span(h.node));
        let what = if code == Code::NonlocalImpl {
            "nonlocal-impl"
        } else {
            "orphan-impl"
        };
        let msg = format!(
            "{what}: `{}` belongs in the module of its trait or target",
            names.path(it.def)
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
    let all_heads: Vec<Vec<Head<'_>>> =
        mods.iter().map(|m| heads(names, &m.src, &m.path)).collect();
    let all_uses: Vec<Vec<UseDecl>> = mods
        .iter()
        .map(|m| use_decls(&m.src, cx.package, &m.path))
        .collect();
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
    };
    for (m, hs) in mods.iter().zip(&all_heads) {
        for h in hs {
            if h.kind == HeadKind::Impl {
                continue;
            }
            r.own
                .entry((m.path.clone(), h.name))
                .or_insert((h.def, h.kind, h.public));
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
                    r.aliases.borrow_mut().insert(s.def, (s.generics.len(), *t));
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
    };
    let mut unsupported = None;
    for ((m, hs), us) in mods.iter().zip(&all_heads).zip(&all_uses) {
        let (scope, kinds) = scope_of(&r, m, hs, us, diags);
        let mut items: Vec<Item> = Vec::new();
        {
            let mut low = Lower {
                r: &r,
                names: cx.names,
                src: m.src,
                module: &m.path,
                scope: &scope,
                kinds: &kinds,
                diags,
                unsupported: None,
            };
            // Aliases first, so a later header in the module may use them.
            for h in hs.iter().filter(|h| h.kind == HeadKind::Alias) {
                low.item(h, &mut items);
            }
            for h in hs.iter().filter(|h| h.kind != HeadKind::Alias) {
                low.item(h, &mut items);
            }
            if unsupported.is_none() {
                unsupported = low.unsupported.take();
            }
        }
        for s in &m.seeds {
            if !items.iter().any(|i| i.def == s.def) {
                items.push(s.clone());
            }
        }
        ownership(names, &m.path, &items, hs, &m.src, diags);
        let anchors = crate::anchor::collect(names, &m.src, hs, &items);
        out.modules.push(ModOut {
            items,
            scope,
            kinds,
            anchors,
        });
    }
    if r.frozen.is_none() {
        let items: HashMap<DefId, &Item> = out
            .modules
            .iter()
            .flat_map(|m| m.items.iter())
            .map(|i| (i.def, i))
            .collect();
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
            };
            low.check_module_decorators(&items);
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
                        let msg = format!("re-export-loop `{}`", names.text(name));
                        diags.error(Code::ReExportLoop, span, &msg);
                    }
                    Err(_) => {}
                }
            }
        }
        out.exports.sort_by(|a, b| {
            (a.module.as_str(), names.text(a.name)).cmp(&(b.module.as_str(), names.text(b.name)))
        });
    }
    match unsupported {
        Some(what) => Err(NotImplemented::new(stage, what)),
        None => Ok(out),
    }
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
