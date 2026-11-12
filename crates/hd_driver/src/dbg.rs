//! Suppression after checking (`module.dbg.dependency`): retain argument
//! evaluation, but remove the generated print calls in fetched code.

use hd_base::{Hash128, StableHasher};
use hd_resolve::Names;
use hd_tir::Body;
use hd_tir::ir::{Callee, Coercion, NONE, Tag};

pub(crate) fn key(original: Hash128) -> Hash128 {
    let mut key = StableHasher::new("dbg-dependency");
    key.hash(original);
    key.finish()
}

fn record(body: &mut Body, words: &[u32]) -> u32 {
    let at = u32::try_from(body.extra.len()).expect("TIR record offset");
    body.extra
        .push(u32::try_from(words.len()).expect("TIR record length"));
    body.extra.extend_from_slice(words);
    at
}

pub(crate) fn suppress(body: &mut Body, names: &Names<'_>) -> bool {
    let dbg = names.known.dbg;
    // Its signature is the same one-tuple-argument, void-result shape as
    // `dbg`, and its ordinary body is `pass`. A function value originating
    // here stays silent even if it is later called in the root package.
    let done = names.item("std.format", "dbg_done");
    let mut changed = false;
    let format_module = names.module("std.format");
    for i in 0..body.tags.len() {
        let [a, b] = body.data[i];
        if body.tags[i] == Tag::ItemRef && body.record(a) == [dbg.raw()] {
            body.data[i][0] = record(body, &[done.raw()]);
            changed = true;
            continue;
        }
        if body.tags[i] != Tag::Call {
            continue;
        }
        let Some(Callee::Item { def, targs }) = Callee::from_words(body.record(a)) else {
            continue;
        };
        if def == dbg {
            body.data[i][0] = record(body, &Callee::Item { def: done, targs }.words());
            changed = true;
            continue;
        }
        if names.module_node(def) != format_module {
            continue;
        }
        match names.display_name(def) {
            "dbg_one" | "dbg_opaque" | "dbg_shown" => {
                let value = body.record(b)[1];
                let coercion = record(body, &[Coercion::Identity as u32, NONE]);
                body.tags[i] = Tag::Coerce;
                body.data[i] = [value, coercion];
            }
            "dbg_bare_one" | "dbg_bare_shown" | "dbg_bare_opaque" | "dbg_here" => {
                let empty = record(body, &[]);
                body.tags[i] = Tag::Block;
                body.data[i] = [empty, NONE];
            }
            _ => continue,
        }
        changed = true;
    }
    changed
}

/// `fn.caller.root-package`: clears the root-package bit of each call-site
/// record this fetched body passes to a `@track_caller` callee (`tracked`),
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
