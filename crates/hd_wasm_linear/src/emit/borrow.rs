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
//! Passing a pointer is no access.
//!
//! **Call loans.** A call's argument takes a loan from its evaluation to
//! the call's return: a `Ref` that crosses a `*T`, and a `*T` that the
//! callee's parameter takes as a `&T` or `&mut T`, on its whole object. A
//! `&mut self` receiver is shared until the call begins, when it becomes
//! exclusive (two-phase). A closure argument takes over the loans of the
//! holders it captures until the call returns. Returned references and
//! suspending bodies take no loan yet (phases 5 and 6).

use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_mono::{ProgramEnv, Target};
use hd_tir::ir::{Body, PlaceRoot, Proj, Tag};
use hd_types::{InternPool, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::excl::{Bind, Held, binds, call_args, call_info, direct_value};
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
    /// crossed, once emitted: the header address, the pointer's key, and
    /// whether elision keeps that loan (`elide.rs`).
    refs: HashMap<u32, Vec<(u32, Option<usize>, bool)>>,
    /// Each holder's loans, the last crossing first: the local of the
    /// object's header address (0 while the holder holds none), and the
    /// local of its key's index plus one.
    slots: HashMap<Held, Vec<(u32, u32)>>,
    /// Whether a holder holds a `&mut`: its first loan is exclusive.
    excl: HashMap<Held, bool>,
    /// The pointers places reach objects through, and the local holding
    /// the state of the function's own loans through each, once used.
    keys: Vec<(Key, Option<u32>)>,
    /// The first `loans` keys are those some holder's or call's loan is
    /// taken through, interned before emission.
    loans: usize,
    /// The `Ref` arguments that take a loan for their call: the call, and
    /// whether the reference is a `&mut self` receiver, shared until the
    /// call begins.
    call_refs: HashMap<u32, (u32, bool)>,
    /// The `*T` values a call lends as a `&` or `&mut`, an auto-ref
    /// receiver's included: the call, whether it lends `&mut`, and whether
    /// it is a `&mut self` receiver, shared until the call begins.
    call_ptrs: HashMap<u32, (u32, bool, bool)>,
    /// The closure arguments of calls: a holder a closure captures lends
    /// its loans to the call that receives the closure.
    call_closures: HashMap<u32, u32>,
    /// Each call's loans, once taken: the header address's local, the
    /// pointer's key index plus one (a constant, or the local holding it),
    /// the delta, and whether it becomes exclusive when the call begins.
    call_loans: HashMap<u32, Vec<CallLoan>>,
    /// Every call loan taken, for the function's exit.
    all_calls: Vec<CallLoan>,
}

/// One loan a call holds from its argument's evaluation to its return.
#[derive(Clone, Copy)]
struct CallLoan {
    header: u32,
    key: KeyOf,
    delta: u32,
    upgrade: bool,
    /// Whether a jump may leave between the loan's begin and its call's
    /// return: the function's exit then ends it if it is still held.
    exit: bool,
}

/// The pointer a loan was taken through: known, unknown, or held in a
/// local as its index plus one.
#[derive(Clone, Copy)]
enum KeyOf {
    Known(Option<usize>),
    Local(u32),
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
pub(super) fn crossings(
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
pub(super) fn root_ty(b: &Body, sub: &dyn Fn(Ty) -> Ty, root: PlaceRoot) -> Option<Ty> {
    match root {
        PlaceRoot::Local(l) => Some(sub(b.local_ty[l.idx()])),
        PlaceRoot::Global(_, t) => Some(sub(t)),
        PlaceRoot::Value(r) => r.as_inst().map(|v| sub(b.ty[v.0 as usize])),
        PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
    }
}

/// The parameter types of a call to `target`, as the callee instance's
/// own body has them ([`crate::emit::callee_params`]): a direct call's.
pub(super) fn call_params(
    input: &crate::Input<'_>,
    m: &Module,
    target: Option<&Target>,
) -> Vec<Ty> {
    match target {
        Some(Target::Call(c)) => super::callee_params(input, m, c.key, c.item, c.args),
        _ => Vec::new(),
    }
}

/// Whether argument `k` of a call, declared as `param` and made of a
/// value of type `value`, lends a whole `*T` object as a local reference:
/// a `*T` where the parameter is `&T` or `&mut T`, or an auto-ref receiver
/// (`receiver`, its mutability). Gives whether it lends `&mut`, and
/// whether that is a two-phase receiver.
pub(super) fn pointer_lent(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    param: Option<Ty>,
    value: Ty,
    k: usize,
    receiver: Option<bool>,
) -> Option<(bool, bool)> {
    let Kind::Ref {
        whole: true,
        fat: false,
        ..
    } = kind(pool, env, value)
    else {
        return None;
    };
    if let (0, Some(m)) = (k, receiver) {
        return Some((m, m));
    }
    let param = param?;
    match kind(pool, env, param) {
        Kind::Ref {
            whole: false,
            fat: false,
            ..
        } => Some((ref_mutable(pool, env, param), false)),
        _ => None,
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
                    self.intern_ref_keys(&mut st, v);
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
        // Call loans: a `Ref` argument, and a closure argument's captures.
        for i in 0..self.b.tags.len() {
            let call = u32::try_from(i).expect("insts");
            let Some(receiver) = call_info(self.input, self.b, i, self.calls.get(&call)) else {
                continue;
            };
            let params = call_params(self.input, self.m, self.calls.get(&call));
            for (k, a) in call_args(self.b, i).into_iter().enumerate() {
                let Some(v) = direct_value(self.b, a) else {
                    continue;
                };
                let v = u32::try_from(v).expect("insts");
                match self.b.tags[v as usize] {
                    Tag::Ref if !st.refs.contains_key(&v) => {
                        let two_phase = k == 0
                            && receiver == Some(true)
                            && ref_mutable(self.pool(), self.env(), self.ty(v));
                        self.intern_ref_keys(&mut st, v);
                        st.call_refs.insert(v, (call, two_phase));
                    }
                    Tag::Closure => {
                        st.call_closures.insert(v, call);
                    }
                    Tag::Read => {
                        let pool = self.pool();
                        let env = self.env();
                        let param = params.get(k).copied();
                        let lent = pointer_lent(pool, env, param, self.ty(v), k, receiver);
                        if let Some((mutable, two_phase)) = lent {
                            if let Some(p) = self.b.decode_place(self.b.data[v as usize][0])
                                && matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                            {
                                let key = (p.root, p.proj);
                                if !st.keys.iter().any(|(x, _)| *x == key) {
                                    st.keys.push((key, None));
                                }
                            }
                            st.call_ptrs.insert(v, (call, mutable, two_phase));
                        }
                    }
                    _ => {}
                }
            }
        }
        st.loans = st.keys.len();
        self.borrows = st;
    }

    /// Interns the pointers `Ref` `v`'s place reaches objects through.
    fn intern_ref_keys(&self, st: &mut Borrows, v: u32) {
        let Some(p) = self.b.decode_place(self.b.data[v as usize][0]) else {
            return;
        };
        if !matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..)) {
            return;
        }
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
        if let Some(&call) = self.borrows.call_closures.get(&i) {
            self.borrow_lend_captures(i, call);
            return Ok(());
        }
        if let Some(&lent) = self.borrows.call_ptrs.get(&i) {
            self.borrow_lend_pointer(i, lent)?;
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
        let call = self.borrows.call_refs.get(&i).copied();
        let lends = self.borrows.refs.contains_key(&i) || call.is_some();
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
        // A two-phase receiver is shared until its call begins. One whose
        // loan elision drops takes the access check of its `&mut` now.
        let last_kept = crossed.last().is_some_and(|&(k, _)| {
            (self.m.kept).contains(&(self.inst, i, u32::try_from(k).expect("steps")))
        });
        let write = write && !(call.is_some_and(|(_, two_phase)| two_phase) && last_kept);
        let mut taken = Vec::new();
        for (n, (k, object)) in crossed.into_iter().enumerate() {
            let writes = write && n == last;
            let mask = match self.m.lent.get(&object) {
                Some(_) if writes => Some(LOAN_STATE),
                Some(true) => Some(LOAN_EXCL),
                _ => None,
            };
            let keep =
                lends && (self.m.kept).contains(&(self.inst, i, u32::try_from(k).expect("steps")));
            let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                .then(|| self.borrow_key((p.root, p.proj[..k].to_vec())));
            if mask.is_none() && !keep {
                if lends {
                    taken.push((u32::MAX, key, false));
                }
                continue;
            }
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
            taken.push((header, key, keep));
        }
        if let Some((call, two_phase)) = call {
            let n = taken.len();
            for (j, (header, key, keep)) in taken.into_iter().enumerate() {
                if !keep {
                    continue;
                }
                let last = j + 1 == n;
                let delta = if last && write {
                    LOAN_EXCL
                } else {
                    LOAN_SHARED
                };
                let loan = CallLoan {
                    header,
                    key: KeyOf::Known(key),
                    delta,
                    upgrade: last && two_phase,
                    exit: self.jumps_between(i, call),
                };
                self.call_loan_add(loan, 1);
                self.borrows.call_loans.entry(call).or_default().push(loan);
                if loan.exit {
                    self.borrows.all_calls.push(loan);
                }
            }
        } else if lends {
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
                    for (s, (header, key, keep)) in taken.into_iter().rev().enumerate() {
                        if !keep {
                            continue;
                        }
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

    /// Adds (or with `sign` -1 subtracts) call loan `loan`'s delta to its
    /// object's state and the function's own loans through its pointer.
    fn call_loan_add(&mut self, loan: CallLoan, sign: i32) {
        match loan.key {
            KeyOf::Local(kl) => self.borrow_add(loan.header, kl, loan.delta, sign),
            KeyOf::Known(key) => {
                let op = if sign < 0 { I::I32Sub } else { I::I32Add };
                self.f.get(loan.header);
                self.f.get(loan.header);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                self.f.u32(loan.delta);
                self.f.op(op);
                self.f.op(I::I32Store(mem(HEAP, 0, 2)));
                if let Some(k) = key.filter(|&k| k < self.borrows.loans) {
                    let own = self.own_local(k);
                    self.f.get(own);
                    self.f.u32(loan.delta);
                    self.f.op(op);
                    self.f.set(own);
                }
            }
        }
    }

    /// At closure `c`, an argument of `call`: the loans of the holders it
    /// captures pass to the call, which ends them when it returns. The
    /// holder's own release then ends nothing.
    fn borrow_lend_captures(&mut self, c: u32, call: u32) {
        let rec = self.b.record(self.b.data[c as usize][1]);
        let (Some(&start), Some(&count)) = (rec.first(), rec.get(1)) else {
            return;
        };
        let captured: Vec<usize> = (start..start + count)
            .filter_map(|x| self.b.cap_local.get(x as usize).map(|l| l.idx()))
            .collect();
        for l in captured {
            let mut held: Vec<Held> = (self.borrows.slots.keys())
                .filter(|h| h.local == l)
                .copied()
                .collect();
            held.sort_stable();
            for held in held {
                let slots = self.borrows.slots[&held].clone();
                for (s, (h, kl)) in slots.into_iter().enumerate() {
                    let (header, key) = (self.f.local(Val::I32), self.f.local(Val::I32));
                    self.f.get(h);
                    self.f.set(header);
                    self.f.get(kl);
                    self.f.set(key);
                    self.f.zero(h);
                    let delta = self.borrow_delta(held, s);
                    let loan = CallLoan {
                        header,
                        key: KeyOf::Local(key),
                        delta,
                        upgrade: false,
                        exit: self.jumps_between(c, call),
                    };
                    self.borrows.call_loans.entry(call).or_default().push(loan);
                    if loan.exit {
                        self.borrows.all_calls.push(loan);
                    }
                }
            }
        }
    }

    /// At `Read` `i` of a `*T` that `call` lends as a local reference: a
    /// loan on its whole object, exclusive for a `&mut` that is no
    /// two-phase receiver, else shared, from here to the call's return.
    fn borrow_lend_pointer(
        &mut self,
        i: u32,
        (call, mutable, two_phase): (u32, bool, bool),
    ) -> StageResult<()> {
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(());
        };
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return Ok(());
        }
        let Kind::Ref { target, .. } = self.kind(self.ty(i)) else {
            return Ok(());
        };
        if !self.m.lent.contains_key(&target) {
            return Ok(());
        }
        let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            .then(|| self.borrow_key((p.root, p.proj.clone())));
        let mut fresh = Vec::new();
        let (at, _) = self.walk_place(i, &p, p.proj.len(), None, &mut fresh)?;
        let o = self.at_i32(&at)?;
        self.release_fresh(&fresh)?;
        let header = self.f.local(Val::I32);
        self.f.get(o);
        self.f.u32(crate::module::HEADER - 4);
        self.f.op(I::I32Sub);
        self.f.set(header);
        if !self.m.kept.contains(&(self.inst, i, u32::MAX)) {
            // No loan: only the access check of the lend, a write for a
            // `&mut`, whose two-phase window nothing can meet.
            if mutable || self.m.lent.get(&target) == Some(&true) {
                let mask = if mutable { LOAN_STATE } else { LOAN_EXCL };
                self.borrow_check(header, mask, key, target, mutable);
            }
            return Ok(());
        }
        let excl = mutable && !two_phase;
        let mask = if excl { LOAN_STATE } else { LOAN_EXCL };
        self.borrow_check(header, mask, key, target, excl);
        let loan = CallLoan {
            header,
            key: KeyOf::Known(key),
            delta: if excl { LOAN_EXCL } else { LOAN_SHARED },
            upgrade: two_phase,
            exit: self.jumps_between(i, call),
        };
        self.call_loan_add(loan, 1);
        self.borrows.call_loans.entry(call).or_default().push(loan);
        if loan.exit {
            self.borrows.all_calls.push(loan);
        }
        Ok(())
    }

    /// When call `i` begins, after its arguments: a two-phase receiver's
    /// shared loan becomes exclusive, which needs no loan but the
    /// function's own through its pointer.
    pub(super) fn borrow_call_begin(&mut self, i: u32) {
        let Some(loans) = self.borrows.call_loans.get(&i).cloned() else {
            return;
        };
        let mut out = Vec::new();
        for mut loan in loans {
            if loan.upgrade {
                let KeyOf::Known(key) = loan.key else {
                    continue;
                };
                let own = key
                    .filter(|&k| k < self.borrows.loans)
                    .map(|k| self.own_local(k));
                self.f.get(loan.header);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                self.f.u32(LOAN_STATE);
                self.f.op(I::I32And);
                match own {
                    Some(own) => self.f.get(own),
                    None => self.f.u32(LOAN_SHARED),
                }
                self.f.op(I::I32Ne);
                self.f.op(I::If(BlockType::Empty));
                self.f.helper(Helper::Panic(
                    "borrow-conflict: a receiver a live reference borrows was lent as `&mut self`"
                        .into(),
                ));
                self.f.op(I::End);
                self.call_loan_add(loan, -1);
                loan.delta = LOAN_EXCL;
                self.call_loan_add(loan, 1);
            }
            out.push(loan);
        }
        self.borrows.call_loans.insert(i, out);
    }

    /// After call `i` returns: the loans it held end.
    pub(super) fn borrow_call_end(&mut self, i: u32) {
        let Some(loans) = self.borrows.call_loans.remove(&i) else {
            return;
        };
        for loan in loans.into_iter().rev() {
            // A closure's lent holder may hold no loan, so its end tests.
            if loan.exit || matches!(loan.key, KeyOf::Local(_)) {
                self.borrow_end_call_loan(loan);
            } else {
                self.call_loan_add(loan, -1);
            }
        }
    }

    /// At the function's exit: the call loans an early exit left between an
    /// argument's evaluation and its call's return end.
    pub(super) fn borrow_exit(&mut self) {
        for loan in self.borrows.all_calls.clone() {
            self.borrow_end_call_loan(loan);
        }
    }

    /// Whether a `break`, `continue` or `return` lies between instruction
    /// `from` and call `call`, so a loan begun at `from` may outlive its
    /// call's emitted end.
    fn jumps_between(&self, from: u32, call: u32) -> bool {
        (from + 1..call).any(|j| {
            matches!(
                self.b.tags[j as usize],
                Tag::Break | Tag::Continue | Tag::Return
            )
        })
    }

    /// Ends call loan `loan` if it is still held, and clears it.
    fn borrow_end_call_loan(&mut self, loan: CallLoan) {
        self.f.get(loan.header);
        self.f.op(I::If(BlockType::Empty));
        self.call_loan_add(loan, -1);
        self.f.zero(loan.header);
        self.f.op(I::End);
    }

    /// The locals whose holders hold pointer loans.
    pub(super) fn borrow_locals(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.borrows.slots.keys().map(|h| h.local).collect();
        out.sort_stable();
        out.dedup();
        out
    }
}
