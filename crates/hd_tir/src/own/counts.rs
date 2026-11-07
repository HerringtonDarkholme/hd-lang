//! Shared writing and verification of operation and function-boundary count statements.

use super::{OwnCx, abi, cache, cold, cold_row, map_key, splice};
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

fn expected(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let root = b.sub_root[0];
    let mut out = Vec::new();
    out.extend(
        abi::parameters(b, cx)
            .into_iter()
            .map(|l| (Tag::Release, root, Info::new(At::Exit, Part::Local(l), 2))),
    );
    for l in cold::parameters(b, cx) {
        for at in [At::Cold, At::Entry] {
            out.push((Tag::Retain, root, Info::new(at, Part::Local(l), 0)));
        }
    }
    for root in cold_row::roots(b, cx) {
        for at in [At::Cold, At::Materialize] {
            out.push((Tag::Retain, root, Info::new(at, Part::Row, 0)));
        }
    }
    if cache::counted(b, cx) {
        for at in [At::CacheRead, At::CacheWrite] {
            out.push((Tag::Retain, root, Info::new(at, Part::Result, 0)));
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
    let mut wanted = expected(b, cx);
    // Index the existing declarations once, not once per closure. On the
    // first run this is empty; operation/value counts are separate.
    let mut existing = Vec::new();
    if !wanted.is_empty() {
        for i in 0..b.len() {
            if let Some(info) = Info::read(b, i)
                && !matches!(info.at, At::Value | At::Operation)
            {
                existing.push(key(b.tags[i], b.data[i][0], info));
            }
        }
        existing.sort_stable();
    }
    wanted.extend(
        (0..b.len())
            .filter(|&i| map_key::needed(b, cx, i))
            .map(|i| {
                (
                    Tag::Retain,
                    u32_of(i),
                    Info::new(At::Operation, Part::Operand(1), 0),
                )
            }),
    );
    for (tag, target, info) in wanted {
        let (block, position, present) = if info.at == At::Operation {
            (
                topo.blk[target as usize],
                topo.pos[target as usize] + 1,
                crate::ir::count::following(b, topo, target, tag) == Some(info),
            )
        } else {
            (
                target,
                0,
                existing.binary_search(&key(tag, target, info)).is_ok(),
            )
        };
        if present {
            continue;
        }
        let record = info.write(b);
        let syn = match info.part {
            Part::Local(l) => b.local_syn[l as usize],
            _ => b.syn[target as usize],
        };
        edits.insert(block, position, tag, [target, record], Ty::VOID, syn);
    }
    splice::apply(b, topo, edits)?;
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
    let expected = expected(b, cx);
    let mut index: Vec<_> = expected
        .iter()
        .enumerate()
        .map(|(i, &(tag, target, info))| (key(tag, target, info), i))
        .collect();
    index.sort_stable_by_key(|&(key, _)| key);
    let mut seen = vec![0; expected.len()];
    for i in 0..b.len() {
        if !matches!(b.tags[i], Tag::Retain | Tag::Release) {
            continue;
        }
        let Some(info) = Info::read(b, i) else {
            err(u32_of(i), "a malformed count record".into());
            continue;
        };
        if b.ty[i] != Ty::VOID {
            err(u32_of(i), "a count statement has a value type".into());
        }
        if info.at == At::Operation && b.tags[i] == Tag::Retain {
            let target = b.data[i][0] as usize;
            if info != Info::new(At::Operation, Part::Operand(1), 0)
                || !map_key::needed(b, cx, target)
                || topo.blk[i] != topo.blk[target]
                || topo.pos[i] != topo.pos[target] + 1
            {
                err(
                    u32_of(i),
                    "an operation retain without its key-copy fact".into(),
                );
            }
            continue;
        }
        if matches!(info.at, At::Value | At::Operation) {
            let wanted = if info.at == At::Value {
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
        if let Some(k) = found.filter(|&k| topo.blk[i] == expected[k].1) {
            seen[k] += 1;
        } else {
            err(u32_of(i), "a boundary count without its count fact".into());
        }
    }
    for i in 0..b.len() {
        if map_key::needed(b, cx, i)
            && crate::ir::count::following(b, topo, u32_of(i), Tag::Retain)
                != Some(Info::new(At::Operation, Part::Operand(1), 0))
        {
            err(u32_of(i), "a map put lacks its stored key retain".into());
        }
    }
    for (k, n) in seen.into_iter().enumerate() {
        if n != 1 {
            err(
                expected[k].1,
                "a required boundary count is absent or repeated".into(),
            );
        }
    }
}
