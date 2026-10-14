//! Random, Sys and Process providers using scoped grants and native I/O.

pub(crate) mod process;
pub(crate) mod random;
pub(crate) mod sys;
#[cfg(test)]
mod tests;

pub use random::random;
pub use sys::sys;
#[cfg(test)]
use sys::sys_read;
