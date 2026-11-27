# `std.process`

Generated from `hd doc std.process`. Run `hd doc std.process.NAME` in a package for one item's full text.

## `ExitCode`

```text
pub type ExitCode(u8)
```

The exit status a `main` returns: a small integer the process exits with.

## `Termination`

```text
pub trait Termination
```

What a `main` may return: `void`, an `ExitCode`, or a `Result` whose error is reported.

## `ProcessOutput`

```text
pub struct ProcessOutput
```

What `run!` returns for a started program: its output, exit status, and program name.

- `pub stdout: string = ""`: The text the program wrote to standard output.
- `pub stderr: string = ""`: The text the program wrote to standard error.
- `pub status: i32 = 0`: The status the program exited with; non-zero is still an ordinary `.Ok`.
- `pub program: string = ""`: The program named by the command, or empty for a literal without one.
- `pub fn ok(self) -> Result[ProcessOutput, ProcessError]`: Returns the complete output on zero status, or Exited carrying it on nonzero status.

## `ProcessError`

```text
pub enum ProcessError
```

Why a program did not start: not found or denied (each naming the program), not granted, or another reason with a message.

## `Command`

```text
pub struct Command
```

A program to start with its arguments, environment, working directory, and standard input. `run!` starts it.

- `pub program: string`: The program to start.
- `pub args: List[string] = []`: The arguments, each one item with no shell.
- `pub env: List[(string, string)] = []`: Name and value pairs the child's environment gets beyond the program's own; a later pair for a name wins.
- `pub current_dir: Path? = .None`: The directory the child starts in, or `.None` for the program's own.
- `pub stdin: string = ""`: The text written to the child's standard input.
- `pub fn new(program: string) -> *mut Command`: Returns a command for `program` with no arguments, no environment, the program's own directory, and no standard input.
- `pub fn arg(*mut self, value: string) -> *mut Command`: Appends one argument and returns the command, so calls chain.
- `pub fn args(*mut self, values: List[string]) -> *mut Command`: Appends each of `values` as its own argument and returns the command.
- `pub fn env(*mut self, name: string, value: string) -> *mut Command`: Appends one environment pair and returns the command.
- `pub fn current_dir(*mut self, dir: Path) -> *mut Command`: Sets the directory the child starts in and returns the command.
- `pub fn stdin(*mut self, text: string) -> *mut Command`: Sets the text written to the child's standard input and returns the command.
- `pub fn run!(self) -> Result[ProcessOutput, ProcessError] $ Process`: Starts the program through the covering `Process` provider, waits for it to exit, and returns its output.

## `Process`

```text
pub trait Process
```

The host capability to start a program, and wait for it to exit.

## `run`

```text
pub fn run!(command: Command) -> Result[ProcessOutput, ProcessError] $ Process
```

Runs a command and rejects a nonzero exit while preserving its complete output.

## `ScriptedProcess`

```text
pub struct ScriptedProcess
```

The deterministic `Process` provider: it answers each program from queued results and records each command, with no host access.

- `pub fn new() -> *mut ScriptedProcess`: Returns a provider with no replies and no recorded commands.
- `pub fn reply(&mut self, program: string, result: Result[ProcessOutput, ProcessError]) -> void`: Queues `result` as the next answer for `program`; once the queue runs out, its last answer repeats.
- `pub fn received(&self) -> List[Command]`: The commands the provider ran, in order, including those it answered with `NotFound`.
