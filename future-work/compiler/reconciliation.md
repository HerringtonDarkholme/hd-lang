# New Compiler: Code And Design Reconciliation

Part of the [compiler design](README.md).

Status: review, 2026-10-07, after M1 (`07c74892`). It compares `compiler/crates`
with the design docs of this folder. It changes neither. Where the code
is right and a doc is wrong, the verdict says "design wrong". Earlier
findings are cited by ID (SK-1 to SK-15, SK-N1 to SK-N16) from
[skeleton-findings.md](skeleton-findings.md).

M1 made the designed architecture move end to end. The one driver now
uses the full parser, `hd_project`, `hd_resolve`, `hd_types`,
`hd_tir::Body`, per-kind result slots, every executor, `CacheStore`,
`hd_mono` and `hd_wasm`. The remaining findings are feature coverage,
missing generated surfaces and incomplete cache/runtime details, not a
second compiler pipeline.

## Summary: The Top 10

Ranked by how much each blocks the next working language slice.

1. **The parser rejects 395 accepted fixtures.** Q8 reduces the failures
   to 34 minimal constructs. The two largest heuristics reject 264 files
   for `and` or `or` in source text and 57 typed `let` forms with a nested
   comma. The one driver now uses this parser, so these are direct build
   blockers rather than a dormant alternate path.
2. **Header checking and much of body checking still answer
   `NotImplemented`.** `BodyCx` uses `InternPool`, `SkeletonSolver`, fuel
   and `TirBuilder`, but unsupported expressions become a coded internal
   diagnostic. `header_check`, derived heads, test overlays and nontrivial
   coherence remain partial. This is safe now because the build stops.
3. **The standard-library pack is not writable.** Folder interfaces can
   be built and the host prelude is represented in `hd_host_abi`, but
   `hd_stdpack::build_pack` still stops at header checking or the pack
   writer. Programs therefore cannot load the ordinary `lib/std` modules.
4. **The complete diagnostic enum is generated but not integrated.** Q9
   writes all 233 spec codes and phase metadata to `hd_diag/src/codes.rs`.
   The task's one-file compiler restriction leaves `hd_diag::Code` in
   `lib.rs`, so the driver can emit only its hand-written subset plus
   `unsupported` until the module hook is authorized.
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
   absent.** The parser gained structured line nodes, but `hd.ungram`, the
   schema generator and the green-tree JavaScript decoder do not exist.
   Browser consumers still lack the designed stable generated surface.
8. **Emission follows the designed types but covers only a scalar slice.**
   It uses `hd_tir::Body`, `layout_of`, collected instances and the host ABI.
   Suspensions, GC aggregates, metadata, panic sites and many TIR forms
   still return `NotImplemented`.
9. **Task-boundary panic isolation regressed.** The old architecture path
   caught panics; the unified `Exec` calls tasks directly under serial and
   rayon executors. One task panic can unwind the build instead of becoming
   the designed internal diagnostic.
10. **Stepping exists but the browser does not use it.** `hd_web` calls
    `analyze_package` as one slice, while `SteppingScheduler` counts tasks,
    not body cursors. The playground therefore has neither the designed
    cooperative budget nor cancellation granularity.

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
| parser | `hd_syntax::parse` | fixed | no subset parser remains |
| discovery and graph | `hd_project::{ModuleTable, FolderGraph, SourceSet}` | fixed | none |
| resolution and interfaces | `hd_resolve::{ModuleScope, FolderIface}` | fixed | `hd_iface` was deleted |
| IDs, symbols and types | `hd_base` IDs, `ShardedInterner`, `InternPool` | fixed | the body-local type pool remains absent |
| TIR | `hd_tir::Body`, verifier and wire codec | fixed | none |
| cache bytes and store | framed entries through `CacheStore`; `DiskStore` in `hd_cli` | fixed | manifest and entry coverage remain incomplete |
| task results and executors | per-kind `OnceLock` slots; one `Exec` over `Spawn` | fixed | browser stepping remains unused |
| diagnostics | `DiagBuf` through the running pipeline | fixed | generated full `Code` is not wired in |
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

### Findings Table

| Design section | Code path | Finding | Verdict | Proposed fix |
| --- | --- | --- | --- | --- |
| design-overview.md §1.1 to §1.3 | `TaskKind::Body(u32)`, one slot per module | The overview and M1 now agree on one scheduled body task per module | both ok | None |
| design-overview.md §1.2 (Coherence row), SK-12 | `TaskKind::Coherence`, `hd_check::stages::coherence` | Unit per trait in the overview; one task per run in scheduler.md §6.1 and cache.md §5.3. The code has one task, with no key | design wrong | Overview row: "traits whose `coh_key` changed, one task" |
| design-overview.md §1.2 (Cached column) | none | The overview names `hdr`, `coh` and `init` entries; cache.md §5.2 makes them parts of one `graph` entry | design wrong | Overview: "part of `graph`" in those three rows and in the §1.1 diagram |
| design-overview.md §1.3 (diagram), SK-4 | `Run::module_prep` creates parse and downstream tasks after misses | The dynamic graph now follows the corrected diagram | both ok | None |
| design-overview.md §2.1 (D2 crates row) | `hd_driver` depends downward on `hd_mono` and `hd_wasm` | The dependency direction now matches the corrected row | both ok | None |
| design-overview.md §2.1 (`hd_check`) | `BodyCx` uses `hd_types`, `hd_diag` and `hd_tir` | The duplicate `World` checker was deleted | both ok | None |
| design-overview.md §2.1 (`hd_tir`) | `hd_tir::Body` and wire codec only | Run-global IDs, types and impl tables were deleted from TIR | both ok | None |
| design-overview.md §2.2 rules 1 and 3 | `Host` takes sources, store, executor and clock; Node and disk live in `hd_cli` | The driver owns no host facility | both ok | None |
| design-overview.md §2.2 rule 2 | `hd_diag::Code`, generated `codes.rs` | Q9 generates all 233 codes, but `lib.rs` still defines the public hand-written subset because the task forbids the module-hook edit | gap | Authorize and add `mod codes; pub use codes::{Code, Phase};` |
| design-overview.md §2.1, SK-11 | `hd_types::solver` | Solver in `hd_types`, as the overview says | both ok | Keep; trait-solver.md §1.1 should name the crate |
| data-structures.md §3.1 | `hd_base` IDs and `hd_types::Ty` | The duplicate `hd_tir::world` IDs were deleted | both ok | None |
| data-structures.md §3.3 | `hd_intern::ShardedInterner`, `hd_types::InternPool` | The string interner takes a global `append` mutex on every miss; the pool takes one global `Mutex<HashMap>` on every lookup, hit or miss, and allocates the key `Vec` first. The design has per-thread columns, 64 shards and a per-worker read-through table | impl wrong | One `ShardedInterner<C>` as §3.3 draws it, used by all three interners |
| data-structures.md §3.4 | `InferTable::fresh`, `InternPool::intern_ty` | Inference variables are interned into the global pool (the code says so); the body-local pool with bit 31 does not exist | gap | Body-local pool owned by the body task |
| data-structures.md §3.9.2, §3.9.4, §3.24 | audited unsafe `hd_base::AppendVec` | M1 implements dense never-moved chunks under the owner-approved exception and tests concurrent publication | both ok | Keep the unsafe surface confined to this module |
| data-structures.md §3.13 | `hd_syntax::green::to_wire` | Wire form exists; no JS decoder anywhere in the repository | gap | Write the decoder with the playground (slice 5) |
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
| syntax.md §4.4, build-order.md slice 1 | `hd_syntax::parser` | The one driver uses it, but it rejects 395 accepted fixtures; Q8 records 34 construct repros | gap | Replace raw-line heuristics with grammar nodes, starting with the 264 word-in-source false positives |
| resolution-and-interfaces.md §4.7 | `ModuleTable::discover` and `FolderGraph` | The duplicate driver loader was deleted | both ok | None |
| resolution-and-interfaces.md §4.9 | `hd_resolve::ModuleScope` and prelude bindings | The duplicate subset resolver was deleted; feature coverage remains incomplete | both ok | Extend the survivor only |
| resolution-and-interfaces.md §4.10, §4.10.1 | `hd_resolve::iface`, `header_check`, `derive_heads` | Interface construction uses the full parser and designed types; stage B and derived heads remain stubs | gap | Complete header validation and derived heads |
| resolution-and-interfaces.md §4.11 | `hd_resolve::iface` codec and deep hashes | The duplicate `hd_iface` crate was deleted; the survivor still materializes decoded items rather than exposing the designed zero-copy reader | gap | Add the indexed reader and validator to `hd_resolve` |
| resolution-and-interfaces.md §4.12, trait-solver.md §5.2 | `hd_check::stages::coherence` over interface heads | Exact-head overlap is checked and failures stop the build; generic and normalized heads remain unsupported | gap | Complete overlap over the full solver |
| checking-and-tir.md §4.13.1 (M1, M3) | `Run::module_prep`, `Run::module_finish` | M1 lowers headers only (no omitted-result walk); M3 writes the entry only (no rows, init summary or sort) | gap | Land with the `hd_types` checker |
| checking-and-tir.md §4.14 | per-stage and per-module `DiagBuf`, assembled in content order | Structured diagnostics are on the running path; the complete generated code enum is unwired and four package-level paths use sentinel spans | gap | Wire Q9's generated `Code` module and retain primary source sites for every diagnostic |
| checking-and-tir.md §4.15, trait-solver.md §7.4 | `BodyCx::charge` and solver fuel | Fuel is charged; exhaustion still reports internal `unsupported` instead of the specified limit code | gap | Emit the generated limit diagnostic |
| type-checking.md §1.4 to §1.6 | `BodyCx` over `InternPool`, `TirBuilder` and `SkeletonSolver` | The designed interfaces are wired; unsupported language forms stop the build | both ok | Extend feature coverage without another checker |
| trait-solver.md §3.1 to §3.4 | `SkeletonSolver`, called by `BodyCx` | Exact heads and memoization are live; generic matching and normalization still answer `NotImplemented` | gap | Extend the wired solver |
| cache.md §5.3 | `toolchain_key`, package, interface, check, program and code keys | The package key now uses the manifest name and the pipeline hash is real; seven designed key families and several toolchain fields remain absent | gap | Add fields and keys as their stages land |
| cache.md §5.2 (packs) | `Run::emit`, `EntryKind::Code` on DiskStore | One disk entry is written per instance, though the design makes code entries sections of folder-group codepacks | impl wrong | Pack misses per folder group at `Link` |
| cache.md §5.5 | `hd_cache::store::ManifestRecord` | The record type exists; no stat manifest is read or written | gap | With the disk store (slice 4) |
| codegen.md §11.2, §11.3, SK-N12 | `Run::collect`, `Run::decode_pending` | `prog_key` lists every module, and a miss decodes every module's TIR, not only modules the root reaches | impl wrong | Reachable modules from the manifest's use lists, as §11.3 says |
| codegen.md §11.3 | `Run::package_result` | `Collect` runs after all of `PackageResult`, not after the `tir` entries and `HeaderCheck` tasks of reached folders only | both ok | Keep for one program; split when tests add programs |
| codegen.md §11.4 | `hd_run` depends on `hd_cache` | The design's crate table now records this dependency | both ok | None |
| codegen.md §12.1, §12.2 | `hd_wasm::emit` over `hd_tir::Body` and `layout_of` | The duplicate emitter types and private layout were deleted; many TIR forms remain unsupported | both ok | Extend this emitter only |
| codegen.md §13.2, SK-2 | `hd_check::body` (`rep_summary`), `A1Rule` | Bounded parameter is always exact, as SK-2 decided; codegen.md §13.2 was updated | both ok | None |
| codegen.md §13.3 | `hd_mono::layout::instance_key` | The duplicate `World`/`CTy` key path was deleted | both ok | None |
| codegen.md §13.8, SK-3 | `hd_cache::code_key` | Callee summaries are in the code key, as SK-3 decided; the `deps` interface hashes of §13.8 are not (SK-N15) | gap | Add `deps` when a callee's interface can change its caller's code (inlining) |
| wasm-layout.md §15.1, §15.2 | `hd_mono::layout::layout_of`, used by hd_wasm | The private four-form emitter layout was deleted | both ok | None |
| wasm-layout.md §15.4 to §15.6 | none | No globals, module init, panic sites or size accounting | gap | Slice 7 |
| runtime-and-host.md §16.4 | `hd_wasm::meta::RuntimeMeta`, `hd_wasm::link` | The codec exists; `link` writes no `hd.runtime` section | gap | `link` writes it from the import set |
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
