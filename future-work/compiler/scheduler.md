# New Compiler Design: Scheduler And Task Graph

Part of the [compiler design](README.md).

## 6. Scheduler And Task Graph

### 6.1 Tasks

```rust
pub enum TaskKind {
    Skim(FileId), Parse(FileId), FolderGraph(PackageId),
    FolderIface(FolderId), HeaderCheck(FolderId),
    ModulePrep(ModuleId), Body(ModuleId), ModuleFinish(ModuleId),  // Body: all of a module's bodies, as one batched parallel iterator (granularity rule below)
    Coherence(PackageId), InitOrder(FolderId), PackageResult(PackageId),  // Coherence: one task over the traits whose key changed
    Ext(ExtTask),                       // D2's tasks, behind a trait object
}
struct TaskNode {
    kind: TaskKind,
    waiting_on: AtomicU32,              // unfinished dependencies
    successors: Mutex<SmallVec<[TaskId; 4]>>,
    priority: u32,                      // §6.3
    state: AtomicU8,                    // Waiting | Ready | Running | Done | Cancelled
}
pub struct TaskGraph { nodes: AppendVec<TaskNode> }
```

- **Creation is dynamic.** The driver creates the discovery tasks. A
  finished task may add tasks and edges, for example the folder graph task
  creates one `FolderIface` per folder whose key missed. An edge to a task
  that is already done counts as satisfied at once. Registration and
  completion take the producer's successor lock, so each edge is
  satisfied exactly once, and a new task holds a creation guard until all
  its edges exist (data-structures.md §3.21).
- **Tests.** The scheduler's interleaving test runs register-versus-
  complete races under `loom` and checks that every task runs once, after
  all its dependencies.
- **Results** go to per-kind slot vectors (`OnceLock<T>` indexed by the
  file, folder or module ID). A task reads only slots of tasks it depends
  on, so reads never race with writes.
- **`HeaderCheck(F)` (stage B of
  [resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages)).**
  One per folder of the program graph, std and dependencies included.
  It depends on `FolderIface(F)` and on the `FolderIface` of each folder
  F's uses reach, since it needs their frozen impl tables and the
  candidate directory. It checks written header types against their
  bounds, impl supertraits and supertrait bindings, derived newtype
  bases and delegation targets, with one fuel budget per item. Nothing
  in checking waits for it: its answers never change what a dependent
  reports. `PackageResult` waits for it, and so does every `Collect`
  (§11.3) for the folders its program reaches, so no program is built
  over an unchecked header. Its result is a part of the package's
  `graph` entry (cache.md §5.2), found by its part key before it runs.
- **Closures and impl universes (Codex re-review N1).** Once the folder
  graphs are built, the driver computes each folder's closure as a bit
  set over the program graph's folders, bottom-up. `ModulePrep(m)` builds
  `closure(m)` for the ordinary role and `test_closure(m)` for the test
  role, with test-only uses added (cache.md §5.3). Then each solving context gets one
  `ImplUniverseId`: the sorted list of the folders in its closure whose
  `arg_impls` section is not empty, interned once per run
  ([trait-solver.md §3.2](trait-solver.md#32-owner-modules)).
  - The contexts: each module's ordinary and test-role bodies, computed
    before any `Body(m)` starts; each
    `HeaderCheck(F)`, from `closure(F)` when the task starts; and each
    derive instance, which uses its module's id.
  - Equal lists get one id, so most modules of a package share one.
  - The id is a run ID. It keys the solver memo and never reaches a
    cache key or output; the keys hash the closure lists instead.
  - Every folder in a context's closure is already a dependency of its
    task, through the `FolderIface` chain, so universes add no edge.
- **Test-role checks (M4c gap 3).** A test run uses the ordinary
  `ModulePrep`, `Body` and `ModuleFinish` tasks with role `test`. The role's
  `check` entry stores synthesized `module.$test<i>` items, registrations
  and TIR; there is no `TestOverlay` task or cache entry.
- **Cache checks are tasks too.** A key can be computed only when the
  deep hashes it names are known, so "compute key, look up, skip or run"
  is the first step of each `FolderIface` and `ModulePrep` task. A hit
  marks the subtree below it done without creating it.
- **The check lookup is `ModulePrep`'s first step (walking skeleton,
  SK-4).** `check_key(m)` needs only the closure's deep hashes and
  `source_hash(m)`. So `ModulePrep(m)` looks it up before anything else,
  and creates `Body(m)` and `ModuleFinish(m)` only on a miss. A lookup at
  `ModuleFinish` would come after the work it could save.
- **`Parse(f)` is created on demand.** A hit needs no parse. The task
  that misses, `FolderIface(F)` or `ModulePrep(m)`, creates `Parse(f)`
  for each file it reads, and a file parsed once in a run is not parsed
  again. `Parse(f)` is not a static predecessor of `ModulePrep`.

### 6.2 Executors

```rust
pub trait Spawn {
    fn add(&self, kind: TaskKind) -> TaskId;
    fn edge(&self, dependency: TaskId, dependent: TaskId);
    fn add_held(&self, kind: TaskKind) -> CreationGuard;
}
pub trait Scheduler: Sync {
    fn run(
        &self,
        graph: &TaskGraph,
        exec: &(dyn Fn(TaskId, TaskKind, &dyn Spawn) + Sync),
    );
    fn threads(&self) -> usize;
}
pub struct SerialScheduler { order: SerialOrder }       // Fifo | Shuffled(seed): tests only
pub struct PoolScheduler { pool: rayon::ThreadPool }    // feature "threads"
pub struct SteppingScheduler { .. }                     // browser: run_for(max_steps) -> Progress
```

Every executor uses that one `exec(TaskId, TaskKind, &dyn Spawn)` task
signature. The serial and stepping executors implement `Spawn` over their
mutable graph; the pool implements it over the atomic graph. `add_held`
returns the creation guard that keeps a task unready until all of its
edges exist
([reconciliation, item 4](reconciliation.md#design-changes-proposed)).

- **Pool.** Ready tasks are spawned into rayon's work-stealing pool. Each
  worker keeps its own bump arena for body tasks. The default thread
  count is `min(cores, 8)`, changed by `--jobs` or `HD_JOBS`
  (open-questions.md, answered).
- **Granularity: items are the logical unit, not the scheduling unit**
  (orchestrator, 2026-10-07, after the owner asked about steal
  thrashing). One rayon task per body would cost about 0.2 to 1 µs of
  spawn, steal and completion work on bodies that check in a few to tens
  of µs (measured on `lib/std` by the systems review: mean 240 bytes and
  37 tokens per body, median 118 bytes and 19 tokens), scatter a module's
  bodies across cores away from its shared scope tables, and stretch the
  per-module joins (M3, `ModuleFinish`). So:
  1. The task graph is per module: parse, interface, body check and
     finish tasks per module, never one graph node per item.
  2. Inside a module's body task, the bodies run as one rayon parallel
     iterator, which splits only when another worker steals (half the
     remaining range, not one item). Bodies are ordered by their byte
     length from skim's `BodyRange`, the cost estimate (skim keeps no
     tokens, and the measured bytes-per-token ratio is steady), with a
     minimum split size of about
     0.5 to 1 ms of estimated work; a very large body runs alone, and a
     small module runs as one sequential task.
  3. Worker arenas reset per batch, not per item.
  4. `Collect` performs the code-entry lookups. For each folder group
     with at least one miss it creates one `ExtTask::Emit(GroupId)`;
     that task emits the missed instances as a batch. The folder group is
     the `codepack` cache unit (cache.md §5.2)
     ([reconciliation, item 5](reconciliation.md#design-changes-proposed)).
  Determinism, fuel, cache keys and TIR stay per item; only scheduling
  coarsens. `parallel-speedup` (at least 0.6x per core up to 8 cores)
  and the scheduler overhead are measured in slice 4.
  - **The numbers behind this (estimates, 2026-10-07).** Bodies in
    `lib/std` plus the dogfood examples (1,239 bodies): p25 8 tokens,
    p50 21, p75 50, p90 94, p99 322, mean 43. Half the bodies have 20
    tokens or fewer but hold only 10% of the tokens; the largest 10%
    hold 45%. At an assumed 0.5 to 2 µs of checking per token and about
    1 to 1.5 µs of overhead per scheduled task, a tiny body pays 15 to
    35% overhead, a median one 5 to 10%, and the whole set about 3 to 4%.
    So one task per body is not classic steal thrashing (which needs
    tasks under about 5 µs on average), but it wastes the most where the
    work is least, and the graph pays per-node bookkeeping. A module's
    bodies as one adaptive parallel iterator removes that cost and still
    spreads the few large bodies. Slice 3 measures the checker's µs per
    token and slice 4 the per-task overhead; the minimum split size is
    set from those, not from the 0.5 to 1 ms guess above.
- **Serial.** One ready queue ordered by priority, then creation order.
  It is the `--threads 1` mode and the browser's base. Its `Shuffled` mode
  picks among ready tasks by a seeded random choice, which finds order
  dependence without threads (§8.1).
- **Stepping (mine).** The browser runs the serial scheduler in slices:
  `run_for(steps)` returns to JavaScript between slices, so the worker can
  receive a "source changed" message and cancel the run. A body cannot
  yield inside itself (Codex re-review N-B5), so one solver-heavy body
  can hold the worker for as long as its fuel allows.
- **A step is one body, not one task (systems review, finding 7).** The
  granularity rule above makes a module's bodies one task. That is right
  for threads, but a playground program is usually one module, so one
  step would be the whole body check: an estimated 15 to 75 ms in Wasm
  for 1,000 lines, and over 200 ms for a 3,000-line paste. So the serial
  and stepping executors run a module's body batch as a loop with a
  cursor, one body per step:
  1. `Body(m)` keeps the index of the next body in its task state. A
     step checks one body, advances the cursor, and returns to the
     executor, which may end the slice there.
  2. M1's omitted-result walk keeps its depth-first stack in the task
     state, so it resumes the same way, one function per step.
  3. M3's worklist and row sweep yield every 4,096 items, the fuel
     check's interval.
  4. The batch is a scheduling unit for threads only. Under the pool
     scheduler, `Body(m)` still runs as one parallel iterator. The
     stepping loop visits bodies in the same order the serial executor
     does, and checking order never reaches output (§6.5).
  So the longest uninterruptible step is the largest body, bounded by its
  fuel, not the largest module. A slice runs steps until about 8 ms have
  passed, then returns to JavaScript; one return costs tens of µs, so a
  1,000-line program pays well under 1 ms for about 100 returns.
- **Staleness.** When an edit arrives, the run is cancelled at the next
  return to JavaScript. The page terminates the compiler worker only if
  the run is stale **and** has not returned for 200 ms, which now means
  one body near its fuel limit, not a large module. A restart reloads
  `hd_web` and the IndexedDB entries and rebuilds the interners, an
  estimated 50 to 300 ms, so it stays the rare case. Entries already
  written back to IndexedDB survive (cache.md §5.8); only the cancelled
  run's new entries are lost.

| Program | Longest step, one task per module | Longest step, one body per step |
| --- | --- | --- |
| 1,000 lines, one module (estimates, Wasm at 1.5 to 2.5x native) | 15 to 75 ms | the largest body: under 2 ms for a p99 body (1,750 bytes), more only near the fuel limit |
| 3,000-line paste | 50 to 200 ms or more; crosses the 200 ms kill | the same as above |

### 6.3 Priority

Priority only affects speed, never output.

- `FolderIface` tasks are ordered by their height in the folder DAG,
  longest path to a sink first, so the critical path starts early.
- `Body` tasks are ordered by body size, largest first (longest
  processing time first), so one huge body does not start last.
- Tasks that unblock many others (`ModulePrep`, `FolderGraph`) come first.

### 6.4 Budgets, Cancellation And The Memory Cap

- **Fuel** is per body (§4.15). A memo hit charges its stored steps.
- **Cancellation** is an atomic flag. A task checks it when it starts,
  and fuel checks it every 4,096 steps. It is set by the stepping
  scheduler on a newer edit, by the opt-in memory cap, and by Ctrl-C.
  `--max-errors` does not cancel checking: output is in content order,
  and counts in the summary stay exact.
- **A panic in a task** is caught at the task boundary, reported as an
  internal error naming the task's item, and the rest of the run goes on.
  The command still exits with status 101.
- **The memory cap** is opt-in (§4.15).

### 6.5 Deterministic Output Assembly

1. **Content order everywhere.** Anything printed, hashed or used to
   break a tie is ordered by stable paths, source positions and codes,
   never by IDs, pointers or hash-map order (lesson 2). IDs have no `Ord`
   (§3.1), and output maps are `BTreeMap`s keyed by content.
2. **Local counters.** Inference variables, closure indices and generated
   names are numbered per body or per item, never per session (rustc's
   leaked allocation IDs).
3. **Print at the end (first release).** Diagnostics are grouped by
   module and printed, sorted, after every task that can produce one has
   finished. Folder, coherence and init diagnostics belong to the module
   of their primary span, and those tasks can finish after the module's
   own bodies. A per-module release cursor would need a barrier on every
   such producer (Codex review, D2). A warm check targets 50 ms, so
   printing at the end costs an agent nothing it would notice. Streaming
   with complete barriers is a later option.
4. **Test results do stream.** A release cursor walks cases in content
   order and prints a case's result once it and every earlier case have
   finished (§19.3). No later task adds to a finished case, so the cursor
   needs no other barrier.
5. **Cycle diagnostics** name the cycle's first member in source or path
   order, whichever thread found it.
6. **The summary** comes last, with counts over the whole run.
