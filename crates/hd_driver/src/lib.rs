#![forbid(unsafe_code)]
//! `hd_driver`: the one driver (`docs/compiler/design-overview.md` §1, `crates/hd_sched/DESIGN.md` §6.1).
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

use hd_base::{HashMap, HashSet};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

use hd_base::wire::{Reader, Writer};
pub use hd_base::{BuildProfile, EngineTarget, OptLevel};
use hd_base::{DefId, FileId, FolderId, Hash128, ModuleId, NotImplemented, Span, StageResult};
use hd_cache::{
    CacheStore, EntryKind, FileApi, MemoryStore, check_key, code_key, hdr_key, iface_key, init_key,
    parse_key, prog_key, reads_key, toolchain_key,
};
use hd_check::BodyCx;
use hd_check::init::InputBinding;
use hd_check::structure::FactBodies;
use hd_diag::{Code, DiagBuf, Severity};
use hd_intern::{PathTable, ShardedInterner};
use hd_mono::layout::{LayoutEnv, StdKind};
use hd_mono::{Collected, JsonItems, ProgramEnv};
use hd_project::{FolderGraph, MemorySources, ModuleTable, PackageIn, Role, Scope, SourceSet};
use hd_resolve::{FolderIface, Item, ItemData, Items, Lookup, ModOut, Names, Src};
use hd_sched::{ExtTask, SerialOrder, SerialScheduler, Spawn, TaskGraph, TaskId, TaskKind};
use hd_syntax::{HeaderKind, Parse, parse, skim};
use hd_tir::Body;
use hd_types::solver::{
    Declarations, FolderImpls, GlobalMemo, ImplRef, ImplUniverseId, ImplUniverses, ImplView, Impls,
    MemberStep, OWN_TABLE, OwnerMap, SealedTraits, TableSolver, TuplePlans, TypeDecl,
    UniverseImpls, folder_table,
};
use hd_types::{InternPool, Ty, TyData, TyList};
use hd_wasm::Code as WasmCode;

pub mod bench;
mod dbg;
pub mod diagnostics;
mod pipeline;
mod positions;
mod report;
mod skims;
#[cfg(test)]
mod strip_comments;

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

/// The embedded std, made once per process: [`std_sources`], each file's
/// text and the set's hash. Its files are part of the binary, so no later
/// run reads, converts or hashes them again (#49).
struct EmbeddedStd {
    sources: MemorySources,
    texts: HashMap<String, Arc<str>>,
    hash: Hash128,
}

fn embedded_std() -> &'static EmbeddedStd {
    static STD: OnceLock<EmbeddedStd> = OnceLock::new();
    STD.get_or_init(|| {
        let sources = std_sources();
        let texts = sources
            .list()
            .into_iter()
            .map(|e| {
                let b = sources.read(&e.path).unwrap_or_else(|| Arc::from(&b""[..]));
                let text = Arc::from(String::from_utf8_lossy(&b));
                (e.path, text)
            })
            .collect();
        let hash = sources_hash(&sources);
        EmbeddedStd {
            sources,
            texts,
            hash,
        }
    })
}

/// `std.rt`: the hd the compiler roots after an entry point or a test case
/// returns (module.entry.exit-report, module.testing.passes): the status
/// `report()` gives, after an `.Err`'s report on standard error
/// (module.entry.err-stream). The driver picks one by the result type:
/// `entry_status` for any `Termination`, `entry_status_error` for a
/// `Result` whose error type implements `Error` and `entry_status_dyn`
/// for the erased `dyn Error`; the last two print the cause chain
/// (module.entry.err-render-chain), on standard output for an error whose
/// `exit_code` is 0 and on standard error otherwise (module.entry.err-stream).
/// A test case's `Result` goes through `test_status_error` and
/// `test_status_dyn` instead: the same report always on standard error,
/// and status 1 for an `.Err` whose `exit_code` is 0
/// (module.testing.fails). It is compiler-supplied, like
/// `std.core`, so the TS prototype's `lib/std` is unchanged.
pub const RT_SOURCE: &str = "\
# std.rt: the entry and test-case result functions (hd_driver::RT_SOURCE).
use std.process.Termination
use std.error.{Error, report_of}

fn entry_status[T < Termination](result: T) -> u8:
    u8(result.report())

fn entry_status_error[T < Termination, E < Error](result: Result[T, E]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            code := error.exit_code()
            entry_report(report_of(error).to_string(), code)
            u8(code)

fn entry_status_dyn[T < Termination](result: Result[T, dyn Error]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            code := error.exit_code()
            entry_report(report_of(error).to_string(), code)
            u8(code)

fn test_status_error[T < Termination, E < Error](result: Result[T, E]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            entry_write(report_of(error).to_string())
            test_failed(error.exit_code())

fn test_status_dyn[T < Termination](result: Result[T, dyn Error]) -> u8:
    match result:
        .Ok(value) => u8(value.report())
        .Err(error) =>
            entry_write(report_of(error).to_string())
            test_failed(error.exit_code())

fn test_failed(code: i32) -> u8:
    if code == 0: 1 else: u8(code)

fn entry_report(text: string, code: i32) -> void:
    if code == 0:
        entry_write_out(text)
    else:
        entry_write(text)

@intrinsic(\"entry_write_out\")
fn entry_write_out(text: string) -> void:
    panic(\"intrinsic\")

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
/// toolchain key, `crates/hd_cache/DESIGN.md` §5.3).
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

/// A clock that reads zero: the browser and deterministic tests.
pub struct NoClock;

impl Clock for NoClock {
    fn now_ns(&self) -> u64 {
        0
    }
}

/// Which executor runs the graph (`crates/hd_sched/DESIGN.md` §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Executor {
    Serial(SerialOrder),
    /// Needs the `threads` feature.
    #[cfg(feature = "threads")]
    Pool(usize),
}

/// A function body of `Body(m)`: its item and declaration, and the body
/// M1 checked for it with that body's diagnostics.
type BodySlot<'a> = (
    DefId,
    hd_syntax::NodeRef<'a>,
    Option<(StageResult<Body>, DiagBuf)>,
);

/// A function body checked in a forked context: the body, its own
/// diagnostics, and what it added to the module's state.
type ForkedBody = (StageResult<Body>, DiagBuf, hd_check::fork::BodyLeft);

/// What the caller supplies.
pub struct Host<'a> {
    /// Item paths whose checked bodies the output renders (golden tests).
    pub render_tir: &'a [&'a str],
    pub sources: &'a dyn SourceSet,
    pub store: &'a dyn CacheStore,
    pub clock: &'a dyn Clock,
    pub executor: Executor,
}

/// A scenario's held call (issue #56): the suspending function whose
/// polls the build stubs, so the driver owns an unfinished root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioHold {
    /// The suspending function to hold, by its top-level name.
    pub func: String,
    /// Whether the stub leaves the frame flagged active, so the next
    /// same-driver poll reports `suspension-reentrant-poll` (`reentrant-poll`).
    pub active: bool,
}

/// What a run is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Goal {
    /// A program whose root is `fn main` of this module (its path below
    /// the package, as `main` or `app.main`).
    Program { entry: String },
    /// The package's unit-test program (`docs/compiler/engines-and-test-runner.md`
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
    Scenario {
        entry: String,
        /// The suspending function a scenario holds: every poll of its
        /// suspension answers pending without running its body (the
        /// `fixture-runtime-pending-function` directive, or `main` for
        /// `competing-drivers`, whose owned-but-unfinished root is the
        /// whole point, and for `reentrant-poll`, whose frame the next
        /// same-driver poll must find active).
        hold: Option<ScenarioHold>,
    },
    /// Every stage over a package; "not implemented" is counted.
    Analyze,
    /// `Analyze`, with each module's `tests:` blocks checked as test code
    /// into its test-role `check` entry, as a test run checks them
    /// (`hd check --tests`, `cli.check.tests`).
    CheckTests,
    /// The `input`th input of a REPL session (`f1c55020:future-work/compiler/repl-plan.md` §2.2): the root
    /// package's synthetic file `repl#N` ([`hd_project::repl_file`]), a
    /// module outside every package folder, polled as a script. The
    /// sources hold the session's earlier inputs as `repl#K` files, `K`
    /// below `input`, without the inputs that panicked
    /// (`cli.repl.panic.binding`). An input sees `src/lib.hd`'s scope with
    /// each earlier input's top-level names nested in it
    /// (`cli.repl.package.lib`), from the first input that names one of
    /// the library's names on. The run reads and builds only what the
    /// input reaches. With [`Packages::session`], it links as
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

/// One program of a test run (`docs/compiler/engines-and-test-runner.md` §19.1), built on
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
    /// Per file, the files of the modules its `use`s reach, its `tests:`
    /// block's included (from the skims this run made; empty for a file it
    /// did not skim).
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
    /// The engine target whose pipeline built this output
    /// ([`Packages::target`]), as the caller asked for it.
    pub target: EngineTarget,
    /// The optimization level whose pipeline built this output
    /// ([`Packages::level`]).
    pub level: OptLevel,
    /// The backend that built this output ([`Packages::backend`]).
    pub backend: Backend,
    /// The run's toolchain key (`crates/hd_cache/DESIGN.md` §5.3): the compiler build and the
    /// std sources it built with.
    pub toolchain: Hash128,
    /// The content key of the pipeline that built this output
    /// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.3), for its `target` and `level`.
    pub pipeline: Hash128,
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
    /// files and lines (`crates/hd_wasm/DESIGN.md` §15.9), its modules kept in `pool`
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
    /// The text's own skim.
    file: Arc<skims::FileSkim>,
    /// The module paths its library code uses: its folder edges and
    /// `closure(m)`.
    uses: Vec<String>,
    /// The module paths only its `tests:` block uses, which make no folder
    /// edge (`module.cycle.test-code`) and widen only the test role's
    /// closure ([`Run::module_closure`]).
    test_uses: Vec<String>,
}

struct GraphOut {
    graph: FolderGraph,
    package_result: TaskId,
}

/// A module's own items after `ModulePrep` (a check miss).
struct PrepOut {
    items: Vec<Item>,
    diags: DiagBuf,
    /// A use of the module names nothing: `HeaderCheck` reports it, so it
    /// may not be in the run's diagnostics yet when a body asks
    /// ([`Run::file_has_errors`]) or the module finishes.
    use_errors: bool,
}

/// A folder's stage-A diagnostics, with the module each belongs to, the
/// severity each was reported with and its fix-its (a cache hit re-emits
/// them).
type IfaceDiags = Vec<(
    usize,
    Code,
    Severity,
    hd_resolve::anchor::Anchor,
    String,
    Vec<IfaceFix>,
)>;

/// A stage-A fix-it as an interface entry keeps it: no byte offsets, as
/// for its diagnostic. Each edit is its module, an anchor of the token at
/// its start, its start and end as byte distances from that token's start
/// (wrapping, so whitespace before the token makes them negative), and
/// its text.
struct IfaceFix {
    title: String,
    exact: bool,
    edits: Vec<(usize, hd_resolve::anchor::Anchor, u32, u32, String)>,
}

/// A module's TIR and body diagnostics after `Body(m)`, the hidden
/// methods its derivations add (`crates/hd_tir/DESIGN.md` §4.13.9), and the
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
/// implementations' methods and their bodies (`crates/hd_mono/DESIGN.md` §12.3).
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
    bodies: Bodies,
    /// Where each impl's row lives: its folder's table.
    impl_rows: HashMap<DefId, ImplRef>,
    /// The program's impl universe: every folder of the program. A build
    /// sees the whole program, so the universe is not narrowed to a
    /// closure (`crates/hd_check/DESIGN.md` (trait solver) §3.2).
    universe: ImplUniverseId,
    extra: Arc<UniverseImpls>,
    /// A test program's impls that no folder table holds: those of the
    /// `tests:` blocks, which only the test role checks
    /// (`names.tests.module-items`, `module.test.code`). The program view
    /// reads them as its own table, so a memo entry that probed one is
    /// never shared with a library-role context, nor one of theirs used
    /// here. Empty outside a test run.
    test_impls: hd_types::solver::ImplTable,
    /// The program's self-recursive enums (`hd_mono::layout::recursive_enums`),
    /// found once on the first layout that asks.
    recursive_enums: OnceLock<hd_base::HashSet<DefId>>,
}

/// The program's TIR bodies ([`Run::body_of`]), by item. Each is decoded
/// on first use: collection reaches a small part of the program (most of
/// std is never called), and a relink after an edit would otherwise
/// decode every body of every module again (#49). An earlier REPL input's
/// bodies are read the same way, so a build does not decode the session.
struct Bodies {
    /// By the item's path hash, which a `check` entry stores with each
    /// body: indexing interns nothing.
    all: HashMap<Hash128, LazyBody>,
    /// Set when a body's bytes did not decode: `Collect` then fails as a
    /// module whose entry did not decode.
    bad: std::sync::atomic::AtomicBool,
}

/// One body: where its wire bytes are, and the body once decoded.
struct LazyBody {
    /// The module, its `check` entry, and the body's bytes in it; `None`
    /// for a body built in this run (a module cycle's init run).
    wire: Option<(usize, Arc<[u8]>, std::ops::Range<usize>)>,
    body: OnceLock<Option<(Body, Hash128)>>,
}

impl LazyBody {
    fn ready(body: Body, tir: Hash128) -> Self {
        LazyBody {
            wire: None,
            body: OnceLock::from(Some((body, tir))),
        }
    }
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
    /// The missed slots by folder group, the folder of each instance's
    /// item (`crates/hd_sched/DESIGN.md` §6.1): one `Emit` task each.
    groups: Vec<Vec<usize>>,
    /// Each emitted group's `codepack`, written once the group's code is
    /// all in.
    packs: Vec<CodePack>,
    /// The package's `packhint` as this run read it, and the packs this
    /// run wrote, which `Link` puts first in it.
    hint: PackHint,
    written: Mutex<Vec<(String, Hash128)>>,
    prog_key: Hash128,
    roots: Roots,
    /// Nominal types' layouts, shared by the program's emissions so each
    /// recursion group is laid out once.
    layouts: hd_wasm::layout::Layouts,
}

/// A folder group's `codepack` (`docs/compiler/data-structures.md`
/// §3.22): its cached slots in content order, the pack's key, and the
/// folder's path, its name in the package's `packhint`.
struct CodePack {
    slots: Vec<usize>,
    key: Hash128,
    folder: String,
}

/// How many of a folder's recent packs the package's `packhint` keeps.
const HINTED_PACKS: usize = 4;

/// A package's `packhint`: each folder's recent packs, newest first.
type PackHint = BTreeMap<String, Vec<Hash128>>;

/// Fills `codes` from a `codepack` entry's `sections`, its code keys and
/// their code: each of `slots` without code whose code key it holds.
fn fill_from(
    sections: &[Vec<u8>],
    slots: &[usize],
    code_keys: &[Hash128],
    codes: &[OnceLock<WasmCode>],
) {
    let [head, body] = sections else {
        return;
    };
    let mut r = hd_base::wire::Reader::new(head);
    let keys: Vec<Hash128> = (0..r.count()).map(|_| r.hash()).collect();
    let Some(stored) = r.ok().then(|| hd_wasm::decode_codes(body)).flatten() else {
        return;
    };
    if stored.len() != keys.len() {
        return;
    }
    let mut at: hd_base::HashMap<Hash128, WasmCode> = keys.into_iter().zip(stored).collect();
    for &s in slots {
        if codes[s].get().is_none()
            && let Some(c) = at.remove(&code_keys[s])
        {
            let _ = codes[s].set(c);
        }
    }
}

/// Module `m`'s skim: its text's skim `file`, with the module paths it
/// uses, `session` among them, the session modules of a REPL input
/// ([`session_uses`]), and apart the paths its `tests:` block uses.
fn skim_module(
    table: &ModuleTable,
    m: usize,
    text: &str,
    file: Arc<skims::FileSkim>,
    session: Vec<String>,
) -> SkimOut {
    let module = &table.modules[m].path;
    let roots = table.use_roots(ModuleId::from_raw(u32_of(m)));
    let mut uses = written_uses(text, &file.use_lines, &roots);
    // A `tests:` block's uses, of this package or another, such as a dev
    // dependency (`module.test.dev-dependency.in-tests`): the block is
    // test code, so they make no folder edge (`module.cycle.test-code`).
    // The block is checked with its module in the test role, whose
    // closure they join.
    let mut test_uses = written_test_uses(&file.test_uses, &roots);
    // Test code has the prelude's `std.testing` row
    // (`module.prelude.test-only`), and its test cases run bodies of
    // `std.testing` (`case_timeout` for a `timeout`), so that folder
    // joins its closure; std's own test code makes no such edge. A test
    // module is test code as a whole, in a folder no library code uses;
    // a `tests:` block is test code only in the test role.
    let package = table.modules[m].package;
    let testing_elsewhere = table
        .module_of_use("std.testing")
        .is_some_and(|t| table.modules[t.idx()].package != package);
    if testing_elsewhere {
        if table.modules[m].role == hd_project::Role::Test {
            uses.push("std.testing".to_owned());
        } else if file.has_tests_block {
            test_uses.push("std.testing".to_owned());
        }
    }
    // A REPL input sees `src/lib.hd`, once its session holds it, and the
    // earlier inputs through its session scope, so their folders join its
    // closure. It uses the library and the input just before it: that
    // input uses the one before, so the closure, and every key and order
    // over it, is the same as with a use of each earlier input, at one
    // edge per input instead of one per pair of inputs.
    uses.extend(session);
    // The prelude's fixed uses (`module.prelude.fixed-uses`).
    if module != "std.core" {
        uses.extend(hd_resolve::prelude_modules().into_iter().map(str::to_owned));
    }
    SkimOut {
        file,
        uses,
        test_uses,
    }
}

/// The roots a program's exports poll.
enum Roots {
    /// `main`, its report and the inits it reaches.
    Main {
        def: DefId,
        key: Hash128,
        /// The entry module's own init bodies, the inits that may
        /// `block_on`: its whole init, or its runs in a module cycle.
        entry_inits: Vec<Hash128>,
        report: Option<Hash128>,
    },
    /// A script: its first own init body, which runs with its entry
    /// providers after the inits before it. `own` holds every own body:
    /// its whole init, or its runs in a module cycle. The inits after the
    /// first then run in order, the own ones with the entry providers.
    Script {
        def: DefId,
        key: Hash128,
        own: Vec<Hash128>,
        /// The report function of a result that `?` makes
        /// (`module.init.script-try`).
        report: Option<Hash128>,
    },
    /// The inputs of a joined REPL build ([`Packages::inputs_from`]), in
    /// order, each a script.
    Inputs(Vec<JoinedInput>),
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
        /// The entry module's own init bodies, the inits that may
        /// `block_on`: its whole init, or its runs in a module cycle.
        entry_inits: Vec<Hash128>,
        report: Option<Hash128>,
        /// The entry module's top-level cleanup function, called by
        /// `hd.scenario.cleanup`; `None` when it declares none.
        cleanup: Option<Hash128>,
    },
}

/// Whether this build of the compiler builds joined REPL inputs
/// ([`Packages::inputs_from`]). The web compiler does not: the JS host's
/// session keeps one module per input, so it leaves that code out.
const JOINED_INPUTS: bool = cfg!(not(target_arch = "wasm32"));

/// One input of a joined REPL build: its number, its first own init body
/// and that body's instance, every init it runs, in order, and its own
/// ones (as [`Roots::Script`]'s `own`).
struct JoinedInput {
    number: u32,
    def: DefId,
    key: Hash128,
    inits: Vec<Hash128>,
    own: Vec<Hash128>,
}

/// A module's `tests:` blocks after checking: the case items and the
/// registrations.
type TestsOut = (Vec<Item>, Vec<hd_check::tests::TestReg>);

/// A user template of a folder's interface: its declaring module's path
/// and its trait, `None` for a tuple template.
type FolderTemplate = (String, Option<DefId>);

/// One tuple template's member steps, computed once (`crates/hd_check/DESIGN.md` (trait solver)
/// §3.9); `None` for a template whose body does not check.
type PlanCell = Arc<OnceLock<Option<Arc<[MemberStep]>>>>;

/// `src/lib.hd` lowered for a REPL session: its scope, its kinds, and its
/// private items, which no interface carries.
struct LibScope {
    scope: hd_resolve::ModuleScope,
    kinds: hd_resolve::Kinds,
    private: Vec<Item>,
}

/// The REPL input modules of a run ([`Goal::ReplInput`]), and which of
/// them see `src/lib.hd`.
struct ReplInputs {
    /// (number, module), by number.
    sorted: Vec<(u32, usize)>,
    /// Each module's number, if it is an input.
    numbers: Vec<Option<u32>>,
    /// The input this run builds, by module; `None` for the prelude, which
    /// is no input of a package ([`build_repl_prelude`]), and every other
    /// goal.
    input: Option<usize>,
    /// The numbers of the inputs this run builds, from the first to the
    /// goal's input ([`Packages::inputs_from`]); `None` for every other
    /// goal.
    built: Option<(u32, u32)>,
    /// Whether the run builds them as one module of several inputs, with
    /// numbered entry exports ([`Packages::inputs_from`]).
    joined: bool,
    /// The root package's `src/lib.hd`, if it has one.
    lib: Option<usize>,
    /// Per module: whether it is an input whose session holds `src/lib.hd`
    /// (`cli.repl.package.lib`). A session loads the library lazily: from
    /// the first input that names one of its names on, so a session whose
    /// library does not build still evaluates an input that never names it
    /// ([`ReplInputs::new`]).
    with_lib: Vec<bool>,
}

impl ReplInputs {
    /// The REPL inputs among `table`'s modules, for a run of `goal`. An
    /// input's session holds `src/lib.hd` when an earlier input's does, or
    /// when one of the input's identifiers is a name the library declares
    /// ([`lib_names`]). `text` reads a module's text; only the library's
    /// and the inputs' are read, and only while no input holds the library.
    fn new(
        table: &ModuleTable,
        goal: &Goal,
        from: Option<u32>,
        text: &mut dyn FnMut(usize) -> Arc<str>,
    ) -> Self {
        let n = table.modules.len();
        let numbers: Vec<Option<u32>> = (0..n)
            .map(|k| {
                (table.modules[k].package == 0)
                    .then(|| hd_project::repl_input(&table.files[k]))
                    .flatten()
            })
            .collect();
        let mut sorted: Vec<(u32, usize)> = numbers
            .iter()
            .enumerate()
            .filter_map(|(k, n)| Some(((*n)?, k)))
            .collect();
        sorted.sort_unstable();
        let built = match goal {
            Goal::ReplInput { input } => Some(*input),
            _ => None,
        };
        let input = built.and_then(|b| sorted.iter().find(|(n, _)| *n == b).map(|(_, k)| *k));
        let lib = table
            .sources
            .iter()
            .position(|(p, file)| *p == 0 && file == "src/lib.hd");
        let mut with_lib = vec![false; n];
        if let (Some(lib), Some(built)) = (lib, built) {
            let mut names: Option<HashSet<String>> = None;
            let mut joined = false;
            for &(_, k) in sorted.iter().take_while(|(number, _)| *number <= built) {
                if !joined {
                    let names = names.get_or_insert_with(|| lib_names(&text(lib)));
                    joined = names_any(&text(k), names);
                }
                with_lib[k] = joined;
            }
        }
        ReplInputs {
            sorted,
            numbers,
            input,
            built: built.map(|b| (from.unwrap_or(b), b)),
            joined: JOINED_INPUTS && built.is_some() && from.is_some(),
            lib,
            with_lib,
        }
    }

    /// Whether the run builds input `number` ([`ReplInputs::built`]).
    fn builds(&self, number: u32) -> bool {
        self.built
            .is_some_and(|(first, last)| (first..=last).contains(&number))
    }
}

/// The names that make an input need `src/lib.hd`: the library's
/// top-level names, and the name of each of its functions and methods, as
/// a method of a library trait implemented for a std type is called by
/// its name alone (`names.member.trait-available`). From its tokens, so
/// a library that does not parse or check still has them.
fn lib_names(text: &str) -> HashSet<String> {
    hd_syntax::top_level_names(text)
        .into_iter()
        .chain(hd_syntax::function_names(text))
        .collect()
}

/// Whether an identifier of `text` is one of `names`.
fn names_any(text: &str, names: &HashSet<String>) -> bool {
    let lexed = hd_syntax::lex(text.as_bytes());
    let tokens = &lexed.tokens;
    (0..tokens.len()).any(|i| {
        tokens.kind[i] == hd_syntax::TokenKind::Ident
            && text
                .get(tokens.start[i] as usize..tokens.end[i] as usize)
                .is_some_and(|w| names.contains(w))
    })
}

/// The module texts of a REPL input's build: the modules the input
/// reaches from its own module, through its uses and its session's
/// (`cli.repl.package.lib`, `cli.repl.uses.other`), and each module of
/// another package's folder it reaches. Every other text is empty and
/// never read, so a root-package folder the input reaches holds only its
/// reached modules for the run ([`Run::builds_module`]): an implementation
/// in a module that no reached module uses is not in the session, as one
/// in a folder outside the input's closure is not. Also each reached
/// module's skim, which the walk computes anyway. `None` when the run
/// builds no input of `repl`.
fn repl_texts(
    table: &ModuleTable,
    repl: &ReplInputs,
    text: &mut dyn FnMut(usize) -> Arc<str>,
    files: &skims::FileSkims<'_>,
) -> Option<ModuleTexts> {
    let start = repl.input?;
    let n = table.modules.len();
    let mut texts: Vec<Arc<str>> = vec![Arc::from(""); n];
    let mut skims: Vec<Option<SkimOut>> = (0..n).map(|_| None).collect();
    let mut stack = vec![start];
    while let Some(m) = stack.pop() {
        if skims[m].is_some() {
            continue;
        }
        texts[m] = text(m);
        let file = files.get(table, m, &texts[m]);
        let s = skim_module(table, m, &texts[m], file, session_uses(table, repl, m));
        for u in &s.uses {
            if let Some(id) = table.module_of_use(u) {
                stack.push(id.idx());
            }
        }
        // Another package's folder is read whole: std and a dependency
        // rely on the impls of every module of a folder they reach.
        let module = &table.modules[m];
        if module.package != 0 {
            let folder = &table.folders[module.folder.idx()];
            stack.extend(folder.modules.iter().map(|k| k.idx()));
        }
        skims[m] = Some(s);
    }
    let reached = Some(skims.iter().map(Option::is_some).collect());
    Some(ModuleTexts {
        texts,
        skims,
        reached,
    })
}

/// The module texts a run reads, by module: an unread module's is empty.
struct ModuleTexts {
    texts: Vec<Arc<str>>,
    /// The skims computed while reading, by module.
    skims: Vec<Option<SkimOut>>,
    /// Per module, whether it was read; `None` when every module was.
    reached: Option<Vec<bool>>,
}

/// The modules REPL input `m` uses for its session scope: `src/lib.hd`,
/// when its session holds it, and the input just before it. None for any
/// other module.
fn session_uses(table: &ModuleTable, repl: &ReplInputs, m: usize) -> Vec<String> {
    let Some(number) = repl.numbers[m] else {
        return Vec::new();
    };
    let at = repl.sorted.partition_point(|(k, _)| *k < number);
    let last = at.checked_sub(1).map(|i| repl.sorted[i].1);
    repl.lib
        .filter(|_| repl.with_lib[m])
        .into_iter()
        .chain(last)
        .map(|k| table.modules[k].path.clone())
        .collect()
}

struct Run<'a> {
    host: &'a Host<'a>,
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
    /// Which folder declares what (`crates/hd_check/DESIGN.md` (trait solver) §3.2), built on first use.
    owners: OnceLock<OwnerMap>,
    /// Per folder: its impl table and `arg_impls` section, built once per
    /// run from its interface on first use.
    folder_impls: Vec<OnceLock<Option<Arc<FolderImpls>>>>,
    toolchain: Hash128,
    package_key: Hash128,
    pipeline: Hash128,
    /// The pipeline's body-unit passes, which collection applies.
    planner: hd_opt::planner::PipelinePlanner,
    /// The target's lowering profile, which emission reads
    /// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §4); its hash is part of `pipeline`.
    lowering: hd_wasm::Lowering,
    /// The build profile emitted code follows ([`Packages::build_profile`]).
    build_profile: BuildProfile,
    /// The code generator ([`Packages::backend`]).
    backend: Backend,
    /// The root package's per-body fuel, when a test sets it
    /// ([`Packages::body_fuel`]).
    body_fuel: Option<u64>,
    /// Per package, in the table's order: whether it is fetched, so not
    /// the user's own code (`module.dbg.own-code`).
    fetched: Vec<bool>,
    /// Each module's text skim, std's from the toolchain's `depfiles` entry.
    files: skims::FileSkims<'a>,
    skim: Vec<OnceLock<SkimOut>>,
    parse: Vec<OnceLock<Parse>>,
    graph: OnceLock<GraphOut>,
    iface: Vec<OnceLock<Option<Arc<FolderIface>>>>,
    prep: Vec<OnceLock<Option<PrepOut>>>,
    /// Per module that declares a derivation template: its items and scope,
    /// lowered once per run for the opt-ins that instantiate it.
    template_mods: Vec<OnceLock<Option<ModOut>>>,
    /// Per folder: the templates outside std that its interface holds, as
    /// (declaring module path, trait), found on first use; the trait is
    /// `None` for a tuple template.
    folder_templates: Vec<OnceLock<Vec<FolderTemplate>>>,
    /// Per tuple template: its member steps (`crates/hd_check/DESIGN.md` (trait solver) §3.9),
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
    /// The package's `graph` entry this run fills on a miss.
    graph_parts: OnceLock<GraphParts>,
    program: OnceLock<ProgramTables>,
    collect: OnceLock<CollectOut>,
    wasm: OnceLock<Vec<u8>>,
    /// The session this run links one more module of ([`Packages::session`]).
    session: Option<&'a Session>,
    /// What this run's module adds to that session ([`Output::session_added`]).
    session_added: OnceLock<Session>,
    /// The entries of each REPL input this run builds, by number, as it
    /// reads or writes them, for [`Session::inputs`].
    input_entries: Mutex<BTreeMap<u32, InputEntries>>,
    /// Per folder: whether it is an earlier REPL input's, read from the
    /// session ([`Run::read_kept`]), so it has no task.
    kept: OnceLock<Vec<bool>>,
    /// `src/lib.hd`'s scope, kinds and items, lowered once for the REPL
    /// inputs' session scopes (`cli.repl.package.lib`).
    lib_scope: OnceLock<Option<LibScope>>,
    /// Per REPL input: the session scope its own scope is nested in.
    session_scopes: Vec<OnceLock<Arc<hd_resolve::SessionScope>>>,
    /// The REPL input modules of the root package.
    repl_inputs: ReplInputs,
    /// For a REPL input's build, per module: whether the run read it, as
    /// the input reaches it ([`repl_texts`]); `None` for every other run,
    /// which reads them all.
    reached: Option<Vec<bool>>,
    /// The files a check is narrowed to ([`Packages::check_roots`]).
    check_roots: Option<Vec<String>>,
    /// For a narrowed check, once the skims are in: per module whether a
    /// check root reaches it, and the folders of those modules' closures
    /// ([`Run::builds`], [`Run::builds_module`]).
    check_scope: OnceLock<Option<(Vec<bool>, hd_project::FolderSet)>>,
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

/// The code generator a `Goal::Program` links with (`cli.backend.select`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// The Wasm GC backend (`hd_wasm`), the default.
    #[default]
    WasmGc,
    /// The linear-memory backend (`hd_wasm_linear`,
    /// `crates/hd_wasm/DESIGN.md` §23), which links the whole program at
    /// once from collection's output and caches no code per instance.
    Linear,
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
    /// The engine the build is tuned for (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5). With
    /// `level` it selects the pipeline (`hd_opt::passes::pipeline`) and the
    /// lowering (`hd_wasm::Lowering::of`), whose content, not the target's
    /// name, is part of every code key. The default is the default engine's
    /// target, `V8` (Node).
    pub target: EngineTarget,
    /// Whether the optimized pipeline runs instead of the dev one
    /// (`cli.profile.pipeline.one`). `--release` selects it for a program,
    /// and `hd test --release` for a test run, whose build profile stays
    /// checked.
    pub level: OptLevel,
    /// Whether the output holds each root-package module's scope
    /// ([`Output::scopes`]), which `hd doc` resolves its links in.
    pub doc_scopes: bool,
    /// For `Goal::Analyze` and `Goal::CheckTests`: the files whose modules
    /// are checked, with every module they use, deeply, and the folders of
    /// those modules' closures (`hd check FILE`, `crates/hd_cli/DESIGN.md`
    /// §7.2). `None` checks the whole package.
    pub check_roots: Option<Vec<String>>,
    /// For a `Goal::Program`: build one more module of this session
    /// instead of a closed program (`f1c55020:future-work/compiler/repl-plan.md` §2.2). Collection stops at
    /// the instances earlier modules export and leaves out the inits they
    /// ran; the module links in `hd_wasm::LinkMode::Session`. Its link is
    /// not cached, since it depends on the session, except the prelude's
    /// ([`build_repl_prelude`]), which every session starts with.
    pub session: Option<&'a Session>,
    /// For a `Goal::ReplInput { input }` with a session: build every input
    /// from this number to `input` as one module, for a rebuild that holds
    /// many inputs in one instance (`docs/future-work/live-execution.md`
    /// §5.4). The sources hold each one's `repl#K` file. Input `K`'s top
    /// level runs from the exports `hd.init.K` and `hd.poll.K`. `None`
    /// builds `input` alone, whose exports are `hd.init` and `hd.poll`.
    pub inputs_from: Option<u32>,
    /// A test-only hook: the fuel of each body of the root package's
    /// modules, and of each of their items' header goals, smaller than
    /// `Fuel::BODY_DEFAULT`, so a small program reaches `item-too-complex`
    /// (`fn.limit.complexity.error`) fast. It is part of those modules'
    /// check keys and of their folders' header-check keys. `None` keeps
    /// the default; every other package always does.
    pub body_fuel: Option<u64>,
    /// The code generator ([`Backend`]). Its name is part of the link's
    /// key, so the two backends' modules never share a cache entry.
    pub backend: Backend,
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
    /// Each REPL input's entries, by number. The build of a later input
    /// reads an earlier input here instead of computing its keys over the
    /// inputs before it and reading the store, so it does not redo the
    /// session's earlier inputs (`f1c55020:future-work/compiler/repl-plan.md` §4).
    inputs: BTreeMap<u32, InputEntries>,
}

/// The sections of a REPL input's `iface`, header `graph` and `check`
/// entries, as its build read or wrote them ([`Session::inputs`]). An
/// input's entries never change in its session: its text, `src/lib.hd`
/// and the inputs before it are fixed until a rebuild starts a new one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InputEntries {
    iface: Option<Vec<Vec<u8>>>,
    /// The other folders its interface names, with their deep hashes,
    /// which are fixed for the session too.
    mentions: Option<Vec<(String, Hash128)>>,
    header: Option<Vec<Vec<u8>>>,
    check: Option<Vec<Vec<u8>>>,
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
            inputs: BTreeMap::new(),
        }
    }

    /// Adds what a module that ran adds ([`Output::session_added`]); the
    /// next module is not the first.
    pub fn add(&mut self, added: Session) {
        self.externs.first = false;
        self.externs.funcs.extend(added.externs.funcs);
        self.externs.globals.extend(added.externs.globals);
        self.inits_run.extend(added.inits_run);
        self.inputs.extend(added.inputs);
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
        (r.ok() && r.at_end()).then_some(Session {
            externs,
            inits_run,
            inputs: BTreeMap::new(),
        })
    }
}

/// The REPL session's prelude (`f1c55020:future-work/compiler/repl-plan.md` §2.3, job C8): input 0, the
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
/// `profile` whose modules are tuned for `target` (a session builds in
/// the dev pipeline), or reads it from `host.store`, where it is cached
/// per toolchain and pipeline. It owns the session's memory and runtime globals and holds
/// the instances that showing a value needs (`cli.repl.value`), so small
/// inputs stay small. A session that starts runs it first, then adds its
/// [`Output::session_added`] to [`Session::start`], and builds its inputs,
/// numbered from 1, over that session. `host.sources` is not read.
#[must_use]
pub fn build_repl_prelude(host: &Host<'_>, profile: BuildProfile, target: EngineTarget) -> Output {
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
        target,
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
    // The embedded std, or the root package's own when it is std.
    let embedded = (package != "std").then(embedded_std);
    let root_std;
    let std: &MemorySources = if let Some(e) = embedded {
        &e.sources
    } else {
        root_std = with_core(host.sources);
        &root_std
    };
    let std_in = PackageIn {
        name: "std",
        identity: "std",
        sources: std,
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
    let mut read: Vec<Option<Arc<str>>> = vec![None; table.modules.len()];
    let mut text = |m: usize| -> Arc<str> {
        let found = read[m].get_or_insert_with(|| {
            let (pi, p) = &table.sources[m];
            let pi = usize::from(*pi);
            if let Some(t) = embedded
                .filter(|_| pi + 1 == ins.len())
                .and_then(|e| e.texts.get(p))
            {
                return Arc::clone(t);
            }
            let set = ins[pi].sources;
            Arc::from(
                set.read(p)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            )
        });
        Arc::clone(found)
    };
    let std_hash = embedded.map_or_else(|| sources_hash(std), |e| e.hash);
    let toolchain = toolchain_key(
        COMPILER,
        TARGET,
        &format!("a1=bounded;std={:032x}", std_hash.0),
    );
    // Std's skims come from the toolchain's entry, unless std is the root
    // package, whose files are the user's.
    let files = skims::FileSkims::new(
        (package != "std").then(|| skims::StdSkims::new(host.store, toolchain)),
        table.modules.len(),
    );
    let repl_inputs = ReplInputs::new(&table, goal, packages.inputs_from, &mut text);
    // A REPL input reads only the modules it reaches; every other run reads
    // them all.
    let ModuleTexts {
        texts,
        skims,
        reached,
    } = repl_texts(&table, &repl_inputs, &mut text, &files).unwrap_or_else(|| {
        let n = table.modules.len();
        ModuleTexts {
            texts: (0..n).map(&mut text).collect(),
            skims: (0..n).map(|_| None).collect(),
            reached: None,
        }
    });
    let folder_cycles = recover_folder_cycles(&mut table, &texts, &files);
    let n = table.modules.len();
    let nf = table.folders.len();
    let pipeline = pipeline::key(packages.target, packages.level);
    let planner = hd_opt::planner::PipelinePlanner::new(hd_opt::passes::pipeline(
        packages.target,
        packages.level,
    ));
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
        toolchain,
        package_key: root_package_key(package, packages.profile.as_deref()),
        profile,
        pipeline,
        planner,
        lowering: hd_wasm::Lowering::of(packages.target),
        build_profile: match goal {
            Goal::Program { .. } | Goal::ReplInput { .. } => packages.build_profile,
            Goal::Tests { .. } | Goal::Scenario { .. } | Goal::Analyze | Goal::CheckTests => {
                BuildProfile::Checked
            }
        },
        body_fuel: packages.body_fuel,
        backend: packages.backend,
        fetched: std::iter::once(false)
            .chain(packages.deps.iter().map(|d| d.fetched))
            .chain(std::iter::once(false))
            .collect(),
        files,
        skim: skims
            .into_iter()
            .map(|s| s.map_or_else(OnceLock::new, OnceLock::from))
            .collect(),
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
        graph_parts: OnceLock::new(),
        program: OnceLock::new(),
        collect: OnceLock::new(),
        wasm: OnceLock::new(),
        session: packages.session,
        session_added: OnceLock::new(),
        input_entries: Mutex::default(),
        kept: OnceLock::new(),
        lib_scope: OnceLock::new(),
        session_scopes: (0..n).map(|_| OnceLock::new()).collect(),
        repl_inputs,
        reached,
        check_roots: packages.check_roots.clone(),
        check_scope: OnceLock::new(),
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
    for cycle in &folder_cycles {
        let mut diags = lock(&run.diags);
        diags.error(Code::FolderCycle, cycle.span, &cycle.message);
        if let Some((from, to)) = &cycle.fix {
            diags.add_fix(
                &format!("move {from} to {to}"),
                hd_diag::FixSafety::Suggestion,
                &[],
            );
        }
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
        // A task graph that leaves tasks behind is a scheduling bug.
        if left > 0 {
            run.stop(
                Stage::PackageResult,
                &NotImplemented::bug(
                    Stage::PackageResult,
                    format!("{left} tasks never became ready"),
                ),
            );
        }
    }
    if let Some(hit) = run.files.finish(&run.table) {
        let mut c = lock(&run.counters);
        let kind = EntryKind::Depfiles.as_str();
        *if hit {
            c.hits.entry(kind)
        } else {
            c.misses.entry(kind)
        }
        .or_default() += 1;
    }
    let mut counters = lock(&run.counters).clone();
    counters.body_passes = run.planner.pass_runs();
    // The interface item records this run decoded from blobs (#213).
    counters.iface_records = run
        .iface
        .iter()
        .filter_map(|i| i.get().cloned().flatten())
        .map(|i| i.items.decoded())
        .sum();
    counters.iface_other_records = run
        .iface
        .iter()
        .filter_map(|i| i.get().cloned().flatten())
        .map(|i| i.items.decoded_other())
        .sum();
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
                    .chain(&s.test_uses)
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
        target: packages.target,
        level: packages.level,
        backend: packages.backend,
        toolchain: run.toolchain,
        pipeline: run.pipeline,
    }
}

/// Folder-cycle recovery (`crates/hd_resolve/DESIGN.md` §4.8 rule 5): the
/// folders of a cycle in the root package are merged into one folder, so
/// they resolve together and their uses do not cascade; merging repeats
/// until the folder graph is acyclic.
///
/// A folder cycle as its diagnostic shows it
/// (`module.cycle.diagnostic.*`).
struct FolderCycle {
    /// The message: the shortest loop, each edge's `use`, then the
    /// tangle size and the fix.
    message: String,
    /// The first edge's `use` line.
    span: Span,
    /// A single-file folder on the loop to move beside its own `mod.hd`,
    /// as `(from, to)` files.
    fix: Option<(String, String)>,
}

/// The folder cycles' diagnostics, recovering by merging as before: one
/// error per cycle of the first graph, each with its shortest loop, its
/// edges' uses, its tangle size and its fix
/// (`module.cycle.diagnostic.*`).
fn recover_folder_cycles(
    table: &mut ModuleTable,
    texts: &[Arc<str>],
    files: &skims::FileSkims<'_>,
) -> Vec<FolderCycle> {
    let module_uses: Vec<Vec<(String, u32, u32)>> = (0..table.modules.len())
        .map(|m| {
            if table.modules[m].package != 0 {
                return Vec::new();
            }
            let roots = table.use_roots(ModuleId::from_raw(u32_of(m)));
            written_use_lines(&texts[m], &files.get(table, m, &texts[m]).use_lines, &roots)
        })
        .collect();
    let uses: Vec<Vec<String>> = module_uses
        .iter()
        .map(|v| v.iter().map(|(p, _, _)| p.clone()).collect())
        .collect();
    let mut out: Option<Vec<FolderCycle>> = None;
    loop {
        let graph = FolderGraph::build(table, &uses);
        let cycles: Vec<Vec<FolderId>> = graph
            .cycles
            .iter()
            .filter(|c| c.iter().all(|f| table.folders[f.idx()].package == 0))
            .cloned()
            .collect();
        if cycles.is_empty() {
            return out.unwrap_or_default();
        }
        if out.is_none() {
            out = Some(describe_cycles(table, texts, &module_uses, &cycles));
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

/// The module paths a `tests:` block's `use` lines name (`test_uses`, as
/// written), their roots replaced as test code resolves them, dev
/// dependencies included.
fn written_test_uses(paths: &[String], roots: &hd_project::UseRoots) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| {
            let segs: Vec<&str> = path.split('.').collect();
            roots.absolute_in_tests(&segs).ok().map(|abs| abs.join("."))
        })
        .collect()
}

/// The module paths a file's `use` lines name with their line spans, as
/// `written_uses`, for the folder edge each use makes.
fn written_use_lines(
    text: &str,
    lines: &[(u32, u32)],
    roots: &hd_project::UseRoots,
) -> Vec<(String, u32, u32)> {
    let mut uses = Vec::new();
    for &(lo, hi) in lines {
        let line = text.get(lo as usize..hi as usize).unwrap_or("").trim();
        let rest = line.strip_prefix("pub ").unwrap_or(line);
        if let Some(rest) = rest.strip_prefix("use ") {
            let path = rest.split(".{").next().unwrap_or(rest);
            let path = path.split(" as ").next().unwrap_or(path).trim();
            let segs: Vec<&str> = path.split('.').collect();
            if let Ok(abs) = roots.absolute(&segs) {
                uses.push((abs.join("."), lo, hi));
            }
        }
    }
    uses
}

/// The 1-based line and character column of byte `at` in `text`.
fn line_col(text: &str, at: u32) -> (usize, usize) {
    let at = (at as usize).min(text.len());
    let line = text[..at].bytes().filter(|b| *b == b'\n').count() + 1;
    let col = text[..at]
        .rsplit('\n')
        .next()
        .map_or(0, |s| s.chars().count())
        + 1;
    (line, col)
}

/// The folder as cycle diagnostics show it: its `mod.hd` directory, or
/// the `x` of a lone `x.hd` (`module.cycle.diagnostic.loop`).
fn folder_display(table: &ModuleTable, f: usize) -> String {
    let mut files: Vec<&str> = table.folders[f]
        .modules
        .iter()
        .map(|m| table.files[m.idx()].as_str())
        .collect();
    files.sort_unstable();
    if let Some(dir) = files.iter().find_map(|p| p.strip_suffix("/mod.hd")) {
        return dir.to_owned();
    }
    if let Some(stem) = files.iter().find_map(|p| p.strip_suffix(".hd")) {
        return stem.to_owned();
    }
    table.folders[f].path.clone()
}

/// The shortest directed loop through `touch`: folder indices from its
/// least, open (the message closes it). Loops compare by length, then by
/// folder path order (`module.cycle.diagnostic.loop`).
fn shortest_loop(table: &ModuleTable, adj: &[Vec<usize>], touch: &HashSet<usize>) -> Vec<usize> {
    let n = adj.len();
    let path_of = |f: usize| table.folders[f].path.clone();
    let mut best: Option<Vec<usize>> = None;
    let better = |cand: &[usize], best: &Option<Vec<usize>>| -> bool {
        let Some(best) = best else {
            return true;
        };
        if cand.len() != best.len() {
            return cand.len() < best.len();
        }
        cand.iter().map(|f| path_of(*f)).collect::<Vec<_>>()
            < best.iter().map(|f| path_of(*f)).collect::<Vec<_>>()
    };
    for s in 0..n {
        if !touch.contains(&s) {
            continue;
        }
        // Breadth-first from `s`: the first edge back to `s` closes the
        // shortest loop through `s`.
        let mut dist = vec![u32::MAX; n];
        let mut parent = vec![usize::MAX; n];
        dist[s] = 0;
        let mut queue = vec![s];
        let mut qi = 0;
        while qi < queue.len() {
            let u = queue[qi];
            qi += 1;
            for &v in &adj[u] {
                if v == s {
                    let mut cyc = vec![s];
                    let mut path = vec![u];
                    let mut x = u;
                    while x != s {
                        x = parent[x];
                        path.push(x);
                    }
                    path.pop();
                    path.reverse();
                    cyc.extend(path);
                    if better(&cyc, &best) {
                        best = Some(cyc);
                    }
                    break;
                }
                if dist[v] == u32::MAX {
                    dist[v] = dist[u] + 1;
                    parent[v] = u;
                    queue.push(v);
                }
            }
            if best.as_ref().is_some_and(|b| b.len() == 2 && b[0] == s) {
                break;
            }
        }
    }
    // From its least folder, open.
    let mut out = best.unwrap_or_default();
    if let Some(at) = out
        .iter()
        .enumerate()
        .min_by_key(|(_, f)| path_of(**f))
        .map(|(i, _)| i)
    {
        out.rotate_left(at);
    }
    out
}

/// One diagnostic per cycle: its shortest loop through the cycle's
/// folders, each edge's `use`, the folders on some loop with them, and
/// the fix.
fn describe_cycles(
    table: &ModuleTable,
    texts: &[Arc<str>],
    module_uses: &[Vec<(String, u32, u32)>],
    cycles: &[Vec<FolderId>],
) -> Vec<FolderCycle> {
    let n = table.folders.len();
    // The folder adjacency the graph built, restricted to this package,
    // with the first `use` line making each edge.
    let mut adj = vec![Vec::new(); n];
    let mut edges: HashMap<(usize, usize), (usize, u32, u32)> = HashMap::default();
    for (m, paths) in module_uses.iter().enumerate() {
        if table.modules[m].package != 0 {
            continue;
        }
        let from = table.modules[m].folder.idx();
        let test_code = table.modules[m].role == Role::Test;
        for (path, lo, hi) in paths {
            let Some(target) = table.module_of_use(path) else {
                continue;
            };
            let to = table.modules[target.idx()].folder.idx();
            if to == from {
                continue;
            }
            if table.folders[to].package != 0 {
                continue;
            }
            let into_tests = table.modules[target.idx()].role == Role::Test;
            if !test_code && into_tests {
                continue;
            }
            if let Err(at) = adj[from].binary_search(&to) {
                adj[from].insert(at, to);
            }
            edges.entry((from, to)).or_insert((m, *lo, *hi));
        }
    }
    // The folders on some loop: every folder of every reported cycle, a
    // strongly connected component of the folder graph.
    let mut looping = HashSet::default();
    for c in cycles {
        looping.extend(c.iter().map(|f| f.idx()));
    }
    cycles
        .iter()
        .map(|c| {
            let touch: HashSet<usize> = c.iter().map(|f| f.idx()).collect();
            let shown = shortest_loop(table, &adj, &touch);
            let shown = if shown.is_empty() {
                c.iter().map(|f| f.idx()).collect()
            } else {
                shown
            };
            let mut lines = Vec::new();
            let mut spans = Vec::new();
            for (i, f) in shown.iter().enumerate() {
                let g = shown[(i + 1) % shown.len()];
                if let Some((m, lo, hi)) = edges.get(&(*f, g)) {
                    let text = &texts[*m];
                    let decl = text.get(*lo as usize..*hi as usize).unwrap_or("").trim();
                    let (line, col) = line_col(text, *lo);
                    let file = &table.files[*m];
                    lines.push(format!("  {file}:{line}:{col}: {decl}"));
                    spans.push((table.modules[*m].file, *lo, *hi));
                }
            }
            let names: Vec<String> = shown.iter().map(|f| folder_display(table, *f)).collect();
            let mut message = format!("folder-cycle: {}", names.join(" -> "));
            if let Some(first) = names.first() {
                message.push_str(" -> ");
                message.push_str(first);
            }
            for line in &lines {
                message.push('\n');
                message.push_str(line);
            }
            // The tangle: the folders on some loop with the shown ones.
            let shown_set: HashSet<usize> = shown.iter().copied().collect();
            let tangle = looping
                .iter()
                .filter(|f| {
                    cycles.iter().any(|c| {
                        c.iter().any(|g| shown_set.contains(&g.idx()))
                            && c.iter().any(|g| g.idx() == **f)
                    })
                })
                .count();
            // The fix moves a lone `x.hd` on the loop beside its own
            // `mod.hd`, which keeps its module name.
            let mut fix = None;
            for f in &shown {
                let display = folder_display(table, *f);
                let from = format!("{display}.hd");
                if table.files.contains(&from) {
                    fix = Some((from.clone(), format!("{display}/mod.hd")));
                    break;
                }
            }
            let _ = write!(message, "\ntangle of {tangle} folders");
            if let Some((from, to)) = &fix {
                let _ = write!(message, "; move {from} to {to}");
            }
            let span = spans.into_iter().next().map_or(
                Span {
                    file: FileId::from_raw(u32::MAX),
                    lo: 0,
                    hi: 0,
                },
                |(file, lo, hi)| Span { file, lo, hi },
            );
            FolderCycle { message, span, fix }
        })
        .collect()
}
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
/// (`cli.package.file`), and that `hd test --affected` maps an edit back
/// through. A `tests:` block's uses count, though they make no folder
/// edge: its cases read those modules. Only discovery and skims run, no
/// check.
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
        let file = skims::FileSkim::of(&text);
        let uses = written_uses(&text, &file.use_lines, &roots);
        for u in uses
            .iter()
            .chain(&written_test_uses(&file.test_uses, &roots))
        {
            if let Some(id) = table.module_of_use(u) {
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
    items: &Items,
    rows: &[(u32, u32, u32, hd_resolve::anchor::Anchor)],
) -> HashMap<(DefId, u32), (u32, hd_resolve::anchor::Anchor)> {
    rows.iter()
        .filter_map(|(i, m, slot, a)| Some(((*items.defs().get(*i as usize)?, *slot), (*m, *a))))
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
    /// A failed internal check is counted the same way and reported as an
    /// internal error.
    fn stage<T>(&self, s: Stage, r: StageResult<T>) -> Option<T> {
        match r {
            Ok(v) => {
                lock(&self.report).ok(s);
                Some(v)
            }
            Err(e) => {
                lock(&self.report).not_implemented(s, &e.what);
                self.stop(s, &e);
                None
            }
        }
    }

    /// Reports a stop at the construct, or at the sentinel span when the
    /// stage has no position: a construct this build does not support, or
    /// a compiler bug ([`NotImplemented::bug`]).
    fn stop(&self, s: Stage, e: &NotImplemented) {
        diagnostics::report_stop(&mut lock(&self.diags), s, e);
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
        let file = self.files.get(&self.table, m, &self.texts[m]);
        skim_module(&self.table, m, &self.texts[m], file, self.session_uses(m))
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

    /// Whether module `m`'s top-level statements are its program's entry
    /// behavior in this run (`module.init.script-body`), and so may use
    /// the default profile's row (`module.init.script-profile`), rather than
    /// requirement-free module initialization
    /// (`module.init.requirement-free`). Every REPL input runs as one. A
    /// program runs its entry module's. `hd check` checks each entry module
    /// as running it does: a single-file program as `hd FILE` runs it
    /// (`cli.package.no-root`, `cli.file.check-test`), and the executables
    /// and tasks as `hd run` does (`cli.check.default`, `cli.check.all`).
    /// A test run runs no entry behavior (`module.init.tests.no-entry`),
    /// so neither a test build nor `hd check --tests` on a module with a
    /// `tests:` block, which it checks as a test run does, gives one.
    ///
    /// The answer depends on the goal, and the module's `check` entry
    /// holds what it decided, so `check_key` hashes it: one command's
    /// entry never answers another's.
    fn runs_entry_behavior(&self, m: usize) -> bool {
        if self.repl_number(m).is_some() {
            return true;
        }
        let module = &self.table.modules[m];
        if module.package != 0 {
            return false;
        }
        match &self.goal {
            Goal::Program { entry } | Goal::Scenario { entry, .. } => {
                module.path == format!("{}.{entry}", hd_project::package_ident(&self.package))
            }
            Goal::Tests { .. } | Goal::ReplInput { .. } => false,
            Goal::Analyze => self.entry_module(m) && module.role != Role::Test,
            Goal::CheckTests => {
                self.entry_module(m)
                    && module.role != Role::Test
                    && !self.skim_of(m).file.has_tests_block
            }
        }
    }

    fn folder_graph(&self, sp: &dyn Spawn) {
        let uses: Vec<Vec<String>> = (0..self.table.modules.len())
            .map(|m| self.skim_of(m).uses.clone())
            .collect();
        let graph = FolderGraph::build(&self.table, &uses);
        lock(&self.report).ok(Stage::FolderGraph);
        // `hd check FILE` checks only what FILE reaches (`Run::check_scope`).
        let _ = self.check_scope.set(self.check_scope(&graph));
        let checked = self.check_scope.get().and_then(Option::as_ref);
        // A REPL input's build builds only its closure (`Run::builds`).
        let scope = self
            .repl_inputs
            .input
            .map(|m| graph.closure[self.table.modules[m].folder.idx()].clone());
        let builds = |f: FolderId| {
            scope.as_ref().is_none_or(|s| s.contains(f))
                && checked.is_none_or(|(_, folders)| folders.contains(f))
        };
        let builds_module = |m: usize| {
            builds(self.table.modules[m].folder)
                && self.reached.as_ref().is_none_or(|r| r[m])
                && checked.is_none_or(|(reach, _)| reach[m])
        };
        // `cli.exe.unselected-main`: a public `main` under the source root
        // of a module no executable names is an ordinary function.
        for (m, module) in self.table.modules.iter().enumerate() {
            let source = self.table.files[m].starts_with("src/");
            if module.package != 0
                || module.entry
                || module.role != Role::Lib
                || !source
                || !builds_module(m)
            {
                continue;
            }
            if let Some((lo, hi)) = self.skim_of(m).file.public_main {
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
            if !self.entry_module(m) || !builds_module(m) {
                continue;
            }
            if let Some((lo, hi)) = self.skim_of(m).file.private_main {
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
            if c.iter()
                .any(|f| self.table.folders[f.idx()].package != 0 || !builds(*f))
            {
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
        // The earlier REPL inputs are the session's, read here: they get
        // no task, so a build's tasks do not grow with the session.
        let kept = self.kept.get_or_init(|| {
            (0..self.table.folders.len())
                .map(|fi| self.read_kept(fi))
                .collect()
        });
        let mut iface_task: Vec<Option<TaskId>> = vec![None; self.table.folders.len()];
        for &f in order.iter().filter(|f| !kept[f.idx()] && builds(**f)) {
            // A folder read from the session has no task: a folder that
            // uses one waits for the tasks of the folders it reaches.
            let mut deps: Vec<TaskId> = Vec::new();
            for u in &g.uses[f.idx()] {
                if kept[u.idx()] {
                    for c in g.closure[u.idx()].iter() {
                        if let Some(t) = iface_task[c.idx()]
                            && !deps.contains(&t)
                        {
                            deps.push(t);
                        }
                    }
                } else if let Some(t) = iface_task[u.idx()]
                    && !deps.contains(&t)
                {
                    deps.push(t);
                }
            }
            let t = sp.add(TaskKind::FolderIface(f.raw()), &deps);
            iface_task[f.idx()] = Some(t);
            let closure: Vec<TaskId> = g.closure[f.idx()]
                .iter()
                .filter_map(|c| iface_task[c.idx()])
                .collect();
            let hc = sp.add(TaskKind::HeaderCheck(f.raw()), &closure);
            sp.edge(hc, pr);
        }
        // A program's collection crosses packages (`crates/hd_mono/DESIGN.md` §13.2): the
        // std modules its folders reach are checked to TIR too.
        let mut reached = hd_project::FolderSet::with_capacity(self.table.folders.len());
        for (m, module) in self.table.modules.iter().enumerate() {
            if module.package == 0 && builds_module(m) {
                reached.union(&self.module_closure_in(g, m));
            }
        }
        // The compiler roots `std.rt`'s result functions after an entry or
        // a test case (`RT_SOURCE`), whatever the program uses, and its
        // folder holds the runtime modules under it too.
        if let Some(rt) = self.table.module("std.rt") {
            reached.union(&self.module_closure_in(g, rt.idx()));
        }
        let mut prep_task: Vec<Option<TaskId>> = vec![None; self.table.modules.len()];
        for (m, module) in self.table.modules.iter().enumerate() {
            // `hd check` needs fetched bodies too, to report their `dbg`
            // references. Local and std dependency bodies remain deferred
            // until a program needs them.
            if (module.package != 0
                && (!reached.contains(module.folder)
                    || (self.analyze() && !self.fetched[usize::from(module.package)])))
                || !builds_module(m)
                || kept[module.folder.idx()]
            {
                continue;
            }
            // In the test role, `test_closure(m)`: its `tests:` block's
            // lowering and check read those interfaces too.
            let deps: Vec<TaskId> = self
                .module_closure_in(g, m)
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
        // task that reads each entry (`check_task`). An edge from the input
        // just before is enough, since that one waits for the one before
        // it; an input read from the session has no task.
        let preps: Vec<TaskId> = self
            .repl_inputs()
            .sorted
            .iter()
            .filter_map(|&(_, m)| prep_task[m])
            .collect();
        for pair in preps.windows(2) {
            sp.edge(pair[0], pair[1]);
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

    /// The folders module `m`'s check reads, in folder order: in the test
    /// role `test_closure(m)`, its folder's closure joined with the
    /// closure of each module its `tests:` block uses; else `closure(m)`
    /// (`crates/hd_cache/DESIGN.md` §5.3). A `tests:` block's use makes no
    /// folder edge (`module.cycle.test-code`), so only this widens to it.
    fn module_closure(&self, m: usize) -> Vec<FolderId> {
        self.graph
            .get()
            .map(|g| self.module_closure_in(&g.graph, m).iter().collect())
            .unwrap_or_default()
    }

    /// [`Self::module_closure`] over `graph`, as a set.
    fn module_closure_in(&self, graph: &FolderGraph, m: usize) -> hd_project::FolderSet {
        let folder = self.table.modules[m].folder;
        let mut set = graph.closure[folder.idx()].clone();
        if self.role(m) == "test" {
            for u in &self.skim_of(m).test_uses {
                if let Some(t) = self.table.module_of_use(u) {
                    set.union(&graph.closure[self.table.modules[t.idx()].folder.idx()]);
                }
            }
        }
        set
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
    fn mentions<'a>(
        &self,
        fid: FolderId,
        items: impl IntoIterator<Item = &'a Item>,
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

    /// The folders an interface entry's mention section names, with this
    /// run's deep hashes: `None` when one is not built or is gone.
    fn mentioned(&self, b: &[u8]) -> Option<Vec<(String, Hash128)>> {
        let mut r = Reader::new(b);
        let mut out = Vec::new();
        for _ in 0..r.count() {
            let path = r.str();
            let f = self.table.folders.iter().find(|f| f.path == path)?;
            out.push((path.to_owned(), self.iface_of(f.id)?.deep_hash));
        }
        r.ok().then_some(out)
    }

    /// Re-emits a folder's stage-A diagnostics against this run's files,
    /// with the severity each was reported with.
    fn emit_iface_diags(&self, fi: usize, diags: &IfaceDiags) {
        let folder = &self.table.folders[fi];
        let place = |m: usize, anchor: &hd_resolve::anchor::Anchor| {
            let mid = folder.modules.get(m)?;
            if self.parse_of(mid.idx()).is_ok() {
                hd_resolve::anchor::resolve(&self.src(mid.idx()), anchor)
            } else {
                None
            }
        };
        let mut d = lock(&self.diags);
        for (m, code, severity, anchor, msg, fixes) in diags {
            let Some(mid) = folder.modules.get(*m) else {
                continue;
            };
            let span = place(*m, anchor).unwrap_or(Span {
                file: FileId::from_raw(mid.raw()),
                lo: 0,
                hi: 0,
            });
            d.push(*code, *severity, span, msg, None);
            for fix in fixes {
                // A fix-it with an edit this run cannot place is left out.
                let edits: Option<Vec<(Span, &str)>> = fix
                    .edits
                    .iter()
                    .map(|(m, anchor, lo, hi, text)| {
                        let at = place(*m, anchor)?;
                        let (lo, hi) = (at.lo.wrapping_add(*lo), at.lo.wrapping_add(*hi));
                        Some((Span { lo, hi, ..at }, text.as_str()))
                    })
                    .collect();
                if let Some(edits) = edits {
                    let safety = if fix.exact {
                        hd_diag::FixSafety::Exact
                    } else {
                        hd_diag::FixSafety::Suggestion
                    };
                    d.add_fix(&fix.title, safety, &edits);
                }
            }
        }
    }

    /// `FolderIface(F)` (`crates/hd_resolve/DESIGN.md` §4.10). Its key
    /// is F's own source and what F's header lowering read last time
    /// (`crates/hd_cache/DESIGN.md` §5.3): the read list, an entry under
    /// `reads_key`, names the modules it asked about and the folders whose
    /// interfaces it opened, and `iface_key` adds the run's answer for
    /// each. A folder it never read, such as one a dependency uses
    /// privately, stays out of the key.
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
        let own = self.reads_key(fi);
        // A module that does not parse blocks the interface, cached or
        // not, so a warm run reports what a clean one does: no header
        // finding past a parse error (#310). An unchanged module's clean
        // parse is a `parse` entry, so this parses only edited modules.
        for m in &folder.modules {
            if !self.parses(m.idx()) {
                self.blocked(Stage::FolderIface);
                let _ = self.iface[fi].set(None);
                return;
            }
        }
        // A REPL input's interface carries every item it declares, for
        // later inputs, which see them whatever their `pub`. Its session
        // scope reads the earlier inputs outside the world, so its key
        // keeps every folder of its closure.
        let input = folder
            .modules
            .iter()
            .any(|m| self.repl_number(m.idx()).is_some());
        let stored = if input {
            None
        } else {
            self.lookup(EntryKind::Iface, own)
                .and_then(|s| s.into_iter().next())
        };
        let key_of = |reads: &[(String, bool)]| {
            if input {
                iface_key(own, &reach_hashes)
            } else {
                let answers = self.read_answers(fid, reads);
                let rows: Vec<(&str, Hash128)> =
                    answers.iter().map(|(m, h)| (m.as_str(), *h)).collect();
                iface_key(own, &rows)
            }
        };
        let last = stored.as_deref().and_then(decode_reads);
        let names = self.names();
        if (input || last.is_some())
            && let Some(sections) =
                self.lookup(EntryKind::Iface, key_of(last.as_deref().unwrap_or(&[])))
            && let Some(mentions) = self.iface_hit(fi, &sections, None)
        {
            let m = folder.modules[0].idx();
            self.keep_input(m, |e| &mut e.iface, || sections);
            self.keep_input(m, |e| &mut e.mentions, || mentions);
            return;
        }
        let mods: Vec<hd_resolve::ModIn<'_>> = folder
            .modules
            .iter()
            .map(|m| self.mod_in(m.idx()))
            .collect();
        let world = hd_resolve::Recorded::new(self);
        let cx = hd_resolve::Cx {
            names,
            folder: fid.raw(),
            world: &world,
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
        let reads = self.read_list(fid, &world.reads());
        let key = key_of(&reads);
        if !input {
            let bytes = encode_reads(&reads);
            if stored.as_deref() != Some(bytes.as_slice()) {
                self.put(EntryKind::Iface, own, &[&bytes]);
            }
        }
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
        let hidden = if input {
            hd_base::HashSet::default()
        } else {
            hd_resolve::private_impl_items(&self.pool, &all)
        };
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
        // What a dependent reads leaves out the impls of private types
        // (`crates/hd_resolve/DESIGN.md` §4.10, "Two hashes, two readers").
        let api_blob = if hidden.is_empty() {
            None
        } else {
            let visible: Vec<Item> = items
                .iter()
                .filter(|i| !hidden.contains(&i.def))
                .cloned()
                .collect();
            match hd_resolve::encode_items(&names, &visible, &out.exports) {
                Ok(b) => Some(hd_base::hash128(&b)),
                Err(e) => {
                    self.stage::<()>(Stage::FolderIface, Err(e));
                    let _ = self.iface[fi].set(None);
                    return;
                }
            }
        };
        let hsec = api_blob.map_or_else(Vec::new, |h| h.0.to_le_bytes().to_vec());
        let idiags = self.anchored_diags(fi, &diags);
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
        let usec = hd_resolve::encode_module_uses(&out.module_uses);
        let nhash = names_hash(&psec, &lsec);
        let Some(mentions) = self.mentions(fid, &items, &out.exports) else {
            self.blocked(Stage::FolderIface);
            let _ = self.iface[fi].set(None);
            return;
        };
        // The folders the interface names, so a hit finds them without
        // decoding its items (#213).
        let msec = encode_mentioned(&mentions);
        self.put(
            EntryKind::Iface,
            key,
            &[&blob, &dsec, &asec, &psec, &lsec, &usec, &hsec, &msec],
        );
        self.keep_input(
            folder.modules[0].idx(),
            |e| &mut e.iface,
            || {
                vec![
                    blob.clone(),
                    dsec,
                    asec,
                    psec,
                    lsec,
                    usec.clone(),
                    hsec,
                    msec,
                ]
            },
        );
        self.keep_input(
            folder.modules[0].idx(),
            |e| &mut e.mentions,
            || mentions.clone(),
        );
        let mut iface = hd_resolve::folder_iface(
            &folder.path,
            Items::new(items),
            out.exports,
            Arc::from(blob),
            api_blob,
            &mentions,
            out.module_uses,
        );
        iface.spans = spans_of(&iface.items, &rows);
        iface.private_names = out.private_names.into_iter().collect();
        iface.loop_names = out.loop_names.into_iter().collect();
        iface.names_hash = nhash;
        lock(&self.diags).append(&diags);
        self.note_iface(&folder.path, &iface, true);
        lock(&self.report).ok(Stage::FolderIface);
        let _ = self.iface[fi].set(Some(Arc::new(iface)));
    }

    /// `diags`, reported in folder `fi`'s files, with each span as a
    /// declaration-relative anchor, as an entry stores them.
    fn anchored_diags(&self, fi: usize, diags: &DiagBuf) -> IfaceDiags {
        let folder = &self.table.folders[fi];
        let module_of = |file: FileId| folder.modules.iter().position(|x| x.raw() == file.raw());
        (0..diags.len())
            .map(|i| {
                let sp = diags.primary[i];
                let m = module_of(sp.file).unwrap_or(0);
                let fixes = diags.fixes[i];
                let fixes = (fixes.start..fixes.start + fixes.len)
                    .filter_map(|f| {
                        let f = f as usize;
                        let edits = diags.edits[diags.fix_edits[f].range()]
                            .iter()
                            .map(|e| {
                                let m = module_of(e.span.file)?;
                                let src = self.src(folder.modules[m].idx());
                                let (anchor, lo, hi) = edit_anchor(&src, e.span);
                                Some((m, anchor, lo, hi, diags.get_text(e.text).to_owned()))
                            })
                            .collect::<Option<Vec<_>>>()?;
                        Some(IfaceFix {
                            title: diags.get_text(diags.fix_title[f]).to_owned(),
                            exact: diags.fix_safety[f] == hd_diag::FixSafety::Exact,
                            edits,
                        })
                    })
                    .collect();
                (
                    m,
                    diags.code[i],
                    diags.severity[i],
                    hd_resolve::anchor::of_span(&self.src(folder.modules[m].idx()), sp.lo, sp.hi),
                    diags.get_text(diags.message[i]).to_owned(),
                    fixes,
                )
            })
            .collect()
    }

    /// The modules and folders a header lowering of folder `fid` read, as
    /// its read list: each module path it asked about, and whether it
    /// opened the interface of that module's folder. A folder opened
    /// through no module it asked about is listed by its first module.
    fn read_list(&self, fid: FolderId, reads: &hd_resolve::Reads) -> Vec<(String, bool)> {
        // `reads.modules` is sorted, so `out` is, by path.
        let mut out: Vec<(String, bool)> = Vec::new();
        let mut covered = Vec::new();
        for m in &reads.modules {
            let f = self.table.folder_of_module(m).map(FolderId::raw);
            let opened = f.is_some_and(|f| f != fid.raw() && reads.folders.contains(&f));
            if opened {
                covered.extend(f);
            }
            out.push((m.clone(), opened));
        }
        for f in reads.folders.iter().filter(|f| !covered.contains(*f)) {
            let first = self
                .table
                .folders
                .get(*f as usize)
                .and_then(|g| g.modules.first());
            if let Some(m) = first {
                let path = &self.table.modules[m.idx()].path;
                match out.binary_search_by(|(p, _)| p.cmp(path)) {
                    Ok(at) => out[at].1 = true,
                    Err(at) => out.insert(at, (path.clone(), true)),
                }
            }
        }
        out
    }

    /// The run's answer for each read of a read list, for `iface_key`:
    /// the module's folder, whether a use may name it and whether it is
    /// test-only code, and, when the lowering opened its folder's
    /// interface, that interface's deep hash and names hash.
    fn read_answers(&self, fid: FolderId, reads: &[(String, bool)]) -> Vec<(String, Hash128)> {
        use hd_resolve::World as _;
        reads
            .iter()
            .map(|(m, opened)| {
                let mut k = hd_base::StableHasher::new("iface-read");
                match self.table.folder_of_module(m) {
                    Some(f) => {
                        k.u8(1);
                        k.str(&self.table.folders[f.idx()].path);
                        if *opened {
                            match self.iface_of(f).filter(|_| f != fid) {
                                Some(i) => {
                                    k.u8(1);
                                    k.hash(i.deep_hash);
                                    k.hash(i.names_hash);
                                }
                                None => k.u8(0),
                            }
                        }
                    }
                    None => k.u8(0),
                }
                k.u8(u8::from(self.usable(m)));
                k.u8(u8::from(self.test_only(m)));
                (m.clone(), k.finish())
            })
            .collect()
    }

    /// `reads_key(F)` (`crates/hd_cache/DESIGN.md` §5.3): what folder
    /// `fi`'s own source decides of its interface and its uses.
    fn reads_key(&self, fi: usize) -> Hash128 {
        let folder = &self.table.folders[fi];
        let apis: Vec<FileApi<'_>> = folder
            .modules
            .iter()
            .map(|m| FileApi {
                module: &self.table.modules[m.idx()].path,
                role: ROLE,
                api_text_hash: self.skim_of(m.idx()).file.api_text_hash,
            })
            .collect();
        reads_key(
            self.toolchain,
            self.package_key_of(folder.package),
            &folder.path,
            &apis,
            self.roots_hash(fi),
        )
    }

    /// The hash of the use roots of folder `fi`'s modules (`UseRoots`):
    /// what a use's root names, which no read records.
    fn roots_hash(&self, fi: usize) -> Hash128 {
        let mut k = hd_base::StableHasher::new("use-roots");
        for m in &self.table.folders[fi].modules {
            let r = self.table.use_roots(*m);
            k.str(&r.pkg);
            k.str(&r.base);
            k.str(&r.floor);
            k.u32(u32_of(r.deps.len()));
            for (name, root) in &r.deps {
                k.str(name);
                k.str(root);
            }
            k.u32(u32_of(r.dev.len()));
            for (name, root, back) in &r.dev {
                k.str(name);
                k.str(root);
                k.u8(u8::from(*back));
            }
            k.u8(r.access as u8);
        }
        k.finish()
    }

    /// Sets folder `fi`'s interface from the sections of its `iface`
    /// entry and the deep hashes of the folders it names: `mentions`, or
    /// else this run's interfaces'. Returns those, if the sections decode.
    fn iface_hit(
        &self,
        fi: usize,
        sections: &[Vec<u8>],
        mentions: Option<&[(String, Hash128)]>,
    ) -> Option<Vec<(String, Hash128)>> {
        let folder = &self.table.folders[fi];
        let names = self.names();
        let (blob, dsec) = (sections.first()?, sections.get(1)?);
        let blob: Arc<[u8]> = Arc::from(blob.as_slice());
        // Each item record decodes on first use (#213).
        let (items, exports) = hd_resolve::decode_lazy(&names, blob.clone())?;
        let (Some(diags), Some(rows), Some(private), Some(looped), Some(module_uses)) = (
            decode_iface_diags(dsec),
            sections.get(2).and_then(|b| hd_resolve::anchor::decode(b)),
            sections
                .get(3)
                .and_then(|b| hd_resolve::decode_private_names(&names, b)),
            sections
                .get(4)
                .and_then(|b| hd_resolve::decode_private_names(&names, b)),
            sections
                .get(5)
                .and_then(|b| hd_resolve::decode_module_uses(b)),
        ) else {
            return None;
        };
        let mentions = match mentions {
            Some(m) => m.to_vec(),
            None => match sections.get(7) {
                Some(b) => self.mentioned(b)?,
                None => self.mentions(folder.id, &items, &exports)?,
            },
        };
        // The hash of the blob without the impls of private types, when
        // it has any: sixteen bytes, or none.
        let api_blob = sections
            .get(6)
            .and_then(|b| <[u8; 16]>::try_from(b.as_slice()).ok())
            .map(|b| Hash128(u128::from_le_bytes(b)));
        let mut iface = hd_resolve::folder_iface(
            &folder.path,
            items,
            exports,
            blob,
            api_blob,
            &mentions,
            module_uses,
        );
        iface.spans = spans_of(&iface.items, &rows);
        iface.private_names = private.into_iter().collect();
        iface.loop_names = looped.into_iter().collect();
        iface.names_hash = names_hash(sections.get(3)?, sections.get(4)?);
        self.emit_iface_diags(fi, &diags);
        self.note_iface(&folder.path, &iface, false);
        lock(&self.report).ok(Stage::FolderIface);
        let _ = self.iface[fi].set(Some(Arc::new(iface)));
        Some(mentions)
    }

    /// Reads folder `fi` from the session when it is an earlier REPL
    /// input's ([`Session::inputs`]): its interface, its header findings,
    /// its init order (one module, so no cycle) and its module's check,
    /// with no task, no key and no store read. Whether it did.
    fn read_kept(&self, fi: usize) -> bool {
        let Some(kept) = self.kept_folder(fi) else {
            return false;
        };
        let (Some(iface), Some(mentions), Some(header), Some(uses), Some(check)) = (
            kept.iface.as_deref(),
            kept.mentions.as_deref(),
            kept.header.as_ref().and_then(|h| h.first()),
            kept.header
                .as_ref()
                .and_then(|h| h.get(1))
                .and_then(|b| decode_iface_diags(b)),
            kept.check.as_deref(),
        ) else {
            return false;
        };
        if self.iface_hit(fi, iface, Some(mentions)).is_none() {
            return false;
        }
        let fid = self.table.folders[fi].id;
        let Some(found) = self
            .iface_of(fid)
            .and_then(|own| decode_header_findings(header, &own.items))
        else {
            return false;
        };
        lock(&self.report).ok(Stage::HeaderCheck);
        self.emit_iface_diags(fi, &uses);
        self.emit_findings(&found);
        let _ = self.init_runs[fi].set(Vec::new());
        lock(&self.report).ok(Stage::InitOrder);
        let m = self.table.folders[fi].modules[0].idx();
        lock(&self.report).ok(Stage::ModulePrep);
        self.read_check(m, check, Arc::from(join_sections(check)));
        let _ = self.prep[m].set(None);
        true
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
        &self.repl_inputs
    }

    /// The root package's `src/lib.hd`, if it has one.
    fn lib_module(&self) -> Option<usize> {
        self.repl_inputs.lib
    }

    /// Whether REPL input `m`'s session holds `src/lib.hd`
    /// ([`ReplInputs::with_lib`]): only then is the library built for it,
    /// and only then does it see the library's names.
    fn with_lib(&self, m: usize) -> bool {
        self.repl_inputs.with_lib[m]
    }

    /// The library, for REPL input `m`, when its session holds it.
    fn session_lib(&self, m: usize) -> Option<usize> {
        self.lib_module().filter(|_| self.with_lib(m))
    }

    /// Whether this run builds folder `f`: a REPL input's build builds the
    /// folders the input's closure holds (`cli.repl.uses.other`), so a
    /// broken module it never reaches does not stop it; every other run
    /// builds them all.
    fn builds(&self, f: FolderId) -> bool {
        let scoped = self
            .check_scope
            .get()
            .and_then(Option::as_ref)
            .is_none_or(|(_, folders)| folders.contains(f));
        scoped
            && match (self.repl_inputs.input, self.graph.get()) {
                (Some(m), Some(g)) => {
                    g.graph.closure[self.table.modules[m].folder.idx()].contains(f)
                }
                _ => true,
            }
    }

    /// Whether this run builds module `m`: a module of a folder it builds,
    /// which a REPL input's build also reached ([`repl_texts`]). A module
    /// of such a folder that the input does not reach is empty for the run,
    /// as are its items.
    fn builds_module(&self, m: usize) -> bool {
        self.builds(self.table.modules[m].folder)
            && self.reached.as_ref().is_none_or(|r| r[m])
            && self
                .check_scope
                .get()
                .and_then(Option::as_ref)
                .is_none_or(|(reach, _)| reach[m])
    }

    /// A narrowed check's scope ([`Packages::check_roots`]): per module,
    /// whether a root's module reaches it through `use`s, its `tests:`
    /// block's included, as `hd check FILE` filters its output; and the
    /// folders of the reached modules' closures. `None` when the run
    /// checks the whole package.
    fn check_scope(&self, g: &FolderGraph) -> Option<(Vec<bool>, hd_project::FolderSet)> {
        let roots = self.check_roots.as_ref().filter(|_| self.analyze())?;
        let n = self.table.modules.len();
        let file_of = |m: usize| &self.table.files[self.table.modules[m].file.idx()];
        let mut reach = vec![false; n];
        let mut stack: Vec<usize> = (0..n).filter(|&m| roots.contains(file_of(m))).collect();
        while let Some(m) = stack.pop() {
            if std::mem::replace(&mut reach[m], true) {
                continue;
            }
            let s = self.skim_of(m);
            stack.extend(
                s.uses
                    .iter()
                    .chain(&s.test_uses)
                    .filter_map(|u| self.table.module_of_use(u))
                    .map(hd_base::ModuleId::idx),
            );
        }
        let mut folders = hd_project::FolderSet::with_capacity(self.table.folders.len());
        for m in (0..n).filter(|&m| reach[m]) {
            folders.union(&self.module_closure_in(g, m));
        }
        Some((reach, folders))
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
    /// `src/lib.hd`, when its session holds it, then the earlier inputs in
    /// order. None for any other module.
    fn session_modules(&self, m: usize) -> Vec<String> {
        if self.repl_number(m).is_none() {
            return Vec::new();
        }
        let earlier = self.earlier_inputs(m).iter().map(|(_, k)| *k);
        self.session_lib(m)
            .into_iter()
            .chain(earlier)
            .map(|k| self.table.modules[k].path.clone())
            .collect()
    }

    /// The entries REPL input `m` left in the session, when it is an input
    /// before the one this run builds ([`Session::inputs`]).
    fn kept_input(&self, m: usize) -> Option<&InputEntries> {
        let n = self.repl_number(m)?;
        match self.goal {
            Goal::ReplInput { .. } if !self.repl_inputs.builds(n) => self.session?.inputs.get(&n),
            _ => None,
        }
    }

    /// [`Run::kept_input`] for folder `fi`, which an input has to itself.
    fn kept_folder(&self, fi: usize) -> Option<&InputEntries> {
        match self.table.folders[fi].modules.as_slice() {
            [m] => self.kept_input(m.idx()),
            _ => None,
        }
    }

    /// Whether module `m` is an earlier REPL input read from the session
    /// ([`Run::read_kept`]).
    fn is_kept(&self, m: usize) -> bool {
        self.kept
            .get()
            .is_some_and(|k| k[self.table.modules[m].folder.idx()])
    }

    /// Records an entry of a REPL input this run builds, when module `m`
    /// is one ([`Session::inputs`]).
    fn keep_input<T>(
        &self,
        m: usize,
        part: fn(&mut InputEntries) -> &mut Option<T>,
        value: impl FnOnce() -> T,
    ) {
        if let Some(n) = self.repl_number(m).filter(|n| self.repl_inputs.builds(*n)) {
            *part(lock(&self.input_entries).entry(n).or_default()) = Some(value());
        }
    }

    /// The modules REPL input `m` uses for its session scope
    /// ([`session_uses`]).
    fn session_uses(&self, m: usize) -> Vec<String> {
        session_uses(&self.table, &self.repl_inputs, m)
    }

    /// The trie nodes of the modules of REPL input `m`'s session
    /// ([`hd_check::BodyCx::session`]): the input, `src/lib.hd` and the
    /// earlier inputs. Never another module of the package or a
    /// dependency's, whose private members stay private
    /// (`cli.repl.package.lib` names only `src/lib.hd`). Empty for any
    /// other module.
    fn session_nodes(&self, m: usize) -> Vec<hd_base::PathId> {
        if self.repl_number(m).is_none() {
            return Vec::new();
        }
        let names = self.names();
        let own = &self.table.modules[m].path;
        std::iter::once(own)
            .chain(&self.session_modules(m))
            .map(|p| names.module(p))
            .collect()
    }

    /// `src/lib.hd` lowered against its frozen interface, once per run.
    /// Only the tasks of a REPL input whose session holds the library call
    /// it ([`Run::with_lib`]): the input's closure holds the library's
    /// folder, so its interface is built first. Another module's task may
    /// run before that interface and would fix the result at `None` for the
    /// whole run.
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
            let lib = if self.with_lib(m) {
                self.lib_scope()
            } else {
                None
            };
            let mut out = lib.map_or_else(hd_resolve::SessionScope::default, |lib| {
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
                    modules: Vec::new(),
                }
            });
            out.modules = self.session_modules(m);
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
                let names = self.names();
                for (_, local, path) in &iface.module_uses {
                    let name = names.syms.intern(local);
                    let value = u32::try_from(inner.modules.len()).unwrap_or(u32::MAX);
                    inner.modules.push(path.clone());
                    let b = hd_resolve::Binding {
                        kind: hd_resolve::BindingKind::Module,
                        value,
                    };
                    inner.bind(name, b, hd_resolve::Origin::Use, u32::MAX);
                    out.declared_by.insert(name, format!("repl#{number}"));
                }
                out.scope.nest(&inner);
            }
            Arc::new(out)
        });
        Some(Arc::clone(scope))
    }

    /// One module's items (private ones included), scope and kinds,
    /// against its folder's frozen interface. Header diagnostics are the
    /// folder interface's and use diagnostics its `HeaderCheck`'s, so
    /// `diags` here is the caller's to drop.
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
    /// impl universe, taken when the task starts (`crates/hd_check/DESIGN.md` (trait solver) §3.2).
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
        // interface carries (`crates/hd_check/DESIGN.md` (trait solver) §3.9): its source is a
        // dependency of the findings, as it is of a module's check.
        let Some(templates) = self.tuple_template_sources(fid) else {
            self.blocked(Stage::HeaderCheck);
            return;
        };
        let reach: Vec<(&str, Hash128)> = reach
            .into_iter()
            .chain(templates.iter().map(|(p, h)| (p.as_str(), *h)))
            .collect();
        // A test's fuel budget for the root package ([`Packages::body_fuel`])
        // bounds each item's goals too, so it is part of the key.
        let test_fuel = self.body_fuel.filter(|_| folder.package == 0);
        let fuel = test_fuel.unwrap_or(hd_base::Fuel::BODY_DEFAULT);
        // The folder's own impls of private types are in its findings.
        // Its uses' diagnostics read its own uses, which a body-only use
        // edit changes and no interface hash sees (`reads_key`), and the
        // private names of the folders they reach.
        let own_hash = {
            let mut k = hd_base::StableHasher::new("hdr-own");
            k.hash(own.deep_hash);
            k.hash(own.heads_hash);
            k.hash(self.reads_key(fi));
            for i in &all {
                k.hash(i.names_hash);
            }
            if let Some(fuel) = test_fuel {
                k.u64(fuel);
            }
            k.finish()
        };
        let key = hdr_key(
            self.toolchain,
            self.package_key_of(folder.package),
            &folder.path,
            own_hash,
            &reach,
        );
        if let Some(sections) = self.lookup(EntryKind::Graph, key)
            && let Some(found) = sections
                .first()
                .and_then(|b| decode_header_findings(b, &own.items))
            && let Some(uses) = sections.get(1).and_then(|b| decode_iface_diags(b))
        {
            lock(&self.report).ok(Stage::HeaderCheck);
            self.emit_iface_diags(fi, &uses);
            self.emit_findings(&found);
            self.keep_input(folder.modules[0].idx(), |e| &mut e.header, || sections);
            return;
        }
        self.computed(Stage::HeaderCheck);
        let names = self.names();
        // The uses' diagnostics (#248), reported whatever stage B finds.
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
        let mut found_uses = DiagBuf::default();
        hd_resolve::use_diags(&cx, &mods, &mut found_uses);
        let uses = encode_iface_diags(&self.anchored_diags(fi, &found_uses));
        lock(&self.diags).append(&found_uses);
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
        let session = match folder.modules.as_slice() {
            [m] => self.session_nodes(m.idx()),
            _ => Vec::new(),
        };
        let cx = hd_check::header::HeaderCx {
            names,
            lookup: &lookup,
            impls: &impls,
            global: &self.memo,
            solver: &TableSolver,
            session: &session,
            fuel,
        };
        let r = hd_check::header::stage_b(&cx, &own.items);
        let Some(findings) = self.stage(Stage::HeaderCheck, r) else {
            return;
        };
        let bytes = encode_header_findings(&findings, &own.items);
        self.put(EntryKind::Graph, key, &[&bytes, &uses]);
        self.keep_input(
            folder.modules[0].idx(),
            |e| &mut e.header,
            || vec![bytes, uses],
        );
        self.emit_findings(&findings);
    }

    fn emit_findings(&self, findings: &[hd_check::header::Finding]) {
        let mut d = lock(&self.diags);
        for f in findings {
            emit_finding(&mut d, f, |slot| self.item_span(f.item, slot));
        }
    }

    /// The init summaries `InitOrder(F)` reads: one list per module cycle
    /// of folder `fi` whose statements sit in several modules, and their
    /// part key, `init_key` over the summaries. `None` until every module
    /// of a cycle is checked.
    fn init_inputs(&self, fi: usize) -> Option<InitInputs<'_>> {
        let folder = &self.table.folders[fi];
        let mut cycles: Vec<Vec<(usize, &[u8])>> = Vec::new();
        for cycle in self.folder_cycles(fi) {
            let mut sums = Vec::new();
            for m in cycle {
                let Some(Some(c)) = self.check[m].get() else {
                    return None;
                };
                // A module with errors stops the build before collection.
                if c.has_errors {
                    sums.clear();
                    break;
                }
                let sections = split_sections(&c.entry)?;
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
        Some((cycles, key))
    }

    /// `InitOrder(F)` (§4.13.10; spec 10 "Order Inside A Cycle"): the
    /// statement order of each module cycle of the folder whose statements
    /// sit in several modules, from the modules' init summaries, or
    /// `top-level-read-before-initialization` where the statements wait on
    /// each other (`module.init.cycle.stuck`). The order is a part of the
    /// package's `graph` entry (`crates/hd_cache/DESIGN.md`, "Graph parts").
    fn init_order(&self, fi: usize) {
        let part = self.init_order_part(fi);
        self.part_done(1 + self.graph_slot(fi), part);
    }

    /// [`Self::init_order`]'s runs as the part's bytes; `None` when it is
    /// blocked or reports a cycle, which every run reports again.
    fn init_order_part(&self, fi: usize) -> Option<Vec<u8>> {
        let names = self.names();
        let Some((cycles, _)) = self.init_inputs(fi) else {
            self.blocked(Stage::InitOrder);
            return None;
        };
        self.computed(Stage::InitOrder);
        let mut out = Vec::new();
        let mut stuck = false;
        for cycle in &cycles {
            let mut sums = Vec::new();
            for (m, b) in cycle {
                let Some(sum) = decode_summary(b, &names, &self.lines(*m)) else {
                    self.stage::<()>(
                        Stage::InitOrder,
                        Err(NotImplemented::bug(
                            Stage::InitOrder,
                            "a module's init summary did not decode",
                        )),
                    );
                    return None;
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
        let bytes = if out.is_empty() {
            Vec::new()
        } else {
            encode_runs(&out)
        };
        let _ = self.init_runs[fi].set(out);
        lock(&self.report).ok(Stage::InitOrder);
        // An initialization cycle is reported again by every run.
        (!stuck).then_some(bytes)
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

    /// The interfaces `Coherence` reads, every folder the run builds (a
    /// REPL input's program is its closure, [`Run::builds`]), and their
    /// part key; `None` while one is not built. The key is coarser than
    /// the per-trait `coh_key`s, which only `Coherence` computes: it
    /// decides whether `Coherence` runs, and that run checks again only
    /// the traits whose `coh_key` changed (`crates/hd_cache/DESIGN.md`,
    /// "Coherence runs per changed trait").
    fn coherence_inputs(&self) -> Option<(Vec<Arc<FolderIface>>, Hash128)> {
        let built: Vec<FolderId> = (0..self.table.folders.len())
            .map(|f| FolderId::from_raw(u32_of(f)))
            .filter(|f| self.builds(*f))
            .collect();
        let mut all = Vec::new();
        for &f in &built {
            all.push(self.iface_of(f)?);
        }
        let mut k = hd_base::StableHasher::new("coh");
        k.hash(self.toolchain);
        for (f, i) in built.iter().zip(&all) {
            k.str(&self.table.folders[f.idx()].path);
            k.hash(i.deep_hash);
            k.hash(i.heads_hash);
        }
        Some((all, k.finish()))
    }

    /// `Coherence`: the overlap check (§4.12.3) over the impl heads of
    /// every interface of the program graph, std's included, per trait:
    /// a trait whose `coh_key` the package's coherence rows hold reads its
    /// findings there. The findings are a part of the package's `graph`
    /// entry.
    fn coherence(&self) {
        let part = self.coherence_part();
        self.part_done(0, part);
    }

    /// [`Self::coherence`]'s findings as the part's bytes; `None` when it
    /// is blocked.
    fn coherence_part(&self) -> Option<Vec<u8>> {
        let Some((all, _)) = self.coherence_inputs() else {
            self.blocked(Stage::Coherence);
            return None;
        };
        self.computed(Stage::Coherence);
        let names = self.names();
        let u = hd_resolve::Universe::new(names, all.iter().flat_map(|i| i.items.impls()));
        // A REPL session is one module whose content runs `src/lib.hd`
        // first, then the inputs by number (`cli.repl.redefine.impl`), so
        // input 10 sorts after input 9.
        let inputs: HashMap<&str, u32> = self
            .repl_inputs()
            .sorted
            .iter()
            .map(|&(n, k)| (self.table.modules[k].path.as_str(), n))
            .collect();
        // The interfaces that carry each impl, in folder order, so a key
        // reads those instead of every interface of the program.
        let mut holders: HashMap<DefId, Vec<&FolderIface>> = HashMap::default();
        for i in &all {
            for it in &i.items {
                if matches!(it.data, ItemData::Impl { .. }) {
                    holders.entry(it.def).or_default().push(i);
                }
            }
        }
        let carrying = |d: DefId| -> Vec<&FolderIface> {
            holders
                .get(&d)
                .cloned()
                .unwrap_or_else(|| all.iter().map(AsRef::as_ref).collect())
        };
        // Content order (§4.12.3): the later impl is the one reported. What
        // `@error` generates ranks first, so a hand-written impl beside it
        // is the one reported (spec 14 `annot.error.hand-written`); a
        // `@derive` ranks after written impls, a derivation block after both.
        let order = |d: DefId| {
            let carriers = carrying(d);
            let rank = carriers
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
            let at = carriers
                .iter()
                .find_map(|i| i.spans.get(&(d, 0)))
                .map_or((0, 0, 0), |(_, a)| (a.decl, a.member, a.tok));
            let mut module = names.module_of(d);
            if let Some(n) = inputs.get(module.as_str())
                && let Some(stem) = module.strip_suffix(&n.to_string())
            {
                module = format!("{stem}{n:010}");
            }
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
        // Per trait, `coh_key` over the head hashes of its impls; a trait
        // whose key the package's coherence rows hold reads its findings
        // there (`crates/hd_cache/DESIGN.md`, "Coherence runs per changed
        // trait"). The head hashes come per folder, read from the rows
        // while the folder's impl records and their ranks are unchanged.
        let old = self.coherence_rows();
        let old_folders: HashMap<Hash128, &CohFolder> =
            old.folders.iter().rev().map(|r| (r.key, r)).collect();
        let old_traits: HashMap<Hash128, &CohRow> =
            old.traits.iter().rev().map(|r| (r.key, r)).collect();
        let trait_of: HashMap<Hash128, DefId> =
            u.impls.keys().map(|t| (names.path_hash(*t), *t)).collect();
        let mut parts: HashMap<DefId, Vec<Hash128>> = HashMap::default();
        let (mut folders, mut hashed) = (Vec::new(), 0_usize);
        let mut cacheable = true;
        for iface in &all {
            let impls: Vec<&Item> = iface
                .items
                .impls()
                .filter(|it| {
                    matches!(it.data, ItemData::Impl { trait_, kind, .. }
                        if trait_ != DefId::NONE && kind.is_impl())
                })
                .collect();
            if impls.is_empty() {
                continue;
            }
            let key = coherence_folder_key(iface, &impls, &holders, &inputs, &order);
            let row = if let Some(r) = old_folders.get(&key) {
                (*r).clone()
            } else {
                hashed += 1;
                let Some(row) = coherence_folder(&u, iface, &impls, key, &order) else {
                    cacheable = false;
                    break;
                };
                row
            };
            for (t, h) in &row.parts {
                if let Some(t) = trait_of.get(t) {
                    parts.entry(*t).or_default().push(*h);
                }
            }
            folders.push(row);
        }
        let part_of = |t: DefId| Some(parts.get(&t).cloned().unwrap_or_default());
        let (mut found, mut rows, mut ran) = (Vec::new(), Vec::new(), Vec::new());
        let (mut tried, mut read) = (0, 0);
        for tr in u.traits() {
            let key = if cacheable {
                u.coh_key(tr, &part_of)
            } else {
                None
            };
            // The key holds each impl's stable path, so a stored path names
            // the same impl of `tr`.
            let reused = key.and_then(|k| old_traits.get(&k)).and_then(|row| {
                row.found
                    .iter()
                    .map(|(path, msg)| {
                        let at = u.impls[&tr].iter().find(|i| names.path(i.def) == *path)?;
                        Some((at.def, msg.clone()))
                    })
                    .collect::<Option<Vec<_>>>()
            });
            let part: Vec<(DefId, String)> = if let Some(part) = reused {
                read += 1;
                part
            } else {
                let (overlaps, n) = u.trait_overlaps(tr, &order);
                tried += n;
                ran.push(names.path(tr));
                overlaps
                    .into_iter()
                    .map(|(a, b, witness)| {
                        let msg = format!(
                            "implementations {} and {} both apply to {witness}",
                            names.path(a),
                            names.path(b)
                        );
                        (b, msg)
                    })
                    .collect()
            };
            if let Some(key) = key {
                rows.push(CohRow {
                    key,
                    trait_: names.path(tr),
                    found: part
                        .iter()
                        .map(|(b, msg)| (names.path(*b), msg.clone()))
                        .collect(),
                });
            }
            found.extend(part);
        }
        if cacheable && (hashed > 0 || !ran.is_empty()) {
            let rows = CohRows {
                traits: keep_previous(rows, &old.traits, |r| (&r.trait_, r.key)),
                folders: keep_previous(folders, &old.folders, |r| (&r.folder, r.key)),
            };
            self.put(
                EntryKind::Graph,
                self.coherence_rows_key(),
                &[&encode_coh_rows(&rows)],
            );
        }
        {
            let mut c = lock(&self.counters);
            c.overlap_pairs += tried;
            c.coherence_traits.extend(ran);
            c.coherence_traits_read += read;
        }
        lock(&self.report).ok(Stage::Coherence);
        let bytes = if found.is_empty() {
            Vec::new()
        } else {
            encode_overlaps(&found, &all)
        };
        let mut d = lock(&self.diags);
        for (item, msg) in found {
            d.error(Code::OverlappingImpl, self.item_span(item, 0), &msg);
        }
        Some(bytes)
    }

    /// The key of the package's coherence rows: one `graph` entry per
    /// package, rewritten when `Coherence` hashes a folder or checks a
    /// trait.
    fn coherence_rows_key(&self) -> Hash128 {
        let mut k = hd_base::StableHasher::new("coh-rows");
        k.hash(self.toolchain);
        k.hash(self.package_key);
        k.finish()
    }

    /// The package's coherence rows, newest first; empty when there are
    /// none.
    fn coherence_rows(&self) -> CohRows {
        self.lookup(EntryKind::Graph, self.coherence_rows_key())
            .and_then(|s| decode_coh_rows(s.first()?))
            .unwrap_or_default()
    }

    /// The slot of folder `fi`'s init order among the graph parts after
    /// coherence's.
    fn graph_slot(&self, fi: usize) -> usize {
        self.graph_parts
            .get()
            .and_then(|g| g.folders.iter().position(|f| *f == fi))
            .unwrap_or(usize::MAX - 1)
    }

    /// Records graph part `slot`'s bytes; the last part to finish writes
    /// the package's `graph` entry, unless a part has none.
    fn part_done(&self, slot: usize, part: Option<Vec<u8>>) {
        let Some(g) = self.graph_parts.get() else {
            return;
        };
        if let Some(s) = g.slots.get(slot) {
            let _ = s.set(part);
        }
        if g.left.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) != 1 {
            return;
        }
        let Some(parts) = g
            .slots
            .iter()
            .map(|s| s.get().cloned().flatten())
            .collect::<Option<Vec<Vec<u8>>>>()
        else {
            return;
        };
        let sections = encode_graph(&parts);
        let sections: Vec<&[u8]> = sections.iter().map(Vec::as_slice).collect();
        self.put(EntryKind::Graph, g.key, &sections);
    }

    /// The package's `graph` entry (`crates/hd_cache/DESIGN.md`, "Graph
    /// parts"), keyed by its parts' keys: coherence's, then each built
    /// folder's init order's. On a hit, applies every part and returns
    /// `true`; on a miss, starts the entry the part tasks fill.
    fn graph_hit(&self, folders: &[usize]) -> bool {
        let Some((all, coh)) = self.coherence_inputs() else {
            return false;
        };
        let mut keys = vec![coh];
        for &f in folders {
            let Some((_, k)) = self.init_inputs(f) else {
                return false;
            };
            keys.push(k);
        }
        let mut k = hd_base::StableHasher::new("graph");
        k.hash(self.toolchain);
        k.u32(u32_of(keys.len()));
        for x in &keys {
            k.hash(*x);
        }
        let key = k.finish();
        let hit = self.lookup(EntryKind::Graph, key).and_then(|s| {
            let parts = decode_graph(&s, keys.len())?;
            let found = if parts[0].is_empty() {
                Vec::new()
            } else {
                decode_overlaps(&parts[0], &all)?
            };
            let runs = parts[1..]
                .iter()
                .map(|b| {
                    if b.is_empty() {
                        Some(Vec::new())
                    } else {
                        decode_runs(b)
                    }
                })
                .collect::<Option<Vec<_>>>()?;
            Some((found, runs))
        });
        let Some((found, runs)) = hit else {
            let _ = self.graph_parts.set(GraphParts {
                key,
                folders: folders.to_vec(),
                slots: (0..keys.len()).map(|_| OnceLock::new()).collect(),
                left: std::sync::atomic::AtomicUsize::new(keys.len()),
            });
            return false;
        };
        lock(&self.report).ok(Stage::Coherence);
        {
            let mut d = lock(&self.diags);
            for (item, msg) in found {
                d.error(Code::OverlappingImpl, self.item_span(item, 0), &msg);
            }
        }
        for (&f, r) in folders.iter().zip(runs) {
            let _ = self.init_runs[f].set(r);
            lock(&self.report).ok(Stage::InitOrder);
        }
        true
    }

    /// The overlap check (`trait.overlap.definition`,
    /// `trait.overlap.error`) for the impls of module `m`'s `tests:` block,
    /// which are module items in the test role (`names.tests.module-items`)
    /// but in no folder interface, so [`Self::coherence`] never sees them.
    /// Each is checked against the impls of `ifaces`, the test role's
    /// closure (`test_closure(m)`), and the block's other impls. The
    /// findings go into the module's test-role `check` entry, whose key
    /// holds those interfaces; the library role never runs this.
    fn test_overlaps(
        &self,
        m: usize,
        ifaces: &[Arc<FolderIface>],
        (items, anchors): (&[Item], &hd_resolve::anchor::Anchors),
        src: &Src<'_>,
        diags: &mut DiagBuf,
    ) {
        let Some(own) = self.iface_of(self.table.modules[m].folder) else {
            return;
        };
        let test_impls: Vec<&Item> = items
            .iter()
            .filter(|i| matches!(i.data, ItemData::Impl { .. }) && own.item(i.def).is_none())
            .collect();
        if test_impls.is_empty() {
            return;
        }
        let test_only: HashSet<DefId> = test_impls.iter().map(|i| i.def).collect();
        let names = self.names();
        let u = hd_resolve::Universe::new(
            names,
            ifaces
                .iter()
                .flat_map(|i| i.items.iter())
                .chain(test_impls.iter().copied()),
        );
        let anchor = |d: DefId| {
            anchors
                .iter()
                .find(|(def, slot, _)| *def == d && *slot == 0)
                .map(|(_, _, a)| *a)
        };
        // Content order: the block's impls come after every impl the test
        // code sees, in source order, so the later impl reported is the
        // block's.
        let order = |d: DefId| {
            let at = anchor(d).map_or((0, 0, 0), |a| (a.decl, a.member, a.tok));
            format!(
                "{}\0{:010}{:010}{:010}\0{}",
                u8::from(test_only.contains(&d)),
                at.0,
                at.1,
                at.2,
                names.path(d)
            )
        };
        let fallback = Span {
            file: src.file,
            lo: 0,
            hi: 0,
        };
        for (a, b, witness) in u.overlaps(&order) {
            if !test_only.contains(&b) {
                continue;
            }
            let span = anchor(b)
                .and_then(|at| hd_resolve::anchor::resolve(src, &at))
                .unwrap_or(fallback);
            let msg = format!(
                "implementations {} and {} both apply to {witness}",
                names.path(a),
                names.path(b)
            );
            diags.error(Code::OverlappingImpl, span, &msg);
        }
    }

    /// The fuel of each body of module `m`: the test's budget for the
    /// root package ([`Packages::body_fuel`]), else `Fuel::BODY_DEFAULT`.
    fn fuel_of(&self, m: usize) -> u64 {
        self.body_fuel
            .filter(|_| self.table.modules[m].package == 0)
            .unwrap_or(hd_base::Fuel::BODY_DEFAULT)
    }

    fn check_key(&self, m: usize) -> Option<Hash128> {
        let module = &self.table.modules[m];
        let templates = self.template_sources(m)?;
        let mut closure = Vec::new();
        for c in self.module_closure(m) {
            closure.push((
                self.table.folders[c.idx()].path.as_str(),
                self.iface_of(c)?.deep_hash,
            ));
        }
        closure.extend(templates.iter().map(|(p, h)| (p.as_str(), *h)));
        // A REPL input sees `src/lib.hd`'s private declarations, which no
        // interface carries (`cli.repl.package.lib`).
        if let Some(lib) = self.session_lib(m) {
            closure.push(("$lib", self.skim_of(lib).file.token_hash));
        }
        // Nor does one carry an earlier input's bindings, whose types come
        // from its statements (`cli.repl.declarations.bindings`).
        for &(_, k) in self.earlier_inputs(m) {
            closure.push(("$input", self.skim_of(k).file.token_hash));
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
        // An entry module's check holds its entry-point findings, and a
        // module's check holds whether its top level ran as entry
        // behavior, which the goal decides.
        let mut role = self.role(m).to_owned();
        if self.entry_module(m) {
            role.push_str("+entry");
        }
        if self.runs_entry_behavior(m) {
            role.push_str("+entry-behavior");
        }
        if let Some(fuel) = self.body_fuel.filter(|_| module.package == 0) {
            role.push_str("+fuel=");
            role.push_str(&fuel.to_string());
        }
        Some(check_key(
            self.toolchain,
            package,
            &module.path,
            &role,
            self.skim_of(m).file.token_hash,
            &closure,
        ))
    }

    /// The template text hashes of the template modules a module's
    /// opt-ins may instantiate. An opt-in checks its template's body, which
    /// no interface carries yet (spec 14 `annot.limit.interfaces`), so an
    /// edit to that body, or to a header or inferred signature it reads,
    /// must reach the key of every module that opts in to it. A module opts
    /// in to a derivation template only when its text names `derive` or
    /// `Structure` and a name its trait goes by. Any module may use a
    /// tuple template, whose member steps come from its body
    /// (`crates/hd_check/DESIGN.md` (trait solver) §3.9). std's sources
    /// are in the toolchain key already.
    fn template_sources(&self, m: usize) -> Option<Vec<(String, Hash128)>> {
        let text = &self.texts[m];
        let opts_in = text.contains("derive") || text.contains("Structure");
        let closure = self.module_closure(m);
        self.template_hashes(&closure, &|trait_| {
            opts_in && self.names_trait(&closure, text, trait_)
        })
    }

    /// Whether `text` holds a name the trait `trait_` goes by in
    /// `closure`: its declared name, or the name of a `pub use` of it.
    fn names_trait(&self, closure: &[FolderId], text: &str, trait_: DefId) -> bool {
        let names = self.names();
        text.contains(names.display_name(trait_))
            || closure.iter().filter_map(|c| self.iface_of(*c)).any(|i| {
                i.exports
                    .iter()
                    .any(|e| e.def == trait_ && text.contains(names.text(e.name)))
            })
    }

    /// The template text hashes of the user tuple-template modules in
    /// `closure(folder)`: what `HeaderCheck(folder)` reads through a
    /// template's member steps, which come from its body and from nothing
    /// an interface carries (`crates/hd_check/DESIGN.md` (trait solver) §3.9). The walkers' bounds
    /// are in the interfaces of the key's reach already, so the template
    /// modules' text is all that is missing. std's templates are in the
    /// toolchain key.
    fn tuple_template_sources(&self, folder: FolderId) -> Option<Vec<(String, Hash128)>> {
        self.template_hashes(&self.closure(folder), &|_| false)
    }

    /// The template text hashes (`HeaderSkeleton::template_text_hash`) of
    /// the user template modules in `closure`: every module with a tuple
    /// template, and every module with a derivation template whose trait
    /// `wanted` takes. A body edit outside the template and outside every
    /// function whose signature it infers keeps the hash.
    fn template_hashes(
        &self,
        closure: &[FolderId],
        wanted: &dyn Fn(DefId) -> bool,
    ) -> Option<Vec<(String, Hash128)>> {
        let mut out = std::collections::BTreeMap::new();
        for &c in closure {
            let iface = self.iface_of(c)?;
            let templates = self.folder_templates[c.idx()].get_or_init(|| {
                let names = self.names();
                // From the item index: only a template's record is read (#213).
                let items = &iface.items;
                let mut ts: Vec<FolderTemplate> = (0..items.len())
                    .filter_map(|i| {
                        let (kind, _) = items.impl_kind(i)?;
                        let template = match kind {
                            hd_resolve::ImplKind::Template => true,
                            hd_resolve::ImplKind::TupleTemplate => false,
                            _ => return None,
                        };
                        let p = names.module_of(*items.defs().get(i)?);
                        if p.starts_with("std.") {
                            return None;
                        }
                        let trait_ = match items.get(i)?.data {
                            ItemData::Impl { trait_, .. } if template => Some(trait_),
                            _ => None,
                        };
                        Some((p, trait_))
                    })
                    .collect();
                ts.sort_by(|a, b| (&a.0, a.1.map(DefId::raw)).cmp(&(&b.0, b.1.map(DefId::raw))));
                ts.dedup();
                ts
            });
            for (p, trait_) in templates {
                if trait_.is_some_and(|t| !wanted(t)) {
                    continue;
                }
                if let Some(tm) = self.table.module(p) {
                    out.insert(
                        format!("template {p}"),
                        self.skim_of(tm.idx()).file.template_text_hash,
                    );
                }
            }
        }
        Some(out.into_iter().collect())
    }

    /// `ModulePrep(m)`: the `check` key lookup first; only on a miss does
    /// it lower headers and create `Body(m)` and `ModuleFinish(m)`
    /// (`crates/hd_sched/DESIGN.md` §6.1).
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
        // The scratch holds what the module's uses report, which the
        // folder's `HeaderCheck` reports to the user.
        let use_errors = (0..scratch.len()).any(|i| {
            scratch.severity[i] == Severity::Error
                && hd_resolve::USE_CODES.contains(&scratch.code[i])
        });
        let _ = self.prep[mi].set(Some(PrepOut {
            items,
            diags: DiagBuf::default(),
            use_errors,
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
        sp.edge(finish, pr);
        Some(finish)
    }

    /// Whether the module's bytes parse without a diagnostic: a `parse`
    /// entry says so, else the parse runs and a clean one is recorded.
    fn parses(&self, m: usize) -> bool {
        let key = parse_key(self.toolchain, self.skim_of(m).file.source_hash);
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
            starts: &self.skim_of(m).file.code_lines,
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

    /// A folder's impls, frozen with its interface (`crates/hd_check/DESIGN.md` (trait solver) §3.2,
    /// §3.3); `None` when the interface was not built.
    fn folder_impls(&self, f: FolderId) -> Option<&FolderImpls> {
        self.folder_impls[f.idx()]
            .get_or_init(|| {
                let iface = self.iface_of(f)?;
                // Only the impls' records are read (#213).
                let impls: Vec<&Item> = iface.items.impls().collect();
                let table = hd_resolve::impl_table(&self.names(), &impls);
                let fi = FolderImpls::new(f, table, self.pool.types(), &self.paths, self.owners());
                Some(Arc::new(fi))
            })
            .as_deref()
    }

    /// A solving context's impls (`crates/hd_check/DESIGN.md` (trait solver) §3.2): the closure's
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
    /// reported by the folder interface, before any body of it. Its uses'
    /// errors come from `HeaderCheck`, which no body waits for: a body
    /// reads them from its `ModulePrep` ([`PrepOut::use_errors`]).
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
        let closure = self.module_closure(m);
        let ifaces: Vec<Arc<FolderIface>> =
            closure.iter().filter_map(|f| self.iface_of(*f)).collect();
        // A REPL input also sees `src/lib.hd`'s private items
        // (`cli.repl.package.lib`), which no interface carries.
        // Only an input whose session holds the library asks: its closure
        // holds `src/lib.hd`'s folder, so that interface is built by now
        // ([`Self::lib_scope`]).
        let input = self.repl_number(m).is_some();
        let lib = if self.with_lib(m) {
            self.lib_scope()
        } else {
            None
        };
        let own: std::borrow::Cow<'_, [Item]> = match lib {
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
        let session = self.session_nodes(m);
        let cx = BodyCx {
            names,
            src,
            scope: &scope,
            lookup: &lookup,
            impls: &impls,
            global: &self.memo,
            solver: &solver,
            methods: std::cell::OnceCell::new(),
            ref_names: std::cell::OnceCell::new(),
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
            session: &session,
            fuel: self.fuel_of(m),
        };
        let mut diags = DiagBuf::default();
        hd_check::init::earlier_binding_clashes(&cx, &heads, &mut diags);
        // The `tests:` block's use and header diagnostics, which no folder
        // interface reports.
        diags.append(&lowered.test_diags);
        if tests {
            self.test_overlaps(
                m,
                &ifaces,
                (&lowered.items, &lowered.anchors),
                &src,
                &mut diags,
            );
        }
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
        // M1 (`crates/hd_tir/DESIGN.md` §4.13.1 "Omitted result types"): the
        // private callables without a result type are checked before
        // their callers, from the top-level statements on.
        let methods = hd_resolve::body_nodes(&names, &src, &heads);
        hd_check::results::omitted_results(&cx, &methods);
        // Top-level statements first: their bindings are visible to every
        // function body of the module (`crates/hd_tir/DESIGN.md` §4.13.10).
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
        let entry = self.runs_entry_behavior(m);
        // `module.init.script-empty`: an entry module with no public
        // `main` is a script even with no statements, and its empty init
        // is the whole program. A REPL input is always one. Every goal
        // gives it the same init, so a check's entry serves a run's.
        let empty_script =
            stmts.is_empty() && (input || (entry && self.skim_of(m).file.public_main.is_none()));
        if !stmts.is_empty() || empty_script {
            let item = hd_check::default_body_def(&names, names.item(&module.path, "init"), "init");
            let top = if input {
                hd_check::init::TopLevel::Input
            } else if entry && self.skim_of(m).file.public_main.is_none() {
                hd_check::init::TopLevel::Script
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
        // The bodies to check, in source order, each with what M1 checked
        // of it.
        let mut slots: Vec<BodySlot<'_>> = Vec::new();
        for (def, node) in methods {
            // A body-less method of a built-in family (`impl[N < Num] Add
            // for N`) is the compiler's: there is no source to check.
            if hd_resolve::Src::child(node, hd_syntax::SyntaxKind::Block).is_none() {
                continue;
            }
            // A method whose impl header was rejected has no item
            // (`unknown-trait`): that error is the answer, there is no
            // signature to check the body against.
            if lookup.item(def).is_none() && (prep.use_errors || self.file_has_errors(m)) {
                continue;
            }
            slots.push((def, node, hd_check::results::take(&cx, def)));
        }
        #[cfg(feature = "threads")]
        let mut early = self.bodies_in_parallel(&cx, &slots, diags.has_errors());
        for (k, (def, node, taken)) in slots.into_iter().enumerate() {
            #[cfg(feature = "threads")]
            let forked = early.get_mut(k).and_then(Option::take);
            #[cfg(not(feature = "threads"))]
            let forked: Option<ForkedBody> = {
                let _ = k;
                None
            };
            let checked = if let Some((checked, own)) = taken {
                diags.append(&own);
                checked
            } else if let Some((checked, own, left)) = forked {
                diags.append(&own);
                cx.join(left);
                checked
            } else {
                hd_check::check_fn(&cx, def, node, &mut diags)
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
        // A generic call cycle of the module's own bodies whose type
        // arguments grow (spec 04 `types.generic.polymorphic-recursion`).
        hd_check::inst_cycles::growing_cycles(&cx, &bodies, &mut diags);
        // The method bodies of the implementations `@error` generates
        // (spec 14 `annot.error.generates`, `crates/hd_mono/DESIGN.md` §13.14).
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
        // Erased newtypes forward `@derive` methods to their base
        // implementation directly: only the position findings are
        // reported here, and no adapter bodies are built (spec 09
        // `trait.derive.newtype.base`).
        for h in heads
            .iter()
            .filter(|h| h.kind == hd_resolve::HeadKind::Newtype)
        {
            if failed.is_some() && !self.analyze() {
                break;
            }
            match hd_check::newtype::derived_newtype_findings(&cx, h.def, h.node, &mut diags) {
                Ok(findings) => {
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
        // (`crates/hd_tir/DESIGN.md` "Default calls").
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
            .filter(|h| lookup.item(h.def).is_some_and(|item| item.has_facts))
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
            self.foreign_repeats(m, (&cx, &heads, &impls), h, &mut diags);
            self.foreign_drift(m, (&cx, &heads, &impls), h, &mut diags);
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
                ) && !hd_resolve::lower::misplaced_template(&names, cx.session, it)
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
        // (spec 14 `annot.template.checked`, `crates/hd_tir/DESIGN.md` §4.13.9),
        // whose bodies join the module's.
        let mut derived = Derived::default();
        if failed.is_none() {
            derived = self.opt_ins(m, &cx, &heads, &impls, &mut diags);
        }
        if let Some(e) = failed {
            // The module's other reports stand beside the stop.
            lock(&self.diags).append(&diags);
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

    /// The function bodies of `Body(m)` that can be checked on the pool,
    /// by slot (`crates/hd_tir/DESIGN.md` §4.13.1): each batch checks its
    /// bodies in order in a context forked from `cx`.
    ///
    /// A body sees whether the module already holds an error, which
    /// changes some of its reports (an error-recovery `never`,
    /// `cannot-infer-type`). So only the bodies before the module's first
    /// error are checked here, with the answer "no": the first body that
    /// reports an error, or fails in a build, ends the run, and the slots
    /// after it are `None`, for `Body(m)` to check in order as before.
    #[cfg(feature = "threads")]
    fn bodies_in_parallel(
        &self,
        cx: &BodyCx<'_>,
        slots: &[BodySlot<'_>],
        errors: bool,
    ) -> Vec<Option<ForkedBody>> {
        /// Below this many bodies, a module is checked on its own thread.
        const MIN_BODIES: usize = 32;
        /// The fewest bodies of a batch, which pays for one fork.
        const MIN_BATCH: usize = 8;
        let threads = hd_sched::task_threads();
        if threads < 2 || errors {
            return Vec::new();
        }
        let analyze = self.analyze();
        // The bodies before the first that M1 checked with an error.
        let mut todo = Vec::new();
        for (k, (def, node, taken)) in slots.iter().enumerate() {
            match taken {
                None => todo.push((k, *def, *node)),
                Some((checked, own)) if own.has_errors() || (checked.is_err() && !analyze) => {
                    break;
                }
                Some(_) => {}
            }
        }
        if todo.len() < MIN_BODIES {
            return Vec::new();
        }
        let fork = cx.fork();
        // The first slot known to end the run.
        let stop = std::sync::atomic::AtomicUsize::new(usize::MAX);
        let size = todo.len().div_ceil(threads * 4).max(MIN_BATCH);
        let batches: Vec<&[(usize, DefId, hd_syntax::NodeRef<'_>)]> = todo.chunks(size).collect();
        let done = hd_sched::par_map(&batches, |batch| {
            let bcx = fork.cx();
            let mut out = Vec::new();
            for &(k, def, node) in *batch {
                if k > stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let mut own = DiagBuf::default();
                let checked = hd_check::check_fn(&bcx, def, node, &mut own);
                if own.has_errors() || (checked.is_err() && !analyze) {
                    stop.fetch_min(k, std::sync::atomic::Ordering::Relaxed);
                }
                out.push((k, (checked, own, bcx.left())));
            }
            out
        });
        let mut early: Vec<Option<ForkedBody>> = (0..slots.len()).map(|_| None).collect();
        for (k, body) in done.into_iter().flatten() {
            let ends = body.1.has_errors() || (body.0.is_err() && !analyze);
            early[k] = Some(body);
            if ends {
                break;
            }
        }
        early
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
        let mut out = HashMap::default();
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
            session: cx.session,
            fuel: cx.fuel,
        };
        let Ok(findings) = hd_check::header::private_items(&hcx, cx.lookup.own) else {
            return;
        };
        for f in findings {
            emit_finding(diags, &f, |slot| {
                hd_resolve::anchor::span_in_module(
                    &cx.names,
                    &cx.src,
                    heads,
                    cx.lookup.own,
                    (f.item, slot),
                )
                .unwrap_or_else(|| self.item_span(f.item, slot))
            });
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
            session: cx.session,
            fuel: cx.fuel,
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
            session: cx.session,
            fuel: cx.fuel,
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
    /// template, in the template's module scope (`crates/hd_tir/DESIGN.md`
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
        let mut decl: HashMap<DefId, Option<FactBodies>> = HashMap::default();
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
            if hd_resolve::lower::misplaced_block(&names, cx.session, cx.lookup.own, it).is_some() {
                continue;
            }
            let template = hd_resolve::lower::template_of(&names, cx.session, cx.lookup, *trait_);
            // A derivation block needs a template, and a comparison trait is
            // derived only with `@derive` (`annot.block.template`). An
            // unimported `Structure` is already `unknown-trait`.
            let comparison =
                [names.known.eq, names.known.partial_ord, names.known.ord].contains(trait_);
            if *kind == hd_resolve::ImplKind::Derivation && (template.is_none() || comparison) {
                if hd_resolve::lower::names_std_structure(&names, cx.scope)
                    && let Some(h) = heads.iter().find(|h| h.def == it.def)
                {
                    let name = names.display_name(*trait_);
                    let why = if comparison {
                        "is derived only with `@derive`"
                    } else {
                        "has no template"
                    };
                    diags.error(
                        Code::UnderivableTrait,
                        cx.src.span(h.node),
                        &format!("`{name}` {why}, so a derivation block cannot derive it"),
                    );
                }
                continue;
            }
            let Some(template) = template else {
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
                // REPL inputs keep the facts visible when their derivation
                // was built. A later input's traitless block must not reuse
                // an earlier derivation's declaration Structure owner.
                opt.lines = !lines.is_empty() || self.repl_number(m).is_some();
                let facts = if heads.iter().any(|h| h.def == opt.data) {
                    derivation_facts(cx, heads, (&opt, &lines), &mut decl, &mut derived)
                } else {
                    self.foreign_facts(
                        m,
                        (cx, heads, impls),
                        &opt,
                        &lines,
                        (&mut decl, &mut derived),
                    )
                };
                groups.entry(tm.idx()).or_default().push((i, opt, facts));
            }
        }
        let mut found: Vec<OptInFinding> = Vec::new();
        for (tm, opts) in groups {
            if tm == m {
                found.extend(self.check_opt_ins(cx, heads, &opts, &mut derived));
                continue;
            }
            let own = cx.lookup.own;
            if let Some(f) = self.in_module(m, tm, (own, impls.own), |tcx, theads| {
                self.check_opt_ins(tcx, theads, &opts, &mut derived)
            }) {
                found.extend(f);
            }
        }
        // A derived newtype's base type must implement the trait.
        let hcx = hd_check::header::HeaderCx {
            names: cx.names,
            lookup: cx.lookup,
            impls: cx.impls,
            global: cx.global,
            solver: cx.solver,
            session: cx.session,
            fuel: cx.fuel,
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

    /// The facts bodies of a derivation whose target `opt.data` another
    /// module of the REPL session declares, as within one module: the
    /// type's declaration facts, built once per type into `decl` by
    /// [`Self::session_decl`]; and when the derivation's member `lines`
    /// edit them, the edited slots' own bodies, checked here in module
    /// `m`, whose context, heads and impl view are `cx`, `heads` and
    /// `impls`. `None` when a fact is not generated yet.
    fn foreign_facts<'t>(
        &self,
        m: usize,
        (cx, heads, impls): (&BodyCx<'t>, &[hd_resolve::Head<'t>], &ImplView<'_>),
        opt: &hd_check::derive::OptIn,
        lines: &[hd_check::derive::MemberLine<'t>],
        (decl, derived): (&mut HashMap<DefId, Option<FactBodies>>, &mut Derived),
    ) -> Option<FactBodies> {
        let data = opt.data;
        let base = decl
            .entry(data)
            .or_insert_with(|| self.session_decl(m, (cx, heads, impls), data, derived))
            .clone()?;
        if !opt.lines {
            return Some(base);
        }
        let pool = cx.names.pool;
        let item = cx.lookup.item(data)?;
        let TyData::Adt { args, .. } = pool.get(opt.target) else {
            return None;
        };
        let (mut view, edited) = hd_check::structure::layered_facts(cx, item, (&base, args), lines);
        view.retarget(cx, data, pool.list_items(args));
        let block = heads.iter().find(|h| h.def == opt.impl_)?.node;
        let (own, bodies) = hd_check::structure::fact_bodies(
            cx,
            (opt.impl_, ""),
            (&view, block),
            Some((&edited, &base)),
        )
        .ok()?;
        derived.bodies.extend(bodies);
        Some(own)
    }

    /// The repeats that the member lines of the block `h` of module `m`
    /// make in a type that another module of the REPL session declares:
    /// the facts of its declaration, edited by the block with no trait
    /// when another module holds it, then by `h`, as within one module
    /// (`annot.line.duplicate`). Each earlier layer is typed in its own
    /// module, which `h`'s lines then repeat (`duplicate-fact`).
    fn foreign_repeats<'t>(
        &self,
        m: usize,
        (cx, heads, impls): (&BodyCx<'t>, &[hd_resolve::Head<'t>], &ImplView<'_>),
        h: &hd_resolve::Head<'t>,
        diags: &mut DiagBuf,
    ) {
        let names = self.names();
        let pool = cx.names.pool;
        let Some(ItemData::Impl {
            by: Some(_),
            self_ty,
            ..
        }) = cx.lookup.item(h.def).map(|i| &i.data)
        else {
            return;
        };
        let TyData::Adt { def: data, .. } = pool.get(*self_ty) else {
            return;
        };
        if heads.iter().any(|x| x.def == data)
            || !cx.session.contains(&names.module_node(data))
            || hd_check::derive::member_lines(&cx.src, h.node).is_empty()
        {
            return;
        }
        let Some(dm) = self
            .table
            .module(&names.module_of(data))
            .map(hd_base::ModuleId::idx)
        else {
            return;
        };
        let Some(mut prior) = self
            .in_module(m, dm, (&[], impls.own), |dcx, dheads| {
                let item = dcx.lookup.item(data)?;
                let node = dheads.iter().find(|x| x.def == data)?.node;
                let view = hd_check::structure::declaration_facts(
                    dcx,
                    item,
                    node,
                    traitless(dcx, dheads, data),
                );
                Some(hd_check::declaration_types(dcx, data, &view))
            })
            .flatten()
        else {
            return;
        };
        // The block with no trait is a layer of its own when it is another
        // module's, and not `h` itself.
        let block = cx
            .lookup
            .own
            .iter()
            .chain(cx.lookup.ifaces.iter().flat_map(|f| &f.items))
            .find(|it| {
                traitless_for(pool, it, data) && cx.session.contains(&names.module_node(it.def))
            })
            .filter(|it| it.def != h.def);
        if let Some(block) = block
            && let Some(bm) = self
                .table
                .module(&names.module_of(block.def))
                .map(hd_base::ModuleId::idx)
            && bm != dm
        {
            let layer = |bcx: &BodyCx<'_>, bheads: &[hd_resolve::Head<'_>]| {
                let bh = bheads.iter().find(|x| x.def == block.def)?;
                hd_check::layer_types(bcx, bh, &prior, None)
            };
            let layered = if bm == m {
                layer(cx, heads)
            } else {
                self.in_module(m, bm, (&[], impls.own), layer).flatten()
            };
            let Some(layered) = layered else {
                return;
            };
            prior = layered;
        }
        hd_check::layer_types(cx, h, &prior, Some(diags));
    }

    /// `annot.line.drift` across the REPL session: the block `h` of input
    /// `m` for a trait of one package on a type warns when its member
    /// lines differ from the first block for that package and type, which
    /// an earlier module of the session holds, as within one module. The
    /// blocks of `m` itself are compared by lowering, so a block that is
    /// not `m`'s first for the package and type, and already differs from
    /// that first, is not reported twice.
    fn foreign_drift<'t>(
        &self,
        m: usize,
        (cx, heads, impls): (&BodyCx<'t>, &[hd_resolve::Head<'t>], &ImplView<'_>),
        h: &hd_resolve::Head<'t>,
        diags: &mut DiagBuf,
    ) {
        let names = self.names();
        let pool = cx.names.pool;
        let block_of = |it: &Item| match &it.data {
            ItemData::Impl {
                trait_,
                by: Some(_),
                self_ty,
                ..
            } if *trait_ != DefId::NONE => match pool.get(*self_ty) {
                TyData::Adt { def, .. } => Some((*trait_, def)),
                _ => None,
            },
            _ => None,
        };
        let Some((trait_, data)) = cx.lookup.item(h.def).and_then(block_of) else {
            return;
        };
        let package = |d: DefId| {
            names
                .module_of(d)
                .split('.')
                .next()
                .unwrap_or("")
                .to_owned()
        };
        let same = |it: &Item| {
            block_of(it).is_some_and(|(t, d)| d == data && package(t) == package(trait_))
        };
        // The earlier modules' blocks, grouped by module.
        let mut earlier: Vec<(usize, Vec<DefId>)> = Vec::new();
        for it in cx
            .lookup
            .own
            .iter()
            .chain(cx.lookup.ifaces.iter().flat_map(|f| &f.items))
            .filter(|it| same(it) && cx.session.contains(&names.module_node(it.def)))
        {
            let Some(bm) = self
                .table
                .module(&names.module_of(it.def))
                .map(hd_base::ModuleId::idx)
            else {
                continue;
            };
            if bm == m {
                continue;
            }
            match earlier.iter_mut().find(|(k, _)| *k == bm) {
                Some((_, defs)) => defs.push(it.def),
                None => earlier.push((bm, vec![it.def])),
            }
        }
        let Some((fm, defs)) = earlier
            .into_iter()
            .min_by_key(|(k, _)| self.repl_number(*k))
        else {
            return;
        };
        let Some((first_trait, first)) = self
            .in_module(m, fm, (&[], impls.own), |fcx, fheads| {
                let fh = fheads.iter().find(|x| defs.contains(&x.def))?;
                let (t, _) = fcx.lookup.item(fh.def).and_then(block_of)?;
                Some((t, drift_signature(&fcx.src, fh.node)))
            })
            .flatten()
        else {
            return;
        };
        let mine = drift_signature(&cx.src, h.node);
        if mine == first {
            return;
        }
        // Within `m`, a later block that differs from `m`'s first block is
        // already reported by lowering.
        let own_first = heads
            .iter()
            .find(|x| cx.lookup.item(x.def).is_some_and(same));
        if let Some(of) = own_first
            && of.def != h.def
            && drift_signature(&cx.src, of.node) != mine
        {
            return;
        }
        let last = |d: DefId| names.path(d).rsplit('/').next().unwrap_or("").to_owned();
        let msg = format!(
            "`{}` configures different member lines for `{}` than `{}` does",
            last(trait_),
            last(data),
            last(first_trait)
        );
        diags.push(
            Code::DerivationLineDrift,
            Severity::Warning,
            cx.src.span(h.node),
            &msg,
            None,
        );
    }

    /// The declaration facts bodies of `data`, which another module of
    /// REPL input `m`'s session declares (`annot.fact.shared`): built in
    /// its own module, then edited by the type's block with no trait when
    /// another module of the session holds that block, as within one
    /// module (`annot.no-trait.declaration-facts`). That block's bodies
    /// are hidden methods of `data` too, apart from the declaration's own,
    /// which a derivation in the type's module already calls. `None` when
    /// a fact is not generated yet.
    fn session_decl<'t>(
        &self,
        m: usize,
        (cx, heads, impls): (&BodyCx<'t>, &[hd_resolve::Head<'t>], &ImplView<'_>),
        data: DefId,
        derived: &mut Derived,
    ) -> Option<FactBodies> {
        let names = self.names();
        let dm = self.table.module(&names.module_of(data))?.idx();
        let base = self
            .in_module(m, dm, (&[], impls.own), |dcx, dheads| {
                declaration_bodies(dcx, dheads, data, derived)
            })
            .flatten()?;
        // The type has at most one block with no trait
        // (`annot.no-trait.unique`), in a module of the session
        // (`annot.no-trait.module`).
        let pool = cx.names.pool;
        let block = cx
            .lookup
            .own
            .iter()
            .chain(cx.lookup.ifaces.iter().flat_map(|f| &f.items))
            .find(|it| {
                traitless_for(pool, it, data) && cx.session.contains(&names.module_node(it.def))
            });
        let Some(block) = block else {
            return Some(base);
        };
        let bm = self.table.module(&names.module_of(block.def))?.idx();
        if bm == dm {
            return Some(base);
        }
        let layer = |bcx: &BodyCx<'_>, bheads: &[hd_resolve::Head<'_>], derived: &mut Derived| {
            let node = bheads.iter().find(|h| h.def == block.def)?.node;
            let lines = hd_check::derive::member_lines(&bcx.src, node);
            let item = bcx.lookup.item(data)?;
            let args = hd_check::structure::own_args(bcx, data);
            let (view, edited) =
                hd_check::structure::layered_facts(bcx, item, (&base, args), &lines);
            let (b, bodies) = hd_check::structure::fact_bodies(
                bcx,
                (data, "block "),
                (&view, node),
                Some((&edited, &base)),
            )
            .ok()?;
            derived.bodies.extend(bodies);
            Some(b)
        };
        if bm == m {
            return layer(cx, heads, derived);
        }
        self.in_module(m, bm, (&[], impls.own), |bcx, bheads| {
            layer(bcx, bheads, derived)
        })
        .flatten()
    }

    /// Runs `f` in the context of module `tm`, as module `m` sees it: its
    /// scope, source and heads, with `own` (module `m`'s items and impls)
    /// and `tm`'s items as the items, and for a REPL input `tm`,
    /// `src/lib.hd`'s private items, as its own bodies see them
    /// (`cli.repl.package.lib`). `None` when `tm` does not lower.
    fn in_module<R>(
        &self,
        m: usize,
        tm: usize,
        (own, own_impls): (&[Item], Option<&hd_types::solver::ImplTable>),
        f: impl for<'a> FnOnce(&BodyCx<'a>, &[hd_resolve::Head<'a>]) -> R,
    ) -> Option<R> {
        let names = self.names();
        let tmod = self.template_mods[tm]
            .get_or_init(|| {
                self.lower_module(tm, Stage::Body, &mut DiagBuf::default())
                    .ok()
                    .flatten()
            })
            .as_ref()?;
        let mut closure = self.module_closure(m);
        for fo in self.closure(self.table.modules[tm].folder) {
            if !closure.contains(&fo) {
                closure.push(fo);
            }
        }
        let ifaces: Vec<Arc<FolderIface>> =
            closure.iter().filter_map(|fo| self.iface_of(*fo)).collect();
        let lib = if self.with_lib(tm) {
            self.lib_scope().map_or(&[][..], |l| l.private.as_slice())
        } else {
            &[]
        };
        let held: HashSet<DefId> = own.iter().map(|o| o.def).collect();
        let items: Vec<Item> = own
            .iter()
            .chain(&tmod.items)
            .chain(lib.iter().filter(|i| !held.contains(&i.def)))
            .cloned()
            .collect();
        let lookup = Lookup::new(&items, ifaces.iter().map(AsRef::as_ref).collect());
        let (folders, universe, extra) = self.closure_impls(&closure);
        let arity = |d: DefId| lookup.item(d).map_or(0, |i| i.generics.len());
        let view = ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: own_impls,
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
        let session = self.session_nodes(tm);
        let tcx = BodyCx {
            names,
            src,
            scope: &tmod.scope,
            lookup: &lookup,
            impls: &view,
            global: &self.memo,
            solver: &TableSolver,
            methods: std::cell::OnceCell::new(),
            ref_names: std::cell::OnceCell::new(),
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
            results: std::cell::RefCell::default(),
            dbg_refs: std::cell::RefCell::default(),
            locals: &tlocals,
            tests: None,
            file_name: &self.table.files[tm],
            session: &session,
            fuel: self.fuel_of(tm),
        };
        Some(f(&tcx, &theads))
    }

    /// A tuple template's member steps (`crates/hd_check/DESIGN.md` (trait solver) §3.9): its
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
            ref_names: std::cell::OnceCell::new(),
            init: std::cell::RefCell::new(hd_check::init::ModuleInit::default()),
            results: std::cell::RefCell::default(),
            dbg_refs: std::cell::RefCell::default(),
            locals: &tlocals,
            tests: None,
            file_name: &self.table.files[tm],
            session: &[],
            fuel: self.fuel_of(tm),
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
        let mut seen = hd_base::HashSet::default();
        steps.retain(|s| seen.insert(s.clone()));
        Some(Arc::from(steps))
    }

    /// Checks opt-ins whose template module `tcx` sees: each checks the
    /// template methods its block does not write (`annot.block.methods`).
    /// A clean opt-in adds its derive instances and, once per owner, the
    /// bodies of the `Structure` it sees to `derived` (`crates/hd_mono/DESIGN.md` §12.3);
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
    /// meta (the module's content hash, which `prog_key` reads); the
    /// module's items; TIR, one record per body; a test run's
    /// registrations; the init summary, which `InitOrder` reads; the `dbg`
    /// references; and each body's frame name (`positions`).
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
            // The row's fix-its (entry layout 6): each title, safety,
            // and its edits as spans with replacement text.
            let fixes = diags.fixes[i];
            dw.u32(fixes.len);
            for f in fixes.start..fixes.start + fixes.len {
                let f = f as usize;
                dw.str(diags.get_text(diags.fix_title[f]));
                dw.u8(match diags.fix_safety[f] {
                    hd_diag::FixSafety::Exact => 0,
                    hd_diag::FixSafety::Suggestion => 1,
                    hd_diag::FixSafety::Migration => 2,
                });
                let edits = diags.fix_edits[f];
                dw.u32(edits.len);
                for e in &diags.edits[edits.range()] {
                    lines.write(&mut dw, e.span.lo);
                    lines.write(&mut dw, e.span.hi);
                    dw.str(diags.get_text(e.text));
                }
            }
        }
        // Invariant 3 (`crates/hd_tir/DESIGN.md` §4.13.11): `Hole` and
        // `Poison` only where something reported an error. A header's
        // error is the folder's, reported before its modules finish, so
        // the run's errors so far stand in for the module's. An import's
        // is the folder's `HeaderCheck`'s, which may finish later: its
        // `ModulePrep` saw it.
        if !diags.has_errors()
            && !prep.use_errors
            && !lock(&self.diags).has_errors()
            && let Some(b) = bodies
                .iter()
                .find(|b| !hd_tir::ir::poison_sites(b).is_empty())
        {
            self.stage::<()>(
                Stage::ModuleFinish,
                Err(NotImplemented::bug(
                    Stage::ModuleFinish,
                    format!(
                        "TIR verifier: invariant 3: poison in `{}`, in a module without errors",
                        names.path(b.item)
                    ),
                )),
            );
            let _ = self.check[m].set(None);
            return;
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
            // Each body under its item's path hash: `Collect` indexes the
            // bodies by it and decodes only the ones it reaches (#49).
            let item = names.path_hash(b.item);
            content.hash(item);
            content.hash(hd_tir::wire::tir_hash(&bytes));
            tw.hash(item);
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
        // code key and its callers' hold its row's keys (`crates/hd_mono/DESIGN.md` §12.4).
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
        // The link reads the declarations as well as the bodies: a trait's
        // methods are a handle's imports and a data type's fields its
        // boundary codec, and a test run's registrations pick its cases.
        // An edit to them can leave every body's TIR as it was (#316).
        let mut ih = hd_base::StableHasher::new("items");
        ih.bytes(&items);
        content.hash(ih.finish());
        content.hash(regs_hash(regs));
        let mut meta = Writer::default();
        meta.hash(content.finish());
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
        let mut fw = Writer::default();
        let body_items: Vec<DefId> = keep.iter().map(|b| b.item).collect();
        positions::write_frame_names(&mut fw, &self.paths, &body_items, &|d| {
            let item = all_items.iter().find(|i| i.def == d)?;
            let ItemData::Impl { self_ty, .. } = &item.data else {
                return None;
            };
            Some(positions::self_type_name(&names, &item.generics, *self_ty))
        });
        let mut sections: Vec<&[u8]> = vec![
            &dw.bytes,
            &meta.bytes,
            &items,
            &tw.bytes,
            &rw.bytes,
            &summary,
            &gw.bytes,
            &fw.bytes,
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

    /// Report outside the cached check entry, once per fetched package,
    /// including when every module or the whole program is cached.
    fn dbg_in_dependencies(&self) {
        let mut warned = BTreeSet::new();
        for (m, module) in self.table.modules.iter().enumerate() {
            let package = usize::from(module.package);
            if !self.fetched.get(package).copied().unwrap_or(false) {
                continue;
            }
            let Some(reference) = self.dbg_refs[m].get().and_then(|refs| refs.first()) else {
                continue;
            };
            if warned.insert(package) {
                lock(&self.diags).push(
                    Code::DbgInDependency,
                    Severity::Warning,
                    reference.span,
                    &format!(
                        "fetched package `{}` contains `dbg`; its debug output is suppressed",
                        self.table.packages[package]
                    ),
                    None,
                );
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
        self.keep_input(m, |e| &mut e.check, || sections.to_vec());
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
            // The row's fix-its (entry layout 6).
            for _ in 0..r.count() {
                let title = r.str();
                let safety = match r.u8() {
                    0 => hd_diag::FixSafety::Exact,
                    2 => hd_diag::FixSafety::Migration,
                    _ => hd_diag::FixSafety::Suggestion,
                };
                let mut edits = Vec::new();
                for _ in 0..r.count() {
                    let (lo, hi) = (lines.read(&mut r), lines.read(&mut r));
                    let text = r.str();
                    edits.push((
                        Span {
                            file: FileId::from_raw(u32_of(m)),
                            lo,
                            hi,
                        },
                        text,
                    ));
                }
                buf.add_fix(title, safety, &edits);
            }
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
        if let Some(b) = sections.get(8) {
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
        let kept = self.kept.get();
        let folders: Vec<usize> = (0..self.table.folders.len())
            .filter(|f| !kept.is_some_and(|k| k[*f]) && self.builds(FolderId::from_raw(u32_of(*f))))
            .collect();
        // Coherence and every folder's init order are parts of one `graph`
        // entry: a hit runs neither. Coherence runs here, after every
        // interface and check, so the entry's key holds both kinds of part.
        let orders: Vec<TaskId> = if self.graph_hit(&folders) {
            Vec::new()
        } else {
            let mut t = vec![sp.add(TaskKind::Coherence, &[])];
            t.extend(
                folders
                    .iter()
                    .map(|f| sp.add(TaskKind::InitOrder(u32_of(*f)), &[])),
            );
            t
        };
        self.dbg_in_release();
        self.dbg_in_dependencies();
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
        let mut items = HashMap::default();
        let mut bodies: HashMap<Hash128, LazyBody> = HashMap::default();
        let mut test_role_impls = Vec::new();
        for m in 0..self.table.modules.len() {
            let Some(Some(c)) = self.check[m].get() else {
                if self.table.modules[m].package != 0 || !self.builds_module(m) {
                    continue;
                }
                return None;
            };
            if c.has_errors {
                return None;
            }
            let sections = split_sections(&c.entry)?;
            let test_role = self.role(m) == "test";
            for it in hd_resolve::decode_items(&names, sections.get(2)?)?.0 {
                if test_role && matches!(it.data, ItemData::Impl { .. }) {
                    test_role_impls.push(it.def);
                }
                items.insert(it.def, it);
            }
            // An earlier REPL input's bodies yield to this build's.
            let kept = self.is_kept(m);
            for (item, at) in body_index(&c.entry)? {
                let lazy = LazyBody {
                    wire: Some((m, c.entry.clone(), at)),
                    body: OnceLock::new(),
                };
                if kept {
                    bodies.entry(item).or_insert(lazy);
                } else {
                    bodies.insert(item, lazy);
                }
            }
        }
        for f in 0..self.table.folders.len() {
            if let Some(i) = self.iface_of(FolderId::from_raw(u32_of(f))) {
                for it in &i.items {
                    items.entry(it.def).or_insert_with(|| it.clone());
                }
            }
        }
        let mut impl_rows = HashMap::default();
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
        // A `tests:` block's impls are in its module's test-role entry but
        // in no folder interface, which is built without test code.
        let test_impls: Vec<&Item> = test_role_impls
            .iter()
            .filter(|d| !impl_rows.contains_key(*d))
            .filter_map(|d| items.get(d))
            .collect();
        let test_impls = hd_resolve::impl_table(&names, &test_impls);
        for (row, d) in test_impls.def.iter().enumerate() {
            impl_rows.insert(
                *d,
                ImplRef {
                    module: OWN_TABLE,
                    row: u32_of(row),
                },
            );
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
                    let Some(Some((whole, tir))) =
                        bodies.get(&names.path_hash(init)).map(|b| self.decode(b))
                    else {
                        continue;
                    };
                    let body = hd_check::init::init_segment(whole, def, r.insts.clone());
                    let mut k = hd_base::StableHasher::new("init-run");
                    k.hash(*tir);
                    k.u32(r.start);
                    k.u32(r.end);
                    bodies.insert(names.path_hash(def), LazyBody::ready(body, k.finish()));
                }
            }
        }
        let (universe, extra) = self.universes.intern(&members);
        let _ = self.program.set(ProgramTables {
            items,
            bodies: Bodies {
                all: bodies,
                bad: std::sync::atomic::AtomicBool::new(false),
            },
            impl_rows,
            universe,
            extra,
            test_impls,
            recursive_enums: OnceLock::new(),
        });
        self.program.get()
    }

    /// The TIR body of `def`, and its content hash, decoded on first use
    /// ([`Bodies`]).
    fn body_of<'p>(&self, p: &'p ProgramTables, def: DefId) -> Option<&'p (Body, Hash128)> {
        let b = p
            .bodies
            .all
            .get(&self.paths.hash(hd_base::PathId::from_raw(def.raw())))?;
        let out = self.decode(b);
        if out.is_none() {
            p.bodies
                .bad
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        out
    }

    /// A body, decoding its wire bytes on first use.
    fn decode<'p>(&self, b: &'p LazyBody) -> Option<&'p (Body, Hash128)> {
        b.body
            .get_or_init(|| {
                let (m, entry, at) = b.wire.as_ref()?;
                let bytes = entry.get(at.clone())?;
                let mut body = hd_tir::wire::read_body(bytes, &self.pool, &self.paths, &self.syms)?;
                let mut hash = hd_tir::wire::tir_hash(bytes);
                if self.fetched[usize::from(self.table.modules[*m].package)]
                    && dbg::suppress(&mut body, &self.names())
                {
                    hash = dbg::key(hash);
                }
                let mut c = lock(&self.counters);
                c.bodies_decoded += 1;
                c.tir_decoded.push(self.table.modules[*m].path.clone());
                Some((body, hash))
            })
            .as_ref()
    }

    /// Every folder's impls, by folder id: the program's tables.
    fn program_folders(&self) -> Vec<Option<&FolderImpls>> {
        (0..self.table.folders.len())
            .map(|f| self.folder_impls(FolderId::from_raw(u32_of(f))))
            .collect()
    }

    /// The program's view of the impls (`crates/hd_check/DESIGN.md` (trait solver) §3.2): owner
    /// lookup over every folder, the `tests:` blocks' impls of a test
    /// program as its own table ([`ProgramTables::test_impls`]), the
    /// program's universe.
    fn program_view<'a>(
        &'a self,
        p: &'a ProgramTables,
        folders: &'a [Option<&'a FolderImpls>],
        arity: &'a (dyn Fn(DefId) -> usize + Sync),
    ) -> ImplView<'a> {
        ImplView {
            paths: &self.paths,
            owners: self.owners(),
            own: (!p.test_impls.def.is_empty()).then_some(&p.test_impls),
            folders,
            universe: p.universe,
            extra: &p.extra,
            arity,
            hidden: &[],
            plans: Some(self),
        }
    }

    /// The scenario hold target: the held function's item, when the
    /// program checks its body. Held polls are stubbed at emission and
    /// never touch the code cache, so a held build shares nothing with
    /// an unheld one.
    fn scenario_hold(&self, p: &ProgramTables) -> Option<(DefId, bool)> {
        let Goal::Scenario { entry, hold } = &self.goal else {
            return None;
        };
        let held = hold.as_ref()?;
        let def = self.names().item(
            &format!("{}.{entry}", hd_project::package_ident(&self.package)),
            &held.func,
        );
        self.body_of(p, def).is_some().then_some((def, held.active))
    }

    /// `Collect` (`crates/hd_mono/DESIGN.md` §11.3): `prog_key`, then instances, then code
    /// keys; hits are looked up here, and only misses become `Emit` tasks.
    /// A program's roots are `main` (with its report), or for a test run
    /// each running case (`docs/compiler/engines-and-test-runner.md` §19.1).
    fn collect(&self, sp: &dyn Spawn) {
        // `InitOrder` may have found an initialization cycle.
        if lock(&self.diags).has_errors() {
            return;
        }
        let names = self.names();
        let (entry_key, roots) = match &self.goal {
            Goal::Program { entry } | Goal::Scenario { entry, .. } => {
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
                Some(Some(c)) if counted => {
                    let hash = if self.fetched[usize::from(module.package)]
                        && self.dbg_refs[m].get().is_some_and(|refs| !refs.is_empty())
                    {
                        dbg::key(c.content)
                    } else {
                        c.content
                    };
                    modules.push((module.path.as_str(), hash));
                }
                _ if module.package == 0 && self.builds_module(m) => {
                    self.blocked(Stage::Collect);
                    return;
                }
                _ => {}
            }
        }
        // A scenario links different exports (`hd.cancel`) from a program
        // of the same entry, so its link key names the goal; a hold
        // changes the held call's code, so the key names it too, stub
        // variant included.
        let keyed_entry = match &self.goal {
            Goal::Scenario {
                hold: Some(held), ..
            } => format!(
                "scenario:hold={}:active={}:{}",
                held.func, held.active, entry_key
            ),
            Goal::Scenario { .. } => format!("scenario:{entry_key}"),
            _ => entry_key.clone(),
        };
        let keyed_entry = match self.backend {
            Backend::WasmGc => keyed_entry,
            Backend::Linear => format!("linear:{keyed_entry}"),
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
                Err(NotImplemented::bug(
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
            .is_none_or(|m| self.skim_of(m).file.public_main.is_some());
        let has_main = self.body_of(p, main).is_some()
            && (public || !matches!(self.goal, Goal::Program { .. } | Goal::Scenario { .. }));
        let script = match self.goal {
            Goal::Program { .. } | Goal::Scenario { .. } => !has_main,
            Goal::ReplInput { .. } => true,
            Goal::Tests { .. } | Goal::Analyze | Goal::CheckTests => false,
        };
        let root = if script { init_of(&entry_key) } else { main };
        if self.body_of(p, root).is_none() {
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
        // An earlier REPL input's init ran in its own build: collection
        // would leave it out (`Session::inits_run`), so it is left out
        // here, before its bodies are decoded.
        let kept_module = |m: &str| self.table.module(m).is_some_and(|k| self.is_kept(k.idx()));
        let has_init = |m: &str| !kept_module(m) && self.body_of(p, init_of(m)).is_some();
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
        // The entry module's own init bodies, in order: its whole init, or
        // one body per run when its module cycle's statements split it.
        // Only they may `block_on`, and a script runs them with its entry
        // providers.
        let mut own: Vec<DefId> = Vec::new();
        // A joined build's one root is its last input, whose init order
        // every input's inits are split from.
        let mut order: Vec<(DefId, &str)> = Vec::new();
        for m in &root_modules {
            let defs = match self.root_init_order(m, &init_of, &has_init) {
                Ok(o) => o,
                Err(e) => {
                    self.stage::<()>(Stage::Collect, Err(e));
                    return;
                }
            };
            if JOINED_INPUTS && self.repl_inputs.joined {
                order.clone_from(&defs);
            }
            for (d, module) in &defs {
                if !all_inits.contains(d) {
                    all_inits.push(*d);
                    if *module == entry_key {
                        own.push(*d);
                    }
                }
            }
            per_module.push(defs.into_iter().map(|(d, _)| d).collect());
        }
        // A script's root is its first own body; the rest run after it.
        let root = match own.first() {
            Some(first) if script => *first,
            _ => root,
        };
        // Extra roots: the other cases, then each distinct report function.
        let mut extra: Vec<(DefId, TyList)> = roots[1..]
            .iter()
            .map(|(d, _)| (*d, TyList::EMPTY))
            .collect();
        // A scenario's cleanup function is an extra root, so its instance
        // is collected even though no case calls it.
        if matches!(self.goal, Goal::Scenario { .. }) {
            let def = names.item(&entry_key, "cleanup_ran");
            if self.body_of(p, def).is_some() {
                extra.push((def, TyList::EMPTY));
            }
        }
        let reports: Vec<Option<(DefId, TyList)>> = roots
            .iter()
            .enumerate()
            .map(|(i, (d, _))| {
                let d = if script && i == 0 { &root } else { d };
                // A REPL input reports no result. A script's result is
                // `Result[void, dyn Error]` when its top level uses `?`
                // (`module.init.script-try`), else `void`: its entry is
                // its init.
                if matches!(self.goal, Goal::ReplInput { .. }) {
                    return None;
                }
                self.report_fn(env.ret(*d).unwrap_or(Ty::VOID))
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
        let collected = hd_mono::collect(
            &self.pool,
            &env,
            &TableSolver,
            root,
            &all_inits,
            &extra,
            &prelude.refs,
            session,
            &self.planner,
        );
        // Bodies decode as collection reaches them: a body whose bytes did
        // not decode fails the stage as a whole entry would.
        let collected = if p.bodies.bad.load(std::sync::atomic::Ordering::Relaxed) {
            Err(NotImplemented::bug(
                Stage::Collect,
                "a module's check entry did not decode",
            ))
        } else {
            collected
        };
        let Some(collected) = self.stage(Stage::Collect, collected) else {
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
        let entry_inits: Vec<Hash128> = own.iter().filter_map(key_of_init).collect();
        let plan = match &self.goal {
            Goal::ReplInput { .. } if JOINED_INPUTS && self.repl_inputs.joined => {
                match self.joined_inputs(&order, &key_of_init) {
                    Ok(inputs) => Roots::Inputs(inputs),
                    Err(e) => {
                        self.stage::<()>(Stage::Collect, Err(e));
                        return;
                    }
                }
            }
            Goal::Program { .. } | Goal::ReplInput { .. } | Goal::Scenario { .. } if script => {
                Roots::Script {
                    def: root,
                    key: collected.table.key[0],
                    own: entry_inits,
                    report: reports[0].as_ref().and_then(key_of_extra),
                }
            }
            Goal::Program { .. } => Roots::Main {
                def: root,
                key: collected.table.key[0],
                entry_inits,
                report: reports[0].as_ref().and_then(key_of_extra),
            },
            Goal::Scenario { .. } => Roots::Scenario {
                def: root,
                key: collected.table.key[0],
                entry_inits,
                report: reports[0].as_ref().and_then(key_of_extra),
                cleanup: {
                    let def = names.item(&entry_key, "cleanup_ran");
                    if self.body_of(p, def).is_some() {
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
        let mut code_keys = Vec::new();
        let codes: Vec<OnceLock<WasmCode>> = order.iter().map(|_| OnceLock::new()).collect();
        let held = self.scenario_hold(p);
        for id in &order {
            let item = collected.table.item[id.idx()];
            let tir = self
                .body_of(p, item)
                .map_or(Hash128(0), |(original, hash)| {
                    collected.instance_bodies.get(id).map_or(*hash, |body| {
                        hd_mono::defaults::body_key(original, body, *hash)
                    })
                });
            // Emitted code embeds the layouts of the types it touches, and
            // instance keys name types by path only. Collection hashes the
            // `layout_hash` of every type an instance lays out into its
            // callee representation hash (`crates/hd_mono/DESIGN.md` §13.8),
            // so a field added to one data type re-emits only the instances
            // that lay it out, and two programs that declare one path with
            // different fields never share code.
            let mut reps = hd_base::StableHasher::new("code-deps");
            reps.hash(self.toolchain);
            reps.hash(collected.callee_reps[id.idx()]);
            let ck = code_key(
                self.pipeline,
                self.build_profile,
                collected.table.key[id.idx()],
                tir,
                reps.finish(),
            );
            code_keys.push(ck);
        }
        // A folder group is the folder of an instance's item; an item in no
        // folder of the table makes a group of its own.
        let mut by_folder: BTreeMap<Option<u32>, Vec<usize>> = BTreeMap::new();
        for (slot, id) in order.iter().enumerate() {
            let item = collected.table.item[id.idx()];
            let folder = self.table.folder_of_module(&names.module_of(item));
            by_folder
                .entry(folder.map(FolderId::raw))
                .or_default()
                .push(slot);
        }
        let mut groups = Vec::new();
        let mut packs = Vec::new();
        // Read only when a group misses its exact pack.
        let recent = OnceLock::new();
        let mut written = Vec::new();
        // The linear backend emits no per-instance code: link builds the
        // whole module.
        if self.backend == Backend::Linear {
            by_folder.clear();
        }
        for (folder, slots) in by_folder {
            // A held call's stub is per build, never cached: it always
            // emits, and no other build may read it.
            let (held_slots, cached): (Vec<usize>, Vec<usize>) = slots.into_iter().partition(|s| {
                held.is_some_and(|(d, _)| d == collected.table.item[order[*s].idx()])
            });
            let (pack, exact) = self.code_pack(folder, cached, &recent, &code_keys, &codes);
            let mut misses: Vec<usize> = pack
                .slots
                .iter()
                .copied()
                .filter(|s| codes[*s].get().is_none())
                .chain(held_slots)
                .collect();
            misses.sort_unstable();
            if !misses.is_empty() {
                groups.push(misses);
                packs.push(pack);
            } else if !exact {
                // Filled from older packs: this set gets its own.
                if self.write_pack(&pack, &codes, &code_keys) {
                    written.push((pack.folder, pack.key));
                }
            }
        }
        let emits = groups.len();
        let _ = self.collect.set(CollectOut {
            collected,
            order,
            codes,
            code_keys,
            groups,
            packs,
            hint: recent.into_inner().unwrap_or_default(),
            written: Mutex::new(written),
            prog_key: pkey,
            roots: plan,
            layouts: hd_wasm::layout::Layouts::new(self.lowering),
        });
        let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
        for g in 0..emits {
            let e = sp.add(TaskKind::Ext(ExtTask::Emit(u32_of(g))), &[]);
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
    /// `Error` form for a `Result` (its cause chain is printed; its error
    /// type implements `Error` by module.entry.termination.result).
    fn report_fn(&self, ret: Ty) -> Option<(DefId, TyList)> {
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
        let tests = matches!(self.goal, Goal::Tests { .. });
        match pool.get(err) {
            TyData::TraitValue { def, .. } if def == error => Some((
                rt(if tests {
                    "test_status_dyn"
                } else {
                    "entry_status_dyn"
                }),
                pool.list(&[ok]),
            )),
            _ => Some((
                rt(if tests {
                    "test_status_error"
                } else {
                    "entry_status_error"
                }),
                args,
            )),
        }
    }

    /// The inputs a joined build runs ([`Packages::inputs_from`]). `order`
    /// is the init order its last input reaches. Each init goes to the
    /// first input whose uses reach its module, so it runs when it ran as
    /// the session first ran its inputs one at a time
    /// (`module.init.cycle.once`). Its place among that input's inits is
    /// the same too: an input's init order is the last one's, cut to the
    /// modules the input reaches.
    fn joined_inputs(
        &self,
        order: &[(DefId, &str)],
        key_of: &dyn Fn(&DefId) -> Option<Hash128>,
    ) -> hd_base::StageResult<Vec<JoinedInput>> {
        let mut owner: Vec<Option<u32>> = vec![None; self.table.modules.len()];
        let mut out = Vec::new();
        let repl = &self.repl_inputs;
        for &(number, m) in repl.sorted.iter().filter(|(n, _)| repl.builds(*n)) {
            let mut stack = vec![m];
            while let Some(x) = stack.pop() {
                if owner[x].is_none() {
                    owner[x] = Some(number);
                    stack.extend(self.use_edges(x));
                }
            }
            let path = self.table.modules[m].path.as_str();
            let mut inits = Vec::new();
            let mut own = Vec::new();
            for (d, module) in order {
                let mine = self.table.module(module).and_then(|k| owner[k.idx()]) == Some(number);
                if let Some(k) = key_of(d).filter(|_| mine) {
                    inits.push(k);
                    if *module == path {
                        own.push((*d, k));
                    }
                }
            }
            let Some(&(def, key)) = own.first() else {
                return Err(NotImplemented::new(
                    Stage::Collect,
                    "a joined REPL input without its own init",
                ));
            };
            out.push(JoinedInput {
                number,
                def,
                key,
                inits,
                own: own.into_iter().map(|(_, k)| k).collect(),
            });
        }
        Ok(out)
    }

    /// The entry exports of a joined build: each input's script exports,
    /// `hd.init.N` and `hd.poll.N`, then one `hd.wake`.
    fn joined_exports(
        &self,
        inputs: &[JoinedInput],
        env: &Env<'_>,
        path: &dyn Fn(DefId) -> String,
        c: &CollectOut,
    ) -> hd_base::StageResult<Vec<(String, hd_wasm::Helper)>> {
        let mut out = Vec::new();
        if !JOINED_INPUTS {
            return Ok(out);
        }
        let mut wake = None;
        for i in inputs {
            let entry = hd_wasm::script_entry(
                &self.pool,
                env,
                path,
                self.lowering,
                i.def,
                i.key,
                &i.inits,
                &i.own,
                None,
                &c.collected.boundary,
            )?;
            for (name, helper) in entry {
                if name == "hd.wake" {
                    wake = Some((name, helper));
                } else {
                    out.push((format!("{name}.{}", i.number), helper));
                }
            }
        }
        out.extend(wake);
        Ok(out)
    }

    /// The init bodies a program root module `entry` reaches
    /// ([`Self::group_init_order`]). A test run also initializes the
    /// modules its `tests:` block uses (`module.init.tests`), each after
    /// the bodies before it, since those uses make no edge of the use
    /// graph (`module.cycle.test-code`).
    fn root_init_order(
        &self,
        entry: &str,
        init_of: &dyn Fn(&str) -> DefId,
        has_init: &dyn Fn(&str) -> bool,
    ) -> hd_base::StageResult<Vec<(DefId, &str)>> {
        let mut defs = self.group_init_order(entry, init_of, has_init)?;
        let Some(m) = self.table.module(entry).map(ModuleId::idx) else {
            return Ok(defs);
        };
        if !matches!(self.goal, Goal::Tests { .. }) || self.role(m) != "test" {
            return Ok(defs);
        }
        for u in &self.skim_of(m).test_uses {
            let Some(t) = self.table.module_of_use(u) else {
                continue;
            };
            let path = self.table.modules[t.idx()].path.as_str();
            for (d, module) in self.group_init_order(path, init_of, has_init)? {
                if !defs.iter().any(|(e, _)| *e == d) {
                    defs.push((d, module));
                }
            }
        }
        Ok(defs)
    }

    /// The init bodies the entry module reaches, in initialization order
    /// (spec/lang/10-modules.md, "Initialization Order"): each module
    /// cycle (a strongly connected component of the use graph) after the
    /// cycles it uses (`module.init.cycle.once`); among ready cycles, the
    /// one with the least module identity first
    /// (`module.init.cycle.ready-order`). Inside a cycle whose statements
    /// sit in several modules, `InitOrder`'s runs: a module's whole init
    /// when its statements run together, else one body per run. Each body
    /// comes with its module's path.
    fn group_init_order(
        &self,
        entry: &str,
        init_of: &dyn Fn(&str) -> DefId,
        has_init: &dyn Fn(&str) -> bool,
    ) -> hd_base::StageResult<Vec<(DefId, &str)>> {
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
        // Each group's count of dependencies not yet ordered, and the
        // groups that wait on it; the ready groups by name.
        let mut waiting: Vec<usize> = deps.iter().map(std::collections::BTreeSet::len).collect();
        let mut users: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
        for (g, ds) in deps.iter().enumerate() {
            for &d in ds {
                users[d].push(g);
            }
        }
        let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<(&str, usize)>> = (0..groups
            .len())
            .filter(|g| waiting[*g] == 0)
            .map(|g| std::cmp::Reverse((name(g), g)))
            .collect();
        let mut out = Vec::new();
        while let Some(std::cmp::Reverse((_, next))) = ready.pop() {
            for &u in &users[next] {
                waiting[u] -= 1;
                if waiting[u] == 0 {
                    ready.push(std::cmp::Reverse((name(u), u)));
                }
            }
            let mut paths: Vec<&str> = groups[next]
                .iter()
                .map(|m| self.table.modules[*m].path.as_str())
                .collect();
            paths.sort_unstable();
            if paths.iter().filter(|p| has_init(p)).count() < 2 {
                out.extend(
                    paths
                        .iter()
                        .filter(|p| has_init(p))
                        .map(|p| (init_of(p), *p)),
                );
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
            out.extend(
                runs.iter()
                    .map(|r| (self.run_def(runs, r, init_of), r.module.as_str())),
            );
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

    /// `Emit(g)`: the missed instances of folder group `g`, as one batch
    /// over one program view.
    /// The `codepack` of a folder group (`folder`, its cached `slots`):
    /// the pack of exactly these code keys, else the folder's recent packs
    /// in the package's `hint`, whose matching instances it fills in
    /// `codes`. Whether the exact pack was found.
    fn code_pack(
        &self,
        folder: Option<u32>,
        slots: Vec<usize>,
        hint: &OnceLock<PackHint>,
        code_keys: &[Hash128],
        codes: &[OnceLock<WasmCode>],
    ) -> (CodePack, bool) {
        let keys: Vec<Hash128> = slots.iter().map(|s| code_keys[*s]).collect();
        let key = hd_cache::codepack_key(&keys);
        let folder =
            folder.map_or_else(String::new, |f| self.table.folders[f as usize].path.clone());
        let pack = CodePack { slots, key, folder };
        if pack.slots.is_empty() {
            return (pack, true);
        }
        if let Some(sections) = self.lookup(EntryKind::Codepack, key) {
            fill_from(&sections, &pack.slots, code_keys, codes);
            return (pack, true);
        }
        let hint = hint.get_or_init(|| self.pack_hint());
        for old in hint.get(&pack.folder).into_iter().flatten() {
            if pack.slots.iter().all(|s| codes[*s].get().is_some()) {
                break;
            }
            if let Some(sections) = self.lookup(EntryKind::Codepack, *old) {
                fill_from(&sections, &pack.slots, code_keys, codes);
            }
        }
        (pack, false)
    }

    /// Writes `pack` once every slot of it has code; whether it did.
    fn write_pack(
        &self,
        pack: &CodePack,
        codes: &[OnceLock<WasmCode>],
        code_keys: &[Hash128],
    ) -> bool {
        if pack.slots.is_empty() {
            return false;
        }
        let Some(all) = pack
            .slots
            .iter()
            .map(|s| codes[*s].get())
            .collect::<Option<Vec<&WasmCode>>>()
        else {
            return false;
        };
        let mut head = hd_base::wire::Writer::default();
        head.len_of(&pack.slots);
        for s in &pack.slots {
            head.hash(code_keys[*s]);
        }
        let body = hd_wasm::encode_codes(&all);
        self.put(EntryKind::Codepack, pack.key, &[&head.bytes, &body]);
        true
    }

    fn pack_hint_key(&self) -> Hash128 {
        hd_cache::packhint_key(self.pipeline, self.build_profile, self.package_key)
    }

    /// The package's `packhint`; empty when there is none.
    fn pack_hint(&self) -> PackHint {
        let Some(s) = self.lookup(EntryKind::Packhint, self.pack_hint_key()) else {
            return PackHint::new();
        };
        let mut r = hd_base::wire::Reader::new(s.first().map_or(&[][..], Vec::as_slice));
        let mut hint = PackHint::new();
        for _ in 0..r.count() {
            let folder = r.str().to_owned();
            let keys = (0..r.count()).map(|_| r.hash()).collect();
            hint.insert(folder, keys);
        }
        if r.ok() { hint } else { PackHint::new() }
    }

    /// Puts the packs this run wrote first in the package's `packhint`,
    /// which keeps each folder's last [`HINTED_PACKS`]: an edit undone and
    /// then made again still finds the code of the undone state.
    fn put_pack_hint(&self, c: &CollectOut) {
        let written = std::mem::take(&mut *lock(&c.written));
        if written.is_empty() {
            return;
        }
        let mut hint = c.hint.clone();
        for (folder, key) in written {
            let keys = hint.entry(folder).or_default();
            keys.retain(|k| *k != key);
            keys.insert(0, key);
            keys.truncate(HINTED_PACKS);
        }
        let mut w = hd_base::wire::Writer::default();
        w.u32(u32_of(hint.len()));
        for (folder, keys) in &hint {
            w.str(folder);
            w.len_of(keys);
            for k in keys {
                w.hash(*k);
            }
        }
        self.put(EntryKind::Packhint, self.pack_hint_key(), &[&w.bytes]);
    }

    fn emit(&self, group: usize) {
        let (Some(c), Some(p)) = (self.collect.get(), self.program.get()) else {
            return;
        };
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
        let scenario = self.scenario_hold(p);
        for &slot in &c.groups[group] {
            self.emit_slot(c, p, &env, &names, scenario, slot);
        }
        let pack = &c.packs[group];
        if self.write_pack(pack, &c.codes, &c.code_keys) {
            lock(&c.written).push((pack.folder.clone(), pack.key));
        }
    }

    fn emit_slot(
        &self,
        c: &CollectOut,
        p: &ProgramTables,
        env: &Env<'_>,
        names: &Names<'_>,
        scenario: Option<(DefId, bool)>,
        slot: usize,
    ) {
        let id = c.order[slot];
        let item = c.collected.table.item[id.idx()];
        let sub = c.collected.table.sub[id.idx()];
        let args = c.collected.table.args[id.idx()];
        let ret = env.ret(item).unwrap_or(Ty::VOID);
        let path = |d: DefId| names.path(d);
        let calls = &c.collected.calls[id.idx()];
        // A held call emits its stub here, and the stub never reaches
        // the code cache (collect misses it on purpose).
        let held = scenario.and_then(|(d, active)| {
            (d == item).then_some(if active {
                hd_wasm::HoldStub::Active
            } else {
                hd_wasm::HoldStub::Pending
            })
        });
        let r = match sub {
            hd_mono::Sub::Body(k) => {
                let body = self.body_of(p, item).map(|b| &b.0);
                match c
                    .collected
                    .instance_bodies
                    .get(&id)
                    .map(AsRef::as_ref)
                    .or_else(|| c.collected.supplied_body(&self.pool, item, args))
                    .or(body)
                {
                    Some(body) => hd_wasm::emit(
                        &self.pool,
                        env,
                        &path,
                        &c.layouts,
                        body,
                        k,
                        args,
                        ret,
                        calls,
                        c.collected.table.key[id.idx()],
                        held,
                    ),
                    None => Err(NotImplemented::new(Stage::Emit, "an instance without TIR")),
                }
            }
            // A function reference's adapter has no TIR (`crates/hd_mono/DESIGN.md` §13.11).
            hd_mono::Sub::Adapter => {
                hd_wasm::emit_adapter(&self.pool, env, &path, &c.layouts, item, args, calls)
            }
            // A generic method's erased body and its thunks (§13.5.1).
            hd_mono::Sub::Erased | hd_mono::Sub::Thunk(_) => match self.body_of(p, item) {
                Some((body, _)) if sub == hd_mono::Sub::Erased => hd_wasm::emit_erased(
                    &self.pool,
                    env,
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
                    env,
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
        lock(&self.counters).emitted += 1;
        let _ = c.codes[slot].set(code);
    }

    fn link(&self) {
        let (Some(c), Some(p)) = (self.collect.get(), self.program.get()) else {
            return;
        };
        if self.backend == Backend::Linear {
            self.link_linear(c, p);
            return;
        }
        self.put_pack_hint(c);
        let mut codes = Vec::new();
        let mut fnames = Vec::new();
        for (slot, id) in c.order.iter().enumerate() {
            let Some(code) = c.codes[slot].get() else {
                return;
            };
            let key = c.collected.table.key[id.idx()];
            codes.push((key, code.clone()));
            let fname = format!(
                "{}#{}",
                self.names().path(c.collected.table.item[id.idx()]),
                c.collected.table.sub[id.idx()]
            );
            // A copy specialized on known closures (`crates/hd_opt/DESIGN.md` (known closure calls), A2)
            // is told from its plain instance by its key.
            fnames.push(if c.collected.table.known[id.idx()].is_empty() {
                fname
            } else {
                format!("{fname}+{:08x}", key.0 >> 96)
            });
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
                entry_inits,
                report,
            } => hd_wasm::entry(
                &self.pool,
                &env,
                &path,
                self.lowering,
                *def,
                *key,
                &c.collected.inits,
                entry_inits,
                *report,
                &c.collected.boundary,
            ),
            Roots::Script {
                def,
                key,
                own,
                report,
            } => hd_wasm::script_entry(
                &self.pool,
                &env,
                &path,
                self.lowering,
                *def,
                *key,
                &c.collected.inits,
                own,
                *report,
                &c.collected.boundary,
            ),
            Roots::Inputs(inputs) => self.joined_exports(inputs, &env, &path, c),
            Roots::Tests { cases, inits } => hd_wasm::test_entry(
                &self.pool,
                &env,
                &path,
                self.lowering,
                cases,
                inits,
                &c.collected.boundary,
            ),
            Roots::Scenario {
                def,
                key,
                entry_inits,
                report,
                cleanup,
            } => hd_wasm::scenario_entry(
                &self.pool,
                &env,
                &path,
                self.lowering,
                *def,
                *key,
                &c.collected.inits,
                entry_inits,
                *report,
                *cleanup,
                &c.collected.boundary,
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
                let entries = std::mem::take(&mut *lock(&self.input_entries));
                for (input, entries) in entries {
                    if input != 0
                        && entries.iface.is_some()
                        && entries.header.is_some()
                        && entries.check.is_some()
                    {
                        added.inputs.insert(input, entries);
                    }
                }
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

    /// `link` for the linear backend ([`Backend::Linear`]): one module
    /// from collection's output and every instance's TIR body.
    fn link_linear(&self, c: &CollectOut, p: &ProgramTables) {
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
        let body = |id: hd_base::InstId| {
            let item = c.collected.table.item[id.idx()];
            let args = c.collected.table.args[id.idx()];
            c.collected
                .instance_bodies
                .get(&id)
                .map(AsRef::as_ref)
                .or_else(|| c.collected.supplied_body(&self.pool, item, args))
                .or_else(|| self.body_of(p, item).map(|b| &b.0))
        };
        let inits = &c.collected.inits;
        let entry = match &c.roots {
            _ if self.session.is_some() => hd_wasm_linear::Entry::Unsupported("REPL sessions"),
            Roots::Main {
                def,
                key,
                entry_inits,
                report,
            } => hd_wasm_linear::Entry::Main {
                main: *def,
                key: *key,
                inits: inits
                    .iter()
                    .map(|k| (*k, !entry_inits.contains(k)))
                    .collect(),
                report: *report,
            },
            // The split of `hd_wasm::script_entry`; this build runs every
            // init, so the first own body is among them.
            Roots::Script {
                def,
                key,
                own,
                report,
            } => match inits.iter().position(|k| k == key) {
                Some(at) if report.is_none() || at + 1 == inits.len() => {
                    hd_wasm_linear::Entry::Script {
                        init: *def,
                        key: *key,
                        before: inits[..at].iter().map(|k| (*k, true)).collect(),
                        after: inits[at + 1..]
                            .iter()
                            .map(|k| (*k, !own.contains(k)))
                            .collect(),
                        report: *report,
                    }
                }
                Some(_) => {
                    hd_wasm_linear::Entry::Unsupported("a script using `?` with later inits")
                }
                None => hd_wasm_linear::Entry::Unsupported("a script whose init is not run"),
            },
            Roots::Inputs(_) => hd_wasm_linear::Entry::Unsupported("REPL inputs"),
            Roots::Tests { cases, inits } => hd_wasm_linear::Entry::Tests {
                cases: cases.clone(),
                inits: inits.clone(),
            },
            Roots::Scenario { .. } => hd_wasm_linear::Entry::Unsupported("runtime scenarios"),
        };
        let input = hd_wasm_linear::Input {
            pool: &self.pool,
            env: &env,
            path: &path,
            collected: &c.collected,
            order: &c.order,
            body: &body,
            entry,
            release: self.build_profile == BuildProfile::Release,
        };
        let Some(wasm) = self.stage(Stage::Link, hd_wasm_linear::build(&input)) else {
            return;
        };
        self.put(EntryKind::Link, c.prog_key, &[&wasm]);
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
/// interface): the type tables, then each binding's item, type and flags
/// (bit 0 `:=`, bit 1 `let mut`), in statement order.
fn encode_bindings(bindings: &[InputBinding], names: &Names<'_>) -> StageResult<Vec<u8>> {
    let mut t = hd_types::wire::TableWriter::new(names.pool, names.paths, names.syms);
    let mut body = Writer::default();
    body.len_of(bindings);
    for b in bindings {
        body.u32(t.def(b.def));
        body.u32(t.ty(b.ty)?);
        body.u8(u8::from(b.short) | (u8::from(b.mutable) << 1));
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
        let flags = r.u8();
        out.push(InputBinding {
            def,
            ty,
            short: flags & 1 != 0,
            mutable: flags & 2 != 0,
        });
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

/// The registrations as a test run's link reads them: everything but
/// their positions, which the runner reads from the `check` entry.
fn regs_hash(regs: &[hd_check::tests::TestReg]) -> Hash128 {
    let mut h = hd_base::StableHasher::new("test-regs");
    let opt = |h: &mut hd_base::StableHasher, s: &Option<String>| match s {
        Some(s) => {
            h.u8(1);
            h.str(s);
        }
        None => h.u8(0),
    };
    h.u32(u32_of(regs.len()));
    for r in regs {
        h.str(&r.name);
        h.str(&r.kind);
        opt(&mut h, &r.body);
        opt(&mut h, &r.ignore);
        opt(&mut h, &r.expect_panic);
        opt(&mut h, &r.unsupported);
    }
    h.finish()
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

/// The init summaries of one folder's module cycles, by module, and their
/// part key ([`Run::init_inputs`]).
type InitInputs<'a> = (Vec<Vec<(usize, &'a [u8])>>, Hash128);

/// The package's `graph` entry while this run fills it: its key, the
/// folders whose init orders are its parts after coherence's, each part's
/// bytes (`None` for one that is blocked or reports a cycle), and how many
/// parts are still running.
struct GraphParts {
    key: Hash128,
    folders: Vec<usize>,
    slots: Vec<OnceLock<Option<Vec<u8>>>>,
    left: std::sync::atomic::AtomicUsize,
}

/// The sections of a package's `graph` entry: an index holding, per
/// part, its section or 0, then each non-empty part. An empty part stores
/// nothing; its key in the entry's key is enough.
fn encode_graph(parts: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut index = Writer::default();
    index.u32(u32_of(parts.len()));
    let mut out = vec![Vec::new()];
    for p in parts {
        if p.is_empty() {
            index.u32(0);
        } else {
            index.u32(u32_of(out.len()));
            out.push(p.clone());
        }
    }
    out[0] = index.bytes;
    out
}

/// Reads what [`encode_graph`] wrote as `n` parts; an empty part is no
/// bytes.
fn decode_graph(sections: &[Vec<u8>], n: usize) -> Option<Vec<Vec<u8>>> {
    let mut r = Reader::new(sections.first()?);
    if r.u32() as usize != n {
        return None;
    }
    let parts = (0..n)
        .map(|_| match r.u32() as usize {
            0 => Some(Vec::new()),
            i => sections.get(i).cloned(),
        })
        .collect::<Option<Vec<_>>>()?;
    r.ok().then_some(parts)
}

/// [`FolderIface::names_hash`] over the encoded private-name and loop-name
/// sections of an interface entry.
fn names_hash(private: &[u8], looped: &[u8]) -> Hash128 {
    let mut k = hd_base::StableHasher::new("iface-names");
    k.bytes(private);
    k.bytes(looped);
    k.finish()
}

/// A header lowering's read list ([`Run::read_list`]), as its entry
/// stores it: sorted rows of (module path, opened).
fn encode_reads(reads: &[(String, bool)]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(reads);
    for (m, opened) in reads {
        w.str(m);
        w.u8(u8::from(*opened));
    }
    w.bytes
}

fn decode_reads(b: &[u8]) -> Option<Vec<(String, bool)>> {
    let mut r = Reader::new(b);
    let reads = (0..r.count())
        .map(|_| (r.str().to_owned(), r.u8() != 0))
        .collect();
    (r.ok() && r.at_end()).then_some(reads)
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

/// The package's coherence rows (`crates/hd_cache/DESIGN.md`, "Coherence
/// runs per changed trait"): per trait, its findings under its `coh_key`;
/// per folder, the head hashes of its impls under
/// [`coherence_folder_key`]. Each name keeps its newest row and one older
/// one.
#[derive(Default)]
struct CohRows {
    traits: Vec<CohRow>,
    folders: Vec<CohFolder>,
}

/// One trait's row: its `coh_key`, its stable path, and its findings,
/// each as the reported impl's stable path and the message.
#[derive(Clone)]
struct CohRow {
    key: Hash128,
    trait_: String,
    found: Vec<(String, String)>,
}

/// One folder's row: its key, its path, and per trait it implements (by
/// stable path hash) the hash of the head hashes of its impls of it.
#[derive(Clone)]
struct CohFolder {
    key: Hash128,
    folder: String,
    parts: Vec<(Hash128, Hash128)>,
}

/// The key of a folder's [`CohFolder`] row: everything its head hashes
/// read. The folder's `heads_hash` covers its impl records (§4.10),
/// stable paths included. An impl's rank, which `order` builds, reads two
/// things outside the blob: the impl's anchor and, in a REPL session, its
/// input's number. An impl another interface also carries takes its rank
/// from the first carrier, so its rank itself goes into the key.
fn coherence_folder_key(
    iface: &FolderIface,
    impls: &[&Item],
    holders: &HashMap<DefId, Vec<&FolderIface>>,
    inputs: &HashMap<&str, u32>,
    order: &dyn Fn(DefId) -> String,
) -> Hash128 {
    let mut k = hd_base::StableHasher::new("coh-folder");
    k.str(&iface.folder);
    k.hash(iface.heads_hash);
    for it in impls {
        if holders.get(&it.def).is_some_and(|h| h.len() == 1) {
            let at = iface
                .spans
                .get(&(it.def, 0))
                .map_or([u32::MAX; 3], |(_, a)| [a.decl, a.member, a.tok]);
            for v in at {
                k.u32(v);
            }
            if !inputs.is_empty() {
                k.str(&order(it.def));
            }
        } else {
            k.str(&order(it.def));
        }
    }
    k.finish()
}

/// A folder's [`CohFolder`] row under `key`: per trait, the head hash of
/// the folder's impls of it, from their records and their ranks `order`.
/// `None` when an impl does not encode.
fn coherence_folder(
    u: &hd_resolve::Universe<'_>,
    iface: &FolderIface,
    impls: &[&Item],
    key: Hash128,
    order: &dyn Fn(DefId) -> String,
) -> Option<CohFolder> {
    let mut by_trait: BTreeMap<Hash128, Vec<&Item>> = BTreeMap::new();
    for it in impls {
        if let ItemData::Impl { trait_, .. } = it.data {
            by_trait
                .entry(u.names.path_hash(trait_))
                .or_default()
                .push(it);
        }
    }
    let parts = by_trait
        .into_iter()
        .map(|(t, group)| {
            let ranks: Vec<String> = group.iter().map(|i| order(i.def)).collect();
            let mut k = hd_base::StableHasher::new("coh-folder-trait");
            k.str(&iface.folder);
            k.hash(u.head_hash(&group, &ranks).ok()?);
            Some((t, k.finish()))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CohFolder {
        key,
        folder: iface.folder.clone(),
        parts,
    })
}

/// `now`, then each name's newest row of `old` with another key, so
/// undoing an edit finds the rows of the state before it. A name `now`
/// lacks keeps no row.
fn keep_previous<T: Clone>(
    mut now: Vec<T>,
    old: &[T],
    name: impl Fn(&T) -> (&String, Hash128),
) -> Vec<T> {
    let current: HashMap<String, Hash128> = now
        .iter()
        .map(|r| {
            let (n, k) = name(r);
            (n.clone(), k)
        })
        .collect();
    let mut kept: HashSet<&str> = HashSet::default();
    let older: Vec<T> = old
        .iter()
        .filter(|r| {
            let (n, k) = name(r);
            current.get(n).is_some_and(|c| *c != k) && kept.insert(n.as_str())
        })
        .cloned()
        .collect();
    now.extend(older);
    now
}

fn encode_coh_rows(rows: &CohRows) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(&rows.traits);
    for r in &rows.traits {
        w.hash(r.key);
        w.str(&r.trait_);
        w.len_of(&r.found);
        for (path, msg) in &r.found {
            w.str(path);
            w.str(msg);
        }
    }
    w.len_of(&rows.folders);
    for r in &rows.folders {
        w.hash(r.key);
        w.str(&r.folder);
        w.len_of(&r.parts);
        for (t, h) in &r.parts {
            w.hash(*t);
            w.hash(*h);
        }
    }
    w.bytes
}

fn decode_coh_rows(b: &[u8]) -> Option<CohRows> {
    let mut r = Reader::new(b);
    let traits = (0..r.count())
        .map(|_| CohRow {
            key: r.hash(),
            trait_: r.str().to_owned(),
            found: (0..r.count())
                .map(|_| (r.str().to_owned(), r.str().to_owned()))
                .collect(),
        })
        .collect();
    let folders = (0..r.count())
        .map(|_| CohFolder {
            key: r.hash(),
            folder: r.str().to_owned(),
            parts: (0..r.count()).map(|_| (r.hash(), r.hash())).collect(),
        })
        .collect();
    (r.ok() && r.at_end()).then_some(CohRows { traits, folders })
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
/// The folders an interface names, by path, for its entry.
fn encode_mentioned(mentions: &[(String, Hash128)]) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(mentions);
    for (path, _) in mentions {
        w.str(path);
    }
    w.bytes
}

/// Reports a stage-B finding at the span `span_of` gives its slot, with
/// its fix-it: `&` written before the type at each of its `ref_fix` slots
/// (`data.struct.no-recursion.fix`, `data.enum.no-recursion.fix`).
fn emit_finding(d: &mut DiagBuf, f: &hd_check::header::Finding, span_of: impl Fn(u32) -> Span) {
    d.error(f.code, span_of(f.slot), &f.message);
    if f.ref_fix.is_empty() {
        return;
    }
    let edits: Vec<(Span, &str)> = f
        .ref_fix
        .iter()
        .map(|&slot| {
            let at = span_of(slot);
            (Span { hi: at.lo, ..at }, "&")
        })
        .collect();
    d.add_fix(
        "hold it through a reference",
        hd_diag::FixSafety::Suggestion,
        &edits,
    );
}

fn encode_header_findings(found: &[hd_check::header::Finding], items: &Items) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(found);
    for f in found {
        let at = items.defs().iter().position(|d| *d == f.item).unwrap_or(0);
        w.u32(u32_of(at));
        w.u32(f.slot);
        w.str(f.code.as_str());
        w.str(&f.message);
        w.len_of(&f.ref_fix);
        for &slot in &f.ref_fix {
            w.u32(slot);
        }
    }
    w.bytes
}

fn decode_header_findings(b: &[u8], items: &Items) -> Option<Vec<hd_check::header::Finding>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let at = r.u32() as usize;
        let slot = r.u32();
        let code = Code::from_name(r.str())?;
        let message = r.str().to_owned();
        let ref_fix = (0..r.count()).map(|_| r.u32()).collect();
        out.push(hd_check::header::Finding {
            item: *items.defs().get(at)?,
            slot,
            code,
            message,
            ref_fix,
        });
    }
    r.ok().then_some(out)
}

/// Where a fix-it edit of `span` sits, without byte offsets: an anchor of
/// the token at its start, and its start and end as byte distances from
/// that token's start ([`IfaceFix`]). An empty edit is anchored by the
/// byte after it.
fn edit_anchor(src: &Src<'_>, span: Span) -> (hd_resolve::anchor::Anchor, u32, u32) {
    let anchor = hd_resolve::anchor::of_span(src, span.lo, span.hi.max(span.lo.saturating_add(1)));
    let base = hd_resolve::anchor::resolve(src, &anchor).map_or(span.lo, |s| s.lo);
    (
        anchor,
        span.lo.wrapping_sub(base),
        span.hi.wrapping_sub(base),
    )
}

fn encode_anchor(w: &mut Writer, a: &hd_resolve::anchor::Anchor) {
    for v in [a.decl, a.member, a.tok, a.count] {
        w.u32(v);
    }
}

fn decode_anchor(r: &mut Reader<'_>) -> hd_resolve::anchor::Anchor {
    hd_resolve::anchor::Anchor {
        decl: r.u32(),
        member: r.u32(),
        tok: r.u32(),
        count: r.u32(),
    }
}

fn encode_iface_diags(d: &IfaceDiags) -> Vec<u8> {
    let mut w = Writer::default();
    w.len_of(d);
    for (m, code, severity, a, msg, fixes) in d {
        w.u32(u32_of(*m));
        w.str(code.as_str());
        w.u8(u8::from(*severity == Severity::Warning));
        encode_anchor(&mut w, a);
        w.str(msg);
        w.len_of(fixes);
        for f in fixes {
            w.str(&f.title);
            w.u8(u8::from(f.exact));
            w.len_of(&f.edits);
            for (m, a, lo, hi, text) in &f.edits {
                w.u32(u32_of(*m));
                encode_anchor(&mut w, a);
                w.u32(*lo);
                w.u32(*hi);
                w.str(text);
            }
        }
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
        let a = decode_anchor(&mut r);
        let msg = r.str().to_owned();
        let mut fixes = Vec::new();
        for _ in 0..r.count() {
            let title = r.str().to_owned();
            let exact = r.u8() == 1;
            let mut edits = Vec::new();
            for _ in 0..r.count() {
                let m = r.u32() as usize;
                let a = decode_anchor(&mut r);
                let (lo, hi) = (r.u32(), r.u32());
                edits.push((m, a, lo, hi, r.str().to_owned()));
            }
            fixes.push(IfaceFix {
                title,
                exact,
                edits,
            });
        }
        out.push((m, code, severity, a, msg, fixes));
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

/// Each TIR body of a `check` entry: its item's path hash, and where its
/// wire bytes are in the entry. Decodes no body; `None` when the entry is
/// malformed.
fn body_index(entry: &[u8]) -> Option<Vec<(Hash128, std::ops::Range<usize>)>> {
    let mut r = Reader::new(entry);
    let mut section = None;
    for i in 0..r.count() {
        let b = r.blob();
        if i == 3 {
            section = Some((r.pos() - b.len(), b));
            break;
        }
    }
    let (start, section) = section?;
    let mut r = Reader::new(section);
    let mut out = Vec::new();
    for _ in 0..r.count() {
        let item = r.hash();
        let bytes = r.blob();
        let end = start + r.pos();
        out.push((item, end - bytes.len()..end));
    }
    r.ok().then_some(out)
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
                            .map(|f| {
                                self.low(hd_mono::subst(&self.run.pool, self, def, args, f.ty))
                            })
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
                    .map(|v| v.fields.iter().map(|f| self.low(f.ty)).collect())
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
    fn newtype_base(&self, def: DefId) -> Option<Ty> {
        match &self.p.items.get(&def)?.data {
            ItemData::Newtype(inner) => Some(self.low(*inner)),
            _ => None,
        }
    }
}

impl Env<'_> {
    fn sig(&self, def: DefId) -> Option<&hd_resolve::FnSig> {
        self.p.items.get(&def)?.sig()
    }

    /// A declared type as the backend reads it: each reference type
    /// lowered to `std.core.Ref[T]` (`hd_types::Types::lower_refs`).
    fn low(&self, t: Ty) -> Ty {
        self.run.pool.lower_refs(self.run.known.ref_cell, t)
    }

    /// `std.result`'s checked unwrap after a boundary `decode`: `is_ok`
    /// to test the `decode` result, then `expect` for its value. The
    /// methods live on `Result`'s inherent impl, whose path the path
    /// table cannot name up front, so find them by declaration.
    fn result_unwrap(&self) -> Option<(DefId, DefId)> {
        // `is_ok` and `expect` may sit in different inherent impls of
        // `Result`: `expect` needs `E < Display`, so std declares it apart.
        let mut is_ok = None;
        let mut expect = None;
        for item in self.p.items.values() {
            let ItemData::Impl {
                trait_,
                self_ty,
                methods,
                ..
            } = &item.data
            else {
                continue;
            };
            if *trait_ != DefId::NONE {
                continue;
            }
            let TyData::Adt { def, .. } = self.run.pool.get(*self_ty) else {
                continue;
            };
            if def != self.run.known.result {
                continue;
            }
            for (name, method) in methods {
                match self.run.syms.resolve(*name) {
                    "is_ok" => is_ok = Some(*method),
                    "expect" => expect = Some(*method),
                    _ => {}
                }
            }
        }
        Some((is_ok?, expect?))
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

    /// A default body (`crates/hd_mono/DESIGN.md` §13.13): a body of its own, whose
    /// parameters are the earlier parameters and whose result is the
    /// parameter's or field's declared type. A facts body is one that takes
    /// no parameter.
    fn default_body(&self, def: DefId) -> Option<&Body> {
        self.run
            .body_of(self.p, def)
            .map(|b| &b.0)
            .filter(|b| matches!(b.kind, hd_tir::BodyKind::Default | hd_tir::BodyKind::Fact))
    }

    /// Whether a module's `Init` body suspends: a REPL input's top level
    /// is a driver context, so it suspends where it awaits outside its
    /// closures. No other top level awaits.
    fn init_suspends(&self, def: DefId) -> bool {
        let Some((b, _)) = self.run.body_of(self.p, def) else {
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
    /// module's init has an entry row inferred under the default profile
    /// (`module.init.script-profile`), so its keys are those of the callees
    /// its top level calls.
    fn init_keys(&self, def: DefId) -> Vec<Ty> {
        let Some((b, _)) = self.run.body_of(self.p, def) else {
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
        self.run.body_of(self.p, def).map(|b| &b.0)
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
        // A script's init returns `Result[void, dyn Error]` when its top
        // level uses `?` (`module.init.script-try`).
        if let Some((b, _)) = self.run.body_of(self.p, def)
            && b.kind == hd_tir::BodyKind::Init
        {
            return b.sub_root.first().map(|&r| b.ty[r as usize]);
        }
        self.sig(def).map(|s| self.low(s.ret))
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
            .map(|s| s.params.iter().map(|p| self.low(p.1)).collect())
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
        let keys: Vec<Ty> = pool
            .row_data(row)
            .keys
            .into_iter()
            .map(|k| self.low(k))
            .collect();
        hd_mono::key_order(pool, &|d| names.path_hash(d), keys)
    }
    fn parent(&self, def: DefId) -> Option<(DefId, usize)> {
        let ItemData::Method { owner, .. } = &self.p.items.get(&def)?.data else {
            return None;
        };
        let o = self.p.items.get(owner)?;
        match &o.data {
            // A data type or enum owns its declaration `Structure`'s
            // hidden methods (`crates/hd_mono/DESIGN.md` §12.3).
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
            } => Some((
                self.low(*self_ty),
                self.run
                    .pool
                    .lower_refs_list(self.run.known.ref_cell, *trait_args),
                it.generics.len(),
            )),
            _ => None,
        }
    }
    fn ref_cell(&self) -> DefId {
        self.run.known.ref_cell
    }
    fn is_derived_newtype_impl(&self, impl_: DefId) -> bool {
        let Some(it) = self.p.items.get(&impl_) else {
            return false;
        };
        let ItemData::Impl { self_ty, kind, .. } = &it.data else {
            return false;
        };
        if *kind != hd_resolve::ImplKind::Derived {
            return false;
        }
        matches!(self.run.pool.get(*self_ty), TyData::Adt { def, .. } if self.p.items.get(&def).is_some_and(|i| matches!(i.data, ItemData::Newtype(_))))
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
            // trait value (trait.dyn.member.assoc-function). A `&self` or
            // `&mut self` method has one: a trait value reference passes
            // its payload, and on a bare trait value the slot traps
            // (`hd_mono::SlotRecv`).
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
        let k = self.p.items.get(&def)?.intrinsic?;
        Some(self.run.syms.resolve(k).to_owned())
    }
    fn json_items(&self) -> Option<JsonItems> {
        let k = &self.run.known;
        let (result_is_ok, result_expect) = self.result_unwrap()?;
        Some(JsonItems {
            encode: k.json_encode,
            decode: k.json_decode,
            serialize: k.serialize,
            deserialize: k.deserialize,
            result_is_ok,
            result_expect,
        })
    }
    fn data_fields(&self, def: DefId) -> Option<Vec<Ty>> {
        match &self.p.items.get(&def)?.data {
            ItemData::Data(fs) => Some(fs.iter().map(|f| self.low(f.ty)).collect()),
            // A newtype is a data value of its one inner value.
            ItemData::Newtype(inner) => Some(vec![self.low(*inner)]),
            _ => None,
        }
    }
    fn is_struct(&self, def: DefId) -> bool {
        self.p.items.get(&def).is_some_and(|i| i.is_struct)
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
        // A tuple's `Structure` (`crates/hd_mono/DESIGN.md` §13.6): the derivation
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
        Some((eq, method, self.run.known.key_bucket_hash))
    }
    fn inline_bucket_hash(&self, string: bool) -> DefId {
        if string {
            self.run.known.string_bucket_hash
        } else {
            self.run.known.int_bucket_hash
        }
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
            || hd_resolve::declares_ref_receiver(
                self.run.names().syms,
                |d| self.p.items.get(&d),
                &self.run.pool,
                trait_,
            )
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
/// for a call of `Structure` (`crates/hd_mono/DESIGN.md` §12.3).
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
/// of its data type or enum, built once per type into `decl` by
/// [`declaration_bodies`]; and when the derivation's member `lines` edit
/// them, the edited slots' own bodies. New bodies join `derived`. `None`
/// when a fact is not generated yet.
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
    let pool = cx.names.pool;
    let data = opt.data;
    let base = decl
        .entry(data)
        .or_insert_with(|| declaration_bodies(cx, heads, data, derived))
        .clone()?;
    if !opt.lines {
        return Some(base);
    }
    let item = cx.lookup.item(data)?;
    let node = heads.iter().find(|h| h.def == data)?.node;
    let mut view =
        hd_check::structure::declaration_facts(cx, item, node, traitless(cx, heads, data));
    let edited = hd_check::structure::edit_facts(cx, item, &mut view, lines);
    // The derivation's bodies are in its own scope, where the target is
    // the data type or enum at the derivation's arguments.
    if let TyData::Adt { args, .. } = pool.get(opt.target) {
        view.retarget(cx, data, pool.list_items(args));
    }
    let (own, bodies) = hd_check::structure::fact_bodies(
        cx,
        (opt.impl_, ""),
        (&view, node),
        Some((&edited, &base)),
    )
    .ok()?;
    derived.bodies.extend(bodies);
    Some(own)
}

/// The node of the derivation block with no trait for `data` among the
/// items of module `cx`, if it has one.
fn traitless<'t>(
    cx: &BodyCx<'t>,
    heads: &[hd_resolve::Head<'t>],
    data: DefId,
) -> Option<hd_syntax::NodeRef<'t>> {
    let it = cx
        .lookup
        .own
        .iter()
        .find(|it| traitless_for(cx.names.pool, it, data))?;
    heads.iter().find(|h| h.def == it.def).map(|h| h.node)
}

/// Whether `it` is a derivation block with no trait for `data`.
/// A derivation block's member lines as `annot.line.drift` compares them:
/// each member, whether it extends, and its right side's tokens.
fn drift_signature(
    src: &hd_resolve::Src<'_>,
    block: hd_syntax::NodeRef<'_>,
) -> Vec<(String, bool, Vec<String>)> {
    hd_check::derive::member_lines(src, block)
        .into_iter()
        .map(|l| {
            let rhs = l.list.map_or_else(
                || vec!["pass".to_owned()],
                |v| src.tokens(v).map(|t| src.text(t).to_owned()).collect(),
            );
            (l.name.to_owned(), l.extend, rhs)
        })
        .collect()
}

fn traitless_for(pool: &InternPool, it: &Item, data: DefId) -> bool {
    matches!(&it.data, ItemData::Impl { trait_, by: Some(_), self_ty, .. }
        if *trait_ == DefId::NONE
            && matches!(pool.get(*self_ty), TyData::Adt { def, .. } if def == data))
}

/// The declaration facts bodies of the data type or enum `data`, which
/// module `cx` declares (`annot.fact.shared`): its decorators, edited by
/// its block with no trait when `cx` holds that block. New bodies join
/// `derived`. `None` when a fact is not generated yet.
fn declaration_bodies<'t>(
    cx: &BodyCx<'t>,
    heads: &[hd_resolve::Head<'t>],
    data: DefId,
    derived: &mut Derived,
) -> Option<FactBodies> {
    let item = cx.lookup.item(data)?;
    let node = heads.iter().find(|h| h.def == data)?.node;
    let facts = hd_check::structure::declaration_facts(cx, item, node, traitless(cx, heads, data));
    let (b, bodies) =
        hd_check::structure::fact_bodies(cx, (data, ""), (&facts, node), None).ok()?;
    derived.bodies.extend(bodies);
    Some(b)
}
