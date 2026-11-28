# New Compiler Design: Scheduler And Task Graph

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 6. Scheduler And Task Graph

### 6.1 Tasks

```rust
pub enum TaskKind {                         // payloads: dense run IDs (file, unit, module, slot)
    Skim(u32), Parse(u32), UnitGraph,
    UnitIface(u32), HeaderCheck(u32),
    ModulePrep(u32), Body(u32), ModuleFinish(u32),
    Coherence, InitOrder(u32), PackageResult,
    Ext(ExtTask),                           // Collect, Link, Precompile, RunCase(case)
}
struct TaskNode { kind, waiting_on: u32, successors: Vec<TaskId>, state, priority: u32, ready_seq: u64 }
pub struct TaskGraph { nodes: Vec<TaskNode>, ready: ReadyQueue, seq: u64, finished: usize, work: u64 }
enum ReadyQueue { Fifo(VecDeque<Entry>), Priority(BinaryHeap<(u32, Reverse<usize>, u64)>), Shuffled(u64, Vec<Entry>) }
```

`hd_sched` owns the graph and the executors; `hd_driver` (`Run::exec`)
owns what each task does.

- **Creation is dynamic.** The driver creates one `Skim` per file (and
  one `Parse` per file for `hd check`), then `UnitGraph` after the
  skims. A running task may add tasks and edges: `UnitGraph` adds a
  `UnitIface` and a `HeaderCheck` per built unit, one `ModulePrep`
  per built module and a held `PackageResult`; on a cache miss, `ModulePrep`
  checks the bodies and puts the result before returning (#1981).
  It cannot yield an unpublished check claim to an unrelated task. Modules
  and independent bodies within a module still run in parallel.
  `PackageResult` looks up the
  package's `graph` entry and, on a miss, adds one `Coherence` and one
  `InitOrder` per unit, then `Collect` after them. `Collect` looks up
  the program's `link` entry; on a miss it collects the instances and
  adds one `ExtTask::Link`, which emits and links the whole module on one
  thread. `Precompile` and `RunCase` are scheduled but do nothing.
  Planned, not built: one `Emit` task per unit group with a code-entry
  miss, the `codepack` cache unit (crates/hd_cache/DESIGN.md §5.4, #49
  point 4). An edge from a task that is already done counts as satisfied
  at once.
- **The creation guard.** `add_held` creates a task with one extra wait
  count, dropped by `release`, so edges added after creation cannot race
  its start (docs/compiler/data-structures.md §3.21). An edge into a
  started task is a compiler bug and asserts.
- **Results** go to per-kind slot vectors (`OnceLock` indexed by the
  file, unit or module ID). A task reads only slots of tasks it depends
  on, so reads never race with writes.
- **`HeaderCheck(F)` (stage B of
  [crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages)).**
  One per built unit. It depends on F's own `UnitIface`, which waits
  for the interfaces of the units F uses, so for F's whole closure. It checks written header types against their bounds,
  impl supertraits and supertrait bindings, derived newtype bases and
  delegation targets, and reports F's use diagnostics (#248). Nothing in
  body checking waits for it: a module's `ModulePrep` notes whether a
  use of the module reports an error, which is what its `Body` and
  `ModuleFinish` ask.
  `PackageResult` waits for it, and `Collect` is created only after
  `PackageResult`, so no program is built over an unchecked header. (The
  design had each `Collect` wait for the `HeaderCheck`s of the units its
  program reaches; one program per run makes the two equal, and the
  built form needs no per-program edge list.) Its
  findings and use diagnostics are a `graph` entry keyed by `hdr_key`
  (crates/hd_cache/DESIGN.md §5.2), looked up before it runs.
- **Closures and impl universes.** A unit's closure is found on demand,
  a walk over the module graph's unit edges when a task asks; no per-unit
  set is kept. Each solving
  context (a module's bodies, a `HeaderCheck`) interns the units of
  its closure that carry argument-owned impls into one `ImplUniverseId`
  ([crates/hd_check/DESIGN.md (trait solver) §3.2](../hd_check/DESIGN.md#32-owner-units)).
  - Equal lists get one id, so most modules of a package share one.
  - The id is a run ID. It keys the solver memo and never reaches a
    cache key or output; the keys hash the closure lists instead.
  - Every unit in a context's closure is already a dependency of its
    task, so universes add no edge: through the `UnitIface` chain, or
    in the test role through `ModulePrep(m)`'s edges to the interfaces
    of the units its `tests:` block uses, whose own chains cover the rest.
- **Test-role checks.** A test run uses the ordinary `ModulePrep`,
  `Body` and `ModuleFinish` tasks with role `test` for the root
  package's modules. A module's closure is `Run::module_closure(m)`,
  walked from the module graph's units when a task asks: `closure(m)` in the
  ordinary role, and in the test role `test_closure(m)`, its unit's
  closure joined with the closure of each module its `tests:` block
  uses, dev dependencies and `std.testing` included
  (crates/hd_cache/DESIGN.md §5.3). The check key, the body check and
  its impl universe all read it, so a `tests:` block solves over the
  argument-owned impls of units only test code reaches. The role's
  `check` entry holds the `tests:` block's items; there is no
  `TestOverlay` task. A `tests:` block's uses make no unit edge, so
  the test role's `ModulePrep(m)` waits for the interfaces of
  `test_closure(m)`.
- **Cache checks are tasks too.** A key can be computed only when the
  deep hashes it names are known, so "compute key, look up, skip or run"
  is the first step of each `UnitIface`, `HeaderCheck` and
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
pub fn run_pool(graph: TaskGraph, threads: usize, exec: &Exec) -> (TaskGraph, usize) // feature "threads"
```

Every executor runs the one `Exec` task signature, so task code does not
depend on the executor. Each run returns the number of tasks that never
became ready; the driver reports a nonzero count as an internal error.

- **Serial.** One ready queue. `Fifo` takes tasks in the order they
  became ready, `Priority` takes the highest priority first, then
  creation order, and `Shuffled` picks by a seeded choice, which finds
  order dependence without threads (§8.1). The queue takes the shape of
  the order it is popped in: a deque, a binary heap on (priority,
  earliest creation), or a bag emptied by `swap_remove`. Each readiness
  pushes one entry stamped with its readiness number; an `edge` into a
  ready task returns it to waiting and leaves its entry dead, and a pop
  drops dead entries as it meets them (lazy deletion). So a pop costs
  O(1), or O(log n) for `Priority`, and an edge O(1), where the scan of
  the whole queue per pop made n ready tasks cost n² (#2032). The CLI runs it for one
  thread; `hd_web` and the tests run it too.
- **Pool.** `run_pool` starts `threads` graph threads and a rayon pool
  of `threads` map workers, each with `WORKER_STACK`. The graph sits
  behind one mutex, held only to pop, add, edge, release or complete;
  each graph thread pops the next ready task, in readiness order. A
  task's batched map (`par_map`) runs on the map pool, which the graph
  thread enters from outside and waits on without running other work.
  So every task starts at the bottom of a graph thread's stack and every
  map item at the bottom of a map worker's, however busy the machine is
  (#2423): a rayon worker waiting in a `join` runs queued jobs on its own
  frames, and when tasks were rayon jobs a task waiting in its map ran
  other whole tasks there. A map item, a nested graph and any thread
  outside a run map in order; a nested `run_pool` runs on its caller. A
  caller on a rayon worker only waits, so the run's depth never depends
  on the caller's stack. A task's panic stops the run and resumes on the
  caller. This replaces a design with per-node atomics and successor
  locks; at module granularity a run has hundreds of tasks, so one
  short-held lock does not contend and is far simpler to get right. If the pool cannot be built, the graph runs serially. The CLI
  uses it when the thread count is above 1: `--jobs N`, else `HD_JOBS`,
  else the number of cores, at most 8.
- **Granularity: items are the logical unit, not the scheduling unit.**
  The task graph is per module, never one node per item: a module's
  bodies are one `Body` task. Inside it, a large module's bodies run as
  one batched parallel iterator on the task's pool (`par_map`), and a
  small module's run in order. One task per
  body would pay about 1 µs of spawn and completion work on bodies that
  check in a few to tens of µs (half of `lib/std`'s bodies have 20 tokens
  or fewer), and would scatter a module's bodies away from its shared
  scope tables. Determinism, fuel, cache keys and TIR stay per item;
  only scheduling coarsens.

Planned, not built: a stepping executor, the serial one in slices, for
browser stepping that returns to JavaScript about
every 8 ms and cancels a stale run on an edit, with a step of one body
rather than one task (a `Body` task keeps a cursor into its bodies), so
the longest uninterruptible step is the largest body. Also planned:
splitting a large module's `Body` task into a rayon parallel iterator
ordered by body size.

### 6.3 Priority

Priority only affects speed, never output. Tasks carry a `priority`
and the serial `Priority` order honors it, but the driver creates every
task at priority 0, so `Priority` runs tasks in creation order.

Planned, not built: `UnitIface` tasks ordered by height in the unit
DAG, `Body` tasks by body size (largest first), and tasks that unblock
many others (`ModulePrep`, `UnitGraph`) first.

### 6.4 Budgets, Cancellation And The Memory Cap

- **Fuel** is per body (§4.15). A memo hit charges its stored steps.
- **Cancellation** is not built: every executor runs a graph to the end.
  `--max-errors` does not cancel checking: output is in content order, and
  counts in the summary stay exact.
- **The memory cap** is opt-in (§4.15). `hd_command` counts compiler heap
  bytes and exits the process with a report when the cap is crossed,
  cancelling no task.

Planned, not built: a panic in a task caught at the task boundary and
reported as an internal error naming the task's item, with the rest of
the run going on; a cancel flag that the stepping executor and fuel
check; Ctrl-C cancelling a build.

### 6.5 Deterministic Output Assembly

1. **Content order everywhere.** Anything printed, hashed or used to
   break a tie is ordered by stable paths, source positions and codes,
   never by IDs, pointers or hash-map order. IDs have no `Ord`
   (§3.1), and output maps are `BTreeMap`s keyed by content.
2. **Local counters.** Inference variables, closure indices and generated
   names are numbered per body or per item, never per session.
3. **Print at the end.** Diagnostics are collected and printed, sorted,
   after every task that can produce one has finished. Unit, coherence
   and init diagnostics can finish after the module's own bodies, so a
   per-module release would need a barrier on each such producer.
4. **Test results do stream.** `hd test`'s `ReleaseCursor` walks cases in
   content order and prints a case's result once it and every earlier
   case have finished (§19.3).
5. **Cycle diagnostics** name the cycle's first member in source or path
   order, whichever thread found it.
6. **The summary** comes last, with counts over the whole run.
