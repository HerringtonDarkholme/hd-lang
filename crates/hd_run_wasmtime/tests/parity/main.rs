//! The wasmtime host's parity tests (core, fixture profiles, HTTP and
//! sockets), linked once. `console_input_parity.rs` and `time_limits.rs`
//! stay separate targets: they re-run their own test binary as a child
//! process, selecting the test by its name.

#[path = "../support/mod.rs"]
mod support;

mod core_parity;
mod fixture_profiles;
mod http_parity;
mod net_parity;
