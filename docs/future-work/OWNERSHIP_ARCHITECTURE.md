# Ownership Compilation Architecture

Status: design exploration, 2026-11-04; nothing here is decided or in the
specification. It asks where the compiler should decide every retain,
release, drop, uniqueness check, loan, cell, closure environment and frame
slot, and whether the value model itself is part of the answer.

It reviews the ownership passes (`crates/hd_tir/src/own.rs` and
`own/*`), the driver's summaries and `own_instance`
(`crates/hd_driver/src/own.rs`), the backend's count, loan, view and
cleanup code (`crates/hd_wasm_linear/src/emit/*`, `layout.rs`, `opt.rs`),
[TIR v2](../compiler/tir-v2.md#the-ownership-passes),
`crates/hd_wasm_linear/DESIGN.md` §23, and the records
[Pointer Loans](POINTER_LOANS.md), [Raw Buffers](RAW_BUFFER.md),
[Standard Library For References](STDLIB_REFERENCES.md),
[The Path To Returned Local References](RETURNED_REFERENCES_PATH.md),
[One Lifetime Solver](LIFETIME_SOLVER.md) and
[Non-Copyable Types](NON_COPYABLE.md). The evidence is the static audit
[#1364](https://github.com/HerringtonDarkholme/hd-lang/issues/1364), its
115 feature snapshots under `crates/hd_driver/tests/cases/features/`,
[#1360](https://github.com/HerringtonDarkholme/hd-lang/issues/1360)
and [#1205](https://github.com/HerringtonDarkholme/hd-lang/issues/1205).
It changes no spec, compiler or std code.

## Contents

- [The Domain](#the-domain)
- [Problem, And What hd Has Today](#problem-and-what-hd-has-today)
- [Survey](#survey)
- [The Design Space](#the-design-space)
- [Option 1: Derived Ownership On Values, Per Instance](#option-1-derived-ownership-on-values-per-instance)
- [Option 2: A Low Ownership IR After Monomorphization](#option-2-a-low-ownership-ir-after-monomorphization)
- [Option 3: Close The World](#option-3-close-the-world)
- [Option 4: Borrow By Meaning](#option-4-borrow-by-meaning)
- [Option 5: Trace Linear Memory, Keep A Shared Bit](#option-5-trace-linear-memory-keep-a-shared-bit)
- [Making The Right Answer Structural](#making-the-right-answer-structural)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

## The Domain

A reference-counted language keeps a count in each heap object. Copying a
reference adds one (a **retain**); dropping one subtracts one (a
**release**); the object is freed at zero. The program never writes these
operations. The compiler decides each one, and every one it adds beyond
need is time and code.

Five ideas carry the whole field.

| Idea | Meaning | Example |
| --- | --- | --- |
| **owned** value | holds one count of its own; someone must consume or release it | a call's result |
| **borrowed** value | holds no count; valid while something else keeps the object alive | a parameter the caller keeps alive (Swift's "+0", Mojo's `read`) |
| **last use** | after it, the value is dead; an owned value is released or moved there | `s` in `f(s)` when nothing reads `s` later |
| **consuming** use | takes the count: a store, an owned argument, a return | `all.push(ys)` |
| **lending** use | only reads: needs the object alive, takes no count | `s.len()` |

From these, the right count is mechanical. An owned value used by two
consumers needs one retain. A value used only by lending uses, whose source
nothing changes meanwhile, needs none. A value whose last use is not a
consumer needs one release right after it. Perceus (Koka) and λRC (Lean 4)
compute exactly this, and prove it minimal relative to their inputs.

**Two families of compilers.** One family **derives** counts from these
facts in one pass: Perceus, λRC, Nim's destructor injection, Lobster. The
other **inserts naively and removes by pattern**: put a retain on every
copy, then run optimizations that cancel pairs they recognize. Swift's
original ARC optimizer was the second kind. Its patterns kept missing
shapes, which is why Swift rebuilt SIL around Ownership SSA, where each
value's ownership is an invariant the verifier checks.

**Places and values.** A *value* is the result of a computation, named
once (static single assignment, SSA). A *place* is storage: a local, a
field, an element. In `t := s; u := t`, an SSA compiler sees one value
under three names. A place-based compiler sees three locals and must prove,
step by step, that each move or borrow is safe.

**Uniqueness.** Copy-on-write lists share one buffer until a write. A write
first checks that the buffer has one holder (`IsUnique`), and copies it
otherwise. A buffer the function just made, and never copied, is unique by
construction: the check is dead.

**Representation.** Where a value lives is also a decision. In Wasm, a
local has no address, so any `&x` forces `x` into linear memory: a frame
slot on the shadow stack, or a counted heap cell. Closure environments,
`using` bindings and suspension frames face the same choice. Escape
analysis answers it: storage nothing outlives can stay in registers.

**Generics.** A generic body can be compiled once for every type (erased,
or uniform: Lean, Koka, Java), or once per type (monomorphized: Rust,
C++). Uniform code counts every value alike. Monomorphized code knows which
types count. Deciding counts once for a generic body that is *then*
monomorphized gets neither benefit: every type parameter is "maybe
counted".

**A worked example.** This is `bind` from the `binding` feature snapshot.

```text
fn bind(s: string) -> usize:
    t := s
    let u = t
    u.len()
```

`u.len()` only reads, and the caller keeps `s` alive for the call. So the
right answer is zero count operations. hd today emits a release of `s` at
the exit and makes the caller retain it, because the passes see three
locals and a move chain, not one borrowed value (see
[the problem](#problem-and-what-hd-has-today)).

## Problem, And What hd Has Today

**The question.** Which architecture lets the compiler find every
needed count, uniqueness check, loan and storage choice, and none it
does not need, as a property of its structure? The owner asked on
2026-11-04: "all the loopholes feel the architecture is bad, and it cannot
direct you to a correct solution." The audit (#1364) found about 35
classes of redundant or naive lowering, tickets #1353-#1397 and #1360.

### The pipeline today

| Step | Where | What it decides |
| --- | --- | --- |
| check | `hd_check` | places, reads in mode `use`, `Ref`s with a local bit; lifetimes, origins and exclusivity of non-`*T` paths |
| inline (release) | `hd_opt/src/inline.rs`, at decode, on the **generic** body | which calls remain; a trait call through a bound, and a callee with a `&mut` parameter, stay calls |
| ownership passes | `hd_tir/src/own/*`, once per **generic** body | late reads, insertion, borrow, move, pair cancellation, known-null, hoisting; frame locals; argument promotion |
| summaries (release) | `hd_driver/src/own.rs` | kept, lent and written parameters per SCC; exclusive copies |
| collection | `hd_driver`, `hd_mono` | instances, erased bodies, thunks, witnesses |
| emission | `hd_wasm_linear/src/emit/*`, per **instance** | count glue the passes could not see (`release_untaken`, `push_args`, entry retains), views, loans and their elision, cells, environments, suspension frames |
| Wasm cleanup | `opt.rs` | release of zero, static counts, generated pairs, effect-free calls |
| link | `module.rs` | folds `rt:ref_is_view` in a module that makes no view |

Only collection and emission see instances. No IR exists after
monomorphization: the emitter is the monomorphizer. `own_instance`, the one
per-instance ownership run, serves only declaration-fact specializations
(`hd_driver/src/lib.rs`, the caller of `own_instance`).

### The hypotheses, tested

| # | Hypothesis | Verdict | Evidence |
| --- | --- | --- | --- |
| 1 | Decisions split across three layers | **Confirmed, and it is six.** | inliner, passes, summaries, emitter glue, Wasm cleanup, link. `elide.rs` runs its own whole-program loan analysis in the emitter |
| 2 | Ownership decided once per generic body | **Confirmed; the inliner too.** | `OwnCx::counted` answers `Maybe` for open types; a bound trait call "is resolved per instance and stays a call" (tir-v2, The inliner). The saving was small: 0.8 to 1.07 instances per decoded body on six programs (tir-v2, Cost) |
| 3 | Passes work on locals, not values | **Confirmed.** | `bind` moves `s` through `$1`, `$2`, `$3` and ends `summary kept [$0]`; `over_value` copies each element because its consumer is a `Store` into `x` |
| 4 | Representation chosen with little escape analysis | **Confirmed, and the checker already knows more.** | a closure passed directly to a `fn` parameter has a fresh lifetime (`r[fn.capture.local-parameter]`), yet `capture_call` allocates its environment; `frame.rs` re-derives the checker's origins over the body |
| 5 | #1360 option C shows the split | **Confirmed.** | `views_absent` is decided at emission; default conformance: 2,578 modules view-free, 55 kept by over-approximation, 140 make views (40e6f5b3d) |
| 6 | #1205's gap is `&mut` calls, not counts | **Confirmed, two causes.** | the inliner runs before monomorphization, so `value.hash(&mut hasher)` through `T < Hash` stays a call; and "mutable-reference parameters keep their call boundary", so `hasher` sits in a shadow-stack frame |
| 7 | hd differs from Lean and Koka | **Confirmed; Swift is the closer analog.** | Swift has value types with copy-on-write, classes (hd's `*T`), `inout` (hd's `&mut`) and dynamic exclusivity checks (hd's model A loans) |

### Three findings beyond the hypotheses

1. **Insert, then weaken by pattern.** Insertion makes every read a copy.
   Five passes then weaken it, each under its own syntactic rule ("one
   straight-line run", "the statement of the innermost block", "a `copy`
   whose `Drop` follows"). Any shape outside every rule keeps a count. The
   audit's 35 classes are the shapes no rule matches. This is the
   pre-OSSA Swift design, not the Perceus one.
2. **Instance facts reach only the emitter.** Whether a type counts,
   whether a reference can be a view, which impl a bound picks, and what a
   callee's instance owns are known only during emission. So the emitter
   must add glue, and the fixes for #1360 and #1205 land there.
3. **The checker's facts are re-derived or dropped.** The lifetime solver
   proves where references point and that non-`*T` paths are exclusive.
   The passes re-derive origins (`frame.rs` rule 2), the inliner keeps a
   `&mut` call boundary the checker already made exclusive, and the backend
   ignores closure lifetimes.

### Where the costs come from: language or compiler

| Cost | Language rule behind it | Compiler-only fix | Language change, and its user cost |
| --- | --- | --- | --- |
| a count per copy of a string or list | every read is a copy by meaning (tir-v2, "Should the checker mark") | borrow on values with exact liveness | reads borrow by rule ([Option 4](#option-4-borrow-by-meaning)): new `conflicting-access` errors on snapshots |
| callee owns each by-value argument | none: `layout::Pass` is an ABI choice | lent ("+0") by default, as Swift | none |
| copy-on-write checks | `List`/`Map` copy-on-write (owner, two families) | uniqueness as a value fact | none needed |
| a cell per shared capture | `r[fn.capture.cell]` shares a captured `let` | a frame slot or a value when no closure escapes | none |
| a heap environment per closure | closures are values `(code, environment)` | escape analysis; lambda sets ([Option 3](#option-3-close-the-world)) | none |
| representation views | `r[types.trait.safe.method-type-param]`: generic methods through `dyn`; enum payload slot sharing (§23.3) | per-value view facts | make those methods unavailable on `dyn`, as Rust: `dyn-member-unavailable` |
| loans on `*T` | model A (owner, constraint) | elision, already built | none: kept |
| a shadow-stack frame per `&mut` of a local | a reference is an address; Wasm locals have none | inline, then scalar replacement; values in and out for small `&mut` | none |
| every local saved at a suspension | suspending functions | liveness before the frame is laid out | none |
| a context object per row call | rows pass providers "as one bundle" (ch. 04, generic sharing) | per-row instances | none: unobservable |

Every row but two has a compiler-only fix. The language decides only the
copy-by-meaning rule and the erased generic method path.

### What Wasm makes cheap and expensive

| Wasm fact | Consequence for hd |
| --- | --- |
| locals have no address | every `&x` that survives inlining puts `x` in linear memory; only the hd compiler can undo it |
| the value stack cannot be scanned | a tracing collector must spill pointers to memory at each allocation (Binaryen's `SpillPointers` pass exists for this [S27]) |
| multi-value results | a small `&mut` struct can cross a call as values in and values out |
| V8 inlines small functions and, since M137, speculates on indirect calls [S28]; Wasmtime's inliner is off by default [S29] | wasmtime pays for every call hd leaves; the hd inliner must decide |
| engines do no escape analysis of linear memory | a shadow-stack frame or a heap cell is never removed by the engine |
| structured control flow | TIR's structured form maps directly; a CFG IR would need a stackifier |

### The fixed use cases

Every option is shown on these six. Each is a real snapshot or measured
case, with the group of #1364's plan it stands for.

| # | Case | Today | Group |
| --- | --- | --- | --- |
| U1 | `binding` `bind`: `t := s; let u = t; u.len()` | caller retains `s`, `bind` releases it | C (#1365) |
| U2 | `for-loops` `over_value`: `for x in xs: n += x.len()` | a retain and a release per element | C (#1366) |
| U3 | `for-loops` `keep_each` / `comprehension`: a fresh list filled in a loop | `rt:list_unique` per element | B (#1371) |
| U4 | `closures` `capture_call`: `f := fn() -> usize: s.len(); f() + f()` | a heap environment, plus a heap cell per call to lend `s` to `string.len` | D (#1383, #1370) |
| U5 | `match xs.get_mut(i): .Some(r) => *r = v`; `size[T](xs: &List[T])` | a release call for a `(&T)?`; `Maybe` for `T` | A (#1360) |
| U6 | #1205 `hash_of[T < Hash](value: T)` | five nested calls and a shadow-stack frame for the `&mut` hasher: about 85% of UniqMap's gap on V8 | #1205 |

```text
fn over_value(xs: List[string]) -> usize:
    let n: usize = 0
    for x in xs:
        n += x.len()
    n

fn keep_each(xs: List[string]) -> List[string]:
    let mut out: List[string] = []
    for x in xs:
        out.push(x)
    out

fn capture_call(s: string) -> usize:
    f := fn() -> usize: s.len()
    f() + f()

fn size[T](xs: &List[T]) -> usize:
    xs.len()
```

`hash_of` is std's, in `lib/std/hash.hd`:

```text
pub fn hash_of[T < Hash](value: T) -> u64:
    let mut hasher = DefaultHasher::new()
    value.hash(&mut hasher)
    hasher.finish()
```

**Is it core?** Options 1 to 3 change only compiler internals, except
Option 3's `dyn` rule. Options 4 and 5 reopen owner decisions, which
each section names. No unsettled language decision blocks the question.

## Survey

| Language | Where counts and ownership are decided | Locals | Generics | Closures and storage |
| --- | --- | --- | --- | --- |
| Swift | SIL in Ownership SSA: `load [copy]`/`[take]`, `load_borrow`, `begin_borrow`, `begin_access [dynamic]`; normal parameters are passed guaranteed (+0) by a 2018 change [S1-S3] | stack locals become SSA values (mem2reg) | specialized where visible; unspecialized code calls value witnesses [S4] | closure parameters are non-escaping by default [S5]; `inout` is copy-in copy-out by meaning [S6] |
| Rust | MIR: drop elaboration places drops and drop flags; borrowck on MIR [S7, S8] | places with projections, as TIR | monomorphized; generic methods are not dyn-compatible [S9] | closures are anonymous structs, on the stack unless boxed |
| Lean 4 | λRC: inc/dec inserted into an IR with none, borrow inference for parameters, reset/reuse [S10] | values (pure IR) | uniform boxed objects | uniform |
| Koka | Perceus: precise dup/drop, drop specialization, reuse, borrowed parameters; garbage-free by theorem [S11]; FBIP [S12] | values | uniform, with value types | uniform |
| Nim | ARC/ORC: destructors, `=copy`/`=sink` injected by a control-flow pass on last reads; `sink` and `lent` parameters; cursor inference for borrows; ORC adds a cycle collector [S13, S14] | CFG over locals | monomorphized | heap environment |
| Lobster | compile-time RC "interwoven with type checking": each allocation gets one owner, other uses borrow, functions specialized by ownership; about 95% of runtime count operations removed [S15] | variables | specialized by type and ownership | — |
| Roc | Perceus-style counting with in-place updates of unique values [S16]; lambda set specialization makes closure calls direct and environments unboxed [S17] | values | monomorphized | defunctionalized |
| MoonBit | Perceus-based counting on its Wasm linear-memory backend, no cycle detection [S18] | values | monomorphized | — |
| Go | no counts (tracing GC); escape analysis decides heap or stack per variable [S19] | SSA | dictionaries plus shapes | a captured variable moves to the heap only if it escapes |
| Mojo | arguments are `read` (borrowed) by default, `mut` and `owned` explicit; values destroyed as soon as possible after last use [S20, S21] | values with origins | monomorphized | — |
| Hylo | mutable value semantics: no shared mutable references, `let` projects (borrows), copies are explicit; storage uniquely owned [S22] | projections | specialized or witness | — |
| Kotlin/Native | first reference counting plus a cycle collector; replaced by a tracing collector [S23] | — | erased | heap |

**Takeaways.**

1. **Every shipped RC compiler decides counts where representation is
   known.** Lean and Koka box polymorphic values uniformly, so one body is
   exact. Swift, Roc, Mojo and Rust decide on specialized code. hd mixes
   per-type layouts with per-generic-body decisions, so it is exact for
   neither.
2. **The reliable ones derive; they do not weaken.** Perceus, λRC, Nim,
   Lobster and Mojo compute each count from ownership and last use. Swift
   moved from pattern-based ARC optimization to OSSA invariants for the
   same reason.
3. **Storage is an escape question on the same IR** (Go, Swift's
   non-escaping closures, Roc's lambda sets), and **defaults remove work
   by convention**: Swift's +0 parameters, Mojo's `read`, SE-0103.

## The Design Space

Breadth first: every direction considered, the axis it changes, and
whether it is carried forward as an option.

| Direction | Axis | Carried forward | Why |
| --- | --- | --- | --- |
| keep the layered passes, fix by group (#1364 plan A-E) | compiler | baseline column | the plan in flight; the comparison needs it |
| derive counts on SSA values, per instance, inside TIR | compiler | **Option 1** | the orchestrator's A, sharpened |
| a low ownership IR after monomorphization | compiler | **Option 2** | the orchestrator's B |
| monomorphize everything, defunctionalize closures, no erased path | compilation model | **Option 3** | radical simplification of the model |
| Swift-style erased bodies over addresses with value witnesses | compilation model | folded into Option 3 as its alternative | keeps generic `dyn` methods without views |
| lent (+0) parameters by default | ABI | folded into Option 1 | a convention, not an architecture |
| reads borrow by language rule (Hylo `let`, Mojo `read`) | language | **Option 4** | radical: the checker decides, not an optimizer |
| non-copyable by default for counted types (Rust, Mojo) | language | dropped | reverses Copy by default outright; Option 4 is the smaller form |
| tracing collector on linear memory, with a shared bit for copy-on-write | value model | **Option 5** | radical: drops counts; breaks the RC constraint |
| generational references (Vale) [S24] | value model | dropped | replaces model A's loans, a constraint; `*T` still needs shared ownership |
| linear types (Austral) [S25] | value model | dropped | no implicit copies or drops at all; incompatible with Copy by default and copy-on-write |
| region or arena allocation (MLKit, Verona) | value model | dropped | region inference leaks space without a collector; non-escaping storage is Options 1-3's escape analysis |
| Perceus reuse and FBIP | optimization | folded into Options 1 and 2 | hd's in-place writes through unique buffers already reuse; reuse of freed cells is a later pass |
| leave counts to the engine | compilation model | dropped | no engine optimizes counts in linear memory |

## Option 1: Derived Ownership On Values, Per Instance

**The idea.** Keep TIR and its pipeline shape, but change three inputs and
one method. Run inlining and ownership per instance, after
monomorphization. Treat never-addressed locals as values. Derive each read
mode and drop from liveness and conventions in one decision, instead of
inserting and weakening. Record storage and every count as TIR
instructions, so the emitter decides nothing.

### What changes

| Piece | Today | Option 1 |
| --- | --- | --- |
| when the passes run | once per generic body; `Maybe` for type parameters | per instance, after substitution and impl rechoice (the `own_instance` path, generalized) |
| inlining | on the generic body; bound calls and `&mut` callees stay calls | per instance; a bound call is an impl call; `&mut` callees inline under the checker's exclusivity |
| locals | every local is a place | a local never addressed, captured or read by a cleanup suite is a value: its reads are the stored value (single-assignment locals first; full SSA over structured control flow by Braun et al.'s method [S30] if needed) |
| read modes | insertion copies all; borrow, move and pairs weaken | one decision per read: lend if every use lends and no write overlaps the value's live range; move at the place's last access; copy otherwise |
| uniqueness | `IsUnique` before every change, then hoisted | `IsUnique` only where the buffer is not known unique: a fresh, never-copied value is |
| parameter convention | by type (`Pass::Owned`); release summaries lend | lent by default; owned only for parameters a body consumes |
| storage | cells and environments chosen in the emitter; frame slots in `frame.rs` | one storage pass: value, frame slot or heap cell, for locals, environments, `using` and defaults, from the checker's origins and lifetimes |
| suspension | every local saved | the frame holds the values live across each point |
| emitter | adds glue; owns views, loans, cells | lowers `Retain`, `Release`, `IsUnique`, `LoanBegin`, `LoanEnd` and storage marks one to one |

### On the use cases

U1 and U2: `s`, `t` and `u` are one value; `x` is the element read. Every
use lends, nothing writes `xs`, so neither is counted.

```ir
fn bind(s: string lent) -> usize
  %0 = Param s
  %1 = Call bytes_len (%0 lend)      # t and u are %0
  Return %1                           # nothing owned: no exit set

fn over_value(xs: List[string] lent) -> usize
  loop
    %e = Read borrow [$xs [%i]]       # x is %e; its one use lends
    %l = Call bytes_len (%e lend)
```

U3: `out` is `NewList`, never copied, so it is unique; the pushes carry no
check. U4: `f` is never stored, returned or passed, so its environment is
the caller's values. The known call takes `s` lent, and `string.len`
inlines.

```ir
fn keep_each(xs: List[string] lent) -> List[string]
  %out = NewList                      # unique: fresh, never copied
  loop
    %e = Read copy [$xs [%i]]         # one count: the list keeps it
    ListPush %out %e                   # no IsUnique

fn capture_call(s: string lent) -> usize
  %a = Call capture_call#1 (%s lend)  # storage: values; the call is known
  %b = Call capture_call#1 (%s lend)  # then inlined: bytes_len, twice
```

U5: the instance knows a `(&T)?` from a concrete place is an address, so it
has no release. The loan ends at `LoanEnd`, its own instruction. U6:
`hash_of[usize]` rechooses `value.hash` to `usize`'s impl, inlines the
chain, and `hasher` becomes a value: no frame, no call.

```ir
fn hash_of[usize](value: usize lent) -> u64
  %h0 = Const 14695981039346656037   # hasher.state, a value
  %h1 = Call fnv_mix (%h0 byte0)      # inlined write_usize, write_integer
  ...
  Return %h8
```

### Groups removed

| Group | Removed by structure | Left |
| --- | --- | --- |
| A releases of nothing | #1381, #1386, #1360 for concrete code: per-instance counted facts and definite initialization | views from erased calls stay per value |
| B copy-on-write placement | #1371, #1397, #1396 (a fresh or proven-unique value); #1380 (no pin behind a `&` the checker proves unchanged) | — |
| C kept or copied when only read | all: one value per binding, lent parameters, exact liveness | #1392 partly: a suspending frame still owns what it keeps |
| D environments and cells | #1370, #1382, #1383, #1395 (storage pass); #1387 (a held closure lends) | #1385 adapter calls need a known callee: Option 3 |
| E suspension and providers | #1393 (live sets); #1390 (a lent provider) | #1391, #1394: per-row instances are Option 3 |
| #1205 | both causes: bound calls inline per instance; a `&mut` local becomes values | — |

### Erased generics, cost and migration

- **Erased bodies and views stay.** Views become a per-value fact (made by
  an erased call or a `!Copy` payload reference), not a module fact, so
  only those values release. Option 3 can remove them later.
- **Compile time.** Per-instance passes cost what instances cost: 0.8 to
  1.07 per decoded body on the measured programs. Inlining per instance
  grows bodies in release only. Emitter glue and `views_absent` go.
- **Size.** Output shrinks with every count and cell removed; release
  inlining grows some functions. The compiler loses emitter glue and
  three weakening passes, and gains one derivation and one storage pass;
  net roughly flat against the 2 MB cap (unmeasured).
- **Migration.** Nine steps, each one cause, each with snapshot diffs that
  may only remove operations: per-instance pipeline; value locals;
  derivation behind a flag beside the old passes; uniqueness facts;
  storage pass; suspension live sets; count and storage instructions;
  `&mut` inlining with values in and out; the verifier of
  [Making The Right Answer Structural](#making-the-right-answer-structural).
  About 25 to 40 one-hour tasks, estimated.
- **Soundness.** Every rule is today's rule on a smaller domain: a value
  local has no alias, by its definition. A missed fact keeps a copy, never
  a dangling value. The verifier's invariant 23 stays.

**Spec impact.** None; compiler internals. It revises two orchestrator
assumptions in tir-v2 ("once per body, not per instance"; "generic bodies
keep symbolic count operations").

## Option 2: A Low Ownership IR After Monomorphization

**The idea.** Add one IR between TIR and Wasm, in the style of Swift's
OSSA: SSA values over structured regions, explicit memory, and every
ownership and access operation an instruction. Lower each instance of TIR
into it. Move the ownership passes, storage, suspension splitting and
inlining there. The emitter prints it.

### The instruction set, in outline

| Kind | Instructions |
| --- | --- |
| memory | `alloc_value`, `alloc_frame`, `alloc_box`, `field_addr`, `elem_addr`, `load [copy\|take\|borrow]`, `store [init\|assign]` |
| ownership | `copy`, `destroy`, `begin_borrow`/`end_borrow`, `is_unique`, `mark_dependence` |
| access (model A) | `begin_access [read\|modify] [static\|dynamic]`/`end_access` |
| control | structured `block`, `if`, `loop` with block parameters, as Wasm |

Standard passes then do the work: memory to values for unaddressed
storage, scalar replacement of small aggregates, canonical copy removal on
SSA lifetimes, access-marker merging (Swift's access enforcement
optimization), and suspension splitting on liveness.

### On the use cases

U4 and U6 show the difference from Option 1: storage is an allocation that
a pass promotes, not a mark another pass reads.

```ir
func capture_call(%s: string @guaranteed) -> usize
  %env = alloc_box {string}           # as lowered: the closure's environment
  store [init] %s to %env.0           # promotion sees no escape:
  ...                                 # %env becomes the value %s
  %a = apply capture_call#1(%s)       # direct call; inlined next

func hash_of<usize>(%value: usize) -> u64
  %h = alloc_frame DefaultHasher      # &mut hasher
  %a = begin_access [modify] [static] %h
  apply DefaultHasher.write_usize(%a, %value)   # inlined
  end_access %a                       # SROA: %h is three values; no frame
```

U1, U2, U3 and U5 come out as in Option 1, from copy canonicalization on
SSA values and `is_unique` folding on fresh allocations.

### Groups removed

Everything Option 1 removes, plus three it handles more naturally: E's
#1393 by liveness on the split points; F's #1369 (`+=` through `&mut`),
since the store's ownership and the buffer's uniqueness are visible in one
IR; and the Wasm cleanup's fresh-local churn, since the IR allocates
locals itself. #1385, #1391 and #1394 still need Option 3.

### Erased generics, cost and migration

- **Erased bodies and views stay**, as explicit `alloc_box` and view
  instructions the same passes see.
- **Compile time.** One more lowering per instance. It may be net faster:
  module finish (Wasm optimization and encoding) was 110 of 126 ms of
  json's link (tir-v2, Cost), mostly cleaning up one fresh local per TIR
  value. Unmeasured.
- **Size.** Best output of the five. The compiler grows by a new crate,
  estimated 10,000 to 15,000 lines, before the passes (12,000 lines) and
  emitter glue go: a risk for the 2 MB cap during migration.
- **Migration.** A parallel path: the new IR for functions it supports,
  the old emitter for the rest, with the same ABI. About 50 to 80 tasks,
  estimated, with the largest gains late.
- **Reopens an owner decision.** tir-v2's "There is no second IR"
  (2026-10-21). New evidence: decisions now sit in six layers, and the
  emitter is the only post-monomorphization stage.

## Option 3: Close The World

**The idea.** A radical simplification of the compilation model: nothing
polymorphic reaches the optimizer. Every generic, every row and every
closure call is resolved to concrete code before ownership runs. Three
slices, each removable on its own:

| Slice | Change | Removes |
| --- | --- | --- |
| 3a, no erased path | a generic method is unavailable on a `dyn` value (Rust's rule [S9]), and enum payloads are laid out per variant so a payload reference is an address | erased bodies, thunks, witnesses, every representation view, `rt:ref_is_view`, `Walk::View`; `erased.rs` (887 lines), `views.rs` (318) and parts of `refs.rs` |
| 3b, per-row instances | a row-polymorphic function is instantiated per row, as generics are | `rt:ctx_new`/`rt:ctx_find` on known callees (#1394) |
| 3c, lambda sets | each function value's possible closures are known per program; a call is a direct call or a switch; an environment is an unboxed value (Roc [S17]) | adapters (#1385), environment allocation and counts (#1383, #1387), `block_on` of a known function (#1391) |

3a's language change, in hd:

```text
trait Codec:
    fn encode[T < Display](&self, value: T) -> string

fn log_count(c: &dyn Codec, n: i32) -> string:
    c.encode(n)  # under 3a: error dyn-member-unavailable
```

3c on a function value:

```text
fn apply(f: fn(string) -> usize, s: string) -> usize:
    f(s)

fn width(s: string) -> usize:
    s.len()

fn demo(s: string) -> usize:
    apply(width, s) + apply(fn(t: string) -> usize: t.len() + 1, s)
```

```ir
fn apply[{width, demo#1}](f: u8, s: string lent) -> usize   # f is a tag
  switch f
    0 => Call width (s lend)          # direct; inlinable
    1 => Call demo#1 (s lend)         # no environment: it captures nothing
```

**The alternative to 3a.** Swift keeps generic methods behind witnesses
[S4]: an erased body handles values by address, with a value witness
(size, copy, destroy) per type. A reference into a concrete place is then
its plain address, so views go without removing the feature. It costs a
second calling convention and dynamically sized frames.

### Groups removed and cost

- **Removed by structure.** A's view releases entirely; D's #1383, #1385,
  #1387; E's #1391, #1394; and, with per-instance inlining, #1205's bound
  call. It does not fix C or B: those are Option 1's or 2's.
- **Compile time.** 3a deletes work. 3b and 3c add whole-program analysis
  and more instances. Lambda sets are type inference over the program in
  Roc; in hd they would be release-only or dev-and-release, a choice that
  doubles test surface if split.
- **Size.** 3a shrinks compiler and output. 3c may grow output: a
  higher-order function is copied per lambda set. Unmeasured against the
  cap and the bench.
- **Language cost.** 3a removes rules 4 to 7 of
  [Dynamic Safety](../../spec/lang/04-type-system.md#dynamic-safety)
  (language tier); five conformance fixtures cite them. `dyn Hash` loses
  `hash`, as in Rust. 3b and 3c change no rule: sharing is unobservable
  (`r[types.generic.sharing]`).
- **Constraint.** 3b and 3c are whole-program analyses; keeping them
  release-only respects "interprocedural optimization is release-only".

## Option 4: Borrow By Meaning

**The idea.** A radical language change: the language, not an optimizer,
says when a value is borrowed. A read-only binding of a place, a
by-value parameter the body does not change, and a `for` element are
borrows by rule. The lifetime checker already enforces borrows; it would
enforce these too. Counts then appear only where the source stores,
returns, changes or explicitly copies, as in Hylo [S22] and Mojo [S20].

```text
fn snapshot(xs: List[string]) -> usize:
    let mut ys = xs
    before := ys
    ys.push("x")  # under this option: error conflicting-access
    before.len() + ys.len()
```

The fix is an explicit copy, `before := copy(ys)  # hypothetical syntax`.

| Effect | Detail |
| --- | --- |
| removes | group C by rule (#1365, #1366, #1367, #1379, #1392); the borrow pass becomes the checker's existing lifetime check |
| leaves | A, B, D, E and #1205: storage and uniqueness are untouched |
| rules | language tier: bindings in [Places And New Objects](../../spec/lang/13-values-and-references.md#places-and-new-objects) and [Copy](../../spec/lang/13-values-and-references.md#copy), loops in ch. 06, parameters in ch. 07; one new std or prelude `copy`. Removes no rule |
| user cost | every snapshot of a changing place needs `copy`; agents meet `conflicting-access` where Go, Swift and Kotlin code just runs |
| reopens | the owner's 2026-10-21 decision that the checker does no reference counting, and the copy-by-meaning rule of tir-v2. New evidence: group C is the largest of the audit |

**Soundness.** It is Hylo's law of exclusivity, restricted to locals:
sound, since the checker already proves the same property for `&T`.

## Option 5: Trace Linear Memory, Keep A Shared Bit

**The idea.** A radical value-model change that **breaks the "reference
counting" constraint** (it keeps linear memory and never uses Wasm GC). A
tracing collector reclaims linear memory; no value carries a count.
Copy-on-write keeps a one-bit sticky "shared" mark (Wise and Friedman's
one-bit count [S26]): a copy of a collection sets it, and a write to a
marked buffer copies it once.

```text
fn grow(xs: List[string]) -> usize:
    let mut ys = xs
    ys.push("a")  # copies: the buffer was shared
    ys.push("b")  # in place: the copy is unmarked
    ys.len()
```

| Effect | Detail |
| --- | --- |
| removes | every retain and release, release glue, drop flags, exit sets, the cycle collector's separate pass: groups A and C, and the count half of D and E |
| keeps | copy-on-write checks (B); allocation of cells and environments (D, though cheaper); frames; loans (model A uses its own bits); #1205 untouched |
| adds | pointer spills to the shadow stack at each allocation, since Wasm's stack cannot be scanned; collector pauses; heap headroom; an extra copy when a once-shared buffer is written after its other holder died |
| fits hd | resources are `Disposable` and `using`, not destructors, so nothing needs prompt release; `Weak[T]` maps to a weak table; hd already has trace glue for cycle-capable types (§23.28) |
| precedent | Kotlin/Native replaced RC plus a cycle collector with tracing [S23]; Go traces linear memory on Wasm |
| reopens | the RC constraint ("RC on linear memory", [Refcount Runtime](REFCOUNT_RUNTIME.md)) and the measured 1.2x RC target |

Worth asking because it is the only option that makes counts disappear
rather than become precise. Against it: hot loops that allocate would pay
spills, and a uniqueness bit that never clears is weaker than a count.

## Making The Right Answer Structural

Audits find redundant counts after the fact. Three mechanisms make them
impossible to emit unnoticed, and they work with Options 1 and 2.

1. **A count exists only with a reason.** Each `Retain`, `Release` and
   `IsUnique` carries one reason from #1364's closed list: storage that
   outlives the frame, a second owner, the owner's last use, a view, or a
   by-design `&dyn` or suspension count (#1362's reason tags). The
   verifier rejects a count whose reason its facts do not support.
2. **A derivation is minimal relative to named facts.** When each count is
   a function of liveness, consuming uses, conventions, escape and
   uniqueness, a redundant count traces to one wrong fact. The `own` view
   prints the fact ("`s` kept: lent to `g`, which may store"), not the
   pattern that missed.
3. **The emitter cannot count.** Count glue has one producer, the IR. A
   debug assertion fails when emission produces a count with no IR
   instruction behind it, so "decides none itself" (`emit/count.rs`) is
   enforced, not stated.

Two checks back them: an exact, slow solver without today's caps (64
frame candidates, loop depth 8, 64-bit parameter masks) runs over the
feature snapshots in CI and reports every count the fast one keeps; and
#1361's runtime detector confirms nothing is left on the bench.

## Comparison

| | Plan A-E today | 1 Derived, per instance | 2 Low IR | 3 Close the world | 4 Borrow by meaning | 5 Trace + shared bit |
| --- | --- | --- | --- | --- | --- | --- |
| U1 `bind` | point fix #1365 | 0 counts | 0 counts | unchanged alone | 0 counts by rule | 0 counts |
| U2 `over_value` | point fix #1366 | 0 counts | 0 counts | unchanged alone | 0 counts by rule | 0 counts |
| U3 fresh list | hoist fix #1371 | no check | no check | unchanged alone | unchanged | check stays |
| U4 `capture_call` | #1383, #1370 | no allocation | no allocation | no environment, direct call | unchanged | allocations stay, uncounted |
| U5 `(&T)?`, `T` | option C, emitter | no release | no release | no views at all | unchanged | no release |
| U6 `hash_of` | not covered | no call, no frame | no call, no frame | bound call resolves; frame stays alone | unchanged | unchanged |
| groups removed by structure | none | A (concrete), B, C, D (most), E (most), #1205 | as 1, plus E fully, #1369 | A views, D adapters, E rows | C | A, C, D/E counts |
| rules added or removed | none | none | none | 3a: four language-tier rules removed | language tier: bindings, loops, parameters borrow; `copy` added | runtime and ch. 08 collection rules |
| soundness | as today | today's rules on values | OSSA invariants | as today | Hylo exclusivity | standard tracing; spills must cover every root |
| agent-writability | unchanged | unchanged | unchanged | 3a: one more `dyn` error | worse: snapshots need `copy` | unchanged |
| compile time | unchanged | about even | uncertain; may drop | 3a less, 3c more | about even | less |
| output size | small cuts | smaller | smallest | 3a smaller, 3c may grow | smaller | smaller, plus spills |
| compiler size (2 MB cap) | grows with patches | about flat | grows, then shrinks | 3a shrinks | small | shrinks |
| migration | per ticket | 9 steps, snapshot oracle | parallel backend | three slices | spec, checker, std rewrite | new runtime |
| owner decisions reopened | none | none | "no second IR" | 3a: generic methods on `dyn` | checker counts nothing; copy by meaning | RC on linear memory |
| evolution | each std move to generic code adds `Maybe` | std-in-hd collections get exact counts | room for later optimizations | composes with 1 or 2 | composes with 1 | collector work replaces count work |

## Recommendation

**Recommendation: Option 1 as the spine, with Option 3a, then 3c after
measurement, and the structural checks.**

- It addresses the cause the audit shows: instance facts and value
  identity are missing where counts are decided. The 35 classes are
  symptoms of three inputs and one method, all of which Option 1 changes.
- It is the survey's best practice inside hd's owner decisions: Swift's
  shape (specialize, SSA values, +0 parameters, ownership on the result)
  without the second IR the owner ruled out.
- It is visible progress early. Step 1 (per-instance inlining and passes)
  should clear every `Maybe` row and turn `hash_of`'s bound call into an
  impl call; step 8 then removes the hasher's frame. Each step reports
  numbers on V8 and wasmtime.
- 3a deletes the view machinery the audit priced at 12 run-time test
  sites, for a feature five fixtures use. Its cost to users is one
  `dyn-member-unavailable` case, as in Rust.
- The reason-tag verifier turns the next audit into a failing build.

**What it gives up.** Option 2's cleaner memory model and probably
better output and link time; Option 5's chance to remove counts entirely.

**Next best: Option 2.** Choose it if step 2 (values in TIR) or the
storage pass proves awkward on places, or if the Wasm cleanup's compile
time becomes the bottleneck.

**What happens to plan A-E.** Its tickets become the acceptance cases of
the steps that remove them, rather than point fixes. In-flight work
(#1360 option C) finishes first, as the owner's rules require.

## Questions For The Owner

1. **Which architecture?** Effect: where every future count fix lands.
   Options: (a) today's plan A-E; (b) Option 1, evolve TIR; (c) Option 2,
   a new IR. Recommended: (b). Example: `bind`'s kept `s` is fixed by
   value locals, not by a #1365 pattern.
2. **Ownership and inlining per instance?** Effect: `Maybe` disappears and
   bound calls inline, at about one instance per body. Options: (a) per
   instance in release only; (b) in every build; (c) keep per generic body.
   Recommended: (b), one pipeline. Example: `value.hash(&mut hasher)` in
   `hash_of[usize]` becomes `usize`'s impl call.
3. **Lent parameters by default?** Effect: a by-value argument the callee
   only reads takes no count in any build. Options: (a) lent by default,
   owned when consumed (Swift); (b) keep by type, release summaries lend
   (#1232). Recommended: (a). Example: `fn bind(s: string)` takes `s` lent.
4. **Generic methods through `dyn`?** Effect: whether representation
   views exist. Options: (a) keep them, with views; (b) unavailable on
   `dyn`, as Rust; (c) keep them through Swift-style witnesses, no views.
   Recommended: (b). Example: `c.encode(n)` on `&dyn Codec` becomes
   `dyn-member-unavailable`.
5. **Lambda sets for closures?** Effect: adapters and environments for
   known closures disappear; output may grow. Options: (a) escape analysis
   only (Option 1); (b) lambda sets in release; (c) both builds.
   Recommended: (a) now, (b) after a size measurement. Example:
   `apply(width, s)` calls `width` directly.
6. **`&mut` callees inline?** Effect: #1205's frame goes. Options: (a)
   inline under the checker's exclusivity, loans as explicit markers;
   (b) also pass small `&mut` structs as values in and out when a call
   stays; (c) keep the call boundary. Recommended: (a) and (b). Example:
   `value.hash(&mut hasher)` leaves `hasher` in Wasm locals.
7. **Borrow by meaning?** Effect: group C by rule, new errors for
   snapshots. Options: (a) no; (b) yes, with an explicit `copy`.
   Recommended: (a). Example: `before := ys; ys.push("x")` would fail.
8. **A tracing collector ceiling?** Effect: data on whether RC is the
   right model. Options: (a) no; (b) a one-week ceiling on three bench
   programs; (c) a design record. Recommended: (a); the constraint stands
   unless the owner wants the number. Example: `grow` copies once, counts
   nothing.
9. **A count without a reason fails the build?** Effect: audits become
   checked-build failures. Options: (a) in checked builds and CI; (b)
   report only. Recommended: (a). Example: the `retain string` in
   `over_value` would have no reason and fail.

## Sources

| # | Source |
| --- | --- |
| S1 | Swift, SIL ownership model proposal (refreshed): https://forums.swift.org/t/sil-ownership-model-proposal-refreshed/16872 |
| S2 | Swift, Ownership Manifesto: https://github.com/swiftlang/swift/blob/main/docs/OwnershipManifesto.md |
| S3 | Swift, proposal to change the default ownership for passing parameters (+1 to +0, 2018): https://forums.swift.org/t/proposal-to-change-the-default-ownership-for-passing-parameters/10261 |
| S4 | Pestov and McCall, Implementing Swift Generics, LLVM Developers' Meeting 2017: https://llvm.org/devmtg/2017-10/slides/Pestov-McCall-ImplementingGenerics.pdf |
| S5 | SE-0103, make non-escaping closures the default: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0103-make-noescape-default.md |
| S6 | The Swift Programming Language, In-Out Parameters: https://docs.swift.org/swift-book/documentation/the-swift-programming-language/declarations/#In-Out-Parameters ; SE-0176, exclusive access: https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md |
| S7 | rustc dev guide, drop elaboration: https://rustc-dev-guide.rust-lang.org/mir/drop-elaboration.html |
| S8 | rustc dev guide, MIR borrow check: https://rustc-dev-guide.rust-lang.org/borrow_check.html |
| S9 | Rust reference, dyn compatibility: https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility |
| S10 | Ullrich and de Moura, Counting Immutable Beans (IFL 2019): https://arxiv.org/abs/1908.05647 |
| S11 | Reinking, Xie, de Moura and Leijen, Perceus (PLDI 2021): https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/ |
| S12 | Lorenzen, Leijen and Swierstra, FP²: Fully in-Place Functional Programming (ICFP 2023): https://www.microsoft.com/en-us/research/publication/fp2-fully-in-place-functional-programming/ |
| S13 | Nim, destructors and move semantics: https://nim-lang.org/docs/destructors.html |
| S14 | Nim, introducing ORC: https://nim-lang.org/blog/2020/12/08/introducing-orc.html |
| S15 | Lobster, memory management: https://aardappel.github.io/lobster/memory_management.html |
| S16 | Roc, functional programming and opportunistic mutation: https://www.roc-lang.org/functional |
| S17 | Brandon et al., Better Defunctionalization through Lambda Set Specialization (PLDI 2023): https://dl.acm.org/doi/10.1145/3591260 |
| S18 | MoonBit updates, Perceus-based counting on the Wasm backend (2024-06-11): https://www.moonbitlang.com/updates/page/5 |
| S19 | Go compiler, escape analysis: https://github.com/golang/go/blob/master/src/cmd/compile/internal/escape/escape.go |
| S20 | Mojo manual, ownership and argument conventions: https://docs.modular.com/mojo/manual/values/ownership/ |
| S21 | Mojo manual, death of a value (ASAP destruction): https://docs.modular.com/mojo/manual/lifecycle/death/ |
| S22 | Racordon et al., Implementation Strategies for Mutable Value Semantics (JOT 2022): https://www.jot.fm/issues/issue_2022_02/article2.pdf |
| S23 | Kotlin/Native memory manager: https://kotlinlang.org/docs/native-memory-manager.html |
| S24 | Vale, generational references: https://verdagon.dev/blog/generational-references |
| S25 | Austral specification, linear types: https://austral-lang.org/spec/spec.html |
| S26 | Wise and Friedman, The one-bit reference count, BIT 17 (1977) |
| S27 | Binaryen, SpillPointers pass: https://github.com/WebAssembly/binaryen/blob/main/src/passes/SpillPointers.cpp |
| S28 | V8, speculative optimizations for WebAssembly using deopts and inlining: https://v8.dev/blog/wasm-speculative-optimizations |
| S29 | Fitzgerald, the Wasmtime inliner (off by default since Wasmtime 36): https://fitzgen.com/2025/11/19/inliner.html |
| S30 | Braun et al., Simple and Efficient Construction of SSA Form (CC 2013): https://doi.org/10.1007/978-3-642-37051-9_6 |

hd evidence: #1364's audit comments and plan; #1360's evidence comment;
#1205's last two comments; commit 40e6f5b3d on `lane/ownership-ab`;
`docs/compiler/tir-v2.md` ("The ownership passes", "The inliner", "Cost");
`crates/hd_wasm_linear/DESIGN.md` §23.3, §23.16, §23.27, §23.38.

## Parse Log

Each `text` block was parsed with `scripts/hd-conformance.sh parse` at
main dae8d82c7. Parsing checks syntax only; no block is claimed to
type-check. `ir` blocks are sketches, not hd, and are not parsed.

| Block | Content | Result |
| --- | --- | --- |
| 1 | `bind` | parse |
| 2 | U2-U5 sources | parse |
| 3 | `hash_of` | parse |
| 4 | Option 3a, `Codec` | parse |
| 5 | Option 3c, `apply` | parse |
| 6 | Option 4, `snapshot` | parse |
| 7 | Option 5, `grow` | parse |
