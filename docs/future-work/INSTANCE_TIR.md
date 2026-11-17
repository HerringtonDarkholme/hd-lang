# Instance TIR: One Instance Form Between hd_mono And Emission

Status: accepted design (owner, 2026-11-17: "Approve, build it"); domain:
compiler internals; step 1 built (#2101: `Phase` with `Checked` and
`Instance`, `verify_instance` in the driver's `Env::mark_instance`). From [#1780](https://github.com/HerringtonDarkholme/hd-lang/issues/1780),
option B of [review-2026-11-12 §14](../audit/review-2026-11-12.md#generic-or-instance-tir),
and [tir-v2.md "As built"](../compiler/tir-v2.md#as-built-2026-11-13).
D01 is §14's per-function code cache ("direction 1", #1783).

## The proposal

Generic TIR stays what the checker emits and what the `check` cache
stores. `hd_mono` builds **one instance TIR per instance, once**: types
substituted, impls chosen, generic local functions instantiated, layout
known. Instance TIR is the only input of the ownership passes, the
optimizer, the emitter and the code cache. It uses the same columns as
generic TIR plus a phase mark, and a verifier rejects any generic form
left in it. The emitter stops substituting, and the 4,096-leaf generic
fallback is deleted.

```
            check cache                     code cache (D01, #2028)
               |                                  |
source -> [check] -> generic TIR -> [hd_mono: collect + instantiate] -> instance TIR
                                              |                            |
                                   key = instance_key(item, sub, args)     v
                                   (exists today, hd_mono/layout.rs)  [own] -> [opt] -> [emit: lower only]
```

The instance key is the one `hd_mono` already computes for collection
(`instance_key = H("inst", item path, sub, [canon(arg) or class])`,
`crates/hd_mono/src/layout.rs:515`). It plays the part of Swift's
mangled specialized name: it deduplicates instances in a run, and it is
the code cache's key across runs.

## Open questions

1. Delete the 4,096-leaf fallback, so the passes ask layout through a
   memo per type id instead? **Recommended: yes**. Swift keeps a runnable
   generic fallback; hd has no runtime generics.
2. Key instances by exact substitution first, and share bodies by layout
   shape (Go's GC shape) only after a measurement shows duplication?
   **Recommended: exact first**; the A1 `REF` class already shares
   reference arguments.
3. Does an instance record the facts it reads (impls chosen, layouts,
   callee summaries) as its cache key, replacing the one program-wide
   declarations hash of #2028? **Recommended: yes**, as rustc's query
   dependency list.
4. A dependent package builds its instances from the dependency's generic
   TIR (owner, 2026-11-16: interface bodies are required). Keep instances
   in the client's code cache only, never in the dependency's?
   **Recommended: yes**, as Swift's `@inlinable` client copy.

## Problems it solves

**1. The emitter substitutes every instruction, generic or not.** On the
19 bench programs, a dev build calls `hd_mono::subst` 165,159 times for
100,787 emitted instructions (1.64 per instruction). 91% of those calls
are in bodies with no type arguments, where `subst` can only open an
opaque result or normalize a projection. The stage corpus agrees: 1.67
calls per instruction, 95.5% outside generic instances. Proposed:
`instantiate` does both once per distinct type, and the emitter reads
`body.ty[i]`.

**2. A generic local function is decided in the emitter.** Its body stays
generic inside each instance (`sub_flags::GENERIC`), so the passes see
`U` as "maybe counted", and the emitter adds the retain once it knows the
layout. No verifier sees that retain.

```text
fn pair_of[T](x: T) -> (T, T):
    fn dup[U](u: U) -> (U, U): (u, u)
    dup(x)

pub fn main() -> void $ Console:
    let (a, _) = pair_of("s")
    let (n, _) = pair_of(3)
    println(a)
    println("{n}")
```

Today each `pair_of` instance keeps `dup` generic: the probe build counts
2 generic definitions and 2 instantiations that only the emitter
resolves. Proposed: `pair_of[String]`'s instance body holds a sub-body
`dup[String]`, and `pair_of[i32]` one for `dup[i32]` with no counts. The
passes insert the `String` retain, and `verify_owned` checks it.

**3. Past 4,096 leaves the generic body serves.** A type that doubles
per alias (`type D1 = (D0, D0)`, 13 levels) gets 5 fallback instances in
a 72-instance build (probe below). Those instances are owned as generic
code, and the emitter decides their counts. Proposed: the instance is
built like any other. Layout questions cost one memo probe per distinct
type, not per leaf, so no limit is needed.

**4. Dev builds re-own every generic instance.** #2028 caches package
bodies, but not instances: in its K = 25 program about 100 std iterator
instances are owned again on a build with no edit. Proposed: an instance's
owned form is cached under its instance key plus the hashes it read.

## How it works

- **Phase mark.** `Body` gains `phase: Checked | Instance | Owned`. A
  non-generic item's checked body becomes its instance body by setting the
  mark: no copy (§14: "A non-generic function costs nothing extra").
- **One builder.** `hd_mono::instantiate(item, sub, args) -> Arc<Body>`,
  memoized per run in the `InstanceTable` by `InstId`. It clones the
  generic body once, maps `ty`, `local_ty` and `consts` through `subst`
  (memoized per distinct `Ty`), rechooses trait calls (today's
  `inline::rechoose`), and instantiates each generic local function at
  each `Closure` that instantiates it, as a new sub-body, deduplicated by
  `(sub, args)`. This is today's driver code (`own.rs:2627`), moved.
- **Folds and the inliner keep their place.** A dev build folds and
  inlines each generic body once and instantiates the result, as today
  (`Env::generic_inlined`). A release build instantiates first, then folds
  and inlines per instance, as today. Only the builder moves.
- **Layout.** Layout facts stay a per-run memo keyed by type id (§14 T4):
  `counted`, leaf count, pass mode. An instance body holds only concrete
  types, so each question is one probe. Layout is "attached" by being one
  probe away; the body stores no copy of it.
- **Verifier.** `verify_instance(body)` runs at the end of `instantiate`,
  in release compilers on CI too. It rejects a `ty`, `local_ty` or const
  type that contains a type parameter, a `TraitMethod` callee whose
  choice is not an impl or builtin, and a sub-body with
  `sub_flags::GENERIC`. One pass over the columns: O(instructions).
- **Passes and emission.** `hd_tir::own::run`, the inliner and the
  emitter take instance bodies only (`phase == Instance`). `Em::sub`,
  `Em::ty`'s substitution memo and the emitter's body-type `subst` calls
  go.
- **Cache key.** `H(instance_key, generic body TIR hash, reads)`, where
  `reads` lists `(query, answer hash)` for each impl chosen, each callee
  summary and each layout of a type outside the body's own package. A
  warm build probes this key per reached instance and decodes on a hit.

## Cost

Bottom-up, from the counters below (Appendix D), 19 bench programs, dev:

| Component | Today | Proposed |
| --- | --- | --- |
| Instance bodies built | 206, 8,729 instructions substituted once each (driver) | the same 206, built in `hd_mono` |
| Emitter substitutions | 165,159 `subst` calls, 1.64 per emitted instruction | 0 |
| Generic local functions | stay generic; emitter decides | one sub-body per (sub, args) |
| Leaf fallback | 5 of 72 instances at depth 13 | 0; layout memo per type id |
| Memory | driver holds each owned instance (`Run::instances`) | the same bodies, held by `hd_mono` |
| Warm dev build, no edit | re-owns every generic instance | one key probe and decode per instance |
| Code removed | `Em::sub`, `inst_ty`, `instruction_types` (dead), `MAX_INSTANCE_LEAVES`, `instance_leaves`, 19 generic-local-function checks after collection | |
| Code added | | `instantiate` (moved, about 200 lines), `verify_instance` (about 80), phase column |
| Runtime, Wasm size | unchanged for the move; fallback instances get exact counts | |
| Web compiler gzip | neutral: code moves, the emitter shrinks | |

Per run: instantiation is O(total instance body size) plus one memo probe
per distinct type; a generic local function adds one sub-body per
distinct instantiation. A warm build with nothing changed probes one key
per reached instance (O(instances), no body work). An edit to k generic
bodies rebuilds the instances of those k bodies and the instances whose
recorded reads changed, nothing else.

## Migration order

Each step lands green on its own and names what it deletes.

| Step | Change | Deletes | Test |
| --- | --- | --- | --- |
| 1 | `phase` column; `verify_instance` on every driver-built instance body | nothing | stage case: a generic body marked `Instance` is rejected |
| 2 | Layout memo per type id (§14 T4) for every pass's layout question | `MAX_INSTANCE_LEAVES`, `instance_leaves`, both leaf checks (`own.rs:2628`, `:2981`) | counted test, depth 13 and 130 via aliases: layout probes linear in depth |
| 3 | `hd_mono::instantiate` replaces `Env::inlined_instance`'s substitution | the driver's substitute-and-rechoose block | `own` view unchanged on the stage corpus |
| 4 | Instantiate generic local functions in instance bodies | 19 `sub_flags::GENERIC` and `is_instantiation` checks in `hd_tir::own`, the inliner, `hd_opt` and the emitter (the checker's stay) | `generic-local-fn-run` cases plus a two-instantiation case |
| 5 | Emitter lowers instance TIR only | `Em::sub`, `Em::inst_ty`, `hd_mono::instruction_types`, body-type `subst` in `emit/*.rs` | `wat` stage cases byte-identical |
| 6 | Code cache by instance key plus recorded reads (D01, extends #2028) | the one program-wide `program_api_hash` in `package_owned_key` (question 3) | rebuild test: K and 10K instances, no edit, zero passes run |
| 7 | Drop flags as TIR locals (#1420), then counts as statements (§14 T3) | `emit/drop_flags.rs` except cell layout | #1420's done check |

## Appendix A. Stress test

| Case | What happens | Design answer |
| --- | --- | --- |
| Doubling type, 20 alias levels | 2^20 leaves; today fallback, emitter counts | layout memo is O(20) probes; the instance is built and owned |
| Doubling type over `String`, depth 13 | 8,192 counted leaves inline; the emitter writes a retain per leaf, today and after | unchanged by this design; per-type copy glue (rustc drop glue) is a separate question |
| Polymorphic recursion, `f[T]` calls `f[(T, T)]` | collection stops (`instantiation-too-deep`), before any body is built | unchanged: `instantiate` runs only on collected instances |
| Generic local function used at two types in one instance | today one generic sub-body, emitter splits | two sub-bodies, keyed `(sub, args)` |
| Generic local function capturing the outer `T` | capture types are the outer instance's | substituted with the outer args before the inner ones |
| Same-shape recursion through erasure (`instantiation-depth` case) | A1 `REF` class keys both | one instance per class key; its body types must name the class, not a parameter |
| 500 closure types through one `map[F]` | 500 instances | linear; question 2 decides shape sharing |
| Bodies `hd_mono` already builds: exclusive copies, known-closure specializations, adapter calls (`instance_bodies`, `lib.rs:518`) | a fourth store of instance bodies, beside the driver's | `instantiate` is their one builder and `InstanceTable` their one store |
| Closure sub-bodies of a generic instance | each is emitted with the instance's arguments (215 emitted generic functions for 206 built) | they are sub-bodies of the one instance body |
| Two packages instantiate one std generic at one type | each builds it | same key, so the second is a cache hit on one machine |
| A dependency's generic body edit | today re-owns every package body (one declarations hash) | only instances whose generic hash or reads changed |

What it found: an instance keyed by the A1 `REF` class serves several
source types, so its body must be written over the class (a marker type)
for the verifier to accept it; step 3 must check today's bodies do that.
`hd_mono` already keeps a fourth instance-body store (`instance_bodies`).
Release clones every body it owns, non-generic ones too (1,470 clones
for 1,220 emitted functions); step 3 makes a non-generic body a mark, not
a clone. Inlining stays per generic body in dev, or dev's inliner cost
would grow with instances.

## Appendix B. Prior art, with code

**rustc**: MIR is generic; `Instance` names an item plus substitution,
and codegen monomorphizes per instruction as it lowers to LLVM.

```rust
// rustc_codegen_ssa/src/mir/mod.rs (abbreviated)
pub fn monomorphize<T>(&self, value: T) -> T {
    self.instance.instantiate_mir_and_normalize_erasing_regions(
        self.cx.tcx(), self.cx.typing_env(), ty::EarlyBinder::bind(value))
}
```

Rust can do this because LLVM optimizes each instance afterwards. hd has
no optimizer after emission, so it does not take this split.
[source](https://github.com/rust-lang/rust/blob/master/compiler/rustc_codegen_ssa/src/mir/mod.rs)

**Swift** (owner asked). The frontend emits generic SIL; the mandatory
passes (diagnostics, OSSA ownership verification) run on it. In the
performance pipeline, `GenericSpecializer` clones one specialized
function per (function, substitution), deduplicated by its mangled name;
ARC optimization and inlining then run on the specialized copy. IRGen
lowers only the SIL functions that exist and never substitutes.

```swift
@inlinable public func dup<T>(_ x: T) -> (T, T) { (x, x) }   // body serialized into the interface
@_specialize(where T == Int)                                 // named specialization in the defining module
public func sum<T: Numeric>(_ xs: [T]) -> T { xs.reduce(0, +) }
// SIL after specialization (name illustrative):
// sil shared @$s4main3dupyx_xtxlFSi_Tg5 : $@convention(thin) (Int) -> (Int, Int)
```

Cross-module, `@inlinable` serializes the SIL body, so the client
specializes and holds the copy. `@_specialize` emits named
specializations with a runtime type check in the generic entry. Generic
SIL stays runnable through type metadata and witness tables, so
specialization is only an optimization.
[cross-module pitch](https://forums.swift.org/t/pitch-cross-module-inlining-and-specialization/6765),
[@_specialize](https://lists.swift.org/pipermail/swift-evolution/Week-of-Mon-20171002/040225.html),
[EagerSpecializer.cpp](https://sources.debian.org/src/swiftlang/6.2.3-1/swift/lib/SILOptimizer/Transforms/EagerSpecializer.cpp),
[OSSA](https://github.com/swiftlang/swift/blob/main/docs/SIL/Ownership.md).

Swift's ARC optimizer gains most on generic code after specialization: a
trivial `T` has ownership `none`, so every copy and destroy vanishes,
and a class `T` exposes retain/release pairs to pair and sink. hd's
passes on instance TIR get the same: `pair_of[i32]` has no counts at all.

hd takes: one instance body per (item, substitution), keyed like a
mangled name, which is also the D01/#2028 cache key; ownership and
emission on the instance; the client building instances from interface
bodies. hd today specializes in the emitter, per instruction, which is
the analogue of specializing in IRGen; the proposal moves it to `hd_mono`,
Swift's split. hd does not take the runnable generic fallback: it has no
runtime generics, so the 4,096-leaf fallback is deleted, not kept.

**Go**: generic IR, stenciled per GC shape with a dictionary argument
before SSA.

```go
// cmd/compile/internal/noder: one stencil per shape
func Sum[T int | int64](xs []T) T   // Sum[go.shape.int] serves int and named int types
```

hd's A1 `REF` class is the same idea for references. Wider shape sharing
is question 2. [design](https://github.com/golang/proposal/blob/master/design/generics-implementation-dictionaries-go1.18.md)

**Zig**: Sema analyzes each generic call's comptime arguments into a new
function instance in the InternPool; AIR, and every backend, see only
instances. [Sema.zig](https://github.com/ziglang/zig/blob/master/src/Sema.zig)

**Cranelift and LLVM** see only instances: a `Function` has concrete
types, and generics are the frontend's job.

## Appendix C. Alternatives considered

- **Option A of §14: keep generic TIR, verify counts at link.** Keeps the
  emitter's substitution and the fallback, and gives the code cache no
  unit. The owner's correction of 2026-11-12 asks for B.
- **Rust's drop glue: decide drops in generic TIR, resolve per type at
  emission.** Needs an optimizer after emission to remove scalar glue;
  hd has none.
- **A new instance IR.** Doubles the passes; §14 asks for the same
  columns with a phase mark.
- **Swift's generic fallback.** Needs runtime type metadata; hd has none.

## Appendix D. Today in hd

The three substitution sites, measured with temporary counters (not
committed) on `2ebf4a5b1`: `hd build --jobs 1`, a fresh `HD_CACHE` per
program for the 19 bench programs, one warm cache for the stage corpus.

| Site | Code | Bench, dev | Bench, release |
| --- | --- | --- | --- |
| Driver: clone and substitute per instance | `Env::inlined_instance`, `own.rs:2627` | 206 generic instances, 8,729 instructions | 174, 5,768; 1,470 clones in all |
| Emitter: per-instruction substitution | `Em::sub`, `Em::ty`, `emit.rs:1834`, `:1895` | 165,159 calls (14,985 in generic instances) for 100,787 instructions | 178,493 for 105,643 |
| Emitter: memo misses (`inst_ty`) | `emit.rs:1895` | 95,204 | 99,621 |
| Generic local functions | `sub_flags::GENERIC`, `closure.rs:646` | 0 | 0 |
| Leaf fallback | `MAX_INSTANCE_LEAVES`, `own.rs:199` | 0 | 0 |

The stage corpus: the 416 `hd_driver` and `hd_wasm` stage cases with a
`main`, built as packages in dev; 248 emit code (the rest stop at a
diagnostic by design). One case,
`allocation/glues-precede-a-large-function-table.hd`, has 33,793 instances
and is shown alone.

| Site | Stage corpus, 247 cases | The 33,793-instance case |
| --- | --- | --- |
| Driver: generic instances built | 1,612, 43,061 instructions | 33,793, 44,107 instructions |
| Emitter: `subst` calls | 2,007,256 for 1,204,735 instructions (1.67 each; 95.5% in bodies without type arguments) | 262,422 for 49,718 (5.3 each: signatures of tiny bodies) |
| Emitter: memo misses | 1,145,059 | 15,113 |
| Generic local functions | 1 definition, 1 instantiation, in one case (`features/a-generic-local-struct-instantiates-every-field`) | 0 |
| Leaf fallback | 0 | 0 |

So generic local functions and the leaf fallback are rare in today's
tests, which is why their emitter-side decisions went unverified: the
design gives each a stage case (steps 2 and 4).

Probe for the fallback: tuples doubled through aliases, a generic `keep`
and a `println` of the value. Depth 11: 63 generic instances, no
fallback. Depth 13: 72 generic instances, 5 fallbacks.

`hd_mono::instruction_types` (`lib.rs:707`), documented as "the
instruction types of this instance's final body, substituted once for
emission", has no caller: the emitter substitutes lazily per instruction
instead.

## Appendix E. Details

- **Pipeline position.** After collection; before ownership; before the
  release inliner, after the dev one (per generic body). Invariant: nothing after `instantiate` calls `subst` on a
  body type.
- **Data layout.** No new columns but `phase`; `InstanceTable` holds
  `Arc<Body>` per `InstId`, replacing the driver's `Run::instances` map.
- **Worst case.** O(sum of instance sizes); instance count is bounded by
  collection's existing limits (`crates/hd_mono/DESIGN.md` §13.4).
- **Determinism.** Instances are built in `InstId` order; keys are
  stable hashes.
- **Rollback.** Each step is one commit; steps 1 to 3 change no output.
- **Parse log.** The one hd block (problem 2) parses with
  `scripts/hd-conformance.sh parse` and builds with `hd build` on
  `2ebf4a5b1`. The pipeline sketch is not hd.
- **Phases.** Steps 1 to 7 above, each about one to three hours, each
  with the test named in its row.
