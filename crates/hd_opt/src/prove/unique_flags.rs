//! A loop's `std.rt.is_unique` tests through one flag (#2369, release).
//!
//! A copy-on-write change tests its block (`is_unique(b)`, a count load
//! and compare) and on a shared or full block replaces it through a cold
//! call that returns a fresh one. After that call, or after a test that
//! found the count 1, the block stays unique until something copies it
//! ([`hd_tir::own::unique`]): the next iteration's test is known. Swift
//! folds the same way (`end_cow_mutation [keep_unique]` after
//! `makeMutableAndUnique`, then no uniqueness check in an `append` loop).
//!
//! Per loop and holder `P` (a local and fields to a `*H`) that the loop
//! tests, a bool local `u` keeps `is_unique(P)`:
//! - before the loop, `u := is_unique(P)`, the test itself;
//! - after a store of a fresh block (`Effect::Fresh`), `u := true`;
//! - after anything that may copy the block or let another holder go
//!   (`Effect::Shares`, `Effect::Releases`), `u := is_unique(P)` again;
//! - each test of `P` in the loop reads `u`.
//!
//! So `u` is the test's answer at every point of the loop, true or false:
//! the count changes only by an instruction that names `P` or runs other
//! code, and each one sets `u` again. A loop that moves or drops `P`
//! (`Effect::Ends`), reaches it by another name, or keeps a borrow of it
//! is left alone, and `P` must hold a block when the loop starts: a store
//! before the loop, or a parameter, and no move or drop of it but after
//! the loop. A push loop's test runs once before the loop and on its cold
//! path; the hot path reads a local.
//!
//! Cost: per body with a loop and a test, one census of uses, then per
//! holder one walk of the body for its effects. O(n × holders).

use super::{Lists, Nest, Sets, Topology, scans};
use hd_tir::ir::{Body, Callee, NONE, ReadMode, Ref, Tag, local_flags, place, u32_of};
use hd_tir::own::unique::{Effect, Holder, Users, effect, reached_otherwise};
use hd_types::Ty;

/// The intrinsics a test passes through: they run no other code.
fn pure_intrinsic(name: &str) -> bool {
    super::loop_loads::PURE.contains(&name)
        || matches!(name, "counted_tail" | "counted_is_unique" | "panic")
}

/// The statement position just after `x` and the counts that go with
/// it: a count stays next to the operation it counts, so a count's flag
/// write follows that operation and the counts after it.
fn after(b: &Body, topo: &Topology, x: u32) -> Option<(u32, u32)> {
    let block = topo.blk[x as usize];
    if block == NONE {
        return None;
    }
    let statements = b.record(b.data[block as usize][0]);
    let mut pos = topo.pos[x as usize] as usize;
    if matches!(b.tags[x as usize], Tag::Retain | Tag::Release)
        && let Some(at) = Ref(b.data[x as usize][0]).as_inst().map(|r| r.0 as usize)
        && topo.blk.get(at) == Some(&block)
    {
        pos = pos.max(topo.pos[at] as usize);
    }
    while statements
        .get(pos + 1)
        .is_some_and(|&s| matches!(b.tags[s as usize], Tag::Retain | Tag::Release))
    {
        pos += 1;
    }
    Some((block, u32_of(pos + 1)))
}

/// A loop, the tests of one holder in it, and the effects on its block
/// in the loop.
type Flagged = (u32, Vec<usize>, Vec<(u32, Effect)>);

/// One test of a holder: the call, and its argument's read and coercion.
struct Test {
    call: u32,
    read: u32,
    coerce: Option<u32>,
}

/// Flags `b`'s loops' uniqueness tests.
///
/// # Errors
/// The splice of the flag's writes failing, a malformed body.
pub(super) fn run(b: &mut Body, cx: &dyn super::Cx) -> Result<(), String> {
    if !b.tags.contains(&Tag::Loop) {
        return Ok(());
    }
    let intrinsic = |b: &Body, i: usize| {
        (b.tags[i] == Tag::Call)
            .then(|| Callee::from_words(b.record(b.data[i][0])))
            .flatten()
            .and_then(|c| cx.intrinsic(&c))
    };
    let mut tests: Vec<(Holder, Test)> = Vec::new();
    for i in 0..b.len() {
        if intrinsic(b, i).as_deref() != Some("counted_is_unique") {
            continue;
        }
        let Some(&arg) = b.call_args(i).first() else {
            continue;
        };
        let Some(mut r) = Ref(arg).as_inst().map(|x| x.0) else {
            continue;
        };
        let mut coerce = None;
        if b.tags[r as usize] == Tag::Coerce {
            coerce = Some(r);
            r = b.data[r as usize][0];
        }
        if (r as usize) < b.len()
            && b.tags[r as usize] == Tag::Read
            && let Some(h) = Holder::of(b, b.data[r as usize][0])
        {
            tests.push((
                h,
                Test {
                    call: u32_of(i),
                    read: r,
                    coerce,
                },
            ));
        }
    }
    if tests.is_empty() {
        return Ok(());
    }
    let topo = Topology::new(b);
    let nest = Nest::new(b, &topo);
    let sets = Sets::new(b);
    let users = Users::new(b);
    scans(2 * b.len());
    let fresh = |c: &Callee| cx.returns_unique(c);
    let pure = |i: usize| intrinsic(b, i).as_deref().is_some_and(pure_intrinsic);
    // (loop, holder's tests, effects in the loop).
    let mut flagged: Vec<Flagged> = Vec::new();
    let mut done = vec![false; tests.len()];
    // A suspension point lets other code run between two tests.
    let mut awaits = vec![false; b.sub_root.len()];
    for i in 0..b.len() {
        if matches!(
            b.tags[i],
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
        ) {
            awaits[topo.sub[i] as usize] = true;
        }
    }
    for l in (0..b.len()).rev().filter(|&l| b.tags[l] == Tag::Loop) {
        let l = u32_of(l);
        let sub = topo.sub[l as usize];
        let cell = |local: usize| {
            b.storage
                .binary_search_by(|&(s, x, _)| (s.idx(), x.idx()).cmp(&(sub as usize, local)))
                .is_ok()
        };
        let lists = Lists {
            b,
            topo: &topo,
            nest: &nest,
            cx,
            cell: &cell,
            sets: &sets,
        };
        if awaits[sub as usize] {
            continue;
        }
        let body = lists.loop_body(l);
        let mut inside = vec![false; b.len()];
        for &x in &body {
            inside[x as usize] = true;
        }
        let mut k = 0;
        while k < tests.len() {
            if done[k] || !inside[tests[k].1.call as usize] {
                k += 1;
                continue;
            }
            let h = tests[k].0.clone();
            let mine: Vec<usize> = (k..tests.len())
                .filter(|&j| !done[j] && tests[j].0 == h && inside[tests[j].1.call as usize])
                .collect();
            for &j in &mine {
                done[j] = true;
            }
            k += 1;
            // The local's flags count addresses taken before inlining;
            // what reaches it now is its `Ref`s and captures.
            if cell(h.local as usize) || reached_otherwise(b, &h) {
                continue;
            }
            // `P` holds a block when the loop starts.
            let param = b.local_flags[h.local as usize] & local_flags::PARAM != 0;
            let mut stored = param;
            let mut effects = Vec::new();
            let mut fit = true;
            for (i, &within) in inside.iter().enumerate() {
                let e = effect(b, &users, &h, i, &fresh, &pure);
                // A borrow kept anywhere may be copied in the loop.
                if e == Some(Effect::Kept) {
                    fit = false;
                    break;
                }
                let x = u32_of(i);
                if within {
                    match e {
                        Some(Effect::Ends) => {
                            fit = false;
                            break;
                        }
                        Some(e) => effects.push((x, e)),
                        None => {}
                    }
                } else if e == Some(Effect::Ends) && !lists.dominates(l, x) {
                    fit = false;
                    break;
                } else if matches!(e, Some(Effect::Fresh | Effect::Shares))
                    && b.tags[i] == Tag::Store
                    && lists.dominates(x, l)
                {
                    stored = true;
                }
            }
            if fit && stored {
                flagged.push((l, mine, effects));
            }
        }
    }
    if flagged.is_empty() {
        return Ok(());
    }
    let n = b.len();
    let yes = Ref::konst(u32_of(b.consts.len())).0;
    b.consts.push((Ty::BOOL, 1));
    let mut inserts: Vec<hd_tir::own::BatchInsert> = Vec::new();
    for (l, mine, effects) in flagged {
        let first = &tests[mine[0]].1;
        let (call, read, coerce) = (first.call as usize, first.read as usize, first.coerce);
        let u = u32_of(b.local_ty.len());
        b.local_ty.push(Ty::BOOL);
        b.local_name.push(cx.load_name());
        b.local_syn.push(b.syn[call]);
        b.local_flags
            .push(local_flags::ASSIGNED | local_flags::READ);
        let callee = b.data[call][0];
        let args: Vec<u32> = b.record(b.data[call][1]).to_vec();
        let syn = b.syn[call];
        // `u := is_unique(P)` before statement position `(block, pos)`.
        let retest = |b: &mut Body, inserts: &mut Vec<_>, block: u32, pos: u32| {
            let at = |inserts: &Vec<hd_tir::own::BatchInsert>| u32_of(n + inserts.len());
            let push = |inserts: &mut Vec<_>, tag, data, ty| {
                inserts.push(hd_tir::own::BatchInsert {
                    block,
                    pos,
                    tag,
                    data,
                    ty,
                    syn,
                });
            };
            let r = at(inserts);
            push(
                inserts,
                Tag::Read,
                [b.data[read][0], ReadMode::Borrow as u32],
                b.ty[read],
            );
            let mut arg = r;
            if let Some(c) = coerce {
                arg = at(inserts);
                push(
                    inserts,
                    Tag::Coerce,
                    [r, b.data[c as usize][1]],
                    b.ty[c as usize],
                );
            }
            let mut words = args.clone();
            words[0] = arg;
            let record = u32_of(b.extra.len());
            b.extra.push(u32_of(words.len()));
            b.extra.extend(words);
            let test = at(inserts);
            push(inserts, Tag::Call, [callee, record], Ty::BOOL);
            push(inserts, Tag::Store, [u | place::BARE_LOCAL, test], Ty::VOID);
        };
        retest(b, &mut inserts, topo.blk[l as usize], topo.pos[l as usize]);
        for (x, e) in effects {
            let Some((block, pos)) = after(b, &topo, x) else {
                return Err(format!("unique flag: %{x} is no statement"));
            };
            if e == Effect::Fresh {
                inserts.push(hd_tir::own::BatchInsert {
                    block,
                    pos,
                    tag: Tag::Store,
                    data: [u | place::BARE_LOCAL, yes],
                    ty: Ty::VOID,
                    syn,
                });
            } else {
                retest(b, &mut inserts, block, pos);
            }
        }
        for &j in &mine {
            let c = tests[j].1.call as usize;
            b.tags[c] = Tag::Read;
            b.data[c] = [u | place::BARE_LOCAL, ReadMode::Copy as u32];
        }
    }
    hd_tir::own::insert_batch(b, &topo, &inserts).map(|_| ())
}
