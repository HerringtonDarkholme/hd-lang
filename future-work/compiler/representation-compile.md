# Runtime Representation: The Compile Side

Status: Study, not decided, 2026-10-07 (compile side).

Part of the [compiler design](README.md). A second study looks at the same
representation from the runtime side. A later lowering pass merges both
into [wasm-layout.md](wasm-layout.md) and [codegen.md](codegen.md). This
file decides nothing. Proposals marked "mine" come from this study, not
from the owner.

## Summary

**What it costs.** Under the current design, a 10k-line application
collects about **4,800 instances**. About 120 of them fold, and about
**4,700 functions** and **2,700 Wasm types** are emitted. The module is
about **640 KB**: 420 KB of code, 30 KB of types, and 190 KB of names,
lines and sites. A 50k-line program is about 22,000 instances and
2.9 MB. These are model numbers with stated rates (section 1), not
measurements. They match the toy compiler's 58 bytes per source line on
`calc.hd`.

**Derives and iterator chains dominate, not collections.** Derived
`Debug`, `Eq`, `Hash` and serde code makes about 27% of the instances.
Iterator adapters and their closures add 14%, and `List` and `Map`
methods 17%. Proposal A (shared `eqref` storage) folds about 600 more
functions and removes about 40% of the Wasm types. It cuts bytes by only
about 12%.

**`dead-code` (10 KB per 1,000 lines) is out of reach.** Every design
here lands at 48 to 64 KB per 1,000 lines, and code alone is 33 to 42.
The toy compiler measures 58 B per line. The target needs an owner
decision (question 1).

**Engine compile time is not the latency risk; numbering is.** Cranelift
at `Speed` is estimated at about 1.5 µs per Wasm byte on one core. That is
0.6 CPU-s for the 10k application, or about 0.1 s on 8 cores. The risk is
that the per-function Cranelift cache stops hitting. The design numbers
panic sites, literal and fact globals, context key ids, Wasm types and
`ref.func` targets densely across the whole program. One inserted site,
literal or type shifts every later number. Every function that holds a
later number then changes its Cranelift input and recompiles. In a debug
build, almost every function holds a site number. So **one edit can
recompile most of a test program**. This breaks the rule that a
function's code depends only on its own definition. Section 3 lists six
such rules, and each has a cheap fix.

**`test-latency` (≤ 300 ms) is reachable** with changes 1 to 4: about 95 ms
on 8 cores and about 135 ms on one (section 2.4). Without them it is
about 130 ms on 8 cores but about 380 ms on one, and agents often get one
core each. **`edit-latency` is a `hd check` metric**, so codegen does not
touch it.

**Ranked compile-side changes** (section 7):

1. stable, type-only numbering;
2. no standalone code for always-inlined callees;
3. per-program packs, instead of thousands of files per link;
4. filtered test programs for `--filter`;
5. A, applied at collection: one instance per representation class;
6. a compact names section;
7. folding stays in debug, and debug uses Cranelift `Speed` unless the spike says otherwise.

B is neutral for compile time. C is neutral for bytes, and it needs a
budget per caller.

## 1. The Instance-Count Model

### 1.1 Method

An instance is one `(item, type arguments)` pair that collection pushes
(codegen.md §13.2). The model counts instances per source of instances:

```text
instances = non-generic user functions + user closures + reachable non-generic std
          + Σ over generic std and user items: distinct type-argument tuples that reach it
```

Inputs, measured where possible:

- **The std generic surface**, counted with a throwaway script over
  `lib/std/*.hd` (not committed). It counts every function with a body
  and marks it generic when it has its own type parameters, sits in an
  `impl[...]`, or is a trait default.
- **Type-argument density**, from the four dogfood programs in
  `examples/dogfood/` (618 lines, 15 data and enum types). They are small,
  so the rates are rounded toward a mid-size application.
- **Bytes per body**, calibrated against the toy compiler. Its baseline
  report gives 14,442 B for `calc.hd` (248 lines), or 58 B per line
  ([goals.md](goals.md#prototype-baselines-to-beat-2026-10-06)). The model
  gives 55 to 64 B per line for every program size.

### 1.2 The std Generic Surface

| Measure | Value |
| --- | --- |
| std functions with a body | 966 |
| generic | 331, with 1,661 body lines; median 4 lines, mean 5.0 |
| non-generic | 635, with 4,208 body lines; median 4 lines |
| generic methods with their own type parameters | 143 (`map[U]`, `serialize[W]`, `fold[A, $R]`, ...) |
| std data and enum types | 117; 30 of them generic |

Generic bodies by file:

| File | Generic bodies | Lines |
| --- | ---: | ---: |
| `collections.hd` | 80 | 462 |
| `serde.hd` | 44 | 140 |
| `format.hd` | 36 | 153 |
| `cmp.hd` | 29 | 185 |
| `iter.hd` | 22 | 158 |
| `testing.hd` | 21 | 115 |
| `ops.hd` | 20 | 62 |
| `num.hd` | 14 | 94 |
| others (13 files) | 65 | 292 |

`text.hd`, `json.hd` and `regex.hd` are almost all non-generic: 227 bodies
and 1,911 lines, compiled once per program. Today `List.push`, `len`,
indexing and the `Map` core are checker intrinsics. In the new compiler
`List` and `Map` are std hd over `Array` (wasm-layout.md §15.2), which
adds about 25 generic bodies (push, grow, get, set, probe, rehash, insert,
remove, iteration). The model includes them in the per-type method counts.

**What a generic body does with `T`** decides whether instances can fold.
The 118 generic bodies in `collections.hd`, `iter.hd`, `option.hd` and
`result.hd` fall into three classes:

| Class | Bodies | Folds under exact types | Folds under A |
| --- | ---: | --- | --- |
| moves `T` only: stores, loads, copies, returns it | 47 (40%) | only when the element Wasm types match | yes, one body per representation class |
| calls a closure over `T`, or builds one (`map`, `filter`, every iterator adapter) | 34 (29%) | no | only if function types are erased too (A2 below) |
| calls a trait method on `T` (`==`, `cmp`, `hash`, `debug`) | 37 (31%) | no | no: the callee differs per `T` |

The trait-method count is high, because the script also counts integer
index compares. The model uses 50% move-only, 30% closure and 20% trait.

### 1.3 Application Rates

| Rate | Value | Source |
| --- | --- | --- |
| user functions and methods | one per 9 lines | std non-generic median is 4 lines, dogfood about 9 |
| user closures | one per 60 lines | dogfood: 13 adapter calls in 618 lines |
| user data and enum types | one per 70 lines | dogfood: one per 41; larger programs are less dense |
| reference `List` element types | 0.35 per user type, plus 6 | dogfood: 6 list element types for 15 types |
| `Option`, tuple and value-enum `List` elements | 15% of reference ones, plus 1 | estimate |
| `Map[K, V]` types | one per 500 lines, plus 4 | dogfood: 3 per 618 lines, mostly `string` keys |
| `T?` and `Result` types with helper calls | one per 80 lines | estimate |
| iterator chains | one per 70 lines, 8 instances each, 60% not shared with another chain | dogfood |
| `List` methods per element type | 9 | push, grow, get, index, len, iter and its closure, plus 2 |
| `Map` methods per `(K, V)` | 12 | including probe and rehash |
| derived instances per user type | 9 | `Debug` 0.9 × 4, `Eq` 0.6 × 2, `Hash` 0.3 × 2, serde 0.3 × 12 (walker members per field type) |
| bytes per body | user 140, closure 60, std generic 70, std non-generic 100 to 120 | release LEBs; calibrated above |
| type-section bytes per type | 11 | struct of 3 to 4 fields, or a func type of 3 to 4 values |
| metadata per function | 32 B of names and function section, 0.6 B per statement of `hd.lines` | `name` holds printed paths with type arguments |

### 1.4 Results

Folding under exact types counts scalar twins (`List[i32]` and
`List[u32]`), 10% of reference element types that share a field layout
(wasm-layout.md §15.3), and 5% elsewhere. "A" is shared `eqref` storage
with erased generic signatures, and exact closure types (A1 in section 5).
"B" boxes value enums over the size threshold; the model boxes half of
the multi-value element types. "C" is closure specialization with a
budget per caller: 70% of chain instances are inlined and dropped, and
each chain adds 180 B to its caller.

The `lib/std` rows are a program that reaches all of std. Each of the
635 non-generic bodies is compiled once, and each of the 331 generic
bodies at 5 type-argument tuples: `i32`, `u8`, `string` and two user data
types.

| Program | Design | Instances | Folded or dropped | Emitted functions | Wasm types | Type section | Code | Total Wasm | KB per 1k lines | Cranelift `Speed`, 1 core | Liftoff, 1 core |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `lib/std` | current | 2,290 | 83 | 2,207 | 800 | 9 KB | 169 KB | 264 KB | n/a | 0.26 s | 30 ms |
| `lib/std` | A | 2,290 | 381 | 1,909 | 550 | 6 KB | 149 KB | 230 KB | n/a | 0.23 s | 26 ms |
| `lib/std` | A+B | 2,290 | 381 | 1,909 | 550 | 6 KB | 149 KB | 230 KB | n/a | 0.23 s | 26 ms |
| `lib/std` | A+B+C | 2,290 | 481 | 1,809 | 520 | 6 KB | 142 KB | 219 KB | n/a | 0.22 s | 25 ms |
| 10k-line app | current | 4,826 | 123 | 4,703 | 2,740 | 29 KB | 417 KB | 636 KB | 64 | 0.64 s | 73 ms |
| 10k-line app | A | 4,826 | 704 | 4,122 | 1,560 | 17 KB | 378 KB | 561 KB | 56 | 0.58 s | 66 ms |
| 10k-line app | A+B | 4,826 | 727 | 4,099 | 1,551 | 17 KB | 373 KB | 555 KB | 55 | 0.57 s | 65 ms |
| 10k-line app | A+B+C | 4,826 | 1,207 | 3,619 | 1,304 | 14 KB | 365 KB | 529 KB | 53 | 0.56 s | 64 ms |
| 50k-line app | current | 22,403 | 548 | 21,854 | 12,876 | 138 KB | 1,917 KB | 2,930 KB | 59 | 2.94 s | 334 ms |
| 50k-line app | A | 22,403 | 3,286 | 19,117 | 7,217 | 78 KB | 1,729 KB | 2,577 KB | 52 | 2.66 s | 301 ms |
| 50k-line app | A+B | 22,403 | 3,384 | 19,019 | 7,177 | 77 KB | 1,708 KB | 2,551 KB | 51 | 2.62 s | 297 ms |
| 50k-line app | A+B+C | 22,403 | 5,784 | 16,619 | 5,943 | 64 KB | 1,670 KB | 2,420 KB | 48 | 2.56 s | 291 ms |

The last two columns are code bytes times 1.5 µs and 0.17 µs per byte
(section 2). Divide by the core count for wall time. **Engine compile
time scales with the code column.** wasmtime's module load and V8's
compile scale with the type count too, because both register and
canonicalize every type. Instantiation scales with neither. It scales
with the constant globals whose initializers allocate (section 2.5).

Breakdown of the 10k application's 3,163 generic instances under the
current design:

| Source | Instances | Share of all 4,826 |
| --- | ---: | ---: |
| derived code (`Debug`, `Eq`, `Hash`, serde) | 1,286 | 27% |
| iterator chains: adapters, their closures, `collect` | 686 | 14% |
| `List` at reference elements | 504 | 10% |
| `Option` and `Result` helpers | 300 | 6% |
| `Map` | 288 | 6% |
| sorting | 64 | 1% |
| `List` at scalar elements | 36 | 1% |
| non-generic: user functions 1,111, closures 167, std 300 | 1,578 | 33% |

**Sensitivity.** Derives are the largest and least certain row. If
`Walker.member` and `Source.member` calls are trivially inlined, which
codegen.md §12.3 implies ("straight-line code"), the per-field instances
disappear and derived instances drop to about 4 per type. Then the 10k
application has about 4,100 instances. A per-field member that is
inlined everywhere still costs emission and a cache entry today, because
collection pushes it before any caller decides to inline it. That is
change 2 in section 7.

### 1.5 What The Model Says About Design Claims

| Claim | Verdict |
| --- | --- |
| "a 10k-line program with about 5,000 instances" (data-structures.md §3.22) | consistent: 4,100 to 4,800 |
| folding gives the .NET sharing for references (research.md Q8) | already withdrawn in codegen.md §13.7; the model confirms it: 123 folds of 4,826 |
| exact types cost only type-section bytes (research.md Q8, Risks) | about 30 KB at 10k lines, but also about 2,000 func types (800 more than under A) and a cache-sensitive type order (section 3) |
| the 2 KB tiny program | not tested here: a tiny program has about 20 instances; its risk is metadata and helpers (wasm-layout.md §15.6) |
| `dead-code` ≤ 10 KB per 1,000 lines | missed about 5x under every design; question 1 |

Names are a large share of the bytes that are not code. The `name`
section holds a printed path with type arguments per function, such as
`std.list/List.push[shop.cart/Item]`. At 4,700 functions that is about
150 KB, a quarter of the 10k module. A compact `hd.names` section of
path and type-argument indices cuts it to about 30 KB (change 6).

### 1.6 Prior Art

| System | Generics | What its compiler sees |
| --- | --- | --- |
| Rust | full monomorphization | some generic functions are instantiated hundreds or thousands of times, such as `Vec::push`, `Option::map` and `Result::map_err`. `RawVec::grow` alone was 1 to 8% of LLVM IR, until it was split into a non-generic part ([rust#72013](https://github.com/rust-lang/rust/pull/72013); [Rust Performance Book](https://nnethercote.github.io/perf-book/compile-times.html)) |
| Go 1.18+ | one body per GC shape, plus a dictionary | all pointer types share one shape, so instance counts stay low; generic code can run about 2x slower ([design](https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md); [PlanetScale](https://planetscale.com/blog/generics-can-make-your-go-code-slower)) |
| .NET | one shared body for reference types, one per value type | the A proposal without its dictionary ([BOTR](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md)) |
| dart2wasm, Kotlin/Wasm | type parameters erased to a top type, with casts on reads (from the compilers' published designs; not re-checked in this pass) | one body per generic function; Binaryen removes casts afterwards |
| MoonBit | whole-program monomorphization over its own multi-level IR | reports very small Wasm (a 253 B Fibonacci) and fast builds; no published instance counts ([MoonBit](https://www.moonbitlang.com/blog/first-announce)) |

hd is closest to Rust with one difference that helps. Rust has drop glue
and layers such as `RawVec`, `Vec` and `IntoIter`; hd has no drop glue,
and collections are one std layer over `Array`. So hd should see fewer
instances per element type than Rust. It should see more than Go or
.NET, which share every reference type.

Rust's fix for `RawVec::grow` applies to hd as well. A body that does not
depend on `T`'s representation should not be emitted once per `T`. A at
collection time (section 5.1) is that fix, done once for all of std.

## 2. Engine Compile Time

### 2.1 Throughput

| Engine and tier | Throughput, one core | Source |
| --- | --- | --- |
| V8 Liftoff | about 150 to 200 ns per input byte (5 to 7 MB/s) | [Titzer 2022](https://arxiv.org/abs/2205.01183), Fig. 8: PolyBench, i7-8700K, linear-memory code |
| V8 TurboFan | about 1,700 to 2,000 ns per byte | same figure |
| JSC BBQ | about 800 ns per byte | same figure |
| Cranelift (wasmtime) | not published as an absolute number. Estimated at 1.0 to 2.0 µs per byte at `Speed`, the TurboFan class, since both are optimizing compilers with an IR and a full register allocator | estimate; spike 0c measures it |
| Cranelift `OptLevel::None` | estimated 10 to 25% faster than `Speed`. `None` skips the e-graph mid-end; lowering and register allocation stay, and they dominate | the e-graph optimizer cost "about the same" as the old passes ([Cranelift 2022](https://bytecodealliance.org/articles/cranelift-progress-2022)); spike 0c measures it |
| Cranelift single-pass register allocator | faster, linear-time allocation, more spills and moves | [wasmtime `Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html); it was disabled for a time over exception handling ([#10554](https://github.com/bytecodealliance/wasmtime/pull/10554)) |
| Winch | 15 to 20x faster than Cranelift, code 1.1 to 1.5x slower, but no GC support | [baseline RFC](https://github.com/bytecodealliance/rfcs/blob/main/accepted/wasmtime-baseline-compilation.md) |

Liftoff's figure is for linear-memory code. Wasm GC code has more calls,
casts and allocations per byte, so both engines may be slower per byte on
hd output. The spike measures hd-shaped modules on both.

**The main finding: `None` is not a cheap debug tier.** The design hoped
debug could use `OptLevel::None` to compile fast (engines-and-test-runner.md
§18.6). Register allocation is most of Cranelift's time at either level,
so `None` saves perhaps a fifth of the time. The single-pass allocator
saves more, but its spills slow the code, which counts against
`release-check-cost`. Caching beats both: a cache hit skips the whole
compile.

### 2.2 The Per-Function Cache: What Hits

wasmtime's incremental cache keys each function by its "meaningful" CLIF.
The design abstracts source locations and the indices of called functions,
so that adding or removing a function does not miss every caller
([wasmtime#4155](https://github.com/bytecodealliance/wasmtime/issues/4155);
Cranelift `ExternalName::User` names live outside the cached stencil).
Everything else that the Wasm body names becomes a constant or a `vmctx`
offset in the CLIF, and so part of the key. That covers:

| What a body names | Abstracted by the cache? | hd's current numbering |
| --- | --- | --- |
| a direct `call` of a defined function | yes, an external name | instance-key order; shifts on insert, harmless |
| a call of an import | no: a `vmctx` offset per import index | sorted by module and name; rarely changes |
| `global.get` and `global.set` | no: a `vmctx` offset per global index | literals by first reference, facts and constants in link order |
| `ref.func` (closure code, adapters) | probably not: a constant func-ref index | instance-key order |
| `struct.new`, `ref.cast`, `array.new` type immediates | partly: field offsets come from the type alone, but allocation and cast checks read a module type index | topological order with a sorted ready set |
| `array.new_data` segment index | no | first reference in function order |
| `i32.const` site numbers, key ids, type ids | no, they are plain constants | site numbers global at link; key ids in key order at link; type ids unspecified |

The rows marked "probably" and "partly" depend on wasmtime internals, so
spike 0c checks each row (experiment S2).

**When a generic std function changes:** the std pack hash is in
`toolchain_key` (cache.md §5.3), so every `code` entry misses, and so does
every Cranelift entry whose CLIF changed. Users never edit std. This
project's agents do, and they pay a cold build of every test program
after each std edit. That is acceptable, but suite timing should expect it.

**When a user generic function changes:** each of its instances
re-emits, plus each caller that inlined it. For Cranelift, those bodies
miss. Callers that did not inline it hit, because the call target is an
external name.

**When a user data type changes:** `layout_hash(T)` changes, so every
instance that lays out `T` re-emits. Under exact types that includes every
`List[T]` method, every derived instance of `T`, and every function that
touches `T`'s fields. Under A, a move-only `List[T]` body never lays out
`T`, so it does not re-emit (section 3.3).

**Collateral misses (the current design).** A one-line edit can add or
remove a panic site, such as an overflow check in debug. Site numbers are
renumbered globally at link (codegen.md §13.8, `Site` relocations), so
every later function's `i32.const` changes and its CLIF misses. The same
holds for a new long string literal (a new global), a new fact, a new
Wasm type early in the order, or a new requirement key. In a debug build
nearly every function has sites, so **an edit early in the instance order
recompiles about half the program.**

### 2.3 Does Merging Run Before Cranelift?

Yes. Folding runs in `Link` (codegen.md §13.7), so Cranelift sees only
representatives. Merging changes neither code keys nor relocations of
cached entries, because folding reads code entries and never writes them.
A fold class's representative is its smallest instance key. When a new
member with a smaller key joins, the name changes but the bytes do not,
and the Cranelift key holds no names, so it hits. When an edit splits a
class, a new function appears. Function indices shift, which is harmless
for calls, but not for `ref.func` (S2 checks this).

Folding saves Cranelift work in proportion to the folded functions:
3% under exact types and 15 to 25% under A or A+B+C. Its own cost is one
hash per body per round: about 400 KB times 2 to 3 rounds, under 1 ms.
**Folding must stay on in debug.** Turning it off would cost more
Cranelift time than it saves.

### 2.4 `test-latency` And `edit-latency` After A One-Function Edit

`edit-latency` is `hd check` only (goals.md, Pillar 1), so no codegen
step is in it. `test-latency` is edit, then `hd test --filter one`. That
builds the edited module's unit-test program, which reaches that module
and the std it uses. The model sizes it at 800 instances and 250 KB of
code.

| Step | Current design, 8 cores | Current, 1 core | With changes 1 to 4, 8 cores | With changes 1 to 4, 1 core |
| --- | ---: | ---: | ---: | ---: |
| process start, warm fast path | 30 ms | 30 ms | 30 ms | 30 ms |
| check the edited module (edit-latency budget) | 40 ms | 40 ms | 40 ms | 40 ms |
| collect the test program | 2 ms | 2 ms | 1 ms (filtered roots) | 1 ms |
| emit the changed instances and inliners | 1 ms | 1 ms | 1 ms | 1 ms |
| link: read 800 code entries, fold, types, patch | 6 ms | 19 ms (800 files) | 3 ms (one pack) | 5 ms |
| Cranelift: translate and hash every function for cache lookup (assumed 25% of a full compile) | 12 ms | 90 ms | 6 ms (filtered: about 400 functions) | 45 ms |
| Cranelift: real misses | 1 ms | 3 ms | 1 ms | 3 ms |
| Cranelift: collateral misses (half the program) | 25 ms | 180 ms | 0 | 0 |
| assemble the object, load and register types | 10 ms | 10 ms | 8 ms | 8 ms |
| instantiate and run one case | 3 ms | 3 ms | 3 ms | 3 ms |
| **total** | **about 130 ms** | **about 380 ms** | **about 95 ms** | **about 135 ms** |

The single-core column matters. The `concurrency` metric runs up to 64
`hd` processes at once, so an agent often gets one core. **Verdict:**
`test-latency` holds with changes 1 to 4, with margin, and misses on one
core without them. The largest unknown is the cost of a cache hit, which
the table assumes is 25% of a full compile. Wasmtime still translates
every function to CLIF to compute its key. Spike 0c measures it (S3).
If a hit costs more than 40%, see the "split modules" fallback in
section 7.

Writing the new `cwasm` (a few MB) is off the critical path: `hd test`
can run the case from the in-memory module, then write the entry before
it exits (mine).

`hd run` of the whole 10k application after one edit: about 4,700
functions and 420 KB of code. Lookup costs about 160 ms on one core, or
25 ms on 8. Reading 4,700 code entries as separate files adds 70 to
140 ms on one core, so `hd run` needs the packs of change 3 as well.

### 2.5 Instantiation

Instantiation cost does not scale with code bytes:

- **Module load:** a `cwasm` is mapped in O(1). Type registration is
  linear in Wasm types: 2,700 types under exact types, 1,600 under A.
  This is paid once per program per process.
- **Per instance:** every immutable global whose initializer runs
  `struct.new` allocates per instance. That covers vtables, capture-free
  closures, payloadless singletons, member handles and short literals
  (wasm-layout.md §15.4). At 10k lines that is perhaps 1,500
  allocations, or 50 to 100 µs per instance. This is the "init per
  case" risk that engines-and-test-runner.md §19.3 already names. A does
  not change it. Short literals as constants are the largest group, and
  making them lazy like long literals trades instantiation time for a
  check per use. The runtime study owns that trade.

## 3. Incremental Behaviour Of Each Layout Rule

### 3.1 The Principle

A function's emitted bytes may depend on its own TIR, its type arguments,
the definitions of the types and impls it reads, and the inline summaries
of its callees. It must not depend on which other functions, types,
literals or sites the program contains. The hd-level `code` cache already
follows this, because relocations are symbolic. **The principle has to
hold one level lower too**, at the bytes Cranelift hashes. That is the
gap in the current design.

### 3.2 Each Layout Rule

| Rule (wasm-layout.md §15) | Depends on | Type-only? | What an edit to the type invalidates |
| --- | --- | --- | --- |
| scalar classes, packed fields | the type | yes | nothing beyond its users |
| `data T` as a struct of mutable fields | `T`'s fields, by layout | yes | instances that lay out `T`: field access, construction, derived code, `List[T]` methods under exact types |
| structural sharing of equal layouts | the descriptor | yes, but which types share is a program fact | link only |
| embedded part as a separate struct | the part's own fields | yes | the part's users |
| `T?` as nullable or tagged | `T`'s class | yes | users of `T?` when `T` changes class (rare) |
| value enum: `multi` up to 4 values, boxed beyond | the enum's payload types' layouts, deeply | yes, transitively through value payloads; data payloads stop the chain | every enum and tuple that holds it by value, and their users |
| box shape: flat (≤ 4 payload fields) or subtypes | the enum alone | yes | the enum's users |
| `Result` as `multi` | both payload layouts | yes | users of that `Result` type |
| `Array` of a `multi` as a struct of arrays | the element layout | yes | instances over that array |
| closure base per signature, subtype per capture shape | the signature and the captures | yes | the closure's creator and callers |
| rec groups by SCC, members ordered by `canon` bytes | the type graph around the type | yes, by the SCC | types in the same SCC |
| **type section order** (Kahn, sorted ready set) | **the whole program's type set** | **no** | Cranelift input of every function that names a later type |
| erased-ABI witness fields `f_I`, one per impl in the program | **the program's impls of `m`** | **no** | every erased body of `m`, and the witness type |
| vtable shape | the trait | yes | coercions to the trait |
| defaultable forms | the layout | yes | |
| **site numbers**, renumbered globally at link | **every site before this one** | **no** | every later function with a site |
| **literal globals and data segments**, by first reference | **every literal before this one** | **no** | every later function that reads a literal |
| **fact, storage and constant globals** | **link order** | **no** | every function that reads a later global |
| **context key ids**, numbered at link in key order | **the program's key set** | **no** | every function that looks up or extends a context |
| type ids for `Any` and `Inspectable` | unspecified | must be decided | `type_id`, `downcast` and their thunks |
| function indices for `ref.func` | instance-key order | no | functions that build closures over a shifted function (if S2 confirms) |
| hot-reload table slots (Later) | content order | no | every user call when the switch is on |

### 3.3 The Violations And Their Fixes

All fixes are mine, and none changes behaviour.

1. **Site numbers.** Do not emit them. An explicit panic calls the
   category's stub with no site immediate. The host finds the site from
   the caller frame's code offset, through `hd.sites`, as it already does
   for engine traps (wasm-layout.md §15.5). This removes one `i32.const`
   per site and the program-wide numbering. If a stub must store a site,
   use a site number local to the function, read together with the
   function index from the backtrace.
2. **Context key ids and type ids.** Use the first 32 or 64 bits of
   `H(canon(K))` instead of a dense number. Link checks for collisions
   within the program. On a collision, link fails with an internal error,
   or rehashes every id with a fixed salt, deterministically. Either way
   the ids depend only on the type.
3. **Literal, fact and other numbered globals.** Two options:
   (a) reach each lazily initialized global through a small getter
   function, so the reader holds a call, which the cache abstracts;
   (b) keep direct `global.get`, and order globals so that most edits
   append. Option (a) costs a direct call per long-literal or fact read,
   which already pays a null check. Short literals and vtables stay
   direct constants: they are only `global.get`, and they are ordered by
   content. Spike 0c decides by S2's hit rates.
4. **Type section order.** No dense order is stable under insertion.
   The first release keeps the content order and measures the misses (S2).
   If type changes miss more than a few functions, the fix is to let
   allocation and cast sites name types through a call to a per-type
   helper, or to ask wasmtime to abstract type indices in its cache key.
   That second option is an upstream change.
5. **Witness fields.** Keep them. Erased bodies are few, and a new impl
   of a `dyn` generic method recompiles only that method's erased bodies.
   Record it as a known exception.
6. **`ref.func`.** If S2 shows misses, route closure construction through
   one constant global per closure code (the code reference as an
   immutable `funcref` global). That turns it into case 3.

With fixes 1 to 3, a one-function edit recompiles that function, its
inliners, and (by fix 4's measurement) few others.

### 3.4 What Each Edit Invalidates

| Edit | Interfaces | TIR | `code` entries (instances) | Wasm module | Native code (fixed numbering) |
| --- | --- | --- | --- | --- | --- |
| private function body | none | its module | its instances; inliners; callers whose inline summary changed | relinked; one body differs | that body and its inliners |
| public non-generic function body | none (signature unchanged) | its module | same as above, across modules | same | same |
| user generic function body | none | its module | every instance of it; their inliners | same | those bodies |
| field added to `data T` | `T`'s folder interface; dependents recheck | dependents | every instance with `T` in its layout set; under A, not the move-only collection bodies | many bodies; new type | those bodies; plus type-order misses (fix 4) |
| variant added to an enum | interface; exhaustiveness recheck of dependents | dependents | instances that lay out the enum, and enums or tuples holding it by value; a `multi` to box switch cascades to their users | many bodies | those bodies |
| std function body (toolchain update) | all, through `toolchain_key` | all | all | all | functions whose CLIF changed |
| new fact or long literal in one function | none | its module | that instance | one body, one global | that body only with fix 3; else every later reader |
| a new panic site (any edit in debug) | none | its module | that instance | one body | that body only with fix 1; else every later function |

## 4. Emission And Link Pipeline Cost

| Stage | Work | Order | 10k application |
| --- | --- | --- | --- |
| `Collect` | worklist over instances; scan each body's tags; substitute per call; `select` with a shared memo; code keys from collection's record | linear in Σ instance TIR size; `select` is memoized per concrete trait reference | about 4,800 × 50 instructions × 20 ns ≈ 5 ms |
| code key computation | per instance: about 30 hashed items (layout hashes, impl hashes, callee summaries) | linear | about 1 ms |
| `Emit` (misses only) | one walk plus bounded analyses per instance | linear per instance times loop depth | 50 µs per instance; cold 4,800 instances ≈ 0.25 CPU-s |
| `Link`: read entries | one file per `code` entry (cache.md §5.4) | linear, but one open and map per entry | 4,800 × 15 to 30 µs = 70 to 140 ms on one core; this is the largest link cost |
| `Link`: fold | hash per body per round, until no merge | O(rounds × bytes); rounds bounded by call-graph depth | under 1 ms at 3 rounds; see risk 1 |
| `Link`: types | canonical descriptors; Tarjan's SCC; Kahn with a sorted ready set | O(T log T) | under 1 ms |
| `Link`: patch, LEB re-encode | per relocation; per-body offset maps | linear | 1 to 2 ms |
| `Link`: custom sections | names, sites, lines, folds | linear | 1 ms |
| `Precompile` | Cranelift, parallel per function | linear in code bytes, with super-linear risk per large function (risk 3) | 0.64 CPU-s cold at `Speed` |

The link is linear except for these risks:

1. **Fold rounds.** Recomputing every key each round costs O(depth ×
   bytes). A deep call chain (50 levels) gives 50 rounds over all bodies,
   about 30 MB of hashing, which is still only a few ms. Make it linear
   anyway: after round one, recompute only bodies whose callees changed
   class (a worklist).
2. **Witness fixpoint.** "Link fills every field of every witness". The
   thunk count is (methods × type-argument tuples × impls × open
   instructions), a product of program sizes (codegen.md §13.5.1). It is
   bounded by `dyn` generic use, which is rare. Count it in the compiler
   statistics, and add the `pathological` case "20 impls × 50 call
   types".
3. **Function size after inlining.** Cranelift's backtracking allocator
   can be super-linear in function size (the
   [wasmtime `Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html)
   docs say so). C and bounded inlining grow functions. The inliner budget
   must cap the size of the caller after inlining, not only the callee's
   size (section 5.3).
4. **File count.** Not quadratic, but the constant is large. The fix is a
   per-program pack (change 3).
5. **Collection of `dyn` methods.** A coercion pushes every method of the
   trait at the type, supertraits included. That is linear in coercion
   sites times trait width. It is fine, but it is the reason a wide trait
   used as `dyn` with many types grows the program.

Memory: data-structures.md §3.24 estimates 6 MB of hd structures plus 20
to 40 MB of Cranelift for a cold build of 10k lines. The model's 4,700
functions agree with its 5,000-instance assumption.

## 5. Proposals A, B And C From The Compile Side

### 5.1 A: Shared Representation For Reference Type Arguments

Two variants:

- **A1:** a type parameter whose argument is a reference is stored as
  `eqref`, and generic signatures carry it as `eqref`. Closure types stay
  exact. Callers cast results back to the exact type.
- **A2:** A1, plus erasing reference parameters in function types, so
  `fn(Point) -> bool` and `fn(User) -> bool` share one `$Fn_sig`, and
  closure code casts its parameters on entry.

**Apply A at collection, not at folding (mine).** Folding after emission
still emits, caches and hashes 56 copies of `List[T].push`, and then
keeps one. A can be applied before that. Each generic item gets a
**representation summary** per type parameter, computed once at check
time and stored in its `tir` entry. The summary says whether the body
needs `T`'s exact representation: a field access, a trait call on `T`, a
construction, or a closure type that mentions `T`. If the body only
moves `T`, collection rewrites the type argument to its class (`REF`,
`REF?`, or the scalar class) in the instance key. So `List[Point].push`
and `List[User].push` are one instance, emitted once. The rule depends
only on the item's own body and the argument's class, so it is
type-only. Callees of the rewritten instance see the class as their
argument too.

| Effect | A1 at 10k lines | Mechanism |
| --- | --- | --- |
| instances emitted and cached | −580 (−12%) | move-only collection and `Option` bodies, and the `V` side of maps |
| Wasm types | −1,180 (−43%) | one `$List_eq` and `$Arr_eq`; fewer map structs; func types of generic signatures collapse |
| code bytes | −39 KB (−9%) | fewer bodies; each caller adds a 3 to 4 byte `ref.cast` per element read |
| Cranelift CPU | −9% | proportional to code bytes |
| incremental: field added to `T` | move-only collection bodies no longer re-emit | they no longer lay out `T` |
| incremental: type-order misses | fewer types, so fewer shifts | |
| erased `dyn` ABI | a reference `T` reaches `List[REF].push` directly, without a thunk | the open value is already the shared representation |
| determinism | unchanged | the summary is part of the item's TIR hash |

A2 adds about 470 folds at 10k lines (closure-taking collection methods
and adapter closures), and about 30 KB less code. It puts a cast in every
closure that takes a reference parameter, which is the runtime study's
question. From the compile side, **A1 at collection is worth doing.** A2
is worth it only if the runtime study finds the entry casts cheap.

### 5.2 B: A Size Rule That Boxes Large Value Enums

Today a `List` of a value enum is a struct of arrays, one array per Wasm
value of the largest variant (wasm-layout.md §15.2). Each move in a
collection body then writes k arrays, so its code is about 2.2 times a
reference list's for the same method. It also adds one struct type plus
up to k array types per element type.

From the compile side:

- **Bytes:** boxing half the multi-value element types saves about 5 KB
  at 10k lines, and the boxed ones fold under A. That is a small win,
  because value-enum lists are a small share of lists.
- **Types:** about −10.
- **Incremental:** the size rule must read only the enum's own deep
  layout, never how the program uses it. A threshold on Wasm values per
  element (such as "box past 2") satisfies that. A rule like "box when it
  appears in a container" does not: one new container anywhere would
  change the enum's layout everywhere.
- **Cascades:** crossing the threshold changes the layout of every
  value-enum and tuple type that embeds it. That is already true of the
  4-value `multi` bound. Fewer thresholds mean fewer cascades, so B
  should reuse the existing bound, not add a second one.

**Verdict: neutral for compile time.** Choose the threshold by runtime,
and keep it a function of the enum alone.

### 5.3 C: Closure Specialization In The Inliner

`Iterator[T]` is a data type with a `step` closure, and every adapter
builds one closure and one iterator (`lib/std/iter.hd`). C inlines an
adapter chain into its caller, and the closures with it, so the chain
becomes one loop.

| Effect | 10k lines (A+B to A+B+C) | Note |
| --- | --- | --- |
| instances emitted | −480 | adapter and closure-body instances that every caller inlined are dropped |
| code bytes | −8 KB, net | each chain adds about 180 B to its caller, and shared adapter copies go away; mostly a wash |
| types | −250 | iterator structs and environment subtypes |
| emission CPU | +10 to 20% on callers with chains | the walk descends into adapter and closure TIR |
| Cranelift CPU | about flat in bytes; a super-linear risk in large callers | needs a cap on the caller's size |
| incremental | an edit to a user closure body re-emits only its creator, which already owns it; an edit to a user function that takes a closure and is specialized re-emits every specializing caller | the fan-out of an edit grows with each inlined generic user function |

The **budget must be local to each caller** (mine): callee size, the
caller's size so far, and nesting depth. A program-wide budget, such as
"inline until the program grows 10%", would make a caller's bytes depend
on other functions, which breaks section 3.1. A caller-local budget is
deterministic, and its inputs are already in `code_key` through the
inline summaries and the inlined items' TIR hashes (codegen.md §13.8).

**Verdict:** C is about neutral for bytes and Cranelift time, and it
helps runtime. Its compile-side cost is the larger edit fan-out of
generic user functions that take closures. Cap it with the budget, and
measure the cap on function size (experiment S6).

### 5.4 Combined

| Design | Emitted functions at 10k | Total Wasm | Change in Cranelift CPU | Edit fan-out |
| --- | ---: | ---: | --- | --- |
| current | 4,703 | 636 KB | baseline | numbering misses dominate until section 3's fixes |
| A | 4,122 | 561 KB | −9% | smaller for `data` field edits |
| A+B | 4,099 | 555 KB | −10% | same |
| A+B+C | 3,619 | 529 KB | −12% | larger for generic functions that take closures |

None of A, B or C moves `dead-code` by more than 20%. The bigger levers
for bytes are names (−120 KB at 10k lines) and derived code. For
latency, the bigger levers are numbering and file counts.

## 6. Debug Versus Release

> **Superseded by [tiering.md](tiering.md) (2026-10-07).** The owner
> chose a dev pipeline and an optimized pipeline. Bounded inlining,
> scalar replacement and closure specialization run in the optimized
> pipeline only. The dev pipeline's Cranelift setting is decided by spike
> T1, not S4, against `dev-speed`. `release-check-cost` became
> `check-cost` and `dev-speed`. The table, the verdict and change 7 below
> are kept as the study's record.

`release-check-cost` compares the runtime of debug and release on one
test suite (debug ≤ 1.3x release). Anything that lowers debug
differently counts against that ratio. Things that do not change the
running code can differ freely.

| Step | Skip in debug? | Why |
| --- | --- | --- |
| folding | no | it saves more Cranelift time than it costs (section 2.3) |
| bounded inlining, scalar replacement, C | no | they change runtime speed; release-check-cost needs the same lowering |
| A and B | no | they are representation; one layout per type in both |
| overflow and shift checks | debug only (spec) | they are already part of the 1.3x budget, and the i64 multiply check is the largest part |
| LEB re-encoding | debug skips it (already decided) | Cranelift's input is the same instructions; it saves a link pass |
| `wasmparser` validation | debug builds of the compiler only | not a user-tier choice |
| `name`, `hd.sites`, `hd.lines` | keep | backtraces need them in both |
| Cranelift `OptLevel::None` | only if S4 shows it saves ≥ 25% compile time and debug stays within 1.3x | section 2.1 expects about 10 to 25%, too little for its runtime cost |
| single-pass register allocator | no, unless S4 shows ≤ 1.1x runtime cost | its spills eat the 1.3x budget that overflow checks already use |
| `cwasm` serialization | after the result is printed | off the critical path |

**Verdict:** the latency levers for debug are caching, stable numbering,
filtered test programs and the pack format, not a cheaper lowering. The
debug tier should probably use Cranelift `Speed`, the same as release.
The decision rule is in S4.

## 7. Ranked Compile-Side Changes And Spike 0c

### 7.1 Ranked Changes

| Rank | Change (all mine) | Gain | Cost | Decided by |
| --- | --- | --- | --- | --- |
| 1 | **Stable, type-only numbering:** no site immediates (map by code offset); hashed key ids and type ids; getter functions or append-friendly order for lazy globals (section 3.3) | the per-function Cranelift cache keeps working; saves 25 ms on 8 cores and 180 ms on one, per edit | small; a call per long-literal or fact read under option (a) | S2 |
| 2 | **No standalone code for always-inlined callees:** collection does not push a callee that passes the trivial-inlining test, unless it is used as a value or a vtable slot | −15 to 25% of emitted instances (derive members, `Option` helpers, getters); less emission and fewer cache entries | collection reads the inline summary, which is already in the `tir` entry | the instance statistics |
| 3 | **Per-program packs:** link reads the previous link's code pack for unchanged code keys; the Cranelift `CacheStore` adapter batches per program | link I/O from 70 to 140 ms down to a few ms; the same for Cranelift lookups | a per-worktree record of the last link per program; packs count toward `disk` | S3 |
| 4 | **Filtered test programs:** with `--filter`, the roots are the selected cases plus the module's init groups | about half the functions to compile and look up for `test-latency` | one more program variant per filter; it shares every code entry; see question 3 | S3 |
| 5 | **A1 at collection:** representation summaries in TIR; instance keys by class for move-only bodies | −12% instances, −43% types, −9% code, smaller edit fan-out for `data` edits | one bit set per type parameter per item; casts at readers (runtime study) | S1, plus the runtime study |
| 6 | **Compact names:** `hd.names` as indices into a path table and a type-argument table; the standard `name` section only in debug | about −120 KB at 10k lines, a quarter of the module | the backtrace symbolizer reads the new section | the size measurements |
| 7 | **Folding in both tiers; debug at Cranelift `Speed` by default** (superseded by tiering.md, 2026-10-07: folding stays in both pipelines; the dev Cranelift setting is decided by T1) | avoids a debug tier that is both slower to run and barely faster to compile | none | S4 |
| 8 | **Fold rounds by worklist** | linear link under deep call chains | small | none |
| 9 | **B with a type-only threshold**, reusing the 4-value `multi` bound | neutral for compile time; one cascade rule instead of two | none | the runtime study |
| 10 | **C with a caller-local budget** that caps the caller's size after inlining | runtime; neutral for compile time | larger edit fan-out for generic functions that take closures | S6 |

**Fallback, not proposed now: split modules.** If S3 shows that a cache
hit costs more than 40% of a compile, wasmtime would still translate
every function on every build. The fallback is one Wasm module per hd
module group, each with its own `cwasm`, linked at instantiation through
imports. An edit then recompiles one module's `cwasm`. Its costs:
cross-module calls become import calls, monomorphized instances need an
owning module, and folding stops at module boundaries. Consider it only
after the measurement.

### 7.2 Spike 0c Additions

Each experiment gives the program, the metric and the decision rule. The
spike generates the Wasm directly with `wasm-encoder`, or uses the first
emitter if it exists. It runs wasmtime at the version the design pins,
and Node for V8.

| ID | Program | Metric | Decision rule |
| --- | --- | --- | --- |
| S1 | 50 collection element types × 10 `List` methods and 10 `Map` methods, plus 20 iterator chains, emitted twice: exact types, and A1 (`eqref` storage, casts at readers). Also 200 and 1,000 element types | Cranelift CPU at `None` and `Speed`; µs per function and per byte; type count; module load time (type registration); V8 Liftoff and TurboFan compile in Node | adopt A1 at collection if it cuts Cranelift CPU ≥ 10% at 200 types, or module load ≥ 20%, and the runtime study finds the casts within budget. Record µs per byte; if it is > 3 µs at `Speed`, revisit section 2.4 |
| S2 | a 1,000-function module, compiled with the incremental cache, then perturbed one way at a time: insert a function at the front; insert one in the middle; add one Wasm type that sorts first; add a global; add a passive data segment; renumber every site constant by +1; add a `ref.func` target before others; add an import | Cranelift cache hits per perturbation | any perturbation with < 95% hits and a type-only fix in section 3.3 gets that fix in the first release. For type indices, if < 90% hits, open the upstream question or use helper calls |
| S3 | the same module with 100% expected hits; a module from 100 to 5,000 functions; `CacheStore` with one file per entry, and with one pack per program | wall and CPU time of a full-hit `precompile`, as a share of a cold compile; per-entry I/O cost on macOS and Linux | if a hit costs > 40% of a compile, raise split modules (section 7.1) as an owner question; if per-file I/O is > 10 µs per entry, build packs (change 3) |
| S4 (superseded by T1 of tiering.md, 2026-10-07) | the `runtime` microbenchmarks and a generated unit-test suite, at `Speed`, `None`, and `Speed` with the single-pass allocator, with debug checks on | compile CPU per level; runtime ratio against release (`Speed`, no checks) | debug uses `None` only if it saves ≥ 25% compile CPU and the ratio stays ≤ 1.3x; the single-pass allocator only if its runtime cost is ≤ 1.1x and it saves ≥ 30% |
| S5 | modules with 0, 500 and 2,000 immutable globals that run `struct.new` in their initializer | instantiation time per instance with the pooling allocator; V8 instantiate time | if > 50 µs per 1,000 globals, short literals and vtables become lazy in the runtime study's terms; report it to the `unit-test-perf` budget |
| S6 | one function grown by inlining from 1 KB to 64 KB of Wasm: straight-line code, a loop with closures inlined, and a `br_table` state machine | Cranelift time per byte against function size, at both levels | set the inliner's caller cap below the size where the time per byte doubles |
| S7 | the 10k-line application model as a generated program (or the largest available hd program), release | bytes per section: code, types, `name`, `hd.sites`, `hd.lines`, data | confirm or correct section 1.4's rates; if names are > 15% of bytes, do change 6; feed the numbers to question 1 |

## 8. Open Questions For The Owner

1. **`dead-code` target.** The model puts every design at 48 to 64 KB of
   release Wasm per 1,000 lines, with 33 to 42 KB of it code. The toy
   compiler measures 58 B per line. Monomorphized Wasm GC code has a
   floor of roughly 10 to 15 B per source line for user code alone.
   **Recommendation:** after S7, re-base the target to about 40 KB per
   1,000 lines of code bytes, count metadata separately, and keep it a
   regression gate. The alternative, a dictionary-passing debug tier,
   conflicts with `release-check-cost`.
2. **Proposal A at collection (A1).** The compile side gains 43% fewer
   types, 12% fewer instances, and smaller edit fan-out. The cost is a
   cast on every element read, which the runtime study prices.
   **Recommendation:** adopt A1 at collection if the runtime study puts
   the casts inside its budget, and decide A2 on runtime data alone.
3. **`hd test --filter` builds only the selected cases.** This is faster.
   It also means a build error that only an unselected case reaches, such
   as `instantiation-too-deep`, does not appear under that filter. A plain
   `hd test` still reports it. **Recommendation:** accept that. `--filter`
   means "build and run these cases". The repro command in a failure
   report already uses `--filter`, so the case it names reproduces.

Not questions, recorded so the owner can veto them: changes 1 to 3 and 6
to 10 change no behaviour, so this study treats them as implementation
choices (mine) for the lowering pass to adopt or drop.

## Sources

- [A fast in-place interpreter for WebAssembly](https://arxiv.org/abs/2205.01183) (Titzer, 2022): translation time per input byte for Liftoff, TurboFan, BBQ and OMG, Fig. 8; output bytes, Fig. 9.
- [wasmtime#4155: incremental compilation cache in Cranelift](https://github.com/bytecodealliance/wasmtime/issues/4155): the cache key is the function's meaningful CLIF, independent of source locations and called function indices.
- [wasmtime `Config`](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html): incremental compilation, register allocator choices, the super-linear cost of the backtracking allocator.
- [wasmtime#10554](https://github.com/bytecodealliance/wasmtime/pull/10554): the single-pass allocator was disabled for a time.
- [Cranelift progress in 2022](https://bytecodealliance.org/articles/cranelift-progress-2022): regalloc2 cut compile time 10 to 20%; the e-graph mid-end costs about as much as the old passes.
- [Wasmtime 1.0 performance](https://bytecodealliance.org/articles/wasmtime-10-performance): relative compile-time gains; 5 µs instantiation of SpiderMonkey.wasm.
- [Wasmtime baseline compilation RFC](https://github.com/bytecodealliance/rfcs/blob/main/accepted/wasmtime-baseline-compilation.md): a baseline compiler is 15 to 20x faster to compile, with code 1.1 to 1.5x slower.
- [V8 Liftoff](https://v8.dev/blog/liftoff) and [the Wasm compilation pipeline](https://v8.dev/docs/wasm-compilation-pipeline).
- [rust#72013](https://github.com/rust-lang/rust/pull/72013) and [the Rust Performance Book, compile times](https://nnethercote.github.io/perf-book/compile-times.html): instance counts of `Vec::push` and `RawVec::grow`.
- [Go 1.18 generics implementation](https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md) and [PlanetScale on Go generics](https://planetscale.com/blog/generics-can-make-your-go-code-slower).
- [.NET shared generics](https://github.com/dotnet/runtime/blob/main/docs/design/coreclr/botr/shared-generics.md).
- [MoonBit announcement](https://www.moonbitlang.com/blog/first-announce).
- In this repository: `lib/std/*.hd` (the generic surface), `examples/dogfood/*.hd` (rates), [goals.md](goals.md) (targets and the toy compiler's sizes), [codegen.md](codegen.md) §11 to §13, [wasm-layout.md](wasm-layout.md), [engines-and-test-runner.md](engines-and-test-runner.md), [cache.md](cache.md), [data-structures.md](data-structures.md) §3.22 to §3.24, and `compiler/crates/hd_syntax/examples/throughput.rs`. That run measured 485 KB of std at lex 69 MB/s, skim 22 MB/s and parse 25 MB/s, so parsing 10k lines takes about 14 ms on one core and the front end is not the back half's bottleneck.
