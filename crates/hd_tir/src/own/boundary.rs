//! Immutable parameter-use facts for instance loan queries.
//!
//! Observe each instruction once. A shared nonbare place is classified once,
//! so work is O(instructions + distinct local-place projection steps), with
//! expected constant-time lookups and no traversal controlled by native depth.

use crate::ir::{Body, Proj, ReadMode, Root, Tag, place, ref_word};
use hd_base::{HashMap, HashSet, SliceSort as _};
use hd_types::Ty;

#[derive(Clone, Copy)]
struct PlaceFacts {
    local: Option<usize>,
    value: Option<u32>,
}

#[derive(Default)]
struct ParameterUses {
    sub_local: HashMap<(u32, usize), Vec<usize>>,
    reads: HashMap<usize, Vec<usize>>,
    moved: HashSet<u32>,
    always: HashSet<usize>,
    framed: HashSet<usize>,
    places: HashMap<u32, PlaceFacts>,
}

impl ParameterUses {
    /// One instruction visit, plus one first place decode and each projection
    /// actually inspected. The caller supplies instructions in body order.
    fn observe(&mut self, body: &Body, index: usize, sub: u32, visit: &mut impl FnMut()) {
        visit();
        let tag = body.tags[index];
        if tag == Tag::Closure {
            let r = body.record(body.data[index][1]);
            for c in r[0]..r[0] + r[1] {
                visit();
                self.sub_local
                    .entry((sub, body.cap_local[c as usize].idx()))
                    .or_default()
                    .push(index);
            }
        }
        if !matches!(tag, Tag::Read | Tag::Store | Tag::Ref) {
            return;
        }
        let word = body.data[index][0];
        if word & place::BARE_LOCAL != 0 {
            let local = (word & !place::BARE_LOCAL) as usize;
            self.sub_local.entry((sub, local)).or_default().push(index);
            if tag == Tag::Read {
                self.reads.entry(local).or_default().push(index);
            } else {
                self.always.insert(local);
            }
            return;
        }
        let facts = *self.places.entry(word).or_insert_with(|| {
            visit();
            let view = body.view(word);
            let local = view.local().filter(|_| {
                view.steps(body).all(|step| {
                    visit();
                    matches!(step, Proj::Field(_) | Proj::Variant(_))
                })
            });
            PlaceFacts {
                local: local.map(|local| local as usize),
                value: match view.root {
                    Root::Value(value) => Some(value),
                    _ => None,
                },
            }
        });
        if let Some(local) = facts.local {
            self.sub_local.entry((sub, local)).or_default().push(index);
            if tag == Tag::Read {
                // A projected read can transfer one counted field to a result
                // or owning callee. Index it just like a whole read so the
                // consumer check cannot mistake an empty read list for a loan.
                self.reads.entry(local).or_default().push(index);
                self.framed.insert(local);
            } else {
                self.always.insert(local);
            }
        }
        if tag == Tag::Read
            && body.data[index][1] == ReadMode::Move as u32
            && let Some(value) = facts.value
        {
            self.moved.insert(value);
        }
    }

    fn reads(&self, local: usize) -> &[usize] {
        self.reads.get(&local).map_or(&[], Vec::as_slice)
    }

    fn moved(&self, value: u32) -> bool {
        self.moved.contains(&value)
    }

    fn rejects(&self, local: usize, framed: bool) -> bool {
        self.always.contains(&local) || framed && self.framed.contains(&local)
    }
}

/// The body's immutable count descriptors and local/capture census.
#[derive(Default)]
pub struct Counts {
    descriptors: HashMap<(u32, Tag, crate::ir::count::At, crate::ir::count::Part), super::Reason>,
    pub owned: HashSet<(u32, usize)>,
    pub exits: HashMap<u32, Vec<usize>>,
    pub unwound: HashSet<u32>,
    pub stored: HashSet<usize>,
    pub captures: HashSet<usize>,
}

impl Counts {
    fn observe(&mut self, b: &Body, i: usize) {
        if b.tags[i] == Tag::Store {
            let p = b.view(b.data[i][0]);
            if p.whole()
                && let Some(l) = p.local()
            {
                self.stored.insert(l as usize);
            }
        }
        if let Some(c) = crate::ir::count::Info::read(b, i) {
            self.descriptors
                .entry((b.data[i][0], b.tags[i], c.at, c.part))
                .or_insert_with(|| super::Reason::from_word(c.reason));
        }
    }

    fn finish(&mut self, b: &Body, visit: &mut impl FnMut()) {
        for &(sub, local) in &b.exit_drops {
            visit();
            self.owned.insert((sub.raw(), local.idx()));
            self.exits.entry(sub.raw()).or_default().push(local.idx());
        }
        for locals in self.exits.values_mut() {
            locals.sort_stable_by(|a, b| {
                visit();
                a.cmp(b)
            });
            locals.dedup_by(|a, b| {
                visit();
                a == b
            });
        }
        self.unwound.extend(b.cancel_releases().map(|(_, v)| v));
        for l in &b.cap_local {
            visit();
            self.captures.insert(l.idx());
        }
    }

    #[must_use]
    pub fn get(
        &self,
        target: u32,
        tag: Tag,
        at: crate::ir::count::At,
        part: crate::ir::count::Part,
    ) -> Option<super::Reason> {
        self.descriptors.get(&(target, tag, at, part)).copied()
    }

    #[cfg(test)]
    fn counted(b: &Body, visit: &mut impl FnMut()) -> Self {
        let mut out = Self::default();
        for i in 0..b.len() {
            visit();
            out.observe(b, i);
        }
        out.finish(b, visit);
        out
    }
}

/// A final instance sub-body's ABI, selected after collection.
#[derive(Clone, Copy)]
pub struct Input {
    pub sub: u32,
    pub by_type: bool,
    pub suspending: bool,
    pub lends: u64,
    pub closure: Option<usize>,
}

/// TIR's final boundary ownership decisions. Emission only reads these rows.
pub struct Boundary {
    pub counts: Counts,
    pub recursive: std::sync::Arc<super::RecursiveBindings>,
    pub consumers: Vec<u32>,
    pub subs: Vec<SubBoundary>,
}

/// Each emitter reads only its sub-body's rows, never the sibling closures'.
#[derive(Default)]
pub struct SubBoundary {
    pub borrowed: Vec<usize>,
    pub loans: Vec<u32>,
    pub slices: Vec<(u32, u32)>,
    pub slice_loans: Vec<u32>,
    /// Each promoted argument with the referent type it is read as.
    pub promoted: Vec<(u32, Ty)>,
}

/// One instruction/place census and one visit per parameter/capture use.
/// No retries, body scans per local, or native input-depth recursion.
pub fn plan(
    b: &Body,
    topo: &crate::ir::Topology,
    cx: &dyn super::OwnCx,
    inputs: &[Input],
    work: &mut impl FnMut(usize),
) -> Boundary {
    use super::ParameterMode;
    use crate::ir::{Callee, IntrinsicOp, NONE};
    let mut uses = ParameterUses::default();
    let mut consumers = Vec::new();
    super::consumers(b, topo, cx, &mut consumers);
    let recursive = std::sync::Arc::new(super::RecursiveBindings::new(b));
    let mut out = Boundary {
        recursive: std::sync::Arc::clone(&recursive),
        counts: Counts::default(),
        consumers: Vec::new(),
        subs: (0..b.sub_root.len())
            .map(|_| SubBoundary::default())
            .collect(),
    };
    let any_await = b.tags.iter().any(|&t| crate::ir::is_suspension(t));
    for i in 0..b.len() {
        uses.observe(b, i, topo.sub[i], &mut || work(1));
        out.counts.observe(b, i);
        // What lends its value with no count of its own
        // (`docs/compiler/tir-v2.md`, "Borrow"): a borrow, a coercion or
        // a wrapper of one, and the block a borrowed scope breaks out of.
        let x = crate::ir::u32_of(i);
        if b.is_borrow(x) || (lending_tag(b.tags[i]) && b.is_borrowed_coercion(x)) {
            let rows = &mut out.subs[topo.sub[i] as usize];
            rows.loans.push(x);
            if b.tags[i] == Tag::Scope {
                rows.loans.push(b.data[i][0]);
            }
        }
        // A promoted argument (§23.40) is a read of its place, typed as its
        // thin referent; a counted one lends, unless it takes its local's
        // count (`ref_word::TAKES`).
        if b.tags[i] == Tag::Ref
            && b.data[i][1] & ref_word::PROMOTED != 0
            && let Some(target) = cx.reference_target(b.ty[i]).filter(|&t| !cx.trait_value(t))
        {
            let rows = &mut out.subs[topo.sub[i] as usize];
            rows.promoted.push((x, target));
            if cx.counted(target) && b.data[i][1] & ref_word::TAKES == 0 {
                rows.loans.push(x);
            }
        }
        if b.tags[i] == Tag::Intrinsic
            && IntrinsicOp::from_u32(b.data[i][0]) == Some(IntrinsicOp::StrEq)
        {
            for &slice in b.record(b.data[i][1]) {
                let s = slice as usize;
                if b.tags.get(s) != Some(&Tag::Call)
                    || !Callee::from_words(b.record(b.data[s][0]))
                        .is_some_and(|c| cx.string_slice(&c))
                {
                    continue;
                }
                let rows = &mut out.subs[topo.sub[i] as usize];
                rows.slices.push((slice, crate::ir::u32_of(i)));
                if let Some(&text) = b.call_args(s).first()
                    && crate::ir::Ref(text).as_inst().is_some()
                    && b.local_read(text as usize).is_some()
                    && b.data[text as usize][1] != ReadMode::Move as u32
                {
                    rows.slice_loans.extend([text, slice]);
                }
            }
        }
    }
    let cells: HashSet<_> = b
        .storage
        .iter()
        .map(|&(s, l, _)| (s.raw(), l.idx()))
        .collect();
    out.counts.finish(b, &mut || work(1));
    let shared: HashSet<_> = b
        .cap_local
        .iter()
        .zip(&b.cap_mode)
        .filter_map(|(&l, m)| m.shares().then_some(l.idx()))
        .collect();
    for input in inputs {
        let sub = input.sub;
        let rows = &mut out.subs[sub as usize];
        let framed = input.suspending && sub == 0 && !any_await;
        if input.by_type || framed {
            for (k, l) in super::cold::locals(b, sub).into_iter().enumerate() {
                let l = l as usize;
                let ty = b.local_ty[l];
                if framed && (cx.cold_parameter_transfers(b.item, ty) || cx.copies_collection(ty)) {
                    continue;
                }
                let mode = cx.parameter_mode(ty, sub != 0);
                let lent = mode.with_lends(input.lends, k) == ParameterMode::Lent;
                if !(framed || lent) || cells.contains(&(sub, l)) || shared.contains(&l) {
                    continue;
                }
                let mut fits = !uses.rejects(l, framed);
                let mut loans = Vec::new();
                for &i in uses.reads(l) {
                    work(1);
                    if !uses.moved(crate::ir::u32_of(i)) && consumers[i] == NONE {
                        loans.push(crate::ir::u32_of(i));
                    } else {
                        fits = false;
                    }
                }
                let owned = out.counts.owned.contains(&(sub, l));
                if owned && !fits {
                    continue;
                }
                if owned {
                    rows.borrowed.push(l);
                }
                rows.loans.extend(loans);
            }
        }
        if !input.suspending
            && let Some(ci) = input.closure
        {
            for (l, shared) in super::closure_captures(b, ci, &recursive) {
                if shared || cells.contains(&(sub, l)) || cx.copies_collection(b.local_ty[l]) {
                    continue;
                }
                let mut fits = true;
                let mut loans = Vec::new();
                for &i in uses.sub_local.get(&(sub, l)).into_iter().flatten() {
                    work(1);
                    if b.tags[i] == Tag::Read
                        && b.data[i][0] & place::BARE_LOCAL != 0
                        && !uses.moved(crate::ir::u32_of(i))
                        && consumers[i] == NONE
                    {
                        loans.push(crate::ir::u32_of(i));
                    } else {
                        fits = false;
                    }
                }
                if fits {
                    rows.borrowed.push(l);
                    rows.loans.extend(loans);
                }
            }
        }
    }
    out.consumers = consumers;
    out
}

/// The tags whose value can be a borrowed coercion: a coercion, a
/// wrapper literal, a selection, or a scope a borrow breaks out of.
fn lending_tag(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::NewStruct
            | Tag::NewTuple
            | Tag::NewVariant
            | Tag::Coerce
            | Tag::Scope
            | Tag::If
            | Tag::Match
            | Tag::SwitchTag
            | Tag::SwitchInt
            | Tag::SwitchChar
            | Tag::SwitchStr
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::BodyKind;
    use hd_base::DefId;
    use std::num::TryFromIntError;

    // These tests exercise instruction/place metadata only. Local field types
    // are irrelevant to the query; no type/layout computation is requested.
    fn body(locals: usize) -> Body {
        let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
        body.local_ty.resize(locals, Ty::I32);
        body
    }

    fn emit(body: &mut Body, tag: Tag, word: u32, mode: u32) -> usize {
        let index = body.len();
        body.tags.push(tag);
        body.data.push([word, mode]);
        index
    }

    fn record(
        body: &mut Body,
        root: u32,
        value: u32,
        steps: &[u32],
    ) -> Result<u32, TryFromIntError> {
        let word = u32::try_from(body.extra.len())?;
        body.extra.push(u32::try_from(2 + steps.len())?);
        body.extra.extend([root, value]);
        body.extra.extend_from_slice(steps);
        Ok(word)
    }

    fn census(body: &Body) -> (ParameterUses, usize) {
        let mut out = ParameterUses::default();
        let mut visits = 0;
        for index in 0..body.len() {
            out.observe(body, index, 0, &mut || visits += 1);
        }
        (out, visits)
    }

    #[test]
    fn bare_reads_and_in_place_rejections_preserve_the_scan_rules() -> Result<(), TryFromIntError> {
        let mut body = body(8);
        let first = emit(
            &mut body,
            Tag::Read,
            place::BARE_LOCAL,
            ReadMode::Use as u32,
        );
        emit(&mut body, Tag::Store, place::BARE_LOCAL | 1, 0);
        emit(&mut body, Tag::Ref, place::BARE_LOCAL | 2, 0);
        for (tag, local, steps) in [
            (Tag::Store, 3, vec![place::PROJ_FIELD]),
            (Tag::Ref, 4, vec![place::PROJ_VARIANT]),
            (Tag::Read, 5, vec![place::PROJ_FIELD]),
            (Tag::Store, 6, vec![place::PROJ_DEREF]),
            (Tag::Ref, 7, vec![place::PROJ_ELEM, 0]),
        ] {
            let word = record(&mut body, place::ROOT_LOCAL, local, &steps)?;
            emit(&mut body, tag, word, ReadMode::Use as u32);
        }
        let first_word = u32::try_from(first)?;
        // Moving any projection of a value prevents lending the original read.
        let moved = record(
            &mut body,
            place::ROOT_VALUE,
            first_word,
            &[place::PROJ_FIELD],
        )?;
        emit(&mut body, Tag::Read, moved, ReadMode::Move as u32);
        let (facts, _) = census(&body);
        assert_eq!(facts.reads(0), [first]);
        assert_eq!(facts.reads(5).len(), 1);
        assert!(facts.reads(100).is_empty());
        assert!(facts.moved(first_word));
        assert!(!facts.moved(1));
        for local in 0..8 {
            assert_eq!(facts.rejects(local, false), (1..=4).contains(&local));
            assert_eq!(facts.rejects(local, true), (1..=5).contains(&local));
        }
        Ok(())
    }

    #[test]
    fn projected_moves_and_borrows_keep_their_consumer_query() -> Result<(), TryFromIntError> {
        let mut body = body(1);
        let word = record(&mut body, place::ROOT_LOCAL, 0, &[place::PROJ_FIELD])?;
        let borrowed = emit(&mut body, Tag::Read, word, ReadMode::Borrow as u32);
        let moved = emit(&mut body, Tag::Read, word, ReadMode::Move as u32);
        let (facts, _) = census(&body);
        assert_eq!(facts.reads(0), [borrowed, moved]);
        // The consumer decides whether each read can lend. Framed locals
        // still reject projections independently of that decision.
        assert!(!facts.rejects(0, false));
        assert!(facts.rejects(0, true));
        Ok(())
    }

    #[test]
    fn nonlocal_and_indirect_places_do_not_reject_the_named_local() -> Result<(), TryFromIntError> {
        let mut body = body(1);
        let first = emit(
            &mut body,
            Tag::Read,
            place::BARE_LOCAL,
            ReadMode::Use as u32,
        );
        let first_word = u32::try_from(first)?;
        for steps in [vec![place::PROJ_DEREF], vec![place::PROJ_ELEM, first_word]] {
            let word = record(&mut body, place::ROOT_LOCAL, 0, &steps)?;
            for tag in [Tag::Read, Tag::Store, Tag::Ref] {
                emit(&mut body, tag, word, ReadMode::Use as u32);
            }
        }
        let word = record(&mut body, place::ROOT_VALUE, first_word, &[])?;
        emit(&mut body, Tag::Read, word, ReadMode::Use as u32);
        let (facts, _) = census(&body);
        assert!(!facts.rejects(0, false));
        assert!(!facts.rejects(0, true));
        assert!(!facts.moved(first_word));
        Ok(())
    }

    #[test]
    fn shared_deep_places_are_walked_once_at_width_thousand_and_depth_thousand()
    -> Result<(), TryFromIntError> {
        for (width, depth) in [(100, 20), (1_000, 20), (20, 1_000)] {
            let mut body = body(width);
            for local in 0..width {
                let local_word = u32::try_from(local)?;
                emit(
                    &mut body,
                    Tag::Read,
                    place::BARE_LOCAL | local_word,
                    ReadMode::Use as u32,
                );
                let steps = vec![place::PROJ_FIELD; depth];
                let word = record(&mut body, place::ROOT_LOCAL, local_word, &steps)?;
                for _ in 0..20 {
                    emit(&mut body, Tag::Read, word, ReadMode::Use as u32);
                }
            }
            let (facts, visits) = census(&body);
            assert_eq!(visits, body.len() + width * (depth + 1));
            for local in 0..width {
                assert_eq!(
                    facts.reads(local),
                    (local * 21..(local + 1) * 21).collect::<Vec<_>>()
                );
                assert!(!facts.rejects(local, false));
                assert!(facts.rejects(local, true));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::super::Reason;
    use super::*;
    use crate::ir::count::{At, Info, Part};
    use crate::ir::{BodyKind, TirBuilder, TirSink, local_flags};
    use hd_base::{DefId, LocalId, NodeIdx, SubId, Symbol};

    #[test]
    fn wide_and_deep_boundaries_are_indexed_once_and_queries_have_no_body_to_rescan()
    -> Result<(), String> {
        for (width, depth) in [(100, 20), (1_000, 20), (20, 1_000)] {
            let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let outer = b.open_block();
            for _ in 0..width {
                b.local(Ty::I32, Symbol::NONE, local_flags::PARAM, NodeIdx::NONE);
            }
            let nested: Vec<_> = (0..depth).map(|_| b.open_block()).collect();
            let mut statements = Vec::new();
            for local in 0..width {
                for (tag, at, reason) in [
                    (Tag::Retain, At::Cold, 8),
                    (Tag::Release, At::Exit, 8),
                    (Tag::Release, At::FrameDrop, 2),
                ] {
                    let record = Info::new(at, Part::Referent(local), reason).write(b.body_mut());
                    statements.push(b.emit(tag, 0, record, Ty::VOID, NodeIdx::NONE));
                }
            }
            for mark in nested.into_iter().rev() {
                b.close_block(mark, None, Ty::VOID, NodeIdx::NONE);
            }
            let root = b.close_block(outer, None, Ty::VOID, NodeIdx::NONE);
            for statement in statements {
                b.body_mut().data[statement.0 as usize][0] = root.0;
            }
            // Construct already-owned records; tolerate only the checked-IR
            // phase gate, retaining the builder's structural verification.
            let mut body = b
                .finish_tolerating(root, &[], &[22])
                .map_err(|errors| format!("{errors:?}"))?;
            body.exit_drops.extend(
                (0..width)
                    .rev()
                    .map(|local| (SubId::from_raw(0), LocalId::from_raw(local))),
            );
            let mut scans = 0;
            let shared = std::sync::OnceLock::<Counts>::new();
            let mut first_work = None;
            for local in 0..width {
                // Model sibling emitters: each owns a cell handle, and
                // every first query supplies its immutable source body.
                let sibling = &shared;
                let counts = sibling.get_or_init(|| Counts::counted(&body, &mut || scans += 1));
                assert_eq!(
                    *first_work.get_or_insert(scans),
                    scans,
                    "sibling queries share the census"
                );
                assert!(counts.owned.contains(&(0, local as usize)));
                assert_eq!(
                    counts.get(root.0, Tag::Retain, At::Cold, Part::Referent(local)),
                    Some(Reason::ByDesign)
                );
                assert_eq!(
                    counts.get(root.0, Tag::Release, At::Exit, Part::Referent(local)),
                    Some(Reason::ByDesign)
                );
                assert_eq!(
                    counts.get(root.0, Tag::Release, At::FrameDrop, Part::Referent(local)),
                    Some(Reason::LastUse)
                );
                assert_eq!(
                    counts.get(root.0, Tag::Retain, At::Cold, Part::Local(local)),
                    None
                );
            }
            let facts = shared.get().ok_or("census initialized")?;
            assert_eq!(
                facts.exits.get(&0),
                Some(&(0..width as usize).collect::<Vec<_>>())
            );
            let sorting = width as usize * (width.ilog2() as usize + 1);
            assert!(scans <= 8 * (body.len() + sorting + width as usize));
        }
        Ok(())
    }
}
