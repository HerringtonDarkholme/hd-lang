//! A plain closure runs while its caller holds its environment. An unchanged
//! by-value capture of a thin local reference holds the same address in its
//! environment and its entry slot. The closure's loan keeps that address
//! valid after a whole drop clears the slot, so a read already loaded
//! through the slot can keep borrowing the referent.
//! A shared slot, a slot whose own storage is written or addressed, and a
//! suspending closure do not supply this proof.

use hd_base::SliceSort as _;

use super::OwnCx;
use crate::ir::{Body, Root, Tag, Topology};

#[derive(Default)]
pub(crate) struct ReferenceDrops {
    /// Rows of (sub-body, local, eligible). The same local ID can name
    /// separate entry slots in several sub-bodies.
    slots: Vec<(u32, u32, bool)>,
    by_inst: Vec<bool>,
}

impl ReferenceDrops {
    /// Whole drops of unchanged by-value reference captures. Sort rows
    /// once so a disqualifying row comes first for each slot and wins.
    pub(crate) fn build(&mut self, b: &Body, cx: &dyn OwnCx, topo: &Topology) {
        self.slots.clear();
        self.by_inst.clear();
        if b.cap_local.is_empty() {
            return;
        }
        for (i, &tag) in b.tags.iter().enumerate() {
            let [a, w] = b.data[i];
            let sub = topo.sub[i];
            if tag == Tag::Closure {
                let record = b.record(w);
                let plain = !cx.function_suspends(b.ty[i]);
                for c in (record[0]..record[0] + record[1]).map(|c| c as usize) {
                    let local = b.cap_local[c];
                    let shared = b.cap_mode[c].shares();
                    let ty = b.local_ty[local.idx()];
                    if cx.reference_target(ty).is_some() && cx.plain_reference(ty) {
                        self.slots.push((a, local.raw(), plain && !shared));
                        if shared && sub != 0 {
                            self.slots.push((sub, local.raw(), false));
                        }
                    }
                }
            } else if sub != 0
                && matches!(tag, Tag::Store | Tag::Ref)
                && let Some(local) = b.local_in_place(a)
            {
                self.slots.push((sub, local.raw(), false));
            }
        }
        self.slots.sort_stable();
        self.slots.dedup_by_key(|slot| (slot.0, slot.1));
        self.slots.retain(|slot| slot.2);
        if self.slots.is_empty() {
            return;
        }
        self.by_inst
            .extend(b.tags.iter().enumerate().map(|(i, &tag)| {
                if tag != Tag::Drop {
                    return false;
                }
                let p = b.view(b.data[i][0]);
                matches!(p.root, Root::Local(l)
                if p.whole() && self.slots.binary_search(&(topo.sub[i], l, true)).is_ok())
            }));
    }

    pub(crate) fn at(&self, i: usize) -> bool {
        self.by_inst.get(i) == Some(&true)
    }
}
