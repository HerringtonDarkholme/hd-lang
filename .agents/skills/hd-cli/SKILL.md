---
name: hd-cli
description: Drive the hd command as a coding agent - run a file, check it with JSON diagnostics, apply fix-its, run tests, build, format, create packages and add dependencies, and look up std signatures - with the edit-check loop to follow, where diagnostics and panics print, and the exit statuses. Use whenever you run hd on hd code; pair it with the write-hd skill for the syntax.
---

# Use The hd CLI

Every command and flag here matches `hd help` and `hd help COMMAND`; run
those when in doubt. The guide is
[website/guide/COMMANDS.md](../../../website/guide/COMMANDS.md) and the
reference is [spec/cli/command-line.md](../../../spec/cli/command-line.md).
In this repo, build the compiler with `cargo build -p hd_cli` and run
`target/debug/hd`.

## The Edit-Check Loop

1. Write or edit the code (see the write-hd skill).
2. `hd check --format json FILE`, or `hd check --format json --tests` in a
   package. Read each diagnostic's `code`, `line`, `column`, `message`, and
   `fixes`. Fix the first error first; later ones are often follow-ons.
3. In a package, `hd fix` applies the safe fix-its for you, then prints
   what remains.
4. `hd test` (or `hd test FILE`). A failure prints a `repro:` line; run
   that one case while you fix it.
5. Run the program: `hd FILE` or `hd run`, with arguments after `--`.
6. Before committing, `hd fmt --check` and `hd check --all`.

Two tries at most per error: when a fix does not take, read the cited
spec chapter or `hd doc` the std item instead of guessing again.

## Commands

| Command | Does |
| --- | --- |
| `hd FILE [-- ARGS]` | run a single file; outside a package it may use only std |
| `hd check [FILE]` | type-check the package, or FILE and the modules it uses; runs nothing |
| `hd check --tests` / `--all` | also the test code / also the tests and tasks |
| `hd check --max-errors N`, `--summary` | print fewer lines; the counts stay the same |
| `hd fix` | apply the package's safe fix-its, at most 3 rounds; no FILE, package only |
| `hd test [FILE]` | run the package's tests, or those of FILE's module |
| `hd test --filter WORD` | only the cases whose name contains WORD |
| `hd test --affected` | only the test programs that changed since they last passed |
| `hd test --update` | rewrite failing snapshots' expected text |
| `hd test --seed N` | fix the base seed of property tests |
| `hd run [NAME] [-- ARGS]` | run the package's executable, or the executable or task NAME |
| `hd build [--release] [FILE]` | write `build/debug/NAME.wasm` (or `build/release/`) |
| `hd fmt [--check] [FILE]` | format in place; `--check` only lists files that would change |
| `hd new PATH --app` / `--lib` | create an application or a library package |
| `hd add ../money`, `hd add OWNER/REPO@1.2` | require a dependency; `--dev` for test-only |
| `hd doc NAME` | print one item's docs, as `hd doc std.text.split`; `hd doc std.text` prints a module |
| `hd` | the REPL on a terminal; piped stdin runs as a program |

Run flags (`hd FILE`, `hd run`, `hd test`, `hd build` where they apply):
`--release` (overflow wraps instead of panicking), `--engine NAME` (this
build offers `node` and `wasmtime`; a program behaves the same on each),
`--time-limit 30s`, `--max-heap 256M`, `--cap NAME=VALUE` (grant or deny a
capability, as `--cap FsRead=false`; with no table or flag, everything is
granted), and `-p NAME` for one workspace member.
Put flags after the command word: `hd check --format json FILE`.

## Reading Output

| What | Where |
| --- | --- |
| text diagnostics and the `check result:` line | stderr |
| `--format json` from `check`, `build`, `test` | stdout, one JSON object per line |
| `--format json` from `hd FILE`, `hd run` | stderr; the program's own output stays on stdout |
| program output (`println`) | stdout |
| `dbg(x)`, `eprintln`, panics and their stack | stderr |
| test results and failures | stdout |

A JSON diagnostic, then the summary that always ends the stream:

```text
{"kind":"diagnostic","code":"trait-used-as-type","severity":"error","message":"`Shape` is a trait, not a type","file":"g7.hd","line":7,"column":23,"fixes":[{"message":"insert `dyn` before the trait name","edits":[{"file":"g7.hd","start":158,"end":158,"text":"dyn "}]}]}
{"kind":"summary","errors":1,"warnings":0,"passed":0,"failed":0,"ignored":0,"status":101,"modules_checked":0}
```

- A fix-it edit replaces bytes `start` up to `end` of `file` with `text`;
  offsets are UTF-8 bytes from 0. Text output does not show fix-its, so
  use JSON or `hd fix` to see them.
- Few diagnostics carry fix-its; most messages say the fix in words,
  such as "use `let` to reassign it".
- `hd test --format json` prints one `{"kind":"test",...}` line per case,
  with `outcome` and `message`, then the summary.
- In a package, `file` is relative to the package root.

A failing test prints the assertion, the stack, and a command that reruns
only that case:

```text
PANIC t_fail.hd:6: doubles
    panic: assertion-failed: 2 * 2: actual 6, expected 4
    <std>/testing.hd:602 std.testing.check_equal
    t_fail.hd:7 test "doubles"
    repro: hd test t_fail.hd --filter "doubles"
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

A runtime panic prints `panic: CODE: message` and the stack on stderr,
and the program exits nonzero.

## Exit Status

| Status | Meaning |
| --- | --- |
| 0 | success; warnings do not change it |
| 1 | `hd test`: a case failed; `hd fmt --check`: a file needs formatting |
| 101 | `hd` failed (a compile error or a usage error) and ran nothing |
| other | `hd FILE`, `hd run`: the program's own status; a panic exits nonzero |

## Packages

`hd new orders --app` writes:

```text
orders/hd.toml          [package] name = "orders"
orders/src/main.hd      pub fn main() -> void $ Console: ...
orders/tests/orders.hd  an integration test that runs the program with hd_run!
```

`hd new money --lib` writes `money/hd.toml`, `money/src/lib.hd` (the
library root: other packages import its items as `dep.money.NAME`) and
`money/tests/money.hd` (an integration test).

- Modules are files under `src/`: `src/shop/cart.hd` is `pkg.shop.cart`.
- `hd add ../money` adds `money = { path = "../money" }` under
  `[dependencies]`; the code then says `use dep.money.greet`.
- `hd run NAME` runs `tasks/NAME.hd` or the executable NAME.
- `hd build` writes `build/debug/NAME.wasm`; `hd FILE.wasm` runs it.
- `hd check` in a package skips test code; add `--tests` while you work.
- `HD_CACHE=DIR` moves the compiled cache, for example to a scratch
  directory in a sandbox.

## Pitfalls

- `hd fix` refuses a FILE and refuses to run outside a package; for a
  single file, apply the JSON `fixes` edits yourself.
- `hd run` is for packages. For one file, run `hd FILE`.
- Program arguments go after `--`: `hd run -- input.txt`.
- A file outside a package may import only `std`.
- `hd test` on a package also runs `tests/*.hd`, doc tests in `##` doc
  comments, and the `tests:` block of every module.
