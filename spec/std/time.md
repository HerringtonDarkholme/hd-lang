# Time

Status: standard library specification draft.

This chapter defines the part of `std.time` that `lib/std` writes in
ordinary hd over the language tier:

- the `Duration` type, its public API, its arithmetic, and its `Display`
  text;
- the duration suffixes `ms`, `s`, `min`, and `h`;
- `impl Parse for Duration`, which reads Go-style duration text, and `impl Parse for Timestamp`, which reads RFC 3339 text;
- the host capability trait `Clock`, the `Timestamp` and `Instant` types
  it returns, and the helpers `now`, `monotonic`, `elapsed`, and `sleep!`;
- `ManualClock`, the deterministic `Clock` provider;
- the UTC `Date` of a `Timestamp`, RFC 3339 and ISO 8601 text, the
  `Timestamp.format` layouts, and `ParseTimeError`;
- the serialization opt-in of `Duration`, `Timestamp`, and `Instant`.

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
2. r[std-time.duration.non-negative] A `Duration` is never negative: no call builds one below zero, and `Duration` holds zero milliseconds at the least. It is modeled on Rust's `std::time::Duration`.
3. r[std-time.suffix.std.duration-api] The public API of `Duration` is `Duration::milliseconds(n: i64)`, `Duration::seconds(n: i64)`, `d.as_milliseconds() -> i64`, `d.checked_sub(rhs: Duration) -> Duration?`, and `d.saturating_sub(rhs: Duration) -> Duration`.
4. r[std-time.duration.negative-count] `Duration::milliseconds(n)` and `Duration::seconds(n)` with a negative `n` panic at run time, in every build. Panic: `explicit-panic`.
5. r[std-time.duration.no-neg] `Duration` does not implement `std.ops.Neg`, so `-5s`, which is `-(5s)`, is rejected by the type check: `type-mismatch`.
6. r[std-time.prelude.time-suffixes] `std.time` declares `Duration` and the duration suffixes `ms`, `s`, `min`, and `h`, which code imports, as in `use std.time.{Duration, s}`.
7. r[std-time.duration.eq-ord] `Duration` implements `Eq` and `Ord`, which compare its milliseconds.
8. r[std-time.duration.value] `Duration` is a struct, a value with private fields, so copying one copies its milliseconds.

```text
use std.time.{Duration, s}

fn remaining(limit: Duration, used: Duration) -> Duration:
    limit.saturating_sub(used)  # zero when used is the longer
```

> **Why.** A wait, a timeout, or a backoff can then never be negative, so
> `sleep!` needs no rule for one. A signed difference is a question of
> order, which `checked_sub` and `since` answer with an absent value or
> zero, as Rust's `Duration` and `Instant` do.

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
2. r[std-time.duration.sub-difference] `Duration` implements `std.ops.Sub` with `Out = Duration`. When `a` is not shorter than `b`, `a - b` holds the milliseconds of `a` minus those of `b`.
3. r[std-time.duration.sub.underflow] `a - b` with `a` shorter than `b` panics at run time, in every build. Panic: `explicit-panic`.
4. r[std-time.duration.checked-sub] `a.checked_sub(b)` is `.Some(a - b)` when `a` is not shorter than `b`, and `.None` otherwise. It never panics.
5. r[std-time.duration.saturating-sub] `a.saturating_sub(b)` is `a - b` when `a` is not shorter than `b`, and zero otherwise. It never panics.
6. r[std-time.duration.add-overflow] A sum that does not fit in `i64` milliseconds panics at run time. In a debug or test build this is checked `i64` arithmetic. Panic: `integer-overflow`. A release build wraps the sum, and the wrapped value is negative, so the result panics as [`std-time.duration.negative-count`](#r-std-time.duration.negative-count) says.
7. r[std-time.timestamp.add] `Timestamp` implements `std.ops.Add[Duration]` with `Out = Timestamp`. `t + d` is the timestamp whose milliseconds are those of `t` plus those of `d`.
8. r[std-time.timestamp.add.overflow] A `t + d` whose milliseconds do not fit in `i64` panics at run time in a debug or test build, and wraps in a release build. Panic: `integer-overflow`.

> **Note.** `Timestamp` has no `Sub`: `t.since(earlier)` is the duration
> between two timestamps, by
> [`std-time.timestamp.since-saturating`](#r-std-time.timestamp.since-saturating).

### Duration Display

A duration displays as Go's `time.Duration` does, with hd's unit names.
That is hours, minutes, and seconds, as in `1h2min3.5s`, or milliseconds
under one second, as in `500ms`:

```text
use std.time.{Duration, min, s}

fn waited() -> string:
    "waited ${90s}, then ${60min}"  # "waited 1min30s, then 1h0min0s"
```

1. r[std-time.duration.text] `Duration` implements `Display` with the algorithm of Go's `time.Duration.String`, using the unit names `h`, `min`, `s`, and `ms`.
2. r[std-time.duration.text.components] A duration of one second or more displays as components. Each is a whole count followed by its unit with no space: hours `h`, then minutes `min`, then seconds `s`.
3. r[std-time.duration.text.first] The first component is the largest unit whose count is not zero.
4. r[std-time.duration.text.lower] Every component after the first is written down to `s`, even when its count is zero, as in `1h0min5s` and `1min0s`.
5. r[std-time.duration.text.fraction] The seconds component carries the remaining milliseconds as a decimal fraction without trailing zeros, as in `3.5s` and `1.005s`. Whole seconds have no point.
6. r[std-time.duration.text.sub-second] A duration under one second that is not zero displays as its milliseconds, then `ms`, as in `500ms`.
7. r[std-time.duration.text.zero] A zero duration displays as `0s`.
8. r[std-time.duration.text.max-value] The longest duration displays as `2562047788015h12min55.807s`. Displaying a duration never overflows.

| Milliseconds | Text |
| --- | --- |
| `0` | `0s` |
| `500` | `500ms` |
| `1500` | `1.5s` |
| `1005` | `1.005s` |
| `2000` | `2s` |
| `60000` | `1min0s` |
| `90000` | `1min30s` |
| `3600000` | `1h0min0s` |
| `3605000` | `1h0min5s` |
| `3723500` | `1h2min3.5s` |

> **Why.** Go's text is the most widely read duration format, so a log
> line needs no explanation. Writing every component below the first, as
> Go writes `1h0m0s`, keeps one spelling per duration and a shape that
> depends only on its largest unit.

### Duration Parsing

`impl Parse for Duration` reads the text form of Go's `time.ParseDuration`, so `"1h30m".parse::[Duration]()` calls it:

```text
use std.time.{Duration, ParseTimeError}

fn timeout_of(text: string) -> Result[Duration, ParseTimeError]:
    text.parse::[Duration]()  # "1h30m" is 5400000 milliseconds
```

The text is an optional `+`, then one or more components. A component
is a number and a unit:

| Part | Form |
| --- | --- |
| sign | `+`, optional, once at the start |
| number | one or more digits, then optionally `.` and one or more digits |
| unit | `h`, `min`, `m`, `s`, `ms`, `us`, `µs`, `μs`, or `ns` |

1. r[std-time.duration.parse] `std.time` declares `impl Parse for Duration` with `type Error = ParseTimeError`, and no inherent parse function. `text.parse::[Duration]()` returns `.Ok` of the duration that `text` names, or `.Err` of the first error.
2. r[std-time.duration.parse.text-grammar] `text` must hold the optional `+`, then one or more components, each with the parts above in order, and nothing else. No space is allowed anywhere.
3. r[std-time.duration.parse.units] `h` is 3600 seconds. `min` and `m` are 60 seconds. `s` is one second, and `ms` is a millisecond. `us`, `µs` (U+00B5), and `μs` (U+03BC) are a microsecond. `ns` is a nanosecond.
4. r[std-time.duration.parse.components-sum] The duration is the sum of its components. A component's value is its number times its unit, with the fraction exact.
5. r[std-time.duration.parse.exact] A component whose value is not a whole number of milliseconds gives `Inexact`. Nothing is rounded, so `1.5ms` and `500us` are errors, and `2000us` and `1.005s` are not.
6. r[std-time.duration.parse.zero-text] The text `0`, with an optional `+` and no unit, is the zero duration. No other number is valid without a unit.
7. r[std-time.duration.parse.range-max] A component or a running total that does not fit in `i64` milliseconds gives `OutOfRange`. The longest duration is in range, so `9223372036854775807ms` parses.
8. r[std-time.duration.parse.no-sign] A `-` is not a duration character: no duration is negative, so `-5s` gives `InvalidCharacter` at 0, as does `-0`.
9. r[std-time.duration.parse.round-trip] For every duration `d`, `d.to_string().parse::[Duration]()` is `.Ok(d)`.
10. r[std-time.duration.parse.no-panic] The `Parse` impl of `Duration` never panics.

| Text | `parse` gives the milliseconds |
| --- | --- |
| `1h30m` | `5400000` |
| `1h0min5s` | `3605000` |
| `250ms` | `250` |
| `1.5s` | `1500` |
| `1.5h` | `5400000` |
| `2000us` | `2` |
| `0` | `0` |

> **Why.** Go's text is what config files and flags already hold. `min`
> is accepted next to Go's `m` so that the text `Display` writes parses
> back. A unit finer than the resolution is allowed when the value is
> whole, and rejected otherwise, so a duration never changes silently.

#### Duration Parse Errors

A failed duration parse reports a [`ParseTimeError`](#time-parse-errors),
the one error type of all time text, at a byte position:

1. r[std-time.duration.parse-error.too-short] Text that ends where a number is needed gives `TooShort` at the text's length. That is empty text, a `+` alone, or a `.` with no digit after it.
2. r[std-time.duration.parse-error.invalid-character] A character that is not a digit, where a number must start, gives `InvalidCharacter` at that character, as does a non-digit after a `.`. So a `-` gives it at its own position.
3. r[std-time.duration.parse-error.missing-unit] A number that is followed by the end of the text, or by a `.`, gives `MissingUnit` at the end of the number.
4. r[std-time.duration.parse-error.unknown-unit] The characters after a number, up to the next digit or `.`, that are not a unit give `UnknownUnit` at their start.
5. r[std-time.duration.parse-error.range-position] `Inexact` and `OutOfRange` are at the first digit of the component.
6. r[std-time.duration.parse-error.order] The text is read from the left, one component at a time. In a component the number and unit errors come first, then `OutOfRange`, then `Inexact`, and the first error wins.

| Text | `parse` gives |
| --- | --- |
| `""` | `.Err(TooShort(0))` |
| `"+"` | `.Err(TooShort(1))` |
| `"-5s"` | `.Err(InvalidCharacter(0))` |
| `"1"` | `.Err(MissingUnit(1))` |
| `"1x"` | `.Err(UnknownUnit(1))` |
| `"1 s"` | `.Err(UnknownUnit(1))` |
| `".5s"` | `.Err(InvalidCharacter(0))` |
| `"1.5ms"` | `.Err(Inexact(0))` |
| `"1h2.5ms"` | `.Err(Inexact(2))` |
| `"9223372036854775808ms"` | `.Err(OutOfRange(0))` |

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
4. r[std-time.suffix.std.overflow] A standard suffix call whose result does not fit in `i64` milliseconds panics at run time in a debug or test build, as in `10_000_000_000_000_000h`. It wraps in a release build, as `i64` arithmetic does. Panic: `integer-overflow`.

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
    fn sleep!(&mut self, duration: Duration) -> void
```

1. r[std-time.clock.decl-ref] `std.time` declares the host capability trait `Clock` with the methods above. Code imports it, as in `use std.time.Clock`.
2. r[std-time.clock.now] `now` returns the current wall-clock time as a `Timestamp`.
3. r[std-time.clock.monotonic] `monotonic` returns the current reading of a clock that never goes backwards, as an `Instant`.
4. r[std-time.clock.sleep] `sleep!(duration)` completes once `duration` has passed on the clock.
5. r[std-time.clock.sleep.zero] `sleep!` with a zero `duration` completes without waiting.
6. r[std-time.clock.plain-reads] `now` and `monotonic` are plain calls. Only `sleep!` suspends.
7. r[std-time.clock.mut-ref] `sleep!` takes `&mut self`, so `Clock` is a mutable requirement trait, `$.use(Clock)` is a `&mut dyn Clock` by [`req.ref.use`](../lang/11-requirements-and-suspension.md#r-req.ref.use), and a provider may advance its own time.

> **Why.** A clock read is a value from the host, as an environment read
> is, so it needs no driver. Replay records it at the boundary either
> way. Waiting is the one operation that suspends.

### Timestamps And Instants

1. r[std-time.timestamp.decl] `std.time` declares `Timestamp`, a point in UTC time held as whole milliseconds since the Unix epoch in an `i64`, with private fields.
2. r[std-time.timestamp.api-milliseconds] The public API of `Timestamp` is `Timestamp::from_unix_milliseconds(milliseconds: i64)`, `t.unix_milliseconds() -> i64`, `t.since(earlier: Timestamp) -> Duration`, `t.checked_since(earlier: Timestamp) -> Duration?`, and the two methods of [Dates](#dates): `date` and `to_rfc3339`. Reading RFC 3339 text is [`impl Parse for Timestamp`](#parsing-rfc-3339).
3. r[std-time.timestamp.unix-milliseconds] `t.unix_milliseconds()` returns the milliseconds since the Unix epoch that `t` holds, so `Timestamp::from_unix_milliseconds(m).unix_milliseconds()` is `m` for every `m`.
4. r[std-time.timestamp.since-saturating] `t.since(earlier)` is the time from `earlier` to `t`. It is zero when `earlier` is the later one, and the longest duration when the time does not fit in `i64` milliseconds. It never panics.
5. r[std-time.timestamp.checked-since] `t.checked_since(earlier)` is `.Some` of that time, and `.None` when `earlier` is the later one or the time does not fit in `i64` milliseconds.
6. r[std-time.instant.decl] `std.time` declares `Instant`, a reading of the monotonic clock as whole milliseconds since an origin that the provider chooses, with private fields.
7. r[std-time.instant.api-milliseconds] The public API of `Instant` is `Instant::from_milliseconds(milliseconds: i64)`, `i.as_milliseconds() -> i64`, `i.since(earlier: Instant) -> Duration`, and `i.checked_since(earlier: Instant) -> Duration?`.
8. r[std-time.instant.since] `i.since(earlier)` is the time from `earlier` to `i`, with the cases of [`std-time.timestamp.since-saturating`](#r-std-time.timestamp.since-saturating): zero when `earlier` is the later one, and the longest duration when the time does not fit.
9. r[std-time.instant.checked-since] `i.checked_since(earlier)` is `.Some` of that time, and `.None` in the cases of [`std-time.timestamp.checked-since`](#r-std-time.timestamp.checked-since).
10. r[std-time.instant.as-milliseconds] `i.as_milliseconds()` returns the milliseconds since the origin that `i` holds, so `Instant::from_milliseconds(m).as_milliseconds()` is `m` for every `m`.
11. r[std-time.time.eq-ord] `Timestamp` and `Instant` implement `Eq` and `Ord`, which order them by time.
12. r[std-time.timestamp.display] `Timestamp` implements `Display`, and its text is `t.to_rfc3339()`. `Instant` does not implement `Display`.
13. r[std-time.time.import] Code imports both, as in `use std.time.{Instant, Timestamp}`.
14. r[std-time.time.value] `Timestamp` and `Instant` are structs, values with private fields, so copying one copies its milliseconds.

```text
use std.time.{Instant, Timestamp}

fn epoch() -> Timestamp:
    Timestamp::from_unix_milliseconds(0)   # displays as 1970-01-01T00:00:00Z

fn same_milliseconds(m: i64) -> bool:
    Timestamp::from_unix_milliseconds(m).unix_milliseconds() == m && Instant::from_milliseconds(m).as_milliseconds() == m
```

### Clock Helpers

```text
use std.time.{Clock, Duration, Timestamp, now, sleep}

fn wait_and_stamp!(pause: Duration) -> Timestamp $ Clock:
    sleep!(pause)
    now()
```

```text
use std.time.{Clock, Duration, elapsed, monotonic}

fn timed_sum() -> Duration $ Clock:
    start := monotonic()
    let total = 0
    for i in 0..1000:
        total = total + i
    elapsed(start)
```

1. r[std-time.helper.now] `std.time` declares `pub fn now() -> Timestamp $ Clock`, which returns `now()` of the `Clock` provider that covers the call.
2. r[std-time.helper.sleep] `std.time` declares `pub fn sleep!(duration: Duration) -> void $ Clock`, which calls `sleep!(duration)` on that provider.
3. r[std-time.helper.monotonic] `std.time` declares `pub fn monotonic() -> Instant $ Clock`, which returns `monotonic()` of the `Clock` provider that covers the call.
4. r[std-time.helper.elapsed] `std.time` declares `pub fn elapsed(since: Instant) -> Duration $ Clock`, which returns `monotonic().since(since)`: the time from `since` to the provider's monotonic reading now.
5. r[std-time.helper.elapsed.manual] Under a `ManualClock`, `elapsed(start)` is the total that was slept since `start` was read.

> **Why.** Timing a block is two lines, as Go's `time.Since` and Rust's
> `Instant::elapsed` make it. The helpers take the provider from the
> requirement row, so the `Clock` trait is unchanged and a test times a
> block in virtual time.

### Manual Clock

`ManualClock` is the deterministic `Clock` provider. Its virtual time
moves only when code sleeps, and a sleep returns at once:

```text
use std.time.{Clock, ManualClock, Timestamp, s}

fn wait_twice!() -> Timestamp $ Clock:
    let mut clock = $.use(Clock)
    clock.sleep!(2s)
    clock.sleep!(3s)
    clock.now()

tests:
    use std.testing.assert

    it("sleeps in virtual time"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Clock=clock):
            assert(wait_twice!() == Timestamp::from_unix_milliseconds(5000), reason="five virtual seconds")
```

1. r[std-time.manual.decl-ref] `std.time` declares the struct `ManualClock`, which implements `Clock`, with private fields. Code imports it, as in `use std.time.ManualClock`. Its `sleep!` takes `&mut self`, as the trait's does, so a `ManualClock` serves as a provider through a reference to it.
2. r[std-time.manual.state] A `ManualClock`'s only state is its current time, a `Timestamp`.
3. r[std-time.manual.new-ref] `ManualClock::new(start: Timestamp) -> &mut ManualClock` returns a clock whose current time is `start`, a new object. A provider installed from that reference, and every copy of it, share the one clock, so a sleep through the provider moves the clock the caller holds.
4. r[std-time.manual.now] `now` returns the current time.
5. r[std-time.manual.monotonic] `monotonic` returns `Instant::from_milliseconds(m)`, where `m` is the current time in milliseconds since the Unix epoch.
6. r[std-time.manual.sleep] `sleep!(duration)` completes without waiting. The `duration` is added to the current time.
7. r[std-time.manual.sleep.zero] A zero `duration` leaves the current time unchanged.
8. r[std-time.manual.reads] `now` and `monotonic` never change the current time.
9. r[std-time.manual.no-host] A `ManualClock` reads nothing from the host's clock.

> **Why.** Virtual time advances by itself. A test of a timeout or a
> backoff sleeps through it in no real time and needs no extra call. A
> test that only moves the clock calls `sleep!` on it.

See also: [Host Capabilities](../cli/command-line.md#host-capabilities),
[Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).

## Dates

A `Date` is a calendar day in UTC, and `t.date()` gives the day that a
timestamp falls on:

```text
use std.time.{Date, Timestamp}

fn day_of(millis: i64) -> Date:
    Timestamp::from_unix_milliseconds(millis).date()  # 2000-02-29 for 951782400000
```

`std.time` declares it with three public fields:

```text
pub struct Date:
    pub year: i32
    pub month: i32
    pub day: i32
```

1. r[std-time.date.decl-struct] `std.time` declares the struct `Date`, a value with the public fields `year`, `month`, and `day`, each an `i32`. Copying a `Date` copies its fields. Code imports it, as in `use std.time.Date`.
2. r[std-time.date.calendar] A `Date` names a day of the proleptic Gregorian calendar: the Gregorian leap-year rule holds for every year, including years before 1582.
3. r[std-time.date.year] `year` uses astronomical numbering: year 0 is 1 BC, and year -1 is 2 BC.
4. r[std-time.date.fields] `month` counts from 1, January, to 12, and `day` counts from 1.
5. r[std-time.date.of-timestamp] `t.date()` returns the UTC date of the day that contains `t`.
6. r[std-time.date.before-epoch] A timestamp before the epoch belongs to the day that contains it, so the timestamp of `-1` milliseconds falls on 1969-12-31.
7. r[std-time.date.traits] `Date` implements `Eq`, which compares the three fields, and `Debug`.
8. r[std-time.date.utc-only] `std.time` has no local time and no time-zone type. A fixed offset appears only in parsed text, by [`std-time.rfc3339.parse.offset`](#r-std-time.rfc3339.parse.offset).

| Milliseconds | `date()` |
| --- | --- |
| `0` | 1970-01-01 |
| `-1` | 1969-12-31 |
| `951782400000` | 2000-02-29 |
| `-2203891200000` | 1900-03-01 |
| `-12219379200000` | 1582-10-14 |
| `-62167219200000` | 0000-01-01 |

> **Note.** The fields are public, so a literal may hold a day that does
> not exist, such as month 13. Only `date()` promises a real day, and no
> `std` function takes a `Date`.

> **Why.** The proleptic Gregorian calendar with a year 0 is the one ISO
> 8601 and RFC 3339 use. So a date needs no table of calendar reforms.
> Time zones need a database, and Rust, Kotlin, and Zig keep them out of
> their standard libraries too.

### RFC 3339 Text

`to_rfc3339` writes a timestamp as RFC 3339 text in UTC:

```text
use std.time.Timestamp

fn stamp(millis: i64) -> string:
    Timestamp::from_unix_milliseconds(millis).to_rfc3339()  # "2023-11-14T22:13:20.500Z" for 1700000000500
```

The text is `YYYY-MM-DDThh:mm:ss`, then `.fff` when the milliseconds are
not zero, then `Z`:

| Part | Text |
| --- | --- |
| `YYYY` | the year of `t.date()` |
| `MM`, `DD` | the month and day of `t.date()`, two digits each |
| `T` | the letter `T` |
| `hh`, `mm`, `ss` | the UTC hour `00` to `23`, minute `00` to `59`, and second `00` to `59`, two digits each |
| `.fff` | a point and the millisecond of the second, `001` to `999`, in three digits |
| `Z` | the letter `Z` |

1. r[std-time.rfc3339.text] `t.to_rfc3339()` returns the text of `t` in UTC with the parts in the table above, in order, and nothing else.
2. r[std-time.rfc3339.text.fraction] The fraction is left out when the millisecond of the second is zero. Otherwise it has exactly three digits, so `500` milliseconds is `.500`.
3. r[std-time.rfc3339.text.year] A year from 0 to 9999 has exactly four digits. A later year has all its digits, and a negative year is `-` and at least four digits, as in `-0001`.
4. r[std-time.rfc3339.text.no-panic] `to_rfc3339` never panics.

| Milliseconds | `to_rfc3339()` |
| --- | --- |
| `0` | `1970-01-01T00:00:00Z` |
| `1700000000500` | `2023-11-14T22:13:20.500Z` |
| `1700000000007` | `2023-11-14T22:13:20.007Z` |
| `-1` | `1969-12-31T23:59:59.999Z` |
| `-62198755200000` | `-0001-01-01T00:00:00Z` |
| `253402300800000` | `10000-01-01T00:00:00Z` |

> **Note.** RFC 3339 allows only the years 0000 to 9999. So the text of
> a year outside them is not RFC 3339, and parsing it as a `Timestamp` rejects it.
> Go's `time.Format` writes such a year the same way.

> **Why.** Whole seconds stay short, and a fraction shows every
> millisecond a `Timestamp` holds, as Rust's chrono writes it.

### ISO 8601 Dates

`to_iso8601` writes a date as the calendar form of ISO 8601:

```text
use std.time.Timestamp

fn day_text(millis: i64) -> string:
    Timestamp::from_unix_milliseconds(millis).date().to_iso8601()  # "2023-11-14" for 1700000000500
```

1. r[std-time.date.iso8601] `d.to_iso8601()` returns `YYYY-MM-DD`: the year, the month in two digits, and the day in two digits, joined by `-`.
2. r[std-time.date.iso8601.year] The year is written as [`std-time.rfc3339.text.year`](#r-std-time.rfc3339.text.year) writes it, so `-0001-01-01` is the first day of 2 BC.
3. r[std-time.date.iso8601.fields] The month and day are the fields, padded to two digits. A `Date` literal that holds a day that does not exist is written as it is. `to_iso8601` never panics.
4. r[std-time.date.iso8601.same-date] `t.date().to_iso8601()` is the first ten characters of `t.to_rfc3339()` for a year from 0 to 9999.

### Layouts

`format` writes a timestamp in UTC by a layout of `%` specifiers, as
`strftime` does:

```text
use std.time.Timestamp

fn clock_text(millis: i64) -> string:
    Timestamp::from_unix_milliseconds(millis).format("%Y-%m-%d %H:%M")  # "2023-11-14 22:13"
```

| Specifier | Text |
| --- | --- |
| `%Y` | the year, as [`std-time.rfc3339.text.year`](#r-std-time.rfc3339.text.year) writes it |
| `%m`, `%d` | the month and the day of `t.date()`, two digits each |
| `%H`, `%M`, `%S` | the UTC hour `00` to `23`, minute, and second, two digits each |
| `%z` | the UTC offset, always `+0000` |
| `%%` | a `%` |

1. r[std-time.format] `t.format(layout: string) -> string` returns `layout` with each specifier in the table above replaced by its text, and every other character kept.
2. r[std-time.format.unknown] A `%` followed by any other character, or by the end of `layout`, is kept as written. So `%q` stays `%q`, and a final `%` stays `%`. `format` never panics.
3. r[std-time.format.no-locale] `format` has no locale, no names of months or days, and no specifier for the milliseconds. A program writes the milliseconds with `to_rfc3339`.
4. r[std-time.format.rfc3339] `t.format("%Y-%m-%dT%H:%M:%SZ")` is `t.to_rfc3339()` when the milliseconds of `t` are zero.

> **Why.** Dates, times, and an offset cover log lines and file names.
> The specifiers are those of C, Python, and Rust's chrono, which agents
> already write, where Go's reference-date layouts are easy to misspell.
> The set stays small because there is no time zone and no locale.

### Parsing RFC 3339

`impl Parse for Timestamp` reads the `date-time` form of RFC 3339,
section 5.6, the text its `Display` writes:

```text
use std.time.{ParseTimeError, Timestamp}

fn read_stamp(text: string) -> Timestamp?:
    match text.parse::[Timestamp]():  # "2026-10-03T12:34:56.789+05:30"
        .Ok(stamp) => .Some(stamp)
        .Err(_) => .None
```

| Part | Form | Range |
| --- | --- | --- |
| year | four digits | `0000` to `9999` |
| month | `-` and two digits | `01` to `12` |
| day | `-` and two digits | `01` to the last day of the month |
| separator | `T` or `t` | |
| hour | two digits | `00` to `23` |
| minute | `:` and two digits | `00` to `59` |
| second | `:` and two digits | `00` to `59` |
| fraction | optional: `.` and one or more digits | any |
| offset | `Z`, `z`, or `+` or `-`, two hour digits, `:`, and two minute digits | hours `00` to `23`, minutes `00` to `59` |

1. r[std-time.rfc3339.parse] `std.time` declares `impl Parse for Timestamp` with `type Error = ParseTimeError`, and no inherent parse function. `text.parse::[Timestamp]()` returns `.Ok` of the instant that `text` names, or `.Err` of the first error.
2. r[std-time.rfc3339.parse.grammar] `text` must hold the parts in the table above, in order, each in its range, and nothing before or after them.
3. r[std-time.rfc3339.parse.case] The separator may be `T` or `t`, and the UTC offset `Z` or `z`.
4. r[std-time.rfc3339.parse.offset] A numeric offset is the local time's offset from UTC, so the instant is the local time minus the offset. `-00:00` and `+00:00` mean UTC, as `Z` does.
5. r[std-time.rfc3339.parse.fraction] The fraction may have any number of digits. Its digits after the third are ignored, so the time is truncated to the millisecond.
6. r[std-time.rfc3339.parse.leap-day] February 29 is in range only in a leap year: a year divisible by 4, except one divisible by 100 but not by 400.
7. r[std-time.rfc3339.parse.leap-second] Second `60` is out of range.
8. r[std-time.rfc3339.parse.round-trip] For every timestamp `t` whose year is 0 to 9999, `t.to_string().parse::[Timestamp]()` is `.Ok(t)`.
9. r[std-time.rfc3339.parse.no-panic] The `Parse` impl of `Timestamp` never panics.

| Text | Parsed as a `Timestamp`, gives the milliseconds |
| --- | --- |
| `1970-01-01T00:00:00Z` | `0` |
| `2000-02-29t00:00:00z` | `951782400000` |
| `2026-10-03T12:34:56.789+05:30` | `1791011096789` |
| `1999-12-31T23:00:00-02:00` | `946688400000` |
| `1969-12-31T23:59:59.9999999Z` | `-1` |

> **Why.** A `Timestamp` counts Unix milliseconds, which have no leap
> seconds, so `:60` has no instant to name; Go's `time.Parse` rejects it
> too. RFC 3339 allows a space for the `T` only in a note, so the parser
> keeps to its grammar.

### Time Parse Errors

One error type serves all time text: RFC 3339 timestamps and duration
text. A failed parse reports one of six errors, each at a position:

```text
pub enum ParseTimeError:
    InvalidCharacter(position: usize)
    OutOfRange(position: usize)
    TooShort(position: usize)
    MissingUnit(position: usize)
    UnknownUnit(position: usize)
    Inexact(position: usize)
```

1. r[std-time.parse-error.declared] `std.time` declares the enum `ParseTimeError` with the variants above, each with one field `position: usize`. Code imports it, as in `use std.time.ParseTimeError`.
2. r[std-time.parse-error.position-bytes] A `position` is a byte offset into the text, counted in bytes of its UTF-8 encoding from 0, as [`slice`](../lang/10-modules.md#r-module.string.byte-offsets) counts.
3. r[std-time.parse-error.invalid-character] A character that the grammar does not allow at its place gives `InvalidCharacter` at that character. So does a character after a complete timestamp.
4. r[std-time.parse-error.out-of-range] A field whose digits are well formed but whose value is out of its range gives `OutOfRange` at the field's first digit.
5. r[std-time.parse-error.too-short] Text that ends before the timestamp is complete gives `TooShort`, whose `position` is the text's length.
6. r[std-time.parse-error.order] The text is read from the left, and the first error found wins. A field's range is checked as soon as its last digit is read.
7. r[std-time.parse-error.traits] `ParseTimeError` implements `Eq`, `Debug`, `Display`, and `Error`.
8. r[std-time.parse-error.duration-variants] `MissingUnit`, `UnknownUnit`, and `Inexact` come only from duration text, as [Duration Parse Errors](#duration-parse-errors) says. A timestamp parse gives only `InvalidCharacter`, `OutOfRange`, and `TooShort`.

| Text | Parsed as a `Timestamp`, gives |
| --- | --- |
| `""` | `.Err(TooShort(0))` |
| `"2026-10-03"` | `.Err(TooShort(10))` |
| `"2026-10-03 12:00:00Z"` | `.Err(InvalidCharacter(10))` |
| `"2026-10-03T12:00:00+0530"` | `.Err(InvalidCharacter(22))` |
| `"2026-10-03T12:00:00Zx"` | `.Err(InvalidCharacter(20))` |
| `"2026-13-03T12:00:00Z"` | `.Err(OutOfRange(5))` |
| `"1900-02-29T12:00:00Z"` | `.Err(OutOfRange(8))` |
| `"2016-12-31T23:59:60Z"` | `.Err(OutOfRange(17))` |
| `"2026-02-30"` | `.Err(OutOfRange(8))`, not `TooShort` |

See also: [Decode Errors](encoding.md#decode-errors),
[Integer Parsing](num.md#integer-parsing).

## Serialization

`Duration` and `Instant` give their
[serialization opt-in](../lang/14-annotations.md#serialization) as their
whole milliseconds, and `Timestamp` as its RFC 3339 text:

```text
use std.json.encode
use std.serde.{Serialize, Deserialize}
use std.time.{Duration, Timestamp}

@derive(Serialize, Deserialize)
struct Lease:
    holder: string
    granted_at: Timestamp
    term: Duration

fn record(lease: Lease) -> string:
    encode(lease)  # {"holder":"ada","granted_at":"2023-11-14T22:13:20Z","term":30000}
```

1. r[std-time.serde.impls] `Duration`, `Timestamp`, and `Instant` each implement `std.serde.Serialize` and `std.serde.Deserialize`.
2. r[std-time.serde.int-form] A `Duration` and an `Instant` each write one `int` holding their milliseconds, as `as_milliseconds()` gives.
3. r[std-time.serde.text-form] A `Timestamp` writes one text: its RFC 3339 text in UTC with milliseconds, the same text its `Display` gives.
4. r[std-time.serde.int-read] An `Instant`, and a `Duration` whose value is a number, each read one `int` with the type's name as `expected`, as in `"Duration"`, and hold that many milliseconds.
5. r[std-time.serde.negative-read] A `Duration` whose number is negative is a decode error naming `Duration`, as one the parser rejects is. No negative `Duration` is built.
6. r[std-time.serde.text-read] A `Timestamp` reads one text with `"Timestamp"` as `expected`, and holds what [`Parse` for `Timestamp`](#parsing-rfc-3339) accepts. A text the parser rejects is a decode error naming `Timestamp`.
7. r[std-time.serde.duration-text-read] A `Duration` whose value is text reads one text with `"Duration"` as `expected`, and holds what [`Parse` for `Duration`](#duration-parsing) accepts, as `"30s"`. A text the parser rejects is a decode error naming `Duration`. Writing stays whole milliseconds, so a `Duration` reads both forms and writes one.

> **Why.** A timestamp reads the way it displays, so JSON carries the same
> text a log line does. Durations and instants stay integers of
> milliseconds, which round trip exactly, and the private fields stay
> private to code.

> **Note.** The implementations let a `Clock` provider's `Timestamp` and
> `Instant` cross the host boundary into hd, and `sleep!`'s `Duration`
> cross out, by
> [`module.boundary.in`](../lang/10-modules.md#r-module.boundary.in).
