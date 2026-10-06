# hd-lang MVP Compiler

This directory contains the executable Wasm GC MVP described in the archived
MVP_IMPLEMENTATION_PLAN.md. The implementation is
deliberately incremental: accepted programs compile to validated Wasm GC, and
features outside the current slice receive stable diagnostics.

Small pipeline stages remain direct modules such as `lexer.ts`, `ast.ts`,
`hir.ts`, and `wasm.ts`. Larger stages use same-named folders (`parser/`,
`checker/`, and `emitter/`). Each folder exposes its public surface only from
`index.ts`; consumers do not import its internal files.

**Frozen (2026-10-06).** The owner is writing a new compiler. This
prototype now serves as a test oracle: it takes bug fixes and spec-sync
fixes only, with no new features and no performance work
([Roadmap](../future-work/ROADMAP.md#order)). Its measured state is in the
[baseline report](../audit/compiler/baseline-2026-10-06.md).

[KNOWN_ISSUES.md](KNOWN_ISSUES.md) lists what the prototype gets wrong today:
its open findings, the tags of its known conformance failures, and the
applied decisions it does not follow yet.

## Run It

The repository pins Node 24.19.0 and its dependencies through
`pnpm-lock.yaml`.

```sh
pnpm install
pnpm run toolchain:gate
pnpm run lint
pnpm run format:check
pnpm run test:portable
pnpm test
pnpm run test:ui
pnpm run test:all
pnpm run hd help
pnpm run hd help test
pnpm run hd check examples/core.hd
pnpm run hd test spec/conformance/runtime/valid/defer-order.hd
pnpm run hd test test/std/text.hd
pnpm run hd examples/core.hd
pnpm run hd repl
pnpm run hd check --format json examples/core.hd
pnpm run hd explain unknown-data-field
pnpm run hd doc main examples/core.hd
pnpm run hd debug parse spec/conformance/parse/valid/layout.hd
pnpm run hd debug hir examples/core.hd
pnpm run check
```

### Commands

`hd`, `hd --help`, and `hd help` list the commands; `hd help COMMAND` (or
`hd COMMAND --help`) prints one command's usage and flags. `cli-args.ts`
holds the command table.

Each command is one function in `commands/`, such as `checkCommand` or
`testCommand`. It takes typed arguments and a `CommandIo` sink for its
stdout and stderr lines, and returns the exit status. No command writes to
the process's streams or sets its exit code. `cli.ts` is the thin wrapper:
it parses the arguments, calls the one function they name with the
process's streams, and returns the status for `bin/hd.js` to set. The
in-process conformance adapter calls the same `main` with a buffering sink
([`../test/portable/README.md`](../test/portable/README.md)).

```text
hd FILE  [-- ARGS]
hd build [--wat] [--release] [FILE]
hd run   [--release] [NAME] [-- ARGS]
hd test  [--update] [--filter PATTERN] [--seed N] [--cases N] [--shrink N] [FILE]
hd check [--tests] [--all] [FILE]
hd new   [--app] [--lib] [--vcs none] [PATH]
hd add NAME PATH@VERSION    hd update [NAME]    hd remove NAME    hd fetch
hd explain CODE      hd doc NAME [FILE|PKG]      hd def NAME [FILE|PKG]
hd repl              hd help [COMMAND]           hd debug parse|hir FILE
```

- Each command owns its flags. `--format text|json` is the only global flag,
  and it may come before or after the command. A flag given to a command
  that does not own it exits 101 and names the commands that do.
- Flags may come before or after the operands. The words after the first
  `--` are the program's arguments ([`cli.args.separator`](../spec/cli/command-line.md#r-cli.args.separator));
  only `hd run` and `hd FILE` take them, and a further word before `--`
  is an error that suggests `--`. No host capability reads them yet.
- A usage error prints to stderr and exits 101 ([`cli.exit.hd-failure`](../spec/cli/command-line.md#r-cli.exit.hd-failure)).
- `hd debug parse` and `hd debug hir` print internal compiler output.
  `hd parse FILE` stays as a hidden spelling of `hd debug parse`, because
  the conformance command contract names it
  ([Command Contract](../spec/conformance/README.md#command-contract)).
- `hd` has no flag for the conformance runner's options (runtime profile,
  scenario, pending function, test layout, package tree, package role).
  The runner passes them through the in-process adapter
  (`test/hd-adapter.ts`, `test/hd-in-process.ts`), which sets
  `CommandEnvironment.runner` for the command functions. No help text,
  usage error, or doc names them. The test layout and package tree stand in
  for a package's layout, which a real package takes from its `hd.toml`. A
  package role compiles FILE as `src/lib.hd` (library) or `src/main.hd`
  (root application) of a package that depends on each runner dependency,
  a source root holding `lib.hd`.
- **Package mode** ([Package Mode](../spec/cli/command-line.md#package-mode)):
  `commands/package-mode.ts` finds the nearest `hd.toml` above the start
  directory, FILE's directory or else the working directory. `manifest.ts`
  reads it: the `[package]` name, the `[[executable]]` tables,
  `[source] root`, which must be `src`, and the dependency tables. Any
  other table or key warns `unknown-manifest-key`. A manifest
  with `[workspace]` and no `[package]` is a workspace. At its root,
  `hd check`, `hd build`, and `hd test` act on every member in `members`
  order, and `hd run NAME` runs the one member program named NAME
  (`commandPackages` in `commands/source.ts`). `-p NAME` selects members,
  at the root or inside a member; with one member and no NAME, `hd run`
  chooses as in package mode, as `cargo run -p` does. The dependency
  commands work only inside a member. A package that an enclosing workspace
  manifest lists in neither `members` nor `exclude` is an error that names
  the manifest, with a fix-it for each array
  ([`cli.mode.member.unlisted`](../spec/cli/command-line.md#r-cli.mode.member.unlisted));
  one in `exclude` works on its own. A `src/` directory without `hd.toml`
  makes no package.
- **Executables**: with no `[[executable]]` table, `src/main.hd` is the
  default executable, named after the package. A table's `module` that names
  no module is `missing-entry-point`; a `src/main.hd` that no table names is
  `unlisted-entry`. A public `main` in a library module warns `unselected-main`.
  Every entry module is its own program, which no module may use.
- **Tasks** ([Tasks](../spec/cli/command-line.md#tasks)): each
  `tasks/NAME.hd` is a task, a program of its own whose relative lookup
  starts at `tasks/`, with shared task modules in its subdirectories. The
  linker gives task modules identities under `<tasks>`, which no use path
  spells; a `super` above `tasks/`, or a use of a task, is `unknown-module`.
  A task named like an executable is `duplicate-executable-name`, and a `tasks/x.hd` or
  `tests/x.hd` beside a directory `x/` is `invalid-module-path`.
- `hd check` and `hd build` without FILE work on the whole package
  (`compilePackage` in `commands/compile.ts`): each executable, each library
  module, with `--tests` each test module and integration test program, and
  with `--all` also each task, is the entry of its own link, unless an
  earlier link already joined it.
  `hd build` writes each executable to `build/debug/NAME.wasm`, or
  `build/release/` with `--release` ([`cli.build.output`](../spec/cli/command-line.md#r-cli.build.output)). `hd build FILE` writes FILE's module to `build/debug/files/STEM.wasm`
  ([`cli.build.output.file`](../spec/cli/command-line.md#r-cli.build.output.file)), and
  `--wat` prints the WAT. Outside any package `hd build` and `hd run` are
  errors, and `hd check` and `hd test` need a FILE.
- `hd run [NAME]` runs the executable or task NAME, or the package's one
  executable. `hd run FILE` and a directory word are errors. A task runs in
  its package directory, and an executable in `hd`'s working directory
  (`cli.run.cwd.task`, `cli.run.cwd.executable`).
- `hd test` without FILE checks the executables, then tests each module
  under `src/` and `tasks/` and each integration test program (a file
  directly under `tests/`), one link each, in path order, running only that
  module's test cases. An executable's or a task's entry module without a
  `tests:` block is skipped (`cli.test.tasks.no-tests`). An error in a
  module that several links join prints once. A run with no test case
  passes.
- A test build (`CheckOptions.testBuild`, set when the test code is linked)
  runs no entry behavior: a script with a `tests:` block initializes its top
  level requirement-free, and the error's note says to move the work into
  `main` (`module.init.tests.requirement-free`). A unit test case's row is
  `TestRunner` alone; a `missing-requirement` in its body notes the std fake
  for each missing host trait (`checker/test-tier-notes.ts`).
- An integration test program (a file under `tests/`, or the runner's
  integration layout) is checked with `integrationTest`: its test cases'
  rows take the default profile's traits and `std.process.Process`, and
  only it may call `hd_run!`, which is `test-only-use` elsewhere. `hd test`
  binds the default profile from the package directory, with no arguments
  and a closed standard input (`commands/test-host.ts`,
  [Test Environments](../spec/cli/command-line.md#test-environments)), and
  `Process` to the package's executables and tasks
  (`commands/processes.ts`): each `run!` starts `hd run NAME -- ARGS` in the
  package directory, and a name that names neither is `.Err(.NotFound)`
  ([`cli.test.process`](../spec/cli/command-line.md#r-cli.test.process)).
  Each run of a test body gets its own `temp_dir()`, an `mkdtemp`
  directory removed when the run ends. A user capability outside that
  profile is a `missing-requirement`; its note suggests
  `$.with(Repo=...)`.
- `hd test` never runs `main`, so neither its output nor its outcome counts
  as a test case. `hd test FILE` exits 101 with `FILE: no test case
  registered` when FILE registers no test case, even if it has an entry point
  ([`cli.test.file-empty`](../spec/cli/command-line.md#r-cli.test.file-empty)).
- **Doc tests** ([Doc Tests](../spec/lang/10-modules.md#doc-tests)):
  `doc-tests.ts` finds each fenced `hd` block in a `##` comment of a module
  under `src/`, names it `doc <module>.<item>[i]`, and writes its program:
  the block's leading `use` lines, then one top-level `it` whose body is the
  rest, with a map back to the `##` lines. `commands/doc-tests.ts` links
  each program at a fresh path under `tests/`, so it builds as an
  integration test program without the package's test code; the linker
  gives its module scope a package key of its own (`entryPackage`), so
  members without `pub` and traits it does not import are those of another
  package. `self` and `super` uses are `unknown-module`, and `hd_run!` is
  `test-only-use` (`CheckOptions.docTest`). `hd test` runs a module's doc
  tests with its test cases, those above its `tests:` block first, and one
  result line counts them all; `hd check --tests` checks all but the
  compile-fail ones, which `hd test` judges by their `# error: CODE`
  codes. A diagnostic or failure names the `##` line. An update run
  rewrites a failing `snapshot`'s `expect` in the block, matched by the
  failure's expected text, and runs the doc test again, at most 20 times.
- `hd build`, `hd check`, and `hd test` on a FILE under a package's `src/`
  or `tests/` link FILE with its package, so `pkg`, `self`, and `super`
  uses between modules resolve. Any other FILE compiles as a single-file
  program. The runner's package tree and test layout turn this off.
- `hd new` (`commands/new.ts`, [Creating A Package](../spec/cli/command-line.md#creating-a-package))
  writes `hd.toml`, `src/main.hd` or `src/lib.hd`, and `tests/NAME.hd` into
  PATH or the working directory, naming the package after the directory.
  With neither `--app` nor `--lib` it asks on a terminal
  (`CommandEnvironment.terminal`), and fails otherwise. It writes nothing when one of its files exists. Outside a
  git repository it runs `git init` and writes a `.gitignore` of `/build/`.
  `--pages` is hidden until `hd doc` exists (CLI-PAGES-HIDDEN). Under a workspace root it
  adds the new directory to the manifest's `members`, unless `members` or
  `exclude` lists it already, editing that array line by line.
- **Dependencies** ([Dependencies](../spec/cli/command-line.md#dependencies)):
  `manifest.ts` reads `[dependencies]` and `[dev-dependencies]`, and
  `dependencies/` does the rest. `requirement.ts` checks keys, host paths,
  and versions (`invalid-requirement`). `resolve.ts` runs minimal version
  selection over every reached version, from the package or from every
  member of its workspace, fetching what the cache lacks. It checks each
  manifest it reads against its `hd.sum` manifest line and each selected
  tree against its tree line, and links only what the package reaches.
  `git.ts` runs the system `git` (`ls-remote`, then a shallow fetch of the
  tag) with prompts off and credentials masked. A pseudo-version fetches
  every branch and tag, then checks out the commit its hash names, after
  checking its committer time. `cache.ts` keeps each
  version read-only under `HD_CACHE` or the user cache directory, with the
  tree hash it recorded under `hash/`. `sum.ts` reads and writes `hd.sum`.
  `hd check`, `hd build`, `hd run`, and `hd test` fetch implicitly and
  verify (`missing-sum-entry`, `sum-mismatch`, `unknown-version`,
  `fetch-failed`); `hd add`, `hd update`, `hd remove`, and `hd fetch`
  (`commands/dependencies.ts`) edit `hd.toml` line by line and write
  `hd.sum`. The linker (`package.ts`) joins each selected package's library
  under its own module keys, so `dep.NAME` sees only `pub` declarations.
  A path requirement names any local package, in a workspace or not; its
  manifest joins the selection, its dev dependencies and own `hd.sum` are
  never read, and it gets no `hd.sum` line. `hd.sum` lives beside the
  workspace manifest when the package is a member. `hd fetch` at a workspace
  root selects for every member (`resolveWorkspaceDependencies`); `hd add`,
  `hd update`, and `hd remove` run in a member. `hd add` that lowers a
  requirement prints `lowered NAME OLD -> NEW`. `hd add --dev` writes
  `[dev-dependencies]`; when a key sits in the other table, `hd add` asks
  the user to run `hd remove NAME` first. Removing a table's last key also
  removes its header. A dev dependency used from non-test code is
  `test-only-use`, whose message names the remove-then-add sequence. `hd clean`
  (`commands/clean.ts`) removes `build/` of the package or of each member;
  `hd clean --cache` (`clearCache` in `dependencies/cache.ts`) removes
  `pkg`, `hash`, and `tmp` of the cache directory, makes read-only entries
  writable first, and refuses the root, the home directory, and any
  directory with other entries. A pseudo-version's base tag
  must exist and be an ancestor of its commit (`git merge-base
  --is-ancestor`), as Go checks. A package under a workspace manifest that
  neither lists nor excludes it is an uncoded error at the manifest, with
  two fix-its (`package-mode.ts`). `hd test` passes the command's environment to the
  `hd run` that `hd_run!` starts, so it reads the same cache. Each linked module's scope names its
  package and the declarations its uses import, so the checker
  (`checker/package-ownership.ts`) applies the orphan rule across packages,
  hides a member without `pub` from every other module, makes a trait of
  another module available only where a use imports it
  (`checker/member-visibility.ts`), and warns on a
  per-trait `Self` line whose fact's package does not supply the trait.
  A REPL session links the dependencies and dev dependencies, and `hd def`
  and `hd doc` take `dep.KEY.ITEM` for a dependency's `pub` item.
- `hd FILE` (a first word that ends in `.hd` and names no command) runs FILE
  as a single-file program, linked with no package ([`cli.file.run`](../spec/cli/command-line.md#r-cli.file.run)).
  A `pkg`, `dep`, `self`, or `super` use in a single-file program is
  `unknown-module`; when FILE lies in a package, its note names the
  executable FILE is the entry of and its `hd run` command, or else
  suggests a task ([`cli.file.in-package`](../spec/cli/command-line.md#r-cli.file.in-package)).

`hd FILE` and `hd run` run the public `main` or `main!`; a module without one
runs its initialization and exits 0, while the adapter's `entry` runner option runs the
exported function it names and prints its result.
`hd check` skips the test cases and test-only functions of a `tests:` block
unless `--tests` is given (Testing T42); `hd test` always compiles them.
`hd test` (`test-runner.ts`) runs each test case in a fresh instance of one
compilation. The parser knows a registration call by the declaration it
names, not its spelling (`registrationOf` in `parser/test-cases.ts`): `it`,
an alias from `use std.testing.{it_each as each}`, or a member of a
`std.testing` namespace use, as `testing.it_each(...)`. It registers the
test cases in declaration order. It checks an `it_each`, `it_prop`, or `it_prop_with`
call's registration and makes its test case's body one call of an hd
function in `lib/std/testing.hd` (`each_case!`, `prop_case!`, or
`prop_with_case!`), and a `timeout` a call of `case_timeout`, after which a
timed `it` body runs as a closure through `run_case!`. These functions
reach the runner through the host capabilities `TestRunner` and
`PropertyRunner` (spec/std/testing.md#runner-capabilities), which the
checker puts in such a test function's row. The written body becomes a
closure with the empty row, so the runner's row never covers it
(spec/lang/10-modules.md#r-module.testing.unit-row). The runner passes one
answering provider for both traits (`AnsweringProvider` in `compiler.ts`).
An `it_each` table is one test function that the runner calls once per
row, as `name[i]`: `TestRunner.row` hears the row count and answers the
row; a failure names the test case. `report_timeout` hands the runner the
`timeout` in milliseconds, and it fails a test case whose call took
longer. It checks after the call returns, so it cannot stop a body that
never returns.

`hd` with no arguments starts an interactive session when standard input is
a terminal, and otherwise runs all of standard input as a single-file
program named `<stdin>` ([`cli.stdin.program`](../spec/cli/command-line.md#r-cli.stdin.program));
`hd repl` always starts a session. In package mode a session acts as code
inside `src/lib.hd`: its source joins the end of that file, the linker joins
the package modules it uses, and a diagnostic in another package file names
that file ([`cli.repl.package.lib`](../spec/cli/command-line.md#r-cli.repl.package.lib)).
In a workspace or outside any package it may use only `std`.

`hd repl` starts an interactive session. Each input is a declaration, a
statement, or an expression; expressions print their value and type. A line
ending in `:` starts a block, which an empty line ends. The session is kept as
one program (`:source` shows it): declarations at the top level and statements
in a synthesized entry point. Every input recompiles and reruns that program,
skipping console output already shown, so declarations cannot see REPL
bindings. `hd` and `hd repl` bind the default profile
([`cli.repl.host.default-profile`](../spec/cli/command-line.md#r-cli.repl.host.default-profile)):
the entry point is `pub fn main!()` whose row names `Console` and each trait
of `commands/default-profile.ts` under a session alias, so bang calls work.
A rerun answers the host calls of the inputs already accepted from their
record, so a file is written and the clock is read once
([`cli.repl.host.once`](../spec/cli/command-line.md#r-cli.repl.host.once)).
`read_line!` reads a line from the terminal, with raw mode off for the read.
The browser session binds no host, so its entry point stays
`pub fn main() -> void $ Console`. An expression's value shows as `dbg`
prints it ([`cli.repl.value`](../spec/cli/command-line.md#r-cli.repl.value)):
the session prints `std.format`'s hidden `dbg_text` of it, which the checker
lowers as it lowers a `dbg` call (see Debug Printing below). The `dbg` lines
of an input print before its value, as `debug` entries.
`:type EXPR`,
`:reset`, `:help`, and `:quit` are the commands. A pasted block, or a
website snippet, is one entry of several inputs. When a binding input in it
is rejected, its later inputs do not also report `unknown-name` for the
names it would have bound; a later entry still does. The session and its
commands live in `repl.ts` and its input rules in `repl-input.ts`; both run
in a browser too. `repl-terminal.ts` adds the terminal front end. The
website's REPL panel runs the same session in the playground's compiler
worker ([`../website/playground/README.md`](../website/playground/README.md)).
In a terminal the REPL colors the line being typed, printed values, and
`:source` output (`src/highlight.ts`), and colors errors and warnings. Set
`NO_COLOR` or `TERM=dumb` to turn coloring off; piped input is never colored.

The package also exposes `bin/hd.js` as the `hd` executable when installed or
linked with `pnpm link`.

## Agent Queries

hd is written mostly by coding agents, so the CLI answers questions about a
program by name and in JSON, not by file position
([roadmap](../future-work/ROADMAP.md#direction)).
Every command below also has the default `--format text`, which is the only
format a person needs.

### Machine-Readable Diagnostics

`--format json` is the one global flag. It follows
[Machine Output](../spec/cli/command-line.md#machine-output): `check`,
`build`, `test`, and the dependency commands (`add`, `update`, `remove`,
`fetch`) write JSON lines to stdout and nothing else, so a test body's
console output goes to stderr there. `run` and `hd FILE` write `hd`'s own
records to stderr, so the program's stdout passes through. `hd doc NAME`
still prints the prototype's symbol object (`CLI-DOC`).
The records are diagnostics, one test object per test case (`hd test`), and
a last summary object, written on success too. The `ok` and `N passed` text
lines and the `build` path are not written. Exit codes do not change.

```sh
hd check --format json app.hd > diagnostics.jsonl
```

```json
{
  "kind": "diagnostic",
  "code": "old-struct-declaration",
  "severity": "error",
  "message": "'struct' was replaced by 'data'",
  "file": "app.hd",
  "line": 1,
  "column": 1,
  "notes": [],
  "related": [],
  "fix": {
    "message": "replace 'struct' with 'data'",
    "edits": [
      {
        "span": {
          "start": { "line": 1, "column": 1, "offset": 0 },
          "end": { "line": 1, "column": 7, "offset": 6 }
        },
        "replacement": "data"
      }
    ]
  },
  "rule": "data.decl.no-struct",
  "rules": [
    { "id": "data.decl.no-struct", "anchor": "spec/lang/08-data-and-enums.md#r-data.decl.no-struct" }
  ]
}
{"kind":"summary","errors":1,"warnings":0,"passed":0,"failed":0,"ignored":0,"status":1}
```

(Each record is printed on one line; the diagnostic is spread out here to read.)

A test object is `{"kind":"test","name","outcome","message"}`, with `outcome`
`passed`, `failed`, or `ignored`, and the failure or ignore reason in
`message`. With `--format json`, `hd test`
runs every test case after a failure, and lists the objects in file path
order, then declaration order. The summary object counts `errors`,
`warnings`, and each outcome, and holds the command's exit `status`.

| Field | Meaning |
| --- | --- |
| `kind` | `diagnostic` for a compiler diagnostic, a panic while running, or a `Result`-returning `main` that returns `Err`. |
| `code` | The stable code from [spec/README.md](../spec/README.md#diagnostics) or the panic category; `null` when a `main` returns `Err`. |
| `severity` | `error` or `warning`. |
| `message`, `notes` | The prose the text format prints. |
| `file` | The source containing the position: in a package the path relative to the package root, outside one the path as written, or `lib/std/<module>.hd` for a standard-library diagnostic. |
| `line`, `column` | Primary position. Both are 1-based, and columns count UTF-16 code units. `null` for a failure with no source location, such as a panic the host cannot place. |
| `related` | Secondary positions as `{message, file, line, column}`. |
| `fix` | `{message, edits}` when the prototype knows the one correct edit, else `null`. An edit replaces its `span` (`offset` is the 0-based UTF-16 offset, and `end` is exclusive) with `replacement`; an empty span inserts and an empty replacement deletes. |
| `fixes` | Every suggested fix-it: `[fix]` when there is one, or the alternatives, as the two of an unlisted workspace member. |
| `rule` | The rule ID naming this code, when exactly one rule does; else `null`. |
| `rules` | Every rule whose text names this code, as `{id, anchor}`. |

Fixes are suggested only where the diagnostic's own message names the
replacement: `old-struct-declaration`, `old-import-declaration`,
`old-export-declaration`, `unexpected-bom`, and `missing-let`
(`diagnostic-report.ts`), and `type-mismatch` for `==` on an enum without `Eq`, which
inserts `@derive(Eq)` above an enum of the same file. In a package build, a
fix-it moves with its diagnostic into its file, and is dropped when an edit
lies in another file (`package.ts`, `locate`). A producer may also attach a `fix` or `related`
spans to a `Diagnostic` directly.

Rule IDs are not kept in a table. `spec-index.ts` reads the specification
each time a command needs it and finds each rule ID marker (`r[data.field.unique]`
opening a list item, paragraph, quote, or table cell, as
[Rule IDs](../spec/STYLE.md#rule-ids) defines it). A rule names a code
with "Error: `code`." or "Warning: `code`." or "is a `code` error". So as more
chapters gain rule IDs, `rule` and `rules` fill in without code changes.
`HD_SPEC_DIR` points the index at another specification directory; the
tests use it.

### `hd explain CODE`

`hd explain` prints what the specification says about a diagnostic code or
runtime panic category: its severity row, its normative meaning when the
[general-code table](../spec/README.md#diagnostics) has one, the rules that
name it, the chapter sections that mention it, and the conformance fixtures
that exercise it. It exits 1 for a code the specification never names.

```text
$ hd explain unknown-data-field
unknown-data-field: error (general)
  A data literal, pattern, or field access names a field the data type does not declare, ...
  (spec/README.md#diagnostics)

mentioned in:
  spec/lang/03-names-and-scopes.md#member-resolution  line 457
  ...

fixtures:
  spec/conformance/typing/invalid/data-literal-unknown-field.hd  type reject:unknown-data-field
  ...
```

`--format json` prints one object with `code`, `known`, `category`,
`meaning`, `meaningSource`, `rules` (`id`, `anchor`, `file`, `line`, `text`),
`mentions` (`anchor`, `heading`, `file`, `line`, `rule`), and `fixtures`
(`path`, `phase`, `expectation`, `specification`).

### `hd def NAME [PATH]` And `hd doc NAME [PATH]`

These resolve a symbol by name in a project. `PATH` is one `.hd` file or a
package directory with a `src/` tree, as in
[the package linker](../website/playground/README.md#packages-and-modules); it
defaults to the current directory.

- In a package, `pkg.user.User` names `User` in `src/user.hd` or
  `src/user/mod.hd`, and `pkg.User` names an item of `src/lib.hd`, including
  one it re-exports with `pub use`. The `pkg.` root may be left out; a name
  with no module path is searched in every module. In a single file the name
  is just the item path.
- After the item come member segments: `User.email` (field), `User.greet`
  (method from any `impl`), `Show.show` (trait method), `Show.Output`
  (associated type), `Status.Banned` (variant), and `Status.Banned.reason`
  (payload field). `Type::function` is accepted for `Type.function`.

`hd def` prints each match's location, kind, qualified name, and signature.
`hd doc` adds the doc comment, the fields, variants, associated types, and
methods (with the trait each method implements), and the traits a type
implements or the implementations of a trait. For a single file that
type-checks, results, requirement rows, and binding types the source omits
are filled in from the checker and marked inferred.

```text
$ hd def pkg.user.User project
project/src/user/mod.hd:2:1: data pkg.user.User
  pub data User
```

Lookups read parsed modules only, so they answer while a program still has
type errors. A module that does not parse reports its diagnostics (in the
chosen format) and is skipped. When nothing matches, the command exits 1 and
lists the qualified names that end in the query's last segment.

`--format json` prints `{"query", "symbols", "suggestions"}`. Each symbol has
`name`, `kind` (`function`, `data`, `enum`, `trait`, `binding`, `field`,
`variant`, `method`, `associated-function`, or `associated-type`), `module`,
`file`, `span`, `public`, `signature`, and `doc`, plus as they apply `owner`,
`trait`, `type`, `embedded`, `parameters` (`name`, `type`, `variadic`,
`default`), `result`, `requirements`, `suspending`, `omitted`, `hasDefault`,
`members`, `implementations` (`trait`, `target`, `file`, `span`),
`supertraits`, and `via` (the re-exported name the query matched).
Promoted members of embedded fields and blanket implementations are not
listed yet.

## Implemented Surface

- indentation-sensitive lexing with source spans and structured diagnostics;
- documentation-comment attachment on AST declarations and members, with
  orphan diagnostics;
- named arguments for statically resolved functions and methods, including
  suspending and dynamic trait dispatch, plus enum payload constructors, with
  source-order evaluation;
- requirement-free function-parameter defaults evaluated per call after all explicit
  arguments, including earlier-parameter references, erased generics, and
  suspending function construction;
- `List[T]` varargs (`values...: List[T]`), whose type is the collected
  list, with positional values, positional list spread, and named-list
  supply across ordinary, generic, suspending, static-trait, and
  dynamic-trait calls;
- function types whose inputs end in the rest element `List[T]...`, as
  `fn(i32, List[i32]...) -> i32`, so a function value keeps its vararg, and
  indirect calls using the same `List[T]` ABI;
- tuple types with a rest element `(A, List[T]...)`, whose value holds the
  fixed elements and then one `List[T]`; tuple expressions collect trailing
  elements into an expected rest element, or end in a list spread
  `(a, xs...)`; spread patterns `(a, xs...)` in `let`, `for` headers, and
  `match` arms. A rest tuple
  compares with `==` and `<` element by element, its rest element as its
  list, and `lib/std/format.hd` declares its `Debug` and `Display`, with the
  rest element's items written inline, for at most 11 fixed elements;
- tuple-typed and `Tuple`-bounded varargs (`args...: (i32, string)`,
  `args...: Args`), which take one tuple input: a call passes its trailing
  arguments as the tuple expression of them, and a `Tuple`-bounded type
  parameter is solved from that tuple;
- `Fn[Args, O, $ R]` and `SuspendFn[Args, O, $ R]` with `Args < Tuple`, whose
  type text is `fn(*Args)->O$R`: one erased input that holds the inputs
  tuple. Substituting a tuple for `Args` gives the plain function type,
  inference solves `Args` as the tuple of a function value's inputs, and a
  closure adapter unpacks the tuple into the actual parameters or packs
  them into it. Implementations over such a target match any arity, and
  their row parameter is solved from the receiver;
- a tuple spread into fixed parameters, as `add(pair...)` or `g(t...)`:
  the operand is evaluated once into hidden locals, must have the same type
  as the tuple of the remaining inputs (rest element included), and passes
  each fixed element, and its rest element's list at the vararg. Into
  `*Args` it passes the operand whole;
- a recursive-descent declaration/statement parser and Pratt expression parser;
- named functions, forward calls, typed parameters, typed results, and locals;
- source-ordered module bindings backed by typed Wasm globals, including
  function reads and reassignment of top-level `let` bindings, binding-point
  visibility, and transitive initialization checks through referenced
  functions and closures; a Wasm start function runs module initialization
  exactly once before either a script entry or a declared `main`;
- top-level `pub` visibility metadata for functions and nominal types, with
  private-signature leak checks and an MVP `Console` host-capability profile
  for public `main` entry points; named runtime profiles can admit explicit
  user trait capabilities;
- named `test` blocks retained in the AST and checked as active driver bodies,
  including the same explicit-discard and must-use rules as functions, and
  emitted as internal `__hd_test_N` Wasm driver exports for the harness;
- explicit generic call arguments with per-slot `_` inference for
  functions and inherent methods, where a short list infers its omitted
  trailing slots and a long one is `argument-count`, plus indented
  zero-argument trailing callback blocks. In an expression the list follows
  `::`, as in `first::[string](xs)`, `fetch!::[User](key)`,
  `Box::[i32] { ... }`, and `Add::[i32]::add(a, b)`; `[` after an operand
  always indexes, and `Box[i32] { ... }` or `Add[i32]::add` is `syntax-error`;
- type-argument defaults (`[T < Bound = Default]`) on functions, methods,
  data types, enums, traits, and `type` declarations, never on an
  implementation header (`syntax-error`). `type-defaults.ts`
  fills the slots a written type omits, with the earlier arguments and
  `Self` substituted (`Self` is the bounded parameter, the implementation's
  target, or the trait's own `Self`), before aliases expand; a written type
  that omits a slot without a default is `partial-generic-arguments`, and a
  trait value type whose default names `Self` is too. It also reports
  `default-order`, `binding-not-yet-visible`, a row default for a type
  parameter (`generic-kind-mismatch`), and, after implementations are
  prepared, a default naming a data type or enum that does not implement
  its bound (`unsatisfied-trait-bound`). A
  call, a generic function value, a data literal, and a call of a trait
  method through a bound apply a default only to what inference left
  unsolved (`applyGenericDefaults`), and an implementation method must
  repeat its trait method's defaults (`trait-method-signature`). The
  `std.ops` binary operator traits default `Rhs = Self`. Bounds on the
  parameters of a data type, enum, or trait are parsed, and the prototype
  checks them only against a default and, for `Eq & Hash`, as map keys;
- stored function fields remain callable through readonly data views and
  preserve their declared result permission;
- first-class `fn!` values for named suspending functions and capturing
  suspending closures, including ordinary construction, direct bang calls,
  provider forwarding, and one-way weakening to `fn(...) -> mut Suspend[T]`;
- the sealed prelude `Waker` trait is available as a dynamic value at the
  suspension boundary, including retention through ordinary functions;
- named local functions lowered through typed closure bindings, including
  enclosing captures, recursion, suspension, and requirement forwarding;
- the sized numeric types `i8` to `i64`, `u8` to `u64`, `f32`, and `f64`,
  `bool`, Unicode-scalar `char`, and UTF-8 `string` values
  (`src/numeric.ts`); an integer or float literal takes its type from the
  expected type and is range-checked; no number widens implicitly, so a
  wider type of the family is `type-mismatch` with a fix-it that writes
  `i64(x)`, a narrowing is `implicit-narrowing`, and mixing the families is
  `mixed-signedness`.
  Every integer of at most 32 bits is an `i32` at run time and `u64` an
  `i64` read as unsigned; arithmetic on the narrow types range-checks its
  result (`emitter/sized-numeric.ts`). Constructor-style casts such as
  `i16(wide)` wrap to the target width, a literal argument is range-checked
  against the target, and a float-to-integer cast saturates through the
  non-trapping `trunc_sat` instructions, clamping a narrow target in `f64`
  first (`types.cast.saturate`);
- heterogeneous tuple literals, tuple types, simultaneous tuple destructuring,
  and statically typed `._0` selection, stored in erased Wasm GC arrays;
- checked integer arithmetic and exponentiation at every width, IEEE `f32` and `f64` power, UTF-8 string
  concatenation, scalar and string comparisons, Wasm GC reference identity,
  boolean short-circuiting, and explicit panics;
- interpreted `$name` and `${expression}` string segments with left-to-right
  canonical `Display` dispatch for concrete and generic implementations
  (such as `lib/std`'s tuple `Display`, with its bounds' dictionaries), generic bounds,
  dynamic trait values, and the standard `string`, numeric, `bool`, and
  `char` implementations, which are hd code in `lib/std/format.hd`: an
  integer's digits come from an hd loop, a `char`'s UTF-8 text from an hd
  encoder over the `string_from_bytes` primitive, and a float's text from
  the host functions `format_f64` and `format_f32`; an `f32` shows its own
  shortest round-trip digits, while exact BigInt-backed host hooks parse
  decimal `f64` text and format fixed-point `f64` text with ties to even;
- `println` with the same display surface, statically requiring a
  lexical `Console` provider. `Console` is a prelude trait with
  `write_line!(mut self, text: string) -> Result[void, ConsoleError]`, so
  `$.use(Console)` is `mut Console` and a program may implement it. The
  host console is a `Console` trait value that boxes the host's `externref`
  and goes through the generic host capability bridge
  (`emitter/host-providers.ts`); its `write_line!` writes the line and is
  ready with `.Ok(())` on its first poll, so direct calls run on it and on a
  program-defined provider. `println` calls `write_line!` on the covering
  provider, the host console or a program-defined one, and drives the call
  with `block_on` (MHP-1). Under `main!` or a test body, `block_on`
  clears the outer driver's active flag, drives only its own suspension,
  and restores the flag (req.drive.block-on.inner-only), so `println`
  writes there. It is `suspension-forbidden-context` in a
  `defer` suite or a default expression. A call that returns `.Err` is an
  `explicit-panic` from `std`'s `panic`. A write pending on a host
  operation is polled again until it finishes, as the entry driver polls
  `main!`, since the prototype's host answers each poll itself. A script's
  top-level `println` is rejected with `missing-requirement`, because the
  prototype infers no script entry requirement row
  (`module.init.script-row`). A public non-suspending function with a
  host provider in its row is exported through a wrapper that makes the
  trait value from the host's `externref`;
- suspending host capability methods with scalar and UTF-8 string arguments
  and results, using opaque per-call tokens and a byte-stream bridge that keeps
  Wasm GC references inside Wasm;
- JSON-safe host-provider replay values with tagged integers, exact IEEE-754
  `f64` bits, and exact hex-encoded UTF-8 bytes;
- value-producing `if`, statement `if`, `while`, value-producing `while ...
else`, `break`, `break value`, and `continue`;
- list and insertion-ordered map `for` iteration with value-producing
  `for ... else`. A `for` loop or comprehension clause takes an irrefutable
  pattern: a name or a tuple of names binds directly, and any other pattern
  binds a hidden item that a one-arm `match` destructures (in a
  comprehension, through a one-element list per item), so a refutable one is
  `refutable-let-pattern` and the bare `for a, b in` is `syntax-error`;
- eager list and map comprehensions with ordered nested clauses, conditional
  filters, lexical clause bindings, and duplicate map-key replacement. A bang
  call in one is valid where it is valid in the loops the comprehension
  abbreviates; in a suspending body such a comprehension lowers to those
  loops, so each call completes before the next clause;
- right-associative single-name binding expressions with enclosing-scope
  visibility, readonly inferred bindings, and flow-sensitive initialization
  across short-circuit conditions. `[a, b := v]` and `(a, b := v)` end in a
  binding; a name list before `:=` inside an expression is `syntax-error`;
- pipe expressions `value |> step` with leading-`|>` continuation lines. The
  parser checks each step's own `_` placeholders, bare steps, and one-line
  steps; the checker binds the value to `_` and lowers the pipe to a one-arm
  match, so the value is evaluated first. A bare step `path` is the call
  `path(_)`;
- `let` bindings that infer the readonly view, and `let mut` bindings,
  per name in a parenthesized `let (mut a, b)` list, that infer `mut T`,
  reject a readonly value (`mutable-upgrade`), a readonly annotation
  (`let-mut-readonly-type`), or a primitive (`mut-on-primitive`), warn on
  a redundant `mut` before a name, alone or in a list, whose annotated type
  is already `mut` (`redundant-let-mut`, with a fix-it that deletes it), and use a
  non-generic data literal as `mut T`. `:=` binds one name: a pattern before
  it, such as `(a, b) := pair`, is `missing-let` with a fix-it that writes
  `let` and `=`, and the bare `let a, b` and `a, b :=` lists are
  `syntax-error`s whose fix-it adds the parentheses. A `let` takes any
  `match` pattern, with `mut` before each name it binds, and an optional
  `else:` block. A name or a tuple of names binds directly and `let _ = v`
  discards; any other pattern, or any `let` with an else block, binds a
  hidden item and hands the pattern's names to an ordinary `let` through a
  match, `let (a, b) = match item: P => (a, b); _ => else-block`. So a
  refutable pattern without else is `refutable-let-pattern`, an else block
  after an irrefutable one is `unreachable-match-arm`, and an else block
  that may complete is `let-else-falls-through`. A `mut` data subject
  matches as its data type, and a direct `mut U` field of a readonly
  subject binds as `U`. A primitive type
  written `mut`, as in `mut i32`, is `mut-on-primitive`. In a `mut self`
  method of an implementation for a primitive, `self` has the plain type,
  and a call, `Type::method` reference, or qualified call of it needs no
  mutable access;
- lexical branch and loop scopes;
- data declarations, literals, and field reads backed by Wasm GC structs,
  including requirement-free per-construction field defaults evaluated after explicit
  initializers and shallow copy-update with source-first evaluation;
- mutable data permissions with one-way `mut T` to `T` weakening, readonly
  aliases over shared identity, permission-aware direct and generic fields,
  mutable-path checking, and field assignment through Wasm GC `struct.set`;
- tagged enums, constructors, exhaustive matching, and payload bindings backed
  by Wasm GC structs, including shared constructor fields,
  requirement-free ordered defaults, per-variant factories, named or `._0` shared-field access, and
  canonical fieldless-variant identities;
- expected-type contextual enum constructors such as `.Ready(42)`;
- exhaustive boolean matching, guarded patterns, and literal matching for
  integers, floats, characters, and strings;
- tuple patterns, nested in any pattern, and exhaustiveness by
  pattern-matrix usefulness over bool, optionals, `Result`, enums, tuples,
  and data, and over integers split at every literal and range-pattern
  bound;
- range expressions (`a..b`, `a..`, `..b`, `a..=b`, `..=b`, `..`), checked
  as data literals of the `std.ops` range types; `for` over a range and
  `text[r]` or `items[r]` slicing go through the `Iterable` and `Index`
  implementations in `lib/std/ops.hd`. A range pattern binds the matched
  value to a hidden local and adds its bound tests to the arm's guard;
  coverage reads the pattern itself, so the guard does not weaken it;
- contextual enum patterns and recursive nominal data patterns with field
  bindings and literal field constraints, plus named enum-payload bindings
  resolved independently of source order and literal, nested-data, or
  nested-enum payload constraints;
- erased optional and `Result` values, contextual constructors, exhaustive
  matching, recursive nominal payload patterns, must-use checking, and postfix
  propagation; `Option[T]` is `T?`, with `.None`, `.Some(value)`, and their
  `Option.`-qualified forms as constructors and patterns over the erased
  carrier;
- named function values plus typed nested and recursive closures, expected-type
  parameter/result inference, result inference for nonrecursive closures, and
  GC environments for direct and transitive captures; a closure captures no
  provider from an enclosing `$.with`, so each key its body uses stays in its
  row and resolves at each call, and its `$.with` keys are compared for
  collisions only with its own row and its own `$.with` blocks (the declared
  row's keys name the enclosing generics); a provider value bound by `$.use` is
  captured like any local and outlives its scope; a captured `let` is a
  shared heap cell, and every closure may assign it and keep mutable
  captures (`mut fn` is a syntax error); a generic function used as a value is
  instantiated from explicit type arguments or the expected function type;
  function types convert by declared variance (permission changes only),
  are implementation targets owned by the standard library, reject a direct
  `is`, and may be spelled `Fn[...]` and `SuspendFn[...]` when imported
  from `std.function`;
- a data, enum, trait, primitive, or type parameter name where a value is
  required is `type-used-as-value`; a type alias name there still reports
  `unknown-name`, because aliases are expanded before checking;
- `type` aliases, generic ones included, expanded before checking, with
  `alias-cycle` for a cycle; row aliases (`type AppRow = $ Db + Cache`,
  generic and nested) expanded in every row, written after `$` in a row slot
  (`$.Context[$ AppRow]`, `Fn[(), void, $ AppRow]`, an explicit row type
  argument), and `generic-kind-mismatch` for one used as a type or single
  key, for a bare key or alias in a row slot, and (with a fix-it adding `$`)
  for a right side without `$`, as in `type AppRow = Db + Cache`; row parameters declared
  `$R` on functions, methods, implementations, and aliases, with
  `generic-kind-mismatch` (and a fix-it adding `$`) for an unmarked
  parameter in a row, a `$R` used as a type, or a `$` on a data, enum,
  trait, or newtype parameter, all reported by the parser except the row
  slot checks; newtypes lowered to one-field
  data types; `data`, `enum`, `trait`, `type`, and `impl` in a block suite,
  hoisted under a scoped name; the prelude traits `Any` and `Iterable` (user
  implementations and bounds drive `for` loops and comprehensions, and
  collections satisfy `Iterable` bounds); declared `+T`/`-T` variance with
  readonly variance conversions; row type arguments such as
  `Fn[(), void, $ Logger + Clock]`, with `generic-kind-mismatch` for a data,
  enum, or trait parameter used in a row; a dynamic trait value satisfying
  bounds on its own trait and supertraits, at the instantiation its trait
  arguments give each, through forwarding dictionaries;
- associated type bindings (`associated-bindings.ts`): a binding may name an
  associated type the trait reaches through its supertraits, and a name two
  reachable declarations share is `ambiguous-associated-type`, as is a
  projection `I::Item` that two bounds on `I` both reach, bound or not. A
  trait value type and a requirement key may bind associated types
  (`Supplier[Item = i32]`, `$ Store[Item = User]`), rendered
  `Supplier[Item=i32]` with the bindings after the positional arguments in
  name order, so two spellings are one type or key. A trait value type is
  dynamically safe only when it binds every associated type its trait
  reaches, and a key that leaves one unbound is
  `trait-not-dynamically-safe`. Through such a value each `Self::Item` is
  the bound type; a concrete value converts, and a provider installs, only
  when its implementation binds the same types (`type-mismatch`); widening
  keeps the bindings; and the value satisfies a bound on its trait with the
  projection equal to its binding. A binding on a type that is not a trait
  is `unknown-associated-type`, and one in an implementation header or a
  trait-qualified call is a `syntax-error`;
- concrete requirement rows with hidden `externref` provider threading;
- `+`-joined requirement rows (`$ A + B` in every position) normalized as
  sets, with `old-row-separator` for the former `$ A, B` and `$(A, B)` and
  `old-bound-operator` for a `+` between bounds, plus statically resolved `$.use`
  (including ordered multi-provider tuple lookup) and lexical `$.with`
  provider overrides;
- requirement-bearing closure types with invocation-time provider threading and
  least-row inference for requirements not satisfied by lexical providers;
- generic requirement-row parameters with least-row inference, symbolic and
  concrete row union, repeated-row consistency, removal of a key by row
  extension (`$ R + K` in the callback row, `$ R` on the callee), keyed Wasm
  GC provider packs, and lexical restoration of removed providers;
  `ambiguous-row-pattern` for a pattern with two unfixed row parameters and
  `row-parameter-in-context` for `$.Context[$ R]`; explicit row type arguments
  for a function's row parameters, as in `provide::[$ Db + Log](job)`;
- row subsumption: a function value with a narrower concrete row fits a
  wider function type through the callable adapter. A value is not widened
  into a row that holds a row parameter it lacks, which least-row inference
  solves instead. A list or map literal with no expected type gives its
  function values the union of their rows, and a spread contributes its
  list's element row: a part whose row is smaller is checked again against
  the union, a spread `xs` as `[for x in xs => x]` (RU12). Diagnostics
  print the union's expanded keys rather than the rows as written. `if`
  branches, `match` arms, and inferred closure and function results take
  the union too (RU15), and a branch or arm type mismatch is
  `no-common-type`. A function type whose result is a function type
  parenthesizes that result in its type string, as in
  `fn(bool)->(fn()->string$Db)`, so the inner row stays the inner
  function's;
- erased generic marker traits as provider keys, with call-site substitution
  and pre-erasure collision checking;
- erased generic functions with call-site type inference, Wasm GC boxing for
  primitive values, inference through optional and `Result` types, and
  higher-order callable adapters for erased type and requirement-row ABIs.
  Arguments that solve one type parameter, `assert_equal`'s two values
  included, must have one type up to `mut`: two numeric widths
  are `type-mismatch`, and a trait-value conversion `no-common-type`
  (`types.generic.infer.join`); a numeric literal takes the solved type.
  Boxing is a toy shortcut: the
  [implementation model](../spec/lang/04-type-system.md#shapes-and-generic-code)
  gives each value layout its own body and keeps values unboxed in generic
  code and containers;
- erased generic suspending functions whose GC frames retain boxed values,
  trait dictionaries, and providers across polls;
- simple traits and explicit implementations with signature validation,
  concrete method lookup, ambiguity diagnostics, static dispatch, and dynamic
  Wasm GC trait values carrying erased receivers and typed method references,
  including `mut self` enforcement through static, dynamic, default, and
  generic-bound dispatch;
- inherent `impl Type:` methods lowered to direct typed functions, including
  erased method-level generics with bounds, explicit or inferred type
  arguments, named arguments, mutable receivers, suspending calls, and
  duplicate-member diagnostics; `impl[T] Box[T]:` and `impl Box[i32]:` targets
  lower to erased generic functions whose target parameters come from the
  receiver, and one name clashes only when two targets unify;
- receiverless associated functions called through `Type::function`, including
  `Self` substitution, method-level generics, and suspending calls, inherent
  first and then the implemented traits (`ambiguous-method` for two);
  on a generic target, as `Box::[Point]::name()`, the target's arguments
  solve the implementation's parameters, which the call's arguments need
  not mention; `T::function()` on a type parameter calls through the
  bound's dictionary; `Trait::function()` infers `Self` from the arguments
  and the expected type, then calls that type's implementation, or the
  bound's when `Self` is a type parameter (`cannot-infer-type` without one);
- method references (`checker/method-references.ts`): `Type::method`,
  `Trait::method`, and `T::method` are checked as the closure that calls the
  member, receiver first, with type arguments written after the name or
  solved from the expected function type (a trait reference's `Self`
  included, also for an associated function such as `Factory::create`,
  which calls `Factory::create()`). `value::method` evaluates the receiver once and closes over it.
  A called reference is an ordinary call: `value::name(...)` is a method
  call, and `Type::method(receiver, ...)` calls the method on its first
  argument. `to_string` on a primitive calls its std `Display`. A
  non-suspending closure shares its enclosing function's generic parameters
  and bounds, and its environment keeps the bound dictionaries after its
  captures, so a `T::method` reference calls through the bound; a
  suspending closure still reaches no bound dictionary;
- blanket trait implementations over generic targets, with unified target and
  trait-argument inference; their adapters materialize static, dynamic, and
  bound dictionaries for ordinary and suspending methods; bounded blanket
  dictionaries capture nested dictionaries, including when forwarded from a
  caller or retained by a parent supertrait;
- embedded data fields (value embedding): promotion of `pub` fields and inherent methods
  (depth at most 3), with generic substitution through each embedded field; an
  embedded field follows its container's access, so through `mut C` a
  promoted field may be assigned and a promoted `mut self` method called;
  every fill copies (`Label: ...value`, copy-update, `place ...= value`), with
  one generated `$hd.copy_d<N>` per embedded data type that copies ordinary
  fields shallowly and parts recursively; a copy of a readonly value whose
  type has mutable edges is readonly (`mutable-upgrade` where `mut` is
  needed); trait conformance through a part is explicit delegation,
  `impl Trait for C by E`;
- default trait methods with target-specific lowering, dynamic method-table
  entries, and explicit override precedence; a default body calls through
  `self` only methods of its trait and supertraits (`unknown-method`);
- bound proofs deeper than 64 nested implementation bounds are
  `trait-resolution-depth`, including bounds of method-less marker
  implementations;
- generic supertraits substitute parent arguments through inherited calls and
  checked trait-value widening; child dictionaries retain blanket or concrete
  parent implementations;
- suspending trait methods with typed dynamic GC-frame wrappers, trait-bound
  dispatch, stored driving, and cancellation forwarding;
- erased generic parameters with independent GC dictionaries for multiple
  trait bounds, dictionary forwarding, method dispatch on values produced
  inside generic bodies, and concrete call-site recovery for returned `T`
  values;
- concrete and bounded generic `Eq` and `PartialOrd` dispatch: the compiler
  compares primitives, and calls every other type's implementation,
  generic ones too, passing the dictionaries of the implementation's
  bounds. `std.cmp` implements `Eq` for lists, optionals, `Result`, and
  maps, and `PartialOrd` and `Ord` for lists and optionals (`.None` first),
  in hd, and tuples through its tuple templates; the prelude uses
  `std.cmp`, so every program gets them. Primitives also satisfy `Eq` and
  `PartialOrd` bounds (floats included, with IEEE equality), and the ones
  without floats satisfy `Ord`; `PartialOrd < Eq` and `Ord < PartialOrd`, so
  each generated dictionary carries its supertrait's; primitives satisfy
  `Display` bounds and become `Display` trait values, through generated
  standard-library dictionaries;
- generic data declarations with inferred or complete explicit construction
  arguments, precise instantiated member types, and uniform `anyref` field
  erasure in one Wasm GC layout per declaration;
- generic enums with inferred and contextual construction, recursive
  instantiations, precise pattern bindings, and uniform `anyref` payload
  erasure in one Wasm GC layout per declaration;
- GADT-style variants (`checker/gadt.ts`, `checker/gadt-checker.ts`): a
  variant's own generic parameters and refined result, construction typed by
  that result, arm-local equalities from first-order unification with the
  subject (`impossible-gadt-pattern` for an arm that cannot unify, and
  exhaustiveness over inhabitable variants), invariance of a refined
  parameter, and existential bounds whose dictionaries the value stores in
  hidden fields (a let-else or `for` pattern is F-613);
- trait values as lexical providers, including dispatch after generic
  requirement-row packing and removal by extension;
- reusable `$.Context[...]` values backed by GC structs, `$.context` creation,
  and left-to-right context spreading into contexts and lexical scopes;
- stackless suspension frames with `fn!`, construction-time provider capture,
  direct and stored bang driving, explicit `mut Suspend[T]` bindings,
  child-pending propagation, local spilling, synchronous cancellation, and
  one-shot, competing-driver, and reentrant poll/cancel state traps;
- uniform Wasm GC `Suspend[T]` wrappers with concrete-frame poll, cancel, and
  boxed-result references, preserving identity through data fields, optionals,
  generic function parameters, and aliases;
- module-level single and grouped `use` syntax, with executable
  `std.task.block_on` support, a per-instance active-driver guard, and nested
  driver traps;
- in-process package linking (`package.ts`, used by the browser playground,
  and by the CLI for a package tree): `pkg`, `self`, and `super` uses between the modules of one
  package resolve to public declarations and `pub use` re-exports, and the
  modules reachable from the entry are joined into one program in
  initialization order. A `*_test.hd` test module joins as a `tests:`
  block, and a test build links every test module; `hd test FILE` parses a
  `*_test.hd` file as a test module, whose top level is test position. A
  file under `tests/` is the integration test module `tests.<path>`, which
  links like a test module: it uses the library through `pkg` (public
  declarations, library modules only) and other integration test modules
  through the `tests` root or `self`, whose base is the test root. Its
  scope hides the library's top-level names that it does not import. Files
  of one folder may use each other in a loop; a loop of folders is
  `folder-cycle`, reported once per tangle with one shortest folder loop,
  each edge's `use` line, the tangle size, and an `x.hd` to `x/mod.hd`
  fix-it; uses in test code make no folder edge
  (10-modules.md#dependency-cycles). Each `pub use` whose chain returns to
  a module it passed is `re-export-loop`, and so is a plain use through
  such a loop. Modules that use each other form one
  initialization group, joined by module identity after the groups it
  uses. The prototype does not order a group's statements by dependency
  (10-modules.md#order-inside-a-group), so a read that needs a later-joined
  module's binding is `top-level-read-before-initialization`. Linked
  modules share one top-level namespace. Each module's scope hides the
  other modules' top-level names that it does not import, so a bare one is
  `unknown-name`, `unknown-type`, or `unknown-trait` with a `use` hint.
  The names a std use binds stay in their module too: the use line joins
  once, and a module that binds a name to another std declaration than an
  earlier module does joins it under a hidden spelling
  (`../website/playground/README.md#packages-and-modules`);
- imported `std.resource.ResourceError[E]` as the canonical generic
  `Operation(E) | Disposed` enum, using the same erased Wasm GC representation
  as source-declared generic enums;
- literal suffixes (Literal Suffixes L1-L18, L11 with Decorators
  D9's names): `250ms` and `1.5kb` lex as one number with a suffix, and the
  parser desugars them to the call `ms(250)`; `-250ms` is the ordinary
  negation `-(ms(250))`. Radix literals take no suffix, and a reserved-word
  suffix such as `5else` is `invalid-token`. The checker resolves the
  suffix among module-scope functions only, requires a function marked
  `@num_suffix` (`FunctionDecl.numSuffix`, set after the std join by
  `checker/decorators.ts` from a value of `std.ops.NumSuffix`), and checks
  the ordinary call, so a generic or provider-needing suffix function
  follows the ordinary rules (`expr.literal-fn.ordinary-call`). The marker
  is a typed fact (see typed facts below), so the marked function's shape
  is checked at the decorator, by `num_suffix`'s signature in
  `lib/std/ops.hd`; an
  unmarked function at the literal is `invalid-literal-suffix`. `std.ops` and
  `std.time` (`Duration`, an `i64` count of milliseconds in the
  prototype's own `millis` field, and the suffix functions `ms`, `s`,
  `min`, and `h`) come from the [standard library](#standard-library), so
  a library suffix function works. A test `timeout` is passed to the
  `Duration` parameter of `std.testing`'s `case_timeout` and enforced after the
  body returns (see `hd test` above);
- string prefixes (Literal Suffixes L19): an identifier directly before
  `"` lexes with the string as one token (`Token.prefix`) whose text is raw
  but still interpolates. The parser desugars `x"a $b c"` to the call
  `x(Template { raw_parts: ["a ", " c"], values: [b] })`, naming the
  template type by its hidden name `__std_ops_Template`, which
  `checker/standard-library.ts` renames when the program imports
  `Template`. The checker resolves the prefix among module-scope functions
  only, requires `@str_prefix` (`FunctionDecl.strPrefix`, set with the
  suffix marker in `checker/decorators.ts` from a value of
  `std.ops.StrPrefix`), and checks the ordinary call, so each value
  converts to the template's `T` like an argument. The prefix shape is
  checked at the decorator, by `str_prefix`'s signature, as for suffixes;
  an unmarked function at the string is
  `invalid-string-prefix`. `std.text` declares `r`, `interpolate`, and
  `process_escapes` (which returns `Result[string, EscapeError]` with the byte offset of
  the bad escape) in hd; a `\u{...}` escape writes its scalar through
  `char`'s `Display`;
- `std.convert.From[T]` and `std.error.Error` as hd trait
  declarations in `lib/std/convert.hd` and `lib/std/error.hd`; the
  prelude uses `std.convert`, so every program declares `From`, and `?`
  finds it by its standard name whether or not the module imports it; `?` on a `Result` converts the error
  by one assignability rule or one `From` call, `Type::from(x)` selects the
  `From` instantiation by argument type, and a single-payload variant
  constructor is a function value (a dynamic trait value does not yet
  satisfy a bound on its own trait). A generic function or generic variant
  constructor passed as a call argument takes its type arguments from the
  call. While a result type is inferred, `?` converts nothing and its
  operand's error must match the inferred result;
- test bodies (Testing T4): a `test` block's result is inferred like a
  closure's and must be `void` or a `Result` with a `Display` error, else
  `unsatisfied-trait-bound`; a test fails when `report()` on its result
  gives a nonzero code, as for `.Err`. Importing `std.process.ExitCode` or
  `Termination` declares both from the
  [standard library](#standard-library) (Testing T8), with the implementations for
  `ExitCode`, `void` (whose `self` is a null `anyref`), and `Result[T, E]`;
  `main` and `main!` may return `void`, `ExitCode`, or a `Result` over them
  (a program's own `Termination` type is reported as not yet supported), and
  `hd run` exits with the code. For an `.Err` it writes the error's report
  to standard error and exits with 1 (module.entry.err-stderr): the checker
  adds a generated renderer for a `main` whose written result is a `Result`
  (`checker/entry-error.ts`), which gives the `Display` text, or
  `std.error`'s report with its `caused by: ` lines for an `Error` type, and
  the entry wrapper hands the text to the host through the `entry_error`
  import. A `Result` reached only through a type alias still reports
  `main returned Err`;
- typed derivation (spec/lang/14-annotations.md#typed-derivation, Typed
  Derivation M1-M29), lowered before checking by `checker/typed-derivation.ts`:
  decorators on data, enum, newtype, field, variant, payload, and function
  parameter declarations; the `+=` token; `@derive` of a trait with a
  `by Structure` template; derivation blocks with member lines (`=`, `+=`,
  `= pass`, `Self`); trait-less derivation blocks `impl T by Structure:`
  (M26), which `checker/member-lines.ts` checks and folds into the
  declaration facts of `T` before any derivation reads them, then drops;
  and the `std.structure` handles, facts, walkers,
  describers, and sources of `lib/std/structure.hd`, which the pass
  declares itself, since it runs before the std join (under hidden
  names when the program declares a type of the same name, as a `data Key`).
  A template is checked once (`annot.template.checked`): each method becomes
  a generic function over the template's `T`, bounded by a hidden trait that
  stands for `T`'s `Structure` in that template, with `hd_name`, `hd_facts`,
  and one method per `walk`, `describe`, or `build` call site. Each
  derivation implements that trait for its target, calling generated
  traversal functions specialized to the target and to the walker,
  describer, or source type, and implements the derived trait by calling
  the template's functions with the target as `T`. So a derivation adds
  only its traversals and handles, never a copy of the template body. The
  template must hold the walker, describer, or source in a local declared
  with its type (`unsupported-derivation` otherwise). A plain handle of a
  non-generic target is a module constant, and derivations of one target
  without member lines share their handles. `T::name()` returns the
  target's declared name (`annot.structure.name`). Inside a template,
  `Structure::f(...)` is `T::f(...)`, and a call qualified by the derived trait, as
  `Named::name()`, calls the target's own implementation
  (`annot.template.qualified-self`). A walker's `member` and `rest` may strengthen their bounds; their dictionary
  entries trap, since only generated code calls them, concretely.
  A tuple template, `impl[T < Tuple] Trait for T by Structure`
  (`annot.template.tuple.*`), is compiled the same way and instantiated
  once per tuple shape, its number of fixed elements and whether it has a
  rest element, as a generic implementation over the element types, such
  as `impl[hd_E0 < Eq, hd_E1 < Eq] Eq for (hd_E0, hd_E1)`
  (`checker/tuple-templates.ts`). Shapes need no inferred types: the pass
  reads them from the program as the std join will declare it, without
  std implementation bodies. It instantiates the program's own tuple
  templates for every shape, and std's (`Eq`, `PartialOrd`, `Ord`,
  `Hash`, `Debug`, `Display`, `Default`) for every one of those traits
  the program sees: the prelude's, and the ones it imports. A rest member is one
  `List[T]` member; its item type takes the bound of the walker's `rest`,
  or of the trait's `List[T]` implementation, and a shape with neither,
  as `Hash` has, gets no instance. Generated `walk` calls `rest` only on a
  walker that implements it, and `member` otherwise, as `Walker.rest`'s
  default does; the prototype's default body itself panics. A tuple's
  `build` returns `Self`, since a tuple takes no `mut`. A hand-written
  implementation of such a trait for a tuple type is `overlapping-impl`.
  A map key whose `Eq` is a generic implementation, as a tuple's is,
  compares through that implementation's dictionary, and the key bound
  `Map[K < Eq & Hash, V]` is checked through generic implementations and
  their bounds (`checker/map-keys.ts`). The checks
  of the chapter's diagnostics (`underivable-trait`, `misplaced-derivation`,
  `marker-template`, `invalid-member-line`, `duplicate-fact`,
  `omitted-member-without-default`, `member-not-derivable`,
  `generic-member-call`, `newtype-derivation-self`, `gadt-derivation`, the
  two warnings, the `structure-variant-mismatch` panic, and
  `suspension-forbidden-context` in facts) are implemented. Gaps: `Facts`
  holds `Inspectable` values rather than `Any`: each generated fact list
  erases its values through the checker intrinsic `hd__structure_fact`,
  which names any type, a function type by its text, so a user's
  `facts.items` may hold such a value; a fact's concrete type for `duplicate-fact` is read from
  syntax (a data literal or a call's declared result), for member lines and
  for declaration facts alike (M25); `VariantInfo.shared`
  is always empty; a build handle's `get` returns the declared type whatever
  its argument's permission; a newtype forwards only through the receiver
  and plain `Self`; `@derive(Eq)`, `PartialOrd`, `Ord`, `Hash`, and
  `Debug` instantiate the std templates in `lib/std/cmp.hd`,
  `lib/std/hash.hd`, and `lib/std/format.hd`, read from the std source as
  `Arbitrary`'s is. An enum orders by variant
  index first, so `PartialOrd` and `Ord` first walk `other` to read its
  index. A newtype derivation of a std trait whose methods all take
  `self` calls its base type's method through a bounded helper,
  and `mixed-derived-law` (`checker/derive-intrinsics.ts`) checks the law
  partners; `==` and `<` use a generic implementation such as a derived
  `impl[T < Eq] Eq for Box[T]`. `Debug`'s template makes the builder
  calls of Rust's mapping (Testing T53, T54): `debug_struct` for a data
  type, even a fieldless one, and a variant with named payload fields,
  `debug_tuple` for a variant with positional ones, and `write` of a
  payload-free variant's name. A variant that mixes both uses
  `debug_struct`, naming a positional field `_0`, `_1`, and so on. A first
  walk reads the variant and counts its positional members, so the
  template picks the builder before it writes a field; a member without
  `Debug` is `member-not-derivable`, reported once though both of the
  template's writing walkers require it. A field
  a derivation compares or hashes without the trait is
  `derive-field-missing-trait` at the field (at the base type for a newtype), and a use whose added bound
  fails is `missing-derived-bound`. `Debug` is a prelude trait; `DebugWriter`, its
  builders, the prelude `debug`, and `std`'s `Debug` implementations for
  the primitives, `List`, `Map`, `T?`, `Result`, and tuples are hd code in
  `lib/std/format.hd`, whose writer is always compact. `assert_equal`
  checks `Debug` through the implementations and their bounds, as a
  call's bound is checked. The drift and unused-fact warnings treat
  the module as one package, except the per-trait `Self` line warning,
  which reads the fact type's package, and the unused-fact warning skips a literal
  fact such as `@"note"` and a fact built by a name imported from `std`,
  such as `@annotate(.Field)` (M25). `@derive` before a function, trait,
  implementation, or method is `decorator-not-annotator`. Trait-less
  blocks follow M27-M29: the header must bind the declaration's parameters
  in order, under any names and without bounds; a second block for one type
  is `overlapping-impl`; a `Self` line's fact warns as unused when the type
  derives nothing; and a member line's right side may be any list-typed
  expression. A name bound by a module `let` to a list literal is inlined,
  so its elements keep their concrete types; any other list expression is
  spread into `Facts` as `value...`, which needs `Inspectable` elements. A
  right side whose type is known from syntax and is not a list, such as
  `name = 5`, is `invalid-member-line`. Two type-level decorators of one
  fact type are `duplicate-fact` on the later one. The per-trait `Self`
  line warning needs a second package, which the prototype CLI cannot
  load. The prototype cannot check `annot.traitless.module` across the
  modules of a linked package, which share one namespace;
- decorators as plain values (spec/lang/14-annotations.md#prefix-decorators,
  Decorators D1-D10), in `checker/decorators.ts`: a decorator before a
  function, trait, implementation, newtype, method, or method parameter
  attaches its value, which is checked as a compile-time expression like
  any fact, counted by `duplicate-fact`, and, on a module-level function,
  read by `facts_of(f).find::[M]()` (spec/lang/14-annotations.md#function-facts).
  `checker/function-facts.ts` gives each function that a `facts_of` call
  names a generated `Facts` builder; the checker lowers a call whose
  argument names that function, not a local, to the builder's call, and any
  other argument, or `facts_of` as a value, is `invalid-facts-of-target`.
  Importing `facts_of` declares `std.structure`, since each call returns
  its `Facts`; the written result in `lib/std/annotation.hd` is `Any`, so
  modules that only depend on `std.annotation`, such as `std.ops`, need not
  declare `std.structure`. A module-qualified argument is not supported,
  since the prototype compiles one module. A bare decorator name of a function
  with no parameters is rewritten to a call, before typed derivation for
  the program's own and imported functions and after the standard library
  is joined for `std`'s own decorators. The target-kind check runs after
  the join: it finds a fact type's `@annotate(...)` fact, recognizes
  `std.annotation.Annotate` by the qualified name the loader records
  (`DataDecl.standardName`), and reads the listed kinds from the written
  arguments, as the other fact passes read types from syntax. A value
  that a member line attaches is checked on that line. A newtype is a
  `.Newtype` target, and a value on a kind its limit omits is
  `decorator-target-kind` (D10);
- typed facts (spec/lang/14-annotations.md#member-typed-facts), in
  `checker/typed-facts.ts`, after trait-less blocks are folded in and
  before derivations read the facts: `@annotate::[F](...)` on a data type
  or enum makes it a typed fact type, its argument must be one of the
  type's parameters (`type-mismatch` at the decorator), and the pass then
  drops the argument. `lib/std`'s `annotate[T = Any]` takes the ordinary
  default, so `@annotate(...)` without one is an untyped fact type.
  A typed fact on any target but a field or a module-level function is
  `decorator-target-kind`. Each value `v` on a field, or on a derivation
  block's member line for one, or on a module-level function becomes
  `hd__typed_fact_N::[X, _](v)`, the call of a generated identity
  function over the fact type's parameters and bounds, so ordinary call
  checking does the `let f: D[X] = v` check: `X` is the expected type,
  the other slots are inferred, and `D`'s bounds are checked, all at the
  decorator or line. `X` is the field's written type or the function's
  signature type with its `!` and row. A generic target's bounded
  parameters become parameters of the fact's check function, with their
  bounds and the bounds' supertraits, and unbounded ones become `Any`; a
  derivation's facts function holds such a fact as its value alone, since
  it has no generic scope. A fact type of `std`, such as `NumSuffix` or
  `With`, is found from the `std` function that the value calls.
  `h.fact::[M]()` on a handle is `std.structure`'s `Field.fact`, which
  finds the member's fact of exactly type `M`; it does not check that
  `M`'s target argument is the handle's `F`. Each handle holds a hidden
  `hd_witness: Inspectable`, built by the checker intrinsic
  `hd__structure_witness::[F]()` from the member's type, whatever it is.
  When `F` has no runtime identity of its own, the checker reads that
  witness at the `h.fact` call as `F`'s Inspectable dictionary
  (annot.handle.fact.key), so `M`'s other parameters still need one. The
  witness is read from `h` again, so only a handle named by a local gets
  the exemption; a handle of a generic target whose parameter has no
  `Inspectable` bound names that parameter by its text;
- runtime type identity: importing a `std.inspect` name, as `std.error`
  does, declares `lib/std/inspect.hd` under its standard names: the sealed
  `Inspectable` (`std.error.Error` extends it) and `TypeId`, a data type
  holding the canonical printable name (an inner
  `mut` kept, the outer `mut` dropped, and a std declaration outside the
  prelude spelled by its qualified name, as `std.error.Error`); every
  inspectable type erases to `Inspectable` or `mut Inspectable` through a
  generated dictionary whose `runtime_type` builds that name, splicing in the
  names carried by `T < Inspectable` dictionaries, or by the `Inspectable`
  part of a bound whose trait extends it, such as `E < Error`; `downcast`, `downcast_mut`,
  `downcast_val`, and `TypeId::of` are checker intrinsics that compare names
  and unwrap the stored payload; `impl Inspectable`, a redeclared or
  implemented `runtime_type`/`downcast`/`downcast_mut`, and an Inspectable
  requirement key in a function's requirement clause are rejected. Not
  covered: `Hash` for `TypeId`, Inspectable keys in
  closure types and provider scopes, qualified printable names for package
  declarations (the key is the linker's joined spelling, unique per
  declaration but not its qualified name, F-620), and opaqueness (`TypeId { key: ... }` is
  constructible). A type parameter instantiated with `mut U` looks up
  implementations for `U`, and its Inspectable dictionary adds the inner
  `mut` when a composite key is built from it;
- error derivation (spec/lang/14-annotations.md#error-derivation), lowered before
  any decorator is resolved by `checker/error-derivation.ts`: it removes each
  `@error` form, and each `@from` and `@source` marker inside an error type,
  whatever a binding named `error`, `from`, or `source` means, and reports
  their placement (`decorator-target-kind`), arguments and cause members
  (`invalid-error-marker`), and overlaps (`overlapping-impl`).
  `checker/error-generation.ts` writes `impl Display`, `impl Error` with
  `cause`, and one `impl From[P]` per `@from` member as hd source with the
  generated bounds. Each message becomes a helper function whose parameters
  are the members it names, so `self` and unnamed shared data are unknown
  names there; a cause or transparent member goes through a bounded helper
  on a line with the member's span, so a member that is not an `Error` is
  `unsatisfied-trait-bound` there. Without `use std.error.Error`, the
  pass imports it under its hidden name; `From` keeps its hidden name
  unless the module imports it. The
  declared `std.error.Error` has `fn cause(self) -> Error?` with a `.None`
  default (spec/lang/09-traits.md#r-trait.error.cause); an optional trait value
  type such as `Error?` is a known type, and a value of `T < Trait` erases
  to `Trait` through the bound's dictionary;
- `std.testing.assert`, an ordinary hd function in `lib/std/testing.hd`
  whose failure panics with `assertion-failed` and the reason through
  `std`'s one panic primitive, the `panic` host function
  (module.testing.shows-reason); and
  `assert_equal` for supported scalar, string, tuple, list, optional, `Result`,
  and order-independent map values and for explicit nominal or bounded generic
  `Eq` implementations, with mandatory reasons and
  `unsatisfied-trait-bound` at types without `Eq`. The checker checks the call and lowers
  it to a call of `check_equal`, hd code in `lib/std/testing.hd`, whose
  failure panics with `assertion-failed`, the reason, and both values'
  `debug` text through the `panic` host function
  (module.testing.assert-equal-debug); `std.testing.snapshot` runs as
  a string `assert_equal` with a literal `expect` (no update run rewrites
  it), and `snapshot_file` is hd code in `lib/std/testing.hd` whose host
  function (`src/snapshots.ts`) compares the text with
  `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`, failing
  with `assertion-failed` (Testing T54) when it is missing or differs, except under
  `hd test --update`, which writes it (Testing T53); `Choices` and
  `Arbitrary` are hd code there too;
- `it_prop` and `it_prop_with` register one property test case, which the
  runner runs once per generated case in a fresh instance
  (`src/property-tests.ts`). Every `Choices` draw goes through
  `PropertyRunner.draw`, which records it. A generated case offers each
  draw from an xoshiro128** generator written in hd (`Xoshiro128` in
  `lib/std/testing.hd`, a `std.random.Random`), which `PropertyRunner.seed`
  starts from the run's seed and the case's index, and whose reach
  `PropertyRunner.size` grows from case to case. A failing case is shrunk
  by replaying shorter or smaller choice streams, and the report names
  the seed, the shrunk input's `Debug` text, and the shrunk stream.
  `cases`, `shrink`, and `hd test --seed N`, `--cases N`, and
  `--shrink N` cap the run. A case that `assume` discards does not count
  toward `cases`, and more than 10 × `cases` discards fail the property.
  The shrunk stream is saved, one draw per line, under
  `__regressions__/<module>/<test-slug>` (`src/snapshots.ts`) and
  replayed before new cases on the next run. `examples` run first, one
  case each: `PropertyRunner.start` answers which example to run, and the
  runner stops once it reports no more. Each case has a draw budget of 256
  draws, which `Choices` counts; once it has spent it, every draw returns
  its simplest value without recording a draw. `Choices.int[N < Integer]` draws through `i64`, so a
  `u64` above the largest `i64` is never drawn;
- `@derive(Arbitrary)` and `impl Arbitrary for T by Structure` for
  `std.testing.Arbitrary` instantiate the template in `lib/std/testing.hd`;
  the checker generates nothing for `Arbitrary` itself.
  The derivation pass runs before std is joined, so it reads the template
  and its `impl Source for ArbitrarySource` from the std source, with the
  program's names (`standardTemplate` in `checker/standard-library.ts`);
  the std join drops both, since they name `std.structure`, which the
  pass declares only for a program that derives. The source picks the
  first variant whose `self_ref` is not `.Required` as the simplest, at
  drawn index 0, and fails with `NoFiniteValue` when no variant is finite,
  which the template reports as the panic `"${T::name()} has no finite
  value"`. For a generic target, each type parameter a member uses gets
  the template's trait and its source's `member` bound, read from
  `lib/std/testing.hd`, so `Box[T]` gets `T < Arbitrary & Inspectable`
  (`std-testing.arbitrary.derive.params`). Its
  `member[F < Arbitrary & Inspectable]` draws a member with
  the generator of its typed `With[F]` fact, read with `h.fact`, or else
  with `F::arbitrary`. A member that fails either bound
  is `unsatisfied-trait-bound` at the opt-in, naming the member, rather
  than the `member-not-derivable` of other templates
  (`std-testing.arbitrary.derive.not-derivable`). `use
  std.testing.arbitrary` imports the std submodule
  `lib/std/testing/arbitrary.hd`, so `arbitrary.with` calls its `with`;
- `Member.self_ref` and `VariantInfo.self_ref` (`SelfRef`) are computed in
  `checker/self-ref.ts`. It also follows owner decisions that agree with
  the spec text: any use of the enclosing declaration counts, whatever its
  type arguments; another enum needs the enclosing type when all its
  variants do; an omitted member counts;
- the runner's test layout option (`test-module` or `integration`)
  compiles a file as a test module (the conformance Test Layouts); both
  layouts are test modules, since the prototype has no separate integration
  view;
- the runner's package tree option (the conformance Package Trees)
  links FILE, as the package path it names, with every `.hd` file under the tree,
  entered at FILE's module, and reports each diagnostic in the file it
  points into. Package dependencies (`package-cycle`) are not modeled;
- one suspension CFG lowering for every function containing child drives,
  including direct calls, nested expressions, call arguments, short-circuiting,
  branches, loops, match guards, propagation, comprehensions, and provider scopes,
  with scoped cleanup and cancellation;
- a frame-level poll ABI that returns readiness separately from the stored
  result, plus host-visible construction, poll, ready, cancellation, and
  invalid-state trace events;
- deterministic host pending fixtures with poll counts and GC-frame resumption,
  including a portable CLI scenario that cancels a root while a named nested
  frame is pending and checks its source-defined cleanup result;
- a `pending-gate` runtime profile that wraps an opaque host provider as a Wasm
  GC trait value, polls its suspending method, forwards cancellation, and
  verifies the original provider-backed cleanup fixture;
- scalar host-provider method arguments and results for `i32`, `f64`, `bool`,
  and `char`, with an opaque per-invocation token and provider poll replay that
  restores recorded readiness and scalar results without calling the live host;
- JSON-safe tagged scalar replay values, with exact IEEE-754 bit strings for
  `f64` values such as negative zero, infinities, and NaN;
- started-frame cancellation that cancels the active child before registered
  top-level cleanup, plus scalar development start/poll/cancel exports;
- suspension poll record/replay with function-name-based site identities that
  survive unrelated declaration insertion, source-derived function code
  identity, and argument/result and provider configuration checks, through the
  in-process compiler API (the CLI has no record or replay command);
- strings backed by Wasm GC byte arrays. The prelude string methods
  (`len`, `trim`, `lower`, `split`, `replace`, `starts_with`) are hd code in
  `lib/std/text.hd` over three byte primitives; `lower` and `upper` call
  the host through the generic host-function boundary
  ([Compiler/Library Boundary](#compilerlibrary-boundary)). Every
  host-boundary decoder keeps a leading U+FEFF;
- non-suspending `defer` on normal completion, return, break, and continue;
- the `std.ops` operator traits
  ([Operator Traits](../spec/lang/05-expressions.md#operator-traits)): an operator
  on primitive operands keeps its built-in code, and any other operand calls
  the left operand's implementation, found by the trait's qualified name
  (`HirTrait.standardName`), or its bound's through a supertrait. Compound
  assignment, `Index` and `IndexSet`, the callable-value traits `Apply`
  and `Update` (`v()` on a value that is not a function calls `apply`,
  with no argument, and `v() = x` calls `update` on a `mut` callee, a call
  of a declared function never being a place), supertrait bindings such as
  `Add[Self, Out = Self]`, and the sealed `std.num` traits `Num`, `Integer`,
  and `Float` follow the same path. The primitive implementations are
  bodiless `@intrinsic` methods, which compile as the
  inline operator on primitive operands ([Boundary Mechanisms](#boundary-mechanisms));
  the index traits of `List`, `Map`, and `string` are hd code whose
  bodies index directly.
  `m[k]` on a `Map` has type `V`, and a missing key panics with
  `index-out-of-bounds`; `m.get(k)` reads `V?`. Floating `%` calls the
  host's `rem_f64`, JavaScript's truncated remainder. Compound assignment
  `place op= value` stores `place op value` for every type, and an index
  place reads and stores its element. A newtype construction
  over an `AnyVal` base is readonly, and one over an `AnyRef` base carries
  its argument's permission, as unwrapping one does. `Num::from_i64` checks
  its range in `lib/std/num.hd`, and `"$x"` on `T < Num` reaches `Display`
  through the supertrait;
- homogeneous `List[T]` literals, indexing, `len()`, and mutable `push()`, `pop()`, `insert()`, `remove_at()`, `clear()` over
  a growable Wasm GC vector with erased backing storage, plus indexed
  replacement through `mut List[T]`;
- insertion-ordered `Map[K, V]` literals with duplicate replacement, optional
  indexed or `get()` lookup, `len()`, growable indexed insertion and
  `remove()` through `mut Map[K, V]`, and erased Wasm GC key/value storage.
  A key type meets the declared bound `Map[K < Eq & Hash, V]`
  (types.map-key.declared-bound) through non-generic `Eq` and `Hash`
  implementations, std's `Eq` and `Hash` for the primitives included, or a type
  parameter's own bounds; a `mut` key type is `invalid-map-key`. An `i32`-like scalar or a string key compares directly; any
  other key, `i64` and `u64` included, compares through a wrapper of its
  type's `Eq`; and a map over a type-parameter key
  (key kind 3) compares its keys through the bound's `Eq` dictionary, which
  the map holds as its key context;
- the prelude `Iterator[T]`, a `lib/std/iter.hd` data type whose private
  `step` closure `next` calls, built by `Iterator::from_fn`, with the
  adapters `filter`, `take`, `enumerate`, `map`, `fold`, and `collect` as
  its ordinary methods (`checker/iteration.ts`). A list or map `iter()` is
  `from_fn` over a closure that advances the built-in Wasm GC cursor
  (`$Cursor[T]`, a name source code cannot spell), so explicit and
  `for`-loop iteration share exhaustion, partly consumed cursor,
  replacement, and structural invalidation behavior. A `for` loop over a
  list or map advances the cursor directly, and one over a `mut Iterator`
  calls its `next`. The loader declares `Iterator` when a program names it
  or `Iterable`, or selects `iter`, `take`, `enumerate`, `fold`, or
  `collect`. A private field of a std type is hidden from code outside std,
  and a private member of a package type from code outside its module;
- `collect[C < FromIterator[T] = List[T]]` over the `std.iter` trait
  `FromIterator`, which is not a prelude name. `C` comes from an explicit
  type argument or the expected type, which reaches the operand of `x?` as
  `Result[T, E]` or `T?`, and otherwise from its ordinary type-argument
  default. `List`, `Map`, `Result`, and optional targets are hd code;
- typed HIR, readable WAT output, Binaryen validation, and V8 execution; and
- an implementation-neutral conformance gate tied to
  `spec/conformance/cases.tsv`, invoked through the public CLI by a concurrent
  TypeScript runner, including stable rejection diagnostics, Wasm runtime panic cases,
  complete prelude-name shadow protection, and non-fatal unreachable-code,
  unused-local, and variant-binding-name-mismatch warnings.

Selected runtime failures cross the development host boundary with stable
codes, including explicit panic, assertions, integer overflow and division,
invalid shifts, list bounds, iterator invalidation, and suspension driver/state
failures. Portable panic fixtures verify the declared code rather than
accepting an arbitrary Wasm trap.

A panic report names the program operation that panicked, as
`FILE:LINE:COL: CODE: detail`. The emitter puts a Binaryen debug-location
line, `;;@ sN:LINE:COL`, before each operation of program code that may panic
(src/emitter/panic-sites.ts); `lib/std` code has none, so a panic inside it
names the program's call. Binaryen turns the lines into a source map, never
into module bytes (src/wasm.ts). On a panic, the host reads the Wasm frames
under the panicking import from a stack trace and maps the innermost annotated
one to its site (src/panic-locator.ts). An `integer-overflow` at a type from
the `usize` literal default adds a note that names the binding and the fix.

The host boundary covers scalars, strings, and structural results
(optionals, tuples, lists, data with public fields, non-generic enums
without shared fields, and results with a structural success type), and
scalar, string, and `List[string]` arguments. Maps and generic enums remain
narrower than the language specification (`module.boundary.allowed`),
tracked in KNOWN_ISSUES.
`all!` calls are typed by their rule (each child a
`mut Suspend[X_i]`, the result `(X_1, ..., X_n)`), and `race!` calls by the
plain signature in `lib/std/task.hd`. Both drive one polling frame, a stored
suspension that `$hd.combinator` in `emitter/stored-suspension.ts`
implements: `race!` is hd code that drives the frame `race_frame` builds,
and the checker lowers each `all!` call to a drive of the frame
`all_frame` builds. Both builders are runtime primitives in
`lib/std/task.hd`. A `race!` with no tasks never completes.
There are no type packs: `...` in a type is only a rest element, and
`[Ts...]` is a `syntax-error`.
Interpolation and `println` report `unsatisfied-trait-bound` when the displayed type
does not implement the canonical prelude trait.

## Standard Library

The toy standard library is hd source in the top-level
[`lib/std/`](../lib/std/) directory, next to `src/` as in Zig, one file per
module: `std.annotation`, `std.cmp`, `std.collections`, `std.convert`, `std.error`, `std.hash`, `std.console`, `std.format`, `std.function`, `std.iter`, `std.num`, `std.ops`,
`std.option`, `std.process`, `std.random`, `std.resource`, `std.result`, `std.testing`, `std.text`, and `std.time`,
with a submodule in a subdirectory: `std.testing.arbitrary` is
`lib/std/testing/arbitrary.hd`. Two more files are declared by a
checker pass rather than joined: `std.structure` (`lib/std/structure.hd`)
by typed derivation, which runs before the join, and `std.inspect`
(`lib/std/inspect.hd`) by `checker/standard-traits.ts`. It
follows the specification's stdlib tier (`spec/std/`); open points are
in [Open Issues](../future-work/OPEN_ISSUES.md).
`checker/standard-sources.ts` reads the files.

The prelude is the file `lib/std/prelude.hd`, `std.prelude` to the loader,
which holds only `use` lines
([`module.prelude.fixed-uses`](../spec/lang/10-modules.md#r-module.prelude.fixed-uses)):

- its `pub use` lines name the specification's prelude table but for its
  `std.testing` row, as `pub use std.cmp.{Eq, PartialOrd, Ord, Ordering}`.
  Every module, user or std, has those names, as if it began with those
  lines. `std.core`, `ConsoleError`, and the `std.task` names are
  compiler-provided;
- the `std.testing` row, `it`, is the `pub use` of `lib/std/prelude/testing.hd`,
  `std.prelude.testing` to the loader. Only a program with test code
  (`Program.testCode`: a `tests:` block or a test module's top level) joins
  it, and so `std.testing`
  ([`module.prelude.test-only`](../spec/lang/10-modules.md#r-module.prelude.test-only)).
  `hd check` without `--tests` drops the test code, and the flag with it.
  `it` is compiler-provided; outside test code it is `unknown-name`;
- `checker/prelude-names.ts` reads `PRELUDE_NAMES`, which no declaration
  or binding may shadow, and each name's module, which a same-name `use`
  may repeat, from the `pub use` lines of both files;
- its private `use` lines name what the compiler calls without a `use` in
  the program: `std.convert.From` for `?`, the operator, index, call, and
  range types of `std.ops`, and the modules whose inherent methods on
  built-in types need no `use`
  ([`trait.own.inherent.std`](../spec/lang/09-traits.md#r-trait.own.inherent.std)):
  `std.text`, which also holds the string kernel, `std.option`,
  `std.result`, `std.collections`, and `std.num`;
- a program cannot `use std.prelude` or `std.prelude.testing`
  (`unknown-module`), since the specification names no such module.

`checker/standard-library.ts` joins a program's use graph into the one
module the prototype compiles:

- the graph holds `std.prelude`, `std.prelude.testing` for a program with
  test code, every std module that a `use` of the
  program reaches, and every std module that a `use` of a joined module
  reaches, transitively. A `use` reaches the module its path names, as
  `std.cmp` for `use std.cmp.{max}`, or a module it names itself, as
  `use std.text`. Nothing else adds a module: no name a program
  mentions, selects, or calls does;
- every joined module joins whole: its declarations, its implementations,
  and its inherent methods on built-in types, which only `std` sources may
  declare (`ImplDecl.standard`). Its templates and `std.structure`
  protocol implementations are left to typed derivation;
- a declaration is declared under the program's local name or alias when
  the program imports it, as in `use std.cmp.Reverse`, under its own name
  when it is a prelude name, and otherwise under a hidden name such as
  `__std_cmp_clamp`. A prelude name that the program binds to a declaration
  of its own, a `prelude-name-shadow` error, leaves the prelude's
  declaration under its hidden name;
- a `use` of a compiler-provided name in a joined module, such as
  `std.console`'s `use std.task.block_on` or `std.error`'s
  `use std.inspect.{Inspectable}`, becomes a program `use` under a hidden
  name, or under the standard name for `std.inspect`;
- a `use` that names a std submodule, as `use std.testing.arbitrary`,
  imports the module. The prototype has no module values, so call checking
  resolves a public function selected through it, as `arbitrary.with(gen)`,
  to the function's hidden name, `__std_testing_arbitrary_with`, after
  lexical value lookup. A local or captured `arbitrary` therefore remains
  the receiver;
- every added declaration keeps its physical `lib/std` source span for
  diagnostics. Its logical joined-program position is the `use` that reaches
  its module, or the program's span when only the prelude does.

A program that uses no std module therefore gets every module but
`std.error` and `std.resource`: the prelude reaches `std.testing`
through `it`, and `std.testing` reaches `std.process`, `std.random`,
`std.time`, and `std.testing.arbitrary`. Checker reachability keeps those
declarations available; emitter reachability excludes unused std code from
the final module.

What it provides:

| Module | Contents |
| --- | --- |
| `std.prelude` | `use` lines only: the prelude names as `pub use` lines, and the private uses of `std.convert`, `std.ops`, `std.text`, `std.option`, `std.result`, `std.collections`, and `std.num` |
| `std.convert` | `From`, which the prelude uses for `?` but does not re-export |
| `std.error` | `Error`, which uses `std.inspect.Inspectable` as its supertrait |
| `std.annotation` | `facts_of`, with an `@intrinsic("facts_of")` body that never runs: the checker lowers each call to a builder that `checker/function-facts.ts` generates. `Target`, `Annotate`, and `annotate`, which limit a fact type's target kinds |
| `std.hash` | `Hash` and `Hasher` (prelude names), and `Hash` for `string`, `bool`, `char`, and every integer type, and its tuple template; no standard hasher, which the specification does not name |
| `std.task` | `retry!`, and `race!`, which drives the frame of the `@intrinsic("task_race_frame")` builder; `all!`'s frame builder, `@intrinsic("task_all_frame")`; `block_on`, `all!` (which has no written signature), and `Waker` stay compiler-provided names of the module |
| `std.option` | on `T?`: `map`, `unwrap_or`, `ok_or`, `is_some`, `is_none`, `expect` |
| `std.result` | on `Result[T, E]`: `map_ok`, `map_err`, `ok`, `err`, `is_ok`, `unwrap_or`, `expect` |
| `std.collections` | on `List[T]`: `map`, `filter`, `first`, `last`, `reversed`, `sorted_by` (stable), `chunks`, `zip`, `view`; `ListView[T]` with `len`, `to_list`, `Index[i32]`, and `Iterable[T]`, which checks the list's length |
| `std.text` | on `string`: `chars`, `char_indices`, `bytes`, `slice`, `to_utf8`, `string::from_utf8` with `Utf8Error`, `is_empty`, `ends_with`, `contains`, `find`, `upper`, `trim_start`, `trim_end`, `strip_prefix`, `strip_suffix`, `lines`, `repeat`; `join`, `StringBuilder`; the prefix `r` and its helpers `interpolate`, `process_escapes`, and `EscapeError` |
| `std.iter` | the prelude `Iterator[T]` and `Iterable[T]`; `Iterator` with `from_fn`, `next`, and the adapters `filter`, `take`, `enumerate`, `map`, `fold`, and `collect`; `FromIterator` for `List`, `Map`, `Result`, and `T?`; `Iterable` for `List` and `Map` (not `Iterator`, which a loop advances directly) |
| `std.cmp` | the prelude `Eq`, `PartialOrd`, `Ord`, and `Ordering`; `min`, `max`, `clamp`, `Reverse[T]`; `Eq` for every primitive, `PartialOrd` for the numbers, `char`, and `string`, and `Ord` for the integers, `char`, and `string`: bodiless `@intrinsic` methods, the numbers' in `impl[N < Num]` and `impl[N < Integer]` blocks, except `string`'s, which compare bytes in hd; `Eq` for `List`, `T?`, `Result`, and `Map`, and `PartialOrd` and `Ord` for `List` and `T?`; the tuple templates of `Eq`, `PartialOrd`, and `Ord` |
| `std.num` | the sealed `Num`, `Integer`, and `Float`, implemented for every primitive number type; on `i32` and `i64`: `checked_*`, `wrapping_add`, `wrapping_sub`, `saturating_*`, `abs_diff`, `count_ones`, `leading_zeros`; on `f64`: `is_nan`, `is_finite`, `to_fixed`; `parse_i32`, `parse_i64`, `parse_f64`, `ParseNumberError` |
| `std.time` | `Duration` with `milliseconds`, `seconds`, `as_milliseconds`; the suffix functions `ms`, `s`, `min`, `h` |
| `std.console` | the prelude `Console` and `println`; `ConsoleInput`, and the recording `BufferConsole` with `new` and `output` |
| `std.process` | `ExitCode`, `Termination`; the host trait `Process` with `ProcessOutput`, and the deterministic `ScriptedProcess` |
| `std.random` | the host trait `Random`, which the default profile binds to the operating system's random source |
| `std.resource` | `ResourceError[E]` |
| `std.ops` | the twelve operator traits, `Index`, `IndexSet`, `Apply`, and `Update`, with the primitive implementations of the operator traits, bodiless `@intrinsic` methods in numeric-family blocks such as `impl[N < Num] Add for N` (`string`'s `Add` is hd), and the index traits' implementations for `List`, `Map`, and `string`; the four range types, `Iterable` for `Range` and `RangeFrom` of each integer type, and the slicing `Index` implementations for `string` and `List`, one per range type, generic over the integer type, as `impl[N < Integer] Index[Range[N]] for string`; `NumSuffix` and `num_suffix`, the literal-suffix marker; `StrPrefix`, `str_prefix`, and `Template`; `Default` and its standard implementations, and its tuple template (spec/std/ops.md) |
| `std.function` | the sealed marker trait `Tuple`, which the compiler implements for every tuple type; a `Tuple` bound passes no dictionary. `Fn` and `SuspendFn` have no declaration: the checker rewrites them to the `fn(...)` sugar |
| `std.format` | the prelude `Display` and `Debug`; `Display` for `string`, `bool`, `char`, and every number type; `DebugWriter` and the builders `DebugStruct`, `DebugTuple`, `DebugList`, `DebugMap`; the prelude `debug`; `Debug` for the primitives, `List`, `Map`, `T?`, `Result`; the template of `Debug`; the tuple templates of `Debug` and `Display` |
| `std.testing` | `assert`, `Choices`, `Arbitrary` (for the primitives, `string`, `List`, `Map`, `T?`, `Result`, pairs, and triples), `snapshot_file`, `RunOutput` and `hd_run!` over `Process`; the runner capabilities `TestRunner` and `PropertyRunner`; the case bodies of `it_each`, `it_prop`, `it_prop_with`, and a timed `it`; the private xoshiro128** generator `Xoshiro128`, a `Random`; the rest of `std.testing` is checked by the compiler |
| `std.testing.arbitrary` | `with` and the typed fact type `With[F]`, in `lib/std/testing/arbitrary.hd` |
| `std.structure` | `Facts`, `Member`, `VariantInfo`, `SelfRef`, the handles `Field`, `Variant`, `Key`, and `Members`, and the protocol traits `Walker`, `Describer`, and `Source`, with hidden fields for the compiler-supplied bodies; `Structure` and the traversals are compiler-provided |
| `std.inspect` | the sealed `Inspectable` and `TypeId`, with `Eq` and `Display` for `TypeId`; `downcast_val` and the `downcast` methods are compiler-provided |

The prelude `string` methods live in `std.text` too, and `lower` and
`upper` are backed by the host. Positions and lengths are byte offsets. A string index is the
`string-index` HIR node, a bounds-checked byte read (`$hd.string_get`).
A runtime `string` is a `$hd.string`, a window (`start`, `length`) on an
immutable `$hd.bytes` array, so `slice` and a string slice `text[a..b]`
share their bytes in constant time.
The string kernel is hd code in `std.text` too: string `+` and
interpolation compile to a call of `string_concat`, `==` and `!=` (and a
string match pattern, a string map key, and a `TypeId` comparison) to
`string_equal`, and `<`, `<=`, `>`, `>=` to `string_compare`. Every
program declares them, since the prelude uses `std.text`.
Prototype limits: no `wrapping_mul` or `Float` rounding methods, no `Set` (the specification does not define it,
and a map built in generic code has no key equality for a type-parameter
key, so a generic `Set.new()` could not create its map), and no host `ConsoleInput`; a `BufferConsole` records both direct
`write_line!` calls and, outside a driver, `println` (MHP-1). `test/std/*.hd` tests each module through `hd test`, and
the playground's `std` example uses several.

## Compiler/Library Boundary

The compiler should know the language, not the library. A capability
such as `fs` or `net`, or a string algorithm, belongs in `lib/std` hd code
plus, where it touches the outside world, a host-side function. It should
not need a HIR node, a checker case, or a hand-written WAT helper. This
section lists where the prototype still breaks that rule, and the plan.

### Boundary Mechanisms

There are two ways for `lib/std` to reach below hd code. Both are
prototype-internal: the specification has no syntax for a library to
declare a host function (a question in
RUNTIME_AND_LIBRARY.md).

1. **Intrinsic functions.** A `lib/std` function preceded by
   `@intrinsic("name")` is an ordinary declaration whose body the compiler
   supplies. The standard-library loader turns the line into
   `FunctionDecl.intrinsic` (`checker/standard-library.ts`). The same line in
   user code is an ordinary decorator whose value calls an undeclared
   `intrinsic` (`unknown-name`), so only `lib/std` can use it. The written body (`panic("intrinsic")`)
   type-checks and is never emitted. Calls are ordinary calls.
   [Standard Library Primitives](../spec/std/README.md#standard-library-primitives)
   lists every primitive `lib/std` may declare; any other needs the
   owner's approval.
   - A **runtime primitive** is a few Wasm instructions over the runtime's
     own value layout, listed in `emitter/intrinsics.ts`: today
     the string primitives `bytes_len`, `bytes_at`, `bytes_slice`,
     `bytes_concat`, and `string_from_bytes`,
     `char_from_scalar`, `char_scalar`,
     `list_truncate`, and the frames `task_race_frame` and
     `task_all_frame`. `list_truncate` shortens a list;
     `pop`, `insert`, `remove_at`, and `clear` are hd code over it.
   - Every other name is a **host function**, imported as `hd`
     `host:<name>` through one generic path. Scalars cross as Wasm numbers,
     and a `string` crosses as a host handle that `emitter/runtime/boundary.wat`
     copies byte by byte. The host looks the name up in
     `src/host-functions.ts` (today `string_lower`, `string_upper`,
     `format_f64`, `format_f32`, `parse_f64`, `format_f64_fixed`, and `panic`, which raises a checked runtime
     panic of a named category, such as `index-out-of-bounds`).
   - An **operation intrinsic** is a bodiless `@intrinsic` trait method,
     one per primitive operation, such as `Add.add` or `Eq.eq`
     (spec/lang/09-traits.md#intrinsic-methods). An implementation of
     such methods is a declaration with no code
     (`HirTraitImplementation.intrinsic`, `HirFunction.intrinsicMethod`).
     A numeric-family implementation such as `impl[N < Num] Add for N`
     stays one implementation whose target is the bare parameter; it
     matches a type only when the type is one that the parameter's sealed
     bound lists (`HirTraitImplementation.family`, read from std's `impl
     Num for T` lines), and coherence checks each implementation it
     stands for. A call on a primitive receiver, such as `x.cmp(y)`,
     is an `intrinsic-call` that compiles as the operator does, and an
     operator on primitive operands never searches a trait. A dictionary
     for a primitive, as a `[T < Ord]` call on `i64` needs, is an
     `intrinsic` builtin: one wrapper per type and method
     (`$tintrinsic`), written when first used. In user code,
     `@intrinsic` stays an `unknown-name` decorator, and the method's
     missing body is not checked further (`FunctionDecl.bodiless`).
2. **Host capability traits.** A capability is a trait whose methods are
   bang calls or plain calls (spec/11 and
   RUNTIME_AND_LIBRARY.md).
   A host-bound trait gets a provider value built by
   `emitter/host-providers.ts`: each method's call goes out through
   generic per-method `host_<trait>_<method>_*` imports with the same
   boundary values, and the host answers through one
   `hostSuspensionInvoke` callback keyed by trait and method name, with
   record and replay. A provider value the embedder passes may answer its
   own calls instead (`AnsweringProvider`), as the test runner's does. A
   plain method's call begins, polls once, and reads its result at once,
   so the host may never leave it pending. An `i64` crosses as a BigInt. A method may also return `Result[T, E]` with a
   scalar, `string`, or `void` `T`: the tag crosses first, then the active
   side's payload. An `E` that is not a boundary type, such as `ConsoleError`,
   can be named but not built, so the host may not report `.Err` for it.
   A `Result` with any other `T` crosses as a node tree, as
   `Process.run!`'s `Result[ProcessOutput, ProcessError]` does: the host
   answers `{ tag: "ok" | "err", value }`, and an enum value as
   `{ tag: "Variant", field: value }`. A `List[string]` argument crosses as
   its length, then each element's length and bytes (`src/host-arguments.ts`).
   The host console is a built-in entry, `Console.write_line`, in
   `HOST_PROVIDERS` (`src/host-functions.ts`); `UNRECORDED_PROVIDERS`
   keeps its calls out of record and replay, as before.

Adding a pure host-backed std function needs its `lib/std` declaration and
one entry in `src/host-functions.ts`. Adding a capability such as `FsRead`
needs its trait in `lib/std` and a host implementation behind
`hostSuspensionInvoke`; neither needs a HIR node or a checker case.

### Audit

The special cases found on 2026-09-28, grouped by where they live. "Done"
marks what this refactor removed.

| Area | Special case | Kind | Status |
| --- | --- | --- | --- |
| HIR | `string-length`, `string-transform` (`trim`, `lower`, `upper`), `string-split`, `string-replace`, `string-starts-with` | string library | Done: hd code in `lib/std/text.hd` on three byte primitives; `lower` and `upper` are host functions |
| Checker | `checkStringMemberCall`: `len`, `trim`, `lower`, `upper`, `split`, `replace`, `starts_with` by name | string library | Done: ordinary `impl string:` methods |
| WAT runtime | `string-split.wat`, `string-transform.wat`, `$hd.string_len`, `$hd.string_starts_with` | string library | Done: removed |
| WAT runtime, emitter | `$hd.string_concat`, `$hd.string_compare`, `$hd.string_slice`; the host `string_from_scalar` | string kernel | Done: `+`, interpolation, `==`, and order call hd functions in `lib/std/text.hd` over five byte primitives; a slice shares its bytes; a scalar's UTF-8 text is hd in `lib/std/format.hd` |
| Host glue | `string_transform_begin`, `_input`, `_output` imports, `trimWhiteSpace` | string library | Done: `trim` is hd code; case mapping goes through `host:` |
| HIR | `console-print` (`println`) | capability | Done: `println` is hd code in `lib/std/console.hd` |
| Checker | `println` by name | capability | Done: an ordinary std function; std may declare a prelude name (`FunctionDecl.standard`) |
| Emitter | `emitPrintln` (`$hd.println`) | capability | Done: `println` drives `write_line!` with `block_on` |
| Host glue | `println_pending`, `println_error` imports | capability | Done: removed; the panics are ordinary `std` panics |
| Checker | `Console` trait declared in TypeScript (`program-types.ts`); `ConsoleError` as a primitive type name (`shared.ts`, `context.ts`, `termination.ts`) | std declarations | Done for `Console`, declared in `lib/std/console.hd`; `ConsoleError` remains, see Console below |
| Emitter | `emitConsole` (a hand-written host `Console` provider), `console.wat` (`$hd.console_print`) | capability | Done: the generic capability bridge, with `Result` results |
| Host glue | `console_byte` import | capability | Done: `Console.write_line` in `HOST_PROVIDERS`, left out of record and replay |
| Checker | `validateHostCapabilities` skipped `Console` | capability | Done: `Console` passes the same boundary check as any host capability |
| HIR | `assert` | `std.testing` | Done: `assert` is hd code in `lib/std/testing.hd` over the `panic` host function (migration M4) |
| HIR | `assert-equal` | `std.testing` | Done: the compiler checks an `assert_equal` or `snapshot` call and lowers it to a call of the hd `check_equal` |
| HIR | `snapshot-file` | `std.testing` | Done: `snapshot_file` is hd code in `lib/std/testing.hd` over the `TestRunner` capability |
| HIR | `each-row-index`, `each-row-count`, `test-timeout` | test runner hooks | Done: `it_each`, `it_prop`, `it_prop_with`, and `timeout` run hd functions in `lib/std/testing.hd` (migration M3) over the host capabilities `TestRunner` and `PropertyRunner` (task #201) |
| HIR | `debug-render` | `std.format` | Done: `debug`, `DebugWriter`, and its builders are hd code in `lib/std/format.hd` |
| Checker | `@derive(Debug)` generator (`deriveDebug`), builtin `debug` dictionary that wrote nothing, `implementsDebug` | `std.format` | Done: `impl[T] Debug for T by Structure` and `Debug` for `Map` are hd in `lib/std/format.hd` (migration M7) |
| HIR | `list-*`, `map-*`, `iterator-next` | built-in `List` and `Map` | Remains: the collection types are built into the runtime layout |
| HIR | `inspect-type-id`, `inspect-downcast` | `std.inspect` | Remains: runtime type identity is a compiler service |
| Checker | `block_on`, `all!`, `race!`, `facts_of`, `downcast_val` | spec-named intrinsics | Remains: the specification names them compiler intrinsics. `race!` is hd code in `lib/std` over the `task_race_frame` runtime primitive, and `facts_of` is declared there, so only its `@intrinsic` name is known; `all!` has no written signature, so the checker types it by name and lowers it to a drive of the `task_all_frame` primitive's frame. `facts_of` lowers to a call of a generated hd builder over `std.structure`'s `Facts`, with no HIR node |
| Checker | `Duration` for test `timeout` | `std.time` | Done: `case_timeout` in `lib/std/testing.hd` takes the `Duration` (migration M3) |
| Checker | `ExitCode` and `Termination` for entry results (`standard-traits.ts`, `termination.ts`) | `std.process` | Remains: language hooks that name a std type; the declarations are already hd |
| Checker | `Display`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Iterable`, `Any`, `Debug`, `Ordering` declared in TypeScript | prelude declarations | Done, except `Any` (`std.core`) and `Waker` (`std.task`), which have no `lib/std` file: the rest are hd in `std.cmp`, `std.format`, and `std.iter`, which every program joins through `lib/std/prelude.hd` (migration M2) |
| HIR | `display`, with the emitter's `emitPrimitiveDisplay`, the built-in `Display` dictionary, four `runtime.wat` digit and `char` helpers, and `float.wat` | `std.format` | Done: `Display` for `string`, `bool`, `char`, and the numbers is hd in `lib/std/format.hd`; a `char`'s and a float's text come from host functions (migration M6) |
| Emitter | the `pow_f64` and `rem_f64` imports | `**` and floating `%` | Remains: operator support |
| Checker, emitter | `Eq`, `PartialOrd`, and `Ord` dictionaries for the primitives built from the operator strategies (the `equality`, `ordering`, and `total-ordering` builtin kinds and their adapters), and a map key's primitive `Eq` | `std.cmp` | Done: `@intrinsic` methods in `lib/std/cmp.hd`, so `(1).cmp(2)` is an ordinary method call; `==` and `<` on a primitive still lower inline |

Counts: the HIR expression union had 92 kinds, of which 15 were library-
or capability-specific. The string step removed 5, the `println` step 1,
the `debug` and `snapshot_file` step 2, the `assert_equal` step 1, and the
test-runner hooks 3, `assert` 1, and the primitive `Display` step 1
(`display`, not counted among the 15), leaving 78 kinds, 2 of them
specific: the `std.inspect` row above. No capability has a HIR node now.

### Console

`println` is hd code in `lib/std/console.hd`
([`module.prelude.println`](../spec/lang/10-modules.md#r-module.prelude.println)).
It calls `write_line` without `!`, which makes a stored suspension, and
drives it with `std.task.block_on`, so it inherits all of `block_on`'s
rules with no checker case of its own
([MHP follow-ups](../future-work/OPEN_ISSUES.md#mutable-host-providers)).
Its `.Err` panic is an ordinary `panic` call. `block_on` is a
compiler-provided name that `lib/std/task.hd` does not declare: the loader
keeps a std module's `use std.task.block_on` line as a program `use` under
a hidden name, so the call is an ordinary `block_on` call. The prelude
re-exports `println`, so every program declares it under its own name.

The compiled module is the entry module, so its top-level statements may
call `block_on` and `println`
([`req.drive.block-on.forbidden-contexts`](../spec/lang/11-requirements-and-suspension.md#r-req.drive.block-on.forbidden-contexts)
forbids only non-entry module initialization). A linked package shares
one namespace, so the prototype cannot reject a driver in another
module's initialization.

The host console is a built-in entry of the generic capability bridge.
Its calls stay out of record and replay (`UNRECORDED_PROVIDERS`), as the
[Mutable Host Providers](../future-work/OPEN_ISSUES.md#mutable-host-providers)
decisions say, so a replay through the compiler API prints console lines
again rather than reading them back.

What remains:

1. `ConsoleError` stays a TypeScript type name until its variants and
   constructor are settled with the other std error types.

### Debug Printing

`dbg` ([Debug Printing](../spec/lang/10-modules.md#debug-printing)) is
declared in `lib/std/format.hd` as the ordinary
`@intrinsic("dbg") pub fn dbg[Args < Tuple](values...: Args) -> void`, so
the plain call rules check each call and its type is `void`. Only its body
is intrinsic: the emitter's body prints nothing, and `checker/debug-print.ts`
swaps in the printing body for the user's own code. That body depends on
each argument's static type and source text, so a program that calls `dbg`
checks in two passes (`checkWithDebugPrinters` in `checker/program.ts`):

1. The first pass checks a call as that ordinary call and records the static
   type of each element it collected into `Args`. A type with `Debug`
   in scope prints through it; a type parameter without it prints `<T>`; a
   function, a suspension, or a trait value prints as text; any other type
   prints structurally, and the pass records which of its parts implement
   `Debug`.
2. Between the passes, `debugPrinters` writes one hd printer function per
   structural type, `hd__dbg_show_N(value, out)`, over the private layout
   methods of `std.format`'s `DebugWriter`. They are marked
   `privateAccess`, so they read private fields. The second pass checks
   each call as `dbg_done` of `dbg_one(head, x, hd__dbg_show_N)`,
   `dbg_shown(head, x)`, or `dbg_opaque(head, x, text)`, which print through
   the host function `dbg_write`; `head` is the call's location and the
   argument's source text, and `dbg_done` drops the printed values, so the
   call is `void`.

A program without a `dbg` call checks once and links no printer. The
writer's limits (100 entries, 10 levels, 1000 characters, `<cycle>`) and
its pretty layout apply only to `dbg`'s writer, never to `debug`. A release
build reports `dbg-in-release`, whose fix-it deletes the statement, in the
first pass. A call in a
module whose scope the linker marks `fetched` (a dependency fetched for a
version requirement) stays the ordinary call, which prints nothing, and
`dbg-in-dependency` warns once per package. `dbg_write` reaches `InstantiateOptions.debugOutput`:
standard error for `hd run`, the test case's kept lines for `hd test`, the
REPL's `debug` entries, and the playground's marked output lines.

## Layout

- `lexer.ts` and `ast.ts` define the small source-frontend stages.
- `parser/` builds the AST and exposes its public API from `parser/index.ts`.
- `checker/` resolves names and produces the typed nodes in `hir.ts`.
- `emitter/` lowers HIR to readable WAT and exposes only `emitter/index.ts`.
- `checker/standard-sources.ts` reads the toy standard library's hd
  sources from the top-level `lib/std/`; `checker/standard-library.ts` joins
  the modules of a program's use graph into it.
- `suspension.ts` lowers suspending HIR into explicit resumable control flow.
- `wasm.ts` parses, validates, and emits Wasm with pinned Binaryen. It
  imports Binaryen on the first assembly, which costs about 200 ms.
- `compiler.ts` exposes the in-process compiler API. `compileToWat` is
  synchronous and stops at WAT: it parses, checks, lowers to HIR, and emits
  WAT, and never loads Binaryen. `compileToWasm` is asynchronous and adds
  the Wasm assembly; `instantiate` builds on it. No setup call comes first.
- `package.ts` links the modules of a multi-file package into one program.
- `cli.ts` turns a command line into one call to a command function;
  `cli-args.ts` holds the command table, flag parsing, and help text.
- `commands/` holds the command functions, one per command, and exposes
  them from `commands/index.ts`: `compile.ts` has `parse`, `check`,
  `debug hir`, and `build`; `execute.ts` has `run` and `test`;
  `queries.ts` has `explain`, `def`, and `doc`; `help.ts` has `help` and
  `repl`. `source.ts` finds and links a FILE's package for them.
- `diagnostic-report.ts` writes diagnostics as text or JSON Lines and derives
  suggested fixes.
- `spec-index.ts` indexes rule IDs, diagnostic codes, and fixtures from the
  specification sources.
- `symbols.ts` resolves name-addressed symbol lookups over parsed modules.
- `toolchain-gate.ts` proves the required Wasm GC operations independently of
  the language frontend.
- `../test/portable/cases.tsv` selects portable `.hd` conformance fixtures;
  `../test/run-portable.ts` runs them through the `hd parse`, `hd check`,
  and `hd test` command lines. By default `../test/hd-adapter.ts` runs those
  in-process on worker threads; `--compiler` spawns a command instead.
- `../test/cli.test.ts` exercises the CLI commands end to end,
  `../test/cli-commands.test.ts` their help output and flag errors, and
  `../test/agent-tooling.test.ts` the JSON diagnostics, `explain`, `def`, and
  `doc`. They call `main` in-process through `../test/hd-in-process.ts`;
  one test in `cli.test.ts` starts `bin/hd.js` itself.
- `main` and the command functions take an optional `cwd` and `specDir`
  (`CommandEnvironment` in `commands/io.ts`). Unset, they are the process's
  current directory and `HD_SPEC_DIR`; a test sets them per call.
