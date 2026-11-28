//! A walk over input-shaped structure on an explicit stack, so the native
//! stack stays constant at any depth the input reaches (AGENTS.md
//! "Performant"; `limits::TYPE_DEPTH` bounds the work, not the stack).

/// Runs `step` on `first` and then on each item it pushes, the last pushed
/// first, until a step returns false or no item is left; true when none
/// returned false. A walk whose order is visible pushes an item's parts in
/// reverse, so they are entered in order, and pushes work for after the
/// parts as an item under them. Cost: one step per item pushed.
pub fn all<T>(first: T, mut step: impl FnMut(T, &mut Vec<T>) -> bool) -> bool {
    let mut stack = vec![first];
    while let Some(item) = stack.pop() {
        if !step(item, &mut stack) {
            return false;
        }
    }
    true
}

/// `make`'s value on the heap, built in this function's own frame: a
/// large value (the checker's `Ck`, the driver's `Run`) never sits in the
/// frame of a caller that runs deep work below it; the caller keeps one
/// pointer. A thread's stack pays for the frames on its deepest path
/// (#2423). Cost: one allocation.
#[inline(never)]
pub fn on_heap<T>(make: impl FnOnce() -> T) -> Box<T> {
    // Allocated first, so the value is written once, not built and then
    // moved: `Box::new(make())` held two copies in this frame.
    Box::write(Box::new_uninit(), make())
}
