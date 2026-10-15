//! The default Http provider: each `send!` is one task on the `hd-io`
//! runtime, which sends the request on the runtime's blocking pool
//! (`client.rs`), so the program runs its other tasks meanwhile and a
//! cancel closes the connection. Each redirect rechecks the URL's grant
//! before DNS or I/O.
//!
//! ureq supplies TCP, TLS, deadlines, header and body framing. A response
//! header name arrives in lowercase and only the values of one name keep
//! their order (`std-http.headers.case`, `.order`). Compression and
//! environment proxies are deliberately disabled. Timed DNS returns promptly, but std's
//! underlying getaddrinfo call in the DNS helper thread cannot be aborted.

pub(crate) mod client;
pub(crate) mod connector;
pub(crate) mod request;
pub(crate) mod target;
#[cfg(test)]
pub(crate) mod test_support;

/// Exchange headers: name and value pairs.
pub(crate) type Headers = Vec<(String, String)>;

pub(crate) fn is_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
