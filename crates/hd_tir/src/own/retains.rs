//! Counts made explicit after a result copies borrowed bits.
//! The result is an owned value; its existing drops still end that owner.
//! A count that would only cancel is never written ([`handed`]).

use super::{OwnCx, ref_copy, splice};
use crate::ir::count::{At, Info, Part};
use crate::ir::{
    Body, Callee, Coercion, NONE, Proj, ReadMode, Ref, Root, Tag, Topology, local_flags, ref_word,
    u32_of,
};
use hd_types::Ty;

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    let results = results(b, cx, topo);
    for (i, &retained) in results.iter().enumerate() {
        if !retained {
            continue;
        }
        let block = topo.blk[i];
        if block == NONE {
            return Err("a retained borrowed result outside a block".into());
        }
        if let Some(drop) = handed(b, cx, topo, i, Tag::Drop) {
            b.tags[drop as usize] = Tag::LoanEnd;
            continue;
        }
        let position = topo.pos[i] + 1;
        let members = b.record(b.data[block as usize][0]);
        if members.get(position as usize).is_some_and(|&next| {
            b.tags[next as usize] == Tag::Retain && b.data[next as usize][0] == u32_of(i)
        }) {
            continue;
        }
        // Stored: the result keeps a reference to the borrowed source.
        // A string slice's empty form points at static storage.
        let record = Info::new(At::Value, Part::Result, 0).write(b);
        edits.insert(
            block,
            position,
            Tag::Retain,
            [u32_of(i), record],
            Ty::VOID,
            b.syn[i],
        );
    }
    splice::apply(b, topo, edits)?;
    if !b.anchors.is_empty() {
        for i in 0..b.len() {
            if b.tags[i] == Tag::Retain {
                b.anchors[i] = b.anchors[b.data[i][0] as usize];
            }
        }
    }
    Ok(())
}

/// The statement of tag `end` (`Drop`, or `LoanEnd` once [`run`] has made it
/// one) that ends the one count result `i` would take, so neither runs: the
/// source's right after a coercion that keeps the source's address, whose
/// count becomes the result's; or, for a provider, its reference's, read only
/// through, since the body's caller holds the provider for the whole body.
pub(crate) fn handed(b: &Body, cx: &dyn OwnCx, topo: &Topology, i: usize, end: Tag) -> Option<u32> {
    let members = member_list(b, topo, i)?;
    let ends = |x: u32, v: u32| b.tags[x as usize] == end && whole_value(b, x, v);
    if b.tags[i] == Tag::Coerce {
        let next = next_after(b, topo, i)?;
        let source = Ref(b.data[i][0]).as_inst()?;
        let mut target = b.ty[source.0 as usize];
        // Through references to a trait value, the result's data is loaded:
        // another object's count than the source's. A provider reference
        // keeps the reference's own words.
        let keeps = b.record(b.data[i][1]).first() == Some(&(Coercion::RefAsProvider as u32));
        while !keeps && !cx.trait_value(target) {
            let Some(inner) = cx.reference_target(target) else {
                break;
            };
            if cx.trait_value(inner) {
                return None;
            }
            target = inner;
        }
        return (ends(next, source.0) && owns_its_count(b, cx, source.0, 8)).then_some(next);
    }
    // A borrowed result dropped at once: nothing wanted the count.
    if b.tags[i] == Tag::Call {
        let next = *members.first()?;
        return ends(next, u32_of(i)).then_some(next);
    }
    if b.tags[i] != Tag::ProviderGet {
        return None;
    }
    let (&coercion, rest) = members.split_first()?;
    if !b.provider_ref(coercion as usize) || b.data[coercion as usize][0] != u32_of(i) {
        return None;
    }
    for &x in rest {
        if ends(x, coercion) {
            return Some(x);
        }
        if b.tags[x as usize] == Tag::Store && b.data[x as usize][1] == coercion {
            return read_through_end(b, x as usize, end);
        }
        if b.tags[x as usize] != Tag::Read
            || b.view(b.data[x as usize][0]).root != Root::Value(coercion)
        {
            return None;
        }
    }
    None
}

/// The one end of the local that `store` fills, when nothing else stores it,
/// references it, captures it or reads all of it, and no suspension point can
/// hold it: every read is through it, so nothing takes a count it doesn't hold.
fn read_through_end(b: &Body, store: usize, end: Tag) -> Option<u32> {
    let p = b.view(b.data[store][0]);
    let l = p.local().filter(|_| p.whole())?;
    // A frame's way out at a suspension point releases the locals it holds.
    if b.cap_local.iter().any(|c| c.idx() == l as usize)
        || b.tags.iter().any(|&tag| crate::ir::is_suspension(tag))
    {
        return None;
    }
    let mut found = None;
    for i in 0..b.len() {
        if !matches!(
            b.tags[i],
            Tag::Store | Tag::Ref | Tag::Read | Tag::Drop | Tag::LoanEnd
        ) {
            continue;
        }
        let v = b.view(b.data[i][0]);
        if v.local() != Some(l) || i == store {
            continue;
        }
        match b.tags[i] {
            Tag::Read if v.steps(b).next() == Some(Proj::Deref) => {}
            t if t == end && v.whole() && found.is_none() => found = Some(u32_of(i)),
            _ => return None,
        }
    }
    found
}

/// Whether value `v` certainly holds a count of its own, whatever the backend keeps
/// uncounted (its loans start at a local's storage or a borrow, never at these): a `Ref`
/// that takes its local's count, a call's result, a read through a reference, a coercion
/// that takes a count or keeps such a value, a `&dyn` or a block whose every value is one,
/// and a move or a stored counted copy of all of a local whose one store is one. A call
/// that lends its result counts too: a trait-value coercion is no use a short loan allows.
fn owns_its_count(b: &Body, cx: &dyn OwnCx, v: u32, depth: u32) -> bool {
    let i = v as usize;
    if depth == 0 || Ref(v).as_inst().is_none() {
        return false;
    }
    match b.tags[i] {
        Tag::Ref => b.data[i][1] & ref_word::TAKES != 0,
        Tag::Call | Tag::CallValue => true,
        // A result that takes a count, stated or its source's.
        Tag::Coerce if result(b, cx, i) => true,
        Tag::Coerce => b.same_value_coercion(i) && owns_its_count(b, cx, b.data[i][0], depth - 1),
        Tag::RefDyn => owns_its_count(b, cx, b.data[i][0], depth - 1),
        // Its tail and each `break` to it.
        Tag::Block | Tag::Scope => {
            let tail = if b.tags[i] == Tag::Scope {
                b.data[i][0]
            } else {
                b.data[i][1]
            };
            let breaks: Vec<u32> = (0..b.len())
                .filter(|&x| {
                    b.tags[x] == Tag::Break && b.label_inst.get(b.data[x][0] as usize) == Some(&v)
                })
                .map(|x| b.data[x][1])
                .collect();
            (tail != NONE || !breaks.is_empty())
                && (tail == NONE || owns_its_count(b, cx, tail, depth - 1))
                && breaks.iter().all(|&x| owns_its_count(b, cx, x, depth - 1))
        }
        Tag::Read if b.is_borrow(v) => false,
        Tag::Read => {
            if b.view(b.data[i][0]).steps(b).next() == Some(Proj::Deref) {
                return true;
            }
            let Some(l) = b.local_read(i) else {
                return false;
            };
            let kept = (0..b.len()).any(|x| b.tags[x] == Tag::Store && b.data[x][1] == v);
            let copy = b.data[i][1] == ReadMode::Copy as u32 && b.ty[i] == b.local_ty[l.idx()];
            if b.data[i][1] != ReadMode::Move as u32 && !(copy && kept && cx.counted(b.ty[i]))
                || b.local_flags[l.idx()] & local_flags::PARAM != 0
                || b.cap_local.iter().any(|c| c.idx() == l.idx())
            {
                return false;
            }
            let mut stores =
                (0..b.len()).filter(|&x| b.stored_local(x).is_some_and(|(y, _)| y == l));
            match (stores.next(), stores.next()) {
                (Some(x), None) if b.stored_local(x).is_some_and(|(_, whole)| whole) => {
                    owns_its_count(b, cx, b.data[x][1], depth - 1)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Whether coercion `i` took its source's count: the next statement ends the
/// source's loan where a stated retain would stand.
#[must_use]
pub fn took_source(b: &Body, topo: &Topology, i: usize) -> bool {
    next_after(b, topo, i)
        .is_some_and(|x| b.tags[x as usize] == Tag::LoanEnd && whole_value(b, x, b.data[i][0]))
}

/// The statement that runs next after `i`: the next in its block; or, past a
/// `&dyn` built from the block `i` ends, the one after that.
fn next_after(b: &Body, topo: &Topology, i: usize) -> Option<u32> {
    if let Some(&x) = member_list(b, topo, i)?.first() {
        return Some(x);
    }
    let block = topo.blk[i] as usize;
    let owner = *topo.owner.get(block)?;
    if b.data[block][1] != u32_of(i) || owner == NONE || b.tags[owner as usize] != Tag::RefDyn {
        return None;
    }
    member_list(b, topo, owner as usize)?.first().copied()
}

/// The statements after `i` in its block.
fn member_list<'a>(b: &'a Body, topo: &Topology, i: usize) -> Option<&'a [u32]> {
    let block = topo.blk[i];
    (block != NONE).then(|| &b.record(b.data[block as usize][0])[topo.pos[i] as usize + 1..])
}

/// Whether statement `x`'s place is all of value `v`.
fn whole_value(b: &Body, x: u32, v: u32) -> bool {
    let p = b.view(b.data[x as usize][0]);
    p.whole() && p.root == Root::Value(v)
}

pub(crate) fn results(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<bool> {
    let mut out = ref_copy::results(b, cx, topo);
    for (i, retained) in out.iter_mut().enumerate() {
        *retained |= result(b, cx, i);
    }
    out
}

pub(crate) fn result(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    match b.tags.get(i) {
        Some(Tag::Call) => {
            b.ty[i] == Ty::STRING
                && Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.retains_result(&c))
        }
        Some(Tag::Coerce) if cx.counted(b.ty[i]) => {
            let kind = b.record(b.data[i][1]).first().copied();
            if kind == Some(Coercion::RefAsProvider as u32) {
                return true;
            }
            if ![
                Coercion::ToTraitValue,
                Coercion::Supertrait,
                Coercion::RefToTraitValue,
            ]
            .iter()
            .any(|c| kind == Some(*c as u32))
            {
                return false;
            }
            let value = Ref(b.data[i][0]);
            let source = match value.as_inst() {
                Some(inst) => b.ty.get(inst.0 as usize).copied().unwrap_or(Ty::POISON),
                None => b
                    .consts
                    .get((value.0 & !Ref::CONST_BIT) as usize)
                    .map_or(Ty::POISON, |c| c.0),
            };
            // A materialized trait value already has its data words;
            // widening only selects a different vtable path.
            if cx.trait_value(source) {
                return true;
            }
            let mut target = source;
            while let Some(inner) = cx.reference_target(target) {
                if cx.trait_value(inner) {
                    // Widening through references keeps the trait's data,
                    // including when reaching it needs loads first. Erasing
                    // the reference's identity into a box is a separate kind.
                    return cx.trait_reference_widens(target, b.ty[i]);
                }
                target = inner;
            }
            cx.single_counted_address(source)
        }
        // Lookup copies the caller-held provider into a result of its own.
        Some(Tag::ProviderGet) => cx.counted(b.ty[i]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Providers, TirBuilder, TirSink, verify_owned};
    use crate::own::{OwnScratch, Pass, consumers};
    use hd_base::{DefId, NodeIdx};
    use hd_types::TyList;

    struct Facts;

    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> hd_base::Symbol {
            hd_base::Symbol::NONE
        }

        fn counted(&self, ty: Ty) -> bool {
            ty == Ty::STRING || ty == Ty(43)
        }
        fn single_counted_address(&self, ty: Ty) -> bool {
            matches!(ty, Ty(42 | 44))
        }
        fn reference_target(&self, ty: Ty) -> Option<Ty> {
            match ty {
                Ty(44 | 46) => Some(Ty(43)),
                Ty(45) => Some(Ty(46)),
                _ => None,
            }
        }
        fn trait_reference_widens(&self, from: Ty, to: Ty) -> bool {
            from == Ty(46) && to == Ty(43)
        }
        fn trait_value(&self, ty: Ty) -> bool {
            ty == Ty(43)
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
        fn retains_result(&self, c: &Callee) -> bool {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(7))
        }
    }

    fn body() -> Body {
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = b.open_block();
        let text = b.const_str("abc");
        let zero = b.const_value(Ty::I32, 0);
        let value = b.call(
            &Callee::Item {
                def: DefId::from_raw(7),
                targs: TyList::EMPTY,
            },
            &[text, zero, zero],
            Providers::None,
            Ty::STRING,
            syn,
        );
        let root = b.close_block(block, Some(value), Ty::STRING, syn);
        b.finish(root, &[]).expect("slice body")
    }

    #[test]
    fn direct_trait_results_count_but_fresh_boxes_and_indirect_loads_do_not() {
        for (kind, source, counted) in [
            (Coercion::ToTraitValue, Ty(42), true),
            (Coercion::RefToTraitValue, Ty(42), true),
            (Coercion::Supertrait, Ty(43), true),
            (Coercion::ToTraitValue, Ty(43), true),
            (Coercion::ToTraitValue, Ty(44), false),
            (Coercion::RefToTraitValue, Ty(46), true),
            (Coercion::RefToTraitValue, Ty(45), true),
            (Coercion::ToTraitValue, Ty::I32, false),
            (Coercion::RefAsProvider, Ty(43), true),
        ] {
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = builder.open_block();
            let value = builder.const_value(source, 0);
            let value = builder.coerce(kind, NONE, value, Ty(43), NodeIdx::NONE);
            let root = builder.close_block(block, Some(value), Ty(43), NodeIdx::NONE);
            let mut body = builder.finish(root, &[]).unwrap();
            let topo = Topology::new(&body);
            run(&mut body, &Facts, &topo, &mut splice::Tables::default()).unwrap();
            assert_eq!(
                body.tags.contains(&Tag::Retain),
                counted,
                "{kind:?} {source:?}"
            );
            assert!(verify_owned(&body, &Facts).is_empty());
        }
    }

    #[test]
    fn the_count_statement_preserves_the_results_single_consumer() {
        let mut b = body();
        let mut out = Vec::new();
        consumers(&b, &Topology::new(&b), &Facts, &mut out);
        let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
        assert_eq!(out[call], b.sub_root[0]);
        crate::own::run(&mut b, &Facts, &mut OwnScratch::default()).unwrap();
        consumers(&b, &Topology::new(&b), &Facts, &mut out);
        let call = b.tags.iter().position(|&t| t == Tag::Call).unwrap();
        assert_eq!(out[call], b.sub_root[0]);
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 1);
        let topo = Topology::new(&b);
        run(&mut b, &Facts, &topo, &mut splice::Tables::default()).unwrap();
        assert_eq!(b.tags.iter().filter(|&&t| t == Tag::Retain).count(), 1);
        assert!(verify_owned(&b, &Facts).is_empty());
        let text = crate::ir::print(&b);
        assert_eq!(crate::ir::print(&crate::ir::parse(&text).unwrap()), text);
    }

    #[test]
    fn the_verifier_rejects_a_missing_or_mislabeled_result_count() {
        let mut b = body();
        crate::own::run(&mut b, &Facts, &mut OwnScratch::default()).unwrap();
        let retain = b.tags.iter().position(|&t| t == Tag::Retain).unwrap();
        b.extra[b.data[retain][1] as usize + 4] = 2;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("borrowed-result proof"))
        );
        b.extra[b.data[retain][1] as usize + 4] = 0;
        let root = b.sub_root[0] as usize;
        let list = b.data[root][0] as usize;
        assert_eq!(b.extra[list + b.extra[list] as usize], u32_of(retain));
        b.extra[list] -= 1;
        assert!(
            verify_owned(&b, &Facts)
                .iter()
                .any(|e| e.what.contains("lacks its stored retain"))
        );
    }
}
