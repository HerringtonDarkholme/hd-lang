//! Argument promotion (#1151), release only: a `&T` parameter whose body
//! only reads through it takes its referent by value in a promoted copy
//! of the body, and a direct call whose argument is the address of the
//! caller's own local passes that local's value instead
//! (`crates/hd_wasm_linear/DESIGN.md` §23.40). Whether `T` is small and
//! holds no counted leaf is the backend's question; these are the TIR
//! halves: which reference a call may replace with its value
//! ([`promotable_ref`]).

use crate::ir::{Body, Root, Tag};

/// Whether `Ref` `i` of `b` is a read-only reference to a part of a local
/// no closure captures, used once by instruction `user` and otherwise only
/// dropped: the reference a promoted argument replaces with its value.
#[must_use]
pub fn promotable_ref(b: &Body, i: usize, user: usize) -> bool {
    if b.tags[i] != Tag::Ref || b.data[i][1] & crate::ir::ref_word::MUT != 0 {
        return false;
    }
    let p = b.view(b.data[i][0]);
    let Root::Local(l) = p.root else {
        return false;
    };
    if !p.in_place(b) || b.cap_local.iter().any(|c| c.raw() == l) {
        return false;
    }
    let mut ops = Vec::new();
    let mut seen = 0;
    for j in 0..b.len() {
        super::operands(b, j, &mut ops);
        let n = ops.iter().filter(|o| o.value as usize == i).count();
        if n == 0 {
            continue;
        }
        if j == user {
            seen += n;
        } else if b.tags[j] != Tag::Drop {
            return false;
        }
    }
    seen == 1
}
