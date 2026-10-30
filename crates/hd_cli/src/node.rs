//! Node as an `hd_run::Engine` (`f1c55020:future-work/compiler/build-order.md` §22: until wasmtime is an
//! approved dependency, `hd run` and `hd test` run on V8 through Node).
//! The JS host (`host/*.mjs`) is embedded, so the binary is
//! relocatable.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, Grants, HostSetup, Instance, Limit, LoadError, Outcome, StartError};

use crate::report::quote;

/// The JS host's platform-neutral core: the import object and the entry
/// driver.
const CORE: &str = include_str!("../../../host/core.mts");
/// Package-host runtime binding imported by the platform-neutral core.
const CORE_PACKAGE: &str = include_str!("../../../host/core-package.mts");
/// The initialized REPL session and checkpoint lifecycle imported by the core.
const SESSION: &str = include_str!("../../../host/session.mts");
/// The checked descriptor report imported by the shared core.
const CHECKED_REPORT: &str = include_str!("../../../host/checked-report.mts");
/// The JS host's Node layer: the providers that touch the machine.
const NODE: &str = include_str!("../../../host/node.mjs");
/// Node path, host-name and file-operation helpers shared with workers.
const NODE_FILES: &str = include_str!("../../../host/node-files.mjs");
/// The Node-only REPL glue loaded by `node.mjs` on demand.
const NODE_SESSION: &str = include_str!("../../../host/node-session.mjs");
/// `hd run`: one instance, `hd.init` then `hd.poll`.
const RUN: &str = include_str!("../../../host/run.mjs");
/// Selects the panic-only startup host before the full Node host.
const STARTUP: &str = include_str!("../../../host/startup.mjs");
/// `hd test`: runs the test program's cases and writes their results.
const TEST: &str = include_str!("../../../host/test.mjs");
/// The worker thread the cases run in, and its supervisor.
const TEST_RUN: &str = include_str!("../../../host/test-run.mjs");
/// The case loop `test.mjs` runs: one fresh instance per case.
const CASES: &str = include_str!("../../../host/cases.mjs");
/// The `Http` provider's client thread.
const HTTP: &str = include_str!("../../../host/http.mjs");
/// The `Http` provider's bounded content-decoding policy.
const HTTP_DECODE: &str = include_str!("../../../host/http-decode.mjs");
/// The `Net` provider's client thread, which holds the sockets.
const NET: &str = include_str!("../../../host/net.mjs");
/// The `Process` provider's client thread, which owns child I/O.
const PROCESS: &str = include_str!("../../../host/process.mjs");
/// The special-file provider's I/O helper.
const FS: &str = include_str!("../../../host/fs.mjs");
/// The `ConsoleInput` provider's client thread.
const INPUT: &str = include_str!("../../../host/input.mts");
/// The ordered output writer.
const OUTPUT: &str = include_str!("../../../host/output.mts");
/// The output writer's thread, started when a pipe is full.
const OUTPUT_WORKER: &str = include_str!("../../../host/output-worker.mts");
/// Correlated replies shared by the Node control clients and workers.
const WORKER_CONTROL: &str = include_str!("../../../host/worker-control.mts");
/// The float text of the host primitives (`core.mts` imports it).
const FLOAT: &str = include_str!("../../../host/float.mts");
/// A REPL session's journal of host calls (`core.mts` imports it).
const JOURNAL: &str = include_str!("../../../host/journal.mts");
/// Dependency-free entry-driver constants and cancellation helpers.
const RUN_SUPPORT: &str = include_str!("../../../host/run-support.mts");
/// The exchange-buffer encoding the hosts' codecs are built on.
const WIRE: &str = include_str!("../../../host/wire.mts");
/// The lightweight decoder for a package-host custom section.
const PACKAGE_CONTRACT: &str = include_str!("../../../host/package-contract.mts");
/// The checked helpers called by generated package host bindings.
const PACKAGE_PROVIDERS: &str = include_str!("../../../host/package-providers.mts");
/// The codecs and method descriptors generated from the ABI model.
const ABI: &str = include_str!("../../../host/gen/abi.mts");

/// Node as the engine. `trace` receives the Wasm frames of a trap the run
/// ended in, innermost first, which the caller symbolizes
/// (`flow.panic.report`).
#[derive(Default)]
pub struct NodeEngine {
    pub trace: Arc<Mutex<Vec<(u32, u32)>>>,
    pub host_providers: Vec<HostProvider>,
}

/// A provider selected from the declaring package after root manifest consent.
#[derive(Clone)]
pub struct HostProvider {
    pub module: String,
    pub grant_path: String,
    pub path: PathBuf,
}

/// Resolve only reachable package providers. A CLI flag cannot supply the
/// missing manifest consent, and no provider module is imported here.
pub(crate) fn manifest_providers(
    wasm: &[u8],
    program: Option<&crate::disk::Program>,
    table: &[(String, hd_project::Grant)],
    grants: &Grants,
) -> Result<Vec<HostProvider>, String> {
    let Some(artifacts) = hd_driver::host_artifacts(wasm).map_err(str::to_owned)? else {
        return Ok(Vec::new());
    };
    let mut selected = Vec::new();
    for interface in artifacts.contract.interfaces.iter().filter(|i| !i.handle) {
        let name = format!(
            "{}@{}/{}",
            interface.identity.package, interface.identity.version, interface.identity.trait_path
        );
        if interface.selectors.iter().any(|path| !grants.allows(path)) {
            return Err(format!("package capability denied: {name}"));
        }
        let grant = interface.selectors.iter().find(|path| table.iter().any(|(key, value)| key == *path && *value == hd_project::Grant::All))
            .ok_or_else(|| format!("package capability {name} is not granted; add `\"{}\" = true` to the app's `[capabilities]` table", interface.selectors.first().map_or("TRAIT_PATH", String::as_str)))?;
        let package = program
            .and_then(|p| p.host_packages.iter().find(|package| package.trait_identity(&interface.identity.trait_path).as_ref() == Some(&interface.identity)))
            .ok_or_else(|| format!("no selected package manifest provides {name}; run this program in its app package"))?;
        let root = program
            .and_then(|p| p.package_directory(package.package))
            .ok_or_else(|| format!("no package directory provides {name}"))?;
        let entry = package
            .declarations
            .as_ref()
            .and_then(|d| d.providers.as_ref())
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.trait_path == interface.identity.trait_path)
            })
            .ok_or_else(|| format!("{} has no `[host]` entry for {name}", package.manifest))?;
        let file = entry.path.as_ref().ok_or_else(|| {
            format!(
                "{}:{}: provider for {name} must name a file",
                package.manifest, entry.line
            )
        })?;
        let root = hd_run::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let wanted = root.join(file);
        let path = hd_run::canonicalize(&wanted).map_err(|e| {
            format!(
                "{}:{}: provider `{file}` for {name}: {e}",
                package.manifest, entry.line
            )
        })?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(format!(
                "{}:{}: provider `{file}` for {name} must be a file inside its declaring package",
                package.manifest, entry.line
            ));
        }
        selected.push(HostProvider {
            module: interface.module.clone(),
            grant_path: grant.clone(),
            path,
        });
    }
    Ok(selected)
}

pub struct NodeModule {
    wasm: Vec<u8>,
}

struct NodeInstance {
    wasm: Vec<u8>,
    host_providers: Vec<HostProvider>,
    /// The program's arguments, after the module's path and its
    /// configuration on Node's command line.
    args: Vec<String>,
    /// What its `Args.program` returns.
    program: String,
    /// Its capability grant, which the host's providers check
    /// (`cli.host.default-profile.granted`).
    grants: Grants,
    /// Its `--time-limit` in milliseconds (`cli.limit.time`), which the
    /// host enforces.
    wall_ms: Option<u64>,
    /// The cleanup bound of a Ctrl-C in milliseconds (`cli.cancel.bound`),
    /// for a run that a Ctrl-C may cancel.
    cleanup_ms: Option<u64>,
    done: Option<Outcome>,
    trace: Arc<Mutex<Vec<(u32, u32)>>>,
}

impl Engine for NodeEngine {
    type Module = NodeModule;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<NodeModule, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        let artifacts =
            hd_driver::host_artifacts(wasm).map_err(|e| LoadError::Engine(e.to_owned()))?;
        let roots: Vec<_> = artifacts
            .as_ref()
            .into_iter()
            .flat_map(|a| &a.contract.interfaces)
            .filter(|i| !i.handle)
            .collect();
        let mut registered = std::collections::BTreeSet::new();
        for provider in &self.host_providers {
            let Some(interface) = roots.iter().find(|i| {
                i.module == provider.module && i.selectors.contains(&provider.grant_path)
            }) else {
                return Err(LoadError::Engine(
                    "selected provider names no required package capability".into(),
                ));
            };
            if !registered.insert(&interface.module) {
                return Err(LoadError::Engine(format!(
                    "duplicate provider for {}",
                    interface.identity.trait_path
                )));
            }
        }
        for interface in roots {
            if !registered.contains(&interface.module) {
                return Err(LoadError::Engine(format!(
                    "missing package host provider for {}; grant the trait in the app's `[capabilities]` table",
                    interface.identity.trait_path
                )));
            }
        }
        Ok(NodeModule {
            wasm: wasm.to_vec(),
        })
    }
    fn instantiate(
        &self,
        m: &NodeModule,
        host: &HostSetup,
    ) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(NodeInstance {
            wasm: m.wasm.clone(),
            host_providers: self.host_providers.clone(),
            args: host.args.clone(),
            program: host.program.clone(),
            grants: host.grants.clone(),
            wall_ms: host.limits.wall_ms,
            cleanup_ms: host.limits.cleanup_ms,
            done: None,
            trace: Arc::clone(&self.trace),
        }))
    }
}

/// A scratch directory with the host scripts and the module, removed on
/// drop. Each is unique, so parallel test workers do not share one.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(wasm: &[u8]) -> Result<Self, String> {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("hd-run-{}-{n}", std::process::id()));
        let s = Scratch(dir);
        std::fs::create_dir_all(s.0.join("gen")).map_err(|e| format!("{}: {e}", s.0.display()))?;
        for (name, text) in [
            ("core.mts", CORE),
            ("core-package.mts", CORE_PACKAGE),
            ("session.mts", SESSION),
            ("checked-report.mts", CHECKED_REPORT),
            ("node.mjs", NODE),
            ("node-files.mjs", NODE_FILES),
            ("node-session.mjs", NODE_SESSION),
            ("run.mjs", RUN),
            ("startup.mjs", STARTUP),
            ("test.mjs", TEST),
            ("test-run.mjs", TEST_RUN),
            ("cases.mjs", CASES),
            ("http.mjs", HTTP),
            ("http-decode.mjs", HTTP_DECODE),
            ("net.mjs", NET),
            ("process.mjs", PROCESS),
            ("fs.mjs", FS),
            ("input.mts", INPUT),
            ("output.mts", OUTPUT),
            ("output-worker.mts", OUTPUT_WORKER),
            ("worker-control.mts", WORKER_CONTROL),
            ("float.mts", FLOAT),
            ("journal.mts", JOURNAL),
            ("run-support.mts", RUN_SUPPORT),
            ("wire.mts", WIRE),
            ("package-contract.mts", PACKAGE_CONTRACT),
            ("package-providers.mts", PACKAGE_PROVIDERS),
            ("gen/abi.mts", ABI),
        ] {
            std::fs::write(s.0.join(name), text).map_err(|e| format!("{}: {e}", s.0.display()))?;
        }
        std::fs::write(s.0.join("main.wasm"), wasm)
            .map_err(|e| format!("{}: {e}", s.0.display()))?;
        if !wasm.is_empty()
            && let Some(artifacts) = hd_driver::host_artifacts(wasm).map_err(str::to_owned)?
        {
            std::fs::write(s.0.join("main.host.mjs"), artifacts.javascript)
                .map_err(|e| format!("{}: {e}", s.0.display()))?;
        }
        Ok(s)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl NodeInstance {
    /// Runs the whole program in one Node process, with our standard
    /// streams, so it reads our input (`ConsoleInput`) and writes as it
    /// goes. Its configuration holds its `Args.program` and its grant.
    /// A host that cannot exit after a time-limit panic, because a worker
    /// thread is blocked in a system call, kills its own process; the
    /// run's status is then in `status.txt`, not in the exit status.
    fn run(&self) -> Outcome {
        let dir = match Scratch::new(&self.wasm) {
            Ok(d) => d,
            Err(e) => return Outcome::Internal(e),
        };
        let config = dir.0.join("config.json");
        let frames = dir.0.join("frames.json");
        let killed = dir.0.join("status.txt");
        let mut text = run_config(
            &self.program,
            &self.grants,
            (self.wall_ms, self.cleanup_ms),
            Some((&frames, &killed)),
        );
        if !self.host_providers.is_empty() {
            text.pop();
            text.push_str(",\"hostProviders\":[");
            for (n, r) in self.host_providers.iter().enumerate() {
                if n != 0 {
                    text.push(',');
                }
                let _ = write!(
                    text,
                    "{{\"module\":{},\"grantPath\":{},\"path\":{}}}",
                    quote(&r.module),
                    quote(&r.grant_path),
                    quote(&r.path.to_string_lossy())
                );
            }
            text.push_str("]}");
        }
        if let Err(e) = std::fs::write(&config, text) {
            return Outcome::Internal(format!("{}: {e}", config.display()));
        }
        let _ = std::io::stdout().flush();
        let status = Command::new("node")
            .arg(dir.0.join("run.mjs"))
            .arg(dir.0.join("main.wasm"))
            .arg(&config)
            .args(&self.args)
            .spawn()
            .and_then(|mut child| {
                RUN_CHILD.store(child.id(), Ordering::Relaxed);
                let status = child.wait();
                RUN_CHILD.store(0, Ordering::Relaxed);
                status
            });
        if let Ok(text) = std::fs::read_to_string(&frames)
            && let Some((raw, _)) = json_frames(&text)
            && let Ok(mut trace) = self.trace.lock()
        {
            *trace = raw;
        }
        if let Ok(text) = std::fs::read_to_string(&killed)
            && let Ok(code) = text.trim().parse::<u8>()
        {
            return Outcome::Exit(code);
        }
        match status {
            Ok(s) if s.success() => Outcome::Exit(0),
            // A Ctrl-C while Wasm ran ended Node with SIGINT's default
            // action, since its host listens only while it waits
            // (`run.mjs`): the program was stopped (`cli.cancel.computing`).
            Ok(s) if interrupted(s) => {
                eprintln!("stopped");
                Outcome::Exit(crate::engine::CANCEL_STATUS)
            }
            Ok(s) => Outcome::Exit(u8::try_from(s.code().unwrap_or(1).clamp(1, 255)).unwrap_or(1)),
            Err(e) => Outcome::Internal(format!("node: {e}")),
        }
    }
}

/// The process id of the Node child that runs the program, while it runs,
/// else 0.
static RUN_CHILD: AtomicU32 = AtomicU32::new(0);

/// The signal that is a Ctrl-C to the Node child on Unix
/// (`cli.cancel.forward`). It is not SIGINT: a terminal's Ctrl-C reaches
/// the child by its process group as well as `hd`, so the child ignores
/// SIGINT and hears the interrupt once, from `hd`.
#[cfg(unix)]
const FORWARD: nix::sys::signal::Signal = nix::sys::signal::Signal::SIGUSR2;

/// Passes an interrupt that `hd` heard on to the Node child, if one runs
/// (`cli.cancel.forward`). Windows has nothing to pass: the console
/// delivers a Ctrl-C to the child itself (`cli.cancel.windows`).
pub(crate) fn forward_interrupt() {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(RUN_CHILD.load(Ordering::Relaxed))
        && pid != 0
    {
        // The child may have just ended; a signal then has no one to reach.
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), FORWARD);
    }
}

/// Whether a Ctrl-C ended a process: the forwarded signal on Unix (the
/// child ignores SIGINT), and on Windows the status
/// `STATUS_CONTROL_C_EXIT` of a console process that has no handler.
fn interrupted(status: std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal() == Some(FORWARD as i32)
    }
    #[cfg(windows)]
    {
        status
            .code()
            .is_some_and(|c| c.cast_unsigned() == 0xC000_013A)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = status;
        false
    }
}

impl Instance for NodeInstance {
    fn init(&mut self) -> Outcome {
        let o = self.run();
        let r = o.clone();
        self.done = Some(o);
        r
    }
    fn poll(&mut self) -> Poll<Outcome> {
        Poll::Ready(self.done.clone().unwrap_or(Outcome::Exit(0)))
    }
    fn wake(&mut self, _: &[u32]) {}
    fn call_test(&mut self, _: u32) -> Poll<Outcome> {
        Poll::Ready(Outcome::Internal(
            "a test case runs through `run_cases` on the Node engine".into(),
        ))
    }
    fn completed(&mut self) -> Vec<u32> {
        Vec::new()
    }
}

/// One finished case, or one row of an `it_each` case, as the test host
/// reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseRun {
    pub test: u32,
    pub status: i64,
    pub trapped: bool,
    pub stdout: String,
    pub stderr: String,
    pub us: u64,
    /// An `it_each` row: its index and the table's row count
    /// (`std-testing.it-each.name`). A table with no rows is one result
    /// with `rows` 0 and no outcome.
    pub row: Option<(u32, u32)>,
    /// A failed property's input, shrunk when applicable, as `Debug` shows it
    /// (`std-testing.prop.failure-input`).
    pub input: Option<String>,
    pub failure: Option<String>,
    /// A Wasm trap's frames, innermost first: function index and module
    /// byte offset (`crates/hd_wasm_linear/DESIGN.md` §15.9).
    pub frames: Vec<(u32, u32)>,
    /// A trapped run's panic as the host recorded it, which the runner
    /// judges instead of the report on standard error.
    pub panic: Option<Panic>,
    /// The snapshots an update run recorded in this case, in call order
    /// (`cli.test.snapshot.update`).
    pub snapshots: Vec<Recorded>,
}

/// A failing `snapshot` call that an update run recorded instead of
/// failing at: the text it compared and the Wasm frames of the call,
/// innermost first, as a trap's are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recorded {
    pub text: String,
    pub frames: Vec<(u32, u32)>,
}

/// A panic's category and message as the host recorded them: its report,
/// `panic: CATEGORY: MESSAGE`, without parsing the text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Panic {
    pub category: String,
    pub message: String,
}

impl CaseRun {
    /// Whether this is the case's last result: a case's only one, or its
    /// table's last row.
    #[must_use]
    pub fn last(&self) -> bool {
        self.row.is_none_or(|(row, rows)| row + 1 >= rows)
    }
}

/// One case as the test host runs it: its `hd.test.i` and `hd.init.j`
/// exports, its own temporary directory (`cli.test.env.temp-dir`), which
/// the host makes on the case's first `temp_dir` call, the base seed of a
/// property test case (`cli.test.seed`), and the case's grant.
pub struct HostCase {
    pub test: u32,
    pub init: u32,
    /// The registration function: `it`, `it_each`, `it_prop` or
    /// `it_prop_with`. Only an `it_each` case runs rows.
    pub kind: String,
    pub temp_dir: PathBuf,
    pub seed: Option<i64>,
    pub grants: Grants,
    pub snapshot: Snapshot,
}

/// Where a case's snapshot files are (`std-testing.snapshot-file.path`):
/// `<root>/<folder>/<slug>-<n>.snap`, and whether this is an update run,
/// which records them (`std-testing.snapshot-file.missing`).
pub struct Snapshot {
    pub root: PathBuf,
    /// `__snapshots__/<module>`, as a message shows it.
    pub folder: String,
    /// `__regressions__/<module>`, where a property case keeps its file
    /// `<slug>` (`std-testing.prop.regression-file`).
    pub regressions: String,
    pub slug: String,
    pub update: bool,
}

/// What the cases of one test program share (Test Environments): an
/// integration test's or a doc test's working directory, the package
/// directory (`cli.test.env.cwd`), and the path that `Args.program`
/// returns (`cli.test.env.args.program`). Every case's program arguments
/// are empty (`cli.test.env.args`) and its standard input is closed
/// (`cli.test.env.stdin`).
pub struct TestEnv<'a> {
    pub cwd: Option<&'a Path>,
    pub program: &'a str,
    /// The programs its `Process` provider starts by name, an integration
    /// test's or a doc test's (`cli.test.process`).
    pub programs: Option<&'a Programs>,
    /// The `--time-limit` in milliseconds that each case gets on its own
    /// (`cli.limit.test`).
    pub wall_ms: Option<u64>,
}

/// The test host's configuration: the program's shared environment, then
/// each case's.
fn host_config(env: &TestEnv<'_>, cases: &[HostCase]) -> String {
    let programs = env.programs.map_or("null", |p| p.json.as_str());
    let launcher = process_launcher();
    let limit = env
        .wall_ms
        .map_or(String::new(), |ms| format!("\"timeLimitMs\":{ms},"));
    let mut out = format!(
        "{{\"program\":{},\"args\":[],{limit}\"programs\":{programs}{launcher},\"cases\":[",
        quote(env.program)
    );
    for (i, c) in cases.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let seed = c.seed.map_or_else(|| "null".to_owned(), |s| s.to_string());
        let s = &c.snapshot;
        // A property case's regression file (`std-testing.prop.regression-file`).
        let regression = if c.seed.is_some() {
            let shown = format!("{}/{}", s.regressions, s.slug);
            format!(
                "{{\"file\":{},\"shown\":{}}}",
                quote(&s.root.join(&shown).to_string_lossy()),
                quote(&shown)
            )
        } else {
            "null".to_owned()
        };
        let _ = write!(
            out,
            "{{\"test\":{},\"init\":{},\"kind\":{},\"tempDir\":{},\"seed\":{seed},\"grants\":{},\"snapshot\":{{\"dir\":{},\"shown\":{},\"slug\":{},\"update\":{}}},\"regression\":{regression}}}",
            c.test,
            c.init,
            quote(&c.kind),
            quote(&c.temp_dir.to_string_lossy()),
            grants_json(&c.grants),
            quote(&s.root.join(&s.folder).to_string_lossy()),
            quote(&s.folder),
            quote(&s.slug),
            s.update
        );
    }
    out.push_str("]}");
    out
}

/// The package's executables and tasks as the test runner's `Process`
/// provider starts them (`cli.test.process`, `.process.tasks`): each built
/// module, with the run host and a configuration of its `Args.program` and
/// grant, in a scratch directory that lives as long as this value.
pub struct Programs {
    _dir: Scratch,
    json: String,
}

impl Programs {
    /// `list` holds each program's name, module and grant; each runs with
    /// `cwd`, the package directory, as its working directory
    /// (`cli.test.process.cwd`).
    pub fn new(list: &[(String, Vec<u8>, Grants)], cwd: &Path) -> Result<Programs, String> {
        let dir = Scratch::new(&[])?;
        let mut json = String::from("{");
        for (i, (name, wasm, grants)) in list.iter().enumerate() {
            let module = dir.0.join(format!("program-{i}.wasm"));
            let config = dir.0.join(format!("program-{i}.json"));
            std::fs::write(&module, wasm).map_err(|e| format!("{}: {e}", module.display()))?;
            std::fs::write(&config, run_config(name, grants, (None, None), None))
                .map_err(|e| format!("{}: {e}", config.display()))?;
            if i > 0 {
                json.push(',');
            }
            let _ = write!(
                json,
                "{}:{{\"host\":{},\"wasm\":{},\"config\":{},\"cwd\":{}}}",
                quote(name),
                quote(&dir.0.join("run.mjs").to_string_lossy()),
                quote(&module.to_string_lossy()),
                quote(&config.to_string_lossy()),
                quote(&cwd.to_string_lossy())
            );
        }
        json.push('}');
        Ok(Programs { _dir: dir, json })
    }
}

/// Windows Node processes receive the launcher explicitly; the standalone
/// JS host never searches PATH or compiler caches for one.
fn process_launcher() -> String {
    #[cfg(windows)]
    if let Ok(executable) = std::env::current_exe() {
        return format!(
            ",\"processLauncher\":{}",
            quote(&executable.to_string_lossy())
        );
    }
    String::new()
}

/// The run host's configuration (`run.mjs`): the program's `Args.program`,
/// its grant, its time limit and its Ctrl-C's cleanup bound in
/// milliseconds, and, for `hd run`, the files where it writes the frames
/// of a trap and the status of a run whose process it kills.
fn run_config(
    program: &str,
    grants: &Grants,
    (wall_ms, cleanup_ms): (Option<u64>, Option<u64>),
    files: Option<(&Path, &Path)>,
) -> String {
    let launcher = process_launcher();
    let mut limit = wall_ms.map_or(String::new(), |ms| format!(",\"timeLimitMs\":{ms}"));
    if let Some(ms) = cleanup_ms {
        let _ = write!(limit, ",\"cleanupMs\":{ms}");
        #[cfg(unix)]
        let _ = write!(limit, ",\"interruptSignal\":\"{}\"", FORWARD.as_str());
    }
    let files = files.map_or(String::new(), |(frames, status)| {
        format!(
            ",\"framesFile\":{},\"statusFile\":{}",
            quote(&frames.to_string_lossy()),
            quote(&status.to_string_lossy())
        )
    });
    format!(
        "{{\"program\":{},\"grants\":{}{limit}{files}{launcher}}}",
        quote(program),
        grants_json(grants)
    )
}

/// A grant as the JS host reads it: each trait with a limit, as `false`
/// for a total deny or the list of its entries; a trait it leaves out has
/// no limit (`cli.cap.order.default`).
fn grants_json(g: &Grants) -> String {
    let mut out = String::from("{");
    for (i, (key, limit)) in g.limits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&quote(key));
        out.push(':');
        match limit {
            Limit::Deny => out.push_str("false"),
            Limit::Entries(list) => {
                let items: Vec<String> = list.iter().map(|e| quote(e)).collect();
                let _ = write!(out, "[{}]", items.join(","));
            }
        }
    }
    out.push('}');
    out
}

/// Runs `cases` of a test program in one Node process, each in a fresh
/// instance, and hands each result to `done` as it ends.
pub fn run_cases(
    wasm: &[u8],
    env: &TestEnv<'_>,
    cases: &[HostCase],
    done: &mut dyn FnMut(CaseRun),
) -> Result<(), String> {
    let dir = Scratch::new(wasm)?;
    let config = dir.0.join("config.json");
    std::fs::write(&config, host_config(env, cases))
        .map_err(|e| format!("{}: {e}", config.display()))?;
    let mut command = Command::new("node");
    command
        .arg(dir.0.join("test.mjs"))
        .arg(dir.0.join("main.wasm"))
        .arg(&config);
    if let Some(cwd) = env.cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("node: {e}"))?;
    let stdout = child.stdout.take().ok_or("node: no standard output")?;
    let tests: Vec<u32> = cases.iter().map(|c| c.test).collect();
    let read = read_results(BufReader::new(stdout), &tests, done);
    let seen = match read {
        Ok(seen) => seen,
        Err(e) => {
            // The host broke its protocol; it has nothing more to say.
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let out = child.wait_with_output().map_err(|e| format!("node: {e}"))?;
    if seen != cases.len() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = match stderr.trim() {
            "" => out.status.to_string(),
            text => text.to_owned(),
        };
        return Err(format!(
            "node: the test host stopped after {seen} of {} cases: {reason}",
            cases.len()
        ));
    }
    Ok(())
}

/// Reads the test host's result lines for the cases `tests` (their test
/// export indices), hands each result to `done`, and returns how many
/// cases ended. A case ends once, with its only result or its table's
/// last row; a result after that, or of a case the host was not given,
/// is an error.
fn read_results(
    input: impl BufRead,
    tests: &[u32],
    done: &mut dyn FnMut(CaseRun),
) -> Result<usize, String> {
    let mut ended = vec![false; tests.len()];
    let mut seen = 0;
    for line in input.lines() {
        let line = line.map_err(|e| format!("node: {e}"))?;
        let r = parse_case(&line).ok_or_else(|| format!("node: unreadable result `{line}`"))?;
        let at = tests.iter().position(|t| *t == r.test).ok_or_else(|| {
            format!("node: a result of a case the test host was not given `{line}`")
        })?;
        if ended[at] {
            return Err(format!(
                "node: the test host reported a case again after it ended `{line}`"
            ));
        }
        if r.last() {
            ended[at] = true;
            seen += 1;
        }
        done(r);
    }
    Ok(seen)
}

/// Reads one flat JSON object of the test host.
fn parse_case(line: &str) -> Option<CaseRun> {
    let mut r = CaseRun::default();
    let mut s = line.trim().strip_prefix('{')?.strip_suffix('}')?;
    while !s.is_empty() {
        let (key, rest) = json_str(s.trim_start())?;
        let rest = rest.trim_start().strip_prefix(':')?.trim_start();
        let rest = if rest.starts_with('[') {
            let (frames, rest) = json_frames(rest)?;
            if key == "frames" {
                r.frames = frames;
            }
            rest
        } else if rest.starts_with('"') {
            let (v, rest) = json_str(rest)?;
            match key.as_str() {
                "stdout" => r.stdout = v,
                "stderr" => r.stderr = v,
                "input" => r.input = Some(v),
                "failure" => r.failure = Some(v),
                "updates" => r.snapshots = parse_updates(&v)?,
                "category" => r.panic.get_or_insert_with(Panic::default).category = v,
                "message" => r.panic.get_or_insert_with(Panic::default).message = v,
                _ => {}
            }
            rest
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let v = rest[..end].trim();
            match key.as_str() {
                "test" => r.test = v.parse().ok()?,
                "status" => r.status = v.parse().ok()?,
                "trapped" => r.trapped = v == "true",
                "us" => r.us = v.parse().ok()?,
                "row" => r.row = Some((v.parse().ok()?, r.row.map_or(0, |x| x.1))),
                "rows" => r.row = Some((r.row.map_or(0, |x| x.0), v.parse().ok()?)),
                _ => {}
            }
            &rest[end..]
        };
        s = rest
            .trim_start()
            .strip_prefix(',')
            .unwrap_or(rest.trim_start());
    }
    Some(r)
}

/// The `updates` of a case record: a line for each recorded snapshot, the
/// call's frames as `function:offset` pairs joined by commas, a tab, and
/// the text as a JSON string (`host/cases.mjs`).
fn parse_updates(s: &str) -> Option<Vec<Recorded>> {
    s.split('\n')
        .map(|line| {
            let (frames, text) = line.split_once('\t')?;
            let frames = frames
                .split(',')
                .filter(|f| !f.is_empty())
                .map(|f| {
                    let (func, at) = f.split_once(':')?;
                    Some((func.parse().ok()?, at.parse().ok()?))
                })
                .collect::<Option<Vec<_>>>()?;
            let (text, rest) = json_str(text)?;
            rest.is_empty().then_some(Recorded { text, frames })
        })
        .collect()
}

/// A JSON array of number pairs at the start of `s`, `[[1,2],[3,4]]`, and
/// the rest after it.
fn json_frames(s: &str) -> Option<(Vec<(u32, u32)>, &str)> {
    let mut rest = s.strip_prefix('[')?.trim_start();
    let mut out = Vec::new();
    loop {
        if let Some(after) = rest.strip_prefix(']') {
            return Some((out, after));
        }
        let pair = rest
            .trim_start_matches(',')
            .trim_start()
            .strip_prefix('[')?;
        let end = pair.find(']')?;
        let (f, off) = pair[..end].split_once(',')?;
        out.push((f.trim().parse().ok()?, off.trim().parse().ok()?));
        rest = pair[end + 1..].trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
        }
    }
}

/// A JSON string at the start of `s`, decoded, and the rest after it.
pub(crate) fn json_str(s: &str) -> Option<(String, &str)> {
    let body = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &body[i + 1..])),
            '\\' => {
                let (_, e) = chars.next()?;
                match e {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            code = code * 16 + chars.next()?.1.to_digit(16)?;
                        }
                        // A surrogate pair is two escapes.
                        if (0xD800..0xDC00).contains(&code) {
                            let rest: String = chars.by_ref().take(6).map(|x| x.1).collect();
                            let low = u32::from_str_radix(rest.strip_prefix("\\u")?, 16).ok()?;
                            let hi = code.checked_sub(0xD800)?;
                            let lo = low.checked_sub(0xDC00).filter(|l| *l < 0x400)?;
                            code = 0x10000 + (hi << 10) + lo;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use hd_run::{Grants, Limit};

    use super::{HostCase, Snapshot, TestEnv, host_config, parse_case, quote, read_results};

    /// Each case ends once (#161): the host's stream that once reported a
    /// time-limit cut twice is an error, never `2 of 1 cases`.
    #[test]
    fn counts_each_case_once() {
        let cut = r#"{"test":3,"status":3,"trapped":true,"stdout":"","stderr":"","us":550000}"#;
        let rows = [
            r#"{"test":5,"row":0,"rows":2,"status":0,"trapped":false,"stdout":"","stderr":"","us":1}"#,
            r#"{"test":5,"row":1,"rows":2,"status":0,"trapped":false,"stdout":"","stderr":"","us":1}"#,
        ];
        let mut got = Vec::new();
        let input = format!("{cut}\n{}\n{}\n", rows[0], rows[1]);
        let seen = read_results(input.as_bytes(), &[3, 5, 7], &mut |r| got.push(r.test));
        assert_eq!(seen, Ok(2));
        assert_eq!(got, [3, 5, 5]);

        let twice = format!("{cut}\n{cut}\n");
        let mut got = Vec::new();
        let seen = read_results(twice.as_bytes(), &[3], &mut |r| got.push(r.test));
        assert_eq!(
            seen,
            Err(format!(
                "node: the test host reported a case again after it ended `{cut}`"
            ))
        );
        assert_eq!(got, [3]);

        let stranger = read_results(cut.as_bytes(), &[4], &mut |_| {});
        assert!(stranger.is_err());
    }

    #[test]
    fn writes_the_host_config() {
        let cases = [HostCase {
            test: 2,
            init: 1,
            kind: "it_prop".into(),
            temp_dir: PathBuf::from("/tmp/hd-test-1-0/4"),
            seed: Some(9),
            grants: Grants {
                limits: vec![
                    ("Console".into(), Limit::Deny),
                    ("FsWrite".into(), Limit::Entries(vec!["/tmp/a\"b".into()])),
                ],
            },
            snapshot: Snapshot {
                root: PathBuf::from("/pkg"),
                folder: "__snapshots__/tests.report".into(),
                regressions: "__regressions__/tests.report".into(),
                slug: "sums-prices".into(),
                update: false,
            },
        }];
        let env = TestEnv {
            cwd: Some(Path::new("/pkg")),
            program: "tests/report.hd",
            programs: None,
            wall_ms: Some(300),
        };
        // `Path::join` writes the platform separator after the root, so
        // Windows has `/pkg\__snapshots__` (JSON-escaped below).
        let sep = if cfg!(windows) { r"\\" } else { "/" };
        let launcher = if cfg!(windows) {
            format!(
                ",\"processLauncher\":{}",
                quote(&std::env::current_exe().unwrap().to_string_lossy())
            )
        } else {
            String::new()
        };
        assert_eq!(
            host_config(&env, &cases),
            format!(
                r#"{{"program":"tests/report.hd","args":[],"timeLimitMs":300,"programs":null{launcher},"cases":[{{"test":2,"init":1,"kind":"it_prop","tempDir":"/tmp/hd-test-1-0/4","seed":9,"grants":{{"Console":false,"FsWrite":["/tmp/a\"b"]}},"snapshot":{{"dir":"/pkg{sep}__snapshots__/tests.report","shown":"__snapshots__/tests.report","slug":"sums-prices","update":false}},"regression":{{"file":"/pkg{sep}__regressions__/tests.report/sums-prices","shown":"__regressions__/tests.report/sums-prices"}}}}]}}"#
            )
        );
    }

    #[test]
    fn reads_a_case_line() {
        let r = parse_case(
            r#"{"test":2,"status":3,"trapped":true,"stdout":"a\"b\n","stderr":"xé😀","us":15}"#,
        )
        .expect("parses");
        assert_eq!(r.test, 2);
        assert_eq!(r.status, 3);
        assert!(r.trapped);
        assert_eq!(r.stdout, "a\"b\n");
        assert_eq!(r.stderr, "x\u{e9}\u{1f600}");
        assert_eq!(r.us, 15);
        assert!(r.last());
        let row = parse_case(
            r#"{"test":2,"row":1,"rows":3,"status":0,"trapped":false,"stdout":"","stderr":"","us":1}"#,
        )
        .expect("parses");
        assert_eq!(row.row, Some((1, 3)));
        assert!(!row.last());
        let failed = parse_case(
            r#"{"test":4,"status":3,"trapped":true,"stdout":"","stderr":"","us":1,"input":"Point { x: 50 }"}"#,
        )
        .expect("parses");
        assert_eq!(failed.input.as_deref(), Some("Point { x: 50 }"));
        let empty = parse_case(r#"{"test":2,"row":0,"rows":0}"#).expect("parses");
        assert!(empty.last());
        let trapped = parse_case(
            r#"{"test":1,"status":3,"trapped":true,"stdout":"","stderr":"","us":1,"frames":[[12,4096],[7,300]]}"#,
        )
        .expect("parses");
        assert_eq!(trapped.frames, [(12, 4096), (7, 300)]);
        assert_eq!(trapped.us, 1);
        assert_eq!(trapped.panic, None);
        let panicked = parse_case(
            r#"{"test":1,"status":3,"trapped":true,"stdout":"","stderr":"panic: explicit-panic: x\npanic: integer-overflow: y\n","us":1,"category":"explicit-panic","message":"x\npanic: integer-overflow: y"}"#,
        )
        .expect("parses");
        let panic = panicked.panic.expect("a panic");
        assert_eq!(panic.category, "explicit-panic");
        assert_eq!(panic.message, "x\npanic: integer-overflow: y");
    }
}
