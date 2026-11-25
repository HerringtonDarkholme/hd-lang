//! Signals share the process broker with CLI cancellation.
use crate::abi::Signal;
use crate::codec::{Dec, Wire};
use crate::host::{Host, Ret, Stop, bytes, handle, int_arg};
use hd_run::signals::Kind;
use wasmtime::{Caller, Val};

pub(super) fn wait(
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        let h = int_arg(params, 0)?;
        caller.data_mut().reactor.remove(handle(h));
        return Ok(Ret::int(0));
    }
    if !caller.data().grants.allows("Signals") {
        return Err(Stop::Contract("capability not granted: Signals".into()).into());
    }
    let bytes = bytes(caller, int_arg(params, 0)?)?;
    let kind = match Signal::dec(&mut Dec::new(&bytes))? {
        Signal::Interrupt => Kind::Interrupt,
        Signal::Terminate => Kind::Terminate,
        Signal::Hangup => Kind::Hangup,
    };
    let h = caller
        .data_mut()
        .reactor
        .signal(kind)
        .map_err(Stop::Internal)?;
    Ok(Ret::pair(1, h))
}
