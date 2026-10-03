# Time

Status: standard library specification draft.

This chapter defines the part of `std.time` that `lib/std` writes in
ordinary hd over the language tier:

- the `Duration` type, its public API, its arithmetic, and its `Display`
  text;
- the duration suffixes `ms`, `s`, `min`, and `h`;
- the host capability trait `Clock`, the `Timestamp` and `Instant` types
  it returns, and the helpers `now` and `sleep!`;
- `ManualClock`, the deterministic `Clock` provider.

The language tier keeps the suffix mechanism
([Literal Suffixes](../lang/05-expressions.md#literal-suffixes)):

| Item | Why it stays in the language tier |
| --- | --- |
| `@num_suffix`, `std.ops.NumSuffix` | the compiler recognizes the marker by its qualified name |
| a suffixed literal as a call | [`expr.suffix.fn-call`](../lang/05-expressions.md#r-expr.suffix.fn-call), a language rule, turns `250ms` into `ms(250)` |
| suffix name lookup and typing | [Literal Suffix Names](../lang/03-names-and-scopes.md#literal-suffix-names) and [Suffixed Literals](../lang/04-type-system.md#suffixed-literals) hold for every suffix |
| `Duration?` in the test signature | `it` names it as its `timeout` type ([Test Cases](../lang/10-modules.md#test-cases)); the stdlib-tier `it_each`, `it_prop`, and `it_prop_with` name it too |

The duration suffixes are ordinary functions marked `@num_suffix`, so
nothing in the language tier names `ms`, `s`, `min`, or `h`. What the
`timeout` option does is stdlib tier as well:
[Test Timeout](testing.md#test-timeout).

## Duration

1. r[std-time.suffix.std.duration] A `std.time.Duration` is a whole number of milliseconds, held in an `i64`.
2. r[std-time.suffix.std.duration-api] The public API of `Duration` is `Duration::milliseconds(n: i64)`, `Duration::seconds(n: i64)`, and `d.as_milliseconds() -> i64`.
3. r[std-time.suffix.std.duration-neg] `Duration` implements `std.ops.Neg` with `Out = Duration`, negating its milliseconds. So `-5s` is the ordinary negation `-(5s)`, minus five seconds.
4. r[std-time.prelude.time-suffixes] `std.time` declares `Duration` and the duration suffixes `ms`, `s`, `min`, and `h`, which code imports, as in `use std.time.{Duration, s}`.
5. r[std-time.duration.eq-ord] `Duration` implements `Eq` and `Ord`, which compare its milliseconds.

```text
use std.time.{Duration, s}

fn rewind() -> Duration:
    -5s  # -(s(5)), through Neg for Duration
```

### Duration Arithmetic

Durations add and subtract, and a duration moves a timestamp:

```text
use std.time.{Duration, Timestamp, ms, s}

fn total(first: Duration, second: Duration) -> Duration:
    first + second - 500ms

fn later(start: Timestamp) -> Timestamp:
    start + 2s
```

1. r[std-time.duration.add] `Duration` implements `std.ops.Add` with `Out = Duration`. `a + b` holds the sum of the milliseconds of `a` and `b`.
2. r[std-time.duration.sub] `Duration` implements `std.ops.Sub` with `Out = Duration`. `a - b` holds the milliseconds of `a` minus those of `b`.
3. r[std-time.duration.overflow] A sum or difference that does not fit in `i64` milliseconds panics at run time, as checked `i64` arithmetic does. Panic: `integer-overflow`.
4. r[std-time.timestamp.add] `Timestamp` implements `std.ops.Add[Duration]` with `Out = Timestamp`. `t + d` is the timestamp whose milliseconds are those of `t` plus those of `d`.
5. r[std-time.timestamp.add.overflow] A `t + d` whose milliseconds do not fit in `i64` panics at run time. Panic: `integer-overflow`.

> **Note.** A negative `d` gives an earlier timestamp. `Timestamp` has no
> `Sub`: `t.since(earlier)` is the duration between two timestamps, by
> [`std-time.timestamp.since`](#r-std-time.timestamp.since).

### Duration Display

A duration displays as Go's `time.Duration` does, with hd's unit names:
hours, minutes, and seconds, as in `1h2min3.5s`, or milliseconds under
one second, as in `500ms`:

```text
use std.time.{Duration, min, s}

fn waited() -> string:
    "waited ${90s}, then ${60min}"  # "waited 1min30s, then 1h0min0s"
```

1. r[std-time.duration.text] `Duration` implements `Display` with the algorithm of Go's `time.Duration.String`, using the unit names `h`, `min`, `s`, and `ms`.
2. r[std-time.duration.text.components] A duration of one second or more displays as components, each a whole count followed by its unit with no space: hours `h`, then minutes `min`, then seconds `s`.
3. r[std-time.duration.text.first] The first component is the largest unit whose count is not zero.
4. r[std-time.duration.text.lower] Every component after the first is written down to `s`, even when its count is zero, as in `1h0min5s` and `1min0s`.
5. r[std-time.duration.text.fraction] The seconds component carries the remaining milliseconds as a decimal fraction without trailing zeros, as in `3.5s` and `1.005s`. Whole seconds have no point.
6. r[std-time.duration.text.sub-second] A duration under one second that is not zero displays as its milliseconds, then `ms`, as in `500ms`.
7. r[std-time.duration.text.zero] A zero duration displays as `0s`.
8. r[std-time.duration.text.negative] A negative duration displays as `-`, then the text of its magnitude, as in `-1min30s` and `-500ms`.
9. r[std-time.duration.text.min-value] The most negative duration follows the same rule and displays as `-2562047788015h12min55.808s`. Displaying a duration never overflows.

| Milliseconds | Text |
| --- | --- |
| `0` | `0s` |
| `500` | `500ms` |
| `-500` | `-500ms` |
| `1500` | `1.5s` |
| `1005` | `1.005s` |
| `2000` | `2s` |
| `60000` | `1min0s` |
| `90000` | `1min30s` |
| `3600000` | `1h0min0s` |
| `3605000` | `1h0min5s` |
| `3723500` | `1h2min3.5s` |
| `-90000` | `-1min30s` |

> **Why.** Go's text is the most widely read duration format, so a log
> line needs no explanation. Writing every component below the first, as
> Go writes `1h0m0s`, keeps one spelling per duration and a shape that
> depends only on its largest unit.

## Duration Suffixes

The standard library declares these suffixes in `std.time`:

| Rule | Suffix | Meaning |
| --- | --- | --- |
| r[std-time.suffix.std.ms] Milliseconds | `ms` | `Duration` of that many milliseconds |
| r[std-time.suffix.std.s] Seconds | `s` | `Duration` of that many seconds |
| r[std-time.suffix.std.min] Minutes | `min` | `Duration` of that many minutes |
| r[std-time.suffix.std.h] Hours | `h` | `Duration` of that many hours |

1. r[std-time.suffix.std.fn] Each is a suffix function that takes one `i64` and returns the standard `std.time.Duration`, as in `@num_suffix pub fn ms(count: i64) -> Duration`.
2. r[std-time.suffix.std.only-four] These four are the only standard suffixes. `std` declares no `ns`, `us`, `m`, `d`, byte-size, or string suffix.
3. r[std-time.suffix.std.import] None is a prelude name; code imports them, as in `use std.time.{ms, s}`.
4. r[std-time.suffix.std.overflow] A standard suffix call whose result does not fit in `i64` milliseconds, as in `10_000_000_000_000_000h`, panics at run time, as checked `i64` arithmetic does. Panic: `integer-overflow`.

```text
use std.time.{Duration, ms, s}

enum Tier(limit: Duration):
    Fast -> Tier(limit=250ms)
    Slow -> Tier(limit=5s)
```

> **Why.** Every duration suffix returns `Duration`, so `5s` and `250ms`
> have one type and mix freely.

See also: [Literal Suffixes](../lang/05-expressions.md#literal-suffixes),
[Test Timeout](testing.md#test-timeout).

## Clock

`Clock` is the host capability trait that reads time and waits:

```text
pub trait Clock:
    fn now(self) -> Timestamp
    fn monotonic(self) -> Instant
    fn sleep!(mut self, duration: Duration) -> void
```

1. r[std-time.clock.decl] `std.time` declares the host capability trait `Clock` with the methods above. Code imports it, as in `use std.time.Clock`.
2. r[std-time.clock.now] `now` returns the current wall-clock time as a `Timestamp`.
3. r[std-time.clock.monotonic] `monotonic` returns the current reading of a clock that never goes backwards, as an `Instant`.
4. r[std-time.clock.sleep] `sleep!(duration)` completes once `duration` has passed on the clock.
5. r[std-time.clock.sleep.zero] `sleep!` with a zero `duration` completes without waiting.
6. r[std-time.clock.sleep.negative] `sleep!` with a negative `duration` must panic, on every provider. Panic: `explicit-panic`.
7. r[std-time.clock.plain-reads] `now` and `monotonic` are plain calls. Only `sleep!` suspends.
8. r[std-time.clock.mut] `sleep!` takes `mut self`, so `Clock` is a mutable requirement trait and a provider may advance its own time.

> **Why.** A clock read is a value from the host, as an environment read
> is, so it needs no driver; replay records it at the boundary either
> way. Waiting is the one operation that suspends.

> **Why.** `Duration` is signed, so a negative wait can be written. It is
> a caller's bug, and a panic shows it where it happens, rather than as a
> sleep that silently returns.

### Timestamps And Instants

1. r[std-time.timestamp.decl] `std.time` declares `Timestamp`, a point in UTC time held as whole milliseconds since the Unix epoch in an `i64`, with private fields.
2. r[std-time.timestamp.api] The public API of `Timestamp` is `Timestamp::from_unix_millis(millis: i64)` and `t.since(earlier: Timestamp) -> Duration`.
3. r[std-time.timestamp.since] `t.since(earlier)` is the time from `earlier` to `t`, negative when `earlier` is the later one.
4. r[std-time.instant.decl] `std.time` declares `Instant`, a reading of the monotonic clock as whole milliseconds since an origin that the provider chooses, with private fields.
5. r[std-time.instant.api] The public API of `Instant` is `Instant::from_millis(millis: i64)` and `i.since(earlier: Instant) -> Duration`, the time from `earlier` to `i`.
6. r[std-time.time.eq-ord] `Timestamp` and `Instant` implement `Eq` and `Ord`, which order them by time.
7. r[std-time.time.import] Code imports both, as in `use std.time.{Instant, Timestamp}`.

### Clock Helpers

```text
use std.time.{Clock, Duration, Timestamp, now, sleep}

fn wait_and_stamp!(pause: Duration) -> Timestamp $ Clock:
    sleep!(pause)
    now()
```

1. r[std-time.helper.now] `std.time` declares `pub fn now() -> Timestamp $ Clock`, which returns `now()` of the `Clock` provider that covers the call.
2. r[std-time.helper.sleep] `std.time` declares `pub fn sleep!(duration: Duration) -> void $ Clock`, which calls `sleep!(duration)` on that provider.

### Manual Clock

`ManualClock` is the deterministic `Clock` provider. Its virtual time
moves only when code sleeps, and a sleep returns at once:

```text
use std.testing.assert
use std.time.{Clock, ManualClock, Timestamp, s}

fn wait_twice!() -> Timestamp $ Clock:
    let mut clock = $.use(Clock)
    clock.sleep!(2s)
    clock.sleep!(3s)
    clock.now()

tests:
    it("sleeps in virtual time"):
        let mut clock = ManualClock::new(Timestamp::from_unix_millis(0))
        $.with(Clock=clock):
            assert(wait_twice!() == Timestamp::from_unix_millis(5000), reason="five virtual seconds")
```

1. r[std-time.manual.decl] `std.time` declares `ManualClock`, which implements `Clock`, with private fields. Code imports it, as in `use std.time.ManualClock`.
2. r[std-time.manual.state] A `ManualClock`'s only state is its current time, a `Timestamp`.
3. r[std-time.manual.new] `ManualClock::new(start: Timestamp) -> mut ManualClock` returns a clock whose current time is `start`.
4. r[std-time.manual.now] `now` returns the current time.
5. r[std-time.manual.monotonic] `monotonic` returns `Instant::from_millis(m)`, where `m` is the current time in milliseconds since the Unix epoch.
6. r[std-time.manual.sleep] `sleep!(duration)` completes without waiting. A positive `duration` is added to the current time.
7. r[std-time.manual.sleep.zero] A zero `duration` leaves the current time unchanged.
8. r[std-time.manual.sleep.panics] A negative `duration` panics, as [`std-time.clock.sleep.negative`](#r-std-time.clock.sleep.negative) requires, and leaves the current time unchanged. Panic: `explicit-panic`.
9. r[std-time.manual.reads] `now` and `monotonic` never change the current time.
10. r[std-time.manual.no-host] A `ManualClock` reads nothing from the host's clock.

> **Why.** Virtual time advances by itself: a test of a timeout or a
> backoff sleeps through it in no real time and needs no extra call. A
> test that only moves the clock calls `sleep!` on it.

See also: [Host Capabilities](../cli/command-line.md#host-capabilities),
[Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).
