# `std.thread`

Generated from `hd doc std.thread`. Run `hd doc std.thread.NAME` in a package for one item's full text.

## `Scope`

```text
pub struct Scope['s]
```

The threads of one `scope` call, whose closures may borrow for `'s`.

- `pub fn spawn[T < ?Copy & Send](&mut self, code: fn() -> T + Send & 's) -> Handle[T]`: Starts a thread that runs `code`, which may borrow for `'s`, and gives its handle.

## `Handle`

```text
pub struct Handle[T < ?Copy & Send]
```

A thread a `Scope` started, which gives the value its closure returned.

- `pub fn join(self) -> T`: Waits for the thread to end and gives the value its closure returned.

## `scope`

```text
pub fn scope['s, R < ?Copy](code: fn(&mut Scope['s]) -> R) -> R
```

Calls `code` with a new `Scope`, then waits for every thread it started, and gives what `code` returned.

## `background`

```text
pub fn background![T < ?Copy & Send](code: fn() -> T + Send) -> T
```

Runs `code` on a new thread and gives the value it returned: the task waits for the thread, not the thread it runs on. A cancel returns at once, and the thread drops the value when it ends.
