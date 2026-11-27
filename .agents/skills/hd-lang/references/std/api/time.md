# `std.time`

Generated from `hd doc std.time`. Run `hd doc std.time.NAME` in a package for one item's full text.

## `Duration`

```text
pub struct Duration
```

A distance in time, as a count of whole milliseconds, never negative.

- `pub fn milliseconds(count: i64) -> Duration`: Returns a `Duration` of that many milliseconds. A negative count panics.
- `pub fn seconds(count: i64) -> Duration`: Returns a `Duration` of that many seconds. A negative count panics.
- `pub fn checked_sub(self, rhs: Duration) -> Duration?`: The duration minus `rhs`, or `.None` when `rhs` is the longer one.
- `pub fn saturating_sub(self, rhs: Duration) -> Duration`: The duration minus `rhs`, or zero when `rhs` is the longer one.
- `pub fn as_milliseconds(self) -> i64`: The milliseconds since the origin that the instant holds.

## `ms`

```text
pub fn ms(count: i64) -> Duration
```

The `ms` suffix: a `Duration` of that many milliseconds, as in `250ms`.

## `s`

```text
pub fn s(count: i64) -> Duration
```

The `s` suffix: a `Duration` of that many seconds, as in `5s`.

## `minutes`

```text
pub fn minutes(count: i64) -> Duration
```

A `Duration` of `count` minutes; the `90min` literal uses this constructor.

## `h`

```text
pub fn h(count: i64) -> Duration
```

The `h` suffix: a `Duration` of that many hours, as in `2h`.

## `Clock`

```text
pub trait Clock
```

The host capability to read the clock and to wait: `now`, `monotonic`, and `sleep!`.

## `now`

```text
pub fn now() -> Timestamp $ Clock
```

Returns the current wall-clock time as a `Timestamp`.

## `monotonic`

```text
pub fn monotonic() -> Instant $ Clock
```

Returns the current reading of the monotonic clock as an `Instant`.

## `elapsed`

```text
pub fn elapsed(since: Instant) -> Duration $ Clock
```

Returns the time that has passed on the monotonic clock since `since`.

## `sleep`

```text
pub fn sleep!(duration: Duration) -> void $ Clock
```

Completes once `duration` has passed on the clock.

## `Timestamp`

```text
pub struct Timestamp
```

A point in UTC time, held as whole milliseconds since the Unix epoch.

- `pub fn from_unix_milliseconds(millis: i64) -> Timestamp`: Returns the timestamp that holds `millis` milliseconds since the Unix epoch.
- `pub fn unix_milliseconds(self) -> i64`: The milliseconds since the Unix epoch that the timestamp holds.
- `pub fn checked_since(self, earlier: Timestamp) -> Duration?`: The time from `earlier` to the timestamp, or `.None` when `earlier` is the later one.
- `pub fn since(self, earlier: Timestamp) -> Duration`: The time from `earlier` to the timestamp, or zero when `earlier` is the later one.
- `pub fn date(self) -> Date`: The calendar day of the timestamp in UTC.
- `pub fn to_rfc3339(self) -> string`: The timestamp as RFC 3339 text in UTC.
- `pub fn format(self, layout: string) -> string`: The timestamp as text in UTC, laid out by `%Y %m %d %H %M %S %z %%`.

## `Date`

```text
pub struct Date
```

A calendar day in UTC, in the proleptic Gregorian calendar.

- `pub year: i32`: The year, with a year 0.
- `pub month: i32`: The month, from 1 to 12.
- `pub day: i32`: The day of the month.
- `pub fn to_iso8601(self) -> string`: The date as ISO 8601 text, as `2026-10-17`.

## `ParseTimeError`

```text
pub enum ParseTimeError
```

Why time text failed to parse: the kind, and the byte position.

## `Instant`

```text
pub struct Instant
```

A reading of the monotonic clock, for measuring intervals.

- `pub fn from_milliseconds(millis: i64) -> Instant`: Returns the instant that holds `millis` milliseconds since the provider's origin.
- `pub fn as_milliseconds(self) -> i64`
- `pub fn checked_since(self, earlier: Instant) -> Duration?`: The time from `earlier` to the instant, or `.None` when `earlier` is the later one.
- `pub fn since(self, earlier: Instant) -> Duration`: The time from `earlier` to the instant, or zero when `earlier` is the later one.

## `ManualClock`

```text
pub struct ManualClock
```

The deterministic `Clock` provider: virtual time that a test advances.

- `pub fn new(start: Timestamp) -> *mut ManualClock`: Returns a `ManualClock` whose current time starts at `start`.
