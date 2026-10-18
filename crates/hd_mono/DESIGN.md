# New Compiler Design: Back Half, Codegen

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## Part D2: The Back Half

The back half monomorphizes, emits Wasm GC and links. Suspension
lowering and the Wasm layouts are in crates/hd_wasm/DESIGN.md, the
optimization passes in crates/hd_opt/DESIGN.md. It consumes these
interfaces and does not reach around them:

| Interface | From | What the back half gets |
| --- | --- | --- |
| TIR per body (crates/hd_tir/DESIGN.md §4.13.11) | `hd_check` through `hd_tir` | the one typed IR: desugared, generic bodies with explicit coercions, dispatch, decision trees, cleanup scopes, suspension and hook points; read from the `check` entry |
| `ModuleResult` | `hd_check` | init summary, fact records, diagnostics; whether the module has errors |
| folder interfaces (crates/hd_resolve/DESIGN.md §4.10) | `hd_resolve` | signatures, impl tables, templates and hidden items, default and fact expressions, per-item hashes |
| types and rows | `hd_types` | the intern pool, stable paths for every `DefId`, and `StableHash` |
| init order | `InitOrder` | statement order per module cycle |
| test plan | `hd_driver` | test programs and their statically registered cases |
| cache | `hd_cache` | `CacheStore`, the key rules of crates/hd_cache/DESIGN.md §5.3, entry framing, eviction; the back half's kinds are `code` and `link` |
| scheduler | `hd_sched` | `TaskKind::Ext` tasks with dependencies on frontend tasks |
| diagnostics | `hd_diag` | the `Diagnostic` record for the back half's own errors, such as instantiation depth |
| determinism | | content order, local counters, no IDs in output or keys |

## 11. Back-Half Overview

### 11.1 From TIR To A Running Program

```text
 ModuleFinish(m) ═══════════════════════════════════► [check entry, TIR sections]  per module
                                  │ generic TIR, per-item TIR hashes, dependency lists
 program root(s) ─────────────────┤  (entry, test registrations, REPL input)
                                  ▼
                       Collect(P): reachability over TIR + impl tables
                                  │ instance set {(item, type args)}, content-ordered
                                  ▼
                       Emit(inst): emit every missed instance,
                                    choose layouts, write Wasm ═════════► [code]     one per instance, parallel
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

`═══►` marks a cache boundary. Every boundary goes through `CacheStore`,
and every key follows crates/hd_cache/DESIGN.md §5.3's hygiene: no paths,
no IDs, no thread counts. Fold at link is not built (§13.7), and
`Precompile` and `RunCase` are scheduled but empty: `hd_run_wasmtime`
compiles and runs outside the task graph.

### 11.2 Stages

| Stage | Input | Output | Unit | Parallel | Cached | Key |
| --- | --- | --- | --- | --- | --- | --- |
| Collect | program roots, TIR of reachable modules, impl tables | the instance set, the import set, the type set; code-entry hits and folder groups with misses | program | serial per program; programs in parallel | inside `link` | none |
| Emit | one folder group's missed instances: generic TIR, substitutions, and callees' TIR | Wasm body bytes, relocations, site and line records | folder group (`codepack`) | groups in parallel | `code` entries in `codepack` | §13.8 |

A folder group is the folder of an instance's item. Its code is one
`codepack` entry (crates/hd_cache/DESIGN.md §5.4, "Code packs").
| Link | code entries of the instance set | one Wasm module with its custom sections | program | serial per program; fold hashing in parallel | `link` | §13.10 |
| Precompile | Wasm bytes, engine config | wasmtime's serialized module | program and pipeline | Cranelift compiles functions in parallel | `cwasm` | `H("cwasm", hash of the Wasm, wasmtime version, config hash, target)` |
| Instantiate and run | precompiled module, host | outcome, output | run or test case | test cases in parallel | no | none |

There is no MIR stage: the checker's TIR is the input of `Collect` and
`Emit`.

### 11.3 Tasks

The back half's tasks are `TaskKind::Ext` tasks (crates/hd_sched/DESIGN.md §6.1):

```rust
pub enum ExtTask {   // hd_sched
    Collect,         // after the check entry of every module the program can reach
    Emit(u32),       // created by Collect, one per folder group with a miss
    Link,            // after every Emit of its program
    Precompile,      // scheduled, no work yet
    RunCase(u32),    // scheduled, no work yet
}
```

- `Collect` starts only after the `tir` entries of every module that the
  program's roots reach in the module use graph exist, and after the
  `HeaderCheck` task of every folder they reach (crates/hd_sched/DESIGN.md §6.1). That set comes
  from the manifest's use lists, so it is known before any check runs.
- `Collect` looks up every code entry itself. It reuses hits and creates
  one `ExtTask::Emit(GroupId)` for each folder group with at least one
  miss. The task emits that group's missed instances as a batch, and the
  group is the `codepack` unit of crates/hd_cache/DESIGN.md §5.2.
- **The program fast key.** Before `Collect`, the driver computes

  ```text
  prog_key            = H("prog", toolchain_key, pipeline_hash, profile, root description,
                          sorted [(module path, tir_content_hash(m))] of the modules
                          reachable in the use graph)
  tir_content_hash(m) = H(sorted [(item path, TIR hash, inline summary, dependency list)]
                          of m's items, m's literal list (§13.8))
  ```

  Built: each module's content hash is its bodies' TIR hashes, its init
  summary, its encoded items, and its test registrations without their
  positions. Linking reads all four. A trait's methods are a profile
  handle's imports, a data type's fields its boundary codec, and a
  registration's `ignore` drops a case, and an edit to any of them can
  leave every TIR hash as it was (#316). The runtime profile's trait
  list is not in it: only checks read the list, under their own keys.

  A hit names the `link` entry directly. Collection, emission and
  linking are skipped, and a warm `hd run` or `hd test` reads one entry
  per program. Module-level reachability is coarser than item-level, so a
  hit is always sound.
  - **Content, not check keys.** A key over each module's check key
    would hold its source hash, so a comment, a blank line or a `tests:`
    edit would miss every program that reaches the module. With content
    hashes such an edit rechecks one module, which writes an equal
    content hash, and every program hits.
  - **Why positions may stay out.** A TIR hash excludes the span columns
    (crates/hd_tir/DESIGN.md, "Lifetime And The `tir` Entry"), and code holds
    no source position: panic sites and statement lines are anchors to a
    TIR instruction, resolved against the current sources only when
    printed (§13.8, "Positions"). So moving code down a file changes no
    program byte that a run depends on.


### 11.4 Crates

TIR lives in `hd_tir`; there is no MIR crate.

| Crate | Holds |
| --- | --- |
| `hd_mono` | collection, substitution, instance keys (`layout`), the depth limit, the erased method ABI (`erased`), the plan types (`plan`) |
| `hd_opt` | the pass manager and the body passes (crates/hd_opt/DESIGN.md) |
| `hd_host_abi` | the host ABI description: traits, methods, codecs, wait flags, intrinsic keys |
| `hd_wasm` | Wasm layouts, emission with `wasm-encoder`, the code entry format, runtime helpers, the linker |
| `hd_run` | engine-independent runtime pieces: panic frames, import checks |
| `hd_run_wasmtime` | the wasmtime engine, providers of the default profile, limits, the `cwasm` cache |
| `hd_web` | the compiler in the playground's worker |

The JS host's codecs and method descriptors (`host/gen/abi.mjs`) are
generated from the ABI model by `hd_host_abi`'s `tests/abi_model.rs`;
its providers and driver (`host/core.mjs`) are written by hand.

## 12. Emitting From TIR

### 12.1 Analysis, Then One Emission Walk

`Emit(inst)` reads the instance's TIR from its module's `check` entry. It
may first run **bounded analysis passes** over the generic TIR under the
substitution, then it writes Wasm in one walk. Some choices need facts about later instructions: whether a value
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
  [docs/compiler/tiering.md §6](../../docs/compiler/tiering.md#6-a-pass-manager-for-hd)), each gated by the
  instance's summary bits: an allocation, a closure, a loop with a
  candidate, or a suspension point. A trivial instance skips them.
  Implementation differs: no instance pass runs in any pipeline,
  suspension liveness included (crates/hd_opt/DESIGN.md §1.3); see
  crates/hd_wasm/DESIGN.md §14.2.
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
| `Prim` | one `prim_value` lowering, shared with the intrinsic std operator and comparison methods (`add`..`rem`, `neg`, bitwise, `not`, `shl`, `shr`, `eq` map to their `PrimOp`): overflow checks, division rules and panics have one source. `cmp` and `partial_cmp` build the `Ordering` tag as 1 + (a > b) − (a < b); float `partial_cmp` answers `.None` on NaN. A shift count keeps its own width and is extended or wrapped to the value's width |
| `CallDyn` | `struct.get` of the vtable slot, then `call_ref`; a generic method also gets its witness global (§13.5.1). When the vtable is a known constant global after inlining, a direct `call` of the slot's function, which the inliner may then inline (§12.6) |
| `CallValue` | `struct.get` of the closure's code, then `call_ref` with the closure as the first argument |
| `MapIter` | §13.12's iterator: a `Closure` struct `{code, map, pos}`, built like `ListIter`'s `{code, list, index}` |
| `MapRemove` | §13.12's removal: probe to the entry, write the tombstone, return the old value as an `Option` |
| `ItemRef` | a function reference value, built as §13.11's adapter: a `Closure` struct whose code is the adapter instance |
| `CallHost` | an import call with the exchange-buffer codecs (§17.2) |
| `Closure` | a struct of the closure's environment: `Copy` and `Move` captures as fields, `Shared` ones as their cells; a closure with no capture is a constant global |
| `CopyData` | evaluate the source once, then the replacements in source order (spec: the spread is first); allocate a fresh value of the source's exact type; copy every non-replaced field with a field read through the source's access view (readonly source reads a direct `mut U` field as `U`); store the replacements; an unreplaced embedded part copies as its own nested `CopyData`. A field the checker read at the spread (`expr.update.copy-time`) arrives as a replacement |
| `NewList` with spreads | a builder seeded with the fixed-element count: evaluate each element and spread operand once, in order, appending values and `array.copy`ing spread lists; one exact-size array at the end, then the `{len, data}` struct |
| `NewTuple` with a rest operand | the fixed elements as usual, plus the rest-list operand as the tuple's rest field |
| `Coerce` | option wrap: a tag set or nothing (§15.2); to a trait value: a pair with a constant vtable; `Supertrait`: the payload unchanged and `struct.get` of the parent's vtable field from the child's vtable (§13.5); readonly view, variance and row subsumption: nothing (§12.4) |
| `Interp` | one builder sized by the literal parts' bytes; literal parts copied from their pooled literal; each `Display` part writes into the builder through its resolved callee; a string part copies its viewed bytes; one exact-size array at the end, viewed from start 0 |
| a string literal | a view over its pooled array (crates/hd_wasm/DESIGN.md §15.2): `i32.const` of its module-local number and a `call` of its module's literal getter, or `global.get` of an `array.new_fixed` constant at 4 bytes or less, then `i64.const` of its span; a literal passed straight to a host call takes the host fast path (crates/hd_wasm/DESIGN.md §15.4). Implementation differs: a `call` of the literal's own getter, and no fast path or short-literal globals (crates/hd_wasm/DESIGN.md §15.4) |
| an explicit panic, a failed check | a `call` of the category's stub with no site immediate; the call's code offset is the site (crates/hd_wasm/DESIGN.md §15.5) |
| `DefaultCall` | a direct `call` of the default body's instance with the earlier argument values, inside the forbidden-context bracket (§12.3) |
| `ForRange`, `ForList`, `ForMap` | counted loops (§12.4) |
| `Await*` | the state machine (§14) |
| `Hook` | nothing, except in hook builds (§14.7) |

**Drops** (13-ownership.md, Drops). A `Scope`'s record also lists
`DropLocal`s; each one whose drop mode is not `Never` and whose type has
glue (§13.2) is a rung of the same ladder, guarded by a drop flag: zeroed
where the scope starts, set where the local takes a value, cleared where
it moves. A `defer` suite copies the owned locals it reads into
snapshots where it registers, reads those, and drops what it did not move
right after it runs. An assignment drops the old value first when the
flag is set, and a store into a field with glue drops the field's old
value first.

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

- **Defaults run per call.** A default is a body of its own, and each
  call that omits the argument runs it through a `DefaultCall`, after
  the explicit arguments
  ([`fn.default.eval`](../../spec/lang/07-functions.md#r-fn.default.eval)),
  so no mutable default is shared between calls. Emission increments the forbidden-context counter around a
  `DefaultCall` whose body makes a call, so an indirect `block_on` or
  `println` there panics (crates/hd_wasm/DESIGN.md §14.9).
- **Facts are lazily initialized globals.** A fact
  and a metadata expression are ordinary bodies, run once on first read
  ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy),
  [`annot.metadata.eval-as-fact`](../../spec/lang/14-annotations.md#r-annot.metadata.eval-as-fact)).
  Each read (`facts_of`, `T::facts()`, or a member handle's `info.facts`)
  is a getter: a mutable global plus an "initialized"
  flag, filled by the body's instance on the first read. Readers `call`
  the getter and never name the storage global, so a new fact elsewhere
  changes no reader's bytes (§13.10); a body calls each getter once on a
  dominating path and reuses the result. The getter runs
  the body inside the forbidden-context counter (crates/hd_wasm/DESIGN.md §14.9), and
  a panic in it is reported as `fact-evaluation-failed` with the original
  category. A fact that nothing reads is never collected, so it is never
  emitted. There is no compile-time evaluator, no evaluation budget, no
  `fact` cache entry and no value-graph serialization; a changed fact
  body changes only its own instance's code key.
- **Shared enum data initializes with its module.**
  Each variant's constructor and defaults are requirement-free bodies in
  the declaring module's group init function
  ([`data.shared.module-init`](../../spec/lang/08-data-and-enums.md#r-data.shared.module-init)).
  They run once per program instance before entry behavior, never through
  a first-read getter.
- **Derives.** Derive instances, including the generated `walk`,
  `describe` and `build`, are ordinary bodies (§4.13.9). With the
  walker's type known at each instance, every `w.member(h, value)` call is
  direct, so a derived `encode` is straight-line code
  ([`annot.walk.members`](../../spec/lang/14-annotations.md#r-annot.walk.members)).
  Member handles are built once per owner instance and held in globals,
  like facts (see "The derivation's own `Structure`" below).
- **The derivation's own `Structure`.** Each derivation sees
  a `Structure` for its target, generated as ordinary TIR in the target
  module (`hd_check/src/structure.rs`): the bodies of `facts`, `name`,
  `walk`, `describe` and `build`, plus one hidden method per handle. A
  handle's body has kind `Handle`: emission gives it the facts getter's
  storage (globals plus a flag, without the panic label or the
  forbidden-context counter), so the handle is built on the first call
  per owner instance and every later call returns the same value
  ([`annot.handle.constants`](../../spec/lang/14-annotations.md#r-annot.handle.constants)).
  Building it reads its facts, so a member fact runs when its handle is
  first built ([`annot.fact.eval.lazy`](../../spec/lang/14-annotations.md#r-annot.fact.eval.lazy)).
  The owner is the data type or enum
  for the declaration's `Structure`, generated once per module and shared
  by every derivation without member lines; a derivation block with
  member lines owns its own (`annot.structure.per-derivation`). A derive
  instance calls the owner's methods directly, so one instance per
  (method, owner's arguments, walker) serves every derivation and call
  site that sees one `Structure`. Each `Facts` (type, variant, member) is
  a hidden body of the owner with no parameter that evaluates the slot's
  decorator values, then its trait-less block's line, then the
  derivation block's line, in source order; a slot the block does not
  edit reuses the declaration's body. `VariantInfo.shared` reads the
  variant's shared-data globals. The bodies are in the module's TIR, so
  a field edit changes the module's content hash, the program key and
  the generated instances' code keys (`hd_driver/tests/structure_run.rs`).
  Not carried yet, each a structured "not implemented" that stops a call
  at collection: a typed fact (`annot.typed-fact.check` needs the fact
  type's pattern), a member line whose list is not a list literal (a
  `List[dyn Any]` value has no conversion to `Facts`' `dyn Inspectable`
  items), and the shared data of a generic enum. Doc comments are not
  carried (`doc` is `.None`). As built, a facts body runs at each read,
  as `facts_of`'s does, not through the first-read getter above, so a
  panic in it keeps its own category.
- **Test registrations.** A `TestCase` body evaluates its registration
  call's run-time arguments (`rows`, `timeout`, examples) and calls the
  std registration function, which drives the test body.
- **Entry reports (`std.rt`).** The compiler supplies the
  virtual module `std.rt`, like `std.core`. Its ordinary hd functions turn
  `Termination` and `Result` values into statuses and rendered error chains;
  only its private `entry_write` operation is intrinsic. Collection roots
  the matching function after each entry point or test case.

### 12.4 Rows And Providers

Requirement rows never become type arguments
([`req.poly.one-body`](../../spec/lang/11-requirements-and-suspension.md#r-req.poly.one-body)).

- **One key order.** Every place that lists a row's
  keys positionally uses one comparator: the bytes of `canon(K)` (§13.3).
  That covers the parameters of a signature, the providers of a call,
  the key ids of a context, and the rows of the `check` entry. In-run sets
  sort keys by `Ty` value for fast merges, so positional data
  never relies on that order. TIR stores a call's providers as
  `(key, provider)` pairs (crates/hd_tir/DESIGN.md, callee records), and
  emission sorts the pairs by this comparator once per call site.
- **A concrete row** (`$ Console + Clock`) adds one parameter per key,
  in that key order. Each is a trait value of the
  key's trait (§15.2). A call passes the providers TIR names for it, so a
  provider costs one Wasm argument per key and no allocation.
- **A row parameter** (`$R`) adds one **context** parameter. A
  context is an immutable linked list of `(key id, provider)` nodes. A
  key id is the first 64 bits of `H(canon(K))`, an `i64`, so adding a key
  elsewhere changes no context-using body. Link checks the
  program's key ids for collisions and, on one, rehashes every id with
  the next fixed salt. Looking up a key
  walks the list and takes the first match, which is how an inner
  `$.with` shadows an outer one.
- **Extension** (`$.with(Logger=...)` around a call that needs `R +
  Logger`) pushes one node: one allocation per call into row-polymorphic
  code.
- **One callable ABI for function values.** Every
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
  `ProviderGet` would. The checker writes such pairs for calls to private
  callees whose rows were solved after checking (crates/hd_tir/DESIGN.md,
  callee records).
- **Cold suspensions** capture their providers in the frame at
  construction ([`req.bind.construction`](../../spec/lang/11-requirements-and-suspension.md#r-req.bind.construction)).
- **Context values.** A value of `$.Context[$ R]` holds one
  provider per key of `R`. Its row is concrete
  (`req.row.param.context`), so its key order is known statically and
  the value needs no key ids. It is one `(ref $CtxProvs)`, the
  providers array of the function-value context (`array eqref`): for
  key number `i` in key order, the provider's payload at `2i` and its
  vtable at `2i + 1`. `$.Context[$()]` is an empty array. Contexts
  have no subsumption, so a context's array always matches its type.
  `ContextNew` builds the array once: for each key of its type, the
  last pair that binds it supplies the provider, as a trait value
  checking already converted, or as two `array.get`s from a spread
  context (`req.context.order`). A `$.with` pair pushes provider locals
  as before, with no conversion of its own, since checking already
  made each value a trait value of its key. A spread pair reads each
  key of the spread context's row into a payload local and a vtable
  local (`ref.cast` to the key's vtable) and pushes them. The pairs
  are pushed left to right and lookup takes the innermost, so a later
  entry wins. Building costs one allocation of `2n` slots; installing
  costs `2n` reads and casts, and no allocation.
- **As built.** The context of a row parameter is the function-value
  context, two arrays (`ctx_keys: array i64`, `ctx_provs: array
  eqref`), not a linked list. Every instance of an item takes the
  placeholder `Canon(0xF1)`, the open row, as the argument of each of
  its own row parameters (`hd_mono::rowless`), so one instance serves
  every row (`req.poly.one-body`). The instance takes one provider per
  key its row lists itself (`declared_row_keys`), then the context
  last. A row parameter of an implementation stays a key of its
  instances.
- A caller passes, in the context, the keys of the row arguments that
  the callee's row has (`row_param_keys`). When an argument lists the
  caller's own open row, those keys go in front of the caller's
  context, and with no key and no open `$.with` scope the caller passes
  its context unchanged, so forwarding `R` allocates nothing. A `$.with`
  key goes in front too, so the nearer provider wins
  (`req.with.nearest.row-parameter`). Extending a context copies both
  arrays, one allocation each.
- A closure whose row lists the open row reads those keys from its own
  call's context, which a suspending closure keeps in its frame. A
  function reference's adapter passes its context on to the instance.

### 12.5 Counted Loops And Checks

`ForRange`, `ForList` and `ForMap` are emitted as counters in both pipelines.
A loop that allocates per step fails the `allocations` target, so
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
| integer overflow (`+`, `-`, `*`, `**`, unary `-`) | panics `integer-overflow` | wraps |
| `INT_MIN / -1` | panics `integer-overflow`; at 64 bits it is tested before `div_s`, which would trap | same |
| `INT_MIN % -1` | zero | same |
| division by zero | panics `integer-division-by-zero` | same |
| shift by the width or more | panics `invalid-shift` | masks the count to the width; Wasm masks at 32 and 64 bits, a sub-word width masks explicitly |
| list index | std's index method compares with the length, `index-out-of-bounds` | same; the backing array is longer than the list |
| string byte index | compares with the view's length, `index-out-of-bounds`; the array may be longer than the view, so the engine's check is not enough | same |
| use of a closed handle | panics with the use site | not emitted; the host provider still refuses a closed handle |

`i64` multiplication has no wide multiply in core Wasm, so its check
divides back, which costs about 20 cycles. The `check-cost` metric
(the optimized pipeline, checks on against off) watches it.

### 12.6 Tiers And Optimizations

There are two pipelines ([docs/compiler/tiering.md](../../docs/compiler/tiering.md)), and the pipeline is
separate from the profile:

- The **profile** is observable: checked (debug, test) or wrapping
  (release). It decides overflow and shift checks and the debug-only
  checks ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
- The driver takes the profile as a build input (`Packages::build_profile`,
  checked for every goal but a program), and the emitter reads it through
  `ProgramEnv::build_profile`. `code_key` and `prog_key` hold it, so a
  debug and a release build never share code (#15).
- The **pipeline** is not observable: dev or optimized. It decides which
  optimization passes run and the Cranelift setting. A program's output,
  panic categories and sites, effect order, `is` and type ids are the
  same in both (docs/compiler/tiering.md §4).
- `hd run`, `hd test` and the REPL use the **dev pipeline**: no hd
  body passes, then Cranelift `None`.
- `--release` selects the **optimized pipeline**: the hd passes of
  crates/hd_opt/DESIGN.md §3.1, then Cranelift `Speed`. On `hd build`, `hd run` and `hd FILE` it also
  selects the release profile; `hd test --release` keeps the test
  profile.
- Bounded inlining, scalar replacement, closure specialization and
  devirtualization run **in the optimized pipeline only**. The `runtime`
  and `allocations` targets measure optimized artifacts.
- What both pipelines share: the layouts (one per type), counted loops,
  `multi` layouts, the literal pool, stable numbering, folding during the
  walk and merging at link.

Each optimization in this table is a named pass with its inputs, outputs
and cost in [docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#optimization-passes);
the baseline emission without any pass is defined there too.

| Rule | Dev pipeline | Optimized pipeline | Built |
| --- | --- | --- | --- |
| counted loops (§12.5) | yes | yes | yes |
| `multi` layouts: `Option`, `Result`, tuples and trait values as several Wasm values; one representation per type in every position, boxed over the bound | yes | yes | yes |
| capture-free closures as constants | yes | yes | yes |
| constant folding and dead branches during the walk (a fact's value is a run-time read, §12.3) | yes | yes | planned, not built |
| constant hoisting: an enum or tuple box whose payloads are constants is an immutable global, so `.Err(ParseError.Empty)` never allocates | yes | yes | planned, not built |
| trivial inlining: the walk descends into a callee of at most 8 instructions with no loop, no suspension point and no closure | yes | yes | planned, not built |
| bounded inlining: callees up to a size budget, and closures passed to a known callee, such as iterator adapters (budgets in §12.8) | no | yes | planned, not built (`inline_plan` is a declared stub) |
| scalar replacement: a non-escaping closure, cell, box or data value after inlining becomes locals, mutable data included; an escape analysis in the analysis passes of §12.1, before emission | no | yes | planned, not built |
| known-vtable devirtualization: a `CallDyn` on a value whose vtable is a constant global becomes a direct call | no | yes | planned, not built (vtables are not globals) |
| known closure calls (A1, A2 of crates/hd_opt/DESIGN.md) | no | yes | yes |
| closure specialization: inline, scalar-replace, devirtualize, repeated (below) | no | yes | not built yet; see #65 |
| overflow and shift checks | by profile | by profile | yes |
| hook points (§14.7) | dropped | dropped | yes; hook builds are planned |
| debug-only checks: closed handles, deadlock reports with frame lists (§14.8) | by profile | by profile | planned, not built |

Inlining walks the callee's generic TIR, mapped from its module's entry,
under the composed substitution. The inlined items' TIR hashes join the
instance's code key (§13.8). Escape facts and inlining decisions are
emission state, never written into TIR.

**Closure specialization.** Not built yet; see #65. The design: an iterator adapter stores
its closure in a field of a mutable `Iterator` struct, so inlining alone
never reveals the `call_ref` target. Specialization is a schedule of the
named single passes of
[docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#optimization-passes), one round
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

The TIR verifier (crates/hd_tir/DESIGN.md §4.13.11) holds before emission
starts. The walk asserts in compiler debug builds and CI:

1. no `Param` type remains after substitution;
2. every `TraitMethod` callee resolves to exactly one impl at the
   instance's types, by `select`'s head match and `Bind` steps (§13.2);
3. every suspension point gets a resume case and a cancel case (§14.2);
4. every relocation names a symbol in the program's instance, type, import
   or global sets (§13.10).

A failure is an internal error naming the instance, with exit status 101.
The linked module is then validated by `wasmparser`
(crates/hd_wasm/DESIGN.md §15.7).

Implementation differs: the emitter trusts verified TIR and runs none of
these assertions; an unexpected shape stops with a structured
not-implemented (`unsupported`), and only a malformed callee record or
the layout of a non-concrete type stops as an internal error. Not judged.

### 12.8 Size Versus Speed Policy

Runtime performance has two axes, code speed and code size. Speed scales with allocations, indirect calls, casts and
pointers per live object. Size scales with distinct instances, inlined
copies and constant-building code, and it feeds download, instantiation,
engine compile time, and the `size-startup-heap` and `dead-code` targets.
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
   and trivial inlining
   ([docs/compiler/tiering.md](../../docs/compiler/tiering.md)). The budgets below apply in the optimized
   pipeline. `dev-speed` (dev at most 4x optimized, geomean, no case over
   10x) guards how slow dev code may get.

**Budgets.** Defaults for each pass's parameters. Only `closure_spec`'s
are built (crates/hd_opt/DESIGN.md §8); the rest wait for their passes.

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
- Planned, not built: **`hd build --size-report`**, printing bytes per
  section, the largest functions with their instance counts, folded
  bytes, thunk counts per method, and code bytes per source module, read
  from the linked module, so it costs nothing in a normal build.

**What the policy buys** in a pipeline that runs every pass (estimates):

| Target | Without these rules | With them |
| --- | --- | --- |
| `runtime` (geomean at most 1.5x Node, no case over 3x) | `map` above 3x on wasmtime from the hash protocol; chains slow from `call_ref` | `map` near 2x on wasmtime; chains near 1x; the heap pathology removed |
| `allocations` (0 per counted loop, at most 1 per chain) | 3 to 5 per map lookup | 0 for both |
| `size-startup-heap` (tiny at most 2 KB) | short literals at 3 bytes per byte | the pool; hello world needs none |
| `dead-code` | list code per reference type | folded by A1; the target is re-based after spike S7 |
| `long-run-memory` | slices that pin sources; heap growth pathology | the heap is sized; a small string slice still pins its source (Go-style strings): a known risk this metric watches |

## 13. Monomorphization And Merging

Code per concrete type, then merge byte-identical functions, in both
pipelines.
 Dictionaries exist only for
trait values and for the type witnesses of a generic method called
through `dyn` (§13.5).

### 13.1 Roots

| Program | Roots |
| --- | --- |
| executable or task (`hd run`, `hd build`) | the entry wrapper of `main` or `main!` (§16.2), and the init function of every group the entry module reaches |
| `hd build FILE` | the same, for FILE's module |
| script: an entry module with top-level statements and no `main` | the module init (`Roots::Script`); its entry row (`module.init.script-row`) is inferred from the init's direct item calls, and `hd_wasm::script_entry` emits it |
| unit test program (one per package) | one `TestCase` body per registration and per doc test, in every module of the package, and one init export per module with tests, which runs that module's reachable init groups |
| integration test program (one file) | its `TestCase` bodies, and its reachable init groups |
| REPL input | the input's top-level statements as an init body |

Only reachable items are compiled, so dead std code never reaches the
binary, and the import list holds only reachable host methods
([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)).

### 13.2 Collection

`hd_mono::collect` returns `Collected`: the instance table
(`layout::InstanceTable`: per instance its key, item, sub-body and type
arguments, with `content_order` sorting by key), each instance's call
targets, vtables, witnesses, compiler-supplied bodies and callee
representation hashes.

A worklist walk, as rustc's collector does:

1. Push the roots with no type arguments.
2. Pop an instance. Map its module's `tir` entry. Scan the body's `tags`
   column for calls, closures, coercions and host calls, substituting the
   instance's type arguments into each one's types.
3. For each `Item` callee, push the callee with the substituted
   arguments. Planned, not built: **in a pipeline that runs
   `inline-trivial`, a callee that is always inlined is not pushed**
   (the pipeline hash is in the code key): when its `inline_summary` says
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

**Body-less intrinsic impls.** A body-less impl method in a
compiler-known intrinsic family has no TIR body and therefore no body or
cache lookup. When collection selects it, it maps the intrinsic key plus
the substituted self and trait arguments to the compiler-generated body
and continues the same worklist from that body. A body-less method that
has no registered intrinsic mapping is an internal unsupported case; it
must not silently disappear from the reachable program.

**Derived newtype methods reuse the base implementation.** A derived
newtype impl has a head and no method bodies. Generate nothing: a newtype is erased to its base at runtime
([Newtypes](../../docs/compiler/lowering-catalog.md#newtypes) — no wrapper, no allocation,
construction and unwrapping emit nothing, and instances over the
newtype and the base fold). When collection selects a method of a
derived newtype impl, it pushes the corresponding method of the base
type's implementation with the same type arguments — the same rule as
the intrinsic mapping above, with the base impl in place of a generated
body. Rewrapping `Self?`, `Result[Self, E]` and `List[Self]` positions
is a type-level fiction: at runtime those values already are base
values, so no adapter runs. Positions outside that set never reach
collection: any other `Self` position is a `newtype-derivation-self`
error at the opt-in. This is sound only where the method means the
same on both types — Eq, Hash, ordering, and Debug, whose derived form
renders the wrapped value as the base type's Debug does. `TypeId`
stays distinct (identity follows `canon`, not representation), and
`Structure` methods are out of scope: a newtype gets no `Structure`
(`trait.derive.newtype.templated`). Cost per derived newtype: no new
code — the instances fold with the base type's — only the impl
identity in the selection table.


**A1 at collection.** Each generic item carries a **representation summary** per type
parameter, computed at check time and stored with its TIR, so the item's
TIR hash covers it. The summary says whether instances need the
parameter's exact representation. An own type parameter stays exact
when a data type, tuple, function type, trait value or projection in
the item's signature or body holds it: those change their Wasm type
with it, and erasing one cast a `Box[Point]` to `Box[REF]`. Only a
value of the parameter itself, `mut`, an optional, and the erased
storage of a `List`, `Map` or enum payload keep it movable. (Reading
only the body's own trait calls on `T` is not enough: `relay[T < Shape]`
passes `T` on to `total[T < Shape]`, which needs an impl at the exact
type.) When the parameter is not exact, the body
only moves `T`, and collection replaces that type
argument in the pushed instance by its **class**: `REF` for a layout of
one non-null reference, `REF?` for one nullable reference, or the scalar
or `void` class. `REF` and `REF?` stay apart because `T?` differs between
them: a nullable reference for `REF`, a tag and a reference for `REF?`.
So `List[Point].push` and `List[User].push` are one instance, emitted,
cached and compiled once. A type argument with **drop glue** stays exact
(design 6.4's drop-aware folding): a concrete type has glue when it
implements `DropOwn` or holds by value a part that has glue, so dropping
it runs some `drop`. Every drop of a value with glue calls the instance of
`std.ownership.drop` at its type, keyed with no class, whose TIR the
compiler supplies from the type's declaration (`ProgramEnv::drop_body`):
its `drop`, then each part as a statement-end temporary. The callees of a class instance see the class
as their argument too. The rule reads only the item's signature, its own
body and the argument's layout, so it is type-only. With bounds decided
by the signature, only closure types still read the body. At 10k lines the compile study
models 12 percent fewer instances, 43 percent fewer Wasm types and 9
percent less code. Erasing reference parameters of function types as
well (A2) is not adopted: it puts a cast in every closure entry.

**`select`.** At an instance every type is
concrete, so `select` is a head match plus reconstruction, never a
proof ([crates/hd_check/DESIGN.md (trait solver) §8.3](../hd_check/DESIGN.md#83-what-codegen-does-with-it),
rule TS-6):

| Step | What it does |
| --- | --- |
| 1. Head match | match the concrete trait reference against the heads in its owner modules. Every trait argument is known, so the candidate directory is not read. Overlap is head-only and the checker proved the bounds generically, so at most one head matches. No match is an internal error (§12.7), never a diagnostic |
| 2. `Bind` steps | run the impl's bound plan's `Bind` steps in plan order, through `normalize_concrete`. Each reads an associated-type binding at concrete types and fixes one impl parameter that the head does not fix |
| 3. Skip `Bound` steps | the checker proved them, so `select` proves no subgoal, has no depth limit and charges no fuel. Implementation differs: `select` is the solver's own search over the concrete goal and proves all of it, including a tuple template's member steps ([crates/hd_check/DESIGN.md (trait solver) §3.9](../hd_check/DESIGN.md#39-compiler-supplied-impls)); not judged |
| 4. Return | the impl and every impl argument. In `impl[T < Display, I < Store[Item = T]] Summary for Feed[I]`, matching `Feed[ConcreteStore]` fixes `I`, and the `Bind` step fixes `T` |

`select` and `normalize_concrete` keep their answers in one **selection
table** per run, keyed by the concrete trait reference, and shared
across programs. It is not the solver's proof memo: a selection assumes
a proof and does not make one, so a selection entry and a proof entry
never share a key.

The order of the walk never reaches output: the result is sorted by
instance key before anything is emitted. Collection is serial per
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

The body hash stays out of the symbol: with it, an edit to a callee's
body would change the symbol its callers' cached code relocates to while
their code keys still hit, and link would find a stale symbol.

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
- The diagnostic `instantiation-too-deep` points at the generic call that grows the type, and shows the first
  three and the last instance of the chain, with types printed in full.
- This is a build error, reported by `hd build`, `hd run` and `hd test`,
  with no boxed fallback. `hd check` does not report it, since it does
  not collect. Two checks report the same code earlier: a private row
  that grows by polymorphic recursion
  ([crates/hd_check/DESIGN.md (type checking) §5.5](../hd_check/DESIGN.md#55-private-rows-and-the-m3-fixpoint)),
  and a generic call cycle of one module whose type arguments grow
  (`types.generic.polymorphic-recursion`, `hd_check::inst_cycles`). So the
  limit, and the stop when an instance re-enters an ancestor through
  reference layouts, are left with growth through a bound or a trait
  value, and with cycles that span files.

### 13.5 Dictionaries: Trait Values And GADT Evidence

- **Vtables follow the trait record's shape** (crates/hd_check/DESIGN.md (trait solver) §9.2).
  The shape is computed once per trait at interface time:
  one slot per method of the trait itself, in declaration order; one
  immutable reference per direct supertrait, in declared order, to that
  supertrait's own vtable at the same type; and a type id when the trait
  is `Inspectable` or extends it. Supertrait slots are never copied, so
  a diamond such as `Error < Display & Inspectable` shares one `Display`
  vtable, and the `Supertrait` coercion is one `struct.get`. Each vtable
  is a global with a constant initializer, built once per `(type, trait
  reference)` pair, so a coercion to a trait value never allocates a
  vtable.
- Implementation differs: the emitter builds a vtable with `struct.new`
  at every trait-value coercion (`Em::vtable_value`): its own method
  slots and its direct-supertrait fields, a diamond's shared supertrait
  built once per coercion. So every coercion allocates a vtable tree.
- **Trait values** are pairs: the value as `eqref` and its vtable
  (§15.2). A call through a trait value is one `struct.get` and one
  `call_ref`.
- **Bindings live in the vtable key.** A `dyn Tr[A,
  Item = T]` type keeps its bindings in the pool (`TraitValue.bindings`,
  interned in sorted order, so binding order never makes two types,
  `trait.dyn.binding.identity`). Emission lays a vtable out per *key*:
  the trait, its arguments, and the binding of every associated type the
  trait reaches (`Lay::dyn_key`). The value's own bindings join those its
  supertrait list fixes (`trait C < S[Out = U]`), so the key is the same
  however the bindings were spelled. A slot's signature substitutes the
  class `REF` for `Self` and then reads each projection of `REF`
  (`Self::Item`) from the key's bindings
  (`trait.dyn.binding.signatures`), so the slot of `get` in `dyn
  Supplier[Item = i32]` is `(eqref) -> i32`, and `dyn Supplier[Item =
  string]` gets a vtable of its own. A parent field's key is the
  supertrait with its supertrait-list bindings plus those of the child's
  bindings it reaches (`trait.dyn.binding.widen`), so widening still
  reads one parent field. Requirement-key vtables use the same key, so a
  `$ Store[Item = User]` provider's `load` returns a `User`. A trait's
  layout hash covers its supertrait list, whose bindings shape the key.
  A projection and a binding name the item of the trait that declares
  the associated type, which resolution finds through the supertraits
  (`hd_resolve::assoc`, crates/hd_check/DESIGN.md (trait solver) §4.1): `dyn NamedSupplier[Item =
  i32]` binds `Supplier.Item`, the item `Self::Item` in `get` names, so
  the lookup is by item, never by name. A `dyn` type also takes the
  bindings its supertrait lists fix, so `dyn Count` under `Count <
  Supplier[Item = i32]` is `dyn Count[Item = i32]`.
- **Delegated implementations are ordinary ones.**
  Once a folder's items are lowered, `impl Tr for C by E` gets, for each
  associated type of `Tr`, the binding `<F as Tr[A]>::Name`, where `F` is
  the embedded field `E`'s type at `C`'s arguments (crates/hd_check/DESIGN.md (trait solver)
  §3.10), and, for each method of `Tr` with a receiver that its body does
  not write, a forwarding method item with the trait method's signature
  at `Self = C[...]` (`hd_resolve::lower::delegations`). The checker
  writes each forwarder's TIR as `Tr::m(self.E, arguments...)`, a
  trait-method call at the part's type with the part's evidence
  (`hd_check::delegate`), as it writes `@error` bodies (§13.14). So
  selection, instances and vtable slots see a written impl: a slot
  points at the forwarder's instance, which calls the part's method; a
  rest parameter passes on as the list it is, and a default method runs
  the part's default with the part as receiver (no overriding). A part
  that does not implement `Tr` gets no forwarders; the header reports
  `invalid-delegation`.

- **Generic methods called through a `dyn` value** use the type-witness
  ABI below.

#### 13.5.1 The Erased Method ABI

A generic method called through a `dyn` value has one erased body per
impl, accepts any type argument, and gets a type witness per call.
Everything else is monomorphized. Examples are a user trait's `fn visit[T](self, f:
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
the slot, would remove the erased path altogether, at a whole body per
pair; it was not chosen.

**Rejected alternatives.** A runtime "representation factory" that
builds `Buffer[T]`'s layout from `T`'s witness, as Swift's metadata
accessors do, cannot work on Wasm GC: a struct type cannot be created
at run time. A uniform erased layout for every generic type that may
reach erased code (`Buffer[T]` with `anyref` fields everywhere) would
slow every monomorphized use of that type. Copying a caller's container
into such a layout would break the identity of data values.

**As built.**

- A slot takes its method row's providers after the arguments and the
  witness (`Lay::slot_keys`), so a suspending method with a row works
  through `dyn`; the slot adapter passes the impl method's keys on.
- Open parameter `k` is the placeholder `Canon(0xE0 + k)` (`open_param`),
  laid out as `eqref`. The erased body is the instance `Sub::Erased` of
  the impl's method, or of the trait's default, at the impl's arguments
  and open parameters; thunk `k` is `Sub::Thunk(k)` at the caller's
  type arguments (`hd_mono::erased`).
- A family is the trait method with its trait's arguments. Its witness
  is an immutable `(array (mut eqref))` holding one struct of thunk
  references per erased body. The body reads its field at the index in
  an `i32` constant global (`GSym::WitnessField`) that link assigns, in
  place of a `Field` relocation and a per-family struct type, so its code
  does not depend on the program's other impls. This is simpler than the
  per-family struct and keeps the same independence.
- A non-reference value crosses in a `box_of` struct of its values, as a
  trait value's payload does, except an `i32` that fits in 31 signed
  bits, which crosses as an `i31ref` (crates/hd_wasm/DESIGN.md §15.2).
- A tag switch over an open value outlines its tag read; `Match` does
  not read its scrutinee and stays.
- A method's own row parameter (`fn run[$R]`) is open too: the erased
  body takes `Canon(0xE0 + k)` for `R`, so a row that lists `R` lists
  that placeholder as a key. Its keys cross as one context in the
  function-value form of §12.4 (`ctx_keys: array i64`, `ctx_provs:
  array eqref`), after the slot's providers. The slot's providers stay
  those of the keys the method's row lists itself
  (`declared_row_keys`). The caller puts in the context the keys of
  each row parameter's argument that the row has (`row_param_keys`).
  Every row-polymorphic instance uses the same ABI (§12.4, "As built").
- In the erased body, a function value whose row lists `R` gets the
  context unchanged when no `$.with` scope is open and the row has no
  other key, so forwarding is free. Otherwise its other keys and the
  open `$.with` keys go in front of a copy, so the nearer provider wins
  (`req.with.nearest.row-parameter`).
- A call whose type arguments include a row that lists `R` is an open
  instruction, as for an open type: its thunk, at the caller's row,
  calls the callee's one instance with that row's keys in the context.
  A thunk of a method with row
  parameters takes, after the body's providers, a context of the other
  keys in scope in front of the open one; a key no provider covers is
  looked up there.
- Planned, not built, each an `unsupported` stop: a closure in a body with
  open instructions (the escaping `fn(T) -> T` of the first test), a
  suspending generic method, a loop or a non-tag switch over an open
  value, and a function reference to a generic method at a trait-value
  `Self`.

### 13.6 Tuples, Arity And `all!`

There are no variadic generics. Tuples are ordinary types:

- `Args < Tuple` instantiates like any type parameter, so `f(args...)`
  is one instance per tuple type.
- Tuple `Eq`, `Ord`, `Hash` and `Debug` are instances of std's tuple
  templates. The checker records an `Impl`
  choice of the template, and collection instantiates the template's
  instance per tuple type, like any impl. A user library's tuple
  template works the same way. No body is generated per arity.
- Only sealed traits are `Builtin` (`Any`, `AnyVal`, `AnyRef`,
  `Inspectable`, `Tuple`, `Suspend`; `Num`, `Integer` and `Float` are
  std's written impls, crates/hd_check/DESIGN.md (trait solver) §3.9). Those with methods get a
  body that the emitter generates per primitive or shape, such as `type_id` and
  `downcast` for `Inspectable`.
- **A tuple's `Structure`.** A tuple template's
  body calls `Structure::walk` (and `describe`, `build`, `name`, `facts`)
  on its `T`, a `Bound` choice, and the spec has the compiler generate the
  tuple's `Structure` while it instantiates the template
  (`annot.tuple.structure`, `.members`, `.rest`, `.build`). Checking keeps
  the `Bound` choice; only codegen meets a concrete tuple. There, the
  trait-solver §3.9 `Structure` row answers a concrete tuple type with
  `Builtin(Structure)` (a declared type still gets its derivation's
  generated methods, §12.3). Collection lowers each `Builtin(Structure)`
  method per tuple type as a compiler-supplied instance, keyed by the
  tuple type like any instance and cached the same way. Its body comes from
  the same generator that writes a declared type's `Structure` at check
  time: the generator is a TIR builder over a target's member list, with
  no checker state, and a tuple's members are its
  elements `_0`, `_1`… (`annot.tuple.members`), and a rest element is its last member, of type `List[T]` (`annot.tuple.rest`). No body is
  generated per arity; one is generated per tuple type that a program's
  instances reach.
- **As built.** The row answers every tuple type, with no mode:
  rows read no context, so their memo entries must not depend on who
  asks. Checking never asks it at a concrete tuple, since `Structure` is
  named only inside a template (`annot.structure.named-positions`), where
  a tuple template's `T` is a parameter that the environment answers. The
  generator is the crate `hd_structure`: a `Target` (type, `name()`,
  variants with their members and how each is read and built: fields,
  payloads or tuple elements) and a `Cx` (the run's names and an `Items`
  view of declarations). The checker builds a derivation's target and
  gets hidden methods of the derived implementation, handles included
  (§12.3). Collection asks `ProgramEnv::supplied_body(method, tuple)`,
  which the driver answers with `tuple_target` and `supplied`: the body
  belongs to the trait method itself, its signature the trait's with
  `Self` replaced, so the instance is `(Structure.m, [tuple, own
  arguments])`, keyed like any instance. A tuple has no implementation to
  hold hidden handle methods, so its handles are built where the
  traversal passes them, and their closures are instantiated per walker.
  Both a `Builtin` callee choice and `select`'s `Builtin` answer go
  through it; a `None` keeps emission's own lowering (`Inspectable`).
  `Collected::supplied` holds the bodies by `(method, tuple)`, and
  emission reads them there. The code key's TIR hash is 0 for such an
  instance: the instance key names the tuple, and the toolchain key covers
  the generator and std. A rest member is read as the last element and
  passed to `w.rest` (`annot.walk.rest`); std's default `rest` panics
  instead of calling `member` (`annot.walk.rest.default`), and the checker
  builds no rest tuple value yet.
- `all!` is one intrinsic frame instance per tuple of child result types
  (§14.5). `race!` is an ordinary generic intrinsic over `T`.

### 13.7 Merging Byte-Identical Functions

Not built yet; see #175 ("merge identical functions"). Link folds
nothing, and no `hd.folds` section is written. The design:

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
would fold only when `Point` and `User` have the same field layout. With
A1 they are one
instance from collection on (§13.2), so folding is left with scalar
twins, equal field layouts and identical helpers.

**The algorithm (after safe ICF in linkers).** Fold exact duplicates
only:

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
   **A worklist:** after the first round, only bodies
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
lines), so a dev build without it would compile slower.

### 13.8 Code Entries

`Emit(inst)` lowers one instance to Wasm with `wasm-encoder` into a
code entry, `hd_wasm::Code`, which its folder group's `codepack` stores
(docs/compiler/data-structures.md §3.22):

```rust
pub struct Code {
    pub params: Vec<VT>,
    pub results: Vec<VT>,
    pub body: Vec<u8>,              // locals and instructions; index immediates as 5-byte padded LEBs
    pub relocs: Vec<(u32, Reloc)>,  // sorted by offset
    pub lines: Vec<(u32, u32)>,     // offset of each call that starts a new anchor -> TIR instruction index
    pub parts: Vec<(Part, Code)>,   // a suspending instance's f$body, f$poll, f$cancel
}
pub enum Reloc {
    Func(Sym),     // an instance, an import, or a runtime helper (a literal's getter among them)
    Type(WTy),     // a canonical Wasm type descriptor
    Global(GSym),  // module storage, runtime globals and immutable constants (witnesses and their fields, closures)
}
```

An anchor is the item, known from the function's name, and a TIR
instruction index: no byte offset, no line. There is no site record: a
panic's site is its stub caller's `lines` row (crates/hd_wasm/DESIGN.md
§15.9). A witness field index is an `i32` constant global
(`GSym::WitnessField`), not a `Field` relocation (§13.5.1).

**Recursive descriptors.** A type naming a recursion group
([crates/hd_wasm/DESIGN.md §15.3](../hd_wasm/DESIGN.md#153-the-type-section)) is
the group and a member index. The entry's bytes start with its type
table: each distinct descriptor and group the entry names, directly or inside another, once
and after the ones it names. A row names the descriptors it holds, and a
member's group, by their places; so do the signature, the `Type`
relocations and the types inside symbols. No descriptor is written twice
and no row nests another. Link decodes the table once per entry, and
equal descriptors come back as one shared value (on one large `main`,
1.88 MB of entry became 36 KB, and its decode 14 ms became 0.6 ms).

**Stable numbering.** An emitted body holds no
program-wide dense number that changes when something is added
elsewhere, apart from function indices in `call`, which wasmtime's
per-function cache abstracts, and the measured exceptions of
[docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#numbering) (type immediates,
direct constant globals, `ref.func`, witness fields). So there is no
`Site` relocation: a site is the code offset of its stub call
(crates/hd_wasm/DESIGN.md §15.5). There is no `Data` relocation in a
body: only the literal getters name the data segment, and a literal use
is a call of its getter, keyed by the literal's bytes
(crates/hd_wasm/DESIGN.md §15.4). So the relocation kinds are `Func`,
`Type` and `Global`. Context key ids are content hashes written as
constants. An index space that leaves fewer than 95 percent of functions
hitting wasmtime's cache after one addition moves behind a getter.

**Positions.** A code entry holds no source
position. A site or a statement line is an **anchor**: the item's stable
path and the index of a TIR instruction in its body. Instruction indices
do not move when a comment or a blank line is added, so the TIR hash
leaves the span columns out, and so do code keys, the fold key and
`prog_key`. A panic's reported location is still program output, so an
anchor is resolved against the current sources whenever a location is
printed:

- `hd run`, `hd test` and the playground keep the anchors in the linked
  module's `hd.lines` (§13.10). On a panic or a failing
  test, the host looks the anchor up in the module's current `check`
  entry: the instruction's span columns give byte offsets, and its `locs`
  section gives the line. That costs microseconds, only when a location
  is printed, as for a diagnostic's token anchor (crates/hd_cache/DESIGN.md §5.3).
- `hd build` writes a file that must stand alone. When it copies the
  `link` entry to `build/`, it resolves every anchor to a file, line and
  column in the written sections: one pass over about 5,000 rows at 10k
  lines, plus one map of each reachable module's `check` entry, a few ms
  per build. The cached `link` and `cwasm` entries stay anchored.
  Implementation differs: the written rows stay anchored, and an
  added `hd.positions` section holds each row's line and each
  function's file and printed name, with no column
  (crates/hd_wasm/DESIGN.md §15.9).

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
                         emit options, engine options)   (docs/compiler/tiering.md §6.4)
```

**`layout_hash` as built.** It hashes `canon(T)` and the
declarations of every item `T` names and of every item their declared
types name (fields, payloads, trait method signatures), however far, not
the instances' descriptors: an instance's layout reads only those, at
`T`'s arguments. Items that reach each other share one hash (Tarjan's
algorithm over items, memoized per item for the build), so the hash
covers a whole recursion group, and the walk ends on any recursion, a
payload at growing arguments (`E[List[T]]` in `E[T]`) included.

Implementation differs: `hd_cache::code_key` hashes the pipeline,
profile, instance key and item TIR hash with one dependency hash
(`hd_driver`, `Collect`): the toolchain key and the instance's callee
representation hash. That hash holds the `layout_hash` of every type the
instance's instructions have, each callee's path, instance key and
signature layouts, and the instance's own signature `layout_hash`es and
row keys. So a field added to one data type re-emits only the instances
that lay it out. The per-item interface hashes and selected-impl hashes
above are not read separately. Inline summaries and literal numbers are not
needed while nothing inlines and literals are keyed by content.

`pipeline_hash` replaces the earlier `tier` key part, and the profile is
a separate part, since `hd test --release` pairs the test profile with
the optimized pipeline. A pass version bump misses only the pipeline that
holds the pass.

**The dependency set is what collection and emission read, not what the
generic TIR names.** Generic TIR does not name the concrete types,
projected associated types and impls that appear under a substitution
Collection already finds all of them for this
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
- **Callee representation summaries.** The
  callee's instance key in the list is the one after A1 classification
  (§13.2): `first[REF]` or `first[Point]`. It picks the symbol the caller
  relocates to and whether the caller casts the result. So a change to a
  callee's summary, such as a new bound, re-emits its callers even when
  their own TIR is unchanged.
- **Runtime helpers** are generated by the compiler, so `toolchain_key`
  covers them.

Any new read that emission adds must join this list in the same change.

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
| Cranelift time | wasmtime's per-function cache |

A per-item instance budget stays a fallback, as the research says. Add it
only if the `dead-code` metric fails after folding.

**A compile model** puts a
10k-line application at about 4,800 instances, 4,700 emitted functions
and 640 KB under exact types, and about 3,600 functions and 530 KB with
A1, the boxing bound and closure specialization. Derived code is the
largest source (27 percent of instances), then iterator chains (14
percent), then `List` and `Map` methods (17 percent together). Skipping
always-inlined callees (§13.2) and the compact `hd.names` section
(about 120 KB at 10k lines) are the largest levers for bytes; stable
numbering, packs and filtered test programs are the largest for latency.


### 13.10 The Link Step

`Link(P)`:

0. **Read the code entries in one batch.** The worktree's last-run record, or else the
   program's `packhint`, names the **code packs** of the program's last
   link: one per folder group, each holding the code entries of that
   group with an index by code key (crates/hd_cache/DESIGN.md §5.4). Link maps those
   packs once and takes every unchanged code key from them; `Emit` runs
   only for keys no pack holds. It then writes a new pack for each group
   whose members changed. Reading 4,800 separate files would cost 0.3 to
   0.6 s on the Mac; about 30 mapped packs cost 2 to 4 ms.
   Implementation differs: `Collect`, not Link, reads the packs, and
   the hint is the package's, not the program's. A group's pack is
   keyed by its code keys, so an unchanged group is one exact lookup;
   only a changed group reads the packs its folder's hint lists. Packs
   are read whole, not mapped.
1. **Drop and fold.** An instance that no relocation names, because every
   caller inlined it, is dropped. Then the rest fold (§13.7).
2. **Order functions.** Imports first, sorted by module and name. Then
   the generated runtime helpers, sorted by name. Then the module
   literal getters and span functions, by module path, and the fact
   getters, by content. Then the
   representatives in instance-key order. This order is deterministic,
   but it does not keep indices stable: inserting a function with a
   smaller key renumbers every later one. That is
   harmless for `call`, which wasmtime's per-function cache abstracts. No
   persistent index allocation is kept, since it would make bytes depend
   on build history (crates/hd_wasm/DESIGN.md §15.8). If the `ref.func`
   case misses wasmtime's cache too often, closure construction reads one
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
   its module reaches, in `InitOrder`'s order (docs/compiler/engines-and-test-runner.md §19.1).
8. **Code.** Copy each representative's body and patch its relocations in
   place. In the optimized pipeline, re-encode the padded LEBs at minimal
   width (the `leb_compact` pass); the dev pipeline keeps them, since
   size does not matter there. Planned, not built: no build compacts LEBs.
   Re-encoding moves every later byte, so the encoder builds an
   old-to-new offset map per body as it goes (a sorted list of
   `(old offset, bytes removed so far)`, one row per shrunk immediate)
   and rewrites the offsets of `sites`, `lines` and the source map
   through it before writing `hd.lines`. A test panics after a shrunk immediate in the optimized
   pipeline and checks the reported site against the dev pipeline.
9. **Custom sections**: `hd.lines`, then the compact `hd.names` in a
   release build, so release omits the standard `name` section, or
   `name` otherwise (crates/hd_wasm/DESIGN.md §15.9). Planned, not
   built: `hd.runtime` and `hd.folds`.

```text
link_key = prog_key (§11.3), and on a prog_key miss
           H("link", toolchain_key, pipeline_hash, profile, root description, sorted code keys)
```

A program is written under both keys, so the next warm run hits the
cheap one. Link is serial per program and linear in output size.

### 13.11 Function References Become Adapters

Check builds an `ItemRef` node for a function used as a value
(`hd_check/src/call.rs`, `item_value`: the item plus its instantiated
type arguments). Emission turns every `ItemRef` into a value by
generating one adapter per instance, reusing the closure representation
(§12.2); no new runtime form.

For each reference kind of spec 07 (`fn.ref.*`, generic function
values):

| Kind | Example | TIR node | Adapter instance key | Adapter body | Receiver |
| --- | --- | --- | --- | --- | --- |
| plain function | `scaled`, `identity` as an argument | `ItemRef(item, args)`; args solved from the call/expected type, defaults applied, error if unsolved (`fn.type.generic.*`) | `H("adapter", item path, canon(args))` | forward `(args…, ctx)` to a `Call` of the item instance | none |
| unbound method | `Counter::bump` | `ItemRef` | as above | forward `(receiver, rest…, ctx)`; receiver first (`fn.ref.unbound.receiver`) | caller supplies it |
| bound method | `counter::bump` | a `Closure` node wrapping the adapter, capturing the receiver | as above (the receiver is data, not a type argument, so one key serves every receiver value) | read receiver from `env[0]`, forward `(rest…, ctx)`; a `mut self` receiver is a `Move` capture (creation already requires mutable access, `fn.ref.bound.mut`) | evaluated once at creation and captured (`fn.ref.bound.capture`) |
| associated function | `Counter::zero` | `ItemRef` | as above | plain forward | none |
| explicit type arguments | `Json::decode::[User]`, `Box::[i32]::get` | `ItemRef` with `b` = the written arguments | as above | plain forward | none |
| trait-qualified | `Trait::name` | `ItemRef` after `select` resolves `Self` from the expected type (`fn.ref.trait-self`; unsolved is `cannot-infer-type`) | as above | plain forward | none |
| suspending | `Store::load` | `ItemRef` of `fn!` type (`fn.ref.suspending`) | as above | plain forward; suspension is the caller's `CallValue` business, as for closures | none |
| variant constructor | `SyncError.Fs` as a value | `ItemRef` | as above | call the constructor (one lowering; no inline tag construction) | none |

Collection (§13.2) scans `ItemRef` like a call: it pushes the adapter
instance and the underlying item instance with the substituted
arguments. The adapter's TIR is synthetic (no source body): parameters
`(env, args…, ctx)`, one `Call` of the item instance, return its
result. Rows are not type arguments (§13.9), so one adapter serves
every caller row; the context passes through untouched (`RowSubsume`).
An adapter adds 1 to the instantiation chain (§13.4).

Wasm shape: the reference value is a `Closure` struct whose code field
is the adapter and whose env holds the bound receiver, if any. Every
capture-free adapter is a constant global, exactly like a capture-free
closure (§12.2); calls use `CallValue` (`struct.get` of the code, then
`call_ref` with the closure first). The adapter code has the closure
ABI `(env, args…, ctx)` (§12.4).

Footprint per adapter: one Wasm function (a forwarder, tens of bytes;
its type section entry dedups at link) plus, for bound references, the
receiver already allocated by the creator. Merging (§13.7) folds
byte-identical adapters, and bounded inlining can inline an adapter at
a known-callee `CallValue` site. Per §13.9's table, adapters are bounded
by reachability: only (item, type arguments) pairs actually referenced
as values exist.

### 13.12 Map Iteration And Removal

A map is the `$Map_K_V` layout (`index`, `hashes`, `keys`, `values`,
`used`, `live`; crates/hd_wasm/DESIGN.md §15.2), with runtime helpers in
`hd_wasm/src/map.rs`: entries in insertion order, a removed entry marked
by hash -1, and a linear-probing index.

**Iterator state.** `MapIter` builds a `Closure` struct `{code, map,
pos: i32}`, mirroring `ListIter`'s `{code, list, index}` and the same
closure ABI. The step function `() -> (K, V)?` scans entries from
`pos` to `used`, skipping tombstones, yields `(keys[i], values[i])`
for the first live entry and advances past it, and answers `.None`
when no live entry remains.

**Order.** Iteration follows entry order: replacing a value keeps its
entry (`types.map.replace.in-place`), removal writes a tombstone
without moving survivors, reinsertion appends a fresh entry at the
end, and growth compacts (drops tombstones, rehashes) preserving entry
order. That satisfies `types.map.order.deterministic` (same operations
→ same order every run) while staying unspecified beyond it
(`types.map.order.unspecified`).

**Removal.** `MapRemove(key)` probes `index` to the entry (past
tombstones, stopping at empty), writes the tombstone marker,
decrements `live`, and returns the old value as an `Option`
(`.None` when absent). Lookup probes the same way. Growth compacts
when the table fills.

**Other key types.** The key bound is already checked
(`Map[K < Eq & Hash, V]`, `types.map-key.declared-bound`): at Emit,
`MapGet`/`MapSet`/`MapRemove` hash with a monomorphized call of
`std.hash.key_bucket_hash` at the substituted key type, which feeds the
key type's `Hash` impl a `BucketHasher` (HalfSipHash-1-3 keyed by the
run's seed: the `map_hash_seed` primitive, the `hd:rt` `hash_seed`
import), and compare with its `Eq` impl on a hash match
(`types.map.eq-hash`). Collection pushes those instances as it does
`Bound` choices (§13.2 step 4), as `Target::MapKey`. An inline key
(integer, `bool`, `char`, `string`) compares inline and hashes through
one keyed function, pushed as `Target::MapInline`:
`std.hash.string_bucket_hash`, the same HalfSipHash-1-3 over a string's
bytes read in place, or `std.hash.int_bucket_hash`, Go's keyed
32-bit-platform mix of the key's bits as a `u64`. So keys chosen to
collide cannot be chosen without the seed
(`std-hash.map.bucket-hash`). The stored
`hashes` array keeps stale-key behavior (`types.map.stale`): a key
mutated after insertion keeps its entry, unreachable by a fresh
lookup. Footprint per `(K, V)` is one step function plus the iterator
closure, the same shape as list iterators; merging (§13.7) folds
identical steps.

### 13.13 Omitted Arguments, Default Bodies And Derived Facts

Check emits one `DefaultCall` per omitted argument, carrying the default
body's `DefId`, the call's type arguments and the earlier arguments'
`Ref`s. Emission calls a per-function default body, not inlined
expressions and not a wrapper.

A call that omits arguments emits the explicit arguments, then one
direct `call` of each omitted parameter's default-body instance in
declaration order, feeding each result into the call's argument slots
(`fn.default.eval`). The earlier arguments pass as values, so nothing
evaluates twice; the forbidden-context bracket applies as §12.3
describes. The alternatives lose: inlining every default at every
call site duplicates code per site, and per-callee wrapper functions
multiply by omission pattern (up to 2^D per generic instantiation).
The thunk shares one default-body instance across all call sites at
the same type arguments. Inlining of small defaults belongs to the
inlining passes (§12.6), with no special case; none is built.

Implementation differs: one case is inlined at collection. A default
body that is one scalar constant, such as `times: i32 = 3`, becomes
`Target::ConstDefault`. Its root block holds no instruction, and its
value is a constant that is not a string. Emission pushes the constant
at the call, so that default body has no instance (`scalar_constant`).

Footprint per function with D defaulted parameters, per distinct type
tuple that reaches it: one instance per default body (the default
expression's code, often a few instructions), plus one call sequence
per omitted argument at each omitting call site. Monomorphic
functions pay D small bodies total, shared by all callers; constants
pay nothing.

Derived defaults and facts use the same shapes. A derived
member's default body is an ordinary instance keyed by (default body,
substituted type arguments) and pushed by collection step 6. A
derived fact gets one lazy global per (fact, type arguments), since
its value can differ per instantiation, filled by its body's instance
on first read (§12.3); the getter is shared. Collection records fact
getters with their instance arguments (step 8), so each instantiation
resolves to its own global.

### 13.14 `@error` Derivation

`@error` is a compiler intrinsic, not an ordinary decorator
(`annot.error.intrinsic`), and for an error type `E` the compiler
generates `impl Display for E`, `impl Error for E`, and one
`impl From[P] for E` per `@from` member (`annot.error.generates`),
as ordinary implementations under ordinary coherence and overlap
(`annot.error.ordinary`, `annot.error.hand-written`).

Neither alternative source fits. A `std.error` template in the style
of tuple templates (§13.6) cannot see the variant payload names, the
shared fields, or the interpolated message scopes the messages are
checked in (`annot.error.message.checked`). Compiler-supplied solver
rows (crates/hd_check/DESIGN.md (trait solver) §3.9) answer whether a sealed trait holds, not how to render
this variant's message or wrap that payload — and `Display`/`Error`/
`From` are not sealed. So each `@error` site generates its bodies:

- `Display::to_string`: a match over the variants. Each arm builds
  its interpolated message (`annot.error.message.*`); a transparent
  variant or data type forwards to its member
  (`annot.error.transparent.display`). Needs no imports
  (`annot.error.no-use`), as `error-without-import.hd` requires.
- `Error::cause`: a match returning the `@from`/`@source` member, or
  `.None` without one (`annot.error.cause.*`); an `E?` source matches
  `None`/`Some`; a transparent member returns its own `cause`
  (`annot.error.transparent.cause`). The cause member's type must
  implement `Error`, checked as any bound
  (`annot.error.cause.type`).
- `From[P]::from` per `@from` member: construct the variant (or the
  data value) holding the payload. The `?` operator's conversions
  then dispatch through these impls ordinarily.

Resolve sees the generated impls exactly like hand-written ones, so
coherence and the hand-written overlap errors fall out with no extra
work. Collection instantiates each used body per type arguments, as
for any impl. Footprint per error type: one `Display` body, one
`Error` body, and one `From` body per `@from` member, each the size
of a small match, plus the pooled message literals — all shared
across call sites at the same instantiation and folded when identical
(§13.7).

**As built.** Resolve lowers each error type's
headers as ordinary impls (`ImplKind::Error` in `hd_resolve`), and the
checker writes their bodies directly (`hd_check/src/error.rs`) rather
than instantiating a std template: a message needs its variant's scope
(named, `_N` and shared members, not `self`) and its source spans for
diagnostics, which the `@derive` template path cannot give. An error
type with no cause omits `cause` and takes the trait default. A
three-variant error measured within 15 bytes of the same impls written
by hand.

### 13.15 Integer Widths

Values of sub-word integers live as `i32` scalars in
locals, packed `i8`/`i16` only in fields and array elements
(crates/hd_wasm/DESIGN.md). Field and array loads sign- or zero-extend, so
every sequence below starts from a full-width exact value.

Per operation and width, with `w` the bit width and `m` the mask:

| Operation | 32/64-bit | Sub-word (`i8`/`u8`/`i16`/`u16`) |
| --- | --- | --- |
| `+`, `-`, `*`, `**`, unary `-`, checked (debug/test) | widen to 64 bits (`i64` ops), compare against the type range, panic `integer-overflow` outside it | same in 32 bits (exact: no two `w`-bit values overflow it), range-check against the type's range |
| same, release | the Wasm op (wraps natively) | the Wasm op, then mask to `w` bits and re-extend to canonical form |
| `wrapping_*` (every build) | the Wasm op | the Wasm op, then mask and re-extend, as release above |
| `saturating_add`/`sub` (never panics) | unsigned: wrapping op, carry/borrow test selects the bound; signed: sign analysis selects `MIN`/`MAX` | exact in 32 bits, then clamp to the type range with selects |
| `checked_*` | as checked, but the overflow flag selects `.None` instead of panicking | as checked, selecting `.None` |
| `abs_diff` | `(a > b) ? a - b : b - a` in unsigned arithmetic | same, on the extended values |
| narrowing cast (never panics) | `i32.wrap_i64` / extends | keep the low `w` bits (`types.cast.wrap`), re-extended: `300 as u8` is 44 |
| `count_ones` → `u32` | `i32.popcnt` / `i64` popcnt plus extend | mask to `w` bits first, then popcnt (`-1i8` counts 8, not 32) |
| `leading_zeros` → `u32` | `i32.clz` | mask, `clz`, subtract `32 - w` (zero gives `w`) |
| `rotate_left`/`right` | native `rotl`/`rotr` | `((x << n) \| (x >> (w - n))) & m`; shift counts masked to `w` in release, `invalid-shift` panic in debug unless valid |
| `<<`, `>>` | `shl`, `shr_s`/`shr_u`; count validated in debug (`invalid-shift`), masked to the width in release | same, on extended values, result masked and re-extended |
| `/`, `%`, `MIN / -1` | native traps (identical in every build, `types.arith.always`) | same |

The checked/wrapping choice is selected by the build profile for the
operator forms (`types.arith.checked` vs `types.arith.release`) and
by the method name for the explicit `checked_*`/`wrapping_*`/
`saturating_*` forms (identical in every build,
`types.arith.always`). Code entries are per profile: debug-checked
and release-wrapped sequences never share a cache entry.

As built: the operators carry the widths. Casts, `<<` and `~`
end canonical; a 64-bit `*` is checked by dividing the product back,
and a 64-bit `MIN / -1` is tested before `i64.div_s`. The `std.num`
methods stay ordinary hd over those operators, as
`spec/std/num.md` says, so they get no dedicated sequences above.
Native sequences would need spec-named intrinsics, a later speed
item.

As built: `PrimOp::Pow` is one `Prim`. An integer base runs
square-and-multiply over the exponent widened to 64 bits, with each
product the checked multiply above; a square is made only while exponent
bits remain, so no product exceeds the final magnitude and a result in
range never panics. A float base calls the generated `PowF64` helper
(`pow.rs`, `(f64, f64) -> f64`), built once per program that uses it; an
`f32` operand is widened first and the result rounded back. The helper
follows IEEE 754 clause 9.2 for the special cases and computes
`exp(y ln x)` in double-double arithmetic otherwise, then rounds once.
No host import is involved, so every host gives the same bits.

As built: floats use the instructions of their own width, so
`f32` `+ - * /`, negation and the comparisons round once to `f32` and
follow IEEE 754 for NaN, infinities and signed zeros. Floating `%` is
the generated `RemF64` helper (`rem.rs`, `(f64, f64) -> f64`), C `fmod`:
exact, with the dividend's sign, and NaN for a zero divisor, an infinite
dividend or a NaN operand. A floating-point quotient does not give the
remainder (`a - b * trunc(a / b)` rounds twice), so the helper works on
integers. It splits both operands into a 53-bit significand and an
exponent (subnormals normalized with `clz`), then subtracts the divisor's
significand from the dividend's where it fits, doubling once per step of
the exponent difference, as musl's `fmod` does. The loop runs at most
2045 times and every step is exact, so there is no rounding to argue
about. An `f32` operand pair is widened, remaindered, and narrowed: the
remainder of two `f32` values is an `f32` value, so the narrowing is
exact. Casts are one `convert` lowering: `f32` to `f64` is `promote`,
`f64` to `f32` is `demote`, integer to float is the `convert` instruction
(one rounding, ties to even, also at `i64`/`u64` to `f32`), and float to
integer is `trunc_sat`, which gives the bound for an overflow or an
infinity and 0 for NaN. The 8- and 16-bit targets saturate to `i32`
first and clamp to the type's range, which agrees with saturating
directly because the clamp is monotone.

### 13.16 Supertrait Calls Through A Trait Value

The spec exposes transitive supertrait methods on every dynamic value
(`trait.dyn.value-methods`, `trait.dyn.supertrait-methods`) and lets
a `dyn` child widen implicitly to a supertrait value
(`trait.dyn.widen`, bindings kept per `trait.dyn.binding.widen`).

A child vtable holds its own methods' slots plus one vtable field
per direct supertrait (the `Supertrait` coercion already reads "the
parent's vtable field from the child's vtable"). A supertrait method
call through a `dyn Sub` value is two loads — parent vtable, then
slot — and one `call_ref`, reusing the `CallDyn` shape; generic
supertrait methods use the witness ABI (§13.5.1) unchanged, since the
witness travels per call, not per vtable. No flattening: parent
method code is referenced once per (type, declaring trait), not once
per child trait.

Widening `dyn Sub` to `dyn Super` allocates nothing: it keeps the
payload and reads the parent's vtable field from the child's vtable,
one `struct.get` per level (the `Coerce` row's `Supertrait` case).
Reusing the child vtable as-is would need the child's vtable type to
be a Wasm GC subtype of the parent's, which only flattening gives, and
this design does not flatten. Child-only
methods become unreachable (`trait.dyn.widen` drops them,
`trait.dyn.no-narrow` never returns).
Vtables exist per (concrete type, trait) pair (§13.2 step 5),
extended by the associated-type bindings the pair carries.

Footprint per trait value stays two words. Per (type, trait) in the
used hierarchies there is one vtable of own slots plus parent
fields; method bodies are shared with the direct-call instances.
Implementation differs: vtables are built per coercion, not once per
pair (§13.5).


### 13.17 Build Profiles And Pipelines

The spec has three profiles: debug (checked), release (wrapping) and
test (always checked), and a separate, unobservable pipeline choice (dev
or optimized). The driver takes the profile as a build input
(`Packages::build_profile`) and the pipeline as a target and level
(crates/hd_opt/DESIGN.md §5); `hd test --release` stays checked and only
switches pipelines. The profile joins `code_key` and `prog_key`, and the
pipeline hash joins every key, so debug, release and test entries, and
dev and optimized ones, never share cache entries. Keying is enough: a
prebuilt module records nothing further, and a mismatch is a miss, never
a miscompile.

Per profile, at Emit: debug and test emit the checked arithmetic
sequences of §13.15; release emits the wrapping ones. `dbg` never
reaches Emit in a release build: Check rejects it there
(`module.dbg.release`), while test builds, `hd test --release` included,
accept it. A dependency `dbg` call is accepted with `dbg-in-dependency`
(`module.dbg.dependency`) and lowers to argument evaluation with no
print, keeping left-to-right evaluation. Release link writes `hd.names`
in place of the `name` section; the line table is separate, so release
keeps panic frames.

### 13.18 Provider Keys Carry Their Type Arguments

The spec keys providers by the whole trait reference — a generic key is entailed only by an
identical key (`req.row.entail.generic`), and a key occurs at most
once per row (`req.row.set.once`), so `Repo[User]` and `Repo[Post]`
are two providers.

The key is the full canonical trait reference: trait plus type
arguments (and bindings), compared as interned `Ty` values — pointer
comparison, no hashing cost. It is compared in three places, all by
the full key:

- **Check** (`require_key`, row entailment): unchanged, already
  exact. Duplicate keys in one row stay a Check error, so lookup
  never faces an ambiguous key.
- **Collect**: callee-record `(key, provider)` pairs already carry
  full keys; matching a call's providers into a callee instance —
  including one provider per instantiation of a generic
  `fn f[T](... $ Repo[T])` — compares full keys, which falls out of
  instance keys carrying the type arguments. A type parameter named by
  a requirement key (the item's own row, a function type's row, a
  `$.with` key) is exact under A1 (§13.2), or the key would be erased.
- **Emit**: the provider list and `push_providers` key by full key,
  so `ProviderGet` for `Repo[User]` never matches `Repo[Post]`.
- **Host**: host capabilities are never generic — a generic key
  cannot come from the host (`nonhost-entry-requirement`) — so the
  host lookup path stays trait-keyed and untouched. A function
  value's context ids are the low bits of the key's content hash
  (`canon` of the whole key, as a row's content order uses), so
  they include the arguments.

Cost per call: zero at run time (the same locals and the same
context walk); at compile time, one interned-`Ty` comparison per
lookup.
