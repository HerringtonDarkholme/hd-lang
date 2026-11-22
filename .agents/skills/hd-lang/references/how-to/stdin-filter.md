# How To: Read Standard Input And Transform It

**Scenario.** A filter in a shell pipeline: `cat access.log | hd errors.hd`
reads lines, keeps the ones that matter, and prints a summary.

## The program

```hd
use std.console.{ConsoleInput, MemoryConsole, ScriptedInput, read_line}

struct Summary:
    lines: usize
    errors: List[string]

# One line in, a decision out: a plain function, tested without any input.
fn error_of(line: string) -> string?:
    let (level, message) = line.split_once(" ")?
    if level == "ERROR": .Some(message.trim()) else: .None

fn summarize!() -> Result[Summary, ConsoleError] $ ConsoleInput:
    let mut summary = Summary { lines: 0, errors: [] }
    while true:
        line := match read_line!()?:
            .Some(text) => text
            .None => break                      # the end of input
        summary.lines += 1
        match error_of(line):
            .Some(message) => summary.errors.push(message)
            .None => pass
    .Ok(summary)

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput:
    summary := summarize!()?
    for message in summary.errors:
        println(message)
    println("${summary.errors.len()} errors in ${summary.lines} lines")
    .Ok(())

tests:
    it("keeps only the error lines"):
        assert_equal(error_of("ERROR disk full"), .Some("disk full"))
        assert_equal(error_of("INFO started"), .None)
        assert_equal(error_of("garbage"), .None)

    it("summarizes scripted input"):
        input := ScriptedInput::new(["INFO started", "ERROR disk full", "ERROR no route"])
        console := MemoryConsole::new()
        $.with(ConsoleInput=input, Console=console):
            assert(main!().is_ok())
        assert_equal(console.output().join(""), "disk full\nno route\n2 errors in 3 lines\n")
```

```sh
printf 'INFO started\nERROR disk full\n' | hd errors.hd
```

## The pieces

- **Line by line**: `read_line!()` returns `Result[string?, ConsoleError]`.
  `?` passes a read failure up, `.None` is the end of input, and the line
  has no line ending.
- **All at once**: `read_all!()` returns the whole input as one string;
  then `text.lines()` splits it.
- **Waiting needs `!`**: `summarize!` and `main!` carry the mark because
  they call `read_line!`.
- **`?` on an optional** works in a function that returns an optional, as
  in `error_of`.
- **Keep the decision pure.** `error_of` has no row, so its tests need no
  fake. Only `summarize!` touches input, and its test scripts the lines
  with `ScriptedInput`.
- **Output order**: `println` writes at once, so a long input prints as it
  goes if you print inside the loop.
