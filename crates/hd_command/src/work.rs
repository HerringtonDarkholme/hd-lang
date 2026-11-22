//! `HD_WORK=FILE`: the work report of the generated scale check (#1880,
//! `bench/metrics/scripts/scale.ts`). With the variable set, `hd` turns on
//! `hd_base::work` before the command and writes one JSON object of each
//! counter's total to FILE when the command ends. Without it, nothing is
//! counted and nothing is written.
//!
//! The allocator below counts allocations and their bytes into that
//! signal: a clone, rebuild or format per item shows up as allocation
//! work even where no named counter sits.

use std::alloc::{GlobalAlloc, Layout};

use mimalloc::MiMalloc;

use hd_base::work::{Counter, add};

/// mimalloc, counting each allocation and reallocation and its bytes while
/// the work signal is on. Cost while off: one relaxed load and a branch
/// per allocation. Against the system allocator, a cold check at 2,000
/// files uses 11% less CPU (#1802 draft 07).
struct Counting;

// SAFETY: every method forwards to `MiMalloc` with its own arguments, so
// its guarantees hold; the count touches only atomics and never
// allocates.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract for `alloc` is `MiMalloc`'s.
        unsafe { MiMalloc.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: as `alloc`.
        unsafe { MiMalloc.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `MiMalloc` with `layout`.
        unsafe { MiMalloc.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: as `dealloc`, and the caller's `new_size` contract.
        unsafe { MiMalloc.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count(bytes: usize) {
    add(Counter::Allocs, 1);
    add(Counter::AllocBytes, bytes as u64);
}

/// Turns the signal on when `HD_WORK` names a file, and returns the file.
pub(crate) fn start() -> Option<std::ffi::OsString> {
    let file = std::env::var_os("HD_WORK").filter(|f| !f.is_empty())?;
    hd_base::work::start();
    #[cfg(feature = "plant-quadratic")]
    plant::run();
    Some(file)
}

/// Writes the report to `file`, as `{"allocs":N,...}`. A write that fails
/// is reported on stderr and does not change the exit status.
pub(crate) fn finish(file: Option<std::ffi::OsString>) {
    let Some(file) = file else { return };
    let totals = hd_base::work::totals();
    let fields: Vec<String> = totals
        .iter()
        .map(|(name, n)| format!("\"{name}\":{n}"))
        .collect();
    let text = format!("{{{}}}\n", fields.join(","));
    if let Err(error) = std::fs::write(&file, text) {
        eprintln!("HD_WORK: cannot write {}: {error}", file.to_string_lossy());
    }
}

/// The scale check's self-test (`cargo build --release -p hd_command
/// --features plant-quadratic`): a quadratic loop over the lines of the
/// package's sources, hashing every pair, which the check must flag on
/// any shape. It runs only with `HD_WORK` set.
#[cfg(feature = "plant-quadratic")]
mod plant {
    use std::hash::{BuildHasher as _, Hash as _, Hasher as _};

    pub(super) fn run() {
        let mut lines = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from("src")];
        while let Some(dir) = dirs.pop() {
            // cost: one listing per folder of the package's `src/`, in the
            // self-test build only; the pair loop below is the planted n².
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if let Ok(text) = std::fs::read_to_string(&path) {
                    lines.extend(text.lines().map(str::to_owned));
                }
            }
        }
        let state = hd_base::FixedState::default();
        let mut sink = 0u64;
        for a in &lines {
            for b in &lines {
                let mut h = state.build_hasher();
                a.hash(&mut h);
                b.hash(&mut h);
                sink ^= h.finish();
            }
        }
        std::hint::black_box(sink);
    }
}

/// The disk cache as a command uses it, with the store's own work kept out
/// of the counters ([`hd_base::work::uncounted`]): each read, write and
/// publication, and what its drop does. That work is not a function of the
/// program: packs are named by the clock and the process id, so a shape
/// counted a few dozen allocations more in one run than the next (#1969).
/// What a command does with the bytes, decoding and checking, stays
/// counted. Cost: two relaxed atomic stores per store call.
pub(crate) struct Store(Option<hd_cache::disk::DiskStore>);

impl Store {
    pub(crate) fn new(store: hd_cache::disk::DiskStore) -> Self {
        Store(Some(store))
    }
}

impl std::ops::Deref for Store {
    type Target = hd_cache::disk::DiskStore;
    fn deref(&self) -> &Self::Target {
        // ruled out: the store is taken only by `drop`.
        self.0.as_ref().expect("the store until its drop")
    }
}

impl hd_cache::CacheStore for Store {
    fn get(
        &self,
        kind: hd_cache::EntryKind,
        key: &hd_base::Hash128,
    ) -> Option<std::sync::Arc<[u8]>> {
        hd_base::work::uncounted(|| (**self).get(kind, key))
    }
    fn put(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128, bytes: &[u8]) {
        hd_base::work::uncounted(|| (**self).put(kind, key, bytes));
    }
    fn touch(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128) {
        hd_base::work::uncounted(|| (**self).touch(kind, key));
    }
    fn put_many(&self, entries: &[(hd_cache::EntryKind, hd_base::Hash128, std::sync::Arc<[u8]>)]) {
        hd_base::work::uncounted(|| (**self).put_many(entries));
    }
    fn wanted(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128) -> bool {
        (**self).wanted(kind, key)
    }
    fn flush_requested(&self) -> bool {
        (**self).flush_requested()
    }
    fn get_claimed(
        &self,
        kind: hd_cache::EntryKind,
        key: &hd_base::Hash128,
    ) -> Option<std::sync::Arc<[u8]>> {
        hd_base::work::uncounted(|| (**self).get_claimed(kind, key))
    }
    fn abandon(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128) {
        hd_base::work::uncounted(|| (**self).abandon(kind, key));
    }
    fn claims(&self, kind: hd_cache::EntryKind, key: &hd_base::Hash128) -> bool {
        (**self).claims(kind, key)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let store = self.0.take();
        hd_base::work::uncounted(|| drop(store));
    }
}
