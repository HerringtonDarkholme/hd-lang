use std::io;
use std::time::Instant;

use hd_run::Grants;

use crate::codec::{Dec, Enc};
use crate::host::Stop;

use super::workers::run_process_at;

pub fn process(bytes: &[u8], grants: &Grants) -> wasmtime::Result<Enc> {
    process_impl(bytes, grants, None, None)
}

pub fn process_with_deadline(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
) -> wasmtime::Result<Enc> {
    match deadline {
        Some(deadline) => process_impl(bytes, grants, Some(deadline), None),
        None => process(bytes, grants),
    }
}

pub(crate) fn process_with_deadline_at(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: &std::path::Path,
) -> wasmtime::Result<Enc> {
    process_impl(bytes, grants, deadline, Some(cwd))
}

fn process_impl(
    bytes: &[u8],
    grants: &Grants,
    deadline: Option<Instant>,
    cwd: Option<&std::path::Path>,
) -> wasmtime::Result<Enc> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    let mut d = Dec::new(bytes);
    let name = d.str()?;
    let argv = d.strings()?;
    let input = d.bytes()?;
    let mut e = Enc::default();
    if !grants.covers_name("Process", &name) {
        e.leb(1).leb(2);
        return Ok(e);
    }
    if name.is_empty() {
        e.leb(1).leb(0);
        return Ok(e);
    }
    if let Some(message) = validate_process(&name, &argv) {
        e.leb(1).leb(3).str(&message);
        return Ok(e);
    }
    let result = run_process_at(&name, &argv, input, 1 << 30, deadline, cwd);
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Stop::TimeLimit.into());
    }
    match result {
        Ok((out, err, status)) => {
            e.leb(0)
                .str(&String::from_utf8_lossy(&out))
                .str(&String::from_utf8_lossy(&err))
                .zz(status);
        }
        Err(err) => {
            let variant = match crate::native_io::error_kind(&err) {
                io::ErrorKind::NotFound => 0,
                io::ErrorKind::PermissionDenied => 1,
                _ => 3,
            };
            e.leb(1).leb(variant);
            if variant == 3 {
                e.str(&format!("cannot run {name:?}: {err}"));
            }
        }
    }
    Ok(e)
}

fn validate_process(name: &str, argv: &[String]) -> Option<String> {
    if name.contains('\0') {
        return Some("program name contains a NUL byte".into());
    }
    argv.iter()
        .position(|arg| arg.contains('\0'))
        .map(|index| format!("process argument {index} contains a NUL byte"))
}
