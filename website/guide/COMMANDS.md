# The `hd` Command

This page is the task-oriented guide to the `hd` command. The normative
reference is [Command Line](../../spec/cli/command-line.md); this page shows
each job the way you would do it, with real output.

## Start A Project

You want a new package. `hd new` needs to know which kind: `--app` for an
application, `--lib` for a library:

```sh
$ hd new orders --app
created application package `orders` in /home/you/orders
```

The layout is small: an `hd.toml` manifest, `src/main.hd` for the entry
point, and `tests/` for integration tests:

```sh
orders/hd.toml
orders/src/main.hd
orders/tests/orders.hd
```

The manifest names the package; everything else is convention:

```toml
[package]
name = "orders"
```

Read next: [Package Mode](../../spec/cli/command-line.md#package-mode).

## Run A Program

You want to see it run. Inside a package, `hd run` runs its executable;
`hd FILE` runs one file anywhere:

```sh
$ hd run
hello, world
```

With several executables or a task, name it: `hd run NAME` runs the
executable or the `tasks/NAME.hd` script. `hd run --release` runs the
release build. In a workspace, `-p NAME` selects the member, from the root
or inside another member:

```sh
$ hd run seed    # runs tasks/seed.hd
seeded
```

Read next: [Running A Package](../../spec/cli/command-line.md#running-a-package)
and [Tasks](../../spec/cli/command-line.md#tasks).

An implementation may offer more than one engine to run programs on.
`hd FILE`, `hd run`, `hd build` and `hd test` take `--engine NAME`, and
`HD_ENGINE` gives the name when the flag is absent. Which engines exist,
and which is the default, depends on your `hd`. A program behaves the same
on each; only speed and memory use differ. A name your `hd` does not offer
is a usage error that exits 101 and runs nothing:

```sh
$ hd run --engine no-such-name
$ echo $?
101
```

Read next: [Engines](../../spec/cli/command-line.md#engines).

## Check And Build

You want to know it compiles, without running it. `hd check` type-checks
the package, and `hd build` writes the Wasm:

```sh
$ hd check
check result: ok. errors: 0; warnings: 0
$ hd build
$ hd build --release
```

The outputs land in `build/debug/` and `build/release/`:

```sh
build/debug/orders.wasm
build/release/orders.wasm
```

Read next: [Building And Checking](../../spec/cli/command-line.md#building-and-checking).

## Test

You want the tests. `hd test` runs every test of the package; `--filter`
runs only the cases whose name contains a word:

```sh
$ hd test
test result: ok. 1 passed; 0 failed; 0 ignored
$ hd test --filter "prints"
test result: ok. 1 passed; 0 failed; 0 ignored
```

A unit test runs on fakes alone; an integration test under `tests/` gets
the real system and can run the built program with `hd_run!`. A doc test
is a fenced `hd` block in a `##` doc comment. A snapshot test compares
with a recorded file, and `hd test --update` records it. For agents, `hd
test --format json` prints one JSON line per case plus a summary:

```sh
$ hd test --format json
{"kind":"test","name":"prints a greeting","outcome":"passed","message":""}
{"kind":"summary","errors":0,"warnings":0,"passed":1,"failed":0,"ignored":0,"status":0}
```

You edit one module and want feedback in seconds, not the whole suite.
After a full pass, `hd` records each test program's fingerprint: its
sources, dependencies, manifest settings, and `hd` version. `hd test
--affected` then runs only the programs whose fingerprint changed, and the
summary counts the rest:

```sh
$ hd test --affected
test result: ok. 0 passed; 0 failed; 0 ignored; 1 program skipped
```

A filtered run or a failing program records nothing, so `--affected` never
skips a test that failed or did not run. `hd clean` removes the records.

A runaway loop should fail, not hang. `--time-limit` gives each test case,
and `hd run` and `hd FILE` give the program, a deadline; past it the
program panics with `time-limit`:

```sh
$ hd test --time-limit 5s
$ hd run --time-limit 30s
panic: time-limit: the program exceeded its time limit
```

A Ctrl-C asks a waiting program to stop and lets its cleanup finish:
`using` disposes and `defer` suites run, for up to ten seconds, then
whatever is left is abandoned. A second Ctrl-C abandons it at once.
`--cleanup-limit` changes the wait, and the run exits with status 130:

```sh
$ hd run --cleanup-limit 2s
^C
cancelled
```

Read next: [Test Runs](../../spec/cli/command-line.md#test-runs),
[Affected Tests](../../spec/cli/command-line.md#affected-tests),
[Program Limits](../../spec/cli/command-line.md#program-limits), and
[Cancelling A Run](../../spec/cli/command-line.md#cancelling-a-run).

## Dependencies And Workspaces

You want another package's code. A requirement is a minimum version of a
host path, or a local path while you develop:

```toml
[dependencies]
json = "github.com/acme/json@2.1.0"
billing = { path = "../billing" }
```

`hd add`, `hd update`, `hd remove`, and `hd fetch` manage the table and
`hd.sum`, the recorded hashes beside the manifest. Several packages of one
repository share one selection through a workspace manifest:

```toml
[workspace]
members = ["libs/ui", "libs/shared"]
```

Read next: [Dependency Commands](../../spec/cli/command-line.md#dependency-commands)
and [Workspaces](../../spec/lang/10-modules.md#workspaces).

## Debug And Explore

You want to see a value while it runs, or read what a function promises.
`dbg(x)` prints the value with its source location on standard error.
`hd doc NAME` prints one item's documentation, and no file is written:

````sh
$ hd doc std.text.split
```hd
pub fn split(self, separator: string) -> List[string]
```

Splits the string around each occurrence of `separator`, in order.

Reference: std.text.string.split in the standard library reference
````

NAME is a module path and an item, as in `text.slugify` for your own
package, or `std.text.split` for the standard library, where the output
is the signature, a summary, and a reference link. A module name
alone, as in `hd doc std.text`, prints the module's page. `hd help
COMMAND` prints one command's usage and flags.

`hd` with no arguments on a terminal opens the interactive session; each
line is evaluated and printed with its type. Text piped into `hd` runs as
a program instead, so a session cannot be scripted through standard input.
A later input may use the top-level bindings and declarations of earlier
ones, but may not redeclare a name an earlier input declared
(`duplicate-binding`, `duplicate-type`); to change a declaration, start a
new session or give the new one another name. An input that panics adds
nothing to the session, and the session goes on. An input that starts with
`:` is a command: `:help` lists them, `:quit` (or `:q`, `:exit`) ends the
session, and `:reload` reads the package's files again after you edit
them, keeping the inputs that still check.

```sh
$ hd
1 + 2
3 : usize
```

Every diagnostic command takes `--format json` for one JSON object per
diagnostic. Read next:
[Machine Output](../../spec/cli/command-line.md#machine-output).

## Common Errors And Their Fixes

- `missing-requirement`: the call needs a capability, such as `Console`,
  that the enclosing signature does not grant. Add it to the row, as in
  `pub fn main() -> void $ Console`, or bind a provider in a test.
- `invalid-result-propagation`: `?` needs the function's own result type
  to be a compatible `Result`; the note names the type to write.
- `non-reassignable-binding`: `:=` bindings are fixed; write `let` for a
  name you reassign.
- `type-mismatch` on `usize` versus `i32`: literals default to `usize`;
  write `+N` for a signed one.
- `mixed-signedness`: an unsigned `usize`, which `len()` and literals
  give, does not mix with a signed `i32`. Declare the configurable count
  `usize`, or cast one side with `usize(n)` or `i32(n)`.
- `bang-call-outside-suspension`: the call suspends, so its caller must be
  a suspending function: `fn!`, or `main!`.

Each message names its code, and the rule IDs it cites are in the specification.
