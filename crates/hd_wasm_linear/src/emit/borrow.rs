//! Pointer loans and the access checks on an object's borrow state (model
//! A of `docs/future-work/POINTER_LOANS.md`, #1336). Every counted object
//! keeps its loans in descriptor bits 16 to 21 (§23.4): bits 16 to 20
//! count shared loans and bit 21 marks an exclusive one.
//!
//! **Loans.** A reference bound to a local (a holder: the binding, or a
//! struct, tuple or variant holding the reference) whose place crosses
//! `*T` objects `o1 … on` takes a shared loan on `o1 … o(n-1)` and, on
//! `on`, a shared loan for `&` or an exclusive one for `&mut`. The `Ref`'s
//! checks are the loans' begin checks; the store into the holder adds the
//! loans, and the holder's release, which the ownership passes place
//! after its last use as a `Drop`, ends them. A copy of a shared holder
//! takes loans of its own on the same objects; a move passes them over.
//!
//! **Checks.** Before a write or a `&mut` through a `*T`, the object it
//! reaches must have no loan; before a read or a `&`, no exclusive loan.
//! Each `*T` before the last on a place is read to reach the next, so its
//! object takes the read check. A conflict panics `borrow-conflict`.
//!
//! **Own loans.** A function's loans taken through a pointer local (the
//! root local and the path to the pointer) are known to it: an access
//! through the same pointer compares the state with them instead of 0.
//! The checker already rejects an access that overlaps one of its own
//! references (`ref.excl.place`), so what remains through that pointer is
//! apart from it (`POINTER_LOANS.md`, question 4). A local per pointer
//! holds the sum.
//!
//! A type no loan may hold needs no check ([`Module::lent`]), so a place
//! that crosses no `*T`, or only `*T`s to such types, emits nothing.
//! Passing a pointer is no access. Call arguments, returned references
//! and suspending bodies take no loan yet (phases 4 to 6).

use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_mono::ProgramEnv;
use hd_tir::ir::{Body, PlaceRoot, Proj, Tag};
use hd_types::{InternPool, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::excl::{Bind, Held, binds, direct_value};
use crate::layout::{Kind, coll_kind, kind, ref_mutable, strip, variants};
use crate::module::{HEAP, LOAN_EXCL, LOAN_SHARED, LOAN_STATE, Module, Val, mem};
use crate::op::Op as I;
use crate::rt::Helper;

/// A pointer a function reaches objects through: a place's root and the
/// steps to the pointer value.
type Key = (PlaceRoot, Vec<Proj>);

/// The per-body state of the loans its holders take.
#[derive(Default)]
pub(super) struct Borrows {
    /// The holder bindings each `Store` makes.
    binds: HashMap<u32, Vec<(Held, Bind)>>,
    /// The `Ref`s whose loans a holder takes, and the objects each one
    /// crossed, once emitted: the header address and the pointer's key.
    refs: HashMap<u32, Vec<(u32, Option<usize>)>>,
    /// Each holder's loans, the last crossing first: the local of the
    /// object's header address (0 while the holder holds none), and the
    /// local of its key's index plus one.
    slots: HashMap<Held, Vec<(u32, u32)>>,
    /// Whether a holder holds a `&mut`: its first loan is exclusive.
    excl: HashMap<Held, bool>,
    /// The pointers places reach objects through, and the local holding
    /// the state of the function's own loans through each, once used.
    keys: Vec<(Key, Option<u32>)>,
    /// The first `loans` keys are those some holder's loan is taken
    /// through, interned before emission.
    loans: usize,
}

/// Records that objects of type `object` may be lent, exclusively when
/// `excl`.
fn lend(m: &mut Module, object: Ty, excl: bool) {
    *m.lent.entry(object).or_insert(false) |= excl;
}

/// The type of struct, tuple or newtype field `f` of `t`.
fn field_ty(pool: &InternPool, env: &dyn ProgramEnv, t: Ty, f: u32) -> Option<Ty> {
    let t = match pool.get(t) {
        TyData::Mut(inner) => inner,
        _ => t,
    };
    match pool.get(t) {
        TyData::Adt { def, args } => {
            let fs = env.data_fields(def)?;
            Some(hd_mono::subst(pool, env, def, args, *fs.get(f as usize)?))
        }
        TyData::Tuple { .. } => crate::layout::tuple_members(pool, t)
            .get(f as usize)
            .copied(),
        _ => None,
    }
}

/// The `*T` dereferences on `steps` from a value of type `root`: each
/// step's index and the type of the object it reaches. The walk stops at
/// a step it cannot type.
fn crossings(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    root: Ty,
    steps: &[Proj],
) -> Vec<(usize, Ty)> {
    let mut out = Vec::new();
    let mut ty = root;
    let mut k = 0;
    while k < steps.len() {
        let next = match steps[k] {
            Proj::Field(f) => field_ty(pool, env, ty, f),
            Proj::Elem(_) => {
                let TyData::Adt { args, .. } = pool.get(strip(pool, env, ty)) else {
                    break;
                };
                let items = pool.list_items(args);
                match coll_kind(pool, env, ty) {
                    Some(hd_mono::layout::StdKind::List) => items.first().copied(),
                    Some(hd_mono::layout::StdKind::Map) => items.get(1).copied(),
                    _ => None,
                }
            }
            Proj::Variant(v) => {
                k += 1;
                let Some(Proj::Field(f)) = steps.get(k) else {
                    break;
                };
                variants(pool, env, ty).and_then(|vs| vs.get(v as usize)?.get(*f as usize).copied())
            }
            Proj::Deref => match kind(pool, env, ty) {
                Kind::Ref { target, fat, whole } => {
                    if whole && !fat {
                        out.push((k, target));
                    }
                    Some(target)
                }
                Kind::Data(tys) => {
                    k += 1;
                    let Some(Proj::Field(f)) = steps.get(k) else {
                        break;
                    };
                    tys.get(*f as usize).copied()
                }
                _ => None,
            },
        };
        let Some(next) = next else {
            break;
        };
        ty = next;
        k += 1;
    }
    out
}

/// The type of a place's root, `sub` giving a type at this instance.
fn root_ty(b: &Body, sub: &dyn Fn(Ty) -> Ty, root: PlaceRoot) -> Option<Ty> {
    match root {
        PlaceRoot::Local(l) => Some(sub(b.local_ty[l.idx()])),
        PlaceRoot::Global(_, t) => Some(sub(t)),
        PlaceRoot::Value(r) => r.as_inst().map(|v| sub(b.ty[v.0 as usize])),
        PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
    }
}

/// Records the objects a holder's loan on `Ref` `a` may hold
/// ([`Module::lent`]).
pub(super) fn lend_ref(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
    m: &mut Module,
    a: u32,
) {
    let Some(v) = direct_value(b, a).filter(|&v| b.tags[v] == Tag::Ref) else {
        return;
    };
    let Some(p) = b.decode_place(b.data[v][0]) else {
        return;
    };
    let Some(root) = root_ty(b, sub, p.root) else {
        return;
    };
    let excl = ref_mutable(pool, env, sub(b.ty[v]));
    let crossed = crossings(pool, env, root, &p.proj);
    let last = crossed.len().wrapping_sub(1);
    for (n, (_, object)) in crossed.into_iter().enumerate() {
        lend(m, object, excl && n == last);
    }
}

impl Em<'_, '_, '_> {
    /// Finds the body's holder bindings that take pointer loans, where the
    /// program lends any object. A suspending body takes none yet: its
    /// frame does not save the loans' locals.
    pub(super) fn find_borrows(&mut self) {
        if self.m.lent.is_empty() {
            return;
        }
        let sub = |t| self.sub(t);
        let found = binds(self.pool(), self.env(), self.b, &sub, true);
        let mut st = Borrows::default();
        for (i, held, bind, _) in found {
            if self.is_cell(held.local) {
                continue;
            }
            match bind {
                Bind::Begin(a) => {
                    let Some(v) = direct_value(self.b, a).filter(|&v| self.b.tags[v] == Tag::Ref)
                    else {
                        continue;
                    };
                    let v = u32::try_from(v).expect("insts");
                    if let Some(p) = self.b.decode_place(self.b.data[v as usize][0])
                        && matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                    {
                        let root = self.at_root_ty(p.root);
                        for (k, step) in p.proj.iter().enumerate() {
                            let thin = *step == Proj::Deref
                                && matches!(
                                    self.walk_ty(root, &p.proj[..k]).map(|t| self.kind(t)),
                                    Ok(Kind::Ref {
                                        fat: false,
                                        whole: true,
                                        ..
                                    })
                                );
                            let key = (p.root, p.proj[..k].to_vec());
                            if thin && !st.keys.iter().any(|(x, _)| *x == key) {
                                st.keys.push((key, None));
                            }
                        }
                    }
                    st.refs.insert(v, Vec::new());
                    st.excl
                        .insert(held, ref_mutable(self.pool(), self.env(), self.ty(v)));
                }
                Bind::Share(from) | Bind::Move(from) => {
                    let Some(&x) = st.excl.get(&from) else {
                        continue;
                    };
                    st.excl.insert(held, x);
                }
                Bind::Call(_) => continue,
            }
            let i = u32::try_from(i).expect("insts");
            st.binds.entry(i).or_default().push((held, bind));
        }
        st.loans = st.keys.len();
        self.borrows = st;
    }

    /// The index of pointer `key`, interned.
    fn borrow_key(&mut self, key: Key) -> usize {
        if let Some(k) = self.borrows.keys.iter().position(|(x, _)| *x == key) {
            return k;
        }
        self.borrows.keys.push((key, None));
        self.borrows.keys.len() - 1
    }

    /// The local holding the state of the function's own loans through
    /// pointer `k`.
    fn own_local(&mut self, k: usize) -> u32 {
        if let Some(l) = self.borrows.keys[k].1 {
            return l;
        }
        let l = self.f.local(Val::I32);
        self.borrows.keys[k].1 = Some(l);
        l
    }

    /// Before instruction `i`: the borrow-state checks of the access it
    /// makes through each `*T` on its place. A `Store` or an `IsUnique`
    /// writes the place, a `Ref` lends it as its mutability says, and a
    /// `Read` reads it. A `Ref` a holder takes loans on keeps each crossed
    /// object's header address.
    // This hook sits on the recursive instruction-emission path. Outlining
    // keeps the 1,000-arm flat-chain regression within its stack bound.
    #[inline(never)]
    pub(super) fn borrow_access(&mut self, i: u32) -> StageResult<()> {
        if self.m.lent.is_empty() {
            return Ok(());
        }
        let write = match self.b.tags[i as usize] {
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
        let lends = self.borrows.refs.contains_key(&i);
        // Each dereference of a `*T`, with the type of the object it reaches.
        let root = self.at_root_ty(p.root);
        let mut crossed = Vec::new();
        for (k, step) in p.proj.iter().enumerate() {
            if *step != Proj::Deref {
                continue;
            }
            if let Kind::Ref {
                target,
                fat: false,
                whole: true,
            } = self.kind(self.walk_ty(root, &p.proj[..k])?)
            {
                crossed.push((k, target));
            }
        }
        let last = crossed.len().wrapping_sub(1);
        let mut taken = Vec::new();
        for (n, (k, object)) in crossed.into_iter().enumerate() {
            let writes = write && n == last;
            let mask = match self.m.lent.get(&object) {
                Some(_) if writes => Some(LOAN_STATE),
                Some(true) => Some(LOAN_EXCL),
                _ => None,
            };
            if mask.is_none() && !lends {
                continue;
            }
            let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                .then(|| self.borrow_key((p.root, p.proj[..k].to_vec())));
            let mut fresh = Vec::new();
            let (at, _) = self.walk_place(i, &p, k, None, &mut fresh)?;
            let o = self.at_i32(&at)?;
            self.release_fresh(&fresh)?;
            let header = self.f.local(Val::I32);
            self.f.get(o);
            self.f.u32(crate::module::HEADER - 4);
            self.f.op(I::I32Sub);
            self.f.set(header);
            if let Some(mask) = mask {
                self.borrow_check(header, mask, key, object, writes);
            }
            taken.push((header, key));
        }
        if lends {
            self.borrows.refs.insert(i, taken);
        }
        Ok(())
    }

    /// Panics `borrow-conflict` when the borrow state of the object whose
    /// header address is in local `header` differs, in the bits of
    /// `mask`, from the function's own loans through pointer `key`.
    fn borrow_check(
        &mut self,
        header: u32,
        mask: u32,
        key: Option<usize>,
        object: Ty,
        writes: bool,
    ) {
        let own = key
            .filter(|&k| k < self.borrows.loans)
            .map(|k| self.own_local(k));
        self.f.get(header);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(mask);
        self.f.op(I::I32And);
        if let Some(own) = own {
            self.f.get(own);
            if mask != LOAN_STATE {
                self.f.u32(mask);
                self.f.op(I::I32And);
            }
            self.f.op(I::I32Ne);
        }
        self.f.op(I::If(BlockType::Empty));
        let access = if writes { "written" } else { "read" };
        self.f.helper(Helper::Panic(format!(
            "borrow-conflict: a `{}` a live reference borrows was {access} through another `*T`",
            hd_mono::printed_type_name(self.pool(), self.env(), object)
        )));
        self.f.op(I::End);
    }

    /// The loan delta of slot `s` of holder `held`: the first, on the
    /// last object crossed, is the reference's kind.
    fn borrow_delta(&self, held: Held, s: usize) -> u32 {
        if s == 0 && self.borrows.excl.get(&held).copied().unwrap_or(false) {
            LOAN_EXCL
        } else {
            LOAN_SHARED
        }
    }

    /// The locals of slot `s` of holder `held`.
    fn borrow_slot(&mut self, held: Held, s: usize) -> (u32, u32) {
        while self.borrows.slots.get(&held).map_or(0, Vec::len) <= s {
            let pair = (self.f.local(Val::I32), self.f.local(Val::I32));
            self.borrows.slots.entry(held).or_default().push(pair);
        }
        self.borrows.slots[&held][s]
    }

    /// Adds `delta` (or subtracts it, with `sign` -1) to the state at the
    /// header address in local `header`, and to the function's own loans
    /// through the pointer whose index plus one local `key` holds.
    fn borrow_add(&mut self, header: u32, key: u32, delta: u32, sign: i32) {
        let op = if sign < 0 { I::I32Sub } else { I::I32Add };
        self.f.get(header);
        self.f.get(header);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(delta);
        self.f.op(op);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        for k in 0..self.borrows.loans {
            let own = self.own_local(k);
            self.f.get(key);
            self.f.u32(u32::try_from(k + 1).expect("keys"));
            self.f.op(I::I32Eq);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(own);
            self.f.u32(delta);
            self.f.op(op);
            self.f.set(own);
            self.f.op(I::End);
        }
    }

    /// After `Store` `i` into a holder: the loans of the reference it
    /// stores, a copy's own loans on its source's objects, or a move's.
    #[inline(never)]
    pub(super) fn borrow_after_store(&mut self, i: u32) {
        let Some(binds) = self.borrows.binds.get(&i).cloned() else {
            return;
        };
        for (held, bind) in binds {
            match bind {
                Bind::Begin(a) => {
                    let Some(v) = direct_value(self.b, a) else {
                        continue;
                    };
                    let v = u32::try_from(v).expect("insts");
                    let Some(taken) = self.borrows.refs.get_mut(&v).map(std::mem::take) else {
                        continue;
                    };
                    for (s, (header, key)) in taken.into_iter().rev().enumerate() {
                        let (h, kl) = self.borrow_slot(held, s);
                        let delta = self.borrow_delta(held, s);
                        self.f.get(header);
                        self.f.set(h);
                        match key {
                            Some(k) => {
                                self.own_local(k);
                                self.f.u32(u32::try_from(k + 1).expect("keys"));
                            }
                            None => self.f.i32(0),
                        }
                        self.f.set(kl);
                        self.borrow_add(h, kl, delta, 1);
                    }
                }
                Bind::Share(from) | Bind::Move(from) => {
                    let n = self.borrows.slots.get(&from).map_or(0, Vec::len);
                    for s in 0..n {
                        let (fh, fk) = self.borrows.slots[&from][s];
                        let (h, kl) = self.borrow_slot(held, s);
                        self.f.get(fh);
                        self.f.set(h);
                        self.f.get(fk);
                        self.f.set(kl);
                        if matches!(bind, Bind::Move(_)) {
                            self.f.zero(fh);
                            continue;
                        }
                        let delta = self.borrow_delta(held, s);
                        self.f.get(h);
                        self.f.op(I::If(BlockType::Empty));
                        self.borrow_add(h, kl, delta, 1);
                        self.f.op(I::End);
                    }
                }
                Bind::Call(_) => {}
            }
        }
    }

    /// Ends the loans local `l`'s holders hold, at its release or before
    /// a store replaces its value.
    pub(super) fn borrow_release(&mut self, l: usize) {
        let mut held: Vec<Held> = (self.borrows.slots.keys())
            .filter(|h| h.local == l)
            .copied()
            .collect();
        held.sort_stable();
        for held in held {
            let slots = self.borrows.slots[&held].clone();
            for (s, (h, kl)) in slots.into_iter().enumerate() {
                let delta = self.borrow_delta(held, s);
                self.f.get(h);
                self.f.op(I::If(BlockType::Empty));
                self.borrow_add(h, kl, delta, -1);
                self.f.zero(h);
                self.f.op(I::End);
            }
        }
    }

    /// Before `Store` `i`: a holder's loans end when the store replaces
    /// its value.
    #[inline(never)]
    pub(super) fn borrow_before_store(&mut self, i: u32) {
        if let Some((l, true)) = self.b.stored_local(i as usize) {
            self.borrow_release(l.idx());
        }
    }

    /// At `Drop` `i` of a whole local: its holders' loans end.
    pub(super) fn borrow_drop(&mut self, i: u32) {
        if self.borrows.slots.is_empty() {
            return;
        }
        if let Some(p) = self.b.decode_place(self.b.data[i as usize][0])
            && p.proj.is_empty()
            && let PlaceRoot::Local(l) = p.root
        {
            self.borrow_release(l.idx());
        }
    }

    /// The locals whose holders hold pointer loans.
    pub(super) fn borrow_locals(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.borrows.slots.keys().map(|h| h.local).collect();
        out.sort_stable();
        out.dedup();
        out
    }
}
