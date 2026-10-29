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
mod drive;
mod emit;
mod layout;
mod map;
mod module;
mod op;
mod opt;
mod origins;
mod pow;
mod rem;
mod rt;
mod scan;
mod task;
pub mod views;
mod wide;

use hd_base::{DefId, Hash128, InstId, NotImplemented, Stage, StageResult};
use hd_mono::layout::{CanonMemo, canon};
use hd_mono::{Collected, ProgramEnv, Target, TargetKind};
use hd_tir::ir::Body;
use hd_types::{InternPool, Ty, TyData};

pub use module::{Session, Sym};
pub use origins::{FunctionOrigins, InstructionOrigin, MappedModule, StaticData};
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

/// Whether a value of `ty` holds a counted leaf on this backend (§23.16):
/// the ownership passes' `OwnCx::counted` for a type with no parameter.
#[must_use]
pub fn counted_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    layout::walk(pool, env, ty).is_some()
}

/// Runs `f` with the layout cache in place (a link's, when `f` runs inside
/// one): within it, [`counted_type`] and the other layout queries compute
/// each type's layout once, so a long chain of types held by value costs
/// its length, not its square, at every query.
pub fn with_layout_cache<R>(f: impl FnOnce() -> R) -> R {
    layout::cached_here(f)
}

/// The layout queries of the whole-object scan (`hd_opt::whole`), which
/// the driver answers its `ObjectLayout` with.
pub mod object_layout {
    pub use crate::layout::{Kind, field_at, kind, payload_offset, strip, variants};
}

/// Whether `ty` is a reference (`&T`, `&mut T`, a trait value
/// reference), which a parameter borrows (`layout::Pass::Borrowed`).
#[must_use]
pub fn reference_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> bool {
    matches!(layout::kind(pool, env, ty), layout::Kind::Ref { .. })
}

/// The parts of a value of `ty`, which names no parameter, laid out in
/// this backend's locals (`layout::Kind::Value`), into `out`: a newtype's
/// one field, a struct's fields, a tuple's members, or the payload of
/// variant `variant` of an enum that does not contain itself. False for
/// any other type, whose parts no `move` or `Drop` can take one by one:
/// a scalar, a string, a list, a map, a trait or function value, a
/// reference, a data value, a recursive enum. The ownership passes'
/// `OwnCx::parts`.
pub fn in_place_parts(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    ty: Ty,
    variant: Option<u32>,
    out: &mut Vec<Ty>,
) -> bool {
    out.clear();
    let mut t = ty;
    while let TyData::Mut(inner) = pool.get(t) {
        t = inner;
    }
    if let TyData::Adt { def, args } = pool.get(t)
        && env.newtype_base(def).is_some()
    {
        let Some(&base) = env.data_fields(def).unwrap_or_default().first() else {
            return false;
        };
        if variant.is_some() {
            return false;
        }
        out.push(hd_mono::subst(pool, env, def, args, base));
        return true;
    }
    if !matches!(layout::kind(pool, env, t), layout::Kind::Value) {
        return false;
    }
    let s = layout::strip(pool, env, t);
    if let TyData::Tuple { .. } = pool.get(s) {
        out.extend(layout::tuple_members(pool, s));
        return variant.is_none();
    }
    if let Some(vs) = layout::variants(pool, env, s) {
        let Some(fs) = variant.and_then(|v| vs.get(v as usize)) else {
            return false;
        };
        out.extend(fs);
        return true;
    }
    match pool.get(s) {
        TyData::Adt { def, args }
            if variant.is_none()
                && env.std_kind(def) == hd_mono::layout::StdKind::Other
                && env.is_struct(def) =>
        {
            let Some(fs) = env.data_fields(def) else {
                return false;
            };
            out.extend(
                fs.into_iter()
                    .map(|f| hd_mono::subst(pool, env, def, args, f)),
            );
            true
        }
        _ => false,
    }
}

/// `ty` with newtypes erased and `mut` dropped, as the layouts read it
/// (`layout::strip`).
#[must_use]
pub fn strip_type(pool: &InternPool, env: &dyn ProgramEnv, ty: Ty) -> Ty {
    layout::strip(pool, env, ty)
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
    /// Development-only fixture bodies, retained without executable exports.
    /// Every supplied instance and its suspension parts survive link pruning.
    Fixture { roots: Vec<Hash128> },
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
            Entry::Script { .. }
            | Entry::Inputs(_)
            | Entry::Fixture { .. }
            | Entry::Unsupported(_) => false,
        }
    }
}

/// What [`build`] reads: the program's instances in content order, with
/// each one's TIR body and collection's call targets.
#[derive(Clone)]
pub struct Input<'a> {
    pub pool: &'a InternPool,
    pub env: &'a dyn ProgramEnv,
    /// An item's stable path, for function names.
    pub path: &'a dyn Fn(DefId) -> String,
    /// Declared package host roots, and identities for their returned handles.
    pub package_host: &'a dyn Fn(DefId) -> Option<hd_host_abi::schema::Identity>,
    pub package_trait: &'a dyn Fn(DefId) -> Option<hd_host_abi::schema::Identity>,
    pub package_selectors: &'a dyn Fn(DefId) -> Vec<String>,
    pub collected: &'a Collected,
    /// The instances in content order (`InstanceTable::content_order`).
    pub order: &'a [InstId],
    /// Each instance's TIR body, by instance: its instance body (a
    /// default argument's or a plan's), its supplied body, or its item's.
    pub body: &'a dyn Fn(InstId) -> Option<&'a Body>,
    /// Concrete `std.json.encode`/`encode_pretty` instances whose selected
    /// `Serialize` walk is statically regular, and the two private fallback
    /// functions. Emission makes those instances' fallback arms unreachable,
    /// so final reachability does not root the tree writer.
    pub direct_json_encoders: &'a hd_base::HashSet<Hash128>,
    pub json_fallbacks: [DefId; 2],
    pub entry: Entry,
    /// The release profile: integer arithmetic wraps and shift counts are
    /// masked (`types.arith.release`).
    pub release: bool,
    /// Whether the optimized pipeline is selected, independently of arithmetic
    /// and count-auditing profiles. Whole-program decoder proofs run only here.
    pub optimized: bool,
    /// The instances of std's runtime that counting calls. Without them a
    /// module counts nothing and allocates from a bump pointer.
    pub runtime: Option<Runtime>,
    /// The `std.rt.host_provider` instance per entry row key whose host
    /// provider is installed behind a reference (`req.ref.host.reference-values`).
    /// Whether the count fast paths on known block starts are emitted
    /// inline (`opt::inline_counts`): the wasmtime pipeline's choice.
    pub inline_counts: bool,
    pub host_refs: &'a [(Ty, Hash128)],
    /// The facts the ownership passes read (`hd_tir::own::OwnCx`). Every
    /// body went through the passes (`hd_tir::own`), so emission lowers
    /// their read modes, `Drop`s and exit sets and decides no count itself
    /// (`docs/compiler/tir-v2.md`, "Stage 2"); from these facts it learns
    /// which use consumes each value (`hd_tir::own::consumers`).
    pub own_cx: &'a dyn hd_tir::own::OwnCx,
    /// The whole-object scan (`hd_opt::whole::interior_targets`): the
    /// reference targets some counted reference may point inside, or
    /// `None` when any may. Called only when the link uses the fact.
    pub interior_targets: &'a dyn Fn() -> Option<hd_base::HashSet<Ty>>,
    /// The whole-program scan (`hd_opt::whole::mutation_targets`): concrete
    /// types whose existing heap objects can be written, or `None` when any
    /// may. Called only for closed-world optimized release links.
    pub mutation_targets: &'a dyn Fn() -> Option<hd_base::HashSet<Ty>>,
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
#[cfg(not(feature = "checked-report"))]
pub const CHECKED_MODULE: &str = "std.rt.checked";
#[cfg(feature = "checked-report")]
pub const CHECKED_MODULE: &str = "std.rt.checked_report";
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
    #[cfg(feature = "checked-report")]
    "collected",
    #[cfg(feature = "checked-report")]
    "pointer",
    #[cfg(feature = "checked-report")]
    "length",
];

/// The cycle collector (`crates/hd_wasm_linear/DESIGN.md` §23.28), which both
/// runtimes link: `collect`, `due`, `opaque` and `visit`.
pub const CYCLE_MODULE: &str = "std.rt.cycle";
pub const CYCLE_FNS: [&str; 4] = ["collect", "due", "opaque", "visit"];

/// The on-demand weak-reference side table. Its functions are collected with
/// the allocator roots, but ordinary programs emit none because no generated
/// helper reaches them.
pub const WEAK_MODULE: &str = "std.rt.weak";
pub const WEAK_FNS: [&str; 6] = ["lookup", "bind", "forget", "clear", "type_of", "type_bind"];

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
/// [`runtime_fns`] of [`runtime_module`], then the collector's
/// [`CYCLE_FNS`], then the allocator metadata/class paths and `block_of`.
/// [`Runtime::from_keys`] reads their keys in this order.
#[must_use]
pub fn runtime_items(checked: bool) -> Vec<(&'static str, &'static str)> {
    let module = runtime_module(checked);
    runtime_fns(checked)
        .iter()
        .map(|n| {
            #[cfg(feature = "checked-report")]
            let module = if checked && !matches!(*n, "collected" | "pointer" | "length") {
                "std.rt.checked"
            } else {
                module
            };
            (module, *n)
        })
        .chain(CYCLE_FNS.iter().map(|n| (CYCLE_MODULE, *n)))
        .chain(WEAK_FNS.iter().map(|n| (WEAK_MODULE, *n)))
        .chain([
            (RUNTIME_MODULE, "metadata"),
            (RUNTIME_MODULE, "alloc_class"),
            (RUNTIME_MODULE, "block_of"),
        ])
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
    pub weak: Weak,
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
/// part in cycle collection (`crates/hd_wasm_linear/DESIGN.md` §23.28), so
/// it links no `std.rt.cycle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acyclic {
    pub free: Hash128,
    pub release: Hash128,
    pub release_known: Hash128,
}

/// The cycle collector's functions (`crates/hd_wasm_linear/DESIGN.md` §23.28):
/// `collect(trace, release, free)`, `due()`, whether a collection is due,
/// `opaque(address)`, which marks a new frame as never entered, and
/// `visit(address)`, which takes a reference that traced glue releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub collect: Hash128,
    pub due: Hash128,
    pub opaque: Hash128,
    pub visit: Hash128,
}

/// The weak side table's raw entry lookup and lifecycle operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weak {
    pub lookup: Hash128,
    pub bind: Hash128,
    pub forget: Hash128,
    pub clear: Hash128,
    pub type_of: Hash128,
    pub type_bind: Hash128,
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
    #[cfg(feature = "checked-report")]
    pub collected: Hash128,
    #[cfg(feature = "checked-report")]
    pub pointer: Hash128,
    #[cfg(feature = "checked-report")]
    pub length: Hash128,
}

impl Runtime {
    /// The runtime from each of [`runtime_items`]' instance keys, in
    /// order, when collection reached them all.
    #[must_use]
    pub fn from_keys(keys: &[Option<Hash128>]) -> Option<Runtime> {
        let keys: Vec<Hash128> = keys.iter().copied().collect::<Option<_>>()?;
        let (keys, [metadata, alloc_class, block_of]) = keys.split_last_chunk::<3>()?;
        let (
            keys,
            [
                weak_lookup,
                weak_bind,
                weak_forget,
                weak_clear,
                weak_type_of,
                weak_type_bind,
            ],
        ) = keys.split_last_chunk::<6>()?;
        let (counts, rest) = keys.split_first_chunk::<9>()?;
        let (checked, rest) = match rest.len() {
            n if n == CYCLE_FNS.len() => (None, rest),
            n if n == CYCLE_FNS.len() + CHECKED_FNS.len() - 9 => {
                let (c, rest) = rest.split_at(CHECKED_FNS.len() - 9);
                (
                    Some(Checked {
                        live: c[0],
                        audit: c[1],
                        retained: c[2],
                        released: c[3],
                        #[cfg(feature = "checked-report")]
                        collected: c[4],
                        #[cfg(feature = "checked-report")]
                        pointer: c[5],
                        #[cfg(feature = "checked-report")]
                        length: c[6],
                    }),
                    rest,
                )
            }
            _ => return None,
        };
        let [collect, due, opaque, visit] = *rest else {
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
            small: SmallAlloc {
                metadata: *metadata,
                alloc: *alloc_class,
            },
            cycle: Cycle {
                collect,
                due,
                opaque,
                visit,
            },
            weak: Weak {
                lookup: *weak_lookup,
                bind: *weak_bind,
                forget: *weak_forget,
                clear: *weak_clear,
                type_of: *weak_type_of,
                type_bind: *weak_type_bind,
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
pub fn build_session_base(input: &Input<'_>) -> StageResult<(Vec<u8>, Session)> {
    let mut session = Session::default();
    let mapped = layout::cached(|| link(input, false, Some(&mut session), None))?;
    Ok((mapped.wasm, session))
}

/// Builds an extension importing the previous modules' initialized storage.
pub fn build_session_extension(
    input: &Input<'_>,
    previous: &Session,
) -> StageResult<(Vec<u8>, Session)> {
    let mut session = Session::default();
    let mapped = layout::cached(|| link(input, false, Some(&mut session), Some(previous)))?;
    Ok((mapped.wasm, session))
}

/// Every entry point runs this link with the same layout cache.
fn link(
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
    let mut m = module::Module::default();
    m.track_origins = track_origins;
    m.open_world = session.is_some() || matches!(input.entry, Entry::Inputs(_));
    let mut canons = CanonMemo::default();
    for calls in &input.collected.calls {
        for target in calls.values() {
            let (Target::Call(call) | Target::Default { call, .. }) = target else {
                continue;
            };
            let TargetKind::Intrinsic(key) = &call.kind else {
                continue;
            };
            if !matches!(key.as_str(), "weak_new" | "weak_new_mut") {
                continue;
            }
            m.uses_weak = true;
            if let Some(&ty) = input.pool.list_items(call.args).first() {
                m.weak_types.insert(canon(
                    input.pool,
                    &|d| input.env.path_hash(d),
                    &mut canons,
                    ty,
                ));
            }
        }
    }
    m.runtime = input.runtime;
    m.release = input.release;
    m.inline_counts = input.inline_counts;
    for id in input.order {
        let key = input.collected.table.key[id.idx()];
        m.inst_of.insert(key, *id);
        let item = input.collected.table.item[id.idx()];
        if (input.path)(item).starts_with("std/rt.") {
            m.runtime_insts.insert(key);
        }
        if (input.path)(item).starts_with("std/rt.cycle/") {
            m.cycle_insts.insert(key);
        }
        if input.env.cold(item) {
            m.cold_insts.insert(key);
        }
    }
    // Release only (#704): the whole-program fact that a reference type's
    // counted addresses are payload starts, for every walk from here on.
    let _whole = layout::whole_refs(
        (input.optimized && !m.open_world && input.runtime.is_some())
            .then(|| (input.interior_targets)())
            .flatten(),
    );
    let _acyclic = cycle::mutation_targets(
        (input.release && input.optimized && !m.open_world && input.runtime.is_some())
            .then(|| (input.mutation_targets)())
            .flatten(),
    );
    address_globals(input, &mut m, session.is_some(), previous)?;
    drive::collect(input, &mut m);
    for id in input.order {
        let f = emit::instance(input, &mut m, *id)?;
        m.define(Sym::Inst(input.collected.table.key[id.idx()]), f);
    }
    rt::entry(input, &mut m)?;
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
    if track_origins {
        m.finish_with_origins()
    } else {
        m.finish().map(|wasm| MappedModule {
            wasm,
            functions: Vec::new(),
        })
    }
}

/// Gives each top-level binding that some reference refers to a static
/// slot of its layout, which its reads and writes use from then on
/// (`crates/hd_wasm_linear/DESIGN.md` §23.20): a reference is its address.
fn address_globals(
    input: &Input<'_>,
    m: &mut module::Module,
    session: bool,
    previous: Option<&Session>,
) -> StageResult<()> {
    for id in input.order {
        let Some(b) = (input.body)(*id) else {
            continue;
        };
        for i in 0..b.len() {
            let global =
                if session && matches!(b.tags[i], hd_tir::ir::Tag::Read | hd_tir::ir::Tag::Store) {
                    b.decode_place(b.data[i][0]).and_then(|p| match p.root {
                        hd_tir::ir::PlaceRoot::Global(def, ty) => Some((def, ty)),
                        _ => None,
                    })
                } else {
                    referenced_global(b, i)
                };
            let Some((def, ty)) = global else {
                continue;
            };
            let h = input.env.path_hash(def);
            if m.addressed.contains_key(&h) {
                continue;
            }
            let address = if session {
                if !previous.is_some_and(|p| p.has_binding(h)) {
                    let lay = layout::mem(input.pool, input.env, ty)?;
                    let at = m.static_slot(lay.size, lay.align.max(4));
                    m.binding_slots.insert(h, at);
                    m.label_static(at, &(input.path)(def));
                }
                module::Address::Binding(h)
            } else {
                let lay = layout::mem(input.pool, input.env, ty)?;
                let at = m.static_slot(lay.size, lay.align.max(4));
                m.label_static(at, &(input.path)(def));
                module::Address::Static(at)
            };
            m.addressed.insert(h, address);
        }
    }
    Ok(())
}

/// The top-level binding, with its type, that reference `i` of body `b`
/// starts its place from: a `Ref` with a global root.
fn referenced_global(b: &hd_tir::ir::Body, i: usize) -> Option<(DefId, Ty)> {
    match b.tags[i] {
        hd_tir::ir::Tag::Ref => match b.decode_place(b.data[i][0])?.root {
            hd_tir::ir::PlaceRoot::Global(def, ty) => Some((def, ty)),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> StageResult<T> {
    Err(NotImplemented::new(
        Stage::Link,
        format!("{} in the linear backend", what.into()),
    ))
}

#[cfg(test)]
mod tests;
