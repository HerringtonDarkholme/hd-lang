//! Test-provider state and orchestration, separate from Wasmtime stores.
//! A property retains one successful instance; failing/discarded bodies
//! poison it. Node currently creates a fresh instance for every body.

pub(crate) mod case;
pub(crate) mod runner;
pub(crate) mod state;
#[cfg(test)]
mod tests;

pub use case::{BodyResult, CaseRun, PanicFrame, PanicInfo, Snapshot, TestCase};
pub use runner::{BodyRunner, execute_case};
pub use state::State;
