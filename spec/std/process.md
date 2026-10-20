# Process

Status: standard library specification draft.

This chapter defines the part of `std.process` that `lib/std` writes in
ordinary hd over the language tier:

- the `Display` text of `ProcessError`;
- the traits of `ExitCode` and `ProcessOutput`;
- the builder `Command` and its terminal `run!`;
- the test provider `ScriptedProcess`;
- the status a signal leaves, and what cancelling `run!` does.

The language tier keeps what the compiler and the test runner name:

| Item | Why it stays in the language tier |
| --- | --- |
| `ExitCode`, `Termination` | lang items that an entry point's result reports through ([Exit Status](../lang/10-modules.md#exit-status)) |
| `Process`, `Command`, `ProcessOutput`, `ProcessError` | the host capability trait that the test runner binds, and the values it takes and returns ([Processes](../lang/10-modules.md#processes)) |

## Process Errors

`ProcessError` says why a program did not start:

```text
use std.process.ProcessError

fn explain(error: ProcessError) -> string:
    "cannot run the tool: ${error}"
```

| Rule | Variant | `Display` text |
| --- | --- | --- |
| r[std-process.error.display.not-found-named] Not found | `NotFound(program)` | `program not found: PROGRAM` |
| r[std-process.error.display.permission-named] Permission | `PermissionDenied(program)` | `permission denied: PROGRAM` |
| r[std-process.error.display.not-granted.plain] Not granted | `NotGranted` | `program not granted` |
| r[std-process.error.display.other] Other | `Other(message)` | `message`, as written |
| r[std-process.error.display.exited] Exited | `Exited(status, stderr)` | `program exited with status STATUS: STDERR` |

1. r[std-process.error.debug-fields] Its `Debug` writes the qualified variant and its argument, as `ProcessError.NotFound(program="git")` and `ProcessError.Other(message="x")`, `ProcessError.Exited(status=2, stderr="boom")`, and `NotGranted` alone, by [`std-format.debug.std-types.calls`](format.md#r-std-format.debug.std-types.calls).
2. r[std-process.error.equal] Two `ProcessError` values are equal when they are the same variant and their arguments, if any, are equal.
3. r[std-process.error.error] `ProcessError` implements `std.error.Error`, as [Standard Error Types](error.md#standard-error-types) requires.

> **Why.** `Exited` is added after `Other`, so the host's variant tags
> keep their numbers. The host never returns it, and nothing in
> `std.process` produces it yet. The host's message is the whole text of
> `Other`, as `FsError` keeps it. `NotFound` and `PermissionDenied` name the program, as
> `FsError.NotFound(path)` names its path, so a message such as
> `program not found: git` needs no caller to add the name.

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

## Command

`Command` is the builder that `run!` starts a program from. It is a
struct that `Command::new` returns behind `&mut`. Each setter takes
`&mut self` and returns the reference, so calls chain on the one object,
and the one terminal `run!` starts the program:

```text
use std.path.Path
use std.process.{Command, Process, ProcessError}

fn commit!(repo: Path) -> Result[string, ProcessError] $ Process:
    output := Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(repo).run!()?
    .Ok(output.stdout.trim())
```

1. r[std-process.command.decl-struct] `Command` is the struct type that [`module.process.decl-struct`](../lang/10-modules.md#r-module.process.decl-struct) declares, with public fields `program`, `args`, `env`, `current_dir`, and `stdin`. `std.process` gives it the methods below.
2. r[std-process.command.new-ref] `Command::new(program: string) -> &mut Command` returns a new object holding a command for `program` with no arguments, no environment pairs, no `current_dir`, and an empty `stdin`.
3. r[std-process.command.arg-ref] `arg(&mut self, value: string) -> &mut Command` appends `value` to `args` as one item and returns the reference it was called through.
4. r[std-process.command.args-ref] `args(&mut self, values: List[string]) -> &mut Command` appends each item of `values` to `args`, in order, as its own item and returns the reference.
5. r[std-process.command.env-ref] `env(&mut self, name: string, value: string) -> &mut Command` appends the pair `(name, value)` to `env` and returns the reference. A later pair for a name replaces an earlier one, as [`module.process.env`](../lang/10-modules.md#r-module.process.env) says.
6. r[std-process.command.current-dir-ref] `current_dir(&mut self, dir: Path) -> &mut Command` sets `current_dir` to `.Some(dir)`, replacing an earlier directory, and returns the reference.
7. r[std-process.command.stdin-ref] `stdin(&mut self, text: string) -> &mut Command` sets `stdin` to `text`, replacing an earlier text, and returns the reference. A command that gets no `stdin` call passes the empty string.
8. r[std-process.command.run] `run!(self) -> Result[ProcessOutput, ProcessError] $ Process` calls `run!(self)` on the covering `Process` provider, so a program need not write `$.use(Process)`. Its row is exactly `Process`, and it is a bang function, as the method it calls is.
9. r[std-process.stdin.unread] When the child exits or closes its standard input before reading all of `stdin`, the rest is dropped. The call still returns `.Ok` with the child's output and status.
10. r[std-process.stdin.start-failure] A failure to start the child returns `.Err`, even when `stdin` is nonempty.
11. r[std-process.command.eq] `Command` implements `Eq`; two commands are equal when `program`, `args`, `env`, `current_dir`, and `stdin` are each equal.
12. r[std-process.command.debug] `Command` implements `Debug`, by [`std-format.debug.std-types`](format.md#r-std-format.debug.std-types).
13. r[std-process.command.compare-ref] Two commands that `Command::new` returned are two references, so `==` between them is an error by [`ref.identity.no-equality`](../lang/13-values-and-references.md#r-ref.identity.no-equality). A program compares their values with `*left == *right`.

> **Why.** Rust's `Command` and Go's `exec.Cmd` carry the arguments, the
> environment, and the directory beside the program, so no call fixes one
> combination of them. Go's `exec.Command(...).Output()` and Python's
> `subprocess.run` take no stdin by default. A non-zero status stays `.Ok`,
> as [`module.process.nonzero-ok`](../lang/10-modules.md#r-module.process.nonzero-ok)
> says. `arg` and `env` append in place, in O(1), and the output text
> passes on without a copy.

## Scripted Process

`ScriptedProcess` is the deterministic `Process` provider. A test queues
the answers it needs, runs the code under test, and reads back the
commands that code started:

```text
use std.process.{Command, Process, ProcessOutput, ScriptedProcess}

fn version!() -> string $ Process:
    match Command::new("git").arg("--version").run!():
        .Ok(output) => output.stdout
        .Err(_) => "no git"

tests:
    use std.testing.assert_equal

    it("answers the scripted program and records its arguments"):
        process := ScriptedProcess::new()
        process.reply("git", .Ok(ProcessOutput { stdout: "git 2.0" }))
        $.with(Process=process):
            assert_equal(version!(), "git 2.0", reason="git is scripted")
        assert_equal(process.received()[0].args, ["--version"], reason="the arguments")
```

1. r[std-process.scripted.decl] `std.process` declares `ScriptedProcess`, which implements `Process` and `Debug`, with private fields. Code imports it, as in `use std.process.ScriptedProcess`.
2. r[std-process.scripted.empty-ref] `ScriptedProcess::new() -> &mut ScriptedProcess` returns a new object holding a provider with no replies and no recorded commands. Installing it as `Process=process` shares the object, so the test reads back what the code under test ran.
3. r[std-process.scripted.reply-ref] `reply(&mut self, program: string, result: Result[ProcessOutput, ProcessError]) -> void` queues `result` as an answer for `program`, after the answers already queued for it.
4. r[std-process.scripted.answer] `run!(command)` records `command`, then returns the first answer queued for `command.program` that no earlier run of that program has taken. When every queued answer is taken, it returns the last one again. The arguments, environment, directory, and standard input of `command` do not change the answer.
5. r[std-process.scripted.unqueued] For a program that has no queued answer, `run!` returns `.Err(ProcessError.NotFound(program))`.
6. r[std-process.scripted.received-ref] `received(&self) -> List[Command]` returns every command that `run!` received, in order, including those answered with `NotFound`.
7. r[std-process.scripted.no-host] A `ScriptedProcess` starts no host program.

> **Why.** Every host capability trait has a deterministic provider
> beside it, so a test never touches the host. A queue per program with a
> repeating last answer is how Python's `responses` library serves them.
> `received` lets a test check the arguments, which a table of outputs
> could not. A constructor keeps the fields private, as `MemoryConsole::new`
> and `ManualClock::new` do.

## Child Processes

A program that `run!` starts on the host is a child of the program that
calls it:

```hd
use std.process.{Command, Process, ProcessError}

fn killed!(tool: string) -> Result[bool, ProcessError] $ Process:
    output := Command::new(tool).run!()?
    .Ok(output.status == 128 + 9)   # SIGKILL, signal 9, ended the tool
```

1. r[std-process.child.signal] When a signal ends a child, its `status` is 128 plus the signal number.
2. r[std-process.cancel] Cancelling the suspension of `run!` kills the child and its process group.

> **Why.** 128 plus the signal number is the shell's convention, so a
> status reads the same in hd and in a shell script.

See also: [Processes](../lang/10-modules.md#processes),
[Running Executables](testing.md#running-executables).
