# Iterator Performance Study: Closure, Nested, And Flat-Stage Designs

Status: deferred. Nothing here is accepted behavior. The owner decided
from this stage 1 analysis (Chaining Study CS8, 2026-09-29): the
closure-backed data `Iterator[T]` is the one public iterator type
([Iteration Protocols](../spec/06-control-flow.md#iteration-protocols)).
The flat composed-stage design C stays a later option, to revisit once
the compiler specializes and inlines closures. This record keeps the
analysis and the stage 2 benchmarks for that revisit; see
[Outcome](#outcome-2026-09-29).

The owner asked how three iterator designs compare in cost, and what a
compiler could do with each. The context is
Chaining Study CS7, which makes
`Iterator[T]` a closure-backed `data` type with method adapters. The record
reviews:

- Chaining Study CS1 and CS7: adapters are methods,
  and `Iterator[T]` holds a `step: fn() -> T?` closure;
- [Iterator Adapters](../spec/06-control-flow.md#iterator-adapters) and
  [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols);
- the prototype's lowering in
  [src/README.md](../src/README.md#compilerlibrary-boundary) and
  `lib/std/iter.hd`;
- audit findings
  [F-502](../audit/findings/F-502-bounded-calls-allocate-dictionaries.md),
  [F-504](../audit/findings/F-504-optional-carrier-allocations.md), and
  [F-505](../audit/findings/F-505-concrete-lists-rebox-elements.md).

Stage 2, run by a cheap-model agent, writes and times the programs that
[Stage 2 Benchmark Specification](#stage-2-benchmark-specification)
defines.

## Contents

1. [The Three Designs](#the-three-designs)
2. [What The Prototype Emits](#what-the-prototype-emits)
3. [Call Counts For A Three-Stage Chain](#call-counts-for-a-three-stage-chain)
4. [Allocation](#allocation)
5. [Fusion Potential](#fusion-potential)
6. [Escape Analysis](#escape-analysis)
7. [The Step Carrier](#the-step-carrier)
8. [Early Exit, Take, And Enumerate In C](#early-exit-take-and-enumerate-in-c)
9. [API Ergonomics](#api-ergonomics)
10. [Literature And Measurements](#literature-and-measurements)
11. [Ranking By Design Cost Order](#ranking-by-design-cost-order)
12. [Stage 2 Benchmark Specification](#stage-2-benchmark-specification)
13. [Preliminary Recommendation](#preliminary-recommendation)
14. [Questions For The Owner](#questions-for-the-owner)
15. [Sources](#sources)
16. [Parse Log](#parse-log)

## The Three Designs

The sketches use benchmark names: `ClosureIter` for A, a `Source` trait
for B and C, and `Flat` for C. The real std names would be `Iterator` and
`Iter`. The core shapes of all three were type-checked and run with
`hd run` at commit `b85cd43`; the sketches below are cut down from that
probe.

**(A) Closure data iterator** (CS7). Each adapter wraps a new closure
around the previous iterator. A `filter` step loops until it finds an
item.

```text
data ClosureIter[T]:
    step: fn() -> T?

impl[T] ClosureIter[T]:
    fn next(mut self) -> T?:
        (self.step)()

    fn filter(mut self, keep: fn(T) -> bool) -> mut ClosureIter[T]:
        let inner: mut ClosureIter[T] = self
        step := fn() -> T?:
            while true:
                match inner.next():
                    .Some(item) =>
                        if keep(item):
                            return .Some(item)
                    .None => return .None
            .None
        ClosureIter[T] { step: step }
```

**(B) Nested adapter types** (Rust style). Each adapter is a struct over its
source type, so a chain's type nests:
`TakeB[MapB[FilterB[Count, i32], i32, i32], i32]`.

```text
trait Source[T]:
    fn next(mut self) -> T?

data FilterB[S, T]:
    source: mut S
    keep: fn(T) -> bool

impl[S < Source[T], T] Source[T] for FilterB[S, T]:
    fn next(mut self) -> T?:
        while true:
            match self.source.next():
                .Some(item) =>
                    if (self.keep)(item):
                        return .Some(item)
                .None => return .None
        .None
```

**(C) Flat iterator with one composed stage** (the owner's favourite). The
source type stays statically known, and every adapter composes a new stage
closure around the previous one. The type stays `Flat[S, A, T]` however
long the chain. The `done` field makes `Stop` sticky.

```text
enum Step[T]:
    Skip
    Yield(T)
    Stop

data Flat[S, A, T]:
    source: mut S
    stage: fn(A) -> Step[T]
    done: bool

impl[S < Source[A], A, T] Flat[S, A, T]:
    fn next(mut self) -> T?:
        while !self.done:
            match self.source.next():
                .Some(item) =>
                    match (self.stage)(item):
                        .Yield(value) => return .Some(value)
                        .Skip => pass
                        .Stop => self.done = true
                .None => self.done = true
        .None

    fn filter(mut self, keep: fn(T) -> bool) -> mut Flat[S, A, T]:
        prev := self.stage
        stage := fn(a: A) -> Step[T]:
            match prev(a):
                .Yield(value) =>
                    if keep(value):
                        return .Yield(value)
                    .Skip
                .Skip => .Skip
                .Stop => .Stop
        Flat[S, A, T] { source: self.source, stage: stage, done: self.done }
```

Java Streams build the same shape: a flat `Stream<T>` whose terminal
operation wraps a chain of `Sink` objects and pushes each element through
them. Clojure transducers compose reducing-function transformers the same
way. Haskell's stream fusion uses the same three-way step type
(`Done | Yield a s | Skip s`); `Skip` keeps each stepper non-recursive.

## What The Prototype Emits

The prototype erases generics and passes trait dictionaries at run time; it
does not specialize ([src/README.md](../src/README.md)). A probe of the
three shapes, compiled with `hd build --wat`, shows these lowerings. Each
row was read from the probe's WAT.

| Construct | Emitted Wasm | Cost |
| --- | --- | --- |
| Closure value | `struct.new $closureN (ref.func $cK) (struct.new $envK ...)` | 2 allocations |
| Captured `let`, even when never reassigned | `struct.new $hd.cell` | 1 allocation |
| Closure call | `call_ref $sigN` with the env as first argument | 1 indirect call |
| A concrete closure stored in a generic field, such as `keep: fn(T) -> bool` | wrapped in `$adaptN`, which unboxes the argument and `call_ref`s the original | 1 allocation once; 2 indirect calls per call |
| `.Some(v)` with `v: i32` | `struct.new $hd.variant` plus `struct.new $hd.box-i32` | 2 allocations ([F-504](../audit/findings/F-504-optional-carrier-allocations.md)) |
| `.Some(v)` with `v` already erased | `struct.new $hd.variant` reusing the box | 1 allocation |
| `.None` | `struct.new $hd.variant (i32.const 0) (ref.null any)` | 1 allocation |
| User enum `Yield(v)` with a generic payload | `struct.new $eN` plus a box if `v` is a scalar | 1 or 2 allocations |
| Payload-free variant (`Skip`, `Stop`) | `global.get` | none |
| `self.source.next()` on `source: S`, `S < Source[T]` | `struct.new $traitN` wrapping the receiver, then `call_ref` through the dictionary, then a `$tadapt` that calls the concrete `next` directly | 1 allocation, 1 indirect and 1 direct call |
| A call from concrete code to a bounded impl's `next` | the caller builds the bound dictionary fresh on each call ([F-502](../audit/findings/F-502-bounded-calls-allocate-dictionaries.md)) | 1 allocation per call for `Count`; more for a nested blanket dictionary |
| Tuple `(i32, T)` | `array.new_fixed $hd.list 2` with boxed elements | 2 or 3 allocations |

Two consequences matter. First, in the prototype B and C do not get direct
source calls: an erased `S` makes `source.next()` a dictionary call. Second,
every user callback costs two `call_ref`s, one for the ABI adapter and one
for the closure. The prototype also has no optimizer: `assembleWat` in
`src/wasm.ts` runs no Binaryen passes.

A specializing compiler would change each design differently:

| Design | What specialization changes | What it cannot change alone |
| --- | --- | --- |
| A | removes boxing and the ABI adapter; `T?` can become an unboxed pair | every stage stays a `call_ref`, because the closure is a runtime value |
| B | every `next` becomes a direct call to a known function, ready to inline | `keep` and `map`'s function stay `call_ref`: hd closures have no per-closure type, unlike Rust |
| C | the source `next` becomes a direct call; boxing and adapters go | each stage stays a `call_ref` into the composed closure chain |

The B row is the key difference from Rust. Rust's `Filter<I, P>` takes the
closure's own unique type as `P`, so monomorphization also fixes the
callback. An hd function type is structural, like Swift's, so B needs the
same closure tracking as A and C to inline callbacks. Swift has a dedicated
SIL pass for this, the closure specializer.

## Call Counts For A Three-Stage Chain

The chain is `source.filter(keep).map(f).take(k)`, drained by a
`while`/`match` loop over `next()`. Counts are per source element, plus
per kept element (one that passes `keep`). `p` is the pass rate of `keep`,
so the cost per yielded element is `source / p + kept`. The last column
takes `p = 2/3`, as in workload W1 below.

With a specializing compiler, before any inlining:

| Design | Per source element | Per kept element | Indirect per yielded element (`p = 2/3`) |
| --- | --- | --- | --- |
| A | 1 direct (`next` method), 2 indirect (source step, `keep`) | 3 direct (`next` methods), 4 indirect (take step, map step, filter step, `f`) | 7 |
| B | 1 direct (`Count.next`), 1 indirect (`keep`) | 3 direct (`Take`, `Map`, `Filter` `next`), 1 indirect (`f`) | 2.5 |
| C, stages composed around the previous one | 1 direct (`Count.next`), 4 indirect (take, map and filter stages, `keep`); 5 with an identity first stage | 1 direct (`Flat.next`), 1 indirect (`f`) | 7; 8.5 with an identity first stage |
| C, stages pushed downstream (transducer order) | 1 direct, 2 indirect (filter stage, `keep`) | 1 direct, 3 indirect (map stage, `f`, take stage) | 6 |
| Hand-written loop | 0 | 0 | 0 |

In the owner's C, each new stage calls the previous stage first. So a
rejected element still enters the take and map stages, which only pass
`Skip` back. Transducers and Java `Sink`s run the other way: the filter
runs first and a rejected element goes no further. Both orders give the
same results.

In the current prototype, every indirect call above for a user callback
doubles through the ABI adapter. B's and C's direct `next` calls become
dictionary calls:

| Design | Per source element | Per kept element | Indirect per yielded element (`p = 2/3`) |
| --- | --- | --- | --- |
| A | 1 direct, 3 indirect | 3 direct, 5 indirect | 9.5 |
| B | 1 direct (`$tadapt`), 3 indirect | 3 direct (the consumer's static call, two `$tadapt`s), 4 indirect | 8.5 |
| C, composed around the previous one, with the identity first stage | 1 direct, 7 indirect | 1 direct, 2 indirect | 12.5 |

These prototype counts are predictions from the emitter code and the probe
WAT. Stage 2 counts `call_ref` in the WAT to confirm them.

The main result: **without closure inlining, C makes no fewer indirect calls
than A.** It removes direct calls, which any compiler inlines anyway. C's
gain has to come from fusion, the next section.

## Allocation

Per chain, in the prototype, from the probe WAT:

| Design | Per adapter | Per user callback | Chain of filter, map, take, plus source |
| --- | --- | --- | --- |
| A | 4: data, closure, env, cell for the captured iterator; take adds a cell for its counter | 3: closure, env, ABI adapter | about 24 |
| B | 1: the adapter struct | 3 | about 10 |
| C | 4: data, closure, env, cell for `prev`; take adds a counter cell | 3 | about 23, including an identity first stage |

Per chain cost is O(stages), paid once, so it matters only for short
chains over few elements. Per element cost is what dominates.

Per element in the prototype, `T = i32`, predicted from the emitter:

| Design | Per source element | Per kept element | Per yielded element (`p = 2/3`) |
| --- | --- | --- | --- |
| Loop | 0 | 0 | 0 |
| A | 2 (`Count`'s `.Some`: carrier and box) | 3 (filter re-wraps `.Some`; map boxes `f`'s result and wraps it) | 6 |
| B | 3 (as A, plus a trait wrap for `source.next()`) | 6 or more (as A, plus two trait wraps and a dictionary tree the consumer rebuilds on every call) | 10.5 or more |
| C | 4 (`.Some`, trait wrap, the identity stage's `Yield`) | 6 (filter `Yield`, map box and `Yield`, take `Yield`, final `.Some`, the consumer's dictionary) | 12 |

With specialization and an unboxed carrier (a scalar `T?` or `Step[T]` as
a tag and a value), every design drops to 0 allocations per element. The
chain's construction cost stays unless escape analysis removes it. So the
allocation gap between the designs is a prototype artifact, not a property
of the designs. The fix is the one F-502 and F-504 already recommend.

## Fusion Potential

Fusion means that a chain built and drained inside one function compiles
to one loop, with no calls and no allocations left: the hand-written loop.
Each design needs a different chain of compiler steps to get there.

| Step | A | B | C |
| --- | --- | --- | --- |
| 1. Inline the adapter methods at the chain site | needed | needed for construction only | needed |
| 2. Know which closure a field or cell holds (store-to-load forwarding through the adapter struct, closure, env, and cell) | needed at every level | needed only for `keep` and `f` | needed at every stage |
| 3. Turn `call_ref` of a known `ref.func` into a direct call | every level | callbacks only | every stage, callbacks |
| 4. Inline the now-direct calls | step closures contain loops (filter) | `next` bodies contain loops (filter) | stage closures are loop-free; the one loop is in `next` |
| 5. Scalar-replace the `T?` or `Step` carrier | per level | per level | per stage |

**B** fuses on types alone for the `next` calls, which is why Rust's
iterators are zero-cost. Its callbacks still need step 2.

**A** needs every step at every level, and each level's target is known
only after the level above it was resolved. Its filter closure holds a
loop; Binaryen does not inline a function with a loop by default
(`setAllowInliningFunctionsWithLoops`).

**C** needs steps 2 to 5 for the stage chain, like A. But its stages are
loop-free, straight-line functions, and the source call is already direct.
After inlining, `next` is one loop whose body is a nest of `match`es on
known `Step` values, which constant folding removes. Stream fusion depends
on the same property: `Skip` makes every stepper non-recursive, so GHC can
inline it.

**Java's C2 does this dynamically.** It inlines through `Sink.accept` and
the lambda calls when their type profiles are monomorphic, then its escape
analysis removes the per-element objects. It fails on profile pollution:
one `Sink` class serves every `map` in the program, so a busy program sees
many lambdas at that call site. The inline depth limit (`MaxInlineLevel`,
raised from 9 in JDK 14) also caps long pipelines.

**An ahead-of-time Wasm compiler** has no profile but sees the whole
program. It can do steps 1 to 5 when the chain is built and drained in one
function, which is the common case. It can also clone an adapter per chain
site, so no call site is shared. Binaryen offers the parts: inlining,
`heap2local` (scalar replacement), `gufa` and closed-world type
optimizations, and `monomorphize`. Stage 2 measures what they achieve on
this code. None of them is a compiler for hd; the hd compiler could do the
same on HIR.

**V8 at run time** adds speculative inlining of `call_ref` from call
feedback, available since the WasmGC launch. Chrome M137 added
deoptimization support for Wasm speculation. Node 24 ships V8 13.6, one
release earlier, so stage 2 sees the older mechanism. Like C2, V8's feedback
is per call site, and an adapter's closure body is one function shared by
every chain of that design. It is unclear how well it inlines here, so
stage 2 measures it.

**Summary.** With whole-program inlining and scalar replacement, all three
designs can reach the loop. C needs the least from the optimizer: no loop
inlining, a direct source call, and one flat loop. B needs the least for
`next` calls but the same closure tracking for callbacks. A needs the most.
The hand-fused C variant in stage 2 estimates the ceiling that fusing C's
stages can reach while keeping one `call_ref` and one `Step` per element.

## Escape Analysis

| Object | A | B | C | Escapes when |
| --- | --- | --- | --- | --- |
| Adapter struct | per stage | per stage | one per adapter call; each replaces the last | the chain is returned, stored, or passed to a non-inlined call |
| Closure and env | per stage | callbacks only | per stage | as above; also whenever its `call_ref` is not inlined, since the env is an argument |
| Captured-`let` cell | per captured name | none | per captured name | same as the closure |
| Per-element carrier (`T?`, `Step`) | per level | per level | per stage and at `next` | never, once the consumer unpacks it in the same function |

Scalar replacement needs inlining first: an object passed as a `call_ref`
argument escapes by definition. So escape analysis follows the fusion
steps and cannot replace them. The per-element carriers are the easy case:
they are born and consumed in adjacent frames. The prototype's cells for
never-reassigned captures are pure waste that an escape-free lowering
(capture by value) removes without any analysis.

## The Step Carrier

On Wasm GC, `Yield(v)` is a heap struct, and a scalar `v` adds a box.
`Skip` and `Stop` are shared globals. The options:

| Option | Allocation per `Yield` | Needs | Limit |
| --- | --- | --- | --- |
| Today: struct plus box | 1 or 2 | nothing | the slowest |
| Sentinels: `Skip` as `ref.null`, `Stop` as one global, `Yield(v)` as `v` itself | a box for scalars only | a reference payload; nulls are free, since hd optionals are never `null` in the prototype | scalars still box |
| Multi-value return: the stage returns `(i32 tag, payload)` | none for a reference payload; a box for an erased scalar | Wasm multi-value, which is standard, including for `call_ref` | still boxes under erasure |
| Specialization plus multi-value: `(i32, i32)` for `Step[i32]` | none | a specializing compiler | code size per instantiation |
| `i31ref` for small integers | none for values that fit 31 bits | a range check | `i32` does not fit, so a fallback box remains |
| Inline and scalar-replace | none | the fusion steps above | only where the chain is fused |

The same analysis applies to `next`'s `T?` in every design, and F-504
already proposes a shared `.None` and unboxed optionals. C has one extra
carrier per element over A: the stage's `Step` is converted into `next`'s
`T?`. A specializing compiler removes both.

## Early Exit, Take, And Enumerate In C

1. **`Stop` must be sticky.** Once a stage returns `Stop`, `next` must
   return `.None` without pulling the source again. The sketch uses a
   `done` field; Rust calls this a fused iterator.
2. **`take` pulls one element too many.** In C the stage sees an element
   only after `source.next()` consumed it. A `take(3)` stage can return
   `Stop` only on the fourth element, which the source has already lost.
   This is visible when the caller keeps using a shared `mut` source, as in
   Rust's `by_ref().take(3)`. A and B check the count before pulling.
3. **Two fixes exist.** Clojure's `take` returns the last value and the
   stop signal together (`ensure-reduced`), which in C is a fourth variant
   `Last(T)`. Java checks `cancellationRequested()` before each pull. The
   first fits C; the second needs a second closure per stage.
4. **`take(0)`** sets `done` when the adapter is built, so no element is
   pulled.
5. **`enumerate`** keeps its counter in a captured cell of its stage
   closure. The counter moves only when the inner stage yields, so its
   position in the chain is honoured. The state is hidden in closure envs,
   as in A; in B it is a visible field.
6. **Adapters that do not fit `fn(A) -> Step[T]`.** `flat_map` makes many
   outputs from one input; `zip` and `chain` read two sources; `chunks`
   must flush at the end. Java handles these with push and `Sink.end()`,
   and Clojure with a completion arity. C would need a nested source for
   `zip` and `chain`, a pending buffer for `flat_map`, and an end signal
   for `chunks`. So C covers the one-in, at-most-one-out adapters, and the
   rest fall back to nesting (B) or to closures (A).

## API Ergonomics

| Aspect | A | B | C |
| --- | --- | --- | --- |
| Return type of a chain | `mut Iterator[T]` | the full nest, such as `Take[Map[Filter[ListIter[i32], i32], i32, i32], i32]` | `mut Iter[ListIter[i32], i32, i32]` |
| Changing the source | no signature change | the signature changes | the signature changes |
| Erasing to one type | none needed | a trait value `mut Source[T]`, an indirect call per `next` | `erase()` into an A iterator, one indirect call per `next` |
| Type in an error message | short | long; Rust's are a known pain point | three arguments |
| User-defined source | a closure | a trait implementation | a trait implementation, or a closure wrapped in a `FromFn` source |
| Adapters as methods | inherent, generic ones allowed | trait default methods; generic `map[U]` needs PL1's static-only rule | inherent on `Iter`; the source trait has only `next` |
| `for` loop | `Iterable[T]` returns `mut Iterator[T]` (CS7) | the trait itself | through `erase()`, or a `for` that knows `Iter` |

hd has no opaque return type such as Rust's `impl Iterator<Item = T>`.
Adding one is new syntax, the costliest change kind. Without it, B and C
leak their internal types into every signature that returns a chain. The
cheap answer is to return an A iterator at API edges and keep C local.

## Literature And Measurements

| System | Design | What is known about its cost | Source |
| --- | --- | --- | --- |
| Rust | B, with a unique type per closure | The Rust Book's search benchmark: loop 19,620,300 ns, iterator 19,234,900 ns. `for_each` can beat a `for` loop on `Chain` through internal iteration. | [Book 13.4](https://doc.rust-lang.org/book/ch13-04-performance.html), [`for_each`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.for_each) |
| Swift | B with closures as values: `LazyFilterSequence` over `LazyMapSequence` | Fast only after generic specialization plus a closure-specialization pass | [`LazyFilterSequence`](https://developer.apple.com/documentation/swift/lazyfiltersequence), [ClosureSpecialization.swift](https://github.com/swiftlang/swift/blob/main/SwiftCompilerSources/Sources/Optimizer/FunctionPasses/ClosureSpecialization.swift) |
| Java Streams | C: a flat `Stream<T>` over a `Sink` chain; `limit` checks cancellation before each pull | Kiselyov et al.: Java 8 streams "are still an order of magnitude slower than hand-written loops" on their benchmarks. Relies on C2 inlining. | [Stream Fusion, to Completeness](https://arxiv.org/abs/1612.06668), [`AbstractPipeline`](https://github.com/openjdk/jdk/blob/master/src/java.base/share/classes/java/util/stream/AbstractPipeline.java), [JDK-8234863](https://bugs.openjdk.org/browse/JDK-8234863) |
| Clojure transducers | C: composed transformers; `reduced` stops early; `take` keeps a `volatile!` counter | No intermediate sequences; each step is a function call | [Transducers](https://clojure.org/reference/transducers), [`take` in core.clj](https://github.com/clojure/clojure/blob/master/src/clj/clojure/core.clj) |
| Haskell stream fusion | a `Step` type with `Skip`; GHC inlines and specializes constructors | Removes the intermediate lists and the `Step` values when fusion fires | [Coutts et al. 2007](https://www.cs.tufts.edu/~nr/cs257/archive/duncan-coutts/stream-fusion.pdf) |
| Staged streams (strymonas) | code generation from the pipeline | "Hand-written-like code, but automatically"; the fusion ceiling | [Kiselyov et al. 2017](https://arxiv.org/abs/1612.06668) |
| Go 1.23 `iter.Seq` | push: `func(yield func(V) bool)` | Matches a hand loop only when the compiler inlines the iterator, inlines the loop body, and devirtualizes `yield`; else one indirect call per element | [Range functions blog](https://go.dev/blog/range-functions), [Rangefunc wiki](https://go.dev/wiki/RangefuncExperiment) |
| Kotlin | `Sequence` objects per stage (B-like with virtual calls); `Iterable` ops are inline functions building lists | Sequence laziness "adds some overhead which may be significant when processing smaller collections or doing simpler computations" | [Sequences](https://kotlinlang.org/docs/sequences.html) |
| JS iterator helpers | one helper object per stage over the protocol | Each `next` returns an `{value, done}` result object, the analogue of `Yield(v)` | [V8 iterator helpers](https://v8.dev/features/iterator-helpers), [proposal](https://github.com/tc39/proposal-iterator-helpers) |
| Gleam `yielder` | A: `Yielder(continuation: fn() -> Action(element))` | `Continue(element, fn() -> Action(element))` allocates a record and a closure per element | [yielder.gleam](https://github.com/gleam-lang/yielder/blob/main/src/gleam/yielder.gleam) |
| Java, Scala, C#, F# compared | several | Lambda-based pipelines vary widely by implementation maturity; optimizers such as ScalaBlitz and LinqOptimizer help greatly | [Clash of the Lambdas](https://arxiv.org/abs/1406.6631) |
| V8 WasmGC | runtime | `call_ref` inlining from feedback; Chrome M137 adds Wasm deopts and reports a 1.59x average on Dart microbenchmarks | [Speculative optimizations](https://v8.dev/blog/wasm-speculative-optimizations), [WasmGC porting](https://v8.dev/blog/wasm-gc-porting) |
| Binaryen | ahead of time | `heap2local` scalar-replaces non-escaping allocations; inlining skips functions with loops by default | [Heap2Local.cpp](https://github.com/WebAssembly/binaryen/blob/main/src/passes/Heap2Local.cpp), [binaryen.js API](https://github.com/WebAssembly/binaryen/blob/main/src/js/binaryen.js-post.js) |
| OCaml Flambda, MLton | ahead of time | Inline known closures, and MLton defunctionalizes the whole program, the AOT route to fusing A and C | [Flambda](https://ocaml.org/manual/5.3/flambda.html), [MLton ClosureConvert](http://mlton.org/ClosureConvert) |

Three lessons stand out:

1. Every fast design either fixes the callbacks by type (Rust), or
   inlines them by profile (C2, V8), or by whole-program analysis
   (Flambda, MLton, GHC).
2. The flat Java-style shape is not fast by itself. Its speed comes from
   the JIT, and profile pollution limits it.
3. A per-element result object (JS, Gleam, hd's `T?`) costs an allocation
   unless the compiler removes it.

## Ranking By Design Cost Order

All three are core library code (kind 4 of the
[Design Cost Order](../AGENTS.md#design-cost-order)). Specialization and
fusion are compiler implementation work that changes no language rule.

| Design | Changes | Costliest kind |
| --- | --- | --- |
| A | none beyond CS7 | 4 |
| B | a trait with generic default adapters, which needs PL1's static-only rule back | 2, a semantic rule exception |
| C | a `Step` enum and a `Source` trait in std; `Iterable` still returns an A iterator, or `for` learns `Iter` | 4, or 2 if `for` special-cases `Iter` |

## Stage 2 Benchmark Specification

Stage 2 is run by a cheap-model agent under
[Writing hd Code](../AGENTS.md#writing-hd-code-model-choice-and-a-feedback-log).
It logs every hd mistake in [audit/hd-writing-log.md](../audit/hd-writing-log.md).
It must not change `src/` or `lib/std/`.

### Files

| Path | Content |
| --- | --- |
| `audit/bench/iterator/loop.hd` | hand-written `while` loops, the baseline |
| `audit/bench/iterator/a-closure.hd` | design A |
| `audit/bench/iterator/b-nested.hd` | design B |
| `audit/bench/iterator/c-flat.hd` | design C, stages composed around the previous one |
| `audit/bench/iterator/c-fused.hd` | design C with one hand-written stage closure per workload |
| `audit/scripts/arch/iter-bench.ts` | the timing harness |
| `audit/evidence/iterator-perf/results.md` | raw harness output |

### Shared Rules For Every Program

1. Each file is standalone. Do not use the prelude `Iterator`, `range`, or
   `List`. The std versions are built on lists, and `range` builds a whole
   list first, so they would measure list boxing instead.
2. Use the names `ClosureIter`, `Source`, `Count`, `FilterB`, `MapB`,
   `TakeB`, `EnumB`, `Flat`, and `Step`. They avoid prelude names.
3. The source is `data Count: at: i32, end: i32` with
   `impl Source[i32] for Count`, yielding `0` to `end - 1`. B, C, and
   `c-fused` use it directly. A wraps it once: `ClosureIter[i32] { step: fn() -> i32?: counter.next() }`.
4. The consumer is the same in every design: a `while true:` loop matching
   `next()` on `.Some(v)` and `.None`. It keeps
   `total = (total + v) % 1000003`.
5. Each workload is a top-level function `fn wN(n: i32) -> i32` returning
   `total`. The prototype exports every top-level function under its name,
   so the harness calls it directly.
6. Each file ends with
   `pub fn main() -> void $ Console:` that prints `w1(1000)`, `w2_4(1000)`,
   and `w3(1000)` on one line each. `hd run` must print the same three
   lines for all five files.
7. Adapters are methods: `filter`, `map[U]`, `take`, `enumerate`. In B they
   are inherent constructors or free functions returning the nested
   struct, because a trait default `map[U]` is not dynamically safe.
8. C builds its first stage in `Flat::from(source)` as the identity
   `fn(a: A) -> Step[A]: .Yield(a)`. Its `take` returns `.Stop` on the
   element after the `k`th; the extra pull is harmless here. It checks
   `done` before pulling, as in the sketch.
9. `c-fused` uses the same `Flat`, `Step`, and `next` as `c-flat`, but
   builds each chain as one hand-written stage closure that does all its
   stages inline, with `take`'s counter in the same closure.

### Workloads

| Name | Chain | Checks |
| --- | --- | --- |
| `w1` | `filter(fn(x): x % 3 != 0)`, `map(fn(x): x * 2 + 1)`, `take(n / 2)` | the three-stage chain; early exit at three quarters of the source |
| `w2_1`, `w2_2`, `w2_4`, `w2_8` | 1, 2, 4, or 8 copies of `map(fn(x): x + 1)`, written out | cost per stage, as a slope |
| `w3` | `enumerate()`, `filter(fn(p): p._0 % 2 == 0)`, `map(fn(p): p._1 * 3)` | adapter state and tuple elements |

Write each chain out in full, since B's type changes with every stage.
In `loop.hd`, each workload is the equivalent `while` loop over a counter.
In `c-fused.hd`, `w2_d` is one stage adding `d`, and `w3` keeps the index
in the stage's captured counter.

### Harness

`audit/scripts/arch/iter-bench.ts` imports `audit/scripts/arch/bench-lib.ts`
and times in process, which removes the roughly 0.5 s compile and Binaryen
load of `hd run`.

1. Compile each file once with `compilePhased`.
2. Build three tiers from its WAT:
   - `dev`: the compiled bytes;
   - `o2`: `optimize(wat, 2)`;
   - `o3cw`: `setClosedWorld(true)`, `setOptimizeLevel(3)`,
     `setAllowInliningFunctionsWithLoops(true)`, `module.optimize()`, then
     `module.runPasses(["gufa-optimizing", "heap2local", "inlining-optimizing"])`,
     then `module.optimize()` again and `validate()`. If a pass is
     rejected, drop it and record which.
3. Instantiate each tier with `instantiateWithBytes` and fetch the
   workload exports with `exported`.
4. Sizes: `w1` at `n` = 10,000, 100,000, and 1,000,000; `w2_*` and `w3` at
   1,000,000. If one `dev` call takes over 2 s, use 100,000 and say so.
5. Per program, tier, workload, and size: 5 warm-up calls, then 15 samples
   interleaved across the five programs, so machine load hits all alike.
6. All five programs and three tiers must return the same value for each
   workload and size. Abort on a mismatch.
7. Static counts: for each program and tier, count `call_ref`, `call `,
   `struct.new`, and `array.new` in the whole module, and in the functions
   that the `w1` loop reaches. Use `functionBodies` and `countMatches`.
8. Optional allocation proxy: run `w1` at 1,000,000 once per program and
   tier in a child `node --trace-gc` process, and count `Scavenge` lines.

### Report

Append a section `Stage 2 Results` to this file, with the raw output in
`audit/evidence/iterator-perf/results.md`. Record the commit, Node and V8
versions, the CPU, and the load average at start.

| Table | Rows | Columns |
| --- | --- | --- |
| Time | program by workload | per tier: min and median ns per source element (`ms * 1e6 / n`), and the ratio of the min to `loop.hd`'s min |
| Stage slope | program by tier | ns per added `map` stage: least squares over `w2_1` to `w2_8` |
| Fusion ceiling | tier | `c-flat` over `c-fused`, and `c-fused` over `loop` |
| Static counts | program by tier | `call_ref`, `call`, `struct.new`, `array.new` |
| Predictions | each prototype prediction in this record | confirmed or refuted, with the count |

Min is the primary estimator, as in the audit's earlier benchmarks.

### Predictions To Test

| Prediction | Where |
| --- | --- |
| In `dev`, B and C allocate more per element than A, through trait wraps and per-call dictionaries | [Allocation](#allocation) |
| In `dev`, C makes as many indirect calls as A or more | [Call Counts](#call-counts-for-a-three-stage-chain) |
| `o2` keeps the ratios between designs, as the audit found for erasure | [audit REPORT](../audit/REPORT.md) |
| `o3cw` helps C and `c-fused` more than A, since their stages are loop-free | [Fusion Potential](#fusion-potential) |
| `c-fused` is well faster than `c-flat` at every tier | [Fusion Potential](#fusion-potential) |
| Every design stays far from `loop` while `T?` and `Step` are boxed | [The Step Carrier](#the-step-carrier) |

## Preliminary Recommendation

**Recommendation (preliminary, before any measurement):** keep A, as CS7
decided, as the one public iterator type. Treat C as a candidate for later,
tied to a specializing compiler.

The reasons:

1. In the prototype as it is, C costs at least as much as A. Its source
   call goes through a dictionary, and each element pays an extra `Step`
   and a conversion to `T?`.
2. With specialization but without closure inlining, C makes about as many
   indirect calls as A.
3. C's real advantage is fusion: loop-free stages and a direct source call
   make it the easiest shape for an inliner to flatten. That advantage
   exists only once the compiler specializes, inlines, and scalar-replaces.
4. B's advantage in Rust comes from per-closure types, which hd does not
   have, and B brings back PL1's static-only rule.
5. The largest per-element costs in every design are boxed `T?` and
   per-call dictionaries (F-502, F-504). They are design-neutral and
   should be fixed first.

If stage 2 shows that `o3cw` fuses C and not A, a later design could keep
A's API and add C as a fused-chain type. Such a type would erase to A at
API edges, as Kotlin keeps `Sequence` beside `Iterable`.

## Questions For The Owner

Questions 1, 2, and 5 are decided by Chaining Study CS8 (2026-09-29):
A stays the one public iterator type, C is a later option tied to a
specializing compiler, and stage 2 ran as specified. Questions 3 and 4
wait until C is pursued.

### 3. How does C's `take` stop without pulling one element too many?

Effect: `take(3)` on a shared `mut` source loses the fourth element.

- **A.** Add `Last(T)` to `Step`, like Clojure's `ensure-reduced`.
- **B.** Give each stage a pre-pull check, like Java's
  `cancellationRequested`.
- **C.** Accept the extra pull and document it.

**Recommendation:** A, if C is pursued.

```text
enum Step[T]:
    Skip
    Yield(T)
    Last(T)
    Stop
```

### 4. What do `zip`, `chain`, and `flat_map` return in C?

Effect: they do not fit a one-in, at-most-one-out stage.

- **A.** Nest sources for them, B-style, inside a flat `Iter`.
- **B.** Fall back to A iterators for them.

**Recommendation:** B, since it adds no new shapes.

```text
fn pairs(xs: List[i32], ys: List[i32]) -> mut Iterator[(i32, i32)]:
    xs.iter().zip(ys.iter())
```

## Sources

- hd: [Iteration Protocols](../spec/06-control-flow.md#iteration-protocols),
  [src/README.md](../src/README.md#compilerlibrary-boundary),
  `lib/std/iter.hd`, `src/emitter/function-body.ts` (`closure`,
  `closure-call`, `trait-bound`, `trait-call`, `enum`),
  `src/emitter/context.ts` (`adaptCallable`, `storeErased`),
  `src/emitter/scalars.ts` (`boxScalar`), `src/emitter/iterator.ts`,
  `src/wasm.ts`, `audit/scripts/arch/bench-lib.ts`,
  [audit REPORT](../audit/REPORT.md)
- Rust: [Book 13.4](https://doc.rust-lang.org/book/ch13-04-performance.html),
  [`Iterator`](https://doc.rust-lang.org/std/iter/trait.Iterator.html)
- Swift: [`LazyFilterSequence`](https://developer.apple.com/documentation/swift/lazyfiltersequence),
  [ClosureSpecialization.swift](https://github.com/swiftlang/swift/blob/main/SwiftCompilerSources/Sources/Optimizer/FunctionPasses/ClosureSpecialization.swift)
- Java: [`java.util.stream`](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/stream/package-summary.html),
  [`AbstractPipeline`](https://github.com/openjdk/jdk/blob/master/src/java.base/share/classes/java/util/stream/AbstractPipeline.java),
  [JDK-8234863](https://bugs.openjdk.org/browse/JDK-8234863),
  [Clash of the Lambdas](https://arxiv.org/abs/1406.6631)
- Clojure: [Transducers](https://clojure.org/reference/transducers),
  [core.clj](https://github.com/clojure/clojure/blob/master/src/clj/clojure/core.clj)
- Haskell: [Stream Fusion (Coutts, Leshchinskiy, Stewart 2007)](https://www.cs.tufts.edu/~nr/cs257/archive/duncan-coutts/stream-fusion.pdf)
- Staged streams: [Stream Fusion, to Completeness (Kiselyov et al. 2017)](https://arxiv.org/abs/1612.06668)
- Go: [Range Over Function Types](https://go.dev/blog/range-functions),
  [Rangefunc wiki](https://go.dev/wiki/RangefuncExperiment)
- Kotlin: [Sequences](https://kotlinlang.org/docs/sequences.html)
- JavaScript: [Iterator helpers](https://v8.dev/features/iterator-helpers),
  [proposal](https://github.com/tc39/proposal-iterator-helpers)
- Gleam: [yielder.gleam](https://github.com/gleam-lang/yielder/blob/main/src/gleam/yielder.gleam)
- V8: [Speculative optimizations for WebAssembly](https://v8.dev/blog/wasm-speculative-optimizations),
  [WasmGC porting](https://v8.dev/blog/wasm-gc-porting)
- Binaryen: [Heap2Local.cpp](https://github.com/WebAssembly/binaryen/blob/main/src/passes/Heap2Local.cpp),
  [binaryen.js API](https://github.com/WebAssembly/binaryen/blob/main/src/js/binaryen.js-post.js)
- OCaml and MLton: [Flambda](https://ocaml.org/manual/5.3/flambda.html),
  [ClosureConvert](http://mlton.org/ClosureConvert)

## Parse Log

Every `text` block was parsed with the reference parser (`parseSource` in
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts)) on
2026-09-29. Parsing checks syntax only; no block is claimed to type-check
here. Separately, one probe file with the design shapes of blocks 1 to 3
type-checked and ran in the prototype at commit `b85cd43`. It printed the
expected sum 27 for `n = 10`.

| Block | Section | Result |
| --- | --- | --- |
| 1 | The Three Designs (A) | parses |
| 2 | The Three Designs (B) | parses |
| 3 | The Three Designs (C) | parses |
| 4 | 3. How does C's `take` stop without pulling one element too many? | parses |
| 5 | 4. What do `zip`, `chain`, and `flat_map` return in C? | parses |

## Outcome (2026-09-29)

The owner decided from the stage-1 analysis: CS7's closure-backed
`Iterator` stays (Chaining Study CS8). **The stage 2 run of 2026-09-29, at
commit `2c93085c`, produced no valid measurements**, so its tables are
left to git history. None of the five programs contains a closure; each
design fell back to plain loops because the prototype can't capture a
`mut` value in a closure or infer tuple element types across closure
boundaries (audit/hd-writing-log.md). Only the dev tier was run, on a
loaded machine. Rerun the benchmarks once those gaps are fixed.
