//! Which engine errors mean the program heap could not allocate, so the
//! program panics `heap-exhausted` (`flow.panic.heap-exhausted`). The host
//! sets no heap limit of its own: the engine's memory reservations and the
//! machine are the limit.

use wasmtime::{Error, GcHeapOutOfMemory, OutOfMemory, Trap};

/// Whether an engine error denotes a failed program-heap allocation.
///
/// GC may swallow a refused expansion, collect, and successfully retry the
/// allocation within its existing capacity. Only an actual propagated error,
/// not the earlier refusal, makes the program panic.
pub(crate) fn is_heap_exhausted(error: &Error) -> bool {
    // Initial Unix backing allocation fails outside memory_grow_failed;
    // Wasmtime's mmap implementation propagates rustix's concrete errno.
    #[cfg(unix)]
    if error.downcast_ref::<rustix::io::Errno>() == Some(&rustix::io::Errno::NOMEM) {
        return true;
    }
    error.is::<GcHeapOutOfMemory<()>>()
        || error.is::<OutOfMemory>()
        || error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::OutOfMemory)
        || matches!(error.downcast_ref::<Trap>(), Some(Trap::AllocationTooLarge))
}

#[cfg(test)]
mod tests {
    use super::is_heap_exhausted;

    #[test]
    fn heap_failure_classification_preserves_typed_errors_through_context() {
        let out_of_memory = wasmtime::Error::from(wasmtime::OutOfMemory::new(100))
            .context("initial heap allocation");
        assert!(is_heap_exhausted(&out_of_memory));
        let os_out_of_memory =
            wasmtime::Error::from(std::io::Error::from(std::io::ErrorKind::OutOfMemory))
                .context("initial OS allocation");
        assert!(is_heap_exhausted(&os_out_of_memory));
        assert!(is_heap_exhausted(
            &wasmtime::Trap::AllocationTooLarge.into()
        ));
        assert!(!is_heap_exhausted(
            &wasmtime::Trap::UnreachableCodeReached.into()
        ));
        assert!(!is_heap_exhausted(&wasmtime::Error::msg(
            "unrelated engine error"
        )));
    }

    #[cfg(unix)]
    #[test]
    fn initial_unix_mmap_oom_is_a_heap_failure_without_string_matching() {
        let error = wasmtime::Error::from(rustix::io::Errno::NOMEM)
            .context("initial linear or GC backing allocation");
        assert!(is_heap_exhausted(&error));
        let permission_denied =
            wasmtime::Error::from(rustix::io::Errno::PERM).context("a different OS failure");
        assert!(!is_heap_exhausted(&permission_denied));
    }
}
