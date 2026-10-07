# New Compiler Design: Scheduler And Task Graph

Part of the [compiler design](README.md).

## 6. Scheduler And Task Graph

### 6.1 Tasks

```rust
pub enum TaskKind {
    Skim(FileId), Parse(FileId), FolderGraph(PackageId),
    FolderIface(FolderId), HeaderCheck(FolderId),
    ModulePrep(ModuleId), Body(ModuleId, ItemIdx), ModuleFinish(ModuleId),
    TestOverlay(ModuleId), Coherence(DefId), InitOrder(FolderId), PackageResult(PackageId),
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
  that is already done counts as satisfied at once.
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
  over an unchecked header. Its cache entry is `hdr` (cache.md §5.2),
  looked up before it runs like any other.
- **Cache checks are tasks too.** A key can be computed only when the
  deep hashes it names are known, so "compute key, look up, skip or run"
  is the first step of each `FolderIface` and `ModuleFinish` path. A hit
  marks the subtree below it done without creating it.

### 6.2 Executors

```rust
pub trait Scheduler: Sync {
    fn run(&self, graph: &TaskGraph, exec: &(dyn Fn(TaskId, &Spawner) + Sync));
    fn threads(&self) -> usize;
}
pub struct SerialScheduler { order: SerialOrder }       // Fifo | Shuffled(seed): tests only
pub struct PoolScheduler { pool: rayon::ThreadPool }    // feature "threads"
pub struct SteppingScheduler { .. }                     // browser: run_for(max_steps) -> Progress
```

- **Pool.** Ready tasks are spawned into rayon's work-stealing pool. Each
  worker keeps its own bump arena for body tasks. The default thread
  count is `min(cores, 8)`, changed by `--jobs` or `HD_JOBS`
  (open-questions.md, answered).
- **Serial.** One ready queue ordered by priority, then creation order.
  It is the `--threads 1` mode and the browser's base. Its `Shuffled` mode
  picks among ready tasks by a seeded random choice, which finds order
  dependence without threads (§8.1).
- **Stepping (mine).** The browser runs the serial scheduler in slices:
  `run_for(steps)` returns to JavaScript between slices, so the worker can
  receive a "source changed" message and cancel the run.

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
