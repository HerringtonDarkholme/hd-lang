mod tests {
    use hd_run_wasmtime::internals::limits::is_heap_exhausted;

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
            .context("initial linear backing allocation");
        assert!(is_heap_exhausted(&error));
        let permission_denied =
            wasmtime::Error::from(rustix::io::Errno::PERM).context("a different OS failure");
        assert!(!is_heap_exhausted(&permission_denied));
    }
}
