# Testing Redesign: Stress Test (T1-T13)

Status: design review, 2026-09-27; changes no decision, design record, spec
text, or prototype code. Nothing here is accepted language behavior. Every
design choice below is a question for the owner.

The design under test is [Testing Redesign](TESTING.md), owner decisions
T2-T13 in its [Owner Decisions](TESTING.md#owner-decisions) (T1 is
superseded by T11). Spec sections relied on:
[Test Blocks](../spec/02-grammar.md#test-blocks),
[Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks),
[Runtime Panics](../spec/06-control-flow.md#runtime-panics),
[Trailing Callback Blocks](../spec/07-functions.md#trailing-callback-blocks),
[Standard Testing](../spec/10-modules.md#standard-testing),
[Executable Entry Point](../spec/10-modules.md#executable-entry-point),
[Suspending Closures](../spec/11-requirements-and-suspension.md#suspending-closures),
[Bang Calls And Driver Contexts](../spec/11-requirements-and-suspension.md#bang-calls-and-driver-contexts),
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers),
[Determinism](../spec/11-requirements-and-suspension.md#determinism), and
[Typed Derivation](../spec/14-annotations.md#typed-derivation). Design
records relied on: [Runtime And Library, Testing](RUNTIME_AND_LIBRARY.md#testing),
[Packages](PACKAGES.md) (test dependencies and the `tests` root),
[Standard Library, Testing Layer](STDLIB.md#testing-layer), and
[Durable Replay](DURABLE_REPLAY.md). Another change is moving the spec to
`ExitCode` and `Termination` at the same time; this report reads T5 and T8
as authoritative.

Problem ids are TS-1 to TS-17. No earlier testing stress test exists.

## Contents

1. [Surface Being Tested](#surface-being-tested)
2. [Method](#method)
3. [Summary](#summary)
4. [Cases](#cases) (1-21)
5. [Comparison With Other Languages](#comparison-with-other-languages)
6. [Problems, Ranked](#problems-ranked)
7. [Questions For The Owner](#questions-for-the-owner)
8. [Parse Log](#parse-log)

## Surface Being Tested

The record fixes these parts:

| Part | Decision | As this report reads it |
| --- | --- | --- |
| `tests:` block | T11, T13 | At most one per file. Compiled only by `hd test`. Sees the file's private names. Holds `use`, helpers, data types, and `test(...)` calls. Code outside cannot name what is inside. |
| `test` | T3, T5, T7 | `std.testing.test`, a library intrinsic. Registered only at the top level of a `tests:` block or of a `*_test.hd` module, with a string literal name. |
| Signature | T5, T8 | `pub fn test[T < Termination, R](name: string, body: fn!() -> T $ R) -> void $ R` |
| Result | T4, T8 | `void`, `ExitCode`, or `Result[T, E]` with `E < Display`. A test fails when `report()` is not `ExitCode(0)`. `?` follows ordinary rules. |
| Host providers | T6 | A body's row is a row parameter. The runner binds it from "the test profile (for example `hd test --grant ...`)". |
| Three views | T9, T13 | Same file: private names. `*_test.hd`: package `pub` names, test dependencies at top level. `tests/`: the library's public API only. |
| Names | T10 | Unique per module. Id `module::name`. `hd test TEXT` runs ids containing `TEXT`. |
| Property testing | T12 | `testing.check(fn(order: Order): ...)` with a derivable `Arbitrary`. Shrinking is a derived `build`. |

Five assumptions go beyond the record. Each case says when it uses one.

| Id | Assumption | Why |
| --- | --- | --- |
| A1 | Each test still runs in its own fresh program instance ([`grammar.test.instance`](../spec/02-grammar.md#test-blocks)). | No decision removes it, and the conformance runner relies on it. |
| A2 | A failed assertion ends the test as a panic of category `assertion-failed`. | [`module.testing.failure`](../spec/10-modules.md#standard-testing) says only that it "reports test failure"; `assertion-failed` is the only matching panic category. |
| A3 | Test-only names are imported with `use` inside the `tests:` block, as T11 says. Examples after case 1 omit those `use` lines. | Brevity. Parsing does not resolve names. |
| A4 | `std.testing` also holds `assert_ok`, `assert_err`, and `assert_contains`, and `std.time` holds `ManualClock`. | [STDLIB Testing Layer](STDLIB.md#testing-layer) drafts them. |
| A5 | Proposed named arguments on `test` (`ignore=`, `expect_panic=`, `timeout=`) are marked `# proposed API`. | They parse as ordinary named arguments, but the signature has no such parameters. |

## Method

Cases follow what real test suites do: Rust (`cargo test`, tokio, proptest,
insta, criterion, rustdoc), Go (`testing`, `t.Run`, `t.Cleanup`,
`t.Parallel`, golden files, fuzzing, benchmarks, examples), Swift Testing,
Kotest, munit, Zig, and MoonBit. Library shapes are approximations written
from their documentation. Each case gives the original, the hd translation,
how it behaves under T2-T13, and a verdict.

Verdicts: *works* (the design expresses it and it behaves like the
original); *friction* (it works with a workaround or a visible behavior
difference); *breaks* (the design cannot express it, or the record leaves
the behavior undefined in a way a user hits). Cases 11-13 test advanced
features. The shared rules say those wait for the core, so their problem
(TS-15) records direction only.

Parsing checks syntax only. Nothing here was type-checked. The reference
parser has no `tests:` production yet: it reads `tests:` as a call to a
function named `tests` with a trailing block. So a `use` or a decorator
inside the block is a syntax error today, and those lines end in
`# hypothetical syntax`.

## Summary

| # | Case | Original | Verdict | Design element at fault |
| --- | --- | --- | --- | --- |
| 1 | The record's own example | Rust `mod tests` | Breaks | `use` inside `tests:` has no grammar (TS-4); `use app...` and a missing `reason=` (TS-16) |
| 2 | `?` over two error types | Rust `-> Result<(), Box<dyn Error>>` | Breaks | Trailing block's result is inferred; no common `E` (TS-3) |
| 3 | Async test; sync code using `block_on` | tokio `#[tokio::test]`, Kotlin `runTest` | Breaks | A trailing block never becomes `fn!` (TS-1); every test is an active driver (TS-2) |
| 4 | Setup, teardown, shared fixtures | Go `t.Cleanup`, `t.TempDir`, `TestMain` | Friction | No cleanup after a failed assertion (TS-5); no once-per-run fixture (TS-11) |
| 5 | Timeouts and cancellation | Go `-timeout`, Swift `.timeLimit` | Friction | No per-test timeout (TS-7); auto-advancing virtual time |
| 6 | Host providers versus fakes | reqwest live test, Go `httptest` | Breaks | No test profile; `--grant` undefined (TS-6) |
| 7 | Output capture | libtest capture, Go examples | Works | Capture needs a runner rule (TS-11) |
| 8 | Expected panics and errors | `#[should_panic]`, Kotest `shouldThrow` | Errors work; panics **break** | Panics are uncatchable; no option (TS-5, TS-7) |
| 9 | Skipping and platform-conditional tests | `#[ignore]`, `#[cfg]`, `t.Skip` | Breaks | No option, no `cfg` (TS-7, TS-6) |
| 10 | Table tests | Go `t.Run`, Swift `arguments:` | Friction | First failure hides the rest (TS-14) |
| 11 | Snapshot and golden files | insta, expect-test, MoonBit `inspect` | Breaks (waits) | No source location, no package files (TS-15) |
| 12 | Doc tests | rustdoc, Go `Example` | Breaks (waits) | No rule (TS-15) |
| 13 | Benchmarks | criterion, Go `b.Loop` | Breaks (waits) | No form, no real clock (TS-15) |
| 14 | Parallelism and isolation | libtest threads, `t.Parallel` | Works | Instance per test (A1); host providers shared (TS-11) |
| 15 | Determinism and flaky tests | proptest seeds, Go `-shuffle` | Friction | Hash seed moves with test code (TS-13) |
| 16 | Property testing and shrinking | proptest, Hypothesis, Kotest | Friction | Panics cannot be caught for shrinking (TS-8); where `Arbitrary` lives (TS-9) |
| 17 | Helpers shared across test files | Rust `tests/common/mod.rs` | Friction | Visibility between the three kinds unspecified (TS-9) |
| 18 | Test dependency that depends back | Cargo dev-dependency cycle | Breaks | Two copies of the package (TS-10) |
| 19 | Durable replay and conformance runner | none | Friction | 362 fixtures and the runner contract; file-name rule (TS-16) |
| 20 | Listing, and the name `test` itself | Zig, Kotest | Breaks silently | A local `test` turns registration into a plain call (TS-2) |
| 21 | `tests:` grammar edge cases | Rust `mod tests` | Breaks | Nested, repeated, statement-bearing blocks all parse (TS-4); `_test.hd` suffix (TS-17) |

The core holds up well. One fresh instance per test gives isolation that
Rust and Go only get by convention (case 14). Providers make fakes
ordinary values, so output capture and fake clocks need no mocking library
(cases 4, 7). A shared `Termination` puts `main` and tests on one rule.
Four gaps stop ordinary suites. A trailing block cannot become the `fn!()`
body, so no async test compiles (TS-1). The body's inferred result makes
`?` over two error types fail (TS-3). A module-local `test` silently turns
registrations into plain calls (TS-2). And the `tests:` block has no
grammar, so the record's own example does not parse (TS-4).

## Cases

### 1. The Record's Own Example

The record's second example fails at line 1: `use app.billing` has no valid
root, since a use path starts with `std`, `pkg`, `dep`, `self`, or `super`.
With `pkg`, it fails at the `use` inside the block. It also calls
`assert_equal` without the `reason` that
[`module.testing.reason`](../spec/10-modules.md#standard-testing) requires.
Corrected, with the one line the grammar lacks marked:

```text
use pkg.billing
use std.time.Clock

fn late_fee(days: i32) -> i32:
    if days > 30: 5 else: 0

tests:
    use dep.fake_clock.FakeClock  # hypothetical syntax
    fn at_noon() -> mut FakeClock:
        FakeClock::at("12:00")

    test("late fee after 30 days"):
        assert_equal(late_fee(31), 5, reason="one day late")

    test("bills on time"):
        $.with(Clock=at_noon()):
            bill := billing.run!()?
            assert_equal(bill.total, 100, reason="no fee at noon")
        .Ok()
```

Without the inner `use`, the rest parses:

```text
use pkg.billing

fn late_fee(days: i32) -> i32:
    if days > 30: 5 else: 0

tests:
    fn at_noon() -> mut FakeClock:
        FakeClock::at("12:00")

    test("late fee after 30 days"):
        assert_equal(late_fee(31), 5, reason="one day late")
```

The second test also hits TS-1: `billing.run!()` is a bang call inside a
trailing block, which is never suspending under
[`req.suspend.closure.inference`](../spec/11-requirements-and-suspension.md#suspending-closures).

**Verdict: breaks** (TS-4, TS-1, TS-16).

### 2. `?` Over Two Error Types

Rust tests that read a fixture and parse it usually write one error type:

```rust
#[test]
fn loads_config() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string("tests/data/app.toml")?;
    let config: Config = toml::from_str(&text)?;
    assert_eq!(config.port, 8080);
    Ok(())
}
```

```text
tests:
    test("loads config"):
        text := read_fixture("app.toml")?   # Result[string, FixtureError]
        config := parse_config(text)?       # Result[Config, ParseError]
        assert_equal(config.port, 8080, reason="default port")
        .Ok()
```

T4 makes `?` follow ordinary rules, and T5 makes the body `fn!() -> T`
with `T` inferred. A trailing block cannot state its result type. Under
[`expr.try.convert.inferred-closure`](../spec/05-expressions.md#error-conversion),
each `?` then contributes its own `E` to the inferred result, and
`FixtureError` and `ParseError` have no common type. The old test-block
rule gave `Result[void, Error]` as the target, which T4 removed along with
the message wrapping. A related effect: a body whose last line is a call
returning `i32` infers `T = i32`, which is not `Termination`.

The one form that works today is an explicit closure, which parses:

```text
tests:
    test("loads config", fn!() -> Result[void, Error]:
        text := read_fixture("app.toml")?
        config := parse_config(text)?
        assert_equal(config.port, 8080, reason="default port")
        .Ok()
    )
```

**Verdict: breaks** for the trailing-block form (TS-3).

### 3. Async Tests, And Sync Code Using `block_on`

```rust
#[tokio::test]
async fn fetches_user() {
    let user = client().user(1).await.unwrap();
    assert_eq!(user.name, "Ada");
}
```

Kotlin's `runTest { ... }` takes a `suspend` lambda, and the compiler makes
the block suspending because the parameter type is suspending
([runTest](https://kotlinlang.org/api/kotlinx.coroutines/kotlinx-coroutines-test/kotlinx.coroutines.test/run-test.html)).

```text
tests:
    test("fetches a user"):
        $.with(Http=scripted_http()):
            user := fetch_user!(UserId(1))?
            assert_equal(user.name, "Ada", reason="first user")
        .Ok()
```

The parameter is `fn!()`, but a trailing block is "equivalent to a
zero-argument closure" ([`fn.trailing.closure`](../spec/07-functions.md#trailing-callback-blocks)),
and closure inference "never silently makes a closure suspending". So
`fetch_user!` is a bang call outside a driver context. The old spec named
"a `test` block" as a driver context
([`req.bang.driver-context`](../spec/11-requirements-and-suspension.md#bang-calls-and-driver-contexts));
that production is gone.

The opposite case also fails. A sync helper that drives a suspension with
`block_on` is legal in a plain function:

```text
use std.task.block_on

fn load_now(path: string) -> string $ FsRead:
    let pending: mut Suspend[string] = load(path)
    block_on(pending)

tests:
    test("loads a file synchronously"):
        $.with(FsRead=memory_fs_with("a.txt", "x")):
            assert_equal(load_now("a.txt"), "x", reason="file text")
```

If the runner drives every body as a suspension, `block_on` panics with
`suspension-nested-driver`
([`req.drive.block-on.nested`](../spec/11-requirements-and-suspension.md#driving-a-stored-suspension)).
The old rule [`req.bang.test-active`](../spec/11-requirements-and-suspension.md#bang-calls-and-driver-contexts)
made that explicit. Rust avoids the clash because a sync `#[test]` has no
runtime; only `#[tokio::test]` bodies do.

**Verdict: breaks** (TS-1, TS-2).

### 4. Setup, Teardown, And Shared Fixtures

Go registers cleanup from a helper and gets a temporary directory that the
testing package removes, even after `t.FailNow`
([testing](https://pkg.go.dev/testing)):

```go
func TestStore(t *testing.T) {
    dir := t.TempDir()
    db := openDB(t, dir) // calls t.Cleanup(db.Close)
    // ...
}
```

In hd a helper cannot register cleanup in its caller's frame, so a fixture
wraps a block. `defer` runs on normal exit and on `?`:

```text
tests:
    fn with_store[R](body: fn() -> void $ R + Store) -> void $ R:
        let store: mut MemoryStore = MemoryStore::new()
        defer:
            store.close()
        $.with(Store=store):
            body()

    test("saves a user"):
        with_store:
            save_user(User { id: 1, name: "Ada" })
            assert_equal(count_users(), 1, reason="one saved user")

    test("a hermetic run overrides one provider"):
        $.with(hermetic(seed=7)..., Env=MapEnv { values: {"MODE": "ci"} }):
            assert_equal(mode(), "ci", reason="env override wins")
```

Both styles work for fakes. Two gaps remain:

- **Cleanup after failure.** Under A2 a failed assertion is a panic, and a
  panic runs no `defer` ([`flow.defer.panic`](../spec/06-control-flow.md#deferred-cleanup)).
  For a fake this is harmless, since the instance is discarded. For a
  granted host resource, such as a real directory, nothing removes it.
- **Once-per-run fixtures.** Go's `TestMain` and munit's `beforeAll` start
  one expensive service for a whole run. In hd each test is a fresh
  instance, and module initialization must be requirement-free
  ([`module.init.requirement-free`](../spec/10-modules.md#requirement-free-initialization)).
  Such a service can only come from the host.

A bang helper also needs `()`: `with_db!():` parses and `with_db!:` does
not, because the bang suffix requires an argument clause.

**Verdict: friction** (TS-5, TS-11).

### 5. Timeouts And Cancellation

Go panics a test binary after `-timeout` (default 10 minutes), Swift has a
`.timeLimit` trait, and munit fails an async test after `munitTimeout`.
The record has no per-test timeout and no runner default. Cancellation
itself works well: a runner that cancels a timed-out body runs its `defer`
suites ([`req.cancel.defer`](../spec/11-requirements-and-suspension.md#cancellation)).
A non-suspending infinite loop cannot be cancelled cooperatively, so the
runner must kill the instance.

Testing timeout logic with a fake clock is harder. Virtual time
auto-advances ([STDLIB decision 10](STDLIB.md#owner-decisions)), so both
sleeps in a race return at once:

```text
tests:
    test("slow answer still beats a five second limit"):
        let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
        $.with(Clock=clock):
            let task: mut Suspend[i32] = slow_answer()   # sleeps one second, then 42
            outcome := timeout!(Duration::seconds(5), task)
            assert(outcome.is_completed(), reason="one second is under five")
```

With auto-advance, which sleep finishes first depends on poll order, not
on the durations. Go's `testing/synctest` and Kotlin's `runTest` advance
time only when every task is blocked, which orders timers correctly. The
record already defers idle-driven time, so this report only notes it.

**Verdict: friction** (TS-7; virtual time is out of scope).

### 6. Host Providers Versus Fakes

A live network test in Rust is usually ignored by default:

```rust
#[test]
#[ignore = "needs network"]
fn fetches_example() { /* reqwest::blocking::get(...) */ }
```

```text
tests:
    test("fetches example.com"):
        page := http_get!("https://example.com")?
        assert_contains(page, "Example Domain", reason="the live page loads")
        .Ok()
```

The body's row is `$ Net`. T6 says the runner binds it from "the test
profile (for example `hd test --grant ...`)". Nothing defines that
profile. A library package has no `[[executable]]` and so no profile in
`hd.toml`, and `--grant` appears nowhere else. For `main`, a row key
outside the profile is the compile error `nonhost-entry-requirement`. If
tests follow that rule, one network test breaks the whole `hd test` build
on a machine without the grant.

The fake side works: `$.with(Net=scripted_net())` gives an empty row.

**Verdict: breaks** (TS-6).

### 7. Output Capture

```text
fn greet(name: string) -> void $ Console:
    println("hello, $name")

tests:
    test("greets by name"):
        let console: mut BufferConsole = BufferConsole::new()
        $.with(Console=console):
            greet("Ada")
        assert_equal(console.output(), ["hello, Ada"], reason="one greeting line")
```

This works: `Console` is a mutable requirement trait, so the buffer
records through `write_line!`, and the test reads it through its own `mut`
alias. It replaces Go's `// Output:` examples and Rust's capture tricks.

A test that prints without `$.with` gets `Console` from the runner. libtest
captures such output per test and shows it only on failure; Go shows
`t.Log` output on failure or with `-v`. Since each test is its own instance
with its own providers (A1), the runner can bind a per-test buffer. The
record does not say it does.

**Verdict: works**; capture needs a runner rule (TS-11).

### 8. Expected Panics And Expected Errors

```rust
#[test]
#[should_panic(expected = "index out of bounds")]
fn first_of_empty_panics() { first(&[]); }
```

Expected errors work with ordinary assertions. Expected panics cannot be
written, because a panic is uncatchable in hd source
([`flow.panic.no-handler`](../spec/06-control-flow.md#panic-behavior)):

```text
tests:
    test("empty input is an error"):
        error := assert_err(parse_digit(""), reason="empty is not a digit")
        assert_equal(error, "not a digit: ", reason="message names the input")

    test("first of an empty list panics", expect_panic="index-out-of-bounds"):  # proposed API
        _ := first_item([])
```

The runner already sees each instance's panic and its stable category
([`flow.panic.category-set`](../spec/06-control-flow.md#panic-categories)).
The conformance runner judges `runtime/panic` cases exactly that way. Swift
reached the same place with exit tests, which run a closure in a child
process ([ST-0008](https://github.com/swiftlang/swift-evolution/blob/main/proposals/testing/0008-exit-tests.md)).

**Verdict:** errors **work**; panics **break** (TS-5, TS-7).

### 9. Skipping And Platform-Conditional Tests

Rust has `#[ignore = "reason"]` and `#[cfg(target_os = "windows")]`. Go
calls `t.Skip` at run time. Swift has `.disabled("reason")` and
`.enabled(if:)`. Zig returns `error.SkipZigTest`. The record has none of
these, and [Packages decision 12](PACKAGES.md#owner-decisions) rules out
conditional compilation.

In hd a platform difference is mostly a host capability difference: a
browser profile has no `FsWrite`. A test's row already says which host
traits it needs (case 6). So "run only where `FsWrite` exists" could fall
out of the row, with no `cfg`.

```text
tests:
    test("writes a report file", ignore="flaky on shared runners"):  # proposed API
        write_report!("out.txt")?
        .Ok()
```

**Verdict: breaks** (TS-7, TS-6).

### 10. Table Tests

```go
for _, tc := range cases {
    t.Run(tc.name, func(t *testing.T) {
        if got := parseDigit(tc.in); got != tc.want { t.Errorf("...") }
    })
}
```

T7 allows one registered test with a loop:

```text
tests:
    data Row:
        input: string
        want: i32

    test("parses digits"):
        rows := [Row { input: "7", want: 7 }, Row { input: "0", want: 0 }]
        for row in rows:
            digit := parse_digit(row.input)?
            assert_equal(digit, row.want, reason="input ${row.input}")
        .Ok()
```

This is what Rust's standard harness offers too (rstest adds cases by
macro). Under A2 the first failing row ends the test, so later rows are
not checked, and one row cannot be run by filter. Go's `t.Run` and Swift's
`@Test(arguments:)` report each case separately. `reason` carries the row,
so the failure is still located.

**Verdict: friction** (TS-14).

### 11. Snapshot And Golden Files

insta's `assert_snapshot!`, expect-test's `expect![[...]]`, and MoonBit's
`inspect(x, content="...")` compare against stored text and rewrite it on
request (`cargo insta review`, `UPDATE_EXPECT=1`, `moon test --update`). Go
reads golden files from `testdata/`, which the go tool ignores, with a
project-defined `-update` flag.

```text
tests:
    test("renders an invoice"):
        snapshot(render(sample_invoice()), expected="""  # proposed API
            INVOICE 42
            total: 2.50
            """)
```

An inline form needs the call's source location, which hd does not expose,
and a runner that rewrites source. A golden file needs `FsRead` on the
package directory, which a test does not have by default.

**Verdict: breaks**, and waits for the core (TS-15).

### 12. Doc Tests

rustdoc compiles each fenced block in a doc comment as its own crate that
sees only the public API. Go runs `func ExampleX()` and compares its
output to a trailing `// Output:` comment.

```text
## Adds two numbers.
##
## ```hd
## assert_equal(add(2, 3), 5, reason="small sums")
## ```
pub fn add(a: i32, b: i32) -> i32:
    a + b
```

The doc comment parses and attaches to `add` (its text becomes the shape's
`doc`). No rule says `hd test` runs it, with which view, or with which
imports.

**Verdict: breaks**, and waits (TS-15).

### 13. Benchmarks

Rust's `#[bench]` is still unstable; criterion runs from `benches/` with
`harness = false`. Go has `func BenchmarkX(b *testing.B)` with `b.Loop()`
(Go 1.24). A benchmark needs a real monotonic clock, which in hd is a host
`Clock` grant, and a registration form that `hd test` does not run by
default. The record has neither.

**Verdict: breaks**, and waits (TS-15).

### 14. Parallelism And Isolation

```text
let seen: mut List[string] = []

fn remember(name: string) -> void:
    seen.append(name)

tests:
    test("remembers one name"):
        remember("Ada")
        assert_equal(seen.len(), 1, reason="one name")

    test("starts empty"):
        assert_equal(seen.len(), 0, reason="no state leaks between tests")
```

Under A1 each test gets a fresh instance, so module state never leaks and
tests may run in parallel safely. In Rust the same pair over a
`static Mutex<Vec<String>>` passes or fails depending on order and thread
timing. This is a real strength of hd's design.

What is shared is the host: two parallel tests granted one real
filesystem or one database race, as in Rust (the `serial_test` crate) and
Go (`t.Parallel` is opt-in). Swift runs tests in parallel by default and
offers `.serialized`. The record does not restate A1 or say whether tests
run in parallel.

**Verdict: works** for module state; friction for host providers (TS-11).

### 15. Determinism And Flaky Tests

Seeds, clocks, and randomness work through providers:
`hermetic(seed=7)` gives `SeededRandom`, `ManualClock`, and `MemoryFs`.
Hash values are the exception:

```text
tests:
    test("shard assignment is stable"):
        assert_equal(shard_of("user-42", 8), 3, reason="pinned shard")
```

`shard_of` uses `hash_of`, whose seed comes from the code identity and the
runtime profile ([`req.determinism.hash-seeded`](../spec/11-requirements-and-suspension.md#determinism)).
The code identity covers the entry module and its dependencies
([Durable Replay decision 9](DURABLE_REPLAY.md#owner-decisions)). A test
build's identity includes test code, so adding an unrelated test, or
granting another profile, can change the expected `3`. Rust randomizes
`HashMap` seeds per process, so such a test fails at once there. In hd it
passes until an unrelated edit. Map iteration is insertion-ordered, so only
explicit hashing is exposed.

Flakiness can only enter through host providers. Durable replay records
every run's history, so a failing host-dependent test could be replayed
offline. The record does not say whether `hd test` keeps histories.

**Verdict: friction** (TS-13).

### 16. Property Testing And Shrinking

```rust
proptest! {
    #[test]
    fn total_is_never_negative(order in any::<Order>()) {
        prop_assert!(order.total() >= 0);
    }
}
```

```text
use std.structure.Structure

data Order:
    lines: List[Line]
    discount_cents: i64

tests:
    impl Arbitrary for Order by Structure:
        discount_cents += [range(0, 10000)]

    test("total is never negative"):
        check(fn(order: Order): assert(order.total() >= 0, reason="totals stay non-negative"))
```

Generation fits typed derivation well. `build` with a `Source` that reads
from a random choice sequence is a generator. Shrinking the sequence and
rebuilding is shrinking, as Hypothesis does, with no derived `shrink`
method. Rust's `arbitrary` crate builds values from an `Unstructured` byte
buffer the same way, and cargo-fuzz reuses it. T12's "shrinking is a
derived build" holds up.

Two parts do not:

- **Panics.** proptest catches a panicking case with `catch_unwind`, then
  shrinks. In hd, `assert` failing (A2), an overflow in `total()`, or an
  index error ends the instance. A library `check` running inside that
  instance never sees the failure, so it cannot shrink.
- **Placement.** A derivation block must sit in the module that declares
  `Order` ([`annot.block.module`](../spec/14-annotations.md#derivation-blocks)).
  The block above is inside `tests:`; whether that counts as the module is
  TS-4. A `*_test.hd` module or a `tests/` module cannot derive it at all.
  Decorators inside `tests:` do not parse today, so `@derive(Arbitrary)` on
  a test-only type cannot be written there.

**Verdict: friction** (TS-8, TS-9, TS-4).

### 17. Helpers Shared Across Test Files

```text
# src/fixtures_test.hd: compiled only by `hd test`
use pkg.billing.Order
use pkg.billing.Line

pub fn sample_order() -> Order:
    Order { lines: [Line { sku: "A1", cents: 250 }], discount_cents: 0 }
```

```text
# src/billing_test.hd
use pkg.fixtures_test.sample_order
use pkg.billing.total
use std.testing.test
use std.testing.assert_equal

test("sums one line"):
    assert_equal(total(sample_order()), 250, reason="one line of 250")
```

```text
# tests/checkout.hd: integration test
use pkg.fixtures_test.sample_order  # error? not part of the public API
```

A `tests:` block's helpers are file-private (T11), so shared helpers need a
`*_test.hd` module. T13 says such a module sees package `pub` names "like
any sibling module"; the record does not say another `*_test.hd` module
may use it. `tests/` sees only the public API, so it cannot use
`fixtures_test`. Rust's answer is `tests/common/mod.rs`, a helper module
that is not itself a test crate. Whether one `tests/` module may use
another is unspecified.

Fixtures for downstream packages work without features. STDLIB decision 4
already puts `ManualClock` beside `Clock` as ordinary `pub` code, where
tokio needs a `test-util` feature.

**Verdict: friction** (TS-9).

### 18. A Test Dependency That Depends Back

```toml
# acme/billing hd.toml
[test-dependencies]
fixtures = "acme/billing_fixtures@1.0.0"   # itself depends on acme/billing
```

```text
tests:
    use dep.fixtures.sample_order  # hypothetical syntax
    test("totals a sample"):
        assert_equal(total(sample_order()), 250, reason="sample total")
```

`sample_order()` returns the `Order` of the published `acme/billing`,
while `total` here is the test build of the local package. Cargo allows
such dev-dependency cycles and ends up with two copies of the crate, so
users see "expected `Order`, found `Order`". Go allows the cycle only from
an external test package (`package billing_test`), which sees the one
normal build. The record says nothing.

**Verdict: breaks** (TS-10).

### 19. Durable Replay And The Conformance Runner

The conformance suite uses the old `test "..."` production in 362
fixtures. Its runner contract says each `test "..."` block runs in its own
fresh instance after module initialization
([Runtime Execution](../spec/conformance/README.md#runtime-execution)). All
of that moves to `tests:` and `test(...)`. The suite also says a fixture
"must not depend on its file name", but T13 gives `*_test.hd` a meaning. A
fixture that tests the standalone form needs a package role instead.

Replay adds a capability rather than a conflict: a history recorded from a
failing host-dependent test could be replayed offline (case 15).

**Verdict: friction** (TS-16).

### 20. Listing, And The Name `test` Itself

Zig and MoonBit make `test` syntax, so a tool lists tests by parsing. Kotest
and munit register at run time, so listing needs execution. T7 sits
between: a literal name at a fixed place. But registration depends on name
resolution:

```text
fn test(name: string, body: fn() -> void) -> void:
    body()

tests:
    test("never registered"):
        assert(false, reason="this never runs")
```

The tests block sees the file's private names (T11). If it forgets
`use std.testing.test`, `test("never registered")` calls the module's own
function. Nothing is registered, nothing runs, and `hd test` reports
success. The same happens with `use std.testing.test as spec`: a lister
that looks for the text `test(` misses `spec(...)`. The record does not say
whether `test` needs an import, whether it may be shadowed or renamed, or
whether it may be used as a value.

**Verdict: breaks silently** (TS-2).

### 21. `tests:` Grammar Edge Cases

Each block below parses today, because `tests:` is read as a call with a
trailing block:

```text
fn helper() -> void:
    tests:
        pass

tests:
    let shared: mut List[i32] = []
    test("a"):
        pass

tests:
    pass
```

The first is a `tests:` inside a function; the last is a second block. The
middle one has a binding at block level: when would it run, and is it
shared across tests? These need rules:

| Question | Record says |
| --- | --- |
| A block inside a function, or nested in `tests:` | nothing |
| Two blocks in one file | "one per file" (T11), no error named |
| Bindings or other statements at block level | nothing |
| `pub`, decorators, `impl`, derivation blocks inside | "helpers, data types"; `pub` and decorators do not parse |
| A `tests:` block in a `*_test.hd` file or a `tests/` file | T13: `*_test.hd` uses top-level calls; `tests/` unspecified |
| A `tests:` block in a script or entry module | nothing |
| An inner name that shadows a module name | nothing |
| Top-level `test(...)` in `*_test.hd` versus requirement-free initialization | nothing |

The suffix also claims production names. A module for an A/B test feature
named `src/ab_test.hd`, or a `load_test.hd` command, silently becomes
test-only. Any normal module that uses it then fails to build, so the error
is loud. Go has the same rule for `_test.go`.

**Verdict: breaks** (TS-4, TS-17).

## Comparison With Other Languages

| Concern | Rust | Go | Swift Testing | Kotest, munit | Zig | MoonBit | hd (T1-T13) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Test form | `#[test] fn` | `func TestX(t *testing.T)` | `@Test func` | `test("name") { }`, run-time registration | `test "name" { }` | `test "name" { }` | `test("name"):` in `tests:` |
| Test-only code | `#[cfg(test)] mod tests` | `_test.go` | separate target | separate source set | lazy analysis | `_test.mbt`, `_wbtest.mbt` | `tests:` block, `*_test.hd`, `tests/` |
| Result | `()` or `Result<(), E: Debug>` via `Termination` | none; `t.Error`, `t.Fatal` | `throws` | exceptions | `anyerror!void`, inferred | none; `raise` | `T < Termination` |
| Failed check | panic, stops | `t.Error` continues, `t.Fatal` stops | `#expect` continues, `#require` stops | exception, stops | error return, stops | `raise`, stops | A2: panic, stops |
| Async | `#[tokio::test]` | goroutines | `async` tests | `runTest`, suspend lambda | none | — | `fn!()` body; TS-1 |
| Expected panic | `#[should_panic]` | `recover` | exit tests (6.2) | `shouldThrow`, `intercept` | none | none | none |
| Skip | `#[ignore]` | `t.Skip` | `.disabled`, `.enabled(if:)` | `enabled=`, `.ignore` | `error.SkipZigTest` | none | none |
| Timeout | none (nextest adds one) | `-timeout` 10 m | `.timeLimit` | `timeout=`, `munitTimeout` | none | none | none |
| Parameterized | loop, rstest | `t.Run` | `arguments:` | `withData` | loop | loop | loop (T7) |
| Parallel | threads by default | opt-in `t.Parallel` | by default | configurable | serial | — | unspecified (TS-11) |
| Isolation | shared process | shared process | fresh suite instance | isolation modes | shared process | shared process | fresh instance per test (A1) |
| Capture | `println!` captured | `t.Log` on failure | — | — | — | — | provider per test, unspecified |
| Snapshots | insta, expect-test | golden files | none | none | none | `inspect` | none |
| Doc tests | rustdoc | `Example` | none | none | none | doc tests | none |
| Benchmarks | criterion | `Benchmark`, `b.Loop` | none | none | none | `moon bench` | none |
| Property | proptest, quickcheck | fuzzing (1.18) | none | kotest-property | fuzzing (0.14) | `quickcheck` | T12 |

Sources: [Rust Book ch. 11](https://doc.rust-lang.org/book/ch11-00-testing.html),
[rustc tests](https://doc.rust-lang.org/rustc/tests/index.html),
[`Termination`](https://doc.rust-lang.org/std/process/trait.Termination.html),
[rustdoc documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html),
[Go `testing`](https://pkg.go.dev/testing),
[`go help test`](https://pkg.go.dev/cmd/go#hdr-Test_packages),
[Go fuzzing](https://go.dev/doc/security/fuzz/),
[`testing/synctest`](https://pkg.go.dev/testing/synctest),
[Swift Testing](https://developer.apple.com/documentation/testing),
[Kotest styles](https://kotest.io/docs/framework/testing-styles.html),
[Kotest property testing](https://kotest.io/docs/proptest/property-based-testing.html),
[munit tests](https://scalameta.org/munit/docs/tests.html),
[munit fixtures](https://scalameta.org/munit/docs/fixtures.html),
[Zig tests](https://ziglang.org/documentation/master/#Zig-Test),
[MoonBit tests](https://docs.moonbitlang.com/en/latest/language/tests.html),
[proptest](https://docs.rs/proptest),
[arbitrary](https://docs.rs/arbitrary),
[Hypothesis](https://hypothesis.readthedocs.io/),
[insta](https://insta.rs),
[expect-test](https://docs.rs/expect-test),
[criterion](https://docs.rs/criterion). A dash means this report did not
confirm a built-in form. MoonBit's `moon bench` and Zig's fuzzing are
recent; check their docs before relying on them.

T13's three views match MoonBit's inline, `_wbtest.mbt`, and `_test.mbt`
split, and Rust's `mod tests`, sibling modules, and `tests/`. hd is the only
one of these where the test result, the host capabilities a test needs, and
its isolation all come from rules `main` already has.

## Problems, Ranked

Rank follows how many cases a problem hits and how silent its failure is.
No candidate reopens an alternative the record rejected, except where it
says which new evidence it rests on.

| Rank | Id | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | TS-1 | A trailing block never becomes the `fn!()` body | Critical | 1, 3, 4, 5, 6 |
| 2 | TS-2 | A local or renamed `test` silently registers nothing | High | 20, 21 |
| 3 | TS-3 | `?` has no target type in an inferred body | High | 1, 2, 6, 10 |
| 4 | TS-4 | `tests:` has no grammar and no scope rule | High | 1, 16, 21 |
| 5 | TS-5 | What a failed assertion does is unstated | High | 4, 8, 10, 16 |
| 6 | TS-6 | No test profile; `--grant` is undefined | High | 6, 7, 9 |
| 7 | TS-7 | No per-test options: skip, expected panic, timeout | Medium | 5, 8, 9 |
| 8 | TS-8 | A library cannot shrink a panicking property | Medium | 16 |
| 9 | TS-9 | Visibility between the three kinds is unspecified | Medium | 16, 17 |
| 10 | TS-10 | A test dependency that depends back gives two copies | Medium | 18 |
| 11 | TS-11 | Isolation and parallelism are not restated | Medium | 4, 7, 14 |
| 12 | TS-12 | `Termination.report` cannot print the error | Medium | 2, 8 |
| 13 | TS-13 | Hash values shift with test code | Low | 15 |
| 14 | TS-14 | One failing row hides the rest of a table | Low | 10 |
| 15 | TS-15 | Snapshots, doc tests, benchmarks have no home | Low (waits) | 11, 12, 13 |
| 16 | TS-16 | Record, packages draft, and conformance suite drift | Low | 1, 19 |
| 17 | TS-17 | The `_test.hd` suffix claims production names | Low | 21 |

TS-2 hits fewer cases than TS-3 but ranks above it, because its failure is
silent.

### TS-1. A Trailing Block Never Becomes The `fn!()` Body

**Severity: critical.** No test that makes a bang call compiles, and most
tests of I/O code make one.

**Effect.** `test`'s body parameter is `fn!() -> T $ R` (T5). A trailing
block is a closure inferred from the parameter
([`fn.trailing.closure`](../spec/07-functions.md#trailing-callback-blocks)),
but inference "never silently makes a closure suspending"
([`req.suspend.closure.inference`](../spec/11-requirements-and-suspension.md#suspending-closures)).
The old driver-context rule named the `test` block, which T3 removed.
Cases 1, 3, 4, 5, and 6 all make bang calls in a test body.

**Candidates.**

- **A. A trailing block for a `fn!` parameter is a suspending closure.**
  The callee's signature states it, so nothing is silent. Kotlin does this
  for `suspend` function-type parameters, which is how `runTest { }` works.
  *Q:* extend `fn.trailing.closure` this way for every callee?
- **B. Only `test`'s block is a driver context.** Keep the old rule as a
  special case of the intrinsic. *Q:* is a test-only rule acceptable?
- **C. Require an explicit closure.** `test("x", fn!(): ...)` works today
  (case 2). *Q:* accept the noise in every async test?

**Recommendation:** A. It is one general rule with Kotlin as precedent, and
it also serves library helpers such as a `with_store!` fixture.

### TS-2. A Local Or Renamed `test` Silently Registers Nothing

**Severity: high.** A suite can pass while running nothing.

**Effect.** Registration depends on name resolution (T3). In case 20 a
module-level `fn test` wins inside the block, so `test("...")` is an
ordinary call and `hd test` reports success. `use std.testing.test as spec`
hides tests from any text-based lister. The record's example has no `use`
for `test` at all, so whether it is implicitly in scope is unknown.

**Candidates.**

- **A. Only `test` calls at the top level.** Every top-level statement of a
  `tests:` block or a `*_test.hd` module must be a call that resolves to
  `std.testing.test`; anything else is an error. `test` is imported like
  any `std` name, and it cannot be used as a value. *Q:* adopt this?
- **B. Implicit `test` in test code.** `test` is in scope in `tests:`
  blocks and `*_test.hd` modules, and declaring or importing another
  `test` there is an error. *Q:* accept a scoped implicit name?
- **C. Warn only.** Warn on a top-level call whose callee is not
  `std.testing.test`. *Q:* is a warning enough?

**Recommendation:** A. It turns the silent failure into an error, needs no
implicit name, and also answers "what may appear at the top level" in
TS-4. Renaming stays legal, and tools list tests by resolution, as an LSP
already does.

### TS-3. `?` Has No Target Type In An Inferred Body

**Severity: high.** Most tests that use `?` mix a fixture's error type with
the code's error type.

**Effect.** T4 makes `?` ordinary, and T5 infers `T`. A trailing block
cannot write its result, so each `?` contributes its own `E`, and two
error types have no common result (case 2). A body whose last line returns
`i32` infers `T = i32` and fails the `Termination` bound. The old rule's
`Result[void, Error]` target is gone with T4.

**Candidates.**

- **A. A body with `?` targets `Result[void, Error]`.** Other bodies target
  `void`. Error types that implement `Error` convert by ordinary rules, and
  a `string` error needs `map_err`, since T4 dropped the message wrapping.
  Zig does the same: a `test` body's error set is inferred as `anyerror`.
  *Q:* narrow T5's `T` for trailing blocks this way?
- **B. Let a trailing block state its result.** For example
  `test("x") -> Result[void, Error]:`, usable after any trailing-block call.
  *Q:* is new syntax worth it?
- **C. Keep the rules.** Mixed errors use the explicit closure form of case
  2 or `map_err`. *Q:* accept the extra wrapping?

**Recommendation:** A. It is what Rust users write by hand in most
fallible tests, and it restores only the target, not the wrapping T4
removed. An explicit closure still allows `ExitCode` or a custom `E`.

### TS-4. `tests:` Has No Grammar And No Scope Rule

**Severity: high.** The record's example does not parse, and a derivation
inside the block may be misplaced.

**Effect.** The parser reads `tests:` as a call to `tests` with a trailing
block. `use`, `pub`, and decorators are rejected inside, while bindings, a
`tests:` inside a function, and a second block all parse (case 21). Whether
items inside are module items (so `impl Arbitrary for Order by Structure`
satisfies [`annot.block.module`](../spec/14-annotations.md#derivation-blocks))
or local declarations of a suite is unstated (case 16).

**Candidates.**

- **A. An item-list production.** `tests_block = "tests", ":", NEWLINE,
  INDENT, { use_decl | decorated_decl | declaration | test_call }, DEDENT`
  at module top level only, at most once per file, not in `*_test.hd` or
  `tests/` files. Its items are module items visible only inside the block.
  Shadowing a module name is an error. `tests` is contextual there. *Q:*
  adopt this shape?
- **B. A suite.** The block is an ordinary suite with `use` allowed at its
  top. Declarations are local, as in any function. *Q:* accept local
  semantics, which rule out derivation blocks and impls?

**Recommendation:** A. It is Rust's `#[cfg(test)] mod tests` with the
`use super::*` built in, and it makes derivations and impls work.

### TS-5. What A Failed Assertion Does Is Unstated

**Severity: high.** Tables, cleanup, property testing, and expected panics
all depend on the answer.

**Effect.** [`module.testing.failure`](../spec/10-modules.md#standard-testing)
says a failed assertion "reports test failure" inside a test and panics
elsewhere. It does not say whether the test stops. If it stops as a panic
(A2), no `defer` runs (case 4), a table stops at the first bad row (case
10), and `check` cannot see the failure (case 16). If it continues, a later
line may index past a list the assertion just rejected.

**Candidates.**

- **A. Stop, as a panic.** A failed assertion is an `assertion-failed`
  panic that ends the instance, as in Rust and Zig. *Q:* adopt this?
- **B. Record and continue.** Go's `t.Error` and Swift's `#expect` keep
  going, with a separate stopping form (`t.Fatal`, `#require`). *Q:* is a
  second assertion family worth it?

**Recommendation:** A. It is the smallest rule, it matches the existing
panic category, and instance-per-test makes it safe. TS-6 covers cleanup
of host resources, and TS-14 covers tables.

### TS-6. No Test Profile; `--grant` Is Undefined

**Severity: high.** One network test can break `hd test` for everyone
without a grant.

**Effect.** T6 binds a test's row from "the test profile". A library has
no profile in `hd.toml`, and `--grant` is defined nowhere (case 6). If a
test row outside the profile is an error, as `nonhost-entry-requirement`
is for `main`, then the whole build fails. There is also no way to say
"only where `FsWrite` exists" (case 9).

**Candidates.**

- **A. Tests compile against one profile.** `hd test` uses `console`
  unless `--profile NAME` is given. A row outside it is a compile error,
  as for `main`. *Q:* adopt this?
- **B. As A, but unmet rows skip.** A test whose row names a trait the
  profile lacks is listed and reported as skipped, with the missing
  traits named. `--profile net` runs it. The row is the precondition, so
  this needs no `cfg` and no skip call. *Q:* adopt this?
- **C. Grants in the manifest.** A `[test]` table lists granted traits.
  *Q:* is a manifest table better than a command-line profile?

**Recommendation:** B. It is Rust's `#[ignore]` for network tests, derived
from what the row already states. The runner should print the skip count
so a missing profile in CI is visible.

### TS-7. No Per-Test Options

**Severity: medium.** Skips, expected panics, and timeouts are common in
real suites, and none can be written.

**Effect.** The signature has a name and a body only. Rust puts
`#[ignore]` and `#[should_panic]` on the test, Swift uses traits, and
Kotest and munit use per-test config (cases 5, 8, 9). A panic cannot be
caught in hd source, so only the runner can judge an expected panic.

**Candidates.**

- **A. Named options with defaults on `test`.** A closed set, such as
  `ignore: string? = .None`, `expect_panic: string? = .None` (a panic
  category), and `timeout: Duration? = .None`, placed before `body`. Values
  must be literals, so listing stays static. *Q:* adopt these three?
- **B. Run-time calls only.** `testing.skip("reason") -> never`, as in Go
  and Zig; no expected panics. *Q:* is run-time skipping enough?
- **C. Nothing yet.** *Q:* defer options until the core ships?

**Recommendation:** A. It needs no syntax, it mirrors Rust's attributes,
and `expect_panic` reuses the stable panic categories the conformance
runner already judges.

### TS-8. A Library Cannot Shrink A Panicking Property

**Severity: medium.** Property tests find overflows and index errors most
often, and those are panics.

**Effect.** proptest catches a failing case with `catch_unwind` and
shrinks. In hd, a panic in the property ends the instance, so `check`
cannot shrink (case 16). Generation and shrinking themselves fit T12: a
derived `build` from a choice-sequence `Source` is a generator, and
shrinking the sequence is shrinking.

**Candidates.**

- **A. The runner drives shrinking.** `check` reads its choices from a
  runner-provided source. On failure the runner reruns the test in fresh
  instances with shorter sequences, and keeps the smallest failing one for
  replay. This is Hypothesis's model and Go's fuzz corpus. *Q:* adopt it?
- **B. Properties return results.** A property returns `bool` or `Result`;
  a panic fails the test without shrinking. *Q:* accept unshrunk panics?

**Recommendation:** A. Fresh instances are cheap in Wasm, and it keeps
shrinking out of user code and out of derived methods.

### TS-9. Visibility Between The Three Kinds Is Unspecified

**Severity: medium.** Every suite with more than one test file needs
shared helpers, and property tests need test-only impls.

**Effect.** A `*_test.hd` module sees package `pub` names (T13), but the
record does not say it may use another `*_test.hd` module (case 17).
Whether one `tests/` module may use another is unstated. Whether `tests/`
sees the library with its `tests:` blocks compiled in decides whether a
test-only `impl Arbitrary for Order` is visible there (case 16).

**Candidates.**

- **A. Rust's and Go's rules.** `*_test.hd` modules may use each other's
  `pub` names. Any `tests/` module may use another `tests/` module, and a
  module with no `test(...)` call is just a helper. `tests/` sees the
  library built without its test code, so test-only impls stay in the
  unit-test build. *Q:* adopt these three rules?
- **B. Keep kinds apart.** No `*_test.hd` or `tests/` module may use
  another; shared helpers go in a separate package. *Q:* accept the extra
  package?

**Recommendation:** A. It is what Rust and Go ship, and it needs no new
mechanism.

### TS-10. A Test Dependency That Depends Back Gives Two Copies

**Severity: medium.** Shared fixture packages are common, and the failure
is a baffling type error.

**Effect.** In case 18, `acme/billing_fixtures` depends on `acme/billing`
and is a test dependency of it. The unit-test build of `billing` and the
published `billing` the fixtures use are different builds, so their
`Order` types differ, as in Cargo.

**Candidates.**

- **A. Only `tests/` may use such a dependency.** `tests/` sees the one
  normal build, as Go's external `_test` package does. Naming it from a
  `tests:` block or a `*_test.hd` module is an error. *Q:* adopt this?
- **B. Forbid the cycle.** *Q:* accept that fixtures packages cannot
  depend on the package they serve?
- **C. Allow two copies.** *Q:* accept Cargo's behavior?

**Recommendation:** A. It gives a clear error at the `use` instead of a
type mismatch later.

### TS-11. Isolation And Parallelism Are Not Restated

**Severity: medium.** Output capture, cleanup, and parallel safety all
rest on the runner's host providers.

**Effect.** A1 is still in the spec but not in the record. Case 14 shows
its value. Parallel tests still share real host providers (case 14),
printed output interleaves unless captured (case 7), and a real directory
outlives a failed test (case 4). A once-per-run fixture cannot be written
in hd code, because each test is a fresh instance (case 4).

**Candidates.**

- **A. Instance per test, parallel by default, sandboxed host providers.**
  The runner gives each test its own `Console` buffer (shown on failure)
  and a filesystem rooted at a temporary directory it deletes afterwards,
  as Go's `t.TempDir` does. Shared services such as a database come from
  the profile and are the user's concern, with `--jobs 1`. *Q:* adopt
  this?
- **B. Sequential by default.** As Go within one package. *Q:* give up
  default parallelism?
- **C. A per-test `serial` option.** As Swift's `.serialized`. *Q:* add it
  now?

**Recommendation:** A. Instances make parallelism safe for hd state, and
per-test sandboxes cover capture and cleanup without new syntax.

### TS-12. `Termination.report` Cannot Print The Error

**Severity: medium.** The failure message of every `Result` test depends
on it.

**Effect.** T8 says `report` for `Result` "prints the error (message and
cause chain) and reports `ExitCode(1)`". But `report(self) -> ExitCode` has
no `$ Console` row, and hd has no standard-error trait. Rust's
`Termination::report` prints to stderr, which hd cannot express. A test
comparing codes also needs `ExitCode` to implement `Eq` and `Display`.

**Candidates.**

- **A. The host renders the error.** The host prints an `.Err` as
  [Entry Results](../spec/10-modules.md#entry-results) says, then calls
  `report` for the code. For a test, the runner puts it in the test report.
  *Q:* adopt this split?
- **B. `report` returns more.** For example, a code and an optional
  message. *Q:* change the trait's shape?

**Recommendation:** A. It keeps the trait T8 decided and the rendering
rule the spec already has. The concurrent `ExitCode` change should check
this.

### TS-13. Hash Values Shift With Test Code

**Severity: low.** Only tests that pin explicit hash results are hit, but
they fail after unrelated edits.

**Effect.** The hash seed follows the code identity and profile. A test
build's identity includes test code, so adding a test or changing the
profile can change a pinned hash (case 15).

**Candidates.**

- **A. Document it.** Tests must not pin hash values. *Q:* enough?
- **B. A fixed test seed.** The runner seeds `Hasher` from the library's
  identity without test code. *Q:* add a test-only seeding rule?
- **C. A random seed per run.** Printed, and reusable with `--hash-seed`,
  as Rust's per-process seeds expose such tests at once. *Q:* adopt?

**Recommendation:** A for now. Map iteration is insertion-ordered, so the
exposure is small.

### TS-14. One Failing Row Hides The Rest Of A Table

**Severity: low.** Table tests work; they report less than Go's `t.Run`.

**Effect.** Under A2 the first failing row ends the test, and one row
cannot be filtered (case 10).

**Candidates.**

- **A. Keep the loop.** `reason` names the row. This is what Rust's
  standard harness offers. *Q:* enough for now?
- **B. A library helper.** For example `testing.rows(rows, fn(row: Row): ...)`
  running each row's check as a `Result` and reporting every failing row.
  *Q:* add it with the standard library?

**Recommendation:** A now; B is library work that needs no language
change.

### TS-15. Snapshots, Doc Tests, And Benchmarks Have No Home

**Severity: low, and waiting.** These are advanced features. The shared
rules say to settle the core first, so this report asks nothing yet.

**Direction found.** Inline snapshots need a call-site location intrinsic
and a runner that rewrites source (MoonBit, expect-test). Golden files
need a read-only package-directory provider, writable under `--update`.
Doc tests fit Rust's model: fenced `hd` blocks in `##` comments of `pub`
items, run with the `tests/` view. Benchmarks need a host clock and their
own registration, like Go's `b.Loop` or a `benches/` root.

### TS-16. Record, Packages Draft, And Conformance Suite Drift

**Severity: low.** Mechanical, but a spec pass must not miss it.

**Effect.**

| Place | Drift |
| --- | --- |
| [TESTING.md example](TESTING.md#owner-decisions) | `use app.billing` has no valid root; `assert_equal` lacks `reason=`; the `std.process` block has bodiless functions, a syntax error at line 12 |
| TESTING.md T3 | "hd has no other top-level statements" is not so: scripts and top-level bindings exist ([Module Initialization](../spec/10-modules.md#module-initialization)) |
| [future-work/README.md](README.md) | lists T1-T12, not T1-T13 |
| [PACKAGES.md](PACKAGES.md) `[source] tests` row and decision 5 | still say a test-only dependency is named by a `use` inside a `test` block |
| [Conformance](../spec/conformance/README.md) | 362 fixtures use `test "..."`; the runner contract names test blocks; the file-name rule conflicts with `*_test.hd` |
| [RUNTIME_AND_LIBRARY.md](RUNTIME_AND_LIBRARY.md#testing) | still describes `test "name":` blocks |

**Candidate.** Fix all of these in the spec pass that applies T2-T13. *Q:*
confirm?

**Recommendation:** yes.

### TS-17. The `_test.hd` Suffix Claims Production Names

**Severity: low.** The error is loud, but the cause is not obvious.

**Effect.** `src/ab_test.hd` for an A/B test feature becomes test-only, and
a normal module that uses it fails to build (case 21).

**Candidates.**

- **A. Accept, as Go does,** with a diagnostic that names the suffix rule.
  *Q:* enough?
- **B. A different suffix,** such as `.test.hd`. *Q:* worth the change to
  T13?

**Recommendation:** A.

## Questions For The Owner

Most blocking first. Each has the labeled recommendation from its problem.

1. **TS-1.** Does a trailing block passed for an `fn!` parameter become a
   suspending closure, for every callee? Recommendation: yes (A).
2. **TS-3.** Does a test body with `?` target `Result[void, Error]`, and
   `void` otherwise? Recommendation: yes (A).
3. **TS-2.** Must every top-level statement of a `tests:` block or
   `*_test.hd` module be a call resolving to `std.testing.test`, imported
   like any `std` name? Recommendation: yes (A).
4. **TS-4.** Is `tests:` an item list whose items are module items visible
   only inside it, once per file, top level only? Recommendation: yes (A).
5. **TS-5.** Does a failed assertion end the test as an `assertion-failed`
   panic? Recommendation: yes (A).
6. **TS-6.** Do tests compile against one profile (`console` unless
   `--profile`), with unmet rows reported as skipped? Recommendation: yes
   (B).
7. **TS-11.** One instance per test, parallel by default, with a per-test
   `Console` buffer and temporary filesystem? Recommendation: yes (A).
8. **TS-7.** Add `ignore`, `expect_panic`, and `timeout` as literal named
   options on `test`? Recommendation: yes (A).
9. **TS-9.** Adopt Rust's and Go's visibility rules between `*_test.hd`
   modules, `tests/` modules, and the library? Recommendation: yes (A).
10. **TS-10.** May only `tests/` use a test dependency that depends on this
    package? Recommendation: yes (A).
11. **TS-12.** Does the host render an `.Err` and `report` only choose the
    code? Recommendation: yes (A).
12. **TS-8.** Does the runner drive property shrinking by rerunning fresh
    instances? Recommendation: yes (A).
13. **TS-14, TS-13, TS-17.** Keep loops for tables, document hash seeds,
    and accept the `_test.hd` suffix as is? Recommendation: yes to all
    three.
14. **TS-16.** Fix the listed drift in the spec pass? Recommendation: yes.

TS-15 (snapshots, doc tests, benchmarks) waits until these are settled.

## Parse Log

Every `text` block above was checked with the reference parser
(`parseSource` in `spec/reference-parser/parser.ts`) on 2026-09-27.
Parsing checks syntax only; nothing was resolved or type-checked.

| Block | Where | Result |
| --- | --- | --- |
| 1 | Case 1: the record's example, corrected | `syntax-error` at line 8, the `use` inside `tests:`, as expected. Marked `# hypothetical syntax`. |
| 2 | Case 1: the same without the inner `use` | Parse. |
| 3-4 | Case 2: trailing-block form, explicit-closure form | Parse. |
| 5-6 | Case 3: async test, `block_on` helper | Parse. |
| 7 | Case 4: `with_store` fixture and `hermetic` | Parse. |
| 8 | Case 5: `timeout!` under `ManualClock` | Parse. |
| 9 | Case 6: live network test | Parse. |
| 10 | Case 7: `BufferConsole` capture | Parse. |
| 11 | Case 8: `assert_err`, proposed `expect_panic=` | Parse (a named argument is ordinary syntax). |
| 12 | Case 9: proposed `ignore=` | Parse. |
| 13 | Case 10: table test | Parse. |
| 14 | Case 11: proposed `snapshot` with a multiline string | Parse. |
| 15 | Case 12: doc comment with an `hd` fence | Parse; the comment attaches to `add`. |
| 16 | Case 14: module state across two tests | Parse. |
| 17 | Case 15: pinned hash | Parse. |
| 18 | Case 16: `Order` and a derivation block inside `tests:` | Parse. |
| 19-21 | Case 17: `fixtures_test`, `billing_test`, `tests/checkout` | Parse. |
| 22 | Case 18: test dependency inside `tests:` | `syntax-error` at line 2, the inner `use`, as expected. Marked `# hypothetical syntax`. |
| 23 | Case 20: a module-level `fn test` | Parse. |
| 24 | Case 21: nested, statement-bearing, and repeated blocks | Parse. |

Reference-parser findings:

- `tests:` parses as a trailing-block call to a function named `tests`.
  Inside it, `use` (blocks 1 and 22), `pub`, and decorators are a
  `syntax-error`. Bindings, nested blocks, and a second block parse (block
  24). The `pub` and decorator results come from separate probes.
- A bang call with no arguments needs `()` before a trailing block:
  `with_db!():` parses and `with_db!:` is a `syntax-error` (separate probe).
- The record's own blocks: the `std.process` block fails at line 12 (a
  function without a body), and the example fails at line 1
  (`use app.billing`).
