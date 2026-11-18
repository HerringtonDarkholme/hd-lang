#![cfg(test)]

// Test-only reference for the former owned-tree semantics.
use crate::Discr;
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Walk {
    /// Value `k` is an address in a counted object, or in static data: a
    /// string's bytes, a local reference, an open value; counting finds its
    /// block through the page table (§23.5).
    Ref(u32),
    /// Value `k` is null, a static object's, or the payload start of a
    /// counted object, eight bytes after its header: a collection's buffer,
    /// a closure's environment, a
    /// thin counted reference (`layout::ref_walk`); counting
    /// reads the header at `k - 8` (§23.30).
    Known(u32),
    /// No counted address, but a value whose release runs a
    /// `std.rt.Drop` implementation: a `UniqList` of scalars, whose
    /// buffer's room is freed by its drop (`module.rt.drop.glue`). It
    /// stands alone; a walk with a counted address already releases.
    Glue,
    /// Each part in turn.
    Seq(Vec<Walk>),
    /// An enum's discriminant; arm `v`, when there is one, walks variant
    /// `v`'s payload, where the discriminant says `v` ([`Enc::is`]).
    /// The discriminant is boxed: a walk is part of many helpers' keys.
    Switch(Box<Discr>, Vec<Option<Walk>>),
}

impl Walk {
    /// The walk without the values `skip`, which a copy replaces rather
    /// than counts; `None` when nothing is left.
    #[must_use]
    pub fn without(&self, skip: &[u32]) -> Option<Walk> {
        match self {
            Walk::Ref(k) => (!skip.contains(k)).then_some(Walk::Ref(*k)),
            Walk::Known(k) => (!skip.contains(k)).then_some(Walk::Known(*k)),
            Walk::Glue => Some(Walk::Glue),
            Walk::Seq(ws) => Walk::seq(ws.iter().filter_map(|w| w.without(skip)).collect()),
            Walk::Switch(d, arms) => Some(Walk::Switch(
                d.clone(),
                arms.iter()
                    .map(|a| a.as_ref().and_then(|w| w.without(skip)))
                    .collect(),
            )),
        }
    }

    fn seq(mut ws: Vec<Walk>) -> Option<Walk> {
        if ws.iter().any(|w| *w != Walk::Glue) {
            ws.retain(|w| *w != Walk::Glue);
        } else {
            ws.truncate(1);
        }
        match ws.len() {
            0 => None,
            1 => ws.pop(),
            _ => Some(Walk::Seq(ws)),
        }
    }
}
