# Thread

Status: standard library specification draft.

This chapter defines `std.thread`, scoped threads on one heap. Each
thread runs a [`Send`](../lang/10-modules.md#r-module.marker.send)
crossing closure in another instance of the program, on the heap every
thread shares ([`module.rt.thread.heap`](../lang/10-modules.md#r-module.rt.thread.heap)).
It is ordinary std code over the
[thread intrinsics](../lang/10-modules.md#thread-intrinsics).

## Scope

`scope` runs a closure with a `Scope`, whose threads may borrow what
outlives the call, and waits for each of them before it returns:

```hd
use std.thread.{Scope, scope}

struct Point:
    x: i32
    y: i32

fn sum(point: &Point) -> i32:
    scope(fn(s: &mut Scope) -> i32:
        left := s.spawn(fn() -> i32: point.x * 2)
        right := s.spawn(fn() -> i32: point.y * 2)
        left.join() + right.join()
    )
```

1. r[std-thread.scope] `scope(code)` calls `code` with a new `Scope`, then waits until every thread that scope started has ended, and returns what `code` returned. The threads run at the same time as the caller and as each other.
2. r[std-thread.scope.stopped] When a thread of the scope stopped before its closure returned, by a panic or any other stop, `scope` panics once every thread has ended. Panic: `thread-stopped`.
3. r[std-thread.spawn] `s.spawn(code)` starts a thread that runs `code` and returns a `Handle[T]` to it. `code` is a crossing closure whose marker is `Send` ([Crossing Closures](../lang/07-functions.md#crossing-closures)), and `T` is `Send`.
4. r[std-thread.spawn.borrow] The closure may capture a reference that outlives the call of `scope`, since the scope's parameter is at that call's lifetime ([`fn.closure.lifetime.call`](../lang/07-functions.md#r-fn.closure.lifetime.call)). The reference is a [frozen loan](../lang/13-values-and-references.md#frozen-loans) until `scope` returns.
5. r[std-thread.spawn.here] On a host that starts no threads, `spawn` runs `code` on the calling thread before it returns.
6. r[std-thread.scope.not-copy] `Scope` and `Handle[T]` are not `Copy`. Only `scope` makes a `Scope`.

> **Why.** This is Rust's `std::thread::scope`. The scope waits for its
> threads whatever the closure did with their handles, so a borrow that
> a thread holds ends before `scope` returns. A closure that captures a
> `List` or a `string` by value owns its buffers
> ([`fn.crossing.mode.unique`](../lang/07-functions.md#r-fn.crossing.mode.unique));
> one that borrows reads only `Sync` data.

## Handle

A `Handle[T]` gives the value its thread's closure returned:

```hd
use std.thread.{Handle, Scope, scope}

fn later() -> usize:
    handle := scope(fn(s: &mut Scope) -> Handle[string]: s.spawn(fn() -> string: "done"))
    handle.join().len()
```

1. r[std-thread.join] `handle.join()` waits until the thread ends and returns the value its closure returned. When the thread stopped first, it panics. Panic: `thread-stopped`.
2. r[std-thread.handle.drop] Dropping a handle that was not joined drops the closure's value where the thread ends, and its scope still waits for the thread.
3. r[std-thread.handle.outlives] A handle may outlive its scope: the thread has then ended, and `join` returns its value at once.
4. r[std-thread.handle.send] `Handle[T]` is `Send`, by an explicit implementation ([`module.marker.explicit.only`](../lang/10-modules.md#r-module.marker.explicit.only)), and neither `Sync` nor `Wire`: it names a block of the shared heap, which another memory does not have.

## Background

`background!` runs a closure on a new thread and gives its value to the
task that awaits it, which suspends while the thread works, so the
task's own thread goes on with its other tasks:

```hd
use std.thread.background

fn total!(items: List[i64]) -> i64:
    background!(fn() -> i64:
        let sum: i64 = 0
        for item in items:
            sum = sum + item
        sum
    )
```

1. r[std-thread.background] `background!(code)` starts a thread that runs `code` and suspends until the thread ends, then returns the value `code` returned. `code` is a crossing closure whose marker is `Send`, and `T` is `Send`. Its captures move into the thread, so it borrows nothing.
2. r[std-thread.background.suspends] While the thread runs, the awaiting task is suspended, not its thread: the thread's other tasks run, and the host wakes the task when the thread ends, with no polling.
3. r[std-thread.background.stopped] When the thread stopped before `code` returned, by a panic or any other stop, `background!` panics. Panic: `thread-stopped`.
4. r[std-thread.background.cancel] Cancelling the task that awaits `background!` returns at once, without waiting for the thread. The thread runs on, and drops the value `code` returns when it ends.
5. r[std-thread.background.here] On a host that starts no threads, `background!` runs `code` on the calling thread, as `spawn` does ([`std-thread.spawn.here`](#r-std-thread.spawn.here)).

> **Why.** This is Kotlin's `withContext(Dispatchers.Default)` and
> Swift's `Task.detached` awaited: work that would block an event loop
> goes to a thread, and the loop runs on. Unlike `scope`, nothing waits
> for the thread when the task is cancelled, so the closure may not
> borrow: the thread outlives any place it could borrow from.

