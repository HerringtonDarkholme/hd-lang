# New Compiler: Footprint

Part of the [compiler design](README.md).

Status: report, 2026-10-08, after the overnight compiler work
(`97ef2c30`). Phase 1, "make it
move", is complete. This footprint records
implementation coverage, not accepted language behavior. One row per
design-doc section: where its code is and how far it goes. **Real**: the
structure or algorithm the section designs is implemented and tested,
possibly over the subset. **Skeleton**: its types and entry points exist
with real signatures, and the work answers a structured "not
implemented". **Missing**: no code yet. Paths are crates under
`compiler/crates`. Unnumbered material is left out. Numbered sections
that hold no code by nature (prior art, estimates, owner questions,
change lists) count as missing, which is most of the missing rows of
type-checking.md, trait-solver.md, live-execution.md and tiering.md.
The original stage counts on `lib/std` are in
[skeleton-findings.md](skeleton-findings.md#architecture-skeleton-findings).
M4a checks all 37 std modules and 1,080 bodies to verified TIR. M4b
collects std, emits layout-driven Wasm and runs a representative program
on V8. M4d runs module initialization, suspension and `defer` programs.
M4c adds shared enum initialization, range loops, entry-error reporting
and end-to-end unit tests. The overnight work of 2026-10-08 adds closure-row
inference, the closure provider context, projections, embedded member
promotion, declaration anchors, `hd check`, `StrIndex`, shared cells and
the designed pool and unifier storage. A row is still only real for the
implemented subset: the known M4a through M4c gaps keep affected sections
partial.

## Totals

| Doc | Real | Skeleton | Missing | Sections |
| --- | --- | --- | --- | --- |
| [data-structures.md](data-structures.md) | 11 | 11 | 3 | 25 |
| [syntax.md](syntax.md) | 5 | 1 | 0 | 6 |
| [resolution-and-interfaces.md](resolution-and-interfaces.md) | 1 | 5 | 0 | 6 |
| [checking-and-tir.md](checking-and-tir.md) | 1 | 2 | 0 | 3 |
| [type-checking.md](type-checking.md) | 30 | 14 | 17 | 61 |
| [trait-solver.md](trait-solver.md) | 14 | 10 | 34 | 58 |
| [cache.md](cache.md) | 4 | 4 | 1 | 9 |
| [scheduler.md](scheduler.md) | 2 | 3 | 0 | 5 |
| [codegen.md](codegen.md) | 9 | 10 | 3 | 22 |
| [suspension.md](suspension.md) | 2 | 6 | 1 | 9 |
| [wasm-layout.md](wasm-layout.md) | 5 | 3 | 0 | 8 |
| [runtime-and-host.md](runtime-and-host.md) | 3 | 9 | 2 | 14 |
| [engines-and-test-runner.md](engines-and-test-runner.md) | 5 | 2 | 5 | 12 |
| [commands.md](commands.md) | 7 | 4 | 4 | 15 |
| [live-execution.md](live-execution.md) | 3 | 2 | 35 | 40 |
| [tiering.md](tiering.md) | 3 | 2 | 26 | 31 |
| all | 105 | 88 | 131 | 324 |

### M3 Corrections

M3 moves no row between categories. Name resolution now covers every std
header and use form, and interface bytes are deterministic across fresh
runs and executor orders. Those rows remain skeletons because private-name
diagnostics, trait defaults, zero-copy reads and declaration anchors are
missing. Trait coherence also remains a skeleton because its pairwise
implementation does not have the designed trie cost.

### M4a Corrections

Body checking moves checking-and-tir.md §4.13 from skeleton to real for
the std-backed subset. In type-checking.md, §1.1, §2.1 to §2.5, §3.6 and
§4.2 move from skeleton to real; §2.7, §2.8, §4.1, §4.3, §4.4, §5.2,
§5.3, §6.2, §7.1 to §7.3, §8.3 and §8.4 move from missing to real; and
§2.6, §5.4, §5.7, §8.1, §8.2, §10.1, §10.3 and §14.2 move from missing
to skeleton. Closure-row inference, full mutability enforcement and
`SuspRow` records keep their affected sections partial.

In trait-solver.md, §1.1, §3.1, §3.4, §3.6 and §7.4 move from skeleton
to real; §3.5, §6.4 and §14.2 move from missing to skeleton. The table
solver matches generic heads and recursively checks bound plans, but
projection, `Instantiations` and `Methods` goals remain unimplemented.
No other category changes. The footprint also corrects the scheduler
row: the unified executor does not catch panics at task boundaries.

### M4b Corrections

In codegen.md, §11.1, §11.2, §12.1, §12.2, §13.1 and §13.2 move from
skeleton to real for the V8-backed subset. Sections §12.4, §12.5,
§12.7, §13.5 and §13.6 move from missing to skeleton. The unsupported
inventory, incomplete code key and absent custom metadata keep the other
backend rows partial.

Suspension.md §14.9 moves from missing to skeleton for the reduced
poll-and-block implementation. In wasm-layout.md, §15.3, §15.7 and
§15.8 move from skeleton to real; §15.4 to §15.6 move from missing to
skeleton. Runtime-and-host.md §16.1, §16.3, §17.3 and §17.4 move from
missing to skeleton. Engines-and-test-runner.md §18.3 moves from missing
to real for Node/V8 instantiation, while §18.4 moves from skeleton to
missing because the browser runner itself has not landed.

### M4d Corrections

Codegen.md §12.3 moves from missing to skeleton for module storage and
simple group initialization. Section §13.6 moves from skeleton to real
for tuple layouts and the tested `all!` subset. Cross-module statement
ordering, shared enum initialization, counted `For` tags, entry-result
termination and reusable provider contexts remain gaps.

Suspension.md §14.1 and §14.6 move from skeleton to real for the corrected
frame hierarchy, generated body/poll/cancel functions, child cancellation
and tested `defer` ladders. Sections §14.2 to §14.5 remain skeletons:
M4d saves every local, uses guarded block lists, and has no waker objects
or wake masks. Sections §14.8 and §14.9 also remain skeletons; their debug
report and forbidden-context counter are absent. Section §14.7 stays
missing because normal emission drops `Hook` and no hook-enabled tier exists.

### M4c Corrections

Shared enum constructors, top-level interpolation, `ForRange` and concrete
entry `.Err` reports now run. Their sections stay in their prior categories
because multi-module initialization, map iteration, erased errors and the
remaining emission inventory are still partial.

Engines-and-test-runner.md §§19.1, 19.3 and 19.5 move from skeleton to
real for the unit-test subset. Section §19.4 moves from missing to skeleton:
`expect_panic` works, while properties and timeouts remain unsupported.
Commands.md §§7.3 and 20.1 move from missing to real for `hd test`, FILE,
filter, jobs, ordered reports and exit statuses. Per-root build-error
isolation, `hd.runtime` case metadata and the rest of P2-9 remain gaps.

### Overnight Corrections (D2j)

The overnight work (`c20e4ca6` to `97ef2c30`) moves eight sections and
extends several others without a category change.

Data-structures.md §3.3 Interners, §3.4 Types and §3.19 Per-Body Checker
Scratch move from skeleton to real: the pool has its 64 dedup shards,
per-thread read-through table and flat extra column; inference variables
live in a body-local hash-consed `LocalPool` with bit-31 identity; the
unifier has its rank column and trail-exact rollback. The local pool's
hash-consing and the typed list-items column are recorded deviations from
the design text.

Type-checking.md §2.6 Closures, §5.1 Representation and §5.4 Closure Rows
move from skeleton to real: the checker infers closure rows, splices row
parameters through call type arguments, solves least rows and checks row
subsumption.

Commands.md §7.1 `hd check` (Package Mode) moves from skeleton to real
and §7.2 `hd check FILE` from missing to real: `hd check` runs the one
driver through checking and coherence, with text and JSON output and
status 101 on failure.

Extended without a category change: resolution-and-interfaces.md §4.10 to
§4.12 (declaration anchors in a third interface section; coherence skips
orphan-rejected impls), checking-and-tir.md §4.14 (header-stage and
coherence findings resolve real spans), type-checking.md §3.6 (literal
suffixes, prefixes, ranges and per-spec defaults), codegen.md §12.2
(shared-cell captures, `StrIndex`, char and byte-string intrinsics) and
§12.4 (the closure provider context). Totals: real rises from 97 to 105,
skeleton falls from 95 to 88, missing falls from 132 to 131, for 324
sections.

## Sections

| Section | Code path | Status |
| --- | --- | --- |
| data-structures.md §3.1 IDs And Their Scopes | hd_base (`id!`, IDs, `NONE`) | real |
| data-structures.md §3.2 Stable Paths And Stable Hashing | hd_intern::paths (`PathTable`), hd_base::stable | real |
| data-structures.md §3.3 Interners | hd_intern (`ShardedInterner`, `PathTable`), hd_types::pool (64 dedup shards, per-thread read-through table, allocation-free probe) | real |
| data-structures.md §3.4 Types | hd_types::pool (`TyData`, `InternPool`, rows) plus body-local `LocalPool` with bit-31 indices and a 4-bit generation, hash-consed (a recorded deviation); the global pool rejects `HAS_INFER` | real |
| data-structures.md §3.5 Arenas And Lifetimes | none | missing |
| data-structures.md §3.6 The Poison Type | hd_types (`Ty::POISON`, `HAS_POISON`, unifier) | real |
| data-structures.md §3.7 Spans And Files | hd_base (`Span`) | real |
| data-structures.md §3.8 Diagnostic Records | hd_diag::buf (`DiagBuf`, renderers) | skeleton |
| data-structures.md §3.9 Data-Oriented Encoding | hd_base::append (`AppendVec`, `Col`, `Range32`), hd_types::pool | skeleton |
| data-structures.md §3.10 IR Abstraction Contracts | hd_tir::ir (verifier contract) | skeleton |
| data-structures.md §3.11 Tokens And Line Tables | hd_syntax::lexer (`TokenBuf`) | real |
| data-structures.md §3.12 Layout Cursor And Skim State | hd_syntax::layout (`LayoutCursor`); skim runs the full lexer, no skim state | skeleton |
| data-structures.md §3.13 The Green Tree, Its Wire Format And The JS Decoder | hd_syntax::green (`GreenTree`, wire form, generic `NodeRef`); no generated named Rust views or JS decoder | skeleton |
| data-structures.md §3.14 The Header Skeleton And The Item Index | hd_syntax::skim, parser (`ItemIndex`); no header tree, use list holds body lines | skeleton |
| data-structures.md §3.15 Name-Resolution Tables | hd_resolve (`ModuleScope`, `Binding`, `FolderExports`), used by the one driver but incomplete | skeleton |
| data-structures.md §3.16 The Folder Interface: In Memory And As A Blob | hd_resolve::iface (`FolderIface`, canonical codec, deep hash, declaration anchors in a third blob section outside every hash); decoded copies remain, with no zero-copy reader or private-name index | skeleton |
| data-structures.md §3.17 Impl Tables | hd_types::solver (`ImplTable`, `HeadKey`) | skeleton |
| data-structures.md §3.18 TIR | hd_tir::ir (`Body` columns), verifier and `wire` codec with stable TIR hash; the running path uses it | real |
| data-structures.md §3.19 Per-Body Checker Scratch | hd_types::unify (`InferTable` with the rank column, union by rank, trail-exact rollback) | real |
| data-structures.md §3.20 Cache Entries And The Manifest | the driver uses `encode_entry` and `decode_entry` for interface, check, code and link entries; the stat manifest and several sections remain absent | skeleton |
| data-structures.md §3.21 The Scheduler's Task Graph | hd_sched (`TaskGraph`, `TaskKind`, `Spawn`, creation guards), used by serial, stepping and rayon executors | real |
| data-structures.md §3.22 Codegen: The Instance Table And Code Entries | hd_mono::layout and collection, used by the driver for per-instance code entries | real |
| data-structures.md §3.23 Wasm Emission Buffers | hd_wasm (subset emitter) | skeleton |
| data-structures.md §3.24 Memory Budget | none | missing |
| data-structures.md §3.25 The Schema Language | none (the tag schema is a Rust macro in hd_tir::ir) | missing |
| syntax.md §4.1 Lexer | hd_syntax::lexer | real |
| syntax.md §4.2 Layout | hd_syntax::layout | real |
| syntax.md §4.3 Skim Mode And The Header Pass | hd_syntax::skim runs the full lexer and builds no header tree | skeleton |
| syntax.md §4.4 Parser And Green Tree | hd_syntax::parser and green, used by the one driver; M2 accepts the full non-reject fixture corpus and all 36 std files | real |
| syntax.md §4.5 Header Extraction And The API Text Hash | hd_syntax::skim (`api_text_hash`) | real |
| syntax.md §4.6 Item Index | hd_syntax::parser (`ItemIndex`) | real |
| resolution-and-interfaces.md §4.7 Discovery, Module Identity And Folders | hd_project (`ModuleTable::discover`, `SourceSet`, manifest), used by the one driver | skeleton |
| resolution-and-interfaces.md §4.8 Folder Graph | hd_project (`FolderGraph`: order, cycles, closures, heights), used by the one driver | real |
| resolution-and-interfaces.md §4.9 Name Resolution | hd_resolve (`ModuleScope`, use worklist, prelude bindings and lookups) resolves every std header; cross-folder private names remain indistinguishable from absent names | skeleton |
| resolution-and-interfaces.md §4.10 Folder Interface Construction | hd_resolve lowers every std header, `Universe::stage_b` runs and header findings carry declaration anchors; the checker completes implicit projection arguments, while derived heads and declared generic defaults remain incomplete | skeleton |
| resolution-and-interfaces.md §4.11 The Interface Blob | hd_resolve::iface codec round-trips canonical bytes across fresh runs and shuffled orders; anchors sit in a third blob section; no zero-copy reader or private-name index | skeleton |
| resolution-and-interfaces.md §4.12 Traits, Impls And Coherence | hd_resolve::Universe runs stage B and generic-head overlap, skipping orphan-rejected impls; coherence is pairwise rather than trie-based | skeleton |
| checking-and-tir.md §4.13 Body Checking | hd_check::BodyCx checks all 38 current std modules and more than 1,000 bodies through inference, rows, usefulness, initialization and verified hd_tir::Body output; non-std forms can remain unsupported | real |
| checking-and-tir.md §4.14 Diagnostics | every running stage uses hd_diag::DiagBuf and the generated complete Code enum; header-stage and coherence findings resolve real spans through declaration anchors; package-level paths still use placeholder spans | skeleton |
| checking-and-tir.md §4.15 Limits | hd_base::Fuel is charged by the checker and solver; limit diagnostics remain incomplete | skeleton |
| type-checking.md §1.1 Who Owns What | hd_check owns checking, hd_types owns inference and solving, and hd_tir owns construction and verification on the full std corpus | real |
| type-checking.md §1.2 Inputs | hd_check::BodyCx reads hd_resolve items and the full green tree incompletely | skeleton |
| type-checking.md §1.3 Outputs | hd_tir::ir bodies plus hd_diag::DiagBuf, for the supported slice | skeleton |
| type-checking.md §1.4 The Type Accessor API | hd_types::pool (`get`, `intern_ty`), used by the partial checker | skeleton |
| type-checking.md §1.5 What The Checker Needs From The TIR Builder | hd_tir::ir (`TirSink`, `TirBuilder`) | real |
| type-checking.md §1.6 The Trait Solver Interface | hd_types::solver (`Solver`, `SolveCx`) | skeleton |
| type-checking.md §1.7 Body Tasks And The Exactly-Once Rule | hd_driver has one `Body(m)` task and one result slot per module; body-level parallel iteration is absent | skeleton |
| type-checking.md §2.1 Modes | hd_check::body bidirectional infer/check paths over the std-backed subset | real |
| type-checking.md §2.2 Expression Forms | hd_check::expr carries every expression form used by std | real |
| type-checking.md §2.3 Statements And Blocks | hd_check::body carries std statements, structured exits, scopes and defer | real |
| type-checking.md §2.4 Calls And Use-Site Type Arguments | hd_check::call handles generic, default, named, vararg, suspending and function-value calls | real |
| type-checking.md §2.5 Methods And Operators | hd_check::call resolves inherent, trait and built-in methods and operators for std | real |
| type-checking.md §2.6 Closures | hd_check checks closure bodies and captures and infers closure requirement rows | real |
| type-checking.md §2.7 When Variables Are Resolved | hd_check postpones calls and bounds, defaults literal classes, then zonks body types once | real |
| type-checking.md §2.8 Empty Collections | hd_check::expr creates typed empty list and map values, including comprehension lowering | real |
| type-checking.md §3.1 Variables | hd_types::unify (`VarKind`) | real |
| type-checking.md §3.2 Union-Find | hd_types::unify (`InferTable`) | real |
| type-checking.md §3.3 The Occurs Check | hd_types::unify (occurs check) | real |
| type-checking.md §3.4 No Levels, No Generalization | hd_types::unify | real |
| type-checking.md §3.5 The Trail And The One Rollback Contract | hd_types::unify (trail, `rollback`), hd_tir::ir (`checkpoint`) | real |
| type-checking.md §3.6 Literal Widths | hd_types::unify literal classes plus hd_check defaulting and range diagnostics | real |
| type-checking.md §4.1 Coercion Sites And Order | hd_check::body and call apply coercions at arguments, results, arms and assignments | real |
| type-checking.md §4.2 Coercion Instructions | hd_check emits hd_tir::ir::Coercion records explicitly | real |
| type-checking.md §4.3 Least Common Type | hd_check joins branch and arm values for the supported forms | real |
| type-checking.md §4.4 Propagation | hd_check lowers `?` to variant switches, conversion and return | real |
| type-checking.md §5.1 Representation | hd_types::pool (`RowData`); row parameters ride call type arguments as `TyData::Row`, with substitution and resolution splicing | real |
| type-checking.md §5.2 Available Keys, Providers And `$.use` | hd_check::call and expr track available providers and emit `ProviderGet`/`With` | real |
| type-checking.md §5.3 Call Checking | hd_check checks written rows and reports `missing-requirement` | real |
| type-checking.md §5.4 Closure Rows | a closure without a clause infers its row from its body; calls solve the least row and function values check row subsumption | real |
| type-checking.md §5.5 Private Rows And The M3 Fixpoint | none | missing |
| type-checking.md §5.6 Row Patterns And Least Solutions | none | missing |
| type-checking.md §5.7 Suspension | hd_check checks bang calls and emits await tags, but writes no `SuspRow` side records | skeleton |
| type-checking.md §5.8 The Direct `block_on` And `println` Ban | none | missing |
| type-checking.md §6.1 Arm-Local Equalities | none | missing |
| type-checking.md §6.2 Tuples, Varargs And Spreads | hd_check checks tuple construction and access, vararg packing and positional spreads | real |
| type-checking.md §7.1 Pattern Typing | hd_check::pat checks binding, literal, tuple, data, enum, option and result patterns | real |
| type-checking.md §7.2 Usefulness | hd_check::pat runs usefulness for exhaustiveness and unreachable arms, then emits a decision tree | real |
| type-checking.md §7.3 `let` Patterns And `let-else` | hd_check::body and pat check let patterns and divergent let-else suites | real |
| type-checking.md §8.1 Access Types And Bindings | hd_check carries binding forms and rejects `mut` on primitives and tuples; transparent `mut` unification leaves `mutable-upgrade` and `redundant-let-mut` absent | skeleton |
| type-checking.md §8.2 Mutation Checks | local reassignment runs, but `mutable-receiver-required` and `readonly-argument-to-mutable-parameter` are absent | skeleton |
| type-checking.md §8.3 Local Flags | hd_check and TirBuilder record read, assigned, mutated and capture flags and finish capture modes | real |
| type-checking.md §8.4 Definite Initialization | hd_check::init combines top-level reads and calls and diagnoses direct read-before-initialization; dispatch edges remain absent | real |
| type-checking.md §9.1 Who Can Omit | none | missing |
| type-checking.md §9.2 The M1 Walk | none | missing |
| type-checking.md §9.3 Cost | none | missing |
| type-checking.md §10.1 Errors Are Values | hd_check emits diagnostics and carries poison/never through later checks; recovery is incomplete | skeleton |
| type-checking.md §10.2 Poison | hd_types (poison unifies with all) | skeleton |
| type-checking.md §10.3 One Diagnostic Per Root Cause | hd_check suppresses several poison and never cascades, without the full designed cause graph | skeleton |
| type-checking.md §10.4 Typed Holes | none | missing |
| type-checking.md §10.5 Fix-Its For Common Mistakes | none | missing |
| type-checking.md §11.1 What Counts | hd_base::Fuel | skeleton |
| type-checking.md §11.2 Running Out | none | missing |
| type-checking.md §11.3 Pathological Cases | none | missing |
| type-checking.md §12 Determinism And Parallelism | none | missing |
| type-checking.md §13 Checker Data Structures | hd_types::unify, hd_tir::ir | skeleton |
| type-checking.md §14.1 Spec Traceability | none | missing |
| type-checking.md §14.2 Test Kinds | hd_driver checker tests cover the std corpus, diagnostics and four golden TIR bodies; property, pathological and full determinism matrices remain | skeleton |
| type-checking.md §15 Prototype Failures And The Rules That Prevent Them | none | missing |
| type-checking.md §16.1 Readings Of The Spec To Confirm | none | missing |
| type-checking.md §16.2 Inconsistencies Found | none | missing |
| type-checking.md §17 Changes Needed In COMPILER_DESIGN.md | none | missing |
| trait-solver.md §1.1 Who Owns What | hd_types::solver owns table solving; hd_check supplies body-local state and consumes answers | real |
| trait-solver.md §1.2 The Four Goals | hd_types::solver (`Goal`) | real |
| trait-solver.md §1.3 Entry Points | hd_types::solver (`Solver` trait) | skeleton |
| trait-solver.md §2.1 Trait References | hd_types::solver (`TraitRef`), pool (`Assoc`) | real |
| trait-solver.md §2.2 Canonical Goals | hd_types::solver (`CanonGoal`, `canonicalize`, `Scope`) | real |
| trait-solver.md §2.3 The Parameter Environment | hd_types::solver (`ParamEnv`, `elaborate`) | skeleton |
| trait-solver.md §3.1 Where Candidates Come From | hd_types::solver (`ImplTable::candidates`) supplies sorted candidates from each closure table | real |
| trait-solver.md §3.2 Owner Modules | hd_types::solver (`ImplUniverseId`, `ImplUniverses`) | real |
| trait-solver.md §3.3 The Head Index | hd_types::solver (`HeadKey`) | real |
| trait-solver.md §3.4 Matching A Head | hd_types::solver recursively matches generic impl heads and learns bare goal arguments | real |
| trait-solver.md §3.5 Committing | TableSolver commits the first coherent matching head; detailed failure chains and all goal kinds remain absent | skeleton |
| trait-solver.md §3.6 Bounds As Subgoals | TableSolver substitutes a matched head into its plan and solves bound steps recursively | real |
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
| trait-solver.md §5.2 The Overlap Check | hd_resolve::Universe::overlaps, called by the driver's one coherence task; pairwise rather than trie-based | skeleton |
| trait-solver.md §5.3 Why No Global Index | none | missing |
| trait-solver.md §5.4 What The Solver Assumes | none | missing |
| trait-solver.md §6.1 Depth First, On An Explicit Stack | hd_types::solver (`Frame`) | skeleton |
| trait-solver.md §6.2 The Search Graph | none | missing |
| trait-solver.md §6.3 Cycles | none | missing |
| trait-solver.md §6.4 Stalling | TableSolver returns `Stalled` on open self structure; watch edges and the complete wake protocol remain checker-local | skeleton |
| trait-solver.md §6.5 Ambiguity | hd_types::solver (`Candidate`) | skeleton |
| trait-solver.md §7.1 Memo Keys And Eligibility | hd_types::solver (`MemoKey`, `GlobalMemo`, `BodyMemo`, `MemoEntry`) | real |
| trait-solver.md §7.2 Depth Is Charged From The Use | none | missing |
| trait-solver.md §7.3 Heights And Lower Bounds | none | missing |
| trait-solver.md §7.4 Fuel | TableSolver charges hd_base::Fuel at every recursive goal and returns `OutOfFuel` | real |
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
| trait-solver.md §14.2 Test Kinds | hd_types tests exact and generic head matching, bound plans and selection; the full pathological matrix remains | skeleton |
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
| scheduler.md §6.4 Budgets, Cancellation And The Memory Cap | hd_sched (`CancelFlag`); the unified driver has no task-boundary panic catch | skeleton |
| scheduler.md §6.5 Deterministic Output Assembly | hd_diag::buf (`content_order`), hd_run::tests_model (`ReleaseCursor`) | skeleton |
| codegen.md §11.1 From TIR To A Running Program | the one driver collects std and runs a representative layout-driven module on V8 | real |
| codegen.md §11.2 Stages | hd_driver runs Collect, per-instance Emit and Link with cache hits on a second build | real |
| codegen.md §11.3 Tasks | hd_sched (`ExtTask`), hd_cache::prog_key | real |
| codegen.md §11.4 Crates | crates hd_mono, hd_host_abi, hd_wasm, hd_run, hd_run_wasmtime, hd_web | skeleton |
| codegen.md §12.1 Analysis, Then One Emission Walk | hd_wasm emits each supported body in one walk using collection results and structural layouts | real |
| codegen.md §12.2 Lowering Rules | hd_wasm::emit covers data, value enums, lists, maps, closures with shared-cell captures, trait calls, matches, interpolation, string indexing and std println | real |
| codegen.md §12.3 Facts, Defaults, Derives And Tests | hd_wasm links bindings, shared enum constructors, synthesized test bodies and module init functions; facts, defaults, derives and multi-module statement ordering remain partial | skeleton |
| codegen.md §12.4 Rows And Providers | provider arguments, host-provider vtables, lexical `With` scopes and per-closure provider contexts emit; entry-top-level providers remain unsupported | skeleton |
| codegen.md §12.5 Counted Loops And Checks | `ForRange`, checks and the `defer` exit ladder emit; `ForMap` remains blocked on `MapIter` | skeleton |
| codegen.md §12.6 Tiers And Optimizations | hd_mono::passes | skeleton |
| codegen.md §12.7 Emission-Time Checks | the emitter consumes verified TIR but trusts record indexes and does not run the designed debug assertions | skeleton |
| codegen.md §12.8 Size Versus Speed Policy | none | missing |
| codegen.md §13.1 Roots | hd_mono::collect reaches the program, required std bodies, helpers, closures and vtables | real |
| codegen.md §13.2 Collection | hd_mono::collect follows calls across std, selects trait impls and maps body-less intrinsics for the supported subset | real |
| codegen.md §13.3 Instance Keys | hd_mono::layout (`canon`, `instance_key`, `KeyArg`) | real |
| codegen.md §13.4 The Instantiation Depth Limit | hd_mono::layout (`MAX_DEPTH`, `MAX_CHAIN`) | skeleton |
| codegen.md §13.5 Dictionaries: Trait Values And GADT Evidence | trait-value coercions and calls emit; vtables allocate per coercion, omit supertraits, and generic dyn methods remain unsupported | skeleton |
| codegen.md §13.6 Tuples, Arity And `all!` | tuple and vararg layouts plus generated intrinsic bodies emit; `all!` state machines run for the tested subset | real |
| codegen.md §13.7 Merging Byte-Identical Functions | none | missing |
| codegen.md §13.8 Code Entries | hd_wasm::Code has function/type relocations and stable bytes; its key omits the full dependency list, and sites/lines are absent | skeleton |
| codegen.md §13.9 What Instances Cost | none | missing |
| codegen.md §13.10 The Link Step | hd_wasm::link assigns structural types, imports, helpers, functions, literals and dev names; folding and hd custom sections remain absent | skeleton |
| suspension.md §14.1 Functions Per Suspending Body | hd_wasm::layout (`task_base`, `suspend_base`, `frame_of`) and hd_wasm::emit (`emit_suspending`) generate cold, body, poll and cancel functions | real |
| suspension.md §14.2 The State Machine | hd_wasm::emit (`plan`, `resume_list`) numbers states and resumes through `pc` guards; liveness and the designed `br_table` dispatch remain absent | skeleton |
| suspension.md §14.3 Lazy Frame Materialization | hd_wasm::emit allocates on first Pending and resumes children directly, but saves every local and never clears dead references | skeleton |
| suspension.md §14.4 Wakers And The Entry Driver | hd_run::drive plus hd_wasm `hd.poll`/`hd.wake` and a completed-handle table; waker objects, generations and reusable slots remain absent | skeleton |
| suspension.md §14.5 `all!` And `race!` | hd_wasm emits `all!` and the std-lowered race helper, but re-polls every unfinished child because wake masks are absent | skeleton |
| suspension.md §14.6 Cancellation And `defer` | hd_wasm::emit checks active/terminal flags, cancels children through `$Task`, and runs tested LIFO exit ladders; external host abort remains partial | real |
| suspension.md §14.7 Hook Points | none | missing |
| suspension.md §14.8 Deadlock Detection | hd_run::drive reports a bare deadlock outcome; the debug frame-tree report is absent | skeleton |
| suspension.md §14.9 `block_on` | hd_wasm emits the poll/block loop; the forbidden-context counter for indirect `block_on` and `println` is absent | skeleton |
| wasm-layout.md §15.1 Layout Classes | hd_mono::layout (`LayoutClass`, `VALUE_BOUND`, slot sharing), consumed by hd_wasm | real |
| wasm-layout.md §15.2 Values | hd_mono::layout (`layout_of` for every type form), consumed by hd_wasm | real |
| wasm-layout.md §15.3 The Type Section | hd_wasm::link interns structural arrays, structs, subtypes and function types dependency-first | real |
| wasm-layout.md §15.4 Globals And Module Initialization | hd_wasm links lazy literal globals, binding storage and reachable group init functions; constant vtable globals and multi-module statement ordering remain absent | skeleton |
| wasm-layout.md §15.5 Panic Sites And Backtraces | panic stubs emit, but no `hd.sites`, `hd.lines`, `hd.folds` or symbolized backtrace metadata is linked | skeleton |
| wasm-layout.md §15.6 The 2 KB Tiny Program | hello measures about 4.8 KB with dev names; Q14 owns the section and reachability accounting | skeleton |
| wasm-layout.md §15.7 Emission | hd_wasm emits a representative program through structural layouts and validates the linked module on V8 | real |
| wasm-layout.md §15.8 Deterministic Bytes | the exit program is byte-identical across both serial orders and pool execution, cold and warm | real |
| runtime-and-host.md §16.1 Where Each Piece Lives | hd_wasm generates reached helpers and hd_cli supplies the Node host; the complete runtime split is partial | skeleton |
| runtime-and-host.md §16.2 Panics And Exit Codes | hd_run (`Outcome`, `PanicReport`), virtual `std.rt` and hd_wasm entry wrappers; concrete `.Err` results exit 1, while erased-error emission remains absent | skeleton |
| runtime-and-host.md §16.3 Allocation | layout-driven structs, arrays, boxes, closures and per-coercion vtables allocate in emitted Wasm; budgets and stats hooks remain absent | skeleton |
| runtime-and-host.md §16.4 Metadata And The Import List | hd_wasm::meta (`RuntimeMeta`), hd_host_abi (`RUNTIME_MODULES`); test cases pass directly from driver to CLI and `link` writes no `hd.runtime` section | skeleton |
| runtime-and-host.md §17.1 One ABI Description | hd_host_abi (`TABLE`, `PRELUDE_IMPORTS`, codecs); checking and emission consume it | real |
| runtime-and-host.md §17.2 Import Shapes | hd_host_abi (`imports_of`, `Handle`, `SlotState`) | real |
| runtime-and-host.md §17.3 The Exchange Buffer | hd_wasm exports linear memory and generated host stubs copy supported values; the full buffer protocol remains partial | skeleton |
| runtime-and-host.md §17.4 Encoding Structured Values | generated host-provider stubs encode the supported Console path; the complete codec table is not implemented | skeleton |
| runtime-and-host.md §17.5 Host Calls On wasmtime | none | missing |
| runtime-and-host.md §17.6 Host Calls In The Browser | none | missing |
| runtime-and-host.md §17.7 Capability Grants | hd_run (`Grants`, `check_grants`) | skeleton |
| runtime-and-host.md §17.8 Resource Limits | hd_run (`Limits`) | skeleton |
| runtime-and-host.md §17.9 The Embedding API | hd_run traits and driver plus hd_cli's Node-backed Engine used by `hd run` | real |
| runtime-and-host.md §17.10 The Generated JS Glue | hd_host_abi::generate_js_glue | skeleton |
| engines-and-test-runner.md §18.1 wasmtime Configuration | hd_run_wasmtime (`EngineConfig`) | skeleton |
| engines-and-test-runner.md §18.2 Compile Caches | none | missing |
| engines-and-test-runner.md §18.3 Instantiation | hd_cli's Node engine compiles, instantiates and runs the std-using exit module cold and warm | real |
| engines-and-test-runner.md §18.4 The Browser Runner | none; M4b's V8 engine is Node, not the browser worker | missing |
| engines-and-test-runner.md §18.5 The Reserved Hot-Reload Table | none | missing |
| engines-and-test-runner.md §18.6 Measured Choices | none | missing |
| engines-and-test-runner.md §19.1 Building | hd_driver `Goal::Tests`, test-role checks and one unit-test program; per-root build-error isolation remains absent | real |
| engines-and-test-runner.md §19.2 Listing Cases | hd_run::tests_model (`TestPlan`, `expand_rows`), hd_wasm::meta (`TestMeta`) | real |
| engines-and-test-runner.md §19.3 Running | Node workers run fresh instances per case; hd_run::tests_model `ReleaseCursor` publishes in content order | real |
| engines-and-test-runner.md §19.4 Property Tests, Panics And Timeouts | `expect_panic` is judged from stderr categories; properties and timeouts remain unsupported | skeleton |
| engines-and-test-runner.md §19.5 Reports | hd_cli `Report` and hd_run::tests_model `CaseResult`, tested cold, warm and across job counts | real |
| engines-and-test-runner.md §19.6 Tests In The Browser | none | missing |
| commands.md §7.1 `hd check` (Package Mode) | hd_cli::check_cmd over the one driver, text and `--format json`, status 101 on failure | real |
| commands.md §7.2 `hd check FILE` | hd_cli::check_cmd checks one FILE as a single-file program | real |
| commands.md §7.3 `hd test` (Up To D2) | hd_cli `test_cmd` over the one driver, with FILE, filter and jobs | real |
| commands.md §7.4 `hd test --affected` | none | missing |
| commands.md §7.5 `hd run`, `hd build` | hd_cli on the one driver, Node Engine and persistent DiskStore; language coverage and std remain incomplete | real |
| commands.md §7.6 `hd fmt`, `hd fix` | hd_fmt (`round_trip`, `format`) | skeleton |
| commands.md §7.7 `hd doc` | hd_doc (`render`) | skeleton |
| commands.md §7.8 `hd repl` | none | missing |
| commands.md §7.9 The Playground | hd_web (`WebSession`) | skeleton |
| commands.md §20.1 `hd test` | hd_cli `test_cmd` runs unit cases and prints ordered reports and statuses | real |
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
