# New Compiler: Code And Design Reconciliation

Part of the [compiler design](README.md).

Status: review, 2026-10-07, after M4d (`b756387e`). This report records
implementation fit, not accepted language behavior. It compares
`compiler/crates` with the design docs of this folder and records the
authorized corrections. Where the code is right and a doc is wrong, the
verdict says "design wrong". Earlier
findings are cited by ID (SK-1 to SK-15, SK-N1 to SK-N16) from
[skeleton-findings.md](skeleton-findings.md).

M1 made the designed architecture move end to end. M2 replaced the
heuristic parser with one recursive-descent parser. M3 resolves and lowers
every std header, builds canonical interfaces for all three std folders,
and makes their prelude types available to programs. M4a carries all 37
std modules and their 1,080 bodies through checking, verified TIR, row
checks, usefulness and definite module initialization. The one driver
uses the full parser, `hd_project`, `hd_resolve`, `hd_types`,
`hd_tir::Body`, every executor, `CacheStore`, `hd_mono` and `hd_wasm`.
M4b carries a std-using program through collection, layout-driven Wasm,
link and V8, with warm cache hits and deterministic bytes across serial
and pool executors. M4d adds module storage and initialization, suspension
state machines, `all!`, cancellation and the `defer` exit ladder. The
remaining findings are feature coverage and incomplete cache/runtime
details, not a second compiler pipeline.

## Summary: The Top 10

Ranked by how much each blocks the next working language slice.

1. **Body checking covers std but still has structural gaps.** `mut`
   is transparent to unification, closure rows are not inferred, dispatch
   is absent from initialization facts, and suspension side records are
   never populated. Shared enum initialization, top-level interpolation
   and counted-loop tags also fail before or during emission. Language
   forms outside the std corpus can still stop with `NotImplemented`.
2. **The standard-library pack is not writable.** M4a produces checked,
   verified std TIR, but `hd_stdpack::build_pack` still has no pack writer.
3. **Interfaces omit diagnostic and top-level-binding fidelity.** They
   have no item kind for ordinary top-level bindings, intentionally making
   those bindings module-local. They also have no private
   name index or declaration anchors. Cross-folder private uses therefore
   say `unknown-import`, while stage-B and coherence errors point at byte
   zero of the file.
4. **Trait defaults and coherence use temporary shortcuts.** Omitted trait
   arguments do not take declaration defaults, and coherence compares
   heads pairwise instead of using the designed discrimination tries.
5. **Cache framing is real, but the cache model is partial.** Interface,
   check, code and link boundaries use `CacheStore` and framed entries;
   `hd_cli` opens `DiskStore`. The stat manifest, several entry kinds and
   key fields remain absent, a program miss decodes every module, and
   M1 over-invalidates both interface dependencies and a body moved by a
   header edit.
6. **Skim still lexes whole files and over-collects uses.** It has no
   header tree or skim-mode lexer and can treat identifier-led body lines
   as imports. The one driver filters the result, preserving the old cost
   and correctness risk.
7. **Generated typed views and wire decoders remain hand-written or
   absent.** The parser has generic `NodeRef` accessors, but `hd.ungram`,
   named Rust views, the schema generator and the JavaScript decoder do
   not exist.
   Browser consumers still lack the designed stable generated surface.
8. **Emission now runs init, suspension and cleanup programs, but the long
   tail is large.** Layout-driven Wasm covers module globals, state
   machines, `all!`, cancellation, provider scopes and `defer`, besides
   the M4b forms. Defaults, reusable contexts, several control-flow and
   collection tags, panic metadata and advanced erased calls remain
   unsupported or use phase-1 shortcuts.
9. **Task-boundary panic isolation regressed.** The old architecture path
   caught panics; the unified `Exec` calls tasks directly under serial and
   rayon executors. One task panic can unwind the build instead of becoming
   the designed internal diagnostic.
10. **Stepping exists but the browser does not use it.** `hd_web` calls
    `analyze_package` as one slice, while `SteppingScheduler` counts tasks,
    not body cursors. The CLI also lacks a check-only path.

## Findings

Verdicts: **gap** (the design has it, the code does not, or only as a
stub); **impl wrong** (the code took a shortcut or misread the design);
**design wrong** (the design is unworkable, contradicts another doc, or
missed a case); **duplicate** (two code paths for one design thing);
**both ok** (both work; the row picks one).

### Duplicate Paths

| Design thing | M1 survivor | Status | Remaining split |
| --- | --- | --- | --- |
| driver | `hd_driver::build` and `analyze_package` over one `Run` | fixed | none |
| parser | `hd_syntax::parse` | fixed | M2 accepts the full corpus; no subset parser remains |
| discovery and graph | `hd_project::{ModuleTable, FolderGraph, SourceSet}` | fixed | none |
| resolution and interfaces | `hd_resolve::{ModuleScope, FolderIface}` | fixed | `hd_iface` was deleted |
| IDs, symbols and types | `hd_base` IDs, `ShardedInterner`, `InternPool` | fixed | the body-local type pool remains absent |
| TIR | `hd_tir::Body`, verifier and wire codec | fixed | none |
| cache bytes and store | framed entries through `CacheStore`; `DiskStore` in `hd_cli` | fixed | manifest and entry coverage remain incomplete |
| task results and executors | per-kind `OnceLock` slots; one `Exec` over `Spawn` | fixed | browser stepping remains unused |
| diagnostics | generated `Code` and `DiagBuf` through the running pipeline | fixed | declaration anchors are absent from interfaces |
| collection and layouts | `hd_mono::collect`, instance keys, `layout_of` | fixed | solver feature coverage remains partial |
| host imports | `hd_host_abi` tables consumed by checking and emission | fixed | runtime metadata is not linked |
| pipeline identity | `hd_mono::passes::DEV` hash | fixed | optimized passes remain later work |
| program runner | `hd_run::Engine`, with Node implemented in `hd_cli` | fixed | wasmtime is still a stub |

### M1 Findings

| Finding | Code evidence | Design result | Status |
| --- | --- | --- | --- |
| 1. TIR hash excludes locations, but a header edit re-emitted a later unchanged body | `hd_tir::wire::{write_body, tir_hash}` puts `syn` and `local_syn` after the hashed prefix | §4.13.11 and cache §5.3 now require location-only shifts to preserve the hash and reuse code | hash boundary fixed; reuse path gap |
| 2. Constants are per-body rows behind `Ref::konst` | `hd_tir::Body::consts`, `Ref::konst` | data structures and TIR now name the per-body `(Ty, u64)` table | fixed |
| 3. An `If`/`Match`/`Loop` owns its child blocks | `TirBuilder::close_block`, verifier ownership map | §4.13.11 now forbids also listing an owned block in its parent's list and makes this verifier invariant 2 | implementation gap |
| 4. Exhausting exact heads is a semantic failure | `hd_types::solver` returns `FailReason::NoImpl` | trait-solver §3.5 now says exact exhaustion is `Fails(NoImpl)`, never a stall | fixed |
| 5. `deep_hash` folds all used folders | `hd_resolve::iface::deep_hash`, driver reach hashes | cache §5.3 and interface §4.11.3 retain `mentions(F)` as the intended dependency set and name the current over-invalidation | implementation gap |
| 6. Four diagnostic paths use sentinel spans | driver folder-cycle, overlapping-impl, missing-entry-point and unsupported paths | checking §4.14 now requires a real primary source span for every diagnostic and names these four gaps | implementation gap |

### M2 Findings

| Finding | Code evidence | Decision | Side that changes |
| --- | --- | --- | --- |
| M2 gap 1. Layout is stored in side columns, not as ordinary zero-width tree tokens | `GreenTree::{layout_at, layout_kind}` | Keep the compact side columns as the tree's zero-width layout positions | design wording changed in syntax §4.2 and data structures §3.12 to §3.13 |
| M2 gap 2. A next-line `else:` attaches after a same-line `if` body | `Parser::at_else` consumes the one pending newline | This is accepted by the same-line-suite rules and keeps the construct contiguous | design clarified in syntax §4.2; implementation stays |
| M2 gap 3. Typed views are hand-written rather than generated | only generic `NodeRef` accessors exist; no `hd.ungram` exists | Generated named Rust and JavaScript views remain the stable tool boundary | implementation changes in the codegen task; design stays |
| M2 gap 4. Skim is a line scanner, not item parsing with `BodyPolicy::Skip` | `hd_syntax::skim::{body_ranges,use_ranges}` | One grammar path is required for exact headers and uses | implementation changes; design stays |
| M2 gap 5. Progress guards and a nesting bound replace parser fuel | `Parser::statements`, `MAX_NESTING = 160`, `enter`, `skip_balanced` | Local progress guards cover stuck loops; the input limit emits `nesting-too-deep` | design replaces parser fuel; implementation stays |
| M2 gap 6. Recovery reports once per statement and suppresses fallout after an unclosed delimiter | `stmt_errored`, `unclosed_from`, `recover_line` | Keep bounded reporting so one broken construct does not create cascades | design specifies the bounds; implementation stays |

### M3 Findings

| Finding | Code evidence | Intended rule | Side that changes |
| --- | --- | --- | --- |
| M3 gap 1. The bootstrap inventory assigned parent modules to their parent directories and inferred a false std folder cycle | `ModuleTable::discover_all` applies `module.folder.parent-file`; M3 builds `std`, `std.testing` and `std.prelude` with no cycle | `testing.hd` shares folder `std.testing` with its child; `std.core` is virtual, and all six seeded modules are inventoried | `std-bootstrap.md` corrected; implementation stays |
| M3 gap 2. Frozen interfaces cannot distinguish a private declaration from an absent name | `Resolver::export` sees only public `FolderIface::exports` across folders | Store a diagnostic-only `private_names` index outside every semantic hash | implementation adds the section |
| M3 gap 3. Fixed prelude uses create folder edges not stated in the design | `Run::skim_now` adds `prelude_modules()` to every module except `std.core` | These edges are intended: all ordinary origins live in `std`; `std.prelude.testing` also reaches `std.testing` | design now states the rule |
| M3 gap 4. Written trait references retain only explicit positional arguments | `Lower::trait_value` interns `type_args` directly | Fill omitted trailing arguments from declaration defaults, substituting the bound or impl target for `Self` | implementation applies defaults during header lowering |
| M3 gap 5. Interface items have no declaration anchors | `Run::item_span` returns the module file at byte zero | Store `TokenAnchor` rows outside semantic hashes and retain their item row through stage B and coherence | implementation adds and consumes anchors |
| M3 gap 6. Coherence is pairwise per trait | `Universe::overlaps` unifies every later head with every earlier head | Keep the result and blame order, but replace quadratic enumeration with the designed ground and generic tries | implementation replaces the simplification |
| M3 gap 7. Two proposed parser follow-ups disagree with the grammar | `Parser::use_decl` already accepts `as`; `associated_type` accepts only the grammar's optional `= type` | `use a.b as c` remains accepted; `type Item < Eq = i32` remains a syntax error unless the spec changes | no parser extension; design records the grammar result |

### M4a Findings

| Finding | Code evidence | Intended rule | Side that changes |
| --- | --- | --- | --- |
| M4a gap 1. `TraitMethod` has no separate trait-argument field | `hd_tir::ir::Callee::TraitMethod`; `hd_check::call` appends the method variables after the trait arguments | One `targs` list stores trait declaration arguments first and method arguments second; the interface's trait generic count is the split point | checking-and-tir.md records the layout; implementation stays |
| M4a gap 2. Header projections reach the checker without their trait arguments | `hd_check::call::{with_assoc_args,normalize}` fills `Self::Out` from the call and extends a seeded bound projection before solving | Header lowering may leave these arguments implicit; checker instantiation fills them before normalization | type-checking.md records the header/checker boundary; implementation stays |
| M4a gap 3. Variant operations use indices, while remapped identities live in `extra` | `hd_tir::wire::id_words`; `NewVariant`, `SwitchTag` and `Payload` have no ID word | Variants use declaration indices; the IDs of `ProviderGet`, `ItemRef`, `GlobalGet`, `GlobalSet`, `DefaultCall` and `With` live in typed `extra` records because only those words are wire-remapped | checking-and-tir.md corrects the catalog; implementation stays |
| M4a gap 4. Two checker-recognized premises had no design home | `hd_check::call` recognizes typed `Field.fact`; `hd_check::body::new_ck` seeds `Structure` for template targets | A typed handle fact matches its explicit type to the member type without `Inspectable`; a `by Structure` template body checks with `target < Structure` | type-checking.md owns both rules; implementation stays |
| M4a gap 5. Top-level bindings have no interface item kind | `hd_resolve::iface::ItemData` has no binding variant; `hd_check::init::ModuleInit` owns them | Ordinary top-level bindings are module-local initialization state and cannot be named from another module | resolution-and-interfaces.md records the boundary; implementation stays |
| M4a gap 6. Intrinsic impl methods have no TIR body | `Run::body` skips body-less intrinsic-family methods; `hd_mono::Env::intrinsic` maps selected intrinsics | Collection maps a body-less intrinsic plus substituted self and trait arguments to its compiler-generated body; no absent TIR is loaded | codegen.md records collection behavior; implementation stays |
| M4a gap 7. Four designed checker results are absent | `InferTable` strips `mut` during unification; closure checking records captures but not inferred rows; `InitFacts` records direct calls and reads but not dispatch; `Body::susp` is never populated | Add `mutable-receiver-required`, `readonly-argument-to-mutable-parameter`, `mutable-upgrade` and `redundant-let-mut`; infer closure rows; include dispatch in init reachability; and write one `SuspRow` per suspension point | implementation gaps; the owning designs stay |

### M4b Findings

| Finding | Code evidence | Intended rule | Side that changes |
| --- | --- | --- | --- |
| M4b gap 1. Suspension is a poll reference only | `hd_wasm::layout::suspend_base`; intrinsic `block_on` loops over `call_ref` then calls `hd:rt.block` | `$Suspend_L` retains state, flags, driver, waker and cancellation behavior; the reduced form is only an M4b bridge | suspension.md records the implementation boundary |
| M4b gap 2. Vtables allocate at each coercion and omit supertraits | `hd_wasm::emit::coerce` emits method refs followed by `struct.new` | One constant global per `(type, trait reference)`, with direct-supertrait fields in declaration order | codegen.md and wasm-layout.md retain the intended rule |
| M4b gap 3. Every erased scalar is boxed | `hd_wasm::emit::erase` calls `box_of` for non-reference layouts | Integers use the `i31ref` fast path when representable and allocate a box only on a miss; floats remain boxed | wasm-layout.md records the gap |
| M4b gap 4. Code keys omit emission dependencies | `hd_cache::code_key` hashes pipeline, instance, TIR and one callee-representation hash | The key also includes read interface, layout, selected-impl, inline, inlined-body and literal dependencies from §13.8 | codegen.md and cache.md record the incomplete key |
| M4b gap 5. Link writes no panic/runtime metadata or folds | `hd_wasm::link` emits only the standard `name` custom section | Link writes `hd.sites`, `hd.lines`, `hd.folds` and `hd.runtime`, remaps offsets through folding and preserves panic sites | codegen.md and runtime-and-host.md record the boundary |
| M4b gap 6. Every reference local is nullable | `hd_wasm::asm::Asm::local` stores `vt.dflt()` and `get` narrows non-null logical refs | Language locals keep non-null types; only conditionally inactive slots use defaultable layouts | wasm-layout.md records the gap |
| M4b gap 7. Emission trusts TIR | `hd_wasm::emit` indexes instruction and record vectors directly | Debug and CI emission assert substituted types, selections, suspension cases and relocation membership; malformed input becomes a named internal error | codegen.md §12.7 records the missing checks |
| M4b gap 8. Hello is about 4.8 KB with dev names | M4b's linked module carries the standard `name` section and reachable formatting helpers | Keep the 2 KB target; attribute every section and reachable body before tuning or changing the design | wasm-layout.md records the measurement; Q14 supplies the accounting |
| M4b gap 9. An interpolation leaks a nested literal into its parts | `hd_check::expr::string_expr` advances syntax children while separately walking string-piece tokens | The outer interpolation owns one expression result; nested literals remain only inside that expression | type-checking.md records the traversal invariant; implementation changes |
| M4b gap 10. Many TIR and layout forms still stop emission | `hd_wasm::emit` and `layout` return structured `NotImplemented` for the listed forms | Extend the one layout-driven emitter; do not add a second lowering path | implementation gap; the design stays |

The **M4b gap 10** inventory is `GlobalGet`/`GlobalSet`; every `Await`
tag; `With`/`ContextNew`/`ContextFor`; `ItemRef`, `Is`, `CallHost`,
`DefaultCall`, `CopyData`, `SwitchStr`, the `For` tags and `Scope` with
`defer`; `MapRemove`, `MapIter` and `StrIndex`; `ToAny` and `Supertrait`;
f32 arithmetic and wider conversions; shared captures; generic methods
through `dyn`; and recursive types. M4d closes `GlobalGet`/`GlobalSet`,
the emitted `Await`, `AwaitValue` and `AwaitAll` forms, `With`, and
`Scope` with `defer`. `AwaitRace` remains unused because the checker
lowers `race!` through std hd.

### M4d Findings

| Finding | Code evidence | Intended rule | Side that changes |
| --- | --- | --- | --- |
| M4d gap 1. Frames save every local and never clear dead references | `hd_wasm::emit::{emit_suspending,save,reload}` learns every Wasm local in a first pass and stores the full set at every point | Suspension §14.2 computes liveness once; §14.3 saves live values and clears dead references | implementation changes in phase 3; design stays |
| M4d gap 2. Resume uses guarded block lists instead of one dispatch loop | `hd_wasm::emit::{plan,resume_list}` numbers points, then re-enters nested control flow through `pc` range tests | Suspension §14.2 uses one flattened `br_table` dispatch loop for linear code size | accepted phase-1 deviation; restore the designed dispatch in phase 3 |
| M4d gap 3. A caller cannot name the callee-private frame type | `hd_wasm::layout::{task_base,suspend_base,frame_of}` and `emit_suspending` expose `$Suspend_L`, cast to `$F_f` in the callee, and cancel children through `$Task` | `$Task` is the layout-independent cancel/state/flags prefix; `$Suspend_L` is public per result layout; `$F_f` is private | suspension.md §14.1 adopts the implementable hierarchy |
| M4d gap 4. Wake tracking has no waker objects or wake masks | `hd_wasm::rt::{wake_mark,wake_take}` stores completed handles; `await_all` re-polls every unfinished child | Suspension §14.4 uses generation-tagged reusable handle slots and wakers; §14.5 uses per-child wake masks | implementation gap; design stays |
| M4d gap 5. Four runtime safeguards are absent | `suspend_base` has no driver field; no forbidden-context counter, hook emission or frame-tree report exists | Suspension §14.3 checks competing drivers, §14.7 emits optional hooks, §14.8 reports debug wait trees, and §14.9 guards indirect `block_on`/`println` | implementation gaps; design stays |
| M4d gap 6. Module initialization covers only the simple group and row cases | `Run::group_init_order` rejects groups with statements in multiple modules; `entry` supplies providers only to `main` | Module Initialization orders statements across a group; `module.init.script-row` gives an entry module's top level its inferred providers | implementation gap; design stays |
| M4d gap 7. Four frontend or entry-result cases block follow-up coverage | shared enum constructors are absent from Init TIR; top-level interpolation reports `unknown-name`; every `for` lowers through `iter`/`next`; a `.Err` result from `main` maps to status 0 | Shared data initializes with its module; top-level reads resolve; codegen receives counted `For` tags; the entry wrapper maps `Result` failure to a nonzero outcome | implementation gaps; design stays |

### Findings Table

| Design section | Code path | Finding | Verdict | Proposed fix |
| --- | --- | --- | --- | --- |
| design-overview.md §1.1 to §1.3 | `TaskKind::Body(u32)`, one slot per module | The overview and M1 now agree on one scheduled body task per module | both ok | None |
| design-overview.md §1.2 (Coherence row), SK-12 | `TaskKind::Coherence`, `Run::coherence`, `Universe::overlaps` | Unit per trait in the overview; one task per run in scheduler.md §6.1 and cache.md §5.3. The code has one task, with no key | design wrong | Overview row: "traits whose `coh_key` changed, one task" |
| design-overview.md §1.2 (Cached column) | none | The overview names `hdr`, `coh` and `init` entries; cache.md §5.2 makes them parts of one `graph` entry | design wrong | Overview: "part of `graph`" in those three rows and in the §1.1 diagram |
| design-overview.md §1.3 (diagram), SK-4 | `Run::module_prep` creates parse and downstream tasks after misses | The dynamic graph now follows the corrected diagram | both ok | None |
| design-overview.md §2.1 (D2 crates row) | `hd_driver` depends downward on `hd_mono` and `hd_wasm` | The dependency direction now matches the corrected row | both ok | None |
| design-overview.md §2.1 (`hd_check`) | `BodyCx` uses `hd_types`, `hd_diag` and `hd_tir` | The duplicate `World` checker was deleted | both ok | None |
| design-overview.md §2.1 (`hd_tir`) | `hd_tir::Body` and wire codec only | Run-global IDs, types and impl tables were deleted from TIR | both ok | None |
| design-overview.md §2.2 rules 1 and 3 | `Host` takes sources, store, executor and clock; Node and disk live in `hd_cli` | The driver owns no host facility | both ok | None |
| design-overview.md §2.2 rule 2 | `hd_diag::Code`, generated `codes.rs` | The generated enum and phase metadata are the public diagnostic surface | both ok | Keep the generator wired through `hd_diag::codes` |
| design-overview.md §2.1, SK-11 | `hd_types::solver` | Solver in `hd_types`, as the overview says | both ok | Keep; trait-solver.md §1.1 should name the crate |
| data-structures.md §3.1 | `hd_base` IDs and `hd_types::Ty` | The duplicate `hd_tir::world` IDs were deleted | both ok | None |
| data-structures.md §3.3 | `hd_intern::ShardedInterner`, `hd_types::InternPool` | The string interner takes a global `append` mutex on every miss; the pool takes one global `Mutex<HashMap>` on every lookup, hit or miss, and allocates the key `Vec` first. The design has per-thread columns, 64 shards and a per-worker read-through table | impl wrong | One `ShardedInterner<C>` as §3.3 draws it, used by all three interners |
| data-structures.md §3.4 | `InferTable::fresh`, `InternPool::intern_ty` | Inference variables are interned into the global pool (the code says so); the body-local pool with bit 31 does not exist | gap | Body-local pool owned by the body task |
| data-structures.md §3.9.2, §3.9.4, §3.24 | audited unsafe `hd_base::AppendVec` | M1 implements dense never-moved chunks under the owner-approved exception and tests concurrent publication | both ok | Keep the unsafe surface confined to this module |
| data-structures.md §3.13, syntax.md §4.4 | `hd_syntax::green`, generic `NodeRef` | Wire form and compact tree exist; generated named Rust and JavaScript views do not | gap | Generate both view surfaces from `hd.ungram` with the playground task (M2 gap 3) |
| data-structures.md §3.14, syntax.md §4.3 | `hd_syntax::skim` | Skim runs the full lexer, keeps no header tree (SK-N4), and records every line that starts with an identifier, body lines included, as a use. The driver filters by the text `use ` | impl wrong | Skim-mode lexer that skips bodies; header tree; `use` lines only |
| data-structures.md §3.18, checking-and-tir.md §4.13.11 | `hd_tir::wire::{write_body, read_body, tir_hash}` | The surviving `Body` has stable remapping, round-trip tests and a content hash | both ok | None |
| data-structures.md §3.20.2, SK-N9 | `hd_tir::write_tables` per body | The design has entry-local tables; the code gives each body its own tables, so a TIR hash depends only on its body | design wrong | §3.20.2: tables per body inside the entry; the later per-body reuse of §4.13.1 needs the same |
| data-structures.md §3.20.4, cache.md §5.2 | `Run::module_finish` | The `check` entry has diagnostics, a meta hash, headers (SK-1) and TIR; no init summary, row results, facts, reads or `locs` section, no `EntryHeader` | gap | Write through `encode_entry`; add sections as their stages land |
| data-structures.md §3.21 | `hd_sched::TaskGraph` | `Vec` nodes, a `VecDeque` ready queue, `&mut` mutation; no atomics, no `AppendVec`, no creation guard | gap | Keep the serial graph; add the concurrent one when threads land |
| data-structures.md §3.21, scheduler.md §6.1 | `Spawn::add_held` and `release` | M1's creation guard keeps a dynamically wired task from starting before its edges exist | both ok | None |
| data-structures.md §3.22 | `hd_mono::Collected` and layout instance keys | The running collector uses the designed type pool and instance identity | both ok | None |
| scheduler.md §6.1 (results in slots) | `Run` has per-kind `OnceLock` vectors | Every task publishes once and readers wait through graph edges | both ok | None |
| scheduler.md §6.2, SK-13 | one `Exec` over `&dyn Spawn` | Serial, stepping and rayon executors run the same task body | both ok | None |
| scheduler.md §6.2 (pool) | rayon tasks around a mutex-protected graph | Work stealing and creation guards landed; draining ready work still scans the FIFO queue while holding the graph mutex | impl wrong | Give the pool an O(log n) or O(1) ready structure and measure contention |
| scheduler.md §6.2 (stepping) | `SteppingScheduler::run_for`, `hd_web::WebSession::check` | A step is one task, not one body; `hd_web` does not step at all ("one slice") | gap | `Body(m)` keeps a body cursor; `hd_web` calls `run_for` |
| scheduler.md §6.3 | `analyze_package` uses `SerialOrder::Priority`; caller selects the build executor | Serial priority is real, but the pool drains FIFO rather than priority order | impl wrong | Preserve priorities when publishing pool work |
| scheduler.md §6.4 | unified `Exec` calls tasks directly | The old architecture path's panic catch was deleted during consolidation | gap | Catch unwind at the executor boundary and emit one internal diagnostic |
| scheduler.md §6.2 rule 4, codegen.md §11.1, §11.3 | `Run::collect`, `ExtTask::Emit(u32)` | §11.3 says one `Emit` per code-entry miss; §6.2 rule 4 says batches per module group; the §11.1 diagram says per folder group. The code makes one task per instance, hits included | design wrong | One rule in both docs: `Emit(group)` per folder group with a miss (the `codepack` unit); hits are looked up in `Collect` |
| syntax.md §4.4, build-order.md slice 1 | `hd_syntax::parser` | M2's one recursive-descent parser accepts every non-reject fixture and all 36 standard-library files | both ok | Keep the corpus, snapshots and mutation tests |
| syntax.md §4.4 (progress and depth) | `Parser::{statements,enter,skip_balanced}`, `MAX_NESTING` | Progress guards and `nesting-too-deep` cover the two failure classes without a second fuel mechanism | design wrong | Replace parser fuel with the implemented progress and depth rules (M2 gap 5) |
| syntax.md §4.4 (recovery) | `stmt_errored`, `unclosed_from`, `recover_line` | Reporting stops after one error per statement and suppresses fallout past an unclosed delimiter | design wrong | State these cascade bounds explicitly (M2 gap 6) |
| resolution-and-interfaces.md §4.7, §4.8 | `ModuleTable::discover_all`, `FolderGraph` | M3 applies the parent-file rule and builds the three-folder std graph; fixed prelude uses intentionally add edges to `std` | both ok | Keep the M3 gap 1 and 3 corrections |
| resolution-and-interfaces.md §4.9 | `hd_resolve::ModuleScope`, export worklist and prelude bindings | All std header uses resolve, but a frozen interface has no private-name index, so cross-folder private uses report `unknown-import` | gap | Add `private_names` outside semantic hashes (M3 gap 2) |
| resolution-and-interfaces.md §4.10, §4.10.1 | `hd_resolve::{lower,header,iface}`; `hd_check::call::{with_assoc_args,normalize}` | M3 lowers every std header and runs stage B without `NotImplemented`; M4a deliberately completes implicit projection arguments at checker instantiation. Derived heads and declared generic defaults remain incomplete | gap | Keep projection completion at the checker boundary; apply declaration defaults during lowering and complete derived heads (M3 gap 4, M4a gap 2) |
| resolution-and-interfaces.md §4.11 | `hd_resolve::iface` codec and deep hashes | Canonical blobs round-trip across fresh runs and shuffled orders; decoded copies, private names and declaration anchors remain gaps | gap | Add the indexed reader, `private_names` and `anchors` (M3 gaps 2 and 5) |
| resolution-and-interfaces.md §4.12, trait-solver.md §5.2 | `hd_resolve::Universe::{stage_b,overlaps}` | Stage B and generic-head overlap run, but coherence enumerates pairs instead of the designed tries | gap | Replace pairwise enumeration with the ground and generic tries (M3 gap 6) |
| checking-and-tir.md §4.13.1 (M1 to M4a) | `Run::{module_prep,body,module_finish}`, `hd_check` | All 37 std modules and 1,080 bodies check to verified TIR; written rows, usefulness and definite module initialization run. Omitted-result M1, omitted-row M3 and body-level parallel iteration remain gaps | gap | Complete the two inference phases and batch bodies without adding another checker |
| checking-and-tir.md §4.14 | generated `Code`; per-stage and per-module `DiagBuf` | Structured diagnostics and the complete code enum are live; interface findings and three package-level paths still lack primary source sites | gap | Retain declaration anchors and real package-level primary spans (M3 gap 5) |
| checking-and-tir.md §4.15, trait-solver.md §7.4 | `BodyCx::charge` and solver fuel | Fuel is charged; exhaustion still reports internal `unsupported` instead of the specified limit code | gap | Emit the generated limit diagnostic |
| type-checking.md §1.4 to §1.6 | `BodyCx` over `InternPool`, `TirBuilder` and `TableSolver` | The interfaces carry the complete std corpus; closure-row inference, mutability checks and suspension side records remain absent | both ok | Extend feature coverage without another checker (M4a gap 7) |
| trait-solver.md §3.1 to §3.6 | `TableSolver`, called by `BodyCx` | Generic impl-head matching and recursive bound plans are live; projection, `Instantiations` and `Methods` goals remain outside the solver | gap | Move the checker-local shortcuts behind the wired solver interface |
| cache.md §5.3 | `toolchain_key`, package, interface, check, program and code keys | The package key now uses the manifest name and the pipeline hash is real; seven designed key families and several toolchain fields remain absent | gap | Add fields and keys as their stages land |
| cache.md §5.2 (packs) | `Run::emit`, `EntryKind::Code` on DiskStore | One disk entry is written per instance, though the design makes code entries sections of folder-group codepacks | impl wrong | Pack misses per folder group at `Link` |
| cache.md §5.5 | `hd_cache::store::ManifestRecord` | The record type exists; no stat manifest is read or written | gap | With the disk store (slice 4) |
| codegen.md §11.2, §11.3, SK-N12 | `Run::collect`, `Run::decode_pending` | `prog_key` lists every module, and a miss decodes every module's TIR, not only modules the root reaches | impl wrong | Reachable modules from the manifest's use lists, as §11.3 says |
| codegen.md §11.3 | `Run::package_result` | `Collect` runs after all of `PackageResult`, not after the `tir` entries and `HeaderCheck` tasks of reached folders only | both ok | Keep for one program; split when tests add programs |
| codegen.md §11.4 | `hd_run` depends on `hd_cache` | The design's crate table now records this dependency | both ok | None |
| codegen.md §12.1, §12.2 | `hd_wasm::emit` over `hd_tir::Body` and the structural layout engine | M4d emits representative init, suspension and cleanup programs through V8; the remaining gap-10 forms stay structured unsupported cases | both ok | Extend this emitter only |
| codegen.md §12.3 | `hd_wasm` binding globals and group init functions; `Run::group_init_order` | Module storage and simple dependency-first group initialization run; multi-module statement order and shared enum constructor initialization do not (M4d gaps 6 and 7) | gap | Emit the designed cross-module statement order and every module-owned initializer |
| codegen.md §12.4 | `hd_wasm::emit::{push_providers,with,entry}` | Concrete rows, default-profile providers and lexical `With` scopes emit; entry top-level providers and reusable contexts do not (M4d gap 6) | gap | Pass an entry module's inferred row to its init body and add context lowering |
| codegen.md §12.5 | `hd_wasm::emit::{scope,exit}` | The `defer` exit ladder runs through returns, breaks, continues and cancellation; counted `For` tags never reach emission (M4d gap 7) | gap | Make the checker emit the designed counted-loop tags |
| codegen.md §13.6 | `hd_wasm::emit::await_all`; generated race helper | Tuple layouts and `all!` run; `all!` re-polls every unfinished child because wake masks are absent (M4d gap 4) | gap | Add wake masks with the complete driver |
| codegen.md §13.2, SK-2 | `hd_check::body` (`rep_summary`), `A1Rule` | Bounded parameter is always exact, as SK-2 decided; codegen.md §13.2 was updated | both ok | None |
| codegen.md §13.3 | `hd_mono::layout::instance_key` | The duplicate `World`/`CTy` key path was deleted | both ok | None |
| codegen.md §13.8, SK-3 | `hd_cache::code_key` | Callee representation summaries are present, but interface, layout, selected-impl, inline, inlined-body and literal dependencies are absent | gap | Complete the dependency record before treating code-cache hits as sound (M4b gap 4) |
| wasm-layout.md §15.1, §15.2 | `hd_mono::layout::layout_of`, used by hd_wasm | The private four-form emitter layout was deleted | both ok | None |
| suspension.md §14.1 | `hd_wasm::layout::{task_base,suspend_base,frame_of}`; `emit_suspending` | Body, cold, poll and cancel functions now use the corrected `$Task`/`$Suspend_L`/`$F_f` hierarchy | both ok | Keep the public result-layout base and private callee frame (M4d gap 3) |
| suspension.md §14.2, §14.3 | `hd_wasm::emit::{plan,resume_list,save,reload}` | State machines and lazy frames run, but save all locals and use guarded lists rather than liveness and `br_table` dispatch | gap | Restore liveness, dead-reference clearing and the designed dispatch in phase 3 (M4d gaps 1 and 2) |
| suspension.md §14.4, §14.5 | `hd_wasm::rt::{wake_mark,wake_take}`; `emit::await_all` | Completed handles wake the root and joins run, but there are no waker objects, wake masks, generations or reusable slots | gap | Add the complete waker and handle-table design (M4d gap 4) |
| suspension.md §14.6 | `hd_wasm::emit::{flag_checks,cancel_task,scope,exit}` | Re-entrant and terminal-state checks, child cancellation and LIFO `defer` ladders run; external abort remains incomplete | both ok | Extend the implemented cancellation path as host operations land |
| suspension.md §14.7 to §14.9 | `hd_run::drive`; `hd_wasm` entry helpers | Basic deadlock outcome and poll/block paths exist; hooks, competing-driver checks, forbidden contexts and debug reports do not | gap | Add the four safeguards without a second driver (M4d gap 5) |
| wasm-layout.md §15.4 to §15.6 | `hd_wasm::{link,rt}` | Literal and module-storage globals, init functions, helper stubs and dev names exist; site metadata, constant globals and folds do not, and hello measures about 4.8 KB | gap | Add constant globals and metadata, then use Q14's attribution against the 2 KB target (M4b gaps 5 and 8) |
| runtime-and-host.md §16.2 | `hd_wasm::emit::entry`; `hd_cli::node` | Entry success and panic paths run, but a `main` result of `.Err` still exits 0 (M4d gap 7) | gap | Map the entry `Result` through the specified outcome rule |
| runtime-and-host.md §16.4 | `hd_wasm::meta::RuntimeMeta`, `hd_wasm::link` | The codec exists; link writes no `hd.runtime`, `hd.sites` or `hd.folds` section | gap | Write metadata and panic-site tables from the reached import and code sets (M4b gap 5) |
| runtime-and-host.md §17.1, §17.2 | `hd_host_abi::{TABLE, PRELUDE_IMPORTS}` | Checking and emission consume the single ABI description | both ok | None |
| runtime-and-host.md §17.9 | `hd_run::{Engine, run_program}`, `hd_cli::node` | Node implements the embedding API and `hd run` uses it; wasmtime remains an approved-later stub | both ok | Add wasmtime after approval |
| engines-and-test-runner.md §18.4, build-order.md §22 | `hd_cli::node`, `hd_web` | Node is correctly behind `Engine`; the browser worker and glue remain incomplete | gap | Implement the browser Engine and worker in slice 9 |
| testing-the-compiler.md §8.1, wasm-layout.md §15.8 | `hd_testkit` | FIFO, priority, shuffled and 2/8-worker pool builds produce byte-identical Wasm | both ok | None |
| commands.md §7.1 | none | No `hd check` command; `hd_cli` has `run` and `build` | gap | Add `hd check` on the single driver |
| commands.md §7.5, §20.2, §20.3 | `hd_cli` | `hd run` and `hd build` use the single driver, Node Engine and persistent disk cache; std and language coverage remain incomplete | both ok | Extend the single pipeline only |
| live-execution.md §4.5, SK-14 | `hd_run::journal` | The journal lives in `hd_run`; live-execution.md names no crate | design wrong | live-execution.md §4.5: name `hd_run` |

### Cost-Model Contradictions

Only places where the code's structure contradicts the design's cost
assumptions. Raw speed is out of scope (build-order.md §9: performance
is eyeballed).

1. **Granularity.** `Emit` is one task per instance miss. Scheduler.md
   §6.2 rule 4 assumes batches per module group, the `codepack` unit.
   Large programs still create thousands of graph nodes.
2. **Contention.** `InternPool` locks one global mutex on lookup;
   `ShardedInterner` locks its append path on misses; the rayon executor
   locks the graph to drain ready work. The designed per-worker
   read-through tables do not exist.
3. **Allocation and locality.** `InternPool::intern` allocates a key
   vector before lookup and owns one boxed extra slice per type. The
   audited `AppendVec` fixed its former `OnceLock` slot inflation.
4. **I/O and incremental cost.** The disk cache is live, but there is no
   stat manifest. Coherence is recomputed over every module, and a
   `prog_key` miss decodes every module's TIR rather than the reachable
   closure.
5. **Skim cost.** Skim lexes whole files and records body lines as
   uses, so the "skim is cheaper than parse" assumption of syntax.md
   §4.3 does not hold; it costs about half a parse (4.2 against
   8.8 ms at 30,000 lines in skeleton-findings.md).
6. **Coherence.** M3's overlap check compares every pair of heads for one
   trait. The designed ground and generic tries avoid quadratic work when
   constructors separate ordinary heads.

## Design Changes Proposed

Historical checklist: D1 applied these 13 corrections before M1. The M1
reconciliation adds one correction: interface ownership belongs to
`hd_resolve`, so the deleted `hd_iface` crate no longer appears in
design-overview.md, build-order.md, codegen.md or data-structures.md.

1. **design-overview.md §1.1, §1.2, §1.3.** State that `Body(m)` is one
   task per module, with bodies as the logical unit only. Change the
   Coherence row to "the traits whose `coh_key` changed, one task". Mark
   the `hdr`, `coh` and `init` cache cells as parts of the `graph` entry.
   Redraw §1.3 with `Parse(f)` created by the task that missed (SK-4).
2. **design-overview.md §2.1.** D2 crates row: depends on `hd_tir`,
   `hd_types`, `hd_host_abi`, not `hd_driver`; fix the arrow in the
   graph. Add `hd_cache` to `hd_run`'s dependencies.
3. **design-overview.md §2.2.** Add: "`hd_driver` takes its sources,
   cache, executor and clock from the caller; it has no `std::fs`,
   `std::process` or `Instant`." This settles the conflict with the
   follow-up table of skeleton-findings.md.
4. **scheduler.md §6.2 (SK-13).** Replace `Scheduler::run(&TaskGraph,
   &(dyn Fn(TaskId, &Spawner) + Sync))` with one task signature for all
   executors: `exec(TaskId, TaskKind, &dyn Spawn)`, where `Spawn` has
   `add`, `edge` and `add_held` (the creation guard). The serial and
   stepping executors implement `Spawn` over their `&mut` graph; the pool
   over the atomic graph.
5. **scheduler.md §6.2 rule 4 and codegen.md §11.1, §11.3.** One
   emission rule: `ExtTask::Emit(GroupId)` per folder group that has at
   least one code-entry miss; `Collect` looks up hits itself. The group
   is the `codepack` unit of cache.md §5.2.
6. **data-structures.md §3.9.4 and §3.24.** Either allow one audited
   `unsafe` `AppendVec` in `hd_base` (raw chunk pointers, a published
   length), or restate the column and pool budgets with the `OnceLock`
   slot cost. Recommendation: allow the audited `unsafe`; it is the
   design's cost basis.
7. **data-structures.md §3.20.2 and checking-and-tir.md §4.13.11
   (SK-N9).** Tables are per body inside the entry, so a TIR hash is a
   function of its body alone. Note the size cost: a module repeats
   shared strings, paths and types once per body.
8. **codegen.md §11.4.** `hd_run` depends on `hd_wasm` and `hd_cache`.
9. **build-order.md §22.** State that until wasmtime is an approved
   dependency (SK-10), `hd run` runs on Node through an `hd_run::Engine`
   implementation, and the slice 6 exit runs on that engine.
10. **build-order.md §9, slice 3.** Add to the slice-3 entry: "the thin
    vertical slice runs on the designed types (`hd_types`, `hd_tir::ir`,
    `hd_resolve`, `hd_project`, `CacheStore`, `hd_diag`); the walking
    skeleton's types are deleted when it lands." Without this line, the
    walking skeleton counts as the vertical slice, and the duplicates
    stay.
11. **build-order.md §9 or checking-and-tir.md §4.13.1.** Add a rule:
    a stage that answers "not implemented" during a build stops that
    build with an internal `unsupported` diagnostic naming the stage and
    the first reason. Counting it is for `analyze_package` only.
12. **trait-solver.md §1.1.** Name `hd_types` as the solver's crate
    (SK-11).
13. **live-execution.md §4.5.** Name `hd_run` as the journal's crate
    (SK-14).

## footprint.md Corrections

Historical pre-M1 corrections follow. Statuses use footprint.md's own definitions. "Real" requires the
structure the section designs, implemented and tested; a type with no
caller in either driver is noted.

1. **data-structures.md §3.4 Types: real → skeleton.** No body-local
   pool; inference variables go into the global pool; one global mutex,
   not the `ShardedInterner`; the checker does not use it.
2. **data-structures.md §3.12 Layout Cursor And Skim State: real →
   skeleton.** The cursor is real; there is no skim state, because skim
   runs the full lexer.
3. **data-structures.md §3.13 The Green Tree, Its Wire Format And The JS
   Decoder: real → skeleton.** No JS decoder exists.
4. **data-structures.md §3.14 The Header Skeleton And The Item Index:
   real → skeleton.** No header tree; the use list holds body lines.
5. **data-structures.md §3.18 TIR: real, with a note.** The designed
   `ir::Body` has no wire form or hash; the running path uses `TirBody`.
6. **data-structures.md §3.20 Cache Entries And The Manifest: real →
   skeleton.** The framing is real but unused; the `check` entry the
   driver writes has none of it and lacks five sections; no manifest is
   read or written.
7. **data-structures.md §3.21 The Scheduler's Task Graph: real →
   skeleton.** No atomics, no `AppendVec`, no creation guard.
8. **data-structures.md §3.22 Codegen: The Instance Table And Code
   Entries: real, with a note.** `InstanceTable` is unused; collection
   uses `InstanceSet`.
9. **syntax.md §4.3 Skim Mode And The Header Pass: real → skeleton.**
   Same reasons as items 2 and 4.
10. **syntax.md §4.4 Parser And Green Tree: real, with a note.** The
    driver that runs programs does not use this parser.
11. **resolution-and-interfaces.md §4.8 Folder Graph: real, with a
    note.** Only the architecture driver uses `FolderGraph`; `hd run`
    uses `Run::visit`.
12. **cache.md §5.3 Key Composition: real → skeleton.** Five of thirteen
    keys; `toolchain_key` lacks five fields; the package key is
    constant.
13. **cache.md §5.4 Entry Format And Atomic Publish: real, with a
    note.** No driver writes framed entries.
14. **scheduler.md §6.2 Executors: real → skeleton.** No driver can run
    on the pool; the stepping executor steps tasks, not bodies, and
    `hd_web` does not use it.
15. **scheduler.md §6.3 Priority: real → skeleton.** Priorities are set
    and then ignored by the FIFO run.
16. **wasm-layout.md §15.1 Layout Classes and §15.2 Values: real, with
    a note.** The emitter uses its own four-form layout.
17. **runtime-and-host.md §16.4 Metadata And The Import List: real →
    skeleton.** `link` writes no `hd.runtime` section.
18. **runtime-and-host.md §17.1 One ABI Description: real, with a
    note.** The emitter's imports come from `hd_wasm::IMPORTS`.
19. **runtime-and-host.md §17.9 The Embedding API: real → skeleton.**
    No engine implements it; `hd run` does not use it.
20. **commands.md §7.5, §20.2 and §20.3 (`hd run`, `hd build`): real →
    skeleton.** Subset only, no std, no cache between runs, on Node.
21. **wasm-layout.md §15.8 Deterministic Bytes: skeleton stands, with a
    note.** The determinism matrix test compares four FIFO runs, because
    `run` takes no executor order.
22. **Totals.** Items 1 to 4, 6, 7, 9, 12, 14, 15, 17, 19 and 20 move
    15 rows (three of them in item 20) from real to skeleton: real
    falls from 61 to 46 and skeleton rises from 90 to 105. Per doc:
    data-structures.md 7 real and 15 skeleton; syntax.md 5 and 1;
    cache.md 4 and 4; scheduler.md 1 and 4; runtime-and-host.md 2 and
    6; commands.md 0 and 8.

### After M1

M1 moves six sections from skeleton to real. It also completes the
previously real TIR, layout and entry-format rows:

1. **Data structures: +1 real.** The task graph has creation guards and
   runs under serial, stepping and rayon executors. The real TIR row now
   names its surviving wire codec and hash.
2. **Scheduler: +1 real.** Every executor runs one `Exec` over
   `&dyn Spawn`.
3. **Cache: real row corrected.** The driver now writes framed interface,
   check, code and link entries through `encode_entry`.
4. **Layout: real rows corrected.** `hd_wasm` consumes
   `hd_mono::layout_of`; the private emitter layout is gone.
5. **Runtime: +1 real.** Node implements `hd_run::Engine`, and `hd run`
   uses it.
6. **Commands: +3 real.** `hd run`, `hd FILE` and `hd build` use the one
   driver and persistent cache.
7. **Totals.** Real rises from 46 to 52; skeleton falls from 105 to 99;
   missing stays 173, for 324 sections.

### After M3

M3 changes no category count. It substantially extends four skeleton
rows without completing every contract:

1. **Resolution §4.9.** Every std use form and prelude binding resolves;
   the private-name diagnostic index is absent.
2. **Interfaces §4.10 and §4.11.** Every std header lowers, stage B runs,
   and blobs round-trip deterministically. Derived heads, trait defaults,
   zero-copy views and declaration anchors remain incomplete.
3. **Traits §4.12.** Generic heads and stage-B obligations run, but
   coherence remains pairwise rather than trie-based.
4. **Diagnostics §4.14.** The complete generated code enum is live;
   interface findings still fall back to file-level spans.
5. **Totals.** Real remains 52, skeleton 99 and missing 173, for 324
   sections.
