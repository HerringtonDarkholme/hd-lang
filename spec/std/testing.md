# Testing

Status: standard library specification draft.

This chapter defines the part of `std.testing` that `lib/std` and the test
runner implement over the language tier:

- how a property test draws, discards, reports, and replays its inputs;
- the draw budget;
- how `@derive(Arbitrary)` builds a type's default generator;
- the registration functions `it_each`, `it_prop`, and `it_prop_with`;
- what the `timeout` option does;
- how an `it_each` call expands and names its rows;
- how snapshots compare text, and where snapshot files live;
- how an integration test runs one of the package's executables.

The language tier keeps the assertion functions, `it` and its options, the
test-position rules, and the literal `expect` of `snapshot`
([Standard Testing](../lang/10-modules.md#standard-testing),
[Test Cases](../lang/10-modules.md#test-cases),
[Snapshots](../lang/10-modules.md#snapshots)).

## Registration Functions

`std.testing` declares `it_each`, which registers one test case per row:

```text
pub fn it_each[A, T < Termination, R](name: string, rows: List[A], ignore: string? = .None,
                                      expect_panic: string? = .None, timeout: Duration? = .None,
                                      body: fn!(A) -> T $ R) -> void $ R
```

It also declares `it_prop` and `it_prop_with`, which register one property
test case each:

```text
pub fn it_prop[T < Arbitrary & Debug, R < Termination](name: string, ignore: string? = .None,
                                                       expect_panic: string? = .None, timeout: Duration? = .None,
                                                       cases: i32 = 100, shrink: i32 = 500, examples: List[T] = [],
                                                       prop: fn!(T) -> R) -> void
pub fn it_prop_with[T < Debug, R < Termination](name: string, gen: fn(mut Choices) -> T, ignore: string? = .None,
                                                expect_panic: string? = .None, timeout: Duration? = .None,
                                                cases: i32 = 100, shrink: i32 = 500, examples: List[T] = [],
                                                prop: fn!(T) -> R) -> void
```

1. r[std-testing.registration] `it_each`, `it_prop`, and `it_prop_with` are [test registration functions](../lang/10-modules.md#r-module.testing.position-statements), so the test-position rules of `it` apply to them.
2. r[std-testing.it-each.import] `it_each` is not a prelude name; code imports it with `use std.testing.it_each`.
3. r[std-testing.it-each.body-closure] Its body has a parameter, so it is an explicit `fn!` closure, not a trailing block. After omitted options, a call passes it by name, as in `body=fn!(value: i32): ...`.
4. r[std-testing.it-each.name-clash] Another test case of the module must not be named `name[i]` for any index `i`. Error: `duplicate-test-name`.
5. r[std-testing.it-prop.registers] A top-level call of `std.testing.it_prop` or `std.testing.it_prop_with` registers one property test case.
6. r[std-testing.it-prop.import] Neither is a prelude name; code imports them from `std.testing`.
7. r[std-testing.variants.name] The name of an `it_each`, `it_prop`, or `it_prop_with` call must be a string literal without interpolation. Any other name is an error. Error: `non-literal-test-argument`.
8. r[std-testing.variants.options] Each takes the options of `it`, `ignore`, `expect_panic`, and `timeout`, under the same rules.
9. r[std-testing.variants.body] The body's result follows [Propagation In Test Blocks](../lang/05-expressions.md#propagation-in-test-blocks): `void`, or `Result[void, Error]` when it uses `?`.
10. r[std-testing.try.test.row-body] The body closure of an `it_each`, `it_prop`, or `it_prop_with` call that writes no result type gets its result type by [`expr.try.test.with-try`](../lang/05-expressions.md#r-expr.try.test.with-try) and [`expr.try.test.without-try`](../lang/05-expressions.md#r-expr.try.test.without-try), as a trailing block given to `it` does.

```text
use std.testing.it_each

fn label() -> string: "halves"

tests:
    it_each("doubles", [1, 2], body=fn!(value: i32): pass)

    it("doubles[0]"):  # error: duplicate-test-name
        pass

    it_each(label(), [1, 2], body=fn!(value: i32): pass)  # error: non-literal-test-argument
```

> **Note.** The `timeout` parameter of each has the type that `it`'s has,
> `std.time.Duration?` ([Test Timeout](#test-timeout)).

See also: [Table-Test Rows](#table-test-rows), for how an `it_each` call
expands and names its rows, and [Property Tests](#property-tests),
[Draw Budget](#draw-budget), and [Derived Arbitrary](#derived-arbitrary),
for how the runner generates a property's inputs.

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
9. r[std-testing.it-prop] The runner generates the inputs of each property test case that `it_prop` or `it_prop_with` registers, and shrinks a failing one.
10. r[std-testing.prop.debug] `it_prop` and `it_prop_with` require `T < Debug`. A property whose input type does not implement `Debug` is an error. Error: `unsatisfied-trait-bound`.
11. r[std-testing.prop.report] When a property test fails, the runner prints the shrunk input with `Debug`.
12. r[std-testing.prop.examples] Each input in `examples` runs first on every run, before the saved regression streams and the generated cases.
13. r[std-testing.prop.discard] A case that `assume` discards does not count toward `cases`. The runner generates another case in its place.
14. r[std-testing.prop.body-no-discard] Only a generator discards a case, through `Choices.assume`. A property body has no `Choices`, so it cannot discard one.
15. r[std-testing.prop.discard-limit] A property test fails when more than 10 times `cases` of its cases are discarded, as Hypothesis's `filter_too_much` health check does.
16. r[std-testing.prop.regression-file] The runner saves a failing property's shrunk choice stream in `<package root>/__regressions__/<module>/<test-slug>`. `<module>` and `<test-slug>` are as for a [snapshot file](#snapshot-files).
17. r[std-testing.prop.regression-format] The file holds the stream's draws in order, one decimal number per line.
18. r[std-testing.prop.regression-replay] On the next run, the runner replays a property's saved stream before it generates new cases.

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

## Derived Arbitrary

`@derive(Arbitrary)` gives a data type or enum its default generator
through the template of `Arbitrary`.

1. r[std-testing.arbitrary.derive] `@derive(Arbitrary)` derives `Arbitrary` through its [template](../lang/14-annotations.md#templates). The derived `arbitrary` draws each member with its type's `Arbitrary`. For an enum, it draws a variant, then that variant's payload.
2. r[std-testing.arbitrary.derive.template] That template is ordinary `std.testing` code over `std.structure`: it reads each variant's and member's [`self_ref`](../lang/14-annotations.md#self-references), and the compiler supplies nothing for `Arbitrary` itself.
3. r[std-testing.arbitrary.derive.member-bound] The template requires the type of every member to implement `Arbitrary`, whether or not `arbitrary.with` tunes the member. It does not require a member to be [inspectable](../lang/09-traits.md#inspectable-types).
4. r[std-testing.arbitrary.derive.params-arbitrary] For a generic type, the derived implementation gets `T < Arbitrary` for each type parameter `T` that a member's type uses, by [`annot.bound.params`](../lang/14-annotations.md#r-annot.bound.params). So `@derive(Arbitrary)` on `data Box[T]` with a member `value: T` needs no hand-written block.
5. r[std-testing.arbitrary.derive.not-derivable] A type with a member whose type fails that bound, such as a function-typed member, is not derivable. `@derive(Arbitrary)` on it is an error, reported at the opt-in and naming the member. Error: `unsatisfied-trait-bound`.
6. r[std-testing.arbitrary.derive.manual] Such a type gets its default generator only from a hand-written `impl Arbitrary`.
7. r[std-testing.arbitrary.derive.simplest] A derived enum's simplest choice is its first non-recursive variant, whatever the declaration order.
8. r[std-testing.arbitrary.derive.recursive] A variant is recursive when its `self_ref` is `.Required`, as [Self References](../lang/14-annotations.md#self-references) computes it from the member types.
9. r[std-testing.arbitrary.derive.recursive.containers] A `List`, `Map`, or optional member does not make its variant recursive, because its `self_ref` is at most `.Optional`: its simplest value is empty or `.None`.
10. r[std-testing.arbitrary.derive.no-finite] When every variant of a derived enum is recursive, the derived `arbitrary` panics on the property's first case, with a message that names the type. Panic: `explicit-panic`.
11. r[std-testing.arbitrary.derive.no-finite.message] The message is `"${T::name()} has no finite value"`, where [`T::name()`](../lang/14-annotations.md#r-annot.structure.name) is the type's declared name.
12. r[std-testing.arbitrary.derive.no-finite.data] When a member of a derived data type has `self_ref` `.Required`, its derived `arbitrary` panics the same way, and the compiler does not reject the type either. Panic: `explicit-panic`.
13. r[std-testing.arbitrary.derive.no-finite.unchecked] The compiler does not reject such an enum, because no derivation check reports it.
14. r[std-testing.arbitrary.with] A member whose facts hold an `arbitrary.with(gen)` value is drawn by `gen` instead of its type's `Arbitrary`.
15. r[std-testing.arbitrary.with.module-typed] The module `std.testing.arbitrary` declares `with` and its result type `With[F]`, a [typed fact type](../lang/14-annotations.md#member-typed-facts), as shown below. Code imports the module, as in `use std.testing.arbitrary`, and writes `@arbitrary.with(gen)`.
16. r[std-testing.arbitrary.with.checked] `gen` must draw the member's declared type. A generator of another type is an error, reported on its decorator, by [`annot.typed-fact.check`](../lang/14-annotations.md#r-annot.typed-fact.check). Error: `type-mismatch`.
17. r[std-testing.arbitrary.with.typed-read] The derived `arbitrary` reads the member's `With[F]` through its handle, with [`h.fact`](../lang/14-annotations.md#r-annot.handle.fact), and draws the member by its `gen`.
18. r[std-testing.arbitrary.with.only] `arbitrary.with` is the only fact that derived `Arbitrary` reads.

```text
@annotate::[F](.Field)
pub data With[F]:
    pub gen: fn(mut Choices) -> F

pub fn with[F](gen: fn(mut Choices) -> F) -> With[F]
```

> **Note.** The member's declared type is the expected type of `F`, by
> [`annot.typed-fact.check.inferred`](../lang/14-annotations.md#r-annot.typed-fact.check.inferred).
> So a generic generator such as `fn any_text[T](c: mut Choices) -> T`
> needs no type argument: `@arbitrary.with(any_text)` on `name: string`
> solves `T = string`.

```text
use std.testing.{Arbitrary, Choices, assert, it_prop}
use std.testing.arbitrary

fn cents(c: mut Choices) -> i32:
    c.int(0, 10_000)

@derive(Arbitrary, Debug)
data Item:
    name: string
    @arbitrary.with(cents)
    price: i32

@derive(Arbitrary, Debug)
enum Expr:
    Add(left: Expr, right: Expr)
    Num(value: i32)

tests:
    it_prop("prices are never negative", examples=[Item { name: "", price: 0 }], prop=fn!(item: Item):
        assert(item.price >= 0, reason="cents draws from 0 to 10_000")
    )
```

`Expr`'s simplest choice is `Num`, although `Add` comes first.

```text
use std.testing.{Arbitrary, Choices}

@derive(Arbitrary)  # error: unsatisfied-trait-bound
data Task:
    run: fn() -> i32

data Job:
    run: fn() -> i32

impl Arbitrary for Job:
    fn arbitrary(c: mut Choices) -> Job:
        n := c.int(0, 9)
        Job { run: fn() -> i32: n }
```

`Task`'s member `run` has a function type, which does not implement
`Arbitrary`, so `Task` is not derivable. `Job` writes
its own `impl Arbitrary` instead.

```text
use std.testing.Arbitrary

@derive(Arbitrary, Debug)
data Box[T]:
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
    More(next: Loop)

@derive(Arbitrary, Debug)
data Ring:
    next: Ring
```

`Tree`'s one variant is not recursive: its `self_ref` is `.Optional`,
because an empty list holds no `Tree`. Every variant of `Loop` is
recursive, so its derived `arbitrary` panics on the first case with the
message `Loop has no finite value`. So does `Ring`'s, because its member
`next` is `.Required`.

> **Why.** One fact that holds a whole generator covers every range,
> length, and shape, so derived `Arbitrary` needs no range or length facts.
> `With[F]` is a typed fact type, so a generator of the wrong type is caught
> where it is written, and the derived code reads it at the member's type.

> **Why.** A template states one bound for all of a type's members, and no
> fact can lift it from one member. So every member meets the bound, and
> a type whose members cannot is written by hand.

> **Note.** A newtype gets no `Structure`
> ([`trait.derive.newtype.templated`](../lang/09-traits.md#r-trait.derive.newtype.templated)).
> A newtype that derives `Arbitrary` through a base with no finite value
> panics with the base type's name.

> **Note.** These are runner behavior, not rules of this chapter: how often
> a draw returns small and boundary values, any small-first order of cases,
> and the size of the draw budget. So are which chars `string` draws and
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
[Registration Functions](#registration-functions) specifies.

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
([Snapshots](../lang/10-modules.md#snapshots)):

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

See also: [Debug Trait](../lang/09-traits.md#debug-trait).

## Running Executables

`std.testing` declares a function that runs one of the package's
executables from an integration test, and the data type of its result:

```text
pub data RunOutput:
    pub stdout: string
    pub stderr: string
    pub status: i32

pub fn hd_run!(name: string, args: List[string] = [], stdin: string = "") -> RunOutput
```

1. r[std-testing.hd-run.import] Neither `hd_run` nor `RunOutput` is a prelude name; code imports them from `std.testing`.
2. r[std-testing.hd-run.runs] `hd_run!(name, args, stdin)` runs the executable of the package under test whose name is `name`, as [`cli.exe.table`](../cli/command-line.md#r-cli.exe.table) names it. It passes `args` as the program's arguments and `stdin` as its standard input.
3. r[std-testing.hd-run.default-name] The default executable is named after the package, by [`cli.exe.default-name`](../cli/command-line.md#r-cli.exe.default-name).
4. r[std-testing.hd-run.waits] The call completes when the executable exits.
5. r[std-testing.hd-run.output] Its result holds the text the executable wrote to standard output and to standard error, and the exit status it exited with.
6. r[std-testing.hd-run.no-row] `hd_run!` has an empty requirement row. The test runner grants the capability it uses, so a test that calls it writes no row for it.
7. r[std-testing.hd-run.integration-only] A call of `hd_run!` outside an [integration test module](../lang/10-modules.md#r-module.test.integration) is an error. Error: `test-only-use`.

The integration test that `hd new --app` writes for a package `hello`:

```text
# tests/hello.hd
use std.testing.{assert_equal, hd_run}

it("prints a greeting"):
    let out = hd_run!("hello")
    assert_equal(out.stdout, "hello, world\n", reason="the greeting")
    assert_equal(out.status, 0, reason="a clean exit")
```

```text
# src/greeting.hd
use std.testing.hd_run

tests:
    it("runs the binary"):
        _ := hd_run!("hello")  # error: test-only-use
```

> **Note.** `hd test` builds the package's executables before it runs any
> test case ([`cli.test.builds-executables`](../cli/command-line.md#r-cli.test.builds-executables)).

> **Why.** An integration test checks a program as its user sees it, as
> Cargo's integration tests do through `CARGO_BIN_EXE_<name>`. A unit test
> runs on fakes alone, so it never starts a process.
