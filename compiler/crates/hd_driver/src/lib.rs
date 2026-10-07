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
use hd_check::stages::{ModuleFacts, coherence, init_order, test_overlay};
use hd_diag::{Code, DiagBuf, Severity};
use hd_intern::{PathTable, ShardedInterner};
use hd_mono::layout::LayoutEnv;
use hd_mono::{Collected, ProgramEnv};
use hd_project::{FolderGraph, ModuleTable, SourceSet};
use hd_resolve::{FolderIface, Item, ItemData, Lookup, Names, Src};
use hd_sched::{ExtTask, SerialOrder, SerialScheduler, Spawn, TaskGraph, TaskId, TaskKind};
use hd_syntax::{HeaderKind, Parse, parse, skim};
use hd_tir::Body;
use hd_types::solver::{GlobalMemo, ImplTable, ImplUniverses, SkeletonSolver};
use hd_types::{InternPool, Ty, TyList};
use hd_wasm::Code as WasmCode;

pub mod bench;
mod report;

pub use report::{Counters, PipelineReport, Tally};

const COMPILER: &str = "hd 0";
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
    /// Every stage over a package; "not implemented" is counted.
    Analyze,
}

/// The result of a run.
pub struct Output {
    pub wasm: Option<Vec<u8>>,
    pub diags: DiagBuf,
    /// Package-relative file paths, indexed by a diagnostic's `FileId`.
    pub files: Vec<String>,
    pub counters: Counters,
    pub report: PipelineReport,
}

impl Output {
    /// Diagnostics as `file:lo..hi: error code: message`, in content order.
    #[must_use]
    pub fn render(&self) -> String {
        self.diags
            .render_compact(&|s: Span| self.files.get(s.file.idx()).cloned().unwrap_or_default())
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
    root_key: Hash128,
}

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
    program: OnceLock<ProgramTables>,
    collect: OnceLock<CollectOut>,
    wasm: OnceLock<Vec<u8>>,
    diags: Mutex<DiagBuf>,
    report: Mutex<PipelineReport>,
    counters: Mutex<Counters>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One run with the caller's sources, store, executor and clock.
#[must_use]
pub fn build(host: &Host<'_>, package: &str, goal: &Goal) -> Output {
    let table = ModuleTable::discover(package, host.sources);
    let texts: Vec<Arc<str>> = table
        .files
        .iter()
        .map(|p| {
            Arc::from(
                host.sources
                    .read(p)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
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
        toolchain: toolchain_key(COMPILER, TARGET, "a1=bounded"),
        package_key: hd_cache::package_key(package),
        pipeline,
        skim: (0..n).map(|_| OnceLock::new()).collect(),
        parse: (0..n).map(|_| OnceLock::new()).collect(),
        graph: OnceLock::new(),
        iface: (0..nf).map(|_| OnceLock::new()).collect(),
        prep: (0..n).map(|_| OnceLock::new()).collect(),
        body: (0..n).map(|_| OnceLock::new()).collect(),
        check: (0..n).map(|_| OnceLock::new()).collect(),
        program: OnceLock::new(),
        collect: OnceLock::new(),
        wasm: OnceLock::new(),
        diags: Mutex::new(DiagBuf::default()),
        report: Mutex::new(PipelineReport {
            package: package.to_owned(),
            ..PipelineReport::default()
        }),
        counters: Mutex::new(Counters::default()),
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
    Output {
        wasm,
        diags,
        files: run.table.files.clone(),
        counters,
        report,
    }
}

/// Runs every stage of the design over one package and counts how far
/// each gets (the same driver as `build`, in analysis mode).
pub fn analyze_package(package: &str, sources: &dyn SourceSet) -> PipelineReport {
    let store = MemoryStore::default();
    let host = Host {
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
                    self.stop(s, &e.what);
                }
                None
            }
        }
    }

    fn stop(&self, s: Stage, what: &str) {
        let span = Span {
            file: FileId::from_raw(u32::MAX),
            lo: 0,
            hi: 0,
        };
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
        let mut uses = Vec::new();
        for &(lo, hi) in &sk.uses {
            let line = text.get(lo as usize..hi as usize).unwrap_or("").trim();
            if let Some(rest) = line.strip_prefix("use ") {
                let path = rest.split(".{").next().unwrap_or(rest).trim();
                let path = match path.strip_prefix("pkg.") {
                    Some(p) => format!("{}.{p}", self.package),
                    None => path.to_owned(),
                };
                uses.push(path);
            }
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
        for (m, module) in self.table.modules.iter().enumerate() {
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

    fn iface_by_path(&self, path: &str) -> Option<Arc<FolderIface>> {
        let f = self.table.folders.iter().find(|x| x.path == path)?;
        self.iface_of(f.id)
    }

    /// `FolderIface(F)` (resolution-and-interfaces.md §4.10).
    fn folder_iface(&self, fi: usize) {
        let folder = &self.table.folders[fi];
        let fid = folder.id;
        let uses: Vec<FolderId> = self
            .graph
            .get()
            .map(|g| g.graph.uses[fi].clone())
            .unwrap_or_default();
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
            self.package_key,
            &folder.path,
            &apis,
            &reach_hashes,
        );
        let used_deep: Vec<Hash128> = uses
            .iter()
            .filter_map(|u| self.iface_of(*u))
            .map(|i| i.deep_hash)
            .collect();
        let names = self.names();
        if let Some(sections) = self.lookup(EntryKind::Iface, key)
            && let Some(blob) = sections.first()
            && let Some(items) = hd_resolve::decode_items(&names, blob)
        {
            let iface = hd_resolve::folder_iface(
                &folder.path,
                items,
                Arc::from(blob.as_slice()),
                &used_deep,
            );
            lock(&self.counters)
                .deep_hashes
                .insert(folder.path.clone(), iface.deep_hash);
            lock(&self.report).ok(Stage::FolderIface);
            let _ = self.iface[fi].set(Some(Arc::new(iface)));
            return;
        }
        let mut items = Vec::new();
        for m in &folder.modules {
            let m = m.idx();
            if !self.parse_of(m).is_ok() {
                self.blocked(Stage::FolderIface);
                let _ = self.iface[fi].set(None);
                return;
            }
            let mut scratch = DiagBuf::default();
            match self.lower_module(m, Stage::FolderIface, &mut scratch) {
                Ok(own) => items.extend(hd_resolve::interface_items(&own)),
                Err(e) => {
                    self.stage::<()>(Stage::FolderIface, Err(e));
                    let _ = self.iface[fi].set(None);
                    return;
                }
            }
        }
        let blob = match hd_resolve::encode_items(&names, &items) {
            Ok(b) => b,
            Err(e) => {
                self.stage::<()>(Stage::FolderIface, Err(e));
                let _ = self.iface[fi].set(None);
                return;
            }
        };
        self.put(EntryKind::Iface, key, &[&blob]);
        let iface = hd_resolve::folder_iface(&folder.path, items, Arc::from(blob), &used_deep);
        {
            let mut c = lock(&self.counters);
            c.ifaces_built.push(folder.path.clone());
            c.deep_hashes.insert(folder.path.clone(), iface.deep_hash);
        }
        lock(&self.report).ok(Stage::FolderIface);
        let _ = self.iface[fi].set(Some(Arc::new(iface)));
    }

    /// Heads, scope and items of one module.
    fn lower_module(&self, m: usize, stage: Stage, diags: &mut DiagBuf) -> StageResult<Vec<Item>> {
        let names = self.names();
        let src = self.src(m);
        let module = &self.table.modules[m].path;
        let heads = hd_resolve::heads(&names, &src, module);
        let (scope, kinds) = hd_resolve::module_scope(
            &names,
            &src,
            &self.package,
            module,
            &heads,
            &|f| self.iface_by_path(f),
            stage,
            diags,
        )?;
        hd_resolve::lower_items(&names, &src, &heads, &scope, &kinds, stage, diags)
    }

    fn header_check(&self, fi: usize) {
        match self.iface.get(fi).and_then(|s| s.get()).cloned().flatten() {
            Some(i) => {
                let r = hd_resolve::header_check(&self.pool, &i.items);
                self.stage(Stage::HeaderCheck, r);
            }
            None => self.blocked(Stage::HeaderCheck),
        }
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

    /// `Coherence`: the overlap check over every interface's impl heads.
    fn coherence(&self) {
        let mut heads = Vec::new();
        for f in 0..self.table.folders.len() {
            let Some(i) = self.iface.get(f).and_then(|s| s.get()).cloned().flatten() else {
                self.blocked(Stage::Coherence);
                return;
            };
            for it in &i.items {
                if let ItemData::Impl {
                    trait_, self_ty, ..
                } = it.data
                {
                    heads.push((trait_, self_ty, it.def));
                }
            }
        }
        heads.sort_by_key(|h| self.names().path_hash(h.2));
        if let Some(pairs) = self.stage(Stage::Coherence, coherence(&self.pool, &heads)) {
            let names = self.names();
            for (a, b) in pairs {
                let span = Span {
                    file: FileId::from_raw(u32::MAX),
                    lo: 0,
                    hi: 0,
                };
                let msg = format!("overlapping-impl: {} and {}", names.path(a), names.path(b));
                lock(&self.diags).error(Code::OverlappingImpl, span, &msg);
            }
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
            ROLE,
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
        let mut diags = DiagBuf::default();
        let items = self.lower_module(mi, Stage::ModulePrep, &mut diags);
        let Some(items) = self.stage(Stage::ModulePrep, items) else {
            self.blocked(Stage::Body);
            self.blocked(Stage::ModuleFinish);
            let _ = self.prep[mi].set(None);
            let _ = self.check[mi].set(None);
            return;
        };
        let _ = self.prep[mi].set(Some(PrepOut { items, diags }));
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
        let Ok((scope, _)) = hd_resolve::module_scope(
            &names,
            &src,
            &self.package,
            &module.path,
            &heads,
            &|f| self.iface_by_path(f),
            Stage::Body,
            &mut DiagBuf::default(),
        ) else {
            self.blocked(Stage::Body);
            let _ = self.body[m].set(None);
            return;
        };
        let universe = self.universes.intern(&closure);
        let solver = SkeletonSolver;
        let cx = BodyCx {
            names,
            src,
            scope: &scope,
            lookup: &lookup,
            impls: &table_refs,
            universe,
            global: &self.memo,
            solver: &solver,
        };
        let mut diags = DiagBuf::default();
        let mut bodies = Vec::new();
        for (def, node) in hd_resolve::body_nodes(&names, &src, &heads) {
            match hd_check::check_fn(&cx, def, node, &mut diags) {
                Ok(b) => bodies.push(b),
                Err(e) => {
                    self.stage::<()>(Stage::Body, Err(e));
                    let _ = self.body[m].set(None);
                    return;
                }
            }
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
        let items = match hd_resolve::encode_items(&names, &prep.items) {
            Ok(x) => x,
            Err(e) => {
                self.stage::<()>(Stage::ModuleFinish, Err(e));
                let _ = self.check[m].set(None);
                return;
            }
        };
        let Some(key) = self.check_key(m) else { return };
        let sections: [&[u8]; 4] = [&dw.bytes, &meta.bytes, &items, &tw.bytes];
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
            Goal::Program { .. } => {
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
                return None;
            };
            if c.has_errors {
                return None;
            }
            let sections = split_sections(&c.entry)?;
            for it in hd_resolve::decode_items(&names, sections.get(2)?)? {
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
    fn collect(&self, sp: &dyn Spawn) {
        let Goal::Program { entry } = &self.goal else {
            return;
        };
        let entry_module = format!("{}.{entry}", self.package);
        let mut modules: Vec<(&str, Hash128)> = Vec::new();
        for (m, module) in self.table.modules.iter().enumerate() {
            let Some(Some(c)) = self.check[m].get() else {
                self.blocked(Stage::Collect);
                return;
            };
            modules.push((module.path.as_str(), c.content));
        }
        let pkey = prog_key(self.toolchain, self.pipeline, &entry_module, &modules);
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
        let names = self.names();
        let root = names.item(&entry_module, "main");
        if !p.bodies.contains_key(&root) {
            let span = Span {
                file: FileId::from_raw(u32::MAX),
                lo: 0,
                hi: 0,
            };
            lock(&self.diags).error(
                Code::MissingEntryPoint,
                span,
                &format!("missing-entry-point: `{entry_module}` has no `fn main`"),
            );
            return;
        }
        let env = Env { run: self, p };
        let Some(collected) = self.stage(
            Stage::Collect,
            hd_mono::collect(&self.pool, &env, &SkeletonSolver, root),
        ) else {
            return;
        };
        let order = collected.table.content_order();
        let mut code_keys = Vec::new();
        let codes: Vec<OnceLock<WasmCode>> = order.iter().map(|_| OnceLock::new()).collect();
        let mut misses = Vec::new();
        for (slot, id) in order.iter().enumerate() {
            let item = collected.table.item[id.idx()];
            let tir = p.bodies.get(&item).map_or(Hash128(0), |b| b.1);
            let ck = code_key(
                self.pipeline,
                collected.table.key[id.idx()],
                tir,
                collected.callee_reps[id.idx()],
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
        let root_key = collected.table.key[0];
        let _ = self.collect.set(CollectOut {
            collected,
            order,
            codes,
            code_keys,
            prog_key: pkey,
            root_key,
        });
        let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
        for slot in misses {
            let e = sp.add(TaskKind::Ext(ExtTask::Emit(u32_of(slot))), &[]);
            sp.edge(e, link);
        }
        sp.release(link);
    }

    fn emit(&self, slot: usize) {
        let (Some(c), Some(p)) = (self.collect.get(), self.program.get()) else {
            return;
        };
        let id = c.order[slot];
        let item = c.collected.table.item[id.idx()];
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
            args,
            ret,
            &c.collected.calls[id.idx()],
        );
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
        for (slot, id) in c.order.iter().enumerate() {
            let Some(code) = c.codes[slot].get() else {
                return;
            };
            codes.push((c.collected.table.key[id.idx()], code.clone()));
        }
        let env = Env { run: self, p };
        let names = self.names();
        let path = |d: DefId| names.path(d);
        let mut structs = Vec::new();
        for d in &c.collected.data {
            let mut fields = Vec::new();
            for t in env.data_fields(d.def()).unwrap_or_default() {
                let vt = match hd_wasm::vt_of(&self.pool, &env, &path, t) {
                    Ok(Some(v)) => v,
                    Ok(None) => hd_wasm::VT::I32,
                    Err(e) => {
                        self.stage::<()>(Stage::Link, Err(e));
                        return;
                    }
                };
                fields.push((vt, t == Ty::BOOL));
            }
            structs.push((names.path(d.def()), fields));
        }
        let imports: Vec<u32> = c.collected.imports.iter().copied().collect();
        let Some(wasm) = self.stage(
            Stage::Link,
            hd_wasm::link(&codes, c.root_key, &structs, &imports),
        ) else {
            return;
        };
        self.put(EntryKind::Link, c.prog_key, &[&wasm]);
        let _ = self.wasm.set(wasm);
    }
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
    fn enum_variants(&self, _: DefId, _: TyList) -> Option<Vec<Vec<Ty>>> {
        None
    }
}

impl ProgramEnv for Env<'_> {
    fn body(&self, def: DefId) -> Option<&Body> {
        self.p.bodies.get(&def).map(|b| &b.0)
    }
    fn bounded(&self, def: DefId) -> Option<Vec<bool>> {
        self.p
            .items
            .get(&def)?
            .sig()
            .map(|s| s.generics.iter().map(|g| g.bound.is_some()).collect())
    }
    fn ret(&self, def: DefId) -> Option<Ty> {
        self.p.items.get(&def)?.sig().map(|s| s.ret)
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
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>> {
        match &self.p.items.get(&def)?.data {
            ItemData::Data(fs) => Some(fs.iter().map(|f| f.ty).collect()),
            _ => None,
        }
    }
    fn path_hash(&self, def: DefId) -> Hash128 {
        self.run.names().path_hash(def)
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
