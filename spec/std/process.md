# Process

Status: standard library specification draft.

This chapter defines the part of `std.process` that `lib/std` writes in
ordinary hd over the language tier:

- the `Display` text of `ProcessError`;
- the traits of `ExitCode` and `ProcessOutput`;
- the test provider `ScriptedProcess`.

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

## Scripted Process

`ScriptedProcess` is the deterministic `Process` provider:

```text
use std.process.{Process, ProcessOutput, ScriptedProcess}
use std.testing.assert_equal

fn version!() -> string $ Process:
    match $.use(Process).run!("git", ["--version"], ""):
        .Ok(output) => output.stdout
        .Err(_) => "no git"

tests:
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

See also: [Processes](../lang/10-modules.md#processes),
[Running Executables](testing.md#running-executables).
