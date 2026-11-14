//! Holder bindings for pointer loans (model A of
//! `docs/compiler/ownership.md#pointer-loans-model-a`, `borrow.rs`): which stores bind a
//! reference into a holder (a binding, or a struct, tuple or variant that
//! holds it), from a `Ref`, a call's result that depends on an argument
//! the checker found it holds ([`Body::held_args`]), or another holder by
//! a copy or a move; and the calls' arguments those loans read.

use hd_base::SliceSort as _;
use hd_mono::{ProgramEnv, Target, subst};
use hd_tir::ir::{Body, Callee, Place, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};
use hd_types::{InternPool, Ty, TyData};

use crate::Input;
use crate::layout::{
    Kind, flat, kind, mem as mem_of, payload_mem, ref_mutable, strip, tuple_members, value_parts,
    variants,
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
    held: Vec<Option<u32>>,
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
        let mut held = vec![None; b.len()];
        for &(value, argument) in &b.held_args {
            work(1);
            if let Some(slot) = held.get_mut(value as usize) {
                slot.get_or_insert(argument);
            }
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

    fn argument(&self, value: usize) -> Option<u32> {
        self.held.get(value).copied().flatten()
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
    use super::ref_slots::{Shape, for_type};
    for_type(strip(pool, env, t), |t| {
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
                |slots| Shape::Mapped(strip(pool, env, inner), slots),
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
                .map(|(field, part)| (part.first, strip(pool, env, field)))
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
    index: &OriginIndex,
    graph: &mut OriginGraph,
    sub: &dyn Fn(Ty) -> Ty,
    v: usize,
) -> Vec<u32> {
    if b.tags.get(v) == Some(&Tag::Ref) {
        return Vec::new();
    }
    index.query(v, |value| {
        let context = OriginContext {
            pool,
            env,
            body: b,
            index,
            sub,
        };
        let mut out = Vec::new();
        let mut seen = hd_base::HashSet::default();
        for (_, bind, _) in graph.collect(value, |c, graph| context.prepare(c, graph)) {
            if let Bind::Call(a) = bind
                && let Some(r) = index.value(a as usize).filter(|&r| b.tags[r] == Tag::Ref)
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
    sub: &'a dyn Fn(Ty) -> Ty,
}

impl OriginContext<'_> {
    fn part(&self, value: u32, map: OriginMap) -> Option<OriginPart> {
        Some(OriginPart {
            value: self.index.value(value as usize)?,
            map,
        })
    }

    fn held(&self, c: usize, graph: &mut OriginGraph) -> OriginAnswer {
        let Some(a) = self.index.argument(c) else {
            return OriginGraph::empty();
        };
        let slots = ref_slots(self.pool, self.env, (self.sub)(self.body.ty[c]));
        if slots.is_empty() {
            return None;
        }
        let set = graph.sets.atom_with(Bind::Call(a), || {
            arg_loan(self.pool, self.env, self.body, self.sub, a)
        });
        graph
            .rows
            .build(slots.into_iter().map(|(slot, _)| (slot, set)))
    }

    fn read(&self, c: usize) -> Option<OriginPlan> {
        let b = self.body;
        let place = b.decode_place(b.data[c][0])?;
        let (root_ty, root) = origin_root_ty(b, place.root)?;
        let root_ty = (self.sub)(root_ty);
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
            if direct.is_some()
                && let Some(a) = self.index.argument(c)
                && let Some(v) = self.index.value(a as usize)
                && b.tags[v] == Tag::Read
                && matches!(
                    kind(self.pool, self.env, (self.sub)(b.ty[v])),
                    Kind::Ref {
                        whole: true,
                        fat: false,
                        ..
                    }
                )
            {
                let set = graph.sets.atom_with(Bind::CallParent(a), || {
                    arg_loan(self.pool, self.env, b, self.sub, a)
                });
                parents = graph.rows.build(
                    graph
                        .rows
                        .collect(direct)
                        .into_iter()
                        .map(|(slot, _)| (slot, set)),
                );
            }
            return Some(OriginPlan::Scope {
                values,
                fallback: direct,
                parents,
            });
        }
        if direct.is_some() {
            return Some(OriginPlan::Ready(direct));
        }
        if b.tags[c] == Tag::If {
            return Some(OriginPlan::Join(
                b.record(b.data[c][1])
                    .iter()
                    .filter_map(|&v| self.part(v, OriginMap::Offset(0)))
                    .collect(),
            ));
        }
        if b.tags[c] == Tag::Call
            && let [(slot, _)] = ref_slots(self.pool, self.env, (self.sub)(b.ty[c]))[..]
        {
            let values = call_args(b, c)
                .into_iter()
                .filter_map(|a| {
                    let t = (self.sub)(*b.ty.get(a as usize)?);
                    if ref_slots(self.pool, self.env, t).is_empty()
                        || matches!(
                            kind(self.pool, self.env, strip(self.pool, self.env, t)),
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
                let loan = arg_loan(self.pool, self.env, b, self.sub, r);
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
                    (tuple_members(self.pool, (self.sub)(b.ty[c])), values)
                } else {
                    (
                        value_parts(self.pool, self.env, (self.sub)(b.ty[c])),
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
                let ty = (self.sub)(b.ty[c]);
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

#[derive(Default)]
pub(super) struct OriginGraph {
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
    fn empty() -> OriginAnswer {
        None
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
                    let plan = Rc::new(prepare(value, self));
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
        sub: &'a dyn Fn(Ty) -> Ty,
    ) -> Self {
        Self {
            context: OriginContext {
                pool,
                env,
                body,
                index,
                sub,
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
/// of another holder. `sub` gives a local's or instruction's type at this
/// instance.
pub(super) fn binds(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    b: &Body,
    index: &OriginIndex,
    sub: &dyn Fn(Ty) -> Ty,
    stores: &[usize],
) -> Vec<(usize, Held, Bind, Option<Loan>)> {
    let mut out: Vec<(usize, Held, Bind, Option<Loan>)> = Vec::new();
    let mut graph = OriginWalk::new(pool, env, b, index, sub);
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
            let slots = types.entry(sub(b.local_ty[local])).or_insert_with(|| {
                Arc::new(
                    ref_slots(pool, env, sub(b.local_ty[local]))
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
        assert_eq!(index.argument(scope.0 as usize), Some(13));
        assert_eq!(index.argument(usize::MAX), None);
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
