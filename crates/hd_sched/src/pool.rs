//! The pool executor (§6.2), feature `threads`: rayon's work-stealing
//! pool. A task that becomes ready is spawned into the pool's scope; the
//! graph itself sits behind one mutex, held only to add, edge, release or
//! complete. Tasks that receive edges after creation are created behind
//! their guard (`add_held`), so no edge can reach a started task.

use std::cell::Cell;
use std::sync::Mutex;

use crate::{Exec, SerialOrder, Spawn, TaskGraph, TaskId, TaskKind};

thread_local! {
    static CALLER_RUN: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn on_caller() -> bool {
    CALLER_RUN.get()
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

struct PoolSpawn<'a, 's> {
    graph: &'s Mutex<TaskGraph>,
    scope: &'a rayon::Scope<'s>,
    exec: &'s Exec<'s>,
}

fn lock<T>(g: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    g.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn drain(g: &mut TaskGraph) -> Vec<(TaskId, TaskKind)> {
    let mut out = Vec::new();
    while let Some(id) = g.pop(SerialOrder::Fifo) {
        out.push((id, g.kind(id)));
    }
    out
}

fn spawn<'s>(
    scope: &rayon::Scope<'s>,
    graph: &'s Mutex<TaskGraph>,
    exec: &'s Exec<'s>,
    ready: Vec<(TaskId, TaskKind)>,
) {
    for (id, kind) in ready {
        scope.spawn(move |s| {
            exec(
                id,
                kind,
                &PoolSpawn {
                    graph,
                    scope: s,
                    exec,
                },
            );
            let next = {
                let mut g = lock(graph);
                g.complete(id);
                drain(&mut g)
            };
            spawn(s, graph, exec, next);
        });
    }
}

impl PoolSpawn<'_, '_> {
    fn after<T>(&self, f: impl FnOnce(&mut TaskGraph) -> T) -> T {
        let (v, ready) = {
            let mut g = lock(self.graph);
            let v = f(&mut g);
            (v, drain(&mut g))
        };
        spawn(self.scope, self.graph, self.exec, ready);
        v
    }
}

impl Spawn for PoolSpawn<'_, '_> {
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

/// A worker's stack, which `hd` gives its command thread too. A syntax
/// walk recurses once per nesting level, and a chain link is one or two
/// levels. At the parser's limit of 10,000 links (`grammar.limit.chain`),
/// checking `f()()...` needs 12 to 16 MiB in a dev build, and 32 MiB is
/// twice that; a main thread's 8 MiB overflowed. Untouched stack pages
/// cost no RSS, so the size costs address space only.
pub const WORKER_STACK: usize = 32 * 1024 * 1024;

/// Runs the graph on `threads` rayon workers; returns it with the number
/// of tasks that never became ready (0 unless the graph has a bug). If the
/// pool cannot be built, the graph runs serially. A nested call on a Rayon
/// worker also runs serially on that caller, including its batched maps.
pub fn run_pool(mut graph: TaskGraph, threads: usize, exec: &Exec<'_>) -> (TaskGraph, usize) {
    // Rayon cross-pool waits help the caller's original pool. A helped
    // task can block on a resource the suspended caller still owns. Keep
    // nested graphs and their par_map work on the caller instead.
    if rayon::current_thread_index().is_some() {
        return run_on_caller(graph, exec, SerialOrder::Fifo);
    }
    let Ok(pool) = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .stack_size(WORKER_STACK)
        .build()
    else {
        return run_on_caller(graph, exec, SerialOrder::Priority);
    };
    let first = drain(&mut graph);
    let shared = Mutex::new(graph);
    pool.scope(|s| spawn(s, &shared, exec, first));
    let g = shared
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let left = g.unfinished();
    (g, left)
}

#[cfg(test)]
mod tests {
    use super::run_pool;
    use crate::{ExtTask, Spawn, TaskGraph, TaskKind};
    use std::sync::Mutex;

    #[test]
    fn nested_graphs_and_their_maps_stay_on_the_caller() -> Result<(), rayon::ThreadPoolBuildError>
    {
        for workers in [1, 2] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(workers)
                .build()?;
            pool.install(|| {
                let caller = std::thread::current().id();
                let mut graph = TaskGraph::default();
                let first = graph.add(TaskKind::FolderGraph, &[]);
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
        Ok(())
    }

    #[test]
    fn a_nested_graph_does_not_help_a_job_waiting_for_its_caller()
    -> Result<(), rayon::ThreadPoolBuildError> {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{TryLockError, mpsc};
        use std::time::Duration;

        let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
        let held = Mutex::new(());
        let helped = AtomicBool::new(false);
        let (started, received) = mpsc::sync_channel(1);
        let received = Mutex::new(received);
        pool.scope(|scope| {
            let caller = std::thread::current().id();
            let guard = super::lock(&held);
            scope.spawn(|_| {
                // A real blocking lock here would deadlock the old cross-pool
                // wait. Record that dependency instead, so the regression
                // fails without leaving a permanently blocked test process.
                helped.store(
                    matches!(held.try_lock(), Err(TryLockError::WouldBlock)),
                    Ordering::SeqCst,
                );
                assert!(started.send(()).is_ok());
            });
            let mut graph = TaskGraph::default();
            graph.add(TaskKind::FolderGraph, &[]);
            let (_, left) = run_pool(graph, 2, &|_, _, _| {
                if std::thread::current().id() != caller {
                    // Make the old pool's job wait until its caller has
                    // helped the queued outer job. No timing assertion.
                    assert!(
                        super::lock(&received)
                            .recv_timeout(Duration::from_secs(2))
                            .is_ok()
                    );
                }
            });
            assert_eq!(left, 0);
            drop(guard);
        });
        assert!(!helped.load(Ordering::SeqCst));
        Ok(())
    }

    #[test]
    fn nested_graph_unwinding_restores_parallel_maps() -> Result<(), rayon::ThreadPoolBuildError> {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build()?;
        pool.install(|| {
            struct TaskFailure;
            let failure = std::panic::catch_unwind(|| {
                let mut graph = TaskGraph::default();
                graph.add(TaskKind::FolderGraph, &[]);
                run_pool(graph, 2, &|_, _, _| std::panic::panic_any(TaskFailure));
            });
            assert!(failure.is_err());
            assert!(!super::on_caller());
            assert_eq!(crate::task_threads(), 2);
        });
        Ok(())
    }

    #[test]
    fn nested_graph_work_is_linear_in_width_and_depth() -> Result<(), rayon::ThreadPoolBuildError> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build()?;
        pool.install(|| {
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
        Ok(())
    }

    #[test]
    fn pool_runs_every_task_once_after_its_dependencies() {
        let mut g = TaskGraph::default();
        let root = g.add(TaskKind::FolderGraph, &[]);
        for f in 0..8 {
            g.add(TaskKind::FolderIface(f), &[root]);
        }
        let log = Mutex::new(Vec::new());
        let (g, left) = run_pool(g, 4, &|_, kind, sp: &dyn Spawn| {
            log.lock().expect("log").push(kind);
            if let TaskKind::FolderIface(f) = kind {
                let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
                let e = sp.add(TaskKind::Body(f), &[]);
                sp.edge(e, link);
                sp.release(link);
            }
        });
        let log = log.into_inner().expect("log");
        assert_eq!(left, 0);
        assert_eq!(log[0], TaskKind::FolderGraph);
        assert_eq!(log.len(), 25);
        assert_eq!(g.len(), 25);
    }
}
