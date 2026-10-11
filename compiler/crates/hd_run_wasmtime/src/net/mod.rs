//! Per-instance synchronous Net client. One worker owns every socket,
//! buffers arrivals and preserves the first read end/error.

use std::time::Duration;

pub(crate) mod client;
pub(crate) mod ops;
pub(crate) mod socket;
pub(crate) mod state;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod validate;

pub use client::Client;

pub(crate) use socket::connect_attempts;

pub(crate) const METHODS: &[&str] = &[
    "lookup",
    "connect",
    "listen",
    "bind_udp",
    "TcpStream.read",
    "TcpStream.write",
    "TcpListener.accept",
    "UdpSocket.send_to",
    "UdpSocket.receive",
];
pub(crate) const CLOSE: &[&str] = &["TcpStream.close", "TcpListener.close", "UdpSocket.close"];
pub(crate) const TICK: Duration = Duration::from_millis(5);
pub(crate) const CHUNK: usize = 65536;
pub(crate) const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);
pub(crate) const CLEANUP_TIMEOUT: Duration = Duration::from_millis(250);
