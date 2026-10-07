# New Compiler Design: Back Half, Codegen

Part of the [compiler design](../README.md).

## Part D2: The Back Half

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
    Emit(InstanceSlot),          // created by Collect, one per code-entry miss
    Link(ProgramId),             // after every Emit of its program
    Precompile(ProgramId, Tier), // native only
    RunCase(ProgramId, CaseIdx), // test runner (§19); the browser runs cases in its worker instead
}
```

- `Collect` starts only after the `tir` entries of every module that the
  program's roots reach in the module use graph exist. That set comes
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

### 12.1 One Walk Per Instance

`Emit(inst)` reads the instance's TIR from its module's mapped `tir`
entry and walks the root block's list in order, with a stack of open
blocks. For each instruction it:

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
| `Call` with a `TraitMethod` callee | the impl is selected per instance by the solver (§13.2); a direct `call` |
| `Call` with an `Evidence` callee | a `call_ref` through the vtable stored in the variant (§13.5) |
| `CallDyn` | `struct.get` of the vtable slot, then `call_ref` |
| `CallValue` | `struct.get` of the closure's code, then `call_ref` with the closure as the first argument |
| `CallHost` | an import call with the exchange-buffer codecs (§17.2) |
| `Closure` | a struct of the closure's environment: `Copy` and `Move` captures as fields, `Shared` ones as their cells; a closure with no capture is a constant global |
| `Coerce` | option wrap: a tag set or nothing (§15.2); to a trait value: a pair with a constant vtable; readonly view and row subsumption: nothing |
| `Interp` | lengths summed first, one string allocated, parts copied; each `Display` part writes into the builder through its resolved callee |
| `Default` | a call of the default's getter (§12.3) |
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

- **Facts and defaults (mine).** A fact or default body whose TIR is a
  tree of constants and constructors is evaluated at emission into a
  constant global. Any other becomes a getter that computes the value on
  first use and stores it in a global. Facts are requirement-free, so the
  getter needs no providers.
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

- **A concrete row** (`$ Console + Clock`) adds one parameter per key,
  in the order of the keys' stable paths. Each is a trait value of the
  key's trait (§15.2). A call passes the providers TIR names for it, so a
  provider costs one Wasm argument per key and no allocation.
- **A row parameter** (`$R`) adds one **context** parameter (mine). A
  context is an immutable linked list of `(key id, provider)` nodes. Key
  ids are numbered at link time in stable-path order. Looking up a key
  walks the list and takes the first match, which is how an inner
  `$.with` shadows an outer one.
- **Extension** (`$.with(Logger=...)` around a call that needs `R +
  Logger`) pushes one node: one allocation per call into row-polymorphic
  code.
- **A function value with a row** takes a context, because its caller may
  not know its row's keys. TIR's `ContextFor` builds it once per calling
  body.
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
| `multi` layouts: `Option`, `Result`, tuples and trait values as several Wasm values (§15.1) | yes | yes | first release |
| capture-free closures as constants | yes | yes | first release |
| constant folding and dead branches during the walk | yes | yes | first release |
| trivial inlining: the walk descends into a callee of at most 8 instructions with no loop, no suspension point and no closure | yes | yes | first release (mine) |
| bounded inlining: callees up to a size budget, and closures passed to a known callee, such as iterator adapters | yes | yes | open question 1 |
| scalar replacement: a non-escaping closure, cell or small data value after inlining becomes locals; an escape analysis over the instance's walk, recorded in the emission state | yes | yes | open question 1 |
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
   instance's types;
3. every suspension point gets a resume case and a cancel case (§14.2);
4. every relocation names a symbol in the program's instance, type, import
   or global sets (§13.10).

A failure is an internal error naming the instance, with exit status 101,
as a task panic is (§6.4). The linked module is then validated by
`wasmparser` (§15.7).

## 13. Monomorphization And Merging

Owner's answer 8: code per concrete type, then merge byte-identical
functions, the same in debug and release. Dictionaries exist only for
trait values and GADT evidence.

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

A worklist walk, as rustc's collector does:

1. Push the roots with no type arguments.
2. Pop an instance. Map its module's `tir` entry. Scan the body's `tags`
   column for calls, closures, coercions and host calls, substituting the
   instance's type arguments into each one's types.
3. For each `Item` callee, push the callee with the substituted
   arguments.
4. For each `TraitMethod` callee, solve the bound at the concrete self
   type with D1's solver (§4.12.2), pick the impl, and push the impl
   method with the impl's type arguments. Coherence guarantees one answer.
   The solver's memo is shared across programs of a run.
5. For each coercion to a trait value, and each evidence choice of a
   `NewVariant`, record the vtable `(type, trait)` and push every method
   of the trait at that type, supertraits included.
6. For each `Closure`, push the closure body with the parent's
   arguments.
7. Record every `CallHost` as an import, and every type whose layout
   an operation needs.
8. Repeat until the worklist is empty.

The order of the walk never reaches output: the result is sorted by
instance key before anything is emitted (§6.5). Collection is serial per
program and takes milliseconds; many programs (a test plan) collect in
parallel.

### 13.3 Instance Keys

```text
canon(T)      = structural encoding of a type over stable paths, with no IDs
                (Prim | Adt(path, args) | Tuple(elems) | Optional(T) | Fn(params, result, row keys, suspends) | ...)

instance_key  = H("inst", item stable path, sub-body index (closures), tir_hash(item),
                  [canon(arg) for each type argument])
```

The instance key names the instance. The code entry key (§13.8) adds what
the emitted bytes depend on.

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
- This is a build error, reported by `hd build`, `hd run` and `hd test`.
  `hd check` does not report it, since it does not collect. Open
  question 2 asks whether that split is acceptable.

### 13.5 Dictionaries: Trait Values And GADT Evidence

- **Vtables.** A vtable is an immutable struct of typed function
  references, one per trait method, at one concrete type, plus references
  to supertrait vtables and a type id for `Any` and `Debug` of erased
  values. Each is a global with a constant initializer, built once per
  `(type, trait)` pair, so a coercion to a trait value never allocates a
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
- **Everything else is static.** A call through a bound on a type
  parameter is a direct call in every instance.

### 13.6 Tuples, Arity And `all!`

There are no variadic generics (§4.13.7). Tuples are ordinary types:

- `Args < Tuple` instantiates like any type parameter, so `f(args...)`
  is one instance per tuple type.
- Compiler-derived tuple `Eq`, `Ord`, `Hash` and `Debug` are generated
  bodies per arity, instantiated on demand.
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

**The algorithm (mine, after safe ICF in linkers).**

1. Start with classes keyed by `H(body bytes with type, global and data
   relocations resolved to canonical targets, and function relocations
   left symbolic)`.
2. Refine: two members stay in a class only if their function relocations
   point to the same classes. Repeat until no class splits. This handles
   mutual recursion and needs at most a few rounds.
3. The representative of a class is the member with the smallest instance
   key. The others become aliases.
4. The fold list (representative, then aliases in key order) goes into
   the `hd.folds` custom section, so a backtrace can say "also
   `List[Point].push`" (§15.5).

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

```text
code_key = H("code", toolchain_key, tier, instance_key,
             sorted [(stable path, per-item interface hash)] of every item its TIR names,
             sorted [(stable path, tir_hash)] of every item inlined into it)
```

- The interface hashes carry layouts: a field added to a `data` type in
  another module changes that type's item hash, so every instance that
  touches the type re-emits. Function bodies of callees do not affect a
  caller's bytes, because calls are relocations.
- An edit to one private function re-emits its own instances, plus the
  instances that inlined it. Every other code entry is shared by `main`,
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
   in instance-key order. Content order keeps indices stable across
   unrelated edits, which helps wasmtime's per-function cache.
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
9. **Custom sections**: `name`, `hd.runtime`, `hd.sites`, `hd.lines`,
   `hd.folds` (§15.5, §16.4).

```text
link_key = prog_key (§11.3), and on a prog_key miss
           H("link", toolchain_key, tier, profile, root description, sorted code keys)
```

A program is written under both keys, so the next warm run hits the
cheap one. Link is serial per program and linear in output size.
