# New Compiler Design: Back Half, Codegen

Part of the [compiler design](README.md).

## Part D2: The Back Half

**Second pass** (backend lane, 2026-10-07), from
[codex-review-response-frontend.md](codex-review-response-frontend.md#changes-for-the-backend-lane)
items 18 to 20 and 22, and the owner's answers of the day: collection
selects impls by head only (§13.2); vtables follow the trait record's
shape, and `CallDyn` passes its evidence operands (§13.5); tuple traits
are template instances (§13.6); defaults run per call and facts are
evaluated at compile time (§12.3); `Result` uses `multi` layouts, and
bounded inlining and scalar replacement are first-release (§12.6);
polymorphic recursion is an error (§13.4).

D2 designs monomorphization, suspension lowering, Wasm GC emission, link,
the runtime, the host interface and the test runner. It consumes these
interfaces from D1 and does not reach around them:

| Interface | From | What D2 gets |
| --- | --- | --- |
| TIR per body (§4.13.11) | `hd_check` through `hd_tir` | the one typed IR: desugared, generic bodies with explicit coercions, dispatch, decision trees, cleanup scopes, suspension and hook points; read from the `tir` entry |
| `ModuleResult` | `hd_check` | init summary, fact records, diagnostics; whether the module has errors |
| folder interfaces (§4.10, §4.11) | `hd_resolve`, `hd_iface` | signatures, impl tables, templates and hidden items, default and fact expressions, per-item hashes for instance keys |
| types and rows | `hd_types` | the InternPool (§3.9.2), stable paths for every `DefId`, and `StableHash` |
| init order | `InitOrder` | statement order per initialization group |
| test plan (§7.3) | `hd_driver` | test programs and their statically registered cases |
| cache | `hd_cache` | `CacheStore`, the key rules of §5.3, entry framing, verify mode, eviction. D2 adds the kinds `code`, `link` and `cwasm` (§11.2) |
| scheduler | `hd_sched` | `TaskKind::Ext` tasks with dependencies on D1 tasks, fuel, cancellation, the opt-in memory cap, content-ordered output |
| diagnostics | `hd_diag` | the `Diagnostic` record for D2's own errors, such as instantiation depth |
| determinism rules | §6.5 | content order, local counters, no IDs in output or keys; D2's entries join the determinism matrix |

## 11. Back-Half Overview

### 11.1 From TIR To A Running Program

```text
 ModuleFinish(m) ═══════════════════════════════════► [tir entry]    per module (D1)
                                  │ generic TIR, per-item TIR hashes, dependency lists
 program root(s) ─────────────────┤  (entry, test registrations, REPL input)
                                  ▼
                       Collect(P): reachability over TIR + impl tables
                                  │ instance set {(item, type args)}, content-ordered
                                  ▼
                       Emit(inst): walk the generic TIR under the substitution,
                                   choose layouts, write Wasm ═══════════► [code entry]   per instance, parallel
                                  │ body bytes + symbolic relocations
                                  ▼
                       Link(P): fold identical bodies, assign indices,
                                build types/globals/data, patch relocations ═► [link entry] = Wasm bytes
                                  │
            ┌─────────────────────┴──────────────────────────┐
            ▼ native (hd_run_wasmtime)                        ▼ browser (hd_web + generated JS glue)
   Precompile(P, tier) ═► [cwasm entry]             program worker: WebAssembly.compile
   InstancePre + pooling allocator                  imports from the JS host
   Store per run / per test case                    poll/wake loop on the worker's event loop
   poll/wake driver + host reactor                  JSPI or sync XHR for block_on
```

`═══►` marks a cache boundary, as in D1. Every boundary goes through
`CacheStore`, and every key follows §5.3's hygiene: no paths, no IDs, no
thread counts.

### 11.2 Stages

This continues D1's table (§1.2).

| Stage | Input | Output | Unit | Parallel | Cached | Key |
| --- | --- | --- | --- | --- | --- | --- |
| Collect | program roots, TIR of reachable modules, impl tables | the instance set, the import set, the type set | program | serial per program; programs in parallel | inside `link` | none |
| Emit | one instance: generic TIR, its substitution, the TIR of callees it inlines | Wasm body bytes, relocations, site and line records | instance | yes | `code` | §13.8 |
| Link | code entries of the instance set | one Wasm module with its custom sections | program | serial per program; fold hashing in parallel | `link` | §13.10 |
| Precompile | Wasm bytes, engine config | wasmtime's serialized module | program and tier | Cranelift compiles functions in parallel | `cwasm` | `H("cwasm", hash of the Wasm, wasmtime version, config hash, target)` |
| Instantiate and run | precompiled module, host | outcome, output | run or test case | test cases in parallel | no | none |

There is no MIR stage: the checker's TIR is the input of `Collect` and
`Emit` (§3.9.1).

### 11.3 Tasks

D2's tasks are `TaskKind::Ext` tasks (§6.1):

```rust
pub enum ExtTask {
    Collect(ProgramId),          // after the tir entry of every module the program can reach
    EvalFact(FactSlot),          // created by Collect, one per fact the program reads (§12.3)
    Emit(InstanceSlot),          // created by Collect, one per code-entry miss
    Link(ProgramId),             // after every Emit of its program
    Precompile(ProgramId, Tier), // native only
    RunCase(ProgramId, CaseIdx), // test runner (§19); the browser runs cases in its worker instead
}
```

- `Collect` starts only after the `tir` entries of every module that the
  program's roots reach in the module use graph exist, and after the
  `HeaderCheck` task of every folder they reach (scheduler.md §6.1). That set comes
  from the manifest's use lists, so it is known before any check runs.
- **The program fast key (mine).** Before `Collect`, D2 computes
  `prog_key = H("prog", toolchain_key, tier, profile, root description,
  sorted [(module path, tir key)] of the modules reachable in the use
  graph)`. A hit names the `link` entry directly. Collection, emission and
  linking are skipped, and a warm `hd run` or `hd test` reads one entry
  per program. Module-level reachability is coarser than item-level, so a
  hit is always sound. An edit to a reachable module that changes no
  instance still misses, and then rebuilds from code-entry hits.
- `Emit` tasks are ordered by TIR size, largest first, as body tasks are
  (§6.3).

### 11.4 Crates

This refines the research's back-half crates
([Q16](research.md#crates)) and D1's §2.1. The
research's `hd_mir` and `hd_opt` are gone: TIR lives in `hd_tir` (§2.1),
and emission-time analyses live in `hd_mono`.

| Crate | Holds | Depends on | Browser |
| --- | --- | --- | --- |
| `hd_mono` | collection, substitution, instance keys, layouts, the depth limit, emission-time analyses (liveness, capture sharing, inlining decisions) | `hd_tir` | yes |
| `hd_host_abi` | the host ABI description: traits, methods, codecs, wait flags; generators for hd-side stubs, wasmtime stubs and the JS glue | `hd_base` | yes |
| `hd_wasm` | emission with `wasm-encoder`, the code entry format, folding, the linker, name and site sections | `hd_mono`, `hd_host_abi` | yes |
| `hd_run` | the embedding API: `Engine`, `Host`, `Provider`, grants, the poll/wake driver logic, panic decoding, the test runner core | `hd_wasm` | yes, without engines |
| `hd_run_wasmtime` | the wasmtime engine, providers of the default profile, the reactor, epochs, limits, the `cwasm` cache | `hd_run`, `wasmtime`, `tokio` (current-thread), `cap-std` | no |
| `hd_web` (extended) | runs D2 in the compiler worker; hands bytes and metadata to the program worker | `hd_run` | only there |

The JS glue is a generated file, not a crate. `cargo xtask codegen`
writes it from `hd_host_abi`, and CI fails when regenerating changes it,
as D1's generated code does (§2.2).

## 12. Emitting From TIR

### 12.1 Analysis, Then One Emission Walk

`Emit(inst)` reads the instance's TIR from its module's mapped `tir`
entry. It may first run **bounded analysis passes** over the generic TIR
under the substitution, then it writes Wasm in one walk (Codex review,
I4). Some choices need facts about later instructions: whether a value
escapes in a later branch or through an inlined callback, which values
are live across a loop's back edge, which are live at a suspension
point. Deciding those during emission alone could be wrong.

- **Analyses.** Use counts, escape bits per allocation, and live sets
  per block are side tables in the worker's arena, indexed by `Inst`.
  Liveness is one backward dataflow over the structured blocks, iterated
  to a fixed point on loops. It is not a scan per suspension point
  (§14.2). Inlining decisions and the inlined callees' instructions join
  the analysed region before any choice is made.
- **Bounds.** Each pass is linear in the instance's instructions times
  the loop nesting depth, and runs only when the instance has an
  allocation, a closure, a loop with a candidate, or a suspension point.
  A trivial instance skips them.
- **What this is not.** The fixed decision forbids a monomorphized copy
  of the IR. Side tables and repeated reads of TIR are allowed.

The emission walk then walks the root block's list in order, with a
stack of open blocks. For each instruction it:

1. substitutes the instance's type arguments into the instruction's type
   (memoized per instance: each distinct generic type is substituted and
   laid out once);
2. picks the layout of the result (§15.1) and assigns Wasm locals;
3. writes Wasm with `wasm-encoder`, recording relocations (§13.8);
4. for a call, records the callee instance for collection's check
   (§13.2) and, when the callee is inlined, walks the callee's TIR in
   place with a remapping of its operands (§12.5).

Nothing is written back to TIR, and no instance IR exists. Per-instance
state is the substitution memo, the value-to-local map, the open-block
stack and the output buffer, all in the worker's arena, reset per
instance.

### 12.2 Lowering Rules

| TIR | Wasm |
| --- | --- |
| a value instruction | a Wasm local (or several, for a `multi` layout, §15.1) when used more than once or across a block; otherwise left on the operand stack |
| `LocalGet`, `LocalSet` | `local.get`, `local.set`; a `Shared` captured local is a cell, a one-field struct |
| `Block`, `If`, `Loop` | `block`, `if`, `loop`; `Break` and `Continue` become `br` |
| `Scope` with `Defer` | the exit ladder below |
| `Match` and its switches | `br_table` on a tag or a dense range; a binary search of `if`s for a sparse range of more than 8 cases; length then bytes for strings; each arm once, in nested blocks that leaves branch to |
| `Call` with an `Item` callee | `call`, relocated to the callee instance |
| `Call` with a `TraitMethod` callee | an `Impl` choice: a direct `call` of that impl's method. A `Bound` choice: the impl selected by head at the instance's types (§13.2); a direct `call`. A `TraitValue` choice: as `CallDyn`. A `Builtin` choice: the generated body (§13.6) |
| `Call` with an `Evidence` callee | a `call_ref` through the vtable stored in the variant (§13.5) |
| `CallDyn` | `struct.get` of the vtable slot, then `call_ref`, passing one vtable per evidence operand (§13.5) |
| `CallValue` | `struct.get` of the closure's code, then `call_ref` with the closure as the first argument |
| `CallHost` | an import call with the exchange-buffer codecs (§17.2) |
| `Closure` | a struct of the closure's environment: `Copy` and `Move` captures as fields, `Shared` ones as their cells; a closure with no capture is a constant global |
| `Coerce` | option wrap: a tag set or nothing (§15.2); to a trait value: a pair with a constant vtable; readonly view, variance, `Refine` and row subsumption: nothing |
| `Interp` | lengths summed first, one string allocated, parts copied; each `Display` part writes into the builder through its resolved callee |
| `DefaultCall` | a direct `call` of the default body's instance with the earlier argument values, inside the forbidden-context bracket (§12.3) |
| `ForRange`, `ForList`, `ForMap` | counted loops (§12.4) |
| `Await*` | the state machine (§14) |
| `Hook` | nothing, except in hook builds (§14.7) |

**The exit ladder.** Each `Scope` with a `Defer` gets one:

1. Every suite gets a "registered" flag local, set at its `Defer`.
   Suites registered unconditionally at the scope's top need no flag.
2. Every exit that leaves the scope (fall-through, `Break`, `Continue`,
   `Return`, a failing `?` leaf) stores its value and an exit code in
   locals, then branches to one cleanup block at the scope's end
   ([`flow.defer.result-saved`](../../spec/lang/06-control-flow.md#r-flow.defer.result-saved)).
3. The cleanup block runs the flagged suites, last in, first out, then
   dispatches on the exit code with a `br_table` to the real target.
4. A scope with one exit runs its suites there directly (the common case).

Panics skip the ladder: a panic is a trap and runs no suite
([`flow.defer.panic`](../../spec/lang/06-control-flow.md#r-flow.defer.panic)).
Cancellation runs the same suites of the same scopes through the frame's
cancel function (§14.6).

### 12.3 Facts, Defaults, Derives And Tests

- **Defaults run per call (Codex finding 4).** A default is a body of
  its own, and each call that omits the argument runs it through a
  `DefaultCall`, after the explicit arguments
  ([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)).
  D1's first-use getter evaluated a default once per program, which
  shared one mutable default between calls; it is gone. A constant
  default is inlined by the trivial-inlining test, so `= 10` costs
  nothing. Emission increments the forbidden-context counter around a
  `DefaultCall` whose body makes a call, so an indirect `block_on` or
  `println` there panics (suspension.md §14.9).
- **Facts are evaluated at compile time.** A fact, a metadata
  expression, and a variant's shared-data constructor with its defaults
  are evaluated once, at compile time
  ([`annot.fact.eval`](../../spec/lang/14-annotations.md#r-annot.fact.eval),
  [`annot.metadata.eval`](../../spec/lang/14-annotations.md#r-annot.metadata.eval),
  [`data.shared.compile-time`](../../spec/lang/08-data-and-enums.md#r-data.shared.compile-time)).
  The value becomes a constant global (wasm-layout.md §15.4); no fact
  has a run-time getter. The design (mine):
  1. **Where it runs.** `Collect` records every fact the program reads:
     through `facts_of`, `T::facts()`, a member handle's `info.facts`, or
     a shared-data access. Each one is an `EvalFact(fact)` task (§11.3),
     run in parallel after `Collect` and before `Emit` needs the value.
     `hd check` does not evaluate facts, as it does not collect
     (open question 23.1-8).
  2. **How.** A TIR interpreter in `hd_mono` walks the fact body's
     generic TIR under a substitution, as emission does, with values in
     an arena of its own. A body that is a tree of constants and
     constructors is folded without the interpreter.
  3. **What it may call.** Any hd function, method, trait method
     (selected by head, rule TS-6), closure and default body whose TIR
     is reachable, and the pure intrinsics: arithmetic, `Array`
     operations, string building, `TypeId`. Facts are requirement-free,
     so no provider exists and no `CallHost` is reachable. A suspension
     point, a host call, or a call that reaches `block_on` or `println`
     stops the evaluation.
  4. **Budget.** 10,000,000 interpreted instructions and 64 MiB of heap
     per fact, counted in language units, so the outcome is the same on
     every run (checking-and-tir.md §4.15). The limits are semantic and
     join `toolchain_key`.
  5. **Result.** The value is converted to a global pool constant
     (`Aggregate`, `Int`, `Str`, `ItemConst`). It may hold scalars,
     strings, data and enum values, tuples, lists, maps and capture-free
     functions. Shared structure stays shared, so a value that two
     fields reach is one global.
  6. **Failure.** A panic, an exhausted budget, a forbidden call, a
     cycle, or a value with no constant form (a closure with captures, a
     suspension, a handle) is `fact-evaluation-failed` (proposed). It is
     reported on the fact expression with the reason and, for a panic,
     the interpreter's backtrace. It is a build error.
  7. **Cache.** A `fact` entry holds the value as an entry-local
     constant row, under `H("fact", toolchain_key, fact stable path,
     sorted [(module path, tir key)] of the modules reachable from the
     fact's module in the use graph)`. The key is coarse and sound, as
     `prog_key` is. The value's content hash joins `link_key` (§13.10).
- **Derives.** Derive instances, including the generated `walk`,
  `describe` and `build`, are ordinary bodies (§4.13.9). With the
  walker's type known at each instance, every `w.member(h, value)` call is
  direct, so a derived `encode` is straight-line code
  ([`annot.walk.members`](../../spec/lang/14-annotations.md#r-annot.walk.members)).
  Member handles are constant globals (§15.4).
- **Test registrations.** A `TestCase` body evaluates its registration
  call's run-time arguments (`rows`, `timeout`, examples) and calls the
  std registration function, which drives the test body (§19.3).

### 12.4 Rows And Providers

Requirement rows never become type arguments
([`req.poly.one-body`](../../spec/lang/11-requirements-and-suspension.md#r-req.poly.one-body)).

- **One key order (Codex review, D1).** Every place that lists a row's
  keys positionally uses one comparator: the bytes of `canon(K)` (§13.3).
  That covers the parameters of a signature, the providers of a call,
  the key ids of a context, and the rows of the `tir` entry. In-run sets
  sort keys by `Ty` value for fast merges (§4.13.4), so positional data
  never relies on that order. TIR stores a call's providers as
  `(key, provider)` pairs (checking-and-tir.md, callee records), and
  emission sorts the pairs by this comparator once per call site.
- **A concrete row** (`$ Console + Clock`) adds one parameter per key,
  in that key order. Each is a trait value of the
  key's trait (§15.2). A call passes the providers TIR names for it, so a
  provider costs one Wasm argument per key and no allocation.
- **A row parameter** (`$R`) adds one **context** parameter (mine). A
  context is an immutable linked list of `(key id, provider)` nodes. Key
  ids are numbered at link time in the key order above. Looking up a key
  walks the list and takes the first match, which is how an inner
  `$.with` shadows an outer one.
- **Extension** (`$.with(Logger=...)` around a call that needs `R +
  Logger`) pushes one node: one allocation per call into row-polymorphic
  code.
- **A function value with a row** takes a context, because its caller may
  not know its row's keys. TIR's `ContextFor` builds it once per calling
  body.
- **Own providers.** A provider pair whose provider is `NONE` passes the
  enclosing sub-body's own provider for that key, exactly as
  `ProviderGet` would. M3 writes such pairs for calls to private
  callees whose rows were solved after checking (checking-and-tir.md,
  callee records).
- **Cold suspensions** capture their providers in the frame at
  construction ([`req.bind.construction`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)).

### 12.5 Counted Loops And Checks

`ForRange`, `ForList` and `ForMap` are emitted as counters in both tiers
(mine). A loop that allocates per step fails the `allocations` target, so
this is a lowering rule, not an optimization:

```text
ForRange a..b       i = a; end = b
                    loop: if i >= end: break; body; i = i + 1     # cannot overflow here

ForRange a..=b      i = a; end = b
                    if i <= end: loop: body; if i == end: break; i = i + 1

ForRange a..        i = a
                    loop: body; i = i + 1   # checked in debug and test, wraps in release
```

The `a..` step is a checked add with the loop's site
([`flow.for.range.from-overflow`](../../spec/lang/06-control-flow.md#r-flow.for.range.from-overflow)).
`continue` jumps to the increment. No range value, no iterator and no
`Option` exist at run time. `ForList` and `ForMap` loop over the backing
arrays' indices, capture the length at the start and compare it before
each step, panicking with `iterator-invalidated`
([`flow.for.invalidate.panic`](../../spec/lang/06-control-flow.md#r-flow.for.invalidate.panic)).

**Checks.** TIR holds no check sequences; emission adds them by profile.

| Check | Debug and test | Release |
| --- | --- | --- |
| integer overflow (`+`, `-`, `*`, unary `-`, narrowing conversions) | panics `integer-overflow` | wraps |
| `INT_MIN / -1`, `INT_MIN % -1` | panics `integer-overflow` | wraps; Wasm's `div_s` would trap, so release needs a test too |
| division by zero | Wasm traps; the trap maps to `integer-division-by-zero` | same |
| shift by the width or more | panics `invalid-shift` | same; Wasm masks the count |
| list index | std's index method compares with the length, `index-out-of-bounds` | same; the backing array is longer than the list |
| string byte index | the engine's array bounds check; the trap maps by code offset | same |
| use of a closed handle | panics with the use site | not emitted; the host provider still refuses a closed handle |

The overflow sequences are those of the research
([Debug-Tier Checks](research.md#debug-tier-checks)).
`i64` multiplication has no wide multiply in core Wasm, so its check
divides back, which costs about 20 cycles. The `release-check-cost` metric
watches it.

### 12.6 Tiers And Optimizations

The triage put the optimizing tier after the first release
([Pillar 3 features](goals.md#pillar-3-features-artifact-quality)).
D2 keeps one emission for both tiers (mine): `release-check-cost` asks
that the debug build cost at most 1.3x the release build, and any
optimization only one tier has counts against that ratio. The tiers
differ only where the spec or a first-release feature says so.

| Rule | Debug | Release | Status |
| --- | --- | --- | --- |
| counted loops (§12.5) | yes | yes | first release |
| `multi` layouts: `Option`, `Result`, tuples and trait values as several Wasm values (§15.1); decision B, extended to `Result` (owner, 2026-10-07) | yes | yes | first release |
| capture-free closures as constants | yes | yes | first release |
| constant folding and dead branches during the walk | yes | yes | first release |
| trivial inlining: the walk descends into a callee of at most 8 instructions with no loop, no suspension point and no closure | yes | yes | first release (mine) |
| bounded inlining: callees up to a size budget, and closures passed to a known callee, such as iterator adapters | yes | yes | first release (owner, 2026-10-07) |
| scalar replacement: a non-escaping closure, cell or small data value after inlining becomes locals; an escape analysis in the analysis passes of §12.1, before emission | yes | yes | first release (owner, 2026-10-07) |
| overflow checks | checked | wrap | spec |
| hook points (§14.7) | dropped | dropped | emitted only by hook builds (Later) |
| debug-only checks: closed handles, deadlock reports with frame lists (§14.8) | yes | no | first release |

Inlining walks the callee's generic TIR, mapped from its module's entry,
under the composed substitution. The inlined items' TIR hashes join the
instance's code key (§13.8). Escape facts and inlining decisions are
emission state, never written into TIR.

### 12.7 Emission-Time Checks

D1's TIR verifier (§4.13.11) holds before emission starts. During the
walk, emission asserts in the compiler's debug builds and in CI:

1. no `Param` type remains after substitution;
2. every `TraitMethod` callee resolves to exactly one impl at the
   instance's types, by head match alone (§13.2);
3. every suspension point gets a resume case and a cancel case (§14.2);
4. every relocation names a symbol in the program's instance, type, import
   or global sets (§13.10).

A failure is an internal error naming the instance, with exit status 101,
as a task panic is (§6.4). The linked module is then validated by
`wasmparser` (§15.7).

## 13. Monomorphization And Merging

Owner's answer 8: code per concrete type, then merge byte-identical
functions, the same in debug and release. Dictionaries exist only for
trait values, GADT evidence and the method-level bound evidence of a
`CallDyn` (§13.5).

### 13.1 Roots

| Program | Roots |
| --- | --- |
| executable or task (`hd run`, `hd build`) | the entry wrapper of `main` or `main!` (§16.2), and the init function of every group the entry module reaches |
| `hd build FILE` | the same, for FILE's module |
| unit test program (one module) | one `TestCase` body per registration, and the module's reachable init groups |
| integration test program (one file) | its `TestCase` bodies, and its reachable init groups |
| doc tests of a module | one `TestCase` body per doc test |
| REPL input | the input's top-level statements as an init body (§20.5) |

Only reachable items are compiled, so dead std code never reaches the
binary, and the import list holds only reachable host methods
([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)).

### 13.2 Collection

```rust
pub struct Instance { pub item: StablePathRef, pub body: BodyRef, pub ty_args: CanonTys }
pub struct InstanceSet {
    pub instances: Vec<(InstanceKey, Instance)>,  // sorted by key bytes at the end
    pub vtables: Vec<(CanonTy, TraitRef)>,        // (concrete type, trait) pairs
    pub imports: BTreeSet<HostMethodId>,
    pub types: BTreeSet<CanonTy>,                 // every type whose layout the program uses
}
```

Its columns, deduplicated by run IDs and sorted by instance key, are in
[data-structures.md §3.22](data-structures.md#322-codegen-the-instance-table-and-code-entries).

A worklist walk, as rustc's collector does:

1. Push the roots with no type arguments.
2. Pop an instance. Map its module's `tir` entry. Scan the body's `tags`
   column for calls, closures, coercions and host calls, substituting the
   instance's type arguments into each one's types.
3. For each `Item` callee, push the callee with the substituted
   arguments.
4. For each `TraitMethod` callee, read its choice. An `Impl` choice
   names the impl; substitute its arguments. A `Bound` choice becomes a
   concrete trait reference under the substitution, and the solver's
   `select` matches it **by head only** (trait-solver.md rule TS-6):
   no subgoal is solved, there is no depth limit and no fuel. Overlap
   is head-only and the checker proved the bounds generically, so at
   most one head matches. No match is an internal error (§12.7), never
   a diagnostic. Push the impl method with the impl's type arguments,
   or the trait's default body with `Self` set when the impl does not
   write the method. `select` is memoized per run, shared across
   programs. A `TraitValue` choice pushes nothing; a `Builtin` choice
   pushes its generated body (§13.6).
5. For each coercion to a trait value, each evidence value of a
   `NewVariant`, and each evidence operand of a `CallDyn`, select the
   impl by head as in step 4, record the vtable `(type, trait)` and push
   every method of the trait at that type, supertraits' vtables
   included. A method with its own type parameters is pushed as its
   erased instance (§13.5).
6. For each `DefaultCall`, push the default body with the call's type
   arguments.
7. For each `Closure`, push the closure body with the parent's
   arguments.
8. Record every `CallHost` as an import, every type whose layout an
   operation needs, and every fact the instance reads (§12.3).
9. Repeat until the worklist is empty.

The order of the walk never reaches output: the result is sorted by
instance key before anything is emitted (§6.5). Collection is serial per
program and takes milliseconds; many programs (a test plan) collect in
parallel.

### 13.3 Instance Keys

```text
canon(T)      = structural encoding of a type over stable paths, with no IDs
                (Prim | Adt(path, args) | Tuple(elems) | Optional(T) | Fn(params, result, row keys, suspends)
                 | Erased(evidence index) (§13.5) | ...)

instance_key  = H("inst", item stable path, sub-body index (closures),
                  [canon(arg) for each type argument])
```

The instance key is the instance's **symbol**: its logical name, which no
body edit changes. Relocations name it, collection dedups by it, and
folding and link order sort by it. The code entry key (§13.8) adds what
the emitted bytes depend on, the item's own TIR hash included. At link,
each symbol resolves to the code entry selected in this build.

An earlier draft put `tir_hash(item)` in the instance key. Then an edit
to a callee's body changed the symbol that its callers' cached code
relocates to, while the callers' code keys still hit. Link found a
missing or stale symbol (Codex review, finding 2). Keeping the body hash
out of the symbol fixes that without re-emitting every caller.

### 13.4 The Instantiation Depth Limit

Polymorphic recursion (a generic call that instantiates itself at a
growing type) has no finite instance set.

- **Depth** is measured on type arguments: the nesting depth of the
  deepest type argument of an instance. Its default limit is **32**.
  A type nests that deep only through a recursive instantiation chain;
  hand-written types stay far below it.
- **Chain length** is the shortest chain of instance requests from a root.
  Its default limit is **256**.
- The walk is breadth-first in content order, so the first instance over
  either limit is the same on every run.
- The diagnostic `instantiation-too-deep` (proposed, in D1's §4.15 style)
  points at the generic call that grows the type, and shows the first
  three and the last instance of the chain, with types printed in full.
- This is a build error, reported by `hd build`, `hd run` and `hd test`
  (owner, 2026-10-07: polymorphic recursion is an error, with no boxed
  fallback). `hd check` does not report it, since it does not collect.
  The one exception is a private row that grows by polymorphic
  recursion: M3 reports the same code at check time
  ([type-checking.md §5.5](type-checking.md#55-private-rows-and-the-m3-fixpoint)).

### 13.5 Dictionaries: Trait Values And GADT Evidence

- **Vtables follow the trait record's shape** (trait-solver.md §9.2,
  change 19). The shape is computed once per trait at interface time:
  one slot per method of the trait itself, in declaration order; one
  immutable reference per direct supertrait, in declared order, to that
  supertrait's own vtable at the same type; and a type id when the trait
  is `Inspectable` or extends it. Supertrait slots are never copied, so
  a diamond such as `Error < Display & Inspectable` shares one `Display`
  vtable, and the `Supertrait` coercion is one `struct.get`. Each vtable
  is a global with a constant initializer, built once per `(type, trait
  reference)` pair, so a coercion to a trait value never allocates a
  vtable.
- **Trait values** are pairs: the value as `anyref` and its vtable
  (§15.2). A call through a trait value is one `struct.get` and one
  `call_ref`.
- **GADT evidence.** A variant whose existential parameter has bounds
  stores one vtable per bound as hidden fields of the variant struct,
  filled at construction, where the concrete type is known
  ([`gadt.runtime.evidence`](../../spec/lang/13-gadts.md#r-gadt.runtime.evidence)).
  The existential payload is erased to `anyref`. The matching arm's code
  calls through the stored vtables, so it is compiled once, not once per
  hidden type.
- **Generic methods called through a trait value.**
  [Dynamic Safety](../../spec/lang/09-traits.md#dynamic-safety) allows a
  method-level type parameter on a dynamically safe trait whatever its
  bounds, and accepts any type argument for it
  ([`types.trait.safe.method-type-arg`](../../spec/lang/04-type-system.md#r-types.trait.safe.method-type-arg)).
  Examples are `Error.find[T < Error]`, called with enum errors, and
  `Inspectable.downcast[T]`. A vtable slot cannot hold one body per
  concrete `T`, so these methods use **erased instances**, the
  dictionary passing that the spec's
  [Shapes and Generic Code](../../spec/lang/04-type-system.md#shapes-and-generic-code)
  describes:
  1. The vtable slot holds the impl method instantiated at its concrete
     self type, with each method type parameter replaced by
     `Erased(i)`. A value of type `Erased(i)` has the `erased` layout,
     `anyref`. There is no `AnyRef` restriction: a value-typed argument
     is boxed at the call, and the box has no identity, so the boxing is
     not observable. Open (S1c): a `T` inside a container parameter, such
     as `mut List[T]` with a packed `List[i32]`, cannot be boxed in place.
  2. The erased instance takes one extra parameter per bound of each
     method type parameter: that bound's vtable at the caller's `T`.
     `T < Inspectable` passes the type id that vtables carry. Operations on
     a `T` value inside the body call through these parameters, as GADT
     arms do.
  3. A call from an erased instance to another generic item passes
     `Erased(i)` on as a type argument. The callee gets its own erased
     instance, which takes the same evidence parameters. This chain is
     bounded by the program's call graph, so it cannot grow types.
  4. **The caller** is an ordinary monomorphized instance, so its `T` is
     concrete. `CallDyn` carries the method's type arguments and one
     evidence operand per method-level bound, chosen by the checker at
     the call (checking-and-tir.md, `CallDyn`). Collection turns each
     operand into an impl by head match at the concrete `T`, as for a
     `TraitMethod` choice (§13.2 step 4), and records the vtable
     constant. A `Bound` operand in generic code becomes concrete the
     same way. Emission passes the vtables, upcasts reference `T`
     arguments to `anyref`, and boxes value-typed ones. When the result
     type mentions `T`, emission adds one `ref.cast` or unboxing to the
     concrete type, which cannot fail. The slot's
     type is the erased instance's signature: the receiver, the erased
     arguments, then one vtable parameter per bound, in the method's
     bound order (`dyn_bounds` of the shape).
  5. Collection pushes the erased instance of every generic method when
     it records a vtable `(type, trait)` (§13.2, step 5).
- **Everything else is static.** A call through a bound on a type
  parameter is a direct call in every instance. Erased instances exist
  only behind a vtable slot and in their own callees.

### 13.6 Tuples, Arity And `all!`

There are no variadic generics (§4.13.7). Tuples are ordinary types:

- `Args < Tuple` instantiates like any type parameter, so `f(args...)`
  is one instance per tuple type.
- Tuple `Eq`, `Ord`, `Hash` and `Debug` are instances of std's tuple
  templates (trait-solver.md change 20). The checker records an `Impl`
  choice of the template, and collection instantiates the template's
  instance per tuple type, like any impl. A user library's tuple
  template works the same way. No body is generated per arity.
- Only sealed traits are `Builtin` (`Any`, `AnyVal`, `AnyRef`,
  `Inspectable`, `Tuple`, `Num`, `Integer`, `Float`, `Suspend`). Those
  with methods get a body that D2 generates per primitive or shape, such
  as `type_id` and `downcast` for `Inspectable`.
- `all!` is one intrinsic frame instance per tuple of child result types
  (§14.5). `race!` is an ordinary generic intrinsic over `T`.

### 13.7 Merging Byte-Identical Functions

**Where merging comes from.** Each instance is emitted with exact Wasm
types (§15.2). Two instances fold when their emitted bytes are equal after
relocation. Equal bytes arise from:

- type arguments with the same Wasm layout: `List[u32].push` and
  `List[i32].push`, `Option[char]` and `Option[u32]` helpers;
- data types with the same field layout. Wasm GC canonicalizes types
  structurally, and hd needs no nominal Wasm types (§15.3), so `data A:
  x: i32` and `data B: x: i32` are one Wasm type, and their instances
  fold;
- type parameters that the body does not use in any operation;
- identical small std helpers at different types.

`List[Point].push` and `List[User].push` fold only when `Point` and
`User` have the same field layout. The research expected them to fold
always; that needs erased element storage, which costs a cast per read
(§23.2, inconsistency 1).

**The algorithm (mine, after safe ICF in linkers; Codex re-review N4
and N-S3).** The first release folds exact duplicates only:

1. **Fold key.** `H(canonical signature, local declarations, body bytes,
   every relocation as (offset, kind, exact target), every site record
   as (offset, kind, stable span))`. The signature comes first: an
   `i32 -> i32` identity and an `i64 -> i64` identity have equal
   instructions and must not fold. Function targets are instance keys,
   global targets are global symbols (a vtable, a fact, a string
   literal), so two bodies that reference different globals never fold.
   Site records are in the key because a panic's reported location is
   program output. Statement lines (`lines`) are not: they only feed
   backtraces, which name the representative and its aliases.
2. **Classes, bottom up.** Instances with equal fold keys form one
   class. Then each function target is replaced by its target's class
   representative, keys are recomputed, and classes merge again, until a
   round merges nothing. This is the pessimistic direction: it merges
   only bodies already proven equal, so `List[u32].push` and
   `List[i32].push` fold once their `Array` callees have folded. Rounds
   are bounded by the call graph's depth and are usually two or three.
3. The representative of a class is the member with the smallest instance
   key. The others become aliases.
4. The fold list (representative, then aliases in key order) goes into
   the `hd.folds` custom section, so a backtrace can say "also
   `List[Point].push`" (§15.5).

Mutually recursive copies never fold in this scheme, since each waits
for the other. Optimistic partition refinement, which would fold them,
comes only if the `dead-code` or tiny-size measurement shows real bytes
left on the table.

Every step works on content keys, so the result does not depend on
threads or order. Merging runs in `Link`, in both tiers.

### 13.8 Code Entries

`Emit(inst)` lowers one instance to Wasm with `wasm-encoder` and stores a
`code` entry:

```rust
pub struct CodeEntry {
    pub body: Box<[u8]>,                  // locals and instructions; index immediates as 5-byte padded LEBs
    pub relocs: Box<[Reloc]>,             // sorted by offset
    pub sites: Box<[SiteRecord]>,         // (offset, site kind, stable span) for traps and explicit panics
    pub lines: Box<[(u32, StableSpan)]>,  // offset -> statement span, for backtraces
    pub sig: CanonSig,                    // Wasm signature, as canonical types
}
pub enum Reloc {
    Func { at: u32, target: FuncTarget },     // InstanceKey | Import(HostMethodId) | Intrinsic helper
    Type { at: u32, ty: CanonWasmTy },        // a canonical Wasm type descriptor
    Global { at: u32, global: GlobalSym },    // module storage, constants, vtables, string literals
    Data { at: u32, bytes: Hash128 },         // a passive data segment, by content
    Site { at: u32, local: u32 },             // a site number, renumbered globally at link
}
```

On disk a relocation is `(at, kind, target)` with `target` indexing the
entry's deduplicated target table
([data-structures.md §3.22](data-structures.md#322-codegen-the-instance-table-and-code-entries)).

```text
code_key = H("code", toolchain_key, tier, instance_key, tir_hash(item),
             sorted [(stable path, per-item interface hash)] of every item its TIR names,
             sorted [layout_hash(T)] of every type the instance lays out,
             sorted [(impl stable path, impl interface hash)] of every impl collection
                    selected for it, for calls and for associated-type projections,
             sorted [(callee instance_key, inline_summary(callee))] of every direct callee,
             sorted [(stable path, tir_hash)] of every item inlined into it)

layout_hash(T)       = H(canonical Wasm layout descriptor of T, and of every type
                         reachable through its fields), memoized per type per run
inline_summary(f)    = H("no-inline") when the trivial-inlining test (§12.6) fails on f's TIR,
                       else H("inline", tir_hash(f)); stored per item in the `tir` entry
```

**The dependency set is what collection and emission read, not what the
generic TIR names.** Generic TIR does not name the concrete types,
projected associated types and impls that appear under a substitution
(Codex review, A4). Collection already finds all of them for this
instance before `Emit` runs, so the key is computed from collection's
record of the instance:

- **Type arguments and laid-out types.** `layout_hash` covers a field
  added to `User` even when `User` reaches the instance only as a type
  argument of an unchanged generic body.
- **Selected impls.** A trait call or a projection resolved at the
  concrete type names the selected impl. Its interface hash covers its
  method signatures and associated-type bindings.
- **Callees.** A call is a relocation to the callee's symbol, so a
  callee's body does not reach the caller's bytes. Its signature does,
  through its interface hash, and so does the decision whether to inline
  it. `inline_summary` changes only when a callee's body changes and the
  callee is or becomes small enough to inline. So a body edit to a large
  callee re-emits no caller.
- **Runtime helpers** are generated by the compiler, so `toolchain_key`
  covers them.

The key completeness tests (§21.4) cover each of these reads. Any new
read that emission adds must join this list in the same change. Verify
mode then checks it on every hit (§5.6).

- An edit to one private function re-emits its own instances, plus the
  instances that inlined it, plus callers for which it became inlinable
  or stopped being inlinable. Every other code entry is shared by `main`,
  the tests and other worktrees.

### 13.9 What Instances Cost

| Cost | Control |
| --- | --- |
| number of instances | reachability; folding; rows are not type arguments |
| emission time | one walk of the generic TIR per instance; cached by `code_key` |
| type section size | structural canonicalization and dedup at link (§15.3) |
| Cranelift time | wasmtime's per-function cache (§18.2) |

A per-item instance budget stays a fallback, as the research says. Add it
only if the `dead-code` metric fails after folding.

### 13.10 The Link Step

`Link(P)`:

1. **Drop and fold.** An instance that no relocation names, because every
   caller inlined it, is dropped. Then the rest fold (§13.7).
2. **Order functions.** Imports first, sorted by module and name. Then
   the generated runtime helpers, sorted by name. Then the representatives
   in instance-key order. This order is deterministic, but it does not
   keep indices stable: inserting a function with a smaller key renumbers
   every later one (Codex re-review N-D2). Slice 6 measures wasmtime's
   per-function cache hits after insertions, deletions and type-section
   changes; a persistent index allocation is added only if that
   measurement asks for it.
3. **Types.** The canonical types the functions, globals and exports name
   (§15.3).
4. **Globals.** Constants (vtables, closures without captures, payloadless
   variant singletons, facts, member handles, short string literals),
   module storage, then the runtime's globals (§15.4).
5. **Data.** One passive segment per distinct string literal, by first
   reference in function order.
6. **Elements.** One declarative segment for every function used with
   `ref.func`. The reserved hot-reload table stays empty (§18.5).
7. **Exports**: `hd.init`, `hd.poll`, `hd.wake`, the exchange buffer, and test entries (§14.4, §15.4, §19.2).
8. **Code.** Copy each representative's body and patch its relocations in
   place. In a release build, re-encode the padded LEBs at minimal width
   (mine); the debug build keeps them, since size does not matter there.
   Re-encoding moves every later byte, so the encoder builds an
   old-to-new offset map per body as it goes (a sorted list of
   `(old offset, bytes removed so far)`, one row per shrunk immediate)
   and rewrites the offsets of `sites`, `lines` and the source map
   through it before writing `hd.sites` and `hd.lines` (Codex re-review
   N-B8). A test panics after a shrunk immediate in release and checks
   the reported site against debug.
9. **Custom sections**: `name`, `hd.runtime`, `hd.sites`, `hd.lines`,
   `hd.folds` (§15.5, §16.4).

```text
link_key = prog_key (§11.3), and on a prog_key miss
           H("link", toolchain_key, tier, profile, root description, sorted code keys,
             sorted [(fact stable path, value hash)] of the facts the program reads)
```

A program is written under both keys, so the next warm run hits the
cheap one. Link is serial per program and linear in output size.
