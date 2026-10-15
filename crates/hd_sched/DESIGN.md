# New Compiler Design: Scheduler And Task Graph

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 6. Scheduler And Task Graph

### 6.1 Tasks

```rust
pub enum TaskKind {                         // payloads: dense run IDs (file, folder, module, slot)
    Skim(u32), Parse(u32), FolderGraph,
    FolderIface(u32), HeaderCheck(u32),
    ModulePrep(u32), Body(u32), ModuleFinish(u32), TestOverlay(u32),
    Coherence, InitOrder(u32), PackageResult,
    Ext(ExtTask),                           // Collect, Emit(slot), Link, Precompile, RunCase(case)
}
struct TaskNode { kind, waiting_on: u32, successors: Vec<TaskId>, state, priority: u32 }
pub struct TaskGraph { nodes: Vec<TaskNode>, ready: VecDeque<TaskId> }
```

`hd_sched` owns the graph and the executors; `hd_driver` (`Run::exec`)
owns what each task does.

- **Creation is dynamic.** The driver creates one `Skim` per file (and
  one `Parse` per file for `hd check`), then `FolderGraph` after the
  skims. A running task may add tasks and edges: `FolderGraph` adds a
  `FolderIface` and a `HeaderCheck` per built folder, one `Coherence`,
  one `ModulePrep` per built module and a held `PackageResult`;
  `ModulePrep` adds `Body`, `ModuleFinish` and `TestOverlay` on a cache
  miss; `PackageResult` adds one `InitOrder` per folder and then
  `Collect`. `Collect` performs the code-entry lookups and adds a held
  `Link`; for each folder group with at least one miss it creates one
  `ExtTask::Emit(GroupId)`, which emits the missed instances as a batch.
  The folder group is the `codepack` cache unit
  (crates/hd_cache/DESIGN.md §5.2). An edge from a task that is already
  done counts as satisfied at once.
  Implementation differs: `Collect` creates one `Emit` per missed
  instance slot, which is worse: a cold build makes thousands of
  single-instance tasks, each paying graph bookkeeping.
- **The creation guard.** `add_held` creates a task with one extra wait
  count, dropped by `release`, so edges added after creation cannot race
  its start (docs/compiler/data-structures.md §3.21). An edge into a
  started task is a compiler bug and asserts.
- **Results** go to per-kind slot vectors (`OnceLock` indexed by the
  file, folder or module ID). A task reads only slots of tasks it depends
  on, so reads never race with writes.
- **`HeaderCheck(F)` (stage B of
  [crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages)).**
  One per built folder. It depends on the `FolderIface` of every folder
  in F's closure. It checks written header types against their bounds,
  impl supertraits and supertrait bindings, derived newtype bases and
  delegation targets. Nothing in body checking waits for it.
  `PackageResult` waits for it, and `Collect` is created only after
  `PackageResult`, so no program is built over an unchecked header. (The
  design had each `Collect` wait for the `HeaderCheck`s of the folders its
  program reaches; one program per run makes the two equal, and the
  built form needs no per-program edge list.) Its
  findings are a `graph` entry keyed by `hdr_key`
  (crates/hd_cache/DESIGN.md §5.2), looked up before it runs.
- **Closures and impl universes.** `FolderGraph` computes each folder's
  closure as a bit set over the program graph's folders. Each solving
  context (a module's bodies, a `HeaderCheck`) interns the folders of
  its closure that carry argument-owned impls into one `ImplUniverseId`
  ([crates/hd_check/DESIGN.md (trait solver) §3.2](../hd_check/DESIGN.md#32-owner-folders)).
  - Equal lists get one id, so most modules of a package share one.
  - The id is a run ID. It keys the solver memo and never reaches a
    cache key or output; the keys hash the closure lists instead.
  - Every folder in a context's closure is already a dependency of its
    task, through the `FolderIface` chain, so universes add no edge.
- **Test-role checks.** A test run uses the ordinary `ModulePrep`,
  `Body` and `ModuleFinish` tasks with role `test` for the root
  package's modules. `ModulePrep(m)` builds `closure(m)` for the
  ordinary role and `test_closure(m)` for the test role, with test-only
  uses added (crates/hd_cache/DESIGN.md §5.3). The role's `check` entry
  holds the `tests:` block's items; there is no `TestOverlay` task.
  - Implementation differs: there is one closure per module, and a
    `tests:` block's uses into other packages (dev dependencies,
    `std.testing`) join it for both roles; its uses of the same package
    make no edge. Not judged: one closure is simpler and sound for other
    packages, but whether a same-package test-only use is always in the
    key needs checking.
  - Implementation differs: a `TestOverlay(m)` task is still created per
    module and checks nothing, which is worse: a dead task per module.
- **Cache checks are tasks too.** A key can be computed only when the
  deep hashes it names are known, so "compute key, look up, skip or run"
  is the first step of each `FolderIface`, `HeaderCheck` and
  `ModulePrep` task. `ModulePrep(m)` computes `check_key(m)` from the
  closure's deep hashes and the module's source, and creates `Body(m)`
  and `ModuleFinish(m)` only on a miss.
- **Parsing is on demand.** Outside `hd check`, no `Parse` task is
  created. The task that misses parses the file it reads through a
  per-run slot, so a file parsed once in a run is not parsed again.

### 6.2 Executors

```rust
pub trait Spawn {
    fn add(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId;
    fn edge(&self, from: TaskId, to: TaskId);
    fn add_held(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId;
    fn release(&self, id: TaskId);
}
pub type Exec<'a> = dyn Fn(TaskId, TaskKind, &dyn Spawn) + Sync + 'a;
pub struct SerialScheduler { pub order: SerialOrder }   // Fifo | Priority | Shuffled(seed)
pub struct SteppingScheduler { pub order: SerialOrder, pub cancel: CancelFlag }
pub fn run_pool(graph: TaskGraph, threads: usize, exec: &Exec) -> (TaskGraph, usize) // feature "threads"
```

Every executor runs the one `Exec` task signature, so task code does not
depend on the executor. Each run returns the number of tasks that never
became ready; the driver reports a nonzero count as an internal error.

- **Serial.** One ready queue. `Fifo` takes tasks in the order they
  became ready, `Priority` takes the highest priority first, then
  creation order, and `Shuffled` picks by a seeded choice, which finds
  order dependence without threads (§8.1). The CLI runs it for one
  thread; `hd_web` and the tests run it too.
- **Pool.** `run_pool` builds a rayon pool of `threads` workers. The
  graph sits behind one mutex, held only to add, edge, release or
  complete; each task that becomes ready is spawned into the pool's
  scope. This replaces a design with per-node atomics and successor
  locks; at module granularity a run has hundreds of tasks, so one
  short-held lock does not contend and is far simpler to get right. If the pool cannot be built, the graph runs serially. The CLI
  uses it when the thread count is above 1: `--jobs N`, else `HD_JOBS`,
  else the number of cores, at most 8.
- **Granularity: items are the logical unit, not the scheduling unit.**
  The task graph is per module, never one node per item: a module's
  bodies are one `Body` task and run in order inside it. One task per
  body would pay about 1 µs of spawn and completion work on bodies that
  check in a few to tens of µs (half of `lib/std`'s bodies have 20 tokens
  or fewer), and would scatter a module's bodies away from its shared
  scope tables. Determinism, fuel, cache keys and TIR stay per item;
  only scheduling coarsens.
- **Stepping.** `SteppingScheduler::run_for(max_steps)` runs the serial
  executor for at most `max_steps` tasks and returns `Done`,
  `Yielded { ran }` or `Cancelled`; the cancel flag is checked before
  each task. Nothing uses it yet: `hd_web` runs a check to completion.

Planned, not built: browser stepping that returns to JavaScript about
every 8 ms and cancels a stale run on an edit, with a step of one body
rather than one task (a `Body` task keeps a cursor into its bodies), so
the longest uninterruptible step is the largest body. Also planned:
splitting a large module's `Body` task into a rayon parallel iterator
ordered by body size.

### 6.3 Priority

Priority only affects speed, never output. Tasks carry a `priority`
and the serial `Priority` order honors it, but the driver creates every
task at priority 0, so `Priority` runs tasks in creation order.

Planned, not built: `FolderIface` tasks ordered by height in the folder
DAG, `Body` tasks by body size (largest first), and tasks that unblock
many others (`ModulePrep`, `FolderGraph`) first.

### 6.4 Budgets, Cancellation And The Memory Cap

- **Fuel** is per body (§4.15). A memo hit charges its stored steps.
- **Cancellation** is `CancelFlag`, an atomic flag; only the stepping
  executor reads it. `--max-errors` does not cancel checking: output is
  in content order, and counts in the summary stay exact.
- **The memory cap** is opt-in (§4.15) and sets the cancellation flag.
  Implementation differs: `hd_cli` counts compiler heap bytes and exits
  the process with a report when the cap is crossed, cancelling no task;
  not judged.

Planned, not built: a panic in a task caught at the task boundary and
reported as an internal error naming the task's item, with the rest of
the run going on; fuel checking the cancel flag; Ctrl-C cancelling a
build.

### 6.5 Deterministic Output Assembly

1. **Content order everywhere.** Anything printed, hashed or used to
   break a tie is ordered by stable paths, source positions and codes,
   never by IDs, pointers or hash-map order. IDs have no `Ord`
   (§3.1), and output maps are `BTreeMap`s keyed by content.
2. **Local counters.** Inference variables, closure indices and generated
   names are numbered per body or per item, never per session.
3. **Print at the end.** Diagnostics are collected and printed, sorted,
   after every task that can produce one has finished. Folder, coherence
   and init diagnostics can finish after the module's own bodies, so a
   per-module release would need a barrier on each such producer.
4. **Test results do stream.** `hd test`'s `ReleaseCursor` walks cases in
   content order and prints a case's result once it and every earlier
   case have finished (§19.3).
5. **Cycle diagnostics** name the cycle's first member in source or path
   order, whichever thread found it.
6. **The summary** comes last, with counts over the whole run.
