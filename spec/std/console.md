# Console

Status: standard library specification draft.

This chapter defines the part of `std.console` that `lib/std` writes in
ordinary hd over the language tier:

- `eprintln`, which writes a line to standard error;
- the host capability trait `ConsoleInput`, and the helper `read_line!`.

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
    fn read_line!(self) -> Result[string?, ConsoleError]
```

1. r[std-console.input.decl] `std.console` declares the host capability trait `ConsoleInput` with the method above. Code imports it, as in `use std.console.ConsoleInput`.
2. r[std-console.input.read-line] `read_line!` returns the next line of input without its line ending, or `.None` at the end of input.
3. r[std-console.input.suspends] Console input is I/O, so `read_line!` is a bang call.
4. r[std-console.input.helper] `std.console` declares `pub fn read_line!() -> Result[string?, ConsoleError] $ ConsoleInput`, which calls `read_line!` on the `ConsoleInput` provider that covers the call.

```text
use std.console.{ConsoleInput, read_line}

fn first_line!() -> string $ ConsoleInput:
    match read_line!():
        .Ok(.Some(line)) => line
        _ => ""
```

> **Why.** Input is its own trait, apart from `Console`, so a program
> that only prints gains no authority to read.

See also: [Console](../lang/10-modules.md#console),
[Host Capabilities](../cli/command-line.md#host-capabilities).
