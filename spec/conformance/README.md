# Conformance Fixtures

These fixtures turn normative specification examples into parser,
type-checker, and runtime tests. They do not define semantics independently
of the specification.

This file is the complete definition of the fixture format, and of the
command contract a conformance runner uses. An implementation needs nothing
outside `spec/` to run the suite.

## Terms

- **Fixture:** one UTF-8 `.hd` file under `spec/conformance/`, the single
  primary input of one case.
- **Case:** one row of `cases.tsv`. It names a fixture, a phase, and an
  expectation.
- **Implementation under test:** a command prefix, such as `hd` or
  `node --experimental-strip-types bin/hd.js`. The runner appends an action,
  options, and the fixture path. See [Command Contract](#command-contract).
- **Runner:** the tool that reads `cases.tsv`, invokes the implementation,
  and judges each case by the rules below.
- **Selection manifest:** an optional list of case paths that an
  implementation claims to support. See [Case Selection](#case-selection).

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
  an inline `# panic: CODE` comment on the triggering source line.

The directory is informative. A case's phase and expectation come from
`cases.tsv`, and the two must agree.

## Self-Containment and Determinism

A fixture may rely only on:

- primitive types and prelude names
  ([Modules](../10-modules.md#prelude));
- standard modules that a numbered chapter specifies, such as `std.testing`,
  `std.task`, and `std.resource`;
- its own declarations;
- the environment its fixture directives name (see
  [Fixture Environments](#fixture-environments)).

Every other user-defined type, trait, and function must be declared in the
fixture, including every requirement key.

A fixture must not depend on:

- its file name, its directory, the working directory, or other files;
- the clock, randomness, or timing;
- diagnostic message text;
- map iteration order beyond insertion order;
- any value rendering the specification does not define.

Runtime results are observed with `assert` and `assert_equal` from
`std.testing`, inside named test blocks.

## Comment Directives

A directive is a line comment that the runner reads. It has no meaning to the
language. There are two forms:

- **Header directives** occupy a whole line: `# `, the directive name, `: `,
  and a value. They may appear on any line; by convention they come first.
  Each appears at most once per fixture.
- **Line markers** end a source line, in the form `# KIND: CODE`. `KIND` is
  `diagnostic`, `warning`, or `panic`, and `CODE` matches `[a-z0-9-]+`,
  followed by optional trailing whitespace and the line end. The marker names
  the source line it ends.

A reject, warn, or panic fixture has exactly one line marker. An accept
fixture has none. `spec/check.sh` enforces both rules, and rejects any header
directive not listed below.

| Directive                                   | Form        | Meaning |
| ------------------------------------------- | ----------- | ------- |
| `# diagnostic: CODE`                        | line marker | The case's error category, reported on this line. `CODE` is in an Error row of [the diagnostic table](../README.md#diagnostics). |
| `# warning: CODE`                           | line marker | The case's warning category, reported on this line. `CODE` is in the Warning row. |
| `# panic: CODE`                             | line marker | The case's panic category, raised by this line. `CODE` is a stable category from [Runtime Panics](../06-control-flow.md#runtime-panics). |
| `# test: NAME`                              | header      | Optional human-readable name. Cases are identified by path, never by name. |
| `# expect: parse`, `accept`, or `test`      | header      | Optional restatement of a `parse`, `type`, or `runtime` accept case. If present, it must agree with `cases.tsv`. |
| `# fixture-runtime-profile: NAME`           | header      | Selects a named host profile. See [Runtime Profiles](#runtime-profiles). |
| `# fixture-runtime-scenario: NAME`          | header      | Replaces ordinary execution with a named driving procedure. See [Runtime Scenarios](#runtime-scenarios). |
| `# fixture-runtime-pending-function: NAME`  | header      | Holds the named suspending function pending. Valid only with the `cancellation-cleanup` scenario. |
| `# fixture-package-role: ROLE`              | header      | Selects the synthetic multi-package environment. See [Package Roles](#package-roles). |

## Case Index

`cases.tsv` is the authoritative fixture index. Its header is
`path	phase	expectation	specification`, and each row has four non-empty
tab-separated fields:

- `path`: the fixture path relative to this directory;
- `phase`: `parse`, `type`, or `runtime`;
- `expectation`: `accept`, `reject:CODE`, `warn:CODE`, or `panic:CODE`. For a
  marked fixture, it equals the marker kind (`diagnostic` is written
  `reject`) and code;
- `specification`: the one primary section, as `NN-chapter.md#anchor`.

Every fixture has exactly one row.

`examples.tsv` inventories every `text` code fence in the numbered core
chapters. Runnable examples map to one or more self-contained fixtures.
Lexical inventories, type fragments, filesystem layouts, and similar
non-program text are classified explicitly rather than being mistaken for
compilable files. `spec/check.sh` verifies that every core `text` fence has
exactly one inventory entry, and that every runnable entry names an indexed
fixture.

## Case Selection

An implementation that supports only part of the language may give the
runner a selection manifest: a text file listing one case path per line.
The runner then runs only the listed cases, and reports pass and fail counts
for them. Unlisted cases are not run and are not reported as passing. Without
a manifest, the runner runs every case.

There is no "unsupported" result. A case that an implementation runs is
judged only by the rules below.

## Judging a Case

`check` and `test` receive the options the fixture's directives select (see
[Command Contract](#command-contract)).

| Phase     | Expectation   | Steps                          | Passes when |
| --------- | ------------- | ------------------------------ | ----------- |
| `parse`   | `accept`      | `parse FILE`                   | exit 0 |
| `parse`   | `reject:CODE` | `parse FILE`                   | exit 1, a located `CODE` on the marker line, and no other located error |
| `type`    | `accept`      | `check FILE`                   | exit 0 (warnings allowed) |
| `type`    | `reject:CODE` | `check FILE`                   | exit 1, a located error `CODE` on the marker line, and no other located error |
| `type`    | `warn:CODE`   | `check FILE`                   | exit 0, and a located warning `CODE` on the marker line |
| `runtime` | `accept`      | `check FILE`, then `test FILE` | both exit 0 |
| `runtime` | `panic:CODE`  | `check FILE`, then `test FILE` | `check` exits 0; `test` exits 1 and reports panic category `CODE` |

Rules that apply to every case:

- **Codes and lines, not wording.** The runner judges only codes, lines, and
  exit statuses. Diagnostic wording is not normative.
- **Marker line.** A located diagnostic counts for the marker when its
  `LINE` equals the marker's line number. Its column is not judged. The line
  is the one given by the location rule in
  [Diagnostics](../README.md#diagnostics).
- **No other errors.** A reject case fails if any error other than its
  marked one is reported. Warnings never fail a case.
- **Phase is the latest point.** The phase names the latest command at which
  a rejection must appear. A `type`-phase rejection may also be reported by
  `parse`. A `parse`-phase rejection must be reported by `parse`.
- **Runtime cases type-check first.** A runtime case whose `check` fails,
  fails.
- **Panics are judged by category.** The reported category must equal
  `CODE` exactly. The panic marker documents the intended line, but the line
  is not judged, because [Runtime Panics](../06-control-flow.md#runtime-panics)
  requires a location only when one is available. There is no wildcard
  category.

## Runtime Execution

`test FILE` executes the fixture as follows:

1. If the module declares an entry point `main` or `main!`
   ([Executable Entry Point](../10-modules.md#executable-entry-point)), it
   runs in a fresh program instance, after module initialization, with its
   requirement row supplied by the selected runtime profile. Its return value
   is not judged.
2. Each `test "..."` block runs in its own fresh program instance, after
   module initialization. `main` does not run in that instance.
3. The command succeeds when nothing panicked and no `std.testing` assertion
   failed.

Only entry points and test blocks execute. A fixture that needs to observe a
function's result calls it from a test block and checks the result with
`assert_equal`.

A fixture with a `# fixture-runtime-scenario:` directive replaces steps 1
and 2 with that scenario's procedure.

## Fixture Environments

### Runtime Profiles

A runtime profile is a named set of host capability traits and the providers
that implement them ([Wasm Boundary](../10-modules.md#wasm-boundary)). With
`# fixture-runtime-profile: NAME`, the runner passes the profile to both
`check` and `test`. The profile's traits become host capabilities that may
appear in entry-point rows. A fixture declares every trait a profile
implements, with the exact method signatures below, and the profile supplies
one provider value per trait.

- `disposed-file` implements the fixture's `Files` and `FileHandle` traits:
  - `Files.open!(self) -> mut FileHandle` completes on its first poll with a
    fresh open handle;
  - `FileHandle.close(mut self) -> Result[void, ResourceError[E]]` returns
    `Ok` on the first call;
  - after a successful close, every `FileHandle` operation returns
    `Err(ResourceError.Disposed)` and must not trap. `E` is the fixture's
    error type.

  The complete trait surface is an open question
  ([Open Issues](../../future-work/OPEN_ISSUES.md#questions-from-the-compiler-audit)).
- `pending-gate` implements the fixture's
  `trait Gate: fn wait!(self) -> void`. Every poll of `wait!` stays pending.

Implementations may define more profiles for their own tests. A conformance
fixture may name only the profiles listed here.

### Runtime Scenarios

A scenario is a fixed host procedure for protocol states that
single-threaded source cannot create by itself. Each scenario starts from a
fresh instance, with the entry `main!` and its row supplied by the selected
profile.

- `cancellation-cleanup`:
  1. Construct the `main!` suspension.
  2. Poll it once. The poll must return pending: it stays pending at the
     first `pending-gate` `wait!`, or at the function named by
     `# fixture-runtime-pending-function:`.
  3. Cancel the suspension.
  4. Call the fixture's top-level `cleanup_ran() -> bool` in the same
     instance. The case passes when it returns `true`.
- `competing-drivers`:
  1. Construct the `main!` suspension.
  2. Poll it once, holding every suspending callee pending, so that the poll
     returns pending.
  3. While the first driver still owns that suspension, start a second drive
     of the same entry.
  4. The case passes when step 3 raises `suspension-competing-driver`.
- `reentrant-poll`:
  1. Construct the `main!` suspension, and poll it.
  2. During that poll, at its first suspension point, poll the same
     suspension again before the first poll returns.
  3. The case passes when step 2 raises `suspension-reentrant-poll`.

How a "second drive" and a "re-entrant poll" are expressed without an
implementation hook is an open question
([Open Issues](../../future-work/OPEN_ISSUES.md#questions-from-the-compiler-audit)).

`# fixture-runtime-pending-function: NAME` makes the deterministic runtime
hold the named suspending function pending at every poll. The runtime makes
no host call for it. The directive is valid only with
`cancellation-cleanup`.

### Package Roles

`# fixture-package-role: library` and
`# fixture-package-role: root-application` select the synthetic
multi-package environment. The primary file is the only case input. It is
compiled as the root module of a package in the named role. The runner also
supplies the packages `dep.validation` and `dep.models`. Where their sources
live is an open question
([Open Issues](../../future-work/OPEN_ISSUES.md#questions-from-the-compiler-audit)).

## Command Contract

The runner invokes the implementation as:

```text
IMPL ACTION [OPTION VALUE]... FILE
```

- `FILE` is the path to the fixture, and is always the last argument.
- The working directory is not part of the contract.
- stdin is closed. stdout and stderr are both read.

| Action  | Options the runner may pass                                     | Used for |
| ------- | --------------------------------------------------------------- | -------- |
| `parse` | none                                                            | `parse` phase |
| `check` | `--profile NAME`                                                | `type` phase, and the first step of `runtime` |
| `test`  | `--profile NAME`, `--scenario NAME`, `--pending-function NAME`  | `runtime` phase |

Exit statuses and limits:

- Exit status 0 means success, and 1 means rejection or runtime failure.
- Exit status 1 always comes with at least one located diagnostic or panic
  report.
- Any other status, a signal, or running longer than 10 seconds fails the
  case.

Output lines the runner reads:

- **Located diagnostic:** `PATH:LINE:COL: CODE: message`. `PATH` names the
  same file as `FILE`, either as given or as any other path to that file.
  `LINE` and `COL` are 1-based. The message is free text.
- **Located warning:** `PATH:LINE:COL: warning: CODE: message`.
- **Panic report:** `CODE: message`, optionally prefixed by
  `PATH:LINE:COL: `.

The runner ignores all other output.
