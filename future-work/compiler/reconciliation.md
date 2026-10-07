# New Compiler: Code And Design Reconciliation

Part of the [compiler design](README.md).

Status: review, 2026-10-07, at b4c14e39. It compares `compiler/crates`
with the design docs of this folder. It changes neither. Where the code
is right and a doc is wrong, the verdict says "design wrong". Earlier
findings are cited by ID (SK-1 to SK-15, SK-N1 to SK-N16) from
[skeleton-findings.md](skeleton-findings.md).

"Make it move" means compiling programs end to end through the designed
architecture: the full parser, the designed types and TIR, the task
graph's slots and executors, the `CacheStore`, and the designed back
half. Today `hd run` works, but only through the walking skeleton's
private pipeline. The architecture skeleton laid the designed types
beside it, and almost nothing connects the two.

## Summary: The Top 10

Ranked by how much each blocks "make it move".

1. **Two pipelines, and the running one uses throwaway types.**
   `hd_driver::run_with` (behind `hd run`, `hd build`, `hd_testkit`)
   runs the walking skeleton: `parse_subset`, `hd_check::resolve`,
   `hd_tir::world::World`, `hd_tir::TirBody`, `hd_iface::CTy`,
   `hd_mono::collect` and `hd_wasm::emit`. `hd_driver::architecture`
   (behind `hd_web` and `hd_stdpack`) is a second driver over
   `hd_project`. The designed types (`InternPool`, `InferTable`,
   `hd_tir::ir::Body` and `TirBuilder`, `ModuleScope`, `FolderGraph`,
   `CacheStore`, `layout_of`, `InstanceTable`, the solver) have no caller
   on either path. Every feature added now is written twice or onto code
   that will be deleted. Fix: do build-order.md slice 3's "thin vertical
   slice" again, on the survivor types listed under
   [Duplicate Paths](#duplicate-paths), and delete the subset types in
   the same change.
2. **The full parser rejects half of std, and the checker reads the
   subset parser's tree** (SK-6). 19 of 36 `lib/std` files report
   `syntax-error` or `comparison-chaining`. `hd_check` builds its typed
   views over `SubsetParse`, so no program can use std or the prelude.
   Fix: make `hd_syntax::parse` accept every std file (slice 1's exit
   test), then move `hd_check::resolve::Cst` to the full parser's
   `GreenTree` and delete `hd_syntax::subset`.
3. **The body checker is not built on `hd_types`.** `hd_check` does not
   depend on `hd_types`. `check_fn` types expressions directly against
   `World` types, with no `InferTable`, no `TirSink`, no solver and no
   fuel. Fix: rewrite `Body(m)` over `InferTable`, `TirBuilder` and
   `SkeletonSolver`, emitting `hd_tir::ir::Body`.
4. **No std and no prelude reach a program.** `println` is an
   intrinsic of the subset TIR, the package name is the literal `pkg`,
   `hd_resolve::derive_heads` and `header_check` are stubs, and
   `hd_stdpack::build_pack` always refuses. Fix: build folder
   interfaces with `hd_resolve` over the full parser and `hd_types`,
   then the std pack, then resolve `println` through the prelude.
5. **Task bodies share `&mut World` and `&mut Run`.** Every driver task
   mutates one `World` (DefIds, types, constants, impl tables) and one
   `Run`. Neither driver can run under `hd_sched::pool`, which hands a
   task a `&Spawner`, not `&mut TaskGraph` (SK-13). The design has
   per-kind result slots and frozen tables (scheduler.md §6.1). Fix:
   per-kind `OnceLock` slots, frozen interface and impl tables, and one
   spawn handle that every executor passes to the task.
6. **"Not implemented" does not stop a build.** In `run_with`,
   `HeaderCheck`, `Coherence` and `TestOverlay` answer
   `NotImplemented`. The answer goes into a counter, and `Collect`
   still runs. So `hd run` builds a program with overlapping impls or
   unchecked header bounds. The trait sample shows it today: it has one
   impl, so `Coherence` answers "not implemented", and the build goes
   on. Fix: a `NotImplemented` answer from a stage that `PackageResult`
   or `Collect` waits for becomes an internal "unsupported" diagnostic
   and stops the build.
7. **Diagnostics after parsing are strings.** The driver collects
   `Vec<String>`. `hd_diag::Code` holds only the 28 syntax codes and is
   written by hand, not generated from the spec (design-overview.md
   §2.2 rule 2). `DiagBuf` and content order are unused. No name-phase
   or type-phase conformance case can be scored. Fix: generate `Code`
   from the spec's code list, and route every stage through `DiagBuf`.
8. **The back half has a second layout, selection and import table.**
   `hd_wasm::emit` has its own `layout` over `World` types, re-runs
   trait selection through `hd_mono::resolve_method`, and imports from
   its own `IMPORTS` constant. `hd_mono::layout::layout_of`,
   `InstanceTable`, `hd_types::solver::select` and `hd_host_abi::TABLE`
   are unused. Fix: `Emit` over `ir::Body`, `layout_of` and the
   `TABLE`; collection records each `Selection` so emission does not
   select again.
9. **No cache boundary goes through `CacheStore`.** The driver uses
   `hd_cache::MemStore` (no trait, no entry framing). `hd_cli` builds
   with the `disk` feature but never opens a `DiskStore`, so every
   `hd run` is cold. `toolchain_key` lacks the std pack hash, the host
   profile, the layout version, the hash id and the limits hash; the
   package key is a constant; `hdr`, `test`, `coh`, `init`, `graph`,
   `pkgres`, `fast` and `manifest` keys do not exist. Fix: the driver
   takes `&dyn CacheStore`; `hd_cli` opens a `DiskStore`; entries use
   `encode_entry`; add the missing keys as their stages land.
10. **`hd_driver` holds file access, a process runner and clocks.**
    `source.rs` reads directories, `node.rs` spawns Node, and
    `run_with` calls `Instant::now()` around every task. Design-overview
    §2.2 rules 1 and 3 put file access in `hd_cache`'s `disk` feature
    and `hd_cli` only. `Instant::now()` panics on
    `wasm32-unknown-unknown`, so `hd_web` cannot use `run_with`.
    `hd run` runs on a Node subprocess, not through `hd_run::Engine`.
    Fix: a disk `SourceSet` in `hd_cli`; a Node engine behind
    `hd_run::Engine`; stage timing through a clock the caller passes in.

## Findings

Verdicts: **gap** (the design has it, the code does not, or only as a
stub); **impl wrong** (the code took a shortcut or misread the design);
**design wrong** (the design is unworkable, contradicts another doc, or
missed a case); **duplicate** (two code paths for one design thing);
**both ok** (both work; the row picks one).

### Duplicate Paths

| Design thing | Path in use (`hd run`) | Designed path (unused or barely used) | Survivor |
| --- | --- | --- | --- |
| driver | `hd_driver::run_with` | `hd_driver::architecture::analyze_package` | one driver: `run_with`'s cache-first flow plus `analyze_package`'s `hd_project` tables and stage tallies |
| parser | `hd_syntax::subset::parse_subset` | `hd_syntax::parser::parse` | `parse` |
| module identity and discovery | `hd_driver::source` (`module_path` fixed to `pkg.`, use-driven folder loading) | `hd_project::ModuleTable::discover`, `SourceSet` | `hd_project` |
| folder graph | `Run::visit`, `Run::closure` (strings, closure recomputed per call) | `hd_project::FolderGraph` (bit sets, heights, cycles) | `FolderGraph` |
| scope | `hd_check::resolve::Scope`, `module_scope` | `hd_resolve::ModuleScope`, `Binding` | `ModuleScope` |
| interface type form | `hd_iface::CTy` (string paths), `HeaderItem` | `hd_types::TyData` plus the blob's own type table (§3.20.2) | `hd_types`; the blob keeps an entry-local table |
| `DefId`, `Ty` | `hd_tir::world::{DefId, Ty, TyKind}` (derive `Ord`) | `hd_base::DefId`, `hd_types::{Ty, TyData}` (no `Ord`, §3.1) | `hd_base` and `hd_types` |
| symbol interner | `hd_intern::Interner` (inside `World`) | `hd_intern::ShardedInterner` | `ShardedInterner` |
| type pool | `World::ty` | `hd_types::InternPool` | `InternPool` |
| impl tables | `World::impls` | `hd_types::solver::ImplTable` | `ImplTable` |
| selection | `hd_mono::select`, `resolve_method` (linear scan) | `hd_types::solver::{Solver, select}` | the solver |
| TIR | `hd_tir::{TirBody, TirTag}` with wire form and `tir_hash` | `hd_tir::ir::{Body, Tag, TirBuilder}` with verifier, printer and parser, no wire form | `ir::Body`; port the wire form and hash to it |
| key hasher | `hd_iface::KeyHasher` (buffers the key in a `Vec`) | `hd_base::StableHasher` (streaming) | `StableHasher` |
| entry bytes | `hd_iface::{put_*, Reader}` ad hoc sections | `hd_cache::store::{encode_entry, decode_entry}` | `encode_entry` with section readers |
| cache store | `hd_cache::MemStore` | `hd_cache::store::{CacheStore, MemoryStore, DiskStore}` | `CacheStore` |
| diagnostics | `Vec<String>` | `hd_diag::{Diagnostic, DiagBuf}` | `hd_diag` |
| instance key | `hd_mono::instance_key` (over `World`) | `hd_mono::layout::instance_key` (over `InternPool`) | `layout::instance_key` |
| A1 class | `hd_mono::classify` | `hd_mono::layout::a1_class` | `a1_class` |
| instance set | `hd_mono::InstanceSet` | `hd_mono::layout::InstanceTable` | `InstanceTable` |
| value layout | `hd_wasm::layout` (private, four type forms) | `hd_mono::layout::layout_of` (every form) | `layout_of`, as codegen.md §11.4 places layouts in `hd_mono` |
| host imports | `hd_wasm::IMPORTS` (`hd.println_i32`) | `hd_host_abi::{TABLE, imports_of}` | `hd_host_abi` |
| pipeline identity | the `PIPELINE = "dev"` string in `code_key` | `hd_mono::passes::DEV` and its pipeline hash | the pipeline hash (tiering.md §6.4) |
| program runner | `hd_driver::node::run_wasm` (a Node subprocess) | `hd_run::{Engine, run_program, drive}` | `hd_run::Engine`, with Node as one engine |
| serial executor | `TaskGraph::run` (FIFO only) | `SerialScheduler`, `SteppingScheduler`, `pool::run_pool` | the executors, behind one task interface |

### Findings Table

| Design section | Code path | Finding | Verdict | Proposed fix |
| --- | --- | --- | --- | --- |
| design-overview.md §1.1 to §1.3 | `hd_sched::TaskKind::Body(u32)` | The overview says "Body tasks per item" and calls the body "the parallel task unit"; scheduler.md §6.1 and §6.2, type-checking.md §1.7 and checking-and-tir.md §4.13.1 make `Body(m)` one task per module. The code follows the three | design wrong | Overview: "Body(m), one task per module; bodies are the logical unit, not the scheduling unit" |
| design-overview.md §1.2 (Coherence row), SK-12 | `TaskKind::Coherence`, `hd_check::stages::coherence` | Unit per trait in the overview; one task per run in scheduler.md §6.1 and cache.md §5.3. The code has one task, with no key | design wrong | Overview row: "traits whose `coh_key` changed, one task" |
| design-overview.md §1.2 (Cached column) | none | The overview names `hdr`, `coh` and `init` entries; cache.md §5.2 makes them parts of one `graph` entry | design wrong | Overview: "part of `graph`" in those three rows and in the §1.1 diagram |
| design-overview.md §1.3 (diagram), SK-4 | `Run::parse`, `Run::module_prep` | The diagram still draws `Parse(f)` as a static predecessor of `ModulePrep`; scheduler.md §6.1 was fixed, the overview was not | design wrong | Redraw: `Parse(f)` is created by the task that missed |
| design-overview.md §2.1 (D2 crates row) | `hd_driver/Cargo.toml` | The row says the D2 crates depend on `hd_driver`; codegen.md §11.4 and the code put them below it (`hd_driver` depends on `hd_mono` and `hd_wasm`) | design wrong | Row: "`hd_tir`, `hd_types`, `hd_host_abi`"; arrow from `hd_driver` down to them |
| design-overview.md §2.1 (`hd_check`) | `hd_check/Cargo.toml` | `hd_check` does not depend on `hd_types` or `hd_diag`; it types against `hd_tir::world` | impl wrong | Depend on `hd_types` and `hd_diag`; drop `World` |
| design-overview.md §2.1 (`hd_tir`) | `hd_tir::world` | `hd_tir` holds the run's DefId table, type pool, symbol interner and impl tables, which the design puts in `hd_intern` and `hd_types` | impl wrong | Delete `world.rs`; TIR holds only bodies |
| design-overview.md §2.2 rules 1 and 3 | `hd_driver::source`, `hd_driver::node`, `Instant` in `run_with` | File reads, a subprocess and clocks in `hd_driver`; skeleton-findings.md's follow-up table records this as intended, against the rule. `Instant::now()` panics on `wasm32-unknown-unknown` | impl wrong | Disk `SourceSet` and Node runner in `hd_cli`; timing through a caller-supplied clock |
| design-overview.md §2.2 rule 2 | `hd_diag::Code` | Hand-written, 28 syntax codes; no `cargo xtask codegen`, no generated typed views (SK-N1) | gap | Add the xtask; generate `Code` and the typed views |
| design-overview.md §2.1, SK-11 | `hd_types::solver` | Solver in `hd_types`, as the overview says | both ok | Keep; trait-solver.md §1.1 should name the crate |
| data-structures.md §3.1 | `hd_tir::world::{DefId, Ty}` | Second ID set that derives `Ord`, against "IDs have no `Ord`" | duplicate | Use `hd_base` IDs |
| data-structures.md §3.3 | `hd_intern::ShardedInterner`, `hd_types::InternPool` | The string interner takes a global `append` mutex on every miss; the pool takes one global `Mutex<HashMap>` on every lookup, hit or miss, and allocates the key `Vec` first. The design has per-thread columns, 64 shards and a per-worker read-through table | impl wrong | One `ShardedInterner<C>` as §3.3 draws it, used by all three interners |
| data-structures.md §3.4 | `InferTable::fresh`, `InternPool::intern_ty` | Inference variables are interned into the global pool (the code says so); the body-local pool with bit 31 does not exist | gap | Body-local pool owned by the body task |
| data-structures.md §3.9.2, §3.9.4, §3.24 | `hd_base::AppendVec` | With `forbid(unsafe_code)`, each slot is a `OnceLock<T>` (8 bytes for a `u32`), chunks are filled slot by slot, and the pool's `extra` column holds one `Box<[u32]>` per type. The memory budget assumes dense 4-byte columns and one shared `extra` array | design wrong | §3.9.4: allow one audited `unsafe` `AppendVec` in `hd_base`, or restate the budget for `OnceLock` slots |
| data-structures.md §3.13 | `hd_syntax::green::to_wire` | Wire form exists; no JS decoder anywhere in the repository | gap | Write the decoder with the playground (slice 5) |
| data-structures.md §3.14, syntax.md §4.3 | `hd_syntax::skim` | Skim runs the full lexer, keeps no header tree (SK-N4), and records every line that starts with an identifier, body lines included, as a use. The driver filters by the text `use ` | impl wrong | Skim-mode lexer that skips bodies; header tree; `use` lines only |
| data-structures.md §3.18, checking-and-tir.md §4.13.11 | `hd_tir::ir` | The designed TIR has no wire form, no remapping and no TIR hash; only `TirBody` has them | gap | Port `write_body`, `read_body`, `tir_hash` to `ir::Body` |
| data-structures.md §3.20.2, SK-N9 | `hd_tir::write_tables` per body | The design has entry-local tables; the code gives each body its own tables, so a TIR hash depends only on its body | design wrong | §3.20.2: tables per body inside the entry; the later per-body reuse of §4.13.1 needs the same |
| data-structures.md §3.20.4, cache.md §5.2 | `Run::module_finish` | The `check` entry has diagnostics, a meta hash, headers (SK-1) and TIR; no init summary, row results, facts, reads or `locs` section, no `EntryHeader` | gap | Write through `encode_entry`; add sections as their stages land |
| data-structures.md §3.21 | `hd_sched::TaskGraph` | `Vec` nodes, a `VecDeque` ready queue, `&mut` mutation; no atomics, no `AppendVec`, no creation guard | gap | Keep the serial graph; add the concurrent one when threads land |
| data-structures.md §3.21, scheduler.md §6.1 | `hd_sched::pool::Spawner` | No creation guard. The driver's pattern "add `Link` with no deps, then add edges from each `Emit`" lets another worker pop `Link` first, and `edge` then asserts "edge into a started task" | impl wrong | `Spawner::add_held` returns a guard; the task becomes ready when the guard drops |
| data-structures.md §3.22 | `hd_mono::layout::InstanceTable` | Exists and is tested; collection uses `InstanceSet` | duplicate | Collect into `InstanceTable` |
| scheduler.md §6.1 (results in slots) | `hd_driver::Run` | Results live in one `&mut Run` (hash maps keyed by strings and run IDs), not per-kind slot vectors | impl wrong | `OnceLock` slot vectors per kind, as §6.1 says |
| scheduler.md §6.2, SK-13 | `Executor`, `pool::run_pool` | `Scheduler: Sync` with `&TaskGraph` (design) against `&mut TaskGraph` (serial, stepping) and `&Spawner` (pool). Task code written for one cannot run under the other | design wrong | §6.2: every executor passes `&dyn Spawn` (add, edge, add_held); the serial one wraps its `&mut` graph. One task signature for all |
| scheduler.md §6.2 (pool) | `pool::run_pool` | One mutex over the whole graph, `notify_all` on every completion, and `pop(Priority)` scans the ready queue (`O(ready)` per pop, quadratic over 4,001 `Emit` tasks). The design uses rayon's work stealing (SK-9) | gap | rayon once approved; until then a binary heap for priority |
| scheduler.md §6.2 (stepping) | `SteppingScheduler::run_for`, `hd_web::WebSession::check` | A step is one task, not one body; `hd_web` does not step at all ("one slice") | gap | `Body(m)` keeps a body cursor; `hd_web` calls `run_for` |
| scheduler.md §6.3 | `analyze_package`, `TaskGraph::run` | Folder heights are set as priorities, then the graph runs `Fifo`, which ignores them; `run_with` sets none | impl wrong | Run with `SerialOrder::Priority` |
| scheduler.md §6.4 | `analyze_package::body` | The task-boundary panic catch exists only in the architecture driver; `run_with` has none | gap | Catch at the executor, for every task |
| scheduler.md §6.2 rule 4, codegen.md §11.1, §11.3 | `Run::collect`, `ExtTask::Emit(u32)` | §11.3 says one `Emit` per code-entry miss; §6.2 rule 4 says batches per module group; the §11.1 diagram says per folder group. The code makes one task per instance, hits included | design wrong | One rule in both docs: `Emit(group)` per folder group with a miss (the `codepack` unit); hits are looked up in `Collect` |
| syntax.md §4.4, build-order.md slice 1 | `hd_syntax::parser` | 19 of 36 std files fail to parse (SK-6, reproduced: 18 `syntax-error`, 1 `comparison-chaining`) | gap | Close slice 1: add a std-parses test |
| resolution-and-interfaces.md §4.7 | `hd_driver::source::load_program` | The run path names modules `pkg.<file>` and loads folders by following uses; `hd_project::discover` exists but only the architecture driver calls it | duplicate | `hd_project` only |
| resolution-and-interfaces.md §4.9 | `hd_check::resolve::module_scope` | Subset resolution: no prelude, no `pub use`, no ambiguity rule. On std it reports `unknown-import: folder use.std` | duplicate | `hd_resolve::ModuleScope` with the use worklist of §3.15 |
| resolution-and-interfaces.md §4.10, §4.10.1 | `hd_iface::build_iface`, `hd_resolve::{header_check, derive_heads}` | Interfaces hold only the subset's public fns, data, traits and impls; stage B and derived heads are stubs | gap | Build interfaces in `hd_resolve` over the full parser |
| resolution-and-interfaces.md §4.11 | `hd_iface::decode_iface` | The blob is decoded into a `Vec` (SK-N6), not read in place; per-item hashes are shallow; no validator | gap | Indexed blob and zero-copy reader, as §4.11.1 lays out |
| resolution-and-interfaces.md §4.12, trait-solver.md §5.2 | `hd_check::stages::coherence` | Counts impls from skim facts; any impl gives `NotImplemented`, which `run_with` ignores | gap | Overlap check per trait over interface heads; see top-10 item 6 for the stop |
| checking-and-tir.md §4.13.1 (M1, M3) | `Run::module_prep`, `Run::module_finish` | M1 lowers headers only (no omitted-result walk); M3 writes the entry only (no rows, init summary or sort) | gap | Land with the `hd_types` checker |
| checking-and-tir.md §4.14 | `Run::diagnostics: Vec<String>` | No codes, spans, root keys or content order past parsing | gap | `DiagBuf` per module, printed at the end (§6.5) |
| checking-and-tir.md §4.15, trait-solver.md §7.4 | `hd_base::Fuel` | Defined; nothing charges it | gap | Charge in the checker and solver |
| type-checking.md §1.4 to §1.6 | `hd_check::body::check_fn` | The checker uses none of the accessor API, `TirSink` or `Solver`; locals are a `HashMap<String, u32>` | gap | Rewrite on the designed APIs (top-10 item 3) |
| trait-solver.md §3.1 to §3.4 | `hd_types::solver::SkeletonSolver` | One exact head only; `select` for generic heads and normalization answer `NotImplemented`; nothing calls it | gap | Wire to the checker first, then widen |
| cache.md §5.3 | `hd_cache::{toolchain_key, iface_key, check_key, prog_key, code_key}` | Five of thirteen keys; `toolchain_key` lacks five of its seven fields; the package key is `H("root", "pkg")` | gap | Add fields and keys as stages land |
| cache.md §5.2 (packs) | `Run::emit`, `EntryKind::Code` | One `code` entry per instance; §5.2 says code entries exist only inside `codepack`s. In memory this is harmless; on disk it is one file per function | impl wrong | Pack per folder group at `Link`, before the disk store is wired |
| cache.md §5.5 | `hd_cache::store::ManifestRecord` | The record type exists; no stat manifest is read or written | gap | With the disk store (slice 4) |
| codegen.md §11.2, §11.3, SK-N12 | `Run::collect`, `Run::decode_pending` | `prog_key` lists every module, and a miss decodes every module's TIR, not only modules the root reaches | impl wrong | Reachable modules from the manifest's use lists, as §11.3 says |
| codegen.md §11.3 | `Run::package_result` | `Collect` runs after all of `PackageResult`, not after the `tir` entries and `HeaderCheck` tasks of reached folders only | both ok | Keep for one program; split when tests add programs |
| codegen.md §11.4 | `hd_run/Cargo.toml` | `hd_run` depends on `hd_cache` (for `Engine::load`'s `cwasm` lookup); §11.4 lists only `hd_wasm` | design wrong | §11.4: add `hd_cache` to `hd_run`'s row |
| codegen.md §12.1, §12.2 | `hd_wasm::emit` | Emits from `TirBody` under `World` substitution; selects again through `resolve_method`; one Wasm local per value (SK-N14) | duplicate | Emit from `ir::Body` with the collected `Selection`s |
| codegen.md §13.2, SK-2 | `hd_check::body` (`rep_summary`), `A1Rule` | Bounded parameter is always exact, as SK-2 decided; codegen.md §13.2 was updated | both ok | None |
| codegen.md §13.3 | `hd_mono::instance_key` | Instance key hashes `CTy` bytes and a sub-body string `"0"`; the designed key in `layout::instance_key` takes a `u16` sub-body index | duplicate | `layout::instance_key` |
| codegen.md §13.8, SK-3 | `hd_cache::code_key` | Callee summaries are in the code key, as SK-3 decided; the `deps` interface hashes of §13.8 are not (SK-N15) | gap | Add `deps` when a callee's interface can change its caller's code (inlining) |
| wasm-layout.md §15.1, §15.2 | `hd_mono::layout::layout_of`, `hd_wasm::layout` | The designed layout is complete and tested but unused; the emitter's own layout knows four type forms | duplicate | Emitter calls `layout_of` |
| wasm-layout.md §15.4 to §15.6 | none | No globals, module init, panic sites or size accounting | gap | Slice 7 |
| runtime-and-host.md §16.4 | `hd_wasm::meta::RuntimeMeta`, `hd_wasm::link` | The codec exists; `link` writes no `hd.runtime` section | gap | `link` writes it from the import set |
| runtime-and-host.md §17.1, §17.2 | `hd_host_abi::TABLE`, `hd_wasm::IMPORTS` | "One ABI description", but the emitter imports from its own constant | duplicate | Imports from `TABLE` |
| runtime-and-host.md §17.9 | `hd_run::{Engine, run_program}` | The API exists; no engine implements it (`hd_run_wasmtime::load` always fails, SK-10); `hd run` bypasses it | gap | A Node engine behind `Engine` now; wasmtime when approved |
| engines-and-test-runner.md §18.4, build-order.md §22 | `hd_driver::node`, `hd_cli::run_command` | `hd run` runs on Node; §22 says `hd run` runs on wasmtime with its default configuration until slice 7, with Node for V8 checks | both ok | Keep Node behind `Engine` until wasmtime is approved; §22 should say so |
| testing-the-compiler.md §8.1, wasm-layout.md §15.8 | `hd_testkit::determinism_matrix` | The test's build closure ignores its `SerialOrder` (`run` has no order parameter), so the matrix compares four FIFO runs | impl wrong | `run_with` takes the executor order |
| commands.md §7.1 | none | No `hd check` command; `hd_cli` has `parse`, `run`, `build` | gap | `hd check` on the single driver |
| commands.md §7.5, §20.2, §20.3 | `hd_cli` | `hd run` and `hd build` work for the subset only, cold every time, with no std | gap | Follows top-10 items 1 to 4 and 9 |
| live-execution.md §4.5, SK-14 | `hd_run::journal` | The journal lives in `hd_run`; live-execution.md names no crate | design wrong | live-execution.md §4.5: name `hd_run` |

### Cost-Model Contradictions

Only places where the code's structure contradicts the design's cost
assumptions. Raw speed is out of scope (build-order.md §9: performance
is eyeballed).

1. **Granularity.** `Emit` is one task per instance, hits included
   (4,001 tasks at 30,000 lines). Scheduler.md §6.2 rule 4 assumes
   batches per module group. With `pop(Priority)` scanning the ready
   queue, that is quadratic under the pool. Fix in the findings table.
2. **Contention.** `InternPool` locks one global mutex on every lookup;
   `ShardedInterner` locks a global append mutex on every miss; the pool
   executor locks the whole graph per pop and per completion and wakes
   every worker. Data-structures.md §3.3 assumes lock-free hits through
   per-worker tables, and scheduler.md §6.2 assumes work stealing.
3. **Shared mutable state.** `&mut World` through checking, collection
   and emission makes `Body(m)` and `Emit` serial by construction.
   Every parallel speedup figure in the design assumes frozen tables.
4. **Allocation and locality.** `InternPool::intern` allocates a key
   `Vec` before the lookup and one `Box<[u32]>` per new type;
   `AppendVec` stores each item in a `OnceLock`. §3.9's dense
   encoding and §3.24's budget assume neither.
5. **Superlinear work.** `analyze_package::body` builds a fresh `World`
   per module and loads every interface into it: modules times
   interface items. `Run::folder_iface` scans every file per folder,
   and `Run::closure` re-walks the graph for every key. `hd_project`
   already has the linear versions.
6. **I/O and incremental cost.** `hd_cli` keeps no cache between runs,
   so the warm paths of design-overview.md §1.4 are never exercised by
   the CLI. Coherence is recomputed over every module on every run, with
   no `coh_key`. On a `prog_key` miss, every module's TIR is decoded.
7. **Skim cost.** Skim lexes whole files and records body lines as
   uses, so the "skim is cheaper than parse" assumption of syntax.md
   §4.3 does not hold; it costs about half a parse (4.2 against
   8.8 ms at 30,000 lines in skeleton-findings.md).

## Design Changes Proposed

These are proposals for the doc owners. This review edits none of them.

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

Statuses use footprint.md's own definitions. "Real" requires the
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
