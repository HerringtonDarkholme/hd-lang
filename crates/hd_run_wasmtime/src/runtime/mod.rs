//! The plumbing every provider shares, independent of any one host trait:
//! the reactor of pending operations and their handles (`reactor.rs`),
//! the `hd-io` runtime that offloaded operations run on (`io.rs`), the
//! epoch ticker that turns wall deadlines into traps (`epochs.rs`), and
//! the native I/O error categories (`native_io.rs`).

pub(crate) mod epochs;
pub(crate) mod io;
pub(crate) mod native_io;
pub(crate) mod reactor;
