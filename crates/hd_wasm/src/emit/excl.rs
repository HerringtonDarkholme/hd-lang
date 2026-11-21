//! Call facts the pointer-loan lowering reads (model A of
//! `docs/compiler/ownership.md#pointer-loans-model-a`, `borrow.rs`): a
//! call's arguments, its auto-ref receiver, and the link-time loan
//! decisions (`hd_tir::own::pointer`). The checker places the loans
//! themselves as `LoanBegin`/`LoanClose` statements (`hd_tir::own::loans`).

use hd_mono::Target;
use hd_tir::ir::{Body, Callee, Proj, Tag, Word, slots};

use crate::Input;
use crate::module::Module;

/// The struct field indices `steps` name, when they are all fields.
pub(super) fn fields(steps: &[Proj]) -> Option<Vec<u32>> {
    steps
        .iter()
        .map(|s| match s {
            Proj::Field(f) => Some(*f),
            _ => None,
        })
        .collect()
}

/// The values passed as arguments by call instruction `i`.
pub(super) fn call_args(b: &Body, i: usize) -> Vec<u32> {
    if b.tags[i] == Tag::CallValue {
        return b.call_args(i).to_vec();
    }
    let mut args = Vec::new();
    slots(b, i, |_, v, word| {
        if matches!(word, Word::Value(_)) && v < u32::try_from(i).expect("insts") {
            args.push(v);
        }
    });
    args
}

/// Whether call `i` is non-suspending, and its auto-ref receiver's
/// mutability when it has one.
#[allow(clippy::option_option)] // Outer: ordinary call; inner: auto-ref receiver.
pub(super) fn call_info(
    input: &Input<'_>,
    b: &Body,
    i: usize,
    target: Option<&Target>,
) -> Option<Option<bool>> {
    if b.tags[i] == Tag::CallValue {
        return Some(None);
    }
    if b.tags[i] != Tag::Call {
        return None;
    }
    let env = input.env;
    match target? {
        Target::Call(call) => Some(env.local_ref_receiver(call.item)),
        // A trait method on a trait value: its declaration says how it
        // takes `self`.
        Target::Dyn => {
            let Callee::TraitMethod { method, .. } = Callee::from_words(b.record(b.data[i][0]))?
            else {
                return None;
            };
            Some(env.local_ref_receiver(method))
        }
        // A map probe, `Map.get_or_insert(&mut self, key, default)`: its
        // target keeps only the key's hash and equality, not the item, and
        // collection gives a `Call` such a target for no other callee.
        Target::MapKey { .. } | Target::MapInline(_) => Some(Some(true)),
        _ => None,
    }
}

/// Install the TIR link pass's pointer-loan tables for emission.
pub(crate) fn collect_loans(input: &Input<'_>, m: &mut Module) {
    let decisions = super::loan_facts::decide(input, m);
    m.kept_sites = decisions
        .kept
        .iter()
        .map(|&(id, site, _)| (id, site))
        .collect();
    m.kept = decisions.kept;
    m.crossed = decisions.crossed;
    m.lent = decisions.lent;
    m.dyn_types = decisions.dyn_types;
}
