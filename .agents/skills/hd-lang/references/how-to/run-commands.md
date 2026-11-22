# How To: Run Another Program

**Scenario.** A release script asks `git` for the current branch and the
changed files, and stops with a clear message when `git` fails.

## The program

```hd
use std.process.{Command, Process, ProcessError, ProcessOutput, ScriptedProcess}

fn git!(arguments: List[string]) -> Result[string, ProcessError] $ Process:
    output := Command::new("git").args(arguments).run!()?
    checked := output.ok()?             # a non-zero status becomes `.Err(.Exited(output))`
    .Ok(checked.stdout)

fn current_branch!() -> Result[string, ProcessError] $ Process:
    .Ok(git!(["rev-parse", "--abbrev-ref", "HEAD"])?.trim())

fn changed_files!() -> Result[List[string], ProcessError] $ Process:
    listing := git!(["status", "--porcelain"])?
    .Ok([for line in listing.lines() if line.len() > 3 => line.slice(3, line.len())])

pub fn main!() -> void $ Console + Process:
    match current_branch!():
        .Ok(branch) => println("on $branch")
        .Err(.NotFound(program)) => println("$program is not installed")
        .Err(.Exited(output)) => println("git failed (${output.status}): ${output.stderr.trim()}")
        .Err(_) => println("could not run git")

tests:
    it("reads the branch from git's output"):
        process := ScriptedProcess::new()
        process.reply("git", .Ok(ProcessOutput { stdout: "main\n" }))
        $.with(Process=process):
            assert_equal(current_branch!().ok(), .Some("main"))
        assert_equal(process.received()[0].args, ["rev-parse", "--abbrev-ref", "HEAD"])

    it("lists changed files"):
        process := ScriptedProcess::new()
        process.reply("git", .Ok(ProcessOutput { stdout: " M src/main.hd\n?? notes.txt\n" }))
        $.with(Process=process):
            assert_equal(changed_files!().ok(), .Some(["src/main.hd", "notes.txt"]))

    it("turns a failing status into an error"):
        process := ScriptedProcess::new()
        process.reply("git", .Ok(ProcessOutput { status: 128, stderr: "fatal: not a git repository\n" }))
        $.with(Process=process):
            match current_branch!():
                .Err(.Exited(output)) => assert_equal(output.status, 128)
                _ => panic("expected Exited")
```

## The pieces

- **Build a command**: `Command::new("git")`, then `.arg("x")`,
  `.args([...])`, `.env(name, value)`, `.current_dir(Path("sub"))`,
  `.stdin(text)`. Each returns the same command, so they chain.
- **Run it**: `.run!()` gives `Result[ProcessOutput, ProcessError]`. The
  output has `stdout`, `stderr` and `status`. The program runs to its end;
  its output is captured, not streamed.
- **Check the status**: a program that ran and exited non-zero is still
  `.Ok`. Call `.ok()` on the output to make that an
  `.Err(.Exited(output))`.
- **Arguments are a list**, never one shell string. There is no shell, so
  no quoting and no `|` or `>`: run each program yourself and pass text
  between them.
- **Capability**: `$ Process`, imported from `std.process`. In a package,
  grant the programs in `hd.toml`: `Process = ["git"]`.
- **Testing**: `ScriptedProcess::new()`, `reply(program, result)`, and
  `received()` for the commands that were run.
