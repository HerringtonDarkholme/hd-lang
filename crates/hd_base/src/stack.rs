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
