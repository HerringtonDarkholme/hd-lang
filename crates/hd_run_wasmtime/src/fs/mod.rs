//! Filesystem providers with native errors and scoped grants. Grants are
//! checked against resolved paths before any filesystem access, and under
//! a grant with scope entries every operation resolves under the handle of
//! a granted directory (`sandbox`), so a path cannot leave the grant
//! between the check and the open.
//!
//! A regular file or a directory runs inline on the program thread, with
//! the deadline checked before and after (`api::start`). A special file (a
//! FIFO, a device, a named pipe) can wait on another process, so its call
//! runs blocking `std::fs` on the `hd-io` blocking pool and `.finish`
//! delivers the result (`std-fs.special-file`). Aborting it only drops the
//! result: the spec gives a special file no deadline guarantee beyond "the
//! program is not blocked" (`std-fs.special-file.deadline`).
//!
//! `call` is the provider of `FsRead` and `FsWrite`; `api` decodes and
//! checks each call, `ops` and `io` do it, and `sandbox` holds the granted
//! directories' handles.

use wasmtime::{Caller, Val};

use crate::abi::HostMethod;
use crate::host::offload::{finished, not_completed, pending};
use crate::host::{Host, Ret, Stop, bytes, int_arg};
use crate::runtime::reactor::Done;

pub(crate) mod api;
/// Special-file coverage; every item is unix-only, like its tests.
#[cfg(all(test, unix))]
pub(crate) mod fifo;
pub(crate) mod io;
pub(crate) mod ops;
pub(crate) mod sandbox;
#[cfg(test)]
mod sandbox_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use api::{read_file_until, write_file_until};
pub(crate) use sandbox::Sandbox;

/// An `FsRead` or `FsWrite` call's `.start`, or its `.finish` when
/// `finish`.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
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
    match api::start(
        key,
        method.name,
        &b,
        &host.grants,
        &host.sandbox,
        host.deadline(),
        &cwd,
    )? {
        api::Start::Ready(e) => Ret::ready(e),
        api::Start::Run(job) => pending(host.reactor.offload(job.run()), api::not_run),
    }
}
