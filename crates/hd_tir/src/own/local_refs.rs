//! Forward fully visible local-reference accesses to physical scalar places.
//! Addresses observed by calls, captures, identity or pointer operations keep
//! their existing storage. Buffer elements and pointer dereferences never enter
//! this proof, so their dynamic loan checks remain intact.
//!
//! This is local scalar replacement after inlining, before storage selection:
//! a direct place preserves every read/write, while address-observing uses
//! prevent replacement. LLVM's SROA uses the same address-use admission:
//! <https://llvm.org/docs/Passes.html#sroa-scalar-replacement-of-aggregates>
//!
//! Cost: a fixed number of linear scans over instructions, locals and operands.
//! Each copied origin has at most `FIELDS` projections; longer paths retain
//! storage. Alias depth adds input instructions, never a recursive walk or a
//! retry. Topology rebuilding and splicing use their existing linear passes.

use super::{Operand, OwnCx, splice};
use crate::ir::{
    Body, Op, Place, PlaceRoot, Proj, Ref, Root, Tag, Topology, local_flags, place, ref_word,
    u32_of,
};
use hd_base::LocalId;

/// Bounds copied projection work independently of alias-chain depth.
const FIELDS: usize = 64;

#[derive(Clone)]
struct Origin {
    local: u32,
    fields: Vec<Proj>,
}

fn copy(origin: Option<&Origin>, measure: &mut impl FnMut(usize)) -> Option<Origin> {
    origin.map(|origin| {
        measure(1 + origin.fields.len());
        origin.clone()
    })
}

fn value(origins: &[Option<Origin>], word: u32, measure: &mut impl FnMut(usize)) -> Option<Origin> {
    copy(
        Ref(word)
            .as_inst()
            .and_then(|v| origins[v.0 as usize].as_ref()),
        measure,
    )
}

fn through(
    b: &Body,
    word: u32,
    values: &[Option<Origin>],
    locals: &[Option<Origin>],
    measure: &mut impl FnMut(usize),
) -> Option<Origin> {
    let p = b.view(word);
    let mut origin = match p.root {
        Root::Local(l) => copy(locals[l as usize].as_ref(), measure),
        Root::Value(v) => value(values, v, measure),
        _ => None,
    }?;
    let mut steps = p.steps(b);
    if steps.next() != Some(Proj::Deref) {
        return None;
    }
    for step in steps {
        measure(1);
        if !matches!(step, Proj::Field(_)) || origin.fields.len() == FIELDS {
            return None;
        }
        origin.fields.push(step);
    }
    Some(origin)
}

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &mut Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    forward(b, cx, topo, edits, |_| {})
}

fn forward(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &mut Topology,
    edits: &mut splice::Tables,
    mut measure: impl FnMut(usize),
) -> Result<(), String> {
    if !cx.summarizes() || cx.suspending(b.item) || !b.tags.contains(&Tag::Ref) {
        return Ok(());
    }
    if b.tags
        .iter()
        .enumerate()
        .any(|(i, &tag)| tag == Tag::Closure && cx.function_suspends(b.ty[i]))
    {
        return Ok(());
    }
    let n = b.len();
    // Operand enumeration also visits element-index operands. Bound its
    // shared-place work before scanning any path; budget exhaustion keeps
    // the original storage and dynamic checks, as alias exhaustion does.
    for i in 0..n {
        measure(1);
        if b.tags[i].operands().0 == Op::Place {
            let p = b.view(b.data[i][0]);
            if p.to - p.from > u32_of(2 * FIELDS) {
                return Ok(());
            }
        }
    }
    measure(n + b.local_ty.len() + b.cap_local.len());
    let mut eligible: Vec<_> = b
        .local_ty
        .iter()
        .map(|&ty| !cx.counted(ty) && !cx.holds_ref(ty) && cx.holds_drop(ty) == super::Tri::No)
        .collect();
    for l in &b.cap_local {
        eligible[l.idx()] = false;
    }
    let mut stores = vec![0usize; b.local_ty.len()];
    for i in 0..n {
        measure(1);
        if b.tags[i] == Tag::Store {
            let p = b.view(b.data[i][0]);
            if p.whole()
                && let Root::Local(l) = p.root
            {
                stores[l as usize] += 1;
            }
        }
    }
    let mut values = vec![None; n];
    let mut locals = vec![None; b.local_ty.len()];
    let mut rewritten = vec![None; n];
    for i in 0..n {
        measure(1);
        let [a, w] = b.data[i];
        if matches!(b.tags[i], Tag::Read | Tag::Store | Tag::Ref) {
            rewritten[i] = through(b, a, &values, &locals, &mut measure);
        }
        if b.tags[i] == Tag::Store {
            let p = b.view(a);
            if p.whole()
                && let Root::Local(l) = p.root
                && stores[l as usize] == 1
                && b.local_flags[l as usize] & local_flags::PARAM == 0
            {
                locals[l as usize] = value(&values, w, &mut measure);
            }
        }
        if !cx.plain_reference(b.ty[i]) {
            continue;
        }
        values[i] = match b.tags[i] {
            Tag::Ref if w & ref_word::LOCAL != 0 && w & ref_word::TAKES == 0 => {
                if let Some(origin) = &rewritten[i] {
                    copy(Some(origin), &mut measure)
                } else {
                    let p = b.view(a);
                    if let Root::Local(l) = p.root
                        && eligible[l as usize]
                    {
                        let fields: Vec<_> = p
                            .steps(b)
                            .take(FIELDS + 1)
                            .inspect(|_| measure(1))
                            .collect();
                        (fields.len() <= FIELDS
                            && fields.iter().all(|p| matches!(p, Proj::Field(_))))
                        .then_some(Origin { local: l, fields })
                    } else {
                        None
                    }
                }
            }
            Tag::Read => {
                let p = b.view(a);
                if p.whole()
                    && let Root::Local(l) = p.root
                {
                    copy(locals[l as usize].as_ref(), &mut measure)
                } else {
                    None
                }
            }
            Tag::Coerce if b.same_value_coercion(i) && !b.provider_ref(i) => {
                value(&values, a, &mut measure)
            }
            _ => None,
        };
    }
    let mut bad = vec![false; b.local_ty.len()];
    for l in &b.cap_local {
        if let Some(origin) = &locals[l.idx()] {
            bad[origin.local as usize] = true;
        }
    }
    let mut ops: Vec<Operand> = Vec::new();
    for i in 0..n {
        measure(1);
        if b.tags[i] == Tag::Ref
            && values[i].is_none()
            && let Root::Local(l) = b.view(b.data[i][0]).root
        {
            bad[l as usize] = true;
        }
        super::operands(b, i, &mut ops);
        for op in &ops {
            measure(1);
            let Some(origin) = &values[op.value as usize] else {
                continue;
            };
            let allowed = match b.tags[i] {
                Tag::Read | Tag::Ref => rewritten[i].is_some() || values[i].is_some(),
                Tag::Store if op.value == b.data[i][1] => {
                    let p = b.view(b.data[i][0]);
                    p.whole()
                        && matches!(p.root, Root::Local(l) if stores[l as usize] == 1 && locals[l as usize].is_some())
                }
                Tag::Store => rewritten[i].is_some(),
                Tag::Coerce => values[i].is_some(),
                Tag::LoanEnd => b.view(b.data[i][0]).whole(),
                _ => false,
            };
            if !allowed {
                bad[origin.local as usize] = true;
            }
        }
        if let Root::Local(l) = if b.tags[i].operands().0 == Op::Place {
            b.view(b.data[i][0]).root
        } else {
            Root::Other
        } && let Some(origin) = &locals[l as usize]
        {
            let p = b.view(b.data[i][0]);
            let allowed = match b.tags[i] {
                Tag::Read | Tag::Store => p.whole() || rewritten[i].is_some(),
                Tag::Ref => values[i].is_some(),
                Tag::LoanEnd => p.whole(),
                _ => false,
            };
            if !allowed {
                bad[origin.local as usize] = true;
            }
        }
    }
    topo.build(b);
    edits.clear();
    for i in 0..n {
        measure(1);
        if values[i].as_ref().is_some_and(|o| !bad[o.local as usize]) {
            edits.remove(u32_of(i));
            continue;
        }
        if let Some(origin) = &rewritten[i]
            && !bad[origin.local as usize]
        {
            let p = Place {
                root: PlaceRoot::Local(LocalId::from_raw(origin.local)),
                proj: origin.fields.clone(),
            };
            let word = if let Some(words) = p.words() {
                let at = u32_of(b.extra.len());
                b.extra.push(u32_of(words.len()));
                b.extra.extend(words);
                at
            } else {
                place::BARE_LOCAL | origin.local
            };
            b.data[i][0] = word;
        }
        if matches!(b.tags[i], Tag::Store | Tag::LoanEnd) {
            let p = b.view(b.data[i][0]);
            let origin = match p.root {
                Root::Local(l) if p.whole() => locals[l as usize].as_ref(),
                Root::Value(v) if p.whole() => values[v as usize].as_ref(),
                _ => None,
            };
            if origin.is_some_and(|o| !bad[o.local as usize]) {
                edits.remove(u32_of(i));
            }
        }
    }
    splice::apply(b, topo, edits)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Callee, NONE, Providers, TirBuilder, TirSink, verify_checked};
    use crate::own::{Pass, Tri};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    const SCALAR: Ty = Ty(1_000_000);
    const LOCAL_REF: Ty = Ty(1_000_001);

    struct Facts;
    impl OwnCx for Facts {
        fn temporary_owner_name(&self) -> Symbol {
            Symbol::NONE
        }
        fn summarizes(&self) -> bool {
            true
        }
        fn suspending(&self, _: DefId) -> bool {
            false
        }
        fn function_suspends(&self, _: Ty) -> bool {
            false
        }
        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
        fn counted(&self, _: Ty) -> bool {
            false
        }
        fn holds_ref(&self, ty: Ty) -> bool {
            ty == LOCAL_REF
        }
        fn holds_drop(&self, _: Ty) -> Tri {
            Tri::No
        }
        fn plain_reference(&self, ty: Ty) -> bool {
            ty == LOCAL_REF
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            true
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            false
        }
    }

    fn generated(width: usize, aliases: usize, fields: usize, observed: bool) -> Body {
        let n = NodeIdx::NONE;
        let mut b = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let root = b.open_block();
        for k in 0..width {
            let local = b.local(SCALAR, Symbol::NONE, local_flags::ASSIGNED, n);
            let zero = b.const_value(Ty::I32, 0);
            let parts = b.refs_record(&[zero]);
            let initial = b.emit(Tag::NewStruct, NONE, parts, SCALAR, n);
            b.emit(
                Tag::Store,
                place::BARE_LOCAL | local.raw(),
                initial.0,
                Ty::VOID,
                n,
            );
            let p = b.place(&Place {
                root: PlaceRoot::Local(local),
                proj: vec![Proj::Field(0); fields],
            });
            let mut address = b.emit(Tag::Ref, p, ref_word::LOCAL | ref_word::MUT, LOCAL_REF, n);
            for _ in 0..aliases {
                let alias = b.local(LOCAL_REF, Symbol::NONE, 0, n);
                b.emit(
                    Tag::Store,
                    place::BARE_LOCAL | alias.raw(),
                    address.0,
                    Ty::VOID,
                    n,
                );
                address = b.get(alias, LOCAL_REF, n);
            }
            let p = b.place(&Place {
                root: PlaceRoot::Value(address),
                proj: vec![Proj::Deref, Proj::Field(0)],
            });
            let value = b.const_value(Ty::I32, k as u64);
            b.emit(Tag::Store, p, value.0, Ty::VOID, n);
            if observed {
                b.call(
                    &Callee::Item {
                        def: DefId::from_raw(2),
                        targs: hd_types::TyList::EMPTY,
                    },
                    &[address],
                    Providers::None,
                    Ty::VOID,
                    n,
                );
            }
        }
        let root = b.close_block(root, None, Ty::VOID, n);
        b.finish(root, &[]).unwrap()
    }

    #[test]
    fn aliases_are_forwarded_once_at_width_1000_and_depth_20() {
        for depth in [1, 20] {
            let mut previous = 0;
            for width in [1, 100, 1000] {
                let mut b = generated(width, depth, 0, false);
                let input = b.len() + b.extra.len();
                let mut work = 0;
                forward(
                    &mut b,
                    &Facts,
                    &mut Topology::default(),
                    &mut splice::Tables::default(),
                    |count| work += count,
                )
                .unwrap();
                assert!(!b.tags.contains(&Tag::Ref));
                assert!(verify_checked(&b).is_empty());
                assert!(
                    work <= (FIELDS + 16) * input,
                    "{width}/{depth}: {work} for {input}"
                );
                if width == 1000 {
                    assert!(work <= 11 * previous);
                }
                previous = work;
                eprintln!("local-ref work width={width} depth={depth}: {work}");
            }
        }
    }

    #[test]
    fn observed_addresses_and_over_budget_projections_keep_their_storage() {
        for mut b in [
            generated(1, 20, 0, true),
            generated(1, 20, FIELDS, false),
            generated(1, 20, FIELDS + 1, false),
            generated(1000, 20, 1000, false),
        ] {
            let before = b.clone();
            forward(
                &mut b,
                &Facts,
                &mut Topology::default(),
                &mut splice::Tables::default(),
                |_| {},
            )
            .unwrap();
            assert_eq!(b, before);
        }
        let mut b = generated(1, 20, 20, false);
        let mut work = 0;
        let input = b.len() + b.extra.len();
        forward(
            &mut b,
            &Facts,
            &mut Topology::default(),
            &mut splice::Tables::default(),
            |count| work += count,
        )
        .unwrap();
        assert!(!b.tags.contains(&Tag::Ref));
        assert!(work <= (FIELDS + 16) * input);
    }
}
