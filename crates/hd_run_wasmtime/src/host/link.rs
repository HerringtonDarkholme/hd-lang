use std::collections::HashSet;

use hd_run::journal::{self, Recorder};
use wasmtime::{Caller, FuncType, Linker, Val, ValType};

use super::Stop;
use super::call::call;
use super::kind::{Kind, Record};
use super::reply::{Ret, Value, bytes, int_arg, int_val, put_bytes};
use super::state::Host;
use crate::profiles::RuntimeProfile;

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
    define_with(
        linker,
        module.imports(),
        &mut HashSet::new(),
        |module, name| Kind::with_profile(module, name, profile),
    )
}

/// Defines the host imports of a session input (repl-plan.md §2.3) in the
/// session's one linker: every import of `module` but its session imports
/// (`hd_wasm::SESSION_MODULE`), which earlier inputs' exports satisfy, and
/// those an earlier input already defined (`defined`).
pub(crate) fn define_session(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    profile: RuntimeProfile,
    defined: &mut HashSet<(String, String)>,
) -> Result<(), String> {
    let host = module
        .imports()
        .filter(|i| i.module() != hd_wasm::SESSION_MODULE);
    define_with(linker, host, defined, |module, name| {
        Kind::with_profile(module, name, profile)
    })
}

pub(crate) fn define_test(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    kind: crate::test_engine::TestHostKind,
) -> Result<(), String> {
    define_with(
        linker,
        module.imports(),
        &mut HashSet::new(),
        |module, name| Kind::with_test(module, name, kind),
    )
}

fn define_with<'m>(
    linker: &mut Linker<Host>,
    imports: impl Iterator<Item = wasmtime::ImportType<'m>>,
    defined: &mut HashSet<(String, String)>,
    kind_of: impl Fn(&str, &str) -> Option<Kind>,
) -> Result<(), String> {
    for import in imports {
        let (m, n) = (import.module(), import.name());
        // Wasm permits repeated imports. Node reuses the same property;
        // the linker likewise needs only one definition for that name, and
        // a session's linker one for all its inputs.
        let key = (m.to_owned(), n.to_owned());
        if defined.contains(&key) {
            continue;
        }
        let Some(kind) = kind_of(m, n) else {
            return Err(unsupported_message(m, n));
        };
        let Some(ty) = import.ty().func().cloned() else {
            return Err(format!("the import `{m}` `{n}` is not a function"));
        };
        let results: Vec<ValType> = ty.results().collect();
        let site = Site {
            index: defined.len(),
            record: kind.record(),
            input: kind.input(),
            module: key.0.clone(),
            name: key.1.clone(),
        };
        linker
            .func_new(
                m,
                n,
                FuncType::clone(&ty),
                move |mut caller, params, out| {
                    kind.validate_results(&results)?;
                    let host = caller.data();
                    let replaying = host.replay.is_some() && site.record != Record::Skip;
                    let args =
                        if site.record == Record::Call && (replaying || host.journal.is_some()) {
                            // A recording whose arguments cannot be read records
                            // nothing: the call fails at the same read.
                            match site.args(&mut caller, params) {
                                Ok(args) => Some(args),
                                Err(error) if replaying => return Err(error),
                                Err(_) => None,
                            }
                        } else {
                            None
                        };
                    let Ret { value, payload } = if replaying {
                        site.serve(&mut caller, params, args.as_deref())?
                    } else {
                        if site.record != Record::Skip {
                            caller.data_mut().calls += 1;
                        }
                        call(kind, &mut caller, params)?
                    };
                    if let Some(journal) = caller.data_mut().journal.as_mut() {
                        let args = args.as_deref().unwrap_or_default();
                        site.record(journal, params, args, &value, payload.as_deref());
                    }
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
        defined.insert(key);
    }
    Ok(())
}

/// One defined import, as a session's journal records its calls and a
/// rebuild's replay serves them: the recorder and the replayer sit here,
/// where every host import is dispatched (repl-plan.md §5 Q8c). `index` is
/// unique in its linker.
struct Site {
    index: usize,
    record: Record,
    /// The parameter that holds the length of the exchange buffer bytes
    /// the call reads (`Kind::input`).
    input: Option<usize>,
    module: String,
    name: String,
}

/// The handle a `.finish` or `abort` names; no live one is negative.
fn handle_param(params: &[Val]) -> u32 {
    params
        .first()
        .and_then(|p| match p {
            Val::I32(v) => u32::try_from(*v).ok(),
            Val::I64(v) => u32::try_from(*v).ok(),
            _ => None,
        })
        .unwrap_or(u32::MAX)
}

impl Site {
    /// The call's arguments as a journal holds them: its scalar
    /// parameters, then the exchange buffer bytes it reads.
    fn args(&self, caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Vec<u8>> {
        let mut args = Vec::new();
        for param in params {
            match param {
                Val::I32(v) => journal::put_int_arg(&mut args, i64::from(*v)),
                Val::I64(v) => journal::put_int_arg(&mut args, *v),
                Val::F32(bits) => journal::put_float_arg(&mut args, u64::from(*bits)),
                Val::F64(bits) => journal::put_float_arg(&mut args, *bits),
                _ => {}
            }
        }
        if let Some(i) = self.input {
            args.extend(bytes(caller, int_arg(params, i)?)?);
        }
        Ok(args)
    }

    /// Answers a call of a replayed input from its record instead of making
    /// it. A call the record does not hold next is a divergence, which
    /// stops the input; the replay keeps its message.
    fn serve(
        &self,
        caller: &mut Caller<'_, Host>,
        params: &[Val],
        args: Option<&[u8]>,
    ) -> wasmtime::Result<Ret> {
        let host = caller.data_mut();
        host.check_deadline()?;
        let replay = host
            .replay
            .as_mut()
            .ok_or_else(|| Stop::Internal("a call served with no replay".into()))?;
        let served_ret = |(value, payload): (journal::Value, Option<Vec<u8>>)| Ret {
            value: match value {
                journal::Value::Void => Value::Void,
                journal::Value::Int(v) => Value::Int(v),
                journal::Value::F64(bits) => Value::F64(f64::from_bits(bits)),
                journal::Value::Pair(status, v) => Value::Pair(status, v),
            },
            payload,
        };
        let served = match self.record {
            Record::Skip => {
                return Err(Stop::Internal("an `hd:prim` call is never served".into()).into());
            }
            Record::Call => replay
                .call(&self.module, &self.name, args.unwrap_or_default())
                .map(served_ret),
            Record::Finish => replay.finish(handle_param(params)).map(served_ret),
            Record::Abort => replay.abort(handle_param(params)).map(|()| Ret::void()),
            Record::Block => match replay.block() {
                Ok(Some(handles)) => return Ret::batch(&handles),
                // The first run's block found nothing pending either.
                Ok(None) => return Err(deadlock(host)),
                Err(message) => Err(message),
            },
        };
        served.map_err(|_| Stop::Diverged.into())
    }

    /// Writes the row of a call with the arguments `args` that returned
    /// `value` and `payload`.
    fn record(
        &self,
        journal: &mut Recorder,
        params: &[Val],
        args: &[u8],
        value: &Value,
        payload: Option<&[u8]>,
    ) {
        let value = match *value {
            Value::Void => journal::Value::Void,
            Value::Int(v) => journal::Value::Int(v),
            Value::F64(v) => journal::Value::F64(v.to_bits()),
            Value::Pair(status, v) => journal::Value::Pair(status, v),
        };
        match self.record {
            Record::Skip => {}
            Record::Call => {
                journal.call(self.index, &self.module, &self.name, args, value, payload);
            }
            Record::Finish => journal.finish(handle_param(params), value, payload),
            Record::Block => {
                let handles: Vec<u32> = payload
                    .unwrap_or_default()
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                journal.block(&handles);
            }
            Record::Abort => journal.abort(handle_param(params)),
        }
    }
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
