# Integration Tests, Snapshots, Doc Tests

These need a package ([cli/packages.md](../cli/packages.md)). `hd test`
in a package runs all three kinds, plus every module's `tests:` block.

## The package under test

<!-- file: reporter/hd.toml -->
```toml
[package]
name = "reporter"

[capabilities]
Console = true

[dev-capabilities]
Process = true
```

<!-- file: reporter/src/render.hd -->
```hd
## One line of the report.
##
## ```hd
## use pkg.render.render
## use std.testing.assert_equal
##
## assert_equal(render("q1", 100), "report q1: 100 cents")
## ```
pub fn render(name: string, total: i32) -> string:
    "report $name: $total cents"
```

<!-- file: reporter/src/main.hd -->
```hd
use pkg.render.render

pub fn main() -> void $ Console:
    println(render("q4", 4500))
```

## Integration tests: `tests/NAME.hd`

A file under `tests/` holds its cases at top level, with no `tests:`
block. It sees only the package's `pub` items, through `pkg`. `hd_run!`
runs the built program and returns its `stdout`, `stderr` and `status`.

<!-- file: reporter/tests/reporter.hd -->
```hd
use pkg.render.render
use std.process.Command
use std.testing.{hd_run, snapshot}

it("renders one line"):
    assert_equal(render("q4", 4500), "report q4: 4500 cents")

it("runs the built program"):
    let out = hd_run!(*Command::new("reporter"))
    assert_equal(out.stdout, "report q4: 4500 cents\n")
    assert_equal(out.status, 0)

it("matches the recorded text"):
    snapshot(render("q2", 250), expect="report q2: 250 cents")
```

- `Command::new("reporter")` names the package's executable. Add
  arguments with `.arg("x")` or `.args(["x", "y"])` before the `*`.
- Running a program needs `Process = true` under `[dev-capabilities]` in
  `hd.toml`.
- An integration test uses the real console, files and clock. `temp_dir()`
  from `std.testing` gives each case its own directory, removed afterwards.

## Snapshots

`snapshot(text, expect="...")` compares text with the string written in
the test. `snapshot_file(text)` compares with a recorded file instead.
Leave `expect` out (or skip recording a file), run `hd test --update`,
and `hd` writes the current text in for you. Review the change like any
other diff.

## Doc tests

A fenced `hd` block inside a `##` doc comment is a test, as on `render`
above. It is its own small program:

- it imports what it uses, from `pkg` or `std`, including `assert_equal`;
- its statements are top-level, with no `it(...)` around them;
- the doc comment sits directly above its declaration, below the file's
  `use` lines, with no blank line between.
