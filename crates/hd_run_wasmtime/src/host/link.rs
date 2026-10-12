use std::collections::HashSet;

use wasmtime::{FuncType, Linker, Val, ValType};

use super::Stop;
use super::call::call;
use super::kind::Kind;
use super::reply::{Ret, Value, int_val, put_bytes};
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
        defined.insert(key);
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
