# Command Line

Status: command-line specification draft.

This chapter is the CLI tier of the specification. It defines how the `hd`
command finds a package and what each command runs:

- package mode and single files;
- `hd`, `hd FILE`, `hd run`, `hd build`, `hd check`, and `hd test`;
- the executables a manifest declares, and package tasks;
- `hd new`, and the REPL.

The language tier defines what a program means: its
[entry module](10-modules.md#module-initialization) and
[entry point](10-modules.md#executable-entry-point), and
[single-file programs](10-modules.md#single-file-programs). This tier
says which program a command starts. Its diagnostic codes are in the
[Diagnostics](README.md#diagnostics) table with the language tier's.

## Commands

| Command | Effect |
| --- | --- |
| `hd` | opens the [REPL](#repl) |
| `hd FILE` | runs FILE as a [single file](#single-files) |
| `hd run`, `hd run NAME` | runs an [executable or a task](#running-a-package) of the package |
| `hd build`, `hd check`, `hd test` | work on the [package](#building-and-checking), or on one FILE |
| `hd new [PATH]` | [creates a package](#creating-a-package) |
| `hd help`, `hd --help` | prints the command list |

1. r[cli.command.help] `hd help` and `hd --help` print the command list.

## Package Mode

A command works on a package when it finds that package's `hd.toml`:

| Start directory | Nearest `hd.toml` | Mode |
| --- | --- | --- |
| `shop/src/cart` | `shop/hd.toml` | package mode, package `shop` |
| `notes`, with no `hd.toml` above it | none | outside any package |

1. r[cli.mode.package] A command works in **package mode** exactly when an `hd.toml` lies in its start directory or a directory above it. The nearest such `hd.toml` names the package.
2. r[cli.mode.start] The start directory is the directory of the FILE that a command names, or the working directory when it names none.
3. r[cli.mode.outside] Otherwise the command works outside any package.

> **Note.** A `src` directory without an `hd.toml` does not make a package.

## Single Files

`hd FILE` runs one file, as `python x.py` or `deno run x.ts` does:

```sh
hd notes.hd
```

1. r[cli.file.run] `hd FILE` runs FILE as a [single-file program](10-modules.md#single-file-programs), whether or not FILE lies in a package.
2. r[cli.file.in-package] When FILE lies in a package, the error for a use of `pkg`, `self`, or `super` in it suggests a task and `hd run NAME`.
3. r[cli.file.check-test] Outside any package, `hd check FILE` and `hd test FILE` check or test FILE as a single-file program.

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

> **Note.** The entry module's `main`, `main!`, or top-level statements
> decide what the executable does, by
> [Module Initialization](10-modules.md#module-initialization) and
> [Executable Entry Point](10-modules.md#executable-entry-point).

> **Why.** The manifest names the module, never the function. An agent
> that renames `main` then sees an error in source, not a stale manifest.

### Tasks

A **task** is a development program of the package, such as a data seeder
or a release script:

```sh
hd run seed    # runs tasks/seed.hd
```

1. r[cli.task.file] Each file `tasks/NAME.hd` in the package directory is a task named `NAME`, and it is an entry module.
2. r[cli.task.uses] A task may use the package's modules through `pkg`.
3. r[cli.task.not-shipped] A task is never part of the package's library or executables, and a dependent package never builds it.
4. r[cli.task.name-clash] A task and an executable with the same name are an error when `hd` reads the manifest.

### Choosing What Runs

```sh
hd run            # the package's one executable
hd run migrate    # the executable or task named migrate
```

1. r[cli.run.name] In package mode, `hd run NAME` runs the executable or task named `NAME`, and is an error when there is none.
2. r[cli.run.default] `hd run` alone runs the package's executable when it declares exactly one. It is an error when the package declares none, as a library-only package does, or several.
3. r[cli.run.file] `hd run FILE` is an error whose message suggests `hd run` or `hd run NAME`.
4. r[cli.run.package-only] Outside any package, `hd run` and `hd build` are errors whose message suggests creating a package with `hd new`.

## Building And Checking

```sh
hd test                   # every test of the package
hd test src/billing.hd    # the tests of module billing
```

1. r[cli.package.whole] In package mode, `hd build`, `hd check`, and `hd test` without a FILE work on the whole package.
2. r[cli.package.file] With a FILE in the package, they work on that file's module, linked with the rest of the package.

## Creating A Package

`hd new` is the one command that creates a package:

```sh
hd new hello    # hello/hd.toml, hello/src/main.hd, hello/tests/
hd new          # the same, in the working directory
```

1. r[cli.new.path] `hd new PATH` creates the directory PATH, holding an `hd.toml`, a `src/main.hd`, and a `tests` directory.
2. r[cli.new.here] `hd new` and `hd new .` do the same in the working directory, and are an error when it already holds an `hd.toml`.

## REPL

```sh
cd shop/src/billing
hd                # use self.util names shop/src/util.hd
```

1. r[cli.repl.open] `hd` with no arguments opens the REPL.
2. r[cli.repl.package] In package mode, the session is linked with the package and acts as its root module, wherever in the package it starts: `use self.util` names `src/util.hd`.
3. r[cli.repl.uses] Names reach the session only through its `use` declarations.
4. r[cli.repl.outside] Outside any package, the session may use only `std`.

## Package Tooling

1. r[cli.tooling.package-schema] The complete `hd.toml` schema, the `hd.sum` format, and the commands that fetch, add, and upgrade dependencies belong to package tooling.
2. r[cli.tooling.package-later] Compatibility checks at release and upgrade, vendoring, and local-path patches are package tooling that this chapter does not define.

See also: [Package Manifest](10-modules.md#package-manifest),
[Test Modules](10-modules.md#test-modules).
