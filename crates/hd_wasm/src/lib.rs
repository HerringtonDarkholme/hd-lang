#![forbid(unsafe_code)]
//! `hd_wasm`: the linear-memory backend (`crates/hd_wasm/DESIGN.md`
//! §23). Values live inline in Wasm linear memory and counted objects are
//! freed by reference counting. This is the sole code generator.
//!
//! [`build`] makes one whole program's module from collection's output:
//! every instance's TIR body becomes one function (`emit`), the runtime
//! pieces the compiler generates are helpers (`rt`), and `module` numbers
//! and encodes them. There is no per-instance code cache yet: the driver
//! calls [`build`] once per program, at link.

/// Counts one emitter decision under a key (`scripts/ledger.ts`): `"key"`
/// or a `format!` key, `if cond => key`, or `"key" += n`. Without the
/// `ledger` feature it expands to nothing.
macro_rules! ledger {
    (flush) => {{ #[cfg(feature = "ledger")] crate::ledger::flush() }};
    (if $on:expr => $a:literal else $($rest:tt)+) => {{
        #[cfg(feature = "ledger")]
        if $on { ledger!($a) } else { ledger!($($rest)+) }
    }};
    (if $on:expr => $($key:tt)+) => {{ #[cfg(feature = "ledger")] if $on { ledger!($($key)+) } }};
    ($key:literal += $n:expr) => {{ #[cfg(feature = "ledger")] crate::ledger::add($key, $n) }};
    ($($key:tt)+) => {{ #[cfg(feature = "ledger")] crate::ledger::add(&format!($($key)+), 1) }};
}

mod boundary;
mod bulk;
#[cfg(feature = "checked-report")]
mod count_audit;
mod count_tests;
mod cycle;
mod cycle_graph;
mod drive;
mod emit;
mod frame_referents;
mod layout;
#[cfg(feature = "ledger")]
mod ledger;
mod map;
mod module;
mod op;
mod opt;
mod origins;
mod rt;
mod scan;
mod static_data;
pub mod suspension_work;
mod tables;
mod task;
#[cfg(feature = "testing")]
pub mod testing;
use module::units;
pub mod views;
mod wide;
pub mod work;

use hd_base::{DefId, Hash128, InstId, StageResult};
use hd_mono::{Collected, ProgramEnv};
use hd_tir::ir::Body;
use hd_types::{InternPool, Ty};

pub use boundary::package_module;
#[cfg(feature = "checked-report")]
pub use count_audit::resolve as resolve_count_audit;
pub use emit::{Settled, settled};
pub use module::{Session, Sym};
pub use origins::{FunctionOrigins, InstructionOrigin, MappedModule, StaticData};
pub use rt::{DISPATCH_ARMS, Helper};

/// An intrinsic's lowering crosses a runtime boundary instead of being
/// ordinary local Wasm work. Shared by emission and inline costing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicBoundary {
    Host,
    Vm,
}

#[must_use]
pub fn intrinsic_boundary(key: &str) -> Option<IntrinsicBoundary> {
    match key {
        "format_f64" | "format_f32" | "format_f64_fixed" | "string_lower" | "string_upper"
        | "parse_f64" | "snapshot_update" => Some(IntrinsicBoundary::Host),
        "mem_grow" => Some(IntrinsicBoundary::Vm),
        _ => None,
    }
}

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
        /// A program that sends crossings to workers: `worker_serve` and its
        /// instance, which `hd.worker` runs in a worker instance after
        /// `hd.init` (`std-remote.workers.serve`).
        worker: Option<(DefId, Hash128)>,
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
    /// A test program (`crates/hd_wasm/DESIGN.md` §15.4):
    /// `hd.test.init(N)` runs the units group `N` reaches in `graph`, in
    /// unit order, each init a forbidden context, and `hd.test(N, ctx)`
    /// runs `cases[N]`, an item, its instance key and the report of its
    /// result, as `hd.poll` runs `main`.
    Tests {
        cases: Vec<(DefId, Hash128, Option<Hash128>)>,
        /// Case items indexed once for entry-root membership queries.
        case_items: hd_base::HashSet<DefId>,
        /// Each unit's inits (a module cycle's), in run order: a unit
        /// runs after every unit it reaches.
        units: Vec<Vec<Hash128>>,
        /// The unit graph as `u32` words: per test module (group) an
        /// offset (`groups + 1` words from word 0), each group's root
        /// units, then per unit an offset (`units + 1` words from word
        /// `unit_offsets`) and each unit's direct dependencies. Every
        /// offset is a word index into `graph`.
        graph: Vec<u32>,
        unit_offsets: u32,
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
    /// Development-only fixture bodies, retained without executable exports.
    /// Every supplied instance and its suspension parts survive link pruning.
    Fixture { roots: Vec<Hash128> },
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
        work::entry_roots(1);
        match self {
            Entry::Main { main, .. } | Entry::Scenario { main, .. } => *main == item,
            Entry::Tests { case_items, .. } => case_items.contains(&item),
            Entry::Script { .. } | Entry::Inputs(_) | Entry::Fixture { .. } => false,
        }
    }
}

/// What [`build`] reads: the program's instances in content order, with
/// each one's TIR body and collection's call targets.
/// [`Input::shim`].
pub type ShimFn<'a> = dyn Fn(
        Ty,
        hd_layout::shims::Shim,
        hd_layout::shims::Portion,
        bool,
    ) -> StageResult<(
        hd_layout::shims::Count,
        Hash128,
        Option<std::sync::Arc<Body>>,
    )> + Sync
    + 'a;

#[derive(Clone)]
pub struct Input<'a> {
    pub pool: &'a InternPool,
    pub env: &'a (dyn ProgramEnv + Sync),
    /// What counting a type takes, and its retain or release shim
    /// (`hd_layout::shims`), made once per run: `None` when it counts
    /// nothing or one address.
    pub shim: &'a ShimFn<'a>,
    /// An item's stable path, for function names.
    pub path: &'a (dyn Fn(DefId) -> String + Sync),
    /// Declared package host roots, and identities for their returned handles.
    pub package_host: &'a (dyn Fn(DefId) -> Option<hd_host_abi::schema::Identity> + Sync),
    pub package_trait: &'a (dyn Fn(DefId) -> Option<hd_host_abi::schema::Identity> + Sync),
    pub package_selectors: &'a (dyn Fn(DefId) -> Vec<String> + Sync),
    /// The C table of a package host root bound to a C provider
    /// (`module.host.package.c-types`), whose methods lower as C calls.
    pub package_c: &'a (dyn Fn(DefId) -> Option<hd_host_abi::c::Table> + Sync),
    pub collected: &'a Collected,
    /// The instances in content order (`InstanceTable::content_order`).
    pub order: &'a [InstId],
    /// Each instance's TIR body, by instance: its instance body (a
    /// default argument's or a plan's), its supplied body, or its item's.
    pub body: &'a (dyn Fn(InstId) -> Option<&'a Body> + Sync),
    pub boundary:
        &'a (dyn Fn(InstId) -> Option<std::sync::Arc<hd_tir::own::boundary::Boundary>> + Sync),
    /// The private `std.num` implementations of floating power and
    /// remainder, which Wasm has no native instructions for.
    pub float_ops: FloatOps,
    pub entry: Entry,
    /// The release profile: integer arithmetic wraps and shift counts are
    /// masked (`types.arith.release`).
    pub release: bool,
    /// Whether the optimized pipeline is selected, independently of arithmetic
    /// and count-auditing profiles. Whole-program decoder proofs run only here.
    pub optimized: bool,
    /// Whether the optimized pipeline promotes arguments (`arg_promote`,
    /// `crates/hd_wasm/DESIGN.md` §23.40): emission then reads a
    /// local's value for a primitive comparison's reference to it.
    pub promote_args: bool,
    /// The required std runtime instances used by allocation and counting.
    pub runtime: Runtime,
    /// The `std.rt.host_provider` instance per entry row key whose host
    /// provider is installed behind a reference (`req.ref.host.reference-values`).
    /// Whether the count fast paths on known block starts are emitted
    /// inline (`opt::inline_counts`): the wasmtime pipeline's choice.
    pub inline_counts: bool,
    pub host_refs: &'a [(Ty, Hash128)],
    /// The facts the ownership passes read (`hd_tir::own::OwnCx`). Every
    /// body went through the passes (`hd_tir::own`), so emission lowers
    /// their read modes, `Drop`s and exit sets and decides no count itself
    /// (`docs/compiler/tir-v2.md`, "The ownership passes"); from these
    /// facts it learns
    /// which use consumes each value (`hd_tir::own::consumers`).
    pub own_cx: &'a (dyn hd_tir::own::OwnCx + Sync),
    /// The whole-program scan (`hd_opt::whole::mutation_targets`): concrete
    /// types whose existing heap objects can be written, or `None` when any
    /// may. Called only for closed-world optimized release links.
    pub mutation_targets: &'a (dyn Fn() -> Option<hd_base::HashSet<Ty>> + Sync),
    /// The flag methods on a call cycle (`hd_opt::whole::coinductive_flags`,
    /// `annot.bound.recursive.flag`): each one's code answers a call made
    /// while the same instance is evaluating with `true`.
    pub coinductive_flags: &'a hd_base::HashSet<Hash128>,
    /// The bang instances that complete at once
    /// (`hd_opt::whole::completes_at_once`, #2419): a bang call of one is
    /// a plain call of its body, not a suspension point.
    pub ready: &'a hd_base::HashSet<Hash128>,
    /// The store of emitted units a dev link reads and fills (#2423),
    /// `None` where a link may not share one.
    pub units: Option<&'a dyn UnitStore>,
}

/// The store of emitted units (#2423): what one instance's emission made,
/// kept between links, as rustc keeps a codegen unit's work product. The
/// driver offers it only to a dev link, whose emission reads no summary
/// of another body: every input of a unit's emission is then in its key
/// or in the reads it records (`crates/hd_wasm/DESIGN.md` §23.41).
pub trait UnitStore: Sync {
    /// What every unit's key holds of the run: the toolchain, the
    /// pipeline and profile, and the program's declarations.
    fn fingerprint(&self) -> Hash128;
    /// The hash of everything instance `id`'s body holds but its node
    /// indices (`hd_tir::wire::content_hash`), as emission reads it.
    fn body_hash(&self, id: InstId) -> Option<Hash128>;
    /// The unit stored under `key`: the reads it recorded, its types and
    /// its bytes.
    fn get(&self, key: Hash128) -> Option<StoredUnit>;
    fn put(&self, key: Hash128, unit: &StoredUnit);
    /// Whether each reused unit is emitted again and compared, as the
    /// TIR verifier's policy says (rustc's `-Z incremental-verify-ich`).
    fn verify(&self) -> bool;
    /// What the link did: the item paths of the units it emitted, in
    /// emission order, and how many it took from the store.
    fn report(&self, emitted: Vec<String>, reused: usize);
}

/// One stored unit ([`UnitStore`]).
pub struct StoredUnit {
    /// Each other instance its emission read, by key, and that
    /// instance's content hash then, sorted by key.
    pub reads: Vec<(Hash128, Hash128)>,
    pub tys: Vec<Ty>,
    pub bytes: Vec<u8>,
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

/// The checked runtime (`crates/hd_wasm/DESIGN.md` §23.24): the same nine
/// functions over the allocator, then the two the exit audit calls and the
/// two count totals the entry exports.
pub const CHECKED_MODULE: &str = "std.rt.checked";
/// The checked report's hooks, linked only when a checked build asks for
/// its report (`report`): a build that does not emits what a compiler
/// without the `checked-report` feature emits (#2308).
pub const REPORT_MODULE: &str = "std.rt.checked_report";
pub const CHECKED_FNS: &[&str] = &[
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

/// [`REPORT_MODULE`]'s functions, in [`Report`]'s order.
#[cfg(feature = "checked-report")]
const REPORT_FNS: &[&str] = &[
    "collected",
    "pointer",
    "length",
    "count_mark",
    "count_mark_generated",
    "count_observe",
    "count_header",
    "count_escape",
    "count_escape_range",
    "count_pairs_pointer",
    "count_pairs_length",
];
#[cfg(not(feature = "checked-report"))]
const REPORT_FNS: &[&str] = &[];

/// Checked-only metrics follow the ordinary checked entry points. Keep
/// these at the end so optional reporter hooks retain their existing slots.
pub const CHECKED_METRICS_FNS: [&str; 5] = [
    "observe_unique",
    "unique_checks",
    "no_op_counts",
    "skipped_test",
    "skipped_tests",
];

/// The cycle collector (`crates/hd_wasm/DESIGN.md` §23.28), which both
/// runtimes link: `collect`, `due`, `opaque` and `visit`.
pub const CYCLE_MODULE: &str = "std.rt.cycle";
pub const CYCLE_FNS: [&str; 5] = ["collect", "due", "opaque", "visit", "marking"];

/// The ordinary std implementations of floating operators absent from Wasm.
/// Collection reaches each through a corresponding operator or source call.
pub const FLOAT_MODULE: &str = "std.num";
pub const FLOAT_POW_F64: &str = "float_pow_f64";
pub const FLOAT_REM_F64: &str = "float_rem_f64";
pub const FLOAT_POW_F32: &str = "libm_powf";

/// Collected floating operators' instance keys; unused implementations are absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatOps {
    pub pow: Option<Hash128>,
    pub rem: Option<Hash128>,
    pub pow_f32: Option<Hash128>,
}

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
    if checked { CHECKED_FNS } else { &RUNTIME_FNS }
}

/// Every runtime function that emitted code calls, as `(module, name)`:
/// [`runtime_fns`] of [`runtime_module`], the report's hooks when a
/// checked build asks for its `report` (and the `checked-report` feature
/// has them), checked metrics when checked, then the collector's
/// [`CYCLE_FNS`], then the allocator metadata/class paths, `block_of` and
/// `block_bytes`.
/// [`Runtime::from_keys`] reads their keys in this order.
#[must_use]
pub fn runtime_items(checked: bool, report: bool) -> Vec<(&'static str, &'static str)> {
    let module = runtime_module(checked);
    runtime_fns(checked)
        .iter()
        .map(|n| (module, *n))
        .chain(
            (checked && report)
                .then_some(REPORT_FNS)
                .into_iter()
                .flatten()
                .map(|n| (REPORT_MODULE, *n)),
        )
        .chain(
            checked
                .then_some(CHECKED_METRICS_FNS)
                .into_iter()
                .flatten()
                .map(|n| ("std.rt.checked", n)),
        )
        .chain(CYCLE_FNS.iter().map(|n| (CYCLE_MODULE, *n)))
        .chain([
            (RUNTIME_MODULE, "metadata"),
            (RUNTIME_MODULE, "alloc_class"),
            (RUNTIME_MODULE, "block_of"),
            (RUNTIME_MODULE, "block_bytes"),
        ])
        .collect()
}

/// The instance keys of the runtime functions that emitted code calls
/// (`crates/hd_wasm/DESIGN.md` §23.16): the allocator and the counts, the
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
    pub block_bytes: Hash128,
    pub cycle: Cycle,
    pub acyclic: Acyclic,
    pub checked: Option<Checked>,
    /// Private constant-class allocator entry points, unused by checked code.
    pub small: SmallAlloc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmallAlloc {
    pub metadata: Hash128,
    pub alloc: Hash128,
}

/// The runtime's free and releases without collector hooks, which a
/// program links in place of [`Runtime`]'s when none of its objects takes
/// part in cycle collection (`crates/hd_wasm/DESIGN.md` §23.28), so
/// it links no `std.rt.cycle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acyclic {
    pub free: Hash128,
    pub release: Hash128,
    pub release_known: Hash128,
}

/// The cycle collector's functions (`crates/hd_wasm/DESIGN.md` §23.28):
/// `collect(trace, release, free)`, `due()`, whether a collection is due,
/// `opaque(address)`, which marks a new frame as never entered, and
/// `visit(address)`, which takes a reference that traced glue releases, and
/// `marking()`, whether a collection traces release glue only to mark or
/// scan, when glue changes nothing but through its releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub collect: Hash128,
    pub due: Hash128,
    pub opaque: Hash128,
    pub visit: Hash128,
    pub marking: Hash128,
}

/// The checked runtime's audit functions (`crates/hd_wasm/DESIGN.md`
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
    pub observe_unique: Hash128,
    pub unique_checks: Hash128,
    pub no_op_counts: Hash128,
    pub skipped_test: Hash128,
    pub skipped_tests: Hash128,
    /// The report's hooks, when the build asked for its report.
    pub report: Option<Report>,
}

/// [`REPORT_MODULE`]'s hooks: the collector's report and the count audit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub collected: Hash128,
    pub pointer: Hash128,
    pub length: Hash128,
    pub count_audit: CountAudit,
}

/// Checked-only runtime hooks. Each address observer returns its address
/// unchanged, so inserting it preserves the surrounding Wasm stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountAudit {
    pub mark: Hash128,
    pub generated: Hash128,
    pub observe: Hash128,
    pub header: Hash128,
    pub escape: Hash128,
    pub escape_range: Hash128,
    pub pointer: Hash128,
    pub length: Hash128,
}

impl Runtime {
    /// The checked report's hooks, when this checked build links them.
    #[must_use]
    pub fn report(&self) -> Option<Report> {
        self.checked.and_then(|c| c.report)
    }

    /// The runtime from each of [`runtime_items`]' instance keys, in
    /// order, when collection reached them all.
    #[must_use]
    pub fn from_keys(keys: &[Option<Hash128>]) -> Option<Runtime> {
        let keys: Vec<Hash128> = keys.iter().copied().collect::<Option<_>>()?;
        let (keys, [metadata, alloc_class, block_of, block_bytes]) =
            keys.split_last_chunk::<4>()?;
        let (counts, rest) = keys.split_first_chunk::<9>()?;
        let base = CHECKED_FNS.len() - 9 + CHECKED_METRICS_FNS.len();
        let (checked, rest) = match rest.len() {
            n if n == CYCLE_FNS.len() => (None, rest),
            n if n == CYCLE_FNS.len() + base || n == CYCLE_FNS.len() + base + REPORT_FNS.len() => {
                let (c, rest) = rest.split_at(n - CYCLE_FNS.len());
                // The report's hooks follow the audit's four.
                let report = (c.len() > base).then(|| Report {
                    collected: c[4],
                    pointer: c[5],
                    length: c[6],
                    count_audit: CountAudit {
                        mark: c[7],
                        generated: c[8],
                        observe: c[9],
                        header: c[10],
                        escape: c[11],
                        escape_range: c[12],
                        pointer: c[13],
                        length: c[14],
                    },
                });
                (
                    Some(Checked {
                        live: c[0],
                        audit: c[1],
                        retained: c[2],
                        released: c[3],
                        observe_unique: c[c.len() - 5],
                        unique_checks: c[c.len() - 4],
                        no_op_counts: c[c.len() - 3],
                        skipped_test: c[c.len() - 2],
                        skipped_tests: c[c.len() - 1],
                        report,
                    }),
                    rest,
                )
            }
            _ => return None,
        };
        let [collect, due, opaque, visit, marking] = *rest else {
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
            block_bytes: *block_bytes,
            small: SmallAlloc {
                metadata: *metadata,
                alloc: *alloc_class,
            },
            cycle: Cycle {
                collect,
                due,
                opaque,
                visit,
                marking,
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
    layout::cached(|| link(input, false, None, None)).map(|mapped| mapped.wasm)
}

/// Builds the same module bytes as [`build`], with instruction origins separately.
pub fn build_with_origins(input: &Input<'_>) -> StageResult<MappedModule> {
    layout::cached(|| link(input, true, None, None))
}

/// Builds a REPL base and records the symbols its later modules can import.
/// The ordinary program entry points never expose this private ABI.
pub fn build_session_base(
    input: &Input<'_>,
    track_origins: bool,
) -> StageResult<(MappedModule, Session)> {
    let mut session = Session::default();
    let mapped = layout::cached(|| link(input, track_origins, Some(&mut session), None))?;
    Ok((mapped, session))
}

/// Builds an extension importing the previous modules' initialized storage.
pub fn build_session_extension(
    input: &Input<'_>,
    previous: &Session,
    track_origins: bool,
) -> StageResult<(MappedModule, Session)> {
    let mut session = Session::default();
    let mapped = layout::cached(|| link(input, track_origins, Some(&mut session), Some(previous)))?;
    Ok((mapped, session))
}

/// Every entry point runs this link with the same layout cache.
#[cfg(not(feature = "testing"))]
use link_prepared as link;

#[cfg(feature = "testing")]
fn link(
    input: &Input<'_>,
    track_origins: bool,
    session: Option<&mut Session>,
    previous: Option<&Session>,
) -> StageResult<MappedModule> {
    let Some(transformed) = testing::emit::transformed_bodies(input) else {
        return link_prepared(input, track_origins, session, previous);
    };
    // Replace final TIR before shared body facts and its ownership boundary.
    // The hook's local input keeps both consistent throughout emission.
    let body = |id| transformed.get(&id).or_else(|| (input.body)(id));
    let boundary = |id| {
        if transformed.contains_key(&id) {
            None
        } else {
            (input.boundary)(id)
        }
    };
    let test_input = Input {
        pool: input.pool,
        env: input.env,
        shim: input.shim,
        path: input.path,
        package_host: input.package_host,
        package_trait: input.package_trait,
        package_selectors: input.package_selectors,
        package_c: input.package_c,
        collected: input.collected,
        order: input.order,
        body: &body,
        boundary: &boundary,
        float_ops: input.float_ops,
        entry: input.entry.clone(),
        release: input.release,
        optimized: input.optimized,
        promote_args: input.promote_args,
        runtime: input.runtime,
        inline_counts: input.inline_counts,
        host_refs: input.host_refs,
        own_cx: input.own_cx,
        mutation_targets: input.mutation_targets,
        coinductive_flags: input.coinductive_flags,
        ready: input.ready,
        // The store's body hashes are of the bodies before the hook.
        units: None,
    };
    link_prepared(&test_input, track_origins, session, previous)
}

fn link_prepared(
    input: &Input<'_>,
    track_origins: bool,
    session: Option<&mut Session>,
    previous: Option<&Session>,
) -> StageResult<MappedModule> {
    let prepared = previous.map(|previous| {
        let mut prepared = input.clone();
        previous.prepare(&mut prepared.entry);
        prepared
    });
    let input = prepared.as_ref().unwrap_or(input);
    let mut m = module::Module::new(&input.runtime);
    let facts = m.facts_mut();
    #[cfg(feature = "checked-report")]
    {
        facts.allocation_kind_reporting =
            input.runtime.report().is_some() && !matches!(input.entry, Entry::Fixture { .. });
        facts.count_audit = input.runtime.report().is_some()
            && !matches!(input.entry, Entry::Fixture { .. } | Entry::Inputs(_))
            && session.is_none()
            && previous.is_none();
        facts.track_origins = track_origins || facts.count_audit;
    }
    #[cfg(not(feature = "checked-report"))]
    {
        facts.track_origins = track_origins;
    }
    facts.open_world = session.is_some() || matches!(input.entry, Entry::Inputs(_));
    // ruled out: hd_driver collects every REPL input, the only open-world
    // link, with all slots (`hd_mono::collect`'s `open_world`), so a later
    // input's call finds every slot it numbers.
    debug_assert!(
        !facts.open_world || input.collected.all_slots,
        "an open-world link of tables that dropped slots"
    );
    facts.init_insts.clone_from(&input.collected.inits);
    facts.threads = input.collected.threads;
    facts.release = input.release;
    facts.optimized = input.optimized;
    facts.inline_counts = input.inline_counts;
    for id in input.order {
        let key = input.collected.table.key[id.idx()];
        facts.index_instance(key, *id);
        let item = input.collected.table.item[id.idx()];
        if (input.path)(item).starts_with("std/") {
            facts.std_insts.insert(key);
        }
        if (input.path)(item).starts_with("std/rt.cycle/") {
            facts.cycle_insts.insert(key);
        }
        if runtime_item(&(input.path)(item)) {
            facts.rt_insts.insert(key);
        }
        if input.env.cold(item) {
            facts.cold_insts.insert(key);
        }
    }
    let _acyclic = cycle::mutation_targets(
        (input.release && input.optimized && !m.open_world)
            .then(|| (input.mutation_targets)())
            .flatten(),
    );
    let span = hd_base::trace::span("phase", || "link: emit prepare".to_owned());
    address_globals(input, &mut m, session.is_some(), previous)?;
    drive::collect(input, &mut m);
    emit::collect_loans(input, &mut m);
    emit::prepare_body_facts(input, &mut m);
    emit::prepare_indirect(input, &mut m);
    drop(span);
    let span = hd_base::trace::span("phase", || "link: emit instances".to_owned());
    // Each instance emits on a module of its own, on the task's threads;
    // the merge, in order, numbers what they added as one serial
    // emission would have (`Module::merge`), so the bytes do not depend
    // on the thread count.
    let facts = m.shared_facts();
    let mut units: Vec<_> = input
        .order
        .iter()
        .map(|&id| (id, m.body_facts.remove(&id), None, false))
        .collect();
    // A pool thread keeps one layout cache for this link's units, as
    // the link's own thread has its own (`hd_layout::cached_for_run`).
    let layouts = {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        1 << 63 | NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    // A dev link takes each unit whose key and reads are unchanged from
    // the store, and stores each unit it emits (#2423, §23.41).
    let keys = input
        .units
        .filter(|_| session.is_none() && previous.is_none())
        .map(|store| units::Keys::new(input, store, &facts));
    hd_sched::par_each_mut(&mut units, |_, (id, cell, out, reused)| {
        let mut cell = cell.take().map(|cell| (*id, cell));
        let mut emit = || {
            hd_layout::cached_for_run(layouts, || {
                module::Module::emit_unit(&facts, cell.take(), |m| emit::instance(input, m, *id))
            })
        };
        let Some((keys, key)) = keys.as_ref().and_then(|k| Some((k, k.key(*id)?))) else {
            *out = Some(emit());
            return;
        };
        if let Some(stored) = keys.store.get(key)
            && keys.fresh(&facts, &stored.reads)
            && let Some(unit) = module::wire::decode(&stored.bytes, &stored.tys)
        {
            if keys.store.verify() {
                let again = module::wire::encode(&emit());
                assert!(
                    again.is_some_and(|(bytes, tys)| bytes == stored.bytes && tys == stored.tys),
                    "a stored unit of {} differs from its emission again",
                    (input.path)(input.collected.table.item[id.idx()])
                );
            }
            *reused = true;
            *out = Some(unit);
            return;
        }
        let emitted = emit();
        if let Some((bytes, tys)) = module::wire::encode(&emitted) {
            let own = input.collected.table.key[id.idx()];
            if let Some(reads) = keys.reads(&facts, own, emitted.take_reads()) {
                keys.store.put(key, &StoredUnit { reads, tys, bytes });
            }
        }
        *out = Some(emitted);
    });
    drop(facts);
    if let Some(keys) = &keys {
        let reused = units.iter().filter(|u| u.3).count();
        let emitted = units.len() - reused;
        drop(hd_base::trace::span("phase", || {
            format!("link: units emitted {emitted}, reused {reused}")
        }));
        let t = &input.collected.table;
        keys.store.report(
            units
                .iter()
                .filter(|u| !u.3)
                .map(|u| (input.path)(t.item[u.0.idx()]))
                .collect(),
            reused,
        );
    }
    for (id, _, emitted, _) in units {
        // ruled out: `par_each_mut` runs every unit.
        let emitted = emit::required(emitted, "an emitted unit");
        let f = m
            .merge(emitted, |m| emit::instance(input, m, id))
            .map_err(|error| {
                locate_layout_error(error, || {
                    input
                        .env
                        .source_span(input.collected.table.item[id.idx()], hd_base::NodeIdx::NONE)
                })
            })?;
        m.define(Sym::Inst(input.collected.table.key[id.idx()]), f);
    }
    drop(span);
    let span = hd_base::trace::span("phase", || "link: entry, shims".to_owned());
    rt::entry(input, &mut m)?;
    emit::shims(input, &mut m)?;
    #[cfg(feature = "checked-report")]
    if let Some(r) = m.runtime.report()
        && m.count_audit
    {
        let h = r.count_audit;
        m.retain_instances(&[
            h.mark,
            h.generated,
            h.observe,
            h.header,
            h.escape,
            h.escape_range,
        ]);
    }
    if let Some(session) = session {
        // Retain the base runtime even when input 1 does not use a facility
        // that a later input needs. Its state must have one owner.
        let runtime: Vec<_> = input
            .order
            .iter()
            .filter_map(|id| {
                (input.path)(input.collected.table.item[id.idx()])
                    .starts_with("std/rt.")
                    .then_some(input.collected.table.key[id.idx()])
            })
            .collect();
        m.retain_instances(&runtime);
        if previous.is_none() {
            // Extension constants are static storage, not counted heap
            // objects. The private bump allocator owns pages outside std's
            // object allocator, so block_of never retains or frees them.
            m.export("hd.session.alloc", Sym::Helper(Helper::Alloc));
        }
        let (mapped, mut linked) = if let Some(previous) = previous {
            m.finish_extension(previous)?
        } else {
            m.finish_session(true)?
        };
        linked.complete(&input.entry);
        *session = linked;
        return Ok(mapped);
    }
    drop(span);
    let _span = hd_base::trace::span("phase", || "link: finish".to_owned());
    if track_origins {
        m.finish_with_origins()
    } else {
        m.finish().map(|wasm| MappedModule {
            wasm,
            functions: Vec::new(),
        })
    }
}

/// Attach the requesting item's position when layout has no finer source
/// context. Successful emission performs no source-position lookup.
fn locate_layout_error(
    error: hd_base::NotImplemented,
    source: impl FnOnce() -> Option<hd_base::Span>,
) -> hd_base::NotImplemented {
    if error.too_large
        && error.span.is_none()
        && let Some(span) = source()
    {
        return error.at(span);
    }
    error
}

fn global_source_span(input: &Input<'_>, global: DefId) -> Option<hd_base::Span> {
    // cost: only a failed global layout asks this. Scan existing initializer
    // roots and each unique body's rows once to find the binding's store,
    // without adding successful-build
    // metadata or decoding places. Sibling init roots may share one body;
    // initializer stores retain their declaration syns.
    let mut seen = hd_base::HashSet::default();
    for id in input.order {
        let Some(body) = (input.body)(*id).filter(|body| body.kind == hd_tir::ir::BodyKind::Init)
        else {
            continue;
        };
        if !seen.insert(std::ptr::from_ref(body)) {
            continue;
        }
        for i in 0..body.len() {
            if body.tags[i] == hd_tir::ir::Tag::Store
                && matches!(body.view(body.data[i][0]).root, hd_tir::ir::Root::Global(def, _) if def == global)
                && body.syn[i] != hd_base::NodeIdx::NONE
                && let Some(span) = input.env.source_span(body.item, body.syn[i])
            {
                return Some(span);
            }
        }
    }
    input.env.source_span(global, hd_base::NodeIdx::NONE)
}

/// Gives each top-level binding that some reference refers to a static
/// slot of its layout, which its reads and writes use from then on
/// (`crates/hd_wasm/DESIGN.md` §23.20): a reference is its address.
fn address_globals(
    input: &Input<'_>,
    m: &mut module::Module,
    session: bool,
    previous: Option<&Session>,
) -> StageResult<()> {
    // The bindings a place writes: a store, a change in place or a `&mut`,
    // through the binding or a read of it.
    let mut written = hd_base::HashSet::default();
    let mut storage = hd_base::HashMap::default();
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        let roots = storage
            .entry(std::ptr::from_ref(b))
            .or_insert_with(|| emit::StorageRoots::new(b));
        for i in 0..b.len() {
            let write = match b.tags[i] {
                hd_tir::ir::Tag::Store | hd_tir::ir::Tag::IsUnique => true,
                hd_tir::ir::Tag::Ref => layout::ref_mutable(input.pool, input.env, b.ty[i]),
                _ => false,
            };
            if let Some(p) = write.then(|| b.decode_place(b.data[i][0])).flatten()
                && let Some((def, _)) = emit::global_root(roots, &p)
            {
                written.insert(input.env.path_hash(def));
            }
        }
    }
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        let roots = storage
            .entry(std::ptr::from_ref(b))
            .or_insert_with(|| emit::StorageRoots::new(b));
        let receivers = emit::lent_receiver_reads(input, *id, b);
        for i in 0..b.len() {
            // Every thread reads the program's bindings, so a program that
            // starts threads keeps them on its shared heap; the runtime's
            // are each instance's own (`fn.crossing.global.runtime`).
            let global = if (session || input.collected.threads)
                && matches!(b.tags[i], hd_tir::ir::Tag::Read | hd_tir::ir::Tag::Store)
            {
                b.decode_place(b.data[i][0]).and_then(|p| match p.root {
                    hd_tir::ir::PlaceRoot::Global(def, ty)
                        if session || !runtime_item(&(input.path)(def)) =>
                    {
                        Some((def, ty))
                    }
                    _ => None,
                })
            } else if receivers.contains(&i) {
                // A receiver read lends the pointer it reads from a
                // written binding, which its call loan then holds.
                b.decode_place(b.data[i][0])
                    .and_then(|p| emit::global_root(roots, &p))
                    .filter(|(def, _)| written.contains(&input.env.path_hash(*def)))
            } else {
                referenced_global(input.env, b, roots, i, &written)
            };
            let Some((def, ty)) = global else {
                continue;
            };
            let h = input.env.path_hash(def);
            if m.addressed.contains_key(&h) {
                continue;
            }
            if session && m.track_origins {
                m.storage_names.insert(h, (input.path)(def));
            }
            // An object's slot follows a zero header (`ref.excl.alias.global`).
            // A session's bindings all have one, as a later input may write
            // one that this one only reads.
            let object = session || written.contains(&h);
            let header = if object { module::HEADER } else { 0 };
            let address = if session {
                if !previous.is_some_and(|p| p.has_binding(h)) {
                    let lay = layout::mem(input.pool, input.env, ty).map_err(|error| {
                        locate_layout_error(error, || global_source_span(input, def))
                    })?;
                    let at = m.static_slot(header + lay.size, lay.align.max(4)) + header;
                    m.facts_mut().binding_slots.insert(h, at);
                    m.label_static(at, &(input.path)(def));
                }
                module::Address::Binding(h)
            } else {
                let lay = layout::mem(input.pool, input.env, ty).map_err(|error| {
                    locate_layout_error(error, || global_source_span(input, def))
                })?;
                let at = m.static_slot(header + lay.size, lay.align.max(4)) + header;
                m.label_static(at, &(input.path)(def));
                module::Address::Static(at)
            };
            if object {
                m.facts_mut().objects.insert(h);
            }
            m.facts_mut().addressed.insert(h, address);
        }
    }
    Ok(())
}

/// Whether the item at `path` is in a runtime module, `std.rt` or one
/// under it (`module.rt.std-only`).
fn runtime_item(path: &str) -> bool {
    ["std/rt/", "std/rt."].iter().any(|p| path.starts_with(p))
}

/// The top-level binding, with its type, that reference `i` of body `b`
/// starts its place from: a `Ref` with a global root, or one rooted at a
/// read of a binding some place writes (`written`), as a loop over
/// `&mut G` makes, since an access from another function must then see
/// its loans.
fn referenced_global(
    env: &dyn ProgramEnv,
    b: &hd_tir::ir::Body,
    roots: &emit::StorageRoots,
    i: usize,
    written: &hd_base::HashSet<hd_base::Hash128>,
) -> Option<(DefId, Ty)> {
    if b.tags[i] != hd_tir::ir::Tag::Ref {
        return None;
    }
    let p = b.decode_place(b.data[i][0])?;
    match p.root {
        hd_tir::ir::PlaceRoot::Global(def, ty) => Some((def, ty)),
        _ => emit::global_root(roots, &p).filter(|(def, _)| written.contains(&env.path_hash(*def))),
    }
}
