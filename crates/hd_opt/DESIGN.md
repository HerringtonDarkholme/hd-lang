# Optimization Pipelines By Target And Level (#162)

Status: design record; may lag the code. The code and the spec win.

Step 1 of #162: a design note, no code. Owner direction (2026-10-13):
optimizations run as passes in pipelines chosen by (target, level), not
as branches inside collection. A new `hd_opt` crate holds the passes.
`hd_mono` goes back to monomorphization and collection. #53's A1 and A2
become the `devirt_known` and `closure_spec` passes. Lowering choices
that depend on the engine, such as E1's closure layout, live in a
per-target profile that `hd_wasm` reads. Every build knows its target
engine.

Inputs: [docs/compiler/tiering.md](../../docs/compiler/tiering.md) §6 (pass manager, facts, pipelines,
keys), [crates/hd_opt/DESIGN.md (known closure calls)](#calling-known-closures-directly-53) §3.1 and §6 to §7 (A1, A2,
E1 and their measurements), [crates/hd_mono/DESIGN.md](../hd_mono/DESIGN.md) §12 to §13, and
[crates/hd_wasm/DESIGN.md](../hd_wasm/DESIGN.md) §15.2.

## 0. Today

| What | Where | Problem |
| --- | --- | --- |
| Pass manager: `PassSpec` (:52), `FactId` (:33), gate bits (:22), `DEV` (:169), `OPTIMIZED` (:174), `validate` hashing the pipeline (:194) | `crates/hd_mono/src/passes.rs` | Every pass except `liveness` (:73) is a stub that returns `Skipped`. Optimization does not run through it |
| A1 and A2 | `crates/hd_mono/src/lib.rs`: `Flow` (:2283), `copyable` (:1583), `copy_depth` (:1608), `copy_target` (:1631), `push_known` (:956), carrying known closures into sub-bodies (:2096-2115), `known_closures` (:2436, called at :2202), `COPY_MAX_INSTS` (:2276), `COPY_MAX_DEPTH` (:2280) | Optimization policy sits inside collection, behind `env.optimized()` (:1911). A third optimization would add a third branch |
| Specialized instances | `crates/hd_mono/src/layout.rs`: `known_digest` (:743), `known_key` (:761), the `known` column (:778), `get_known` (:803) | Already generic: "an instance specialized on known parameter values". It stays |
| Direct call of a known closure | `crates/hd_wasm/src/emit.rs` :619-628 reads `Target::KnownClosure` | Already generic. It stays |
| E1 closure layout | `crates/hd_wasm/src/layout.rs` `closure_base` (:211), used at layout.rs :868, :1635 and emit.rs :3447, :3757 | Applies to every target. It costs V8 3% to 6% on closure-heavy code (crates/hd_opt/DESIGN.md (known closure calls) §7) |
| Pipeline choice | `hd_driver::Packages::optimized` (`crates/hd_driver/src/lib.rs` :1135), `Run::optimized` (:1013), the `validate` call (:1423), `ProgramEnv::optimized` (impl :7485) | A single bool. The build does not know its engine |
| Engine choice | `crates/hd_cli/src/engine.rs` `from_env` (:264), `run(kind, release, ..)` (:272); `hd_run_wasmtime` `EngineConfig::pipeline(release)` (`engine.rs` :33) | Chosen after compile, used only to run. `hd build` takes no `--engine` |
| Conformance | `HD_CONFORMANCE_PIPELINE` (`crates/hd_driver/tests/conformance.rs` :622), `HD_CONFORMANCE_ENGINE` modes (:587) | Compare mode builds once and runs that build on both engines |

Status: **the plan of #162 is complete.** Jobs (a), (b1), (b2), (c1),
(c2), (d) and (e) of §6 are done, so the
A1, A2, E1, pipeline-choice, engine-choice and conformance rows above
describe the code before them. A1 and A2 are
now the body passes `value_flow`, `devirt_known` and `closure_spec` in
`hd_opt` (`passes.rs`, `planner.rs`); collection applies their plans by
rules 1 to 4 of §2.2 and reads no pipeline flag. The build's
`Counters::body_passes` counts body-pass runs: 0 in dev, which a test in
`known_closure_calls.rs` checks.

After (c1), `hd_base` has `EngineTarget` and `OptLevel`, and
`Packages` has `target` and `level` instead of `optimized`.
`hd_opt::passes::pipeline(target, level)` is the §3.1 table as a total
`match`; every target has the dev row `DEV` and the optimized row
`OPTIMIZED`. `hd_wasm::Lowering::of(target)` has one field,
`closure_code`, abstract for every target; nothing reads it yet, since
job (d) threads it into `Layouts`. `closure_spec`'s bounds are now
pipeline knobs (`PassSpec::params` declares them with their ranges,
`Pipeline::knobs` gives the values): `max_insts` 64 and `max_depth` 2 in
`OPTIMIZED`. `PipelinePlanner` reads them, and gives `max_depth` to
collection as `spec_depth` (0 for a pipeline without `closure_spec`).
Job (e) keeps only the measurement and the choice of values. The
pipeline key is `hd_driver::pipeline::key`: the pass hash (names,
versions and knob values of the passes, then the link passes; no
pipeline or target name) combined with `Lowering::hash`. Validator checks
1 to 4 of §3.2 run once per process and in unit tests.

After (c2), every build path of §5 passes its target and level. `hd
run`, `hd FILE`, standard input, `hd test` (its programs and the
executables it builds) and `hd build` take the run's engine (`--engine`,
else `HD_ENGINE`, else Node; `hd build` now takes `--engine` and refuses
`wasmtime` without the feature with `hd run`'s message) and `--release`
for the level. The REPL builds for `Wasmtime` and `hd_web` for `Browser`,
both dev; `build_repl_prelude` takes the session's target.
`EngineConfig::pipeline` takes `OptLevel`. `Output` reports the target and
level it was built with, and a test per path runs the real command and
checks them. The conformance engine modes build for their engine's target:
Node for `V8`, `wasmtime-only` for `Wasmtime`, and compare mode once per
target. CI runs the compare pass. `HD_CONFORMANCE_WASM_DIGEST` now notes
every module a runtime case runs (entries per profile, test programs, doc
tests, scenario drives: 1041 modules of 1028 cases, up from 180 entry
modules; the runner's own tests add 7 rows) in one file per target,
`conformance-wasm-digests-<target>-<level>.tsv`.

After (d), `ClosureCode` is `Typed` for `V8` and `Browser` and `Abstract`
for `Wasmtime`. `Layouts::new(lowering)` carries the profile to every
`Lay`; `closure_base(closure, code)`, the closure call in emit.rs and the
`List.iter()` and `Map.iter()` cursors read it, and the typed form is the
one before E1 (d2e81fa5). The entry exports (`entry`, `script_entry`,
`test_entry`, `scenario_entry`) pass it to `root_poll`'s layouts. So the `V8` and `Wasmtime` keys differ and compare mode
builds each case twice. The `Wasmtime` modules are byte-identical to
before; 606 of 1048 `V8` modules changed in each level (every one with a
closure type, test programs included), and hello is byte-identical on
every target. `closure_code_targets.rs` is the cross-run test of §5: each
target's build of a closure program runs on Node and, with the `wasmtime`
feature, on wasmtime, with the same output. Closures bench (p50 of 9, median of three interleaved rounds, load 3.5
to 7): V8 dev 374 to 358 ms (-4.3%), release 298 to 285 ms (-4.4%),
Wasm 8712 / 7786 bytes as before E1; wasmtime dev 1684 to 1672 ms,
release 745 to 738 ms, byte-identical Wasm (noise). Results in
crates/hd_opt/DESIGN.md (known closure calls) §7.

After (e), the knobs are measured (§8). `max_insts` 64 and `max_depth`
2 stay on every target, and `closure_spec` stays on: 128/2 emits the
same bench Wasm as 64/2, and 32/1 or no A2 loses most of A2's gain on
both engines. So the table keeps one shared `OPTIMIZED` row. A test in
`hd_driver::pipeline` retunes one row and checks that only that row's
key moves.

## 1. Crates And Boundaries

### 1.1 Who owns what

| Crate | Owns | Never |
| --- | --- | --- |
| `hd_mono` | Collection: the worklist, instance keys (`known_key` included), selection, vtables, witnesses, `Collected`. A new module `hd_mono::plan` holds the types collection reads: `BodyPlan`, `Source` and the `Planner` trait (§2.2). Applying a plan is generic: "call this instance specialized on these known values", "call this known closure directly" | Deciding *whether* to specialize or devirtualize. No pass names, no size or depth policy beyond its own instantiation limits, no `optimized()` |
| `hd_opt` (new) | The pass manager (moved from `hd_mono/src/passes.rs`), the pass bodies, the facts, the pipeline table keyed by (target, level), tuning knobs, the validator and the pipeline hash. It implements `hd_mono::plan::Planner` | Creating instances, emitting Wasm, choosing layouts |
| `hd_wasm` | Emission and link. A `Lowering` profile per (target, level), such as the closure code field. It reads `Collected` targets (`KnownClosure` and the rest) as today | Reading pass names or the pipeline |
| `hd_run_wasmtime` | Engine options per level (Cranelift `None` or `Speed`), as today | Code keys |
| `hd_driver` | Wiring: it picks the (target, level), asks `hd_opt` for the planner and pass hash, asks `hd_wasm` for the lowering, combines both into the one `pipeline` key part, and passes the planner to collection | Policy |
| `hd_base` | Two plain enums, `EngineTarget { V8, Wasmtime, Browser }` and `OptLevel { Dev, Optimized }`, next to `BuildProfile` | Tables |

`EngineTarget` is not `TARGET = "wasm32-gc"` (`hd_driver` :151), which
names the output format and stays in the toolchain key. Every target
emits standard Wasm GC that every engine runs. A target only tunes speed.

### 1.2 Dependency direction

```text
hd_base <- hd_tir <- hd_mono (collection + plan types)
                        ^   ^
                        |   +-- hd_wasm (emission, Lowering)
                        +------ hd_opt  (passes, pipelines; implements Planner)
hd_driver -> hd_opt, hd_wasm, hd_mono       hd_run_wasmtime -> hd_wasm
```

`hd_opt` depends on `hd_mono`, not the reverse. Collection defines the
interface it can consume (dependency inversion), and the driver hands it
a `&dyn Planner`. `hd_wasm` and `hd_opt` never depend on each other.
Module-level (link) passes work on `hd_wasm`'s module form, so their
bodies stay in `hd_wasm` as link options, and the pipeline table only
selects them. No cycle.

### 1.3 How instances flow

```text
TIR bodies ──► body-unit passes (hd_opt, per body, memoized per run)
                 │ writes BodyPlan (facts collection reads)
                 ▼
collection (hd_mono): worklist; resolves each BodyPlan entry against the
                 instance being scanned (its args, sub, known list, the
                 Target::Closure keys it recorded) → Target::KnownClosure,
                 specialized CallTargets, carried known lists
                 ▼
instance-unit passes (hd_opt, Emit task, per instance; later: inline_plan,
                 escape, scalar_replace, liveness) → InstancePlan
                 ▼
emission (hd_wasm) reads Collected + InstancePlan + Lowering
                 ▼
link (hd_wasm) with the pipeline's link options
```

Tiering.md §6.1 had one unit for TIR passes, the instance. This design
adds the **body unit** for decisions that change the instance set. Such
a decision must be known while collection runs, and collection is the
only place instances are made. A1's and A2's analysis reads only the
body's own TIR (`Flow::new` reads no instance argument), so it can run
once per body instead of once per instance as it does today.

## 2. The Pass Interface

### 2.1 Units and declarations

`PassSpec` keeps its fields and gains two:

| Field | Change |
| --- | --- |
| `unit` | `Body` (generic TIR, before and during collection), `Instance` (instance view, Emit task) or `Program` (link). Replaces `Level` |
| `params` | the knobs the pass reads, as `(name, value)` pairs from the pipeline, hashed with the pass (§3.3) |
| `run` | per unit: `fn(&BodyCx, &mut BodyFacts, &mut Fuel)`, `fn(&InstanceCx, &mut InstanceFacts, &mut Fuel)`. `BodyCx` holds the body, its `DefId`, `&dyn ProgramEnv` and the pass's params |

Facts for the two passes:

| Fact | Written by | Read by | Content |
| --- | --- | --- | --- |
| `ValueFlow` | `value_flow` (body, analysis; today's `Flow`) | the two below | sets per local, sub of each instruction, item params, sub parents, generic subs, parameter calls |
| `KnownCallees` (renames `KnownVtables`) | `devirt_known` (A1) | collection | per `CallValue`: `Source::Made(inst)` |
| `ClosureSpec` | `closure_spec` (A2) | collection | per `Call`: one `Option<Source>` per argument; per `Closure`: the item params its sub-body calls (`called_under`); as a callee: `spec_params`, the params a known value makes direct, or none when the body is over `max_insts`, an intrinsic, suspends, or calls no param (today's `copyable`); per `CallValue` of a param: `Source::Param(p)` |

### 2.2 What collection consumes

```rust
// hd_mono::plan
pub enum Source { Made(u32), Param(u32) }     // an instruction, or an item parameter
pub struct BodyPlan {
    pub direct: Vec<(u32, Source)>,               // CallValue -> its callee's source
    pub spec_args: Vec<(u32, Vec<Option<Source>>)>, // Call -> per argument
    pub carry: Vec<(u32, Vec<u32>)>,              // Closure -> params its sub-body calls
    pub spec_params: Option<Vec<bool>>,           // this body as a callee
}
pub trait Planner: Sync {
    fn plan(&self, def: DefId, body: &Body, env: &dyn ProgramEnv) -> Option<Arc<BodyPlan>>;
    fn spec_depth(&self) -> u32;                  // was COPY_MAX_DEPTH
}
```

Collection applies a plan with rules that name no pass:

1. **Direct calls.** At a `CallValue` with `Made(m)`: if `calls[m]` is
   `Target::Closure(k)`, record `Target::KnownClosure(k)`. With
   `Param(p)` in the instance's own sub-body: if its known list has `p`,
   record that key. This is `known_closures` (:2436), unchanged.
2. **Specialization.** At a `Call` whose callee's plan has
   `spec_params`: for each marked parameter, resolve the caller's
   `Source` (a `Made` through `calls`, a `Param` through the known
   list). A `Param` outside the instance's own sub-body skips the call,
   as `Specialize::Skip` does today. Collection then checks that each
   value's type equals the parameter's at the copy's arguments; that
   check is about soundness, not policy. Then it applies `spec_depth` and
   calls `push_known`. This is `copy_target` (:1631) with `copyable`
   replaced by the callee's `spec_params`.
3. **Carrying.** At a `Closure` of a specialized instance: carry the
   known entries whose param the plan lists for it. Skip the closure
   outside the own sub-body, as today (:2104).
4. **Never in a thunk.** A thunk emits one instruction and takes no plan
   (today's `only.is_none()` at :1912).

Every check runs in today's order, so (b1) and (b2) of §6 can keep the
output byte-identical. The dev pipeline's planner has no body passes and
returns `None`, so collection does no plan work at all. Collection no
longer reads `env.optimized()`. `ProgramEnv::optimized` (:131) is
deleted. A new body-unit pass reuses the same three rules if it decides
the same kinds of things (known values, direct calls). If it needs a new
kind, that kind becomes a fourth generic rule in `hd_mono::plan`.

One counter where rules 1 to 3 apply gives docs/compiler/tiering.md §4.3's
`HD_OPT_BISECT` for every body-unit decision at once (later; not in this
plan's jobs).

### 2.3 Caching and incremental cost

- **The rule (docs/compiler/tiering.md §6.4, kept).** A pass's output must follow from
  what the code key already covers, or its effect must be hashed into
  `callee_reps`. A body plan reads the body's TIR, which the code key
  covers through the TIR hash, and the pass params, which the pipeline
  hash covers. The one cross-body read is the callee's `spec_params` at
  the caller. Its effect is the copy's instance key (`known_key`), which
  `hash_target` already hashes into the caller's `callee_reps`. A
  `KnownClosure` follows from the instance's own TIR, args and known
  list, all in its code key (`hd_driver` :6111).
- **No new cache entry.** Plans are memoized per run by body identity,
  not stored. Cost: `value_flow` plus the two decisions are linear in
  instructions and locals, an estimated 5 to 20 ns per instruction per
  pass (docs/compiler/tiering.md §1). That is 1 to 5 ms for the 10k-line application,
  against about 640 ms of Cranelift. Before job (b2), `Flow::new` ran once per
  *instance* scanned (every A2 copy and every carried sub-body
  included), so the memo is no slower and usually faster. A disk entry
  would save at most those few ms per edit, and its lookup costs a hash
  and a read per body. Not worth it until a body pass costs more than
  about 10% of collection.
- **Per edit.** An edited body re-plans in microseconds. Its instances
  re-emit (their TIR changed). Callers re-emit only if a copy decision
  changed, because the copy's key moved. Collection reruns on any
  change today anyway (the link key gates it), so plans add no new
  invalidation.
- **Versions.** A pass `version` bump or a knob change misses only the
  pipelines that hold that pass.

## 3. Pipelines Keyed By (Target, Level)

### 3.1 The table

The composite for each key. Each column is data in the crate that reads
it: passes and knobs in `hd_opt`, lowering in `hd_wasm`, engine options
in `hd_run_wasmtime`.

| Target | Level | Body passes | Instance passes | Knobs | Lowering | Engine |
| --- | --- | --- | --- | --- | --- | --- |
| `V8` (Node) | dev | none | `liveness` | none | closure code typed | V8 tiers by itself |
| `Wasmtime` | dev | none | `liveness` | none | closure code abstract (E1) | Cranelift `None` |
| `Browser` | dev | none | `liveness` | none | as `V8` (Q2) | the browser's |
| `V8` | optimized | `value_flow`, `devirt_known`, `closure_spec` | the declared stubs, `liveness` | `closure_spec`: `max_insts` 64, `max_depth` 2 (measured in §8) | typed | none |
| `Wasmtime` | optimized | same | same | 64, 2 | abstract (E1) | Cranelift `Speed` |
| `Browser` | optimized | same | same | as `V8` | as `V8` | the browser's |

- Dev runs no A1 or A2 on any target: the owner decided on 2026-10-07
  that specialization and devirtualization run in the optimized pipeline
  only (docs/compiler/tiering.md §8.1 Q3). This note does not reopen it.
- **Typed** is the layout before commit d2e81fa5. Each signature has an
  open base `$Fn` whose code field has the concrete function type, and a
  call does `struct.get` then `call_ref` with no cast. **Abstract** is
  E1: one base for all signatures with a `funcref` field, and a call
  casts the code before `call_ref`. That commit touched about 70 lines
  of `hd_wasm`, so keeping both is cheap.
- Until job (d), every row's lowering is abstract, which is today's
  output.

### 3.2 Validation

The current checks stay (`validate`, passes.rs :194): each fact is read
only after a pass writes it, and every `Always` pass is present. New
checks, all run once per process and in a unit test:

1. The table is total. A `match` on `(EngineTarget, OptLevel)` with no
   wildcard makes this a compile-time check.
2. A body-unit pass reads only body-unit facts. Collection-time facts
   are written only by body-unit passes.
3. Every knob is within its declared range, for example `max_depth` ≥ 1.
4. Each pipeline's hash differs from every other pipeline whose content
   differs, and equals that of every pipeline whose content is the same.

### 3.3 Hashing

`pipeline = H(hd_opt pass hash, hd_wasm lowering hash)`. The pass hash
covers each body, instance and link pass's name, version and params, in
order. The lowering hash covers each lowering field by name and value.
The **target name is not hashed, only content.** So `V8` and `Browser`
share cache entries while their content is equal, and a target whose
lowering does not change keeps its cache across a job like (d). Engine
options stay out of code keys: the wasmtime cwasm cache keys its own
opt level. The value goes where `self.pipeline` goes today: `prog_key`,
`code_key` and `prelude_key` (`hd_driver` :1965, :5842, :6111).

Cost of the coarse key: switching targets rebuilds every instance once,
an estimated 0.55 CPU-s for the 10k-line application in dev (docs/compiler/tiering.md
§2.5). After that, both targets stay cached. A finer key would add the
lowering hash only to instances whose code reads a lowering choice. For
E1 that means instances that make or call closures. It needs read
tracking in emission. Reopen it only if users switch targets often, or
if there are more than about three lowering choices.

## 4. Target Profiles For Lowering

`hd_wasm::Lowering` is a small `Copy` struct with one field per choice.
`Lowering::of(EngineTarget, OptLevel)` returns it, and `Layouts` (shared
per build, `crates/hd_wasm/src/layout.rs` :117) carries it to every
`Lay`. So `closure_base(lowering)` and the call sequence in emit.rs
:619-628 branch on one field. `ProgramEnv` gains nothing.

| Choice | V8 | Wasmtime | Browser | Evidence |
| --- | --- | --- | --- | --- |
| closure code field (E1) | typed | abstract | typed | wasmtime: abstract is -21.5% dev and -31.3% release on closures; V8: abstract is +2.8% to +6.3% (crates/hd_opt/DESIGN.md (known closure calls) §7) |
| E2, code index plus table (future) | | candidate | | crates/hd_opt/DESIGN.md (known closure calls) §3 |
| capture-free closures as globals (built, #49) | all | all | all | not engine-specific: plain lowering in `hd_wasm`, in no profile |

A choice goes into the profile only if it is engine-specific and
measured on at least two engines. A choice that helps every engine is
plain lowering.

**Artifacts that must run on more than one engine.** `hd build` writes
a `.wasm` that `hd FILE.wasm` later runs on whatever engine that command
picks. Every profile emits valid Wasm GC for every engine, so a mismatch
costs speed only. The worst case measured so far is a wasmtime run of a
typed build: about 21% to 31% slower on closure-heavy code. A V8 run of
an abstract build is 3% to 6% slower. Options:

| Option | Cost | Verdict |
| --- | --- | --- |
| One target per build, resolved like `hd run`'s engine: `--engine`, else `HD_ENGINE`, else the default engine | `hd build` takes `--engine` (an implementation flag, as on `hd run`; the spec is untouched). `hd FILE.wasm` with the same environment runs it on the engine it was tuned for | **recommended** (Q1) |
| A portable profile (each choice chosen for the smallest worst case; today abstract) | Never the best on any engine; one more profile to test | only if Q1 rejects per-engine builds |
| Two modules per build | 2x build time and disk for one artifact | rejected |

The default engine is Node today. When wasmtime becomes the default (the
wasmtime CLI direction), `hd build`'s default target follows it, so
`bench/runtime/run.mjs`, which runs the output on Node, must pass
`--engine node`.

## 5. How The Target Reaches Each Build Path

`Packages::optimized: bool` becomes `target: EngineTarget` and `level:
OptLevel`. `build_profile` stays separate (docs/compiler/tiering.md §5.3: profile and
pipeline are different things).

| Path | Target | Level | Change |
| --- | --- | --- | --- |
| `hd run`, `hd FILE` | the engine of the run: `--engine`, else `HD_ENGINE`, else Node | `--release` | the engine is parsed before compile (`Words::engine`); pass it into `compile_in` (`crates/hd_cli/src/main.rs` :383) with the level |
| `hd test` | the same, from `test_cmd` options | `--release` (checked profile) | `test_cmd.rs` :539 and :739 |
| `hd build` | the same resolution (Q1) | `--release` | accept `--engine` (today only `hd run` takes it, main.rs :659) |
| `hd FILE.wasm` | none: runs a prebuilt module | none | none |
| REPL | `Wasmtime` (the REPL is wasmtime-only) | dev | `crates/hd_cli/src/repl.rs` :457; session preludes key on the pipeline as now |
| playground, web compiler (`hd_web`) | `Browser` | dev | `crates/hd_web/src/lib.rs` :162 and :337 |
| `hd check`, `hd doc`, `Goal::Analyze` | `Packages::default()`, never emitted | none | `crates/hd_cli/src/disk.rs` :217 |
| conformance | per engine run: the Node pass builds for `V8`, the wasmtime pass for `Wasmtime` | `HD_CONFORMANCE_PIPELINE` | Compare mode builds each runtime case once per target, then compares outputs (Q3) |
| engine options | — | from the level | `EngineConfig::pipeline(release)` takes `OptLevel` |

Conformance today has 1034 runtime cases (929 under `runtime/valid`, 105
under `runtime/panic`); 1028 of them build and run 1041 modules (an entry
per runtime profile, test programs, doc tests, scenario drives). While
two targets have equal content, their keys are equal and the second build
is a cache hit.
After job (d) they differ, so Compare mode builds each case twice. A
small cross-run test (closure fixtures built for one target and run on
the other engine) guards the "valid everywhere" property.

## 6. Refactor Plan

One job at a time, about an hour each. **Common done checks for every
job:**

- conformance green with `HD_CONFORMANCE_PIPELINE=dev` and with
  `=optimized`, each with `HD_CONFORMANCE_ENGINE=wasmtime` (Compare);
- `cargo fmt`, `clippy` and `test` green;
- a report of hello (dev and release bytes, function count, build time)
  and the closures bench (wasmtime dev and release with `hd run
  [--release] --engine wasmtime`; V8 dev and release with `node
  host/run.mjs`; p50, load average noted). The baseline is crates/hd_opt/DESIGN.md (known closure calls)
  §7: hello 4549 / 4032 bytes; closures wasmtime 1502 / 716 ms, V8 361 /
  281 ms, Wasm 8714 / 7767 bytes.

| # | Job | Done check |
| --- | --- | --- |
| a | Create `hd_opt`. Move `passes.rs` into it byte for byte: `PassSpec`, `FactId`, gates, `DEV`, `OPTIMIZED`, `validate` and their tests. Point `hd_driver` :1423 at it, and update the `crates/README.md` table and graph. Add a test-only switch to the conformance harness that writes each runtime case's Wasm digest, so later jobs can prove output identity in one command | both pipeline hashes equal before and after (a unit test pins them); every runtime case's Wasm digest identical in both pipelines; `hd_mono` has no `passes` module |
| b1 | Add `hd_mono::plan` (`Source`, `BodyPlan`, `Planner`) and collection rules 1 and 4 (§2.2). In `hd_opt`: `value_flow` (today's `Flow`, moved) and `devirt_known` (A1's `Made` half of `known_closures`). The driver passes the pipeline's planner to collection. A2 stays in place behind the old flag for this one job | digests identical in both pipelines; `known_closure_calls.rs` green; the A1 path in `hd_mono` reads no pass name |
| b2 | `closure_spec`: `spec_params` (`copyable` with `max_insts`), per-call argument sources, `carry`, and the `Param` half of direct calls. Collection rules 2 and 3 replace `copy_target`, `copyable` and the carry block; `spec_depth` comes from the planner. Delete `Flow`, `COPY_MAX_*` and `ProgramEnv::optimized` from `hd_mono` | digests identical in both pipelines; no `optimized()`, `COPY_MAX` or `struct Flow` left under `crates/hd_mono`; dev collection makes no plan call (a counter in a test) |
| c1 | Add `EngineTarget` and `OptLevel` to `hd_base`. Add the `hd_opt` table (`pipeline(target, level)`, a total `match`), `hd_wasm::Lowering::of` with every row abstract, the content hash (§3.3) and validator checks 1 to 4. Replace `Packages::optimized` with `target` and `level` | digests identical on every target in both levels; a test shows `V8` and `Wasmtime` hashes equal while their content is equal, and different when a knob differs; keys change once (noted in the commit) |
| c2 | Pass the target into every build path in the §5 table: `hd run`, `hd FILE`, `hd test` and `hd build` (`--engine`), the REPL, `hd_web`, and conformance per engine. `EngineConfig::pipeline` takes `OptLevel` | a test per path asserts the (target, level) it builds with; `hd build --engine wasmtime` accepted, and refused with today's message when the feature is off; `bench/runtime/run.mjs` passes `--engine node` |
| d | E1 for wasmtime only: `Lowering.closure_code` typed or abstract; `closure_base`, emit.rs :619-628, :3447, :3757 and the `rt`/`map` helpers branch on it, with the typed path restored from before d2e81fa5. `V8` and `Browser` become typed. Add the cross-run test (§5) | V8 closures back to pre-E1 (about 347 / 273 ms, within noise); wasmtime closures unchanged (1502 / 716 ms within noise); hello byte-identical on all targets; the wasmtime target's digests unchanged; the cross-run test passes |
| e | Per-pipeline knobs: `closure_spec`'s `max_insts` and `max_depth` as table params, hashed. Measure on V8 release: A2 off, and bounds 32/1, 64/2 and 128/2, on the runtime suite. Apply the E8 rule (crates/hd_mono/DESIGN.md §12.8: keep a budget only if it buys at least 5% speed per 10% size). Record the table in this file | a knob change changes only its own pipeline's hash (test); a measured table; V8 knobs set by the rule, wasmtime unchanged unless the rule says otherwise |

b1 and b2 split the task's job (b), because the plan types, the
collection rules and the A1 move together fill an hour before A2 starts.
c1 and c2 split job (c) the same way.

## 7. Owner Questions

1. **What target does `hd build` use?** Options: (a) resolve it like
   `hd run`'s engine (`--engine`, `HD_ENGINE`, the default engine), so
   `hd build` gains `--engine`; (b) a portable profile for every build;
   (c) a new `--target` word. **Recommendation: (a).** It reuses one
   rule and one flag. `hd FILE.wasm` in the same environment runs the
   build on the engine it was tuned for. A mismatch costs speed, never
   correctness. The spec needs no change, since engines are an
   implementation matter there.
2. **What does the `Browser` profile choose?** Only V8 is measured;
   SpiderMonkey and JavaScriptCore are not. **Recommendation:** the same
   choices as `V8` until a measurement on Firefox shows otherwise. The
   playground's floor already names Chrome first (crates/hd_web/DESIGN.md §3).
3. **What does conformance Compare mode build?** Options: (a) one build
   per engine's target, which tests what users run and costs a second
   build of each runtime case once targets differ; (b) one build run on
   both engines, which tests engine parity only. **Recommendation:
   (a)**, plus the small cross-run test of §5, because target-specific
   lowering is exactly what a single build would miss. Report the suite
   CPU delta in job (d) against the `suite-cpu` budget.
4. **May job (e) turn a pass off for one target when the E8 rule says
   so?** On V8, A2 buys about ±3% (crates/hd_opt/DESIGN.md (known closure calls) §3) and adds code.
   **Recommendation:** yes. Job (e) sets knobs and pass lists per target
   by the E8 rule, records the measurements here, and asks only when the
   rule and the numbers disagree.

## 8. Knob Measurements (Job e)

Measured 2026-10-14 on the runtime suite (`bench/runtime/progs`, six
programs) with `hd build --release`, a fresh cache per build. Each
variant is a dev `hd` binary whose `OPTIMIZED` row was edited, built
and copied; the source was restored, and no override switch was
committed. V8 runs `node host/run.mjs`. Wasmtime runs `hd --release
--engine wasmtime FILE.wasm`; the cwasm is cached after the warmup, so
it times the run, not Cranelift. Each round takes each variant's p50
of 7 runs (V8) or 5 runs (wasmtime) after a warmup, rotating the
variant order. The cells are the median of three rounds.

### 8.1 V8 release (`--engine node`)

Load average 7.7 at the start, then 3.7 to 4.6.

| Program | A2 off | 32/1 | 64/2 (today) | 128/2 |
| --- | ---: | ---: | ---: | ---: |
| closures | 323 ms | 331 ms | 292 ms | 289 ms |
| int-loop | 1629 ms | 1656 ms | 1655 ms | 1648 ms |
| list-sort | 1052 ms | 1031 ms | 1039 ms | 1034 ms |
| map-count | 37 ms | 37 ms | 40 ms | 38 ms |
| string-build | 2485 ms | 2570 ms | 2532 ms | 2518 ms |
| trait-dispatch | 796 ms | 796 ms | 797 ms | 789 ms |
| closures Wasm | 7774 B | 7781 B | 7786 B | 7786 B, identical to 64/2 |
| total Wasm, six programs | 37540 B | 37547 B | 37552 B | 37552 B |
| total compile, six builds | 346 ms | 359 ms | 349 ms | 370 ms |

### 8.2 Wasmtime release (`--engine wasmtime`)

Load average 5.6 at the start, peaking at 13.7.

| Program | A2 off | 32/1 | 64/2 (today) | 128/2 |
| --- | ---: | ---: | ---: | ---: |
| closures | 1152 ms | 1020 ms | 781 ms | 781 ms |
| int-loop | 974 ms | 973 ms | 971 ms | 976 ms |
| list-sort | 463 ms | 461 ms | 464 ms | 475 ms |
| map-count | 24 ms | 24 ms | 24 ms | 25 ms |
| string-build | 167 ms | 166 ms | 166 ms | 166 ms |
| trait-dispatch | 663 ms | 668 ms | 664 ms | 665 ms |
| closures Wasm | 7776 B | 7776 B | 7767 B | 7767 B, identical to 64/2 |
| total Wasm, six programs | 37547 B | 37547 B | 37538 B | 37538 B |

### 8.3 Notes

- Only closures changes with the knobs. The other five programs emit
  byte-identical Wasm in every variant, so their spread is noise:
  within 3.5%, and int-loop's first round with A2 off hit 2332 ms.
- Compile time is noise: 55 to 72 ms per build in every variant. One
  wasmtime run showed about 320 ms for closures in three variants
  during the load peak. A rerun gave 61 to 125 ms for every variant.
- Conformance, optimized `V8` digests against 64/2: 128/2 changes 2 of
  1048 modules (the test programs of `list-group-by` and
  `list-sorted-by-key`). 32/1 changes 9.

### 8.4 Decision

The E8 rule (crates/hd_mono/DESIGN.md §12.8) keeps a budget only if it buys at least
5% speed per 10% size.

| Choice | V8 | Wasmtime | Verdict |
| --- | --- | --- | --- |
| A2 on (64/2) against off | -9.6% time on closures for +12 B (+0.15%) | -32% time for -9 B | keep A2 on both targets (Q4: no pass is turned off) |
| 128/2 against 64/2 | same Wasm on the suite; 2 corpus modules change | same Wasm | no measured gain, so the larger budget is not taken |
| 32/1 against 64/2 | +13% time, -5 B | +31% time, +9 B | loses most of A2's gain; rejected |

So `V8`, `Browser` and `Wasmtime` all keep `max_insts` 64 and
`max_depth` 2. The table keeps one shared `OPTIMIZED` row. A target
whose measured best differs would get its own `Pipeline` static in
`hd_opt::passes::pipeline`.

# Calling Known Closures Directly (#53)

Status: design record; may lag the code. The code and the spec win.

Step 1 of #53: a plan, no code. It covers where closures are made and
called today, what is known at each call after monomorphization, five
options with prices, a recommended order of jobs, and owner questions.
Inputs: the #52 profile (deleted 2026-10-15, its numbers are on #65; `git show
44890c30:audit/compiler/profile-wasmtime-closures.md`),
M37 in `f1c55020:future-work/compiler/wasmtime-perf.md`,
and the closure specialization schedule that
[crates/hd_mono/DESIGN.md §12.6](../hd_mono/DESIGN.md) and
[docs/compiler/lowering-catalog.md](../../docs/compiler/lowering-catalog.md#optimization-passes) already
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
| B: hd inlining alone | the declared `inline_plan`: direct callees within budget inlined through an instance view | the 2.5 direct `next` calls: ≈ −75 ms | ≈ 0 | per the existing budgets (crates/hd_mono/DESIGN.md §12.8); needs inline call sites in `hd.lines` for panic frames | optimized |
| C: the specialization schedule | inline, forward the never-written `step` and environment fields, call the known body, inline it; at most 4 rounds (crates/hd_mono/DESIGN.md §12.6) | all 5 calls, the `next` calls and the `Option` traffic: ≈ 0.57 s (1.5x V8; the rest is #52 cause 3, bounds and GC access shape) | −0 to −5% | +50 to 150 bytes per stage per chain site; emission +10 to 20% on callers with chains; the largest job | optimized; this is #65's ground |
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
- **Capture-free closures as constant globals** (crates/hd_mono/DESIGN.md table)
  saves one allocation and one interning per construction. The bench
  builds about 5 closures in total, so no gain here; it stays a separate
  item. Built in #49 as plain lowering in every pipeline: a closure whose
  environment holds no value is `global.get` of an immutable global per
  closure code (`GSym::Closure`).

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
| 6 | D: measure wasmtime's inliner at `Speed` on the bench, the runtime suite and the conformance suite's Cranelift time; count panic frame-list differences | ship only if it buys at least 5% (docs/compiler/tiering.md T1 rule) and owner Q4 allows it; if shipped, the compile cache key includes the setting |
| 7 | (If owner Q3 approves E1) the abstract `$code` field plus `ref.cast`, dev and release | ship if wasmtime dev closures improves at least 10% and V8 loses at most 3% on the runtime suite; hello bytes at most +0.5% |
| 8 | Re-measure and close #53: final numbers into this file, then the #65 decision on C | the table in section 3 filled with measured values |

## 6. Owner Questions

1. **Does A2 run in the dev pipeline too?** The 2026-10-07 decision puts
   specialization and devirtualization in the optimized pipeline only,
   so `hd run`, `hd test` and the REPL would keep paying about 15 ns per
   closure call. A2 also puts specialization in collection (new
   instances), where docs/compiler/tiering.md §6.6 had `closure_spec` extend an inline
   set. **Recommendation:** optimized only, as decided; job 3 reports
   the dev cost (instances, emission and Cranelift time over the suite)
   so the question can be reopened with numbers.
2. **How much code growth is acceptable for specialization?**
   **Recommendation:** the E8 rule already in crates/hd_mono/DESIGN.md §12.8 (a budget
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

## 6. Result of job 6 (D: wasmtime's inliner), 2026-10-13

Not shipped. Measured on wasmtime 49.0.2 with `hd run --release
--engine wasmtime` after A1 and A2, a dev `hd` binary, load average 6 to
9, median of 10 runs per cell, wall time including the compile.

| Program | Inliner off | `InterModuleAndIntraGc` | `Inlining::Yes` |
| --- | --- | --- | --- |
| closures | 1084 ms | about 1085 ms (0%) | 991 ms (-8.6%) |
| int-loop | 983 ms | | 1002 ms (+1.9%) |
| list-sort | 465 ms | | 487 ms (+4.7%) |
| map-count | 31 ms | | 48 ms (+56%) |
| string-build | 184 ms | | 195 ms (+6.1%) |
| trait-dispatch | 671 ms | | 683 ms (+1.9%) |

- The mode named in the plan, `InterModuleAndIntraGc`, did nothing for
  the bench: hd's release Wasm does not qualify for its intra-module
  inlining. Only `Inlining::Yes` inlines, and it gains on the closures
  bench alone.
- The rest of the runtime suite gets slower, 2% to 6%, and the compile
  grows (map-count, mostly compile: +17 ms; an `hd test --release` of a
  four-case package: user time 0.04 s to 0.07 s).
- With `Yes`, every wasmtime panic report in a release `hd test` lost all
  its frames: the four panic cases (assert in std, explicit panic, index
  out of bounds, a panic inside a `map` closure) went from 3 to 8 frame
  lines each to none, not just the inlined ones. Dev is unchanged.

The T1 rule (at least 5% on the closures bench and the runtime suite
without a compile regression) fails on the suite, the compile time and
the frame lists, so inlining stays off. Owner Q4 is answered: leave
wasmtime inlining off. Re-measure after a wasmtime move or after job 7 if
the closures bench still lags V8.

## 7. Result of job 7 (E1: abstract code field), 2026-10-13

Shipped. The closure base is one struct for every signature,
`(sub (struct (field (mut funcref))))`; environments and the `ListStep`
and `MapStep` cursors extend it. A call reads the field, then
`ref.cast (ref $Code)` (a final type) before `call_ref`. Known closures
(A1, A2) still call directly.

Measured with a dev `hd` binary, the old and new builds interleaved
per round, p50 of 9 runs after a warmup (closures) or 5 runs (suite),
load average 7 to 10 with spikes. Wasmtime is `hd run [--release]
--engine wasmtime`, wall time including the compile; V8 is
`node host/run.mjs`.

| Program | Engine, profile | Before | After | Change |
| --- | --- | --- | --- | --- |
| closures | wasmtime dev | 1912 ms | 1502 ms | -21.5% |
| closures | wasmtime release | 1042 ms | 716 ms | -31.3% |
| closures | V8 dev | 347 ms | 361 ms | +4.0% |
| closures | V8 release | 273 ms | 281 ms | +2.8% |
| list-sort | wasmtime dev / release | 1270 / 462 ms | 1273 / 457 ms | +0.3% / -1.0% |
| list-sort | V8 dev / release | 1300 / 1034 ms | 1296 / 1030 ms | -0.3% / -0.4% |
| int-loop, map-count, string-build, trait-dispatch | both | | | Wasm byte-identical; noise within ±4% |

- A V8-only rerun of closures, 15 runs: dev 348 to 370 ms (+6.3%),
  release 279 to 287 ms (+2.9%). The cast costs V8 about 0.2 ns per
  call, and V8 still runs the bench 2.5x to 4x faster than wasmtime.
- Release gains more than dev: A2 leaves the data-field calls
  (`Iterator.step` and captured iterators) indirect, and each of those
  paid the full subtype check.
- Hello: dev 4549 bytes, release 4032 bytes, both byte-identical (no
  closure). Closures bench Wasm: dev 8712 to 8714 bytes, release 7786
  to 7767 bytes.
- The plan-row rule holds: wasmtime dev closures -21% (at least 10%), V8
  on the runtime suite within 3% (four programs are byte-identical,
  list-sort -0.4%), hello +0%. The cost is V8 on the closures bench
  itself, +3% to +6%.
- Per target since job (d) of #162 (crates/hd_opt/DESIGN.md (optimization pipelines) §4): only the
  `Wasmtime` target uses E1; `V8` and `Browser` keep the typed code
  field. Closures bench, p50 of 9, median of three interleaved rounds:
  V8 dev 374 to 358 ms (-4.3%), release 298 to 285 ms (-4.4%); wasmtime
  dev 1684 to 1672 ms, release 745 to 738 ms (byte-identical Wasm).
