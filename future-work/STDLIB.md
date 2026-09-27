# Standard Library Design

Status: survey and first design draft for
[Roadmap area 4](ROADMAP.md#4-standard-library). Nothing here is accepted.
Accepted parts move into the [specification](../spec/README.md); unresolved
language questions stay in [Open Issues](OPEN_ISSUES.md).

The roadmap orders this work after areas 2 and 3 settle traits, derivation,
and requirements. This draft therefore fixes the parts that do not wait on
them: module layout, naming, the effect-trait pattern, and the list of
language features each module needs. Questions for the owner are collected at
the end.

Code sketches use only syntax that [the grammar](../spec/02-grammar.md)
accepts, and each block parses with the
[reference parser](../spec/reference-parser/index.ts). Elided bodies are
written `pass`; they are syntax placeholders, not type-correct bodies.

## Contents

1. [What The Specification Already Names](#what-the-specification-already-names)
2. [Survey](#survey)
3. [Design Principles](#design-principles)
4. [The hd Difference In Code](#the-hd-difference-in-code)
5. [Module Tree](#module-tree)
6. [Core Layer](#core-layer)
7. [Effect Layer](#effect-layer)
8. [Task Layer](#task-layer)
9. [Data Layer](#data-layer)
10. [Testing Layer](#testing-layer)
11. [Open Language Dependencies](#open-language-dependencies)
12. [Questions For The Owner](#questions-for-the-owner)

## What The Specification Already Names

The design must keep every name below. The prelude list is normative, and the
`prelude-name-shadow` rule means any name added to the prelude later breaks
every user module that already declares it.

| Module | Names fixed today | Source |
| --- | --- | --- |
| `std.core` | primitives, `List`, `Map`, `Any`, `Reference`, `Result`, `Ok`, `Err`, `panic` | [Prelude](../spec/10-modules.md#prelude) |
| `std.format` | `Display` | [Prelude](../spec/10-modules.md#prelude), [string interpolation](../spec/05-expressions.md) |
| `std.cmp` | `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Ordering` | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.hash` | `Hash`, `Hasher` | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.iter` | `Iterator`, `Iterable` | [For Loops](../spec/06-control-flow.md) |
| `std.console` | `Console`, `ConsoleError`, `println` | [Prelude](../spec/10-modules.md#prelude) |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker`, `block_on`, `host_wait!`, `HostWait`, `all!`, `race!`, a retry combinator | [Requirements and Suspension](../spec/11-requirements-and-suspension.md) |
| `std.annotation` | shape and annotator names | [Annotations](../spec/14-annotations.md) |
| `std.testing` | `assert`, `assert_equal` | [Standard Testing](../spec/10-modules.md#standard-testing) |
| `std.resource` | `ResourceError[E]` | [Wasm Boundary](../spec/10-modules.md#wasm-boundary) |
| `std.time` | `Duration` | [Use Forms](../spec/10-modules.md#use-forms) (example) |
| `std.host` | `Args` | [Program Entry Points](../guide/LANGUAGE_TOUR.md#program-entry-points) (example) |
| `std.fingerprint` | the persisted-identity digest | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `std.incremental` | incremental graph library | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `Observability`, `log.info` | provider draft and logging helper | [Observability](RUNTIME_AND_LIBRARY.md#observability) |

Other facts the library must respect:

- There is no `bytes` primitive; `Hasher.write` takes `List[u8]`.
- An inherent `impl` cannot target a primitive, so methods on `string`, `i32`,
  and other primitives come either from the normative built-in method table or
  from traits that the standard library implements for them.
- A trait's methods are callable with dot syntax only in modules that name the
  trait with `use` (or get it from the prelude).
- `decimal` is named as a possible library type.
- `Hash` values are process-dependent; persisted identity uses
  `std.fingerprint`.
- A provider returned by `$.use` is a readonly value of its trait type.

## Survey

### Summary Table

| Language | Core vs separate packages | How effects and IO are exposed | Testing seam for effects | Serialization | Naming and layout |
| --- | --- | --- | --- | --- | --- |
| Python | Large "batteries included" stdlib (~200 modules); pytest, requests, attrs, pydantic live outside | Ambient module functions: `open`, `time.time`, `random.random`, `os.environ`, `subprocess.run` | Monkeypatching globals (`unittest.mock.patch`), `freezegun`; no language seam | `json`, `pickle` in stdlib; typed models in third-party pydantic | Flat top-level names (`json`, `os.path`, `collections`), historical inconsistency |
| Rust | Three tiers: `core` (no allocation, no OS), `alloc`, `std` (OS). Most ecosystem basics (rand, serde, regex, chrono, tokio) are crates | Ambient free functions (`std::fs::read`, `Instant::now`, `std::env::var`); IO through `Read`/`Write` traits | Traits (`Read`, `Write`, generic `R: Read`); no `Clock` trait in std, so crates (`mock_instant`) or hand-written traits | Not in std; `serde` derive macros are the de facto standard | `std::collections`, `std::io`, `std::fs`; one concept per module |
| Kotlin | Small `kotlin-stdlib`; `kotlinx.coroutines`, `kotlinx.serialization`, `kotlinx-datetime`, `kotlinx-io` are separate libraries | JVM/platform APIs ambient; coroutines are a library over compiler `suspend` | `kotlin.time.Clock` interface (moved into stdlib in 2.1.20) is passed explicitly; `TestCoroutineScheduler` gives virtual time in `runTest` | `kotlinx.serialization` compiler plugin plus runtime library | `kotlin.collections`, `kotlin.time`, `kotlin.text` |
| Swift | Stdlib holds core types and concurrency; Foundation (now `swift-foundation`), `swift-collections`, `swift-algorithms`, `swift-system`, `swift-testing` are packages | Foundation ambient APIs (`FileManager.default`, `Date()`); structured concurrency (`async let`, task groups) in stdlib | `Clock` protocol (SE-0329) with `ContinuousClock` and `SuspendingClock`; tests inject custom clocks as generic parameters | `Codable` protocol with compiler-synthesized conformance | Package-per-concern, capitalized types, protocols as seams |
| Go | Large stdlib covering net/http, crypto, encoding, testing | Ambient functions (`os.ReadFile`, `time.Now`, `http.Get`); small interfaces (`io.Reader`, `io.Writer`, `fs.FS`) | Small interfaces: `fs.FS` with `testing/fstest.MapFS`, `httptest`, `iotest`; Go 1.25 `testing/synctest` bubbles give a fake clock automatically | `encoding/json` by reflection and struct tags | Short lowercase package names (`io`, `fs`, `http`), `x/` for staging |
| MoonBit | `builtin` plus a prelude that needs no import; the rest of `moonbitlang/core` is ordinary packages (`json`, `random`, `quickcheck`, `test`, `env`); async is a separate package | Mostly pure core; IO lives outside core, platform-dependent | Inline `test` blocks, snapshot `inspect`, `quickcheck` in core | `derive(ToJson, FromJson)` built into the compiler | `@pkg.name` qualified use; package manifest per directory |

### Takeaways

1. **Tiering is universal.** Every language separates a pure, allocation-level
   core from host facilities. Rust makes it a compile-time tier; MoonBit makes
   it `builtin` versus packages. hd gets the split for free: a pure module has
   an empty requirement row, and anything host-bound shows in `$`.
2. **Small stdlibs win on evolution, large ones on onboarding.** Kotlin moved
   `Clock` and `Instant` from a separate library into the stdlib only after
   they stabilized; calendar and time-zone logic stayed outside. Python's large
   stdlib has modules nobody can remove. The lesson for hd: ship pure
   primitives and effect traits in `std`, keep formats and domain libraries
   (calendar, HTTP servers, regex engines) as ordinary packages until proven.
3. **Every language that did not make effects injectable grew a patching
   culture.** Python patches globals, Rust grew `mock_instant` and
   per-project traits, Go added a runtime-level fake clock (`synctest`) nine
   years after `fs.FS`. Go's small interfaces (`io.Reader`, `fs.FS`) are the
   closest precedent for hd: a narrow trait plus an in-memory implementation in
   the standard library (`fstest.MapFS`).
4. **The test implementation belongs next to the interface.** Go ships
   `fstest.MapFS` and `httptest`; Kotlin ships `TestCoroutineScheduler` with
   coroutines. A deterministic provider that lives in a separate testing
   package tends to lag the interface.
5. **Virtual time needs the scheduler.** Kotlin's `runTest` and Go's
   `synctest` both advance fake time only when every task is blocked. A clock
   object alone cannot do that; the driver must say when it is idle.
6. **Serialization is always derivation.** Swift `Codable`, Kotlin
   serialization, serde, and MoonBit `derive(ToJson)` all rely on compiler
   generation. Go relies on reflection. hd has neither in a library-usable
   form yet (see [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets)).
   An untyped JSON value type needs neither and can ship first.
7. **Structured concurrency beat detached tasks.** Swift task groups, Kotlin
   coroutine scopes, and Trio nurseries all bound child lifetime to a scope.
   hd's one-shot `Suspend[T]` and synchronous cancellation fit that model.
8. **Naming: short, lowercase module names, one concept each.** Go, Rust, and
   MoonBit agree; hd's existing `std.cmp`, `std.hash`, `std.iter`,
   `std.task`, and `std.time` already follow Rust.

## Design Principles

1. **Every host effect is a requirement trait.** Clock, randomness,
   filesystem, network, environment, console, and subprocess access are traits
   in `$` rows. There is no ambient global such as `time.now()` or
   `fs.read_file()`. `println` is the existing model: prelude-visible but
   still `$ Console`.
2. **Pure modules need no requirement.** Collections, text, numbers, parsing,
   JSON values, fingerprints, and durations have empty rows. A reviewer can
   tell a pure module from its signatures alone.
3. **Each effect trait ships with a deterministic provider** in the same
   module: `ManualClock`, `SeededRandom`, `MemoryFs`, `MapEnv`,
   `BufferConsole`, `ScriptedHttp`, `ScriptedProcess`. They serve tests,
   replay debugging, and simulation, not only unit tests.
4. **Host providers cannot be constructed in hd code.** A real clock or
   filesystem reaches a program only through an entry-point row bound by the
   runtime profile. The standard library exposes the trait and the
   deterministic provider, never a public constructor for the host one.
5. **Narrow traits, attenuation by wrapping.** Traits are small enough that a
   provider can be wrapped to restrict it (a read-only or path-scoped
   filesystem) without new language features.
6. **Inputs that replay must record are suspending.** A read whose result can
   differ between runs goes through a bang call, so durable replay records it
   ([Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)). Invocation inputs
   fixed at start (arguments, environment) are recorded once with the
   invocation. See question 2.
7. **Errors are values.** Each domain has its own error enum implementing
   `Display`, like the existing `ConsoleError`. Panics stay reserved for bugs.
8. **Keep the prelude fixed.** New names go in modules and are imported. The
   shadowing ban makes every prelude addition a breaking change.
9. **Formats and domains stay out of `std` until proven.** Calendar and time
   zones, HTTP servers, regex, compression, and crypto beyond hashing start as
   packages (area 5).

## The hd Difference In Code

A function that reads configuration and stamps it with the current time
declares exactly what it touches:

```text
use std.fs.{FsRead, FsError}
use std.path.Path
use std.time.{Clock, Timestamp}

pub data Stamped:
    pub text: string
    pub loaded_at: Timestamp

pub fn load_stamped!(path: Path) -> Result[Stamped, FsError] $ FsRead + Clock:
    files, clock := $.use(FsRead, Clock)
    text := files.read_text!(path)?
    Ok(Stamped { text: text, loaded_at: clock.now!() })
```

The test binds deterministic providers through the ordinary provider scope.
No patching, no global state, no special test mode:

```text
use std.fs.{FsRead, MemoryFs}
use std.path.Path
use std.testing.{assert_equal, assert_ok}
use std.time.{Clock, ManualClock, Timestamp}

test "load_stamped uses the injected clock and files":
    start := Timestamp::from_unix_seconds(1_700_000_000)
    files := MemoryFs::with_files({"app.toml": "port = 8080"})
    clock := ManualClock::starting_at(start)
    $.with(FsRead=files, Clock=clock):
        stamped := assert_ok(
            load_stamped!(Path::parse("app.toml")),
            reason="the file exists in the in-memory filesystem",
        )
        assert_equal(stamped.text, "port = 8080", reason="text comes from MemoryFs")
        assert_equal(stamped.loaded_at, start, reason="time comes from ManualClock")
```

The entry point states the host capabilities it needs. The compiler derives
the provider-binding list from this row, and the runtime profile must bind
each key before `main!` runs:

```text
use std.fs.{FsRead, FsError}
use std.path.Path
use std.time.Clock

pub fn main!() -> Result[void, FsError] $ FsRead + Clock + Console:
    stamped := load_stamped!(Path::parse("app.toml"))?
    println(stamped.text)
    Ok()
```

A pure helper has no row at all, and a reviewer knows it touches nothing:

```text
use std.text.StringExt

pub fn port_line(text: string) -> string?:
    for line in text.split("\n"):
        if line.starts_with("port"):
            return line.trim()
    .None
```

Compared with the surveyed languages: Python would patch `open` and
`time.time`; Rust would thread a hand-written `Clock` generic; Go would use
`fs.FS` for files but `synctest` for time. In hd both seams are the same
mechanism, and the entry row doubles as the host permission list.

## Module Tree

Layers from bottom to top. A module may use modules in its own or a lower
layer only.

```text
std
├── core            prelude primitives, Result, panic          (fixed)
├── format          Display, Formatter, padding and number formatting
├── cmp             comparison traits (fixed), min, max, clamp, sort keys
├── hash            Hash, Hasher (fixed), default hasher
├── iter            Iterator, Iterable (fixed), adapters
├── num             checked, wrapping, saturating arithmetic; parsing; Integer, Float traits
├── bigint          arbitrary-precision integer (optional, see question 9)
├── decimal         decimal number
├── text            StringExt, StringBuilder, chars, UTF-8 encode and decode
├── bytes           Bytes (see question 5)
├── option          helpers for T?
├── result          helpers for Result[T, E]
├── error           Error trait, error chains
├── collections     Set, Deque, SortedMap, SortedSet, ListExt, MapExt
├── path            Path (pure, platform-neutral)
├── resource        ResourceError[E] (fixed)
├── cell            interior-mutable cell (only if question 1 chooses it)
│
├── time            Duration, Instant, Timestamp; Clock; ManualClock
├── random          Rng (pure PRNG); Random; SeededRandom
├── host            Args, Env; MapArgs, MapEnv
├── console         Console (fixed), ConsoleInput; BufferConsole
├── fs              FsRead, FsWrite, FsError; MemoryFs
├── http            Http, Request, Response; ScriptedHttp
├── net             sockets (later; after resource non-escape)
├── process         Process, Command, Output; ScriptedProcess
├── observe         Observability (from the runtime draft); RecordingObservability
├── log             info, warn, error helpers over Observability
│
├── task            Suspend protocol (fixed), block_on, all!, race!, timeout!, retry!, scope!
│
├── json            Json value, parse, print; typed codecs after derivation
├── fingerprint     Fingerprint, Algorithm, fingerprinting trait
├── secret          Secret[T], Redact
├── incremental     incremental computation (runtime draft)
│
├── annotation      shapes and annotators (fixed)
└── testing         assert (fixed), assertion helpers, hermetic contexts, property testing
```

Names follow the existing convention: lowercase single-word modules, one
concept each, and capitalized nominal types, including the literal-bearing
built-ins `List` and `Map`. Only primitive types such as `i32`, `bool`, and
`string` have lowercase names.

## Core Layer

All core modules are pure: no declaration here has a requirement row.

### `std.num`

Integer overflow is checked by default; the tour promises "explicit wrapping
APIs". Since primitives cannot take inherent methods, arithmetic variants are
trait methods implemented by `std` for every integer type:

```text
pub enum ParseNumberError:
    Empty
    InvalidDigit(position: i32)
    OutOfRange

pub trait Integer < Ord + Hash + Display:
    fn checked_add(self, other: Self) -> Self?
    fn checked_sub(self, other: Self) -> Self?
    fn checked_mul(self, other: Self) -> Self?
    fn checked_div(self, other: Self) -> Self?
    fn wrapping_add(self, other: Self) -> Self
    fn wrapping_sub(self, other: Self) -> Self
    fn wrapping_mul(self, other: Self) -> Self
    fn saturating_add(self, other: Self) -> Self
    fn saturating_sub(self, other: Self) -> Self
    fn abs_diff(self, other: Self) -> Self
    fn count_ones(self) -> i32
    fn leading_zeros(self) -> i32

pub trait Float < PartialOrd + Display:
    fn is_nan(self) -> bool
    fn is_finite(self) -> bool
    fn floor(self) -> Self
    fn ceil(self) -> Self
    fn round(self) -> Self
    fn sqrt(self) -> Self
    fn total_cmp(self, other: Self) -> Ordering

pub fn parse_i32(text: string) -> Result[i32, ParseNumberError]:
    pass

pub fn parse_i64(text: string) -> Result[i64, ParseNumberError]:
    pass

pub fn parse_f64(text: string) -> Result[f64, ParseNumberError]:
    pass
```

One `parse_*` function per type stands in for a generic
`parse[T < FromText](text)`: calling `T::parse` through a bound is still an
open question ([Runtime Type Identity, question 2](OPEN_ISSUES.md#runtime-type-identity-and-reified)).

### `std.text`

The built-in `string` methods are the six in the
[normative table](../spec/10-modules.md#prelude). The rest arrives through an
extension trait, so one `use std.text.StringExt` makes them dot-callable:

```text
pub trait StringExt:
    fn ends_with(self, suffix: string) -> bool
    fn contains(self, needle: string) -> bool
    fn find(self, needle: string) -> i32?
    fn upper(self) -> string
    fn trim_start(self) -> string
    fn trim_end(self) -> string
    fn strip_prefix(self, prefix: string) -> string?
    fn strip_suffix(self, suffix: string) -> string?
    fn chars(self) -> List[char]
    fn lines(self) -> List[string]
    fn repeat(self, count: i32) -> string
    fn is_empty(self) -> bool
    fn to_utf8(self) -> List[u8]

impl StringExt for string:
    fn ends_with(self, suffix: string) -> bool:
        pass

pub enum Utf8Error:
    InvalidSequence(position: i32)
    Truncated

pub fn from_utf8(bytes: List[u8]) -> Result[string, Utf8Error]:
    pass

pub fn join(parts: List[string], separator: string) -> string:
    pass

pub data StringBuilder:
    parts: List[string]

impl StringBuilder:
    pub fn new() -> mut StringBuilder:
        pass

    pub fn push(mut self, text: string) -> void:
        pass

    pub fn build(self) -> string:
        pass
```

Unicode rules follow the built-ins: scalar values, no locale, full case
mappings. Normalization, segmentation, and collation are later additions
(questions for a Unicode-data policy stay with the lexical Unicode version).

### `std.option` and `std.result`

The normative method table gives `T?` only `map`. Two candidate mechanisms
exist (question 8): grow the built-in table, or ship extension traits. The
sketch uses extension traits, because they need no specification change:

```text
pub trait OptionExt[T]:
    fn unwrap_or(self, fallback: T) -> T
    fn ok_or[E](self, error: E) -> Result[T, E]
    fn is_some(self) -> bool
    fn is_none(self) -> bool
    fn expect(self, message: string) -> T

pub trait ResultExt[T, E]:
    fn map_ok[U](self, transform: fn(T) -> U) -> Result[U, E]
    fn map_err[F](self, transform: fn(E) -> F) -> Result[T, F]
    fn ok(self) -> T?
    fn err(self) -> E?
    fn is_ok(self) -> bool
    fn unwrap_or(self, fallback: T) -> T
    fn expect(self, message: string) -> T
```

Callbacks take no row parameter here, so `map_err(fn(e): ...)` cannot use a
requirement. A row-polymorphic version, `fn map_err[F, R](self, transform:
fn(E) -> F $ R) -> Result[T, F] $ R`, is possible and matches
[Requirement Polymorphism](../spec/11-requirements-and-suspension.md#requirement-polymorphism);
it is the recommended final form.

### `std.error`

```text
pub trait Error < Display:
    fn cause(self) -> Error?:
        .None
```

[Runtime Type Identity](OPEN_ISSUES.md#runtime-type-identity-and-reified)
already directs that the standard error trait extends `Inspectable`, so a
chain can be searched for a concrete type with `downcast`. Until that issue
lands, `Error` has only `Display` and `cause`. Domain errors (`FsError`,
`HttpError`) are enums that implement `Error`.

### `std.collections`

`List` and `Map` stay built in. `set` is not part of the prelude, and the
survey favors library types for the rest:

```text
pub data Set[T < Eq + Hash]:
    entries: Map[T, bool]

impl[T < Eq + Hash] Set[T]:
    pub fn new() -> mut Set[T]:
        pass

    pub fn contains(self, value: T) -> bool:
        pass

    pub fn insert(mut self, value: T) -> bool:
        pass

    pub fn remove(mut self, value: T) -> bool:
        pass

    pub fn len(self) -> i32:
        pass

pub data Deque[T]:
    items: List[T]

pub data SortedMap[K < Ord, V]:
    keys: List[K]
    values: List[V]

pub trait ListExt[T]:
    fn filter(self, keep: fn(T) -> bool) -> List[T]
    fn sorted_by(self, compare: fn(T, T) -> Ordering) -> List[T]
    fn first(self) -> T?
    fn last(self) -> T?
    fn reversed(self) -> List[T]
    fn chunks(self, size: i32) -> List[List[T]]
    fn zip[U](self, other: List[U]) -> List[(T, U)]
```

`Set` iterates in insertion order to match `Map`. `SortedMap` gives ordered
iteration for deterministic output. Field layouts above are placeholders.

### `std.iter`

Adapters are methods of an extension trait over `mut Iterator[T]`, since the
prelude `Iterator` trait has only `next`:

```text
pub trait IteratorExt[T]:
    fn map_each[U](mut self, transform: fn(T) -> U) -> mut Iterator[U]
    fn filter(mut self, keep: fn(T) -> bool) -> mut Iterator[T]
    fn take(mut self, count: i32) -> mut Iterator[T]
    fn enumerate(mut self) -> mut Iterator[(i32, T)]
    fn collect(mut self) -> List[T]
    fn fold[A](mut self, initial: A, step: fn(A, T) -> A) -> A

pub fn range(start: i32, end: i32) -> mut Iterator[i32]:
    pass
```

Iterator adapters that call suspending code are not provided: comprehensions
already forbid suspension points, and the same rule keeps adapters simple.

### `std.cmp`, `std.hash`, `std.format`

These keep their fixed traits and add small helpers: `min`, `max`, `clamp`,
and `Reverse[T]` in `std.cmp`; a default `SipHasher`-style hasher in
`std.hash`; padding, radix, and precision formatting in `std.format`. None is
blocked; none needs a question.

### `std.path`

`Path` is a pure, platform-neutral value (WASI paths are relative to a
preopened directory). It exists so that `FsRead` does not take raw strings:

```text
pub data Path:
    segments: List[string]

impl Path:
    pub fn parse(text: string) -> Path:
        pass

    pub fn join(self, child: string) -> Path:
        pass

    pub fn join_path(self, child: Path) -> Path:
        pass

    pub fn parent(self) -> Path?:
        pass

    pub fn file_name(self) -> string?:
        pass
```

## Effect Layer

Every trait below is a requirement key. Each module has the same shape:

1. the trait (or a read/write pair);
2. a domain error enum implementing `Error`;
3. a deterministic provider with a public constructor;
4. no public constructor for the host provider.

### A gap: stateful providers

`$.use` returns a readonly value, and viewpoint adaptation makes a field
declared `mut U` read as `U` through it. A provider method with a `self`
receiver therefore cannot change the provider's own state. Host providers
hide their state in the host, so they are unaffected. Deterministic providers
are not: `ManualClock.advance`, `MemoryFs` writes, `BufferConsole` capture,
and the `RecordingObservability` example in
[Runtime and Library Design](RUNTIME_AND_LIBRARY.md#provider-strategies) all
need mutation through a readonly provider. Question 1 asks how to close this.
The sketches below assume a sealed `std.cell.Cell[T]` whose `set` works
through a readonly reference; that is option A of question 1.

### `std.time`

```text
pub data Duration:
    nanos: i64

impl Duration:
    pub fn nanoseconds(count: i64) -> Duration:
        pass

    pub fn milliseconds(count: i64) -> Duration:
        pass

    pub fn seconds(count: i64) -> Duration:
        pass

    pub fn as_nanoseconds(self) -> i64:
        pass

pub data Timestamp:
    unix_nanos: i64

impl Timestamp:
    pub fn from_unix_seconds(seconds: i64) -> Timestamp:
        pass

    pub fn plus(self, duration: Duration) -> Timestamp:
        pass

    pub fn since(self, earlier: Timestamp) -> Duration:
        pass

pub data Instant:
    ticks: i64

pub trait Clock:
    fn now!(self) -> Timestamp
    fn monotonic!(self) -> Instant
    fn sleep!(self, duration: Duration) -> void

pub data ManualClock:
    current: Cell[Timestamp]

impl ManualClock:
    pub fn starting_at(start: Timestamp) -> ManualClock:
        pass

    pub fn advance(self, duration: Duration) -> void:
        pass

impl Clock for ManualClock:
    fn now!(self) -> Timestamp:
        self.current.get()

    fn monotonic!(self) -> Instant:
        pass

    fn sleep!(self, duration: Duration) -> void:
        self.advance(duration)
```

`Timestamp` is UTC wall time; `Instant` is monotonic and meaningful only
against the clock that produced it. Calendar dates, time zones, and
formatting stay outside `std` at first, as Kotlin kept them in
`kotlinx-datetime`. `ManualClock.sleep!` advances immediately; a virtual-time
provider that fires timers only when the driver is idle is question 10.

### `std.random`

Randomness splits into a pure generator and an effect that seeds it:

```text
pub data Rng:
    state: u64

impl Rng:
    pub fn from_seed(seed: u64) -> mut Rng:
        pass

    pub fn next_u64(mut self) -> u64:
        pass

    pub fn below(mut self, bound: u64) -> u64:
        pass

    pub fn shuffle[T](mut self, items: mut List[T]) -> void:
        pass

pub trait Random:
    fn next_u64!(self) -> u64
    fn fill!(self, count: i32) -> List[u8]

pub fn rng!() -> mut Rng $ Random:
    Rng::from_seed($.use(Random).next_u64!())

pub data SeededRandom:
    generator: Cell[Rng]

impl SeededRandom:
    pub fn new(seed: u64) -> SeededRandom:
        pass
```

Code that needs many numbers takes one suspending seed and then runs the pure
`Rng`, so replay records one event instead of one per number. Cryptographic
randomness is `Random.fill!` from the host; no pure generator claims to be
cryptographic.

### `std.host`

`Args` already appears in the tour. Arguments and environment are fixed for
one invocation, so they are recorded once and their reads are not suspending:

```text
pub trait Args:
    fn program_name(self) -> string
    fn arguments(self) -> List[string]

pub trait Env:
    fn get(self, name: string) -> string?
    fn names(self) -> List[string]

pub data MapEnv:
    values: Map[string, string]

impl Env for MapEnv:
    fn get(self, name: string) -> string?:
        self.values.get(name)

    fn names(self) -> List[string]:
        pass

pub data MapArgs:
    program: string
    values: List[string]
```

Secrets do not come from `Env` as plain strings; see `std.secret`.

### `std.console`

`Console` and `println` are fixed. Input is a separate key, so a program that
only prints never gains read access:

```text
pub trait ConsoleInput:
    fn read_line!(self) -> Result[string?, ConsoleError]

pub data BufferConsole:
    lines: Cell[List[string]]

impl BufferConsole:
    pub fn new() -> BufferConsole:
        pass

    pub fn output(self) -> List[string]:
        self.lines.get()

impl Console for BufferConsole:
    fn write_line!(self, text: string) -> Result[void, ConsoleError]:
        pass
```

### `std.fs`

The sketch uses a read/write split (question 3). Whole-file operations come
first; streaming handles wait for
[Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy):

```text
use std.path.Path

pub enum FsError:
    NotFound(path: Path)
    PermissionDenied(path: Path)
    AlreadyExists(path: Path)
    NotADirectory(path: Path)
    InvalidData(message: string)
    Other(message: string)

pub enum EntryKind:
    File
    Directory
    Symlink

pub data Metadata:
    pub kind: EntryKind
    pub size: u64

pub trait FsRead:
    fn read!(self, path: Path) -> Result[List[u8], FsError]
    fn read_text!(self, path: Path) -> Result[string, FsError]
    fn list!(self, path: Path) -> Result[List[Path], FsError]
    fn metadata!(self, path: Path) -> Result[Metadata, FsError]

pub trait FsWrite:
    fn write!(self, path: Path, contents: List[u8]) -> Result[void, FsError]
    fn write_text!(self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir!(self, path: Path) -> Result[void, FsError]
    fn remove!(self, path: Path) -> Result[void, FsError]
    fn rename!(self, from: Path, to: Path) -> Result[void, FsError]

pub data MemoryFs:
    files: Cell[Map[string, List[u8]]]

impl MemoryFs:
    pub fn new() -> MemoryFs:
        pass

    pub fn with_files(files: Map[string, string]) -> MemoryFs:
        pass

    pub fn scoped(self, root: Path) -> MemoryFs:
        pass
```

Attenuation is ordinary wrapping. A caller can hand a subsystem a read-only
view or a subtree view without any language feature:

```text
use std.fs.{FsRead, FsError, Metadata}
use std.path.Path

data Subtree:
    inner: FsRead
    root: Path

impl FsRead for Subtree:
    fn read!(self, path: Path) -> Result[List[u8], FsError]:
        self.inner.read!(self.root.join_path(path))

    fn read_text!(self, path: Path) -> Result[string, FsError]:
        self.inner.read_text!(self.root.join_path(path))

    fn list!(self, path: Path) -> Result[List[Path], FsError]:
        self.inner.list!(self.root.join_path(path))

    fn metadata!(self, path: Path) -> Result[Metadata, FsError]:
        self.inner.metadata!(self.root.join_path(path))

fn index_docs!() -> Result[i32, FsError] $ FsRead:
    docs := Subtree { inner: $.use(FsRead), root: Path::parse("docs") }
    $.with(FsRead=docs):
        count_files!()
```

### `std.http` and `std.net`

WASI 0.3 exposes `wasi:http` as the portable network surface, so HTTP comes
before sockets:

```text
pub enum Method:
    Get
    Post
    Put
    Delete
    Patch

pub data Request:
    pub method: Method
    pub url: string
    pub headers: Map[string, string]
    pub body: List[u8]

pub data Response:
    pub status: u16
    pub headers: Map[string, string]
    pub body: List[u8]

pub enum HttpError:
    InvalidUrl(url: string)
    Connection(message: string)
    Timeout
    Denied(host: string)

pub trait Http:
    fn send!(self, request: Request) -> Result[Response, HttpError]

pub data ScriptedHttp:
    routes: Map[string, Response]
    requests: Cell[List[Request]]
```

`ScriptedHttp` answers from a route table and records what it received.
`std.net` (TCP, UDP, DNS) waits for resource non-escape, since a socket is a
live handle.

### `std.process`

WASI has no subprocess interface, so `Process` is a host-profile extension:
the default profile omits it, and a registration or tooling profile may bind
it.

```text
pub data Command:
    pub program: string
    pub arguments: List[string]
    pub environment: Map[string, string]
    pub stdin: List[u8]

pub data Output:
    pub status: i32
    pub stdout: List[u8]
    pub stderr: List[u8]

pub enum ProcessError:
    NotFound(program: string)
    Denied(program: string)
    Failed(message: string)

pub trait Process:
    fn run!(self, command: Command) -> Result[Output, ProcessError]

pub data ScriptedProcess:
    outputs: Map[string, Output]
```

### `std.observe` and `std.log`

These take the draft in
[Observability](RUNTIME_AND_LIBRARY.md#observability) as written:
`Observability` with `sample` and `emit`, the `Observation` enum, and
`log.info`, `log.warn`, `log.error` helpers with a `$ Observability` row.
`RecordingObservability` is the deterministic provider; it hits the
stateful-provider gap above.

## Task Layer

`std.task` holds the fixed `Suspend` protocol and the combinators the
specification already names. The combinators are compiler intrinsics with
ordinary `fn!` signatures:

```text
pub fn block_on[T](s: mut Suspend[T]) -> T:
    pass

pub fn all![Ts...](tasks: mut Suspend[Ts]...) -> (Ts...):
    pass

pub fn all_list![T](tasks: List[mut Suspend[T]]) -> List[T]:
    pass

pub fn race![T](tasks: List[mut Suspend[T]]) -> T:
    pass

pub enum Timeout[T]:
    Completed(value: T)
    Elapsed

pub fn timeout![T](limit: Duration, task: mut Suspend[T]) -> Timeout[T] $ Clock:
    pass

pub data RetryPolicy:
    pub attempts: i32
    pub initial_delay: Duration
    pub multiplier: f64

pub fn retry![T, E](
    policy: RetryPolicy,
    attempt: fn() -> mut Suspend[Result[T, E]],
) -> Result[T, E] $ Clock:
    pass
```

`timeout!` and `retry!` require `Clock`, so a test binds `ManualClock` and
the retry schedule becomes deterministic. `retry!` takes a constructor, not
a suspension, because a `Suspend[T]` runs once. `race!` takes a homogeneous
list; a heterogeneous race returns an enum the caller defines.

`Task[T]` is the open "higher-level" API. The recommendation (question 11) is
a structured scope rather than detached spawning:

```text
pub data Scope:
    id: i64

impl Scope:
    pub fn start[T](self, task: mut Suspend[T]) -> Task[T]:
        pass

pub data Task[T]:
    slot: i64

impl[T] Task[T]:
    pub fn join!(self) -> T:
        pass

    pub fn cancel(self) -> void:
        pass

pub fn scope![T](body: fn!(Scope) -> T) -> T:
    pass
```

`scope!` returns only after every started task finishes, and cancelling the
scope cancels its tasks, so no task outlives its parent. This preserves
one-shot `Suspend[T]` semantics: `start` takes ownership of driving, and
`join!` returns the stored result.

## Data Layer

### `std.json`

An untyped value and a parser and printer need no derivation and can ship now:

```text
pub enum Json:
    Null
    Bool(value: bool)
    Number(value: f64)
    Text(value: string)
    Array(items: List[Json])
    Object(fields: Map[string, Json])

pub data JsonError:
    pub message: string
    pub offset: i32

pub fn parse(text: string) -> Result[Json, JsonError]:
    pass

pub fn print(value: Json) -> string:
    pass

pub fn print_pretty(value: Json, indent: i32 = 2) -> string:
    pass
```

`Object` keeps insertion order because `Map` does. Integers beyond 2^53 need
a decision: a separate `Integer(value: i64)` case, or a raw-number case.

Typed encoding and decoding (`User` to `Json` and back) is blocked on
[Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
This draft does not invent that protocol. The library needs, from the
language:

1. **Field read by shape.** Given `T`, iterate its fields and read each one's
   value, typed. Shapes describe fields today but cannot read them.
2. **Construction by shape.** Build a `T` from decoded field values, including
   private-field rules (boundary values already require every crossing field
   to be `pub`).
3. **Target-indexed output.** Produce a `Decoder[T]` or `Encoder[T]` per type,
   so the result is typed rather than `Any`.
4. **Static calls through a bound.** A generic `decode[T < Decode](json)` must
   call `T::decode`; see
   [Runtime Type Identity, question 2](OPEN_ISSUES.md#runtime-type-identity-and-reified).
5. **Complete shape coverage** for every legal field type, or a clear rejection
   ([Complete Runtime Shape Coverage](OPEN_ISSUES.md#complete-runtime-shape-coverage)).
6. **Field metadata** for renames, defaults, and skipping; annotations already
   provide this.
7. **Enum encoding policy**: tagged, adjacent, or untagged, chosen per type.
8. **Redaction**: `Secret[T]` fields must not encode by default.
9. **Round-trip stability** that matches the boundary encoding, so a value
   crossing a registered boundary and one written to JSON agree.

The same needs cover property-test generators and tool schemas, which is why
the open issue groups them.

### `std.fingerprint`

Persisted identity for incremental computation and replay. The algorithm and
canonical encoding are research in
[Roadmap area 3](ROADMAP.md#3-runtime-durable-replay); the surface can be
fixed now:

```text
pub enum Algorithm:
    Sha256
    Blake3

pub data Fingerprint:
    pub algorithm: Algorithm
    pub version: u32
    pub digest: List[u8]

pub trait Fingerprinter:
    fn write_bytes(mut self, bytes: List[u8]) -> void
    fn write_text(mut self, text: string) -> void
    fn finish(mut self) -> Fingerprint

pub trait Fingerprintable:
    fn fingerprint(self, state: mut Fingerprinter) -> void

pub fn of[T < Fingerprintable](value: T) -> Fingerprint:
    pass

pub fn of_bytes(bytes: List[u8]) -> Fingerprint:
    pass
```

It mirrors `Hash` and `Hasher`, but the output is stable across processes and
carries its algorithm and version. Deriving `Fingerprintable` needs a
derivation rule, which falls under the same typed-derivation issue.

### `std.secret`

```text
pub data Secret[T]:
    value: T

impl[T] Secret[T]:
    pub fn new(value: T) -> Secret[T]:
        pass

    pub fn expose(self) -> T:
        self.value

impl[T] Display for Secret[T]:
    fn to_string(self) -> string:
        "<redacted>"

pub trait Redact:
    fn redacted(self) -> string

pub trait SecretStore:
    fn get!(self, name: string) -> Result[Secret[string], SecretError]

pub enum SecretError:
    NotFound(name: string)
    Denied(name: string)
```

A library type can already hide the value from `Display` and interpolation.
It cannot stop `Secret[T]` from crossing a registered boundary, being
serialized, or being logged through a derived encoder. Those rules belong to
[Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets)
and [Observability Hooks](OPEN_ISSUES.md#observability-hooks). See question 12.

## Testing Layer

`std.testing` keeps `assert` and `assert_equal` with their mandatory reason,
and adds three groups.

Assertion helpers, same shape:

```text
pub fn assert_ok[T, E < Display](result: Result[T, E], reason: string) -> T:
    pass

pub fn assert_err[T, E](result: Result[T, E], reason: string) -> E:
    pass

pub fn assert_some[T](value: T?, reason: string) -> T:
    pass

pub fn assert_contains(text: string, needle: string, reason: string) -> void:
    pass
```

A hermetic context bundles every deterministic provider, so one spread makes a
test independent of the host:

```text
use std.console.{Console, BufferConsole}
use std.fs.{FsRead, FsWrite, MemoryFs}
use std.host.{Env, MapEnv}
use std.random.{Random, SeededRandom}
use std.time.{Clock, ManualClock, Timestamp}

pub fn hermetic(seed: u64 = 0) -> $.Context[Clock + Random + Env + FsRead + FsWrite + Console]:
    files := MemoryFs::new()
    $.context(
        Clock=ManualClock::starting_at(Timestamp::from_unix_seconds(0)),
        Random=SeededRandom::new(seed),
        Env=MapEnv { values: {} },
        FsRead=files,
        FsWrite=files,
        Console=BufferConsole::new(),
    )

test "a hermetic run can override one provider":
    $.with(hermetic(seed=7)..., Env=MapEnv { values: {"MODE": "ci"} }):
        pass
```

Later entries win, so a test replaces one provider after the spread.

Property testing stays a library facility, as
[Runtime and Library Design](RUNTIME_AND_LIBRARY.md#testing) requires. The
part that needs no derivation is a strategy type over the pure `Rng`:

```text
pub data Strategy[T]:
    generate: fn(mut Rng, i32) -> T
    shrink: fn(T) -> List[T]

pub fn integers(low: i64, high: i64) -> Strategy[i64]:
    pass

pub fn lists[T](element: Strategy[T], max_len: i32) -> Strategy[List[T]]:
    pass

pub fn check[T](strategy: Strategy[T], property: fn(T) -> bool, reason: string, cases: i32 = 100) -> void:
    pass
```

Deriving a `Strategy[T]` from a type's shape waits for typed derivation.
Stateful testing and replay artifacts wait for area 3's event log.

## Open Language Dependencies

| Module | Depends on | Open item |
| --- | --- | --- |
| every deterministic provider, `std.observe` | mutation through a readonly provider | question 1 (no open issue yet) |
| `std.time`, `std.random`, `std.fs`, `std.http` | which host reads suspend for replay | [Replay Determinism](OPEN_ISSUES.md#replay-determinism-and-durable-workflows), question 2 |
| `std.num` (`parse[T]`), `std.json`, `std.testing` strategies | static calls through a bound | [Runtime Type Identity, question 2](OPEN_ISSUES.md#runtime-type-identity-and-reified) |
| `std.error` | `Inspectable` and `downcast` | [Runtime Type Identity](OPEN_ISSUES.md#runtime-type-identity-and-reified) |
| `std.json` typed codecs, `std.fingerprint` derive, property generators | typed derivation protocol | [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets) |
| schema output, tool adapters | shape cases for `mut`, trait values, `Any` | [Complete Runtime Shape Coverage](OPEN_ISSUES.md#complete-runtime-shape-coverage) |
| `std.secret` | boundary and encoder redaction contract | [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets), [Observability Hooks](OPEN_ISSUES.md#observability-hooks) |
| `std.fs` handles, `std.net`, `std.process` streaming | non-escaping handles and fallible cleanup | [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) |
| `std.observe`, `std.log` | task-local trace context | [Observability Hooks](OPEN_ISSUES.md#observability-hooks) |
| `std.incremental` | closure identity, weak references | [Serializable Closures](OPEN_ISSUES.md#serializable-closures-and-incremental-computation) |
| `std.task.all!` | variadic packs (could be cut) | [Scope Reduction](OPEN_ISSUES.md#scope-reduction-and-distinctive-requirements) |
| `std.task` virtual time | a driver idle signal | question 10 (no open issue yet) |
| attenuated providers (`for_tenant`) | principal and tenancy patterns | [Access Control](OPEN_ISSUES.md#access-control-and-tenancy-expressibility) |
| capability catalog, provider configuration, combinator set | library and runtime work | [Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work) |
| how `std` versions with the compiler | package tooling | [Roadmap area 5](ROADMAP.md#5-packages) |

## Owner Decisions

Decided 2026-09-26:

1. **Question 1: `$.use` can return `mut`.** A provider installed with
   `mut` access may be used as `$.use(mut Clock)`, so a deterministic
   provider such as `ManualClock` changes its own state through ordinary
   `mut self` methods. Applied 2026-09-26: the access rules are in
   [Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers).
   A provider is installed with `$.with(mut Clock=clock)`, requested in rows
   as `$ mut Clock`, and retrieved with `$.use(mut Clock)`.

2. **Question 2: I/O suspends; clock, random, and environment reads do not.**
   Filesystem and network reads are `!` calls; `clock.now()`,
   `rng.next_u64()`, and `env.get()` are plain calls. Replay records every
   host call at the boundary either way.
3. **Question 3: `FsRead` and `FsWrite`** (and `Console` / `ConsoleInput`).
4. **Question 4: deterministic providers live next to their trait**
   (`std.time.ManualClock`); `std.testing.hermetic()` bundles them.
5. **Question 5: a library `std.bytes.Bytes`**, readonly and compact,
   convertible to and from `List[u8]`.
6. **Question 6: one error enum per domain** (`FsError`, `HttpError`).
7. **Question 7: the prelude does not grow**; `Error`, `Duration`, `Set` are
   imported.
8. **Question 8: more methods on built-in types live in the standard library
   as inherent methods,** not in the normative table and not in extension
   traits. `std` owns the built-in types, so it may declare inherent impls
   for them, and those methods are available without a `use`.
9. **Question 9: `decimal` only** beyond the primitives; `BigInt` is a
   package.
10. **Question 10: virtual time auto-advances now** (`sleep!` on a manual
    clock returns at once); idle-driven timers come later with a driver
    hook.
11. **Question 11: structured scopes only.** `scope!` with `start` and
    `join!`; no task outlives its scope; an error or cancellation cancels the
    siblings.
13. **Question 13: ship untyped `std.json.Json` now, with a unified
    `Number`** modeled on `serde_json::Number`: a private representation
    (unsigned integer, signed integer, or finite float); constructors from
    every integer type and `Number::from_f64(x) -> Number?` rejecting NaN and
    infinity; accessors `is_i64`, `is_u64`, `is_f64`, `as_i64() -> i64?`,
    `as_u64()`, `as_f64()`; `Eq`, `Hash`, and printing as the original JSON
    text. `Json.Number(Number)` is a single variant.
12. **Question 12: `Secret[T]` is removed from the design for now.** It is
    too early; it is parked with typed derivation in
    [Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
    Sections of this document that mention `Secret[T]` or `Redact` are not
    part of the current design.

## Questions For The Owner

### 1. How does a deterministic provider change its own state?

`$.use` returns a readonly provider, so `ManualClock.advance` and
`MemoryFs.write!` cannot mutate through `self`.

- **A. Sealed interior-mutable cell.** `std.cell.Cell[T]` with
  `get(self) -> T` and `set(self, value: T) -> void`, implemented as a
  compiler intrinsic. Mutation through readonly becomes possible, but only
  where the type says `Cell`.
- **B. Mutable requirement keys.** Allow trait methods with `mut self` on
  requirement traits and make `$.use` return `mut Trait` when the bound value
  is mutable. This changes chapter 11 and the context-entry typing.
- **C. Runtime-backed test providers.** Deterministic providers are host
  objects created by an intrinsic, like real ones. No language change, but
  users cannot write their own stateful fakes.

**Recommendation: A.** It is a library type with one intrinsic, it matches
how host providers already behave, and users can write their own fakes.

```text
data CountingClock:
    calls: Cell[i32]

impl Clock for CountingClock:
    fn now!(self) -> Timestamp:
        self.calls.set(self.calls.get() + 1)
        Timestamp::from_unix_seconds(0)

    fn monotonic!(self) -> Instant:
        pass

    fn sleep!(self, duration: Duration) -> void:
        pass
```

### 2. Which host reads are suspending?

Replay records bang calls. A non-suspending read of the clock is invisible to
replay; a suspending one forces every caller to be `fn!`.

- **A.** Every host read is suspending (`now!`, `next_u64!`, `get!`).
- **B.** Live reads suspend (clock, random, filesystem, network); invocation
  inputs fixed at start (`Args`, `Env`) do not, and replay records them once
  with the invocation.
- **C.** No reads suspend; replay intercepts every host provider call,
  suspending or not.

**Recommendation: B.** It matches the replay rule that external inputs go
through suspending dependencies, and keeps configuration code
non-suspending. `Rng::from_seed` keeps bulk randomness cheap.

```text
fn deadline!(budget: Duration) -> Timestamp $ Clock + Env:
    clock, env := $.use(Clock, Env)
    extra := env.get("EXTRA_SECONDS")
    clock.now!().plus(budget)
```

### 3. One filesystem trait or several?

- **A.** One `Fs` trait.
- **B.** `FsRead` and `FsWrite`, bound separately.
- **C.** Finer traits: `FileRead`, `FileWrite`, `DirectoryList`, and more.

**Recommendation: B.** The row then shows whether a function can change the
disk, which is the fact reviewers care about. C multiplies keys without a
matching review benefit. The same split applies to `Console` and
`ConsoleInput`.

```text
fn build_report!(input: Path, output: Path) -> Result[void, FsError] $ FsRead + FsWrite:
    text := $.use(FsRead).read_text!(input)?
    $.use(FsWrite).write_text!(output, text.upper())
```

### 4. Where do deterministic providers live?

- **A.** Next to their trait (`std.time.ManualClock`).
- **B.** In `std.testing` (`std.testing.ManualClock`).

**Recommendation: A.** They serve replay debugging and simulation too, and
the survey shows test doubles kept apart from the interface drift from it.
`std.testing.hermetic` bundles them.

```text
use std.time.{Clock, ManualClock, Timestamp}

fn simulate!() -> Timestamp $ Clock:
    $.use(Clock).now!()
```

### 5. What is the byte-sequence type?

- **A.** `List[u8]` everywhere, as `Hasher.write` does today.
- **B.** A library `std.bytes.Bytes`: readonly, compact, convertible to and
  from `List[u8]`.
- **C.** A primitive `bytes` type with literals.

**Recommendation: B.** A `List[u8]` of Wasm GC references is wasteful for
file and network payloads; a library type avoids a grammar change. The
sketches above use `List[u8]` until this is decided.

```text
use std.bytes.Bytes

fn checksum(payload: Bytes) -> u32:
    pass
```

### 6. How are domain errors shaped?

- **A.** One enum per domain (`FsError`, `HttpError`), like `ConsoleError`.
- **B.** One shared `IoError` with a kind field.

**Recommendation: A.** Each row key then has its own error type, and a
`match` is exhaustive over errors that key can produce. A shared
`IoErrorKind` can be embedded where domains overlap.

```text
fn describe(error: FsError) -> string:
    match error:
        FsError.NotFound(path) => "missing"
        _ => "other"
```

### 7. Does the prelude grow?

Every prelude addition collides with user declarations of the same name.

- **A.** Keep the prelude as specified; new names are imported.
- **B.** Add a few (`Error`, `Duration`, `Set`) now, before users exist.

**Recommendation: A.** Adding them later would break code, and importing is
cheap for agents. If B is chosen, it must happen before any package is
published.

```text
use std.error.Error
use std.time.Duration
```

### 8. How do built-in types get more methods?

`string`, `T?`, `List`, and `Result` have a short normative method table.

- **A.** Grow the normative table in the specification.
- **B.** Extension traits in `std` (`StringExt`, `OptionExt`), dot-callable
  after one `use`.
- **C.** Free functions only (`text.ends_with(s, "x")`).

**Recommendation: B**, with A reserved for methods the compiler must
understand. It needs no specification change and keeps the method set of a
module visible from its `use` lines. It depends on the specification allowing
a std `impl` whose target is `T?`; that needs confirming.

```text
use std.option.OptionExt
use std.text.StringExt

fn greeting(name: string?) -> string:
    name.unwrap_or("guest").upper()
```

### 9. Which numeric types ship beyond primitives?

- **A.** None at first.
- **B.** `decimal` only (money and exact arithmetic).
- **C.** `decimal` and `BigInt`.

**Recommendation: B.** The tour already names `decimal`, and JSON and
money-handling tools need it; `BigInt` can be a package.

```text
use std.decimal.Decimal

fn total(prices: List[Decimal]) -> Decimal:
    pass
```

### 10. How does virtual time advance?

- **A.** `ManualClock.sleep!` advances immediately (auto-advance).
- **B.** Timers fire only when the driver is idle, as in Go `synctest` and
  Kotlin `runTest`. This needs an idle signal from the test driver.
- **C.** Only explicit `advance` moves time.

**Recommendation: A now, B later.** A makes `retry!` and `timeout!` tests
deterministic today. B is correct for concurrent code (a timeout racing a
fake network call) and needs a `std.task` driver hook, which belongs with
area 3.

```text
test "retry waits between attempts":
    clock := ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    $.with(Clock=clock):
        pass
```

### 11. What is `Task[T]`?

- **A.** Structured scopes only: `scope!` with `start` and `join!`; no task
  outlives its scope.
- **B.** Detached `spawn` returning a handle, as in Tokio.
- **C.** No `Task[T]`; only `all!`, `race!`, and friends.

**Recommendation: A.** It fits one-shot `Suspend[T]` and synchronous
cancellation, and it keeps replay order deterministic. B makes lifetime and
cancellation depend on handles that can be dropped.

```text
fn fetch_both!(left: Request, right: Request) -> (Response?, Response?) $ Http:
    scope!(fn!(tasks: Scope) -> (Response?, Response?):
        a := tasks.start(fetch(left))
        b := tasks.start(fetch(right))
        (a.join!(), b.join!())
    )
```

### 12. What may `Secret[T]` do at a boundary?

- **A.** A `Secret[T]` is not boundary-safe and never encodes.
- **B.** It crosses a registered boundary and encodes only through an
  explicit `expose`.
- **C.** It encodes as a redacted placeholder by default.

**Recommendation: A** until the typed-derivation contract exists, then
revisit B for tool inputs such as API keys.

```text
fn authorize!(token: Secret[string]) -> Result[void, HttpError] $ Http:
    header := "Bearer " + token.expose()
    Ok()
```

### 13. Does untyped JSON ship before typed derivation?

- **A.** Ship `std.json.Json`, `parse`, and `print` now.
- **B.** Wait and ship typed and untyped together.

**Recommendation: A.** It is pure, needs no language feature, and every
later typed codec goes through the same value type. The integer-precision
case (`Integer(value: i64)` or not) is part of this decision.

```text
use std.json

fn port(text: string) -> f64?:
    match json.parse(text):
        Ok(json.Json.Object(fields)) => pass
        _ => pass
    .None
```
