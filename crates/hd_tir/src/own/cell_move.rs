//! A whole-local move transfers its payload and ends its storage owner.
//! Storage rows are per sub-body. Their physical heap-cell lowering ends
//! one cell count, independently of whether the payload itself counts;
//! a stack slot or a promoted local lowers no cell release.

use crate::ir::{
    Body, ReadMode, Tag, Topology,
    count::{At, Info, Part},
    u32_of,
};

pub(super) fn counts(b: &Body, topo: &Topology) -> Vec<(Tag, u32, Info)> {
    if b.storage.is_empty() {
        return Vec::new();
    }
    b.tags
        .iter()
        .enumerate()
        .filter_map(|(i, &tag)| {
            if tag != Tag::Read || b.data[i][1] != ReadMode::Move as u32 {
                return None;
            }
            let local = b.local_read(i)?.raw();
            // storage::run writes rows in (sub, local) order. A row says this
            // local has placed storage here, without repeating its inference.
            b.storage
                .binary_search_by_key(&(topo.sub[i], local), |&(s, l, _)| (s.raw(), l.raw()))
                .is_ok()
                .then_some((
                    Tag::Release,
                    u32_of(i),
                    Info::new(At::Operation, Part::Local(local), 2),
                ))
        })
        .collect()
}
