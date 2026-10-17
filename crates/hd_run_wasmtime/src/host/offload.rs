//! The two halves every waiting provider shares: a `.start` whose work
//! runs on the `hd-io` runtime replies with its pending handle, and its
//! `.finish` takes the result the work delivered into the reactor.

use wasmtime::{Caller, Val};

use super::reply::{Ret, handle, int_arg};
use super::{Host, Stop};
use crate::codec::Enc;
use crate::runtime::reactor::{Done, Work};

/// The reply of a `.start` that offloaded its work (`started`): pending on
/// its handle, or, when the runtime cannot start, `not_run`'s result at
/// once.
pub(crate) fn pending(
    started: Result<i32, String>,
    not_run: impl FnOnce(&str) -> Enc,
) -> wasmtime::Result<Ret> {
    match started {
        Ok(h) => Ok(Ret::pair(1, h)),
        Err(error) => Ret::ready(not_run(&error)),
    }
}

/// Takes the result of the offloaded operation whose handle is the first
/// argument, for its `.finish`.
pub(crate) fn finished(caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Done> {
    let h = handle(int_arg(params, 0)?);
    let Some(op) = caller.data_mut().reactor.remove(h) else {
        return Err(Stop::Contract("a finish of an operation that is not pending".into()).into());
    };
    match op.work {
        Work::Offloaded(Some(done)) => Ok(done),
        _ => Err(not_completed()),
    }
}

/// The `host-contract` stop of a `.finish` whose operation has not
/// completed, or completed with another provider's result.
pub(crate) fn not_completed() -> wasmtime::Error {
    Stop::Contract("a finish of an operation that is not completed".into()).into()
}
