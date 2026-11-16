//! Work counters for resident suspension-frame lowering (#1898).
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default)]
pub struct Work {
    pub instructions: u64,
    pub accesses: u64,
    pub slots: u64,
    pub points: u64,
    pub dispatch: u64,
}

thread_local! {
    static WORK: Cell<Option<Work>> = const { Cell::new(None) };
}

pub fn count<R>(run: impl FnOnce() -> R) -> (R, Work) {
    struct Restore(Option<Work>);
    impl Drop for Restore {
        fn drop(&mut self) {
            WORK.set(self.0);
        }
    }
    let restore = Restore(WORK.replace(Some(Work::default())));
    let result = run();
    let work = WORK.get().unwrap_or_default();
    drop(restore);
    (result, work)
}

pub(crate) fn add(
    instructions: usize,
    accesses: usize,
    slots: usize,
    points: usize,
    dispatch: usize,
) {
    WORK.with(|cell| {
        if let Some(mut work) = cell.get() {
            work.instructions += instructions as u64;
            work.accesses += accesses as u64;
            work.slots += slots as u64;
            work.points += points as u64;
            work.dispatch += dispatch as u64;
            cell.set(Some(work));
        }
    });
}
