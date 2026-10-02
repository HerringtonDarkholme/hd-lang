# Command Line

Status: command-line specification draft.

This chapter is the CLI tier of the specification. It defines how the `hd`
command finds a package and what each command runs:

- package mode, workspace mode, and single files;
- `hd`, `hd FILE`, `hd run`, `hd build`, `hd check`, and `hd test`;
- the executables a manifest declares, and package tasks;
- program arguments, host capabilities, machine output, and exit status;
- `hd new`, and the REPL.

The language tier defines what a program means: its
[entry module](../lang/10-modules.md#module-initialization) and
[entry point](../lang/10-modules.md#executable-entry-point), and
[single-file programs](../lang/10-modules.md#single-file-programs). This tier
says which program a command starts. Its diagnostic codes are in the
[Diagnostics](../README.md#diagnostics) table with the language tier's.

## Commands

| Command | Effect |
| --- | --- |
| `hd` | opens the [REPL](#repl), or runs [standard input](#r-cli.stdin.program) when it is not a terminal |
| `hd FILE` | runs FILE as a [single file](#single-files) |
| `hd run`, `hd run NAME` | runs an [executable or a task](#running-a-package) of the package |
| `hd build`, `hd check`, `hd test` | work on the [package](#building-and-checking), or on one FILE |
| `hd new [--app \| --lib] [--vcs none] [PATH]` | [creates a package](#creating-a-package) |
| `hd help`, `hd --help` | prints the command list |

1. r[cli.command.help] `hd help` and `hd --help` print the command list.
2. r[cli.command.positional] A positional word of a command is a NAME or a FILE. A directory is neither: `hd test libs/ui` is an error, and its message suggests [`-p`](#selecting-members).

## Package Mode

A command works on a package when it finds that package's `hd.toml`:

| Start directory | Nearest `hd.toml` | Mode |
| --- | --- | --- |
| `shop/src/cart` | `shop/hd.toml` | package mode, package `shop` |
| `ws/libs/ui/src` | `ws/libs/ui/hd.toml`, a member of `ws` | package mode, package `ui`, with the selection and `hd.sum` of `ws` |
| `ws`, whose `hd.toml` lists members and declares no package | `ws/hd.toml` | workspace mode |
| `notes`, with no `hd.toml` above it | none | outside any package |

1. r[cli.mode.package.nearest] A command works in **package mode** exactly when the nearest `hd.toml` in its start directory or a directory above it declares a package. That `hd.toml` names the package.
2. r[cli.mode.start] The start directory is the directory of the FILE that a command names, or the working directory when it names none.
3. r[cli.mode.workspace] A command works in **workspace mode** when that nearest `hd.toml` is a workspace manifest, which lists members and declares no package.
4. r[cli.mode.outside] Otherwise the command works outside any package.
5. r[cli.mode.workspace-file] A command that names a FILE under a workspace root but in no member works outside any package, so FILE is a single-file program that may use only `std`.
6. r[cli.mode.member] In package mode, the command also searches the directories above the package's directory for the nearest workspace manifest. When that manifest lists the package as a member, the package is still the one the command works on.
7. r[cli.mode.member.workspace] Version selection, `hd.sum`, and path requirements then come from that workspace, by [Workspaces](../lang/10-modules.md#workspaces), as Cargo's do.
8. r[cli.mode.member.unlisted] When that workspace manifest neither lists the package in `members` nor in `exclude`, the command is an error that names the manifest, as Cargo's is.
9. r[cli.mode.member.unlisted.fix] The error has two fix-its: one adds the package's directory to the manifest's `members`, and the other adds it to the manifest's `exclude`.
10. r[cli.mode.member.excluded] A package whose directory the workspace manifest's `exclude` lists is not a member, and a command in it works on it as on a package outside any workspace.

> **Note.** A `src` directory without an `hd.toml` does not make a package.

> **Why.** Developers and agents mostly work inside a member. A command
> there uses the same selected versions as one at the workspace root.

## Single Files

`hd FILE` runs one file, as `python x.py` or `deno run x.ts` does:

```sh
hd notes.hd
```

1. r[cli.file.run] `hd FILE` runs FILE as a [single-file program](../lang/10-modules.md#single-file-programs), whether or not FILE lies in a package.
2. r[cli.file.in-package] When FILE lies in a package, the error for a use of `pkg`, `self`, or `super` in it suggests a task and `hd run NAME`.
3. r[cli.file.entry-hint] When FILE is the entry module of an executable or a task, that error instead names it and the exact `hd run` command. For `src/main.hd` of package `shop`, it names `hd run shop`.
4. r[cli.file.check-test] Outside any package, `hd check FILE` and `hd test FILE` check or test FILE as a single-file program.
5. r[cli.file.check-test.no-file] Outside any package, `hd check` or `hd test` without a FILE is an error whose message suggests passing a FILE or creating a package with `hd new`.

> **Why.** One file or one run uses only `std`. A program that needs
> several files or a dependency is a package.

## Running A Package

`hd run` runs the programs a package declares: its executables, which it
ships, and its tasks, which it does not.

### Executables

Each `[[executable]]` table of `hd.toml` declares one program the package
ships:

```toml
[[executable]]
name = "invoice"
module = "main"

[[executable]]
name = "migrate"
module = "tools.migrate"
```

1. r[cli.exe.table] Each `[[executable]]` table declares one **executable**: `name` names it, and `module` names its entry module by its module path under the source root.
2. r[cli.exe.several] A package may declare several executables, with unique names.
3. r[cli.exe.missing-module] An executable whose `module` names no module of the package is an error. Error: `missing-entry-point`.
4. r[cli.exe.unselected-main] A public `main` or `main!` in a module under the source root that no executable names is an ordinary function, and `hd` warns about it. Warning: `unselected-main`.
5. r[cli.exe.default-main] With no `[[executable]]` table in `hd.toml`, `src/main.hd` is the package's **default executable**, as Cargo's `src/main.rs` is.
6. r[cli.exe.main-unlisted] When `hd.toml` has an `[[executable]]` table, a `src/main.hd` that no table names is an error. Its fix-it adds a table for it, with the package's name and `module = "main"`.
7. r[cli.exe.other-module] An executable's entry module other than `src/main.hd`, such as `src/tools/migrate.hd`, resolves relative uses as an ordinary module does: `self` is its own module.
8. r[cli.exe.entry-program] Every executable's entry module is its own program and never part of the package's library, as `src/main.hd` is by [`module.path.main-file`](../lang/10-modules.md#r-module.path.main-file).
9. r[cli.exe.entry-no-use] A use of an executable's entry module from another module is an error. Error: `unknown-module`.
10. r[cli.exe.default-name] The default executable is named after the package, the `name` of its `[package]` table, as Cargo names `src/main.rs`.
11. r[cli.exe.default-name.clash] A task with the package's name therefore clashes with the default executable, by [`cli.task.name-clash`](#r-cli.task.name-clash).

```toml
[package]
name = "shop"

[[executable]]
name = "migrate"
module = "tools.migrate"
# error: src/main.hd is no executable; the fix-it adds name = "shop", module = "main"
```

> **Note.** The entry module's `main`, `main!`, or top-level statements
> decide what the executable does, by
> [Module Initialization](../lang/10-modules.md#module-initialization) and
> [Executable Entry Point](../lang/10-modules.md#executable-entry-point).

> **Why.** The manifest names the module, never the function. An agent
> that renames `main` then sees an error in source, not a stale manifest.
> An entry module is a program, as in Go and Rust. Its top level may
> print, and no dependent sees it.

### Tasks

A **task** is a development program of the package, such as a data seeder
or a release script:

```sh
hd run seed    # runs tasks/seed.hd
```

Each task is its own program, and the subdirectories of `tasks` hold the
modules that tasks share:

| File | Role |
| --- | --- |
| `tasks/build.hd` | task `build`, which uses `use self.shared.zip` |
| `tasks/seed.hd` | task `seed` |
| `tasks/shared/zip.hd` | a shared task module |

1. r[cli.task.file] Each file `tasks/NAME.hd` in the package directory is a task named `NAME`, and it is an entry module.
2. r[cli.task.program] Each task is its own program, compiled separately from the package's other tasks.
3. r[cli.task.shared] A module in a subdirectory of `tasks`, such as `tasks/shared/zip.hd`, is a **shared task module**. Every task of the package may use it.
4. r[cli.task.beside-dir] A file directly under `tasks` beside a directory of the same name, such as `tasks/shared.hd` beside `tasks/shared/`, is an error. Its fix-it moves the file to `tasks/shared/mod.hd`.
5. r[cli.task.relative] In a task or a shared task module, relative lookup works as it does under `src`, with `tasks` in place of the package root.
6. r[cli.task.root-file] A task resolves relative uses as a [root file](../lang/10-modules.md#r-module.relative.root-file) does, so its lookup starts at `tasks`. From `tasks/build.hd`, `self.shared.zip` names `tasks/shared/zip.hd`.
7. r[cli.task.super] A `super` in a task is an error. Error: `unknown-module`.
8. r[cli.task.above-root] In a shared task module, a `super` that moves above `tasks` is an error. Error: `unknown-module`.
9. r[cli.task.program-use] A use of a task from another module is an error. Error: `unknown-module`.
10. r[cli.task.shared-copy] Each task gets its own copy of the shared task modules it uses, so their top-level statements run once per task.
11. r[cli.task.shared-unused] A warning that a declaration of a shared task module is unused is given only when no task uses that declaration.
12. r[cli.task.uses] A task may use the package's modules through `pkg`.
13. r[cli.task.dev-dependencies] A task may use the package's dependencies and its [dev dependencies](../lang/10-modules.md#r-module.test.dev-dependency).
14. r[cli.task.cyclic-dev-dependency] A task may use a dev dependency that itself depends on the package, as an integration test module may.
15. r[cli.task.not-shipped] A task is never part of the package's library or executables, and a dependent package never builds it.
16. r[cli.task.name-clash] A task and an executable with the same name are an error when `hd` reads the manifest.

> **Why.** Tasks follow the test root's layout
> ([Test Modules](../lang/10-modules.md#test-modules)): a top-level file is
> a program, and a subdirectory holds what programs share. A top-level
> `x.hd` beside `x/` is a program under that layout, but the folder's
> parent under `src`, so it is rejected.

### Choosing What Runs

```sh
hd run            # the package's one executable, such as src/main.hd
hd run shop       # src/main.hd again, by the package's name
hd run migrate    # the executable or task named migrate
```

1. r[cli.run.name] In package mode, `hd run NAME` runs the executable or task named `NAME`, and is an error when there is none.
2. r[cli.run.default.one] `hd run` alone runs the package's executable when it has exactly one, declared or default. It is an error when the package has none, as a library-only package does, or several.
3. r[cli.run.file] `hd run FILE` is an error whose message suggests `hd run` or `hd run NAME`.
4. r[cli.run.package-only] Outside any package, `hd run` and `hd build` are errors whose message suggests creating a package with `hd new`.

### Working Directory

1. r[cli.run.cwd.task] A task runs with its package directory, the directory of its `hd.toml`, as its working directory.
2. r[cli.run.cwd.executable] An executable runs in the working directory of the `hd run` command.

> **Why.** A task acts on its package, as an npm script does. What it
> reads then does not depend on where the command ran. An executable is
> the user's tool, as with `cargo run`.

## Program Arguments

```sh
hd run gen -- --out x    # task gen gets the arguments --out and x
hd notes.hd -- a b       # notes.hd gets the arguments a and b
```

1. r[cli.args.separator] The words after the first `--` of a command line are arguments of the program, and `hd` reads none of them as its own.
2. r[cli.args.pass] `hd FILE` and `hd run` pass those arguments to the program they run, in order, as Cargo's `cargo run --` does.
3. r[cli.args.extra-word] A further positional word before `--`, as in `hd run gen x`, is an error whose message suggests `--`.

> **Note.** A program reads its arguments through the host capability
> trait [`Args`](../std/host.md#program-arguments)
> ([`module.entry.host-facilities`](../lang/10-modules.md#r-module.entry.host-facilities)).

## Host Capabilities

1. r[cli.host.entry-row] When `hd` runs an executable, a task, or a single file, it binds each host capability trait that its entry module's requirement row names. That row is the row of `main` or `main!`, or a [script's inferred row](../lang/10-modules.md#r-module.init.script-row).

`hd FILE`, `hd run`, and a task use the **default profile**, which binds
these traits:

| Trait | Module | What `hd` binds |
| --- | --- | --- |
| `Console` | `std.console` | `write_line!` writes to standard output, and `write_error_line!` to standard error |
| `ConsoleInput` | `std.console` | reads standard input |
| `Args` | `std.host` | the FILE or NAME the command ran, and the [program arguments](#program-arguments) |
| `Env` | `std.host` | the environment of the `hd` process |
| `Clock` | `std.time` | the system's wall clock and monotonic clock; `sleep!` waits in real time |
| `Random` | `std.random` | the operating system's random source |
| `FsRead`, `FsWrite` | `std.fs` | the file system, with relative paths from the program's [working directory](#working-directory) |

2. r[cli.host.default-profile] `hd FILE`, `hd run`, and a task run their program under the default profile, which binds the host capability traits in the table above.
3. r[cli.host.default-profile.row] The entry module's row still limits what the program gets: `hd` binds only the traits of the default profile that the row names, by [`cli.host.entry-row`](#r-cli.host.entry-row).
4. r[cli.host.default-profile.other] A row key outside the default profile is an error, as [`module.entry.row.host`](../lang/10-modules.md#r-module.entry.row.host) states. Error: `nonhost-entry-requirement`.

> **Note.** `Process` and an HTTP client are not in the default profile.
> They come later, as host extensions.

> **Why.** The row already states what the program needs, so no flag or
> manifest table repeats it. Deno asks for `--allow-read`; in hd the row
> is that permission.

## Building And Checking

```sh
hd check                  # the library and the executables
hd check --tests          # also the test code
hd check --all            # also the tasks
hd test                   # every test of the package
hd test src/billing.hd    # the tests of module billing
```

1. r[cli.package.whole] In package mode, `hd build`, `hd check`, and `hd test` without a FILE work on the whole package.
2. r[cli.package.file] With a FILE in the package, they work on that file's module, linked with the rest of the package.
3. r[cli.package.no-root] In package mode, `hd check FILE` and `hd test FILE` treat a FILE under no root as a single-file program, as `hd FILE` does. The roots are the source root, the test root, and `tasks`.
4. r[cli.check.default] A whole-package `hd check` checks what `hd build` compiles, the library and the executables, and no [test code](../lang/10-modules.md#r-module.test.code), as Cargo's `cargo check` does without `--all-targets`.
5. r[cli.check.tests] `hd check --tests` also checks the package's test code: its `tests:` blocks, test modules, and integration test modules.
6. r[cli.check.all] `hd check --all` checks the library, the executables, the test code, and the package's [tasks](#tasks).

### Test Runs

```sh
hd test --filter "sums prices"    # only the test cases whose name holds it
hd test --deny-skipped            # a skipped test case fails the run
```

1. r[cli.test.file-empty] `hd test FILE` is an error when FILE registers no test case.
2. r[cli.test.package-empty] A whole-package `hd test` that registers no test case passes.
3. r[cli.test.filter] `hd test --filter PATTERN` runs only the test cases whose name contains PATTERN, with a FILE or without one.
4. r[cli.test.filter.none] `hd test FILE --filter PATTERN` is an error when no test case of FILE has a name that contains PATTERN, as when FILE registers none.
5. r[cli.test.summary.skipped] The summary of `hd test` counts [skipped](../lang/10-modules.md#r-module.testing.skipped) test cases apart from ignored ones.
6. r[cli.test.deny-skipped] With `--deny-skipped`, a skipped test case is a failure.
7. r[cli.test.builds-executables] `hd test` builds the package's executables before it runs any test case, so an integration test may run them with [`hd_run!`](../std/testing.md#running-executables).
8. r[cli.test.process] For each [integration test module](../lang/10-modules.md#r-module.test.integration), `hd test` binds the host trait [`Process`](../lang/10-modules.md#processes) to a provider whose programs are the package's executables, each started by its name, as [`cli.exe.table`](#r-cli.exe.table) names it.
9. r[cli.test.process.missing] That provider returns `.None` for a program name that names no executable of the package.
10. r[cli.test.process.cwd] Each executable that provider starts runs with the package directory, the directory of its `hd.toml`, as its working directory.
11. r[cli.test.process.decode] The provider decodes the executable's standard output and standard error as UTF-8, and replaces each byte sequence that is not valid UTF-8 with U+FFFD.

> **Why.** Naming a FILE asks for its tests, so none is a mistake, while a
> new package may have none yet. A filter that matches nothing in a named
> FILE is most often a typo, so it does not pass silently. A changed profile can skip a whole
> suite, so CI can opt in to treating that as a failure.

## Machine Output

```sh
hd check --format json
# {"kind":"diagnostic","code":"type-mismatch","severity":"error","message":"...","file":"src/cart.hd","line":3,"column":5}
# {"kind":"summary","errors":1,"warnings":0,"passed":0,"failed":0,"skipped":0,"ignored":0,"status":101}
```

1. r[cli.json.commands] `hd build`, `hd check`, `hd test`, `hd run`, and `hd FILE` take `--format json`.
2. r[cli.json.lines.build] With it, `hd build`, `hd check`, and `hd test` write JSON lines to stdout: one JSON object per line, and no other text.
3. r[cli.json.run] With it, `hd run` and `hd FILE` write only `hd`'s own diagnostics and summary as JSON lines, and write them to stderr.
4. r[cli.json.run.program] The program's standard output passes through to stdout untouched.
5. r[cli.json.kind] Each object's `kind` field is `"diagnostic"`, `"test"`, or `"summary"`.
6. r[cli.json.diagnostic] Each diagnostic is one object, with its stable code, its severity, and its file and position.
7. r[cli.json.diagnostic.fields] A diagnostic object has the fields `code`, `severity`, `message`, `file`, `line`, and `column`.
8. r[cli.json.test] Each test case's result is one object, with the test case's name and its outcome: passed, failed, skipped, or ignored.
9. r[cli.json.test.fields] A test object has the fields `name`, `outcome`, and `message`. `outcome` is `"passed"`, `"failed"`, `"skipped"`, or `"ignored"`, and `message` holds the failure, skip, or ignore reason, or `""` when there is none.
10. r[cli.json.summary] The last object is a summary, with the count of errors, warnings, and each test outcome, and the command's exit status. It is written on success too.
11. r[cli.json.summary.fields] A summary object has the counts `errors`, `warnings`, `passed`, `failed`, `skipped`, and `ignored`, and `status`, the command's exit status.

> **Why.** A stream lets an agent act on the first error. The summary
> makes a clean run explicit, as Cargo's
> [`build-finished` message](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
> does, and a missing summary means that `hd` did not finish.

## Exit Status

| Status | Meaning |
| --- | --- |
| 0 | the command succeeded |
| 1 | `hd test`: a test case failed |
| 101 | `hd` itself failed, and ran no program |
| the program's | `hd FILE`, `hd run`: the status of the program |

1. r[cli.exit.success] A command that completes with no error exits with status 0. Warnings do not change its status.
2. r[cli.exit.hd-failure] When `hd` itself fails, it exits with status 101. It fails when it reports an error, rejects its command line or a manifest, or cannot finish for an internal reason.
3. r[cli.exit.program] Once its program is built, `hd FILE` or `hd run` exits with the program's own status, by [Exit Status](../lang/10-modules.md#exit-status) and [`module.entry.panic`](../lang/10-modules.md#r-module.entry.panic).
4. r[cli.exit.test-failure] `hd test` exits with status 1 when a test case fails and `hd` reports no error.

> **Why.** Cargo reserves 101 for its own failures. CI and agents then
> tell "my code did not compile" apart from "my program failed", which
> most often exits with 1.

## Workspace Mode

At a workspace root, commands act on the members, as Cargo's do:

```sh
cd ws
hd test           # the tests of every member
hd run invoice    # the one member executable or task named invoice
```

1. r[cli.workspace.members] In workspace mode, `hd check`, `hd test`, and `hd build` act on every member of the workspace.
2. r[cli.workspace.run-name] In workspace mode, `hd run NAME` runs the executable or task named `NAME` when exactly one member has one.
3. r[cli.workspace.run-ambiguous] When several members have one, `hd run NAME` is an error whose message lists those members.
4. r[cli.workspace.run-missing] When no member has one, `hd run NAME` is an error.
5. r[cli.workspace.run-bare] In workspace mode, `hd run` with no NAME is an error whose message lists each member's executables and tasks.
6. r[cli.workspace.repl] In workspace mode, the REPL session may use only `std`.

### Selecting Members

```sh
hd test -p ui -p shared    # the tests of members ui and shared
hd run -p web serve        # serve of member web
cd libs/ui && hd test -p shared    # from inside member ui, the tests of shared
```

1. r[cli.workspace.select.anywhere] `-p NAME` or `--package NAME` selects the member whose package is named `NAME`, wherever a command finds a workspace: at its root, or inside a member by [`cli.mode.member`](#r-cli.mode.member). It works on `hd run`, `hd test`, `hd check`, and `hd build`.
2. r[cli.workspace.select.repeat] The flag may be repeated, and the command then acts on the selected members only, by the rules above.
3. r[cli.workspace.select.unknown] A `-p NAME` that names no member of the workspace is an error.

> **Why.** CI and agents usually run from the root. A directory argument
> would read as `hd run NAME`, so a flag selects members, as Cargo's `-p`
> does.

## Names

1. r[cli.name.hyphen] A package's name and an executable's name may contain `-`, as Cargo's do.
2. r[cli.name.task] A task's name is its file name, so it is an identifier and contains no `-`.

> **Note.** Where source needs an identifier for such a name, each `-`
> becomes `_`, as the dependency key `my-app` is `dep.my_app`
> ([`module.dep.key-name`](../lang/10-modules.md#r-module.dep.key-name)).

## Creating A Package

`hd new` is the one command that creates a package:

```sh
hd new --app hello   # hello/hd.toml, src/main.hd, and tests/hello.hd, in a new git repository
hd new --lib util    # util/hd.toml, src/lib.hd, and tests/util.hd
hd new --app         # an application in the working directory
hd new hello         # asks which kind, or fails without a terminal
cd hello
hd run               # runs src/main.hd
hd test              # runs tests/hello.hd, which runs the executable
```

1. r[cli.new.kind] `hd new` creates an application with `--app` and a library with `--lib`.
2. r[cli.new.kind.ask] With neither flag, `hd new` asks which kind to create when standard input is a terminal.
3. r[cli.new.kind.no-terminal] With neither flag and standard input not a terminal, `hd new` is an error whose message names `--app` and `--lib`. It never picks a kind itself.
4. r[cli.new.app] `hd new --app PATH` creates the directory PATH, holding an `hd.toml`, a `src/main.hd` whose program prints `hello, world`, and an integration test `tests/NAME.hd`.
5. r[cli.new.app.test] That test runs the executable with [`hd_run!`](../std/testing.md#running-executables), and checks its output and its exit status.
6. r[cli.new.lib] `hd new --lib PATH` creates the directory PATH, holding an `hd.toml`, a `src/lib.hd` that declares one sample public function, and an integration test `tests/NAME.hd`.
7. r[cli.new.lib.test] That test checks the sample function through the package's public interface, with `use pkg`.
8. r[cli.new.test-name] In both, `NAME` is the package's name with each `-` replaced by `_`, so the file is a valid module path.
9. r[cli.new.here.dir] With no PATH, or with `.`, `hd new` does the same in the working directory.
10. r[cli.new.existing] `hd new` is an error when any file it would write already exists, and it then writes nothing.
11. r[cli.new.no-executable-table] The `hd.toml` that `hd new` writes has no `[[executable]]` table. With `--app`, `src/main.hd` is then the default executable, and `hd run` runs it right after `hd new`.
12. r[cli.new.workspace-member] When the new package's directory lies under a workspace root, `hd new` also adds that directory to the workspace manifest's `members`, as Cargo does.
13. r[cli.new.vcs] Unless the new package's directory is already inside a git repository, `hd new` runs `git init` there and writes a `.gitignore`, as Cargo does.
14. r[cli.new.vcs.ignore] That `.gitignore` lists only the directory where `hd` writes its build and cache output. `hd.sum` is not listed, so it is committed.
15. r[cli.new.vcs-none] `hd new --vcs none` runs no `git init` and writes no `.gitignore`.

> **Note.** [Running Executables](../std/testing.md#running-executables)
> shows the test that `hd new --app` writes.

> **Why.** A new package starts with a passing test in `tests/`. Where
> tests go, and how they reach the package, is then visible from the first
> run.
> A package left out of `members` would be skipped by every root command
> without a word.

## REPL

```sh
cd shop/src/billing
hd                # use self.util names shop/src/util.hd
echo 'println(1 + 2)' | hd    # prints 3
```

1. r[cli.repl.open.terminal] `hd` with no arguments opens the REPL when standard input is a terminal.
2. r[cli.stdin.program] When standard input is not a terminal, `hd` with no arguments runs all of it as a [single-file program](../lang/10-modules.md#single-file-programs), as `python` does. This holds in every mode.
3. r[cli.repl.package.lib] In package mode, the session acts as code inside `src/lib.hd`, wherever in the package it starts. It sees the private declarations of `src/lib.hd`, and `use self.util` names `src/util.hd`.
4. r[cli.repl.package.dependencies] In package mode, the session may use the package's dependencies and its dev dependencies.
5. r[cli.repl.package.no-lib] In a package without `src/lib.hd`, the session may still use the public declarations of the package's modules through `self`, as `src/main.hd` does, and the package's dependencies, its dev dependencies, and `std`.
6. r[cli.repl.package.no-lib.main] `src/main.hd` itself stays unusable from the session, as it is from every module by [`module.path.main-no-use`](../lang/10-modules.md#r-module.path.main-no-use).
7. r[cli.repl.uses.other] Apart from the declarations of `src/lib.hd`, names reach the session only through its `use` declarations.
8. r[cli.repl.outside] Outside any package, the session may use only `std`.

> **Why.** An agent that pipes code into `hd` gets a run, not a prompt
> that waits for a terminal.

## Package Tooling

1. r[cli.tooling.package-schema] The complete `hd.toml` schema, the `hd.sum` format, and the commands that fetch, add, and upgrade dependencies belong to package tooling.
2. r[cli.tooling.package-later] Compatibility checks at release and upgrade, vendoring, and local-path patches are package tooling that this chapter does not define.

See also: [Package Manifest](../lang/10-modules.md#package-manifest),
[Workspaces](../lang/10-modules.md#workspaces),
[Test Modules](../lang/10-modules.md#test-modules).
