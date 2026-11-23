# Sync

Status: standard library specification draft.

This chapter defines `std.sync`, shared ownership across threads. It is
ordinary std code over the [atomic intrinsics](../lang/10-modules.md#r-module.rt.memory.atomic)
of `std.rt.memory`. The compiler's own counts are never atomic
([`module.marker.send.counts`](../lang/10-modules.md#r-module.marker.send.counts)),
so a value that several threads own at once is held by an `Arc`.

## Arc

An `Arc[T]` owns a `T` that several threads may hold. Its counts change
by atomic operations:

```hd
use std.sync.Arc

fn shared() -> usize:
    first := Arc::new(41)
    second := first.clone()
    *second.get() + Arc::strong_count(&first)
```

1. r[std-sync.arc.new] `Arc::new(value)` returns a new `Arc[T]` that owns `value`, with a strong count of one.
2. r[std-sync.arc.clone] `arc.clone()` returns another `Arc[T]` that owns the same value, and adds one to its strong count.
3. r[std-sync.arc.drop] Dropping an `Arc[T]` subtracts one from the strong count. The drop that ends the last strong count drops the value. The room for the value is freed when the last `Arc` or `Weak` that refers to it is dropped.
4. r[std-sync.arc.get] `arc.get()` returns a `&T` to the owned value.
5. r[std-sync.arc.counts] `Arc::strong_count(&arc)` returns how many `Arc`s own the value, and `Arc::weak_count(&arc)` how many `Weak`s refer to it, as `usize`. Another thread may change either at any time.
6. r[std-sync.arc.ptr-eq] `Arc::ptr_eq(&a, &b)` is true when `a` and `b` own the same value.
7. r[std-sync.arc.get-mut] `Arc::get_mut(&mut arc)` returns `.Some` of a `&mut T` to the value when no other `Arc` or `Weak` refers to it, and `.None` otherwise.
8. r[std-sync.arc.try-unwrap] `Arc::try_unwrap(arc)` returns `.Ok` of the value when `arc` is its only `Arc`, which ends the value's strong count. Otherwise it returns `.Err(arc)`.
9. r[std-sync.arc.not-copy] `Arc[T]` is not `Copy`. A copy would share the count without adding to it.
10. r[std-sync.arc.send] `Arc[T]` is `Send` and `Sync` when `T` is both, by explicit implementations ([`module.marker.explicit.only`](../lang/10-modules.md#r-module.marker.explicit.only)). It is never `Wire`.

> **Why.** This is Rust's `alloc::sync::Arc`. A clone adds with `Relaxed`
> ordering, a drop subtracts with `Release`, and only the last drop fences
> with `Acquire` before it drops the value. Every Wasm atomic is
> sequentially consistent, so the orderings are one here. `get_mut`
> tests uniqueness by locking the weak count. `Arc[T]` needs `T` to be
> `Sync`, since every thread that holds it reads the same `T` at once, and
> `Send`, since any of them may drop it last.

## Weak

A `Weak[T]` refers to an `Arc`'s value without keeping it alive:

```hd
use std.sync.Arc

fn outlived() -> bool:
    first := Arc::new(7)
    weak := Arc::downgrade(&first)
    before := weak.upgrade().is_some()
    _ := first
    before && weak.upgrade().is_none()
```

1. r[std-sync.weak.downgrade] `Arc::downgrade(&arc)` returns a `Weak[T]` to the value, and adds one to its weak count.
2. r[std-sync.weak.upgrade] `weak.upgrade()` returns `.Some` of a new `Arc[T]` while an `Arc` owns the value, and `.None` after the value was dropped.
3. r[std-sync.weak.clone] `weak.clone()` returns another `Weak[T]` and adds one to the weak count. Dropping a `Weak[T]` subtracts one.
4. r[std-sync.weak.send] `Weak[T]` is not `Copy`. It is `Send` and `Sync` when `T` is both, as `Arc[T]` is, and never `Wire`.
