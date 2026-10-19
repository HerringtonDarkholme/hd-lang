#![forbid(unsafe_code)]
//! `hd_wasm_linear`: the linear-memory backend (`crates/hd_wasm/DESIGN.md`
//! §23). Values live inline in Wasm linear memory and counted objects are
//! freed by reference counting; the Wasm GC backend (`hd_wasm`) stays the
//! default until this one passes conformance.
//!
//! [`build`] makes one whole program's module from collection's output:
//! every instance's TIR body becomes one function (`emit`), the runtime
//! pieces the compiler generates are helpers (`rt`), and `module` numbers
//! and encodes them. There is no per-instance code cache yet: the driver
//! calls [`build`] once per program, at link.

mod emit;
mod layout;
mod module;
mod rt;

use hd_base::{DefId, Hash128, InstId, NotImplemented, Stage, StageResult};
use hd_mono::{Collected, ProgramEnv};
use hd_tir::ir::Body;
use hd_types::InternPool;

pub use module::Sym;
pub use rt::Helper;

/// The entry behavior of the program a module runs
/// (`crates/hd_wasm/DESIGN.md` §15.4, "Init order").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A program with `main`: `hd.init` runs `inits` in order, each
    /// flagged one as a forbidden context, and `hd.poll` runs `main`'s
    /// instance `key`; `report` turns its result into the exit status.
    Main {
        main: DefId,
        key: Hash128,
        inits: Vec<(Hash128, bool)>,
        report: Option<Hash128>,
    },
    /// A script (`module.init.script`): `hd.init` runs `before`, and
    /// `hd.poll` runs the script's first own init body `key`, then
    /// `after` (a flagged one as a forbidden context), and exits with 0.
    Script {
        init: DefId,
        key: Hash128,
        before: Vec<(Hash128, bool)>,
        after: Vec<(Hash128, bool)>,
        report: Option<Hash128>,
    },
    /// A test program (`crates/hd_wasm/DESIGN.md` §15.4): `hd.init.N`
    /// runs module `N`'s `inits[N]`, each a forbidden context, and
    /// `hd.test.N` runs `cases[N]`, an item, its instance key and the
    /// report of its result, as `hd.poll` runs `main`.
    Tests {
        cases: Vec<(DefId, Hash128, Option<Hash128>)>,
        inits: Vec<Vec<Hash128>>,
    },
    /// An entry this backend does not build yet: scenarios and REPL
    /// inputs.
    Unsupported(&'static str),
}

impl Entry {
    /// Whether `item` is a root of the entry that only the entry calls:
    /// `main`, or a test case.
    #[must_use]
    pub fn root(&self, item: DefId) -> bool {
        match self {
            Entry::Main { main, .. } => *main == item,
            Entry::Tests { cases, .. } => cases.iter().any(|(def, _, _)| *def == item),
            Entry::Script { .. } | Entry::Unsupported(_) => false,
        }
    }
}

/// What [`build`] reads: the program's instances in content order, with
/// each one's TIR body and collection's call targets.
pub struct Input<'a> {
    pub pool: &'a InternPool,
    pub env: &'a dyn ProgramEnv,
    /// An item's stable path, for function names.
    pub path: &'a dyn Fn(DefId) -> String,
    pub collected: &'a Collected,
    /// The instances in content order (`InstanceTable::content_order`).
    pub order: &'a [InstId],
    /// Each instance's TIR body, by instance: its instance body (a
    /// default argument's or a plan's), its supplied body, or its item's.
    pub body: &'a dyn Fn(InstId) -> Option<&'a Body>,
    pub entry: Entry,
    /// The release profile: integer arithmetic wraps and shift counts are
    /// masked (`types.arith.release`).
    pub release: bool,
}

/// Builds the program's module, or says which construct the linear
/// backend does not lower yet.
pub fn build(input: &Input<'_>) -> StageResult<Vec<u8>> {
    let mut m = module::Module::default();
    for (slot, id) in input.order.iter().enumerate() {
        let f = emit::instance(input, &mut m, slot, *id)?;
        m.define(Sym::Inst(input.collected.table.key[id.idx()]), f);
    }
    rt::entry(input, &mut m)?;
    m.finish()
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(
        Stage::Link,
        format!("{} in the linear backend", what.into()),
    ))
}

#[cfg(test)]
mod tests;
