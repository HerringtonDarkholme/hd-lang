//! Suspending bodies (`crates/hd_wasm_linear/DESIGN.md` §23.21, job 13): the
//! state machine of §14.2 over a frame in linear memory. A frame is a
//! counted object: the task prefix ([`F_POLL`] to [`F_CHILD`]), then a
//! save area that holds every Wasm local of the body. A suspension point
//! saves them all and returns Pending with the frame; a resume reloads
//! them and branches to its point through the structural state numbering
//! of [`plan`]: a block list that holds states is entered by one
//! `br_table` on `pc`, and a branch that holds the resumed state is taken
//! whatever its condition was.

use hd_base::{Hash128, HashMap, StageResult};
use hd_mono::{Target, TargetKind};
use hd_tir::ir::{Body, NONE, Tag};
use wasm_encoder::{BlockType, Instruction as I};

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
        Tag::Loop => vec![a, bw],
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
    /// Each `all!` point's done mask and, per child, its instruction, the
    /// locals that keep its result, and the result's type.
    pub alls: HashMap<u32, AllPoint>,
}

/// An `all!` point's state in the save area (§14.5).
#[derive(Clone)]
pub(super) struct AllPoint {
    pub done: u32,
    pub node: u32,
    pub parts: Vec<(u32, Vec<u32>, hd_types::Ty)>,
}

/// The frame: the body's parameter 0.
pub(super) const FRAME: u32 = 0;

impl Em<'_, '_, '_> {
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

    /// Statements `items` (each with its position) of block `blk`, which
    /// holds a state: one `br_table` on `pc` enters the segment that holds
    /// the resumed state; fresh code falls through every segment.
    pub(super) fn resume_list(&mut self, blk: u32, items: &[(usize, u32)]) -> StageResult<()> {
        let points: Vec<usize> = (0..items.len())
            .filter(|&j| self.range(items[j].1).is_some())
            .collect();
        let run = |em: &mut Self, its: &[(usize, u32)]| -> StageResult<()> {
            for &(p, i) in its {
                em.inst(i)?;
                em.drop_dead(blk, p)?;
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
            .filter_map(|&j| self.range(items[j].1))
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
        self.f.op(I::BrTable(targets.into(), 0));
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
        for d in self
            .susp
            .as_ref()
            .map(|s| s.provs.clone())
            .unwrap_or_default()
        {
            self.ref_retain(d);
        }
        self.f.op(I::End);
        Ok(())
    }

    fn susp_key(&self) -> StageResult<Hash128> {
        match &self.susp {
            Some(s) => Ok(s.key),
            None => unsupported("a suspension point outside a suspending body"),
        }
    }

    /// Pending at state `k`, waiting on the child in local `child`, whose
    /// count the frame takes: saves every local and returns the frame.
    fn suspend(&mut self, k: u32, child: u32) -> StageResult<()> {
        self.materialize()?;
        self.f.push(Ins::SaveAll);
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

    /// The awaited child in local `child` (or none, for `all!`) was
    /// cancelled by another task, so this task is cancelled at the point
    /// (`req.cancel.await`): its frame is marked and the point's
    /// cancellation branch runs. A child the frame does not hold yet
    /// becomes its child, with the count in `child`.
    fn cancel_here(&mut self, child: Option<u32>) -> StageResult<()> {
        let Some(at) = self.susp.as_ref().and_then(|s| s.cancel_to) else {
            return unsupported("a cancelled await outside a point");
        };
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
        let Some(k) = self.susp.as_ref().and_then(|s| s.states.get(&i).copied()) else {
            return unsupported("a suspension point outside a suspending body");
        };
        let pc = self.pc();
        self.slot(i)?;
        // A point in a `using` binding's dispose suite is shielded: it has
        // no cancellation branch, and waits on (flow.using.cancel).
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
            // `all!` cancels its unfinished children in argument order
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
            self.release_done(&all)?;
        }
        // The cancelled child is done with: a ladder that waits keeps
        // its own child in the frame's child slot.
        self.release_child(c);
        // The ladders run as fresh code.
        self.f.i32(0);
        self.f.set(pc);
        self.exit(Exit::Cancel)?;
        self.close();
        Ok(())
    }

    /// The await of point `i` at state `k`.
    fn await_point(&mut self, i: u32, k: u32, a: u32, bw: u32) -> StageResult<()> {
        match self.b.tags[i as usize] {
            Tag::AwaitValue => self.await_stored(i, k, false, None, &|em| em.comp(a, 0)),
            Tag::Await => {
                let rec = self.rec(bw);
                let args = rec[..rec.len().saturating_sub(3)].to_vec();
                match self.calls.get(&i).cloned() {
                    Some(Target::Call(t)) if t.kind == TargetKind::Instance => {
                        if !self.env().suspends(t.item) {
                            return unsupported("a bang call of a function that does not suspend");
                        }
                        self.await_direct(i, k, t.key, t.item, t.args, &args)
                    }
                    Some(Target::Dyn) => {
                        // A generic method's erased body finishes with an
                        // open value (`erased.rs`).
                        let open = self.dyn_open_ret(a)?;
                        self.await_stored(i, k, true, open, &|em| em.push_dyn(a, &args, None))
                    }
                    _ => unsupported("a bang call that collection did not resolve"),
                }
            }
            Tag::AwaitAll => self.await_all(i, k, bw),
            _ => unsupported("emission of `AwaitRace` (the checker emits `race!` as std hd)"),
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
        for r in args {
            self.push(*r)?;
        }
        let c = self.push_call_providers(item, targs)?;
        self.f.call(Sym::Part(key, Part::Body));
        self.f.set(nc);
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.release_ctx(c);
        if shielded {
            self.shield_leave(nc, Some(ch));
        }
        self.f.get(nc);
        self.f.op(I::If(BlockType::Empty));
        self.ctl.push(Ctl::Plain);
        // A child that ended cancelled hands back its frame: this task is
        // cancelled too (`req.cancel.await`).
        if self.susp.as_ref().is_some_and(|s| s.cancel_to.is_some()) {
            self.is_cancelled(nc);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(Some(nc))?;
            self.close();
        }
        self.suspend(k, nc)?;
        self.close();
        // Ready: a resumed child's frame is done with.
        self.release_child(ch);
        self.f.i32(0);
        self.f.set(pc);
        for l in &tmp {
            self.f.get(*l);
        }
        self.store_owned(i)
    }

    /// One count less on the child frame in local `c`, which the frame
    /// held, and none held any more.
    fn release_child(&mut self, c: u32) {
        self.f.get(c);
        self.f.op(I::If(BlockType::Empty));
        self.f.get(FRAME);
        self.f.i32(0);
        self.f.op(I::I32Store(mem(HEAP, F_CHILD, 2)));
        if self.input.runtime.is_some() {
            self.f.get(c);
            self.f.helper(Helper::ReleaseRef);
        }
        self.f.op(I::End);
    }

    /// A stored suspension (`s!()`, a trait-value bang call): polled
    /// through its poll slot. `fresh` pushes it on the fresh path: a new
    /// cold frame when `owned`, whose count the frame takes when it waits
    /// and which is let go once it is Ready; else a borrowed suspension,
    /// of which the frame takes a count of its own when it waits. With
    /// `open`, the result is an open value of that erased type.
    fn await_stored(
        &mut self,
        i: u32,
        k: u32,
        owned: bool,
        open: Option<hd_types::Ty>,
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
        let res = match open {
            Some(_) => vec![Val::I32],
            None => self.vals(self.ty(i))?,
        };
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
            self.shield_leave(gone, None);
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
            self.ref_retain(s);
            self.f.op(I::End);
        }
        if self.susp.as_ref().is_some_and(|x| x.cancel_to.is_some()) {
            self.is_cancelled(s);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(Some(s))?;
            self.close();
        }
        self.suspend(k, s)?;
        self.close();
        self.f.get(held);
        self.f.op(I::If(BlockType::Empty));
        self.release_child(s);
        if owned && self.input.runtime.is_some() {
            self.f.op(I::Else);
            self.f.get(s);
            self.f.helper(Helper::ReleaseRef);
        }
        self.f.op(I::End);
        self.f.i32(0);
        self.f.set(pc);
        let tmp = match open {
            Some(et) => self.unerase(tmp[0], et, self.ty(i), true)?,
            None => tmp,
        };
        for l in &tmp {
            self.f.get(*l);
        }
        self.store_owned(i)
    }

    /// `all!(a, b, ...)` (§14.5): the first poll polls every child;
    /// later polls visit only unfinished children whose waker bit is set.
    /// The counted node lives in the save area, and registrations keep
    /// it and its enclosing nodes alive until completion or cancellation.
    fn await_all(&mut self, i: u32, k: u32, bw: u32) -> StageResult<()> {
        let children = self.rec(bw);
        if children.len() > 31 {
            return unsupported("`all!` of more than 31 suspensions");
        }
        let pc = self.pc();
        let tys = crate::layout::tuple_members(
            self.pool(),
            crate::layout::strip(self.pool(), self.env(), self.ty(i)),
        );
        if tys.len() != children.len() {
            return unsupported("an `all!` result that is not a tuple of its children's");
        }
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
        self.temps.push(node);
        let all = (1i32 << children.len()) - 1;
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
        if self.susp.as_ref().is_some_and(|x| x.cancel_to.is_some()) {
            self.f.get(gone);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.cancel_here(None)?;
            self.close();
        }
        let no_child = self.f.local(Val::I32);
        self.suspend(k, no_child)?;
        self.close();
        self.f.get(node);
        self.f.helper(Helper::ReleaseRef);
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

    /// Lets go of the results that `all!` point `all` kept for the
    /// children its mask marks done, and clears the mask.
    fn release_done(&mut self, all: &AllPoint) -> StageResult<()> {
        for (j, (_, ls, t)) in all.parts.iter().enumerate() {
            if self.counted(*t).is_none() {
                continue;
            }
            self.f.get(all.done);
            self.f.i32(1 << j);
            self.f.op(I::I32And);
            self.f.op(I::If(BlockType::Empty));
            self.release_locals(ls, *t)?;
            self.f.op(I::End);
        }
        self.f.i32(0);
        self.f.set(all.done);
        self.f.get(all.node);
        self.f.helper(Helper::ReleaseRef);
        self.f.i32(0);
        self.f.set(all.node);
        Ok(())
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
    fn shield_leave(&mut self, out: u32, fresh: Option<u32>) {
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
            self.f.get(out);
            self.f.helper(Helper::ReleaseRef);
            self.f.op(I::End);
        }
        self.f.zero(out);
        self.f.op(I::End);
        self.f.op(I::End);
        self.close();
    }

    /// One more count on the address in local `x`.
    fn ref_retain(&mut self, x: u32) {
        if let Some(r) = self.input.runtime {
            self.f.get(x);
            self.f.call(Sym::Inst(r.retain));
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
pub(super) fn store_op(v: Val, at: u32) -> I<'static> {
    let at = u64::from(at);
    match v {
        Val::I32 => I::I32Store(mem(HEAP, at, 2)),
        Val::I64 => I::I64Store(mem(HEAP, at, 3)),
        Val::F32 => I::F32Store(mem(HEAP, at, 2)),
        Val::F64 => I::F64Store(mem(HEAP, at, 3)),
    }
}

/// The load instruction of a `v` at `at` in a heap object.
pub(super) fn load_op(v: Val, at: u32) -> I<'static> {
    let at = u64::from(at);
    match v {
        Val::I32 => I::I32Load(mem(HEAP, at, 2)),
        Val::I64 => I::I64Load(mem(HEAP, at, 3)),
        Val::F32 => I::F32Load(mem(HEAP, at, 2)),
        Val::F64 => I::F64Load(mem(HEAP, at, 3)),
    }
}

/// Replaces a body's [`Ins::SaveAll`] and [`Ins::LoadAll`] by the stores
/// and loads of its save area `area`.
pub(super) fn expand(f: &mut crate::module::Func, area: &[(u32, u32, Val)]) {
    let body = std::mem::take(&mut f.body);
    let origins = f.origins.as_mut().map(std::mem::take);
    let outer = f.origin_at;
    for (position, ins) in body.into_iter().enumerate() {
        f.origin_at = origins.as_ref().and_then(|o| o[position]);
        match ins {
            Ins::SaveAll => {
                for &(l, at, v) in area {
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
            return unsupported("a cancellation outside a suspending body");
        };
        if self.counting {
            // An owned body's exit releases only its exit set.
            if self.owned {
                self.release_unwound()?;
            }
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
    for &l in plocals {
        if em.is_cell(l) {
            return unsupported("a mutably captured parameter of a suspending function");
        }
    }
    let mut vals_all: Vec<Val> = em.f.params.clone();
    vals_all.extend(em.f.locals.iter().copied());
    let (area, size) = save_area(&vals_all, &[FRAME, pc]);
    expand(&mut em.f, &area);
    em.m.frames.insert(key, size);
    let at_of = |l: u32| area.iter().find(|x| x.0 == l).map(|x| (x.1, x.2));
    let results = em.results.clone();
    let name = em.f.name.clone();
    crate::task::guard_frame(&mut em.f);
    let body = std::mem::take(&mut em.f);

    // The release glue: the places the body owns while it waits, unless
    // it has left, then the child.
    em.f = crate::module::Func::new(format!("{name}.drop"), vec![Val::I32], vec![]);
    em.inherit_origin_source();
    em.counting = em.input.runtime.is_some();
    if em.counting {
        em.f.get(FRAME);
        em.f.op(I::I32Load(mem(HEAP, F_FLAGS, 2)));
        em.f.i32(EXITED);
        em.f.op(I::I32And);
        em.f.op(I::I32Eqz);
        em.f.op(I::If(BlockType::Empty));
        let mut places: Vec<(Option<u32>, Option<usize>, Vec<u32>)> = Vec::new();
        let mut vals: Vec<(u32, Vec<u32>)> = em.vals.iter().map(|(k, v)| (*k, v.clone())).collect();
        vals.sort_unstable();
        for (i, ls) in vals {
            if !em.moves.loans.contains(&i) {
                places.push((Some(i), None, ls));
            }
        }
        let mut locals: Vec<(usize, Vec<u32>)> =
            em.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        locals.sort_unstable();
        for (l, ls) in locals {
            if !em.moves.loan_locals.contains_key(&l) {
                places.push((None, Some(l), ls));
            }
        }
        for t in em.temps.clone() {
            places.push((None, None, vec![t]));
        }
        for (inst, local, ls) in places {
            let mut loaded = Vec::new();
            let mut ok = true;
            for l in &ls {
                let Some((at, v)) = at_of(*l) else {
                    ok = false;
                    break;
                };
                let x = em.f.local(v);
                em.f.get(FRAME);
                em.f.op(load_op(v, at));
                em.f.set(x);
                loaded.push(x);
            }
            if !ok {
                continue;
            }
            match (inst, local) {
                (Some(i), _) => em.release_locals(&loaded, em.ty(i))?,
                (None, Some(l)) => em.count_local(l, &loaded, false)?,
                (None, None) => {
                    em.f.get(loaded[0]);
                    em.f.helper(Helper::ReleaseRef);
                }
            }
        }
        // The results an `all!` kept for its done children.
        let mut alls: Vec<(u32, AllPoint)> = em
            .susp
            .as_ref()
            .map(|s| s.alls.iter().map(|(k, v)| (*k, v.clone())).collect())
            .unwrap_or_default();
        alls.sort_unstable_by_key(|x| x.0);
        for (_, all) in alls {
            let Some((dat, dv)) = at_of(all.done) else {
                continue;
            };
            let done = em.f.local(dv);
            em.f.get(FRAME);
            em.f.op(load_op(dv, dat));
            em.f.set(done);
            for (j, (_, ls, t)) in all.parts.iter().enumerate() {
                if em.counted(*t).is_none() {
                    continue;
                }
                let Some(slots) = ls.iter().map(|l| at_of(*l)).collect::<Option<Vec<_>>>() else {
                    continue;
                };
                em.f.get(done);
                em.f.i32(1 << j);
                em.f.op(I::I32And);
                em.f.op(I::If(BlockType::Empty));
                let mut loaded = Vec::new();
                for (at, v) in slots {
                    let x = em.f.local(v);
                    em.f.get(FRAME);
                    em.f.op(load_op(v, at));
                    em.f.set(x);
                    loaded.push(x);
                }
                em.release_locals(&loaded, *t)?;
                em.f.op(I::End);
            }
        }
        em.f.op(I::End);
        for d in em
            .susp
            .as_ref()
            .map(|s| s.provs.clone())
            .unwrap_or_default()
        {
            if let Some((at, v)) = at_of(d) {
                em.f.get(FRAME);
                em.f.op(load_op(v, at));
                em.f.helper(Helper::ReleaseRef);
            }
        }
        em.f.get(FRAME);
        em.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
        em.f.op(I::If(BlockType::Empty));
        em.f.get(FRAME);
        em.f.op(I::I32Load(mem(HEAP, F_CHILD, 2)));
        em.f.helper(Helper::ReleaseRef);
        em.f.op(I::End);
    }
    let drop = std::mem::take(&mut em.f);

    // The cold constructor: a frame in state 0 holding the arguments and
    // the providers, with a count of each counted argument.
    let cold_params: Vec<Val> = params[1..].to_vec();
    em.f = crate::module::Func::new(name.clone(), cold_params.clone(), vec![Val::I32]);
    em.inherit_origin_source();
    let fr = em.f.local(Val::I32);
    em.f.helper(Helper::Frame(key));
    em.f.set(fr);
    for j in 0..cold_params.len() {
        let l = u32::try_from(j + 1).expect("params");
        if let Some((at, v)) = at_of(l) {
            em.f.get(fr);
            em.f.get(l - 1);
            em.f.op(store_op(v, at));
        }
    }
    if em.counting {
        for &l in plocals {
            let ls: Vec<u32> = em.locals[&l].iter().map(|x| x - 1).collect();
            let t = em.sub(em.b.local_ty[l]);
            em.retain_locals(&ls, t)?;
        }
        for d in em
            .susp
            .as_ref()
            .map(|s| s.provs.clone())
            .unwrap_or_default()
        {
            em.ref_retain(d - 1);
        }
    }
    // A closure's code reads its captures from its environment, its first
    // parameter, into the frame, with a count of each.
    if let Some(ci) = closure {
        if let Some(l) = super::closure::self_local(em.b, ci) {
            // The executing closure's value is not an environment field.
            // Save it directly in the fresh frame, with the same ownership
            // as a capture loaded from that environment.
            let ls = em.locals[&l].clone();
            for (j, x) in ls.iter().enumerate() {
                if let Some((at, v)) = at_of(*x) {
                    em.f.get(fr);
                    if j == 0 {
                        em.f.push(Ins::TableSlot(Sym::Inst(key)));
                    } else {
                        em.f.get(0);
                    }
                    em.f.op(store_op(v, at));
                }
            }
            if em.counting {
                em.ref_retain(0);
            }
        }
        let caps = super::closure::captures(em.b, ci);
        if !caps.is_empty() {
            let (m, parts, _) = em.env_layout(&caps)?;
            for ((l, _), p) in caps.iter().zip(&parts) {
                let Some(ls) = em.locals.get(l).cloned() else {
                    continue;
                };
                let mut loaded = Vec::new();
                for (leaf, x) in m.leaves[p.first..p.first + p.count].iter().zip(&ls) {
                    let y = em.f.local(leaf.val);
                    em.f.get(0);
                    em.f.op(leaf.load(0));
                    em.f.set(y);
                    if let Some((at, v)) = at_of(*x) {
                        em.f.get(fr);
                        em.f.get(y);
                        em.f.op(store_op(v, at));
                    }
                    loaded.push(y);
                }
                if em.counting {
                    em.count_local(*l, &loaded, true)?;
                }
            }
        }
    }
    em.f.get(fr);
    let cold = std::mem::take(&mut em.f);

    // The poll: the runtime checks, then the body over the frame.
    let mut pres = vec![Val::I32];
    pres.extend(results.iter().copied());
    let mut f = crate::module::Func::new(format!("{name}.poll"), vec![Val::I32], pres);
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
    em.f = crate::module::Func::new(format!("{name}.cancel"), vec![Val::I32], vec![]);
    em.inherit_origin_source();
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
            let mut p = crate::module::Func::new(format!("{name}.poll"), vec![Val::I32], {
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
            let mut b = crate::module::Func::new(name.clone(), params.to_vec(), bres);
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
        let mut f = crate::module::Func::new("frame", vec![Val::I32], vec![]);
        f.locals.push(Val::I32);
        f.body = vec![Ins::SaveAll, Ins::LoadAll];
        f.origins = Some(vec![Some(10), Some(20)]);
        expand(&mut f, &[(1, 32, Val::I32)]);
        assert_eq!(
            f.origins.unwrap(),
            [Some(10), Some(10), Some(10), Some(20), Some(20), Some(20)]
        );
        assert_eq!(f.body.len(), 6);
    }
}
