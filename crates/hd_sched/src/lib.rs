#![forbid(unsafe_code)]
//! `hd_sched`: the task graph and its executors (`crates/hd_sched/DESIGN.md` §6.1 to
//! §6.3; `docs/compiler/data-structures.md` §3.21): serial (FIFO, priority or shuffled)
//! and the rayon pool (feature `threads`). Every executor runs the same task signature, `Exec`: a task
//! gets its id, its kind and a `&dyn Spawn` (SK-13).

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

#[cfg(feature = "threads")]
pub mod pool;

/// The threads a batched parallel iterator inside a task may use (§6.2):
/// the pool's when this thread is one of its workers, else 1. Nested graphs
/// run on their caller and keep their batched maps serial too.
#[must_use]
pub fn task_threads() -> usize {
    #[cfg(feature = "threads")]
    if !pool::on_caller() && rayon::current_thread_index().is_some() {
        return rayon::current_num_threads();
    }
    1
}

/// Maps `items` inside one task (§6.2 granularity rule): on the task's
/// pool when [`task_threads`] is above 1, else in order on this thread.
/// Either way the output is in item order.
pub fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    #[cfg(feature = "threads")]
    if task_threads() > 1 {
        use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
        return items.par_iter().map(f).collect();
    }
    items.iter().map(f).collect()
}

/// Runs `f` on each of `items` with its index, as [`par_map`] maps: on
/// the task's pool when [`task_threads`] is above 1, else in order. Each
/// call owns its item, so the items end the same at any thread count.
pub fn par_each_mut<T: Send>(items: &mut [T], f: impl Fn(usize, &mut T) + Sync + Send) {
    #[cfg(feature = "threads")]
    if task_threads() > 1 {
        use rayon::iter::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};
        // A job of at least 16 items: one split per small function costs
        // more than its cleanup.
        items
            .par_iter_mut()
            .with_min_len(16)
            .enumerate()
            .for_each(|(k, item)| f(k, item));
        return;
    }
    items
        .iter_mut()
        .enumerate()
        .for_each(|(k, item)| f(k, item));
}

/// Every task of the run (§6.1). Payloads are dense run IDs: a file,
/// unit or module index, or a program or instance slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TaskKind {
    Skim(u32),
    Parse(u32),
    UnitGraph,
    UnitIface(u32),
    HeaderCheck(u32),
    ModulePrep(u32),
    /// All of a module's bodies, as one task (the granularity rule).
    Body(u32),
    ModuleFinish(u32),
    /// One task over the traits whose key changed.
    Coherence,
    InitOrder(u32),
    PackageResult,
    Ext(ExtTask),
}

/// D2's tasks (`crates/hd_mono/DESIGN.md` §11.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtTask {
    Collect,
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
            TaskKind::UnitGraph => "UnitGraph",
            TaskKind::UnitIface(_) => "UnitIface",
            TaskKind::HeaderCheck(_) => "HeaderCheck",
            TaskKind::ModulePrep(_) => "ModulePrep",
            TaskKind::Body(_) => "Body",
            TaskKind::ModuleFinish(_) => "ModuleFinish",
            TaskKind::Coherence => "Coherence",
            TaskKind::InitOrder(_) => "InitOrder",
            TaskKind::PackageResult => "PackageResult",
            TaskKind::Ext(ExtTask::Collect) => "Collect",
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub usize);

struct TaskNode {
    kind: TaskKind,
    waiting_on: u32,
    successors: Vec<TaskId>,
    state: State,
    priority: u32,
    /// The readiness number of the task's live ready entry, while it is
    /// `Ready` ([`Entry`]).
    ready_seq: u64,
}

/// One readiness of a task in the ready queue: the task and its readiness
/// number, which orders readiness. It is live while the task is `Ready`
/// from that readiness. `edge` returning a ready task to waiting leaves its
/// entry behind, dead, and a pop drops a dead entry when it reaches it
/// (lazy deletion), so neither scans the queue.
#[derive(Clone, Copy)]
struct Entry {
    seq: u64,
    id: TaskId,
}

/// The ready queue, in the shape of the last pop's order. Every
/// readiness pushes one entry, and every entry is popped at most once, so
/// a run's queue work is its readinesses times the shape's step.
enum ReadyQueue {
    /// `Fifo`: readiness order, O(1) per push and pop.
    Fifo(VecDeque<Entry>),
    /// `Priority`: a binary heap on (priority, earliest creation), the
    /// readiness number breaking no ties since a task has one live entry;
    /// O(log n) per push and pop.
    Priority(BinaryHeap<(u32, Reverse<usize>, u64)>),
    /// `Shuffled` with its seed: a bag picked from by a seeded index and
    /// emptied by `swap_remove`, O(1) per push and pop.
    Shuffled(u64, Vec<Entry>),
}

impl Default for ReadyQueue {
    fn default() -> Self {
        Self::Fifo(VecDeque::new())
    }
}

/// Dynamic task graph: a running task may add tasks and edges. An edge from
/// a finished task counts as satisfied at once.
#[derive(Default)]
pub struct TaskGraph {
    nodes: Vec<TaskNode>,
    ready: ReadyQueue,
    /// The readinesses so far: the next entry's number.
    seq: u64,
    /// The tasks done, so `unfinished` is one subtraction.
    finished: usize,
    /// Ready entries pushed, popped, dropped dead or moved to another
    /// queue shape, and edges added: the queue's counted work.
    work: u64,
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

impl TaskGraph {
    pub fn add(&mut self, kind: TaskKind, deps: &[TaskId]) -> TaskId {
        self.add_with_priority(kind, deps, 0)
    }

    pub fn add_with_priority(&mut self, kind: TaskKind, deps: &[TaskId], priority: u32) -> TaskId {
        let id = self.add_held(kind, deps, priority);
        self.release(id);
        id
    }

    /// The creation guard (`docs/compiler/data-structures.md` §3.21): the task waits on
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
            ready_seq: 0,
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
            self.make_ready(id);
        }
    }

    /// Marks a waiting task whose last dependency is gone ready, with a
    /// new live entry in the ready queue.
    fn make_ready(&mut self, id: TaskId) {
        self.seq += 1;
        self.work += 1;
        let n = &mut self.nodes[id.0];
        n.state = State::Ready;
        n.ready_seq = self.seq;
        let e = Entry { seq: self.seq, id };
        match &mut self.ready {
            ReadyQueue::Fifo(q) => q.push_back(e),
            ReadyQueue::Priority(h) => h.push((n.priority, Reverse(id.0), e.seq)),
            ReadyQueue::Shuffled(_, b) => b.push(e),
        }
    }

    /// Whether `e` is its task's live entry.
    fn live(&self, e: Entry) -> bool {
        let n = &self.nodes[e.id.0];
        n.state == State::Ready && n.ready_seq == e.seq
    }

    /// The ready queue in `order`'s shape. A change of order moves the
    /// live entries over in readiness order: O(n log n) once per change,
    /// and a scheduler keeps one order.
    fn queue_for(&mut self, order: SerialOrder) -> &mut ReadyQueue {
        let same = matches!(
            (&self.ready, order),
            (ReadyQueue::Fifo(_), SerialOrder::Fifo)
                | (ReadyQueue::Priority(_), SerialOrder::Priority)
        ) || matches!(
            (&self.ready, order),
            (ReadyQueue::Shuffled(had, _), SerialOrder::Shuffled(seed)) if *had == seed
        );
        if !same {
            let mut entries: Vec<Entry> = match std::mem::take(&mut self.ready) {
                ReadyQueue::Fifo(q) => q.into_iter().collect(),
                ReadyQueue::Priority(h) => h
                    .into_iter()
                    .map(|(_, Reverse(id), seq)| Entry {
                        seq,
                        id: TaskId(id),
                    })
                    .collect(),
                ReadyQueue::Shuffled(_, b) => b,
            };
            self.work += entries.len() as u64;
            entries.retain(|&e| self.live(e));
            // Readiness order: a heap sort on the readiness number, which
            // is unique per entry.
            let entries: Vec<Entry> = entries
                .iter()
                .map(|e| (e.seq, e.id.0))
                .collect::<BinaryHeap<_>>()
                .into_sorted_vec()
                .into_iter()
                .map(|(seq, id)| Entry {
                    seq,
                    id: TaskId(id),
                })
                .collect();
            self.ready = match order {
                SerialOrder::Fifo => ReadyQueue::Fifo(entries.into()),
                SerialOrder::Priority => ReadyQueue::Priority(
                    entries
                        .iter()
                        .map(|e| (self.nodes[e.id.0].priority, Reverse(e.id.0), e.seq))
                        .collect(),
                ),
                SerialOrder::Shuffled(seed) => ReadyQueue::Shuffled(seed, entries),
            };
        }
        &mut self.ready
    }

    /// `to` waits for `from`. `to` must not have started.
    pub fn edge(&mut self, from: TaskId, to: TaskId) {
        assert!(
            matches!(self.nodes[to.0].state, State::Waiting | State::Ready),
            "edge into a started task"
        );
        if self.nodes[from.0].state == State::Done {
            return;
        }
        self.work += 1;
        if self.nodes[to.0].state == State::Ready {
            // Its ready entry is dead from here (lazy deletion).
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

    /// Takes the next ready task in `order` and marks it running: the
    /// first ready (`Fifo`), the highest priority then the earliest
    /// created (`Priority`), or a seeded pick (`Shuffled`). Dead entries
    /// met on the way are dropped, each once.
    pub fn pop(&mut self, order: SerialOrder) -> Option<TaskId> {
        let nodes = self.nodes.len() as u64;
        loop {
            let e = match self.queue_for(order) {
                ReadyQueue::Fifo(q) => q.pop_front()?,
                ReadyQueue::Priority(h) => {
                    let (_, Reverse(id), seq) = h.pop()?;
                    Entry {
                        seq,
                        id: TaskId(id),
                    }
                }
                ReadyQueue::Shuffled(seed, b) => {
                    if b.is_empty() {
                        return None;
                    }
                    let x = *seed ^ nodes.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (b.len() as u64);
                    let at = usize::try_from(x.wrapping_mul(0xbf58_476d_1ce4_e5b9) >> 33)
                        .expect("index")
                        % b.len();
                    b.swap_remove(at)
                }
            };
            self.work += 1;
            if self.live(e) {
                self.nodes[e.id.0].state = State::Running;
                return Some(e.id);
            }
        }
    }

    /// Marks a task done and readies successors whose last dependency it was.
    pub fn complete(&mut self, id: TaskId) {
        if self.nodes[id.0].state != State::Done {
            self.finished += 1;
        }
        self.nodes[id.0].state = State::Done;
        let succ = std::mem::take(&mut self.nodes[id.0].successors);
        for s in succ {
            let n = &mut self.nodes[s.0];
            n.waiting_on -= 1;
            if n.waiting_on == 0 && n.state == State::Waiting {
                self.make_ready(s);
            }
        }
    }

    /// The tasks not done: one subtraction.
    #[must_use]
    pub fn unfinished(&self) -> usize {
        self.nodes.len() - self.finished
    }

    /// The queue work so far: entries pushed, popped, dropped dead or
    /// moved between queue shapes, and edges added.
    #[must_use]
    pub fn work(&self) -> u64 {
        self.work
    }
}

/// What a running task may do to the graph (`crates/hd_sched/DESIGN.md` §6.2, SK-13):
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

#[cfg(test)]
mod tests {
    use super::{ExtTask, SerialOrder, SerialScheduler, Spawn, State, TaskGraph, TaskId, TaskKind};
    use std::sync::Mutex;

    #[test]
    fn tasks_added_while_running_wait_for_their_edges() {
        let mut g = TaskGraph::default();
        let first = g.add(TaskKind::UnitGraph, &[]);
        g.add(TaskKind::PackageResult, &[first]);
        let order = Mutex::new(Vec::new());
        let left = SerialScheduler {
            order: SerialOrder::Fifo,
        }
        .run(&mut g, &|_, kind, sp: &dyn Spawn| {
            order.lock().expect("log").push(kind.name());
            if kind == TaskKind::PackageResult {
                let link = sp.add_held(TaskKind::Ext(ExtTask::Link), &[]);
                let body = sp.add(TaskKind::Body(0), &[]);
                sp.edge(body, link);
                sp.release(link);
            }
        });
        assert_eq!(left, 0);
        assert_eq!(
            order.into_inner().expect("log"),
            ["UnitGraph", "PackageResult", "Body", "Link"]
        );
    }

    fn diamond() -> TaskGraph {
        let mut g = TaskGraph::default();
        let a = g.add(TaskKind::Skim(0), &[]);
        let b = g.add_with_priority(TaskKind::UnitIface(0), &[a], 1);
        let c = g.add_with_priority(TaskKind::UnitIface(1), &[a], 5);
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
                assert_eq!(seen[1], TaskKind::UnitIface(1), "higher priority first");
            }
        }
    }

    /// `n` tasks ready at once, half of them sent back to waiting by an
    /// edge from a task of their own and made ready again when it runs:
    /// #2032's shape, `n` modules ready together.
    fn many_ready(n: usize) -> TaskGraph {
        let mut g = TaskGraph::default();
        let ready: Vec<TaskId> = (0..n)
            .map(|i| {
                let p = u32::try_from(i % 3).expect("priority");
                g.add_with_priority(TaskKind::Body(u32::try_from(i).expect("body")), &[], p)
            })
            .collect();
        for &to in ready.iter().step_by(2) {
            let before = g.add(TaskKind::Coherence, &[]);
            g.edge(before, to);
        }
        g
    }

    fn drain(g: &mut TaskGraph, order: SerialOrder) -> Vec<TaskId> {
        let mut out = Vec::new();
        while let Some(id) = g.pop(order) {
            out.push(id);
            g.complete(id);
        }
        out
    }

    // #2032: popping and un-readying cost O(1), or O(log n) in the
    // heap, per entry: `pop(Priority)` scanned every ready task and
    // `edge` filtered the whole queue.
    #[test]
    fn ready_queue_work_is_linear_in_the_ready_tasks() {
        for order in [
            SerialOrder::Fifo,
            SerialOrder::Priority,
            SerialOrder::Shuffled(0x5eed),
        ] {
            let work = |n: usize| {
                let mut g = many_ready(n);
                let ran = drain(&mut g, order);
                assert_eq!(ran.len(), g.len(), "{order:?}: every task runs once");
                assert_eq!(g.unfinished(), 0);
                g.work()
            };
            let (small, large) = (work(1_000), work(10_000));
            assert!(
                small > 0 && large <= 10 * small + 100,
                "{order:?}: queue work for 1,000 and 10,000 ready tasks: {small} and {large}"
            );
        }
    }

    #[test]
    fn priority_runs_the_highest_then_the_earliest_created() {
        // At each pop, of the tasks ready then, the highest priority,
        // then the earliest created.
        let mut g = many_ready(12);
        let priority: Vec<u32> = g.nodes.iter().map(|n| n.priority).collect();
        let key = |id: TaskId| (std::cmp::Reverse(priority[id.0]), id.0);
        let mut ready: Vec<TaskId> = (0..g.len())
            .map(TaskId)
            .filter(|&id| g.state(id) == State::Ready)
            .collect();
        while let Some(id) = g.pop(SerialOrder::Priority) {
            let best = ready
                .iter()
                .copied()
                .min_by_key(|&r| key(r))
                .expect("ready");
            assert_eq!(id, best, "the highest priority, then the earliest created");
            g.complete(id);
            ready = (0..g.len())
                .map(TaskId)
                .filter(|&id| g.state(id) == State::Ready)
                .collect();
        }
    }

    #[test]
    fn fifo_puts_a_readied_again_task_behind_the_others() {
        let mut g = TaskGraph::default();
        let a = g.add(TaskKind::Skim(0), &[]);
        let b = g.add(TaskKind::Skim(1), &[]);
        let c = g.add(TaskKind::Skim(2), &[]);
        let gate = g.add_held(TaskKind::Coherence, &[], 0);
        g.edge(gate, a);
        g.release(gate);
        assert_eq!(g.state(a), State::Waiting);
        assert_eq!(drain(&mut g, SerialOrder::Fifo), [b, c, gate, a]);
    }

    #[test]
    fn shuffled_is_seeded_and_runs_each_task_once() {
        let run = |seed| drain(&mut many_ready(50), SerialOrder::Shuffled(seed));
        let (one, again, other) = (run(1), run(1), run(2));
        assert_eq!(one, again, "one seed, one order");
        assert_ne!(one, other, "another seed, another order");
        let mut ran = vec![0_u32; one.iter().map(|id| id.0 + 1).max().unwrap_or(0)];
        for id in &one {
            ran[id.0] += 1;
        }
        assert!(ran.iter().all(|&n| n == 1), "each task once: {ran:?}");
    }

    // A change of order moves the live entries over in readiness order,
    // dropping the dead ones, and the next pop follows the new order.
    #[test]
    fn a_change_of_order_keeps_the_ready_tasks() {
        let mut g = TaskGraph::default();
        let low = g.add_with_priority(TaskKind::Skim(0), &[], 0);
        let high = g.add_with_priority(TaskKind::Skim(1), &[], 7);
        let gate = g.add(TaskKind::Coherence, &[]);
        let late = g.add_with_priority(TaskKind::Skim(2), &[], 9);
        g.edge(gate, late);
        assert_eq!(g.pop(SerialOrder::Fifo), Some(low));
        g.complete(low);
        assert_eq!(g.pop(SerialOrder::Priority), Some(high));
        g.complete(high);
        assert_eq!(g.pop(SerialOrder::Shuffled(3)), Some(gate));
        g.complete(gate);
        assert_eq!(g.pop(SerialOrder::Shuffled(4)), Some(late));
        g.complete(late);
        assert_eq!(g.pop(SerialOrder::Fifo), None);
        assert_eq!(g.unfinished(), 0);
    }
}
