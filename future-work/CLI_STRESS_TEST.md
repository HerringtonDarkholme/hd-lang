# CLI Stress Test

Status: design review, 2026-10-02; changes no decision, design record,
spec text, or prototype code. It tests the CLI tier,
[Command Line](../spec/cli/command-line.md), and the package rules it
depends on in [Modules](../spec/lang/10-modules.md):
[Package Manifest](../spec/lang/10-modules.md#package-manifest),
[Workspaces](../spec/lang/10-modules.md#workspaces),
[Root Files](../spec/lang/10-modules.md#root-files),
[Test Modules](../spec/lang/10-modules.md#test-modules),
[Relative Uses](../spec/lang/10-modules.md#relative-uses),
[Single-File Programs](../spec/lang/10-modules.md#single-file-programs), and
[Folders](../spec/lang/10-modules.md#folders). Nothing here is accepted
behavior. Task #173.

## Contents

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Comparison With Other Tools](#comparison-with-other-tools)
- [Problems, Ranked](#problems-ranked)
- [Parse Log](#parse-log)

## Surface Being Tested

The report assumes exactly the spec text at commit `7954df11`:

| Area | Rules assumed |
| --- | --- |
| Modes | Nearest `hd.toml` decides package mode, workspace mode, or outside ([`cli.mode.package.nearest`](../spec/cli/command-line.md#r-cli.mode.package.nearest), [`cli.mode.workspace`](../spec/cli/command-line.md#r-cli.mode.workspace)). The start directory is FILE's directory, else the working directory. |
| `hd`, `hd FILE` | `hd` opens the REPL. `hd FILE` always runs FILE as a single-file program that may use only `std` ([`cli.file.run`](../spec/cli/command-line.md#r-cli.file.run)). |
| `hd run` | Runs an executable or a task by name; bare `hd run` needs exactly one executable. `hd run FILE` is an error. |
| Executables | `[[executable]]` tables with `name` and `module`. With no table, `src/main.hd` is the default executable, named after the package. |
| Tasks | `tasks/NAME.hd` is a program; subfolders of `tasks` hold shared modules reached through `self`. Tasks may use dev dependencies. |
| Build and check | In a package, `hd build`, `hd check`, `hd test` act on the whole package, or on one FILE's module. Outside, `hd check FILE` and `hd test FILE` treat FILE as a single file, and `hd build` is an error. |
| Workspaces | A workspace manifest lists members and declares no package. At its root, `check`, `test`, `build` act on every member; `hd run NAME` needs exactly one member with NAME. |
| `hd new` | Writes `hd.toml`, `src/main.hd`, and `tests/`, with no `[[executable]]` table. |
| Roots | `src/lib.hd` is the only library root; `src/main.hd` is its own program and not importable; `self` is the current module; `x.hd` is in the folder of `x/`. |
| Dev dependencies | `[dev-dependencies]` serve test code and tasks only. |

Where the spec is silent, the examples assume:

- **No program arguments, files, or processes.** No host trait for
  arguments, the file system, or child processes is specified
  ([OPEN_ISSUES](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work):
  "the complete standard host capability-trait catalog"). Blocks that need
  one name a `std.fs` or `std.process` item and are marked hypothetical.
- **The default profile.** Every program runs with the default runtime
  profile, which binds at least `Console`
  ([`module.profile.default`](../spec/lang/10-modules.md#r-module.profile.default)).
- **No machine output.** `--format json`, exit codes, and test filters
  appear only in the [Packages draft](PACKAGES.md#8-agent-first-cli) and
  the prototype, not in the CLI tier.

## Method

**Case selection.** Eight workflows from the task brief, each from a
well-known tool, plus two operation cases that reuse their layouts: shared
helpers under `tests/` and `tasks/`, and the REPL in a package. Library and
tool shapes are approximations written for this report.

**Translation.** Each case gives the original layout and commands, the hd
layout as a table, the hd commands, and the hd source. Every hd file is a
`text` block parsed with `hd debug parse`.

**Verdicts.** *Works*: the CLI tier states the behavior and it matches the
original. *Friction*: it works, with an extra step or a visible difference.
*Breaks*: the spec cannot express it, leaves it undefined, or gives a
different answer silently.

**Ranking.** A problem ranks higher when more cases hit it and when its
failure is silent: a green run that checked nothing beats a clear error.

**Limits.** Parsing checks syntax only; no block is claimed to type-check.
The prototype lags the spec (it still has `hd run FILE` and `hd repl`), so
its behavior is cited only as evidence of what an agent sees today. One
probe, piping input into the prototype REPL under a time limit, was refused
by a permission check and was not retried another way.

## Summary

| # | Case | Original | Verdict | Spec element at fault |
| --- | --- | --- | --- | --- |
| 1 | Library, binary, and xtask tasks | Cargo, cargo-xtask | breaks | program arguments; task capabilities; `hd check` coverage; adding an executable drops `src/main.hd` |
| 2 | Several `cmd/` binaries | Go module | friction | entry modules inside the library; no default executable |
| 3 | Script that grows into a package | Deno, Bun | friction | `hd new .` in a non-empty directory; `hd check FILE` changes meaning; arguments |
| 4 | Scripts and dev dependencies | uv, `pyproject.toml` | works, with friction | names that are not identifiers; arguments |
| 5 | Monorepo with three members | npm workspaces | breaks | member commands ignore the workspace; `hd new` misses members; member selection |
| 6 | CI and a release task | GitHub Actions, cargo, go | breaks | `hd check` coverage; skipped tests; exit codes; release with path requirements |
| 7 | AI agent edit-check-test loop | Claude Code on any repo | breaks | JSON output; `hd` without a terminal; scratch files; test code unchecked |
| 8 | Teaching: first file, first package | Python, Cargo tutorials | friction | `tests/` cannot reach an executable-only package; messages |
| 9 | Shared helpers in `tests/` and `tasks/` | Cargo `tests/common` | breaks | `x.hd` beside `x/` has two readings |
| 10 | REPL in a package | `cargo` has none; `iex -S mix`, `python -i` | friction | REPL scope; REPL as a tool |

The package model holds up: Cargo's lib-plus-bin, uv's dev dependencies,
and Go's several binaries all translate with few lines. What breaks is
mostly what the CLI tier leaves unstated. Programs get no arguments or
files, a whole-package `hd check` may skip test code and tasks, and a
command inside a workspace member ignores the workspace. Agents are hit
hardest, because their failures are silent: a green check that checked
nothing, an empty JSON stream, or a REPL that waits for a terminal.

## Cases

### Case 1: Cargo Library, Binary, And xtask

A ripgrep-like crate: a library, a binary that wraps it, and an xtask
directory with `codegen`, `release`, and `lint`
([cargo-xtask](https://github.com/matklad/cargo-xtask)).

```toml
# Cargo.toml (a workspace with the crate and xtask)
[workspace]
members = [".", "xtask"]

[package]
name = "grepper"

# .cargo/config.toml
[alias]
xtask = "run --package xtask --"
```

```sh
cargo run -- --count foo src/     # the binary, with arguments
cargo xtask codegen                # a dev task
cargo xtask release 1.4.0          # a dev task with an argument
cargo check --all-targets          # lib, bins, tests, examples
```

The hd layout needs no second package, since `tasks/` replaces xtask:

| File | Role |
| --- | --- |
| `grepper/hd.toml` | `[package] name = "grepper"`, no `[[executable]]` |
| `src/lib.hd` | the library root |
| `src/config.hd` | a library module |
| `src/main.hd` | the default executable `grepper` |
| `tasks/codegen.hd`, `tasks/release.hd`, `tasks/lint.hd` | three tasks |
| `tasks/common/paths.hd` | a shared task module |

```text
# src/lib.hd: the package root module
pub use self.config.{Config, load}
```

```text
# src/config.hd
pub data Config:
    pub pattern: string
    pub count_only: bool

pub fn load(pattern: string) -> Result[Config, string]:
    if pattern.len() == 0:
        return .Err("empty pattern")
    .Ok(Config { pattern: pattern, count_only: false })
```

```text
# src/main.hd: the default executable, named after the package
use self.{load}

pub fn main() -> Result[void, string] $ Console:
    config := load("foo")?
    println("searching for ${config.pattern}")
```

```text
# tasks/codegen.hd: run with `hd run codegen`
use pkg.{Config}
use self.common.paths.{schema_file}
use std.fs.{Fs}  # hypothetical: no file system trait is specified

pub fn main() -> Result[void, string] $ Fs + Console:
    text := $.use(Fs).read(schema_file())?  # hypothetical API
    println("generated from ${text.len()} bytes")
```

```text
# tasks/common/paths.hd: a shared task module
pub fn schema_file() -> string:
    "schema/config.json"
```

```text
# tasks/release.hd: `hd run release 1.4.0` has no way to pass 1.4.0
use std.process.{Args}  # hypothetical: no argument trait is specified

pub fn main() -> Result[void, string] $ Args + Console:
    match $.use(Args).get(1):  # hypothetical API
        .Some(version) => println("tagging v${version}")
        .None => return .Err("usage: hd run release VERSION")
    .Ok()
```

```sh
hd run                 # src/main.hd; no way to pass --count foo src/
hd run grepper         # the same, by the package name
hd run codegen         # tasks/codegen.hd
hd run release 1.4.0   # unspecified: does 1.4.0 reach the task?
hd check               # whole package: are tasks/ and tests/ checked?
```

**What breaks.**

- The binary cannot receive `--count foo src/`, and `release` cannot
  receive `1.4.0`: no CLI rule passes arguments ([CLI-3](#cli-3-program-arguments)).
- `codegen` must read and write files. No profile binds a file trait, and
  nothing says where a task's profile comes from ([CLI-4](#cli-4-host-capabilities-of-tasks-and-scripts)).
- Whether `hd check` covers `tasks/` is unstated, so a task can rot until
  someone runs it ([CLI-2](#cli-2-what-a-whole-package-hd-check-covers)).
- Adding a fourth program, such as an executable `grepper-server`, needs an
  `[[executable]]` table. That table removes `src/main.hd` as the default
  executable, so `hd run grepper` stops working ([CLI-7](#cli-7-adding-an-executable-drops-the-default-one)).
- Where `hd run codegen` runs, and so where `schema/config.json` resolves,
  is unstated ([CLI-11](#cli-11-working-directory-of-hd-run)).

**Verdict: breaks.** The layout is shorter than Cargo's, since tasks need
no second package, but two of three tasks cannot be written today.

### Case 2: Go Module With Several `cmd/` Binaries

A Go service with `cmd/server`, `cmd/cli`, and `cmd/migrate`, sharing
`internal/store` ([go run](https://pkg.go.dev/cmd/go#hdr-Compile_and_run_Go_program)).

```sh
go run ./cmd/server -port 8080
go run ./cmd/migrate up
go build ./...
go vet ./...
```

| File | Role |
| --- | --- |
| `hd.toml` | three `[[executable]]` tables |
| `src/store.hd` | shared code |
| `src/cmd/server.hd`, `src/cmd/cli.hd`, `src/cmd/migrate.hd` | entry modules |

```toml
[package]
name = "orders"

[[executable]]
name = "server"
module = "cmd.server"

[[executable]]
name = "cli"
module = "cmd.cli"

[[executable]]
name = "migrate"
module = "cmd.migrate"
```

```text
# src/store.hd
pub data Store:
    pub name: string

pub fn open_store(name: string) -> Result[Store, string]:
    .Ok(Store { name: name })
```

```text
# src/cmd/server.hd: executable `server`
use pkg.store.{open_store}

pub fn main() -> Result[void, string] $ Console:
    store := open_store("data")?
    println("serving ${store.name}")
```

```text
# src/cmd/migrate.hd: an executable written as a script
use pkg.store.{open_store}

store := open_store("data")
println("migrating")  # fine in an entry module; a library module bans it
```

```sh
hd run server     # needs a name: three executables, no default
hd run migrate    # still no way to pass `up`
hd build          # all three executables
```

**What breaks.**

- `src/cmd/server.hd` is an entry module, and nothing says it is outside
  the library. If the package also has `src/lib.hd`, a dependent could
  `use dep.orders.cmd.server`. Then `migrate.hd`'s top-level `println` is
  legal as an entry and illegal as a library module, whose initialization
  must be requirement-free ([CLI-8](#cli-8-entry-modules-inside-the-library)).
- Bare `hd run` is always an error with several executables. Go has the
  same rule; Cargo has `default-run`. This is friction only.
- `go vet ./...` and `go build ./...` map to `hd check` and `hd build` with
  no path argument. Selecting a subtree, as `./cmd/...` does, has no form.

**Verdict: friction.** The manifest is explicit and short. The library
question decides whether the `cmd` modules leak to dependents.

### Case 3: Deno Or Bun Script That Grows Into A Package

A one-file script run as `deno run notes.ts todo.txt`, which later needs a
helper file and a dependency ([deno run](https://docs.deno.com/runtime/reference/cli/run/)).

```text
# notes.hd, run with `hd notes.hd`
notes := ["buy milk", "call Ada"]
for note in notes:
    println("- ${note}")
```

```sh
hd notes.hd             # works
hd notes.hd todo.txt    # unspecified: no argument rule
hd check notes.hd       # works: outside any package, a single file
```

Splitting out a helper is an error, by design
([`module.single-file.roots`](../spec/lang/10-modules.md#r-module.single-file.roots)):

```text
# notes.hd with render.hd beside it
use self.render.{bullet}  # error: unknown-module
```

The user turns the directory into a package:

```sh
hd new .              # writes hd.toml, src/main.hd, tests/
mv notes.hd src/main.hd   # overwrites the generated main.hd
hd run
```

```text
# src/main.hd, after the move
use self.render.{bullet}

notes := ["buy milk", "call Ada"]
for note in notes:
    println(bullet(note))
```

```text
# src/render.hd
pub fn bullet(note: string) -> string:
    "- " + note
```

**What breaks.**

- `hd new .` in a directory that already holds `src/main.hd` or `tests/`
  is unstated: overwrite, keep, or fail ([CLI-14](#cli-14-hd-new-in-a-non-empty-directory)).
- Before `hd new .`, `hd check notes.hd` checked a single file. After it,
  the file lies in package `notes` but under no root, and
  [`cli.package.file`](../spec/cli/command-line.md#r-cli.package.file) has
  no module to check. `hd notes.hd` still runs it as a single file
  ([CLI-10](#cli-10-a-package-file-under-no-root)).
- Deno and uv let one file declare dependencies
  ([PEP 723](https://peps.python.org/pep-0723/)). hd chose not to, and the
  spec's Why says so; this report does not reopen it.

**Verdict: friction.** The growth path is two commands, but the first one
has unstated behavior on exactly the directory this workflow produces.

### Case 4: uv Project With Scripts And Dev Dependencies

A Python project with a console script, a seeding script, and pytest as a
dev dependency ([uv projects](https://docs.astral.sh/uv/concepts/projects/dependencies/#development-dependencies)).

```toml
# pyproject.toml
[project]
name = "billing-tool"
[project.scripts]
billing-tool = "billing_tool.cli:main"
[dependency-groups]
dev = ["pytest", "billing-fixtures"]
```

```sh
uv run billing-tool --month 2026-09
uv run scripts/seed.py
uv run pytest -k totals
```

```toml
# hd.toml
[package]
name = "billing_tool"

[dev-dependencies]
fixtures = "github.com/acme/billing-fixtures@0.2.0"
```

| File | Role |
| --- | --- |
| `src/lib.hd`, `src/billing.hd` | the library |
| `src/main.hd` | default executable `billing_tool` |
| `src/billing_test.hd` | a test module |
| `tasks/seed.hd` | task `seed`, using the dev dependency |

```text
# src/billing.hd
pub fn total(amounts: List[i32]) -> i32:
    let mut sum = 0
    for amount in amounts:
        sum += amount
    sum
```

```text
# src/lib.hd
pub use self.billing.{total}
```

```text
# src/billing_test.hd: a test module may use a dev dependency
use dep.fixtures.{sample_amounts}
use std.testing.assert_equal
use super.billing.{total}

it("totals the sample invoice"):
    assert_equal(total(sample_amounts()), 1200, reason="fixture total")
```

```text
# tasks/seed.hd: `hd run seed`
use dep.fixtures.{sample_amounts}
use pkg.{total}

pub fn main() -> void $ Console:
    println("seeded an invoice worth ${total(sample_amounts())}")
```

```sh
hd run billing_tool    # or bare `hd run`; still no `--month 2026-09`
hd run seed
hd test                # no way to select `-k totals`
```

**What breaks.**

- The executable is `billing_tool`, not `billing-tool`. Package names and
  task names follow identifier rules, and the spec does not say whether an
  `[[executable]]` name may hold `-` ([CLI-24](#cli-24-names-that-are-not-identifiers)).
- Arguments again ([CLI-3](#cli-3-program-arguments)), and no test
  filter ([CLI-18](#cli-18-running-one-test-case)).
- Dev dependencies map one to one, including their use from tasks.

**Verdict: works, with friction.**

### Case 5: npm Monorepo As An hd Workspace

An npm workspace with `apps/web`, `libs/ui`, and `libs/shared`, where the
root runs `npm run dev -w apps/web`
([npm workspaces](https://docs.npmjs.com/cli/v10/using-npm/workspaces)).

| File | Role |
| --- | --- |
| `hd.toml` | `[workspace] members = ["apps/web", "libs/ui", "libs/shared"]` |
| `apps/web/hd.toml` | package `web`, default executable `web` |
| `libs/ui/hd.toml` | package `ui`, library |
| `libs/shared/hd.toml` | package `shared`, library |
| `hd.sum` | one, at the workspace root |

```toml
# apps/web/hd.toml
[package]
name = "web"

[dependencies]
ui = { path = "../../libs/ui" }
```

```text
# apps/web/src/main.hd: executable `web`
use dep.ui.{page}

pub fn main() -> void $ Console:
    println(page("home"))
```

```text
# libs/ui/src/lib.hd
use dep.shared.{escape}

pub fn page(title: string) -> string:
    "<h1>" + escape(title) + "</h1>"
```

```text
# libs/shared/src/lib.hd
pub fn escape(text: string) -> string:
    text.trim()
```

```sh
hd run web             # from the root: the one member executable `web`
hd test                # from the root: every member
cd libs/ui && hd test  # from a member: package mode, package `ui`
hd new libs/icons      # a fourth package
```

**What breaks.**

- `cd libs/ui && hd test` finds `libs/ui/hd.toml` first, so it works in
  package mode on `ui` alone. Nothing makes it find the workspace above.
  Selection would then start at `ui` instead of the members, `hd.sum`
  would be the member's own, and the path requirement on `shared` would
  be outside any workspace ([CLI-1](#cli-1-a-member-command-ignores-its-workspace)).
- `hd new libs/icons` creates a package that the workspace does not list.
  A root `hd test` then skips it silently ([CLI-13](#cli-13-hd-new-inside-a-workspace)).
- Two members with a `lint` task make `hd run lint` an error, and only
  `cd` picks one. The root also cannot test one member without `cd`
  ([CLI-15](#cli-15-selecting-one-member-from-the-root)).
- npm runs a script with the package directory as its working directory;
  the spec does not say what `hd run web` uses ([CLI-11](#cli-11-working-directory-of-hd-run)).
- Root-level scripts have no home: a file under the root but in no member
  may use only `std` ([`cli.mode.workspace-file`](../spec/cli/command-line.md#r-cli.mode.workspace-file)).
  A `tools` member holds them instead ([CLI-25](#cli-25-workspace-wide-tasks)).

**Verdict: breaks.** Running from the root works. Running from a member,
which is what developers and agents do most, has no stated workspace
behavior.

### Case 6: CI With Check, Test, Build, And A Release Task

A GitHub Actions job over the workspace of case 5, plus a `release` task
that tags each changed member.

```yaml
steps:
  - run: hd check --format json > check.jsonl
  - run: hd test
  - run: hd build
  - run: hd run release 0.4.2
```

The release task lives in a fourth member, `tools`, since a workspace root
declares no package:

```text
# tools/tasks/release.hd
use std.process.{Args, Command}  # hypothetical: no argument or process trait

pub fn main() -> Result[void, string] $ Args + Command + Console:
    match $.use(Args).get(1):  # hypothetical API
        .Some(version) => $.use(Command).run("git", ["tag", "ui/v${version}"])?  # hypothetical API
        .None => return .Err("usage: hd run release VERSION")
    .Ok()
```

**What breaks.**

- `--format json` is not in the CLI tier ([CLI-5](#cli-5-machine-readable-output)).
- If whole-package `hd check` skips test code and tasks, a broken
  `release.hd` is first found on release day ([CLI-2](#cli-2-what-a-whole-package-hd-check-covers)).
- An integration test whose row names a trait the profile does not bind is
  skipped and "not an error" ([`module.testing.skipped`](../spec/lang/10-modules.md#r-module.testing.skipped)).
  A profile change can skip a whole suite with CI green ([CLI-17](#cli-17-skipped-integration-tests-are-green)).
- `hd build` names no output location for the artifacts that a release
  uploads ([CLI-21](#cli-21-what-hd-build-produces)).
- `web` requires `ui` by path, and a tagged version may not hold a path
  requirement ([`module.version.no-path-release`](../spec/lang/10-modules.md#r-module.version.no-path-release)).
  So every release of `web` first rewrites its manifest ([CLI-19](#cli-19-releasing-a-member-with-path-requirements)).
- Exit status 1 means both "hd found errors" and "the program returned
  `.Err`" ([CLI-12](#cli-12-exit-codes)).

**Verdict: breaks.** Check, test, and build run, but each can be green
without covering what CI assumes.

### Case 7: An AI Agent's Edit-Check-Test Loop

An agent such as Claude Code edits a member of the case 5 workspace. Its
loop is: edit, `hd check --format json`, read diagnostics, fix, `hd test`,
probe an API in a scratch file or the REPL, then run a task.

```sh
hd check --format json src/cart.hd   # 1. after each edit
hd test src/cart.hd                  # 2. the module's tests
hd check scratch.hd                  # 3. a probe file at the package root
hd                                   # 4. the REPL, as a tool, without a TTY
hd run codegen                       # 5. regenerate after a schema edit
```

The agent edits a test with a type error:

```text
# src/cart.hd
use std.testing.assert_equal

pub fn total(prices: List[i32]) -> i32:
    let mut sum = 0
    for price in prices:
        sum += price
    sum

tests:
    it("sums prices"):
        assert_equal(total([1, 2]), "3", reason="sum")  # error: type-mismatch
```

And a probe of the package API:

```text
# scratch.hd at the package root: an agent's probe
use pkg.{total}

println(total([1, 2, 3]))
```

**What the agent sees.**

| Step | Spec | Prototype today | Silent? |
| --- | --- | --- | --- |
| 1 | `hd check FILE` checks the module; test code is compiled "only by a test build" ([`module.test.code`](../spec/lang/10-modules.md#r-module.test.code)) | exit 0 unless `--tests`; the test error is not shown | yes |
| 1 | `--format json` unspecified | JSON lines per diagnostic; success prints plain text `FILE: ok`; a missing file prints a Node stack trace with exit 1 | partly |
| 2 | runs the module's tests | a file with no tests prints `0 passed`, exit 0 | yes |
| 3 | FILE is in the package but under no root; [`cli.package.file`](../spec/cli/command-line.md#r-cli.package.file) has no module | not probed | undefined |
| 4 | `hd` opens the REPL; nothing covers a pipe or no terminal | not probed (refused by a permission check) | likely a hang |
| 5 | runs `tasks/codegen.hd`; working directory and capabilities unstated | `run FILE` only | undefined |

**Awkward for an agent.**

- Test code is unchecked by `hd check`, so the cheap step passes and the
  expensive one fails later ([CLI-2](#cli-2-what-a-whole-package-hd-check-covers)).
- No JSON shape, no final summary record, and no stable exit codes
  ([CLI-5](#cli-5-machine-readable-output), [CLI-12](#cli-12-exit-codes)).
- `hd` alone is the REPL. An agent that runs `hd $FILE` with an empty
  variable, or pipes code into `hd`, waits on a prompt ([CLI-6](#cli-6-hd-without-a-terminal)).
- A scratch file has no supported place: under `src` it joins the
  library, under `tasks` it becomes a task, and at the root it is
  undefined ([CLI-10](#cli-10-a-package-file-under-no-root)).
- No way to run one failing test ([CLI-18](#cli-18-running-one-test-case)),
  and zero tests is green ([CLI-16](#cli-16-zero-test-cases-is-green)).
- Inside a member, every command ignores the workspace ([CLI-1](#cli-1-a-member-command-ignores-its-workspace)).

**What an agent needs.** One JSON record per diagnostic with code,
severity, file, span, and fix-it edit; a final summary record with counts
and the files checked; exit codes that separate "errors found" from "hd
failed"; a non-interactive evaluation path; and a check that covers test
code and tasks by default. The prototype already emits most diagnostic
fields; its `rules` list is every rule sharing the code, 39 entries for
`type-mismatch`, which is noise rather than a pointer.

**Verdict: breaks.**

### Case 8: Teaching, First File Then First Package

A beginner follows the [Language Tour](../guide/LANGUAGE_TOUR.md).

```text
# hello.hd
println("hello, hd-lang")
```

```sh
hd hello.hd
hd new greeter
cd greeter
hd run
```

`hd new` writes a `src/main.hd`; its content is unstated. Assume:

```text
# greeter/src/main.hd, as `hd new greeter` might write it
pub fn greeting(name: string) -> string:
    "hello, " + name

pub fn main() -> void $ Console:
    println(greeting("world"))
```

The beginner then writes a test in the `tests/` directory that `hd new`
made:

```text
# greeter/tests/greeting.hd: an integration test program
use std.testing.assert_equal
use pkg.{greeting}  # error: unknown-module, since the package has no library

it("greets by name"):
    assert_equal(greeting("Ada"), "hello, Ada", reason="greeting text")
```

**What breaks.**

- An integration test sees only the library, and a package without
  `src/lib.hd` has none. So the `tests/` directory that `hd new` creates
  cannot test anything the generated package holds ([CLI-20](#cli-20-hd-new-makes-a-tests-directory-with-nothing-to-test)).
- A beginner who runs `hd src/main.hd` gets a single-file run. It works
  until `main.hd` adds `use self.x`; the error then suggests a task, not
  `hd run` ([CLI-23](#cli-23-the-error-for-hd-srcmainhd)).
- `hd new my-app` needs a package name, which must be an identifier
  ([CLI-24](#cli-24-names-that-are-not-identifiers)).

**Verdict: friction.** The first file is one line and one command. The
first package works until the first test.

### Case 9: Shared Helpers In `tests/` And `tasks/`

Cargo's `tests/common/mod.hd` pattern is specified. A user who knows the
`src` rule that `x.hd` is the parent of `x/`
([`module.folder.interchangeable`](../spec/lang/10-modules.md#r-module.folder.interchangeable))
writes the parent as a file instead:

| File | `src` reading | `tests/` program reading |
| --- | --- | --- |
| `tests/common.hd` | module `common`, parent of `common/` | integration test program `common` |
| `tests/common/fixtures.hd` | module `common.fixtures` | shared test module |

```text
# tests/common.hd
pub use self.fixtures.{sample_cart}
```

```text
# tests/checkout.hd
use std.testing.assert
use self.common.{sample_cart}

it("checks out a sample cart"):
    assert(sample_cart().len() > 0, reason="cart has items")
```

Under the program reading, `tests/common.hd` is a root file, so `self` is
the test root and `self.fixtures` names `tests/fixtures.hd`. Under the
parent reading, `self.fixtures` names `tests/common/fixtures.hd`. And
`use self.common` from `checkout.hd` either names a program, an
`unknown-module` error, or the shared parent. The same holds for
`tasks/shared.hd` beside `tasks/shared/`, where the program reading also
makes `hd run shared` a task.

**Verdict: breaks.** Two specified rules give two different files
([CLI-9](#cli-9-xhd-beside-x-under-tests-and-tasks)).

### Case 10: The REPL In A Package

```sh
cd shop/src/billing
hd
```

```text
# typed at the REPL prompt in shop/src/billing
use self.util.{slug}
slug("Spring Sale")
```

`self.util` names `src/util.hd`, as
[`cli.repl.package`](../spec/cli/command-line.md#r-cli.repl.package) says.

**What breaks.**

- "Acts as its root module" does not say whether private declarations of
  `src/lib.hd` are visible, or whether dev dependencies are
  ([CLI-22](#cli-22-what-the-package-repl-can-see)).
- Started in `tasks/`, the session still roots at `src`, so
  `self.common` cannot reach `tasks/common/`. This matches the rule; it is
  noted only.
- From a workspace root the session gets only `std`
  ([`cli.workspace.repl`](../spec/cli/command-line.md#r-cli.workspace.repl)),
  so an agent at the root must `cd` first.

**Verdict: friction.**

## Comparison With Other Tools

| Question | hd (spec) | Cargo | Go | npm / Deno / uv | Source |
| --- | --- | --- | --- | --- | --- |
| Command inside a member finds the workspace | unstated | yes, searches parent directories | `go.work` found upward | npm: `-w` from root; uv: yes | [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) |
| Program arguments | unstated | after `--` | after the package | Deno: after the script | [cargo run](https://doc.rust-lang.org/cargo/commands/cargo-run.html), [go run](https://pkg.go.dev/cmd/go#hdr-Compile_and_run_Go_program) |
| Check covers tests | unstated; test code "only in a test build" | no by default; `--all-targets` | `go vet` does | `deno check` checks the given files | [cargo check](https://doc.rust-lang.org/cargo/commands/cargo-check.html) |
| Adding a binary keeps `main` | no: any table removes the default | yes: auto-discovery stays on | n/a | n/a | [Target auto-discovery](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#target-auto-discovery) |
| Machine output | not in the CLI tier | `--message-format json`, JSON lines, a final `build-finished` record | `go test -json`, `go vet -json` | n/a | [Cargo JSON messages](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages) |
| No terminal on stdin | unstated | n/a | n/a | Python runs stdin as a script | [Python command line](https://docs.python.org/3/using/cmdline.html#interface-options) |
| Zero tests collected | unstated | exit 0 | exit 0, "no test files" | pytest exit 5 | [pytest exit codes](https://docs.pytest.org/en/stable/reference/exit-codes.html) |
| Run one test | no form | `cargo test NAME` | `go test -run RE` | `pytest -k` | [cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html) |
| New package in a workspace | not added | added to members | `go work use` is separate | npm `init -w` adds it | [Cargo 1.75 changelog](https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-175-2023-12-28) |
| Init in a non-empty directory | unstated | `cargo init` keeps existing sources | n/a | n/a | [cargo init](https://doc.rust-lang.org/cargo/commands/cargo-init.html) |
| Script working directory | unstated | unchanged | unchanged | npm: the package root | [npm run](https://docs.npmjs.com/cli/v10/commands/npm-run-script) |
| Path dependency at release | rewrite the manifest | `{ path, version }` in one entry | `go.work` keeps versions | n/a | [Cargo multiple locations](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations) |
| Host access of a script | unstated | full | full | Deno: `--allow-*` flags | [Deno security](https://docs.deno.com/runtime/fundamentals/security/) |

## Problems, Ranked

Severity: **high** means a silent wrong result or a blocked common
workflow; **medium** means a clear error or a workaround; **low** means a
message or polish.

| Rank | ID | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | CLI-1 | A member command ignores its workspace | high, silent | 5, 6, 7 |
| 2 | CLI-2 | What a whole-package `hd check` covers | high, silent | 1, 4, 6, 7 |
| 3 | CLI-3 | Program arguments | high, blocks | 1, 2, 3, 4, 6 |
| 4 | CLI-4 | Host capabilities of tasks and scripts | high, blocks | 1, 3, 6 |
| 5 | CLI-5 | Machine-readable output | high | 6, 7 |
| 6 | CLI-6 | `hd` without a terminal | high, silent hang | 7, 10 |
| 7 | CLI-7 | Adding an executable drops the default one | medium, silent | 1, 2 |
| 8 | CLI-8 | Entry modules inside the library | medium | 2 |
| 9 | CLI-9 | `x.hd` beside `x/` under `tests` and `tasks` | medium, silent | 9, 1 |
| 10 | CLI-10 | A package file under no root | medium | 3, 7 |
| 11 | CLI-11 | Working directory of `hd run` | medium, silent | 1, 5, 6, 7 |
| 12 | CLI-12 | Exit codes | medium | 6, 7 |
| 13 | CLI-13 | `hd new` inside a workspace | medium, silent | 5 |
| 14 | CLI-14 | `hd new` in a non-empty directory | medium | 3 |
| 15 | CLI-15 | Selecting one member from the root | medium | 5, 6, 7 |
| 16 | CLI-16 | Zero test cases is green | medium, silent | 7, 8 |
| 17 | CLI-17 | Skipped integration tests are green | medium, silent | 6 |
| 18 | CLI-18 | Running one test case | medium | 4, 7 |
| 19 | CLI-19 | Releasing a member with path requirements | medium | 6 |
| 20 | CLI-20 | `hd new` makes a `tests` directory with nothing to test | low | 8 |
| 21 | CLI-21 | What `hd build` produces | low | 6 |
| 22 | CLI-22 | What the package REPL can see | low | 10 |
| 23 | CLI-23 | The error for `hd src/main.hd` | low | 8 |
| 24 | CLI-24 | Names that are not identifiers | low | 4, 8 |
| 25 | CLI-25 | Workspace-wide tasks | low | 5, 6 |

### CLI-1: A Member Command Ignores Its Workspace

**Effect.** `cd libs/ui && hd test` stops at `libs/ui/hd.toml`, which
declares a package, so it runs in package mode on `ui` alone (cases 5, 6,
7). Selection starts at the root package, so member versions can differ
from the workspace's, and `hd.sum` belongs beside `ui`'s manifest. The path
requirement on `shared` is valid "only between workspace members", and
here no workspace is in play. Developers and agents work inside members
most of the time.

**Candidates.**

1. Should a command in a member search upward for a workspace manifest
   that lists it, and use that workspace's selection and `hd.sum`, as Cargo
   does?
2. Should a member's manifest name its workspace, as Cargo's
   `package.workspace` key does, instead of an upward search?
3. Or should a member stay a root package, with its path requirements an
   error outside workspace mode?

**Recommendation.** Candidate 1: it keeps one selection per workspace with
no new manifest key, and Cargo users expect it.

```sh
cd ws/libs/ui
hd test    # candidate 1: package ui, with the selection and hd.sum of ws
```

### CLI-2: What A Whole-Package `hd check` Covers

**Effect.** [`module.test.code`](../spec/lang/10-modules.md#r-module.test.code)
says only a test build compiles test code. Read literally, `hd check`
skips `tests:` blocks, test modules, and `tests/`, and nothing says it
covers `tasks/` (cases 1, 4, 6, 7). The prototype's `hd check` passes a
file whose test has a type error, unless `--tests` is given. An agent's
cheap step is green while its tests do not compile.

**Candidates.**

1. Should `hd check` cover everything a package holds by default: source,
   test code, integration tests, and tasks?
2. Should it cover only what `hd build` compiles, with a flag such as
   `--tests` adding the rest, as Cargo's `--all-targets` does?
3. Should `hd check` cover source and tasks, with test code checked only
   by `hd test`?

**Recommendation.** Candidate 1: a check that can be green while code is
broken defeats its purpose for agents, and checking costs no run.

```sh
hd check    # candidate 1: src, tests: blocks, *_test.hd, tests/, tasks/
```

### CLI-3: Program Arguments

**Effect.** No rule passes arguments to `hd FILE`, `hd run`, or
`hd run NAME` (cases 1, 2, 3, 4, 6). A release task cannot receive a
version, and a binary cannot receive flags. Nothing says whether
`hd run server --port 80` gives `--port` to hd or to the program. The
host trait that reads arguments is a separate, open std question.

**Candidates.**

1. Should every word after FILE or NAME go to the program, as `go run`
   and `deno run` do, with hd's own flags only before it?
2. Should arguments follow `--`, as `cargo run -- ARGS` does?
3. Should the CLI tier wait until the argument trait exists?

**Recommendation.** Candidate 1: it is shorter for agents and scripts, and
unambiguous once hd's flags must precede the program.

```sh
hd run release 1.4.0         # candidate 1: the task sees ["1.4.0"]
hd notes.hd --sort todo.txt  # the script sees ["--sort", "todo.txt"]
```

### CLI-4: Host Capabilities Of Tasks And Scripts

**Effect.** A task's or single file's profile has no stated source (cases
1, 3, 6). Executables may get one from the manifest someday; a task and
`hd FILE` have no table. With the default profile, codegen cannot read a
schema and release cannot run git. The trait catalog is open, but where
the grant comes from is a CLI question.

**Candidates.**

1. Should `hd run` and `hd FILE` grant every host trait that the entry's
   requirement row names, so the row is the declaration, as Python and Go
   grant everything?
2. Should a grant need a flag or manifest entry beyond the row, as Deno's
   `--allow-read` does?
3. Should tasks use a fixed development profile that binds every host
   trait, while executables keep a selected one?

**Recommendation.** Candidate 1, labeled as waiting on the catalog: the
row already states what the program needs, which matches the
[Packages](PACKAGES.md#21-design-goals) goal that provider bindings come
from entry points.

```text
pub fn main() -> Result[void, string] $ Fs + Console:  # hypothetical trait Fs
    pass
```

### CLI-5: Machine-Readable Output

**Effect.** `--format json` exists only in the Packages draft (one
document per command) and the prototype (one JSON line per diagnostic).
The two disagree, and neither is in the CLI tier (cases 6, 7). In the
prototype, a clean check prints plain text under `--format json`, and an
internal failure prints a stack trace. An agent cannot tell "no
diagnostics" from "nothing ran".

**Candidates.**

1. Should the CLI tier specify `--format json` as JSON lines: one record
   per diagnostic, then one final summary record with counts and the files
   covered, as Cargo's `build-finished` does?
2. Should it be one JSON document per command, as the Packages draft
   says?
3. Should machine output stay package tooling, outside the CLI tier?

**Recommendation.** Candidate 1: a stream lets an agent act on the first
error, and the summary record makes a clean run explicit.

```sh
hd check --format json
# {"kind":"diagnostic","code":"type-mismatch","file":"src/cart.hd",...}
# {"kind":"summary","errors":1,"warnings":0,"files":12}
```

### CLI-6: `hd` Without A Terminal

**Effect.** `hd` with no arguments opens the REPL, and nothing covers a
pipe or a missing terminal (cases 7, 10). An agent that pipes code into
`hd`, or runs `hd $FILE` with an empty variable, waits on a prompt until a
timeout. That failure is silent: no output, no exit.

**Candidates.**

1. When stdin is not a terminal, should `hd` read stdin as one
   single-file program and run it, as Python does?
2. Should it fail at once with a message naming `hd FILE`?
3. Should the REPL read lines from a pipe and print each result, so an
   agent can use it as a tool?

**Recommendation.** Candidate 1: it gives agents a non-interactive
evaluation path with no new command, and keeps single-file rules.

```sh
echo 'println(1 + 2)' | hd    # candidate 1: prints 3, exits 0
```

### CLI-7: Adding An Executable Drops The Default One

**Effect.** Only a package with no `[[executable]]` table has the default
executable (cases 1, 2). Adding a second program, such as `migrate`,
silently drops `src/main.hd`: `hd run shop` becomes an unknown name. The
file stays a program that nothing runs, with at most an `unselected-main`
warning, if that rule covers it.

**Candidates.**

1. Should `src/main.hd` stay the default executable unless a table names
   module `main`, as Cargo's auto-discovery keeps `src/main.rs`?
2. Should a package with tables and an unnamed `src/main.hd` be an error
   whose fix-it adds its table?
3. Keep the rule, and state that `unselected-main` covers `src/main.hd`?

**Recommendation.** Candidate 2: it keeps every executable in the
manifest, as the spec's Why prefers, and the error is not silent.

```toml
[[executable]]
name = "migrate"
module = "tools.migrate"
# candidate 2: error, src/main.hd is no executable; add name = "shop", module = "main"
```

### CLI-8: Entry Modules Inside The Library

**Effect.** `src/main.hd` is never part of the library. An executable's
other entry module, such as `src/cmd/server.hd`, has no such rule (case 2).
If it is a library module, dependents can use it, and its top-level code
must be requirement-free. As an entry module, its top level may print. The
same file then meets two rules.

**Candidates.**

1. Should every module an `[[executable]]` names be outside the library
   and non-importable, as `src/main.hd` is?
2. Should it stay a library module, so its top level must be
   requirement-free and it must use `main`?
3. Should executable entries live under a reserved directory such as
   `src/bin/`, as Cargo's are?

**Recommendation.** Candidate 1: it extends the one rule `src/main.hd`
already has to every entry module, with no new layout.

```text
# src/cmd/migrate.hd, named by an [[executable]] table
println("migrating")  # candidate 1: valid, the module is a program
```

### CLI-9: `x.hd` Beside `x/` Under `tests` And `tasks`

**Effect.** Under `src`, `x.hd` is the parent of `x/`. Under `tests` and
`tasks`, a top-level `x.hd` is a program (case 9). So `tests/common.hd`
beside `tests/common/` is either a test program or the shared parent, and
`self.fixtures` inside it names a different file under each reading.
Users carry the `src` habit over.

**Candidates.**

1. Should a top-level `x.hd` beside a directory `x/` under `tests` or
   `tasks` be an error whose fix-it moves it to `x/mod.hd`?
2. Should the folder rule win, so `tests/common.hd` is the shared parent
   and not a program?
3. Should the program rule win, and the spec say so with an example?

**Recommendation.** Candidate 1: it removes the clash at the one place it
occurs, and the fix-it matches the existing `folder-cycle` fix-it.

```text
# tests/common.hd beside tests/common/: candidate 1 rejects this file
pub use self.fixtures.{sample_cart}
```

### CLI-10: A Package File Under No Root

**Effect.** [`cli.package.file`](../spec/cli/command-line.md#r-cli.package.file)
works on "that file's module". A file at the package root, such as an
agent's `scratch.hd` or a script left before `hd new .`, has none (cases
3, 7). `hd scratch.hd` runs it as a single file, so `hd check scratch.hd`
and `hd scratch.hd` disagree or are undefined.

**Candidates.**

1. Should `hd check` and `hd test` treat a package file under no root as
   a single-file program, matching `hd FILE`?
2. Should it be an error that names the roots `src`, `tests`, and
   `tasks`?
3. Should it be checked as a probe that sees the package's library, as an
   integration test does?

**Recommendation.** Candidate 1: one rule for every command on such a
file, and no new kind of module.

```sh
hd check scratch.hd   # candidate 1: a single file; `use pkg` is unknown-module
```

### CLI-11: Working Directory Of `hd run`

**Effect.** Nothing states the working directory of `hd run NAME` (cases
1, 5, 6, 7). Run from a workspace root, a codegen task that resolves
`schema/config.json` reads a different file than when run from its
member. A task also cannot learn its package directory.

**Candidates.**

1. Should `hd run` keep the caller's working directory, as Cargo and Go
   do?
2. Should a task, or every program, run in its package directory, as npm
   scripts do?
3. Keep the caller's directory, and give programs the package directory
   through a host fact once the catalog exists?

**Recommendation.** Candidate 2 for tasks only: tasks act on their
package, and the result then does not depend on where the command ran.

```sh
cd ws && hd run codegen   # candidate 2: runs in ws/libs/ui, the task's package
```

### CLI-12: Exit Codes

**Effect.** The CLI tier states no exit codes (cases 6, 7). A program's
`.Err` exits with 1, and the prototype also exits with 1 when it reports
diagnostics. If `hd run` passes the program's code through, an agent or CI
step cannot tell
"my code did not compile" from "my program failed".

**Candidates.**

1. Should hd reserve codes for its own failures, such as Cargo's 101,
   distinct from 0 and 1?
2. Should the codes follow the Packages draft: 0 success, 1 diagnostics,
   2 invalid invocation, 3 network?
3. Should the distinction live only in machine output?

**Recommendation.** Candidate 2, with `hd run` passing the program's own
code through only after a successful build.

```sh
hd check; echo $?    # candidate 2: 1 means diagnostics were reported
```

### CLI-13: `hd new` Inside A Workspace

**Effect.** `hd new libs/icons` under a workspace root creates a package
that the workspace manifest does not list (case 5). Every root command
then skips it without a word.

**Candidates.**

1. Should `hd new` under a workspace root add the new package to
   `members`, as Cargo 1.75 does?
2. Should it leave `members` alone and print a note naming the line to
   add?
3. Should it be an error unless a flag says which?

**Recommendation.** Candidate 1: `hd` already writes manifests, and the
workspace manifest is committed, so the edit shows in review.

```sh
hd new libs/icons   # candidate 1: also adds "libs/icons" to ws/hd.toml members
```

### CLI-14: `hd new` In A Non-Empty Directory

**Effect.** `hd new .` fails only when `hd.toml` exists (case 3). With an
existing `src/main.hd` or `tests/`, it may overwrite, keep, or fail.
Overwriting loses a user's program.

**Candidates.**

1. Should `hd new` keep every existing file and write only the missing
   ones, as `cargo init` does?
2. Should it fail when any file it would write already exists?

**Recommendation.** Candidate 1: it makes `hd new .` safe to run on a
half-made package, and never loses code.

```sh
hd new .   # candidate 1: keeps src/main.hd, writes only hd.toml
```

### CLI-15: Selecting One Member From The Root

**Effect.** At a workspace root, `hd run NAME` fails when two members have
NAME, and `hd test` runs every member (cases 5, 6, 7). Only `cd` selects
one member. CI and agents usually run from the root.

**Candidates.**

1. Should a directory argument select a package, as in `hd test libs/ui`
   and `hd run libs/ui lint`?
2. Should a member flag select it, as Cargo's `-p NAME` does?
3. Keep `cd` as the only way, with the error naming the `cd` to run?

**Recommendation.** Candidate 1: it reuses the start-directory rule, so a
directory acts as FILE already does, with no new flag.

```sh
hd test libs/ui    # candidate 1: start directory libs/ui, package ui
```

### CLI-16: Zero Test Cases Is Green

**Effect.** A `hd test` that registers no test case passes (cases 7, 8).
A test in the wrong place, or a module with none, looks the same as a
passing suite. The prototype prints `0 passed` and exits 0.

**Candidates.**

1. Should `hd test` with a FILE fail when FILE registers no test case, as
   pytest's exit code 5 does?
2. Should it pass, with the summary stating zero?

**Recommendation.** Candidate 1 for an explicit FILE only: naming a file
asks for its tests, while a whole package may have none yet.

```sh
hd test src/cart.hd   # candidate 1: error, src/cart.hd registers no test case
```

### CLI-17: Skipped Integration Tests Are Green

**Effect.** An integration test whose row names a trait the profile does
not bind is skipped, which is not an error
([`module.testing.skipped`](../spec/lang/10-modules.md#r-module.testing.skipped)).
A profile change can skip a whole suite while CI stays green (case 6).

**Candidates.**

1. Should the `hd test` summary count skipped tests apart from ignored
   ones, with a flag that turns skips into failures?
2. Should a skip fail by default, with a flag to allow it?

**Recommendation.** Candidate 1: the language-tier rule stays, and CI opts
in to strictness.

```sh
hd test --deny-skipped   # candidate 1; hypothetical flag
```

### CLI-18: Running One Test Case

**Effect.** `hd test FILE` is the finest selection (cases 4, 7). An agent
fixing one failing test reruns every test of the module. Test names are
string literals, so a filter is static.

**Candidates.**

1. Should `hd test FILE NAME` run the test cases whose name contains NAME?
2. Should a `--filter PATTERN` flag do it, for packages too?

**Recommendation.** Candidate 1: names are literals by rule, so a
substring match needs no new concept.

```sh
hd test src/cart.hd "sums prices"
```

### CLI-19: Releasing A Member With Path Requirements

**Effect.** A member requires another member by `{ path = "..." }`, and a
tagged version may not hold a path requirement (case 6). So every release
of `web` rewrites its manifest to a host path first. The spec's Note says
so, and a release task must do it each time. DEP10 rejected `go.work`;
Cargo's combined entry is a different option.

**Candidates.**

1. Should a path requirement also carry a version, as in
   `ui = { path = "../../libs/ui", version = "0.4.2" }`, with the version
   used when the package is fetched?
2. Keep the rewrite, done by a toolchain release command?

**Recommendation.** Candidate 2 for now: releases wait on package tooling,
and candidate 1 changes a decided requirement form.

```toml
ui = { path = "../../libs/ui", version = "0.4.2" }  # candidate 1
```

### CLI-20: `hd new` Makes A `tests` Directory With Nothing To Test

**Effect.** `hd new` writes `src/main.hd` and `tests/`, and no
`src/lib.hd` (case 8). Integration tests see only the library, so the new
`tests/` can test nothing until the user adds `src/lib.hd`.

**Candidates.**

1. Should `hd new` also write `src/lib.hd`, with `src/main.hd` using it?
2. Should `hd new` skip `tests/`, leaving tests in `tests:` blocks?

**Recommendation.** Candidate 1: a beginner's first test then works in
either place, and the layout is Cargo's lib-plus-bin.

```text
# src/lib.hd, as candidate 1 would write it
pub fn greeting(name: string) -> string:
    "hello, " + name
```

### CLI-21: What `hd build` Produces

**Effect.** `hd build` names no artifact or location, and no output for a
library-only package (case 6). A release step cannot find what to upload.

**Candidates.**

1. Should `hd build` write one Wasm file per executable to a fixed
   directory, such as `build/NAME.wasm`?
2. Should this wait for package tooling?

**Recommendation.** Candidate 2: the Wasm boundary is still open, and
nothing else in this report depends on it.

### CLI-22: What The Package REPL Can See

**Effect.** The package REPL "acts as its root module" (case 10). Whether
it sees private declarations of `src/lib.hd`, and dev dependencies, is
unstated.

**Candidates.**

1. Should the session see what `src/main.hd` sees: public names, plus
   dependencies and dev dependencies?
2. Should it be inside `src/lib.hd`, seeing its private names too?

**Recommendation.** Candidate 1: it matches the one existing non-library
root, and keeps privacy as it is.

### CLI-23: The Error For `hd src/main.hd`

**Effect.** `hd src/main.hd` runs a single file. Once `main.hd` uses
`self`, the error suggests a task and `hd run NAME`
([`cli.file.in-package`](../spec/cli/command-line.md#r-cli.file.in-package)),
though the right command is `hd run` (case 8).

**Candidates.**

1. Should the message name the executable or task whose entry is FILE,
   and the exact `hd run` command?

**Recommendation.** Candidate 1.

### CLI-24: Names That Are Not Identifiers

**Effect.** Task names come from file names, which are identifiers. The
spec does not say whether a package or `[[executable]]` name may hold `-`,
or what `hd new my-app` writes (cases 4, 8).

**Candidates.**

1. Should package and executable names follow identifier rules, with
   `hd new my-app` writing `my_app` and saying so?
2. Should executable names allow `-`, as Cargo binaries do?

**Recommendation.** Candidate 1: one name rule for packages, executables,
and tasks.

### CLI-25: Workspace-Wide Tasks

**Effect.** A workspace root declares no package, so it has no `tasks/`
(cases 5, 6). A release task for the whole repository lives in a `tools`
member, as Cargo's xtask does.

**Candidates.**

1. Keep the member, and document the pattern in the guide?
2. Allow `tasks/` at a workspace root, with the members as dependencies?

**Recommendation.** Candidate 1: it needs no rule, and Cargo users know
the pattern.

## Parse Log

Every `text` block above was parsed with
`node --experimental-strip-types bin/hd.js debug parse FILE`, by the script
in the shared rules. Parsing checks syntax only.

| Block | Case | Content | Result |
| --- | --- | --- | --- |
| 1 | 1 | `src/lib.hd` | parses |
| 2 | 1 | `src/config.hd` | parses |
| 3 | 1 | `src/main.hd` | parses |
| 4 | 1 | `tasks/codegen.hd`, hypothetical `std.fs` | parses |
| 5 | 1 | `tasks/common/paths.hd` | parses |
| 6 | 1 | `tasks/release.hd`, hypothetical `std.process` | parses |
| 7 | 2 | `src/store.hd` | parses |
| 8 | 2 | `src/cmd/server.hd` | parses |
| 9 | 2 | `src/cmd/migrate.hd` | parses |
| 10 | 3 | `notes.hd` | parses |
| 11 | 3 | `notes.hd` with a relative use | parses; `unknown-module` is a resolution error |
| 12 | 3 | `src/main.hd` | parses |
| 13 | 3 | `src/render.hd` | parses |
| 14 | 4 | `src/billing.hd` | parses |
| 15 | 4 | `src/lib.hd` | parses |
| 16 | 4 | `src/billing_test.hd` | parses |
| 17 | 4 | `tasks/seed.hd` | parses |
| 18 | 5 | `apps/web/src/main.hd` | parses |
| 19 | 5 | `libs/ui/src/lib.hd` | parses |
| 20 | 5 | `libs/shared/src/lib.hd` | parses |
| 21 | 6 | `tools/tasks/release.hd`, hypothetical `std.process` | parses |
| 22 | 7 | `src/cart.hd` | parses; `type-mismatch` is a type error |
| 23 | 7 | `scratch.hd` | parses |
| 24 | 8 | `hello.hd` | parses |
| 25 | 8 | `greeter/src/main.hd` | parses |
| 26 | 8 | `greeter/tests/greeting.hd` | parses; `unknown-module` is a resolution error |
| 27 | 9 | `tests/common.hd` | parses |
| 28 | 9 | `tests/checkout.hd` | parses |
| 29 | 10 | REPL input | parses |
| 30 | CLI-4 | an entry row with `Fs` | parses |
| 31 | CLI-8 | `src/cmd/migrate.hd` | parses |
| 32 | CLI-9 | `tests/common.hd` | parses |
| 33 | CLI-20 | `src/lib.hd` | parses |

No block uses syntax that a chapter does not specify. The hypothetical
items are `std` APIs, marked on their lines: `std.fs.Fs`,
`std.process.Args`, `std.process.Command`, and their methods. No block was
rejected by the compiler.

The prototype probes of case 7 used `hd check --format json`,
`hd check --tests --format json`, and `hd test --format json` on a scratch
file with a type error in a `tests:` block, and on a missing file.
