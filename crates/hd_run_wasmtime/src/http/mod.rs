//! The default Http provider: each `send!` is one task on the `hd-io`
//! runtime, which sends the request on the runtime's blocking pool
//! (`client.rs`), so the program runs its other tasks meanwhile and a
//! cancel closes the connection. Each redirect rechecks the URL's grant
//! before DNS or I/O.
//!
//! ureq supplies TCP, TLS, deadlines and response-body framing. Its
//! `HeaderMap` cannot retain raw header spelling and order, so a plaintext
//! transport wrapper preserves response pairs and writes Node's original
//! request headers and byte body. Compression and environment proxies
//! are deliberately disabled. Timed DNS returns promptly, but std's
//! underlying getaddrinfo call in the DNS helper thread cannot be aborted.

pub(crate) mod chunk;
pub(crate) mod client;
pub(crate) mod connector;
pub(crate) mod headers;
pub(crate) mod request;
pub(crate) mod target;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod transport;

/// Exchange headers: name and value pairs, in wire order.
pub(crate) type Headers = Vec<(String, String)>;

pub(crate) fn is_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(is_token_byte)
}

pub(crate) fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

pub(crate) fn parse_error(reason: &str) -> ureq::Error {
    ureq::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Parse Error: {reason}"),
    ))
}
