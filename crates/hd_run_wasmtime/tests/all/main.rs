//! The wasmtime host's integration tests, linked once: parity (core,
//! fixture profiles, HTTP and sockets), console flushing and input, the
//! native companion and time limits. Each test binary linked the compiler
//! and Wasmtime again (7 binaries, 129 CPU-s to rebuild locally). Tests
//! that re-run this binary as a child process select their test by its
//! full name, module included. `io_runtime.rs` stays its own binary: it
//! checks that no other test in the process started the I/O runtime, and
//! `child_signals.rs` has no test harness.

#[path = "../support/mod.rs"]
mod support;

mod console_flush;
mod console_input_parity;
mod core_parity;
mod fixture_profiles;
mod http_parity;
mod native_companion;
mod net_parity;
mod time_limits;
