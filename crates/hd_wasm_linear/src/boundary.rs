//! Host codecs over linear memory (`crates/hd_wasm_linear/DESIGN.md` §23.23):
//! the encoders and decoders of the values that cross the exchange buffer
//! (`crates/hd_run_wasmtime/DESIGN.md` §17.4), generated per boundary type
//! from its linear layout, and the host method slots that call them.
//!
//! The boundary bytes follow the host ABI: an encoder reads a value from its Wasm values (and, for a list, from
//! its buffer in the heap) and writes it to memory 0; a decoder reads
//! memory 0 and builds the value's Wasm values, allocating its strings and
//! lists in the heap with their counts. Strings and byte lists cross with
//! one `memory.copy` between the two memories.
//!
//! Each encoder is `(off, values...) -> off'`: it writes the value at
//! `off`, growing the buffer first, and returns the end. Each decoder is
//! `(off) -> (values..., off')`: it reads the value at `off`, checking
//! every length against the buffer's size, and returns its values, each
//! with a count of its own, and the end. A malformed or out-of-range
//! result is the `host-contract` panic.

use hd_base::{DefId, Hash128, StageResult};
use hd_mono::{BoundaryMap, ProgramEnv, subst};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};
use wasm_encoder::{BlockType, Instruction as I};

use crate::layout::{self, Leaf, Walk};
use crate::module::{BUF, EMPTY, Func, HEAP, Import, Ins, Module, Sym, Val, mem};
use crate::rt::{Elem, Helper, LIST_ELEMS, LIST_LEN, own_arg};
use crate::task::{HostArg, HostRes};
use crate::{Input, unsupported};

/// How deep boundary types may nest: a recursive type does not cross
/// (`module.boundary.cycle`).
const MAX_DEPTH: u32 = 32;

/// A boundary type on this backend: its structure, and what its codecs
/// need of its linear layout.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LTy {
    /// `void`: no bytes, no values.
    Void,
    /// An integer, `bool` (one bit) or `char` (21 bits): LEB128, zigzag
    /// when signed; an `i64` past 32 bits, else an `i32`.
    Int {
        bits: u8,
        signed: bool,
    },
    /// Raw IEEE 754 little-endian bytes.
    F32,
    F64,
    /// A string `(address, length)`: its length, then its bytes.
    Str,
    /// `List[u8]`, a header's address: its length, then its bytes.
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
    /// boundary: its JSON text (`module.boundary.serialize.exact`).
    Json(Box<LJson>),
}

/// A list's element: its layout in the buffer and its boundary type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LList {
    pub elem: Elem,
    pub ty: LTy,
}

/// An enum's values (the tag, then the shared slots, `layout::enum_mem`)
/// and, per variant, each payload field's value indices and type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LEnum {
    pub vals: Vec<Val>,
    pub variants: Vec<Vec<(Vec<u32>, LTy)>>,
}

/// A host handle's trait value: a box holding the handle number, beside
/// the static vtable `vtable` of host calls that take the number first.
/// With `wrap`, the `std.rt.host_provider` instance that puts it behind
/// a trait value reference (`std-net.handles.ref`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LHandle {
    pub vtable: usize,
    pub wrap: Option<Hash128>,
}

/// A private-field data type's JSON codecs: `encode` when the value
/// leaves hd; `decode`, its `Result`'s `is_ok` and `expect` when it
/// enters, with that `Result`'s values and counted references.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LJson {
    pub vals: Vec<Val>,
    /// The value's counted references, which `encode` owns
    /// (`layout::Pass`): the encoder counts a copy for it.
    pub walk: Option<Walk>,
    pub encode: Option<Hash128>,
    pub decode: Option<[Hash128; 3]>,
    pub decoded: Vec<Val>,
    pub decoded_walk: Option<Walk>,
}

impl LTy {
    /// The type's Wasm values (`layout::vals`).
    #[must_use]
    pub fn vals(&self) -> Vec<Val> {
        match self {
            LTy::Void => vec![],
            LTy::Int { bits, .. } => vec![if *bits > 32 { Val::I64 } else { Val::I32 }],
            LTy::F32 => vec![Val::F32],
            LTy::F64 => vec![Val::F64],
            LTy::Str | LTy::Handle(_) => vec![Val::I32, Val::I32],
            LTy::Bytes | LTy::List(_) => vec![Val::I32],
            LTy::Record(fields) => fields.iter().flat_map(LTy::vals).collect(),
            LTy::Enum(e) => e.vals.clone(),
            LTy::Json(j) => j.vals.clone(),
        }
    }

    /// Whether a host handle is in the value: it only crosses from the
    /// host.
    #[must_use]
    pub fn holds_handle(&self) -> bool {
        match self {
            LTy::Handle(_) => true,
            LTy::List(l) => l.ty.holds_handle(),
            LTy::Record(fields) => fields.iter().any(LTy::holds_handle),
            LTy::Enum(e) => e.variants.iter().flatten().any(|(_, t)| t.holds_handle()),
            _ => false,
        }
    }

    /// An enum whose payloads are at most one payload-free enum each, as
    /// `Result[void, ConsoleError]`: its encoding is one or two variant
    /// indices, which a host leaf reads as bytes (`HostRes::Tags`).
    #[must_use]
    pub fn is_tags(&self) -> bool {
        let LTy::Enum(e) = self else {
            return false;
        };
        e.variants.len() < 128
            && e.variants.iter().all(|v| match v.as_slice() {
                [] | [(_, LTy::Void)] => true,
                [(_, LTy::Enum(p))] => {
                    p.variants.len() < 128 && p.variants.iter().all(Vec::is_empty)
                }
                _ => false,
            })
    }
}

/// A runtime profile's boundary: the JSON codecs its private-field types
/// cross through, whether the value leaves hd, and the profile handle
/// traits whose slots enclose it, innermost last.
#[derive(Clone, Copy)]
pub struct AtProfile<'a> {
    pub codecs: &'a BoundaryMap,
    pub out: bool,
    pub handles: &'a [DefId],
}

/// The boundary type of `t`, or `unsupported` for a type that does not
/// cross (a function, another trait value, a map, a recursive type).
pub fn of(input: &Input<'_>, m: &mut Module, t: Ty, at: Option<AtProfile<'_>>) -> StageResult<LTy> {
    of_at(input, m, t, 0, at)
}

fn of_at(
    input: &Input<'_>,
    m: &mut Module,
    t: Ty,
    depth: u32,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let what = || format!("`{}` at the host boundary", pool.display(t));
    if depth > MAX_DEPTH {
        return unsupported(what());
    }
    let unmut = unmut(pool, t);
    let s = layout::strip(pool, env, t);
    // A data type with a field that is not `pub` crosses a profile's
    // boundary as its JSON text, through the codecs collection gathered.
    if let (TyData::Adt { def, .. }, Some(at)) = (pool.get(s), at)
        && def != env.ref_cell()
        && env.has_private_field(def)
    {
        return json(input, unmut, s, at);
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
            Prim::Char => int(21, false),
            Prim::String => LTy::Str,
        },
        TyData::Tuple { elems, rest: None } => {
            let mut fields = Vec::new();
            for e in pool.list_items(elems) {
                fields.push(of_at(input, m, *e, depth + 1, at)?);
            }
            // `void` is the empty tuple (`types.void`): it crosses as no
            // value.
            if fields.is_empty() {
                LTy::Void
            } else {
                LTy::Record(fields)
            }
        }
        TyData::Option(inner) => enum_of(input, m, &[vec![], vec![inner]], depth, at)?,
        TyData::Adt { def, args } if def == env.ref_cell() => {
            // A trait value reference to a host handle: the handle,
            // behind the reference the `host_provider` instance makes.
            let inner = pool.list_items(args).first().copied().unwrap_or(Ty::VOID);
            let inner = layout::strip(pool, env, inner);
            let wrap = input
                .collected
                .boundary
                .get(&unmut)
                .and_then(|c| c.handle_ref);
            match (of_at(input, m, inner, depth + 1, at)?, wrap) {
                (LTy::Handle(mut h), Some(wrap)) => {
                    h.wrap = Some(wrap);
                    LTy::Handle(h)
                }
                _ => return unsupported(what()),
            }
        }
        TyData::Adt { def, args } => match env.std_kind(def) {
            hd_mono::layout::StdKind::List => {
                let Some(&e) = pool.list_items(args).first() else {
                    return unsupported(what());
                };
                if input.runtime.is_none() {
                    return unsupported("lists without std's allocator");
                }
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
                        colls: layout::coll_leaves(pool, env, e),
                    };
                    let ty = of_at(input, m, e, depth + 1, at)?;
                    LTy::List(Box::new(LList { elem, ty }))
                }
            }
            hd_mono::layout::StdKind::Other => match layout::kind(pool, env, s) {
                layout::Kind::Value => {
                    if let Some(variants) = env.enum_variants(def, args) {
                        enum_of(input, m, &variants, depth, at)?
                    } else {
                        let fields: Vec<Ty> = env
                            .data_fields(def)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|f| subst(pool, env, def, args, f))
                            .collect();
                        if fields.is_empty() {
                            return unsupported(what());
                        }
                        let mut out = Vec::new();
                        for f in fields {
                            out.push(of_at(input, m, f, depth + 1, at)?);
                        }
                        LTy::Record(out)
                    }
                }
                _ => return unsupported(what()),
            },
            _ => return unsupported(what()),
        },
        TyData::TraitValue { def, args, .. } if args == TyList::EMPTY => {
            let path = (input.path)(def).replace('/', ".");
            let vtable = if let Some(host) = hd_host_abi::handle_trait(&path) {
                handle_vtable(input, m, def, Some(host), None)?
            } else if let Some(at) = at
                && !at.out
                && !path.starts_with("std.")
            {
                // A value of a program's own trait that a profile's host
                // method returns can only be a handle the profile made.
                // Its slots' own results are built here too, so a trait
                // that one of its methods returns cannot cross.
                if at.handles.contains(&def) {
                    return unsupported(format!(
                        "the host handle trait `{path}`, which a method of its own returns"
                    ));
                }
                let handles = [at.handles, &[def]].concat();
                let inner = AtProfile {
                    handles: &handles,
                    ..at
                };
                handle_vtable(input, m, def, None, Some(inner))?
            } else {
                return unsupported(what());
            };
            LTy::Handle(Box::new(LHandle { vtable, wrap: None }))
        }
        _ => return unsupported(what()),
    })
}

/// The boundary type of an enum of `variants`, laid out as
/// `layout::enum_mem` lays it out.
fn enum_of(
    input: &Input<'_>,
    m: &mut Module,
    variants: &[Vec<Ty>],
    depth: u32,
    at: Option<AtProfile<'_>>,
) -> StageResult<LTy> {
    let (mem, positions) = layout::enum_mem(input.pool, input.env, variants)?;
    let mut vs = Vec::new();
    for (fields, ps) in variants.iter().zip(positions) {
        let mut v = Vec::new();
        for (f, p) in fields.iter().zip(ps) {
            let ty = of_at(input, m, *f, depth + 1, at)?;
            let p = p
                .into_iter()
                .map(|k| u32::try_from(k).expect("values"))
                .collect();
            v.push((p, ty));
        }
        vs.push(v);
    }
    Ok(LTy::Enum(Box::new(LEnum {
        vals: mem.vals(),
        variants: vs,
    })))
}

/// A private-field type `t` (stripped `s`) at a profile's boundary.
fn json(input: &Input<'_>, t: Ty, s: Ty, at: AtProfile<'_>) -> StageResult<LTy> {
    let (pool, env) = (input.pool, input.env);
    let name = || pool.display(s);
    let Some(codecs) = at.codecs.get(&t).or_else(|| at.codecs.get(&s)) else {
        return unsupported(format!(
            "`{}` at a profile's boundary without a collected JSON codec",
            name()
        ));
    };
    let vals = layout::vals(pool, env, s)?;
    let walk = match layout::pass(pool, env, input.runtime.is_some(), s) {
        layout::Pass::Owned(w) => Some(w),
        _ => None,
    };
    if at.out {
        let Some(encode) = codecs.encode else {
            return unsupported(format!(
                "`{}` at a profile's boundary without its Serialize codec",
                name()
            ));
        };
        return Ok(LTy::Json(Box::new(LJson {
            vals,
            walk,
            encode: Some(encode),
            decode: None,
            decoded: Vec::new(),
            decoded_walk: None,
        })));
    }
    let (Some(decode), Some(decoded)) = (codecs.decode, codecs.decoded) else {
        return unsupported(format!(
            "`{}` at a profile's boundary without its Deserialize codec",
            name()
        ));
    };
    Ok(LTy::Json(Box::new(LJson {
        vals,
        walk,
        encode: None,
        decode: Some([decode.decode, decode.is_ok, decode.expect]),
        decoded: layout::vals(pool, env, decoded)?,
        decoded_walk: input.runtime.and_then(|_| layout::walk(pool, env, decoded)),
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

/// `t` without its `mut`.
fn unmut(pool: &InternPool, t: Ty) -> Ty {
    let mut t = t;
    while let TyData::Mut(inner) = pool.get(t) {
        t = inner;
    }
    t
}

/// The method `m`'s parameter types after the receiver and its result
/// type, at the trait value (`Self` the class `REF`).
fn signature(pool: &InternPool, env: &dyn ProgramEnv, m: DefId) -> (Vec<Ty>, Ty) {
    let full = pool.list(&[hd_mono::class_ref(pool)]);
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
    let (pool, env) = (input.pool, input.env);
    let (params, ret) = signature(pool, env, m);
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
/// At a profile's boundary `at`, private-field types cross as JSON.
pub fn host_slot(
    input: &Input<'_>,
    mm: &mut Module,
    key: &str,
    row: &Row<'_>,
    m: DefId,
    handle: bool,
    at: Option<AtProfile<'_>>,
) -> StageResult<Helper> {
    use hd_host_abi::{Codec, Wait};
    let (pool, env) = (input.pool, input.env);
    let refused = || unsupported(format!("the host method `{key}.{}`", row.name));
    let waits = row.wait == Wait::May;
    if env.suspends(m) != waits {
        return refused();
    }
    let (params, ret) = signature(pool, env, m);
    let mut args = Vec::new();
    let mut types = Vec::new();
    let codecs = if handle {
        args.push(HostArg::Scalar(Val::I32));
        types.push(Ty::I32);
        row.params.get(1..).unwrap_or_default()
    } else {
        row.params
    };
    if params.len() != codecs.len() {
        return refused();
    }
    for (pt, c) in params.iter().zip(codecs) {
        let vals = layout::vals(pool, env, *pt)?;
        args.push(match (c, vals.as_slice()) {
            // A lone string crosses as its bytes alone (§17.4).
            // A newtype of a string, as `Path`, is not one: it crosses with
            // its length.
            (Codec::Buffer(_), _) if row.params.len() == 1 && unmut(pool, *pt) == Ty::STRING => {
                HostArg::Str
            }
            (Codec::Buffer(_), _) => {
                let b = of(input, mm, *pt, at.map(|a| AtProfile { out: true, ..a }))?;
                if b.holds_handle() {
                    return unsupported("a host handle passed to the host");
                }
                HostArg::Buf(b)
            }
            (Codec::Scalar(_), [v @ (Val::I32 | Val::I64 | Val::F64)]) => HostArg::Scalar(*v),
            _ => return refused(),
        });
        types.push(*pt);
    }
    let rvals = layout::vals(pool, env, ret)?;
    let module = format!("hd:{key}");
    let method = row.name.to_owned();
    if !waits {
        let result = match (row.result, rvals.as_slice()) {
            (Codec::Void, []) => CallRes::Void,
            (Codec::Scalar(_), [v @ (Val::I32 | Val::I64 | Val::F64)]) => CallRes::Scalar(*v),
            (Codec::Buffer(_), _) => CallRes::Buf(of(
                input,
                mm,
                ret,
                at.map(|a| AtProfile { out: false, ..a }),
            )?),
            _ => return refused(),
        };
        return Ok(Helper::HostCall {
            module,
            method,
            args,
            result,
        });
    }
    let res = match (row.result, rvals.as_slice()) {
        (Codec::Void, []) => HostRes::Void,
        (Codec::Buffer(_), _) => {
            let b = of(input, mm, ret, at.map(|a| AtProfile { out: false, ..a }))?;
            if b.is_tags() && rvals == [Val::I32, Val::I32] {
                HostRes::Tags
            } else {
                HostRes::Buf(b)
            }
        }
        _ => return refused(),
    };
    Ok(Helper::HostCold {
        module,
        method,
        args,
        area: args_area(input, &types)?,
        res,
    })
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
        colls: if counts { colls } else { Vec::new() },
    })
}

/// The vtable of host handle trait `trait_`: each slot a
/// [`Helper::HandleSlot`] over its host call, then the vtables of its
/// direct supertraits, whose methods are the handle trait's host methods
/// too, as `TcpStream.dispose` (flow.using.disposable-ref). A trait in
/// `hd_host_abi::HANDLES` names its methods `<Trait>.<method>` in the
/// capability's module; a runtime profile's own trait imports them from
/// `hd:<Trait>` by name, with every argument and result in the buffer.
fn handle_vtable(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    host: Option<&hd_host_abi::HostTrait>,
    at: Option<AtProfile<'_>>,
) -> StageResult<usize> {
    let tp = (input.path)(trait_);
    let tname = tp.rsplit(['.', '/']).next().unwrap_or("").to_owned();
    let supers: Vec<Ty> = input.env.decls().supertraits(trait_).to_vec();
    if !supers.is_empty() && host.is_none() {
        return unsupported(format!(
            "the host handle trait `{tp}`, which has supertraits"
        ));
    }
    let mut parents = Vec::new();
    for s in supers {
        let TyData::TraitValue { def, .. } = input.pool.get(s) else {
            return unsupported("a supertrait that is not a trait");
        };
        if !input.env.decls().supertraits(def).is_empty() {
            return unsupported(format!(
                "the host handle trait `{tp}`, whose supertrait has supertraits"
            ));
        }
        let slots = handle_slots(input, m, def, &tname, host, at)?;
        parents.push(m.vtable(slots));
    }
    let slots = handle_slots(input, m, trait_, &tname, host, at)?;
    Ok(m.vtable_with(slots, parents))
}

/// The slots of trait `trait_`'s vtable for a host handle of trait
/// `tname` ([`handle_vtable`]).
fn handle_slots(
    input: &Input<'_>,
    m: &mut Module,
    trait_: DefId,
    tname: &str,
    host: Option<&hd_host_abi::HostTrait>,
    at: Option<AtProfile<'_>>,
) -> StageResult<Vec<Sym>> {
    use hd_host_abi::{Codec, Scalar, Wait};
    let (pool, env) = (input.pool, input.env);
    let mut slots = Vec::new();
    for method in env.trait_methods(trait_) {
        if !input.collected.calls_dyn(method) {
            slots.push(Sym::Helper(uncalled_slot(input, method)?));
            continue;
        }
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("");
        let inner = match (host, at) {
            (Some(host), _) => {
                let full = format!("{tname}.{name}");
                let Some(hm) = host.methods.iter().find(|x| x.name == full) else {
                    return unsupported(format!("the host method `{full}`"));
                };
                host_slot(input, m, host.key, &Row::of(hm), method, true, None)?
            }
            (None, Some(at)) => {
                let (params, ret) = signature(pool, env, method);
                let mut codecs = vec![Codec::Scalar(Scalar::I32)];
                codecs.extend(params.iter().map(|_| Codec::Buffer("")));
                let row = Row {
                    name,
                    params: &codecs,
                    result: if layout::vals(pool, env, ret)?.is_empty() {
                        Codec::Void
                    } else {
                        Codec::Buffer("")
                    },
                    wait: if env.suspends(method) {
                        Wait::May
                    } else {
                        Wait::Never
                    },
                };
                host_slot(input, m, tname, &row, method, true, Some(at))?
            }
            (None, None) => return unsupported(format!("the host handle trait `{tname}`")),
        };
        let (params, ret) = signature(pool, env, method);
        let mut pv = Vec::new();
        for p in &params {
            pv.extend(layout::vals(pool, env, *p)?);
        }
        let results = if env.suspends(method) {
            vec![Val::I32]
        } else {
            layout::vals(pool, env, ret)?
        };
        slots.push(Sym::Helper(Helper::HandleSlot {
            inner: Box::new(inner),
            params: pv,
            results,
        }));
    }
    Ok(slots)
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
            HostArg::Scalar(_) => {}
        }
        k += n;
    }
    let mut params = Vec::new();
    let mut k = 0;
    for a in args {
        if let HostArg::Scalar(v) = a {
            get(f, k);
            params.push(*v);
        }
        k += u32::try_from(a.vals().len()).expect("values");
    }
    if structured {
        f.get(len);
        params.push(Val::I32);
    }
    params
}

/// [`Helper::HostCall`]: `(data, arguments) -> results`, one import call
/// of a host method that never waits.
pub fn host_call(module: &str, method: &str, args: &[HostArg], result: &CallRes) -> Func {
    let mut params = vec![Val::I32];
    params.extend(args.iter().flat_map(HostArg::vals));
    let mut f = Func::new(format!("rt:{module}/{method}"), params, result.vals());
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

/// [`Helper::HandleSlot`]: `(data, params) -> results`, a host handle's
/// vtable slot: the handle number from the box at `data`, then the inner
/// host call with it first.
pub fn handle_slot(inner: &Helper, params: &[Val], results: &[Val]) -> Func {
    let mut all = vec![Val::I32];
    all.extend(params.iter().copied());
    let mut f = Func::new("rt:handle_slot", all, results.to_vec());
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
pub fn fit() -> Func {
    let mut f = Func::new("rt:fit", vec![Val::I32], vec![]);
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
pub fn leb_put() -> Func {
    let mut f = Func::new("rt:leb_put", vec![Val::I32, Val::I64], vec![Val::I32]);
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
pub fn leb_get() -> Func {
    let mut f = Func::new("rt:leb_get", vec![Val::I32], vec![Val::I64, Val::I32]);
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
        f.op(I::MemoryCopy {
            src_mem: HEAP,
            dst_mem: BUF,
        });
    } else {
        at(f);
        f.get(off);
        f.get(n);
        f.op(I::MemoryCopy {
            src_mem: BUF,
            dst_mem: HEAP,
        });
    }
    f.get(off);
    f.get(n);
    f.op(I::I32Add);
    f.set(off);
}

/// Calls `b`'s encoder at local `off` over the values `get` pushes, and
/// advances `off`.
fn enc_call(f: &mut Func, off: u32, b: &LTy, get: impl FnOnce(&mut Func)) {
    f.get(off);
    get(f);
    f.helper(Helper::Enc(b.clone()));
    f.set(off);
}

/// Calls `b`'s decoder at local `off`, advances `off`, and stores the
/// values in fresh locals, which it returns.
fn dec_into(f: &mut Func, off: u32, b: &LTy) -> Vec<u32> {
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
/// Its value's type was built for this direction (`of`), so a JSON
/// value has its `Serialize` codec and no handle is in it.
pub fn enc(b: &LTy, m: &Module) -> Func {
    let mut params = vec![Val::I32];
    params.extend(b.vals());
    let mut f = Func::new("rt:enc", params, vec![Val::I32]);
    match b {
        LTy::Void => {}
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
            put(&mut f, 0, |f| {
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
        LTy::Str => {
            put(&mut f, 0, |f| {
                f.get(2);
                f.op(I::I64ExtendI32U);
            });
            copy(&mut f, 0, 2, true, |f| f.get(1));
        }
        LTy::Bytes => {
            let (buf, n) = (f.local(Val::I32), f.local(Val::I32));
            f.get(1);
            f.op(I::I32Load(mem(HEAP, 0, 2)));
            f.set(buf);
            f.get(buf);
            f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            f.set(n);
            put(&mut f, 0, |f| {
                f.get(n);
                f.op(I::I64ExtendI32U);
            });
            copy(&mut f, 0, n, true, |f| {
                f.get(buf);
                f.u32(LIST_ELEMS);
                f.op(I::I32Add);
            });
        }
        LTy::List(l) => {
            let (at, n, i) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
            f.get(1);
            f.op(I::I32Load(mem(HEAP, 0, 2)));
            f.set(at);
            f.get(at);
            f.op(I::I32Load(mem(HEAP, LIST_LEN, 2)));
            f.set(n);
            put(&mut f, 0, |f| {
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
            enc_call(&mut f, 0, &l.ty, |f| {
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
            let mut first = 1;
            for fb in fields {
                let n = u32::try_from(fb.vals().len()).expect("values");
                enc_call(&mut f, 0, fb, |f| {
                    for k in first..first + n {
                        f.get(k);
                    }
                });
                first += n;
            }
        }
        LTy::Enum(e) => {
            put(&mut f, 0, |f| {
                f.get(1);
                f.op(I::I64ExtendI32U);
            });
            for (v, fields) in e.variants.iter().enumerate() {
                if fields.iter().all(|(ps, _)| ps.is_empty()) {
                    continue;
                }
                f.get(1);
                f.i32(i32::try_from(v).expect("variants"));
                f.op(I::I32Eq);
                f.op(I::If(BlockType::Empty));
                for (ps, fb) in fields {
                    enc_call(&mut f, 0, fb, |f| {
                        for p in ps {
                            f.get(1 + p);
                        }
                    });
                }
                f.op(I::End);
            }
        }
        LTy::Json(j) => {
            let Some(encode) = j.encode else {
                f.op(I::Unreachable);
                return f;
            };
            let text = [f.local(Val::I32), f.local(Val::I32)];
            // The encoder borrows the value; `encode` owns its copy.
            let n = u32::try_from(j.vals.len()).expect("values");
            let get = |f: &mut Func| {
                for k in 0..n {
                    f.get(1 + k);
                }
            };
            if m.runtime.is_some() {
                own_arg(&mut f, &j.vals, j.walk.as_ref(), &get);
            }
            get(&mut f);
            f.call(Sym::Inst(encode));
            f.set(text[1]);
            f.set(text[0]);
            enc_call(&mut f, 0, &LTy::Str, |f| {
                f.get(text[0]);
                f.get(text[1]);
            });
            release(
                &mut f,
                m,
                &text,
                vec![Val::I32, Val::I32],
                Some(Walk::Ref(0)),
            );
        }
        LTy::Handle(_) => {
            f.op(I::Unreachable);
            return f;
        }
    }
    f.get(0);
    f
}

/// [`Helper::Dec`]: the decoder of `b`, `(off) -> (values..., off')`.
/// Its value's type was built for this direction (`of`), so a JSON
/// value has its `Deserialize` codecs, and a list is built only when the
/// program links std's allocator.
pub fn dec(b: &LTy, m: &mut Module) -> Func {
    let mut results = b.vals();
    results.push(Val::I32);
    let mut f = Func::new("rt:dec", vec![Val::I32], results);
    match b {
        LTy::Void => {}
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
        LTy::F32 | LTy::F64 => {
            let n = if *b == LTy::F64 { 8 } else { 4 };
            let size = f.local(Val::I32);
            f.i32(n);
            f.set(size);
            check_room(&mut f, 0, size);
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
        LTy::Str => {
            let (n, s) = (f.local(Val::I32), f.local(Val::I32));
            get_len(&mut f, 0);
            f.set(n);
            check_room(&mut f, 0, n);
            // Every empty string is the one static view (§23.7).
            f.u32(EMPTY);
            f.set(s);
            f.get(n);
            f.op(I::If(BlockType::Empty));
            f.get(n);
            f.helper(Helper::Str);
            f.set(s);
            copy(&mut f, 0, n, false, |f| f.get(s));
            f.op(I::End);
            f.get(s);
            f.get(n);
        }
        LTy::Bytes => {
            let (n, buf) = (f.local(Val::I32), f.local(Val::I32));
            get_len(&mut f, 0);
            f.set(n);
            check_room(&mut f, 0, n);
            f.get(n);
            f.helper(Helper::ListBuf { elem: byte_elem() });
            f.set(buf);
            f.get(buf);
            f.get(n);
            f.op(I::I32Store(mem(HEAP, LIST_LEN, 2)));
            copy(&mut f, 0, n, false, |f| {
                f.get(buf);
                f.u32(LIST_ELEMS);
                f.op(I::I32Add);
            });
            f.get(buf);
            f.helper(Helper::ListHeader);
        }
        LTy::List(l) => {
            let (n, buf, at, i) = (
                f.local(Val::I32),
                f.local(Val::I32),
                f.local(Val::I32),
                f.local(Val::I32),
            );
            get_len(&mut f, 0);
            f.set(n);
            // Every element takes at least one byte.
            check_room(&mut f, 0, n);
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
            let vals = dec_into(&mut f, 0, &l.ty);
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
            f.helper(Helper::ListHeader);
        }
        LTy::Record(fields) => {
            let mut all = Vec::new();
            for fb in fields {
                all.extend(dec_into(&mut f, 0, fb));
            }
            for l in all {
                f.get(l);
            }
        }
        LTy::Enum(e) => {
            let tag = f.local(Val::I32);
            let res: Vec<u32> = e.vals.iter().map(|v| f.local(*v)).collect();
            get_len(&mut f, 0);
            f.set(tag);
            f.get(tag);
            f.i32(i32::try_from(e.variants.len()).expect("variants"));
            f.op(I::I32GeU);
            f.op(I::If(BlockType::Empty));
            malformed(&mut f);
            f.op(I::End);
            for (v, fields) in e.variants.iter().enumerate() {
                if fields.iter().all(|(ps, _)| ps.is_empty()) {
                    continue;
                }
                f.get(tag);
                f.i32(i32::try_from(v).expect("variants"));
                f.op(I::I32Eq);
                f.op(I::If(BlockType::Empty));
                for (ps, fb) in fields {
                    let ls = dec_into(&mut f, 0, fb);
                    for (p, l) in ps.iter().zip(ls) {
                        f.get(l);
                        f.set(res[*p as usize]);
                    }
                }
                f.op(I::End);
            }
            f.get(tag);
            f.set(res[0]);
            for r in res {
                f.get(r);
            }
        }
        LTy::Handle(h) => {
            let (number, data) = (f.local(Val::I32), f.local(Val::I32));
            get_len(&mut f, 0);
            f.set(number);
            // The box of the number: nothing in it is counted.
            f.i32(4);
            f.i32(0);
            f.helper(Helper::New);
            f.set(data);
            f.get(data);
            f.get(number);
            f.op(I::I32Store(mem(HEAP, 0, 2)));
            f.get(data);
            f.body.push(Ins::VTable(h.vtable));
            if let Some(wrap) = h.wrap {
                // The reference arrives with a count of its own; the box's
                // first goes. `wrap` owns its argument (`layout::Pass`),
                // so it takes a count of the box of its own.
                if m.runtime.is_some() {
                    own_arg(&mut f, &[Val::I32], Some(&Walk::Ref(0)), &|f| f.get(data));
                }
                f.call(Sym::Inst(wrap));
                release(&mut f, m, &[data], vec![Val::I32], Some(Walk::Ref(0)));
            }
        }
        LTy::Json(j) => {
            let Some([decode, is_ok, expect]) = j.decode else {
                f.op(I::Unreachable);
                return f;
            };
            let text = dec_into(&mut f, 0, &LTy::Str);
            // `decode`, `is_ok` and `expect` own their arguments
            // (`layout::Pass`): each takes a count of its own.
            let counts = m.runtime.is_some();
            if counts {
                own_arg(&mut f, &[Val::I32, Val::I32], Some(&Walk::Ref(0)), &|f| {
                    f.get(text[0]);
                    f.get(text[1]);
                });
            }
            for l in &text {
                f.get(*l);
            }
            f.call(Sym::Inst(decode));
            let res: Vec<u32> = j.decoded.iter().map(|v| f.local(*v)).collect();
            for l in res.iter().rev() {
                f.set(*l);
            }
            release(
                &mut f,
                m,
                &text,
                vec![Val::I32, Val::I32],
                Some(Walk::Ref(0)),
            );
            let get_res = |f: &mut Func| {
                for l in &res {
                    f.get(*l);
                }
            };
            if counts {
                own_arg(&mut f, &j.decoded, j.decoded_walk.as_ref(), &get_res);
            }
            get_res(&mut f);
            f.call(Sym::Inst(is_ok));
            f.op(I::I32Eqz);
            f.op(I::If(BlockType::Empty));
            // A `deserialize` error is a `host-contract` panic the caller
            // sees (`module.boundary.deserialize.exact`).
            f.helper(Helper::Panic(
                "host-contract: a host boundary value does not deserialize".into(),
            ));
            f.op(I::End);
            // Checked above, so this never fails: the value `decode` built.
            let checked = b"a host boundary decode already checked";
            let lit = m.static_bytes(checked, 1);
            let value: Vec<u32> = j.vals.iter().map(|v| f.local(*v)).collect();
            if counts {
                own_arg(&mut f, &j.decoded, j.decoded_walk.as_ref(), &get_res);
            }
            get_res(&mut f);
            f.u32(lit);
            f.u32(u32::try_from(checked.len()).expect("literal"));
            f.call(Sym::Inst(expect));
            for l in value.iter().rev() {
                f.set(*l);
            }
            release(&mut f, m, &res, j.decoded.clone(), j.decoded_walk.clone());
            for l in value {
                f.get(l);
            }
        }
    }
    f.get(0);
    f
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
        colls: Vec::new(),
    }
}
