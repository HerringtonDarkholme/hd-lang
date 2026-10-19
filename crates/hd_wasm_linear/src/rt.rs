//! Runtime pieces the compiler generates (`crates/hd_wasm/DESIGN.md`
//! §23.9): the entry exports. Each is a [`Helper`], built once on its
//! first request.

use hd_base::{Hash128, StageResult};
use hd_types::{Ty, TyList};
use wasm_encoder::Instruction;

use crate::module::{Func, Global, Module, Sym};
use crate::{Entry, Input, unsupported};

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// `hd.init`: the inits in order; a flagged one belongs to a module
    /// other than the entry's and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts`).
    EntryInit { inits: Vec<(Hash128, bool)> },
    /// `hd.poll(ctx)`: runs `main` (no providers yet), passes its results
    /// to `report` when there is one, then runs `then` as the script's
    /// later inits; returns the exit status (0 without a report).
    EntryPoll {
        main: Hash128,
        results: u32,
        report: Option<Hash128>,
        then: Vec<(Hash128, bool)>,
    },
}

/// Defines the entry exports of `input.entry` in `m`.
pub fn entry(input: &Input<'_>, m: &mut Module) -> StageResult<()> {
    let (main, key, before, after, report) = match &input.entry {
        Entry::Main {
            main,
            key,
            inits,
            report,
        } => (*main, *key, inits.clone(), Vec::new(), *report),
        Entry::Script {
            init,
            key,
            before,
            after,
            report,
        } => (*init, *key, before.clone(), after.clone(), *report),
        Entry::Unsupported(what) => return unsupported(*what),
    };
    if !input.env.row_keys(main, TyList::EMPTY).is_empty() {
        return unsupported("an entry with providers (job 3, #540)");
    }
    let ret = input.env.ret(main).unwrap_or(Ty::VOID);
    let results = crate::layout::vals(input.pool, ret)?.len();
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: before }));
    m.export(
        "hd.poll",
        Sym::Helper(Helper::EntryPoll {
            main: key,
            results: u32::try_from(results).expect("results"),
            report,
            then: after,
        }),
    );
    Ok(())
}

/// Adds `by` to the forbidden-context counter.
fn forbid_by(f: &mut Func, by: i32) {
    f.body.push(crate::module::Ins::GlobalGet(Global::Forbid));
    f.i32(by);
    f.op(Instruction::I32Add);
    f.body.push(crate::module::Ins::GlobalSet(Global::Forbid));
}

/// Calls init `k`, as a forbidden context when flagged.
fn call_init(f: &mut Func, k: Hash128, forbid: bool) {
    if forbid {
        forbid_by(f, 1);
    }
    f.call(Sym::Inst(k));
    if forbid {
        forbid_by(f, -1);
    }
}

/// Builds helper `h`; it may ask `m` for more helpers.
pub fn helper(h: &Helper, _m: &mut Module) -> Func {
    use crate::module::Val;
    match h {
        Helper::EntryInit { inits } => {
            let mut f = Func::new("rt:entry_init", vec![], vec![]);
            for (k, forbid) in inits {
                call_init(&mut f, *k, *forbid);
            }
            f
        }
        Helper::EntryPoll {
            main,
            results,
            report,
            then,
        } => {
            // Parameter 0 is the driver's `PollContext`; a plain entry
            // ignores it.
            let mut f = Func::new("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
            f.call(Sym::Inst(*main));
            if let Some(k) = report {
                f.call(Sym::Inst(*k));
            } else {
                for _ in 0..*results {
                    f.op(Instruction::Drop);
                }
                // A script's later inits: its own bodies, which share the
                // first's result, and the others, as forbidden contexts.
                for (k, forbid) in then {
                    call_init(&mut f, *k, *forbid);
                    if !forbid {
                        for _ in 0..*results {
                            f.op(Instruction::Drop);
                        }
                    }
                }
                f.i32(0);
            }
            f
        }
    }
}
