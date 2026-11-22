# Reading `hd` Output

## Where things print

| What | Where |
| --- | --- |
| text diagnostics and the `check result:` line | standard error |
| `--format json` from `check`, `lint`, `build`, `test` | standard output, one JSON object per line |
| `--format json` from `hd FILE`, `hd run` | standard error; the program's own output stays on standard output |
| `println` | standard output |
| `dbg(x)`, `eprintln`, panics and their stack | standard error |
| test results and failures | standard output |

## Text diagnostics

```text
error: src/main.hd:5:24: missing-capability: `main` requires `FsRead` in `$ Console + FsRead`, but `[capabilities]` in hd.toml does not list it
    fix: list the missing capabilities in hd.toml
check result: FAILED. errors: 1; warnings: 0
```

The shape is `severity: file:line:column: code: message`. The **code**
(`missing-capability`) is stable: look it up in the
[mistakes](../mistakes/requirements.md) files.

## JSON diagnostics

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
- In a package, `hd check --fix` applies the safe fix-its for you. For a
  single file, apply the edits yourself.

`hd test --format json` prints one line per case, then the summary:

```text
{"kind":"test","name":"doubles","outcome":"failed","message":"panic: assertion-failed: 2 * 2: actual 6, expected 4"}
{"kind":"summary","errors":0,"warnings":0,"passed":2,"failed":1,"ignored":0,"status":1}
```

## Exit status

| Status | Meaning |
| --- | --- |
| 0 | success; warnings from `hd lint` do not change it |
| 1 | `hd test`: a case failed. `hd fmt --check`: a file needs formatting. A program whose `main` returned an `.Err` |
| 101 | `hd` itself refused: a compile error or a usage error, and nothing ran |
| other | the program's own status, from an `ExitCode` or a panic |

## Panics

A panic prints its category, its message and the stack on standard error,
and the program exits with a failing status:

```text
panic: index-out-of-bounds: list index out of bounds
    orders.hd:2 orders.total
    orders.hd:3 orders.init
```

Common categories: `explicit-panic` (a `panic(...)` call or a failed
`expect`), `assertion-failed`, `index-out-of-bounds`, `integer-overflow`,
`integer-division-by-zero`, `time-limit`.

## Debugging

- `dbg(value)` prints the value with its file and line to standard error.
  It is a statement: it returns nothing.
- `println(debug(value))` prints any `@derive(Debug)` value, list, map or
  optional.
- `hd FILE.hd --time-limit 10s` turns a hang into a panic with a stack.
