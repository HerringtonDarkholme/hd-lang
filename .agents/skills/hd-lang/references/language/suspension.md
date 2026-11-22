# Suspension: `!`

A function that can wait (for input, a file, a timer, the network) has a
name ending in `!`, and every call to it is written with `!`. The mark
travels up the call chain, so every point where a program can pause is
visible in the source.

## Bang functions and calls

```hd
use std.time.{Clock, ms, sleep}

fn ready!() -> string $ Clock:
    sleep!(1ms)                     # `1ms` is a Duration; import the suffix `ms`
    "ready"

pub fn main!() -> void $ Clock + Console:
    println(ready!())
```

```output
ready
```

- Only a `!` function may make a bang call. Elsewhere the error is
  `bang-call-outside-suspension`: add `!` to the enclosing function's
  name, and to every call of it, up to `main!`.
- Import a bang function by its plain name, `use std.fs.read_text`, and
  call it as `read_text!(...)`.
- A script may make bang calls at top level.
- A closure that makes bang calls is written `fn!(...)`.
- Duration suffixes come from `std.time`: `ms`, `s`, `min`, `h`.

## Reading input and files

```hd
use std.console.{ConsoleInput, read_line}
use std.fs.{FsError, FsRead, read_text}
use std.path.Path

fn count_lines!(path: string) -> Result[usize, FsError] $ FsRead:
    text := read_text!(Path(path))?
    .Ok(text.lines().len())

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput + FsRead:
    while true:
        line := match read_line!()?:        # `.None` at the end of input
            .Some(text) => text.trim()
            .None => break
        match count_lines!(line):
            .Ok(count) => println("$line: $count lines")
            .Err(error) => println("$line: $error")
    .Ok(())
```

`?` on `read_line!()` needs `main!` to return a `Result` with that error
type, so it returns `Result[void, ConsoleError]` and ends with `.Ok(())`.

## Running work together

`join!` runs several suspending calls together and returns their values in
order. `race!` returns the first to finish and cancels the rest. Their
arguments are written **without** `!`: `first()` describes work that has
not started, and `join!` starts it.

```hd
use std.task.{join, race}
use std.time.{Clock, ms, sleep}

fn fetch!(name: string, delay: i64) -> string $ Clock:
    sleep!(ms(delay))
    "$name done"

pub fn main!() -> void $ Clock + Console:
    let (a, b) = join!(fetch("user", 20), fetch("orders", 10))
    println("$a, $b")
    println(race!(fetch("slow", 30), fetch("fast", 5)))
```

```output
user done, orders done
fast done
```

Writing `join!(fetch!("user", 20), ...)` is a `type-mismatch`: the bang
call already ran and gave a string.

## Timeouts and retries

`apply!(policy):` runs a block under a policy from `std.task`. The block
returns a `Result` whose error type can hold a `TimedOut`.

```hd
use std.task.{TimedOut, apply, timeout}
use std.time.{Clock, ms, sleep}

fn slow!() -> string $ Clock:
    sleep!(50ms)
    "late"

fn guarded!() -> Result[string, TimedOut] $ Clock:
    apply!(timeout(5ms)):
        let result: Result[string, TimedOut] = .Ok(slow!())
        result

pub fn main!() -> void $ Clock + Console:
    match guarded!():
        .Ok(value) => println(value)
        .Err(error) => println(error.to_string())
```

```output
timed out after 5ms
```

`std.task` also has `retry(Backoff { attempts: 3, initial: 10ms })`,
`with_timeout(duration, policy)` and `fallback(backup, policy)`, which
compose with `|>`. Run `hd doc std.task` in a package for their
signatures.

## Bang methods in traits

A trait method may be a bang method, and its implementations and calls
carry the `!` too.

```hd
trait Store:
    fn lookup!(&self, key: string) -> string

struct MemoryStore:
    prefix: string

impl Store for MemoryStore:
    fn lookup!(&self, key: string) -> string:
        "${self.prefix}-$key"

fn title!() -> string $ Store:
    $.use(Store).lookup!("title")

pub fn main!() -> void $ Console:
    $.with(Store=box(MemoryStore { prefix: "mem" })):
        println(title!())
```

```output
mem-title
```
