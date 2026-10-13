# Calling Known Closures Directly (#53)

Step 1 of #53: a plan, no code. It covers where closures are made and
called today, what is known at each call after monomorphization, five
options with prices, a recommended order of jobs, and owner questions.
Inputs: the #52 profile (`audit/compiler/profile-wasmtime-closures.md`),
M37 in [wasmtime-perf.md](wasmtime-perf.md#closures-on-wasmtime-vs-v8),
and the closure specialization schedule that
[codegen.md §12.6](codegen.md) and
[lowering-catalog.md](lowering-catalog.md#optimization-passes) already
describe. Owner constraints: the public API stays `mut Iterator[T]`
(no concrete adapter types, no opaque returns), and adapter-chain fusion
(#65) waits until #53 lands and is re-measured.

## 1. Today

### 1.1 Where closures are made and called

| Stage | Make | Call |
| --- | --- | --- |
| TIR (`hd_tir/src/ir.rs`) | `Closure` (sub id, capture range; a generic local function's uses add type arguments); `ItemRef` (a function reference) | `CallValue` (callee value, argument values). A trait-value call is `CallDyn` |
| `hd_mono` collect (`lib.rs`, `Cx::scan`) | each `Closure` pushes the instance `(item, Sub::Body(k), args)`; each `ItemRef` pushes `(def, Sub::Adapter, args)`; both record `Target::Closure(key)` | `CallValue` records nothing: the callee is a run-time value |
| `hd_wasm` emit (`emit.rs`, `closure_value`, `Tag::CallValue`) | `ref.func $code`, the captured values, `struct.new $Env` (a final subtype of the signature's open base `$Fn`). Every construction allocates, capture-free ones included | `struct.get $Fn 0` (the code), `call_ref $Code`. The callee starts with `ref.cast $Env` and loads each capture |
| std (`lib/std/iter.hd`) | `from_fn` stores its argument in the private `Iterator.step`; each adapter passes a new closure that captures `self` and the user's closure. `list.iter()` is a built-in `ListStep` helper closure (`emit.rs` near line 3448) | `next` is `(self.step)()`; adapters call the user's closure; `fold` calls `step` in its loop |

The pass manager (`hd_mono/src/passes.rs`) declares `inline_plan`,
`closure_spec`, `devirt_known`, `escape` and `scalar_replace`, but each
body returns "not implemented". The driver always hashes the `DEV`
pipeline (`hd_driver/src/lib.rs` near line 1122). `--release` changes
the profile and the Cranelift level, not the pipeline. No hd pass runs
in either build.

### 1.2 Where the wasmtime time goes

Wasmtime 49.0.2 stores a funcref inside a GC object as a 32-bit id in a
per-store table. Reading one back (`struct.get` of the `$code` field)
calls the `get_interned_func_ref` libcall
(`wasmtime-internal-cranelift-49.0.2/src/func_environ/gc.rs` near line 371).
For a field of a concrete function type, the libcall also looks up the
engine type and runs a full `is_subtype` check
(`wasmtime-49.0.2/src/runtime/vm/libcalls.rs` near line 573). That is the
59% M37 measured: `is_subtype` 24.6%, `get_interned_func_ref` 22%, the
libcall wrapper 7.1%, `engine_type_index` 5%. The `call_ref` itself and
the callee's `ref.cast` to a final type are inline and cheap. A field of
abstract type `funcref` skips the type lookup and the subtype check but
still calls the libcall. Storing a funcref (`struct.new` with `ref.func`)
calls `intern_func_ref_for_gc_heap`: once per construction, not per call.
The #52 report says `call_indirect`; the emitter uses `call_ref`, and the
cost is in the field read.

### 1.3 What is known at each call site after monomorphization

| Where the callee comes from | Example | Known today? | Known with |
| --- | --- | --- | --- |
| a literal called in place | `(fn(x): x + 1)(2)` | yes, same body | a local def-use check (option A1) |
| a local bound once to a literal | `let f = fn(x): x + 1` then `f(2)` | yes, if the local is never reassigned (`local_flags` has no `ASSIGNED` past the binding) | A1 |
| a function reference | `xs.map(double)` | yes: the adapter instance's key | A1 at the creating body, A2 in the callee |
| a parameter of a higher-order function | `transform` in `map`, `step` in `fold` | no: one instance serves every literal, because closure types are structural | A2: one instance per (callee, literal) |
| a capture of a parameter | `keep` inside `filter`'s step closure | no | A2, carried into the sub-body instance |
| a data field | `Iterator.step` read by `next` | no: every `Iterator[T]` shares the field | inlining, then field forwarding (option C) |
| a captured upstream iterator | `self.next()` inside `map`'s step closure | no | C |
| a global or a value from the host or a list | `let handlers = [...]` | no | stays indirect (option E makes it cheaper) |

`Iterator.step` is private and written only by `from_fn`'s `NewData`.
No `FieldSet` touches it, so once a body sees the `NewData`, every read
of `step` from that value is the stored closure, even if the value
escapes. Closure environment fields with `Copy` and `Move` captures are
also never written. `Shared` captures are cells and stay loads.

## 2. Cost Model

**The bench.** `total(20000000)` fills a list, then runs
`items.iter().map(f).filter(g).fold(0, h)`. About half the items pass
`g`, so p ≈ 0.5. Per source item:

| Call | Count per item | Class |
| --- | --- | --- |
| `ListStep` helper, from `map`'s step | 1 | data field / capture |
| `map`'s step, from `filter`'s step | 1 | data field / capture |
| `filter`'s step, from `fold` | p | data field |
| `transform` (f), `keep` (g) | 2 | literal argument |
| fold's `step` (h) | p | literal argument |
| direct calls to `Iterator.next` | 2 + p | already direct |

That is 4 + 2p ≈ 5 closure calls per item: 2.5 whose target a literal
argument decides, and 2.5 that read a data field or a capture.

**Fitted unit costs (wasmtime, dev ≈ release on this bench).** From #52:
2047 ms total, 127 ms fill, so 1920 ms over 20M items is 96 ns per item.
The same work as a plain `for` loop costs 438 ms (22 ns per item). The
remaining 1480 ms (74 ns per item) is the closure machinery, so
c_ref ≈ 15 ns per closure call, its share of `Option` traffic included.
From M37, 59% of a closure call is the funcref libcall (≈ 9 ns); the
typed half (type lookup plus `is_subtype`) is ≈ 4.5 ns. A direct call
costs c_dir ≈ 1 to 2 ns (M37: direct calls run at 0.55x V8). V8 runs the
whole bench in 386 ms, 177 ms past the fill. All estimates carry about
±30%: the shares come from a micro profile.

## 3. Options

| Option | What changes | Bench, wasmtime (from 2047 ms) | V8 | Size and compile cost | Pipelines |
| --- | --- | --- | --- | --- | --- |
| A1: local known closure | `CallValue` whose callee resolves in the same body to a `Closure` or `ItemRef` becomes a direct `call` of the code instance, the closure value passed as the environment | 0 (no such call in the bench) | 0 | smaller: `call` replaces `struct.get` plus `call_ref`; no new instances | optimized |
| A2: specialize per literal | a call passing a known closure for an unassigned parameter pushes the callee at `(item, sub, args, {param: closure key})`; inside it, calls of that parameter, and of captures of it in sub-bodies, are direct | −2.5 × (c_ref − c_dir) ≈ −34 ns per item: ≈ 1.35 s (3.5x V8) | ±3% (V8 already inlines monomorphic `call_ref`) | one copy of the callee and its sub-bodies per literal: about 150 to 300 bytes for a std adapter; the plain instance drops when no caller needs it. Bench: +0 to +600 bytes on 9.5 KB. Hello: 0 | optimized (owner Q1) |
| B: hd inlining alone | the declared `inline_plan`: direct callees within budget inlined through an instance view | the 2.5 direct `next` calls: ≈ −75 ms | ≈ 0 | per the existing budgets (codegen.md §12.8); needs inline call sites in `hd.lines` for panic frames | optimized |
| C: the specialization schedule | inline, forward the never-written `step` and environment fields, call the known body, inline it; at most 4 rounds (codegen.md §12.6) | all 5 calls, the `next` calls and the `Option` traffic: ≈ 0.57 s (1.5x V8; the rest is #52 cause 3, bounds and GC access shape) | −0 to −5% | +50 to 150 bytes per stage per chain site; emission +10 to 20% on callers with chains; the largest job | optimized; this is #65's ground |
| D: wasmtime's inliner | `Config::compiler_inlining(Inlining::InterModuleAndIntraGc)` at `Speed`. hd modules use GC, so intra-module calls qualify; it inlines direct `call`s only (callee ≤ 50 size units, caller plus callee ≤ 2000) | alone ≈ −75 ms; after A2 it also inlines the known closure bodies: ≈ −75 ms more | n/a | Cranelift time up, unmeasured; backtraces omit inlined frames, so wasmtime's panic frame lists differ from V8's in release; the compile cache keys only the opt level today (`cache.rs`) | release engine setting |
| E1: abstract code field | the closure base's `$code` field becomes `funcref`; calls add `ref.cast (ref $Code)` (final type: inline) | −5 × (4.5 − 1) ns ≈ −350 ms: ≈ 1.7 s; after A2 ≈ −175 ms | one cast per call, likely ≈ 0 (measure) | +3 bytes per closure call site | both (a layout; agent-originated, owner Q3) |
| E2: code index plus table | the code is an `i32` index into one funcref table; calls use `call_indirect` (inline signature check on final types, no libcall); construction needs no interning | −5 × (9 − 1) ns ≈ −800 ms: ≈ 1.25 s; after A2 ≈ −400 ms: ≈ 0.95 s | ≈ 0 | a table and an element segment only when the program makes a closure (hello: 0); `ListStep`, `MapStep`, adapters and suspension poll and cancel functions move too; 3 to 4 jobs | both (a layout; agent-originated, owner Q3) |

Also rejected:

- **Specializing on field contents** (k-CFA style): key `fold` on "an
  `Iterator` whose `step` is closure K, whose capture is an `Iterator`
  whose `step` is L". It reaches C's result without inlining, but keys
  become trees, every chain shape gets its own copy of every adapter,
  and a function's code depends on its callers' data. It costs more than
  C and gains nothing over it.
- **Concrete adapter types or opaque returns:** owner-rejected API.
- **Capture-free closures as constant globals** (codegen.md table, not
  built yet) saves one allocation and one interning per construction.
  The bench builds about 5 closures in total, so no gain here; it stays
  a separate item.

### 3.1 Option A2 in detail

- **Detection, in the caller (`Cx::scan`).** An argument is a known
  closure if its value is a `Closure` (not a generic local function's
  defining one) or an `ItemRef` in the same body, directly or through a
  local bound once and never reassigned. This is A1's check.
- **The key.** Today an instance is `(item, sub, args)`
  (`instance_key`). A2 adds a sorted list of `(param index, closure
  key)`. The closure key is an identity (item path, sub index, type
  arguments), not a body hash. So an edit to a literal's body re-emits
  only the literal's own instance, plus the copies whose key moved
  because sub indices shifted.
- **Inside the copy.** `CallValue` on a known parameter records
  `Target::KnownClosure(key)`. Each `Closure` in the copy that captures
  that parameter pushes its sub-body instance with the matching capture
  slot known, so `map`'s and `filter`'s step closures call `transform`
  and `keep` directly. The emitter emits `call $code` with the closure
  value as the environment argument and skips the field read and
  `call_ref`. The code ABI is unchanged, so suspending closures and
  context rows need nothing new.
- **Bounds.** Specialize only when the callee's body plus its sub-bodies
  is at most 64 TIR instructions (owner Q2). Stop at depth 2: a literal
  made inside a copy may make one more copy, then plain instances
  follow. The depth bound ends recursion that wraps its own parameter
  (`fn f(g): f(fn(x): g(x) + 1)`). Past a bound the call uses the plain
  instance, never an error. A recursive call that passes the parameter
  through unchanged has the same key, so it hits the memo.
- **Cost per program.** Copies grow linearly with the closure literals
  passed to small higher-order functions. Erased bodies, thunks and
  adapters are never specialized.
- **REPL.** Copies are ordinary content-keyed instances. A literal in
  input n makes its copies in input n's module, and earlier inputs and
  the prelude keep the plain adapters for every other call (Session
  `known` works unchanged). R1 measured about 70 KiB of code mapping per
  input module whatever its size (16 KiB pages), so a few hundred extra
  bytes rarely cost a page. A literal bound by a top-level `let` is a
  global and stays indirect in the first cut.
- **Caching.** A copy's code key covers the callee's TIR, the literal's
  key and the layouts, as for any instance. An edit to a higher-order
  function re-emits each of its copies. That fan-out is the number of
  literals passed to it, smaller than inlining's.

## 4. Recommendation

Do A1, then A2, in the optimized pipeline. They take the 2.5 literal
calls per item off the funcref path for about 3 hours of work, with
bounded code growth, no layout change and no new machinery beyond a key
component. Then measure D on top: once A2 makes the user's closures
direct calls, it is free inlining. Run E1 as a prototype only if the
owner approves a layout change. It is the only option that also helps
the dev pipeline and closures that escape. Hold E2 until a newer
wasmtime has been checked for a fast path. Leave C to #65's decision,
re-measured after A2. A2 does not block C: the schedule's inlining makes
A2's copies unneeded where it applies, and A2 still covers callees over
the inlining budget.

Expected after A2 plus D: the closures bench on wasmtime from about
1.9 to 2.05 s to about 1.2 to 1.35 s, 3x to 3.5x V8, against 5x today.
With E1 too, about 1.05 s. Getting near 1.5x needs C.

## 5. Step Plan

Each job is about an hour, one at a time. Common done checks for every
job: conformance unchanged on both engines (`HD_CONFORMANCE_ENGINE`
Compare mode, 0 differences); `cargo fmt`, `clippy` and `test` green;
the hello program's Wasm bytes, function count and build time unchanged
(the footprint guard); the closures bench timed before and after, dev
and release, on wasmtime (`hd --engine wasmtime`, with `--release` for
the `Speed` pipeline) and V8 (`node host/run.mjs`), p50 of 5 runs after
1 warmup, with the machine load noted.

| # | Job | Done check |
| --- | --- | --- |
| 1 | Wire the optimized pipeline: `--release` (and `hd test --release`) hashes `passes::OPTIMIZED` into code keys; unimplemented passes report `Skipped`, not an error; collection and emission can read whether the pipeline is optimized. Add a conformance switch that runs runtime cases through the optimized pipeline | release Wasm byte-identical to today's release build; dev and release code keys differ; the new switch passes every runtime case |
| 2 | A1: the local known-closure check in `Cx::scan` (`Closure` or `ItemRef`, directly or through a once-bound local), `Target::KnownClosure`, and the direct `call` in the emitter | an `hd_wasm` test: `let f = fn(x): x + 1; f(2)` has `call` and no `call_ref` in the optimized pipeline, `call_ref` in dev; fixtures with a suspending closure and a function reference pass in both pipelines |
| 3 | A2, part 1: the specialization key component, copies for known parameters called in the callee's own body (`fold`), the size bound, and the memo for pass-through recursion | the bench's fold closure is a direct call in release; wasmtime release improves (expect about −10 to −15%); V8 within 3%; bench Wasm size delta reported |
| 4 | A2, part 2: carry known parameters into captured sub-bodies (`map`, `filter`), the depth bound with fallback, and probes: wrapping recursion, a closure escaping a copy, a function reference passed to `map` | wasmtime release closures bench at most 1.5 s (from about 1.9 s); probes compile, end and print the same on both engines; size delta reported |
| 5 | A2 in the REPL and the cache: a literal in input 2 and another in input 3 passed to std adapters; rebuild from the journal; edit-and-rebuild of a literal body | session tests pass; copies appear only in their own input's module; a literal-body edit re-emits only that literal's instance (cache stats); per-input module bytes reported |
| 6 | D: measure wasmtime's inliner at `Speed` on the bench, the runtime suite and the conformance suite's Cranelift time; count panic frame-list differences | ship only if it buys at least 5% (tiering.md T1 rule) and owner Q4 allows it; if shipped, the compile cache key includes the setting |
| 7 | (If owner Q3 approves E1) the abstract `$code` field plus `ref.cast`, dev and release | ship if wasmtime dev closures improves at least 10% and V8 loses at most 3% on the runtime suite; hello bytes at most +0.5% |
| 8 | Re-measure and close #53: final numbers into this file, then the #65 decision on C | the table in section 3 filled with measured values |

## 6. Owner Questions

1. **Does A2 run in the dev pipeline too?** The 2026-10-07 decision puts
   specialization and devirtualization in the optimized pipeline only,
   so `hd run`, `hd test` and the REPL would keep paying about 15 ns per
   closure call. A2 also puts specialization in collection (new
   instances), where tiering.md §6.6 had `closure_spec` extend an inline
   set. **Recommendation:** optimized only, as decided; job 3 reports
   the dev cost (instances, emission and Cranelift time over the suite)
   so the question can be reopened with numbers.
2. **How much code growth is acceptable for specialization?**
   **Recommendation:** the E8 rule already in codegen.md §12.8 (a budget
   must buy at least 5% speed per 10% size), a cap of 64 TIR
   instructions per copied callee, depth 2, and zero growth for programs
   that pass no literal (hello).
3. **May the closure layout change (E1, later perhaps E2)?** Layouts are
   shared by both pipelines, so this changes dev and V8 output too. It
   is an agent-originated design, not in the approved records.
   **Recommendation:** approve E1 as a measured prototype (job 7); defer
   E2 until a newer wasmtime is checked for a fast path on funcrefs read
   from GC objects.
4. **May wasmtime's inliner change release panic frame lists?**
   Tiering.md §4.1 lets frame lists differ by pipeline, but with D they
   would also differ between wasmtime and V8 in release.
   **Recommendation:** allow it in release only if job 6 shows at least
   5%; otherwise leave inlining off.
5. **Should the wasmtime pin move?** Checking a newer release for a
   cheaper GC funcref read is a cargo fetch plus a source read; moving
   the pin touches the whole engine. **Recommendation:** one read-only
   check before any E2 work, no pin change inside #53.
