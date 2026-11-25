# Talking To The Outside: Console, Files, Time, JSON

Each capability is a trait that the function's row must name
([language/requirements.md](../language/requirements.md)). Calls that wait
end in `!` ([language/suspension.md](../language/suspension.md)).

## Console

| Call | Needs | Does |
| --- | --- | --- |
| `println(x)`, `print(x)` | `$ Console` | write to standard output; `print` adds no newline |
| `eprintln(x)` | `$ Console` | write to standard error |
| `read_line!()` | `$ ConsoleInput` | `Result[string?, ConsoleError]`; `.Ok(.None)` at the end of input |
| `read_all!()` | `$ ConsoleInput` | all of standard input, `Result[string, ConsoleError]` |

```hd
use std.console.{ConsoleInput, eprintln, read_all}

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput:
    text := read_all!()?
    words := text.split_whitespace().len()
    if words == 0:
        eprintln("no input")
    println("words: $words")
    .Ok(())
```

`read_line!` returns the line without its line ending.

## Arguments and environment

```hd
use std.host.{Env, env}
use std.cli.{Args, args}

pub fn main() -> void $ Console + Args + Env:
    words := args()                             # the arguments after `--`
    name := words.get(0).unwrap_or("world")
    home := env("HOME").unwrap_or("?")          # `string?`
    println("hello, $name from $home")
```

Run it as `hd greet.hd -- Ada`. For options, flags and `--help`, use
`std.cli`:

```hd
use std.cli.{Arg, ArgParser}
use std.cli.Args
use std.process.ExitCode

pub fn main() -> ExitCode $ Args + Console:
    parser := ArgParser::new("volume").arg(Arg { name: "level", help: "how loud" })
    parsed := match parser.parse_args():
        .Ok(parsed) => parsed
        .Err(error) => return error.exit()      # prints usage; status 2, or 0 for --help
    println(parsed.value("level").unwrap_or("quiet"))
    ExitCode(0)
```

## Files

Paths are `Path` values from `std.path`. Every call returns a `Result`
with `FsError`.

| Call | Needs | Gives |
| --- | --- | --- |
| `read_text!(path)` | `FsRead` | `string` |
| `read_bytes!(path)` | `FsRead` | `List[u8]` |
| `list_dir!(path)`, `walk!(path)`, `glob!(pattern)` | `FsRead` | entries or paths |
| `stat!(path)` | `FsRead` | an entry, or `.None` when missing |
| `write_text!(path, text)`, `append_text!(path, text)` | `FsWrite` | `void` |
| `create_dir_all!(path)`, `remove!(path)`, `rename!(from, to)`, `copy!(from, to)` | `FsWrite` | `void` |

```hd
use std.fs.{FsError, FsRead, FsWrite, create_dir_all, read_text, write_text}
use std.path.Path

fn save_report!(name: string, total: i32) -> Result[void, FsError] $ FsWrite:
    create_dir_all!(Path("reports"))?       # a write does not create the folder
    write_text!(Path("reports").join("$name.txt"), "total: $total\n")

fn report_total!(name: string) -> Result[i32, FsError] $ FsRead:
    text := read_text!(Path("reports/$name.txt"))?
    .Ok(text.trim().strip_prefix("total: ").unwrap_or("0").parse::[i32]().unwrap_or(0))

pub fn main!() -> Result[void, FsError] $ Console + FsRead + FsWrite:
    save_report!("q4", 4500)?
    println(report_total!("q4")?)
    .Ok(())
```

`Path` methods: `join(part)`, `parent()`, `file_name()`, `file_stem()`,
`extension()`, `with_extension(ext)`. `"$path"` gives its text.

## Time

`Timestamp` is a point in UTC time, `Duration` a length of time, and
`Instant` a reading for measuring intervals. Duration literals use the
units `ms`, `s`, `min`, `h`; ordinary calls import `ms`, `s`, `minutes`, `h` from `std.time`.

```hd
use std.time.{Clock, Timestamp, elapsed, monotonic, ms, now, s, sleep}

fn deadline(from: Timestamp) -> Timestamp:
    from + 30s

pub fn main!() -> void $ Clock + Console:
    start := monotonic()
    sleep!(5ms)
    println("took at least 5ms: ${elapsed(start) >= 5ms}")
    println(now() < deadline(now()))
    fixed := "2026-10-06T12:00:00Z".parse::[Timestamp]().expect("a valid time")
    println("${deadline(fixed).to_rfc3339()} ${fixed.unix_milliseconds()}")
```

```output
took at least 5ms: true
true
2026-10-06T12:00:30Z 1791288000000
```

## JSON

Derive `Serialize` and `Deserialize`, then `decode::[T](text)` and
`encode(value)`. A missing field is an error that names its path; an
extra field is ignored; a field with a default may be left out.

```hd
use std.encoding.json.{JsonError, decode, encode, parse}
use std.encoding.{Deserialize, Serialize}

@derive(Serialize, Deserialize, Debug)
struct Limits:
    retries: i32
    timeout_ms: i32 = 250

@derive(Serialize, Deserialize, Debug)
struct Config:
    name: string
    limits: Limits
    tags: List[string] = []

fn load(text: string) -> Result[Config, JsonError]:
    decode::[Config](text)

match load("{\"name\": \"shop\", \"limits\": {\"retries\": 3}}"):
    .Ok(config) => println("${config.name} ${config.limits.timeout_ms} ${encode(config.limits)}")
    .Err(error) => println("bad config: $error")
match load("{\"name\": \"shop\"}"):
    .Ok(_) => println("loaded")
    .Err(error) => println("bad config: $error")

# Untyped: `parse` gives a `Json` value to inspect.
tree := parse("{\"items\": [{\"id\": 7}]}").expect("valid JSON")
println(debug(tree.pointer("/items/0/id").and_then(fn(id): id.as_i64())))
```

```output
shop 250 {"retries":3,"timeout_ms":250}
bad config: missing field at $.limits
Option.Some(7)
```

`encode_pretty(value)` writes indented text.

## Random

```hd
use std.random.{Random, rng}

pub fn main() -> void $ Console + Random:
    dice := rng()
    roll := dice.int(1..=6)
    pick := dice.choose(["tea", "cake"]).unwrap_or("nothing")
    println("${roll >= 1 && roll <= 6} ${pick.len() > 0}")
```

```output
true true
```
