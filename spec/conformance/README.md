# Conformance Fixtures

These fixtures turn normative specification examples into parser,
type-checker, and runtime tests. They do not define semantics independently
of the specification.

This file is the complete definition of the fixture format, and of the
command contract a conformance runner uses. An implementation needs nothing
outside `spec/` to run the suite.

## Terms

- **Fixture:** one UTF-8 `.hd` file under `spec/conformance/`, outside
  `packages/` and `trees/`, the single primary input of one case.
- **Case:** one row of `cases.tsv`. It names a fixture, a phase, and an
  expectation.
- **CLI case:** one row of `cli-cases.tsv`. It names a directory under `cli/`
  that holds input files and an `expect.txt`. See [CLI Cases](#cli-cases).
- **Implementation under test:** a command prefix, such as `hd` or
  `node --experimental-strip-types bin/hd.js`. The runner appends an action,
  if any, options, and the fixture path. See [Command Contract](#command-contract).
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

`cli/NAME/` holds the inputs and the `expect.txt` of one CLI case. It holds no
fixture of the `cases.tsv` suite, and `cases.tsv` has no row for it.

## Self-Containment and Determinism

A fixture may rely only on:

- primitive types and prelude names
  ([Modules](../lang/10-modules.md#prelude));
- the standard items its [tier](#tiers) allows, listed below;
- its own declarations;
- the environment its fixture directives name (see
  [Fixture Environments](#fixture-environments)), including the package
  sources a package role or a package tree supplies.

Every other user-defined type, trait, and function must be declared in the
fixture, including every requirement key.

The standard items split by tier. **Language-tier items** are usable by
every fixture. They are the items a numbered chapter specifies:

- the harness from `std.testing`: `it`, `assert`, `assert_equal`, and a
  literal `snapshot`, and the test registration functions `it_each`,
  `it_prop`, and `it_prop_with`;
- the lang items and traits of `std.task`, `std.resource`, `std.convert`,
  `std.error`, `std.inspect`, `std.function`, `std.process`, `std.ops`,
  `std.num`, `std.structure`, `std.annotation`, and `std.format`
  (`Display`, `Debug`, `DebugWriter`, and `debug`), and `Iterable` and
  `Iterator` from `std.iter`;
- the intrinsics of built-in types, such as `List.push`, `string.len`,
  and `string.slice`.

**Stdlib-tier items** are usable only by a stdlib-tier fixture. They are the
items a [stdlib chapter](../std/README.md#chapters) specifies, and
[`stdlib-items.tsv`](stdlib-items.tsv) lists the import path of each.
A std method, such as `string.trim` or `Iterator.filter`, has no import
path, so the list cannot name it; a language-tier fixture still does not
call one.

A numbered chapter still specifies an item until a migration task moves it
into `spec/std/`, and until then any fixture may use it. Every planned
move is done.

A fixture must not depend on:

- its file name, its directory, the working directory, or other files,
  except the package sources its package role or package tree supplies;
- the clock, randomness, or timing;
- diagnostic message text;
- map iteration order beyond insertion order;
- any value rendering the specification does not define.

Runtime results are observed with `assert` and `assert_equal` from
`std.testing`, inside test cases (`it("name"):` calls in the fixture's
`tests:` block), and console output is observed with `# expect-stdout:` (see
[Standard Output](#standard-output)).

A fixture is a single ordinary module, never a
[test module](../lang/10-modules.md#test-modules) or an integration test module,
whatever its file name, unless a `# fixture-test-layout:` header places it
as one (see [Test Layouts](#test-layouts)).

A fixture with no package role, test layout, or package tree header is a
[single-file program](../lang/10-modules.md#single-file-programs): it is in no
package, whatever directory holds the suite.

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
| `# panic: CODE`                             | line marker | The case's panic category, raised by this line. `CODE` is a stable category from [Runtime Panics](../lang/06-control-flow.md#runtime-panics). |
| `# test: NAME`                              | header      | Optional human-readable name. Cases are identified by path, never by name. |
| `# expect: parse`, `accept`, or `test`      | header      | Optional restatement of a `parse`, `type`, or `runtime` accept case. If present, it must agree with `cases.tsv`. |
| `# fixture-runtime-profile: NAME`           | header      | Selects a named host profile. See [Runtime Profiles](#runtime-profiles). |
| `# fixture-runtime-scenario: NAME`          | header      | Replaces ordinary execution with a named driving procedure. See [Runtime Scenarios](#runtime-scenarios). |
| `# fixture-runtime-pending-function: NAME`  | header      | Holds the named suspending function pending. Valid only with the `cancellation-cleanup` scenario. |
| `# fixture-package-role: ROLE`              | header      | Selects the synthetic multi-package environment. See [Package Roles](#package-roles). |
| `# fixture-test-layout: LAYOUT`             | header      | Places the fixture as a test module or an integration test module. See [Test Layouts](#test-layouts). |
| `# fixture-package-tree: TREE/PATH`         | header      | Places the fixture in a package of several files. See [Package Trees](#package-trees). |
| `# expect-stdout: TEXT`                     | header      | One line of the entry point's exact standard output, in order. Valid only in a `runtime` `accept` case. See [Standard Output](#standard-output). |
| `# expect-empty-stdout: true`               | header      | Running the fixture as a single file exits 0 and writes nothing to standard output. Valid only in a `runtime` `accept` case. See [Standard Output](#standard-output). |

## Case Index

`cases.tsv` is the authoritative fixture index. Its header is
`path	phase	expectation	specification`, and each row has four non-empty
tab-separated fields:

- `path`: the fixture path relative to this directory;
- `phase`: `parse`, `type`, or `runtime`;
- `expectation`: `accept`, `reject:CODE`, `warn:CODE`, or `panic:CODE`. For a
  marked fixture, it equals the marker kind (`diagnostic` is written
  `reject`) and code;
- `specification`: the one primary section, as `lang/NN-chapter.md#anchor`, or
  as `std/MODULE.md#anchor` for a stdlib chapter. The anchor may instead
  name one rule, as `r-` followed by its [rule ID](../STYLE.md#rule-ids).
  This column sets the case's [tier](#tiers).

Every fixture has exactly one row.

`examples.tsv` inventories every `text` code fence in the numbered core
chapters. Runnable examples map to one or more self-contained fixtures.
Lexical inventories, type fragments, filesystem layouts, and similar
non-program text are classified explicitly rather than being mistaken for
compilable files. `spec/check.sh` verifies that every core `text` fence has
exactly one inventory entry, and that every runnable entry names an indexed
fixture.

## Tiers

The suite covers both tiers of the specification
([spec/std](../std/README.md#tiers)).

1. A case's tier is the tier of its `specification` column. A value that
   starts with `std/` makes the case stdlib tier. Every other case is
   language tier. A [CLI case](#cli-cases) is CLI tier.
2. There is no tier column, directive, or directory. The fixture's
   directory stays informative.
3. A language-tier fixture uses only language-tier standard items (see
   [Self-Containment and Determinism](#self-containment-and-determinism)).
   A stdlib-tier fixture may use items of both tiers.
4. [`stdlib-items.tsv`](stdlib-items.tsv) lists the stdlib-tier items by
   import path. Its header is `item	specification`. `item` is a module
   path, such as `std.time`, or an item path, such as `std.time.s`.
   `specification` is the `std/` section that specifies the item. Each
   migration task that moves a section adds its items.
5. `spec/check.sh` rejects a language-tier fixture whose `use` line imports
   a listed item, or a member of a listed module.
6. [`tier-crossings.tsv`](tier-crossings.tsv) lists the language-tier
   fixtures that still use a stdlib-tier item, until a migration task fixes
   each one. Its header is `path	items	reason`. `items` is a
   comma-separated list of import paths, such as `std.time.s`, and method
   calls, such as `string.trim`. The check allows a listed import.
7. `spec/check.sh` rejects a crossing row whose case is stdlib tier, or
   whose fixture no longer imports or calls one of its items.
8. A `parse`-phase fixture resolves no names, so a name it uses selects no
   item of either tier. Such a fixture may call a method or apply a prefix,
   such as `r"..."`, that is stdlib tier elsewhere. It needs no crossing
   row, and it runs in `--tier language` unchanged.

An implementation with a different standard library runs the stdlib-tier
cases alone with `--tier std`. A new compiler whose standard library has
only the language tier runs `--tier language`, which needs no stdlib-tier
item once `tier-crossings.tsv` is empty.

## Case Selection

An implementation that supports only part of the language may give the
runner a selection manifest: a text file listing one case path per line.
The runner then runs only the listed cases, and reports pass and fail counts
for them. Unlisted cases are not run and are not reported as passing. Without
a manifest, the runner runs every case.

A selection manifest names a [CLI case](#cli-cases) by its directory,
`cli/NAME`, in the same list as the fixtures.

A runner may also select cases by [tier](#tiers) with `--tier language`,
`--tier std`, or `--tier cli`, and by phase with `--phase parse`, `type`, or
`runtime`. A CLI case has no phase, so `--phase` leaves it out. Each
option narrows a manifest's selection further. Without them, the runner
runs cases of every tier and phase. Its summary line reports passes per
tier, as `language: X of Y; stdlib: X of Y; cli: X of Y`.

There is no "unsupported" result. A case that an implementation runs is
judged only by the rules below.

## Judging a Case

`check` and `test` receive the [runner options](#runner-options) the
fixture's directives select, and `check` always receives `--tests` (see
[Command Contract](#command-contract)).

| Phase     | Expectation   | Steps                          | Passes when |
| --------- | ------------- | ------------------------------ | ----------- |
| `parse`   | `accept`      | `parse FILE`                   | exit 0 |
| `parse`   | `reject:CODE` | `parse FILE`                   | exit 101, a located `CODE` on the marker line, and no other located error |
| `type`    | `accept`      | `check FILE`                   | exit 0 (warnings allowed) |
| `type`    | `reject:CODE` | `check FILE`                   | exit 101, a located error `CODE` on the marker line, and no other located error |
| `type`    | `warn:CODE`   | `check FILE`                   | exit 0, and a located warning `CODE` on the marker line |
| `runtime` | `accept`      | `check FILE`, then `test FILE` or the entry run | both exit 0 |
| `runtime` | `accept` with `# expect-stdout:` | `check FILE`, `test FILE`, then `FILE` | all exit 0, and the stdout of the last equals the expected text |
| `runtime` | `accept` with `# expect-empty-stdout:` | `check FILE`, `test FILE`, then `FILE` | all exit 0, and the last writes no standard output |
| `runtime` | `accept` with the `pending-first-poll` scenario | `check FILE`, `test FILE`, then `test FILE` with the `pending-first-poll` scenario | all exit 0 |
| `runtime` | `panic:CODE`  | `check FILE`, then `test FILE` or the entry run | `check` exits 0; `test` exits 1, or the entry run exits nonzero but not 101, and it reports panic category `CODE` |

Rules that apply to every case:

- **Codes and lines, not wording.** The runner judges only codes, lines, and
  exit statuses. Diagnostic wording is not normative.
- **Marker line.** A located diagnostic counts for the marker when its
  `LINE` equals the marker's line number. Its column is not judged. The line
  is the one given by the location rule in
  [Diagnostics](../README.md#diagnostics). A [package tree](#package-trees) case
  judges the code and not the line.
- **No other errors.** A reject case fails if any error other than its
  marked one is reported. Warnings never fail a case.
- **Phase is the latest point.** The phase names the latest command at which
  a rejection must appear. A `type`-phase rejection may also be reported by
  `parse`. A `parse`-phase rejection must be reported by `parse`.
- **Runtime cases type-check first.** A runtime case whose `check` fails,
  fails.
- **Panics are judged by category.** The reported category must equal
  `CODE` exactly. The panic marker documents the intended line, but the line
  is not judged, because [Runtime Panics](../lang/06-control-flow.md#runtime-panics)
  requires a location only when one is available. There is no wildcard
  category.

## Runtime Execution

`test FILE` runs steps 2 and 3, the fixture's test cases, and never step 1.
Step 1 is `IMPL FILE`:

1. If the module declares an entry point `main` or `main!`
   ([Executable Entry Point](../lang/10-modules.md#executable-entry-point)),
   `IMPL FILE` runs it in a fresh program instance, after module
   initialization, with its requirement row supplied by the selected runtime
   profile. Its return value is not judged.
2. Each test case of its `tests:` block runs in its own fresh program
   instance, after module initialization, as
   [Test Outcomes](../lang/10-modules.md#test-outcomes) describes. `main` does not
   run in that instance. A test case with the `ignore` option does not run.
3. The command succeeds when every test case that ran passed: nothing
   panicked except as a case's `expect_panic` option expects, and every
   result reported `ExitCode(0)`. A failed `std.testing` assertion is a
   panic.

`test FILE` is an error when FILE registers no test case
([`cli.test.file-empty`](../cli/command-line.md#r-cli.test.file-empty)), so the
runner runs `test FILE` only when the fixture registers one. A fixture
registers a test case when its source has a line that starts with `tests:`
or with `## ` and a fenced `hd` block (a [doc test](../lang/10-modules.md#doc-tests)),
or when a [test layout](#test-layouts) or a [package tree](#package-trees)
whose package path starts with `tests/` or ends in `_test.hd` places it as a
test module. Otherwise the runner runs only the entry, as `IMPL FILE` with the
fixture's [runner options](#runner-options): step 1 above, with no step 2. The
case passes when that run exits 0. For `panic:CODE`, the entry run must exit
nonzero but not 101, because [`module.entry.panic`](../lang/10-modules.md#r-module.entry.panic)
leaves the status to the runtime profile, and it must report panic category
`CODE`.

Only entry points and test cases execute. A fixture that needs to observe a
function's result calls it from a test case and checks the result with
`assert_equal`.

A fixture with a `# fixture-runtime-scenario:` directive other than
`pending-first-poll` replaces steps 1 and 2 with that scenario's procedure,
and the runner runs it as `test` whether or not the fixture registers a test
case. The `pending-first-poll` scenario runs steps 1 and 2 with its hook, so it
follows the rule above.

## Fixture Environments

### Runtime Profiles

A runtime profile is a named set of host capability traits and the providers
that implement them ([Wasm Boundary](../lang/10-modules.md#wasm-boundary)). With
`# fixture-runtime-profile: NAME`, the runner passes the profile to both
`check` and `test`. The profile's traits become host capabilities that may
appear in entry-point rows. A fixture declares every trait a profile
implements, with the exact method signatures below, and the profile supplies
one provider value per trait. Each provider has the access its trait gives
([Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers)):
mutable when the trait has a `mut self` method, readonly otherwise.

- `console` is the profile a fixture gets when it names no profile. The
  runner selects no runtime profile for it. It implements the prelude
  `Console` ([Prelude](../lang/10-modules.md#prelude)): each
  `write_line!(text)` completes on its first poll, writes the UTF-8 encoding
  of `text` followed by one U+000A to standard output, and returns `.Ok(())`.
- `consent-vault` implements the fixture's
  `trait Vault: fn keep(self, token: Token) -> Token`, where `Token` is the
  fixture's own data type. `keep` returns its argument's boundary value
  unchanged, so a `Token` crosses out of hd and back in
  ([Private Fields At A Boundary](../lang/10-modules.md#private-fields-at-a-boundary)).
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
    `.Ok("")`.
  - The first `close` returns `.Ok(())`.
  - After a successful close, every operation on that handle, including a
    second `close`, returns `.Err(ResourceError.Disposed)` and must not trap.
- `pending-gate` implements the fixture's
  `trait Gate: fn wait!(self) -> void`. Every poll of `wait!` stays pending.
- `pending-write` implements the prelude `Console`, as `console` does,
  except that each `write_line!(text)` call is pending on its first poll.
  Its second poll writes the line as `console` does, and completes with
  `.Ok(())`.
- `misbehaving-host` implements the fixture's
  `trait Gauge: fn level(self) -> u8` with a host that breaks its contract.
  Every call of `level` returns the integer 300, which no `u8` holds, so
  the boundary check of [Host Results](../lang/10-modules.md#host-results)
  fails.
- `special-float-host` implements the fixture's
  `trait Sensor: fn reading(mut self) -> f64`. Its first call of `reading`
  returns a NaN, its second positive infinity, and its third and every
  later call `-0.0`, each as a raw IEEE 754 value, so the
  [float row](../lang/10-modules.md#r-module.profile.host-result.float)
  of the boundary check accepts them.

Implementations may define more profiles for their own tests. A conformance
fixture may name only the profiles listed here.

### Runtime Scenarios

A scenario is a fixed host procedure for protocol states that
single-threaded source cannot create by itself. Each scenario but
`pending-first-poll` starts from a fresh instance, with the entry `main!` and
its row supplied by the selected profile. Its steps use only host operations on the `main!` suspension: poll
it with a `PollContext`, and cancel it
([`Suspend[T]` Protocol](../lang/11-requirements-and-suspension.md#suspendt-protocol)).
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

- `pending-first-poll`:
  1. Run steps 1 and 2 of [Runtime Execution](#runtime-execution), the entry
     and then the test cases, in one run under the `console` profile, except that every `write_line!` call is pending on
     its first poll, as under `pending-write`. The call's second poll writes
     the line and completes with `.Ok(())`.
  2. The case passes when that run exits 0, as the ordinary `test FILE` run
     of the same fixture does. The runner runs the ordinary `test FILE`
     first, so the case passes only when both runs succeed.

`pending-first-poll` is a fixture format, not a language rule: it shows that
a program's observable result does not depend on whether a host call
completes at once or after one pending poll. The fixture is a `runtime`
`accept` case, names no runtime profile, and reaches every suspension point
through a `Console` call, because only host calls become pending.

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
  `dep.NAME` ([Use Roots](../lang/10-modules.md#use-roots)). Its `lib.hd` is the
  package's root module.
- [`dep.models`](packages/models/lib.hd) declares `data User` with one
  public `name: string` field.
- [`dep.members`](packages/members/lib.hd) declares the public traits
  `Tagged` (method `tag`) and `Stamped` (method `mark`), the public data
  types `Inner` and `Outer`, and the public function `make_outer`. `Outer`
  embeds `Inner` (embedded fields are always public), has private fields
  `code` and `revision` and a private inherent method `stamp`, and
  implements `Tagged` and `Stamped`. `Inner` has a public field `note`, a
  private field `serial`, public inherent methods `tag` and `code`, and a
  private inherent method `audit`. It also declares the public data types
  `CreatedBySystem`, with a public field `id` and a public inherent method
  `label`, and `ReviewDraft`, which embeds `CreatedBySystem` and hides both
  members with a public field `id` and a public inherent method `label`,
  and the public function `make_draft`.

Package files are not cases. They have no row in `cases.tsv`, carry no
directives, and are never judged on their own.

The runner gives `check` and `test` the package role as a
[runner option](#runner-options). It also gives one dependency for each
package directory, in ascending order of `NAME`, where `DIR` is the absolute
path of `packages/NAME`.

### Test Layouts

`# fixture-test-layout: LAYOUT` places the primary file in a synthetic
package, as Package Roles does for the multi-package environment
(Testing T53). `LAYOUT` is one of:

| Layout | The file is compiled as | Its module |
| --- | --- | --- |
| `test-module` | `src/NAME_test.hd`, a [test module](../lang/10-modules.md#test-modules) | `NAME_test` |
| `integration` | `tests/NAME.hd`, an integration test module | `tests.NAME` |

- `NAME` is the fixture's file name without `.hd`. The package holds no
  other source file.
- The runner gives `check` and `test` the layout as a
  [runner option](#runner-options).
- A fixture with this header names no package role.

### Package Trees

`# fixture-package-tree: TREE/PATH` places the primary file in a package
of several files (Dependency Cycles DC11), as Package Roles does for the
multi-package environment:

- `TREE` names a directory [`trees/TREE/`](trees), which is the root of one
  package: its files are `src/...` and `tests/...` paths, as
  [Path-Inferred Modules](../lang/10-modules.md#path-inferred-modules) and
  [Test Modules](../lang/10-modules.md#test-modules) place them.
- `PATH` is the package path the primary file takes, such as
  `src/shop/mod.hd`. The tree holds no file at that path.
- The package has the tree's files and the primary file, and no
  dependencies. Its module under test is the primary file's module.
- Tree files are not cases. They have no row in `cases.tsv`, carry no
  directives, and are never judged on their own.
- A fixture with this header names no package role and no test layout.

The runner gives `check` and `test` the package tree as a
[runner option](#runner-options): `DIR`, the absolute path of `trees/TREE`,
and `PATH`.

A tree case differs from the Judging a Case table in two points:

- **Codes, not lines.** A located error or warning in any file of the tree
  counts for the marker, whatever its line, as a panic's line is not judged.
  A rule about several files, such as a loop of folders, does not say which
  file reports it. The marker documents one intended line.
- **Paths.** A located diagnostic's `PATH` names the primary file, as
  `FILE` or another path to it, or a file under `DIR`.

`test FILE` runs the primary module's entry point and test cases, after
initializing that module and the modules it uses, as
[Runtime Execution](#runtime-execution) describes. Its test cases include
the [doc tests](../lang/10-modules.md#doc-tests) of the primary file, and
`check FILE` covers them as `hd check --tests` does.

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

A `runtime` `accept` case may instead state that running it writes no
standard output, with the one header `# expect-empty-stdout: true`. The
value is always `true`. Such a fixture names no runtime profile, no
scenario, and no `# expect-stdout:` line, and it need not declare an
entry point, so a script with no entry behavior can be run.

After `check` and `test` pass, the runner invokes `FILE` with no action,
as `hd FILE` runs a single file ([Single Files](../cli/command-line.md#single-files)).
The case passes when that run exits 0 and its standard output, decoded as UTF-8, equals
the expected output exactly, or is empty for `# expect-empty-stdout:`. No
carriage return, trailing newline, or whitespace is normalized. Standard
error is not judged.

## Command Contract

The runner invokes the implementation as:

```text
IMPL ACTION [OPTION]... FILE
IMPL FILE
```

- `FILE` is the path to the fixture, and is always the last argument.
- The second form, with no action, runs the fixture as a single file.
- `parse` also covers the fixture's `tests:` block.
- The runner always passes `check` the `--tests` option, so `check` also
  covers the fixture's test code, as a test build does. Without `--tests`,
  `hd check` does not check test code, while `hd test` always compiles it.
- The working directory is not part of the contract. A [CLI case](#cli-cases)
  is the exception: it runs each command in the case's own directory.
- stdin is closed. stdout and stderr are both read.
- A runner may run the command lines in its own process through an adapter
  ([Adapters](../tools/README.md#adapters)). The adapter reports the exit
  status and output that the spawned command would.

| Action  | Command-line options | Used for |
| ------- | -------------------- | -------- |
| `parse` | none                 | `parse` phase |
| `check` | `--tests` (always)   | `type` phase, and the first step of `runtime` |
| `test`  | none                 | `runtime` phase |
| none    | none                 | `runtime` cases with `# expect-stdout:` or `# expect-empty-stdout:` |

`IMPL FILE` executes only the entry point, in a fresh program instance under
the `console` profile, as in step 1 of
[Runtime Execution](#runtime-execution). Its standard output is exactly the
program's console output.

### Runner Options

A fixture's directives select options for `check` and `test` that no `hd`
command line has: the runtime profile, the runtime scenario, the pending
function, the package role with its dependencies, the test layout, and the
package tree. The runner passes them to the implementation through its
[adapter](../tools/README.md#adapters), together with the command line,
and never as command-line options. An implementation takes them as its
adapter chooses, and its `hd` command does not accept or mention them.

| Option | Given to | From |
| ------ | -------- | ---- |
| profile `NAME` | `check`, `test` | `# fixture-runtime-profile:` |
| scenario `NAME` | `test` | `# fixture-runtime-scenario:` |
| pending function `NAME` | `test` | `# fixture-runtime-pending-function:` |
| package role `ROLE`, and dependencies `NAME` with `DIR` | `check`, `test` | `# fixture-package-role:`, and the directories of [`packages/`](packages) |
| test layout `LAYOUT` | `check`, `test` | `# fixture-test-layout:` |
| package tree `DIR` with package path `PATH` | `check`, `test` | `# fixture-package-tree:` |

A runner that spawns the implementation without an adapter has no way to give
these options, so it cannot run a case that selects one.

Exit statuses and limits:

- Exit status 0 means success.
- Exit status 101 means rejection: `hd` reports an error
  ([`cli.exit.hd-failure`](../cli/command-line.md#r-cli.exit.hd-failure)). It
  always comes with at least one located diagnostic.
- Exit status 1 means that `test` ran and a test case failed
  ([`cli.exit.test-failure`](../cli/command-line.md#r-cli.exit.test-failure)).
  It always comes with at least one panic report.
- Any other status, a signal, or running longer than 10 seconds fails the
  case.

Output lines the runner reads:

- **Located diagnostic:** `PATH:LINE:COL: CODE: message`. `PATH` names the
  same file as `FILE`, either as given or as any other path to that file,
  or, in a [package tree](#package-trees) case, a file under the tree.
  `LINE` and `COL` are 1-based. The message is free text.
- **Located warning:** `PATH:LINE:COL: warning: CODE: message`.
- **Panic report:** `CODE: message`, optionally prefixed by
  `PATH:LINE:COL: `.

The runner ignores all other output.

## CLI Cases

A CLI case judges the `hd` command line itself: exit statuses, the files a
command writes, and the machine output of `--format json`. It covers the
`cli.*` rules of [Command Line](../cli/command-line.md) that have such an
observable contract. The REPL, the wording of messages, and agent tooling
are not CLI cases.

The CLI tier is separate from the `cases.tsv` suite. A CLI case needs a
runner that starts the implementation in a directory, so an implementation
may run the language and stdlib tiers without it.

### Layout and Index

- `cli/NAME/` holds the input files of one case, such as an `hd.toml` and
  a source tree, and one `expect.txt`. `NAME` matches
  `[a-z0-9][a-z0-9-]*`. The case's path is `cli/NAME`.
- `cli-cases.tsv` is the index. Its header is `case	rules`, and each row
  has two non-empty tab-separated fields:
  - `case`: the directory name `NAME`;
  - `rules`: the comma-separated IDs of the `cli.*` rules the case checks,
    such as `cli.new.app,cli.new.test-name`. Each ID is defined in
    [`command-line.md`](../cli/command-line.md).
- Every directory under `cli/` has exactly one row, and every row has a
  directory with an `expect.txt`. `spec/check.sh` enforces both.
- The `.hd` files of a case are inputs. They carry no fixture directives and
  have no row in `cases.tsv`.
- A case's tier is CLI. A [selection manifest](#case-selection) lists it
  as `cli/NAME`.

### Running a Case

The runner:

1. copies the case directory, without `expect.txt`, to a fresh directory
   named `NAME`, so a package that a command creates there takes the name
   `NAME`;
2. runs each `run:` line of `expect.txt` in order, in that directory, with
   standard input closed. The first word, `hd`, stands for the
   implementation under test; the runner passes the other words as
   arguments, split at spaces, with no quoting;
3. after each command, judges the lines that follow its `run:` line, and
   stops the case at the first failure;
4. removes the directory.

A command that runs longer than 10 seconds, or ends with a signal, fails the
case. An implementation command that names a file by a relative path must
work from any directory, so the runner makes such a path absolute. A case
that expects a `.git` entry needs `git` on the `PATH`.

### expect.txt

Blank lines and lines that start with `#` are ignored. Every other line is
`KIND: VALUE`. A `run:` line starts a step, and the lines after it, up to
the next `run:`, are that step's assertions. A line before the first
`run:` is invalid.

| Line | Meaning |
| --- | --- |
| `run: hd WORDS` | Starts a step. `WORDS` may be empty. |
| `exit: CODE` | The step's exit status, from 0 to 255. A step with no `exit:` line must exit 0. At most one per step. |
| `stdout: TEXT` | One line of the step's exact standard output. `TEXT` follows the escapes of [Standard Output](#standard-output). The expected output is each `TEXT` followed by U+000A, in order. May repeat. Excludes `stdout-json:`. |
| `stdout-json: JSON` | The step's standard output is JSON lines that match `JSON`, as below. At most one. Excludes `stdout:`. |
| `stderr-json: JSON` | The same, for standard error. At most one. |
| `file: PATH` | After the step, `PATH` exists, as a file or a directory. `PATH` is relative to the case directory and has no `..`. |
| `no-file: PATH` | After the step, `PATH` does not exist. |

A step with neither `stdout:` nor `stdout-json:` does not judge standard
output, and one with no `stderr-json:` does not judge standard error.
Diagnostic message text is free text and no line asserts it.

**JSON lines.** The runner splits the stream at U+000A, drops the empty
piece after the last U+000A, and parses each remaining line as JSON. Every
line must be a JSON object, and a blank line is an error. This is
[`cli.json.lines.build`](../cli/command-line.md#r-cli.json.lines.build)'s
"no other text".

**Matching.** `JSON` is an array with one element per line, and the stream
must have exactly that many lines. `[]` means no output. Element `i` matches
line `i` by these rules, where `E` is the expected value and `A` the actual
one:

| `E` is | `E` matches `A` when |
| --- | --- |
| an object | `A` is an object, and for each key of `E`, `A` has the key and the values match |
| an array | `A` is an array of the same length, and the elements match in order |
| `null` | `A` is any value, so the key must exist and nothing more is checked |
| a string, number, or boolean | `A` equals it |

Keys of `A` that `E` does not name are ignored. A field of free text, such as
`message`, is written `null` to require that it exists.

```text
run: hd check --format json src/cart.hd
exit: 101
stdout-json: [{"kind":"diagnostic","code":"type-mismatch","message":null,"file":"src/cart.hd","line":3},{"kind":"summary","errors":1,"status":101}]
```
