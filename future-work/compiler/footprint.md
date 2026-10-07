# New Compiler: Footprint

Part of the [compiler design](README.md).

Status: report, 2026-10-07, after M1 (`07c74892`). One row per
design-doc section: where its code is and how far it goes. **Real**: the
structure or algorithm the section designs is implemented and tested,
possibly over the subset. **Skeleton**: its types and entry points exist
with real signatures, and the work answers a structured "not
implemented". **Missing**: no code yet. Paths are crates under
`compiler/crates`. Unnumbered material is left out. Numbered sections
that hold no code by nature (prior art, estimates, owner questions,
change lists) count as missing, which is most of the missing rows of
type-checking.md, trait-solver.md, live-execution.md and tiering.md.
The stage counts on `lib/std` are in
[skeleton-findings.md](skeleton-findings.md#architecture-skeleton-findings).

## Totals

| Doc | Real | Skeleton | Missing | Sections |
| --- | --- | --- | --- | --- |
| [data-structures.md](data-structures.md) | 8 | 14 | 3 | 25 |
| [syntax.md](syntax.md) | 5 | 1 | 0 | 6 |
| [resolution-and-interfaces.md](resolution-and-interfaces.md) | 1 | 5 | 0 | 6 |
| [checking-and-tir.md](checking-and-tir.md) | 0 | 3 | 0 | 3 |
| [type-checking.md](type-checking.md) | 6 | 17 | 38 | 61 |
| [trait-solver.md](trait-solver.md) | 9 | 12 | 37 | 58 |
| [cache.md](cache.md) | 4 | 4 | 1 | 9 |
| [scheduler.md](scheduler.md) | 2 | 3 | 0 | 5 |
| [codegen.md](codegen.md) | 2 | 11 | 9 | 22 |
| [suspension.md](suspension.md) | 0 | 7 | 2 | 9 |
| [wasm-layout.md](wasm-layout.md) | 2 | 3 | 3 | 8 |
| [runtime-and-host.md](runtime-and-host.md) | 3 | 5 | 6 | 14 |
| [engines-and-test-runner.md](engines-and-test-runner.md) | 1 | 5 | 6 | 12 |
| [commands.md](commands.md) | 3 | 5 | 7 | 15 |
| [live-execution.md](live-execution.md) | 3 | 2 | 35 | 40 |
| [tiering.md](tiering.md) | 3 | 2 | 26 | 31 |
| all | 52 | 99 | 173 | 324 |

## Sections

| Section | Code path | Status |
| --- | --- | --- |
| data-structures.md §3.1 IDs And Their Scopes | hd_base (`id!`, IDs, `NONE`) | real |
| data-structures.md §3.2 Stable Paths And Stable Hashing | hd_intern::paths (`PathTable`), hd_base::stable | real |
| data-structures.md §3.3 Interners | hd_intern (`ShardedInterner`, `PathTable`) | skeleton |
| data-structures.md §3.4 Types | hd_types::pool (`TyData`, `InternPool`, rows); no body-local pool, inference variables use the global pool and one global mutex, checker unused | skeleton |
| data-structures.md §3.5 Arenas And Lifetimes | none | missing |
| data-structures.md §3.6 The Poison Type | hd_types (`Ty::POISON`, `HAS_POISON`, unifier) | real |
| data-structures.md §3.7 Spans And Files | hd_base (`Span`) | real |
| data-structures.md §3.8 Diagnostic Records | hd_diag::buf (`DiagBuf`, renderers) | skeleton |
| data-structures.md §3.9 Data-Oriented Encoding | hd_base::append (`AppendVec`, `Col`, `Range32`), hd_types::pool | skeleton |
| data-structures.md §3.10 IR Abstraction Contracts | hd_tir::ir (verifier contract) | skeleton |
| data-structures.md §3.11 Tokens And Line Tables | hd_syntax::lexer (`TokenBuf`) | real |
| data-structures.md §3.12 Layout Cursor And Skim State | hd_syntax::layout (`LayoutCursor`); skim runs the full lexer, no skim state | skeleton |
| data-structures.md §3.13 The Green Tree, Its Wire Format And The JS Decoder | hd_syntax::green (`GreenTree`, wire form); no JS decoder | skeleton |
| data-structures.md §3.14 The Header Skeleton And The Item Index | hd_syntax::skim, parser (`ItemIndex`); no header tree, use list holds body lines | skeleton |
| data-structures.md §3.15 Name-Resolution Tables | hd_resolve (`ModuleScope`, `Binding`, `FolderExports`), used by the one driver but incomplete | skeleton |
| data-structures.md §3.16 The Folder Interface: In Memory And As A Blob | hd_resolve::iface (`FolderIface`, codec, deep hash); no zero-copy reader | skeleton |
| data-structures.md §3.17 Impl Tables | hd_types::solver (`ImplTable`, `HeadKey`) | skeleton |
| data-structures.md §3.18 TIR | hd_tir::ir (`Body` columns), verifier and `wire` codec with stable TIR hash; the running path uses it | real |
| data-structures.md §3.19 Per-Body Checker Scratch | hd_types::unify (`InferTable`) | skeleton |
| data-structures.md §3.20 Cache Entries And The Manifest | the driver uses `encode_entry` and `decode_entry` for interface, check, code and link entries; the stat manifest and several sections remain absent | skeleton |
| data-structures.md §3.21 The Scheduler's Task Graph | hd_sched (`TaskGraph`, `TaskKind`, `Spawn`, creation guards), used by serial, stepping and rayon executors | real |
| data-structures.md §3.22 Codegen: The Instance Table And Code Entries | hd_mono::layout and collection, used by the driver for per-instance code entries | real |
| data-structures.md §3.23 Wasm Emission Buffers | hd_wasm (subset emitter) | skeleton |
| data-structures.md §3.24 Memory Budget | none | missing |
| data-structures.md §3.25 The Schema Language | none (the tag schema is a Rust macro in hd_tir::ir) | missing |
| syntax.md §4.1 Lexer | hd_syntax::lexer | real |
| syntax.md §4.2 Layout | hd_syntax::layout | real |
| syntax.md §4.3 Skim Mode And The Header Pass | hd_syntax::skim runs the full lexer and builds no header tree | skeleton |
| syntax.md §4.4 Parser And Green Tree | hd_syntax::parser, structure and green, used by the one driver; 395 accepted fixtures still report a parse diagnostic | real |
| syntax.md §4.5 Header Extraction And The API Text Hash | hd_syntax::skim (`api_text_hash`) | real |
| syntax.md §4.6 Item Index | hd_syntax::parser (`ItemIndex`) | real |
| resolution-and-interfaces.md §4.7 Discovery, Module Identity And Folders | hd_project (`ModuleTable::discover`, `SourceSet`, manifest), used by the one driver | skeleton |
| resolution-and-interfaces.md §4.8 Folder Graph | hd_project (`FolderGraph`: order, cycles, closures, heights), used by the one driver | real |
| resolution-and-interfaces.md §4.9 Name Resolution | hd_resolve (`ModuleScope`, prelude bindings and lookups), used by interface construction and checking | skeleton |
| resolution-and-interfaces.md §4.10 Folder Interface Construction | hd_resolve::iface builds the blob; `header_check` and derived heads remain incomplete | skeleton |
| resolution-and-interfaces.md §4.11 The Interface Blob | hd_resolve::iface codec, stable remapping, per-item hashes and deep hash; no zero-copy reader | skeleton |
| resolution-and-interfaces.md §4.12 Traits, Impls And Coherence | hd_resolve::orphan_ok, hd_check::stages::coherence | skeleton |
| checking-and-tir.md §4.13 Body Checking | hd_check::BodyCx over hd_types, the solver and hd_tir::TirBuilder; unsupported forms stop the build | skeleton |
| checking-and-tir.md §4.14 Diagnostics | every running stage uses hd_diag::DiagBuf and content-order assembly; some phases emit placeholder spans and the generated complete code enum is not wired in | skeleton |
| checking-and-tir.md §4.15 Limits | hd_base::Fuel is charged by the checker and solver; limit diagnostics remain incomplete | skeleton |
| type-checking.md §1.1 Who Owns What | hd_check, hd_types and hd_tir are wired, with unsupported cases | skeleton |
| type-checking.md §1.2 Inputs | hd_check::BodyCx reads hd_resolve items and the full green tree incompletely | skeleton |
| type-checking.md §1.3 Outputs | hd_tir::ir bodies plus hd_diag::DiagBuf, for the supported slice | skeleton |
| type-checking.md §1.4 The Type Accessor API | hd_types::pool (`get`, `intern_ty`), used by the partial checker | skeleton |
| type-checking.md §1.5 What The Checker Needs From The TIR Builder | hd_tir::ir (`TirSink`, `TirBuilder`) | real |
| type-checking.md §1.6 The Trait Solver Interface | hd_types::solver (`Solver`, `SolveCx`) | skeleton |
| type-checking.md §1.7 Body Tasks And The Exactly-Once Rule | hd_driver has one `Body(m)` task and one result slot per module; body-level parallel iteration is absent | skeleton |
| type-checking.md §2.1 Modes | hd_check::body (subset) | skeleton |
| type-checking.md §2.2 Expression Forms | hd_check::body (subset) | skeleton |
| type-checking.md §2.3 Statements And Blocks | hd_check::body (subset) | skeleton |
| type-checking.md §2.4 Calls And Use-Site Type Arguments | hd_check::body (subset) | skeleton |
| type-checking.md §2.5 Methods And Operators | hd_check::body (subset) | skeleton |
| type-checking.md §2.6 Closures | none | missing |
| type-checking.md §2.7 When Variables Are Resolved | none | missing |
| type-checking.md §2.8 Empty Collections | none | missing |
| type-checking.md §3.1 Variables | hd_types::unify (`VarKind`) | real |
| type-checking.md §3.2 Union-Find | hd_types::unify (`InferTable`) | real |
| type-checking.md §3.3 The Occurs Check | hd_types::unify (occurs check) | real |
| type-checking.md §3.4 No Levels, No Generalization | hd_types::unify | real |
| type-checking.md §3.5 The Trail And The One Rollback Contract | hd_types::unify (trail, `rollback`), hd_tir::ir (`checkpoint`) | real |
| type-checking.md §3.6 Literal Widths | hd_types::unify (literal kinds) | skeleton |
| type-checking.md §4.1 Coercion Sites And Order | none | missing |
| type-checking.md §4.2 Coercion Instructions | hd_tir::ir (`Coercion`) | skeleton |
| type-checking.md §4.3 Least Common Type | none | missing |
| type-checking.md §4.4 Propagation | none | missing |
| type-checking.md §5.1 Representation | hd_types::pool (`RowData`) | skeleton |
| type-checking.md §5.2 Available Keys, Providers And `$.use` | none | missing |
| type-checking.md §5.3 Call Checking | none | missing |
| type-checking.md §5.4 Closure Rows | none | missing |
| type-checking.md §5.5 Private Rows And The M3 Fixpoint | none | missing |
| type-checking.md §5.6 Row Patterns And Least Solutions | none | missing |
| type-checking.md §5.7 Suspension | none | missing |
| type-checking.md §5.8 The Direct `block_on` And `println` Ban | none | missing |
| type-checking.md §6.1 Arm-Local Equalities | none | missing |
| type-checking.md §6.2 Tuples, Varargs And Spreads | none | missing |
| type-checking.md §7.1 Pattern Typing | none | missing |
| type-checking.md §7.2 Usefulness | none | missing |
| type-checking.md §7.3 `let` Patterns And `let-else` | none | missing |
| type-checking.md §8.1 Access Types And Bindings | none | missing |
| type-checking.md §8.2 Mutation Checks | none | missing |
| type-checking.md §8.3 Local Flags | none | missing |
| type-checking.md §8.4 Definite Initialization | none | missing |
| type-checking.md §9.1 Who Can Omit | none | missing |
| type-checking.md §9.2 The M1 Walk | none | missing |
| type-checking.md §9.3 Cost | none | missing |
| type-checking.md §10.1 Errors Are Values | none | missing |
| type-checking.md §10.2 Poison | hd_types (poison unifies with all) | skeleton |
| type-checking.md §10.3 One Diagnostic Per Root Cause | none | missing |
| type-checking.md §10.4 Typed Holes | none | missing |
| type-checking.md §10.5 Fix-Its For Common Mistakes | none | missing |
| type-checking.md §11.1 What Counts | hd_base::Fuel | skeleton |
| type-checking.md §11.2 Running Out | none | missing |
| type-checking.md §11.3 Pathological Cases | none | missing |
| type-checking.md §12 Determinism And Parallelism | none | missing |
| type-checking.md §13 Checker Data Structures | hd_types::unify, hd_tir::ir | skeleton |
| type-checking.md §14.1 Spec Traceability | none | missing |
| type-checking.md §14.2 Test Kinds | none | missing |
| type-checking.md §15 Prototype Failures And The Rules That Prevent Them | none | missing |
| type-checking.md §16.1 Readings Of The Spec To Confirm | none | missing |
| type-checking.md §16.2 Inconsistencies Found | none | missing |
| type-checking.md §17 Changes Needed In COMPILER_DESIGN.md | none | missing |
| trait-solver.md §1.1 Who Owns What | hd_types::solver | skeleton |
| trait-solver.md §1.2 The Four Goals | hd_types::solver (`Goal`) | real |
| trait-solver.md §1.3 Entry Points | hd_types::solver (`Solver` trait) | skeleton |
| trait-solver.md §2.1 Trait References | hd_types::solver (`TraitRef`), pool (`Assoc`) | real |
| trait-solver.md §2.2 Canonical Goals | hd_types::solver (`CanonGoal`, `canonicalize`, `Scope`) | real |
| trait-solver.md §2.3 The Parameter Environment | hd_types::solver (`ParamEnv`, `elaborate`) | skeleton |
| trait-solver.md §3.1 Where Candidates Come From | hd_types::solver (`ImplTable::candidates`) | skeleton |
| trait-solver.md §3.2 Owner Modules | hd_types::solver (`ImplUniverseId`, `ImplUniverses`) | real |
| trait-solver.md §3.3 The Head Index | hd_types::solver (`HeadKey`) | real |
| trait-solver.md §3.4 Matching A Head | hd_types::solver (`MatchResult`) | skeleton |
| trait-solver.md §3.5 Committing | none | missing |
| trait-solver.md §3.6 Bounds As Subgoals | hd_types::solver (`PlanStep`) | skeleton |
| trait-solver.md §3.7 Supertraits | none | missing |
| trait-solver.md §3.8 What A Goal May Teach The Checker | none | missing |
| trait-solver.md §3.9 Compiler-Supplied Impls | none | missing |
| trait-solver.md §3.10 Derives, Delegation And `@error` | none | missing |
| trait-solver.md §3.11 The Synthetic Impl Inventory | none | missing |
| trait-solver.md §4.1 Where Projections Come From | none | missing |
| trait-solver.md §4.2 Elaboration Of The Environment | none | missing |
| trait-solver.md §4.3 Normalization: Lazy, At Three Points | none | missing |
| trait-solver.md §4.4 Projections In Impl Heads | none | missing |
| trait-solver.md §5.1 What Runs When | none | missing |
| trait-solver.md §5.2 The Overlap Check | hd_check::stages::coherence | skeleton |
| trait-solver.md §5.3 Why No Global Index | none | missing |
| trait-solver.md §5.4 What The Solver Assumes | none | missing |
| trait-solver.md §6.1 Depth First, On An Explicit Stack | hd_types::solver (`Frame`) | skeleton |
| trait-solver.md §6.2 The Search Graph | none | missing |
| trait-solver.md §6.3 Cycles | none | missing |
| trait-solver.md §6.4 Stalling | none | missing |
| trait-solver.md §6.5 Ambiguity | hd_types::solver (`Candidate`) | skeleton |
| trait-solver.md §7.1 Memo Keys And Eligibility | hd_types::solver (`MemoKey`, `GlobalMemo`, `BodyMemo`, `MemoEntry`) | real |
| trait-solver.md §7.2 Depth Is Charged From The Use | none | missing |
| trait-solver.md §7.3 Heights And Lower Bounds | none | missing |
| trait-solver.md §7.4 Fuel | hd_base::Fuel | skeleton |
| trait-solver.md §7.5 Limits And Their Diagnostics | none | missing |
| trait-solver.md §7.6 Bounded Is Not Linear | none | missing |
| trait-solver.md §8.1 What A `Holds` Answer Carries | hd_types::solver (`Evidence`, `BuiltinImpl`) | real |
| trait-solver.md §8.2 What TIR Records | hd_tir::ir (`ChoiceKind`, `Callee`) | real |
| trait-solver.md §8.3 What Codegen Does With It | hd_types::solver (`select`, `Selection`) | skeleton |
| trait-solver.md §9.1 Which Traits Can Be Values | none | missing |
| trait-solver.md §9.2 Vtable Shapes | none | missing |
| trait-solver.md §9.3 Trait Values As Self Types | none | missing |
| trait-solver.md §9.4 Conversion To A Trait Value | none | missing |
| trait-solver.md §10.1 `FailInfo` | hd_types::solver (`FailInfo`, `FailReason`, `NearMiss`) | real |
| trait-solver.md §10.2 Which Code | none | missing |
| trait-solver.md §10.3 Messages And Fix-Its | none | missing |
| trait-solver.md §10.4 One Diagnostic Per Root Cause | none | missing |
| trait-solver.md §11 Determinism And Parallelism | none | missing |
| trait-solver.md §12 Data Structures | hd_types::solver | skeleton |
| trait-solver.md §13 Performance Targets And Pathological Cases | none | missing |
| trait-solver.md §14.1 Spec Traceability | none | missing |
| trait-solver.md §14.2 Test Kinds | none | missing |
| trait-solver.md §15 Prototype Failures And The Rules That Prevent Them | none | missing |
| trait-solver.md §16.1 Questions For The Owner | none | missing |
| trait-solver.md §16.2 Readings Of The Spec To Confirm | none | missing |
| trait-solver.md §16.3 Inconsistencies Found | none | missing |
| trait-solver.md §16.4 Changes Needed In type-checking.md And The Other Design Files | none | missing |
| trait-solver.md §16.5 Codex Review | none | missing |
| cache.md §5.1 Where Things Live | hd_cache::DiskStore is used by hd_cli; the complete placement and lifecycle are partial | skeleton |
| cache.md §5.2 Entry Kinds | hd_cache::store (`EntryKind`) | real |
| cache.md §5.3 Key Composition | hd_cache has five of thirteen keys; `toolchain_key` lacks five fields and the package key is constant | skeleton |
| cache.md §5.4 Entry Format And Atomic Publish | the driver writes framed interface, check, code and link entries through `encode_entry`; DiskStore publishes by rename | real |
| cache.md §5.5 The Stat Manifest And Change Detection | hd_cache::store (`ManifestRecord`) | skeleton |
| cache.md §5.6 Verify Mode | hd_cache::store (`verify_entry`) | skeleton |
| cache.md §5.7 Eviction And The Size Cap | hd_cache::store (`shard_of`, `plan_eviction`) | real |
| cache.md §5.8 The Browser `CacheStore` | hd_cache::store (`CacheStore`, `MemoryStore`) | real |
| cache.md §5.9 What Cache I/O Costs | none | missing |
| scheduler.md §6.1 Tasks | hd_sched (`TaskKind`, `TaskGraph`), hd_driver (dynamic full graph and per-kind slots) | real |
| scheduler.md §6.2 Executors | one `Exec` over `&dyn Spawn` runs under serial, stepping and rayon pool executors with creation guards | real |
| scheduler.md §6.3 Priority | hd_sched (`SerialOrder::Priority`), hd_driver (folder heights); FIFO ignores the priorities | skeleton |
| scheduler.md §6.4 Budgets, Cancellation And The Memory Cap | hd_sched (`CancelFlag`), hd_driver (panic caught at the task boundary) | skeleton |
| scheduler.md §6.5 Deterministic Output Assembly | hd_diag::buf (`content_order`), hd_run::tests_model (`ReleaseCursor`) | skeleton |
| codegen.md §11.1 From TIR To A Running Program | the one driver runs Collect, Emit and Link for the supported scalar slice | skeleton |
| codegen.md §11.2 Stages | hd_driver, hd_wasm (subset) | skeleton |
| codegen.md §11.3 Tasks | hd_sched (`ExtTask`), hd_cache::prog_key | real |
| codegen.md §11.4 Crates | crates hd_mono, hd_host_abi, hd_wasm, hd_run, hd_run_wasmtime, hd_web | skeleton |
| codegen.md §12.1 Analysis, Then One Emission Walk | hd_wasm emits the supported forms from hd_tir::Body with precomputed layouts | skeleton |
| codegen.md §12.2 Lowering Rules | hd_wasm::emit (subset) | skeleton |
| codegen.md §12.3 Facts, Defaults, Derives And Tests | none | missing |
| codegen.md §12.4 Rows And Providers | none | missing |
| codegen.md §12.5 Counted Loops And Checks | none | missing |
| codegen.md §12.6 Tiers And Optimizations | hd_mono::passes | skeleton |
| codegen.md §12.7 Emission-Time Checks | none | missing |
| codegen.md §12.8 Size Versus Speed Policy | none | missing |
| codegen.md §13.1 Roots | hd_mono::collect over the supported program items and TIR | skeleton |
| codegen.md §13.2 Collection | hd_mono::collect and layout::a1_class, incomplete for the full language | skeleton |
| codegen.md §13.3 Instance Keys | hd_mono::layout (`canon`, `instance_key`, `KeyArg`) | real |
| codegen.md §13.4 The Instantiation Depth Limit | hd_mono::layout (`MAX_DEPTH`, `MAX_CHAIN`) | skeleton |
| codegen.md §13.5 Dictionaries: Trait Values And GADT Evidence | none | missing |
| codegen.md §13.6 Tuples, Arity And `all!` | none | missing |
| codegen.md §13.7 Merging Byte-Identical Functions | none | missing |
| codegen.md §13.8 Code Entries | hd_cache::code_key, hd_wasm::Code (subset) | skeleton |
| codegen.md §13.9 What Instances Cost | none | missing |
| codegen.md §13.10 The Link Step | hd_wasm::link over the scalar slice; metadata and runtime parts remain incomplete | skeleton |
| suspension.md §14.1 Functions Per Suspending Body | hd_mono::suspend (`SuspendFn`) | skeleton |
| suspension.md §14.2 The State Machine | hd_mono::suspend (`StateMachine`, `plan_state_machine`) | skeleton |
| suspension.md §14.3 Lazy Frame Materialization | hd_mono::suspend (`FrameLayout`) | skeleton |
| suspension.md §14.4 Wakers And The Entry Driver | hd_run::drive (poll/wake loop) | skeleton |
| suspension.md §14.5 `all!` And `race!` | hd_mono::suspend (`JoinKind`) | skeleton |
| suspension.md §14.6 Cancellation And `defer` | hd_mono::suspend (`FrameState::Cancelled`) | skeleton |
| suspension.md §14.7 Hook Points | none | missing |
| suspension.md §14.8 Deadlock Detection | hd_run::drive (deadlock outcome) | skeleton |
| suspension.md §14.9 `block_on` | none | missing |
| wasm-layout.md §15.1 Layout Classes | hd_mono::layout (`LayoutClass`, `VALUE_BOUND`, slot sharing), consumed by hd_wasm | real |
| wasm-layout.md §15.2 Values | hd_mono::layout (`layout_of` for every type form), consumed by hd_wasm | real |
| wasm-layout.md §15.3 The Type Section | hd_wasm::link (subset type section) | skeleton |
| wasm-layout.md §15.4 Globals And Module Initialization | none | missing |
| wasm-layout.md §15.5 Panic Sites And Backtraces | none | missing |
| wasm-layout.md §15.6 The 2 KB Tiny Program | none | missing |
| wasm-layout.md §15.7 Emission | hd_wasm emits a partial form set from hd_tir::Body through hd_mono::layout_of | skeleton |
| wasm-layout.md §15.8 Deterministic Bytes | hd_testkit compares FIFO, priority, shuffled and 2/8-worker pool runs byte for byte; full-language coverage is absent | skeleton |
| runtime-and-host.md §16.1 Where Each Piece Lives | none | missing |
| runtime-and-host.md §16.2 Panics And Exit Codes | hd_run (`Outcome`, `PanicReport`) | skeleton |
| runtime-and-host.md §16.3 Allocation | none | missing |
| runtime-and-host.md §16.4 Metadata And The Import List | hd_wasm::meta (`RuntimeMeta`), hd_host_abi (`RUNTIME_MODULES`); `link` writes no `hd.runtime` section | skeleton |
| runtime-and-host.md §17.1 One ABI Description | hd_host_abi (`TABLE`, `PRELUDE_IMPORTS`, codecs); checking and emission consume it | real |
| runtime-and-host.md §17.2 Import Shapes | hd_host_abi (`imports_of`, `Handle`, `SlotState`) | real |
| runtime-and-host.md §17.3 The Exchange Buffer | none | missing |
| runtime-and-host.md §17.4 Encoding Structured Values | none | missing |
| runtime-and-host.md §17.5 Host Calls On wasmtime | none | missing |
| runtime-and-host.md §17.6 Host Calls In The Browser | none | missing |
| runtime-and-host.md §17.7 Capability Grants | hd_run (`Grants`, `check_grants`) | skeleton |
| runtime-and-host.md §17.8 Resource Limits | hd_run (`Limits`) | skeleton |
| runtime-and-host.md §17.9 The Embedding API | hd_run traits and driver plus hd_cli's Node-backed Engine used by `hd run` | real |
| runtime-and-host.md §17.10 The Generated JS Glue | hd_host_abi::generate_js_glue | skeleton |
| engines-and-test-runner.md §18.1 wasmtime Configuration | hd_run_wasmtime (`EngineConfig`) | skeleton |
| engines-and-test-runner.md §18.2 Compile Caches | none | missing |
| engines-and-test-runner.md §18.3 Instantiation | none | missing |
| engines-and-test-runner.md §18.4 The Browser Runner | hd_driver::node (V8 through Node) | skeleton |
| engines-and-test-runner.md §18.5 The Reserved Hot-Reload Table | none | missing |
| engines-and-test-runner.md §18.6 Measured Choices | none | missing |
| engines-and-test-runner.md §19.1 Building | hd_run::tests_model (`ProgramKey`) | skeleton |
| engines-and-test-runner.md §19.2 Listing Cases | hd_run::tests_model (`TestPlan`, `expand_rows`), hd_wasm::meta (`TestMeta`) | real |
| engines-and-test-runner.md §19.3 Running | hd_run::tests_model (`ReleaseCursor`) | skeleton |
| engines-and-test-runner.md §19.4 Property Tests, Panics And Timeouts | none | missing |
| engines-and-test-runner.md §19.5 Reports | hd_run::tests_model (`CaseResult`) | skeleton |
| engines-and-test-runner.md §19.6 Tests In The Browser | none | missing |
| commands.md §7.1 `hd check` (Package Mode) | hd_driver::architecture (stage report; no `hd check` command yet) | skeleton |
| commands.md §7.2 `hd check FILE` | none | missing |
| commands.md §7.3 `hd test` (Up To D2) | none | missing |
| commands.md §7.4 `hd test --affected` | none | missing |
| commands.md §7.5 `hd run`, `hd build` | hd_cli on the one driver, Node Engine and persistent DiskStore; language coverage and std remain incomplete | real |
| commands.md §7.6 `hd fmt`, `hd fix` | hd_fmt (`round_trip`, `format`) | skeleton |
| commands.md §7.7 `hd doc` | hd_doc (`render`) | skeleton |
| commands.md §7.8 `hd repl` | none | missing |
| commands.md §7.9 The Playground | hd_web (`WebSession`) | skeleton |
| commands.md §20.1 `hd test` | none | missing |
| commands.md §20.2 `hd run` And `hd FILE` | hd_cli uses the one driver, Node Engine and persistent cache | real |
| commands.md §20.3 `hd build` | hd_cli writes the one driver's deterministic Wasm and uses the persistent cache | real |
| commands.md §20.4 `hd app.wasm` | none | missing |
| commands.md §20.5 The REPL | none | missing |
| commands.md §20.6 The Playground | hd_web | skeleton |
| live-execution.md §1.1 REPLs | none | missing |
| live-execution.md §1.2 Durable Execution And Replay | none | missing |
| live-execution.md §2.1 What A Session Is | none | missing |
| live-execution.md §2.2 Checking One Input | none | missing |
| live-execution.md §2.3 Compiling And Linking One Input | none | missing |
| live-execution.md §3.1 The Options | none | missing |
| live-execution.md §3.2 Why Not Replace And Replay | none | missing |
| live-execution.md §3.3 Recommendation: Shadowing, With A Note | none | missing |
| live-execution.md §4.1 Testing The Framing | hd_run::journal (torn-tail test) | real |
| live-execution.md §4.2 What Is Recorded | hd_run::journal (`RowKind`, `request_digest`) | real |
| live-execution.md §4.3 Format | hd_run::journal (`append`, `read`, `crc32`) | real |
| live-execution.md §4.4 Keys And Identity | hd_run::journal | skeleton |
| live-execution.md §4.5 Where It Lives | none | missing |
| live-execution.md §4.6 Size And Growth | hd_run::journal (`BLOB_THRESHOLD`) | skeleton |
| live-execution.md §5.1 Deterministic Re-Execution | none | missing |
| live-execution.md §5.2 Divergence | none | missing |
| live-execution.md §5.3 Replay At A Different Code Version | none | missing |
| live-execution.md §5.4 Rebuilding A REPL Session | none | missing |
| live-execution.md §6.1 Model | none | missing |
| live-execution.md §6.2 Code Identity | none | missing |
| live-execution.md §6.3 Durability | none | missing |
| live-execution.md §6.4 What Cannot Be Resumed | none | missing |
| live-execution.md §6.5 Checkpoints (Later Still) | none | missing |
| live-execution.md §7.1 Panics | none | missing |
| live-execution.md §7.2 Cancellation: Ctrl-C While Waiting | none | missing |
| live-execution.md §7.3 Stops: Ctrl-C While Running | none | missing |
| live-execution.md §7.4 Interaction Summary | none | missing |
| live-execution.md §8.1 The Same Model In A Worker | none | missing |
| live-execution.md §8.2 Stop | none | missing |
| live-execution.md §8.3 Persistence | none | missing |
| live-execution.md §8.4 Share Links | none | missing |
| live-execution.md §8.5 Playground Run Without `main` | none | missing |
| live-execution.md §9.1 Input Latency | none | missing |
| live-execution.md §9.2 `long-session` | none | missing |
| live-execution.md §9.3 Replay Cost | none | missing |
| live-execution.md §9.4 Recording Cost | none | missing |
| live-execution.md §10.1 What The Framing Gets For Free Later | none | missing |
| live-execution.md §11.1 Spec | none | missing |
| live-execution.md §11.2 Design Files | none | missing |
| live-execution.md §12 Open Questions For The Owner | none | missing |
| tiering.md §1.1 Where Compile Time Goes In Other Compilers | none | missing |
| tiering.md §1.2 What A Pass Costs, Part By Part | none | missing |
| tiering.md §1.3 When Fusing Pays | none | missing |
| tiering.md §1.4 What This Costs On The 10k-Line Application | none | missing |
| tiering.md §2.1 Goal | none | missing |
| tiering.md §2.2 Candidates | none | missing |
| tiering.md §2.3 Winch | none | missing |
| tiering.md §2.4 Lazy Compilation | none | missing |
| tiering.md §2.5 Estimated Dev Compile Time | none | missing |
| tiering.md §2.6 Std In The Dev Tier (Later Option) | none | missing |
| tiering.md §3.1 Our Own TIR Passes | none | missing |
| tiering.md §3.2 Binaryen's Wasm GC Optimizations | none | missing |
| tiering.md §3.3 Embedding Binaryen, Or Not | none | missing |
| tiering.md §3.4 Cranelift And wasmtime In Release | none | missing |
| tiering.md §3.5 Estimated Release Compile Time And Gains | none | missing |
| tiering.md §4.1 What May Differ | none | missing |
| tiering.md §4.2 Rules For A Release Pass | none | missing |
| tiering.md §4.3 Testing Both Tiers | none | missing |
| tiering.md §5.1 Prior Art | none | missing |
| tiering.md §5.2 Which hd Tests Feel Poor Code | none | missing |
| tiering.md §5.3 Recommendation | none | missing |
| tiering.md §6.1 Units And Levels | hd_mono::passes (`Level`) | real |
| tiering.md §6.2 A Pass Declaration | hd_mono::passes (`PassSpec`, validator) | real |
| tiering.md §6.3 Pipelines As Data | hd_mono::passes (`DEV`, `OPTIMIZED`) | real |
| tiering.md §6.4 Keys And Determinism | hd_mono::passes (pipeline hash) | skeleton |
| tiering.md §6.5 Fuel And Limits | none | missing |
| tiering.md §6.6 Where The Named Passes Plug In | hd_mono::passes (`PASSES`) | skeleton |
| tiering.md §7.1 The Pipelines | none | missing |
| tiering.md §7.2 Spike 0c Additions | none | missing |
| tiering.md §8.1 Open Questions For The Owner | none | missing |
| tiering.md §8.2 Design Statements That Must Change If Adopted | none | missing |
