# Command Line

Status: command-line specification draft.

This chapter is the CLI tier of the specification. It defines how the `hd`
command finds a package and what each command runs:

- package mode, workspace mode, and single files;
- `hd`, `hd FILE`, `hd run`, `hd build`, `hd check`, and `hd test`;
- the executables a manifest declares, and package tasks;
- program arguments, host capabilities, machine output, and exit status;
- `hd doc`, which writes a package's documentation;
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
| `hd doc [--private] [--out DIR] [--open]`, `hd doc NAME` | [writes the package's documentation](#documentation), or prints one item's |
| `hd new [--app \| --lib] [--pages] [--vcs none] [PATH]` | [creates a package](#creating-a-package) |
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
5. r[cli.host.default-profile.console-closed] Under the default profile, `write_line!` and `write_error_line!` return `.Err(ConsoleError.Closed)` when the host cannot write the line, as when the reader of a pipe has closed it.
6. r[cli.host.default-profile.input-closed] `read_line!` returns `.Err(ConsoleError.Closed)` when standard input is not attached or a read fails. The end of input is `.Ok(.None)`, and every later call returns `.Ok(.None)` again.

> **Why.** A closed pipe makes `println` panic, by
> [`module.console.println-error`](../lang/10-modules.md#r-module.console.println-error),
> as Rust's `println!` does; `hd FILE | head -1` stops the program
> instead of writing into nothing.

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
6. r[cli.check.tests.doc] `hd check --tests` also checks the package's [doc tests](../lang/10-modules.md#doc-tests), except compile-fail ones, which only `hd test` judges.
7. r[cli.check.all] `hd check --all` checks the library, the executables, the test code, and the package's [tasks](#tasks).

### Test Runs

```sh
hd test --filter "sums prices"    # only the test cases whose name holds it
hd test --deny-skipped            # a skipped test case fails the run
hd test --filter doc              # only the doc tests
hd test --filter text.slugify     # only the doc tests of slugify in src/text.hd
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
12. r[cli.test.runner] When it runs a test case, `hd test` binds the host traits [`TestRunner` and `PropertyRunner`](../std/testing.md#runner-capabilities) for the `std.testing` code around the body, by [`std-testing.runner.binding`](../std/testing.md#r-std-testing.runner.binding).
13. r[cli.test.doc.default] `hd test` runs the package's [doc tests](../lang/10-modules.md#doc-tests) with its other test cases. With a FILE, it runs the doc tests of that file's module.
14. r[cli.test.doc.name] A doc test is named `doc <module>.<item>[i]`, by the table below.
15. r[cli.test.doc.filter] `--filter` matches a doc test by that name, so `--filter doc` selects every doc test, and `--filter text.slugify` the doc tests of `slugify` in `src/text.hd`.
16. r[cli.test.doc.location] A diagnostic or a failure of a doc test names the `.hd` source file and a `##` line of the doc test's block.
17. r[cli.test.doc.json] With `--format json`, a doc test's test object holds that name in `name`, and its diagnostics give that file and line in `file` and `line`.
18. r[cli.test.doc.name.root] For a block in `src/lib.hd`, `<module>` is `pkg`, which names the package root module by [`module.path.lib-file`](../lang/10-modules.md#r-module.path.lib-file).
19. r[cli.test.doc.name.member] For a block on a member, `<item>` is `Type.member`, so the first block on `new` of `Slug` in `src/text.hd` is `doc text.Slug.new[0]`.
20. r[cli.test.doc.name.module] A block in a module's [module documentation](../lang/01-lexical-structure.md#r-lex.doc.module) has no `<item>` and no dot. The first such block in `src/text.hd` is `doc text[0]`.
21. r[cli.test.doc.update] An update run, as `hd test --update` makes, rewrites a failing doc test `snapshot`'s expected text in place, inside its block's `##` lines.

| Part | Value |
| --- | --- |
| `<module>` | the path of the module whose comment holds the block, such as `text` for `src/text.hd`, and `pkg` for `src/lib.hd` |
| `<item>` | for a block on a declaration, the name of the documented declaration, such as `slugify`; for a member, the declaration's name, a dot, and the member's name, such as `Slug.new` |
| `[i]` | the block's index among that item's doc tests, from 0, as in the `name[i]` of an `it_each` row |

So the first `hd` block on `slugify` in `src/text.hd` is `doc text.slugify[0]`,
and the JSON line of its result is
`{"kind":"test","name":"doc text.slugify[0]","outcome":"passed","message":""}`.
The first block on `slugify` in `src/lib.hd` would be `doc pkg.slugify[0]`.

> **Why.** Naming a FILE asks for its tests, so none is a mistake, while a
> new package may have none yet. A filter that matches nothing in a named
> FILE is most often a typo, so it does not pass silently. A changed profile can skip a whole
> suite, so CI can opt in to treating that as a failure.

## Documentation

`hd doc` writes a package's documentation, built from its declarations and
their `##` [documentation comments](../lang/01-lexical-structure.md#documentation-comments):

```sh
hd doc                   # writes the pages into the build directory's doc/
hd doc --open            # also opens index.html, the entry page
hd doc --private         # also documents the private items
hd doc --out site        # writes the pages into site/
hd doc text.slugify      # prints the Markdown of one item
hd doc std.text.split    # prints a std item and links to its spec
```

A module's page looks like this, in Markdown:

````markdown
# Module `text`
Text helpers for URLs and titles.

- [`slugify`](#slugify): Turns a title into a URL slug.

<a id="slugify"></a>
## `slugify`
```hd
pub fn slugify(title: string) -> string
```
Turns a title into a URL slug: each space becomes a hyphen.
See `Slug` for a checked slug.

Example `doc text.slugify[0]`:
```hd
use pkg.text.{slugify}
use std.testing.assert_equal

assert_equal(slugify("Ship It Now"), "Ship-It-Now", reason="each space becomes a hyphen")
```
````

### Scope

1. r[cli.doc.command] `hd doc` writes the HTML and the Markdown of the package's documentation together. No flag selects one format.
2. r[cli.doc.package] `hd doc` works on the package of [package mode](#package-mode), and is an error outside any package.
3. r[cli.doc.workspace] In workspace mode, `hd doc` needs `-p NAME`, by [Selecting Members](#selecting-members), to select one member. Without it, `hd doc` is an error.
4. r[cli.doc.check] `hd doc` checks the package as `hd check` does. When that reports an error, `hd doc` writes and prints no documentation.
5. r[cli.doc.markdown] The text of a documentation comment is Markdown, as CommonMark defines it.
6. r[cli.doc.items] `hd doc` documents each `pub` item of every module under the source root, with the `pub` members of each, as the table below lists. Items are functions, data types, enums, traits, and type aliases.
7. r[cli.doc.private] `--private` also documents the private items and members. It applies to the package only, and to `hd doc NAME` as to the pages.
8. r[cli.doc.main] An application's `src/main.hd` is no library module, so it has a page only with `--private`. That page is the module `main`, as `main.md` and `main.html`.
9. r[cli.doc.reexport] A `pub use` of a module is listed under that module, with a link to the page of the module that declares the item. It copies no signature and no doc, since the item keeps its [identity](../lang/10-modules.md#r-module.pub-use.identity).

| Item | The signature block holds |
| --- | --- |
| function | the declaration through its result type and requirement row, with no body |
| data type | the declaration head and its `pub` fields; the line `# private fields omitted` stands for the private ones |
| enum | the declaration head and its variants with their payloads |
| trait | the declaration head and the signature of each method |
| type alias | the whole declaration |
| `pub` method of an inherent `impl` | the method's signature, under the type's heading |

### Output

1. r[cli.doc.dir] `hd doc` writes into the directory `doc` of the build directory, which is where `hd` writes its build and cache output. `--out DIR` writes into DIR instead.
2. r[cli.doc.open] `--open` opens the entry page `index.html` of the output with the platform's default program, after the files are written.
3. r[cli.doc.files] The output holds the files of the table below. A module's HTML and Markdown pages sit side by side.
4. r[cli.doc.root-page] The root module's pages are named `pkg`, as [`module.path.reserved-pkg`](../lang/10-modules.md#r-module.path.reserved-pkg) reserves the name, so no user module's page takes their file names.
5. r[cli.doc.relative] A link between the output's files is a relative path, so the output works from any base path.
6. r[cli.doc.html] A module's HTML page holds everything its Markdown page holds, under the same anchors. Its styling and navigation are not specified.

| Files | Hold |
| --- | --- |
| `index.md`, `index.html` | a small entry page: the package name, a link to the root module `pkg` when `src/lib.hd` exists, and the list of the other modules |
| `pkg.md`, `pkg.html` | the root module `pkg`, which is `src/lib.hd`; present only when `src/lib.hd` exists |
| `PATH.md`, `PATH.html` | one pair for each other module under the source root; `PATH` is the module path with each `.` a `/`, so module `shop.cart` is `shop/cart.md` |
| `llms.txt` | the index for agents, by [`cli.doc.llms`](#r-cli.doc.llms) |
| `llms-full.txt` | the whole documentation in one file, by [`cli.doc.llms-full`](#r-cli.doc.llms-full) |

### Pages

1. r[cli.doc.page] A module's Markdown page starts with the heading `` # Module `PATH` ``, which reads `` # Module `pkg` `` for the root module, then the module's documentation, then a list of its items. Each list entry links to the item's anchor and ends with the item's summary.
2. r[cli.doc.summary] An item's **summary** is the first sentence of its documentation, or empty when it has none.
3. r[cli.doc.item] Each item has a second-level heading with its name in code, and each member a third-level heading with `Type.member` in code. Below the heading come the signature, the documentation, and the examples.
4. r[cli.doc.anchor] An item's anchor is its item name, as [`cli.test.doc.name`](#r-cli.test.doc.name) forms `<item>`, such as `slugify` or `Slug.new`.
5. r[cli.doc.signature] A signature is the source text of the declaration, copied as written, in a fenced block whose info string is `hd`.
6. r[cli.doc.impls] A type lists the traits it implements, one line for each implementation head. That includes implementations in other modules of the package. A derived implementation is marked `(derived)`.
7. r[cli.doc.concise] A page holds no function body and no navigation text.
8. r[cli.doc.example] Each [doc test](../lang/10-modules.md#doc-tests) appears in place, as written, in an `hd` block. The word `Example` and the doc test's name in code, as [`cli.test.doc.name`](#r-cli.test.doc.name) forms it, come before the block.
9. r[cli.doc.example.compile-fail] A compile-fail doc test is labelled `Does not compile (CODE)`, with its error code, and its block keeps the `# error: CODE` line.

### Pages For Agents

1. r[cli.doc.llms] `llms.txt` starts with a heading of the package's name. Then it holds a blockquote with the summary of the root module's documentation, and a `## Modules` list.
2. r[cli.doc.llms.modules] The list has one entry for each module, `pkg` first: a link to its `.md` page, a colon, and the summary of the module's documentation.
3. r[cli.doc.llms-full] `llms-full.txt` holds the Markdown of every module page, the root module first and the others in path order.

### Links

1. r[cli.doc.link.form] A documentation comment links to an item with the Markdown shortcut `` [`NAME`] ``, as in `` [`Slug.new`] ``. A Markdown link with a target is ordinary Markdown and is not checked.
2. r[cli.doc.link.scope] NAME resolves in the module scope of the documented item: its own declarations and the names its `use` declarations bind. A dotted NAME names a member or a path through modules.
3. r[cli.doc.link.resolved] A resolved link becomes a link to the target's anchor, on the target's page.
4. r[cli.doc.link.external] A NAME that resolves to a `std` item links to the page for that item in the standard library reference. A NAME that resolves into a dependency is checked, but shows as plain code.
5. r[cli.doc.link.broken] A NAME that resolves to no item is a warning, as is one that resolves to a private item the run does not document. The text shows as plain code. Warning: `broken-doc-link`.

### Looking Up One Item

1. r[cli.doc.name] `hd doc NAME` prints the Markdown of one item to standard output, and writes no file. It prints the item's section of the module page, from its heading to the next heading of the same or a higher level.
2. r[cli.doc.name.form] NAME is a module path and an item name joined by a dot, as in `text.slugify` or `text.Slug.new`. The module is the longest prefix that names a module, and `pkg` names the root module.
3. r[cli.doc.name.module] A NAME that names only a module prints that module's whole Markdown page.
4. r[cli.doc.name.dependency] A NAME that starts with `dep.` and a dependency's key, as in `dep.json.parse`, names an item of that dependency. Only its `pub` items are found.
5. r[cli.doc.name.std] A NAME that starts with `std.`, as in `std.text.split`, names an item of the standard library: `std`, a module, and an item of that module.
6. r[cli.doc.name.std.output] It prints the item's signature in an `hd` block, its summary, and a link to the item's section of the standard library reference.
7. r[cli.doc.name.missing] A NAME that names no documented item is an error. That includes a `std` NAME that names no item the standard library reference specifies.

> **Why.** The Markdown pages let an agent fetch one module in one
> request, and `hd doc NAME` answers a lookup without any file. A broken
> link is a warning, so a rename does not block a build. Anchors equal
> doc-test names, so one name finds the item, its page section, and its
> examples.

See also: [Doc Tests](../lang/10-modules.md#doc-tests),
[Documentation Comments](../lang/01-lexical-structure.md#documentation-comments),
[Creating A Package](#creating-a-package).

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
8. r[cli.json.diagnostic.file] In a package, `file` is the path relative to the package root, as `src/cart.hd`, whatever the working directory.
9. r[cli.json.test] Each test case's result is one object, with the test case's name and its outcome: passed, failed, skipped, or ignored.
10. r[cli.json.test.fields] A test object has the fields `name`, `outcome`, and `message`. `outcome` is `"passed"`, `"failed"`, `"skipped"`, or `"ignored"`, and `message` holds the failure, skip, or ignore reason, or `""` when there is none.
11. r[cli.json.test.order] `hd test` lists the test objects in the order of the files' paths, and within one file in declaration order.
12. r[cli.json.summary] The last object is a summary, with the count of errors, warnings, and each test outcome, and the command's exit status. It is written on success too.
13. r[cli.json.summary.fields] A summary object has the counts `errors`, `warnings`, `passed`, `failed`, `skipped`, and `ignored`, and `status`, the command's exit status.

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
hd new --lib util --pages    # also writes .github/workflows/docs.yml
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
16. r[cli.new.pages] `hd new --pages` also writes `.github/workflows/docs.yml` in the new package's directory, for an application and for a library.
17. r[cli.new.pages.workflow] That workflow runs [`hd doc`](#documentation) and deploys the output directory to GitHub Pages.
18. r[cli.new.pages.ask] When `hd new` asks which kind to create, by [`cli.new.kind.ask`](#r-cli.new.kind.ask), it also asks whether to publish the documentation to GitHub Pages. A yes is `--pages`.
19. r[cli.new.pages.default] Without `--pages`, and without that question, `hd new` writes no workflow file.

> **Note.** [Running Executables](../std/testing.md#running-executables)
> shows the test that `hd new --app` writes.

The workflow file that `--pages` writes looks like this. Its exact text is
not specified:

```yaml
# Written by `hd new --pages` (hd 0.4.0).
name: Docs
on:
  push:
    branches: [main]
  workflow_dispatch:
permissions:
  contents: read
  pages: write
  id-token: write
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  docs:
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deploy.outputs.page_url }}
    steps:
      - uses: actions/checkout@v5
      - uses: hd-lang/setup-hd@0.4.0
      - run: hd doc --out _site
      - uses: actions/upload-pages-artifact@v4
        with:
          path: _site
      - id: deploy
        uses: actions/deploy-pages@v4
```

> **Note.** The workflow installs `hd` with a `setup-hd` action, pinned to
> the version of `hd` that wrote the file. That action is not published
> yet, so a workflow written today cannot run until it is. The repository's
> Pages source must also be set to "GitHub Actions". `hd new` cannot set it.

> **Note.** [`cli.new.existing`](#r-cli.new.existing) covers the workflow
> file too: when it exists already, `hd new` writes nothing.

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
9. r[cli.repl.input-body] Each input of a session is its own body for [Open Literal Width](../lang/04-type-system.md#open-literal-width): its open variables fall back at the end of that input. A later input never changes the type of an earlier input's binding.

> **Note.** An input such as `x := 21` has no signed literal, so `x`
> falls back to `usize`, and a session that shows types shows `x * 2` as a
> `usize`. `y := -21` is an `i32`, by
> [`types.literal.open.int-fallback`](../lang/04-type-system.md#r-types.literal.open.int-fallback).

> **Why.** An agent that pipes code into `hd` gets a run, not a prompt
> that waits for a terminal.

## Package Tooling

1. r[cli.tooling.package-schema] The complete `hd.toml` schema, the `hd.sum` format, and the commands that fetch, add, and upgrade dependencies belong to package tooling.
2. r[cli.tooling.package-later] Compatibility checks at release and upgrade, vendoring, and local-path patches are package tooling that this chapter does not define.

See also: [Package Manifest](../lang/10-modules.md#package-manifest),
[Workspaces](../lang/10-modules.md#workspaces),
[Test Modules](../lang/10-modules.md#test-modules).
