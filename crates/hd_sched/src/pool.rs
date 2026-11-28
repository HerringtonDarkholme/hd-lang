//! The pool executor (§6.2), feature `threads`. `threads` graph threads
//! each take the next ready task from the graph, which sits behind one
//! mutex, held only to pop, add, edge, release or complete. Tasks that
//! receive edges after creation are created behind their guard
//! (`add_held`), so no edge can reach a started task.
//!
//! A task's batched maps ([`crate::par_map`]) run on the run's rayon pool,
//! which a graph thread enters from outside: it sleeps until the map is
//! done and runs nothing else meanwhile. So each task starts at the bottom
//! of a graph thread's stack, and each map item at the bottom of a map
//! worker's, whatever the other threads do (#2423). A rayon worker that
//! waits in a `join` runs any queued job on top of its own frames; when
//! tasks were rayon jobs, a task waiting in its map ran other whole tasks
//! there, as deep as the machine was busy.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Condvar, Mutex};

use crate::{Exec, SerialOrder, Spawn, TaskGraph, TaskId, TaskKind};

thread_local! {
    static CALLER_RUN: Cell<bool> = const { Cell::new(false) };
    /// The map pool of the run whose graph thread this is.
    static MAPS: RefCell<Option<Arc<rayon::ThreadPool>>> = const { RefCell::new(None) };
    /// Whether this thread is a map pool's worker.
    static MAP_WORKER: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn on_caller() -> bool {
    CALLER_RUN.get()
}

/// The map pool a batched map on this thread runs on: the run's, on a
/// graph thread outside a nested graph; none on any other thread.
pub(crate) fn maps() -> Option<Arc<rayon::ThreadPool>> {
    if CALLER_RUN.get() {
        return None;
    }
    MAPS.with_borrow(Clone::clone)
}

struct CallerRun(bool);

impl Drop for CallerRun {
    fn drop(&mut self) {
        CALLER_RUN.set(self.0);
    }
}

fn run_on_caller(mut graph: TaskGraph, exec: &Exec<'_>, order: SerialOrder) -> (TaskGraph, usize) {
    let _caller = CallerRun(CALLER_RUN.replace(true));
    let left = crate::SerialScheduler { order }.run(&mut graph, exec);
    (graph, left)
}

fn lock<T>(g: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    g.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The run's shared state: the graph, the tasks running, and the first
/// task panic, which stops the run and resumes on the caller.
struct Run {
    graph: TaskGraph,
    running: usize,
    failed: Option<Box<dyn Any + Send>>,
}

struct Shared {
    run: Mutex<Run>,
    /// Signalled when a task may have become ready or the run ended.
    changed: Condvar,
}

impl Shared {
    fn after<T>(&self, f: impl FnOnce(&mut TaskGraph) -> T) -> T {
        let v = f(&mut lock(&self.run).graph);
        self.changed.notify_all();
        v
    }
}

impl Spawn for Shared {
    fn add(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.after(|g| g.add(kind, deps))
    }
    fn edge(&self, from: TaskId, to: TaskId) {
        self.after(|g| g.edge(from, to));
    }
    fn add_held(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.after(|g| g.add_held(kind, deps, 0))
    }
    fn release(&self, id: TaskId) {
        self.after(|g| g.release(id));
    }
}

/// A graph thread: runs ready tasks, in readiness order, until none is
/// ready or running, or a task panicked. Cost per task: one pop and one
/// completion under the lock, and one wakeup of each idle graph thread
/// per change that may ready a task.
fn graph_thread(shared: &Shared, maps: &Arc<rayon::ThreadPool>, exec: &Exec<'_>) {
    MAPS.set(Some(Arc::clone(maps)));
    let mut run = lock(&shared.run);
    while run.failed.is_none() {
        let Some(id) = run.graph.pop(SerialOrder::Fifo) else {
            if run.running == 0 {
                break;
            }
            run = shared
                .changed
                .wait(run)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            continue;
        };
        let kind = run.graph.kind(id);
        run.running += 1;
        drop(run);
        let ran = catch_unwind(AssertUnwindSafe(|| exec(id, kind, shared)));
        run = lock(&shared.run);
        run.running -= 1;
        match ran {
            Ok(()) => run.graph.complete(id),
            Err(payload) => {
                run.failed.get_or_insert(payload);
            }
        }
        shared.changed.notify_all();
    }
    shared.changed.notify_all();
    drop(run);
    MAPS.set(None);
}

/// A worker's stack, which `hd` gives its command thread too. A syntax
/// walk recurses once per nesting level, and a chain link is one or two
/// levels. At the parser's limit of 10,000 links (`grammar.limit.chain`),
/// checking `f()()...` needs 12 to 16 MiB in a dev build, and 32 MiB is
/// twice that; a main thread's 8 MiB overflowed. Untouched stack pages
/// cost no RSS, so the size costs address space only. Graph threads and
/// map workers each get one.
pub const WORKER_STACK: usize = 32 * 1024 * 1024;

/// Runs the graph on `threads` graph threads, with `threads` map workers
/// for their tasks' batched maps; returns it with the number of tasks
/// that never became ready (0 unless the graph has a bug). A task's panic
/// stops the run and resumes here. If the threads cannot be made, the
/// graph runs serially. A nested call, from a task or a map item, runs
/// serially on its caller, maps included. Any other caller, a rayon
/// worker included, only waits, so the run's stack depth never depends
/// on the caller's.
pub fn run_pool(graph: TaskGraph, threads: usize, exec: &Exec<'_>) -> (TaskGraph, usize) {
    // A nested graph stays on its caller: its tasks may wait for state the
    // calling task holds, and more threads would oversubscribe the run.
    if CALLER_RUN.get() || MAP_WORKER.get() || MAPS.with_borrow(Option::is_some) {
        return run_on_caller(graph, exec, SerialOrder::Fifo);
    }
    let threads = threads.max(1);
    // Named, so a panic or a stack overflow names the kind of thread.
    let Ok(maps) = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|k| format!("hd-map-{k}"))
        .stack_size(WORKER_STACK)
        .start_handler(|_| MAP_WORKER.set(true))
        .build()
    else {
        return run_on_caller(graph, exec, SerialOrder::Priority);
    };
    let maps = Arc::new(maps);
    let shared = Shared {
        run: Mutex::new(Run {
            graph,
            running: 0,
            failed: None,
        }),
        changed: Condvar::new(),
    };
    let started = std::thread::scope(|scope| {
        (0..threads)
            .map_while(|k| {
                std::thread::Builder::new()
                    .name(format!("hd-task-{k}"))
                    .stack_size(WORKER_STACK)
                    .spawn_scoped(scope, || graph_thread(&shared, &maps, exec))
                    .ok()
            })
            .count()
    });
    let Run { graph, failed, .. } = shared
        .run
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(payload) = failed {
        resume_unwind(payload);
    }
    if started == 0 {
        return run_on_caller(graph, exec, SerialOrder::Priority);
    }
    let left = graph.unfinished();
    (graph, left)
}

#[cfg(test)]
mod tests {
    use super::run_pool;
    use crate::{ExtTask, Spawn, TaskGraph, TaskKind};
    use std::sync::Mutex;

    /// One pool run of a single task, `exec`, on `workers` threads.
    fn in_task(workers: usize, exec: impl Fn() + Sync) {
        let mut graph = TaskGraph::default();
        graph.add(TaskKind::UnitGraph, &[]);
        let (_, left) = run_pool(graph, workers, &|_, _, _| exec());
        assert_eq!(left, 0);
    }

    #[test]
    fn nested_graphs_and_their_maps_stay_on_the_task_thread() {
        for workers in [1, 2] {
            in_task(workers, || {
                let caller = std::thread::current().id();
                assert_eq!(crate::task_threads(), workers);
                let mut graph = TaskGraph::default();
                let first = graph.add(TaskKind::UnitGraph, &[]);
                graph.add(TaskKind::PackageResult, &[first]);
                let ran = Mutex::new(Vec::new());
                let (graph, left) = run_pool(graph, 2, &|_, kind, spawn| {
                    assert_eq!(std::thread::current().id(), caller);
                    assert_eq!(crate::task_threads(), 1);
                    let input = (0..1_000).collect::<Vec<_>>();
                    let output = crate::par_map(&input, |item| {
                        assert_eq!(std::thread::current().id(), caller);
                        item + 1
                    });
                    assert_eq!(output.len(), input.len());
                    super::lock(&ran).push(kind);
                    if kind == TaskKind::PackageResult {
                        let link = spawn.add_held(TaskKind::Ext(ExtTask::Link), &[]);
                        let body = spawn.add(TaskKind::Body(0), &[]);
                        spawn.edge(body, link);
                        spawn.release(link);
                    }
                });
                assert_eq!(left, 0);
                assert_eq!(graph.len(), 4);
                assert_eq!(super::lock(&ran).len(), 4);
                assert!(!super::on_caller());
                assert_eq!(crate::task_threads(), workers);
            });
        }
    }

    /// #2423: a task waiting for its map runs nothing else on its stack,
    /// so a task never starts on another task's frames, and a map item
    /// never on a task's. When tasks were rayon jobs, a task waiting in
    /// its map's `join` ran the queued tasks on top of its own frames.
    #[test]
    fn tasks_and_map_items_never_run_on_a_tasks_frames() {
        use std::cell::Cell;
        use std::sync::atomic::{AtomicUsize, Ordering};
        thread_local! {
            static IN_TASK: Cell<bool> = const { Cell::new(false) };
        }
        let stacked = AtomicUsize::new(0);
        let mut graph = TaskGraph::default();
        for m in 0..4 {
            graph.add(TaskKind::ModulePrep(m), &[]);
        }
        let (_, left) = run_pool(graph, 2, &|_, kind, spawn| {
            if IN_TASK.replace(true) {
                stacked.fetch_add(1, Ordering::Relaxed);
            }
            if let TaskKind::ModulePrep(m) = kind {
                for b in 0..16 {
                    spawn.add(TaskKind::Body(m * 16 + b), &[]);
                }
                let items = (0..64).collect::<Vec<u32>>();
                crate::par_map(&items, |_| {
                    if IN_TASK.get() {
                        stacked.fetch_add(1, Ordering::Relaxed);
                    }
                    std::thread::sleep(std::time::Duration::from_micros(200));
                });
            }
            IN_TASK.set(false);
        });
        assert_eq!(left, 0);
        assert_eq!(stacked.load(Ordering::Relaxed), 0);
    }

    /// A pool run from a rayon worker only waits: its tasks run on the
    /// run's own threads, and the worker runs no job of its own pool
    /// meanwhile, which could wait for a lock its suspended caller holds.
    #[test]
    fn a_run_from_a_rayon_worker_leaves_the_worker_waiting()
    -> Result<(), rayon::ThreadPoolBuildError> {
        use std::sync::TryLockError;
        use std::sync::atomic::{AtomicBool, Ordering};

        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
        let held = Mutex::new(());
        let helped = AtomicBool::new(false);
        pool.scope(|scope| {
            let caller = std::thread::current().id();
            let guard = super::lock(&held);
            // cost: one queued outer task in this regression.
            scope.spawn(|_| {
                helped.store(
                    matches!(held.try_lock(), Err(TryLockError::WouldBlock)),
                    Ordering::SeqCst,
                );
            });
            let mut graph = TaskGraph::default();
            graph.add(TaskKind::UnitGraph, &[]);
            let (_, left) = run_pool(graph, 2, &|_, _, _| {
                assert_ne!(std::thread::current().id(), caller);
                assert_eq!(crate::task_threads(), 2);
            });
            assert_eq!(left, 0);
            drop(guard);
        });
        assert!(!helped.load(Ordering::SeqCst));
        Ok(())
    }

    #[test]
    fn a_task_panic_stops_the_run_and_resumes_on_the_caller() {
        struct TaskFailure;
        let failure = std::panic::catch_unwind(|| {
            let mut graph = TaskGraph::default();
            let first = graph.add(TaskKind::UnitGraph, &[]);
            graph.add(TaskKind::PackageResult, &[first]);
            run_pool(graph, 2, &|_, kind, _| {
                assert_eq!(kind, TaskKind::UnitGraph);
                std::panic::panic_any(TaskFailure)
            })
        });
        assert!(failure.is_err_and(|payload| payload.is::<TaskFailure>()));
    }

    #[test]
    fn nested_graph_unwinding_restores_parallel_maps() {
        in_task(2, || {
            struct TaskFailure;
            let failure = std::panic::catch_unwind(|| {
                let mut graph = TaskGraph::default();
                graph.add(TaskKind::UnitGraph, &[]);
                run_pool(graph, 2, &|_, _, _| std::panic::panic_any(TaskFailure));
            });
            assert!(failure.is_err());
            assert!(!super::on_caller());
            assert_eq!(crate::task_threads(), 2);
        });
    }

    #[test]
    fn nested_graph_work_is_linear_in_width_and_depth() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        in_task(2, || {
            for depth in [1, 20] {
                let work = |width: u32| {
                    let mut graph = TaskGraph::default();
                    for column in 0..width {
                        let mut previous = None;
                        for _ in 0..depth {
                            previous = Some(graph.add(TaskKind::Body(column), previous.as_slice()));
                        }
                    }
                    let ran = AtomicUsize::new(0);
                    let (graph, left) = run_pool(graph, 2, &|_, _, _| {
                        assert_eq!(crate::task_threads(), 1);
                        ran.fetch_add(1, Ordering::Relaxed);
                    });
                    assert_eq!(left, 0);
                    assert_eq!(ran.load(Ordering::Relaxed), graph.len());
                    // One readiness and pop per node, and one edge per
                    // non-root node. No graph scan or nesting retry.
                    assert_eq!(graph.work(), 3 * graph.len() as u64 - u64::from(width));
                    graph.work()
                };
                assert_eq!(work(10_000), 10 * work(1_000));
            }
        });
    }

    #[test]
    fn pool_runs_every_task_once_after_its_dependencies() {
        let mut g = TaskGraph::default();
        let root = g.add(TaskKind::UnitGraph, &[]);
        for f in 0..8 {
            g.add(TaskKind::UnitIface(f), &[root]);
        }
        let log = Mutex::new(Vec::new());
        let (g, left) = run_pool(g, 4, &|_, kind, sp: &dyn Spawn| {
            log.lock().expect("log").push(kind);
            if let TaskKind::UnitIface(f) = kind {
                let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
                let e = sp.add(TaskKind::Body(f), &[]);
                sp.edge(e, link);
                sp.release(link);
            }
        });
        let log = log.into_inner().expect("log");
        assert_eq!(left, 0);
        assert_eq!(log[0], TaskKind::UnitGraph);
        assert_eq!(log.len(), 25);
        assert_eq!(g.len(), 25);
    }
}
