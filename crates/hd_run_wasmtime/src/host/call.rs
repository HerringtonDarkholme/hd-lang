use std::time::{SystemTime, UNIX_EPOCH};

use wasmtime::{Caller, Val};

use hd_host_abi::Wait;

use super::kind::Kind;
use super::link::deadlock;
use super::reply::{Ret, bytes, complete, float_arg, handle, int_arg, text};
use super::{Control, Host, Stop};
use crate::abi::{ConsoleError, HostMethod, Id};
use crate::prim;
use crate::profiles;
use crate::reactor::{Done, NS_PER_MS, Work, now_ns};
use crate::{core, fs, http, net};

pub(crate) fn call(
    kind: Kind,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    caller.data().check_deadline()?;
    let result = match kind {
        Kind::Host { method, finish } => host(method, finish, caller, params)?,
        Kind::Fixture {
            method,
            finish: true,
        } => {
            let h = handle(int_arg(params, 0)?);
            let Some(op) = caller.data_mut().reactor.remove(h) else {
                return Err(
                    Stop::Contract("a finish of an operation that is not pending".into()).into(),
                );
            };
            let bytes = match (method, op.work) {
                (profiles::Method::FilesOpen, Work::FixtureOpen) => {
                    let id = caller.data_mut().fixture.open_file();
                    let mut encoded = crate::codec::Enc::default();
                    encoded.leb(u64::try_from(id).unwrap_or(u64::MAX));
                    encoded.into_bytes()
                }
                (profiles::Method::FileHandleRead, Work::FixtureRead { file }) => caller
                    .data_mut()
                    .fixture
                    .read_file(file)
                    .map_err(Stop::Contract)?,
                _ => {
                    return Err(Stop::Contract(
                        "a finish of an operation that is not completed".into(),
                    )
                    .into());
                }
            };
            Ret::bytes(bytes)
        }
        Kind::Fixture {
            method,
            finish: false,
        } => match method {
            profiles::Method::FilesOpen => Ret::pair(1, caller.data_mut().reactor.fixture_open()),
            profiles::Method::FileHandleRead => {
                let file = handle(int_arg(params, 0)?);
                Ret::pair(1, caller.data_mut().reactor.fixture_read(file))
            }
            profiles::Method::FileHandleClose => {
                let file = handle(int_arg(params, 0)?);
                Ret::bytes(
                    caller
                        .data_mut()
                        .fixture
                        .close_file(file)
                        .map_err(Stop::Contract)?,
                )
            }
            profiles::Method::GateWait => Ret::pair(1, caller.data_mut().reactor.gate()),
            profiles::Method::GaugeLevel
            | profiles::Method::SensorReading
            | profiles::Method::VaultKeep => {
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
        },
        Kind::Stderr => {
            let t = text(caller, int_arg(params, 0)?)?;
            caller.data_mut().guest_err(&t);
            Ret::void()
        }
        Kind::Panic => {
            caller.data_mut().begin_panic();
            Ret::void()
        }
        // Held with `Console`'s output, so a later `stderr` write puts it
        // first.
        Kind::Stdout => {
            let t = text(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            host.out.extend_from_slice(t.as_bytes());
            host.flush_over_bound();
            Ret::void()
        }
        Kind::Block => loop {
            let handles = complete(caller)?;
            if !handles.is_empty() {
                return Ret::batch(&handles);
            }
            if !caller.data_mut().wait()? {
                return Err(deadlock(caller.data_mut()));
            }
        },
        Kind::Abort => {
            let h = int_arg(params, 0)?;
            caller.data_mut().control(Control::Abort(handle(h)));
            Ret::void()
        }
        Kind::FormatF64 => Ret::value(&prim::shortest_f64(float_arg(params, 0)?)),
        Kind::FormatF32 => {
            let s = match params.first() {
                Some(Val::F32(bits)) => prim::shortest_f32(f32::from_bits(*bits)),
                _ => return Err(Stop::Contract("format_f32 takes an f32".into()).into()),
            };
            Ret::value(&s)
        }
        Kind::FormatFixed => {
            let value = float_arg(params, 0)?;
            let places = u32::try_from(int_arg(params, 1)?)
                .map_err(|_| Stop::Internal("format_f64_fixed: digits out of range".into()))?;
            let s = prim::fixed(value, places);
            Ret::value(&s)
        }
        Kind::Lower | Kind::Upper => {
            let t = text(caller, int_arg(params, 0)?)?;
            let s = if matches!(kind, Kind::Lower) {
                prim::lower(&t)
            } else {
                prim::upper(&t)
            };
            Ret::value(&s)
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
    };
    caller.data().check_deadline()?;
    Ok(result)
}

/// A host method of the ABI model: its one import, or its `.start`, or its
/// `.finish` when `finish`.
fn host(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match method.id {
        Id::TestRunnerRow => {
            let state = runner(caller)?;
            Ret::int(i64::from(
                state.row(
                    u32::try_from(int_arg(params, 0)?)
                        .map_err(|_| Stop::Contract("invalid row count".into()))?,
                ),
            ))
        }
        Id::TestRunnerReportTimeout => {
            let state = runner(caller)?;
            let millis = int_arg(params, 0)?;
            state.report_timeout(millis);
            caller.data_mut().report_timeout(millis);
            state.set_io_deadline(caller.data().deadline());
            wasmtime::AsContextMut::as_context_mut(caller).set_epoch_deadline(1);
            Ret::void()
        }
        Id::TestRunnerSnapshotCheck => {
            let state = runner(caller)?;
            let value = text(caller, int_arg(params, 0)?)?;
            let message = state.snapshot_check(&value);
            caller.data().check_deadline()?;
            Ret::value(&message.map_err(Stop::Internal)?)
        }
        Id::TestRunnerTempDir => {
            let state = runner(caller)?;
            Ret::value(&state.temp_dir().map_err(Stop::Internal)?)
        }
        Id::PropertyRunnerStart => {
            let state = runner(caller)?;
            let mut counts = [0; 3];
            for (index, count) in counts.iter_mut().enumerate() {
                *count = u32::try_from(int_arg(params, index)?)
                    .map_err(|_| Stop::Contract("invalid property count".into()))?;
            }
            let case = state.start(counts[0], counts[1], counts[2]);
            caller.data().check_deadline()?;
            Ret::value(&case.map_err(Stop::Internal)?.ok_or(Stop::EndCase)?)
        }
        Id::PropertyRunnerRecord => {
            let state = runner(caller)?;
            state.record(int_arg(params, 0)?);
            Ret::void()
        }
        Id::PropertyRunnerShow => {
            let state = runner(caller)?;
            state.show(&text(caller, int_arg(params, 0)?)?);
            Ret::void()
        }
        Id::FsReadReadBytes
        | Id::FsReadReadText
        | Id::FsReadListDir
        | Id::FsReadStat
        | Id::FsWriteWriteBytes
        | Id::FsWriteWriteText
        | Id::FsWriteAppendText
        | Id::FsWriteCreateDirAll
        | Id::FsWriteRemove
        | Id::FsWriteRename => {
            if finish {
                let Done::Fs(encoded) = finished(caller, params)? else {
                    return Err(not_completed());
                };
                return Ok(Ret::encoded(encoded));
            }
            let key = method.module.strip_prefix("hd:").unwrap_or(method.module);
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            let cwd = match &host.environment {
                Some(environment) => environment.cwd.clone(),
                None => std::env::current_dir().map_err(|e| Stop::Internal(e.to_string()))?,
            };
            match fs::start(
                key,
                method.name,
                &b,
                &host.grants,
                &host.sandbox,
                host.deadline(),
                &cwd,
            )? {
                fs::Start::Ready(e) => Ret::ready(e)?,
                fs::Start::Run(job) => match host.reactor.offload(job.run()) {
                    Ok(h) => Ret::pair(1, h),
                    Err(error) => Ret::ready(fs::not_run(&error))?,
                },
            }
        }
        Id::ProcessRun if finish => match finished(caller, params)? {
            Done::Process(ran) => Ret::encoded(core::process::finish(ran)),
            // A test child that ran into the time limit ends the case.
            Done::Child(None) => return Err(Stop::TimeLimit.into()),
            Done::Child(Some(encoded)) => Ret::encoded(encoded),
            _ => return Err(not_completed()),
        },
        Id::ProcessRun => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            match &host.environment {
                Some(environment) if host.testing.is_some() => {
                    match crate::test_engine::start(&b, &host.grants, environment, host.deadline())?
                    {
                        crate::test_engine::Start::Ready(e) => Ret::ready(e)?,
                        crate::test_engine::Start::Run(run) => {
                            match host.reactor.offload(run.wait()) {
                                Ok(h) => Ret::pair(1, h),
                                Err(error) => Ret::ready(crate::test_engine::not_run(&error))?,
                            }
                        }
                    }
                }
                environment => {
                    let cwd = environment.as_ref().map(|e| e.cwd.as_path());
                    match core::process::start(&b, &host.grants, cwd)? {
                        core::process::Start::Refused(e) => Ret::ready(e)?,
                        core::process::Start::Run(request) => {
                            let name = request.name.clone();
                            match host.reactor.offload(request.run()) {
                                Ok(h) => Ret::pair(1, h),
                                Err(error) => Ret::ready(core::process::not_run(&name, &error))?,
                            }
                        }
                    }
                }
            }
        }
        Id::HttpSend if finish => {
            let Done::Http(sent) = finished(caller, params)? else {
                return Err(not_completed());
            };
            Ret::encoded(http::client::finish(sent)?)
        }
        Id::HttpSend => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            match http::client::start(&b, &host.grants)? {
                http::client::Start::Ready(e) => Ret::ready(e)?,
                http::client::Start::Run(job) => match host.reactor.offload(job.run()) {
                    Ok(h) => Ret::pair(1, h),
                    Err(error) => Ret::ready(http::client::not_run(&error))?,
                },
            }
        }
        Id::NetLookup
        | Id::NetConnect
        | Id::NetListen
        | Id::NetBindUdp
        | Id::NetTcpStreamRead
        | Id::NetTcpStreamWrite
        | Id::NetTcpStreamClose
        | Id::NetTcpListenerAccept
        | Id::NetTcpListenerClose
        | Id::NetUdpSocketSendTo
        | Id::NetUdpSocketReceive
        | Id::NetUdpSocketClose => {
            if finish {
                let Done::Net(done) = finished(caller, params)? else {
                    return Err(not_completed());
                };
                return Ok(Ret::encoded(caller.data_mut().net.finish(done)?));
            }
            // `close` never waits: its reply is the call's result.
            let close = method.wait == Wait::Never;
            let args = (0..params.len())
                .map(|i| int_arg(params, i))
                .collect::<wasmtime::Result<Vec<_>>>()?;
            let b = match (Kind::Host { method, finish }).input() {
                Some(i) => bytes(caller, int_arg(params, i)?)?,
                None => Vec::new(),
            };
            let host = caller.data_mut();
            match host.net.start(method.name, &args, &b, &host.grants)? {
                net::client::Start::Ready(e) if close => Ret::encoded(e),
                net::client::Start::Ready(e) => Ret::ready(e)?,
                net::client::Start::Closed { reply, handle } => {
                    net::ops::dispose(&mut host.reactor, handle);
                    Ret::encoded(reply)
                }
                net::client::Start::Run(job) => {
                    let resource = job.resource();
                    match job.offload(&mut host.reactor) {
                        Ok(h) => Ret::pair(1, h),
                        Err(error) => Ret::ready(net::client::not_run(&error, resource)?)?,
                    }
                }
            }
        }
        Id::RandomNextU64 => {
            let b: [u8; 8] = core::random(8)?
                .try_into()
                .map_err(|_| Stop::Internal("random byte count".into()))?;
            Ret::int(i64::from_le_bytes(b))
        }
        Id::RandomFill => {
            // The ABI count is an unsigned 32-bit value.
            let b = core::random(u32::from_le_bytes(
                int_arg(params, 0)?.to_le_bytes()[..4]
                    .try_into()
                    .unwrap_or([0; 4]),
            ) as usize)?;
            Ret::value(&b)
        }
        Id::SysOs | Id::SysArch | Id::SysHostname | Id::SysCpuCount => {
            Ret::encoded(core::sys(method.name, &caller.data().grants))
        }
        Id::ConsoleWriteLine if finish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::value(&Ok::<(), ConsoleError>(()))
        }
        Id::ConsoleWriteLine => {
            let line = bytes(caller, int_arg(params, 0)?)?;
            // Normalize invalid UTF-8 through the text boundary.
            let line = String::from_utf8_lossy(&line).into_owned().into_bytes();
            Ret::pair(1, caller.data_mut().reactor.write_line(line))
        }
        // `Result[string?, ConsoleError]`: the line, `.None` at the end of
        // input, `.Err(.Closed)` when it cannot be read.
        Id::ConsoleInputReadLine if finish => {
            let h = handle(int_arg(params, 0)?);
            let Some(Work::Read(Some(line))) =
                caller.data_mut().reactor.remove(h).map(|op| op.work)
            else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            Ret::value(&line.map_err(|()| ConsoleError::Closed))
        }
        // Make a completed prompt visible before asking the reader thread
        // for input. Pending until that thread delivers the line.
        Id::ConsoleInputReadLine => {
            let host = caller.data_mut();
            host.flush();
            Ret::pair(1, host.reactor.read_line())
        }
        Id::ClockNow => {
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
            Ret::int(ms)
        }
        Id::ClockMonotonic => Ret::int(i64::try_from(now_ns() / NS_PER_MS).unwrap_or(i64::MAX)),
        Id::ClockSleep if finish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::int(0)
        }
        Id::ClockSleep => {
            let ms = int_arg(params, 0)?;
            Ret::pair(1, caller.data_mut().reactor.sleep(ms))
        }
        Id::ArgsProgram => Ret::value(&caller.data().program),
        Id::ArgsList => Ret::value(&caller.data().args),
        Id::EnvGet => {
            let name = text(caller, int_arg(params, 0)?)?;
            let value = env_var(&name);
            let host = caller.data_mut();
            if host.grants.covers_name("Env", &name) {
                Ret::value(&value)
            } else {
                // Outside the grant, a variable reads as unset.
                if value.is_some() && host.noticed.insert(name.clone()) {
                    host.err(&format!("{}\n", hd_run::env_notice(&name)));
                }
                Ret::value(&None::<String>)
            }
        }
        Id::EnvNames => {
            let names: Vec<String> = std::env::vars_os()
                .map(|(k, _)| k.to_string_lossy().into_owned())
                .filter(|n| caller.data().grants.covers_name("Env", n))
                .collect();
            Ret::value(&names)
        }
    })
}

/// The test runner state a `TestRunner` or `PropertyRunner` call acts on,
/// with the run's deadline for its I/O.
fn runner(caller: &mut Caller<'_, Host>) -> wasmtime::Result<crate::testing::State> {
    let state = caller
        .data()
        .testing
        .clone()
        .ok_or_else(|| Stop::Contract("no test runner binds this run".into()))?;
    state.set_io_deadline(caller.data().deadline());
    Ok(state)
}

/// Takes the result of the offloaded operation whose handle is the first
/// argument, for its `.finish`.
fn finished(caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Done> {
    let h = handle(int_arg(params, 0)?);
    let Some(op) = caller.data_mut().reactor.remove(h) else {
        return Err(Stop::Contract("a finish of an operation that is not pending".into()).into());
    };
    match op.work {
        Work::Offloaded(Some(done)) => Ok(done),
        _ => Err(not_completed()),
    }
}

fn not_completed() -> wasmtime::Error {
    Stop::Contract("a finish of an operation that is not completed".into()).into()
}

/// A variable's value; a name the environment cannot hold is unset.
fn env_var(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return None;
    }
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}
