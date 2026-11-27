//! `hd_run_wasmtime`'s unit tests, over the `internals` facade (`src/internals.rs`):
//! one module per source module they test, at the same path, so each
//! test keeps its name.

pub(crate) mod cache;
pub(crate) mod cancel;
pub(crate) mod checked_report;
pub(crate) mod code_cache;
pub(crate) mod codec;
pub(crate) mod engine;
pub(crate) mod fs;
#[path = "../support/hd_source.rs"]
pub(crate) mod hd_source;
pub(crate) mod host;
pub(crate) mod http;
pub(crate) mod limits;
pub(crate) mod native;
pub(crate) mod net;
pub(crate) mod prim;
pub(crate) mod process;
pub(crate) mod runtime;
pub(crate) mod sampling;
pub(crate) mod session_tests;
pub(crate) mod sys;
pub(crate) mod test_engine;
#[cfg(unix)]
pub(crate) mod test_fifo;
pub(crate) mod testing;
pub(crate) mod tests;
pub(crate) mod workers;
