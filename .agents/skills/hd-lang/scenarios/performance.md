# Performance: copies, text, release builds, hot spots

**Scenario.** A program is correct but slow, or you are porting code that
was fast in Go, Rust or Python and want it fast in hd. This page covers when
copies happen, how to avoid the ones that cost, building text, release
builds, compiler hints, and finding a hot spot without a profiler.

## When copies happen

`List`, `Map` and `string` are values with copy-on-write: assigning,
passing or returning one gives a logical copy that shares its elements
until one side changes. The first change then copies that side's elements,
once ([values and references](../reference/references.md)).

| You write | Cost |
| --- | --- |
| `b := a`, passing `a` to a function, returning it | none until `a` or `b` is changed |
| `for x in xs:` | none to begin; a change to `xs` inside the loop copies its elements once |
| `xs.push(x)` on a list nothing else shares | amortized constant |
| `before := xs`, then `xs.push(x)`, repeated in a loop | the whole list copies at each push |
| `xs.slice(a, b)`, `xs.take(n)`, `s.slice(a, b)` | a new value, as long as the range |
| `xs.view(a, b)` | none: a read-only window on the elements |
| a struct, tuple or enum passed by value | its fields copy; lists and maps inside share |

Passing a 20000-element list by value 2000 times took the same time as
lending it with `&`. The cost is keeping an old copy alive while you change
the new one.

The usual quadratic loop is repeated slicing, ported from `xs = xs[1:]` in
Python or `s = s[1:]` in Go. Each turn builds a new list. Index into the
list instead, or read a window with `view`:

```hd
use std.meta.{cold, inline}
use std.text.StringBuilder
use std.time.{Clock, elapsed, monotonic}

# Runs only on bad input: keep it out of the callers.
@cold
fn bad_row(at: usize) -> string:
    "bad row at $at"

# A tiny helper called in a hot loop: ask for a copy at each call.
@inline
fn square(x: i64) -> i64:
    x * x

fn sum_with_slices(items: List[i32]) -> i64:
    let mut rest = items
    let mut total: i64 = 0
    while !rest.is_empty():
        total += i64(rest[0])
        rest = rest.slice(1, rest.len())    # a new list each turn
    total

fn sum_by_index(items: &List[i32]) -> i64:
    let mut total: i64 = 0
    let mut at = 0
    while at < items.len():
        total += i64(items[at])
        at += 1
    total

pub fn main() -> void $ Clock + Console:
    data := [for i in 0..20000 => i32(i)]
    started := monotonic()
    slow := sum_with_slices(data)
    slow_time := elapsed(started)
    started2 := monotonic()
    fast := sum_by_index(&data)
    fast_time := elapsed(started2)
    println("same result: ${slow == fast}")
    println("slices slower: ${slow_time > fast_time}")
    out := StringBuilder::new()
    for i in 0..3:
        out.push("${square(i64(i))} ")
    out.push(bad_row(7))
    println(out.build())
```

```output
same result: true
slices slower: true
0 1 4 bad row at 7
```

Here the slice loop took 320 ms for 20000 elements and 1250 ms for 40000.
The index loop took under 1 ms for both. Four times the time for double the
input is the sign of a copy in a loop.

## Lend or box

| Situation | Write |
| --- | --- |
| a function reads a large list, map or struct | take `&T`: no copy, and the caller keeps its value |
| a function fills or edits the caller's collection | take `&mut T`, call with `&mut x` |
| an object that several owners change, or that outlives a call | `box(value)`, a `*mut T` |

`for x in &xs` reads each element in place; `for x in &mut xs` edits each in
place. Do not box for speed: a box shares the object, which changes what the
program means ([shared state](shared-state.md)).

## Building text

`s += piece` on a string you own is amortized constant, and so is
`s = s + piece`. Rebuilding the string from itself is not:

```text
let mut s = ""
for i in 0..40000:
    s = "$s line $i\n"      # copies all of s each turn: 165 ms
```

The same 40000 pieces took 2 ms with `+=`, with `StringBuilder`, and with
`parts.join("")`. Use `std.text.StringBuilder` when several functions add
to one text: `StringBuilder::new()` returns a shared `*mut`, `push` adds,
and `build()` returns the text and empties the builder (see `out` in the
example above). `hd doc std.text.StringBuilder` lists the other methods.

## Release builds

| Command | Overflow | `require` |
| --- | --- | --- |
| `hd FILE.hd`, `hd run`, `hd build` | panics | checked |
| `hd --release FILE.hd`, `hd run --release`, `hd build --release` | wraps | skipped |
| `hd test`, `hd test --release` | panics | checked |

`hd test` is always a checked build; `--release` there only selects the
optimizing pipeline. `ensure` is checked in every build
([contracts](../design/contracts.md)). In release, `2147483647 + 1` on an
`i32` gives `-2147483648`, and a failing `require` lets the call go on.

Test in the default profile and use `--release` for the run that matters.
Where wrap-around is intended, as in a hash, write `wrapping_mul` and the
other `wrapping_` methods instead of relying on release.

## Hints: `@inline` and `@cold`

`std.meta` exports `inline` and `cold`, not in the prelude. They change how
the compiler compiles calls, never what the program prints or whether it
panics. The first example above uses both.

- `@inline` or `@inline(.Auto)`: copy a small body into each known call.
- `@inline(.Always)`: copy it at every call. The function must not suspend,
  make a closure, have a requirement row, or use `with`.
- `@inline(.Never)`: never copy it, as for a long loop.
- `@cold`: rarely called, such as an error message builder; never copied.

They go before a function or method with a body. Use them only after a
measurement.

## Finding a hot spot without a profiler

There is no profiler yet ([limitations](../reference/limitations.md)).
Time sections with the monotonic clock. Reading it needs `$ Clock`, so
time in `main` or add `Clock` to the row:

```text
started := monotonic()
rows := parse_all(text)
println("parse: ${elapsed(started)}")      # prints as 36ms or 2s
```

Time the whole run, then each stage, then the loops inside the slowest
stage. `elapsed(started).as_milliseconds()` gives an `i64`. `ManualClock`
only moves when a test advances it, so measure in a real run.

To cap a run that may never end, add `--time-limit 10s` to `hd FILE.hd`,
`hd run` or `hd test`. Past it, the program panics with `time-limit`. Halve
the input to see whether the time grows with it, then time the stages.
