# Process

Status: standard library specification draft.

This chapter defines the part of `std.process` that `lib/std` writes in
ordinary hd over the language tier:

- the `Display` text of `ProcessError`;
- the traits of `ExitCode` and `ProcessOutput`;
- the free helper `run!`;
- the test provider `ScriptedProcess`;
- what a child process inherits, the status a signal leaves, and what
  cancelling `run!` does.

The language tier keeps what the compiler and the test runner name:

| Item | Why it stays in the language tier |
| --- | --- |
| `ExitCode`, `Termination` | lang items that an entry point's result reports through ([Exit Status](../lang/10-modules.md#exit-status)) |
| `Process`, `ProcessOutput`, `ProcessError` | the host capability trait that the test runner binds ([Processes](../lang/10-modules.md#processes)) |

## Process Errors

`ProcessError` says why a program did not start:

```text
use std.process.ProcessError

fn explain(error: ProcessError) -> string:
    "cannot run the tool: ${error}"
```

| Rule | Variant | `Display` text |
| --- | --- | --- |
| r[std-process.error.display.not-found] Not found | `NotFound` | `program not found` |
| r[std-process.error.display.permission] Permission | `PermissionDenied` | `permission denied` |
| r[std-process.error.display.not-granted] Not granted | `NotGranted` | `program not granted; run with --cap Process=PROGRAM` |
| r[std-process.error.display.other] Other | `Other(message)` | `message`, as written |

1. r[std-process.error.debug-text] Its `Debug` writes the qualified variant, and for `Other` the `message` argument, as `ProcessError.Other(message="x")`, by [`std-format.debug.std-types.calls`](format.md#r-std-format.debug.std-types.calls).
2. r[std-process.error.eq] Two `ProcessError` values are equal when they are the same variant and, for `Other`, their messages are equal.
3. r[std-process.error.error] `ProcessError` implements `std.error.Error`, as [Standard Error Types](error.md#standard-error-types) requires.

> **Why.** The host's message is the whole text of `Other`, as `FsError`
> keeps it. `NotFound` carries no program name, so the caller adds it.

## Exit Data

`ExitCode` and `ProcessOutput` are plain values, so they compare and print
like other std values:

```text
use std.process.{ExitCode, ProcessOutput}

fn clean(output: ProcessOutput) -> bool:
    output == ProcessOutput { stdout: "", stderr: "", status: 0 }

fn same(left: ExitCode, right: ExitCode) -> bool:
    left == right
```

1. r[std-process.exit-code.eq] `ExitCode` implements `Eq`; two codes are equal when their `u8` values are.
2. r[std-process.output.eq] `ProcessOutput` implements `Eq`; two outputs are equal when `stdout`, `stderr`, and `status` are each equal.
3. r[std-process.debug-text] `ExitCode` and `ProcessOutput` implement `Debug`, by [`std-format.debug.std-types`](format.md#r-std-format.debug.std-types). An `ExitCode` writes its constructor call, as `ExitCode(0)`.

## Run Helper

`run!` starts a program through the covering provider, so a program need
not write `$.use(Process)`:

```text
use std.process.{Process, ProcessError, run}

fn commit!() -> Result[string, ProcessError] $ Process:
    output := run!("git", ["rev-parse", "--short", "HEAD"])?
    .Ok(output.stdout.trim())
```

1. r[std-process.helper.run] `std.process` declares `pub fn run!(program: string, args: List[string], stdin: string = "") -> Result[ProcessOutput, ProcessError] $ Process`, which calls `run!(program, args, stdin)` on the covering `Process` provider.
2. r[std-process.helper.stdin-default] A call that gives no `stdin` passes the empty string.
3. r[std-process.helper.row] The helper's row is exactly `Process`.
4. r[std-process.helper.suspends] `run!` is a bang function, as the method it calls is.

> **Why.** Go's `exec.Command(...).Output()` and Python's `subprocess.run`
> take no stdin by default. A non-zero status stays `.Ok`, as
> [`module.process.nonzero-ok`](../lang/10-modules.md#r-module.process.nonzero-ok) says.

## Scripted Process

`ScriptedProcess` is the deterministic `Process` provider:

```text
use std.process.{Process, ProcessOutput, ScriptedProcess}
fn version!() -> string $ Process:
    match $.use(Process).run!("git", ["--version"], ""):
        .Ok(output) => output.stdout
        .Err(_) => "no git"

tests:
    use std.testing.assert_equal

    it("answers the scripted program"):
        let mut process = ScriptedProcess::new({"git": ProcessOutput { stdout: "git 2.0", stderr: "", status: 0 }})
        $.with(Process=process):
            assert_equal(version!(), "git 2.0", reason="git is scripted")
```

1. r[std-process.scripted.decl] `std.process` declares `ScriptedProcess`, which implements `Process` and `Debug`, with private fields. Code imports it, as in `use std.process.ScriptedProcess`.
2. r[std-process.scripted.new] `ScriptedProcess::new(outputs: Map[string, ProcessOutput]) -> mut ScriptedProcess` returns a provider that answers each program in `outputs`.
3. r[std-process.scripted.run] `run!(program, args, stdin)` returns `.Ok` of the output that `outputs` holds for `program`, whatever `args` and `stdin` are.
4. r[std-process.scripted.not-found] For a program that `outputs` does not hold, `run!` returns `.Err(ProcessError.NotFound)`.
5. r[std-process.scripted.no-host] A `ScriptedProcess` starts no host program.

> **Why.** Every host capability trait has a deterministic provider
> beside it, so a test never touches the host. A constructor keeps the
> fields private, as `BufferConsole::new` and `ManualClock::new` do.

## Child Processes

A program that `run!` starts on the host is a child of the program that
calls it:

```hd
use std.process.{Process, ProcessError}

fn killed!(tool: string) -> Result[bool, ProcessError] $ Process:
    output := $.use(Process).run!(tool, [], "")?
    .Ok(output.status == 128 + 9)   # SIGKILL, signal 9, ended the tool
```

1. r[std-process.child.inherit] A child inherits the program's environment and working directory, and the host adds nothing to either.
2. r[std-process.child.signal] When a signal ends a child, its `status` is 128 plus the signal number.
3. r[std-process.cancel] Cancelling the suspension of `run!` kills the child and its process group.

> **Why.** 128 plus the signal number is the shell's convention, so a
> status reads the same in hd and in a shell script.

See also: [Processes](../lang/10-modules.md#processes),
[Running Executables](testing.md#running-executables).
