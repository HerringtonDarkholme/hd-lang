# Standard Library Design

Status: design draft for
[Roadmap area 4](ROADMAP.md#4-standard-library), revised to the
[owner decisions](#owner-decisions) of 2026-09-26 and 2026-09-29. Nothing
here is in the specification unless it links there. Accepted parts move into the
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
13. [Still Open](#still-open)
14. [Questions For The Owner](#questions-for-the-owner)

## What The Specification Already Names

The design must keep every name below. The prelude list is normative, and the
`prelude-name-shadow` rule means any name added to the prelude later breaks
every user module that already declares it.

The Tier column follows the [Spec Tiers inventory](SPEC_TIERS.md#inventory).
A language item stays in the numbered chapters. A std item moves to
[spec/std/](../spec/std/README.md) in its migration task, and until then its
rules stay where the Source column links. "Not in the spec" marks
undecided design.

| Module | Names fixed today | Tier | Source |
| --- | --- | --- | --- |
| `std.core` | primitives, `List`, `Map`, `Any`, `AnyVal`, `AnyRef`, `Option`, `Result`, `panic` | language | [Prelude](../spec/10-modules.md#prelude) |
| `std.format` | `Display`, `Debug`, `DebugWriter`, `debug` | language; `debug`'s text, `DebugWriter` builders and layout, derived builder calls: std | [Prelude](../spec/10-modules.md#prelude), [string interpolation](../spec/05-expressions.md), [Debug Trait](../spec/09-traits.md#debug-trait), [Format](../spec/std/format.md) |
| `std.cmp` | `Eq`, `PartialOrd`, `Ord`, `Ordering` | language | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.hash` | `Hash`, `Hasher` | language | [Comparison Traits](../spec/09-traits.md#comparison-traits) |
| `std.iter` | `Iterator` (a data type), `Iterable`, `FromIterator`; the adapters `filter`, `take`, `enumerate`, `map`, `fold`, and `collect` as `Iterator` methods | language: `Iterator`, `Iterable`; std: `FromIterator` and the adapters | [For Loops](../spec/06-control-flow.md), [Iterator Adapters](../spec/std/iter.md#iterator-adapters) |
| `std.console` | `Console`, `ConsoleError`, `println` | language | [Prelude](../spec/10-modules.md#prelude) |
| `std.task` | `Suspend`, `Poll`, `PollContext`, `Waker`, `block_on`, `host_wait!`, `HostWait`, `all!`, `race!`, `retry!` | language; std: `retry!`, a library loop (RETRY, batch 29) | [Requirements and Suspension](../spec/11-requirements-and-suspension.md), [Task](../spec/std/task.md) |
| `std.annotation` | shape names and `shape`, `shape_of`; `Target`, `Annotate`, `annotate` | language | [Annotations](../spec/14-annotations.md) |
| `std.testing` | `assert`, `assert_equal`, `it`, `it_each`, `it_prop`, `it_prop_with`, `snapshot`, `snapshot_file` | language: `assert`, `assert_equal`, `it`, the test-position rules, `snapshot`'s literal check; std: `it_each`, `it_prop`, and `it_prop_with` (ST6, revised), rows, generation, shrinking, draw budget, snapshot files, `timeout` | [Standard Testing](../spec/10-modules.md#standard-testing), [Testing](../spec/std/testing.md) |
| `std.resource` | `ResourceError[E]` | language | [Wasm Boundary](../spec/10-modules.md#wasm-boundary) |
| `std.convert` | `From[T]` | language | [Conversion Trait](../spec/09-traits.md#conversion-trait) |
| `std.error` | `Error` (a `Display` subtrait whose members all have defaults) | language | [Error Trait](../spec/09-traits.md#error-trait) |
| `std.ops` | `NumSuffix` and `num_suffix`, the literal-suffix marker; `StrPrefix`, `str_prefix`, and `Template`; the twelve operator traits `Add` to `Shr`, and `Index` and `IndexSet` | language | [Literal Suffixes](../spec/05-expressions.md#literal-suffixes), [Prefixed Strings](../spec/05-expressions.md#prefixed-strings), [Operator Traits](../spec/05-expressions.md#operator-traits), [Compound Assignment](../spec/05-expressions.md#compound-assignment), [Index Traits](../spec/05-expressions.md#index-traits) |
| `std.num` | the sealed traits `Num`, `Integer`, and `Float`, with `zero`, `one`, and `from_i64` on `Num` | language | [Numeric Traits](../spec/09-traits.md#numeric-traits) |
| `std.text` | the string prefix `r`; the `string` methods `trim`, `lower`, `split`, `replace`, and `starts_with` | std | [Text](../spec/std/text.md) |
| `std.time` | `Duration`, which implements `Neg`; the literal suffixes `ms`, `s`, `min`, `h` | std | [Time](../spec/std/time.md) |
| `std.host` | `Args` | not in the spec | [Program Entry Points](../guide/LANGUAGE_TOUR.md#program-entry-points) (example) |
| `std.fingerprint` | the persisted-identity digest | not in the spec | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `std.incremental` | incremental graph library | not in the spec | [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation) |
| `Observability`, `log.info` | provider draft and logging helper | not in the spec | [Observability](RUNTIME_AND_LIBRARY.md#observability) |

Other facts the library must respect:

- There is no `bytes` primitive; `Hasher.write` takes `List[u8]`.
- User code cannot write an inherent `impl` for a primitive or another
  built-in type. By decision 8, `std` owns the built-in types and declares
  their extra methods as inherent methods, available without a `use`
  ([`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std),
  applied 2026-09-27).
- A trait's methods are callable with dot syntax only in modules that name the
  trait with `use` (or get it from the prelude).
- `decimal` is named as a possible library type.
- `Hash` values from the standard `Hasher` are stable only within one code
  identity and runtime profile
  ([Durable Replay decision 8](RUNTIME_AND_LIBRARY.md#replay-rules)); persisted
  identity uses `std.fingerprint`.
- `$.use(K)` returns `mut K` when the trait `K` declares or inherits a
  `mut self` method, and a readonly `K` otherwise; rows and provider
  bindings never write `mut`
  ([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)).
  A runtime profile binds each host provider with that same access.

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
   with `$.with(K=value)` from a `mut` value (decision 1).
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
   `Display`, like the existing `ConsoleError`. Panics stay reserved for bugs,
   so an out-of-range count, such as `repeat(-1)` or `chunks(0)`, panics
   (question 18).
8. **Keep the prelude fixed.** New names go in modules and are imported. The
   shadowing ban makes every prelude addition a breaking change.
9. **Formats and domains stay out of `std` until proven.** Calendar and time
   zones, HTTP servers, regex, compression, and crypto beyond hashing start as
   packages (area 5).
10. **Value types compare.** Every `std` value type implements `Eq`,
    `Duration` also implements `Ord`, and every error enum implements
    `Display` (question 21). So `assert_equal` compares two durations, and
    `main` may return a `ParseNumberError`. Providers, builders, and handles,
    such as `ManualClock`, `StringBuilder`, and `Task`, are not value types
    ([Owner Decisions](#owner-decisions)).

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
    (files, clock) := $.use(FsRead, Clock)
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

tests:
    it("load_stamped uses the injected clock and files"):
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
    let console: mut Console = $.use(Console)
    _ := console.write_line!(stamped.text)  # println would panic under main!'s driver
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
├── num             Num, Integer, Float (fixed, sealed); inherent checked, wrapping, saturating arithmetic; parsing
├── decimal         decimal number
├── text            inherent string methods, StringBuilder, UTF-8 encode and decode
├── bytes           Bytes (decision 5)
├── option          inherent methods on T?
├── result          inherent methods on Result[T, E]
├── convert         From[T] (fixed)
├── ops             NumSuffix, num_suffix; operator, assign, and index traits (fixed)
├── error           Error trait (fixed), error chains
├── collections     Set, Deque, SortedMap, SortedSet; inherent List and Map methods
├── path            Path (pure, platform-neutral)
├── resource        ResourceError[E] (fixed)
│
├── time            Duration, Instant, Timestamp; Clock; ManualClock; suffixes ms s min h
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
├── annotation      shapes (fixed)
└── testing         assert (fixed), assertion helpers, hermetic contexts, property testing
```

Names follow the existing convention: lowercase single-word modules, one
concept each, and capitalized nominal types, including the literal-bearing
built-ins `List` and `Map`. Only primitive types such as `i32`, `bool`, and
`string` have lowercase names, apart from literal suffix newtypes such as
`ms`, which are named after the suffix they declare.

Arbitrary-precision integers are not in `std`; `BigInt` is an ordinary
package (decision 9). `std.cell` is gone: a provider changes its own state
through `mut self` methods, which `$.use(K)` can call (decision 1). `std.secret`,
`Secret[T]`, and `Redact` are parked with typed derivation in
[Open Issues](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets)
(decision 12).

## Core Layer

All core modules are pure: no declaration here has a requirement row.

### `std.num`

Integer overflow is checked by default; the tour promises "explicit wrapping
APIs". `std` declares the arithmetic variants as inherent methods on every
integer type (decision 8), so `count.checked_add(1)` needs no `use`.

The specification fixes three sealed traits
([Numeric Traits](../spec/09-traits.md#numeric-traits), Operator Traits
OP9). Only std implements them, for the primitive number types, and each
implementation's body is an intrinsic. `Num` carries `+ - * / %` through
its supertraits, and generic code builds constants with `zero`, `one`, and
`from_i64`, which is checked: a value the integer type cannot hold panics
with `integer-overflow` (Operator Traits OP10). `Num` also has `PartialOrd`
and `Display`:

```text
use std.ops.{Add, Div, Mul, Rem, Sub}

pub trait Num < AnyVal & PartialOrd & Display & Add[Out = Self] & Sub[Out = Self] & Mul[Out = Self] & Div[Out = Self] & Rem[Out = Self]:
    fn zero() -> Self
    fn one() -> Self
    fn from_i64(n: i64) -> Self
```

`Integer` and `Float` extend it with the supertraits the specification
lists, and collect the inherent methods for generic code. The earlier draft
had `Integer < Ord & Hash & Display`; the specification's list has no
`Hash`, which every primitive number implements anyway, and `Display`
comes through `Num`
([Numeric Traits](../spec/09-traits.md#numeric-traits)):

```text
use std.ops.{BitAnd, BitOr, BitXor, Neg, Not, Shl, Shr}

pub enum ParseNumberError:
    Empty
    InvalidDigit(position: i32)
    OutOfRange

pub trait Integer < Num & Ord & BitAnd[Out = Self] & BitOr[Out = Self] & BitXor[Out = Self] & Not[Out = Self] & Shl[u32, Out = Self] & Shr[u32, Out = Self]:
    fn checked_add(self, other: Self) -> Self?
    fn checked_sub(self, other: Self) -> Self?
    fn checked_mul(self, other: Self) -> Self?
    fn checked_div(self, other: Self) -> Self?
    fn wrapping_add(self, other: Self) -> Self
    fn wrapping_sub(self, other: Self) -> Self
    fn wrapping_mul(self, other: Self) -> Self
    fn saturating_add(self, other: Self) -> Self
    fn saturating_sub(self, other: Self) -> Self
    fn count_ones(self) -> i32
    fn leading_zeros(self) -> i32

pub trait Float < Num & PartialOrd & Neg[Out = Self]:
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

`abs_diff` returns the unsigned type of the same width, as Rust's does
(question 19), so the distance between the minimum and the maximum fits.
It is an inherent method of each integer type, not an `Integer` member,
because the result type differs per type
([Owner Decisions](#owner-decisions)):

| Receiver | `abs_diff` result |
| --- | --- |
| `i8`, `u8` | `u8` |
| `i16`, `u16` | `u16` |
| `i32`, `u32` | `u32` |
| `i64`, `u64` | `u64` |

```text
impl i32:
    pub fn abs_diff(self, other: i32) -> u32:
        pass
```

`parse_i32` and `parse_i64` accept the text of Rust's `str::parse` for
integers (question 20):

| Input | Result |
| --- | --- |
| an optional `+` or `-`, then one or more ASCII digits `0`-`9`, and nothing else | `.Ok(value)` |
| the empty string | `.Err(.Empty)` |
| a lone `+` or `-` | `.Err(.InvalidDigit(0))` |
| any other character, including `_`, a space, or a radix prefix | `.Err(.InvalidDigit(position))`, the byte offset of the first such character (Strings STR10) |
| digits whose value does not fit the type | `.Err(.OutOfRange)` |

Parsing user input accepts no literal syntax: `"1_000"` and `"0x10"` are
`InvalidDigit`.

One `parse_*` function per type stands in for a generic
`parse[T < FromText](text)`. This draft was written while calling `T::parse`
through a bound was open; it is now specified in
[Associated Function Calls](../spec/09-traits.md#associated-function-calls).

### `std.text`

The specified `string` methods are ten: `len`, `chars`, `char_indices`,
`bytes`, and `slice` in the language tier's
[normative table](../spec/10-modules.md#built-in-methods), four of them from
Strings STR4 and STR5, and `trim`, `lower`, `split`, `replace`, and
`starts_with` in [Text](../spec/std/text.md#string-methods). The rest are inherent
methods that `std` declares on `string` (decision 8). They are available in
every module without a `use`:

```text
impl string:
    pub fn ends_with(self, suffix: string) -> bool:
        pass

    pub fn contains(self, needle: string) -> bool:
        pass

    pub fn find(self, needle: string) -> i32?:  # a byte offset (STR6)
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

    pub fn lines(self) -> List[string]:
        pass

    pub fn repeat(self, count: i32) -> string:
        pass

    pub fn is_empty(self) -> bool:
        pass

    pub fn to_utf8(self) -> List[u8]:
        pass

    pub fn from_utf8(bytes: List[u8]) -> Result[string, Utf8Error]:
        pass

pub enum Utf8Error:
    InvalidSequence(position: i32)
    Truncated

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

`std.text` also holds the raw-text prefix `r` and the prefix helpers
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes) L20 to L22), which
a prefix function imports with `use std.text.{interpolate, process_escapes}`:

```text
pub fn interpolate[T < Display](t: Template[T]) -> string:
    pass

pub data EscapeError:
    pub offset: i32

pub fn process_escapes(text: string) -> Result[string, EscapeError]:
    pass
```

`lines` follows Rust's `str::lines` (question 17). It splits at each `\n`,
and removes one `\r` directly before a `\n`. A final `\n` ends the last line
and starts no empty one, and a `\r` not followed by `\n` stays:

| Text | `lines()` |
| --- | --- |
| `"a\nb"` | `["a", "b"]` |
| `"a\nb\n"` | `["a", "b"]` |
| `"a\r\nb\r\n"` | `["a", "b"]` |
| `"a\n\nb"` | `["a", "", "b"]` |
| `""` | `[]` |

`repeat` with a negative count panics (question 18); `repeat(0)` is `""`.

`interpolate` joins the pieces with the values' `Display` text.
`process_escapes` fails at the first invalid escape, and
`EscapeError.offset` is the byte offset of its backslash: every position in
std counts bytes, as string positions do
(Strings STR10).

`string::from_utf8` is the one checked conversion from bytes that
[`types.string.from-bytes`](../spec/04-type-system.md#r-types.string.from-bytes)
requires (Strings STR1). `Utf8Error.InvalidSequence` gives the byte offset
of the first invalid sequence. `to_utf8` copies a string's bytes out.

Unicode rules follow the built-ins: valid UTF-8, byte offsets, no locale,
full case mappings. Normalization, segmentation, and collation are later additions
(questions for a Unicode-data policy stay with the lexical Unicode version).

`lib/std/text.hd` follows Strings STR1-STR6 and STR10 (applied
2026-09-29): `len` is `byte_len(self)`, `find` and `EscapeError.offset`
are byte offsets, and `chars`, `char_indices`, `bytes`, `slice`, and
`string::from_utf8` are on `impl string`. `parse_i32` and `parse_i64` in
`lib/std/num.hd` still walk the text through `split("")`, but every
character before the first invalid one is ASCII, so the scalar index they
report as `ParseNumberError.InvalidDigit` is also its byte offset.

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

    pub fn and_then[U](self, transform: fn(T) -> U?) -> U?:
        pass

impl[T, E] Result[T, E]:
    pub fn map_ok[U](self, transform: fn(T) -> U) -> Result[U, E]:
        pass

    pub fn and_then[U](self, transform: fn(T) -> Result[U, E]) -> Result[U, E]:
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

`and_then` chains a second fallible step, as Rust's
[`Option::and_then`](https://doc.rust-lang.org/std/option/enum.Option.html#method.and_then)
does (question 15). It calls `transform` only on `.Some` or `.Ok`, and
passes `.None` or `.Err` through unchanged.

Callbacks take no row parameter here, so `map_err(fn(e): ...)` cannot use a
requirement. A row-polymorphic version, `fn map_err[F, R](self, transform:
fn(E) -> F $ R) -> Result[T, F] $ R`, is possible and matches
[Requirement Polymorphism](../spec/11-requirements-and-suspension.md#requirement-polymorphism);
it is the recommended final form.

### `std.error`

```text
use std.inspect.Inspectable

pub trait Error < Display & Inspectable:
    fn cause(self) -> Error?:
        .None

    fn find[T < AnyRef & Inspectable](self) -> T?:
        for part in chain(self):
            match part.downcast::[T]():
                .Some(found) => return .Some(found)
                .None => pass
        .None

pub fn chain(error: Error) -> List[Error]:
    let found: mut List[Error] = [error]
    let current: Error? = error.cause()
    while true:
        match current:
            .Some(next) =>
                if seen(found, next):
                    break                        # a cyclic chain stops at the first repeat
                found.append(next)
                current = next.cause()
            .None => break
    found

pub fn root_cause(error: Error) -> Error:
    let parts: List[Error] = chain(error)
    parts[parts.len() - 1]

fn seen(parts: List[Error], candidate: Error) -> bool:
    for part in parts:
        if part is candidate:                    # identity: errors are reference values
            return true
    false
```

Context and boundary reports (Error Conversion decision
15):

```text
pub data Context:
    pub message: string
    pub cause: Error

impl Display for Context:
    fn to_string(self) -> string:
        self.message

impl Error for Context:
    fn cause(self) -> Error?:
        .Some(self.cause)

pub fn context[T, E < Error](result: Result[T, E], message: string) -> Result[T, Error]:
    match result:
        .Ok(value) => .Ok(value)
        .Err(error) => .Err(Context { message: message, cause: error })

pub fn with_context[T, E < Error](result: Result[T, E], message: fn() -> string) -> Result[T, Error]:
    match result:
        .Ok(value) => .Ok(value)                 # the message is never built on success
        .Err(error) => .Err(Context { message: message(), cause: error })

pub data ErrorReport:                    # boundary-safe snapshot
    pub message: string
    pub causes: List[string]

pub fn report_of(error: Error) -> ErrorReport:
    let causes: mut List[string] = []
    for part in chain(error):
        if !(part is error):                     # the causes after the top error
            causes.append(part.to_string())
    ErrorReport { message: error.to_string(), causes: causes }
```

`.context(...)` and `.with_context(...)` are written as methods on
`Result` once method syntax for them is settled; the free functions show
their typing. `chain` stops at the first part it has already visited, so a
cyclic chain cannot loop (`find` and `root_cause` walk `chain`), and
`ErrorReport.causes` excludes the top error, matching the entry point's
`caused by:` lines (Error Conversion review gaps 5-7, decided
2026-09-27).

`Error` extends the sealed `Inspectable`
([Runtime Type Identity](../spec/09-traits.md#runtime-type-identity)), so it
inherits `downcast` and `downcast_mut`, and a chain can be searched for a
concrete type. `error.find[T]()` is Go's `errors.As`: a default method,
bounded like `downcast` ([Inspectable decisions 11 and 15](../spec/09-traits.md#runtime-type-identity)),
that walks `chain` and returns the first part whose recorded type is exactly
`T`. A value-type error payload is found with `chain` and
`std.inspect.downcast_val`. `root_cause` returns the last part. Domain errors
(`FsError`, `HttpError`) are enums that implement `Error`; their
`Inspectable` part is supplied by the compiler.

The specification fixes the trait's module, its `Display` and `Inspectable`
supertraits, the rule that every member has a default, and that an erased
`Error` never crosses a registered boundary
([Error Trait](../spec/09-traits.md#error-trait)). It also fixes `cause`'s
signature, because the `@error` intrinsic generates `cause`
([Error Derivation](../spec/14-annotations.md#error-derivation)).
`Context`, `.context(...)`, `chain`, `find`, and `root_cause` are library
API. How `?`
combines errors from several domains is specified in
[Propagation](../spec/05-expressions.md#propagation), with the conversion
trait `std.convert.From` in
[Conversion Trait](../spec/09-traits.md#conversion-trait), and error
derivation in [Error Derivation](../spec/14-annotations.md#error-derivation).

### `std.collections`

`List` and `Map` stay built in. `set` is not part of the prelude, and the
survey favors library types for the rest:

```text
pub data Set[T < Eq & Hash]:
    entries: Map[T, bool]

impl[T < Eq & Hash] Set[T]:
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

impl[K, V] Map[K, V]:
    pub fn contains_key(self, key: K) -> bool:
        pass

    pub fn keys(self) -> List[K]:
        pass

    pub fn values(self) -> List[V]:
        pass
```

The `List` and `Map` methods beyond the normative table are inherent methods
declared by `std` (decision 8), like the `string` methods. `keys` and
`values` list the entries in insertion order, as `for` visits them
(question 16). `chunks` with a size below 1 panics (question 18).

`Set` iterates in insertion order to match `Map`. `SortedMap` gives ordered
iteration for deterministic output. Field layouts above are placeholders.

### `std.iter`

`Iterator[T]` is a concrete `data` type that holds a `step` closure, and
the adapters are its ordinary methods
(Chaining Study CS7, CS8). This
replaces the `Iterator` trait of question 14. The specification fixes
`next`, `filter`, `take`, `enumerate`, `map`, `fold`, and `collect` in
[Iteration Protocols](../spec/06-control-flow.md#iteration-protocols) and
[Iterator Adapters](../spec/std/iter.md#iterator-adapters). `step`
is private, and user code builds an iterator with `Iterator::from_fn`
(Chaining Study CS9):

```text
pub data Iterator[T]:
    step: fn() -> T?

impl[T] Iterator[T]:
    pub fn from_fn(step: fn() -> T?) -> mut Iterator[T]:
        pass

    pub fn next(mut self) -> T?:
        pass

    pub fn filter(mut self, keep: fn(T) -> bool) -> mut Iterator[T]:
        pass

    pub fn take(mut self, count: i32) -> mut Iterator[T]:
        pass

    pub fn enumerate(mut self) -> mut Iterator[(i32, T)]:
        pass

    pub fn map[U](mut self, transform: fn(T) -> U) -> mut Iterator[U]:
        pass

    pub fn fold[A, R](mut self, initial: A, step: fn(A, T) -> A $ R) -> A $ R:
        pass

    pub fn collect[C < FromIterator[T] = List[T]](mut self) -> C:
        pass

pub trait FromIterator[T]:
    fn from_iter(items: mut Iterator[T]) -> Self

pub fn range(start: i32, end: i32) -> mut Iterator[i32]:
    pass
```

`collect` builds the target that the expected type names, or its declared
default `List[T]` when nothing does (Collect CO1 and CO5,
[Collect Targets](../spec/std/iter.md#collect-targets)). `std`
implements `FromIterator` for `List`, `Map` (the last value of an equal key
wins), all-or-nothing `Result[C, E]` and `C?`, and `Set`. Convenience
names such as `to_map` or `try_collect`, if `std` adds any, are std-only
(CO3).

**Key-function helpers** (Chaining Study CS6,
decided 2026-09-29, std-only): `std` adds helpers that take a key function
instead of a comparator or a mapped value, as Rust's `sort_by_key` and
Kotlin's `sumOf` do. Draft signatures:

```text
impl[T] List[T]:
    pub fn sorted_by_key[K < Ord](self, key: fn(T) -> K) -> List[T]:
        pass

impl[T] Iterator[T]:
    pub fn sum_by[N < Num](mut self, value: fn(T) -> N) -> N:
        pass
```

A caller writes `people.sorted_by_key(fn(p): p.age)` and
`orders.iter().sum_by(fn(o): o.total)`. Which other key-function helpers
ship is std design, not a language rule.

`filter`, `take`, `enumerate`, and `map` are lazy: the returned iterator
advances `self` only from its own `next`. `collect` and `fold` drain
`self`, and a negative `take` count panics. A caller writes
`items.iter().filter(keep).collect()`. `for` uses the prelude trait
`Iterable[T]`, which `List` and `Map` implement, and takes a mutable
`Iterator` directly (batch 24, IT2). User code
builds an `Iterator` from its own closure with `Iterator::from_fn`
([Iteration Protocols](../spec/06-control-flow.md#iteration-protocols)).

A lazy adapter calls its callback from `next`, whose row is empty, so the
callback takes no requirement; one that needs a provider captures the value
from `$.use` explicitly, since an enclosing `$.with` never satisfies it
([Owner Decisions](#owner-decisions),
[Lexical And Dynamic Providers](../spec/11-requirements-and-suspension.md#lexical-and-dynamic-providers)). `fold` calls its callback
before it returns, so it carries the row `R`. Iterator adapters that call
suspending code are not provided: a lazy adapter's `next` is not a driver
context. A comprehension in a suspending body may make bang calls, since it
runs eagerly where it is written (Syntax And Semantics Cost Q2, batch 26).

### `std.cmp`, `std.hash`, `std.format`

These keep their fixed traits and add small helpers: `min`, `max`, `clamp`,
and `Reverse[T]` in `std.cmp`, where `clamp(value, low, high)` with `low`
greater than `high` panics (question 18); a default `SipHasher`-style hasher in
`std.hash`, seeded per code identity and runtime profile, plus an
explicitly chosen keyed hasher for hash-flooding defense
([Durable Replay decision 8](RUNTIME_AND_LIBRARY.md#replay-rules)); padding, radix, and precision formatting in `std.format`. None is
blocked; none needs a question.

### `std.ops`

`std.ops` holds what gives library types literal and, later, operator
syntax. Its first member is the literal-suffix marker
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes) L11,
[Decorators](../spec/14-annotations.md#prefix-decorators) D9):

```text
use std.annotation.annotate

@annotate(.Fn)
pub data NumSuffix: pass

pub fn num_suffix() -> NumSuffix:
    NumSuffix {}
```

A library declares a suffix by marking a function `@num_suffix`; `250ms`
then means `ms(250)`. String prefixes work the same way with `@str_prefix`
([Literal Suffixes](../spec/05-expressions.md#literal-suffixes) L19): `sql"a $x"`
calls `sql` with a `Template[T]` of the raw text pieces and the values.
`std.ops` declares `StrPrefix`, `str_prefix`, and `Template`. The
raw-text prefix `r` lives in `std.text`
(L20, [Prefixed Strings](../spec/05-expressions.md#prefixed-strings)), so
code writes `use std.text.r`. The helpers `interpolate`, `process_escapes`,
and `EscapeError` moved there too (L22; see [`std.text`](#stdtext)).

`std.ops` also declares the operator traits
([Operator Traits](../spec/05-expressions.md#operator-traits) OP1-OP9, OP11): `Add`,
`Sub`, `Mul`, `Div`, `Rem`, `Neg`, `BitAnd`, `BitOr`, `BitXor`, `Not`,
`Shl`, and `Shr`, each with an associated `Out`, and `Index` and
`IndexSet`; OP13 removed the assign traits. `List`, `Map`, and `string`
implement `Index`, and `List` and `Map` implement `IndexSet`, with
intrinsic bodies
([Built-In Implementations](../spec/05-expressions.md#built-in-implementations),
Strings STR8). Neither `lib/std` nor the
prototype provides them yet. Std implements the operator
traits for the primitive numbers with intrinsic bodies and `Add` for
`string`, and `std.time` can implement them for `Duration`, so `5s + 250ms` works. `std.time` is not yet written against
them; that is library work, not a language question. Each binary
operator trait defaults `Rhs = Self`
(Type-Argument Defaults TD10,
[`expr.op.trait.shape-default`](../spec/05-expressions.md#r-expr.op.trait.shape-default)), so
`impl Add for Money` means `Add[Money]`. `lib/std/ops.hd` and
`lib/std/num.hd` still spell the argument out; that is library work too.

```text
pub trait Add[Rhs = Self]:
    type Out
    fn add(self, rhs: Rhs) -> Self::Out

pub trait Index[K]:
    type Out
    fn index(self, key: K) -> Self::Out

pub trait IndexSet[K, V]:
    fn index_set(mut self, key: K, value: V) -> void
```

### `Clone`

`Clone` is a standard-library trait
([Typed Derivation M22 and M23](../spec/14-annotations.md#typed-derivation)). It
has two methods: `clone(self)` copies from a readonly value through the
readonly views, and `clone_mut(mut self) -> mut Self` reads the declared
member types, so a derived `clone_mut` clones `mut` members as `mut`. It is
derived through its `by Structure` template with a source that reads the old
value, as the specification's
[`CopySource`](../spec/14-annotations.md#handles) example shows. Its module
is not yet chosen; Typed Derivation M24 chooses it together with the rest of
the standard library.

### Derived Function Cache

A facet is an ordinary trait with an associated function, such as
`trait Validate: fn validator() -> Validator`, derived through a template
([Typed Derivation decision 10](../spec/14-annotations.md#typed-derivation)). One
standard-library cache memoizes derived associated functions: each value is
built lazily, once per (trait, type) per program instance. A `Ref[T]`
deferred reference, with cycle detection, lets a recursive type's value refer
to itself; it replaces the removed `AnnotationRef[T]`. The cache's API and
module are not yet designed; Typed Derivation M24 chooses them together
with the rest of the standard library.

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
`mut self`, which makes the trait a mutable requirement trait; callers write
`$ K` and get `mut K` from `$.use(K)`; a test installs a mutable value with
`$.with(K=value)` and may keep its own `mut` alias to inspect the state
afterwards
([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)).

A host provider for a trait whose methods take `mut self` (`Clock.sleep!`,
`Random`, `FsWrite` below) is bound with mutable access because of those
methods, and an entry point requires it as a plain `$ K`
([Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers)).

### `std.time`

```text
use std.ops.num_suffix

pub data Duration:
    millis: i64

@num_suffix
pub fn ms(count: i64) -> Duration:
    Duration::milliseconds(count)

@num_suffix
pub fn s(count: i64) -> Duration:
    Duration::seconds(count)

@num_suffix
pub fn min(count: i64) -> Duration:
    Duration::seconds(count * 60)

@num_suffix
pub fn h(count: i64) -> Duration:
    Duration::seconds(count * 3_600)

impl Duration:
    pub fn milliseconds(count: i64) -> Duration:
        pass

    pub fn seconds(count: i64) -> Duration:
        pass

    pub fn as_milliseconds(self) -> i64:
        pass

pub data Timestamp:                    # whole milliseconds, like Duration (Literal Suffixes L18)
    unix_millis: i64

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

The suffix newtypes let code write `250ms` or `5s` for a `Duration`, each
imported by name, as in `use std.time.{ms, s}`
([Duration Suffixes](../spec/std/time.md#duration-suffixes)). There is no
`m`, which could mean meters, and no `d`, since a day is not always 24
hours (L9). A `Duration` is a whole number of milliseconds in an `i64`, so
there is no `ns` or `us` until a finer representation exists (L17,
[`std-time.suffix.std.duration`](../spec/std/time.md#r-std-time.suffix.std.duration)).

Reading the clock is a plain call (decision 2); only `sleep!` suspends. A
test installs a manual clock with mutable access, so `sleep!` can advance it:

```text
use std.time.{Clock, Duration, ManualClock, Timestamp, s}

fn pause!(step: Duration) -> void $ Clock:
    $.use(Clock).sleep!(step)

fn simulate!() -> Timestamp:
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    $.with(Clock=clock):
        pause!(5s)
    clock.now()
```

`Timestamp` is UTC wall time; `Instant` is monotonic and meaningful only
against the clock that produced it. Both implement `Eq` and `Ord`, as
Rust's `SystemTime` and `Instant` do, so timestamps sort. Calendar dates, time zones, and
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

pub fn rng() -> mut Rng $ Random:
    Rng::from_seed($.use(Random).next_u64())

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
    fn write_line!(mut self, text: string) -> Result[void, ConsoleError]:
        pass
```

A test installs the buffer with `$.with(Console=console)` and reads
`console.output()` through its own `mut` alias. The prelude
[`Console.write_line!`](../spec/10-modules.md#console) takes `mut self`, so
the buffer appends through it, and printing code still writes only
`$ Console` ([Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers)).

### `std.fs`

The traits are a read/write split (decision 3). Writes take `mut self`, so
`$.use(FsWrite)` is mutable and `MemoryFs` stores what it is given.
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
would need `send!` to take `mut self`, which would make `Http` a mutable
requirement trait, so every installed provider would need a `mut` value; the
sketch keeps `Http` readonly until a test needs the record.
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

impl ScriptedProcess:
    pub fn new(outputs: Map[string, Output]) -> ScriptedProcess:
        pass

impl Process for ScriptedProcess:
    fn run!(self, command: Command) -> Result[Output, ProcessError]:
        pass
```

`ScriptedProcess::new(outputs)` is the only way to build one, since its
field is private (question 22). `run!` answers from `outputs` by
`command.program`, and a program with no entry is
`.Err(ProcessError.NotFound(program))`:

```text
use std.process.{Output, ScriptedProcess}

fn fake_run() -> ScriptedProcess:
    ScriptedProcess::new({"make": Output { status: 0, stdout: [], stderr: [] }})
```

`std.process` also declares `ExitCode` and `Termination`, which turn the
result of `main` or a test into an exit code
([Exit Status](../spec/10-modules.md#exit-status),
[Testing T8](../spec/10-modules.md#exit-status)). Unlike `Process`, neither needs
a host binding:

```text
pub type ExitCode(u8)                  # 0 is success

pub trait Termination:
    fn report(self) -> ExitCode

impl Termination for void              # ExitCode(0)
impl Termination for ExitCode          # itself
impl[T < Termination, E < Display] Termination for Result[T, E]
```

`.Ok(value)` reports `value.report()`; `.Err(error)` prints the error and
its cause chain and reports `ExitCode(1)`. A tool that wants another code
returns `ExitCode` or `Result[ExitCode, E]`. This replaces the earlier
`ExitStatus` trait and its never-zero `StatusCode`
([Exit Status](../spec/10-modules.md#exit-status)).

### `std.observe` and `std.log`

These take the draft in
[Observability](RUNTIME_AND_LIBRARY.md#observability) as written:
`Observability` with `sample` and `emit`, the `Observation` enum, and
`log.info`, `log.warn`, `log.error` helpers with a `$ Observability` row.
`RecordingObservability` is the deterministic provider. It records through
`mut self` methods like the other stateful providers, so `Observability` is
a mutable requirement trait and its host provider is bound mutable without
any profile marking.

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

pub fn race![T](tasks...: List[mut Suspend[T]]) -> T:
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

`timeout!` and `retry!` sleep, so they require `Clock`, a mutable requirement
trait. A test installs a `ManualClock` with `$.with(Clock=clock)`, and the
retry schedule becomes
deterministic. `retry!` takes a constructor, not
a suspension, because a `Suspend[T]` runs once. `race!` takes a homogeneous
list; a heterogeneous race returns an enum the caller defines.

**Superseded for `retry!` (owner decision RETRY, batch 29, 2026-09-30).**
`retry!` is a plain library loop over a `fn!` attempt, with no policy and
no clock: `retry![T, E](times: i32, attempt: fn!() -> Result[T, E])`. The
stdlib tier specifies it in [Task](../spec/std/task.md#retry). The
`RetryPolicy` draft above is not decided.

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

impl Eq for Number:
    fn eq(self, other: Number) -> bool:
        pass

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
   call `T::decode`; this is specified in
   [Associated Function Calls](../spec/09-traits.md#associated-function-calls).
5. **Complete shape coverage** for every legal field type, now specified in
   [Common Shape Representation](../spec/14-annotations.md#common-shape-representation).
6. **Field metadata** for renames, defaults, and skipping; member metadata
   and typed-derivation facts already provide this.
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

It mirrors `Hash` and `Hasher`, but the output is stable across code
identities, runtime profiles, and processes, and carries its algorithm and
version. Deriving `Fingerprintable` needs a
derivation rule, which falls under the same typed-derivation issue.

## Testing Layer

`std.testing` keeps `assert` and `assert_equal` with their mandatory reason,
declares the test-case functions, and adds four groups.

The test-case functions are specified in
[Test Cases](../spec/10-modules.md#test-cases), from
[Testing](../spec/10-modules.md#test-modules) T16, T31, T40, T41, and T47. All four
are ordinary functions; only their registration is special. Each is called
only directly in test position, with a literal name and `it`'s literal
options.

| Name | What it is | How code reaches it | Tier |
| --- | --- | --- | --- |
| `it` | One test case; the body is a trailing block or `body=`. | The prelude supplies it, so it cannot be shadowed. | language; its `timeout` option: std |
| `it_each` | One test case per row, named `name[i]`; the body is `body=fn!(row: A): ...`. | `use std.testing.it_each` | language: registration; std: rows, names, run timing |
| `it_prop`, `it_prop_with` | One property test case; the body is `prop=fn!(value: T): ...`. | `use std.testing.{it_prop, it_prop_with}` | language: registration; std: generation, shrinking, draw budget |

```text
use std.process.Termination

pub fn it[T < Termination, R](name: string, ignore: string? = .None, expect_panic: string? = .None,
                             timeout: Duration? = .None, body: fn!() -> T $ R) -> void $ R:
    pass

pub fn it_each[A, T < Termination, R](name: string, rows: List[A], ignore: string? = .None,
                                      expect_panic: string? = .None, timeout: Duration? = .None,
                                      body: fn!(A) -> T $ R) -> void $ R:
    pass
```

`timeout` takes any `Duration` value, usually a suffixed literal, as in
`timeout=5s` with `use std.time.s`
([`std-testing.option.timeout-any-duration`](../spec/std/testing.md#r-std-testing.option.timeout-any-duration)).

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

pub fn hermetic(seed: u64 = 0) -> $.Context[$ Clock + Random + Env + FsRead + FsWrite + Console]:
    let clock: mut ManualClock = ManualClock::starting_at(Timestamp::from_unix_seconds(0))
    let random: mut SeededRandom = SeededRandom::new(seed)
    let files: mut MemoryFs = MemoryFs::new()
    let console: mut BufferConsole = BufferConsole::new()
    $.context(
        Clock=clock,
        Random=random,
        Env=MapEnv { values: {} },
        FsRead=files,
        FsWrite=files,
        Console=console,
    )

tests:
    it("a hermetic run can override one provider"):
        $.with(hermetic(seed=7)..., Env=MapEnv { values: {"MODE": "ci"} }):
            pass
```

Later entries win, so a test replaces one provider after the spread. The
stateful providers enter the context with `mut` access, so code under test
can advance the clock, step the generator, and write files.

Snapshot functions compare a string with expected text (T30, T32). Their
signatures are specified in [Snapshots](../spec/10-modules.md#snapshots)
and [Snapshot Files](../spec/std/testing.md#snapshot-files) (T49), and `expect` must be a string literal:

```text
pub fn snapshot(text: string, expect: string = "") -> void:
    pass

pub fn snapshot_file(text: string) -> void:
    pass
```

The caller renders the value, for example with `json.pretty(x)`,
`yaml.encode(x)`, or `debug(x)`. `hd test --update` rewrites the `expect`
literal in the source. `snapshot_file` takes no name: the runner names its
file from the running test, under the package's one `__snapshots__/` folder
(T34), as [Snapshot Tests](RUNTIME_AND_LIBRARY.md#snapshot-tests) lays out.

`debug(x)` renders the derivable `Debug` trait (T33, T39, T48), specified in
[Debug Trait](../spec/09-traits.md#debug-trait). `std.format` declares it
beside `Display`, and both `Debug` and `debug` are prelude names:

```text
pub trait Debug:
    fn debug(self, out: mut DebugWriter) -> void
```

`DebugWriter` is a structured writer with builder calls, like Rust's
`debug_struct` and `field`, as [Debug Builders](../spec/std/format.md#debug-builders) specifies (Testing T53). The
derived implementation is a walker over the members, and `debug(x) -> string`
prints stable, multi-line, consistently indented output. `std` implements
`Debug` for primitives, collections, `T?`, `Result`, and tuples.
`assert_equal` requires `T < Eq & Debug`, so a failure shows both values.

Property testing stays a library facility (T12), as
[Runtime and Library Design](RUNTIME_AND_LIBRARY.md#property-testing)
describes. `@derive(Arbitrary)` derives a `build` over a recording
`std.testing.Choices` source (T35), and one member fact,
`arbitrary.with(gen)`, tunes a member (PT2). Other constraints use a plain
generator `fn(mut Choices) -> T`. The runner shrinks
by replaying smaller choice streams through the same generator, so no type
needs shrink code (T25, T35). Properties register with `it_prop` and
`it_prop_with` (T36); failing streams are committed under
`__regressions__/` (T37); the budget is 100 cases (T38), and shrinking
stops after 500 attempts, reporting the smallest failing input so far,
marked "shrinking stopped early" (T51). A property body returns `void`, or
`Result[void, Error]` when it uses `?`, and an `.Err` is a failing case
shrunk like an assertion failure (T50). The signatures, with `it`'s options
(T41) in the order the specification uses for `it_each`, and `examples`
(PT7) before `prop`:

```text
pub fn it_prop[T < Arbitrary & Debug, R < Termination](name: string, ignore: string? = .None,
                                                       expect_panic: string? = .None, timeout: Duration? = .None,
                                                       cases: i32 = 100, shrink: i32 = 500, examples: List[T] = [],
                                                       prop: fn!(T) -> R) -> void:
    pass

pub fn it_prop_with[T < Debug, R < Termination](name: string, gen: fn(mut Choices) -> T, ignore: string? = .None,
                                                expect_panic: string? = .None, timeout: Duration? = .None,
                                                cases: i32 = 100, shrink: i32 = 500, examples: List[T] = [],
                                                prop: fn!(T) -> R) -> void:
    pass
```

#### Proposal: choices first, `Arbitrary` for defaults

Draft, 2026-09-28 (owner: "choices, Hypothesis first, then Arbitrary to
give sensible defaults"), revised by Testing PT1-PT9 (2026-09-29). The
specification states the language-facing parts in
[Property Tests](../spec/std/testing.md#property-tests); names below that it
does not list may change when the library is built.

**`Choices`** is the only source of randomness a generator sees. Every draw
is recorded as a number in one stream, and spans mark where each logical
value (a list element, a member, a variant payload) starts and ends, so the
shrinker can delete whole values. There is no size (PT1): as in
Hypothesis, draws lean toward small values and edges, and any small-first
order of cases is the runner's own business.

```text
pub data Choices                               # opaque; created by the runner
impl Choices:
    pub fn int[N < Integer](mut self, lo: N, hi: N) -> N      # biased toward lo, 0, -1, hi
    pub fn float[F < Float](mut self, lo: F, hi: F) -> F      # finite; also 0.0, -0.0, bounds
    pub fn bool(mut self) -> bool
    pub fn pick[T](mut self, items: List[T]) -> T           # earlier items shrink first
    pub fn list[T](mut self, max: i32, item: fn(mut Choices) -> T) -> List[T]
    pub fn map[K < Eq & Hash, V](mut self, max: i32, key: fn(mut Choices) -> K,
                                 value: fn(mut Choices) -> V) -> Map[K, V]
    pub fn string(mut self, max_chars: i32) -> string       # counts chars, not bytes
    pub fn assume(mut self, ok: bool) -> void               # discards this case
    pub fn draw[T < Arbitrary](mut self) -> T               # the type's default
```

`int` and `float` take their type from the bounds or the context (PT3):
`let b: u8 = c.int(0, 255)`. `map` draws up to `max` entries, and a
duplicate key keeps the last value, so the map may be smaller (PT5).
`string(max_chars=12)` counts chars, and which chars it draws is the
runner's choice (PT4).

`list`'s item generator takes its own `Choices` (PT1). It is the same `c`,
passed back, for two reasons. `list` brackets each element's draws as one
span, so the shrinker can delete or simplify an element whole. And the
closure does not capture the outer `mut c` while `list` is using it. A
named generator needs no closure at all:

```text
fn digit(c: mut Choices) -> i32:
    c.int(0, 9)

fn digits(c: mut Choices) -> List[i32]:
    c.list(50, digit)

fn small_counts(c: mut Choices) -> List[i32]:
    c.list(3, fn(inner: mut Choices) -> i32: inner.int(0, 10))
```

**The draw budget** ends recursive generation, with no API (PT6). Each case
has a budget of draws; once it is spent, every draw returns its simplest
value: an integer `0` or the bound nearest `0`, a float likewise, `false`,
`pick`'s first item, and an empty list, map, or string. The default `T?`
generator gives `.None`, `Result[T, E]` gives `.Ok` of `T`'s simplest
value, and a tuple gives each element's simplest value (Q8). So a hand-written
recursive generator ends when its leaf comes first, as in
`match c.int(0, 5)` with the leaf at `0`. The budget's size is the
runner's.

**`assume`** belongs to generators only (PT8). A property body has no
`Choices`, so it cannot discard a case.

**`Arbitrary`** gives each type a sensible default generator:

```text
pub trait Arbitrary:
    fn arbitrary(c: mut Choices) -> Self
```

`std` implements it for primitives (boundary values drawn more often),
`string`, `List[T]` and `Map[K, V]` (a length, then elements), `T?`
(`.None` or `.Some`), `Result[T, E]`, and tuples. The `f32` and `f64`
defaults cover the whole type, NaN, the infinities, `-0.0`, and subnormals
included, as Hypothesis's `st.floats()` does (PT9); `c.float(lo, hi)` stays
finite. `@derive(Arbitrary)` is a derived `build` whose source is
`Choices`: each member is drawn by its own `Arbitrary`, and an enum picks a
variant, then its payload. The template is ordinary `std.testing` code: it
reads the compiler-computed `self_ref` of each variant and member from
`std.structure` (SR1). A derived enum's simplest choice is its first
variant whose `self_ref` is not `.Required`, whatever the declaration order
(PT6). A variant is `.Required` when its simplest payload still needs a
value of the enum; `List`, `Map`, and optional members are at most
`.Optional` (Q7). When every variant is `.Required`, or a data type has a
`.Required` member, the derived generator panics on the first case, naming
the type (Q6, SR1). See
[Self References](../spec/14-annotations.md#self-references).

> **Note.** Other templates can read `self_ref` too (SR1). A derived
> `Default` picks its simplest variant the same way. When any `self_ref`
> is not `.Absent`, a codec such as `std.json` adds a nesting-depth limit,
> and a schema generator emits a named definition with a `$ref`. `Debug`
> may truncate deep output.

One fact tunes a member: `arbitrary.with(gen)`, from the module
`std.testing.arbitrary`, draws that member with `gen` (PT2). There are no
range or length facts, because a generator already expresses any range,
length, or shape:

```text
use std.structure.Structure
use std.testing.{Arbitrary, Choices}
use std.testing.arbitrary

fn cents(c: mut Choices) -> i32:
    c.int(0, 10_000)

fn short_name(c: mut Choices) -> string:
    c.string(max_chars=12)

@derive(Debug)
data Item:
    name: string
    price: i32

impl Arbitrary for Item by Structure:
    price = [arbitrary.with(cents)]
    name = [arbitrary.with(short_name)]
```

`with` is generic, `pub fn with[T < Inspectable](gen: fn(mut Choices) -> T) -> Generator`
(Q5). It wraps `gen` so that each drawn value is erased to `Inspectable`,
and the derived code downcasts the first drawn value to the member's type.
Every member's type must be inspectable and implement `Arbitrary`, tuned
or not; a type whose members cannot is written by hand (Testing AT-with). A fact is an unchecked value, so
the compiler does not catch a generator of the wrong type. The derived
code panics on the first case instead, naming the member and both types;
it never ignores the fact. A generic `With[T]` fact was rejected, because
looking up `With[i32]` would miss a `With[string]` and silently use the
default. See
[Derived Arbitrary](../spec/std/testing.md#derived-arbitrary).

**A generator** for anything a type cannot express is a plain function
over `Choices`, passed to `it_prop_with`. Dependent draws need nothing
extra: a later draw reads an earlier one, as Hypothesis's `@composite`
does. Several inputs are one tuple:

```text
fn ordered_pair(c: mut Choices) -> (i32, i32):
    low := c.int(0, 100)
    high := c.int(low, 100)
    (low, high)

tests:
    it_prop("total is never negative", prop=fn!(order: Order):
        assert(total(order) >= 0, reason="total")
    )
    it_prop_with("a range holds its low end", gen=ordered_pair, prop=fn!(pair: (i32, i32)):
        let (low, high) = pair
        assert(clamp(low, low, high) == low, reason="the low end is in range")
    )
```

**A worked example: a JSON round trip.** Printing a value and parsing it
back must give the value back, the first property Zac Hatfield-Dodds
suggests in ["Sufficiently Advanced Testing"](https://zhd.dev/sufficiently/).
The generator puts the leaf first, so the draw budget ends the recursion:

```text
use std.json
use std.json.{Json, Number}
use std.testing.{assert_equal, Choices, it_prop_with}

fn key(c: mut Choices) -> string:
    c.string(max_chars=8)

fn json_value(c: mut Choices) -> Json:
    match c.int(0, 5):
        0 => .Null
        1 => .Bool(c.bool())
        2 => .Number(Number::from_i64(c.draw::[i64]()))
        3 => .Text(c.string(max_chars=12))
        4 => .Array(c.list(4, json_value))
        _ => .Object(c.map(4, key, json_value))

tests:
    it_prop_with("print then parse gives the value back", gen=json_value, prop=fn!(value: Json):
        match json.parse(json.print(value)):
            .Ok(back) => assert_equal(back, value, reason="JSON round trip")
            .Err(error) => panic("printed JSON does not parse: ${error.message}")
    )
```

This assumes that `Json` implements `Eq` and `Debug`, which
[`std.json`](#stdjson) does not list yet. Integers are drawn as `i64`,
because `Number::from_f64` rejects NaN and the infinities.

**The runner** owns generation, shrinking, and replay:

1. Run the property's `examples` first (PT7).
2. Replay the committed streams under
   `__regressions__/<module>/<test-slug>`.
3. Run `cases` fresh instances (default 100) from a printed seed.
4. On a failure (assertion panic, other panic, or `.Err`), shrink the
   recorded stream. Each candidate is replayed through the same generator
   in a fresh instance; a stream the generator rejects is discarded, and a
   stream already tried is skipped. Passes, repeated until none helps:
   delete spans (whole values), zero spans, lower each number by binary
   search, sort and swap neighbours toward shortlex order, and
   redistribute between pairs of numbers. Stop after `shrink` attempts
   (default 500), marking the result "shrinking stopped early".
5. Report the shrunk value with `Debug`, the seed, and the saved path, and
   write the shrunk stream to `__regressions__/`.

Because shrinking edits choices rather than values, every shrunk input is
one the generator can produce, so constraints hold and `map`-like or
dependent generation needs no extra code. Compared with QuickCheck
(per-type `shrink`, breaks constraints) and proptest or fast-check (value
trees, a combinator API), this keeps the user API to `Choices`,
`Arbitrary`, and two registration calls, and puts the complexity in one
runner. Coverage-guided fuzzing can later mutate the same streams, and
stateful testing waits for area 3's event log.

The owner decided the rest on 2026-09-28 (Open Issues items 2, 6 and 7):
discarded cases do not count toward `cases` and fail the property beyond
10 × `cases`, `T < Debug`, and the `__regressions__` file holds one
decimal draw per line. This replaces the earlier `Strategy` sketch with
its own `shrink` function. Testing PT1-PT9 (2026-09-29) settled the size
model, the facts, and the draw API above. The generators keep the
`mut Choices` parameter style; a requirement-row style
(`fn() -> T $ Choices`) is being compared, as
[Testing Still Open](TESTING.md#still-open) records.

## Open Language Dependencies

| Module | Depends on | Open item |
| --- | --- | --- |
| host providers for `Clock.sleep!`, `Random`, `FsWrite`; a recording `Console` | `write_line!` taking `mut self`: applied, [Console](../spec/10-modules.md#console) | none |
| `std.time`, `std.random`, `std.host` | replay recording of non-suspending host calls (decision 2): answered, every host method is marked input or output by its runtime profile, suspending or not ([Durable Replay decision 7](RUNTIME_AND_LIBRARY.md#replay-rules)) | none |
| inherent methods on `string`, `T?`, `List`, `Map`, integers (decision 8) | a `std` exception to the inherent-target rule: applied, [`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std) | none |
| `std.json` typed codecs, `std.fingerprint` derive, property generators | typed derivation protocol | [Typed Derivation](OPEN_ISSUES.md#typed-derivation-tool-adapters-and-secrets) |
| `std.fs` handles, `std.net`, `std.process` streaming | non-escaping handles and fallible cleanup | [Resource Non-Escape](OPEN_ISSUES.md#resource-non-escape-and-cleanup-policy) |
| `std.observe`, `std.log` | task-local trace context | [Observability Hooks](OPEN_ISSUES.md#observability-hooks) |
| `std.incremental` | closure identity, weak references | [Serializable Closures](OPEN_ISSUES.md#serializable-closures-and-incremental-computation) |
| `std.task.all!` | none: an intrinsic with one typing rule since batch 31b removed packs | none |
| idle-driven virtual time (after decision 10) | a driver idle signal | no open issue yet |
| attenuated providers (`for_tenant`) | principal and tenancy patterns | [Access Control](OPEN_ISSUES.md#access-control-and-tenancy-expressibility) |
| capability catalog, provider configuration, combinator set | library and runtime work | [Runtime, Library, ABI, And Tooling Work](OPEN_ISSUES.md#runtime-library-abi-and-tooling-work) |
| how `std` versions with the compiler | package tooling | [Roadmap area 5](ROADMAP.md#5-packages) |

## Owner Decisions

**STDLIB Still Open after the question 14-22 pass (decided 2026-09-29).**
- `map[U]` and `fold[A]` on `Iterator`: closed by Chaining Study CS7,
  which makes `Iterator` a data type, so both are ordinary methods
  ([Iterator Adapters](../spec/std/iter.md#iterator-adapters)).
- A callback passed to a lazy adapter (`filter`) runs inside `next`, whose
  row is empty. It needs no providers; one that uses a provider captures
  the value from `$.use` explicitly, since a closure never captures a
  provider from an enclosing `$.with` (provider scope batch 14, PS3,
  [`std-iter.adapter.callback-row.capture`](../spec/std/iter.md#r-std-iter.adapter.callback-row.capture)). When the
  eager `fold` lands, it carries a row parameter:
  `fold[A, R](init: A, step: fn(A, T) -> A $ R) -> A $ R`.
- Kept as applied:
  - `take(-1)` panics;
  - `Map.keys()` and `values()` return list snapshots;
  - `abs_diff` is an inherent method on each integer type;
  - "value types" that implement `Eq` are data and enums holding only
    values, not providers, builders or handles.
- `Timestamp` and `Instant` implement `Ord`.

Applied 2026-09-29, in this record only. The specification's `Iterator`
text names no `fold`, so the row parameter is in the draft signature under
[`std.iter`](#stditer). `Ord` for `Timestamp` and `Instant` is `std` scope
(AGENTS.md, Spec Scope For The Standard Library), so it is in
[`std.time`](#stdtime) only; `lib/std` has no `Timestamp` or `Instant` yet.

**Questions 14-22 (decided 2026-09-29, all as recommended).**
- Iterator adapters are default methods on the prelude `Iterator`, as in
  Rust (`map`, `filter`, `take`, `enumerate`, `fold`, `collect`). There is
  no separate `IteratorExt`.
- `and_then` is added on `T?` and `Result`.
- `Map` gets `contains_key`, `keys` and `values`.
- `lines()` follows Rust: a final `\n` ends the last line, and `\r\n`
  is stripped.
- Out-of-range counts panic: `repeat(-1)`, `chunks(0)`, and `clamp` with
  low > high.
- `abs_diff` returns the unsigned type of the same width.
- Integer parsing takes an optional `+` or `-`, then decimal digits only;
  a lone sign is `InvalidDigit(0)`.
- Std value types implement `Eq` everywhere, `Ord` for `Duration`, and
  `Display` for each error enum.
- `ScriptedProcess::new(outputs)` is the constructor.

Applied 2026-09-29. The prelude `Iterator` part is in the specification:
[Iterator Adapters](../spec/std/iter.md#iterator-adapters) gives
`filter`, `take`, `enumerate`, and `collect`. Superseded 2026-09-29 by
Chaining Study CS7 and CS8:
`Iterator` is a data type, so `map` and `fold` are ordinary methods and
are in the specification too. The rest is `std`-only and is in the draft sections above:
[`std.iter`](#stditer), [`std.option` and `std.result`](#stdoption-and-stdresult),
[`std.collections`](#stdcollections), [`std.text`](#stdtext),
[`std.num`](#stdnum), [`std.process`](#stdprocess), and design principles 7
and 10.

Decided 2026-09-26:

1. **Question 1: `$.use` can return `mut`.** A provider installed with
   `mut` access may be used as `$.use(mut Clock)`, so a deterministic
   provider such as `ManualClock` changes its own state through ordinary
   `mut self` methods. Applied 2026-09-26: the access rules are in
   [Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers).
   A provider is installed with `$.with(mut Clock=clock)`, requested in rows
   as `$ mut Clock`, and retrieved with `$.use(mut Clock)`. Superseded
   2026-09-27: access now follows from the trait, and no `mut` is written
   ([Mutable Host Providers](OPEN_ISSUES.md#mutable-host-providers)).

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
   for them, and those methods are available without a `use`. Applied
   2026-09-27:
   [`trait.own.inherent.std`](../spec/09-traits.md#r-trait.own.inherent.std).
   Detail decided 2026-09-27: the list includes `Result` as well as
   `Option`. Tuples get no helper methods at all, only trait
   implementations; a tuple that needs methods should be a named `data`
   ([`trait.own.inherent.std.no-tuple`](../spec/09-traits.md#r-trait.own.inherent.std.no-tuple)).
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
    Superseded 2026-09-27: profiles no longer mark traits; a trait with a
    `mut self` method is always bound mutable.

## Still Open

None. The seven points the question 14-22 apply pass met were decided on
2026-09-29 and are summarized at the top of
[Owner Decisions](#owner-decisions).

## Questions For The Owner

Questions 1 to 22 are decided: 1 to 13 on 2026-09-26, and 14 to 22 on
2026-09-29, all as recommended. [Owner Decisions](#owner-decisions)
summarizes each, and the draft sections above apply them. The options
weighed for each question are in git history.
