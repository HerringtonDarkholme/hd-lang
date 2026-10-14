use std::time::{SystemTime, UNIX_EPOCH};

use wasmtime::{Caller, Val};

use super::kind::{Kind, TestMethod};
use super::link::deadlock;
use super::reply::{Ret, bytes, complete, encode_line, float_arg, handle, int_arg, text};
use super::{Control, Host, Stop};
use crate::codec::Enc;
use crate::prim;
use crate::profiles;
use crate::reactor::{Done, NS_PER_MS, Work, now_ns};
use crate::{core, fs, net};

pub(crate) fn call(
    kind: Kind,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
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
                    let mut encoded = Enc::default();
                    encoded.leb(u64::try_from(id).unwrap_or(u64::MAX));
                    encoded.into_bytes()
                }
                (profiles::Method::FileHandleRead, Work::FixtureRead { file }) => caller
                    .data_mut()
                    .fixture
                    .read_file(file)
                    .map_err(Stop::Contract)?,
                (profiles::Method::FileHandleClose, Work::FixtureClose { file }) => caller
                    .data_mut()
                    .fixture
                    .close_file(file)
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
                Ret::pair(1, caller.data_mut().reactor.fixture_close(file))
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
            let host = caller.data_mut();
            match &host.environment {
                Some(environment) if host.testing.is_some() => {
                    Ret::ready(crate::test_engine::process(
                        &b,
                        &host.grants,
                        environment,
                        host.deadline(),
                        host.memory_bytes,
                    )?)?
                }
                environment => {
                    let cwd = environment.as_ref().map(|e| e.cwd.as_path());
                    match core::process::start(&b, &host.grants, cwd)? {
                        core::process::Start::Refused(e) => Ret::ready(e)?,
                        core::process::Start::Run(request) => {
                            let name = request.name.clone();
                            match host
                                .reactor
                                .offload(async move { Done::Process(request.run().await) })
                            {
                                Ok(h) => Ret::pair(1, h),
                                Err(error) => Ret::ready(core::process::not_run(&name, &error))?,
                            }
                        }
                    }
                }
            }
        }
        Kind::ProcessFinish => {
            let h = handle(int_arg(params, 0)?);
            let Some(op) = caller.data_mut().reactor.remove(h) else {
                return Err(
                    Stop::Contract("a finish of an operation that is not pending".into()).into(),
                );
            };
            let Work::Offloaded(Some(Done::Process(ran))) = op.work else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            Ret::encoded(core::process::finish(ran))
        }
        Kind::HttpStart => {
            let b = bytes(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            let e = host.http.send_until(&b, &host.grants, host.deadline())?;
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
            match host.net.start(method, &args, &b, &host.grants)? {
                net::client::Start::Ready(e) if close => Ret::encoded(e),
                net::client::Start::Ready(e) => Ret::ready(e)?,
                net::client::Start::Run(job) => {
                    let resource = job.resource();
                    match host
                        .reactor
                        .offload(async move { Done::Net(job.run().await) })
                    {
                        Ok(h) => Ret::pair(1, h),
                        Err(error) => Ret::ready(net::client::not_run(&error, resource)?)?,
                    }
                }
            }
        }
        Kind::NetFinish => {
            let h = handle(int_arg(params, 0)?);
            let Some(op) = caller.data_mut().reactor.remove(h) else {
                return Err(
                    Stop::Contract("a finish of an operation that is not pending".into()).into(),
                );
            };
            let Work::Offloaded(Some(Done::Net(done))) = op.work else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            Ret::encoded(caller.data_mut().net.finish(done)?)
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

/// A variable's value; a name the environment cannot hold is unset.
fn env_var(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return None;
    }
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}
