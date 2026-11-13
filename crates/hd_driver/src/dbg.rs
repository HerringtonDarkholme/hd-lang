//! Fetched code's call sites (`fn.caller.root-package`): their records
//! say they are not in the user's own code, so `dbg` prints nothing there
//! at run time (`module.dbg.dependency`).

use hd_base::{Hash128, StableHasher};
use hd_tir::Body;
use hd_tir::ir::{Callee, Tag};

pub(crate) fn key(original: Hash128) -> Hash128 {
    let mut key = StableHasher::new("dbg-dependency");
    key.hash(original);
    key.finish()
}

/// `fn.caller.root-package`: clears the root-package bit of each call-site
/// record this fetched body passes to a `@track_caller` callee that takes
/// one (`tracked`),
/// so `Location::in_root_package()` answers false for its call sites. A
/// record a body receives as its own `$site` came from its caller and is
/// left alone. Cost: one pass over the body's instructions.
pub(crate) fn foreign_sites(body: &mut Body, tracked: impl Fn(hd_base::DefId) -> bool) -> bool {
    let mut changed = false;
    for i in 0..body.tags.len() {
        if !matches!(body.tags[i], Tag::Call | Tag::Await) {
            continue;
        }
        let [a, b] = body.data[i];
        let Some(Callee::Item { def, .. }) = Callee::from_words(body.record(a)) else {
            continue;
        };
        if !tracked(def) {
            continue;
        }
        // The record is the arguments, then three provider words; the
        // record is the last argument.
        let args = body.record(b);
        let Some(&site) = args.len().checked_sub(4).and_then(|k| args.get(k)) else {
            continue;
        };
        if site & hd_tir::ir::Ref::CONST_BIT == 0 {
            continue;
        }
        let Some(&(ty, bits)) = body
            .consts
            .get((site & !hd_tir::ir::Ref::CONST_BIT) as usize)
        else {
            continue;
        };
        let Some(text) = (ty == hd_types::Ty::STRING)
            .then(|| usize::try_from(bits).ok())
            .flatten()
            .and_then(|k| body.strings.get_mut(k))
        else {
            continue;
        };
        if let Some(rest) = text.strip_prefix('1') {
            *text = format!("0{rest}").into();
            changed = true;
        }
    }
    changed
}
