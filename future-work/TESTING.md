# Testing Redesign

Status: design record, started 2026-09-27. Owner decisions T1-T53 are
decided. T2-T31 and T36's statement rule are applied to the specification
on 2026-09-27 (T4, T5, and T8 earlier that day), and the language parts of
T33, T39-T50, T52, and T53, and the `timeout` note after T52, on 2026-09-28. T29, T30, T32, T34, T37, T38, T51, and the
runner parts of T20, T21, and T42 are tooling and library text, recorded in
[Runtime And Library](RUNTIME_AND_LIBRARY.md#testing) and
[Standard Library](STDLIB.md#testing-layer). The questions this pass raised
are under [Still Open](#still-open).

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
    runner. The note after T52 changes `timeout` to a suffixed literal,
    `timeout=5s`, and Literal Suffixes L16 lets it take any `Duration`
    value.
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
    `T < Eq & Debug`, so a failure shows both values. `Display` stays
    user-facing text. (The name avoids `Inspect`, since `Inspectable` is
    the runtime type-information trait.) Applied 2026-09-28, with T39 and
    T48, in [Debug Trait](../spec/09-traits.md#debug-trait) and
    [`module.testing.assert-equal-debug`](../spec/10-modules.md#r-module.testing.assert-equal-debug).
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
    (combinator values with value trees, a large API). Reconfirmed
    2026-09-28: choices first (Hypothesis), with derived `Arbitrary` giving
    sensible defaults; proptest-style strategy combinators are not the
    API.
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
    [`fn.default.order`](../spec/07-functions.md#default-values) for
    the last parameter when its type is a function type, since a trailing
    block or a named argument always supplies it. So `it` is an ordinary
    function, not an intrinsic; only its registration (T7, T16) is special.
    The rule is general: `retry(3, backoff=...):` works too. Returning a
    function from `it(name, options)` was considered and rejected: it
    needs a new trailing-block rule, and a bare call would register
    nothing. Applied 2026-09-28 in
    [`fn.default.order-final-function`](../spec/07-functions.md#r-fn.default.order-final-function)
    and [Test Cases](../spec/10-modules.md#test-cases).
41. **T41: `it_each`, `it_prop`, and `it_prop_with` follow `it`'s rules:**
    a string-literal name, the same options, top level of a `tests:` block
    or test module only. `it_each` rows are evaluated when that test runs,
    its body follows T15, and a `name[i]` may not equal another test's
    name. Applied 2026-09-28 in
    [Table Tests](../spec/10-modules.md#table-tests), with `it`'s options
    before `body` (see [Still Open](#still-open)).
42. **T42: `hd check` checks test code only with `--tests`,** like
    `cargo check`; `hd test` always compiles it. Applied 2026-09-28 in the
    [conformance command contract](../spec/conformance/README.md#command-contract):
    the runner passes `check --tests`.

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
    `use tests.common`), like Rust's `tests/common/`. T43-T46 are applied
    2026-09-28 in [Test Cases](../spec/10-modules.md#test-cases),
    [Tests Blocks](../spec/03-names-and-scopes.md#tests-blocks), and
    [Test Modules](../spec/10-modules.md#test-modules), with the new codes
    `unknown-panic-category`, `public-test-item`, and
    `misplaced-tests-block`, and the use root `tests`.
47. **T47: `it`, `it_each`, `it_prop`, and `it_prop_with` are only called
    directly in test position;** any other use, including as a value, is
    `misplaced-test-case`, so tests stay statically listable. Applied
    2026-09-28 in
    [`module.testing.direct-call`](../spec/10-modules.md#r-module.testing.direct-call).
48. **T48: `Debug` is a structured writer.**
    `trait Debug: fn debug(self, out: mut DebugWriter) -> void`, with
    builder calls like Rust's `debug_struct` and `field`; the derived impl
    is a walker, and `debug(x) -> string` prints stable, multi-line,
    consistently indented output.
49. **T49: the snapshot API lives in `std.testing`, imported.**
    `snapshot(text: string, expect: string = "")` and
    `snapshot_file(text: string)`. `expect=` must be a string literal so
    `hd test --update` can rewrite it; an empty or missing `expect` is
    recorded on the first `--update`. Applied 2026-09-28 in
    [Snapshots](../spec/10-modules.md#snapshots).
50. **T50 (2026-09-28): a property body follows T15.** `it_prop` and
    `it_prop_with` take `prop: fn!(T) -> R` with `R < Termination`: `void`,
    or `Result[void, Error]` when the body uses `?`. An `.Err` counts as a
    failing case and is shrunk like an assertion failure. Applied
    2026-09-28 in
    [`expr.try.test.row-body`](../spec/05-expressions.md#r-expr.try.test.row-body).
51. **T51 (2026-09-28): shrinking is capped by runs.** Each shrink attempt
    is a fresh instance, so shrinking stops after 500 attempts by default
    (Hypothesis's figure) and reports the smallest failing input so far,
    marked "shrinking stopped early". `shrink=` on `it_prop` and
    `hd test --shrink N` override it.
52. **T52 (2026-09-28): answers to the T33/T39-T51 apply questions.**
    Parameterized bodies are passed by name (`body=` for `it_each`,
    `prop=` for `it_prop`); a trailing block with parameters was declined.
    Option order is required arguments, then defaulted options, then the
    final function (for `it_prop`: options, `cases`, `shrink`, `prop`).
    `DebugWriter` stays imported, not a prelude name. `use tests.x` outside
    `tests/` is `test-only-use`. `hd test --list` shows one `name[..]`
    entry per `it_each` call. The codes `unknown-panic-category`,
    `public-test-item`, and `misplaced-tests-block` are kept. Applied
    2026-09-28 in
    [`module.test.tests-root-elsewhere`](../spec/10-modules.md#r-module.test.tests-root-elsewhere)
    and [`trait.debug.writer-import`](../spec/09-traits.md#r-trait.debug.writer-import);
    the `--list` rows are runner text in
    [Runtime And Library](RUNTIME_AND_LIBRARY.md#test-runner). Named
    bodies and the option order were already in the specification.
53. **T53 (2026-09-28): answers to Still Open 1, 3 and 4.**
    - Test-module fixtures use a `# fixture-test-layout:` header, which
      works like the package-role fixture headers.
    - `snapshot_file` keeps its file under the recorded `__snapshots__`
      layout, `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`.
      A missing file fails the test, except on `--update`.
    - The full STDLIB draft of `Choices` and `Arbitrary` is accepted:
      `int`, `float`, `bool`, `pick`, `list`, `string`, `assume` and `draw`
      on `Choices`, and `fn arbitrary(c: mut Choices) -> Self`.
    - Still Open 2 (the `DebugWriter` API) was decided the same day, after
      the owner compared plain write methods with builders. Builders were
      chosen, like Rust's `Formatter`: `debug_struct(name).field(n, v)
      .finish()`, `debug_tuple`, `debug_list`, `debug_map`, and
      `write(text)` for custom text. `@derive(Debug)` generates builder
      calls. The writer decides compact or pretty layout. Plain writes
      were declined because they fix the layout in each impl, which leaves
      no pretty mode or depth limit and lets derived and hand-written
      output drift apart.

    Applied 2026-09-28 in
    [Debug Builders](../spec/09-traits.md#debug-builders),
    [Property Tests](../spec/10-modules.md#property-tests),
    [`module.testing.snapshot-file.path`](../spec/10-modules.md#r-module.testing.snapshot-file.path)
    and [Test Layouts](../spec/conformance/README.md#test-layouts). The
    prototype follows: `DebugWriter`, its builders, `debug`, and the
    standard `Debug` implementations are hd code in `lib/std/format.hd`;
    `Choices`, `Arbitrary`, and `snapshot_file` are hd code in
    `lib/std/testing.hd`; and `hd test --update` records snapshot files.
    The readings this raised are under
    [Still Open After T53](#still-open-after-t53).

The `timeout=` value follows the literal-suffix decisions
([Literal Suffixes](LITERAL_SUFFIXES.md#owner-decisions) L1-L9, 2026-09-28):
`timeout: Duration? = .None`, written `timeout=5s` with `use std.time.s`.
Literal Suffixes L16 then relaxed T22 for `timeout` alone: it takes any
`Duration` value, such as `budget()`, evaluated when the test case runs.
Applied 2026-09-28 in
[`module.testing.option.timeout-any-duration`](../spec/10-modules.md#r-module.testing.option.timeout-any-duration)
and [`module.testing.option.timeout-at-run`](../spec/10-modules.md#r-module.testing.option.timeout-at-run),
replacing the `"5s"` string and the compile-time reading of L7.

```text
# std.testing (T40)
pub fn it[T < Termination, R](name: string, ignore: string? = .None,
                             expect_panic: string? = .None, timeout: Duration? = .None,
                             body: fn!() -> T $ R) -> void $ R
```

```text
# std.testing (T50, T51; T41 adds it's options, placed as applied)
pub fn it_prop[T < Arbitrary, R < Termination](name: string, ignore: string? = .None,
                                               expect_panic: string? = .None, timeout: Duration? = .None,
                                               cases: i32 = 100, shrink: i32 = 500, prop: fn!(T) -> R) -> void
pub fn it_prop_with[T, R < Termination](name: string, gen: fn(mut Choices) -> T, ignore: string? = .None,
                                        expect_panic: string? = .None, timeout: Duration? = .None,
                                        cases: i32 = 100, shrink: i32 = 500, prop: fn!(T) -> R) -> void
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

# std.testing: this 2026-09-27 shape had no options; T40 gives the full one above
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

These questions came up while applying the decisions. Each waits for the
owner; the specification states none of them as a rule. The 2026-09-27
questions 1, 3-9, 11, and 12 were answered by T39-T49 and applied on
2026-09-28, and question 2 (`timeout`) by the literal-suffix decisions. The
2026-09-28 questions 2-6 (named bodies, option order, `DebugWriter`, the
`tests` root outside `tests/`, and listing `it_each` rows) were answered by
T52 and applied the same day. The prototype-pass questions 1-4 below were
answered by T53 and applied the same day; they are kept as history.

1. **Fixtures for test modules.** The conformance suite has no fixture
   environment for test modules, integration tests, or test dependencies.
   So `cyclic-test-dependency`, `misplaced-tests-block` (T45), and the
   `tests` use root inside `tests/` (T46) have no fixtures. `test-only-use`
   has one, for `use tests.common` in an ordinary module, since every
   fixture is one. The prototype pass on 2026-09-28 left this unbuilt,
   because the spec owns the fixture format. The prototype now runs test
   modules (`hd test` on a `*_test.hd` file, and the playground's package
   linker), so only the fixture format waits. **Recommendation:** a header
   like `# fixture-package-role`, such as `# fixture-test-layout: test-module`
   or `integration`, that places the fixture as `src/<name>_test.hd` or
   `tests/<name>.hd` in a synthetic package, with `--test-layout` passed to
   `check` and `test`. It reuses the Package Roles mechanism.
2. **The `DebugWriter` builder calls (prototype pass, 2026-09-28).** T33
   gives `debug(self, out: mut DebugWriter)`, and the spec leaves the
   builder calls and the `debug` layout to the standard library. Without
   them a hand-written `Debug` cannot write anything, and the prototype
   renders no `debug` text. Options: Rust's `Formatter` builders
   (`debug_struct`, `debug_tuple`, `debug_list`, `debug_map`), or one
   `field(name, value)` call plus `item(value)`. **Recommendation:** the
   Rust builders, a proven model that the derived `Debug` maps onto one
   builder per declaration kind.

```text
use std.format.DebugWriter

data Point:
    x: i32

impl Debug for Point:
    fn debug(self, out: mut DebugWriter) -> void:
        out.field("x", self.x)  # a hypothetical builder call
```

3. **Where `snapshot_file` keeps its file (prototype pass, 2026-09-28).**
   T49 says the runner names the file from the running test case, but not
   its directory, its name, or what a missing file means outside an update
   run. Options: a `snapshots/` folder beside the module, one file per test
   named `<module>__<test name>.snap`, as Rust's `insta` does; or one file
   per module holding every case. **Recommendation:** the `insta` layout,
   with a missing file failing the test unless the run updates snapshots.

```text
use std.testing.snapshot_file

tests:
    it("renders the report"):
        snapshot_file("total: 3")  # which file, and what if it is missing?
```

4. **The `Choices` and `Arbitrary` API (prototype pass, 2026-09-28).** T35
   and T36 decide the model, but `Choices`' members, `Arbitrary`, and the
   `it_prop` signatures exist only in the
   [non-normative STDLIB draft](STDLIB.md#proposal-choices-first-arbitrary-for-defaults),
   and the spec gives no signature. So the prototype does not implement
   `it_prop`, `it_prop_with`, or a shrinker yet. **Recommendation:** accept
   the draft's core as the first API: `Choices.int`, `bool`, `pick`,
   `list`, and `assume`, `Arbitrary` for the primitives, and the draft's
   `it_prop` signatures. Hypothesis ships the same core
   (`integers`, `booleans`, `sampled_from`, `lists`, `assume`).

```text
use std.testing.{Choices, it_prop_with}

fn small(c: mut Choices) -> i64: c.int(0, 9)

tests:
    it_prop_with("is small", gen=small, prop=fn!(n: i64): assert(n < 10))
```

The earlier [Testing Stress Test](TESTING_STRESS_TEST.md) ranks 17 problems
found on 21 cases against T1-T13; T14-T27 answer its questions.

### Still Open After T53

**Decided (owner, 2026-09-28, T54).**
- A snapshot mismatch or a missing snapshot file fails with
  `assertion-failed`, as `assert_equal` does.
- The `DebugWriter` builder types keep Rust's names: `DebugStruct`,
  `DebugTuple`, `DebugList` and `DebugMap`.
- `@derive(Debug)` follows Rust's mapping: a data type or record variant
  uses a struct builder, a tuple variant uses a tuple builder, and a unit
  variant prints its name only. The spec states this mapping.
- Test-layout fixture packages for `cyclic-test-dependency` and a `tests`
  root inside `tests/` get added when those rules need coverage.

**T54 applied (2026-09-28)** in
[`module.testing.snapshot.mismatch`](../spec/10-modules.md#r-module.testing.snapshot.mismatch),
[`module.testing.snapshot-file.missing-panic`](../spec/10-modules.md#r-module.testing.snapshot-file.missing-panic),
and
[`trait.debug.derive-builders.mapping`](../spec/09-traits.md#r-trait.debug.derive-builders.mapping)
with its table. The builder names were already
[`trait.debug.builder.types`](../spec/09-traits.md#r-trait.debug.builder.types).
`snapshot_file` in `lib/std/testing.hd` now fails through `assert`, and a
derived `Debug` for a fieldless data type uses `debug_struct`. Applying
T54 raised one question; nothing here is decided:

| Question | Effect | Resolution |
| --- | --- | --- |
| A variant with both positional and named payload fields | Rust has no such variant, so its mapping names no builder for `Mixed(i32, label: string)`. | Decided 2026-09-28 ([Open Issues](OPEN_ISSUES.md#casts-property-discards-type-names-as-values-std-scope) item 8) and applied as [`trait.debug.derive-builders.mixed`](../spec/09-traits.md#r-trait.debug.derive-builders.mixed): `debug_struct`, naming a positional field `_0`, printing `Mixed { _0: 6, label: "m" }`. |

```text
@derive(Debug)
enum Shape:
    Mixed(i32, label: string)   # Mixed { _0: 6, label: "m" }
```

Applying T53 on 2026-09-28 needed these readings. The spec states each as
applied, so each can change without breaking a decision. T54 decided the
builder names, the derived builders, how a snapshot fails, and when
test-layout fixture packages get added; nothing else here is decided:

| Question | Applied | **Recommendation** |
| --- | --- | --- |
| `Choices` beyond T53 | The draft's `@derive(Arbitrary)`, its member-line facts, and size scheduling are not specified. The `__regressions__` format and the `assume` discard limit were decided on 2026-09-28 (below) | Decide the rest with the property-test runner. |

The prototype runs `it_prop` and `it_prop_with` (T35-T38, T50, T51): every
`Choices` draw is recorded, a failing case is shrunk by replaying shorter
or smaller choice streams, `cases` and `shrink` cap the run, and
`hd test --seed N`, `--cases N`, and `--shrink N` override them
(`src/property-tests.ts`). `std` implements `Arbitrary` only for the
primitives and `string`. The owner decided the three questions its
stand-ins waited on
([Open Issues](OPEN_ISSUES.md#casts-property-discards-type-names-as-values-std-scope)
items 2, 6 and 7), and the prototype follows each:

| Question | Resolution (2026-09-28) |
| --- | --- |
| How many discarded cases a property may have | Discards do not count toward `cases`, and more than 10 × `cases` discards fail the property: [`module.testing.prop.discard`](../spec/10-modules.md#r-module.testing.prop.discard) and [`discard-limit`](../spec/10-modules.md#r-module.testing.prop.discard-limit). |
| How a failure shows the shrunk value | `it_prop` and `it_prop_with` require `T < Debug`, and the report prints the shrunk input: [`module.testing.prop.debug`](../spec/10-modules.md#r-module.testing.prop.debug) and [`report`](../spec/10-modules.md#r-module.testing.prop.report). |
| Where a failing case is saved (T37) | The shrunk choice stream, one decimal number per line, in `__regressions__/<module>/<test-slug>`, replayed first: [`module.testing.prop.regression-file`](../spec/10-modules.md#r-module.testing.prop.regression-file). |

The prototype's
`DebugWriter` is always compact, and `Map` and tuples of more than two
elements render no `debug` text.
