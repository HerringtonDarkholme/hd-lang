# Standard Library Plan

Status: research, 2026-10-02 (task #172). Nothing here is accepted
behavior except where a note links the specification. Spec pass 64
(2026-10-02) applied the answers to [Q1 to Q6](#questions-for-the-owner)
and part of the [Earlier Owner Decisions](#earlier-owner-decisions); every
other module sketch below is still a proposal for the owner.

Under review: the stdlib tier ([spec/std/](../spec/std/README.md)), the
library itself ([lib/std/](../lib/std/)), the host rules of
[Modules](../spec/lang/10-modules.md) ([Console](../spec/lang/10-modules.md#console),
[Processes](../spec/lang/10-modules.md#processes),
[Runtime Profiles](../spec/lang/10-modules.md#runtime-profiles)), the
[Command Line](../spec/cli/command-line.md) tier
([Single Files](../spec/cli/command-line.md#single-files),
[Host Capabilities](../spec/cli/command-line.md#host-capabilities)), and the
[standard combinators](../spec/lang/11-requirements-and-suspension.md#standard-combinators).

Owner direction: "If you need multiple files, deps, create a package...
let's provide rich, useful, ergonomic stdlib." A single-file program
(`hd FILE`) and a task (`tasks/NAME.hd`) may use only `std`, so `std` must
cover what scripts do. The owner also asked for a review of the
[Effect v4 API](https://effect.website/docs/v4/api/effect), noting that not
every feature belongs in `std`.

## Contents

1. [Findings In Brief](#findings-in-brief)
2. [What Exists Today](#what-exists-today)
3. [Earlier Owner Decisions](#earlier-owner-decisions)
4. [Decided, Not Yet Applied](#decided-not-yet-applied)
5. [Survey Matrices](#survey-matrices)
6. [Gap Survey By Area](#gap-survey-by-area)
7. [A Script With The Proposed Surface](#a-script-with-the-proposed-surface)
8. [The Effect Review](#the-effect-review)
9. [Ranked Rollout](#ranked-rollout)
10. [Questions For The Owner](#questions-for-the-owner)
11. [Sources](#sources)
12. [Parse Log](#parse-log)

## Findings In Brief

- **A script cannot do real work today.** `std` has no way to read
  program arguments, the environment, files, the clock, or randomness,
  and no way to write to standard error. `Process` exists but no runtime
  profile binds it. The trait that reads arguments waits on the host
  catalog ([`cli.args.pass` Note](../spec/cli/command-line.md#program-arguments)).
- **Decided std design was lost.** The STDLIB design record held owner
  decisions of 2026-09-26 and 2026-09-29 that were "applied in this record
  only". The record was archived and then deleted, so those decisions now
  live only in git history. See [Earlier Owner Decisions](#earlier-owner-decisions).
- **Typed JSON is no longer blocked.** The archived record blocked typed
  codecs on typed derivation. Derivation now ships `Walker` and `Source`
  ([lib/std/structure.hd](../lib/std/structure.hd)), and derived `Hash` and
  `Arbitrary` already use them. `ToJson` and `FromJson` can be written in
  `lib/std` the same way.
- **Most gaps are pure library work.** Collections, text, encoding,
  hashing, JSON, durations, error chains, and argument parsing need no
  host. Host areas need one decision, the first host catalog, plus small
  prototype hooks.
- **Effect maps well onto hd.** Requirement rows already are Effect's
  `Context` and `Layer`, `fn!` is the suspended effect, and `all!`,
  `race!`, and `retry!` exist. Worth adding: `Clock` with `sleep!`,
  `timeout!`, a small backoff policy, and `Duration` helpers. Exclude
  fibers, STM, streams, and `Scope`.

## What Exists Today

| Module | In `lib/std` | Stdlib-tier spec | Notes |
| --- | --- | --- | --- |
| `std.text` | `string` methods (`split`, `trim`, `replace`, `find`, `lines`, `repeat`, ...), `join`, `StringBuilder`, `r` prefix, UTF-8 conversion | [text.md](../spec/std/text.md) | no `split_once`, padding, or float parsing |
| `std.collections` | `List`: `map`, `filter`, `first`, `last`, `reversed`, `sorted_by`, `chunks`, `zip`, `view` | [collections.md](../spec/std/collections.md) | no `Set`, `Deque`, heap; `List` mutation is `append` and index set only; no `Map` methods past `get` and `remove` |
| `std.iter` | `Iterator` with `filter`, `take`, `enumerate`, `map`, `fold`, `collect`; `FromIterator` | [iter.md](../spec/std/iter.md) | no `any`, `all`, `find`, `zip`, `chain`, `skip`, `flat_map` |
| `std.option`, `std.result` | `map`, `unwrap_or`, `ok_or`, `expect`, `map_ok`, `map_err`, `ok`, `err`, `is_*` | none | `and_then` decided, not written |
| `std.num` | numeric traits, checked/wrapping/saturating `i32` and `i64` ops, `parse_i32`, `parse_i64` | none (language tier) | no `u32`/`u64` wrapping ops, no `parse_f64`, no fixed-point float text |
| `std.cmp`, `std.hash`, `std.format`, `std.ops` | comparison, hashing, `Display`, `Debug`, operators, `Default` | [cmp.md](../spec/std/cmp.md), [hash.md](../spec/std/hash.md), [format.md](../spec/std/format.md), [ops.md](../spec/std/ops.md) | no standard `Hasher`, no digest |
| `std.time` | `Duration` (milliseconds), suffixes `ms`, `s`, `min`, `h` | [time.md](../spec/std/time.md) | no `Clock`, `Timestamp`, arithmetic, `Ord`, or `Display` |
| `std.task` | `race!`, `retry!` in hd; `all!`, `block_on` intrinsic | [task.md](../spec/std/task.md) | no `sleep!`, `timeout!`, backoff |
| `std.console` | `Console`, `println`, `ConsoleInput`, `BufferConsole` | none (language tier) | no standard error; no profile binds `ConsoleInput` |
| `std.process` | `ExitCode`, `Termination`, `Process.run!`, `ScriptedProcess` | none (language tier) | no profile binds `Process`; no working directory or environment |
| `std.resource` | `ResourceError[E]` | none | no handle type uses it yet |
| `std.error` | not in `lib/std` (compiler-declared `Error`) | none | `chain` and `report_of` are named by [Entry Results](../spec/lang/10-modules.md#entry-results) and the boundary Note, but not written |
| `std.testing`, `std.structure`, `std.inspect`, `std.annotation`, `std.function` | test, derivation, and type-identity support | [testing.md](../spec/std/testing.md) | complete for their purpose |
| absent | args, env, fs, path, json, random, encoding, digest, cli, log, http, regex | none | the gap this plan covers |

The prototype also lacks two things scripts need: `hd FILE` itself
(known failure `CLI-ENTRY`), and the inferred row of a script's top level
(`MHP-1` in [src/KNOWN_ISSUES.md](../src/KNOWN_ISSUES.md)).

## Earlier Owner Decisions

The deleted record `future-work/STDLIB.md` (last version at commit
`ed7fbfc5^`) lists these owner decisions. They are not in the
specification or in [Open Issues](OPEN_ISSUES.md). This plan builds on
them and reopens none.

| # | Decided | What it fixes for this plan |
| --- | --- | --- |
| 2 | 2026-09-26 | I/O suspends (`fs`, network are `!` calls); clock, random, and environment reads are plain calls |
| 3 | 2026-09-26 | the file system is two traits, `FsRead` and `FsWrite`; console input is its own `ConsoleInput` |
| 4 | 2026-09-26 | each host trait's deterministic provider lives beside it, as `std.time.ManualClock` |
| 5 | 2026-09-26 | a library `std.bytes.Bytes`, convertible to and from `List[u8]` |
| 6 | 2026-09-26 | one error enum per domain, as `FsError`, `HttpError` |
| 7 | 2026-09-26 | the prelude does not grow; new names are imported |
| 9 | 2026-09-26 | `decimal` is the only number type past the primitives; `BigInt` is a package |
| 10 | 2026-09-26 | virtual time auto-advances: `sleep!` on a manual clock returns at once |
| 11 | 2026-09-26 | tasks are structured scopes only: `scope!`, `start`, `join!`; no detached spawn |
| 12 | 2026-09-26 | `Secret[T]` is removed for now |
| 13 | 2026-09-26 | ship an untyped `std.json.Json` with one `Number` type modeled on `serde_json::Number` |
| Q15 | 2026-09-29 | `and_then` on `T?` and `Result` |
| Q16 | 2026-09-29 | `Map` gets `contains_key`, `keys`, and `values`, in insertion order |
| Q18 | 2026-09-29 | out-of-range counts panic, as `repeat(-1)` and `chunks(0)` |
| Q21 | 2026-09-29 | std value types implement `Eq`; `Duration`, `Timestamp`, and `Instant` implement `Ord`; error enums implement `Display` |

The same record drafted module names (`std.host`, `std.fs`, `std.path`,
`std.random`, `std.json`, `std.http`) and a `RetryPolicy`. Those drafts were
not decided; this plan reuses the names to avoid churn.

Spec pass 64 applied the ones that are pure spec text and fit today's
specification:

| # | Applied in |
| --- | --- |
| 2 | I/O methods are bang methods, reads are plain calls: [Clock](../spec/std/time.md#clock), [Host](../spec/std/host.md#plain-reads), [Random](../spec/std/random.md#random-source), [Fs](../spec/std/fs.md#suspension), [Console Input](../spec/std/console.md#console-input) |
| 3 | `FsRead`, `FsWrite`, and `ConsoleInput`: [Fs](../spec/std/fs.md), [Console Input](../spec/std/console.md#console-input) |
| 6 | one `FsError` for every file system method: [File System Errors](../spec/std/fs.md#file-system-errors) |
| 7 | every new name is imported, not a prelude name |
| Q16 | `contains_key`, `keys`, and `values`, as insertion-order snapshots: [Map Methods](../spec/std/collections.md#map-methods) |
| Q18 | `repeat(-1)`, `chunks(0)`, and `clamp` with `low > high` panic: [Text](../spec/std/text.md#string-methods), [Collections](../spec/std/collections.md#list-methods), [Clamp](../spec/std/cmp.md#clamp) |
| Q21 | `Eq` on the new value types; `Ord` on `Duration`, `Timestamp`, and `Instant`; `Display` on `FsError`: [Time](../spec/std/time.md), [Fs](../spec/std/fs.md#file-system-errors) |
| 2026-09-29 | `lines()` follows Rust: [Text](../spec/std/text.md#string-methods) |

The rest are listed in [Decided, Not Yet Applied](#decided-not-yet-applied).

## Decided, Not Yet Applied

These owner decisions still stand. Each waits for the spec text or the
design it names.

| Decision | Decided | What it says | Waits for |
| --- | --- | --- | --- |
| 4 | 2026-09-26 | each host trait's deterministic provider lives beside it, as `std.time.ManualClock`; `std.testing.hermetic()` bundles them | the providers' design: `ManualClock`, `MapArgs`, `MapEnv`, `MemoryFs`, `SeededRandom` |
| 5 | 2026-09-26 | a library `std.bytes.Bytes`, readonly and compact, convertible to and from `List[u8]` | a use that `List[u8]` serves badly |
| 9 | 2026-09-26 | `decimal` is the only number type past the primitives; `BigInt` is a package | a `decimal` design |
| 10 | 2026-09-26 | virtual time auto-advances: `sleep!` on a manual clock returns at once | `ManualClock` (decision 4) |
| 11 | 2026-09-26 | tasks are structured scopes only: `scope!`, `start`, `join!`; no detached spawn | a new polling intrinsic, a language-tier item |
| 12 | 2026-09-26 | `Secret[T]` is removed for now | nothing: it removes a draft, so no spec text follows |
| 13 | 2026-09-26 | an untyped `std.json.Json` with one `Number` type modeled on `serde_json::Number` | a `std.json` chapter (tier 6) |
| Q15 | 2026-09-29 | `and_then` on `T?` and `Result` | a stdlib chapter for `std.option` and `std.result`, whose other methods no chapter specifies yet |
| Q14-22 | 2026-09-29 | `abs_diff` returns the unsigned type of the same width | a stdlib chapter for `std.num`; `lib/std` returns the signed type today |
| Q14-22 | 2026-09-29 | integer parsing takes an optional `+` or `-`, then decimal digits only; a lone sign is `InvalidDigit(0)` | a stdlib chapter for `std.num` |
| Q14-22 | 2026-09-29 | `ScriptedProcess::new(outputs)` is the constructor | a `std.process` provider section; `Process` itself is language tier |
| SNAPSHOT-ROW, RUNNER-SURFACE | 2026-10-02 | the spec has them ([Runner Capabilities](../spec/std/testing.md#runner-capabilities)): `TestRunner.snapshot_check`, `snapshot_file` with `$ TestRunner`, and `PropertyRunner` with only `start`, `record`, and `show` | the compiler session. In `lib/std/testing.hd`: add `snapshot_check` and `PropertyCase`, give `snapshot_file` its row and drop `snapshot_file_check`, make `Choices` replay `replay` and `record` each draw, and discard with the `std.testing: case discarded` panic. In the runner: bind `TestRunner` for every test body, read that panic before `show` as a discard, and keep a case's recorded draws after a panic |

## Survey Matrices

Each cell says where the area lives: **S** in the standard library, **P**
partly in it, or **X** in a common external package, with the name. Twenty
libraries are grouped into three tables so that each stays readable. Each
column's sources are in [Sources](#sources).

Legend: S = std; P = std, partly; X = external package; "boot" = a
package that ships with the compiler but is not the base library.

### Systems And Enterprise Languages

| Area | Go | Rust | Zig | Nim | Java | C#/.NET |
| --- | --- | --- | --- | --- | --- | --- |
| files | S `os.ReadFile` | S `std::fs` | S `std.fs` | S `readFile` | S `Files` | S `File` |
| path | S `path/filepath` | S `std::path` | S `std.fs.path` | S `std/paths` | S `Path` | S `Path` |
| env, args | S `os.Getenv`, `os.Args` | S `std::env` | S `std.process` | S `getEnv`, `commandLineParams` | S `System.getenv` | S `Environment` |
| process | S `os/exec` | S `process::Command` | S `process.Child` | S `osproc` | S `ProcessBuilder` | S `Process` |
| JSON | S `encoding/json` | X `serde_json` | S `std.json` | S `std/json` | X Jackson, Gson | S `System.Text.Json` |
| time | S `time`, zones | P `Instant`, `SystemTime`; X `chrono`, `jiff` | P `std.time`, no calendar | S `std/times` | S `java.time` | S `DateTime`, `TimeProvider` |
| regex | S `regexp` (RE2) | X `regex` | X none common | S `std/re` | S `java.util.regex` | S `Regex` |
| collections | S `slices`, `maps`, `container/heap`; no set | S `VecDeque`, `BTreeMap`, `HashSet`, `BinaryHeap` | S `ArrayList`, `HashMap`, `PriorityQueue` | S `tables`, `sets`, `deques` | S rich | S rich, `PriorityQueue` |
| random | S `math/rand/v2`, `crypto/rand` | X `rand` | S `std.Random` | S `std/random` | S `Random`, `SecureRandom` | S `Random.Shared` |
| CLI args | S `flag` | X `clap` | X (std only iterates args) | S `parseopt` | X picocli | X `System.CommandLine` |
| logging | S `log/slog` | X `log`, `tracing` | S `std.log` | S `std/logging` | S `System.Logger` | X `Microsoft.Extensions.Logging` |
| HTTP client | S `net/http` | X `reqwest` | S `std.http.Client` | S `httpclient` | S `java.net.http` | S `HttpClient` |
| hash, encoding | S `crypto/sha256`, `encoding/base64`, `encoding/hex` | X `sha2`, `base64`, `hex` | S `std.crypto`, `std.base64` | P `base64`; X `checksums` | S `MessageDigest`, `Base64`, `HexFormat` | S `SHA256`, `Convert` |
| concurrency | S goroutines, `context`; X `errgroup` | P threads; X `tokio` | S `std.Thread` | P `asyncdispatch` | S `java.util.concurrent` | S `Task`, `Channel` |

### Scripting Runtimes

| Area | Python | Ruby | Julia | Node | Deno | Bun |
| --- | --- | --- | --- | --- | --- | --- |
| files | S `pathlib`, `open` | S `File` | S `read`, `write` | S `fs` | S `Deno.readTextFile` | S `Bun.file`, `Bun.write` |
| path | S `pathlib` | S `Pathname` | S `joinpath` | S `path` | S `@std/path` | S `node:path` |
| env, args | S `os.environ`, `sys.argv` | S `ENV`, `ARGV` | S `ENV`, `ARGS` | S `process.env`, `process.argv` | S `Deno.env`, `Deno.args` | S `Bun.env`, `Bun.argv` |
| process | S `subprocess.run` | S `Open3` | S `run` | S `child_process` | S `Deno.Command` | S `Bun.spawn`, `Bun.$` |
| JSON | S `json` | S `json` | X `JSON3.jl` | S `JSON` | S `JSON` | S `JSON` |
| time | S `datetime`, `zoneinfo` | S `Time`, `Date` | S `Dates` | P `Date` | P `Date`, `@std/datetime` | P `Date` |
| regex | S `re` | S `Regexp` | S `Regex` | S `RegExp` | S `RegExp` | S `RegExp` |
| collections | S `collections`, `heapq`, `itertools` | S `Array`, `Set` | P `Dict`, `Set`; X `DataStructures.jl` | P `Map`, `Set` | P `Map`, `Set`, `@std/collections` | P `Map`, `Set` |
| random | S `random`, `secrets` | S `Random`, `SecureRandom` | S `Random` | S `crypto.randomInt` | S `@std/random` | S `crypto` |
| CLI args | S `argparse` | S `OptionParser` | X `ArgParse.jl` | S `util.parseArgs` | S `@std/cli` | S `util.parseArgs` |
| logging | S `logging` | S `Logger` | S `Logging` | P `console` | P `@std/log` | P `console` |
| HTTP client | S `urllib.request`; X `requests` | S `Net::HTTP` | S `Downloads`; X `HTTP.jl` | S `fetch` | S `fetch` | S `fetch` |
| hash, encoding | S `hashlib`, `base64` | S `Digest`; `base64` a bundled gem | S `SHA`, `Base64` | S `crypto`, `Buffer` | S Web Crypto, `@std/encoding` | S `Bun.CryptoHasher` |
| concurrency | S `asyncio` | S `Thread`, `Queue` | S `Threads`, `Channel` | S `Promise`, `worker_threads` | S `Promise`, `@std/async` | S `Promise`, workers |

### Functional And Application Languages

| Area | Kotlin | Swift | Scala | Dart | Haskell | OCaml | Elixir | Effect |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| files | P `kotlin.io` (JVM) | P Foundation | P `scala.io.Source`; X `os-lib` | S `dart:io` | S `readFile`; boot `directory` | S `In_channel` | S `File` | S `FileSystem` |
| path | P `kotlin.io.path` (JVM) | P `URL`; X `swift-system` | P `java.nio.file` | X `path` | boot `filepath` | S `Filename` | S `Path` | S `Path` |
| env, args | P `System.getenv` (JVM) | S `ProcessInfo`, `CommandLine` | S `sys.env` | S `Platform` | S `System.Environment` | S `Sys` | S `System` | S `Config` |
| process | P `ProcessBuilder` (JVM) | P Foundation `Process` | S `scala.sys.process` | S `Process.run` | boot `process` | P `Unix`, `Sys.command` | S `System.cmd` | S `ChildProcess` |
| JSON | X `kotlinx.serialization` | S `Codable` + Foundation | X circe, upickle | S `dart:convert` | X `aeson` | X `yojson` | S `JSON` (1.18) | S `Schema` |
| time | S `kotlin.time`; X `kotlinx-datetime` | S `Duration`, `Clock` | P `java.time` | S `DateTime`, `Duration` | boot `time` | P `Unix.gettimeofday` | S `DateTime`, `Calendar` | S `DateTime`, `Clock`, `Cron` |
| regex | S `Regex` | S `Regex` | S `Regex` | S `RegExp` | X `regex-tdfa` | P `Str`; X `re` | S `Regex` | P `RegExp` helpers |
| collections | S rich | S `Array`, `Set`; X `swift-collections` | S rich | S `dart:collection` | boot `containers` | S `Map`, `Set`, `Queue` | S `Map`, `MapSet` | S `HashMap`, `HashSet`, `Chunk` |
| random | S `kotlin.random` | S `SystemRandomNumberGenerator` | S `Random` | S `dart:math` | X `random` | S `Random` | P Erlang `:rand` | S `Random` |
| CLI args | X `clikt` | X `swift-argument-parser` | X `scopt` | X `args` | S `GetOpt`; X `optparse-applicative` | S `Arg`; X `cmdliner` | S `OptionParser` | S `Cli` modules |
| logging | X `kotlin-logging` | X `swift-log` | X `scala-logging` | X `logging` | X `co-log` | X `logs` | S `Logger` | S `Logger` |
| HTTP client | X `ktor` | P `URLSession` | X `sttp` | S `HttpClient`; X `http` | X `http-client` | X `cohttp` | P `:httpc`; X `Req` | S `HttpClient` |
| hash, encoding | P `Base64`, `HexFormat` | P `Data` base64; X `swift-crypto` | P JVM | P `base64`; X `crypto` | X `crypton` | P `Digest` (MD5, BLAKE2) | S `:crypto`, `Base` | S `Encoding`, `Crypto` |
| concurrency | X `kotlinx.coroutines` | S `async`, `TaskGroup` | S `Future` | S `Future`, `Isolate` | S `forkIO`; X `async` | S `Domain`; X `eio` | S `Task`, OTP | S `Fiber`, `Queue` |

### Takeaways

1. **Files, paths, env, args, and processes ship with every toolchain.**
   Most put them in std; Kotlin and Scala reuse the JVM's, and Haskell
   ships them as boot packages. Even Rust and Zig, the smallest libraries
   here, have them.
2. **JSON, CLI parsing, and HTTP split about half and half.** The
   scripting runtimes (Python, Ruby, Node, Deno, Bun) ship all three;
   compiled languages with strong package managers (Rust, Kotlin, Scala,
   Haskell, OCaml) leave them to packages. hd's single-file rule puts it
   with the scripting runtimes.
3. **Regex is std in 15 of the 20.** The holdouts are Rust, Zig, Haskell,
   OCaml, and Effect (which reuses JavaScript's).
4. **Calendar and time-zone logic is the most often external area.** Rust,
   Kotlin, Zig, Haskell, and OCaml keep it out; Go, Java, .NET, and Python
   pay for a time-zone database.
5. **Testable time is a recent addition.** .NET added `TimeProvider` in
   .NET 8, Go added `testing/synctest` in Go 1.25, and Effect ships
   `TestClock`. hd already has the seam: a `Clock` requirement trait.

## Gap Survey By Area

Each area gives the standout designs worth copying, the minimal hd module,
and its dependencies. Sketches are signatures with `pass` bodies; they
parse but are not type-checked. Tier names refer to the
[Ranked Rollout](#ranked-rollout).

### Program Arguments And Environment

Standouts:

- **Deno** reads `Deno.args` and `Deno.env.get`, and each needs a
  permission flag such as `--allow-env`: authority is declared, as hd's
  row declares it.
- **Effect `Config`** reads typed settings through a swappable
  `ConfigProvider`, so tests supply a map. hd's `MapEnv` is the same seam.

Minimal `std.host` (reads are plain calls by decision 2):

```text
pub trait Args:
    fn program(self) -> string
    fn list(self) -> List[string]

pub trait Env:
    fn get(self, name: string) -> string?
    fn names(self) -> List[string]

pub data MapArgs:
    program: string
    values: List[string]

pub data MapEnv:
    values: Map[string, string]

pub fn args() -> List[string] $ Args:
    $.use(Args).list()

pub fn env(name: string) -> string? $ Env:
    $.use(Env).get(name)
```

Depends on: the first host catalog ([Question 2](#q2-the-first-host-catalog)).
The prototype's host bridge carries scalars and strings only, so `list`
needs a small hook that returns `List[string]`.

### Console Output And Input

Standouts:

- **Rust `eprintln!`** and **Go `fmt.Fprintln(os.Stderr, ...)`** keep
  diagnostics off standard output, so `script | jq` still works.
- **Python `sys.stdin.read()`** reads piped input whole.

Minimal additions to `std.console`:

```text
pub trait ErrorConsole:
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]

pub fn eprintln[T < Display](value: T) -> void $ ErrorConsole:
    pass

pub fn read_line!() -> string? $ ConsoleInput:
    pass

pub fn read_all!() -> string $ ConsoleInput:
    pass
```

`eprintln` drives its write with `block_on`, exactly as `println` does.
Depends on: [Question 4](#q4-standard-error).

### Files And Paths

Standouts:

- **Go `os.ReadFile` and `os.WriteFile`** cover most script needs in one
  call each, and `fs.FS` with `testing/fstest.MapFS` gives an in-memory
  file system for tests.
- **Go `path/filepath`** works on plain strings: `Join`, `Dir`, `Base`,
  `Ext`, `Match`, `WalkDir`.
- **Python `pathlib`** and **Bun `Glob`** make globbing one call.

Minimal `std.path`, a newtype over the text, as Go's functions over
strings:

```text
pub type Path(string)

impl Path:
    pub fn join(self, child: string) -> Path: pass
    pub fn parent(self) -> Path?: pass
    pub fn file_name(self) -> string?: pass
    pub fn extension(self) -> string?: pass
    pub fn with_extension(self, extension: string) -> Path: pass
    pub fn components(self) -> List[string]: pass
    pub fn normalize(self) -> Path: pass
    pub fn matches(self, glob: string) -> bool: pass
```

Minimal `std.fs`, whole-file operations only (decisions 2, 3, 6):

```text
use std.path.Path

pub enum FsError:
    NotFound(path: Path)
    PermissionDenied(path: Path)
    AlreadyExists(path: Path)
    NotADirectory(path: Path)
    IsADirectory(path: Path)
    InvalidUtf8(path: Path)
    Other(message: string)

pub enum EntryKind:
    File
    Directory
    Symlink

pub data Entry:
    pub path: Path
    pub kind: EntryKind
    pub size: u64

pub trait FsRead:
    fn read_bytes!(self, path: Path) -> Result[List[u8], FsError]
    fn read_text!(self, path: Path) -> Result[string, FsError]
    fn list_dir!(self, path: Path) -> Result[List[Entry], FsError]
    fn stat!(self, path: Path) -> Result[Entry?, FsError]

pub trait FsWrite:
    fn write_bytes!(mut self, path: Path, bytes: List[u8]) -> Result[void, FsError]
    fn write_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn append_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir_all!(mut self, path: Path) -> Result[void, FsError]
    fn remove!(mut self, path: Path) -> Result[void, FsError]
    fn rename!(mut self, from: Path, to: Path) -> Result[void, FsError]

pub data MemoryFs:
    files: mut Map[string, List[u8]]

pub fn read_text!(path: Path) -> Result[string, FsError] $ FsRead:
    $.use(FsRead).read_text!(path)

pub fn write_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite:
    $.use(FsWrite).write_text!(path, text)

pub fn walk!(root: Path) -> Result[List[Entry], FsError] $ FsRead:
    pass

pub fn glob!(root: Path, pattern: string) -> Result[List[Path], FsError] $ FsRead:
    pass
```

`walk!` and `glob!` are hd loops over `list_dir!`. Open file handles and
streaming wait for the parked NonEscapable design
([Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy)).

Depends on: the host catalog; a host binding (WASI preopened directories
in the official runtime, Node's `fs` in the prototype).

### Processes

Standouts:

- **Python `subprocess.run(args, capture_output=True, check=True)`**
  turns a nonzero status into an error on request.
- **Bun `$` and zx** build a command from a template literal and quote
  each interpolated value as one argument, so no shell injection.

The spec's `Process.run!(program, args, stdin) -> ProcessOutput?` stays.
Minimal additions:

```text
pub enum ProcessError:
    NotFound(program: string)
    Failed(program: string, output: ProcessOutput)

impl ProcessOutput:
    pub fn success(self) -> bool: pass

pub fn run!(program: string, args: List[string] = [], stdin: string = "") -> Result[ProcessOutput, ProcessError] $ Process:
    pass

pub fn run_ok!(program: string, args: List[string] = []) -> Result[string, ProcessError] $ Process:
    pass
```

`run_ok!` returns the standard output, or `Failed` on a nonzero status,
as `check=True` does. A working directory and environment need a change
to the language-tier `Process` trait, so they wait. A `cmd"git log -n ${n}"`
prefix in Bun's style is possible with
[`@str_prefix`](../spec/lang/05-expressions.md#prefixed-strings), but it
is an agent-originated adaptation and is left out of the plan.

Depends on: a profile that binds `Process` for `hd FILE` and tasks. WASI
has no subprocess interface, so this is a toolchain host extension, as
`hd test` already binds one for integration tests.

### JSON

Standouts:

- **Rust `serde_json`**: an untyped `Value` plus derived typed codecs, and
  a `Number` that keeps 64-bit integers exact (decision 13 copies it).
- **Go `encoding/json`**: one call each way, `Marshal` and `Unmarshal`.
- **Elixir 1.18** moved JSON into std after a decade of `Jason`, a sign
  that scripts need it built in.

Minimal `std.json`. The untyped part is decision 13; the typed part uses
the derivation protocol that `Hash` and `Arbitrary` already use:

```text
use std.structure.{Field, Source, Variant, Walker}

pub enum Json:
    Null
    Bool(value: bool)
    Number(value: Number)
    Text(value: string)
    Array(items: List[Json])
    Object(fields: Map[string, Json])

pub data Number:
    repr: i64

pub data JsonError:
    pub message: string
    pub offset: i32

impl Json:
    pub fn get(self, key: string) -> Json?: pass
    pub fn at(self, index: i32) -> Json?: pass
    pub fn as_text(self) -> string?: pass
    pub fn as_i64(self) -> i64?: pass
    pub fn as_f64(self) -> f64?: pass
    pub fn as_bool(self) -> bool?: pass

pub fn parse(text: string) -> Result[Json, JsonError]:
    pass

pub fn pretty(value: Json, indent: i32 = 2) -> string:
    pass

pub trait ToJson:
    fn to_json(self) -> Json

pub trait FromJson:
    fn from_json(value: Json) -> Result[Self, JsonError]

impl[T] ToJson for T by Structure:
    fn to_json(self) -> Json:
        pass

impl[T] FromJson for T by Structure:
    fn from_json(value: Json) -> Result[T, JsonError]:
        pass

pub fn encode[T < ToJson](value: T) -> string:
    pass

pub fn decode[T < FromJson](text: string) -> Result[T, JsonError]:
    pass
```

`Json` implements `Display` with compact text. The `Number`
representation here is a placeholder for decision 13's private one.
Renames and skipped fields come later through typed member facts.

Depends on: a `parse_f64` host function, a minimal hook like the existing
`format_f64`. A correctly rounded float parser in hd is possible but large.

### Time And Dates

Standouts:

- **Rust** splits `Instant` (monotonic) from `SystemTime` (wall), so
  elapsed-time code cannot go backwards.
- **Java `java.time`** splits `Instant` from calendar types, and keeps time
  zones in their own types.
- **.NET `TimeProvider`**, **Go `synctest`**, and **Effect `TestClock`**
  make time injectable; hd's `ManualClock` does that (decision 10).

Minimal additions to `std.time`, UTC only:

```text
pub data Timestamp:
    unix_millis: i64

pub data Instant:
    ticks: i64

pub data Date:
    pub year: i32
    pub month: i32
    pub day: i32

pub enum TimeParseError:
    Invalid(offset: i32)

pub trait Clock:
    fn now(self) -> Timestamp
    fn monotonic(self) -> Instant
    fn sleep!(mut self, duration: Duration) -> void

pub data ManualClock:
    current: Timestamp

impl Duration:
    pub fn minutes(count: i64) -> Duration: pass
    pub fn as_seconds(self) -> i64: pass

impl Timestamp:
    pub fn from_unix_millis(millis: i64) -> Timestamp: pass
    pub fn plus(self, duration: Duration) -> Timestamp: pass
    pub fn since(self, earlier: Timestamp) -> Duration: pass
    pub fn date(self) -> Date: pass
    pub fn to_rfc3339(self) -> string: pass
    pub fn parse_rfc3339(text: string) -> Result[Timestamp, TimeParseError]: pass

pub fn now() -> Timestamp $ Clock:
    $.use(Clock).now()

pub fn sleep!(duration: Duration) -> void $ Clock:
    $.use(Clock).sleep!(duration)
```

`Duration` also gets `Add`, `Sub`, `Ord`, and a `Display` like Go's
`1m30s`. Time zones and locale formatting are excluded: they need a
time-zone database, and Rust, Kotlin, and Zig keep them out too.

### Text And Formatting

Standouts:

- **Rust `split_once`** and **Python `str.partition`** parse `key=value`
  in one call.
- **Python format specs** and **JavaScript `toFixed`** print `3.14` from
  `3.14159`; scripts that print money or timings need it.

Minimal additions to `std.text` and `std.num`:

```text
impl string:
    pub fn split_once(self, separator: string) -> (string, string)?: pass
    pub fn split_whitespace(self) -> List[string]: pass
    pub fn pad_start(self, width: i32, fill: char = ' ') -> string: pass
    pub fn pad_end(self, width: i32, fill: char = ' ') -> string: pass
    pub fn count(self, needle: string) -> i32: pass

impl f64:
    pub fn to_fixed(self, digits: i32) -> string: pass

pub fn parse_f64(text: string) -> Result[f64, ParseNumberError]:
    pass
```

`to_fixed` and `parse_f64` need host hooks beside the existing
`format_f64`. Regex is [Question 6](#q6-regular-expressions).

### Collections And Iterators

Standouts:

- **Python `collections`**: `Counter`, `defaultdict`, `deque`, and
  `itertools.groupby` cover most data shaping in scripts.
- **Kotlin collections**: `groupBy`, `associateBy`, `partition`,
  `windowed`, and `sortedBy` as methods.
- **Rust** `VecDeque` and `BinaryHeap`, and `sort_by_key`.

Minimal additions (decisions Q15, Q16, Q18 included):

```text
impl[T] List[T]:
    pub fn sorted_by_key[K < Ord](self, key: fn(T) -> K) -> List[T]: pass
    pub fn group_by[K < Eq & Hash](self, key: fn(T) -> K) -> Map[K, List[T]]: pass
    pub fn partition(self, keep: fn(T) -> bool) -> (List[T], List[T]): pass
    pub fn any(self, test: fn(T) -> bool) -> bool: pass
    pub fn all(self, test: fn(T) -> bool) -> bool: pass
    pub fn find(self, test: fn(T) -> bool) -> T?: pass
    pub fn flat_map[U](self, transform: fn(T) -> List[U]) -> List[U]: pass
    pub fn windows(self, size: i32) -> List[List[T]]: pass
    pub fn pop(mut self) -> T?: pass
    pub fn insert(mut self, index: i32, value: T) -> void: pass
    pub fn remove_at(mut self, index: i32) -> T: pass
    pub fn clear(mut self) -> void: pass

impl[T < Eq] List[T]:
    pub fn contains(self, value: T) -> bool: pass
    pub fn index_of(self, value: T) -> i32?: pass

impl[T < Ord] List[T]:
    pub fn sorted(self) -> List[T]: pass
    pub fn min(self) -> T?: pass
    pub fn max(self) -> T?: pass

impl[K, V] Map[K, V]:
    pub fn contains_key(self, key: K) -> bool: pass
    pub fn keys(self) -> List[K]: pass
    pub fn values(self) -> List[V]: pass
    pub fn get_or(self, key: K, fallback: V) -> V: pass

impl[T] T?:
    pub fn and_then[U](self, next: fn(T) -> U?) -> U?: pass

pub data Set[T < Eq & Hash]:
    entries: Map[T, void]

pub data Deque[T]:
    slots: List[T?]
    head: i32
    count: i32

pub data Heap[T < Ord]:
    items: List[T?]
    count: i32

pub fn counts[T < Eq & Hash](items: List[T]) -> Map[T, i32]:
    pass
```

`Iterator` gets the lazy and draining forms of the same names: `any`,
`all`, `find`, `count`, `skip`, `zip`, `chain`, `flat_map`, and
`take_while`. `pop`, `remove_at`, and `clear` shrink a list, and `std` has
no way to do that today, so they need one list-truncate primitive, a hook
like `list_version`. `Set`, `Deque`, and `Heap` are plain hd over `Map` and
`List`.

### Random Numbers

Standouts:

- **Go `math/rand/v2`**: a seeded generator value (`rand.New(rand.NewPCG(...))`)
  beside a host-seeded default.
- **Python `secrets`**: secure randomness is a separate, named API.

Minimal `std.random` (the archived draft, unchanged in shape):

```text
pub data Rng:
    state: u64

impl Rng:
    pub fn from_seed(seed: u64) -> mut Rng: pass
    pub fn next_u64(mut self) -> u64: pass
    pub fn int(mut self, low: i64, high: i64) -> i64: pass
    pub fn float(mut self) -> f64: pass
    pub fn choose[T](mut self, items: List[T]) -> T?: pass
    pub fn shuffle[T](mut self, items: mut List[T]) -> void: pass

pub trait Random:
    fn next_u64(mut self) -> u64
    fn fill(mut self, count: i32) -> List[u8]

pub data SeededRandom:
    generator: mut Rng

pub fn rng() -> mut Rng $ Random:
    Rng::from_seed($.use(Random).next_u64())
```

`int` includes both bounds, as `Choices.int` in `std.testing` does. A
PCG step needs wrapping `u64` multiplication. `std.num` has wrapping ops
for `i32` and `i64` only; hd can build it from 32-bit halves, or a hook can
supply it.

### Command-Line Parsing

Standouts:

- **Node `util.parseArgs`** and **Deno `@std/cli` `parseArgs`**: one
  function, an options table in, values and positionals out.
- **Go `flag`** prints usage text from the same table.
- **Rust `clap` derive** builds a typed struct; in hd that is a later
  `Source` template, like `FromJson`.

Minimal `std.cli`:

```text
pub data Opt:
    pub name: string
    pub short: string?
    pub takes_value: bool
    pub help: string

pub data Parsed:
    pub values: Map[string, string]
    pub flags: List[string]
    pub positionals: List[string]

impl Parsed:
    pub fn flag(self, name: string) -> bool: pass
    pub fn value(self, name: string) -> string?: pass

pub enum CliError:
    UnknownOption(name: string)
    MissingValue(name: string)

pub fn parse_args(args: List[string], options: List[Opt]) -> Result[Parsed, CliError]:
    pass

pub fn usage(program: string, options: List[Opt]) -> string:
    pass
```

It is pure: the caller passes `args()` from `std.host`.

### Hashing And Encoding

Standouts:

- **Go** keeps one package per format: `encoding/hex`,
  `encoding/base64` (`StdEncoding`, `URLEncoding`), `crypto/sha256.Sum256`.
- **Deno `@std/encoding`** gives the same as plain functions.

Minimal `std.encoding` and `std.digest`, both pure hd:

```text
pub data DecodeError:
    pub offset: i32

pub fn hex_encode(bytes: List[u8]) -> string: pass
pub fn hex_decode(text: string) -> Result[List[u8], DecodeError]: pass
pub fn base64_encode(bytes: List[u8], url_safe: bool = false) -> string: pass
pub fn base64_decode(text: string, url_safe: bool = false) -> Result[List[u8], DecodeError]: pass

pub data Sha256:
    state: List[u32]
    pending: List[u8]
    length: u64

impl Sha256:
    pub fn new() -> mut Sha256: pass
    pub fn update(mut self, bytes: List[u8]) -> void: pass
    pub fn finish(mut self) -> List[u8]: pass

pub fn sha256(bytes: List[u8]) -> List[u8]:
    pass
```

SHA-256 needs wrapping `u32` additions, which hd can do in `u64` with a
mask. MD5, SHA-1, and HMAC are excluded; `std.fingerprint`'s algorithm is
still open ([Open Issues](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work)).

### Errors

Standouts:

- **Rust `anyhow::Context`** adds a message on the way up:
  `read(path).context("loading config")?`.
- **Go `errors.As`** finds a typed cause in a chain.

Minimal `std.error`. The spec already names `chain` and `report_of`, but
`lib/std` has neither:

```text
use std.error.Error

pub data ErrorReport:
    pub message: string
    pub causes: List[string]

pub fn chain(error: Error) -> List[Error]:
    pass

pub fn root_cause(error: Error) -> Error:
    pass

pub fn report_of(error: Error) -> ErrorReport:
    pass

impl[T, E < Error] Result[T, E]:
    pub fn context(self, message: string) -> Result[T, Error]:
        pass
```

### Concurrency Helpers

Standouts:

- **Deno `@std/async`**: `delay`, `deadline`, `retry` with
  `{maxAttempts, minTimeout, multiplier, maxTimeout}`, and `pooledMap`
  with a concurrency limit.
- **Go `context.WithTimeout`** and **Effect `Effect.timeoutOption`** turn a
  deadline into an ordinary outcome.

Minimal additions to `std.task`, all hd over existing intrinsics:

```text
use std.time.{Clock, Duration}

pub fn timeout![T](limit: Duration, task: mut Suspend[T]) -> T? $ Clock:
    race!(finished(task), expired(limit))

fn finished![T](task: mut Suspend[T]) -> T?:
    .Some(task!())

fn expired![T](limit: Duration) -> T? $ Clock:
    $.use(Clock).sleep!(limit)
    .None

pub data Backoff:
    pub attempts: i32
    pub initial: Duration
    pub factor: i32
    pub max: Duration

pub fn retry_with![T, E, $R](backoff: Backoff, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock:
    pass

pub fn all_list![T](tasks: List[mut Suspend[T]]) -> List[T]:
    pass

pub fn map_limited![T, U, $R](items: List[T], limit: i32, work: fn!(T) -> U $ R) -> List[U] $ R:
    pass
```

`timeout!` races the task against a sleep; the loser is cancelled by
[`req.combinator.race-losers`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers).
`all_list!` drives the existing `all_frame` intrinsic, as `race!` drives
`race_frame`. `map_limited!` runs batches of `limit` through `all_list!`:
a sliding window would need a new polling intrinsic, since user code
cannot write one
([`req.combinator.user`](../spec/lang/11-requirements-and-suspension.md#r-req.combinator.user)).
The structured `scope!` of decision 11 needs a new intrinsic too, so it
is a language-tier item and waits.

### Logging

Standouts:

- **Go `log/slog`**: levels, key-value fields, and a swappable handler.
- **Effect `Logger`**: the logger is a provided service, as an hd
  requirement would be.

A `std.log` with `info`, `warn`, and `error` over a `Log` requirement
trait waits on the decided but unapplied
[Observability Hooks](OPEN_ISSUES.md#observability-hooks). Until then,
`eprintln` covers script diagnostics.

### HTTP Client

Standouts:

- **`fetch`** (Node, Deno, Bun): one function from a request to a response.
- **Go `net/http/httptest`**: a scripted server for tests, as hd's
  `ScriptedHttp` would be.

The archived `Http.send!` sketch stands. Its host binding needs
`wasi:http` in the official runtime, and structured values across the
prototype's host bridge, which carries only scalars and strings. So HTTP
comes after the other host areas.

## A Script With The Proposed Surface

A task that counts words in the files a pattern names, and writes a JSON
summary. Every name past the prelude is a proposal from this plan:

```text
use std.console.{ErrorConsole, eprintln}
use std.fs.{FsError, FsRead, FsWrite, glob, read_text, write_text}
use std.host.{Args, args}
use std.json.{Json, Number, pretty}
use std.path.Path
use std.process.ExitCode

pub fn main!() -> Result[ExitCode, FsError] $ Args + FsRead + FsWrite + ErrorConsole:
    let argv = args()
    if argv.len() != 1:
        eprintln("usage: hd run count -- PATTERN")
        return .Ok(ExitCode(2))
    let totals: mut Map[string, Json] = {}
    for path in glob!(Path("."), argv[0])?:
        text := read_text!(path)?
        totals["$path"] = Json.Number(Number::from_i32(text.split_whitespace().len()))
    write_text!(Path("counts.json"), pretty(Json.Object(totals)))?
    .Ok(ExitCode(0))
```

The row lists exactly what the script touches, and `hd` binds each key
([`cli.host.entry-row`](../spec/cli/command-line.md#r-cli.host.entry-row)).
The file reads are bang calls, so the script needs `main!`: a script's top
level is not a driver
([`module.init.script-not-driver`](../spec/lang/10-modules.md#r-module.init.script-not-driver)).

## The Effect Review

Effect is a TypeScript library whose `Effect<A, E, R>` is a suspended
computation with a value, a typed error, and required services. hd has
each part in the language: `fn!() -> Result[A, E] $ R`.

### Covered By hd's Own Means

| Effect | hd today |
| --- | --- |
| `Effect<A, E, R>` | a `fn!` with a `Result` and a requirement row |
| `Context`, `Tag`, `Layer`, `Effect.provide` | requirement rows, `$.use`, `$.with`, row aliases, reusable contexts |
| `Effect.all`, `Effect.race` | `all!`, `race!` |
| `Effect.retry` with a count | `retry!` |
| interruption | `cancel`, which runs `defer` suites ([Cancellation](../spec/lang/11-requirements-and-suspension.md#cancellation)) |
| `Exit`, `Option`, `Result` | `Result`, `T?` |
| `Cause.Fail` versus `Cause.Die` | `.Err` versus a panic |
| `Data`, `Equal`, `Hash`, `Order` | derived `Eq`, `Hash`, `Ord` |
| `Match`, `Pipeable` | `match`, pipe expressions |
| `Arbitrary` | `std.testing` `Arbitrary` |
| `Brand`, `Newtype` | newtypes |

### Worth Adding To std

| Effect | hd addition | Tier |
| --- | --- | --- |
| `Clock`, `TestClock` | `Clock`, `ManualClock` (decision 10) | 4 |
| `Effect.sleep`, `Effect.delay` | `sleep!` | 4 |
| `Effect.timeoutOption` | `timeout!` returning `T?`; a typed error is `.ok_or(...)` | 8 |
| `Schedule.exponential` with `Schedule.recurs` | `Backoff` data and `retry_with!`, Deno's option set | 8 |
| `Duration` helpers | `Add`, `Sub`, `Ord`, `Display`, `minutes`, `as_seconds` | 4 |
| `Effect.forEach` with `concurrency` | `map_limited!`, batched | 8 |
| `Cause` pretty printing | `std.error` `chain`, `root_cause`, `report_of`, `context` | 4 |
| `Random` | `std.random` | 9 |
| `Config` with `ConfigProvider` | `Env` with `MapEnv`; a typed config template later | 1 |
| `FileSystem`, `Path`, `ChildProcess` | `std.fs`, `std.path`, `std.process` helpers | 1, 2 |
| `Encoding`, `Crypto` | `std.encoding`, `std.digest.sha256` | 5 |
| `DateTime` | UTC `Timestamp`, `Date`, RFC 3339 | 10 |
| `Cli` | `std.cli.parse_args` | 9 |
| `HashSet`, `Chunk`, queues | `Set`, `Deque`, `Heap` | 3, 10 |

### Excluded

| Effect | Why not in std |
| --- | --- |
| `Fiber`, `fork`, `FiberSet`, `FiberMap` | decision 11: structured scopes only, no detached tasks |
| `Scope`, `acquireRelease`, `addFinalizer` | `defer` is hd's cleanup, and async or fallible cleanup is the parked [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) question |
| `Schedule` combinators (`union`, `intersect`, `andThen`, `jittered`) | an algebra no other surveyed std ships; Deno's four retry options cover scripts |
| `Effect.repeat`, `repeatWhile` | a `while` loop with `sleep!` |
| `Cause.Parallel`, `Cause.Sequential` | `all!` children carry no error channel, and a failing `defer` is out of scope |
| `Stream`, `Sink`, `Channel`, `Pull` | `Iterator` covers pure pulls; a suspending stream needs a suspending `next` and handle lifetimes |
| `Queue`, `PubSub`, `Deferred`, `Latch`, `Semaphore` | useful only beside `scope!` tasks, which wait on an intrinsic |
| `Ref`, `SynchronizedRef`, STM (`TxRef`, `TxQueue`, ...) | one instance runs on one thread ([`module.instance.thread`](../spec/lang/10-modules.md#r-module.instance.thread)); `mut` is `Ref` |
| `Metric`, `Tracer`, `Logger` layers | wait on [Observability Hooks](OPEN_ISSUES.md#observability-hooks); exporters are providers |
| `Cache`, `ScopedCache`, `Pool`, `RcMap`, `RequestResolver` | application-level, and they need scopes |
| `Redacted` | decision 12 parks `Secret[T]` |
| `Cron`, `Trie`, `Graph`, `HashRing` | domain libraries; packages |
| `ManagedRuntime`, `Runtime` | hosts and entry drivers do this |
| `Ai`, `Cluster`, `Rpc`, `Sql`, `Workflow` | application domains; durable workflows are hd's replay runtime work |

## Ranked Rollout

Each tier is about one hour of `lib/std` work by one agent. Before it
starts, the owner approves its surface, and a `spec-update` pass writes
its stdlib-tier chapter (spec before implementation). Tiers that touch a
host also add a prototype host binding, a minimal TypeScript hook.

| Tier | Contents | Depends on | Value for scripts |
| --- | --- | --- | --- |
| 1 | `std.host` `Args`, `Env`, `MapArgs`, `MapEnv`, `args()`, `env()`; `ErrorConsole`, `eprintln`, `read_line!`, `read_all!` | [Q2](#q2-the-first-host-catalog), [Q4](#q4-standard-error); a `List[string]` result on the host bridge | a script can take input and report errors |
| 2 | `std.path` `Path`; `std.fs` `FsRead`, `FsWrite`, `FsError`, `MemoryFs`, `read_text!`, `write_text!`, `walk!`, `glob!` | tier 1's catalog; a Node `fs` binding in the prototype | a script can read and write files |
| 3 | collections and iterators: decided `Map` and `and_then` items, the `List`, `Iterator`, `Set`, and `counts` helpers | a list-truncate hook for `pop`, `remove_at`, `clear` | data shaping without hand loops |
| 4 | `std.error` helpers; `Duration` operators and `Display`; `Clock`, `Timestamp`, `Instant`, `ManualClock`, `now()`, `sleep!` | the catalog for `Clock` | timing, error reports |
| 5 | text helpers, `to_fixed`, `parse_f64`; `std.encoding` hex and base64; `std.digest` SHA-256 | two float hooks | formatting, checksums |
| 6 | `std.json` `Json`, `Number`, `parse`, `Display`, `pretty`, accessors | tier 5's `parse_f64` | reading and writing JSON |
| 7 | `ToJson` and `FromJson` templates; `encode`, `decode` | tier 6 | typed JSON |
| 8 | `timeout!`, `Backoff`, `retry_with!`, `all_list!`, `map_limited!` | tier 4's `Clock`; [Q5](#q5-retry-and-backoff) | robust automation |
| 9 | `std.random` `Rng`, `Random`, `SeededRandom`; `std.cli` `parse_args`, `usage` | tier 1's `Args`; `u64` wrapping arithmetic | real command-line tools |
| 10 | RFC 3339 and UTC `Date`; `Deque`, `Heap` | tier 4 | dates in logs and file names |
| 11 | `std.regex`: the RE2 subset, linear time, no backreferences, written in hd (about two hours) | tier 10 | filtering lines by pattern |

Later, blocked:

| Item | Blocked on | Kind |
| --- | --- | --- |
| `std.process` helpers bound for `hd FILE` and tasks | a toolchain profile that binds `Process` | host profile |
| `std.http` | `wasi:http` binding; structured values over the host bridge | runtime |
| `std.log` | [Observability Hooks](OPEN_ISSUES.md#observability-hooks), decided, not applied | language and runtime |
| `scope!`, `start`, `join!` | a new polling intrinsic | language tier |
| file handles, streaming, sockets | the parked NonEscapable design | language feature |
| `Process` with working directory and environment | a change to the language-tier `Process` trait | language tier |
| `std.bytes.Bytes` (decision 5) | none; it waits for a use that `List[u8]` serves badly | library |
| typed config from `Env`, typed CLI structs | tiers 7 and 9, as more `Source` templates | library |

Tier 3 needs no host decision, so it can run first if the catalog
question waits.

## Questions For The Owner

Batch 64 (2026-10-02) answered all six. Each answer is under its
question.

### Q1. The Earlier Std Decisions

**Decided** (STD-1): they still stand. The pure spec text went into
`spec/std`, and the rest is in
[Decided, Not Yet Applied](#decided-not-yet-applied).

**Effect.** The decisions in [Earlier Owner Decisions](#earlier-owner-decisions)
exist only in git history, so the next std agent may contradict them.

Options: (a) they still stand; record the unapplied ones in
[Open Issues](OPEN_ISSUES.md) as decided and unapplied; (b) they stand,
and this plan is their only record; (c) they are void, and each comes back
as a new question.

**Recommendation:** (a). Each is a small, already-made call, and Open
Issues is the backlog the shared rules name.

```text
let names: Map[string, i32] = {"a": 1}
let present = names.contains_key("a")  # Q16, decided 2026-09-29
```

### Q2. The First Host Catalog

**Decided** (STD-2): (a), without `ErrorConsole` (see Q4). Applied in
[Host Capabilities](../spec/cli/command-line.md#host-capabilities).

**Effect.** Without a catalog, no script can read arguments, files, or the
clock, and `cli.host.entry-row` binds only `Console`.

Options: (a) the default profile for `hd FILE`, `hd run`, and tasks binds
`Args`, `Env`, `ErrorConsole`, `ConsoleInput`, `Clock`, `Random`,
`FsRead`, and `FsWrite`; the row of the entry point still limits what a
program gets; (b) the same, without `FsWrite` and `Random` until later;
(c) a per-command flag such as Deno's `--allow-read`.

**Recommendation:** (a). The row already states the authority, which is
what Deno's flags state, and [`cli.host.entry-row`](../spec/cli/command-line.md#r-cli.host.entry-row)
says no flag repeats it. `Process` and `Http` follow later, as host
extensions.

```text
pub fn main!() -> void $ Args + FsRead + Console:
    pass
```

### Q3. Free Helpers Over Capability Traits

**Decided** (STD-3): (a). Every I/O helper is a bang function, and no
non-suspending wrapper exists.

**Effect.** With traits only, every call is
`$.use(FsRead).read_text!(path)`; scripts are mostly such calls.

Options: (a) each module adds free functions over its trait, as
`println` is over `Console`: `read_text!(path)`, `args()`, `now()`;
(b) also non-suspending wrappers through `block_on`, so a script's top
level can read files without `main!`; (c) trait methods only.

**Recommendation:** (a). It matches `println` and Go's `os.ReadFile`, and
adds no mechanism. (b) would give each I/O call two spellings.

```text
use std.fs.{FsRead, FsError, read_text}
use std.path.Path

fn load!() -> Result[string, FsError] $ FsRead:
    read_text!(Path("notes.txt"))
```

### Q4. Standard Error

**Decided** (STD-4): (b), changed. No `ErrorConsole`: the prelude
`Console` gains `write_error_line!`, and `eprintln` sits over it
([Standard Error](../spec/std/console.md#standard-error)). The
`ErrorConsole` sketches in this plan are superseded.

**Effect.** A script cannot report an error without mixing it into its
output, so `hd FILE | jq` breaks on the first warning.

Options: (a) a new trait `std.console.ErrorConsole` with the same
`write_line!`, and an imported `eprintln`; (b) a second method on the
prelude `Console`, a language-tier change; (c) no standard error until
`std.log`.

**Recommendation:** (a). It keeps `Console` and the prelude unchanged
(decision 7), and a program that only prints gains no extra authority.

```text
use std.console.{ErrorConsole, eprintln}

fn warn(message: string) -> void $ ErrorConsole:
    eprintln("warning: $message")
```

### Q5. Retry And Backoff

**Decided** (STD-5): (a). Applied in
[Retry With Backoff](../spec/std/task.md#retry-with-backoff).

**Effect.** `retry!` retries at once, so it hammers a busy service; Effect
answers with `Schedule`, Deno with four options.

Options: (a) a `Backoff` data value (`attempts`, `initial`, `factor`,
`max`) and `retry_with!`, requiring `Clock`; (b) Effect's composable
`Schedule` algebra; (c) only `sleep!`, and callers write the loop.

**Recommendation:** (a). It is Deno's `retry` option set and the archived
`RetryPolicy` draft, it stays deterministic under `ManualClock`, and it
leaves `retry!` as decided.

```text
use std.task.{Backoff, retry_with}
use std.time.{ms, s}

fn policy() -> Backoff:
    Backoff { attempts: 5, initial: 100ms, factor: 2, max: 5s }
```

### Q6. Regular Expressions

**Decided** (STD-6): (a). `std.regex` is tier 11 of the
[Ranked Rollout](#ranked-rollout), not yet specified.

**Effect.** Scripts often filter lines by pattern; without regex they
hand-write `find` loops. Fifteen of the twenty surveyed libraries ship
regex.

Options: (a) a `std.regex` of the RE2 subset, linear time, no
backreferences, written in hd, after tier 10; (b) a package outside
`std`, as Rust and OCaml do; (c) glob matching only (`Path.matches`) in
`std`.

**Recommendation:** (a). The single-file rule means a package is out of
reach for scripts, and Go's RE2 shows the subset is enough. It is the
largest pure tier, about two hours.

```text
use std.regex.Regex  # a proposed module

fn is_version(text: string) -> bool:
    Regex::new(r"^v\d+\.\d+$").matches(text)
```

## Sources

Effect:

- [Effect v4 API reference](https://effect.website/docs/v4/api/effect) (module list).
- [Built-in schedules](https://effect.website/docs/scheduling/built-in-schedules/).
- [Timing out](https://effect.website/docs/error-management/timing-out/).
- [Scope](https://effect.website/docs/resource-management/scope/).
- [Cause](https://effect.website/docs/data-types/cause/).

Standard library references, one per column:

- Go: [pkg.go.dev/std](https://pkg.go.dev/std); [testing/synctest](https://pkg.go.dev/testing/synctest).
- Rust: [doc.rust-lang.org/std](https://doc.rust-lang.org/std/).
- Zig: [ziglang.org/documentation/master/std](https://ziglang.org/documentation/master/std/).
- Nim: [nim-lang.org/docs/lib.html](https://nim-lang.org/docs/lib.html); [Nim 2.0 changelog](https://nim-lang.org/blog/2023/08/01/nim-v20-released.html) (checksums moved out).
- Java: [docs.oracle.com/en/java/javase/21/docs/api](https://docs.oracle.com/en/java/javase/21/docs/api/index.html).
- C#/.NET: [learn.microsoft.com/dotnet/api](https://learn.microsoft.com/en-us/dotnet/api/); [TimeProvider](https://learn.microsoft.com/en-us/dotnet/api/system.timeprovider).
- Python: [docs.python.org/3/library](https://docs.python.org/3/library/index.html).
- Ruby: [docs.ruby-lang.org](https://docs.ruby-lang.org/en/master/); [Ruby 3.4 bundled gems](https://www.ruby-lang.org/en/news/2024/12/25/ruby-3-4-0-released/).
- Julia: [docs.julialang.org standard library](https://docs.julialang.org/en/v1/).
- Node: [nodejs.org/api](https://nodejs.org/api/); [util.parseArgs](https://nodejs.org/api/util.html#utilparseargsconfig).
- Deno: [jsr.io/@std](https://jsr.io/@std); [@std/async](https://jsr.io/@std/async).
- Bun: [bun.sh/docs](https://bun.sh/docs); [Bun shell](https://bun.sh/docs/runtime/shell).
- Kotlin: [kotlinlang.org/api/core/kotlin-stdlib](https://kotlinlang.org/api/core/kotlin-stdlib/).
- Swift: [Swift standard library](https://developer.apple.com/documentation/swift/swift-standard-library); [SE-0329 Clock](https://github.com/apple/swift-evolution/blob/main/proposals/0329-clock-instant-duration.md).
- Scala: [scala-lang.org/api](https://www.scala-lang.org/api/current/).
- Dart: [api.dart.dev](https://api.dart.dev/).
- Haskell: [base on Hackage](https://hackage.haskell.org/package/base).
- OCaml: [OCaml standard library](https://ocaml.org/manual/latest/api/index.html).
- Elixir: [hexdocs.pm/elixir](https://hexdocs.pm/elixir/); [Elixir 1.18 JSON](https://hexdocs.pm/elixir/JSON.html).

hd sources: the deleted design record, `git show ed7fbfc5^:future-work/STDLIB.md`;
[lib/std/structure.hd](../lib/std/structure.hd);
[lib/std/task.hd](../lib/std/task.hd);
[src/host-functions.ts](../src/host-functions.ts).

## Parse Log

Each `text` block was parsed with `hd debug parse`. Parsing checks syntax
only; no block is type-checked.

| Block | Section | Result |
| --- | --- | --- |
| 1 | Program Arguments And Environment | parses |
| 2 | Console Output And Input | parses |
| 3, 4 | Files And Paths | parse |
| 5 | Processes | parses |
| 6 | JSON | parses |
| 7 | Time And Dates | parses |
| 8 | Text And Formatting | rejected at line 4: `expected ')', found '='`. The toy parser rejects a default on a method parameter; [`fn.default.allowed`](../spec/lang/07-functions.md#r-fn.default.allowed) allows it, so the parser lags the spec. |
| 9 | Collections And Iterators | parses |
| 10 | Random Numbers | parses |
| 11 | Command-Line Parsing | parses |
| 12 | Hashing And Encoding | parses |
| 13 | Errors | parses |
| 14 | Concurrency Helpers | parses |
| 15 | A Script With The Proposed Surface | parses |
| 16 to 21 | Questions Q1 to Q6 | parse |

A first draft of block 15 wrote module-qualified types, such as
`Map[string, json.Json]`, and the parser rejected them; the block now
imports the names.
