//! Which engine errors mean the program heap could not allocate, so the
//! program panics `heap-exhausted` (`flow.panic.heap-exhausted`). The host
//! sets no heap limit of its own: the engine's memory reservations and the
//! machine are the limit.

use wasmtime::{Error, OutOfMemory, Trap};

/// Whether an engine error denotes a failed program-heap allocation.
///
#[must_use]
pub fn is_heap_exhausted(error: &Error) -> bool {
    // Initial Unix backing allocation fails outside memory_grow_failed;
    // Wasmtime's mmap implementation propagates rustix's concrete errno.
    #[cfg(unix)]
    if error.downcast_ref::<rustix::io::Errno>() == Some(&rustix::io::Errno::NOMEM) {
        return true;
    }
    error.is::<OutOfMemory>()
        || error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::OutOfMemory)
        || matches!(error.downcast_ref::<Trap>(), Some(Trap::AllocationTooLarge))
}
