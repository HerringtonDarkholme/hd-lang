//! Which engine runs a program: V8 through Node (`node::NodeEngine`, the
//! developer default) or wasmtime in-process (`hd_run_wasmtime`), chosen by
//! `--engine NAME` or else `HD_ENGINE`. wasmtime runs the default
//! profile's `Console`, `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`,
//! `FsWrite`, `Process`, `Random`, `Sys`, `Http`, `Net` and their handle traits; a
//! package host trait selects Node before startup unless `--engine` names
//! Wasmtime, or every one has a C provider, which selects Wasmtime. Tests use shared
//! prepared programs and typed named children on Wasmtime, while Node
//! keeps its existing subprocess protocol.
//!
//! The wasmtime engine is the `wasmtime` cargo feature, off by default.
//! The `dist` feature enables it for the optimized shipped compiler; developer
//! and test builds with debug assertions keep Node as their default.
//! `wasmtime.rs` has it; a build without the feature compiles
//! `no_wasmtime.rs` in its place, and naming the engine is an error.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use hd_driver::{EngineTarget, OptLevel};
use hd_run::journal::End;
use hd_run::{HostSetup, Limits, Outcome, run_program};

pub(crate) use crate::node::{CaseRun, HostCase, Panic, Snapshot};

#[cfg_attr(not(feature = "wasmtime"), path = "engine/no_wasmtime.rs")]
mod wasmtime;
#[cfg(feature = "wasmtime")]
pub(crate) use wasmtime::code_cache;

/// Named test children stay typed on the native engine; only Node needs
/// its private scratch-file configuration.
pub(crate) enum TestPrograms {
    Node(crate::node::Programs),
    Wasmtime(wasmtime::Programs),
}

pub(crate) struct TestProgram {
    pub(crate) name: String,
    pub(crate) wasm: Vec<u8>,
    pub(crate) grants: hd_run::Grants,
    pub(crate) providers: Vec<crate::node::HostProvider>,
    #[cfg(feature = "wasmtime")]
    pub(crate) engine: EngineKind,
}

impl TestPrograms {
    pub(crate) fn new(
        kind: EngineKind,
        list: Vec<TestProgram>,
        cwd: &std::path::Path,
    ) -> Result<Self, String> {
        match kind {
            EngineKind::Node => crate::node::Programs::new(&list, cwd).map(Self::Node),
            EngineKind::Wasmtime => wasmtime::Programs::new(list, cwd).map(Self::Wasmtime),
        }
    }
}

pub(crate) struct TestEnv<'a> {
    pub(crate) cwd: Option<&'a std::path::Path>,
    pub(crate) program: &'a str,
    pub(crate) programs: Option<&'a TestPrograms>,
}

pub(crate) enum PreparedCases<'a> {
    Node(&'a [u8]),
    Wasmtime(wasmtime::Prepared),
}

/// One engine and compiled child-program list for a package's test run.
/// Every test module is prepared before any guest starts.
pub(crate) enum TestSession {
    Node,
    Wasmtime(wasmtime::Session),
}

impl TestSession {
    pub(crate) fn new(
        kind: EngineKind,
        level: OptLevel,
        jobs: usize,
        programs: Option<&TestPrograms>,
    ) -> Result<Self, String> {
        match kind {
            EngineKind::Node => Ok(Self::Node),
            EngineKind::Wasmtime => {
                wasmtime::Session::new(level, jobs, programs).map(Self::Wasmtime)
            }
        }
    }

    pub(crate) fn prepare<'a>(
        &self,
        wasm: &'a [u8],
        integration: bool,
    ) -> Result<PreparedCases<'a>, String> {
        match self {
            Self::Node => Ok(PreparedCases::Node(wasm)),
            Self::Wasmtime(session) => session
                .prepare(wasm, integration)
                .map(PreparedCases::Wasmtime),
        }
    }

    pub(crate) fn run(
        &self,
        prepared: &PreparedCases<'_>,
        env: &TestEnv<'_>,
        cases: &[HostCase],
        limits: ProgramLimits,
        done: &mut dyn FnMut(CaseRun),
    ) -> Result<(), String> {
        match (self, prepared) {
            (Self::Node, PreparedCases::Node(wasm)) => {
                let programs = match env.programs {
                    Some(TestPrograms::Node(programs)) => Some(programs),
                    None => None,
                    Some(TestPrograms::Wasmtime(_)) => {
                        return Err("internal: Wasmtime child programs on Node".into());
                    }
                };
                let env = crate::node::TestEnv {
                    cwd: env.cwd,
                    program: env.program,
                    programs,
                    wall_ms: limits.wall_ms,
                };
                crate::node::run_cases(wasm, &env, cases, done)
            }
            (Self::Wasmtime(session), PreparedCases::Wasmtime(prepared)) => {
                session.run(prepared, env, cases, limits, done)
            }
            _ => Err("internal: prepared tests belong to another engine".into()),
        }
    }
}

/// The explicit program limit, which has no default (`cli.limit.none`),
/// and the `--cleanup-limit` of a run that a Ctrl-C may cancel
/// (`cli.cancel.flag`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProgramLimits {
    pub wall_ms: Option<u64>,
    pub cleanup_ms: Option<u64>,
}

impl From<ProgramLimits> for Limits {
    fn from(limits: ProgramLimits) -> Self {
        Self {
            fuel: None,
            wall_ms: limits.wall_ms,
            cleanup_ms: limits.cleanup_ms,
        }
    }
}

/// The cleanup bound of a Ctrl-C without `--cleanup-limit`, in
/// milliseconds (`cli.cancel.default`): ten seconds, as `docker stop` waits
/// before it kills.
pub(crate) const DEFAULT_CLEANUP_MS: u64 = 10_000;

/// The exit status of a run whose entry ended cancelled, or that a Ctrl-C
/// stopped while it computed (`cli.cancel.status`): 128 plus SIGINT's
/// number, the status a POSIX shell reports for a process that SIGINT
/// ended, on every platform alike.
pub(crate) const CANCEL_STATUS: u8 = 130;

fn scaled(value: &str, suffixes: &[(&str, u64)]) -> Option<u64> {
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (digits, suffix) = value.split_at(end);
    let number = digits.parse::<u64>().ok()?;
    let factor = suffixes.iter().find(|(s, _)| *s == suffix)?.1;
    number.checked_mul(factor)
}

/// Whole bytes, or a whole number with a binary K/M/G suffix.
pub(crate) fn parse_size(value: &str) -> Result<u64, String> {
    scaled(
        value,
        &[
            ("", 1),
            ("K", 1024),
            ("M", 1024 * 1024),
            ("G", 1024 * 1024 * 1024),
        ],
    )
    .ok_or_else(|| {
        format!("`{value}` is not a SIZE; use whole bytes or a whole number followed by K, M or G")
    })
}

/// Whole milliseconds, seconds, minutes or hours (`cli.limit.forms`).
pub(crate) fn parse_duration(value: &str) -> Result<u64, String> {
    scaled(
        value,
        &[("ms", 1), ("s", 1000), ("min", 60_000), ("h", 3_600_000)],
    )
    .ok_or_else(|| {
        format!("`{value}` is not a DURATION; use a whole number followed by ms, s, min or h")
    })
}

/// `--time-limit DURATION` at `args[i]` (`cli.limit.time`): the limit in
/// milliseconds, and the number of words it used.
pub(crate) fn time_limit_flag(args: &[OsString], i: usize) -> Option<Result<(u64, usize), String>> {
    duration_flag(args, i, "--time-limit")
}

/// `--cleanup-limit DURATION` at `args[i]` (`cli.cancel.flag`): the
/// cleanup bound of a Ctrl-C in milliseconds, and the number of words it
/// used.
pub(crate) fn cleanup_limit_flag(
    args: &[OsString],
    i: usize,
) -> Option<Result<(u64, usize), String>> {
    duration_flag(args, i, "--cleanup-limit")
}

/// The flag `flag DURATION`, or `flag=DURATION`, at `args[i]`: the
/// duration in milliseconds, and the number of words it used.
fn duration_flag(args: &[OsString], i: usize, flag: &str) -> Option<Result<(u64, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (value, used) = if text == flag {
        let Some(value) = args.get(i + 1) else {
            return Some(Err(format!("`{flag}` needs a value")));
        };
        (value.to_string_lossy().into_owned(), 2)
    } else {
        (text.strip_prefix(flag)?.strip_prefix('=')?.to_owned(), 1)
    };
    Some(
        parse_duration(&value)
            .map(|ms| (ms, used))
            .map_err(|e| format!("{flag}: {e}")),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EngineKind {
    Node,
    Wasmtime,
}

impl Default for EngineKind {
    fn default() -> Self {
        if cfg!(all(feature = "dist", not(debug_assertions))) {
            Self::Wasmtime
        } else {
            Self::Node
        }
    }
}

impl EngineKind {
    /// The stack a test worker thread needs, when the engine asks for one.
    pub(crate) fn worker_stack(self, level: OptLevel) -> Option<usize> {
        match self {
            Self::Node => None,
            Self::Wasmtime => Some(wasmtime::worker_stack(level)),
        }
    }

    /// The target a build that this engine runs is tuned for
    /// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5).
    pub(crate) const fn target(self) -> EngineTarget {
        match self {
            Self::Node => EngineTarget::V8,
            Self::Wasmtime => EngineTarget::Wasmtime,
        }
    }
}

pub(crate) fn parse(name: &str) -> Result<EngineKind, String> {
    match name {
        "node" => Ok(EngineKind::Node),
        "wasmtime" => match wasmtime::UNAVAILABLE {
            None => Ok(EngineKind::Wasmtime),
            Some(why) => Err(why.to_owned()),
        },
        _ => Err(format!(
            "`{name}` is no engine; the engines are `node` and `wasmtime`"
        )),
    }
}

/// `--engine NAME` or `--engine=NAME` at `args[i]`, and the words it used.
pub(crate) fn engine_flag(
    args: &[OsString],
    i: usize,
) -> Option<Result<(EngineKind, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (name, used) = if text == "--engine" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--engine` needs NAME".to_owned())),
        }
    } else {
        (text.strip_prefix("--engine=")?.to_owned(), 1)
    };
    Some(parse(&name).map(|k| (k, used)))
}

/// The engine a command's words chose, and whether an `--engine` flag
/// named it. Only a named engine is kept for a module that needs a
/// package host trait (`cli.host.package.native-cli-choice`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Choice {
    pub(crate) kind: EngineKind,
    pub(crate) named: bool,
}

impl Choice {
    /// `--engine NAME`'s engine.
    pub(crate) const fn named(kind: EngineKind) -> Self {
        Self { kind, named: true }
    }

    /// The engine of a command with no `--engine` flag: `HD_ENGINE`, else this build's default.
    pub(crate) fn from_env() -> Result<Self, String> {
        from_env().map(|kind| Self { kind, named: false })
    }

    /// The engine that runs a module whose package host contract is
    /// `contract`, decided once before the module loads
    /// (`cli.host.package.native-cli-startup`); `c` holds the import
    /// modules of its C-bound root interfaces (`hd_host_abi::c`). With no
    /// `--engine`, a module whose every root interface has a C provider
    /// runs on Wasmtime, in a build that has it, and any other module with
    /// a root interface on Node (`native-cli-choice`). A named Wasmtime
    /// refuses a root interface no C provider serves, naming the trait:
    /// `hd` registers no other native provider (`native-cli-binds-c`,
    /// `native-cli-named`). Cost: one pass over the contract's
    /// interfaces, which the caller decoded once for the run, with one
    /// probe of `c` each.
    pub(crate) fn for_module(
        self,
        contract: Option<&hd_host_abi::schema::Contract>,
        c: &std::collections::BTreeSet<String>,
    ) -> Result<EngineKind, String> {
        let mut roots = contract
            .into_iter()
            .flat_map(|c| &c.interfaces)
            .filter(|i| !i.handle)
            .peekable();
        if roots.peek().is_none() {
            return Ok(self.kind);
        }
        let uncovered = roots.find(|i| !c.contains(&i.module));
        if !self.named {
            return Ok(match uncovered {
                None if cfg!(feature = "wasmtime") => EngineKind::Wasmtime,
                _ => EngineKind::Node,
            });
        }
        match (self.kind, uncovered) {
            (EngineKind::Node, _) | (EngineKind::Wasmtime, None) => Ok(self.kind),
            (EngineKind::Wasmtime, Some(root)) => Err(format!(
                "no native provider registered for package host trait {:?}: it has no C provider, \
                 so `--engine wasmtime` cannot run this program; another native provider needs a Rust \
                 program that embeds `hd_run_wasmtime`, and without `--engine`, `hd` runs it on Node",
                root.identity.to_string()
            )),
        }
    }
}

/// The engine of a run with no `--engine` flag: `HD_ENGINE`, else this build's default.
pub(crate) fn from_env() -> Result<EngineKind, String> {
    match std::env::var("HD_ENGINE") {
        Ok(name) if !name.is_empty() => parse(&name).map_err(|e| format!("HD_ENGINE: {e}")),
        _ => Ok(EngineKind::default()),
    }
}

/// Runs a built program to its end on `kind`, with the process's standard
/// streams, with the Wasm frames of the trap it ended in, innermost first
/// (empty when it ended any other way). `level` selects wasmtime's engine
/// options.
///
/// A run with a cleanup bound (`Limits::cleanup_ms`) hears Ctrl-C
/// (`cli.cancel.unhandled-ctrl-c`): on wasmtime the handler asks the run to cancel
/// its entry; on Node the program's own process hears the same Ctrl-C from
/// the terminal, so `hd` passes each interrupt on to it
/// (`cli.cancel.forward`) and keeps itself alive until it ends.
pub(crate) fn run(
    kind: EngineKind,
    level: OptLevel,
    image: hd_run::artifact::Image<'_>,
    host: &HostSetup,
    (providers, c): (&[crate::node::HostProvider], &[crate::node::CLibrary]),
    sampling: Option<&Path>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    match kind {
        EngineKind::Node => {
            if sampling.is_some() {
                return Err("guest profiling requires `--engine wasmtime`".into());
            }
            let hd_run::artifact::Image::Wasm(wasm) = image else {
                return Err("a .cwasm module requires wasmtime; build `--format wasm` for a portable module".into());
            };
            let store = crate::disk_store();
            let node = crate::node::NodeEngine {
                host_providers: providers.to_vec(),
                ..crate::node::NodeEngine::default()
            };
            let outcome = run_program(&node, wasm, &store, host).map_err(|e| e.to_string())?;
            let frames = node.trace.lock().map(|t| t.clone()).unwrap_or_default();
            Ok((outcome, frames))
        }
        EngineKind::Wasmtime => {
            // The native engine binds the C providers `c` and refuses any
            // other reachable package trait before guest start.
            wasmtime::run_program(level, image, host, c, sampling)
        }
    }
}

/// `--profile[=FILE]`: an optional joined path, without consuming a task
/// name or a guest argument. Native path bytes are preserved.
pub(crate) fn sampling_flag(argument: &OsStr) -> Option<Result<PathBuf, String>> {
    if argument == "--profile" {
        return Some(Ok(PathBuf::from("profile.json")));
    }
    let bytes = argument.as_encoded_bytes();
    let suffix = bytes.strip_prefix(b"--profile=")?;
    if suffix.is_empty() {
        return Some(Err("`--profile=FILE` needs a nonempty FILE".into()));
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt as _;
        PathBuf::from(OsStr::from_bytes(suffix))
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
        PathBuf::from(OsString::from_wide(
            &argument
                .encode_wide()
                .skip(b"--profile=".len())
                .collect::<Vec<_>>(),
        ))
    };
    #[cfg(not(any(unix, windows)))]
    let path = match std::str::from_utf8(suffix) {
        Ok(path) => PathBuf::from(path),
        Err(error) => return Some(Err(format!("invalid profile path: {error}"))),
    };
    Some(Ok(path))
}

/// Scoped cancellation when no program wait owns the delivered kind.
#[cfg(feature = "wasmtime")]
pub(crate) fn on_ctrl_c(
    handler: impl Fn(hd_run::signals::Kind) + Send + Sync + 'static,
) -> Vec<hd_run::signals::Subscription> {
    use hd_run::signals::Kind;
    match hd_run::signals::fallback(&[Kind::Interrupt, Kind::Terminate], handler) {
        Ok(subscriptions) => subscriptions,
        Err(error) => {
            eprintln!("warning: Ctrl-C cannot cancel the program: {error}");
            Vec::new()
        }
    }
}

/// Streams of the shared REPL loop. Keep the native stream type when that
/// engine is present, so existing embedders and tests keep their API.
#[cfg(feature = "wasmtime")]
pub(crate) use hd_run_wasmtime::HostIo;

#[cfg(not(feature = "wasmtime"))]
pub(crate) struct HostIo {
    pub(crate) out: Box<dyn std::io::Write + Send>,
    pub(crate) err: Box<dyn std::io::Write + Send>,
    pub(crate) input: Option<Box<dyn std::io::BufRead + Send>>,
}

#[cfg(not(feature = "wasmtime"))]
impl HostIo {
    pub(crate) fn process() -> Self {
        Self {
            out: Box::new(std::io::stdout()),
            err: Box::new(std::io::stderr()),
            input: Some(Box::new(std::io::BufReader::new(std::io::stdin()))),
        }
    }
}

#[derive(Clone)]
pub(crate) enum SessionStopper {
    // Scripted sessions install no interrupt handler, on either engine.
    Node,
    #[cfg(feature = "wasmtime")]
    Wasmtime(hd_run_wasmtime::Stopper),
}

#[cfg(feature = "wasmtime")]
impl SessionStopper {
    pub(crate) fn stop(&self) {
        if let Self::Wasmtime(stopper) = self {
            stopper.stop();
        }
    }

    #[cfg(test)]
    pub(crate) fn waiting(&self) -> bool {
        match self {
            Self::Node => false,
            Self::Wasmtime(stopper) => stopper.waiting(),
        }
    }
}

/// What one run of a session program did (`hd_run_wasmtime::Ran`): the
/// inputs it replayed, skipped (`cli.repl.rebuild.stopped`) and dropped
/// (`cli.repl.rebuild.prefix`), and how the new input ended, when it ran.
pub(crate) struct Ran {
    pub(crate) kept: Vec<u32>,
    pub(crate) skipped: Vec<u32>,
    pub(crate) dropped: Vec<u32>,
    pub(crate) reason: Option<String>,
    pub(crate) live: Option<(Outcome, End)>,
}

pub(crate) enum ReplSession {
    Node(Box<NodeSession>),
    #[cfg(feature = "wasmtime")]
    Wasmtime(Box<hd_run_wasmtime::WasmtimeSession>),
}

#[cfg(feature = "wasmtime")]
impl From<hd_run_wasmtime::WasmtimeSession> for ReplSession {
    fn from(session: hd_run_wasmtime::WasmtimeSession) -> Self {
        Self::Wasmtime(Box::new(session))
    }
}

impl ReplSession {
    pub(crate) fn new(kind: EngineKind, io: HostIo, cwd: &std::path::Path) -> Result<Self, String> {
        match kind {
            EngineKind::Node => {
                NodeSession::new(io, cwd).map(|session| Self::Node(Box::new(session)))
            }
            #[cfg(feature = "wasmtime")]
            EngineKind::Wasmtime => {
                let setup = HostSetup {
                    grants: hd_run::Grants::default(),
                    limits: Limits::default(),
                    args: Vec::new(),
                    program: String::new(),
                };
                let engine = hd_run_wasmtime::WasmtimeEngine::new(
                    hd_run_wasmtime::EngineConfig::pipeline(OptLevel::Dev),
                )?;
                engine
                    .session(&setup, io)
                    .map(Into::into)
                    .map_err(|error| match error {
                        hd_run::StartError::Refused(message)
                        | hd_run::StartError::Engine(message) => message,
                    })
            }
            #[cfg(not(feature = "wasmtime"))]
            EngineKind::Wasmtime => {
                Err(wasmtime::UNAVAILABLE.unwrap_or("no wasmtime engine").into())
            }
        }
    }

    pub(crate) fn kind(&self) -> EngineKind {
        match self {
            Self::Node(_) => EngineKind::Node,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(_) => EngineKind::Wasmtime,
        }
    }

    pub(crate) fn stopper(&self) -> SessionStopper {
        match self {
            Self::Node(_) => SessionStopper::Node,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => SessionStopper::Wasmtime(session.stopper()),
        }
    }

    /// Runs the session program `wasm`: replays the session's inputs from
    /// its journal, each of its text in `sources`, then runs input `live`
    /// when one is asked for (`hd_run_wasmtime::WasmtimeSession::run`).
    pub(crate) fn run(
        &mut self,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        match self {
            Self::Node(session) => session.run(wasm, sources, live),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => {
                let ran = session.run(wasm, sources, live)?;
                Ok(Ran {
                    kept: ran.kept,
                    skipped: ran.skipped,
                    dropped: ran.dropped,
                    reason: ran.reason,
                    live: ran.live,
                })
            }
        }
    }

    /// Forget an empty rebuilt prefix without replacing its provider.
    pub(crate) fn clear(&mut self) -> Result<(), String> {
        match self {
            Self::Node(session) => {
                session.open()?;
                session.send("{\"op\":\"clear\"}")?;
                if session.receive()? != "cleared" {
                    return Err("the Node session did not clear its inputs".into());
                }
                session.has_journal = true;
                session.notice = false;
                Ok(())
            }
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.clear(),
        }
    }

    pub(crate) fn set_journal_cap(&mut self, cap: usize) {
        match self {
            Self::Node(session) => session.cap = cap,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.set_journal_cap(cap),
        }
    }

    /// Whether the journal still records, so the session can run another
    /// input.
    pub(crate) fn recording(&self) -> bool {
        match self {
            Self::Node(session) => session.has_journal,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.journal().is_some(),
        }
    }

    /// The journal's size in bytes so far; 0 on Node, whose journal
    /// stays in its process.
    #[cfg(all(test, feature = "wasmtime"))]
    pub(crate) fn journal_len(&self) -> usize {
        match self {
            Self::Node(_) => 0,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.journal().map_or(0, <[u8]>::len),
        }
    }

    pub(crate) fn take_journal_notice(&mut self) -> bool {
        match self {
            Self::Node(session) => std::mem::take(&mut session.notice),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.take_journal_notice(),
        }
    }
}

/// One persistent Node session. Its protocol carries output and guest
/// reads in order; the real standard input is never the control pipe.
pub(crate) struct NodeSession {
    child: std::process::Child,
    control: std::process::ChildStdin,
    replies: std::io::BufReader<std::process::ChildStdout>,
    scratch: crate::node::Scratch,
    io: HostIo,
    opened: bool,
    cap: usize,
    has_journal: bool,
    notice: bool,
}

impl NodeSession {
    fn new(io: HostIo, cwd: &std::path::Path) -> Result<Self, String> {
        use std::process::Stdio;
        let scratch = crate::node::Scratch::new(&[])?;
        // cost: one Node process per session.
        let mut child = crate::node::node_command()
            .args(["--input-type=module", "--eval", "import { pathToFileURL } from 'node:url'; const { driveNodeSession } = await import(pathToFileURL(process.argv[1])); await driveNodeSession(); process.exit(0);"])
            .arg(scratch.0.join("node.js"))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn().map_err(|error| format!("the Node session: {error}"))?;
        let control = child
            .stdin
            .take()
            .ok_or("the Node session has no control stream")?;
        let replies = std::io::BufReader::new(
            child
                .stdout
                .take()
                .ok_or("the Node session has no reply stream")?,
        );
        Ok(Self {
            child,
            control,
            replies,
            scratch,
            io,
            opened: false,
            cap: hd_run::journal::SESSION_CAP,
            has_journal: true,
            notice: false,
        })
    }

    fn send(&mut self, request: &str) -> Result<(), String> {
        use std::io::Write;
        writeln!(self.control, "{request}")
            .and_then(|()| self.control.flush())
            .map_err(|error| format!("the Node session control stream: {error}"))
    }

    fn receive(&mut self) -> Result<String, String> {
        use std::io::BufRead;
        loop {
            let mut row = String::new();
            if self
                .replies
                .read_line(&mut row)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err("the Node session ended without a reply".into());
            }
            let row = row.trim_end_matches(['\r', '\n']);
            if let Some(text) = row.strip_prefix("out\t") {
                let text = Self::text(text)?;
                self.io
                    .out
                    .write_all(text.as_bytes())
                    .and_then(|()| self.io.out.flush())
                    .map_err(|error| error.to_string())?;
            } else if let Some(text) = row.strip_prefix("err\t") {
                let text = Self::text(text)?;
                self.io
                    .err
                    .write_all(text.as_bytes())
                    .and_then(|()| self.io.err.flush())
                    .map_err(|error| error.to_string())?;
            } else if let Some(max) = row.strip_prefix("bytes\t") {
                let max: usize = max
                    .parse::<u32>()
                    .map_err(|error| format!("invalid session input size: {error}"))?
                    as usize;
                // BufRead's fill/consume pair shares unread bytes with the
                // text reads, without allocating the guest's requested max.
                let value = match self.io.input.as_mut() {
                    None => "[]".into(),
                    Some(input) => match input.fill_buf() {
                        Ok(bytes) => {
                            let count = max.min(bytes.len());
                            let value = serde_json::to_string(&bytes[..count])
                                .map_err(|error| error.to_string())?;
                            input.consume(count);
                            value
                        }
                        Err(_) => "false".into(),
                    },
                };
                self.send(&format!("{{\"value\":{value}}}"))?;
            } else if matches!(row, "input" | "all") {
                let mut bytes = Vec::new();
                let value = match self.io.input.as_mut() {
                    None if row == "all" => crate::report::quote(""),
                    None => "null".into(),
                    Some(input) if row == "all" => match input.read_to_end(&mut bytes) {
                        Ok(_) => crate::report::quote(&String::from_utf8_lossy(&bytes)),
                        Err(_) => "false".into(),
                    },
                    Some(input) => match input.read_until(b'\n', &mut bytes) {
                        Ok(0) => "null".into(),
                        Ok(_) => {
                            if bytes.last() == Some(&b'\n') {
                                bytes.pop();
                                if bytes.last() == Some(&b'\r') {
                                    bytes.pop();
                                }
                            }
                            crate::report::quote(&String::from_utf8_lossy(&bytes))
                        }
                        Err(_) => "false".into(),
                    },
                };
                self.send(&format!("{{\"value\":{value}}}"))?;
            } else if let Some(error) = row.strip_prefix("error\t") {
                return Err(Self::text(error)?);
            } else {
                return Ok(row.to_owned());
            }
        }
    }

    fn text(json: &str) -> Result<String, String> {
        crate::node::json_str(json)
            .filter(|(_, rest)| rest.is_empty())
            .map(|(text, _)| text)
            .ok_or_else(|| "the Node session sent an invalid text frame".into())
    }

    fn open(&mut self) -> Result<(), String> {
        if !self.opened {
            self.send(&format!("{{\"op\":\"open\",\"cap\":{}}}", self.cap))?;
            if self.receive()? != "ready" {
                return Err("the Node session did not start".into());
            }
            self.opened = true;
        }
        Ok(())
    }

    fn run(
        &mut self,
        wasm: &[u8],
        sources: &[(u32, &str)],
        live: Option<u32>,
    ) -> Result<Ran, String> {
        self.open()?;
        let path = self.scratch.0.join("session.wasm");
        std::fs::write(&path, wasm).map_err(|error| error.to_string())?;
        let pairs: Vec<String> = sources
            .iter()
            .map(|(n, text)| format!("[{n},{}]", crate::report::quote(text)))
            .collect();
        self.send(&format!(
            "{{\"op\":\"run\",\"wasm\":{},\"sources\":[{}],\"live\":{}}}",
            crate::report::quote(&path.to_string_lossy()),
            pairs.join(","),
            live.map_or_else(|| "null".to_owned(), |n| n.to_string()),
        ))?;
        let row = self.receive()?;
        let Some(fields) = row.strip_prefix("ran\t") else {
            return Err("the Node session sent no run outcome".into());
        };
        let fields: Vec<&str> = fields.split('\t').collect();
        let [
            kept,
            skipped,
            dropped,
            reason,
            status,
            end,
            error,
            recording,
            notice,
        ] = fields[..]
        else {
            return Err("the Node session sent an invalid run outcome".into());
        };
        let numbers = |list: &str| -> Result<Vec<u32>, String> {
            list.split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().map_err(|_| "invalid input number".to_owned()))
                .collect()
        };
        let reason = match reason {
            "null" => None,
            text => Some(Self::text(text)?),
        };
        let live = match (status, end) {
            ("-", _) | (_, "-") => None,
            (status, end) => {
                let end = match end {
                    "trapped" => End::Trapped,
                    "stopped" => End::Stopped,
                    "cancelled" => End::Cancelled,
                    "failed" => End::Failed,
                    exit => End::Exit(
                        exit.strip_prefix("exit")
                            .and_then(|s| s.parse().ok())
                            .ok_or("the Node session sent an invalid end")?,
                    ),
                };
                let outcome = match status.parse::<u8>() {
                    Ok(status) => Outcome::Exit(status),
                    Err(_) => Outcome::Internal(match error {
                        "null" => "the input failed".to_owned(),
                        text => Self::text(text)?,
                    }),
                };
                Some((outcome, end))
            }
        };
        self.has_journal = recording == "1";
        self.notice = notice == "1";
        Ok(Ran {
            kept: numbers(kept)?,
            skipped: numbers(skipped)?,
            dropped: numbers(dropped)?,
            reason,
            live,
        })
    }
}

impl Drop for NodeSession {
    fn drop(&mut self) {
        if self.send("{\"op\":\"close\"}").is_err() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

pub(crate) fn native_key() -> Result<hd_base::Hash128, String> {
    wasmtime::native_key()
}
pub(crate) fn precompile(wasm: &[u8]) -> Result<Vec<u8>, String> {
    wasmtime::precompile(wasm)
}

#[cfg(test)]
mod tests {
    use super::{EngineKind, ProgramLimits, parse_duration, parse_size, time_limit_flag};
    use hd_run::Limits;
    use std::ffi::OsString;

    #[test]
    fn sampling_flag_keeps_task_and_guest_arguments_separate() {
        use std::ffi::OsStr;
        use std::path::PathBuf;
        assert_eq!(
            super::sampling_flag(OsStr::new("--profile")),
            Some(Ok(PathBuf::from("profile.json")))
        );
        assert_eq!(
            super::sampling_flag(OsStr::new("--profile=cpu samples.json")),
            Some(Ok(PathBuf::from("cpu samples.json")))
        );
        assert!(matches!(
            super::sampling_flag(OsStr::new("--profile=")),
            Some(Err(_))
        ));
        assert_eq!(super::sampling_flag(OsStr::new("--profiled")), None);
    }

    #[cfg(unix)]
    #[test]
    fn sampling_flag_preserves_native_path_bytes() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::ffi::OsStrExt;
        let argument = std::ffi::OsStr::from_bytes(b"--profile=samples-\xff.json");
        let path = super::sampling_flag(argument).ok_or("profile flag")??;
        assert_eq!(path.as_os_str().as_bytes(), b"samples-\xff.json");
        Ok(())
    }

    #[cfg(all(feature = "dist", not(debug_assertions)))]
    #[test]
    fn shipped_build_defaults_to_wasmtime() {
        assert_eq!(EngineKind::default(), EngineKind::Wasmtime);
        assert!(super::wasmtime::UNAVAILABLE.is_none());
    }

    #[cfg(not(all(feature = "dist", not(debug_assertions))))]
    #[test]
    fn developer_and_test_builds_default_to_node() {
        assert_eq!(EngineKind::default(), EngineKind::Node);
    }

    #[cfg(feature = "wasmtime")]
    #[test]
    fn native_named_children_retain_modules_and_grants_without_node_configuration() {
        use super::TestPrograms;
        let grants = hd_run::Grants::with_limits(vec![("Env".into(), hd_run::Limit::Deny)]);
        let list = vec![super::TestProgram {
            name: "tool".to_owned(),
            wasm: vec![0, 97, 115, 109],
            grants: grants.clone(),
            providers: Vec::new(),
            engine: EngineKind::Wasmtime,
        }];
        let programs = TestPrograms::new(
            EngineKind::Wasmtime,
            list,
            std::path::Path::new("unused-native-cwd"),
        )
        .unwrap();
        let TestPrograms::Wasmtime(programs) = programs else {
            panic!("native children must not use Node's scratch configuration");
        };
        let list = programs.native;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "tool");
        assert_eq!(list[0].1, [0, 97, 115, 109]);
        assert_eq!(list[0].2, grants);
    }

    /// `cli.host.package.native-cli-choice`, `native-cli-named`: every engine source
    /// against a module with no contract, a handle-only contract and a
    /// root interface.
    #[test]
    fn a_package_host_trait_selects_node_unless_wasmtime_is_named() {
        use super::Choice;
        use hd_host_abi::schema::{Contract, Identity, Interface};
        let interface = |trait_path: &str, handle: bool| Interface {
            identity: Identity {
                package: "sdk".into(),
                version: "1.2.0".into(),
                trait_path: trait_path.into(),
            },
            module: format!("pkg:{trait_path}"),
            selectors: Vec::new(),
            handle,
            methods: Vec::new(),
        };
        let handles = Contract {
            interfaces: vec![interface("Conn", true)],
        };
        let rooted = Contract {
            interfaces: vec![interface("Conn", true), interface("Server", false)],
        };
        let env = |kind| Choice { kind, named: false };
        let named = Choice::named;
        let none = std::collections::BTreeSet::new();
        for choice in [env(EngineKind::Wasmtime), named(EngineKind::Wasmtime)] {
            assert_eq!(choice.for_module(None, &none), Ok(EngineKind::Wasmtime));
            assert_eq!(
                choice.for_module(Some(&handles), &none),
                Ok(EngineKind::Wasmtime)
            );
        }
        for choice in [env(EngineKind::Node), named(EngineKind::Node)] {
            assert_eq!(choice.for_module(None, &none), Ok(EngineKind::Node));
            assert_eq!(
                choice.for_module(Some(&rooted), &none),
                Ok(EngineKind::Node)
            );
        }
        // Unnamed: Node, decided from the contract alone.
        assert_eq!(
            env(EngineKind::Wasmtime).for_module(Some(&rooted), &none),
            Ok(EngineKind::Node)
        );
        // Named: refused, naming the root trait and how to provide it.
        let error = named(EngineKind::Wasmtime)
            .for_module(Some(&rooted), &none)
            .unwrap_err();
        assert!(error.contains("\"sdk@1.2.0/Server\""), "{error}");
        assert!(error.contains("embeds `hd_run_wasmtime`"), "{error}");
        assert_eq!(Choice::default(), env(EngineKind::default()));
        assert_eq!(
            Choice::default().for_module(Some(&rooted), &none),
            Ok(EngineKind::Node)
        );
        // `native-cli-choice`, as decision 7 revises it: unnamed, a module whose
        // every root has a C provider runs on Wasmtime in a build that has
        // it, whatever `HD_ENGINE` says; a named Wasmtime runs it too.
        let c: std::collections::BTreeSet<String> = ["pkg:Server".to_owned()].into();
        let wasmtime = if cfg!(feature = "wasmtime") {
            EngineKind::Wasmtime
        } else {
            EngineKind::Node
        };
        for kind in [EngineKind::Node, EngineKind::Wasmtime] {
            assert_eq!(env(kind).for_module(Some(&rooted), &c), Ok(wasmtime));
        }
        assert_eq!(
            named(EngineKind::Wasmtime).for_module(Some(&rooted), &c),
            Ok(EngineKind::Wasmtime)
        );
        assert_eq!(
            named(EngineKind::Node).for_module(Some(&rooted), &c),
            Ok(EngineKind::Node)
        );
    }

    #[cfg(not(feature = "wasmtime"))]
    #[test]
    fn wasmtime_is_refused_without_the_feature() {
        use super::parse;
        let error = parse("wasmtime").unwrap_err();
        assert!(error.contains("no wasmtime engine"), "{error}");
        assert!(error.contains("--features wasmtime"), "{error}");
        assert_eq!(parse("node"), Ok(EngineKind::Node));
    }

    #[test]
    fn sizes_use_exact_binary_units_and_allow_zero() {
        for (input, expected) in [
            ("0", 0),
            ("0G", 0),
            ("0001K", 1024),
            ("65536", 65_536),
            ("256M", 256 * 1024 * 1024),
            ("2G", 2 * 1024 * 1024 * 1024),
            ("18446744073709551615", u64::MAX),
            ("18014398509481983K", u64::MAX - 1023),
        ] {
            assert_eq!(parse_size(input), Ok(expected), "{input}");
        }
    }

    #[test]
    fn sizes_reject_non_forms_and_overflow() {
        for input in [
            "",
            "K",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1 K",
            "1.0",
            "1k",
            "1m",
            "1g",
            "1KB",
            "1KiB",
            "1T",
            "1ms",
            "１",
            "١",
            "18446744073709551616",
            "18014398509481984K",
            "17592186044416M",
            "17179869184G",
        ] {
            assert!(parse_size(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn durations_use_exact_units_and_allow_zero() {
        for (input, expected) in [
            ("0ms", 0),
            ("0h", 0),
            ("0001s", 1000),
            ("500ms", 500),
            ("30s", 30_000),
            ("2min", 120_000),
            ("1h", 3_600_000),
            ("18446744073709551615ms", u64::MAX),
            ("18446744073709551s", 18_446_744_073_709_551_000),
        ] {
            assert_eq!(parse_duration(input), Ok(expected), "{input}");
        }
    }

    #[test]
    fn durations_reject_non_forms_and_overflow() {
        for input in [
            "",
            "0",
            "1",
            "ms",
            "+1s",
            "-1s",
            " 1s",
            "1s ",
            "1 s",
            "1.0s",
            "1MS",
            "1S",
            "1m",
            "1d",
            "1mins",
            "1sec",
            "1K",
            "１s",
            "18446744073709551616ms",
            "18446744073709552s",
            "307445734561826min",
            "5124095576031h",
        ] {
            assert!(parse_duration(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn time_limit_flag_accepts_both_value_forms_and_returns_words_used() {
        let separate = [
            OsString::from("other"),
            OsString::from("--time-limit"),
            OsString::from("2min"),
        ];
        assert_eq!(time_limit_flag(&separate, 1), Some(Ok((120_000, 2))));
        let joined = [OsString::from("--time-limit=2min")];
        assert_eq!(time_limit_flag(&joined, 0), Some(Ok((120_000, 1))));
    }

    #[test]
    fn time_limit_flag_reports_missing_and_invalid_values() {
        let args = [OsString::from("--time-limit")];
        assert_eq!(
            time_limit_flag(&args, 0),
            Some(Err("`--time-limit` needs a value".to_owned()))
        );
        for value in ["", "+1", "1.5s", "18446744073709551616ms"] {
            let joined = [OsString::from(format!("--time-limit={value}"))];
            let error = time_limit_flag(&joined, 0)
                .expect("recognized flag")
                .expect_err("invalid value");
            assert!(error.starts_with("--time-limit: "), "{error}");
            let separate = [OsString::from("--time-limit"), OsString::from(value)];
            assert_eq!(time_limit_flag(&separate, 0), Some(Err(error)));
        }
    }

    /// `--max-heap` and `--max-memory` are no flags of `hd`.
    #[test]
    fn time_limit_flag_leaves_other_arguments_to_the_caller() {
        for arg in [
            "--",
            "--engine=wasmtime",
            "main.hd",
            "--max-heap=1M",
            "--max-memory=4G",
            "--time-limits=1s",
        ] {
            assert_eq!(time_limit_flag(&[OsString::from(arg)], 0), None, "{arg}");
        }
        assert_eq!(time_limit_flag(&[], 0), None);
        assert_eq!(
            time_limit_flag(&[OsString::from("--time-limit=1s")], 1),
            None
        );
    }

    #[test]
    fn program_limits_preserve_omission_zero_and_explicit_values() {
        for wall_ms in [None, Some(0), Some(500)] {
            assert_eq!(
                Limits::from(ProgramLimits {
                    wall_ms,
                    cleanup_ms: wall_ms,
                }),
                Limits {
                    fuel: None,
                    wall_ms,
                    cleanup_ms: wall_ms,
                }
            );
        }
    }
}
