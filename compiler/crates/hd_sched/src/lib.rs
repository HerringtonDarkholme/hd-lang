//! `hd_sched`: the task graph and the serial executor (scheduler.md §6.1,
//! §6.2).

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Skim(u32),
    Parse(u32),
    FolderGraph,
    FolderIface(u32),
    ModulePrep(u32),
    /// All of a module's bodies, as one task (the granularity rule).
    Body(u32),
    ModuleFinish(u32),
    PackageResult,
    Ext(ExtTask),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtTask {
    Collect,
    Emit(u32),
    Link,
}

impl TaskKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TaskKind::Skim(_) => "Skim",
            TaskKind::Parse(_) => "Parse",
            TaskKind::FolderGraph => "FolderGraph",
            TaskKind::FolderIface(_) => "FolderIface",
            TaskKind::ModulePrep(_) => "ModulePrep",
            TaskKind::Body(_) => "Body",
            TaskKind::ModuleFinish(_) => "ModuleFinish",
            TaskKind::PackageResult => "PackageResult",
            TaskKind::Ext(ExtTask::Collect) => "Collect",
            TaskKind::Ext(ExtTask::Emit(_)) => "Emit",
            TaskKind::Ext(ExtTask::Link) => "Link",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Waiting,
    Ready,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub usize);

struct TaskNode {
    kind: TaskKind,
    waiting_on: u32,
    successors: Vec<TaskId>,
    state: State,
}

/// Dynamic task graph: a running task may add tasks and edges. An edge from
/// a finished task counts as satisfied at once.
#[derive(Default)]
pub struct TaskGraph {
    nodes: Vec<TaskNode>,
    ready: VecDeque<TaskId>,
}

impl TaskGraph {
    pub fn add(&mut self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        let id = TaskId(self.nodes.len());
        self.nodes.push(TaskNode { kind, waiting_on: 0, successors: Vec::new(), state: State::Waiting });
        for &d in deps {
            self.edge(d, id);
        }
        if self.nodes[id.0].waiting_on == 0 {
            self.nodes[id.0].state = State::Ready;
            self.ready.push_back(id);
        }
        id
    }
    /// `to` waits for `from`. `to` must not have started.
    pub fn edge(&mut self, from: TaskId, to: TaskId) {
        assert!(self.nodes[to.0].state != State::Done, "edge into a finished task");
        if self.nodes[from.0].state == State::Done {
            return;
        }
        if self.nodes[to.0].state == State::Ready {
            // Pull it back out of the ready queue.
            self.ready.retain(|&r| r != to);
            self.nodes[to.0].state = State::Waiting;
        }
        self.nodes[from.0].successors.push(to);
        self.nodes[to.0].waiting_on += 1;
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The serial executor: FIFO over ready tasks, in creation order.
    pub fn run(&mut self, exec: &mut dyn FnMut(TaskId, TaskKind, &mut TaskGraph)) {
        while let Some(id) = self.ready.pop_front() {
            let kind = self.nodes[id.0].kind;
            exec(id, kind, self);
            self.nodes[id.0].state = State::Done;
            let succ = std::mem::take(&mut self.nodes[id.0].successors);
            for s in succ {
                let n = &mut self.nodes[s.0];
                n.waiting_on -= 1;
                if n.waiting_on == 0 && n.state == State::Waiting {
                    n.state = State::Ready;
                    self.ready.push_back(s);
                }
            }
        }
        let stuck = self.nodes.iter().filter(|n| n.state != State::Done).count();
        assert!(stuck == 0, "{stuck} tasks never became ready (a cycle or a missing edge)");
    }
}

#[cfg(test)]
mod tests {
    use super::{ExtTask, TaskGraph, TaskKind};

    #[test]
    fn tasks_added_while_running_wait_for_their_edges() {
        let mut g = TaskGraph::default();
        let first = g.add(TaskKind::FolderGraph, &[]);
        g.add(TaskKind::PackageResult, &[first]);
        let mut order = Vec::new();
        g.run(&mut |_, kind, g| {
            order.push(kind.name());
            if kind == TaskKind::PackageResult {
                // Link is created first, then the Emit it waits for.
                let link = g.add(TaskKind::Ext(ExtTask::Link), &[]);
                let emit = g.add(TaskKind::Ext(ExtTask::Emit(0)), &[]);
                g.edge(emit, link);
            }
        });
        assert_eq!(order, ["FolderGraph", "PackageResult", "Emit", "Link"]);
    }
}
