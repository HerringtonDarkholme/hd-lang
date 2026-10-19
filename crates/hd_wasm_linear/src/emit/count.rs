//! Naive counting (`crates/hd_wasm/DESIGN.md` §23.16, job 8): every place
//! that holds a counted reference owns one count. An instruction's value
//! owns one from its store, a TIR local from its set, a field from its
//! object's construction or its write; a call's result arrives with its
//! own. A parameter is borrowed from the caller and retained on entry, so
//! a function releases every place it owns at its one exit. A store in a
//! loop releases the value the place held from the last iteration.
//! Elision is later work (`REFCOUNT_RUNTIME.md` §10.2, jobs 17 and 18).

use hd_base::StageResult;
use hd_tir::ir::{NONE, Ref};
use hd_types::Ty;

use super::Em;
use crate::layout::{Walk, walk};
use crate::rt::Helper;

impl Em<'_, '_, '_> {
    /// The counted references of a `t` value, when the program counts.
    pub(super) fn counted(&self, t: Ty) -> Option<Walk> {
        self.input.runtime?;
        walk(self.pool(), self.env(), t)
    }

    /// Calls `on` for the value in locals `ls` of type `t`: one count more
    /// (`retain`) or less on each of its counted references.
    pub(super) fn count_locals(&mut self, ls: &[u32], t: Ty, retain: bool) -> StageResult<()> {
        let Some(w) = self.counted(t) else {
            return Ok(());
        };
        if let Walk::Ref(k) = w {
            self.f.get(ls[k as usize]);
            self.ref_count(retain);
            return Ok(());
        }
        for l in ls {
            self.f.get(*l);
        }
        let vals = self.vals(t)?;
        self.f.helper(if retain {
            Helper::Retain { vals, walk: w }
        } else {
            Helper::Release { vals, walk: w }
        });
        Ok(())
    }

    /// One count more or less on the address on the stack.
    fn ref_count(&mut self, retain: bool) {
        match (retain, self.input.runtime) {
            (true, Some(r)) => self.f.call(crate::module::Sym::Inst(r.retain)),
            (false, Some(_)) => self.f.helper(Helper::ReleaseRef),
            (_, None) => self.f.op(wasm_encoder::Instruction::Drop),
        }
    }

    /// One count more or less on TIR local `l`'s value in `ls`: its
    /// cell's, when it lives in one.
    pub(super) fn count_local(&mut self, l: usize, ls: &[u32], retain: bool) -> StageResult<()> {
        if self.is_cell(l) {
            self.f.get(ls[0]);
            self.ref_count(retain);
            return Ok(());
        }
        let t = self.sub(self.b.local_ty[l]);
        self.count_locals(ls, t, retain)
    }

    pub(super) fn retain_locals(&mut self, ls: &[u32], t: Ty) -> StageResult<()> {
        self.count_locals(ls, t, true)
    }

    pub(super) fn release_locals(&mut self, ls: &[u32], t: Ty) -> StageResult<()> {
        self.count_locals(ls, t, false)
    }

    /// One more count on value `r`'s references, for a place that keeps a
    /// copy of it. A constant is static data, which has no count.
    pub(super) fn retain_ref(&mut self, r: u32) -> StageResult<()> {
        if r == NONE || Ref(r).as_inst().is_none() || self.counted(self.ty_of(r)).is_none() {
            return Ok(());
        }
        let Some(ls) = self.vals.get(&r).cloned() else {
            return crate::unsupported("a value its instruction did not compute");
        };
        self.retain_locals(&ls, self.ty_of(r))
    }

    /// Stores the values on the stack as instruction `i`'s. Unless `owned`
    /// (a call's result, a new object), the value is a copy and its place
    /// takes a count of its own. In a loop, the value the place held from
    /// the last iteration is released after the new one is counted.
    pub(super) fn store_as(&mut self, i: u32, owned: bool) -> StageResult<()> {
        let ls = self.slot(i)?;
        let t = self.ty(i);
        if self.counted(t).is_none() {
            for l in ls.iter().rev() {
                self.f.set(*l);
            }
            return Ok(());
        }
        let old: Vec<u32> = if self.loops > 0 {
            let vs = self.vals(t)?;
            let old: Vec<u32> = vs.iter().map(|v| self.f.local(*v)).collect();
            for (l, o) in ls.iter().zip(&old) {
                self.f.get(*l);
                self.f.set(*o);
            }
            old
        } else {
            Vec::new()
        };
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        if !owned {
            self.retain_locals(&ls, t)?;
        }
        if !old.is_empty() {
            self.release_locals(&old, t)?;
        }
        Ok(())
    }

    /// Stores a value that arrives with its own count as instruction `i`'s.
    pub(super) fn store_owned(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, true)
    }

    /// Whether the function holds a counted reference anywhere: a
    /// parameter, a local or an instruction's value. Only then does it
    /// count, and its returns go through its exit.
    pub(super) fn counts_anything(&self) -> bool {
        if self.input.runtime.is_none() {
            return false;
        }
        let b = self.b;
        !self.cells.is_empty()
            || (0..b.local_ty.len()).any(|l| self.counted(self.sub(b.local_ty[l])).is_some())
            || (0..b.ty.len()).any(|i| {
                self.counted(self.ty(u32::try_from(i).expect("insts")))
                    .is_some()
            })
    }

    /// Releases every place the function owns: each counted instruction
    /// value and TIR local, a parameter's too.
    pub(super) fn release_all(&mut self) -> StageResult<()> {
        let mut vals: Vec<(u32, Vec<u32>)> =
            self.vals.iter().map(|(k, v)| (*k, v.clone())).collect();
        vals.sort_unstable();
        for (i, ls) in vals {
            self.release_locals(&ls, self.ty(i))?;
        }
        let mut locals: Vec<(usize, Vec<u32>)> =
            self.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        locals.sort_unstable();
        for (l, ls) in locals {
            self.count_local(l, &ls, false)?;
        }
        for tmp in self.temps.clone() {
            self.f.get(tmp);
            self.f.helper(Helper::ReleaseRef);
        }
        Ok(())
    }
}
