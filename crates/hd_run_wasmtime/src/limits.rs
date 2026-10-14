//! A program's heap budget (`cli.limit.heap`): its store's combined
//! linear-memory and GC-heap backing bytes, and the data its host
//! operations hold (`std-host.op.heap`), count against one `Ledger`.
//!
//! Wasmtime 49.0.2 routes both initial GC memory and GC memory growth through
//! `ResourceLimiter::memory_growing`, before allocating the backing memory.
//! This counts accessible backing bytes, including both copying semispaces,
//! not reserved address space, live objects, or the host's runtime metadata.
//! Configure the engine's initial GC heap to zero for a bounded store: a fixed
//! 64 MiB initial heap otherwise prevents small allocations under small caps.
//! Growth still follows Wasmtime's page rounding and geometric GC policy.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The heap budget of one program: its store's memory and the data its
/// host operations hold count against one maximum (`std-host.op.heap`).
/// Tasks on `hd-io` and the blocking pool charge it as the store does, so
/// the count is atomic.
#[derive(Debug)]
pub(crate) struct Ledger {
    max_bytes: u64,
    charged: AtomicU64,
}

impl Ledger {
    pub(crate) fn new(max_bytes: u64) -> Arc<Ledger> {
        Arc::new(Ledger {
            max_bytes,
            charged: AtomicU64::new(0),
        })
    }

    /// Counts `bytes` more, unless that passes the maximum.
    fn charge(&self, bytes: u64) -> Result<(), HeapExhausted> {
        self.charged
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |charged| {
                charged
                    .checked_add(bytes)
                    .filter(|total| *total <= self.max_bytes)
            })
            .map(drop)
            .map_err(|_| HeapExhausted)
    }

    fn release(&self, bytes: u64) {
        self.charged.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// A reservation of `bytes`, for data a host operation is about to hold.
    pub(crate) fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, HeapExhausted> {
        let mut held = Reservation::empty(self);
        held.grow(bytes)?;
        Ok(held)
    }

    /// The bytes counted now, the store's memory included.
    #[cfg(test)]
    pub(crate) fn charged(&self) -> u64 {
        self.charged.load(Ordering::Relaxed)
    }
}

/// Bytes of a ledger that a host operation holds data in. Dropping it
/// releases them: when `.finish` takes the data, just before the copy into
/// the exchange buffer that the store's memory growth counts, or when the
/// operation is aborted.
#[derive(Debug)]
pub(crate) struct Reservation {
    ledger: Arc<Ledger>,
    bytes: u64,
}

impl Reservation {
    pub(crate) fn empty(ledger: &Arc<Ledger>) -> Reservation {
        Reservation {
            ledger: Arc::clone(ledger),
            bytes: 0,
        }
    }

    /// Holds `bytes` more, or fails and holds what it held.
    pub(crate) fn grow(&mut self, bytes: usize) -> Result<(), HeapExhausted> {
        let bytes = bytes as u64;
        self.ledger.charge(bytes)?;
        self.bytes += bytes;
        Ok(())
    }

    /// Releases `bytes` of what it holds.
    pub(crate) fn shrink(&mut self, bytes: usize) {
        let bytes = (bytes as u64).min(self.bytes);
        self.ledger.release(bytes);
        self.bytes -= bytes;
    }

    /// What it holds, in a new reservation; it is left holding nothing.
    pub(crate) fn take(&mut self) -> Reservation {
        Reservation {
            ledger: Arc::clone(&self.ledger),
            bytes: std::mem::take(&mut self.bytes),
        }
    }

    /// Takes over what `other`, a reservation of the same ledger, holds.
    pub(crate) fn absorb(&mut self, mut other: Reservation) {
        self.bytes += std::mem::take(&mut other.bytes);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.ledger.release(self.bytes);
    }
}

/// A store's resource limiter, installed before instantiation so initial
/// memories count against the ledger. Dropping it, with its store or at a
/// session's rebuild, releases the memory it charged.
pub(crate) struct HeapBudget {
    ledger: Arc<Ledger>,
    /// The bytes this store's memories charged.
    charged: u64,
    last_reservation: u64,
}

impl HeapBudget {
    pub(crate) fn new(ledger: Arc<Ledger>) -> Self {
        Self {
            ledger,
            charged: 0,
            last_reservation: 0,
        }
    }

    pub(crate) fn ledger(&self) -> &Arc<Ledger> {
        &self.ledger
    }

    /// What the ledger counts past this store's memory: the heap host
    /// operations hold. The test that reads it runs Unix programs.
    #[cfg(all(test, unix))]
    pub(crate) fn held(&self) -> u64 {
        self.ledger.charged() - self.charged
    }
}

impl Drop for HeapBudget {
    fn drop(&mut self) {
        self.ledger.release(self.charged);
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
        self.ledger.charge(delta)?;
        self.charged += delta;
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
            self.ledger.release(self.last_reservation);
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
