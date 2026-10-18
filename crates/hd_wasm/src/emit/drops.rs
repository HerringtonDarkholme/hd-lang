//! Drop emission (spec/lang/13-ownership.md, Drops; ownership prototype
//! slice 10). Collection gives every instruction that drops a value with
//! glue a call of `drop[T]` (`hd_mono::drops`): a `DropLocal`, a
//! `DropTemp`, and a `LocalSet` or `FieldSet` that overwrites an owned
//! value. Here:
//!
//! - An owned local whose drop mode is not `Never` gets a **drop flag**, an
//!   `i32` local zeroed where its scope starts, set where the local takes a
//!   value (its `DropLocal`, an assignment) and cleared where it moves. Its
//!   scope's exit ladder drops it when the flag is set, in one last-in,
//!   first-out order with the scope's `defer` suites.
//! - A `defer` suite takes the owned locals it reads when it registers
//!   (`own.move.defer`): each is copied into a snapshot with a flag of its
//!   own, the suite reads the snapshot, and what the suite did not move
//!   drops right after it.
//! - An assignment drops the old value first when the flag is set; a store
//!   into an owned field drops the field's old value first.

use hd_base::StageResult;
use hd_mono::{CallTarget, Target, subst};
use hd_tir::ir::{DROP_ENTRY, DropMode, Tag};

use super::{Em, u32_of};
use crate::{Sym, VT, unsupported};

/// An owned local a `defer` suite took when it registered: its snapshot's
/// locals and their types, the snapshot's flag, and its glue call.
#[derive(Clone)]
pub(super) struct Taken {
    local: u32,
    snap: Vec<u32>,
    vts: Vec<VT>,
    flag: u32,
    glue: CallTarget,
}

/// One suite of a cleanup scope, in its exit ladder.
#[derive(Clone)]
pub(super) enum Entry {
    /// A `defer` suite, by its block.
    Suite(u32),
    /// An owned local's drop, with the call that drops it.
    Drop(u32, CallTarget),
}

impl Em<'_> {
    /// The suites of the `Scope` whose record is at `rec`: its `defer`
    /// suites and the drops of its owned locals that may hold a value
    /// with glue at a scope exit, in instruction order.
    pub(super) fn scope_entries(&self, rec: u32) -> Vec<Entry> {
        self.rec(rec)
            .into_iter()
            .filter_map(|w| {
                if w & DROP_ENTRY == 0 {
                    return Some(Entry::Suite(w));
                }
                let d = w & !DROP_ENTRY;
                if self.drop_mode(d) == DropMode::Never {
                    return None;
                }
                let Some(Target::Call(t)) = self.calls.get(&d) else {
                    return None;
                };
                Some(Entry::Drop(self.b.data[d as usize][0], t.clone()))
            })
            .collect()
    }

    /// The drop mode of `DropLocal` instruction `d`: the body's
    /// `drop_mode` column holds one per `DropLocal`, in instruction order.
    fn drop_mode(&self, d: u32) -> DropMode {
        let k = self.b.tags[..d as usize]
            .iter()
            .filter(|t| **t == Tag::DropLocal)
            .count();
        self.b.drop_mode.get(k).copied().unwrap_or(DropMode::Always)
    }

    /// The drop flag of owned local `l`: a `defer` suite's snapshot's while
    /// that suite is emitted, else the local's own.
    pub(super) fn drop_flag(&self, l: u32) -> Option<u32> {
        match self.taken.get(&l) {
            Some((_, f)) => Some(*f),
            None => self.drop_flags.get(&l).copied(),
        }
    }

    /// Where local `l` takes a value: its flag is set.
    pub(super) fn set_drop_flag(&mut self, l: u32, on: bool) {
        if let Some(f) = self.drop_flag(l) {
            self.a.i32(i32::from(on));
            self.a.set(f);
        }
    }

    /// After `LocalGet` `i` of `l`: a move clears the flag.
    pub(super) fn note_move(&mut self, i: u32, l: u32) {
        if self.b.moves.binary_search(&i).is_ok() {
            self.set_drop_flag(l, false);
        }
    }

    /// The Wasm value types the glue call `t` takes its value in.
    fn glue_vts(&self, t: &CallTarget) -> StageResult<Vec<VT>> {
        let pool = self.pool();
        let Some(&p) = self.env().params(t.item).unwrap_or_default().first() else {
            return unsupported("a `drop` without its parameter");
        };
        self.vts(subst(pool, self.env(), t.item, t.args, p))
    }

    /// Calls the glue `t` with the value on the stack.
    fn call_glue(&mut self, t: &CallTarget) -> StageResult<()> {
        self.push_call_providers(t.item, t.args)?;
        self.a.call(Sym::Inst(t.key));
        Ok(())
    }

    /// Drops the value local `l` holds now, through `t`.
    fn drop_local_value(&mut self, l: u32, t: &CallTarget) -> StageResult<()> {
        if self.cells.contains_key(&l) {
            return unsupported("a drop of an owned local a closure shares");
        }
        let ls = self.local(l)?;
        let have = self.local_vts(l)?;
        let want = self.glue_vts(t)?;
        for ((l, h), w) in ls.iter().zip(&have).zip(&want) {
            self.a.get(*l);
            self.a.conv(h, w);
        }
        self.call_glue(t)
    }

    /// Before an assignment `i` to `l`: the old value drops when the flag
    /// says the local holds one.
    pub(super) fn drop_overwritten(&mut self, i: u32, l: u32) -> StageResult<()> {
        let (Some(f), Some(Target::Call(t))) = (self.drop_flag(l), self.calls.get(&i).cloned())
        else {
            return Ok(());
        };
        self.a.get(f);
        self.a.if_();
        self.open(super::Ctl::Plain);
        self.drop_local_value(l, &t)?;
        self.ctrl.pop();
        self.a.end();
        Ok(())
    }

    /// Before `FieldSet` `i` into the struct `st` at component `start`
    /// with components `vs`, base `base`: the field's old value drops.
    pub(super) fn drop_field(
        &mut self,
        i: u32,
        base: u32,
        st: &crate::WTy,
        start: u32,
        vs: &[VT],
    ) -> StageResult<()> {
        let Some(Target::Call(t)) = self.calls.get(&i).cloned() else {
            return Ok(());
        };
        let want = self.glue_vts(&t)?;
        for (k, (h, w)) in vs.iter().zip(&want).enumerate() {
            self.comp(base, 0, &VT::r(st.clone()))?;
            self.a.struct_get(st, start + u32_of(k));
            self.a.conv(h, w);
        }
        self.call_glue(&t)
    }

    /// `DropTemp` `i` of value `v`: the temporary drops now.
    pub(super) fn drop_temp(&mut self, i: u32, v: u32) -> StageResult<()> {
        let Some(Target::Call(t)) = self.calls.get(&i).cloned() else {
            return Ok(());
        };
        let want = self.glue_vts(&t)?;
        self.load_as(v, &want)?;
        self.call_glue(&t)
    }

    /// Allocates the flags of a scope's entries: each `defer` suite's
    /// "registered" flag, and each owned local's drop flag, with the
    /// snapshots of the locals each suite takes.
    pub(super) fn entry_flags(&mut self, entries: &[Entry]) -> Vec<u32> {
        let mut flags = Vec::new();
        for e in entries {
            let f = self.a.local(VT::I32);
            flags.push(f);
            match e {
                Entry::Suite(s) => {
                    self.defer_flags.insert(*s, f);
                }
                Entry::Drop(l, _) => {
                    self.drop_flags.insert(*l, f);
                }
            }
        }
        flags
    }

    /// The owned locals with glue that `defer` suite `s` reads, at any
    /// depth (`own.move.defer`), each once, newest first.
    fn suite_takes(&self, s: u32) -> Vec<(u32, CallTarget)> {
        let mut locals = Vec::new();
        self.suite_reads(s, &mut locals);
        locals.sort_unstable();
        locals.dedup();
        locals.reverse();
        locals
            .into_iter()
            .filter_map(|l| self.local_glue(l).map(|t| (l, t)))
            .collect()
    }

    fn suite_reads(&self, blk: u32, out: &mut Vec<u32>) {
        if self.b.tags.get(blk as usize) != Some(&Tag::Block) {
            return;
        }
        for &i in self.b.record(self.b.data[blk as usize][0]) {
            match self.b.tags[i as usize] {
                // A closure's root is a body of its own.
                Tag::Block if !self.b.sub_root.contains(&i) => self.suite_reads(i, out),
                Tag::LocalGet => out.push(self.b.data[i as usize][0]),
                _ => {}
            }
        }
    }

    /// The glue call of owned local `l`, from its `DropLocal`.
    fn local_glue(&self, l: u32) -> Option<CallTarget> {
        (0..self.b.len()).find_map(|d| {
            if self.b.tags[d] != Tag::DropLocal || self.b.data[d][0] != l {
                return None;
            }
            match self.calls.get(&u32_of(d)) {
                Some(Target::Call(t)) => Some(t.clone()),
                _ => None,
            }
        })
    }

    /// `Defer` of suite `s` registers: the owned locals it reads move into
    /// snapshots, each flagged as holding its value.
    pub(super) fn take_into_suite(&mut self, s: u32) -> StageResult<()> {
        let takes = self.suite_takes(s);
        if takes.is_empty() {
            return Ok(());
        }
        let mut held = Vec::new();
        for (l, t) in takes {
            if self.cells.contains_key(&l) {
                return unsupported("a `defer` that takes an owned local a closure shares");
            }
            let ls = self.local(l)?;
            let vts = self.local_vts(l)?;
            let snap: Vec<u32> = vts.iter().map(|v| self.a.local(v.clone())).collect();
            for (from, to) in ls.iter().zip(&snap) {
                self.a.get(*from);
                self.a.set(*to);
            }
            let f = self.a.local(VT::I32);
            self.a.i32(1);
            self.a.set(f);
            self.set_drop_flag(l, false);
            held.push(Taken {
                local: l,
                snap,
                vts,
                flag: f,
                glue: t,
            });
        }
        self.defer_takes.insert(s, held);
        Ok(())
    }

    /// Emits suite `s` over the snapshots it took.
    fn suite_over_takes(&mut self, s: u32) -> StageResult<()> {
        let held = self.defer_takes.get(&s).cloned().unwrap_or_default();
        let outer = std::mem::take(&mut self.taken);
        for h in &held {
            self.taken.insert(h.local, (h.snap.clone(), h.flag));
        }
        let r = self.block_into(s, None);
        self.taken = outer;
        r
    }

    /// After suite `s`: what it took and did not move drops, newest first.
    fn drop_takes(&mut self, s: u32) -> StageResult<()> {
        let held = self.defer_takes.get(&s).cloned().unwrap_or_default();
        for h in held {
            let want = self.glue_vts(&h.glue)?;
            self.a.get(h.flag);
            self.a.if_();
            self.open(super::Ctl::Plain);
            for ((l, have), w) in h.snap.iter().zip(&h.vts).zip(&want) {
                self.a.get(*l);
                self.a.conv(have, w);
            }
            self.call_glue(&h.glue)?;
            self.ctrl.pop();
            self.a.end();
        }
        Ok(())
    }

    /// The exit ladder's rung for entry `e`, whose flag is set.
    pub(super) fn run_entry(&mut self, e: &Entry) -> StageResult<()> {
        match e {
            // A suite that waits is a `using` binding's `dispose!`, which runs
            // under the driver (flow.using.not-forbidden), so it is no forbidden
            // context; a count held across its wait would leak to others.
            Entry::Suite(s) if self.range(*s).is_some() => {
                self.shield += 1;
                let r = self.suite_over_takes(*s);
                self.shield -= 1;
                r?;
                self.drop_takes(*s)?;
            }
            Entry::Suite(s) => {
                // A `defer` suite is a forbidden context
                // (`req.drive.block-on.indirect`): `block_on` reached from it
                // panics. The suite cannot leave early, so the count balances.
                self.forbid(1);
                self.suite_over_takes(*s)?;
                self.forbid(-1);
                self.drop_takes(*s)?;
            }
            Entry::Drop(l, t) => self.drop_local_value(*l, t)?,
        }
        Ok(())
    }
}
