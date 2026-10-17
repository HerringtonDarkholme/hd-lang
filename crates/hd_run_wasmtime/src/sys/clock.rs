//! `Clock`: wall time, the monotonic clock of `performance.now()`, and
//! `sleep`, a reactor timer that completes when its duration has passed.

use std::time::{SystemTime, UNIX_EPOCH};

use wasmtime::{Caller, Val};

use crate::host::{Host, Ret, handle, int_arg};
use crate::runtime::reactor::{NS_PER_MS, now_ns};

/// `Clock.now`: milliseconds since the Unix epoch.
pub(super) fn now() -> Ret {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Ret::int(ms)
}

/// `Clock.monotonic`: milliseconds since the host started.
pub(super) fn monotonic() -> Ret {
    Ret::int(i64::try_from(now_ns() / NS_PER_MS).unwrap_or(i64::MAX))
}

/// `Clock.sleep`'s `.start`, a timer of its argument's milliseconds, or
/// its `.finish`.
pub(super) fn sleep(
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        let h = int_arg(params, 0)?;
        caller.data_mut().reactor.remove(handle(h));
        return Ok(Ret::int(0));
    }
    let ms = int_arg(params, 0)?;
    Ok(Ret::pair(1, caller.data_mut().reactor.sleep(ms)))
}
