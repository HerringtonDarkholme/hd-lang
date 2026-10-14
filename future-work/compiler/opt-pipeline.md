# Optimization Pipelines By Target And Level (#162)

Step 1 of #162: a design note, no code. Owner direction (2026-10-13):
optimizations run as passes in pipelines chosen by (target, level), not
as branches inside collection. A new `hd_opt` crate holds the passes.
`hd_mono` goes back to monomorphization and collection. #53's A1 and A2
become the `devirt_known` and `closure_spec` passes. Lowering choices
that depend on the engine, such as E1's closure layout, live in a
per-target profile that `hd_wasm` reads. Every build knows its target
engine.

Inputs: [tiering.md](tiering.md) §6 (pass manager, facts, pipelines,
keys), [closure-calls.md](closure-calls.md) §3.1 and §6 to §7 (A1, A2,
E1 and their measurements), [codegen.md](codegen.md) §12 to §13, and
[wasm-layout.md](wasm-layout.md) §15.2.

## 0. Today

| What | Where | Problem |
| --- | --- | --- |
| Pass manager: `PassSpec` (:52), `FactId` (:33), gate bits (:22), `DEV` (:169), `OPTIMIZED` (:174), `validate` hashing the pipeline (:194) | `crates/hd_mono/src/passes.rs` | Every pass except `liveness` (:73) is a stub that returns `Skipped`. Optimization does not run through it |
| A1 and A2 | `crates/hd_mono/src/lib.rs`: `Flow` (:2283), `copyable` (:1583), `copy_depth` (:1608), `copy_target` (:1631), `push_known` (:956), carrying known closures into sub-bodies (:2096-2115), `known_closures` (:2436, called at :2202), `COPY_MAX_INSTS` (:2276), `COPY_MAX_DEPTH` (:2280) | Optimization policy sits inside collection, behind `env.optimized()` (:1911). A third optimization would add a third branch |
| Specialized instances | `crates/hd_mono/src/layout.rs`: `known_digest` (:743), `known_key` (:761), the `known` column (:778), `get_known` (:803) | Already generic: "an instance specialized on known parameter values". It stays |
| Direct call of a known closure | `crates/hd_wasm/src/emit.rs` :619-628 reads `Target::KnownClosure` | Already generic. It stays |
| E1 closure layout | `crates/hd_wasm/src/layout.rs` `closure_base` (:211), used at layout.rs :868, :1635 and emit.rs :3447, :3757 | Applies to every target. It costs V8 3% to 6% on closure-heavy code (closure-calls.md §7) |
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
closure-calls.md §7.

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

One counter where rules 1 to 3 apply gives tiering.md §4.3's
`HD_OPT_BISECT` for every body-unit decision at once (later; not in this
plan's jobs).

### 2.3 Caching and incremental cost

- **The rule (tiering.md §6.4, kept).** A pass's output must follow from
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
  pass (tiering.md §1). That is 1 to 5 ms for the 10k-line application,
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
  only (tiering.md §8.1 Q3). This note does not reopen it.
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
an estimated 0.55 CPU-s for the 10k-line application in dev (tiering.md
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
| closure code field (E1) | typed | abstract | typed | wasmtime: abstract is -21.5% dev and -31.3% release on closures; V8: abstract is +2.8% to +6.3% (closure-calls.md §7) |
| E2, code index plus table (future) | | candidate | | closure-calls.md §3 |
| capture-free closures as globals (future) | all | all | all | not engine-specific: such a choice belongs in no profile |

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
OptLevel`. `build_profile` stays separate (tiering.md §5.3: profile and
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
  host/run.mjs`; p50, load average noted). The baseline is closure-calls.md
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
| e | Per-pipeline knobs: `closure_spec`'s `max_insts` and `max_depth` as table params, hashed. Measure on V8 release: A2 off, and bounds 32/1, 64/2 and 128/2, on the runtime suite. Apply the E8 rule (codegen.md §12.8: keep a budget only if it buys at least 5% speed per 10% size). Record the table in this file | a knob change changes only its own pipeline's hash (test); a measured table; V8 knobs set by the rule, wasmtime unchanged unless the rule says otherwise |

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
   playground's floor already names Chrome first (playground-wasm.md §3).
3. **What does conformance Compare mode build?** Options: (a) one build
   per engine's target, which tests what users run and costs a second
   build of each runtime case once targets differ; (b) one build run on
   both engines, which tests engine parity only. **Recommendation:
   (a)**, plus the small cross-run test of §5, because target-specific
   lowering is exactly what a single build would miss. Report the suite
   CPU delta in job (d) against the `suite-cpu` budget.
4. **May job (e) turn a pass off for one target when the E8 rule says
   so?** On V8, A2 buys about ±3% (closure-calls.md §3) and adds code.
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

The E8 rule (codegen.md §12.8) keeps a budget only if it buys at least
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
