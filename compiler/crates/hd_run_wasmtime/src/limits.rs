//! A store's combined linear-memory and GC-heap backing-byte budget.
//!
//! Wasmtime 49.0.2 routes both initial GC memory and GC memory growth through
//! `ResourceLimiter::memory_growing`, before allocating the backing memory.
//! This counts accessible backing bytes, including both copying semispaces,
//! not reserved address space, live objects, or the host's runtime metadata.
//! Configure the engine's initial GC heap to zero for a bounded store: a fixed
//! 64 MiB initial heap otherwise prevents small allocations under small caps.
//! Growth still follows Wasmtime's page rounding and geometric GC policy.

use wasmtime::{Error, GcHeapOutOfMemory, OutOfMemory, ResourceLimiter, Trap};

/// A failed allocation or a request exceeding the store's backing-byte cap.
#[derive(Debug)]
pub(crate) struct HeapExhausted;

impl std::fmt::Display for HeapExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the program heap cannot allocate more memory")
    }
}

impl std::error::Error for HeapExhausted {}

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
    error.is::<HeapExhausted>()
        || error.is::<GcHeapOutOfMemory<()>>()
        || error.is::<OutOfMemory>()
        || error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::OutOfMemory)
        || matches!(error.downcast_ref::<Trap>(), Some(Trap::AllocationTooLarge))
}

/// Installed before instantiation, so initial memories count against the cap.
pub(crate) struct HeapBudget {
    max_bytes: u64,
    charged: u64,
    last_reservation: u64,
}

impl HeapBudget {
    pub(crate) fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            charged: 0,
            last_reservation: 0,
        }
    }
}

impl ResourceLimiter for HeapBudget {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Each new callback commits the preceding successful reservation.
        // Failed growth reports synchronously before any following callback.
        self.last_reservation = 0;
        let delta = desired.checked_sub(current).ok_or(HeapExhausted)? as u64;
        let total = self.charged.checked_add(delta).ok_or(HeapExhausted)?;
        if total > self.max_bytes {
            return Err(HeapExhausted.into());
        }
        self.charged = total;
        self.last_reservation = delta;
        Ok(true)
    }

    fn memory_grow_failed(&mut self, error: Error) -> wasmtime::Result<()> {
        // Pinned Wasmtime 49.0.2 has one pre-callback failure path:
        // runtime/vm/memory.rs:643-646 rejects an unrepresentable custom-page
        // memory size without calling memory_growing. Its untyped error is
        // distinct from every post-callback failure (same file:723). Do not
        // roll back the previous SUCCESSFUL allocation on that path. This
        // distinction must be re-audited if the exact Wasmtime pin changes.
        if error.to_string() != "memory growth exceeds memory type's limits" {
            self.charged -= self.last_reservation;
        }
        self.last_reservation = 0;
        Err(error.context(HeapExhausted))
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Wasmtime's tables and runtime metadata are outside this backing-
        // memory budget; retain the engine profile's existing table behavior.
        Ok(true)
    }
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
