//! Host codecs over linear memory (`crates/hd_wasm/DESIGN.md` §23.23):
//! the encoders and decoders of the values that cross the exchange buffer
//! (`crates/hd_run_wasmtime/DESIGN.md` §17.4), generated per boundary type
//! from its linear layout, and the host method slots that call them.
//!
//! The boundary bytes follow the host ABI: an encoder reads a value from its Wasm values (and, for a list, from
//! its buffer in the heap) and writes it to the exchange buffer; a decoder
//! reads the exchange buffer and builds the value's Wasm values, allocating its strings and
//! lists in the heap with their counts. Strings and byte lists cross with
//! one `memory.copy` between the two memories.
//!
//! Each encoder is `(off, values...) -> off'`: it writes the value at
//! `off`, growing the buffer first, and returns the end. Each decoder is
//! `(off) -> (values..., off')`: it reads the value at `off`, checking
//! every length against the buffer's size, and returns its values, each
//! with a count of its own, and the end. A malformed or out-of-range
//! result is the `host-contract` panic.

use crate::op::Op as I;
use hd_base::{DefId, Hash128, StageResult};
use hd_mono::layout::StdKind;
use hd_mono::{BoundaryMap, ProgramEnv, Target, subst};
use hd_tir::ir::{
    Body, Callee, NONE, PlaceRoot, Proj, Ref, Role, Tag, Topology, Word, local_flags, slot_words,
};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};
use wasm_encoder::BlockType;

use crate::Input;
use crate::layout::{self, Leaf, Walk};
use crate::module::{BUF, EMPTY, Func, HEAP, Import, Ins, Module, Sym, Val, mem};
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_LEN, own_arg};
use crate::task::{HostArg, HostRes};

/// A boundary type on this backend: its structure, and what its codecs
/// need of its linear layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LTy {
    /// `void`: no bytes, no values.
    Void,
    /// An integer or `bool` (one bit): LEB128, zigzag
    /// when signed; an `i64` past 32 bits, else an `i32`.
    Int {
        bits: u8,
        signed: bool,
    },
    /// A Unicode scalar value: unsigned LEB128, checked before narrowing.
    Char,
    /// Raw IEEE 754 little-endian bytes.
    F32,
    F64,
    /// A string `(address, length)`: its length, then its bytes.
    Str,
    /// A decoder-only string whose bytes no reachable consumer observes.
    /// It checks the length and bounds as `Str`, returning the static empty view.
    DiscardStr,
    /// `List[u8]`, a buffer's address: its length, then its bytes.
    Bytes,
    /// Any other list: its count, then its elements.
    List(Box<LList>),
    /// A struct or a tuple, inline: its fields in order, whose values
    /// follow one another.
    Record(Vec<LTy>),
    /// An enum or an optional: the variant index, then the payload fields.
    Enum(Box<LEnum>),
    /// A host handle: its number. It only crosses from the host.
    Handle(Box<LHandle>),
    /// A data type with a field that is not `pub`, at a runtime profile's
    /// boundary: the string its std host codec writes or reads
    /// (`module.boundary.serialize.exact`).
    Codec(Box<LCodec>),
    /// A recursive data type: its one encoder and one decoder, which call
    /// themselves where the type recurs ([`Named`]).
    Named(Box<LNamed>),
    /// `Map[K, V]`, a table's address: its live entry count, then each
    /// entry's key and value in the order they were added
    /// (`module.boundary.allowed`).
    Map(Box<LMap>),
    /// A host method's `never` result (`module.boundary.never-result`): no
    /// bytes and no values. Its decoder is the `host-contract` panic that
    /// names the method `label`, for a host that answers anyway.
    Never(Box<str>),
    /// A counted reference `*T` or `*mut T` to data, the object's
    /// address: its referent's tree (`module.boundary.tree`).
    Object(Box<LObject>),
}

/// A counted reference at the boundary. It crosses as its referent, with
/// no trace of the reference, as std's `Serialize for *T` writes it
/// (`std-serde.std.counted-reference`). The decoder puts each referent in
/// a new object (`ref.box.new`), so a node that an acyclic graph shares
/// arrives once per path, as that many objects (`module.boundary.sharing`).
///
/// The encoder marks an object on its path in the object's count word
/// ([`ON_PATH`]), as a tracing collector marks a header during its walk,
/// so a value that reaches itself again is the `boundary-cycle` failure
/// (`module.boundary.cycle`) at O(1) per object, with no set to allocate.
/// Only an object that can be on a cycle (`cycle::holding`) is marked.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LObject {
    /// The referent's layout in the object and the object's release
    /// glue: its counted references, and whether it takes part in cycle
    /// collection. No std type with a `Drop` hook crosses into hd (none
    /// implements `Deserialize`, `module.boundary.error`), so it has no
    /// drop calls.
    pub elem: Elem,
    /// Whether its release glue can reach another object with the same
    /// glue (`cycle::recursive_object`), which then drains iteratively.
    pub recursive: bool,
    /// The descriptor alias a weak reference to the type reads, when the
    /// program makes one (`Module::weak_types`).
    pub weak: Option<Hash128>,
    /// Whether an object of the type can be on a cycle, so the encoder
    /// marks it on its path.
    pub tracked: bool,
    pub ty: LTy,
}

/// The count-word bit the encoder sets on an object while the object is
/// on its path ([`LObject`]). A count stays below it, as 2^30 references
/// would fill all of wasm32's memory. Every `*T` object is a heap one from
/// `rt:new` (`box`, `ref.expr.allocate`, or a decoder), counted from 1: a
/// static object, whose count word is 0, is a list buffer, a string, a
/// once-cell or a top-level binding's slot, and `ref.box.object` lets no
/// counted reference refer to one. Between the mark and the unmark only the
/// referent's encoders run; they read the value and write the buffer, and
/// a codec's retain of its argument is released by the codec, so any count
/// taken in between is given back around the bit.
const ON_PATH: u32 = 1 << 30;

/// A map at the boundary: its entry as the map glue lays it out, the key
/// operations its decoder rebuilds the table with
/// (`module.boundary.map-decode`), and its key and value types.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LMap {
    pub entry: crate::map::Entry,
    pub ops: crate::map::KeyOps,
    pub key: LTy,
    pub val: LTy,
}

/// A recursive data type's codec pair by number, in [`Named`], and its
/// Wasm values. Every occurrence of the type, the recursive ones inside
/// its own definition included, is this one node, so the type has one
/// encoder and one decoder.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LNamed {
    pub id: usize,
    pub vals: Vec<Val>,
}

/// The module's recursive boundary types (#1550): each one's definition,
/// in which the type's own occurrences are [`LTy::Named`], by number, and
/// the number of each type at each boundary direction.
///
/// [`of`] finds a recursive type as a nominal type it reaches again on its
/// path, as rustc's trait solver finds a cycle on its evaluation stack.
/// Only such a type is named; any other stays inline, so a program with
/// no recursive boundary type emits the codecs it did before.
#[derive(Default)]
pub struct Named {
    defs: Vec<Option<LTy>>,
    at: hd_base::HashMap<(Ty, Option<(bool, bool)>), usize>,
}

impl Named {
    /// Type `id`'s definition.
    fn def(&self, id: usize) -> &LTy {
        // ruled out: `data_of` fills a definition before it returns any
        // type that names it; a failed fill fails the link first.
        self.defs[id]
            .as_ref()
            .expect("ruled out: a named boundary type is defined before its codec is built")
    }
}

/// A list's element: its layout in the buffer and its boundary type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LList {
    pub elem: Elem,
    pub ty: LTy,
}

/// An enum's layout (`layout::Flat`: its cells, its tag, each payload
/// field's leaves among the cells) and, per variant, each payload field's
/// boundary type. The host format is the variant index, then the fields.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LEnum {
    pub layout: Box<layout::Flat>,
    pub variants: Vec<Vec<LTy>>,
}

/// A host handle's trait value: a box holding the handle number, beside
/// the static vtable `vtable` of host calls that take the number first.
/// With `wrap`, the `std.rt.host_provider` instance that puts it behind
/// a trait value reference (`std-net.handles.ref`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LHandle {
    pub vtable: usize,
    pub wrap: Option<Hash128>,
    /// Package handles release the JS registry record on the last HD count.
    /// This drops a binding, without invoking a provider's resource methods.
    pub release: Option<String>,
}

/// A private-field data type's host codec: the std instance that makes
/// its string (`T -> string`) when the value leaves hd, or builds it from
/// the string (`string -> T`) when it enters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LCodec {
    pub vals: Vec<Val>,
    /// The value's counted references, which the codec owns
    /// (`layout::Pass`): the encoder counts a copy for it.
    pub walk: Option<Walk>,
    pub codec: Hash128,
}

impl LTy {
    /// The type's Wasm values (`layout::vals`).
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            LTy::Void | LTy::Never(_) => vec![],
            LTy::Int { bits, .. } => vec![if *bits > 32 { Val::I64 } else { Val::I32 }],
            LTy::Char => vec![Val::I32],
            LTy::F32 => vec![Val::F32],
            LTy::F64 => vec![Val::F64],
            LTy::Str | LTy::DiscardStr | LTy::Handle(_) => vec![Val::I32, Val::I32],
            LTy::Bytes | LTy::List(_) | LTy::Map(_) | LTy::Object(_) => vec![Val::I32],
            LTy::Record(fields) => fields.iter().flat_map(LTy::vals).collect(),
            LTy::Enum(e) => e.layout.mem.vals(),
            LTy::Codec(c) => c.vals.clone(),
            LTy::Named(n) => n.vals.clone(),
        }
    }

    /// Whether the value has no bytes on the wire and no Wasm values:
    /// `void`, or a record of such fields, as a struct with no fields.
    /// Its codecs do nothing, so a caller emits no call to them.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            LTy::Void => true,
            LTy::Record(fields) => fields.iter().all(LTy::is_empty),
            _ => false,
        }
    }

    /// Whether a host handle is in the value: it only crosses from the
    /// host. Each named type's definition is visited once. Only a debug
    /// build's check of `module.boundary.not-safe.error` asks it.
    #[cfg(debug_assertions)]
    #[must_use]
    pub fn holds_handle(&self, named: &Named) -> bool {
        let mut seen = hd_base::HashSet::default();
        let mut work = vec![self];
        while let Some(t) = work.pop() {
            match t {
                LTy::Handle(_) => return true,
                LTy::List(l) => work.push(&l.ty),
                LTy::Map(mp) => work.extend([&mp.key, &mp.val]),
                LTy::Object(o) => work.push(&o.ty),
                LTy::Record(fields) => work.extend(fields),
                LTy::Enum(e) => work.extend(e.variants.iter().flatten()),
                LTy::Named(n) => {
                    if seen.insert(n.id) {
                        work.push(named.def(n.id));
                    }
                }
                LTy::Void
                | LTy::Never(_)
                | LTy::Int { .. }
                | LTy::Char
                | LTy::F32
                | LTy::F64
                | LTy::Str
                | LTy::DiscardStr
                | LTy::Bytes
                | LTy::Codec(_) => {}
            }
        }
        false
    }

    /// An enum whose payloads are at most one payload-free enum each. Its
    /// encoding is one or two variant indices, which a host leaf reads as
    /// bytes (`HostRes::Tags`).
    #[must_use]
    pub fn is_tags(&self) -> bool {
        let LTy::Enum(e) = self else {
            return false;
        };
        e.variants.len() < 128
            && e.layout
                .cell()
                .is_some_and(|d| d.enc == layout::Enc::Direct)
            && e.variants.iter().all(|v| match v.as_slice() {
                [] | [LTy::Void] => true,
                [LTy::Enum(p)] => p.variants.len() < 128 && p.variants.iter().all(Vec::is_empty),
                _ => false,
            })
    }
}

/// A runtime profile's boundary: the host codecs its private-field types
/// cross through, and whether the value leaves hd.
#[derive(Clone, Copy)]
pub struct AtProfile<'a> {
    pub codecs: &'a BoundaryMap,
    pub out: bool,
    /// Package adapters use nominal namespaces, including returned handles.
    pub package: bool,
    /// The concrete owner arguments of an inherited method, Self first.
    pub arguments: Option<TyList>,
}

/// The boundary type of `t`, a type in a checked host signature
/// (`module.boundary.not-safe.error` rejects one that cannot cross).
pub fn of(input: &Input<'_>, m: &mut Module, t: Ty, at: Option<AtProfile<'_>>) -> StageResult<LTy> {
    of_at(input, m, t, &mut Vec::new(), at)
}

/// The nominal data types `of_at` is building, outermost first, each with
/// its [`Named`] number once one of its parts reaches it again.
type Path = Vec<(Ty, Option<usize>)>;

/// The nominal data types [`package_value`] is building, outermost first,
/// each with its schema binder number once one of its parts reaches it
/// again.
type Binders = Vec<(Ty, Option<u32>)>;

fn of_at(
    input: &Input<'_>,
    m: &mut Module,
    t: Ty,
    path: &mut Path,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let s = layout::strip(pool, env, t);
    // A data type with a field that is not `pub` crosses a profile's
    // boundary as the string of its std host codec.
    if let (TyData::Adt { def, .. }, Some(at)) = (pool.get(s), at)
        && env.has_private_field(def)
    {
        return codec(input, t, s, at);
    }
    let int = |bits, signed| LTy::Int { bits, signed };
    Ok(match pool.get(s) {
        TyData::Prim(p) => match p {
            Prim::F32 => LTy::F32,
            Prim::F64 => LTy::F64,
            Prim::I8 => int(8, true),
            Prim::I16 => int(16, true),
            Prim::I32 => int(32, true),
            Prim::I64 => int(64, true),
            Prim::U8 => int(8, false),
            Prim::U16 => int(16, false),
            Prim::U32 | Prim::Usize => int(32, false),
            Prim::U64 => int(64, false),
            Prim::Bool => int(1, false),
            Prim::Char => LTy::Char,
            Prim::String => LTy::Str,
        },
        // A rest element is one more member, its `List[T]`
        // (`types.tuple.rest.list`).
        TyData::Tuple { .. } => {
            let mut fields = Vec::new();
            for e in layout::tuple_members(pool, s) {
                fields.push(of_at(input, m, e, path, at)?);
            }
            // `void` is the empty tuple (`types.void`): it crosses as no
            // value.
            if fields.is_empty() {
                LTy::Void
            } else {
                LTy::Record(fields)
            }
        }
        TyData::Ref { inner, counted, .. } => {
            let referent = layout::strip(pool, env, inner);
            let TyData::TraitValue { def, args, .. } = pool.get(referent) else {
                // ruled out: module.boundary.not-safe.error (hd_check
                // header::crossing_fault) rejects a local reference to data.
                debug_assert!(counted, "a local reference to data at the host boundary");
                return object_of(input, m, inner, path, at);
            };
            // A trait value reference to a host handle: the handle,
            // behind the reference the `host_provider` instance makes.
            // ruled out: hd_mono's note_boundary_value records that
            // wrapper for each trait value reference a host result brings
            // in, and module.boundary.not-safe.error (hd_check
            // header::crossing_fault) rejects one passed to the host.
            let wrap = input.collected.boundary.get(&t).and_then(|c| c.handle_ref);
            debug_assert!(
                wrap.is_some(),
                "a host handle without its `host_provider` wrapper"
            );
            // ruled out: module.host.package.not-generic (hd_check
            // header::boundary) rejects a returned handle trait with type
            // parameters, and std's handle traits declare none.
            debug_assert_eq!(args, TyList::EMPTY, "a generic host handle trait");
            LTy::Handle(Box::new(LHandle {
                wrap,
                ..handle_of(input, m, def, at)?
            }))
        }
        TyData::Adt { def, args } => match env.std_kind(def) {
            hd_mono::layout::StdKind::List => {
                let items = pool.list_items(args);
                debug_assert_eq!(items.len(), 1, "ruled out: `List` has one element type");
                let e = items[0];
                // ruled out: std's `List` comes with `std.rt`, whose
                // runtime items hd_driver roots whenever std has their
                // bodies, so a program with a list links the allocator.
                debug_assert!(
                    input.runtime.is_some(),
                    "a boundary list without std's allocator"
                );
                if matches!(
                    pool.get(layout::strip(pool, env, e)),
                    TyData::Prim(Prim::U8)
                ) {
                    LTy::Bytes
                } else {
                    let mem = layout::mem(pool, env, e)?;
                    let elem = Elem {
                        size: mem.size,
                        leaves: mem.leaves,
                        walk: input.runtime.and_then(|_| layout::walk(pool, env, e)),
                        drops: Vec::new(),
                        colls: layout::coll_leaves(pool, env, e),
                        cyclic: input.runtime.is_some() && crate::cycle::object(pool, env, t),
                    };
                    let ty = of_at(input, m, e, path, at)?;
                    LTy::List(Box::new(LList { elem, ty }))
                }
            }
            hd_mono::layout::StdKind::Map => {
                let items = pool.list_items(args);
                debug_assert_eq!(
                    items.len(),
                    2,
                    "ruled out: `Map` has a key and a value type"
                );
                let (k, v) = (items[0], items[1]);
                let entry = crate::map::Entry::of(input, s, k, v)?;
                let collected = &input.collected.boundary;
                // ruled out: hd_mono's note_boundary_value selects the key
                // operations of every map type a boundary value reaches.
                let ops = (collected.get(&t).or_else(|| collected.get(&s)))
                    .and_then(|c| c.map_key.as_ref())
                    .map(|o| crate::map::KeyOps::of(input, m, o, &entry.key_vals()))
                    .expect("ruled out: a boundary map without its key operations");
                let key = of_at(input, m, k, path, at)?;
                let val = of_at(input, m, v, path, at)?;
                LTy::Map(Box::new(LMap {
                    entry,
                    ops,
                    key,
                    val,
                }))
            }
            StdKind::Other | StdKind::Suspend => {
                // ruled out: module.boundary.not-safe.error (hd_check
                // header::crossing_fault) rejects a suspension, a live
                // runtime handle (`module.boundary.not-safe`).
                debug_assert_ne!(
                    env.std_kind(def),
                    StdKind::Suspend,
                    "a suspension at the host boundary"
                );
                // ruled out: data.enum.no-recursion (hd_check header) and
                // its instance check through associated payloads (hd_check
                // value.rs `ProjectionCycles`, #1554) reject every enum
                // that holds itself by value, so none is boxed here.
                debug_assert!(
                    matches!(layout::kind(pool, env, s), layout::Kind::Value),
                    "a boxed enum at the host boundary"
                );
                return data_of(input, m, s, def, args, path, at);
            }
        },
        // A union crosses as the enum of its cases
        // (`types.union.representation`, `module.boundary.allowed`): its
        // tag, then that case's payload.
        TyData::Union(members) => {
            enum_of(input, m, &layout::union_variants(pool, members), path, at)?
        }
        // ruled out: `never` crosses only as a host method's whole result,
        // which `result_type` takes first, and module.boundary.not-safe.error
        // (hd_check header::crossing_fault) rejects it anywhere else, a
        // function and a provider context (`module.boundary.rows`); a bare
        // trait value is no value type (grammar.type.dyn.reference-values);
        // `signature` and `subst` leave no parameter, projection or opaque
        // type in a checked host method's signature (`dyn-member-unavailable`
        // rejects a method with its own type parameters or an `impl Trait`
        // result, and module.host.package.no-assoc (hd_check
        // header::boundary) a host or handle trait with an associated
        // type); a row is no value type, and checking solves every
        // inference variable.
        TyData::Never
        | TyData::Poison
        | TyData::Fn { .. }
        | TyData::TraitValue { .. }
        | TyData::Opaque { .. }
        | TyData::Param(_)
        | TyData::Const(_)
        | TyData::Assoc { .. }
        | TyData::Row(_)
        | TyData::Context(_)
        | TyData::Infer(_)
        | TyData::Canon(_) => {
            debug_assert!(false, "{:?} at the host boundary", pool.get(t));
            LTy::Void
        }
    })
}

/// The host handle of trait `def`, returned by a host method at `at`
/// ([`LHandle`]), before [`of_at`] gives it its `host_provider` wrapper.
fn handle_of(
    input: &Input<'_>,
    m: &mut Module,
    def: DefId,
    at: Option<AtProfile<'_>>,
) -> StageResult<LHandle> {
    let path = (input.path)(def).replace('/', ".");
    // ruled out: module.boundary.not-safe.error (hd_check
    // header::crossing_fault) rejects a trait value passed to the host.
    debug_assert!(
        at.is_none_or(|a| !a.out),
        "a host handle passed to the host"
    );
    let host = match (at, hd_host_abi::handle_trait(&path)) {
        (Some(at), _) if at.package => HandleHost::Profile(at),
        (_, Some(abi)) => HandleHost::Abi(abi),
        (at, None) => {
            // A value of a program's own trait that a profile's host
            // method returns can only be a handle the profile made, whose
            // methods it imports from `hd:<Trait>` as a profile does.
            // ruled out: module.boundary.not-safe.error (hd_check
            // header::boundary) rejects a returned trait value whose trait
            // is std's but not a host handle trait, and std's own host
            // methods return only `hd_host_abi::HANDLES` traits.
            debug_assert!(
                at.is_some() && !path.starts_with("std."),
                "a returned trait value that is not a host handle"
            );
            HandleHost::Profile(at.unwrap_or(AtProfile {
                codecs: &input.collected.boundary,
                out: false,
                package: false,
                arguments: None,
            }))
        }
    };
    let release = match host {
        HandleHost::Profile(at) if at.package => {
            (input.package_trait)(def).map(|i| package_module(&i, true))
        }
        HandleHost::Profile(_) | HandleHost::Abi(_) => None,
    };
    Ok(LHandle {
        vtable: handle_vtable(input, m, def, host)?,
        wrap: None,
        release,
    })
}

/// The boundary type of a counted reference to `target` ([`LObject`]):
/// the referent's, with the layout and descriptor of the object the
/// decoder puts it in, as `box` makes one (`ref.box.new`).
fn object_of(
    input: &Input<'_>,
    m: &mut Module,
    target: Ty,
    path: &mut Path,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let mem = layout::mem(pool, env, target)?;
    let counts = input.runtime.is_some();
    let tracked = crate::cycle::holding(pool, env, &[target]);
    let key = hd_mono::layout::canon(
        pool,
        &|d| env.path_hash(d),
        &mut hd_mono::layout::CanonMemo::default(),
        target,
    );
    let weak = (m.open_world || m.weak_types.contains(&key)).then_some(key);
    let elem = Elem {
        size: mem.size,
        leaves: mem.leaves,
        walk: input
            .runtime
            .and_then(|_| layout::count_walk(pool, env, target)),
        drops: Vec::new(),
        colls: Vec::new(),
        cyclic: counts && tracked,
    };
    let ty = of_at(input, m, target, path, at)?;
    Ok(LTy::Object(Box::new(LObject {
        elem,
        recursive: counts && crate::cycle::recursive_object(pool, env, target),
        weak,
        tracked,
        ty,
    })))
}

/// The boundary type of the unboxed struct or enum `s`, instance `def[args]`.
/// A type that one of its parts reaches again is recursive: that part,
/// and every later occurrence, is [`LTy::Named`], whose definition this
/// builds (`module.boundary.allowed` admits it; #1550).
///
/// Cost: one memo lookup per nominal type reached and one scan of the
/// path of nominal types being built, which is as deep as the type's
/// nesting of distinct nominal types. A recursive type's definition is
/// built once per boundary direction.
fn data_of(
    input: &Input<'_>,
    m: &mut Module,
    s: Ty,
    def: DefId,
    args: TyList,
    path: &mut Path,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let key = (s, at.map(|a| (a.out, a.package)));
    let named = |id: usize| -> StageResult<LTy> {
        Ok(LTy::Named(Box::new(LNamed {
            id,
            vals: layout::vals(pool, env, s)?,
        })))
    };
    if let Some(&id) = m.named_codecs.at.get(&key) {
        return named(id);
    }
    if let Some((_, id)) = path.iter_mut().find(|(t, _)| *t == s) {
        let id = *id.get_or_insert_with(|| {
            m.named_codecs.defs.push(None);
            m.named_codecs.defs.len() - 1
        });
        return named(id);
    }
    path.push((s, None));
    let built = if let Some(variants) = env.enum_variants(def, args) {
        enum_of(input, m, &variants, path, at)
    } else {
        // A struct with no fields crosses as an empty record: no bytes
        // and no Wasm values.
        let mut out = Vec::new();
        // A failed part fails the link, so its path needs no unwinding,
        // as in `enum_of`.
        for f in env.data_fields(def).unwrap_or_default() {
            out.push(of_at(input, m, subst(pool, env, def, args, f), path, at)?);
        }
        Ok(LTy::Record(out))
    };
    // The entry pushed above, with its number if a part reached it.
    let id = path.pop().and_then(|(_, id)| id);
    let built = built?;
    let Some(id) = id else {
        return Ok(built);
    };
    let node = named(id)?;
    assert_eq!(
        built.vals(),
        node.vals(),
        "a named boundary type's values are its layout's (layout::vals)"
    );
    m.named_codecs.defs[id] = Some(built);
    m.named_codecs.at.insert(key, id);
    Ok(node)
}

/// The boundary type of an enum of `variants`, laid out as
/// `layout::enum_mem` lays it out.
fn enum_of(
    input: &Input<'_>,
    m: &mut Module,
    variants: &[Vec<Ty>],
    path: &mut Path,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let layout = layout::enum_mem(input.pool, input.env, variants)?;
    let mut vs = Vec::new();
    for fields in variants {
        let mut v = Vec::new();
        for f in fields {
            v.push(of_at(input, m, *f, path, at)?);
        }
        vs.push(v);
    }
    Ok(LTy::Enum(Box::new(LEnum {
        layout: Box::new(layout),
        variants: vs,
    })))
}

/// A private-field type `t` (stripped `s`) at a profile's boundary.
fn codec(input: &Input<'_>, t: Ty, s: Ty, at: AtProfile<'_>) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let codecs = at.codecs.get(&t).or_else(|| at.codecs.get(&s));
    let codec = codecs
        .and_then(|c| match (at.out, at.package) {
            (true, true) => c.package_encode,
            (true, false) => c.encode,
            (false, true) => c.package_decode,
            (false, false) => c.decode,
        })
        .expect(
            "ruled out: hd_mono's note_boundary_value collects the host codec of each \
             private-field type a host signature reaches, in its direction and namespace",
        );
    Ok(LTy::Codec(Box::new(LCodec {
        vals: layout::vals(pool, env, s)?,
        walk: match layout::pass(pool, env, input.runtime.is_some(), s) {
            layout::Pass::Owned(w) => Some(w),
            _ => None,
        },
        codec,
    })))
}

/// One host method's row as [`host_slot`] lowers it: from the ABI table,
/// or derived from a runtime profile's own trait.
pub struct Row<'a> {
    pub name: &'a str,
    pub params: &'a [hd_host_abi::Codec],
    pub result: hd_host_abi::Codec,
    pub wait: hd_host_abi::Wait,
}

impl Row<'_> {
    #[must_use]
    pub fn of(hm: &hd_host_abi::HostMethod) -> Row<'_> {
        Row {
            name: hm.name,
            params: hm.params,
            result: hm.result,
            wait: hm.wait,
        }
    }
}

/// The method `m`'s parameter types after the receiver and its result
/// type, at the trait value (`Self` the class `REF`).
pub fn signature(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    m: DefId,
    arguments: Option<TyList>,
) -> (Vec<Ty>, Ty) {
    let full = arguments.unwrap_or_else(|| pool.list(&[hd_mono::class_ref(pool)]));
    let params = env
        .params(m)
        .unwrap_or_default()
        .into_iter()
        .skip(1)
        .map(|p| subst(pool, env, m, full, p))
        .collect();
    let ret = subst(pool, env, m, full, env.ret(m).unwrap_or(Ty::VOID));
    (params, ret)
}

/// A host provider's vtable slot for method `m` that no reachable code
/// calls (`hd_mono::Collected::calls_dyn`): `(data, params) -> results`, a
/// trap with no import.
pub fn uncalled_slot(input: &Input<'_>, m: DefId) -> StageResult<Helper> {
    uncalled_slot_at(input, m, None)
}

pub fn uncalled_slot_at(
    input: &Input<'_>,
    m: DefId,
    arguments: Option<TyList>,
) -> StageResult<Helper> {
    let (pool, env) = (input.pool, input.env);
    let (params, ret) = signature(pool, env, m, arguments);
    let mut all = vec![Val::I32];
    for p in &params {
        all.extend(layout::vals(pool, env, *p)?);
    }
    let results = if env.suspends(m) {
        vec![Val::I32]
    } else {
        layout::vals(pool, env, ret)?
    };
    Ok(Helper::DynTrap {
        params: all,
        results,
    })
}

/// One vtable slot of a host provider (`crates/hd_run_wasmtime/DESIGN.md`
/// §17.2) for method `m` of the trait imported from `hd:<key>`: a host
/// call for a method that never waits, a cold leaf frame for one that
/// may. With `handle`, the slot is a host handle's inner call: the
/// handle number is its first argument, an `i32` (`Helper::HandleSlot`).
/// At a profile's boundary `at`, private-field types cross as their host codec's string.
pub fn host_slot(
    input: &Input<'_>,
    mm: &mut Module,
    key: &str,
    row: &Row<'_>,
    m: DefId,
    handle: bool,
    at: Option<AtProfile<'_>>,
) -> StageResult<Helper> {
    use hd_host_abi::{Codec, Scalar, Wait};
    let (pool, env) = (input.pool, input.env);
    // The row states each scalar's Wasm value.
    let scalar = |s: Scalar| match s {
        Scalar::I32 => Val::I32,
        Scalar::I64 => Val::I64,
        Scalar::F64 => Val::F64,
    };
    // ruled out, for each row check below: a std host method's row in
    // `hd_host_abi::TABLE` or `HANDLES` mirrors its declaration in lib/std,
    // which ships with it, and a profile's or package's row is built from
    // the method's own signature (`rt::profile_tree`, `HandleTree::slots`,
    // `result_codec`).
    let waits = row.wait == Wait::May;
    debug_assert_eq!(
        env.suspends(m),
        waits,
        "the host method `{key}.{}` waits",
        row.name
    );
    let (params, ret) = signature(pool, env, m, at.and_then(|a| a.arguments));
    let mut args = Vec::new();
    let mut types = Vec::new();
    let codecs = if handle {
        args.push(HostArg::Scalar(Val::I32));
        types.push(Ty::I32);
        row.params.get(1..).unwrap_or_default()
    } else {
        row.params
    };
    debug_assert_eq!(
        params.len(),
        codecs.len(),
        "the host method `{key}.{}`'s parameters",
        row.name
    );
    for (pt, c) in params.iter().zip(codecs) {
        let vals = layout::vals(pool, env, *pt)?;
        args.push(match *c {
            // A parameter with no values passes nothing.
            Codec::Void => {
                debug_assert!(vals.is_empty(), "a `void` row parameter with values");
                continue;
            }
            // A lone string crosses as its bytes alone (§17.4).
            // A newtype of a string, as `Path`, is not one: it crosses with
            // its length.
            Codec::Buffer(_) if row.params.len() == 1 && *pt == Ty::STRING => HostArg::Str,
            Codec::Buffer(_) => {
                let b = of(input, mm, *pt, at.map(|a| AtProfile { out: true, ..a }))?;
                #[cfg(debug_assertions)]
                assert!(
                    !b.holds_handle(&mm.named_codecs),
                    "ruled out: module.boundary.not-safe.error (hd_check header::boundary) \
                     rejects a host handle passed to the host; no std host method takes one"
                );
                HostArg::Buf(b)
            }
            Codec::Scalar(s) => {
                let v = scalar(s);
                debug_assert_eq!(vals, [v], "a scalar row parameter of other values");
                HostArg::Scalar(v)
            }
        });
        types.push(*pt);
    }
    let rvals = layout::vals(pool, env, ret)?;
    let module = format!("hd:{key}");
    let method = row.name.to_owned();
    if !waits {
        let result = match row.result {
            Codec::Void => {
                debug_assert!(rvals.is_empty(), "a `void` row result with values");
                CallRes::Void
            }
            Codec::Scalar(s) => {
                let v = scalar(s);
                debug_assert_eq!(rvals, [v], "a scalar row result of other values");
                CallRes::Scalar(v)
            }
            Codec::Buffer(_) => CallRes::Buf(result_type(input, mm, m, ret, at)?),
        };
        return Ok(Helper::HostCall {
            module,
            method,
            args,
            result,
        });
    }
    let res = match row.result {
        Codec::Void => {
            debug_assert!(rvals.is_empty(), "a `void` row result with values");
            HostRes::Void
        }
        // A call that may wait answers in the buffer: no row that waits
        // states a scalar result.
        Codec::Scalar(_) | Codec::Buffer(_) => {
            debug_assert!(
                matches!(row.result, Codec::Buffer(_)),
                "a waiting host method `{key}.{}` with a scalar result",
                row.name
            );
            let b = result_type(input, mm, m, ret, at)?;
            if b.is_tags() && rvals == [Val::I32, Val::I32] {
                HostRes::Tags
            } else {
                HostRes::Buf(b)
            }
        }
    };
    Ok(Helper::HostCold {
        module,
        method,
        args,
        area: args_area(input, &types)?,
        res,
    })
}

/// Whether `ty`, a host method's result, is `never`
/// (`module.boundary.never-result`).
fn never(input: &Input<'_>, ty: Ty) -> bool {
    layout::strip(input.pool, input.env, ty) == Ty::NEVER
}

/// The row codec of a profile or package host method's result `ret`: none
/// for a result with no values, except `never`, whose call a host that
/// answers anyway breaks (`module.boundary.never-result`); else the
/// exchange buffer.
pub fn result_codec(input: &Input<'_>, ret: Ty) -> StageResult<hd_host_abi::Codec> {
    Ok(
        if layout::vals(input.pool, input.env, ret)?.is_empty() && !never(input, ret) {
            hd_host_abi::Codec::Void
        } else {
            hd_host_abi::Codec::Buffer("")
        },
    )
}

/// The codec belongs to the method's caller set, never merely its result type.
fn result_type(
    input: &Input<'_>,
    module: &mut Module,
    method: DefId,
    ty: Ty,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    if never(input, ty) {
        let label = (input.path)(method).replace('/', ".");
        return Ok(LTy::Never(label.into()));
    }
    let mut boundary = of(input, module, ty, at.map(|a| AtProfile { out: false, ..a }))?;
    if input.optimized
        && !module.open_world
        && has_direct_strings(&boundary)
        && host_result_tags_only(input, method)
    {
        discard_strings(&mut boundary);
    }
    Ok(boundary)
}

/// Whether a direct string field can use the non-owning decoder. Host codecs,
/// handles and collection construction retain their existing behavior and checks.
fn has_direct_strings(ty: &LTy) -> bool {
    match ty {
        LTy::Str => true,
        LTy::Record(fields) => fields.iter().any(has_direct_strings),
        LTy::Enum(e) => e.variants.iter().flatten().any(has_direct_strings),
        _ => false,
    }
}

fn discard_strings(ty: &mut LTy) {
    match ty {
        LTy::Str => *ty = LTy::DiscardStr,
        LTy::Record(fields) => fields.iter_mut().for_each(discard_strings),
        LTy::Enum(e) => e.variants.iter_mut().flatten().for_each(discard_strings),
        _ => {}
    }
}

/// A closed-program proof for one dynamic method, not one boundary type.
/// Any unknown, escaping, or observing use keeps its ordinary owned decoder.
fn host_result_tags_only(input: &Input<'_>, method: DefId) -> bool {
    let mut found = false;
    for id in input.order {
        let calls = &input.collected.calls[id.idx()];
        let mut topology = None;
        for (&call, target) in calls {
            if !matches!(target, Target::Dyn) {
                continue;
            }
            let Some(body) = (input.body)(*id) else {
                return false;
            };
            // A dynamic target may be a CallValue through a constant,
            // parameter or local function value. Its first operand is a Ref,
            // not a callee record; without a method identity any observer is
            // possible, so keep ordinary decoding for the whole method.
            let Some(callee) = dynamic_method(body, call) else {
                return false;
            };
            if callee != method {
                continue;
            }
            found = true;
            let result = if body.tags[call as usize] == Tag::Await {
                call
            } else if input.env.suspends(method) {
                let topo = topology.get_or_insert_with(|| Topology::new(body));
                let Some(on) = crate::drive::block_on_of(body, topo, calls, call) else {
                    return false;
                };
                // block_on_of checks the intrinsic target, so its value is the completed outcome.
                on
            } else if body.tags[call as usize] == Tag::Call {
                call
            } else {
                return false;
            };
            if !tags_only(body, result) {
                return false;
            }
        }
    }
    found
}

/// Read a method identity only from instruction kinds with a callee record.
/// Inlining can leave dynamic function values or rows this proof cannot resolve;
/// those are conservative failures, never record or instruction indices.
fn dynamic_method(body: &Body, call: u32) -> Option<DefId> {
    let i = usize::try_from(call).ok()?;
    if !matches!(body.tags.get(i), Some(Tag::Call | Tag::Await)) {
        return None;
    }
    let start = usize::try_from(body.data.get(i)?[0]).ok()?;
    let count = usize::try_from(*body.extra.get(start)?).ok()?;
    let start = start.checked_add(1)?;
    let words = body.extra.get(start..start.checked_add(count)?)?;
    match Callee::from_words(words)? {
        Callee::TraitMethod { method, .. } => Some(method),
        Callee::Item { .. } => None,
    }
}

fn tracked(values: &[bool], value: u32) -> bool {
    Ref(value)
        .as_inst()
        .is_some_and(|i| values.get(i.0 as usize) == Some(&true))
}

/// Follow the completed value through copies and enum projections. Stores to
/// another object, references, captures, calls and non-tag observations refuse.
/// The flow-insensitive union deliberately keeps a decoder when aliasing is unclear.
fn tags_only(body: &Body, root: u32) -> bool {
    if root as usize >= body.len() {
        return false;
    }
    let mut values = vec![false; body.len()];
    let mut locals = vec![false; body.local_ty.len()];
    let topo = Topology::new(body);
    values[root as usize] = true;
    loop {
        let mut changed = false;
        for i in 0..body.len() {
            let tag = body.tags[i];
            let used = slot_words(body, i, |word| {
                matches!(word, Word::Value(role) if role != Role::Dropped) || word == Word::Provider
            })
            .into_iter()
            .any(|v| tracked(&values, v));
            let place = matches!(
                tag,
                Tag::Read
                    | Tag::Store
                    | Tag::Ref
                    | Tag::Drop
                    | Tag::LoanEnd
                    | Tag::IsUnique
                    | Tag::FlagSet
                    | Tag::FlagClear
            )
            .then(|| body.decode_place(body.data[i][0]))
            .flatten();
            let placed = place.as_ref().is_some_and(|p| match p.root {
                PlaceRoot::Local(l) => locals.get(l.idx()) == Some(&true),
                PlaceRoot::Value(v) => tracked(&values, v.0),
                _ => false,
            });
            if place.as_ref().is_some_and(|p| {
                p.proj
                    .iter()
                    .any(|step| matches!(step, Proj::Elem(index) if tracked(&values, index.0)))
            }) {
                return false;
            }
            if tag == Tag::Store {
                // A tainted place root or index is an observation, not a value copy.
                if used && !tracked(&values, body.data[i][1]) {
                    return false;
                }
                if tracked(&values, body.data[i][1]) {
                    let Some(p) = &place else {
                        return false;
                    };
                    let PlaceRoot::Local(local) = p.root else {
                        return false;
                    };
                    if !p.proj.is_empty() {
                        return false;
                    }
                    if !locals[local.idx()] {
                        locals[local.idx()] = true;
                        changed = true;
                    }
                } else if placed && place.as_ref().is_some_and(|p| !p.proj.is_empty()) {
                    return false;
                }
            } else if tag == Tag::Read && placed {
                if place.as_ref().is_some_and(|p| {
                    p.proj
                        .iter()
                        .any(|s| !matches!(s, Proj::Field(_) | Proj::Variant(_)))
                }) {
                    return false;
                }
                if !values[i] {
                    values[i] = true;
                    changed = true;
                }
            } else if matches!(
                tag,
                Tag::Drop | Tag::LoanEnd | Tag::Release | Tag::FlagSet | Tag::FlagClear
            ) {
                if used {
                    return false;
                }
                // Ending a value's lifetime does not observe its string bytes.
            } else if used || placed {
                match tag {
                    Tag::SwitchTag | Tag::Match => {}
                    Tag::Block => {
                        if !values[i] {
                            values[i] = true;
                            changed = true;
                        }
                    }
                    Tag::Coerce if body.same_value_coercion(i) => {
                        if !values[i] {
                            values[i] = true;
                            changed = true;
                        }
                    }
                    _ => return false,
                }
            }
            // A block's value becomes the control expression's value, not its condition.
            if tag == Tag::Block && values[i] {
                let owner = topo.owner[i];
                if owner != NONE && !values[owner as usize] {
                    values[owner as usize] = true;
                    changed = true;
                }
            }
        }
        if locals
            .iter()
            .enumerate()
            .any(|(i, yes)| *yes && body.local_flags[i] & local_flags::CAPTURED != 0)
            || body.sub_root.iter().any(|root| tracked(&values, *root))
        {
            return false;
        }
        if !changed {
            return true;
        }
    }
}

/// A host leaf frame's arguments as one record of `types`, which the
/// frame keeps with a count of its own of each (`Helper::CopyElems`).
fn args_area(input: &Input<'_>, types: &[Ty]) -> StageResult<Elem> {
    let (pool, env) = (input.pool, input.env);
    let (mem, parts) = layout::payload_mem(pool, env, types)?;
    let mut colls = Vec::new();
    for (t, p) in types.iter().zip(&parts) {
        let first = u32::try_from(p.first).expect("values");
        colls.extend(
            layout::coll_leaves(pool, env, *t)
                .into_iter()
                .map(|k| first + k),
        );
    }
    let counts = input.runtime.is_some();
    Ok(Elem {
        size: mem.size,
        leaves: mem.leaves,
        walk: if counts {
            layout::fields_walk(pool, env, types)
        } else {
            None
        },
        drops: Vec::new(),
        colls: if counts { colls } else { Vec::new() },
        // The leaf frame that keeps a host call's arguments takes part as
        // they say: only an open value holds the frame.
        cyclic: counts && crate::cycle::holding(pool, env, types),
    })
}

/// Where a host handle's methods come from.
#[derive(Clone, Copy)]
enum HandleHost<'a> {
    /// A std handle trait's rows in `hd_host_abi::HANDLES`, each named
    /// `<Trait>.<method>` in the capability's module.
    Abi(&'static hd_host_abi::HostTrait),
    /// A runtime profile's or a package's own trait: each method is
    /// imported from `hd:<Trait>` by name, every argument and result in the
    /// buffer, private-field types through the codecs at `AtProfile`.
    Profile(AtProfile<'a>),
}

/// The vtable of host handle trait `trait_`: each slot a
/// [`Helper::HandleSlot`] over its host call, then the vtables of its
/// direct supertraits, whose methods are the handle trait's host methods
/// too, as `TcpStream.dispose!` (flow.using.async-disposable.row).
fn handle_vtable(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    host: HandleHost<'_>,
) -> StageResult<usize> {
    let package = matches!(host, HandleHost::Profile(a) if a.package);
    // One vtable per handle trait and boundary, reserved before its slots
    // are built: a slot whose result is a handle of a trait being built,
    // its own included, names the reserved table (#1551). A handle trait
    // declares no type parameters (`module.host.package.not-generic`), so
    // the trait and the boundary decide its slots.
    let memo = (trait_, package);
    if let Some(&k) = m.handle_vtables.get(&memo) {
        return Ok(k);
    }
    let reserved = m.vtable_reserve();
    m.handle_vtables.insert(memo, reserved.index());
    let built = handle_vtable_slots(input, m, trait_, host)?;
    Ok(reserved.fill(m, built))
}

/// The vtable [`handle_vtable`] reserves for `trait_`: built, and then
/// moved into the reserved index.
fn handle_vtable_slots(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    host: HandleHost<'_>,
) -> StageResult<usize> {
    let tp = (input.path)(trait_);
    let package = matches!(host, HandleHost::Profile(a) if a.package);
    let identity = package.then(|| (input.package_trait)(trait_)).flatten();
    let module = identity.as_ref().map(|i| package_module(i, true));
    let tname = module
        .as_deref()
        .and_then(|m| m.strip_prefix("hd:"))
        .unwrap_or_else(|| tp.rsplit(['.', '/']).next().unwrap_or(""))
        .to_owned();
    // A returned public capability keeps its package handle identity,
    // but only its wire methods/codecs cross the host boundary. The
    // collected library factory wraps this value after handle decoding.
    let key = input.pool.intern_ty(&TyData::TraitValue {
        def: trait_,
        args: TyList::EMPTY,
        bindings: vec![],
    });
    let bridge = input.env.host_bridge(key);
    // Constructing the ordinary adapter collects its vtable bodies even
    // when no public method is called. Those bodies alone must not grant
    // an unused returned package handle any wire methods.
    let enabled = !package
        || bridge.is_none()
        || input
            .env
            .trait_methods(trait_)
            .into_iter()
            .any(|method| input.collected.calls_dyn(method));
    // ruled out: a bridge's wire is a trait value (`ProgramEnv::host_bridge`).
    let trait_ = bridge.map_or(trait_, |b| input.pool.trait_value_parts(b.wire).0);
    let mut methods = Vec::new();
    let tree = HandleTree {
        input,
        target: trait_,
        key: &tname,
        host,
        enabled,
    };
    let vtable = tree.build(m, package.then_some(&mut methods))?;
    if let (Some(identity), Some(module)) = (identity, module) {
        record_package_interface(
            m,
            hd_host_abi::schema::Interface {
                identity,
                module,
                selectors: Vec::new(),
                handle: true,
                methods,
            },
        );
    }
    Ok(vtable)
}

/// The vtables of a host handle trait `target` and of every supertrait
/// instance it reaches, all importing from `key` through `host`.
struct HandleTree<'a, 'i> {
    input: &'a Input<'i>,
    target: DefId,
    key: &'a str,
    host: HandleHost<'a>,
    /// Whether called methods get host slots; otherwise every slot traps.
    enabled: bool,
}

/// A trait of a [`HandleTree`] whose vtable waits for its supertraits'.
struct HandleFrame {
    owner: DefId,
    /// The trait's arguments, `Self` first; `None` for the target.
    arguments: Option<TyList>,
    slots: Vec<Helper>,
    supers: Vec<(DefId, TyList)>,
    parents: Vec<usize>,
}

impl HandleTree<'_, '_> {
    /// The target's vtable. A package handle's called methods, the
    /// inherited ones under their wire names, go to `methods`.
    ///
    /// Cost: one visit per distinct supertrait instance the target
    /// reaches, each building its slots once; a diamond's shared ancestor
    /// is built once and its vtable shared. The walk is post-order over an
    /// explicit stack, as deep as the supertrait chain, so a deep chain
    /// does not deepen the native stack. `trait.super.acyclic` (`hd_resolve`)
    /// makes the walk finite.
    fn build(
        &self,
        m: &mut Module,
        mut methods: Option<&mut Vec<hd_host_abi::schema::Method>>,
    ) -> StageResult<usize> {
        let mut built: hd_base::HashMap<(DefId, TyList), usize> = hd_base::HashMap::default();
        let mut stack = Vec::new();
        let mut cur = self.frame(m, self.target, None, methods.as_deref_mut())?;
        loop {
            if let Some(&(def, args)) = cur.supers.get(cur.parents.len()) {
                if let Some(&v) = built.get(&(def, args)) {
                    cur.parents.push(v);
                } else {
                    let next = self.frame(m, def, Some(args), methods.as_deref_mut())?;
                    stack.push(std::mem::replace(&mut cur, next));
                }
                continue;
            }
            let v = crate::drive::vtable(
                self.input,
                m,
                cur.owner,
                std::mem::take(&mut cur.slots),
                std::mem::take(&mut cur.parents),
            )?;
            let Some(parent) = stack.pop() else {
                return Ok(v);
            };
            if let Some(args) = cur.arguments {
                built.insert((cur.owner, args), v);
            }
            cur = parent;
            cur.parents.push(v);
        }
    }

    /// `owner`'s slots, its called methods' package schemas, and its
    /// direct supertraits at `owner`'s `arguments`.
    fn frame(
        &self,
        m: &mut Module,
        owner: DefId,
        arguments: Option<TyList>,
        methods: Option<&mut Vec<hd_host_abi::schema::Method>>,
    ) -> StageResult<HandleFrame> {
        let input = self.input;
        let pool = input.pool;
        let slots = self.slots(m, owner, arguments)?;
        if let Some(methods) = methods {
            for method in input.env.trait_methods(owner) {
                if self.enabled && input.collected.calls_dyn(method) {
                    add_package_method(
                        methods,
                        package_method(input, method, self.target, owner, arguments)?,
                    );
                }
            }
        }
        let full = arguments.unwrap_or_else(|| pool.list(&[hd_mono::class_ref(pool)]));
        let supers = (input.env.decls().supertraits(owner).iter())
            .map(|&sup| {
                // ruled out: hd_resolve's supertrait lowering
                // (`Lower::supers`, through `Lower::trait_value`) makes
                // each supertrait a trait value over `Self`, and `subst`
                // keeps the constructor.
                let (def, args) = pool.trait_value_parts(subst(pool, input.env, owner, full, sup));
                let args = pool.list(
                    &std::iter::once(hd_mono::class_ref(pool))
                        .chain(pool.list_items(args).iter().copied())
                        .collect::<Vec<_>>(),
                );
                (def, args)
            })
            .collect();
        Ok(HandleFrame {
            owner,
            arguments,
            slots,
            supers,
            parents: Vec::new(),
        })
    }

    /// The slots of `owner`'s part of the handle's vtable, one per method.
    fn slots(
        &self,
        m: &mut Module,
        owner: DefId,
        arguments: Option<TyList>,
    ) -> StageResult<Vec<Helper>> {
        use hd_host_abi::{Codec, Scalar, Wait};
        let input = self.input;
        let (pool, env) = (input.pool, input.env);
        let mut slots = Vec::new();
        for method in env.trait_methods(owner) {
            let mp = (input.path)(method);
            let member = mp.rsplit(['.', '/']).next().unwrap_or("");
            let inner = match self.host {
                _ if !self.enabled || !input.collected.calls_dyn(method) => None,
                HandleHost::Abi(abi) => {
                    let full = format!("{}.{member}", self.key);
                    let hm = abi.methods.iter().find(|x| x.name == full);
                    // ruled out: hd_host_abi's
                    // table_mirrors_std_capability_traits test checks a
                    // row for each method a std handle trait and its
                    // supertraits declare.
                    debug_assert!(hm.is_some(), "the std host method `{full}` has no ABI row");
                    hm.map(|hm| host_slot(input, m, abi.key, &Row::of(hm), method, true, None))
                        .transpose()?
                }
                HandleHost::Profile(at) => {
                    let at = AtProfile { arguments, ..at };
                    let name = if at.package {
                        package_wire_name(input, self.target, owner, member)
                    } else {
                        member.to_owned()
                    };
                    let (params, ret) = signature(pool, env, method, arguments);
                    let mut codecs = vec![Codec::Scalar(Scalar::I32)];
                    codecs.extend(params.iter().map(|_| Codec::Buffer("")));
                    let row = Row {
                        name: &name,
                        params: &codecs,
                        result: result_codec(input, ret)?,
                        wait: if env.suspends(method) {
                            Wait::May
                        } else {
                            Wait::Never
                        },
                    };
                    Some(host_slot(input, m, self.key, &row, method, true, Some(at))?)
                }
            };
            // A method no reachable code calls links no host method.
            let Some(inner) = inner else {
                slots.push(uncalled_slot_at(input, method, arguments)?);
                continue;
            };
            let (params, ret) = signature(pool, env, method, arguments);
            let mut pv = Vec::new();
            for p in &params {
                pv.extend(layout::vals(pool, env, *p)?);
            }
            let results = if env.suspends(method) {
                vec![Val::I32]
            } else {
                layout::vals(pool, env, ret)?
            };
            slots.push(Helper::HandleSlot {
                inner: Box::new(inner),
                params: pv,
                ctx: crate::emit::slot_ctx(pool, env, method),
                results,
            });
        }
        Ok(slots)
    }
}

/// Adds `method` to a package host interface's `methods`, once: a trait
/// that two supertrait paths reach gives its methods twice.
pub fn add_package_method(
    methods: &mut Vec<hd_host_abi::schema::Method>,
    method: hd_host_abi::schema::Method,
) {
    if let Some(other) = methods.iter().find(|m| m.name == method.name) {
        // ruled out: module.host.package.one-instance (hd_check
        // header::boundary) rejects a host or handle trait that reaches one
        // trait at two argument lists, the one way two methods share a
        // wire name (trait path and member) with different signatures.
        debug_assert!(
            other == &method,
            "conflicting inherited package host signatures for `{}`",
            method.name
        );
    } else {
        methods.push(method);
    }
}

/// How a host call that never waits answers (§17.4, "Results"): nothing,
/// a scalar as the import's result, or a structured value in the buffer,
/// its length the import's result.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CallRes {
    Void,
    Scalar(Val),
    Buf(LTy),
}

impl CallRes {
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            CallRes::Void => vec![],
            CallRes::Scalar(v) => vec![*v],
            CallRes::Buf(b) => b.vals(),
        }
    }

    fn import(&self) -> Vec<Val> {
        match self {
            CallRes::Void => vec![],
            CallRes::Scalar(v) => vec![*v],
            CallRes::Buf(_) => vec![Val::I32],
        }
    }
}

/// Writes the arguments `args`, whose values `get(k)` pushes for value
/// index `k`, to the exchange buffer, and pushes the import's
/// parameters: the scalars in order, then the encoded bytes' length when
/// any argument is structured (§17.4, "Arguments"). Returns the import's
/// parameter types.
pub fn push_args(f: &mut Func, args: &[HostArg], get: &dyn Fn(&mut Func, u32)) -> Vec<Val> {
    let len = f.local(Val::I32);
    let mut structured = false;
    // Each scalar argument's first value index, for the import's
    // parameters after the buffer is written.
    let mut scalars = Vec::new();
    let mut k = 0;
    for a in args {
        let n = u32::try_from(a.vals().len()).expect("values");
        match a {
            HostArg::Str => {
                get(f, k);
                get(f, k + 1);
                f.helper(Helper::ToBuf);
                f.set(len);
                structured = true;
            }
            HostArg::Buf(b) => {
                f.get(len);
                for j in 0..n {
                    get(f, k + j);
                }
                f.helper(Helper::Enc(b.clone()));
                f.set(len);
                structured = true;
            }
            HostArg::Scalar(v) => scalars.push((k, *v)),
        }
        k += n;
    }
    let mut params = Vec::new();
    for (k, v) in scalars {
        get(f, k);
        params.push(v);
    }
    if structured {
        f.get(len);
        params.push(Val::I32);
    }
    params
}

/// [`Helper::HostCall`]: `(data, arguments) -> results`, one import call
/// of a host method that never waits.
pub fn host_call(
    m: &Module,
    module: &str,
    method: &str,
    args: &[HostArg],
    result: &CallRes,
) -> Func {
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut f = m.func(format!("rt:{module}/{method}"), params, result.vals());
    let imp = push_args(&mut f, args, &|f, k| f.get(k + 1));
    f.call(Sym::Import(Import::new(
        module,
        method,
        &imp,
        &result.import(),
    )));
    if let CallRes::Buf(b) = result {
        f.op(I::Drop);
        f.i32(0);
        f.helper(Helper::Dec(b.clone()));
        f.op(I::Drop);
    }
    f
}

/// [`Helper::HandleSlot`]: `(data, params, ctx?) -> results`, a host
/// handle's vtable slot: the handle number from the box at `data`, then
/// the inner host call with it first. A host method has no row, so the
/// slot ignores a context.
pub fn handle_slot(m: &Module, inner: &Helper, params: &[Val], ctx: bool, results: &[Val]) -> Func {
    let mut all = vec![Val::I32];
    all.extend(params.iter().copied());
    if ctx {
        all.push(Val::I32);
    }
    let mut f = m.func("rt:handle_slot", all, results.to_vec());
    f.get(0);
    f.get(0);
    f.op(I::I32Load(mem(HEAP, 0, 2)));
    for k in 1..=params.len() {
        f.get(u32::try_from(k).expect("params"));
    }
    f.helper(inner.clone());
    f
}

/// Pushes the exchange buffer's size in bytes.
fn buffer_size(f: &mut Func) {
    f.op(I::MemorySize(BUF));
    f.i32(16);
    f.op(I::I32Shl);
}

/// The `host-contract` panic of a result that does not decode.
fn malformed(f: &mut Func) {
    f.helper(Helper::Panic(
        "host-contract: a host result does not decode".into(),
    ));
}

/// Panics unless the `n` bytes (local `n`) from offset `off` lie inside
/// the buffer.
fn check_room(f: &mut Func, off: u32, n: u32) {
    f.get(n);
    buffer_size(f);
    f.get(off);
    f.op(I::I32Sub);
    f.op(I::I32GtU);
    f.op(I::If(BlockType::Empty));
    malformed(f);
    f.op(I::End);
}

/// [`Helper::Fit`]: `(end)`, grows the exchange buffer to at least `end`
/// bytes (§17.3); a refused growth is `heap-exhausted`.
pub fn fit(m: &Module) -> Func {
    let mut f = m.func("rt:fit", vec![Val::I32], vec![]);
    f.get(0);
    buffer_size(&mut f);
    f.op(I::I32GtU);
    f.op(I::If(BlockType::Empty));
    f.get(0);
    f.i32(0xffff);
    f.op(I::I32Add);
    f.i32(16);
    f.op(I::I32ShrU);
    f.op(I::MemorySize(BUF));
    f.op(I::I32Sub);
    f.op(I::MemoryGrow(BUF));
    f.i32(-1);
    f.op(I::I32Eq);
    f.op(I::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "heap-exhausted: the exchange buffer cannot grow".into(),
    ));
    f.op(I::End);
    f.op(I::End);
    f
}

/// [`Helper::LebPut`]: `(off, v: i64) -> off'`, `v` as unsigned LEB128.
pub fn leb_put(m: &Module) -> Func {
    let mut f = m.func("rt:leb_put", vec![Val::I32, Val::I64], vec![Val::I32]);
    let b = f.local(Val::I32);
    f.get(0);
    f.i32(10);
    f.op(I::I32Add);
    f.helper(Helper::Fit);
    f.op(I::Loop(BlockType::Empty));
    f.get(1);
    f.op(I::I32WrapI64);
    f.i32(127);
    f.op(I::I32And);
    f.set(b);
    f.get(1);
    f.op(I::I64Const(7));
    f.op(I::I64ShrU);
    f.set(1);
    f.get(1);
    f.op(I::I64Const(0));
    f.op(I::I64Ne);
    f.op(I::If(BlockType::Empty));
    f.get(b);
    f.i32(128);
    f.op(I::I32Or);
    f.set(b);
    f.op(I::End);
    f.get(0);
    f.get(b);
    f.op(I::I32Store8(mem(BUF, 0, 0)));
    f.get(0);
    f.i32(1);
    f.op(I::I32Add);
    f.set(0);
    f.get(1);
    f.op(I::I64Const(0));
    f.op(I::I64Ne);
    f.op(I::BrIf(0));
    f.op(I::End);
    f.get(0);
    f
}

/// [`Helper::LebGet`]: `(off) -> (v: i64, off')`, an unsigned LEB128
/// number; a byte past the buffer is `host-contract`.
pub fn leb_get(m: &Module) -> Func {
    let mut f = m.func("rt:leb_get", vec![Val::I32], vec![Val::I64, Val::I32]);
    let (r, sh, b) = (f.local(Val::I64), f.local(Val::I64), f.local(Val::I32));
    f.op(I::Loop(BlockType::Empty));
    f.get(0);
    buffer_size(&mut f);
    f.op(I::I32GeU);
    f.op(I::If(BlockType::Empty));
    malformed(&mut f);
    f.op(I::End);
    f.get(0);
    f.op(I::I32Load8U(mem(BUF, 0, 0)));
    f.set(b);
    f.get(0);
    f.i32(1);
    f.op(I::I32Add);
    f.set(0);
    f.get(r);
    f.get(b);
    f.i32(127);
    f.op(I::I32And);
    f.op(I::I64ExtendI32U);
    f.get(sh);
    f.op(I::I64Shl);
    f.op(I::I64Or);
    f.set(r);
    f.get(sh);
    f.op(I::I64Const(7));
    f.op(I::I64Add);
    f.set(sh);
    f.get(b);
    f.i32(128);
    f.op(I::I32And);
    f.op(I::BrIf(0));
    f.op(I::End);
    f.get(r);
    f.get(0);
    f
}

/// Reads a LEB128 length at local `off`, advances `off`, and leaves it on
/// the stack as an `i32`.
fn get_len(f: &mut Func, off: u32) {
    f.get(off);
    f.helper(Helper::LebGet);
    f.set(off);
    f.op(I::I32WrapI64);
}

/// Writes the `i64` that `push` pushes as LEB128 at local `off`, and
/// advances `off`.
fn put(f: &mut Func, off: u32, push: impl FnOnce(&mut Func)) {
    f.get(off);
    push(f);
    f.helper(Helper::LebPut);
    f.set(off);
}

/// Copies `n` bytes (local `n`) between the heap at `heap_at` (pushed by
/// `at`) and the buffer at local `off`, in the direction `out` says, then
/// advances `off`.
fn copy(f: &mut Func, off: u32, n: u32, out: bool, at: impl FnOnce(&mut Func)) {
    if out {
        f.get(off);
        f.get(n);
        f.op(I::I32Add);
        f.helper(Helper::Fit);
        f.get(off);
        at(f);
        f.get(n);
        f.push(crate::bulk::copy(HEAP, BUF));
    } else {
        at(f);
        f.get(off);
        f.get(n);
        f.push(crate::bulk::copy(BUF, HEAP));
    }
    f.get(off);
    f.get(n);
    f.op(I::I32Add);
    f.set(off);
}

/// Calls `b`'s encoder at local `off` over the values `get` pushes, and
/// advances `off`.
fn enc_call(f: &mut Func, off: u32, b: &LTy, get: impl FnOnce(&mut Func)) {
    if b.is_empty() {
        return;
    }
    f.get(off);
    get(f);
    f.helper(Helper::Enc(b.clone()));
    f.set(off);
}

/// Calls `b`'s decoder at local `off`, advances `off`, and stores the
/// values in fresh locals, which it returns.
fn dec_into(f: &mut Func, off: u32, b: &LTy) -> Vec<u32> {
    if b.is_empty() {
        return Vec::new();
    }
    f.get(off);
    f.helper(Helper::Dec(b.clone()));
    f.set(off);
    let ls: Vec<u32> = b.vals().into_iter().map(|v| f.local(v)).collect();
    for l in ls.iter().rev() {
        f.set(*l);
    }
    ls
}

/// One count less on the values in locals `ls`, laid out with counted
/// references at `walk`, when the program counts.
fn release(f: &mut Func, m: &Module, ls: &[u32], vals: Vec<Val>, walk: Option<Walk>) {
    let (Some(_), Some(walk)) = (m.runtime, walk) else {
        return;
    };
    for l in ls {
        f.get(*l);
    }
    f.helper(Helper::Release { vals, walk });
}

/// [`Helper::Enc`]: the encoder of `b`, `(off, values...) -> off'`.
/// Its value's type was built for this direction (`of`), so a codec
/// value has its encoder and no handle is in it.
pub fn enc(b: &LTy, m: &Module) -> Func {
    let mut params = vec![Val::I32];
    params.extend(b.vals());
    let mut f = m.func("rt:enc", params, vec![Val::I32]);
    enc_body(&mut f, b, m);
    f
}

/// [`enc`]'s body for `b`; a named type's is its definition's.
fn enc_body(f: &mut Func, b: &LTy, m: &Module) {
    // ruled out: module.boundary.not-safe.error (hd_check header::boundary)
    // rejects a trait value passed to the host, and `never` anywhere but a
    // host method's whole result, so no encoder meets a handle or `never`.
    debug_assert!(
        !matches!(b, LTy::Handle(_) | LTy::Never(_)),
        "an encoder of a host handle or of `never`"
    );
    match b {
        LTy::Void | LTy::Handle(_) | LTy::Never(_) => {}
        LTy::Int { bits, signed } => {
            let x = f.local(Val::I64);
            f.get(1);
            if *bits <= 32 {
                f.op(if *signed {
                    I::I64ExtendI32S
                } else {
                    I::I64ExtendI32U
                });
            }
            f.set(x);
            put(f, 0, |f| {
                f.get(x);
                if *signed {
                    f.op(I::I64Const(1));
                    f.op(I::I64Shl);
                    f.get(x);
                    f.op(I::I64Const(63));
                    f.op(I::I64ShrS);
                    f.op(I::I64Xor);
                }
            });
        }
        LTy::Char => put(f, 0, |f| {
            f.get(1);
            f.op(I::I64ExtendI32U);
        }),
        LTy::F32 | LTy::F64 => {
            let n = if *b == LTy::F64 { 8 } else { 4 };
            f.get(0);
            f.i32(n);
            f.op(I::I32Add);
            f.helper(Helper::Fit);
            f.get(0);
            f.get(1);
            f.op(if n == 8 {
                I::F64Store(mem(BUF, 0, 0))
            } else {
                I::F32Store(mem(BUF, 0, 0))
            });
            f.get(0);
            f.i32(n);
            f.op(I::I32Add);
            f.set(0);
        }
        LTy::Str | LTy::DiscardStr => {
            put(f, 0, |f| {
                f.get(2);
                f.op(I::I64ExtendI32U);
            });
            copy(f, 0, 2, true, |f| f.get(1));
        }
        LTy::Bytes => {
            let (buf, n) = (1, f.local(Val::I32));
            f.get(buf);
            f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            f.set(n);
            put(f, 0, |f| {
                f.get(n);
                f.op(I::I64ExtendI32U);
            });
            copy(f, 0, n, true, |f| {
                f.get(buf);
                f.u32(LIST_ELEMS);
                f.op(I::I32Add);
            });
        }
        LTy::List(l) => {
            let (at, n, i) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
            f.get(1);
            f.set(at);
            f.get(at);
            f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            f.set(n);
            put(f, 0, |f| {
                f.get(n);
                f.op(I::I64ExtendI32U);
            });
            f.get(at);
            f.u32(LIST_ELEMS);
            f.op(I::I32Add);
            f.set(at);
            f.op(I::Block(BlockType::Empty));
            f.op(I::Loop(BlockType::Empty));
            f.get(i);
            f.get(n);
            f.op(I::I32GeU);
            f.op(I::BrIf(1));
            enc_call(f, 0, &l.ty, |f| {
                for leaf in &l.elem.leaves {
                    f.get(at);
                    f.op(leaf.load(0));
                }
            });
            f.get(at);
            f.u32(l.elem.size);
            f.op(I::I32Add);
            f.set(at);
            f.get(i);
            f.i32(1);
            f.op(I::I32Add);
            f.set(i);
            f.op(I::Br(0));
            f.op(I::End);
            f.op(I::End);
        }
        LTy::Record(fields) => {
            let mut params = 1..;
            for fb in fields {
                let own: Vec<u32> = params.by_ref().take(fb.vals().len()).collect();
                enc_call(f, 0, fb, |f| {
                    for k in own {
                        f.get(k);
                    }
                });
            }
        }
        LTy::Map(mp) => {
            let t = f.local(Val::I32);
            f.get(1);
            f.set(t);
            put(f, 0, |f| {
                f.get(t);
                f.op(I::I32Load(mem(HEAP, crate::map::LIVE, 2)));
                f.op(I::I64ExtendI32U);
            });
            let (key, val) = mp.entry.leaves.split_at(mp.entry.keys);
            crate::map::each_entry(f, t, mp.entry.size, &|f, at| {
                for (ty, leaves) in [(&mp.key, key), (&mp.val, val)] {
                    enc_call(f, 0, ty, |f| {
                        for leaf in leaves {
                            f.get(at);
                            f.op(leaf.load(0));
                        }
                    });
                }
            });
        }
        LTy::Enum(e) => {
            let flat = &e.layout;
            // The host format is the variant's index (`read_discr`), then
            // its payload, under a test of the discriminant cell.
            put(f, 0, |f| {
                layout::read_discr(f, flat.disc, &|f, k| f.get(1 + k));
                f.op(I::I64ExtendI32U);
            });
            for (v, fields) in (0u32..).zip(&e.variants) {
                if fields.iter().all(|fb| fb.vals().is_empty()) {
                    continue;
                }
                let test = match flat.test(v) {
                    layout::Test::Const(false) | layout::Test::Read(_, layout::Is::Never) => {
                        continue;
                    }
                    layout::Test::Const(true) => false,
                    layout::Test::Read(d, is) => {
                        f.get(1 + d.value);
                        for op in is.ops() {
                            f.op(op);
                        }
                        f.op(I::If(BlockType::Empty));
                        true
                    }
                };
                for (fi, fb) in fields.iter().enumerate() {
                    enc_call(f, 0, fb, |f| {
                        for &k in &flat.positions[v as usize][fi] {
                            f.get(1 + k);
                        }
                    });
                }
                if test {
                    f.op(I::End);
                }
            }
        }
        LTy::Codec(c) => {
            let text = [f.local(Val::I32), f.local(Val::I32)];
            // The encoder borrows the value; the codec owns its copy.
            let get = |f: &mut Func| {
                for k in (1..).take(c.vals.len()) {
                    f.get(k);
                }
            };
            if m.runtime.is_some() {
                own_arg(f, &c.vals, c.walk.as_ref(), &get);
            }
            get(f);
            f.call(Sym::Inst(c.codec));
            f.set(text[1]);
            f.set(text[0]);
            enc_call(f, 0, &LTy::Str, |f| {
                f.get(text[0]);
                f.get(text[1]);
            });
            release(f, m, &text, vec![Val::I32, Val::I32], Some(Walk::Ref(0)));
        }
        LTy::Object(o) => {
            // The count word, marked while the object is on the path.
            let count = f.local(Val::I32);
            if o.tracked {
                f.get(1);
                f.u32(crate::module::HEADER);
                f.op(I::I32Sub);
                f.set(count);
                f.get(count);
                f.op(I::I32Load(mem(HEAP, 0, 2)));
                f.u32(ON_PATH);
                f.op(I::I32And);
                f.op(I::If(BlockType::Empty));
                f.helper(Helper::Panic(
                    "boundary-cycle: a value reaches itself".into(),
                ));
                f.op(I::End);
                on_path(f, count, I::I32Add);
            }
            enc_call(f, 0, &o.ty, |f| {
                for leaf in &o.elem.leaves {
                    f.get(1);
                    f.op(leaf.load(0));
                }
            });
            if o.tracked {
                on_path(f, count, I::I32Sub);
            }
        }
        LTy::Named(n) => return enc_body(f, m.named_codecs.def(n.id), m),
    }
    f.get(0);
}

/// Adds [`ON_PATH`] to the count word at local `count`, or takes it off,
/// by `op`.
fn on_path(f: &mut Func, count: u32, op: I) {
    f.get(count);
    f.get(count);
    f.op(I::I32Load(mem(HEAP, 0, 2)));
    f.u32(ON_PATH);
    f.op(op);
    f.op(I::I32Store(mem(HEAP, 0, 2)));
}

/// [`Helper::Dec`]: the decoder of `b`, `(off) -> (values..., off')`.
/// Its value's type was built for this direction (`of`), so a codec
/// value has its decoder, and a list is built only when the program links
/// std's allocator.
pub fn dec(b: &LTy, m: &mut Module) -> Func {
    let mut results = b.vals();
    results.push(Val::I32);
    let mut f = m.func("rt:dec", vec![Val::I32], results);
    dec_body(&mut f, b, m);
    f
}

/// [`dec`]'s body for `b`; a named type's is its definition's.
fn dec_body(f: &mut Func, b: &LTy, m: &mut Module) {
    match b {
        LTy::Void => {}
        // A host that answers a call that never returns broke its contract
        // (`module.boundary.never-result`).
        LTy::Never(label) => {
            // The panic's call ends with `unreachable` (`Func::helper`).
            f.helper(Helper::Panic(format!(
                "host-contract: {label} answered, but it never returns"
            )));
            return;
        }
        LTy::Int { bits, signed } => {
            let x = f.local(Val::I64);
            f.get(0);
            f.helper(Helper::LebGet);
            f.set(0);
            f.set(x);
            if *signed {
                f.get(x);
                f.op(I::I64Const(1));
                f.op(I::I64ShrU);
                f.op(I::I64Const(0));
                f.get(x);
                f.op(I::I64Const(1));
                f.op(I::I64And);
                f.op(I::I64Sub);
                f.op(I::I64Xor);
                f.set(x);
            }
            if *bits < 64 {
                let rest = i64::from(64 - *bits);
                f.get(x);
                if *signed {
                    f.op(I::I64Const(rest));
                    f.op(I::I64Shl);
                    f.op(I::I64Const(rest));
                    f.op(I::I64ShrS);
                    f.get(x);
                    f.op(I::I64Ne);
                } else {
                    f.op(I::I64Const(i64::from(*bits)));
                    f.op(I::I64ShrU);
                    f.op(I::I64Const(0));
                    f.op(I::I64Ne);
                }
                f.op(I::If(BlockType::Empty));
                f.helper(Helper::Panic(
                    "host-contract: a host result is out of range".into(),
                ));
                f.op(I::End);
            }
            f.get(x);
            if *bits <= 32 {
                f.op(I::I32WrapI64);
            }
        }
        LTy::Char => {
            let scalar = f.local(Val::I64);
            f.get(0);
            f.helper(Helper::LebGet);
            f.set(0);
            f.set(scalar);
            // module.profile.host-result.char: reject values above Unicode's
            // maximum or in its surrogate gap, before any i32 narrowing.
            f.get(scalar);
            f.op(I::I64Const(0x10_ffff));
            f.op(I::I64GtU);
            f.get(scalar);
            f.op(I::I64Const(0xd800));
            f.op(I::I64Sub);
            f.op(I::I64Const(0x7ff));
            f.op(I::I64LeU);
            f.op(I::I32Or);
            f.op(I::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "host-contract: a host character result is not a Unicode scalar".into(),
            ));
            f.op(I::End);
            f.get(scalar);
            f.op(I::I32WrapI64);
        }
        LTy::F32 | LTy::F64 => {
            let n = if *b == LTy::F64 { 8 } else { 4 };
            let size = f.local(Val::I32);
            f.i32(n);
            f.set(size);
            check_room(f, 0, size);
            f.get(0);
            f.op(if n == 8 {
                I::F64Load(mem(BUF, 0, 0))
            } else {
                I::F32Load(mem(BUF, 0, 0))
            });
            f.get(0);
            f.i32(n);
            f.op(I::I32Add);
            f.set(0);
        }
        LTy::DiscardStr => {
            let n = f.local(Val::I32);
            get_len(f, 0);
            f.set(n);
            check_room(f, 0, n);
            f.get(0);
            f.get(n);
            f.op(I::I32Add);
            f.set(0);
            f.u32(EMPTY);
            f.i32(0);
        }
        LTy::Str => {
            let (n, s) = (f.local(Val::I32), f.local(Val::I32));
            get_len(f, 0);
            f.set(n);
            check_room(f, 0, n);
            // Every empty string is the one static view (§23.7).
            f.u32(EMPTY);
            f.set(s);
            f.get(n);
            f.op(I::If(BlockType::Empty));
            f.get(n);
            f.helper(Helper::Str);
            f.set(s);
            copy(f, 0, n, false, |f| f.get(s));
            f.op(I::End);
            f.get(s);
            f.get(n);
        }
        LTy::Bytes => {
            let (n, buf) = (f.local(Val::I32), f.local(Val::I32));
            get_len(f, 0);
            f.set(n);
            check_room(f, 0, n);
            f.get(n);
            f.helper(Helper::ListBuf { elem: byte_elem() });
            f.set(buf);
            f.get(buf);
            f.get(n);
            f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
            copy(f, 0, n, false, |f| {
                f.get(buf);
                f.u32(LIST_ELEMS);
                f.op(I::I32Add);
            });
            f.get(buf);
        }
        LTy::List(l) => {
            let (n, buf, at, i) = (
                f.local(Val::I32),
                f.local(Val::I32),
                f.local(Val::I32),
                f.local(Val::I32),
            );
            get_len(f, 0);
            f.set(n);
            // Every element takes at least one byte.
            check_room(f, 0, n);
            f.get(n);
            f.helper(Helper::ListBuf {
                elem: l.elem.clone(),
            });
            f.set(buf);
            f.get(buf);
            f.u32(LIST_ELEMS);
            f.op(I::I32Add);
            f.set(at);
            f.op(I::Block(BlockType::Empty));
            f.op(I::Loop(BlockType::Empty));
            f.get(i);
            f.get(n);
            f.op(I::I32GeU);
            f.op(I::BrIf(1));
            let vals = dec_into(f, 0, &l.ty);
            for (leaf, v) in l.elem.leaves.iter().zip(&vals) {
                f.get(at);
                f.get(*v);
                f.op(leaf.store(0));
            }
            // The buffer's length counts the element only once it is
            // stored, so its glue never releases an unwritten one.
            f.get(buf);
            f.get(i);
            f.i32(1);
            f.op(I::I32Add);
            f.set(i);
            f.get(i);
            f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
            f.get(at);
            f.u32(l.elem.size);
            f.op(I::I32Add);
            f.set(at);
            f.op(I::Br(0));
            f.op(I::End);
            f.op(I::End);
            f.get(buf);
        }
        LTy::Map(mp) => dec_map(f, mp),
        LTy::Object(o) => {
            // The referent's values, each with its count, move into a new
            // object, as `box` makes one (`emit::value`'s `object_drops`).
            let vals = dec_into(f, 0, &o.ty);
            let obj = f.local(Val::I32);
            f.u32(o.elem.size);
            let glue = o.elem.walk.as_ref().map(|w| {
                Sym::Helper(Helper::DropFields {
                    leaves: o.elem.leaves.clone(),
                    walk: w.clone(),
                    drops: Vec::new(),
                })
            });
            if let Some(key) = o.weak {
                crate::rt::push_typed_desc_recursive(f, glue, o.elem.cyclic, o.recursive, key);
            } else {
                crate::rt::push_desc_recursive(f, glue, o.elem.cyclic, o.recursive);
            }
            if m.runtime.is_some_and(|r| r.checked.is_none())
                && o.elem
                    .size
                    .checked_add(crate::module::HEADER)
                    .is_some_and(|n| n <= 2048)
            {
                f.helper(Helper::NewFixed { size: o.elem.size });
            } else {
                f.helper(Helper::New);
            }
            f.set(obj);
            for (leaf, v) in o.elem.leaves.iter().zip(&vals) {
                f.get(obj);
                f.get(*v);
                f.op(leaf.store(0));
            }
            f.get(obj);
        }
        LTy::Record(fields) => {
            let mut all = Vec::new();
            for fb in fields {
                all.extend(dec_into(f, 0, fb));
            }
            for l in all {
                f.get(l);
            }
        }
        LTy::Enum(e) => {
            let flat = &e.layout;
            let tag = f.local(Val::I32);
            let res: Vec<u32> = flat.mem.leaves.iter().map(|l| f.local(l.val)).collect();
            get_len(f, 0);
            f.set(tag);
            f.get(tag);
            f.i32(i32::try_from(e.variants.len()).expect("variants"));
            f.op(I::I32GeU);
            f.op(I::If(BlockType::Empty));
            malformed(f);
            f.op(I::End);
            // A tag is the index itself, set once; a variant whose values
            // are then all set or zero needs no more. Any other builds its
            // values (`write_discr` and its payload) under a test of the
            // index; a value it leaves zero stays zero, as its local starts.
            let tag_value = flat
                .cell()
                .filter(|d| d.enc == layout::Enc::Direct)
                .map(|d| d.value as usize);
            if let Some(k) = tag_value {
                f.get(tag);
                f.set(res[k]);
            }
            for (v, fields) in (0u32..).zip(&e.variants) {
                let values = layout::variant_steps(flat, v);
                let built = |c: usize| values[c].is_some() && tag_value != Some(c);
                if !(0..values.len()).any(built) {
                    continue;
                }
                f.get(tag);
                f.u32(v);
                f.op(I::I32Eq);
                f.op(I::If(BlockType::Empty));
                let ls: Vec<Vec<u32>> = fields.iter().map(|fb| dec_into(f, 0, fb)).collect();
                let leaf = |f: &mut Func, fi: usize, k: usize| f.get(ls[fi][k]);
                for (c, (steps, r)) in values.iter().zip(&res).enumerate() {
                    if let Some(steps) = steps
                        && built(c)
                    {
                        layout::run_steps(f, steps, &leaf);
                        f.set(*r);
                    }
                }
                f.op(I::End);
            }
            for r in res {
                f.get(r);
            }
        }
        LTy::Handle(h) => {
            let (number, data) = (f.local(Val::I32), f.local(Val::I32));
            get_len(f, 0);
            f.set(number);
            // The number owns one host binding. Aliases count this box;
            // its last release drops the binding, never an I/O resource.
            f.i32(4);
            crate::rt::push_desc(
                f,
                h.release.as_ref().map(|module| {
                    Sym::Helper(Helper::PackageHandleRelease {
                        module: module.clone(),
                    })
                }),
                false,
            );
            f.helper(Helper::New);
            f.set(data);
            f.get(data);
            f.get(number);
            f.op(I::I32Store(mem(HEAP, 0, 2)));
            f.get(data);
            f.push(Ins::VTable(h.vtable));
            if let Some(wrap) = h.wrap {
                // The reference arrives with a count of its own; the box's
                // first goes. `wrap` owns its argument (`layout::Pass`),
                // so it takes a count of the box of its own.
                if m.runtime.is_some() {
                    own_arg(f, &[Val::I32], Some(&Walk::Ref(0)), &|f| f.get(data));
                }
                f.call(Sym::Inst(wrap));
                release(f, m, &[data], vec![Val::I32], Some(Walk::Ref(0)));
            }
        }
        LTy::Codec(c) => {
            // The decoded string's count goes to the codec, which owns its
            // argument (`layout::Pass`); a value that does not deserialize
            // is its `host-contract` panic (`module.boundary.deserialize.exact`).
            let text = dec_into(f, 0, &LTy::Str);
            for l in text {
                f.get(l);
            }
            f.call(Sym::Inst(c.codec));
        }
        LTy::Named(n) => {
            let def = m.named_codecs.def(n.id).clone();
            return dec_body(f, &def, m);
        }
    }
    f.get(0);
}

/// [`dec_body`] of a map: its count, then each entry put into a table
/// sized for them all, so no put grows it. Each key hashes and probes
/// through the operations collection chose, so a repeated key's later
/// value replaces the earlier one in its place, as in a literal
/// (`module.boundary.map-decode`, `expr.map.duplicate.last`).
///
/// Cost: one hash, one probe and one put per entry; the table is
/// allocated once.
fn dec_map(f: &mut Func, mp: &LMap) {
    let e = &mp.entry;
    let (n, t, cap, i, h, probe) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    get_len(f, 0);
    f.set(n);
    // Every entry takes at least one byte.
    check_room(f, 0, n);
    // The capacity: the least power of two of at least `n` entries, and
    // at least 4, as a literal's (`1 << (32 - clz(n - 1))`; Wasm shifts
    // by the count mod 32, so 0 and 1 entries give 1).
    f.i32(1);
    f.i32(32);
    f.get(n);
    f.i32(1);
    f.op(I::I32Sub);
    f.op(I::I32Clz);
    f.op(I::I32Sub);
    f.op(I::I32Shl);
    f.set(cap);
    f.get(cap);
    f.i32(4);
    f.get(cap);
    f.i32(4);
    f.op(I::I32GtU);
    f.op(I::Select);
    f.helper(Helper::MapTable { entry: e.clone() });
    f.set(t);
    f.op(I::Block(BlockType::Empty));
    f.op(I::Loop(BlockType::Empty));
    f.get(i);
    f.get(n);
    f.op(I::I32GeU);
    f.op(I::BrIf(1));
    let key = dec_into(f, 0, &mp.key);
    let val = dec_into(f, 0, &mp.val);
    for l in &key {
        f.get(*l);
    }
    f.helper(Helper::MapHash {
        key: e.key_vals(),
        ops: mp.ops.clone(),
    });
    f.set(h);
    f.get(t);
    f.get(h);
    for l in &key {
        f.get(*l);
    }
    f.helper(Helper::MapFind {
        entry: e.clone(),
        ops: mp.ops.clone(),
    });
    f.set(probe);
    // The table takes the key's and the value's counts as they stand.
    f.get(t);
    f.get(h);
    f.get(probe);
    for l in key.iter().chain(&val) {
        f.get(*l);
    }
    f.helper(Helper::MapPut {
        entry: e.clone(),
        ops: mp.ops.clone(),
    });
    f.set(t);
    f.get(i);
    f.i32(1);
    f.op(I::I32Add);
    f.set(i);
    f.op(I::Br(0));
    f.op(I::End);
    f.op(I::End);
    f.get(t);
}

/// The element of a `List[u8]`.
fn byte_elem() -> Elem {
    Elem {
        size: 1,
        leaves: vec![Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 1,
            signed: false,
        }],
        walk: None,
        drops: Vec::new(),
        colls: Vec::new(),
        cyclic: false,
    }
}

/// Length-delimited full nominal identity, independent of dependency aliases.
/// Role separates a default provider from a returned handle of the same trait.
pub fn package_module(identity: &hd_host_abi::schema::Identity, handle: bool) -> String {
    let mut h = hd_base::StableHasher::new("package-host-wire");
    h.u32(hd_host_abi::schema::VERSION);
    h.str(&identity.package);
    h.str(&identity.version);
    h.str(&identity.trait_path);
    h.u8(u8::from(handle));
    format!("hd:pkg:{:032x}", h.finish().0)
}

/// Records a package host interface once per module. Its module names its
/// trait's identity and role (`package_module`), and the trait, its
/// called methods and their signatures decide the rest, so a second record
/// of one module is the same interface.
pub fn record_package_interface(m: &mut Module, interface: hd_host_abi::schema::Interface) {
    if let Some(previous) = m
        .package_contract
        .interfaces
        .iter()
        .find(|i| i.module == interface.module)
    {
        // ruled out: rt::profile_vtable records a provider's interface
        // once per key, and `handle_vtable` a handle's once per trait,
        // each from the trait's own declaration.
        debug_assert!(
            previous == &interface,
            "two package host interfaces for one module"
        );
    } else {
        m.package_contract.interfaces.push(interface);
    }
}

/// The wire name of method `name` of `owner`, a supertrait of the package
/// host trait `target` or `target` itself: its own name, or an inherited
/// one qualified by its trait's package identity.
pub fn package_wire_name(input: &Input<'_>, target: DefId, owner: DefId, name: &str) -> String {
    if target == owner {
        return name.to_owned();
    }
    let identity = (input.package_trait)(owner);
    // ruled out: hd_cli's dependency resolution (disk.rs `dependencies`)
    // gives each package of a build its `HostPackage`, and hd_driver's
    // `package_trait_identity` gives std's traits the ABI identity, so a
    // package host's supertrait has an identity.
    debug_assert!(
        identity.is_some(),
        "a supertrait without a package identity"
    );
    match identity {
        Some(id) => format!("{}@{}/{}::{name}", id.package, id.version, id.trait_path),
        None => format!("{}::{name}", (input.path)(owner).replace('/', ".")),
    }
}

pub fn package_method(
    input: &Input<'_>,
    method: DefId,
    target: DefId,
    owner: DefId,
    arguments: Option<TyList>,
) -> StageResult<hd_host_abi::schema::Method> {
    let (params, ret) = signature(input.pool, input.env, method, arguments);
    let path = (input.path)(method);
    Ok(hd_host_abi::schema::Method {
        name: package_wire_name(
            input,
            target,
            owner,
            path.rsplit(['.', '/']).next().unwrap_or(""),
        ),
        member: path.rsplit(['.', '/']).next().unwrap_or("").to_owned(),
        params: params
            .into_iter()
            .map(|t| package_value(input, t, &mut Binders::new()))
            .collect::<StageResult<_>>()?,
        result: if never(input, ret) {
            hd_host_abi::schema::Value::Never
        } else {
            package_value(input, ret, &mut Binders::new())?
        },
        suspends: input.env.suspends(method),
    })
}

/// The package schema of `t`. A struct or enum that one of its parts
/// reaches again is recursive: it is a `Rec` binder, numbered on `path`,
/// and the part a `Ref` to it (#1550). Cost: one scan of the path of
/// nominal types being built per nominal type reached.
fn package_value(
    input: &Input<'_>,
    t: Ty,
    path: &mut Binders,
) -> StageResult<hd_host_abi::schema::Value> {
    use hd_host_abi::schema::Value;
    let (pool, env) = (input.pool, input.env);
    let s = layout::strip(pool, env, t);
    if let TyData::Adt { def, .. } = pool.get(s)
        && env.has_private_field(def)
    {
        return Ok(Value::Json);
    }
    let int = |bits, signed| Value::Int { bits, signed };
    Ok(match pool.get(s) {
        TyData::Prim(p) => match p {
            Prim::Bool => Value::Bool,
            Prim::Char => Value::Char,
            Prim::I8 => int(8, true),
            Prim::I16 => int(16, true),
            Prim::I32 => int(32, true),
            Prim::I64 => int(64, true),
            Prim::U8 => int(8, false),
            Prim::U16 => int(16, false),
            Prim::U32 | Prim::Usize => int(32, false),
            Prim::U64 => int(64, false),
            Prim::F32 => Value::F32,
            Prim::F64 => Value::F64,
            Prim::String => Value::String,
        },
        TyData::Tuple { .. } => {
            let ts = layout::tuple_members(pool, s)
                .into_iter()
                .map(|t| package_value(input, t, path))
                .collect::<StageResult<Vec<_>>>()?;
            if ts.is_empty() {
                Value::Void
            } else {
                Value::Tuple(ts)
            }
        }
        TyData::Adt { .. } if let Some(inner) = pool.sole_arg(t, env.option()) => {
            Value::Option(Box::new(package_value(input, inner, path)?))
        }
        TyData::Adt { def, args } if env.std_kind(def) == hd_mono::layout::StdKind::List => {
            let items = pool.list_items(args);
            debug_assert_eq!(items.len(), 1, "ruled out: `List` has one element type");
            let elem = items[0];
            if pool.get(layout::strip(pool, env, elem)) == TyData::Prim(Prim::U8) {
                Value::Bytes
            } else {
                Value::List(Box::new(package_value(input, elem, path)?))
            }
        }
        TyData::Adt { def, args } if env.std_kind(def) == hd_mono::layout::StdKind::Map => {
            let items = pool.list_items(args);
            debug_assert_eq!(
                items.len(),
                2,
                "ruled out: `Map` has a key and a value type"
            );
            Value::Map(
                Box::new(package_value(input, items[0], path)?),
                Box::new(package_value(input, items[1], path)?),
            )
        }
        TyData::Adt { def, args } => {
            if let Some(at) = path.iter().position(|(t, _)| *t == s) {
                // Each binder on the path has its own number: the count
                // of binders before it.
                let next = path.iter().map(|(_, id)| u32::from(id.is_some())).sum();
                let id = *path[at].1.get_or_insert(next);
                return Ok(Value::Ref(id));
            }
            path.push((s, None));
            let built = package_data(input, def, args, path);
            // The entry pushed above, with its number if a part reached it.
            let id = path.pop().and_then(|(_, id)| id);
            let built = built?;
            return Ok(match id {
                Some(id) => Value::Rec(id, Box::new(built)),
                None => built,
            });
        }
        // A reference crosses as its referent: a counted one as its tree
        // (`module.boundary.tree`), a returned trait value reference as its
        // handle.
        TyData::Ref { inner, .. } => return package_value(input, inner, path),
        TyData::TraitValue { def, args, .. } => {
            // ruled out: module.host.package.not-generic (hd_check
            // header::boundary) rejects a returned handle trait with type
            // parameters.
            debug_assert_eq!(args, TyList::EMPTY, "a generic package host handle trait");
            let identity = (input.package_trait)(def).expect(
                "ruled out: module.boundary.not-safe.error (hd_check header::boundary) rejects \
                 a package host's returned trait value without a package identity",
            );
            Value::Handle(package_module(&identity, true))
        }

        // A union crosses as the enum of its cases, `_0`, `_1`, ..., each
        // with its one positional payload `0`, as a nominal enum with one
        // single-payload variant per case would (`types.union.representation`).
        TyData::Union(members) => Value::Enum(
            pool.list_items(members)
                .iter()
                .enumerate()
                .map(|(i, &case)| {
                    Ok(hd_host_abi::schema::Variant {
                        name: format!("_{i}"),
                        fields: vec![hd_host_abi::schema::Field {
                            name: "0".to_owned(),
                            value: package_value(input, case, path)?,
                        }],
                    })
                })
                .collect::<StageResult<_>>()?,
        ),
        // ruled out: `package_method` takes a whole `never` result first,
        // and module.boundary.not-safe.error (hd_check
        // header::crossing_fault) rejects `never` anywhere else, a
        // function and a provider context (`module.boundary.rows`);
        // `signature` and `subst` leave no parameter, projection or opaque
        // type in a checked host method's signature (module.host.package.no-assoc
        // rejects a host trait's associated type); a bare trait value is no
        // value type; a row is no value type, and checking solves every
        // inference variable.
        TyData::Never
        | TyData::Poison
        | TyData::Fn { .. }
        | TyData::Opaque { .. }
        | TyData::Param(_)
        | TyData::Const(_)
        | TyData::Assoc { .. }
        | TyData::Row(_)
        | TyData::Context(_)
        | TyData::Infer(_)
        | TyData::Canon(_) => {
            debug_assert!(false, "{:?} in a package host schema", pool.get(t));
            Value::Void
        }
    })
}

/// [`package_value`]'s struct or enum instance `def[args]`.
///
/// ruled out, for each length check: `hd_driver`'s `enum_variants` and
/// `enum_variant_names`, and its `data_fields` and `data_field_names`,
/// read one declaration, so they agree. A newtype, the one data type with
/// fields but no field names, is its base here (`layout::strip`).
fn package_data(
    input: &Input<'_>,
    def: DefId,
    args: TyList,
    path: &mut Binders,
) -> StageResult<hd_host_abi::schema::Value> {
    use hd_host_abi::schema::{Field, Value, Variant};
    let (pool, env) = (input.pool, input.env);
    Ok({
        {
            if let Some(variants) = env.enum_variants(def, args) {
                if (input.path)(def).replace('/', ".") == "std.core.Result"
                    && variants.len() == 2
                    && variants.iter().all(|v| v.len() == 1)
                {
                    Value::Result(
                        Box::new(package_value(input, variants[0][0], path)?),
                        Box::new(package_value(input, variants[1][0], path)?),
                    )
                } else {
                    let names = env.enum_variant_names(def).unwrap_or_default();
                    debug_assert_eq!(names.len(), variants.len(), "an enum's variant names");
                    let mut out = Vec::new();
                    for ((name, fields), types) in names.into_iter().zip(variants) {
                        debug_assert_eq!(fields.len(), types.len(), "a variant's field names");
                        out.push(Variant {
                            name,
                            fields: fields
                                .into_iter()
                                .zip(types)
                                .map(|(name, ty)| {
                                    Ok(Field {
                                        name,
                                        value: package_value(input, ty, path)?,
                                    })
                                })
                                .collect::<StageResult<_>>()?,
                        });
                    }
                    Value::Enum(out)
                }
            } else {
                let types = env.data_fields(def).unwrap_or_default();
                let names = env.data_field_names(def).unwrap_or_default();
                debug_assert_eq!(names.len(), types.len(), "a struct's field names");
                Value::Record(
                    names
                        .into_iter()
                        .zip(types)
                        .map(|(name, ty)| {
                            Ok(Field {
                                name,
                                value: package_value(input, subst(pool, env, def, args, ty), path)?,
                            })
                        })
                        .collect::<StageResult<_>>()?,
                )
            }
        }
    })
}

#[cfg(test)]
mod discarded_tests {
    use super::*;
    use hd_base::{NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Place};

    fn record(b: &mut Body, words: &[u32]) -> u32 {
        let at = u32::try_from(b.extra.len()).expect("record");
        b.extra.push(u32::try_from(words.len()).expect("length"));
        b.extra.extend_from_slice(words);
        at
    }

    fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
        let i = u32::try_from(b.len()).expect("instruction");
        b.tags.push(tag);
        b.data.push([a, w]);
        b.ty.push(Ty::VOID);
        b.syn.push(NodeIdx::NONE);
        i
    }

    fn place(b: &mut Body, text: &str) -> u32 {
        let p: Place = text.parse().expect("place");
        match (p.words(), p.root) {
            (Some(words), _) => record(b, &words),
            (None, PlaceRoot::Local(l)) => l.raw() | hd_tir::ir::place::BARE_LOCAL,
            _ => unreachable!("place words"),
        }
    }

    fn body() -> Body {
        let mut b = Body::new(DefId::from_raw(0), BodyKind::Fn);
        b.local_ty.push(Ty::VOID);
        b.local_name.push(Symbol::from_raw(0));
        b.local_syn.push(NodeIdx::NONE);
        b.local_flags.push(local_flags::ASSIGNED);
        inst(&mut b, Tag::Hole, NONE, NONE);
        b
    }

    fn finish(b: &mut Body, tail: u32) {
        let list = record(
            b,
            &(0..u32::try_from(b.len()).expect("body")).collect::<Vec<_>>(),
        );
        let root = inst(b, Tag::Block, list, tail);
        b.sub_root.push(root);
    }

    #[test]
    fn tag_tests_copies_and_drops_do_not_observe_payload_bytes() {
        let mut b = body();
        let destination = place(&mut b, "[$0]");
        inst(&mut b, Tag::Store, destination, 0);
        let source = place(&mut b, "[$0 as .0 .0]");
        let read = inst(&mut b, Tag::Read, source, 0);
        let branches = record(&mut b, &[0, NONE, NONE]);
        inst(&mut b, Tag::SwitchTag, read, branches);
        inst(&mut b, Tag::Drop, destination, NONE);
        finish(&mut b, NONE);
        assert!(tags_only(&b, 0));
    }

    #[test]
    fn references_captures_returns_and_calls_keep_owned_decoding() {
        for use_ in [Tag::Ref, Tag::Return, Tag::Call] {
            let mut b = body();
            let p = place(&mut b, "[%0]");
            match use_ {
                Tag::Ref => {
                    inst(&mut b, Tag::Ref, p, 0);
                }
                Tag::Return => {
                    inst(&mut b, Tag::Return, 0, NONE);
                }
                Tag::Call => {
                    let args = record(&mut b, &[0, 0, 0, 0]);
                    inst(&mut b, Tag::Call, NONE, args);
                }
                _ => unreachable!(),
            }
            finish(&mut b, NONE);
            assert!(!tags_only(&b, 0), "{use_:?}");
        }
        let mut b = body();
        let p = place(&mut b, "[$0]");
        inst(&mut b, Tag::Store, p, 0);
        b.local_flags[0] |= local_flags::CAPTURED;
        finish(&mut b, NONE);
        assert!(!tags_only(&b, 0));
        let mut b = body();
        finish(&mut b, 0);
        assert!(!tags_only(&b, 0));
    }

    #[test]
    fn a_payload_used_as_an_element_key_is_an_observation() {
        let mut b = body();
        let other = inst(&mut b, Tag::Hole, NONE, NONE);
        let destination = place(&mut b, &format!("[%{other} [%0]]"));
        inst(&mut b, Tag::Store, destination, other);
        finish(&mut b, NONE);
        assert!(!tags_only(&b, 0));
    }

    #[test]
    fn dynamic_function_values_are_not_callee_records() {
        for value in [Ref::konst(0).0, 0, 1, NONE] {
            let mut b = body();
            let call = inst(&mut b, Tag::CallValue, value, NONE);
            assert_eq!(dynamic_method(&b, call), None);
        }
        let mut b = body();
        for row in [Ref::CONST_BIT, NONE, 0] {
            let call = inst(&mut b, Tag::Call, row, NONE);
            assert_eq!(dynamic_method(&b, call), None);
        }
        let method = DefId::from_raw(19);
        let callee = Callee::TraitMethod {
            trait_: DefId::from_raw(7),
            method,
            self_ty: Ty::VOID,
            targs: TyList::EMPTY,
            choice: (hd_tir::ir::ChoiceKind::TraitValue, 0),
        };
        let row = record(&mut b, &callee.words());
        for tag in [Tag::Call, Tag::Await] {
            let call = inst(&mut b, tag, row, NONE);
            assert_eq!(dynamic_method(&b, call), Some(method));
        }
    }

    fn result_type() -> LTy {
        let string = crate::layout::Mem::string;
        let error_layout = crate::layout::flat_of(&[vec![], vec![string()]]);
        let outer = crate::layout::flat_of(&[vec![], vec![error_layout.mem.clone()]]);
        let error = LTy::Enum(Box::new(LEnum {
            layout: Box::new(error_layout),
            variants: vec![vec![], vec![LTy::Str]],
        }));
        LTy::Enum(Box::new(LEnum {
            layout: Box::new(outer),
            variants: vec![vec![], vec![error]],
        }))
    }

    /// Walk the emitted helper graph separately from unrelated host frame edges.
    #[test]
    fn discarded_error_payload_decoder_has_no_owned_string_allocation_edge() {
        fn reaches_allocation(ty: LTy) -> bool {
            let mut module = Module::default();
            let mut pending = vec![Helper::Dec(ty)];
            let mut seen = std::collections::BTreeSet::new();
            while let Some(helper) = pending.pop() {
                if !seen.insert(helper.clone()) {
                    continue;
                }
                if matches!(helper, Helper::Str | Helper::New | Helper::NewFixed { .. }) {
                    return true;
                }
                let body = crate::rt::helper(&helper, &mut module);
                for instruction in &body.body {
                    if let Ins::Call(symbol) = instruction
                        && let Sym::Helper(callee) = &*body.symbols.get(*symbol)
                    {
                        pending.push(callee.clone());
                    }
                }
            }
            false
        }
        let owned = result_type();
        assert!(reaches_allocation(owned.clone()));
        let mut discarded = owned;
        discard_strings(&mut discarded);
        assert!(!reaches_allocation(discarded));
    }

    #[test]
    fn discarded_and_owned_decoder_keys_are_distinct_and_validate_the_same_wire() {
        let owned = result_type();
        let mut discarded = owned.clone();
        discard_strings(&mut discarded);
        assert_ne!(owned, discarded);
        assert_eq!(owned.vals(), discarded.vals());
        let mut m = Module::default();
        m.export("owned", Sym::Helper(Helper::Dec(owned)));
        m.export("discarded", Sym::Helper(Helper::Dec(discarded)));
        let mut byte = m.func("byte", vec![Val::I32], vec![Val::I32]);
        byte.get(0);
        byte.op(I::I32Load8U(mem(HEAP, 0, 0)));
        m.define(Sym::Inst(Hash128(77)), byte);
        m.export("byte", Sym::Inst(Hash128(77)));
        let wasm = m.finish().expect("decoder module");
        let script = format!(
            r"
const bytes = new Uint8Array({wasm:?});
const module = new WebAssembly.Module(bytes);
let panics = 0;
const imports = {{}};
for (const i of WebAssembly.Module.imports(module)) {{
  imports[i.module] ??= {{}};
  imports[i.module][i.name] = () => {{ if (i.name === 'panic') panics++; return 0; }};
}}
const e = new WebAssembly.Instance(module, imports).exports;
// The result is `string??`'s values (the outer tag, then the inner
// optional's string, whose non-null address says `.Some`) and the bytes read.
const buffer = new Uint8Array(e['hd.x'].buffer);
const text = 'failure λ 🦀';
const utf8 = new TextEncoder().encode(text);
const reply = new Uint8Array([1, 1, utf8.length, ...utf8]);
buffer.set(reply);
const full = e.owned(0);
if (full[0] !== 1 || full[1] === 0 || full[2] !== utf8.length || full[3] !== reply.length) throw Error('owned result shape');
buffer.fill(90);
const kept = new Uint8Array(Array.from({{length:full[2]}}, (_, i) => e.byte(full[1] + i)));
if (new TextDecoder().decode(kept) !== text) throw Error('owned error lost after another host buffer write');
buffer.set(reply);
const skipped = e.discarded(0);
if (skipped[0] !== 1 || skipped[1] === 0 || skipped[2] !== 0 || skipped[3] !== reply.length) throw Error('discarded result shape');
// Invalid outer/inner tags and a string length exceeding the exchange memory.
for (const bad of [[2], [1,2], [1,1,128,128,8]]) {{
  for (const decode of [e.owned, e.discarded]) {{
    buffer.fill(0); buffer.set(bad);
    const before = panics;
    let failed = false;
    try {{ decode(0); }} catch {{ failed = true; }}
    if (!failed || panics !== before + 1) throw Error('malformed reply was accepted');
  }}
}}
console.log('owned and discarded boundary decoders passed');
"
        );
        let output = std::process::Command::new("node")
            .args(["--input-type=module", "-e", &script])
            .output()
            .expect("Node available");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "owned and discarded boundary decoders passed"
        );
    }
}
