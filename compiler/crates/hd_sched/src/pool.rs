//! The pool executor (§6.2), feature `threads`: rayon's work-stealing
//! pool. A task that becomes ready is spawned into the pool's scope; the
//! graph itself sits behind one mutex, held only to add, edge, release or
//! complete. Tasks that receive edges after creation are created behind
//! their guard (`add_held`), so no edge can reach a started task.

use std::sync::Mutex;

use crate::{Exec, SerialOrder, Spawn, TaskGraph, TaskId, TaskKind};

struct PoolSpawn<'a, 's> {
    graph: &'s Mutex<TaskGraph>,
    scope: &'a rayon::Scope<'s>,
    exec: &'s Exec<'s>,
}

fn lock(g: &Mutex<TaskGraph>) -> std::sync::MutexGuard<'_, TaskGraph> {
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

/// Runs the graph on `threads` rayon workers; returns it with the number
/// of tasks that never became ready (0 unless the graph has a bug). If the
/// pool cannot be built, the graph runs serially.
pub fn run_pool(mut graph: TaskGraph, threads: usize, exec: &Exec<'_>) -> (TaskGraph, usize) {
    let Ok(pool) = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
    else {
        let left = crate::SerialScheduler {
            order: SerialOrder::Priority,
        }
        .run(&mut graph, exec);
        return (graph, left);
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
                let e = sp.add(TaskKind::Ext(ExtTask::Emit(f)), &[]);
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
