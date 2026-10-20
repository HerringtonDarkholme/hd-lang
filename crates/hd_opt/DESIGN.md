# Optimization Pipelines By Target And Level (#162)

Status: describes the code at 3209d690; the code and the spec win.

Optimizations run as passes in a pipeline chosen by (target, level), not
as branches inside collection. `hd_opt` holds the passes; `hd_mono`
collects instances and applies plans by generic rules. Lowering choices
that depend on the engine live in a per-target profile in `hd_wasm`.
Every build knows its target engine.

## 1. Crates And Boundaries

### 1.1 Who owns what

| Crate | Owns | Never |
| --- | --- | --- |
| `hd_mono` | Collection: the worklist, instance keys (`known_key` included), selection, vtables, witnesses, `Collected`. `hd_mono::plan` holds what collection reads: `BodyPlan`, `Source` and the `Planner` trait (§2.2). Applying a plan is generic | Deciding *whether* to specialize or devirtualize; pass names; size or depth policy beyond its own instantiation limits |
| `hd_opt` | The pass manager (`passes.rs`: `PassSpec`, `FactId`, gates, `DEV`, `OPTIMIZED`, `pipeline`, `validate`), the body passes, `ValueFlow` (`value_flow.rs`), and `PipelinePlanner` (`planner.rs`), which implements `Planner` | Creating instances, emitting Wasm, choosing layouts |
| `hd_wasm` | Emission and link. `Lowering::of(target)`, the per-target profile (§4). It reads `Collected` targets (`KnownClosure` and the rest) | Reading pass names or the pipeline |
| `hd_run_wasmtime` | Engine options per level: `EngineConfig::pipeline(level)` gives Cranelift `None` or `Speed` | Code keys |
| `hd_driver` | Wiring: it picks (target, level), builds the planner, combines the pass hash and the lowering hash into the `pipeline` key part (`hd_driver::pipeline`), and passes the planner to collection | Policy |
| `hd_base` | `EngineTarget { V8, Wasmtime, Browser }` and `OptLevel { Dev, Optimized }`, next to `BuildProfile` | Tables |

`EngineTarget` is not the output format (`wasm32-gc`), which stays in the
toolchain key. Every target emits standard Wasm GC that every engine runs;
a target only tunes speed.

### 1.2 Dependency direction

```text
hd_base <- hd_tir <- hd_mono (collection + plan types)
                        ^   ^
                        |   +-- hd_wasm (emission, Lowering)
                        +------ hd_opt  (passes, pipelines; implements Planner)
hd_driver -> hd_opt, hd_wasm, hd_mono
```

`hd_opt` depends on `hd_mono`, not the reverse: collection defines the
interface it consumes, and the driver hands it a `&dyn Planner`. `hd_wasm`
and `hd_opt` never depend on each other. Module-level (link) passes work
on `hd_wasm`'s module form, so their bodies stay in `hd_wasm` as link
options, and the pipeline table only selects them.

Implementation differs: `hd_wasm::link` takes a `release` bool from the
build profile (`hd_driver` passes `build_profile == Release`), and the
pipeline's link pass names are only hashed; not judged, since
crates/hd_mono/DESIGN.md §13.10 ties the `hd.names` section to the profile.

### 1.3 How instances flow

```text
TIR bodies ──► body-unit passes (hd_opt, per generic body)
                 │ BodyPlan, memoized in collection by (def, sub-body type)
                 ▼
collection (hd_mono): resolves each BodyPlan entry against the instance
                 being scanned → Target::KnownClosure, specialized
                 instances, carried known lists
                 ▼
emission (hd_wasm) reads Collected + Lowering
                 ▼
link (hd_wasm)
```

The **body unit** exists for decisions that change the instance set: such
a decision must be known while collection runs, and collection is the only
place instances are made. `ValueFlow` reads only the body's own TIR, so it
runs once per body, not once per instance.

Planned, not built: instance-unit passes. `use_counts`, `inline_plan`,
`escape` and `scalar_replace` are declared and report `Skipped`;
`liveness` is declared, and suspension lowering does that work. No build
runs an instance pass.

## 2. The Pass Interface

### 2.1 Units and declarations

`PassSpec` has `name`, `version`, `level` (`TirAnalysis`, `TirDecision`,
`WasmModule`), `reads` and `writes` (facts), `gate` (summary bits),
`required_by`, `run` and `params`. `run` is `Run::Body(fn(&BodyCx,
&mut BodyFacts, &mut Fuel))` for a pass per generic body, before and
during collection, or `Run::Instance(..)` for a pass per instance at
emission. The unit is the `Run` variant rather than a separate field, so
a pass's unit and its function signature cannot disagree. `BodyCx` holds the body, its `DefId`, `&dyn ProgramEnv` and the
pass's knob values. `params` declares each knob with its inclusive range;
the pipeline gives the values, and both are hashed (§3.3).

| Fact | Written by | Read by | Content |
| --- | --- | --- | --- |
| `ValueFlow` | `value_flow` | the two below | where each value is made, which parameters each sub-body calls |
| `KnownCallees` | `devirt_known` (A1) | collection | per `CallValue` whose callee is made in its own sub-body: `Source::Made(inst)` |
| `ClosureSpec` | `closure_spec` (A2) | collection | per `Call` of an item: each argument's source; per `Closure`: the parameters its sub-body calls; per `CallValue` of a parameter: `Source::Param(p)`; as a callee: `spec_params`, the parameters it calls, or none when the body is over `max_insts`, an intrinsic, suspends, or calls no parameter |

### 2.2 What collection consumes

`hd_mono::plan` defines `Source { Made(u32), Param(u32) }` (an
instruction, or an item parameter), `BodyPlan { direct, spec_args, carry,
spec_params }`, and `Planner { plan(def, body, env) -> Option<Arc<BodyPlan>>,
spec_depth() }`. `PipelinePlanner::plan` runs each body pass whose gate the
body passes and merges `KnownCallees` and the `Param` calls into `direct`.
A pass out of fuel leaves the body unplanned, which is plain code.

Collection applies a plan with rules that name no pass:

1. **Direct calls.** At a `CallValue` with `Made(m)`: if `calls[m]` is
   `Target::Closure(k)`, record `Target::KnownClosure(k)`. With
   `Param(p)` in the instance's own sub-body: if its known list has `p`,
   record that key (`known_closures`).
2. **Specialization.** At a `Call` whose callee's plan has `spec_params`:
   for each marked parameter, resolve the caller's `Source` (a `Made`
   through `calls`, a `Param` through the known list). A `Param` outside
   the instance's own sub-body skips the call. Collection checks that
   each value's type equals the parameter's (soundness, not policy),
   applies `spec_depth`, and pushes the specialized instance
   (`copy_target`).
3. **Carrying.** At a `Closure` of a specialized instance: carry the known
   entries whose parameter the plan lists for it, inside the own sub-body
   only.
4. **Never in a thunk.** A thunk emits one instruction and takes no plan.

The dev pipeline has no body pass, so its planner returns `None` and
collection does no plan work: `Counters::body_passes` is 0 in dev (a test
in `known_closure_calls.rs`). A new body pass that decides the same kinds
of things reuses these rules; a new kind becomes a new generic rule in
`hd_mono::plan`.

Planned, not built: one counter where rules 1 to 3 apply, giving
docs/compiler/tiering.md §4.3's `HD_OPT_BISECT` for every body-unit
decision.

### 2.3 Caching and incremental cost

- **The rule (docs/compiler/tiering.md §6.4).** A pass's output must
  follow from what the code key covers, or its effect must be hashed into
  `callee_reps`. A body plan reads the body's TIR (covered by the TIR
  hash) and the pass knobs (covered by the pipeline hash). The one
  cross-body read is the callee's `spec_params` at the caller; its effect
  is the copy's instance key (`known_key`), which `hash_target` hashes
  into the caller's `callee_reps`. A `KnownClosure` follows from the
  instance's own TIR, arguments and known list, all in its code key.
- **No cache entry.** Plans are memoized per run, not stored. The passes
  are linear in instructions and locals, an estimated 1 to 5 ms for a
  10k-line application, against about 640 ms of Cranelift. A disk entry is
  not worth it until a body pass costs more than about 10% of collection.
- **Per edit.** An edited body re-plans in microseconds. Callers re-emit
  only if a copy decision changed, because the copy's key moved.
- **Versions.** A pass `version` bump or a knob change misses only the
  pipelines that hold that pass.

## 3. Pipelines Keyed By (Target, Level)

### 3.1 The table

`hd_opt::passes::pipeline(target, level)` is a total `match`. Every target
has the same two rows; each column is data in the crate that reads it.

| Level | Passes (`tir`) | Link passes (hashed only) | Knobs | Engine |
| --- | --- | --- | --- | --- |
| `DEV` | `liveness` | `fold` | none | V8 tiers by itself; Cranelift `None` |
| `OPTIMIZED` | `value_flow`, `use_counts`, `inline_plan`, `devirt_known`, `closure_spec`, `escape`, `scalar_replace`, `liveness` | `drop_dead`, `fold`, `leb_compact`, `compact_names` | `closure_spec`: `max_insts` 64, `max_depth` 2 (§8) | Cranelift `Speed` |

The lowering profile differs by target (§4). Dev runs no A1 or A2 on any
target: specialization and devirtualization run in the optimized pipeline
only (docs/compiler/tiering.md §3.1 and §7.1). A target whose measured best
values differ would get its own `Pipeline` static.

### 3.2 Validation

The table is validated once per process (`hd_driver::pipeline`) and in
unit tests:

1. The table is total: the `match` has no wildcard, so a new target or
   level does not compile without a row.
2. `validate`: each fact is read only after a pass writes it, every
   non-optional pass is present, a body pass reads only body facts, and
   only body passes write them.
3. `check_knobs`: every knob belongs to a pass present, lies within its
   declared range, and every declared knob has exactly one value.
4. `check_distinct`: two rows have equal keys exactly when their passes,
   knobs and lowering are equal.

### 3.3 Hashing

`pipeline = H(pass hash, lowering hash)` (`hd_driver::pipeline::content_key`).
The pass hash covers each pass's name, version and knob values in order,
then the link pass names. The lowering hash covers each lowering field by
name and value. The **target name is not hashed, only content**, so
targets with equal content share cache entries. Engine options stay out of
code keys: the wasmtime cwasm cache keys its own opt level. The value goes
into `prog_key`, `code_key` and `prelude_key`.

Cost of the coarse key: switching targets rebuilds every instance once,
an estimated 0.55 CPU-s for a 10k-line application in dev. A finer key
would add the lowering hash only to instances whose code reads a lowering
choice, which needs read tracking in emission; it is worth it only if
users switch targets often or there are more than about three lowering
choices.

## 4. Target Profiles For Lowering

`hd_wasm::Lowering` is a `Copy` struct with one field per choice.
`Lowering::of(EngineTarget)` returns it, and `Layouts::new(lowering)`
carries it to every `Lay`. It takes no level, since no lowering choice
depends on one, which keeps the table to one axis. So `closure_base(closure, code)`, the closure
call in `emit.rs`, and the `List.iter()` and `Map.iter()` cursors branch on
one field. `ProgramEnv` has none.

| Choice | V8 | Wasmtime | Browser | Evidence |
| --- | --- | --- | --- | --- |
| `closure_code` (E1) | `Typed` | `Abstract` | `Typed` | wasmtime: abstract is -21.5% dev and -31.3% release on closures; V8: abstract is +2.8% to +6.3% (known closure calls §7) |

`Browser` follows `V8` until another browser engine is measured (§7).
Capture-free closures as constant globals help every engine, so they are
plain lowering in `hd_wasm`, in no profile. A choice goes into the profile
only if it is engine-specific and measured on at least two engines.

**Artifacts run on more than one engine.** `hd build` writes a `.wasm`
that `hd FILE.wasm` may run on any engine. Every profile emits valid Wasm
GC for every engine, so a mismatch costs speed only: a wasmtime run of a
typed build is about 21% to 31% slower on closure-heavy code, a V8 run of
an abstract build 3% to 6%. A cross-run test (`closure_code_targets.rs`)
runs each target's build of a closure program on Node and, with the
`wasmtime` feature, on wasmtime, with the same output.

## 5. How The Target Reaches Each Build Path

`Packages` and `Run` carry `target: EngineTarget` and `level: OptLevel`;
`build_profile` stays separate (docs/compiler/tiering.md §5.3: profile and
pipeline are different things). `Output` reports the target and level a
build used.

| Path | Target | Level |
| --- | --- | --- |
| `hd run`, `hd FILE`, standard input, `hd test`, `hd build` | the run's engine (`--engine`, else `HD_ENGINE`, else Node): Node builds for `V8`, wasmtime for `Wasmtime`. `hd build --engine wasmtime` without the feature is refused with `hd run`'s message | `--release` |
| `hd FILE.wasm` | none: runs a prebuilt module | none |
| REPL | `Wasmtime` (the REPL is wasmtime-only) | dev |
| playground, web compiler (`hd_web`) | `Browser` | dev |
| `hd check`, `hd doc` | never emitted | none |
| conformance | per engine: Node builds for `V8`, wasmtime for `Wasmtime`; compare mode builds each runnable program once per target | `HD_CONFORMANCE_PIPELINE` |

`HD_CONFORMANCE_WASM_DIGEST=record|check` writes or checks the hash of
every module a runtime case runs, one file per target and pipeline, so a
refactor can prove its output byte-identical.

## 7. Decisions

1. `hd build` resolves its target like `hd run`'s engine and takes
   `--engine`. A mismatch costs speed, never correctness.
2. `Browser` makes the same choices as `V8` until a measurement on another
   browser engine shows otherwise.
3. Conformance compare mode builds once per engine's target, since a
   single build would miss target-specific lowering; the cross-run test of
   §4 guards validity everywhere.
4. A knob or pass list may differ per target when the E8 rule says so
   (crates/hd_mono/DESIGN.md §12.8); today it does not.

## 8. Knob Measurements (Job e)

Measured on the runtime suite (`bench/runtime/progs`, six programs) with
`hd build --release`; V8 through `node host/run.mjs`, wasmtime with a
cached cwasm. Only `closures` changes with the knobs; the other five emit
byte-identical Wasm in every variant.

| Variant, `closures` | V8 release | Wasmtime release | Closures Wasm (V8 / wasmtime) |
| --- | ---: | ---: | ---: |
| A2 off | 323 ms | 1152 ms | 7774 / 7776 B |
| 32/1 | 331 ms | 1020 ms | 7781 / 7776 B |
| 64/2 | 292 ms | 781 ms | 7786 / 7767 B |
| 128/2 | 289 ms | 781 ms | same Wasm as 64/2 |

The E8 rule (crates/hd_mono/DESIGN.md §12.8) keeps a budget only if it
buys at least 5% speed per 10% size. A2 at 64/2 buys -9.6% on V8 for
+12 B and -32% on wasmtime, so it stays on both. 128/2 emits the same
suite Wasm, so the larger budget buys nothing. 32/1 loses most of A2's
gain. So every target keeps `max_insts` 64 and `max_depth` 2, and the
table keeps one shared `OPTIMIZED` row. A test in `hd_driver::pipeline`
retunes one row and checks that only that row's key moves.

# Calling Known Closures Directly (#53)

Why `devirt_known` (A1), `closure_spec` (A2) and the per-target closure
code field (E1) exist, and the options not taken. The public API stays
`&mut Iterator[T]`: no concrete adapter types, no opaque returns.

## 1. Today

### 1.1 Where closures are made and called

| Stage | Make | Call |
| --- | --- | --- |
| TIR (`hd_tir/src/ir.rs`) | `Closure` (sub id, capture range); `ItemRef` (a function reference) | `CallValue` (callee value, arguments); a trait-value call is `CallDyn` |
| `hd_mono` collection | each `Closure` pushes the instance `(item, Sub::Body(k), args)`; each `ItemRef` pushes `(def, Sub::Adapter, args)`; both record `Target::Closure(key)` | `CallValue` records `Target::KnownClosure(key)` where a plan makes it direct (§2.2 above), else nothing |
| `hd_wasm` emission | `ref.func $code`, the captures, `struct.new $Env`, a subtype of the closure base (`closure_base`); a closure with no captures is `global.get` of a constant | a known closure: `call $code` with the closure as the environment. Otherwise `struct.get` of the code field and `call_ref`, with `ref.cast` first under `Abstract` |
| std (`lib/std/iter.hd`) | `from_fn` stores its argument in the private `Iterator.step`; each adapter passes a new closure capturing `self` and the user's closure | `next` is `(self.step)()`; adapters call the user's closure |

### 1.2 Where the wasmtime time goes

Wasmtime 49.0.2 stores a funcref inside a GC object as a 32-bit id in a
per-store table. Reading one back (`struct.get` of the code field) calls
the `get_interned_func_ref` libcall. For a field of a concrete function
type, the libcall also looks up the engine type and runs a full
`is_subtype` check: about 59% of a closure call in a profile of the
closures bench. The `call_ref` itself and a `ref.cast` to a final type
are inline and cheap. A field of abstract type `funcref` skips the type
lookup and the subtype check but still calls the libcall. That is why
`Wasmtime` uses `Abstract` (E1) and V8, which inlines monomorphic
`call_ref`, uses `Typed`.

### 1.3 What is known at each call site after monomorphization

| Where the callee comes from | Example | Direct with |
| --- | --- | --- |
| a literal, or a local bound once to one, in the same sub-body | `let f = fn(x): x + 1` then `f(2)` | A1 |
| a function reference | `xs.map(double)` | A1 at the creating body, A2 in the callee |
| a parameter of a higher-order function | `transform` in `map`, `step` in `fold` | A2: one instance per (callee, literal), since closure types are structural |
| a capture of a parameter | `keep` inside `filter`'s step closure | A2, carried into the sub-body instance |
| a struct field, a captured upstream iterator | `Iterator.step` read by `next` | inlining, then field forwarding (option C; see #65) |
| a global, a host value, a list element | `let handlers = [...]` | stays indirect |

`Iterator.step` is private and written only by `from_fn`'s `NewData`, so
once a body sees the `NewData`, every read of `step` from that value is
the stored closure. That fact is what option C would use.

## 3. Options

| Option | What it does | Status |
| --- | --- | --- |
| A1: local known closure | a `CallValue` whose callee is made in the same sub-body calls the code instance directly, the closure passed as the environment | built: `devirt_known`, optimized pipeline |
| A2: specialize per literal | a call passing a known closure for a parameter goes to the callee's instance specialized on it; inside, calls of that parameter and of captures of it are direct | built: `closure_spec`, optimized pipeline |
| B: hd inlining | the declared `inline_plan`, within the budgets of crates/hd_mono/DESIGN.md §12.8 | planned, not built |
| C: the specialization schedule | inline, forward the never-written `step` and environment fields, call the known body (crates/hd_mono/DESIGN.md §12.6) | not built yet; see #65 |
| D: wasmtime's inliner | Cranelift inlining at `Speed` | rejected: `InterModuleAndIntraGc` does not apply to hd's Wasm; `Inlining::Yes` gained 8.6% on closures but slowed the rest of the suite 2% to 6%, grew compile time, and dropped every frame from release panic reports |
| E1: abstract code field | one closure base with a `funcref` field; a call casts before `call_ref` | built, `Wasmtime` target only (§7) |
| E2: code index plus table | the code is an `i32` index into one funcref table, called with `call_indirect`, with no libcall | planned, not built; worth checking a newer wasmtime for a fast path first |

Also rejected:

- **Specializing on field contents** (k-CFA style): keys become trees,
  every chain shape gets its own copy of every adapter, and a function's
  code depends on its callers' data. It costs more than C and gains
  nothing over it.
- **Concrete adapter types or opaque returns:** owner-rejected API.

### 3.1 Option A2 in detail

- **Detection.** An argument is a known closure if its value is a
  `Closure` or an `ItemRef` made in the call's sub-body, or a parameter
  passed on unchanged that the instance knows.
- **The key.** An instance is `(item, sub, args)`; a specialized one adds
  a sorted list of `(param index, closure key)` (`known_key`). The closure
  key is an identity (item path, sub index, type arguments), not a body
  hash, so an edit to a literal's body re-emits only the literal's own
  instance, plus copies whose key moved.
- **Inside the copy.** A `CallValue` on a known parameter records
  `Target::KnownClosure(key)`. A `Closure` in the copy that captures the
  parameter pushes its sub-body instance with that capture known. The
  code ABI is unchanged, so suspending closures and context rows need
  nothing new.
- **Bounds.** Only a callee of at most `max_insts` (64) TIR instructions,
  not an intrinsic and not suspending, is specialized. A specialized
  instance requested from inside `max_depth` (2) levels of specialized
  instances uses an existing one or the plain one. The depth bound ends
  recursion that wraps its own parameter (`fn f(g): f(fn(x): g(x) + 1)`).
  Past a bound the call uses the plain instance, never an error. A
  recursive call passing the parameter on unchanged has the same key.
- **Caching.** A copy's code key covers the callee's TIR, the literal's
  key and the layouts. An edit to a higher-order function re-emits each
  of its copies: one per literal passed to it, fewer than inlining's.

## 7. Result of job 7 (E1: abstract code field)

The `Abstract` closure base is one struct for every signature,
`(sub (struct (field (mut funcref))))`; environments and the `ListStep`
and `MapStep` cursors extend it. A call reads the field, then
`ref.cast (ref $Code)` (a final type) before `call_ref`. Known closures
(A1, A2) still call directly. `Typed` keeps one base per signature whose
code field has the concrete function type, with no cast.

Measured on the closures bench, the abstract form against the typed one:

| Engine, profile | Typed | Abstract | Change |
| --- | --- | --- | --- |
| wasmtime dev | 1912 ms | 1502 ms | -21.5% |
| wasmtime release | 1042 ms | 716 ms | -31.3% |
| V8 dev | 347 ms | 361 ms | +4.0% |
| V8 release | 273 ms | 281 ms | +2.8% |

The other runtime programs stayed within noise. Release gains more than
dev on wasmtime because A2 leaves the data-field calls indirect, and each
paid the full subtype check. So only the `Wasmtime` target uses E1
(§4); `V8` and `Browser` keep `Typed`.
