# Console

Status: standard library specification draft.

This chapter defines the part of `std.console` that `lib/std` writes in
ordinary hd over the language tier:

- `eprintln`, which writes a line to standard error;
- when the host's console output becomes visible;
- the host capability trait `ConsoleInput`, and the helpers `read_line!` and `read_all!`;
- the traits of `ConsoleError`;
- the test providers `MemoryConsole` and `ScriptedInput`.

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

## Host Output

The host's `Console` provider writes each line out before the program
waits on the host:

```hd
use std.console.eprintln
use std.time.{Clock, sleep}

fn main!() -> void $ Console + Clock:
    println("working")   # visible before the sleep starts
    sleep!(2s)
    eprintln("done")     # after "working", on standard error
```

1. r[std-console.host.visible] Each completed `write_line!` or `write_error_line!` of the host's `Console` provider reaches its host stream before the program next waits on the host or ends.
2. r[std-console.host.order] The host's `Console` provider keeps standard output and standard error in their relative order: their lines reach the host in call order.

> **Note.** So a `println` or `eprintln` line is visible before the next
> host wait, such as a `sleep!` or a `send!`.

> **Why.** A program that prints progress and then waits on a slow
> request shows the progress first.

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

`read_all!` reads the rest of the input in one call, for a filter such as
`cat orders.json | total`:

```text
use std.console.{ConsoleInput, read_all}

fn whole_input!() -> string $ ConsoleInput:
    match read_all!():
        .Ok(text) => text
        .Err(_) => ""
```

6. r[std-console.input.read-all] `ConsoleInput` also declares `fn read_all!(mut self) -> Result[string, ConsoleError]`, a bang call that takes `mut self` as `read_line!` does. It returns the rest of the input as one string with the line structure of the text, as Go's `io.ReadAll`, Rust's `read_to_string` and Python's `read()` do: `"a\nb\n"` gives `"a\nb\n"`, `"a\nb"` gives `"a\nb"`, and `"\n"` gives `"\n"`. A `\r\n` is read as `\n`, as `read_line!` reads it. The result is `""` when no input is left. After it, `read_line!` returns `.Ok(.None)`.
7. r[std-console.input.read-all-default] `read_all!` has a default body, so a provider need not define it. The body calls `read_line!` until it returns `.Ok(.None)`, appends each line and a `\n` to one `StringBuilder`, and returns the first `.Err` that `read_line!` returns. Since `read_line!` drops each line ending, the body cannot tell whether the last line had one: it approximates by ending every line with `\n`.
8. r[std-console.input.read-all-host] The host's `ConsoleInput` provider overrides `read_all!` with one read of the rest of standard input. Its result is the input as read, with each `\r\n` read as `\n` and nothing else changed; bytes that are not UTF-8 become U+FFFD. It returns `.Err(ConsoleError.Closed)` when standard input cannot be read.
9. r[std-console.input.read-all-helper] `std.console` declares `pub fn read_all!() -> Result[string, ConsoleError] $ ConsoleInput`, which calls `read_all!` on the `ConsoleInput` provider that covers the call. A typed read is `decode::[T](read_all!()?)`.

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

`MemoryConsole` is the recording `Console` provider for tests:

```text
use std.console.MemoryConsole
fn greet(name: string) -> void $ Console:
    println("hello, ${name}")

tests:
    use std.testing.assert_equal

    it("records the line"):
        let mut console = MemoryConsole::new()
        $.with(Console=console):
            greet("Ada")
        assert_equal(console.output(), ["hello, Ada"], reason="one line")
```

1. r[std-console.buffer.decl] `std.console` declares `MemoryConsole`, which implements `Console` and `Debug`, with private fields. Code imports it, as in `use std.console.MemoryConsole`.
2. r[std-console.buffer.new] `MemoryConsole::new() -> mut MemoryConsole` returns a console that has recorded nothing.
3. r[std-console.buffer.output] `c.output() -> List[string]` returns the text of each `write_line!` call, in call order.
4. r[std-console.buffer.error-output] `c.error_output() -> List[string]` returns the text of each `write_error_line!` call, in call order. `MemoryConsole` overrides `write_error_line!`, so error lines are not in `output()`.
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
5. r[std-console.scripted.read-all] `ScriptedInput` overrides `read_all!`: it returns each unread line followed by `\n`, then has read every line. A scripted line has no ending of its own, so this is the text a host gives for input whose every line ends.
6. r[std-console.scripted.no-host] A `ScriptedInput` reads nothing from the host's standard input.

> **Why.** Every host capability trait has a deterministic provider
> beside it, so a test never touches the host.

See also: [Console](../lang/10-modules.md#console),
[Host Capabilities](../cli/command-line.md#host-capabilities).
