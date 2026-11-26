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
type. Floats also provide `sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
`atan2`, `exp`, `ln`, `log2`, `log10`, `powf`, `powi`, `abs` and `signum`
without an import. Angles are radians. The constants are associated
functions, also without an import: `f64::pi()`, `f64::e()`, `f32::pi()`
and `f32::e()`.

A name may not shadow a prelude name: a function or variable called
`string`, `f32` or `println` is `prelude-name-shadow`.

## Everything else

Every line below is a working `use`. Import a bang function by its plain
name and call it with the bang: `use std.fs.read_text`, then
`read_text!(...)`.

```hd
use std.cli.{Args, Cli, ScriptedArgs, args, positional, short}  # @derive(Cli): typed options and --help
use std.cmp.{clamp, max, min}
use std.collections.{Deque, Heap, Set, counts}
use std.console.{ConsoleInput, MemoryConsole, ScriptedInput, eprintln, print, read_all, read_line}
use std.contract.require                                # check a precondition
use std.convert.{From, TryFrom}
use std.encoding.{Deserialize, MapKey, Serialize, rename, rename_all, skip, skip_none}
use std.encoding.json.{Json, JsonError, decode, encode, encode_pretty, parse}
use std.error.{Error, message, report_of}
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, append_text, create_dir_all, list_dir, read_text, remove, write_text}
use std.hash.Hasher
use std.host.{Env, HostError, ScriptedEnv, ScriptedSignals, Signal, Signals, Sys, env, env_as, env_decode}
use std.meta.{Inspectable, Structure, cold, inline}     # @inline, @cold hints; Structure for `impl T for X by Structure:`
use std.num.{Integer, ParseNumberError, TryFromIntError}
use std.ops.{Add, Default, Disposable, Div, Fn, FnMut, Index, Mul, Neg, Sub, defer}  # Fn, FnMut: bounds for kept closures
use std.path.Path
use std.process.{Command, ExitCode, Process}
use std.random.{Random, Rng, SeededRandom, rng}
use std.regex.{Regex, RegexError}                       # `pattern.decode::[T](line)`
use std.task.{Backoff, TimedOut, apply, fallback, join, race, retry, timeout, with_timeout}
use std.testing.{Arbitrary, hd_run, it_each, it_prop, snapshot, snapshot_file, temp_dir}
use std.text.{StringBuilder, r}
use std.validate.{Validate, ValidationError, at_least, email, length, max, one_of, pattern, range, url}
use std.time.{Clock, Duration, Instant, ManualClock, Timestamp, h, ms, now, s, sleep}
```

`std.time.minutes(count)` constructs minutes; `90min` is the fixed duration literal spelling.

A derive imports its trait, and each field fact its constructor, from the
trait's module: `@derive(Cli)` with `@short('n')` needs
`use std.cli.{Cli, short}`; `@range(1, 9)` needs `std.validate.range`.
The prelude traits (`Eq`, `Debug`, `Hash`, `PartialOrd`, `Ord`) need none.

Other modules: `std.http` and `std.net` (requests and sockets),
`std.encoding.base64` and `std.encoding.hex`, `std.crypto` (hashes, as
`std.crypto.sha256`), `std.iter` (iterator types), `std.format`, `std.thread` (scoped threads) and `std.sync` (`Arc`
for values shared across threads).

## Looking a name up

Inside a package (a folder with `hd.toml`):

```sh
hd doc std.text.split     # one item: its signature and a summary
hd doc std.fs             # a whole module
```

The message for a missing import usually names the line to add, as in
``no trait named `Clock`; add `use std.time.Clock` ``.
