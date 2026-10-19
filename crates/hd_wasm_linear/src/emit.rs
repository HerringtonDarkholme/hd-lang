//! One instance's TIR body to one Wasm function
//! (`crates/hd_wasm/DESIGN.md` §23).
//!
//! Every value lives in Wasm locals: an instruction's value is stored in
//! fresh locals of its layout as it is computed, a constant is pushed as an
//! immediate, and a TIR local owns the locals of its layout. Structured
//! control is Wasm's own: a `Match` is a block per arm around its decision
//! tree, whose `ToArm` branches out to its arm, and a `Loop` is a block
//! around a loop. Integer arithmetic follows the Wasm GC backend's
//! lowering (§13.15 of `crates/hd_mono/DESIGN.md`), so both backends panic
//! alike.

use hd_base::{DefId, HashMap, InstId, StageResult};
use hd_mono::{ProgramEnv, Sub, Target, TargetKind, subst};
use hd_tir::ir::{Body, Callee, Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, local_flags};
use hd_types::{InternPool, Ty, TyData, TyList};
use wasm_encoder::{BlockType, Instruction, Instruction as I};

use crate::layout::{num, vals};
use crate::module::{Func, Global, HEAP, Ins, Module, Sym, Val, mem};
use crate::rt::Helper;
use crate::{Input, unsupported};

mod count;
mod list;
mod map;
mod text;
mod value;

/// The function of instance `id`, at `slot` in content order.
pub fn instance(input: &Input<'_>, m: &mut Module, _slot: usize, id: InstId) -> StageResult<Func> {
    let t = &input.collected.table;
    let item = t.item[id.idx()];
    let args = t.args[id.idx()];
    let Sub::Body(sub) = t.sub[id.idx()] else {
        return unsupported("adapters, erased bodies and thunks");
    };
    if sub != 0 {
        return unsupported("closures (job 11)");
    }
    let Some(b) = (input.body)(id) else {
        return unsupported("an instance without TIR");
    };
    // A suspending root that no code but the entry calls runs as a plain
    // function: until job 13 any suspension point in it is refused, and a
    // body that never suspends ends on its first poll.
    if input.env.suspends(item) && !input.entry.root(item) {
        return unsupported("suspending functions (job 13)");
    }
    let mut em = Em {
        input,
        m,
        b,
        item,
        args,
        calls: &input.collected.calls[id.idx()],
        f: Func::default(),
        vals: HashMap::default(),
        locals: HashMap::default(),
        providers: Vec::new(),
        ctl: Vec::new(),
        deciding: Vec::new(),
        loops: 0,
        counting: false,
        ret: Vec::new(),
        temps: Vec::new(),
    };
    let mut params = Vec::new();
    for l in param_locals(b) {
        let vs = em.vals(em.sub(b.local_ty[l]))?;
        let at = u32::try_from(params.len()).expect("params");
        em.locals.insert(
            l,
            (at..at + u32::try_from(vs.len()).expect("params")).collect(),
        );
        params.extend(vs);
    }
    for key in input.env.row_keys(item, args) {
        let at = u32::try_from(params.len()).expect("params");
        em.providers.push((key, [at, at + 1]));
        params.extend([Val::I32, Val::I32]);
    }
    let ret = em.sub(input.env.ret(item).unwrap_or(Ty::VOID));
    let results = em.vals(ret)?;
    em.f = Func::new(
        format!("{}#{}", (input.path)(item), t.sub[id.idx()]),
        params,
        results.clone(),
    );
    // A function that holds a counted reference ends at one exit, which
    // releases what it owns (`count.rs`): a return stores its value in
    // `ret` and branches there.
    em.counting = em.counts_anything();
    if em.counting {
        em.ret = results.iter().map(|v| em.f.local(*v)).collect();
        let mut params: Vec<(usize, Vec<u32>)> =
            em.locals.iter().map(|(k, v)| (*k, v.clone())).collect();
        params.sort_unstable();
        for (l, ls) in params {
            em.retain_locals(&ls, em.sub(b.local_ty[l]))?;
        }
        em.open(Ctl::Exit);
    }
    let root = b.sub_root[0];
    em.block_into(root, None)?;
    let tail = b.data[root as usize][1];
    if !results.is_empty() {
        if tail == NONE || em.diverges(tail) {
            em.f.op(Instruction::Unreachable);
        } else if em.counting {
            em.retain_ref(tail)?;
            em.push(tail)?;
            for l in em.ret.clone().iter().rev() {
                em.f.set(*l);
            }
        } else {
            em.push(tail)?;
        }
    }
    if em.counting {
        em.close();
        em.release_all()?;
        for l in em.ret.clone() {
            em.f.get(l);
        }
    }
    Ok(em.f)
}

/// The parameter locals of the body's own function, in order: those
/// flagged as parameters that belong to no closure.
fn param_locals(b: &Body) -> Vec<usize> {
    let mut in_sub = vec![false; b.local_ty.len()];
    for r in &b.sub_params[1..] {
        for &l in &b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize] {
            if let Some(x) = in_sub.get_mut(l as usize) {
                *x = true;
            }
        }
    }
    (0..b.local_ty.len())
        .filter(|&l| b.local_flags[l] & local_flags::PARAM != 0 && !in_sub[l])
        .collect()
}

/// An open Wasm block, for branch depths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctl {
    Plain,
    /// The end of a `Loop`, a `Match` or a labelled `Block`.
    Brk(u32),
    /// The head of a `Loop`.
    Cont(u32),
    /// Arm `k` of `Match` `i`: its code follows this block's end.
    Arm(u32, u32),
    /// The body of a function that counts: its end is the one exit.
    Exit,
}

struct Em<'i, 'a, 'm> {
    input: &'i Input<'a>,
    m: &'m mut Module,
    b: &'a Body,
    item: DefId,
    args: TyList,
    calls: &'i HashMap<u32, Target>,
    f: Func,
    /// Each computed instruction's locals.
    vals: HashMap<u32, Vec<u32>>,
    /// Each TIR local's locals.
    locals: HashMap<usize, Vec<u32>>,
    /// The providers in scope: each key's data and vtable locals.
    providers: Vec<(Ty, [u32; 2])>,
    ctl: Vec<Ctl>,
    /// The `Match` instructions whose decision trees are being emitted.
    deciding: Vec<u32>,
    /// How many `Loop`s the instruction being emitted is inside.
    loops: u32,
    /// Whether the function counts, so that a return goes through its
    /// exit ([`Ctl::Exit`]).
    counting: bool,
    /// The locals of the function's results, at its exit.
    ret: Vec<u32>,
    /// The locals of the collection copies that operands were stored as
    /// (`list.rs`, `copy_src`), each owning one count until the exit.
    temps: Vec<u32>,
}

impl Em<'_, '_, '_> {
    fn pool(&self) -> &InternPool {
        self.input.pool
    }

    fn env(&self) -> &dyn ProgramEnv {
        self.input.env
    }

    fn sub(&self, t: Ty) -> Ty {
        subst(self.input.pool, self.input.env, self.item, self.args, t)
    }

    fn vals(&self, t: Ty) -> StageResult<Vec<Val>> {
        vals(self.input.pool, self.input.env, t)
    }

    fn rec(&self, at: u32) -> Vec<u32> {
        self.b.record(at).to_vec()
    }

    /// The type of value `r`, substituted; `NONE` is the unit value.
    fn ty_of(&self, r: u32) -> Ty {
        if r == NONE {
            return Ty::VOID;
        }
        let t = match Ref(r).as_inst() {
            Some(i) => self.b.ty[i.0 as usize],
            None => self.b.consts[(r & !Ref::CONST_BIT) as usize].0,
        };
        self.sub(t)
    }

    /// The type of instruction `i`, substituted.
    fn ty(&self, i: u32) -> Ty {
        self.sub(self.b.ty[i as usize])
    }

    /// Whether value `r` never produces a value: its type is `never`.
    fn diverges(&self, r: u32) -> bool {
        self.pool().get(self.ty_of(r)) == TyData::Never
    }

    /// The arithmetic view of a type: its width, signedness and float kind.
    fn num(&self, t: Ty) -> (u8, bool, bool) {
        let t = hd_mono::layout::strip_newtype(self.pool(), self.env(), t);
        num(self.pool(), t)
    }

    /// Pushes every value of `r`. `NONE` is the unit value, which has none.
    fn push(&mut self, r: u32) -> StageResult<()> {
        if r == NONE {
            return Ok(());
        }
        if Ref(r).as_inst().is_none() {
            return self.konst(r);
        }
        let Some(ls) = self.vals.get(&r).cloned() else {
            return unsupported("a value its instruction did not compute");
        };
        for l in ls {
            self.f.get(l);
        }
        Ok(())
    }

    /// Pushes value `k` of `r`.
    fn comp(&mut self, r: u32, k: usize) -> StageResult<()> {
        if Ref(r).as_inst().is_none() {
            let Some(ops) = self.konst_ops(r)?.into_iter().nth(k) else {
                return unsupported("a component a constant does not have");
            };
            for op in ops {
                self.f.op(op);
            }
            return Ok(());
        }
        let Some(l) = self.vals.get(&r).and_then(|ls| ls.get(k)).copied() else {
            return unsupported("a value its instruction did not compute");
        };
        self.f.get(l);
        Ok(())
    }

    /// Pushes constant `r`.
    fn konst(&mut self, r: u32) -> StageResult<()> {
        for op in self.konst_ops(r)?.into_iter().flatten() {
            self.f.op(op);
        }
        Ok(())
    }

    /// Constant `r`'s values, each as the instructions that push it: a
    /// string's bytes are static data, and the string is their address
    /// and length.
    fn konst_ops(&mut self, r: u32) -> StageResult<Vec<Vec<Instruction<'static>>>> {
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        let ty = self.sub(ty);
        let stripped = hd_mono::layout::strip_newtype(self.pool(), self.env(), ty);
        if stripped == Ty::STRING {
            let text = self.b.strings[usize::try_from(bits).expect("string constant")].clone();
            // Every empty string is the one static empty value (§23.7).
            let at = if text.is_empty() {
                crate::module::EMPTY
            } else {
                self.m.static_bytes(text.as_bytes(), 1)
            };
            let len = u32::try_from(text.len()).expect("string constant");
            return Ok(vec![
                vec![I::I32Const(at.cast_signed())],
                vec![I::I32Const(len.cast_signed())],
            ]);
        }
        let low = u32::try_from(bits & 0xffff_ffff).expect("low bits");
        Ok(match self.vals(ty)?.as_slice() {
            [] => vec![],
            [Val::I32] => vec![vec![I::I32Const(low.cast_signed())]],
            [Val::I64] => vec![vec![I::I64Const(bits.cast_signed())]],
            // A float literal's constant holds its `f64` bits whatever
            // type the literal ends up with (`hd_check`'s literal).
            [Val::F32] => vec![vec![
                I::F64Const(wasm_encoder::Ieee64::new(bits)),
                I::F32DemoteF64,
            ]],
            [Val::F64] => vec![vec![I::F64Const(wasm_encoder::Ieee64::new(bits))]],
            _ => return unsupported(format!("a constant of type `{}`", self.pool().display(ty))),
        })
    }

    /// Instruction `i`'s locals, made on first use.
    fn slot(&mut self, i: u32) -> StageResult<Vec<u32>> {
        if let Some(ls) = self.vals.get(&i) {
            return Ok(ls.clone());
        }
        let vs = self.vals(self.ty(i))?;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.vals.insert(i, ls.clone());
        Ok(ls)
    }

    /// Stores the values on the stack as instruction `i`'s, a copy that
    /// takes a count of its own ([`Em::store_as`]).
    fn store(&mut self, i: u32) -> StageResult<()> {
        self.store_as(i, false)
    }

    /// TIR local `l`'s locals, made on first use.
    fn local(&mut self, l: u32) -> StageResult<Vec<u32>> {
        let l = l as usize;
        if let Some(ls) = self.locals.get(&l) {
            return Ok(ls.clone());
        }
        let vs = self.vals(self.sub(self.b.local_ty[l]))?;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.locals.insert(l, ls.clone());
        Ok(ls)
    }

    fn open(&mut self, c: Ctl) {
        self.f.op(Instruction::Block(BlockType::Empty));
        self.ctl.push(c);
    }

    fn close(&mut self) {
        self.ctl.pop();
        self.f.op(Instruction::End);
    }

    /// The branch depth of the open block `c`.
    fn depth(&self, c: Ctl) -> StageResult<u32> {
        match self.ctl.iter().rev().position(|x| *x == c) {
            Some(d) => Ok(u32::try_from(d).expect("depth")),
            None => unsupported("a branch to a block that is not open"),
        }
    }

    /// Emits block `blk`'s instructions and stores its tail as `dest`'s
    /// value. Blocks in the list are owned by an instruction, which emits
    /// them.
    fn block_into(&mut self, blk: u32, dest: Option<u32>) -> StageResult<()> {
        if blk == NONE {
            return Ok(());
        }
        if self.b.tags[blk as usize] != Tag::Block {
            return unsupported("a block operand that is not a block");
        }
        let [list, tail] = self.b.data[blk as usize];
        let labelled = self.b.label_inst.contains(&blk);
        if labelled {
            self.open(Ctl::Brk(blk));
        }
        for i in self.rec(list) {
            if self.b.tags[i as usize] != Tag::Block {
                self.inst(i)?;
            }
        }
        if tail != NONE
            && let Some(d) = dest
        {
            self.tail_into(tail, d)?;
        }
        if labelled {
            self.close();
            if let Some(d) = dest {
                // A break out of the block stored its value in the
                // block's own locals.
                if d != blk && self.vals.contains_key(&blk) {
                    for l in self.slot(blk)? {
                        self.f.get(l);
                    }
                    self.store(d)?;
                }
            }
        }
        Ok(())
    }

    /// Stores value `r` as `dest`'s, or ends the path when `r` diverges.
    fn tail_into(&mut self, r: u32, dest: u32) -> StageResult<()> {
        if self.vals(self.ty(dest))?.is_empty() {
            return Ok(());
        }
        if self.diverges(r) {
            self.f.op(Instruction::Unreachable);
            return Ok(());
        }
        self.push(r)?;
        self.store(dest)
    }

    fn inst(&mut self, i: u32) -> StageResult<()> {
        let [a, bw] = self.b.data[i as usize];
        match self.b.tags[i as usize] {
            Tag::LocalGet => {
                for l in self.local(a)? {
                    self.f.get(l);
                }
                self.store(i)?;
            }
            Tag::LocalSet => {
                let ls = self.local(a)?;
                if bw != NONE {
                    if self.diverges(bw) {
                        self.f.op(Instruction::Unreachable);
                        return Ok(());
                    }
                    // The local takes a count of the new value and drops
                    // the old one's.
                    let t = self.sub(self.b.local_ty[a as usize]);
                    if self.counted(t).is_some() {
                        self.retain_ref(bw)?;
                        self.release_locals(&ls, t)?;
                    }
                    self.push(bw)?;
                    for l in ls.iter().rev() {
                        self.f.set(*l);
                    }
                }
            }
            Tag::GlobalGet => {
                let h = self.global_hash(a);
                for (k, v) in self.vals(self.ty(i))?.into_iter().enumerate() {
                    self.f
                        .global_get(Global::Storage(h, u32::try_from(k).expect("k"), v));
                }
                self.store(i)?;
            }
            Tag::GlobalSet => {
                let h = self.global_hash(a);
                if self.diverges(bw) {
                    self.f.op(Instruction::Unreachable);
                    return Ok(());
                }
                let vs = self.vals(self.ty_of(bw))?;
                let t = self.ty_of(bw);
                if self.counted(t).is_some() {
                    self.retain_ref(bw)?;
                    let old: Vec<u32> = vs.iter().map(|v| self.f.local(*v)).collect();
                    for (k, (v, o)) in vs.iter().zip(&old).enumerate() {
                        self.f
                            .global_get(Global::Storage(h, u32::try_from(k).expect("k"), *v));
                        self.f.set(*o);
                    }
                    self.release_locals(&old, t)?;
                }
                self.push(bw)?;
                for (k, v) in vs.into_iter().enumerate().rev() {
                    self.f
                        .global_set(Global::Storage(h, u32::try_from(k).expect("k"), v));
                }
            }
            Tag::Prim => self.prim(i, a, bw)?,
            Tag::And | Tag::Or => {
                let r = self.slot(i)?[0];
                self.push(a)?;
                if self.b.tags[i as usize] == Tag::Or {
                    self.f.op(Instruction::I32Eqz);
                }
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(bw, Some(i))?;
                self.f.op(Instruction::Else);
                self.f.i32(i32::from(self.b.tags[i as usize] == Tag::Or));
                self.f.set(r);
                self.close();
            }
            Tag::Call => self.call(i, a, bw)?,
            Tag::DefaultCall => self.default_call(i, bw)?,
            Tag::Interp => {
                let parts = self.rec(bw);
                self.concat(i, &parts)?;
            }
            Tag::Coerce => {
                let kind = self.rec(bw).first().copied().unwrap_or(NONE);
                match kind {
                    k if k == Coercion::Never as u32 => {
                        self.f.op(Instruction::Unreachable);
                        self.slot(i)?;
                    }
                    k if [
                        Coercion::Weaken,
                        Coercion::Variance,
                        Coercion::RowSubsume,
                        Coercion::SuspendFnToCtor,
                        Coercion::Identity,
                    ]
                    .iter()
                    .any(|c| *c as u32 == k) =>
                    {
                        if self.vals(self.ty_of(a))? != self.vals(self.ty(i))? {
                            return unsupported("a coercion that changes the layout");
                        }
                        self.push(a)?;
                        self.store(i)?;
                    }
                    k if k == Coercion::WrapSome as u32 => {
                        self.new_variant(i, 1, &[value::Src::Ref(a)])?;
                    }
                    _ => return unsupported("trait-value and `Any` coercions (job 11)"),
                }
            }
            Tag::ProviderGet => {
                let key = self.sub(Ty(self.rec(a).first().copied().unwrap_or(NONE)));
                let [d, v] = self.provider(key)?;
                self.f.get(d);
                self.f.get(v);
                self.store(i)?;
            }
            Tag::If => {
                let r = self.rec(bw);
                self.slot(i)?;
                self.push(a)?;
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(r[0], Some(i))?;
                if let Some(&els) = r.get(1).filter(|e| **e != NONE) {
                    self.f.op(Instruction::Else);
                    self.block_into(els, Some(i))?;
                }
                self.close();
            }
            Tag::Loop => {
                self.slot(i)?;
                self.open(Ctl::Brk(i));
                self.f.op(Instruction::Loop(BlockType::Empty));
                self.ctl.push(Ctl::Cont(i));
                self.loops += 1;
                self.block_into(a, None)?;
                self.loops -= 1;
                self.f.op(Instruction::Br(0));
                self.close();
                self.close();
            }
            Tag::Break | Tag::Continue => {
                let Some(&target) = self.b.label_inst.get(a as usize) else {
                    return unsupported("a break with an unknown label");
                };
                if self.b.tags[i as usize] == Tag::Break {
                    if bw != NONE {
                        self.tail_into(bw, target)?;
                    }
                    let d = self.depth(Ctl::Brk(target))?;
                    self.f.op(Instruction::Br(d));
                } else {
                    let d = self.depth(Ctl::Cont(target))?;
                    self.f.op(Instruction::Br(d));
                }
            }
            Tag::Return => {
                if a != NONE && self.diverges(a) {
                    self.f.op(Instruction::Unreachable);
                    return Ok(());
                }
                if self.counting {
                    // The caller's count of the result, then the exit.
                    if a != NONE {
                        self.retain_ref(a)?;
                        self.push(a)?;
                        for l in self.ret.clone().iter().rev() {
                            self.f.set(*l);
                        }
                    }
                    let d = self.depth(Ctl::Exit)?;
                    self.f.op(Instruction::Br(d));
                } else {
                    if a != NONE {
                        self.push(a)?;
                    }
                    self.f.op(Instruction::Return);
                }
            }
            Tag::Unreachable => self.f.op(Instruction::Unreachable),
            Tag::Scope => {
                if !self.rec(bw).is_empty() {
                    return unsupported("`defer` suites (job 13)");
                }
                self.slot(i)?;
                self.open(Ctl::Plain);
                self.block_into(a, Some(i))?;
                self.close();
            }
            Tag::Match => {
                let blocks = self.rec(bw);
                let n = u32::try_from(blocks.len() - 1).expect("arms");
                self.slot(i)?;
                self.open(Ctl::Brk(i));
                for k in (0..n).rev() {
                    self.open(Ctl::Arm(i, k));
                }
                self.deciding.push(i);
                self.block_into(blocks[0], None)?;
                self.deciding.pop();
                self.f.op(Instruction::Unreachable);
                for arm in &blocks[1..] {
                    self.close();
                    self.block_into(*arm, Some(i))?;
                    let d = self.depth(Ctl::Brk(i))?;
                    self.f.op(Instruction::Br(d));
                }
                self.close();
            }
            Tag::ToArm => {
                let Some(&m) = self.deciding.last() else {
                    return unsupported("an arm jump outside a match decision");
                };
                let d = self.depth(Ctl::Arm(m, a))?;
                self.f.op(Instruction::Br(d));
            }
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                let r = self.rec(bw);
                let [case, then, other] = r[..] else {
                    return unsupported("a switch that is not one case and a default");
                };
                if self.b.tags[i as usize] != Tag::SwitchTag
                    && self.vals(self.ty_of(a))? != [Val::I32]
                {
                    return unsupported("a switch on a 64-bit integer");
                }
                if self.b.tags[i as usize] == Tag::SwitchTag {
                    self.tag(a)?;
                } else {
                    self.comp(a, 0)?;
                }
                self.f.u32(case);
                self.f.op(Instruction::I32Eq);
                self.f.op(Instruction::If(BlockType::Empty));
                self.ctl.push(Ctl::Plain);
                self.block_into(then, Some(i))?;
                self.f.op(Instruction::Else);
                if other == NONE {
                    self.f.op(Instruction::Unreachable);
                } else {
                    self.block_into(other, Some(i))?;
                }
                self.close();
            }
            Tag::Payload => {
                let r = self.rec(bw);
                self.payload(i, a, r[0], r[1])?;
            }
            Tag::Unwrap => self.payload(i, a, 1, 0)?,
            Tag::NewVariant => {
                let tys = crate::layout::variants(self.pool(), self.env(), self.ty(i))
                    .and_then(|vs| vs.get(a as usize).cloned())
                    .unwrap_or_default();
                let mut payload = Vec::new();
                for (r, pt) in self.rec(bw).into_iter().zip(tys) {
                    payload.push(self.slot_src(r, pt, false)?);
                }
                self.new_variant(i, a, &payload)?;
            }
            Tag::Intrinsic => {
                let op = hd_tir::ir::IntrinsicOp::from_u32(a);
                let args = self.rec(bw);
                match op {
                    Some(IntrinsicOp::ListLen) => self.list_len(i, args[0])?,
                    Some(IntrinsicOp::ListIndex) => self.list_index(i, &args, false)?,
                    Some(IntrinsicOp::ListIndexMut) => self.list_index(i, &args, true)?,
                    Some(IntrinsicOp::ListSet) => self.list_set(&args)?,
                    Some(IntrinsicOp::ListPush) => self.list_push(&args)?,
                    Some(IntrinsicOp::ValueCopy) => self.value_copy(i, args[0])?,
                    Some(IntrinsicOp::SnapBegin) => self.snap_begin(i, args[0])?,
                    Some(IntrinsicOp::SnapEnd) => self.snap_end(args[1])?,
                    Some(IntrinsicOp::ListPin) => self.list_pin(i, &args)?,
                    Some(IntrinsicOp::ListUnpin) => self.list_unpin(args[0])?,
                    Some(IntrinsicOp::ListIter) => {
                        return unsupported("list iterators, which hold a closure (job 11)");
                    }
                    Some(hd_tir::ir::IntrinsicOp::StrConcat) => self.concat(i, &args)?,
                    Some(hd_tir::ir::IntrinsicOp::StrIndex) => self.str_at(i, args[0], args[1])?,
                    Some(hd_tir::ir::IntrinsicOp::StrEq) => {
                        self.push(args[0])?;
                        self.push(args[1])?;
                        self.f.helper(Helper::StrEq);
                        self.store(i)?;
                    }
                    Some(IntrinsicOp::MapLen) => self.map_len(i, args[0])?,
                    Some(
                        op @ (IntrinsicOp::MapIndex
                        | IntrinsicOp::MapIndexMut
                        | IntrinsicOp::MapGet
                        | IntrinsicOp::MapSet
                        | IntrinsicOp::MapRemove),
                    ) => self.map_op(i, op, &args)?,
                    Some(IntrinsicOp::MapSlots) => self.map_slots(i, args[0])?,
                    Some(IntrinsicOp::MapSlotNext) => self.map_slot_next(i, &args)?,
                    Some(IntrinsicOp::MapSlotEntry) => self.map_slot_entry(i, args[0], args[1])?,
                    Some(IntrinsicOp::MapIter) => {
                        return unsupported("map iterators, which hold a closure (job 11)");
                    }
                    _ => return unsupported("an intrinsic operation without its kernel"),
                }
            }
            Tag::Hook => {}
            Tag::NewData => self.new_data(i, bw)?,
            Tag::CopyData => self.copy_data(i, a, bw)?,
            Tag::Field => self.field(i, a, bw)?,
            Tag::FieldSet => {
                let r = self.rec(bw);
                self.field_set(a, r[0], r[1])?;
            }
            Tag::NewTuple => {
                // The rest list, if any, is the last member.
                let mut members = self.rec(bw);
                if a != NONE {
                    members.push(a);
                }
                let tys = crate::layout::tuple_members(
                    self.pool(),
                    crate::layout::strip(self.pool(), self.env(), self.ty(i)),
                );
                let mut srcs = Vec::new();
                for (r, mt) in members.into_iter().zip(&tys) {
                    srcs.push(self.slot_src(r, *mt, false)?);
                }
                for (s, mt) in srcs.iter().zip(tys) {
                    for k in 0..self.vals(mt)?.len() {
                        self.src(s, k)?;
                    }
                }
                self.store(i)?;
            }
            Tag::TupleGet => self.tuple_get(i, a, bw)?,
            Tag::Is => self.is(i, a, bw)?,
            Tag::NewList => self.new_list(i, a, bw)?,
            Tag::ForList => return unsupported("a `for` over a list that lowering kept"),
            Tag::NewMap => self.new_map(i, bw)?,
            Tag::ForMap => return unsupported("a `for` over a map that lowering kept"),
            Tag::Closure | Tag::ItemRef | Tag::CallValue | Tag::CallDyn => {
                return unsupported("function and `dyn` values (job 11)");
            }
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace | Tag::Defer => {
                return unsupported("suspension and `defer` (job 13)");
            }
            tag => return unsupported(format!("`{}` (job 3, #540)", tag.name())),
        }
        Ok(())
    }

    /// The path hash of a `GlobalGet` or `GlobalSet` binding.
    fn global_hash(&self, a: u32) -> hd_base::Hash128 {
        let def = DefId::from_raw(self.rec(a)[0]);
        self.env().path_hash(def)
    }

    /// The data and vtable locals of the innermost provider of key `k`.
    fn provider(&self, k: Ty) -> StageResult<[u32; 2]> {
        match self.providers.iter().rev().find(|p| p.0 == k) {
            Some((_, ls)) => Ok(*ls),
            None => unsupported(format!(
                "a provider of `{}` this function does not take",
                self.pool().display(k)
            )),
        }
    }

    /// Pushes the providers of a callee's row keys, in key order.
    fn push_providers(&mut self, item: DefId, args: TyList) -> StageResult<()> {
        for key in self.env().row_keys(item, args) {
            let [d, v] = self.provider(key)?;
            self.f.get(d);
            self.f.get(v);
        }
        Ok(())
    }

    /// Stores a call's results as `i`'s, or ends the path when its result
    /// is `never`.
    fn call_results(&mut self, i: u32, ret: Ty) -> StageResult<()> {
        if self.vals(ret)? != self.vals(self.ty(i))? {
            return unsupported("a call whose result is erased");
        }
        if self.pool().get(self.ty(i)) == TyData::Never {
            self.f.op(Instruction::Unreachable);
            return Ok(());
        }
        // A function returns its result with a count for the caller.
        self.store_owned(i)
    }

    fn call(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        let mut args = self.rec(bw);
        args.truncate(args.len().saturating_sub(3));
        match self.calls.get(&i).cloned() {
            Some(Target::Call(ct)) => match &ct.kind {
                TargetKind::Instance => {
                    for r in &args {
                        self.push(*r)?;
                    }
                    self.push_providers(ct.item, ct.args)?;
                    self.f.call(Sym::Inst(ct.key));
                    self.call_results(i, ct.ret)
                }
                TargetKind::Intrinsic(key) => self.intrinsic(i, key, &args),
                TargetKind::Builtin { self_ty, .. } => {
                    let path = (self.input.path)(ct.item);
                    let name = path.rsplit(['.', '/']).next().unwrap_or("").to_owned();
                    self.builtin(i, &name, *self_ty, &args)
                }
            },
            Some(Target::Dyn) => self.call_dyn(i, a, &args),
            _ => unsupported("a call without a direct target (job 11)"),
        }
    }

    /// A trait method call at a trait-value receiver `args[0]`: an
    /// indirect call through the slot of the receiver's vtable that the
    /// method's position in the trait names.
    fn call_dyn(&mut self, i: u32, a: u32, args: &[u32]) -> StageResult<()> {
        let Some(Callee::TraitMethod {
            trait_,
            method,
            targs,
            ..
        }) = Callee::from_words(self.b.record(a))
        else {
            return unsupported("a dynamic call without a trait method");
        };
        let Some(slot) = self
            .env()
            .trait_methods(trait_)
            .iter()
            .position(|m| *m == method)
        else {
            return unsupported("a dynamic call of a method without a slot");
        };
        if !self.env().row_keys(method, targs).is_empty() {
            return unsupported("a dynamic call with providers (job 11)");
        }
        let Some((&recv, rest)) = args.split_first() else {
            return unsupported("a dynamic call without a receiver");
        };
        let mut params = vec![Val::I32];
        self.comp(recv, 0)?;
        for r in rest {
            params.extend(self.vals(self.ty_of(*r))?);
            self.push(*r)?;
        }
        self.comp(recv, 1)?;
        self.f.op(Instruction::I32Load(mem(
            HEAP,
            4 * u64::try_from(slot).expect("slot"),
            2,
        )));
        let results = self.vals(self.ty(i))?;
        self.f.body.push(Ins::CallIndirect(params, results));
        let ty = self.ty(i);
        self.call_results(i, ty)
    }

    /// An omitted argument: its default body's instance, called with the
    /// earlier arguments, or a constant default inlined.
    fn default_call(&mut self, i: u32, bw: u32) -> StageResult<()> {
        match self.calls.get(&i).cloned() {
            Some(Target::ConstDefault(bits)) => {
                match self.vals(self.ty(i))?.as_slice() {
                    [Val::I32] => self.f.i32(i32::from_ne_bytes(
                        u32::try_from(bits & 0xffff_ffff)
                            .expect("low bits")
                            .to_ne_bytes(),
                    )),
                    [Val::I64] => self.f.op(Instruction::I64Const(i64::from_ne_bytes(
                        bits.to_ne_bytes(),
                    ))),
                    [Val::F64] => self
                        .f
                        .op(Instruction::F64Const(wasm_encoder::Ieee64::new(bits))),
                    _ => return unsupported("a constant default of this layout"),
                }
                self.store(i)
            }
            Some(Target::Default { call, bracket }) => {
                for r in self.rec(bw) {
                    self.push(r)?;
                }
                self.push_providers(call.item, call.args)?;
                if bracket {
                    self.forbid_by(1);
                }
                self.f.call(Sym::Inst(call.key));
                if bracket {
                    self.forbid_by(-1);
                }
                self.call_results(i, call.ret)
            }
            _ => unsupported("a default argument without a target"),
        }
    }

    fn forbid_by(&mut self, by: i32) {
        self.f.global_get(Global::Forbid);
        self.f.i32(by);
        self.f.op(Instruction::I32Add);
        self.f.global_set(Global::Forbid);
    }

    fn intrinsic(&mut self, i: u32, key: &str, args: &[u32]) -> StageResult<()> {
        match key {
            // `panic(message)`: the `explicit-panic` report.
            "panic_message" => {
                self.push(args[0])?;
                self.f.helper(Helper::PanicStr);
                self.f.op(Instruction::Unreachable);
                Ok(())
            }
            // std's `panic_with(category, message)`.
            "panic" => {
                self.push(args[0])?;
                self.push(args[1])?;
                self.f.helper(Helper::PanicWith);
                self.f.op(Instruction::Unreachable);
                Ok(())
            }
            // std's `format_int[N < Integer](value: N)`.
            "format_int" => {
                let t = self.ty_of(args[0]);
                let (bits, signed, float) = self.num(t);
                if float {
                    return unsupported("`format_int` of a float");
                }
                self.push(args[0])?;
                if bits < 64 {
                    self.f.op(if signed {
                        Instruction::I64ExtendI32S
                    } else {
                        Instruction::I64ExtendI32U
                    });
                }
                self.f.helper(Helper::FmtInt { signed });
                self.store_owned(i)
            }
            "block_on" => self.block_on(i, args[0]),
            "map_hash_seed" => self.hash_seed(i),
            "bytes_len" => {
                self.comp(args[0], 1)?;
                self.store(i)
            }
            "bytes_at" => self.str_at(i, args[0], args[1]),
            "bytes_slice" => self.str_slice(i, args[0], args[1], args[2]),
            "bytes_concat" => self.concat(i, args),
            // A `char` is its scalar value at run time.
            "char_scalar" => {
                self.comp(args[0], 0)?;
                self.store(i)
            }
            "char_from_scalar" => self.char_from_scalar(i, args[0]),
            // The string kernels over lists (§23.17).
            "string_from_bytes" | "concat_all" => {
                self.comp(args[0], 0)?;
                self.f.helper(if key == "concat_all" {
                    Helper::ConcatAll
                } else {
                    Helper::StrFromBytes
                });
                self.store_owned(i)
            }
            // The bit casts keep every bit, a NaN's sign and payload too.
            "f64_to_bits" | "f64_from_bits" | "f32_to_bits" | "f32_from_bits" => {
                self.comp(args[0], 0)?;
                self.f.op(match key {
                    "f64_to_bits" => I::I64ReinterpretF64,
                    "f64_from_bits" => I::F64ReinterpretI64,
                    "f32_to_bits" => I::I32ReinterpretF32,
                    _ => I::F32ReinterpretI32,
                });
                self.store(i)
            }
            "format_f64" | "format_f32" | "format_f64_fixed" | "string_lower" | "string_upper"
            | "parse_f64" | "snapshot_update" => self.host_prim(i, key, args),
            key if key.starts_with("mem_") => self.memory(i, key, args),
            other => unsupported(format!("the intrinsic `{other}`")),
        }
    }

    /// `std.rt.memory`'s intrinsics on the heap, memory 1
    /// (`module.rt.memory.*`): each is one instruction. A load or a store
    /// is little-endian and takes any address; `grow` reads Wasm's `-1` as
    /// `.None`; `copy` is `memory.copy`, which allows overlap.
    fn memory(&mut self, i: u32, key: &str, args: &[u32]) -> StageResult<()> {
        let op = match key {
            "mem_pages" => I::MemorySize(HEAP),
            "mem_grow" => {
                self.push(args[0])?;
                self.f.op(I::MemoryGrow(HEAP));
                let before = self.f.local(Val::I32);
                self.f.set(before);
                self.f.get(before);
                self.f.i32(-1);
                self.f.op(I::I32Ne);
                self.f.get(before);
                return self.store(i);
            }
            "mem_load_u8" => I::I32Load8U(mem(HEAP, 0, 0)),
            "mem_load_u16" => I::I32Load16U(mem(HEAP, 0, 1)),
            "mem_load_u32" => I::I32Load(mem(HEAP, 0, 2)),
            "mem_load_u64" => I::I64Load(mem(HEAP, 0, 3)),
            "mem_store_u8" => I::I32Store8(mem(HEAP, 0, 0)),
            "mem_store_u16" => I::I32Store16(mem(HEAP, 0, 1)),
            "mem_store_u32" => I::I32Store(mem(HEAP, 0, 2)),
            "mem_store_u64" => I::I64Store(mem(HEAP, 0, 3)),
            "mem_copy" => I::MemoryCopy {
                src_mem: HEAP,
                dst_mem: HEAP,
            },
            "mem_fill" => I::MemoryFill(HEAP),
            other => return unsupported(format!("the intrinsic `{other}`")),
        };
        for a in args {
            self.push(*a)?;
        }
        self.f.op(op);
        self.store(i)
    }

    /// `block_on(s)` (`req.drive.block-on.*`): polls the suspension's frame
    /// through its poll slot, and while it is pending waits in the host
    /// (`hd:rt.block`), then gives `.Completed(value)`. No suspension of
    /// this backend is cancelled yet, so `.Cancelled` and `.Abandoned` are
    /// never made.
    fn block_on(&mut self, i: u32, s: u32) -> StageResult<()> {
        let TyData::Adt { args, .. } = self.pool().get(self.strip_mut(self.ty_of(s))) else {
            return unsupported("`block_on` of a value that is not a suspension");
        };
        let Some(&inner) = self.pool().list_items(args).first() else {
            return unsupported("`block_on` of a suspension without its result type");
        };
        let result = self.vals(inner)?;
        self.f.helper(Helper::ForbidCheck);
        let frame = self.f.local(Val::I32);
        let ready = self.f.local(Val::I32);
        let tmp: Vec<u32> = result.iter().map(|v| self.f.local(*v)).collect();
        self.comp(s, 0)?;
        self.f.set(frame);
        self.f.op(Instruction::Block(BlockType::Empty));
        self.f.op(Instruction::Loop(BlockType::Empty));
        self.f.get(frame);
        self.f.get(frame);
        self.f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        self.f.body.push(Ins::CallIndirect(
            vec![Val::I32],
            [vec![Val::I32], result.clone()].concat(),
        ));
        for l in tmp.iter().rev() {
            self.f.set(*l);
        }
        self.f.set(ready);
        self.f.get(ready);
        self.f.op(Instruction::BrIf(1));
        self.f.call(Sym::Import(crate::module::Import::new(
            "hd:rt",
            "block",
            &[],
            &[Val::I32],
        )));
        self.f.op(Instruction::Drop);
        self.f.op(Instruction::Br(0));
        self.f.op(Instruction::End);
        self.f.op(Instruction::End);
        // `.Completed(value)`, which counts its own copy of the poll's
        // result.
        self.new_variant(i, 0, &[value::Src::Locals(tmp.clone())])?;
        self.release_locals(&tmp, inner)
    }

    /// A body-less method of a primitive's std impl at `self_ty`
    /// (`expr.op.std.intrinsic-method`, `expr.ord.std.intrinsic`): the
    /// operator's lowering, an `Ordering`, or the value's text.
    fn builtin(&mut self, i: u32, name: &str, self_ty: Ty, args: &[u32]) -> StageResult<()> {
        let t = hd_mono::layout::strip_newtype(self.pool(), self.env(), self.strip_mut(self_ty));
        let prim = matches!(self.pool().get(t), TyData::Prim(_));
        let op = match name {
            "add" => Some(PrimOp::Add),
            "sub" => Some(PrimOp::Sub),
            "mul" => Some(PrimOp::Mul),
            "div" => Some(PrimOp::Div),
            "rem" => Some(PrimOp::Rem),
            "neg" => Some(PrimOp::Neg),
            "bit_and" => Some(PrimOp::BitAnd),
            "bit_or" => Some(PrimOp::BitOr),
            "bit_xor" => Some(PrimOp::BitXor),
            "not" => Some(PrimOp::Not),
            "shl" => Some(PrimOp::Shl),
            "shr" => Some(PrimOp::Shr),
            "eq" if t != Ty::STRING => Some(PrimOp::Eq),
            _ => None,
        };
        if prim && let Some(op) = op {
            self.prim_value(op, args, self.ty(i))?;
            return self.store(i);
        }
        let (_, signed, float) = self.num(t);
        match name {
            "cmp" | "partial_cmp" if prim && t != Ty::STRING => {
                self.ordering(i, args, name == "partial_cmp", float)
            }
            "to_string" if t == Ty::BOOL => {
                let yes = self.m.static_bytes(b"true", 1);
                let no = self.m.static_bytes(b"false", 1);
                for (a, b) in [(yes, no), (4, 5)] {
                    self.f.u32(a);
                    self.f.u32(b);
                    self.comp(args[0], 0)?;
                    self.f.op(I::Select);
                }
                self.store(i)
            }
            "to_string" if t == Ty::STRING => {
                self.push(args[0])?;
                self.store(i)
            }
            "to_string" if prim && !float => {
                let (bits, _, _) = self.num(t);
                self.push(args[0])?;
                if bits < 64 {
                    self.f.op(if signed {
                        I::I64ExtendI32S
                    } else {
                        I::I64ExtendI32U
                    });
                }
                self.f.helper(Helper::FmtInt { signed });
                self.store_owned(i)
            }
            // A float's text is the host primitive its `Display`
            // implementation calls.
            "to_string" if prim && float => {
                let key = if self.vals(t)? == [Val::F32] {
                    "format_f32"
                } else {
                    "format_f64"
                };
                self.host_prim(i, key, args)
            }
            "cmp" | "partial_cmp" if t == Ty::STRING => {
                self.str_ordering(i, args, name == "partial_cmp")
            }
            "eq" if t == Ty::STRING => {
                self.push(args[0])?;
                self.push(args[1])?;
                self.f.helper(Helper::StrEq);
                self.store(i)
            }
            "sqrt" if prim && float => {
                self.comp(args[0], 0)?;
                self.f.op(if self.vals(t)? == [Val::F32] {
                    I::F32Sqrt
                } else {
                    I::F64Sqrt
                });
                self.store(i)
            }
            _ => unsupported(format!("the built-in method `{name}`")),
        }
    }

    /// `cmp` or `partial_cmp` of two numbers or characters: `>` minus
    /// `<`, plus one, as an `Ordering` (`Less`, `Equal`, `Greater`), which
    /// `partial_cmp` wraps in `.Some`, or `.None` when a float operand is
    /// NaN (`expr.ord.unordered`).
    fn ordering(&mut self, i: u32, args: &[u32], partial: bool, float: bool) -> StageResult<()> {
        let ops = [args[0], args[1]];
        let tag = self.f.local(Val::I32);
        self.f.i32(1);
        self.prim_value(PrimOp::Gt, &ops, Ty::BOOL)?;
        self.f.op(I::I32Add);
        self.prim_value(PrimOp::Lt, &ops, Ty::BOOL)?;
        self.f.op(I::I32Sub);
        self.f.set(tag);
        let want = if partial { 2 } else { 1 };
        if self.vals(self.ty(i))?.len() != want {
            return unsupported("an `Ordering` of this layout");
        }
        if partial {
            if float {
                let some = self.f.local(Val::I32);
                self.prim_value(PrimOp::Eq, &[ops[0], ops[0]], Ty::BOOL)?;
                self.prim_value(PrimOp::Eq, &[ops[1], ops[1]], Ty::BOOL)?;
                self.f.op(I::I32And);
                self.f.set(some);
                self.f.get(some);
                self.f.get(tag);
                self.f.get(some);
                self.f.op(I::I32Mul);
            } else {
                self.f.i32(1);
                self.f.get(tag);
            }
        } else {
            self.f.get(tag);
        }
        self.store(i)
    }

    fn strip_mut(&self, t: Ty) -> Ty {
        match self.pool().get(t) {
            TyData::Mut(inner) => inner,
            _ => t,
        }
    }

    fn panic(&mut self, msg: &str) {
        self.f.helper(Helper::Panic(msg.to_owned()));
        self.f.op(Instruction::Unreachable);
    }

    fn prim(&mut self, i: u32, op: u32, rec: u32) -> StageResult<()> {
        let Some(op) = PrimOp::from_u32(op) else {
            return unsupported("an unknown operator");
        };
        let ops = self.rec(rec);
        self.prim_value(op, &ops, self.ty(i))?;
        self.store(i)
    }

    /// Pushes the value of operator `op` on `ops`, whose result has type
    /// `ty`: the `Prim` instruction and the intrinsic methods of the
    /// primitives' operator and comparison impls
    /// (`expr.op.std.intrinsic-method`) share it.
    fn prim_value(&mut self, op: PrimOp, ops: &[u32], ty: Ty) -> StageResult<()> {
        let t0 = ops.first().map_or(Ty::I32, |r| self.ty_of(*r));
        let (bits, signed, float) = self.num(t0);
        let v0 = self.vals(t0)?;
        let [v] = v0[..] else {
            return unsupported(format!("the operator {op:?} on this layout"));
        };
        let wide = v == Val::I64;
        if float && op != PrimOp::Conv {
            return self.float_value(op, ops, v);
        }
        match op {
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Neg if self.input.release => {
                self.wrapping(op, ops, bits, signed)?;
            }
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem | PrimOp::Neg => {
                self.checked(op, ops, bits, signed)?;
            }
            PrimOp::Pow => return unsupported("integer `**` (job 3, #540)"),
            PrimOp::Eq | PrimOp::Ne | PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge => {
                for r in ops {
                    self.comp(*r, 0)?;
                }
                self.f.op(match (op, wide, signed) {
                    (PrimOp::Eq, false, _) => I::I32Eq,
                    (PrimOp::Ne, false, _) => I::I32Ne,
                    (PrimOp::Lt, false, true) => I::I32LtS,
                    (PrimOp::Lt, false, false) => I::I32LtU,
                    (PrimOp::Le, false, true) => I::I32LeS,
                    (PrimOp::Le, false, false) => I::I32LeU,
                    (PrimOp::Gt, false, true) => I::I32GtS,
                    (PrimOp::Gt, false, false) => I::I32GtU,
                    (PrimOp::Ge, false, true) => I::I32GeS,
                    (PrimOp::Ge, false, false) => I::I32GeU,
                    (PrimOp::Eq, true, _) => I::I64Eq,
                    (PrimOp::Ne, true, _) => I::I64Ne,
                    (PrimOp::Lt, true, true) => I::I64LtS,
                    (PrimOp::Lt, true, false) => I::I64LtU,
                    (PrimOp::Le, true, true) => I::I64LeS,
                    (PrimOp::Le, true, false) => I::I64LeU,
                    (PrimOp::Gt, true, true) => I::I64GtS,
                    (PrimOp::Gt, true, false) => I::I64GtU,
                    (PrimOp::Ge, true, true) => I::I64GeS,
                    _ => I::I64GeU,
                });
            }
            PrimOp::And | PrimOp::BitAnd | PrimOp::Or | PrimOp::BitOr | PrimOp::BitXor => {
                for r in ops {
                    self.comp(*r, 0)?;
                }
                self.f.op(match (op, wide) {
                    (PrimOp::And | PrimOp::BitAnd, false) => I::I32And,
                    (PrimOp::And | PrimOp::BitAnd, true) => I::I64And,
                    (PrimOp::Or | PrimOp::BitOr, false) => I::I32Or,
                    (PrimOp::Or | PrimOp::BitOr, true) => I::I64Or,
                    (_, false) => I::I32Xor,
                    (_, true) => I::I64Xor,
                });
            }
            // Bits shifted past a sub-word width are dropped, as at 32 and
            // 64 bits, so the result is brought back to canonical form.
            PrimOp::Shl => {
                self.shift(ops, bits, wide)?;
                if wide {
                    self.f.op(Instruction::I64Shl);
                } else {
                    self.f.op(Instruction::I32Shl);
                    self.wrap_to(bits, signed);
                }
            }
            PrimOp::Shr => {
                self.shift(ops, bits, wide)?;
                self.f.op(match (wide, signed) {
                    (false, true) => Instruction::I32ShrS,
                    (false, false) => Instruction::I32ShrU,
                    (true, true) => Instruction::I64ShrS,
                    (true, false) => Instruction::I64ShrU,
                });
            }
            PrimOp::Not => {
                self.comp(ops[0], 0)?;
                let stripped = hd_mono::layout::strip_newtype(self.pool(), self.env(), t0);
                if stripped == Ty::BOOL {
                    self.f.op(Instruction::I32Eqz);
                } else if wide {
                    self.f.op(Instruction::I64Const(-1));
                    self.f.op(Instruction::I64Xor);
                } else {
                    // A sign-extended value stays so under `~`; a
                    // zero-extended one flips only its own bits.
                    self.f.i32(if signed || bits >= 32 {
                        -1
                    } else {
                        width_mask(bits)
                    });
                    self.f.op(Instruction::I32Xor);
                }
            }
            PrimOp::Conv => self.convert(ops[0], v, (signed, float), ty)?,
        }
        Ok(())
    }

    /// A shift's count: any unsigned type (`expr.shift.count-unsigned`),
    /// below the shifted value's width in a checked build
    /// (`types.arith.shift-count.debug`) and masked to it in a release
    /// build (`types.arith.shift-count.release`), brought to the value's
    /// width. Pushes the value, then the count.
    fn shift(&mut self, ops: &[u32], bits: u8, wide: bool) -> StageResult<()> {
        self.comp(ops[0], 0)?;
        let [cv] = self.vals(self.ty_of(ops[1]))?[..] else {
            return unsupported("a shift count without one value");
        };
        let n = self.f.local(cv);
        self.comp(ops[1], 0)?;
        self.f.set(n);
        self.f.get(n);
        let c64 = cv == Val::I64;
        if self.input.release {
            if bits < 32 {
                if c64 {
                    self.f.op(Instruction::I64Const(i64::from(bits) - 1));
                    self.f.op(Instruction::I64And);
                } else {
                    self.f.i32(i32::from(bits) - 1);
                    self.f.op(Instruction::I32And);
                }
            }
        } else {
            if c64 {
                self.f.op(Instruction::I64Const(i64::from(bits)));
                self.f.op(Instruction::I64GeU);
            } else {
                self.f.i32(i32::from(bits));
                self.f.op(Instruction::I32GeU);
            }
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("invalid-shift: shift count out of range");
            self.f.op(Instruction::End);
            self.f.get(n);
        }
        match (c64, wide) {
            (false, true) => self.f.op(Instruction::I64ExtendI32U),
            (true, false) => self.f.op(Instruction::I32WrapI64),
            _ => {}
        }
        Ok(())
    }

    /// A numeric cast of `op` from layout `from` to type `ty`
    /// (`types.cast.*`); no cast panics (`types.cast.no-panic`).
    fn convert(
        &mut self,
        op: u32,
        from: Val,
        (signed, float): (bool, bool),
        ty: Ty,
    ) -> StageResult<()> {
        let [to] = self.vals(ty)?[..] else {
            return unsupported("a numeric cast to a type without one value");
        };
        let (tbits, tsigned, tfloat) = self.num(ty);
        self.comp(op, 0)?;
        match (from, to) {
            (Val::I32, Val::I64) => self.f.op(if signed {
                I::I64ExtendI32S
            } else {
                I::I64ExtendI32U
            }),
            (Val::I64, Val::I32) => self.f.op(I::I32WrapI64),
            (Val::I32, Val::F32) => self.f.op(if signed {
                I::F32ConvertI32S
            } else {
                I::F32ConvertI32U
            }),
            (Val::I64, Val::F32) => self.f.op(if signed {
                I::F32ConvertI64S
            } else {
                I::F32ConvertI64U
            }),
            (Val::I32, Val::F64) => self.f.op(if signed {
                I::F64ConvertI32S
            } else {
                I::F64ConvertI32U
            }),
            (Val::I64, Val::F64) => self.f.op(if signed {
                I::F64ConvertI64S
            } else {
                I::F64ConvertI64U
            }),
            (Val::F32, Val::F64) => self.f.op(I::F64PromoteF32),
            (Val::F64, Val::F32) => self.f.op(I::F32DemoteF64),
            (Val::F32 | Val::F64, Val::I32 | Val::I64) => {
                self.float_to_int(from, tbits, tsigned);
            }
            (a, b) if a == b => {}
            _ => return unsupported("this numeric conversion"),
        }
        // An integer cast keeps the target's low bits (`types.cast.wrap`).
        if !tfloat && !float {
            self.wrap_to(tbits, tsigned);
        }
        Ok(())
    }

    /// Truncates the float on the stack toward zero and saturates it to
    /// the integer type of `bits` and `signed` (`types.cast.saturate`).
    fn float_to_int(&mut self, from: Val, bits: u8, signed: bool) {
        let single = from == Val::F32;
        self.f.op(match (bits, single, signed) {
            (64, true, true) => I::I64TruncSatF32S,
            (64, true, false) => I::I64TruncSatF32U,
            (64, false, true) => I::I64TruncSatF64S,
            (64, false, false) => I::I64TruncSatF64U,
            (32, true, true) => I::I32TruncSatF32S,
            (32, true, false) => I::I32TruncSatF32U,
            (32, false, true) => I::I32TruncSatF64S,
            (32, false, false) => I::I32TruncSatF64U,
            (_, true, _) => I::I32TruncSatF32S,
            (_, false, _) => I::I32TruncSatF64S,
        });
        if bits >= 32 {
            return;
        }
        let (lo, hi) = if signed {
            (-(1i32 << (bits - 1)), (1i32 << (bits - 1)) - 1)
        } else {
            (0, width_mask(bits))
        };
        let v = self.f.local(Val::I32);
        self.f.set(v);
        for (bound, cmp) in [(lo, I::I32LtS), (hi, I::I32GtS)] {
            self.f.i32(bound);
            self.f.get(v);
            self.f.get(v);
            self.f.i32(bound);
            self.f.op(cmp);
            self.f.op(I::Select);
            self.f.set(v);
        }
        self.f.get(v);
    }

    /// Brings an `i32` holding a `bits`-wide integer's low bits back to
    /// canonical form: sign-extended when `signed`, zero-extended otherwise.
    fn wrap_to(&mut self, bits: u8, signed: bool) {
        match (bits, signed) {
            (8, true) => self.f.op(Instruction::I32Extend8S),
            (16, true) => self.f.op(Instruction::I32Extend16S),
            (8 | 16, false) => {
                self.f.i32(width_mask(bits));
                self.f.op(Instruction::I32And);
            }
            _ => {}
        }
    }

    /// The basic float operations and comparisons, at the operand's width
    /// (`expr.float.basic`); `%` and `**` come with their helpers later.
    fn float_value(&mut self, op: PrimOp, ops: &[u32], v: Val) -> StageResult<()> {
        let single = v == Val::F32;
        for r in ops {
            self.comp(*r, 0)?;
        }
        self.f.op(match (op, single) {
            (PrimOp::Neg, false) => I::F64Neg,
            (PrimOp::Add, false) => I::F64Add,
            (PrimOp::Sub, false) => I::F64Sub,
            (PrimOp::Mul, false) => I::F64Mul,
            (PrimOp::Div, false) => I::F64Div,
            (PrimOp::Eq, false) => I::F64Eq,
            (PrimOp::Ne, false) => I::F64Ne,
            (PrimOp::Lt, false) => I::F64Lt,
            (PrimOp::Le, false) => I::F64Le,
            (PrimOp::Gt, false) => I::F64Gt,
            (PrimOp::Ge, false) => I::F64Ge,
            (PrimOp::Neg, true) => I::F32Neg,
            (PrimOp::Add, true) => I::F32Add,
            (PrimOp::Sub, true) => I::F32Sub,
            (PrimOp::Mul, true) => I::F32Mul,
            (PrimOp::Div, true) => I::F32Div,
            (PrimOp::Eq, true) => I::F32Eq,
            (PrimOp::Ne, true) => I::F32Ne,
            (PrimOp::Lt, true) => I::F32Lt,
            (PrimOp::Le, true) => I::F32Le,
            (PrimOp::Gt, true) => I::F32Gt,
            (PrimOp::Ge, true) => I::F32Ge,
            _ => return unsupported(format!("the float operator {op:?} (job 3, #540)")),
        });
        Ok(())
    }

    /// Release-build `+`, `-`, `*` and unary `-` (`types.arith.release`):
    /// the native operation, brought back to a sub-word type's canonical
    /// form. Division and remainder stay checked (`types.arith.always`).
    fn wrapping(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let wide = bits == 64;
        if op == PrimOp::Neg {
            self.f.op(if wide {
                Instruction::I64Const(0)
            } else {
                Instruction::I32Const(0)
            });
            self.comp(ops[0], 0)?;
        } else {
            self.comp(ops[0], 0)?;
            self.comp(ops[1], 0)?;
        }
        self.f.op(match (op, wide) {
            (PrimOp::Add, true) => I::I64Add,
            (PrimOp::Mul, true) => I::I64Mul,
            (_, true) => I::I64Sub,
            (PrimOp::Add, false) => I::I32Add,
            (PrimOp::Mul, false) => I::I32Mul,
            (_, false) => I::I32Sub,
        });
        self.wrap_to(bits, signed);
        Ok(())
    }

    /// Checked integer arithmetic (`types.arith.checked`): sub-word widths
    /// compute exactly in `i32` and 32-bit widths in `i64`, each checked
    /// against the type's range; 64-bit widths test the native result.
    fn checked(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let wide = bits == 64;
        let in64 = bits >= 32;
        let exact = if in64 { Val::I64 } else { Val::I32 };
        let (x, y, r) = (
            self.f.local(exact),
            self.f.local(exact),
            self.f.local(exact),
        );
        let (lhs, rhs) = if op == PrimOp::Neg {
            (None, ops[0])
        } else {
            (Some(ops[0]), ops[1])
        };
        for (src, dst) in [(lhs, x), (Some(rhs), y)] {
            match src {
                Some(v) => {
                    self.comp(v, 0)?;
                    if bits == 32 {
                        self.f.op(if signed {
                            Instruction::I64ExtendI32S
                        } else {
                            Instruction::I64ExtendI32U
                        });
                    }
                }
                None => self.f.op(if in64 {
                    Instruction::I64Const(0)
                } else {
                    Instruction::I32Const(0)
                }),
            }
            self.f.set(dst);
        }
        if matches!(op, PrimOp::Div | PrimOp::Rem) {
            self.f.get(y);
            self.f.op(if in64 {
                Instruction::I64Eqz
            } else {
                Instruction::I32Eqz
            });
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("integer-division-by-zero: division by zero");
            self.f.op(Instruction::End);
        }
        // `MIN / -1` overflows in every build (`types.arith.min-division`);
        // at 64 bits it is tested before `i64.div_s`, which would trap.
        if wide && signed && op == PrimOp::Div {
            self.f.get(x);
            self.f.op(Instruction::I64Const(i64::MIN));
            self.f.op(Instruction::I64Eq);
            self.f.get(y);
            self.f.op(Instruction::I64Const(-1));
            self.f.op(Instruction::I64Eq);
            self.f.op(Instruction::I32And);
            self.f.op(Instruction::If(BlockType::Empty));
            self.panic("integer-overflow: integer overflow");
            self.f.op(Instruction::End);
        }
        self.f.get(x);
        self.f.get(y);
        self.f.op(match (op, signed, in64) {
            (PrimOp::Add, _, true) => I::I64Add,
            (PrimOp::Sub | PrimOp::Neg, _, true) => I::I64Sub,
            (PrimOp::Mul, _, true) => I::I64Mul,
            (PrimOp::Div, true, true) => I::I64DivS,
            (PrimOp::Div, false, true) => I::I64DivU,
            (PrimOp::Rem, true, true) => I::I64RemS,
            (_, _, true) => I::I64RemU,
            (PrimOp::Add, _, false) => I::I32Add,
            (PrimOp::Sub | PrimOp::Neg, _, false) => I::I32Sub,
            (PrimOp::Mul, _, false) => I::I32Mul,
            (PrimOp::Div, true, false) => I::I32DivS,
            (PrimOp::Div, false, false) => I::I32DivU,
            (PrimOp::Rem, true, false) => I::I32RemS,
            (_, _, false) => I::I32RemU,
        });
        self.f.set(r);
        if wide {
            self.wide_overflow(op, signed, (x, y, r));
        } else {
            // The exact result against the type's range; an unsigned result
            // below zero reads as a large unsigned value.
            let (lo, hi): (i64, i64) = if signed {
                (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
            } else {
                (0, (1i64 << bits) - 1)
            };
            let bound = |em: &mut Self, v: i64| {
                if in64 {
                    em.f.op(I::I64Const(v));
                } else {
                    em.f.i32(i32::try_from(v).expect("a sub-word bound"));
                }
            };
            self.f.get(r);
            bound(self, hi);
            self.f.op(match (signed, in64) {
                (true, true) => I::I64GtS,
                (false, true) => I::I64GtU,
                (true, false) => I::I32GtS,
                (false, false) => I::I32GtU,
            });
            if signed {
                self.f.get(r);
                bound(self, lo);
                self.f.op(if in64 { I::I64LtS } else { I::I32LtS });
                self.f.op(I::I32Or);
            }
        }
        self.f.op(I::If(BlockType::Empty));
        self.panic("integer-overflow: integer overflow");
        self.f.op(I::End);
        self.f.get(r);
        if bits == 32 {
            self.f.op(I::I32WrapI64);
        }
        Ok(())
    }

    /// Whether the 64-bit `r = x op y` overflowed, as an `i32` on the
    /// stack.
    fn wide_overflow(&mut self, op: PrimOp, signed: bool, (x, y, r): (u32, u32, u32)) {
        match (op, signed) {
            (PrimOp::Add, true) => {
                for i in [I::LocalGet(x), I::LocalGet(r), I::I64Xor] {
                    self.f.op(i);
                }
                for i in [I::LocalGet(y), I::LocalGet(r), I::I64Xor, I::I64And] {
                    self.f.op(i);
                }
                self.f.op(I::I64Const(0));
                self.f.op(I::I64LtS);
            }
            (PrimOp::Sub | PrimOp::Neg, true) => {
                for i in [I::LocalGet(x), I::LocalGet(y), I::I64Xor] {
                    self.f.op(i);
                }
                for i in [I::LocalGet(x), I::LocalGet(r), I::I64Xor, I::I64And] {
                    self.f.op(i);
                }
                self.f.op(I::I64Const(0));
                self.f.op(I::I64LtS);
            }
            (PrimOp::Add, false) => {
                for i in [I::LocalGet(r), I::LocalGet(x), I::I64LtU] {
                    self.f.op(i);
                }
            }
            (PrimOp::Sub | PrimOp::Neg, false) => {
                for i in [I::LocalGet(x), I::LocalGet(y), I::I64LtU] {
                    self.f.op(i);
                }
            }
            // The product overflowed when `x` is not zero and `r / x` is
            // not `y`; `x = -1` is tested apart, since `MIN / -1` traps.
            (PrimOp::Mul, _) => {
                let o = self.f.local(Val::I32);
                self.f.get(x);
                self.f.op(I::I64Eqz);
                self.f.op(I::I32Eqz);
                self.f.op(I::If(BlockType::Empty));
                if signed {
                    self.f.get(x);
                    self.f.op(I::I64Const(-1));
                    self.f.op(I::I64Eq);
                    self.f.op(I::If(BlockType::Empty));
                    self.f.get(y);
                    self.f.op(I::I64Const(i64::MIN));
                    self.f.op(I::I64Eq);
                    self.f.set(o);
                    self.f.op(I::Else);
                }
                self.f.get(r);
                self.f.get(x);
                self.f.op(if signed { I::I64DivS } else { I::I64DivU });
                self.f.get(y);
                self.f.op(I::I64Ne);
                self.f.set(o);
                if signed {
                    self.f.op(I::End);
                }
                self.f.op(I::End);
                self.f.get(o);
            }
            // Division and remainder overflow only at `MIN / -1`, tested
            // before the division.
            _ => self.f.i32(0),
        }
    }
}

/// The mask of a sub-word width's bits.
const fn width_mask(bits: u8) -> i32 {
    (1i32 << bits) - 1
}
