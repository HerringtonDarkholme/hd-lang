//! The process-wide runtime that offloaded host operations run on
//! One current-thread tokio runtime, driven
//! by `Runtime::block_on` on a dedicated `hd-io` thread, since only
//! `block_on` drives a current-thread runtime's I/O and timer drivers.
//! Program threads spawn tasks through its `Handle`; each task delivers
//! its result into its instance's reactor inbox (`reactor.rs`).
//!
//! It starts lazily, as the `hd-epochs` ticker does (`deadline.rs`): the
//! first offloaded operation starts it, so a program that makes none
//! starts no thread.

use std::sync::OnceLock;

use tokio::runtime::Handle;

static RUNTIME: OnceLock<Result<Handle, String>> = OnceLock::new();

/// Whether this process has started the `hd-io` runtime.
#[must_use]
pub fn io_started() -> bool {
    RUNTIME.get().is_some()
}

/// The runtime's handle, starting its thread on the first call.
pub(crate) fn handle() -> Result<&'static Handle, String> {
    RUNTIME
        .get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .map_err(|error| format!("the I/O runtime cannot start: {error}"))?;
            let handle = runtime.handle().clone();
            std::thread::Builder::new()
                .name("hd-io".into())
                .spawn(move || runtime.block_on(std::future::pending::<()>()))
                .map_err(|error| format!("the I/O runtime cannot start: {error}"))?;
            Ok(handle)
        })
        .as_ref()
        .map_err(Clone::clone)
}
