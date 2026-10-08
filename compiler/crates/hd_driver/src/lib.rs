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
    CacheStore, EntryKind, FileApi, MemoryStore, check_key, code_key, iface_key, prog_key,
    toolchain_key,
};
use hd_check::BodyCx;
use hd_check::stages::{ModuleFacts, init_order, test_overlay};
use hd_diag::{Code, DiagBuf, Severity};
use hd_intern::{PathTable, ShardedInterner};
use hd_mono::layout::LayoutEnv;
use hd_mono::{Collected, ProgramEnv};
use hd_project::{FolderGraph, MemorySources, ModuleTable, SourceSet};
use hd_resolve::{FolderIface, Item, ItemData, Lookup, ModOut, Names, Src};
use hd_sched::{ExtTask, SerialOrder, SerialScheduler, Spawn, TaskGraph, TaskId, TaskKind};
use hd_syntax::{HeaderKind, Parse, parse, skim};
use hd_tir::Body;
use hd_types::solver::{GlobalMemo, ImplTable, ImplUniverses, TableSolver};
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
    pub counters: Counters,
    pub report: PipelineReport,
    /// Each built folder interface's blob, by folder path.
    pub ifaces: Vec<(String, Arc<[u8]>)>,
    /// Readable TIR of the bodies a `Host::render_tir` named, by item path.
    pub tir_text: std::collections::BTreeMap<String, String>,
    /// A test run's selected cases, in content order.
    pub tests: Vec<TestCase>,
}

impl Output {
    /// Diagnostics as `file:lo..hi: error code: message`, in content order.
    #[must_use]
    pub fn render(&self) -> String {
        self.diags.render_compact(&|s: Span| {
            s.file
                .get()
                .and_then(|_| self.files.get(s.file.idx()).cloned())
                .unwrap_or_default()
        })
    }
}

struct SkimOut {
    source_hash: Hash128,
    api_text_hash: Hash128,
    uses: Vec<String>,
    facts: ModuleFacts,
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
type IfaceDiags = Vec<(usize, Code, u32, u32, String)>;

/// A module's TIR and body diagnostics after `Body(m)`.
type BodyOut = (Vec<Body>, DiagBuf);

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
    impl_tables: Vec<(ModuleId, ImplTable)>,
}

struct CollectOut {
    collected: Collected,
    order: Vec<hd_base::InstId>,
    codes: Vec<OnceLock<WasmCode>>,
    code_keys: Vec<Hash128>,
    prog_key: Hash128,
    roots: Roots,
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
    syms: ShardedInterner,
    universes: ImplUniverses,
    memo: GlobalMemo,
    toolchain: Hash128,
    package_key: Hash128,
    pipeline: Hash128,
    skim: Vec<OnceLock<SkimOut>>,
    parse: Vec<OnceLock<Parse>>,
    graph: OnceLock<GraphOut>,
    iface: Vec<OnceLock<Option<Arc<FolderIface>>>>,
    prep: Vec<OnceLock<Option<PrepOut>>>,
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

/// One run with the caller's sources, store, executor and clock.
#[must_use]
pub fn build(host: &Host<'_>, package: &str, goal: &Goal) -> Output {
    let std = if package == "std" {
        with_core(host.sources)
    } else {
        std_sources()
    };
    let table = if package == "std" {
        ModuleTable::discover_all(&[("std", &std)])
    } else {
        ModuleTable::discover_all(&[(package, host.sources), ("std", &std)])
    };
    let texts: Vec<Arc<str>> = table
        .sources
        .iter()
        .map(|(pi, p)| {
            let set: &dyn SourceSet = if *pi == 0 && package != "std" {
                host.sources
            } else {
                &std
            };
            Arc::from(
                set.read(p)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let std_hash = sources_hash(&std);
    let n = table.modules.len();
    let nf = table.folders.len();
    let pipeline = hd_mono::passes::validate(&hd_mono::passes::DEV).unwrap_or_default();
    let run = Run {
        host,
        package: package.to_owned(),
        goal: goal.clone(),
        texts,
        pool: InternPool::new(),
        paths: PathTable::new(),
        syms: ShardedInterner::default(),
        universes: ImplUniverses::default(),
        memo: GlobalMemo::default(),
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
    let mut g = TaskGraph::default();
    let mut skims = Vec::new();
    for i in 0..n {
        skims.push(g.add(TaskKind::Skim(u32::try_from(i).expect("files")), &[]));
        if *goal == Goal::Analyze {
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
    Output {
        tests: cases,
        wasm,
        diags,
        files: run.table.files.clone(),
        counters,
        report,
        ifaces,
        tir_text: std::mem::take(&mut *lock(&run.tir_text)),
    }
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

impl Run<'_> {
    fn names(&self) -> Names<'_> {
        Names {
            pool: &self.pool,
            paths: &self.paths,
            syms: &self.syms,
        }
    }

    fn analyze(&self) -> bool {
        self.goal == Goal::Analyze
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
            &format!("unsupported: {}: {what}", s.name()),
        );
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
        let package = &self.table.packages[usize::from(self.table.modules[m].package)];
        let mut uses = Vec::new();
        for &(lo, hi) in &sk.uses {
            let line = text.get(lo as usize..hi as usize).unwrap_or("").trim();
            let rest = line.strip_prefix("pub ").unwrap_or(line);
            if let Some(rest) = rest.strip_prefix("use ") {
                let path = rest.split(".{").next().unwrap_or(rest);
                let path = path.split(" as ").next().unwrap_or(path).trim();
                uses.push(absolute_use(path, package, module));
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
        SkimOut {
            source_hash: sk.source_hash,
            api_text_hash: sk.api_text_hash,
            uses,
            facts,
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
                &format!("folder-cycle: {}", names.join(" -> ")),
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
        if self.goal != Goal::Analyze {
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
        for (m, code, lo, hi, msg) in diags {
            let Some(mid) = folder.modules.get(*m) else {
                continue;
            };
            let span = Span {
                file: FileId::from_raw(mid.raw()),
                lo: *lo,
                hi: *hi,
            };
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
            && let Some(mentions) = self.mentions(fid, &items, &exports)
        {
            let iface = hd_resolve::folder_iface(
                &folder.path,
                items,
                exports,
                Arc::from(blob.as_slice()),
                &mentions,
            );
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
            package: &self.table.packages[usize::from(folder.package)],
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
        let all: Vec<Item> = out.modules.into_iter().flat_map(|m| m.items).collect();
        for it in &all {
            if let Some(k) = it.intrinsic
                && hd_host_abi::intrinsic(names.text(k)).is_none()
            {
                let msg = format!(
                    "unsupported: `{}` names the unknown intrinsic `{}`",
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
                    sp.lo,
                    sp.hi,
                    diags.get_text(diags.message[i]).to_owned(),
                )
            })
            .collect();
        let dsec = encode_iface_diags(&idiags);
        self.put(EntryKind::Iface, key, &[&blob, &dsec]);
        let Some(mentions) = self.mentions(fid, &items, &out.exports) else {
            self.blocked(Stage::FolderIface);
            let _ = self.iface[fi].set(None);
            return;
        };
        let iface =
            hd_resolve::folder_iface(&folder.path, items, out.exports, Arc::from(blob), &mentions);
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
            package: &self.table.packages[usize::from(module.package)],
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

    /// The span of an item's module file, for interface-level findings.
    fn item_span(&self, d: DefId) -> Span {
        let file = self
            .table
            .module(&self.names().module_of(d))
            .map_or(u32::MAX, |m| self.table.modules[m.idx()].file.raw());
        Span {
            file: FileId::from_raw(file),
            lo: 0,
            hi: 0,
        }
    }

    /// `HeaderCheck(F)`: stage B (§4.10.1) over F's interface, against the
    /// interfaces of its closure.
    fn header_check(&self, fi: usize) {
        let fid = self.table.folders[fi].id;
        let (Some(own), Some(all)) = (self.iface_of(fid), self.closure_ifaces(fid)) else {
            self.blocked(Stage::HeaderCheck);
            return;
        };
        let u = hd_resolve::Universe::new(self.names(), all.iter().flat_map(|i| i.items.iter()));
        let findings = u.stage_b(&own.items);
        let mut d = lock(&self.diags);
        for f in findings {
            d.error(f.code, self.item_span(f.item), &f.message);
        }
        drop(d);
        lock(&self.report).ok(Stage::HeaderCheck);
    }

    fn init_order(&self, fi: usize) {
        let folder = &self.table.folders[fi];
        let facts: Vec<&ModuleFacts> = folder
            .modules
            .iter()
            .map(|m| &self.skim_of(m.idx()).facts)
            .collect();
        let r = init_order(&folder.path, &facts);
        self.stage(Stage::InitOrder, r);
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
        let names = self.names();
        let u = hd_resolve::Universe::new(names, all.iter().flat_map(|i| i.items.iter()));
        let order = |d: DefId| names.path(d);
        let overlaps = u.overlaps(&order);
        lock(&self.report).ok(Stage::Coherence);
        let mut d = lock(&self.diags);
        for (a, b, witness) in overlaps {
            let msg = format!(
                "overlapping-impl: {} and {} both apply to {witness}",
                names.path(a),
                names.path(b)
            );
            d.error(Code::OverlappingImpl, self.item_span(b), &msg);
        }
    }

    fn check_key(&self, m: usize) -> Option<Hash128> {
        let module = &self.table.modules[m];
        let mut closure = Vec::new();
        for c in self.closure(module.folder) {
            closure.push((
                self.table.folders[c.idx()].path.as_str(),
                self.iface_of(c)?.deep_hash,
            ));
        }
        Some(check_key(
            self.toolchain,
            self.package_key,
            &module.path,
            self.role(m),
            self.skim_of(m).source_hash,
            &closure,
        ))
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
            lock(&self.report).ok(Stage::ModulePrep);
            self.read_check(mi, &sections, Arc::from(join_sections(&sections)));
            let _ = self.prep[mi].set(None);
            return;
        }
        if !self.parse_of(mi).is_ok() {
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

    fn impl_tables_of(&self, own: &[Item], closure: &[FolderId]) -> Vec<(ModuleId, ImplTable)> {
        let names = self.names();
        let mut out = Vec::new();
        let own_impls: Vec<&Item> = own
            .iter()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }))
            .collect();
        out.push((
            ModuleId::from_raw(u32::MAX - 1),
            hd_resolve::impl_table(&names, &own_impls),
        ));
        for f in closure {
            if let Some(i) = self.iface_of(*f) {
                let impls: Vec<&Item> = i
                    .items
                    .iter()
                    .filter(|x| matches!(x.data, ItemData::Impl { .. }))
                    .collect();
                out.push((
                    ModuleId::from_raw(f.raw()),
                    hd_resolve::impl_table(&names, &impls),
                ));
            }
        }
        out
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
        let tables = self.impl_tables_of(&prep.items, &closure);
        let table_refs: Vec<(ModuleId, &ImplTable)> = tables.iter().map(|(m, t)| (*m, t)).collect();
        let names = self.names();
        let src = self.src(m);
        let heads = hd_resolve::heads(&names, &src, &module.path);
        let Ok(Some(lowered)) = self.lower_module(m, Stage::Body, &mut DiagBuf::default()) else {
            self.blocked(Stage::Body);
            let _ = self.body[m].set(None);
            return;
        };
        let scope = lowered.scope;
        let universe = self.universes.intern(&closure);
        let solver = TableSolver;
        let cx = BodyCx {
            names,
            src,
            scope: &scope,
            lookup: &lookup,
            impls: &table_refs,
            universe,
            global: &self.memo,
            solver: &solver,
            methods: std::cell::OnceCell::new(),
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
        };
        let mut diags = DiagBuf::default();
        let mut bodies = Vec::new();
        let mut failed = None;
        // Top-level statements first: their bindings are visible to every
        // function body of the module (checking-and-tir.md §4.13.10).
        let stmts = hd_check::init::init_statements(src.root());
        let mut init_facts = Vec::new();
        if !stmts.is_empty() {
            // A test program has no entry module: every top level is
            // requirement-free.
            let entry_name = match &self.goal {
                Goal::Program { entry } => Some(entry.as_str()),
                Goal::Tests { .. } => None,
                Goal::Analyze => Some("main"),
            };
            let entry = module.package == 0
                && entry_name.is_some_and(|e| module.path == format!("{}.{e}", self.package));
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
        for (def, node) in hd_resolve::body_nodes(&names, &src, &heads) {
            // A body-less method of a built-in family (`impl[N < Num] Add
            // for N`) is the compiler's: there is no source to check.
            if hd_resolve::Src::child(node, hd_syntax::SyntaxKind::Block).is_none() {
                continue;
            }
            match hd_check::check_fn(&cx, def, node, &mut diags) {
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
                    if self.goal != Goal::Analyze {
                        break;
                    }
                }
            }
        }
        // A test run checks the `tests:` blocks too, into the module's
        // test-role `check` entry (`check_key` role "test").
        if self.role(m) == "test" && failed.is_none() {
            match hd_check::tests::check_tests(&cx, &module.path, &mut diags) {
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
        if let Some(e) = failed {
            self.stage::<()>(Stage::Body, Err(e));
            let _ = self.body[m].set(None);
            return;
        }
        lock(&self.report).ok(Stage::Body);
        let _ = self.body[m].set(Some((bodies, diags)));
    }

    /// `ModuleFinish(m)`: writes the `check` entry. Sections: diagnostics;
    /// meta (the module's TIR content hash, which `prog_key` reads); the
    /// module's items; TIR, one record per body.
    fn module_finish(&self, m: usize) {
        let (Some(Some(prep)), Some(Some((bodies, bdiags)))) =
            (self.prep[m].get(), self.body[m].get())
        else {
            self.blocked(Stage::ModuleFinish);
            let _ = self.check[m].set(None);
            return;
        };
        let names = self.names();
        let mut diags = prep.diags.clone();
        diags.append(bdiags);
        let mut dw = Writer::default();
        dw.len_of(&diags.code);
        for i in 0..diags.len() {
            dw.str(diags.code[i].as_str());
            dw.u32(diags.primary[i].lo);
            dw.u32(diags.primary[i].hi);
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
        let all_items: Vec<Item> = prep.items.iter().chain(case_items).cloned().collect();
        let mut rw = Writer::default();
        encode_regs(regs, &mut rw);
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
        let mut r = Reader::new(d);
        let mut buf = DiagBuf::default();
        for _ in 0..r.count() {
            let code = Code::from_name(r.str()).unwrap_or(Code::Unsupported);
            let (lo, hi) = (r.u32(), r.u32());
            let msg = r.str().to_owned();
            buf.push(
                code,
                Severity::Error,
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
            .map(|b| decode_regs(&mut Reader::new(b)))
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
            Goal::Analyze => {
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
        let mut impls: Vec<&Item> = items
            .values()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }))
            .collect();
        impls.sort_by_key(|i| names.path_hash(i.def));
        let impl_tables = vec![(
            ModuleId::from_raw(0),
            hd_resolve::impl_table(&names, &impls),
        )];
        lock(&self.counters).tir_decoded.extend(decoded);
        let _ = self.program.set(ProgramTables {
            items,
            bodies,
            impl_tables,
        });
        self.program.get()
    }

    /// `Collect` (codegen.md §11.3): `prog_key`, then instances, then code
    /// keys; hits are looked up here, and only misses become `Emit` tasks.
    /// A program's roots are `main` (with its report), or for a test run
    /// each running case (engines-and-test-runner.md §19.1).
    fn collect(&self, sp: &dyn Spawn) {
        let names = self.names();
        let (entry_key, roots) = match &self.goal {
            Goal::Program { entry } => {
                let entry_module = format!("{}.{entry}", self.package);
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
            Goal::Analyze => return,
        };
        let mut modules: Vec<(&str, Hash128)> = Vec::new();
        for (m, module) in self.table.modules.iter().enumerate() {
            if module.package != 0 {
                continue;
            }
            let Some(Some(c)) = self.check[m].get() else {
                self.blocked(Stage::Collect);
                return;
            };
            modules.push((module.path.as_str(), c.content));
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
        let script = matches!(self.goal, Goal::Program { .. }) && !p.bodies.contains_key(&main);
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
                &format!(
                    "missing-entry-point: `{entry_key}` has no `fn main` or top-level statements"
                ),
            );
            return;
        }
        let env = Env { run: self, p };
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
        let mut code_keys = Vec::new();
        let codes: Vec<OnceLock<WasmCode>> = order.iter().map(|_| OnceLock::new()).collect();
        let mut misses = Vec::new();
        for (slot, id) in order.iter().enumerate() {
            let item = collected.table.item[id.idx()];
            let tir = p.bodies.get(&item).map_or(Hash128(0), |b| b.1);
            let mut reps = hd_base::StableHasher::new("code-deps");
            reps.hash(self.toolchain);
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
        });
        let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
        for slot in misses {
            let e = sp.add(TaskKind::Ext(ExtTask::Emit(u32_of(slot))), &[]);
            sp.edge(e, link);
        }
        sp.release(link);
    }

    /// The `check` role of module `m`: a test run checks the package's
    /// modules with their `tests:` blocks, into separate entries.
    fn role(&self, m: usize) -> &'static str {
        if matches!(self.goal, Goal::Tests { .. }) && self.table.modules[m].package == 0 {
            "test"
        } else {
            ROLE
        }
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
        let TyData::Adt { def, args } = pool.get(ret) else {
            return Some((rt("entry_status"), pool.list(&[ret])));
        };
        if def != names.item("std.core", "Result") {
            return Some((rt("entry_status"), pool.list(&[ret])));
        }
        let a = pool.list_items(args);
        let (ok, err) = (a[0], a[1]);
        let error = names.item("std.error", "Error");
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
        let Some((body, _)) = p.bodies.get(&item) else {
            self.stage::<()>(
                Stage::Emit,
                Err(NotImplemented::new(Stage::Emit, "an instance without TIR")),
            );
            return;
        };
        let env = Env { run: self, p };
        let names = self.names();
        let ret = env.ret(item).unwrap_or(Ty::VOID);
        let path = |d: DefId| names.path(d);
        let r = hd_wasm::emit(
            &self.pool,
            &env,
            &path,
            body,
            sub,
            args,
            ret,
            &c.collected.calls[id.idx()],
            c.collected.table.key[id.idx()],
        );
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
        let env = Env { run: self, p };
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
    fn iface(&self, folder: u32) -> Option<Arc<FolderIface>> {
        self.iface_of(FolderId::from_raw(folder))
    }
}

/// A use path made absolute: `pkg` is the package, `self` the module,
/// each leading `super` its parent.
fn absolute_use(path: &str, package: &str, module: &str) -> String {
    let mut segs = path.split('.');
    match segs.next() {
        Some("pkg") => std::iter::once(package)
            .chain(segs)
            .collect::<Vec<_>>()
            .join("."),
        Some("self") => std::iter::once(module)
            .chain(segs)
            .collect::<Vec<_>>()
            .join("."),
        Some("super") => {
            let mut base: Vec<&str> = module.split('.').collect();
            base.pop();
            let mut rest: Vec<&str> = segs.collect();
            while rest.first() == Some(&"super") {
                rest.remove(0);
                base.pop();
            }
            base.extend(rest);
            base.join(".")
        }
        _ => path.to_owned(),
    }
}

/// A module's test registrations, as its test-role `check` entry holds them.
fn encode_regs(regs: &[hd_check::tests::TestReg], w: &mut Writer) {
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
        w.u32(r.at);
    }
}

fn decode_regs(r: &mut Reader<'_>) -> Vec<hd_check::tests::TestReg> {
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
            at: r.u32(),
        });
    }
    out
}

fn encode_iface_diags(d: &IfaceDiags) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(d);
    for (m, code, lo, hi, msg) in d {
        w.u32(u32_of(*m));
        w.str(code.as_str());
        w.u32(*lo);
        w.u32(*hi);
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
        let (lo, hi) = (r.u32(), r.u32());
        out.push((m, code, lo, hi, r.str().to_owned()));
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
}

impl LayoutEnv for Env<'_> {
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
}

impl Env<'_> {
    fn sig(&self, def: DefId) -> Option<&hd_resolve::FnSig> {
        self.p.items.get(&def)?.sig()
    }

    /// The row keys of a module init, which has no signature. An entry
    /// module's init has an inferred entry row (`module.init.script-row`),
    /// so its keys are those of the callees its top level calls.
    fn init_keys(&self, def: DefId) -> Vec<DefId> {
        let Some((b, _)) = self.p.bodies.get(&def) else {
            return Vec::new();
        };
        if b.kind != hd_tir::BodyKind::Init {
            return Vec::new();
        }
        let mut keys: Vec<DefId> = Vec::new();
        for (i, tag) in b.tags.iter().enumerate() {
            if *tag != hd_tir::Tag::Call {
                continue;
            }
            let Some(hd_tir::Callee::Item { def: callee, .. }) =
                hd_tir::Callee::from_words(b.record(b.data[i][0]))
            else {
                continue;
            };
            for k in self.row_keys(callee) {
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        keys
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
        self.sig(def).map(|s| s.ret)
    }
    fn params(&self, def: DefId) -> Option<Vec<Ty>> {
        self.sig(def)
            .map(|s| s.params.iter().map(|p| p.1).collect())
    }
    fn suspends(&self, def: DefId) -> bool {
        self.sig(def).is_some_and(|s| s.suspends)
    }
    fn row_keys(&self, def: DefId) -> Vec<DefId> {
        let Some(s) = self.sig(def) else {
            return self.init_keys(def);
        };
        let mut keys: Vec<DefId> = self
            .run
            .pool
            .row_data(s.row)
            .keys
            .into_iter()
            .filter_map(|k| match self.run.pool.get(k) {
                hd_types::TyData::TraitValue { def, .. } => Some(def),
                _ => None,
            })
            .collect();
        let names = self.run.names();
        keys.sort_by_key(|k| names.path_hash(*k));
        keys
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
        let name = match &self.p.items.get(&method)?.data {
            ItemData::Method { .. } => self.p.items.get(&method)?.name,
            _ => return None,
        };
        match &self.p.items.get(&impl_)?.data {
            ItemData::Impl { methods, .. } => {
                methods.iter().find(|(n, _)| *n == name).map(|(_, d)| *d)
            }
            _ => None,
        }
    }
    fn trait_methods(&self, trait_: DefId) -> Vec<DefId> {
        match self.p.items.get(&trait_).map(|i| &i.data) {
            Some(ItemData::Trait(t)) => t.methods.iter().map(|m| m.1).collect(),
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
        let names = self.run.names();
        (names.path(def) == "std/task/block_on").then(|| "block_on".to_owned())
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
    fn describe(&self, def: DefId) -> String {
        self.run.names().path(def)
    }
    fn impl_tables(&self) -> Vec<(ModuleId, &ImplTable)> {
        self.p.impl_tables.iter().map(|(m, t)| (*m, t)).collect()
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
