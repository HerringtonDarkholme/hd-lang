# Console

Status: standard library specification draft.

This chapter defines the part of `std.console` that `lib/std` writes in
ordinary hd over the language tier:

- `eprintln`, which writes a line to standard error;
- the host capability trait `ConsoleInput`, and the helper `read_line!`;
- the traits of `ConsoleError`;
- the test providers `BufferConsole` and `ScriptedInput`.

The language tier keeps what the prelude and the conformance harness name
([Console](../lang/10-modules.md#console)):

| Item | Why it stays in the language tier |
| --- | --- |
| `Console`, `ConsoleError` | prelude names, and `Console` is in every default profile ([`module.profile.default`](../lang/10-modules.md#r-module.profile.default)) |
| `println` | a prelude function that the conformance harness uses |

## Standard Error

`eprintln` writes a line to standard error, as `println` writes one to
standard output:

```text
use std.console.eprintln

fn warn(message: string) -> void $ Console:
    eprintln("warning: ${message}")
```

1. r[std-console.eprintln.decl] `std.console` declares `pub fn eprintln[T < Display](value: T) -> void $ Console`. Code imports it, as in `use std.console.eprintln`.
2. r[std-console.eprintln.write] A call `eprintln(value)` calls `write_error_line!(value.to_string())` on the `Console` provider that covers the call.
3. r[std-console.eprintln.like-println] `eprintln` drives that call as `println` drives `write_line!`, with the same rules: [`block_on`](../lang/11-requirements-and-suspension.md#r-req.drive.block-on) driving, its forbidden contexts, and an `explicit-panic` on `.Err`.
4. r[std-console.eprintln.row] Each `eprintln` call must be covered by a `Console` requirement row or a lexical provider scope. Error: `missing-requirement`.

```text
use std.console.eprintln

pub fn main() -> void:
    eprintln("no console")  # error: missing-requirement
```

> **Why.** Standard error is a second stream of the same console, so it
> is a second method of `Console`, not a second trait. A program that
> prints needs one requirement for both streams.

## Console Input

`ConsoleInput` reads standard input one line at a time:

```text
pub trait ConsoleInput:
    fn read_line!(mut self) -> Result[string?, ConsoleError]
```

1. r[std-console.input.decl] `std.console` declares the host capability trait `ConsoleInput` with the method above. Code imports it, as in `use std.console.ConsoleInput`.
2. r[std-console.input.read-line] `read_line!` returns the next line of input without its line ending, or `.None` at the end of input.
3. r[std-console.input.suspends] Console input is I/O, so `read_line!` is a bang call.
4. r[std-console.input.mut] `read_line!` takes `mut self`, so `ConsoleInput` is a [mutable requirement trait](../lang/11-requirements-and-suspension.md#r-req.mut.trait) and a provider may advance through its input.
5. r[std-console.input.helper] `std.console` declares `pub fn read_line!() -> Result[string?, ConsoleError] $ ConsoleInput`, which calls `read_line!` on the `ConsoleInput` provider that covers the call.

```text
use std.console.{ConsoleInput, read_line}

fn first_line!() -> string $ ConsoleInput:
    match read_line!():
        .Ok(.Some(line)) => line
        _ => ""
```

> **Why.** Input is its own trait, apart from `Console`, so a program
> that only prints gains no authority to read.

## Console Errors

`ConsoleError` is a prelude enum with the one case `Closed`:

```text
fn closed(error: ConsoleError) -> bool:
    error == ConsoleError.Closed
```

1. r[std-console.error.traits] `ConsoleError` implements `Eq`, `Debug`, and `Display`. Its `Display` text for `Closed` is `console closed`.
2. r[std-console.error.debug-text] Its `Debug` writes the qualified case name, `ConsoleError.Closed`.
3. r[std-console.error.error] `ConsoleError` implements `std.error.Error`, as [Standard Error Types](error.md#standard-error-types) requires.

## Buffer Console

`BufferConsole` is the recording `Console` provider for tests:

```text
use std.console.BufferConsole
fn greet(name: string) -> void $ Console:
    println("hello, ${name}")

tests:
    use std.testing.assert_equal

    it("records the line"):
        let mut console = BufferConsole::new()
        $.with(Console=console):
            greet("Ada")
        assert_equal(console.output(), ["hello, Ada"], reason="one line")
```

1. r[std-console.buffer.decl] `std.console` declares `BufferConsole`, which implements `Console` and `Debug`, with private fields. Code imports it, as in `use std.console.BufferConsole`.
2. r[std-console.buffer.new] `BufferConsole::new() -> mut BufferConsole` returns a console that has recorded nothing.
3. r[std-console.buffer.output] `c.output() -> List[string]` returns the text of each `write_line!` call, in call order.
4. r[std-console.buffer.error-output] `c.error_output() -> List[string]` returns the text of each `write_error_line!` call, in call order. `BufferConsole` overrides `write_error_line!`, so error lines are not in `output()`.
5. r[std-console.buffer.never-fails] Every write returns `.Ok(())`.

> **Why.** A test checks what a program printed and what it reported
> apart, as Go's `bytes.Buffer` pair for `Stdout` and `Stderr` lets it.

## Scripted Input

`ScriptedInput` is the deterministic `ConsoleInput` provider:

```text
use std.console.{ConsoleInput, ScriptedInput, read_line}
fn first_line!() -> string $ ConsoleInput:
    match read_line!():
        .Ok(.Some(line)) => line
        _ => ""

tests:
    use std.testing.assert

    it("reads the scripted line"):
        let mut input = ScriptedInput::new(["yes"])
        $.with(ConsoleInput=input):
            assert(first_line!() == "yes", reason="the first line")
```

1. r[std-console.scripted.decl] `std.console` declares `ScriptedInput`, which implements `ConsoleInput` and `Debug`, with private fields. Code imports it, as in `use std.console.ScriptedInput`.
2. r[std-console.scripted.new] `ScriptedInput::new(lines: List[string]) -> mut ScriptedInput` returns a provider that has read none of `lines`.
3. r[std-console.scripted.read] Each `read_line!` returns `.Ok(.Some(line))` for the next unread line, in order.
4. r[std-console.scripted.end] Once every line is read, `read_line!` returns `.Ok(.None)`, on every later call too.
5. r[std-console.scripted.no-host] A `ScriptedInput` reads nothing from the host's standard input.

> **Why.** Every host capability trait has a deterministic provider
> beside it, so a test never touches the host.

See also: [Console](../lang/10-modules.md#console),
[Host Capabilities](../cli/command-line.md#host-capabilities).
