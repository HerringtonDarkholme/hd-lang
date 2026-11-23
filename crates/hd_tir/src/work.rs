//! Opt-in counts of owned-place verifier queries, for generated scale checks.
//! Off: one global signal check and one thread-local cell read per charge.
//! On: constant work per charge; one optional counter per thread, with no
//! state proportional to program size. Nested measurements restore callers.
use std::cell::Cell;
thread_local! {
    static EMPTY_PLACES: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Count actual place candidates and state words examined on this thread.
pub fn count_empty_place_work<R>(run: impl FnOnce() -> R) -> (R, u64) {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            EMPTY_PLACES.with(|work| work.set(self.0));
        }
    }
    let restore = Restore(EMPTY_PLACES.with(|work| work.replace(Some(0))));
    let result = run();
    let count = EMPTY_PLACES.with(|work| work.get().unwrap_or(0));
    drop(restore);
    (result, count)
}

pub(crate) fn empty_place_work(n: usize) {
    hd_base::work::add(hd_base::work::Counter::UseFacts, n as u64);
    EMPTY_PLACES.with(|work| {
        if let Some(count) = work.get() {
            work.set(Some(count + n as u64));
        }
    });
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
