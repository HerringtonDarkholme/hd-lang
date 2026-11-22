# What Needs An Import

## The prelude: no import

| Kind | Names |
| --- | --- |
| Types | `bool`, `i8` to `i64`, `u8` to `u64`, `usize`, `f32`, `f64`, `char`, `string`, `void`, `never`, `List`, `Map`, `Option`, `Result`, `Ordering`, `Any` |
| Traits | `Display`, `Debug`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Copy`, `Iterator`, `Iterable` |
| Capabilities | `Console`, `ConsoleError` |
| Functions | `println`, `panic`, `debug`, `dbg`, `box` |
| In test code only | `it`, `assert`, `assert_equal` |

`@derive`, `@error` and `@from` need no import either. The methods of
strings, lists, maps, optionals and results need none: they come with the
type.

A name may not shadow a prelude name: a function or variable called
`string`, `f32` or `println` is `prelude-name-shadow`.

## Everything else

Every line below is a working `use`. Import a bang function by its plain
name and call it with the bang: `use std.fs.read_text`, then
`read_text!(...)`.

```hd
use std.annotation.{cold, inline}                     # @inline, @cold hints
use std.cli.{Arg, ArgParser}                          # command-line options and --help
use std.cmp.{clamp, max, min}
use std.collections.{Deque, Heap, Set, counts}
use std.console.{ConsoleInput, MemoryConsole, ScriptedInput, eprintln, print, read_all, read_line}
use std.contract.require                              # check a precondition
use std.convert.From
use std.error.{Error, message, report_of}
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, append_text, create_dir_all, list_dir, read_text, remove, write_text}
use std.function.{Fn, FnMut}                          # bounds for closures that are kept
use std.hash.Hasher
use std.host.{Args, Env, Sys, args, env}
use std.inspect.Inspectable
use std.json.{Json, JsonError, decode, encode, encode_pretty, parse}
use std.num.ParseNumberError
use std.ops.{Add, Disposable, Div, Index, Mul, Neg, Sub, defer}
use std.path.Path
use std.process.{Command, ExitCode, Process}
use std.random.{Random, Rng, SeededRandom, rng}
use std.regex.Regex
use std.serde.{Deserialize, Serialize}
use std.task.{Backoff, TimedOut, apply, fallback, join, race, retry, timeout, with_timeout}
use std.testing.{hd_run, it_each, it_prop, snapshot, snapshot_file, temp_dir}
use std.text.{StringBuilder, r}
use std.time.{Clock, Duration, Instant, ManualClock, Timestamp, h, ms, now, s, sleep}
```

`std.time` also exports the suffix `min` for minutes, which clashes with
`std.cmp.min` in one file: import one of them under another name, as
`use std.time.{min as minutes}`.

Other modules: `std.http` and `std.net` (requests and sockets),
`std.encoding` (base64 and hex), `std.digest` (hashes), `std.validate`
(checked input), `std.iter` (iterator types), `std.format`.

## Looking a name up

Inside a package (a folder with `hd.toml`):

```sh
hd doc std.text.split     # one item: its signature and a summary
hd doc std.fs             # a whole module
```

The message for a missing import usually names the line to add, as in
``no trait named `Clock`; add `use std.time.Clock` ``.
