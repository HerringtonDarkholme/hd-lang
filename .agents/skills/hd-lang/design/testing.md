# Testing: Cases, Properties, Fakes And Snapshots

This page owns testing. It covers `tests:` blocks, assertions, tables,
property tests, model-based tests, the std fakes, integration tests,
snapshots, doc tests and temporary directories. The commands that run
them are in [cli.md](../cli.md).

## When

| The code | Write |
| --- | --- |
| any function with a rule you can state in one sentence | an `it` case per example in the file's `tests:` block |
| several examples that share one assertion | one `it_each` table, not copied cases |
| something that round-trips: encode/decode, parse/print, serialize/load | an `it_prop` test that `decode(encode(x)) == x` |
| something that holds for every input: sorted output, a bound, idempotence | an `it_prop` test of that rule |
| a stateful structure: a stack, queue, cache, counter, account | a model-based test with `commands` against a plain model |
| a function with `$ Console`, `$ FsWrite`, `$ Clock` and the like | a fake installed with `$.with(...)` |
| your own effect trait | your own fake ([effects](effects.md)) |
| a whole program: arguments in, `stdout` and `status` out | an integration test under `tests/` with `hd_run!` |
| long text output you would rather record than type | `snapshot` or `snapshot_file`, then `hd test --update` |
| a public function, to show how it is called | a doc test in its `##` comment |
| a call that must panic: a broken `require`, an overflow | `it("...", expect_panic="CATEGORY")` ([panic categories](errors.md#panics)) |
| a case that cannot pass yet | `it("...", ignore="reason")` |

Write the example cases first, then add the property or model test for
the rule behind them. A property finds the inputs you did not think of.

## Cases: `tests:`, `it` and `it_each`

A file keeps its unit tests in one `tests:` block at the end. Each
`it("name"):` is a case. Test code may call the file's private
functions, and it is left out of the built program.

`it_each` runs one body over a table of rows. Each row is its own case,
named `name[0]`, `name[1]`, and so on, so one failing row does not hide
the others. The body is an `fn!` closure that takes one row.

```hd
@derive(Eq, Debug)
enum WithdrawError:
    Low(balance: i32)
    Closed

fn withdraw(balance: i32, amount: i32) -> Result[i32, WithdrawError]:
    if amount > balance: .Err(.Low(balance)) else: .Ok(balance - amount)

fn shipping_cents(weight_grams: i32) -> i32:
    if weight_grams <= 0: panic("a parcel has weight")
    if weight_grams <= 1000: 499 else: 499 + (weight_grams - 1000) / 500 * 150

tests:
    use std.testing.it_each

    it("a parcel up to 1 kg pays the base rate"):
        assert_equal(shipping_cents(800), 499)
        assert(shipping_cents(5000) > 799, reason="heavier costs more")

    it_each(
        "each full 500 g adds 150",
        [(1500, 649), (2000, 799), (3000, 1099)],
        body=fn!(row: (i32, i32)): assert_equal(shipping_cents(row._0), row._1),
    )

    it("a parcel with no weight panics", expect_panic="explicit-panic"):
        _ := shipping_cents(0)

    it("compares a whole Result"):
        assert_equal(withdraw(10, 4), .Ok(6))
        assert_equal(withdraw(5, 9), .Err(.Low(5)))

    it("reads an error's payload"):
        match withdraw(5, 9):
            .Err(.Low(balance)) => assert_equal(balance, 5)
            _ => panic("expected Low")

    it("compares an optional"):
        assert_equal([1, 2].get(5), .None)
        assert_equal("a=b".split_once("="), .Some(("a", "b")))
```

- `it`, `assert` and `assert_equal` need no import
  inside test code. `it_each`, `it_prop`, `it_prop_with` do.
- `assert_equal(actual, expected)` needs the type to be `Eq` and
  `Debug`: add `@derive(Eq, Debug)` to your own types, as on
  `WithdrawError`. Compare a whole `Result` or optional.
- `reason="..."` is optional context printed on failure.
- `expect_panic="CATEGORY"` passes the case only if its body panics with
  that category, and fails it if the body completes or panics with
  another. The value is a string literal, and an unknown category is
  `unknown-panic-category`. A test can check no other way that code
  panics, since source cannot catch a panic.
- `ignore="reason"` skips the case and reports it as `IGNORED`.
- `timeout=5s` limits one case (a `Duration`, imported like any suffix):
  `it("loads", timeout=5s):`.
- One `tests:` block per file. Helpers, structs and `impl` blocks used
  only by tests go inside it, and so do its `use` lines.
- A test run runs no top-level statements. A file with tests keeps its
  work in functions and `main`, not in a script body.

## Properties: `it_prop`

`it_prop` checks a rule on generated inputs. The body takes one
parameter, and its type picks the generator: numbers, `bool`, `string`,
`T?`, `Result`, lists, maps, tuples, and your own types with
`@derive(Arbitrary)` (import `Arbitrary` from `std.testing`; an enum
draws a variant first). Test several inputs together by taking a tuple.
The input type must be `Debug`.

This property checks a round trip over generated points.

```hd
use std.testing.Arbitrary

@derive(Arbitrary, Eq, Debug)
struct Point:
    x: i32
    y: i32

fn encode(p: Point) -> string:
    "${p.x},${p.y}"

fn decode(text: string) -> Point?:
    let .Some((x, y)) = text.split_once(",") else: return .None
    .Some(Point { x: x.parse::[i32]().ok()?, y: y.parse::[i32]().ok()? })

tests:
    use std.testing.it_prop

    it_prop("decode undoes encode", prop=fn!(p: Point):
        assert_equal(decode(encode(p)), .Some(p))
    )
```

Options, all optional: `cases=N` sets how many inputs are drawn,
`examples=[...]` lists inputs that run first on every run, and
`timeout=` limits one case. Edge values (`0`, the bounds of a type, the
empty list) come up in the first cases.

### When a property fails

The runner shrinks the failing input, prints it with `Debug`, and
prints a `repro:` line that holds the seed.

```text
PANIC p2.hd:6: pad reaches width
    panic: assertion-failed: actual 1, expected 0
    input: ("a", 0)
    repro: hd test p2.hd --filter "pad reaches width" --seed 5664202132148859222
test result: FAILED. 0 passed; 1 failed; 0 ignored
```

- Run the `repro:` line to see the same failure. `hd test --seed N`
  repeats any run with that seed.
- The shrunk failure is saved under `__regressions__/` at the package
  root and replays first on the next run. Commit that folder.
- Fix the bug, not the property. Turn the shrunk input into an `it`
  case or an `examples=[...]` entry so it stays named.

### Your own generator: `it_prop_with`

`it_prop_with` takes `gen=`, a function `fn(&mut Choices) -> T`, when the
default generator draws too much: a bounded number, a sorted list, a
valid id. `Choices` has `int(range)`, `float(range)`, `bool()`,
`choose(items)`, `list(max, item)`, `map(max, key, value)`,
`string(max_chars)`, `draw::[T]()` for a type's default, and
`assume(ok)` to discard a case. Draw parts in order: a later draw may
depend on an earlier one.

```hd
use std.testing.Choices

fn percent_of(cents: i64, percent: i64) -> i64:
    cents * percent / 100

fn price_and_percent(c: &mut Choices) -> (i64, i64):
    (c.int(0..=1_000_000), c.int(0..=100))

tests:
    use std.testing.it_prop_with

    it_prop_with("a discount never exceeds the price", gen=price_and_percent, prop=fn!(input: (i64, i64)):
        assert(percent_of(input._0, input._1) <= input._0)
    )
```

## Model-based tests: `commands`

For a stateful structure, test that it behaves like a plain model over a
random history of operations. `std.testing.commands` draws the history:

- `next(c, &model)` picks a command that is valid for the current model,
  or returns `.None` to stop;
- `advance(&model, &command)` returns the model after that command;
- `max` bounds the history length.

Both functions must be pure and use only the draws. The property body
then runs the history on a fresh system and a fresh model, and compares
them. Do not skip commands in the body: `next` already picked only
valid ones. Because the history is rebuilt from the model, a shrunk
history stays valid.

This test checks a capped meter against an integer model. Any history
where the two disagree fails, and the failure prints the history that
was run.

```hd
use std.testing.{Choices, commands, it_prop_with}

struct Meter:
    level: i32
    cap: i32

impl Meter:
    fn add(&mut self, amount: i32) -> void:
        self.level = if self.level + amount > self.cap: self.cap else: self.level + amount

    fn drain(&mut self, amount: i32) -> void:
        self.level -= amount

@derive(Debug)
enum Cmd:
    Add(i32)
    Drain(i32)

# The model: the level as a plain integer.
fn next_cmd(c: &mut Choices, level: &i32) -> Cmd?:
    current := *level
    if c.bool() || current <= 0:
        .Some(.Add(c.int(1..=30)))
    else:
        .Some(.Drain(c.int(1..=current)))

fn advance_cmd(level: &i32, cmd: &Cmd) -> i32:
    match cmd:
        .Add(n) => if level + n > 50: 50 else: level + n
        .Drain(n) => level - n

fn history(c: &mut Choices) -> List[Cmd]:
    commands(c, max=20, initial=0, next=next_cmd, advance=advance_cmd)

tests:
    it_prop_with("meter follows its model", gen=history, prop=fn!(cmds: List[Cmd]):
        let mut meter = Meter { level: 0, cap: 50 }
        let mut level = 0
        for cmd in cmds:
            match cmd:
                .Add(n) => meter.add(n)
                .Drain(n) => meter.drain(n)
            level = advance_cmd(&level, &cmd)
            assert_equal(meter.level, level, reason="after ${debug(cmd)}")
    )
```

## Standard fakes

Code reaches the outside through the traits in its row, so a test
installs a replacement with `$.with(Trait=fake):` and needs no mocking
library. A unit test gets no real console, files or clock: install
whatever the code under test requires.

| Capability | Fake | Read back with |
| --- | --- | --- |
| `Console` | `MemoryConsole::new()` from `std.console` | `console.output()`, `console.error_output()` |
| `ConsoleInput` | `ScriptedInput::new(lines)` from `std.console` | |
| `FsRead`, `FsWrite` | `MemoryFs::new()` from `std.fs` | `read_text!` through the same fake |
| `Clock` | `ManualClock::new(start)` from `std.time` | sleeps finish at once, in virtual time |
| `Random` | `SeededRandom::new(seed)` from `std.random` | |
| `Args` | `box(ScriptedArgs::new(program, values))` from `std.cli` | |
| `Env` | `box(ScriptedEnv::new({"HOME": "/h"}))` from `std.host` | |
| `Http` | `ScriptedHttp::new()` from `std.http`, then `reply(url, result)` | `http.received()` ([http-api](../scenarios/http-api.md)) |
| `Process` | `ScriptedProcess::new()` from `std.process`, then `reply(program, result)` | `process.received()` ([run-commands](../scenarios/run-commands.md)) |
| `Signals` | `ScriptedSignals::new([.Interrupt])` from `std.host` | |

Each `new` returns a shared object, ready to pass to `$.with`, except
`ScriptedArgs` and `ScriptedEnv`: they are plain values, so box them at
the call. Use these before writing a fake. Write one only for your own
traits ([effects](effects.md)).

A unit test that reaches `println` with no console installed fails to
check:

```text
missing-requirement: this needs `$ Console`, which the runner does not bind for this test case
```

### Examples

One program shows a console, a file system and a clock. One `MemoryFs`
serves both reading and writing, and a `ManualClock` finishes sleeps at
once, in virtual time. A case may make bang calls directly.

```hd
use std.console.MemoryConsole
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, create_dir_all, read_text, write_text}
use std.path.Path
use std.time.{Clock, ManualClock, Timestamp, now, s, sleep}

pub fn greet(name: string) -> void $ Console:
    println("Hello, $name")

fn save_report!(name: string, total: i32) -> Result[void, FsError] $ FsWrite:
    create_dir_all!(Path("reports"))?
    write_text!(Path("reports/$name.txt"), "total: $total")

fn read_report!(name: string) -> string $ FsRead:
    read_text!(Path("reports/$name.txt")).unwrap_or("missing")

fn expires!() -> string $ Clock:
    sleep!(90s)
    now().to_rfc3339()

pub fn main() -> void $ Console:
    greet("Ada")

tests:
    it("greets through a replacement console"):
        console := MemoryConsole::new()
        $.with(Console=console):
            greet("Grace")
        assert_equal(console.output().join(""), "Hello, Grace\n")

    it("writes and reads back a report"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            assert(save_report!("q4", 4500).is_ok())
            assert_equal(read_report!("q4"), "total: 4500")
            assert_equal(read_report!("q1"), "missing")

    it("waits ninety seconds of virtual time"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Clock=clock):
            assert_equal(expires!(), "1970-01-01T00:01:30Z")
```

## Integration tests

Integration tests, snapshots and doc tests need a package
([packages](../reference/packages.md)). `hd test` in a package runs all
three, plus every module's `tests:` block. This package has a doc test
on `render`, an executable, and a test file.

<!-- file: reporter/hd.toml -->
```toml
[package]
name = "reporter"

[capabilities]
Console = true

[dev-capabilities]
Process = true
FsRead = true
FsWrite = true
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

A file under `tests/` holds its cases at top level, with no `tests:`
block. It sees only the package's `pub` items, through `pkg`. `hd_run!`
runs the built program and returns its `stdout`, `stderr` and `status`.

<!-- file: reporter/tests/reporter.hd -->
```hd
use pkg.render.render
use std.fs.{read_text, write_text}
use std.path.Path
use std.process.Command
use std.testing.{hd_run, snapshot, temp_dir}

it("renders one line"):
    assert_equal(render("q4", 4500), "report q4: 4500 cents")

it("runs the built program"):
    let out = hd_run!(*Command::new("reporter"))
    assert_equal(out.stdout, "report q4: 4500 cents\n")
    assert_equal(out.status, 0)

it("matches the recorded text"):
    snapshot(render("q2", 250), expect="report q2: 250 cents")

it("writes into its own directory"):
    path := Path("${temp_dir()}/report.txt")
    write_text!(path, render("q1", 5)).expect("write")
    assert_equal(read_text!(path).expect("read"), "report q1: 5 cents")
```

- `Command::new("reporter")` names the package's executable. Add
  arguments with `.arg("x")` or `.args(["x", "y"])` before the `*`.
- `hd_run!` needs `Process = true` under `[dev-capabilities]`, and file
  access needs `FsRead` and `FsWrite` there, or `hd test` reports
  `missing-capability` ([capabilities](capabilities.md)).
- An integration test uses the real console, files and clock.
- `temp_dir()` from `std.testing` gives each case its own empty
  directory, removed afterwards. Each `it_each` row and each generated
  property case gets its own.

## Snapshots

`snapshot(text, expect="...")` compares text with the string written in
the test. `snapshot_file(text)` compares with a recorded file instead,
kept at `__snapshots__/MODULE/SLUG-N.snap` at the package root. For a
case named "matches a recorded file" in `tests/reporter.hd`, that is
`__snapshots__/$tests.reporter/matches-a-recorded-file-1.snap`. The
slug is the lowercased case name, and N counts the `snapshot_file` calls
in the case.

A snapshot with no `expect`, a stale `expect`, or no file yet fails.
Run `hd test --update` and `hd` rewrites the `expect` in the source or
writes the file. Review the change like any other diff.

## Doc tests

A fenced `hd` block inside a `##` doc comment is a test, as on `render`
above. It is its own small program:

- it imports what it uses, from `pkg` or `std`, including `assert_equal`;
- its statements are top-level, with no `it(...)` around them;
- the doc comment sits directly above its declaration, below the file's
  `use` lines, with no blank line between.
