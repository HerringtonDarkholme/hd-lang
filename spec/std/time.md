# Time

Status: standard library specification draft.

This chapter defines the part of `std.time` that `lib/std` writes in
ordinary hd over the language tier:

- the `Duration` type, its public API, its arithmetic, and its `Display`
  text;
- the duration suffixes `ms`, `s`, `min`, and `h`;
- the host capability trait `Clock`, the `Timestamp` and `Instant` types
  it returns, and the helpers `now` and `sleep!`;
- `ManualClock`, the deterministic `Clock` provider;
- the UTC `Date` of a `Timestamp`, RFC 3339 text, and `TimeParseError`;
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
3. r[std-time.duration.overflow] A sum or difference that does not fit in `i64` milliseconds panics at run time in a debug or test build. This is as checked `i64` arithmetic does, and it wraps in a release build. Panic: `integer-overflow`.
4. r[std-time.timestamp.add] `Timestamp` implements `std.ops.Add[Duration]` with `Out = Timestamp`. `t + d` is the timestamp whose milliseconds are those of `t` plus those of `d`.
5. r[std-time.timestamp.add.overflow] A `t + d` whose milliseconds do not fit in `i64` panics at run time in a debug or test build, and wraps in a release build. Panic: `integer-overflow`.

> **Note.** A negative `d` gives an earlier timestamp. `Timestamp` has no
> `Sub`: `t.since(earlier)` is the duration between two timestamps, by
> [`std-time.timestamp.since`](#r-std-time.timestamp.since).

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
> is, so it needs no driver. Replay records it at the boundary either
> way. Waiting is the one operation that suspends.

> **Why.** `Duration` is signed, so a negative wait can be written. It is
> a caller's bug, and a panic shows it where it happens, rather than as a
> sleep that silently returns.

### Timestamps And Instants

1. r[std-time.timestamp.decl] `std.time` declares `Timestamp`, a point in UTC time held as whole milliseconds since the Unix epoch in an `i64`, with private fields.
2. r[std-time.timestamp.api-milliseconds] The public API of `Timestamp` is `Timestamp::from_unix_milliseconds(milliseconds: i64)`, `t.unix_milliseconds() -> i64`, `t.since(earlier: Timestamp) -> Duration`, and the three methods of [Dates](#dates): `date`, `to_rfc3339`, and `Timestamp::parse_rfc3339`.
3. r[std-time.timestamp.unix-milliseconds] `t.unix_milliseconds()` returns the milliseconds since the Unix epoch that `t` holds, so `Timestamp::from_unix_milliseconds(m).unix_milliseconds()` is `m` for every `m`.
4. r[std-time.timestamp.since] `t.since(earlier)` is the time from `earlier` to `t`, negative when `earlier` is the later one.
5. r[std-time.instant.decl] `std.time` declares `Instant`, a reading of the monotonic clock as whole milliseconds since an origin that the provider chooses, with private fields.
6. r[std-time.instant.api-milliseconds] The public API of `Instant` is `Instant::from_milliseconds(milliseconds: i64)`, `i.as_milliseconds() -> i64`, and `i.since(earlier: Instant) -> Duration`, the time from `earlier` to `i`.
7. r[std-time.instant.as-milliseconds] `i.as_milliseconds()` returns the milliseconds since the origin that `i` holds, so `Instant::from_milliseconds(m).as_milliseconds()` is `m` for every `m`.
8. r[std-time.time.eq-ord] `Timestamp` and `Instant` implement `Eq` and `Ord`, which order them by time.
9. r[std-time.timestamp.display] `Timestamp` implements `Display`, and its text is `t.to_rfc3339()`. `Instant` does not implement `Display`.
10. r[std-time.time.import] Code imports both, as in `use std.time.{Instant, Timestamp}`.

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

1. r[std-time.helper.now] `std.time` declares `pub fn now() -> Timestamp $ Clock`, which returns `now()` of the `Clock` provider that covers the call.
2. r[std-time.helper.sleep] `std.time` declares `pub fn sleep!(duration: Duration) -> void $ Clock`, which calls `sleep!(duration)` on that provider.

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
        let mut clock = ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Clock=clock):
            assert(wait_twice!() == Timestamp::from_unix_milliseconds(5000), reason="five virtual seconds")
```

1. r[std-time.manual.decl] `std.time` declares `ManualClock`, which implements `Clock`, with private fields. Code imports it, as in `use std.time.ManualClock`.
2. r[std-time.manual.state] A `ManualClock`'s only state is its current time, a `Timestamp`.
3. r[std-time.manual.new] `ManualClock::new(start: Timestamp) -> mut ManualClock` returns a clock whose current time is `start`.
4. r[std-time.manual.now] `now` returns the current time.
5. r[std-time.manual.monotonic] `monotonic` returns `Instant::from_milliseconds(m)`, where `m` is the current time in milliseconds since the Unix epoch.
6. r[std-time.manual.sleep] `sleep!(duration)` completes without waiting. A positive `duration` is added to the current time.
7. r[std-time.manual.sleep.zero] A zero `duration` leaves the current time unchanged.
8. r[std-time.manual.sleep.panics] A negative `duration` panics, as [`std-time.clock.sleep.negative`](#r-std-time.clock.sleep.negative) requires, and leaves the current time unchanged. Panic: `explicit-panic`.
9. r[std-time.manual.reads] `now` and `monotonic` never change the current time.
10. r[std-time.manual.no-host] A `ManualClock` reads nothing from the host's clock.

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
pub data Date:
    pub year: i32
    pub month: i32
    pub day: i32
```

1. r[std-time.date.decl] `std.time` declares the data type `Date` with the public fields `year`, `month`, and `day`, each an `i32`. Code imports it, as in `use std.time.Date`.
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
> a year outside them is not RFC 3339, and `parse_rfc3339` rejects it.
> Go's `time.Format` writes such a year the same way.

> **Why.** Whole seconds stay short, and a fraction shows every
> millisecond a `Timestamp` holds, as Rust's chrono writes it.

### Parsing RFC 3339

`Timestamp::parse_rfc3339` reads the `date-time` form of RFC 3339,
section 5.6:

```text
use std.time.{TimeParseError, Timestamp}

fn read_stamp(text: string) -> Timestamp?:
    match Timestamp::parse_rfc3339(text):  # "2026-10-03T12:34:56.789+05:30"
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

1. r[std-time.rfc3339.parse] `std.time` declares `Timestamp::parse_rfc3339(text: string) -> Result[Timestamp, TimeParseError]`. It returns `.Ok` of the instant that `text` names, or `.Err` of the first error.
2. r[std-time.rfc3339.parse.grammar] `text` must hold the parts in the table above, in order, each in its range, and nothing before or after them.
3. r[std-time.rfc3339.parse.case] The separator may be `T` or `t`, and the UTC offset `Z` or `z`.
4. r[std-time.rfc3339.parse.offset] A numeric offset is the local time's offset from UTC, so the instant is the local time minus the offset. `-00:00` and `+00:00` mean UTC, as `Z` does.
5. r[std-time.rfc3339.parse.fraction] The fraction may have any number of digits. Its digits after the third are ignored, so the time is truncated to the millisecond.
6. r[std-time.rfc3339.parse.leap-day] February 29 is in range only in a leap year: a year divisible by 4, except one divisible by 100 but not by 400.
7. r[std-time.rfc3339.parse.leap-second] Second `60` is out of range.
8. r[std-time.rfc3339.parse.round-trip] For every timestamp `t` whose year is 0 to 9999, `Timestamp::parse_rfc3339(t.to_rfc3339())` is `.Ok(t)`.
9. r[std-time.rfc3339.parse.no-panic] `parse_rfc3339` never panics.

| Text | `parse_rfc3339` gives the milliseconds |
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

A failed parse reports one of three errors, each at a position:

```text
pub enum TimeParseError:
    InvalidCharacter(position: usize)
    OutOfRange(position: usize)
    TooShort(position: usize)
```

1. r[std-time.parse-error.declared] `std.time` declares the enum `TimeParseError` with the variants `InvalidCharacter`, `OutOfRange`, and `TooShort`, each with one field `position: usize`. Code imports it, as in `use std.time.TimeParseError`.
2. r[std-time.parse-error.position-bytes] A `position` is a byte offset into the text, counted in bytes of its UTF-8 encoding from 0, as [`slice`](../lang/10-modules.md#r-module.string.byte-offsets) counts.
3. r[std-time.parse-error.invalid-character] A character that the grammar does not allow at its place gives `InvalidCharacter` at that character. So does a character after a complete timestamp.
4. r[std-time.parse-error.out-of-range] A field whose digits are well formed but whose value is out of its range gives `OutOfRange` at the field's first digit.
5. r[std-time.parse-error.too-short] Text that ends before the timestamp is complete gives `TooShort`, whose `position` is the text's length.
6. r[std-time.parse-error.order] The text is read from the left, and the first error found wins. A field's range is checked as soon as its last digit is read.
7. r[std-time.parse-error.traits] `TimeParseError` implements `Eq`, `Debug`, and `Display`.

| Text | `parse_rfc3339` gives |
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
data Lease:
    holder: string
    granted_at: Timestamp
    term: Duration

fn record(lease: Lease) -> string:
    encode(lease)  # {"holder":"ada","granted_at":"2023-11-14T22:13:20Z","term":30000}
```

1. r[std-time.serde.impls] `Duration`, `Timestamp`, and `Instant` each implement `std.serde.Serialize` and `std.serde.Deserialize`.
2. r[std-time.serde.int-form] A `Duration` and an `Instant` each write one `int` holding their milliseconds, as `as_milliseconds()` gives.
3. r[std-time.serde.text-form] A `Timestamp` writes one text: its RFC 3339 text in UTC with milliseconds, the same text its `Display` gives.
4. r[std-time.serde.int-read] A `Duration` and an `Instant` each read one `int` with the type's name as `expected`, as in `"Duration"`, and hold that many milliseconds.
5. r[std-time.serde.text-read] A `Timestamp` reads one text with `"Timestamp"` as `expected`, and holds what [`Timestamp::parse_rfc3339`](#parsing-rfc-3339) accepts. A text the parser rejects is a decode error naming `Timestamp`.

> **Why.** A timestamp reads the way it displays, so JSON carries the same
> text a log line does. Durations and instants stay integers of
> milliseconds, which round trip exactly, and the private fields stay
> private to code.

> **Note.** The implementations let a `Clock` provider's `Timestamp` and
> `Instant` cross the host boundary into hd, and `sleep!`'s `Duration`
> cross out, by
> [`module.boundary.in`](../lang/10-modules.md#r-module.boundary.in).
