#![forbid(unsafe_code)]
//! `hd_sched`: the task graph and its executors (scheduler.md §6.1 to
//! §6.4; data-structures.md §3.21): serial (FIFO, priority or shuffled),
//! stepping (the browser's `run_for`), and the rayon pool (feature
//! `threads`). Every executor runs the same task signature, `Exec`: a task
//! gets its id, its kind and a `&dyn Spawn` (SK-13).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "threads")]
pub mod pool;

/// Every task of the run (§6.1). Payloads are dense run IDs: a file,
/// folder or module index, or a program or instance slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Skim(u32),
    Parse(u32),
    FolderGraph,
    FolderIface(u32),
    HeaderCheck(u32),
    ModulePrep(u32),
    /// All of a module's bodies, as one task (the granularity rule).
    Body(u32),
    ModuleFinish(u32),
    TestOverlay(u32),
    /// One task over the traits whose key changed.
    Coherence,
    InitOrder(u32),
    PackageResult,
    Ext(ExtTask),
}

/// D2's tasks (codegen.md §11.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtTask {
    Collect,
    Emit(u32),
    Link,
    Precompile,
    RunCase(u32),
}

impl TaskKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            TaskKind::Skim(_) => "Skim",
            TaskKind::Parse(_) => "Parse",
            TaskKind::FolderGraph => "FolderGraph",
            TaskKind::FolderIface(_) => "FolderIface",
            TaskKind::HeaderCheck(_) => "HeaderCheck",
            TaskKind::ModulePrep(_) => "ModulePrep",
            TaskKind::Body(_) => "Body",
            TaskKind::ModuleFinish(_) => "ModuleFinish",
            TaskKind::TestOverlay(_) => "TestOverlay",
            TaskKind::Coherence => "Coherence",
            TaskKind::InitOrder(_) => "InitOrder",
            TaskKind::PackageResult => "PackageResult",
            TaskKind::Ext(ExtTask::Collect) => "Collect",
            TaskKind::Ext(ExtTask::Emit(_)) => "Emit",
            TaskKind::Ext(ExtTask::Link) => "Link",
            TaskKind::Ext(ExtTask::Precompile) => "Precompile",
            TaskKind::Ext(ExtTask::RunCase(_)) => "RunCase",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Waiting,
    Ready,
    Running,
    Done,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub usize);

struct TaskNode {
    kind: TaskKind,
    waiting_on: u32,
    successors: Vec<TaskId>,
    state: State,
    priority: u32,
}

/// Dynamic task graph: a running task may add tasks and edges. An edge from
/// a finished task counts as satisfied at once.
#[derive(Default)]
pub struct TaskGraph {
    nodes: Vec<TaskNode>,
    ready: VecDeque<TaskId>,
}

/// How the serial executor picks among ready tasks (§6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialOrder {
    /// In the order tasks became ready.
    Fifo,
    /// Highest priority first, then creation order (§6.3).
    Priority,
    /// A seeded random choice: finds order dependence without threads (§8.1).
    Shuffled(u64),
}

/// The cancellation flag (§6.4): checked when a task starts.
#[derive(Clone, Default, Debug)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl TaskGraph {
    pub fn add(&mut self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.add_with_priority(kind, deps, 0)
    }

    pub fn add_with_priority(&mut self, kind: TaskKind, deps: &[TaskId], priority: u32) -> TaskId {
        let id = self.add_held(kind, deps, priority);
        self.release(id);
        id
    }

    /// The creation guard (data-structures.md §3.21): the task waits on
    /// one extra count until `release`, so edges added after creation can
    /// never race its start.
    pub fn add_held(&mut self, kind: TaskKind, deps: &[TaskId], priority: u32) -> TaskId {
        let id = TaskId(self.nodes.len());
        self.nodes.push(TaskNode {
            kind,
            waiting_on: 1,
            successors: Vec::new(),
            state: State::Waiting,
            priority,
        });
        for &d in deps {
            self.edge(d, id);
        }
        id
    }

    /// Drops a task's creation guard.
    pub fn release(&mut self, id: TaskId) {
        let n = &mut self.nodes[id.0];
        n.waiting_on -= 1;
        if n.waiting_on == 0 && n.state == State::Waiting {
            n.state = State::Ready;
            self.ready.push_back(id);
        }
    }

    /// `to` waits for `from`. `to` must not have started.
    pub fn edge(&mut self, from: TaskId, to: TaskId) {
        assert!(
            matches!(self.nodes[to.0].state, State::Waiting | State::Ready),
            "edge into a started task"
        );
        if matches!(self.nodes[from.0].state, State::Done | State::Cancelled) {
            return;
        }
        if self.nodes[to.0].state == State::Ready {
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
    #[must_use]
    pub fn kind(&self, id: TaskId) -> TaskKind {
        self.nodes[id.0].kind
    }
    #[must_use]
    pub fn state(&self, id: TaskId) -> State {
        self.nodes[id.0].state
    }

    /// Takes the next ready task in `order` and marks it running.
    pub fn pop(&mut self, order: SerialOrder) -> Option<TaskId> {
        let at = match order {
            SerialOrder::Fifo => 0,
            SerialOrder::Priority => {
                let best = self
                    .ready
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, t)| (self.nodes[t.0].priority, usize::MAX - t.0))?;
                best.0
            }
            SerialOrder::Shuffled(seed) => {
                if self.ready.is_empty() {
                    return None;
                }
                let x = seed
                    ^ (self.nodes.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                    ^ (self.ready.len() as u64);
                usize::try_from(x.wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 33).expect("index")
                    % self.ready.len()
            }
        };
        let id = self.ready.remove(at)?;
        self.nodes[id.0].state = State::Running;
        Some(id)
    }

    /// Marks a task done and readies successors whose last dependency it was.
    pub fn complete(&mut self, id: TaskId) {
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

    /// Marks every unfinished task cancelled (§6.4).
    pub fn cancel_all(&mut self) {
        for n in &mut self.nodes {
            if n.state != State::Done {
                n.state = State::Cancelled;
            }
        }
        self.ready.clear();
    }

    #[must_use]
    pub fn unfinished(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| !matches!(n.state, State::Done | State::Cancelled))
            .count()
    }
}

/// What a running task may do to the graph (scheduler.md §6.2, SK-13):
/// every executor implements it, so task code is executor-independent.
pub trait Spawn {
    fn add(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId;
    fn edge(&self, from: TaskId, to: TaskId);
    /// Adds a task behind its creation guard; it starts only after
    /// `release`, so the creator can add edges into it first.
    fn add_held(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId;
    fn release(&self, id: TaskId);
}

/// The one task signature of every executor.
pub type Exec<'a> = dyn Fn(TaskId, TaskKind, &dyn Spawn) + Sync + 'a;

/// `Spawn` over a serial executor's `&mut` graph.
struct SerialSpawn<'g>(RefCell<&'g mut TaskGraph>);

impl Spawn for SerialSpawn<'_> {
    fn add(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.0.borrow_mut().add(kind, deps)
    }
    fn edge(&self, from: TaskId, to: TaskId) {
        self.0.borrow_mut().edge(from, to);
    }
    fn add_held(&self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.0.borrow_mut().add_held(kind, deps, 0)
    }
    fn release(&self, id: TaskId) {
        self.0.borrow_mut().release(id);
    }
}

/// The serial executor: `--threads 1` and the browser's base (§6.2).
#[derive(Clone, Copy, Debug)]
pub struct SerialScheduler {
    pub order: SerialOrder,
}

impl SerialScheduler {
    /// Runs every task; returns how many never became ready (a cycle or a
    /// missing edge, a compiler bug the caller reports).
    pub fn run(&self, g: &mut TaskGraph, exec: &Exec<'_>) -> usize {
        while let Some(id) = g.pop(self.order) {
            let kind = g.kind(id);
            exec(id, kind, &SerialSpawn(RefCell::new(g)));
            g.complete(id);
        }
        g.unfinished()
    }
}

/// What a stepping slice ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Done,
    Yielded { ran: usize },
    Cancelled,
}

/// The stepping executor (§6.2): the serial executor in slices, so the
/// browser worker can return to JavaScript between them.
#[derive(Clone, Debug)]
pub struct SteppingScheduler {
    pub order: SerialOrder,
    pub cancel: CancelFlag,
}

impl SteppingScheduler {
    /// Runs at most `max_steps` tasks, then returns.
    pub fn run_for(&mut self, g: &mut TaskGraph, max_steps: usize, exec: &Exec<'_>) -> Progress {
        let mut ran = 0;
        while ran < max_steps {
            if self.cancel.is_cancelled() {
                g.cancel_all();
                return Progress::Cancelled;
            }
            let Some(id) = g.pop(self.order) else {
                return Progress::Done;
            };
            let kind = g.kind(id);
            exec(id, kind, &SerialSpawn(RefCell::new(g)));
            g.complete(id);
            ran += 1;
        }
        if g.unfinished() == 0 {
            Progress::Done
        } else {
            Progress::Yielded { ran }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancelFlag, ExtTask, Progress, SerialOrder, SerialScheduler, Spawn, SteppingScheduler,
        TaskGraph, TaskId, TaskKind,
    };
    use std::sync::Mutex;

    #[test]
    fn tasks_added_while_running_wait_for_their_edges() {
        let mut g = TaskGraph::default();
        let first = g.add(TaskKind::FolderGraph, &[]);
        g.add(TaskKind::PackageResult, &[first]);
        let order = Mutex::new(Vec::new());
        let left = SerialScheduler {
            order: SerialOrder::Fifo,
        }
        .run(&mut g, &|_, kind, sp: &dyn Spawn| {
            order.lock().expect("log").push(kind.name());
            if kind == TaskKind::PackageResult {
                let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
                let emit = sp.add(TaskKind::Ext(ExtTask::Emit(0)), &[]);
                sp.edge(emit, link);
                sp.release(link);
            }
        });
        assert_eq!(left, 0);
        assert_eq!(
            order.into_inner().expect("log"),
            ["FolderGraph", "PackageResult", "Emit", "Link"]
        );
    }

    fn diamond() -> TaskGraph {
        let mut g = TaskGraph::default();
        let a = g.add(TaskKind::Skim(0), &[]);
        let b = g.add_with_priority(TaskKind::FolderIface(0), &[a], 1);
        let c = g.add_with_priority(TaskKind::FolderIface(1), &[a], 5);
        g.add(TaskKind::PackageResult, &[b, c]);
        g
    }

    #[test]
    fn every_order_respects_edges() {
        for order in [
            SerialOrder::Fifo,
            SerialOrder::Priority,
            SerialOrder::Shuffled(7),
            SerialOrder::Shuffled(8),
        ] {
            let mut g = diamond();
            let seen = Mutex::new(Vec::new());
            SerialScheduler { order }.run(&mut g, &|_, k, _: &dyn Spawn| {
                seen.lock().expect("log").push(k);
            });
            let seen = seen.into_inner().expect("log");
            assert_eq!(seen[0], TaskKind::Skim(0));
            assert_eq!(seen[3], TaskKind::PackageResult);
            if order == SerialOrder::Priority {
                assert_eq!(seen[1], TaskKind::FolderIface(1), "higher priority first");
            }
        }
    }

    #[test]
    fn stepping_yields_and_cancels() {
        let mut g = diamond();
        let cancel = CancelFlag::default();
        let mut s = SteppingScheduler {
            order: SerialOrder::Fifo,
            cancel: cancel.clone(),
        };
        let noop = |_: TaskId, _: TaskKind, _: &dyn Spawn| {};
        assert_eq!(s.run_for(&mut g, 2, &noop), Progress::Yielded { ran: 2 });
        cancel.cancel();
        assert_eq!(s.run_for(&mut g, 2, &noop), Progress::Cancelled);
        assert_eq!(g.unfinished(), 0);
        let mut g = diamond();
        let mut s = SteppingScheduler {
            order: SerialOrder::Fifo,
            cancel: CancelFlag::default(),
        };
        assert_eq!(s.run_for(&mut g, 100, &noop), Progress::Done);
    }
}
