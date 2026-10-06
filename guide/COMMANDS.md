# The `hd` Command

This page is the task-oriented guide to the `hd` command. The normative
reference is [Command Line](../spec/cli/command-line.md); this page shows
each job the way you would do it, with real output.

## Start A Project

You want a new package. `hd new` needs to know which kind: `--app` for an
application, `--lib` for a library:

```sh
$ hd new orders --app
Created application package 'orders' in orders
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

Read next: [Package Mode](../spec/cli/command-line.md#package-mode).

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

Read next: [Running A Package](../spec/cli/command-line.md#running-a-package)
and [Tasks](../spec/cli/command-line.md#tasks).

## Check And Build

You want to know it compiles, without running it. `hd check` type-checks
the package, and `hd build` writes the Wasm:

```sh
$ hd check
orders: ok
$ hd build
$ hd build --release
```

The outputs land in `build/debug/` and `build/release/`:

```sh
build/debug/orders.wasm
build/release/orders.wasm
```

Read next: [Building And Checking](../spec/cli/command-line.md#building-and-checking).

## Test

You want the tests. `hd test` runs every test of the package; `--filter`
runs only the cases whose name contains a word:

```sh
$ hd test
tests/orders.hd: 1 passed
$ hd test --filter "prints"
tests/orders.hd: 1 passed
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

Read next: [Test Runs](../spec/cli/command-line.md#test-runs).

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

Read next: [Dependency Commands](../spec/cli/command-line.md#dependency-commands)
and [Workspaces](../spec/lang/10-modules.md#workspaces).

## Debug And Explore

You want to see a value while it runs, or ask where a name comes from.
`dbg(x)` prints the value with its source location on standard error. `hd
explain CODE` prints what the specification says about a diagnostic code,
and `hd def NAME` shows where a name is defined:

```sh
$ hd explain missing-requirement
missing-requirement: error
$ hd def main
src/main.hd:1:1: function pkg.main.main
  pub fn main() -> void $ Console
```

`hd` with no arguments opens the interactive session; each line is
evaluated and printed with its type:

```sh
$ hd repl
1 + 2
3 : usize
```

Every diagnostic command takes `--format json` for one JSON object per
diagnostic, with the rule IDs that explain it. Read next:
[Machine Output](../spec/cli/command-line.md#machine-output).

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
- `bang-call-outside-suspension`: the call suspends, so its caller must be
  a suspending function: `fn!`, or `main!`.

Each message names its code; `hd explain CODE` links the rules behind it.
