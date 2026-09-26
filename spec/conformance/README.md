# Conformance Fixtures

These fixtures turn normative specification examples into parser,
type-checker, and runtime tests. They do not define semantics independently
of the specification.

This file is the complete definition of the fixture format, and of the
command contract a conformance runner uses. An implementation needs nothing
outside `spec/` to run the suite.

## Terms

- **Fixture:** one UTF-8 `.hd` file under `spec/conformance/`, outside
  `packages/`, the single primary input of one case.
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
  [Fixture Environments](#fixture-environments)), including the package
  sources a package role supplies.

Every other user-defined type, trait, and function must be declared in the
fixture, including every requirement key.

A fixture must not depend on:

- its file name, its directory, the working directory, or other files,
  except the package sources its package role supplies;
- the clock, randomness, or timing;
- diagnostic message text;
- map iteration order beyond insertion order;
- any value rendering the specification does not define.

Runtime results are observed with `assert` and `assert_equal` from
`std.testing`, inside named test blocks, and console output is observed with
`# expect-stdout:` (see [Standard Output](#standard-output)).

## Comment Directives

A directive is a line comment that the runner reads. It has no meaning to the
language. There are two forms:

- **Header directives** occupy a whole line: `# `, the directive name, `: `,
  and a value. They may appear on any line; by convention they come first.
  Each appears at most once per fixture, except `# expect-stdout:`, which
  may repeat.
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
| `# expect-stdout: TEXT`                     | header      | One line of the entry point's exact standard output, in order. Valid only in a `runtime` `accept` case. See [Standard Output](#standard-output). |

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
| `runtime` | `accept` with `# expect-stdout:` | `check FILE`, `test FILE`, then `run FILE` | all exit 0, and the stdout of `run` equals the expected text |
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

- `console` is the profile a fixture gets when it names no profile. The
  runner passes no `--profile` option for it. It implements the prelude
  `Console` ([Prelude](../10-modules.md#prelude)): each
  `write_line!(text)` completes on its first poll, writes the UTF-8 encoding
  of `text` followed by one U+000A to standard output, and returns `Ok`.
- `disposed-file` implements the fixture's `Files` and `FileHandle` traits.
  The fixture declares exactly these three methods, where `E` is the
  fixture's own error type:

  ```text
  trait Files:
      fn open!(self) -> mut FileHandle

  trait FileHandle:
      fn read!(mut self) -> Result[string, ResourceError[E]]
      fn close(mut self) -> Result[void, ResourceError[E]]
  ```

  - `open!` completes on its first poll with a fresh open handle.
  - Before the handle is closed, `read!` completes on its first poll with
    `Ok("")`.
  - The first `close` returns `Ok`.
  - After a successful close, every operation on that handle, including a
    second `close`, returns `Err(ResourceError.Disposed)` and must not trap.
- `pending-gate` implements the fixture's
  `trait Gate: fn wait!(self) -> void`. Every poll of `wait!` stays pending.

Implementations may define more profiles for their own tests. A conformance
fixture may name only the profiles listed here.

### Runtime Scenarios

A scenario is a fixed host procedure for protocol states that
single-threaded source cannot create by itself. Each scenario starts from a
fresh instance, with the entry `main!` and its row supplied by the selected
profile. Its steps use only host operations on the `main!` suspension: poll
it with a `PollContext`, and cancel it
([`Suspend[T]` Protocol](../11-requirements-and-suspension.md#suspendt-protocol)).
It also uses two actions of the deterministic fixture runtime:

- **Hold** a suspending call: answer every poll of that call's suspension
  with pending, without evaluating its body and without a host call.
- **Intercept** a suspension point: when the body of the `main!` suspension
  first reaches a bang call, give control to the scenario before the callee's
  suspension is polled. The scenario's next step runs there, inside the poll
  that reached the bang call.

A **driver** is one host drive loop that owns a suspension from its first
poll until the suspension completes or is cancelled. Each driver polls with
its own `PollContext`.

- `cancellation-cleanup`:
  1. Construct the `main!` suspension.
  2. Poll it once. The poll must return pending: it stays pending at the
     first `pending-gate` `wait!`, or at the function named by
     `# fixture-runtime-pending-function:`.
  3. Cancel the suspension.
  4. Call the fixture's top-level `cleanup_ran() -> bool` in the same
     instance. The case passes when it returns `true`.
- `competing-drivers`:
  1. Construct the `main!` suspension `S`.
  2. Driver A polls `S` once, holding the first suspending call that `S`'s
     body reaches. The poll returns pending. A still owns `S`, which is
     unfinished but not active.
  3. A second driver B, which has never polled `S`, polls `S` with its own
     `PollContext`.
  4. The case passes when the poll in step 3 raises
     `suspension-competing-driver`.
- `reentrant-poll`:
  1. Construct the `main!` suspension `S`.
  2. Driver A polls `S` once, intercepting the first suspension point that
     `S`'s body reaches.
  3. At that interception, while A's poll of `S` is still active, A polls `S`
     again with the same `PollContext`.
  4. The case passes when the poll in step 3 raises
     `suspension-reentrant-poll`.

`# fixture-runtime-pending-function: NAME` makes the deterministic runtime
hold every call of the named suspending function. The directive is valid only
with `cancellation-cleanup`.

### Package Roles

`# fixture-package-role: library` and
`# fixture-package-role: root-application` select the synthetic
multi-package environment. The primary file is compiled as the root module
of a package in the named role. That package depends on every package under
[`packages/`](packages), each in the library role:

- Each directory `packages/NAME/` is the source root of one package, used as
  `dep.NAME` ([Use Roots](../10-modules.md#use-roots)). Its `mod.hd` is the
  package's root module.
- [`dep.validation`](packages/validation/mod.hd) declares the facet
  `Validation`, with its `Annotation`, `TypeAnnotator`, and `DataAnnotator`
  implementations, and annotates `string`.
- [`dep.models`](packages/models/mod.hd) declares `data User` with one
  public `name: string` field.
- Neither package annotates `User`, so no library in the graph owns the pair
  `(Validation, User)`.

Package files are not cases. They have no row in `cases.tsv`, carry no
directives, and are never judged on their own.

The runner passes `--package-role ROLE` to `check` and `test`. It also
passes `--dependency NAME=DIR` once for each package directory, in ascending
order of `NAME`, where `DIR` is the absolute path of `packages/NAME`.

### Standard Output

A `runtime` `accept` case may state the exact standard output of its entry
point with one or more `# expect-stdout: TEXT` lines. Such a fixture names no
runtime profile and no scenario, so it runs under the `console` profile, and
it declares an entry point.

- Each directive is one output line. The directives are read in source
  order.
- `TEXT` is the rest of the line after `# expect-stdout: `. It is taken
  literally, except for three escapes: `\\` is one backslash, `\t` is
  U+0009, and `\u{H}` is the scalar value with 1 to 6 hexadecimal digits
  `H`. Any other backslash sequence makes the fixture invalid.
- `TEXT` may not end in whitespace; write a trailing space as `\u{20}`. The
  line `# expect-stdout:` with nothing after the colon is an empty line.
- The expected output is each decoded `TEXT` followed by one U+000A,
  concatenated in order. Output that does not end in U+000A cannot be
  expected.

After `check` and `test` pass, the runner invokes `run FILE`. The case
passes when `run` exits 0 and its standard output, decoded as UTF-8, equals
the expected output exactly. No carriage return, trailing newline, or
whitespace is normalized. Standard error is not judged.

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
| `check` | `--profile NAME`, `--package-role ROLE`, `--dependency NAME=DIR` | `type` phase, and the first step of `runtime` |
| `test`  | `--profile NAME`, `--scenario NAME`, `--pending-function NAME`, `--package-role ROLE`, `--dependency NAME=DIR` | `runtime` phase |
| `run`   | none                                                            | `runtime` cases with `# expect-stdout:` |

`run FILE` executes only the entry point, in a fresh program instance under
the `console` profile, as in step 1 of
[Runtime Execution](#runtime-execution). Its standard output is exactly the
program's console output.

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
