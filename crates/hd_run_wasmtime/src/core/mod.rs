//! Random, Sys and Process providers using scoped grants and native I/O.

pub(crate) mod pipes;
pub(crate) mod process;
pub(crate) mod random;
pub(crate) mod sys;
#[cfg(test)]
mod tests;
pub(crate) mod workers;

#[cfg(all(test, unix))]
use pipes::{ReadEvent, Spawned, drain, feed_input, spawn_process, wait_pipe};
#[cfg(test)]
use process::process;
pub use process::process_with_deadline;
pub(crate) use process::process_with_deadline_at;
pub use random::random;
pub use sys::sys;
#[cfg(test)]
use sys::sys_read;
#[cfg(test)]
use workers::run_process;
#[cfg(all(test, windows))]
use workers::run_process_with_limit_and_deadline;
#[cfg(all(test, unix))]
use workers::{
    PipeWorkers, run_process_at, run_process_with_limit, run_process_workers,
    wait_process_and_pipes,
};
