//! Holder bindings for pointer loans (model A of
//! `docs/compiler/ownership.md#pointer-loans-model-a`, `borrow.rs`): which stores bind a
//! reference into a holder (a binding, or a struct, tuple or variant that
//! holds it), from a `Ref`, a call's result that depends on an argument
//! the checker found it holds ([`Body::held_args`]), or another holder by
//! a copy or a move; and the calls' arguments those loans read.

use hd_base::SliceSort as _;
use hd_layout::field_ty;
use hd_mono::{ProgramEnv, Target};
use hd_tir::ir::{Body, Callee, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};
use hd_types::{InternPool, Ty};

use crate::Input;
use crate::layout::{
    Kind, flat, kind, mem as mem_of, payload_mem, ref_mutable, strip_newtype, tuple_members,
    value_parts, variants,
};
use crate::module::Module;
use std::rc::Rc;
use std::sync::Arc;

/// Body-only lookups shared by all concrete instances and origin queries.
/// Preparation visits each instruction/coercion edge and held row once;
/// scope exits retain instruction order without rescanning the body.
#[derive(Debug, Default)]
pub(crate) struct OriginIndex {
    canonical: Vec<usize>,
    held: hd_base::HashMap<usize, Vec<u32>>,
    loan_slots: hd_base::HashMap<usize, Vec<hd_tir::ir::LoanSlot>>,
    breaks: hd_base::HashMap<usize, Vec<(usize, u32)>>,
}

impl OriginIndex {
    pub(super) fn new(b: &Body) -> Self {
        // No seed means the least fixed point of holder propagation is empty:
        // local reads only copy published holders, and none can be published.
        if b.held_args.is_empty() && !b.tags.contains(&Tag::Ref) {
            return Self::default();
        }
        Self::prepare(b, |_| {})
    }

    fn prepare(b: &Body, mut work: impl FnMut(usize)) -> Self {
        let mut canonical = vec![usize::MAX; b.len()];
        let mut state = vec![0u8; b.len()];
        let mut path = Vec::new();
        for start in 0..b.len() {
            work(1);
            if state[start] == 2 {
                continue;
            }
            let mut at = Some(start);
            let root = loop {
                work(1);
                let Some(value) = at.filter(|&v| v < b.len()) else {
                    break usize::MAX;
                };
                if state[value] == 2 {
                    break canonical[value];
                }
                if state[value] == 1 {
                    // A pure alias ring has no reference-producing node.
                    break usize::MAX;
                }
                state[value] = 1;
                path.push(value);
                if !b.same_value_coercion(value) {
                    break value;
                }
                at = Ref(b.data[value][0]).as_inst().map(|v| v.0 as usize);
            };
            for value in path.drain(..).rev() {
                work(1);
                canonical[value] = root;
                state[value] = 2;
            }
        }
        let mut held: hd_base::HashMap<usize, Vec<u32>> = hd_base::HashMap::default();
        for &(value, argument) in &b.held_args {
            work(1);
            if (value as usize) < b.len() {
                held.entry(value as usize).or_default().push(argument);
            }
        }
        let mut loan_slots: hd_base::HashMap<usize, Vec<hd_tir::ir::LoanSlot>> =
            hd_base::HashMap::default();
        for row in &b.loan_slots {
            work(1 + row.path.len());
            loan_slots
                .entry(row.call as usize)
                .or_default()
                .push(row.clone());
        }
        let mut breaks: hd_base::HashMap<usize, Vec<(usize, u32)>> = hd_base::HashMap::default();
        for (i, &tag) in b.tags.iter().enumerate() {
            work(1);
            if tag == Tag::Break
                && b.data[i][1] != hd_tir::ir::NONE
                && let Some(&target) = b.label_inst.get(b.data[i][0] as usize)
                && (target as usize) < b.len()
            {
                breaks
                    .entry(target as usize)
                    .or_default()
                    .push((i, b.data[i][1]));
            }
        }
        Self {
            canonical,
            held,
            loan_slots,
            breaks,
        }
    }

    fn value(&self, value: usize) -> Option<usize> {
        self.canonical
            .get(value)
            .copied()
            .filter(|&root| root != usize::MAX)
    }

    fn query<T>(&self, value: usize, query: impl FnOnce(usize) -> Vec<T>) -> Vec<T> {
        self.value(value).map_or_else(Vec::new, query)
    }

    fn arguments(&self, value: usize) -> &[u32] {
        self.held.get(&value).map_or(&[], Vec::as_slice)
    }

    /// Tail values precede breaks; the two labels of a Scope interleave
    /// their breaks in the original instruction order, not label order.
    fn returns<'a>(&'a self, b: &'a Body, value: usize) -> impl Iterator<Item = u32> + 'a {
        let inner = (b.tags[value] == Tag::Scope).then_some(b.data[value][0] as usize);
        let tail = |at: usize| {
            if b.tags.get(at) == Some(&Tag::Block) {
                b.data[at][1]
            } else {
                hd_tir::ir::NONE
            }
        };
        let tails = [tail(value), inner.map_or(hd_tir::ir::NONE, tail)];
        let mut left = self
            .breaks
            .get(&value)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .peekable();
        let mut right = inner
            .filter(|&at| at != value)
            .and_then(|at| self.breaks.get(&at))
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .peekable();
        let exits = std::iter::from_fn(move || {
            let take_left = match (left.peek(), right.peek()) {
                (Some(a), Some(b)) => a.0 <= b.0,
                (Some(_), None) => true,
                _ => false,
            };
            if take_left { left.next() } else { right.next() }.map(|&(_, value)| value)
        });
        tails
            .into_iter()
            .filter(|&v| v != hd_tir::ir::NONE)
            .chain(exits)
    }
}

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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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

/// Published holder slots in first-discovery order, indexed by local.
/// Publishing the same slot again does no work for subsequent read queries.
#[derive(Default)]
struct KnownOrigins {
    locals: hd_base::HashMap<usize, Vec<usize>>,
    positions: hd_base::HashMap<Held, usize>,
}

impl KnownOrigins {
    fn publish(&mut self, held: Held) {
        if self.positions.contains_key(&held) {
            return;
        }
        let slots = self.locals.entry(held.local).or_default();
        self.positions.insert(held, slots.len());
        slots.push(held.slot);
    }

    #[cfg(test)]
    fn slots(&self, local: usize) -> &[usize] {
        self.locals.get(&local).map_or(&[], Vec::as_slice)
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
pub(super) fn fields(steps: &[Proj]) -> Option<Vec<u32>> {
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
fn arg_loan(pool: &InternPool, env: &dyn ProgramEnv, b: &Body, a: u32) -> Option<Loan> {
    let v = b.direct_value(a)?;
    let excl = ref_mutable(pool, env, *b.ty.get(a as usize)?);
    if b.tags[v] == Tag::Ref {
        let (_, (object, path, guards)) = ref_loan_place(pool, env, b, v)?;
        return Some(Loan {
            object,
            path,
            guards: guards.into_iter().map(|(_, g)| g).collect(),
            excl,
        });
    }
    let t = *b.ty.get(v)?;
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
    v: usize,
) -> Option<(Place, LoanPlace)> {
    let p = b.decode_place(b.data[v][0])?;
    let root = match p.root {
        PlaceRoot::Local(r) => b.local_ty[r.idx()],
        PlaceRoot::Global(_, t) => t,
        _ => return None,
    };
    let place = loan_place(pool, env, root, &p)?;
    Some((p, place))
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
        // A map probe, `Map.get_or_insert(&mut self, key, default)`: its
        // target keeps only the key's hash and equality, not the item, and
        // collection gives a `Call` such a target for no other callee.
        Target::MapKey { .. } | Target::MapInline(_) => Some(Some(true)),
        _ => None,
    }
}

/// The flattened value slots of `t` that hold local references, and each
/// slot's reference type. Struct and tuple fields retain their layout
/// positions; a nullable `Option[&T]` uses its payload's sole slot.
fn ref_slots(pool: &InternPool, env: &dyn ProgramEnv, t: Ty) -> Vec<(usize, Ty)> {
    use super::ref_slots::{Shape, for_type};
    for_type(strip_newtype(pool, env, t), |t| {
        if matches!(
            kind(pool, env, t),
            Kind::Ref {
                whole: false,
                fat: false,
                ..
            }
        ) {
            return Shape::Leaf;
        }
        if let Some(inner) = pool.sole_arg(t, env.option()) {
            let positions = flat(pool, env, t).ok().flatten().and_then(|flat| {
                flat.positions
                    .get(1)?
                    .first()
                    .map(|slots| slots.iter().map(|&slot| slot as usize).collect())
            });
            return positions.map_or_else(
                || Shape::Fields(Vec::new()),
                |slots| Shape::Mapped(strip_newtype(pool, env, inner), slots),
            );
        }
        if !matches!(kind(pool, env, t), Kind::Value) || variants(pool, env, t).is_some() {
            return Shape::Fields(Vec::new());
        }
        let fields = value_parts(pool, env, t);
        let Ok((_, parts)) = payload_mem(pool, env, &fields) else {
            return Shape::Fields(Vec::new());
        };
        Shape::Fields(
            fields
                .into_iter()
                .zip(parts)
                .map(|(field, part)| (part.first, strip_newtype(pool, env, field)))
                .collect(),
        )
    })
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
        ty = strip_newtype(pool, env, ty);
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

/// Whether holder slot `held` holds a `&mut`.
pub(super) fn held_mutable(pool: &InternPool, env: &dyn ProgramEnv, b: &Body, held: Held) -> bool {
    ref_slots(pool, env, b.local_ty[held.local])
        .into_iter()
        .any(|(slot, t)| slot == held.slot && ref_mutable(pool, env, t))
}

/// The `Ref`s whose loans argument value `v` carries through calls'
/// results (including an inlined getter's containing-object origins),
/// when it is no `Ref` itself, as `observe(p.items.get_mut(1)
/// .expect("slot"))` passes the `&mut p.items` it got: each takes its call
/// loan until the call that `v` is an argument of returns.
pub(super) fn carried_refs(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    index: &OriginIndex,
    graph: &mut OriginGraph,
    v: usize,
) -> Vec<u32> {
    if b.tags.get(v) == Some(&Tag::Ref)
        || b.tags.get(v) == Some(&Tag::Read) && ref_slots(pool, env, b.ty[v]).is_empty()
    {
        return Vec::new();
    }
    index.query(v, |value| {
        let context = OriginContext {
            pool,
            env,
            body: b,
            index,
            call_argument: true,
        };
        let mut out = Vec::new();
        let mut seen = hd_base::HashSet::default();
        for (_, bind, _) in graph.collect(value, |c, graph| context.prepare(c, graph)) {
            // Inlining exposes the returned Ref directly. Its argument's
            // containing objects remain a separate origin: the local &self
            // inside the scope does not itself cross their counted pointers.
            let reference = match bind {
                Bind::Begin(r) | Bind::Call(r) | Bind::CallParent(r) => Some(r),
                _ => None,
            };
            if let Some(r) = reference
                .and_then(|r| index.value(r as usize))
                .filter(|&r| b.tags[r] == Tag::Ref)
            {
                let r = u32::try_from(r).expect("insts"); // ruled out: u32 TIR IDs.
                if seen.insert(r) {
                    out.push(r);
                }
            }
        }
        out
    })
}

/// Shared, interned endpoint unions. Forwarding chains reuse one root;
/// branching adds one DAG edge per held-argument row, without copying a
/// transitive endpoint list into every intermediate value. Queries are
/// iterative and cached by root, so aliases of a chain share the answer.
#[derive(Default)]
pub(super) struct ValueEnds {
    roots: Vec<Option<usize>>,
    nodes: Vec<EndNode>,
    points: hd_base::HashMap<u32, usize>,
    unions: hd_base::HashMap<[usize; 2], usize>,
    calls: hd_base::HashMap<u32, usize>,
}

enum EndNode {
    Point(u32),
    Union([usize; 2]),
    Call { at: u32, forwarded: Option<usize> },
}

impl ValueEnds {
    fn point(&mut self, at: u32) -> usize {
        *self.points.entry(at).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push(EndNode::Point(at));
            id
        })
    }

    fn join(&mut self, left: Option<usize>, right: Option<usize>) -> Option<usize> {
        let (Some(left), Some(right)) = (left, right) else {
            return left.or(right);
        };
        if left == right {
            return Some(left);
        }
        let children = [left.min(right), left.max(right)];
        Some(*self.unions.entry(children).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push(EndNode::Union(children));
            id
        }))
    }

    fn call(&mut self, at: u32) -> usize {
        *self.calls.entry(at).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push(EndNode::Call {
                at,
                forwarded: self.roots[at as usize],
            });
            id
        })
    }
}

/// Concrete reference-slot decisions and endpoint answers belong to one
/// emitter. The immutable graph remains shared across body instances.
#[derive(Default)]
pub(super) struct EndValues {
    graph: Arc<ValueEnds>,
    empty: Arc<[u32]>,
    answers: std::cell::RefCell<hd_base::HashMap<usize, Arc<[u32]>>>,
    ref_types: std::cell::RefCell<hd_base::HashMap<Ty, bool>>,
    resolved: std::cell::RefCell<hd_base::HashMap<usize, Option<usize>>>,
    nodes: std::cell::RefCell<hd_base::HashMap<usize, ConcreteEnd>>,
}

#[derive(Clone, Copy)]
enum ConcreteEnd {
    Point(u32),
    Union([usize; 2]),
}

impl EndValues {
    fn new(graph: Arc<ValueEnds>) -> Self {
        Self {
            graph,
            ..Self::default()
        }
    }

    pub(super) fn get(
        &self,
        value: usize,
        b: &Body,
        pool: &InternPool,
        env: &dyn ProgramEnv,
    ) -> Arc<[u32]> {
        self.get_counted(
            value,
            |at| {
                let ty = b.ty[at as usize];
                *self
                    .ref_types
                    .borrow_mut()
                    .entry(ty)
                    .or_insert_with(|| !ref_slots(pool, env, ty).is_empty())
            },
            || crate::work::use_facts(1),
        )
    }

    /// Normalize type-dependent forwarding once per reachable node. A
    /// chain aliases its final point instead of being walked per query.
    fn resolve(
        &self,
        root: usize,
        returns_refs: &mut impl FnMut(u32) -> bool,
        work: &mut impl FnMut(),
    ) -> Option<usize> {
        let mut done = self.resolved.borrow_mut();
        let mut nodes = self.nodes.borrow_mut();
        if let Some(&answer) = done.get(&root) {
            return answer;
        }
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if done.contains_key(&id) {
                continue;
            }
            work();
            let answer = match self.graph.nodes[id] {
                EndNode::Point(at) => {
                    nodes.insert(id, ConcreteEnd::Point(at));
                    Some(id)
                }
                EndNode::Call { at, forwarded } if returns_refs(at) => {
                    let Some(child) = forwarded else {
                        done.insert(id, None);
                        continue;
                    };
                    if let Some(&answer) = done.get(&child) {
                        answer
                    } else {
                        pending.extend([id, child]);
                        continue;
                    }
                }
                EndNode::Call { at, .. } => {
                    nodes.insert(id, ConcreteEnd::Point(at));
                    Some(id)
                }
                EndNode::Union(children) => {
                    if children.iter().any(|c| !done.contains_key(c)) {
                        pending.push(id);
                        pending.extend(children.into_iter().filter(|c| !done.contains_key(c)));
                        continue;
                    }
                    match children.map(|c| done[&c]) {
                        [None, other] | [other, None] => other,
                        [Some(left), Some(right)] if left == right => Some(left),
                        [Some(left), Some(right)] => {
                            nodes.insert(id, ConcreteEnd::Union([left, right]));
                            Some(id)
                        }
                    }
                }
            };
            done.insert(id, answer);
        }
        done[&root]
    }

    fn get_counted(
        &self,
        value: usize,
        mut returns_refs: impl FnMut(u32) -> bool,
        mut work: impl FnMut(),
    ) -> Arc<[u32]> {
        work();
        let Some(root) = self.graph.roots.get(value).copied().flatten() else {
            return Arc::clone(&self.empty);
        };
        let Some(root) = self.resolve(root, &mut returns_refs, &mut work) else {
            return Arc::clone(&self.empty);
        };
        if let Some(answer) = self.answers.borrow().get(&root) {
            return Arc::clone(answer);
        }
        let mut pending = vec![root];
        let mut seen = hd_base::HashSet::default();
        let mut points = hd_base::HashSet::default();
        let mut out = Vec::new();
        let nodes = self.nodes.borrow();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            work();
            match nodes[&id] {
                ConcreteEnd::Point(at) => {
                    if points.insert(at) {
                        out.push(at);
                    }
                }
                ConcreteEnd::Union(children) => pending.extend(children),
            }
        }
        let out: Arc<[u32]> = out.into();
        self.answers.borrow_mut().insert(root, Arc::clone(&out));
        out
    }
}

/// Lifetime-ending uses include dependency-bearing call arguments even
/// when ownership does not count the borrowed argument as a consumer.
/// In particular, a temporary's Drop after a call cannot end the loan
/// before the call result reaches its holder. Like OSSA forwarding uses,
/// these edges extend the introducing borrow through the result's uses.
/// Preparation: O(instructions + call operands + held rows + drops), with shared unions.
fn value_ends(b: &Body, topo: &hd_tir::ir::Topology, consumers: &[u32]) -> ValueEnds {
    value_ends_counted(b, topo, consumers, || crate::work::use_facts(1))
}

fn value_ends_counted(
    b: &Body,
    topo: &hd_tir::ir::Topology,
    consumers: &[u32],
    mut work: impl FnMut(),
) -> ValueEnds {
    use hd_tir::ir::{NONE, u32_of};
    let mut ends = ValueEnds {
        roots: vec![None; b.len()],
        ..ValueEnds::default()
    };
    let held: hd_base::HashSet<_> = b.held_args.iter().copied().collect();
    let mut users = vec![Vec::new(); b.len()];
    for &(call, arg) in &b.held_args {
        work();
        if let Some(row) = users.get_mut(arg as usize) {
            row.push((call, true));
        }
    }
    for i in 0..b.len() {
        work();
        if b.tags[i] == Tag::Drop
            && let Some(p) = b.decode_place(b.data[i][0])
            && p.proj.is_empty()
            && let PlaceRoot::Value(v) = p.root
            && v.as_inst().is_some()
        {
            let at = ends.point(u32_of(i));
            ends.roots[v.0 as usize] = ends.join(ends.roots[v.0 as usize], Some(at));
        }
        if matches!(
            b.tags[i],
            Tag::Call | Tag::CallValue | Tag::DefaultCall | Tag::CallDyn
        ) {
            for arg in call_args(b, i) {
                work();
                if !held.contains(&(u32_of(i), arg))
                    && let Some(row) = users.get_mut(arg as usize)
                {
                    row.push((u32_of(i), false));
                }
            }
        }
    }
    for v in (0..b.len()).rev() {
        work();
        if !users[v].is_empty() {
            let mut root = None;
            for &(call, forwarded) in &users[v] {
                work();
                // ruled out: a call follows its SSA argument.
                debug_assert!(call as usize > v);
                let next = if forwarded {
                    Some(ends.call(call))
                } else {
                    // A borrowed argument's temporary loan ends at a call
                    // that does not return it, even without an ownership
                    // consumer. Its count-free Drop may emit no operation.
                    Some(ends.point(call))
                };
                root = ends.join(root, next);
            }
            ends.roots[v] = root;
            continue;
        }
        let Some(&user) = consumers.get(v).filter(|&&u| u != NONE) else {
            continue;
        };
        let u = user as usize;
        let forwarded = match b.tags[u] {
            Tag::Block => (!topo.is_root[u]).then_some(topo.owner[u]),
            Tag::Break => b.label_inst.get(b.data[u][0] as usize).map(|&block| {
                let owner = topo.owner[block as usize];
                if owner != NONE && matches!(b.tags[owner as usize], Tag::Scope | Tag::Loop) {
                    owner
                } else {
                    block
                }
            }),
            Tag::NewStruct | Tag::NewTuple | Tag::NewVariant | Tag::Coerce => Some(user),
            Tag::Store | Tag::Call | Tag::CallValue | Tag::DefaultCall => {
                let at = ends.point(user);
                ends.roots[v] = Some(at);
                None
            }
            _ => None,
        };
        if let Some(next) = forwarded.filter(|&next| next != NONE) {
            // ruled out: SSA consumers and postorder control owners follow
            // the definitions they forward, including break targets.
            debug_assert!(next as usize > v);
            ends.roots[v] = ends.roots[next as usize];
        }
    }
    ends
}

/// Body-only lifetime and definition facts, shared across concrete instances
/// and sibling closures. The lazy `BorrowFacts` cell builds these once.
pub(super) struct LifetimeFacts {
    stored: Vec<(usize, usize)>,
    ends: Arc<ValueEnds>,
}

impl LifetimeFacts {
    pub fn new(b: &Body, topo: &hd_tir::ir::Topology, consumers: &[u32]) -> Self {
        crate::work::use_facts(b.len() + b.local_ty.len());
        let mut stored = vec![(usize::MAX, 0); b.local_ty.len()];
        for i in 0..b.len() {
            if let Some((local, true)) = b.stored_local(i) {
                let at = &mut stored[local.idx()];
                at.0 = at.0.min(i);
                at.1 = at.1.max(i);
            }
        }
        Self {
            stored,
            ends: value_ends(b, topo, consumers).into(),
        }
    }
}

/// Loans moved into an inlined body's bindings follow its returned value's
/// lifetime-ending use, as Swift OSSA's forwarding uses carry ownership.
/// Reuse the holder-origin DAG for match arms and aggregate payloads; only
/// bindings defined wholly inside the returned Scope are ended here.
pub(super) fn temporary_ends(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    (b, topo): (&Body, &hd_tir::ir::Topology),
    index: &OriginIndex,
    found: &[(usize, Held, Bind)],
    (facts, rows): (&LifetimeFacts, &[usize]),
) -> LoanEnds {
    if found.is_empty() {
        return LoanEnds::default();
    }
    let mut graph = OriginGraph::default();
    for &(_, held, _) in found {
        graph.publish(held);
    }
    let context = OriginContext {
        pool,
        env,
        body: b,
        index,
        call_argument: false,
    };
    let stored = &facts.stored;
    let handoffs: hd_base::HashSet<_> = found
        .iter()
        .filter_map(|&(at, _, bind)| match bind {
            Bind::Share(from) | Bind::Move(from) => Some((hd_tir::ir::u32_of(at), from)),
            _ => None,
        })
        .collect();
    let ends = EndValues::new(Arc::clone(&facts.ends));
    let mut ranges = ScopeHolderRanges::default();
    let mut declared: Vec<_> = found
        .iter()
        .map(|(_, held, _)| stored[held.local].0)
        .filter(|&at| at < b.len())
        .collect();
    declared.sort_stable();
    declared.dedup();
    let mut out: hd_base::HashMap<u32, Vec<Held>> = hd_base::HashMap::default();
    for &value in rows {
        if b.tags[value] != Tag::Scope {
            continue;
        }
        let first = topo.first[value] as usize;
        if declared
            .get(declared.partition_point(|&at| at < first))
            .is_none_or(|&at| at > value)
        {
            continue;
        }
        let answer = graph.answer(value, |v, graph| context.prepare(v, graph));
        if let Some(root) = graph.collapse(answer) {
            for held in ranges.collect(&graph.sets, root, stored, (first, value), &mut || {}) {
                for &end in ends.get(value, b, pool, env).iter() {
                    if b.tags[end as usize] != Tag::Store || handoffs.contains(&(end, held)) {
                        out.entry(end).or_default().push(held);
                    }
                }
            }
        }
    }
    for locals in out.values_mut() {
        locals.sort_stable();
        locals.dedup();
    }
    LoanEnds {
        holders: out,
        values: ends,
        handoffs,
    }
}

/// The same immutable-DAG bounds used by reference projection queries:
/// prune callers' holders before enumerating an inlined scope's own ones.
/// Each new set node gets its bounds once, without a per-query body walk.
#[derive(Default)]
struct ScopeHolderRanges {
    bounds: Vec<Option<(usize, usize)>>,
}

impl ScopeHolderRanges {
    fn collect(
        &mut self,
        sets: &OriginSets,
        root: usize,
        stored: &[(usize, usize)],
        (first, last): (usize, usize),
        visit: &mut impl FnMut(),
    ) -> Vec<Held> {
        while self.bounds.len() < sets.nodes.len() {
            visit();
            let bounds = match &sets.nodes[self.bounds.len()] {
                OriginSet::Atom(Bind::Share(held) | Bind::Move(held), _) => {
                    let (lo, hi) = stored[held.local];
                    (lo <= hi).then_some((lo, hi))
                }
                OriginSet::Atom(..) => None,
                OriginSet::Union(children) => match children.map(|c| self.bounds[c]) {
                    [None, other] | [other, None] => other,
                    [Some((a, b)), Some((c, d))] => Some((a.max(c), b.min(d))),
                },
            };
            self.bounds.push(bounds);
        }
        let mut seen = hd_base::HashSet::default();
        let mut pending = vec![root];
        let mut out = Vec::new();
        while let Some(node) = pending.pop() {
            visit();
            if !seen.insert(node)
                || self.bounds[node].is_none_or(|(lo, hi)| lo < first || hi > last)
            {
                continue;
            }
            match &sets.nodes[node] {
                OriginSet::Atom(Bind::Share(held) | Bind::Move(held), _) => out.push(*held),
                OriginSet::Union(children) => pending.extend(children.iter().rev().copied()),
                OriginSet::Atom(..) => {}
            }
        }
        out
    }
}

#[derive(Default)]
pub(super) struct LoanEnds {
    pub holders: hd_base::HashMap<u32, Vec<Held>>,
    pub values: EndValues,
    pub handoffs: hd_base::HashSet<(u32, Held)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct OriginRow {
    first: usize,
    node: usize,
}
type OriginAnswer = Option<OriginRow>;

#[derive(Default)]
struct OriginRows {
    nodes: Vec<OriginRowNode>,
    pairs: hd_base::HashMap<[OriginRow; 2], usize>,
    leaves: hd_base::HashMap<usize, usize>,
    bounds: Vec<(usize, usize)>,
    #[cfg(test)]
    work: std::cell::Cell<usize>,
}

enum OriginRowNode {
    Leaf(usize),
    Pair([OriginRow; 2]),
}

impl OriginRows {
    fn leaf(&mut self, slot: usize, set: usize) -> OriginRow {
        let node = *self.leaves.entry(set).or_insert_with(|| {
            let node = self.nodes.len();
            self.nodes.push(OriginRowNode::Leaf(set));
            self.bounds.push((0, 0));
            node
        });
        OriginRow { first: slot, node }
    }

    fn pair(&mut self, left: OriginAnswer, right: OriginAnswer) -> OriginAnswer {
        match (left, right) {
            (None, other) | (other, None) => other,
            (Some(left), Some(right)) if left == right => Some(left),
            (Some(left), Some(right)) => {
                let children = [left, right];
                let node = *self.pairs.entry(children).or_insert_with(|| {
                    let node = self.nodes.len();
                    let bounds = children.iter().fold((usize::MAX, 0), |(lo, hi), child| {
                        let (a, b) = self.bounds[child.node];
                        (lo.min(child.first + a), hi.max(child.first + b))
                    });
                    self.nodes.push(OriginRowNode::Pair(children));
                    self.bounds.push(bounds);
                    node
                });
                Some(OriginRow { first: 0, node })
            }
        }
    }

    fn build(&mut self, rows: impl IntoIterator<Item = (usize, usize)>) -> OriginAnswer {
        let mut level: Vec<_> = rows
            .into_iter()
            .map(|(slot, set)| Some(self.leaf(slot, set)))
            .collect();
        while level.len() > 1 {
            level = level
                .chunks(2)
                .map(|pair| self.pair(pair[0], pair.get(1).copied().flatten()))
                .collect();
        }
        level.pop().flatten()
    }

    fn collect(&self, answer: OriginAnswer) -> Vec<(usize, usize)> {
        self.collect_new(answer, &mut hd_base::HashSet::default())
    }

    fn collect_new(
        &self,
        answer: OriginAnswer,
        seen: &mut hd_base::HashSet<OriginRow>,
    ) -> Vec<(usize, usize)> {
        let mut pending: Vec<_> = answer.into_iter().collect();
        let mut rows = Vec::new();
        while let Some(edge) = pending.pop() {
            #[cfg(test)]
            {
                self.work.set(self.work.get() + 1);
            }
            if !seen.insert(edge) {
                continue;
            }
            match &self.nodes[edge.node] {
                OriginRowNode::Leaf(set) => rows.push((edge.first, *set)),
                OriginRowNode::Pair(children) => {
                    pending.extend(children.iter().rev().map(|child| OriginRow {
                        first: edge.first + child.first,
                        node: child.node,
                    }));
                }
            }
        }
        rows
    }
}

/// A balanced fold over immutable operand results. Replacing k operands costs
/// O(k log(width)); unchanged operand and row subtrees retain their arena IDs.
struct OriginJoin {
    tree: Vec<OriginAnswer>,
    base: usize,
}

impl OriginJoin {
    fn new(width: usize) -> Self {
        let base = width.max(1).next_power_of_two();
        Self {
            tree: vec![None; 2 * base],
            base,
        }
    }
}

/// Prepared handles carry their immutable plan and topological rank into
/// work items, so a queued item cannot be missing its plan or slot events.
#[derive(Clone)]
struct OriginHandle {
    value: usize,
    depth: usize,
    plan: Rc<OriginPlan>,
}

enum OriginTask {
    Enter(usize),
    Exit(usize, Rc<OriginPlan>),
}

#[derive(Clone, Copy)]
enum OriginTarget {
    Offset(usize),
    Slot(usize),
}

impl OriginTarget {
    fn slot(self, source: usize) -> usize {
        match self {
            Self::Offset(first) => first + source,
            Self::Slot(slot) => slot,
        }
    }
}

#[derive(Clone)]
struct OriginSubscription {
    parent: OriginHandle,
    position: usize,
    target: OriginTarget,
}

#[derive(Default)]
struct OriginParents {
    whole: Vec<OriginSubscription>,
    slots: hd_base::HashMap<usize, Vec<OriginSubscription>>,
}

struct OriginUpdate {
    handle: OriginHandle,
    slots: hd_base::HashSet<usize>,
}

fn queue_origin(
    pending: &mut std::collections::BTreeMap<(usize, usize), OriginUpdate>,
    handle: &OriginHandle,
    slot: usize,
) {
    pending
        .entry((handle.depth, handle.value))
        .or_insert_with(|| OriginUpdate {
            handle: handle.clone(),
            slots: hd_base::HashSet::default(),
        })
        .slots
        .insert(slot);
}

/// Flat arena: IDs share suffixes and dropping a deep graph is nonrecursive.
#[derive(Default)]
struct OriginSets {
    atoms: hd_base::HashMap<Bind, usize>,
    nodes: Vec<OriginSet>,
    unions: hd_base::HashMap<[usize; 2], usize>,
}

enum OriginSet {
    Atom(Bind, Option<Loan>),
    Union([usize; 2]),
}

impl OriginSets {
    fn atom(&mut self, bind: Bind, loan: Option<Loan>) -> usize {
        self.atom_with(bind, || loan)
    }

    fn atom_with(&mut self, bind: Bind, loan: impl FnOnce() -> Option<Loan>) -> usize {
        *self.atoms.entry(bind).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push(OriginSet::Atom(bind, loan()));
            id
        })
    }

    fn pair(&mut self, left: usize, right: usize) -> usize {
        if left == right {
            return left;
        }
        let children = [left, right];
        *self.unions.entry(children).or_insert_with(|| {
            let id = self.nodes.len();
            self.nodes.push(OriginSet::Union(children));
            id
        })
    }

    fn join(&mut self, left: Option<usize>, right: Option<usize>) -> Option<usize> {
        match (left, right) {
            (None, other) | (other, None) => other,
            (Some(left), Some(right)) => Some(self.pair(left, right)),
        }
    }
}

enum OriginMap {
    Offset(usize),
    Positions(Vec<usize>),
    Projection(hd_base::HashMap<usize, usize>),
}

#[derive(Clone, Copy)]
enum OriginMapRef<'a> {
    Offset(usize),
    Positions(&'a [usize]),
    Projection(&'a hd_base::HashMap<usize, usize>),
}

impl OriginMapRef<'_> {
    fn slot(self, slot: usize) -> Option<usize> {
        match self {
            Self::Offset(first) => Some(first + slot),
            Self::Positions(slots) => slots.get(slot).copied(),
            Self::Projection(slots) => slots.get(&slot).copied(),
        }
    }
}

struct OriginField {
    value: usize,
    first: usize,
}

#[derive(Clone, Copy)]
struct OriginPartRef<'a> {
    value: usize,
    map: OriginMapRef<'a>,
}

#[derive(Clone, Copy)]
enum OriginOperands<'a> {
    Mapped(&'a [OriginPart]),
    Fields(&'a [OriginField]),
}

impl<'a> OriginOperands<'a> {
    fn len(self) -> usize {
        match self {
            Self::Mapped(parts) => parts.len(),
            Self::Fields(fields) => fields.len(),
        }
    }

    fn part(self, position: usize) -> OriginPartRef<'a> {
        match self {
            Self::Mapped(parts) => {
                let part = &parts[position];
                let map = match &part.map {
                    OriginMap::Offset(first) => OriginMapRef::Offset(*first),
                    OriginMap::Positions(slots) => OriginMapRef::Positions(slots),
                    OriginMap::Projection(slots) => OriginMapRef::Projection(slots),
                };
                OriginPartRef {
                    value: part.value,
                    map,
                }
            }
            Self::Fields(fields) => {
                let field = &fields[position];
                OriginPartRef {
                    value: field.value,
                    map: OriginMapRef::Offset(field.first),
                }
            }
        }
    }
}

struct OriginPart {
    value: usize,
    map: OriginMap,
}

enum OriginPlan {
    Ready(OriginAnswer),
    Local {
        local: usize,
        copy: bool,
        projected: Vec<usize>,
    },
    Join(Vec<OriginPart>),
    Fields(Vec<OriginField>),
    Scope {
        values: Vec<OriginPart>,
        fallback: OriginAnswer,
        parents: OriginAnswer,
    },
    Call {
        values: Vec<OriginPart>,
        slot: usize,
    },
}

impl OriginPlan {
    fn operands(&self) -> OriginOperands<'_> {
        match self {
            Self::Join(values) | Self::Scope { values, .. } | Self::Call { values, .. } => {
                OriginOperands::Mapped(values)
            }
            Self::Fields(fields) => OriginOperands::Fields(fields),
            _ => OriginOperands::Mapped(&[]),
        }
    }

    fn children(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let operands = self.operands();
        (0..operands.len()).map(move |position| operands.part(position).value)
    }
}

/// Local holder propagation has no table of global or raw-pointer holders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OriginReadRoot {
    Local(usize),
    Value(u32),
}

fn origin_root_ty(b: &Body, root: PlaceRoot) -> Option<(Ty, OriginReadRoot)> {
    match root {
        PlaceRoot::Local(local) => {
            Some((b.local_ty[local.idx()], OriginReadRoot::Local(local.idx())))
        }
        PlaceRoot::Value(value) if value.as_inst().is_some() => {
            Some((b.ty[value.0 as usize], OriginReadRoot::Value(value.0)))
        }
        _ => None,
    }
}

/// One concrete instance/mode's graph. Plans are immutable; only answers whose
/// local-slot dependencies change are invalidated. All edges use arena IDs.
struct OriginContext<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    body: &'a Body,
    index: &'a OriginIndex,
    /// A call argument has no holder registration to follow its reborrow root.
    /// Retain the inlined argument's Ref origin until the consuming call.
    call_argument: bool,
}

/// Physical reference positions of one value-relative checked fragment.
/// A hole's concrete slots retain their relative ordinal across forwarding.
fn loan_fragment(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    fragment: &hd_tir::ir::LoanSlot,
) -> Option<Vec<usize>> {
    let root = *b.ty.get(fragment.value as usize)?;
    let mut ty = root;
    let mut variant = None;
    let mut path = Vec::with_capacity(fragment.path.len());
    for step in &fragment.path {
        ty = strip_newtype(pool, env, ty);
        match *step {
            hd_tir::ir::LoanField::Variant(index) => {
                variant = Some(index);
                path.push(Proj::Variant(index));
            }
            hd_tir::ir::LoanField::Field(index) => {
                ty = match variant.take() {
                    Some(case) => *variants(pool, env, ty)?
                        .get(case as usize)?
                        .get(index as usize)?,
                    None => *value_parts(pool, env, ty).get(index as usize)?,
                };
                path.push(Proj::Field(index));
            }
        }
    }
    let physical = projection_slots(pool, env, root, &path)?;
    let mut references = ref_slots(pool, env, ty);
    // An auto-ref argument may still use its whole counted pointer word.
    if references.is_empty()
        && !fragment.expand
        && matches!(kind(pool, env, ty), Kind::Ref { whole: true, .. })
    {
        references.push((0, ty));
    }
    references
        .into_iter()
        .map(|(slot, _)| physical.get(slot).copied())
        .collect()
}

impl OriginContext<'_> {
    fn fragment(
        &self,
        row: &hd_tir::ir::LoanSlot,
        graph: &mut OriginGraph,
    ) -> Vec<(usize, Option<usize>)> {
        let Some(&ty) = self.body.ty.get(row.value as usize) else {
            return Vec::new();
        };
        let key = (ty, row.path.clone(), row.expand);
        let slots = graph
            .fragments
            .entry(key)
            .or_insert_with(|| loan_fragment(self.pool, self.env, self.body, row).map(Rc::new));
        slots
            .iter()
            .flat_map(|s| s.iter())
            .enumerate()
            .map(|(ordinal, &slot)| (slot, row.expand.then_some(ordinal)))
            .collect()
    }

    fn call_parts(&self, call: usize, graph: &mut OriginGraph) -> Option<Vec<OriginPart>> {
        let rows = self.index.loan_slots.get(&call)?;
        let mut group_indices = hd_base::HashMap::default();
        let mut groups: Vec<OriginGroup> = Vec::new();
        for row in rows.iter().filter(|row| row.value as usize != call) {
            for (slot, lane) in self.fragment(row, graph) {
                let Some(source) = self.part(
                    row.value,
                    OriginMap::Projection([(slot, 0)].into_iter().collect()),
                ) else {
                    continue;
                };
                let direct = graph.sets.atom_with(Bind::Call(row.value), || {
                    arg_loan(self.pool, self.env, self.body, row.value)
                });
                let fallback = matches!(graph.sets.nodes[direct], OriginSet::Atom(_, Some(_)))
                    .then(|| graph.rows.leaf(0, direct));
                let source = graph.virtual_plan(
                    self.body.len(),
                    OriginPlan::Scope {
                        values: vec![source],
                        fallback,
                        parents: None,
                    },
                );
                let group = *group_indices.entry((row.origin, lane)).or_insert_with(|| {
                    let index = groups.len();
                    groups.push(((row.origin, lane), Vec::new()));
                    index
                });
                groups[group].1.push(OriginPart {
                    value: source,
                    map: OriginMap::Offset(0),
                });
            }
        }
        let mut roots = hd_base::HashMap::default();
        let mut origin_indices = hd_base::HashMap::default();
        let mut by_origin: Vec<(u32, Vec<OriginPart>)> = Vec::new();
        for (key, values) in groups {
            let value = graph.virtual_plan(self.body.len(), OriginPlan::Call { values, slot: 0 });
            roots.insert(key, value);
            let group = *origin_indices.entry(key.0).or_insert_with(|| {
                let index = by_origin.len();
                by_origin.push((key.0, Vec::new()));
                index
            });
            by_origin[group].1.push(OriginPart {
                value,
                map: OriginMap::Offset(0),
            });
        }
        let all: hd_base::HashMap<_, _> = by_origin
            .into_iter()
            .map(|(origin, values)| {
                (
                    origin,
                    graph.virtual_plan(self.body.len(), OriginPlan::Call { values, slot: 0 }),
                )
            })
            .collect();
        let mut out = Vec::new();
        for row in rows.iter().filter(|row| row.value as usize == call) {
            for (slot, lane) in self.fragment(row, graph) {
                let sources: Vec<_> = match lane {
                    None => all.get(&row.origin).copied().into_iter().collect(),
                    Some(_) => [
                        roots.get(&(row.origin, None)),
                        roots.get(&(row.origin, lane)),
                    ]
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect(),
                };
                out.extend(sources.into_iter().map(|value| OriginPart {
                    value,
                    map: OriginMap::Offset(slot),
                }));
            }
        }
        Some(out)
    }

    fn part(&self, value: u32, map: OriginMap) -> Option<OriginPart> {
        Some(OriginPart {
            value: self.index.value(value as usize)?,
            map,
        })
    }

    fn held(&self, c: usize, graph: &mut OriginGraph) -> OriginAnswer {
        let slots = ref_slots(self.pool, self.env, self.body.ty[c]);
        if slots.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        for &a in self.index.arguments(c) {
            let set = graph.sets.atom_with(Bind::Call(a), || {
                arg_loan(self.pool, self.env, self.body, a)
            });
            rows.extend(slots.iter().map(|&(slot, _)| (slot, set)));
        }
        graph.rows.build(rows)
    }

    fn read(&self, c: usize) -> Option<OriginPlan> {
        let b = self.body;
        let place = b.decode_place(b.data[c][0])?;
        let (root_ty, root) = origin_root_ty(b, place.root)?;
        let projected = projection_slots(self.pool, self.env, root_ty, &place.proj)?;
        Some(match root {
            OriginReadRoot::Local(local) => OriginPlan::Local {
                local,
                copy: b.data[c][1] == ReadMode::Copy as u32,
                projected,
            },
            OriginReadRoot::Value(value) => {
                let mut inverse = hd_base::HashMap::default();
                for (slot, source) in projected.into_iter().enumerate() {
                    inverse.entry(source).or_insert(slot);
                }
                OriginPlan::Join(
                    self.part(value, OriginMap::Projection(inverse))
                        .into_iter()
                        .collect(),
                )
            }
        })
    }

    fn prepare(&self, c: usize, graph: &mut OriginGraph) -> OriginPlan {
        self.plan(c, graph).unwrap_or(OriginPlan::Ready(None))
    }

    fn plan(&self, c: usize, graph: &mut OriginGraph) -> Option<OriginPlan> {
        let b = self.body;
        let direct = self.held(c, graph);
        if matches!(b.tags[c], Tag::Block | Tag::Scope) {
            let values = self
                .index
                .returns(b, c)
                .filter_map(|v| self.part(v, OriginMap::Offset(0)))
                .collect();
            let mut parents = None;
            // Holder registration already follows a returned Ref's reborrow
            // roots. A transient call argument instead needs the inlined
            // receiver's own Ref origin as well as the result's origin.
            for &a in self.index.arguments(c) {
                if direct.is_some()
                    && let Some(v) = self.index.value(a as usize)
                    && (self.call_argument && b.tags[v] == Tag::Ref
                        || b.tags[v] == Tag::Read
                            && matches!(
                                kind(self.pool, self.env, b.ty[v]),
                                Kind::Ref {
                                    whole: true,
                                    fat: false,
                                    ..
                                }
                            ))
                {
                    let set = graph
                        .sets
                        .atom_with(Bind::CallParent(a), || arg_loan(self.pool, self.env, b, a));
                    let added = graph.rows.build(
                        graph
                            .rows
                            .collect(direct)
                            .into_iter()
                            .map(|(slot, _)| (slot, set)),
                    );
                    parents = graph.rows.pair(parents, added);
                }
            }
            return Some(OriginPlan::Scope {
                values,
                fallback: direct,
                parents,
            });
        }
        if direct.is_some() {
            if matches!(
                b.tags[c],
                Tag::Call | Tag::CallValue | Tag::DefaultCall | Tag::CallDyn
            ) {
                // A borrowed result forwards the argument's existing loan,
                // as OSSA forwarding uses extend the introducing borrow.
                // The direct loan remains the fallback for an untracked
                // argument; returning a reference does not end its loan.
                if let Some(values) = self.call_parts(c, graph) {
                    return Some(OriginPlan::Scope {
                        values,
                        fallback: direct,
                        parents: None,
                    });
                }
                let slots = ref_slots(self.pool, self.env, b.ty[c]);
                let values = self
                    .index
                    .arguments(c)
                    .iter()
                    .flat_map(|&argument| {
                        slots.iter().filter_map(move |&(slot, _)| {
                            self.part(argument, OriginMap::Offset(slot))
                        })
                    })
                    .collect();
                return Some(OriginPlan::Scope {
                    values,
                    fallback: direct,
                    parents: None,
                });
            }
            return Some(OriginPlan::Ready(direct));
        }
        if matches!(b.tags[c], Tag::If | Tag::Match) {
            return Some(OriginPlan::Join(
                b.record(b.data[c][1])
                    .iter()
                    .skip(usize::from(b.tags[c] == Tag::Match))
                    .filter_map(|&v| self.part(v, OriginMap::Offset(0)))
                    .collect(),
            ));
        }
        if b.tags[c] == Tag::Call
            && let [(slot, _)] = ref_slots(self.pool, self.env, b.ty[c])[..]
        {
            let values = call_args(b, c)
                .into_iter()
                .filter_map(|a| {
                    let t = *b.ty.get(a as usize)?;
                    if ref_slots(self.pool, self.env, t).is_empty()
                        || matches!(
                            kind(self.pool, self.env, strip_newtype(self.pool, self.env, t)),
                            Kind::Ref { .. }
                        )
                    {
                        return None;
                    }
                    self.part(a, OriginMap::Offset(0))
                })
                .collect();
            return Some(OriginPlan::Call { values, slot });
        }
        match b.tags[c] {
            Tag::Ref => {
                let r = u32::try_from(c).expect("insts"); // ruled out: u32 TIR IDs.
                let loan = arg_loan(self.pool, self.env, b, r);
                Some(OriginPlan::Ready(graph.ready(vec![(
                    0,
                    Bind::Begin(r),
                    loan,
                )])))
            }
            Tag::Read => self.read(c),
            Tag::NewStruct | Tag::NewTuple => {
                let (fields, values) = if b.tags[c] == Tag::NewTuple {
                    let mut values = b.record(b.data[c][1]).to_vec();
                    if b.data[c][0] != hd_tir::ir::NONE {
                        values.push(b.data[c][0]);
                    }
                    (tuple_members(self.pool, b.ty[c]), values)
                } else {
                    (
                        value_parts(self.pool, self.env, b.ty[c]),
                        b.record(b.data[c][1]).to_vec(),
                    )
                };
                // Normal emission reports a failed layout before a module
                // can be returned; discovery does not manufacture loan facts.
                let (_, parts) = payload_mem(self.pool, self.env, &fields).ok()?;
                Some(OriginPlan::Fields(
                    values
                        .into_iter()
                        .zip(fields)
                        .zip(parts)
                        .filter_map(|((v, _), part)| {
                            self.index.value(v as usize).map(|value| OriginField {
                                value,
                                first: part.first,
                            })
                        })
                        .collect(),
                ))
            }
            Tag::NewVariant => {
                let ty = b.ty[c];
                let all = variants(self.pool, self.env, ty)?;
                let layout = flat(self.pool, self.env, ty).ok()??;
                let case = b.data[c][0] as usize;
                let fields = all.get(case)?;
                let at = layout.positions.get(case)?;
                Some(OriginPlan::Join(
                    b.record(b.data[c][1])
                        .iter()
                        .zip(fields)
                        .zip(at)
                        .filter_map(|((&v, _), slots)| {
                            self.part(
                                v,
                                OriginMap::Positions(slots.iter().map(|&s| s as usize).collect()),
                            )
                        })
                        .collect(),
                ))
            }
            _ => None,
        }
    }
}

/// A store keeps this cursor because discovery never retracts emitted facts.
/// Unchanged immutable row/set subtrees can be skipped across publications.
#[derive(Default)]
struct OriginCursor {
    rows: hd_base::HashSet<OriginRow>,
    sets: hd_base::HashSet<(usize, usize)>,
    emitted: hd_base::HashSet<(usize, Bind)>,
}

struct LocalOrigins {
    inverse: hd_base::HashMap<usize, usize>,
    answer: OriginAnswer,
}

type OriginGroup = ((u32, Option<usize>), Vec<OriginPart>);
type FragmentCache =
    hd_base::HashMap<(Ty, Vec<hd_tir::ir::LoanField>, bool), Option<Rc<Vec<usize>>>>;

#[derive(Default)]
pub(super) struct OriginGraph {
    virtual_plans: hd_base::HashMap<usize, Rc<OriginPlan>>,
    fragments: FragmentCache,
    plans: hd_base::HashMap<usize, Rc<OriginPlan>>,
    answers: hd_base::HashMap<usize, OriginAnswer>,
    parents: hd_base::HashMap<usize, OriginParents>,
    readers: hd_base::HashMap<Held, Vec<(OriginHandle, usize)>>,
    known: KnownOrigins,
    sets: OriginSets,
    rows: OriginRows,
    joins: hd_base::HashMap<usize, OriginJoin>,
    changed: hd_base::HashMap<usize, hd_base::HashSet<usize>>,
    collapsed: hd_base::HashMap<usize, usize>,
    mapped_cache: hd_base::HashMap<(usize, usize, OriginRow), OriginAnswer>,
    point_cache: hd_base::HashMap<(usize, usize), Option<usize>>,
    row_points: hd_base::HashMap<(OriginRow, usize), Option<usize>>,
    locals: hd_base::HashMap<usize, LocalOrigins>,
    local_changes: hd_base::HashMap<usize, Vec<usize>>,
    depths: hd_base::HashMap<usize, usize>,
    #[cfg(test)]
    work: usize,
}

impl OriginGraph {
    fn virtual_plan(&mut self, instructions: usize, plan: OriginPlan) -> usize {
        #[cfg(test)]
        {
            self.work += 1;
        }
        let value = instructions + self.virtual_plans.len();
        self.virtual_plans.insert(value, Rc::new(plan));
        value
    }
    fn ready(&mut self, rows: Vec<(usize, Bind, Option<Loan>)>) -> OriginAnswer {
        self.rows.build(
            rows.into_iter()
                .map(|(slot, bind, loan)| (slot, self.sets.atom(bind, loan))),
        )
    }

    fn row_point(&mut self, root: OriginAnswer, slot: usize) -> Option<usize> {
        let root = root?;
        let mut pending = vec![(root, false)];
        while let Some((edge, exit)) = pending.pop() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            let key = (edge, slot);
            if self.row_points.contains_key(&key) {
                continue;
            }
            let (lo, hi) = self.rows.bounds[edge.node];
            if slot < edge.first + lo || slot > edge.first + hi {
                self.row_points.insert(key, None);
                continue;
            }
            let children = match &self.rows.nodes[edge.node] {
                OriginRowNode::Leaf(set) => {
                    self.row_points.insert(key, Some(*set));
                    continue;
                }
                OriginRowNode::Pair(children) => children.map(|c| OriginRow {
                    first: edge.first + c.first,
                    node: c.node,
                }),
            };
            if exit {
                let result = self.sets.join(
                    self.row_points[&(children[0], slot)],
                    self.row_points[&(children[1], slot)],
                );
                self.row_points.insert(key, result);
            } else {
                pending.push((edge, true));
                pending.extend(children.into_iter().rev().map(|c| (c, false)));
            }
        }
        self.row_points[&(root, slot)]
    }

    fn point(&mut self, mut value: usize, mut slot: usize) -> Option<usize> {
        let mut path = Vec::new();
        let result = loop {
            #[cfg(test)]
            {
                self.work += 1;
            }
            if let Some(&result) = self.point_cache.get(&(value, slot)) {
                break result;
            }
            path.push((value, slot));
            match self.plans[&value].as_ref() {
                OriginPlan::Fields(fields) => {
                    let at = fields.partition_point(|field| field.first <= slot);
                    let Some(field) = at.checked_sub(1).and_then(|at| fields.get(at)) else {
                        break None;
                    };
                    slot -= field.first;
                    value = field.value;
                }
                OriginPlan::Local {
                    local,
                    copy,
                    projected,
                } => {
                    let Some(&source) = projected.get(slot) else {
                        break None;
                    };
                    if self.locals[&value].inverse.get(&source) != Some(&slot)
                        || !self.known.positions.contains_key(&Held {
                            local: *local,
                            slot: source,
                        })
                    {
                        break None;
                    }
                    let held = Held {
                        local: *local,
                        slot: source,
                    };
                    let bind = if *copy {
                        Bind::Share(held)
                    } else {
                        Bind::Move(held)
                    };
                    break Some(self.sets.atom(bind, None));
                }
                _ => break self.row_point(self.answers[&value], slot),
            }
        };
        for key in path {
            self.point_cache.insert(key, result);
        }
        result
    }

    fn mapped(&mut self, value: usize, position: usize, part: OriginPartRef<'_>) -> OriginAnswer {
        let answer = self.answers[&part.value];
        if let OriginMapRef::Offset(first) = part.map {
            return answer.map(|edge| OriginRow {
                first: edge.first + first,
                ..edge
            });
        }
        let single = match part.map {
            OriginMapRef::Projection(slots) if slots.len() == 1 => slots
                .iter()
                .next()
                .map(|(&source, &target)| (source, target)),
            OriginMapRef::Positions(slots) if slots.len() == 1 => Some((0, slots[0])),
            _ => None,
        };
        if let Some((source, target)) = single {
            return self
                .point(part.value, source)
                .map(|set| self.rows.leaf(target, set));
        }
        let root = answer?;
        let mut pending = vec![(root, false)];
        while let Some((edge, exit)) = pending.pop() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            let key = (value, position, edge);
            if self.mapped_cache.contains_key(&key) {
                continue;
            }
            let children = match &self.rows.nodes[edge.node] {
                OriginRowNode::Leaf(set) => {
                    let set = *set;
                    let mapped = part
                        .map
                        .slot(edge.first)
                        .map(|slot| self.rows.leaf(slot, set));
                    self.mapped_cache.insert(key, mapped);
                    continue;
                }
                OriginRowNode::Pair(children) => children.map(|child| OriginRow {
                    first: edge.first + child.first,
                    node: child.node,
                }),
            };
            if exit {
                let left = self.mapped_cache[&(value, position, children[0])];
                let right = self.mapped_cache[&(value, position, children[1])];
                let mapped = self.rows.pair(left, right);
                self.mapped_cache.insert(key, mapped);
            } else {
                pending.push((edge, true));
                pending.extend(children.into_iter().rev().map(|child| (child, false)));
            }
        }
        self.mapped_cache[&(value, position, root)]
    }

    fn joined(&mut self, value: usize, parts: OriginOperands<'_>) -> OriginAnswer {
        let existing = self.joins.remove(&value);
        let initial = existing.is_none();
        let mut join = existing.unwrap_or_else(|| OriginJoin::new(parts.len()));
        let changed = self.changed.remove(&value);
        if initial {
            for position in 0..parts.len() {
                let part = parts.part(position);
                #[cfg(test)]
                {
                    self.work += 1;
                }
                join.tree[join.base + position] = self.mapped(value, position, part);
            }
            for at in (1..join.base).rev() {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                join.tree[at] = self.rows.pair(join.tree[2 * at], join.tree[2 * at + 1]);
            }
        } else {
            let positions: Vec<_> = changed.into_iter().flatten().collect();
            for position in positions {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                let mut at = join.base + position;
                join.tree[at] = self.mapped(value, position, parts.part(position));
                while at > 1 {
                    #[cfg(test)]
                    {
                        self.work += 1;
                    }
                    at /= 2;
                    join.tree[at] = self.rows.pair(join.tree[2 * at], join.tree[2 * at + 1]);
                }
            }
        }
        let answer = join.tree[1];
        self.joins.insert(value, join);
        answer
    }

    fn local(
        &mut self,
        value: usize,
        local: usize,
        copy: bool,
        projected: &[usize],
    ) -> OriginAnswer {
        let (mut state, slots) = if let Some(state) = self.locals.remove(&value) {
            (state, self.local_changes.remove(&value).unwrap_or_default())
        } else {
            let mut inverse = hd_base::HashMap::default();
            for (slot, &source) in projected.iter().enumerate() {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                inverse.entry(source).or_insert(slot);
            }
            // A reader prepared after publication uses discovery order, not
            // projection order. It inspects only its own projected slots.
            let mut present: Vec<_> = inverse
                .keys()
                .filter_map(|&slot| {
                    self.known
                        .positions
                        .get(&Held { local, slot })
                        .map(|&rank| (rank, slot))
                })
                .collect();
            present.sort_stable_by_key(|&(rank, _)| rank);
            self.local_changes.remove(&value);
            (
                LocalOrigins {
                    inverse,
                    answer: None,
                },
                present.into_iter().map(|(_, slot)| slot).collect(),
            )
        };
        let mut rows = Vec::with_capacity(slots.len());
        for source in slots {
            #[cfg(test)]
            {
                self.work += 1;
            }
            let slot = state.inverse[&source];
            let held = Held {
                local,
                slot: source,
            };
            let bind = if copy {
                Bind::Share(held)
            } else {
                Bind::Move(held)
            };
            rows.push((slot, self.sets.atom(bind, None)));
        }
        let added = self.rows.build(rows);
        state.answer = self.rows.pair(state.answer, added);
        let answer = state.answer;
        self.locals.insert(value, state);
        answer
    }

    fn compute(&mut self, value: usize, plan: &OriginPlan) -> OriginAnswer {
        match plan {
            OriginPlan::Ready(result) => *result,
            OriginPlan::Local {
                local,
                copy,
                projected,
            } => self.local(value, *local, *copy, projected),
            OriginPlan::Join(parts) => self.joined(value, OriginOperands::Mapped(parts)),
            OriginPlan::Fields(fields) => self.joined(value, OriginOperands::Fields(fields)),
            OriginPlan::Scope {
                values,
                fallback,
                parents,
            } => {
                let answer = self.joined(value, OriginOperands::Mapped(values));
                if answer.is_none() {
                    return *fallback;
                }
                if parents.is_none() {
                    return answer;
                }
                // Parents follow structural origins, even when the same slot occurs twice.
                self.rows.pair(answer, *parents)
            }
            OriginPlan::Call { values, slot } => {
                let answer = self.joined(value, OriginOperands::Mapped(values));
                self.collapse(answer).map(|set| self.rows.leaf(*slot, set))
            }
        }
    }

    fn answer(
        &mut self,
        root: usize,
        mut prepare: impl FnMut(usize, &mut Self) -> OriginPlan,
    ) -> OriginAnswer {
        if let Some(&answer) = self.answers.get(&root) {
            return answer;
        }
        let mut pending = vec![OriginTask::Enter(root)];
        let mut active = hd_base::HashSet::default();
        while let Some(task) = pending.pop() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            match task {
                OriginTask::Enter(value) => {
                    if self.answers.contains_key(&value) {
                        continue;
                    }
                    assert!(
                        active.insert(value),
                        "ruled out: structural TIR value edges are acyclic; local reads are leaves"
                    );
                    let plan = if let Some(plan) = self.virtual_plans.get(&value) {
                        Rc::clone(plan)
                    } else {
                        Rc::new(prepare(value, self))
                    };
                    pending.push(OriginTask::Exit(value, Rc::clone(&plan)));
                    pending.extend(plan.children().rev().map(OriginTask::Enter));
                }
                OriginTask::Exit(value, plan) => {
                    let answer = self.compute(value, &plan);
                    let depth = plan
                        .children()
                        .map(|child| self.depths[&child] + 1)
                        .max()
                        .unwrap_or(0);
                    let handle = OriginHandle {
                        value,
                        depth,
                        plan: Rc::clone(&plan),
                    };
                    self.depths.insert(value, depth);
                    if let OriginPlan::Local {
                        local, projected, ..
                    } = plan.as_ref()
                    {
                        let mut seen = hd_base::HashSet::default();
                        for (slot, &source) in projected.iter().enumerate() {
                            if seen.insert(source) {
                                self.readers
                                    .entry(Held {
                                        local: *local,
                                        slot: source,
                                    })
                                    .or_default()
                                    .push((handle.clone(), slot));
                            }
                        }
                    }
                    let operands = plan.operands();
                    for position in 0..operands.len() {
                        let part = operands.part(position);
                        let parents = self.parents.entry(part.value).or_default();
                        let edge = |target| OriginSubscription {
                            parent: handle.clone(),
                            position,
                            target,
                        };
                        let collapse = if let OriginPlan::Call { slot, .. } = plan.as_ref() {
                            Some(*slot)
                        } else {
                            None
                        };
                        match part.map {
                            OriginMapRef::Offset(first) => parents.whole.push(edge(
                                collapse.map_or(OriginTarget::Offset(first), OriginTarget::Slot),
                            )),
                            OriginMapRef::Positions(slots) => {
                                for (source, &slot) in slots.iter().enumerate() {
                                    parents
                                        .slots
                                        .entry(source)
                                        .or_default()
                                        .push(edge(OriginTarget::Slot(collapse.unwrap_or(slot))));
                                }
                            }
                            OriginMapRef::Projection(slots) => {
                                for (&source, &slot) in slots {
                                    parents
                                        .slots
                                        .entry(source)
                                        .or_default()
                                        .push(edge(OriginTarget::Slot(collapse.unwrap_or(slot))));
                                }
                            }
                        }
                    }
                    self.plans.insert(value, plan);
                    self.answers.insert(value, answer);
                    active.remove(&value);
                }
            }
        }
        self.answers[&root]
    }

    fn collapse(&mut self, answer: OriginAnswer) -> Option<usize> {
        let root = answer?.node;
        let mut pending = vec![(root, false)];
        while let Some((node, exit)) = pending.pop() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            if self.collapsed.contains_key(&node) {
                continue;
            }
            match &self.rows.nodes[node] {
                OriginRowNode::Leaf(set) => {
                    self.collapsed.insert(node, *set);
                }
                OriginRowNode::Pair(children) if exit => {
                    let set = self.sets.pair(
                        self.collapsed[&children[0].node],
                        self.collapsed[&children[1].node],
                    );
                    self.collapsed.insert(node, set);
                }
                OriginRowNode::Pair(children) => {
                    pending.push((node, true));
                    pending.extend(children.iter().rev().map(|c| (c.node, false)));
                }
            }
        }
        Some(self.collapsed[&root])
    }

    fn collect(
        &mut self,
        value: usize,
        prepare: impl FnMut(usize, &mut Self) -> OriginPlan,
    ) -> Vec<(usize, Bind, Option<Loan>)> {
        self.collect_delta(value, prepare, &mut OriginCursor::default())
    }

    fn collect_delta(
        &mut self,
        value: usize,
        prepare: impl FnMut(usize, &mut Self) -> OriginPlan,
        cursor: &mut OriginCursor,
    ) -> Vec<(usize, Bind, Option<Loan>)> {
        let answer = self.answer(value, prepare);
        if answer.is_none_or(|root| cursor.rows.contains(&root)) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (slot, root) in self.rows.collect_new(answer, &mut cursor.rows) {
            let mut pending = vec![root];
            while let Some(set) = pending.pop() {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                if !cursor.sets.insert((slot, set)) {
                    continue;
                }
                match &self.sets.nodes[set] {
                    OriginSet::Atom(bind, loan) => {
                        if cursor.emitted.insert((slot, *bind)) {
                            out.push((slot, *bind, loan.clone()));
                        }
                    }
                    OriginSet::Union(children) => pending.extend(children.iter().rev().copied()),
                }
            }
        }
        out
    }

    #[cfg(test)]
    fn cost(&self) -> usize {
        self.work + self.rows.work.get()
    }

    fn publish(&mut self, held: Held) -> Vec<usize> {
        if self.known.positions.contains_key(&held) {
            return Vec::new();
        }
        self.known.publish(held);
        let mut pending = std::collections::BTreeMap::new();
        for (reader, slot) in self.readers.get(&held).into_iter().flatten() {
            self.local_changes
                .entry(reader.value)
                .or_default()
                .push(held.slot);
            queue_origin(&mut pending, reader, *slot);
        }
        let mut affected = Vec::new();
        let mut subscriptions = Vec::new();
        while let Some((_, OriginUpdate { handle, mut slots })) = pending.pop_first() {
            #[cfg(test)]
            {
                self.work += 1;
            }
            let value = handle.value;
            let old = self.answers[&value];
            let plan = handle.plan;
            let scope_was_empty = matches!(plan.as_ref(), OriginPlan::Scope { .. })
                .then(|| self.joins[&value].tree[1].is_none());
            let answer = self.compute(value, &plan);
            if let OriginPlan::Scope {
                fallback, parents, ..
            } = plan.as_ref()
                && scope_was_empty != Some(self.joins[&value].tree[1].is_none())
            {
                for (slot, _) in self
                    .rows
                    .collect(*fallback)
                    .into_iter()
                    .chain(self.rows.collect(*parents))
                {
                    slots.insert(slot);
                }
            }
            for &slot in &slots {
                self.point_cache.remove(&(value, slot));
            }
            self.answers.insert(value, answer);
            if old == answer {
                continue;
            }
            affected.push(value);
            subscriptions.clear();
            if let Some(parents) = self.parents.get(&value) {
                for &slot in &slots {
                    subscriptions.extend(parents.whole.iter().map(|edge| (edge.clone(), slot)));
                    subscriptions.extend(
                        parents
                            .slots
                            .get(&slot)
                            .into_iter()
                            .flatten()
                            .map(|edge| (edge.clone(), slot)),
                    );
                }
            }
            for (edge, source) in &subscriptions {
                #[cfg(test)]
                {
                    self.work += 1;
                }
                self.changed
                    .entry(edge.parent.value)
                    .or_default()
                    .insert(edge.position);
                queue_origin(&mut pending, &edge.parent, edge.target.slot(*source));
            }
        }
        affected
    }
}

struct OriginWalk<'a> {
    context: OriginContext<'a>,
    graph: OriginGraph,
}

impl<'a> OriginWalk<'a> {
    fn new(
        pool: &'a InternPool,
        env: &'a dyn ProgramEnv,
        body: &'a Body,
        index: &'a OriginIndex,
    ) -> Self {
        Self {
            context: OriginContext {
                pool,
                env,
                body,
                index,
                call_argument: false,
            },
            graph: OriginGraph::default(),
        }
    }

    fn collect_delta(
        &mut self,
        value: usize,
        cursor: &mut OriginCursor,
    ) -> Vec<(usize, Bind, Option<Loan>)> {
        self.context.index.query(value, |value| {
            self.graph
                .collect_delta(value, |c, graph| self.context.prepare(c, graph), cursor)
        })
    }

    fn publish(&mut self, held: Held) -> Vec<usize> {
        self.graph.publish(held)
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

/// Source-order rounds preserve the existing first-discovery semantics.
struct StoreWork {
    pending: std::collections::BTreeSet<(usize, usize)>,
    queued: Vec<bool>,
}

impl StoreWork {
    fn new(width: usize) -> Self {
        Self {
            pending: (0..width).map(|position| (0, position)).collect(),
            queued: vec![true; width],
        }
    }

    fn pop(&mut self) -> Option<(usize, usize)> {
        let item = self.pending.pop_first()?;
        self.queued[item.1] = false;
        Some(item)
    }

    fn schedule(&mut self, round: usize, position: usize, user: usize) {
        // Queue order only advances. An existing visit is always no later
        // than this publication's next source-order visit.
        if self.queued[user] {
            return;
        }
        self.queued[user] = true;
        self.pending
            .insert((round + usize::from(user <= position), user));
    }
}

#[derive(Default)]
struct OriginStores {
    occupied: hd_base::HashSet<(usize, Held)>,
    parents: hd_base::HashSet<(usize, Held, Bind)>,
}

impl OriginStores {
    fn delta<'a>(
        &'a mut self,
        instruction: usize,
        local: usize,
        slots: &'a hd_base::HashSet<usize>,
        rows: Vec<(usize, Bind, Option<Loan>)>,
    ) -> impl Iterator<Item = (Held, Bind, Option<Loan>)> + 'a {
        rows.into_iter().filter_map(move |(slot, bind, loan)| {
            if !slots.contains(&slot) {
                return None;
            }
            let held = Held { local, slot };
            let fresh = if matches!(bind, Bind::CallParent(_)) {
                self.parents.insert((instruction, held, bind))
            } else {
                !self.occupied.contains(&(instruction, held))
            };
            if !fresh {
                return None;
            }
            self.occupied.insert((instruction, held));
            Some((held, bind, loan))
        })
    }
}

struct OriginStore {
    instruction: usize,
    local: usize,
    value: usize,
    slots: Arc<hd_base::HashSet<usize>>,
}

/// Each `Store` of a sub-body into a local binding that registers one or more
/// loans, by flattened holder slot: a loan-taking `Ref` nested in a value,
/// a call's result that holds one ([`Body::held_args`]), or a copy or move
/// of another holder. The body already records concrete instance types.
pub(super) fn binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    index: &OriginIndex,
    stores: &[usize],
) -> Vec<(usize, Held, Bind, Option<Loan>)> {
    let mut out: Vec<(usize, Held, Bind, Option<Loan>)> = Vec::new();
    let mut graph = OriginWalk::new(pool, env, b, index);
    let mut registrations = OriginStores::default();
    let mut types = hd_base::HashMap::default();
    // Primitive/constant stores have no runtime holder facts and never enter
    // the ordered queue. Slot queries are shared by concrete type, not local.
    let sites: Vec<_> = stores
        .iter()
        .filter_map(|&instruction| {
            // ruled out: binding_stores records only whole-local stores.
            let local = b
                .stored_local(instruction)
                .and_then(|(local, whole)| whole.then_some(local))?
                .idx();
            let value = index.value(b.data[instruction][1] as usize)?;
            let slots = types.entry(b.local_ty[local]).or_insert_with(|| {
                Arc::new(
                    ref_slots(pool, env, b.local_ty[local])
                        .into_iter()
                        .map(|x| x.0)
                        .collect::<hd_base::HashSet<usize>>(),
                )
            });
            if slots.is_empty() {
                return None;
            }
            Some(OriginStore {
                instruction,
                local,
                value,
                slots: Arc::clone(slots),
            })
        })
        .collect();
    let mut users: hd_base::HashMap<usize, Vec<usize>> = hd_base::HashMap::default();
    for (position, site) in sites.iter().enumerate() {
        users.entry(site.value).or_default().push(position);
    }
    let mut work = StoreWork::new(sites.len());
    let mut cursors: Vec<_> = std::iter::repeat_with(OriginCursor::default)
        .take(sites.len())
        .collect();
    while let Some((round, position)) = work.pop() {
        let site = &sites[position];
        let rows = graph.collect_delta(site.value, &mut cursors[position]);
        for (held, bind, loan) in
            registrations.delta(site.instruction, site.local, &site.slots, rows)
        {
            for value in graph.publish(held) {
                for &user in users.get(&value).into_iter().flatten() {
                    work.schedule(round, position, user);
                }
            }
            out.push((site.instruction, held, bind, loan));
        }
    }
    out
}

/// Install the TIR link pass's pointer-loan tables for emission.
pub(crate) fn collect_loans(input: &Input<'_>, m: &mut Module) {
    let decisions = super::loan_facts::decide(input, m);
    m.kept = decisions.kept;
    m.kept_origins.take();
    m.lent = decisions.lent;
    m.result_inner = decisions.result_inner;
    m.dyn_types = decisions.dyn_types;
}

#[cfg(test)]
mod origin_index_tests {
    use super::{
        Bind, Held, KnownOrigins, OriginCursor, OriginGraph, OriginIndex, OriginMap, OriginPart,
        OriginPlan,
    };
    use super::{OriginField, OriginStores, StoreWork, origin_root_ty};
    use hd_base::{DefId, NodeIdx};
    use hd_tir::ir::{BodyKind, Coercion, NONE, Ref, Tag, TirBuilder, TirSink as _};
    use hd_types::Ty;

    #[test]
    fn temporary_scope_origins_are_filtered_at_width_and_depth() {
        for width in [1_000usize, 10_000] {
            for depth in [1usize, 20] {
                let mut sets = super::OriginSets::default();
                let mut level: Vec<_> = (0..width)
                    .map(|local| sets.atom(Bind::Share(Held { local, slot: 0 }), None))
                    .collect();
                while level.len() > 1 {
                    level = level
                        .chunks(2)
                        .map(|pair| {
                            pair.get(1)
                                .map_or(pair[0], |&other| sets.pair(pair[0], other))
                        })
                        .collect();
                }
                let stored: Vec<_> = (0..width + depth).map(|at| (at, at)).collect();
                let mut root = level[0];
                let mut ranges = super::ScopeHolderRanges::default();
                let mut visits = 0;
                for local in width..width + depth {
                    let held = Held { local, slot: 0 };
                    let leaf = sets.atom(Bind::Move(held), None);
                    root = sets.pair(root, leaf);
                    assert_eq!(
                        ranges.collect(&sets, root, &stored, (local, local), &mut || visits += 1),
                        [held]
                    );
                }
                assert_eq!(visits, 2 * width - 1 + 5 * depth);
                let before = visits;
                for _ in 0..100 {
                    assert_eq!(
                        ranges.collect(
                            &sets,
                            root,
                            &stored,
                            (width + depth - 1, width + depth - 1),
                            &mut || visits += 1
                        ),
                        [Held {
                            local: width + depth - 1,
                            slot: 0
                        }]
                    );
                }
                assert_eq!(visits - before, 300);
            }
        }
    }

    #[test]
    fn temporary_scope_origins_keep_caller_and_cross_scope_holders() {
        let mut sets = super::OriginSets::default();
        let outside = sets.atom(Bind::Share(Held { local: 0, slot: 0 }), None);
        let inside = sets.atom(Bind::Move(Held { local: 1, slot: 2 }), None);
        let unstored = sets.atom(Bind::Share(Held { local: 2, slot: 0 }), None);
        let call = sets.atom(Bind::Call(7), None);
        let a = sets.pair(outside, inside);
        let b = sets.pair(unstored, call);
        let root = sets.pair(a, b);
        let stored = [(0, 100), (4, 4), (usize::MAX, 0)];
        assert_eq!(
            super::ScopeHolderRanges::default().collect(&sets, root, &stored, (3, 5), &mut || {}),
            [Held { local: 1, slot: 2 }]
        );
    }

    fn unit(b: &mut TirBuilder) -> Ref {
        let fields = b.words_record(&[]);
        b.emit(Tag::NewTuple, NONE, fields, Ty::VOID, NodeIdx::NONE)
    }

    #[test]
    fn origin_graph_counts_width_depth_and_shared_empty_queries() {
        for (width, depth) in [(100, 20), (1000, 20), (1, 1000), (1, 10000)] {
            for positive in [false, true] {
                for shared in [false, true] {
                    let mut graph = OriginGraph::default();
                    let mut plans = Vec::new();
                    let mut previous = Vec::new();
                    for lane in 0..width {
                        let bind = Bind::Begin(if shared {
                            0
                        } else {
                            u32::try_from(lane).expect("test width")
                        });
                        let rows = if positive {
                            vec![(0, bind, None)]
                        } else {
                            vec![]
                        };
                        previous.push(plans.len());
                        plans.push(Some(OriginPlan::Ready(graph.ready(rows))));
                    }
                    for _ in 0..depth {
                        for value in &mut previous {
                            let parts = (0..if shared { 2 } else { 1 })
                                .map(|_| OriginPart {
                                    value: *value,
                                    map: OriginMap::Offset(0),
                                })
                                .collect();
                            *value = plans.len();
                            plans.push(Some(OriginPlan::Join(parts)));
                        }
                    }
                    for &root in &previous {
                        let out = graph.collect(root, |value, _| {
                            plans[value].take().expect("one preparation")
                        });
                        assert_eq!(out.len(), usize::from(positive));
                    }
                    assert!(plans.iter().all(Option::is_none));
                    assert!(
                        graph.work <= 8 * plans.len(),
                        "width={width} depth={depth} work={}",
                        graph.work
                    );
                    assert_eq!(
                        graph.sets.nodes.len(),
                        if positive {
                            if shared { 1 } else { width }
                        } else {
                            0
                        }
                    );
                    let before = graph.cost();
                    for &root in &previous {
                        graph.collect(root, |_, _| panic!("cached graph prepared twice"));
                    }
                    assert_eq!(graph.cost() - before, width * (2 * usize::from(positive)));
                }
            }
        }
    }

    #[test]
    fn shared_call_origin_groups_do_not_expand_source_result_products() {
        for depth in [1, 20] {
            let mut costs = Vec::new();
            for width in [1_000usize, 10_000] {
                let mut graph = OriginGraph::default();
                let mut sources = Vec::new();
                for lane in 0..width {
                    let bind = Bind::Begin(u32::try_from(lane).expect("test width"));
                    let answer = graph.ready(vec![(0, bind, None)]);
                    let value = graph.virtual_plan(0, OriginPlan::Ready(answer));
                    sources.push(OriginPart {
                        value,
                        map: OriginMap::Offset(0),
                    });
                }
                let mut group = graph.virtual_plan(
                    0,
                    OriginPlan::Call {
                        values: sources,
                        slot: 0,
                    },
                );
                for _ in 1..depth {
                    group = graph.virtual_plan(
                        0,
                        OriginPlan::Call {
                            values: vec![OriginPart {
                                value: group,
                                map: OriginMap::Offset(0),
                            }],
                            slot: 0,
                        },
                    );
                }
                let outputs = (0..width)
                    .map(|slot| OriginPart {
                        value: group,
                        map: OriginMap::Offset(slot),
                    })
                    .collect();
                let root = graph.virtual_plan(0, OriginPlan::Join(outputs));
                let answer = graph.answer(root, |_, _| panic!("all nodes are virtual"));
                assert!(answer.is_some());
                // Each result slot shares the same candidate set rather than
                // constructing width squared individual origin facts.
                assert!(graph.sets.nodes.len() <= 2 * width);
                let cost = graph.cost();
                assert_eq!(graph.answer(root, |_, _| panic!("cached")), answer);
                assert_eq!(graph.cost(), cost);
                costs.push(cost);
            }
            assert!(costs[1] <= 12 * costs[0], "depth={depth} costs={costs:?}");
        }
    }

    #[test]
    fn wide_join_publications_do_not_revisit_unchanged_operands() {
        for width in [100usize, 1000] {
            for depth in [1, 20] {
                for collapse in [false, true] {
                    let mut graph = OriginGraph::default();
                    let mut cursor = OriginCursor::default();
                    let mut plans: Vec<_> = (0..width)
                        .map(|local| {
                            Some(OriginPlan::Local {
                                local,
                                copy: true,
                                projected: vec![0],
                            })
                        })
                        .collect();
                    let mut roots: Vec<_> = (0..width).collect();
                    for _ in 1..depth {
                        for root in &mut roots {
                            let child = *root;
                            *root = plans.len();
                            plans.push(Some(OriginPlan::Join(vec![OriginPart {
                                value: child,
                                map: OriginMap::Offset(0),
                            }])));
                        }
                    }
                    let root = plans.len();
                    let parts = roots
                        .into_iter()
                        .enumerate()
                        .map(|(slot, value)| OriginPart {
                            value,
                            map: OriginMap::Offset(slot),
                        })
                        .collect();
                    plans.push(Some(if collapse {
                        OriginPlan::Call {
                            values: parts,
                            slot: 7,
                        }
                    } else {
                        OriginPlan::Join(parts)
                    }));
                    assert!(
                        graph
                            .collect_delta(root, |v, _| plans[v].take().unwrap(), &mut cursor)
                            .is_empty()
                    );
                    let before = graph.cost();
                    for local in 0..width {
                        let held = Held { local, slot: 0 };
                        graph.publish(held);
                        let out = graph.collect_delta(
                            root,
                            |_, _| panic!("prepared plans must survive publication"),
                            &mut cursor,
                        );
                        assert_eq!(out.len(), 1);
                        assert_eq!(out[0].0, if collapse { 7 } else { local });
                        assert!(matches!(out[0].1, Bind::Share(found) if found == held));
                    }
                    let work = graph.cost() - before;
                    eprintln!(
                        "wide join width={width} depth={depth} collapse={collapse}: total delta work={work}"
                    );
                    assert!(
                        work <= 40 * width * (depth + width.ilog2() as usize + 1),
                        "delta work={work}"
                    );
                    let before = graph.cost();
                    assert!(
                        graph
                            .collect_delta(root, |_, _| panic!("cached plan"), &mut cursor)
                            .is_empty()
                    );
                    assert!(graph.cost() - before <= 2);
                }
            }
        }
    }

    #[test]
    fn mapped_publications_reuse_unchanged_row_subtrees() {
        for width in [100usize, 1000] {
            for variant in [false, true] {
                let mut graph = OriginGraph::default();
                let mut cursor = OriginCursor::default();
                let mut plans: Vec<_> = (0..width)
                    .map(|local| {
                        Some(OriginPlan::Local {
                            local,
                            copy: true,
                            projected: vec![0],
                        })
                    })
                    .collect();
                let aggregate = plans.len();
                plans.push(Some(OriginPlan::Join(
                    (0..width)
                        .map(|value| OriginPart {
                            value,
                            map: OriginMap::Offset(value),
                        })
                        .collect(),
                )));
                let map = if variant {
                    OriginMap::Positions((0..width).rev().collect())
                } else {
                    OriginMap::Projection(
                        (0..width)
                            .filter(|slot| slot % 2 == 0)
                            .map(|slot| (slot, slot / 2))
                            .collect(),
                    )
                };
                let root = plans.len();
                plans.push(Some(OriginPlan::Join(vec![OriginPart {
                    value: aggregate,
                    map,
                }])));
                assert!(
                    graph
                        .collect_delta(root, |v, _| plans[v].take().unwrap(), &mut cursor)
                        .is_empty()
                );
                let before = graph.cost();
                for local in 0..width {
                    graph.publish(Held { local, slot: 0 });
                    let out =
                        graph.collect_delta(root, |_, _| panic!("cached mapped plan"), &mut cursor);
                    assert_eq!(out.len(), usize::from(variant || local % 2 == 0));
                    if let Some(row) = out.first() {
                        assert_eq!(
                            row.0,
                            if variant {
                                width - local - 1
                            } else {
                                local / 2
                            }
                        );
                    }
                }
                let work = graph.cost() - before;
                eprintln!("mapped width={width} variant={variant}: total delta work={work}");
                assert!(
                    work <= 40 * width * (width.ilog2() as usize + 1),
                    "mapped delta work={work}"
                );
            }
        }
    }

    #[test]
    fn publishing_one_field_does_not_recheck_other_field_projections() {
        for width in [100usize, 1000] {
            let mut graph = OriginGraph::default();
            let mut plans: Vec<_> = (0..width)
                .map(|local| {
                    Some(OriginPlan::Local {
                        local,
                        copy: true,
                        projected: vec![0],
                    })
                })
                .collect();
            let aggregate = plans.len();
            plans.push(Some(OriginPlan::Fields(
                (0..width)
                    .map(|value| OriginField {
                        value,
                        first: value,
                    })
                    .collect(),
            )));
            let first = plans.len();
            for slot in 0..width {
                plans.push(Some(OriginPlan::Join(vec![OriginPart {
                    value: aggregate,
                    map: OriginMap::Projection([(slot, 0)].into_iter().collect()),
                }])));
            }
            let mut cursors: Vec<_> = std::iter::repeat_with(OriginCursor::default)
                .take(width)
                .collect();
            for (slot, cursor) in cursors.iter_mut().enumerate() {
                assert!(
                    graph
                        .collect_delta(first + slot, |v, _| plans[v].take().unwrap(), cursor)
                        .is_empty()
                );
            }
            let before = graph.cost();
            let mut facts = 0;
            for local in 0..width {
                let affected = graph.publish(Held { local, slot: 0 });
                for value in affected.into_iter().filter(|&value| value >= first) {
                    let out = graph.collect_delta(
                        value,
                        |_, _| panic!("cached projection plan"),
                        &mut cursors[value - first],
                    );
                    facts += out.len();
                    assert_eq!(value, first + local);
                }
            }
            let work = graph.cost() - before;
            eprintln!("selective projection width={width}: total work={work}");
            assert_eq!(facts, width);
            assert!(
                work <= 40 * width * (width.ilog2() as usize + 1),
                "selective work={work}"
            );
        }
    }

    #[test]
    fn cold_projections_after_publication_do_not_scan_every_local_slot() {
        for width in [100usize, 1000] {
            let mut graph = OriginGraph::default();
            for rank in 0..width {
                graph.publish(Held {
                    local: 0,
                    slot: (rank * 37) % width,
                });
            }
            let mut plans = vec![
                Some(OriginPlan::Local {
                    local: 0,
                    copy: true,
                    projected: (0..width).collect(),
                }),
                Some(OriginPlan::Fields(vec![OriginField { value: 0, first: 0 }])),
            ];
            for slot in 0..width {
                plans.push(Some(OriginPlan::Join(vec![OriginPart {
                    value: 1,
                    map: OriginMap::Projection([(slot, 0)].into_iter().collect()),
                }])));
            }
            let before = graph.cost();
            for slot in 0..width {
                let out = graph.collect(slot + 2, |v, _| plans[v].take().unwrap());
                assert_eq!(out.len(), 1);
                assert!(
                    matches!(out[0].1, Bind::Share(Held { local: 0, slot: found }) if found == slot)
                );
            }
            let work = graph.cost() - before;
            eprintln!("late projections width={width}: work={work}");
            assert!(
                work <= 40 * width * (width.ilog2() as usize + 1),
                "late projection work={work}"
            );
        }
    }

    #[test]
    fn nested_scope_fallback_changes_propagate_added_and_removed_slots() {
        let mut graph = OriginGraph::default();
        let first_fallback = graph.ready(vec![(1, Bind::Call(17), None)]);
        let second_fallback = graph.ready(vec![(2, Bind::Call(29), None)]);
        let parents = graph.ready(vec![(2, Bind::CallParent(29), None)]);
        let mut plans = [
            Some(OriginPlan::Local {
                local: 0,
                copy: true,
                projected: vec![0],
            }),
            Some(OriginPlan::Scope {
                values: vec![OriginPart {
                    value: 0,
                    map: OriginMap::Offset(0),
                }],
                fallback: first_fallback,
                parents: None,
            }),
            Some(OriginPlan::Join(vec![OriginPart {
                value: 1,
                map: OriginMap::Projection([(1, 0)].into_iter().collect()),
            }])),
            Some(OriginPlan::Scope {
                values: vec![OriginPart {
                    value: 2,
                    map: OriginMap::Offset(0),
                }],
                fallback: second_fallback,
                parents,
            }),
            Some(OriginPlan::Join(vec![OriginPart {
                value: 3,
                map: OriginMap::Projection([(2, 7)].into_iter().collect()),
            }])),
        ];
        let mut cursor = OriginCursor::default();
        let initial = graph.collect_delta(4, |v, _| plans[v].take().unwrap(), &mut cursor);
        assert!(matches!(
            initial.as_slice(),
            [(7, Bind::CallParent(29), None)]
        ));
        let affected = graph.publish(Held { local: 0, slot: 0 });
        assert!(affected.contains(&4));
        let next = graph.collect_delta(4, |_, _| panic!("cached nested scopes"), &mut cursor);
        assert!(matches!(next.as_slice(), [(7, Bind::Call(29), None)]));
    }

    #[test]
    fn origin_seed_absence_and_read_root_classification_are_explicit() {
        use hd_base::LocalId;
        use hd_tir::ir::PlaceRoot;
        let mut builder = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        builder.body_mut().local_ty.push(Ty::VOID);
        let value = unit(&mut builder);
        let constant = builder.const_value(Ty::prim(hd_types::Prim::Usize), 0);
        let index = OriginIndex::new(builder.body_mut());
        assert!(index.canonical.is_empty() && index.held.is_empty() && index.breaks.is_empty());
        assert_eq!(
            origin_root_ty(builder.body_mut(), PlaceRoot::Local(LocalId::from_raw(0)))
                .map(|(ty, _)| ty),
            Some(Ty::VOID)
        );
        assert_eq!(
            origin_root_ty(builder.body_mut(), PlaceRoot::Value(value)).map(|(ty, _)| ty),
            Some(Ty::VOID)
        );
        assert_eq!(
            origin_root_ty(builder.body_mut(), PlaceRoot::Value(constant)).map(|(ty, _)| ty),
            None
        );
        assert_eq!(
            origin_root_ty(
                builder.body_mut(),
                PlaceRoot::Global(DefId::from_raw(0), Ty::VOID)
            ),
            None
        );
        assert_eq!(
            origin_root_ty(builder.body_mut(), PlaceRoot::Ptr(constant)).map(|(ty, _)| ty),
            None
        );
        builder.body_mut().held_args.push((value.0, value.0));
        let index = OriginIndex::new(builder.body_mut());
        assert_eq!(index.value(value.0 as usize), Some(value.0 as usize));
        builder.body_mut().held_args.clear();
        builder.emit(Tag::Ref, 0, 0, Ty::VOID, NodeIdx::NONE);
        assert!(!OriginIndex::new(builder.body_mut()).canonical.is_empty());
    }

    #[test]
    fn store_work_retains_source_rounds_and_coalesces_notifications() {
        let mut work = StoreWork::new(3);
        assert_eq!(work.pop(), Some((0, 0)));
        work.schedule(0, 0, 2);
        assert_eq!(work.pop(), Some((0, 1)));
        work.schedule(0, 1, 0);
        work.schedule(0, 1, 0);
        assert_eq!(work.pop(), Some((0, 2)));
        work.schedule(0, 2, 1);
        assert_eq!(work.pop(), Some((1, 0)));
        assert_eq!(work.pop(), Some((1, 1)));
        assert_eq!(work.pop(), None);
    }

    #[test]
    fn store_registration_keeps_first_origins_and_distinct_call_parents() {
        let mut registrations = OriginStores::default();
        let slots = [0].into_iter().collect();
        let rows = vec![
            (1, Bind::Begin(9), None),
            (0, Bind::Begin(1), None),
            (0, Bind::Begin(2), None),
            (0, Bind::CallParent(3), None),
            (0, Bind::CallParent(3), None),
            (0, Bind::CallParent(4), None),
        ];
        let found: Vec<_> = registrations
            .delta(7, 2, &slots, rows)
            .map(|(_, bind, _)| bind)
            .collect();
        assert!(matches!(
            found.as_slice(),
            [Bind::Begin(1), Bind::CallParent(3), Bind::CallParent(4)]
        ));
        let found: Vec<_> = registrations
            .delta(
                8,
                2,
                &slots,
                vec![(0, Bind::CallParent(3), None), (0, Bind::Begin(1), None)],
            )
            .map(|(_, bind, _)| bind)
            .collect();
        assert!(matches!(found.as_slice(), [Bind::CallParent(3)]));
        assert_eq!(
            registrations
                .delta(7, 2, &slots, vec![(0, Bind::Begin(1), None)])
                .count(),
            0
        );
    }

    #[test]
    fn atom_queries_cache_the_loan_and_point_queries_cache_empty_answers() {
        let mut graph = OriginGraph::default();
        let set = graph.sets.atom_with(Bind::Begin(7), || None);
        assert_eq!(
            graph
                .sets
                .atom_with(Bind::Begin(7), || panic!("cached loan recomputed")),
            set
        );
        let row = graph.rows.leaf(2, set);
        assert_eq!(graph.row_point(Some(row), 2), Some(set));
        assert_eq!(graph.row_point(Some(row), 2), Some(set));
        let mut plans = [
            Some(OriginPlan::Fields(vec![])),
            Some(OriginPlan::Local {
                local: 0,
                copy: true,
                projected: vec![],
            }),
            Some(OriginPlan::Ready(Some(row))),
        ];
        for value in 0..3 {
            graph.answer(value, |v, _| plans[v].take().unwrap());
        }
        assert_eq!(graph.point(0, 0), None);
        assert_eq!(graph.point(0, 0), None);
        assert_eq!(graph.point(1, 0), None);
        assert_eq!(graph.point(2, 2), Some(set));
        assert_eq!(graph.point(2, 2), Some(set));
        let mut builder = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let value = unit(&mut builder);
        let index = OriginIndex::prepare(builder.body_mut(), |_| {});
        assert_eq!(
            index.query(value.0 as usize, |v| vec![v]),
            [value.0 as usize]
        );
        assert!(
            index
                .query(usize::MAX, |_| -> Vec<usize> {
                    panic!("empty query evaluated")
                })
                .is_empty()
        );
    }

    #[test]
    fn an_equivalent_scope_fallback_does_not_republish_unchanged_origins() {
        let mut graph = OriginGraph::default();
        let fallback = graph.ready(vec![(1, Bind::Call(17), None)]);
        let outer_fallback = graph.ready(vec![(0, Bind::Call(17), None)]);
        let mut plans = [
            Some(OriginPlan::Local {
                local: 0,
                copy: true,
                projected: vec![0],
            }),
            Some(OriginPlan::Scope {
                values: vec![OriginPart {
                    value: 0,
                    map: OriginMap::Offset(0),
                }],
                fallback,
                parents: None,
            }),
            Some(OriginPlan::Join(vec![OriginPart {
                value: 1,
                map: OriginMap::Projection([(1, 0)].into_iter().collect()),
            }])),
            Some(OriginPlan::Scope {
                values: vec![OriginPart {
                    value: 2,
                    map: OriginMap::Offset(0),
                }],
                fallback: outer_fallback,
                parents: None,
            }),
        ];
        let mut cursor = OriginCursor::default();
        let initial = graph.collect_delta(3, |v, _| plans[v].take().unwrap(), &mut cursor);
        assert!(matches!(initial.as_slice(), [(0, Bind::Call(17), None)]));
        assert!(!graph.publish(Held { local: 0, slot: 0 }).contains(&3));
        assert!(
            graph
                .collect_delta(3, |_, _| panic!("cached scope"), &mut cursor)
                .is_empty()
        );
    }

    #[test]
    fn a_late_reader_uses_publication_order_and_only_projected_slots() {
        let mut graph = OriginGraph::default();
        for slot in [8, 3, 6, 1, 9, 0] {
            graph.publish(Held { local: 0, slot });
        }
        for slot in 0..1000 {
            graph.publish(Held { local: 1, slot });
        }
        let out = graph.collect(0, |_, _| OriginPlan::Local {
            local: 0,
            copy: false,
            projected: vec![0, 3, 1, 3],
        });
        assert_eq!(out.iter().map(|row| row.0).collect::<Vec<_>>(), [1, 2, 0]);
        assert!(out.iter().all(|row| matches!(row.1, Bind::Move(_))));
        assert!(graph.cost() < 30);
    }

    #[test]
    fn scope_fallback_deltas_retain_parent_order_without_repeating_old_origins() {
        let mut graph = OriginGraph::default();
        let mut cursor = OriginCursor::default();
        let mut plans = vec![Some(OriginPlan::Local {
            local: 0,
            copy: true,
            projected: vec![0, 1],
        })];
        let fallback = graph.ready(vec![(0, Bind::Call(17), None)]);
        let parents = graph.ready(vec![(0, Bind::CallParent(17), None)]);
        plans.push(Some(OriginPlan::Scope {
            values: vec![OriginPart {
                value: 0,
                map: OriginMap::Offset(0),
            }],
            fallback,
            parents,
        }));
        let first = graph.collect_delta(1, |v, _| plans[v].take().unwrap(), &mut cursor);
        assert!(matches!(first.as_slice(), [(0, Bind::Call(17), None)]));
        graph.publish(Held { local: 0, slot: 0 });
        let next = graph.collect_delta(1, |_, _| panic!("cached scope plan"), &mut cursor);
        assert!(matches!(
            next.as_slice(),
            [(0, Bind::Share(_), None), (0, Bind::CallParent(17), None)]
        ));
        graph.publish(Held { local: 0, slot: 1 });
        let last = graph.collect_delta(1, |_, _| panic!("cached scope plan"), &mut cursor);
        assert!(matches!(last.as_slice(), [(1, Bind::Share(_), None)]));
    }

    #[test]
    fn many_publications_to_one_local_process_only_the_new_slot() {
        for width in [100usize, 1000] {
            for copy in [false, true] {
                let mut graph = OriginGraph::default();
                let mut cursor = OriginCursor::default();
                assert!(
                    graph
                        .collect_delta(
                            0,
                            |_, _| OriginPlan::Local {
                                local: 0,
                                copy,
                                projected: (0..width).collect(),
                            },
                            &mut cursor
                        )
                        .is_empty()
                );
                let before = graph.cost();
                for slot in (0..width).rev() {
                    let held = Held { local: 0, slot };
                    graph.publish(held);
                    let out =
                        graph.collect_delta(0, |_, _| panic!("cached local plan"), &mut cursor);
                    assert_eq!(out.len(), 1);
                    assert_eq!(out[0].0, slot);
                    assert!(
                        matches!(out[0].1, Bind::Share(found) if copy && found == held)
                            || matches!(out[0].1, Bind::Move(found) if !copy && found == held)
                    );
                }
                let work = graph.cost() - before;
                eprintln!("local width={width} copy={copy}: total delta work={work}");
                assert!(work <= 10 * width, "local delta work={work}");
                assert_eq!(
                    graph
                        .collect(0, |_, _| panic!("cached local plan"))
                        .iter()
                        .map(|x| x.0)
                        .collect::<Vec<_>>(),
                    (0..width).rev().collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn publishing_a_slot_invalidates_only_its_readers_and_cached_ancestors() {
        for width in [100, 1000] {
            let mut graph = OriginGraph::default();
            let mut plans: Vec<_> = (0..width)
                .map(|local| {
                    Some(OriginPlan::Local {
                        local,
                        copy: true,
                        projected: vec![0],
                    })
                })
                .collect();
            let root = plans.len();
            plans.push(Some(OriginPlan::Join(
                (0..width)
                    .map(|value| OriginPart {
                        value,
                        map: OriginMap::Offset(value),
                    })
                    .collect(),
            )));
            assert!(
                graph
                    .collect(root, |v, _| plans[v].take().unwrap())
                    .is_empty()
            );
            let before = graph.work;
            assert!(
                graph
                    .publish(Held {
                        local: width + 1,
                        slot: 0
                    })
                    .is_empty()
            );
            assert_eq!(graph.work, before);
            let held = Held {
                local: width / 2,
                slot: 0,
            };
            let affected = graph.publish(held);
            assert_eq!(affected, [width / 2, root]);
            assert!(graph.work - before <= 10 + width.ilog2() as usize);
            assert!(graph.publish(held).is_empty());
            let answer = graph.collect(root, |_, _| panic!("immutable plans must remain cached"));
            assert_eq!(answer.len(), 1);
            assert_eq!(answer[0].0, width / 2);
            assert!(matches!(answer[0].1, Bind::Share(found) if found == held));
            assert_eq!(graph.answers.len(), width + 1);
        }
    }

    #[test]
    fn known_origins_preserve_first_discovery_and_ignore_republication() {
        let mut known = KnownOrigins::default();
        assert!(known.slots(0).is_empty());
        for slot in [3, 1, 3, 0, 1] {
            known.publish(Held { local: 0, slot });
        }
        known.publish(Held { local: 1, slot: 3 });
        assert_eq!(known.slots(0), [3, 1, 0]);
        assert_eq!(known.slots(1), [3]);
        assert!(known.slots(2).is_empty());
        assert_eq!(known.positions.len(), 4);
    }

    #[test]
    fn known_origin_reads_visit_only_the_selected_locals_slots() {
        for width in [100, 1000] {
            let mut known = KnownOrigins::default();
            for local in 0..width {
                for slot in 0..20 {
                    known.publish(Held { local, slot });
                    known.publish(Held { local, slot });
                }
            }
            let mut visits = 0;
            for _ in 0..100 {
                for (expected, &slot) in known.slots(width / 2).iter().enumerate() {
                    visits += 1;
                    assert_eq!(slot, expected);
                }
            }
            assert_eq!(visits, 2000);
            assert_eq!(known.positions.len(), width * 20);
            assert_eq!(known.locals.len(), width);
        }
    }

    #[test]
    fn origin_index_preparation_and_alias_queries_have_linear_work() {
        for (width, depth) in [(100, 20), (1000, 20), (1, 1000), (1, 10000)] {
            let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
            let root = unit(&mut b);
            let mut previous = vec![root; width];
            for _ in 0..depth {
                for value in &mut previous {
                    *value = b.coerce(Coercion::Identity, 0, *value, Ty::VOID, NodeIdx::NONE);
                }
            }
            let mut work = 0;
            let index = OriginIndex::prepare(b.body_mut(), |n| work += n);
            assert!(work <= 5 * b.body_mut().len());
            let prepared = work;
            for _ in 0..100 {
                for &value in &previous {
                    assert_eq!(index.value(value.0 as usize), Some(root.0 as usize));
                }
            }
            assert_eq!(work, prepared);
            assert_eq!(index.value(usize::MAX), None);
        }
    }

    #[test]
    fn origin_scope_exits_keep_tail_and_interleaved_break_order() {
        let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
        let values: Vec<_> = (0..4).map(|_| unit(&mut b)).collect();
        let empty = b.words_record(&[]);
        let inner = b.emit(Tag::Block, empty, values[1].0, Ty::VOID, NodeIdx::NONE);
        let scope = b.emit(Tag::Scope, inner.0, empty, Ty::VOID, NodeIdx::NONE);
        b.body_mut().label_inst = vec![scope.0, inner.0];
        b.emit(Tag::Break, 0, values[3].0, Ty::VOID, NodeIdx::NONE);
        b.emit(Tag::Break, 1, values[0].0, Ty::VOID, NodeIdx::NONE);
        b.emit(Tag::Break, 0, values[2].0, Ty::VOID, NodeIdx::NONE);
        b.emit(Tag::Break, 0, NONE, Ty::VOID, NodeIdx::NONE);
        b.body_mut().held_args = vec![(scope.0, 13), (scope.0, 14)];
        let index = OriginIndex::new(b.body_mut());
        assert_eq!(index.arguments(scope.0 as usize), &[13, 14]);
        assert_eq!(index.arguments(usize::MAX), &[]);
        assert_eq!(
            index
                .returns(b.body_mut(), scope.0 as usize)
                .collect::<Vec<_>>(),
            [values[1].0, values[3].0, values[0].0, values[2].0]
        );
        assert_eq!(
            index
                .returns(b.body_mut(), inner.0 as usize)
                .collect::<Vec<_>>(),
            [values[1].0, values[0].0]
        );
    }

    #[test]
    fn empty_alias_rings_finish_without_a_recursive_walk() {
        for depth in [1000, 10000] {
            let mut b = TirBuilder::new(DefId::from_raw(0), BodyKind::Fn);
            let first = b.coerce(Coercion::Identity, 0, Ref(NONE), Ty::VOID, NodeIdx::NONE);
            let mut previous = first;
            for _ in 1..depth {
                previous = b.coerce(Coercion::Identity, 0, previous, Ty::VOID, NodeIdx::NONE);
            }
            b.body_mut().data[first.0 as usize][0] = previous.0;
            let mut work = 0;
            let index = OriginIndex::prepare(b.body_mut(), |n| work += n);
            assert!(work <= 5 * b.body_mut().len() + 1);
            assert_eq!(index.value(first.0 as usize), None);
            assert_eq!(index.value(previous.0 as usize), None);
        }
    }
    #[test]
    fn nonempty_set_pairs_and_forward_reverse_offsets_preserve_their_contracts() {
        let mut graph = OriginGraph::default();
        let left = graph.sets.atom(Bind::Begin(5), None);
        let right = graph.sets.atom(Bind::Begin(9), None);
        assert_eq!(graph.sets.pair(left, left), left);
        let joined = graph.sets.join(Some(left), Some(right));
        let row = joined.map(|set| graph.rows.leaf(3, set));
        let out = graph.collect(0, |_, _| OriginPlan::Ready(row));
        assert!(matches!(
            out.as_slice(),
            [(3, Bind::Begin(5), None), (3, Bind::Begin(9), None)]
        ));
        let fields = [OriginField {
            value: 11,
            first: 7,
        }];
        let forward = super::OriginOperands::Fields(&fields).part(0);
        let reverse = super::OriginTarget::Offset(7);
        assert_eq!(forward.value, 11);
        for slot in 0..1024 {
            assert_eq!(forward.map.slot(slot), Some(reverse.slot(slot)));
        }
    }
}

#[cfg(test)]
mod value_end_tests {
    use super::*;
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_tir::ir::{BodyKind, Providers, TirBuilder, TirSink, local_flags};
    use hd_types::TyList;

    // Endpoint propagation is type-independent. Integer stand-ins keep this
    // generated test about the dependency graph, without runtime layouts.
    fn body(n: usize, branching: bool) -> Result<(Body, Vec<u32>), String> {
        let mut build = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = build.open_block();
        let source = build.local(Ty::I32, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
        let first = build.get(source, Ty::I32, NodeIdx::NONE);
        let mut value = first;
        let mut held = Vec::new();
        let mut consumers = Vec::new();
        let callee = Callee::Item {
            def: DefId::from_raw(2),
            targs: TyList::EMPTY,
        };
        for _ in 0..n {
            let argument = if branching { first } else { value };
            let call = build.call(
                &callee,
                &[argument],
                Providers::None,
                Ty::I32,
                NodeIdx::NONE,
            );
            held.push((call.0, argument.0));
            if !branching {
                let word = build.place(&Place {
                    root: PlaceRoot::Value(argument),
                    proj: Vec::new(),
                });
                build.emit(Tag::Drop, word, 0, Ty::VOID, NodeIdx::NONE);
            }
            value = call;
            if branching {
                let local = build.local(Ty::I32, Symbol::NONE, 0, NodeIdx::NONE);
                build.set(local, value, NodeIdx::NONE);
                consumers.push((value.0, hd_tir::ir::u32_of(build.body().len() - 1)));
            }
        }
        if !branching {
            let local = build.local(Ty::I32, Symbol::NONE, 0, NodeIdx::NONE);
            build.set(local, value, NodeIdx::NONE);
            consumers.push((value.0, hd_tir::ir::u32_of(build.body().len() - 1)));
        }
        let block = build.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut body = build
            .finish_tolerating(block, &[], &[22])
            .map_err(|errors| format!("{errors:?}"))?;
        body.held_args = held;
        let mut users = vec![hd_tir::ir::NONE; body.len()];
        for (value, user) in consumers {
            users[value as usize] = user;
        }
        Ok((body, users))
    }

    #[test]
    fn value_copy_results_end_loans_without_changing_other_instances() -> Result<(), String> {
        let (body, consumers) = body(3, false)?;
        let topo = hd_tir::ir::Topology::new(&body);
        let graph = Arc::new(value_ends(&body, &topo, &consumers));
        let last = body.held_args.last().ok_or("missing generated call")?.0;
        let borrowed = EndValues::new(Arc::clone(&graph));
        let copied = EndValues::new(graph);
        assert_eq!(
            borrowed.get_counted(0, |_| true, || {}).as_ref(),
            &[consumers[last as usize]]
        );
        assert_eq!(
            copied.get_counted(0, |at| at != last, || {}).as_ref(),
            &[last]
        );
        assert_eq!(
            borrowed.get_counted(0, |_| true, || {}).as_ref(),
            &[consumers[last as usize]]
        );
        Ok(())
    }

    #[test]
    fn dependency_endpoints_scale_with_call_chain_depth_and_branch_width() -> Result<(), String> {
        for branching in [false, true] {
            let mut small = 0;
            for n in [1_000usize, 10_000] {
                let (body, consumers) = body(n, branching)?;
                let topo = hd_tir::ir::Topology::new(&body);
                let mut work = 0;
                let ends = EndValues::new(Arc::new(value_ends_counted(
                    &body,
                    &topo,
                    &consumers,
                    || work += 1,
                )));
                let first = ends.get_counted(0, |_| true, || work += 1);
                assert_eq!(first.len(), if branching { n } else { 1 });
                for &(call, argument) in &body.held_args {
                    assert!(
                        !ends
                            .get_counted(call as usize, |_| true, || work += 1)
                            .is_empty()
                    );
                    assert!(
                        !ends
                            .get_counted(argument as usize, |_| true, || work += 1)
                            .is_empty()
                    );
                }
                assert!(ends.graph.nodes.len() <= 3 * body.len());
                eprintln!("borrowed call endpoints: branching={branching} n={n} work={work}");
                if n == 1_000 {
                    small = work;
                } else {
                    assert!(work <= 11 * small, "small={small}, large={work}");
                }
            }
        }
        Ok(())
    }
}
