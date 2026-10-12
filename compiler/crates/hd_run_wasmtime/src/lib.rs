#![forbid(unsafe_code)]
//! `hd_run_wasmtime`: the wasmtime engine and the default profile's host
//! (codegen.md §11.4; engines-and-test-runner.md §18; runtime-and-host.md
//! §17.5).
//!
//! A program runs in-process: its module is compiled by Cranelift with
//! Wasm GC on, instantiated with the imports of `host.rs`, and driven by
//! `hd.init`, then `hd.poll` and `hd.wake` until it is done, as the Node
//! host (`compiler/host/core.mjs`) drives it. Its standard output, error
//! and status are the Node host's: a panic stub's report and status 3, an
//! entry's own status (1 for `.Err`), and `internal error: ...` with
//! status 3 for any other trap. Two traps Node cannot report are panics
//! here, as the spec asks: a stack overflow is `stack-exhausted`
//! (`flow.panic.stack-exhausted`), a refused heap allocation is
//! `heap-exhausted`, and a wall deadline is `time-limit`.
//!
//! The host traits of the default profile that run here are `Console`,
//! `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`, `FsWrite`, `Process`,
//! `Random`, `Sys`, `Http`, `Net`, their handle traits, the `hd:rt` runtime
//! imports and the `hd:prim` primitives.
//! A module that imports any other is refused at
//! load with a message naming it; `hd` runs those on Node.

mod cache;
mod codec;
mod core;
mod deadline;
mod engine;
mod fs;
mod host;
mod http;
mod instance;
mod limits;
mod native_io;
mod net;
pub mod prim;
mod profiles;
mod property;
mod reactor;
mod run;
mod test_engine;
pub mod testing;
mod testing_files;
mod testing_json;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows_pipe;

pub use engine::{EngineConfig, Module, OptLevel, WasmtimeEngine, thread_stack};
pub(crate) use engine::{new_store, wasmtime_config};
pub use host::HostIo;
pub use instance::WasmtimeInstance;
pub use profiles::RuntimeProfile;
pub(crate) use run::Shared;
pub use run::{
    Captured, run, run_captured, run_captured_with_profile, run_with_guards, run_with_profile,
};
pub use test_engine::{
    NamedProgram, NativeBodyRunner, PreparedTest, TestEngine, TestEnvironment, TestHostKind,
    TestModule, TestPoolConfig,
};
