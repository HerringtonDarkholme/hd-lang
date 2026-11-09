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
//! **Root objects.** A variable a closure shares lives in a cell, an
//! object with a descriptor, which a place rooted at the variable crosses
//! before its first step ([`ROOT`]): a reference into the variable takes
//! a loan on the cell, and each use of it, in the closure or in its own
//! function, checks it, so a closure cannot change a variable under a
//! reference. A top-level binding that is an object ([`Module::objects`])
//! is crossed the same way, at its static slot.
//!
//! **Call loans.** A call's argument takes a loan from its evaluation to
//! the call's return: a `Ref` that crosses a `*T`, and a `*T` that the
//! callee's parameter takes as a `&T` or `&mut T`, on its whole object. A
//! `&mut self` receiver is shared until the call begins, when it becomes
//! exclusive (two-phase). A closure argument takes over the loans of the
//! holders it captures until the call returns. A holder of a call's
//! result takes loans on the argument's objects and on the object the
//! result points into; a reborrow through a holder takes its loans.
//!
//! **Suspension.** A suspending body's frame saves every local, the
//! loans' too, so a loan held across a suspension stays held while the
//! task waits. A task that unwinds ends its loans at its exits; one whose
//! frame is released while it waits ends them in its release glue
//! ([`Em::borrow_abandon`]), so no object stays borrowed after it.

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
/// steps to the pointer value. A root object's key ([`ROOT`]) has no
/// steps and names the inlined copy the access came from
/// ([`frame_of`]), since the checker saw each copy's own references
/// apart from the caller's; a pointer's key names [`ANY_FRAME`], as an
/// inlined copy reaches a pointer through locals of its own.
type Key = (PlaceRoot, Vec<Proj>, u32);

/// The frame of a pointer's key ([`Key`]).
const ANY_FRAME: u32 = u32::MAX;

/// The inlined copy each instruction of `b` came from: its anchor's row
/// in [`Body::inlined`], or `ANY_FRAME - 1` for the body's own. An
/// instruction the passes added takes the next one's, as an `IsUnique`
/// stands before the change it guards.
fn frames_of(b: &Body) -> Vec<u32> {
    const OWN: u32 = ANY_FRAME - 1;
    let n = b.tags.len();
    let mut out = vec![OWN; n];
    let mut next = OWN;
    for i in (0..n).rev() {
        let a = b.anchors.get(i).copied().unwrap_or(hd_tir::ir::NONE);
        if a != hd_tir::ir::NONE {
            next = if a & hd_tir::ir::INLINED == 0 {
                OWN
            } else {
                let a = a & !hd_tir::ir::INLINED;
                let k = b.inlined.partition_point(|e| e[0] <= a);
                u32::try_from(k).expect("inlined copies")
            };
        }
        out[i] = next;
    }
    out
}

/// The per-body state of the loans its holders take.
#[derive(Default)]
pub(super) struct Borrows {
    /// The holder bindings each `Store` makes.
    binds: HashMap<u32, Vec<(Held, Bind)>>,
    /// The `Ref`s whose loans a holder takes, and the objects each one
    /// crossed, once emitted: the header address, the pointer's key, and
    /// whether elision keeps that loan (`hd_tir::own::pointer`).
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
    /// The arguments whose call's result a holder binds: their crossed
    /// objects' header addresses are kept at their evaluation.
    call_holds: hd_base::HashSet<u32>,
    /// Those header addresses, once emitted: the local, the pointer's key
    /// and the dereference index (`u32::MAX` for a whole `*T`).
    arg_heads: HashMap<u32, Vec<(u32, Option<usize>, u32)>>,
    /// The `Ref`s that reborrow through a holder local, as an inlined
    /// getter's result does through its `self`: the holder's local.
    reborrows: HashMap<u32, usize>,
    /// Their source's loans, once emitted, by the source's slot: the
    /// header's and key's copies, and the source's delta when the loan was
    /// exclusive and so passed over rather than shared.
    reborrowed: HashMap<u32, Vec<(u32, u32, Option<u32>)>>,
    /// Set once a frame's release glue is emitted ([`Em::borrow_abandon`]):
    /// the body's locals are gone, and a cold frame holds no loan.
    abandoned: bool,
    /// The locals a closure shares: each lives in a cell, an object its
    /// places cross at their root ([`ROOT`]).
    shared: hd_base::HashSet<usize>,
    /// The inlined copy each instruction came from ([`frames_of`]).
    frames: Vec<u32>,
    /// The local references that hold only `*T` values
    /// ([`pointer_locals`]).
    pointers: hd_base::HashSet<usize>,
    /// The locals a match binds from a part of a call's result, by the
    /// value the match decides on: their loans end with the match, or
    /// when that value is dropped, as such a binding borrows it and has no
    /// `Drop` of its own.
    via_call: HashMap<u32, Vec<usize>>,
    /// The `Ref`s each call's arguments carry through calls' results, and
    /// whether each is `&mut`: their loans run while the call does
    /// ([`super::excl::carried_refs`]).
    carried: HashMap<u32, Vec<(u32, bool)>>,
    /// The `*T` locals a closure of the body captures with its creator's
    /// own loans through them ([`Em::own_caps`]).
    own_shared: hd_base::HashSet<usize>,
    /// The cell holding the own loans through each of them: the local of
    /// its address, and whether this body holds a count of it, which its
    /// end or abandonment releases: one it made, or a suspending closure's,
    /// whose frame counts its environment's cells.
    own_cells: HashMap<usize, (u32, bool)>,
    /// The header locals of call loans on a `*dyn` receiver's object,
    /// which are 0 where its data address is (a trait value of no Wasm
    /// values): no loan is taken then.
    nullable: hd_base::HashSet<u32>,
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
    walk_crossings(pool, env, root, steps).0
}

/// [`crossings`], and the type `steps` reach when the walk types them all.
fn walk_crossings(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    root: Ty,
    steps: &[Proj],
) -> (Vec<(usize, Ty)>, Option<Ty>) {
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
                _ => None,
            },
        };
        let Some(next) = next else {
            return (out, None);
        };
        ty = next;
        k += 1;
    }
    (out, (k == steps.len()).then_some(ty))
}

/// The trait of a call's receiver read `v` that reads a trait value
/// through a `*dyn Tr` or `*mut dyn Tr` (`Read [*$t]`): such a receiver
/// lends the object at the pointer's data address, of one of the types
/// [`Module::dyn_types`] lists for the trait.
pub(super) fn dyn_receiver(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    (b, sub): (&Body, &dyn Fn(Ty) -> Ty),
    v: usize,
) -> Option<hd_base::DefId> {
    if b.tags[v] != Tag::Read {
        return None;
    }
    let p = b.decode_place(b.data[v][0])?;
    let (Some(Proj::Deref), Some(root)) = (p.proj.last(), root_ty(b, sub, p.root)) else {
        return None;
    };
    let pointer = walk_crossings(pool, env, root, &p.proj[..p.proj.len() - 1]).1?;
    let Kind::Ref {
        target,
        fat: true,
        whole: true,
    } = kind(pool, env, pointer)
    else {
        return None;
    };
    match pool.get(strip(pool, env, target)) {
        TyData::TraitValue { def, .. } => Some(def),
        _ => None,
    }
}

/// The step index of a place's root object: a variable a closure shares
/// (its cell) or a top-level binding that is an object, which the place
/// crosses before its first step.
pub(super) const ROOT: usize = u32::MAX as usize - 2;

/// The local or top-level binding place `p` starts from: its root, or the
/// root of the read its root value is, as a loop over `&mut xs` makes.
fn root_binding(b: &Body, p: &hd_tir::ir::Place) -> Option<PlaceRoot> {
    let root = match p.root {
        PlaceRoot::Value(r) => {
            let x = r.as_inst()?.0 as usize;
            if b.tags[x] != Tag::Read {
                return None;
            }
            b.decode_place(b.data[x][0])?.root
        }
        root => root,
    };
    matches!(root, PlaceRoot::Local(_) | PlaceRoot::Global(..)).then_some(root)
}

/// The top-level binding place `p` starts from ([`root_binding`]).
pub(crate) fn global_root(b: &Body, p: &hd_tir::ir::Place) -> Option<(hd_base::DefId, Ty)> {
    match root_binding(b, p)? {
        PlaceRoot::Global(def, ty) => Some((def, ty)),
        _ => None,
    }
}

/// The objects a place may start at: the locals a closure shares, and the
/// top-level bindings that are objects ([`Module::objects`]).
pub(super) struct Roots<'a> {
    pub(super) shared: &'a hd_base::HashSet<usize>,
    /// The local references that hold only `*T` values ([`pointer_locals`]):
    /// a place through one crosses the object it points to.
    pub(super) pointers: &'a hd_base::HashSet<usize>,
    pub(super) objects: &'a hd_base::HashSet<hd_base::Hash128>,
    pub(super) env: &'a dyn ProgramEnv,
}

impl Roots<'_> {
    /// The object place `p` crosses at its root, as the root it is keyed
    /// by, and its type before substitution.
    pub(super) fn of(&self, b: &Body, p: &hd_tir::ir::Place) -> Option<(PlaceRoot, Ty)> {
        match root_binding(b, p)? {
            PlaceRoot::Local(l) if self.shared.contains(&l.idx()) => {
                Some((PlaceRoot::Local(l), b.local_ty[l.idx()]))
            }
            PlaceRoot::Global(def, ty) if self.objects.contains(&self.env.path_hash(def)) => {
                Some((PlaceRoot::Global(def, ty), ty))
            }
            _ => None,
        }
    }
}

/// The step index of a dereference on the place of the read a place's
/// root value is ([`root_read`]): `READ + k` for its step `k`.
pub(super) const READ: usize = 1 << 30;

/// The read whose value place `p` starts at, and that read's place, as a
/// loop over `&mut p.items` makes: the read's dereferences are crossed
/// too.
pub(super) fn root_read(b: &Body, p: &hd_tir::ir::Place) -> Option<(u32, hd_tir::ir::Place)> {
    let PlaceRoot::Value(r) = p.root else {
        return None;
    };
    let r = r.as_inst()?.0;
    if b.tags.get(r as usize) != Some(&Tag::Read) {
        return None;
    }
    let q = b.decode_place(b.data[r as usize][0])?;
    matches!(q.root, PlaceRoot::Local(_) | PlaceRoot::Global(..)).then_some((r, q))
}

/// [`crossings`], led by the place's root object ([`Roots::of`]) and by
/// the dereferences of the read its root value is ([`root_read`]).
pub(super) fn place_crossings(
    pool: &InternPool,
    (b, sub): (&Body, &dyn Fn(Ty) -> Ty),
    roots: &Roots<'_>,
    root: Ty,
    p: &hd_tir::ir::Place,
) -> Vec<(usize, Ty)> {
    let mut out = Vec::new();
    if let Some((_, t)) = roots.of(b, p) {
        out.push((ROOT, sub(t)));
    }
    if let Some(t) = pointer_root(pool, roots.env, (b, sub), roots.pointers, p) {
        out.push((0, t));
    }
    if let Some((_, q)) = root_read(b, p)
        && let Some(qt) = root_ty(b, sub, q.root)
    {
        out.extend(
            (crossings(pool, roots.env, qt, &q.proj).into_iter()).map(|(k, t)| (READ + k, t)),
        );
    }
    out.extend(crossings(pool, roots.env, root, &p.proj));
    out
}

/// The local references of `b` that hold only whole `*T` values: every
/// store of all of one stores a `*T` read (through a same-value coercion),
/// as an inlined callee's `&T` parameter bound to a `*T` argument does.
/// Such a reference is the address of the object, so a place through it
/// crosses that object.
pub(super) fn pointer_locals(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    sub: &dyn Fn(Ty) -> Ty,
) -> hd_base::HashSet<usize> {
    let thin_local = |t: Ty| {
        matches!(
            kind(pool, env, t),
            Kind::Ref {
                whole: false,
                fat: false,
                ..
            }
        )
    };
    let whole_ptr = |t: Ty| {
        matches!(
            kind(pool, env, t),
            Kind::Ref {
                whole: true,
                fat: false,
                ..
            }
        )
    };
    let mut ok: HashMap<usize, bool> = HashMap::default();
    for i in 0..b.tags.len() {
        let Some((l, true)) = b.stored_local(i) else {
            continue;
        };
        let l = l.idx();
        if !thin_local(sub(b.local_ty[l])) {
            continue;
        }
        let pointer = direct_value(b, b.data[i][1]).is_some_and(|v| whole_ptr(sub(b.ty[v])));
        let e = ok.entry(l).or_insert(true);
        *e &= pointer;
    }
    ok.into_iter().filter(|(_, y)| *y).map(|(l, _)| l).collect()
}

/// The object a place starting `*$l` crosses when `l` holds only `*T`
/// values ([`pointer_locals`]): the type the reference refers to.
fn pointer_root(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    (b, sub): (&Body, &dyn Fn(Ty) -> Ty),
    pointers: &hd_base::HashSet<usize>,
    p: &hd_tir::ir::Place,
) -> Option<Ty> {
    let PlaceRoot::Local(l) = p.root else {
        return None;
    };
    if p.proj.first() != Some(&Proj::Deref) || !pointers.contains(&l.idx()) {
        return None;
    }
    match kind(pool, env, sub(b.local_ty[l.idx()])) {
        Kind::Ref { target, .. } => Some(target),
        _ => None,
    }
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
    /// program lends any object.
    pub(super) fn find_borrows(&mut self) {
        if self.m.lent.is_empty() {
            return;
        }
        let sub = |t| self.sub(t);
        let found = binds(self.pool(), self.env(), self.b, &sub, true);
        let mut st = Borrows {
            shared: super::closure::shared_locals(self.b),
            frames: frames_of(self.b),
            pointers: pointer_locals(self.pool(), self.env(), self.b, &sub),
            ..Borrows::default()
        };
        for (i, held, bind, _) in found {
            if self.is_cell(held.local) {
                continue;
            }
            // A supplementary inlined parent has no access check of its
            // own. If the link pass elided all its loans, record nothing.
            if matches!(bind, Bind::CallParent(_))
                && !self
                    .m
                    .kept
                    .iter()
                    .any(|&(id, at, k)| id == self.inst && at as usize == i && k < u32::MAX - 1)
            {
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
                Bind::Call(a) | Bind::CallParent(a) => {
                    // A holder of a call's result, or of a part of it a
                    // match binds, that holds argument `a`: the result's
                    // loans (phase 5).
                    let stored = self.b.data[i][1];
                    let call = self
                        .b
                        .held_args
                        .iter()
                        .find(|x| x.1 == a)
                        .map(|x| x.0 as usize);
                    let (Some(c), Some(v), Some(s)) =
                        (call, direct_value(self.b, a), direct_value(self.b, stored))
                    else {
                        continue;
                    };
                    let (c, v) = (
                        u32::try_from(c).expect("insts"),
                        u32::try_from(v).expect("insts"),
                    );
                    if !self.b.held_args.contains(&(c, a)) {
                        continue;
                    }
                    if self.b.tags[v as usize] == Tag::Ref {
                        self.intern_ref_keys(&mut st, v);
                    }
                    st.call_holds.insert(v);
                    if s != c as usize {
                        // Keyed by the value the binding's read projects,
                        // which the match decides on: the call, or the
                        // block an inlined getter leaves its result in.
                        let from = (self.b.tags[s] == Tag::Read)
                            .then(|| self.b.decode_place(self.b.data[s][0]))
                            .flatten()
                            .and_then(|p| match p.root {
                                PlaceRoot::Value(r) => r.as_inst().map(|r| r.0),
                                _ => None,
                            })
                            .unwrap_or(c);
                        st.via_call.entry(from).or_default().push(held.local);
                    }
                    let s = u32::try_from(s).expect("insts");
                    let sub = |t| self.sub(t);
                    let excl = [c, s]
                        .iter()
                        .any(|&x| ref_mutable(self.pool(), self.env(), self.ty(x)))
                        || super::excl::held_mutable(self.pool(), self.env(), self.b, &sub, held);
                    st.excl.insert(held, excl);
                }
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
                        let sub = |t| self.sub(t);
                        let dyn_recv = dyn_receiver(pool, env, (self.b, &sub), v as usize);
                        let lent = pointer_lent(pool, env, param, self.ty(v), k, receiver).or(
                            match (k, receiver, dyn_recv) {
                                (0, Some(m), Some(_)) => Some((m, m)),
                                _ => None,
                            },
                        );
                        if let Some((mutable, two_phase)) = lent {
                            if let Some(mut p) = self.b.decode_place(self.b.data[v as usize][0])
                                && matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                            {
                                // A `*dyn` receiver's pointer is the place
                                // before its dereference.
                                if dyn_recv.is_some() {
                                    p.proj.pop();
                                }
                                let key = (p.root, p.proj, ANY_FRAME);
                                if !st.keys.iter().any(|(x, _)| *x == key) {
                                    st.keys.push((key, None));
                                }
                            }
                            st.call_ptrs.insert(v, (call, mutable, two_phase));
                        }
                    }
                    _ => {
                        let sub = |t| self.sub(t);
                        let carried = super::excl::carried_refs(
                            self.pool(),
                            self.env(),
                            self.b,
                            &sub,
                            v as usize,
                        );
                        for r in carried {
                            self.intern_ref_keys(&mut st, r);
                            st.call_holds.insert(r);
                            let excl = ref_mutable(self.pool(), self.env(), self.ty(r));
                            st.carried.entry(call).or_default().push((r, excl));
                        }
                    }
                }
            }
        }
        for &v in st.refs.keys() {
            let Some(p) = self.b.decode_place(self.b.data[v as usize][0]) else {
                continue;
            };
            if let PlaceRoot::Local(l) = p.root
                && p.proj.first() == Some(&Proj::Deref)
                && st.excl.keys().any(|h| h.local == l.idx())
            {
                st.reborrows.insert(v, l.idx());
            }
        }
        st.loans = st.keys.len();
        for ci in 0..self.b.tags.len() {
            if self.b.tags[ci] == Tag::Closure {
                st.own_shared.extend(self.own_caps(ci));
            }
        }
        self.borrows = st;
    }

    /// The captures of closure `ci` that carry its creator's own loans
    /// (`POINTER_LOANS.md` E.2): each `*T` local whose pointee a loan may
    /// hold. The closure checks an access through one against the loans
    /// its creator holds through it, which a cell in its environment
    /// keeps, as the creator's own local does.
    pub(super) fn own_caps(&self, ci: usize) -> Vec<usize> {
        if self.m.lent.is_empty() {
            return Vec::new();
        }
        (super::closure::captures(self.b, ci).into_iter())
            .map(|(l, _)| l)
            .filter(|&l| {
                matches!(self.kind(self.sub(self.b.local_ty[l])), Kind::Ref {
                    target,
                    fat: false,
                    whole: true,
                } if self.lent_of(target).is_some())
            })
            .collect()
    }

    /// The index of pointer local `l`'s own key, when the body takes a
    /// loan through it.
    fn own_key(&self, l: usize) -> Option<usize> {
        let k = (self.borrows.keys.iter()).position(|((r, proj, f), _)| {
            matches!(r, PlaceRoot::Local(x) if x.idx() == l) && proj.is_empty() && *f == ANY_FRAME
        })?;
        (k < self.borrows.loans).then_some(k)
    }

    /// The local holding the address of the cell of the own loans through
    /// captured pointer local `l`, 0 until the body makes it.
    fn own_cell(&mut self, l: usize) -> u32 {
        if let Some(&(c, _)) = self.borrows.own_cells.get(&l) {
            return c;
        }
        let c = self.f.local(Val::I32);
        self.borrows.own_cells.insert(l, (c, true));
        c
    }

    /// At a closure that captures pointer local `l`: the cell of the own
    /// loans through it, for its environment, which takes a count of it.
    /// The body makes it once, at its first such closure, and keeps it
    /// current as its own loans change, across suspensions too.
    pub(super) fn own_cell_for(&mut self, l: usize) -> StageResult<u32> {
        let own = self.own_key(l).map(|k| self.own_local(k));
        let m = crate::layout::mem(self.pool(), self.env(), Ty::I32)?;
        let c = self.own_cell(l);
        if self.borrows.own_cells.get(&l).is_some_and(|x| !x.1) {
            return Ok(c);
        }
        self.f.get(c);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        let at = self.object(&m, None, false, false, None);
        self.f.get(at);
        self.f.set(c);
        self.f.get(c);
        match own {
            Some(own) => self.f.get(own),
            None => self.f.i32(0),
        }
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.op(I::End);
        Ok(c)
    }

    /// In closure `ci`'s prologue: the cells of the own loans through
    /// the pointers it captures, from the environment's slots `cells`, and
    /// its own loans through each start as its creator's; `held` when its
    /// frame holds a count of each.
    pub(super) fn own_cells_load(&mut self, ci: usize, cells: &[u32], held: bool) {
        for (l, c) in self.own_caps(ci).into_iter().zip(cells.iter().copied()) {
            self.borrows.own_cells.insert(l, (c, held));
            if let Some(k) = self.own_key(l) {
                let own = self.own_local(k);
                self.f.get(c);
                self.f.op(I::I32Load(mem(HEAP, 0, 2)));
                self.f.set(own);
            }
        }
    }

    /// The cell of own loans through key `k`'s pointer, when a closure
    /// of the body captures that pointer.
    fn own_cell_of_key(&mut self, k: usize) -> Option<u32> {
        let l = match &self.borrows.keys[k].0 {
            (PlaceRoot::Local(x), proj, f) if proj.is_empty() && *f == ANY_FRAME => x.idx(),
            _ => return None,
        };
        self.borrows
            .own_shared
            .contains(&l)
            .then(|| self.own_cell(l))
    }

    /// The local of the own loans through key `k`, read again from their
    /// cell where one is made: the creator and its closures, suspending
    /// ones too, change one count (E.2).
    fn own_read(&mut self, k: usize) -> u32 {
        let own = self.own_local(k);
        if let Some(c) = self.own_cell_of_key(k) {
            self.f.get(c);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(c);
            self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            self.f.set(own);
            self.f.op(I::End);
        }
        own
    }

    /// The body's locals for captured pointer `l`'s own loans, for a
    /// suspending closure's frame: its cell's (`true`) and its own count's.
    pub(super) fn own_frame_locals(&self, l: usize) -> Vec<(u32, bool)> {
        let mut out: Vec<(u32, bool)> = self
            .borrows
            .own_cells
            .get(&l)
            .map(|x| (x.0, true))
            .into_iter()
            .collect();
        if let Some(k) = self.own_key(l)
            && let Some(own) = self.borrows.keys[k].1
        {
            out.push((own, false));
        }
        out
    }

    /// After own loans through key `k` change by `delta` (`sign` -1 for
    /// an end): their cell, once made, takes the same change, and the
    /// local reads it back.
    fn own_mirror(&mut self, k: usize, own: u32, delta: u32, sign: i32) {
        let Some(c) = self.own_cell_of_key(k) else {
            return;
        };
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(c);
        self.f.get(c);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(delta);
        self.f.op(if sign < 0 { I::I32Sub } else { I::I32Add });
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
        self.f.get(c);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.set(own);
        self.f.op(I::End);
    }

    /// At the exit: the cells of own loans the body made.
    fn own_cells_release(&mut self) {
        let mut mine: Vec<u32> = (self.borrows.own_cells.values())
            .filter(|x| x.1)
            .map(|x| x.0)
            .collect();
        mine.sort_stable();
        for c in mine {
            self.f.get(c);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(c);
            self.because(
                hd_tir::own::Reason::Conservative("loan cell"),
                Self::release_cell,
            );
            self.f.zero(c);
            self.f.op(I::End);
        }
    }

    /// Whether a loan may hold an object of type `t`, and an exclusive one.
    fn lent_of(&self, t: Ty) -> Option<bool> {
        self.m.lent.get(&t).copied()
    }

    /// Whether a loan may hold the object at a trait value of trait
    /// `tr`'s data address, and an exclusive one: of any type the program
    /// converts to `tr` ([`Module::dyn_types`]).
    fn lent_dyn(&self, tr: hd_base::DefId) -> Option<bool> {
        (self.m.dyn_types.get(&tr).into_iter().flatten())
            .filter_map(|&t| self.lent_of(t))
            .reduce(|a, b| a || b)
    }

    /// The local holding the data address of the trait value reference at
    /// `at`, its first value.
    fn data_address(&mut self, at: &super::refs::At) -> StageResult<u32> {
        match at {
            super::refs::At::Vals(ls) if !ls.is_empty() => Ok(ls[0]),
            _ => self.at_i32(at),
        }
    }

    /// The objects this body's places may start at.
    fn roots<'s>(
        &'s self,
        shared: &'s hd_base::HashSet<usize>,
        pointers: &'s hd_base::HashSet<usize>,
    ) -> Roots<'s> {
        Roots {
            shared,
            pointers,
            objects: &self.m.objects,
            env: self.env(),
        }
    }

    /// Interns the pointers `Ref` `v`'s place reaches objects through.
    fn intern_ref_keys(&self, st: &mut Borrows, v: u32) {
        let Some(p) = self.b.decode_place(self.b.data[v as usize][0]) else {
            return;
        };
        if let Some((r, _)) = self.roots(&st.shared, &st.pointers).of(self.b, &p) {
            let key = (r, Vec::new(), st.frames[v as usize]);
            if !st.keys.iter().any(|(x, _)| *x == key) {
                st.keys.push((key, None));
            }
        }
        let p = match root_read(self.b, &p) {
            Some((_, q)) => q,
            None => p,
        };
        if !matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..)) {
            return;
        }
        let root = self.at_root_ty(p.root);
        let pointer_root = matches!(p.root, PlaceRoot::Local(l) if st.pointers.contains(&l.idx()));
        for (k, step) in p.proj.iter().enumerate() {
            let thin = *step == Proj::Deref
                && (k == 0 && pointer_root
                    || matches!(
                        self.walk_ty(root, &p.proj[..k]).map(|t| self.kind(t)),
                        Ok(Kind::Ref {
                            fat: false,
                            whole: true,
                            ..
                        })
                    ));
            let key = (p.root, p.proj[..k].to_vec(), ANY_FRAME);
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
        let holds = self.borrows.call_holds.contains(&i);
        let lends = self.borrows.refs.contains_key(&i) || call.is_some() || holds;
        // Each dereference of a `*T`, with the type of the object it reaches.
        let root = self.at_root_ty(p.root);
        let mut crossed = Vec::new();
        // The root object: a shared variable's cell, but not at the store
        // that makes it, as no reference reaches a cell before it exists;
        // or a top-level binding's slot.
        let shared = std::mem::take(&mut self.borrows.shared);
        let pointers = std::mem::take(&mut self.borrows.pointers);
        let root_obj = self
            .roots(&shared, &pointers)
            .of(self.b, &p)
            .filter(|(r, _)| match r {
                PlaceRoot::Local(l) => {
                    self.is_cell(l.idx()) && self.cells.get(&l.idx()) != Some(&i)
                }
                _ => true,
            });
        let ptr_root = {
            let sub = |t| self.sub(t);
            pointer_root(self.pool(), self.env(), (self.b, &sub), &pointers, &p)
        };
        self.borrows.shared = shared;
        self.borrows.pointers = pointers;
        if let Some((_, t)) = root_obj {
            crossed.push((ROOT, self.sub(t)));
        }
        // The dereferences of the read the place's root value is.
        let read = root_read(self.b, &p);
        if let Some((_, q)) = &read {
            let qt = self.at_root_ty(q.root);
            for (k, step) in q.proj.iter().enumerate() {
                if *step != Proj::Deref {
                    continue;
                }
                if let Kind::Ref {
                    target,
                    fat: false,
                    whole: true,
                } = self.kind(self.walk_ty(qt, &q.proj[..k])?)
                {
                    crossed.push((READ + k, target));
                }
            }
        }
        if let Some(t) = ptr_root {
            crossed.push((0, t));
        }
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
        let mut heads = Vec::new();
        // A top-level binding root's slot: `Roots::of` returns a global
        // root only for a `Module::objects` member.
        let slot = match root_obj {
            Some((PlaceRoot::Global(def, _), _)) => self.def_slot(def),
            _ => None,
        };
        for (n, (k, object)) in crossed.into_iter().enumerate() {
            let writes = write && n == last;
            let mask = match self.lent_of(object) {
                Some(_) if writes => Some(LOAN_STATE),
                Some(true) => Some(LOAN_EXCL),
                _ => None,
            };
            let keep =
                lends && (self.m.kept).contains(&(self.inst, i, u32::try_from(k).expect("steps")));
            let key = match (k, root_obj) {
                (ROOT, Some((r, _))) => {
                    let frame = self.borrows.frames[i as usize];
                    Some(self.borrow_key((r, Vec::new(), frame)))
                }
                (k, _) if k >= READ && k != ROOT => read.as_ref().map(|(_, q)| {
                    self.borrow_key((q.root, q.proj[..k - READ].to_vec(), ANY_FRAME))
                }),
                _ => matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                    .then(|| self.borrow_key((p.root, p.proj[..k].to_vec(), ANY_FRAME))),
            };
            if mask.is_none() && !keep && !holds {
                if lends {
                    taken.push((u32::MAX, key, false));
                }
                continue;
            }
            let o = match (k, root_obj, slot, &read) {
                (ROOT, Some((PlaceRoot::Local(l), _)), _, _) => self.cell_of(l.idx())?,
                (ROOT, _, Some(at), _) => {
                    let o = self.f.local(Val::I32);
                    self.f.address(at);
                    self.f.set(o);
                    o
                }
                (k, _, _, Some((r, q))) if k >= READ && k != ROOT => {
                    let (at, _) = self.walk_place(*r, q, k - READ, None)?;
                    self.at_i32(&at)?
                }
                _ => {
                    // ruled out: `ROOT` is pushed only for `Roots::of`'s root,
                    // a shared local's cell or a `Module::objects` binding,
                    // and lib.rs `address_globals` gives every object its
                    // `Module::addressed` slot; `READ + k` only for a
                    // dereference of `root_read`'s place, so the read is
                    // there. What remains is a step of the place itself.
                    debug_assert!(k < READ, "a root or read crossing without its object");
                    let (at, _) = self.walk_place(i, &p, k, None)?;
                    self.at_i32(&at)?
                }
            };
            let header = self.f.local(Val::I32);
            self.f.get(o);
            self.f.u32(crate::module::HEADER - 4);
            self.f.op(I::I32Sub);
            self.f.set(header);
            if let Some(mask) = mask {
                self.borrow_check(header, mask, key, object, writes);
            }
            taken.push((header, key, keep));
            heads.push((header, key, u32::try_from(k).expect("steps")));
        }
        if let Some(&l) = self.borrows.reborrows.get(&i) {
            self.borrow_reborrow(i, l);
        }
        // A `*T` whose callee's result holds it: its whole object.
        if holds
            && self.b.tags[i as usize] == Tag::Read
            && let Kind::Ref {
                target,
                fat: false,
                whole: true,
            } = self.kind(self.ty(i))
            && self.m.lent.contains_key(&target)
        {
            let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                .then(|| self.borrow_key((p.root, p.proj.clone(), ANY_FRAME)));
            let (at, _) = self.walk_place(i, &p, p.proj.len(), None)?;
            let o = self.at_i32(&at)?;
            let header = self.f.local(Val::I32);
            self.f.get(o);
            self.f.u32(crate::module::HEADER - 4);
            self.f.op(I::I32Sub);
            self.f.set(header);
            heads.push((header, key, u32::MAX));
        }
        if holds
            && (self.b.tags[i as usize] == Tag::Ref
                || matches!(
                    self.kind(self.ty(i)),
                    Kind::Ref {
                        whole: true,
                        fat: false,
                        ..
                    }
                ))
        {
            self.borrows.arg_heads.insert(i, heads);
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
            .map(|k| self.own_read(k));
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
        let what = format!(
            "a `{}`",
            hd_mono::printed_type_name(self.pool(), self.env(), object)
        );
        self.f.helper(Helper::Panic(format!(
            "borrow-conflict: {what} a live reference borrows was {access} through another `*T`"
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
        self.loan_state_add(header, delta, sign);
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
            self.own_mirror(k, own, delta, sign);
            self.f.op(I::End);
        }
    }

    /// Adds `delta` (or subtracts it, with `sign` -1) to the borrow state
    /// at the header address in local `header`. A shared loan goes
    /// through `rt:loan_share`, which spills past 31 (E.2).
    fn loan_state_add(&mut self, header: u32, delta: u32, sign: i32) {
        let op = if sign < 0 { I::I32Sub } else { I::I32Add };
        if delta == LOAN_SHARED {
            // Inline below a count of 31; the helper at 31.
            let full = LOAN_STATE - LOAN_EXCL;
            let st = self.f.local(Val::I32);
            self.f.get(header);
            self.f.op(I::I32Load(mem(HEAP, 0, 2)));
            self.f.set(st);
            self.f.get(st);
            self.f.u32(full);
            self.f.op(I::I32And);
            self.f.u32(full);
            self.f.op(I::I32Eq);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(header);
            self.f.helper(if sign < 0 {
                Helper::LoanUnshare
            } else {
                Helper::LoanShare
            });
            self.f.op(I::Else);
            self.f.get(header);
            self.f.get(st);
            self.f.u32(delta);
            self.f.op(op);
            self.f.op(I::I32Store(mem(HEAP, 0, 2)));
            self.f.op(I::End);
            return;
        }
        self.f.get(header);
        self.f.get(header);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(delta);
        self.f.op(op);
        self.f.op(I::I32Store(mem(HEAP, 0, 2)));
    }

    /// After `Store` `i` into a holder: the loans of the reference it
    /// stores, a copy's own loans on its source's objects, or a move's.
    #[inline(never)]
    pub(super) fn borrow_after_store(&mut self, i: u32) {
        // A match on a call's result ends the loans of the bindings its
        // arms made from the result.
        if self.b.tags[i as usize] == Tag::Match
            && let Some(bound) = self
                .borrows
                .via_call
                .get(&self.b.data[i as usize][0])
                .cloned()
        {
            for l in bound {
                self.borrow_release(l);
            }
        }
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
                    let source = self.borrows.reborrowed.remove(&v);
                    let Some(taken) = self.borrows.refs.get_mut(&v).map(std::mem::take) else {
                        continue;
                    };
                    // The objects the reborrowed holder's place crosses come
                    // first on the path, so after this `Ref`'s own.
                    if let Some(source) = source {
                        self.borrow_from_reborrow(held, &source, taken.len());
                    }
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
                        let delta = self.borrow_delta(held, s);
                        // An exclusive loan cannot be shared: a reborrow of
                        // one passes it over, as a move does.
                        if matches!(bind, Bind::Move(_)) || delta == LOAN_EXCL {
                            self.f.zero(fh);
                            continue;
                        }
                        self.f.get(h);
                        self.f.op(I::If(BlockType::Empty));
                        self.borrow_add(h, kl, delta, 1);
                        self.f.op(I::End);
                    }
                }
                Bind::Call(a) => self.borrow_result(i, held, a, false),
                Bind::CallParent(a) => self.borrow_result(i, held, a, true),
            }
        }
    }

    /// At `Ref` `i`, a reborrow through holder local `l`: keeps its loans
    /// for the holder the reference is stored in. An exclusive loan passes
    /// over, since it cannot be shared; a shared one is copied.
    fn borrow_reborrow(&mut self, i: u32, l: usize) {
        let mut held: Vec<Held> = (self.borrows.slots.keys())
            .filter(|h| h.local == l)
            .copied()
            .collect();
        held.sort_stable();
        let Some(&from) = held.first() else {
            return;
        };
        let slots = self.borrows.slots[&from].clone();
        let mut out = Vec::new();
        for (s, (h, kl)) in slots.into_iter().enumerate() {
            let delta = self.borrow_delta(from, s);
            let (ch, ck) = (self.f.local(Val::I32), self.f.local(Val::I32));
            self.f.get(h);
            self.f.set(ch);
            self.f.get(kl);
            self.f.set(ck);
            let moved = (delta == LOAN_EXCL).then_some(delta);
            if moved.is_some() {
                self.f.zero(h);
            }
            out.push((ch, ck, moved));
        }
        self.borrows.reborrowed.insert(i, out);
    }

    /// At the store of a reborrow into `held`: the loans [`Em::borrow_reborrow`]
    /// kept, slot by slot, as `held`'s kind gives them.
    fn borrow_from_reborrow(&mut self, held: Held, source: &[(u32, u32, Option<u32>)], at: usize) {
        for (s, &(ch, ck, moved)) in source.iter().enumerate() {
            let s = s + at;
            let (h, kl) = self.borrow_slot(held, s);
            let delta = self.borrow_delta(held, s);
            self.f.get(ch);
            self.f.set(h);
            self.f.get(ck);
            self.f.set(kl);
            self.f.get(h);
            self.f.op(I::If(BlockType::Empty));
            match moved {
                // An exclusive loan passed over keeps its state, in the
                // holder's kind.
                Some(old) if old != delta => {
                    self.borrow_add(h, kl, old, -1);
                    self.borrow_add(h, kl, delta, 1);
                }
                Some(_) => {}
                None => self.borrow_add(h, kl, delta, 1),
            }
            self.f.op(I::End);
        }
    }

    /// After `Store` `i` binds into `held` a call's result that holds
    /// argument `a`: the result's loans, until the holder's release. They
    /// are the argument's objects, as its evaluation found them, and the
    /// object the result points into, which the page table finds, where
    /// the callee's reference reaches one more object (`hd_tir::own::pointer`).
    /// The last of them takes the result's kind; the others are shared.
    fn borrow_result(&mut self, i: u32, held: Held, a: u32, parents: bool) {
        let Some(v) = direct_value(self.b, a) else {
            return;
        };
        let v = u32::try_from(v).expect("insts");
        // An argument whose objects no kept loan holds recorded none.
        let heads = self.borrows.arg_heads.get(&v).cloned().unwrap_or_default();
        let inner = !parents && self.m.result_inner.contains(&(self.inst, i));
        let mut s = if parents {
            // Slot zero is exclusive for an exclusive result. Parent
            // objects always stay shared, even if the inner loan elides.
            self.borrows
                .slots
                .get(&held)
                .map_or(0, Vec::len)
                .max(usize::from(self.borrows.excl[&held]))
        } else {
            usize::from(inner)
        };
        if inner
            && self.m.kept.contains(&(self.inst, i, u32::MAX - 1))
            && let Some(rt) = self.m.runtime
        {
            let Some(&r) = self
                .locals
                .get(&held.local)
                .and_then(|ls| ls.get(held.slot))
            else {
                return;
            };
            let (h, kl) = self.borrow_slot(held, 0);
            let delta = self.borrow_delta(held, 0);
            let mask = if delta == LOAN_EXCL {
                LOAN_STATE
            } else {
                LOAN_EXCL
            };
            self.f.get(r);
            self.f.call(crate::module::Sym::Inst(rt.block_of));
            self.f.op(I::LocalTee(h));
            self.f.op(I::If(BlockType::Empty));
            self.f.get(h);
            self.f.u32(4);
            self.f.op(I::I32Add);
            self.f.set(h);
            self.f.i32(0);
            self.f.set(kl);
            self.borrow_check_raw(h, mask);
            self.borrow_add(h, kl, delta, 1);
            self.f.op(I::End);
        }
        for (header, key, k) in heads.into_iter().rev() {
            if parents && k == u32::MAX {
                continue;
            }
            if !self.m.kept.contains(&(self.inst, i, k)) {
                s += 1;
                continue;
            }
            // A Read lends a whole counted object at MAX. All its
            // crossed place objects are parents, even when that whole
            // object's loan was elided and no header was recorded for it.
            if k != u32::MAX && self.b.tags[v as usize] == Tag::Read {
                s = s.max(usize::from(self.borrows.excl[&held]));
            }
            let (h, kl) = self.borrow_slot(held, s);
            let delta = self.borrow_delta(held, s);
            let mask = if delta == LOAN_EXCL {
                LOAN_STATE
            } else {
                LOAN_EXCL
            };
            self.f.get(header);
            self.f.set(h);
            match key {
                Some(k) => {
                    let own = self.own_local(k);
                    let _ = own;
                    self.f.u32(u32::try_from(k + 1).expect("keys"));
                }
                None => self.f.i32(0),
            }
            self.f.set(kl);
            match key {
                Some(k) => {
                    let own = self.own_read(k);
                    self.borrow_check_own(h, mask, own);
                }
                None => self.borrow_check_raw(h, mask),
            }
            self.borrow_add(h, kl, delta, 1);
            s += 1;
        }
    }

    /// Panics `borrow-conflict` when the state at header address `h` has a
    /// bit of `mask` set.
    fn borrow_check_raw(&mut self, h: u32, mask: u32) {
        self.f.get(h);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(mask);
        self.f.op(I::I32And);
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Panic(
            "borrow-conflict: an object a live reference borrows was lent by a call's result"
                .into(),
        ));
        self.f.op(I::End);
    }

    /// [`Em::borrow_check_raw`], against the own loans in local `own`.
    fn borrow_check_own(&mut self, h: u32, mask: u32, own: u32) {
        self.f.get(h);
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.u32(mask);
        self.f.op(I::I32And);
        self.f.get(own);
        if mask != LOAN_STATE {
            self.f.u32(mask);
            self.f.op(I::I32And);
        }
        self.f.op(I::I32Ne);
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Panic(
            "borrow-conflict: an object a live reference borrows was lent by a call's result"
                .into(),
        ));
        self.f.op(I::End);
    }

    /// Ends the loans local `l`'s holders hold, at its release or before
    /// a store replaces its value.
    pub(super) fn borrow_release(&mut self, l: usize) {
        if self.borrows.abandoned {
            return;
        }
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
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return;
        };
        match p.root {
            PlaceRoot::Local(l) if p.proj.is_empty() => self.borrow_release(l.idx()),
            PlaceRoot::Value(r) if p.proj.is_empty() => {
                let bound = r
                    .as_inst()
                    .and_then(|c| self.borrows.via_call.get(&c.0).cloned());
                for l in bound.unwrap_or_default() {
                    self.borrow_release(l);
                }
            }
            _ => {}
        }
    }

    /// Adds (or with `sign` -1 subtracts) call loan `loan`'s delta to its
    /// object's state and the function's own loans through its pointer.
    fn call_loan_add(&mut self, loan: CallLoan, sign: i32) {
        match loan.key {
            KeyOf::Local(kl) => self.borrow_add(loan.header, kl, loan.delta, sign),
            KeyOf::Known(key) => {
                let op = if sign < 0 { I::I32Sub } else { I::I32Add };
                self.loan_state_add(loan.header, loan.delta, sign);
                if let Some(k) = key.filter(|&k| k < self.borrows.loans) {
                    let own = self.own_local(k);
                    self.f.get(own);
                    self.f.u32(loan.delta);
                    self.f.op(op);
                    self.f.set(own);
                    self.own_mirror(k, own, loan.delta, sign);
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
        // A `*dyn` receiver lends the object at its data address, of one
        // of the types the program converts to its trait.
        let sub = |t| self.sub(t);
        let dyn_recv = dyn_receiver(self.pool(), self.env(), (self.b, &sub), i as usize);
        let (target, lent) = match (self.kind(self.ty(i)), dyn_recv) {
            (_, Some(tr)) => (
                strip(self.pool(), self.env(), self.ty(i)),
                self.lent_dyn(tr),
            ),
            (Kind::Ref { target, .. }, None) => (target, self.lent_of(target)),
            _ => return Ok(()),
        };
        if lent.is_none() {
            return Ok(());
        }
        let pointer = p.proj.len() - usize::from(dyn_recv.is_some());
        let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            .then(|| self.borrow_key((p.root, p.proj[..pointer].to_vec(), ANY_FRAME)));
        let (at, _) = self.walk_place(i, &p, pointer, None)?;
        let o = if dyn_recv.is_some() {
            self.data_address(&at)?
        } else {
            self.at_i32(&at)?
        };
        let header = self.f.local(Val::I32);
        if dyn_recv.is_some() {
            // No object behind a data address of 0: header 0, no loan.
            self.borrows.nullable.insert(header);
            self.f.get(o);
            self.f.u32(crate::module::HEADER - 4);
            self.f.op(I::I32Sub);
            self.f.i32(0);
            self.f.get(o);
            self.f.op(I::Select);
            self.f.set(header);
            self.f.get(header);
            self.f.op(I::If(BlockType::Empty));
        } else {
            self.f.get(o);
            self.f.u32(crate::module::HEADER - 4);
            self.f.op(I::I32Sub);
            self.f.set(header);
        }
        if self.borrows.call_holds.contains(&i) {
            self.borrows
                .arg_heads
                .insert(i, vec![(header, key, u32::MAX)]);
        }
        let end = |em: &mut Self| {
            if dyn_recv.is_some() {
                em.f.op(I::End);
            }
        };
        if !self.m.kept.contains(&(self.inst, i, u32::MAX)) {
            // No loan: only the access check of the lend, a write for a
            // `&mut`, whose two-phase window nothing can meet.
            if mutable || lent == Some(true) {
                let mask = if mutable { LOAN_STATE } else { LOAN_EXCL };
                self.borrow_check(header, mask, key, target, mutable);
            }
            end(self);
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
        end(self);
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
        self.borrow_carried(i);
        let Some(loans) = self.borrows.call_loans.get(&i).cloned() else {
            return;
        };
        let mut out = Vec::new();
        for mut loan in loans {
            if loan.upgrade {
                let KeyOf::Known(key) = loan.key else {
                    continue;
                };
                let nullable = self.borrows.nullable.contains(&loan.header);
                if nullable {
                    self.f.get(loan.header);
                    self.f.op(I::If(BlockType::Empty));
                }
                let own = key
                    .filter(|&k| k < self.borrows.loans)
                    .map(|k| self.own_read(k));
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
                if nullable {
                    self.f.op(I::End);
                }
            }
            out.push(loan);
        }
        self.borrows.call_loans.insert(i, out);
    }

    /// As call `i` begins: the loans of the `Ref`s its arguments carry
    /// through calls' results, on the objects each crossed, until it
    /// returns.
    fn borrow_carried(&mut self, i: u32) {
        let Some(refs) = self.borrows.carried.get(&i).cloned() else {
            return;
        };
        for (r, excl) in refs {
            let heads = self.borrows.arg_heads.get(&r).cloned().unwrap_or_default();
            let n = heads.len();
            for (j, (head, key, k)) in heads.into_iter().enumerate() {
                if !self.m.kept.contains(&(self.inst, r, k)) {
                    continue;
                }
                // A local of its own: another call's loan on the same
                // `Ref` may clear its header local when it ends.
                let header = self.f.local(Val::I32);
                self.f.get(head);
                self.f.set(header);
                let delta = if excl && j + 1 == n {
                    LOAN_EXCL
                } else {
                    LOAN_SHARED
                };
                let mask = if delta == LOAN_EXCL {
                    LOAN_STATE
                } else {
                    LOAN_EXCL
                };
                match key {
                    Some(k) => {
                        let own = self.own_read(k);
                        self.borrow_check_own(header, mask, own);
                    }
                    None => self.borrow_check_raw(header, mask),
                }
                let loan = CallLoan {
                    header,
                    key: KeyOf::Known(key),
                    delta,
                    upgrade: false,
                    exit: self.jumps_between(r, i),
                };
                self.call_loan_add(loan, 1);
                self.borrows.call_loans.entry(i).or_default().push(loan);
                if loan.exit {
                    self.borrows.all_calls.push(loan);
                }
            }
        }
    }

    /// After call `i` returns: the loans it held end.
    pub(super) fn borrow_call_end(&mut self, i: u32) {
        let Some(loans) = self.borrows.call_loans.remove(&i) else {
            return;
        };
        for loan in loans.into_iter().rev() {
            // A closure's lent holder may hold no loan, so its end tests.
            if loan.exit
                || matches!(loan.key, KeyOf::Local(_))
                || self.borrows.nullable.contains(&loan.header)
            {
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
        self.own_cells_release();
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

    /// In a frame's release glue (`.drop`), which frees only a frame that
    /// never started (#1430): releases the cells of the creator's own loans
    /// a suspending closure's frame holds, read from the frame's save
    /// (`at_of`). A cold frame holds no loan of its body's own, so
    /// `borrow_release` ends none in this glue (`abandoned`).
    pub(super) fn borrow_abandon(&mut self, at_of: &dyn Fn(u32) -> Option<(u32, Val)>) {
        self.borrows.abandoned = true;
        // The cells of own loans the body made, from the frame's save.
        let mut mine: Vec<u32> = (self.borrows.own_cells.values())
            .filter(|x| x.1)
            .map(|x| x.0)
            .collect();
        mine.sort_stable();
        for c in mine {
            let Some((at, v)) = at_of(c) else {
                continue;
            };
            let x = self.f.local(v);
            self.f.get(super::susp::FRAME);
            self.f.op(super::susp::load_op(v, at));
            self.f.set(x);
            self.f.get(x);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(x);
            self.release_cell();
            self.f.op(I::End);
        }
    }

    /// The locals whose holders hold pointer loans.
    /// Whether the body takes a pointer loan that its exit may have to
    /// end: a holder's, or a call's.
    pub(super) fn borrow_needs_exit(&self) -> bool {
        let b = &self.borrows;
        !b.binds.is_empty()
            || !b.call_refs.is_empty()
            || !b.call_ptrs.is_empty()
            || !b.call_closures.is_empty()
            || !b.carried.is_empty()
            || !b.own_shared.is_empty()
    }

    pub(super) fn borrow_locals(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.borrows.slots.keys().map(|h| h.local).collect();
        out.sort_stable();
        out.dedup();
        out
    }
}
