//! Synchronous filesystem providers with native errors and scoped grants.
//! Grants are checked against resolved paths before any filesystem access.
//! Limited Unix calls use nonblocking descriptors and deadline-bounded FIFO
//! polling. Ordinary filesystem syscalls (including network filesystem access)
//! may still block inside the kernel; safe std does not cancel them. Limited
//! Windows named-pipe calls poll immediate I/O without background workers.
//! No mutation is delegated to a worker that could outlive the deadline.

pub(crate) mod api;
/// Unix FIFO deadline coverage; every item is unix-only, like its tests.
#[cfg(all(test, unix))]
pub(crate) mod fifo;
pub(crate) mod io;
pub(crate) mod ops;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use api::{READ, WRITE, read_file_until, write_file_until};
pub use api::{call, call_until, call_until_at};
