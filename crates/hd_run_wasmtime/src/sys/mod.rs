//! The system providers: `Clock` (`clock.rs`), `Sys` (`platform.rs`),
//! `Random` (`random.rs`), and `Args` and `Env` (`env.rs`). None of their
//! methods waits on the `hd-io` runtime: `Clock.sleep` is a reactor timer.

use wasmtime::{Caller, Val};

use crate::abi::{HostMethod, Id};
use crate::host::{Host, Ret, Stop, int_arg};

mod clock;
mod env;
mod platform;
mod random;
#[cfg(test)]
mod tests;

pub(crate) use random::random;

/// A `Clock`, `Sys`, `Random`, `Args` or `Env` call: its one import, or
/// `Clock.sleep`'s `.start` or, when `finish`, its `.finish`.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match method.id {
        Id::ClockNow => clock::now(),
        Id::ClockMonotonic => clock::monotonic(),
        Id::ClockSleep => clock::sleep(finish, caller, params)?,
        Id::RandomNextU64 => {
            let b: [u8; 8] = random(8)?
                .try_into()
                .map_err(|_| Stop::Internal("random byte count".into()))?;
            Ret::int(i64::from_le_bytes(b))
        }
        Id::RandomFill => {
            // The ABI count is an unsigned 32-bit value.
            let b = random(u32::from_le_bytes(
                int_arg(params, 0)?.to_le_bytes()[..4]
                    .try_into()
                    .unwrap_or([0; 4]),
            ) as usize)?;
            Ret::value(&b)
        }
        Id::SysOs | Id::SysArch | Id::SysHostname | Id::SysCpuCount => {
            Ret::encoded(platform::sys(method.name, &caller.data().grants))
        }
        Id::ArgsProgram => Ret::value(&caller.data().program),
        Id::ArgsList => Ret::value(&caller.data().args),
        Id::EnvGet => env::get(caller, params)?,
        Id::EnvNames => env::names(caller),
        _ => return Err(crate::host::misrouted(method)),
    })
}
