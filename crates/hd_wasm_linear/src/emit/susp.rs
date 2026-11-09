//! Suspending bodies (`crates/hd_wasm_linear/DESIGN.md` §23.21, job 13): the
//! state machine of §14.2 over a frame in linear memory. A frame is a
//! counted object: the task prefix ([`F_POLL`] to [`F_CHILD`]), then a
//! save area with a slot for every Wasm local of the body. A suspension
//! point saves what a resume there reads and returns Pending with the
//! frame; a resume reloads the slots and branches to its point through the structural state numbering
//! of [`plan`]: a block list that holds states is entered by one
//! `br_table` on `pc`, and a branch that holds the resumed state is taken
//! whatever its condition was.

use crate::op::Op as I;
use hd_base::SliceSort as _;
use hd_base::{Hash128, HashMap, StageResult};
use hd_mono::{Target, TargetKind};
use hd_tir::ir::count::{At as CountAt, Part as CountPart};
use hd_tir::ir::{Body, NONE, Tag};
use hd_tir::own::Reason;
use wasm_encoder::BlockType;

use super::scope::Exit;
use super::{Ctl, Em};
use crate::module::{Global, HEAP, Ins, Part, Sym, Val, mem};
use crate::rt::Helper;
use crate::unsupported;

use crate::task::{
    ABANDONED, ACTIVE, CANCELLED, DONE, EXITED, F_CANCEL, F_CHILD, F_FLAGS, F_POLL, F_SAVED,
    F_STATE, RUNNING, UNWINDING,
};

/// Whether instruction tag `t` is a suspension point.
pub(super) fn is_point(t: Tag) -> bool {
    matches!(
        t,
        Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
    )
}

/// The blocks instruction `i` runs, in the order emission enters them.
fn operand_blocks(b: &Body, i: u32) -> Vec<u32> {
    let [a, bw] = b.data[i as usize];
    let rec = |x: u32| {
        if x == NONE {
            vec![]
        } else {
            b.record(x).to_vec()
        }
    };
    let mut out = match b.tags[i as usize] {
        Tag::If | Tag::Match => rec(bw),
        Tag::Loop => vec![a],
        Tag::Scope | Tag::Guard => [vec![a], rec(bw)].concat(),
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => {
            rec(bw).into_iter().skip(1).take(2).collect()
        }
        Tag::And | Tag::Or | Tag::With => vec![bw],
        _ => vec![],
    };
    out.retain(|x| *x != NONE && b.tags.get(*x as usize) == Some(&Tag::Block));
    out
}

/// Each suspension point's state, and each block's and instruction's
/// range of states, when it holds one.
pub(super) type Plan = (HashMap<u32, u32>, HashMap<u32, (u32, u32)>);

/// Numbers the suspension points of the body under `root` in structural
/// order, from 1, and records the range of states each block and
/// instruction holds: a subtree's states are contiguous (§14.2).
pub(super) fn plan(b: &Body, root: u32) -> Plan {
    fn join(r: Option<(u32, u32)>, x: Option<(u32, u32)>) -> Option<(u32, u32)> {
        match (r, x) {
            (Some(a), Some(c)) => Some((a.0.min(c.0), a.1.max(c.1))),
            (a, c) => a.or(c),
        }
    }
    fn block(b: &Body, blk: u32, p: &mut Plan, next: &mut u32) -> Option<(u32, u32)> {
        let mut r = None;
        for &i in b.record(b.data[blk as usize][0]) {
            if b.tags[i as usize] != Tag::Block {
                r = join(r, inst(b, i, p, next));
            }
        }
        if let Some(x) = r {
            p.1.insert(blk, x);
        }
        r
    }
    fn inst(b: &Body, i: u32, p: &mut Plan, next: &mut u32) -> Option<(u32, u32)> {
        let mut r = None;
        for ob in operand_blocks(b, i) {
            r = join(r, block(b, ob, p, next));
        }
        if is_point(b.tags[i as usize]) {
            *next += 1;
            p.0.insert(i, *next);
            r = join(r, Some((*next, *next)));
        }
        if let Some(x) = r {
            p.1.insert(i, x);
        }
        r
    }
    let mut p = (HashMap::default(), HashMap::default());
    let mut next = 0;
    block(b, root, &mut p, &mut next);
    p
}

/// The state machine of the body being emitted.
pub(super) struct Susp {
    /// The instance whose parts the frame names.
    pub key: Hash128,
    /// The resume point: 0 on the fresh path.
    pub pc: u32,
    pub states: HashMap<u32, u32>,
    pub ranges: HashMap<u32, (u32, u32)>,
    /// The `ctl` index of the current point's cancellation block.
    pub cancel_to: Option<usize>,
    /// Set when the body leaves by its cancellation (`Exit::Cancel`), for
    /// the counting exit.
    pub cancelled: u32,
    /// The data locals of the body's providers: a frame holds a count of
    /// each, since the scope that installed them may end first.
    pub provs: Vec<u32>,
    /// Each `join!` point's done mask and, per child, its instruction, the
    /// locals that keep its result, and the result's type.
    pub alls: HashMap<u32, AllPoint>,
}

impl Susp {
    /// Saved waker identities in their original Wasm-local allocation order.
    pub(super) fn wakers(&self) -> Vec<(u32, u32)> {
        let mut nodes: Vec<_> = self.alls.iter().map(|(&i, all)| (all.node, i)).collect();
        nodes.sort_stable_by_key(|x| x.0);
        nodes
    }
}

/// A `join!` point's state in the save area (§14.5).
#[derive(Clone)]
pub(super) struct AllPoint {
    pub done: u32,
    pub node: u32,
    pub parts: Vec<(u32, Vec<u32>, hd_types::Ty)>,
}

/// The frame: the body's parameter 0.
pub(super) const FRAME: u32 = 0;

impl Em<'_, '_, '_> {
    /// The homogeneous list frame, unlike the fixed-arity tuple point.
    pub(super) fn all_frame(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        // ruled out: no task list. Lowering checks the declaration
        // against the intrinsic table (`hd_resolve::intrinsics`), whose
        // `task_all_frame` takes one `List`: `[T](List[Suspend[T]])`.
        let tasks = args[0];
        let inner = self.susp_inner(self.ty_of(i));
        let (_, elem) = self.elem(inner)?;
        self.comp(tasks, 0)?;
        self.f.helper(Helper::All { part: 0, elem });
        self.store_owned(i)
    }

    pub(super) fn range(&self, x: u32) -> Option<(u32, u32)> {
        self.susp.as_ref().and_then(|s| s.ranges.get(&x).copied())
    }

    fn pc(&self) -> u32 {
        self.susp.as_ref().map_or(0, |s| s.pc)
    }

    /// Pushes whether `lo <= pc <= hi`.
    pub(super) fn in_range(&mut self, lo: u32, hi: u32) {
        let pc = self.pc();
        self.f.get(pc);
        self.f.u32(lo);
        if lo == hi {
            self.f.op(I::I32Eq);
            return;
        }
        self.f.op(I::I32GeU);
        self.f.get(pc);
        self.f.u32(hi);
        self.f.op(I::I32LeU);
        self.f.op(I::I32And);
    }

    /// `if pc == 0` around fresh-path-only code; no guard outside a
    /// machine.
    pub(super) fn fresh_only(
        &mut self,
        f: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        if self.susp.is_none() {
            return f(self);
        }
        let pc = self.pc();
        self.f.get(pc);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        f(self)?;
        self.close();
        Ok(())
    }

    /// `if pc == 0 || lo <= pc <= hi` around code that holds the states
    /// `x` covers: it runs fresh or when resuming into it.
    pub(super) fn entered(
        &mut self,
        x: u32,
        f: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        let Some((lo, hi)) = self.range(x) else {
            return self.fresh_only(f);
        };
        let pc = self.pc();
        self.f.get(pc);
        self.f.op(I::I32Eqz);
        self.in_range(lo, hi);
        self.f.op(I::I32Or);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        f(self)?;
        self.close();
        Ok(())
    }

    /// With the branch condition of `i` on the stack: on a resume, it
    /// becomes whether the resumed state is in `then`, so the branch that
    /// holds the point is entered.
    pub(super) fn resume_branch(&mut self, i: u32, then: u32) {
        if self.range(i).is_none() {
            return;
        }
        match self.range(then) {
            Some((lo, hi)) => self.in_range(lo, hi),
            None => self.f.i32(0),
        }
        let pc = self.pc();
        self.f.get(pc);
        self.f.op(I::I32Eqz);
        self.f.op(I::Select);
    }

    /// Branches from inside an open `Match` `i`'s arm blocks to the arm
    /// that holds the resumed state, before its decision runs.
    pub(super) fn resume_arms(&mut self, i: u32, arms: &[u32]) -> StageResult<()> {
        if self.range(i).is_none() {
            return Ok(());
        }
        for (k, arm) in arms.iter().enumerate() {
            if let Some((lo, hi)) = self.range(*arm) {
                self.in_range(lo, hi);
                let d = self.depth(Ctl::Arm(i, u32::try_from(k).expect("arms")))?;
                self.f.op(I::BrIf(d));
            }
        }
        Ok(())
    }

    /// Statements `items` of a block that holds a state: one `br_table` on
    /// `pc` enters the segment that holds the resumed state; fresh code
    /// falls through every segment.
    pub(super) fn resume_list(&mut self, items: &[u32]) -> StageResult<()> {
        let points: Vec<usize> = (0..items.len())
            .filter(|&j| self.range(items[j]).is_some())
            .collect();
        let run = |em: &mut Self, its: &[u32]| -> StageResult<()> {
            for &i in its {
                em.inst(i)?;
            }
            Ok(())
        };
        let Some(&first) = points.first() else {
            return run(self, items);
        };
        if points.len() == 1 {
            if first > 0 {
                self.fresh_only(|em| run(em, &items[..first]))?;
            }
            return run(self, &items[first..]);
        }
        let ranges: Vec<(u32, u32)> = points
            .iter()
            .filter_map(|&j| self.range(items[j]))
            .collect();
        let (lo, hi) = (ranges[0].0, ranges[ranges.len() - 1].1);
        for _ in 0..=points.len() {
            self.open(Ctl::Plain);
        }
        let targets: Vec<u32> = (lo..=hi)
            .map(|s| {
                ranges
                    .iter()
                    .position(|&(a, b)| a <= s && s <= b)
                    .map_or(0, |k| u32::try_from(k + 1).expect("segments"))
            })
            .collect();
        let pc = self.pc();
        self.f.get(pc);
        self.f.u32(lo);
        self.f.op(I::I32Sub);
        self.f.br_table(targets, 0);
        self.close();
        run(self, &items[..first])?;
        for (k, &p) in points.iter().enumerate() {
            self.close();
            let next = points.get(k + 1).copied().unwrap_or(items.len());
            run(self, &items[p..next])?;
        }
        Ok(())
    }

    /// Allocates the frame when it is still 0 (§14.3): a zeroed counted
    /// object with this body's poll and cancel.
    fn materialize(&mut self) -> StageResult<()> {
        let key = self.susp_key()?;
        self.f.get(FRAME);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.f.helper(Helper::Frame(key));
        self.f.set(FRAME);
        self.flags_or(FRAME, RUNNING);
        self.retain_row(CountAt::Materialize, 0)?;
        self.f.op(I::End);
        Ok(())
    }

    /// Copies provider data and the open context in this physical row ABI.
    /// A cold constructor omits the body's frame parameter (shift 1).
    fn retain_row(&mut self, at: CountAt, shift: u32) -> StageResult<()> {
        let row = self
            .susp
            .as_ref()
            .map(|s| s.provs.clone())
            .unwrap_or_default();
        if row.is_empty() {
            return Ok(());
        }
        let Some(reason) = self.stated_count(Tag::Retain, at, CountPart::Row) else {
            // ruled out: counts::expected states Cold/Materialize Row retains
            // for cold_row::roots, including every closure's caller context.
            return unsupported("a suspension row without its stated copy");
        };
        self.because(reason, |em| {
            for d in row {
                em.ref_retain(d - shift);
            }
        });
        Ok(())
    }

    fn susp_key(&self) -> StageResult<Hash128> {
        match &self.susp {
            Some(s) => Ok(s.key),
            // ruled out: only suspend/cancel_here materialize under point;
            // emit::instance installs Susp for their checked suspending ABI.
            None => unsupported("a suspension point outside a suspending body"),
        }
    }

    /// Pending at state `k`, waiting on the child in local `child`, whose
    /// count the frame takes: saves every local and returns the frame.
    fn suspend(&mut self, k: u32, child: u32) -> StageResult<()> {
        self.materialize()?;
        self.f.push(Ins::Save(k));
        self.f.get(FRAME);
        self.f.get(child);
        self.f.op(I::I32Store(mem(HEAP, F_CHILD, 2)));
        self.f.get(FRAME);
        self.f.u32(k);
        self.f.op(I::I32Store(mem(HEAP, F_STATE, 2)));
        for v in self.results.clone() {
            push_zero(&mut self.f, v);
        }
        self.f.get(FRAME);
        self.f.op(I::Return);
        Ok(())
    }

    /// Pushes whether the task at the address in local `c` is cancelled.
    fn is_cancelled(&mut self, c: u32) {
        self.f.get(c);
        self.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        self.f.i32(CANCELLED);
        self.f.op(I::I32And);
    }

    /// The awaited child in local `child` (or none, for `join!`) was
    /// cancelled by another task, so this task is cancelled at the point
    /// (`req.cancel.await`): its frame is marked and the point's
    /// cancellation branch runs. A child the frame does not hold yet
    /// becomes its child, with the count in `child`.
    fn cancel_here(&mut self, at: usize, child: Option<u32>) -> StageResult<()> {
        // ruled out: callers pass point's cancellation target before emitting the branch.
        self.materialize()?;
        if let Some(c) = child {
            self.f.get(FRAME);
            self.f.get(c);
            self.f.op(I::I32Store(mem(HEAP, F_CHILD, 2)));
        }
        self.flags_or(FRAME, CANCELLED | UNWINDING);
        let d = u32::try_from(self.ctl.len() - 1 - at).expect("depth");
        self.f.op(I::Br(d));
        Ok(())
    }

    /// Sets bits `bits` in the flags of the task in local `t`.
    fn flags_or(&mut self, t: u32, bits: i32) {
        self.f.get(t);
        self.f.get(t);
        self.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        self.f.i32(bits);
        self.f.op(I::I32Or);
        self.f.op(I::I32Store(mem(HEAP, F_FLAGS, 2)));
    }

    /// Cancels the task in local `c`, when there is one, and sets local
    /// `wait` when it is left unwinding.
    fn cancel_task(&mut self, c: u32, wait: u32) {
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(c);
        self.f.get(c);
        self.f.op(I::I32Load(mem(HEAP, F_CANCEL, 2)));
        self.f.call_indirect(vec![Val::I32], vec![]);
        self.f.get(c);
        self.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        self.f.i32(UNWINDING);
        self.f.op(I::I32And);
        self.f.op(I::If(BlockType::Empty));
        self.f.i32(1);
        self.f.set(wait);
        self.f.op(I::End);
        self.f.op(I::End);
    }

    /// A suspension point (§14.2 to §14.6): its await in a block whose
    /// end is the cancellation branch, entered on a resume for
    /// cancellation or when the awaited child turns out cancelled.
    pub(super) fn point(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        // Checking puts a point only in a suspending body
        // (`bang-call-outside-suspension`), whose plan numbers it.
        let k = self.susp.as_ref().expect("a suspending body").states[&i];
        let pc = self.pc();
        self.slot(i)?;
        // A point in a `using!` binding's dispose suite is shielded: it has
        // no cancellation branch, by (flow.using.async.cancel.shielded).
        if self.shield > 0 {
            return self.await_point(i, k, a, bw);
        }
        self.open(Ctl::Plain);
        self.open(Ctl::Plain);
        let at = self.ctl.len() - 1;
        self.f.get(pc);
        self.f.u32(k);
        self.f.op(I::I32Eq);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.f.get(FRAME);
        self.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        self.f.i32(CANCELLED);
        self.f.op(I::I32And);
        self.f.op(I::BrIf(1));
        self.close();
        let outer = self.susp.as_mut().and_then(|s| s.cancel_to.replace(at));
        let r = self.await_point(i, k, a, bw);
        if let Some(s) = self.susp.as_mut() {
            s.cancel_to = outer;
        }
        r?;
        self.f.op(I::Br(1));
        self.close();
        // Cancel the child, then leave through the enclosing scopes'
        // ladders. A child left unwinding is waited for here first; the
        // child cancels are bracketed as a frame's cancel brackets its
        // work (`req.abandon.carry`).
        let (c, wait, seen) = (
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
        );
        self.f.i32(0);
        self.f.set(wait);
        self.f.helper(Helper::CancelEnter);
        self.f.set(seen);
        let all = self.susp.as_ref().and_then(|s| s.alls.get(&i).cloned());
        if let Some(all) = &all {
            // `join!` cancels its unfinished children in argument order
            // (`req.combinator.cancel-order`).
            for (j, (ch, _, _)) in all.parts.iter().enumerate() {
                self.f.get(all.done);
                self.f.i32(1 << j);
                self.f.op(I::I32And);
                self.f.op(I::I32Eqz);
                self.f.op(I::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.comp(*ch, 0)?;
                self.f.set(c);
                self.cancel_task(c, wait);
                self.close();
            }
            self.f.i32(0);
            self.f.set(c);
        } else {
            self.f.get(FRAME);
            self.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
            self.f.set(c);
            self.cancel_task(c, wait);
        }
        self.f.get(FRAME);
        self.f.get(seen);
        self.f.helper(Helper::CancelLeave);
        self.f.get(wait);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.suspend(k, c)?;
        self.close();
        // The results of the children that completed are let go.
        if let Some(all) = all {
            for (j, (_, ls, t)) in all.parts.iter().enumerate() {
                if !self.needs_drop(*t) {
                    continue;
                }
                self.f.get(all.done);
                self.f.i32(1 << j);
                self.f.op(I::I32And);
                self.f.op(I::If(BlockType::Empty));
                self.release_join_result(i, ls, *t)?;
                self.f.op(I::End);
            }
            self.f.i32(0);
            self.f.set(all.done);
            self.count_point_ref(i, CountPart::Waker, all.node, false)?;
            self.f.i32(0);
            self.f.set(all.node);
        } else {
            // A ladder that waits keeps its own child in the frame's slot.
            // A join's local c is zero: its children live in the argument list.
            self.release_child(i, c)?;
        }
        // The ladders run as fresh code.
        self.f.i32(0);
        self.f.set(pc);
        self.cancel_from = i;
        self.exit(Exit::Cancel)?;
        self.close();
        Ok(())
    }

    /// The await of point `i` at state `k`.
    fn await_point(&mut self, i: u32, k: u32, a: u32, bw: u32) -> StageResult<()> {
        match self.b.tags[i as usize] {
            Tag::AwaitValue => self.await_stored(i, k, false, &|em| em.comp(a, 0)),
            Tag::Await => {
                let rec = self.rec(bw);
                let args = rec[..rec.len().saturating_sub(3)].to_vec();
                match self.calls.get(&i).cloned() {
                    // Checking calls with `!` only a suspending function
                    // (`not-suspending`), which an implementation matches
                    // (`trait-method-signature`).
                    Some(Target::Call(t)) if t.kind == TargetKind::Instance => {
                        self.await_direct(i, k, t.key, t.item, t.args, &args)
                    }
                    Some(Target::Dyn) => {
                        if self.shield == 0 && self.drives(a) {
                            return self.await_driven(i, k, a, &args);
                        }
                        self.await_stored(i, k, true, &|em| em.push_dyn(i, a, &args, None, None))
                    }
                    // ruled out: mono scans each Await's checked callee;
                    // req.suspend.type.not-suspending excludes plain intrinsics.
                    _ => unsupported("a bang call that collection did not resolve"),
                }
            }
            // `AwaitAll`: the checker emits `race!` as std hd, never an
            // `AwaitRace`.
            _ => self.await_all(i, k, bw),
        }
    }

    /// `g!(x)` of a known suspending instance: its body called directly,
    /// with the saved child frame on a resume (§14.3).
    fn await_direct(
        &mut self,
        i: u32,
        k: u32,
        key: Hash128,
        item: hd_base::DefId,
        targs: hd_types::TyList,
        args: &[u32],
    ) -> StageResult<()> {
        let pc = self.pc();
        let ch = self.f.local(Val::I32);
        self.f.i32(0);
        self.f.set(ch);
        self.f.get(pc);
        self.f.u32(k);
        self.f.op(I::I32Eq);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(FRAME);
        self.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
        self.f.set(ch);
        self.f.op(I::End);
        let res = self.vals(self.ty(i))?;
        let tmp: Vec<u32> = res.iter().map(|v| self.f.local(*v)).collect();
        let nc = self.f.local(Val::I32);
        let shielded = self.shield > 0;
        if shielded {
            self.shield_enter(k, ch, &tmp, nc);
        }
        self.f.get(ch);
        self.push_args(i, args, None, 0, self.f.reason)?;
        let c = self.push_call_providers(i, item, targs)?;
        self.f.call(Sym::Part(key, Part::Body));
        self.f.set(nc);
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.release_call_ctx(i, c)?;
        if shielded {
            self.shield_leave(i, nc, Some(ch))?;
        }
        self.f.get(nc);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        // A child that ended cancelled hands back its frame: this task is
        // cancelled too (`req.cancel.await`).
        if let Some(at) = self.susp.as_ref().and_then(|s| s.cancel_to) {
            self.is_cancelled(nc);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(at, Some(nc))?;
            self.close();
        }
        self.suspend(k, nc)?;
        self.close();
        // Ready: a resumed child's frame is done with.
        self.release_child(i, ch)?;
        self.f.i32(0);
        self.f.set(pc);
        for l in &tmp {
            self.f.get(*l);
        }
        self.store_owned(i)
    }

    /// One count less on the child frame in local `c`, which the frame
    /// held, and none held any more.
    fn release_child(&mut self, i: u32, c: u32) -> StageResult<()> {
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(FRAME);
        self.f.i32(0);
        self.f.op(I::I32Store(mem(HEAP, F_CHILD, 2)));
        self.count_child(i, c, false)?;
        self.f.op(I::End);
        Ok(())
    }

    /// Lowers the stated owner of a point's child, never its result.
    fn count_child(&mut self, i: u32, c: u32, retain: bool) -> StageResult<()> {
        if self.input.runtime.is_none() {
            return Ok(());
        }
        self.count_point_ref(i, CountPart::Child, c, retain)
    }

    /// A physical reference count stated at a suspension point.
    pub(super) fn count_point_ref(
        &mut self,
        i: u32,
        part: CountPart,
        c: u32,
        retain: bool,
    ) -> StageResult<()> {
        let tag = if retain { Tag::Retain } else { Tag::Release };
        let Some(reason) = self.operation_count(i, tag, part) else {
            // ruled out: own::child states Await/AwaitValue child ownership;
            // own::waker states AwaitAll's release, including its cleanup path.
            return unsupported("a suspension point without its stated reference count");
        };
        self.because(reason, |em| {
            if retain {
                em.ref_retain(c);
            } else {
                em.f.get(c);
                em.f.helper(Helper::ReleaseRef);
            }
        });
        Ok(())
    }

    /// A stored suspension (`s!()`, a trait-value bang call): polled
    /// through its poll slot. `fresh` pushes it on the fresh path: a new
    /// cold frame when `owned`, whose count the frame takes when it waits
    /// and which is let go once it is Ready; else a borrowed suspension,
    /// of which the frame takes a count of its own when it waits.
    fn await_stored(
        &mut self,
        i: u32,
        k: u32,
        owned: bool,
        fresh: &dyn Fn(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        let pc = self.pc();
        let (s, held) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.f.get(pc);
        self.f.u32(k);
        self.f.op(I::I32Eq);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.f.get(FRAME);
        self.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
        self.f.set(s);
        self.f.i32(1);
        self.f.set(held);
        self.f.op(I::Else);
        fresh(self)?;
        self.f.set(s);
        self.f.i32(0);
        self.f.set(held);
        self.close();
        let res = self.vals(self.ty(i))?;
        let tmp: Vec<u32> = res.iter().map(|v| self.f.local(*v)).collect();
        let ready = self.f.local(Val::I32);
        // `gone` is the child a shielded point is left to wait on.
        let shielded = self.shield > 0;
        let gone = self.f.local(Val::I32);
        if shielded {
            self.shield_enter(k, s, &tmp, gone);
        }
        self.f.get(s);
        self.f.get(s);
        self.f.op(I::I32Load(mem(HEAP, F_POLL, 2)));
        self.f
            .call_indirect(vec![Val::I32], [vec![Val::I32], res].concat());
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.f.set(ready);
        if shielded {
            self.f.i32(0);
            self.f.set(gone);
            self.f.get(ready);
            self.f.op(I::I32Eqz);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(s);
            self.f.set(gone);
            self.f.op(I::End);
            self.shield_leave(i, gone, None)?;
            self.f.get(gone);
            self.f.op(I::I32Eqz);
            self.f.set(ready);
        }
        self.f.get(ready);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        // The frame takes its own count of a borrowed suspension it starts
        // waiting on.
        if !owned {
            self.f.get(held);
            self.f.op(I::I32Eqz);
            self.f.op(I::If(BlockType::Empty));
            self.count_child(i, s, true)?;
            self.f.op(I::End);
        }
        if let Some(at) = self.susp.as_ref().and_then(|s| s.cancel_to) {
            self.is_cancelled(s);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(at, Some(s))?;
            self.close();
        }
        self.suspend(k, s)?;
        self.close();
        self.f.get(held);
        self.f.op(I::If(BlockType::Empty));
        self.release_child(i, s)?;
        if owned && self.input.runtime.is_some() {
            self.f.op(I::Else);
            self.count_child(i, s, false)?;
        }
        self.f.op(I::End);
        self.f.i32(0);
        self.f.set(pc);
        for l in &tmp {
            self.f.get(*l);
        }
        self.store_owned(i)
    }

    /// Whether a dynamic bang call of the callee record at `a` goes
    /// through its method's drive slot (§23.39).
    fn drives(&self, a: u32) -> bool {
        crate::drive::callee(self.b, a).is_some_and(|(trait_, method)| {
            self.m.driven.contains(&method) && crate::drive::drivable(self.env(), trait_, method)
        })
    }

    /// `x.m!(args)` through `m`'s drive slot (§23.39), as
    /// [`Em::await_stored`] awaits the cold call's frame: a fresh start
    /// calls the drive slot, whose frame is 0 when the call is done and
    /// was then released by the slot; a Pending frame becomes the child,
    /// and a resume polls it as before.
    fn await_driven(&mut self, i: u32, k: u32, a: u32, args: &[u32]) -> StageResult<()> {
        let pc = self.pc();
        let (s, held) = (self.f.local(Val::I32), self.f.local(Val::I32));
        let res = self.vals(self.ty(i))?;
        let tmp: Vec<u32> = res.iter().map(|v| self.f.local(*v)).collect();
        let ready = self.f.local(Val::I32);
        self.f.get(pc);
        self.f.u32(k);
        self.f.op(I::I32Eq);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        self.f.get(FRAME);
        self.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
        self.f.set(s);
        self.f.i32(1);
        self.f.set(held);
        self.f.get(s);
        self.f.get(s);
        self.f.op(I::I32Load(mem(HEAP, F_POLL, 2)));
        self.f
            .call_indirect(vec![Val::I32], [vec![Val::I32], res.clone()].concat());
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.f.set(ready);
        self.f.op(I::Else);
        self.push_dyn(i, a, args, None, Some((self.ty(i), None)))?;
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.f.set(s);
        self.f.i32(0);
        self.f.set(held);
        self.f.get(s);
        self.f.op(I::I32Eqz);
        self.f.set(ready);
        self.close();
        self.f.get(ready);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        if let Some(at) = self.susp.as_ref().and_then(|s| s.cancel_to) {
            self.is_cancelled(s);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(at, Some(s))?;
            self.close();
        }
        self.suspend(k, s)?;
        self.close();
        self.f.get(held);
        self.f.op(I::If(BlockType::Empty));
        self.release_child(i, s)?;
        self.f.op(I::End);
        self.f.i32(0);
        self.f.set(pc);
        for l in &tmp {
            self.f.get(*l);
        }
        self.store_owned(i)
    }

    /// Ends one completed child result where its point's cleanup packet
    /// says so.
    fn release_join_result(&mut self, i: u32, ls: &[u32], ty: hd_types::Ty) -> StageResult<()> {
        match self.operation_count(i, Tag::Release, CountPart::Results) {
            Some(reason) => self.because(reason, |em| em.release_locals(ls, ty)),
            None => Ok(()),
        }
    }

    /// `join!(a, b, ...)` (§14.5): the first poll polls every child;
    /// later polls visit only unfinished children whose waker bit is set.
    /// The counted node lives in the save area, and registrations keep
    /// it and its enclosing nodes alive until completion or cancellation.
    fn await_all(&mut self, i: u32, k: u32, bw: u32) -> StageResult<()> {
        let children = self.rec(bw);
        let pc = self.pc();
        // Checking types `join!` the tuple of its children's results.
        let tys = crate::layout::tuple_members(
            self.pool(),
            crate::layout::strip(self.pool(), self.env(), self.ty(i)),
        );
        let (done, gone, ready, s) = (
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
            self.f.local(Val::I32),
        );
        let mut parts = Vec::new();
        for (&ch, &t) in children.iter().zip(&tys) {
            let vs = self.vals(t)?;
            let ls: Vec<u32> = vs.iter().map(|v| self.f.local(*v)).collect();
            parts.push((ch, ls, t));
        }
        let node = self.f.local(Val::I32);
        // ruled out: req.combinator.join-arity bounds children.len() to 0..=31.
        let all = i32::MAX >> (31 - children.len());
        let point = AllPoint { done, node, parts };
        if let Some(x) = self.susp.as_mut() {
            x.alls.insert(i, point.clone());
        }
        self.fresh_only(|em| {
            em.f.i32(0);
            em.f.set(done);
            em.f.i32(all);
            em.f.helper(Helper::WakerNew);
            em.f.set(node);
            Ok(())
        })?;
        self.f.get(node);
        self.f.helper(Helper::WakerParent);
        let parent = self.f.local(Val::I32);
        let parent_bit = self.f.local(Val::I32);
        self.f.global_get(Global::Waker);
        self.f.set(parent);
        self.f.global_get(Global::WakeBit);
        self.f.set(parent_bit);
        self.f.i32(0);
        self.f.set(gone);
        for (j, (ch, ls, t)) in point.parts.iter().enumerate() {
            let bit = 1i32 << j;
            self.f.get(done);
            self.f.i32(bit);
            self.f.op(I::I32And);
            self.f.op(I::I32Eqz);
            self.f.get(node);
            self.f.op(I::I32Load(mem(HEAP, crate::task::W_READY, 2)));
            self.f.i32(bit);
            self.f.op(I::I32And);
            self.f.i32(0);
            self.f.op(I::I32Ne);
            self.f.op(I::I32And);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.f.get(node);
            self.f.get(node);
            self.f.op(I::I32Load(mem(HEAP, crate::task::W_READY, 2)));
            self.f.i32(!bit);
            self.f.op(I::I32And);
            self.f.op(I::I32Store(mem(HEAP, crate::task::W_READY, 2)));
            self.f.get(node);
            self.f.global_set(Global::Waker);
            self.f.i32(bit);
            self.f.global_set(Global::WakeBit);
            if !self.input.release {
                self.f.global_get(Global::AllPolls);
                self.f.i32(1);
                self.f.op(I::I32Add);
                self.f.global_set(Global::AllPolls);
            }
            self.comp(*ch, 0)?;
            self.f.set(s);
            self.f.get(s);
            self.f.get(s);
            self.f.op(I::I32Load(mem(HEAP, F_POLL, 2)));
            let vs = self.vals(*t)?;
            self.f
                .call_indirect(vec![Val::I32], [vec![Val::I32], vs].concat());
            for l in ls.iter().rev() {
                self.f.set(*l);
            }
            self.f.set(ready);
            self.f.get(parent);
            self.f.global_set(Global::Waker);
            self.f.get(parent_bit);
            self.f.global_set(Global::WakeBit);
            self.f.get(ready);
            self.f.op(I::If(BlockType::Empty));
            self.f.get(done);
            self.f.i32(bit);
            self.f.op(I::I32Or);
            self.f.set(done);
            self.f.op(I::Else);
            self.f.get(gone);
            self.is_cancelled(s);
            self.f.op(I::I32Or);
            self.f.set(gone);
            self.f.op(I::End);
            self.close();
        }
        self.f.get(done);
        self.f.i32(all);
        self.f.op(I::I32Ne);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        if let Some(at) = self.susp.as_ref().and_then(|s| s.cancel_to) {
            self.f.get(gone);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(at, None)?;
            self.close();
        }
        let no_child = self.f.local(Val::I32);
        self.suspend(k, no_child)?;
        self.close();
        self.count_point_ref(i, CountPart::Waker, node, false)?;
        self.f.i32(0);
        self.f.set(node);
        // The results move into the tuple; a stale mask must not name
        // them to the release glue.
        self.f.i32(0);
        self.f.set(done);
        self.f.i32(0);
        self.f.set(pc);
        for (_, ls, _) in &point.parts {
            for l in ls {
                self.f.get(*l);
            }
        }
        self.store_owned(i)
    }

    /// Opens a shielded point's block (`crates/hd_wasm_linear/DESIGN.md` §14.6,
    /// "Abandoning"). Resumed into under `abandon()`, the point kills its
    /// waiting child, the `dispose!`, with no step: it leaves the block
    /// with `out` 0 and the results zero, so the suite goes on as if the
    /// dispose returned.
    fn shield_enter(&mut self, k: u32, child: u32, tmp: &[u32], out: u32) {
        let pc = self.pc();
        let wait = self.f.local(Val::I32);
        self.open(Ctl::Plain);
        self.f.get(pc);
        self.f.u32(k);
        self.f.op(I::I32Eq);
        self.f.global_get(crate::module::Global::Aband);
        self.f.op(I::I32Eqz);
        self.f.op(I::I32Eqz);
        self.f.op(I::I32And);
        self.f.op(I::If(BlockType::Empty));
        self.cancel_task(child, wait);
        self.f.zero(out);
        for l in tmp {
            self.f.zero(*l);
        }
        self.f.op(I::Br(1));
        self.f.op(I::End);
    }

    /// Closes a shielded point's block after the step left its pending
    /// child in `out` (0 when it returned). A child left pending under
    /// `abandon()`, or one cancelled because it awaited a suspension
    /// cancelled elsewhere (`req.abandon.dependency`), is abandoned and
    /// `out` cleared, so the suite goes on; this frame's cleanup is then
    /// `ABANDONED`. A child that a fresh step made (`fresh` 0) loses the
    /// count it came with.
    fn shield_leave(&mut self, i: u32, out: u32, fresh: Option<u32>) -> StageResult<()> {
        let wait = self.f.local(Val::I32);
        self.f.get(out);
        self.f.op(I::If(BlockType::Empty));
        self.f.global_get(crate::module::Global::Aband);
        self.is_cancelled(out);
        self.f.op(I::I32Or);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(FRAME);
        self.f.op(I::If(BlockType::Empty));
        self.flags_or(FRAME, ABANDONED);
        self.f.op(I::End);
        crate::task::abandon_by(&mut self.f, 1);
        self.cancel_task(out, wait);
        crate::task::abandon_by(&mut self.f, -1);
        if let Some(h) = fresh
            && self.input.runtime.is_some()
        {
            self.f.get(h);
            self.f.op(I::I32Eqz);
            self.f.op(I::If(BlockType::Empty));
            self.count_child(i, out, false)?;
            self.f.op(I::End);
        }
        self.f.zero(out);
        self.f.op(I::End);
        self.f.op(I::End);
        self.close();
        Ok(())
    }

    /// One more count on the address in local `x`.
    fn ref_retain(&mut self, x: u32) {
        if let Some(r) = self.input.runtime {
            self.f.get(x);
            self.f
                .count_call(Sym::Inst(r.retain), crate::views::CountAction::Retain);
        }
    }
}

/// Pushes the zero of `v`.
pub(super) fn push_zero(f: &mut crate::module::Func, v: Val) {
    f.op(match v {
        Val::I32 => I::I32Const(0),
        Val::I64 => I::I64Const(0),
        Val::F32 => I::F32Const(0.0.into()),
        Val::F64 => I::F64Const(0.0.into()),
    });
}

/// The save area of a body whose locals `0..n` are `vals`: each local's
/// offset in the frame, but the frame's own and `pc`'s, and the frame's
/// payload size.
pub(super) fn save_area(vals: &[Val], skip: &[u32]) -> (Vec<(u32, u32, Val)>, u32) {
    let mut at = F_SAVED;
    let mut out = Vec::new();
    for (l, v) in vals.iter().enumerate() {
        let l = u32::try_from(l).expect("locals");
        if skip.contains(&l) {
            continue;
        }
        let size = match v {
            Val::I32 | Val::F32 => 4,
            Val::I64 | Val::F64 => 8,
        };
        at = at.div_ceil(size) * size;
        out.push((l, at, *v));
        at += size;
    }
    (out, at.div_ceil(8) * 8)
}

/// The store of local `l` of `v` at `at` in the frame.
pub(super) fn store_local(f: &mut crate::module::Func, l: u32, at: u32, v: Val) {
    f.get(FRAME);
    f.get(l);
    f.op(store_op(v, at));
}

/// The store instruction of a `v` at `at` in a heap object.
pub(super) fn store_op(v: Val, at: u32) -> I {
    let at = u64::from(at);
    match v {
        Val::I32 => I::I32Store(mem(HEAP, at, 2)),
        Val::I64 => I::I64Store(mem(HEAP, at, 3)),
        Val::F32 => I::F32Store(mem(HEAP, at, 2)),
        Val::F64 => I::F64Store(mem(HEAP, at, 3)),
    }
}

/// The load instruction of a `v` at `at` in a heap object.
pub(super) fn load_op(v: Val, at: u32) -> I {
    let at = u64::from(at);
    match v {
        Val::I32 => I::I32Load(mem(HEAP, at, 2)),
        Val::I64 => I::I64Load(mem(HEAP, at, 3)),
        Val::F32 => I::F32Load(mem(HEAP, at, 2)),
        Val::F64 => I::F64Load(mem(HEAP, at, 3)),
    }
}

/// Replaces a body's [`Ins::Save`]s and [`Ins::LoadAll`] by the stores
/// and loads of its save area `area`: a save at state `k` stores
/// `held[k]` when there is one, else all of it.
pub(super) fn expand(
    f: &mut crate::module::Func,
    area: &[(u32, u32, Val)],
    held: &HashMap<u32, Vec<(u32, u32, Val)>>,
) {
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        match ins {
            Ins::Save(k) => {
                for &(l, at, v) in held.get(&k).map_or(area, Vec::as_slice) {
                    store_local(f, l, at, v);
                }
            }
            Ins::LoadAll => {
                for &(l, at, v) in area {
                    f.get(FRAME);
                    f.op(load_op(v, at));
                    f.set(l);
                }
            }
            other => f.push(other),
        }
    }
    f.origin_at = outer;
}

/// Each Wasm local of the body `em` emits with the TIR locals and values
/// it holds.
fn owners(em: &Em<'_, '_, '_>) -> HashMap<u32, Vec<u32>> {
    let mut owners: HashMap<u32, Vec<u32>> = HashMap::default();
    for (&l, ws) in &em.locals {
        let word = hd_tir::ir::place::BARE_LOCAL | u32::try_from(l).expect("local");
        for w in ws {
            owners.entry(*w).or_default().push(word);
        }
    }
    for (&i, ws) in &em.vals {
        for w in ws {
            owners.entry(*w).or_default().push(i);
        }
    }
    owners
}

/// What the frame holds at each state of the body `em` emits, from the
/// TIR's live rows (`Body::live`): of save area `area`, each Wasm local
/// of a local or value live there, and every Wasm local that holds none,
/// which is the emitter's own, but those in `free`. A point with no row
/// holds all of it.
fn held_at(
    em: &Em<'_, '_, '_>,
    area: &[(u32, u32, Val)],
    owners: &HashMap<u32, Vec<u32>>,
    free: &hd_base::HashSet<u32>,
) -> HashMap<u32, Vec<(u32, u32, Val)>> {
    let mut out = HashMap::default();
    let Some(s) = em.susp.as_ref() else {
        return out;
    };
    for (&p, &k) in &s.states {
        let Ok(r) = em.b.live.binary_search_by_key(&p, |r| r.0) else {
            continue;
        };
        let live = &em.b.live[r].1;
        let kept = area
            .iter()
            .filter(|(w, _, _)| match owners.get(w) {
                None => !free.contains(w),
                Some(os) => os.iter().any(|o| live.binary_search(o).is_ok()),
            })
            .copied()
            .collect();
        out.insert(k, kept);
    }
    out
}

impl Em<'_, '_, '_> {
    /// Pushes the body's frame result after its results at its end: 0,
    /// Ready, once it marked a frame it has `EXITED`; or, when it left by
    /// its cancellation through the counting exit, the frame, no longer
    /// unwinding.
    pub(super) fn exit_frame(&mut self) {
        let Some(cancelled) = self.susp.as_ref().map(|s| s.cancelled) else {
            return;
        };
        self.f.get(FRAME);
        self.f.op(I::If(BlockType::Empty));
        self.flags_or(FRAME, EXITED);
        self.f.op(I::End);
        self.f.get(cancelled);
        self.f
            .op(I::If(BlockType::Result(wasm_encoder::ValType::I32)));
        self.unwound();
        self.f.get(FRAME);
        self.f.op(I::Else);
        self.f.i32(0);
        self.f.op(I::End);
    }

    /// Clears the frame's `UNWINDING`: its cancellation ended.
    pub(super) fn unwound(&mut self) {
        self.f.get(FRAME);
        self.f.get(FRAME);
        self.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        self.f.i32(!UNWINDING);
        self.f.op(I::I32And);
        self.f.op(I::I32Store(mem(HEAP, F_FLAGS, 2)));
    }

    /// The final step of `Exit::Cancel`: with the function's one exit,
    /// noted for [`Self::exit_frame`] and taken; without, the zero
    /// results and the frame returned.
    pub(super) fn leave_cancelled(&mut self) -> StageResult<()> {
        let Some(cancelled) = self.susp.as_ref().map(|s| s.cancelled) else {
            // ruled out: Exit::Cancel comes only from point or a suspending
            // dispose ladder; emit::instance installs Susp for their ABI.
            return unsupported("a cancellation outside a suspending body");
        };
        if self.counting {
            // The exit releases only its exit set.
            self.release_unwound(self.cancel_from)?;
            for l in self.ret.clone() {
                self.f.zero(l);
            }
            self.f.i32(1);
            self.f.set(cancelled);
            let d = self.depth(Ctl::Exit)?;
            self.f.op(I::Br(d));
            return Ok(());
        }
        self.flags_or(FRAME, EXITED);
        self.unwound();
        for v in self.results.clone() {
            push_zero(&mut self.f, v);
        }
        self.f.get(FRAME);
        self.f.op(I::Return);
        Ok(())
    }
}

/// Finishes suspending instance `key`'s emission (§23.21): expands the
/// body's saves and loads over its save area, and returns its cold
/// constructor, the instance's own function, after defining its body,
/// poll, cancel and release glue. `params` are the body's parameters,
/// the frame first; `plocals` the TIR locals of the others.
pub(super) fn finish(
    mut em: Em<'_, '_, '_>,
    key: Hash128,
    plocals: &[usize],
    params: &[Val],
    ret: hd_types::Ty,
    closure: Option<usize>,
    held: Option<bool>,
) -> StageResult<crate::module::Func> {
    let pc = em.susp.as_ref().map_or(0, |s| s.pc);
    let mut vals_all: Vec<Val> = em.f.params.clone();
    vals_all.extend(em.f.locals.iter().copied());
    let local_frame = em.frame.filter(|(base, _)| *base != NONE);
    let mut skipped = vec![FRAME, pc];
    if let Some((base, _)) = local_frame {
        skipped.push(base);
    }
    // A parameter in a cell: once the cell holds it, its arrival locals are
    // never read again.
    let arrived = |em: &Em<'_, '_, '_>, l: usize| -> Vec<u32> {
        em.param_vals
            .get(&l)
            .unwrap_or(&em.locals[&l])
            .iter()
            .map(|x| x - 1)
            .collect()
    };
    skipped.extend(em.param_vals.values().flatten().copied());
    let (area, size) = save_area(&vals_all, &skipped);
    let owners = owners(&em);
    // The emitter's own locals that need no save: the resume reloads the
    // frame and runs from there, and no run from there reads one before
    // writing it.
    let resumed = em.f.body.iter().position(|i| matches!(i, Ins::LoadAll));
    let mut free: hd_base::HashSet<u32> =
        match resumed.and_then(|at| crate::opt::live_after(&em.f, at)) {
            Some(live) => area
                .iter()
                .map(|x| x.0)
                .filter(|w| !owners.contains_key(w) && !live.contains(w))
                .collect(),
            None => hd_base::HashSet::default(),
        };
    em.m.frames.insert(key, size);
    let at_of = |l: u32| area.iter().find(|x| x.0 == l).map(|x| (x.1, x.2));
    let results = em.results.clone();
    let name = em.f.name.clone();
    let mut body = std::mem::take(&mut em.f);
    let body_drop_states = em.drop_states.clone();

    // The release glue: the places the body owns while it waits, unless
    // it has left, then the child.
    em.f = em.m.func(format!("{name}.drop"), vec![Val::I32], vec![]);
    em.inherit_origin_source(Reason::Conservative("frame drop"));
    em.counting = em.input.runtime.is_some();
    if em.counting {
        em.f.get(FRAME);
        em.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        em.f.i32(EXITED);
        em.f.op(I::I32And);
        em.f.op(I::I32Eqz);
        em.f.op(I::If(BlockType::Empty));
        // Pins #1430: a started frame is only ever traced, never freed or
        // collected as garbage, so this glue releases no value row.
        if let Some(rt) = em.input.runtime.filter(|r| r.checked.is_some()) {
            em.f.get(FRAME);
            em.f.op(I::I32Load(mem(HEAP, crate::task::F_STATE, 2)));
            em.f.op(I::If(BlockType::Empty));
            em.f.call(crate::module::Sym::Inst(rt.cycle.marking));
            em.f.op(I::I32Eqz);
            em.f.op(I::If(BlockType::Empty));
            em.f.op(I::Unreachable);
            em.f.op(I::End);
            em.f.op(I::End);
        }
        em.reload_drop_flags(&area);
        // What a cold frame holds of the exit set: its parameters and a
        // closure's captures. The body's other locals are set only once it
        // runs, and a started frame is never freed (pinned above).
        let mut places: Vec<(usize, Vec<u32>)> = Vec::new();
        let mut owned: Vec<usize> =
            em.b.exit_drops
                .iter()
                .filter(|&&(sub, l)| {
                    sub.raw() == u32::from(em.sub_id)
                        && (plocals.contains(&l.idx()) || em.b.cap_local.contains(&l))
                })
                .map(|&(_, l)| l.idx())
                .collect();
        owned.sort_stable();
        owned.dedup();
        for l in owned {
            if !em.loan_locals.contains_key(&l)
                && !em.borrowed.contains(&l)
                && let Some(ls) = em.locals.get(&l)
            {
                places.push((l, ls.clone()));
            }
        }
        // The pointer loans the waiting body holds end with it.
        em.borrow_abandon(&at_of);
        let load = |em: &mut Em<'_, '_, '_>, ls: &[u32]| {
            let mut loaded = Vec::new();
            for &l in ls {
                let (at, v) = at_of(l)?;
                let x = em.f.local(v);
                em.f.get(FRAME);
                em.f.op(load_op(v, at));
                em.f.set(x);
                loaded.push(x);
            }
            Some(loaded)
        };
        for (l, ls) in places {
            let Some(loaded) = load(&mut em, &ls) else {
                continue;
            };
            em.borrow_release(l);
            em.because(Reason::Cancelled, |em| {
                if em.is_cell(l) {
                    em.count_local(l, &loaded, false)
                } else {
                    em.release_drop_place(&loaded, em.sub(em.b.local_ty[l]), &Em::local_place(l))
                }
            })?;
        }
        // Reference parameter slots precede all body-allocated waker nodes.
        // These typed identities replace opaque temporary ownership inference.
        for (l, held) in em.frame_refs.clone() {
            let Some(loaded) = load(&mut em, &[held]) else {
                continue;
            };
            let Some(reason) = em.stated_count(
                Tag::Release,
                CountAt::FrameDrop,
                CountPart::Referent(u32::try_from(l).expect("local")),
            ) else {
                // ruled out: own::referent states FrameDrop for each saved
                // reference parameter's object, separately from its value count.
                return unsupported("a frame reference without its stated object release");
            };
            em.because(reason, |em| {
                em.f.get(loaded[0]);
                em.f.helper(Helper::ReleaseRef);
            });
        }
        em.f.op(I::End);
        // A parameter a body with no suspension point borrows from its
        // frame (`Em::instance_loans`): the frame's count outlives the body.
        for &l in plocals {
            if em.borrowed.contains(&l)
                && let Some(ls) = em.locals.get(&l).cloned()
                && let Some(slots) = ls.iter().map(|x| at_of(*x)).collect::<Option<Vec<_>>>()
            {
                let mut loaded = Vec::new();
                for (at, v) in slots {
                    let x = em.f.local(v);
                    em.f.get(FRAME);
                    em.f.op(load_op(v, at));
                    em.f.set(x);
                    loaded.push(x);
                }
                let t = em.sub(em.b.local_ty[l]);
                em.because(Reason::ByDesign, |em| em.release_locals(&loaded, t))?;
            }
        }
        let row = em
            .susp
            .as_ref()
            .map(|s| s.provs.clone())
            .unwrap_or_default();
        if !row.is_empty() {
            let Some(reason) = em.stated_count(Tag::Release, CountAt::FrameDrop, CountPart::Row)
            else {
                // ruled out: counts::expected pairs cold_row::roots' row
                // copies with this distinct FrameDrop Row release.
                return unsupported("a suspension row without its stated teardown release");
            };
            em.because(reason, |em| {
                for d in row {
                    if let Some((at, v)) = at_of(d) {
                        em.f.get(FRAME);
                        em.f.op(load_op(v, at));
                        em.f.helper(Helper::ReleaseRef);
                    }
                }
            });
        }
        if let Some(reason) = em.stated_count(Tag::Release, CountAt::FrameDrop, CountPart::Child) {
            em.because(reason, |em| {
                em.f.get(FRAME);
                em.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
                em.f.op(I::If(BlockType::Empty));
                em.f.get(FRAME);
                em.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
                em.f.helper(Helper::ReleaseRef);
                em.f.op(I::End);
            });
        }
    }
    let drop = std::mem::take(&mut em.f);
    em.drop_states = body_drop_states;

    // The cold constructor: a frame in state 0 holding the arguments and
    // the providers, with a count of each counted argument.
    let cold_params: Vec<Val> = params[1..].to_vec();
    em.f = em.m.func(name.clone(), cold_params.clone(), vec![Val::I32]);
    em.inherit_origin_source(Reason::Conservative("cold constructor"));
    let fr = em.f.local(Val::I32);
    em.f.helper(Helper::Frame(key));
    em.f.set(fr);
    // Each argument's count, a collection's its copy, before the frame
    // keeps it.
    if em.counting {
        for &l in plocals {
            let ls = arrived(&em, l);
            let t = em.sub(em.b.local_ty[l]);
            if let Some(reason) = em.stated_count(
                Tag::Retain,
                CountAt::Cold,
                CountPart::Local(u32::try_from(l).expect("local")),
            ) && !crate::layout::holds_drop(em.pool(), em.env(), t)
            {
                em.because(reason, |em| em.retain_locals(&ls, t))?;
            }
        }
    }
    for j in 0..cold_params.len() {
        let l = u32::try_from(j + 1).expect("params");
        if let Some((at, v)) = at_of(l) {
            em.f.get(fr);
            em.f.get(l - 1);
            em.f.op(store_op(v, at));
        }
    }
    // A parameter a reference refers to or a closure shares is in its cell
    // from the start, so the release glue of a frame that never ran finds
    // it where it does later.
    for &l in plocals {
        if em.param_vals.contains_key(&l) {
            let ls = arrived(&em, l);
            let cell = em.new_cell(l, &super::value::Src::Locals(ls))?;
            if let Some((at, v)) = at_of(em.locals[&l][0]) {
                em.f.get(fr);
                em.f.get(cell);
                em.f.op(store_op(v, at));
            }
        }
    }
    // The frame's count of each reference parameter's object, in the
    // temporary its exit or release glue releases.
    if let Some(r) = em.input.runtime.filter(|_| em.counting) {
        for (l, held) in em.frame_refs.clone() {
            let (Some(x), Some((at, v))) = (arrived(&em, l).first().copied(), at_of(held)) else {
                continue;
            };
            let Some(reason) = em.stated_count(
                Tag::Retain,
                CountAt::Cold,
                CountPart::Referent(u32::try_from(l).expect("local")),
            ) else {
                // ruled out: own::referent states Cold for every parameter
                // whose thin referent the concrete frame layout saves here.
                return unsupported("a frame reference without its stated object retain");
            };
            em.f.get(x);
            em.because(reason, |em| {
                em.f.count_call(Sym::Inst(r.retain), crate::views::CountAction::Retain);
            });
            em.f.get(fr);
            em.f.get(x);
            em.f.op(store_op(v, at));
        }
    }
    em.save_initial_drop_flags(plocals, fr, &area);
    if em.counting {
        em.retain_row(CountAt::Cold, 1)?;
    }
    // A closure's code reads its captures from its environment, its first
    // parameter, into the frame, with a count of each.
    if let Some(ci) = closure {
        let copy = em.stated_count(Tag::Retain, CountAt::Cold, CountPart::Captures);
        if let Some(l) = super::closure::self_local(em.b, ci) {
            // The executing closure's value is not an environment field.
            // Save it directly in the fresh frame, with the same ownership
            // as a capture loaded from that environment.
            let ls = em.locals[&l].clone();
            for (j, x) in ls.iter().enumerate() {
                if let Some((at, v)) = at_of(*x) {
                    em.f.get(fr);
                    if j == 0 {
                        em.f.table_slot(Sym::Inst(key));
                    } else {
                        em.f.get(0);
                    }
                    em.f.op(store_op(v, at));
                }
            }
            if em.counting {
                let Some(reason) = copy else {
                    // ruled out: own::cold_captures states Captures whenever
                    // closure_self exists, even without other environment fields.
                    return unsupported("a cold closure without its stated self copy");
                };
                em.because(reason, |em| em.ref_retain(0));
            }
        }
        let caps = super::closure::captures(em.b, ci);
        if !caps.is_empty() {
            let (m, parts, _) = em.env_layout(ci, &caps)?;
            for ((l, _), p) in caps.iter().zip(&parts) {
                let Some(ls) = em.locals.get(l).cloned() else {
                    continue;
                };
                let mut loaded = Vec::new();
                for leaf in &m.leaves[p.first..p.first + p.count] {
                    let y = em.f.local(leaf.val);
                    em.f.get(0);
                    em.f.op(leaf.load(0));
                    em.f.set(y);
                    loaded.push(y);
                }
                if em.counting
                    && let Some(reason) = copy
                {
                    em.because(reason, |em| em.count_local(*l, &loaded, true))?;
                }
                for (x, y) in ls.iter().zip(&loaded) {
                    if let Some((at, v)) = at_of(*x) {
                        em.f.get(fr);
                        em.f.get(*y);
                        em.f.op(store_op(v, at));
                    }
                }
            }
            // The cells of the creator's own loans (E.2), each with a count
            // the frame holds, and the body's own loans as they stand.
            let own = em.own_caps(ci);
            for (l, p) in own.into_iter().zip(&parts[caps.len()..]) {
                let y = em.f.local(Val::I32);
                em.f.get(0);
                em.f.op(m.leaves[p.first].load(0));
                em.f.set(y);
                let Some(reason) = copy else {
                    // ruled out: own::cold_captures includes the counted/shared
                    // pointer captures whose creator loan cells own_caps copies.
                    return unsupported("a cold closure without its stated creator loan copy");
                };
                em.because(reason, |em| {
                    em.f.get(y);
                    em.f.helper(Helper::RetainKnown);
                });
                for (x, cell) in em.own_frame_locals(l) {
                    if let Some((at, v)) = at_of(x) {
                        em.f.get(fr);
                        em.f.get(y);
                        if !cell {
                            em.f.op(I::I32Load(crate::module::mem(crate::module::HEAP, 0, 2)));
                        }
                        em.f.op(store_op(v, at));
                    }
                }
            }
        }
    }
    em.f.get(fr);
    let cold = std::mem::take(&mut em.f);
    // The release glue may trace a started frame: what it loads, such as
    // a drop flag, stays saved.
    for ins in &drop.body {
        if let Ins::Op(I::I32Load(m) | I::I64Load(m) | I::F32Load(m) | I::F64Load(m)) = ins {
            for &(l, at, _) in &area {
                if u64::from(at) == m.offset {
                    free.remove(&l);
                }
            }
        }
    }
    let saved = held_at(&em, &area, &owners, &free);
    // The saves and the reload first, over the body's own local numbers.
    expand(&mut body, &area, &saved);
    crate::task::guard_frame(&mut body);
    if let Some((base, size)) = local_frame {
        // This is a physical poll/body invocation, not the persistent task
        // frame. Its base is deliberately absent from suspension save areas.
        crate::wide::locals_frame(&mut body, base, size);
    }

    // The poll: the runtime checks, then the body over the frame.
    let mut pres = vec![Val::I32];
    pres.extend(results.iter().copied());
    let mut f = em.m.func(format!("{name}.poll"), vec![Val::I32], pres);
    em.annotate_func(&mut f);
    let tmp: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    let nf = f.local(Val::I32);
    let pending = |f: &mut crate::module::Func| {
        f.i32(0);
        for v in &results {
            push_zero(f, *v);
        }
        f.op(I::Return);
    };
    f.get(FRAME);
    f.helper(Helper::PollCheck);
    f.op(I::If(BlockType::Empty));
    pending(&mut f);
    f.op(I::End);
    set_flags(&mut f, ACTIVE, true);
    f.get(FRAME);
    for v in &cold_params {
        push_zero(&mut f, *v);
    }
    f.call(Sym::Part(key, Part::Body));
    f.set(nf);
    for l in tmp.iter().rev() {
        f.set(*l);
    }
    set_flags(&mut f, ACTIVE, false);
    f.get(nf);
    f.op(I::I32Eqz);
    f.op(I::If(BlockType::Empty));
    set_flags(&mut f, DONE, true);
    f.i32(1);
    for l in &tmp {
        f.get(*l);
    }
    f.op(I::Return);
    f.op(I::End);
    pending(&mut f);
    let poll = f;

    // The cancel (§14.6): a frame that never started is only marked; a
    // started one unwinds until its body leaves by its cancellation.
    em.f = em.m.func(format!("{name}.cancel"), vec![Val::I32], vec![]);
    em.inherit_origin_source(Reason::Cancelled);
    let seen = em.f.local(Val::I32);
    let tmp: Vec<u32> = results.iter().map(|v| em.f.local(*v)).collect();
    let back = em.f.local(Val::I32);
    em.f.helper(Helper::CancelEnter);
    em.f.set(seen);
    em.f.op(I::Block(BlockType::Empty));
    em.f.get(FRAME);
    em.f.helper(Helper::CancelCheck);
    em.f.op(I::I32Eqz);
    em.f.op(I::BrIf(0));
    em.f.get(FRAME);
    em.f.op(I::I32Load(mem(HEAP, F_STATE, 2)));
    em.f.op(I::I32Eqz);
    em.f.op(I::BrIf(0));
    em.flags_or(FRAME, UNWINDING);
    em.f.get(FRAME);
    for v in &cold_params {
        push_zero(&mut em.f, *v);
    }
    em.f.call(Sym::Part(key, Part::Body));
    em.f.set(back);
    for l in tmp.iter().rev() {
        em.f.set(*l);
    }
    // A body that completed after all leaves nothing to unwind, and its
    // result is let go.
    em.f.get(back);
    em.f.op(I::I32Eqz);
    em.f.op(I::If(BlockType::Empty));
    em.unwound();
    if em.counting {
        em.release_locals(&tmp, ret)?;
    }
    em.f.op(I::End);
    em.f.op(I::End);
    em.f.get(FRAME);
    em.f.get(seen);
    em.f.helper(Helper::CancelLeave);
    let cancel = std::mem::take(&mut em.f);

    // A held call (a runtime scenario's) never runs its body: its poll
    // answers Pending, flagging the frame active for `reentrant-poll`,
    // and a direct caller's step builds the frame through the cold
    // constructor and hands it back as the pending child.
    let (body, poll) = match held {
        None => (body, poll),
        Some(active) => {
            let mut p = em.m.func(format!("{name}.poll"), vec![Val::I32], {
                let mut r = vec![Val::I32];
                r.extend(results.iter().copied());
                r
            });
            if active {
                set_flags(&mut p, ACTIVE, true);
            }
            p.i32(0);
            for v in &results {
                push_zero(&mut p, *v);
            }
            let mut bres = results.clone();
            bres.push(Val::I32);
            let mut b = em.m.func(name.clone(), params.to_vec(), bres);
            let fr = b.local(Val::I32);
            b.get(FRAME);
            b.op(I::I32Eqz);
            b.op(I::If(BlockType::Empty));
            for j in 1..params.len() {
                b.get(u32::try_from(j).expect("params"));
            }
            b.call(Sym::Inst(key));
            b.set(fr);
            b.op(I::Else);
            b.get(FRAME);
            b.set(fr);
            b.op(I::End);
            for v in &results {
                push_zero(&mut b, *v);
            }
            b.get(fr);
            (b, p)
        }
    };
    em.m.define(Sym::Part(key, Part::Body), body);
    em.m.define(Sym::Part(key, Part::Poll), poll);
    em.m.define(Sym::Part(key, Part::Cancel), cancel);
    em.m.define(Sym::Part(key, Part::Drop), drop);
    Ok(cold)
}

/// Sets (`on`) or clears flag bits `bits` of the frame in local 0.
fn set_flags(f: &mut crate::module::Func, bits: i32, on: bool) {
    f.get(FRAME);
    f.get(FRAME);
    f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
    if on {
        f.i32(bits);
        f.op(I::I32Or);
    } else {
        f.i32(!bits);
        f.op(I::I32And);
    }
    f.op(I::I32Store(mem(HEAP, F_FLAGS, 2)));
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn frame_expansion_keeps_each_save_and_load_origin() {
        let m = crate::module::Module::default();
        let mut f = m.func("frame", vec![Val::I32], vec![]);
        f.locals.push(Val::I32);
        f.body = vec![Ins::Save(1), Ins::LoadAll];
        f.origins = Some(vec![Some(10), Some(20)]);
        expand(&mut f, &[(1, 32, Val::I32)], &HashMap::default());
        assert_eq!(
            f.origins.unwrap(),
            [Some(10), Some(10), Some(10), Some(20), Some(20), Some(20)]
        );
        assert_eq!(f.body.len(), 6);
    }
}
