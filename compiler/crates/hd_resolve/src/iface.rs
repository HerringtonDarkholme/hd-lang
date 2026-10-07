//! Item heads, module scopes, header lowering, folder interfaces and their
//! blobs, and impl tables (resolution-and-interfaces.md §4.9 to §4.11;
//! data-structures.md §3.15; trait-solver.md §3.3). Types are `hd_types`
//! types in the run's pool; a blob carries them through entry-local tables.

use std::collections::HashMap;
use std::sync::Arc;

use hd_base::wire::{Reader, Writer};
use hd_base::{
    DefId, Hash128, NotImplemented, PathId, Span, StableHasher, Stage, StageResult, Symbol,
};
use hd_diag::{Code, DiagBuf};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_types::solver::{HeadKey, ImplOrigin, ImplTable};
use hd_types::wire::{TableWriter, Tables};
use hd_types::{InternPool, ParamRef, Prim, Ty, TyData, TyList};

use crate::view::Src;
use crate::{Binding, BindingKind, ModuleScope, Origin};

/// The run-wide tables a resolver writes into.
#[derive(Clone, Copy)]
pub struct Names<'a> {
    pub pool: &'a InternPool,
    pub paths: &'a PathTable,
    pub syms: &'a ShardedInterner,
}

impl Names<'_> {
    /// The trie node of a dotted module path (`app.geo.shapes`): the first
    /// segment is the package.
    #[must_use]
    pub fn module(&self, module: &str) -> PathId {
        let mut segs = module.split('.');
        let mut p = self
            .paths
            .intern(PathId::NONE, PathKind::Package, segs.next().unwrap_or(""));
        for s in segs {
            p = self.paths.intern(p, PathKind::Module, s);
        }
        p
    }
    #[must_use]
    pub fn item(&self, module: &str, name: &str) -> DefId {
        DefId::from_raw(
            self.paths
                .intern(self.module(module), PathKind::Item, name)
                .raw(),
        )
    }
    #[must_use]
    pub fn member(&self, owner: DefId, kind: PathKind, name: &str) -> DefId {
        DefId::from_raw(
            self.paths
                .intern(PathId::from_raw(owner.raw()), kind, name)
                .raw(),
        )
    }
    /// The stable path of an item, for messages and relocations.
    #[must_use]
    pub fn path(&self, d: DefId) -> String {
        self.paths.display(PathId::from_raw(d.raw())).to_string()
    }
    #[must_use]
    pub fn path_hash(&self, d: DefId) -> Hash128 {
        self.paths.hash(PathId::from_raw(d.raw()))
    }
    #[must_use]
    pub fn text(&self, s: Symbol) -> &str {
        self.syms.resolve(s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generic {
    pub name: Symbol,
    pub bound: Option<DefId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub generics: Vec<Generic>,
    pub params: Vec<(Symbol, Ty)>,
    pub ret: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: Symbol,
    pub ty: Ty,
    pub public: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemData {
    Fn(FnSig),
    Data(Vec<Field>),
    /// Method name and its `Method` item.
    Trait(Vec<(Symbol, DefId)>),
    Impl {
        trait_: DefId,
        self_ty: Ty,
        methods: Vec<(Symbol, DefId)>,
    },
    /// A trait's or impl's method; `Self` in a trait method is the trait's
    /// parameter 0.
    Method {
        owner: DefId,
        sig: FnSig,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub def: DefId,
    pub name: Symbol,
    pub public: bool,
    pub data: ItemData,
}

impl Item {
    #[must_use]
    pub fn sig(&self) -> Option<&FnSig> {
        match &self.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => Some(s),
            _ => None,
        }
    }
}

/// An item's head: what scopes need before any type is lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadKind {
    Fn,
    Data,
    Trait,
    Impl,
}

/// Finds items by `DefId` across one module's own items and the
/// interfaces of its closure.
pub struct Lookup<'a> {
    pub own: &'a [Item],
    pub own_index: HashMap<DefId, usize>,
    pub ifaces: Vec<&'a FolderIface>,
}

impl<'a> Lookup<'a> {
    #[must_use]
    pub fn new(own: &'a [Item], ifaces: Vec<&'a FolderIface>) -> Self {
        let own_index = own.iter().enumerate().map(|(i, it)| (it.def, i)).collect();
        Self {
            own,
            own_index,
            ifaces,
        }
    }
    #[must_use]
    pub fn item(&self, d: DefId) -> Option<&'a Item> {
        if let Some(&i) = self.own_index.get(&d) {
            return self.own.get(i);
        }
        self.ifaces.iter().find_map(|f| f.item(d))
    }
    /// Every impl visible here, own first, then by folder.
    #[must_use]
    pub fn impls(&self) -> Vec<&'a Item> {
        let mut out: Vec<&Item> = self
            .own
            .iter()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }))
            .collect();
        for f in &self.ifaces {
            out.extend(
                f.items
                    .iter()
                    .filter(|i| matches!(i.data, ItemData::Impl { .. })),
            );
        }
        out
    }
}

/// `use M.{a, b}` with `pkg` replaced by the package name.
#[derive(Clone, Debug)]
pub struct UseDecl {
    pub module: String,
    pub names: Vec<(String, Span)>,
    pub span: Span,
    pub braced: bool,
}

/// The use declarations of a file, in order.
#[must_use]
pub fn use_decls(src: &Src<'_>, package: &str) -> Vec<UseDecl> {
    let mut out = Vec::new();
    for item in src
        .root()
        .children()
        .filter(|c| c.kind() == SyntaxKind::UseDecl)
    {
        let mut path: Vec<&str> = Vec::new();
        let mut names = Vec::new();
        let mut braced = false;
        for t in src.tokens(item).skip(1) {
            match src.tkind(t) {
                Some(TokenKind::LBrace) => braced = true,
                Some(TokenKind::Ident) if braced => {
                    names.push((src.text(t).to_owned(), src.token_span(t)));
                }
                Some(TokenKind::Ident) => path.push(src.text(t)),
                _ => {}
            }
        }
        if path.first() == Some(&"pkg") {
            path[0] = package;
        }
        out.push(UseDecl {
            module: path.join("."),
            names,
            span: src.span(item),
            braced,
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

/// Heads of the module's own top-level items.
#[must_use]
pub fn heads<'t>(names: &Names<'_>, src: &Src<'t>, module: &str) -> Vec<Head<'t>> {
    let mut out = Vec::new();
    for n in src.root().children() {
        let (kind, kw) = match n.kind() {
            SyntaxKind::FnDecl => (HeadKind::Fn, TokenKind::KwFn),
            SyntaxKind::DataDecl => (HeadKind::Data, TokenKind::KwData),
            SyntaxKind::TraitDecl => (HeadKind::Trait, TokenKind::KwTrait),
            SyntaxKind::ImplDecl => {
                let tys: Vec<NodeRef<'t>> = n
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::NamedType)
                    .collect();
                let seg: Vec<&str> = tys.iter().map(|t| src.text(src.first(*t))).collect();
                let name = seg.join(".");
                let def = DefId::from_raw(
                    names
                        .paths
                        .intern(names.module(module), PathKind::Impl, &name)
                        .raw(),
                );
                out.push(Head {
                    name: names.syms.intern(&name),
                    def,
                    kind: HeadKind::Impl,
                    node: n,
                    public: true,
                });
                continue;
            }
            _ => continue,
        };
        let Some(t) = src.name_after(n, kw) else {
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

/// The kind of every item a scope can name: own heads and interface items.
pub type Kinds = HashMap<DefId, HeadKind>;

fn kind_of(item: &Item) -> Option<HeadKind> {
    match item.data {
        ItemData::Fn(_) => Some(HeadKind::Fn),
        ItemData::Data(_) => Some(HeadKind::Data),
        ItemData::Trait(_) => Some(HeadKind::Trait),
        ItemData::Impl { .. } => Some(HeadKind::Impl),
        ItemData::Method { .. } => None,
    }
}

/// Builds a module's scope (§4.9): own items, then uses against the
/// interfaces of used folders, then the prelude. Returns the kinds of every
/// item the scope names.
pub fn module_scope(
    names: &Names<'_>,
    src: &Src<'_>,
    package: &str,
    module: &str,
    own: &[Head<'_>],
    iface_of: &dyn Fn(&str) -> Option<Arc<FolderIface>>,
    stage: Stage,
    diags: &mut DiagBuf,
) -> StageResult<(ModuleScope, Kinds)> {
    let mut scope = ModuleScope::default();
    let mut kinds = Kinds::new();
    for h in own {
        if h.kind != HeadKind::Impl {
            scope.bind(
                h.name,
                Binding {
                    kind: BindingKind::Item,
                    value: h.def.raw(),
                },
                Origin::Own,
                u32::MAX,
            );
        }
        kinds.insert(h.def, h.kind);
    }
    for (row, u) in use_decls(src, package).into_iter().enumerate() {
        let row = u32::try_from(row).expect("uses");
        if !u.braced {
            return Err(NotImplemented::new(
                stage,
                format!("a use without braces (`use {}`)", u.module),
            ));
        }
        let folder = hd_project::folder_of(&u.module);
        if folder == hd_project::folder_of(module) {
            return Err(NotImplemented::new(
                stage,
                "a use of a module in the same folder",
            ));
        }
        let Some(iface) = iface_of(folder) else {
            diags.error(
                Code::UnknownModule,
                u.span,
                &format!("unknown-module `{}`", u.module),
            );
            continue;
        };
        for (name, span) in u.names {
            let def = names.item(&u.module, &name);
            match iface.item(def) {
                Some(it) if it.public => {
                    scope.bind(
                        names.syms.intern(&name),
                        Binding {
                            kind: BindingKind::Item,
                            value: def.raw(),
                        },
                        Origin::Use,
                        row,
                    );
                    if let Some(k) = kind_of(it) {
                        kinds.insert(def, k);
                    }
                }
                _ => diags.error(
                    Code::UnknownImport,
                    span,
                    &format!("unknown-import `{name}` in `{}`", u.module),
                ),
            }
        }
        for it in &iface.items {
            if let Some(k) = kind_of(it) {
                kinds.entry(it.def).or_insert(k);
            }
        }
    }
    for (i, p) in hd_host_abi::PRELUDE_IMPORTS.iter().enumerate() {
        let i = u32::try_from(i).expect("prelude");
        scope.bind(
            names.syms.intern(p.name),
            Binding {
                kind: BindingKind::Prelude,
                value: i,
            },
            Origin::Prelude,
            u32::MAX,
        );
    }
    Ok((scope, kinds))
}

struct Lower<'a, 'n> {
    names: &'a Names<'n>,
    src: &'a Src<'a>,
    scope: &'a ModuleScope,
    kinds: &'a Kinds,
    diags: &'a mut DiagBuf,
    unsupported: Option<String>,
}

impl Lower<'_, '_> {
    fn ty(&mut self, n: Option<NodeRef<'_>>, generics: &[(Symbol, Ty)], self_ty: Option<Ty>) -> Ty {
        let Some(n) = n else { return Ty::VOID };
        if n.kind() != SyntaxKind::NamedType {
            self.unsupported
                .get_or_insert_with(|| "this type form".into());
            return Ty::POISON;
        }
        if n.children().next().is_some() {
            self.unsupported
                .get_or_insert_with(|| "type arguments in a header".into());
            return Ty::POISON;
        }
        let t = self.src.first(n);
        if self.src.tkind(t) == Some(TokenKind::KwSelfType) {
            return self_ty.unwrap_or(Ty::POISON);
        }
        let name = self.src.text(t);
        if let Some(p) = Prim::ALL.iter().find(|p| p.name() == name) {
            return Ty::prim(*p);
        }
        let sym = self.names.syms.intern(name);
        if let Some((_, t)) = generics.iter().find(|(g, _)| *g == sym) {
            return *t;
        }
        match self.scope.lookup(sym) {
            Some(Binding {
                kind: BindingKind::Item,
                value,
            }) if self.kinds.get(&DefId::from_raw(value)) == Some(&HeadKind::Data) => {
                self.names.pool.intern_ty(&TyData::Adt {
                    def: DefId::from_raw(value),
                    args: TyList::EMPTY,
                })
            }
            _ => {
                self.diags.error(
                    Code::UnknownType,
                    self.src.span(n),
                    &format!("unknown-type `{name}`"),
                );
                Ty::POISON
            }
        }
    }

    fn trait_ref(&mut self, n: NodeRef<'_>) -> Option<DefId> {
        let name = self.src.text(self.src.first(n));
        match self.scope.lookup(self.names.syms.intern(name)) {
            Some(Binding {
                kind: BindingKind::Item,
                value,
            }) if self.kinds.get(&DefId::from_raw(value)) == Some(&HeadKind::Trait) => {
                Some(DefId::from_raw(value))
            }
            _ => {
                self.diags.error(
                    Code::UnknownTrait,
                    self.src.span(n),
                    &format!("unknown-trait `{name}`"),
                );
                None
            }
        }
    }

    /// A function header: generics, parameters, result.
    fn sig(&mut self, f: NodeRef<'_>, owner: DefId, self_ty: Option<Ty>) -> FnSig {
        let mut generics = Vec::new();
        let mut gtys = Vec::new();
        if let Some(gl) = Src::child(f, SyntaxKind::GenericParameterList) {
            for (i, g) in gl
                .children()
                .filter(|c| c.kind() == SyntaxKind::GenericParameter)
                .enumerate()
            {
                let Some(t) = self.src.first_ident(g) else {
                    continue;
                };
                let name = self.names.syms.intern(self.src.text(t));
                let index = u16::try_from(i).expect("generics");
                gtys.push((
                    name,
                    self.names
                        .pool
                        .intern_ty(&TyData::Param(ParamRef { owner, index })),
                ));
                let bound = Src::child(g, SyntaxKind::NamedType).and_then(|b| self.trait_ref(b));
                generics.push(Generic { name, bound });
            }
        }
        let mut params = Vec::new();
        if let Some(pl) = Src::child(f, SyntaxKind::ParameterList) {
            for p in pl.children().filter(|c| c.kind() == SyntaxKind::Parameter) {
                if self.src.tkind(self.src.first(p)) == Some(TokenKind::KwSelfValue) {
                    params.push((
                        self.names.syms.intern("self"),
                        self_ty.unwrap_or(Ty::POISON),
                    ));
                    continue;
                }
                let Some(t) = self.src.first_ident(p) else {
                    continue;
                };
                let ty = self.ty(Src::child(p, SyntaxKind::NamedType), &gtys, self_ty);
                params.push((self.names.syms.intern(self.src.text(t)), ty));
            }
        }
        let ret = self.ty(Src::child(f, SyntaxKind::NamedType), &gtys, self_ty);
        FnSig {
            generics,
            params,
            ret,
        }
    }
}

/// Lowers every header of a module to items (§4.10 step 3). Private items
/// are included; a folder interface keeps the public ones and impls.
pub fn lower_items(
    names: &Names<'_>,
    src: &Src<'_>,
    heads: &[Head<'_>],
    scope: &ModuleScope,
    kinds: &Kinds,
    stage: Stage,
    diags: &mut DiagBuf,
) -> StageResult<Vec<Item>> {
    let mut low = Lower {
        names,
        src,
        scope,
        kinds,
        diags,
        unsupported: None,
    };
    let mut out = Vec::new();
    for h in heads {
        let n = h.node;
        let body = Src::child(n, SyntaxKind::Block);
        match h.kind {
            HeadKind::Fn => {
                let sig = low.sig(n, h.def, None);
                out.push(Item {
                    def: h.def,
                    name: h.name,
                    public: h.public,
                    data: ItemData::Fn(sig),
                });
            }
            HeadKind::Data => {
                let mut fields = Vec::new();
                for f in body
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::DataField)
                {
                    let Some(t) = src.first_ident(f) else {
                        continue;
                    };
                    let ty = low.ty(Src::child(f, SyntaxKind::NamedType), &[], None);
                    fields.push(Field {
                        name: names.syms.intern(src.text(t)),
                        ty,
                        public: src.is_pub(f),
                    });
                }
                out.push(Item {
                    def: h.def,
                    name: h.name,
                    public: h.public,
                    data: ItemData::Data(fields),
                });
            }
            HeadKind::Trait => {
                let self_ty = names.pool.intern_ty(&TyData::Param(ParamRef {
                    owner: h.def,
                    index: 0,
                }));
                let mut methods = Vec::new();
                for f in body
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                {
                    let Some(t) = src.name_after(f, TokenKind::KwFn) else {
                        continue;
                    };
                    let name = names.syms.intern(src.text(t));
                    let def = names.member(h.def, PathKind::Member, src.text(t));
                    let sig = low.sig(f, def, Some(self_ty));
                    out.push(Item {
                        def,
                        name,
                        public: h.public,
                        data: ItemData::Method { owner: h.def, sig },
                    });
                    methods.push((name, def));
                }
                out.push(Item {
                    def: h.def,
                    name: h.name,
                    public: h.public,
                    data: ItemData::Trait(methods),
                });
            }
            HeadKind::Impl => {
                let tys: Vec<NodeRef<'_>> = n
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::NamedType)
                    .collect();
                let [tr, target] = tys.as_slice() else {
                    low.unsupported
                        .get_or_insert_with(|| "an impl without a trait".into());
                    continue;
                };
                let Some(trait_) = low.trait_ref(*tr) else {
                    continue;
                };
                let self_ty = low.ty(Some(*target), &[], None);
                let mut methods = Vec::new();
                for f in body
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                {
                    let Some(t) = src.name_after(f, TokenKind::KwFn) else {
                        continue;
                    };
                    let name = names.syms.intern(src.text(t));
                    let def = names.member(h.def, PathKind::Member, src.text(t));
                    let sig = low.sig(f, def, Some(self_ty));
                    out.push(Item {
                        def,
                        name,
                        public: true,
                        data: ItemData::Method { owner: h.def, sig },
                    });
                    methods.push((name, def));
                }
                out.push(Item {
                    def: h.def,
                    name: h.name,
                    public: true,
                    data: ItemData::Impl {
                        trait_,
                        self_ty,
                        methods,
                    },
                });
            }
        }
    }
    match low.unsupported {
        Some(what) => Err(NotImplemented::new(stage, what)),
        None => Ok(out),
    }
}

/// The function nodes with bodies, by item: own functions and impl methods.
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
            HeadKind::Impl => {
                for f in Src::child(h.node, SyntaxKind::Block)
                    .iter()
                    .flat_map(|b| b.children())
                {
                    if f.kind() == SyntaxKind::FnDecl
                        && let Some(t) = src.name_after(f, TokenKind::KwFn)
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

// ------------------------------------------------------------- the blob

/// A folder's interface (§4.10): its public items, impls and their
/// methods, the blob they were read from or written to, and the deep hash.
#[derive(Debug)]
pub struct FolderIface {
    pub folder: String,
    pub items: Vec<Item>,
    pub by_def: HashMap<DefId, usize>,
    pub blob: Arc<[u8]>,
    pub deep_hash: Hash128,
}

impl FolderIface {
    #[must_use]
    pub fn item(&self, d: DefId) -> Option<&Item> {
        self.by_def.get(&d).and_then(|&i| self.items.get(i))
    }
}

fn put_sig(w: &mut Writer, t: &mut TableWriter<'_>, s: &FnSig) -> StageResult<()> {
    w.len_of(&s.generics);
    for g in &s.generics {
        w.u32(t.sym(g.name));
        w.u32(g.bound.map_or(u32::MAX, |b| t.def(b)));
    }
    w.len_of(&s.params);
    for (n, ty) in &s.params {
        w.u32(t.sym(*n));
        w.u32(t.ty(*ty)?);
    }
    w.u32(t.ty(s.ret)?);
    Ok(())
}

fn get_sig(r: &mut Reader<'_>, t: &Tables) -> Option<FnSig> {
    let mut generics = Vec::new();
    for _ in 0..r.count() {
        let name = t.sym(r.u32())?;
        let b = r.u32();
        generics.push(Generic {
            name,
            bound: if b == u32::MAX { None } else { Some(t.def(b)?) },
        });
    }
    let mut params = Vec::new();
    for _ in 0..r.count() {
        let n = t.sym(r.u32())?;
        params.push((n, t.ty(r.u32())?));
    }
    Some(FnSig {
        generics,
        params,
        ret: t.ty(r.u32())?,
    })
}

/// Encodes items: entry-local tables, then the item records.
pub fn encode_items(names: &Names<'_>, items: &[Item]) -> StageResult<Vec<u8>> {
    let mut t = TableWriter::new(names.pool, names.paths, names.syms);
    let mut w = Writer::default();
    w.len_of(items);
    for it in items {
        w.u32(t.def(it.def));
        w.u32(t.sym(it.name));
        w.u8(u8::from(it.public));
        match &it.data {
            ItemData::Fn(s) => {
                w.u8(0);
                put_sig(&mut w, &mut t, s)?;
            }
            ItemData::Data(fields) => {
                w.u8(1);
                w.len_of(fields);
                for f in fields {
                    w.u32(t.sym(f.name));
                    w.u32(t.ty(f.ty)?);
                    w.u8(u8::from(f.public));
                }
            }
            ItemData::Trait(ms) => {
                w.u8(2);
                w.len_of(ms);
                for (n, d) in ms {
                    w.u32(t.sym(*n));
                    w.u32(t.def(*d));
                }
            }
            ItemData::Impl {
                trait_,
                self_ty,
                methods,
            } => {
                w.u8(3);
                w.u32(t.def(*trait_));
                w.u32(t.ty(*self_ty)?);
                w.len_of(methods);
                for (n, d) in methods {
                    w.u32(t.sym(*n));
                    w.u32(t.def(*d));
                }
            }
            ItemData::Method { owner, sig } => {
                w.u8(4);
                w.u32(t.def(*owner));
                put_sig(&mut w, &mut t, sig)?;
            }
        }
    }
    let mut out = Writer::default();
    t.write(&mut out);
    out.bytes.extend_from_slice(&w.bytes);
    Ok(out.bytes)
}

/// Decodes items into this run; `None` on a malformed blob (a miss).
#[must_use]
pub fn decode_items(names: &Names<'_>, bytes: &[u8]) -> Option<Vec<Item>> {
    let mut r = Reader::new(bytes);
    let t = Tables::read(&mut r, names.pool, names.paths, names.syms)?;
    let mut items = Vec::new();
    for _ in 0..r.count() {
        let def = t.def(r.u32())?;
        let name = t.sym(r.u32())?;
        let public = r.u8() != 0;
        let data = match r.u8() {
            0 => ItemData::Fn(get_sig(&mut r, &t)?),
            1 => {
                let mut fields = Vec::new();
                for _ in 0..r.count() {
                    let name = t.sym(r.u32())?;
                    let ty = t.ty(r.u32())?;
                    fields.push(Field {
                        name,
                        ty,
                        public: r.u8() != 0,
                    });
                }
                ItemData::Data(fields)
            }
            2 => {
                let mut ms = Vec::new();
                for _ in 0..r.count() {
                    let n = t.sym(r.u32())?;
                    ms.push((n, t.def(r.u32())?));
                }
                ItemData::Trait(ms)
            }
            3 => {
                let trait_ = t.def(r.u32())?;
                let self_ty = t.ty(r.u32())?;
                let mut methods = Vec::new();
                for _ in 0..r.count() {
                    let n = t.sym(r.u32())?;
                    methods.push((n, t.def(r.u32())?));
                }
                ItemData::Impl {
                    trait_,
                    self_ty,
                    methods,
                }
            }
            4 => {
                let owner = t.def(r.u32())?;
                ItemData::Method {
                    owner,
                    sig: get_sig(&mut r, &t)?,
                }
            }
            _ => return None,
        };
        items.push(Item {
            def,
            name,
            public,
            data,
        });
    }
    r.ok().then_some(items)
}

/// `deep_hash(F) = H(blob, deep hashes of the folders F uses)` (§4.11).
#[must_use]
pub fn deep_hash(blob: &[u8], used: &[Hash128]) -> Hash128 {
    let mut h = StableHasher::new("deep");
    h.hash(hd_base::hash128(blob));
    h.u32(u32::try_from(used.len()).expect("uses"));
    for u in used {
        h.hash(*u);
    }
    h.finish()
}

/// Keeps what other folders may see: public items, impls and the methods
/// of both.
#[must_use]
pub fn interface_items(items: &[Item]) -> Vec<Item> {
    items.iter().filter(|i| i.public).cloned().collect()
}

/// A folder interface over items (already filtered) and their blob.
#[must_use]
pub fn folder_iface(
    folder: &str,
    items: Vec<Item>,
    blob: Arc<[u8]>,
    used: &[Hash128],
) -> FolderIface {
    let by_def = items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.def, i))
        .collect();
    let deep_hash = deep_hash(&blob, used);
    FolderIface {
        folder: folder.to_owned(),
        items,
        by_def,
        blob,
        deep_hash,
    }
}

/// The solver's impl table (trait-solver.md §3.3) over the impls of a
/// scope: rows grouped by trait, in stable path order within a trait.
#[must_use]
pub fn impl_table(names: &Names<'_>, impls: &[&Item]) -> ImplTable {
    let mut rows: Vec<(DefId, DefId, Ty, Hash128)> = impls
        .iter()
        .filter_map(|i| match i.data {
            ItemData::Impl {
                trait_, self_ty, ..
            } => Some((trait_, i.def, self_ty, names.path_hash(i.def))),
            _ => None,
        })
        .collect();
    rows.sort_by_key(|r| (names.path_hash(r.0), r.3));
    let mut t = ImplTable::default();
    for (n, (trait_, def, self_ty, rank)) in rows.into_iter().enumerate() {
        let n = u32::try_from(n).expect("impls");
        t.by_trait
            .entry(trait_.raw())
            .and_modify(|e| e.1 = n + 1)
            .or_insert((n, n + 1));
        t.trait_.push(trait_);
        t.def.push(def);
        t.head_key.push(HeadKey::of(names.pool, self_ty));
        t.arg_key.push([HeadKey::Any; 2]);
        t.n_params.push(0);
        t.head_self.push(self_ty);
        t.head_args.push(TyList::EMPTY);
        t.plan.push(vec![]);
        t.assoc.push(vec![]);
        t.origin.push(ImplOrigin::Written);
        t.rank
            .push(u64::try_from(rank.0 & u128::from(u64::MAX)).expect("rank"));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::{FnSig, Generic, Item, ItemData, Names, decode_items, encode_items};
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::{InternPool, ParamRef, Ty, TyData};

    #[test]
    fn items_round_trip_into_a_fresh_run() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let n = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
        };
        let f = n.item("app.geo", "first");
        let t = pool.intern_ty(&TyData::Param(ParamRef { owner: f, index: 0 }));
        let sig = FnSig {
            generics: vec![Generic {
                name: syms.intern("T"),
                bound: None,
            }],
            params: vec![(syms.intern("a"), t)],
            ret: t,
        };
        let items = vec![Item {
            def: f,
            name: syms.intern("first"),
            public: true,
            data: ItemData::Fn(sig),
        }];
        let blob = encode_items(&n, &items).expect("encode");
        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let n2 = Names {
            pool: &pool2,
            paths: &paths2,
            syms: &syms2,
        };
        let back = decode_items(&n2, &blob).expect("decode");
        assert_eq!(back[0].def, n2.item("app.geo", "first"));
        assert_eq!(encode_items(&n2, &back).expect("again"), blob);
        let _ = Ty::I32;
    }
}
