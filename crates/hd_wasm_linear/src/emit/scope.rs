//! Cleanup scopes (`crates/hd_wasm/DESIGN.md` §23.20, job 12): a `Scope`
//! with `defer` suites is the Wasm GC backend's exit ladder
//! (`crates/hd_mono/DESIGN.md` §12.2). Each suite has a "registered" flag
//! that its `Defer` sets; every way out of the body stores its exit's
//! code and branches to the scope's end, which runs the registered
//! suites, last in, first out, then takes the exit again, through the
//! next scope out. A suite that suspends (a `using` binding's `dispose!`)
//! waits for job 13.

use hd_base::StageResult;
use hd_tir::ir::NONE;
use wasm_encoder::{BlockType, Instruction as I};

use super::{Ctl, Em};
use crate::module::Val;
use crate::unsupported;

/// A way out of a cleanup scope.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Exit {
    /// To the end of a `Loop`, a `Match` or a labelled `Block`.
    Brk(u32),
    /// To the head of a `Loop`.
    Cont(u32),
    /// Out of the function, its result in `Em::ret`.
    Ret,
}

/// A cleanup scope's exit code local and the exits its body takes; exit
/// `k` has code `k + 1`, and 0 is the body's own end.
pub(super) struct ScopeInfo {
    code: u32,
    exits: Vec<Exit>,
}

impl Em<'_, '_, '_> {
    /// `Scope` with `defer` suites `rec`: the body in a block, then each
    /// registered suite, then the exits the body took.
    pub(super) fn scope(&mut self, i: u32, body: u32, rec: u32) -> StageResult<()> {
        let suites = self.rec(rec);
        let code = self.f.local(Val::I32);
        let mut flags = Vec::new();
        for &s in &suites {
            let f = self.f.local(Val::I32);
            self.f.i32(0);
            self.f.set(f);
            self.defer_flags.insert(s, f);
            flags.push(f);
        }
        self.scopes.insert(
            i,
            ScopeInfo {
                code,
                exits: Vec::new(),
            },
        );
        self.slot(i)?;
        self.open(Ctl::Scope(i));
        self.block_into(body, Some(i))?;
        self.f.i32(0);
        self.f.set(code);
        self.close();
        for (&s, &f) in suites.iter().zip(&flags).rev() {
            self.f.get(f);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            // A suite is a forbidden context (`req.drive.block-on.indirect`);
            // it cannot leave early, so the count balances.
            self.forbid_by(1);
            self.block_into(s, None)?;
            self.forbid_by(-1);
            self.close();
        }
        let exits = self.scopes.remove(&i).map(|s| s.exits).unwrap_or_default();
        for (k, e) in exits.into_iter().enumerate() {
            self.f.get(code);
            self.f.i32(i32::try_from(k + 1).expect("exits"));
            self.f.op(I::I32Eq);
            self.f.op(I::If(BlockType::Empty));
            self.ctl.push(Ctl::Plain);
            self.exit(e)?;
            self.close();
        }
        Ok(())
    }

    /// `Defer`: suite `s` is registered.
    pub(super) fn defer(&mut self, s: u32) -> StageResult<()> {
        let Some(&f) = self.defer_flags.get(&s) else {
            return unsupported("a `Defer` outside its scope");
        };
        self.f.i32(1);
        self.f.set(f);
        Ok(())
    }

    /// Leaves by `e`: to the innermost cleanup scope it crosses, which runs
    /// its suites and then takes `e` again; else directly.
    pub(super) fn exit(&mut self, e: Exit) -> StageResult<()> {
        let floor = match e {
            Exit::Brk(t) => self.ctl.iter().rposition(|c| *c == Ctl::Brk(t)),
            Exit::Cont(t) => self.ctl.iter().rposition(|c| *c == Ctl::Cont(t)),
            Exit::Ret => None,
        };
        let scope = self
            .ctl
            .iter()
            .rposition(|c| matches!(c, Ctl::Scope(_)))
            .filter(|p| floor.is_none_or(|f| *p > f));
        // The blocks this jump leaves before it reaches the target or a
        // scope's suites, which may still read what holds the scope.
        if e != Exit::Ret
            && let Some(f) = scope.or(floor)
        {
            self.drop_exited(f)?;
        }
        if let Some(p) = scope {
            let Ctl::Scope(s) = self.ctl[p] else {
                return unsupported("a cleanup scope marker");
            };
            let Some(info) = self.scopes.get_mut(&s) else {
                return unsupported("a cleanup scope without its ladder");
            };
            let k = if let Some(k) = info.exits.iter().position(|x| *x == e) {
                k + 1
            } else {
                info.exits.push(e);
                info.exits.len()
            };
            let code = info.code;
            self.f.i32(i32::try_from(k).expect("exits"));
            self.f.set(code);
            let d = u32::try_from(self.ctl.len() - 1 - p).expect("depth");
            self.f.op(I::Br(d));
            return Ok(());
        }
        match e {
            Exit::Brk(t) => {
                let d = self.depth(Ctl::Brk(t))?;
                self.f.op(I::Br(d));
            }
            Exit::Cont(t) => {
                let d = self.depth(Ctl::Cont(t))?;
                self.f.op(I::Br(d));
            }
            Exit::Ret if self.counting => {
                let d = self.depth(Ctl::Exit)?;
                self.f.op(I::Br(d));
            }
            Exit::Ret => {
                for l in self.ret.clone() {
                    self.f.get(l);
                }
                self.f.op(I::Return);
            }
        }
        Ok(())
    }

    /// `return a`: its value, with a count for the caller when the
    /// function counts, in the result locals, then out through the exit
    /// ladders of the scopes it leaves and the function's one exit.
    pub(super) fn ret_value(&mut self, a: u32) -> StageResult<()> {
        let crossing = self.ctl.iter().any(|c| matches!(c, Ctl::Scope(_)));
        if !self.counting && !crossing {
            if a != NONE {
                self.push(a)?;
            }
            self.f.op(I::Return);
            return Ok(());
        }
        if self.ret.len() != self.results.len() {
            self.ret = self.results.iter().map(|v| self.f.local(*v)).collect();
        }
        if a != NONE && !self.results.is_empty() {
            if self.counting {
                self.retain_ref(a)?;
            }
            self.push(a)?;
            for l in self.ret.clone().iter().rev() {
                self.f.set(*l);
            }
        }
        self.exit(Exit::Ret)
    }
}
