//! Interface records, folder interfaces and their blobs, and impl tables
//! (`crates/hd_resolve/DESIGN.md` §4.10 to §4.12; `docs/compiler/data-structures.md` §3.16;
//! `crates/hd_check/DESIGN.md` (trait solver) §3.3). Types are `hd_types` types in the run's pool; a
//! blob carries them through entry-local tables.

use hd_base::HashMap;
use hd_base::SliceSort as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::known::KnownItems;
use hd_base::wire::{Reader, Writer};
use hd_base::{AppendVec, DefId, FolderId, Hash128, PathId, StableHasher, StageResult, Symbol};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_types::solver::{
    Declarations, HeadKey, ImplOrigin, ImplTable, PlanStep, SealedTraits, TypeDecl,
};
use hd_types::wire::{TableWriter, Tables};
use hd_types::{InternPool, ParamRef, RowId, Ty, TyData, TyList};

/// The segment prefix of a local declaration's hidden scope; `#` is in no
/// identifier, so no field or parameter default (also `Hidden`) has it.
const LOCAL_SCOPE: &str = "local#";

/// [`Names::is_local`] over a path table.
fn is_local_path(paths: &PathTable, d: DefId) -> bool {
    let mut p = PathId::from_raw(d.raw());
    while p.get().is_some() {
        if paths.kind(p) == PathKind::Hidden && paths.segment(p).starts_with(LOCAL_SCOPE) {
            return true;
        }
        p = paths.parent(p);
    }
    false
}

/// The run-wide tables a resolver writes into.
#[derive(Clone, Copy)]
pub struct Names<'a> {
    pub pool: &'a InternPool,
    pub paths: &'a PathTable,
    pub syms: &'a ShardedInterner,
    /// The std items the compiler recognizes, resolved once for the run.
    pub known: &'a KnownItems,
}

impl Names<'_> {
    /// Most names already exist; only declaration or generated-name misses insert.
    fn path_node(&self, parent: PathId, kind: PathKind, segment: &str) -> PathId {
        self.paths
            .find(parent, kind, segment)
            .unwrap_or_else(|| self.paths.intern(parent, kind, segment))
    }

    /// The trie node of a dotted module path (`app.geo.shapes`): the first
    /// segment is the package.
    #[must_use]
    pub fn module(&self, module: &str) -> PathId {
        let mut segs = module.split('.');
        let package = segs.next().unwrap_or("");
        let mut p = self.path_node(PathId::NONE, PathKind::Package, package);
        for s in segs {
            p = self.path_node(p, PathKind::Module, s);
        }
        p
    }
    #[must_use]
    pub fn item(&self, module: &str, name: &str) -> DefId {
        DefId::from_raw(
            self.path_node(self.module(module), PathKind::Item, name)
                .raw(),
        )
    }
    #[must_use]
    pub fn member(&self, owner: DefId, kind: PathKind, name: &str) -> DefId {
        DefId::from_raw(
            self.path_node(PathId::from_raw(owner.raw()), kind, name)
                .raw(),
        )
    }
    /// The hidden scope of a block-local declaration: a child of the item
    /// whose body declares it, numbered by the declaration's place among
    /// that body's local declarations (`crates/hd_tir/DESIGN.md` "Local items
    /// lift to hidden module items"). The declaration's own path is a child
    /// of this scope, so its last segment is the name a user writes.
    #[must_use]
    pub fn local_scope(&self, owner: DefId, index: usize) -> DefId {
        self.member(owner, PathKind::Hidden, &format!("{LOCAL_SCOPE}{index}"))
    }
    /// Whether `d` is a block-local declaration or one of its members
    /// (`names.local-type.static`): some ancestor is a [`Self::local_scope`].
    #[must_use]
    pub fn is_local(&self, d: DefId) -> bool {
        is_local_path(self.paths, d)
    }
    /// The dotted path of the module that declares `d` (`std.format`).
    #[must_use]
    pub fn module_of(&self, d: DefId) -> String {
        self.module_text(self.module_node(d))
    }
    /// Dotted text at a display or serialization boundary. Hot lookups keep IDs.
    #[must_use]
    pub fn module_text(&self, mut p: PathId) -> String {
        let mut segs = Vec::new();
        while p.get().is_some() {
            segs.push(self.paths.segment(p).to_owned());
            p = self.paths.parent(p);
        }
        segs.reverse();
        segs.join(".")
    }
    /// Whether `d` is declared directly in `module` (its nearest module or
    /// package ancestor is `module`), without rendering any path.
    #[must_use]
    pub fn declared_in(&self, d: DefId, module: PathId) -> bool {
        self.module_node(d) == module
    }
    /// The trie node of the module that declares `d`: its nearest module
    /// or package ancestor.
    #[must_use]
    pub fn module_node(&self, d: DefId) -> PathId {
        self.paths.def_module(PathId::from_raw(d.raw()))
    }
    /// The package node declaring an item, without rendering its module.
    #[must_use]
    pub fn package_node(&self, d: DefId) -> PathId {
        let mut p = self.module_node(d);
        while p.get().is_some() && self.paths.kind(p) != PathKind::Package {
            p = self.paths.parent(p);
        }
        p
    }
    /// The stable path of an item, for messages and relocations.
    #[must_use]
    pub fn path(&self, d: DefId) -> String {
        self.paths.display(PathId::from_raw(d.raw())).to_string()
    }
    /// An item as its user writes it: its declared name (`Eq`). Every
    /// user-facing message and [`show_ty`] name items this way.
    #[must_use]
    pub fn display_name(&self, d: DefId) -> &str {
        self.paths.segment(PathId::from_raw(d.raw()))
    }
    /// Records the names an item's own type and row parameters are
    /// written with, for [`show_ty`]: a trait's `Self` is parameter 0 and
    /// its declared parameters follow.
    pub fn record_params(&self, item: &Item) {
        let owner = PathId::from_raw(item.def.raw());
        let (generics, offset) = match &item.data {
            ItemData::Fn(sig) | ItemData::Method { sig, .. } => (sig.generics.as_slice(), 0),
            ItemData::Trait(_) => {
                self.paths.set_param_name(owner, 0, "Self");
                (item.generics.as_slice(), 1)
            }
            _ => (item.generics.as_slice(), 0),
        };
        for (i, g) in generics.iter().enumerate() {
            let index = offset + u16::try_from(i).unwrap_or(u16::MAX);
            self.paths.set_param_name(owner, index, self.text(g.name));
        }
    }
    /// A declared parameter as its user writes it (`T`), or by position
    /// where no declaration was recorded.
    #[must_use]
    pub fn param_name(&self, owner: DefId, index: u16) -> String {
        self.paths
            .param_name(PathId::from_raw(owner.raw()), index)
            .unwrap_or_else(|| format!("{}#{index}", self.display_name(owner)))
    }
    fn qualified_name(&self, d: DefId) -> String {
        format!("{}.{}", self.module_of(d), self.display_name(d))
    }

    /// [`Self::display_name`] of each item of one message; items that would
    /// print the same name are qualified by their module (`std.cmp.Eq`).
    #[must_use]
    pub fn display_names(&self, ds: &[DefId]) -> Vec<String> {
        ds.iter()
            .map(|&d| {
                let name = self.display_name(d);
                let clash = ds.iter().any(|&o| o != d && self.display_name(o) == name);
                if clash {
                    self.qualified_name(d)
                } else {
                    name.to_owned()
                }
            })
            .collect()
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
    /// The declared variance marker: `+T` is 1, `-T` is -1, unmarked 0.
    pub variance: i8,
    /// A struct's lifetime parameter (`ref.lifetime.struct`): it follows
    /// the type parameters, and its argument is always [`lifetime_arg`].
    pub lifetime: bool,
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
            variance: 0,
            lifetime: false,
        }
    }
}

/// The argument of every struct lifetime parameter (`ref.lifetime.struct`):
/// one local reference slot, so a value of the struct holds a local
/// reference and its origins live in that slot. Lifetimes themselves are
/// not types: a signature's slot table names them (`FnSig::lifetimes`).
#[must_use]
pub fn lifetime_arg(pool: hd_types::Types<'_>) -> Ty {
    pool.intern_ty(&TyData::Ref {
        inner: Ty::VOID,
        mutable: false,
        counted: false,
    })
}

/// Whether `t` is [`lifetime_arg`].
fn is_lifetime_arg(pool: hd_types::Types<'_>, t: Ty) -> bool {
    matches!(
        pool.get(t),
        TyData::Ref {
            inner: Ty::VOID,
            mutable: false,
            counted: false
        }
    )
}

/// A trait reference's arguments with the trait's omitted trailing
/// parameters filled from their defaults, in declaration order, each with
/// the earlier arguments and `Self` substituted (`types.generic.default.fill`,
/// `types.generic.default.written`; `crates/hd_check/DESIGN.md` (trait solver) §2.1). `generics` are
/// the trait's declared parameters (parameter 0 is `Self`). `self_ty` is
/// `None` where no `Self` is known (a `dyn` type or a row key).
///
/// The arguments stay as written when a missing slot has no default, when
/// its default names `Self` and `self_ty` is `None`, or when its default
/// is poisoned: those are errors reported elsewhere.
#[must_use]
pub fn fill_trait_args(
    pool: &InternPool,
    tr: DefId,
    generics: &[Generic],
    args: TyList,
    self_ty: Option<Ty>,
) -> TyList {
    let n = pool.list_items(args).len();
    if n >= generics.len() {
        return args;
    }
    let mut v = pool.list_items(args).to_vec();
    for g in &generics[n..] {
        let Some(d) = g.default else {
            return args;
        };
        if pool.has_poison(d) {
            return args;
        }
        let needs_self = std::cell::Cell::new(false);
        let known = &v;
        let t = pool.subst(d, &|p: ParamRef| {
            if p.owner != tr {
                return None;
            }
            if p.index == 0 {
                needs_self.set(self_ty.is_none());
                self_ty
            } else {
                known.get(usize::from(p.index) - 1).copied()
            }
        });
        if needs_self.get() {
            return args;
        }
        v.push(t);
    }
    pool.list(&v)
}

/// A written type constructor's arguments with the omitted trailing
/// parameters filled from their defaults, in declaration order, each with
/// the earlier arguments substituted (`types.generic.default.written`,
/// `types.generic.default.bare`). `generics` are the declaration's
/// parameters, position for position with its arguments.
///
/// The result is shorter than `generics` where a slot has no default, or a
/// poisoned one (an error reported elsewhere): the caller keeps its own
/// handling of a missing argument.
#[must_use]
pub fn fill_type_args(pool: &InternPool, def: DefId, generics: &[Generic], args: &[Ty]) -> Vec<Ty> {
    let mut v = args.to_vec();
    for g in generics.iter().skip(args.len()) {
        let Some(d) = g.default else { break };
        if pool.has_poison(d) {
            break;
        }
        let t = pool.subst(d, &|p: ParamRef| {
            (p.owner == def)
                .then(|| v.get(usize::from(p.index)).copied())
                .flatten()
        });
        v.push(t);
    }
    v
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
    /// The written lifetimes, for checking them (#1011): one per local
    /// reference slot of the parameters, then of the result, in
    /// [`hd_types::Types::ref_slots`] order over the declared types. 0 is
    /// elided; `k` is the `k`-th lifetime in scope, an enclosing
    /// implementation's first. Empty when the signature writes none.
    pub lifetimes: Vec<u16>,
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
            lifetimes: Vec::new(),
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

/// What an impl declaration is (`crates/hd_check/DESIGN.md` (trait solver) §3.10, annotations §Templates).
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
    /// `Display`, `Error` or a `From` that `@error` generates for an error
    /// type (spec 14 `annot.error.generates`): an ordinary implementation
    /// whose method bodies the checker writes (`crates/hd_mono/DESIGN.md` §13.14).
    Error,
    /// An unresolved trait impl: its target survives for error recovery only.
    Poison,
}

impl ImplKind {
    const ALL: [ImplKind; 8] = [
        ImplKind::Written,
        ImplKind::Delegated,
        ImplKind::Template,
        ImplKind::TupleTemplate,
        ImplKind::Derivation,
        ImplKind::Derived,
        ImplKind::Error,
        ImplKind::Poison,
    ];
    /// Whether the head takes a coherence slot and an impl-table row.
    #[must_use]
    pub fn is_impl(self) -> bool {
        !matches!(self, ImplKind::Template | ImplKind::Poison)
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

/// Attached documentation, keyed by the relative declaration path.
pub type ItemDocs = Vec<(Vec<String>, String)>;

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
    /// A function's or method's `@inline` or `@cold` line, as one of
    /// `INLINE_AUTO`, `INLINE_ALWAYS`, `INLINE_NEVER` and `COLD`, or 0
    /// (`annot.inline.recognized`).
    pub inline: u8,
    /// A function whose decorators attach values: `facts_of` of it reads
    /// its facts body (`annot.facts-of.result.compiler-decorators`).
    pub has_facts: bool,
    /// Whether this callable has one checked body for each build profile.
    pub configured: bool,
    /// A typed fact type's pattern: the type argument of its
    /// `@annotate::[pattern]` line, over the type's own parameters
    /// (`annot.typed-fact.declare`, `annot.typed-fact.pattern`); `None`
    /// for an untyped fact type or any other item.
    pub fact_pattern: Option<Ty>,
    /// An implementation that carries `std.structure`'s `fills_defaults()`
    /// (`annot.impl-fact.fills-defaults`): as a derivation's source, its
    /// `build` fills each member that declares a default from it.
    pub fills_defaults: bool,
    /// The fact types an implementation names with `std.structure`'s
    /// `skips::[F]()` (`annot.impl-fact.skips`): as a derivation's walker,
    /// describer or source, it omits each member whose known facts hold a
    /// value of one of them.
    pub skips: Vec<Ty>,
    /// A `struct` declaration: `ItemData::Data` fields, a value type
    /// without identity (`data.struct.value`).
    pub is_struct: bool,
    /// Attached declaration docs, keyed by relative variant/member names;
    /// an empty path is the type itself. Stored outside semantic blobs.
    pub docs: ItemDocs,
    pub data: ItemData,
}

/// `Item::literal_fn` of a suffix function.
pub const LITERAL_SUFFIX: u8 = 1;
/// `Item::literal_fn` of a prefix function.
pub const LITERAL_PREFIX: u8 = 2;

/// `Item::inline` of `@inline` and `@inline(.Auto)`.
pub const INLINE_AUTO: u8 = 1;
/// `Item::inline` of `@inline(.Always)`.
pub const INLINE_ALWAYS: u8 = 2;
/// `Item::inline` of `@inline(.Never)`.
pub const INLINE_NEVER: u8 = 3;
/// `Item::inline` of `@cold`.
pub const COLD: u8 = 4;

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
            inline: 0,
            has_facts: false,
            configured: false,
            fact_pattern: None,
            fills_defaults: false,
            skips: Vec::new(),
            is_struct: false,
            docs: Vec::new(),
            data,
        }
    }
    /// What the solver's compiler-supplied rows read of a type
    /// declaration (`crates/hd_check/DESIGN.md` (trait solver) §3.9).
    #[must_use]
    pub fn type_decl(&self) -> TypeDecl {
        match &self.data {
            ItemData::Enum { .. } => TypeDecl::Enum,
            ItemData::Newtype(base) => TypeDecl::Newtype(*base),
            ItemData::Data(_) if self.is_struct => TypeDecl::Struct,
            _ => TypeDecl::Data,
        }
    }
    /// A trait's direct supertraits; none for any other item.
    #[must_use]
    pub fn supertraits(&self) -> &[Ty] {
        match &self.data {
            ItemData::Trait(t) => &t.supers,
            _ => &[],
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

/// An import hint from public std exports, including the modules that
/// contain them. Re-exports of one declaration count as one item, with
/// its declaring module preferred over a re-export's path.
#[must_use]
pub fn std_import_hint<'a>(
    names: &Names<'_>,
    exports: impl IntoIterator<Item = &'a Export>,
    name: &str,
) -> String {
    let mut items = HashMap::<DefId, (bool, String)>::default();
    let mut modules = std::collections::BTreeSet::new();
    for e in exports {
        let declared = names.module_of(e.def);
        let runtime = |module: &str| module == "std.rt" || module.starts_with("std.rt.");
        if !e.module.starts_with("std.") || runtime(&e.module) || runtime(&declared) {
            continue;
        }
        if e.module.rsplit('.').next() == Some(name) {
            modules.insert(e.module.clone());
        }
        if names.text(e.name) == name {
            let reexport = declared != e.module;
            let path = format!("{}.{name}", e.module);
            let found = items.entry(e.def).or_insert((reexport, path.clone()));
            if (reexport, &path) < (found.0, &found.1) {
                *found = (reexport, path);
            }
        }
    }
    let mut paths: Vec<String> = items.into_values().map(|(_, path)| path).collect();
    paths.extend(modules);
    paths.sort_stable();
    paths.dedup();
    match paths.as_slice() {
        [] => String::new(),
        [path] => format!("; add `use {path}`"),
        _ => {
            let modules: Vec<_> = paths
                .iter()
                .take(3)
                .map(|p| {
                    format!(
                        "`{}`",
                        p.rsplit_once('.').map_or(p.as_str(), |(module, _)| module)
                    )
                })
                .collect();
            format!(
                "; `{name}` has several public std homes: {}; add a `use` from the intended module",
                modules.join(", ")
            )
        }
    }
}

/// A run's dense interface-item directory. Each folder registers its index
/// once, after attaching metadata and before publishing its frozen interface.
/// Registration reads no item records. Rows may grow while other folders are
/// being checked; published rows are atomic and readers never take the writer
/// lock. Each lookup still restricts access to its own interface closure.
#[derive(Default)]
pub struct IfaceIndex {
    rows: AppendVec<AtomicU64>,
    writer: Mutex<()>,
}

impl IfaceIndex {
    const MISSING: u64 = u64::MAX;
    const AMBIGUOUS: u64 = u64::MAX - 1;

    /// Adds one frozen folder's `(DefId, item slot)` index. The driver calls
    /// this before publishing the interface's `Arc`, on cache hits and misses.
    /// `FolderId::NONE` is not a folder. Re-registering the same index is inert.
    pub fn insert(&self, folder: FolderId, iface: &FolderIface) {
        assert!(folder.get().is_some(), "an interface has a folder");
        let Some(last) = iface
            .by_def
            .keys()
            .filter_map(|d| d.get())
            .map(DefId::raw)
            .max()
        else {
            return;
        };
        let _writer = self.writer.lock().expect("interface index");
        while self.rows.len() <= last {
            self.rows.push(AtomicU64::new(Self::MISSING));
        }
        for (&def, &slot) in &iface.by_def {
            if def.get().is_none() {
                continue;
            }
            let value = (u64::from(folder.raw()) << 32)
                | u64::from(u32::try_from(slot).expect("interface item slot"));
            let row = self.rows.get(def.raw()).expect("interface index row");
            let old = row.load(Ordering::Relaxed);
            row.store(
                if old == Self::MISSING || old == value {
                    value
                } else {
                    // A malformed/recovery interface may repeat an identity
                    // in another folder. Keep the old ordered search for that
                    // identity alone, including fallback after a bad record.
                    Self::AMBIGUOUS
                },
                Ordering::Release,
            );
        }
    }

    fn location(&self, def: DefId) -> u64 {
        self.rows
            .get(def.raw())
            .map_or(Self::MISSING, |row| row.load(Ordering::Acquire))
    }
}

/// Finds items by `DefId` across one module's own items and the
/// interfaces of its closure.
pub struct Lookup<'a> {
    pub own: &'a [Item],
    pub own_index: HashMap<DefId, usize>,
    pub ifaces: Vec<&'a FolderIface>,
    index: &'a IfaceIndex,
    by_folder: Vec<Option<&'a FolderIface>>,
}

impl<'a> Lookup<'a> {
    #[must_use]
    pub fn new(
        own: &'a [Item],
        ifaces: impl IntoIterator<Item = (FolderId, &'a FolderIface)>,
        index: &'a IfaceIndex,
    ) -> Self {
        let own_index = own.iter().enumerate().map(|(i, it)| (it.def, i)).collect();
        let mut visible = Vec::new();
        let mut by_folder = Vec::new();
        for (folder, iface) in ifaces {
            if by_folder.len() <= folder.idx() {
                by_folder.resize(folder.idx() + 1, None);
            }
            by_folder[folder.idx()] = Some(iface);
            visible.push(iface);
        }
        Self {
            own,
            own_index,
            ifaces: visible,
            index,
            by_folder,
        }
    }
    #[must_use]
    pub fn item(&self, d: DefId) -> Option<&'a Item> {
        if let Some(&i) = self.own_index.get(&d) {
            return self.own.get(i);
        }
        match self.index.location(d) {
            IfaceIndex::AMBIGUOUS => self.ifaces.iter().find_map(|f| f.item(d)),
            IfaceIndex::MISSING if d == DefId::NONE => self.ifaces.iter().find_map(|f| f.item(d)),
            IfaceIndex::MISSING => None,
            location => {
                let folder = usize::try_from(location >> 32).expect("interface folder");
                let slot =
                    usize::try_from(location & u64::from(u32::MAX)).expect("interface item slot");
                self.by_folder.get(folder)?.as_ref()?.items.get(slot)
            }
        }
    }
    /// Failed trait declarations whose target still needs error recovery.
    #[must_use]
    pub fn poisoned_impls(&self) -> Vec<&'a Item> {
        let poisoned = |i: &&Item| {
            matches!(
                i.data,
                ItemData::Impl {
                    kind: ImplKind::Poison,
                    ..
                }
            )
        };
        let mut out: Vec<&Item> = self.own.iter().filter(poisoned).collect();
        for f in &self.ifaces {
            out.extend(f.items.impls().filter(poisoned));
        }
        out
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

/// The declarations the solver's compiler-supplied rows and trait-value
/// rows read (`crates/hd_check/DESIGN.md` (trait solver) §3.9, §9.3), over a context's lookup.
pub struct LookupDecls<'a> {
    pub lookup: &'a Lookup<'a>,
    pub pool: &'a InternPool,
    pub sealed: SealedTraits,
    pub paths: &'a PathTable,
    pub syms: &'a ShardedInterner,
}

/// Whether the trait `trait_` declares an associated function, a member
/// with no `self` receiver, which cannot work through a `dyn` value
/// (`trait.dyn.member.assoc-function`). `item` reads a declaration.
#[must_use]
pub fn declares_assoc_function<'i>(
    syms: &ShardedInterner,
    item: impl Fn(DefId) -> Option<&'i Item>,
    trait_: DefId,
) -> bool {
    let Some(ItemData::Trait(t)) = item(trait_).map(|i| &i.data) else {
        return false;
    };
    t.methods.iter().any(|(_, m)| {
        matches!(
            item(*m).map(|i| &i.data),
            Some(ItemData::Method { sig, .. })
                if sig.params.first().is_none_or(|p| syms.resolve(p.0) != "self")
        )
    })
}

/// Whether the trait `trait_` declares a method with a `&self` or
/// `&mut self` receiver, which a bare trait value does not provide
/// (`trait.dyn.ref.member`). `item` reads a declaration.
#[must_use]
pub fn declares_ref_receiver<'i>(
    syms: &ShardedInterner,
    item: impl Fn(DefId) -> Option<&'i Item>,
    pool: &InternPool,
    trait_: DefId,
) -> bool {
    let Some(ItemData::Trait(t)) = item(trait_).map(|i| &i.data) else {
        return false;
    };
    t.methods.iter().any(|(_, m)| {
        matches!(
            item(*m).map(|i| &i.data),
            Some(ItemData::Method { sig, .. })
                if sig.params.first().is_some_and(|p| {
                    syms.resolve(p.0) == "self" && matches!(pool.get(p.1), TyData::Ref { .. })
                })
        )
    })
}

impl Declarations for LookupDecls<'_> {
    fn sealed(&self) -> &SealedTraits {
        &self.sealed
    }
    fn type_decl(&self, def: DefId) -> TypeDecl {
        self.lookup
            .item(def)
            .map_or(TypeDecl::Data, Item::type_decl)
    }
    fn supertraits(&self, trait_: DefId) -> &[Ty] {
        self.lookup.item(trait_).map_or(&[], Item::supertraits)
    }
    fn has_unavailable_member(&self, trait_: DefId) -> bool {
        declares_assoc_function(self.syms, |d| self.lookup.item(d), trait_)
            || declares_ref_receiver(self.syms, |d| self.lookup.item(d), self.pool, trait_)
    }
    fn has_unavailable_reference_member(&self, trait_: DefId, mutable: bool) -> bool {
        let Some(ItemData::Trait(t)) = self.lookup.item(trait_).map(|i| &i.data) else {
            return false;
        };
        t.methods.iter().any(|(_, method)| {
            self.lookup.item(*method).and_then(Item::sig).is_some_and(|sig| {
                self.pool.has_opaque(sig.ret) || sig.params.first().is_none_or(|&(name, ty)| {
                    self.syms.resolve(name) != "self"
                        || matches!(self.pool.get(ty), TyData::Ref { mutable: true, .. } if !mutable)
                })
            })
        })
    }
    fn is_local(&self, def: DefId) -> bool {
        is_local_path(self.paths, def)
    }
}

// ------------------------------------------------------------- the blob

/// A folder's interface (§4.10): its exported items, every impl and their
/// members, the export index with chains followed, the blob they were read
/// from or written to, and the deep hash.
#[derive(Debug)]
pub struct FolderIface {
    pub folder: String,
    pub items: Items,
    pub by_def: HashMap<DefId, usize>,
    pub exports: Vec<Export>,
    pub export_index: HashMap<(String, Symbol), usize>,
    pub blob: Arc<[u8]>,
    /// The hash of what a dependent's check can read: the blob without
    /// the impls of private types (`private_impl_items`).
    pub api_hash: Hash128,
    /// The hash of the whole blob, every impl head included: what
    /// coherence and the folder's own header check read.
    pub heads_hash: Hash128,
    pub deep_hash: Hash128,
    /// Where each item and written type sits: `(item, slot)` to (module
    /// index in the folder, anchor). Outside every hash; diagnostics only.
    pub spans: HashMap<(DefId, u32), (u32, crate::anchor::Anchor)>,
    /// The private-name index (`crates/hd_resolve/DESIGN.md` §4.9): each
    /// top-level declaration without `pub`, as (module path, name). A
    /// failed export lookup that finds a row is `private-import`. Outside
    /// every hash; diagnostics only.
    pub private_names: hd_base::HashSet<(String, Symbol)>,
    /// Names no use can reach through this folder: each public name whose
    /// `pub use` chain loops (`module.pub-use.chain.loop`), as (module
    /// path, name). A failed export lookup that finds a row is
    /// `re-export-loop`, reported where the loop was built, quietly
    /// through other folders. Outside every hash; diagnostics only.
    pub loop_names: hd_base::HashSet<(String, Symbol)>,
    /// The hash of `private_names` and `loop_names` as their entry
    /// sections encode them: what a failed lookup reads besides the
    /// exports, so a key over the reads of a folder's header lowering
    /// takes it with the deep hash (#248). Zero until the driver sets it.
    pub names_hash: Hash128,
    /// The modules a REPL input's `use`s name, as (input module path,
    /// local name, module path): what the input leaves in the session
    /// besides its exports (`cli.repl.uses.other`). Empty for any other
    /// folder. Part of the API hash when not empty, since later inputs
    /// resolve names through it.
    pub module_uses: Vec<(String, String, String)>,
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

    /// Whether `module` declares `name` without `pub`.
    #[must_use]
    pub fn is_private(&self, module: &str, name: Symbol) -> bool {
        self.private_names.contains(&(module.to_owned(), name))
    }
}

/// The private-name index section of an interface entry: (module path,
/// name) rows, sorted.
#[must_use]
pub fn encode_private_names(names: &Names<'_>, rows: &[(String, Symbol)]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(rows);
    for (module, name) in rows {
        w.str(module);
        w.str(names.text(*name));
    }
    w.bytes
}

/// The module-use section of an interface entry: (input module path,
/// local name, module path) rows.
#[must_use]
pub fn encode_module_uses(rows: &[(String, String, String)]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(rows);
    for (module, local, target) in rows {
        w.str(module);
        w.str(local);
        w.str(target);
    }
    w.bytes
}

/// Reads [`encode_module_uses`].
#[must_use]
pub fn decode_module_uses(b: &[u8]) -> Option<Vec<(String, String, String)>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let module = r.str().to_owned();
        let local = r.str().to_owned();
        let target = r.str().to_owned();
        out.push((module, local, target));
    }
    r.ok().then_some(out)
}

/// Reads [`encode_private_names`].
#[must_use]
pub fn decode_private_names(names: &Names<'_>, b: &[u8]) -> Option<Vec<(String, Symbol)>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let module = r.str().to_owned();
        let name = names.syms.intern(r.str());
        out.push((module, name));
    }
    r.ok().then_some(out)
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
        w.u8(u8::from(g.row) | variance | (u8::from(g.lifetime) << 4));
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
            variance: match flags & 0b1100 {
                0b0100 => 1,
                0b1000 => -1,
                _ => 0,
            },
            lifetime: flags & 0b1_0000 != 0,
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
    w.len_of(&s.lifetimes);
    for l in &s.lifetimes {
        w.u32(u32::from(*l));
    }
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
    let mut lifetimes = Vec::new();
    for _ in 0..r.count() {
        lifetimes.push(u16::try_from(r.u32()).ok()?);
    }
    Some(FnSig {
        generics,
        params,
        defaults,
        ret,
        row,
        suspends: flags & 1 != 0,
        variadic: flags & 2 != 0,
        lifetimes,
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
    w.u8(it.inline);
    w.u8(u8::from(it.has_facts));
    w.u8(u8::from(it.configured));
    w.u32(opt_ty(t, it.fact_pattern)?);
    w.u8(u8::from(it.fills_defaults));
    put_tys(w, t, &it.skips)?;
    w.u8(u8::from(it.is_struct));
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
    let inline = r.u8();
    let has_facts = r.u8() != 0;
    let configured = r.u8() != 0;
    let fact_pattern = get_opt_ty(r, t).ok()?;
    let fills_defaults = r.u8() != 0;
    let skips = get_tys(r, t)?;
    let is_struct = r.u8() != 0;
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
        inline,
        has_facts,
        configured,
        fact_pattern,
        fills_defaults,
        skips,
        is_struct,
        docs: Vec::new(),
        data,
    })
}

/// Documentation is an auxiliary entry section: never part of API, head,
/// coherence or deep hashes, even when its text changes generated bodies.
#[must_use]
pub fn encode_docs(items: &[Item]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(items);
    for item in items {
        w.len_of(&item.docs);
        for (path, text) in &item.docs {
            w.len_of(path);
            for segment in path {
                w.str(segment);
            }
            w.str(text);
        }
    }
    w.bytes
}

/// Encodes items and exports: entry-local tables, then the records. The
/// bytes depend only on content: items in their given (module, source)
/// order, exports sorted by (module, name).
pub fn encode_items(names: &Names<'_>, items: &[Item], exports: &[Export]) -> StageResult<Vec<u8>> {
    let mut t = TableWriter::new(names.pool, names.paths, names.syms);
    // The records, each at an offset the index names, so one decodes alone.
    let mut body = Writer::default();
    let mut index = Writer::default();
    index.len_of(items);
    for it in items {
        index.u32(t.def(it.def));
        index.u32(u32::try_from(body.bytes.len()).expect("blob"));
        index.u8(impl_tag(it));
        let (own, params) = param_names(it);
        index.u8(u8::from(own));
        index.len_of(params);
        for g in params {
            index.u32(t.sym(g.name));
        }
        put_item(&mut body, &mut t, it)?;
    }
    let mut w = Writer::default();
    w.bytes.extend_from_slice(&index.bytes);
    w.blob(&body.bytes);
    let mut ex: Vec<&Export> = exports.iter().collect();
    ex.sort_stable_by(|a, b| {
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

/// An item's index tag: its impl kind's position in `ImplKind::ALL`, with
/// `INHERENT` set for an inherent block; `TRAIT` for a trait; else
/// `NOT_IMPL`.
fn impl_tag(item: &Item) -> u8 {
    match &item.data {
        ItemData::Impl { kind, trait_, .. } => {
            let at = ImplKind::ALL.iter().position(|k| k == kind).unwrap_or(0);
            u8::try_from(at).unwrap_or(0) | if *trait_ == DefId::NONE { INHERENT } else { 0 }
        }
        ItemData::Trait(_) => TRAIT,
        _ => NOT_IMPL,
    }
}

const NOT_IMPL: u8 = 0xff;
const TRAIT: u8 = 0xfe;
const INHERENT: u8 = 0x80;

/// The generic parameters `record_params` names for an item, and whether
/// its own `Self` comes first (a trait's).
fn param_names(item: &Item) -> (bool, &[Generic]) {
    match &item.data {
        ItemData::Fn(sig) | ItemData::Method { sig, .. } => (false, &sig.generics),
        ItemData::Trait(_) => (true, &item.generics),
        _ => (false, &item.generics),
    }
}

/// Decodes items and exports into this run; `None` on a malformed blob (a miss).
#[must_use]
pub fn decode_items(names: &Names<'_>, bytes: &[u8]) -> Option<(Vec<Item>, Vec<Export>)> {
    let (items, exports) = decode_lazy(names, Arc::from(bytes))?;
    let all = (0..items.len())
        .map(|i| items.get(i).cloned())
        .collect::<Option<Vec<Item>>>()?;
    Some((all, exports))
}

/// Reads a blob's tables, item index and exports into this run, and
/// leaves each item record to decode on first use (`Items`,
/// docs/compiler/data-structures.md §3.9.1). The items' parameter names
/// are recorded now, as an eager decode records them.
#[must_use]
pub fn decode_lazy(names: &Names<'_>, bytes: Arc<[u8]>) -> Option<(Items, Vec<Export>)> {
    let mut r = Reader::new(&bytes);
    let t = Tables::read(&mut r, names.pool, names.paths, names.syms)?;
    let n = r.count();
    let (mut defs, mut offsets, mut tags) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    for _ in 0..n {
        let def = t.def(r.u32())?;
        offsets.push(r.u32());
        tags.push(r.u8());
        let own = r.u8() != 0;
        let owner = PathId::from_raw(def.raw());
        if own {
            names.paths.set_param_name(owner, 0, "Self");
        }
        for i in 0..r.count() {
            let name = t.sym(r.u32())?;
            let index = u16::from(own) + u16::try_from(i).unwrap_or(u16::MAX);
            names.paths.set_param_name(owner, index, names.text(name));
        }
        defs.push(def);
    }
    let body = r.blob();
    let base = r.pos() - body.len();
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
    if !r.ok() {
        return None;
    }
    let source = Some(Source {
        bytes,
        tables: t,
        base,
        offsets,
        docs: Vec::new(),
    });
    Some((Items::with_source(defs, tags, source), exports))
}

/// Where a lazily decoded item's records are: the blob, its tables in
/// this run, and each record's offset in the item region at `base`.
#[derive(Debug)]
struct Source {
    bytes: Arc<[u8]>,
    tables: Tables,
    base: usize,
    offsets: Vec<u32>,
    docs: Vec<ItemDocs>,
}

/// A folder interface's items, by position: each decoded from the blob on
/// first use (docs/compiler/data-structures.md §3.9.1), or given whole when the
/// interface was built in this run. `decoded` counts the records read.
#[derive(Debug, Default)]
pub struct Items {
    defs: Vec<DefId>,
    /// Each item's `impl_tag`, so an impl is found without its record.
    tags: Vec<u8>,
    slots: Vec<std::sync::OnceLock<Option<Item>>>,
    source: Option<Source>,
    decoded: std::sync::atomic::AtomicUsize,
}

impl Items {
    /// Items built in this run.
    #[must_use]
    pub fn new(items: Vec<Item>) -> Self {
        let defs = items.iter().map(|i| i.def).collect();
        let tags = items.iter().map(impl_tag).collect();
        let slots = items
            .into_iter()
            .map(|i| std::sync::OnceLock::from(Some(i)))
            .collect();
        Self {
            defs,
            tags,
            slots,
            source: None,
            decoded: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn with_source(defs: Vec<DefId>, tags: Vec<u8>, source: Option<Source>) -> Self {
        let slots = defs.iter().map(|_| std::sync::OnceLock::new()).collect();
        Self {
            defs,
            tags,
            slots,
            source,
            decoded: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The items' definitions, in order, without decoding a record.
    #[must_use]
    pub fn defs(&self) -> &[DefId] {
        &self.defs
    }

    /// Item `i`, decoded on first use; `None` past the end or for a
    /// malformed record.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<&Item> {
        self.slots
            .get(i)?
            .get_or_init(|| {
                let src = self.source.as_ref()?;
                let at = src.base + *src.offsets.get(i)? as usize;
                self.decoded
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut item = get_item(&mut Reader::new(src.bytes.get(at..)?), &src.tables)?;
                item.docs = src.docs.get(i).cloned().unwrap_or_default();
                Some(item)
            })
            .as_ref()
    }

    /// Attaches the auxiliary doc section without changing semantic hashes.
    /// This must run before any lazy item is read.
    pub fn attach_docs(&mut self, bytes: &[u8]) -> Option<()> {
        let mut r = Reader::new(bytes);
        let count = r.count();
        if count != self.len() {
            return None;
        }
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let mut docs = Vec::new();
            for _ in 0..r.count() {
                let path = (0..r.count()).map(|_| r.str().to_owned()).collect();
                docs.push((path, r.str().to_owned()));
            }
            rows.push(docs);
        }
        if !r.ok() || !r.at_end() {
            return None;
        }
        for (slot, docs) in self.slots.iter_mut().zip(&rows) {
            if let Some(Some(item)) = slot.get_mut() {
                item.docs.clone_from(docs);
            }
        }
        if let Some(src) = &mut self.source {
            src.docs = rows;
        }
        Some(())
    }

    /// Item `i`'s impl kind, and whether it is an inherent block, read
    /// from the index: `None` for an item that is not an impl.
    #[must_use]
    pub fn impl_kind(&self, i: usize) -> Option<(ImplKind, bool)> {
        let tag = *self.tags.get(i)?;
        if tag == NOT_IMPL || tag == TRAIT {
            return None;
        }
        let kind = *ImplKind::ALL.get(usize::from(tag & !INHERENT))?;
        Some((kind, tag & INHERENT != 0))
    }

    /// The impls, in order, decoding only their records.
    pub fn impls(&self) -> impl Iterator<Item = &Item> {
        (0..self.len())
            .filter(|&i| self.impl_kind(i).is_some())
            .filter_map(|i| self.get(i))
    }

    /// The traits, in order, decoding only their records.
    pub fn traits(&self) -> impl Iterator<Item = &Item> {
        (0..self.len())
            .filter(|&i| self.tags.get(i) == Some(&TRAIT))
            .filter_map(|i| self.get(i))
    }

    /// How many records of items other than impls and traits were
    /// decoded: the ones a run asked for by definition.
    #[must_use]
    pub fn decoded_other(&self) -> usize {
        if self.source.is_none() {
            return 0;
        }
        (0..self.len())
            .filter(|&i| self.tags.get(i) == Some(&NOT_IMPL))
            .filter(|&i| self.slots.get(i).is_some_and(|s| s.get().is_some()))
            .count()
    }

    /// Every item, in order, each decoded on first use.
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        (0..self.len()).filter_map(|i| self.get(i))
    }

    /// How many records were decoded from the blob.
    #[must_use]
    pub fn decoded(&self) -> usize {
        self.decoded.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = &'a Item;
    type IntoIter = Box<dyn Iterator<Item = &'a Item> + 'a>;
    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// `deep_hash(F) = H("deep", api_hash(F), sorted [(path(G), deep_hash(G))
/// for G in mentions(F)])` (§4.11.3).
#[must_use]
pub fn deep_hash(api_hash: Hash128, mentions: &[(String, Hash128)]) -> Hash128 {
    let mut m: Vec<&(String, Hash128)> = mentions.iter().collect();
    m.sort_stable();
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

/// The impls among `all` (a folder's items) whose target or trait is a
/// private declaration of the folder, and their members: only
/// coherence and the folder's own checks read them, so they stay out of
/// `api_hash` (`crates/hd_resolve/DESIGN.md` §4.10, "Two hashes, two
/// readers"). A target is private when its head is a private type; a
/// type in an argument does not count, which keeps more in the hash.
#[must_use]
pub fn private_impl_items(pool: &InternPool, all: &[Item]) -> hd_base::HashSet<DefId> {
    let private: hd_base::HashSet<DefId> = all
        .iter()
        .filter(|i| {
            !i.public
                && !matches!(
                    i.data,
                    ItemData::Impl { .. } | ItemData::Method { .. } | ItemData::AssocType { .. }
                )
        })
        .map(|i| i.def)
        .collect();
    let mut out = hd_base::HashSet::default();
    for i in all {
        if let ItemData::Impl {
            trait_, self_ty, ..
        } = &i.data
        {
            let head = match pool.get(*self_ty) {
                TyData::Adt { def, .. } => Some(def),
                _ => None,
            };
            if private.contains(trait_) || head.is_some_and(|d| private.contains(&d)) {
                out.insert(i.def);
            }
        }
    }
    for i in all {
        if let ItemData::Method { owner, .. } | ItemData::AssocType { owner, .. } = &i.data
            && out.contains(owner)
        {
            out.insert(i.def);
        }
    }
    out
}

/// Every item a set of interface items names, in first-mention order:
/// the folders of these are the interface's `mentions` (§4.11.3).
#[must_use]
pub fn mentioned_defs<'a>(
    pool: &InternPool,
    items: impl IntoIterator<Item = &'a Item>,
    exports: &[Export],
) -> Vec<DefId> {
    let mut out = Vec::new();
    let mut seen = hd_base::HashSet::default();
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
        tys.extend(pool.row_data(s.row).keys);
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
    let mut seen_ty = hd_base::HashSet::default();
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
            TyData::Opaque { args, bound, .. } => {
                stack.extend(pool.list_items(args));
                stack.push(bound);
            }
            TyData::Tuple { elems, rest } => {
                stack.extend(pool.list_items(elems));
                stack.extend(rest);
            }
            TyData::Option(i) | TyData::Mut(i) | TyData::Ref { inner: i, .. } => stack.push(i),
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
            TyData::Row(row) | TyData::Context(row) => stack.extend(pool.row_data(row).keys),
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
    items: Items,
    exports: Vec<Export>,
    blob: Arc<[u8]>,
    api_blob: Option<Hash128>,
    mentions: &[(String, Hash128)],
    module_uses: Vec<(String, String, String)>,
) -> FolderIface {
    let by_def = items
        .defs()
        .iter()
        .enumerate()
        .map(|(i, d)| (*d, i))
        .collect();
    let export_index = exports
        .iter()
        .enumerate()
        .map(|(i, e)| ((e.module.clone(), e.name), i))
        .collect();
    // `heads_hash` covers the whole blob; `api_hash` covers it without
    // the impls of private types (`api_blob`, the hash of that encoding,
    // when there are any).
    let with_uses = |h: Hash128| {
        if module_uses.is_empty() {
            h
        } else {
            let mut k = StableHasher::new("iface-uses");
            k.hash(h);
            k.bytes(&encode_module_uses(&module_uses));
            k.finish()
        }
    };
    let heads_hash = with_uses(hd_base::hash128(&blob));
    let api_hash = api_blob.map_or(heads_hash, with_uses);
    let deep_hash = deep_hash(api_hash, mentions);
    FolderIface {
        folder: folder.to_owned(),
        items,
        by_def,
        exports,
        export_index,
        blob,
        api_hash,
        heads_hash,
        deep_hash,
        spans: HashMap::default(),
        private_names: hd_base::HashSet::default(),
        loop_names: hd_base::HashSet::default(),
        names_hash: Hash128::default(),
        module_uses,
    }
}

/// The solver's impl table (`crates/hd_check/DESIGN.md` (trait solver) §3.3) over the impls of a
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
    rows.sort_stable_by_key(|r| (names.path_hash(r.0), r.6));
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
            HeadKey::of(names.pool.types(), self_ty)
        });
        let arg_keys: Vec<HeadKey> = names
            .pool
            .list_items(args)
            .iter()
            .take(2)
            .map(|a| HeadKey::of(names.pool.types(), *a))
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
                if let TyData::TraitValue {
                    def: tr,
                    args,
                    bindings,
                } = names.pool.get(*b)
                {
                    let param = u8::try_from(gi).unwrap_or(u8::MAX);
                    plan.push(PlanStep::Bound {
                        param,
                        trait_: tr,
                        args,
                        mut_: false,
                    });
                    // A binding to a bare impl parameter fixes it.
                    for (assoc, bt) in bindings {
                        if let TyData::Param(p) = names.pool.get(bt)
                            && p.owner == def
                        {
                            plan.push(PlanStep::Bind {
                                param,
                                trait_: tr,
                                args,
                                assoc,
                                target: u8::try_from(p.index).unwrap_or(u8::MAX),
                            });
                        }
                    }
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
            ImplKind::Written | ImplKind::Template | ImplKind::Error | ImplKind::Poison => {
                ImplOrigin::Written
            }
        });
        t.rank
            .push(u64::try_from(rank.0 & u128::from(u64::MAX)).expect("rank"));
    }
    t.index();
    t
}

/// A type as a user reads it: items by name, parameters by position.
#[must_use]
pub fn show_ty(names: &Names<'_>, t: Ty) -> String {
    show_ty_in(names, names.pool.types(), t)
}

/// [`show_ty`] for a type that may live in a body's local pool.
#[must_use]
pub fn show_ty_in(names: &Names<'_>, pool: hd_types::Types<'_>, t: Ty) -> String {
    show_ty_opts(names, pool, t, false, false)
}

/// [`show_ty_in`] with the requirement row of every function type written
/// after its result, as `fn() -> string $ Db`. A mismatch between types
/// that differ only in rows reads with these.
#[must_use]
pub fn show_ty_rows_in(names: &Names<'_>, pool: hd_types::Types<'_>, t: Ty) -> String {
    show_ty_opts(names, pool, t, true, false)
}

/// Item paths for a diagnostic pair whose short rendered names clash.
#[must_use]
pub fn show_ty_qualified_in(
    names: &Names<'_>,
    pool: hd_types::Types<'_>,
    t: Ty,
    rows: bool,
) -> String {
    show_ty_opts(names, pool, t, rows, true)
}

fn show_ty_opts(
    names: &Names<'_>,
    pool: hd_types::Types<'_>,
    t: Ty,
    rows: bool,
    qualified: bool,
) -> String {
    show_marked(names, pool, t, (rows, qualified), None)
}

/// The lifetime marks of a function type's reference slots, inputs then
/// result, for a message (`fn.type.lifetime.identity`): `None` when
/// elision gives its map, else a name for each slot that shares one.
fn lifetime_marks(pool: hd_types::Types<'_>, t: Ty) -> Option<Marks> {
    let (ins, outs) = pool.fn_ty_deps(t)?;
    let TyData::Fn {
        params,
        result,
        deps,
        ..
    } = pool.get(t)
    else {
        return None;
    };
    if pool.fn_deps_written(pool.list_items(params), result, &[]) == deps {
        return None;
    }
    let all: Vec<u32> = ins.iter().chain(outs).copied().collect();
    let name = |l: u32| {
        (l != 0 && all.iter().filter(|&&x| x == l).count() > 1)
            .then(|| char::from(b'a' + u8::try_from((l - 1) % 26).unwrap_or(0)))
    };
    Some(std::cell::RefCell::new(
        all.iter().map(|&l| name(l)).collect(),
    ))
}

/// Each reference slot's mark, in [`hd_types::Types::ref_slots`] order.
type Marks = std::cell::RefCell<std::collections::VecDeque<Option<char>>>;

/// [`show_ty_opts`], writing the next of `marks` after each local `&`.
fn show_marked(
    names: &Names<'_>,
    pool: hd_types::Types<'_>,
    t: Ty,
    (rows, qualified): (bool, bool),
    marks: Option<&Marks>,
) -> String {
    let seg = |d: DefId| {
        if qualified {
            names.qualified_name(d)
        } else {
            names.display_name(d).to_owned()
        }
    };
    let list = |l: TyList| {
        pool.list_items(l)
            .iter()
            .map(|x| show_marked(names, pool, *x, (rows, qualified), marks))
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
        // A struct's lifetime arguments are not shown (`ref.lifetime.struct`).
        TyData::Adt { def, args }
            if pool
                .list_items(args)
                .iter()
                .any(|&x| is_lifetime_arg(pool, x)) =>
        {
            let shown: Vec<String> = pool
                .list_items(args)
                .iter()
                .filter(|&&x| !is_lifetime_arg(pool, x))
                .map(|x| show_marked(names, pool, *x, (rows, qualified), marks))
                .collect();
            if shown.is_empty() {
                seg(def)
            } else {
                format!("{}[{}]", seg(def), shown.join(", "))
            }
        }
        TyData::Adt { def, args } | TyData::TraitValue { def, args, .. } => {
            format!("{}[{}]", seg(def), list(args))
        }
        TyData::Tuple { elems, rest } => format!(
            "({}{})",
            list(elems),
            rest.map_or(String::new(), |r| format!(
                ", {}...",
                show_marked(names, pool, r, (rows, qualified), marks)
            ))
        ),
        TyData::Option(i) => format!("{}?", show_marked(names, pool, i, (rows, qualified), marks)),
        TyData::Mut(i) => format!(
            "mut {}",
            show_marked(names, pool, i, (rows, qualified), marks)
        ),
        TyData::Ref {
            inner,
            mutable,
            counted,
        } => {
            let dyn_prefix = if matches!(pool.get(inner), TyData::TraitValue { .. }) {
                "dyn "
            } else {
                ""
            };
            let mark = marks
                .filter(|_| !counted)
                .and_then(|m| m.borrow_mut().pop_front())
                .flatten()
                .map_or(String::new(), |c| format!("'{c} "));
            format!(
                "{}{mark}{}{dyn_prefix}{}",
                if counted { "*" } else { "&" },
                if mutable { "mut " } else { "" },
                show_marked(names, pool, inner, (rows, qualified), marks)
            )
        }
        // Inputs of an arity not known here have no sugar form.
        TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs: hd_types::Inputs::Tuple,
            ..
        } => format!(
            "{}[{}, {}, {}]",
            if suspends { "SuspendFn" } else { "Fn" },
            pool.list_items(params)
                .iter()
                .map(|x| show_ty_opts(names, pool, *x, rows, qualified))
                .collect::<Vec<_>>()
                .join(", "),
            show_ty_opts(names, pool, result, rows, qualified),
            show_row_in(names, pool, row, qualified)
        ),
        TyData::Fn {
            params,
            result,
            row,
            suspends,
            ..
        } => {
            let shown = if rows && row != hd_types::RowId::EMPTY {
                format!(" {}", show_row_in(names, pool, row, qualified))
            } else {
                String::new()
            };
            let own = lifetime_marks(pool, t);
            let own = own.as_ref();
            let params: Vec<String> = pool
                .list_items(params)
                .iter()
                .map(|x| show_marked(names, pool, *x, (rows, qualified), own))
                .collect();
            format!(
                "fn{}({}) -> {}{shown}",
                if suspends { "!" } else { "" },
                params.join(", "),
                show_marked(names, pool, result, (rows, qualified), own)
            )
        }
        TyData::Opaque {
            owner, args, bound, ..
        } => {
            let shown = show_ty_opts(names, pool, bound, rows, qualified);
            let shown = format!("impl {}", shown.strip_prefix("dyn ").unwrap_or(&shown));
            if qualified {
                format!("{shown} returned by {}[{}]", seg(owner), list(args))
            } else {
                shown
            }
        }
        TyData::Param(p) => names.param_name(p.owner, p.index),
        TyData::Assoc { assoc, self_ty, .. } => {
            format!(
                "{}::{}",
                show_ty_opts(names, pool, self_ty, rows, qualified),
                seg(assoc)
            )
        }
        TyData::Infer(_) | TyData::Canon(_) => "?".into(),
        TyData::Row(r) => show_row_in(names, pool, r, qualified),
        TyData::Context(r) => format!("$.Context[{}]", show_row_in(names, pool, r, qualified)),
    }
}

/// A row as a user reads it, after `$`: its keys in content order
/// (`crates/hd_sched/DESIGN.md` §6.5), not interning order, then its row parameters.
fn show_row_in(
    names: &Names<'_>,
    pool: hd_types::Types<'_>,
    r: hd_types::RowId,
    qualified: bool,
) -> String {
    let d = pool.row_data(r);
    if d.keys.is_empty() && d.params.is_empty() {
        return "$()".into();
    }
    let mut parts: Vec<String> = d
        .keys
        .iter()
        .map(|k| show_ty_opts(names, pool, *k, false, qualified))
        .collect();
    parts.sort_stable();
    let mut ps: Vec<String> = d
        .params
        .iter()
        .map(|p| names.param_name(p.owner, p.index))
        .collect();
    ps.sort_stable();
    parts.extend(ps);
    format!("$ {}", parts.join(" + "))
}

#[cfg(test)]
mod tests {
    use super::{
        Export, FnSig, Generic, HeadKind, Item, ItemData, Names, decode_items, decode_lazy,
        encode_items,
    };
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::{InternPool, ParamRef, TyData};

    fn indexed_item(def: hd_base::DefId, name: u32) -> Item {
        Item::new(
            def,
            hd_base::Symbol::from_raw(name),
            true,
            ItemData::Data(Vec::new()),
        )
    }

    fn indexed_folder(folder: &str, items: super::Items) -> super::FolderIface {
        super::folder_iface(
            folder,
            items,
            Vec::new(),
            std::sync::Arc::from(&b""[..]),
            None,
            &[],
            Vec::new(),
        )
    }

    #[test]
    fn lookup_keeps_own_precedence_and_restricts_sparse_ids_to_visible_folders() {
        use hd_base::{DefId, FolderId};
        let shared = DefId::from_raw(3);
        let distant = DefId::from_raw(257);
        let first = indexed_folder("first", super::Items::new(vec![indexed_item(shared, 1)]));
        let later = indexed_folder("later", super::Items::new(vec![indexed_item(distant, 2)]));
        let index = super::IfaceIndex::default();
        index.insert(FolderId::from_raw(2), &first);
        let visible = [(FolderId::from_raw(2), &first)];
        let lookup = super::Lookup::new(&[], visible, &index);
        assert!(std::ptr::eq(
            lookup.item(shared).unwrap(),
            first.item(shared).unwrap()
        ));
        assert!(
            lookup.item(distant).is_none(),
            "a pending folder has no row"
        );
        // A later folder may publish while an older lookup is being used.
        // Its new rows must not become visible in that lookup's closure.
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                for _ in 0..100 {
                    assert!(lookup.item(shared).is_some());
                    assert!(lookup.item(distant).is_none());
                }
            });
            index.insert(FolderId::from_raw(7), &later);
            reader.join().unwrap();
        });
        assert!(lookup.item(distant).is_none(), "outside the closure");
        assert!(lookup.item(DefId::from_raw(256)).is_none(), "a sparse hole");
        assert!(
            lookup.item(DefId::from_raw(u32::MAX - 1)).is_none(),
            "foreign id"
        );
        assert!(lookup.item(DefId::NONE).is_none());
        let own = vec![indexed_item(shared, 3), indexed_item(shared, 4)];
        let lookup = super::Lookup::new(
            &own,
            [
                (FolderId::from_raw(2), &first),
                (FolderId::from_raw(7), &later),
            ],
            &index,
        );
        assert!(
            std::ptr::eq(lookup.item(shared).unwrap(), own.get(1).unwrap()),
            "last own item wins"
        );
        assert!(std::ptr::eq(
            lookup.item(distant).unwrap(),
            later.item(distant).unwrap()
        ));
    }

    #[test]
    fn lookup_preserves_folder_order_and_bad_record_fallback_for_duplicate_ids() {
        use hd_base::{DefId, FolderId};
        let shared = DefId::from_raw(3);
        let first = indexed_folder("first", super::Items::new(vec![indexed_item(shared, 1)]));
        let second = indexed_folder(
            "second",
            super::Items::new(vec![indexed_item(shared, 2), indexed_item(shared, 3)]),
        );
        let missing = indexed_folder(
            "missing",
            super::Items::with_source(vec![shared], vec![super::NOT_IMPL], None),
        );
        let index = super::IfaceIndex::default();
        // Registration order cannot decide a lookup's duplicate precedence.
        index.insert(FolderId::from_raw(4), &second);
        index.insert(FolderId::from_raw(1), &first);
        index.insert(FolderId::from_raw(7), &missing);
        let lookup = super::Lookup::new(
            &[],
            [
                (FolderId::from_raw(1), &first),
                (FolderId::from_raw(4), &second),
            ],
            &index,
        );
        assert!(std::ptr::eq(
            lookup.item(shared).unwrap(),
            first.item(shared).unwrap()
        ));
        let lookup = super::Lookup::new(
            &[],
            [
                (FolderId::from_raw(7), &missing),
                (FolderId::from_raw(4), &second),
            ],
            &index,
        );
        assert!(std::ptr::eq(
            lookup.item(shared).unwrap(),
            second.items.get(1).unwrap()
        ));
    }

    #[test]
    fn lookup_directory_keeps_interface_records_lazy() {
        use hd_base::FolderId;
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let known = crate::KnownItems::new(&paths);
        let names = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
        };
        let items: Vec<Item> = (0..3)
            .map(|i| {
                let name = format!("item{i}");
                Item::new(
                    names.item("app", &name),
                    syms.intern(&name),
                    true,
                    ItemData::Data(Vec::new()),
                )
            })
            .collect();
        let blob = encode_items(&names, &items, &[]).unwrap();
        let (lazy, _) = decode_lazy(&names, std::sync::Arc::from(blob)).unwrap();
        let iface = indexed_folder("app", lazy);
        let index = super::IfaceIndex::default();
        index.insert(FolderId::from_raw(0), &iface);
        let lookup = super::Lookup::new(&[], [(FolderId::from_raw(0), &iface)], &index);
        assert_eq!(
            iface.items.decoded(),
            0,
            "directory construction reads only indexes"
        );
        assert_eq!(lookup.item(items[1].def), Some(&items[1]));
        assert_eq!(lookup.item(items[1].def), Some(&items[1]));
        assert_eq!(
            iface.items.decoded(),
            1,
            "only the selected record decodes, once"
        );
    }

    #[test]
    fn docs_round_trip_outside_the_semantic_blob() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let known = crate::KnownItems::new(&paths);
        let names = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
        };
        let mut item = Item::new(
            names.item("app", "Record"),
            syms.intern("Record"),
            true,
            ItemData::Data(vec![]),
        );
        let plain = encode_items(&names, &[item.clone()], &[]).expect("semantic blob");
        item.docs = vec![
            (vec![], "Type docs.".into()),
            (vec!["field".into()], "Field docs.".into()),
        ];
        let blob = encode_items(&names, &[item.clone()], &[]).expect("semantic blob");
        assert_eq!(blob, plain, "docs never alter API or coherence hashes");
        let docs = super::encode_docs(&[item.clone()]);
        let (mut lazy, _) = decode_lazy(&names, std::sync::Arc::from(blob)).expect("decode");
        lazy.attach_docs(&docs).expect("auxiliary docs");
        assert_eq!(lazy.decoded(), 0, "attaching docs preserves lazy items");
        assert_eq!(lazy.get(0), Some(&item));
        assert!(
            lazy.attach_docs(&docs[..docs.len() - 1]).is_none(),
            "truncated metadata is a miss"
        );
    }

    #[test]
    fn items_round_trip_into_a_fresh_run() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let known = crate::KnownItems::new(&paths);
        let n = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
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
        let known2 = crate::KnownItems::new(&paths2);
        let n2 = Names {
            pool: &pool2,
            paths: &paths2,
            syms: &syms2,
            known: &known2,
        };
        let (back, ex) = decode_items(&n2, &blob).expect("decode");
        assert_eq!(back[0].def, n2.item("app.geo", "first"));
        assert_eq!(ex[0].def, n2.item("app.geo", "first"));
        assert_eq!(encode_items(&n2, &back, &ex).expect("again"), blob);
        // The parameter names come with the index, before any record.
        assert_eq!(n2.param_name(back[0].def, 0), "T");
    }

    /// A lazily read interface decodes only the records asked for
    /// (docs/compiler/data-structures.md §3.9.1, #213): its index names
    /// every item's definition and parameters without decoding one.
    #[test]
    fn a_lazy_interface_decodes_only_the_items_it_is_asked_for() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let known = crate::KnownItems::new(&paths);
        let n = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
        };
        let items: Vec<Item> = (0..10)
            .map(|i| {
                let name = format!("f{i}");
                let f = n.item("app.geo", &name);
                let t = pool.intern_ty(&TyData::Param(ParamRef { owner: f, index: 0 }));
                let sig = FnSig::simple(
                    vec![Generic::plain(syms.intern("T"))],
                    vec![(syms.intern("a"), t)],
                    t,
                );
                Item::new(f, syms.intern(&name), true, ItemData::Fn(sig))
            })
            .collect();
        let blob = encode_items(&n, &items, &[]).expect("encode");
        let (items2, _) = decode_lazy(&n, std::sync::Arc::from(blob.as_slice())).expect("decode");
        assert_eq!(items2.len(), 10);
        assert_eq!(items2.decoded(), 0, "the index reads no record");
        assert_eq!(items2.defs()[7], n.item("app.geo", "f7"));
        assert_eq!(items2.get(7).expect("item").name, syms.intern("f7"));
        assert_eq!(items2.get(7).expect("again").def, n.item("app.geo", "f7"));
        assert_eq!(items2.decoded(), 1, "one record, decoded once");
        assert_eq!(items2.iter().count(), 10);
        assert_eq!(items2.decoded(), 10);
    }
}
