//! `HD_WORK=FILE`: the work report of the generated scale check (#1880,
//! `bench/metrics/scripts/scale.ts`). With the variable set, `hd` turns on
//! `hd_base::work` before the command and writes one JSON object of each
//! counter's total to FILE when the command ends. Without it, nothing is
//! counted and nothing is written.
//!
//! The allocator below counts allocations and their bytes into that
//! signal: a clone, rebuild or format per item shows up as allocation
//! work even where no named counter sits.

use std::alloc::{GlobalAlloc, Layout, System};

use hd_base::work::{Counter, add};

/// The system allocator, counting each allocation and reallocation and
/// its bytes while the work signal is on. Cost while off: one relaxed
/// load and a branch per allocation.
struct Counting;

// SAFETY: every method forwards to `System` with its own arguments, so
// `System`'s guarantees hold; the count touches only atomics and never
// allocates.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: the caller's contract for `alloc` is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: as `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System` with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        // SAFETY: as `dealloc`, and the caller's `new_size` contract.
        unsafe { System.realloc(ptr, layout, new_size) }
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

/// The scale check's self-test (`cargo build --release -p hd_cli
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
