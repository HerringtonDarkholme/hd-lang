# Proposal: complete fixture format for `spec/conformance/README.md`

**Status:** proposal from audit step 1.6 (2026-09-25, commit `bd985d7`). The
owner decides whether to apply it. The audit has not edited `spec/`.

**Why:** the plan (Phase 7, "Fixture format, decided 2026-09-25") requires the
conformance suite to be fully specified by `spec/` alone. Today `# expect-result:`,
`# expect:`, `# test:`, the phase-to-command mapping, the command options, exit
statuses, and the located-diagnostic format live only in `test/README.md`,
`test/run-portable.ts`, or `src/cli.ts`
(`audit/evidence/01-test-quality/directives.tsv`, finding F-200).

**How to read this file:**

- The text between the two `---8<---` lines is the proposed replacement for
  `spec/conformance/README.md`. It keeps every rule of the current README.
- The text writes down what the repository runner does today wherever that
  behavior is language-neutral.
- Where today's behavior is implementation-specific or undefined, the text
  holds an `[OPEN-n]` marker instead of a decision. Each marker is listed at
  the end as an `OPEN_ISSUES.md` question, with the evidence behind it.
- Nothing here adds language features. The only new source-level vocabulary is
  in fixture comments, which have no meaning to the language.

---8<---

# Conformance Fixtures

These fixtures turn normative specification examples into parser, type-checker,
and runtime tests. They do not define semantics independently of the
specification. This file is the complete definition of the fixture format and
of the command contract a conformance runner uses. An implementation needs
nothing outside `spec/` to run the suite.

## Terms

- **Fixture:** one UTF-8 `.hd` file under `spec/conformance/`, the single
  primary input of one case.
- **Case:** one row of `cases.tsv`. It names a fixture, a phase, and an
  expectation.
- **Implementation under test:** a command prefix, for example `hd` or
  `node --experimental-strip-types bin/hd.js`. The runner appends an action,
  options, and the fixture path. See [Command Contract](#command-contract).
- **Runner:** the tool that reads `cases.tsv`, invokes the implementation, and
  judges each case by the rules below.

## Layout

- `parse/valid`: must parse under the core grammar.
- `parse/invalid`: must be rejected during lexing or parsing.
- `typing/valid`: must parse and type-check.
- `typing/invalid`: must parse, then fail with the diagnostic category in an
  inline `# diagnostic: CODE` comment on the rejected source line.
- `typing/warnings`: must parse and type-check, then emit the category in an
  inline `# warning: CODE` comment on the warned source line.
- `runtime/valid`: must parse and type-check, then complete successfully when
  executed by its declared fixture environment.
- `runtime/panic`: must parse and type-check, then panic with the category in
  an inline `# panic: CODE` comment on the triggering source line when executed.

The directory is informative. The case's phase and expectation come from
`cases.tsv`, and the two must agree.

## Self-Containment And Determinism

A fixture may rely only on:

- primitive types and prelude names ([Modules](../10-modules.md#prelude));
- standard modules that a numbered chapter specifies, such as `std.testing`,
  `std.task`, and `std.resource`;
- its own declarations;
- the environment named by its fixture directives (see
  [Fixture Environments](#fixture-environments)).

Every other user-defined type, trait, and function, including every
requirement key, must be declared in the fixture. A fixture must not depend
on its file name, its directory, the working directory, other files, the
clock, randomness, or timing. Its expectation must not depend on diagnostic
message text, on map iteration order beyond insertion order, or on any value
rendering that the specification does not define.

## Comment Directives

A directive is a line comment that the runner reads. It has no meaning to the
language. There are two forms.

- **Header directives** occupy a whole line that starts with `# `, followed by
  the directive name, `: `, and a value. They may appear on any line, but by
  convention they come first. Each header directive appears at most once per
  fixture, except `# expect-result:`.
- **Line markers** end a source line. They have the form `# KIND: CODE`, where
  `KIND` is `diagnostic`, `warning`, or `panic` and `CODE` matches
  `[a-z0-9-]+`, followed by optional trailing whitespace and the line end. The
  marker names the source line it ends.

A reject, warn, or panic fixture has exactly one line marker. An accept
fixture has none. `spec/check.sh` enforces both rules.

| Directive | Form | Meaning |
| --- | --- | --- |
| `# diagnostic: CODE` | line marker | The case's error category, reported on this line. `CODE` is in the Error rows of [the diagnostic table](../README.md#diagnostics). |
| `# warning: CODE` | line marker | The case's warning category, reported on this line. `CODE` is in the Warning row. |
| `# panic: CODE` | line marker | The case's panic category, raised by this line. `CODE` is a stable category from [Runtime Panics](../06-control-flow.md#runtime-panics). |
| `# test: NAME` | header | Optional human-readable name. Cases are identified by path, never by name. |
| `# expect: parse` | header | Optional restatement of a `parse` accept case. |
| `# expect: accept` | header | Optional restatement of a `type` accept case. |
| `# expect: test` | header | Optional restatement of a `runtime` accept case. |
| `# expect-result: ENTRY = VALUE` | header, repeatable | A runtime accept case also requires `ENTRY` to return `VALUE`. See [Result Expectations](#result-expectations). |
| `# fixture-runtime-profile: NAME` | header | Selects a named host profile. See [Runtime Profiles](#runtime-profiles). |
| `# fixture-runtime-scenario: NAME` | header | Replaces ordinary execution with a named driving procedure. See [Runtime Scenarios](#runtime-scenarios). |
| `# fixture-runtime-pending-function: NAME` | header | Holds the named suspending function pending. Valid only with the `cancellation-cleanup` scenario. |
| `# fixture-package-role: ROLE` | header | Selects the synthetic multi-package environment. See [Package Roles](#package-roles). |

If an `# expect:` directive is present, it must agree with the case's phase
and expectation in `cases.tsv`. A fixture never mixes line markers with
`# expect-result:`.

## Case Index

`cases.tsv` is the authoritative fixture index. Its header is
`path	phase	expectation	specification`, and each row has four non-empty
tab-separated fields:

- `path`: the fixture path relative to this directory.
- `phase`: `parse`, `type`, or `runtime`.
- `expectation`: `accept`, `reject:CODE`, `warn:CODE`, or `panic:CODE`. For a
  marked fixture it equals the marker kind (`diagnostic` is written `reject`)
  and code.
- `specification`: the one primary section, as `NN-chapter.md#anchor`.

Every fixture has exactly one row.

`examples.tsv` inventories every `text` code fence in the numbered core
chapters. Runnable examples map to one or more self-contained fixtures.
Lexical inventories, type fragments, filesystem layouts, and similar
non-program text are classified explicitly rather than being mistaken for
compilable files. `spec/check.sh` verifies that every core `text` fence has
exactly one inventory entry and that every runnable entry names an indexed
fixture.

## Judging A Case

The runner performs these steps. `check` and `test` receive the options that
the fixture's directives select (see [Command Contract](#command-contract)).

| Phase | Expectation | Steps | Passes when |
| --- | --- | --- | --- |
| `parse` | `accept` | `parse FILE` | exit 0 |
| `parse` | `reject:CODE` | `parse FILE` | exit 1, and a located `CODE` on the marker line |
| `type` | `accept` | `check FILE` | exit 0 (warnings allowed) |
| `type` | `reject:CODE` | `check FILE` | exit 1, and a located error `CODE` on the marker line |
| `type` | `warn:CODE` | `check FILE` | exit 0, and a located warning `CODE` on the marker line |
| `runtime` | `accept` | `check FILE`, then `test FILE` | both exit 0, and every `# expect-result:` holds |
| `runtime` | `panic:CODE` | `check FILE`, then `test FILE` | `check` exits 0; `test` exits 1 and reports panic `CODE` [OPEN-4] |

A runtime case whose `check` fails, fails. The runner judges only codes,
lines, and exit statuses. Diagnostic wording is not normative.

A located diagnostic counts for the marker line when its `LINE` equals the
marker's line number. Its column is not judged. [OPEN-5] Other diagnostics in
the same output do not fail the case. [OPEN-6]

## Runtime Execution

`test FILE` executes one fresh program instance:

1. Module initialization runs as defined in
   [Module Initialization](../10-modules.md#module-initialization).
2. If the module declares `main` or `main!`, it runs as the entry point, with
   its requirement row supplied by the selected runtime profile. Its return
   value is not judged.
3. Each `test "..."` block runs in source order in the same instance.
   [OPEN-7]
4. The command succeeds when nothing panicked and no `std.testing` assertion
   failed.

A fixture with a `# fixture-runtime-scenario:` directive replaces steps 2 and
3 with that scenario's procedure.

## Result Expectations

`# expect-result: ENTRY = VALUE` adds one check to a runtime accept case. The
text before the first ` = ` is `ENTRY`. The rest of the line, with surrounding
whitespace removed, is `VALUE`.

- `ENTRY` names a top-level function in the fixture. It has no parameters, its
  requirement row is empty or is supplied by the selected profile, and it
  returns a type that has a `Display` implementation. [OPEN-2]
- The runner invokes `run --entry ENTRY FILE` once per directive, each time in
  a fresh program instance, after module initialization.
- The case passes when every invocation exits 0 and its result line equals
  `VALUE`. The result is rendered as [OPEN-1], on the line format given in
  [Command Contract](#command-contract). [OPEN-3]

## Fixture Environments

### Runtime Profiles

A runtime profile is a named set of host capability traits and the providers
that implement them ([Wasm Boundary](../10-modules.md#wasm-boundary)). With
`# fixture-runtime-profile: NAME`, the runner passes the profile to both
`check` and `test` (and to `run`). The profile's traits become host
capabilities that may appear in entry-point rows. A fixture declares every
trait a profile implements, with the exact method signatures below. The
profile supplies one provider value per trait.

- `disposed-file` implements the fixture's `Files` and `FileHandle` traits:
  - `Files.open!(self) -> mut FileHandle` completes on its first poll with a
    fresh open handle;
  - `FileHandle.close(mut self) -> Result[void, ResourceError[E]]` returns
    `Ok` on the first call;
  - after a successful close, every `FileHandle` operation returns
    `Err(ResourceError.Disposed)` and must not trap. `E` is the fixture's error
    type. [OPEN-8]
- `pending-gate` implements the fixture's
  `trait Gate: fn wait!(self) -> void`. Every poll of `wait!` stays pending.

Implementations may define more profiles. A conformance fixture may name only
the profiles listed here. [OPEN-9]

### Runtime Scenarios

A scenario is a fixed host procedure for protocol states that
single-threaded source cannot create by itself. Each scenario starts from a
fresh instance, with the entry `main!` and its row supplied by the selected
profile.

- `cancellation-cleanup`:
  1. Construct the `main!` suspension.
  2. Poll it once. The poll must return pending. It stays pending at the first
     `pending-gate` `wait!`, or at the function named by
     `# fixture-runtime-pending-function:`.
  3. Cancel the suspension.
  4. Call the fixture's top-level `cleanup_ran() -> bool` in the same
     instance. The case passes when it returns `true`.
- `competing-drivers`:
  1. Construct the `main!` suspension.
  2. Poll it once, holding every suspending callee pending, so that the poll
     returns pending.
  3. While that suspension is still owned by the first driver, start a second
     drive of the same entry. [OPEN-10]
  4. The case passes when step 3 raises `suspension-competing-driver`.
- `reentrant-poll`:
  1. Construct the `main!` suspension and poll it.
  2. During that poll, at its first suspension point, poll the same
     suspension again before the first poll returns.
  3. The case passes when step 2 raises `suspension-reentrant-poll`.

`# fixture-runtime-pending-function: NAME` makes the deterministic runtime
hold the named suspending function pending at every poll. The runtime makes
no host call for it. The directive is valid only with `cancellation-cleanup`.

### Package Roles

`# fixture-package-role: library` and `# fixture-package-role: root-application`
select the synthetic multi-package environment. The primary file is the only
case input. It is compiled as the root module of a package in the named role.
The runner also supplies the packages `dep.validation` and `dep.models`, whose
sources are part of this suite. [OPEN-11]

## Command Contract

The runner invokes the implementation as:

```text
IMPL ACTION [OPTION VALUE]... FILE
```

- `FILE` is an absolute path to the fixture and is always the last argument.
- The working directory is not part of the contract. [OPEN-12]
- stdin is closed. stdout and stderr are both read.

| Action | Options the runner may pass | Used for |
| --- | --- | --- |
| `parse` | none | `parse` phase |
| `check` | `--profile NAME` | `type` phase, and the first step of `runtime` |
| `test` | `--profile NAME`, `--scenario NAME`, `--pending-function NAME` | `runtime` phase |
| `run` | `--entry NAME`, `--profile NAME` | `# expect-result:` |

Exit status 0 means success and 1 means rejection or runtime failure. Any
other status, a signal, or a timeout fails the case. [OPEN-13]

Output lines the runner reads:

- **Located diagnostic:** `PATH:LINE:COL: CODE: message`, where `PATH` names
  `FILE` [OPEN-5], `LINE` and `COL` are 1-based, and the message is free text.
- **Located warning:** `PATH:LINE:COL: warning: CODE: message`.
- **Panic report:** `CODE: message`, which may be prefixed by `PATH:LINE:COL: `.
  [OPEN-4]
- **Result line (run only):** the rendered result [OPEN-3].

The runner ignores all other output.

---8<---

## Open questions for `OPEN_ISSUES.md`

Each question comes from a measured gap. None of them is decided here.

1. **Result rendering.** Should `VALUE` be the language's `Display` rendering
   of the returned value? The MVP prints the host's JavaScript formatting of
   the Wasm return: an f64 `3.0` prints `3`, and a char prints its scalar
   number (`128512`). Or should `# expect-result:` be retired in favor of
   `assert_equal` inside test blocks? (F-202; 140 fixtures, 169 directives.)
2. **Entry functions.** May `ENTRY` be any zero-parameter function, or only
   entry points as defined in chapter 10 (`pub fn main() -> void`)? Today 125
   fixtures run a non-`pub` `main` returning `i32`, and 13 of them carry a
   requirement row that no profile supplies. (F-201, `selfcontain.tsv`.)
3. **Result line format.** Should `run` print the result on a marked line,
   such as `result: VALUE`, so that console output cannot satisfy the check?
   (F-156.)
4. **Panic location.** Chapter 06 requires a location "when one is
   available", and markers name a line. Should the runner compare the panic
   line with the marker, and what does a scenario-driven panic mark? Today no
   implementation reports one and no runner checks it. (F-155, F-204.)
5. **Diagnostic location and path.** Must `PATH` equal the argv path byte for
   byte (today's runner), or only name the same file (the fuzz contract:
   "ends with the basename")? Is the marker line a per-diagnostic spec rule or
   an MVP span? (F-207: 239 of 551 cases fail with relative paths; F-102.)
6. **Extra diagnostics.** May a reject case report errors other than its
   marker? (F-150, F-208.)
7. **Test-block execution.** Do test blocks run in the same instance as
   `main`, and after it? Four MVP fixtures (suspension/32, 33, 35, 36) read
   state that `main` wrote.
8. **`disposed-file` surface.** Does the profile implement any fixture
   `Files`/`FileHandle` pair by method name, or one fixed trait declaration
   that the suite supplies? What do reads before close and a second close
   return?
9. **Profile inventory.** Should the MVP's `ready-counter`, `ready-float`,
   `ready-gate`, and `ready-text` profiles (defined only in `src/cli.ts`)
   join the suite, or should those fixtures stay implementation tests?
10. **Competing drivers in source terms.** What is "a second driver":
    `block_on` of the same stored suspension, a second host `start`, or a
    second call of the entry export? The MVP calls the `main` export again,
    and the panic appears even when `main!` is `pass`.
11. **Synthetic packages.** Where do the sources of `dep.validation` and
    `dep.models` live? Nothing in the repository defines them, and no runner
    reads `fixture-package-role`.
12. **Working directory.** The runner sets it to the repository root. Can the
    contract drop that requirement?
13. **Timeout and bad exits.** Should the conformance runner adopt the fuzz
    contract's 10 s timeout and `bad-exit` rule?
14. **`runtime-error`.** Should the `runtime-error` panic wildcard, allowed
    only in `test/fixtures`, be removed from the format? If so, which stable
    category does a negative integer exponent raise? (F-153.)
15. **Stdout expectations.** Should the format gain an output expectation for
    console fixtures? `suspension/27` claims UTF-8 streaming but only
    type-checks. (F-210.)

## Mapping from today's `test/fixtures` directives

| `test/fixtures` form | Conformance form under this proposal |
| --- | --- |
| `# test: NAME` | optional; cases.tsv path is the identity |
| `# expect: parse` | `parse` phase, `accept`, under `parse/valid` |
| `# expect: accept` | `type` phase, `accept`, under `typing/valid` |
| `# expect: test` | `runtime` phase, `accept`, under `runtime/valid` |
| `# expect-result: E = V` | `runtime` phase, `accept`, plus the directive (after OPEN-1 to OPEN-3) |
| `# diagnostic:` at a lexer or parser code | `parse` phase, `reject:CODE`, under `parse/invalid` |
| `# diagnostic:` otherwise | `type` phase, `reject:CODE`, under `typing/invalid` |
| `# warning:` | `type` phase, `warn:CODE`, under `typing/warnings` |
| `# panic: CODE` | `runtime` phase, `panic:CODE`, under `runtime/panic`; `runtime-error` has no equivalent |
| more than one marker | split into one fixture per marker |
