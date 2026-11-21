//! ABI-owned parameter cleanup stated after ownership has decided the body.
//!
//! A release descriptor declares cleanup at the common function exit, after its
//! ordinary exit drops and loan cleanup. Its lexical position emits nothing.
//! It releases only when the resolved ABI supplies an owned argument; an
//! instance receiving it borrowed has no count to release.

use super::{OwnCx, Pass, params};
use crate::ir::{Body, BodyKind};

/// The root parameters whose declarations may supply a count the body
/// itself does not own. Closure and suspension ABIs are separate kinds; a
/// default has no item signature, so its parameter locals are its own.
pub(crate) fn parameters(b: &Body, cx: &dyn OwnCx) -> Vec<u32> {
    if !matches!(
        b.kind,
        BodyKind::Fn | BodyKind::DeriveInstance | BodyKind::Default
    ) || b.kind != BodyKind::Default && cx.suspending(b.item)
    {
        return Vec::new();
    }
    let params = params(b);
    if b.kind != BodyKind::Default && cx.parameter_count(b.item) != Some(params.len()) {
        return Vec::new();
    }
    params
        .iter()
        .enumerate()
        .filter_map(|(k, &l)| {
            (cx.counted(b.local_ty[l as usize])
                && cx.received(b.item, k, params.len()) == Pass::Owned
                && !b
                    .exit_drops
                    .iter()
                    .any(|&(sub, local)| sub.raw() == 0 && local.raw() == l))
            .then_some(l)
        })
        .collect()
}

#[cfg(test)]
mod tests;
