//! The test providers and their state, separate from Wasmtime stores: the
//! `TestRunner` and `PropertyRunner` imports (`provider.rs`), a case's
//! shared state (`state.rs`), its snapshot and temporary files
//! (`files.rs`), property plans and shrinking (`property.rs`), the case
//! runner (`runner.rs`), and the Node runner's JSON-line protocol
//! (`json.rs`). A property retains one successful instance;
//! failing/discarded bodies poison it. Node currently creates a fresh
//! instance for every body.

pub(crate) mod case;
pub(crate) mod files;
mod json;
pub(crate) mod property;
pub(crate) mod provider;
pub(crate) mod runner;
pub(crate) mod state;
#[cfg(test)]
mod tests;

pub use case::{BodyResult, CaseRun, PanicFrame, PanicInfo, Recorded, Snapshot, TestCase};
pub use runner::{BodyRunner, execute_case};
pub use state::State;
