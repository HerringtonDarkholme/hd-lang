//! The batched next-use and write queries against the plain walks they
//! replace: per copy read, a scan for a later use of its root and a scan of
//! its live range for a write. The walks are the reference; they cost
//! reads times body size, so only tests run them.

use super::*;
use crate::ir::{BodyKind, Callee, Proj, Providers, ReadMode, TirBuilder, TirSink, local_flags};
use crate::own::OwnCx;
use crate::own::reason::read_start;
use crate::own::values::{Level, after};
use hd_base::{DefId, LocalId, NodeIdx, Symbol};
use hd_types::{Ty, TyList};

fn branch(b: &mut TirBuilder, locals: &[LocalId], seed: u32, depth: usize) {
    let syn = NodeIdx::from_raw(1);
    b.get(locals[seed as usize % locals.len()], Ty::STRING, syn);
    if depth == 0 {
        if seed.is_multiple_of(3) {
            b.emit(Tag::Return, NONE, NONE, Ty::NEVER, syn);
        }
        return;
    }
    let cond = b.call(
        &Callee::Item {
            def: DefId::from_raw(1),
            targs: TyList::EMPTY,
        },
        &[],
        Providers::None,
        Ty::BOOL,
        syn,
    );
    let looping = b.open_loop();
    let body = b.open_block();
    let then = b.open_block();
    branch(b, locals, seed.wrapping_mul(5).wrapping_add(1), depth - 1);
    let then = b.close_block(then, None, Ty::VOID, syn);
    let other = b.open_block();
    branch(b, locals, seed.wrapping_mul(7).wrapping_add(3), depth - 1);
    if seed.is_multiple_of(2) {
        b.emit(Tag::Break, looping.0.raw(), NONE, Ty::NEVER, syn);
    } else {
        b.emit(Tag::Continue, looping.0.raw(), NONE, Ty::NEVER, syn);
    }
    let other = b.close_block(other, None, Ty::VOID, syn);
    let arms = b.refs_record(&[then, other]);
    b.emit(Tag::If, cond.0, arms, Ty::VOID, syn);
    let read = b.get(locals[0], Ty::STRING, syn);
    if seed.is_multiple_of(4) {
        b.set(locals[0], read, syn);
    }
    let body = b.close_block(body, None, Ty::VOID, syn);
    b.close_loop(looping, body, Ty::VOID, syn);
    b.get(locals[1], Ty::STRING, syn);
}

#[test]
fn a_wide_body_answers_all_next_uses_together() {
    let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
    let syn = NodeIdx::from_raw(1);
    let block = builder.open_block();
    let local = builder.local(Ty::STRING, Symbol::from_raw(0), local_flags::PARAM, syn);
    for _ in 0..20_000 {
        builder.get(local, Ty::STRING, syn);
    }
    let root = builder.close_block(block, None, Ty::VOID, syn);
    let mut body = builder.finish(root, &[]).expect("TIR");
    for i in 0..20_000 {
        body.data[i][1] = ReadMode::Copy as u32;
    }
    let actual = uses(&body, &Topology::new(&body));
    assert!(actual[..19_999].iter().all(|&used| used));
    assert!(!actual[19_999]);
}

#[test]
fn batched_queries_match_the_original_walks() {
    for seed in 0..24 {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let syn = NodeIdx::from_raw(1);
        let block = builder.open_block();
        let locals = [0, 1]
            .map(|name| builder.local(Ty::STRING, Symbol::from_raw(name), local_flags::PARAM, syn));
        branch(&mut builder, &locals, seed, 3);
        let root = builder.close_block(block, None, Ty::VOID, syn);
        let mut body = builder.finish(root, &[]).expect("structural TIR");
        for i in 0..body.len() {
            if body.tags[i] == Tag::Read {
                body.data[i][1] = ReadMode::Copy as u32;
            }
        }
        let topo = Topology::new(&body);
        let actual = uses(&body, &topo);
        let last: Vec<_> = (0..body.len())
            .map(|i| u32_of((i + seed as usize % 11).min(body.len() - 1)))
            .collect();
        let changed = crate::own::reason::changes(&body, &topo, &crate::own::tests::Cx, &last);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for i in 0..body.len() {
            if body.tags[i] != Tag::Read {
                continue;
            }
            let src = source(&body, body.data[i][0]);
            assert_eq!(
                actual[i],
                used_again(&body, &topo, i, src.root, &mut left, &mut right),
                "seed {seed}, read {i}"
            );
            assert_eq!(
                changed[i],
                changed_while(&body, &topo, &crate::own::tests::Cx, i, last[i], src),
                "write query seed {seed}, read {i}"
            );
        }
    }
}

/// Whether root `root` of copy read `r` is used again after it: another
/// read, reference or capture of the same local (or use of the same
/// value) that may run after `r`, or one in a loop around `r` that holds
/// no whole store of the local.
fn used_again(
    b: &Body,
    topo: &Topology,
    r: usize,
    root: Option<Root>,
    cu: &mut Vec<Level>,
    cw: &mut Vec<Level>,
) -> bool {
    let touches = |i: usize| -> bool {
        match root {
            Some(Root::Local(l)) => match b.tags[i] {
                Tag::Read | Tag::Ref | Tag::IsUnique => b.view(b.data[i][0]).local() == Some(l),
                Tag::Closure => {
                    let rec = b.record(b.data[i][1]);
                    (rec[0]..rec[0] + rec[1]).any(|c| b.cap_local[c as usize].raw() == l)
                }
                _ => false,
            },
            Some(Root::Value(v)) => {
                let mut hit = false;
                crate::ir::slots(b, i, |_, x, s| {
                    hit |= x == v && matches!(s, crate::ir::Word::Value(_));
                });
                hit && b.tags[i] != Tag::Drop
            }
            _ => false,
        }
    };
    let whole_store = |i: usize| {
        b.tags[i] == Tag::Store
            && matches!(root, Some(Root::Local(l)) if {
                let p = b.view(b.data[i][0]);
                p.local() == Some(l) && p.whole()
            })
    };
    statement_chain(topo, u32_of(r), cu);
    let inner = topo.loop_of[r];
    let holds = |l: u32, i: usize| {
        let mut m = topo.loop_of[i];
        while m != NONE {
            if m == l {
                return true;
            }
            m = topo.loop_of[m as usize];
        }
        false
    };
    let renewed = inner != NONE && (0..b.len()).any(|i| whole_store(i) && holds(inner, i));
    (0..b.len()).any(|j| {
        if j == r || !touches(j) {
            return false;
        }
        if inner != NONE && !renewed && holds(inner, j) {
            return true;
        }
        statement_chain(topo, u32_of(j), cw);
        after(b, cu, cw)
    })
}

/// Whether a write may change the source of copy read `r` while its value
/// lives, up to its last use `end` ([`copy_reasons`]), over the range of
/// instructions from the first of the read's statement.
fn changed_while(
    b: &Body,
    topo: &Topology,
    cx: &dyn OwnCx,
    r: usize,
    end: u32,
    src: Source,
) -> bool {
    if end == NONE {
        return false;
    }
    let lo = read_start(b, topo, r) as usize;
    let local = match src.root {
        Some(Root::Local(l)) => Some(l),
        _ => None,
    };
    (lo..=end as usize).any(|i| {
        if i == r {
            return false;
        }
        let names_local = |i: usize| {
            local.is_some() && {
                let p = b.view(b.data[i][0]);
                p.local() == local || matches!(p.root, Root::Local(l) if Some(l) == local)
            }
        };
        match b.tags[i] {
            Tag::Store | Tag::IsUnique => {
                names_local(i)
                    || (src.behind
                        && b.view(b.data[i][0])
                            .steps(b)
                            .any(|s| matches!(s, Proj::Deref | Proj::Elem(_))))
            }
            Tag::Ref => b.data[i][1] & crate::ir::ref_word::MUT != 0 && names_local(i),
            Tag::Call => {
                src.behind
                    && crate::ir::Callee::from_words(b.record(b.data[i][0]))
                        .is_none_or(|c| cx.may_write(&c))
            }
            Tag::CallValue => src.behind,
            Tag::Intrinsic => src.behind && crate::own::changed(b, i).next().is_some(),
            _ => false,
        }
    })
}
