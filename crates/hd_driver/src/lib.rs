#![forbid(unsafe_code)]
//! `hd_driver`: the one driver (design-overview.md §1, scheduler.md §6.1).
//! A run is the task graph of §6.1 over a package: skim, folder graph,
//! folder interfaces, header checks, module prep, bodies, module finish,
//! coherence, init order, package result, then `Collect`, `Emit` and
//! `Link` for a program. Every task writes its result into a per-kind slot
//! (`OnceLock`), so the serial, stepping and pool executors run the same
//! task code. Every stage boundary goes through the caller's `CacheStore`.
//!
//! The driver holds no file system, process or clock: sources, the cache
//! store, the executor and the clock come from the caller (design-overview
//! §2.2 rules 1 and 3).
//!
//! A run stops at a stage that answers "not implemented": the answer
//! becomes an internal `unsupported` diagnostic, which is an error.
//! `analyze_package` runs the same code and only reports the counts.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

pub use hd_base::BuildProfile;
use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, FileId, FolderId, Hash128, ModuleId, NotImplemented, Span, StageResult};
use hd_cache::{
    CacheStore, EntryKind, FileApi, MemoryStore, check_key, code_key, hdr_key, iface_key, init_key,
    parse_key, prog_key, toolchain_key,
};
use hd_check::BodyCx;
use hd_check::init::InputBinding;
use hd_check::stages::{ModuleFacts, test_overlay};
use hd_check::structure::FactBodies;
use hd_diag::{Code, DiagBuf, Severity};
use hd_intern::{PathTable, ShardedInterner};
use hd_mono::layout::{LayoutEnv, StdKind};
use hd_mono::{Collected, ProgramEnv};
use hd_project::{FolderGraph, MemorySources, ModuleTable, PackageIn, Role, Scope, SourceSet};
use hd_resolve::{FolderIface, Item, ItemData, Lookup, ModOut, Names, Src};
use hd_sched::{ExtTask, SerialOrder, SerialScheduler, Spawn, TaskGraph, TaskId, TaskKind};
use hd_syntax::{HeaderKind, Parse, parse, skim};
use hd_tir::Body;
use hd_types::solver::{
    Declarations, FolderImpls, GlobalMemo, ImplRef, ImplUniverseId, ImplUniverses, ImplView, Impls,
    MemberStep, OwnerMap, SealedTraits, TableSolver, TuplePlans, TypeDecl, UniverseImpls,
    folder_table,
};
use hd_types::{InternPool, Ty, TyData, TyList};
use hd_wasm::Code as WasmCode;

pub mod bench;
pub mod diagnostics;
mod positions;
mod report;

include!(concat!(env!("OUT_DIR"), "/std_files.rs"));

/// The std sources every run reads: `lib/std` as embedded at build time,
/// plus the virtual `core.hd` of the compiler-supplied `std.core` and the
/// virtual `rt.hd` of `std.rt`.
#[must_use]
pub fn std_sources() -> MemorySources {
    let mut s = MemorySources::default();
    for (path, text) in STD_FILES {
        s.insert(path, text);
    }
    s.insert("core.hd", hd_resolve::seed::CORE_SOURCE);
    s.insert("rt.hd", RT_SOURCE);
    s
}

/// `std.rt`: the hd the compiler roots after an entry point or a test case
/// returns (module.entry.exit-report, module.testing.pass): the status
/// `report()` gives, after an `.Err`'s report on standard error
/// (module.entry.err-stderr). The driver picks one by the result type:
/// `entry_status` for any `Termination`, `entry_status_display` for a
/// `Result` whose error is only `Display`, `entry_status_error` for one
/// whose error type implements `Error` and `entry_status_dyn` for the
/// erased `dyn Error`; the last two print the cause chain
/// (module.entry.err-render-chain). It is compiler-supplied, like
/// `std.core`, so the TS prototype's `lib/std` is unchanged.
pub const RT_SOURCE: &str = "\
# std.rt: the entry and test-case result functions (hd_driver::RT_SOURCE).
use std.process.Termination
use std.error.{Error, report_of}

fn entry_status[T < Termination](result: T) -> u8:
    u8(result.report())

fn entry_status_display[T < Termination, E < Display](result: Result[T, E]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            entry_write(error.to_string())
            1

fn entry_status_error[T < Termination, E < Error](result: Result[T, E]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            entry_write(report_of(error).to_string())
            1

fn entry_status_dyn[T < Termination](result: Result[T, dyn Error]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            entry_write(report_of(error).to_string())
            1

@intrinsic(\"entry_write\")
fn entry_write(text: string) -> void:
    panic(\"intrinsic\")
";

/// A source set with the virtual `core.hd` and `rt.hd` added when they are
/// missing: a run whose root package is std itself.
fn with_core(sources: &dyn SourceSet) -> MemorySources {
    let mut s = MemorySources::default();
    for e in sources.list() {
        if let Some(b) = sources.read(&e.path) {
            s.insert(&e.path, &String::from_utf8_lossy(&b));
        }
    }
    if sources.read("core.hd").is_none() {
        s.insert("core.hd", hd_resolve::seed::CORE_SOURCE);
    }
    if sources.read("rt.hd").is_none() {
        s.insert("rt.hd", RT_SOURCE);
    }
    s
}

/// The hash of a source set's paths and texts (the std part of the
/// toolchain key, cache.md §5.3).
fn sources_hash(sources: &dyn SourceSet) -> Hash128 {
    let mut h = hd_base::StableHasher::new("std-sources");
    for e in sources.list() {
        h.str(&e.path);
        if let Some(b) = sources.read(&e.path) {
            h.bytes(&b);
        }
    }
    h.finish()
}

pub use hd_base::Stage;
pub use positions::{PositionPool, Positions};
pub use report::{Counters, PipelineReport, Tally};

/// The compiler build id: `build.rs` hashes the compiler crates' sources.
const COMPILER: &str = concat!("hd 0 ", env!("HD_BUILD_ID"));
const TARGET: &str = "wasm32-gc";
const ROLE: &str = "lib";
const LAYOUT: u16 = hd_cache::LAYOUT_VERSION;

/// The caller's clock, for stage times only (never in an output).
pub trait Clock: Sync {
    fn now_ns(&self) -> u64;
}

/// Observes the stage executing on the calling thread, including pool workers.
/// `enter` returns an opaque restoration token, passed to `exit` on that same
/// thread when the stage ends (also when unwinding). Nested stages restore the
/// enclosing stage. Observers must not panic.
pub trait StageObserver: Sync {
    fn enter(&self, stage: Stage) -> usize;
    fn exit(&self, token: usize);
}

struct ObservedStage<'a> {
    observer: Option<&'a dyn StageObserver>,
    token: usize,
}

impl<'a> ObservedStage<'a> {
    fn new(observer: Option<&'a dyn StageObserver>, stage: Stage) -> Self {
        Self {
            observer,
            token: observer.map_or(0, |o| o.enter(stage)),
        }
    }
}

impl Drop for ObservedStage<'_> {
    fn drop(&mut self) {
        if let Some(observer) = self.observer {
            observer.exit(self.token);
        }
    }
}

/// A clock that reads zero: the browser and deterministic tests.
pub struct NoClock;

impl Clock for NoClock {
    fn now_ns(&self) -> u64 {
        0
    }
}

/// Which executor runs the graph (scheduler.md §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Executor {
    Serial(SerialOrder),
    /// Needs the `threads` feature.
    #[cfg(feature = "threads")]
    Pool(usize),
}

/// What the caller supplies.
pub struct Host<'a> {
    /// Item paths whose checked bodies the output renders (golden tests).
    pub render_tir: &'a [&'a str],
    pub sources: &'a dyn SourceSet,
    pub store: &'a dyn CacheStore,
    pub clock: &'a dyn Clock,
    pub executor: Executor,
}

/// What a run is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Goal {
    /// A program whose root is `fn main` of this module (its path below
    /// the package, as `main` or `app.main`).
    Program { entry: String },
    /// The package's unit-test program (engines-and-test-runner.md
    /// §19.1): every selected `it` case of its `tests:` blocks, each a
    /// root. `module` limits the cases to one module (`hd test FILE`),
    /// `filter` to the names that contain it (`cli.test.filter`).
    Tests {
        module: Option<String>,
        filter: Option<String>,
    },
    /// A scenario program: the package's entry `main` or `main!` built
    /// with the scenario exports (`hd.cancel`), which drives a runtime
    /// scenario instead of running to completion. The runner builds it,
    /// never the CLI.
    Scenario { entry: String },
    /// Every stage over a package; "not implemented" is counted.
    Analyze,
    /// `Analyze`, with each module's `tests:` blocks checked as test code
    /// into its test-role `check` entry, as a test run checks them
    /// (`hd check --tests`, `cli.check.tests`).
    CheckTests,
    /// The `input`th input of a REPL session (repl-plan.md §2.2): the root
    /// package's synthetic file `repl#N` ([`hd_project::repl_file`]), a
    /// module outside every package folder, polled as a script. The
    /// sources hold the session's earlier inputs as `repl#K` files, `K`
    /// below `input`, without the inputs that panicked
    /// (`cli.repl.panic.binding`). An input sees `src/lib.hd`'s scope with
    /// each earlier input's top-level names nested in it
    /// (`cli.repl.package.lib`). With [`Packages::session`], it links as
    /// one more module of that session, so only it is checked, emitted
    /// and linked. Input 0 is the session's prelude, which
    /// [`build_repl_prelude`] builds.
    ReplInput { input: u32 },
}

/// A test case as the test plan lists it (§19.2), in content order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestCase {
    /// The module path, as `pkg.math`.
    pub module: String,
    /// The package-relative file and the registration's 1-based line.
    pub file: String,
    pub line: u32,
    pub name: String,
    /// `it`, `it_each`, `it_prop` or `it_prop_with`.
    pub kind: String,
    pub ignore: Option<String>,
    pub expect_panic: Option<String>,
    /// Why the case cannot run yet.
    pub unsupported: Option<String>,
    /// For a case that runs: its `hd.test.i` and `hd.init.j` exports.
    pub run: Option<(u32, u32)>,
}

impl TestCase {
    /// A property test case's base seed (`cli.test.seed`): `flag` (`hd
    /// test --seed N`, `cli.test.seed.flag`) when given, else a hash of the
    /// case's name (64-bit FNV-1a, kept non-negative), so the same name
    /// gives the same seed on every run. Other cases draw nothing and have
    /// none.
    #[must_use]
    pub fn base_seed(&self, flag: Option<i64>) -> Option<i64> {
        if !matches!(self.kind.as_str(), "it_prop" | "it_prop_with") {
            return None;
        }
        Some(flag.unwrap_or_else(|| {
            let hash = self.name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            });
            i64::try_from(hash >> 1).unwrap_or(i64::MAX)
        }))
    }
}

/// Whether a package-relative file is an integration test file
/// (`module.test.integration`): a file directly under `tests/`.
#[must_use]
pub fn integration_test_file(path: &str) -> bool {
    path.strip_prefix("tests/")
        .is_some_and(|rest| !rest.contains('/'))
}

/// One program of a test run (engines-and-test-runner.md §19.1), built on
/// its own: the unit-test program, or an integration test file
/// (`module.test.integration`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestProgram {
    /// The integration test file; `None` for the unit-test program.
    pub file: Option<String>,
    /// The files its build links: FILE's module or the integration test
    /// file with what it uses (`cli.package.file`), else every file but
    /// those under `tests/`.
    pub keep: Vec<String>,
    /// The module whose cases it selects ([`Goal::Tests`]).
    pub module: Option<String>,
}

/// The programs a test run of `package` builds over `sources`: the
/// unit-test program unless `only` (`hd test FILE`) is an integration test
/// file; then each integration test file, or `only` alone when it is one.
/// When `only` is another file, a file under `tests/` is a program only if
/// `doc(path)` says it is a doc test of `only`'s (`module.test.doc.program`).
#[must_use]
pub fn test_programs(
    package: &str,
    sources: &dyn SourceSet,
    only: Option<&str>,
    doc: &dyn Fn(&str) -> bool,
) -> Vec<TestProgram> {
    let paths: Vec<String> = sources.list().into_iter().map(|e| e.path).collect();
    let mut out = Vec::new();
    if !only.is_some_and(integration_test_file) {
        out.push(TestProgram {
            file: None,
            keep: match only {
                Some(f) => use_closure(package, sources, f),
                None => paths
                    .iter()
                    .filter(|p| !p.starts_with("tests/"))
                    .cloned()
                    .collect(),
            },
            module: only.map(|f| hd_project::module_path(package, f)),
        });
    }
    for p in &paths {
        let wanted = match only {
            Some(f) if integration_test_file(f) => p == f,
            Some(_) => doc(p),
            None => integration_test_file(p),
        };
        if wanted {
            out.push(TestProgram {
                file: Some(p.clone()),
                keep: use_closure(package, sources, p),
                module: Some(hd_project::module_path(package, p)),
            });
        }
    }
    out
}

/// The result of a run.
pub struct Output {
    pub wasm: Option<Vec<u8>>,
    pub diags: DiagBuf,
    /// Package-relative file paths, indexed by a diagnostic's `FileId`.
    pub files: Vec<String>,
    /// Per file, the text the run read: the text its spans index. A
    /// dependency's file (`<std>/serde.hd`) is not in the root package's
    /// sources, so positions are located in this text.
    pub texts: Vec<Arc<str>>,
    /// Per file, the files of the modules its `use`s reach (from the
    /// skims this run made; empty for a file it did not skim).
    pub uses: Vec<Vec<usize>>,
    /// Per file, its module's path.
    pub modules: Vec<String>,
    /// Per file, its module's `check` entry, which [`Output::positions`]
    /// reads.
    checks: Vec<Option<Arc<[u8]>>>,
    /// Per file, its module's path as a panic frame names it
    /// (`cli.test.report.frame`), which [`Output::positions`] reads.
    printed: Vec<String>,
    /// Each selected test case's body, by its stable item path
    /// (`shop/$test0`), with the case's registered name.
    test_bodies: Vec<(String, String)>,
    /// The module roots that differ from their package's identifier, each
    /// with that identifier (`ModuleTable::renamed_roots`).
    pub renamed: Vec<(String, String)>,
    pub counters: Counters,
    pub report: PipelineReport,
    /// Each built folder interface's blob, by folder path.
    pub ifaces: Vec<(String, Arc<[u8]>)>,
    /// With [`Packages::doc_scopes`], each module of the root package with
    /// the names its scope binds: its own declarations and its `use`s.
    pub scopes: Vec<(String, Vec<(String, hd_resolve::Named)>)>,
    /// Readable TIR of the bodies a `Host::render_tir` named, by item path.
    pub tir_text: std::collections::BTreeMap<String, String>,
    /// A test run's selected cases, in content order.
    pub tests: Vec<TestCase>,
    /// Items in the run's global `InternPool` at the end (a size counter).
    pub pool_items: u32,
    /// The trait solver's memo hits and misses over the run (a counter;
    /// with several threads it varies from run to run).
    pub memo: hd_types::solver::MemoStats,
    /// With [`Packages::session`] and a linked module: what the module adds
    /// to the session, its exports and the inits it runs. Once the module
    /// has run, the caller adds it ([`Session::add`]) before building the
    /// next module; a module that failed adds nothing.
    pub session_added: Option<Session>,
}

impl Output {
    /// Diagnostics as `severity: file:lo..hi: code: message`, in content
    /// order. Tests use this form; the CLI uses [`Output::render_located`].
    #[must_use]
    pub fn render(&self) -> String {
        self.diags
            .render_compact(&|s: Span| format!("{}:{}..{}", self.file_name(s), s.lo, s.hi))
    }

    /// Diagnostics as `severity: file:line:column: code: message`, in content
    /// order. Line and column are 1-based; the column counts characters from
    /// the line start.
    #[must_use]
    pub fn render_located(&self) -> String {
        self.diags.render_compact(&|s: Span| {
            let (file, line, column) = self.locate(s);
            format!("{file}:{line}:{column}")
        })
    }

    /// Text for the user: each internal module path of test or task code
    /// named by its file (`hd_project::user_text`), and each package named
    /// by its name (`hd_project::rename_roots`).
    #[must_use]
    pub fn user_text(&self, text: &str) -> String {
        let text = hd_project::user_text(text, &|path| {
            let i = self.modules.iter().position(|m| m == path)?;
            self.files.get(i).cloned()
        });
        if self.renamed.is_empty() {
            text
        } else {
            hd_project::rename_roots(&text, &self.renamed)
        }
    }

    /// The package-relative file, 1-based line and column of a span's start.
    #[must_use]
    pub fn locate(&self, s: Span) -> (String, usize, usize) {
        let file = self.file_name(s);
        let (line, column) = s
            .file
            .get()
            .and_then(|_| self.texts.get(s.file.idx()))
            .map_or((1, 1), |text| line_column(text.as_bytes(), s.lo as usize));
        (file, line, column)
    }

    /// What resolves the anchors of the built program's line table to
    /// files and lines (wasm-layout.md §15.9), its modules kept in `pool`
    /// with those of the command's other programs.
    #[must_use]
    pub fn positions(&self, pool: &mut PositionPool) -> Positions {
        Positions::new(
            pool,
            &self.modules,
            &self.files,
            &self.texts,
            &self.checks,
            &self.printed,
            &self.test_bodies,
        )
    }

    fn file_name(&self, s: Span) -> String {
        s.file
            .get()
            .and_then(|_| self.files.get(s.file.idx()).cloned())
            .unwrap_or_default()
    }
}

/// The 1-based line and column of byte `offset` in `text`. The column counts
/// characters after the line's last newline.
fn line_column(text: &[u8], offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    // Each newline ends one segment, so the segment count is the line number.
    let line = before.split(|&b| b == b'\n').count();
    let start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
    (line, column)
}

/// Whether a top-level function header declares `main` or `main!`, and if
/// so whether it is `pub`. The header is read as tokens, so spacing and
/// comments between `pub`, `fn` and the name do not matter.
fn main_visibility(header: &str) -> Option<bool> {
    let lexed = hd_syntax::lex(header.as_bytes());
    let t = &lexed.tokens;
    let kinds = &t.kind;
    let public = kinds.first() == Some(&hd_syntax::TokenKind::KwPub);
    let at = usize::from(public);
    let named_main = kinds.get(at) == Some(&hd_syntax::TokenKind::KwFn)
        && kinds.get(at + 1) == Some(&hd_syntax::TokenKind::Ident)
        && header.get(t.start[at + 1] as usize..t.end[at + 1] as usize) == Some("main");
    let after = at + 2 + usize::from(kinds.get(at + 2) == Some(&hd_syntax::TokenKind::Bang));
    (named_main
        && matches!(
            kinds.get(after),
            Some(hd_syntax::TokenKind::LParen | hd_syntax::TokenKind::LBracket)
        ))
    .then_some(public)
}

struct SkimOut {
    source_hash: Hash128,
    api_text_hash: Hash128,
    token_hash: Hash128,
    code_lines: Vec<u32>,
    uses: Vec<String>,
    facts: ModuleFacts,
    /// The header span of a top-level `pub fn main` or `main!`, which
    /// `cli.exe.unselected-main` is about.
    public_main: Option<(u32, u32)>,
    /// The header span of a top-level `main` or `main!` that is not
    /// `pub` (`module.entry.private-main.warn`).
    private_main: Option<(u32, u32)>,
}

struct GraphOut {
    graph: FolderGraph,
    package_result: TaskId,
}

/// A module's own items after `ModulePrep` (a check miss).
struct PrepOut {
    items: Vec<Item>,
    diags: DiagBuf,
}

/// A folder's stage-A diagnostics, with the module each belongs to and
/// the severity each was reported with (a cache hit re-emits them).
type IfaceDiags = Vec<(usize, Code, Severity, hd_resolve::anchor::Anchor, String)>;

/// A module's TIR and body diagnostics after `Body(m)`, the hidden
/// methods its derivations add (checking-and-tir.md §4.13.9), and the
/// results and rows inferred for its private callables (§4.13.1).
type BodyOut = (
    Vec<Body>,
    DiagBuf,
    Vec<Item>,
    Vec<hd_check::results::Inferred>,
    hd_check::init::InitSummary,
    Vec<hd_check::DbgRef>,
    Vec<InputBinding>,
);

/// What a module's derivations add to its `check` entry: the derived
/// implementations' methods and their bodies (codegen.md §12.3).
#[derive(Default)]
struct Derived {
    items: Vec<Item>,
    bodies: Vec<Body>,
    /// The owners whose `Structure` bodies are generated, each once.
    structures: Vec<DefId>,
}

/// One derivation opt-in of a module: its item's index, the opt-in, and
/// the facts bodies of the `Structure` it sees, or `None` when they are
/// not generated (its `Structure` then gets no bodies).
type OptInEntry = (usize, hd_check::derive::OptIn, Option<FactBodies>);

/// A module's `check` entry, as far as `prog_key` needs it.
struct CheckOut {
    entry: Arc<[u8]>,
    content: Hash128,
    has_errors: bool,
}

/// The program as `Collect` decoded it.
struct ProgramTables {
    items: HashMap<DefId, Item>,
    bodies: HashMap<DefId, (Body, Hash128)>,
    /// Where each impl's row lives: its folder's table.
    impl_rows: HashMap<DefId, ImplRef>,
    /// The program's impl universe: every folder of the program. A build
    /// sees the whole program, so the universe is not narrowed to a
    /// closure (trait-solver.md §3.2).
    universe: ImplUniverseId,
    extra: Arc<UniverseImpls>,
    /// The program's self-recursive enums (`hd_mono::layout::recursive_enums`),
    /// found once on the first layout that asks.
    recursive_enums: OnceLock<std::collections::HashSet<DefId>>,
}

/// One run of a module cycle's initialization (`InitOrder`, spec 10
/// `module.init.cycle.step`): statements `start..end` of `module`, which
/// run one after another, and their instructions in its `Init` body.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InitRun {
    module: String,
    start: u32,
    end: u32,
    insts: std::ops::Range<u32>,
}

struct CollectOut {
    collected: Collected,
    order: Vec<hd_base::InstId>,
    codes: Vec<OnceLock<WasmCode>>,
    code_keys: Vec<Hash128>,
    prog_key: Hash128,
    roots: Roots,
    /// Nominal types' layouts, shared by the program's emissions so each
    /// recursion group is laid out once.
    layouts: hd_wasm::layout::Layouts,
}

/// The roots a program's exports poll.
enum Roots {
    /// `main`, its report and the inits it reaches.
    Main {
        def: DefId,
        key: Hash128,
        /// The entry module's own init, the one init that may `block_on`.
        entry_init: Option<Hash128>,
        report: Option<Hash128>,
    },
    /// A script: its init, which runs its top-level statements after the
    /// other inits it reaches.
    Script { def: DefId, key: Hash128 },
    /// Each running case (item, instance, report), and per test module
    /// the inits it reaches.
    Tests {
        cases: Vec<(DefId, Hash128, Option<Hash128>)>,
        inits: Vec<Vec<Hash128>>,
    },
    /// A scenario's `main`, its report and the inits it reaches: like
    /// `Main`, but linked with the scenario exports.
    Scenario {
        def: DefId,
        key: Hash128,
        /// The entry module's own init, the one init that may `block_on`.
        entry_init: Option<Hash128>,
        report: Option<Hash128>,
        /// The entry module's top-level cleanup function, called by
        /// `hd.scenario.cleanup`; `None` when it declares none.
        cleanup: Option<Hash128>,
    },
}

/// A module's `tests:` blocks after checking: the case items and the
/// registrations.
type TestsOut = (Vec<Item>, Vec<hd_check::tests::TestReg>);

/// One tuple template's member steps, computed once (trait-solver.md
/// §3.9); `None` for a template whose body does not check.
type PlanCell = Arc<OnceLock<Option<Arc<[MemberStep]>>>>;

/// `src/lib.hd` lowered for a REPL session: its scope, its kinds, and its
/// private items, which no interface carries.
struct LibScope {
    scope: hd_resolve::ModuleScope,
    kinds: hd_resolve::Kinds,
    private: Vec<Item>,
}

/// The REPL input modules of a run ([`Goal::ReplInput`]).
struct ReplInputs {
    /// (number, module), by number.
    sorted: Vec<(u32, usize)>,
    /// Each module's number, if it is an input.
    numbers: Vec<Option<u32>>,
}

struct Run<'a> {
    host: &'a Host<'a>,
    observer: Option<&'a dyn StageObserver>,
    package: String,
    /// Whether the root package is a single-file program
    /// ([`Packages::single_file`]).
    single_file: bool,
    goal: Goal,
    /// The host capability traits of the run's runtime profile, by item
    /// path ([`Packages::profile`]).
    profile: Vec<String>,
    table: ModuleTable,
    texts: Vec<Arc<str>>,
    pool: InternPool,
    paths: PathTable,
    /// The compiler-known std items, resolved once for the run.
    known: hd_resolve::KnownItems,
    syms: ShardedInterner,
    universes: ImplUniverses,
    memo: GlobalMemo,
    /// Which folder declares what (trait-solver.md §3.2), built on first use.
    owners: OnceLock<OwnerMap>,
    /// Per folder: its impl table and `arg_impls` section, built once per
    /// run from its interface on first use.
    folder_impls: Vec<OnceLock<Option<Arc<FolderImpls>>>>,
    toolchain: Hash128,
    package_key: Hash128,
    pipeline: Hash128,
    /// The build profile emitted code follows ([`Packages::build_profile`]).
    build_profile: BuildProfile,
    /// Per package, in the table's order: whether it is fetched, so not
    /// the user's own code (`module.dbg.own-code`).
    fetched: Vec<bool>,
    skim: Vec<OnceLock<SkimOut>>,
    parse: Vec<OnceLock<Parse>>,
    graph: OnceLock<GraphOut>,
    iface: Vec<OnceLock<Option<Arc<FolderIface>>>>,
    prep: Vec<OnceLock<Option<PrepOut>>>,
    /// Per module that declares a derivation template: its items and scope,
    /// lowered once per run for the opt-ins that instantiate it.
    template_mods: Vec<OnceLock<Option<ModOut>>>,
    /// Per folder: the modules outside std that declare a derivation
    /// template, from its interface, found on first use; `true` for a
    /// module that declares a tuple template.
    folder_templates: Vec<OnceLock<Vec<(String, bool)>>>,
    /// Per tuple template: its member steps (trait-solver.md §3.9),
    /// computed once per run from its body on first use.
    tuple_plans: Mutex<HashMap<DefId, PlanCell>>,
    body: Vec<OnceLock<Option<BodyOut>>>,
    check: Vec<OnceLock<Option<CheckOut>>>,
    /// Per module, in a test run: the checked `tests:` blocks (a miss).
    tests: Vec<OnceLock<TestsOut>>,
    /// Per module, in a test run: the registrations its check entry holds.
    regs: Vec<OnceLock<Vec<hd_check::tests::TestReg>>>,
    /// Each module's references to `dbg` (`module.dbg.release`).
    dbg_refs: Vec<OnceLock<Vec<hd_check::DbgRef>>>,
    /// Per folder: `InitOrder`'s runs, one list per module cycle of the
    /// folder whose statements sit in several modules.
    init_runs: Vec<OnceLock<Vec<Vec<InitRun>>>>,
    program: OnceLock<ProgramTables>,
    collect: OnceLock<CollectOut>,
    wasm: OnceLock<Vec<u8>>,
    /// The session this run links one more module of ([`Packages::session`]).
    session: Option<&'a Session>,
    /// What this run's module adds to that session ([`Output::session_added`]).
    session_added: OnceLock<Session>,
    /// `src/lib.hd`'s scope, kinds and items, lowered once for the REPL
    /// inputs' session scopes (`cli.repl.package.lib`).
    lib_scope: OnceLock<Option<LibScope>>,
    /// Per REPL input: the session scope its own scope is nested in.
    session_scopes: Vec<OnceLock<Arc<hd_resolve::SessionScope>>>,
    /// The REPL input modules of the root package.
    repl_inputs: OnceLock<ReplInputs>,
    /// Per REPL input: its input interface's top-level bindings, read from
    /// its `check` entry (`cli.repl.declarations.bindings`).
    input_bindings: Vec<OnceLock<Vec<InputBinding>>>,
    /// Per module: the task after which its `check` entry is read, its
    /// `ModulePrep` on a hit and its `ModuleFinish` on a miss. A later REPL
    /// input's bodies wait for it, since they read its bindings.
    check_task: Vec<OnceLock<TaskId>>,
    diags: Mutex<DiagBuf>,
    report: Mutex<PipelineReport>,
    counters: Mutex<Counters>,
    tir_text: Mutex<std::collections::BTreeMap<String, String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A package the root package reaches through its requirements (§4.7).
pub struct Dependency<'a> {
    /// Its manifest name; its module paths start with its identifier form,
    /// unless an earlier package holds it (`ModuleTable::roots`).
    pub name: String,
    /// What tells it from another package of the same name
    /// (`module.dep.identity`): a fetched version's host path and
    /// compatibility line, or a local package's canonical directory. It
    /// keys the package's cached outputs.
    pub identity: String,
    /// Its files, package-relative; only its library is built.
    pub sources: &'a dyn SourceSet,
    /// Its own requirements, as in [`Packages::requires`].
    pub requires: Vec<(String, u16)>,
    /// Whether it is a package fetched for a version requirement. Every
    /// other dependency is the user's own code, which a release build
    /// holds to `module.dbg.release` (`module.dbg.own-code`).
    pub fetched: bool,
}

/// The packages around a build's root package.
#[derive(Default)]
pub struct Packages<'a> {
    /// The root's requirements: each dependency name, as source writes it
    /// after `dep.`, and its package index: 0 is the root, `i + 1` is
    /// `deps[i]`.
    pub requires: Vec<(String, u16)>,
    pub deps: Vec<Dependency<'a>>,
    /// The root's executable entry modules, by their path below its source
    /// root (`cli.exe.entry-program`).
    pub entries: Vec<String>,
    /// The root's dev dependencies, as `requires` (`module.test.dev-dependency`).
    pub dev_requires: Vec<(String, u16)>,
    /// The host capability traits of the selected runtime profile, by item
    /// path (`app.Vault`), whose signatures are boundaries
    /// (`module.profile.definition`, `module.boundary.direction`); `None`
    /// selects the default profile, whose traits the host ABI table lists
    /// (`module.profile.default`).
    pub profile: Option<Vec<String>>,
    /// Whether the root package is a single-file program, whose
    /// declarations print under the file's stem instead of the package
    /// name (`trait.typeid.name.single-file`).
    pub single_file: bool,
    /// The build profile of a `Goal::Program` (`cli.profile.default`,
    /// `cli.profile.release`). Every other goal is checked: a test run
    /// keeps the test profile (`cli.profile.test`, `cli.profile.test.release`).
    pub build_profile: BuildProfile,
    /// Whether the output holds each root-package module's scope
    /// ([`Output::scopes`]), which `hd doc` resolves its links in.
    pub doc_scopes: bool,
    /// For a `Goal::Program`: build one more module of this session
    /// instead of a closed program (repl-plan.md §2.2). Collection stops at
    /// the instances earlier modules export and leaves out the inits they
    /// ran; the module links in `hd_wasm::LinkMode::Session`. Its link is
    /// not cached, since it depends on the session, except the prelude's
    /// ([`build_repl_prelude`]), which every session starts with.
    pub session: Option<&'a Session>,
}

/// What the modules a session built so far hold, for the build of its next
/// module ([`Packages::session`]), such as the inputs before a REPL input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// What those modules export: their instances, parts and globals, and
    /// whether the next module is the session's first.
    pub externs: hd_wasm::Externs,
    /// The keys of the init bodies' instances those modules ran.
    pub inits_run: BTreeSet<Hash128>,
}

impl Session {
    /// A session with no module yet: the next module is its first.
    #[must_use]
    pub fn start() -> Session {
        Session {
            externs: hd_wasm::Externs {
                first: true,
                ..hd_wasm::Externs::default()
            },
            inits_run: BTreeSet::new(),
        }
    }

    /// Adds what a module that ran adds ([`Output::session_added`]); the
    /// next module is not the first.
    pub fn add(&mut self, added: Session) {
        self.externs.first = false;
        self.externs.funcs.extend(added.externs.funcs);
        self.externs.globals.extend(added.externs.globals);
        self.inits_run.extend(added.inits_run);
    }

    /// A cache section: the externs' bytes, then the inits run.
    fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.blob(&self.externs.encode());
        w.u32(u32_of(self.inits_run.len()));
        for k in &self.inits_run {
            w.hash(*k);
        }
        w.bytes
    }

    fn decode(bytes: &[u8]) -> Option<Session> {
        let mut r = Reader::new(bytes);
        let externs = hd_wasm::Externs::decode(r.blob())?;
        let inits_run = (0..r.count()).map(|_| r.hash()).collect();
        (r.ok() && r.at_end()).then_some(Session { externs, inits_run })
    }
}

/// The REPL session's prelude (repl-plan.md §2.3, job C8): input 0, the
/// session's first module. It owns the memory and the runtime globals and
/// holds what a value-showing input needs (`cli.repl.value`), so a later
/// input imports those instances instead of linking its own copies: the
/// value line, and `dbg`'s layout and printer, a function reference
/// included, for each primitive type and the lists in `PRELUDE_LISTS`.
/// Its one statement does nothing when it runs.
const REPL_PRELUDE: &str = "\
# The REPL session's prelude (hd_driver::REPL_PRELUDE).
pass
";

/// The package the prelude is the one file of: it reads only std, so it
/// is the same for every session of one toolchain and build profile.
const PRELUDE_PACKAGE: &str = "repl";

/// The element types of the lists the prelude shows: those of number
/// literals written bare or with `+`, and `string`. Each list costs the
/// prelude about 1.5 KB; a list of another type is shown by the input's
/// own copy of its instances.
const PRELUDE_LISTS: [hd_types::Prim; 4] = [
    hd_types::Prim::Usize,
    hd_types::Prim::I32,
    hd_types::Prim::F64,
    hd_types::Prim::String,
];

/// The instances the prelude holds besides its init's
/// ([`build_repl_prelude`]), by item and type arguments.
#[derive(Default)]
struct PreludeRoots {
    /// The calls: `repl_value`, and `dbg_layout` at each type.
    calls: Vec<(DefId, TyList)>,
    /// The function references, as adapters: `dbg_show` at each type.
    refs: Vec<(DefId, TyList)>,
}

/// The prelude's cache key: per toolchain and build profile, since its
/// source is the compiler's.
fn prelude_key(toolchain: Hash128, pipeline: Hash128, profile: BuildProfile) -> Hash128 {
    prog_key(toolchain, pipeline, profile, "repl:prelude", &[])
}

/// Builds the REPL session's prelude, input 0, for a session in
/// `profile`, or reads it from `host.store`, where it is cached per
/// toolchain. It owns the session's memory and runtime globals and holds
/// the instances that showing a value needs (`cli.repl.value`), so small
/// inputs stay small. A session that starts runs it first, then adds its
/// [`Output::session_added`] to [`Session::start`], and builds its inputs,
/// numbered from 1, over that session. `host.sources` is not read.
#[must_use]
pub fn build_repl_prelude(host: &Host<'_>, profile: BuildProfile) -> Output {
    let mut sources = MemorySources::default();
    sources.insert(&hd_project::repl_file(0), REPL_PRELUDE);
    let host = Host {
        render_tir: host.render_tir,
        sources: &sources,
        store: host.store,
        clock: host.clock,
        executor: host.executor,
    };
    let start = Session::start();
    let packages = Packages {
        build_profile: profile,
        session: Some(&start),
        ..Packages::default()
    };
    build_packages(
        &host,
        PRELUDE_PACKAGE,
        &packages,
        &Goal::ReplInput { input: 0 },
    )
}

/// The root package's key: its name, and the traits of a runtime profile
/// other than the default one, whose boundary checks its entries hold.
fn root_package_key(package: &str, profile: Option<&[String]>) -> Hash128 {
    let base = hd_cache::package_key(package);
    let Some(traits) = profile else {
        return base;
    };
    let mut k = hd_base::StableHasher::new("pkg-profile");
    k.hash(base);
    for t in traits {
        k.str(t);
    }
    k.finish()
}

/// One run with the caller's sources, store, executor and clock.
#[must_use]
pub fn build(host: &Host<'_>, package: &str, goal: &Goal) -> Output {
    build_packages(host, package, &Packages::default(), goal)
}

/// `build`, for a root package with dependencies.
#[must_use]
pub fn build_packages(
    host: &Host<'_>,
    package: &str,
    packages: &Packages<'_>,
    goal: &Goal,
) -> Output {
    build_packages_inner(host, package, packages, goal, None)
}

/// `build_packages`, with thread-local stage observation for host accounting.
#[must_use]
pub fn build_packages_observed(
    host: &Host<'_>,
    package: &str,
    packages: &Packages<'_>,
    goal: &Goal,
    observer: &dyn StageObserver,
) -> Output {
    build_packages_inner(host, package, packages, goal, Some(observer))
}

fn build_packages_inner(
    host: &Host<'_>,
    package: &str,
    packages: &Packages<'_>,
    goal: &Goal,
    observer: Option<&dyn StageObserver>,
) -> Output {
    let _discover = ObservedStage::new(observer, Stage::Discover);
    let std = if package == "std" {
        with_core(host.sources)
    } else {
        std_sources()
    };
    let std_in = PackageIn {
        name: "std",
        identity: "std",
        sources: &std,
        scope: Scope::All,
        requires: Vec::new(),
        entries: Vec::new(),
        dev_requires: Vec::new(),
    };
    let ins: Vec<PackageIn<'_>> = if package == "std" {
        vec![std_in]
    } else {
        std::iter::once(PackageIn {
            name: package,
            identity: package,
            sources: host.sources,
            scope: Scope::All,
            requires: packages.requires.clone(),
            entries: packages.entries.clone(),
            dev_requires: packages.dev_requires.clone(),
        })
        .chain(packages.deps.iter().map(|d| PackageIn {
            name: &d.name,
            identity: &d.identity,
            sources: d.sources,
            scope: Scope::Library,
            requires: d.requires.clone(),
            entries: Vec::new(),
            dev_requires: Vec::new(),
        }))
        .chain(std::iter::once(std_in))
        .collect()
    };
    let mut table = ModuleTable::discover_all(&ins);
    let texts: Vec<Arc<str>> = table
        .sources
        .iter()
        .map(|(pi, p)| {
            let set = ins[usize::from(*pi)].sources;
            Arc::from(
                set.read(p)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let folder_cycles = recover_folder_cycles(&mut table, &texts);
    let std_hash = sources_hash(&std);
    let n = table.modules.len();
    let nf = table.folders.len();
    let pipeline = hd_mono::passes::validate(&hd_mono::passes::DEV).unwrap_or_default();
    let paths = PathTable::new();
    let known = hd_resolve::KnownItems::new(&paths);
    let profile = packages.profile.clone().unwrap_or_else(|| {
        hd_host_abi::TABLE
            .iter()
            .map(|t| t.std_path.to_owned())
            .collect()
    });
    let run = Run {
        host,
        observer,
        package: package.to_owned(),
        single_file: packages.single_file,
        goal: goal.clone(),
        texts,
        pool: InternPool::new(),
        paths,
        known,
        syms: ShardedInterner::default(),
        universes: ImplUniverses::default(),
        memo: GlobalMemo::default(),
        owners: OnceLock::new(),
        folder_impls: (0..nf).map(|_| OnceLock::new()).collect(),
        toolchain: toolchain_key(
            COMPILER,
            TARGET,
            &format!("a1=bounded;std={:032x}", std_hash.0),
        ),
        package_key: root_package_key(package, packages.profile.as_deref()),
        profile,
        pipeline,
        build_profile: match goal {
            Goal::Program { .. } | Goal::ReplInput { .. } => packages.build_profile,
            Goal::Tests { .. } | Goal::Scenario { .. } | Goal::Analyze | Goal::CheckTests => {
                BuildProfile::Checked
            }
        },
        fetched: std::iter::once(false)
            .chain(packages.deps.iter().map(|d| d.fetched))
            .chain(std::iter::once(false))
            .collect(),
        skim: (0..n).map(|_| OnceLock::new()).collect(),
        parse: (0..n).map(|_| OnceLock::new()).collect(),
        graph: OnceLock::new(),
        iface: (0..nf).map(|_| OnceLock::new()).collect(),
        prep: (0..n).map(|_| OnceLock::new()).collect(),
        template_mods: (0..n).map(|_| OnceLock::new()).collect(),
        folder_templates: (0..nf).map(|_| OnceLock::new()).collect(),
        tuple_plans: Mutex::default(),
        body: (0..n).map(|_| OnceLock::new()).collect(),
        check: (0..n).map(|_| OnceLock::new()).collect(),
        tests: (0..n).map(|_| OnceLock::new()).collect(),
        regs: (0..n).map(|_| OnceLock::new()).collect(),
        dbg_refs: (0..n).map(|_| OnceLock::new()).collect(),
        init_runs: (0..nf).map(|_| OnceLock::new()).collect(),
        program: OnceLock::new(),
        collect: OnceLock::new(),
        wasm: OnceLock::new(),
        session: packages.session,
        session_added: OnceLock::new(),
        lib_scope: OnceLock::new(),
        session_scopes: (0..n).map(|_| OnceLock::new()).collect(),
        repl_inputs: OnceLock::new(),
        input_bindings: (0..n).map(|_| OnceLock::new()).collect(),
        check_task: (0..n).map(|_| OnceLock::new()).collect(),
        diags: Mutex::new(DiagBuf::default()),
        report: Mutex::new(PipelineReport {
            package: package.to_owned(),
            ..PipelineReport::default()
        }),
        counters: Mutex::new(Counters::default()),
        tir_text: Mutex::new(std::collections::BTreeMap::new()),
        table,
    };
    lock(&run.report).ok(Stage::Discover);
    for names in &folder_cycles {
        let span = Span {
            file: FileId::from_raw(u32::MAX),
            lo: 0,
            hi: 0,
        };
        lock(&run.diags).error(
            Code::FolderCycle,
            span,
            &format!("folders form a cycle: {}", names.join(" -> ")),
        );
    }
    for cycle in run.table.package_cycles() {
        let names: Vec<&str> = cycle
            .iter()
            .chain(cycle.first())
            .map(|p| run.table.packages[usize::from(*p)].as_str())
            .collect();
        let span = Span {
            file: FileId::from_raw(u32::MAX),
            lo: 0,
            hi: 0,
        };
        lock(&run.diags).error(
            Code::PackageCycle,
            span,
            &format!("packages depend on each other: {}", names.join(" -> ")),
        );
    }
    for (file, code, message) in &run.table.problems {
        let span = Span {
            file: *file,
            lo: 0,
            hi: 0,
        };
        lock(&run.diags).error(*code, span, message);
    }
    // A cached prelude needs none of the stages.
    if !run.cached_prelude() {
        let mut g = TaskGraph::default();
        let mut skims = Vec::new();
        for i in 0..n {
            skims.push(g.add(TaskKind::Skim(u32::try_from(i).expect("files")), &[]));
            if matches!(goal, Goal::Analyze | Goal::CheckTests) {
                g.add(TaskKind::Parse(u32::try_from(i).expect("files")), &[]);
            }
        }
        g.add(TaskKind::FolderGraph, &skims);
        let exec = |id: TaskId, kind: TaskKind, sp: &dyn Spawn| run.exec(id, kind, sp);
        let left = match host.executor {
            Executor::Serial(order) => SerialScheduler { order }.run(&mut g, &exec),
            #[cfg(feature = "threads")]
            Executor::Pool(threads) => hd_sched::pool::run_pool(g, threads, &exec).1,
        };
        if left > 0 {
            run.stop(
                Stage::PackageResult,
                &format!("{left} tasks never became ready"),
                None,
            );
        }
    }
    let _finish = ObservedStage::new(observer, Stage::PackageResult);
    let mut counters = lock(&run.counters).clone();
    counters.sort();
    let report = lock(&run.report).clone();
    let diags = std::mem::take(&mut *lock(&run.diags));
    let wasm = if diags.has_errors() {
        None
    } else {
        run.wasm.get().cloned()
    };
    let session_added = wasm.as_ref().and_then(|_| run.session_added.get().cloned());
    let ifaces = run
        .table
        .folders
        .iter()
        .filter_map(|f| Some((f.path.clone(), run.iface_of(f.id)?.blob.clone())))
        .collect();
    let scopes = if packages.doc_scopes {
        run.doc_scopes()
    } else {
        Vec::new()
    };
    let cases = run.test_cases();
    let uses = run
        .skim
        .iter()
        .map(|s| {
            s.get().map_or_else(Vec::new, |s| {
                s.uses
                    .iter()
                    .filter_map(|u| run.table.module_of_use(u))
                    .map(|m| run.table.modules[m.idx()].file.idx())
                    .collect()
            })
        })
        .collect();
    Output {
        tests: cases,
        wasm,
        diags,
        files: run.table.files.clone(),
        texts: run.texts.clone(),
        uses,
        modules: run.table.modules.iter().map(|m| m.path.clone()).collect(),
        checks: run
            .check
            .iter()
            .map(|c| {
                c.get()
                    .and_then(Option::as_ref)
                    .map(|c| Arc::clone(&c.entry))
            })
            .collect(),
        printed: run
            .table
            .modules
            .iter()
            .map(|m| run.frame_module(&m.path))
            .collect(),
        test_bodies: run.test_bodies(),
        renamed: run.table.renamed_roots(),
        counters,
        report,
        ifaces,
        scopes,
        tir_text: std::mem::take(&mut *lock(&run.tir_text)),
        pool_items: run.pool.len(),
        memo: run.memo.stats(),
        session_added,
    }
}

/// Folder-cycle recovery (resolution-and-interfaces.md §4.8 rule 5): the
/// folders of a cycle in the root package are merged into one folder, so
/// they resolve together and their uses do not cascade; merging repeats
/// until the folder graph is acyclic. Returns the cycles found first, as
/// folder paths, for one `folder-cycle` error each.
fn recover_folder_cycles(table: &mut ModuleTable, texts: &[Arc<str>]) -> Vec<Vec<String>> {
    let uses: Vec<Vec<String>> = (0..table.modules.len())
        .map(|m| {
            if table.modules[m].package != 0 {
                return Vec::new();
            }
            let roots = table.use_roots(ModuleId::from_raw(u32_of(m)));
            written_uses(&texts[m], &skim(texts[m].as_bytes()).uses, &roots)
        })
        .collect();
    let mut found: Vec<Vec<String>> = Vec::new();
    loop {
        let graph = FolderGraph::build(table, &uses);
        let cycles: Vec<Vec<FolderId>> = graph
            .cycles
            .into_iter()
            .filter(|c| c.iter().all(|f| table.folders[f.idx()].package == 0))
            .collect();
        if cycles.is_empty() {
            return found;
        }
        if found.is_empty() {
            found = cycles
                .iter()
                .map(|c| {
                    c.iter()
                        .map(|f| table.folders[f.idx()].path.clone())
                        .collect()
                })
                .collect();
        }
        table.merge_folders(&cycles);
    }
}

/// The module paths a file's `use` lines name, their roots replaced by
/// the module paths they name. A path whose root names nothing makes no
/// edge; resolution reports it.
fn written_uses(text: &str, lines: &[(u32, u32)], roots: &hd_project::UseRoots) -> Vec<String> {
    let mut uses = Vec::new();
    for &(lo, hi) in lines {
        let line = text.get(lo as usize..hi as usize).unwrap_or("").trim();
        let rest = line.strip_prefix("pub ").unwrap_or(line);
        if let Some(rest) = rest.strip_prefix("use ") {
            let path = rest.split(".{").next().unwrap_or(rest);
            let path = path.split(" as ").next().unwrap_or(path).trim();
            let segs: Vec<&str> = path.split('.').collect();
            if let Ok(abs) = roots.absolute(&segs) {
                uses.push(abs.join("."));
            }
        }
    }
    uses
}

/// Whether a file holds a `tests:` block, from its skim alone.
#[must_use]
pub fn has_tests_block(text: &str) -> bool {
    skim(text.as_bytes())
        .bodies
        .iter()
        .any(|b| b.kind == HeaderKind::Tests)
}

/// The files of a package that the module of `file` reaches through its
/// `use` lines, deeply, `file` first: the part of the package that
/// `hd test FILE` and `hd check FILE` link with FILE's module
/// (`cli.package.file`). Only discovery and skims run, no check.
#[must_use]
pub fn use_closure(package: &str, sources: &dyn SourceSet, file: &str) -> Vec<String> {
    let table = ModuleTable::discover_all(&[PackageIn {
        name: package,
        identity: package,
        sources,
        scope: Scope::All,
        requires: Vec::new(),
        entries: Vec::new(),
        dev_requires: Vec::new(),
    }]);
    let Some(start) = table.files.iter().position(|f| f == file) else {
        return vec![file.to_owned()];
    };
    let mut seen = vec![false; table.modules.len()];
    let mut order = Vec::new();
    let mut stack = vec![start];
    while let Some(m) = stack.pop() {
        if std::mem::replace(&mut seen[m], true) {
            continue;
        }
        order.push(table.files[m].clone());
        let text = sources
            .read(&table.sources[m].1)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let roots = table.use_roots(ModuleId::from_raw(u32_of(m)));
        for u in written_uses(&text, &skim(text.as_bytes()).uses, &roots) {
            if let Some(id) = table.module_of_use(&u) {
                stack.push(id.idx());
            }
        }
    }
    order
}

/// Runs every stage of the design over one package and counts how far
/// each gets (the same driver as `build`, in analysis mode).
pub fn analyze_package(package: &str, sources: &dyn SourceSet) -> PipelineReport {
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, package, &Goal::Analyze).report
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("run index")
}

/// The interface's span table from decoded anchor rows (item index, module
/// index, slot, anchor).
fn spans_of(
    items: &[Item],
    rows: &[(u32, u32, u32, hd_resolve::anchor::Anchor)],
) -> HashMap<(DefId, u32), (u32, hd_resolve::anchor::Anchor)> {
    rows.iter()
        .filter_map(|(i, m, slot, a)| Some(((items.get(*i as usize)?.def, *slot), (*m, *a))))
        .collect()
}

/// A failing member of a derivation opt-in.
struct OptInFinding {
    /// The opt-in's index among its module's own items.
    item: usize,
    /// The data type or enum the member belongs to, and the member's
    /// written type position in it.
    data: DefId,
    slot: u32,
    /// Whether the opt-in derives `Eq`, `PartialOrd`, `Ord` or `Hash`, whose
    /// failing member is `derive-field-missing-trait`.
    compare: bool,
    message: String,
}

impl Run<'_> {
    fn names(&self) -> Names<'_> {
        Names {
            pool: &self.pool,
            paths: &self.paths,
            syms: &self.syms,
            known: &self.known,
        }
    }

    /// A module's path as its declarations print (trait.typeid.name.*): a
    /// single-file program's module by the file's stem
    /// (trait.typeid.name.single-file), any other package's by its
    /// manifest name with `-` written `_` (trait.typeid.name.package),
    /// though its module root may be disambiguated
    /// (`module.package.identity-part`).
    fn printed_module(&self, module: &str) -> String {
        let table = &self.table;
        let (root, below) = module.split_once('.').unwrap_or((module, ""));
        let Some(package) = table.roots.iter().position(|r| r == root) else {
            return module.to_owned();
        };
        if package == 0 && self.single_file {
            return below.to_owned();
        }
        let ident = hd_project::package_ident(&table.packages[package]);
        if below.is_empty() {
            ident
        } else {
            format!("{ident}.{below}")
        }
    }

    /// A module's path as a panic frame names its functions
    /// (`cli.test.report.frame`): as its declarations print, without the
    /// `$tests` or `$tasks` segment of a test or task module, which no
    /// source spells, so `tests/cart.hd` is `shop.cart`.
    fn frame_module(&self, module: &str) -> String {
        self.printed_module(module)
            .split('.')
            .filter(|s| !matches!(*s, "$tests" | "$tasks"))
            .collect::<Vec<_>>()
            .join(".")
    }

    fn analyze(&self) -> bool {
        matches!(self.goal, Goal::Analyze | Goal::CheckTests)
    }

    fn exec(&self, id: TaskId, kind: TaskKind, sp: &dyn Spawn) {
        let stage = match kind {
            TaskKind::Skim(_) => Stage::Skim,
            TaskKind::Parse(_) => Stage::Parse,
            TaskKind::FolderGraph => Stage::FolderGraph,
            TaskKind::FolderIface(_) => Stage::FolderIface,
            TaskKind::HeaderCheck(_) => Stage::HeaderCheck,
            TaskKind::ModulePrep(_) => Stage::ModulePrep,
            TaskKind::Body(_) => Stage::Body,
            TaskKind::ModuleFinish(_) => Stage::ModuleFinish,
            TaskKind::TestOverlay(_) => Stage::TestOverlay,
            TaskKind::Coherence => Stage::Coherence,
            TaskKind::InitOrder(_) => Stage::InitOrder,
            TaskKind::PackageResult => Stage::PackageResult,
            TaskKind::Ext(ExtTask::Collect) => Stage::Collect,
            TaskKind::Ext(ExtTask::Emit(_)) => Stage::Emit,
            TaskKind::Ext(ExtTask::Link) => Stage::Link,
            TaskKind::Ext(ExtTask::Precompile) => Stage::Precompile,
            TaskKind::Ext(ExtTask::RunCase(_)) => Stage::Run,
        };
        let _stage = ObservedStage::new(self.observer, stage);
        let t0 = self.host.clock.now_ns();
        match kind {
            TaskKind::Skim(f) => self.skim(f as usize),
            TaskKind::FolderGraph => self.folder_graph(sp),
            TaskKind::FolderIface(f) => self.folder_iface(f as usize),
            TaskKind::HeaderCheck(f) => self.header_check(f as usize),
            TaskKind::InitOrder(f) => self.init_order(f as usize),
            TaskKind::Coherence => self.coherence(),
            TaskKind::ModulePrep(m) => self.module_prep(id, m, sp),
            TaskKind::Body(m) => self.body(m as usize),
            TaskKind::ModuleFinish(m) => self.module_finish(m as usize),
            TaskKind::TestOverlay(m) => {
                let r = test_overlay(&self.skim_of(m as usize).facts);
                self.stage(Stage::TestOverlay, r);
            }
            TaskKind::PackageResult => self.package_result(sp),
            TaskKind::Ext(ExtTask::Collect) => self.collect(sp),
            TaskKind::Ext(ExtTask::Emit(i)) => self.emit(i as usize),
            TaskKind::Ext(ExtTask::Link) => self.link(),
            TaskKind::Parse(f) => {
                self.parse_of(f as usize);
            }
            TaskKind::Ext(ExtTask::Precompile | ExtTask::RunCase(_)) => {}
        }
        let mut c = lock(&self.counters);
        *c.tasks.entry(kind.name()).or_default() += 1;
        *c.stage_ns.entry(kind.name()).or_default() += self.host.clock.now_ns().saturating_sub(t0);
    }

    /// Records a stage's answer. A "not implemented" answer is counted and
    /// reported as an internal `unsupported` diagnostic, in analysis too: a
    /// package whose checking stops at a construct is not a checked package.
    fn stage<T>(&self, s: Stage, r: StageResult<T>) -> Option<T> {
        match r {
            Ok(v) => {
                lock(&self.report).ok(s);
                Some(v)
            }
            Err(e) => {
                lock(&self.report).not_implemented(s, &e.what);
                self.stop(s, &e.what, e.span);
                None
            }
        }
    }

    /// Reports a stop; `at` is where the stage found the construct, or the
    /// sentinel span when the stage has no position.
    fn stop(&self, s: Stage, what: &str, at: Option<Span>) {
        let span = at.unwrap_or(Span {
            file: FileId::from_raw(u32::MAX),
            lo: 0,
            hi: 0,
        });
        lock(&self.diags).error(
            Code::Unsupported,
            span,
            &format!("stage {}: {what} is not supported", s.name()),
        );
    }

    /// Reports collection's stop at an instantiation limit
    /// (`types.generic.instantiation-depth.error`) on the call that would
    /// exceed it, in the module that declares the calling item.
    fn too_deep(&self, deep: &hd_mono::TooDeep) {
        let span = deep
            .at
            .and_then(|(item, node)| {
                let module = self.names().module_of(item);
                let m = self.table.modules.iter().position(|x| x.path == module)?;
                let src = self.src(m);
                Some(src.span(src.parse.tree.node(node)))
            })
            .unwrap_or(Span {
                file: FileId::from_raw(u32::MAX),
                lo: 0,
                hi: 0,
            });
        lock(&self.diags).error(Code::InstantiationTooDeep, span, &deep.message);
    }

    fn blocked(&self, s: Stage) {
        lock(&self.report).blocked(s);
    }

    /// Whether this run builds a REPL session's prelude
    /// ([`build_repl_prelude`]): input 0, the first module of a session.
    fn prelude(&self) -> bool {
        matches!(self.goal, Goal::ReplInput { input: 0 })
            && self.session.is_some_and(|s| *s == Session::start())
    }

    /// For the prelude: reads it from the store, its module and what it
    /// adds to the session; whether it was there.
    fn cached_prelude(&self) -> bool {
        if !self.prelude() {
            return false;
        }
        let key = prelude_key(self.toolchain, self.pipeline, self.build_profile);
        let Some(sections) = self.lookup(EntryKind::Link, key) else {
            return false;
        };
        let (Some(wasm), Some(added)) = (
            sections.first(),
            sections.get(1).and_then(|b| Session::decode(b)),
        ) else {
            return false;
        };
        let _ = self.wasm.set(wasm.clone());
        let _ = self.session_added.set(added);
        lock(&self.report).ok(Stage::Collect);
        true
    }

    /// The prelude's roots besides its init: what an input that is one
    /// expression reaches (`hd_check`'s `repl_value` and `dbg_text`).
    fn prelude_roots(&self) -> PreludeRoots {
        let names = self.names();
        let format = |n: &str| names.item("std.format", n);
        let pool = &self.pool;
        let lists = PRELUDE_LISTS.iter().map(|p| {
            pool.intern_ty(&TyData::Adt {
                def: self.known.list,
                args: pool.list(&[Ty::prim(*p)]),
            })
        });
        let types: Vec<Ty> = hd_types::Prim::ALL
            .iter()
            .map(|p| Ty::prim(*p))
            .chain(lists)
            .collect();
        let mut calls = vec![(format("repl_value"), TyList::EMPTY)];
        let layout = format("dbg_layout");
        calls.extend(types.iter().map(|t| (layout, pool.list(&[*t]))));
        let show = format("dbg_show");
        let refs = types.iter().map(|t| (show, pool.list(&[*t]))).collect();
        PreludeRoots { calls, refs }
    }

    fn lookup(&self, kind: EntryKind, key: Hash128) -> Option<Vec<Vec<u8>>> {
        let bytes = self.host.store.get(kind, &key);
        let sections = bytes.as_deref().and_then(|b| {
            let (h, s) = hd_cache::decode_entry(b).ok()?;
            (h.key == key && h.toolchain == self.toolchain && h.layout == LAYOUT)
                .then(|| s.into_iter().map(|(_, p)| p.to_vec()).collect::<Vec<_>>())
        });
        let mut c = lock(&self.counters);
        *if sections.is_some() {
            c.hits.entry(kind.as_str())
        } else {
            c.misses.entry(kind.as_str())
        }
        .or_default() += 1;
        sections
    }

    fn put(&self, kind: EntryKind, key: Hash128, sections: &[&[u8]]) {
        let s: Vec<(u16, u32, &[u8])> = sections
            .iter()
            .enumerate()
            .map(|(i, b)| (u16::try_from(i).expect("sections"), 0, *b))
            .collect();
        let bytes = hd_cache::encode_entry(kind, LAYOUT, key, self.toolchain, 0, &s);
        self.host.store.put(kind, &key, &bytes);
    }

    fn skim_of(&self, m: usize) -> &SkimOut {
        self.skim[m].get_or_init(|| self.skim_now(m))
    }

    fn skim_now(&self, m: usize) -> SkimOut {
        let text = &self.texts[m];
        let sk = skim(text.as_bytes());
        let module = &self.table.modules[m].path;
        let roots = self.table.use_roots(ModuleId::from_raw(u32_of(m)));
        let mut uses = written_uses(text, &sk.uses, &roots);
        // A `tests:` block's uses into another package, such as a dev
        // dependency (`module.test.dev-dependency.in-tests`): the block is
        // checked with its module, so their folders join its closure. They
        // cannot close a folder cycle of this package, unlike its uses of
        // this package, which make no edge (`module.cycle.test-code`).
        let package = self.table.modules[m].package;
        for b in sk.bodies.iter().filter(|b| b.kind == HeaderKind::Tests) {
            let block = text
                .get(b.body_start as usize..b.body_end as usize)
                .unwrap_or("");
            for line in block.lines() {
                let Some(rest) = line.trim().strip_prefix("use ") else {
                    continue;
                };
                let path = rest.split(".{").next().unwrap_or(rest);
                let path = path.split(" as ").next().unwrap_or(path).trim();
                let segs: Vec<&str> = path.split('.').collect();
                if let Ok(abs) = roots.absolute_in_tests(&segs) {
                    let abs = abs.join(".");
                    let other = self
                        .table
                        .module_of_use(&abs)
                        .is_some_and(|t| self.table.modules[t.idx()].package != package);
                    if other {
                        uses.push(abs);
                    }
                }
            }
        }
        // Test code has the prelude's `std.testing` row
        // (`module.prelude.test-only`), and its test cases run bodies of
        // `std.testing` (`case_timeout` for a `timeout`), so that folder
        // joins its closure; std's own test code makes no such edge.
        let test_code = self.table.modules[m].role == hd_project::Role::Test
            || sk.bodies.iter().any(|b| b.kind == HeaderKind::Tests);
        let testing_elsewhere = self
            .table
            .module_of_use("std.testing")
            .is_some_and(|t| self.table.modules[t.idx()].package != package);
        if test_code && testing_elsewhere {
            uses.push("std.testing".to_owned());
        }
        // A REPL input sees `src/lib.hd` and the earlier inputs through
        // its session scope, so their folders join its closure.
        uses.extend(self.session_modules(m));
        // The prelude's fixed uses (`module.prelude.fixed-uses`).
        if module != "std.core" {
            uses.extend(hd_resolve::prelude_modules().into_iter().map(str::to_owned));
        }
        let facts = ModuleFacts {
            path: self.table.modules[m].path.clone(),
            has_tests_block: sk.bodies.iter().any(|b| b.kind == HeaderKind::Tests),
        };
        let main_header = |public: bool| {
            sk.bodies
                .iter()
                .filter(|b| b.kind == HeaderKind::Function && b.header_indent == 0)
                .find(|b| {
                    let header = text
                        .get(b.header_start as usize..b.body_start as usize)
                        .unwrap_or("");
                    main_visibility(header) == Some(public)
                })
                .map(|b| (b.header_start, b.body_start))
        };
        let public_main = main_header(true);
        let private_main = main_header(false);
        SkimOut {
            source_hash: sk.source_hash,
            api_text_hash: sk.api_text_hash,
            token_hash: sk.token_hash,
            code_lines: sk.code_lines,
            uses,
            facts,
            public_main,
            private_main,
        }
    }

    fn skim(&self, m: usize) {
        self.skim_of(m);
        lock(&self.report).ok(Stage::Skim);
    }

    /// The full parse, on demand (the "full parse if needed" of M1). Its
    /// diagnostics are the user's.
    fn parse_of(&self, m: usize) -> &Parse {
        self.parse[m].get_or_init(|| {
            let p = parse(self.texts[m].as_bytes());
            {
                let mut c = lock(&self.counters);
                c.parsed.push(self.table.modules[m].path.clone());
                *c.tasks.entry("Parse").or_default() += 1;
            }
            if p.is_ok() {
                lock(&self.report).ok(Stage::Parse);
            } else {
                let code = p.diagnostics[0].code.as_str();
                lock(&self.report)
                    .not_implemented(Stage::Parse, &format!("full parser reports {code}"));
                let mut d = lock(&self.diags);
                for x in &p.diagnostics {
                    let span = Span {
                        file: FileId::from_raw(u32_of(m)),
                        ..x.primary
                    };
                    d.push(x.code, x.severity, span, &x.message, None);
                }
            }
            p
        })
    }

    fn src(&self, m: usize) -> Src<'_> {
        Src {
            parse: self.parse_of(m),
            text: &self.texts[m],
            file: FileId::from_raw(u32_of(m)),
        }
    }

    /// Whether module `m` of the root package is an entry module that the
    /// entry-point rules cover (`module.entry.private-main.warn`,
    /// `module.entry.parameters`): a root file other than the library
    /// root, an executable's module, or the file of a single-file program.
    fn entry_module(&self, m: usize) -> bool {
        let module = &self.table.modules[m];
        let single_file = self
            .table
            .sources
            .get(module.file.idx())
            .is_some_and(|(_, file)| hd_project::Root::of(file).0 == hd_project::Root::Flat);
        module.package == 0 && (module.entry || single_file)
    }

    fn folder_graph(&self, sp: &dyn Spawn) {
        let uses: Vec<Vec<String>> = (0..self.table.modules.len())
            .map(|m| self.skim_of(m).uses.clone())
            .collect();
        let graph = FolderGraph::build(&self.table, &uses);
        lock(&self.report).ok(Stage::FolderGraph);
        // `cli.exe.unselected-main`: a public `main` under the source root
        // of a module no executable names is an ordinary function.
        for (m, module) in self.table.modules.iter().enumerate() {
            let source = self.table.files[m].starts_with("src/");
            if module.package != 0 || module.entry || module.role != Role::Lib || !source {
                continue;
            }
            if let Some((lo, hi)) = self.skim_of(m).public_main {
                let span = Span {
                    file: module.file,
                    lo,
                    hi,
                };
                lock(&self.diags).push(
                    Code::UnselectedMain,
                    Severity::Warning,
                    span,
                    "this public `main` is an ordinary function, since no executable names its module; name it in an `[[executable]]` table of `hd.toml` to run it",
                    None,
                );
            }
        }
        // `module.entry.private-main.warn`: a `main` or `main!` that is not
        // `pub` in an entry module is an ordinary function.
        for (m, module) in self.table.modules.iter().enumerate() {
            if !self.entry_module(m) {
                continue;
            }
            if let Some((lo, hi)) = self.skim_of(m).private_main {
                let span = Span {
                    file: module.file,
                    lo,
                    hi,
                };
                lock(&self.diags).push(
                    Code::PrivateMain,
                    Severity::Warning,
                    span,
                    "main is not pub, so it is not the entry point",
                    None,
                );
            }
        }
        for c in &graph.cycles {
            if c.iter().any(|f| self.table.folders[f.idx()].package != 0) {
                continue;
            }
            let names: Vec<&str> = c
                .iter()
                .map(|f| self.table.folders[f.idx()].path.as_str())
                .collect();
            let span = Span {
                file: FileId::from_raw(u32::MAX),
                lo: 0,
                hi: 0,
            };
            lock(&self.diags).error(
                Code::FolderCycle,
                span,
                &format!("folders form a cycle: {}", names.join(" -> ")),
            );
        }
        let pr = sp.add_held(TaskKind::PackageResult, &[]);
        let order = graph.order.clone();
        let _ = self.graph.set(GraphOut {
            graph,
            package_result: pr,
        });
        let g = &self.graph.get().expect("set above").graph;
        let mut iface_task: Vec<Option<TaskId>> = vec![None; self.table.folders.len()];
        for &f in &order {
            let deps: Vec<TaskId> = g.uses[f.idx()]
                .iter()
                .filter_map(|u| iface_task[u.idx()])
                .collect();
            let t = sp.add(TaskKind::FolderIface(f.raw()), &deps);
            iface_task[f.idx()] = Some(t);
            let closure: Vec<TaskId> = g.closure[f.idx()]
                .iter()
                .filter_map(|c| iface_task[c.idx()])
                .collect();
            let hc = sp.add(TaskKind::HeaderCheck(f.raw()), &closure);
            sp.edge(hc, pr);
        }
        let all: Vec<TaskId> = iface_task.iter().flatten().copied().collect();
        let coh = sp.add(TaskKind::Coherence, &all);
        sp.edge(coh, pr);
        // A program's collection crosses packages (codegen.md §13.2): the
        // std modules its folders reach are checked to TIR too.
        let mut reached = vec![false; self.table.folders.len()];
        if !self.analyze() {
            for module in &self.table.modules {
                if module.package == 0 {
                    for c in g.closure[module.folder.idx()].iter() {
                        reached[c.idx()] = true;
                    }
                }
            }
        }
        let mut prep_task: Vec<Option<TaskId>> = vec![None; self.table.modules.len()];
        for (m, module) in self.table.modules.iter().enumerate() {
            if module.package != 0 && !reached[module.folder.idx()] {
                continue;
            }
            let deps: Vec<TaskId> = g.closure[module.folder.idx()]
                .iter()
                .filter_map(|c| iface_task[c.idx()])
                .collect();
            // A REPL input's task is held until its edges from the
            // earlier inputs' are in.
            let kind = TaskKind::ModulePrep(u32_of(m));
            let prep = if self.repl_number(m).is_some() {
                sp.add_held(kind, &deps)
            } else {
                sp.add(kind, &deps)
            };
            sp.edge(prep, pr);
            prep_task[m] = Some(prep);
        }
        // A REPL input's bodies read the earlier inputs' bindings from their
        // `check` entries: its `ModulePrep` waits for theirs, which name the
        // task that reads each entry (`check_task`).
        for &(_, m) in &self.repl_inputs().sorted {
            for &(_, k) in self.earlier_inputs(m) {
                if let (Some(from), Some(to)) = (prep_task[k], prep_task[m]) {
                    sp.edge(from, to);
                }
            }
        }
        for &(_, m) in &self.repl_inputs().sorted {
            if let Some(prep) = prep_task[m] {
                sp.release(prep);
            }
        }
        sp.release(pr);
    }

    fn closure(&self, folder: FolderId) -> Vec<FolderId> {
        self.graph
            .get()
            .map(|g| g.graph.closure[folder.idx()].iter().collect())
            .unwrap_or_default()
    }

    fn iface_of(&self, f: FolderId) -> Option<Arc<FolderIface>> {
        self.iface
            .get(f.idx())
            .and_then(|s| s.get())
            .cloned()
            .flatten()
    }

    /// The package key of a folder's package: the root's, or another
    /// package's identity, so two packages of one name never share an
    /// entry (`module.dep.identity`).
    fn package_key_of(&self, package: u16) -> Hash128 {
        if package == 0 {
            self.package_key
        } else {
            hd_cache::package_key(&self.table.identities[usize::from(package)])
        }
    }

    /// The other folders an interface names, with their deep hashes
    /// (§4.11.3): `None` when one of them is not built.
    fn mentions(
        &self,
        fid: FolderId,
        items: &[Item],
        exports: &[hd_resolve::Export],
    ) -> Option<Vec<(String, Hash128)>> {
        let names = self.names();
        let mut folders = std::collections::BTreeSet::new();
        for d in hd_resolve::mentioned_defs(&self.pool, items, exports) {
            if let Some(f) = self.table.folder_of_module(&names.module_of(d))
                && f != fid
            {
                folders.insert(f.raw());
            }
        }
        folders
            .into_iter()
            .map(|f| {
                let f = FolderId::from_raw(f);
                Some((
                    self.table.folders[f.idx()].path.clone(),
                    self.iface_of(f)?.deep_hash,
                ))
            })
            .collect()
    }

    /// Re-emits a folder's stage-A diagnostics against this run's files,
    /// with the severity each was reported with.
    fn emit_iface_diags(&self, fi: usize, diags: &IfaceDiags) {
        let folder = &self.table.folders[fi];
        let mut d = lock(&self.diags);
        for (m, code, severity, anchor, msg) in diags {
            let Some(mid) = folder.modules.get(*m) else {
                continue;
            };
            let span = if self.parse_of(mid.idx()).is_ok() {
                hd_resolve::anchor::resolve(&self.src(mid.idx()), anchor)
            } else {
                None
            }
            .unwrap_or(Span {
                file: FileId::from_raw(mid.raw()),
                lo: 0,
                hi: 0,
            });
            d.push(*code, *severity, span, msg, None);
        }
    }

    /// `FolderIface(F)` (resolution-and-interfaces.md §4.10).
    fn folder_iface(&self, fi: usize) {
        let folder = &self.table.folders[fi];
        let fid = folder.id;
        let reach: Vec<FolderId> = self
            .closure(fid)
            .into_iter()
            .filter(|c| *c != fid)
            .collect();
        let mut reach_hashes = Vec::new();
        for c in &reach {
            if let Some(i) = self.iface_of(*c) {
                reach_hashes.push((self.table.folders[c.idx()].path.as_str(), i.deep_hash));
            } else {
                self.blocked(Stage::FolderIface);
                let _ = self.iface[fi].set(None);
                return;
            }
        }
        let apis: Vec<FileApi<'_>> = folder
            .modules
            .iter()
            .map(|m| FileApi {
                module: &self.table.modules[m.idx()].path,
                role: ROLE,
                api_text_hash: self.skim_of(m.idx()).api_text_hash,
            })
            .collect();
        let key = iface_key(
            self.toolchain,
            self.package_key_of(folder.package),
            &folder.path,
            &apis,
            &reach_hashes,
        );
        let names = self.names();
        if let Some(sections) = self.lookup(EntryKind::Iface, key)
            && let (Some(blob), Some(dsec)) = (sections.first(), sections.get(1))
            && let Some((items, exports)) = hd_resolve::decode_items(&names, blob)
            && let Some(diags) = decode_iface_diags(dsec)
            && let Some(rows) = sections.get(2).and_then(|b| hd_resolve::anchor::decode(b))
            && let Some(private) = sections
                .get(3)
                .and_then(|b| hd_resolve::decode_private_names(&names, b))
            && let Some(looped) = sections
                .get(4)
                .and_then(|b| hd_resolve::decode_private_names(&names, b))
            && let Some(mentions) = self.mentions(fid, &items, &exports)
        {
            let mut iface = hd_resolve::folder_iface(
                &folder.path,
                items,
                exports,
                Arc::from(blob.as_slice()),
                &mentions,
            );
            iface.spans = spans_of(&iface.items, &rows);
            iface.private_names = private.into_iter().collect();
            iface.loop_names = looped.into_iter().collect();
            self.emit_iface_diags(fi, &diags);
            self.note_iface(&folder.path, &iface, false);
            lock(&self.report).ok(Stage::FolderIface);
            let _ = self.iface[fi].set(Some(Arc::new(iface)));
            return;
        }
        for m in &folder.modules {
            if !self.parse_of(m.idx()).is_ok() {
                self.blocked(Stage::FolderIface);
                let _ = self.iface[fi].set(None);
                return;
            }
        }
        let mods: Vec<hd_resolve::ModIn<'_>> = folder
            .modules
            .iter()
            .map(|m| self.mod_in(m.idx()))
            .collect();
        let cx = hd_resolve::Cx {
            names,
            folder: fid.raw(),
            world: self,
        };
        let mut diags = DiagBuf::default();
        let out = match hd_resolve::build_folder(&cx, &mods, None, Stage::FolderIface, &mut diags) {
            Ok(o) => o,
            Err(e) => {
                self.stage::<()>(Stage::FolderIface, Err(e));
                let _ = self.iface[fi].set(None);
                return;
            }
        };
        let mut anchored = Vec::new();
        for (mi, m) in out.modules.iter().enumerate() {
            for (def, slot, a) in &m.anchors {
                anchored.push((*def, u32_of(mi), *slot, *a));
            }
        }
        let all: Vec<Item> = out.modules.into_iter().flat_map(|m| m.items).collect();
        for it in &all {
            if let Some(k) = it.intrinsic
                && hd_host_abi::intrinsic(names.text(k)).is_none()
            {
                let msg = format!(
                    "`{}` names the unknown intrinsic `{}`",
                    names.path(it.def),
                    names.text(k)
                );
                let span = Span {
                    file: FileId::from_raw(folder.modules[0].raw()),
                    lo: 0,
                    hi: 0,
                };
                diags.error(Code::Unsupported, span, &msg);
            }
        }
        // A REPL input's interface carries every item it declares, for
        // later inputs, which see them whatever their `pub`.
        let input = folder
            .modules
            .iter()
            .any(|m| self.repl_number(m.idx()).is_some());
        let items = if input {
            all
        } else {
            hd_resolve::interface_items(&all)
        };
        let blob = match hd_resolve::encode_items(&names, &items, &out.exports) {
            Ok(b) => b,
            Err(e) => {
                self.stage::<()>(Stage::FolderIface, Err(e));
                let _ = self.iface[fi].set(None);
                return;
            }
        };
        let idiags: IfaceDiags = (0..diags.len())
            .map(|i| {
                let sp = diags.primary[i];
                let m = folder
                    .modules
                    .iter()
                    .position(|x| x.raw() == sp.file.raw())
                    .unwrap_or(0);
                (
                    m,
                    diags.code[i],
                    diags.severity[i],
                    hd_resolve::anchor::of_span(&self.src(folder.modules[m].idx()), sp.lo, sp.hi),
                    diags.get_text(diags.message[i]).to_owned(),
                )
            })
            .collect();
        let dsec = encode_iface_diags(&idiags);
        let by_def: HashMap<DefId, u32> = items
            .iter()
            .enumerate()
            .map(|(i, it)| (it.def, u32_of(i)))
            .collect();
        let rows: Vec<(u32, u32, u32, hd_resolve::anchor::Anchor)> = anchored
            .into_iter()
            .filter_map(|(d, m, slot, a)| Some((*by_def.get(&d)?, m, slot, a)))
            .collect();
        let asec = hd_resolve::anchor::encode(&rows);
        let psec = hd_resolve::encode_private_names(&names, &out.private_names);
        let lsec = hd_resolve::encode_private_names(&names, &out.loop_names);
        self.put(EntryKind::Iface, key, &[&blob, &dsec, &asec, &psec, &lsec]);
        let Some(mentions) = self.mentions(fid, &items, &out.exports) else {
            self.blocked(Stage::FolderIface);
            let _ = self.iface[fi].set(None);
            return;
        };
        let mut iface =
            hd_resolve::folder_iface(&folder.path, items, out.exports, Arc::from(blob), &mentions);
        iface.spans = spans_of(&iface.items, &rows);
        iface.private_names = out.private_names.into_iter().collect();
        iface.loop_names = out.loop_names.into_iter().collect();
        lock(&self.diags).append(&diags);
        self.note_iface(&folder.path, &iface, true);
        lock(&self.report).ok(Stage::FolderIface);
        let _ = self.iface[fi].set(Some(Arc::new(iface)));
    }

    fn note_iface(&self, path: &str, iface: &FolderIface, built: bool) {
        let mut c = lock(&self.counters);
        if built {
            c.ifaces_built.push(path.to_owned());
        }
        c.deep_hashes.insert(path.to_owned(), iface.deep_hash);
        c.iface_blobs
            .insert(path.to_owned(), hd_base::hash128(&iface.blob));
    }

    fn mod_in(&self, m: usize) -> hd_resolve::ModIn<'_> {
        let path = self.table.modules[m].path.clone();
        let seeds = hd_resolve::seed::items(&self.names(), &path);
        hd_resolve::ModIn {
            path,
            roots: self.table.use_roots(ModuleId::from_raw(u32_of(m))),
            src: self.src(m),
            seeds,
            tests: false,
            session: self.session_scope(m),
        }
    }

    /// The number of the REPL input module `m` is, if it is one
    /// ([`Goal::ReplInput`]).
    fn repl_number(&self, m: usize) -> Option<u32> {
        self.repl_inputs().numbers[m]
    }

    fn repl_inputs(&self) -> &ReplInputs {
        self.repl_inputs.get_or_init(|| {
            let numbers: Vec<Option<u32>> = (0..self.table.modules.len())
                .map(|k| {
                    (self.table.modules[k].package == 0)
                        .then(|| hd_project::repl_input(&self.table.files[k]))
                        .flatten()
                })
                .collect();
            let mut sorted: Vec<(u32, usize)> = numbers
                .iter()
                .enumerate()
                .filter_map(|(k, n)| Some(((*n)?, k)))
                .collect();
            sorted.sort_unstable();
            ReplInputs { sorted, numbers }
        })
    }

    /// The root package's `src/lib.hd`, if it has one.
    fn lib_module(&self) -> Option<usize> {
        self.table
            .sources
            .iter()
            .position(|(p, file)| *p == 0 && file == "src/lib.hd")
    }

    /// The inputs before REPL input `m`, in order; none for any other
    /// module.
    fn earlier_inputs(&self, m: usize) -> &[(u32, usize)] {
        let sorted = &self.repl_inputs().sorted;
        let n = self
            .repl_number(m)
            .map_or(0, |n| sorted.partition_point(|(k, _)| *k < n));
        &sorted[..n]
    }

    /// The modules REPL input `m` sees through its session scope:
    /// `src/lib.hd`, then the earlier inputs in order. None for any other
    /// module.
    fn session_modules(&self, m: usize) -> Vec<String> {
        if self.repl_number(m).is_none() {
            return Vec::new();
        }
        let earlier = self.earlier_inputs(m).iter().map(|(_, k)| *k);
        self.lib_module()
            .into_iter()
            .chain(earlier)
            .map(|k| self.table.modules[k].path.clone())
            .collect()
    }

    /// `src/lib.hd` lowered against its frozen interface, once per run.
    fn lib_scope(&self) -> Option<&LibScope> {
        self.lib_scope
            .get_or_init(|| {
                let lib = self.lib_module()?;
                let o = self
                    .lower_module(lib, Stage::ModulePrep, &mut DiagBuf::default())
                    .ok()??;
                let private = o
                    .items
                    .into_iter()
                    .filter(|i| !i.public && !matches!(i.data, ItemData::Impl { .. }))
                    .collect();
                Some(LibScope {
                    scope: o.scope,
                    kinds: o.kinds,
                    private,
                })
            })
            .as_ref()
    }

    /// The session scope of REPL input `m` (`cli.repl.package.lib`):
    /// `src/lib.hd`'s scope, with the top-level names of each earlier input
    /// nested in it, the newest last. An input exports every top-level
    /// name it declares or uses (`hd_resolve::ModIn::session`), so its
    /// interface's exports are those names.
    fn session_scope(&self, m: usize) -> Option<Arc<hd_resolve::SessionScope>> {
        self.repl_number(m)?;
        let scope = self.session_scopes[m].get_or_init(|| {
            let mut out = self
                .lib_scope()
                .map_or_else(hd_resolve::SessionScope::default, |lib| {
                    hd_resolve::SessionScope {
                        scope: lib.scope.clone(),
                        kinds: lib.kinds.clone(),
                        declared_by: lib
                            .scope
                            .name
                            .iter()
                            .zip(&lib.scope.origin)
                            .filter(|(_, o)| **o != hd_resolve::Origin::Prelude)
                            .map(|(&n, _)| (n, "src/lib.hd".to_owned()))
                            .collect(),
                    }
                });
            for &(number, k) in self.earlier_inputs(m) {
                let module = &self.table.modules[k];
                let Some(iface) = self.iface_of(module.folder) else {
                    continue;
                };
                let mut inner = hd_resolve::ModuleScope::default();
                for e in iface.exports.iter().filter(|e| e.module == module.path) {
                    let b = hd_resolve::Binding {
                        kind: hd_resolve::BindingKind::Item,
                        value: e.def.raw(),
                    };
                    inner.bind(e.name, b, hd_resolve::Origin::Use, u32::MAX);
                    out.kinds.insert(e.def, e.kind);
                    out.declared_by.insert(e.name, format!("repl#{number}"));
                }
                out.scope = out.scope.overlay(&inner);
            }
            Arc::new(out)
        });
        Some(Arc::clone(scope))
    }

    /// One module's items (private ones included), scope and kinds,
    /// against its folder's frozen interface. Use and header diagnostics
    /// are the folder interface's, so `diags` here is the caller's to drop.
    fn lower_module(
        &self,
        m: usize,
        stage: Stage,
        diags: &mut DiagBuf,
    ) -> StageResult<Option<ModOut>> {
        let module = &self.table.modules[m];
        let Some(own) = self.iface_of(module.folder) else {
            return Ok(None);
        };
        let cx = hd_resolve::Cx {
            names: self.names(),
            folder: module.folder.raw(),
            world: self,
        };
        // In the test role, its `tests:` block's items are its items too.
        let mods = [hd_resolve::ModIn {
            tests: self.role(m) == "test",
            ..self.mod_in(m)
        }];
        let out = hd_resolve::build_folder(&cx, &mods, Some(own), stage, diags)?;
        Ok(out.modules.into_iter().next())
    }

    /// The scope of each module of the root package, as text (`hd doc`
    /// links): the module's own declarations and the names its `use`s bind.
    fn doc_scopes(&self) -> Vec<(String, Vec<(String, hd_resolve::Named)>)> {
        let names = self.names();
        let mut out = Vec::new();
        for (m, module) in self.table.modules.iter().enumerate() {
            if module.package != 0 || !self.parse_of(m).is_ok() {
                continue;
            }
            let lowered = self.lower_module(m, Stage::ModulePrep, &mut DiagBuf::default());
            if let Ok(Some(o)) = lowered {
                out.push((module.path.clone(), o.scope.named(&names)));
            }
        }
        out
    }

    /// Every interface of a folder's closure (its own included).
    fn closure_ifaces(&self, fid: FolderId) -> Option<Vec<Arc<FolderIface>>> {
        self.closure(fid)
            .into_iter()
            .map(|c| self.iface_of(c))
            .collect()
    }

    /// Where an item, or one of its written types (`slot`), sits in its
    /// file: resolved from the interface's declaration-relative anchors
    /// against this run's parse. Falls back to the start of the module file.
    fn item_span(&self, d: DefId, slot: u32) -> Span {
        let Some(mid) = self.table.module(&self.names().module_of(d)) else {
            return Span {
                file: FileId::from_raw(u32::MAX),
                lo: 0,
                hi: 0,
            };
        };
        let module = &self.table.modules[mid.idx()];
        let fallback = Span {
            file: module.file,
            lo: 0,
            hi: 0,
        };
        let Some(iface) = self.iface_of(module.folder) else {
            return fallback;
        };
        let Some((m, anchor)) = iface
            .spans
            .get(&(d, slot))
            .or_else(|| iface.spans.get(&(d, 0)))
        else {
            return fallback;
        };
        let Some(target) = self.table.folders[module.folder.idx()]
            .modules
            .get(*m as usize)
        else {
            return fallback;
        };
        if !self.parse_of(target.idx()).is_ok() {
            return fallback;
        }
        hd_resolve::anchor::resolve(&self.src(target.idx()), anchor).unwrap_or(fallback)
    }

    /// `HeaderCheck(F)`: stage B (§4.10.1) over F's interface, its goals
    /// asked through the solver with the impls of `closure(F)` and its
    /// impl universe, taken when the task starts (trait-solver.md §3.2).
    fn header_check(&self, fi: usize) {
        let fid = self.table.folders[fi].id;
        let (Some(own), Some(all)) = (self.iface_of(fid), self.closure_ifaces(fid)) else {
            self.blocked(Stage::HeaderCheck);
            return;
        };
        let closure = self.closure(fid);
        let folder = &self.table.folders[fi];
        let reach: Vec<(&str, Hash128)> = closure
            .iter()
            .zip(&all)
            .map(|(c, i)| (self.table.folders[c.idx()].path.as_str(), i.deep_hash))
            .collect();
        // A tuple template's member steps come from its body, which no
        // interface carries (trait-solver.md §3.9): its source is a
        // dependency of the findings, as it is of a module's check.
        let Some(templates) = self.tuple_template_sources(fid) else {
            self.blocked(Stage::HeaderCheck);
            return;
        };
        let reach: Vec<(&str, Hash128)> = reach
            .into_iter()
            .chain(templates.iter().map(|(p, h)| (p.as_str(), *h)))
            .collect();
        let key = hdr_key(
            self.toolchain,
            self.package_key_of(folder.package),
            &folder.path,
            own.deep_hash,
            &reach,
        );
        if let Some(sections) = self.lookup(EntryKind::Graph, key)
            && let Some(found) = sections
                .first()
                .and_then(|b| decode_header_findings(b, &own.items))
        {
            lock(&self.report).ok(Stage::HeaderCheck);
            self.emit_findings(&found);
            return;
        }
        self.computed(Stage::HeaderCheck);
        let names = self.names();
        let lookup = Lookup::new(&[], all.iter().map(AsRef::as_ref).collect());
        let (folders, universe, extra) = self.closure_impls(&closure);
        let arity = |d: DefId| lookup.item(d).map_or(0, |i| i.generics.len());
        let impls = ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: None,
            folders: &folders,
            universe,
            extra: &extra,
            arity: &arity,
            hidden: &[],
            plans: Some(self),
        };
        let cx = hd_check::header::HeaderCx {
            names,
            lookup: &lookup,
            impls: &impls,
            global: &self.memo,
            solver: &TableSolver,
        };
        let r = hd_check::header::stage_b(&cx, &own.items);
        let Some(findings) = self.stage(Stage::HeaderCheck, r) else {
            return;
        };
        self.put(
            EntryKind::Graph,
            key,
            &[&encode_header_findings(&findings, &own.items)],
        );
        self.emit_findings(&findings);
    }

    fn emit_findings(&self, findings: &[hd_check::header::Finding]) {
        let mut d = lock(&self.diags);
        for f in findings {
            d.error(f.code, self.item_span(f.item, f.slot), &f.message);
        }
    }

    /// `InitOrder(F)` (§4.13.10; spec 10 "Order Inside A Cycle"): the
    /// statement order of each module cycle of the folder whose statements
    /// sit in several modules, from the modules' init summaries, or
    /// `top-level-read-before-initialization` where the statements wait on
    /// each other (`module.init.cycle.stuck`). The order is a `graph` part
    /// keyed by `init_key` over the summaries, so a warm run with no change
    /// to what it reads reads it (cache.md, "Part keys").
    fn init_order(&self, fi: usize) {
        let folder = &self.table.folders[fi];
        let names = self.names();
        let mut cycles: Vec<Vec<(usize, &[u8])>> = Vec::new();
        for cycle in self.folder_cycles(fi) {
            let mut sums = Vec::new();
            for m in cycle {
                let Some(Some(c)) = self.check[m].get() else {
                    self.blocked(Stage::InitOrder);
                    return;
                };
                // A module with errors stops the build before collection.
                if c.has_errors {
                    sums.clear();
                    break;
                }
                let Some(sections) = split_sections(&c.entry) else {
                    self.blocked(Stage::InitOrder);
                    return;
                };
                sums.push((m, sections.get(5).copied().unwrap_or(&[])));
            }
            if !sums.is_empty() {
                cycles.push(sums);
            }
        }
        let summaries: Vec<(&str, Hash128)> = cycles
            .iter()
            .flatten()
            .map(|(m, b)| {
                let mut k = hd_base::StableHasher::new("init-summary");
                k.bytes(b);
                (self.table.modules[*m].path.as_str(), k.finish())
            })
            .collect();
        let key = init_key(self.toolchain, &folder.path, &summaries);
        if let Some(sections) = self.lookup(EntryKind::Graph, key)
            && let Some(runs) = sections.first().and_then(|b| decode_runs(b))
        {
            let _ = self.init_runs[fi].set(runs);
            lock(&self.report).ok(Stage::InitOrder);
            return;
        }
        self.computed(Stage::InitOrder);
        let mut out = Vec::new();
        let mut stuck = false;
        for cycle in &cycles {
            let mut sums = Vec::new();
            for (m, b) in cycle {
                let Some(sum) = decode_summary(b, &names, &self.lines(*m)) else {
                    self.stage::<()>(
                        Stage::InitOrder,
                        Err(NotImplemented::new(
                            Stage::InitOrder,
                            "a module's init summary did not decode",
                        )),
                    );
                    return;
                };
                sums.push(sum);
            }
            let refs: Vec<&hd_check::init::InitSummary> = sums.iter().collect();
            match hd_check::stages::cycle_order(&refs) {
                hd_check::stages::CycleOrder::Steps(steps) => {
                    let runs = hd_check::stages::runs(&steps)
                        .into_iter()
                        .map(|(i, r)| {
                            let stmts = &sums[i].stmts;
                            let lo = if r.start == 0 {
                                0
                            } else {
                                stmts[r.start].first_inst
                            };
                            let hi = stmts.get(r.end).map_or(u32::MAX, |s| s.first_inst);
                            InitRun {
                                module: self.table.modules[cycle[i].0].path.clone(),
                                start: u32_of(r.start),
                                end: u32_of(r.end),
                                insts: lo..hi,
                            }
                        })
                        .collect();
                    out.push(runs);
                }
                hd_check::stages::CycleOrder::Stuck {
                    at: (i, k),
                    waits_on,
                } => {
                    stuck = true;
                    let st = &sums[i].stmts[k];
                    let late = names
                        .paths
                        .segment(hd_base::PathId::from_raw(waits_on.raw()))
                        .to_owned();
                    let msg = format!(
                        "this statement reads `{late}` before its initialization: the statements of its module cycle wait on each other"
                    );
                    let span = Span {
                        file: self.table.modules[cycle[i].0].file,
                        lo: st.lo,
                        hi: st.hi,
                    };
                    lock(&self.diags).error(Code::TopLevelReadBeforeInitialization, span, &msg);
                }
            }
        }
        // An initialization cycle is reported again by every run.
        if !stuck {
            self.put(EntryKind::Graph, key, &[&encode_runs(&out)]);
        }
        let _ = self.init_runs[fi].set(out);
        lock(&self.report).ok(Stage::InitOrder);
    }

    /// The modules each module uses directly, by index, itself left out.
    fn use_edges(&self, m: usize) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .skim_of(m)
            .uses
            .iter()
            .filter_map(|u| self.table.module_of_use(u))
            .map(hd_base::ModuleId::idx)
            .filter(|x| *x != m)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The module cycles of folder `fi` whose statements sit in several
    /// modules, each in module identity order (`module.init.cycle`). A
    /// cycle never leaves its folder (`module.init.use-graph`), and only a
    /// checked module's summary counts its statements.
    fn folder_cycles(&self, fi: usize) -> Vec<Vec<usize>> {
        let folder = FolderId::from_raw(u32_of(fi));
        let mods = &self.table.folders[fi].modules;
        if mods.len() < 2 {
            return Vec::new();
        }
        let mut edges: Vec<Vec<usize>> = vec![Vec::new(); self.table.modules.len()];
        for m in mods {
            edges[m.idx()] = self
                .use_edges(m.idx())
                .into_iter()
                .filter(|w| self.table.modules[*w].folder == folder)
                .collect();
        }
        let starts: Vec<usize> = mods.iter().map(|m| m.idx()).collect();
        let (groups, _) = strongly_connected(&edges, &starts);
        let initializes = |m: usize| match self.check[m].get() {
            Some(Some(c)) => split_sections(&c.entry)
                .and_then(|s| s.get(5).map(|b| summary_statements(b) > 0))
                .unwrap_or(false),
            _ => false,
        };
        groups
            .into_iter()
            .filter(|g| g.len() > 1 && g.iter().filter(|m| initializes(**m)).count() > 1)
            .map(|mut g| {
                g.sort_by(|a, b| {
                    self.table.modules[*a]
                        .path
                        .cmp(&self.table.modules[*b].path)
                });
                g
            })
            .collect()
    }

    /// Counts a `graph` part this run computed instead of reading.
    fn computed(&self, s: Stage) {
        *lock(&self.counters)
            .parts_computed
            .entry(s.name())
            .or_default() += 1;
    }

    /// `Coherence`: the overlap check (§4.12.3) over the impl heads of
    /// every interface of the program graph, std's included.
    fn coherence(&self) {
        let mut all = Vec::new();
        for f in 0..self.table.folders.len() {
            let Some(i) = self.iface_of(FolderId::from_raw(u32_of(f))) else {
                self.blocked(Stage::Coherence);
                return;
            };
            all.push(i);
        }
        // The result is a function of every interface: a warm run with no
        // interface change reads it (cache.md, "Part keys"; this key is
        // coarser than the per-trait `coh_key`).
        let key = {
            let mut k = hd_base::StableHasher::new("coh");
            k.hash(self.toolchain);
            for (f, i) in self.table.folders.iter().zip(&all) {
                k.str(&f.path);
                k.hash(i.deep_hash);
            }
            k.finish()
        };
        if let Some(sections) = self.lookup(EntryKind::Graph, key)
            && let Some(found) = sections.first().and_then(|b| decode_overlaps(b, &all))
        {
            lock(&self.report).ok(Stage::Coherence);
            let mut d = lock(&self.diags);
            for (item, msg) in found {
                d.error(Code::OverlappingImpl, self.item_span(item, 0), &msg);
            }
            return;
        }
        self.computed(Stage::Coherence);
        let names = self.names();
        let u = hd_resolve::Universe::new(names, all.iter().flat_map(|i| i.items.iter()));
        // Content order (§4.12.3): the later impl is the one reported. What
        // `@error` generates ranks first, so a hand-written impl beside it
        // is the one reported (spec 14 `annot.error.hand-written`); a
        // `@derive` ranks after written impls, a derivation block after both.
        let order = |d: DefId| {
            let rank = all
                .iter()
                .find_map(|i| i.item(d))
                .map_or(1, |it| match &it.data {
                    ItemData::Impl {
                        kind: hd_resolve::ImplKind::Error,
                        ..
                    } => 0,
                    ItemData::Impl {
                        kind: hd_resolve::ImplKind::Derived,
                        ..
                    } => 2,
                    ItemData::Impl {
                        kind: hd_resolve::ImplKind::Derivation,
                        ..
                    } => 3,
                    _ => 1,
                });
            let at = all
                .iter()
                .find_map(|i| i.spans.get(&(d, 0)))
                .map_or((0, 0, 0), |(_, a)| (a.decl, a.member, a.tok));
            let module = names.module_of(d);
            // std first: its impls are the earlier ones.
            let package = u8::from(!module.starts_with("std."));
            format!(
                "{package}{rank}\0{module}\0{:010}{:010}{:010}\0{}",
                at.0,
                at.1,
                at.2,
                names.path(d)
            )
        };
        let overlaps = u.overlaps(&order);
        lock(&self.report).ok(Stage::Coherence);
        let found: Vec<(DefId, String)> = overlaps
            .into_iter()
            .map(|(a, b, witness)| {
                let msg = format!(
                    "implementations {} and {} both apply to {witness}",
                    names.path(a),
                    names.path(b)
                );
                (b, msg)
            })
            .collect();
        self.put(EntryKind::Graph, key, &[&encode_overlaps(&found, &all)]);
        let mut d = lock(&self.diags);
        for (item, msg) in found {
            d.error(Code::OverlappingImpl, self.item_span(item, 0), &msg);
        }
    }

    fn check_key(&self, m: usize) -> Option<Hash128> {
        let module = &self.table.modules[m];
        let templates = self.template_sources(m)?;
        let mut closure = Vec::new();
        for c in self.closure(module.folder) {
            closure.push((
                self.table.folders[c.idx()].path.as_str(),
                self.iface_of(c)?.deep_hash,
            ));
        }
        closure.extend(templates.iter().map(|(p, h)| (p.as_str(), *h)));
        // A REPL input sees `src/lib.hd`'s private declarations, which no
        // interface carries (`cli.repl.package.lib`).
        if self.repl_number(m).is_some()
            && let Some(lib) = self.lib_module()
        {
            closure.push(("$lib", self.skim_of(lib).token_hash));
        }
        // Nor does one carry an earlier input's bindings, whose types come
        // from its statements (`cli.repl.declarations.bindings`).
        for &(_, k) in self.earlier_inputs(m) {
            closure.push(("$input", self.skim_of(k).token_hash));
        }
        // A dependency's module is checked against the root's profile, so
        // its key holds both packages.
        let package = if module.package == 0 {
            self.package_key
        } else {
            let mut k = hd_base::StableHasher::new("pkg-dep");
            k.hash(self.package_key);
            k.hash(self.package_key_of(module.package));
            k.finish()
        };
        // An entry module's check holds its entry-point findings.
        let role = if self.entry_module(m) {
            format!("{}+entry", self.role(m))
        } else {
            self.role(m).to_owned()
        };
        Some(check_key(
            self.toolchain,
            package,
            &module.path,
            &role,
            self.skim_of(m).token_hash,
            &closure,
        ))
    }

    /// The source hashes of the template modules a module's opt-ins may
    /// instantiate. An opt-in checks its template's body, which no
    /// interface carries yet (spec 14 `annot.limit.interfaces`), so a
    /// template edit must reach the key of every module that opts in. A
    /// module whose text names neither `derive` nor `Structure` opts in
    /// nowhere, but any module may use a tuple template, whose member
    /// steps come from its body (trait-solver.md §3.9). std's sources are
    /// in the toolchain key already.
    fn template_sources(&self, m: usize) -> Option<Vec<(String, Hash128)>> {
        let text = &self.texts[m];
        let opts_in = text.contains("derive") || text.contains("Structure");
        self.template_hashes(self.table.modules[m].folder, opts_in)
    }

    /// The source hashes of the user tuple-template modules in
    /// `closure(folder)`: what `HeaderCheck(folder)` reads through a
    /// template's member steps, which come from its body and from nothing
    /// an interface carries (trait-solver.md §3.9). The walkers' bounds
    /// are in the interfaces of the key's reach already, so the template
    /// modules' text is all that is missing. std's templates are in the
    /// toolchain key.
    fn tuple_template_sources(&self, folder: FolderId) -> Option<Vec<(String, Hash128)>> {
        self.template_hashes(folder, false)
    }

    /// The source hashes of the user template modules in
    /// `closure(folder)`: every derivation template with `opts_in`, the
    /// tuple templates always.
    fn template_hashes(&self, folder: FolderId, opts_in: bool) -> Option<Vec<(String, Hash128)>> {
        let mut out = std::collections::BTreeMap::new();
        for c in self.closure(folder) {
            let iface = self.iface_of(c)?;
            let paths = self.folder_templates[c.idx()].get_or_init(|| {
                let names = self.names();
                let mut ps: Vec<(String, bool)> = iface
                    .items
                    .iter()
                    .filter(|it| {
                        matches!(
                            it.data,
                            ItemData::Impl {
                                kind: hd_resolve::ImplKind::Template
                                    | hd_resolve::ImplKind::TupleTemplate,
                                ..
                            }
                        )
                    })
                    .map(|it| {
                        let tuple = matches!(
                            it.data,
                            ItemData::Impl {
                                kind: hd_resolve::ImplKind::TupleTemplate,
                                ..
                            }
                        );
                        (names.module_of(it.def), tuple)
                    })
                    .filter(|(p, _)| !p.starts_with("std."))
                    .collect();
                ps.sort();
                ps.dedup();
                ps
            });
            for (p, tuple) in paths {
                if !opts_in && !tuple {
                    continue;
                }
                if let Some(tm) = self.table.module(p) {
                    out.insert(format!("template {p}"), self.skim_of(tm.idx()).token_hash);
                }
            }
        }
        Some(out.into_iter().collect())
    }

    /// `ModulePrep(m)`: the `check` key lookup first; only on a miss does
    /// it lower headers and create `Body(m)`, `ModuleFinish(m)` and
    /// `TestOverlay(m)` (scheduler.md §6.1).
    fn module_prep(&self, id: TaskId, m: u32, sp: &dyn Spawn) {
        let mi = m as usize;
        let body = self.module_prep_now(id, mi, sp);
        let _ = self.check_task[mi].set(body.unwrap_or(id));
    }

    /// `module_prep`; the module's `ModuleFinish` task when it checks
    /// the module's bodies.
    fn module_prep_now(&self, id: TaskId, mi: usize, sp: &dyn Spawn) -> Option<TaskId> {
        let m = u32_of(mi);
        let Some(key) = self.check_key(mi) else {
            for s in [Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
                self.blocked(s);
            }
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return None;
        };
        lock(&self.counters)
            .check_keys
            .insert(self.table.modules[mi].path.clone(), key);
        if let Some(sections) = self.lookup(EntryKind::Check, key) {
            // The key ignores comments and blank lines, which can still
            // break the parse (a detached documentation comment).
            if !self.parses(mi) {
                for s in [Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
                    self.blocked(s);
                }
                let _ = self.prep[mi].set(None);
                let _ = self.check[mi].set(None);
                return None;
            }
            lock(&self.report).ok(Stage::ModulePrep);
            self.read_check(mi, &sections, Arc::from(join_sections(&sections)));
            let _ = self.prep[mi].set(None);
            return None;
        }
        if !self.parses(mi) {
            for s in [Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
                self.blocked(s);
            }
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return None;
        }
        let mut scratch = DiagBuf::default();
        let items = self
            .lower_module(mi, Stage::ModulePrep, &mut scratch)
            .map(|o| o.map(|o| o.items));
        let Some(Some(items)) = self.stage(Stage::ModulePrep, items) else {
            self.blocked(Stage::Body);
            self.blocked(Stage::ModuleFinish);
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return None;
        };
        let _ = self.prep[mi].set(Some(PrepOut {
            items,
            diags: DiagBuf::default(),
        }));
        let pr = self.graph.get().map(|g| g.package_result)?;
        // A REPL input's bodies read the earlier inputs' bindings.
        let deps: Vec<TaskId> = std::iter::once(id)
            .chain(
                self.earlier_inputs(mi)
                    .iter()
                    .filter_map(|&(_, k)| self.check_task[k].get().copied()),
            )
            .collect();
        let body = sp.add(TaskKind::Body(m), &deps);
        let finish = sp.add(TaskKind::ModuleFinish(m), &[body]);
        let overlay = sp.add(TaskKind::TestOverlay(m), &[id]);
        sp.edge(finish, pr);
        sp.edge(overlay, pr);
        Some(finish)
    }

    /// Whether the module's bytes parse without a diagnostic: a `parse`
    /// entry says so, else the parse runs and a clean one is recorded.
    fn parses(&self, m: usize) -> bool {
        let key = parse_key(self.toolchain, self.skim_of(m).source_hash);
        if self.lookup(EntryKind::Parse, key).is_some() {
            return true;
        }
        let ok = self.parse_of(m).is_ok();
        if ok {
            self.put(EntryKind::Parse, key, &[]);
        }
        ok
    }

    /// The module's code lines, which place the `check` entry's positions.
    fn lines(&self, m: usize) -> Lines<'_> {
        Lines {
            starts: &self.skim_of(m).code_lines,
            len: u32_of(self.texts[m].len()),
        }
    }

    /// Which folder declares what: every module's path node.
    fn owners(&self) -> &OwnerMap {
        self.owners.get_or_init(|| {
            let names = self.names();
            let mut o = OwnerMap::default();
            for m in &self.table.modules {
                o.insert(names.module(&m.path), m.folder);
            }
            o
        })
    }

    /// A folder's impls, frozen with its interface (trait-solver.md §3.2,
    /// §3.3); `None` when the interface was not built.
    fn folder_impls(&self, f: FolderId) -> Option<&FolderImpls> {
        self.folder_impls[f.idx()]
            .get_or_init(|| {
                let iface = self.iface_of(f)?;
                let impls: Vec<&Item> = iface
                    .items
                    .iter()
                    .filter(|x| matches!(x.data, ItemData::Impl { .. }))
                    .collect();
                let table = hd_resolve::impl_table(&self.names(), &impls);
                let fi = FolderImpls::new(f, table, self.pool.types(), &self.paths, self.owners());
                Some(Arc::new(fi))
            })
            .as_deref()
    }

    /// A solving context's impls (trait-solver.md §3.2): the closure's
    /// folder tables by folder id, read by owner, and its impl universe,
    /// the closure's folders whose `arg_impls` or unowned rows are not
    /// empty.
    fn closure_impls(
        &self,
        closure: &[FolderId],
    ) -> (
        Vec<Option<&FolderImpls>>,
        ImplUniverseId,
        Arc<UniverseImpls>,
    ) {
        let mut folders: Vec<Option<&FolderImpls>> = vec![None; self.table.folders.len()];
        let mut members = Vec::new();
        for f in closure {
            let fi = self.folder_impls(*f);
            folders[f.idx()] = fi;
            if let Some(fi) = fi.filter(|fi| fi.in_universe()) {
                members.push((*f, fi));
            }
        }
        let (universe, extra) = self.universes.intern(&members);
        (folders, universe, extra)
    }

    /// Whether a module's file already has an error: its header errors are
    /// reported by the folder interface, before any body of it.
    fn file_has_errors(&self, m: usize) -> bool {
        let d = lock(&self.diags);
        let file = FileId::from_raw(u32_of(m));
        d.severity
            .iter()
            .zip(&d.primary)
            .any(|(s, at)| *s == Severity::Error && at.file == file)
    }

    /// `Body(m)`: every body of the module, in source order.
    fn body(&self, m: usize) {
        let Some(Some(prep)) = self.prep[m].get() else {
            self.blocked(Stage::Body);
            let _ = self.body[m].set(None);
            return;
        };
        let module = &self.table.modules[m];
        let closure = self.closure(module.folder);
        let ifaces: Vec<Arc<FolderIface>> =
            closure.iter().filter_map(|f| self.iface_of(*f)).collect();
        // A REPL input also sees `src/lib.hd`'s private items
        // (`cli.repl.package.lib`), which no interface carries.
        let input = self.repl_number(m).is_some();
        let own: std::borrow::Cow<'_, [Item]> = match self.lib_scope().filter(|_| input) {
            Some(lib) => prep.items.iter().chain(&lib.private).cloned().collect(),
            None => std::borrow::Cow::Borrowed(&prep.items),
        };
        let lookup = Lookup::new(&own, ifaces.iter().map(AsRef::as_ref).collect());
        let names = self.names();
        let own_impls: Vec<&Item> = prep
            .items
            .iter()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }))
            .collect();
        let own_table = hd_resolve::impl_table(&names, &own_impls);
        let (folders, universe, extra) = self.closure_impls(&closure);
        let arity = |d: DefId| lookup.item(d).map_or(0, |i| i.generics.len());
        let impls = ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: Some(&own_table),
            folders: &folders,
            universe,
            extra: &extra,
            arity: &arity,
            hidden: &[],
            plans: Some(self),
        };
        let src = self.src(m);
        // A test run checks the `tests:` block's items with the module's
        // (`names.tests.module-items`), each seeing the block's scope.
        let tests = self.role(m) == "test";
        let heads = hd_resolve::heads(&names, &src, &module.path, tests);
        let Ok(Some(lowered)) = self.lower_module(m, Stage::Body, &mut DiagBuf::default()) else {
            self.blocked(Stage::Body);
            let _ = self.body[m].set(None);
            return;
        };
        let scope = lowered.scope;
        let blocks: Vec<Span> = src
            .root()
            .children()
            .filter(|c| c.kind() == hd_syntax::SyntaxKind::TestsBlock)
            .map(|b| src.span(b))
            .collect();
        let solver = TableSolver;
        let locals = hd_resolve::local_items(&heads);
        let cx = BodyCx {
            names,
            src,
            scope: &scope,
            lookup: &lookup,
            impls: &impls,
            global: &self.memo,
            solver: &solver,
            methods: std::cell::OnceCell::new(),
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::after(
                self.earlier_bindings(m),
                self.declared_by(m),
            )),
            results: std::cell::RefCell::default(),
            dbg_refs: std::cell::RefCell::default(),
            locals: &locals,
            tests: lowered.tests.as_ref().map(|scope| hd_check::TestsView {
                blocks: &blocks,
                scope,
            }),
            file_name: &self.table.files[m],
        };
        let mut diags = DiagBuf::default();
        hd_check::init::earlier_binding_clashes(&cx, &heads, &mut diags);
        // The `tests:` block's use and header diagnostics, which no folder
        // interface reports.
        diags.append(&lowered.test_diags);
        // `grammar.tests.once`: a file's second and later `tests:` blocks.
        // `module.test.no-tests-block`: no block of a test module or an
        // integration test. Each offending block gets one diagnostic.
        for (i, block) in blocks.iter().enumerate() {
            if module.role == Role::Test {
                diags.error(
                    Code::MisplacedTestsBlock,
                    *block,
                    "a test module holds its test cases at top level; it has no `tests:` block",
                );
            } else if i > 0 {
                diags.error(
                    Code::DuplicateTestsBlock,
                    *block,
                    "a file has at most one `tests:` block",
                );
            }
        }
        let mut bodies = Vec::new();
        let mut failed = None;
        // M1 (checking-and-tir.md §4.13.1 "Omitted result types"): the
        // private callables without a result type are checked before
        // their callers, from the top-level statements on.
        let methods = hd_resolve::body_nodes(&names, &src, &heads);
        hd_check::results::omitted_results(&cx, &methods);
        // Top-level statements first: their bindings are visible to every
        // function body of the module (checking-and-tir.md §4.13.10).
        let mut stmts = hd_check::init::init_statements(src.root());
        // In test code, a top-level registration call is in test position
        // (`module.testing.test-position`): it registers a test case below,
        // and is not a module initialization statement.
        let mut test_stmts = Vec::new();
        if module.role == Role::Test {
            (test_stmts, stmts) = stmts
                .into_iter()
                .partition(|s| hd_check::tests::is_registration(&cx, *s));
        }
        let mut init_facts = Vec::new();
        // A test program has no entry module: every top level is
        // requirement-free. Every REPL input is an entry, whichever input
        // the run builds: its top level runs as the input does.
        let entry_name = match &self.goal {
            Goal::Program { entry } | Goal::Scenario { entry } => Some(entry.as_str()),
            Goal::Tests { .. } | Goal::ReplInput { .. } => None,
            Goal::Analyze | Goal::CheckTests => Some("main"),
        };
        let entry = input
            || (module.package == 0
                && entry_name.is_some_and(|e| {
                    module.path == format!("{}.{e}", hd_project::package_ident(&self.package))
                }));
        // `module.init.script-empty`: a program's entry module with no
        // public `main` is a script even with no statements, and its empty
        // init is the whole program. A REPL input is always one.
        let empty_script = stmts.is_empty()
            && (input
                || (entry
                    && matches!(self.goal, Goal::Program { .. })
                    && self.skim_of(m).public_main.is_none()));
        if !stmts.is_empty() || empty_script {
            let item = hd_check::default_body_def(&names, names.item(&module.path, "init"), "init");
            let top = if input {
                hd_check::init::TopLevel::Input
            } else if entry {
                hd_check::init::TopLevel::Entry
            } else {
                hd_check::init::TopLevel::Module
            };
            match hd_check::init::check_init(&cx, item, &module.path, &stmts, top, &mut diags) {
                Ok((b, facts)) => {
                    lock(&self.report).body_ok += 1;
                    init_facts = facts;
                    bodies.push(b);
                }
                Err(e) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = e.what.chars().take(90).collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures
                        .push(format!("{} init: {}", module.path, e.what));
                    drop(r);
                    failed.get_or_insert(e);
                }
            }
        }
        // The rest of M1, in source order; each body it checks is final.
        hd_check::results::infer_results(&cx);
        diags.append(&hd_check::results::module_diags(&cx));
        // A written implementation writes every required trait method
        // (spec 09 `trait.impl.required`), reported at its header.
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Impl)
        {
            let missing = hd_check::omitted_trait_methods(&names, &lookup, h.def);
            let faults = hd_check::assoc_faults(&lookup, h.def);
            let mut omitted = Vec::new();
            if !missing.is_empty() {
                omitted.push(format!("write `{}`", missing.join("`, `")));
            }
            if !faults.omitted.is_empty() {
                let types: Vec<&str> = faults
                    .omitted
                    .iter()
                    .map(|d| names.display_name(*d))
                    .collect();
                omitted.push(format!("bind `{}`", types.join("`, `")));
            }
            if !omitted.is_empty() {
                let msg = format!(
                    "the implementation does not {}",
                    omitted.join(" and does not ")
                );
                diags.error(Code::MissingTraitMethod, src.span(h.node), &msg);
            }
            // An associated type the trait does not declare
            // (spec 09 `trait.impl.extra-methods`), at its binding.
            for extra in faults.extra {
                let name = names.display_name(extra);
                let decl = hd_resolve::Src::child(h.node, hd_syntax::SyntaxKind::Block)
                    .into_iter()
                    .flat_map(hd_syntax::NodeRef::children)
                    .find(|c| {
                        c.kind() == hd_syntax::SyntaxKind::AssociatedTypeDecl
                            && c.name(&src.parse.tokens)
                                .is_some_and(|t| src.text(t) == name)
                    });
                let msg = format!("the trait does not declare an associated type `{name}`");
                let at = decl.map_or_else(|| src.span(h.node), |d| src.span(d));
                diags.error(Code::ExtraTraitMember, at, &msg);
            }
        }
        // Member promotion conflicts are declaration errors (spec 03
        // `names.conflict.error`), reported on the embedded field or on the
        // private member.
        let mut inherent = None;
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Data)
        {
            let Some(item) = lookup.item(h.def) else {
                continue;
            };
            let embeds =
                matches!(&item.data, ItemData::Data(fields) if fields.iter().any(|f| f.embedded));
            if !embeds {
                continue;
            }
            let inherent = inherent
                .get_or_insert_with(|| hd_check::conflicts::inherent_methods(&names, &lookup));
            for c in hd_check::conflicts::promotion_conflicts(&names, &lookup, inherent, item) {
                let node = match c.field {
                    Some(i) => hd_resolve::Src::child(h.node, hd_syntax::SyntaxKind::Block)
                        .into_iter()
                        .flat_map(hd_syntax::NodeRef::children)
                        .filter(|f| {
                            matches!(
                                f.kind(),
                                hd_syntax::SyntaxKind::DataField
                                    | hd_syntax::SyntaxKind::EmbeddedField
                            )
                        })
                        .nth(i),
                    None => methods.iter().find(|(d, _)| *d == c.item).map(|(_, n)| *n),
                };
                if let Some(node) = node {
                    diags.error(Code::AmbiguousPromotedMember, src.span(node), &c.message);
                }
            }
        }
        for (def, node) in methods {
            // A body-less method of a built-in family (`impl[N < Num] Add
            // for N`) is the compiler's: there is no source to check.
            if hd_resolve::Src::child(node, hd_syntax::SyntaxKind::Block).is_none() {
                continue;
            }
            // A method whose impl header was rejected has no item
            // (`unknown-trait`): that error is the answer, there is no
            // signature to check the body against.
            if lookup.item(def).is_none() && self.file_has_errors(m) {
                continue;
            }
            let checked = match hd_check::results::take(&cx, def) {
                Some((checked, own)) => {
                    diags.append(&own);
                    checked
                }
                None => hd_check::check_fn(&cx, def, node, &mut diags),
            };
            match checked {
                Ok(b) => {
                    lock(&self.report).body_ok += 1;
                    let path = names.path(def);
                    if self.host.render_tir.iter().any(|p| *p == path) {
                        lock(&self.tir_text).insert(path, hd_check::render(&names, &b));
                    }
                    bodies.push(b);
                }
                Err(e) => {
                    // Analysis counts every body; a build stops at the first.
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = e
                        .what
                        .split(" @")
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(90)
                        .collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures
                        .push(format!("{}: {}", names.path(def), e.what));
                    drop(r);
                    failed.get_or_insert(e);
                    if !self.analyze() {
                        break;
                    }
                }
            }
        }
        // The method bodies of the implementations `@error` generates
        // (spec 14 `annot.error.generates`, codegen.md §13.14).
        for h in heads.iter().filter(|h| {
            matches!(
                h.kind,
                hd_resolve::HeadKind::Data | hd_resolve::HeadKind::Enum
            )
        }) {
            if failed.is_some() && !self.analyze() {
                break;
            }
            let Some(shape) = hd_resolve::error_type::shape(&src, h.node) else {
                continue;
            };
            match hd_check::error::error_bodies(&cx, h.def, &shape, &mut diags) {
                Ok(bs) => {
                    lock(&self.report).body_ok += bs.len();
                    for b in &bs {
                        let path = names.path(b.item);
                        if self.host.render_tir.iter().any(|p| *p == path) {
                            lock(&self.tir_text).insert(path, hd_check::render(&names, b));
                        }
                    }
                    bodies.extend(bs);
                }
                Err(e) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = e.what.chars().take(90).collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures
                        .push(format!("{} @error: {}", names.path(h.def), e.what));
                    drop(r);
                    failed.get_or_insert(e);
                }
            }
        }
        // The forwarding methods of the delegating implementations
        // (spec 09 `trait.by.generated`).
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Impl)
        {
            if failed.is_some() && !self.analyze() {
                break;
            }
            match hd_check::delegate::forwarder_bodies(&cx, h.def, h.node, &mut diags) {
                Ok(bs) => {
                    lock(&self.report).body_ok += bs.len();
                    for b in &bs {
                        let path = names.path(b.item);
                        if self.host.render_tir.iter().any(|p| *p == path) {
                            lock(&self.tir_text).insert(path, hd_check::render(&names, b));
                        }
                    }
                    bodies.extend(bs);
                }
                Err(e) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = e.what.chars().take(90).collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures
                        .push(format!("{} by: {}", names.path(h.def), e.what));
                    drop(r);
                    failed.get_or_insert(e);
                }
            }
        }
        // The methods of `@derive` on a newtype: the base type's, applied
        // to the wrapped values (spec 09 `trait.derive.newtype.base`).
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Newtype)
        {
            if failed.is_some() && !self.analyze() {
                break;
            }
            match hd_check::newtype::derived_newtype_bodies(&cx, h.def, h.node, &mut diags) {
                Ok((bs, findings)) => {
                    lock(&self.report).body_ok += bs.len();
                    for b in &bs {
                        let path = names.path(b.item);
                        if self.host.render_tir.iter().any(|p| *p == path) {
                            lock(&self.tir_text).insert(path, hd_check::render(&names, b));
                        }
                    }
                    bodies.extend(bs);
                    for f in findings {
                        diags.error(
                            Code::NewtypeDerivationSelf,
                            self.item_span(f.impl_, 0),
                            &f.message,
                        );
                    }
                }
                Err(e) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = e.what.chars().take(90).collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures
                        .push(format!("{} derive: {}", names.path(h.def), e.what));
                    drop(r);
                    failed.get_or_insert(e);
                }
            }
        }
        // A test run checks the `tests:` blocks too, into the module's
        // test-role `check` entry (`check_key` role "test"). A test module
        // or an integration test module is test code throughout: its
        // top-level registrations are checked in every role.
        if self.role(m) == "test" {
            test_stmts.extend(hd_check::tests::tests_block_statements(&src));
        }
        if !test_stmts.is_empty() && failed.is_none() {
            let profile = self.test_profile(m);
            match hd_check::tests::check_tests(&cx, &module.path, &test_stmts, profile, &mut diags)
            {
                Ok(t) => {
                    lock(&self.report).body_ok += t.bodies.len();
                    bodies.extend(t.bodies);
                    let _ = self.tests[m].set((t.items, t.regs));
                }
                Err(e) => {
                    lock(&self.report)
                        .body_failures
                        .push(format!("{} tests: {}", module.path, e.what));
                    failed.get_or_insert(e);
                }
            }
        }
        // Default bodies: data field defaults and parameter defaults
        // (checking-and-tir.md "Default calls").
        let mut defaults = Vec::new();
        for h in &heads {
            if h.kind == hd_resolve::HeadKind::Data {
                for f in h.node.descendants() {
                    if f.kind() == hd_syntax::SyntaxKind::DataField
                        && let Some(dv) =
                            hd_resolve::Src::child(f, hd_syntax::SyntaxKind::DefaultValue)
                        && let Some(e) = dv.children().next()
                        && let Some(t) = f.name(&src.parse.tokens)
                    {
                        defaults.push((h.def, src.text(t).to_owned(), e));
                    }
                }
            }
        }
        for (def, node) in hd_resolve::body_nodes(&names, &src, &heads) {
            let Some(pl) = hd_resolve::Src::child(node, hd_syntax::SyntaxKind::ParameterList)
            else {
                continue;
            };
            for p in pl.children() {
                if let Some(dv) = hd_resolve::Src::child(p, hd_syntax::SyntaxKind::DefaultValue)
                    && let Some(e) = dv.children().next()
                    && let Some(t) = p.name(&src.parse.tokens)
                {
                    defaults.push((def, src.text(t).to_owned(), e));
                }
            }
        }
        for (owner, name, e) in defaults {
            match hd_check::check_default(&cx, owner, &name, e, &mut diags) {
                Ok(b) => {
                    lock(&self.report).body_ok += 1;
                    bodies.push(b);
                }
                Err(err) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    let short: String = err.what.chars().take(90).collect();
                    *r.body_reasons.entry(short).or_default() += 1;
                    r.body_failures.push(format!(
                        "{} default {name}: {}",
                        names.path(owner),
                        err.what
                    ));
                    drop(r);
                    failed.get_or_insert(err);
                }
            }
        }
        // The facts body of each function whose decorators attach values,
        // which `facts_of` reads (spec 14 `annot.facts-of.result`).
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Fn)
            .filter(|h| !hd_resolve::facts::fact_lines(&src, h.node).is_empty())
        {
            match hd_check::check_facts(&cx, h.def, h.node, &mut diags) {
                Ok(b) => {
                    lock(&self.report).body_ok += 1;
                    bodies.push(b);
                }
                Err(err) => {
                    let mut r = lock(&self.report);
                    r.body_failed += 1;
                    r.body_failures
                        .push(format!("{} facts: {}", names.path(h.def), err.what));
                    drop(r);
                    failed.get_or_insert(err);
                }
            }
        }
        // The fact and metadata expressions of the declarations, their
        // members and their derivation blocks, checked though nothing reads
        // them (spec 14 `annot.metadata.eval-as-fact`).
        for h in &heads {
            hd_check::check_decl_facts(&cx, h.def, h.node, &mut diags);
        }
        for h in &heads {
            hd_check::check_duplicate_facts(&cx, &heads, h, &mut diags);
        }
        if !init_facts.is_empty() {
            hd_check::init::definite_init(&cx, &stmts, &init_facts, &mut diags);
        }
        // A template needs a body in which a method calls `walk`,
        // `describe` or `build` of `Structure` (`annot.template.body`). A
        // misplaced template is already one error, and a body that did not
        // finish says nothing about its calls.
        if failed.is_none() {
            for h in heads
                .iter()
                .filter(|h| h.kind == hd_resolve::HeadKind::Impl)
            {
                let Some(it) = lookup.item(h.def) else {
                    continue;
                };
                if matches!(
                    &it.data,
                    ItemData::Impl {
                        kind: hd_resolve::ImplKind::Template | hd_resolve::ImplKind::TupleTemplate,
                        ..
                    }
                ) && !hd_resolve::lower::misplaced_template(&names, &module.path, it)
                    && !hd_check::results::reads_structure(&cx, h.def)
                {
                    diags.error(
                        Code::MarkerTemplate,
                        src.span(h.node),
                        "a template must have a body that calls `walk`, `describe`, or `build`",
                    );
                }
            }
        }
        // The reports that template bodies left for the module: they are
        // checked after the first sweep took the module's reports.
        diags.append(&hd_check::results::module_diags(&cx));
        self.private_headers(&cx, &heads, &mut diags);
        self.profile_boundaries(&cx, &heads, &mut diags);
        self.entry_points(m, &cx, &heads, &mut diags);
        // Each derivation opt-in checks its template's instance
        // (spec 14 `annot.template.checked`, checking-and-tir.md §4.13.9),
        // whose bodies join the module's.
        let mut derived = Derived::default();
        if failed.is_none() {
            derived = self.opt_ins(m, &cx, &heads, &impls, &mut diags);
        }
        if let Some(e) = failed {
            self.stage::<()>(Stage::Body, Err(e));
            let _ = self.body[m].set(None);
            return;
        }
        lock(&self.report).body_ok += derived.bodies.len();
        for b in &derived.bodies {
            let path = names.path(b.item);
            if self.host.render_tir.iter().any(|p| *p == path) {
                lock(&self.tir_text).insert(path, hd_check::render(&names, b));
            }
        }
        bodies.extend(derived.bodies);
        lock(&self.report).ok(Stage::Body);
        let inferred = hd_check::results::inferred(&cx);
        let folder = module.folder;
        let same_folder =
            |d: DefId| self.table.folder_of_module(&names.module_of(d)) == Some(folder);
        let summary = hd_check::init::summary(&cx, init_facts, &same_folder);
        let dbg = cx.dbg_refs.take();
        // A REPL input's interface carries its bindings for later inputs.
        let bindings = if input {
            hd_check::init::input_bindings(&cx)
        } else {
            Vec::new()
        };
        let _ = self.body[m].set(Some((
            bodies,
            diags,
            derived.items,
            inferred,
            summary,
            dbg,
            bindings,
        )));
    }

    /// What declared each name REPL input `m` sees: `src/lib.hd`, or the
    /// earlier input that declared it, bindings included
    /// (`cli.repl.redeclare.binding`). Empty for any other module.
    fn declared_by(&self, m: usize) -> HashMap<hd_base::Symbol, String> {
        let names = self.names();
        let mut out = self
            .session_scope(m)
            .map(|s| s.declared_by.clone())
            .unwrap_or_default();
        for &(number, k) in self.earlier_inputs(m) {
            for b in self.input_bindings[k].get().into_iter().flatten() {
                let name = names
                    .paths
                    .segment(hd_base::PathId::from_raw(b.def.raw()))
                    .to_owned();
                out.insert(names.syms.intern(&name), format!("repl#{number}"));
            }
        }
        out
    }

    /// The top-level bindings of the inputs before REPL input `m`, by name,
    /// from their input interfaces; a newer input's name wins. Empty for
    /// any other module.
    fn earlier_bindings(&self, m: usize) -> HashMap<hd_base::Symbol, InputBinding> {
        let names = self.names();
        let mut out = HashMap::new();
        for &(_, k) in self.earlier_inputs(m) {
            for b in self.input_bindings[k].get().into_iter().flatten() {
                let name = names
                    .paths
                    .segment(hd_base::PathId::from_raw(b.def.raw()))
                    .to_owned();
                out.insert(names.syms.intern(&name), *b);
            }
        }
        out
    }

    /// Stage B (§4.10.1) over the module's items that `HeaderCheck` cannot
    /// see: its private items and the impls that name one, under the
    /// module's own lookup (`hd_check::header::private_items`).
    fn private_headers(
        &self,
        cx: &BodyCx<'_>,
        heads: &[hd_resolve::Head<'_>],
        diags: &mut DiagBuf,
    ) {
        let hcx = hd_check::header::HeaderCx {
            names: cx.names,
            lookup: cx.lookup,
            impls: cx.impls,
            global: cx.global,
            solver: cx.solver,
        };
        let Ok(findings) = hd_check::header::private_items(&hcx, cx.lookup.own) else {
            return;
        };
        for f in findings {
            let span = hd_resolve::anchor::span_in_module(
                &cx.names,
                &cx.src,
                heads,
                cx.lookup.own,
                (f.item, f.slot),
            )
            .unwrap_or_else(|| self.item_span(f.item, f.slot));
            diags.error(f.code, span, &f.message);
        }
    }

    /// The boundary checks of the host capability traits of the run's
    /// runtime profile that this module declares
    /// (`hd_check::header::boundary`, `module.boundary.error`).
    fn profile_boundaries(
        &self,
        cx: &BodyCx<'_>,
        heads: &[hd_resolve::Head<'_>],
        diags: &mut DiagBuf,
    ) {
        let traits: Vec<DefId> = self
            .profile
            .iter()
            .filter_map(|p| {
                let (module, name) = p.rsplit_once('.')?;
                Some(cx.names.item(module, name))
            })
            .collect();
        let hcx = hd_check::header::HeaderCx {
            names: cx.names,
            lookup: cx.lookup,
            impls: cx.impls,
            global: cx.global,
            solver: cx.solver,
        };
        for it in cx.lookup.own.iter().filter(|it| traits.contains(&it.def)) {
            let Ok(findings) = hd_check::header::boundary(&hcx, it) else {
                continue;
            };
            for f in findings {
                let span = hd_resolve::anchor::span_in_module(
                    &cx.names,
                    &cx.src,
                    heads,
                    cx.lookup.own,
                    (f.item, f.slot),
                )
                .unwrap_or_else(|| self.item_span(f.item, f.slot));
                diags.error(f.code, span, &f.message);
            }
        }
    }

    /// The entry point of module `m`, when it is an entry module with a
    /// public top-level `main` or `main!`: its parameters, requirement row
    /// and result are checked once here, so `check` reports them
    /// (`hd_check::header::entry_point`, `module.entry.definition`).
    fn entry_points(
        &self,
        m: usize,
        cx: &BodyCx<'_>,
        heads: &[hd_resolve::Head<'_>],
        diags: &mut DiagBuf,
    ) {
        if !self.entry_module(m) || self.table.modules[m].role == Role::Test {
            return;
        }
        let host: Vec<DefId> = self
            .profile
            .iter()
            .filter_map(|p| {
                let (module, name) = p.rsplit_once('.')?;
                Some(cx.names.item(module, name))
            })
            .collect();
        let hcx = hd_check::header::HeaderCx {
            names: cx.names,
            lookup: cx.lookup,
            impls: cx.impls,
            global: cx.global,
            solver: cx.solver,
        };
        for h in heads.iter().filter(|h| {
            h.kind == hd_resolve::HeadKind::Fn
                && h.public
                && h.local.is_none()
                && h.test.is_none()
                && cx.names.display_name(h.def) == "main"
        }) {
            let Some(it) = cx.lookup.own.iter().find(|i| i.def == h.def) else {
                continue;
            };
            let Ok(findings) = hd_check::header::entry_point(&hcx, it, &host) else {
                continue;
            };
            for f in findings {
                let span = Self::entry_span(cx, h.node, &f);
                diags.error(f.code, span, &f.message);
            }
        }
    }

    /// Where an entry-point finding points: the function's name, its
    /// written result type, or the requirement key that names the trait.
    /// A key written under another name points at the name.
    fn entry_span(
        cx: &BodyCx<'_>,
        node: hd_syntax::NodeRef<'_>,
        f: &hd_check::header::EntryFinding,
    ) -> Span {
        use hd_check::header::EntryAt;
        let name = cx
            .src
            .name_after(node, hd_syntax::TokenKind::KwFn)
            .map_or_else(|| cx.src.span(node), |t| cx.src.token_span(t));
        match f.at {
            EntryAt::Name => name,
            EntryAt::Result => hd_resolve::Src::type_child(node).map_or(name, |t| cx.src.span(t)),
            EntryAt::Key(def) => {
                let wanted = cx.names.display_name(def);
                hd_resolve::Src::child(node, hd_syntax::SyntaxKind::RequirementRow)
                    .into_iter()
                    .flat_map(hd_syntax::NodeRef::children)
                    .filter(|c| c.kind().is_type())
                    .find(|c| {
                        cx.src
                            .tokens(*c)
                            .filter(|t| {
                                matches!(
                                    cx.src.tkind(*t),
                                    Some(
                                        hd_syntax::TokenKind::Ident
                                            | hd_syntax::TokenKind::RawIdent
                                    )
                                )
                            })
                            .last()
                            .is_some_and(|t| cx.src.text(t) == wanted)
                    })
                    .map_or(name, |c| cx.src.span(c))
            }
        }
    }

    /// The derivation opt-ins of module `m`, its derived implementations
    /// and derivation blocks, each checked as an instance of its trait's
    /// template, in the template's module scope (checking-and-tir.md
    /// §4.13.9 "Derive instances"). A failing member is one
    /// `member-not-derivable` at the opt-in (spec 14
    /// `annot.walker.obligation.error`). `cx` and `impls` are the module's
    /// own body context and impl view.
    fn opt_ins(
        &self,
        m: usize,
        cx: &BodyCx<'_>,
        heads: &[hd_resolve::Head<'_>],
        impls: &ImplView<'_>,
        diags: &mut DiagBuf,
    ) -> Derived {
        let names = self.names();
        // Per template module, in module order: the opt-ins it checks.
        let mut groups: std::collections::BTreeMap<usize, Vec<OptInEntry>> =
            std::collections::BTreeMap::new();
        // Per data type or enum, its declaration facts bodies, built once.
        let mut decl: HashMap<DefId, Option<FactBodies>> = HashMap::new();
        let mut derived = Derived::default();
        for (i, it) in cx.lookup.own.iter().enumerate() {
            let ItemData::Impl { trait_, kind, .. } = &it.data else {
                continue;
            };
            if !matches!(
                kind,
                hd_resolve::ImplKind::Derived | hd_resolve::ImplKind::Derivation
            ) || *trait_ == DefId::NONE
            {
                continue;
            }
            // A rejected block (`misplaced-derivation`) derives nothing.
            if hd_resolve::lower::misplaced_block(
                &names,
                &names.module_of(it.def),
                cx.lookup.own,
                it,
            )
            .is_some()
            {
                continue;
            }
            let Some(template) = hd_resolve::lower::template_of(&names, cx.lookup, *trait_) else {
                // `@derive` of a trait with no template derives nothing
                // (`annot.derive.other`, `annot.derive.error-trait`).
                // A newtype derives through its base type instead, with
                // no template (`trait.derive.newtype.*`).
                if *kind == hd_resolve::ImplKind::Derived
                    && let ItemData::Impl { self_ty, .. } = &it.data
                    && let TyData::Adt { def, .. } = names.pool.get(*self_ty)
                    && !cx
                        .lookup
                        .own
                        .iter()
                        .any(|i| i.def == def && matches!(i.data, ItemData::Newtype(_)))
                    && let Some(span) = hd_resolve::lower::derive_span(
                        &names, cx.scope, &cx.src, heads, def, *trait_,
                    )
                {
                    let name = names.display_name(*trait_);
                    diags.error(
                        Code::UnderivableTrait,
                        span,
                        &format!("`{name}` has no template and cannot be derived"),
                    );
                }
                continue;
            };
            let Some(tm) = self.table.module(&names.module_of(template)) else {
                continue;
            };
            let block = heads.iter().find(|h| h.def == it.def).map(|h| h.node);
            let omitted = block
                .map(|b| hd_check::derive::omitted_members(&cx.src, b))
                .unwrap_or_default();
            if let Some(b) = block {
                hd_check::derive::check_omitted_defaults(
                    &names,
                    cx.lookup,
                    &cx.src,
                    (it, b),
                    diags,
                );
            }
            if let Some(mut opt) =
                hd_check::derive::OptIn::new(&names, cx.lookup, it, template, &omitted)
            {
                let lines = block
                    .map(|b| hd_check::derive::member_lines(&cx.src, b))
                    .unwrap_or_default();
                opt.lines = !lines.is_empty();
                let facts = derivation_facts(cx, heads, (&opt, &lines), &mut decl, &mut derived);
                groups.entry(tm.idx()).or_default().push((i, opt, facts));
            }
        }
        let mut found: Vec<OptInFinding> = Vec::new();
        for (tm, opts) in groups {
            if tm == m {
                found.extend(self.check_opt_ins(cx, heads, &opts, &mut derived));
                continue;
            }
            let Some(tmod) = self.template_mods[tm].get_or_init(|| {
                self.lower_module(tm, Stage::Body, &mut DiagBuf::default())
                    .ok()
                    .flatten()
            }) else {
                continue;
            };
            let module = &self.table.modules[m];
            let mut closure = self.closure(module.folder);
            for f in self.closure(self.table.modules[tm].folder) {
                if !closure.contains(&f) {
                    closure.push(f);
                }
            }
            let ifaces: Vec<Arc<FolderIface>> =
                closure.iter().filter_map(|f| self.iface_of(*f)).collect();
            let items: Vec<Item> = cx.lookup.own.iter().chain(&tmod.items).cloned().collect();
            let lookup = Lookup::new(&items, ifaces.iter().map(AsRef::as_ref).collect());
            let (folders, universe, extra) = self.closure_impls(&closure);
            let arity = |d: DefId| lookup.item(d).map_or(0, |i| i.generics.len());
            let view = ImplView {
                paths: &self.paths,
                owners: self.owners(),
                own: impls.own,
                folders: &folders,
                universe,
                extra: &extra,
                arity: &arity,
                hidden: &[],
                plans: Some(self),
            };
            let src = self.src(tm);
            let theads = hd_resolve::heads(&names, &src, &self.table.modules[tm].path, false);
            let tlocals = hd_resolve::local_items(&theads);
            let tcx = BodyCx {
                names,
                src,
                scope: &tmod.scope,
                lookup: &lookup,
                impls: &view,
                global: &self.memo,
                solver: &TableSolver,
                methods: std::cell::OnceCell::new(),
                init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
                results: std::cell::RefCell::default(),
                dbg_refs: std::cell::RefCell::default(),
                locals: &tlocals,
                tests: None,
                file_name: &self.table.files[tm],
            };
            found.extend(self.check_opt_ins(&tcx, &theads, &opts, &mut derived));
        }
        // A derived newtype's base type must implement the trait.
        let hcx = hd_check::header::HeaderCx {
            names: cx.names,
            lookup: cx.lookup,
            impls: cx.impls,
            global: cx.global,
            solver: cx.solver,
        };
        if let Ok(fs) = hd_check::header::derived_newtypes(&hcx, cx.lookup.own) {
            for f in fs {
                diags.error(f.code, self.item_span(f.item, f.slot), &f.message);
            }
        }
        found.sort_by_key(|f| (f.item, f.slot));
        // A field that misses several derived comparison traits is one
        // error at the field, for the first trait (spec 09
        // `trait.derive.field-missing-trait`).
        let mut seen: Vec<(DefId, u32)> = Vec::new();
        for f in found {
            if f.compare {
                if seen.contains(&(f.data, f.slot)) {
                    continue;
                }
                seen.push((f.data, f.slot));
                let def = cx.lookup.own[f.item].def;
                diags.error(
                    Code::DeriveFieldMissingTrait,
                    self.item_span(def, f.slot),
                    &f.message,
                );
            } else {
                let def = cx.lookup.own[f.item].def;
                diags.error(Code::MemberNotDerivable, self.item_span(def, 0), &f.message);
            }
        }
        derived
    }

    /// A tuple template's member steps (trait-solver.md §3.9): its
    /// methods checked once in its own module, with `T` its parameter,
    /// and the bounds of each walker, describer or source a `walk`,
    /// `describe` or `build` call there chose. That check sees no tuple
    /// template's plan and keeps a memo of its own, so it never asks the
    /// plan it computes and none of its answers reach the run's memo. A
    /// template whose body has an error, which its module reports, has no
    /// plan.
    fn compute_tuple_plan(&self, template: DefId) -> Option<Arc<[MemberStep]>> {
        let names = self.names();
        let tm = self.table.module(&names.module_of(template))?.idx();
        let tmod = self.template_mods[tm]
            .get_or_init(|| {
                self.lower_module(tm, Stage::Body, &mut DiagBuf::default())
                    .ok()
                    .flatten()
            })
            .as_ref()?;
        let closure = self.closure(self.table.modules[tm].folder);
        let ifaces: Vec<Arc<FolderIface>> =
            closure.iter().filter_map(|f| self.iface_of(*f)).collect();
        let lookup = Lookup::new(&tmod.items, ifaces.iter().map(AsRef::as_ref).collect());
        let own_impls: Vec<&Item> = tmod
            .items
            .iter()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }))
            .collect();
        let own_table = hd_resolve::impl_table(&names, &own_impls);
        let (folders, universe, extra) = self.closure_impls(&closure);
        let arity = |d: DefId| lookup.item(d).map_or(0, |i| i.generics.len());
        let view = ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: Some(&own_table),
            folders: &folders,
            universe,
            extra: &extra,
            arity: &arity,
            hidden: &[],
            plans: None,
        };
        let memo = GlobalMemo::default();
        let src = self.src(tm);
        let theads = hd_resolve::heads(&names, &src, &self.table.modules[tm].path, false);
        let tlocals = hd_resolve::local_items(&theads);
        let tcx = BodyCx {
            names,
            src,
            scope: &tmod.scope,
            lookup: &lookup,
            impls: &view,
            global: &memo,
            solver: &TableSolver,
            methods: std::cell::OnceCell::new(),
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
            results: std::cell::RefCell::default(),
            dbg_refs: std::cell::RefCell::default(),
            locals: &tlocals,
            tests: None,
            file_name: &self.table.files[tm],
        };
        let ItemData::Impl { methods, .. } = &lookup.item(template)?.data else {
            return None;
        };
        let bodies = hd_resolve::body_nodes(&names, &tcx.src, &theads);
        let sink = std::cell::RefCell::new(Vec::new());
        for (_, d) in methods {
            let Some(&(def, node)) = bodies.iter().find(|b| b.0 == *d) else {
                continue;
            };
            let mut scratch = DiagBuf::default();
            hd_check::body::check_tuple_template_method(&tcx, def, node, &mut scratch, &sink)
                .ok()?;
            if scratch.has_errors() {
                return None;
            }
        }
        let mut steps = sink.into_inner();
        // Two calls through one walker ask the same of each member.
        let mut seen = std::collections::HashSet::new();
        steps.retain(|s| seen.insert(s.clone()));
        Some(Arc::from(steps))
    }

    /// Checks opt-ins whose template module `tcx` sees: each checks the
    /// template methods its block does not write (`annot.block.methods`).
    /// A clean opt-in adds its derive instances and, once per owner, the
    /// bodies of the `Structure` it sees to `derived` (codegen.md §12.3);
    /// a `Structure` the generator does not carry yet adds no body, so a
    /// call of it stops at collection.
    fn check_opt_ins(
        &self,
        tcx: &BodyCx<'_>,
        theads: &[hd_resolve::Head<'_>],
        opts: &[OptInEntry],
        derived: &mut Derived,
    ) -> Vec<OptInFinding> {
        let names = self.names();
        let k = &self.known;
        let bodies = hd_resolve::body_nodes(&names, &tcx.src, theads);
        let mut out = Vec::new();
        for (i, opt, facts) in opts {
            let methods_of = |d: DefId| match tcx.lookup.item(d).map(|it| &it.data) {
                Some(ItemData::Impl { methods, .. }) => methods.clone(),
                _ => Vec::new(),
            };
            let written: Vec<_> = methods_of(opt.impl_).into_iter().map(|w| w.0).collect();
            let methods: Vec<_> = methods_of(opt.template)
                .into_iter()
                .filter(|(n, _)| !written.contains(n))
                .filter_map(|(_, d)| bodies.iter().find(|b| b.0 == d).copied())
                .filter(|(_, n)| hd_resolve::Src::child(*n, hd_syntax::SyntaxKind::Block).is_some())
                .collect();
            // A template the checker cannot carry yet decides nothing.
            if let Ok(checked) = hd_check::derive::check_opt_in(tcx, opt, &methods) {
                let compare = [k.eq, k.partial_ord, k.ord, k.hash].contains(&opt.trait_);
                if checked.failures.is_empty() && checked.bodies.len() == methods.len() {
                    for (def, b) in checked.bodies {
                        if let Ok((it, b)) = hd_check::derive::instance(tcx, opt, def, b) {
                            derived.items.push(it);
                            derived.bodies.push(b);
                        }
                    }
                    // The `Structure` this derivation sees, generated once
                    // per owner (`annot.structure.per-derivation`).
                    let owner = opt.structure_owner();
                    if let Some(facts) = facts
                        && !derived.structures.contains(&owner)
                        && let Ok(g) = hd_check::structure::structure_bodies(tcx, opt, facts)
                    {
                        derived.structures.push(owner);
                        derived.items.extend(g.items);
                        derived.bodies.extend(g.bodies);
                    }
                }
                out.extend(
                    checked
                        .failures
                        .into_iter()
                        .map(|(member, message)| OptInFinding {
                            item: *i,
                            data: opt.data,
                            slot: opt.members[member].slot,
                            compare,
                            message,
                        }),
                );
            }
        }
        out
    }

    /// `ModuleFinish(m)`: writes the `check` entry. Sections: diagnostics;
    /// meta (the module's TIR content hash, which `prog_key` reads); the
    /// module's items; TIR, one record per body; a test run's
    /// registrations; the init summary, which `InitOrder` reads.
    fn module_finish(&self, m: usize) {
        let (
            Some(Some(prep)),
            Some(Some((bodies, bdiags, derived, inferred, summary, dbg, bindings))),
        ) = (self.prep[m].get(), self.body[m].get())
        else {
            self.blocked(Stage::ModuleFinish);
            let _ = self.check[m].set(None);
            return;
        };
        let names = self.names();
        let lines = self.lines(m);
        let mut diags = prep.diags.clone();
        diags.append(bdiags);
        let mut dw = Writer::default();
        dw.len_of(&diags.code);
        for i in 0..diags.len() {
            dw.str(diags.code[i].as_str());
            dw.u8(u8::from(diags.severity[i] == Severity::Warning));
            lines.write(&mut dw, diags.primary[i].lo);
            lines.write(&mut dw, diags.primary[i].hi);
            dw.str(diags.get_text(diags.message[i]));
        }
        let mut tw = Writer::default();
        let mut content = hd_base::StableHasher::new("tir-content");
        let keep: &[Body] = if diags.has_errors() { &[] } else { bodies };
        tw.len_of(keep);
        for b in keep {
            let bytes = match hd_tir::wire::write_body(b, &self.pool, &self.paths, &self.syms) {
                Ok(x) => x,
                Err(e) => {
                    self.stage::<()>(Stage::ModuleFinish, Err(e));
                    let _ = self.check[m].set(None);
                    return;
                }
            };
            content.hash(names.path_hash(b.item));
            content.hash(hd_tir::wire::tir_hash(&bytes));
            tw.blob(&bytes);
        }
        // A module with errors has no TIR, so no statement to order.
        let empty = hd_check::init::InitSummary::default();
        let summary = encode_summary(
            if diags.has_errors() { &empty } else { summary },
            &names,
            &lines,
        );
        // The init order of a module cycle reads the summary, and the
        // program runs that order: the summary is part of the content
        // `prog_key` reads.
        let mut sh = hd_base::StableHasher::new("init-summary");
        sh.bytes(&summary);
        content.hash(sh.finish());
        let mut meta = Writer::default();
        meta.hash(content.finish());
        // A test run's case items join the module's items, and its
        // registrations are a fifth section.
        let (case_items, regs) = match self.tests[m].get() {
            Some((i, r)) => (i.as_slice(), r.as_slice()),
            None => (&[][..], &[][..]),
        };
        let mut all_items: Vec<Item> = prep.items.iter().chain(case_items).cloned().collect();
        add_derived_methods(&names, &mut all_items, derived);
        // An omitted result type or row is the inferred one in the module's
        // items, which `Collect` and the program's emission read: a body's
        // code key and its callers' hold its row's keys (codegen.md §12.4).
        let inferred: HashMap<DefId, hd_check::results::Inferred> =
            inferred.iter().map(|i| (i.def, *i)).collect();
        for it in &mut all_items {
            if let Some(inf) = inferred.get(&it.def)
                && let ItemData::Fn(sig) | ItemData::Method { sig, .. } = &mut it.data
            {
                if let Some(ret) = inf.ret {
                    sig.ret = ret;
                }
                if let Some(row) = inf.row {
                    sig.row = row;
                }
            }
        }
        let mut rw = Writer::default();
        encode_regs(regs, &mut rw, &lines);
        let items = match hd_resolve::encode_items(&names, &all_items, &[]) {
            Ok(x) => x,
            Err(e) => {
                self.stage::<()>(Stage::ModuleFinish, Err(e));
                let _ = self.check[m].set(None);
                return;
            }
        };
        let Some(key) = self.check_key(m) else { return };
        // The references to `dbg`, whichever the profile: a release build
        // reads them (`module.dbg.release`), so they stay out of the TIR
        // content hash and the entry holds one answer for every profile.
        let mut gw = Writer::default();
        gw.len_of(dbg);
        for r in dbg {
            lines.write(&mut gw, r.span.lo);
            lines.write(&mut gw, r.span.hi);
            gw.u8(u8::from(r.call));
        }
        let mut sections: Vec<&[u8]> = vec![
            &dw.bytes,
            &meta.bytes,
            &items,
            &tw.bytes,
            &rw.bytes,
            &summary,
            &gw.bytes,
        ];
        // A REPL input's interface: its top-level bindings, which later
        // inputs read and assign (`cli.repl.declarations.bindings`). A
        // module with errors joins no session, so it binds nothing.
        let input_iface;
        if self.repl_number(m).is_some() {
            let keep: &[InputBinding] = if diags.has_errors() { &[] } else { bindings };
            input_iface = match encode_bindings(keep, &names) {
                Ok(x) => x,
                Err(e) => {
                    self.stage::<()>(Stage::ModuleFinish, Err(e));
                    let _ = self.check[m].set(None);
                    return;
                }
            };
            sections.push(&input_iface);
        }
        self.put(EntryKind::Check, key, &sections);
        lock(&self.counters)
            .modules_checked
            .push(self.table.modules[m].path.clone());
        lock(&self.report).ok(Stage::ModuleFinish);
        let owned: Vec<Vec<u8>> = sections.iter().map(|s| s.to_vec()).collect();
        self.read_check(m, &owned, Arc::from(join_sections(&owned)));
    }

    /// `module.dbg.release`: a release build rejects each reference to
    /// `dbg` in the user's own code, with the call's statement deleted by
    /// the fix-it. The references are the same in every profile, so this
    /// reads them from the `check` entries here, where the profile is known,
    /// and no entry depends on it. Only a program goal builds in release
    /// (`Run::build_profile`); a test run keeps the test profile.
    fn dbg_in_release(&self) {
        if self.build_profile != BuildProfile::Release {
            return;
        }
        let std = self.table.packages.len() - 1;
        for (m, module) in self.table.modules.iter().enumerate() {
            let package = usize::from(module.package);
            if package == std || self.fetched.get(package).copied().unwrap_or(false) {
                continue;
            }
            let Some(refs) = self.dbg_refs[m].get() else {
                continue;
            };
            for r in refs {
                let what = if r.call { "call" } else { "use" };
                let msg =
                    format!("`dbg` is for debug builds; a release build rejects this {what} of it");
                let mut diags = lock(&self.diags);
                diags.error(Code::DbgInRelease, r.span, &msg);
                if r.call
                    && let Some(del) = self.statement_deletion(m, r.span)
                {
                    diags.add_fix(
                        "delete the `dbg` statement",
                        hd_diag::FixSafety::Suggestion,
                        &[(del, "")],
                    );
                }
            }
        }
    }

    /// The span to delete to remove the statement that is the call at
    /// `call`: the whole line when the statement is alone on it. `None` when
    /// the call is no statement of its own, so deleting it would change types.
    fn statement_deletion(&self, m: usize, call: Span) -> Option<Span> {
        let src = self.src(m);
        let stmt = src.root().descendants().find(|n| {
            n.kind() == hd_syntax::SyntaxKind::ExprStmt
                && n.children().next().is_some_and(|c| src.span(c) == call)
        })?;
        let span = src.span(stmt);
        let text = self.texts[m].as_bytes();
        let (mut lo, mut hi) = (span.lo as usize, span.hi as usize);
        let line = text[..lo]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let alone_before = text[line..lo].iter().all(|b| matches!(b, b' ' | b'\t'));
        let rest = text[hi..]
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(text.len() - hi);
        let alone_after = text[hi..hi + rest]
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\r'));
        if alone_before && alone_after {
            lo = line;
            hi = (hi + rest + 1).min(text.len());
        }
        Some(Span {
            file: span.file,
            lo: u32_of(lo),
            hi: u32_of(hi),
        })
    }

    /// Reads diagnostics and the meta section; the rest waits for `Collect`.
    fn read_check(&self, m: usize, sections: &[Vec<u8>], entry: Arc<[u8]>) {
        let (Some(d), Some(meta)) = (sections.first(), sections.get(1)) else {
            let _ = self.check[m].set(None);
            return;
        };
        let lines = self.lines(m);
        let mut r = Reader::new(d);
        let mut buf = DiagBuf::default();
        for _ in 0..r.count() {
            let code = Code::from_name(r.str()).unwrap_or(Code::Unsupported);
            let severity = if r.u8() == 0 {
                Severity::Error
            } else {
                Severity::Warning
            };
            let (lo, hi) = (lines.read(&mut r), lines.read(&mut r));
            let msg = r.str().to_owned();
            buf.push(
                code,
                severity,
                Span {
                    file: FileId::from_raw(u32_of(m)),
                    lo,
                    hi,
                },
                &msg,
                None,
            );
        }
        let has_errors = buf.has_errors();
        lock(&self.diags).append(&buf);
        let regs = sections
            .get(4)
            .map(|b| decode_regs(&mut Reader::new(b), &lines))
            .unwrap_or_default();
        let _ = self.regs[m].set(regs);
        let mut dbg = Vec::new();
        if let Some(b) = sections.get(6) {
            let mut r = Reader::new(b);
            for _ in 0..r.count() {
                let (lo, hi) = (lines.read(&mut r), lines.read(&mut r));
                let call = r.u8() != 0;
                let file = FileId::from_raw(u32_of(m));
                dbg.push(hd_check::DbgRef {
                    span: Span { file, lo, hi },
                    call,
                });
            }
        }
        let _ = self.dbg_refs[m].set(dbg);
        if let Some(b) = sections.get(7) {
            let bindings = decode_bindings(b, &self.names()).unwrap_or_default();
            let _ = self.input_bindings[m].set(bindings);
        }
        let content = Reader::new(meta).hash();
        let _ = self.check[m].set(Some(CheckOut {
            entry,
            content,
            has_errors,
        }));
    }

    fn package_result(&self, sp: &dyn Spawn) {
        lock(&self.report).ok(Stage::PackageResult);
        // `InitOrder(F)` reads the init summaries of the folder's modules,
        // so it runs once they are checked.
        let orders: Vec<TaskId> = (0..self.table.folders.len())
            .map(|f| sp.add(TaskKind::InitOrder(u32_of(f)), &[]))
            .collect();
        self.dbg_in_release();
        match &self.goal {
            Goal::Program { .. }
            | Goal::ReplInput { .. }
            | Goal::Tests { .. }
            | Goal::Scenario { .. } => {
                if !lock(&self.diags).has_errors() {
                    sp.add(TaskKind::Ext(ExtTask::Collect), &orders);
                }
            }
            Goal::Analyze | Goal::CheckTests => {
                lock(&self.report).not_implemented(
                    Stage::Collect,
                    "no program root: a library package needs a test plan (§13.1)",
                );
                for s in [Stage::Emit, Stage::Link, Stage::Precompile, Stage::Run] {
                    self.blocked(s);
                }
            }
        }
    }

    /// Decodes every module's items and TIR (a `prog_key` miss).
    fn program(&self) -> Option<&ProgramTables> {
        if let Some(p) = self.program.get() {
            return Some(p);
        }
        let names = self.names();
        let mut items = HashMap::new();
        let mut bodies = HashMap::new();
        let mut decoded = Vec::new();
        for m in 0..self.table.modules.len() {
            let Some(Some(c)) = self.check[m].get() else {
                if self.table.modules[m].package != 0 {
                    continue;
                }
                return None;
            };
            if c.has_errors {
                return None;
            }
            let sections = split_sections(&c.entry)?;
            for it in hd_resolve::decode_items(&names, sections.get(2)?)?.0 {
                items.insert(it.def, it);
            }
            let mut r = Reader::new(sections.get(3)?);
            for _ in 0..r.count() {
                let bytes = r.blob();
                let b = hd_tir::wire::read_body(bytes, &self.pool, &self.paths, &self.syms)?;
                bodies.insert(b.item, (b, hd_tir::wire::tir_hash(bytes)));
            }
            decoded.push(self.table.modules[m].path.clone());
        }
        for f in 0..self.table.folders.len() {
            if let Some(i) = self.iface_of(FolderId::from_raw(u32_of(f))) {
                for it in &i.items {
                    items.entry(it.def).or_insert_with(|| it.clone());
                }
            }
        }
        let mut impl_rows = HashMap::new();
        let mut members = Vec::new();
        for f in 0..self.table.folders.len() {
            let fid = FolderId::from_raw(u32_of(f));
            let Some(fi) = self.folder_impls(fid) else {
                continue;
            };
            for (row, d) in fi.table.def.iter().enumerate() {
                impl_rows.insert(
                    *d,
                    ImplRef {
                        module: folder_table(fid),
                        row: u32_of(row),
                    },
                );
            }
            if fi.in_universe() {
                members.push((fid, fi));
            }
        }
        // A module cycle's runs that split a module's init: one body per
        // run (`module.init.cycle.step`).
        for cycles in self.init_runs.iter().filter_map(OnceLock::get) {
            for runs in cycles {
                for r in runs {
                    let init =
                        hd_check::default_body_def(&names, names.item(&r.module, "init"), "init");
                    let def = self.run_def(runs, r, &|_| init);
                    if def == init {
                        continue;
                    }
                    let Some((whole, tir)) = bodies.get(&init) else {
                        continue;
                    };
                    let body = hd_check::init::init_segment(whole, def, r.insts.clone());
                    let mut k = hd_base::StableHasher::new("init-run");
                    k.hash(*tir);
                    k.u32(r.start);
                    k.u32(r.end);
                    bodies.insert(def, (body, k.finish()));
                }
            }
        }
        let (universe, extra) = self.universes.intern(&members);
        lock(&self.counters).tir_decoded.extend(decoded);
        let _ = self.program.set(ProgramTables {
            items,
            bodies,
            impl_rows,
            universe,
            extra,
            recursive_enums: OnceLock::new(),
        });
        self.program.get()
    }

    /// Every folder's impls, by folder id: the program's tables.
    fn program_folders(&self) -> Vec<Option<&FolderImpls>> {
        (0..self.table.folders.len())
            .map(|f| self.folder_impls(FolderId::from_raw(u32_of(f))))
            .collect()
    }

    /// The program's view of the impls (trait-solver.md §3.2): owner
    /// lookup over every folder, no module's own table, the program's
    /// universe.
    fn program_view<'a>(
        &'a self,
        p: &'a ProgramTables,
        folders: &'a [Option<&'a FolderImpls>],
        arity: &'a dyn Fn(DefId) -> usize,
    ) -> ImplView<'a> {
        ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: None,
            folders,
            universe: p.universe,
            extra: &p.extra,
            arity,
            hidden: &[],
            plans: Some(self),
        }
    }

    /// `Collect` (codegen.md §11.3): `prog_key`, then instances, then code
    /// keys; hits are looked up here, and only misses become `Emit` tasks.
    /// A program's roots are `main` (with its report), or for a test run
    /// each running case (engines-and-test-runner.md §19.1).
    fn collect(&self, sp: &dyn Spawn) {
        // `InitOrder` may have found an initialization cycle.
        if lock(&self.diags).has_errors() {
            return;
        }
        let names = self.names();
        let (entry_key, roots) = match &self.goal {
            Goal::Program { entry } | Goal::Scenario { entry } => {
                let entry_module = format!("{}.{entry}", hd_project::package_ident(&self.package));
                let root = names.item(&entry_module, "main");
                (entry_module, vec![(root, String::new())])
            }
            // An input is a script: its init is the root.
            Goal::ReplInput { input } => {
                let entry_module =
                    hd_project::module_path(&self.package, &hd_project::repl_file(*input));
                let root =
                    hd_check::default_body_def(&names, names.item(&entry_module, "init"), "init");
                (entry_module, vec![(root, String::new())])
            }
            Goal::Tests { module, filter } => {
                let cases: Vec<(DefId, String)> = self
                    .test_plan()
                    .into_iter()
                    .filter_map(|(m, r)| {
                        let body = r.body.as_ref().filter(|_| r.ignore.is_none())?;
                        let path = &self.table.modules[m].path;
                        Some((names.item(path, body), path.clone()))
                    })
                    .collect();
                if cases.is_empty() {
                    return;
                }
                let key = format!(
                    "test:{}:{}",
                    module.as_deref().unwrap_or("*"),
                    filter.as_deref().unwrap_or("")
                );
                (key, cases)
            }
            Goal::Analyze | Goal::CheckTests => return,
        };
        let mut modules: Vec<(&str, Hash128)> = Vec::new();
        // std's content is in the toolchain key; a dependency's reached
        // modules count as the root's do.
        let std = self.table.packages.len() - 1;
        for (m, module) in self.table.modules.iter().enumerate() {
            let counted = module.package == 0 || usize::from(module.package) != std;
            match self.check[m].get() {
                Some(Some(c)) if counted => modules.push((module.path.as_str(), c.content)),
                _ if module.package == 0 => {
                    self.blocked(Stage::Collect);
                    return;
                }
                _ => {}
            }
        }
        // A scenario links different exports (`hd.cancel`) from a program
        // of the same entry, so its link key names the goal.
        let keyed_entry = match &self.goal {
            Goal::Scenario { .. } => format!("scenario:{entry_key}"),
            _ => entry_key.clone(),
        };
        let pkey = prog_key(
            self.toolchain,
            self.pipeline,
            self.build_profile,
            &keyed_entry,
            &modules,
        );
        // A session module's link depends on the session: it is not cached.
        if self.session.is_none()
            && let Some(sections) = self.lookup(EntryKind::Link, pkey)
            && let Some(w) = sections.first()
        {
            lock(&self.report).ok(Stage::Collect);
            let _ = self.wasm.set(w.clone());
            return;
        }
        let Some(p) = self.program() else {
            self.stage::<()>(
                Stage::Collect,
                Err(NotImplemented::new(
                    Stage::Collect,
                    "a module's check entry did not decode",
                )),
            );
            return;
        };
        let init_of = |m: &str| hd_check::default_body_def(&names, names.item(m, "init"), "init");
        // A script has no `main`: its top-level statements are its module
        // initialization, and that is the whole entry behavior.
        let (main, _) = roots[0];
        // `module.entry.private-main`: only a `pub` main is the entry point.
        let public = self
            .table
            .modules
            .iter()
            .position(|module| module.path == entry_key)
            .is_none_or(|m| self.skim_of(m).public_main.is_some());
        let has_main = p.bodies.contains_key(&main)
            && (public || !matches!(self.goal, Goal::Program { .. } | Goal::Scenario { .. }));
        let script = match self.goal {
            Goal::Program { .. } | Goal::Scenario { .. } => !has_main,
            Goal::ReplInput { .. } => true,
            Goal::Tests { .. } | Goal::Analyze | Goal::CheckTests => false,
        };
        let root = if script { init_of(&entry_key) } else { main };
        if !p.bodies.contains_key(&root) {
            let span = Span {
                file: FileId::from_raw(u32::MAX),
                lo: 0,
                hi: 0,
            };
            lock(&self.diags).error(
                Code::MissingEntryPoint,
                span,
                &format!("`{entry_key}` has no `fn main` or top-level statements"),
            );
            return;
        }
        let folders = self.program_folders();
        let arity = p.arity();
        let view = self.program_view(p, &folders, &arity);
        let env = Env {
            run: self,
            p,
            impls: Impls::Owned(&view),
            sealed: self.known.sealed(),
        };
        let has_init = |m: &str| p.bodies.contains_key(&init_of(m));
        // The init groups each root module reaches, in order, and their union.
        let mut root_modules: Vec<String> = Vec::new();
        for (_, m) in &roots {
            let m = if m.is_empty() {
                entry_key.clone()
            } else {
                m.clone()
            };
            if !root_modules.contains(&m) {
                root_modules.push(m);
            }
        }
        let mut per_module: Vec<Vec<DefId>> = Vec::new();
        let mut all_inits: Vec<DefId> = Vec::new();
        for m in &root_modules {
            let defs = match self.group_init_order(m, &init_of, &has_init) {
                Ok(o) => o,
                Err(e) => {
                    self.stage::<()>(Stage::Collect, Err(e));
                    return;
                }
            };
            for d in &defs {
                if !all_inits.contains(d) {
                    all_inits.push(*d);
                }
            }
            per_module.push(defs);
        }
        // The entry module's init is one body: a script polls it last with
        // its entry providers, and only it may `block_on`.
        if matches!(self.goal, Goal::Program { .. } | Goal::ReplInput { .. })
            && has_init(&entry_key)
        {
            let whole = init_of(&entry_key);
            if !all_inits.contains(&whole) || (script && all_inits.last() != Some(&whole)) {
                self.stage::<()>(
                    Stage::Collect,
                    Err(NotImplemented::new(
                        Stage::Collect,
                        "an entry module whose statements run among its module cycle's",
                    )),
                );
                return;
            }
        }
        // Extra roots: the other cases, then each distinct report function.
        let mut extra: Vec<(DefId, TyList)> = roots[1..]
            .iter()
            .map(|(d, _)| (*d, TyList::EMPTY))
            .collect();
        // A scenario's cleanup function is an extra root, so its instance
        // is collected even though no case calls it.
        if matches!(self.goal, Goal::Scenario { .. }) {
            let def = names.item(&entry_key, "cleanup_ran");
            if p.bodies.contains_key(&def) {
                extra.push((def, TyList::EMPTY));
            }
        }
        let reports: Vec<Option<(DefId, TyList)>> = roots
            .iter()
            .map(|(d, _)| {
                // A script has no `main`, so there is no result to report.
                if script {
                    return None;
                }
                self.report_fn(p, env.ret(*d).unwrap_or(Ty::VOID))
            })
            .collect();
        for r in reports.iter().flatten() {
            if !extra.contains(r) {
                extra.push(*r);
            }
        }
        let prelude = if self.prelude() {
            self.prelude_roots()
        } else {
            PreludeRoots::default()
        };
        extra.extend(prelude.calls);
        let known = |k: Hash128| {
            self.session
                .is_some_and(|s| s.externs.funcs.contains_key(&hd_wasm::Sym::Inst(k)))
        };
        let inits_run = |k: Hash128| self.session.is_some_and(|s| s.inits_run.contains(&k));
        let session = self.session.map(|_| hd_mono::Session {
            known: &known,
            inits_run: &inits_run,
        });
        let Some(collected) = self.stage(
            Stage::Collect,
            hd_mono::collect(
                &self.pool,
                &env,
                &TableSolver,
                root,
                &all_inits,
                &extra,
                &prelude.refs,
                session,
            ),
        ) else {
            return;
        };
        let collected = match collected {
            Ok(c) => c,
            Err(deep) => {
                self.too_deep(&deep);
                return;
            }
        };
        let key_of_extra = |r: &(DefId, TyList)| {
            extra
                .iter()
                .position(|x| x == r)
                .and_then(|i| collected.extra.get(i).copied())
        };
        // An init's instance, when this module runs it: a session leaves
        // out the inits earlier modules ran.
        let key_of_init = |d: &DefId| {
            collected
                .table
                .get(*d, hd_mono::Sub::Body(0), TyList::EMPTY)
                .map(|id| collected.table.key[id.idx()])
                .filter(|k| collected.inits.contains(k))
        };
        let plan = match &self.goal {
            Goal::Program { .. } | Goal::ReplInput { .. } | Goal::Scenario { .. } if script => {
                Roots::Script {
                    def: root,
                    key: collected.table.key[0],
                }
            }
            Goal::Program { .. } => Roots::Main {
                def: root,
                key: collected.table.key[0],
                entry_init: key_of_init(&init_of(&entry_key)),
                report: reports[0].as_ref().and_then(key_of_extra),
            },
            Goal::Scenario { .. } => Roots::Scenario {
                def: root,
                key: collected.table.key[0],
                entry_init: key_of_init(&init_of(&entry_key)),
                report: reports[0].as_ref().and_then(key_of_extra),
                cleanup: {
                    let def = names.item(&entry_key, "cleanup_ran");
                    if p.bodies.contains_key(&def) {
                        key_of_extra(&(def, TyList::EMPTY))
                    } else {
                        None
                    }
                },
            },
            _ => Roots::Tests {
                cases: roots
                    .iter()
                    .enumerate()
                    .map(|(i, (d, _))| {
                        let key = if i == 0 {
                            collected.table.key[0]
                        } else {
                            key_of_extra(&(*d, TyList::EMPTY)).unwrap_or(Hash128(0))
                        };
                        (*d, key, reports[i].as_ref().and_then(key_of_extra))
                    })
                    .collect(),
                inits: per_module
                    .iter()
                    .map(|ds| ds.iter().filter_map(key_of_init).collect())
                    .collect(),
            },
        };
        let order = collected.table.content_order();
        // Emitted code embeds the layouts of the data types it touches, and
        // instance keys name types by path only: the program's data layouts
        // are part of every code key, so two programs that declare one path
        // with different fields never share code.
        let mut layouts = hd_base::StableHasher::new("data-layouts");
        let mut canons = hd_mono::layout::CanonMemo::default();
        let ph = |x: DefId| env.path_hash(x);
        for d in &collected.data {
            layouts.hash(d.0);
            let fields = env.data_fields(d.def()).unwrap_or_default();
            layouts.u32(u32::try_from(fields.len()).unwrap_or(u32::MAX));
            for f in fields {
                layouts.hash(hd_mono::layout::canon(&self.pool, &ph, &mut canons, f));
            }
        }
        let layouts = layouts.finish();
        let mut code_keys = Vec::new();
        let codes: Vec<OnceLock<WasmCode>> = order.iter().map(|_| OnceLock::new()).collect();
        let mut misses = Vec::new();
        for (slot, id) in order.iter().enumerate() {
            let item = collected.table.item[id.idx()];
            let tir = p.bodies.get(&item).map_or(Hash128(0), |b| b.1);
            let mut reps = hd_base::StableHasher::new("code-deps");
            reps.hash(self.toolchain);
            reps.hash(layouts);
            reps.hash(collected.callee_reps[id.idx()]);
            let ck = code_key(
                self.pipeline,
                self.build_profile,
                collected.table.key[id.idx()],
                tir,
                reps.finish(),
            );
            code_keys.push(ck);
            match self
                .lookup(EntryKind::Code, ck)
                .and_then(|s| s.first().and_then(|b| WasmCode::decode(b)))
            {
                Some(c) => {
                    let _ = codes[slot].set(c);
                }
                None => misses.push(slot),
            }
        }
        let _ = self.collect.set(CollectOut {
            collected,
            order,
            codes,
            code_keys,
            prog_key: pkey,
            roots: plan,
            layouts: hd_wasm::layout::Layouts::default(),
        });
        let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
        for slot in misses {
            let e = sp.add(TaskKind::Ext(ExtTask::Emit(u32_of(slot))), &[]);
            sp.edge(e, link);
        }
        sp.release(link);
    }

    /// The `check` role of module `m`: a test run, and `hd check --tests`,
    /// check the package's modules with their `tests:` blocks, into
    /// separate entries.
    fn role(&self, m: usize) -> &'static str {
        if matches!(self.goal, Goal::Tests { .. } | Goal::CheckTests)
            && self.table.modules[m].package == 0
        {
            "test"
        } else {
            ROLE
        }
    }

    /// The keys the runner binds for a test case of module `m`: the
    /// test profile's `TestRunner` for every case
    /// (`module.testing.runner-provider`), which is all a unit test case
    /// gets (`module.testing.unit-row.test-runner`); an integration test
    /// case also gets the host capability traits of the default profile
    /// (`module.testing.integration-row`, `module.profile.default`), as the
    /// host ABI table lists them. The kind decides (`module.testing.kind-decides`).
    fn test_profile(&self, m: usize) -> hd_types::RowId {
        let module = &self.table.modules[m];
        let integration = module.role == Role::Test
            && self
                .table
                .sources
                .get(module.file.idx())
                .is_some_and(|(_, file)| hd_project::Root::of(file).0 == hd_project::Root::Test);
        let mut paths = vec!["std.testing.TestRunner"];
        if integration {
            paths.extend(hd_host_abi::TABLE.iter().map(|t| t.std_path));
        }
        let names = self.names();
        let keys = paths
            .iter()
            .filter_map(|p| {
                let (owner, name) = p.rsplit_once('.')?;
                Some(self.pool.intern_ty(&TyData::TraitValue {
                    def: names.item(owner, name),
                    args: TyList::EMPTY,
                    bindings: vec![],
                }))
            })
            .collect();
        self.pool.row(&hd_types::RowData {
            keys,
            params: vec![],
        })
    }

    /// The selected registrations of a test run, in content order: module
    /// order, then registration order (§19.2). `--filter` keeps the names
    /// that contain it (`cli.test.filter`), and an `it_each` call whose row
    /// names `name[i]` may contain it.
    fn test_plan(&self) -> Vec<(usize, hd_check::tests::TestReg)> {
        let Goal::Tests { module, filter } = &self.goal else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (m, md) in self.table.modules.iter().enumerate() {
            if md.package != 0 || module.as_ref().is_some_and(|x| *x != md.path) {
                continue;
            }
            for r in self.regs[m].get().into_iter().flatten() {
                // A row name `name[i]` selects its `it_each` case, whose
                // rows the runner selects by name.
                let row_of = |f: &str| {
                    r.kind == "it_each"
                        && f.rsplit_once('[').is_some_and(|(n, _)| r.name.ends_with(n))
                };
                if filter
                    .as_ref()
                    .is_none_or(|f| r.name.contains(f.as_str()) || row_of(f))
                {
                    out.push((m, r.clone()));
                }
            }
        }
        out
    }

    /// Each body of the test plan, by its stable item path, with its
    /// case's registered name: what a panic frame in the body names.
    fn test_bodies(&self) -> Vec<(String, String)> {
        let names = self.names();
        self.test_plan()
            .into_iter()
            .filter_map(|(m, r)| {
                let body = r.body.as_ref()?;
                let item = names.item(&self.table.modules[m].path, body);
                Some((names.path(item), r.name))
            })
            .collect()
    }

    /// The test plan as `Output::tests` lists it: running cases get their
    /// `hd.test.i` and `hd.init.j` exports, in the order `collect` roots them.
    fn test_cases(&self) -> Vec<TestCase> {
        let mut modules: Vec<usize> = Vec::new();
        let mut next = 0u32;
        let mut out = Vec::new();
        for (m, r) in self.test_plan() {
            let run = if r.body.is_some() && r.ignore.is_none() {
                let j = if let Some(j) = modules.iter().position(|x| *x == m) {
                    j
                } else {
                    modules.push(m);
                    modules.len() - 1
                };
                next += 1;
                Some((next - 1, u32_of(j)))
            } else {
                None
            };
            let text = &self.texts[m];
            let upto = text.get(..r.at as usize).unwrap_or("");
            out.push(TestCase {
                module: self.table.modules[m].path.clone(),
                file: self.table.files.get(m).cloned().unwrap_or_default(),
                line: u32_of(upto.matches('\n').count() + 1),
                name: r.name,
                kind: r.kind,
                ignore: r.ignore,
                expect_panic: r.expect_panic,
                unsupported: r.unsupported,
                run,
            });
        }
        out
    }

    /// The `std.rt` function that turns a root's result into its exit
    /// status (module.entry.exit-report), or `None` for `void`: the
    /// `Error` form for an error type that implements `Error` (its cause
    /// chain is printed), the `Display` form otherwise.
    fn report_fn(&self, p: &ProgramTables, ret: Ty) -> Option<(DefId, TyList)> {
        let pool = &self.pool;
        let names = self.names();
        if ret == Ty::VOID || ret == Ty::NEVER {
            return None;
        }
        let rt = |n: &str| names.item("std.rt", n);
        let known = &self.known;
        let TyData::Adt { def, args } = pool.get(ret) else {
            return Some((rt("entry_status"), pool.list(&[ret])));
        };
        if def != known.result {
            return Some((rt("entry_status"), pool.list(&[ret])));
        }
        let a = pool.list_items(args);
        let (ok, err) = (a[0], a[1]);
        let error = known.error;
        match pool.get(err) {
            TyData::TraitValue { def, .. } if def == error => {
                Some((rt("entry_status_dyn"), pool.list(&[ok])))
            }
            TyData::Adt { def: ed, .. }
                if p.items.values().any(|i| {
                    matches!(&i.data, ItemData::Impl { trait_, self_ty, .. }
                        if *trait_ == error
                            && matches!(pool.get(*self_ty), TyData::Adt { def, .. } if def == ed))
                }) =>
            {
                Some((rt("entry_status_error"), args))
            }
            _ => Some((rt("entry_status_display"), args)),
        }
    }

    /// The init bodies the entry module reaches, in initialization order
    /// (spec/lang/10-modules.md, "Initialization Order"): each module
    /// cycle (a strongly connected component of the use graph) after the
    /// cycles it uses (`module.init.cycle.once`); among ready cycles, the
    /// one with the least module identity first
    /// (`module.init.cycle.ready-order`). Inside a cycle whose statements
    /// sit in several modules, `InitOrder`'s runs: a module's whole init
    /// when its statements run together, else one body per run.
    fn group_init_order(
        &self,
        entry: &str,
        init_of: &dyn Fn(&str) -> DefId,
        has_init: &dyn Fn(&str) -> bool,
    ) -> hd_base::StageResult<Vec<DefId>> {
        let Some(start) = self.table.module(entry) else {
            return Ok(Vec::new());
        };
        let n = self.table.modules.len();
        let edges: Vec<Vec<usize>> = (0..n).map(|m| self.use_edges(m)).collect();
        let (groups, comp) = strongly_connected(&edges, &[start.idx()]);
        // Kahn's order over the groups: dependencies first, ties by the
        // least module identity.
        let mut deps: Vec<std::collections::BTreeSet<usize>> =
            vec![std::collections::BTreeSet::new(); groups.len()];
        for (gi, g) in groups.iter().enumerate() {
            for &m in g {
                for &w in &edges[m] {
                    if comp[w] != usize::MAX && comp[w] != gi {
                        deps[gi].insert(comp[w]);
                    }
                }
            }
        }
        let name = |g: usize| {
            groups[g]
                .iter()
                .map(|m| self.table.modules[*m].path.as_str())
                .min()
                .unwrap_or("")
        };
        let mut done = vec![false; groups.len()];
        let mut out = Vec::new();
        for _ in 0..groups.len() {
            let Some(next) = (0..groups.len())
                .filter(|g| !done[*g] && deps[*g].iter().all(|d| done[*d]))
                .min_by_key(|g| name(*g))
            else {
                break;
            };
            done[next] = true;
            let mut paths: Vec<&str> = groups[next]
                .iter()
                .map(|m| self.table.modules[*m].path.as_str())
                .collect();
            paths.sort_unstable();
            if paths.iter().filter(|p| has_init(p)).count() < 2 {
                out.extend(paths.iter().filter(|p| has_init(p)).map(|p| init_of(p)));
                continue;
            }
            let folder = self.table.modules[groups[next][0]].folder;
            let runs = self.init_runs[folder.idx()]
                .get()
                .and_then(|cycles| {
                    cycles.iter().find(|c| {
                        c.first()
                            .is_some_and(|r| paths.contains(&r.module.as_str()))
                    })
                })
                .ok_or_else(|| {
                    NotImplemented::new(Stage::Collect, "a module cycle without its init order")
                })?;
            out.extend(runs.iter().map(|r| self.run_def(runs, r, init_of)));
        }
        Ok(out)
    }

    /// The body that runs `r`, one of a cycle's `runs`: its module's whole
    /// init when it is the module's one run, else the run's own body.
    fn run_def(&self, runs: &[InitRun], r: &InitRun, init_of: &dyn Fn(&str) -> DefId) -> DefId {
        let init = init_of(&r.module);
        if runs.iter().filter(|x| x.module == r.module).count() == 1 {
            return init;
        }
        self.names().member(
            init,
            hd_intern::PathKind::Hidden,
            &format!("run{}", r.start),
        )
    }

    fn emit(&self, slot: usize) {
        let (Some(c), Some(p)) = (self.collect.get(), self.program.get()) else {
            return;
        };
        let id = c.order[slot];
        let item = c.collected.table.item[id.idx()];
        let sub = c.collected.table.sub[id.idx()];
        let args = c.collected.table.args[id.idx()];
        let folders = self.program_folders();
        let arity = p.arity();
        let view = self.program_view(p, &folders, &arity);
        let env = Env {
            run: self,
            p,
            impls: Impls::Owned(&view),
            sealed: self.known.sealed(),
        };
        let names = self.names();
        let ret = env.ret(item).unwrap_or(Ty::VOID);
        let path = |d: DefId| names.path(d);
        let calls = &c.collected.calls[id.idx()];
        let r = match sub {
            hd_mono::Sub::Body(k) => {
                let body = p.bodies.get(&item).map(|b| &b.0);
                match c.collected.supplied_body(&self.pool, item, args).or(body) {
                    Some(body) => hd_wasm::emit(
                        &self.pool,
                        &env,
                        &path,
                        &c.layouts,
                        body,
                        k,
                        args,
                        ret,
                        calls,
                        c.collected.table.key[id.idx()],
                    ),
                    None => Err(NotImplemented::new(Stage::Emit, "an instance without TIR")),
                }
            }
            // A function reference's adapter has no TIR (codegen.md §13.11).
            hd_mono::Sub::Adapter => {
                hd_wasm::emit_adapter(&self.pool, &env, &path, &c.layouts, item, args, calls)
            }
            // A generic method's erased body and its thunks (§13.5.1).
            hd_mono::Sub::Erased | hd_mono::Sub::Thunk(_) => match p.bodies.get(&item) {
                Some((body, _)) if sub == hd_mono::Sub::Erased => hd_wasm::emit_erased(
                    &self.pool,
                    &env,
                    &path,
                    &c.layouts,
                    body,
                    args,
                    ret,
                    calls,
                    c.collected.table.key[id.idx()],
                ),
                Some((body, _)) => hd_wasm::emit_thunk(
                    &self.pool,
                    &env,
                    &path,
                    &c.layouts,
                    body,
                    args,
                    calls,
                    c.collected.table.key[id.idx()],
                ),
                None => Err(NotImplemented::new(
                    Stage::Emit,
                    "an erased body without TIR",
                )),
            },
        };
        // The instance's item names where emission stopped.
        let r = r.map_err(|mut e| {
            e.what = format!("{} (in `{}`)", e.what, names.path(item));
            e
        });
        let Some(code) = self.stage(Stage::Emit, r) else {
            return;
        };
        self.put(EntryKind::Code, c.code_keys[slot], &[&code.encode()]);
        lock(&self.counters).emitted += 1;
        let _ = c.codes[slot].set(code);
    }

    fn link(&self) {
        let (Some(c), Some(p)) = (self.collect.get(), self.program.get()) else {
            return;
        };
        let mut codes = Vec::new();
        let mut fnames = Vec::new();
        for (slot, id) in c.order.iter().enumerate() {
            let Some(code) = c.codes[slot].get() else {
                return;
            };
            codes.push((c.collected.table.key[id.idx()], code.clone()));
            fnames.push(format!(
                "{}#{}",
                self.names().path(c.collected.table.item[id.idx()]),
                c.collected.table.sub[id.idx()]
            ));
        }
        let folders = self.program_folders();
        let arity = p.arity();
        let view = self.program_view(p, &folders, &arity);
        let env = Env {
            run: self,
            p,
            impls: Impls::Owned(&view),
            sealed: self.known.sealed(),
        };
        let names = self.names();
        let path = |d: DefId| names.path(d);
        let exports = match &c.roots {
            Roots::Main {
                def,
                key,
                entry_init,
                report,
            } => hd_wasm::entry(
                &self.pool,
                &env,
                &path,
                *def,
                *key,
                &c.collected.inits,
                *entry_init,
                *report,
            ),
            Roots::Script { def, key } => {
                hd_wasm::script_entry(&self.pool, &env, &path, *def, *key, &c.collected.inits)
            }
            Roots::Tests { cases, inits } => {
                hd_wasm::test_entry(&self.pool, &env, &path, cases, inits)
            }
            Roots::Scenario {
                def,
                key,
                entry_init,
                report,
                cleanup,
            } => hd_wasm::scenario_entry(
                &self.pool,
                &env,
                &path,
                *def,
                *key,
                &c.collected.inits,
                *entry_init,
                *report,
                *cleanup,
            ),
        };
        let Some(entry) = self.stage(Stage::Link, exports) else {
            return;
        };
        let mode = match self.session {
            Some(s) => hd_wasm::LinkMode::Session(&s.externs),
            None => hd_wasm::LinkMode::Program,
        };
        let Some(wasm) = self.stage(
            Stage::Link,
            hd_wasm::link(
                &codes,
                &fnames,
                &entry,
                &c.collected.witnesses,
                self.build_profile == BuildProfile::Release,
                mode,
            ),
        ) else {
            return;
        };
        match self.session {
            Some(_) => {
                let mut added = Session::default();
                added.externs.add_module(&codes);
                added.inits_run.extend(c.collected.inits.iter().copied());
                if self.prelude() {
                    let key = prelude_key(self.toolchain, self.pipeline, self.build_profile);
                    self.put(EntryKind::Link, key, &[&wasm, &added.encode()]);
                }
                let _ = self.session_added.set(added);
            }
            None => self.put(EntryKind::Link, c.prog_key, &[&wasm]),
        }
        let _ = self.wasm.set(wasm);
    }
}

impl TuplePlans for Run<'_> {
    fn tuple_plan(&self, template: DefId) -> Option<Arc<[MemberStep]>> {
        let cell = Arc::clone(lock(&self.tuple_plans).entry(template).or_default());
        cell.get_or_init(|| self.compute_tuple_plan(template))
            .clone()
    }
}

impl hd_resolve::World for Run<'_> {
    fn module_folder(&self, module: &str) -> Option<u32> {
        self.table.folder_of_module(module).map(FolderId::raw)
    }
    fn usable(&self, module: &str) -> bool {
        self.table.usable(module)
    }
    fn test_only(&self, module: &str) -> bool {
        self.table
            .module(module)
            .is_some_and(|m| self.table.modules[m.idx()].role == hd_project::Role::Test)
    }
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>> {
        self.iface_of(FolderId::from_raw(folder))
    }
}

/// A file's code lines (`HeaderSkeleton::code_lines`) and length. A `check`
/// entry stores a byte offset as `(code line, offset from its start)`,
/// which an edit of comments and blank lines keeps, since its key holds
/// the lines' contents; an offset before the first code line stays raw.
struct Lines<'a> {
    starts: &'a [u32],
    len: u32,
}

impl Lines<'_> {
    const RAW: u32 = u32::MAX;

    fn write(&self, w: &mut Writer, at: u32) {
        match self.starts.partition_point(|&s| s <= at) {
            0 => {
                w.u32(Self::RAW);
                w.u32(at);
            }
            k => {
                w.u32(u32_of(k - 1));
                w.u32(at - self.starts[k - 1]);
            }
        }
    }

    /// The offset in this file; one past its code line's content stays
    /// before the next code line.
    fn read(&self, r: &mut Reader<'_>) -> u32 {
        let (line, off) = (r.u32() as usize, r.u32());
        let first = self.starts.first().copied().unwrap_or(self.len);
        let Some(&start) = self.starts.get(line) else {
            return off.min(first);
        };
        let next = self.starts.get(line + 1).copied().unwrap_or(self.len);
        start.saturating_add(off).min(next)
    }
}

/// A module's init summary in its `check` entry: the statement count
/// first, which `InitOrder` peeks, then the path table, then the rows.
/// Each set is written in path order, so the bytes are a function of the
/// summary.
fn encode_summary(
    sum: &hd_check::init::InitSummary,
    names: &Names<'_>,
    lines: &Lines<'_>,
) -> Vec<u8> {
    let mut t = hd_types::wire::TableWriter::new(names.pool, names.paths, names.syms);
    let mut body = Writer::default();
    let set = |t: &mut hd_types::wire::TableWriter<'_>, w: &mut Writer, s: &HashSet<DefId>| {
        let mut v: Vec<DefId> = s.iter().copied().collect();
        v.sort_by_cached_key(|d| names.path(*d));
        w.len_of(&v);
        for d in v {
            w.u32(t.def(d));
        }
    };
    body.len_of(&sum.stmts);
    for st in &sum.stmts {
        body.u32(st.first_inst);
        lines.write(&mut body, st.lo);
        lines.write(&mut body, st.hi);
        set(&mut t, &mut body, &st.facts.reads);
        set(&mut t, &mut body, &st.facts.calls);
    }
    body.len_of(&sum.globals);
    for (d, at) in &sum.globals {
        body.u32(t.def(*d));
        body.u32(*at);
    }
    body.len_of(&sum.fns);
    for (d, f) in &sum.fns {
        body.u32(t.def(*d));
        set(&mut t, &mut body, &f.reads);
        set(&mut t, &mut body, &f.calls);
    }
    let mut w = Writer::default();
    w.len_of(&sum.stmts);
    t.write(&mut w);
    w.bytes.extend_from_slice(&body.bytes);
    w.bytes
}

/// The statement count of an encoded init summary; 0 for none.
fn summary_statements(b: &[u8]) -> usize {
    if b.is_empty() {
        return 0;
    }
    Reader::new(b).count()
}

fn decode_summary(
    b: &[u8],
    names: &Names<'_>,
    lines: &Lines<'_>,
) -> Option<hd_check::init::InitSummary> {
    let mut r = Reader::new(b);
    let _ = r.count();
    let t = hd_types::wire::Tables::read(&mut r, names.pool, names.paths, names.syms)?;
    let set = |r: &mut Reader<'_>| -> Option<HashSet<DefId>> {
        (0..r.count()).map(|_| t.def(r.u32())).collect()
    };
    let mut sum = hd_check::init::InitSummary::default();
    for _ in 0..r.count() {
        let first_inst = r.u32();
        let lo = lines.read(&mut r);
        let hi = lines.read(&mut r);
        let reads = set(&mut r)?;
        let calls = set(&mut r)?;
        sum.stmts.push(hd_check::init::StmtSummary {
            first_inst,
            lo,
            hi,
            facts: hd_check::init::InitFacts { reads, calls },
        });
    }
    for _ in 0..r.count() {
        let d = t.def(r.u32())?;
        sum.globals.push((d, r.u32()));
    }
    for _ in 0..r.count() {
        let d = t.def(r.u32())?;
        let reads = set(&mut r)?;
        let calls = set(&mut r)?;
        sum.fns
            .push((d, hd_check::init::InitFacts { reads, calls }));
    }
    r.ok().then_some(sum)
}

/// A REPL input's top-level bindings in its `check` entry (its input
/// interface): the type tables, then each binding's item, type and `:=`
/// flag, in statement order.
fn encode_bindings(bindings: &[InputBinding], names: &Names<'_>) -> StageResult<Vec<u8>> {
    let mut t = hd_types::wire::TableWriter::new(names.pool, names.paths, names.syms);
    let mut body = Writer::default();
    body.len_of(bindings);
    for b in bindings {
        body.u32(t.def(b.def));
        body.u32(t.ty(b.ty)?);
        body.u8(u8::from(b.short));
    }
    let mut w = Writer::default();
    t.write(&mut w);
    w.bytes.extend_from_slice(&body.bytes);
    Ok(w.bytes)
}

fn decode_bindings(b: &[u8], names: &Names<'_>) -> Option<Vec<InputBinding>> {
    let mut r = Reader::new(b);
    let t = hd_types::wire::Tables::read(&mut r, names.pool, names.paths, names.syms)?;
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let def = t.def(r.u32())?;
        let ty = t.ty(r.u32())?;
        let short = r.u8() != 0;
        out.push(InputBinding { def, ty, short });
    }
    r.ok().then_some(out)
}

/// A module's test registrations, as its test-role `check` entry holds them.
fn encode_regs(regs: &[hd_check::tests::TestReg], w: &mut Writer, lines: &Lines<'_>) {
    let opt = |w: &mut Writer, s: &Option<String>| match s {
        Some(s) => {
            w.u8(1);
            w.str(s);
        }
        None => w.u8(0),
    };
    w.len_of(regs);
    for r in regs {
        w.str(&r.name);
        w.str(&r.kind);
        opt(w, &r.body);
        opt(w, &r.ignore);
        opt(w, &r.expect_panic);
        opt(w, &r.unsupported);
        lines.write(w, r.at);
    }
}

fn decode_regs(r: &mut Reader<'_>, lines: &Lines<'_>) -> Vec<hd_check::tests::TestReg> {
    let opt = |r: &mut Reader<'_>| (r.u8() == 1).then(|| r.str().to_owned());
    let n = r.count();
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(hd_check::tests::TestReg {
            name: r.str().to_owned(),
            kind: r.str().to_owned(),
            body: opt(r),
            ignore: opt(r),
            expect_panic: opt(r),
            unsupported: opt(r),
            at: lines.read(r),
        });
    }
    out
}

/// `InitOrder`'s runs for one folder: one list per module cycle.
fn encode_runs(cycles: &[Vec<InitRun>]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(cycles);
    for runs in cycles {
        w.len_of(runs);
        for r in runs {
            w.str(&r.module);
            w.u32(r.start);
            w.u32(r.end);
            w.u32(r.insts.start);
            w.u32(r.insts.end);
        }
    }
    w.bytes
}

fn decode_runs(b: &[u8]) -> Option<Vec<Vec<InitRun>>> {
    let mut r = Reader::new(b);
    let cycles = (0..r.count())
        .map(|_| {
            (0..r.count())
                .map(|_| InitRun {
                    module: r.str().to_owned(),
                    start: r.u32(),
                    end: r.u32(),
                    insts: r.u32()..r.u32(),
                })
                .collect()
        })
        .collect();
    r.ok().then_some(cycles)
}

/// Tarjan's strongly connected components of the graph `edges` reaches
/// from `starts`, each as it completes, so a component comes after every
/// component it reaches; and each node's component (`usize::MAX` when
/// unreached).
fn strongly_connected(edges: &[Vec<usize>], starts: &[usize]) -> (Vec<Vec<usize>>, Vec<usize>) {
    let n = edges.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on = vec![false; n];
    let mut stack = Vec::new();
    let mut comp = vec![usize::MAX; n];
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut counter = 0;
    for &s in starts {
        if index[s] != usize::MAX {
            continue;
        }
        // Iterative DFS: (node, next edge).
        let mut work = vec![(s, 0usize)];
        index[s] = counter;
        low[s] = counter;
        counter += 1;
        stack.push(s);
        on[s] = true;
        while let Some(&mut (v, ref mut e)) = work.last_mut() {
            if let Some(&w) = edges[v].get(*e) {
                *e += 1;
                if index[w] == usize::MAX {
                    index[w] = counter;
                    low[w] = counter;
                    counter += 1;
                    stack.push(w);
                    on[w] = true;
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(u, _)) = work.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                let mut g = Vec::new();
                while let Some(w) = stack.pop() {
                    on[w] = false;
                    comp[w] = groups.len();
                    g.push(w);
                    if w == v {
                        break;
                    }
                }
                groups.push(g);
            }
        }
    }
    (groups, comp)
}

/// The overlaps of a coherence run, each reported impl as its folder's
/// index and its item's index in that folder's interface.
fn encode_overlaps(found: &[(DefId, String)], all: &[Arc<FolderIface>]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(found);
    for (item, msg) in found {
        let (f, i) = all
            .iter()
            .enumerate()
            .find_map(|(f, iface)| {
                iface
                    .items
                    .iter()
                    .position(|it| it.def == *item)
                    .map(|i| (f, i))
            })
            .unwrap_or((0, 0));
        w.u32(u32_of(f));
        w.u32(u32_of(i));
        w.str(msg);
    }
    w.bytes
}

fn decode_overlaps(b: &[u8], all: &[Arc<FolderIface>]) -> Option<Vec<(DefId, String)>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let f = r.u32() as usize;
        let i = r.u32() as usize;
        let item = all.get(f)?.items.get(i)?.def;
        out.push((item, r.str().to_owned()));
    }
    Some(out)
}

/// Stage-B findings by item index in the folder's interface.
fn encode_header_findings(found: &[hd_check::header::Finding], items: &[Item]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(found);
    for f in found {
        let at = items.iter().position(|i| i.def == f.item).unwrap_or(0);
        w.u32(u32_of(at));
        w.u32(f.slot);
        w.str(f.code.as_str());
        w.str(&f.message);
    }
    w.bytes
}

fn decode_header_findings(b: &[u8], items: &[Item]) -> Option<Vec<hd_check::header::Finding>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let at = r.u32() as usize;
        let slot = r.u32();
        let code = Code::from_name(r.str())?;
        let message = r.str().to_owned();
        out.push(hd_check::header::Finding {
            item: items.get(at)?.def,
            slot,
            code,
            message,
        });
    }
    r.ok().then_some(out)
}

fn encode_iface_diags(d: &IfaceDiags) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(d);
    for (m, code, severity, a, msg) in d {
        w.u32(u32_of(*m));
        w.str(code.as_str());
        w.u8(u8::from(*severity == Severity::Warning));
        for v in [a.decl, a.member, a.tok, a.count] {
            w.u32(v);
        }
        w.str(msg);
    }
    w.bytes
}

fn decode_iface_diags(b: &[u8]) -> Option<IfaceDiags> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let m = r.u32() as usize;
        let code = Code::from_name(r.str()).unwrap_or(Code::Unsupported);
        let severity = if r.u8() == 0 {
            Severity::Error
        } else {
            Severity::Warning
        };
        let a = hd_resolve::anchor::Anchor {
            decl: r.u32(),
            member: r.u32(),
            tok: r.u32(),
            count: r.u32(),
        };
        out.push((m, code, severity, a, r.str().to_owned()));
    }
    r.ok().then_some(out)
}

/// Sections are kept joined (length-prefixed) as one `Arc`, so a slot
/// holds one allocation per module.
fn join_sections(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(sections);
    for s in sections {
        w.blob(s);
    }
    w.bytes
}

fn split_sections(bytes: &[u8]) -> Option<Vec<&[u8]>> {
    let mut r = Reader::new(bytes);
    let n = r.count();
    let v: Vec<&[u8]> = (0..n).map(|_| r.blob()).collect();
    r.ok().then_some(v)
}

/// The program as collection and emission see it.
struct Env<'r> {
    run: &'r Run<'r>,
    p: &'r ProgramTables,
    impls: Impls<'r>,
    sealed: SealedTraits,
}

impl ProgramTables {
    /// A trait's number of arguments, for owner lookup.
    fn arity(&self) -> impl Fn(DefId) -> usize + '_ {
        |d| self.items.get(&d).map_or(0, |i| i.generics.len())
    }
}

impl LayoutEnv for Env<'_> {
    fn std_kind(&self, def: DefId) -> StdKind {
        let k = &self.run.known;
        if def == k.list {
            StdKind::List
        } else if def == k.map {
            StdKind::Map
        } else if def == k.suspend {
            StdKind::Suspend
        } else {
            StdKind::Other
        }
    }
    fn enum_variants(&self, def: DefId, args: TyList) -> Option<Vec<Vec<Ty>>> {
        match &self.p.items.get(&def)?.data {
            ItemData::Enum { variants, .. } => Some(
                variants
                    .iter()
                    .map(|v| {
                        v.fields
                            .iter()
                            .map(|f| hd_mono::subst(&self.run.pool, self, def, args, f.ty))
                            .collect()
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
    fn enum_declared(&self, def: DefId) -> Option<Vec<Vec<Ty>>> {
        match &self.p.items.get(&def)?.data {
            ItemData::Enum { variants, .. } => Some(
                variants
                    .iter()
                    .map(|v| v.fields.iter().map(|f| f.ty).collect())
                    .collect(),
            ),
            _ => None,
        }
    }
    fn recursive_enum(&self, def: DefId) -> bool {
        self.p
            .recursive_enums
            .get_or_init(|| {
                let mut enums: Vec<DefId> = self
                    .p
                    .items
                    .iter()
                    .filter(|(_, i)| matches!(i.data, ItemData::Enum { .. }))
                    .map(|(d, _)| *d)
                    .collect();
                enums.sort_unstable_by_key(|d| d.raw());
                hd_mono::layout::recursive_enums(&self.run.pool, self, &enums)
            })
            .contains(&def)
    }
}

impl Env<'_> {
    fn sig(&self, def: DefId) -> Option<&hd_resolve::FnSig> {
        self.p.items.get(&def)?.sig()
    }

    /// Whether a method has a `self` receiver; one without is an
    /// associated function.
    fn takes_self(&self, item: &Item) -> bool {
        item.sig()
            .and_then(|s| s.params.first())
            .is_some_and(|(name, _)| self.run.syms.resolve(*name) == "self")
    }

    /// Whether a method's signature names `Self` outside its receiver; such
    /// a method is unavailable on a trait value (trait.dyn.member.self).
    fn names_self(&self, item: &Item, trait_: DefId) -> bool {
        let Some(sig) = item.sig() else { return false };
        let pool = self.run.pool.types();
        let has_receiver = self.takes_self(item);
        sig.params
            .iter()
            .skip(usize::from(has_receiver))
            .map(|p| p.1)
            .chain([sig.ret])
            .chain(sig.generics.iter().flat_map(|g| g.bounds.iter().copied()))
            .any(|t| pool.names_trait_self(t, trait_))
            || pool.row_names_trait_self(sig.row, trait_)
    }

    /// A default body (codegen.md §13.13): a body of its own, whose
    /// parameters are the earlier parameters and whose result is the
    /// parameter's or field's declared type. A facts body is one that takes
    /// no parameter.
    fn default_body(&self, def: DefId) -> Option<&Body> {
        self.p
            .bodies
            .get(&def)
            .map(|b| &b.0)
            .filter(|b| matches!(b.kind, hd_tir::BodyKind::Default | hd_tir::BodyKind::Fact))
    }

    /// Whether a module's `Init` body suspends: a REPL input's top level
    /// is a driver context, so it suspends where it awaits outside its
    /// closures. No other top level awaits.
    fn init_suspends(&self, def: DefId) -> bool {
        let Some((b, _)) = self.p.bodies.get(&def) else {
            return false;
        };
        let awaits = |t: &hd_tir::Tag| {
            matches!(
                t,
                hd_tir::Tag::Await
                    | hd_tir::Tag::AwaitValue
                    | hd_tir::Tag::AwaitAll
                    | hd_tir::Tag::AwaitRace
            )
        };
        if b.kind != hd_tir::BodyKind::Init || !b.tags.iter().any(awaits) {
            return false;
        }
        let subs = b.inst_subs();
        b.tags
            .iter()
            .zip(subs)
            .any(|(t, sub)| awaits(t) && sub == 0)
    }

    /// The row keys of a module init, which has no signature. An entry
    /// module's init has an inferred entry row (`module.init.script-row`),
    /// so its keys are those of the callees its top level calls.
    fn init_keys(&self, def: DefId) -> Vec<Ty> {
        let Some((b, _)) = self.p.bodies.get(&def) else {
            return Vec::new();
        };
        if b.kind != hd_tir::BodyKind::Init {
            return Vec::new();
        }
        let mut keys: Vec<Ty> = Vec::new();
        for (i, tag) in b.tags.iter().enumerate() {
            if !matches!(tag, hd_tir::Tag::Call | hd_tir::Tag::Await) {
                continue;
            }
            let Some(hd_tir::Callee::Item { def: callee, targs }) =
                hd_tir::Callee::from_words(b.record(b.data[i][0]))
            else {
                continue;
            };
            keys.extend(self.row_keys(callee, targs));
        }
        // Content order, as `row_keys` does for a signature's row.
        let names = self.run.names();
        hd_mono::key_order(&self.run.pool, &|d| names.path_hash(d), keys)
    }
}

impl ProgramEnv for Env<'_> {
    fn body(&self, def: DefId) -> Option<&Body> {
        self.p.bodies.get(&def).map(|b| &b.0)
    }
    fn bounded(&self, def: DefId) -> Option<Vec<bool>> {
        self.sig(def)
            .map(|s| s.generics.iter().map(|g| g.bound.is_some()).collect())
    }
    fn generic_rows(&self, def: DefId) -> Vec<bool> {
        self.sig(def)
            .map(|s| s.generics.iter().map(|g| g.row).collect())
            .unwrap_or_default()
    }
    fn ret(&self, def: DefId) -> Option<Ty> {
        if let Some(b) = self.default_body(def) {
            return b.sub_root.first().map(|&r| b.ty[r as usize]);
        }
        self.sig(def).map(|s| s.ret)
    }
    fn params(&self, def: DefId) -> Option<Vec<Ty>> {
        if let Some(b) = self.default_body(def) {
            return Some(
                (0..b.local_ty.len())
                    .filter(|&l| b.local_flags[l] & hd_tir::ir::local_flags::PARAM != 0)
                    .map(|l| b.local_ty[l])
                    .collect(),
            );
        }
        self.sig(def)
            .map(|s| s.params.iter().map(|p| p.1).collect())
    }
    fn suspends(&self, def: DefId) -> bool {
        match self.sig(def) {
            Some(s) => s.suspends,
            None => self.init_suspends(def),
        }
    }
    fn row_keys(&self, def: DefId, args: TyList) -> Vec<Ty> {
        let Some(s) = self.sig(def) else {
            return self.init_keys(def);
        };
        let pool = &self.run.pool;
        let row = match pool.get(hd_mono::subst(
            pool,
            self,
            def,
            args,
            pool.intern_ty(&TyData::Row(s.row)),
        )) {
            TyData::Row(r) => r,
            _ => s.row,
        };
        let names = self.run.names();
        hd_mono::key_order(pool, &|d| names.path_hash(d), pool.row_data(row).keys)
    }
    fn parent(&self, def: DefId) -> Option<(DefId, usize)> {
        let ItemData::Method { owner, .. } = &self.p.items.get(&def)?.data else {
            return None;
        };
        let o = self.p.items.get(owner)?;
        match &o.data {
            // A data type or enum owns its declaration `Structure`'s
            // hidden methods (codegen.md §12.3).
            ItemData::Impl { .. } | ItemData::Data(_) | ItemData::Enum { .. } => {
                Some((*owner, o.generics.len()))
            }
            ItemData::Trait(_) => Some((*owner, 1 + o.generics.len())),
            _ => None,
        }
    }
    fn generics_owner(&self, def: DefId) -> DefId {
        if self.default_body(def).is_some() {
            let p = self
                .run
                .names()
                .paths
                .parent(hd_base::PathId::from_raw(def.raw()));
            return DefId::from_raw(p.raw());
        }
        def
    }
    fn impl_head(&self, impl_: DefId) -> Option<(Ty, TyList, usize)> {
        let it = self.p.items.get(&impl_)?;
        match &it.data {
            ItemData::Impl {
                self_ty,
                trait_args,
                ..
            } => Some((*self_ty, *trait_args, it.generics.len())),
            _ => None,
        }
    }
    fn impl_method(&self, impl_: DefId, method: DefId) -> Option<DefId> {
        let item = self.p.items.get(&method)?;
        let ItemData::Method { .. } = &item.data else {
            return None;
        };
        let name = item.name;
        match &self.p.items.get(&impl_)?.data {
            ItemData::Impl { methods, .. } => {
                methods.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
            }
            _ => None,
        }
    }
    fn trait_methods(&self, trait_: DefId) -> Vec<DefId> {
        match self.p.items.get(&trait_).map(|i| &i.data) {
            // A method the compiler lowers at each call (`Inspectable`'s
            // `downcast`) has no slot, nor has an associated function: it
            // has no receiver to dispatch on, so it is unavailable on a
            // trait value (trait.dyn.member.assoc-function).
            Some(ItemData::Trait(t)) => t
                .methods
                .iter()
                .map(|m| m.1)
                .filter(|m| {
                    self.p.items.get(m).is_none_or(|i| {
                        i.intrinsic.is_none() && self.takes_self(i) && !self.names_self(i, trait_)
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    fn trait_arity(&self, trait_: DefId) -> usize {
        self.p.items.get(&trait_).map_or(0, |i| i.generics.len())
    }
    fn trait_assocs(&self, trait_: DefId) -> Vec<DefId> {
        match self.p.items.get(&trait_).map(|i| &i.data) {
            Some(ItemData::Trait(t)) => t.assoc.iter().map(|a| a.1).collect(),
            _ => Vec::new(),
        }
    }
    fn intrinsic(&self, def: DefId) -> Option<String> {
        let it = self.p.items.get(&def)?;
        if let Some(k) = it.intrinsic {
            return Some(self.run.syms.resolve(k).to_owned());
        }
        (def == self.run.known.block_on).then(|| "block_on".to_owned())
    }
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>> {
        match &self.p.items.get(&def)?.data {
            ItemData::Data(fs) => Some(fs.iter().map(|f| f.ty).collect()),
            // A newtype is a data value of its one inner value.
            ItemData::Newtype(inner) => Some(vec![*inner]),
            _ => None,
        }
    }
    fn has_private_field(&self, def: DefId) -> bool {
        self.p
            .items
            .get(&def)
            .is_some_and(|i| matches!(&i.data, ItemData::Data(fs) if fs.iter().any(|f| !f.public)))
    }
    fn path_hash(&self, def: DefId) -> Hash128 {
        self.run.names().path_hash(def)
    }
    fn type_name(&self, def: DefId) -> String {
        let names = self.run.names();
        let (module, name) = (names.module_of(def), names.display_name(def));
        if hd_resolve::PRELUDE
            .iter()
            .any(|(m, ns)| *m == module && ns.contains(&name))
        {
            return name.to_owned();
        }
        // A package name's `-` is written `_` (trait.typeid.name.package).
        // The name is also the identity a `TypeId` compares, so a package
        // that shares its name with an earlier one keeps its own module
        // root here (`module.package.identity-part`).
        format!("{}.{name}", module.replace('-', "_"))
    }
    fn printed_name(&self, def: DefId) -> String {
        let names = self.run.names();
        let (module, name) = (names.module_of(def), names.display_name(def));
        if hd_resolve::PRELUDE
            .iter()
            .any(|(m, ns)| *m == module && ns.contains(&name))
        {
            return name.to_owned();
        }
        format!("{}.{name}", self.run.printed_module(&module))
    }
    fn describe(&self, def: DefId) -> String {
        self.run.names().path(def)
    }
    fn impls(&self) -> Impls<'_> {
        self.impls
    }
    fn solving(&self) -> (ImplUniverseId, &GlobalMemo) {
        (self.p.universe, &self.run.memo)
    }
    fn impl_row(&self, impl_: DefId) -> Option<ImplRef> {
        self.p.impl_rows.get(&impl_).copied()
    }
    fn decls(&self) -> &dyn Declarations {
        self
    }
    fn supplied_body(&self, method: DefId, self_ty: Ty) -> StageResult<Option<Body>> {
        // A tuple's `Structure` (codegen.md §13.6): the derivation
        // generator over the tuple's member list.
        let names = self.run.names();
        let Some(target) = hd_structure::tuple_target(names.pool, self_ty) else {
            return Ok(None);
        };
        let cx = hd_structure::Cx {
            names: &names,
            items: self.p,
            stage: Stage::Collect,
        };
        hd_structure::supplied(&cx, &target, method)
    }
    fn map_key_items(&self) -> Option<(DefId, DefId, DefId)> {
        // `Eq`'s method names `Self` in its parameter, so it has no vtable
        // slot; the map lowering reads it from the declaration.
        let eq = self.run.known.eq;
        let Some(ItemData::Trait(t)) = self.p.items.get(&eq).map(|i| &i.data) else {
            return None;
        };
        let method = t.methods.first()?.1;
        Some((eq, method, self.run.known.hash_of))
    }
    fn build_profile(&self) -> BuildProfile {
        self.run.build_profile
    }
    fn dbg_body(&self, item: DefId, key: &str, ty: Ty, debug: bool) -> StageResult<Body> {
        // `dbg`'s printing bodies (spec/lang/10-modules.md "Debug Printing"):
        // the generator over the type's declaration.
        let names = self.run.names();
        let cx = hd_structure::Cx {
            names: &names,
            items: self.p,
            stage: Stage::Collect,
        };
        match key {
            "dbg_show" => hd_structure::dbg_show_body(&cx, item, ty, debug),
            _ => hd_structure::dbg_values_body(&cx, item, ty),
        }
    }
    fn debug_trait(&self) -> DefId {
        self.run.known.debug
    }
}

impl hd_structure::Items for ProgramTables {
    fn item(&self, def: DefId) -> Option<&Item> {
        self.items.get(&def)
    }
}

impl Declarations for Env<'_> {
    fn sealed(&self) -> &SealedTraits {
        &self.sealed
    }
    fn type_decl(&self, def: DefId) -> TypeDecl {
        self.p
            .items
            .get(&def)
            .map_or(TypeDecl::Data, Item::type_decl)
    }
    fn supertraits(&self, trait_: DefId) -> &[Ty] {
        self.p.items.get(&trait_).map_or(&[], Item::supertraits)
    }
    fn has_unavailable_member(&self, trait_: DefId) -> bool {
        hd_resolve::declares_assoc_function(self.run.names().syms, |d| self.p.items.get(&d), trait_)
    }
    fn is_local(&self, def: DefId) -> bool {
        self.run.names().is_local(def)
    }
}

/// Diagnostics of a run by stable module order, for tests.
#[must_use]
pub fn messages(o: &Output) -> Vec<String> {
    let mut v: Vec<String> = o
        .diags
        .content_order()
        .into_iter()
        .map(|i| o.diags.get_text(o.diags.message[i]).to_owned())
        .collect();
    v.dedup();
    v
}

/// Joins a module's derived methods to its items: each derived
/// implementation lists its derive instances among its methods; its
/// `Structure`'s methods stay hidden members, which `impl_method` finds
/// for a call of `Structure` (codegen.md §12.3).
fn add_derived_methods(names: &Names<'_>, items: &mut Vec<Item>, derived: &[Item]) {
    for d in derived {
        let ItemData::Method { owner, .. } = d.data else {
            continue;
        };
        if d.def != names.member(owner, hd_intern::PathKind::Member, names.text(d.name)) {
            continue;
        }
        if let Some(ItemData::Impl { methods, .. }) = items
            .iter_mut()
            .find(|i| i.def == owner)
            .map(|i| &mut i.data)
            && !methods.iter().any(|(n, _)| *n == d.name)
        {
            methods.push((d.name, d.def));
        }
    }
    items.extend(derived.iter().cloned());
}

/// The facts bodies of the `Structure` that `opt` sees (annotations,
/// "Facts"), checked in the target's module `cx`: the declaration facts
/// of its data type or enum, edited by the type's block with no trait,
/// built once per type into `decl`; and when the derivation's member
/// `lines` edit them, the edited slots' own bodies. New bodies join
/// `derived`. `None` when a fact is not generated yet.
fn derivation_facts<'t>(
    cx: &BodyCx<'t>,
    heads: &[hd_resolve::Head<'t>],
    (opt, lines): (
        &hd_check::derive::OptIn,
        &[hd_check::derive::MemberLine<'t>],
    ),
    decl: &mut HashMap<DefId, Option<FactBodies>>,
    derived: &mut Derived,
) -> Option<FactBodies> {
    let head = |d: DefId| heads.iter().find(|h| h.def == d).map(|h| h.node);
    let pool = cx.names.pool;
    let data = opt.data;
    let item = cx.lookup.item(data)?;
    let node = head(data)?;
    let traitless = cx
        .lookup
        .own
        .iter()
        .find(|it| {
            matches!(&it.data, ItemData::Impl { trait_, by: Some(_), self_ty, .. }
                if *trait_ == DefId::NONE
                    && matches!(pool.get(*self_ty), TyData::Adt { def, .. } if def == data))
        })
        .and_then(|it| head(it.def));
    let facts = hd_check::structure::declaration_facts(cx, item, node, traitless);
    let base = decl
        .entry(data)
        .or_insert_with(|| {
            let (b, bodies) =
                hd_check::structure::fact_bodies(cx, data, (&facts, node), None).ok()?;
            derived.bodies.extend(bodies);
            Some(b)
        })
        .clone()?;
    if !opt.lines {
        return Some(base);
    }
    let mut view = facts;
    let edited = hd_check::structure::edit_facts(cx, item, &mut view, lines);
    // The derivation's bodies are in its own scope, where the target is
    // the data type or enum at the derivation's arguments.
    if let TyData::Adt { args, .. } = pool.get(opt.target) {
        view.retarget(cx, data, pool.list_items(args));
    }
    let (own, bodies) =
        hd_check::structure::fact_bodies(cx, opt.impl_, (&view, node), Some((&edited, &base)))
            .ok()?;
    derived.bodies.extend(bodies);
    Some(own)
}
