//! Native providers for `hd run` on wasmtime, implementing the imports
//! described by `hd_host_abi` and the specification's result contracts.
//!
//! - `hd:rt`: `block()` waits for at least one completion and writes the
//!   completed handles to the exchange buffer; `stderr(len)` writes the
//!   buffer's first `len` bytes to standard error; `abort(h)` drops an
//!   operation.
//! - `hd:prim`: the float text, parsing and case mapping of `prim.rs`.
//! - `hd:Console` `write_line` is pending at once and writes its line when
//!   it completes; `hd:ConsoleInput` `read_line` is pending until a thread
//!   of its own has read the next line of standard input (`reactor.rs`),
//!   so a program's other operations go on while it waits for input.
//! - `hd:Clock`: `now`, `monotonic`, and `sleep`, which completes when its
//!   duration has passed.
//! - `hd:Args` and `hd:Env`; `Env` asks the grant and refuses a variable
//!   outside it as unset, with a notice once per name
//!   (`cli.cap.env.notice`).
//!
//! Standard output is held until input starts, standard error is written,
//! a deadlock is reported, or the run ends.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hd_run::{Grants, Outcome};
use wasmtime::{Caller, FuncType, Linker, Memory, Val, ValType};

use crate::codec::Enc;
use crate::prim;
use crate::profiles::{self, RuntimeProfile};
use crate::reactor::{Line, NS_PER_MS, Reactor, Work, now_ns};
use crate::{core, fs, http, net};

/// Where a run's standard streams go: the process's own, or buffers a
/// caller reads afterwards.
pub struct HostIo {
    pub out: Box<dyn Write + Send>,
    pub err: Box<dyn Write + Send>,
    /// Standard input, or `None` when it is closed (end of input at once).
    pub input: Option<Box<dyn BufRead + Send>>,
}

impl HostIo {
    /// The process's standard output, error and input.
    #[must_use]
    pub fn process() -> HostIo {
        HostIo {
            out: Box::new(std::io::stdout()),
            err: Box::new(std::io::stderr()),
            input: Some(Box::new(std::io::BufReader::new(std::io::stdin()))),
        }
    }
}

/// Why the host stopped the instance, for the driver's panic report.
#[derive(Debug)]
pub enum Stop {
    /// A host result or call that breaks the ABI: the `host-contract` panic.
    Contract(String),
    /// The program waits and nothing can wake it; already reported.
    Deadlock,
    /// The exchange buffer cannot grow: the `heap-exhausted` panic.
    HeapExhausted,
    /// The program's wall-clock deadline has elapsed.
    TimeLimit,
    /// A bug in std or the host.
    Internal(String),
    /// Property exhaustion is successful early termination, not a panic.
    EndCase,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Contract(m) | Stop::Internal(m) => f.write_str(m),
            Stop::Deadlock => f.write_str("deadlock"),
            Stop::HeapExhausted => f.write_str("the exchange buffer cannot grow"),
            Stop::TimeLimit => f.write_str("the program exceeded its time limit"),
            Stop::EndCase => f.write_str("the property has no remaining cases"),
        }
    }
}

impl std::error::Error for Stop {}

/// Existing host control events, distinct from reply and wake delivery.
/// Deadlines become traps through import dispatch or the engine driver;
/// this boundary owns their final flush and bounded provider cleanup.
#[derive(Clone, Copy)]
pub(crate) enum Control {
    Abort(i32),
    Exit { status: i32, close: bool },
    Trap { close: bool },
}

/// Starts the monotonic clock, so it counts from the process's start.
pub fn start_clock() {
    now_ns();
}

/// The per-instance host state of the store.
pub struct Host {
    out_stream: Box<dyn Write + Send>,
    err_stream: Box<dyn Write + Send>,
    /// Standard output not yet written.
    out: Vec<u8>,
    pub memory: Option<Memory>,
    /// Whether a panic report or any standard error was written, so a trap
    /// is not also an internal error.
    pub reported: bool,
    reactor: Reactor,
    args: Vec<String>,
    program: String,
    grants: Grants,
    noticed: HashSet<String>,
    http: http::Client,
    net: net::Client,
    pub(crate) heap: crate::limits::HeapBudget,
    deadline: Option<Instant>,
    fixture: profiles::State,
    pub(crate) testing: Option<crate::testing::State>,
    pub(crate) environment: Option<crate::test_engine::TestEnvironment>,
    base_deadline: Option<Instant>,
    memory_bytes: u64,
    timeout_millis: Option<i64>,
    pub(crate) output_overflow: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(crate) output_budget: Option<(std::sync::Arc<std::sync::atomic::AtomicUsize>, usize)>,
}

impl Host {
    #[must_use]
    pub fn new(
        io: HostIo,
        args: Vec<String>,
        program: String,
        grants: Grants,
        wall_ms: Option<u64>,
        memory_bytes: u64,
    ) -> Host {
        let deadline = wall_ms.and_then(|ms| Instant::now().checked_add(Duration::from_millis(ms)));
        Host {
            out_stream: io.out,
            err_stream: io.err,
            out: Vec::new(),
            memory: None,
            reported: false,
            reactor: Reactor::new(io.input),
            args,
            program,
            grants,
            noticed: HashSet::new(),
            http: http::Client::default(),
            net: net::Client::default(),
            heap: crate::limits::HeapBudget::new(memory_bytes),
            deadline,
            fixture: profiles::State::default(),
            testing: None,
            environment: None,
            base_deadline: deadline,
            memory_bytes,
            timeout_millis: None,
            output_overflow: None,
            output_budget: None,
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn set_deadline(&mut self, deadline: Option<Instant>) {
        self.deadline = deadline;
        self.base_deadline = deadline;
    }

    pub(crate) fn begin_test_body(&mut self) {
        self.deadline = self.base_deadline;
        self.timeout_millis = None;
    }

    pub(crate) fn time_limit_report(&self) -> String {
        match self.timeout_millis {
            Some(millis) => format!(
                "panic: time-limit: the test case ran longer than its timeout of {millis}ms\n"
            ),
            None => "panic: time-limit: the program exceeded its time limit\n".into(),
        }
    }

    fn report_timeout(&mut self, millis: i64) {
        let millis = u64::try_from(millis).unwrap_or(0);
        let reported = Instant::now().checked_add(Duration::from_millis(millis));
        self.timeout_millis = reported
            .filter(|reported| self.base_deadline.is_none_or(|base| *reported <= base))
            .map(|_| i64::try_from(millis).unwrap_or(i64::MAX));
        self.deadline = match (self.base_deadline, reported) {
            (Some(base), Some(reported)) => Some(base.min(reported)),
            (base, reported) => base.or(reported),
        };
    }

    pub fn check_deadline(&self) -> wasmtime::Result<()> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(Stop::TimeLimit.into());
        }
        // Console stdout is held until a flush. Count it before it reaches the
        // capture sink as well, otherwise an endless write-only child could
        // evade the shared stdout/stderr cap.
        if self.output_budget.as_ref().is_some_and(|(count, limit)| {
            count
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_add(self.out.len())
                > *limit
        }) && let Some(flag) = &self.output_overflow
        {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if self
            .output_overflow
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Err(Stop::Internal("process output exceeds the host limit".into()).into());
        }
        Ok(())
    }

    /// The one boundary for operation abort and completed-run cleanup.
    /// Reusable test instances retain providers only when close is false.
    pub(crate) fn control(&mut self, event: Control) -> Option<Outcome> {
        let (status, close) = match event {
            Control::Abort(handle) => {
                self.reactor.remove(handle);
                return None;
            }
            Control::Exit { status, close } => (status, close),
            Control::Trap { close } => (3, close),
        };
        self.flush();
        if close {
            self.close();
        }
        Some(Outcome::Exit(u8::try_from(status & 0xff).unwrap_or(1)))
    }

    /// Releases sockets and HTTP worker guards even for a retained instance.
    fn close(&mut self) {
        self.net.stop();
        self.http = http::Client::default();
    }

    /// Writes the held standard output.
    pub fn flush(&mut self) {
        if !self.out.is_empty() {
            let _ = self.out_stream.write_all(&self.out);
            let _ = self.out_stream.flush();
            self.out.clear();
        }
    }

    /// Writes `text` to standard error, after the held output.
    pub fn err(&mut self, text: &str) {
        self.flush();
        let _ = self.err_stream.write_all(text.as_bytes());
        let _ = self.err_stream.flush();
    }

    /// Whether an operation is pending and not completed: something can
    /// still wake the program.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.reactor.waiting()
    }

    /// Waits for the next completion; `false` when nothing is pending.
    pub fn wait(&self) -> wasmtime::Result<bool> {
        self.check_deadline()?;
        let waiting = self.reactor.wait_until(self.deadline);
        self.check_deadline()?;
        Ok(waiting)
    }
}

/// Collects due handles without writing the guest's exchange buffer.
/// Reactor completion still advances operation state and holds completed
/// Console output; recording/replay of those effects is a separate change.
pub fn complete(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
) -> wasmtime::Result<Vec<u32>> {
    let mut cx = caller.as_context_mut();
    let host = cx.data_mut();
    host.check_deadline()?;
    Ok(host
        .reactor
        .due(&mut host.out)
        .into_iter()
        .map(|handle| u32::from_le_bytes(handle.to_le_bytes()))
        .collect())
}

/// Delivers precisely the supplied batch, shared by driver wake and block.
/// Collection must not preload memory: an embedding may supply a recorded
/// batch without polling live providers, or overwrite the buffer meanwhile.
pub(crate) fn deliver_batch(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
    handles: &[u32],
) -> wasmtime::Result<i32> {
    let (count, bytes) = encode_batch(handles)?;
    if !handles.is_empty() {
        put_bytes(caller, &bytes)?;
    }
    Ok(count)
}
fn encode_batch(handles: &[u32]) -> wasmtime::Result<(i32, Vec<u8>)> {
    let count = i32::try_from(handles.len())
        .map_err(|_| Stop::Contract("the wake batch exceeds the i32 count range".into()))?;
    let bytes = handles
        .iter()
        .flat_map(|handle| handle.to_le_bytes())
        .collect();
    Ok((count, bytes))
}

/// Writes `bytes` at offset 0 of the exchange buffer, growing it to fit.
fn put_bytes(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
    bytes: &[u8],
) -> wasmtime::Result<i64> {
    let mut cx = caller.as_context_mut();
    let Some(memory) = cx.data().memory else {
        return Err(Stop::Contract("the module has no exchange buffer".into()).into());
    };
    let size = memory.data_size(&cx);
    if bytes.len() > size {
        let pages = (bytes.len() - size).div_ceil(65536) as u64;
        if memory.grow(&mut cx, pages).is_err() {
            return Err(Stop::HeapExhausted.into());
        }
    }
    memory.data_mut(&mut cx)[..bytes.len()].copy_from_slice(bytes);
    Ok(i64::try_from(bytes.len()).unwrap_or(i64::MAX))
}

/// The exchange buffer's first `len` bytes.
fn bytes(caller: &mut Caller<'_, Host>, len: i64) -> wasmtime::Result<Vec<u8>> {
    let Some(memory) = caller.data().memory else {
        return Err(Stop::Contract("the module has no exchange buffer".into()).into());
    };
    let data = memory.data(&*caller);
    usize::try_from(len)
        .ok()
        .and_then(|n| data.get(..n))
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            Stop::Contract("an argument length is past the exchange buffer".into()).into()
        })
}

fn text(caller: &mut Caller<'_, Host>, len: i64) -> wasmtime::Result<String> {
    Ok(String::from_utf8_lossy(&bytes(caller, len)?).into_owned())
}

/// An import reply before the one dispatch-side exchange-buffer write.
struct Ret {
    value: Value,
    payload: Option<Vec<u8>>,
}
enum Value {
    Void,
    Int(i64),
    F64(f64),
    Pair(i32, i32),
}
impl Ret {
    fn scalar(value: Value) -> Self {
        Self {
            value,
            payload: None,
        }
    }
    fn void() -> Self {
        Self::scalar(Value::Void)
    }
    fn int(value: i64) -> Self {
        Self::scalar(Value::Int(value))
    }
    fn float(value: f64) -> Self {
        Self::scalar(Value::F64(value))
    }
    fn pair(status: i32, value: i32) -> Self {
        Self::scalar(Value::Pair(status, value))
    }
    fn bytes(bytes: Vec<u8>) -> Self {
        Self {
            value: Value::Int(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
            payload: Some(bytes),
        }
    }
    fn encoded(encoded: Enc) -> Self {
        Self::bytes(encoded.into_bytes())
    }
    fn text(text: &str) -> Self {
        let mut encoded = Enc::default();
        encoded.str(text);
        Self::encoded(encoded)
    }
    fn byte_list(bytes: &[u8]) -> Self {
        let mut encoded = Enc::default();
        encoded.bytes(bytes);
        Self::encoded(encoded)
    }
    fn ready(encoded: Enc) -> wasmtime::Result<Self> {
        let bytes = encoded.into_bytes();
        let length = result_len(i64::try_from(bytes.len()).unwrap_or(i64::MAX))?;
        Ok(Self {
            value: Value::Pair(0, length),
            payload: Some(bytes),
        })
    }
    fn batch(handles: &[u32]) -> wasmtime::Result<Self> {
        let (count, bytes) = encode_batch(handles)?;
        Ok(Self {
            value: Value::Int(i64::from(count)),
            payload: (!handles.is_empty()).then_some(bytes),
        })
    }
}

/// The imports that run on wasmtime.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Test(TestMethod),
    Fixture(profiles::Method),
    Fs {
        key: &'static str,
        method: &'static str,
        finish: bool,
    },
    ProcessStart,
    HttpStart,
    Net {
        method: &'static str,
        input: Option<usize>,
        close: bool,
    },
    NeverFinish,
    RandomNext,
    RandomFill,
    Sys(&'static str),
    Block,
    Abort,
    Stderr,
    FormatF64,
    FormatF32,
    FormatFixed,
    Lower,
    Upper,
    ParseF64,
    WriteLineStart,
    WriteLineFinish,
    ReadLineStart,
    ReadLineFinish,
    Now,
    Monotonic,
    SleepStart,
    SleepFinish,
    Program,
    ArgsList,
    EnvGet,
    EnvNames,
}

#[derive(Clone, Copy, Debug)]
enum TestMethod {
    Row,
    Timeout,
    Snapshot,
    TempDir,
    Start,
    Record,
    Show,
}

impl Kind {
    /// Reject malformed result shapes before executing any provider effects.
    /// Integer width compatibility is retained; floats/references are never
    /// silently filled with integer values.
    fn validate_results(self, results: &[ValType]) -> wasmtime::Result<()> {
        let integer = |ty: &ValType| matches!(ty, ValType::I32 | ValType::I64);
        let valid = match self {
            Self::Test(TestMethod::Timeout | TestMethod::Record | TestMethod::Show)
            | Self::Abort
            | Self::Stderr => results.is_empty(),
            Self::ParseF64 => matches!(results, [ValType::F64]),
            Self::Fs { finish: false, .. }
            | Self::ProcessStart
            | Self::HttpStart
            | Self::Net { close: false, .. }
            | Self::WriteLineStart
            | Self::ReadLineStart
            | Self::SleepStart => match results {
                [status, value] => integer(status) && integer(value),
                _ => false,
            },
            _ => match results {
                [value] => integer(value),
                _ => false,
            },
        };
        if valid {
            Ok(())
        } else {
            Err(Stop::Contract(format!("the import {self:?} has an unexpected signature")).into())
        }
    }
    fn with_test(module: &str, name: &str, kind: crate::test_engine::TestHostKind) -> Option<Self> {
        let test = match (module, name) {
            ("hd:TestRunner", "row") => Some(TestMethod::Row),
            ("hd:TestRunner", "report_timeout") => Some(TestMethod::Timeout),
            ("hd:TestRunner", "snapshot_check") => Some(TestMethod::Snapshot),
            ("hd:TestRunner", "temp_dir") => Some(TestMethod::TempDir),
            ("hd:PropertyRunner", "start") => Some(TestMethod::Start),
            ("hd:PropertyRunner", "record") => Some(TestMethod::Record),
            ("hd:PropertyRunner", "show") => Some(TestMethod::Show),
            _ => None,
        };
        if let Some(method) = test {
            return Some(Self::Test(method));
        }
        if kind == crate::test_engine::TestHostKind::Unit && !matches!(module, "hd:rt" | "hd:prim")
        {
            return None;
        }
        Self::with_profile(module, name, kind.profile())
    }
    fn with_profile(module: &str, name: &str, profile: RuntimeProfile) -> Option<Self> {
        Self::of(module, name).or_else(|| profile.method(module, name).map(Self::Fixture))
    }

    fn of(module: &str, name: &str) -> Option<Kind> {
        if module == "hd:Net" {
            for &method in net::CLOSE {
                if name == method {
                    return Some(Kind::Net {
                        method,
                        input: None,
                        close: true,
                    });
                }
            }
            let (method, suffix) = name.rsplit_once('.')?;
            let method = *net::METHODS.iter().find(|m| **m == method)?;
            let input = match method {
                "lookup" => Some(0),
                "connect" | "listen" | "bind_udp" | "TcpStream.write" => Some(1),
                "UdpSocket.send_to" => Some(2),
                _ => None,
            };
            return match suffix {
                "start" => Some(Kind::Net {
                    method,
                    input,
                    close: false,
                }),
                "finish" => Some(Kind::NeverFinish),
                _ => None,
            };
        }
        if let Some((key, methods)) = match module {
            "hd:FsRead" => Some(("FsRead", fs::READ)),
            "hd:FsWrite" => Some(("FsWrite", fs::WRITE)),
            _ => None,
        } {
            let (method, suffix) = name.rsplit_once('.')?;
            let method = *methods.iter().find(|m| **m == method)?;
            return match suffix {
                "start" => Some(Kind::Fs {
                    key,
                    method,
                    finish: false,
                }),
                "finish" => Some(Kind::Fs {
                    key,
                    method,
                    finish: true,
                }),
                _ => None,
            };
        }
        Some(match (module, name) {
            ("hd:Http", "send.start") => Kind::HttpStart,
            ("hd:Http", "send.finish") => Kind::NeverFinish,
            ("hd:Process", "run.start") => Kind::ProcessStart,
            ("hd:Process", "run.finish") => Kind::NeverFinish,
            ("hd:Random", "next_u64") => Kind::RandomNext,
            ("hd:Random", "fill") => Kind::RandomFill,
            ("hd:Sys", "os") => Kind::Sys("os"),
            ("hd:Sys", "arch") => Kind::Sys("arch"),
            ("hd:Sys", "hostname") => Kind::Sys("hostname"),
            ("hd:Sys", "cpu_count") => Kind::Sys("cpu_count"),
            ("hd:rt", "block") => Kind::Block,
            ("hd:rt", "abort") => Kind::Abort,
            ("hd:rt", "stderr") => Kind::Stderr,
            ("hd:prim", "format_f64") => Kind::FormatF64,
            ("hd:prim", "format_f32") => Kind::FormatF32,
            ("hd:prim", "format_f64_fixed") => Kind::FormatFixed,
            ("hd:prim", "string_lower") => Kind::Lower,
            ("hd:prim", "string_upper") => Kind::Upper,
            ("hd:prim", "parse_f64") => Kind::ParseF64,
            ("hd:Console", "write_line.start") => Kind::WriteLineStart,
            ("hd:Console", "write_line.finish") => Kind::WriteLineFinish,
            ("hd:ConsoleInput", "read_line.start") => Kind::ReadLineStart,
            ("hd:ConsoleInput", "read_line.finish") => Kind::ReadLineFinish,
            ("hd:Clock", "now") => Kind::Now,
            ("hd:Clock", "monotonic") => Kind::Monotonic,
            ("hd:Clock", "sleep.start") => Kind::SleepStart,
            ("hd:Clock", "sleep.finish") => Kind::SleepFinish,
            ("hd:Args", "program") => Kind::Program,
            ("hd:Args", "list") => Kind::ArgsList,
            ("hd:Env", "get") => Kind::EnvGet,
            ("hd:Env", "names") => Kind::EnvNames,
            _ => return None,
        })
    }
}

/// Whether an import runs on wasmtime.
#[must_use]
pub fn supported(module: &str, name: &str, profile: RuntimeProfile) -> bool {
    Kind::with_profile(module, name, profile).is_some()
}

fn int_arg(params: &[Val], i: usize) -> wasmtime::Result<i64> {
    match params.get(i) {
        Some(Val::I32(v)) => Ok(i64::from(*v)),
        Some(Val::I64(v)) => Ok(*v),
        _ => Err(Stop::Contract(format!("argument {i} is not an integer")).into()),
    }
}

fn float_arg(params: &[Val], i: usize) -> wasmtime::Result<f64> {
    match params.get(i) {
        Some(Val::F64(bits)) => Ok(f64::from_bits(*bits)),
        Some(Val::F32(bits)) => Ok(f64::from(f32::from_bits(*bits))),
        _ => Err(Stop::Contract(format!("argument {i} is not a float")).into()),
    }
}

/// The value of result type `ty` for the integer `v`.
fn int_val(ty: &ValType, v: i64) -> Val {
    match ty {
        ValType::I64 => Val::I64(v),
        // A narrower result keeps its low bits, as a JS number crossing to
        // an `i32` does.
        _ => Val::I32(i32::from_le_bytes(
            v.to_le_bytes()[..4].try_into().unwrap_or([0; 4]),
        )),
    }
}

fn call(kind: Kind, caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Ret> {
    caller.data().check_deadline()?;
    let result = match kind {
        Kind::Test(method) => {
            let state = caller
                .data()
                .testing
                .clone()
                .ok_or_else(|| Stop::Contract("no test runner binds this run".into()))?;
            state.set_io_deadline(caller.data().deadline());
            match method {
                TestMethod::Row => Ret::int(i64::from(
                    state.row(
                        u32::try_from(int_arg(params, 0)?)
                            .map_err(|_| Stop::Contract("invalid row count".into()))?,
                    ),
                )),
                TestMethod::Timeout => {
                    let millis = int_arg(params, 0)?;
                    state.report_timeout(millis);
                    caller.data_mut().report_timeout(millis);
                    state.set_io_deadline(caller.data().deadline());
                    wasmtime::AsContextMut::as_context_mut(caller).set_epoch_deadline(1);
                    Ret::void()
                }
                TestMethod::Snapshot | TestMethod::Show => {
                    let value =
                        String::from_utf8_lossy(&bytes(caller, int_arg(params, 0)?)?).into_owned();
                    if matches!(method, TestMethod::Show) {
                        state.show(&value);
                        Ret::void()
                    } else {
                        let message = state.snapshot_check(&value);
                        caller.data().check_deadline()?;
                        let message = message.map_err(Stop::Internal)?;
                        let mut encoded = Enc::default();
                        encoded.str(&message);
                        Ret::encoded(encoded)
                    }
                }
                TestMethod::TempDir => {
                    let path = state.temp_dir().map_err(Stop::Internal)?;
                    let mut encoded = Enc::default();
                    encoded.str(&path);
                    Ret::encoded(encoded)
                }
                TestMethod::Start => {
                    let mut counts = [0; 3];
                    for (index, count) in counts.iter_mut().enumerate() {
                        *count = u32::try_from(int_arg(params, index)?)
                            .map_err(|_| Stop::Contract("invalid property count".into()))?;
                    }
                    let encoded = state.start(counts[0], counts[1], counts[2]);
                    caller.data().check_deadline()?;
                    let encoded = encoded.map_err(Stop::Internal)?.ok_or(Stop::EndCase)?;
                    Ret::encoded(encoded)
                }
                TestMethod::Record => {
                    state.record(int_arg(params, 0)?);
                    Ret::void()
                }
            }
        }
        Kind::Fixture(method) => {
            let length = if matches!(method, profiles::Method::VaultKeep) {
                int_arg(params, 0)?
            } else {
                0
            };
            match caller.data_mut().fixture.call(method, length) {
                profiles::Reply::Encoded(encoded) => Ret::encoded(encoded),
                profiles::Reply::EchoLength(length) => Ret::int(length),
            }
        }
        Kind::Fs { finish: true, .. } | Kind::NeverFinish => {
            return Err(
                Stop::Contract("a finish of an operation that is not pending".into()).into(),
            );
        }
        Kind::Fs {
            key,
            method,
            finish: false,
        } => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data();
            let e = match &host.environment {
                Some(environment) => fs::call_until_at(
                    key,
                    method,
                    &b,
                    &host.grants,
                    host.deadline(),
                    &environment.cwd,
                )?,
                None => match host.deadline() {
                    Some(deadline) => {
                        fs::call_until(key, method, &b, &host.grants, Some(deadline))?
                    }
                    None => fs::call(key, method, &b, &host.grants)?,
                },
            };
            Ret::ready(e)?
        }
        Kind::ProcessStart => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data();
            let e = match &host.environment {
                Some(environment) if host.testing.is_some() => crate::test_engine::process(
                    &b,
                    &host.grants,
                    environment,
                    host.deadline(),
                    host.memory_bytes,
                )?,
                Some(environment) => core::process_with_deadline_at(
                    &b,
                    &host.grants,
                    host.deadline(),
                    &environment.cwd,
                )?,
                None => core::process_with_deadline(&b, &host.grants, host.deadline())?,
            };
            Ret::ready(e)?
        }
        Kind::HttpStart => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            let e = host.http.send_until(&b, &host.grants, host.deadline)?;
            Ret::ready(e)?
        }
        Kind::Net {
            method,
            input,
            close,
        } => {
            let args = (0..params.len())
                .map(|i| int_arg(params, i))
                .collect::<wasmtime::Result<Vec<_>>>()?;
            let b = match input {
                Some(i) => bytes(caller, int_arg(params, i)?)?,
                None => Vec::new(),
            };
            let host = caller.data_mut();
            let e = host
                .net
                .call_until(method, &args, &b, &host.grants, host.deadline)?;
            if close {
                Ret::encoded(e)
            } else {
                Ret::ready(e)?
            }
        }
        Kind::RandomNext => {
            let b: [u8; 8] = core::random(8)?
                .try_into()
                .map_err(|_| Stop::Internal("random byte count".into()))?;
            Ret::int(i64::from_le_bytes(b))
        }
        Kind::RandomFill => {
            // The ABI count is an unsigned 32-bit value.
            let b = core::random(u32::from_le_bytes(
                int_arg(params, 0)?.to_le_bytes()[..4]
                    .try_into()
                    .unwrap_or([0; 4]),
            ) as usize)?;
            Ret::byte_list(&b)
        }
        Kind::Sys(name) => {
            let e = core::sys(name, &caller.data().grants);
            Ret::encoded(e)
        }
        Kind::Stderr => {
            let t = text(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            host.reported = true;
            host.err(&t);
            Ret::void()
        }
        Kind::Block => loop {
            let handles = complete(caller)?;
            if !handles.is_empty() {
                return Ret::batch(&handles);
            }
            if !caller.data().wait()? {
                return Err(deadlock(caller.data_mut()));
            }
        },
        Kind::Abort => {
            let h = int_arg(params, 0)?;
            caller.data_mut().control(Control::Abort(handle(h)));
            Ret::void()
        }
        Kind::FormatF64 => {
            let s = prim::shortest_f64(float_arg(params, 0)?);
            Ret::text(&s)
        }
        Kind::FormatF32 => {
            let s = match params.first() {
                Some(Val::F32(bits)) => prim::shortest_f32(f32::from_bits(*bits)),
                _ => return Err(Stop::Contract("format_f32 takes an f32".into()).into()),
            };
            Ret::text(&s)
        }
        Kind::FormatFixed => {
            let value = float_arg(params, 0)?;
            let places = u32::try_from(int_arg(params, 1)?)
                .map_err(|_| Stop::Internal("format_f64_fixed: digits out of range".into()))?;
            let s = prim::fixed(value, places);
            Ret::text(&s)
        }
        Kind::Lower | Kind::Upper => {
            let t = text(caller, int_arg(params, 0)?)?;
            let s = if matches!(kind, Kind::Lower) {
                prim::lower(&t)
            } else {
                prim::upper(&t)
            };
            Ret::text(&s)
        }
        Kind::ParseF64 => {
            let t = text(caller, int_arg(params, 0)?)?;
            match prim::parse(&t) {
                Some(v) => Ret::float(v),
                None => {
                    return Err(Stop::Internal(format!(
                        "parse_f64: text outside the grammar: {t}"
                    ))
                    .into());
                }
            }
        }
        Kind::WriteLineStart => {
            let line = bytes(caller, int_arg(params, 0)?)?;
            // Normalize invalid UTF-8 through the text boundary.
            let line = String::from_utf8_lossy(&line).into_owned().into_bytes();
            Ret::pair(1, caller.data_mut().reactor.write_line(line))
        }
        Kind::WriteLineFinish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            // `.Ok(())`.
            Ret::bytes(vec![0])
        }
        // Make a completed prompt visible before asking the reader thread
        // for input. Pending until that thread delivers the line.
        Kind::ReadLineStart => {
            let host = caller.data_mut();
            host.flush();
            Ret::pair(1, host.reactor.read_line())
        }
        // `Result[string?, ConsoleError]`: the line, `.None` at the end of
        // input, `.Err(.Closed)` when it cannot be read.
        Kind::ReadLineFinish => {
            let h = handle(int_arg(params, 0)?);
            let Some(Work::Read(Some(line))) =
                caller.data_mut().reactor.remove(h).map(|op| op.work)
            else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            Ret::encoded(encode_line(&line))
        }
        Kind::Now => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
            Ret::int(ms)
        }
        Kind::Monotonic => Ret::int(i64::try_from(now_ns() / NS_PER_MS).unwrap_or(i64::MAX)),
        Kind::SleepStart => {
            let ms = int_arg(params, 0)?;
            Ret::pair(1, caller.data_mut().reactor.sleep(ms))
        }
        Kind::SleepFinish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::int(0)
        }
        Kind::Program => {
            let p = caller.data().program.clone();
            Ret::text(&p)
        }
        Kind::ArgsList => {
            let args = caller.data().args.clone();
            let mut e = Enc::default();
            e.list(&args, |e, a| {
                e.str(a);
            });
            Ret::encoded(e)
        }
        Kind::EnvGet => {
            let name = text(caller, int_arg(params, 0)?)?;
            let value = env_var(&name);
            let host = caller.data_mut();
            let mut e = Enc::default();
            if !host.grants.covers_name("Env", &name) {
                if value.is_some() && host.noticed.insert(name.clone()) {
                    host.err(&format!("{}\n", hd_run::env_notice(&name)));
                }
                e.leb(0);
            } else if let Some(v) = value {
                e.leb(1).str(&v);
            } else {
                e.leb(0);
            }
            Ret::encoded(e)
        }
        Kind::EnvNames => {
            let names: Vec<String> = std::env::vars_os()
                .map(|(k, _)| k.to_string_lossy().into_owned())
                .filter(|n| caller.data().grants.covers_name("Env", n))
                .collect();
            let mut e = Enc::default();
            e.list(&names, |e, n| {
                e.str(n);
            });
            Ret::encoded(e)
        }
    };
    caller.data().check_deadline()?;
    Ok(result)
}

fn result_len(len: i64) -> wasmtime::Result<i32> {
    i32::try_from(len).map_err(|_| Stop::HeapExhausted.into())
}

/// A handle argument; one that no `i32` holds names no operation.
fn handle(h: i64) -> i32 {
    i32::try_from(h).unwrap_or(-1)
}

fn encode_line(line: &Line) -> Enc {
    let mut e = Enc::default();
    match line {
        Err(()) => e.leb(1).leb(0),
        Ok(None) => e.leb(0).leb(0),
        Ok(Some(text)) => e.leb(0).leb(1).str(text),
    };
    e
}

/// A variable's value; a name the environment cannot hold is unset.
fn env_var(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return None;
    }
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// Reports the deadlock and stops the instance.
pub fn deadlock(host: &mut Host) -> wasmtime::Error {
    host.err("panic: deadlock: the program waits, but no host operation is pending\n");
    host.reported = true;
    Stop::Deadlock.into()
}

/// The capability key of an import module, `hd:<Key>`.
#[must_use]
pub fn capability(module: &str) -> Option<&str> {
    module.strip_prefix("hd:")
}

/// Defines each import of `module` in `linker`, or names the first one
/// that does not run on wasmtime yet.
pub fn define(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    profile: RuntimeProfile,
) -> Result<(), String> {
    define_with(linker, module, |module, name| {
        Kind::with_profile(module, name, profile)
    })
}

pub(crate) fn define_test(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    kind: crate::test_engine::TestHostKind,
) -> Result<(), String> {
    define_with(linker, module, |module, name| {
        Kind::with_test(module, name, kind)
    })
}

fn define_with(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    kind_of: impl Fn(&str, &str) -> Option<Kind>,
) -> Result<(), String> {
    let mut defined = HashSet::new();
    for import in module.imports() {
        let (m, n) = (import.module(), import.name());
        // Wasm permits repeated imports. Node reuses the same property;
        // the linker likewise needs only one definition for that name.
        if !defined.insert((m.to_owned(), n.to_owned())) {
            continue;
        }
        let Some(kind) = kind_of(m, n) else {
            return Err(unsupported_message(m, n));
        };
        let Some(ty) = import.ty().func().cloned() else {
            return Err(format!("the import `{m}` `{n}` is not a function"));
        };
        let results: Vec<ValType> = ty.results().collect();
        linker
            .func_new(
                m,
                n,
                FuncType::clone(&ty),
                move |mut caller, params, out| {
                    kind.validate_results(&results)?;
                    let Ret { value, payload } = call(kind, &mut caller, params)?;
                    match (value, out) {
                        (Value::Void, []) => {}
                        (Value::Int(v), [one]) => *one = int_val(&results[0], v),
                        (Value::F64(v), [one]) => *one = Val::F64(v.to_bits()),
                        (Value::Pair(s, v), [a, b]) => {
                            *a = int_val(&results[0], i64::from(s));
                            *b = int_val(&results[1], i64::from(v));
                        }
                        _ => {
                            return Err(Stop::Contract(format!(
                                "the import {kind:?} has an unexpected signature"
                            ))
                            .into());
                        }
                    }
                    if let Some(bytes) = payload {
                        put_bytes(&mut caller, &bytes)?;
                    }
                    Ok(())
                },
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The error of an import that does not run on wasmtime yet.
#[must_use]
pub fn unsupported_message(module: &str, name: &str) -> String {
    match capability(module).filter(|k| {
        hd_host_abi::TABLE
            .iter()
            .chain(hd_host_abi::RUNNERS)
            .any(|t| t.key == *k)
    }) {
        Some(key) => format!(
            "the program uses `{key}` (`{module}` `{name}`), which is not yet on wasmtime; run it on Node, without `--engine wasmtime`"
        ),
        None => format!("the import `{module}` `{name}` is not yet on wasmtime"),
    }
}
