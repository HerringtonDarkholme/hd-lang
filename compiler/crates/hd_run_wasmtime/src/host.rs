//! The host of `hd run` on wasmtime: the import functions of the ABI in
//! `hd_host_abi` that run here, with the behavior of the Node host
//! (`compiler/host/core.mjs`) they replace, so a program writes the same
//! bytes and ends with the same status on either engine.
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
//! Standard output is held until standard error is written, a deadlock is
//! reported, or the run ends, as the Node host holds it.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use hd_run::Grants;
use wasmtime::{Caller, FuncType, Linker, Memory, Val, ValType};

use crate::codec::Enc;
use crate::prim;
use crate::reactor::{Line, NS_PER_MS, Reactor, Work, now_ns};

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

/// Why the host stopped the instance: each is a trap that the driver
/// reports as Node's host does.
#[derive(Debug)]
pub enum Stop {
    /// A host result or call that breaks the ABI: the `host-contract` panic.
    Contract(String),
    /// The program waits and nothing can wake it; already reported.
    Deadlock,
    /// The exchange buffer cannot grow: the `heap-exhausted` panic.
    HeapExhausted,
    /// A bug in std or the host (Node throws a JS error here).
    Internal(String),
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Contract(m) | Stop::Internal(m) => f.write_str(m),
            Stop::Deadlock => f.write_str("deadlock"),
            Stop::HeapExhausted => f.write_str("the exchange buffer cannot grow"),
        }
    }
}

impl std::error::Error for Stop {}

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
}

impl Host {
    #[must_use]
    pub fn new(io: HostIo, args: Vec<String>, program: String, grants: Grants) -> Host {
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
        }
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
    #[must_use]
    pub fn wait(&self) -> bool {
        self.reactor.wait()
    }
}

/// Completes the due operations, writing their handles to the exchange
/// buffer as little-endian `i32`s, and returns them.
pub fn complete(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
) -> wasmtime::Result<Vec<i32>> {
    let mut cx = caller.as_context_mut();
    let host = cx.data_mut();
    let handles = host.reactor.due(&mut host.out);
    if !handles.is_empty() {
        let bytes: Vec<u8> = handles.iter().flat_map(|h| h.to_le_bytes()).collect();
        put_bytes(caller, &bytes)?;
    }
    Ok(handles)
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

fn put(caller: &mut Caller<'_, Host>, e: &Enc) -> wasmtime::Result<i64> {
    put_bytes(caller, e.done())
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

/// What an import returns, before it is matched to the import's result
/// types.
enum Ret {
    Void,
    Int(i64),
    F64(f64),
    /// A `.start`'s status and value.
    Pair(i32, i32),
}

/// The imports that run on wasmtime.
#[derive(Clone, Copy, Debug)]
enum Kind {
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

impl Kind {
    fn of(module: &str, name: &str) -> Option<Kind> {
        Some(match (module, name) {
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
pub fn supported(module: &str, name: &str) -> bool {
    Kind::of(module, name).is_some()
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
    Ok(match kind {
        Kind::Stderr => {
            let t = text(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            host.reported = true;
            host.err(&t);
            Ret::Void
        }
        Kind::Block => loop {
            let n = complete(caller)?.len();
            if n > 0 {
                return Ok(Ret::Int(i64::try_from(n).unwrap_or(i64::MAX)));
            }
            if !caller.data().wait() {
                return Err(deadlock(caller.data_mut()));
            }
        },
        Kind::Abort => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::Void
        }
        Kind::FormatF64 => {
            let s = prim::shortest_f64(float_arg(params, 0)?);
            Ret::Int(put(caller, Enc::default().str(&s))?)
        }
        Kind::FormatF32 => {
            let s = match params.first() {
                Some(Val::F32(bits)) => prim::shortest_f32(f32::from_bits(*bits)),
                _ => return Err(Stop::Contract("format_f32 takes an f32".into()).into()),
            };
            Ret::Int(put(caller, Enc::default().str(&s))?)
        }
        Kind::FormatFixed => {
            let value = float_arg(params, 0)?;
            let places = u32::try_from(int_arg(params, 1)?)
                .map_err(|_| Stop::Internal("format_f64_fixed: digits out of range".into()))?;
            let s = prim::fixed(value, places);
            Ret::Int(put(caller, Enc::default().str(&s))?)
        }
        Kind::Lower | Kind::Upper => {
            let t = text(caller, int_arg(params, 0)?)?;
            let s = if matches!(kind, Kind::Lower) {
                prim::lower(&t)
            } else {
                prim::upper(&t)
            };
            Ret::Int(put(caller, Enc::default().str(&s))?)
        }
        Kind::ParseF64 => {
            let t = text(caller, int_arg(params, 0)?)?;
            match prim::parse(&t) {
                Some(v) => Ret::F64(v),
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
            // Decoded and encoded again, as the Node host's string is.
            let line = String::from_utf8_lossy(&line).into_owned().into_bytes();
            Ret::Pair(1, caller.data_mut().reactor.write_line(line))
        }
        Kind::WriteLineFinish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            // `.Ok(())`.
            Ret::Int(put_bytes(caller, &[0])?)
        }
        // Pending until the reader thread delivers the line.
        Kind::ReadLineStart => Ret::Pair(1, caller.data_mut().reactor.read_line()),
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
            Ret::Int(put(caller, &encode_line(&line))?)
        }
        Kind::Now => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
            Ret::Int(ms)
        }
        Kind::Monotonic => Ret::Int(i64::try_from(now_ns() / NS_PER_MS).unwrap_or(i64::MAX)),
        Kind::SleepStart => {
            let ms = int_arg(params, 0)?;
            Ret::Pair(1, caller.data_mut().reactor.sleep(ms))
        }
        Kind::SleepFinish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::Int(0)
        }
        Kind::Program => {
            let p = caller.data().program.clone();
            Ret::Int(put(caller, Enc::default().str(&p))?)
        }
        Kind::ArgsList => {
            let args = caller.data().args.clone();
            let mut e = Enc::default();
            e.list(&args, |e, a| {
                e.str(a);
            });
            Ret::Int(put(caller, &e)?)
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
            Ret::Int(put(caller, &e)?)
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
            Ret::Int(put(caller, &e)?)
        }
    })
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

/// A variable's value, as Node's `process.env` reads it: a name the
/// environment cannot hold is unset.
fn env_var(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return None;
    }
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// Reports the deadlock, as the Node host does, and stops the instance.
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
pub fn define(linker: &mut Linker<Host>, module: &wasmtime::Module) -> Result<(), String> {
    for import in module.imports() {
        let (m, n) = (import.module(), import.name());
        let Some(kind) = Kind::of(m, n) else {
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
                    let ret = call(kind, &mut caller, params)?;
                    match (ret, out) {
                        (Ret::Void, []) => {}
                        (Ret::Int(v), [one]) => *one = int_val(&results[0], v),
                        (Ret::F64(v), [one]) => *one = Val::F64(v.to_bits()),
                        (Ret::Pair(s, v), [a, b]) => {
                            *a = int_val(&results[0], i64::from(s));
                            *b = int_val(&results[1], i64::from(v));
                        }
                        _ => {
                            return Err(Stop::Contract(format!(
                                "the import `{kind:?}` has an unexpected signature"
                            ))
                            .into());
                        }
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
