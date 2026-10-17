//! `Net` on the `hd-io` runtime: the
//! instance keeps a table of its sockets (`client.rs`), and each operation
//! that waits is one task (`ops.rs`) that delivers into the reactor.
//! `call` is the provider of `Net` and its handle traits; `validate`
//! checks addresses and grants, and `socket` holds the sockets.

use std::time::Duration;

use hd_host_abi::Wait;
use wasmtime::{Caller, Val};

use crate::abi::HostMethod;
use crate::host::offload::{finished, not_completed};
use crate::host::{Host, Ret, bytes, int_arg};
use crate::runtime::reactor;

pub(crate) mod client;
pub(crate) mod ops;
pub(crate) mod socket;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod validate;

pub use client::Client;
pub use ops::Done;

pub(crate) use socket::connect_attempts;

/// The most bytes one read takes, and the receive buffer.
pub(crate) const CHUNK: usize = 65536;
pub(crate) const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

/// A `Net`, `TcpStream`, `TcpListener` or `UdpSocket` call: its one import,
/// or its `.start`, or its `.finish` when `finish`. `input` is the
/// parameter that holds its structured argument's length, if it has one.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    input: Option<usize>,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        let reactor::Done::Net(done) = finished(caller, params)? else {
            return Err(not_completed());
        };
        return Ok(Ret::encoded(caller.data_mut().net.finish(done)?));
    }
    // `set_timeout`, the options, `local_address` and `close` never wait: the reply is the
    // call's result.
    let at_once = method.wait == Wait::Never;
    let args = (0..params.len())
        .map(|i| int_arg(params, i))
        .collect::<wasmtime::Result<Vec<_>>>()?;
    let b = match input {
        Some(i) => bytes(caller, int_arg(params, i)?)?,
        None => Vec::new(),
    };
    let host = caller.data_mut();
    match host.net.start(method.name, &args, &b, &host.grants)? {
        client::Start::Ready(e) if at_once => Ok(Ret::encoded(e)),
        client::Start::Ready(e) => Ret::ready(e),
        client::Start::Closed { reply, handle } => {
            ops::dispose(&mut host.reactor, handle);
            Ok(Ret::encoded(reply))
        }
        client::Start::Run(job) => {
            let resource = job.resource();
            match job.offload(&mut host.reactor) {
                Ok(h) => Ok(Ret::pair(1, h)),
                Err(error) => Ret::ready(client::not_run(&error, resource)?),
            }
        }
    }
}
