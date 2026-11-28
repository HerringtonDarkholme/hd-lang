# Arena

Status: standard library specification draft.

This chapter defines `std.arena`, an untyped arena: values of any types
that live and die together, as Rust's `bumpalo::Bump`. It is ordinary std
code over the [typed memory](../lang/10-modules.md#typed-memory) of
`std.rt.memory`.

## Arena

An `Arena` hands out values that live while it is lent, and drops them
all at once:

```hd
use std.arena.Arena

struct Account:
    owner: string
    balance: i64

fn open(bank: &Arena, owner: string, balance: i64) -> &mut Account:
    bank.alloc(Account { owner: owner, balance: balance })

fn settle() -> i64:
    bank := Arena::new()
    alice := open(&bank, "alice", 100)
    bob := open(&bank, "bob", 5)
    alice.balance -= 30
    bob.balance += 30
    alice.balance + bob.balance
```

1. r[std-arena.type] `Arena` is an arena of values of any types. It is not `Copy`, and neither `Send` nor `Sync`.
2. r[std-arena.new] `Arena::new()` returns an empty arena.
3. r[std-arena.alloc] `arena.alloc(val)` moves `val` into the arena and returns a `&mut T` to it, where `T` is `val`'s type. The value stays at that place until the arena drops it.
4. r[std-arena.alloc.disjoint] Each `alloc` returns a value that no other result of the same arena refers to, so several results may be live and written at once.
5. r[std-arena.alloc.lent] An `alloc` result holds the arena shared ([`ref.excl.call`](../lang/13-values-and-references.md#r-ref.excl.call)): while it is used, the arena is not reset, moved or dropped, and the result never outlives it. Error: `conflicting-access`, or `local-reference-escapes` where it would outlive the arena.
6. r[std-arena.reset] `arena.reset()` drops every value in the arena, newest first, and keeps its newest chunk of memory for later values.
7. r[std-arena.drop] Dropping an arena drops every value in it, newest first, and frees its memory.
8. r[std-arena.no-count] An `alloc` result counts nothing: the arena keeps no count of a value, and a value's own counted parts are released when the arena drops it.

> **Why.** This is bumpalo's API: `alloc(&self, val: T) -> &mut T` and
> `reset(&mut self)`. A result keeps the arena shared, so many are live at
> once and `reset` waits for them, as Rust's borrow checker does. Only std
> can make that `&mut` from a shared receiver, through `Ptr.as_mut`
> ([`module.rt.ptr.as-ref`](../lang/10-modules.md#r-module.rt.ptr.as-ref)).
> A value whose type has drop work gets an entry in the arena, and one
> without gets none ([`module.rt.ptr.needs-drop`](../lang/10-modules.md#r-module.rt.ptr.needs-drop)).
