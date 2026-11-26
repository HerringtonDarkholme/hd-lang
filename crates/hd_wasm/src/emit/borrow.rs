//! Pointer loans and the access checks on an object's borrow state (model
//! A of `docs/compiler/ownership.md#pointer-loans-model-a`, #1336). Every counted object
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
//! apart from it (`docs/compiler/ownership.md#pointer-loans-model-a`). A local per pointer
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
//! task waits. A task that unwinds ends its loans at its exits; a frame
//! released before it starts holds none.

use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_layout::field_ty;
use hd_mono::{ProgramEnv, Target};
use hd_tir::ir::{Body, PlaceRoot, Proj, Tag, loan_word};
use hd_tir::own::pointer::{READ, ROOT, root_read, root_ty};
use hd_types::{InternPool, Ty, TyData};
use wasm_encoder::BlockType;

use super::Em;
use super::excl::{call_args, call_info};
use crate::layout::{Kind, coll_kind, kind, ref_mutable, strip_newtype, variants};
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

/// Only an eliminated closure's thin whole-pointer capture inherits the
/// creator's loan context. Ordinary value aliases and child-pointer paths
/// remain distinct. Metadata parents precede snapshots, so one ascending
/// pass closes every chain: O(locals + origin rows), no depth walk/retry.
#[derive(Default)]
pub struct CaptureOrigins {
    roots: Vec<Option<hd_base::LocalId>>,
    /// The lent parameters whose pointer is a part of a place, as
    /// `self.items` is: that place's key.
    parts: HashMap<usize, Lent>,
    /// The inlined parameters that take a lent argument's pointer.
    lent: Vec<bool>,
    #[cfg(feature = "testing")]
    pub work: usize,
}

/// A lent pointer's place: a root and the steps to the pointer value.
type Lent = (PlaceRoot, Vec<Proj>);

/// The inlined parameters that receive a lent `*T` argument (the `Read` a
/// `LoanBegin` names), each with the place it was read from, a local or a
/// part of one (`self.items`): they reach the object through that
/// pointer, so their accesses are the loan's own. So is one stored from a
/// whole read of such a parameter (named second), or its `&mut T` to `&T`
/// coercion: the `self` of a method inlined in an inlined method, through
/// its caller's. One whole store each. Cost: two passes over the body,
/// once per body ([`BorrowFacts`]).
fn lent_params(b: &Body) -> Vec<(u32, Option<u32>, Lent)> {
    let mut sites = vec![false; b.len()];
    let mut stores = vec![0u8; b.local_ty.len()];
    for i in 0..b.len() {
        match b.tags[i] {
            Tag::LoanBegin => sites[b.data[i][0] as usize] = true,
            Tag::Store => {
                if let Some((l, true)) = b.stored_local(i) {
                    stores[l.idx()] = stores[l.idx()].saturating_add(1);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    // Each lent parameter's pointer, by local.
    let mut pointer: Vec<Option<Lent>> = vec![None; b.local_ty.len()];
    for i in 0..b.len() {
        let Some((l, true)) = (b.tags[i] == Tag::Store)
            .then(|| b.stored_local(i))
            .flatten()
        else {
            continue;
        };
        let Some(mut v) = (b.direct_value(b.data[i][1])).filter(|_| stores[l.idx()] == 1) else {
            continue;
        };
        let direct = sites[v] && b.tags[v] == Tag::Read;
        while b.tags[v] == Tag::Coerce
            && let Some(u) = b.direct_value(b.data[v][0])
        {
            v = u;
        }
        let Some(place) = (b.tags[v] == Tag::Read)
            .then(|| b.decode_place(b.data[v][0]))
            .flatten()
        else {
            continue;
        };
        let (lent, from) = match place.root {
            PlaceRoot::Local(_) | PlaceRoot::Global(..) if direct => {
                (Some((place.root, place.proj)), None)
            }
            PlaceRoot::Local(f) if place.proj.is_empty() => {
                (pointer[f.idx()].clone(), Some(f.raw()))
            }
            _ => continue,
        };
        if let Some(lent) = lent {
            pointer[l.idx()] = Some(lent.clone());
            out.push((l.raw(), from, lent));
        }
    }
    out
}

impl CaptureOrigins {
    pub fn new(
        b: &Body,
        lent_params: &[(u32, Option<u32>, Lent)],
        eligible: impl Fn(hd_base::LocalId) -> bool,
    ) -> Self {
        let mut lent: Vec<Option<Lent>> = Vec::new();
        for (l, from, p) in lent_params {
            // One derived from a lent parameter reaches the object through
            // that parameter's pointer, when that one was taken.
            let take = match from {
                Some(f) => lent.get(*f as usize).is_some_and(Option::is_some),
                None => eligible(hd_base::LocalId::from_raw(*l)),
            };
            if take {
                lent.resize(b.local_ty.len(), None);
                lent[*l as usize] = Some(p.clone());
            }
        }
        if b.inlined_capture_origins.is_empty() && lent.is_empty() {
            return Self::default();
        }
        let lent_params = lent.iter().map(Option::is_some).collect();
        let mut roots = vec![None; b.local_ty.len()];
        #[cfg(feature = "testing")]
        let mut work = roots.len();
        for &(_, snapshot_id, creator_id) in &b.inlined_capture_origins {
            let (snapshot, creator) = (snapshot_id.idx(), creator_id.idx());
            // ruled out: the IR producer appends snapshots after creators;
            // the structural verifier checks bounds, order and uniqueness.
            assert!(creator < snapshot && snapshot < roots.len());
            if eligible(snapshot_id) {
                // ruled out: a snapshot has exactly one creation capture;
                // ordinary copying relocates that one metadata row.
                assert!(roots[snapshot].is_none(), "capture snapshot must be unique");
                roots[snapshot] = Some(creator_id);
            }
            #[cfg(feature = "testing")]
            {
                work += 1;
            }
        }
        let mut parts = HashMap::default();
        for (l, p) in lent.into_iter().enumerate() {
            match p {
                Some((PlaceRoot::Local(r), proj)) if proj.is_empty() => {
                    roots[l] = roots[l].or(Some(r));
                }
                Some(p) => {
                    parts.insert(l, p);
                }
                None => {}
            }
        }
        for l in 0..roots.len() {
            if let Some(parent) = roots[l] {
                roots[l] = roots[parent.idx()].or(Some(parent));
            }
            #[cfg(feature = "testing")]
            {
                work += 1;
            }
        }
        Self {
            roots,
            parts,
            lent: lent_params,
            #[cfg(feature = "testing")]
            work,
        }
    }

    /// Whether local `l` is an inlined parameter that takes a lent
    /// argument's pointer, whose object the argument's loan holds.
    fn is_lent(&self, l: usize) -> bool {
        self.lent.get(l).copied().unwrap_or(false)
    }

    #[cfg(feature = "testing")]
    pub fn local(&self, l: usize) -> usize {
        self.roots
            .get(l)
            .copied()
            .flatten()
            .map_or(l, hd_base::LocalId::idx)
    }

    #[must_use]
    pub fn key(&self, mut key: Key) -> Key {
        if key.1.is_empty()
            && key.2 == ANY_FRAME
            && let PlaceRoot::Local(l) = key.0
        {
            if let Some(root) = self.roots.get(l.idx()).copied().flatten() {
                key.0 = PlaceRoot::Local(root);
            } else if let Some((root, proj)) = self.parts.get(&l.idx()) {
                return (*root, proj.clone(), ANY_FRAME);
            }
        }
        key
    }
}

/// The frame of a pointer's key ([`Key`]).
pub const ANY_FRAME: u32 = u32::MAX;

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

/// Immutable frame and lifetime facts shared by sibling emitters.
pub(super) struct BorrowFacts {
    frames: std::sync::Arc<[u32]>,
    lent_params: Vec<(u32, Option<u32>, Lent)>,
}

impl BorrowFacts {
    fn new(b: &Body) -> Self {
        crate::work::use_facts(b.len());
        Self {
            frames: frames_of(b).into(),
            lent_params: lent_params(b),
        }
    }
}

/// The per-body state of its loans and own-loan checks.
#[derive(Default)]
pub struct Borrows {
    pub capture_origins: CaptureOrigins,
    /// The loan sites (`Ref`s, and `*T` reads a call lends), and the
    /// objects each one crossed, once emitted: the header address, the
    /// pointer's key, and whether elision keeps that loan
    /// (`hd_tir::own::pointer`).
    refs: HashMap<u32, Vec<(u32, Option<usize>, bool)>>,
    /// The `*T` reads a call lends: whether as `&mut`, and whether as a
    /// two-phase receiver, shared until its call.
    lent: HashMap<u32, (bool, bool)>,
    /// Each kept `LoanBegin`: a slot per object its site crosses, in
    /// `LoanBegin` order (the last crossed first): the local of the
    /// header address, 0 while no loan is held, made once the function
    /// has locals, and the pointer key its loan counts as the
    /// function's own through, fixed before emission (a close can be
    /// emitted before its begin, as a loop's close where its site runs
    /// again).
    begins: HashMap<u32, Vec<(u32, Option<usize>)>>,
    /// Each site's `LoanBegin`, and per two-phase `LoanBegin` the local
    /// set once its call activates it, so its last object's loan is
    /// exclusive.
    site_begin: HashMap<u32, u32>,
    active: HashMap<u32, u32>,
    /// The sites that are `*T` values rather than places.
    valued: hd_base::HashSet<u32>,
    /// The pointers places reach objects through, and the local holding
    /// the state of the function's own loans through each, once used.
    pub keys: Vec<(Key, Option<u32>)>,
    /// Stable vector IDs, indexed without scanning other pointers.
    pub key_index: HashMap<Key, usize>,
    #[cfg(feature = "testing")]
    pub key_work: usize,
    /// The first `loans` keys are those a loan is taken through, interned
    /// before emission.
    loans: usize,
    /// The inlined copy each instruction came from ([`frames_of`]).
    frames: std::sync::Arc<[u32]>,
    /// The counter local of the own loans through each pointer local a
    /// closure shares (`hd_tir::own::loans::share_counters`), and where
    /// each counter is: a local, or a cell's address and the word's
    /// offset in it.
    counters: HashMap<usize, usize>,
    counter_at: HashMap<usize, Option<u32>>,
    /// The header locals of loans on a `*dyn` receiver's object, which
    /// are 0 where its data address is (a trait value of no Wasm values):
    /// no loan is taken then.
    nullable: hd_base::HashSet<u32>,
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
                let TyData::Adt(_, args) = pool.get(strip_newtype(pool, env, ty)) else {
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
                Kind::Value => None,
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
    b: &Body,
    v: usize,
) -> Option<hd_base::DefId> {
    if b.tags[v] != Tag::Read {
        return None;
    }
    let p = b.decode_place(b.data[v][0])?;
    let (Some(Proj::Deref), Some(root)) = (p.proj.last(), root_ty(b, p.root)) else {
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
    match pool.get(strip_newtype(pool, env, target)) {
        TyData::TraitValue { def, .. } => Some(def),
        _ => None,
    }
}

/// The receiver reads among body `id`'s calls that lend a whole pointer
/// as `&self` or `&mut self` (`ref.excl.alias.call-loan`), in one pass
/// over the body's resolved calls. A top-level binding such a read starts at is reached by
/// a reference, as by a `Ref` (`ref.excl.alias.global`).
pub(crate) fn lent_receiver_reads(
    input: &crate::Input<'_>,
    id: hd_base::InstId,
    b: &Body,
) -> hd_base::HashSet<usize> {
    let mut out = hd_base::HashSet::default();
    // Only a `Call` with a resolved target has an auto-ref receiver, and
    // each one has an entry here, so the walk visits the calls alone.
    for (&i, target) in &input.collected.calls[id.idx()] {
        let i = i as usize;
        let Some(Some(mutable)) = call_info(input, b, i, Some(target)) else {
            continue;
        };
        let Some(&a) = call_args(b, i).first() else {
            continue;
        };
        let Some(v) = b.direct_value(a).filter(|&v| b.tags[v] == Tag::Read) else {
            continue;
        };
        let lent = pointer_lent(input.pool, input.env, None, b.ty[v], 0, Some(mutable)).is_some()
            || dyn_receiver(input.pool, input.env, b, v).is_some();
        if lent {
            out.insert(v);
        }
    }
    out
}

/// The parameter types of a call to `target`, as the callee instance's
/// own body has them ([`crate::emit::callee_params`]): a direct call's.
pub(super) fn call_params(
    input: &crate::Input<'_>,
    m: &Module,
    target: Option<&Target>,
) -> Vec<Ty> {
    match target {
        Some(Target::Call(c) | Target::CrossingNumber { call: c, .. }) => {
            super::callee_params(input, m, c.key, c.item, c.args)
        }
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

impl Borrows {
    /// Expected O(key projection words); IDs retain first-insertion order.
    pub fn intern_key(&mut self, key: Key) -> usize {
        let key = self.capture_origins.key(key);
        #[cfg(feature = "testing")]
        {
            self.key_work += 1 + key.1.len();
        }
        match self.key_index.entry(key) {
            hd_base::hash::hash_map::Entry::Occupied(entry) => *entry.get(),
            hd_base::hash::hash_map::Entry::Vacant(entry) => {
                let index = self.keys.len();
                #[cfg(feature = "testing")]
                {
                    self.key_work += 1 + entry.key().1.len();
                }
                self.keys.push((entry.key().clone(), None));
                entry.insert(index);
                index
            }
        }
    }
}

impl Em<'_, '_, '_> {
    /// Before emission: the sub-body's loan sites, from its `LoanBegin`s,
    /// which the checker placed (`hd_tir::own::loans`), and the pointers
    /// their loans go through, whose own-loan counts the access checks
    /// compare with. Cost: one pass over the body's instructions.
    pub(super) fn find_borrows(&mut self) {
        if self.m.lent.is_empty() {
            return;
        }
        let facts = std::sync::Arc::clone(&self.borrow_facts);
        let immutable = facts.get_or_init(|| BorrowFacts::new(self.b));
        let mut st = Borrows {
            capture_origins: CaptureOrigins::new(self.b, &immutable.lent_params, |l| {
                self.own_pointer(l)
            }),
            frames: std::sync::Arc::clone(&immutable.frames),
            ..Borrows::default()
        };
        // The counters of the own loans through the pointers a closure
        // shares, which the `LoanBegin`s through them name in any
        // sub-body: a closure's check through one compares with it even
        // where the closure takes no loan itself.
        for i in 0..self.b.tags.len() {
            let [site, word] = self.b.data[i];
            if self.b.tags[i] == Tag::LoanBegin
                && let Some(c) = loan_word::counter(word)
                && let Some(p) = hd_tir::own::loans::site_pointer(self.b, site, true)
            {
                let local = PlaceRoot::Local(hd_base::LocalId::from_raw(p));
                let key = st.capture_origins.key((local, Vec::new(), ANY_FRAME));
                if let PlaceRoot::Local(l) = key.0 {
                    st.counters.insert(l.idx(), c as usize);
                }
                st.counters.insert(p as usize, c as usize);
                st.intern_key(key);
            }
        }
        let sub = u32::from(self.sub_id);
        // The place sites, for their pointer keys below, and the
        // `LoanBegin` of each whose loan elision keeps.
        let (mut kept_begin, mut sites) = (HashMap::default(), Vec::new());
        for i in 0..self.b.tags.len() {
            if self.b.tags[i] != Tag::LoanBegin || self.topo.sub[i] != sub {
                continue;
            }
            let [site, word] = self.b.data[i];
            if word & loan_word::ACTIVATE != 0 {
                st.active.insert(site, 0);
                continue;
            }
            st.site_begin.insert(site, u32::try_from(i).expect("insts"));
            // A site that keeps no loan takes its access check only: its
            // `LoanBegin` and closes lower to nothing.
            let kept = self.m.kept_sites.contains(&(self.inst, site));
            // A site an ownership pass forwarded to a `*T` value: its
            // whole object, at the value's address.
            if !matches!(self.b.tags[site as usize], Tag::Ref | Tag::Read) {
                st.valued.insert(site);
                if kept {
                    st.begins
                        .insert(u32::try_from(i).expect("insts"), vec![(0, None)]);
                }
                continue;
            }
            if self.b.decode_place(self.b.data[site as usize][0]).is_none() {
                continue;
            }
            match self.b.tags[site as usize] {
                Tag::Ref => {}
                // A `*T` a call lends: its whole object, through the
                // pointer the read's place names (a `*dyn` receiver's
                // pointer is the place before its dereference).
                Tag::Read => {
                    st.lent.insert(site, (word & loan_word::EXCL != 0, false));
                }
                _ => continue,
            }
            st.refs.insert(site, Vec::new());
            if kept {
                kept_begin.insert(site, u32::try_from(i).expect("insts"));
            }
            sites.push(site);
        }
        // A two-phase receiver lends `&mut`, shared until its call.
        for site in st.active.keys() {
            if let Some(x) = st.lent.get_mut(site) {
                *x = (true, true);
            }
        }
        self.borrows = st;
        // Each site's crossings and their pointer keys, as its access
        // check takes them (`Em::crossings_of`): the keys are the
        // function's own-loan counters, and each kept begin's slots,
        // last crossed first, with a lent pointer's whole object last
        // crossed of all.
        for site in sites {
            // The link table lists only places from a local, a value or
            // a top-level binding, as the access check takes them.
            let Some(p) = self.b.decode_place(self.b.data[site as usize][0]) else {
                continue;
            };
            let mut keys: Vec<Option<usize>> = self
                .crossings_of(site, &p)
                .0
                .into_iter()
                .map(|c| c.2)
                .collect();
            if self.borrows.lent.contains_key(&site)
                && let Some((.., key)) = self.lend_of(site)
            {
                keys.push(key);
            }
            if let Some(&begin) = kept_begin.get(&site) {
                let slots = keys.into_iter().rev().map(|k| (0, k)).collect();
                self.borrows.begins.insert(begin, slots);
            }
        }
        let mut st = std::mem::take(&mut self.borrows);
        st.loans = st.keys.len();
        self.borrows = st;
    }

    /// Once the function has its locals: each loan's slots, a header
    /// address and a pointer key per object its site may cross, zero
    /// while it holds no loan.
    pub(super) fn borrow_init_flags(&mut self) -> StageResult<()> {
        let mut counters: Vec<usize> = self.borrows.counters.values().copied().collect();
        counters.sort_stable();
        counters.dedup();
        for c in counters {
            // The counter's own local is looked up at its first use,
            // after its store at the root or its closure's capture load
            // made it: only the word's offset in a cell is known here.
            let at = if self.is_cell(c) {
                Some(self.cell_mem(c)?.leaves[0].offset)
            } else {
                None
            };
            self.borrows.counter_at.insert(c, at);
        }
        let mut begins: Vec<u32> = self.borrows.begins.keys().copied().collect();
        begins.sort_stable();
        for i in begins {
            let keys: Vec<Option<usize>> = self.borrows.begins[&i].iter().map(|x| x.1).collect();
            let slots = keys
                .into_iter()
                .map(|k| (self.f.local(Val::I32), k))
                .collect();
            self.borrows.begins.insert(i, slots);
        }
        // A two-phase site's flag, keyed by its `LoanBegin`.
        let mut sites: Vec<u32> = self.borrows.active.keys().copied().collect();
        sites.sort_stable();
        self.borrows.active.clear();
        for site in sites {
            if let Some(&begin) = self.borrows.site_begin.get(&site) {
                let flag = self.f.local(Val::I32);
                self.borrows.active.insert(begin, flag);
            }
        }
        Ok(())
    }

    /// `LoanBegin` `i`: the loans of its site on each object the site
    /// crossed and elision keeps, shared but on the last object for a
    /// `&mut`. A site run again (a loop's next round) first ends the loans
    /// its last run took, which no live value holds any more.
    pub(super) fn loan_begin(&mut self, i: u32) {
        let [site, word] = self.b.data[i as usize];
        if word & loan_word::ACTIVATE != 0 {
            return self.loan_activate(site);
        }
        if self.borrows.valued.contains(&site) {
            self.loan_value(site);
        }
        let Some(taken) = self.borrows.refs.get(&site).cloned() else {
            return;
        };
        let Some(slots) = self.borrows.begins.get(&i).cloned() else {
            return;
        };
        debug_assert_eq!(taken.len(), slots.len(), "a slot per crossing");
        let excl = word & loan_word::EXCL != 0;
        for (s, (header, key, keep)) in taken.into_iter().rev().enumerate() {
            if !keep {
                continue;
            }
            let (h, k) = slots[s];
            debug_assert_eq!(key, k, "a slot's key is its crossing's");
            let delta = if s == 0 && excl {
                LOAN_EXCL
            } else {
                LOAN_SHARED
            };
            self.f.get(h);
            self.f.op(I::If(BlockType::Empty));
            self.slot_end(i, s, h, k, delta);
            self.f.op(I::End);
            self.f.get(header);
            self.f.set(h);
            // A `*dyn` receiver's object may be absent: header 0.
            let nullable = self.borrows.nullable.contains(&header);
            if nullable {
                self.f.get(h);
                self.f.op(I::If(BlockType::Empty));
            }
            self.borrow_add(h, k, delta, 1);
            if nullable {
                self.f.op(I::End);
            }
        }
    }

    /// A value site's whole-object crossing, as a lent `*T` read's: the
    /// lend's check, and the header for its `LoanBegin`, which
    /// [`Em::loan_begin`] then takes as any site's.
    fn loan_value(&mut self, site: u32) {
        let Some(&o) = self.vals.get(&site).and_then(|v| v.first()) else {
            return;
        };
        let Kind::Ref {
            target,
            fat: false,
            whole: true,
        } = self.kind(self.ty(site))
        else {
            return;
        };
        let Some(lent) = self.lent_of(target) else {
            return;
        };
        let begin = self.borrows.site_begin.get(&site).copied();
        let word = begin.map_or(0, |b| self.b.data[b as usize][1]);
        let two_phase = begin.is_some_and(|b| self.borrows.active.contains_key(&b));
        let mutable = word & loan_word::EXCL != 0 || two_phase;
        let header = self.f.local(Val::I32);
        self.f.get(o);
        self.f.u32(crate::module::HEADER - 4);
        self.f.op(I::I32Sub);
        self.f.set(header);
        let keep = self.m.kept.contains(&(self.inst, site, u32::MAX));
        let writes = mutable && !(two_phase && keep);
        if keep || mutable || lent {
            let mask = if writes { LOAN_STATE } else { LOAN_EXCL };
            self.borrow_check(header, mask, None, target, writes);
        }
        self.borrows.refs.insert(site, vec![(header, None, keep)]);
    }

    /// A cancelled body's way out: every loan it still holds ends, as
    /// its drops run (`docs/compiler/ownership.md`, "Suspension").
    pub(super) fn loan_close_all(&mut self) {
        let mut begins: Vec<u32> = self.borrows.begins.keys().copied().collect();
        begins.sort_stable();
        for begin in begins {
            self.loan_end(begin);
        }
    }

    /// `LoanClose` `i`: the loans its `LoanBegin` holds end, if they are
    /// held on this path.
    pub(super) fn loan_close(&mut self, i: u32) {
        self.loan_end(self.b.data[i as usize][0]);
    }

    /// Ends the loans `LoanBegin` `begin` holds on this path.
    fn loan_end(&mut self, begin: u32) {
        let Some(slots) = self.borrows.begins.get(&begin).cloned() else {
            return;
        };
        let excl = self.b.data[begin as usize][1] & loan_word::EXCL != 0;
        for (s, (h, kl)) in slots.into_iter().enumerate() {
            let delta = if s == 0 && excl {
                LOAN_EXCL
            } else {
                LOAN_SHARED
            };
            self.f.get(h);
            self.f.op(I::If(BlockType::Empty));
            self.slot_end(begin, s, h, kl, delta);
            self.f.zero(h);
            self.f.op(I::End);
        }
    }

    /// Ends slot `s` of `LoanBegin` `begin` (header local `h`, pointer
    /// key `kl`), whose loan is `delta`, or exclusive once its two-phase
    /// call activated it.
    fn slot_end(&mut self, begin: u32, s: usize, h: u32, kl: Option<usize>, delta: u32) {
        match self.borrows.active.get(&begin).copied().filter(|_| s == 0) {
            Some(flag) => {
                self.f.get(flag);
                self.f.op(I::If(BlockType::Empty));
                self.borrow_add(h, kl, LOAN_EXCL, -1);
                self.f.op(I::Else);
                self.borrow_add(h, kl, delta, -1);
                self.f.op(I::End);
                self.f.zero(flag);
            }
            None => self.borrow_add(h, kl, delta, -1),
        }
    }

    /// The two-phase activation of `site`'s loan, right before its call:
    /// its last object's shared loan becomes exclusive, which the state
    /// allows only when the function's own loans through its pointer are
    /// all the state holds.
    fn loan_activate(&mut self, site: u32) {
        let Some(&begin) = self.borrows.site_begin.get(&site) else {
            return;
        };
        let (Some(&flag), Some(slots)) = (
            self.borrows.active.get(&begin),
            self.borrows.begins.get(&begin).cloned(),
        ) else {
            return;
        };
        let key = (self.borrows.refs.get(&site))
            .and_then(|t| t.last().copied())
            .and_then(|(_, key, _)| key);
        let Some(&(h, kl)) = slots.first() else {
            return;
        };
        self.f.get(h);
        self.f.op(I::If(BlockType::Empty));
        let own = key
            .filter(|&k| k < self.borrows.loans)
            .map(|k| self.own_read(k));
        self.f.get(h);
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
            "borrow-conflict: a receiver a live reference borrows was lent as `&mut self`".into(),
        ));
        self.f.op(I::End);
        self.borrow_add(h, kl, LOAN_SHARED, -1);
        self.borrow_add(h, kl, LOAN_EXCL, 1);
        self.f.i32(1);
        self.f.set(flag);
        self.f.op(I::End);
    }

    /// Whether the body takes a pointer loan.
    pub(super) fn borrow_needs_exit(&self) -> bool {
        !self.borrows.begins.is_empty()
    }

    fn own_pointer(&self, l: hd_base::LocalId) -> bool {
        matches!(self.kind(self.b.local_ty[l.idx()]), Kind::Ref {
                target,
                fat: false,
                whole: true,
            } if self.lent_of(target).is_some())
    }

    /// The local of the own loans through key `k`, read again from their
    /// cell where one is made: the creator and its closures, suspending
    /// ones too, change one count
    /// Where the function's own loans through key `k` are counted: in the
    /// counter a closure shares, a cell's word (its address local and the
    /// word's offset) or a local, or else in a local of its own.
    fn own_at(&mut self, k: usize) -> (u32, Option<u32>) {
        if let (PlaceRoot::Local(l), proj, frame) = &self.borrows.keys[k].0
            && proj.is_empty()
            && *frame == ANY_FRAME
            && let Some(&c) = self.borrows.counters.get(&l.idx())
            && let Some(&off) = self.borrows.counter_at.get(&c)
            && let Ok(ls) = self.local(hd_tir::ir::u32_of(c))
        {
            return (ls[0], off);
        }
        (self.own_local(k), None)
    }

    /// A local holding the function's own loans through key `k` as they
    /// stand.
    fn own_read(&mut self, k: usize) -> u32 {
        match self.own_at(k) {
            (own, None) => own,
            (cell, Some(off)) => {
                let own = self.f.local(Val::I32);
                self.f.get(cell);
                self.f.op(I::I32Load(mem(HEAP, u64::from(off), 2)));
                self.f.set(own);
                own
            }
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
    fn data_address(&mut self, at: &super::refs::At) -> u32 {
        match at {
            super::refs::At::Vals(ls) if !ls.is_empty() => ls[0],
            _ => self.at_i32(at),
        }
    }

    /// The index of pointer `key`, interned.
    fn borrow_key(&mut self, key: Key) -> usize {
        self.borrows.intern_key(key)
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

    /// The objects access `i`'s place `p` crosses, in order, each with
    /// the pointer key its check compares the function's own loans
    /// through: from the link pass's table (`Module::crossed`), less a
    /// shared variable's cell where it is no cell yet; with the root
    /// object it starts at and the read its root value is.
    #[allow(clippy::type_complexity)]
    fn crossings_of(
        &mut self,
        i: u32,
        p: &hd_tir::ir::Place,
    ) -> (
        Vec<(usize, Ty, Option<usize>)>,
        Option<(PlaceRoot, Ty)>,
        Option<(u32, hd_tir::ir::Place)>,
    ) {
        let listed = self
            .m
            .crossed
            .get(&(self.inst, i))
            .cloned()
            .unwrap_or_default();
        // The root object: a shared variable's cell, but not at the store
        // that makes it, as no reference reaches a cell before it exists;
        // or a top-level binding's slot.
        let root_obj = listed
            .root
            .zip(listed.steps.first().filter(|s| s.0 == ROOT).map(|s| s.1))
            .filter(|(r, _)| match r {
                PlaceRoot::Local(l) => {
                    self.is_cell(l.idx()) && self.cells.get(&l.idx()) != Some(&i)
                }
                _ => true,
            });
        // The dereferences of the read the place's root value is.
        let read = root_read(self.b, p);
        let mut out = Vec::with_capacity(listed.steps.len());
        for (k, object) in listed.steps {
            let key = match (k, root_obj) {
                (ROOT, None) => continue,
                (ROOT, Some((r, _))) => {
                    let frame = self.borrows.frames[i as usize];
                    Some(self.borrow_key((r, Vec::new(), frame)))
                }
                (k, _) if k >= READ => read.as_ref().map(|(_, q)| {
                    self.borrow_key((q.root, q.proj[..k - READ].to_vec(), ANY_FRAME))
                }),
                _ => matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
                    .then(|| self.borrow_key((p.root, p.proj[..k].to_vec(), ANY_FRAME))),
            };
            out.push((k, object, key));
        }
        (out, root_obj, read)
    }

    /// What a `*T` read `i` lends to a call, when the lend takes a loan
    /// slot ([`Em::borrow_lend_pointer`]): its place, the trait of a
    /// `*dyn` receiver, the lent object's type, whether a loan of it may
    /// be exclusive, and the pointer key of its whole object.
    #[allow(clippy::type_complexity)]
    fn lend_of(
        &mut self,
        i: u32,
    ) -> Option<(
        hd_tir::ir::Place,
        Option<hd_base::DefId>,
        Ty,
        bool,
        Option<usize>,
    )> {
        let p = self.b.decode_place(self.b.data[i as usize][0])?;
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return None;
        }
        // A `*dyn` receiver lends the object at its data address, of one
        // of the types the program converts to its trait.
        let dyn_recv = dyn_receiver(self.pool(), self.env(), self.b, i as usize);
        let (target, lent) = match (self.kind(self.ty(i)), dyn_recv) {
            (_, Some(tr)) => (
                strip_newtype(self.pool(), self.env(), self.ty(i)),
                self.lent_dyn(tr),
            ),
            (Kind::Ref { target, .. }, None) => (target, self.lent_of(target)),
            _ => return None,
        };
        let pointer = p.proj.len() - usize::from(dyn_recv.is_some());
        let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            .then(|| self.borrow_key((p.root, p.proj[..pointer].to_vec(), ANY_FRAME)));
        Some((p, dyn_recv, target, lent?, key))
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
        // A `*T` a call lends: its whole object, the last it crosses.
        let whole = match self.borrows.lent.get(&i) {
            Some(&lend) => self.borrow_lend_pointer(i, lend)?,
            None => None,
        };
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
        let (crossed, root_obj, read) = self.crossings_of(i, &p);
        let last = crossed.len().wrapping_sub(1);
        let mut taken = Vec::new();
        // A top-level binding root's slot: `Roots::of` returns a global
        // root only for a `Module::objects` member.
        let slot = match root_obj {
            Some((PlaceRoot::Global(def, _), _)) => self.def_slot(def),
            _ => None,
        };
        for (n, (k, object, key)) in crossed.into_iter().enumerate() {
            let writes = write && n == last;
            let mask = match self.lent_of(object) {
                Some(_) if writes => Some(LOAN_STATE),
                Some(true) => Some(LOAN_EXCL),
                _ => None,
            };
            // The object an inlined parameter's lent pointer reaches is
            // the argument's loan's: no second loan of its own on it.
            let lent_root = k == 0
                && matches!(p.root, PlaceRoot::Local(l) if self.borrows.capture_origins.is_lent(l.idx()));
            let keep = lends
                && !lent_root
                && (self.m.kept).contains(&(self.inst, i, u32::try_from(k).expect("steps")));
            if mask.is_none() && !keep {
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
                    self.at_i32(&at)
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
                    self.at_i32(&at)
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
        }
        taken.extend(whole);
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

    /// Adds `delta` (or subtracts it, with `sign` -1) to the state at the
    /// header address in local `header`, and to the function's own loans
    /// through pointer `key`.
    fn borrow_add(&mut self, header: u32, key: Option<usize>, delta: u32, sign: i32) {
        let op = if sign < 0 { I::I32Sub } else { I::I32Add };
        self.loan_state_add(header, delta, sign);
        if let Some(k) = key.filter(|&k| k < self.borrows.loans) {
            match self.own_at(k) {
                (own, None) => {
                    self.f.get(own);
                    self.f.u32(delta);
                    self.f.op(op);
                    self.f.set(own);
                }
                (cell, Some(off)) => {
                    self.f.get(cell);
                    self.f.get(cell);
                    self.f.op(I::I32Load(mem(HEAP, u64::from(off), 2)));
                    self.f.u32(delta);
                    self.f.op(op);
                    self.f.op(I::I32Store(mem(HEAP, u64::from(off), 2)));
                }
            }
        }
    }

    /// Adds `delta` (or subtracts it, with `sign` -1) to the borrow state
    /// at the header address in local `header`. A shared loan goes
    /// through `rt:loan_share`, which spills past 31
    /// (`docs/compiler/ownership.md#the-mechanism-in-detail-formerly-e2`).
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

    /// At `Read` `i` of a `*T` a call lends as a local reference: the
    /// check of the lend on its whole object, and the object's header for
    /// its `LoanBegin`, exclusive for a `&mut`, with whether elision keeps
    /// the loan. `None` for a type no loan holds.
    fn borrow_lend_pointer(
        &mut self,
        i: u32,
        (mutable, two_phase): (bool, bool),
    ) -> StageResult<Option<(u32, Option<usize>, bool)>> {
        let Some((p, dyn_recv, target, lent, key)) = self.lend_of(i) else {
            return Ok(None);
        };
        let pointer = p.proj.len() - usize::from(dyn_recv.is_some());
        let (at, _) = self.walk_place(i, &p, pointer, None)?;
        let o = if dyn_recv.is_some() {
            self.data_address(&at)
        } else {
            self.at_i32(&at)
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
        let keep = self.m.kept.contains(&(self.inst, i, u32::MAX));
        // Without a kept loan only the lend's check, a write for a `&mut`.
        // A kept two-phase loan is shared until its call, whose
        // activation checks the whole state; without a loan the lend's
        // check is a write's at once.
        let writes = mutable && !(two_phase && keep);
        if keep || mutable || lent {
            let mask = if writes { LOAN_STATE } else { LOAN_EXCL };
            self.borrow_check(header, mask, key, target, writes);
        }
        if dyn_recv.is_some() {
            self.f.op(I::End);
        }
        Ok(Some((header, key, keep)))
    }
}
