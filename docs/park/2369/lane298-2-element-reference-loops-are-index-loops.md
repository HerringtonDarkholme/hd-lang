# Lower `for p in &xs` and `for p in &mut xs` as index loops

Rule: `flow.for.ref`, `flow.for.ref.conflict` (lane 298: the loop
borrows the whole list, as its element references do).

Today: `crates/hd_check/src/elem_refs.rs` `ref_loop` builds a loop of
`Ref [xs Elem i]` places over the builtin list, keyed by
`Indexed::List`. With the std `List` there is no element place.

Fix: since lane 298 made an element loan a borrow of the whole list,
the loop is exactly `n := xs.len(); for i in 0..n: p := &mut xs[i]`
through `IndexMut[usize]` (`&xs[i]` through `Index[usize]`) for any type
with `len()` and those traits. Lower it so in the checker, keep the
loop's one-access conflict (`Order::ref_loop`) on the desugared
references, and delete the list half of `elem_refs.rs` (the map half
goes at step 2).

Where: `crates/hd_check/src/elem_refs.rs`, `expr.rs` `iterate`,
`dep_results/constraints/exclusive.rs` `Order::ref_loop`.

Done: `for-loops` and `element-references` conformance pass in three
modes; `bench/micro/list_ref_*` and `list_for_borrow_*` keep their WAT
(the bounds test is decided by `i < n`, as `loop_loads` does for the
strided write loop).
