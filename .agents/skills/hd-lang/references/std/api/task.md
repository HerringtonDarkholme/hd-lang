# `std.task`

Generated from `hd doc std.task`. Run `hd doc std.task.NAME` in a package for one item's full text.

## `Layer`

```text
pub trait Layer[W]
```

A configuration that wraps work and declares the resulting type.

## `apply`

```text
pub fn apply![T, U, $ R, $ S, P < Layer[fn!() -> T $ R, Wrapped = fn!() -> U $ S]](
```

Wraps work with a policy and invokes the returned function once.

## `race`

```text
pub fn race![T](tasks...: List[Suspend[T]]) -> T
```

Drives the tasks together and returns the first one's value; the losers are cancelled. A compiler intrinsic.

## `BlockOn`

```text
pub enum BlockOn[T]
```

How a `block_on` drive ended: `Completed` with the suspension's value, or `Cancelled` or `Abandoned` when the suspension was cancelled and its cleanup ran to its end or was cut short.

## `block_on`

```text
pub fn block_on[T](s: Suspend[T]) -> BlockOn[T]
```

Drives `s` from code that does not suspend, and returns how the drive ended. A compiler intrinsic.

## `CleanupEnd`

```text
pub enum CleanupEnd
```

How a job's cleanup ended: `Finished` when it ran to its end, `Abandoned` when a part of it was cut short.

## `Backoff`

```text
pub struct Backoff
```

How many times to try, how long to wait, and the wait's cap.

## `TimedOut`

```text
pub struct TimedOut
```

The error when a TimeLimit deadline wins; cleanup says how the work’s cleanup ended.

## `TimeLimit`

```text
pub struct TimeLimit
```

A deadline configuration; wrapping Result work adds Clock and converts TimedOut into its error type.

## `Retry`

```text
pub struct Retry
```

A retry configuration whose error type is selected when work is applied.

- `pub fn when[E, F < FnMut(E) -> bool](self, predicate: F) -> SelectiveRetry[E, F]`: Selects which errors allow another attempt.

## `SelectiveRetry`

```text
pub struct SelectiveRetry[E, F < FnMut(E) -> bool = fn(E) -> bool]
```

A retry configuration with a predicate for one concrete error type.

## `Fallback`

```text
pub struct Fallback[F]
```

A backup configuration that retains its concrete work function and row.

## `CircuitOpen`

```text
pub struct CircuitOpen
```

A marker converted into the work's error type when a guard refuses it.

## `Breaker`

```text
pub struct Breaker
```

Caller-owned state for a consecutive-failure circuit breaker.

## `Guard`

```text
pub struct Guard
```

A guard retaining shared mutable breaker state across applications.

## `Hedge`

```text
pub struct Hedge
```

A delay configuration for racing an immediate invocation against a later copy.

## `retry`

```text
pub fn retry(backoff: Backoff) -> Retry
```

Builds a retry configuration without acquiring a provider.

## `timeout`

```text
pub fn timeout(limit: Duration, cleanup: Duration? = .None) -> TimeLimit
```

Builds a deadline configuration without starting work.

## `with_timeout`

```text
pub fn with_timeout[I](limit: Duration, inner: I, cleanup: Duration? = .None) -> (I, TimeLimit)
```

Places a deadline outside an inner policy.

## `fallback`

```text
pub fn fallback[F, I](other: F, inner: I) -> (I, Fallback[F])
```

Places a backup outside an inner policy without starting work.

## `guarded`

```text
pub fn guarded(state: *mut Breaker) -> Guard
```

Builds a guard retaining the caller's shared mutable breaker state.

## `hedged`

```text
pub fn hedged(delay: Duration) -> Hedge
```

Builds a hedge configuration without acquiring a clock or starting work.

## `map_join`

```text
pub fn map_join![T < 'static, U < 'static, F < Fn(T) -> Suspend[U] $ R, $ R](
```

Runs `work` on every item, at most `limit` at a time, and returns the values in item order.

## `Sender`

```text
pub struct Sender[T < 'static]
```

The sending handle of a channel; copies send into the same channel.

- `pub fn send!(&self, value: T) -> void`: Puts `value` at the back of the channel, waiting while it is full.
- `pub fn close(&self) -> void`: Closes the channel: receivers drain what it holds, then get `.None`.

## `Receiver`

```text
pub struct Receiver[T < 'static]
```

The receiving handle of a channel; copies receive from the same channel.

- `pub fn recv!(&self) -> T?`: The item at the front of the channel, waiting while it is empty and open; `.None` once it is closed and drained.

## `channel`

```text
pub fn channel[T < 'static](capacity: usize) -> (Sender[T], Receiver[T])
```

A new channel that holds at most `capacity` items, as its two handles.

## `Suspend`

- `pub fn cancel!(self) -> CleanupEnd`: Stops the job, waits until its cleanup has ended, and returns how it ended.
- `pub fn abandon(self) -> void`: Stops the job and unwinds it at once: its `defer` blocks run, and each `dispose!` is cut short at its suspension point.
