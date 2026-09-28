# Testing Redesign

Status: design record, started 2026-09-27. Owner decisions T1-T4 are
decided; none is in the specification yet. The redesign may continue with
more issues (property testing is folded in from
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing)).

## Owner Decisions

1. **T1: `@test` marks test-only items.** `@test` on any top-level item (a
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
   form is the one intrinsic.
4. **T4: a test body follows the entry-point result rule.** Like Rust's
   `Termination`, a test body returns `void` or `Result[void, E]` with
   `E < Display`. `?` works through ordinary rules; an `.Err` fails the
   test and prints like a failing `main`, cause chain included. This
   replaces [Error Conversion decision 16](ERROR_CONVERSION.md#owner-decisions)
   (wrapping other errors in a standard message error), which is applied
   in the specification and must be revised.

```text
use app.billing
@test use dep.fake_clock.FakeClock

@test
fn at_noon() -> mut FakeClock: FakeClock.at("12:00")

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

- Property testing API (library-level, `std.testing`).
- Whether `tests/` (the separate test root) keeps its current role.
- Test filtering, naming rules for duplicate names, and parameterized tests.
