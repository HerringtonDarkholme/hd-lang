#![forbid(unsafe_code)]
//! `hd_wasm_linear`: the linear-memory backend (`crates/hd_wasm_linear/DESIGN.md`
//! §23). Values live inline in Wasm linear memory and counted objects are
//! freed by reference counting. This is the sole code generator.
//!
//! [`build`] makes one whole program's module from collection's output:
//! every instance's TIR body becomes one function (`emit`), the runtime
//! pieces the compiler generates are helpers (`rt`), and `module` numbers
//! and encodes them. There is no per-instance code cache yet: the driver
//! calls [`build`] once per program, at link.

mod asm;
mod boundary;
mod cycle;
mod emit;
mod layout;
mod map;
mod module;
mod opt;
mod pow;
mod rem;
mod rt;
mod task;
mod wide;

use hd_base::{DefId, Hash128, InstId, NotImplemented, Stage, StageResult};
use hd_mono::{Collected, ProgramEnv};
use hd_tir::ir::Body;
use hd_types::{InternPool, Ty};

pub use module::Sym;
pub use rt::Helper;

/// A1's shared reference-storage class on this backend: data and boxed
/// enums are object addresses; inline values retain concrete type keys.
#[must_use]
pub fn shared_reference_layout(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    matches!(
        layout::kind(pool, env, ty),
        layout::Kind::Data(_) | layout::Kind::Boxed | layout::Kind::Ref { fat: false, .. }
    )
}

/// The entry behavior of the program a module runs
/// (`crates/hd_wasm_linear/DESIGN.md` §15.4, "Init order").
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
    /// A test program (`crates/hd_wasm_linear/DESIGN.md` §15.4): `hd.init.N`
    /// runs module `N`'s `inits[N]`, each a forbidden context, and
    /// `hd.test.N` runs `cases[N]`, an item, its instance key and the
    /// report of its result, as `hd.poll` runs `main`.
    Tests {
        cases: Vec<(DefId, Hash128, Option<Hash128>)>,
        inits: Vec<Vec<Hash128>>,
        /// The inits among `inits` of std's runtime modules, which
        /// checked mode's reset after a case never runs again (§23.24).
        runtime_inits: Vec<Hash128>,
    },
    /// A runtime scenario's program (`spec/conformance/README.md`,
    /// Runtime Scenarios): `main`'s entry as [`Entry::Main`] has it, with
    /// `hd.scenario.cleanup` calling `cleanup` when the module declares
    /// it, and every poll of `hold`'s suspension answering Pending
    /// without running its body (flagging the frame active when the
    /// flag is set).
    Scenario {
        main: DefId,
        key: Hash128,
        inits: Vec<(Hash128, bool)>,
        report: Option<Hash128>,
        cleanup: Option<Hash128>,
        hold: Option<(DefId, bool)>,
    },
    /// A REPL session's inputs, all in one module, which a host runs one
    /// after another in one instance: `hd.init.N` and `hd.poll.N` run
    /// input `N` as [`Entry::Script`]'s exports run a script, with no
    /// audit at its end, since its bindings outlive it. After an input
    /// that trapped or did not end with status 0, the host calls
    /// `hd.reset`, which clears the runtime state a trap leaves set, so
    /// the next input starts as the first did.
    Inputs(Vec<ReplInput>),
    /// An entry this backend does not build.
    Unsupported(&'static str),
}

/// One input of [`Entry::Inputs`], as [`Entry::Script`] has a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplInput {
    pub number: u32,
    pub init: DefId,
    pub key: Hash128,
    pub before: Vec<(Hash128, bool)>,
    pub after: Vec<(Hash128, bool)>,
}

impl Entry {
    /// Whether the polls of `item`'s suspensions are held, and whether
    /// a held poll flags the frame active.
    #[must_use]
    pub fn held(&self, item: DefId) -> Option<bool> {
        match self {
            Entry::Scenario {
                hold: Some((def, active)),
                ..
            } if *def == item => Some(*active),
            _ => None,
        }
    }

    /// Whether `item` is a root of the entry that only the entry calls:
    /// `main`, or a test case.
    #[must_use]
    pub fn root(&self, item: DefId) -> bool {
        match self {
            Entry::Main { main, .. } | Entry::Scenario { main, .. } => *main == item,
            Entry::Tests { cases, .. } => cases.iter().any(|(def, _, _)| *def == item),
            Entry::Script { .. } | Entry::Inputs(_) | Entry::Unsupported(_) => false,
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
    /// The instances of std's runtime that counting calls. Without them a
    /// module counts nothing and allocates from a bump pointer.
    pub runtime: Option<Runtime>,
    /// The `std.rt.host_provider` instance per entry row key whose host
    /// provider is installed behind a reference (`req.ref.host.reference-values`).
    pub host_refs: &'a [(Ty, Hash128)],
}

/// The std module whose functions emitted code calls by key, and their
/// names: the driver roots them for this backend ([`Runtime`]).
pub const RUNTIME_MODULE: &str = "std.rt.alloc";
pub const RUNTIME_FNS: [&str; 9] = [
    "alloc",
    "free",
    "retain",
    "release",
    "retain_known",
    "release_known",
    "free_block",
    "release_acyclic",
    "release_known_acyclic",
];

/// The checked runtime (`crates/hd_wasm_linear/DESIGN.md` §23.24): the same nine
/// functions over the allocator, then the two the exit audit calls and the
/// two count totals the entry exports.
pub const CHECKED_MODULE: &str = "std.rt.checked";
pub const CHECKED_FNS: [&str; 13] = [
    "allocate",
    "free",
    "retain",
    "release",
    "retain_known",
    "release_known",
    "free_acyclic",
    "release_acyclic",
    "release_known_acyclic",
    "live",
    "audit",
    "retained",
    "released",
];

/// The cycle collector (`crates/hd_wasm_linear/DESIGN.md` §23.28), which both
/// runtimes link: `collect`, `due`, `track` and `opaque`.
pub const CYCLE_MODULE: &str = "std.rt.cycle";
pub const CYCLE_FNS: [&str; 5] = ["collect", "due", "track", "opaque", "visit"];

/// The runtime module a build links: the checked one in checked mode.
#[must_use]
pub const fn runtime_module(checked: bool) -> &'static str {
    if checked {
        CHECKED_MODULE
    } else {
        RUNTIME_MODULE
    }
}

/// The names of [`runtime_module`]'s functions that emitted code calls.
#[must_use]
pub const fn runtime_fns(checked: bool) -> &'static [&'static str] {
    if checked { &CHECKED_FNS } else { &RUNTIME_FNS }
}

/// Every runtime function that emitted code calls, as `(module, name)`:
/// [`runtime_fns`] of [`runtime_module`], then the collector's
/// [`CYCLE_FNS`], then the allocator's `block_of` for reference views.
/// [`Runtime::from_keys`] reads their keys in this order.
#[must_use]
pub fn runtime_items(checked: bool) -> Vec<(&'static str, &'static str)> {
    let module = runtime_module(checked);
    runtime_fns(checked)
        .iter()
        .map(|n| (module, *n))
        .chain(CYCLE_FNS.iter().map(|n| (CYCLE_MODULE, *n)))
        .chain([(RUNTIME_MODULE, "block_of")])
        .collect()
}

/// The instance keys of the runtime functions that emitted code calls
/// (`crates/hd_wasm_linear/DESIGN.md` §23.16): the allocator and the counts, the
/// collector's, and in checked mode the audit's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub alloc: Hash128,
    pub free: Hash128,
    pub retain: Hash128,
    pub release: Hash128,
    pub retain_known: Hash128,
    pub release_known: Hash128,
    pub block_of: Hash128,
    pub cycle: Cycle,
    pub acyclic: Acyclic,
    pub checked: Option<Checked>,
}

/// The runtime's free and releases without collector hooks, which a
/// program links in place of [`Runtime`]'s when none of its objects takes
/// part in cycle collection (`crates/hd_wasm_linear/DESIGN.md` §23.28), so
/// it links no `std.rt.cycle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acyclic {
    pub free: Hash128,
    pub release: Hash128,
    pub release_known: Hash128,
}

/// The cycle collector's functions (`crates/hd_wasm_linear/DESIGN.md` §23.28):
/// `collect(trace, release, free)`, `due()`, whether the candidate buffer
/// is full, `track(block)`, which zeroes a new object's collector word,
/// `opaque(address)`, which marks a new frame as never entered, and
/// `visit(address)`, which takes a reference that traced glue releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub collect: Hash128,
    pub due: Hash128,
    pub track: Hash128,
    pub opaque: Hash128,
    pub visit: Hash128,
}

/// The checked runtime's audit functions (`crates/hd_wasm_linear/DESIGN.md`
/// §23.24): `live()`, the blocks allocated and not yet freed, and
/// `audit(expected)`, which panics unless that is `expected`; `retained()`
/// and `released()`, the counts added and taken so far, which the entry
/// exports as `hd.retained` and `hd.released` for measuring elision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    pub live: Hash128,
    pub audit: Hash128,
    pub retained: Hash128,
    pub released: Hash128,
}

impl Runtime {
    /// The runtime from each of [`runtime_items`]' instance keys, in
    /// order, when collection reached them all.
    #[must_use]
    pub fn from_keys(keys: &[Option<Hash128>]) -> Option<Runtime> {
        let keys: Vec<Hash128> = keys.iter().copied().collect::<Option<_>>()?;
        let (block_of, keys) = keys.split_last()?;
        let (counts, rest) = keys.split_first_chunk::<9>()?;
        let (checked, rest) = match rest.len() {
            n if n == CYCLE_FNS.len() => (None, rest),
            n if n == CYCLE_FNS.len() + 4 => {
                let (c, rest) = rest.split_first_chunk::<4>()?;
                (
                    Some(Checked {
                        live: c[0],
                        audit: c[1],
                        retained: c[2],
                        released: c[3],
                    }),
                    rest,
                )
            }
            _ => return None,
        };
        let [collect, due, track, opaque, visit] = *rest else {
            return None;
        };
        let [
            alloc,
            free,
            retain,
            release,
            retain_known,
            release_known,
            acyclic_free,
            acyclic_release,
            acyclic_release_known,
        ] = *counts;
        Some(Runtime {
            alloc,
            free,
            retain,
            release,
            retain_known,
            release_known,
            block_of: *block_of,
            cycle: Cycle {
                collect,
                due,
                track,
                opaque,
                visit,
            },
            acyclic: Acyclic {
                free: acyclic_free,
                release: acyclic_release,
                release_known: acyclic_release_known,
            },
            checked,
        })
    }
}

/// Builds the program's module, or says which construct the linear
/// backend does not lower yet.
pub fn build(input: &Input<'_>) -> StageResult<Vec<u8>> {
    // Every layout and walk the link asks for is computed once.
    layout::cached(|| link(input))
}

/// [`build`]'s link, with the layout cache in place.
fn link(input: &Input<'_>) -> StageResult<Vec<u8>> {
    let mut m = module::Module::default();
    m.runtime = input.runtime;
    m.release = input.release;
    for id in input.order {
        let key = input.collected.table.key[id.idx()];
        m.inst_of.insert(key, *id);
        if (input.path)(input.collected.table.item[id.idx()]).starts_with("std/rt.cycle/") {
            m.cycle_insts.insert(key);
        }
    }
    address_globals(input, &mut m)?;
    for (slot, id) in input.order.iter().enumerate() {
        let f = emit::instance(input, &mut m, slot, *id)?;
        m.define(Sym::Inst(input.collected.table.key[id.idx()]), f);
    }
    rt::entry(input, &mut m)?;
    m.finish()
}

/// Gives each top-level binding that some reference refers to a static
/// slot of its layout, which its reads and writes use from then on
/// (`crates/hd_wasm_linear/DESIGN.md` §23.20): a reference is its address.
fn address_globals(input: &Input<'_>, m: &mut module::Module) -> StageResult<()> {
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        for i in 0..b.len() {
            let Some(p) = b.place(i) else {
                continue;
            };
            if p.kind != hd_tir::ir::place_root::GLOBAL {
                continue;
            }
            let h = input.env.path_hash(DefId::from_raw(p.x));
            if m.addressed.contains_key(&h) {
                continue;
            }
            let lay = layout::mem(input.pool, input.env, Ty(p.y))?;
            let at = m.static_slot(lay.size, lay.align.max(4));
            m.addressed.insert(h, at);
        }
    }
    Ok(())
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(
        Stage::Link,
        format!("{} in the linear backend", what.into()),
    ))
}

#[cfg(test)]
mod tests;
