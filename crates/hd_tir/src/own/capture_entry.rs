//! A closure body takes copies of the captured edges it owns on entry.
//! Its environment keeps those edges; borrowed captures and uncounted
//! concrete fields lower no copy. This is separate from the cold frame's
//! constructor packet and from explicit value parameters.

use super::OwnCx;
use crate::ir::{
    Body, Tag,
    count::{At, Info, Part},
};

pub(super) fn counts(b: &Body, cx: &dyn OwnCx) -> Vec<(Tag, u32, Info)> {
    let mut seen = vec![false; b.sub_root.len()];
    let mut out = Vec::new();
    for (i, &tag) in b.tags.iter().enumerate() {
        if tag != Tag::Closure {
            continue;
        }
        let sub = b.data[i][0] as usize;
        if sub == 0 || seen[sub] {
            continue;
        }
        let record = b.record(b.data[i][1]);
        let own = b.closure_self(i);
        let copied = own.is_some()
            || (record[0]..record[0] + record[1]).any(|c| {
                let c = c as usize;
                b.cap_mode[c].shares() || cx.counted(b.local_ty[b.cap_local[c].idx()])
            });
        if copied {
            seen[sub] = true;
            out.push((
                Tag::Retain,
                b.sub_root[sub],
                Info::new(At::Entry, Part::Captures, 0),
            ));
        }
    }
    out
}
