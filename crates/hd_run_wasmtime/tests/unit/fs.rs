pub(crate) mod api;
/// Special-file coverage; every item is unix-only, like its tests.
#[cfg(unix)]
pub(crate) mod fifo;
pub(crate) mod io;
pub(crate) mod ops;
pub(crate) mod sandbox;
pub(crate) mod sandbox_tests;
pub(crate) mod test_support;
