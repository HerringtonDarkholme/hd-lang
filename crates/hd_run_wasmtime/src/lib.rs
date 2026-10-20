#![forbid(unsafe_code)]
//! `hd_run_wasmtime`: the wasmtime engine and the default profile's host
//! (`crates/hd_mono/DESIGN.md` §11.4; `docs/compiler/engines-and-test-runner.md` §18; `crates/hd_run_wasmtime/DESIGN.md`
//! §17.5).
//!
//! A program runs in-process: its RC module is compiled by Cranelift, instantiated with the imports of `host/`, and driven by
//! `hd.init`, then `hd.poll` and `hd.wake` until it is done, as the Node
//! host (`host/core.mjs`) drives it. Its standard output, error
//! and status are the Node host's: a panic stub's report and status 3, an
//! entry's own status (1 for `.Err`), and `internal error: ...` with
//! status 3 for any other trap. Two traps Node cannot report are panics
//! here, as the spec asks: a stack overflow is `stack-exhausted`
//! (`flow.panic.stack-exhausted`), a refused heap allocation is
//! `heap-exhausted`, and a wall deadline is `time-limit`.
//!
//! A REPL session (`WasmtimeSession`, `session.rs`) runs its program the
//! same way, one input after another, replaying the inputs before the new
//! one from its journal.
//!
//! The host traits of the default profile that run here are `Console`,
//! `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`, `FsWrite`, `Process`,
//! `Random`, `Sys`, `Http`, `Net`, their handle traits, the `hd:rt` runtime
//! imports and the `hd:prim` primitives. Each trait's provider is one
//! module with one `call` entry point (`console`, `sys`, `fs`, `process`,
//! `http`, `net`, `testing::provider`, and the fixtures' `profiles`), which
//! `host/call.rs` routes to; what they share (the reactor, the `hd-io`
//! runtime, the epoch ticker, native error categories) is `runtime/`.
//! A module that imports any other is refused at
//! load with a message naming it; `hd` runs those on Node.

pub mod abi;
mod cache;
mod cancel;
mod code_cache;
mod codec;
mod console;
mod engine;
mod fs;
#[cfg(test)]
#[path = "../tests/support/hd_source.rs"]
mod hd_source;
mod host;
mod http;
mod instance;
mod limits;
mod net;
pub mod prim;
mod process;
mod profiles;
mod run;
mod runtime;
pub mod scenario;
mod session;
#[cfg(test)]
mod session_tests;
mod sys;
mod test_engine;
#[cfg(all(test, unix))]
mod test_fifo;
pub mod testing;
#[cfg(test)]
mod tests;

pub use cancel::HostInterrupt;
pub use code_cache::{CodeCache, CodeCacheStats, code_cache_stats, install_code_cache};
pub use engine::{EngineConfig, Module, OptLevel, WasmtimeEngine, thread_stack};
pub(crate) use engine::{new_store, wasmtime_config};
pub use host::HostIo;
pub use instance::WasmtimeInstance;
pub use profiles::RuntimeProfile;
pub(crate) use run::Shared;
pub use run::{
    Captured, run, run_cancellable, run_captured, run_captured_reading, run_captured_with_profile,
    run_traced, run_with_profile,
};
pub use runtime::io::io_started;
pub use session::{Ran, Stopper, WasmtimeSession};
pub use test_engine::{
    NamedProgram, NativeBodyRunner, PreparedTest, TestEngine, TestEnvironment, TestHostKind,
    TestModule, TestPoolConfig,
};
