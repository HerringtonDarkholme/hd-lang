# Standard Library Design

Status: design draft for
[Roadmap area 4](ROADMAP.md#4-standard-library), revised to the
[owner decisions](#owner-decisions) of 2026-09-26. Nothing here is in the
specification yet. Accepted parts move into the
[specification](../spec/README.md); unresolved language questions stay in
[Open Issues](OPEN_ISSUES.md).

The roadmap orders this work after areas 2 and 3 settle traits, derivation,
and requirements. This draft therefore fixes the parts that do not wait on
them: module layout, naming, the effect-trait pattern, and the list of
language features each module needs. The owner's decisions, and the questions
they answered, are collected at the end.

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
12. [Owner Decisions](#owner-decisions)
13. [Questions For The Owner](#questions-for-the-owner)

## What The Specification Already Names

The design must keep every name below. The prelude list is normative, and the
`prelude-name-shadow` rule means any name added to the prelude later breaks
every user module that already declares it.

| Module | Names fixed today | Source |
| --- | --- | --- |
| `std.core` | primitives, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` | [Prelude](../spec/10-modules.md#prelude) |
| `std.format` | `Display` | [Prelude](../spec/10-modules.md#prelude), [string interpolation](../spec/05-expressions.md) |
| `std.cmp` | `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Ordering` | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.hash` | `Hash`, `Hasher` | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.iter` | `Iterator`, `Iterable` | [For Loops](../spec/06-control-flow.md) |
| `std.console` | `Console`, `ConsoleError`, `println` | [Prelude](../spec/10-modules.md#prelude) |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker`, `block_on`, `host_wait!`, `HostWait`, `all!`, `race!`, a retry combinator | [Requirements and Suspension](../spec/11-requirements-and-suspension.md) |
| `std.annotation` | shape and annotator names | [Annotations](../spec/14-annotations.md) |
| `std.testing` | `assert`, `assert_equal` | [Standard Testing](../spec/10-modules.md#standard-testing) |
| `std.resource` | `ResourceError[E]` | [Wasm Boundary](../spec/10-modules.md#wasm-boundary) |
| `std.convert` | `From[T]` | [Conversion Trait](../spec/09-traits.md#conversion-trait) |
| `std.error` | `Error` (a `Display` subtrait whose members all have defaults) | [Error Trait](../spec/09-traits.md#error-trait) |
| `std.time` | `Duration` | [Use Forms](../spec/10-modules.md#use-forms) (example) |
| `std.host` | `Args` | [Program Entry Points](../guide/LANGUAGE_TOUR.md#program-entry-points) (example) |
| `std.fingerprint` | the persisted-identity digest | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `std.incremental` | incremental graph library | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `Observability`, `log.info` | provider draft and logging helper | [Observability](RUNTIME_AND_LIBRARY.md#observability) |

Other facts the library must respect:

- There is no `bytes` primitive; `Hasher.write` takes `List[u8]`.
- User code cannot write an inherent `impl` for a primitive or another
  built-in type. By decision 8, `std` owns the built-in types and declares
  their extra methods as inherent methods, available without a `use`. This
  needs an exception in
  [Implementation Targets](../spec/09-traits.md#implementation-targets),
  which today bars inherent implementations on primitives.
- A trait's methods are callable with dot syntax only in modules that name the
  trait with `use` (or get it from the prelude).
- `decimal` is named as a possible library type.
- `Hash` values are process-dependent; persisted identity uses
  `std.fingerprint`.
- `$.use(K)` returns a readonly value of the trait type. `$.use(mut K)`
  returns `mut K` when the provider was installed with `$.with(mut K=value)`
  ([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)).
  A runtime profile binds a host provider with `mut` access for a trait it
  marks mutable, and readonly otherwise (decision 14).

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
   replay debugging, and simulation, not only unit tests. A provider that
   changes its own state does so through `mut self` methods, and is installed
   with `$.with(mut K=value)` (decision 1).
4. **Host providers cannot be constructed in hd code.** A real clock or
   filesystem reaches a program only through an entry-point row bound by the
   runtime profile. The standard library exposes the trait and the
   deterministic provider, never a public constructor for the host one.
5. **Narrow traits, attenuation by wrapping.** Traits are small enough that a
   provider can be wrapped to restrict it (a read-only or path-scoped
   filesystem) without new language features.
6. **I/O suspends; clock, random, and environment reads do not.**
   Filesystem and network operations are bang calls. `clock.now()`,
   `random.next_u64()`, and `env.get()` are plain calls. Replay records every
   host call at the boundary regardless of suspension
   ([Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules)). See decision 2.
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
    .Ok(Stamped { text: text, loaded_at: clock.now() })
```

The test binds deterministic providers through the ordinary provider scope.
No patching, no global state, no special test mode. The function only reads,
so readonly bindings are enough:

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
    .Ok()
```

A pure helper has no row at all, and a reviewer knows it touches nothing.
The string methods it calls are inherent methods from `std`, so it needs no
`use`:

```text
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
├── num             inherent checked, wrapping, saturating arithmetic; parsing; Integer, Float traits
├── decimal         decimal number
├── text            inherent string methods, StringBuilder, UTF-8 encode and decode
├── bytes           Bytes (decision 5)
├── option          inherent methods on T?
├── result          inherent methods on Result[T, E]
├── convert         From[T] (fixed)
├── error           Error trait (fixed), error chains
├── collections     Set, Deque, SortedMap, SortedSet; inherent List and Map methods
├── path            Path (pure, platform-neutral)
├── resource        ResourceError[E] (fixed)
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
├── task            Suspend protocol (fixed), block_on, all!, race!, timeout!, retry!, scope! (structured only)
│
├── json            Json value, parse, print; typed codecs after derivation
├── fingerprint     Fingerprint, Algorithm, fingerprinting trait
├── incremental     incremental computation (runtime draft)
│
├── annotation      shapes and annotators (fixed)
└── testing         assert (fixed), assertion helpers, hermetic contexts, property testing
```

Names follow the existing convention: lowercase single-word modules, one
concept each, and capitalized nominal types, including the literal-bearing
built-ins `List` and `Map`. Only primitive types such as `i32`, `bool`, and
`string` have lowercase names.

Arbitrary-precision integers are not in `std`; `BigInt` is an ordinary
package (decision 9). `std.cell` is gone: a provider changes its own state
through `mut self` methods and `$.use(mut K)` (decision 1). `std.secret`,
`Secret[T]`, and `Redact` are parked with typed derivation in
[Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets)
(decision 12).

## Core Layer

All core modules are pure: no declaration here has a requirement row.

### `std.num`

Integer overflow is checked by default; the tour promises "explicit wrapping
APIs". `std` declares the arithmetic variants as inherent methods on every
integer type (decision 8), so `count.checked_add(1)` needs no `use`. The
`Integer` and `Float` traits collect the same methods for generic code:

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
[normative table](../spec/10-modules.md#prelude). The rest are inherent
methods that `std` declares on `string` (decision 8). They are available in
every module without a `use`:

```text
impl string:
    pub fn ends_with(self, suffix: string) -> bool:
        pass

    pub fn contains(self, needle: string) -> bool:
        pass

    pub fn find(self, needle: string) -> i32?:
        pass

    pub fn upper(self) -> string:
        pass

    pub fn trim_start(self) -> string:
        pass

    pub fn trim_end(self) -> string:
        pass

    pub fn strip_prefix(self, prefix: string) -> string?:
        pass

    pub fn strip_suffix(self, suffix: string) -> string?:
        pass

    pub fn chars(self) -> List[char]:
        pass

    pub fn lines(self) -> List[string]:
        pass

    pub fn repeat(self, count: i32) -> string:
        pass

    pub fn is_empty(self) -> bool:
        pass

    pub fn to_utf8(self) -> List[u8]:
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

The normative method table gives `T?` only `map`. By decision 8, `std`
declares the rest as inherent methods on `T?` and `Result[T, E]`. They need
no `use`, and the normative table stays short:

```text
impl[T] T?:
    pub fn unwrap_or(self, fallback: T) -> T:
        pass

    pub fn ok_or[E](self, error: E) -> Result[T, E]:
        pass

    pub fn is_some(self) -> bool:
        pass

    pub fn is_none(self) -> bool:
        pass

    pub fn expect(self, message: string) -> T:
        pass

impl[T, E] Result[T, E]:
    pub fn map_ok[U](self, transform: fn(T) -> U) -> Result[U, E]:
        pass

    pub fn map_err[F](self, transform: fn(E) -> F) -> Result[T, F]:
        pass

    pub fn ok(self) -> T?:
        pass

    pub fn err(self) -> E?:
        pass

    pub fn is_ok(self) -> bool:
        pass

    pub fn unwrap_or(self, fallback: T) -> T:
        pass

    pub fn expect(self, message: string) -> T:
        pass
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

The specification fixes the trait's module, its `Display` supertrait, the
rule that every member has a default, and that an erased `Error` never
crosses a registered boundary ([Error Trait](../spec/09-traits.md#error-trait)).
`cause`, `Context`, `.context(...)`, and `chain` are library API. How `?`
combines errors from several domains is specified in
[Propagation](../spec/05-expressions.md#propagation), with the conversion
trait `std.convert.From` in
[Conversion Trait](../spec/09-traits.md#conversion-trait); the design record
is [Error Conversion](ERROR_CONVERSION.md).

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

impl[T] List[T]:
    pub fn filter(self, keep: fn(T) -> bool) -> List[T]:
        pass

    pub fn sorted_by(self, compare: fn(T, T) -> Ordering) -> List[T]:
        pass

    pub fn first(self) -> T?:
        pass

    pub fn last(self) -> T?:
        pass

    pub fn reversed(self) -> List[T]:
        pass

    pub fn chunks(self, size: i32) -> List[List[T]]:
        pass

    pub fn zip[U](self, other: List[U]) -> List[(T, U)]:
        pass
```

The `List` and `Map` methods beyond the normative table are inherent methods
declared by `std` (decision 8), like the `string` methods.

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

### Stateful providers

Deterministic providers keep state: `ManualClock` advances, `MemoryFs`
stores writes, `SeededRandom` steps its generator. Decision 1 gives them
ordinary `mut self` methods. A trait method that changes provider state takes
`mut self`; callers require `mut K` in their row and retrieve the provider
with `$.use(mut K)`; a test installs a mutable value with
`$.with(mut K=value)` and may keep its own `mut` alias to inspect the state
afterwards
([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)).

A host provider for a trait whose methods take `mut self` (`Clock.sleep!`,
`Random`, `FsWrite` below) is bound by a runtime profile that marks the trait
mutable (decision 14). An entry point then requires it as `$ mut K`; a
`mut K` entry for a trait the profile does not mark mutable is a
`mutable-upgrade` error. Which traits each toolchain profile marks is open in
[Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers).

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
    fn now(self) -> Timestamp
    fn monotonic(self) -> Instant
    fn sleep!(mut self, duration: Duration) -> void

pub data ManualClock:
    current: Timestamp

impl ManualClock:
    pub fn starting_at(start: Timestamp) -> mut ManualClock:
        pass

    pub fn advance(mut self, duration: Duration) -> void:
        self.current = self.current.plus(duration)

impl Clock for ManualClock:
    fn now(self) -> Timestamp:
        self.current

    fn monotonic(self) -> Instant:
        pass

    fn sleep!(mut self, duration: Duration) -> void:
        self.advance(duration)
```

Reading the clock is a plain call (decision 2); only `sleep!` suspends. A
test installs a manual clock with mutable access, so `sleep!` can advance it:

```text
use std.time.{Clock, Duration, ManualClock, Timestamp}

fn pause!(step: Duration) -> void $ mut Clock:
    $.use(mut Clock).sleep!(step)

fn simulate!() -> Timestamp:
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    $.with(mut Clock=clock):
        pause!(Duration::seconds(5))
    clock.now()
```

`Timestamp` is UTC wall time; `Instant` is monotonic and meaningful only
against the clock that produced it. Calendar dates, time zones, and
formatting stay outside `std` at first, as Kotlin kept them in
`kotlinx-datetime`. `ManualClock.sleep!` advances immediately (decision 10);
a virtual-time provider that fires timers only when the driver is idle comes
later with a driver hook.

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
    fn next_u64(mut self) -> u64
    fn fill(mut self, count: i32) -> List[u8]

pub fn rng() -> mut Rng $ mut Random:
    Rng::from_seed($.use(mut Random).next_u64())

pub data SeededRandom:
    generator: mut Rng

impl SeededRandom:
    pub fn new(seed: u64) -> mut SeededRandom:
        pass

impl Random for SeededRandom:
    fn next_u64(mut self) -> u64:
        self.generator.next_u64()

    fn fill(mut self, count: i32) -> List[u8]:
        pass
```

Random reads are plain calls (decision 2). Code that needs many numbers takes
one seed from the host and then runs the pure `Rng`, so replay records one
host call instead of one per number. Cryptographic randomness is
`Random.fill` from the host; no pure generator claims to be cryptographic.

### `std.host`

`Args` already appears in the tour. Argument and environment reads are
plain calls (decision 2), and replay records each one at the boundary:

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

### `std.console`

`Console` and `println` are fixed. Input is a separate key, so a program that
only prints never gains read access:

```text
pub trait ConsoleInput:
    fn read_line!(self) -> Result[string?, ConsoleError]

pub data BufferConsole:
    lines: mut List[string]

impl BufferConsole:
    pub fn new() -> mut BufferConsole:
        pass

    pub fn output(self) -> List[string]:
        self.lines

impl Console for BufferConsole:
    fn write_line!(self, text: string) -> Result[void, ConsoleError]:
        pass
```

A test installs the buffer with `$.with(mut Console=console)` and reads
`console.output()` through its own `mut` alias. The fixed
`Console.write_line!` takes `self`, so the buffer cannot append through it
yet. Recording needs `write_line!` to take `mut self`, a change to the
[prelude trait](../spec/10-modules.md#prelude). A profile may now bind the
host console with `mut` access (decision 14), so the host binding no longer
blocks it. Whether the change is worth `mut Console` in every printing row is
open in [Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers).

### `std.fs`

The traits are a read/write split (decision 3). Writes take `mut self`, so a
writer requires `mut FsWrite` and `MemoryFs` stores what it is given.
Whole-file operations come first; streaming handles wait for
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
    fn write!(mut self, path: Path, contents: List[u8]) -> Result[void, FsError]
    fn write_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir!(mut self, path: Path) -> Result[void, FsError]
    fn remove!(mut self, path: Path) -> Result[void, FsError]
    fn rename!(mut self, from: Path, to: Path) -> Result[void, FsError]

pub data MemoryFs:
    files: mut Map[string, List[u8]]

impl MemoryFs:
    pub fn new() -> mut MemoryFs:
        pass

    pub fn with_files(files: Map[string, string]) -> mut MemoryFs:
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
```

`ScriptedHttp` answers from a route table. Recording the requests it receives
would need `send!` to take `mut self`, which would put `mut Http` in every
caller's row; the sketch keeps `Http` readonly until a test needs the record.
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
`RecordingObservability` is the deterministic provider. It records through
`mut self` methods like the other stateful providers, and its host provider
needs a profile that marks `Observability` mutable (decision 14).

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

pub fn timeout![T](limit: Duration, task: mut Suspend[T]) -> Timeout[T] $ mut Clock:
    pass

pub data RetryPolicy:
    pub attempts: i32
    pub initial_delay: Duration
    pub multiplier: f64

pub fn retry![T, E](
    policy: RetryPolicy,
    attempt: fn() -> mut Suspend[Result[T, E]],
) -> Result[T, E] $ mut Clock:
    pass
```

`timeout!` and `retry!` sleep, so they require `mut Clock`. A test installs a
`ManualClock` with `$.with(mut Clock=clock)`, and the retry schedule becomes
deterministic. `retry!` takes a constructor, not
a suspension, because a `Suspend[T]` runs once. `race!` takes a homogeneous
list; a heterogeneous race returns an enum the caller defines.

`Task[T]` exists only inside a structured scope (decision 11). There is no
detached spawn:

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
scope cancels its tasks, so no task outlives its parent. An error or
cancellation in one task cancels its siblings. This preserves
one-shot `Suspend[T]` semantics: `start` takes ownership of driving, and
`join!` returns the stored result.

## Data Layer

### `std.json`

An untyped value and a parser and printer need no derivation and ship now
(decision 13). Numbers use one `Number` type modeled on `serde_json::Number`:

```text
pub enum Json:
    Null
    Bool(value: bool)
    Number(Number)
    Text(value: string)
    Array(items: List[Json])
    Object(fields: Map[string, Json])

pub data Number:
    repr: NumberRepr

enum NumberRepr:
    Unsigned(value: u64)
    Signed(value: i64)
    Float(value: f64)

impl Number:
    pub fn from_i64(value: i64) -> Number:
        pass

    pub fn from_u64(value: u64) -> Number:
        pass

    pub fn from_f64(value: f64) -> Number?:
        pass

    pub fn is_i64(self) -> bool:
        pass

    pub fn is_u64(self) -> bool:
        pass

    pub fn is_f64(self) -> bool:
        pass

    pub fn as_i64(self) -> i64?:
        pass

    pub fn as_u64(self) -> u64?:
        pass

    pub fn as_f64(self) -> f64?:
        pass

impl PartialEq for Number:
    fn eq(self, other: Number) -> bool:
        pass

impl Eq for Number

impl Hash for Number:
    fn hash(self, state: mut Hasher) -> void:
        pass

impl Display for Number:
    fn to_string(self) -> string:
        pass

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

`Object` keeps insertion order because `Map` does. The representation of
`Number` is private: an unsigned integer, a signed integer, or a finite float.
There is one constructor per integer type (`from_i8` through `from_u64`; the
sketch shows two). `from_f64` returns `.None` for NaN and infinity, so a
`Number` is always valid JSON. Integers keep full 64-bit precision, `Eq` and
`Hash` compare the representation, and `Display` prints the JSON text.

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
8. **Round-trip stability** that matches the boundary encoding, so a value
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

pub fn hermetic(seed: u64 = 0) -> $.Context[mut Clock + mut Random + Env + FsRead + mut FsWrite + mut Console]:
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    let random: mut SeededRandom = SeededRandom::new(seed)
    let files: mut MemoryFs = MemoryFs::new()
    let console: mut BufferConsole = BufferConsole::new()
    $.context(
        mut Clock=clock,
        mut Random=random,
        Env=MapEnv { values: {} },
        FsRead=files,
        mut FsWrite=files,
        mut Console=console,
    )

test "a hermetic run can override one provider":
    $.with(hermetic(seed=7)..., Env=MapEnv { values: {"MODE": "ci"} }):
        pass
```

Later entries win, so a test replaces one provider after the spread. The
stateful providers enter the context with `mut` access, so code under test
can advance the clock, step the generator, and write files.

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
| host providers for `Clock.sleep!`, `Random`, `FsWrite`; a recording `Console` | which traits each profile marks mutable; `write_line!` taking `mut self` | [Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers) |
| `std.time`, `std.random`, `std.host` | replay recording of non-suspending host calls (decision 2) | [Replay Determinism](OPEN_ISSUES.md#replay-determinism-and-durable-workflows) |
| inherent methods on `string`, `T?`, `List`, `Map`, integers (decision 8) | a `std` exception to the inherent-target rule | [Implementation Targets](../spec/09-traits.md#implementation-targets) |
| `std.num` (`parse[T]`), `std.json`, `std.testing` strategies | static calls through a bound | [Runtime Type Identity, question 2](OPEN_ISSUES.md#runtime-type-identity-and-reified) |
| `std.error` | `Inspectable` and `downcast` | [Runtime Type Identity](OPEN_ISSUES.md#runtime-type-identity-and-reified) |
| `std.json` typed codecs, `std.fingerprint` derive, property generators | typed derivation protocol | [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets) |
| schema output, tool adapters | shape cases for `mut`, trait values, `Any` | [Complete Runtime Shape Coverage](OPEN_ISSUES.md#complete-runtime-shape-coverage) |
| `std.fs` handles, `std.net`, `std.process` streaming | non-escaping handles and fallible cleanup | [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) |
| `std.observe`, `std.log` | task-local trace context | [Observability Hooks](OPEN_ISSUES.md#observability-hooks) |
| `std.incremental` | closure identity, weak references | [Serializable Closures](OPEN_ISSUES.md#serializable-closures-and-incremental-computation) |
| `std.task.all!` | variadic packs (could be cut) | [Scope Reduction](OPEN_ISSUES.md#scope-reduction-and-distinctive-requirements) |
| idle-driven virtual time (after decision 10) | a driver idle signal | no open issue yet |
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
12. **Question 12: `Secret[T]` is removed from the design for now.** It is
    too early; it is parked with typed derivation in
    [Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets).
    This document no longer sketches `std.secret`, `Secret[T]`, or
    `Redact`.
13. **Question 13: ship untyped `std.json.Json` now, with a unified
    `Number`** modeled on `serde_json::Number`: a private representation
    (unsigned integer, signed integer, or finite float); constructors from
    every integer type and `Number::from_f64(x) -> Number?` rejecting NaN and
    infinity; accessors `is_i64`, `is_u64`, `is_f64`, `as_i64() -> i64?`,
    `as_u64()`, `as_f64()`; `Eq`, `Hash`, and printing as the original JSON
    text. `Json.Number(Number)` is a single variant.

14. **Mutable host providers.** Runtime profiles may bind host providers as
    `mut` for traits the profile marks mutable (`Clock`, `Random`,
    `FsWrite`, `Console`); an entry row may then contain `$ mut K`. Tests
    keep installing deterministic providers with `$.with(mut K=...)`.
    Applied 2026-09-26: the rules are in
    [Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)
    and [Wasm Boundary](../spec/10-modules.md#wasm-boundary). Which traits
    each toolchain profile marks mutable stays open in
    [Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers).

## Questions For The Owner

All thirteen questions are decided; see [Owner Decisions](#owner-decisions).
Each entry keeps the options that were weighed and states the decision. The
examples follow the decided design.

### 1. How does a deterministic provider change its own state?

`$.use` returned only a readonly provider, so `ManualClock.advance` and
`MemoryFs.write!` could not mutate through `self`.

- **A. Sealed interior-mutable cell.** `std.cell.Cell[T]` with
  `get(self) -> T` and `set(self, value: T) -> void`, implemented as a
  compiler intrinsic.
- **B. Mutable requirement keys.** Allow trait methods with `mut self` on
  requirement traits and make `$.use` return `mut Trait` when the bound value
  is mutable.
- **C. Runtime-backed test providers.** Deterministic providers are host
  objects created by an intrinsic, like real ones.

**Decided: B** (decision 1). `$.use(mut K)` returns `mut K` for a provider
installed with `$.with(mut K=value)`; there is no `std.cell`. The rules are in
[Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers).
Binding a host provider with `mut` access is decision 14.

```text
use std.time.{Clock, Duration, ManualClock, Timestamp}

fn wait_twice!(step: Duration) -> void $ mut Clock:
    $.use(mut Clock).sleep!(step)
    $.use(mut Clock).sleep!(step)

fn elapsed!() -> Timestamp:
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    $.with(mut Clock=clock):
        wait_twice!(Duration::seconds(1))
    clock.now()
```

### 2. Which host reads are suspending?

- **A.** Every host read is suspending (`now!`, `next_u64!`, `get!`).
- **B.** Live reads suspend (clock, random, filesystem, network); invocation
  inputs fixed at start (`Args`, `Env`) do not.
- **C.** No reads suspend; replay intercepts every host provider call,
  suspending or not.

**Decided: I/O suspends; clock, random, and environment reads do not**
(decision 2). Filesystem and network operations are bang calls;
`clock.now()`, `random.next_u64()`, and `env.get()` are plain calls. Replay
records every host call at the boundary regardless of suspension.

```text
fn deadline(budget: Duration) -> Timestamp $ Clock + Env:
    clock, env := $.use(Clock, Env)
    extra := env.get("EXTRA_SECONDS")
    clock.now().plus(budget)
```

### 3. One filesystem trait or several?

- **A.** One `Fs` trait.
- **B.** `FsRead` and `FsWrite`, bound separately.
- **C.** Finer traits: `FileRead`, `FileWrite`, `DirectoryList`, and more.

**Decided: B** (decision 3). The row shows whether a function can change the
disk. The same split applies to `Console` and `ConsoleInput`. Writes take
`mut self`, so a writer requires `mut FsWrite`.

```text
fn build_report!(input: Path, output: Path) -> Result[void, FsError] $ FsRead + mut FsWrite:
    text := $.use(FsRead).read_text!(input)?
    $.use(mut FsWrite).write_text!(output, text.upper())
```

### 4. Where do deterministic providers live?

- **A.** Next to their trait (`std.time.ManualClock`).
- **B.** In `std.testing` (`std.testing.ManualClock`).

**Decided: A** (decision 4). They serve replay debugging and simulation too.
`std.testing.hermetic` bundles them.

```text
use std.time.{Clock, ManualClock, Timestamp}

fn simulate() -> Timestamp $ Clock:
    $.use(Clock).now()
```

### 5. What is the byte-sequence type?

- **A.** `List[u8]` everywhere, as `Hasher.write` does today.
- **B.** A library `std.bytes.Bytes`: readonly, compact, convertible to and
  from `List[u8]`.
- **C.** A primitive `bytes` type with literals.

**Decided: B** (decision 5). A `List[u8]` of Wasm GC references is wasteful
for file and network payloads, and a library type needs no grammar change.
The sketches above still write `List[u8]`; they move to `Bytes` when
`std.bytes` is sketched.

```text
use std.bytes.Bytes

fn checksum(payload: Bytes) -> u32:
    pass
```

### 6. How are domain errors shaped?

- **A.** One enum per domain (`FsError`, `HttpError`), like `ConsoleError`.
- **B.** One shared `IoError` with a kind field.

**Decided: A** (decision 6). Each row key has its own error type, and a
`match` is exhaustive over the errors that key can produce.

```text
fn describe(error: FsError) -> string:
    match error:
        FsError.NotFound(path) => "missing"
        _ => "other"
```

### 7. Does the prelude grow?

- **A.** Keep the prelude as specified; new names are imported.
- **B.** Add a few (`Error`, `Duration`, `Set`) now, before users exist.

**Decided: A** (decision 7). `Error`, `Duration`, and `Set` are imported.
One later owner exception: the sealed `AnyVal` joined `std.core` next to
`AnyRef`, the renamed `Reference`
([Revision Notes](../spec/README.md#revision-notes)).

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

**Decided: inherent methods declared in `std`** (decision 8), neither the
normative table nor extension traits. `std` owns the built-in types, so it
declares inherent implementations for them, and their methods need no `use`.
This needs a `std` exception in
[Implementation Targets](../spec/09-traits.md#implementation-targets).

```text
fn greeting(name: string?) -> string:
    name.unwrap_or("guest").upper()
```

### 9. Which numeric types ship beyond primitives?

- **A.** None at first.
- **B.** `decimal` only (money and exact arithmetic).
- **C.** `decimal` and `BigInt`.

**Decided: B** (decision 9). `BigInt` is an ordinary package, not a `std`
module.

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

**Decided: A now, B later** (decision 10). Idle-driven timers come with a
`std.task` driver hook.

```text
test "retry waits between attempts":
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    $.with(mut Clock=clock):
        pass
```

### 11. What is `Task[T]`?

- **A.** Structured scopes only: `scope!` with `start` and `join!`; no task
  outlives its scope.
- **B.** Detached `spawn` returning a handle, as in Tokio.
- **C.** No `Task[T]`; only `all!`, `race!`, and friends.

**Decided: A** (decision 11). An error or cancellation in one task cancels
its siblings.

```text
fn fetch_both!(left: Request, right: Request) -> (Response?, Response?) $ Http:
    scope!(fn!(tasks: Scope) -> (Response?, Response?):
        a := tasks.start(fetch(left))
        b := tasks.start(fetch(right))
        (a.join!(), b.join!())
    )
```

### 12. What may `Secret[T]` do at a boundary?

**Decided: not now** (decision 12). `Secret[T]` and `Redact` are removed from
the design and parked with typed derivation in
[Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets),
which keeps the options that were discussed.

### 13. Does untyped JSON ship before typed derivation?

- **A.** Ship `std.json.Json`, `parse`, and `print` now.
- **B.** Wait and ship typed and untyped together.

**Decided: A** (decision 13), with one `Number` type modeled on
`serde_json::Number` in a single `Json.Number(Number)` variant. See
[`std.json`](#stdjson).

```text
use std.json

fn port(text: string) -> i64?:
    match json.parse(text):
        .Ok(json.Json.Number(number)) => number.as_i64()
        _ => .None
```
