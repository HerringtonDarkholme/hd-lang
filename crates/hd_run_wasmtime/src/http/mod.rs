//! The default Http provider: each `send!` is one task on the `hd-io`
//! runtime, which sends the request on the runtime's blocking pool
//! (`client.rs`), so the program runs its other tasks meanwhile and a
//! cancel closes the connection. Each redirect rechecks the URL's grant
//! before DNS or I/O.
//!
//! ureq supplies TCP, TLS, deadlines, header and body framing. A response
//! header name arrives in lowercase and only the values of one name keep
//! their order (`std-http.headers.case`, `.order`). The provider advertises
//! and decodes gzip and zlib-wrapped deflate itself; ureq's implicit
//! compression and environment proxies are disabled. Timed DNS returns
//! promptly, but std's
//! underlying getaddrinfo call in the DNS helper thread cannot be aborted.
//!
//! `call` is the provider; `client` starts and finishes a send, `request`
//! checks and frames it, `target` resolves and checks each URL, and
//! `connector` opens its connections.

use wasmtime::{Caller, Val};

use crate::abi::HostMethod;
use crate::host::offload::{finished, not_completed, pending};
use crate::host::{Host, Ret, bytes, int_arg};
use crate::runtime::reactor::Done;

pub(crate) mod client;
pub(crate) mod connector;
pub(crate) mod request;
pub(crate) mod target;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod tls;

/// Exchange headers: name and value pairs.
pub(crate) type Headers = Vec<(String, String)>;

pub(crate) fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// `Http.send`'s `.start`, or its `.finish` when `finish`.
pub(crate) fn call(
    _method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        let Done::Http(sent) = finished(caller, params)? else {
            return Err(not_completed());
        };
        return Ok(Ret::encoded(client::finish(sent)?));
    }
    let b = bytes(caller, int_arg(params, 0)?)?;
    let host = caller.data_mut();
    match client::start(&b, &host.grants)? {
        client::Start::Ready(e) => Ret::ready(e),
        client::Start::Run(job) => pending(host.reactor.offload(job.run()), client::not_run),
    }
}
