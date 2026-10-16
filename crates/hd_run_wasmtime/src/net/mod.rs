//! `Net` on the `hd-io` runtime: the
//! instance keeps a table of its sockets (`client.rs`), and each operation
//! that waits is one task (`ops.rs`) that delivers into the reactor.

use std::time::Duration;

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
