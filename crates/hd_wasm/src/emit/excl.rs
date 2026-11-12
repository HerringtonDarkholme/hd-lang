//! Holder bindings for pointer loans (model A of
//! `docs/future-work/POINTER_LOANS.md`, `borrow.rs`): which stores bind a
//! reference into a holder (a binding, or a struct, tuple or variant that
//! holds it), from a `Ref`, a call's result that depends on an argument
//! the checker found it holds ([`Body::held_args`]), or another holder by
//! a copy or a move; and the calls' arguments those loans read.

use hd_mono::{ProgramEnv, Target, subst};
use hd_tir::ir::{Body, Callee, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};
use hd_types::{InternPool, Ty, TyData};

use crate::Input;
use crate::layout::{
    Kind, flat, kind, mem as mem_of, payload_mem, ref_mutable, strip, tuple_members, value_parts,
    variants,
};
use crate::module::Module;
use std::sync::Arc;

type Guard = (Ty, Vec<u32>);
/// A loan place: the counted object, the field path in it, and each
/// guarded pointer slot with the index of the dereference that reads it.
type LoanPlace = (Ty, Vec<u32>, Vec<(usize, Guard)>);

/// A loan the program may register: the type of the counted object its
/// place lies in, the struct field path to the place, and whether it is
/// exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Loan {
    pub object: Ty,
    pub path: Vec<u32>,
    /// Pointer slots on the counted path, as places in their containing
    /// objects. Replacing one invalidates the path while the loan is live.
    pub guards: Vec<Guard>,
    pub excl: bool,
}

/// How a `Store` into a holder binding registers its loan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Bind {
    /// A new loan of the stored reference's place.
    Begin(u32),
    /// A new loan of what a call's argument `.0` refers to.
    Call(u32),
    /// An inlined call's containing pointer slots, in addition to the
    /// returned reference's own loans.
    CallParent(u32),
    /// A copy of holder `.0`: one more count of its entry.
    Share(Held),
    /// A move from holder `.0`: the entry passes over.
    Move(Held),
}

/// One flattened value slot of a local that holds a run-time loan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct Held {
    pub(super) local: usize,
    pub(super) slot: usize,
}

/// The type at the end of the struct field `f` of type `t`.
fn field_ty(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, f: u32) -> Option<Ty> {
    match pool.get(t) {
        TyData::Adt { def, args } => {
            let fs = env.data_fields(def)?;
            Some(subst(pool, env, def, args, *fs.get(f as usize)?))
        }
        TyData::Tuple { .. } => crate::layout::tuple_members(pool, t)
            .get(f as usize)
            .copied(),
        _ => None,
    }
}

/// The place of loan-taking `Ref` `word`, rooted at a value of type
/// `root`, as a counted object's field: the object's type and the field
/// path, when the place's last dereference reads a thin counted reference,
/// only fields follow it, and no element or variant step is on the path
/// (an element reference's loan is its collection's pin). The guards are
/// the counted pointer slots read on the way, each in the object the
/// previous thin reference refers to: a `&T` root's slots too.
fn loan_place(pool: &InternPool, env: &dyn ProgramEnv, root: Ty, p: &Place) -> Option<LoanPlace> {
    let mut ty = root;
    let mut counted: Option<(Ty, usize)> = None;
    // The target of the last local reference before the first counted one.
    let mut local: Option<(Ty, usize)> = None;
    let mut guards = Vec::new();
    let mut k = 0;
    while k < p.proj.len() {
        ty = match p.proj[k] {
            Proj::Field(f) => field_ty(pool, env, ty, f)?,
            Proj::Deref => match kind(pool, env, ty) {
                Kind::Ref {
                    target,
                    fat: false,
                    whole: true,
                } => {
                    if let Some((object, start)) = counted {
                        guards.push((k, (object, fields(&p.proj[start..k])?)));
                    } else if let Some((object, start)) = local
                        && let Some(path) = fields(&p.proj[start..k])
                    {
                        guards.push((k, (object, path)));
                    }
                    counted = Some((target, k + 1));
                    target
                }
                Kind::Ref {
                    target, fat: false, ..
                } => {
                    if counted.is_none() {
                        local = Some((target, k + 1));
                    }
                    target
                }
                Kind::Ref { target, .. } => target,
                _ => return None,
            },
            Proj::Elem(_) | Proj::Variant(_) => return None,
        };
        k += 1;
    }
    let (object, start) = counted?;
    Some((object, fields(&p.proj[start..])?, guards))
}

/// The struct field indices `steps` name, when they are all fields.
fn fields(steps: &[Proj]) -> Option<Vec<u32>> {
    steps
        .iter()
        .map(|s| match s {
            Proj::Field(f) => Some(*f),
            _ => None,
        })
        .collect()
}

/// The loan argument `a` of a call takes: a `Ref`'s place inside a
/// counted object, or the whole object a counted reference refers to.
fn arg_loan(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    a: u32,
) -> Option<Loan> {
    let v = direct_value(b, a)?;
    let excl = ref_mutable(pool, env, sub(*b.ty.get(a as usize)?));
    if b.tags[v] == Tag::Ref {
        let (_, (object, path, guards)) = ref_loan_place(pool, env, b, sub, v)?;
        return Some(Loan {
            object,
            path,
            guards: guards.into_iter().map(|(_, g)| g).collect(),
            excl,
        });
    }
    let t = sub(*b.ty.get(v)?);
    match kind(pool, env, t) {
        Kind::Ref {
            target,
            fat: false,
            whole: true,
        } => Some(Loan {
            object: target,
            path: Vec::new(),
            guards: Vec::new(),
            excl,
        }),
        _ => None,
    }
}

/// The place of `Ref` instruction `v` and its [`loan_place`].
fn ref_loan_place(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    v: usize,
) -> Option<(Place, LoanPlace)> {
    let p = b.decode_place(b.data[v][0])?;
    let root = match p.root {
        PlaceRoot::Local(r) => sub(b.local_ty[r.idx()]),
        PlaceRoot::Global(_, t) => sub(t),
        _ => return None,
    };
    let place = loan_place(pool, env, root, &p)?;
    Some((p, place))
}

/// The instruction behind representation-preserving coercions of `a`.
pub(super) fn direct_value(b: &Body, a: u32) -> Option<usize> {
    let mut v = Ref(a).as_inst()?.0 as usize;
    while b.same_value_coercion(v) {
        v = Ref(b.data[v][0]).as_inst()?.0 as usize;
    }
    Some(v)
}

/// The values passed as arguments by call instruction `i`.
pub(super) fn call_args(b: &Body, i: usize) -> Vec<u32> {
    if b.tags[i] == Tag::CallValue {
        return b.record(b.data[i][1]).to_vec();
    }
    let mut args = Vec::new();
    slots(b, i, |_, v, word| {
        if matches!(word, Word::Value(_)) && v < u32::try_from(i).expect("insts") {
            args.push(v);
        }
    });
    args
}

/// Whether call `i` is non-suspending, and its auto-ref receiver's
/// mutability when it has one.
#[allow(clippy::option_option)] // Outer: ordinary call; inner: auto-ref receiver.
pub(super) fn call_info(
    input: &Input<'_>,
    b: &Body,
    i: usize,
    target: Option<&Target>,
) -> Option<Option<bool>> {
    if b.tags[i] == Tag::CallValue {
        return Some(None);
    }
    if b.tags[i] != Tag::Call {
        return None;
    }
    let env = input.env;
    match target? {
        Target::Call(call) => Some(env.local_ref_receiver(call.item)),
        // A trait method on a trait value: its declaration says how it
        // takes `self`.
        Target::Dyn => {
            let Callee::TraitMethod { method, .. } = Callee::from_words(b.record(b.data[i][0]))?
            else {
                return None;
            };
            Some(env.local_ref_receiver(method))
        }
        _ => None,
    }
}

/// The flattened value slots of `t` that hold local references, and each
/// slot's reference type. Struct and tuple fields retain their layout
/// positions; a nullable `Option[&T]` uses its payload's sole slot.
fn ref_slots(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<(usize, Ty)> {
    ref_slots_at(pool, env, t, 0)
}

fn ref_slots_at(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, depth: usize) -> Vec<(usize, Ty)> {
    if depth > 32 {
        return Vec::new();
    }
    let t = strip(pool, env, t);
    if matches!(
        kind(pool, env, t),
        Kind::Ref {
            whole: false,
            fat: false,
            ..
        }
    ) {
        return vec![(0, t)];
    }
    if let Some(inner) = pool.sole_arg(t, env.option()) {
        let Ok(Some(flat)) = flat(pool, env, t) else {
            return Vec::new();
        };
        return ref_slots_at(pool, env, inner, depth + 1)
            .into_iter()
            .filter_map(|(slot, ty)| {
                Some((*flat.positions.get(1)?.first()?.get(slot)? as usize, ty))
            })
            .collect();
    }
    if !matches!(kind(pool, env, t), Kind::Value) || variants(pool, env, t).is_some() {
        return Vec::new();
    }
    let fields = value_parts(pool, env, t);
    let Ok((_, parts)) = payload_mem(pool, env, &fields) else {
        return Vec::new();
    };
    fields
        .into_iter()
        .zip(parts)
        .flat_map(|(field, part)| {
            ref_slots_at(pool, env, field, depth + 1)
                .into_iter()
                .map(move |(slot, ty)| (part.first + slot, ty))
        })
        .collect()
}

/// For a projection that stays within a value's flattened locals, the
/// root slot that supplies each result slot. A variant and its following
/// field are one layout step because enum payload fields need not occupy
/// contiguous slots.
fn projection_slots(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    mut ty: Ty,
    proj: &[Proj],
) -> Option<Vec<usize>> {
    let mut slots: Vec<usize> = (0..mem_of(pool, env, ty).ok()?.leaves.len()).collect();
    let mut k = 0;
    while k < proj.len() {
        ty = strip(pool, env, ty);
        if !matches!(kind(pool, env, ty), Kind::Value) {
            return None;
        }
        match proj[k] {
            Proj::Field(field) => {
                let fields = value_parts(pool, env, ty);
                let (_, parts) = payload_mem(pool, env, &fields).ok()?;
                let (field_ty, part) = (*fields.get(field as usize)?, *parts.get(field as usize)?);
                slots = slots[part.first..part.first + part.count].to_vec();
                ty = field_ty;
                k += 1;
            }
            Proj::Variant(case) => {
                let Proj::Field(field) = *proj.get(k + 1)? else {
                    return None;
                };
                let all = variants(pool, env, ty)?;
                let layout = flat(pool, env, ty).ok()??;
                // A loan is a reference, a whole cell; a leaf in shared
                // bits names its cell.
                let at: Vec<usize> = layout
                    .positions
                    .get(case as usize)?
                    .get(field as usize)?
                    .iter()
                    .map(|&p| p as usize)
                    .collect();
                ty = *all.get(case as usize)?.get(field as usize)?;
                slots = at.iter().map(|&slot| slots[slot]).collect();
                k += 2;
            }
            Proj::Deref | Proj::Elem(_) => return None,
        }
    }
    Some(slots)
}

/// The sources of the run-time loans in value instruction `v`, by the
/// flattened slot of its result. A construction remaps each part's slots;
/// a whole-local read copies or moves every known holder slot.
fn value_binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    known: &[(usize, Held, Bind, Option<Loan>)],
    v: usize,
    every_ref: bool,
) -> Vec<(usize, Bind, Option<Loan>)> {
    value_binds_at(pool, env, b, sub, known, v, 0, every_ref)
}

/// Whether holder slot `held` holds a `&mut`.
pub(super) fn held_mutable(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    held: Held,
) -> bool {
    ref_slots(pool, env, sub(b.local_ty[held.local]))
        .into_iter()
        .any(|(slot, t)| slot == held.slot && ref_mutable(pool, env, t))
}

/// The `Ref`s whose loans argument value `v` carries through calls'
/// results, when it is no `Ref` itself, as `observe(p.items.get_mut(1)
/// .expect("slot"))` passes the `&mut p.items` it got: each takes its call
/// loan until the call that `v` is an argument of returns.
pub(super) fn carried_refs(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    v: usize,
) -> Vec<u32> {
    if b.tags.get(v) == Some(&Tag::Ref) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (_, bind, _) in value_binds_at(pool, env, b, sub, &[], v, 0, true) {
        if let Bind::Call(a) = bind
            && let Some(r) = direct_value(b, a).filter(|&r| b.tags[r] == Tag::Ref)
        {
            let r = u32::try_from(r).expect("insts");
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

fn value_binds_at(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    known: &[(usize, Held, Bind, Option<Loan>)],
    v: usize,
    depth: usize,
    every_ref: bool,
) -> Vec<(usize, Bind, Option<Loan>)> {
    if depth > 64 {
        return Vec::new();
    }
    if v >= b.tags.len() {
        return Vec::new();
    }
    let mut c = v;
    while b.same_value_coercion(c) {
        c = b.data[c][0] as usize;
    }
    if c >= b.tags.len() {
        return Vec::new();
    }
    // A call's result holds the arguments its callee's result depends on.
    // An inlined call's `Scope` keeps the call's rows, which stand in for
    // its result when its body shows no reference (a pointer's `as_mut`).
    let held = || -> Vec<(usize, Bind, Option<Loan>)> {
        if let Some(&(_, a)) = b.held_args.iter().find(|r| r.0 as usize == c) {
            let loan = arg_loan(pool, env, b, sub, a);
            // Runtime holders retain all crossed objects, including element
            // and variant paths that the field-only promotion model omits.
            if every_ref || loan.is_some() {
                return ref_slots(pool, env, sub(b.ty[c]))
                    .into_iter()
                    .map(|(slot, _)| (slot, Bind::Call(a), loan.clone()))
                    .collect();
            }
        }
        Vec::new()
    };
    // A block's or scope's value is its tail's or a `break`'s to it,
    // as an inlined getter leaves its result (`borrow.rs`).
    if every_ref && matches!(b.tags[c], Tag::Block | Tag::Scope) {
        let mut targets = vec![u32::try_from(c).expect("insts")];
        let mut values = Vec::new();
        let tail = |blk: usize, values: &mut Vec<u32>| {
            if b.tags.get(blk) == Some(&Tag::Block) && b.data[blk][1] != hd_tir::ir::NONE {
                values.push(b.data[blk][1]);
            }
        };
        tail(c, &mut values);
        if b.tags[c] == Tag::Scope {
            let inner = b.data[c][0];
            targets.push(inner);
            tail(inner as usize, &mut values);
        }
        for j in 0..b.tags.len() {
            if b.tags[j] == Tag::Break
                && b.data[j][1] != hd_tir::ir::NONE
                && b.label_inst
                    .get(b.data[j][0] as usize)
                    .is_some_and(|l| targets.contains(l))
            {
                values.push(b.data[j][1]);
            }
        }
        let mut out = Vec::new();
        for v in values {
            for x in value_binds_at(pool, env, b, sub, known, v as usize, depth + 1, every_ref) {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
        }
        if out.is_empty() {
            return held();
        }
        // Inlining materializes a counted argument as an owned local. Its
        // returned reference still depends on the original pointer slot,
        // even though the callee's Ref now starts at that new local.
        for (slot, bind, loan) in held() {
            if let Bind::Call(a) = bind
                && let Some(v) = direct_value(b, a)
                && b.tags[v] == Tag::Read
                && matches!(
                    kind(pool, env, sub(b.ty[v])),
                    Kind::Ref {
                        whole: true,
                        fat: false,
                        ..
                    }
                )
            {
                out.push((slot, Bind::CallParent(a), loan));
            }
        }
        return out;
    }
    let direct = held();
    if !direct.is_empty() {
        return direct;
    }
    // An `if`'s value is its arms' blocks', as an inlined getter that
    // chooses its result leaves it.
    if every_ref && b.tags[c] == Tag::If {
        let mut out = Vec::new();
        for &arm in b.record(b.data[c][1]) {
            for x in value_binds_at(pool, env, b, sub, known, arm as usize, depth + 1, every_ref) {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
        }
        return out;
    }
    // A call's result of one reference holds the references its
    // arguments pass by value, as `expect` passes on its option's.
    if every_ref
        && b.tags[c] == Tag::Call
        && let [(slot, _)] = ref_slots(pool, env, sub(b.ty[c]))[..]
    {
        let mut out = Vec::new();
        for a in call_args(b, c) {
            let Some(&t) = b.ty.get(a as usize) else {
                continue;
            };
            let t = sub(t);
            if ref_slots(pool, env, t).is_empty()
                || matches!(kind(pool, env, strip(pool, env, t)), Kind::Ref { .. })
            {
                continue;
            }
            for (_, bind, loan) in
                value_binds_at(pool, env, b, sub, known, a as usize, depth + 1, every_ref)
            {
                if !out
                    .iter()
                    .any(|x: &(usize, Bind, Option<Loan>)| x.1 == bind)
                {
                    out.push((slot, bind, loan));
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    match b.tags[c] {
        Tag::Ref => {
            let r = u32::try_from(c).expect("insts");
            match arg_loan(pool, env, b, sub, r) {
                Some(loan) => vec![(0, Bind::Begin(r), Some(loan))],
                None if every_ref => vec![(0, Bind::Begin(r), None)],
                None => Vec::new(),
            }
        }
        Tag::Read => {
            let Some(place) = b.decode_place(b.data[c][0]) else {
                return Vec::new();
            };
            let copy = b.data[c][1] == ReadMode::Copy as u32;
            let (root_ty, sources) = match place.root {
                PlaceRoot::Local(local) => {
                    let local = local.idx();
                    let mut seen = hd_base::HashSet::default();
                    let sources = known
                        .iter()
                        .filter_map(|(_, held, _, _)| {
                            (held.local == local && seen.insert(held.slot)).then_some((
                                held.slot,
                                if copy {
                                    Bind::Share(*held)
                                } else {
                                    Bind::Move(*held)
                                },
                                None,
                            ))
                        })
                        .collect();
                    (sub(b.local_ty[local]), sources)
                }
                PlaceRoot::Value(root) if root.as_inst().is_some() => (
                    sub(b.ty[root.0 as usize]),
                    value_binds_at(
                        pool,
                        env,
                        b,
                        sub,
                        known,
                        root.0 as usize,
                        depth + 1,
                        every_ref,
                    ),
                ),
                _ => return Vec::new(),
            };
            let Some(projected) = projection_slots(pool, env, root_ty, &place.proj) else {
                return Vec::new();
            };
            sources
                .into_iter()
                .filter_map(|(root_slot, bind, loan)| {
                    projected
                        .iter()
                        .position(|slot| *slot == root_slot)
                        .map(|slot| (slot, bind, loan))
                })
                .collect()
        }
        Tag::NewStruct | Tag::NewTuple => {
            let (fields, values, parts) = if b.tags[c] == Tag::NewTuple {
                let mut values = b.record(b.data[c][1]).to_vec();
                if b.data[c][0] != hd_tir::ir::NONE {
                    values.push(b.data[c][0]);
                }
                let fields = tuple_members(pool, sub(b.ty[c]));
                let Ok((_, parts)) = payload_mem(pool, env, &fields) else {
                    return Vec::new();
                };
                (fields, values, parts)
            } else {
                let fields = value_parts(pool, env, sub(b.ty[c]));
                let values = b.record(b.data[c][1]).to_vec();
                let Ok((_, parts)) = payload_mem(pool, env, &fields) else {
                    return Vec::new();
                };
                (fields, values, parts)
            };
            values
                .into_iter()
                .zip(fields)
                .zip(parts)
                .flat_map(|((value, _), part)| {
                    value_binds_at(
                        pool,
                        env,
                        b,
                        sub,
                        known,
                        value as usize,
                        depth + 1,
                        every_ref,
                    )
                    .into_iter()
                    .map(move |(slot, bind, loan)| (part.first + slot, bind, loan))
                })
                .collect()
        }
        Tag::NewVariant => {
            let ty = sub(b.ty[c]);
            let Some(all) = variants(pool, env, ty) else {
                return Vec::new();
            };
            let Ok(Some(layout)) = flat(pool, env, ty) else {
                return Vec::new();
            };
            let case = b.data[c][0] as usize;
            let values = b.record(b.data[c][1]);
            let (Some(fields), Some(at)) = (all.get(case), layout.positions.get(case)) else {
                return Vec::new();
            };
            values
                .iter()
                .zip(fields)
                .zip(at)
                .flat_map(|((&value, _), slots)| {
                    value_binds_at(
                        pool,
                        env,
                        b,
                        sub,
                        known,
                        value as usize,
                        depth + 1,
                        every_ref,
                    )
                    .into_iter()
                    // A loan's holder is a reference, a whole cell.
                    .filter_map(|(slot, bind, loan)| Some((*slots.get(slot)? as usize, bind, loan)))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Whole-local stores grouped by their owning sub-body, in source order.
/// Preparing this once per distinct checked body costs one topology pass
/// and one instruction pass; instances then read only their own stores.
pub(super) fn binding_stores(b: &Body) -> Vec<Arc<[usize]>> {
    let mut stores = vec![Vec::new(); b.sub_root.len()];
    for (i, sub) in b.inst_subs().into_iter().enumerate() {
        if matches!(b.stored_local(i), Some((_, true))) {
            stores[sub as usize].push(i);
        }
    }
    stores.into_iter().map(Arc::from).collect()
}

/// Each `Store` of a sub-body into a local binding that registers one or more
/// loans, by flattened holder slot: a loan-taking `Ref` nested in a value,
/// a call's result that holds one ([`Body::held_args`]), or a copy or move
/// of another holder. `sub` gives a local's or instruction's type at this
/// instance.
pub(super) fn binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    stores: &[usize],
    every_ref: bool,
) -> Vec<(usize, Held, Bind, Option<Loan>)> {
    let mut out: Vec<(usize, Held, Bind, Option<Loan>)> = Vec::new();
    let mut grew = true;
    while grew {
        grew = false;
        for &i in stores {
            let Some((l, true)) = b.stored_local(i) else {
                continue;
            };
            let (l, v) = (l.idx(), b.data[i][1] as usize);
            if v >= b.tags.len() {
                continue;
            }
            let slots: hd_base::HashSet<usize> = ref_slots(pool, env, sub(b.local_ty[l]))
                .into_iter()
                .map(|x| x.0)
                .collect();
            if slots.is_empty() {
                continue;
            }
            for (slot, bind, loan) in value_binds(pool, env, b, sub, &out, v, every_ref) {
                let held = Held { local: l, slot };
                if slots.contains(&slot)
                    && !out.iter().any(|x| {
                        x.0 == i
                            && x.1 == held
                            && (!matches!(bind, Bind::CallParent(_)) || x.2 == bind)
                    })
                {
                    out.push((i, held, bind, loan));
                    grew = true;
                }
            }
        }
    }
    out
}

/// Install the TIR link pass's pointer-loan tables for emission.
pub(crate) fn collect_loans(input: &Input<'_>, m: &mut Module) {
    let decisions = super::loan_facts::decide(input, m);
    m.kept = decisions.kept;
    m.lent = decisions.lent;
    m.result_inner = decisions.result_inner;
    m.dyn_types = decisions.dyn_types;
}
