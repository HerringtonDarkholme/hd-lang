//! Run-time exclusivity through unseen counted aliases (`ref.excl.alias`,
//! stage 7e of `docs/future-work/RETURNED_REFERENCES_PATH.md`). A `&` or
//! `&mut` binding of a place inside a counted object registers the
//! place's address range in the runtime's loan table
//! ([`Helper::LoanBegin`]) until a direct binding's last use or a
//! containing value's release; a copy of a holder shares the entry.
//! An access through a counted reference that a registered loan of the
//! program could overlap checks the table ([`Helper::LoanCheck`]) and
//! panics `borrow-conflict` on a conflicting overlap. A call's result
//! that such a binding stores takes the loan of the argument the checker
//! found it holds ([`Body::held_args`]), keyed by that argument's address,
//! which a local of its own keeps. In a development build, a direct
//! reference argument takes the same loan only until its call returns.
//! A program with no such binding or argument emits none of this.

use hd_base::{SliceSort as _, StageResult};
use hd_mono::{ProgramEnv, Sub, Target, subst};
use hd_tir::ir::{Body, Callee, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};
use hd_types::{InternPool, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use crate::Input;
use crate::layout::{
    Kind, enum_mem, kind, mem as mem_of, payload_mem, ref_mutable, strip, tuple_members,
    value_parts, variants,
};
use crate::module::{Global, Module, Val};
use crate::op::Op as I;
use crate::rt::Helper;

type Guard = (Ty, Vec<u32>);
/// A loan place: the counted object, the field path in it, and each
/// guarded pointer slot with the index of the dereference that reads it.
type LoanPlace = (Ty, Vec<u32>, Vec<(usize, Guard)>);

/// A loan the program may register: the type of the counted object its
/// place lies in, the struct field path to the place, and whether it is
/// exclusive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
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

/// One loan action after a store into a holder slot.
#[derive(Clone, Copy)]
pub(super) struct Binding {
    pub held: Held,
    pub bind: Bind,
}

/// A binding that holds a registered loan: its loan's size and whether it
/// is `&mut`, which with its value, or for a call's result the address in
/// its key local (`keyed`), key the loan's entry.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Holder {
    size: u32,
    excl: bool,
    keyed: bool,
    guards: usize,
}

/// The type at the end of the struct field `f` of type `t`.
fn field_ty(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, f: u32) -> Option<Ty> {
    let t = match pool.get(t) {
        TyData::Mut(inner) => inner,
        _ => t,
    };
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
                Kind::Data(tys) => {
                    k += 1;
                    let Some(Proj::Field(f)) = p.proj.get(k) else {
                        return None;
                    };
                    *tys.get(*f as usize)?
                }
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

/// Whether two field paths from one object overlap: one is a prefix of
/// the other (`ref.excl.place`).
fn overlap(a: &[u32], b: &[u32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x == y)
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

/// The direct local-reference arguments of call `i` that take a loan for
/// the call. A stored reference already has its binding's longer loan.
fn call_loans(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    i: usize,
    receiver: Option<bool>,
) -> Vec<(u32, Loan)> {
    call_args(b, i)
        .into_iter()
        .enumerate()
        .filter_map(|(k, a)| {
            let v = direct_value(b, a)?;
            let auto = (k == 0).then_some(receiver).flatten();
            if auto.is_none() && b.tags[v] != Tag::Ref {
                return None;
            }
            let mut loan = arg_loan(pool, env, b, sub, a)?;
            if let Some(mutable) = auto {
                loan.excl = mutable;
            }
            Some((a, loan))
        })
        .collect()
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
    if let TyData::Option(inner) = pool.get(t) {
        let variants = vec![vec![], vec![inner]];
        let Ok((_, positions)) = enum_mem(pool, env, &variants) else {
            return Vec::new();
        };
        return ref_slots_at(pool, env, inner, depth + 1)
            .into_iter()
            .filter_map(|(slot, ty)| Some((*positions.get(1)?.first()?.get(slot)?, ty)))
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
                let (_, positions) = enum_mem(pool, env, &all).ok()?;
                let at = positions.get(case as usize)?.get(field as usize)?;
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
        return out;
    }
    if let Some(&(_, a)) = b.held_args.iter().find(|r| r.0 as usize == c)
        && let Some(loan) = arg_loan(pool, env, b, sub, a)
    {
        return ref_slots(pool, env, sub(b.ty[c]))
            .into_iter()
            .map(|(slot, _)| (slot, Bind::Call(a), Some(loan.clone())))
            .collect();
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
            let Ok((_, positions)) = enum_mem(pool, env, &all) else {
                return Vec::new();
            };
            let case = b.data[c][0] as usize;
            let values = b.record(b.data[c][1]);
            let (Some(fields), Some(at)) = (all.get(case), positions.get(case)) else {
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
                    .filter_map(|(slot, bind, loan)| Some((*slots.get(slot)?, bind, loan)))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Each `Store` of a body into a local binding that registers one or more
/// loans, by flattened holder slot: a loan-taking `Ref` nested in a value,
/// a call's result that holds one ([`Body::held_args`]), or a copy or move
/// of another holder. `sub` gives a local's or instruction's type at this
/// instance.
pub(super) fn binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    every_ref: bool,
) -> Vec<(usize, Held, Bind, Option<Loan>)> {
    let mut out: Vec<(usize, Held, Bind, Option<Loan>)> = Vec::new();
    let mut grew = true;
    while grew {
        grew = false;
        for i in 0..b.tags.len() {
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
                if slots.contains(&slot) && !out.iter().any(|x| x.0 == i && x.1 == held) {
                    out.push((i, held, bind, loan));
                    grew = true;
                }
            }
        }
    }
    out
}

/// The loans the program may register ([`Module::loans`]): every
/// instance body's loan-taking bindings. Erased bodies and thunks
/// register none.
fn push_loan(m: &mut Module, loan: Loan) {
    // Path-slot markers are development-build checks; release registers none.
    for (object, path) in loan.guards.iter().filter(|_| !m.release) {
        let guard = Loan {
            object: *object,
            path: path.clone(),
            guards: Vec::new(),
            excl: false,
        };
        if !m.loans.contains(&guard) {
            m.loans.push(guard);
        }
    }
    if !m.loans.contains(&loan) {
        m.loans.push(loan);
    }
}

pub(crate) fn collect_loans(input: &Input<'_>, m: &mut Module) {
    let t = &input.collected.table;
    if input.runtime.is_none() {
        return;
    }
    for id in input.order {
        let (Sub::Body(_), Some(b)) = (t.sub[id.idx()], (input.body)(*id)) else {
            continue;
        };
        let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
        let sub = |ty| subst(input.pool, input.env, item, args, ty);
        for (_, _, _, loan) in binds(input.pool, input.env, b, &sub, false) {
            if let Some(loan) = loan {
                push_loan(m, loan);
            }
        }
        // Call loans are development-build checks; release registers none.
        if input.release {
            continue;
        }
        for i in 0..b.tags.len() {
            let target = input.collected.calls[id.idx()].get(&u32::try_from(i).expect("insts"));
            let Some(receiver) = call_info(input, b, i, target) else {
                continue;
            };
            for (_, loan) in call_loans(input.pool, input.env, b, &sub, i, receiver) {
                push_loan(m, loan);
            }
        }
    }
    super::elide::decide(input, m);
}

impl Em<'_, '_, '_> {
    /// Registers the pointer slots that keep a reference place's counted
    /// path connected. Marker loans are shared and end with the following
    /// ordinary loan. Release builds register none.
    fn loan_path_begin(&mut self, a: u32, count: usize) -> StageResult<()> {
        if self.input.release || count == 0 {
            return Ok(());
        }
        let Some(v) = direct_value(self.b, a) else {
            return Ok(());
        };
        if self.b.tags[v] != Tag::Ref {
            return Ok(());
        }
        let v = u32::try_from(v).expect("insts");
        let marks = match self.path_marks.get(&v) {
            Some(marks) => marks.clone(),
            None => self.loan_path_slots(v)?,
        };
        for address in marks {
            self.f.get(address);
            self.f.u32(4);
            self.f.i32(-1);
            self.f.helper(Helper::LoanBegin);
        }
        Ok(())
    }

    /// The locals of the addresses of the pointer slots `Ref` `v`'s place
    /// reads along its path. A slot kept in Wasm locals has none.
    fn loan_path_slots(&mut self, v: u32) -> StageResult<Vec<u32>> {
        let sub = |t| self.sub(t);
        let Some((p, (_, _, guards))) =
            ref_loan_place(self.pool(), self.env(), self.b, &sub, v as usize)
        else {
            return Ok(Vec::new());
        };
        let mut marks = Vec::new();
        for (d, _) in guards {
            let mut fresh = Vec::new();
            let physical = self.deref_physical;
            self.deref_physical = true;
            let walked = self.walk_place(v, &p, d, None, &mut fresh);
            self.deref_physical = physical;
            let (at, _) = walked?;
            if let Some(address) = self.at_address(&at) {
                // The address may be in the root's own local, which the
                // root's release clears.
                let x = self.f.local(Val::I32);
                self.f.get(address);
                self.f.set(x);
                marks.push(x);
            }
            self.release_fresh(&fresh)?;
        }
        Ok(marks)
    }

    /// After `Ref` `v` whose loan registers path-slot markers: keeps the
    /// slots' addresses while its place's root is still live.
    fn loan_path_keep(&mut self, v: u32) -> StageResult<()> {
        if self.marked_refs.contains(&v) && self.vals.contains_key(&v) {
            let marks = self.loan_path_slots(v)?;
            self.path_marks.insert(v, marks);
        }
        Ok(())
    }

    /// Marks the `Ref`s whose holder or call loans register path-slot
    /// markers (`ref.excl.alias.path-pointer`). Release builds mark none.
    fn mark_path_refs(&mut self, a: u32) {
        if let Some(v) = direct_value(self.b, a)
            && self.b.tags[v] == Tag::Ref
        {
            self.marked_refs.insert(u32::try_from(v).expect("insts"));
        }
    }

    /// Registers the direct reference arguments of call `i` until that
    /// call returns. Their values are the addresses that key the loans.
    /// Release builds register none.
    pub(super) fn loan_call_begin(&mut self, i: u32) -> StageResult<Vec<(u32, u32, i32)>> {
        if self.input.release || self.m.loans.is_empty() || !self.owned {
            return Ok(Vec::new());
        }
        let sub = |t| self.sub(t);
        let Some(receiver) = call_info(self.input, self.b, i as usize, self.calls.get(&i)) else {
            return Ok(Vec::new());
        };
        let loans = call_loans(self.pool(), self.env(), self.b, &sub, i as usize, receiver);
        let mut active = Vec::new();
        for (arg, loan) in loans {
            let (target, _) = self.fold_path(loan.object, &loan.path)?;
            let size = mem_of(self.pool(), self.env(), target)?.size;
            let excl = i32::from(loan.excl);
            let key = self.f.local(Val::I32);
            self.comp(arg, 0)?;
            self.f.set(key);
            let conditional = loan.path.is_empty();
            if conditional {
                self.f.get(key);
                self.f.u32(crate::module::HEADER);
                self.f.op(I::I32Sub);
                self.f
                    .op(I::I32Load(crate::module::mem(crate::module::HEAP, 0, 2)));
                self.f.i32(1);
                self.f.op(I::I32Eq);
                self.f.op(I::If(BlockType::Empty));
                self.f.i32(0);
                self.f.set(key);
                self.f.op(I::Else);
            }
            self.loan_path_begin(arg, loan.guards.len())?;
            self.f.get(key);
            self.f.u32(size);
            self.f.i32(excl);
            self.f.helper(Helper::LoanBegin);
            if conditional {
                self.f.op(I::End);
            }
            active.push((key, size, excl));
        }
        Ok(active)
    }

    /// Ends the temporary argument loans after their call returns.
    pub(super) fn loan_call_end(&mut self, loans: &[(u32, u32, i32)]) {
        for &(key, size, excl) in loans.iter().rev() {
            self.f.get(key);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(key);
            self.f.u32(size);
            self.f.i32(excl);
            self.f.i32(-1);
            self.f.helper(Helper::LoanCount);
            self.f.op(I::End);
        }
    }

    /// Finds the body's holder bindings and the stores that bind them,
    /// when the program registers loans at all. A binding keeps its first
    /// holder's key; a suspending body's frame saves the key's local.
    pub(super) fn find_holders(&mut self) {
        if self.m.loans.is_empty() {
            return;
        }
        let sub = |t| self.sub(t);
        let found = binds(self.pool(), self.env(), self.b, &sub, false);
        if !self.input.release {
            let mut marked = Vec::new();
            for (_, _, bind, loan) in &found {
                if let (Bind::Begin(a) | Bind::Call(a), Some(loan)) = (bind, loan)
                    && !loan.guards.is_empty()
                {
                    marked.push(*a);
                }
            }
            for i in 0..self.b.tags.len() {
                let target = self.calls.get(&u32::try_from(i).expect("insts"));
                let Some(receiver) = call_info(self.input, self.b, i, target) else {
                    continue;
                };
                for (a, loan) in call_loans(self.pool(), self.env(), self.b, &sub, i, receiver) {
                    if !loan.guards.is_empty() {
                        marked.push(a);
                    }
                }
            }
            for a in marked {
                self.mark_path_refs(a);
            }
        }
        for (i, held, bind, loan) in found {
            let keyed = match bind {
                Bind::Share(m) | Bind::Move(m) => self.holders.get(&m).filter(|h| h.keyed),
                _ => None,
            };
            let h = if let Some(&h) = keyed {
                h
            } else {
                let ty = self.sub(self.b.local_ty[held.local]);
                let guards = loan.as_ref().map_or(0, |loan| loan.guards.len());
                let (target, excl, keyed) = match (bind, loan) {
                    (Bind::Call(_), Some(loan)) => {
                        let t = self.fold_path(loan.object, &loan.path).ok();
                        (t.map(|x| x.0), loan.excl, true)
                    }
                    (Bind::Call(_), _) => continue,
                    _ => match ref_slots(self.pool(), self.env(), ty)
                        .into_iter()
                        .find(|x| x.0 == held.slot)
                        .map(|x| x.1)
                    {
                        Some(ref_ty) => match self.kind(ref_ty) {
                            Kind::Ref { target, .. } => (
                                Some(target),
                                ref_mutable(self.pool(), self.env(), ref_ty),
                                false,
                            ),
                            _ => continue,
                        },
                        _ => continue,
                    },
                };
                let Some(Ok(m)) = target.map(|t| mem_of(self.pool(), self.env(), t)) else {
                    continue;
                };
                Holder {
                    size: m.size,
                    excl,
                    keyed,
                    guards,
                }
            };
            if self.holders.get(&held).is_some_and(|o| *o != h) {
                continue;
            }
            self.holders.insert(held, h);
            self.binds
                .entry(u32::try_from(i).expect("insts"))
                .or_default()
                .push(Binding { held, bind });
            if let Bind::Call(a) = bind {
                self.held_srcs.insert(a, None);
            }
        }
    }

    /// The Wasm local that keeps the address keying the loan of keyed
    /// holder `l`.
    fn loan_key(&mut self, held: Held) -> u32 {
        if let Some(&k) = self.loan_keys.get(&held) {
            return k;
        }
        let k = self.f.local(Val::I32);
        self.loan_keys.insert(held, k);
        k
    }

    /// Pushes the address keying holder `l`'s loan, `x` its value's local.
    fn push_key(&mut self, held: Held, h: Holder, x: u32) {
        let k = if h.keyed { self.loan_key(held) } else { x };
        self.f.get(k);
    }

    /// Whether local `l` holds a registered loan: a holder binding with a
    /// count of its own, in no cell.
    fn holder(&self, held: Held) -> Option<Holder> {
        let h = *self.holders.get(&held)?;
        (!self.is_cell(held.local) && !self.loan_locals.contains_key(&held.local)).then_some(h)
    }

    /// Locals whose flattened value contains at least one loan holder.
    pub(super) fn loan_holder_locals(&self) -> Vec<usize> {
        let mut locals: Vec<usize> = self.holders.keys().map(|held| held.local).collect();
        locals.extend(self.borrow_locals());
        locals.sort_stable();
        locals.dedup();
        locals
    }

    /// Pushes the key of holder `h`'s entry after its value, then calls
    /// [`Helper::LoanCount`] with `delta`.
    fn loan_count(&mut self, h: Holder, delta: i32) {
        self.f.u32(h.size);
        self.f.i32(i32::from(h.excl));
        self.f.i32(delta);
        self.f.helper(Helper::LoanCount);
    }

    /// The end of the loan holder local `l` (Wasm locals `ls`) holds, at
    /// its release: one count less of its entry.
    pub(super) fn loan_release(&mut self, l: usize, ls: &[u32]) {
        self.borrow_release(l);
        let mut held: Vec<Held> = self
            .holders
            .keys()
            .filter(|h| h.local == l)
            .copied()
            .collect();
        held.sort_stable();
        for held in held {
            if let (Some(h), Some(&x)) = (self.holder(held), ls.get(held.slot)) {
                self.push_key(held, h, x);
                self.loan_count(h, -1);
            }
        }
    }

    /// Before `Store` `i`: the loan of the value a holder binding still
    /// holds ends, since the store replaces it.
    // This hook sits on the recursive instruction-emission path. Outlining
    // keeps the 1,000-arm flat-chain regression within its stack bound.
    #[inline(never)]
    pub(super) fn loan_before_store(&mut self, i: u32) {
        let Some((l, true)) = self.b.stored_local(i as usize) else {
            return;
        };
        if let Some(ls) = self.locals.get(&l.idx()).cloned() {
            self.loan_release(l.idx(), &ls);
        }
    }

    /// After `Store` `i` into a holder binding: its new loan's entry, or
    /// one more count of the entry of the binding it copied. After the
    /// argument of a call whose result a holder binds: the address that
    /// keys the loan, kept before the call or an inlined copy moves it.
    // See `loan_before_store` for why this hook stays outlined.
    #[inline(never)]
    pub(super) fn loan_after_store(&mut self, i: u32) -> StageResult<()> {
        self.loan_path_keep(i)?;
        if let Some(None) = self.held_srcs.get(&i)
            && self.vals.contains_key(&i)
        {
            let k = self.f.local(Val::I32);
            self.comp(i, 0)?;
            self.f.set(k);
            self.held_srcs.insert(i, Some(k));
        }
        let Some(bindings) = self.binds.get(&i).cloned() else {
            return Ok(());
        };
        let Some((l, true)) = self.b.stored_local(i as usize) else {
            return Ok(());
        };
        let l = l.idx();
        for Binding { held, bind } in bindings {
            let (Some(h), Some(&x)) = (
                self.holder(held),
                self.locals.get(&l).and_then(|ls| ls.get(held.slot)),
            ) else {
                continue;
            };
            // A keyed holder's key: the address its call's argument held,
            // or the key of the holder it copies.
            let from = match bind {
                Bind::Call(a) => self.held_srcs.get(&a).copied().flatten(),
                Bind::Share(source) | Bind::Move(source) if h.keyed => Some(self.loan_key(source)),
                _ => None,
            };
            if h.keyed {
                let k = self.loan_key(held);
                match from {
                    Some(f) => self.f.get(f),
                    None => self.f.i32(0),
                }
                self.f.set(k);
                if matches!(bind, Bind::Move(_)) || from.is_none() {
                    continue;
                }
            }
            self.push_key(held, h, x);
            match bind {
                Bind::Begin(a) | Bind::Call(a) => {
                    self.loan_path_begin(a, h.guards)?;
                    self.f.u32(h.size);
                    self.f.i32(i32::from(h.excl));
                    self.f.helper(Helper::LoanBegin);
                }
                Bind::Share(_) => self.loan_count(h, 1),
                Bind::Move(_) => self.f.op(I::Drop),
            }
        }
        Ok(())
    }

    /// Whether an access of `path` in a `object`, a write or a read, may
    /// conflict with a loan the program registers.
    fn may_conflict(&self, object: Ty, path: &[u32], write: bool) -> bool {
        (self.m.loans.iter())
            .any(|l| l.object == object && overlap(&l.path, path) && (write || l.excl))
    }

    /// Before instruction `i`: the loan check of the access it makes
    /// through a counted reference, where a registered loan may conflict
    /// with it. A `Store` or an `IsUnique` writes its place, a `Ref`
    /// borrows it, and a `Read` reads it; a call that takes a counted
    /// reference accesses its whole object, a write for a `*mut`.
    pub(super) fn loan_access(&mut self, i: u32) -> StageResult<()> {
        if self.m.loans.is_empty() || !self.owned {
            return Ok(());
        }
        let tag = self.b.tags[i as usize];
        if matches!(tag, Tag::Call | Tag::CallDyn | Tag::CallValue) {
            let mut args = Vec::new();
            slots(self.b, i as usize, |_, v, word| {
                if matches!(word, Word::Value(_)) && v < i {
                    args.push(v);
                }
            });
            self.loan_call_args(&args, None)?;
            return Ok(());
        }
        let write = match tag {
            Tag::Store | Tag::IsUnique => true,
            Tag::Ref => ref_mutable(self.pool(), self.env(), self.ty(i)),
            Tag::Read => false,
            _ => return Ok(()),
        };
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(());
        };
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return Ok(());
        }
        let Some((d, Some((target, false)))) = self.last_through(&p)? else {
            return Ok(());
        };
        let before = self.walk_ty(self.at_root_ty(p.root), &p.proj[..d])?;
        let (Kind::Ref { whole: true, .. }, Some(path)) =
            (self.kind(before), fields(&p.proj[d + 1..]))
        else {
            return Ok(());
        };
        if !self.may_conflict(target, &path, write) {
            return Ok(());
        }
        let (ft, part) = self.fold_path(target, &path)?;
        let size = mem_of(self.pool(), self.env(), ft)?.size;
        self.loan_check(write, size, |em| {
            let mut fresh = Vec::new();
            let (at, _) = em.walk_place(i, &p, d, None, &mut fresh)?;
            let r = em.at_i32(&at)?;
            em.release_fresh(&fresh)?;
            em.f.get(r);
            if part.offset != 0 {
                em.f.u32(part.offset);
                em.f.op(I::I32Add);
            }
            Ok(())
        })
    }

    /// Checks the counted-reference arguments of a call as whole-object
    /// accesses. A bang call passes `(pc, state)` so its resumed polls skip
    /// the checks: only the first poll accesses the arguments.
    pub(super) fn loan_call_args(
        &mut self,
        args: &[u32],
        first_poll: Option<(u32, u32)>,
    ) -> StageResult<()> {
        if self.m.loans.is_empty() || !self.owned {
            return Ok(());
        }
        let mut accesses = Vec::new();
        for &v in args {
            let ty = self.ty_of(v);
            let Kind::Ref {
                target,
                fat: false,
                whole: true,
            } = self.kind(ty)
            else {
                continue;
            };
            let write = ref_mutable(self.pool(), self.env(), ty);
            if !self.vals.contains_key(&v) || !self.may_conflict(target, &[], write) {
                continue;
            }
            accesses.push((v, write, mem_of(self.pool(), self.env(), target)?.size));
        }
        if accesses.is_empty() {
            return Ok(());
        }
        if let Some((pc, state)) = first_poll {
            self.f.get(pc);
            self.f.u32(state);
            self.f.op(I::I32Ne);
            self.f.op(I::If(BlockType::Empty));
        }
        for (v, write, size) in accesses {
            self.loan_check(write, size, |em| em.comp(v, 0))?;
        }
        if first_poll.is_some() {
            self.f.op(I::End);
        }
        Ok(())
    }

    /// While any loan is registered: `addr` pushes the accessed place's
    /// address, of `size` bytes, and [`Helper::LoanCheck`] checks it.
    fn loan_check(
        &mut self,
        write: bool,
        size: u32,
        addr: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        self.f.global_get(Global::LoanTop);
        self.f.op(I::If(BlockType::Empty));
        addr(self)?;
        self.f.u32(size);
        self.f.i32(i32::from(write));
        self.f.helper(Helper::LoanCheck);
        self.f.op(I::End);
        Ok(())
    }
}
