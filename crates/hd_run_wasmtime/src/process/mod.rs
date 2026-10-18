//! `Process.run!`. An ordinary run starts a native child (`native.rs`); a
//! test case's run starts one of its package's executables as a child
//! instance (`test_child.rs`). Either `.start` checks the grant and the
//! arguments at once, and the child runs off the program thread, so
//! `.finish` takes its result from the reactor.

use wasmtime::{Caller, Val};

use crate::abi::HostMethod;
use crate::host::offload::{finished, not_completed, pending};
use crate::host::{Host, Ret, Stop, bytes, int_arg};
use crate::runtime::reactor::Done;

pub(crate) mod native;
pub(crate) mod test_child;
#[cfg(test)]
mod tests;

/// `Process.run`'s `.start`, or its `.finish` when `finish`.
pub(crate) fn call(
    _method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        return Ok(match finished(caller, params)? {
            Done::Process(ran) => Ret::encoded(native::finish(ran)),
            // A test child that ran into the time limit ends the case.
            Done::Child(None) => return Err(Stop::TimeLimit.into()),
            Done::Child(Some(encoded)) => Ret::encoded(encoded),
            _ => return Err(not_completed()),
        });
    }
    let b = bytes(caller, int_arg(params, 0)?)?;
    let host = caller.data_mut();
    match &host.environment {
        Some(environment) if host.testing.is_some() => {
            match test_child::start(
                &b,
                &host.grants,
                environment,
                &host.env_overlay,
                host.deadline(),
            )? {
                test_child::Start::Ready(e) => Ret::ready(e),
                test_child::Start::Run(run) => {
                    pending(host.reactor.offload(run.wait()), test_child::not_run)
                }
            }
        }
        environment => {
            let cwd = environment.as_ref().map(|e| e.cwd.as_path());
            match native::start(&b, &host.grants, cwd)? {
                native::Start::Refused(e) => Ret::ready(e),
                native::Start::Run(request) => {
                    let name = request.name.clone();
                    pending(host.reactor.offload(request.run()), |error| {
                        native::not_run(&name, error)
                    })
                }
            }
        }
    }
}
