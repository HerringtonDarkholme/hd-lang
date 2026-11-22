//! Shared writing and verification of operation and function-boundary count statements.

use super::{
    OwnCx, abi, append_copy, arguments, cache, call_args, call_row, captures, child, cold,
    cold_row, context, copy_struct, join_results, map_key, map_probe_unique, map_value,
    parameter_copy, referent, splice,
};
use crate::ir::{
    Body, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};
use hd_base::SliceSort as _;
use hd_types::Ty;

fn key(tag: Tag, target: u32, info: Info) -> (u32, u32, Info) {
    (target, tag as u32, info)
}

fn expected(b: &Body, cx: &dyn OwnCx, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    let root = b.sub_root[0];
    let mut out = Vec::new();
    out.extend(
        abi::parameters(b, cx)
            .into_iter()
            .map(|l| (Tag::Release, root, Info::new(At::Exit, Part::Local(l), 2))),
    );
    for (root, l) in cold::parameters(b, cx) {
        for at in [At::Cold, At::Entry] {
            out.push((Tag::Retain, root, Info::new(at, Part::Local(l), 0)));
        }
    }
    for root in cold_row::roots(b, cx) {
        for at in [At::Cold, At::Materialize] {
            out.push((Tag::Retain, root, Info::new(at, Part::Row, 0)));
        }
        out.push((Tag::Release, root, Info::new(At::FrameDrop, Part::Row, 2)));
    }
    if cache::counted(b, cx) {
        for at in [At::CacheRead, At::CacheWrite] {
            out.push((Tag::Retain, root, Info::new(at, Part::Result, 0)));
        }
    }
    out.extend(referent::counts(b, cx));
    out.extend(captures::counts(b, cx));
    out.extend(context::counts(b, cx));
    out.extend(call_row::counts(b, cx, topo));
    out.extend(call_args::counts(b, cx, topo));
    out.extend(append_copy::counts(b, cx));
    out.extend(parameter_copy::counts(b, cx));
    out.extend(arguments::counts(b, cx));
    out.extend(copy_struct::counts(b, cx));
    out.extend(join_results::counts(b, cx));
    out.extend(map_value::counts(b, cx));
    out.extend(map_probe_unique::counts(b, cx));
    out.extend(child::counts(b, cx, topo));
    for i in 0..b.len() {
        if matches!(b.tags[i], Tag::Call | Tag::Await)
            && let Some(callee) = crate::ir::Callee::from_words(b.record(b.data[i][0]))
        {
            if b.tags[i] == Tag::Call && cx.copies_map_snapshot(&callee) {
                out.push((
                    Tag::Retain,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(0), 0),
                ));
            }
            if let Some(&receiver) = b.record(b.data[i][1]).first()
                && let Some(receiver) = crate::ir::Ref(receiver).as_inst()
                && cx.dynamic_receiver_may_load(&callee, b.ty[receiver.0 as usize])
            {
                for (tag, reason) in [(Tag::Retain, 1), (Tag::Release, 2)] {
                    out.push((
                        tag,
                        u32_of(i),
                        Info::new(At::Operation, Part::DynamicReceiver, reason),
                    ));
                }
            }
        }
        for k in (1..3).filter(|&k| map_key::needed(b, cx, i, k as usize)) {
            let info = Info::new(At::Operation, Part::Operand(k), 0);
            out.push((Tag::Retain, u32_of(i), info));
        }
    }
    out
}

pub(super) fn run(
    b: &mut Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    let wanted = expected(b, cx, topo);
    // Index the existing declarations once, not once per closure. On the
    // first run this is empty; value counts have a separate producer.
    let mut existing = Vec::new();
    if !wanted.is_empty() {
        for i in 0..b.len() {
            if let Some(info) = Info::read(b, i)
                && info.at != At::Value
            {
                existing.push(key(b.tags[i], b.data[i][0], info));
            }
        }
        existing.sort_stable();
    }
    for (tag, target, info) in wanted {
        if existing.binary_search(&key(tag, target, info)).is_ok() {
            continue;
        }
        let (block, position) = if info.at == At::Operation {
            let block = topo.blk[target as usize];
            let mut position = topo.pos[target as usize] + 1;
            // A borrowed result becomes owned before any operation cleanup
            // can end its source argument. Keep its value retain first.
            if b.record(b.data[block as usize][0])
                .get(position as usize)
                .is_some_and(|&next| {
                    b.tags[next as usize] == Tag::Retain
                        && b.data[next as usize][0] == target
                        && Info::read(b, next as usize).is_some_and(|c| c.at == At::Value)
                })
            {
                position += 1;
            }
            (block, position)
        } else {
            (target, 0)
        };
        let record = info.write(b);
        let syn = match info.part {
            Part::Local(l) | Part::Referent(l) => b.local_syn[l as usize],
            _ => b.syn[target as usize],
        };
        edits.insert(block, position, tag, [target, record], Ty::VOID, syn);
    }
    // Borrowed-result retains were queued first. At the same position,
    // stable edit order keeps them before their operation cleanup packet.
    splice::apply(b, topo, edits)?;
    super::retains::anchor(b);
    if !b.anchors.is_empty() {
        for i in 0..b.len() {
            if Info::read(b, i).is_some_and(|c| c.at == At::Operation) {
                b.anchors[i] = b.anchors[b.data[i][0] as usize];
            }
        }
    }
    Ok(())
}

pub(crate) fn verify(b: &Body, cx: &dyn OwnCx, topo: &Topology, mut err: impl FnMut(u32, String)) {
    let expected = expected(b, cx, topo);
    let mut index: Vec<_> = expected
        .iter()
        .enumerate()
        .map(|(i, &(tag, target, info))| (key(tag, target, info), i))
        .collect();
    index.sort_stable_by_key(|&(key, _)| key);
    let mut seen = vec![0; expected.len()];
    for i in 0..b.len() {
        if !matches!(b.tags[i], Tag::Retain | Tag::Release | Tag::MakeUnique) {
            continue;
        }
        let Some(info) = Info::read(b, i) else {
            err(u32_of(i), "a malformed count record".into());
            continue;
        };
        if b.ty[i] != Ty::VOID {
            err(u32_of(i), "a count statement has a value type".into());
        }
        if info.at == At::Value || (info.at == At::Operation && info.part == Part::Result) {
            // A value's release is a cancel block's (`known.rs`).
            let wanted = if info.at == At::Value && b.tags[i] == Tag::Release {
                let reason = super::reason::CANCELLED;
                (Tag::Release, Info::new(At::Value, Part::Result, reason))
            } else if info.at == At::Value {
                (Tag::Retain, Info::new(At::Value, Part::Result, 0))
            } else {
                (Tag::Release, Info::new(At::Operation, Part::Result, 3))
            };
            if (b.tags[i], info) != wanted {
                err(u32_of(i), "a count without its operation fact".into());
            }
            continue;
        }
        let found = index
            .binary_search_by_key(&key(b.tags[i], b.data[i][0], info), |&(key, _)| key)
            .ok()
            .map(|k| index[k].1);
        if let Some(k) = found.filter(|&k| match info.at {
            At::Operation => crate::ir::count::adjacent(b, topo, expected[k].1, u32_of(i)),
            // A sub-body's exit count stands on its ways out ([`Exits`]).
            At::Exit if b.sub_root.contains(&expected[k].1) => true,
            _ => topo.blk[i] == expected[k].1,
        }) {
            seen[k] += 1;
        } else {
            err(u32_of(i), "a boundary count without its count fact".into());
        }
    }
    for (k, n) in seen.into_iter().enumerate() {
        let (_, target, info) = expected[k];
        if n == 0 || n > 1 && !(info.at == At::Exit && b.sub_root.contains(&target)) {
            err(
                target,
                "a required boundary count is absent or repeated".into(),
            );
        }
    }
    for &root in &b.sub_root {
        let mut counts: Vec<_> = (expected.iter())
            .filter(|&&(_, target, info)| target == root && info.at == At::Exit)
            .map(|&(tag, target, info)| key(tag, target, info))
            .collect();
        if counts.is_empty() {
            continue;
        }
        counts.sort_stable();
        let mut walk = Exits {
            counts,
            errs: Vec::new(),
        };
        let mut s = super::flow::Flow::default();
        s.arena.words = (3 * walk.counts.len()).div_ceil(64);
        let st = s.arena.alloc(None);
        for k in 0..walk.counts.len() {
            s.arena.set(st, 3 * u32_of(k), true);
        }
        if super::flow::forward(b, topo, &mut s, &mut walk, root, st) {
            walk.out(root, &s.arena, st);
        }
        for (x, what) in walk.errs {
            err(x, what);
        }
    }
}

/// A sub-body's exit counts run exactly once on every way out of it: its
/// end, each `return` and each cancel exit. Per count, the state holds
/// which of 0, 1 or more runs a path may have made (bits `3k`, `3k + 1`,
/// `3k + 2`), joined over paths.
struct Exits {
    counts: Vec<(u32, u32, Info)>,
    errs: Vec<(u32, String)>,
}

impl Exits {
    fn out(&mut self, x: u32, s: &super::flow::BitArena, st: usize) {
        for k in 0..u32_of(self.counts.len()) {
            if s.get(st, 3 * k) || s.get(st, 3 * k + 2) {
                let what = "a boundary count is absent or repeated on a way out";
                self.errs.push((x, what.into()));
            }
        }
    }
}

impl super::flow::Forward for Exits {
    const SUITES: super::flow::Suites = super::flow::Suites::FromEnd { union: false };

    fn stmt(&mut self, b: &Body, x: u32, s: &mut super::flow::BitArena, st: usize) {
        let i = x as usize;
        let counted = matches!(b.tags[i], Tag::Retain | Tag::Release);
        let Some(info) = Info::read(b, i).filter(|_| counted) else {
            return;
        };
        if let Ok(k) = self
            .counts
            .binary_search(&key(b.tags[i], b.data[i][0], info))
        {
            let k = 3 * u32_of(k);
            let (none, once) = (s.get(st, k), s.get(st, k + 1));
            s.set(st, k, false);
            s.set(st, k + 1, none);
            s.set(st, k + 2, once || s.get(st, k + 2));
        }
    }

    fn enter(&mut self, _: &Body, _: u32, _: &mut super::flow::BitArena, _: usize) {}

    fn exit(&mut self, _: &Body, x: u32, s: &mut super::flow::BitArena, st: usize) {
        self.out(x, s, st);
    }
}
