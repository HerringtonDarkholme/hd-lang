//! Shared writing and verification of operation and function-boundary count statements.

use super::{
    OwnCx, abi, append_copy, arguments, cache, call_args, call_row, capture_entry, captures,
    cell_move, child, cold, cold_captures, cold_row, context, copy_struct, join_results, map_key,
    map_probe_unique, map_value, parameter_copy, referent, splice, waker,
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
    out.extend(cold_captures::counts(b, cx));
    out.extend(capture_entry::counts(b, cx));
    out.extend(context::counts(b, cx));
    out.extend(call_row::counts(b, cx, topo));
    out.extend(call_args::counts(b, cx, topo));
    out.extend(append_copy::counts(b, cx));
    out.extend(cell_move::counts(b, topo));
    out.extend(parameter_copy::counts(b, cx));
    out.extend(arguments::counts(b, cx));
    out.extend(copy_struct::counts(b, cx));
    out.extend(join_results::counts(b, cx));
    out.extend(map_value::counts(b, cx));
    out.extend(map_probe_unique::counts(b, cx));
    out.extend(child::counts(b, cx, topo));
    out.extend(waker::counts(b));
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
        if let Some(k) = found.filter(|&k| {
            if info.at == At::Operation {
                crate::ir::count::adjacent(b, topo, expected[k].1, u32_of(i))
            } else {
                topo.blk[i] == expected[k].1
            }
        }) {
            seen[k] += 1;
        } else {
            err(u32_of(i), "a boundary count without its count fact".into());
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
