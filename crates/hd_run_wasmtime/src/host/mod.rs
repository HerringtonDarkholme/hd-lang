//! The boundary between a Wasm instance and the native providers for
//! `hd run` on wasmtime, implementing the imports described by
//! `hd_host_abi` and the specification's result contracts.
//!
//! - `kind.rs` resolves each import to what runs it, and `link.rs`
//!   defines it in a linker, with a REPL journal's record and replay.
//! - `call.rs` routes each call to its provider's one entry point:
//!   `console`, `sys` (`Clock`, `Sys`, `Random`, `Args`, `Env`), `fs`,
//!   `process`, `http`, `net`, `testing::provider`, and the conformance
//!   fixtures of `profiles`.
//! - `rt.rs` runs the imports no host trait owns. `hd:rt`: `block()` waits
//!   for at least one completion and writes the completed handles to the
//!   exchange buffer; `stderr(len)` writes the buffer's first `len` bytes
//!   to standard error; `stdout(len)` holds them with `Console`'s standard
//!   output (the REPL's value line); `abort(h)` drops an operation;
//!   `panic()` starts a panic stub's report: it writes `panic: `, and the
//!   standard error written until the trap is the report a test runner
//!   reads the category and message of; `fact(len)` marks that a facts
//!   body starts, its label in the buffer, or ends (`-1`), and every
//!   panic report while one runs, the stub's or the host's own, is
//!   `fact-evaluation-failed` naming it. `hd:prim`: the float text, parsing
//!   and case mapping of `prim.rs`.
//! - `reply.rs` reads arguments from and writes results to the exchange
//!   buffer; `offload.rs` is the `.start` and `.finish` every provider
//!   that waits on the `hd-io` runtime shares.
//! - `state.rs` is the per-instance `Host`: its streams, grants, deadline,
//!   reactor and provider state.
//!
//! Standard output is held until input starts, standard error is written,
//! a deadlock is reported, or the run ends.

use crate::abi::HostMethod;
use crate::runtime::reactor::now_ns;

mod call;
mod kind;
mod link;
pub(crate) mod offload;
mod output;
mod reply;
mod rt;
mod state;

pub use kind::supported;
pub use link::{deadlock, define, unsupported_message};
pub(crate) use link::{define_session, define_test};
pub(crate) use output::OutputError;
pub use reply::complete;
pub(crate) use reply::{Ret, bytes, deliver_batch, handle, int_arg, text};
pub use state::{Host, HostIo};

/// Why the host stopped the instance, for the driver's panic report.
#[derive(Debug)]
pub enum Stop {
    /// A host result or call that breaks the ABI: the `host-contract` panic.
    Contract(String),
    /// The program waits and nothing can wake it; already reported.
    Deadlock,
    /// The exchange buffer cannot grow: the `heap-exhausted` panic.
    HeapExhausted,
    /// The program's wall-clock deadline has elapsed.
    TimeLimit,
    /// The session's stop handle was used: an interrupted REPL input.
    Interrupted,
    /// A bug in std or the host.
    Internal(String),
    /// Property exhaustion is successful early termination, not a panic.
    EndCase,
    /// A replayed input asked the host for what its record does not hold;
    /// the replay keeps the message.
    Diverged,
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stop::Contract(m) | Stop::Internal(m) => f.write_str(m),
            Stop::Deadlock => f.write_str("deadlock"),
            Stop::HeapExhausted => f.write_str("the exchange buffer cannot grow"),
            Stop::TimeLimit => f.write_str("the program exceeded its time limit"),
            Stop::Interrupted => f.write_str("stopped"),
            Stop::EndCase => f.write_str("the property has no remaining cases"),
            Stop::Diverged => f.write_str("the replay diverged from its record"),
        }
    }
}

impl std::error::Error for Stop {}

/// Existing host control events, distinct from reply and wake delivery.
/// Deadlines become traps through import dispatch or the engine driver;
/// this boundary owns their final flush and bounded provider cleanup.
#[derive(Clone, Copy)]
pub(crate) enum Control {
    Abort(i32),
    Exit {
        status: i32,
        close: bool,
    },
    Trap {
        close: bool,
    },
    /// A run the host may cancel that a stop request ended while Wasm ran.
    Stop {
        close: bool,
    },
}

/// Starts the monotonic clock, so it counts from the process's start.
pub fn start_clock() {
    now_ns();
}

/// The stop for a host method routed to a provider that does not handle
/// it: a bug in `call.rs`.
pub(crate) fn misrouted(method: &HostMethod) -> wasmtime::Error {
    Stop::Internal(format!(
        "no provider handles `{}` `{}`",
        method.module, method.name
    ))
    .into()
}
