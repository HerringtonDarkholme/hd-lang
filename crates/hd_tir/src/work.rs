//! Opt-in counts of owned-place verifier queries, for generated scale checks.
//! Off: one global signal check and one thread-local cell read per charge.
//! On: constant work per charge; one optional counter per thread, with no
//! state proportional to program size. Nested measurements restore callers.
use std::cell::Cell;
use std::thread::LocalKey;
thread_local! {
    static EMPTY_PLACES: Cell<Option<u64>> = const { Cell::new(None) };
    static MATCH_READS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Runs `run` with `key` counting from zero, then restores the caller's.
fn measure<R>(key: &'static LocalKey<Cell<Option<u64>>>, run: impl FnOnce() -> R) -> (R, u64) {
    struct Restore(&'static LocalKey<Cell<Option<u64>>>, Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            self.0.with(|work| work.set(self.1));
        }
    }
    let restore = Restore(key, key.with(|work| work.replace(Some(0))));
    let result = run();
    let count = key.with(|work| work.get().unwrap_or(0));
    drop(restore);
    (result, count)
}

fn charge(key: &'static LocalKey<Cell<Option<u64>>>, n: usize) {
    hd_base::work::add(hd_base::work::Counter::UseFacts, n as u64);
    key.with(|work| {
        if let Some(count) = work.get() {
            work.set(Some(count + n as u64));
        }
    });
}

/// Count actual place candidates and state words examined on this thread.
pub fn count_empty_place_work<R>(run: impl FnOnce() -> R) -> (R, u64) {
    measure(&EMPTY_PLACES, run)
}

pub(crate) fn empty_place_work(n: usize) {
    charge(&EMPTY_PLACES, n);
}

/// Count the blocks, reads and tour steps of the match-reference read
/// queries ([`crate::own::derive`]'s `MatchReads`) on this thread.
pub fn count_match_read_work<R>(run: impl FnOnce() -> R) -> (R, u64) {
    measure(&MATCH_READS, run)
}

pub(crate) fn match_read_work(n: usize) {
    charge(&MATCH_READS, n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_measurements_restore_the_outer_count_and_disabled_work_is_ignored() {
        empty_place_work(20);
        let ((), outer) = count_empty_place_work(|| {
            empty_place_work(2);
            let ((), inner) = count_empty_place_work(|| empty_place_work(7));
            assert_eq!(inner, 7);
            empty_place_work(3);
        });
        assert_eq!(outer, 5);
        empty_place_work(20);
        assert_eq!(count_empty_place_work(|| ()).1, 0);
    }
}
