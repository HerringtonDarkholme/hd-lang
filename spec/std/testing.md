# Testing

Status: standard library specification draft.

This chapter defines the part of `std.testing` that `lib/std` and the test
runner implement over the language tier:

- how a property test draws, discards, reports, and replays its inputs;
- the draw budget;
- what the `timeout` option does;
- how an `it_each` call expands and names its rows;
- how snapshots compare text, and where snapshot files live.

The language tier keeps what the compiler checks. That is the assertion
functions, `it` and its options, the signatures and registration of
`it_each`, `it_prop`, and `it_prop_with`, and the literal `expect` of
`snapshot` ([Standard Testing](../10-modules.md#standard-testing),
[Table Tests](../10-modules.md#table-tests),
[Snapshots](../10-modules.md#snapshots)).

## Property Tests

A property test draws its inputs from a `Choices` source, the only
randomness a generator sees:

```text
trait Arbitrary:
    fn arbitrary(c: mut Choices) -> Self
```

| Rule | Member of `Choices` | Draws |
| --- | --- | --- |
| r[std-testing.choices.int-generic] Integer | `fn int[N < Integer](mut self, lo: N, hi: N) -> N` | an integer from `lo` to `hi` |
| r[std-testing.choices.float-generic] Float | `fn float[F < Float](mut self, lo: F, hi: F) -> F` | a finite float from `lo` to `hi` |
| r[std-testing.choices.bool] Boolean | `fn bool(mut self) -> bool` | a `bool` |
| r[std-testing.choices.pick] Pick | `fn pick[T](mut self, items: List[T]) -> T` | one of `items`; earlier items shrink first |
| r[std-testing.choices.list] List | `fn list[T](mut self, max: i32, item: fn(mut Choices) -> T) -> List[T]` | at most `max` items, each drawn by `item` |
| r[std-testing.choices.map] Map | `fn map[K < Eq & Hash, V](mut self, max: i32, key: fn(mut Choices) -> K, value: fn(mut Choices) -> V) -> Map[K, V]` | at most `max` entries, each key drawn by `key` and its value by `value` |
| r[std-testing.choices.string-chars] String | `fn string(mut self, max_chars: i32) -> string` | a string of at most `max_chars` chars |
| r[std-testing.choices.assume] Assume | `fn assume(mut self, ok: bool) -> void` | nothing; a false `ok` discards the case |
| r[std-testing.choices.draw] Draw | `fn draw[T < Arbitrary](mut self) -> T` | `T::arbitrary(self)`, the type's default |

1. r[std-testing.choices.declare] `std.testing` declares `Choices`, `Arbitrary`, `it_prop`, and `it_prop_with`. None is a prelude name.
2. r[std-testing.choices.runner] The runner creates every `Choices`. It records each draw, so the runner can replay and shrink a case.
3. r[std-testing.choices.no-size] `Choices` has no size: no member reads or sets one, and no option of `it_prop` or `it_prop_with` sets one.
4. r[std-testing.choices.string-limit] The limit of `string` counts `char` values, not bytes.
5. r[std-testing.choices.map.duplicate] When `key` draws a key that the map already holds, the later value replaces the earlier one. So the map may hold fewer entries than were drawn.
6. r[std-testing.arbitrary] `Arbitrary` gives a type its default generator, which `it_prop` and `Choices.draw` use.
7. r[std-testing.arbitrary.std] `std` implements `Arbitrary` for the primitives, `string`, `List[T]`, `Map[K, V]`, `T?`, `Result[T, E]`, and tuples, each when its type arguments implement it.
8. r[std-testing.arbitrary.float] The `Arbitrary` implementations of `f32` and `f64` draw any value of the type, including NaN, both infinities, `-0.0`, and subnormal values, as Hypothesis's `floats()` does.
9. r[std-testing.prop.debug] `it_prop` and `it_prop_with` require `T < Debug`. A property whose input type does not implement `Debug` is an error. Error: `unsatisfied-trait-bound`.
10. r[std-testing.prop.report] When a property test fails, the runner prints the shrunk input with `Debug`.
11. r[std-testing.prop.examples] Each input in `examples` runs first on every run, before the saved regression streams and the generated cases.
12. r[std-testing.prop.discard] A case that `assume` discards does not count toward `cases`. The runner generates another case in its place.
13. r[std-testing.prop.body-no-discard] Only a generator discards a case, through `Choices.assume`. A property body has no `Choices`, so it cannot discard one.
14. r[std-testing.prop.discard-limit] A property test fails when more than 10 times `cases` of its cases are discarded, as Hypothesis's `filter_too_much` health check does.
15. r[std-testing.prop.regression-file] The runner saves a failing property's shrunk choice stream in `<package root>/__regressions__/<module>/<test-slug>`. `<module>` and `<test-slug>` are as for a [snapshot file](#snapshot-files).
16. r[std-testing.prop.regression-format] The file holds the stream's draws in order, one decimal number per line.
17. r[std-testing.prop.regression-replay] On the next run, the runner replays a property's saved stream before it generates new cases.

```text
use std.testing.{Arbitrary, Choices}

data Point:
    x: i64
    y: i64

impl Arbitrary for Point:
    fn arbitrary(c: mut Choices) -> Point:
        Point { x: c.int(0, 9), y: c.int(-5, 5) }

fn small_counts(c: mut Choices) -> List[i64]:
    c.list(3, fn(inner: mut Choices) -> i64: inner.int(0, 10))

fn label(c: mut Choices) -> string:
    c.string(max_chars=8)

fn stock(c: mut Choices) -> Map[string, i32]:
    c.map(5, key=label, value=fn(inner: mut Choices) -> i32: inner.int(0, 99))
```

```text
use std.testing.{Arbitrary, Choices, it_prop}

data Reading:
    level: i64

impl Arbitrary for Reading:
    fn arbitrary(c: mut Choices) -> Reading:
        Reading { level: c.int(0, 9) }

tests:
    it_prop("levels stay small", prop=fn!(reading: Reading):  # error: unsatisfied-trait-bound
        pass
    )
```

> **Why.** A generator draws the parts of its value in order, so a later
> draw may depend on an earlier one. The runner shrinks the recorded draws,
> not the value, so it needs no size.

## Draw Budget

1. r[std-testing.budget] Each case has a draw budget. Once the case's draws have spent it, every draw returns its simplest value.
2. r[std-testing.budget.no-api] No member of `Choices` reads or changes the budget.
3. r[std-testing.budget.every-draw] The budget applies to every draw from the case's `Choices`, including a hand-written generator's. So a recursive generator whose simplest draws choose a leaf ends.

| Rule | Draw | Simplest value |
| --- | --- | --- |
| r[std-testing.budget.simplest.int] Integer | `int` and the default integer generators | `0`, or the bound nearest `0` when `0` is out of range |
| r[std-testing.budget.simplest.float] Float | `float` and the default `f32` and `f64` generators | `0.0`, or the bound nearest `0.0` when `0.0` is out of range |
| r[std-testing.budget.simplest.bool] Boolean | `bool` | `false` |
| r[std-testing.budget.simplest.pick] Pick | `pick` | the first item |
| r[std-testing.budget.simplest.empty] Collections | `list`, `map`, and `string` | an empty list, map, or string |
| r[std-testing.budget.simplest.optional] Optional | the default `T?` generator | `.None` |
| r[std-testing.budget.simplest.result] Result | the default `Result[T, E]` generator | `.Ok` of `T`'s simplest value |
| r[std-testing.budget.simplest.tuple] Tuple | the default tuple generators | each element's simplest value |

```text
use std.testing.Choices

enum Tree:
    Leaf
    Node(left: Tree, right: Tree)

fn tree(c: mut Choices) -> Tree:
    match c.int(0, 2):
        0 => .Leaf
        _ => .Node(tree(c), tree(c))
```

Once the budget is spent, `c.int(0, 2)` returns `0`, so `tree` returns
`.Leaf`.

## Test Timeout

`it`, `it_each`, `it_prop`, and `it_prop_with` each take a `timeout`
option of type `Duration?` ([Test Cases](../10-modules.md#test-cases)).

| Rule | Option | Value | Effect |
| --- | --- | --- | --- |
| r[std-testing.option.timeout-any-duration] Timeout | `timeout` | any `std.time.Duration` value, such as `5s` or a call that returns one | The runner fails the test case when its body runs longer than the duration. |

1. r[std-testing.option.timeout-at-run] A `timeout` value is an ordinary argument, not a literal. It is evaluated when the test case runs, in its program instance, as [`it_each` rows](#r-std-testing.it-each.rows-at-run) are.
2. r[std-testing.option.timeout-import] A suffix in a `timeout` value is imported like any other, as in `use std.time.s`; `it` adds no suffix of its own.

```text
use std.time.{Duration, s}

fn budget() -> Duration: 30s

tests:
    it("fetches the index", timeout=5s):
        pass

    it("loads the archive", timeout=budget()):
        pass
```

## Table-Test Rows

A call of `it_each` in test position is registered as the language tier
specifies ([Table Tests](../10-modules.md#table-tests)).

1. r[std-testing.it-each] A top-level call of `std.testing.it_each` registers one test case for each element of `rows`, which runs `body` with that element.
2. r[std-testing.it-each.name] The test case for the element at index `i` is named `name[i]`.
3. r[std-testing.it-each.rows-at-run] `rows` is evaluated when the test runs, in its program instance, not when test cases are listed.

```text
use std.testing.{assert_equal, it_each}

fn double(value: i32) -> i32: value * 2

tests:
    it_each("doubles", [1, 2, 3], body=fn!(value: i32):
        assert_equal(double(value), value + value, reason="doubling adds the value to itself")
    )
```

The call runs the test cases `doubles[0]`, `doubles[1]`, and `doubles[2]`.

## Snapshot Files

`std.testing` declares a second snapshot function, beside `snapshot`
([Snapshots](../10-modules.md#snapshots)):

```text
pub fn snapshot_file(text: string) -> void
```

1. r[std-testing.snapshot] `snapshot` compares `text` with `expect`, the expected text written in the source.
2. r[std-testing.snapshot-file] `snapshot_file` compares `text` with a snapshot file, which the test runner names from the running test case.
3. r[std-testing.snapshot.import] Neither is a prelude name; code imports them from `std.testing`.
4. r[std-testing.snapshot-file.path] `snapshot_file` keeps its file at `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`, whose parts the table below defines.
5. r[std-testing.snapshot-file.missing] When that file does not exist, the test case fails, except in an update run, as `hd test --update` makes, which records the file.
6. r[std-testing.snapshot.mismatch] When `text` differs from the expected text, `snapshot` or `snapshot_file` fails as a failed assertion does. Panic: `assertion-failed`.
7. r[std-testing.snapshot-file.missing-panic] A missing snapshot file outside an update run fails the same way. Panic: `assertion-failed`.

| Rule | Part | Value |
| --- | --- | --- |
| r[std-testing.snapshot-file.folder] Folder | `__snapshots__/` | one folder at the package root, beside `hd.toml`; it has no `mod.hd`, so it is never a module |
| r[std-testing.snapshot-file.module] Module | `<module>` | the test's module path, such as `billing`; a module under `tests/` is `tests.<name>` |
| r[std-testing.snapshot-file.slug] Slug | `<test-slug>` | the test case name, lowercased, with each run of characters other than ASCII letters and digits turned into `-` |
| r[std-testing.snapshot-file.counter] Counter | `<n>` | the count of `snapshot_file` calls within one test case run, from 1 |
| r[std-testing.snapshot-file.row] Table row | `<test-slug>.<i>` | the slug of an `it_each` row adds the row's index |

> **Why.** The test picks the rendering, such as `debug(value)`. A missing
> snapshot file fails outside an update run, so a test that was never
> recorded cannot pass by accident.

See also: [Debug Trait](../09-traits.md#debug-trait).
