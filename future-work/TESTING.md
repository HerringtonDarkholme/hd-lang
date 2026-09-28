# Testing Redesign

Status: design record, started 2026-09-27. Owner decisions T1-T27 are
decided. T4, T5, and T8 are applied to the specification (for the current
`test "name":` syntax); the others are not yet. The redesign may continue with
more issues (property testing is folded in from
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing)).

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
    already groups test-only code; T13 keeps `*_test.hd` as its own module (Rust's `#[cfg(test)] mod
    tests`).
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
    dependent does (T9).
14. **T14 (TS-1): a trailing block for an `fn!` parameter is suspending.**
    When the parameter's type is `fn!(...)`, the trailing block is a
    suspending closure, for every callee, not only tests; the `!` in the
    callee's signature is the visible marker (Kotlin's `suspend` lambdas).
15. **T15 (TS-3): a test body using `?` returns `Result[void, Error]`.**
    Its type is fixed, so every error type that implements `Error`
    converts; a body without `?` returns `void` (Zig's `anyerror!void`).
16. **T16 (TS-2): the test-case call is `it`, not `test`.** `test` stays
    free for user code. `it` is a prelude name, so it cannot be shadowed,
    and every statement at the top level of a `tests:` block or a
    `_test.hd` module must be an `it(...)` call. This renames T3's
    `test(...)` throughout.
17. **T17 (TS-4): `tests:` is an item block, like Rust's `mod tests`.** A
    top-level `tests:` block holds module items (uses, functions, data,
    and `it(...)` calls) visible only inside it; once per file, top level
    only, never nested. `tests` becomes a keyword.
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

The signature, as spelled out on 2026-09-27 (T8 fixes `Outcome` as
`ExitCode`):

```text
# std.process: shared by main and tests
pub trait Termination:
    fn report(self) -> ExitCode

type ExitCode(u8)

impl Termination for void                                  # ExitCode(0)
impl Termination for ExitCode                              # itself
impl[T < Termination, E < Display] Termination for Result[T, E]

# std.testing
pub fn test[T < Termination, R](name: string, body: fn!() -> T $ R) -> void $ R
```

Whether a non-suspending block fits `fn!()` is a detail for the
specification pass.

```text
use app.billing

fn late_fee(days: i32) -> i32: ...         # private

tests:                                      # compiled only by `hd test`
    use dep.fake_clock.FakeClock            # test dependency
    fn at_noon() -> mut FakeClock: FakeClock.at("12:00")

    test("late fee after 30 days"):
        assert_equal(late_fee(31), 5)       # sees the private function

    test("bills on time"):
        $.with(Clock=at_noon()):
            bill := billing.run!()?
            assert_equal(bill.total, 100)
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

- [Testing Stress Test](TESTING_STRESS_TEST.md) ranks 17 problems found
  on 21 cases against T1-T13 and lists the owner questions they raise.
