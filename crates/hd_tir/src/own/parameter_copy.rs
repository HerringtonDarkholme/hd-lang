//! The copy a plain body owns when its explicit parameter arrives lent.
//! The exit set states that ownership; the concrete ABI either supplies
//! the count or lends the value and lowers this entry copy. Captures and
//! suspending boundaries have separate contracts.

use super::{OwnCx, Tri, cold};
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut suspended = vec![false; b.sub_root.len()];
    for sub in cold::sub_bodies(b, cx) {
        suspended[sub as usize] = true;
    }
    let mut out = Vec::new();
    for (sub, &suspends) in suspended.iter().enumerate() {
        if suspends {
            continue;
        }
        for l in cold::locals(b, crate::ir::u32_of(sub)) {
            let ty = b.local_ty[l as usize];
            if cx.counted(ty)
                && (sub != 0 || cx.holds_drop(ty) != Tri::Yes)
                && b.exit_drops
                    .iter()
                    .any(|&(s, local)| s.idx() == sub && local.raw() == l)
            {
                out.push((
                    Tag::Retain,
                    b.sub_root[sub],
                    Info::new(At::Entry, Part::Local(l), 0),
                ));
            }
        }
    }
    out
}
