# Testing

Status: standard library specification draft.

This chapter defines the part of `std.testing` that `lib/std` and the test
runner implement over the language tier:

- how a property test draws, discards, reports, and replays its inputs;
- the draw limit;
- how `@derive(Arbitrary)` builds a type's default generator;
- what the `timeout` option does;
- how an `it_each` call expands and names its rows;
- how snapshots compare text, and where snapshot files live;
- how an integration test runs one of the package's executables or tasks;
- the temporary directory of a test case;
- which std fakes stand in for host providers in a unit test;
- the host capabilities through which `std.testing` reaches the test runner.

The language tier keeps the assertion functions and the literal `expect` of
`snapshot`. It also keeps `it`, `it_each`, `it_prop`, and `it_prop_with`,
the functions a test registration call may call. Their options, position rules, and
diagnostics stay in the language tier too
([Standard Testing](../lang/10-modules.md#standard-testing),
[Test Cases](../lang/10-modules.md#test-cases),
[Registration Calls](../lang/10-modules.md#registration-calls),
[Snapshots](../lang/10-modules.md#snapshots)).

## Registration Calls

The language tier declares the functions of the test registration calls
`it_each`, `it_prop`, and `it_prop_with`, and checks their names, options,
and positions ([Registration Calls](../lang/10-modules.md#registration-calls)).
This chapter defines what their test cases do when they run.

> **Note.** The `timeout` parameter of each has the type that `it`'s has,
> `std.time.Duration?` ([Test Timeout](#test-timeout)).

See also: [Table-Test Rows](#table-test-rows), for how an `it_each` call
expands and names its rows. [Property Tests](#property-tests),
[Draw Limit](#draw-limit), and [Derived Arbitrary](#derived-arbitrary)
show how the runner generates a property's inputs.

## Property Tests

A property test draws its inputs from a `Choices` source, the only
randomness a generator sees:

```text
trait Arbitrary:
    fn arbitrary(c: &mut Choices) -> Self
```

| Rule | Member of `Choices` | Draws |
| --- | --- | --- |
| r[std-testing.choices.int-range] Integer | `fn int[N < Integer](&mut self, range: Range[N]) -> N` | an integer of `range` |
| r[std-testing.choices.float-range] Float | `fn float[F < Float](&mut self, range: Range[F]) -> F` | a finite float of `range` |
| r[std-testing.choices.bool] Boolean | `fn bool(&mut self) -> bool` | a `bool` |
| r[std-testing.choices.choose] Choose | `fn choose[T](&mut self, items: List[T]) -> T` | one of `items`; earlier items shrink first |
| r[std-testing.choices.list-max-ref] List | `fn list[T](&mut self, max: usize, item: fn(&mut Choices) -> T) -> List[T]` | at most `max` items, each drawn by `item` |
| r[std-testing.choices.map-max-ref] Map | `fn map[K < Eq & Hash, V](&mut self, max: usize, key: fn(&mut Choices) -> K, value: fn(&mut Choices) -> V) -> Map[K, V]` | at most `max` entries, each key drawn by `key` and its value by `value` |
| r[std-testing.choices.string-max-chars] String | `fn string(&mut self, max_chars: usize) -> string` | a string of at most `max_chars` chars |
| r[std-testing.choices.assume] Assume | `fn assume(&mut self, ok: bool) -> void` | nothing; a false `ok` discards the case |
| r[std-testing.choices.draw] Draw | `fn draw[T < Arbitrary](&mut self) -> T` | `T::arbitrary(self)`, the type's default |

1. r[std-testing.choices.declare-ref] `std.testing` declares `Choices` and `Arbitrary`. Neither is a prelude name. `Choices` is a struct that a generator takes behind `&mut`, as `&mut Choices`, and each member above takes `&mut self`, so every draw advances the one source.
2. r[std-testing.choices.from-case] `std.testing` creates the `Choices` of each generated case from the [`PropertyCase`](#runner-capabilities) that the runner returns. It reports each draw to the runner, so the runner can replay and shrink a case.
3. r[std-testing.choices.no-size] `Choices` has no size: no member reads or sets one, and no option of `it_prop` or `it_prop_with` sets one.
4. r[std-testing.choices.string-limit] The limit of `string` counts `char` values, not bytes.
5. r[std-testing.choices.map.duplicate] When `key` draws a key that the map already holds, the later value replaces the earlier one. So the map may hold fewer entries than were drawn.
6. r[std-testing.choices.choose.empty] `choose` on an empty list panics. Panic: `explicit-panic`.
7. r[std-testing.choices.int-range.empty] An `int` range that holds at most one value, such as `5..5`, `5..=5`, or `6..=5`, returns its `start` and records no draw.
8. r[std-testing.choices.int-range.convert] `int` works through the [`Integer`](num.md#r-std-num.u64.members) conversions, so any integer type is a valid `N`, and the range may span the whole type, `0..=255` for `u8` or `0..=18446744073709551615` for `u64`. It never converts a value to text.
9. r[std-testing.choices.float-range.end] A `float` range whose `inclusive` is `false` never returns its `end`, and one whose `inclusive` is `true` may. A range whose `start` is not below its `end` returns `start` and records no draw.
10. r[std-testing.arbitrary] `Arbitrary` gives a type its default generator, which `it_prop` and `Choices.draw` use.
11. r[std-testing.arbitrary.std] `std` implements `Arbitrary` for the primitives, `string`, `List[T]`, `Map[K, V]`, `T?`, `Result[T, E]`, and tuples, each when its type arguments implement it.
12. r[std-testing.arbitrary.reference] `std` also implements `Arbitrary` for `&T` and `&mut T` when `T` implements it. Each draws a `T` and refers to a new object that holds it.
13. r[std-testing.arbitrary.reference.static] Those two implementations bound `T` by `'static`, since the new object keeps the drawn value ([Generic Locality](../lang/13-values-and-references.md#generic-locality)).
14. r[std-testing.arbitrary.float] The `Arbitrary` implementations of `f32` and `f64` draw any value of the type, including NaN, both infinities, `-0.0`, and subnormal values, as Hypothesis's `floats()` does.
15. r[std-testing.prop.generate-and-shrink] The runner generates inputs for `it_prop` and `it_prop_with`, shrinking a failure only after observing it in a fresh instance.
16. r[std-testing.prop.failure-input] When a property test fails, the runner prints its failing input with `Debug`.
17. r[std-testing.prop.examples] Each input in `examples` runs first on every run, before the saved regression streams and the generated cases.
18. r[std-testing.prop.discard] A case that `assume` discards does not count toward `cases`. The runner generates another case in its place.
19. r[std-testing.prop.body-no-discard] Only a generator discards a case, through `Choices.assume`. A property body has no `Choices`, so it cannot discard one.
20. r[std-testing.prop.discard-limit] A property test fails when more than 10 times `cases` of its cases are discarded, as Hypothesis's `filter_too_much` health check does.
21. r[std-testing.prop.regression-file] The runner saves a failing property's shrunk choice stream in `<package root>/__regressions__/<module>/<test-slug>`. `<module>` and `<test-slug>` are as for a [snapshot file](#snapshot-files).
22. r[std-testing.prop.regression-format] The file holds the stream's draws in order, one decimal number per line.
23. r[std-testing.prop.regression-replay] On the next run, the runner replays a property's saved stream before it generates new cases.
24. r[std-testing.prop.shared-instance] Within a property test, each case reuses the preceding case's instance when that case completed successfully.
25. r[std-testing.prop.shrink-fresh] Each shrink attempt runs in a fresh program instance.
26. r[std-testing.prop.state-dependent-failure] A failure absent from fresh-instance replay fails with its input, seed, and explanation of earlier cases' state, without shrinking or saving a regression.

```text
use std.testing.{Arbitrary, Choices}

struct Point:
    x: i64
    y: i64

impl Arbitrary for Point:
    fn arbitrary(c: &mut Choices) -> Point:
        Point { x: c.int(0..=9), y: c.int(-5..=5) }

fn small_counts(c: &mut Choices) -> List[i64]:
    c.list(3, fn(inner: &mut Choices) -> i64: inner.int(0..=10))

fn ratio(c: &mut Choices) -> f64:
    c.float(0.0..=1.0)

fn label(c: &mut Choices) -> string:
    c.string(max_chars=8)

fn stock(c: &mut Choices) -> Map[string, i32]:
    c.map(5, key=label, value=fn(inner: &mut Choices) -> i32: inner.int(0..=99))
```

```text
use std.testing.{Arbitrary, Choices, it_prop}

struct Reading:
    level: i64

impl Arbitrary for Reading:
    fn arbitrary(c: &mut Choices) -> Reading:
        Reading { level: c.int(0..=9) }

tests:
    it_prop("levels stay small", prop=fn!(reading: Reading):  # error: unsatisfied-trait-bound
        pass
    )
```

A property's input type must implement `Debug`, by
[`module.testing.reg.prop-debug`](../lang/10-modules.md#r-module.testing.reg.prop-debug),
so the example above is an error.

> **Why.** A generator draws the parts of its value in order, so a later
> draw may depend on an earlier one. The runner shrinks the recorded draws,
> not the value, so it needs no size.

## Draw Limit

1. r[std-testing.limit] Each case has a draw limit. Once the case's draws have spent it, every draw returns its simplest value.
2. r[std-testing.limit.no-api] No member of `Choices` reads or changes the limit.
3. r[std-testing.limit.every-draw] The limit applies to every draw from the case's `Choices`, including a hand-written generator's. So a recursive generator whose simplest draws choose a leaf ends.

| Rule | Draw | Simplest value |
| --- | --- | --- |
| r[std-testing.limit.simplest.int] Integer | `int` and the default integer generators | `0`, or the bound nearest `0` when `0` is out of range |
| r[std-testing.limit.simplest.float] Float | `float` and the default `f32` and `f64` generators | `0.0`, or the bound nearest `0.0` when `0.0` is out of range |
| r[std-testing.limit.simplest.bool] Boolean | `bool` | `false` |
| r[std-testing.limit.simplest.choose] Choose | `choose` | the first item |
| r[std-testing.limit.simplest.empty] Collections | `list`, `map`, and `string` | an empty list, map, or string |
| r[std-testing.limit.simplest.optional] Optional | the default `T?` generator | `.None` |
| r[std-testing.limit.simplest.result] Result | the default `Result[T, E]` generator | `.Ok` of `T`'s simplest value |
| r[std-testing.limit.simplest.tuple] Tuple | the default tuple generators | each element's simplest value |

```text
use std.testing.Choices

enum Tree:
    Leaf
    Node(left: *Tree, right: *Tree)

fn tree(c: &mut Choices) -> Tree:
    match c.int(0..=2):
        0 => .Leaf
        _ => .Node(box(tree(c)), box(tree(c)))
```

Once the limit is spent, `c.int(0..=2)` returns `0`, so `tree` returns
`.Leaf`.

## Edge Values

The first generated cases of a property run draw edge values. So a
property meets `0`, the bounds of a range, the smallest and largest value
of an integer type, and an empty collection early. Each edge case takes
one edge, which its seed picks:

| Rule | Edge | `Choices.int(range)` returns | A default integer generator returns | `list`, `map`, and `string` return |
| --- | --- | --- | --- | --- |
| r[std-testing.edge.low] Seed modulo 4 is 0: low | the range's first value | the smallest value of the type | an empty collection |
| r[std-testing.edge.high] Seed modulo 4 is 1: high | the range's last value | the largest value of the type | any collection |
| r[std-testing.edge.zero] Seed modulo 4 is 2: zero | `0` when the range holds `0`, else any value | `0` | any collection |
| r[std-testing.edge.none] Seed modulo 4 is 3: none | any value | any value | any collection |

1. r[std-testing.edge.cases] A generated case is an edge case when its `size` is below 4. Every other case takes no edge.
2. r[std-testing.edge.plan] An edge case takes the edge of its seed modulo 4 in the table above, for each fresh draw it makes.
3. r[std-testing.edge.replay] An edge changes fresh draws only. A replayed draw returns its recorded value, by [`std-testing.runner.replay`](#r-std-testing.runner.replay).
4. r[std-testing.edge.coverage] When a run discards no case, its first four generated cases take the low, high, zero, and no edge once each. This holds because their seeds are consecutive.
5. r[std-testing.edge.int-type] A default generator of an integer type returns every value from the type's smallest to its largest. So the low and high edges reach the type's own bounds. For `i32`, `i64`, `u32`, `u64`, and `usize` they may reach them outside an edge case as well.
6. r[std-testing.edge.limit] A spent draw limit still returns the simplest value, by [`std-testing.limit.simplest.int`](#r-std-testing.limit.simplest.int), whatever the edge.

```text
use std.testing.{assert, Choices, it_prop_with}

fn span(c: &mut Choices) -> i64:
    c.int(-10..=10)

tests:
    # the first generated cases draw the edges: -10, 10, and 0
    it_prop_with("the span holds", gen=span, cases=8, prop=fn!(n: i64):
        assert(n >= -10 && n <= 10)
    )
```

> **Why.** Hypothesis and QuickCheck bias draws toward `0`, the bounds,
> and empty collections, since off-by-one and overflow bugs sit there. A
> property that never sees the largest `i32` cannot find an overflow.
> The seed picks the edge, and seeds are consecutive, so four cases cover
> the edges without a counter, and a discarded case's retry still varies.

## Derived Arbitrary

`@derive(Arbitrary)` gives a struct or enum its default generator
through the template of `Arbitrary`.

1. r[std-testing.arbitrary.derive] `@derive(Arbitrary)` derives `Arbitrary` through its [template](../lang/14-annotations.md#templates). The derived `arbitrary` draws each member with its type's `Arbitrary`. For an enum, it draws a variant, then that variant's payload.
2. r[std-testing.arbitrary.derive.template] That template is ordinary `std.testing` code over `std.structure`: it reads each variant's and member's [`self_ref`](../lang/14-annotations.md#self-references), and the compiler supplies nothing for `Arbitrary` itself.
3. r[std-testing.arbitrary.derive.member-bound] The template requires the type of every member to implement `Arbitrary`, whether or not `arbitrary.with` tunes the member. It does not require a member to be [inspectable](../lang/09-traits.md#inspectable-types).
4. r[std-testing.arbitrary.derive.params-arbitrary] For a generic type, the derived implementation gets `T < Arbitrary` for each type parameter `T` that a member's type uses, by [`annot.bound.params`](../lang/14-annotations.md#r-annot.bound.params). So `@derive(Arbitrary)` on `struct Box[T]` with a member `value: T` needs no hand-written block.
5. r[std-testing.arbitrary.derive.not-derivable] A type with a member whose type fails that bound, such as a function-typed member, is not derivable. `@derive(Arbitrary)` on it is an error, reported at the opt-in and naming the member. Error: `member-not-derivable`.
6. r[std-testing.arbitrary.derive.manual] Such a type gets its default generator only from a hand-written `impl Arbitrary`.
7. r[std-testing.arbitrary.derive.simplest] A derived enum's simplest choice is its first non-recursive variant, whatever the declaration order.
8. r[std-testing.arbitrary.derive.recursive] A variant is recursive when its `self_ref` is `.Required`, as [Self References](../lang/14-annotations.md#self-references) computes it from the member types.
9. r[std-testing.arbitrary.derive.recursive.containers] A `List`, `Map`, or optional member does not make its variant recursive. Its `self_ref` is at most `.Optional`: its simplest value is empty or `.None`.
9. r[std-testing.arbitrary.derive.no-finite] When every variant of a derived enum is recursive, the derived `arbitrary` panics on the property's first case, with a message that names the type. Panic: `explicit-panic`.
10. r[std-testing.arbitrary.derive.no-finite.message] The message is `"${T::name()} has no finite value"`, where [`T::name()`](../lang/14-annotations.md#r-annot.structure.name) is the type's declared name.
11. r[std-testing.arbitrary.derive.no-finite.data] When a member of a derived struct has `self_ref` `.Required`, its derived `arbitrary` panics the same way. The compiler does not reject the type either. Panic: `explicit-panic`.
12. r[std-testing.arbitrary.derive.no-finite.unchecked] The compiler does not reject such an enum, because no derivation check reports it.
13. r[std-testing.arbitrary.with] A member whose facts hold an `arbitrary.with(gen)` value is drawn by `gen` instead of its type's `Arbitrary`.
14. r[std-testing.arbitrary.with.module-typed-struct] The module `std.testing.arbitrary` declares `with` and its result type `With[F]`, a struct value and a [typed fact type](../lang/14-annotations.md#member-typed-facts), as shown below. Code imports the module, as in `use std.testing.arbitrary`, and writes `@arbitrary.with(gen)`.
15. r[std-testing.arbitrary.with.checked] `gen` must draw the member's declared type. A generator of another type is an error, reported on its decorator, by [`annot.typed-fact.check`](../lang/14-annotations.md#r-annot.typed-fact.check). Error: `type-mismatch`.
16. r[std-testing.arbitrary.with.typed-read] The derived `arbitrary` reads the member's `With[F]` through its handle, with [`h.fact`](../lang/14-annotations.md#r-annot.handle.fact.typed), and draws the member by its `gen`.
17. r[std-testing.arbitrary.with.only] `arbitrary.with` is the only fact that derived `Arbitrary` reads.

```text
@annotate::[F](.Field)
pub struct With[F]:
    pub gen: fn(&mut Choices) -> F

pub fn with[F](gen: fn(&mut Choices) -> F) -> With[F]
```

> **Note.** The member's declared type is the expected type of `F`, by
> [`annot.typed-fact.check.inferred`](../lang/14-annotations.md#r-annot.typed-fact.check.inferred).
> So a generic generator such as `fn any_text[T](c: &mut Choices) -> T`
> needs no type argument: `@arbitrary.with(any_text)` on `name: string`
> solves `T = string`.

```text
use std.testing.{Arbitrary, Choices, assert, it_prop}
use std.testing.arbitrary

fn cents(c: &mut Choices) -> i32:
    c.int(0..=10_000)

@derive(Arbitrary, Debug)
struct Item:
    name: string
    @arbitrary.with(cents)
    price: i32

@derive(Arbitrary, Debug)
enum Expr:
    Add(left: *Expr, right: *Expr)
    Num(value: i32)

tests:
    it_prop("prices are never negative", examples=[Item { name: "", price: 0 }], prop=fn!(item: Item):
        assert(item.price >= 0, reason="cents draws from 0 to 10_000")
    )
```

`Expr`'s simplest choice is `Num`, although `Add` comes first.

```text
use std.testing.{Arbitrary, Choices}

@derive(Arbitrary)  # error: member-not-derivable
struct Task:
    run: fn() -> i32

struct Job:
    run: fn() -> i32

impl Arbitrary for Job:
    fn arbitrary(c: &mut Choices) -> Job:
        n := c.int(0..=9)
        Job { run: fn() -> i32: n }
```

`Task`'s member `run` has a function type, which does not implement
`Arbitrary`, so `Task` is not derivable. `Job` writes
its own `impl Arbitrary` instead.

```text
use std.testing.Arbitrary

@derive(Arbitrary, Debug)
struct Box[T]:
    value: T

fn needs[T < Arbitrary](value: T) -> T: value

fn check(item: Box[i32]) -> Box[i32]:
    needs(item)

fn invalid(item: Box[fn() -> i32]) -> Box[fn() -> i32]:
    needs(item)  # error: unsatisfied-trait-bound
```

The derived implementation is for `Box[T]` with `T < Arbitrary`.
`Box[i32]` meets it; `Box[fn() -> i32]` does not, since a function type
does not implement `Arbitrary`.

```text
use std.testing.Arbitrary

@derive(Arbitrary, Debug)
enum Tree:
    Node(children: List[Tree])

@derive(Arbitrary, Debug)
enum Loop:
    More(next: *Loop)

@derive(Arbitrary, Debug)
struct Ring:
    next: *Ring
```

`Tree`'s one variant is not recursive: its `self_ref` is `.Optional`,
because an empty list holds no `Tree`. Every variant of `Loop` is
recursive, so its derived `arbitrary` panics on the first case with the
message `Loop has no finite value`. So does `Ring`'s, because its member
`next` is `.Required`.

> **Why.** One fact that holds a whole generator covers every range,
> length, and shape, so derived `Arbitrary` needs no range or length facts.
> `With[F]` is a typed fact type. So a generator of the wrong type is
> caught where it is written, and the derived code reads it at the member's
> type.

> **Why.** A template states one bound for all of a type's members, and no
> fact can lift it from one member. So every member meets the bound, and
> a type whose members cannot is written by hand.

> **Note.** A newtype gets no `Structure`
> ([`trait.derive.newtype.templated`](../lang/09-traits.md#r-trait.derive.newtype.templated)).
> A newtype that derives `Arbitrary` through a base with no finite value
> panics with the base type's name.

> **Note.** These are runner behavior, not rules of this chapter. One is
> how often a draw outside an [edge case](#edge-values) returns a small
> value. So are any small-first order of cases and the size of the draw
> budget. So are which chars `string` draws and
> how the runner shrinks a failing case.

## Test Timeout

`it`, `it_each`, `it_prop`, and `it_prop_with` each take a `timeout`
option of type [`Duration?`](time.md#duration)
([Test Cases](../lang/10-modules.md#test-cases)).

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

A call of `it_each` in test position is registered as
[Registration Calls](#registration-calls) specifies.

1. r[std-testing.it-each] A top-level call of `std.testing.it_each` registers one test case for each element of `rows`, which runs `body` with that element.
2. r[std-testing.it-each.name] The test case for the element at index `i` is named `name[i]`.
3. r[std-testing.it-each.rows-at-run] `rows` is evaluated when the test runs, in its program instance, not when test cases are listed.

```text
fn double(value: i32) -> i32: value * 2

tests:
    use std.testing.{assert_equal, it_each}

    it_each("doubles", [1, 2, 3], body=fn!(value: i32):
        assert_equal(double(value), value + value)
    )
```

The call runs the test cases `doubles[0]`, `doubles[1]`, and `doubles[2]`.

## Snapshot Files

`std.testing` declares a second snapshot function, beside `snapshot`
([Snapshots](../lang/10-modules.md#snapshots)):

```text
pub fn snapshot_file(text: string) -> void $ TestRunner:
    problem := $.use(TestRunner).snapshot_check(text)
    assert(problem == "", problem)
```

1. r[std-testing.snapshot] `snapshot` compares `text` with `expect`, the expected text written in the source.
2. r[std-testing.snapshot-file] `snapshot_file` compares `text` with a snapshot file, which the test runner names from the running test case.
3. r[std-testing.snapshot.import] Neither is a prelude name; code imports them from `std.testing`.
4. r[std-testing.snapshot-file.path] `snapshot_file` keeps its file at `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`, whose parts the table below defines.
5. r[std-testing.snapshot-file.missing] When that file does not exist, the test case fails, except in an update run, as `hd test --update` makes, which records the file.
6. r[std-testing.snapshot.mismatch] When `text` differs from the expected text, `snapshot` or `snapshot_file` fails as a failed assertion does. Panic: `assertion-failed`.
7. r[std-testing.snapshot.update] In an update run, as `hd test --update` makes, of an `it` test case, a `snapshot` call whose `text` differs from the expected text records `text` and the place of the call with the test runner, and returns instead of failing. The runner then rewrites the call's `expect` by [`cli.test.snapshot.update`](../cli/command-line.md#r-cli.test.snapshot.update).
8. r[std-testing.snapshot-file.missing-panic] A missing snapshot file outside an update run fails the same way. Panic: `assertion-failed`.
9. r[std-testing.snapshot-file.runner] `snapshot_file` has the requirement row `$ TestRunner`. It calls [`snapshot_check`](#r-std-testing.runner.snapshot-check) on the `TestRunner` provider that covers the call, and fails as above when the result is not empty, showing it.

| Rule | Part | Value |
| --- | --- | --- |
| r[std-testing.snapshot-file.folder] Folder | `__snapshots__/` | one folder at the package root, beside `hd.toml`; it has no `mod.hd`, so it is never a module |
| r[std-testing.snapshot-file.module] Module | `<module>` | the test's module path, such as `billing`; a module under `tests/` is `tests.<name>` |
| r[std-testing.snapshot-file.module.root] Root module | `pkg` | the `<module>` of `src/lib.hd`, the package root module that `pkg` names by [`module.path.lib-file`](../lang/10-modules.md#r-module.path.lib-file) |
| r[std-testing.snapshot-file.slug] Slug | `<test-slug>` | the test case name, lowercased, with each run of characters other than ASCII letters and digits turned into `-` |
| r[std-testing.snapshot-file.counter] Counter | `<n>` | the count of `snapshot_file` calls within one test case run, from 1 |
| r[std-testing.snapshot-file.row] Table row | `<test-slug>.<i>` | the slug of an `it_each` row adds the row's index |

> **Why.** The test picks the rendering, such as `debug(value)`. A missing
> snapshot file fails outside an update run, so a test that was never
> recorded cannot pass by accident.

See also: [Debug Trait](../lang/09-traits.md#debug-trait).

## Running Executables

`hd_run!` runs a `Command` naming one of the package's executables or
tasks from an integration test, and returns its `ProcessOutput`.
It is plain hd over the host trait
[`Process`](../lang/10-modules.md#processes):

```text
use std.process.{Command, Process, ProcessOutput}

pub fn hd_run!(command: Command) -> ProcessOutput $ Process:
    match $.use(Process).run!(command):
        .Ok(output) => output
        .Err(.NotFound(_)) => panic("hd_run!: the package has no executable or task named '${command.program}'")
        .Err(error) => panic("hd_run!: '${command.program}' did not start: ${error}")
```

1. r[std-testing.hd-run.command-import] Code imports `hd_run` from `std.testing`, and `Command` and `ProcessOutput` from `std.process`.
2. r[std-testing.hd-run.command-runs] `hd_run!(command: Command) -> ProcessOutput $ Process` runs the executable named by `command.program`, by [`cli.exe.table`](../cli/command-line.md#r-cli.exe.table).
3. r[std-testing.hd-run.default-name] The default executable is named after the package, by [`cli.exe.default-name`](../cli/command-line.md#r-cli.exe.default-name).
4. r[std-testing.hd-run.command-task] `command.program` may also name a [task](../cli/command-line.md#tasks) of the package, which runs as `hd run NAME` runs it. No name names both, by [`cli.task.name-clash`](../cli/command-line.md#r-cli.task.name-clash).
5. r[std-testing.hd-run.waits] The call completes when the program exits.
6. r[std-testing.hd-run.process-output] Its result is the `ProcessOutput` returned by the covering `Process` provider, with stdout, stderr, and any exit status unchanged.
7. r[std-testing.hd-run.row] `hd_run!` has the requirement row `$ Process`, the host capability to start a process. It calls `run!` on the `Process` provider that covers the call.
8. r[std-testing.hd-run.binding] In an integration test, the test runner binds `Process` to the package's executables and tasks, by [`cli.test.process`](../cli/command-line.md#r-cli.test.process). A test body's row takes `Process` from the call, so a test case writes no row for it.
9. r[std-testing.hd-run.unknown-command] When `command.program` names neither an executable nor a task of the package, `hd_run!` panics at run time, whether or not `command.program` is a literal. Its category is that of a `panic` call. Panic: `explicit-panic`.
9. r[std-testing.hd-run.start-failure] When the program exists but does not start, so `run!` returns `PermissionDenied`, `NotGranted`, or `Other`, `hd_run!` panics too, with the error's text in its message. Panic: `explicit-panic`.
10. r[std-testing.hd-run.integration-only] A call of `hd_run!` outside an [integration test module](../lang/10-modules.md#r-module.test.integration) is an error. Error: `test-only-use`.

11. r[std-testing.hd-run.command-fields] `hd_run!` passes the command unchanged, including its arguments, environment, current directory, and standard input.

The integration test that `hd new --app` writes for a package `hello`:

```text
# tests/hello.hd
use std.process.Command
use std.testing.{assert_equal, hd_run}

it("prints a greeting"):
    let out = hd_run!(*Command::new("hello"))
    assert_equal(out.stdout, "hello, world\n")
    assert_equal(out.status, 0)
```

```text
# src/greeting.hd
tests:
    use std.process.Command
    use std.testing.hd_run

    it("runs the binary"):
        _ := hd_run!(*Command::new("hello"))  # error: test-only-use
```

> **Note.** `hd test` builds the package's executables before it runs any
> test case ([`cli.test.builds-executables`](../cli/command-line.md#r-cli.test.builds-executables)).

> **Why.** An integration test checks a program as its user sees it, as
> Cargo's integration tests do through `CARGO_BIN_EXE_<name>`. A unit test
> runs on fakes alone, so it never starts a process. A real row, not a
> hidden grant, lets `lib/std` write `hd_run!` in plain hd.

## Temporary Directories

`std.testing` declares a function that returns the running test case's own
temporary directory, as Go's `t.TempDir` does:

```text
pub fn temp_dir() -> Path $ TestRunner:
    Path($.use(TestRunner).temp_dir())
```

1. r[std-testing.temp-dir.decl] `std.testing` declares `temp_dir` with the signature above. It is not a prelude name; code imports it, as in `use std.testing.temp_dir`.
2. r[std-testing.temp-dir.fresh] `temp_dir()` returns the absolute path of a fresh, empty directory that belongs to the running test case alone, by [`cli.test.env.temp-dir`](../cli/command-line.md#r-cli.test.env.temp-dir).
3. r[std-testing.temp-dir.same] Every call within one run of a test body returns the same path.
4. r[std-testing.temp-dir.per-run] Each row of an `it_each` call and each generated case of a property gets a directory of its own.
5. r[std-testing.temp-dir.removed] The runner removes the directory once the test body's run ends, by [`cli.test.env.temp-dir.removed`](../cli/command-line.md#r-cli.test.env.temp-dir.removed).

An integration test that reads a fixture of the package and writes its
output into its own directory:

```text
# tests/export.hd
use std.fs.{read_text, write_text}
use std.path.Path
use std.testing.{assert_equal, temp_dir}

it("exports the orders into its own directory"):
    orders := read_text!(Path("fixtures/orders.csv")).expect("the orders fixture")
    out := Path("${temp_dir()}/export.csv")
    write_text!(out, orders).expect("the export")
    assert_equal(read_text!(out).expect("the written export"), orders)
```

> **Note.** A unit test may call `temp_dir` too, but it reaches no
> real file. Only an integration test case gets `FsRead` and `FsWrite`
> from the runner.

> **Why.** A shared output path makes tests that run at once overwrite
> each other's files. A directory per run of a body, which the runner
> removes, leaves nothing behind in the package.

## Unit Test Providers

A unit test gets `TestRunner` alone
([`module.testing.unit-row.anywhere`](../lang/10-modules.md#r-module.testing.unit-row.anywhere)).
Each host capability trait has a deterministic std provider that the
test binds with `$.with` instead:

| Host trait | Provider | Defined in |
| --- | --- | --- |
| `Console` | `MemoryConsole` | [Buffer Console](console.md#buffer-console) |
| `ConsoleInput` | `ScriptedInput` | [Scripted Input](console.md#scripted-input) |
| `Args`, `Env` | `ScriptedArgs`, `ScriptedEnv` | [Map Providers](host.md#map-providers) |
| `Clock` | `ManualClock` | [Manual Clock](time.md#manual-clock) |
| `Random` | `SeededRandom` | [Seeded Random](random.md#seeded-random) |
| `FsRead`, `FsWrite` | `MemoryFs` | [Memory File System](fs.md#memory-file-system) |
| `Process` | `ScriptedProcess` | [Scripted Process](process.md#scripted-process) |
| `Http` | `ScriptedHttp` | [Scripted Http](http.md#scripted-http) |
| `Net` | `ScriptedNet` | [Scripted Net](net.md#scripted-net) |
| `Sys` | `ScriptedSys` | [Map Sys](host.md#map-sys) |

```text
use std.time.{Clock, ManualClock, Timestamp, now}

fn receipt_header(store: string) -> string $ Clock:
    "${store} at ${now().to_rfc3339()}"

tests:
    use std.testing.assert_equal

    it("stamps the receipt with the clock's time"):
        $.with(Clock=ManualClock::new(Timestamp::from_unix_milliseconds(0))):
            assert_equal(receipt_header("Main St"), "Main St at 1970-01-01T00:00:00Z")

    it("reads the real clock"):
        _ := receipt_header("Main St")  # error: missing-requirement
```

1. r[std-testing.unit.hint] The `missing-requirement` error for a host trait of this table, inside a unit test, names the trait's provider and `$.with`. It also suggests moving the test case to the test root.

> **Why.** An agent that meets the error learns both fixes at once. A fake
> keeps the test a unit test, and the test root makes it an integration
> test.

## Runner Capabilities

`std.testing` reaches the test runner through two host capability traits,
and describes each property case with a struct value:

```text
pub trait TestRunner:
    fn row(&mut self, count: usize) -> usize
    fn report_timeout(&mut self, millis: i64) -> void
    fn snapshot_check(&mut self, text: string) -> string
    fn temp_dir(&mut self) -> string

pub trait PropertyRunner:
    fn start(&mut self, cases: usize, shrink: usize, examples: usize) -> PropertyCase
    fn record(&mut self, value: i64) -> void
    fn show(&mut self, text: string) -> void

pub struct PropertyCase:
    pub example: usize?
    pub seed: i64
    pub size: i32
    pub replay: List[i64]
```

1. r[std-testing.runner.declares-ref.reference-values] `std.testing` declares the host capability traits `TestRunner` and `PropertyRunner` with the methods above. Each method takes `&mut self`, so `$.use(TestRunner)` is a `&mut dyn TestRunner` by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values). Code imports them, as in `use std.testing.TestRunner`.
2. r[std-testing.runner.case-type-struct] `std.testing` declares the public struct value `PropertyCase` with the fields above. Code imports it, as in `use std.testing.PropertyCase`.
3. r[std-testing.runner.plain] Every method is a plain call, not a bang call.
4. r[std-testing.runner.binding] When it runs a test case, the test runner binds `TestRunner` for the `std.testing` code that runs the case around its body. For a property test case, it also binds `PropertyRunner`.
5. r[std-testing.runner.body-property] A test body's row may hold `TestRunner`, by [`module.testing.unit-row.test-runner`](../lang/10-modules.md#r-module.testing.unit-row.test-runner), but not `PropertyRunner`. So a body that uses `PropertyRunner` without a provider scope is an error. Error: `missing-requirement`.

| Rule | Method | What it does |
| --- | --- | --- |
| r[std-testing.runner.row] Row | `row(count)` | reports that an `it_each` call has `count` rows, and returns the index of the row that the test case runs |
| r[std-testing.runner.timeout] Timeout | `report_timeout(millis)` | reports the test case's `timeout` in milliseconds, before its body runs |
| r[std-testing.runner.snapshot-check] Snapshot | `snapshot_check(text)` | compares `text` with the running test case's next [snapshot file](#snapshot-files), and records the file in an update run; returns `""` when the text matches or was recorded, else a message that says how the text differs or that the file is missing |
| r[std-testing.runner.temp-dir] Temporary directory | `temp_dir()` | returns the absolute path of the running test case's [temporary directory](#temporary-directories), and makes the directory on the case's first call |
| r[std-testing.runner.start-case] Start | `start(cases, shrink, examples)` | reports a property's `cases`, `shrink`, and count of `examples`, and returns the `PropertyCase` that the case runs |
| r[std-testing.runner.record] Record | `record(value)` | records the case's next draw, a value from 0 to that draw's bound |
| r[std-testing.runner.show] Show | `show(text)` | reports the `Debug` text of the case's input, which a failure report prints |

| Rule | Field of `PropertyCase` | Holds |
| --- | --- | --- |
| r[std-testing.runner.case.example] Example | `example` | the index of the example that the case runs, or `.None` for a generated case |
| r[std-testing.runner.case.seed] Seed | `seed` | the seed of the case's fresh draws |
| r[std-testing.runner.case.size] Size | `size` | how far the case's fresh draws reach, which grows from case to case |
| r[std-testing.runner.case.replay] Replay | `replay` | the recorded draws that the case replays or shrinks, in order; empty for a fresh case |

6. r[std-testing.runner.examples-done] Once every example has run, `start` ends the case instead of returning, and the runner counts the case as discarded.
7. r[std-testing.runner.show-before-body] `std.testing` calls `show` once for each case, after it has drawn or taken the input and before the property body runs.
8. r[std-testing.runner.choices] Each `Choices` holds its case's `PropertyRunner` provider, a `&mut dyn PropertyRunner`, so it records its draws through it with no requirement row, by [`req.use.value.flow`](../lang/11-requirements-and-suspension.md#r-req.use.value.flow).
9. r[std-testing.runner.replay] The draw at position `i` of a case returns `replay[i]`, limited to the draw's bound, when `replay` holds a value at `i`. Every other draw is fresh.
9. r[std-testing.runner.record-every] `Choices` calls `record` once for every draw, replayed or fresh, before it uses the value. So the runner holds the case's draws even when the case then panics.
10. r[std-testing.runner.random] The fresh draws of a case come from a [`Random`](random.md#random-source) provider that `std.testing` seeds with the case's `seed`. So the same seed draws the same cases.
11. r[std-testing.runner.case.size-count] The `size` of a run's first generated case is 0. Each later generated case has one more than the case before it, a discarded case not counting.
12. r[std-testing.runner.case.seed-step] The `seed` of each case of a run, a discarded one included, is one more than the `seed` of the case before it.

```text
use std.testing.{PropertyRunner, TestRunner}

struct FirstRow:
    rows: usize

impl TestRunner for FirstRow:
    fn row(&mut self, count: usize) -> usize:
        self.rows = count
        0

    fn report_timeout(&mut self, millis: i64) -> void:
        pass

    fn snapshot_check(&mut self, text: string) -> string:
        ""

    fn temp_dir(&mut self) -> string:
        "/tmp/first-row"

fn pick(rows: List[string]) -> string $ TestRunner:
    rows[$.use(TestRunner).row(rows.len())]

fn label(text: string) -> void $ PropertyRunner:
    $.use(PropertyRunner).show(text)

tests:
    it("picks a row"):
        _ := pick(["a"])

    it("labels the input"):
        label("a")  # error: missing-requirement
```

> **Why.** The runner is a host, as a console is, so `std.testing` reaches
> it through a capability in a row, not a primitive. Recording and
> shrinking stay on the runner's side, and replay is ordinary hd. The
> generator is ordinary hd, so a seed means the same draws on every host.

> **Note.** Which generator `std.testing` seeds is runner behavior, as is
> how `size` grows. `lib/std` uses xoshiro128\*\*, seeded through
> SplitMix32.

See also: [Host Capabilities](../cli/command-line.md#host-capabilities),
[Table-Test Rows](#table-test-rows), [Property Tests](#property-tests).

### Discarding A Case

1. r[std-testing.runner.discard-panic] `Choices.assume(false)` ends the case with a panic whose category is that of a `panic` call and whose message is exactly `std.testing: case discarded`. Panic: `explicit-panic`.
2. r[std-testing.runner.discard-read] The runner reads such a panic, raised before the case calls `show`, as a discard. The case neither passes nor fails, and it does not count toward `cases`.
3. r[std-testing.runner.discard-after-show] The same panic after `show` is an ordinary failure, so a property body cannot discard a case, as [`std-testing.prop.body-no-discard`](#r-std-testing.prop.body-no-discard) states.

```text
use std.testing.{assert, Choices, it_prop_with}

fn even(c: &mut Choices) -> i64:
    let value: i64 = c.int(0..=100)
    c.assume(value % 2 == 0)   # a false assumption discards the case
    value

tests:
    it_prop_with("only even values are checked", gen=even, cases=20, prop=fn!(n: i64):
        assert(n % 2 == 0, reason="assume discards odd values")
    )
```

> **Why.** A discard is a panic, so it ends the case through the panic
> rules every case already has. The runner needs no method that does
> not return. Each draw was recorded before the panic, so the runner
> still holds the stream.

## Stateful Command Histories

`commands` draws a bounded history of commands by advancing a pure model during generation.

| Function | Signature |
| --- | --- |
| `std.testing.commands` | `fn commands[M, C](c: &mut Choices, max: usize, initial: M, next: fn(&mut Choices, &M) -> C?, advance: fn(&M, &C) -> M) -> List[C]` |

```text
use std.testing.{Choices, commands, it_prop_with}

fn next_count(_c: &mut Choices, model: &usize) -> usize?:
    if *model == 4: .None else: .Some(*model + 1)

fn advance_count(_model: &usize, command: &usize) -> usize:
    *command

fn counts(c: &mut Choices) -> List[usize]:
    commands(c, max=8, initial=0usize, next=next_count, advance=advance_count)

tests:
    it_prop_with("counter follows its model", gen=counts, prop=fn!(history: List[usize]):
        let system: usize = 0
        for expected in history:
            system += 1
            assert_equal(system, expected)
    )
```

1. r[std-testing.commands.declare] `std.testing` declares `commands` with the signature above.
2. r[std-testing.commands.length] `commands` draws its target length with `c.int(0..=max)`.
3. r[std-testing.commands.initial] Each invocation starts its model at `initial` and its history empty.
4. r[std-testing.commands.next] Until the target length is reached, `next(c, &model)` chooses a command valid for the current model or returns `.None`.
5. r[std-testing.commands.stop] A `.None` from `next` ends generation and returns the history drawn so far.
6. r[std-testing.commands.append] Each `.Some(command)` appends `command` to the history.
7. r[std-testing.commands.advance] After appending a command, `advance(&model, &command)` supplies the model for the next call to `next`.
8. r[std-testing.commands.deterministic] `next` and `advance` must depend only on the supplied choices, model, and command.
9. r[std-testing.commands.replay] Replay regenerates the history from `initial` through `next` and `advance`, including when the property runner shrinks the choices.
10. r[std-testing.commands.execute] A stateful property's body must execute the generated history without filtering commands whose preconditions no longer hold.
11. r[std-testing.commands.fresh-system] A stateful property's body must construct fresh model and system state for each invocation.

The same recipe can be written directly over `Choices` and `it_prop_with`:

```text
use std.testing.{Choices, it_prop_with}

fn increments(c: &mut Choices) -> List[usize]:
    let mut history: List[usize] = []
    let model: usize = 0
    count := c.int(0..=8usize)
    for _ in 0..count:
        model += 1
        history.push(model)
    history

tests:
    it_prop_with("counter follows the direct recipe", gen=increments, prop=fn!(history: List[usize]):
        let system: usize = 0
        for expected in history:
            system += 1
            assert_equal(system, expected)
    )
```

> **Why.** Regenerating from a pure model keeps a shrunk history valid.
> Local fakes can be constructed inside the property body, so existing
> shrinking and saved choice streams need no execution journal.

See also: [Property Tests](#property-tests), [Unit Test Providers](#unit-test-providers).
