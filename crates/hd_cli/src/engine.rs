//! Which engine runs a program: V8 through Node (`node::NodeEngine`, the
//! default) or wasmtime in-process (`hd_run_wasmtime`), chosen by
//! `--engine NAME` or else `HD_ENGINE`. wasmtime runs the default
//! profile's `Console`, `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`,
//! `FsWrite`, `Process`, `Random`, `Sys`, `Http`, `Net` and their handle traits; a
//! program that imports another host trait is refused with a message that
//! names it (`docs/compiler/engines-and-test-runner.md` §18). Tests use shared
//! prepared programs and typed named children on Wasmtime, while Node
//! keeps its existing subprocess protocol.
//!
//! The wasmtime engine is the `wasmtime` cargo feature, off by default.
//! `wasmtime.rs` has it; a build without the feature compiles
//! `no_wasmtime.rs` in its place, and naming the engine is an error.

use std::ffi::OsString;

use hd_driver::{EngineTarget, OptLevel};
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

impl TestPrograms {
    pub(crate) fn new(
        kind: EngineKind,
        list: Vec<(String, Vec<u8>, hd_run::Grants)>,
        cwd: &std::path::Path,
    ) -> Result<Self, String> {
        match kind {
            EngineKind::Node => crate::node::Programs::new(&list, cwd).map(Self::Node),
            EngineKind::Wasmtime => wasmtime::Programs::try_from(list).map(Self::Wasmtime),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EngineKind {
    #[default]
    Node,
    Wasmtime,
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

fn parse(name: &str) -> Result<EngineKind, String> {
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

/// The engine of a run with no `--engine` flag: `HD_ENGINE`, else Node.
pub(crate) fn from_env() -> Result<EngineKind, String> {
    match std::env::var("HD_ENGINE") {
        Ok(name) if !name.is_empty() => parse(&name).map_err(|e| format!("HD_ENGINE: {e}")),
        _ => Ok(EngineKind::Node),
    }
}

/// Runs a built program to its end on `kind`, with the process's standard
/// streams, with the Wasm frames of the trap it ended in, innermost first
/// (empty when it ended any other way). `level` selects wasmtime's engine
/// options.
///
/// A run with a cleanup bound (`Limits::cleanup_ms`) hears Ctrl-C
/// (`cli.cancel.ctrl-c`): on wasmtime the handler asks the run to cancel
/// its entry; on Node the program's own process hears the same Ctrl-C from
/// the terminal, so `hd` passes each interrupt on to it
/// (`cli.cancel.forward`) and keeps itself alive until it ends.
pub(crate) fn run(
    kind: EngineKind,
    level: OptLevel,
    wasm: &[u8],
    host: &HostSetup,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    match kind {
        EngineKind::Node => {
            if host.limits.cleanup_ms.is_some() {
                on_ctrl_c(crate::node::forward_interrupt);
            }
            let store = crate::disk_store();
            let node = crate::node::NodeEngine::default();
            let outcome = run_program(&node, wasm, &store, host).map_err(|e| e.to_string())?;
            let frames = node.trace.lock().map(|t| t.clone()).unwrap_or_default();
            Ok((outcome, frames))
        }
        EngineKind::Wasmtime => wasmtime::run_program(level, wasm, host),
    }
}

/// Calls `handler` on each interrupt from now on, in place of ending `hd`:
/// a Ctrl-C, and on Unix a SIGTERM too (`cli.cancel.signals`). One that
/// cannot be installed leaves the default action, and says so.
#[cfg(unix)]
pub(crate) fn on_ctrl_c(mut handler: impl FnMut() + Send + 'static) {
    use signal_hook::consts::{SIGINT, SIGTERM};
    let installed = signal_hook::iterator::Signals::new([SIGINT, SIGTERM])
        .and_then(|mut signals| {
            std::thread::Builder::new()
                .name("interrupt".into())
                .spawn(move || signals.forever().for_each(|_| handler()))
        })
        .map(drop);
    if let Err(e) = installed {
        eprintln!("warning: Ctrl-C cannot cancel the program: {e}");
    }
}

/// As on Unix, for the console's Ctrl-C and Ctrl-Break.
#[cfg(not(unix))]
pub(crate) fn on_ctrl_c(handler: impl FnMut() + Send + 'static) {
    if let Err(e) = ctrlc::set_handler(handler) {
        eprintln!("warning: Ctrl-C cannot cancel the program: {e}");
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

pub(crate) trait SessionBuilder {
    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String>;
    #[cfg(feature = "wasmtime")]
    fn build_joined(&mut self, numbers: &[u32]) -> Result<(Vec<String>, Vec<u8>), String>;
    fn replayed(&mut self, number: u32, joined: bool);
    fn restart(&mut self);
}

pub(crate) struct Rebuilt {
    pub(crate) kept: Vec<u32>,
    pub(crate) skipped: Vec<u32>,
    pub(crate) dropped: Vec<u32>,
    pub(crate) reason: Option<String>,
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
                    providers: Vec::new(),
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

    pub(crate) fn run_input(
        &mut self,
        number: u32,
        source: &str,
        wasm: &[u8],
    ) -> Result<Outcome, String> {
        match self {
            Self::Node(session) => session.input(number, source, wasm),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.run_input(number, source, wasm),
        }
    }

    pub(crate) fn set_journal_cap(&mut self, cap: usize) {
        match self {
            Self::Node(session) => session.cap = cap,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.set_journal_cap(cap),
        }
    }

    pub(crate) fn set_rebuild_after(&mut self, inputs: usize) {
        match self {
            Self::Node(session) => session.rebuild_after = inputs,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.set_rebuild_after(inputs),
        }
    }

    pub(crate) fn wants_rebuild(&self) -> bool {
        match self {
            Self::Node(session) => session.ran > session.rebuild_after,
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.wants_rebuild(),
        }
    }

    pub(crate) fn journal(&self) -> Option<&[u8]> {
        match self {
            Self::Node(session) => session.has_journal.then_some(&[]),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.journal(),
        }
    }

    pub(crate) fn take_journal_notice(&mut self) -> bool {
        match self {
            Self::Node(session) => std::mem::take(&mut session.notice),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => session.take_journal_notice(),
        }
    }

    pub(crate) fn rebuild(&mut self, builder: &mut dyn SessionBuilder) -> Result<Rebuilt, String> {
        match self {
            Self::Node(session) => session.rebuild(builder),
            #[cfg(feature = "wasmtime")]
            Self::Wasmtime(session) => {
                struct NativeBuilder<'a>(&'a mut dyn SessionBuilder);
                impl hd_run_wasmtime::Rebuilder for NativeBuilder<'_> {
                    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String> {
                        self.0.build(number)
                    }
                    fn build_joined(
                        &mut self,
                        numbers: &[u32],
                    ) -> Result<(Vec<String>, Vec<u8>), String> {
                        self.0.build_joined(numbers)
                    }
                    fn replayed(&mut self, number: u32, joined: bool) {
                        self.0.replayed(number, joined);
                    }
                    fn restart(&mut self) {
                        self.0.restart();
                    }
                }
                let result = session.rebuild(&mut NativeBuilder(builder))?;
                Ok(Rebuilt {
                    kept: result.kept,
                    skipped: result.skipped,
                    dropped: result.dropped,
                    reason: result.reason,
                })
            }
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
    ran: usize,
    rebuild_after: usize,
    has_journal: bool,
    notice: bool,
}

impl NodeSession {
    fn new(io: HostIo, cwd: &std::path::Path) -> Result<Self, String> {
        use std::process::{Command, Stdio};
        let scratch = crate::node::Scratch::new(&[])?;
        let mut child = Command::new("node")
            .args(["--input-type=module", "--eval", "import { pathToFileURL } from 'node:url'; const { driveNodeSession } = await import(pathToFileURL(process.argv[1])); await driveNodeSession(); process.exit(0);"])
            .arg(scratch.0.join("node.mjs"))
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
            ran: 0,
            rebuild_after: 1000,
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
            } else if matches!(row, "input" | "all") {
                let mut text = String::new();
                let value = match self.io.input.as_mut() {
                    None if row == "all" => crate::report::quote(""),
                    None => "null".into(),
                    Some(input) if row == "all" => match input.read_to_string(&mut text) {
                        Ok(_) => crate::report::quote(&text.replace("\r\n", "\n")),
                        Err(_) => "false".into(),
                    },
                    Some(input) => match input.read_line(&mut text) {
                        Ok(0) => "null".into(),
                        Ok(_) => {
                            if text.ends_with('\n') {
                                text.pop();
                                if text.ends_with('\r') {
                                    text.pop();
                                }
                            }
                            crate::report::quote(&text)
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

    fn module(&self, number: u32, wasm: &[u8]) -> Result<String, String> {
        let path = self.scratch.0.join(format!("input-{number}.wasm"));
        std::fs::write(&path, wasm).map_err(|error| error.to_string())?;
        Ok(crate::report::quote(&path.to_string_lossy()))
    }

    fn input(&mut self, number: u32, source: &str, wasm: &[u8]) -> Result<Outcome, String> {
        self.open()?;
        let path = self.module(number, wasm)?;
        self.send(&format!(
            "{{\"op\":\"input\",\"number\":{number},\"source\":{},\"wasm\":{path}}}",
            crate::report::quote(source)
        ))?;
        let row = self.receive()?;
        let mut fields = row.split('\t');
        if fields.next() != Some("done") {
            return Err("the Node session sent no input outcome".into());
        }
        let status = fields
            .next()
            .and_then(|s| s.parse::<u8>().ok())
            .ok_or("the Node session sent an invalid status")?;
        self.has_journal = fields.next() == Some("1");
        self.notice = fields.next() == Some("1");
        self.ran += usize::from(number != 0);
        Ok(Outcome::Exit(status))
    }

    fn rebuild(&mut self, builder: &mut dyn SessionBuilder) -> Result<Rebuilt, String> {
        builder.restart();
        self.send("{\"op\":\"rebuild\"}")?;
        loop {
            let row = self.receive()?;
            if let Some(number) = row.strip_prefix("build\t").and_then(|s| s.parse().ok()) {
                let reply = match builder.build(number) {
                    Ok((source, wasm)) => format!(
                        "{{\"source\":{},\"wasm\":{}}}",
                        crate::report::quote(&source),
                        self.module(number, &wasm)?
                    ),
                    Err(error) => format!("{{\"error\":{}}}", crate::report::quote(&error)),
                };
                self.send(&reply)?;
            } else if let Some(fields) = row.strip_prefix("replayed\t") {
                let (number, joined) = fields.split_once('\t').ok_or("invalid replay outcome")?;
                builder.replayed(
                    number.parse().map_err(|_| "invalid replay input")?,
                    joined == "1",
                );
            } else if let Some(fields) = row.strip_prefix("rebuilt\t") {
                let mut fields = fields.splitn(4, '\t');
                let mut numbers = || -> Result<Vec<u32>, String> {
                    fields
                        .next()
                        .ok_or_else(|| "incomplete rebuild outcome".to_owned())?
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.parse().map_err(|_| "invalid rebuilt input".into()))
                        .collect()
                };
                let kept = numbers()?;
                let skipped = numbers()?;
                let dropped = numbers()?;
                let reason = match fields.next() {
                    Some("null") => None,
                    Some(text) => Some(Self::text(text)?),
                    None => return Err("incomplete rebuild reason".into()),
                };
                self.ran = 0;
                return Ok(Rebuilt {
                    kept,
                    skipped,
                    dropped,
                    reason,
                });
            } else {
                return Err("the Node session sent an invalid rebuild frame".into());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::{EngineKind, ProgramLimits, parse_duration, parse_size, time_limit_flag};
    use hd_run::Limits;
    use std::ffi::OsString;

    #[cfg(feature = "wasmtime")]
    #[test]
    fn native_named_children_retain_modules_and_grants_without_node_configuration() {
        use super::TestPrograms;
        let grants = hd_run::Grants {
            limits: vec![("Env".into(), hd_run::Limit::Deny)],
        };
        let list = vec![("tool".to_owned(), vec![0, 97, 115, 109], grants.clone())];
        let programs = TestPrograms::new(
            EngineKind::Wasmtime,
            list,
            std::path::Path::new("unused-native-cwd"),
        )
        .unwrap();
        let TestPrograms::Wasmtime(super::wasmtime::Programs(list)) = programs else {
            panic!("native children must not use Node's scratch configuration");
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "tool");
        assert_eq!(list[0].1, [0, 97, 115, 109]);
        assert_eq!(list[0].2.limits, grants.limits);
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
