//! The pool executor (§6.2), feature `threads`. The design uses rayon's
//! work-stealing pool; the workspace has no rayon yet, so a shared ready
//! queue under a mutex with a condition variable stands in (SK-9). The
//! task graph's contract is the same: a running task may add tasks and
//! edges through the `Spawner`, and each edge is satisfied exactly once.

use std::sync::{Condvar, Mutex};

use crate::{SerialOrder, TaskGraph, TaskId, TaskKind};

/// What a running task may do to the graph.
pub struct Spawner<'a> {
    shared: &'a Shared,
}

struct Shared {
    graph: Mutex<(TaskGraph, usize)>,
    wake: Condvar,
}

impl Spawner<'_> {
    #[must_use]
    pub fn add(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        let mut g = self.shared.graph.lock().expect("graph");
        let id = g.0.add(kind, deps);
        self.shared.wake.notify_all();
        id
    }
    pub fn edge(&self, from: TaskId, to: TaskId) {
        self.shared.graph.lock().expect("graph").0.edge(from, to);
    }
}

/// Runs the graph on `threads` workers; returns it with every task done.
pub fn run_pool(graph: TaskGraph, threads: usize, exec: &(dyn Fn(TaskId, TaskKind, &Spawner<'_>) + Sync)) -> TaskGraph {
    let shared = Shared { graph: Mutex::new((graph, 0)), wake: Condvar::new() };
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                loop {
                    let next = {
                        let mut g = shared.graph.lock().expect("graph");
                        loop {
                            if let Some(id) = g.0.pop(SerialOrder::Priority) {
                                g.1 += 1;
                                break Some((id, g.0.kind(id)));
                            }
                            if g.1 == 0 {
                                break None;
                            }
                            g = shared.wake.wait(g).expect("wait");
                        }
                    };
                    let Some((id, kind)) = next else {
                        shared.wake.notify_all();
                        return;
                    };
                    exec(id, kind, &Spawner { shared: &shared });
                    let mut g = shared.graph.lock().expect("graph");
                    g.0.complete(id);
                    g.1 -= 1;
                    shared.wake.notify_all();
                }
            });
        }
    });
    let (g, _) = shared.graph.into_inner().expect("graph");
    let stuck = g.unfinished();
    assert!(stuck == 0, "{stuck} tasks never became ready (a cycle or a missing edge)");
    g
}

#[cfg(test)]
mod tests {
    use super::run_pool;
    use crate::{TaskGraph, TaskKind};
    use std::sync::Mutex;

    #[test]
    fn pool_runs_every_task_once_after_its_dependencies() {
        let mut g = TaskGraph::default();
        let root = g.add(TaskKind::FolderGraph, &[]);
        for f in 0..8 {
            g.add(TaskKind::FolderIface(f), &[root]);
        }
        let log = Mutex::new(Vec::new());
        let g = run_pool(g, 4, &|_, kind, sp| {
            log.lock().expect("log").push(kind);
            if let TaskKind::FolderIface(f) = kind {
                let _ = sp.add(TaskKind::ModulePrep(f), &[]);
            }
        });
        let log = log.into_inner().expect("log");
        assert_eq!(log[0], TaskKind::FolderGraph);
        assert_eq!(log.len(), 17);
        assert_eq!(g.len(), 17);
    }
}
