//! Raw Buffer rooms use the ordinary allocator in unchecked builds. In a
//! checked build their allocation and both free paths use the same ledger
//! as objects, before any room body can be copied into a caller.

use hd_base::{Hash128, StableHasher};
use hd_tir::ir::{Body, Callee};

use crate::Run;

impl Run<'_> {
    pub(super) fn checked_room_calls(&self, body: &mut Body, hash: Hash128) -> Hash128 {
        if !self.checked() {
            return hash;
        }
        let names = self.names();
        let known = names.known;
        let (from, to) = if body.item == known.room {
            (known.alloc, known.checked_allocate)
        } else if body.item == known.free_room || body.item == known.cycle_collect {
            (known.free_block, known.checked_free_acyclic)
        } else {
            return hash;
        };
        let mut changed = false;
        for i in 0..body.len() {
            if let Some(Callee::Item { def, .. }) = hd_tir::inline::known_call(body, i)
                && def == from
            {
                // Callee record: [length, Item, definition, type arguments].
                let at = body.data[i][0] as usize;
                body.extra[at + 2] = to.raw();
                changed = true;
            }
        }
        if !changed {
            return hash;
        }
        let mut digest = StableHasher::new("checked-room-calls-v1");
        digest.hash(hash);
        digest.hash(names.path_hash(to));
        digest.finish()
    }
}
