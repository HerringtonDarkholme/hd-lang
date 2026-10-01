# Time

Status: standard library specification draft.

This chapter defines the part of `std.time` that `lib/std` writes in
ordinary hd over the language tier:

- the `Duration` type and its public API;
- the duration suffixes `ms`, `s`, `min`, and `h`.

The language tier keeps the suffix mechanism
([Literal Suffixes](../05-expressions.md#literal-suffixes)):

| Item | Why it stays in the language tier |
| --- | --- |
| `@num_suffix`, `std.ops.NumSuffix` | the compiler recognizes the marker by its qualified name |
| a suffixed literal as a call | [`expr.suffix.fn-call`](../05-expressions.md#r-expr.suffix.fn-call), a language rule, turns `250ms` into `ms(250)` |
| suffix name lookup and typing | [Literal Suffix Names](../03-names-and-scopes.md#literal-suffix-names) and [Suffixed Literals](../04-type-system.md#suffixed-literals) hold for every suffix |
| `Duration?` in the test signature | `it` names it as its `timeout` type ([Test Cases](../10-modules.md#test-cases)); the stdlib-tier `it_each`, `it_prop`, and `it_prop_with` name it too |

The duration suffixes are ordinary functions marked `@num_suffix`, so
nothing in the language tier names `ms`, `s`, `min`, or `h`. What the
`timeout` option does is stdlib tier as well:
[Test Timeout](testing.md#test-timeout).

## Duration

1. r[std-time.suffix.std.duration] A `std.time.Duration` is a whole number of milliseconds, held in an `i64`.
2. r[std-time.suffix.std.duration-api] The public API of `Duration` is `Duration::milliseconds(n: i64)`, `Duration::seconds(n: i64)`, and `d.as_milliseconds() -> i64`.
3. r[std-time.suffix.std.duration-neg] `Duration` implements `std.ops.Neg` with `Out = Duration`, negating its milliseconds. So `-5s` is the ordinary negation `-(5s)`, minus five seconds.
4. r[std-time.prelude.time-suffixes] `std.time` declares `Duration` and the duration suffixes `ms`, `s`, `min`, and `h`, which code imports, as in `use std.time.{Duration, s}`.

```text
use std.time.{Duration, s}

fn rewind() -> Duration:
    -5s  # -(s(5)), through Neg for Duration
```

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

See also: [Literal Suffixes](../05-expressions.md#literal-suffixes),
[Test Timeout](testing.md#test-timeout).
