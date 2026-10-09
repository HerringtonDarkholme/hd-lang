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
//! A build stops when a stage it needs answers "not implemented": the
//! answer becomes an internal `unsupported` diagnostic. `analyze_package`
//! runs the same code and only counts such answers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use hd_base::wire::{Reader, Writer};
use hd_base::{
    DefId, FileId, FolderId, Hash128, ModuleId, NotImplemented, Span, Stage, StageResult,
};
use hd_cache::{
    CacheStore, EntryKind, FileApi, MemoryStore, check_key, code_key, hdr_key, iface_key, init_key,
    parse_key, prog_key, toolchain_key,
};
use hd_check::BodyCx;
use hd_check::stages::{ModuleFacts, init_order, test_overlay};
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
    OwnerMap, SealedTraits, TableSolver, TypeDecl, UniverseImpls, folder_table,
};
use hd_types::{InternPool, Ty, TyData, TyList};
use hd_wasm::Code as WasmCode;

pub mod bench;
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
    /// Every stage over a package; "not implemented" is counted.
    Analyze,
    /// `Analyze`, with each module's `tests:` blocks checked as test code
    /// into its test-role `check` entry, as a test run checks them
    /// (`hd check --tests`, `cli.check.tests`).
    CheckTests,
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

/// The result of a run.
pub struct Output {
    pub wasm: Option<Vec<u8>>,
    pub diags: DiagBuf,
    /// Package-relative file paths, indexed by a diagnostic's `FileId`.
    pub files: Vec<String>,
    /// Per file, the files of the modules its `use`s reach (from the
    /// skims this run made; empty for a file it did not skim).
    pub uses: Vec<Vec<usize>>,
    /// Per file, its module's path.
    pub modules: Vec<String>,
    pub counters: Counters,
    pub report: PipelineReport,
    /// Each built folder interface's blob, by folder path.
    pub ifaces: Vec<(String, Arc<[u8]>)>,
    /// Readable TIR of the bodies a `Host::render_tir` named, by item path.
    pub tir_text: std::collections::BTreeMap<String, String>,
    /// A test run's selected cases, in content order.
    pub tests: Vec<TestCase>,
    /// Items in the run's global `InternPool` at the end (a size counter).
    pub pool_items: u32,
    /// The trait solver's memo hits and misses over the run (a counter;
    /// with several threads it varies from run to run).
    pub memo: hd_types::solver::MemoStats,
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
    /// the line start. `sources` supplies the text the positions index.
    #[must_use]
    pub fn render_located(&self, sources: &dyn SourceSet) -> String {
        self.diags.render_compact(&|s: Span| {
            let (file, line, column) = self.locate(sources, s);
            format!("{file}:{line}:{column}")
        })
    }

    /// Text for the user: each internal module path of test or task code
    /// named by its file (`hd_project::user_text`).
    #[must_use]
    pub fn user_text(&self, text: &str) -> String {
        hd_project::user_text(text, &|path| {
            let i = self.modules.iter().position(|m| m == path)?;
            self.files.get(i).cloned()
        })
    }

    /// The package-relative file, 1-based line and column of a span's start.
    #[must_use]
    pub fn locate(&self, sources: &dyn SourceSet, s: Span) -> (String, usize, usize) {
        let file = self.file_name(s);
        let (line, column) = sources
            .read(&file)
            .map_or((1, 1), |text| line_column(&text, s.lo as usize));
        (file, line, column)
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

/// A folder's stage-A diagnostics, with the module each belongs to.
type IfaceDiags = Vec<(usize, Code, hd_resolve::anchor::Anchor, String)>;

/// A module's TIR and body diagnostics after `Body(m)`, the hidden
/// methods its derivations add (checking-and-tir.md §4.13.9), and the
/// results M1 inferred for its private callables (§4.13.1).
type BodyOut = (Vec<Body>, DiagBuf, Vec<Item>, Vec<(DefId, Ty)>);

/// What a module's derivations add to its `check` entry: the derived
/// implementations' methods and their bodies (codegen.md §12.3).
#[derive(Default)]
struct Derived {
    items: Vec<Item>,
    bodies: Vec<Body>,
}

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
}

/// A module's `tests:` blocks after checking: the case items and the
/// registrations.
type TestsOut = (Vec<Item>, Vec<hd_check::tests::TestReg>);

struct Run<'a> {
    host: &'a Host<'a>,
    package: String,
    goal: Goal,
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
    skim: Vec<OnceLock<SkimOut>>,
    parse: Vec<OnceLock<Parse>>,
    graph: OnceLock<GraphOut>,
    iface: Vec<OnceLock<Option<Arc<FolderIface>>>>,
    prep: Vec<OnceLock<Option<PrepOut>>>,
    /// Per module that declares a derivation template: its items and scope,
    /// lowered once per run for the opt-ins that instantiate it.
    template_mods: Vec<OnceLock<Option<ModOut>>>,
    /// Per folder: the modules outside std that declare a derivation
    /// template, from its interface, found on first use.
    folder_templates: Vec<OnceLock<Vec<String>>>,
    body: Vec<OnceLock<Option<BodyOut>>>,
    check: Vec<OnceLock<Option<CheckOut>>>,
    /// Per module, in a test run: the checked `tests:` blocks (a miss).
    tests: Vec<OnceLock<TestsOut>>,
    /// Per module, in a test run: the registrations its check entry holds.
    regs: Vec<OnceLock<Vec<hd_check::tests::TestReg>>>,
    program: OnceLock<ProgramTables>,
    collect: OnceLock<CollectOut>,
    wasm: OnceLock<Vec<u8>>,
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
    /// Its manifest name; its module paths start with its identifier form.
    pub name: String,
    /// Its files, package-relative; only its library is built.
    pub sources: &'a dyn SourceSet,
    /// Its own requirements, as in [`Packages::requires`].
    pub requires: Vec<(String, u16)>,
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
    let std = if package == "std" {
        with_core(host.sources)
    } else {
        std_sources()
    };
    let std_in = PackageIn {
        name: "std",
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
            sources: host.sources,
            scope: Scope::All,
            requires: packages.requires.clone(),
            entries: packages.entries.clone(),
            dev_requires: packages.dev_requires.clone(),
        })
        .chain(packages.deps.iter().map(|d| PackageIn {
            name: &d.name,
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
    let run = Run {
        host,
        package: package.to_owned(),
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
        package_key: hd_cache::package_key(package),
        pipeline,
        skim: (0..n).map(|_| OnceLock::new()).collect(),
        parse: (0..n).map(|_| OnceLock::new()).collect(),
        graph: OnceLock::new(),
        iface: (0..nf).map(|_| OnceLock::new()).collect(),
        prep: (0..n).map(|_| OnceLock::new()).collect(),
        template_mods: (0..n).map(|_| OnceLock::new()).collect(),
        folder_templates: (0..nf).map(|_| OnceLock::new()).collect(),
        body: (0..n).map(|_| OnceLock::new()).collect(),
        check: (0..n).map(|_| OnceLock::new()).collect(),
        tests: (0..n).map(|_| OnceLock::new()).collect(),
        regs: (0..n).map(|_| OnceLock::new()).collect(),
        program: OnceLock::new(),
        collect: OnceLock::new(),
        wasm: OnceLock::new(),
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
        Executor::Pool(threads) => hd_sched::pool::run_pool(g, threads, &exec).1,
    };
    if left > 0 {
        run.stop(
            Stage::PackageResult,
            &format!("{left} tasks never became ready"),
            None,
        );
    }
    let mut counters = lock(&run.counters).clone();
    counters.sort();
    let report = lock(&run.report).clone();
    let diags = std::mem::take(&mut *lock(&run.diags));
    let wasm = if diags.has_errors() {
        None
    } else {
        run.wasm.get().cloned()
    };
    let ifaces = run
        .table
        .folders
        .iter()
        .filter_map(|f| Some((f.path.clone(), run.iface_of(f.id)?.blob.clone())))
        .collect();
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
        uses,
        modules: run.table.modules.iter().map(|m| m.path.clone()).collect(),
        counters,
        report,
        ifaces,
        tir_text: std::mem::take(&mut *lock(&run.tir_text)),
        pool_items: run.pool.len(),
        memo: run.memo.stats(),
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

    fn analyze(&self) -> bool {
        matches!(self.goal, Goal::Analyze | Goal::CheckTests)
    }

    fn exec(&self, id: TaskId, kind: TaskKind, sp: &dyn Spawn) {
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

    /// Records a stage's answer. A "not implemented" answer stops a build
    /// with an internal `unsupported` diagnostic; analysis only counts it.
    fn stage<T>(&self, s: Stage, r: StageResult<T>) -> Option<T> {
        match r {
            Ok(v) => {
                lock(&self.report).ok(s);
                Some(v)
            }
            Err(e) => {
                lock(&self.report).not_implemented(s, &e.what);
                if !self.analyze() {
                    self.stop(s, &e.what, e.span);
                }
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
        // The prelude's fixed uses (`module.prelude.fixed-uses`).
        if module != "std.core" {
            uses.extend(hd_resolve::prelude_modules().into_iter().map(str::to_owned));
        }
        let facts = ModuleFacts {
            path: self.table.modules[m].path.clone(),
            has_tests_block: sk.bodies.iter().any(|b| b.kind == HeaderKind::Tests),
            top_level_statements: sk
                .bodies
                .iter()
                .filter(|b| b.kind == HeaderKind::Control && b.header_indent == 0)
                .count(),
            impls: sk
                .bodies
                .iter()
                .filter(|b| b.kind == HeaderKind::Impl && b.header_indent == 0)
                .count(),
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
                    d.push(x.code, x.severity, span, x.code.as_str(), None);
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
            let flat_main =
                module.path == format!("{}.main", hd_project::package_ident(&self.package));
            if module.package != 0 || !(module.entry || flat_main) {
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
            let io = sp.add(TaskKind::InitOrder(f.raw()), &[t]);
            sp.edge(hc, pr);
            sp.edge(io, pr);
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
        for (m, module) in self.table.modules.iter().enumerate() {
            if module.package != 0 && !reached[module.folder.idx()] {
                continue;
            }
            let deps: Vec<TaskId> = g.closure[module.folder.idx()]
                .iter()
                .filter_map(|c| iface_task[c.idx()])
                .collect();
            let prep = sp.add(TaskKind::ModulePrep(u32_of(m)), &deps);
            sp.edge(prep, pr);
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

    /// The package key of a folder's package.
    fn package_key_of(&self, package: u16) -> Hash128 {
        if package == 0 {
            self.package_key
        } else {
            hd_cache::package_key(&self.table.packages[usize::from(package)])
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

    /// Re-emits a folder's stage-A diagnostics against this run's files.
    fn emit_iface_diags(&self, fi: usize, diags: &IfaceDiags) {
        let folder = &self.table.folders[fi];
        let mut d = lock(&self.diags);
        for (m, code, anchor, msg) in diags {
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
            d.push(*code, Severity::Error, span, msg, None);
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
        let items = hd_resolve::interface_items(&all);
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
        self.put(EntryKind::Iface, key, &[&blob, &dsec, &asec]);
        let Some(mentions) = self.mentions(fid, &items, &out.exports) else {
            self.blocked(Stage::FolderIface);
            let _ = self.iface[fi].set(None);
            return;
        };
        let mut iface =
            hd_resolve::folder_iface(&folder.path, items, out.exports, Arc::from(blob), &mentions);
        iface.spans = spans_of(&iface.items, &rows);
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
        }
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
        let mods = [self.mod_in(m)];
        let out = hd_resolve::build_folder(&cx, &mods, Some(own), stage, diags)?;
        Ok(out.modules.into_iter().next())
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

    /// `InitOrder(F)` (§4.13.10). Its statement order is a `graph` part
    /// keyed by `init_key`, so a warm run with no change to what it reads
    /// reads it (cache.md, "Part keys"). What it reads is each module's
    /// facts, which stand in for the init summary until the checker writes
    /// one.
    fn init_order(&self, fi: usize) {
        let folder = &self.table.folders[fi];
        let mut facts: Vec<&ModuleFacts> = folder
            .modules
            .iter()
            .map(|m| &self.skim_of(m.idx()).facts)
            .collect();
        facts.sort_by(|a, b| a.path.cmp(&b.path));
        let summaries: Vec<(&str, Hash128)> = facts
            .iter()
            .map(|f| (f.path.as_str(), facts_hash(f)))
            .collect();
        let key = init_key(self.toolchain, &folder.path, &summaries);
        if let Some(sections) = self.lookup(EntryKind::Graph, key)
            && sections.first().and_then(|b| decode_order(b)).is_some()
        {
            lock(&self.report).ok(Stage::InitOrder);
            return;
        }
        self.computed(Stage::InitOrder);
        let r = init_order(&folder.path, &facts);
        if let Some(order) = self.stage(Stage::InitOrder, r) {
            self.put(EntryKind::Graph, key, &[&encode_order(&order)]);
        }
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
        // Impls the orphan check rejected (`orphan-impl`, `nonlocal-impl`)
        // are already one error; they stay out of the overlap check.
        let placed = all.iter().flat_map(|i| i.items.iter()).filter(|it| {
            hd_resolve::lower::misplaced_impl(&names, &names.module_of(it.def), it).is_none()
        });
        let u = hd_resolve::Universe::new(names, placed);
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
        Some(check_key(
            self.toolchain,
            self.package_key,
            &module.path,
            self.role(m),
            self.skim_of(m).token_hash,
            &closure,
        ))
    }

    /// The source hashes of the template modules a module's opt-ins may
    /// instantiate. An opt-in checks its template's body, which no
    /// interface carries yet (spec 14 `annot.limit.interfaces`), so a
    /// template edit must reach the key of every module that opts in. A
    /// module whose text names neither `derive` nor `Structure` opts in
    /// nowhere. std's sources are in the toolchain key already.
    fn template_sources(&self, m: usize) -> Option<Vec<(String, Hash128)>> {
        let text = &self.texts[m];
        if !text.contains("derive") && !text.contains("Structure") {
            return Some(Vec::new());
        }
        let mut out = std::collections::BTreeMap::new();
        for c in self.closure(self.table.modules[m].folder) {
            let iface = self.iface_of(c)?;
            let paths = self.folder_templates[c.idx()].get_or_init(|| {
                let names = self.names();
                let mut ps: Vec<String> = iface
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
                    .map(|it| names.module_of(it.def))
                    .filter(|p| !p.starts_with("std."))
                    .collect();
                ps.sort();
                ps.dedup();
                ps
            });
            for p in paths {
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
        let Some(key) = self.check_key(mi) else {
            for s in [Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
                self.blocked(s);
            }
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return;
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
                return;
            }
            lock(&self.report).ok(Stage::ModulePrep);
            self.read_check(mi, &sections, Arc::from(join_sections(&sections)));
            let _ = self.prep[mi].set(None);
            return;
        }
        if !self.parses(mi) {
            for s in [Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
                self.blocked(s);
            }
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return;
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
            return;
        };
        let _ = self.prep[mi].set(Some(PrepOut {
            items,
            diags: DiagBuf::default(),
        }));
        let Some(pr) = self.graph.get().map(|g| g.package_result) else {
            return;
        };
        let body = sp.add(TaskKind::Body(m), &[id]);
        let finish = sp.add(TaskKind::ModuleFinish(m), &[body]);
        let overlay = sp.add(TaskKind::TestOverlay(m), &[id]);
        sp.edge(finish, pr);
        sp.edge(overlay, pr);
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
        let lookup = Lookup::new(&prep.items, ifaces.iter().map(AsRef::as_ref).collect());
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
        };
        let src = self.src(m);
        let heads = hd_resolve::heads(&names, &src, &module.path);
        let Ok(Some(lowered)) = self.lower_module(m, Stage::Body, &mut DiagBuf::default()) else {
            self.blocked(Stage::Body);
            let _ = self.body[m].set(None);
            return;
        };
        let scope = lowered.scope;
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
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
            results: std::cell::RefCell::default(),
            locals: &locals,
        };
        let mut diags = DiagBuf::default();
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
                .partition(|s| hd_check::tests::is_registration(&src, *s));
        }
        let mut init_facts = Vec::new();
        if !stmts.is_empty() {
            // A test program has no entry module: every top level is
            // requirement-free.
            let entry_name = match &self.goal {
                Goal::Program { entry } => Some(entry.as_str()),
                Goal::Tests { .. } => None,
                Goal::Analyze | Goal::CheckTests => Some("main"),
            };
            let entry = module.package == 0
                && entry_name.is_some_and(|e| {
                    module.path == format!("{}.{e}", hd_project::package_ident(&self.package))
                });
            let item = hd_check::default_body_def(&names, names.item(&module.path, "init"), "init");
            match hd_check::init::check_init(&cx, item, &module.path, &stmts, entry, &mut diags) {
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
        diags.append(&hd_check::results::cycles(&cx));
        // A written implementation writes every required trait method
        // (spec 09 `trait.impl.required`), reported at its header.
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Impl)
        {
            let missing = hd_check::omitted_trait_methods(&names, &lookup, h.def);
            if !missing.is_empty() {
                let msg = format!(
                    "the implementation does not write `{}`",
                    missing.join("`, `")
                );
                diags.error(Code::MissingTraitMethod, src.span(h.node), &msg);
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
        // A test run checks the `tests:` blocks too, into the module's
        // test-role `check` entry (`check_key` role "test"). A test module
        // or an integration test module is test code throughout: its
        // top-level registrations are checked in every role.
        if self.role(m) == "test" {
            test_stmts.extend(hd_check::tests::tests_block_statements(src.root()));
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
        if !init_facts.is_empty() {
            hd_check::init::definite_init(&cx, &stmts, &init_facts, &mut diags);
        }
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
        let _ = self.body[m].set(Some((bodies, diags, derived.items, inferred)));
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
        // Per template module, in module order: (item index, opt-in, the
        // template methods it checks).
        let mut groups: std::collections::BTreeMap<usize, Vec<(usize, hd_check::derive::OptIn)>> =
            std::collections::BTreeMap::new();
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
            let Some(template) = template_of(&names, cx.lookup, *trait_) else {
                continue;
            };
            let Some(tm) = self.table.module(&names.module_of(template)) else {
                continue;
            };
            let omitted = heads
                .iter()
                .find(|h| h.def == it.def)
                .map(|h| hd_check::derive::omitted_members(&cx.src, h.node))
                .unwrap_or_default();
            if let Some(mut opt) =
                hd_check::derive::OptIn::new(&names, cx.lookup, it, template, &omitted)
            {
                opt.facts = derivation_facts(cx, heads, it.def, opt.data);
                groups.entry(tm.idx()).or_default().push((i, opt));
            }
        }
        let mut found: Vec<OptInFinding> = Vec::new();
        let mut derived = Derived::default();
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
            };
            let src = self.src(tm);
            let theads = hd_resolve::heads(&names, &src, &self.table.modules[tm].path);
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
                locals: &tlocals,
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

    /// Checks opt-ins whose template module `tcx` sees: each checks the
    /// template methods its block does not write (`annot.block.methods`).
    /// A clean opt-in adds its derive instances and its `Structure`'s
    /// bodies to `derived` (codegen.md §12.3); a `Structure` the generator
    /// does not carry yet adds no body, so a call of it stops at
    /// collection.
    fn check_opt_ins(
        &self,
        tcx: &BodyCx<'_>,
        theads: &[hd_resolve::Head<'_>],
        opts: &[(usize, hd_check::derive::OptIn)],
        derived: &mut Derived,
    ) -> Vec<OptInFinding> {
        let names = self.names();
        let k = &self.known;
        let bodies = hd_resolve::body_nodes(&names, &tcx.src, theads);
        let mut out = Vec::new();
        for (i, opt) in opts {
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
                    if let Ok(g) = hd_check::structure::structure_bodies(tcx, opt, opt.facts) {
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
    /// module's items; TIR, one record per body.
    fn module_finish(&self, m: usize) {
        let (Some(Some(prep)), Some(Some((bodies, bdiags, derived, inferred)))) =
            (self.prep[m].get(), self.body[m].get())
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
        // An omitted result type is the inferred one in the module's items,
        // which `Collect` and the program's emission read.
        let inferred: HashMap<DefId, Ty> = inferred.iter().copied().collect();
        for it in &mut all_items {
            if let Some(&ret) = inferred.get(&it.def)
                && let ItemData::Fn(sig) | ItemData::Method { sig, .. } = &mut it.data
            {
                sig.ret = ret;
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
        let sections: [&[u8]; 5] = [&dw.bytes, &meta.bytes, &items, &tw.bytes, &rw.bytes];
        self.put(EntryKind::Check, key, &sections);
        lock(&self.counters)
            .modules_checked
            .push(self.table.modules[m].path.clone());
        lock(&self.report).ok(Stage::ModuleFinish);
        let owned: Vec<Vec<u8>> = sections.iter().map(|s| s.to_vec()).collect();
        self.read_check(m, &owned, Arc::from(join_sections(&owned)));
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
        let content = Reader::new(meta).hash();
        let _ = self.check[m].set(Some(CheckOut {
            entry,
            content,
            has_errors,
        }));
    }

    fn package_result(&self, sp: &dyn Spawn) {
        lock(&self.report).ok(Stage::PackageResult);
        match &self.goal {
            Goal::Program { .. } | Goal::Tests { .. } => {
                if !lock(&self.diags).has_errors() {
                    sp.add(TaskKind::Ext(ExtTask::Collect), &[]);
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
        }
    }

    /// `Collect` (codegen.md §11.3): `prog_key`, then instances, then code
    /// keys; hits are looked up here, and only misses become `Emit` tasks.
    /// A program's roots are `main` (with its report), or for a test run
    /// each running case (engines-and-test-runner.md §19.1).
    fn collect(&self, sp: &dyn Spawn) {
        let names = self.names();
        let (entry_key, roots) = match &self.goal {
            Goal::Program { entry } => {
                let entry_module = format!("{}.{entry}", hd_project::package_ident(&self.package));
                let root = names.item(&entry_module, "main");
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
        let pkey = prog_key(self.toolchain, self.pipeline, &entry_key, &modules);
        if let Some(sections) = self.lookup(EntryKind::Link, pkey)
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
        let has_main =
            p.bodies.contains_key(&main) && (public || !matches!(self.goal, Goal::Program { .. }));
        let script = matches!(self.goal, Goal::Program { .. }) && !has_main;
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
            let order = match self.group_init_order(m, &has_init) {
                Ok(o) => o,
                Err(e) => {
                    self.stage::<()>(Stage::Collect, Err(e));
                    return;
                }
            };
            let defs: Vec<DefId> = order
                .iter()
                .map(|m| init_of(m))
                .filter(|d| p.bodies.contains_key(d))
                .collect();
            for d in &defs {
                if !all_inits.contains(d) {
                    all_inits.push(*d);
                }
            }
            per_module.push(defs);
        }
        // Extra roots: the other cases, then each distinct report function.
        let mut extra: Vec<(DefId, TyList)> = roots[1..]
            .iter()
            .map(|(d, _)| (*d, TyList::EMPTY))
            .collect();
        let reports: Vec<Option<(DefId, TyList)>> = roots
            .iter()
            .map(|(d, _)| self.report_fn(p, env.ret(*d).unwrap_or(Ty::VOID)))
            .collect();
        for r in reports.iter().flatten() {
            if !extra.contains(r) {
                extra.push(*r);
            }
        }
        let Some(collected) = self.stage(
            Stage::Collect,
            hd_mono::collect(&self.pool, &env, &TableSolver, root, &all_inits, &extra),
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
        let key_of_init = |d: &DefId| {
            all_inits
                .iter()
                .position(|x| x == d)
                .and_then(|i| collected.inits.get(i).copied())
        };
        let plan = match &self.goal {
            Goal::Program { .. } if script => Roots::Script {
                def: root,
                key: collected.table.key[0],
            },
            Goal::Program { .. } => Roots::Main {
                def: root,
                key: collected.table.key[0],
                report: reports[0].as_ref().and_then(key_of_extra),
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
    /// that contain it (`cli.test.filter`).
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
                if filter.as_ref().is_none_or(|f| r.name.contains(f.as_str())) {
                    out.push((m, r.clone()));
                }
            }
        }
        out
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

    /// The modules the entry module reaches, in initialization order
    /// (spec/lang/10-modules.md, "Initialization Order"): each
    /// initialization group (a strongly connected component of the use
    /// graph) after the groups it uses; among ready groups, the one with
    /// the least module identity first; inside a group, by module path.
    fn group_init_order(
        &self,
        entry: &str,
        has_init: &dyn Fn(&str) -> bool,
    ) -> hd_base::StageResult<Vec<String>> {
        let Some(start) = self.table.module(entry) else {
            return Ok(Vec::new());
        };
        let n = self.table.modules.len();
        let edges: Vec<Vec<usize>> = (0..n)
            .map(|m| {
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
            })
            .collect();
        // Tarjan's strongly connected components over the reachable graph.
        let mut index = vec![usize::MAX; n];
        let mut low = vec![0; n];
        let mut on = vec![false; n];
        let mut stack = Vec::new();
        let mut comp = vec![usize::MAX; n];
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut counter = 0;
        // Iterative DFS: (node, next edge).
        let mut work = vec![(start.idx(), 0usize)];
        index[start.idx()] = counter;
        low[start.idx()] = counter;
        counter += 1;
        stack.push(start.idx());
        on[start.idx()] = true;
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
                g.sort_by(|a, b| {
                    self.table.modules[*a]
                        .path
                        .cmp(&self.table.modules[*b].path)
                });
                groups.push(g);
            }
        }
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
        let name = |g: usize| self.table.modules[groups[g][0]].path.clone();
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
            let with_init: Vec<&usize> = groups[next]
                .iter()
                .filter(|m| has_init(&self.table.modules[**m].path))
                .collect();
            if with_init.len() > 1 {
                return Err(NotImplemented::new(
                    Stage::Collect,
                    "statement order inside a multi-module initialization group (`InitOrder`)",
                ));
            }
            out.extend(
                groups[next]
                    .iter()
                    .map(|m| self.table.modules[*m].path.clone()),
            );
        }
        Ok(out)
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
                match body.or_else(|| c.collected.supplied_body(&self.pool, item, args)) {
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
            Roots::Main { def, key, report } => hd_wasm::entry(
                &self.pool,
                &env,
                &path,
                *def,
                *key,
                &c.collected.inits,
                *report,
            ),
            Roots::Script { def, key } => {
                hd_wasm::script_entry(&self.pool, &env, &path, *def, *key, &c.collected.inits)
            }
            Roots::Tests { cases, inits } => {
                hd_wasm::test_entry(&self.pool, &env, &path, cases, inits)
            }
        };
        let Some(entry) = self.stage(Stage::Link, exports) else {
            return;
        };
        let Some(wasm) = self.stage(Stage::Link, hd_wasm::link(&codes, &fnames, &entry)) else {
            return;
        };
        self.put(EntryKind::Link, c.prog_key, &[&wasm]);
        let _ = self.wasm.set(wasm);
    }
}

impl hd_resolve::World for Run<'_> {
    fn module_folder(&self, module: &str) -> Option<u32> {
        self.table.folder_of_module(module).map(FolderId::raw)
    }
    fn usable(&self, module: &str) -> bool {
        self.table.usable(module)
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

/// A module's part of `init_key`: every fact `InitOrder` reads. The
/// destructuring names each field, so a new fact cannot miss the key.
fn facts_hash(f: &ModuleFacts) -> Hash128 {
    let ModuleFacts {
        path,
        has_tests_block,
        top_level_statements,
        impls,
    } = f;
    let mut k = hd_base::StableHasher::new("init-facts");
    k.str(path);
    k.u8(u8::from(*has_tests_block));
    k.u64(u64::try_from(*top_level_statements).unwrap_or(u64::MAX));
    k.u64(u64::try_from(*impls).unwrap_or(u64::MAX));
    k.finish()
}

/// `InitOrder`'s statement order for one folder.
fn encode_order(order: &[String]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(order);
    for s in order {
        w.str(s);
    }
    w.bytes
}

fn decode_order(b: &[u8]) -> Option<Vec<String>> {
    let mut r = Reader::new(b);
    let order = (0..r.count()).map(|_| r.str().to_owned()).collect();
    r.ok().then_some(order)
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
    for (m, code, a, msg) in d {
        w.u32(u32_of(*m));
        w.str(code.as_str());
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
        let a = hd_resolve::anchor::Anchor {
            decl: r.u32(),
            member: r.u32(),
            tok: r.u32(),
            count: r.u32(),
        };
        out.push((m, code, a, r.str().to_owned()));
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

    /// A default body (codegen.md §13.13): a body of its own, whose
    /// parameters are the earlier parameters and whose result is the
    /// parameter's or field's declared type.
    fn default_body(&self, def: DefId) -> Option<&Body> {
        self.p
            .bodies
            .get(&def)
            .map(|b| &b.0)
            .filter(|b| b.kind == hd_tir::BodyKind::Default)
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
            if *tag != hd_tir::Tag::Call {
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
        self.sig(def).is_some_and(|s| s.suspends)
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
            ItemData::Impl { .. } => Some((*owner, o.generics.len())),
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
        let (name, owner) = match &self.p.items.get(&method)?.data {
            ItemData::Method { owner, .. } => (self.p.items.get(&method)?.name, *owner),
            _ => return None,
        };
        // A derivation's own `Structure` (codegen.md §12.3): the hidden
        // methods of the derived implementation.
        if owner == self.run.known.structure {
            let names = self.run.names();
            let d = names.member(impl_, hd_intern::PathKind::Hidden, names.text(name));
            return self.p.items.contains_key(&d).then_some(d);
        }
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
            // `downcast`) has no slot.
            Some(ItemData::Trait(t)) => t
                .methods
                .iter()
                .map(|m| m.1)
                .filter(|m| self.p.items.get(m).is_none_or(|i| i.intrinsic.is_none()))
                .collect(),
            _ => Vec::new(),
        }
    }
    fn trait_arity(&self, trait_: DefId) -> usize {
        self.p.items.get(&trait_).map_or(0, |i| i.generics.len())
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
        format!("{}.{name}", module.replace('-', "_"))
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
    fn map_key_items(&self) -> (DefId, DefId) {
        (self.run.known.eq, self.run.known.hash_of)
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

/// Whether the derivation `impl_` of `data` sees a fact (annotations,
/// "Facts"): a decorator on the declaration or a member, a member line of
/// its block, or a trait-less block for the type.
fn derivation_facts(
    cx: &BodyCx<'_>,
    heads: &[hd_resolve::Head<'_>],
    impl_: DefId,
    data: DefId,
) -> bool {
    let head = |d: DefId| heads.iter().find(|h| h.def == d).map(|h| h.node);
    let pool = cx.names.pool;
    head(data).is_some_and(|n| hd_check::derive::has_decorator_facts(&cx.src, n))
        || head(impl_).is_some_and(|n| hd_check::derive::has_fact_lines(&cx.src, n))
        || cx.lookup.own.iter().any(|it| {
            matches!(&it.data, ItemData::Impl { trait_, by: Some(_), self_ty, .. }
                if *trait_ == DefId::NONE
                    && matches!(pool.get(*self_ty), TyData::Adt { def, .. } if def == data))
        })
}

/// The derivation template of `trait_` (`annot.template.form`), among a
/// module's own items, then the interfaces it sees.
/// A template declared outside its trait's module is rejected
/// (`misplaced-derivation`) and derives nothing.
fn template_of(names: &Names<'_>, lookup: &Lookup<'_>, trait_: DefId) -> Option<DefId> {
    let is = |it: &&Item| {
        matches!(&it.data, ItemData::Impl { trait_: t, kind: hd_resolve::ImplKind::Template, .. } if *t == trait_)
            && !hd_resolve::lower::misplaced_template(names, &names.module_of(it.def), it)
    };
    lookup
        .own
        .iter()
        .find(is)
        .or_else(|| lookup.ifaces.iter().find_map(|f| f.items.iter().find(is)))
        .map(|it| it.def)
}
