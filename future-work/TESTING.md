# Testing Redesign

Status: design record, started 2026-09-27. Owner decisions T1-T12 are
decided; none is in the specification yet. The redesign may continue with
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
   (wrapping other errors in a standard message error), which is applied
   in the specification and must be revised.

5. **T5: `main` and tests share one `Termination` trait.** It lives in
   `std.process`; `main`'s entry-result rule and `test` both read
   `T < Termination`, so the special entry rule becomes an ordinary bound
   (Rust's model). `void` and `Result[void, E]` with `E < Display` implement
   it; `ExitStatus` and `StatusCode` still pick `main`'s exit code.
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
    module top level. A separate `*_test.hd` companion file was considered
    and dropped: a child module cannot see its parent's private names, and
    the block already groups test-only code (Rust's `#[cfg(test)] mod
    tests`).
12. **T12: property testing is a `std.testing` library.** A test calls, for
    example, `testing.check(fn(order: Order): ...)`; generators come from a
    derivable `Arbitrary` trait through typed derivation, and shrinking is a
    derived build. The API is designed with the standard library.

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
