# Testing Redesign

Status: design record, started 2026-09-27. Owner decisions T1-T51 are
decided. T2-T31 and T36's statement rule are applied to the specification
on 2026-09-27 (T4, T5, and T8 earlier that day). T33 is not applied; it waits on the questions under
[Still Open](#still-open). T29, T30, T32, T34, and the runner parts of T20
and T21 are tooling and library text, recorded in
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing) and
[Standard Library](STDLIB.md#testing-layer). The redesign may continue with
more issues.

## Owner Decisions

1. **T1: `@test` marks test-only items.** (Superseded by T11: a `tests:`
   block replaces the labels.) `@test` on any top-level item (a
   `use`, function, data type, or other declaration) compiles it only in
   test builds, like Rust's `#[cfg(test)]` or D's `version(unittest)`.
   Non-test code that names a `@test` item is an error. Test-only
   dependencies may be named only from `@test` items and tests. This
   removes the repetition of test-only imports and helpers in every test.
2. **T2: no `use` inside a test.** `use` stays top-level only. This
   supersedes the test-block `use` of
   [Packages decision 5](PACKAGES.md#owner-decisions) and its 2026-09-27
   placement detail, which were never in the specification.
3. **T3: `test("name"):` is a standard-library intrinsic, not syntax.** The
   `test` keyword and grammar item go away. `test` is the standard-library
   name `std.testing.test`, called at module top level with a string
   literal name and a trailing block; the compiler registers each such
   call as a test. Since hd has no other top-level statements, this call
   form is the one intrinsic. (Amended by T11: the call sits at the top
   level of a `tests:` block, not at module top level.)
4. **T4: a test body follows the entry-point result rule.** Like Rust's
   `Termination`, a test body returns `void` or `Result[void, E]` with
   `E < Display`. `?` works through ordinary rules; an `.Err` fails the
   test and prints like a failing `main`, cause chain included. This
   replaces [Error Conversion decision 16](ERROR_CONVERSION.md#owner-decisions)
   (wrapping other errors in a standard message error). Applied
   2026-09-27 in
   [Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks):
   the block's result is inferred like a closure's and must implement
   `Termination`.

5. **T5: `main` and tests share one `Termination` trait.** It lives in
   `std.process`; `main`'s entry-result rule and `test` both read
   `T < Termination`, so the special entry rule becomes an ordinary bound
   (Rust's model). `void` and `Result[void, E]` with `E < Display` implement
   it; `ExitStatus` and `StatusCode` still pick `main`'s exit code (revised
   by T8). Applied 2026-09-27 in
   [Executable Entry Point](../spec/10-modules.md#executable-entry-point):
   a result that does not implement it is `unsatisfied-trait-bound`.
6. **T6: a test may require host providers.** Its body's row is a row
   parameter; the test runner binds it from the test profile (for example
   `hd test --grant ...`), as a host binds `main`'s row. Unit tests use
   `$.with` fakes and have an empty row.
7. **T7: `test(...)` registers only at module top level,** with a string
   literal name, so tests can be listed without running them. Calling
   `test` anywhere else is an error; table-driven cases loop inside one
   test for now. (Amended by T11: top level of the `tests:` block.)

8. **T8: exit codes follow Rust's `ExitCode`.** `std.process` declares
   `type ExitCode(u8)` (any `u8`; 0 is success) and
   `Termination.report(self) -> ExitCode`. `void` reports `ExitCode(0)`,
   `ExitCode` reports itself, and `Result[T, E]` with `T < Termination`,
   `E < Display` reports the `.Ok` value's code, or prints the error (message
   and cause chain) and reports `ExitCode(1)`. A program that wants a
   specific code returns `ExitCode` or `Result[ExitCode, E]` from `main`.
   This removes the `ExitStatus` trait
   ([Error Conversion decision 17](ERROR_CONVERSION.md#owner-decisions)),
   `StatusCode`, the erased-`Error` exit rule, and the zero check, all of
   which were applied to the specification on 2026-09-27 and must be
   removed. A test fails when its body's `report()` is not `ExitCode(0)`.
   Applied 2026-09-27 in [Exit Status](../spec/10-modules.md#exit-status).
9. **T9: `tests/` is for integration tests.** The separate test root keeps
   its role, like Rust's `tests/`: its modules are compiled only by
   `hd test`, see only the package's public surface as a dependent would,
   and may use test dependencies anywhere. Unit tests live beside the code
   with `test(...)` and `@test` items.
10. **T10: test names are unique per module; filtering is by substring.**
    Two `test(...)` calls with one name in a module are an error. A test's
    full id is `module::name`, and `hd test <text>` runs the tests whose id
    contains the text, like `cargo test`.
11. **T11: one `tests:` block per file replaces `@test` labels.** This
    supersedes T1 and amends T3 and T7. A file may have one `tests:` block,
    compiled only by `hd test`. It sees the file's private names and holds
    `use` declarations (test dependencies allowed), helpers, data types, and
    `test(...)` calls. Code outside the block cannot name what is inside.
    `test(...)` may appear only at the top level of a `tests:` block, not at
    module top level. A `*_test.hd` file that joins its module was considered
    and dropped: it would split one module across files, and the block
    already groups test-only code; T13 keeps `*_test.hd` as its own module
    (Rust's `#[cfg(test)] mod tests`). Applied 2026-09-27 in
    [Test Blocks](../spec/02-grammar.md#test-blocks) and
    [Tests Blocks](../spec/03-names-and-scopes.md#tests-blocks).
12. **T12: property testing is a `std.testing` library.** A test calls, for
    example, `testing.check(fn(order: Order): ...)`; generators come from a
    derivable `Arbitrary` trait through typed derivation, and shrinking is a
    derived build. The API is designed with the standard library.
13. **T13: three kinds of test, each with its own view.** (1) Same-file:
    the `tests:` block sees the module's private names. (2) Standalone
    file: a module whose file name ends in `_test.hd` (for example
    `src/billing_test.hd`, module `billing_test`) is an ordinary module
    compiled only by `hd test`. It sees `pub` names package-wide like any
    sibling module, may use test dependencies at top level, and holds
    `test(...)` calls at its top level with no `tests:` block. (3)
    Integration: `tests/` sees only the library's public API, as a
    dependent does (T9). Applied 2026-09-27, with T23 and T24, in
    [Test Modules](../spec/10-modules.md#test-modules).
14. **T14 (TS-1): a trailing block for an `fn!` parameter is suspending.**
    When the parameter's type is `fn!(...)`, the trailing block is a
    suspending closure, for every callee, not only tests; the `!` in the
    callee's signature is the visible marker (Kotlin's `suspend` lambdas).
    Applied 2026-09-27 in
    [Trailing Callback Blocks](../spec/07-functions.md#trailing-callback-blocks).
15. **T15 (TS-3): a test body using `?` returns `Result[void, Error]`.**
    Its type is fixed, so every error type that implements `Error`
    converts; a body without `?` returns `void` (Zig's `anyerror!void`).
    Applied 2026-09-27 in
    [Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks).
16. **T16 (TS-2): the test-case call is `it`, not `test`.** `test` stays
    free for user code. `it` is a prelude name, so it cannot be shadowed,
    and every statement at the top level of a `tests:` block or a
    `_test.hd` module must be an `it(...)` call. This renames T3's
    `test(...)` throughout. Applied 2026-09-27, with T22, in
    [Test Cases](../spec/10-modules.md#test-cases).
17. **T17 (TS-4): `tests:` is an item block, like Rust's `mod tests`.** A
    top-level `tests:` block holds module items (uses, functions, data,
    and `it(...)` calls) visible only inside it; once per file, top level
    only, never nested. `tests` becomes a keyword. Applied 2026-09-27 in
    [Keywords And Reserved Words](../spec/01-lexical-structure.md#keywords-and-reserved-words).
18. **T18 (TS-12): the host prints a failed result.** `report()` only
    computes the `ExitCode`; the host or test runner prints the error
    (message and cause chain), as Entry Results words it.
19. **T19 (TS-5): a failed assertion panics** with category
    `assertion-failed`, ending that test; the runner reports it and runs
    the next test.
20. **T20 (TS-6): tests compile against one profile.** `hd test` uses the
    `console` profile unless `--profile` is given; a test whose
    requirements the profile cannot bind is reported as skipped. This also
    covers platform-specific tests without `cfg`.
21. **T21 (TS-11): each test runs in a fresh program instance, in parallel
    by default,** with its own `Console` buffer (shown on failure) and a
    temporary filesystem the runner deletes afterwards.
22. **T22 (TS-7): `it` takes literal named options** `ignore="reason"`,
    `expect_panic="category"`, and `timeout="5s"`, read statically by the
    runner.
23. **T23 (TS-9): shared helpers follow Rust and Go.** `_test.hd` modules
    may `use` each other, `tests/` modules may `use` each other, and
    `tests/` sees the library without its test code.
24. **T24 (TS-10): a test dependency that depends back on this package is
    usable only from `tests/`.** Using it from a `tests:` block or a
    `_test.hd` module is an error, since it would create a second copy of
    the package.
25. **T25 (TS-8): the runner drives property shrinking** by rerunning the
    property in fresh instances with smaller inputs, so a panic is just a
    failed run; generation stays a derived `build` (Hypothesis's model).
26. **T26 (TS-13, TS-14, TS-17):** table tests stay a loop inside one `it`;
    the documentation notes that hash values shift when test code changes
    (the hasher is seeded by code identity); the `_test.hd` suffix stays.
27. **T27 (TS-16): the specification pass fixes the drift** it found: this
    record's example, stale README and PACKAGES text, the conformance
    fixtures on the old `test "..."` syntax, and fixture file names that
    clash with `_test.hd`. The removal of `entry-error-not-display` (now
    `unsatisfied-trait-bound`) is kept.
28. **T28: unit tests fake, integration tests use real providers.** Tests
    in a `tests:` block or a `_test.hd` module get no host providers: every
    requirement must come from `$.with` fakes, and a missing one is a
    compile error. `tests/` binds real providers from the test profile,
    and a test whose providers the profile cannot bind is skipped (T20).
    This replaces T6 for unit tests. Applied 2026-09-27, with the language
    part of T20, in [Test Outcomes](../spec/10-modules.md#test-outcomes).
29. **T29: names stay freeform strings; `hd test --list` prints
    `module::name  file:line`.** Filters match substrings of the id.
30. **T30: snapshot tests, inline and file.** `snapshot(value,
    expect="...")` keeps the expected text in the source, and
    `hd test --update` rewrites the literal (MoonBit's `inspect`, insta's
    inline snapshots). `snapshot_file(value)` stores larger output
    in a snapshot file (named and placed by T34), approved by a review
    command. The
    value renders with `Display`, or a derivable `Inspect` when it has
    none. The file layout, the review command, and `Inspect` are designed
    with the standard library.
31. **T31: `std.testing.it_each` for table tests, not a prelude name.**
    `it_each("name", rows, body)` with `body: fn!(A) -> T` runs one case per
    row, reported as `name[i]`. It lives in `std.testing` and is imported with
    `use std.testing.it_each`; only `it` is in the prelude. The calls-only rule
    admits a call to `it` or to `std.testing.it_each`. `it.each` was
    considered: it would need members on a function or callable values, a
    new language feature. This replaces T26's loop-only answer for per-row
    results; a loop inside one `it` still works. Applied 2026-09-27 in
    [Table Tests](../spec/10-modules.md#table-tests).

```text
# std.testing
pub fn it_each[A, T < Termination, R](name: string, rows: List[A], body: fn!(A) -> T $ R) -> void $ R:
    pass
```
32. **T32: a snapshot takes a string.** `snapshot(text, expect="...")` and
    `snapshot_file` compare text; the user picks the format by rendering,
    for example `json.pretty(x)`, `yaml.encode(x)`, or `debug(x)`. There is
    no strategy system. This refines T30's rendering sentence.
33. **T33: a derivable `Debug` trait is the default rendering.** `std`
    declares `Debug` (derivable through typed derivation, implemented by
    `std` for primitives and collections) with stable, field-by-field,
    multi-line output via `debug(x)`. `assert_equal` requires
    `T < Eq + Debug`, so a failure shows both values. `Display` stays
    user-facing text. (The name avoids `Inspect`, since `Inspectable` is
    the runtime type-information trait.)
34. **T34: snapshot files are named from the test.** `snapshot_file(text)`
    takes no name; it writes `__snapshots__/<module>/<test-slug>-<n>.snap`
    under one `__snapshots__/` folder at the package root (beside
    `hd.toml`), with `tests/` modules under `tests.<name>`; `src/` stays code
    only. The owner chose one folder per package over the per-folder
    default of Jest and insta. Details filled in with
    the decision, open to owner correction: the slug lowercases the test
    name and turns each run of non-alphanumeric characters into `-`; `<n>`
    counts `snapshot_file` calls within one test run, from 1; an `it_each`
    row adds its index (`<test-slug>.<i>-<n>.snap`). `hd test --update`
    writes new or changed files, `hd test --review` shows diffs to accept
    or reject and lists snapshot files no test wrote (after a rename or
    reorder) for deletion. `__snapshots__/` has no `mod.hd`, so it is never
    a module. Known cost, accepted: reordering snapshots within a test or
    renaming a test changes file names.
35. **T35: property testing combines type defaults with choice-stream
    shrinking.** `Arbitrary` is derivable (`@derive(Arbitrary)`): the
    derived code is a `build` over a recording `std.testing.Choices`
    source, so each member is generated by its type (lists draw a length,
    numbers are biased toward 0, -1, and the extremes, optionals and enum
    variants are picked). Member lines in a derivation block tune a
    member (for example `quantity = arbitrary.range(1, 99)`); other
    constraints use a plain generator function `fn(mut Choices) -> T`.
    Shrinking replays smaller recorded choice streams through the same
    generator (Hypothesis's model), so no type needs shrink code and
    constraints always hold. This refines T12 and T25. Compared: QuickCheck
    (type-based shrink breaks constraints), proptest and fast-check
    (combinator values with value trees, a large API).
36. **T36: properties register with `std.testing.it_prop`.**
    `it_prop(name, fn(x: T): ...)` for `T < Arbitrary` and
    `it_prop_with(name, gen, prop)`, allowed wherever `it` is, imported
    (not prelude). The runner owns generation, shrinking, and replay; a
    failure prints the shrunk value via `Debug` and the seed, and
    `hd test --seed N` reproduces a run.
37. **T37: failing cases are committed per package.** The shrunk choice
    stream is saved under `__regressions__/<module>/<test-slug>` at the
    package root, next to `__snapshots__/`, and replayed first on every
    run (proptest's `proptest-regressions/`, Go's `testdata/fuzz`).
38. **T38: the default budget is 100 cases per property,** with sizes
    growing from small to large, overridable by `cases=` and
    `hd test --cases N`.

39. **T39: `Debug` lives in `std.format` as a prelude name,** beside
    `Display`, so `@derive(Debug)` and `debug(x)` need no `use`. `std`
    implements it for primitives, collections, `T?`, `Result`, and tuples.
    This settles T33's home.
40. **T40: a final function parameter may follow defaulted parameters**
    (Kotlin's and Swift's rule). This relaxes
    [`fn.default.order`](../spec/07-functions.md#r-fn.default.order) for
    the last parameter when its type is a function type, since a trailing
    block or a named argument always supplies it. So `it` is an ordinary
    function, not an intrinsic; only its registration (T7, T16) is special.
    The rule is general: `retry(3, backoff=...):` works too. Returning a
    function from `it(name, options)` was considered and rejected: it
    needs a new trailing-block rule, and a bare call would register
    nothing.
41. **T41: `it_each`, `it_prop`, and `it_prop_with` follow `it`'s rules:**
    a string-literal name, the same options, top level of a `tests:` block
    or test module only. `it_each` rows are evaluated when that test runs,
    its body follows T15, and a `name[i]` may not equal another test's
    name.
42. **T42: `hd check` checks test code only with `--tests`,** like
    `cargo check`; `hd test` always compiles it.

43. **T43: an unknown `expect_panic` category is a compile error.** The
    categories are the specification's fixed list.
44. **T44: `pub` on an item inside a `tests:` block is an error.** Nothing
    outside the block sees it (T17); shared helpers go in a `_test.hd`
    module (T23).
45. **T45: a `_test.hd` module or a `tests/` file may not contain a
    `tests:` block.** The whole file is already test-only, with `it(...)`
    at its top level (T13).
46. **T46: under `tests/`, `pkg.<module>` names the library's modules with
    only their public API,** as a dependent sees them; other integration
    modules are `tests.<name>` (shared helpers in `tests/common.hd` are
    `use tests.common`), like Rust's `tests/common/`.
47. **T47: `it`, `it_each`, `it_prop`, and `it_prop_with` are only called
    directly in test position;** any other use, including as a value, is
    `misplaced-test-case`, so tests stay statically listable.
48. **T48: `Debug` is a structured writer.**
    `trait Debug: fn debug(self, out: mut DebugWriter) -> void`, with
    builder calls like Rust's `debug_struct` and `field`; the derived impl
    is a walker, and `debug(x) -> string` prints stable, multi-line,
    consistently indented output.
49. **T49: the snapshot API lives in `std.testing`, imported.**
    `snapshot(text: string, expect: string = "")` and
    `snapshot_file(text: string)`. `expect=` must be a string literal so
    `hd test --update` can rewrite it; an empty or missing `expect` is
    recorded on the first `--update`.
50. **T50 (2026-09-28): a property body follows T15.** `it_prop` and
    `it_prop_with` take `prop: fn!(T) -> R` with `R < Termination`: `void`,
    or `Result[void, Error]` when the body uses `?`. An `.Err` counts as a
    failing case and is shrunk like an assertion failure.
51. **T51 (2026-09-28): shrinking is capped by runs.** Each shrink attempt
    is a fresh instance, so shrinking stops after 500 attempts by default
    (Hypothesis's figure) and reports the smallest failing input so far,
    marked "shrinking stopped early". `shrink=` on `it_prop` and
    `hd test --shrink N` override it.

The `timeout=` value waits on the literal-suffix design (owner: design
now, so `timeout=5s` may replace the `"5s"` string).

```text
# std.testing (T40)
pub fn it[T < Termination, R](name: string, ignore: string? = .None,
                             expect_panic: string? = .None, timeout: string? = .None,
                             body: fn!() -> T $ R) -> void $ R
```

```text
# std.testing
pub fn it_prop[T < Arbitrary, R < Termination](name: string, cases: i32 = 100, prop: fn!(T) -> R) -> void
pub fn it_prop_with[T, R < Termination](name: string, gen: fn(mut Choices) -> T, cases: i32 = 100, prop: fn!(T) -> R) -> void
```

The signature, as spelled out on 2026-09-27 (T8 fixes `Outcome` as
`ExitCode`):

```text
# std.process: shared by main and tests
pub trait Termination:
    fn report(self) -> ExitCode

pub type ExitCode(u8)

impl Termination for void:
    fn report(self) -> ExitCode: ExitCode(0)

impl Termination for ExitCode:
    fn report(self) -> ExitCode: self

impl[T < Termination, E < Display] Termination for Result[T, E]:
    fn report(self) -> ExitCode:
        match self:
            .Ok(value) => value.report()
            .Err(_) => ExitCode(1)

# std.testing: `it` is a compiler intrinsic; this is its shape without options
pub fn it[T < Termination, R](name: string, body: fn!() -> T $ R) -> void $ R:
    pass
```

Whether a non-suspending block fits `fn!()` is a detail for the
specification pass. Applied 2026-09-27: T14 makes such a trailing block
suspending, and T18 moves printing out of `report`
([Exit Status](../spec/10-modules.md#exit-status)).

```text
use pkg.billing
use std.testing.assert_equal

fn late_fee(days: i32) -> i32:              # private
    if days > 30: 5 else: 0

tests:                                      # compiled only by `hd test`
    use dep.fake_clock.FakeClock            # test dependency
    fn at_noon() -> mut FakeClock: FakeClock::at("12:00")

    it("late fee after 30 days"):
        assert_equal(late_fee(31), 5, reason="the block sees the private function")

    it("bills on time"):
        $.with(Clock=at_noon()):            # unit tests get no host providers
            bill := billing.run!()?
            assert_equal(bill.total, 100, reason="an on-time bill has no fee")
        .Ok()
```

## Survey Notes

| Language | Test shape | Discovery | Test-only imports |
| --- | --- | --- | --- |
| Rust | `#[test] fn` returning `()` or `Result<(), E: Debug>` | attribute | `#[cfg(test)]` on any item or a `mod tests` |
| Go | `func TestX(t *testing.T)`, subtests with `t.Run` | name and signature | `_test.go` file imports |
| MoonBit | `test "name" { }` | syntax | `*_test.mbt` files, `test-import` in `moon.pkg.json` |
| Zig, D | `test "name" { }`, `unittest { }` | syntax | lazy analysis; `version(unittest) import` |
| Swift Testing | `@Test func`, `@Test(arguments:)` | macro | separate test target |
| Kotest, munit, Jest | `test("name") { }` | runtime registration | separate source set |

## Still Open

These questions came up while applying the decisions on 2026-09-27. Each
waits for the owner; the specification states none of them as a rule.
Items 1, 3-9, 11, and 12 are answered by T39-T49; item 2 waits on the
literal-suffix design; item 10 (fixture environments) is implementation work.

1. **`it` as an intrinsic.** Its options cannot be ordinary defaulted
   parameters before `body`, since
   [`fn.default.order`](../spec/07-functions.md#r-fn.default.order) puts
   defaults last. The specification therefore states `it` as an intrinsic.
   Confirm.
2. **`timeout` syntax.** The duration syntax beyond the `"5s"` example is
   not decided.
3. **Unknown panic category.** Is an `expect_panic` value that names no
   panic category an error?
4. **`pub` inside `tests:`.** Is `pub` on an item inside a `tests:` block
   an error?
5. **`tests:` in test modules.** May a `_test.hd` module or an integration
   test module have a `tests:` block? T13 says a test module holds `it`
   calls at its top level.
6. **Naming from `tests/`.** How does an integration test module name the
   library and the other integration test modules? What does `pkg` mean
   under `tests/`?
7. **`it` as a value.** May code use `it` as a value? The specification
   only rejects calls of `it` outside test code.
8. **`it_each` details.** T31 fixes the signature; these parts are open:

   | Question | Why it matters |
   | --- | --- |
   | Must its name be a string literal? | Listing tests without running them needs a static name. |
   | Does it take `ignore`, `expect_panic`, and `timeout`? | `it` takes them; the signature above has no place for them. |
   | May it be called outside the top level of test code? | `misplaced-test-case` covers `it` only. |
   | Is `rows` evaluated in each case's own instance? | Each case runs in a fresh instance (T21). |
   | How does `name[i]` meet `duplicate-test-name`? | An `it("name[0]")` beside `it_each("name", ...)` could clash. |
   | What result type has its closure body when it uses `?`? | T15 covers trailing blocks only, and the body is an explicit closure. |

9. **T33 is not applied.** `assert_equal[T < Eq + Debug]` needs `Debug`'s
   module, whether it is a prelude name, its members, and where `debug`
   lives. Eight conformance fixtures compare user types with `assert_equal`
   and would need a `Debug` implementation: `assert-equal-nominal`,
   `assert-equal-nominal-unequal`, `assert-equal-generic-nominal`,
   `assert-equal-generic-nominal-unequal`, `assert-equal-generic-primitive`,
   `assert-equal-generic-primitive-unequal`, `assert-equal-cross-check`, and
   `partial-equality-dispatch`. The prototype cannot parse `@derive` on data
   types either.
   **Recommendation:** declare `Debug` beside `Display` in `std.format` and
   put it in the prelude, since `assert_equal` bounds on it.
10. **Fixtures for test modules.** The conformance suite has no fixture
    environment for test modules, integration tests, or test dependencies.
    So `test-only-use` and `cyclic-test-dependency` have no fixtures.
11. **`hd check` and test code.** The conformance command contract now has
    `parse` and `check` cover a fixture's `tests:` block. Confirm that
    `hd check` type-checks test code, although only test builds compile it.
12. **Snapshot API.** The signatures of `snapshot` and `snapshot_file`,
    whether `expect=` must be a literal, and the `Debug` rendering format
    are open.

The earlier [Testing Stress Test](TESTING_STRESS_TEST.md) ranks 17 problems
found on 21 cases against T1-T13; T14-T27 answer its questions.
