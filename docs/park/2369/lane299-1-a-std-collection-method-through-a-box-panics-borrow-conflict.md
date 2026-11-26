# Nested `@inline(.Always)` methods through a `*mut` panic borrow-conflict

Rule: `ref.ptr.loan` (a reference through `*T` is a local loan); an
inlined method's accesses through its `self` are the caller's loan's
own (the inlined-parameter key, `crates/hd_wasm/src/emit/borrow.rs`,
`loan-lowering/an-inlined-mut-self-call-shares-its-callers-loan-key`).

Today, at 73270e86c (before the switch, no list involved):

```hd
use std.meta.inline

struct C:
    v: i32

impl C:
    @inline(.Always)
    fn get(&self) -> i32:
        self.v

    @inline(.Always)
    fn bump(&mut self) -> void:
        if self.get() > 0:
            self.v += 1

pub fn main() -> void $ Console:
    c := box(C { v: 1 })
    c.bump()
    println("${c.v}")
```

panics `borrow-conflict: a h.C a live reference borrows was read through
another *T` at `get` inlined in `bump` inlined in `main`, in dev. Without
the two `@inline(.Always)` it prints 2. One inlined level works (the stage
case above); a method inlined inside an inlined method does not.

Effect on #2369: every std `List` method is `@inline(.Always)` and calls
others (`index_set` -> `set` -> `len`), so `items[1] = 9` on a `box`ed
list panics; 17 of the 20 runtime conformance failures of the lane 299
switch WIP are this (`index-traits-run`, `owned-temporary-collection-loops`,
the `iterator-*` and `adapters` cursors under `box(..).by_ref()`,
`readonly-element-calls-preserve-cow-snapshots`, the
`pointer-loan-bang-ref-list-read*` group, ...). PlainList at 73270e86c
panics the same way.

Tried (lane 299, all reverted, no change): in `lent_params`, marking an
inlined parameter stored once from a lent parameter's whole read, its
`&*x` reborrow, or a `Coerce` of either; in `crossings_of`, keying a
`[*%r ..]` crossing through the local place `%r` reborrows.

Where: `crates/hd_wasm/src/emit/borrow.rs` (`lent_params`,
`CaptureOrigins`, `crossings_of`, `borrow_check`), the inliner's
parameter stores in `crates/hd_opt`.

Fix: read the inlined TIR of `main` (the second level's `self` store and
the `Read [.. .v]` place) and give the inner copy the outer copy's key,
as the one-level rule does.

Done: the repro prints 2 in dev and release; a `loan-lowering` stage
case with two inlined levels holds it; the 17 conformance cases pass on
the switch branch.
