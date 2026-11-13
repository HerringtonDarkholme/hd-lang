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
    let returned = returned_ends(b, cx, topo, Tag::Drop);
    for (i, &retained) in results.iter().enumerate() {
        if !retained {
            continue;
        }
        let block = topo.blk[i];
        if block == NONE {
            return Err("a retained borrowed result outside a block".into());
        }
        if let Some(drop) = returned[i].or_else(|| handed(b, cx, topo, &results, i, Tag::Drop)) {
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

/// Cancel a read-only returned loan only when its whole source parameter
/// stays unchanged and unaddressed for the entire frame. Index local and
/// SSA uses once: O(instructions + locals + operand edges), with no body
/// scan or lifetime search per call. Other sources take an explicit count.
pub(crate) fn returned_ends(
    b: &Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    end: Tag,
) -> Vec<Option<u32>> {
    returned_ends_with(b, cx, topo, end, || {})
}

fn returned_ends_with(
    b: &Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    end: Tag,
    mut visit: impl FnMut(),
) -> Vec<Option<u32>> {
    #[derive(Default)]
    struct Local {
        invalid: bool,
        donor_invalid: bool,
        stores: usize,
        ends: usize,
        end: Option<u32>,
        source: Option<u32>,
    }
    let mut candidates = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        visit();
        if tag == Tag::Call
            && let Some(callee) = Callee::from_words(b.record(b.data[i][0]))
            && let Some(k) = cx.returned_loan(&callee)
        {
            candidates.push((i, usize::from(k)));
        }
    }
    let mut answer = vec![None; b.len()];
    if candidates.is_empty() || b.tags.iter().copied().any(crate::ir::is_suspension) {
        return answer;
    }
    let mut locals: Vec<Local> = (0..b.local_ty.len()).map(|_| Local::default()).collect();
    for l in &b.cap_local {
        locals[l.idx()].invalid = true;
        locals[l.idx()].donor_invalid = true;
    }
    let mut invalid = vec![false; b.len()];
    let mut ends = vec![None; b.len()];
    let mut stores = vec![None; b.len()];
    let mut ops = Vec::new();
    let mut heap_changed = false;
    let mut breaks = vec![0usize; b.label_inst.len()];
    for i in 0..b.len() {
        visit();
        let tag = b.tags[i];
        if tag == Tag::Break {
            breaks[b.data[i][0] as usize] += 1;
        }
        heap_changed |= match tag {
            Tag::Call => {
                Callee::from_words(b.record(b.data[i][0])).is_none_or(|c| cx.may_write(&c))
            }
            Tag::CallValue
            | Tag::CallHost
            | Tag::CallDyn
            | Tag::DefaultCall
            | Tag::Hook
            | Tag::ProviderGet
            | Tag::With
            | Tag::ContextNew
            | Tag::ContextFor => true,
            Tag::Intrinsic => {
                matches!(
                    crate::ir::IntrinsicOp::from_u32(b.data[i][0]),
                    None | Some(crate::ir::IntrinsicOp::Item)
                ) || super::changed(b, i).next().is_some()
            }
            Tag::Ref if b.data[i][1] & ref_word::MUT != 0 => {
                let p = b.view(b.data[i][0]);
                !matches!(p.root, Root::Local(_) | Root::Value(_))
                    || p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
            }
            Tag::Store | Tag::Drop | Tag::LoanEnd | Tag::IsUnique => {
                let p = b.view(b.data[i][0]);
                !matches!(p.root, Root::Local(_) | Root::Value(_))
                    || p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
            }
            Tag::Read if b.data[i][1] == ReadMode::Move as u32 => {
                let p = b.view(b.data[i][0]);
                p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_)))
            }
            _ => false,
        };
        if tag.operands().0 == crate::ir::Op::Place {
            let p = b.view(b.data[i][0]);
            if let Root::Local(l) = p.root {
                let info = &mut locals[l as usize];
                match tag {
                    Tag::Read if b.is_borrow(u32_of(i)) || !cx.counted(b.ty[i]) => {}
                    Tag::Store if p.whole() => {
                        info.stores += 1;
                        info.source = Some(b.data[i][1]);
                    }
                    t if t == end && p.whole() => {
                        info.ends += 1;
                        info.end = Some(u32_of(i));
                    }
                    _ => {
                        info.invalid = true;
                        info.donor_invalid |= tag != Tag::Read;
                    }
                }
            }
        }
        super::operands(b, i, &mut ops);
        for op in &ops {
            visit();
            let v = op.value as usize;
            if tag == end && whole_value(b, u32_of(i), op.value) {
                if ends[v].replace(u32_of(i)).is_some() {
                    invalid[v] = true;
                }
            } else if tag == Tag::Store && b.data[i][1] == op.value {
                let local = b
                    .view(b.data[i][0])
                    .local()
                    .filter(|_| b.view(b.data[i][0]).whole());
                if local.is_none() || stores[v].replace(local).is_some() {
                    invalid[v] = true;
                }
            } else if (tag != Tag::Retain && !returned_lending_use(b, cx, i, op))
                || matches!(tag, Tag::Ref | Tag::Closure)
            {
                invalid[v] = true;
            }
        }
    }
    for &(i, k) in &candidates {
        visit();
        let Some(&arg) = b.record(b.data[i][1]).get(k) else {
            continue;
        };
        let Some(source) = Ref(arg).as_inst().map(|r| r.0 as usize) else {
            continue;
        };
        if b.tags[source] != Tag::Read || !b.is_borrow(arg) {
            continue;
        }
        let p = b.view(b.data[source][0]);
        let Some(l) = p.local() else { continue };
        // Inline fields cannot change without addressing their holder.
        // Heap projections additionally need an effect-free frame: calls
        // and writes through another alias must not invalidate the loan.
        if heap_changed && p.steps(b).any(|s| matches!(s, Proj::Deref | Proj::Elem(_))) {
            continue;
        }
        let info = &locals[l as usize];
        if b.local_flags[l as usize] & local_flags::PARAM == 0
            || info.invalid
            || info.stores != 0
            || info.ends != 0
            || invalid[i]
        {
            continue;
        }
        answer[i] = match stores[i] {
            Some(Some(l)) if ends[i].is_none() => {
                let info = &locals[l as usize];
                (!info.invalid && info.stores == 1 && info.ends == 1)
                    .then_some(info.end)
                    .flatten()
            }
            None => ends[i],
            _ => None,
        };
    }
    // A temporary's existing count can become the result's. Calls are in
    // SSA definition order, so an earlier returned call either owns its
    // result (a retain or handoff), or is a canceled read-only loan.
    let mut owned: Vec<_> = b
        .tags
        .iter()
        .map(|&t| t == Tag::Call || t == Tag::CallValue)
        .collect();
    for (i, k) in candidates {
        visit();
        if answer[i].is_some() {
            owned[i] = false;
            continue;
        }
        if let Some(next) = next_after(b, topo, i)
            && b.tags[next as usize] == end
            && whole_value(b, next, u32_of(i))
        {
            answer[i] = Some(next);
            owned[i] = false;
            continue;
        }
        let Some(&arg) = b.record(b.data[i][1]).get(k) else {
            continue;
        };
        let Some(next) = next_after(b, topo, i) else {
            continue;
        };
        if b.tags[next as usize] != end {
            continue;
        }
        let drop = b.view(b.data[next as usize][0]);
        let mut source = arg;
        let donor = if whole_value(b, next, arg) {
            true
        } else if let Some(r) = Ref(arg).as_inst() {
            if b.tags[r.0 as usize] == Tag::Read {
                let read = b.view(b.data[r.0 as usize][0]);
                read.whole() && drop.whole() && read.root == drop.root
            } else {
                false
            }
        } else {
            false
        };
        if !donor {
            continue;
        }
        for _ in 0..16 {
            visit();
            let Some(r) = Ref(source).as_inst() else {
                break;
            };
            let x = r.0 as usize;
            let owns = match b.tags[x] {
                Tag::Call | Tag::CallValue => owned[x],
                // A fresh closure owns its environment; an empty or
                // frame-local environment has no physical count to move.
                Tag::Closure => true,
                Tag::Ref => b.data[x][1] & ref_word::TAKES != 0,
                _ => false,
            };
            if owns {
                answer[i] = Some(next);
                break;
            }
            if b.same_value_coercion(x) {
                source = b.data[x][0];
                continue;
            }
            if let Some(value) = b.wrapper_value(x) {
                let block = b.data[x][0] as usize;
                if let Some(&last) = b.record(b.data[block][0]).last()
                    && breaks[b.data[last as usize][0] as usize] == 1
                {
                    source = value;
                    continue;
                }
            }
            if b.tags[x] == Tag::Read {
                let p = b.view(b.data[x][0]);
                if p.whole()
                    && drop.whole()
                    && p.root == drop.root
                    && let Root::Value(value) = p.root
                {
                    source = value;
                    continue;
                }
                if let Some(l) = p.local().filter(|_| p.whole())
                    && (b.data[x][1] == ReadMode::Move as u32
                        || drop.whole() && drop.root == p.root)
                {
                    let info = &locals[l as usize];
                    if b.local_flags[l as usize] & local_flags::PARAM == 0
                        && !info.donor_invalid
                        && info.stores == 1
                        && let Some(value) = info.source
                    {
                        source = value;
                        continue;
                    }
                }
            }
            break;
        }
    }
    answer
}

// Constant work per operand, including calls with many arguments. An
// owned formal rejects the proof even when a default might lend it; that
// narrower fallback avoids rescanning the argument list for each operand.
fn returned_lending_use(b: &Body, cx: &dyn OwnCx, i: usize, op: &super::Operand) -> bool {
    match op.role {
        super::Role::Lend | super::Role::Dropped => true,
        super::Role::Arg(k) if b.tags[i] == Tag::Call => Callee::from_words(b.record(b.data[i][0]))
            .is_some_and(|c| {
                cx.pass(
                    &c,
                    k as usize,
                    b.record(b.data[i][1]).len().saturating_sub(3),
                ) == super::Pass::Lent
            }),
        _ => false,
    }
}

/// The statement of tag `end` (`Drop`, or `LoanEnd` once [`run`] has made it
/// one) that ends the one count result `i` would take, so neither runs: the
/// source's right after a coercion that keeps the source's address, whose
/// count becomes the result's; or, for a provider, its reference's, read only
/// through, since the body's caller holds the provider for the whole body.
pub(crate) fn handed(
    b: &Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    results: &[bool],
    i: usize,
    end: Tag,
) -> Option<u32> {
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
        return (ends(next, source.0) && owns_its_count(b, cx, results, source.0, 8))
            .then_some(next);
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
fn owns_its_count(b: &Body, cx: &dyn OwnCx, results: &[bool], v: u32, depth: u32) -> bool {
    let i = v as usize;
    if depth == 0 || Ref(v).as_inst().is_none() {
        return false;
    }
    match b.tags[i] {
        // A physical reference copied from a frame cell has its own count
        // from this pass's result retain. A Ref never hands off its source.
        Tag::Ref => b.data[i][1] & ref_word::TAKES != 0 || results[i],
        Tag::Call => Callee::from_words(b.record(b.data[i][0]))
            .is_none_or(|c| cx.returned_loan(&c).is_none()),
        Tag::CallValue => true,
        // A result that takes a count, stated or its source's.
        Tag::Coerce if result(b, cx, i) => true,
        Tag::Coerce => {
            b.same_value_coercion(i) && owns_its_count(b, cx, results, b.data[i][0], depth - 1)
        }
        Tag::RefDyn => owns_its_count(b, cx, results, b.data[i][0], depth - 1),
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
                && (tail == NONE || owns_its_count(b, cx, results, tail, depth - 1))
                && breaks
                    .iter()
                    .all(|&x| owns_its_count(b, cx, results, x, depth - 1))
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
                    owns_its_count(b, cx, results, b.data[x][1], depth - 1)
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

/// The `ProviderAsRef` coercion that makes `ProviderGet` `i` a `&dyn`, a
/// local reference: the checker emits it right after the lookup
/// (`hd_check`'s `provider_get`), and a few statements at most come between.
/// `plain` says whether a type is a local reference.
pub fn provider_view(b: &Body, i: usize, plain: impl Fn(Ty) -> bool) -> Option<usize> {
    (i + 1..b.len().min(i + 5))
        .find(|&c| b.provider_ref(c) && b.data[c][0] == u32_of(i) && plain(b.ty[c]))
}

pub(crate) fn result(b: &Body, cx: &dyn OwnCx, i: usize) -> bool {
    match b.tags.get(i) {
        Some(Tag::Call) => {
            Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| cx.retains_result(&c))
        }
        Some(Tag::Coerce) if cx.counted(b.ty[i]) => {
            let kind = b.record(b.data[i][1]).first().copied();
            if kind == Some(Coercion::RefAsProvider as u32) {
                return true;
            }
            if ![Coercion::Supertrait, Coercion::RefToTraitValue]
                .iter()
                .any(|c| kind == Some(*c as u32))
                || super::dyn_view(b, cx, i).is_some()
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
        // Lookup copies the caller-held provider into a result of its own,
        // except for the `&dyn` a `$.use` makes of it: a local reference,
        // which counts nothing (`ref.count.local`), while the caller holds
        // the provider for the whole body.
        Some(Tag::ProviderGet) => {
            cx.counted(b.ty[i]) && provider_view(b, i, |t| cx.plain_reference(t)).is_none()
        }
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
        fn returned_loan(&self, c: &Callee) -> Option<u8> {
            matches!(c, Callee::Item { def, .. } if *def == DefId::from_raw(8)).then_some(0)
        }
    }

    #[test]
    fn returned_loan_cancellation_requires_a_stable_whole_parameter() -> Result<(), String> {
        for changed in [false, true] {
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let node = NodeIdx::from_raw(1);
            let block = builder.open_block();
            let param = builder.local(Ty::STRING, hd_base::Symbol::NONE, local_flags::PARAM, node);
            let value = builder.get(param, Ty::STRING, node);
            let call = builder.call(
                &Callee::Item {
                    def: DefId::from_raw(8),
                    targs: TyList::EMPTY,
                },
                &[value],
                Providers::None,
                Ty::STRING,
                node,
            );
            if changed {
                let replacement = builder.const_str("replacement");
                builder.set(param, replacement, node);
            }
            builder.call(
                &Callee::Item {
                    def: DefId::from_raw(9),
                    targs: TyList::EMPTY,
                },
                &[call],
                Providers::None,
                Ty::VOID,
                node,
            );
            let place = builder.place(&crate::ir::Place {
                root: crate::ir::PlaceRoot::Value(call),
                proj: vec![],
            });
            let drop = builder.call(
                &Callee::Item {
                    def: DefId::from_raw(9),
                    targs: TyList::EMPTY,
                },
                &[call],
                Providers::None,
                Ty::VOID,
                node,
            );
            let root = builder.close_block(block, None, Ty::VOID, node);
            let mut b = builder.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            b.tags[drop.0 as usize] = Tag::Drop;
            b.data[drop.0 as usize] = [place, NONE];
            b.data[value.0 as usize][1] = ReadMode::Borrow as u32;
            let answer = returned_ends(&b, &Facts, &Topology::new(&b), Tag::Drop);
            assert_eq!(answer[call.0 as usize], (!changed).then_some(drop.0));
            // A canceled call cannot donate a count to another coercion.
            let mut results = vec![false; b.len()];
            results[call.0 as usize] = true;
            assert!(!owns_its_count(&b, &Facts, &results, call.0, 8));
        }
        Ok(())
    }

    #[test]
    fn returned_loan_handoff_work_is_bounded_at_width_1000_depth_20() -> Result<(), String> {
        for width in [1, 1000] {
            for depth in [1, 20] {
                let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
                let node = NodeIdx::from_raw(1);
                let block = builder.open_block();
                let mut cases = Vec::new();
                for _ in 0..width {
                    let sink = Callee::Item {
                        def: DefId::from_raw(9),
                        targs: TyList::EMPTY,
                    };
                    let mut value = builder.call(&sink, &[], Providers::None, Ty::STRING, node);
                    for _ in 0..depth {
                        value = builder.coerce(Coercion::Identity, NONE, value, Ty::STRING, node);
                    }
                    let call = builder.call(
                        &Callee::Item {
                            def: DefId::from_raw(8),
                            targs: TyList::EMPTY,
                        },
                        &[value],
                        Providers::None,
                        Ty::STRING,
                        node,
                    );
                    let place = builder.place(&crate::ir::Place {
                        root: crate::ir::PlaceRoot::Value(value),
                        proj: vec![],
                    });
                    // Finish checks raw TIR first; the test then supplies the
                    // post-insertion end in this statement's existing slot.
                    let drop = builder.call(&sink, &[value], Providers::None, Ty::VOID, node);
                    builder.call(&sink, &[call], Providers::None, Ty::VOID, node);
                    cases.push((call.0, drop.0, place));
                }
                let root = builder.close_block(block, None, Ty::VOID, node);
                let mut b = builder.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
                for &(_, drop, place) in &cases {
                    b.tags[drop as usize] = Tag::Drop;
                    b.data[drop as usize] = [place, NONE];
                }
                let mut work = 0;
                let answer =
                    returned_ends_with(&b, &Facts, &Topology::new(&b), Tag::Drop, || work += 1);
                for (call, drop, _) in cases {
                    assert_eq!(answer[call as usize], (depth < 16).then_some(drop));
                }
                // Two visits per instruction, depth+3 operand edges per
                // chain, two candidate visits and at most 16 donor steps.
                assert_eq!(
                    work,
                    2 * b.len() + (depth + 5 + (depth + 1).min(16)) * width
                );
            }
        }
        Ok(())
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
            (Coercion::RefToTraitValue, Ty(42), true),
            (Coercion::Supertrait, Ty(43), true),
            (Coercion::RefToTraitValue, Ty(44), false),
            (Coercion::RefToTraitValue, Ty(46), true),
            (Coercion::RefToTraitValue, Ty(45), true),
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
