# New Compiler Design: Back Half, Codegen

Part of the [compiler design](README.md).

## Part D2: The Back Half

**Second pass** (backend lane, 2026-10-07), from Codex review items 18 to 20 and 22
([trait-solver.md §16.4](trait-solver.md#164-changes-needed-in-type-checkingmd-and-the-other-design-files)), and the owner's answers of the day: collection
selects impls by a head match plus `Bind` steps, with no proof
(§13.2); vtables follow the trait record's shape (§13.5); tuple traits
are template instances (§13.6); defaults run per call and facts are
lazily initialized globals (§12.3, owner, 2026-10-07); `Result` uses `multi` layouts, and
bounded inlining and scalar replacement are first-release (§12.6);
polymorphic recursion is an error (§13.4).

**Lowering pass** (2026-10-07), from the two representation studies
and the orchestrator's calls; the per-type and per-form costs are in
[lowering-catalog.md](lowering-catalog.md): stable, type-only numbering
(§12.4, §13.8, §13.10); erased storage A1 applied at collection (§13.2,
§13.3); collection skips always-inlined callees (§13.2); closure
specialization and known-vtable devirtualization (§12.6); a size versus
speed policy with budgets and size guards (§12.8); merging by worklist
(§13.7); per-program packs (§13.10). Each optimization is a named pass
with its inputs, outputs and cost. Which passes run where follows the
tiering decision ([tiering.md](tiering.md), owner, 2026-10-07): a dev
pipeline and an optimized pipeline (§12.6).

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
 ModuleFinish(m) ═══════════════════════════════════► [check entry, TIR sections]  per module (D1)
                                  │ generic TIR, per-item TIR hashes, dependency lists
 program root(s) ─────────────────┤  (entry, test registrations, REPL input)
                                  ▼
                       Collect(P): reachability over TIR + impl tables
                                  │ instance set {(item, type args)}, content-ordered
                                  ▼
                       Emit(group): emit every missed instance in one folder group,
                                    choose layouts, write Wasm ═════════► [codepack]     one per folder group, parallel
                                  │ body bytes + symbolic relocations
                                  ▼
                       Link(P): fold identical bodies, assign indices,
                                build types/globals/data, patch relocations ═► [link entry] = Wasm bytes
                                  │
            ┌─────────────────────┴──────────────────────────┐
            ▼ native (hd_run_wasmtime)                        ▼ browser (hd_web + generated JS glue)
   Precompile(P, pipe) ═► [cwasm entry]             program worker: WebAssembly.compile
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
| Collect | program roots, TIR of reachable modules, impl tables | the instance set, the import set, the type set; code-entry hits and folder groups with misses | program | serial per program; programs in parallel | inside `link` | none |
| Emit | one folder group's missed instances: generic TIR, substitutions, and callees' TIR | Wasm body bytes, relocations, site and line records | folder group (`codepack`) | groups in parallel | `code` entries in `codepack` | §13.8 |
| Link | code entries of the instance set | one Wasm module with its custom sections | program | serial per program; fold hashing in parallel | `link` | §13.10 |
| Precompile | Wasm bytes, engine config | wasmtime's serialized module | program and pipeline | Cranelift compiles functions in parallel | `cwasm` | `H("cwasm", hash of the Wasm, wasmtime version, config hash, target)` |
| Instantiate and run | precompiled module, host | outcome, output | run or test case | test cases in parallel | no | none |

There is no MIR stage: the checker's TIR is the input of `Collect` and
`Emit` (§3.9.1).

### 11.3 Tasks

D2's tasks are `TaskKind::Ext` tasks (§6.1):

```rust
pub enum ExtTask {
    Collect(ProgramId),          // after the tir entry of every module the program can reach
    Emit(GroupId),               // created by Collect, one per folder group with a miss
    Link(ProgramId),             // after every Emit of its program
    Precompile(ProgramId, PipelineId), // native only; the pipeline gives the engine config
    RunCase(ProgramId, CaseIdx), // test runner (§19); the browser runs cases in its worker instead
}
```

- `Collect` starts only after the `tir` entries of every module that the
  program's roots reach in the module use graph exist, and after the
  `HeaderCheck` task of every folder they reach (scheduler.md §6.1). That set comes
  from the manifest's use lists, so it is known before any check runs.
- `Collect` looks up every code entry itself. It reuses hits and creates
  one `ExtTask::Emit(GroupId)` for each folder group with at least one
  miss. The task emits that group's missed instances as a batch, and the
  group is the `codepack` unit of cache.md §5.2
  ([reconciliation, item 5](reconciliation.md#design-changes-proposed)).
- **The program fast key (mine; keyed by TIR content since the systems
  review, finding 4).** Before `Collect`, D2 computes

  ```text
  prog_key            = H("prog", toolchain_key, pipeline_hash, profile, root description,
                          sorted [(module path, tir_content_hash(m))] of the modules
                          reachable in the use graph)
  tir_content_hash(m) = H(sorted [(item path, TIR hash, inline summary, dependency list)]
                          of m's items, m's literal list (§13.8))
  ```

  A hit names the `link` entry directly. Collection, emission and
  linking are skipped, and a warm `hd run` or `hd test` reads one entry
  per program. Module-level reachability is coarser than item-level, so a
  hit is always sound.
  - **Content, not check keys.** An earlier draft used each module's
    `tir` key, which held `check_key(m)` and so `source_hash(m)`. Then a
    comment, a blank line, a doc comment or a `tests:` edit missed every
    program that reaches the module: each one collected, linked and ran
    the Cranelift lookup pass again, for identical bytes. Now such an
    edit rechecks one module, which writes an equal content hash, and
    every program hits.
  - **Why positions may stay out.** A TIR hash excludes the span columns
    (checking-and-tir.md, "Lifetime And The `tir` Entry"), and code holds
    no source position: panic sites and statement lines are anchors to a
    TIR instruction, resolved against the current sources only when
    printed (§13.8, "Positions"). So moving code down a file changes no
    program byte that a run depends on.
  - **Cost** (`ordinary-10k`; 50 test programs reach the edited module
    under the old per-module test plan, one package program under §13.1's):

    | Edit | Old key: work per edit | Content key |
    | --- | --- | --- |
    | comment or blank line | every reaching program: collect (about 2 ms), 800 to 4,800 code lookups, link, the Cranelift lookup pass (12 to 90 ms) | one module recheck; every program hits |
    | `tests:` block | as above | the module's overlay; only the package's unit-test program relinks |
    | private body, TIR unchanged (a renamed local) | as above | as for a comment |
    | private body, TIR changed | as above | one or two code entries; the reaching programs relink from packs |
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
| `hd_run` | the embedding API: `Engine`, `Host`, `Provider`, grants, the poll/wake driver logic, panic decoding, the test runner core | `hd_wasm`, `hd_cache` ([reconciliation, item 8](reconciliation.md#design-changes-proposed)) | yes, without engines |
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
  the loop nesting depth. Analyses are passes in a pipeline (§12.6,
  [tiering.md §6](tiering.md#6-a-pass-manager-for-hd)), each gated by the
  instance's summary bits: an allocation, a closure, a loop with a
  candidate, or a suspension point. A trivial instance skips them. The
  dev pipeline runs only suspension liveness.
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
| `Match` and its switches | `br_table` on a tag or a dense range; a binary search of `if`s for a sparse range of more than 8 cases; length then the viewed bytes for strings; each arm once, in nested blocks that leaves branch to |
| `Call` with an `Item` callee | `call`, relocated to the callee instance |
| `Call` with a `TraitMethod` callee | an `Impl` choice: a direct `call` of that impl's method. A `Bound` choice: the impl that `select` picks at the instance's types (§13.2); a direct `call`. A `TraitValue` choice: as `CallDyn`. A `Builtin` choice: the generated body (§13.6) |
| `CallDyn` | `struct.get` of the vtable slot, then `call_ref`; a generic method also gets its witness global (§13.5.1). When the vtable is a known constant global after inlining, a direct `call` of the slot's function, which the inliner may then inline (§12.6) |
| `CallValue` | `struct.get` of the closure's code, then `call_ref` with the closure as the first argument |
| `CallHost` | an import call with the exchange-buffer codecs (§17.2) |
| `Closure` | a struct of the closure's environment: `Copy` and `Move` captures as fields, `Shared` ones as their cells; a closure with no capture is a constant global |
| `Coerce` | option wrap: a tag set or nothing (§15.2); to a trait value: a pair with a constant vtable; `Supertrait`: the payload unchanged and `struct.get` of the parent's vtable field from the child's vtable (§13.5); readonly view, variance and row subsumption: nothing (§12.4) |
| `Interp` | one builder sized by the literal parts' bytes; literal parts copied from their pooled literal; each `Display` part writes into the builder through its resolved callee; a string part copies its viewed bytes; one exact-size array at the end, viewed from start 0 |
| a string literal | a view over its pooled array (wasm-layout.md §15.2): `i32.const` of its module-local number and a `call` of its module's literal getter, or `global.get` of an `array.new_fixed` constant at 4 bytes or less, then `i64.const` of its span; a literal passed straight to a host call takes the host fast path (wasm-layout.md §15.4) |
| an explicit panic, a failed check | a `call` of the category's stub with no site immediate; the call's code offset is the site (wasm-layout.md §15.5) |
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
- **Facts are lazily initialized globals (owner, 2026-10-07).** A fact,
  a metadata expression, and a variant's shared-data constructor with
  its defaults are ordinary bodies, run once on first read
  ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy),
  [`annot.metadata.eval-as-fact`](../../spec/lang/14-annotations.md#r-annot.metadata.eval-as-fact),
  [`data.shared.eval-as-fact`](../../spec/lang/08-data-and-enums.md#r-data.shared.eval-as-fact)).
  Each read (`facts_of`, `T::facts()`, a member handle's `info.facts`, a
  shared-data access) is a getter: a mutable global plus an "initialized"
  flag, filled by the body's instance on the first read. Readers `call`
  the getter and never name the storage global, so a new fact elsewhere
  changes no reader's bytes (§13.10); a body calls each getter once on a
  dominating path and reuses the result. The getter runs
  the body inside the forbidden-context counter (suspension.md §14.9), and
  a panic in it is reported as `fact-evaluation-failed` with the original
  category. A fact that nothing reads is never collected, so it is never
  emitted. There is no compile-time evaluator, no evaluation budget, no
  `fact` cache entry and no value-graph serialization; a changed fact
  body changes only its own instance's code key.
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
  context is an immutable linked list of `(key id, provider)` nodes. A
  key id is the first 64 bits of `H(canon(K))`, an `i64` (lowering pass;
  an earlier draft numbered keys densely at link, which changed every
  context-using body when a key was added anywhere). Link checks the
  program's key ids for collisions and, on one, rehashes every id with
  the next fixed salt. Looking up a key
  walks the list and takes the first match, which is how an inner
  `$.with` shadows an outer one.
- **Extension** (`$.with(Logger=...)` around a call that needs `R +
  Logger`) pushes one node: one allocation per call into row-polymorphic
  code.
- **One callable ABI for function values (Codex re-review N-I2).** Every
  closure's code has the signature `(env, args..., ctx: (ref null
  $Ctx)) -> results`, whatever its row; an empty row ignores `ctx`, and
  a caller with nothing to pass passes null. A function value's caller
  may not know the value's row, and a context is looked up by key id,
  so any context that holds `R1`'s keys serves a function of row `R1`.
  Row subsumption `fn $ R1` to `fn $ R2` is therefore no instruction,
  and the closure reference types of both are the same `$Fn_sig`, where
  the signature leaves the row out. A named function used as a value
  gets one generated adapter that reads its keys from `ctx` and calls
  the function's direct entry, which takes one parameter per key. TIR's
  `ContextFor` builds the context once per calling body.
- **Own providers.** A provider pair whose provider is `NONE` passes the
  enclosing sub-body's own provider for that key, exactly as
  `ProviderGet` would. M3 writes such pairs for calls to private
  callees whose rows were solved after checking (checking-and-tir.md,
  callee records).
- **Cold suspensions** capture their providers in the frame at
  construction ([`req.bind.construction`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)).

### 12.5 Counted Loops And Checks

`ForRange`, `ForList` and `ForMap` are emitted as counters in both pipelines
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
| string byte index | compares with the view's length, `index-out-of-bounds`; the array may be longer than the view, so the engine's check is not enough | same |
| use of a closed handle | panics with the use site | not emitted; the host provider still refuses a closed handle |

The overflow sequences are those of the research
([Debug-Tier Checks](research.md#debug-tier-checks)).
`i64` multiplication has no wide multiply in core Wasm, so its check
divides back, which costs about 20 cycles. The `check-cost` metric
(the optimized pipeline, checks on against off) watches it.

### 12.6 Tiers And Optimizations

**Decided (owner, 2026-10-07; [tiering.md](tiering.md)).** There are two
pipelines, and the pipeline is separate from the profile:

- The **profile** is observable: checked (debug, test) or wrapping
  (release). It decides overflow and shift checks and the debug-only
  checks ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
- The **pipeline** is not observable: dev or optimized. It decides which
  optimization passes run and the Cranelift setting. A program's output,
  panic categories and sites, effect order, `is` and type ids are the
  same in both (tiering.md §4).
- `hd run`, `hd test` and the REPL use the **dev pipeline**: almost no
  hd passes, then Cranelift `None` with the single-pass register
  allocator if spike T1 confirms it (engines-and-test-runner.md §18.6).
- `--release` selects the **optimized pipeline**: bounded inlining,
  closure specialization, devirtualization and scalar replacement, then
  Cranelift `Speed`. On `hd build`, `hd run` and `hd FILE` it also
  selects the release profile; `hd test --release` keeps the test
  profile.
- Bounded inlining, scalar replacement, closure specialization and
  devirtualization run **in the optimized pipeline only**. This reverses
  the earlier "the same in debug and release" rule. The `runtime` and
  `allocations` targets measure optimized artifacts, so they still apply.
- What both pipelines share: the layouts (one per type), counted loops,
  `multi` layouts, the literal pool, stable numbering, folding during the
  walk and merging at link. Binaryen in the optimized pipeline is decided
  after spike T2.

Each optimization in this table is a named pass with its inputs, outputs
and cost in [lowering-catalog.md](lowering-catalog.md#optimization-passes);
the baseline emission without any pass is defined there too.

| Rule | Dev pipeline | Optimized pipeline | Status |
| --- | --- | --- | --- |
| counted loops (§12.5) | yes | yes | first release |
| `multi` layouts: `Option`, `Result`, tuples and trait values as several Wasm values (§15.1); decision B, extended to `Result` (owner, 2026-10-07); one representation per type in every position, boxed over the bound (lowering pass) | yes | yes | first release |
| capture-free closures as constants | yes | yes | first release |
| constant folding and dead branches during the walk (a fact's value is a run-time read, §12.3) | yes | yes | first release |
| constant hoisting: an enum or tuple box whose payloads are constants is an immutable global, so `.Err(ParseError.Empty)` never allocates | yes | yes | first release (lowering pass) |
| trivial inlining: the walk descends into a callee of at most 8 instructions with no loop, no suspension point and no closure | if spike T4 keeps it (default on) | yes | first release (mine) |
| bounded inlining: callees up to a size budget, and closures passed to a known callee, such as iterator adapters (budgets in §12.8) | no | yes | first release (owner, 2026-10-07) |
| scalar replacement: a non-escaping closure, cell, box or data value after inlining becomes locals, mutable data included (an `Iterator` whose identity no one observes); an escape analysis in the analysis passes of §12.1, before emission | no | yes | first release (owner, 2026-10-07) |
| known-vtable devirtualization: a `CallDyn` on a value whose vtable is a constant global becomes a direct call | no | yes | first release (lowering pass) |
| closure specialization: inline, scalar-replace, devirtualize, repeated (below) | no | yes | first release (lowering pass) |
| overflow and shift checks | by profile | by profile | spec: checked in debug and test, wrapping in release |
| hook points (§14.7) | dropped | dropped | emitted only by hook builds (Later) |
| debug-only checks: closed handles, deadlock reports with frame lists (§14.8) | by profile | by profile | first release: on in debug and test, off in release |

Inlining walks the callee's generic TIR, mapped from its module's entry,
under the composed substitution. The inlined items' TIR hashes join the
instance's code key (§13.8). Escape facts and inlining decisions are
emission state, never written into TIR.

**Closure specialization (lowering pass).** An iterator adapter stores
its closure in a field of a mutable `Iterator` struct, so inlining alone
never reveals the `call_ref` target. Specialization is a schedule of the
named single passes of
[lowering-catalog.md](lowering-catalog.md#optimization-passes), one round
per stage of a chain:

1. **`inline-bounded`** the adapter call (`map`): its body builds a
   closure and an `Iterator`.
2. **`escape`, then `scalar-replace`** the `Iterator`: it does not
   escape, so its `step` field becomes a local holding a known closure
   literal.
3. **`devirt-closure`:** a `call_ref` whose target is a known closure
   literal becomes a direct call of that body, with the environment
   fields as locals; **`devirt-vtable`** does the same for a `CallDyn` on
   a known vtable.
4. **`inline-bounded`** that body, then `scalar-replace` its environment
   and cells.

Rounds repeat to a fixed point, at most 4 by default, and stop at the
caller's size cap (§12.8). Without a closure literal or a known vtable
the indirect call stays. `xs.iter().map(fn x: x + 1).sum()` then becomes
a counted loop with no allocation. The budget is local to the caller:
callee size, the caller's size so far and nesting depth. A program-wide
budget would make one function's bytes depend on others.

### 12.7 Emission-Time Checks

D1's TIR verifier (§4.13.11) holds before emission starts. During the
walk, emission asserts in the compiler's debug builds and in CI:

1. no `Param` type remains after substitution;
2. every `TraitMethod` callee resolves to exactly one impl at the
   instance's types, by `select`'s head match and `Bind` steps (§13.2);
3. every suspension point gets a resume case and a cancel case (§14.2);
4. every relocation names a symbol in the program's instance, type, import
   or global sets (§13.10).

A failure is an internal error naming the instance, with exit status 101,
as a task panic is (§6.4). The linked module is then validated by
`wasmparser` (§15.7).

### 12.8 Size Versus Speed Policy

Runtime performance has two axes, code speed and code size (owner,
2026-10-07). Speed scales with allocations, indirect calls, casts and
pointers per live object. Size scales with distinct instances, inlined
copies and constant-building code, and it feeds download, instantiation,
engine compile time, and the `size-startup-heap` and `dead-code` targets.
This policy comes from the runtime study's section 9
([representation-runtime.md](representation-runtime.md#9-size-versus-speed)).
Rule 1 is representation and holds in every pipeline. Rules 2 to 4 say
how to set each pass's budget wherever it runs. Rule 5 says which
pipeline runs the passes (§12.6).

**Policy.**

1. **Layouts are the same in every pipeline.** A layout is part of every
   instance's code; two layouts would double the cache, make a dev run
   measure a different program, and break `check-cost` and `dev-speed`
   comparisons.
2. **Speed where it is hot, size everywhere else.** Specialize and inline
   inside loops and for closure literals; everything else is a shared,
   folded call.
3. **Fold always.** Folding costs no speed, and A1 makes it work for
   reference element types.
4. **Budgets are per function**, local to the caller, so one hot spot
   cannot blow up a module and one function's bytes never depend on
   another's (§12.6).
5. **The optimized pipeline runs every pass; the dev pipeline runs the
   baseline emission** plus suspension liveness, folding during the walk
   and, if spike T4 keeps it, trivial inlining (owner, 2026-10-07;
   [tiering.md](tiering.md)). The budgets below apply in the optimized
   pipeline. `dev-speed` (dev at most 4x optimized, geomean, no case over
   10x) guards how slow dev code may get.

**Budgets.** Defaults for each pass's parameters, decided by the
experiments named:

| Feature | Speed it buys | Size it costs | Budget, wherever the pass runs |
| --- | --- | --- | --- |
| trivial inlining | a call per small callee | none or negative | callee at most 8 TIR instructions, no loop |
| bounded inlining | a call, plus later folding | callee size per site | callee at most 40 TIR instructions inside a loop, 15 outside; the caller grows at most 2x or 2 KB, whichever is smaller; an absolute caller cap set by S6 below the size where Cranelift's time per byte doubles |
| closure specialization | 1 to 5 `call_ref`s per element on wasmtime | 50 to 150 bytes per stage | at most 4 rounds; at most 512 bytes of growth per chain site |
| known-vtable devirtualization | one `call_ref` | none; enables inlining | always |
| scalar replacement | an allocation | usually smaller | always, when the value does not escape |
| A1, erased reference storage | a cast per element read lost | negative: one instance per class | always, unless E1 rejects it |
| parallel arrays for value-layout elements | an allocation per element | about 25 bytes per slot per list method | up to the bound (E2) |
| literal pool | none | negative above about 5 bytes per literal | always above the `array.new_fixed` threshold (E10) |
| erased-ABI thunks | the owner accepted a slow `dyn` | about 25 bytes each | warn past 200 per method |

E8 keeps these budgets unless the next level up buys 5 percent speed on
the runtime suite for under 10 percent size on the `dead-code` packages;
a budget that buys less than 5 percent speed for more than 10 percent
size is cut. "Size" there means Wasm bytes, Cranelift compile time at
both levels, V8 Liftoff and TurboFan compile time, and instantiation
time.

**Size guards.**

- The inliner's budgets above, including the caller cap.
- A thunk count per `dyn` generic method, with a warning past 200 for one
  method. If the guard fires in std or the examples, outlining a whole
  basic block of open instructions per thunk (§13.5.1, "Later") is the
  first fix.
- **`hd build --size-report`** prints bytes per section (code, types,
  `hd.names`, `hd.sites`, `hd.lines`, data), the largest functions with
  their instance counts, folded bytes, thunk counts per method, and code
  bytes per source module. It reads the linked module and its custom
  sections, so it costs nothing in a normal build.

**What the policy buys** in a pipeline that runs every pass, as a release
build will (estimates; spike 0c and the slices measure):

| Target | Without these rules | With them |
| --- | --- | --- |
| `runtime` (geomean at most 1.5x Node, no case over 3x) | `map` above 3x on wasmtime from the hash protocol; chains slow from `call_ref` | `map` near 2x on wasmtime; chains near 1x; the heap pathology removed |
| `allocations` (0 per counted loop, at most 1 per chain) | 3 to 5 per map lookup | 0 for both |
| `size-startup-heap` (tiny at most 2 KB) | short literals at 3 bytes per byte | the pool; hello world needs none |
| `dead-code` | list code per reference type | folded by A1; the target is re-based after spike S7 ([lowering-catalog.md](lowering-catalog.md#2-rebasing-the-dead-code-target)) |
| `long-run-memory` | slices that pin sources; heap growth pathology | the heap is sized; a small string slice still pins its source (Go-style strings, owner, 2026-10-07): a known risk this metric watches |

## 13. Monomorphization And Merging

Owner's answer 8: code per concrete type, then merge byte-identical
functions, in both pipelines. Dictionaries exist only for
trait values and for the type witnesses of a generic method called
through `dyn` (§13.5).

### 13.1 Roots

| Program | Roots |
| --- | --- |
| executable or task (`hd run`, `hd build`) | the entry wrapper of `main` or `main!` (§16.2), and the init function of every group the entry module reaches |
| `hd build FILE` | the same, for FILE's module |
| unit test program (one per package) | one `TestCase` body per registration and per doc test, in every module of the package, and one init export per module with tests, which runs that module's reachable init groups |
| integration test program (one file) | its `TestCase` bodies, and its reachable init groups |
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
   arguments. **In a pipeline that runs `inline-trivial`, a callee that
   is always inlined is not pushed** (lowering pass; the pipeline hash is
   in the code key): when its `inline_summary` says
   it passes the trivial-inlining
   test (§13.8), which reads only its own TIR, every caller inlines it, so
   collection scans its body in place under the composed substitution and
   pushes its callees instead. It is still pushed when it is used as a
   value (a function reference, an adapter) or fills a vtable slot. This
   removes 15 to 25 percent of emitted instances, mostly derive members,
   `Option` helpers and getters.
4. For each `TraitMethod` callee, read its choice. An `Impl` choice
   names the impl; substitute its arguments. A `Bound` choice becomes a
   concrete trait reference under the substitution, and the solver's
   `select` finds the impl (table below). Push the impl method with the
   impl's type arguments, or the trait's default body with `Self` set
   when the impl does not write the method. A `TraitValue` choice
   pushes nothing; a `Builtin` choice pushes its generated body (§13.6).
5. For each coercion to a trait value, select the impl as in step 4,
   record the vtable `(type, trait)` and push every method of the trait
   at that type, supertraits' vtables included. A method with its own type parameters is pushed as its
   erased instance, and paired with every type-argument tuple that a
   `CallDyn` of that method reaches, to push its thunks (§13.5.1).
6. For each `DefaultCall`, push the default body with the call's type
   arguments.
7. For each `Closure`, push the closure body with the parent's
   arguments.
8. Record every `CallHost` as an import, every type whose layout an
   operation needs, and every fact getter the instance calls (§12.3).
9. Repeat until the worklist is empty.

**A1 at collection (lowering pass; default adopted, decided by E1 and
S1).** Each generic item carries a **representation summary** per type
parameter, computed at check time and stored with its TIR, so the item's
TIR hash covers it. The summary says whether instances need the
parameter's exact representation. A parameter with a bound is always
exact. An unbounded parameter is exact when the body has a closure type
that mentions `T`. (Walking skeleton, SK-2: the earlier rule read only
the body's own trait calls on `T`. It made `relay[T < Shape]`, which
passes `T` on to `total[T < Shape]`, an instance at `REF`, and `select`
then found no impl at `REF`.) When the parameter is not exact, the body
only moves `T`, and collection replaces that type
argument in the pushed instance by its **class**: `REF` for a layout of
one non-null reference, `REF?` for one nullable reference, or the scalar
or `void` class. `REF` and `REF?` stay apart because `T?` differs between
them: a nullable reference for `REF`, a tag and a reference for `REF?`.
So `List[Point].push` and `List[User].push` are one instance, emitted,
cached and compiled once. The callees of a class instance see the class
as their argument too. The rule reads only the item's signature, its own
body and the argument's layout, so it is type-only. With bounds decided
by the signature, only closure types still read the body. At 10k lines the compile study
models 12 percent fewer instances, 43 percent fewer Wasm types and 9
percent less code. Erasing reference parameters of function types as
well (A2) is not adopted: it puts a cast in every closure entry.

**`select` (Codex re-review N7).** At an instance every type is
concrete, so `select` is a head match plus reconstruction, never a
proof ([trait-solver.md §8.3](trait-solver.md#83-what-codegen-does-with-it),
rule TS-6):

| Step | What it does |
| --- | --- |
| 1. Head match | match the concrete trait reference against the heads in its owner modules. Every trait argument is known, so the candidate directory is not read. Overlap is head-only and the checker proved the bounds generically, so at most one head matches. No match is an internal error (§12.7), never a diagnostic |
| 2. `Bind` steps | run the impl's bound plan's `Bind` steps in plan order, through `normalize_concrete`. Each reads an associated-type binding at concrete types and fixes one impl parameter that the head does not fix |
| 3. Skip `Bound` steps | the checker proved them; `select` proves no subgoal, has no depth limit and charges no fuel |
| 4. Return | the impl and every impl argument. In `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`, matching `Feed[ConcreteStore]` fixes `I`, and the `Bind` step fixes `T` |

`select` and `normalize_concrete` keep their answers in one **selection
table** per run, keyed by the concrete trait reference, and shared
across programs. It is not the solver's proof memo: a selection assumes
a proof and does not make one, so a selection entry and a proof entry
never share a key.

The order of the walk never reaches output: the result is sorted by
instance key before anything is emitted (§6.5). Collection is serial per
program and takes milliseconds; many programs (a test plan) collect in
parallel.

### 13.3 Instance Keys

```text
canon(T)      = structural encoding of a type over stable paths, with no IDs
                (Prim | Adt(path, args) | Tuple(elems) | Optional(T) | Fn(params, result, row keys, suspends)
                 | ...)

instance_key  = H("inst", item stable path, sub-body index (closures),
                  [canon(arg) or its class for each type argument])
```

A type argument that the item's representation summary marks move-only
is keyed by its class (§13.2), so its instances share one key.

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
- **Trait values** are pairs: the value as `eqref` and its vtable
  (§15.2). A call through a trait value is one `struct.get` and one
  `call_ref`.
- **GADT evidence.** Removed with GADTs (owner, 2026-10-07).
- **Generic methods called through a `dyn` value** use the type-witness
  ABI below.

#### 13.5.1 The Erased Method ABI (Codex re-review N5)

The owner's decision: a generic method called through a `dyn` value has
one erased body per impl, accepts any type argument, and gets a type
witness per call (goals.md, 2026-10-07). Everything else is
monomorphized. Examples are a user trait's `fn visit[T](self, f:
fn(T) -> T, out: mut List[T])`, and `Inspectable.downcast[T]`, whose
builtin body has one open instruction, the type-id read. The
inherent `dyn Error` methods `find`, `root_cause` and `chain` are
ordinary generic functions over `T` with a `dyn` receiver, so they are
monomorphized per `T` and need no witness.

**Open types and open values.** In the erased body of `m` at impl `I`,
a type is **open** when it mentions a method type parameter of `m`,
including a projection such as `T::Item`. The impl's own parameters
are concrete, since the erased body is instantiated at the impl's
concrete self type. A value of an open type is always **the caller's
concrete representation, viewed as `eqref`**:

| Concrete layout of the value | As an open value |
| --- | --- |
| one reference, nullable or not (data, `List`, `Map`, closure, `T?` of a reference, a value layout over the bound, which is already a box) | the same reference, upcast; no allocation. Under A1 a reference `T` reaches a class instance such as `List[REF].push` directly, with no thunk |
| a scalar | §15.2's erased form: `i31ref` for 16-bit-or-smaller scalars and for any integer whose value fits in 31 signed bits, else `$Box_i32` or `$Box_i64`; floats in `$Box_f32` or `$Box_f64` |
| a `multi` layout (a string view, a scalar's `T?`, `Result`, a value enum, a tuple, a trait value) | one immutable struct of its Wasm values |
| `void` | `ref.null eq` |

Every boxed layout is identity-free (S1c), so boxing is not observable.
A mutable container is a reference, so it is never boxed or copied:
the erased body holds the caller's own `$List_i32`.

**What the erased body emits.** Control stays in the erased body:
blocks, loops, `if`, returns, locals and `defer` work on open values as
plain `eqref` locals. Every other instruction whose operand or result
type is open is an **open instruction**, and it is **outlined**:

1. Emission numbers the body's open instructions in TIR order: `0, 1,
   2, ...`. The numbering reads only this body's TIR, so it is the same
   in every program.
2. Open instruction `k` becomes `struct.get $Ops_I.m k`, then
   `call_ref`. Its open operands and results cross as open values; the
   others keep their layouts.
3. A `Match` on an open enum outlines one tag read, and one payload read
   per bound payload; the switch itself stays in the erased body.
4. Its **thunk** is the same TIR instruction emitted at the caller's
   concrete types, wrapped in `ref.cast` or unboxing on the way in and
   upcast or boxing on the way out. The thunk is an ordinary instance
   with instance key `H("thunk", I.m stable path, k, [canon(arg)...])`;
   its code key is §13.8's, with `tir_hash(I.m)` as the item hash.

Outlining is complete by construction: any instruction that emission
can emit at concrete types can be a thunk, so the erased body has no
list of supported operations. In particular:

| Operation in the erased body | Its thunk at `T = i32` |
| --- | --- |
| `Buffer[T] { items: ..., len: 0 }` for a user `data Buffer[T]` | `struct.new $Buffer_i32` of the unboxed fields; returns the reference |
| `out.push(x)` on a caller-owned `mut List[T]` | `ref.cast $List_i32`, unbox `x`, `call List[i32].push`; in place |
| `x.show()` with `T < Display` | the impl that `select` picks at `i32` (§13.2), called directly |
| `T::type_id()`, `downcast_val::[T](a)` | the concrete type id, the concrete cast |
| a closure `fn(T) -> T` built in the body | a concrete `$Fn_i32_i32` closure whose code unboxes, calls the erased closure body, and boxes |
| `f(x)` where `f: fn(T) -> T` came from the caller | `ref.cast` to the caller's closure type, unbox, `call_ref`, box |
| a call of a generic item, `helper[T](x)` or `helper[List[T]](xs)` | a direct call of the monomorphized `helper[i32]` or `helper[List[i32]]` |
| a `dyn` call of another generic method at `T` or `C[T]` | the same `CallDyn` at `i32` or `C[i32]`, with that witness constant |

So **erasure is one level deep.** Only `I.m`'s own body is erased, with
its closures, which are sub-bodies of it and capture `$Ops_I.m` like a
local. Every callee is a monomorphized instance. There are no erased
callee instances and no witness chains.

**The witness.** A witness is a constant global per `(method m, concrete
method type arguments)`:

```text
$Ops_I.m = (struct (field (ref $Thunk_0)) (field (ref $Thunk_1)) ...)   ;; one per open instruction of I.m
$W_m     = (struct (field (ref null $Ops_I1.m)) (field (ref null $Ops_I2.m)) ...)
                                                ;; one field per impl of m in the program
```

- **What it carries:** for each impl's erased body, the thunks of its
  open instructions at the caller's types. That covers boxing and
  unboxing (inside the thunks), construction of composites such as
  `Buffer[T]`, container reads and writes in place, bound evidence,
  type ids, and function adapters. It carries no size or layout class,
  because no erased code ever lays out an open type.
- **How it is built:** at link time, not at the caller. Collection
  records each `(m, type arguments)` that a `CallDyn` reaches, and each
  impl of `m`'s trait whose vtable is in the program (§13.2 step 5).
  For each pair it pushes the thunks of that impl's erased body at those
  type arguments. Link writes `$W_m(args)` as a constant expression of
  `struct.new` and `ref.func`. A thunk that is itself a `CallDyn` adds
  its `(m2, args2)` pair, so collection runs to a fixed point. A pair
  whose type grows is polymorphic recursion, and the instantiation depth
  limit reports it (§13.4).
- **How it is passed:** the slot's signature is `(receiver, the
  arguments with open ones as eqref, (ref $W_m), providers...)`. The
  caller passes `global.get $W_m(args)`, upcasts or boxes its open
  arguments, and unboxes or casts an open result, which cannot fail. The
  erased body's prologue reads its own field, `struct.get $W_m f_I`,
  and narrows it with `ref.as_non_null`. Link fills every field of
  every witness, since any impl's value can reach any call site.
- **Lifetime.** Witnesses are immutable globals. A closure or a stored
  value that captured one can outlive the call, and nothing is freed.
- **Relocations.** The field index `f_I` is assigned at link, so the
  erased body records it as a `Field { at, sym: (m, I) }` relocation
  (§13.8). Witness types and globals are `Type` and `Global` symbols
  named by `(m, canon(args))`. So cached erased bodies and thunks do not
  depend on which other impls the program has.
- **No bound evidence.** Codegen reads neither `CallDyn`'s evidence
  operands nor the vtable shape's `dyn_bounds`. A bound call in the
  erased body is a thunk, chosen by `select` at the concrete type. So
  the slot signature has no vtable parameter per bound, and collection
  builds no vtable for a bound. Dropping the operand and the field is a
  frontend cleanup.

**Cost.**

| What | Cost |
| --- | --- |
| a `dyn` generic call | one `global.get` for the witness, plus boxing of open value-layout arguments and unboxing of an open result |
| erased body entry | one `struct.get` and one null check |
| an open instruction | one `struct.get` and one `call_ref`, plus boxing of open value-layout results. Reference-layout values cross free; a wide scalar or a `multi` value allocates a 16 to 32 byte box per crossing |
| code size | one erased body per `(impl, method)`; one thunk per `(impl, method, type arguments, open instruction)`; one witness per `(method, type arguments)`. Thunks are a few instructions and fold (§13.7) when layouts match |
| compile time | collection's pair fixed point, linear in the pairs; emitting a thunk is emitting one instruction |

The thunk count is the product of a method's impls, its distinct type
arguments and its open instructions. A trait with 20 impls, 5 type
arguments and 10 open instructions per body gives 1,000 thunks of about
20 bytes each, before folding. Only user generic trait methods called
through `dyn` pay it in full. `downcast` pays one thunk per type
argument, and `find` pays nothing.

**Later, if measured.** Outlining a whole basic block of consecutive
open instructions as one thunk cuts crossings and boxes. Full
monomorphization per `(impl, type arguments)`, with a type-id switch in
the slot, would remove the erased path altogether. That is not the
owner's decision, and it costs a whole body per pair.

**Rejected alternatives.** A runtime "representation factory" that
builds `Buffer[T]`'s layout from `T`'s witness, as Swift's metadata
accessors do, cannot work on Wasm GC: a struct type cannot be created
at run time. A uniform erased layout for every generic type that may
reach erased code (`Buffer[T]` with `anyref` fields everywhere) would
slow every monomorphized use of that type. Copying a caller's container
into such a layout would break the identity of data values.

**First test (build-order.md §22, slice 6b).** One `dyn` generic method
takes a packed `T = i32` and a reference `T`, mutates a caller-owned
`mut List[(T, T?)]`, constructs and returns a user `Buffer[T]`, calls a
generic helper, and returns an escaping `fn(T) -> T`. It validates and
runs on wasmtime and V8. The caller sees its own list mutated in place
and the returned buffer's identity preserved.

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

Under exact element types, `List[Point].push` and `List[User].push`
would fold only when `Point` and `User` have the same field layout (§23.2,
inconsistency 1). With A1 (lowering pass, default adopted), they are one
instance from collection on (§13.2), so folding is left with scalar
twins, equal field layouts and identical helpers.

**The algorithm (mine, after safe ICF in linkers; Codex re-review N4
and N-S3).** The first release folds exact duplicates only:

1. **Fold key.** `H(canonical signature, local declarations, body bytes,
   every relocation as (offset, kind, exact target), every site record
   as (offset, kind, anchor))`, where an anchor names an item and a TIR
   instruction in it (§13.8). The signature comes first: an
   `i32 -> i32` identity and an `i64 -> i64` identity have equal
   instructions and must not fold. Function targets are instance keys or
   getter symbols (a literal, a fact), global targets are global symbols
   (a vtable, a constant, module storage), so two bodies that reference
   different literals or globals never fold.
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
   **A worklist (lowering pass):** after the first round, only bodies
   whose function targets changed class are rehashed, so a deep call
   chain costs linear work, not depth times bytes.
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
threads or order. Merging runs in `Link`, in both pipelines (§12.6): it
saves more Cranelift time than its hashing costs (under 1 ms at 10k
lines), so a dev build without it would compile slower. Whether the
optimized pipeline adds `wasm-opt` on top is decided after spike T2.

### 13.8 Code Entries

`Emit(inst)` lowers one instance to Wasm with `wasm-encoder` and stores a
`code` entry:

```rust
pub struct CodeEntry {
    pub body: Box<[u8]>,                  // locals and instructions; index immediates as 5-byte padded LEBs
    pub relocs: Box<[Reloc]>,             // sorted by offset
    pub sites: Box<[SiteRecord]>,         // (offset, site kind, anchor) for traps and explicit panics
    pub lines: Box<[(u32, Anchor)]>,      // offset -> statement anchor, for backtraces
    // Anchor = (item path row, TIR instruction index in that item's body): no byte offset, no line
    pub sig: CanonSig,                    // Wasm signature, as canonical types
}
pub enum Reloc {
    Func { at: u32, target: FuncTarget },     // InstanceKey | Import(HostMethodId) | Intrinsic helper
                                              // | Getter(GlobalSym): a module's literal getter or span function, or a fact's getter
    Type { at: u32, ty: CanonWasmTy },        // a canonical Wasm type descriptor
    Global { at: u32, global: GlobalSym },    // module storage and immutable constants (vtables, closures, short literals)
    Field { at: u32, sym: WitnessField },     // an impl's field in a dyn method's witness (§13.5.1)
}
```

**Stable numbering (lowering pass).** An emitted body holds no
program-wide dense number that changes when something is added
elsewhere, apart from function indices in `call`, which wasmtime's
per-function cache abstracts, and the measured exceptions of
[lowering-catalog.md](lowering-catalog.md#numbering) (type immediates,
direct constant globals, `ref.func`, witness fields). So there is no
`Site` relocation: a site is the code offset of its stub call
(wasm-layout.md §15.5). There is no `Data` relocation in a body: only the
link-generated module literal getters, span functions and the fill helper
name the data segment. A lazily initialized global is reached through a
`Getter` call target. A literal use also holds its module-local number,
which depends only on its module's literal list (lowering-catalog.md,
[Literals](lowering-catalog.md#literals)). So the relocation kinds are
`Func`, `Type`, `Global` and `Field`; there is no `Site` and no `Data`.
Context key ids and type ids are content hashes written as constants.
Spike 0c's S2 measures each remaining index space and moves any that
keeps fewer than 95 percent of functions hitting behind a getter.

On disk a relocation is `(at, kind, target)` with `target` indexing the
entry's deduplicated target table
([data-structures.md §3.22](data-structures.md#322-codegen-the-instance-table-and-code-entries)).

**Positions (systems review, finding 4).** A code entry holds no source
position. A site or a statement line is an **anchor**: the item's stable
path and the index of a TIR instruction in its body. Instruction indices
do not move when a comment or a blank line is added, so the TIR hash
leaves the span columns out, and so do code keys, the fold key and
`prog_key`. A panic's reported location is still program output, so an
anchor is resolved against the current sources whenever a location is
printed:

- `hd run`, `hd test` and the playground keep the anchors in the linked
  module's `hd.sites` and `hd.lines` (§13.10). On a panic or a failing
  test, the host looks the anchor up in the module's current `check`
  entry: the instruction's span columns give byte offsets, and its `locs`
  section gives the line. That costs microseconds, only when a location
  is printed, as for a diagnostic's token anchor (cache.md §5.3).
- `hd build` writes a file that must stand alone. When it copies the
  `link` entry to `build/`, it resolves every anchor to a file, line and
  column in the written sections: one pass over about 5,000 rows at 10k
  lines, plus one map of each reachable module's `check` entry, a few ms
  per build. The cached `link` and `cwasm` entries stay anchored.

So a comment edit above a panic changes the line it reports and nothing
that is compiled or cached.

```text
code_key = H("code", toolchain_key, pipeline_hash, profile, instance_key, tir_hash(item),
             sorted [(stable path, per-item interface hash)] of every item its TIR names,
             sorted [layout_hash(T)] of every type the instance lays out,
             sorted [(impl stable path, impl interface hash)] of every impl collection
                    selected for it, for calls and for associated-type projections,
             sorted [(callee instance_key after A1 classification, inline_summary(callee))]
                    of every direct callee,
             sorted [(stable path, tir_hash)] of every item inlined into it,
             sorted [(literal hash, module-local number)] of every pooled literal it reads)

layout_hash(T)       = H(canonical Wasm layout descriptor of T, and of every type
                         reachable through its fields), memoized per type per run
inline_summary(f)    = H("no-inline") when the trivial-inlining test (§12.6) fails on f's TIR,
                       else H("inline", tir_hash(f)); stored per item in the `tir` entry
pipeline_hash        = H(pipeline name, each pass's name, version and parameters,
                         emit options, engine options)   (tiering.md §6.4)
```

`pipeline_hash` replaces the earlier `tier` key part, and the profile is
a separate part, since `hd test --release` pairs the test profile with
the optimized pipeline. A pass version bump misses only the pipeline that
holds the pass.

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
- **Callee representation summaries (walking skeleton, SK-3).** The
  callee's instance key in the list is the one after A1 classification
  (§13.2): `first[REF]` or `first[Point]`. It picks the symbol the caller
  relocates to and whether the caller casts the result. So a change to a
  callee's summary, such as a new bound, re-emits its callers even when
  their own TIR is unchanged.
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

**The compile study's model** (representation-compile.md §1.4) puts a
10k-line application at about 4,800 instances, 4,700 emitted functions
and 640 KB under exact types, and about 3,600 functions and 530 KB with
A1, the boxing bound and closure specialization. Derived code is the
largest source (27 percent of instances), then iterator chains (14
percent), then `List` and `Map` methods (17 percent together). Skipping
always-inlined callees (§13.2) and the compact `hd.names` section
(about 120 KB at 10k lines) are the largest levers for bytes; stable
numbering, packs and filtered test programs are the largest for latency.
`hd build --size-report` (§12.8) shows where a program's bytes go.

### 13.10 The Link Step

`Link(P)`:

0. **Read the code entries in one batch** (lowering pass; systems
   review, finding 1). The worktree's last-run record, or else the
   program's `packhint`, names the **code packs** of the program's last
   link: one per folder group, each holding the code entries of that
   group with an index by code key (cache.md §5.4). Link maps those
   packs once and takes every unchanged code key from them; `Emit` runs
   only for keys no pack holds. It then writes a new pack for each group
   whose members changed. Reading 4,800 separate files would cost 0.3 to
   0.6 s on the Mac; about 30 mapped packs cost 2 to 4 ms.
1. **Drop and fold.** An instance that no relocation names, because every
   caller inlined it, is dropped. Then the rest fold (§13.7).
2. **Order functions.** Imports first, sorted by module and name. Then
   the generated runtime helpers, sorted by name. Then the module
   literal getters and span functions, by module path, and the fact
   getters, by content. Then the
   representatives in instance-key order. This order is deterministic,
   but it does not keep indices stable: inserting a function with a
   smaller key renumbers every later one (Codex re-review N-D2). That is
   harmless for `call`, which wasmtime's per-function cache abstracts. No
   persistent index allocation is kept, since it would make bytes depend
   on build history (wasm-layout.md §15.8). S2 of spike 0c measures the
   `ref.func` case; if it misses, closure construction reads one
   immutable `funcref` global per closure code instead.
3. **Types.** The canonical types the functions, globals and exports name
   (§15.3).
4. **Globals.** First the fixed globals: the runtime's globals. Then
   one literal table per module that has pooled literals, by module path.
   Then the immutable constants (vtables, closures without
   captures, payloadless variant singletons, member handles, witnesses,
   `TypeId` values, literals of at most 4 bytes), by kind and content
   key. Then module storage, by module path and binding index. Then fact
   storage, which only getters name (§15.4).
5. **Data.** One passive segment holding every pooled literal once,
   deduplicated by content, in content order, then each module's index
   area of `(offset, length)` pairs. Only the literal getters, span
   functions and the fill helper name it. Key ids and type ids are checked for 64-bit
   collisions here (§12.4).
6. **Elements.** One declarative segment for every function used with
   `ref.func`. The reserved hot-reload table stays empty (§18.5).
7. **Exports**: `hd.init`, `hd.poll`, `hd.wake`, the exchange buffer, and test entries (§14.4, §15.4, §19.2).
   A package's unit-test program exports one init function per module
   with tests instead of one `hd.init`; each runs exactly the init groups
   its module reaches, in D1's order (engines-and-test-runner.md §19.1).
8. **Code.** Copy each representative's body and patch its relocations in
   place. In the optimized pipeline, re-encode the padded LEBs at minimal
   width (mine, the `leb_compact` pass); the dev pipeline keeps them, since
   size does not matter there.
   Re-encoding moves every later byte, so the encoder builds an
   old-to-new offset map per body as it goes (a sorted list of
   `(old offset, bytes removed so far)`, one row per shrunk immediate)
   and rewrites the offsets of `sites`, `lines` and the source map
   through it before writing `hd.sites` and `hd.lines` (Codex re-review
   N-B8). A test panics after a shrunk immediate in the optimized
   pipeline and checks the reported site against the dev pipeline.
9. **Custom sections**: the compact `hd.names` in the optimized pipeline,
   so release builds omit the standard `name` section (owner,
   2026-10-07), or `name` in the dev pipeline; then
   `hd.runtime`, `hd.sites`, `hd.lines`, `hd.folds` (§15.5, §16.4).
   `hd.sites` and `hd.lines` hold anchors here; `hd build` resolves them
   to positions in the file it writes (§13.8, "Positions").

```text
link_key = prog_key (§11.3), and on a prog_key miss
           H("link", toolchain_key, pipeline_hash, profile, root description, sorted code keys)
```

A program is written under both keys, so the next warm run hits the
cheap one. Link is serial per program and linear in output size.
