# The `hd` Command

How to drive `hd` while writing code: the edit-check loop, the commands
and flags an agent needs, and how to read what `hd` prints. `hd help`
lists every command; `hd help COMMAND` one command's flags. Flags go after
the command word: `hd test --filter doubles FILE.hd`.

## One file or a package

Decide first: it changes which commands work.

| | A single file | A package (a folder with `hd.toml`) |
| --- | --- | --- |
| make one | write `FILE.hd` | `hd new PATH --app` or `--lib` ([packages](reference/packages.md)) |
| run | `hd FILE.hd -- ARGS` | `hd run -- ARGS` |
| check, test | `hd check FILE.hd`, `hd test FILE.hd` | `hd check --tests`, `hd test` |
| imports | `std` only | `std`, `pkg.MODULE`, dependencies |
| outside world | every capability | only what `[capabilities]` grants ([capabilities](design/capabilities.md)) |
| `hd doc`, `hd check --fix`, `hd build` | not available | available |

Use a package for anything with more than one module, dependencies, an
executable to install, or integration tests.

## The loop

```sh
hd check src/cart.hd       # or, in a package: hd check --tests
hd test src/cart.hd        # or, in a package: hd test
hd fmt && hd lint          # before finishing
```

Read the plain text output. It has everything you act on, and a failing
test also shows its stack, a property's failing input and a `repro:`
line, which `--format json` leaves out ([JSON output](#json-output-for-scripts)).

1. **Check after every edit.** Checking is incremental and fast, so
   batching edits saves nothing and hides which edit broke what.
2. **Fix the first error first.** Later ones are often follow-ons.
3. **Look up the code** in [diagnostics](reference/diagnostics.md) when
   the message alone does not give the fix.
4. **Two tries at most per error.** Then stop guessing and look the item
   up with `hd doc` ([explore](reference/explore.md)).
5. `hd check` in a package skips test code: add `--tests` while working.

## Find out instead of guessing

- **Read signatures, not bodies.** Every hd signature names its inputs,
  output, errors and effects. `hd doc std.text.split` prints one item,
  `hd doc std.text` a module, `hd doc pkg.cart.total` your own code;
  `--private` adds private items.
- **`hd doc` needs a package.** For single-file work, make one probe
  package once and run `hd doc` there:

  ```sh
  hd new /tmp/hd-probe --lib --vcs none
  cd /tmp/hd-probe && hd doc std.collections.map
  ```

- **Probe a snippet** by piping it to `hd`; it runs as a script:

  ```sh
  echo 'println("héllo".len())' | hd       # prints 6: len() counts bytes
  ```

- **Look up a std name** in [reference/std/](reference/std/README.md), one
  small file per module: open `reference/std/time.txt`, or search them all
  with `grep -rh 'split' reference/std/`.

## Everyday commands

| Command | Does |
| --- | --- |
| `hd FILE.hd [-- ARGS]` | run one file |
| `hd run [NAME] [-- ARGS]` | run the package's program, or the executable or task `NAME` |
| `hd check [FILE.hd]` | type-check without running |
| `hd check --tests` | also check test code; `--all` adds tasks too |
| `hd check --fix` | apply the safe fix-its to the package |
| `hd test [FILE.hd]` | run the tests of the package, or of one file |
| `hd test --filter TEXT` | only the cases whose name contains `TEXT` |
| `hd test --update` | rewrite failing snapshots |
| `hd test --affected` | only the test programs that changed since they last passed |
| `hd fmt [FILE.hd]` | format in place: one fixed style, four-space indentation, 100 columns |
| `hd fmt --check` | write nothing; list the files that would change |
| `hd lint [FILE.hd]` | optional advice on code that already checks; `--fix` applies it |
| `hd build [--release]` | write `build/debug/NAME.wasm` (or `build/release/`) |
| `hd doc NAME` | print the docs of one item or module |
| `hd new PATH --app` / `--lib` | create a package; `--vcs none` skips `git init` |
| `hd add TARGET` | add a dependency ([packages](reference/packages.md)) |
| `hd` | an interactive session on a terminal; piped input runs as a program |

Program arguments always go after `--`: `hd run -- input.txt --verbose`.

## Useful flags

| Flag | On | Does |
| --- | --- | --- |
| `--format json` | every command that reports diagnostics | one JSON object per line, for a script that parses it ([below](#json-output-for-scripts)) |
| `--max-errors N` | `check`, `lint` | stop printing after the Nth error |
| `--summary` | `check`, `lint` | counts per file and code instead of each diagnostic |
| `--release` | `hd FILE`, `run`, `build` | optimized build: integer overflow wraps, `require` is skipped ([performance](scenarios/performance.md)) |
| `--time-limit 30s` | `hd FILE`, `run`, `test` | stop a runaway program with a `time-limit` panic |
| `--cap NAME=VALUE` | `hd FILE`, `run`, `test` | grant or deny a capability for one run, as `--cap FsRead=false` |
| `--seed N` | `test` | repeat a property test run |
| `-p NAME` | in a workspace | act on one member package |
| `--allow CODE`, `--warn CODE`, `--deny CODE` | `lint` | set one lint's level |

## Reading the output

### Where things print

| What | Where |
| --- | --- |
| text diagnostics and the `check result:` line | standard error |
| `--format json` from `check`, `lint`, `build`, `test` | standard output, one JSON object per line |
| `--format json` from `hd FILE`, `hd run` | standard error; the program's own output stays on standard output |
| `println` | standard output |
| `dbg(x)`, `eprintln`, panics and their stack | standard error |
| test results and failures | standard output |

### Text diagnostics

```text
error: src/main.hd:5:24: missing-capability: `main` requires `FsRead` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it
    fix: list the missing capabilities in hd.toml
check result: FAILED. errors: 1; warnings: 0
```

The shape is `severity: file:line:column: code: message`, then the fix in
words when there is one. The **code** (`missing-capability`) is stable:
look it up in [diagnostics](reference/diagnostics.md).

### Test failures

A failing case prints its panic, its stack, and the command that reruns
it alone:

```text
PANIC orders.hd:6: doubles
    panic: assertion-failed: actual 4, expected 5
    <std>/testing.hd:739 std.testing.check_equal
    orders.hd:7 test "doubles"
    repro: hd test orders.hd --filter "doubles"
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

A failing property test also prints the shrunk input and the seed that
reproduces it ([testing](design/testing.md)):

```text
    input: 200
    repro: hd test prop.hd --filter "small" --seed 4518923217868180678
```

### JSON output, for scripts

`--format json` is for a program that parses `hd`'s output, such as a CI
step or an editor. An agent reading the output does better with text: the
JSON is twice as long for the same diagnostics, and a failing test's JSON
line keeps only the message, without the stack, the failing input or the
`repro:` line.

`hd check --format json` prints one line per diagnostic, then a summary
that always ends the stream:

```text
{"kind":"diagnostic","code":"trait-used-as-type","severity":"error","message":"`Shape` is a trait, not a type","file":"src/main.hd","line":8,"column":23,"fixes":[{"message":"insert `&dyn` before the trait name","guess":false,"edits":[{"file":"src/main.hd","start":132,"end":132,"text":"&dyn "}]}]}
{"kind":"summary","errors":1,"warnings":0,"passed":0,"failed":0,"ignored":0,"status":101,"modules_checked":1}
```

- `line` and `column` count from 1. In a package, `file` is relative to
  the package root.
- `fixes` lists repairs, likeliest first. Each edit replaces the bytes
  from `start` up to `end` of `file` with `text`; offsets are UTF-8 bytes
  from 0. `"guess": true` marks an uncertain repair.
- Many diagnostics have no fix-it and say the fix in the message, such as
  "use `let` to reassign it".
- In a package, `hd check --fix` applies the safe fix-its. For a single
  file, apply the edits yourself.

`hd test --format json` prints one line per case, then the summary:

```text
{"kind":"test","name":"doubles","outcome":"failed","message":"panic: assertion-failed: actual 4, expected 5"}
{"kind":"summary","errors":0,"warnings":0,"passed":0,"failed":1,"ignored":0,"status":1}
```

### Exit status

| Status | Meaning |
| --- | --- |
| 0 | success; warnings from `hd lint` do not change it |
| 1 | `hd test`: a case failed. `hd fmt --check`: a file needs formatting. A program whose `main` returned an `.Err` |
| 101 | `hd` itself refused: a compile error or a usage error, and nothing ran |
| other | the program's own status, from an `ExitCode` or a panic |

### Panics

A panic prints its category, its message and the stack on standard error,
and the program exits with a failing status:

```text
panic: index-out-of-bounds: list index out of bounds
    orders.hd:2 orders.total
    orders.hd:3 orders.init
```

Common categories: `explicit-panic` (a `panic(...)` call, a failed
`expect`, a broken contract), `assertion-failed`, `index-out-of-bounds`,
`integer-overflow`, `integer-division-by-zero`, `time-limit`.

### Debugging

- `dbg(value)` prints the value with its file and line to standard error.
  It is a statement: it returns nothing.
- `println(debug(value))` prints any `@derive(Debug)` value, list, map or
  optional.
- `hd FILE.hd --time-limit 10s` turns a hang into a panic with a stack.

## Things that trip people

- `hd run FILE.hd` is an error: `hd run` runs a package. For one file,
  write `hd FILE.hd`.
- `hd fmt` takes at most one file. Format several with one command each,
  or run `hd fmt` with no file in the package.
- `hd fmt` leaves a file with a syntax error untouched.
- `hd check`, `hd build` and `hd test` report errors only. Advice such as
  `redundant-let-mut` comes from `hd lint`.
- `hd doc` outside a package is an error. Any folder with an `hd.toml`
  will do.
- `hd build` needs a package, and writes nothing for a library.
- `hd new` in a folder that already has files works: it adds the package
  files beside them.
