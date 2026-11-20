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
//! task waits. A task that unwinds ends its loans at its exits; one whose
//! frame is released while it waits ends them in its release glue
//! ([`Em::borrow_abandon`]), so no object stays borrowed after it.

use hd_base::{HashMap, SliceSort as _, StageResult};
use hd_layout::field_ty;
use hd_mono::{ProgramEnv, Target};
use hd_tir::ir::{Body, PlaceRoot, Proj, Tag, loan_word};
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
struct CaptureOrigins {
    roots: Vec<Option<hd_base::LocalId>>,
    /// The inlined parameters that take a lent argument's pointer.
    lent: Vec<bool>,
    #[cfg(test)]
    work: usize,
}

#[cfg(test)]
mod capture_origin_tests {
    use super::*;
    use hd_base::{DefId, LocalId, SubId};
    use hd_tir::ir::{TirBuilder, TirSink as _};

    #[test]
    fn pointer_key_interning_is_independent_of_other_keys() {
        for (width, depth) in [(100_u32, 20), (1000, 20), (20, 1000)] {
            let mut borrows = Borrows::default();
            let keys: Vec<_> = (0..width)
                .map(|local| {
                    (
                        PlaceRoot::Local(LocalId::from_raw(local)),
                        vec![Proj::Field(0); depth],
                        ANY_FRAME,
                    )
                })
                .collect();
            for (expected, key) in keys.iter().enumerate() {
                assert_eq!(borrows.intern_key(key.clone()), expected);
            }
            for _ in 0..10 {
                for (expected, key) in keys.iter().enumerate() {
                    let before = borrows.key_work;
                    assert_eq!(borrows.intern_key(key.clone()), expected);
                    assert_eq!(borrows.key_work - before, depth + 1);
                }
            }
            assert_eq!(u32::try_from(borrows.keys.len()), Ok(width));
            assert_eq!(borrows.key_work, 12 * keys.len() * (depth + 1));
        }
    }

    #[test]
    fn pointer_keys_normalize_capture_origins_before_interning() {
        let body = body(3, &[(1, 0), (2, 1)]);
        let mut borrows = Borrows {
            capture_origins: CaptureOrigins::new(&body, &[], |_| true),
            ..Borrows::default()
        };
        for local in [2, 0, 1] {
            assert_eq!(
                borrows.intern_key((
                    PlaceRoot::Local(LocalId::from_raw(local)),
                    vec![],
                    ANY_FRAME
                )),
                0
            );
        }
        assert_eq!(borrows.keys.len(), 1);
        assert_eq!(borrows.key_index.len(), 1);
        assert_eq!(borrows.keys[0].0.0, PlaceRoot::Local(LocalId::from_raw(0)));
    }

    fn storage_body(locals: usize) -> TirBuilder {
        let mut builder = TirBuilder::new(DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        builder.body_mut().local_ty = vec![Ty::VOID; locals];
        builder
    }

    fn storage_read(body: &mut TirBuilder, root: PlaceRoot) -> hd_tir::ir::Ref {
        let word = body.place(&hd_tir::ir::Place { root, proj: vec![] });
        body.emit(
            Tag::Read,
            word,
            hd_tir::ir::ReadMode::Use as u32,
            Ty::VOID,
            hd_base::NodeIdx::NONE,
        )
    }

    #[test]
    fn storage_roots_follow_nested_reads_but_stop_at_named_copies() {
        let mut body = storage_body(2);
        let first = storage_read(&mut body, PlaceRoot::Local(LocalId::from_raw(0)));
        let nested = storage_read(&mut body, PlaceRoot::Value(first));
        let copied = storage_read(&mut body, PlaceRoot::Local(LocalId::from_raw(1)));
        let copied_child = storage_read(&mut body, PlaceRoot::Value(copied));
        let global = storage_read(&mut body, PlaceRoot::Global(DefId::from_raw(0), Ty::VOID));
        let global_child = storage_read(&mut body, PlaceRoot::Value(global));
        let roots = StorageRoots::new(body.body_mut());
        let place = |value| hd_tir::ir::Place {
            root: PlaceRoot::Value(value),
            proj: vec![],
        };
        assert_eq!(
            roots.binding(&place(nested)),
            Some(PlaceRoot::Local(LocalId::from_raw(0)))
        );
        assert_eq!(
            roots.binding(&place(copied_child)),
            Some(PlaceRoot::Local(LocalId::from_raw(1)))
        );
        assert_eq!(
            global_root(&roots, &place(global_child)),
            Some((DefId::from_raw(0), Ty::VOID))
        );
    }

    #[test]
    fn storage_root_preparation_and_queries_have_bounded_work() {
        for (width, depth) in [(100, 20), (1000, 20), (20, 1000)] {
            let mut body = storage_body(1);
            let first = storage_read(&mut body, PlaceRoot::Local(LocalId::from_raw(0)));
            let mut previous = vec![first; width];
            for _ in 0..depth {
                for value in &mut previous {
                    *value = storage_read(&mut body, PlaceRoot::Value(*value));
                }
            }
            let mut work = 0;
            let roots = StorageRoots::prepare(body.body_mut(), |n| work += n);
            assert_eq!(work, 2 * body.body_mut().len());
            for value in previous {
                let place = hd_tir::ir::Place {
                    root: PlaceRoot::Value(value),
                    proj: vec![],
                };
                for _ in 0..10 {
                    assert_eq!(
                        roots.binding(&place),
                        Some(PlaceRoot::Local(LocalId::from_raw(0)))
                    );
                }
            }
        }
    }

    fn body(locals: usize, edges: &[(u32, u32)]) -> Body {
        let mut b = Body::new(DefId::from_raw(0), hd_tir::ir::BodyKind::Fn);
        b.local_ty = vec![Ty::VOID; locals];
        b.inlined_capture_origins = edges
            .iter()
            .map(|&(s, c)| {
                (
                    SubId::from_raw(0),
                    LocalId::from_raw(s),
                    LocalId::from_raw(c),
                )
            })
            .collect();
        b
    }

    #[test]
    fn only_whole_pointer_capture_keys_inherit_creator_context() {
        let b = body(5, &[(1, 0), (2, 1), (3, 0)]);
        let origins = CaptureOrigins::new(&b, &[], |l| l.raw() != 3);
        let root = PlaceRoot::Local(LocalId::from_raw(2));
        assert_eq!(
            origins.key((root, vec![], ANY_FRAME)).0,
            PlaceRoot::Local(LocalId::from_raw(0))
        );
        let field = (root, vec![Proj::Field(0)], ANY_FRAME);
        assert_eq!(origins.key(field.clone()), field);
        let cell = (root, vec![], 0);
        assert_eq!(origins.key(cell.clone()), cell);
        assert_eq!(origins.local(3), 3, "non-pointer capture has no exemption");
        assert_eq!(origins.local(4), 4, "ordinary aliases have no provenance");
    }

    #[test]
    fn loaded_capture_cells_route_creator_and_snapshot_lookups() {
        let body = body(4, &[(1, 0), (2, 1)]);
        let mut borrows = Borrows {
            capture_origins: CaptureOrigins::new(&body, &[], |_| true),
            ..Borrows::default()
        };
        // An ordinary capture has no source routing entry.
        borrows.load_own_cell(3, 43, true);
        assert!(borrows.own_cell_sources.is_empty());
        assert_eq!(borrows.own_cell_source(3), 3);
        assert_eq!(borrows.own_cells[&3], (43, true));

        // The closure prologue loads a snapshot's cell. Later accesses
        // through either its creator or another eliminated snapshot must
        // find that same cell, including its borrowed-frame flag.
        borrows.load_own_cell(2, 41, false);
        for local in 0..3 {
            let source = borrows.own_cell_source(local);
            assert_eq!(source, 2);
            assert_eq!(borrows.own_cells[&source], (41, false));
        }
        assert_eq!(borrows.own_cell_sources.len(), 1);

        // A physical cell already held by the creator takes precedence.
        borrows.load_own_cell(0, 47, true);
        assert_eq!(borrows.own_cell_source(0), 0);
        assert_eq!(borrows.own_cells[&0], (47, true));
    }

    #[test]
    fn capture_provenance_chains_have_linear_counted_work() {
        for width in [100u32, 1_000] {
            let depth = 20;
            let mut edges = Vec::new();
            for layer in 0..depth {
                for column in 0..width {
                    let snapshot = 1 + layer * width + column;
                    let creator = if layer == 0 { 0 } else { snapshot - width };
                    edges.push((snapshot, creator));
                }
            }
            let n = 1 + (width * depth) as usize;
            let b = body(n, &edges);
            let origins = CaptureOrigins::new(&b, &[], |_| true);
            assert_eq!(origins.work, 2 * n + edges.len());
            for l in 0..n {
                assert_eq!(origins.local(l), 0);
            }
        }
    }
}

/// The inlined parameters that receive a lent `*T` argument (the `Read` a
/// `LoanBegin` names), each with the pointer local it was read from: they
/// reach the object through that pointer, so their accesses are the loan's
/// own. One whole store each. Cost: two passes over the body, once per body
/// ([`BorrowFacts`]).
fn lent_params(b: &Body) -> Vec<(u32, u32)> {
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
    for i in 0..b.len() {
        let Some((l, true)) = (b.tags[i] == Tag::Store)
            .then(|| b.stored_local(i))
            .flatten()
        else {
            continue;
        };
        let Some(v) = b
            .direct_value(b.data[i][1])
            .filter(|&v| sites[v] && b.tags[v] == Tag::Read)
        else {
            continue;
        };
        let place = b.view(b.data[v][0]);
        if let Some(p) = place.local().filter(|_| place.whole())
            && stores[l.idx()] == 1
        {
            out.push((l.raw(), p));
        }
    }
    out
}

impl CaptureOrigins {
    fn new(
        b: &Body,
        lent_params: &[(u32, u32)],
        eligible: impl Fn(hd_base::LocalId) -> bool,
    ) -> Self {
        let mut lent: Vec<Option<hd_base::LocalId>> = Vec::new();
        for &(l, p) in lent_params {
            if eligible(hd_base::LocalId::from_raw(l)) {
                lent.resize(b.local_ty.len(), None);
                lent[l as usize] = Some(hd_base::LocalId::from_raw(p));
            }
        }
        if b.inlined_capture_origins.is_empty() && lent.is_empty() {
            return Self::default();
        }
        let lent_params = lent.iter().map(Option::is_some).collect();
        let mut roots = vec![None; b.local_ty.len()];
        #[cfg(test)]
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
            #[cfg(test)]
            {
                work += 1;
            }
        }
        for (l, p) in lent.iter().enumerate() {
            if roots[l].is_none() {
                roots[l] = *p;
            }
        }
        for l in 0..roots.len() {
            if let Some(parent) = roots[l] {
                roots[l] = roots[parent.idx()].or(Some(parent));
            }
            #[cfg(test)]
            {
                work += 1;
            }
        }
        Self {
            roots,
            lent: lent_params,
            #[cfg(test)]
            work,
        }
    }

    /// Whether local `l` is an inlined parameter that takes a lent
    /// argument's pointer, whose object the argument's loan holds.
    fn is_lent(&self, l: usize) -> bool {
        self.lent.get(l).copied().unwrap_or(false)
    }

    fn local(&self, l: usize) -> usize {
        self.roots
            .get(l)
            .copied()
            .flatten()
            .map_or(l, hd_base::LocalId::idx)
    }

    fn key(&self, mut key: Key) -> Key {
        if key.1.is_empty()
            && key.2 == ANY_FRAME
            && let PlaceRoot::Local(l) = key.0
            && let Some(root) = self.roots.get(l.idx()).copied().flatten()
        {
            key.0 = PlaceRoot::Local(root);
        }
        key
    }
}

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

/// Immutable frame and lifetime facts shared by sibling emitters.
pub(super) struct BorrowFacts {
    frames: std::sync::Arc<[u32]>,
    lent_params: Vec<(u32, u32)>,
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
pub(super) struct Borrows {
    capture_origins: CaptureOrigins,
    storage_roots: std::sync::Arc<StorageRoots>,
    /// The loan sites (`Ref`s, and `*T` reads a call lends), and the
    /// objects each one crossed, once emitted: the header address, the
    /// pointer's key, and whether elision keeps that loan
    /// (`hd_tir::own::pointer`).
    refs: HashMap<u32, Vec<(u32, Option<usize>, bool)>>,
    /// The `*T` reads a call lends: whether as `&mut`, and whether as a
    /// two-phase receiver, shared until its call.
    lent: HashMap<u32, (bool, bool)>,
    /// Each `LoanBegin`: how many objects its site may cross, and once
    /// the function has locals, a slot per object: the local of the
    /// header address (0 while no loan is held) and of the pointer's key
    /// index plus one.
    begins: HashMap<u32, (usize, Vec<(u32, u32)>)>,
    /// Each site's `LoanBegin`, and per two-phase `LoanBegin` the local
    /// set once its call activates it, so its last object's loan is
    /// exclusive.
    site_begin: HashMap<u32, u32>,
    active: HashMap<u32, u32>,
    /// The sites that are `*T` values rather than places.
    valued: hd_base::HashSet<u32>,
    /// The pointers places reach objects through, and the local holding
    /// the state of the function's own loans through each, once used.
    keys: Vec<(Key, Option<u32>)>,
    /// Stable vector IDs, indexed without scanning other pointers.
    key_index: HashMap<Key, usize>,
    #[cfg(test)]
    key_work: usize,
    /// The first `loans` keys are those a loan is taken through, interned
    /// before emission.
    loans: usize,
    /// Set once a frame's release glue is emitted ([`Em::borrow_abandon`]):
    /// the body's locals are gone, and a cold frame holds no loan.
    abandoned: bool,
    /// The locals a closure shares: each lives in a cell, an object its
    /// places cross at their root ([`ROOT`]).
    shared: std::sync::Arc<hd_base::HashSet<usize>>,
    /// The inlined copy each instruction came from ([`frames_of`]).
    frames: std::sync::Arc<[u32]>,
    /// The local references that hold only `*T` values
    /// ([`pointer_locals`]).
    pointers: std::sync::Arc<hd_base::HashSet<usize>>,
    /// The `*T` locals a closure of the body captures with its creator's
    /// own loans through them ([`Em::own_caps`]).
    own_shared: hd_base::HashSet<usize>,
    /// The cell holding the own loans through each of them: the local of
    /// its address, and whether this body holds a count of it, which its
    /// end or abandonment releases: one it made, or a suspending closure's,
    /// whose frame counts its environment's cells.
    own_cells: HashMap<usize, (u32, bool)>,
    /// A canonical creator's loaded capture cell, when its environment
    /// names a snapshot. Keep ownership keyed by the actual loaded local:
    /// distinct environment fields still release their own held counts.
    own_cell_sources: HashMap<usize, usize>,
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
                let TyData::Adt { args, .. } = pool.get(strip_newtype(pool, env, ty)) else {
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

/// The step index of a place's root object: a variable a closure shares
/// (its cell) or a top-level binding that is an object, which the place
/// crosses before its first step.
pub(super) const ROOT: usize = u32::MAX as usize - 2;

/// The terminal storage binding of each value-backed Read chain. This is
/// prepared once per body, not by walking a chain at every loan/access.
#[derive(Default)]
pub(crate) struct StorageRoots {
    reads: Vec<Option<PlaceRoot>>,
}

impl StorageRoots {
    pub(crate) fn new(body: &Body) -> Self {
        Self::prepare(body, |_| {})
    }

    fn prepare(body: &Body, mut measure: impl FnMut(usize)) -> Self {
        let mut reads = vec![None; body.len()];
        measure(body.len());
        // The verifier orders a Read's value-root producer before that Read.
        // Reading a named binding stops here: a stored counted copy does not
        // inherit the original binding's loan identity.
        for i in 0..body.len() {
            measure(1);
            if body.tags[i] != Tag::Read {
                continue;
            }
            reads[i] = match body.view(body.data[i][0]).root {
                hd_tir::ir::Root::Local(local) => {
                    Some(PlaceRoot::Local(hd_base::LocalId::from_raw(local)))
                }
                hd_tir::ir::Root::Global(def, ty) => Some(PlaceRoot::Global(def, ty)),
                hd_tir::ir::Root::Value(value) => reads.get(value as usize).copied().flatten(),
                _ => None,
            };
        }
        Self { reads }
    }

    fn binding(&self, place: &hd_tir::ir::Place) -> Option<PlaceRoot> {
        match place.root {
            root @ (PlaceRoot::Local(_) | PlaceRoot::Global(..)) => Some(root),
            PlaceRoot::Value(value) => value
                .as_inst()
                .and_then(|value| self.reads.get(value.0 as usize).copied().flatten()),
            _ => None,
        }
    }
}

/// The top-level binding place `p` starts from, through its Read roots.
pub(crate) fn global_root(
    roots: &StorageRoots,
    p: &hd_tir::ir::Place,
) -> Option<(hd_base::DefId, Ty)> {
    match roots.binding(p)? {
        PlaceRoot::Global(def, ty) => Some((def, ty)),
        _ => None,
    }
}

/// The objects a place may start at: the locals a closure shares, and the
/// top-level bindings that are objects ([`Module::objects`]).
pub(super) struct Roots<'a> {
    pub(super) storage: &'a StorageRoots,
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
        match self.storage.binding(p)? {
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
    b: &Body,
    roots: &Roots<'_>,
    root: Ty,
    p: &hd_tir::ir::Place,
) -> Vec<(usize, Ty)> {
    let mut out = Vec::new();
    if let Some((_, t)) = roots.of(b, p) {
        out.push((ROOT, t));
    }
    if let Some(t) = pointer_root(pool, roots.env, b, roots.pointers, p) {
        out.push((0, t));
    }
    if let Some((_, q)) = root_read(b, p)
        && let Some(qt) = root_ty(b, q.root)
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
    stores: &[usize],
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
    for &i in stores {
        let Some((l, true)) = b.stored_local(i) else {
            continue;
        };
        let l = l.idx();
        if !thin_local(b.local_ty[l]) {
            continue;
        }
        let pointer = b
            .direct_value(b.data[i][1])
            .is_some_and(|v| whole_ptr(b.ty[v]));
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
    b: &Body,
    pointers: &hd_base::HashSet<usize>,
    p: &hd_tir::ir::Place,
) -> Option<Ty> {
    let PlaceRoot::Local(l) = p.root else {
        return None;
    };
    if p.proj.first() != Some(&Proj::Deref) || !pointers.contains(&l.idx()) {
        return None;
    }
    match kind(pool, env, b.local_ty[l.idx()]) {
        Kind::Ref { target, .. } => Some(target),
        _ => None,
    }
}

/// The type of a place's root, `sub` giving a type at this instance.
pub(super) fn root_ty(b: &Body, root: PlaceRoot) -> Option<Ty> {
    match root {
        PlaceRoot::Local(l) => Some(b.local_ty[l.idx()]),
        PlaceRoot::Global(_, t) => Some(t),
        PlaceRoot::Value(r) => r.as_inst().map(|v| b.ty[v.0 as usize]),
        PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
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

impl Borrows {
    /// Expected O(key projection words); IDs retain first-insertion order.
    fn intern_key(&mut self, key: Key) -> usize {
        let key = self.capture_origins.key(key);
        #[cfg(test)]
        {
            self.key_work += 1 + key.1.len();
        }
        match self.key_index.entry(key) {
            hd_base::hash::hash_map::Entry::Occupied(entry) => *entry.get(),
            hd_base::hash::hash_map::Entry::Vacant(entry) => {
                let index = self.keys.len();
                #[cfg(test)]
                {
                    self.key_work += 1 + entry.key().1.len();
                }
                self.keys.push((entry.key().clone(), None));
                entry.insert(index);
                index
            }
        }
    }

    fn load_own_cell(&mut self, local: usize, cell: u32, held: bool) {
        self.own_cells.insert(local, (cell, held));
        let root = self.capture_origins.local(local);
        if root != local {
            self.own_cell_sources.insert(root, local);
        }
    }

    fn own_cell_source(&self, local: usize) -> usize {
        if self.own_cells.contains_key(&local) {
            return local;
        }
        let root = self.capture_origins.local(local);
        self.own_cell_sources.get(&root).copied().unwrap_or(root)
    }
}

impl Em<'_, '_, '_> {
    /// Before emission: the sub-body's loan sites, from its `LoanBegin`s,
    /// which the checker placed (`hd_tir::own::loans`), and the pointers
    /// their loans go through, whose own-loan counts the access checks
    /// compare with. Cost: one pass over the body's instructions.
    pub(super) fn find_borrows(&mut self, closure: Option<usize>) {
        if self.m.lent.is_empty() {
            return;
        }
        // collect_loans indexed every emitted instance's own sub-body.
        let holder = std::sync::Arc::clone(&self.m.loan_holders[self.inst.idx()]);
        let facts = std::sync::Arc::clone(&self.borrow_facts);
        let immutable = facts.get_or_init(|| BorrowFacts::new(self.b));
        let mut st = Borrows {
            capture_origins: CaptureOrigins::new(self.b, &immutable.lent_params, |l| {
                self.own_pointer(l)
            }),
            storage_roots: std::sync::Arc::clone(&holder.storage),
            shared: std::sync::Arc::clone(&holder.shared),
            frames: std::sync::Arc::clone(&immutable.frames),
            pointers: std::sync::Arc::clone(&holder.pointers),
            ..Borrows::default()
        };
        // A closure inherits its creator's own-loan cells for captured
        // pointers even when this sub-body takes no reference itself.
        if let Some(ci) = closure {
            for local in self.own_capture_ids(ci) {
                st.intern_key((PlaceRoot::Local(local), Vec::new(), ANY_FRAME));
            }
        }
        let sub = u32::from(self.sub_id);
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
            // The object a call's result points into: one slot.
            if word & loan_word::INNER != 0 {
                if kept {
                    st.begins
                        .insert(u32::try_from(i).expect("insts"), (1, Vec::new()));
                }
                continue;
            }
            // A site an ownership pass forwarded to a `*T` value: its
            // whole object, at the value's address.
            if !matches!(self.b.tags[site as usize], Tag::Ref | Tag::Read) {
                st.valued.insert(site);
                if kept {
                    st.begins
                        .insert(u32::try_from(i).expect("insts"), (1, Vec::new()));
                }
                continue;
            }
            let Some(p) = self.b.decode_place(self.b.data[site as usize][0]) else {
                continue;
            };
            match self.b.tags[site as usize] {
                Tag::Ref => {}
                // A `*T` a call lends: its whole object, through the
                // pointer the read's place names (a `*dyn` receiver's
                // pointer is the place before its dereference).
                Tag::Read => {
                    if matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..)) {
                        let mut q = p.clone();
                        if dyn_receiver(self.pool(), self.env(), self.b, site as usize).is_some() {
                            q.proj.pop();
                        }
                        let key = st.capture_origins.key((q.root, q.proj, ANY_FRAME));
                        st.intern_key(key);
                    }
                    st.lent.insert(site, (word & loan_word::EXCL != 0, false));
                }
                _ => continue,
            }
            self.intern_ref_keys(&mut st, site);
            st.refs.insert(site, Vec::new());
            // Each object the site's place may cross: its root's, each
            // dereference on its read's path and its own, and a lent
            // pointer's whole object.
            let derefs =
                |q: &hd_tir::ir::Place| q.proj.iter().filter(|s| **s == Proj::Deref).count();
            let read = root_read(self.b, &p).map_or(0, |(_, q)| derefs(&q));
            let i = u32::try_from(i).expect("insts");
            if kept {
                st.begins.insert(i, (derefs(&p) + read + 3, Vec::new()));
            }
        }
        // A two-phase receiver lends `&mut`, shared until its call.
        for site in st.active.keys() {
            if let Some(x) = st.lent.get_mut(site) {
                *x = (true, true);
            }
        }
        st.loans = st.keys.len();
        for ci in 0..self.b.tags.len() {
            if self.b.tags[ci] == Tag::Closure {
                st.own_shared.extend(
                    self.own_caps(ci)
                        .into_iter()
                        .map(|l| st.capture_origins.local(l)),
                );
            }
        }
        self.borrows = st;
    }

    /// Once the function has its locals: each loan's slots, a header
    /// address and a pointer key per object its site may cross, zero
    /// while it holds no loan.
    pub(super) fn borrow_init_flags(&mut self) {
        let mut begins: Vec<u32> = self.borrows.begins.keys().copied().collect();
        begins.sort_stable();
        for i in begins {
            let n = self.borrows.begins[&i].0;
            let slots = (0..n)
                .map(|_| (self.f.local(Val::I32), self.f.local(Val::I32)))
                .collect();
            self.borrows.begins.get_mut(&i).expect("begin").1 = slots;
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
        if word & loan_word::INNER != 0 {
            return self.loan_inner(i, site, word & loan_word::EXCL != 0);
        }
        if self.borrows.valued.contains(&site) {
            self.loan_value(site);
        }
        let Some(taken) = self.borrows.refs.get(&site).cloned() else {
            return;
        };
        let Some((_, slots)) = self.borrows.begins.get(&i).cloned() else {
            return;
        };
        debug_assert!(
            taken.len() <= slots.len(),
            "a site's crossings fit its slots"
        );
        let excl = word & loan_word::EXCL != 0;
        for (s, (header, key, keep)) in taken.into_iter().rev().enumerate() {
            if !keep {
                continue;
            }
            let (h, kl) = slots[s];
            let delta = if s == 0 && excl {
                LOAN_EXCL
            } else {
                LOAN_SHARED
            };
            self.f.get(h);
            self.f.op(I::If(BlockType::Empty));
            self.slot_end(i, s, h, kl, delta);
            self.f.op(I::End);
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
            // A `*dyn` receiver's object may be absent: header 0.
            let nullable = self.borrows.nullable.contains(&header);
            if nullable {
                self.f.get(h);
                self.f.op(I::If(BlockType::Empty));
            }
            self.borrow_add(h, kl, delta, 1);
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

    /// `LoanBegin` `i` of call `site`'s INNER loan: on the object its
    /// returned reference points into, which the page table finds, where
    /// the callee reached it through a pointer past the argument's own
    /// object (`hd_tir::own::pointer`). The last run's loan ends first.
    fn loan_inner(&mut self, i: u32, site: u32, excl: bool) {
        if !self.m.kept.contains(&(self.inst, site, u32::MAX - 1)) {
            return;
        }
        let Some((_, slots)) = self.borrows.begins.get(&i).cloned() else {
            return;
        };
        let Some(&r) = self.vals.get(&site).and_then(|v| v.first()) else {
            return;
        };
        let (h, kl) = slots[0];
        let delta = if excl { LOAN_EXCL } else { LOAN_SHARED };
        let mask = if excl { LOAN_STATE } else { LOAN_EXCL };
        self.f.get(h);
        self.f.op(I::If(BlockType::Empty));
        self.borrow_add(h, kl, delta, -1);
        self.f.op(I::End);
        let rt = self.m.runtime;
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
        self.borrow_add(h, kl, delta, 1);
        self.f.op(I::End);
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
        let Some((_, slots)) = self.borrows.begins.get(&begin).cloned() else {
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

    /// Ends slot `s` of `LoanBegin` `begin` (header local `h`, key local
    /// `kl`), whose loan is `delta`, or exclusive once its two-phase call
    /// activated it.
    fn slot_end(&mut self, begin: u32, s: usize, h: u32, kl: u32, delta: u32) {
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
        let (Some(&flag), Some((_, slots))) = (
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

    /// At the function's exit: the own-loan cells it holds go.
    pub(super) fn borrow_exit(&mut self) {
        self.own_cells_release();
    }

    /// Whether the body takes a pointer loan or holds an own-loan cell.
    pub(super) fn borrow_needs_exit(&self) -> bool {
        !self.borrows.begins.is_empty() || !self.borrows.own_shared.is_empty()
    }

    /// The captures of closure `ci` that carry its creator's own loans
    /// (`docs/compiler/ownership.md#the-mechanism-in-detail-formerly-e2`): each `*T`
    /// local whose pointee a loan may
    /// hold. The closure checks an access through one against the loans
    /// its creator holds through it, which a cell in its environment
    /// keeps, as the creator's own local does.
    pub(super) fn own_caps(&self, ci: usize) -> Vec<usize> {
        self.own_capture_ids(ci)
            .map(hd_base::LocalId::idx)
            .collect()
    }

    /// Keeps the capture column's typed IDs through own-loan discovery.
    fn own_capture_ids(&self, ci: usize) -> impl Iterator<Item = hd_base::LocalId> + '_ {
        let rec = self.b.record(self.b.data[ci][1]);
        let (start, len) = (rec[0] as usize, rec[1] as usize);
        let captures = if self.m.lent.is_empty() {
            &[][..]
        } else {
            &self.b.cap_local[start..start + len]
        };
        let own = self.recursive_bindings.self_local(ci);
        captures
            .iter()
            .copied()
            .filter(move |l| Some(*l) != own && self.own_pointer(*l))
    }

    fn own_pointer(&self, l: hd_base::LocalId) -> bool {
        matches!(self.kind(self.b.local_ty[l.idx()]), Kind::Ref {
                target,
                fat: false,
                whole: true,
            } if self.lent_of(target).is_some())
    }

    /// The index of pointer local `l`'s own key, when the body takes a
    /// loan through it.
    fn own_key(&self, l: usize) -> Option<usize> {
        let local = hd_base::LocalId::from_raw(u32::try_from(l).ok()?);
        let key =
            self.borrows
                .capture_origins
                .key((PlaceRoot::Local(local), Vec::new(), ANY_FRAME));
        let k = *self.borrows.key_index.get(&key)?;
        (k < self.borrows.loans).then_some(k)
    }

    /// The local holding the address of the cell of the own loans through
    /// captured pointer local `l`, 0 until the body makes it.
    fn own_cell(&mut self, l: usize) -> u32 {
        if let Some(&(c, _)) = self.borrows.own_cells.get(&l) {
            return c;
        }
        let l = self.own_cell_source(l);
        if let Some(&(c, _)) = self.borrows.own_cells.get(&l) {
            return c;
        }
        let c = self.f.local(Val::I32);
        self.borrows.own_cells.insert(l, (c, true));
        c
    }

    fn own_cell_source(&self, l: usize) -> usize {
        self.borrows.own_cell_source(l)
    }

    /// At a closure that captures pointer local `l`: the cell of the own
    /// loans through it, for its environment, which takes a count of it.
    /// The body makes it once, at its first such closure, and keeps it
    /// current as its own loans change, across suspensions too.
    pub(super) fn own_cell_for(&mut self, l: usize) -> StageResult<u32> {
        let own = self.own_key(l).map(|k| self.own_local(k));
        let m = crate::layout::mem(self.pool(), self.env(), Ty::I32)?;
        let c = self.own_cell(l);
        if self
            .borrows
            .own_cells
            .get(&self.own_cell_source(l))
            .is_some_and(|x| !x.1)
        {
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
            self.borrows.load_own_cell(l, c, held);
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
    /// ones too, change one count
    /// (`docs/compiler/ownership.md#the-mechanism-in-detail-formerly-e2`).
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
            .get(&self.own_cell_source(l))
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
    fn data_address(&mut self, at: &super::refs::At) -> u32 {
        match at {
            super::refs::At::Vals(ls) if !ls.is_empty() => ls[0],
            _ => self.at_i32(at),
        }
    }

    /// The objects this body's places may start at.
    fn roots<'s>(
        &'s self,
        shared: &'s hd_base::HashSet<usize>,
        pointers: &'s hd_base::HashSet<usize>,
        storage: &'s StorageRoots,
    ) -> Roots<'s> {
        Roots {
            storage,
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
        if let Some((r, _)) = self
            .roots(&st.shared, &st.pointers, &st.storage_roots)
            .of(self.b, &p)
        {
            let key = st
                .capture_origins
                .key((r, Vec::new(), st.frames[v as usize]));
            st.intern_key(key);
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
        if pointer_root && p.proj.first() == Some(&Proj::Deref) {
            st.intern_key((p.root, Vec::new(), ANY_FRAME));
        }
        for (k, _) in crossings(self.pool(), self.env(), root, &p.proj) {
            st.intern_key((p.root, p.proj[..k].to_vec(), ANY_FRAME));
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
        // Each dereference of a `*T`, with the type of the object it reaches.
        let root = self.at_root_ty(p.root);
        let mut crossed = Vec::new();
        // The root object: a shared variable's cell, but not at the store
        // that makes it, as no reference reaches a cell before it exists;
        // or a top-level binding's slot.
        let shared = std::mem::take(&mut self.borrows.shared);
        let pointers = std::mem::take(&mut self.borrows.pointers);
        let root_obj = self
            .roots(&shared, &pointers, &self.borrows.storage_roots)
            .of(self.b, &p)
            .filter(|(r, _)| match r {
                PlaceRoot::Local(l) => {
                    self.is_cell(l.idx()) && self.cells.get(&l.idx()) != Some(&i)
                }
                _ => true,
            });
        let ptr_root = { pointer_root(self.pool(), self.env(), self.b, &pointers, &p) };
        self.borrows.shared = shared;
        self.borrows.pointers = pointers;
        if let Some((_, t)) = root_obj {
            crossed.push((ROOT, t));
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
        let mut taken = Vec::new();
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
            // The object an inlined parameter's lent pointer reaches is
            // the argument's loan's: no second loan of its own on it.
            let lent_root = k == 0
                && matches!(p.root, PlaceRoot::Local(l) if self.borrows.capture_origins.is_lent(l.idx()));
            let keep = lends
                && !lent_root
                && (self.m.kept).contains(&(self.inst, i, u32::try_from(k).expect("steps")));
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
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(None);
        };
        if !matches!(
            p.root,
            PlaceRoot::Local(_) | PlaceRoot::Value(_) | PlaceRoot::Global(..)
        ) {
            return Ok(None);
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
            _ => return Ok(None),
        };
        if lent.is_none() {
            return Ok(None);
        }
        let pointer = p.proj.len() - usize::from(dyn_recv.is_some());
        let key = matches!(p.root, PlaceRoot::Local(_) | PlaceRoot::Global(..))
            .then(|| self.borrow_key((p.root, p.proj[..pointer].to_vec(), ANY_FRAME)));
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
        if keep || mutable || lent == Some(true) {
            let mask = if writes { LOAN_STATE } else { LOAN_EXCL };
            self.borrow_check(header, mask, key, target, writes);
        }
        if dyn_recv.is_some() {
            self.f.op(I::End);
        }
        Ok(Some((header, key, keep)))
    }

    /// Creator loan cells that the separate frame destructor reads.
    pub(super) fn select_frame_loan_cells(&self, selected: &mut [bool]) {
        for &(local, owned) in self.borrows.own_cells.values() {
            if owned {
                selected[local as usize] = true;
            }
        }
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
}
