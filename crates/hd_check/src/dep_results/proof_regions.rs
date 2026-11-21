//! Maps deferred outlives proofs to their root input positions. Type
//! equality is insufficient here: two positions may have the same type
//! and different lifetimes. Impl patterns supply the correspondence.

mod member_inputs;
mod slot_maps;

use hd_base::{DefId, HashMap, HashSet, NotImplemented, Stage, StageResult};
use hd_resolve::ItemData;
use hd_types::solver::{
    ImplOrigin, ImplRef, Impls, MEMBER_PARAM, PlanStep, RegionBound, RegionObligation,
    RegionObligations, RegionProofId, TraitRef,
};
use hd_types::{ParamRef, Ty, TyData};

use super::{Key, Layouts, SlotCounts, Var, arg_ids, whole_referent};
use crate::body::{Ck, ProofSource, ProofUse};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Region {
    Call(Var),
    Value(u32, usize),
    Declared(RegionBound),
    Inference(u32),
}

pub(super) struct Check {
    pub from: Vec<Region>,
    pub to: Vec<Region>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Origin {
    #[cfg(test)]
    Input {
        port: usize,
        slot: usize,
    },
    Lifetime(RegionBound),
    Region(Region),
    Inference(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Relation {
    pub from: Vec<Origin>,
    pub to: Vec<Origin>,
}

type Slots = slot_maps::MapId;
type Bindings = HashMap<ParamRef, Slots>;
type Lifetimes = HashMap<RegionBound, Slots>;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Inputs {
    tref: TraitRef,
    ports: Vec<Slots>,
    lifetimes: usize,
}

#[derive(Clone, Copy)]
struct Span {
    key: Key,
    at: usize,
    len: usize,
    stride: usize,
}

struct RowInputs {
    owner: DefId,
    bindings: Bindings,
    environment: usize,
    bound_slots: Vec<Vec<Vec<u32>>>,
    members: Option<member_inputs::Members>,
}

#[derive(Default)]
struct Projection {
    maps: slot_maps::Maps,
    spans: HashMap<(Ty, Ty), std::sync::Arc<[Span]>>,
    canonical: HashMap<(Ty, DefId), Ty>,
    widths: HashMap<Ty, usize>,
    environments: Vec<Lifetimes>,
    inference: HashMap<(usize, RegionBound), u32>,
    seen: HashSet<(RegionProofId, Inputs)>,
    leaves: HashSet<(Ty, Slots, Slots)>,
    rows: HashMap<(usize, ImplRef, hd_types::TyList), std::sync::Arc<RowInputs>>,
}

fn bug(message: &'static str) -> NotImplemented {
    NotImplemented::bug(Stage::Body, message)
}

fn declared(owner: DefId, index: u32) -> RegionBound {
    if index == u32::from(hd_resolve::STATIC_LIFETIME) {
        RegionBound::Static
    } else {
        // Named lifetimes use resolver slots; anonymous positions use
        // ANON-based indices in the same declaration's region scope.
        RegionBound::Lifetime { owner, index }
    }
}

impl Projection {
    fn charge(n: usize) {
        crate::work::scan_work(n);
    }

    fn width(&mut self, ls: &Layouts<'_>, root: Ty) -> usize {
        let counts = SlotCounts::new(ls.suspend);
        let mut stack = vec![(root, false)];
        let mut parts = Vec::new();
        while let Some((ty, exit)) = stack.pop() {
            if self.widths.contains_key(&ty) {
                continue;
            }
            if let Some(layout) = ls.cached(ty) {
                Self::charge(1);
                self.widths.insert(ty, layout.slots.len());
                continue;
            }
            parts.clear();
            let own = counts.parts(ls.pool, ty, &mut parts) as usize;
            Self::charge(1 + parts.len());
            if !exit {
                stack.push((ty, true));
                stack.extend(parts.iter().map(|&p| (p, false)));
                continue;
            }
            // ruled out: every child exits before its parent's exit frame.
            let mut width = parts.iter().fold(own, |n, p| n + self.widths[p]);
            if let TyData::Ref {
                counted: false,
                inner,
                ..
            } = ls.pool.get(ty)
                && self.widths[&inner] == 0
                && whole_referent(ls, inner)
            {
                width += 1;
            }
            self.widths.insert(ty, width);
        }
        // ruled out: the root exit frame has run before the stack empties.
        self.widths[&root]
    }

    fn spans(
        &mut self,
        ls: &Layouts<'_>,
        pattern: Ty,
        actual: Ty,
        owner: DefId,
    ) -> StageResult<std::sync::Arc<[Span]>> {
        enum Step {
            Pair(Ty, Ty),
            Whole(Ty, Ty),
            Retained,
        }
        let pool = ls.pool;
        Self::charge(1);
        let canonical = *self.canonical.entry((pattern, owner)).or_insert_with(|| {
            pool.subst(pattern, &|p: ParamRef| {
                (p.owner == owner).then(|| {
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner: DefId::NONE,
                        index: p.index,
                    }))
                })
            })
        });
        if let Some(spans) = self.spans.get(&(canonical, actual)) {
            return Ok(spans.clone());
        }
        let mut spans = Vec::new();
        let (mut at, mut declared) = (0, 0);
        let mut stack = vec![Step::Pair(canonical, actual)];
        while let Some(step) = stack.pop() {
            Self::charge(1);
            let mut emit = |key, len, stride| {
                if len != 0 {
                    spans.push(Span {
                        key,
                        at,
                        len,
                        stride,
                    });
                }
                at += len;
            };
            let (d, t) = match step {
                Step::Pair(d, t) => (d, t),
                Step::Retained => {
                    emit(Key::Decl(declared), 1, 0);
                    declared += 1;
                    continue;
                }
                Step::Whole(d, t) => {
                    let (TyData::Ref { inner: a, .. }, TyData::Ref { inner: b, .. }) =
                        (pool.get(d), pool.get(t))
                    else {
                        return Err(bug("a whole referent lost its reference pattern"));
                    };
                    let x = self.width(ls, a) == 0 && whole_referent(ls, a);
                    let y = self.width(ls, b) == 0 && whole_referent(ls, b);
                    match (x, y) {
                        (true, true) => {
                            emit(Key::Decl(declared), 1, 0);
                            declared += 1;
                        }
                        (true, false) => declared += 1,
                        (false, true) => {
                            let key = match pool.get(a) {
                                TyData::Param(p) => Key::Param(p, 0),
                                _ => Key::None,
                            };
                            emit(key, 1, 0);
                        }
                        (false, false) => {}
                    }
                    continue;
                }
            };
            match (pool.get(d), pool.get(t)) {
                (TyData::Param(p), _) => {
                    let n = self.width(ls, t);
                    emit(Key::Param(p, 0), n, 1);
                }
                (TyData::Fn { .. }, TyData::Fn { .. }) => {
                    emit(Key::Decl(declared), 1, 0);
                    declared += 1;
                }
                (TyData::Opaque { borrows: true, .. }, _) => {
                    let n = self.width(ls, t);
                    emit(Key::Decl(declared), n, 0);
                    declared += 1;
                }
                (TyData::Opaque { .. }, _) => {
                    let n = self.width(ls, t);
                    emit(Key::None, n, 0);
                }
                (
                    TyData::Ref {
                        counted: false,
                        inner: a,
                        ..
                    },
                    TyData::Ref {
                        counted: false,
                        inner: b,
                        ..
                    },
                ) => {
                    emit(Key::Decl(declared), 1, 0);
                    declared += 1;
                    stack.push(Step::Whole(d, t));
                    stack.push(Step::Pair(a, b));
                }
                (
                    TyData::Ref {
                        counted: true,
                        inner: a,
                        ..
                    },
                    TyData::Ref {
                        counted: true,
                        inner: b,
                        ..
                    },
                ) => {
                    stack.push(Step::Pair(a, b));
                }
                (TyData::Tuple { elems: a, rest: ra }, TyData::Tuple { elems: b, rest: rb }) => {
                    if ra.is_some() != rb.is_some()
                        || pool.list_items(a).len() != pool.list_items(b).len()
                    {
                        return Err(bug("a proof tuple pattern changed shape"));
                    }
                    if let (Some(a), Some(b)) = (ra, rb) {
                        stack.push(Step::Pair(a, b));
                    }
                    stack.extend(
                        pool.list_items(a)
                            .iter()
                            .zip(pool.list_items(b))
                            .rev()
                            .map(|(&a, &b)| Step::Pair(a, b)),
                    );
                }
                (TyData::Union(a), TyData::Union(b)) => {
                    if pool.list_items(a).len() != pool.list_items(b).len() {
                        return Err(bug("a proof union pattern changed shape"));
                    }
                    stack.extend(
                        pool.list_items(a)
                            .iter()
                            .zip(pool.list_items(b))
                            .rev()
                            .map(|(&a, &b)| Step::Pair(a, b)),
                    );
                }
                (TyData::Adt { def: x, args: a }, TyData::Adt { def: y, args: b })
                | (
                    TyData::TraitValue {
                        def: x, args: a, ..
                    },
                    TyData::TraitValue {
                        def: y, args: b, ..
                    },
                ) if x == y => {
                    if pool.list_items(a).len() != pool.list_items(b).len() {
                        return Err(bug("a proof nominal pattern changed shape"));
                    }
                    if x == ls.suspend {
                        stack.push(Step::Retained);
                    }
                    stack.extend(
                        pool.list_items(a)
                            .iter()
                            .zip(pool.list_items(b))
                            .rev()
                            .map(|(&a, &b)| Step::Pair(a, b)),
                    );
                }
                _ if self.width(ls, d) == 0 && self.width(ls, t) == 0 => {}
                _ => return Err(bug("a proof pattern lost its region correspondence")),
            }
        }
        let spans: std::sync::Arc<[Span]> = spans.into();
        self.spans.insert((canonical, actual), spans.clone());
        Ok(spans)
    }

    fn param(owner: DefId, p: ParamRef) -> ParamRef {
        if p.owner == DefId::NONE {
            ParamRef { owner, ..p }
        } else {
            p
        }
    }

    fn bind(
        &mut self,
        ls: &Layouts<'_>,
        pattern: Ty,
        actual: Ty,
        origins: Slots,
        bindings: &mut Bindings,
        lifetimes: &mut Lifetimes,
        owner: DefId,
        written: &[u32],
    ) -> StageResult<()> {
        for span in self.spans(ls, pattern, actual, owner)?.iter() {
            let from = self.maps.slice(origins, span.at, span.len);
            match span.key {
                Key::Param(p, start) => {
                    let p = Self::param(owner, p);
                    let end = start + span.len;
                    let from = if start == 0 {
                        from
                    } else {
                        let prefix = self.maps.empty(start);
                        self.maps.concat([prefix, from])
                    };
                    let into = match bindings.get(&p).copied() {
                        None => from,
                        Some(old) if self.maps.len(old) == self.maps.len(from) => {
                            self.maps.union(old, from)
                        }
                        Some(old) if self.maps.len(old) == start => {
                            let tail = self.maps.slice(from, start, span.len);
                            self.maps.concat([old, tail])
                        }
                        Some(_) => return Err(bug("an impl parameter's region ranges disagree")),
                    };
                    if self.maps.len(into) != end {
                        return Err(bug("a bound lost its parameter region range"));
                    }
                    bindings.insert(p, into);
                }
                Key::AllOf(p, _) => {
                    let p = Self::param(owner, p);
                    let from = self.maps.all(from);
                    let from = match bindings.get(&p).copied() {
                        Some(old) => self.maps.union(old, from),
                        None => from,
                    };
                    bindings.insert(p, from);
                }
                Key::Decl(start) => {
                    for k in 0..if span.stride == 0 { 1 } else { span.len } {
                        let index = written.get(start + k).copied().unwrap_or(0);
                        if index == 0 || index == u32::from(hd_resolve::STATIC_LIFETIME) {
                            continue;
                        }
                        let region = declared(owner, index);
                        let from = if span.stride == 0 {
                            self.maps.all(from)
                        } else {
                            self.maps.slice(from, k, 1)
                        };
                        let from = match lifetimes.get(&region).copied() {
                            Some(old) => self.maps.union(old, from),
                            None => from,
                        };
                        lifetimes.insert(region, from);
                    }
                }
                Key::None => {}
            }
        }
        Ok(())
    }

    fn mapped(
        &mut self,
        ls: &Layouts<'_>,
        pattern: Ty,
        actual: Ty,
        bindings: &Bindings,
        lifetimes: usize,
        owner: DefId,
        written: &[u32],
        member: Option<(ParamRef, Slots)>,
    ) -> StageResult<Slots> {
        let mut parts = Vec::new();
        for span in self.spans(ls, pattern, actual, owner)?.iter() {
            parts.push(match span.key {
                Key::Param(p, start) => {
                    let p = Self::param(owner, p);
                    let source = member
                        .filter(|&(param, _)| param == p)
                        .map(|(_, slots)| slots)
                        .or_else(|| bindings.get(&p).copied())
                        .ok_or_else(|| bug("an impl bound refers to an unbound input region"))?;
                    self.maps.slice(source, start, span.len)
                }
                Key::AllOf(p, _) => {
                    let p = Self::param(owner, p);
                    let source = member
                        .filter(|&(param, _)| param == p)
                        .map(|(_, slots)| slots)
                        .or_else(|| bindings.get(&p).copied())
                        .ok_or_else(|| bug("a projection bound lost its input regions"))?;
                    let source = self.maps.all(source);
                    self.maps.repeat(source, span.len)
                }
                Key::Decl(start) => {
                    let mut declared_parts = Vec::new();
                    for k in 0..if span.stride == 0 { 1 } else { span.len } {
                        let index = written.get(start + k).copied().unwrap_or(0);
                        let index = if index == 0 {
                            super::ANON + super::w(start + k)
                        } else {
                            index
                        };
                        let region = declared(owner, index);
                        let id = self.region(lifetimes, region);
                        declared_parts.push(id);
                    }
                    let id = self.maps.concat(declared_parts);
                    if span.stride == 0 {
                        self.maps.repeat(id, span.len)
                    } else {
                        id
                    }
                }
                Key::None => self.maps.empty(span.len),
            });
        }
        Ok(self.maps.concat(parts))
    }

    /// Slotless inputs have the unique empty correspondence. They need
    /// no entry in the call's input map, even when another trait argument
    /// supplies the lifetime required by this proof.
    fn input(
        &mut self,
        ls: &Layouts<'_>,
        parameters: &Bindings,
        param: ParamRef,
        ty: Ty,
    ) -> StageResult<Slots> {
        let width = self.width(ls, ty);
        match parameters.get(&param).copied() {
            Some(id) if self.maps.len(id) == width => Ok(id),
            None if width == 0 => Ok(self.maps.empty(0)),
            _ => Err(bug(
                "a proof input lost its positional region correspondence",
            )),
        }
    }

    fn environment(&mut self, lifetimes: Lifetimes, scoped: bool) -> usize {
        // Static-only proofs have no existential binder to freshen. Keep
        // their shared proof tails in the canonical empty environment.
        if lifetimes.is_empty() && !scoped {
            return 0;
        }
        let id = self.environments.len();
        self.environments.push(lifetimes);
        id
    }

    fn region(&mut self, environment: usize, region: RegionBound) -> Slots {
        if let Some(&slots) = self.environments[environment].get(&region) {
            return slots;
        }
        if region == RegionBound::Static {
            return self.maps.group([Origin::Lifetime(region)]);
        }
        let next = hd_tir::ir::u32_of(self.inference.len());
        let id = *self.inference.entry((environment, region)).or_insert(next);
        self.maps.group([Origin::Inference(id)])
    }
}

impl Ck<'_, '_> {
    pub(super) fn call_proof_checks(
        &self,
        ls: &Layouts<'_>,
        body: &hd_tir::ir::Body,
        call: usize,
        took: &[(Key, Var)],
        uses: &[ProofUse],
    ) -> StageResult<Box<[Check]>> {
        let pool = ls.pool;
        let mut projection = Projection::default();
        projection.environments.push(Lifetimes::default());
        let mut groups = HashMap::<(ParamRef, usize), Slots>::default();
        for &(key, var) in took {
            if let Key::Param(p, slot) = key {
                let next = projection.maps.group([Origin::Region(Region::Call(var))]);
                let into = match groups.get(&(p, slot)).copied() {
                    Some(old) => projection.maps.union(old, next),
                    None => next,
                };
                groups.insert((p, slot), into);
            }
        }
        let empty = projection.maps.empty(1);
        let mut parameters = HashMap::<ParamRef, Vec<Slots>>::default();
        for ((p, slot), origins) in groups {
            let slots = parameters.entry(p).or_default();
            slots.resize(slots.len().max(slot + 1), empty);
            slots[slot] = origins;
        }
        let parameters: Bindings = parameters
            .into_iter()
            .map(|(p, slots)| (p, projection.maps.concat(slots)))
            .collect();
        let mut checks = Vec::new();
        let mut values = HashMap::default();
        for use_ in uses {
            let tref = use_.tref;
            let mut ports = Vec::new();
            match use_.source {
                ProofSource::Param { param, bound } => {
                    let id = projection.input(ls, &parameters, param, tref.self_ty)?;
                    ports.push(id);
                    if !pool.list_items(tref.args).is_empty() {
                        let item = self
                            .cx
                            .lookup
                            .item(param.owner)
                            .ok_or_else(|| bug("a generic proof lost its declaration"))?;
                        let generics = item.sig().map_or(&item.generics, |s| &s.generics);
                        let generic = generics
                            .get(usize::from(param.index))
                            .ok_or_else(|| bug("a generic proof lost its parameter"))?;
                        let bound_ty = generic
                            .bounds
                            .get(bound)
                            .copied()
                            .ok_or_else(|| bug("a generic proof lost its written bound"))?;
                        let (_, patterns) = pool
                            .trait_value(bound_ty)
                            .ok_or_else(|| bug("a generic proof's bound is not a trait"))?;
                        let lts: Vec<u32> = generic
                            .bound_lifetimes
                            .get(bound)
                            .into_iter()
                            .flatten()
                            .map(|&x| u32::from(x))
                            .collect();
                        let ids = arg_ids(ls, pool.list_items(patterns), &lts);
                        let mut environment = Lifetimes::default();
                        for &index in &lts {
                            if index != 0 && index != u32::from(hd_resolve::STATIC_LIFETIME) {
                                let region = declared(param.owner, index);
                                let map = projection
                                    .maps
                                    .group([Origin::Region(Region::Call(Var::Life(index)))]);
                                environment.insert(region, map);
                            }
                        }
                        let environment = projection
                            .environment(environment, use_.obligations.has_named_regions());
                        for (k, (&pattern, &actual)) in pool
                            .list_items(patterns)
                            .iter()
                            .zip(pool.list_items(tref.args))
                            .enumerate()
                        {
                            ports.push(projection.mapped(
                                ls,
                                pattern,
                                actual,
                                &parameters,
                                environment,
                                param.owner,
                                &ids[k],
                                None,
                            )?);
                        }
                    }
                }
                ProofSource::Receiver(value) | ProofSource::Value(value) => {
                    let inst = value.as_inst().ok_or_else(|| {
                        bug("a deferred reference proof has no value instruction")
                    })?;
                    let mut ty = body.ty[inst.0 as usize];
                    let mut offset = 0;
                    while ty != tref.self_ty {
                        let TyData::Ref { inner, counted, .. } = pool.get(ty) else {
                            return Err(bug("a proof receiver lost its reference correspondence"));
                        };
                        offset += usize::from(!counted);
                        ty = inner;
                    }
                    let key = (inst.0, offset, ty);
                    let id = if let Some(&id) = values.get(&key) {
                        id
                    } else {
                        let mapped_slots: Vec<_> = (0..projection.width(ls, ty))
                            .map(|slot| {
                                projection
                                    .maps
                                    .group([Origin::Region(Region::Value(inst.0, offset + slot))])
                            })
                            .collect();
                        let id = projection.maps.concat(mapped_slots);
                        values.insert(key, id);
                        id
                    };
                    ports.push(id);
                    for (index, &arg) in (1u16..).zip(pool.list_items(tref.args)) {
                        let id = projection.input(
                            ls,
                            &parameters,
                            ParamRef {
                                owner: tref.trait_,
                                index,
                            },
                            arg,
                        )?;
                        ports.push(id);
                    }
                }
                ProofSource::Expression | ProofSource::Implementation(_) => {
                    let Some(hd_tir::ir::Callee::TraitMethod { trait_, .. }) =
                        hd_tir::ir::Callee::from_words(body.record(body.data[call][0]))
                    else {
                        return Err(bug(
                            "an expression proof has no committed input correspondence",
                        ));
                    };
                    let written_inputs = if let ProofSource::Implementation(owner) = use_.source {
                        let item = self
                            .cx
                            .lookup
                            .item(owner)
                            .ok_or_else(|| bug("a generated proof lost its implementation"))?;
                        let ItemData::Impl { trait_slots, .. } = &item.data else {
                            return Err(bug("a generated proof names a non-implementation"));
                        };
                        let written: Vec<u32> = trait_slots.iter().map(|&l| u32::from(l)).collect();
                        let environment = projection.environment(Lifetimes::default(), true);
                        Some((
                            owner,
                            arg_ids(ls, pool.list_items(tref.args), &written),
                            environment,
                        ))
                    } else {
                        None
                    };
                    let mut anonymous = 0;
                    for (index, ty) in (0u16..).zip(
                        std::iter::once(tref.self_ty)
                            .chain(pool.list_items(tref.args).iter().copied()),
                    ) {
                        let param = ParamRef {
                            owner: trait_,
                            index,
                        };
                        if let Some((owner, ids, environment)) = &written_inputs
                            && index > 0
                            && !parameters.contains_key(&param)
                        {
                            let mut argument_slots = Vec::new();
                            for &id in &ids[usize::from(index) - 1] {
                                Projection::charge(1);
                                let region = declared(
                                    *owner,
                                    if id == 0 { super::ANON + anonymous } else { id },
                                );
                                anonymous += 1;
                                let slots = if id == 0 {
                                    projection.region(*environment, region)
                                } else {
                                    projection.maps.group([Origin::Lifetime(region)])
                                };
                                argument_slots.push(slots);
                            }
                            ports.push(projection.maps.concat(argument_slots));
                        } else {
                            ports.push(projection.input(ls, &parameters, param, ty)?);
                        }
                    }
                }
            }
            let region = |origin| -> StageResult<Region> {
                match origin {
                    Origin::Region(region) => Ok(region),
                    Origin::Lifetime(region) => Ok(Region::Declared(region)),
                    Origin::Inference(id) => Ok(Region::Inference(id)),
                    #[cfg(test)]
                    Origin::Input { .. } => {
                        Err(bug("a call proof retained an abstract root input"))
                    }
                }
            };
            for relation in proof_relations(
                self.cx.lookup,
                Impls::Owned(self.cx.impls),
                ls,
                tref,
                &use_.obligations,
                &mut projection,
                ports,
            )? {
                let from = relation
                    .from
                    .into_iter()
                    .map(region)
                    .collect::<StageResult<_>>()?;
                let to = relation
                    .to
                    .into_iter()
                    .map(region)
                    .collect::<StageResult<_>>()?;
                checks.push(Check { from, to });
            }
        }
        Ok(checks.into_boxed_slice())
    }
}

fn proof_relations(
    lookup: &hd_resolve::Lookup<'_>,
    impls: Impls<'_>,
    ls: &Layouts<'_>,
    tref: TraitRef,
    obligations: &RegionObligations,
    projection: &mut Projection,
    ports: Vec<Slots>,
) -> StageResult<Vec<Relation>> {
    let pool = ls.pool;
    let mut stack = vec![(
        obligations,
        Inputs {
            tref,
            ports,
            lifetimes: 0,
        },
    )];
    let mut out = Vec::new();
    while let Some((requirements, inputs)) = stack.pop() {
        Projection::charge(1);
        let Some(id) = requirements.id() else {
            continue;
        };
        if !projection.seen.insert((id, inputs.clone())) {
            continue;
        }
        let context = projection.seen.len();
        for requirement in requirements {
            Projection::charge(1);
            match requirement {
                RegionObligation::TypeOutlives { ty, region } => {
                    if *ty != inputs.tref.self_ty {
                        return Err(bug("a type-outlives proof lost its subject"));
                    }
                    let to = projection.region(inputs.lifetimes, *region);
                    let source = inputs.ports[0];
                    if !projection.leaves.insert((*ty, source, to)) {
                        continue;
                    }
                    let to = projection
                        .maps
                        .expand(to, 0, 1)
                        .into_iter()
                        .next()
                        .ok_or_else(|| bug("an outlives target lost its one region"))?;
                    let slots = projection
                        .maps
                        .expand(source, 0, projection.maps.len(source));
                    let layout = ls.of(*ty);
                    let mut from = Vec::new();
                    let mut atoms = HashSet::default();
                    for &slot in &layout.tops {
                        for &origin in slots
                            .get(slot)
                            .ok_or_else(|| bug("an outlives source lost its slot"))?
                        {
                            if atoms.insert(origin) {
                                from.push(origin);
                            }
                        }
                    }
                    // An origin-free value needs no region edge. This
                    // also keeps ordinary scalar-only calls off the
                    // region solve's path after a compound static proof.
                    if from.is_empty() {
                        continue;
                    }
                    out.push(Relation { from, to });
                }
                RegionObligation::ImplBound {
                    row,
                    args,
                    step,
                    obligations,
                } => {
                    let table = impls
                        .table(row.module)
                        .ok_or_else(|| bug("a deferred proof lost its impl table"))?;
                    let r = row.row as usize;
                    let key = (context, *row, *args);
                    if !projection.rows.contains_key(&key) {
                        let owner = *table
                            .def
                            .get(r)
                            .ok_or_else(|| bug("a deferred proof lost its impl row"))?;
                        let item = lookup
                            .item(owner)
                            .ok_or_else(|| bug("a deferred proof lost its impl declaration"))?;
                        let mut bindings = Bindings::default();
                        // Each application binds a fresh impl lifetime scope.
                        let mut lifetimes = Lifetimes::default();
                        let (self_lts, trait_lts) = match &item.data {
                            ItemData::Impl {
                                self_lifetimes,
                                trait_slots,
                                ..
                            } => (self_lifetimes.as_slice(), trait_slots.as_slice()),
                            _ => return Err(bug("a deferred impl proof names a non-impl")),
                        };
                        let written: Vec<u32> = self_lts.iter().map(|&v| u32::from(v)).collect();
                        let ids = arg_ids(ls, &[table.head_self[r]], &written);
                        projection.bind(
                            ls,
                            table.head_self[r],
                            inputs.tref.self_ty,
                            inputs.ports[0],
                            &mut bindings,
                            &mut lifetimes,
                            owner,
                            &ids[0],
                        )?;
                        let written: Vec<u32> = trait_lts.iter().map(|&v| u32::from(v)).collect();
                        let patterns = pool.list_items(table.head_args[r]);
                        let ids = arg_ids(ls, patterns, &written);
                        for (k, (&pat, &actual)) in patterns
                            .iter()
                            .zip(pool.list_items(inputs.tref.args))
                            .enumerate()
                        {
                            projection.bind(
                                ls,
                                pat,
                                actual,
                                inputs.ports[k + 1],
                                &mut bindings,
                                &mut lifetimes,
                                owner,
                                &ids[k],
                            )?;
                        }
                        let environment =
                            projection.environment(lifetimes, obligations.has_named_regions());
                        // Preserve each written bound's positional lifetimes in
                        // the same order as the resolver's Bound/Bind plan.
                        // Build once per impl application, never per subgoal.
                        let mut bound_slots = Vec::with_capacity(table.plan[r].len());
                        for generic in &item.generics {
                            Projection::charge(1 + generic.bounds.len());
                            for (index, &bound) in generic.bounds.iter().enumerate() {
                                let TyData::TraitValue { args, bindings, .. } = pool.get(bound)
                                else {
                                    continue;
                                };
                                let written: Vec<u32> = generic
                                    .bound_lifetimes
                                    .get(index)
                                    .into_iter()
                                    .flatten()
                                    .map(|&v| u32::from(v))
                                    .collect();
                                bound_slots.push(arg_ids(ls, pool.list_items(args), &written));
                                for (_, ty) in bindings {
                                    if let TyData::Param(p) = pool.get(ty)
                                        && p.owner == owner
                                    {
                                        bound_slots.push(Vec::new());
                                    }
                                }
                            }
                        }
                        if bound_slots.len() != table.plan[r].len() {
                            return Err(bug("an impl's written bounds disagree with its plan"));
                        }
                        let members = if matches!(
                            table.origin[r],
                            ImplOrigin::TupleTemplate | ImplOrigin::UnionTemplate
                        ) {
                            impls
                                .tuple_plan(owner)
                                .map(|steps| {
                                    member_inputs::Members::new(
                                        projection,
                                        ls,
                                        table.origin[r],
                                        inputs.tref.self_ty,
                                        inputs.ports[0],
                                        steps,
                                    )
                                })
                                .transpose()?
                        } else {
                            None
                        };
                        projection.rows.insert(
                            key,
                            std::sync::Arc::new(RowInputs {
                                owner,
                                bindings,
                                environment,
                                bound_slots,
                                members,
                            }),
                        );
                    }
                    // ruled out: the row context was inserted immediately above.
                    let row_inputs = projection.rows[&key].clone();
                    let owner = row_inputs.owner;
                    if usize::from(*step) >= table.plan[r].len() {
                        let member = row_inputs
                            .members
                            .as_ref()
                            .and_then(|members| {
                                members.get(usize::from(*step) - table.plan[r].len())
                            })
                            .ok_or_else(|| bug("a deferred template proof lost its member step"))?;
                        let (step, ty, slots) = member;
                        let instantiate = |t| {
                            pool.subst(t, &|p: ParamRef| {
                                if p.owner != owner {
                                    None
                                } else if p.index == MEMBER_PARAM {
                                    Some(ty)
                                } else {
                                    pool.list_items(*args).get(usize::from(p.index)).copied()
                                }
                            })
                        };
                        let mut actuals = Vec::new();
                        let mut ports = vec![slots];
                        for &pattern in pool.list_items(step.args) {
                            let actual = instantiate(pattern);
                            actuals.push(actual);
                            ports.push(projection.mapped(
                                ls,
                                pattern,
                                actual,
                                &row_inputs.bindings,
                                row_inputs.environment,
                                owner,
                                &[],
                                Some((
                                    ParamRef {
                                        owner,
                                        index: MEMBER_PARAM,
                                    },
                                    slots,
                                )),
                            )?);
                        }
                        stack.push((
                            obligations,
                            Inputs {
                                tref: TraitRef {
                                    trait_: step.trait_,
                                    self_ty: ty,
                                    args: pool.list(&actuals),
                                },
                                ports,
                                lifetimes: row_inputs.environment,
                            },
                        ));
                        continue;
                    }
                    let Some(PlanStep::Bound {
                        param,
                        trait_,
                        args: patterns,
                        ..
                    }) = table.plan[r].get(usize::from(*step))
                    else {
                        return Err(bug("a deferred proof lost its bound plan step"));
                    };
                    let arg = pool
                        .list_items(*args)
                        .get(usize::from(*param))
                        .copied()
                        .ok_or_else(|| bug("a deferred bound lost its impl argument"))?;
                    let p = ParamRef {
                        owner,
                        index: u16::from(*param),
                    };
                    let slots = match row_inputs.bindings.get(&p).copied() {
                        Some(slots) => slots,
                        None => projection.maps.empty(0),
                    };
                    if projection.maps.len(slots) != projection.width(ls, arg) {
                        return Err(bug(
                            "a bound's subject regions were not supplied by its impl head",
                        ));
                    }
                    let actuals: Vec<Ty> = pool
                        .list_items(*patterns)
                        .iter()
                        .map(|&t| {
                            pool.subst(t, &|p: ParamRef| {
                                (p.owner == owner)
                                    .then(|| {
                                        pool.list_items(*args).get(usize::from(p.index)).copied()
                                    })
                                    .flatten()
                            })
                        })
                        .collect();
                    let mut ports = vec![slots];
                    for (index, (&pat, &actual)) in
                        pool.list_items(*patterns).iter().zip(&actuals).enumerate()
                    {
                        ports.push(projection.mapped(
                            ls,
                            pat,
                            actual,
                            &row_inputs.bindings,
                            row_inputs.environment,
                            owner,
                            &row_inputs.bound_slots[usize::from(*step)][index],
                            None,
                        )?);
                    }
                    stack.push((
                        obligations,
                        Inputs {
                            tref: TraitRef {
                                trait_: *trait_,
                                self_ty: arg,
                                args: pool.list(&actuals),
                            },
                            ports,
                            lifetimes: row_inputs.environment,
                        },
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// The schema caches survive header items; allocation identities and
/// instantiation environments are query-local because answers may drop.
#[derive(Default)]
pub(super) struct HeaderProofs(Projection);

impl HeaderProofs {
    pub(super) fn fails(
        &mut self,
        ls: &Layouts<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        impls: Impls<'_>,
        proof: &super::WrittenProof<'_>,
    ) -> StageResult<bool> {
        let projection = &mut self.0;
        projection.seen.clear();
        projection.leaves.clear();
        projection.rows.clear();
        projection.environments.clear();
        projection.environments.push(Lifetimes::default());
        projection.inference.clear();
        let mut ports = if let Some(subject) = &proof.subject_projection {
            let args = ls.pool.list_items(subject.args);
            let inputs = Self::inputs(projection, ls, proof.owner, args, subject.source_lifetimes);
            let mut bindings = Bindings::default();
            for (index, slots) in inputs.iter().copied().enumerate() {
                Projection::charge(1);
                let index = u16::try_from(index).map_err(|_| {
                    bug("a projected header type exceeds the generic parameter limit")
                })?;
                bindings.insert(
                    ParamRef {
                        owner: subject.declaration,
                        index,
                    },
                    slots,
                );
            }
            let declaration = lookup
                .item(subject.declaration)
                .ok_or_else(|| bug("a projected header type lost its declaration"))?;
            let mut lifetimes = Lifetimes::default();
            let mut named = 0;
            for (index, generic) in declaration.generics.iter().enumerate() {
                Projection::charge(1);
                if generic.lifetime {
                    named += 1;
                    let slots = *inputs
                        .get(index)
                        .ok_or_else(|| bug("a projected lifetime parameter lost its argument"))?;
                    lifetimes.insert(declared(subject.declaration, named), slots);
                }
            }
            let environment = projection.environment(lifetimes, true);
            let ids = Self::written_ids(ls, &[subject.pattern], subject.pattern_lifetimes);
            vec![projection.mapped(
                ls,
                subject.pattern,
                proof.tref.self_ty,
                &bindings,
                environment,
                subject.declaration,
                &ids[0],
                None,
            )?]
        } else {
            Self::inputs(
                projection,
                ls,
                proof.owner,
                &[proof.tref.self_ty],
                proof.subject_lifetimes,
            )
        };
        if let Some(instantiation) = &proof.instantiation {
            let args = ls.pool.list_items(instantiation.args);
            let inputs = Self::inputs(
                projection,
                ls,
                proof.owner,
                args,
                instantiation.argument_lifetimes,
            );
            let source_self = Self::inputs(
                projection,
                ls,
                proof.owner,
                &[instantiation.self_ty],
                instantiation.self_lifetimes,
            );
            let mut bindings = Bindings::default();
            for (index, slots) in source_self.iter().chain(&inputs).copied().enumerate() {
                Projection::charge(1);
                let index = u16::try_from(index).map_err(|_| {
                    bug("a header substitution exceeds the generic parameter limit")
                })?;
                bindings.insert(
                    ParamRef {
                        owner: instantiation.declaration,
                        index,
                    },
                    slots,
                );
            }
            let mut lifetimes = Lifetimes::default();
            for (index, &lifetime) in instantiation.lifetimes.iter().enumerate() {
                Projection::charge(1);
                let target = declared(instantiation.declaration, super::w(index + 1));
                let source = declared(proof.owner, u32::from(lifetime));
                let source = if lifetime == 0 {
                    projection.region(0, target)
                } else {
                    projection
                        .maps
                        .group([Origin::Region(Region::Declared(source))])
                };
                lifetimes.insert(target, source);
            }
            let environment =
                projection.environment(lifetimes, proof.obligations.has_named_regions());
            let (_, patterns) = ls
                .pool
                .trait_value(instantiation.pattern)
                .ok_or_else(|| bug("an inherited header bound is not a trait"))?;
            let patterns = ls.pool.list_items(patterns);
            let actuals = ls.pool.list_items(proof.tref.args);
            if patterns.len() != actuals.len() {
                return Err(bug("an inherited bound lost its argument substitution"));
            }
            let written: Vec<u32> = proof
                .argument_lifetimes
                .iter()
                .map(|&l| u32::from(l))
                .collect();
            let ids = arg_ids(ls, patterns, &written);
            for ((&pattern, &actual), ids) in patterns.iter().zip(actuals).zip(ids) {
                ports.push(projection.mapped(
                    ls,
                    pattern,
                    actual,
                    &bindings,
                    environment,
                    instantiation.declaration,
                    &ids,
                    None,
                )?);
            }
        } else {
            ports.extend(Self::inputs(
                projection,
                ls,
                proof.owner,
                ls.pool.list_items(proof.tref.args),
                proof.argument_lifetimes,
            ));
        }
        let relations = proof_relations(
            lookup,
            impls,
            ls,
            proof.tref,
            proof.obligations,
            projection,
            ports,
        )?;
        Self::violates(relations)
    }

    fn written_ids(ls: &Layouts<'_>, types: &[Ty], written: &[u16]) -> Vec<Vec<u32>> {
        let mut reference = 0;
        let mut position = 0;
        let held =
            std::cell::Cell::new(types.iter().map(|&ty| ls.pool.ref_slots(ty)).sum::<usize>());
        let next_held = || {
            let at = held.get();
            held.set(at + 1);
            written.get(at).copied().filter(|&l| l != 0).map(u32::from)
        };
        types
            .iter()
            .map(|&ty| {
                super::slot_ids(ls, ty, (&mut reference, &mut position), &next_held, |at| {
                    written.get(at).copied().filter(|&l| l != 0).map(u32::from)
                })
            })
            .collect()
    }

    fn inputs(
        projection: &mut Projection,
        ls: &Layouts<'_>,
        owner: DefId,
        types: &[Ty],
        written: &[u16],
    ) -> Vec<Slots> {
        Self::written_ids(ls, types, written)
            .into_iter()
            .map(|ids| {
                let mut slots = Vec::with_capacity(ids.len());
                for id in ids {
                    Projection::charge(1);
                    let region = declared(owner, id);
                    slots.push(if id >= super::ANON {
                        projection.region(0, region)
                    } else {
                        projection
                            .maps
                            .group([Origin::Region(Region::Declared(region))])
                    });
                }
                projection.maps.concat(slots)
            })
            .collect()
    }

    /// Universal inputs seed distinct loans; unbound impl lifetimes are
    /// existential variables. The ordinary region solver propagates those
    /// loans through every outlives edge before testing universal targets.
    fn violates(relations: Vec<Relation>) -> StageResult<bool> {
        use crate::regions::{Category, Constraints, RegionTable};
        let mut table = RegionTable::default();
        let mut constraints = Constraints::default();
        let mut regions = HashMap::default();
        let mut universals = HashMap::default();
        let mut next_loan = 0;
        for relation in relations {
            Projection::charge(1);
            let mut nodes = Vec::new();
            for side in [&relation.from, &relation.to] {
                let mut group = Vec::new();
                for &origin in side {
                    Projection::charge(1);
                    let key = match origin {
                        Origin::Lifetime(RegionBound::Static)
                        | Origin::Region(Region::Declared(RegionBound::Static)) => {
                            Origin::Lifetime(RegionBound::Static)
                        }
                        Origin::Region(Region::Declared(_)) | Origin::Inference(_) => origin,
                        _ => return Err(bug("a header proof contains a body-local origin")),
                    };
                    let node = *regions.entry(key).or_insert_with(|| {
                        let node = table.fresh(1).start;
                        match key {
                            Origin::Lifetime(RegionBound::Static) => {
                                universals.insert(node, None);
                            }
                            Origin::Region(Region::Declared(_)) => {
                                let loan = next_loan;
                                next_loan += 1;
                                constraints.seed(node, loan);
                                universals.insert(node, Some(loan));
                            }
                            _ => {}
                        }
                        node
                    });
                    group.push(node);
                }
                nodes.push(group);
            }
            let sources = &nodes[0];
            let source = if let [one] = sources.as_slice() {
                *one
            } else {
                let hub = table.fresh(1).start;
                for &from in sources {
                    Projection::charge(1);
                    constraints.outlives(from, hub, 0, Category::Flow);
                }
                hub
            };
            for &target in &nodes[1] {
                Projection::charge(1);
                constraints.outlives(source, target, 0, Category::Outlives);
            }
        }
        let solution = constraints.solve(table.len());
        Ok(!solution
            .violations(&constraints, |edge, loan| {
                universals
                    .get(&edge.to)
                    .is_none_or(|allowed| *allowed == Some(loan))
            })
            .is_empty())
    }
}

#[cfg(test)]
mod header_constraint_tests {
    use super::*;

    fn universal(index: u32) -> Origin {
        Origin::Region(Region::Declared(RegionBound::Lifetime {
            owner: DefId::NONE,
            index,
        }))
    }

    #[test]
    fn inferred_header_regions_propagate_universal_restrictions() -> StageResult<()> {
        let a = universal(1);
        let b = universal(2);
        let inferred = Origin::Inference(0);
        let static_ = Origin::Lifetime(RegionBound::Static);
        let relation = |from: &[Origin], to: &[Origin]| Relation {
            from: from.to_vec(),
            to: to.to_vec(),
        };
        assert!(HeaderProofs::violates(vec![
            relation(&[a], &[inferred]),
            relation(&[inferred], &[static_]),
        ])?);
        assert!(!HeaderProofs::violates(vec![
            relation(&[static_], &[inferred]),
            relation(&[inferred], &[a]),
        ])?);
        assert!(HeaderProofs::violates(vec![relation(&[a], &[a, b])])?);
        assert!(!HeaderProofs::violates(vec![relation(
            &[static_, a],
            &[a]
        )])?);
        Ok(())
    }
}
