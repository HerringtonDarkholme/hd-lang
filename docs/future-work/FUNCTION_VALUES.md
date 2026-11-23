# Function Values: Opt-In Closure Types And A Capture-Free `FnPtr`

Proposal, 2026-11-21, revised 2026-11-23 after the owner's feedback;
domain: language (function types, closures), std, backend ABI; not
accepted. Issues #1436 (Route A) and #1449 on top.

## The proposal

`fn(..)` keeps today's meaning: a function value that may capture. Two
faster forms are opt-in, with no new syntax:

- **A type per closure**, through `F < Fn/FnMut(..)` or an `impl FnMut(..)`
  parameter: a closure literal or function name there gets its own type,
  called directly, captures held by value, no environment object.
- **A capture-free function**, `FnPtr[(A,), O]`: one word, never counted.

```text
use std.function.{FnMut, FnPtr}

struct Handlers:
    on_event: List[fn(i32) -> void] # unchanged: may capture, erased

struct Keep[F < FnMut(i32) -> bool]:
    test: F # opt-in: one type per closure

struct Collector:
    trace: FnPtr[(usize,), void] # opt-in: captures nothing, one word

fn demo(xs: List[i32], floor: i32) -> usize:
    xs.filter(fn(x): *x > floor).len() # filter's F: this closure's own type
```

## Open questions

1. Type per closure spelled by the existing `F < FnMut(..)` and
   `impl FnMut(..)`, with no new syntax (B1 and B2 below)?
   **Recommendation:** yes.
2. Capture-free form spelled `FnPtr[(A,), O]`, a std constructor with no
   sugar (A1)? **Recommendation:** yes; add `thin fn(..)` sugar (A2)
   only if user code needs it.
3. A function name at those positions also gets its own zero-size type,
   as a Rust fn item does? **Recommendation:** yes, the same rule.
4. A closure type is `Copy` only when its captures are, and the 50 std
   bounds without `?Copy` gain it? **Recommendation:** yes (Rust parity).
5. Delete `hd_opt`'s literal-consumer copies, so `fn(..)` parameters
   are no longer specialized? **Recommendation:** yes, by the decision.
6. `sorted_by` keeps `F` and costs 4.9 KB per distinct comparator?
   **Recommendation:** yes, as Rust's `sort_by`.
7. Growth-OK for +300 net lines, all from `FnPtr` and #1449?
   **Recommendation:** yes.

## Problems it solves

**1. Iterator chains call through the table.** In the `closures` bench
(`items.iter().map(..).filter(..).fold(..)`), each `next` of the one
`MapIter[.., fn(i32) -> i32]` uses `call_indirect`. Hand-written with a
struct per closure, as closure types give, it takes **126 ms, not 246
(-49%)**, and **6844 bytes, not 7806 (-12%)**; a fused loop, 104 ms.

**2. One `filter` closure costs an allocation.** `xs.filter(fn(x): *x >
floor)` calls `rt:new/class-1` for `floor`'s environment, then the shared
`List[T].filter[i32, fn(&i32) -> bool]`. With a closure type,
`filter[i32, {closure}]` holds `floor` and runs `*x > floor` inline.

**3. A capture-free hook carries an environment.** `rt/cycle`'s `trace`,
`release` and `free` hold runtime helpers with empty environments
(`Helper::Collect` in `crates/hd_wasm/src/rt.rs`). A call loads two words
and passes an environment and a context: 25 bytes. As `FnPtr`: 10.

## Candidate spellings

| Form | Candidate | Example | Parses today |
|---|---|---|---|
| capture-free | **A1** std constructor (Rust's name) | `trace: FnPtr[(usize,), void]` | yes; the checker needs the type |
| capture-free | A2 modifier (Swift's `thin`) | `trace: thin fn(usize) -> void` | no: parser work |
| capture-free | A3 modifier (C++ `static`) | `trace: static fn(usize) -> void` | no: parser work |
| per closure | **B1** generic bound | `struct Keep[F < FnMut(i32) -> bool]` | yes, checks |
| per closure | **B2** `impl` parameter | `mut test: impl FnMut(&i32) -> bool` | yes, checks |
| per closure | B3 Kotlin `inline` callee | `@inline(.Always)` over `test: fn(..)` | yes, checks |

**Recommendation: A1, and B1 with B2.** No parser work; they reuse forms
hd has (`Fn[I, O, $R]` names what `fn(..)` stands for). B1 is already
std's opt-in: 97 bounds, iterator adaptors included. A1's 20 std sites,
all runtime or derived code, are too few for a keyword (A2, A3). B3
cannot live in a struct field (nor can Kotlin's), so `Filter` can't use it.

## How it works

**Closure types.** A closure literal or function name has its own type
at the two positions `fn.trait.read-only.positions` lists: a call
argument for a bare type parameter (an `impl` parameter is one), and a
struct literal field of one. Elsewhere it is `fn(..)`, so `f := fn(x): x +
1` stays `fn(..)`. It implements `Fn` or only `FnMut` by
`fn.trait.read-only.*`, and `Copy` when its captures are. Its value is a
struct of its captures: a copy for an immutable one, the cell's address
for a `let` one, so copies share cells. Calling it calls its body
directly; a function name's type has no fields. No conversion to
`fn(..)` is added: `F` stays opaque in an instance.

**`FnPtr`.** `std.function` declares `FnPtr[I, O, $R]` beside `Fn`.

| From | To | When |
|---|---|---|
| function name, one-field variant, method reference | `FnPtr[..]` | always |
| closure literal | `FnPtr[..]` | it captures nothing; else `closure-captures`, fix-it: `fn(..)` |
| `FnPtr[..]` | `fn(..)` | always: paired with environment 0 |
| `FnPtr[..]` | `F < Fn(..)` | always: it implements `Fn` |

Its value is an `i32` table slot holding the function itself: a call is
`call_indirect (args)`, with a context only when `$R` is not empty.

**#1449 on top.** A closure type has no environment object by
construction. #1449 keeps only captured `let` cells: when the holder (the
closure, or a `Filter` holding it) never leaves its frame by a return, a
`*mut` store or a `box`, the cell becomes a frame slot (`CaptureMode::Frame`).

## Cost

**Lines.** `crates/` is 378,673 lines at 6712c5923. End state about
378,970: +750 added, -450 deleted, net +300. Step 1 (closure types)
deletes the consumer copies in the same commit, about net zero.

| Component | Lines | What |
|---|---:|---|
| **Delete:** literal-consumer copies | **-450** | `hd_opt/src/inline.rs`: `LiteralConsumer`, `consumer_copy`, `literal_consumer`, `make_literal_consumer`, their tests (-330); `hd_tir/src/inline/closures.rs`: `literal_arguments`, `called_parameters`, their tests (-120) |
| Closure types: checker | +200 to +250 | identity at the two positions, `Fn`/`FnMut`, `Copy` |
| Closure types: mono, TIR, Wasm | +200 to +250 | instance key; captures struct; direct call |
| `FnPtr` | +200 to +250 | the type, coercions, `closure-captures`, one-word ABI |
| #1449 | +50 to +100 | frame slots for closure-type cells |

**Runtime and size**, measured at 6712c5923 (release, V8; Appendix D):

| Case | Today | With the proposal |
|---|---|---|
| call through a `fn(i32) -> i32` field | 2 loads, `call_indirect (env, x, ctx)`, 25 bytes | `FnPtr`: 1 load, `call_indirect (x)`, 10 bytes |
| create a capturing closure | `rt:new/class-1` + store | closure type: none (a struct field) |
| one more closure to `filter` | shared instance | +159 bytes (a new instance) |
| one more closure to `.iter().map(..).sum()` | shared instance | +379 bytes |
| one more comparator to `sorted_by` | shared instance | +4873 bytes |
| compile time | one instance per `fn(..)` type | at most one instance per literal per generic: linear, no iteration |
| web compiler gzip | | the net lines, under 1 KB |

**std signatures** (`lib/std`, 6712c5923; 107 `fn(..)` type sites, 99
closure literals, 97 `Fn`/`FnMut` bounds):

| Move | Count | Sites |
|---|---:|---|
| already `F < Fn/FnMut`: becomes per closure, no edit | 97 bounds | `iter` adaptors, `List`/`Map`/`Option` combinators, `defer`, `ensure`, retry |
| add `?Copy` to such a bound (question 4) | 50 | `iter` (26), `task` (7), `ops` (7), `contract` (6), 4 others |
| `fn(..)` to `F < FnMut(..)` | 4 | `Result.map`, `map_err`, `and_then`, `Option.and_then` |
| `fn(..)` to `FnPtr` | about 20 | `rt/cycle` (7), `structure` records (4), derived-order `other_index` (3), `wire_put`/`wire_take` (2), `rt/wait`, `rt/checked` (3), `start_thread` (1) |
| stays `fn(..)` | about 83 | stored callbacks that may capture: `task` layers, `testing` generators, `format` printers, `remote`, `thread` |

---

## A. Stress test

1. **The `closures` bench** (map, filter, fold): unchanged source; each
   closure gets an instance. Measured ceiling -49% time (Problem 1).
2. **Rust's `Iterator::filter(..).map(..)` chains** as `dyn_iter` writes
   them: `Filter[Counter, i64, {c1}]` inside `MapIter[.., {c2}]`, one
   instance per chain, linear in literals. Works.
3. **An event registry** (`addEventListener`): `List[fn(Event) -> void]`
   stays erased, and each listener may capture. Works unchanged; this is
   why `fn(..)` keeps its meaning.
4. **A C `qsort` comparator or a runtime hook table:**
   `List[FnPtr[(usize,), void]]` of named functions. Works; a capturing
   closure there is `closure-captures` with a `fn(..)` fix-it.
5. **Two closures for one `F`**: `f := if c: fn(x): x + 1 else: fn(x): x
   * 2; apply(f)`. `f` is a binding, so it is `fn(..)`, and `F = fn(i32)
   -> i32` gives today's erased instance. Works, with no fast path;
   writing `apply` in each branch restores it.
6. **A capture of a `!Copy` value** (`xs.iter().filter(fn(x): .. image
   ..)` with `impl !Copy for Image`) checks today because `fn(..)` is
   `Copy`. With closure types it fails `Filter`'s implicit `Copy` bound.
   **Found:** question 4 adds `?Copy` to 50 bounds.
7. **Parser combinators** (nom's `tag(..)` returns `impl Fn`): an hd
   combinator returns `fn(..)`, which stays erased. **Found:** closure
   types at opaque results (`-> impl Fn(..)`) are a later extension, not
   in this proposal.
8. **`F` passed on to a `fn(..)` parameter** inside an instance:
   `type-mismatch` today (probe), and still after. The callee goes
   generic, or the caller passes a `fn(..)`.
9. **Code size**: a program with ten different `sorted_by` comparators
   grows by about 44 KB. **Found:** question 6.
10. **Crossing closures** (`fn() -> T + Send`): already literal-only and
    per call; unchanged.

## B. Prior art, with code

Probes in `/tmp/lt255/probes` (not in the repo); each claim below names
its source.

**Rust** (rustc 1.97.1 `-O`, `--emit=mir`; the
[Reference](https://doc.rust-lang.org/reference/types/closure.html): "a
closure value with a unique, anonymous type"). A fn item is 0 bytes, a
`fn` pointer 8, a capture-free closure 0, a closure capturing an `i32`
4, a `Box<dyn FnMut>` 16.

```rust
let item = double;                          // own type, size 0
let ptr: fn(i32) -> i32 = double;           // one word: ReifyFnPointer
let coerced: fn(i32) -> i32 = |x| x + 1;    // MIR: ClosureFnPointer
let bad: fn(i32) -> i32 = move |x| x + k;   // E0308: expected fn pointer, found closure
fn apply_generic<F: FnMut(i32) -> i32>(mut f: F, x: i32) -> i32 { f(x) }
```

hd takes the closure type and the capture-free pointer, both opt-in;
Rust's default spelling is the fast one, hd's is the erased one.

**Swift** (swiftc 6.4). Function values are thick: 16 bytes. The
[reference](https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/ReferenceManual/Attributes.md)
documents `@convention(swift | block | c)` only; `@convention(thin)`
compiles (8 bytes), but a capturing closure there is "INTERNAL ERROR:
feature not implemented". `@escaping` marks a parameter that may be
stored.

```swift
struct Mapper { var transform: (Int32) -> Int32 }               // 16 bytes
struct CMapper { var transform: @convention(c) (Int32) -> Int32 } // 8 bytes
let c: @convention(c) (Int32) -> Int32 = { $0 + k }
// error: a C function pointer cannot be formed from a closure that captures context
```

hd matches Swift's default (thick, may capture) and spells the thin form
as a type, A1, not an attribute. Swift's non-escaping default does not
fit hd, where a `fn(..)` parameter may be stored.

**Kotlin** ([inline functions](https://kotlinlang.org/docs/inline-functions.html)):
`inline` inlines the function and "the lambdas passed to it"; `noinline`
lambdas "can be manipulated in any way you like, including being stored";
`crossinline` forbids non-local returns from a lambda called in another
context.

```kotlin
inline fun <T> lock(lock: Lock, body: () -> T): T { ... }
inline fun foo(inlined: () -> Unit, noinline notInlined: () -> Unit) { ... }
inline fun f(crossinline body: () -> Unit) { val f = object: Runnable { override fun run() = body() } }
```

hd rejects it as the opt-in (B3): an inlined lambda cannot live in a
struct field, and hd's adaptors store theirs.

**Zig** (0.17.1 probe; [Functions](https://ziglang.org/documentation/0.15.2/#Functions):
"function bodies are comptime-only types while function pointers may be
runtime-known"). There are no closures.

```zig
const Mapper = struct { transform: *const fn (i32) i32 };   // 8 bytes
fn applyComptime(comptime f: fn (i32) i32, x: i32) i32 { return f(x); }
// var f: fn (i32) i32 = double;  error: must be const or comptime
```

hd's `FnPtr` is Zig's `*const fn`, and a closure type at `F` is its
comptime body.

**C++** (clang 23 `-std=c++20 -O2`;
[cppreference](https://en.cppreference.com/w/cpp/language/lambda):
"unique unnamed ... class type, known as closure type"; conversion to a
pointer "only if the lambda expression has no captures"). Sizes: pointer
8, capture-free lambda 1, lambda capturing an `int` 4, `std::function` 32.

```cpp
template <class F> int apply_generic(F f, int x) { return f(x); }  // per lambda
int (*p)(int) = [](int x) { return x + 1; };                      // captureless converts
std::function<int(int)> erased = [k](int x) { return x + k; };    // erased, may allocate
// int (*q)(int) = [k](int x) { return x + k; };  error: no viable conversion
```

The closest match: `std::function` is hd's `fn(..)`, a template
parameter is `F`, and a function pointer is `FnPtr`.

## C. Candidate spellings in full

Each block was checked with `hd check` (Parse log, Appendix E).

```text
use std.function.FnPtr

struct Collector:
    trace: FnPtr[(usize,), void]

fn run(c: &Collector, block: usize) -> void:
    (c.trace)(block)
```

```text
struct Collector:
    trace: thin fn(usize) -> void  # hypothetical syntax
```

```text
struct Collector:
    trace: static fn(usize) -> void  # hypothetical syntax
```

```text
use std.function.FnMut

struct Keep[F < FnMut(&i32) -> bool]:
    test: F

fn count_if[F < FnMut(&i32) -> bool](items: List[i32], mut test: F) -> usize:
    let mut n: usize = 0
    for item in items:
        if test(&item):
            n += 1
    n
```

```text
use std.annotation.inline

@inline(.Always)
fn count_if(items: List[i32], test: fn(&i32) -> bool) -> usize:
    let mut n: usize = 0
    for item in items:
        if test(&item):
            n += 1
    n
```

Also rejected: `*fn(..)` and `&fn(..)` (`*` and `&` are hd references;
`&fn` is already `syntax-error: a function value is already a reference`),
and a marker bound `fn(..) -> O + Bare` (marker bounds mean a crossing
parameter, `fn.crossing.parameter`, and fail to parse in a field).

The previous revision made `fn(..)` capture-free and spelled stored
closures `*mut dyn FnMut(..)`. The owner rejected it (2026-11-21), and
`ref.type.dyn.no-fn` forbade that type anyway.

## D. Today in hd

- `fn(..)` is (table slot, counted environment), environment 0 when
  nothing is captured; a code instance takes (environment, parameters,
  context) (`crates/hd_wasm/src/emit/closure.rs`).
- Stage cases, built `v8 release` and kept in `/tmp/lt255/stage-cases`
  (not in the repo). A field call (`(self.transform)(x)`):

  ```wat
  local.get $self
  i32.load $heap
  local.get $self
  i32.load $heap offset=4
  local.set $self
  local.set $t0
  local.get $self
  local.get $x
  i32.const 0
  local.get $t0
  call_indirect $table0 (param i32 i32 i32) (result i32)
  ```

  A capturing closure (`fn(x: i32) -> i32: x * k`) calls
  `rt:new/class-1` and stores `k`. A closure in a generic field gives
  `Keep[F].count[fn(i32) -> bool]`, which calls it with `call_indirect`.
  Release builds take no counts at these sites (#1436 Route B).
- `F` already solves to `fn(..)` for a closure (`fn.trait.bound.closure-inputs`),
  and `hd_opt`'s literal-consumer copies specialize some consumers under a
  budget. `List[T].filter` was not specialized in the stage case.
- **Bench method.** `bench/study/closures.hd` as is; the same with
  `MapR`/`Even` structs written for the two closures; and a fused loop.
  Each built with `hd build --release`, run as `HD_ENGINE=node hd
  FILE.wasm`, timed by `hyperfine -w 2 -r 10`; all print 745614.
- **Instance sizes.** Release module bytes with a second element type
  (`i32` and `i64`) at the same call: a second `sorted_by` +4873,
  `filter` +159, `.iter().map(..).sum()` +379.

## E. Details

- **Spec (07, 13):** new `fn.closure.type` (the two positions),
  `fn.closure.type.copy`, `fn.ptr.*` (the `FnPtr` table, `closure-captures`);
  `fn.trait.bound.closure-inputs` changes "still solves `F`" to the
  closure's own type; `fn.capture.*` gain the frame-slot rule from #1449.
- **Ownership:** a closure type counts its captures as struct fields do;
  `FnPtr` is never counted; `fn(..)` is unchanged, Route B included.
- **Diagnostics:** `closure-captures` at the closure, naming its first
  capture, with a fix-it to the `fn(..)` type.
- **Phases** (each about an hour, each with its test): closure types in
  the checker; mono keys and direct calls; delete the consumer copies;
  std `?Copy` bounds and the four `Result`/`Option` moves; bench report;
  then `FnPtr` (type, ABI, std moves); then #1449 (frame-slot cells, with
  the issue's stage case).
- **Parse log** (`scripts/hd-conformance.sh parse` and `hd check`, at
  6712c5923): blocks 1, 2, 5 and 6 parse; 3 and 4 fail at `fn` (`expected
  a line ending`), the hypothetical modifiers. Blocks 5 and 6 check.
  Blocks 1 and 2 fail `hd check` only on the import of `FnPtr`, which
  does not exist yet; with `fn(usize) -> void` in its place, both check.
  Blocks 1, 2, 5 and 6 pass `hd fmt --check`.
