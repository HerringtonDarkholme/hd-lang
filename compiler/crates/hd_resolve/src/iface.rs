//! Interface records, folder interfaces and their blobs, and impl tables
//! (resolution-and-interfaces.md §4.10 to §4.12; data-structures.md §3.15;
//! trait-solver.md §3.3). Types are `hd_types` types in the run's pool; a
//! blob carries them through entry-local tables.

use std::collections::HashMap;
use std::sync::Arc;

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, Hash128, PathId, StableHasher, StageResult, Symbol};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_types::solver::{HeadKey, ImplOrigin, ImplTable, PlanStep};
use hd_types::wire::{TableWriter, Tables};
use hd_types::{InternPool, RowId, Ty, TyData, TyList};

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
    /// The dotted path of the module that declares `d` (`std.format`).
    #[must_use]
    pub fn module_of(&self, d: DefId) -> String {
        let mut p = PathId::from_raw(d.raw());
        while p.get().is_some()
            && !matches!(self.paths.kind(p), PathKind::Module | PathKind::Package)
        {
            p = self.paths.parent(p);
        }
        let mut segs = Vec::new();
        while p.get().is_some() {
            segs.push(self.paths.segment(p).to_owned());
            p = self.paths.parent(p);
        }
        segs.reverse();
        segs.join(".")
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

/// A declared type or row parameter: its bounds as trait-value types (the
/// trait's own arguments and bindings included), and its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Generic {
    pub name: Symbol,
    /// The first bound's trait, for checkers that read one bound.
    pub bound: Option<DefId>,
    pub bounds: Vec<Ty>,
    pub default: Option<Ty>,
    /// A row parameter (`$R`).
    pub row: bool,
    /// A `mut` bound (`T < mut Trait`, `T < mut Any`): values of the
    /// parameter have mutable access (types.path.access.mut-bound).
    pub mut_bound: bool,
    /// The declared variance marker: `+T` is 1, `-T` is -1, unmarked 0.
    pub variance: i8,
}

impl Generic {
    #[must_use]
    pub fn plain(name: Symbol) -> Self {
        Self {
            name,
            bound: None,
            bounds: Vec::new(),
            default: None,
            row: false,
            mut_bound: false,
            variance: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnSig {
    pub generics: Vec<Generic>,
    pub params: Vec<(Symbol, Ty)>,
    /// Per parameter: whether it has a default (defaults are not in the
    /// interface as expressions, §4.10).
    pub defaults: Vec<bool>,
    pub ret: Ty,
    pub row: RowId,
    pub suspends: bool,
    /// The last parameter collects the rest of the arguments (`xs...: T`).
    pub variadic: bool,
}

impl FnSig {
    #[must_use]
    pub fn simple(generics: Vec<Generic>, params: Vec<(Symbol, Ty)>, ret: Ty) -> Self {
        let n = params.len();
        Self {
            generics,
            params,
            defaults: vec![false; n],
            ret,
            row: RowId::EMPTY,
            suspends: false,
            variadic: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: Symbol,
    pub ty: Ty,
    pub public: bool,
    pub has_default: bool,
    pub embedded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub name: Symbol,
    pub def: DefId,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TraitData {
    pub methods: Vec<(Symbol, DefId)>,
    pub assoc: Vec<(Symbol, DefId)>,
    /// Supertraits as trait-value types over the trait's `Self`.
    pub supers: Vec<Ty>,
}

/// What an impl declaration is (trait-solver.md §3.10, annotations §Templates).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ImplKind {
    /// A written implementation (or an inherent block when the trait is NONE).
    Written,
    /// `impl Tr for T by E`: delegates to the embedded field `E`.
    Delegated,
    /// `impl[T] Tr for T by Structure`: a derivation template, no coherence slot.
    Template,
    /// `impl[T < Tuple] Tr for T by Structure`: every tuple, at every size.
    TupleTemplate,
    /// `impl Tr for C by Structure`: a derivation block for one target.
    Derivation,
    /// `@derive(Tr)` on a declaration.
    Derived,
}

impl ImplKind {
    const ALL: [ImplKind; 6] = [
        ImplKind::Written,
        ImplKind::Delegated,
        ImplKind::Template,
        ImplKind::TupleTemplate,
        ImplKind::Derivation,
        ImplKind::Derived,
    ];
    /// Whether the head takes a coherence slot and an impl-table row.
    #[must_use]
    pub fn is_impl(self) -> bool {
        self != ImplKind::Template
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemData {
    Fn(FnSig),
    Data(Vec<Field>),
    Enum {
        shared: Vec<Field>,
        variants: Vec<Variant>,
    },
    Trait(TraitData),
    /// `trait_` is `DefId::NONE` for an inherent block.
    Impl {
        trait_: DefId,
        trait_args: TyList,
        self_ty: Ty,
        methods: Vec<(Symbol, DefId)>,
        assoc: Vec<(DefId, Ty)>,
        by: Option<Symbol>,
        kind: ImplKind,
    },
    /// A trait's or impl's method; `Self` in a trait method is the trait's
    /// parameter 0, and the trait's declared parameters follow it.
    Method {
        owner: DefId,
        sig: FnSig,
        has_body: bool,
    },
    AssocType {
        owner: DefId,
        bounds: Vec<Ty>,
        default: Option<Ty>,
    },
    Alias(Ty),
    Newtype(Ty),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub def: DefId,
    pub name: Symbol,
    pub public: bool,
    /// The declaration's own parameters (a function's are in its `FnSig`).
    pub generics: Vec<Generic>,
    /// `@intrinsic("key")`: the body is the compiler's.
    pub intrinsic: Option<Symbol>,
    /// The target kinds a fact type's `@annotate(...)` line allows, as the
    /// bits of `crate::lower::kind` (`annot.target.limit`); `None` limits
    /// nothing.
    pub targets: Option<u16>,
    /// A function marked `@num_suffix` (`LITERAL_SUFFIX`) or `@str_prefix`
    /// (`LITERAL_PREFIX`), or 0 (`expr.literal-fn.marker`).
    pub literal_fn: u8,
    pub data: ItemData,
}

/// `Item::literal_fn` of a suffix function.
pub const LITERAL_SUFFIX: u8 = 1;
/// `Item::literal_fn` of a prefix function.
pub const LITERAL_PREFIX: u8 = 2;

impl Item {
    #[must_use]
    pub fn new(def: DefId, name: Symbol, public: bool, data: ItemData) -> Self {
        Self {
            def,
            name,
            public,
            generics: Vec::new(),
            intrinsic: None,
            targets: None,
            literal_fn: 0,
            data,
        }
    }
    #[must_use]
    pub fn sig(&self) -> Option<&FnSig> {
        match &self.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => Some(s),
            _ => None,
        }
    }
    #[must_use]
    pub fn kind(&self) -> Option<HeadKind> {
        Some(match self.data {
            ItemData::Fn(_) => HeadKind::Fn,
            ItemData::Data(_) => HeadKind::Data,
            ItemData::Enum { .. } => HeadKind::Enum,
            ItemData::Trait(_) => HeadKind::Trait,
            ItemData::Impl { .. } => HeadKind::Impl,
            ItemData::Alias(_) => HeadKind::Alias,
            ItemData::Newtype(_) => HeadKind::Newtype,
            ItemData::Method { .. } | ItemData::AssocType { .. } => return None,
        })
    }
}

/// An item's head: what scopes need before any type is lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HeadKind {
    Fn,
    Data,
    Enum,
    Trait,
    Impl,
    Alias,
    Newtype,
}

impl HeadKind {
    const ALL: [HeadKind; 7] = [
        HeadKind::Fn,
        HeadKind::Data,
        HeadKind::Enum,
        HeadKind::Trait,
        HeadKind::Impl,
        HeadKind::Alias,
        HeadKind::Newtype,
    ];
    /// Names a type constructor.
    #[must_use]
    pub fn is_type(self) -> bool {
        matches!(
            self,
            HeadKind::Data | HeadKind::Enum | HeadKind::Alias | HeadKind::Newtype
        )
    }
}

/// One exported name of a module, `pub use` chains followed to the real
/// declaration (§4.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub module: String,
    pub name: Symbol,
    pub def: DefId,
    pub kind: HeadKind,
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
    /// Every trait impl visible here (templates excluded), own first, then
    /// by folder.
    #[must_use]
    pub fn impls(&self) -> Vec<&'a Item> {
        let is_impl = |i: &&Item| matches!(i.data, ItemData::Impl { trait_, kind, .. } if trait_ != DefId::NONE && kind.is_impl());
        let mut out: Vec<&Item> = self.own.iter().filter(is_impl).collect();
        for f in &self.ifaces {
            out.extend(f.items.iter().filter(is_impl));
        }
        out
    }
}

// ------------------------------------------------------------- the blob

/// A folder's interface (§4.10): its exported items, every impl and their
/// members, the export index with chains followed, the blob they were read
/// from or written to, and the deep hash.
#[derive(Debug)]
pub struct FolderIface {
    pub folder: String,
    pub items: Vec<Item>,
    pub by_def: HashMap<DefId, usize>,
    pub exports: Vec<Export>,
    pub export_index: HashMap<(String, Symbol), usize>,
    pub blob: Arc<[u8]>,
    pub api_hash: Hash128,
    pub deep_hash: Hash128,
    /// Where each item and written type sits: `(item, slot)` to (module
    /// index in the folder, anchor). Outside every hash; diagnostics only.
    pub spans: HashMap<(DefId, u32), (u32, crate::anchor::Anchor)>,
}

impl FolderIface {
    #[must_use]
    pub fn item(&self, d: DefId) -> Option<&Item> {
        self.by_def.get(&d).and_then(|&i| self.items.get(i))
    }
    #[must_use]
    pub fn export(&self, module: &str, name: Symbol) -> Option<&Export> {
        self.export_index
            .get(&(module.to_owned(), name))
            .and_then(|&i| self.exports.get(i))
    }
    /// The exported names of one module, in name order.
    #[must_use]
    pub fn exports_of(&self, module: &str) -> Vec<&Export> {
        self.exports.iter().filter(|e| e.module == module).collect()
    }
}

fn put_generics(w: &mut Writer, t: &mut TableWriter<'_>, gs: &[Generic]) -> StageResult<()> {
    w.len_of(gs);
    for g in gs {
        w.u32(t.sym(g.name));
        w.u32(g.bound.map_or(u32::MAX, |b| t.def(b)));
        w.len_of(&g.bounds);
        for b in &g.bounds {
            w.u32(t.ty(*b)?);
        }
        w.u32(match g.default {
            Some(d) => t.ty(d)?,
            None => u32::MAX,
        });
        let variance = match g.variance {
            1 => 0b0100,
            -1 => 0b1000,
            _ => 0,
        };
        w.u8(u8::from(g.row) | (u8::from(g.mut_bound) << 1) | variance);
    }
    Ok(())
}

fn get_generics(r: &mut Reader<'_>, t: &Tables) -> Option<Vec<Generic>> {
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let name = t.sym(r.u32())?;
        let b = r.u32();
        let bound = if b == u32::MAX { None } else { Some(t.def(b)?) };
        let mut bounds = Vec::new();
        for _ in 0..r.count() {
            bounds.push(t.ty(r.u32())?);
        }
        let d = r.u32();
        let default = if d == u32::MAX { None } else { Some(t.ty(d)?) };
        let flags = r.u8();
        out.push(Generic {
            name,
            bound,
            bounds,
            default,
            row: flags & 1 != 0,
            mut_bound: flags & 2 != 0,
            variance: match flags & 0b1100 {
                0b0100 => 1,
                0b1000 => -1,
                _ => 0,
            },
        });
    }
    Some(out)
}

fn put_sig(w: &mut Writer, t: &mut TableWriter<'_>, s: &FnSig) -> StageResult<()> {
    put_generics(w, t, &s.generics)?;
    w.len_of(&s.params);
    for (i, (n, ty)) in s.params.iter().enumerate() {
        w.u32(t.sym(*n));
        w.u32(t.ty(*ty)?);
        w.u8(u8::from(s.defaults.get(i).copied().unwrap_or(false)));
    }
    w.u32(t.ty(s.ret)?);
    w.u32(t.row_id(s.row)?);
    w.u8(u8::from(s.suspends) | (u8::from(s.variadic) << 1));
    Ok(())
}

fn get_sig(r: &mut Reader<'_>, t: &Tables) -> Option<FnSig> {
    let generics = get_generics(r, t)?;
    let mut params = Vec::new();
    let mut defaults = Vec::new();
    for _ in 0..r.count() {
        let n = t.sym(r.u32())?;
        params.push((n, t.ty(r.u32())?));
        defaults.push(r.u8() != 0);
    }
    let ret = t.ty(r.u32())?;
    let row = RowId(*t.rows.get(r.u32() as usize)?);
    let flags = r.u8();
    Some(FnSig {
        generics,
        params,
        defaults,
        ret,
        row,
        suspends: flags & 1 != 0,
        variadic: flags & 2 != 0,
    })
}

fn put_fields(w: &mut Writer, t: &mut TableWriter<'_>, fields: &[Field]) -> StageResult<()> {
    w.len_of(fields);
    for f in fields {
        w.u32(t.sym(f.name));
        w.u32(t.ty(f.ty)?);
        w.u8(u8::from(f.public) | (u8::from(f.has_default) << 1) | (u8::from(f.embedded) << 2));
    }
    Ok(())
}

fn get_fields(r: &mut Reader<'_>, t: &Tables) -> Option<Vec<Field>> {
    let mut fields = Vec::new();
    for _ in 0..r.count() {
        let name = t.sym(r.u32())?;
        let ty = t.ty(r.u32())?;
        let f = r.u8();
        fields.push(Field {
            name,
            ty,
            public: f & 1 != 0,
            has_default: f & 2 != 0,
            embedded: f & 4 != 0,
        });
    }
    Some(fields)
}

fn put_pairs(w: &mut Writer, t: &mut TableWriter<'_>, ps: &[(Symbol, DefId)]) {
    w.len_of(ps);
    for (n, d) in ps {
        w.u32(t.sym(*n));
        w.u32(t.def(*d));
    }
}

fn get_pairs(r: &mut Reader<'_>, t: &Tables) -> Option<Vec<(Symbol, DefId)>> {
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let n = t.sym(r.u32())?;
        out.push((n, t.def(r.u32())?));
    }
    Some(out)
}

fn put_tys(w: &mut Writer, t: &mut TableWriter<'_>, tys: &[Ty]) -> StageResult<()> {
    w.len_of(tys);
    for x in tys {
        w.u32(t.ty(*x)?);
    }
    Ok(())
}

fn get_tys(r: &mut Reader<'_>, t: &Tables) -> Option<Vec<Ty>> {
    (0..r.count()).map(|_| t.ty(r.u32())).collect()
}

fn opt_ty(t: &mut TableWriter<'_>, x: Option<Ty>) -> StageResult<u32> {
    match x {
        Some(x) => t.ty(x),
        None => Ok(u32::MAX),
    }
}

/// An optional type row: `Ok(None)` for the absent marker, `Err(())` for
/// a malformed row.
fn get_opt_ty(r: &mut Reader<'_>, t: &Tables) -> Result<Option<Ty>, ()> {
    let x = r.u32();
    if x == u32::MAX {
        Ok(None)
    } else {
        t.ty(x).map(Some).ok_or(())
    }
}

fn put_item(w: &mut Writer, t: &mut TableWriter<'_>, it: &Item) -> StageResult<()> {
    w.u32(t.def(it.def));
    w.u32(t.sym(it.name));
    w.u8(u8::from(it.public));
    put_generics(w, t, &it.generics)?;
    w.u32(it.intrinsic.map_or(u32::MAX, |s| t.sym(s)));
    w.u32(it.targets.map_or(u32::MAX, u32::from));
    w.u8(it.literal_fn);
    match &it.data {
        ItemData::Fn(s) => {
            w.u8(0);
            put_sig(w, t, s)?;
        }
        ItemData::Data(fields) => {
            w.u8(1);
            put_fields(w, t, fields)?;
        }
        ItemData::Trait(td) => {
            w.u8(2);
            put_pairs(w, t, &td.methods);
            put_pairs(w, t, &td.assoc);
            put_tys(w, t, &td.supers)?;
        }
        ItemData::Impl {
            trait_,
            trait_args,
            self_ty,
            methods,
            assoc,
            by,
            kind,
        } => {
            w.u8(3);
            w.u32(if *trait_ == DefId::NONE {
                u32::MAX
            } else {
                t.def(*trait_)
            });
            w.u32(t.list(*trait_args)?);
            w.u32(t.ty(*self_ty)?);
            put_pairs(w, t, methods);
            w.len_of(assoc);
            for (d, x) in assoc {
                w.u32(t.def(*d));
                w.u32(t.ty(*x)?);
            }
            w.u32(by.map_or(u32::MAX, |s| t.sym(s)));
            w.u8(*kind as u8);
        }
        ItemData::Method {
            owner,
            sig,
            has_body,
        } => {
            w.u8(4);
            w.u32(t.def(*owner));
            put_sig(w, t, sig)?;
            w.u8(u8::from(*has_body));
        }
        ItemData::Enum { shared, variants } => {
            w.u8(5);
            put_fields(w, t, shared)?;
            w.len_of(variants);
            for v in variants {
                w.u32(t.sym(v.name));
                w.u32(t.def(v.def));
                put_fields(w, t, &v.fields)?;
            }
        }
        ItemData::AssocType {
            owner,
            bounds,
            default,
        } => {
            w.u8(6);
            w.u32(t.def(*owner));
            put_tys(w, t, bounds)?;
            w.u32(opt_ty(t, *default)?);
        }
        ItemData::Alias(x) => {
            w.u8(7);
            w.u32(t.ty(*x)?);
        }
        ItemData::Newtype(x) => {
            w.u8(8);
            w.u32(t.ty(*x)?);
        }
    }
    Ok(())
}

fn get_item(r: &mut Reader<'_>, t: &Tables) -> Option<Item> {
    let def = t.def(r.u32())?;
    let name = t.sym(r.u32())?;
    let public = r.u8() != 0;
    let generics = get_generics(r, t)?;
    let i = r.u32();
    let intrinsic = if i == u32::MAX { None } else { Some(t.sym(i)?) };
    let targets = match r.u32() {
        u32::MAX => None,
        m => Some(u16::try_from(m).ok()?),
    };
    let literal_fn = r.u8();
    let data = match r.u8() {
        0 => ItemData::Fn(get_sig(r, t)?),
        1 => ItemData::Data(get_fields(r, t)?),
        2 => ItemData::Trait(TraitData {
            methods: get_pairs(r, t)?,
            assoc: get_pairs(r, t)?,
            supers: get_tys(r, t)?,
        }),
        3 => {
            let tr = r.u32();
            let trait_ = if tr == u32::MAX {
                DefId::NONE
            } else {
                t.def(tr)?
            };
            let trait_args = t.list(r.u32())?;
            let self_ty = t.ty(r.u32())?;
            let methods = get_pairs(r, t)?;
            let mut assoc = Vec::new();
            for _ in 0..r.count() {
                let d = t.def(r.u32())?;
                assoc.push((d, t.ty(r.u32())?));
            }
            let b = r.u32();
            let by = if b == u32::MAX { None } else { Some(t.sym(b)?) };
            let kind = *ImplKind::ALL.get(r.u8() as usize)?;
            ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                methods,
                assoc,
                by,
                kind,
            }
        }
        4 => {
            let owner = t.def(r.u32())?;
            let sig = get_sig(r, t)?;
            ItemData::Method {
                owner,
                sig,
                has_body: r.u8() != 0,
            }
        }
        5 => {
            let shared = get_fields(r, t)?;
            let mut variants = Vec::new();
            for _ in 0..r.count() {
                let name = t.sym(r.u32())?;
                let def = t.def(r.u32())?;
                variants.push(Variant {
                    name,
                    def,
                    fields: get_fields(r, t)?,
                });
            }
            ItemData::Enum { shared, variants }
        }
        6 => {
            let owner = t.def(r.u32())?;
            let bounds = get_tys(r, t)?;
            ItemData::AssocType {
                owner,
                bounds,
                default: get_opt_ty(r, t).ok()?,
            }
        }
        7 => ItemData::Alias(t.ty(r.u32())?),
        8 => ItemData::Newtype(t.ty(r.u32())?),
        _ => return None,
    };
    Some(Item {
        def,
        name,
        public,
        generics,
        intrinsic,
        targets,
        literal_fn,
        data,
    })
}

/// Encodes items and exports: entry-local tables, then the records. The
/// bytes depend only on content: items in their given (module, source)
/// order, exports sorted by (module, name).
pub fn encode_items(names: &Names<'_>, items: &[Item], exports: &[Export]) -> StageResult<Vec<u8>> {
    let mut t = TableWriter::new(names.pool, names.paths, names.syms);
    let mut w = Writer::default();
    w.len_of(items);
    for it in items {
        put_item(&mut w, &mut t, it)?;
    }
    let mut ex: Vec<&Export> = exports.iter().collect();
    ex.sort_by(|a, b| {
        (a.module.as_str(), names.text(a.name)).cmp(&(b.module.as_str(), names.text(b.name)))
    });
    w.len_of(&ex);
    for e in ex {
        w.str(&e.module);
        w.u32(t.sym(e.name));
        w.u32(t.def(e.def));
        w.u8(e.kind as u8);
    }
    let mut out = Writer::default();
    t.write(&mut out);
    out.bytes.extend_from_slice(&w.bytes);
    Ok(out.bytes)
}

/// Decodes items and exports into this run; `None` on a malformed blob (a miss).
#[must_use]
pub fn decode_items(names: &Names<'_>, bytes: &[u8]) -> Option<(Vec<Item>, Vec<Export>)> {
    let mut r = Reader::new(bytes);
    let t = Tables::read(&mut r, names.pool, names.paths, names.syms)?;
    let mut items = Vec::new();
    for _ in 0..r.count() {
        items.push(get_item(&mut r, &t)?);
    }
    let mut exports = Vec::new();
    for _ in 0..r.count() {
        let module = r.str().to_owned();
        let name = t.sym(r.u32())?;
        let def = t.def(r.u32())?;
        let kind = *HeadKind::ALL.get(r.u8() as usize)?;
        exports.push(Export {
            module,
            name,
            def,
            kind,
        });
    }
    r.ok().then_some((items, exports))
}

/// `deep_hash(F) = H("deep", api_hash(F), sorted [(path(G), deep_hash(G))
/// for G in mentions(F)])` (§4.11.3).
#[must_use]
pub fn deep_hash(api_hash: Hash128, mentions: &[(String, Hash128)]) -> Hash128 {
    let mut m: Vec<&(String, Hash128)> = mentions.iter().collect();
    m.sort();
    let mut h = StableHasher::new("deep");
    h.hash(api_hash);
    h.u32(u32::try_from(m.len()).expect("mentions"));
    for (p, d) in m {
        h.str(p);
        h.hash(*d);
    }
    h.finish()
}

/// What other folders may see: public items, every impl, and the members
/// of both.
#[must_use]
pub fn interface_items(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter(|i| i.public || matches!(i.data, ItemData::Impl { .. }))
        .cloned()
        .collect()
}

/// Every item a set of interface items names, in first-mention order:
/// the folders of these are the interface's `mentions` (§4.11.3).
#[must_use]
pub fn mentioned_defs(pool: &InternPool, items: &[Item], exports: &[Export]) -> Vec<DefId> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push = |d: DefId, out: &mut Vec<DefId>| {
        if d != DefId::NONE && seen.insert(d) {
            out.push(d);
        }
    };
    let mut tys = Vec::new();
    let gens = |gs: &[Generic], tys: &mut Vec<Ty>| {
        for g in gs {
            tys.extend(g.bounds.iter().copied());
            tys.extend(g.default);
        }
    };
    let sig = |s: &FnSig, tys: &mut Vec<Ty>| {
        gens(&s.generics, tys);
        tys.extend(s.params.iter().map(|p| p.1));
        tys.push(s.ret);
    };
    for it in items {
        gens(&it.generics, &mut tys);
        match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => sig(s, &mut tys),
            ItemData::Data(fs) => tys.extend(fs.iter().map(|f| f.ty)),
            ItemData::Enum { shared, variants } => {
                tys.extend(shared.iter().map(|f| f.ty));
                for v in variants {
                    tys.extend(v.fields.iter().map(|f| f.ty));
                }
            }
            ItemData::Trait(t) => tys.extend(t.supers.iter().copied()),
            ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                assoc,
                ..
            } => {
                push(*trait_, &mut out);
                tys.extend(pool.list_items(*trait_args));
                tys.push(*self_ty);
                tys.extend(assoc.iter().map(|a| a.1));
            }
            ItemData::AssocType {
                bounds, default, ..
            } => {
                tys.extend(bounds.iter().copied());
                tys.extend(*default);
            }
            ItemData::Alias(x) | ItemData::Newtype(x) => tys.push(*x),
        }
    }
    for e in exports {
        push(e.def, &mut out);
    }
    let mut stack = tys;
    let mut seen_ty = std::collections::HashSet::new();
    while let Some(t) = stack.pop() {
        if !seen_ty.insert(t) {
            continue;
        }
        match pool.get(t) {
            TyData::Adt { def, args } => {
                push(def, &mut out);
                stack.extend(pool.list_items(args));
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                push(def, &mut out);
                stack.extend(pool.list_items(args));
                stack.extend(bindings.iter().map(|b| b.1));
            }
            TyData::Tuple { elems, rest } => {
                stack.extend(pool.list_items(elems));
                stack.extend(rest);
            }
            TyData::Option(i) | TyData::Mut(i) => stack.push(i),
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                stack.extend(pool.list_items(params));
                stack.push(result);
                stack.extend(pool.row_data(row).keys);
            }
            TyData::Assoc {
                trait_,
                self_ty,
                args,
                ..
            } => {
                push(trait_, &mut out);
                stack.push(self_ty);
                stack.extend(pool.list_items(args));
            }
            _ => {}
        }
    }
    out
}

/// A folder interface over items (already filtered), exports and their
/// blob; `mentions` are the deep hashes of the other folders it names.
#[must_use]
pub fn folder_iface(
    folder: &str,
    items: Vec<Item>,
    exports: Vec<Export>,
    blob: Arc<[u8]>,
    mentions: &[(String, Hash128)],
) -> FolderIface {
    let by_def = items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.def, i))
        .collect();
    let export_index = exports
        .iter()
        .enumerate()
        .map(|(i, e)| ((e.module.clone(), e.name), i))
        .collect();
    let api_hash = hd_base::hash128(&blob);
    let deep_hash = deep_hash(api_hash, mentions);
    FolderIface {
        folder: folder.to_owned(),
        items,
        by_def,
        exports,
        export_index,
        blob,
        api_hash,
        deep_hash,
        spans: HashMap::new(),
    }
}

/// The solver's impl table (trait-solver.md §3.3) over the impls of a
/// scope: rows grouped by trait, in stable path order within a trait.
/// Templates take no row; inherent blocks are not trait impls.
#[must_use]
pub fn impl_table(names: &Names<'_>, impls: &[&Item]) -> ImplTable {
    let by_def: HashMap<DefId, &Item> = impls.iter().map(|i| (i.def, *i)).collect();
    let mut rows: Vec<(DefId, DefId, Ty, TyList, ImplKind, usize, Hash128)> = impls
        .iter()
        .filter_map(|i| match i.data {
            ItemData::Impl {
                trait_,
                self_ty,
                trait_args,
                kind,
                ..
            } if trait_ != DefId::NONE && kind.is_impl() => Some((
                trait_,
                i.def,
                self_ty,
                trait_args,
                kind,
                i.generics.len(),
                names.path_hash(i.def),
            )),
            _ => None,
        })
        .collect();
    rows.sort_by_key(|r| (names.path_hash(r.0), r.6));
    let mut t = ImplTable::default();
    for (n, (trait_, def, self_ty, args, kind, n_params, rank)) in rows.into_iter().enumerate() {
        let n = u32::try_from(n).expect("impls");
        t.by_trait
            .entry(trait_.raw())
            .and_modify(|e| e.1 = n + 1)
            .or_insert((n, n + 1));
        t.trait_.push(trait_);
        t.def.push(def);
        t.head_key.push(if kind == ImplKind::TupleTemplate {
            HeadKey::TupleAny
        } else {
            HeadKey::of(names.pool, self_ty)
        });
        let arg_keys: Vec<HeadKey> = names
            .pool
            .list_items(args)
            .iter()
            .take(2)
            .map(|a| HeadKey::of(names.pool, *a))
            .collect();
        t.arg_key.push([
            arg_keys.first().copied().unwrap_or(HeadKey::Any),
            arg_keys.get(1).copied().unwrap_or(HeadKey::Any),
        ]);
        t.n_params.push(u8::try_from(n_params).unwrap_or(u8::MAX));
        t.head_self.push(self_ty);
        t.head_args.push(args);
        let item = by_def.get(&def);
        let mut plan = Vec::new();
        for (gi, g) in item
            .map_or(&[][..], |i| i.generics.as_slice())
            .iter()
            .enumerate()
        {
            for b in &g.bounds {
                if let TyData::TraitValue { def: tr, args, .. } = names.pool.get(*b) {
                    plan.push(PlanStep::Bound {
                        param: u8::try_from(gi).unwrap_or(u8::MAX),
                        trait_: tr,
                        args,
                        mut_: false,
                    });
                }
            }
        }
        t.plan.push(plan);
        t.assoc.push(match item.map(|i| &i.data) {
            Some(ItemData::Impl { assoc, .. }) => assoc.clone(),
            _ => vec![],
        });
        t.origin.push(match kind {
            ImplKind::TupleTemplate => ImplOrigin::TupleTemplate,
            ImplKind::Delegated => ImplOrigin::Delegated { field: 0 },
            ImplKind::Derived | ImplKind::Derivation => ImplOrigin::Derived { template: trait_ },
            ImplKind::Written | ImplKind::Template => ImplOrigin::Written,
        });
        t.rank
            .push(u64::try_from(rank.0 & u128::from(u64::MAX)).expect("rank"));
    }
    t
}

/// A type as a user reads it: items by name, parameters by position.
#[must_use]
pub fn show_ty(names: &Names<'_>, t: Ty) -> String {
    let pool = names.pool;
    let seg = |d: DefId| names.paths.segment(PathId::from_raw(d.raw())).to_owned();
    let list = |l: TyList| {
        pool.list_items(l)
            .iter()
            .map(|x| show_ty(names, *x))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match pool.get(t) {
        TyData::Prim(p) => p.name().to_owned(),
        TyData::Never => "never".into(),
        TyData::Poison => "_".into(),
        TyData::Adt { def, args } | TyData::TraitValue { def, args, .. }
            if args == TyList::EMPTY =>
        {
            seg(def)
        }
        TyData::Adt { def, args } | TyData::TraitValue { def, args, .. } => {
            format!("{}[{}]", seg(def), list(args))
        }
        TyData::Tuple { elems, rest } => format!(
            "({}{})",
            list(elems),
            rest.map_or(String::new(), |r| format!(", {}...", show_ty(names, r)))
        ),
        TyData::Option(i) => format!("{}?", show_ty(names, i)),
        TyData::Mut(i) => format!("mut {}", show_ty(names, i)),
        TyData::Fn {
            params,
            result,
            suspends,
            ..
        } => format!(
            "fn{}({}) -> {}",
            if suspends { "!" } else { "" },
            list(params),
            show_ty(names, result)
        ),
        TyData::Param(p) => format!("{}#{}", seg(p.owner), p.index),
        TyData::Assoc { assoc, self_ty, .. } => {
            format!("{}::{}", show_ty(names, self_ty), seg(assoc))
        }
        TyData::Infer(_) | TyData::Canon(_) => "?".into(),
        TyData::Row(r) => {
            let d = pool.row_data(r);
            if d.keys.is_empty() && d.params.is_empty() {
                return "$()".into();
            }
            // Content order (scheduler.md §6.5), not interning order.
            let mut parts: Vec<String> = d.keys.iter().map(|k| show_ty(names, *k)).collect();
            parts.sort();
            let mut ps: Vec<String> = d
                .params
                .iter()
                .map(|p| format!("{}#{}", seg(p.owner), p.index))
                .collect();
            ps.sort();
            parts.extend(ps);
            format!("$ {}", parts.join(" + "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Export, FnSig, Generic, HeadKind, Item, ItemData, Names, decode_items, encode_items,
    };
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::{InternPool, ParamRef, TyData};

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
        let sig = FnSig::simple(
            vec![Generic::plain(syms.intern("T"))],
            vec![(syms.intern("a"), t)],
            t,
        );
        let items = vec![Item::new(f, syms.intern("first"), true, ItemData::Fn(sig))];
        let exports = vec![Export {
            module: "app.geo".into(),
            name: syms.intern("first"),
            def: f,
            kind: HeadKind::Fn,
        }];
        let blob = encode_items(&n, &items, &exports).expect("encode");
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
        let (back, ex) = decode_items(&n2, &blob).expect("decode");
        assert_eq!(back[0].def, n2.item("app.geo", "first"));
        assert_eq!(ex[0].def, n2.item("app.geo", "first"));
        assert_eq!(encode_items(&n2, &back, &ex).expect("again"), blob);
    }
}
