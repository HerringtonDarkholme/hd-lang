# A std list's index panic is a call_indirect with two messages

Rule: #2369 step 3's WAT identity (SWITCH.md risk 1).

Today: sieve's strided loop (`a-strided-write-loop-keeps-its-length-in-a-local`)
is the builtin's instruction for instruction except the out-of-bounds
arm: the builtin calls `rt:panic`; the std list pushes four string words
and an index and does `call_indirect` (`panic_with("index-out-of-bounds",
"list index out of bounds")`). Cold path, but more bytes per bounds
test site.

Where: `lib/std/collections/plain_list.hd` (`index`, `index_mut`,
`index_set`, `get`, `set`), `std.rt.panic_with`'s lowering.

Fix: decide at step 1 whether the std message stays (it is the better
diagnostic) and price it in bench sizes; or give `panic_with` with
constant arguments a shared out-of-line stub per message pair.

Done: bench sizes before and after step 1 state the cost, and the
choice is recorded in SWITCH.md.
