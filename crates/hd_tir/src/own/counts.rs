//! Shared writing and verification of function-boundary count statements.

use super::{OwnCx, abi, cache, cold, cold_row, splice};
use crate::ir::{
    Body, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};
use hd_types::Ty;

fn expected(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, Info)> {
    let mut out = Vec::new();
    out.extend(
        abi::parameters(b, cx)
            .into_iter()
            .map(|l| (Tag::Release, Info::new(At::Exit, Part::Local(l), 2))),
    );
    out.extend(
        cold::parameters(b, cx)
            .into_iter()
            .map(|l| (Tag::Retain, Info::new(At::Cold, Part::Local(l), 0))),
    );
    if cold_row::needed(b, cx) {
        out.push((Tag::Retain, Info::new(At::Cold, Part::Row, 0)));
    }
    if cache::counted(b, cx) {
        for at in [At::CacheRead, At::CacheWrite] {
            out.push((Tag::Retain, Info::new(at, Part::Result, 0)));
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
    let root = b.sub_root[0];
    for (tag, info) in expected(b, cx) {
        if crate::ir::count::find(b, tag, root, info.at, info.part).is_some() {
            continue;
        }
        let record = info.write(b);
        let syn = match info.part {
            Part::Local(l) => b.local_syn[l as usize],
            _ => b.syn[root as usize],
        };
        edits.insert(root, 0, tag, [root, record], Ty::VOID, syn);
    }
    splice::apply(b, topo, edits)
}

pub(crate) fn verify(b: &Body, cx: &dyn OwnCx, topo: &Topology, mut err: impl FnMut(u32, String)) {
    let root = b.sub_root[0];
    let expected = expected(b, cx);
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
        let found = expected
            .iter()
            .position(|&(t, c)| t == b.tags[i] && c == info);
        if let Some(k) = found.filter(|_| b.data[i][0] == root && topo.blk[i] == root) {
            seen[k] += 1;
        } else {
            err(u32_of(i), "a boundary count without its count fact".into());
        }
    }
    for n in seen {
        if n != 1 {
            err(
                root,
                "a required boundary count is absent or repeated".into(),
            );
        }
    }
}
