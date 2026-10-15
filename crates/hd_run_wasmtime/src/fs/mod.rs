//! Filesystem providers with native errors and scoped grants. Grants are
//! checked against resolved paths before any filesystem access, and under
//! a grant with scope entries every operation resolves under the handle of
//! a granted directory (`sandbox`), so a path cannot leave the grant
//! between the check and the open.
//!
//! A regular file or a directory runs inline on the program thread, with
//! the deadline checked before and after (`api::start`). A special file (a
//! FIFO, a device, a named pipe) can wait on another process, so its call
//! runs blocking `std::fs` on the `hd-io` blocking pool and `.finish`
//! delivers the result (`std-fs.special-file`). Aborting it only drops the
//! result: the spec gives a special file no deadline guarantee beyond "the
//! program is not blocked" (`std-fs.special-file.deadline`).

pub(crate) mod api;
/// Special-file coverage; every item is unix-only, like its tests.
#[cfg(all(test, unix))]
pub(crate) mod fifo;
pub(crate) mod io;
pub(crate) mod ops;
pub(crate) mod sandbox;
#[cfg(test)]
mod sandbox_tests;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use api::{READ, Start, WRITE, not_run, read_file_until, start, write_file_until};
pub(crate) use sandbox::Sandbox;
