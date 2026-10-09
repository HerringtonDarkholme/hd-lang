//! `Emit(inst)` (codegen.md §12.1, §12.2): one walk of an instance's
//! generic TIR under its substitution. Every value instruction's result
//! lives in Wasm locals, one per value of its layout (`multi` layouts have
//! several), so structured control needs only empty block types. Calls go
//! to collection's recorded targets; nothing is selected here. A tag or
//! form this walk does not lower yet answers the structured `unsupported`.

use std::collections::HashMap;

use hd_base::{DefId, Hash128, StageResult};
use hd_mono::layout::inline_map_key;
use hd_mono::{CallTarget, ProgramEnv, Target, TargetKind, subst};
use hd_tir::ir::{
    Body, Callee, ChoiceKind, Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, local_flags,
};
use hd_types::{InternPool, Prim, RowId, Ty, TyData, TyList};

use crate::asm::Asm;
use crate::layout::{
    ACTIVE, CANCELLED, DONE, EnumShape, F_CANCEL, F_CHILD, F_FLAGS, F_POLL, F_SAVED, F_STATE, Lay,
    Layouts, M_HASHES, M_KEYS, M_LIVE, M_USED, OptShape, Shape, box_of, cancel_fn, ctx_keys,
    ctx_provs, frame_of, key_id, storage, suspend_base, task_base,
};
use crate::rt::{Helper, KeyOps, OptForm, block_import};
use crate::{Code, GSym, Part, Sym, VT, WTy, unsupported};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctl {
    Plain,
    /// The block whose end starts arm `k` of match instruction `m`.
    Arm(u32, u32),
    Brk(u32),
    Cont(u32),
    /// The cleanup block of `Scope` instruction `s` (codegen.md §12.2,
    /// "The exit ladder"): a branch to it runs the scope's suites.
    Scope(u32),
}

/// A way out of a cleanup scope: each is one rung of its exit ladder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exit {
    Ret,
    Brk(u32),
    Cont(u32),
    /// Cancellation at a suspension point (suspension.md §14.6): it runs
    /// the same suites, then the body returns.
    Cancel,
}

/// A cleanup scope's ladder: the exit-code local and the exits taken.
struct ScopeInfo {
    code: u32,
    exits: Vec<Exit>,
}

/// A suspending body's state machine (suspension.md §14.2, §14.3).
struct Susp {
    /// `$F_f`: the frame.
    frame_ty: WTy,
    /// `$Suspend_L` of the body's result.
    base: WTy,
    frame: u32,
    pc: u32,
    /// Each suspension point's state, numbered in structural order.
    states: HashMap<u32, u32>,
    /// The states inside each instruction that contains a point.
    ranges: HashMap<u32, (u32, u32)>,
    /// The saved Wasm locals, in field order from `F_SAVED` (the second
    /// pass; the first pass learns them).
    saved: Vec<(u32, VT)>,
}

struct Em<'a> {
    lay: Lay<'a>,
    b: &'a Body,
    item: DefId,
    args: TyList,
    calls: &'a HashMap<u32, Target>,
    a: Asm,
    locals: Vec<Option<Vec<u32>>>,
    /// Locals that live in a shared heap cell (a closure captures them and
    /// someone assigns them), each with the index of its first `LocalSet`.
    cells: HashMap<u32, u32>,
    vals: HashMap<u32, (Vec<u32>, Vec<VT>)>,
    ctrl: Vec<Ctl>,
    deciding: Vec<u32>,
    /// Covering providers, innermost last: key trait, its two locals.
    providers: Vec<(DefId, [u32; 2])>,
    /// The instance's own key (a suspending body names its parts).
    key: Hash128,
    /// The function's result values (defaultable in a suspending body).
    results: Vec<VT>,
    /// Locals holding a return value across an exit ladder.
    ret: Option<Vec<u32>>,
    scopes: HashMap<u32, ScopeInfo>,
    /// Each `defer` suite's "registered" flag local.
    defer_flags: HashMap<u32, u32>,
    susp: Option<Susp>,
}

/// The locals a closure captures by shared cell, each with its first
/// `LocalSet` (the declaration, which makes a fresh cell).
fn cell_locals(b: &Body) -> HashMap<u32, u32> {
    let mut cells = HashMap::new();
    for (c, l) in b.cap_local.iter().enumerate() {
        if b.cap_mode.get(c) == Some(&hd_tir::ir::CaptureMode::Shared) {
            cells.insert(l.raw(), u32::MAX);
        }
    }
    for i in 0..b.len() {
        if b.tags[i] == Tag::LocalSet
            && let Some(first) = cells.get_mut(&b.data[i][0])
            && *first == u32::MAX
        {
            *first = u32_of(i);
        }
    }
    cells
}

/// The operator an intrinsic method of the primitives' std
/// implementations computes (`expr.op.std.intrinsic-method`,
/// `expr.eq.std.intrinsic`); `cmp` and `partial_cmp` build an `Ordering`
/// instead.
fn intrinsic_op(method: &str) -> Option<PrimOp> {
    Some(match method {
        "add" => PrimOp::Add,
        "sub" => PrimOp::Sub,
        "mul" => PrimOp::Mul,
        "div" => PrimOp::Div,
        "rem" => PrimOp::Rem,
        "neg" => PrimOp::Neg,
        "bit_and" => PrimOp::BitAnd,
        "bit_or" => PrimOp::BitOr,
        "bit_xor" => PrimOp::BitXor,
        "not" => PrimOp::Not,
        "shl" => PrimOp::Shl,
        "shr" => PrimOp::Shr,
        "eq" => PrimOp::Eq,
        _ => return None,
    })
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("index")
}

/// The integer kind of a scalar type: bits, signed, float.
fn num(pool: &InternPool, t: Ty) -> (u8, bool, bool) {
    let t = match pool.get(t) {
        TyData::Mut(i) => i,
        _ => t,
    };
    match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::I8 => (8, true, false),
            Prim::U8 | Prim::Bool => (8, false, false),
            Prim::I16 => (16, true, false),
            Prim::U16 => (16, false, false),
            Prim::I32 => (32, true, false),
            Prim::U32 | Prim::Usize | Prim::Char => (32, false, false),
            Prim::I64 => (64, true, false),
            Prim::U64 => (64, false, false),
            Prim::F32 => (32, true, true),
            Prim::F64 => (64, true, true),
            _ => (32, true, false),
        },
        _ => (32, false, false),
    }
}

impl Em<'_> {
    fn pool(&self) -> &InternPool {
        self.lay.pool
    }
    fn env(&self) -> &dyn ProgramEnv {
        self.lay.env
    }
    fn sub(&self, t: Ty) -> Ty {
        subst(self.lay.pool, self.lay.env, self.item, self.args, t)
    }
    fn ty_of(&self, r: u32) -> Ty {
        if Ref(r).as_inst().is_some() {
            self.sub(self.b.ty[r as usize])
        } else {
            self.sub(
                self.b
                    .consts
                    .get((r & !Ref::CONST_BIT) as usize)
                    .map_or(Ty::VOID, |c| c.0),
            )
        }
    }
    fn vts(&self, t: Ty) -> StageResult<Vec<VT>> {
        self.lay.vts(t)
    }
    fn depth_of(&self, want: Ctl) -> StageResult<u32> {
        match self.ctrl.iter().rposition(|c| *c == want) {
            Some(p) => Ok(u32_of(self.ctrl.len() - 1 - p)),
            None => unsupported("a branch to a label outside its construct"),
        }
    }
    fn open(&mut self, c: Ctl) {
        self.ctrl.push(c);
    }

    /// The cell type of a shared local: one mutable field per component.
    fn cell_ty(&self, l: u32) -> StageResult<WTy> {
        let fields = self.vts(self.sub(self.b.local_ty[l as usize]))?;
        Ok(WTy::Struct {
            fields,
            sup: None,
            open: false,
        })
    }

    /// The value types a body local is held in: its components, or one
    /// reference to its cell.
    fn local_vts(&self, l: u32) -> StageResult<Vec<VT>> {
        if self.cells.contains_key(&l) {
            return Ok(vec![VT::rn(self.cell_ty(l)?)]);
        }
        self.vts(self.sub(self.b.local_ty[l as usize]))
    }

    /// The Wasm locals of a body local.
    fn local(&mut self, l: u32) -> StageResult<Vec<u32>> {
        if let Some(Some(v)) = self.locals.get(l as usize) {
            return Ok(v.clone());
        }
        let vs: Vec<u32> = self
            .local_vts(l)?
            .into_iter()
            .map(|v| self.a.local(v))
            .collect();
        if self.locals.len() <= l as usize {
            self.locals.resize(l as usize + 1, None);
        }
        self.locals[l as usize] = Some(vs.clone());
        Ok(vs)
    }

    /// Stores the temporaries `tmp` (the local's components) into local
    /// `l`'s cell. `fresh` makes a new cell, as a declaration does; any other
    /// store writes the existing cell, making one first if it has none yet.
    fn cell_store(&mut self, l: u32, tmp: &[u32], fresh: bool) -> StageResult<()> {
        let cell = self.cell_ty(l)?;
        let cl = self.local(l)?[0];
        let make = |em: &mut Self| {
            for t in tmp {
                em.a.get(*t);
            }
            em.a.struct_new(&cell);
            em.a.set(cl);
        };
        if fresh {
            make(self);
            return Ok(());
        }
        self.a.get(cl);
        self.a.s().ref_is_null();
        self.a.if_();
        make(self);
        self.a.else_();
        for (k, t) in tmp.iter().enumerate() {
            self.a.get(cl);
            self.a.get(*t);
            self.a.struct_set(&cell, u32_of(k));
        }
        self.a.end();
        Ok(())
    }

    /// Pushes component `k` of a value, converted to `want`.
    fn comp(&mut self, r: u32, k: usize, want: &VT) -> StageResult<()> {
        if r == NONE {
            self.a.zero(want);
            return Ok(());
        }
        if Ref(r).as_inst().is_none() {
            let Some(&(t, bits)) = self.b.consts.get((r & !Ref::CONST_BIT) as usize) else {
                return unsupported("a constant outside the body's column");
            };
            let t = self.sub(t);
            match self.lay.shape(t)? {
                Shape::Str => {
                    let Some(s) = self.b.strings.get(usize::try_from(bits).unwrap_or(0)) else {
                        return unsupported("a string constant outside the table");
                    };
                    if k == 0 {
                        self.a.call(Sym::Helper(Helper::Lit(s.as_bytes().to_vec())));
                    } else {
                        self.a.i64(i64::try_from(s.len()).unwrap_or(0) << 32);
                    }
                }
                Shape::Scalar(VT::I64) => self.a.i64(bits.cast_signed()),
                Shape::Scalar(VT::F64) => {
                    self.a.s().f64_const(f64::from_bits(bits).into());
                }
                Shape::Scalar(VT::F32) => {
                    let b32 = u32::try_from(bits & 0xffff_ffff).expect("32 bits");
                    self.a.s().f32_const(f32::from_bits(b32).into());
                }
                Shape::Scalar(_) | Shape::Enum(EnumShape { boxed: None, .. }) => {
                    let low = u32::try_from(bits & 0xffff_ffff).expect("32 bits");
                    self.a.i32(low.cast_signed());
                }
                Shape::Void => {}
                _ => return unsupported("a constant of this type"),
            }
            return Ok(());
        }
        let Some((ls, vs)) = self.vals.get(&r).cloned() else {
            return unsupported(format!("a value used before emission (%{r})"));
        };
        let (Some(l), Some(have)) = (ls.get(k), vs.get(k)) else {
            return unsupported("a value with fewer components than its use");
        };
        self.a.get(*l);
        self.a.conv(have, want);
        Ok(())
    }

    /// Pushes every component of a value, converted to `want`.
    fn load_as(&mut self, r: u32, want: &[VT]) -> StageResult<()> {
        for (k, w) in want.iter().enumerate() {
            self.comp(r, k, w)?;
        }
        Ok(())
    }
    fn load(&mut self, r: u32) -> StageResult<Vec<VT>> {
        let want = self.vts(self.ty_of(r))?;
        self.load_as(r, &want)?;
        Ok(want)
    }

    /// Fresh result locals of instruction `i`.
    fn result(&mut self, i: u32) -> StageResult<Vec<u32>> {
        if let Some((ls, _)) = self.vals.get(&i) {
            return Ok(ls.clone());
        }
        let vs = self.vts(self.sub(self.b.ty[i as usize]))?;
        let ls: Vec<u32> = vs.iter().map(|v| self.a.local(v.clone())).collect();
        self.vals.insert(i, (ls.clone(), vs));
        Ok(ls)
    }
    /// Pops the instruction's values (pushed in its layout) into its locals.
    fn store(&mut self, i: u32) -> StageResult<()> {
        let ls = self.result(i)?;
        for l in ls.iter().rev() {
            self.a.set(*l);
        }
        Ok(())
    }
    /// Stores the values on the stack, in `have`, as `i`'s result.
    fn store_from(&mut self, i: u32, have: &[VT]) -> StageResult<()> {
        let tmp: Vec<u32> = have.iter().map(|v| self.a.local(v.clone())).collect();
        for l in tmp.iter().rev() {
            self.a.set(*l);
        }
        let ls = self.result(i)?;
        let want = self.vals[&i].1.clone();
        for ((t, h), (l, w)) in tmp.iter().zip(have).zip(ls.iter().zip(&want)) {
            self.a.get(*t);
            self.a.conv(h, w);
            self.a.set(*l);
        }
        Ok(())
    }

    fn rec(&self, at: u32) -> Vec<u32> {
        self.b.record(at).to_vec()
    }

    /// A block instruction: its list, then its tail into `dest`.
    fn block_into(&mut self, blk: u32, dest: Option<u32>) -> StageResult<()> {
        if blk == NONE {
            return Ok(());
        }
        if self.b.tags[blk as usize] != Tag::Block {
            return unsupported("a block operand that is not a block");
        }
        let [list, tail] = self.b.data[blk as usize];
        let items: Vec<u32> = self
            .rec(list)
            .into_iter()
            .filter(|i| self.b.tags[*i as usize] != Tag::Block)
            .collect();
        if self.range(blk).is_some() {
            self.resume_list(&items)?;
        } else {
            for i in items {
                self.inst(i)?;
            }
        }
        if tail != NONE
            && let Some(d) = dest
        {
            let want = self.vts(self.sub(self.b.ty[d as usize]))?;
            if !want.is_empty() {
                self.load_as(tail, &want)?;
                self.store(d)?;
            }
        }
        Ok(())
    }

    fn panic(&mut self, msg: &str) {
        self.a.call(Sym::Helper(Helper::Panic(msg.to_owned())));
        self.a.s().unreachable();
    }

    fn inst(&mut self, i: u32) -> StageResult<()> {
        let [a, bw] = self.b.data[i as usize];
        let ty = self.sub(self.b.ty[i as usize]);
        match self.b.tags[i as usize] {
            Tag::Block => {
                self.a.block();
                self.open(Ctl::Plain);
                self.block_into(i, Some(i))?;
                self.ctrl.pop();
                self.a.end();
            }
            Tag::LocalGet => {
                let ls = self.local(a)?;
                if self.cells.contains_key(&a) {
                    let cell = self.cell_ty(a)?;
                    for k in 0..self.vts(self.sub(self.b.local_ty[a as usize]))?.len() {
                        self.a.get(ls[0]);
                        self.a.struct_get(&cell, u32_of(k));
                    }
                } else {
                    for l in &ls {
                        self.a.get(*l);
                    }
                }
                self.store(i)?;
            }
            Tag::LocalSet => {
                let ls = self.local(a)?;
                let t = self.sub(self.b.local_ty[a as usize]);
                let want = self.vts(t)?;
                self.load_as(bw, &want)?;
                if let Some(first) = self.cells.get(&a).copied() {
                    let tmp: Vec<u32> = want.iter().map(|v| self.a.local(v.clone())).collect();
                    for l in tmp.iter().rev() {
                        self.a.set(*l);
                    }
                    self.cell_store(a, &tmp, first == i)?;
                } else {
                    for l in ls.iter().rev() {
                        self.a.set(*l);
                    }
                }
            }
            Tag::Prim => self.prim(i, a, bw)?,
            Tag::And | Tag::Or => {
                if self.range(i).is_some() {
                    return unsupported("a suspension point in the right operand of `and`/`or`");
                }
                let r = self.result(i)?;
                self.comp(a, 0, &VT::I32)?;
                if self.b.tags[i as usize] == Tag::Or {
                    self.a.s().i32_eqz();
                }
                self.a.if_();
                self.open(Ctl::Plain);
                self.block_into(bw, Some(i))?;
                self.ctrl.pop();
                self.a.else_();
                self.a.i32(i32::from(self.b.tags[i as usize] == Tag::Or));
                self.a.set(r[0]);
                self.a.end();
            }
            Tag::Call => self.call(i, a, bw, ty)?,
            Tag::DefaultCall => self.default_call(i, bw, ty)?,
            Tag::CallValue => {
                let Shape::Fn { base, code } = self.lay.shape(self.ty_of(a))? else {
                    return unsupported("a call of a value that is not a closure");
                };
                let WTy::Func(ps, rs) = &code else {
                    return unsupported("a closure code type");
                };
                let f = self.a.local(VT::r(base.clone()));
                self.comp(a, 0, &VT::r(base.clone()))?;
                self.a.set(f);
                self.a.get(f);
                let args = self.rec(bw);
                let mut k = 1;
                for r in args {
                    let n = self.vts(self.ty_of(r))?.len();
                    let want: Vec<VT> = ps[k..k + n].to_vec();
                    self.load_as(r, &want)?;
                    k += n;
                }
                let callee = self.ty_of(a);
                self.push_ctx(callee)?;
                self.a.get(f);
                self.a.struct_get(&base, 0);
                self.a.call_ref(&code);
                self.store_from(i, &rs.clone())?;
            }
            Tag::Interp => {
                let parts = self.rec(bw);
                self.concat(i, &parts)?;
            }
            Tag::Intrinsic => self.intrinsic(i, a, bw, ty)?,
            Tag::Coerce => self.coerce(i, a, bw, ty)?,
            Tag::NewData => {
                let Shape::Data { ty: st, fields } = self.lay.shape(ty)? else {
                    return unsupported("a data value without a struct layout");
                };
                for (r, (_, vs)) in self.rec(bw).into_iter().zip(fields) {
                    self.load_as(r, &vs)?;
                }
                self.a.struct_new(&st);
                self.store(i)?;
            }
            Tag::CopyData => {
                let Shape::Data { ty: st, fields } = self.lay.shape(ty)? else {
                    return unsupported("a copied value without a struct layout");
                };
                // Replacements are `(field, value)` pairs; every other field
                // is read from the source (codegen.md `CopyData`).
                let words = self.rec(bw);
                let repl: Vec<(u32, u32)> = words.chunks(2).map(|w| (w[0], w[1])).collect();
                for (idx, (start, vs)) in fields.iter().enumerate() {
                    if let Some((_, v)) = repl.iter().find(|(f, _)| *f as usize == idx) {
                        self.load_as(*v, vs)?;
                    } else {
                        for k in 0..vs.len() {
                            self.comp(a, 0, &VT::r(st.clone()))?;
                            self.a.struct_get(&st, start + u32_of(k));
                        }
                    }
                }
                self.a.struct_new(&st);
                self.store(i)?;
            }
            Tag::Field | Tag::FieldSet => {
                let base_t = self.ty_of(a);
                let Shape::Data { ty: st, fields } = self.lay.shape(base_t)? else {
                    return unsupported("a field of a value without a struct layout");
                };
                let (idx, val) = if self.b.tags[i as usize] == Tag::Field {
                    (bw, None)
                } else {
                    let r = self.rec(bw);
                    (r[0], Some(r[1]))
                };
                let Some((start, vs)) = fields.get(idx as usize).cloned() else {
                    return unsupported("a field index outside the struct");
                };
                match val {
                    None => {
                        for k in 0..vs.len() {
                            self.comp(a, 0, &VT::r(st.clone()))?;
                            self.a.struct_get(&st, start + u32_of(k));
                        }
                        self.store_from(i, &vs)?;
                    }
                    Some(v) => {
                        for (k, w) in vs.iter().enumerate() {
                            self.comp(a, 0, &VT::r(st.clone()))?;
                            self.comp(v, k, w)?;
                            self.a.struct_set(&st, start + u32_of(k));
                        }
                    }
                }
            }
            Tag::NewTuple => {
                let Shape::Tuple { elems, boxed } = self.lay.shape(ty)? else {
                    return unsupported("a tuple without a tuple layout");
                };
                for (r, vs) in self.rec(bw).into_iter().zip(elems) {
                    self.load_as(r, &vs)?;
                }
                if let Some(b) = boxed {
                    self.a.struct_new(&b);
                }
                self.store(i)?;
            }
            Tag::TupleGet => {
                let Shape::Tuple { elems, boxed } = self.lay.shape(self.ty_of(a))? else {
                    return unsupported("a tuple read of a non-tuple");
                };
                let start: usize = elems[..bw as usize].iter().map(Vec::len).sum();
                let vs = elems[bw as usize].clone();
                for (k, v) in vs.iter().enumerate() {
                    match &boxed {
                        Some(b) => {
                            self.comp(a, 0, &VT::r(b.clone()))?;
                            self.a.struct_get(b, u32_of(start + k));
                        }
                        None => self.comp(a, start + k, v)?,
                    }
                }
                self.store_from(i, &vs)?;
            }
            Tag::NewVariant => self.new_variant(i, a, bw, ty)?,
            Tag::NewList => {
                let Shape::List { elem, ty: lt } = self.lay.shape(ty)? else {
                    return unsupported("a list literal without a list layout");
                };
                let items = self.rec(bw);
                self.a.i32(i32::try_from(items.len()).unwrap_or(0));
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    for r in &items {
                        self.comp(*r, k, v)?;
                    }
                    self.a.array_new_fixed(&WTy::Array(st), u32_of(items.len()));
                }
                self.a.struct_new(&lt);
                self.store(i)?;
            }
            Tag::NewMap => self.map_literal(i, &self.rec(bw), ty)?,
            Tag::Closure => self.closure(i, bw, ty)?,
            // No captures: a bound reference is a `Closure` (§13.11).
            Tag::ItemRef => self.closure_value(i, 0..0, ty)?,
            Tag::ProviderGet => {
                let key = self.rec(a).first().copied().unwrap_or(NONE);
                let key_t = Ty(key);
                let TyData::TraitValue { def, .. } = self.pool().get(key_t) else {
                    return unsupported("a provider key that is not a trait");
                };
                let Some((_, ls)) = self.providers.iter().rev().find(|p| p.0 == def) else {
                    return unsupported("a provider the function's row does not pass");
                };
                let ls = *ls;
                self.a.get(ls[0]);
                self.a.get(ls[1]);
                self.store(i)?;
            }
            Tag::If => {
                let r = self.rec(bw);
                self.result(i)?;
                self.comp(a, 0, &VT::I32)?;
                if let Some((lo, hi)) = self.range(r[0]) {
                    // Resuming into the then block takes it; into the
                    // else block, not (suspension.md §14.2). A `select`,
                    // since structured blocks here carry no values.
                    self.in_range(lo, hi);
                    let pc = self.pc();
                    self.a.get(pc);
                    self.a.s().i32_eqz();
                    self.a.s().select();
                }
                self.a.if_();
                self.open(Ctl::Plain);
                self.block_into(r[0], Some(i))?;
                if r.get(1).is_some_and(|e| *e != NONE) {
                    self.a.else_();
                    self.block_into(r[1], Some(i))?;
                }
                self.ctrl.pop();
                self.a.end();
            }
            Tag::Loop => {
                self.result(i)?;
                self.a.block();
                self.open(Ctl::Brk(i));
                self.a.loop_();
                self.open(Ctl::Cont(i));
                self.block_into(a, None)?;
                self.a.br(0);
                self.ctrl.pop();
                self.a.end();
                self.ctrl.pop();
                self.a.end();
            }
            Tag::Break | Tag::Continue => {
                let Some(&target) = self.b.label_inst.get(a as usize) else {
                    return unsupported("a break with an unknown label");
                };
                if self.b.tags[i as usize] == Tag::Break {
                    if bw != NONE {
                        let want = self.vts(self.sub(self.b.ty[target as usize]))?;
                        if !want.is_empty() {
                            self.load_as(bw, &want)?;
                            self.store(target)?;
                        }
                    }
                    self.exit(Exit::Brk(target))?;
                } else {
                    self.exit(Exit::Cont(target))?;
                }
            }
            Tag::Return => self.ret(a)?,
            Tag::GlobalGet => {
                let h = self.global_hash(a);
                let vs = self.vts(ty)?;
                let dv: Vec<VT> = vs.iter().map(VT::dflt).collect();
                for (k, v) in dv.iter().enumerate() {
                    self.a.global_get(GSym::Binding(h, u32_of(k), v.clone()));
                }
                self.store_from(i, &dv)?;
            }
            Tag::GlobalSet => {
                let h = self.global_hash(a);
                let vs = self.vts(self.ty_of(bw))?;
                for (k, v) in vs.iter().enumerate() {
                    self.comp(bw, k, v)?;
                    self.a.global_set(GSym::Binding(h, u32_of(k), v.dflt()));
                }
            }
            Tag::Unreachable => {
                self.a.s().unreachable();
            }
            Tag::Scope => {
                if !self.rec(bw).is_empty() {
                    return self.scope(i, a, bw);
                }
                self.result(i)?;
                self.a.block();
                self.open(Ctl::Plain);
                self.block_into(a, Some(i))?;
                self.ctrl.pop();
                self.a.end();
            }
            Tag::Match => {
                let blocks = self.rec(bw);
                let n = blocks.len() - 1;
                self.result(i)?;
                self.a.block();
                self.open(Ctl::Brk(i));
                for k in (0..n).rev() {
                    self.a.block();
                    self.open(Ctl::Arm(i, u32_of(k)));
                }
                if self.range(blocks[0]).is_some() {
                    return unsupported("a suspension point in a match decision (a guard)");
                }
                for (k, arm) in blocks[1..].iter().enumerate() {
                    if let Some((lo, hi)) = self.range(*arm) {
                        self.in_range(lo, hi);
                        let d = self.depth_of(Ctl::Arm(i, u32_of(k)))?;
                        self.a.br_if(d);
                    }
                }
                self.deciding.push(i);
                self.block_into(blocks[0], None)?;
                self.deciding.pop();
                self.a.s().unreachable();
                for k in 0..n {
                    self.ctrl.pop();
                    self.a.end();
                    self.block_into(blocks[k + 1], Some(i))?;
                    let d = self.depth_of(Ctl::Brk(i))?;
                    self.a.br(d);
                }
                self.ctrl.pop();
                self.a.end();
            }
            Tag::ToArm => {
                let Some(&m) = self.deciding.last() else {
                    return unsupported("an arm jump outside a match decision");
                };
                let d = self.depth_of(Ctl::Arm(m, a))?;
                self.a.br(d);
            }
            Tag::With => self.with(i, a, bw)?,
            Tag::Guard => {
                if self.range(i).is_some() {
                    return unsupported("a suspension point in a match guard");
                }
                let r = self.rec(bw);
                self.block_value(a)?;
                self.a.if_();
                self.open(Ctl::Plain);
                self.block_into(r[0], None)?;
                self.a.else_();
                self.block_into(r[1], None)?;
                self.ctrl.pop();
                self.a.end();
            }
            Tag::SwitchTag => {
                let r = self.rec(bw);
                self.result(i)?;
                self.tag_of(a)?;
                self.a.i32(r[0].cast_signed());
                self.a.s().i32_eq();
                self.a.if_();
                self.open(Ctl::Plain);
                self.block_into(r[1], Some(i))?;
                self.a.else_();
                self.block_into(r[2], Some(i))?;
                self.ctrl.pop();
                self.a.end();
            }
            Tag::SwitchInt | Tag::SwitchChar => {
                let r = self.rec(bw);
                self.result(i)?;
                self.comp(a, 0, &VT::I32)?;
                self.a.i32(r[0].cast_signed());
                self.a.s().i32_eq();
                self.a.if_();
                self.open(Ctl::Plain);
                self.block_into(r[1], Some(i))?;
                self.a.else_();
                self.block_into(r[2], Some(i))?;
                self.ctrl.pop();
                self.a.end();
            }
            Tag::Payload => {
                let r = self.rec(bw);
                self.payload(i, a, r[0], r[1])?;
            }
            Tag::Unwrap => {
                let Shape::Opt(o, inner) = self.lay.shape(self.ty_of(a))? else {
                    return unsupported("an unwrap of a non-optional");
                };
                self.unwrap(a, &o, &inner)?;
                self.store_from(i, &inner)?;
            }
            Tag::Hook => {}
            Tag::Defer => {
                let Some(&f) = self.defer_flags.get(&a) else {
                    return unsupported("a `Defer` outside its scope");
                };
                self.a.i32(1);
                self.a.set(f);
            }
            Tag::Await | Tag::AwaitValue | Tag::AwaitAll => self.point(i, a, bw)?,
            Tag::AwaitRace => {
                return unsupported(
                    "emission of `AwaitRace` (the checker emits `race!` as std hd)",
                );
            }
            other => return unsupported(format!("emission of TIR tag {}", other.name())),
        }
        Ok(())
    }

    /// A block's tail value on the stack (a guard's condition).
    fn block_value(&mut self, blk: u32) -> StageResult<()> {
        let [list, tail] = self.b.data[blk as usize];
        for i in self.rec(list) {
            if self.b.tags[i as usize] != Tag::Block {
                self.inst(i)?;
            }
        }
        self.comp(tail, 0, &VT::I32)
    }

    /// Pushes the tag of an enum or optional value.
    fn tag_of(&mut self, v: u32) -> StageResult<()> {
        match self.lay.shape(self.ty_of(v))? {
            Shape::Enum(e) => match e.boxed {
                Some(b) => {
                    self.comp(v, 0, &VT::r(b.clone()))?;
                    self.a.struct_get(&b, 0);
                }
                None => self.comp(v, 0, &VT::I32)?,
            },
            Shape::Opt(o, inner) => match o {
                OptShape::NullRef(r) => {
                    self.comp(v, 0, &r.dflt())?;
                    self.a.s().ref_is_null().i32_eqz();
                }
                OptShape::NullStr => {
                    self.comp(v, 0, &VT::rn(WTy::Bytes))?;
                    self.a.s().ref_is_null().i32_eqz();
                }
                OptShape::Tagged(_) => self.comp(v, 0, &VT::I32)?,
                OptShape::Boxed(b, _) => {
                    let _ = inner;
                    self.comp(v, 0, &VT::rn(b))?;
                    self.a.s().ref_is_null().i32_eqz();
                }
            },
            _ => return unsupported("a tag switch on a value without tags"),
        }
        Ok(())
    }

    fn unwrap(&mut self, v: u32, o: &OptShape, inner: &[VT]) -> StageResult<()> {
        match o {
            OptShape::NullRef(r) => {
                self.comp(v, 0, &r.dflt())?;
                self.a.conv(&r.dflt(), &inner[0]);
            }
            OptShape::NullStr => {
                self.comp(v, 0, &VT::rn(WTy::Bytes))?;
                self.a.s().ref_as_non_null();
                self.comp(v, 1, &VT::I64)?;
            }
            OptShape::Tagged(dv) => {
                for (k, (d, w)) in dv.iter().zip(inner).enumerate() {
                    self.comp(v, k + 1, d)?;
                    self.a.conv(d, w);
                }
            }
            OptShape::Boxed(b, dv) => {
                for (k, (d, w)) in dv.iter().zip(inner).enumerate() {
                    self.comp(v, 0, &VT::rn(b.clone()))?;
                    self.a.struct_get(b, u32_of(k));
                    self.a.conv(d, w);
                }
            }
        }
        Ok(())
    }

    fn payload(&mut self, i: u32, v: u32, variant: u32, field: u32) -> StageResult<()> {
        let shape = self.lay.shape(self.ty_of(v))?;
        match shape {
            Shape::Enum(e) => {
                let Some((slots, vs)) = e
                    .fields
                    .get(variant as usize)
                    .and_then(|f| f.get(field as usize))
                    .cloned()
                else {
                    return unsupported("a payload outside the variant");
                };
                if let (Some(b), Some(Some(sub))) = (&e.boxed, e.subtypes.get(variant as usize)) {
                    // A subtype box: the variant's subtype holds the
                    // payload's exact values after the tag.
                    for s in &slots {
                        self.comp(v, 0, &VT::r(b.clone()))?;
                        self.a.ref_cast(sub, false);
                        self.a.struct_get(sub, u32_of(*s + 1));
                    }
                    return self.store_from(i, &vs);
                }
                for (s, want) in slots.iter().zip(&vs) {
                    let st = e.slots[*s].clone();
                    match &e.boxed {
                        Some(b) => {
                            self.comp(v, 0, &VT::r(b.clone()))?;
                            self.a.struct_get(b, u32_of(*s + 1));
                        }
                        None => self.comp(v, *s + 1, &st)?,
                    }
                    self.a.conv(&st, want);
                }
                self.store_from(i, &vs)
            }
            Shape::Opt(o, inner) => {
                self.unwrap(v, &o, &inner)?;
                self.store_from(i, &inner)
            }
            _ => unsupported("a payload of a value without variants"),
        }
    }

    fn new_variant(&mut self, i: u32, variant: u32, vals: u32, ty: Ty) -> StageResult<()> {
        let args = self.rec(vals);
        match self.lay.shape(ty)? {
            Shape::Enum(e) => {
                self.a.i32(variant.cast_signed());
                let fields = e.fields.get(variant as usize).cloned().unwrap_or_default();
                if let (Some(base), Some(sub)) = (&e.boxed, e.subtypes.get(variant as usize)) {
                    // A subtype box: the tag, then the payload's exact
                    // values; a payloadless variant is the base alone.
                    match sub {
                        Some(sub) => {
                            for ((_, vs), r) in fields.iter().zip(&args) {
                                for (k, x) in vs.iter().enumerate() {
                                    self.comp(*r, k, x)?;
                                }
                            }
                            self.a.struct_new(sub);
                        }
                        None => self.a.struct_new(base),
                    }
                    return self.store(i);
                }
                for (s, st) in e.slots.iter().enumerate() {
                    let src = fields.iter().zip(&args).find_map(|((slots, vs), r)| {
                        slots
                            .iter()
                            .position(|x| *x == s)
                            .map(|k| (*r, k, vs[k].clone()))
                    });
                    match src {
                        Some((r, k, exact)) => {
                            self.comp(r, k, &exact)?;
                            self.a.conv(&exact, st);
                        }
                        None => self.a.zero(st),
                    }
                }
                if let Some(b) = &e.boxed {
                    self.a.struct_new(b);
                }
                self.store(i)
            }
            Shape::Opt(o, inner) => {
                let some = variant == 1;
                match &o {
                    OptShape::NullRef(r) => {
                        if some {
                            self.comp(args[0], 0, r)?;
                        } else {
                            self.a.zero(&r.dflt());
                        }
                    }
                    OptShape::NullStr => {
                        if some {
                            self.load_as(args[0], &inner)?;
                        } else {
                            self.a.ref_null(&WTy::Bytes);
                            self.a.i64(0);
                        }
                    }
                    OptShape::Tagged(dv) | OptShape::Boxed(_, dv) => {
                        if matches!(o, OptShape::Tagged(_)) {
                            self.a.i32(i32::from(some));
                        }
                        if some || matches!(o, OptShape::Tagged(_)) {
                            for (k, d) in dv.iter().enumerate() {
                                if some {
                                    self.comp(args[0], k, &inner[k])?;
                                } else {
                                    self.a.zero(d);
                                }
                            }
                        }
                        if let OptShape::Boxed(b, _) = &o {
                            if some {
                                self.a.struct_new(b);
                            } else {
                                self.a.ref_null(b);
                            }
                        }
                    }
                }
                self.store(i)
            }
            _ => unsupported("a variant of a type without variants"),
        }
    }

    /// Joins string values into one exact-size array, viewed from 0.
    fn concat(&mut self, i: u32, parts: &[u32]) -> StageResult<()> {
        let sv = [VT::r(WTy::Bytes), VT::I64];
        let mut ls = Vec::new();
        for p in parts {
            let b = self.a.local(sv[0].clone());
            let s = self.a.local(VT::I64);
            self.load_as(*p, &sv)?;
            self.a.set(s);
            self.a.set(b);
            ls.push((b, s));
        }
        let (total, off, arr) = (
            self.a.local(VT::I32),
            self.a.local(VT::I32),
            self.a.local(sv[0].clone()),
        );
        self.a.i32(0);
        self.a.set(total);
        self.a.i32(0);
        self.a.set(off);
        for (_, s) in &ls {
            self.a.get(total);
            self.a.get(*s);
            self.a.i64(32);
            self.a.s().i64_shr_u().i32_wrap_i64().i32_add();
            self.a.set(total);
        }
        self.a.get(total);
        self.a.array_new_default(&WTy::Bytes);
        self.a.set(arr);
        for (b, s) in &ls {
            self.a.get(arr);
            self.a.get(off);
            self.a.get(*b);
            self.a.get(*s);
            self.a.s().i32_wrap_i64();
            self.a.get(*s);
            self.a.i64(32);
            self.a.s().i64_shr_u().i32_wrap_i64();
            self.a.array_copy(&WTy::Bytes, &WTy::Bytes);
            self.a.get(off);
            self.a.get(*s);
            self.a.i64(32);
            self.a.s().i64_shr_u().i32_wrap_i64().i32_add();
            self.a.set(off);
        }
        self.a.get(arr);
        self.a.get(total);
        self.a.s().i64_extend_i32_u();
        self.a.i64(32);
        self.a.s().i64_shl();
        self.store(i)
    }

    fn prim(&mut self, i: u32, op: u32, rec: u32) -> StageResult<()> {
        let Some(op) = PrimOp::from_u32(op) else {
            return unsupported("an unknown operator");
        };
        let ops = self.rec(rec);
        let ty = self.sub(self.b.ty[i as usize]);
        self.prim_value(op, &ops, ty)?;
        self.store(i)
    }

    /// Pushes the value of primitive operator `op` on the operands `ops`,
    /// whose result has type `ty`. This is the one lowering of the
    /// operators on numbers, `char` and `bool`: the `Prim` instruction and
    /// the intrinsic methods of the std operator and comparison traits
    /// (`expr.op.std.intrinsic-method`) both come here.
    fn prim_value(&mut self, op: PrimOp, ops: &[u32], ty: Ty) -> StageResult<()> {
        let t0 = ops.first().map_or(Ty::I32, |r| self.ty_of(*r));
        let (bits, signed, float) = num(self.pool(), t0);
        let vt0 = self.vts(t0)?;
        if !matches!(vt0.as_slice(), [VT::I32 | VT::I64 | VT::F32 | VT::F64]) {
            return unsupported(format!("the operator {op:?} on this layout"));
        }
        let wide = vt0[0] == VT::I64;
        if float {
            return self.float_value(op, ops, &vt0[0]);
        }
        let s = |em: &mut Self| -> StageResult<()> {
            for r in ops {
                em.comp(*r, 0, &vt0[0])?;
            }
            Ok(())
        };
        // A shift's count is any unsigned type (`expr.shift.count-unsigned`),
        // so it is brought to the shifted value's width.
        let shift = |em: &mut Self| -> StageResult<()> {
            em.comp(ops[0], 0, &vt0[0])?;
            let count = em.vts(em.ty_of(ops[1]))?;
            let Some(cv) = count.first() else {
                return unsupported("a shift count without a value");
            };
            em.comp(ops[1], 0, cv)?;
            match (cv, wide) {
                (VT::I32, true) => {
                    em.a.s().i64_extend_i32_u();
                }
                (VT::I64, false) => {
                    em.a.s().i32_wrap_i64();
                }
                _ => {}
            }
            Ok(())
        };
        match op {
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem | PrimOp::Neg => {
                self.checked(op, ops, bits, signed, wide)?;
            }
            PrimOp::Eq | PrimOp::Ne | PrimOp::Lt | PrimOp::Le | PrimOp::Gt | PrimOp::Ge => {
                s(self)?;
                let mut x = self.a.s();
                match (op, wide, signed) {
                    (PrimOp::Eq, false, _) => x.i32_eq(),
                    (PrimOp::Ne, false, _) => x.i32_ne(),
                    (PrimOp::Lt, false, true) => x.i32_lt_s(),
                    (PrimOp::Lt, false, false) => x.i32_lt_u(),
                    (PrimOp::Le, false, true) => x.i32_le_s(),
                    (PrimOp::Le, false, false) => x.i32_le_u(),
                    (PrimOp::Gt, false, true) => x.i32_gt_s(),
                    (PrimOp::Gt, false, false) => x.i32_gt_u(),
                    (PrimOp::Ge, false, true) => x.i32_ge_s(),
                    (PrimOp::Ge, false, false) => x.i32_ge_u(),
                    (PrimOp::Eq, true, _) => x.i64_eq(),
                    (PrimOp::Ne, true, _) => x.i64_ne(),
                    (PrimOp::Lt, true, true) => x.i64_lt_s(),
                    (PrimOp::Lt, true, false) => x.i64_lt_u(),
                    (PrimOp::Le, true, true) => x.i64_le_s(),
                    (PrimOp::Le, true, false) => x.i64_le_u(),
                    (PrimOp::Gt, true, true) => x.i64_gt_s(),
                    (PrimOp::Gt, true, false) => x.i64_gt_u(),
                    (PrimOp::Ge, true, true) => x.i64_ge_s(),
                    _ => x.i64_ge_u(),
                };
            }
            PrimOp::And | PrimOp::BitAnd => {
                s(self)?;
                if wide {
                    self.a.s().i64_and();
                } else {
                    self.a.s().i32_and();
                }
            }
            PrimOp::Or | PrimOp::BitOr => {
                s(self)?;
                if wide {
                    self.a.s().i64_or();
                } else {
                    self.a.s().i32_or();
                }
            }
            PrimOp::BitXor => {
                s(self)?;
                if wide {
                    self.a.s().i64_xor();
                } else {
                    self.a.s().i32_xor();
                }
            }
            PrimOp::Shl => {
                shift(self)?;
                if wide {
                    self.a.s().i64_shl();
                } else {
                    self.a.s().i32_shl();
                }
            }
            PrimOp::Shr => {
                shift(self)?;
                let mut x = self.a.s();
                match (wide, signed) {
                    (false, true) => x.i32_shr_s(),
                    (false, false) => x.i32_shr_u(),
                    (true, true) => x.i64_shr_s(),
                    (true, false) => x.i64_shr_u(),
                };
            }
            PrimOp::Not => {
                s(self)?;
                if self.pool().get(t0) == TyData::Prim(Prim::Bool) {
                    self.a.s().i32_eqz();
                } else if wide {
                    self.a.i64(-1);
                    self.a.s().i64_xor();
                } else {
                    self.a.i32(-1);
                    self.a.s().i32_xor();
                }
            }
            PrimOp::Conv => {
                s(self)?;
                let to = self.vts(ty)?;
                let (_, _, tfloat) = num(self.pool(), ty);
                let mut x = self.a.s();
                match (
                    vt0[0].clone(),
                    to.first().cloned().unwrap_or(VT::I32),
                    tfloat,
                ) {
                    (VT::I32, VT::I64, _) => {
                        if signed {
                            x.i64_extend_i32_s()
                        } else {
                            x.i64_extend_i32_u()
                        };
                    }
                    (VT::I64, VT::I32, _) => {
                        x.i32_wrap_i64();
                    }
                    (VT::I32, VT::F64, _) => {
                        if signed {
                            x.f64_convert_i32_s()
                        } else {
                            x.f64_convert_i32_u()
                        };
                    }
                    (VT::I64, VT::F64, _) => {
                        if signed {
                            x.f64_convert_i64_s()
                        } else {
                            x.f64_convert_i64_u()
                        };
                    }
                    (a, b, _) if a == b => {}
                    _ => return unsupported("this numeric conversion"),
                }
            }
        }
        Ok(())
    }

    fn float_value(&mut self, op: PrimOp, ops: &[u32], vt: &VT) -> StageResult<()> {
        if *vt != VT::F64 {
            return unsupported("f32 arithmetic");
        }
        if op == PrimOp::Neg {
            self.comp(ops[0], 0, vt)?;
            self.a.s().f64_neg();
            return Ok(());
        }
        for r in ops {
            self.comp(*r, 0, vt)?;
        }
        let mut x = self.a.s();
        match op {
            PrimOp::Add => x.f64_add(),
            PrimOp::Sub => x.f64_sub(),
            PrimOp::Mul => x.f64_mul(),
            PrimOp::Div => x.f64_div(),
            PrimOp::Eq => x.f64_eq(),
            PrimOp::Ne => x.f64_ne(),
            PrimOp::Lt => x.f64_lt(),
            PrimOp::Le => x.f64_le(),
            PrimOp::Gt => x.f64_gt(),
            PrimOp::Ge => x.f64_ge(),
            _ => return unsupported(format!("the float operator {op:?}")),
        };
        Ok(())
    }

    /// Checked integer arithmetic (lowering-catalog.md, "Arithmetic And
    /// Overflow Checks"): 32-bit and narrower in `i64` with a range check;
    /// 64-bit add and subtract by sign tests.
    fn checked(
        &mut self,
        op: PrimOp,
        ops: &[u32],
        bits: u8,
        signed: bool,
        wide: bool,
    ) -> StageResult<()> {
        let vt = if wide { VT::I64 } else { VT::I32 };
        let x = self.a.local(VT::I64);
        let y = self.a.local(VT::I64);
        let r = self.a.local(VT::I64);
        let (lhs, rhs) = if op == PrimOp::Neg {
            (None, ops[0])
        } else {
            (Some(ops[0]), ops[1])
        };
        for (src, dst) in [(lhs, x), (Some(rhs), y)] {
            match src {
                Some(v) => {
                    self.comp(v, 0, &vt)?;
                    if !wide {
                        if signed {
                            self.a.s().i64_extend_i32_s();
                        } else {
                            self.a.s().i64_extend_i32_u();
                        }
                    }
                }
                None => self.a.i64(0),
            }
            self.a.set(dst);
        }
        if matches!(op, PrimOp::Div | PrimOp::Rem) {
            self.a.get(y);
            self.a.s().i64_eqz();
            self.a.if_();
            self.panic("division-by-zero: division by zero");
            self.a.end();
        }
        self.a.get(x);
        self.a.get(y);
        {
            let mut s = self.a.s();
            match (op, signed) {
                (PrimOp::Add, _) => s.i64_add(),
                (PrimOp::Sub | PrimOp::Neg, _) => s.i64_sub(),
                (PrimOp::Mul, _) => s.i64_mul(),
                (PrimOp::Div, true) => s.i64_div_s(),
                (PrimOp::Div, false) => s.i64_div_u(),
                (PrimOp::Rem, true) => s.i64_rem_s(),
                _ => s.i64_rem_u(),
            };
        }
        self.a.set(r);
        if wide {
            // 64-bit: add and subtract overflow by sign tests.
            match (op, signed) {
                (PrimOp::Add, true) => {
                    self.a.get(x);
                    self.a.get(r);
                    self.a.s().i64_xor();
                    self.a.get(y);
                    self.a.get(r);
                    self.a.s().i64_xor().i64_and();
                    self.a.i64(0);
                    self.a.s().i64_lt_s();
                }
                (PrimOp::Sub | PrimOp::Neg, true) => {
                    self.a.get(x);
                    self.a.get(y);
                    self.a.s().i64_xor();
                    self.a.get(x);
                    self.a.get(r);
                    self.a.s().i64_xor().i64_and();
                    self.a.i64(0);
                    self.a.s().i64_lt_s();
                }
                (PrimOp::Add, false) => {
                    self.a.get(r);
                    self.a.get(x);
                    self.a.s().i64_lt_u();
                }
                (PrimOp::Sub | PrimOp::Neg, false) => {
                    self.a.get(x);
                    self.a.get(y);
                    self.a.s().i64_lt_u();
                }
                _ => self.a.i32(0),
            }
        } else {
            let (lo, hi): (i64, i64) = if signed {
                (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
            } else {
                (0, (1i64 << bits) - 1)
            };
            self.a.get(r);
            self.a.i64(lo);
            self.a.s().i64_lt_s();
            self.a.get(r);
            self.a.i64(hi);
            self.a.s().i64_gt_s().i32_or();
        }
        self.a.if_();
        self.panic("integer-overflow: integer overflow");
        self.a.end();
        self.a.get(r);
        if !wide {
            self.a.s().i32_wrap_i64();
        }
        Ok(())
    }

    /// The current providers for a callee's row keys, in key order.
    fn push_providers(&mut self, keys: &[DefId]) -> StageResult<()> {
        for k in keys {
            let Some((_, ls)) = self.providers.iter().rev().find(|p| p.0 == *k) else {
                return unsupported("a call whose row this function does not cover");
            };
            let ls = *ls;
            self.a.get(ls[0]);
            self.a.get(ls[1]);
        }
        Ok(())
    }

    /// The keys of a function type's row, as trait items.
    fn fn_row_keys(&self, t: Ty) -> Vec<DefId> {
        let pool = self.pool();
        let t = match pool.get(t) {
            TyData::Mut(i) => i,
            _ => t,
        };
        let TyData::Fn { row, .. } = pool.get(t) else {
            return Vec::new();
        };
        let mut keys: Vec<DefId> = pool
            .row_data(row)
            .keys
            .into_iter()
            .filter_map(|k| match pool.get(k) {
                TyData::TraitValue { def, .. } => Some(def),
                _ => None,
            })
            .collect();
        keys.sort_by_key(|k| self.env().path_hash(*k));
        keys.dedup();
        keys
    }

    /// The context a call of a function value of type `callee` passes:
    /// the key ids and this body's providers of the callee type's row
    /// (codegen.md §12.4), or two nulls for the empty row.
    fn push_ctx(&mut self, callee: Ty) -> StageResult<()> {
        let keys = self.fn_row_keys(callee);
        if keys.is_empty() {
            self.a.ref_null(&ctx_keys());
            self.a.ref_null(&ctx_provs());
            return Ok(());
        }
        for k in &keys {
            self.a.i64(key_id(self.env(), *k));
        }
        self.a.array_new_fixed(&ctx_keys(), u32_of(keys.len()));
        self.push_providers(&keys)?;
        self.a.array_new_fixed(&ctx_provs(), u32_of(2 * keys.len()));
        Ok(())
    }

    fn call(&mut self, i: u32, callee_at: u32, args_at: u32, ty: Ty) -> StageResult<()> {
        let rec = self.rec(args_at);
        let args = &rec[..rec.len().saturating_sub(3)];
        let Some(c) = Callee::from_words(self.b.record(callee_at)) else {
            return unsupported("a malformed callee record");
        };
        if let Callee::TraitMethod {
            trait_,
            method,
            choice: (ChoiceKind::TraitValue, _),
            ..
        } = c
        {
            return self.call_dyn(i, trait_, method, args);
        }
        let Some(Target::Call(t)) = self.calls.get(&i).cloned() else {
            return unsupported("a call that collection did not resolve");
        };
        match &t.kind {
            TargetKind::Instance => self.call_instance(i, &t, args, ty, false),
            TargetKind::Intrinsic(key) => self.call_intrinsic(i, key, args, ty),
            TargetKind::Builtin { self_ty, .. } => {
                let name = (self.lay.path)(t.item);
                let name = name.rsplit(['.', '/']).next().unwrap_or("").to_owned();
                self.builtin(i, &name, *self_ty, args, ty)
            }
        }
    }

    /// A direct `call` of a collected instance: the arguments at the
    /// callee's parameter types, then its providers. `bracket` counts the
    /// call as a forbidden context (suspension.md §14.9), so `block_on`
    /// reached inside it panics.
    fn call_instance(
        &mut self,
        i: u32,
        t: &CallTarget,
        args: &[u32],
        ty: Ty,
        bracket: bool,
    ) -> StageResult<()> {
        let pool = self.pool();
        let mut ps = Vec::new();
        for p in self.env().params(t.item).unwrap_or_default() {
            ps.push(subst(pool, self.env(), t.item, t.args, p));
        }
        for (r, p) in args.iter().zip(&ps) {
            let want = self.vts(*p)?;
            self.load_as(*r, &want)?;
        }
        let keys = self.env().row_keys(t.item, t.args);
        self.push_providers(&keys)?;
        if bracket {
            self.forbid(1);
        }
        self.a.call(Sym::Inst(t.key));
        if bracket {
            self.forbid(-1);
        }
        // A suspending callee's plain call is its cold constructor.
        let got = if self.env().suspends(t.item) {
            self.vts(ty)?
        } else {
            self.vts(t.ret)?
        };
        self.store_from(i, &got)
    }

    /// Adds `by` to the forbidden-context counter.
    fn forbid(&mut self, by: i32) {
        self.a.global_get(crate::rt::forbid_global());
        self.a.i32(by);
        self.a.s().i32_add();
        self.a.global_set(crate::rt::forbid_global());
    }

    /// An omitted argument (`fn.default.eval`, codegen.md §13.13): a
    /// direct `call` of its default body's instance with the earlier
    /// argument values, bracketed when the body makes a call (§12.3).
    fn default_call(&mut self, i: u32, args_at: u32, ty: Ty) -> StageResult<()> {
        let Some(Target::Default { call, bracket }) = self.calls.get(&i).cloned() else {
            return unsupported("a default call that collection did not resolve");
        };
        let args = self.rec(args_at);
        self.call_instance(i, &call, &args, ty, bracket)
    }

    fn builtin(
        &mut self,
        i: u32,
        name: &str,
        self_ty: Ty,
        args: &[u32],
        ty: Ty,
    ) -> StageResult<()> {
        let self_ty = match self.pool().get(self_ty) {
            TyData::Mut(x) => x,
            _ => self_ty,
        };
        let shape = self.lay.shape(self_ty)?;
        // The intrinsic methods of the primitives' std operator and
        // comparison implementations compute what the operator computes
        // (`expr.op.std.intrinsic-method`, `expr.ord.std.intrinsic`), so
        // they share its lowering.
        if matches!(shape, Shape::Scalar(_)) && matches!(self.pool().get(self_ty), TyData::Prim(_))
        {
            if let Some(op) = intrinsic_op(name) {
                self.prim_value(op, args, ty)?;
                return self.store(i);
            }
            if matches!(name, "cmp" | "partial_cmp") {
                return self.ordering(i, args, name == "partial_cmp", ty);
            }
        }
        match (name, &shape) {
            ("to_string", Shape::Scalar(v @ (VT::I32 | VT::I64))) => {
                if self.pool().get(self_ty) == TyData::Prim(Prim::Bool) {
                    self.comp(args[0], 0, &VT::I32)?;
                    self.a.if_();
                    self.lit_into(i, "true")?;
                    self.a.else_();
                    self.lit_into(i, "false")?;
                    self.a.end();
                    return Ok(());
                }
                let (_, signed, _) = num(self.pool(), self_ty);
                self.comp(args[0], 0, v)?;
                self.a.call(Sym::Helper(Helper::IntToStr {
                    wide: *v == VT::I64,
                    signed,
                }));
                self.store(i)
            }
            ("to_string", Shape::Str) => {
                self.load(args[0])?;
                self.store(i)
            }
            ("eq", Shape::Str) => {
                self.load(args[0])?;
                self.load(args[1])?;
                self.a.call(Sym::Helper(Helper::StrEq));
                self.store(i)
            }
            _ => unsupported(format!(
                "the built-in method `{name}` at {}",
                self.pool().display(self_ty)
            )),
        }
    }

    /// `cmp` or `partial_cmp` of two numbers or characters: the operator
    /// lowering's `>` minus its `<`, as an `Ordering` (`Less`, `Equal`,
    /// `Greater` are variants 0, 1 and 2). `partial_cmp` wraps it in
    /// `.Some`, or answers `.None` when a float operand is NaN
    /// (`expr.ord.unordered`).
    fn ordering(&mut self, i: u32, args: &[u32], partial: bool, ty: Ty) -> StageResult<()> {
        let ops = [args[0], args[1]];
        let tag = self.a.local(VT::I32);
        self.a.i32(1);
        self.prim_value(PrimOp::Gt, &ops, Ty::BOOL)?;
        self.a.s().i32_add();
        self.prim_value(PrimOp::Lt, &ops, Ty::BOOL)?;
        self.a.s().i32_sub();
        self.a.set(tag);
        if !partial {
            let Shape::Enum(EnumShape {
                slots, boxed: None, ..
            }) = self.lay.shape(ty)?
            else {
                return unsupported("an `Ordering` that is not a plain tag");
            };
            if !slots.is_empty() {
                return unsupported("an `Ordering` that is not a plain tag");
            }
            self.a.get(tag);
            return self.store(i);
        }
        let Shape::Opt(OptShape::Tagged(dv), _) = self.lay.shape(ty)? else {
            return unsupported("an `Ordering?` that is not a tagged pair");
        };
        if dv.as_slice() != [VT::I32] {
            return unsupported("an `Ordering?` that is not a tagged pair");
        }
        let (_, _, float) = num(self.pool(), self.ty_of(ops[0]));
        if float {
            // Ordered exactly when neither operand is NaN; `.None` has a
            // zero payload, as a constructed one does.
            let some = self.a.local(VT::I32);
            self.prim_value(PrimOp::Eq, &[ops[0], ops[0]], Ty::BOOL)?;
            self.prim_value(PrimOp::Eq, &[ops[1], ops[1]], Ty::BOOL)?;
            self.a.s().i32_and();
            self.a.set(some);
            self.a.get(some);
            self.a.get(tag);
            self.a.get(some);
            self.a.s().i32_mul();
        } else {
            self.a.i32(1);
            self.a.get(tag);
        }
        self.store(i)
    }

    fn lit_into(&mut self, i: u32, s: &str) -> StageResult<()> {
        self.a.call(Sym::Helper(Helper::Lit(s.as_bytes().to_vec())));
        self.a.i64(i64::try_from(s.len()).unwrap_or(0) << 32);
        self.store(i)
    }

    fn call_intrinsic(&mut self, i: u32, key: &str, args: &[u32], ty: Ty) -> StageResult<()> {
        match key {
            "panic_message" => {
                self.load(args[0])?;
                self.a.call(Sym::Helper(Helper::PanicStr));
                self.a.s().unreachable();
                Ok(())
            }
            // std's `panic_with(category, message)`: the report names the
            // category, then the message when it is not empty
            // (`flow.panic.report`).
            "panic" => {
                crate::rt::write_lit(&mut self.a, "panic: ");
                self.load(args[0])?;
                self.a.call(Sym::Helper(Helper::StrToBuf));
                self.a.call(crate::rt::stderr_import());
                let sv = [VT::r(WTy::Bytes), VT::I64];
                let (b, w) = (self.a.local(sv[0].clone()), self.a.local(VT::I64));
                self.load_as(args[1], &sv)?;
                self.a.set(w);
                self.a.set(b);
                self.a.get(w);
                self.a.i64(32);
                self.a.s().i64_shr_u().i32_wrap_i64();
                self.a.if_();
                crate::rt::write_lit(&mut self.a, ": ");
                self.a.get(b);
                self.a.get(w);
                self.a.call(Sym::Helper(Helper::StrToBuf));
                self.a.call(crate::rt::stderr_import());
                self.a.end();
                crate::rt::write_lit(&mut self.a, "\n");
                self.a.s().unreachable();
                Ok(())
            }
            // `std.rt`'s report of an entry point's or a test case's `.Err`
            // on standard error (`module.entry.err-stderr`).
            "entry_write" => {
                self.load(args[0])?;
                self.a.call(Sym::Helper(Helper::StrToBuf));
                self.a.call(crate::rt::stderr_import());
                crate::rt::write_lit(&mut self.a, "\n");
                Ok(())
            }
            "block_on" => {
                // suspension.md §14.9: poll; when Pending, wait in the host.
                let Shape::Suspend { base, poll, result } = self.lay.shape(self.ty_of(args[0]))?
                else {
                    return unsupported("`block_on` of a value that is not a suspension");
                };
                let s = self.a.local(VT::r(base.clone()));
                self.comp(args[0], 0, &VT::r(base.clone()))?;
                self.a.set(s);
                // The indirect ban (`req.drive.block-on.indirect`): reached
                // while a forbidden context runs, it panics.
                self.a.global_get(crate::rt::forbid_global());
                self.a.if_();
                self.panic("suspension-forbidden-context: block_on in a forbidden context");
                self.a.end();
                let ready = self.a.local(VT::I32);
                let tmp: Vec<u32> = result.iter().map(|v| self.a.local(v.dflt())).collect();
                self.a.block();
                self.a.loop_();
                self.a.get(s);
                self.a.get(s);
                self.a.struct_get(&base, F_POLL);
                self.a.call_ref(&poll);
                for l in tmp.iter().rev() {
                    self.a.set(*l);
                }
                self.a.set(ready);
                self.a.get(ready);
                self.a.br_if(1);
                // The host waits for at least one completion and writes
                // the handles; they go to the wake table.
                self.a.call(block_import());
                self.a.call(Sym::Helper(Helper::WakeMark));
                self.a.br(0);
                self.a.end();
                self.a.end();
                for (l, v) in tmp.iter().zip(&result) {
                    self.a.get(*l);
                    self.a.conv(&v.dflt(), v);
                }
                let _ = ty;
                self.store_from(i, &result)
            }
            "bytes_len" => {
                let (_, span) = self.str_view(args[0])?;
                self.a.get(span);
                self.a.i64(32);
                self.a.s().i64_shr_u().i32_wrap_i64();
                self.store(i)
            }
            "bytes_at" => self.str_at(i, args[0], args[1]),
            "bytes_slice" => self.str_slice(i, args[0], args[1], args[2]),
            "bytes_concat" => self.concat(i, args),
            "string_from_bytes" => self.string_from_list(i, args[0]),
            "char_scalar" => {
                self.comp(args[0], 0, &VT::I32)?;
                self.store(i)
            }
            "char_from_scalar" => self.char_from_scalar(i, args[0]),
            "task_race_frame" => {
                let Shape::Suspend { base, poll, result } = self.lay.shape(ty)? else {
                    return unsupported("a `race!` frame of a type that is not a suspension");
                };
                let (_, lt) = self.list_parts(self.ty_of(args[0]))?;
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.call(Sym::Helper(Helper::RaceCold {
                    list: lt,
                    base: base.clone(),
                    poll,
                    result,
                }));
                self.store_from(i, &[VT::r(base)])
            }
            other => unsupported(format!("the intrinsic `{other}`")),
        }
    }

    fn call_dyn(&mut self, i: u32, trait_: DefId, method: DefId, args: &[u32]) -> StageResult<()> {
        let rs = self.push_dyn(trait_, method, args)?;
        self.store_from(i, &rs)
    }

    /// A trait-value call, its results left on the stack.
    fn push_dyn(&mut self, trait_: DefId, method: DefId, args: &[u32]) -> StageResult<Vec<VT>> {
        let recv_t = self.ty_of(args[0]);
        let Shape::Dyn {
            trait_: recv_trait,
            vt,
            args: targs,
        } = self.lay.shape(recv_t)?
        else {
            return unsupported("a trait-value call on a value that is not a trait value");
        };
        // The receiver's vtable holds its own trait's methods only.
        let slot = (recv_trait == trait_)
            .then(|| {
                self.env()
                    .trait_methods(trait_)
                    .iter()
                    .position(|m| *m == method)
            })
            .flatten();
        let Some(slot) = slot else {
            return unsupported("a trait-value call of a supertrait's method");
        };
        let sig = self.lay.slot_sig(trait_, targs, method)?;
        let WTy::Func(ps, rs) = &sig else {
            return unsupported("a vtable slot type");
        };
        let rl = self.a.local(VT::Eq);
        let vl = self.a.local(VT::r(vt.clone()));
        self.comp(args[0], 0, &VT::Eq)?;
        self.a.set(rl);
        self.comp(args[0], 1, &VT::r(vt.clone()))?;
        self.a.set(vl);
        self.a.get(rl);
        let mut k = 1;
        for r in &args[1..] {
            let n = self.vts(self.ty_of(*r))?.len();
            let want = ps[k..k + n].to_vec();
            self.load_as(*r, &want)?;
            k += n;
        }
        self.a.get(vl);
        self.a.struct_get(&vt, u32_of(slot));
        self.a.call_ref(&sig);
        Ok(rs.clone())
    }

    fn closure(&mut self, i: u32, rec: u32, ty: Ty) -> StageResult<()> {
        let caps = self.rec(rec);
        let (start, len) = (caps[0] as usize, caps[1] as usize);
        self.closure_value(i, start..start + len, ty)
    }

    /// A closure value: its code, the instance collection recorded (a
    /// closure body, or a function reference's adapter, codegen.md
    /// §13.11), and the captured locals `caps` (indices into
    /// `cap_local`).
    fn closure_value(&mut self, i: u32, caps: std::ops::Range<usize>, ty: Ty) -> StageResult<()> {
        let Some(Target::Closure(key)) = self.calls.get(&i).cloned() else {
            return unsupported("a closure that collection did not record");
        };
        let (env, fields) = self.env_of(caps, ty)?;
        self.a.ref_func(Sym::Inst(key));
        for (_, l) in fields {
            self.a.get(l);
        }
        self.a.struct_new(&env);
        self.store(i)
    }

    /// The environment struct of a closure value of type `ty` that
    /// captures `caps` (codegen.md, the `Closure` row): a subtype of the
    /// function type's base holding the code, then each captured local's
    /// values (a `Shared` one's cell). Each captured value comes with its
    /// field and the Wasm local holding it, in field order.
    fn env_of(
        &mut self,
        caps: std::ops::Range<usize>,
        ty: Ty,
    ) -> StageResult<(WTy, Vec<(u32, u32)>)> {
        let Shape::Fn { base, code } = self.lay.shape(ty)? else {
            return unsupported("a closure without a function type");
        };
        let mut fields = vec![VT::r(code)];
        let mut at = Vec::new();
        for c in caps {
            let l = self.b.cap_local[c].raw();
            let vs = self.local_vts(l)?;
            let ls = self.local(l)?;
            for (x, v) in ls.into_iter().zip(vs) {
                at.push((u32_of(fields.len()), x));
                fields.push(v);
            }
        }
        let env = WTy::Struct {
            fields,
            sup: Some(Box::new(base)),
            open: false,
        };
        Ok((env, at))
    }

    /// [`Self::env_of`] for the closure instruction `ci`, from the body
    /// it creates.
    fn closure_env(&mut self, ci: usize) -> StageResult<(WTy, Vec<(u32, u32)>)> {
        let caps = self.b.record(self.b.data[ci][1]).to_vec();
        let (start, len) = (caps[0] as usize, caps[1] as usize);
        let ty = self.sub(self.b.ty[ci]);
        self.env_of(start..start + len, ty)
    }

    fn coerce(&mut self, i: u32, v: u32, rec: u32, ty: Ty) -> StageResult<()> {
        let r = self.rec(rec);
        let kind = r[0];
        let want = self.vts(ty)?;
        if kind == Coercion::Never as u32 {
            self.a.s().unreachable();
            return Ok(());
        }
        if kind == Coercion::WrapSome as u32 {
            let Shape::Opt(o, inner) = self.lay.shape(ty)? else {
                return unsupported("an option wrap to a non-optional");
            };
            match &o {
                OptShape::NullRef(_) | OptShape::NullStr => self.load_as(v, &want)?,
                OptShape::Tagged(_) => {
                    self.a.i32(1);
                    self.load_as(v, &inner)?;
                }
                OptShape::Boxed(b, _) => {
                    self.load_as(v, &inner)?;
                    self.a.struct_new(b);
                }
            }
            return self.store(i);
        }
        if kind == Coercion::ToTraitValue as u32 {
            let Some(Target::VTable(trait_, slots)) = self.calls.get(&i).cloned() else {
                return unsupported("a trait-value coercion that collection did not record");
            };
            let Shape::Dyn {
                vt, args: targs, ..
            } = self.lay.shape(ty)?
            else {
                return unsupported("a coercion to a non-trait type");
            };
            let from = self.ty_of(v);
            let fv = self.vts(from)?;
            self.erase(v, &fv)?;
            let methods = self.env().trait_methods(trait_);
            for (m, t) in methods.iter().zip(&slots) {
                let sig = self.lay.slot_sig(trait_, targs, *m)?;
                // A method the compiler supplies for every type
                // (`Inspectable.runtime_type`) has no lowering yet: its
                // slot panics when called, as a host slot does.
                if matches!(t.kind, TargetKind::Builtin { .. }) {
                    self.a.ref_func(Sym::Helper(Helper::Unlowered {
                        sig,
                        what: format!("the compiler-supplied `{}`", (self.lay.path)(*m)),
                    }));
                    continue;
                }
                let target = Self::adapter_target(t)?;
                let mut ps = Vec::new();
                for p in self.env().params(t.item).unwrap_or_default() {
                    ps.extend(self.vts(subst(self.pool(), self.env(), t.item, t.args, p))?);
                }
                let mut results = self.vts(t.ret)?;
                if self.env().suspends(*m) {
                    results = vec![VT::r(suspend_base(&results).0)];
                }
                self.a.ref_func(Sym::Helper(Helper::Adapter {
                    sig,
                    self_vts: fv.clone(),
                    target: Box::new(target),
                    params: ps,
                    results,
                }));
            }
            self.a.struct_new(&vt);
            return self.store(i);
        }
        if kind == Coercion::Weaken as u32
            || kind == Coercion::Variance as u32
            || kind == Coercion::RowSubsume as u32
        {
            self.load_as(v, &want)?;
            return self.store(i);
        }
        unsupported(format!("the coercion kind {kind}"))
    }

    fn adapter_target(t: &CallTarget) -> StageResult<Sym> {
        match t.kind {
            TargetKind::Instance => Ok(Sym::Inst(t.key)),
            _ => unsupported("a vtable slot filled by a compiler lowering"),
        }
    }

    /// Pushes a value as `eqref` (§13.5.1, "As an open value").
    fn erase(&mut self, v: u32, vts: &[VT]) -> StageResult<()> {
        if let [x @ (VT::Eq | VT::Ref(..))] = vts {
            self.comp(v, 0, x)
        } else {
            self.load_as(v, vts)?;
            self.a.struct_new(&box_of(vts));
            Ok(())
        }
    }

    fn list_parts(&self, t: Ty) -> StageResult<(Vec<VT>, WTy)> {
        match self.lay.shape(t)? {
            Shape::List { elem, ty } => Ok((elem, ty)),
            _ => unsupported("a list operation on a non-list"),
        }
    }

    fn intrinsic(&mut self, i: u32, op: u32, rec: u32, ty: Ty) -> StageResult<()> {
        let Some(op) = IntrinsicOp::from_u32(op) else {
            return unsupported("an unknown intrinsic operation");
        };
        let args = self.rec(rec);
        match op {
            IntrinsicOp::ListLen | IntrinsicOp::MapLen => {
                let t = self.ty_of(args[0]);
                let (Shape::List { ty: lt, .. } | Shape::Map { ty: lt, .. }) = self.lay.shape(t)?
                else {
                    return unsupported("a length of a non-collection");
                };
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.struct_get(&lt, 0);
                self.store(i)
            }
            IntrinsicOp::ListIndex => {
                let (elem, lt) = self.list_parts(self.ty_of(args[0]))?;
                let l = self.a.local(VT::r(lt.clone()));
                let ix = self.a.local(VT::I32);
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.comp(args[1], 0, &VT::I32)?;
                self.a.set(ix);
                self.bounds(l, &lt, ix);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    self.a.get(l);
                    self.a.struct_get(&lt, 1 + u32_of(k));
                    self.a.get(ix);
                    self.a.array_get(&WTy::Array(st.clone()));
                    self.a.conv(&st, v);
                }
                self.store_from(i, &elem)
            }
            IntrinsicOp::ListSet => {
                let (elem, lt) = self.list_parts(self.ty_of(args[0]))?;
                let l = self.a.local(VT::r(lt.clone()));
                let ix = self.a.local(VT::I32);
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.comp(args[1], 0, &VT::I32)?;
                self.a.set(ix);
                self.bounds(l, &lt, ix);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    self.a.get(l);
                    self.a.struct_get(&lt, 1 + u32_of(k));
                    self.a.get(ix);
                    self.comp(args[2], k, v)?;
                    self.a.array_set(&WTy::Array(st));
                }
                Ok(())
            }
            IntrinsicOp::ListPush => {
                let (elem, lt) = self.list_parts(self.ty_of(args[0]))?;
                let l = self.a.local(VT::r(lt.clone()));
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                let n = self.a.local(VT::I32);
                self.a.get(l);
                self.a.struct_get(&lt, 0);
                self.a.set(n);
                self.grow(l, &lt, n, 1, &elem);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    self.a.get(l);
                    self.a.struct_get(&lt, 1 + u32_of(k));
                    self.a.get(n);
                    self.comp(args[1], k, v)?;
                    self.a.array_set(&WTy::Array(st));
                }
                self.a.get(l);
                self.a.get(n);
                self.a.i32(1);
                self.a.s().i32_add();
                self.a.struct_set(&lt, 0);
                Ok(())
            }
            IntrinsicOp::ListIter => self.list_iter(i, args[0], ty),
            IntrinsicOp::StrConcat => self.concat(i, &args),
            IntrinsicOp::StrIndex => self.str_at(i, args[0], args[1]),
            IntrinsicOp::StrEq => {
                let sv = [VT::r(WTy::Bytes), VT::I64];
                self.load_as(args[0], &sv)?;
                self.load_as(args[1], &sv)?;
                self.a.call(Sym::Helper(Helper::StrEq));
                self.store(i)
            }
            IntrinsicOp::MapIndex
            | IntrinsicOp::MapGet
            | IntrinsicOp::MapSet
            | IntrinsicOp::MapRemove => self.map_op(i, op, &args, ty),
            IntrinsicOp::MapIter => self.map_iter(i, args[0], ty),
            IntrinsicOp::Item => unsupported("the intrinsic operation Item"),
        }
    }

    /// `i < len` or the `index-out-of-bounds` panic.
    /// A string's array and span in fresh locals.
    fn str_view(&mut self, r: u32) -> StageResult<(u32, u32)> {
        let sv = [VT::r(WTy::Bytes), VT::I64];
        let (b, s) = (self.a.local(sv[0].clone()), self.a.local(VT::I64));
        self.load_as(r, &sv)?;
        self.a.set(s);
        self.a.set(b);
        Ok((b, s))
    }

    /// Pushes the `len` field of the span in local `s`.
    fn span_len(&mut self, s: u32) {
        self.a.get(s);
        self.a.i64(32);
        self.a.s().i64_shr_u().i32_wrap_i64();
    }

    /// `s[i]` (`StrIndex`, `bytes_at`): `i < len`, then `bytes[start + i]`.
    /// The array may be longer than the view, so the engine's own bounds
    /// check is not enough.
    fn str_at(&mut self, i: u32, text: u32, index: u32) -> StageResult<()> {
        let (b, s) = self.str_view(text)?;
        let ix = self.a.local(VT::I32);
        self.comp(index, 0, &VT::I32)?;
        self.a.set(ix);
        self.a.get(ix);
        self.span_len(s);
        self.a.s().i32_ge_u();
        self.a.if_();
        self.panic("index-out-of-bounds: string index out of bounds");
        self.a.end();
        self.a.get(b);
        self.a.get(s);
        self.a.s().i32_wrap_i64();
        self.a.get(ix);
        self.a.s().i32_add();
        self.a.array_get(&WTy::Bytes);
        self.store(i)
    }

    /// `string_from_bytes(bytes)`: a new array of the list's `len` bytes,
    /// viewed from 0. The caller has checked that they are UTF-8.
    fn string_from_list(&mut self, i: u32, list: u32) -> StageResult<()> {
        let (elem, lt) = self.list_parts(self.ty_of(list))?;
        let [ev] = elem.as_slice() else {
            return unsupported("a list of bytes whose elements are not one word");
        };
        let st = storage(ev).dflt();
        let l = self.a.local(VT::r(lt.clone()));
        let (n, k) = (self.a.local(VT::I32), self.a.local(VT::I32));
        let arr = self.a.local(VT::r(WTy::Bytes));
        self.comp(list, 0, &VT::r(lt.clone()))?;
        self.a.set(l);
        self.a.get(l);
        self.a.struct_get(&lt, 0);
        self.a.set(n);
        self.a.get(n);
        self.a.array_new_default(&WTy::Bytes);
        self.a.set(arr);
        self.a.i32(0);
        self.a.set(k);
        self.a.block();
        self.a.loop_();
        self.a.get(k);
        self.a.get(n);
        self.a.s().i32_ge_u();
        self.a.br_if(1);
        self.a.get(arr);
        self.a.get(k);
        self.a.get(l);
        self.a.struct_get(&lt, 1);
        self.a.get(k);
        self.a.array_get(&WTy::Array(st));
        self.a.array_set(&WTy::Bytes);
        self.a.get(k);
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.set(k);
        self.a.br(0);
        self.a.end();
        self.a.end();
        self.a.get(arr);
        self.a.get(n);
        self.a.s().i64_extend_i32_u();
        self.a.i64(32);
        self.a.s().i64_shl();
        self.store(i)
    }

    /// `char_from_scalar(point)`: `.Some(point)` when `point` is a scalar
    /// value (at most `0x10FFFF`, not a surrogate), else `.None`. The
    /// payload is `point * valid`, so `.None` carries zero.
    fn char_from_scalar(&mut self, i: u32, point: u32) -> StageResult<()> {
        let p = self.a.local(VT::I32);
        let ok = self.a.local(VT::I32);
        self.comp(point, 0, &VT::I32)?;
        self.a.set(p);
        self.a.get(p);
        self.a.i32(0x10_FFFF);
        self.a.s().i32_le_u();
        self.a.get(p);
        self.a.i32(0xD800);
        self.a.s().i32_sub();
        self.a.i32(0x7FF);
        self.a.s().i32_gt_u();
        self.a.s().i32_and();
        self.a.set(ok);
        self.a.get(ok);
        self.a.get(p);
        self.a.get(ok);
        self.a.s().i32_mul();
        self.store(i)
    }

    /// `bytes_slice(text, start, end)`: `start <= end <= len`, then a new
    /// span over the same array; nothing is allocated or copied.
    fn str_slice(&mut self, i: u32, text: u32, start: u32, end: u32) -> StageResult<()> {
        let (b, s) = self.str_view(text)?;
        let (lo, hi) = (self.a.local(VT::I32), self.a.local(VT::I32));
        self.comp(start, 0, &VT::I32)?;
        self.a.set(lo);
        self.comp(end, 0, &VT::I32)?;
        self.a.set(hi);
        self.a.get(lo);
        self.a.get(hi);
        self.a.s().i32_gt_u();
        self.a.get(hi);
        self.span_len(s);
        self.a.s().i32_gt_u().i32_or();
        self.a.if_();
        self.panic("index-out-of-bounds: string slice out of bounds");
        self.a.end();
        self.a.get(b);
        self.a.get(hi);
        self.a.get(lo);
        self.a.s().i32_sub().i64_extend_i32_u();
        self.a.i64(32);
        self.a.s().i64_shl();
        self.a.get(s);
        self.a.s().i32_wrap_i64();
        self.a.get(lo);
        self.a.s().i32_add().i64_extend_i32_u().i64_or();
        self.store(i)
    }

    fn bounds(&mut self, l: u32, lt: &WTy, ix: u32) {
        self.a.get(ix);
        self.a.get(l);
        self.a.struct_get(lt, 0);
        self.a.s().i32_ge_u();
        self.a.if_();
        self.panic("index-out-of-bounds: list index out of bounds");
        self.a.end();
    }

    /// Grows the arrays `first..` of collection `l` (length in `n`) when full.
    fn grow(&mut self, l: u32, lt: &WTy, n: u32, first: u32, comps: &[VT]) {
        if comps.is_empty() {
            return;
        }
        let cap = self.a.local(VT::I32);
        self.a.get(n);
        self.a.get(l);
        self.a.struct_get(lt, first);
        self.a.array_len();
        self.a.s().i32_eq();
        self.a.if_();
        self.a.get(n);
        self.a.i32(2);
        self.a.s().i32_mul();
        self.a.i32(4);
        self.a.s().i32_add();
        self.a.set(cap);
        for (k, v) in comps.iter().enumerate() {
            let arr = WTy::Array(storage(v).dflt());
            let fresh = self.a.local(VT::r(arr.clone()));
            self.a.get(cap);
            self.a.array_new_default(&arr);
            self.a.set(fresh);
            self.a.get(fresh);
            self.a.i32(0);
            self.a.get(l);
            self.a.struct_get(lt, first + u32_of(k));
            self.a.i32(0);
            self.a.get(n);
            self.a.array_copy(&arr, &arr);
            self.a.get(l);
            self.a.get(fresh);
            self.a.struct_set(lt, first + u32_of(k));
        }
        self.a.end();
    }

    fn list_iter(&mut self, i: u32, list: u32, ty: Ty) -> StageResult<()> {
        let lt_ty = self.ty_of(list);
        let (elem, lt) = self.list_parts(lt_ty)?;
        let TyData::Adt { args, .. } = self.pool().get(match self.pool().get(lt_ty) {
            TyData::Mut(x) => x,
            _ => lt_ty,
        }) else {
            return unsupported("a list type");
        };
        let et = self.pool().list_items(args)[0];
        let ot = self.pool().intern_ty(&TyData::Option(et));
        let Shape::Opt(o, _) = self.lay.shape(ot)? else {
            return unsupported("an optional element layout");
        };
        let opt = opt_form(o);
        // The closure ABI: environment, then the (unused) context.
        let code = WTy::Func(
            vec![VT::Eq, VT::rn(ctx_keys()), VT::rn(ctx_provs())],
            self.vts(ot)?,
        );
        let base = crate::layout::closure_base(&code);
        let env = WTy::Struct {
            fields: vec![VT::r(code.clone()), VT::r(lt.clone()), VT::I32],
            sup: Some(Box::new(base)),
            open: false,
        };
        let Shape::Data { ty: it, .. } = self.lay.shape(ty)? else {
            return unsupported("an iterator without a struct layout");
        };
        self.a.ref_func(Sym::Helper(Helper::ListStep {
            code,
            env: env.clone(),
            list: lt.clone(),
            elem,
            opt,
        }));
        self.comp(list, 0, &VT::r(lt))?;
        self.a.i32(0);
        self.a.struct_new(&env);
        self.a.struct_new(&it);
        self.store(i)
    }

    /// A map type's key and value layouts, its struct, and its key type.
    fn map_parts(&self, t: Ty) -> StageResult<(Vec<VT>, Vec<VT>, WTy, Ty)> {
        let Shape::Map { key, val, ty } = self.lay.shape(t)? else {
            return unsupported("a map operation on a non-map");
        };
        let pool = self.pool();
        let t = match pool.get(t) {
            TyData::Mut(x) => x,
            _ => t,
        };
        let TyData::Adt { args, .. } = pool.get(t) else {
            return unsupported("a map type");
        };
        let Some(&k) = pool.list_items(args).first() else {
            return unsupported("a map type without a key");
        };
        Ok((key, val, ty, k))
    }

    /// How instruction `i`'s map hashes and compares keys of type `kt`
    /// (codegen.md §13.12): inline for an integer, `bool`, `char` or
    /// `string`, else through the `hash_of` and `Eq.eq` instances that
    /// collection selected at the key type.
    fn key_ops(&self, i: u32, kt: Ty, key: &[VT]) -> StageResult<KeyOps> {
        if let Some(Target::MapKey { hash, eq }) = self.calls.get(&i) {
            let params = self.key_params(hash, 0)?;
            if params.len() != key.len()
                || self.key_params(eq, 0)? != params
                || self.key_params(eq, 1)? != params
                || self.vts(hash.ret)? != [VT::I64]
                || self.vts(eq.ret)? != [VT::I32]
            {
                return unsupported("a map key whose `hash_of` or `eq` takes other values");
            }
            return Ok(KeyOps::Call {
                hash: Box::new(Sym::Inst(hash.key)),
                eq: Box::new(Sym::Inst(eq.key)),
                params,
            });
        }
        if !inline_map_key(self.pool(), kt) {
            return unsupported("a map key whose operations collection did not select");
        }
        Ok(match key {
            [VT::I32] => KeyOps::I32,
            [VT::I64] => KeyOps::I64,
            [VT::Ref(t, false), VT::I64] if **t == WTy::Bytes => KeyOps::Str,
            _ => return unsupported("an inline map key of another layout"),
        })
    }

    /// The values parameter `k` of a key operation's instance takes.
    fn key_params(&self, t: &CallTarget, k: usize) -> StageResult<Vec<VT>> {
        if t.kind != TargetKind::Instance
            || self.env().suspends(t.item)
            || !self.env().row_keys(t.item, t.args).is_empty()
        {
            return unsupported("a map key whose `Eq` or `Hash` is not a plain function");
        }
        let ps = self.env().params(t.item).unwrap_or_default();
        let Some(&p) = ps.get(k) else {
            return unsupported("a map key operation without its parameter");
        };
        self.vts(subst(self.pool(), self.env(), t.item, t.args, p))
    }

    /// Pushes an empty map whose entry arrays hold `cap` entries (0 or a
    /// power of two), with an index of twice that.
    fn new_map(&mut self, mt: &WTy, comps: &[VT], cap: i32) {
        let ints = WTy::Array(VT::I32);
        self.a.i32(0);
        self.a.i32(0);
        self.a.i32(cap * 2);
        self.a.array_new_default(&ints);
        self.a.i32(cap);
        self.a.array_new_default(&ints);
        for v in comps {
            self.a.i32(cap);
            self.a.array_new_default(&WTy::Array(storage(v).dflt()));
        }
        self.a.struct_new(mt);
    }

    /// Evaluates key `r` into fresh locals, and its bucket hash into one.
    fn map_key(&mut self, r: u32, key: &[VT], ops: &KeyOps) -> StageResult<(Vec<u32>, u32)> {
        let kl: Vec<u32> = key.iter().map(|v| self.a.local(v.clone())).collect();
        for (k, v) in key.iter().enumerate() {
            self.comp(r, k, v)?;
            self.a.set(kl[k]);
        }
        for l in &kl {
            self.a.get(*l);
        }
        self.a.call(Sym::Helper(Helper::MapHash {
            key: key.to_vec(),
            ops: ops.clone(),
        }));
        let h = self.a.local(VT::I32);
        self.a.set(h);
        Ok((kl, h))
    }

    /// Inserts or replaces the entry of the key in `kl` (hash `h`) with
    /// value `v` in the map in local `m`.
    fn map_put(
        &mut self,
        m: u32,
        parts: (&WTy, &[VT], &[VT]),
        ops: &KeyOps,
        kl: &[u32],
        h: u32,
        v: u32,
    ) -> StageResult<()> {
        let (mt, key, val) = parts;
        self.a.get(m);
        self.a.get(h);
        for l in kl {
            self.a.get(*l);
        }
        for (k, vt) in val.iter().enumerate() {
            self.comp(v, k, vt)?;
        }
        self.a.call(Sym::Helper(Helper::MapPut {
            map: mt.clone(),
            key: key.to_vec(),
            val: val.to_vec(),
            ops: ops.clone(),
        }));
        Ok(())
    }

    /// A map literal: each entry inserted in source order
    /// (`expr.map.order`), so a later equal key replaces the earlier value
    /// in the earlier entry's place (`expr.map.duplicate.last`).
    fn map_literal(&mut self, i: u32, items: &[u32], ty: Ty) -> StageResult<()> {
        let (key, val, mt, kt) = self.map_parts(ty)?;
        let n = items.len() / 2;
        let cap = if n == 0 {
            0
        } else {
            n.next_power_of_two().max(4)
        };
        let Ok(cap) = i32::try_from(cap) else {
            return unsupported("a map literal too long for one array");
        };
        self.new_map(&mt, &[key.as_slice(), val.as_slice()].concat(), cap);
        if n > 0 {
            let ops = self.key_ops(i, kt, &key)?;
            let m = self.a.local(VT::r(mt.clone()));
            self.a.set(m);
            for e in 0..n {
                let (kl, h) = self.map_key(items[2 * e], &key, &ops)?;
                self.map_put(m, (&mt, &key, &val), &ops, &kl, h, items[2 * e + 1])?;
            }
            self.a.get(m);
        }
        self.store(i)
    }

    /// `m[k]`, `m.get(k)`, `m[k] = v` and `m.remove(k)` (codegen.md
    /// §13.12): hash the key, then probe for its entry.
    fn map_op(&mut self, i: u32, op: IntrinsicOp, args: &[u32], ty: Ty) -> StageResult<()> {
        let (key, val, mt, kt) = self.map_parts(self.ty_of(args[0]))?;
        let ops = self.key_ops(i, kt, &key)?;
        let m = self.a.local(VT::r(mt.clone()));
        self.comp(args[0], 0, &VT::r(mt.clone()))?;
        self.a.set(m);
        let (kl, h) = self.map_key(args[1], &key, &ops)?;
        if op == IntrinsicOp::MapSet {
            return self.map_put(m, (&mt, &key, &val), &ops, &kl, h, args[2]);
        }
        let e = self.a.local(VT::I32);
        self.a.get(m);
        self.a.get(h);
        for l in &kl {
            self.a.get(*l);
        }
        self.a.call(Sym::Helper(Helper::MapFind {
            map: mt.clone(),
            key: key.clone(),
            ops,
        }));
        self.a.set(e);
        let vfirst = M_KEYS + u32_of(key.len());
        let read = |em: &mut Self| {
            for (k, v) in val.iter().enumerate() {
                let st = storage(v).dflt();
                em.a.get(m);
                em.a.struct_get(&mt, vfirst + u32_of(k));
                em.a.get(e);
                em.a.array_get(&WTy::Array(st.clone()));
                em.a.conv(&st, v);
            }
        };
        if op == IntrinsicOp::MapIndex {
            // expr.index.map.read-value
            self.a.get(e);
            self.a.i32(0);
            self.a.s().i32_lt_s();
            self.a.if_();
            self.panic("index-out-of-bounds: map key not found");
            self.a.end();
            read(self);
            return self.store_from(i, &val);
        }
        // `get` and `remove` answer `V?`.
        let Shape::Opt(o, _) = self.lay.shape(ty)? else {
            return unsupported("a map lookup without an optional result");
        };
        let res = self.result(i)?;
        let want = self.vals[&i].1.clone();
        self.a.get(e);
        self.a.i32(0);
        self.a.s().i32_ge_s();
        self.a.if_();
        if matches!(o, OptShape::Tagged(_)) {
            self.a.i32(1);
        }
        read(self);
        if let OptShape::Boxed(b, _) = &o {
            self.a.struct_new(b);
        }
        for l in res.iter().rev() {
            self.a.set(*l);
        }
        if op == IntrinsicOp::MapRemove {
            // The entry stays in place as a tombstone: probes pass it and
            // iteration skips it, so the survivors keep their order.
            let ints = WTy::Array(VT::I32);
            self.a.get(m);
            self.a.struct_get(&mt, M_HASHES);
            self.a.get(e);
            self.a.i32(-1);
            self.a.array_set(&ints);
            for (k, v) in key.iter().chain(&val).enumerate() {
                let st = storage(v).dflt();
                self.a.get(m);
                self.a.struct_get(&mt, M_KEYS + u32_of(k));
                self.a.get(e);
                self.a.zero(&st);
                self.a.array_set(&WTy::Array(st));
            }
            self.a.get(m);
            self.a.get(m);
            self.a.struct_get(&mt, M_LIVE);
            self.a.i32(1);
            self.a.s().i32_sub();
            self.a.struct_set(&mt, M_LIVE);
        }
        self.a.else_();
        for (l, v) in res.iter().zip(&want) {
            self.a.zero(&v.dflt());
            self.a.set(*l);
        }
        self.a.end();
        Ok(())
    }

    /// `Map.iter()`: a cursor closure over the entries (codegen.md
    /// §13.12), capturing the written and live entry counts.
    fn map_iter(&mut self, i: u32, map: u32, ty: Ty) -> StageResult<()> {
        let (key, val, mt, _) = self.map_parts(self.ty_of(map))?;
        let pool = self.pool();
        let it_t = match pool.get(ty) {
            TyData::Mut(x) => x,
            _ => ty,
        };
        let TyData::Adt { args, .. } = pool.get(it_t) else {
            return unsupported("an iterator type");
        };
        let Some(&item) = pool.list_items(args).first() else {
            return unsupported("an iterator type without its item");
        };
        let ot = pool.intern_ty(&TyData::Option(item));
        let Shape::Opt(o, _) = self.lay.shape(ot)? else {
            return unsupported("an optional entry layout");
        };
        let Shape::Tuple { boxed: tuple, .. } = self.lay.shape(item)? else {
            return unsupported("a map entry that is not a tuple");
        };
        let code = WTy::Func(
            vec![VT::Eq, VT::rn(ctx_keys()), VT::rn(ctx_provs())],
            self.vts(ot)?,
        );
        let base = crate::layout::closure_base(&code);
        let env = WTy::Struct {
            fields: vec![
                VT::r(code.clone()),
                VT::r(mt.clone()),
                VT::I32,
                VT::I32,
                VT::I32,
            ],
            sup: Some(Box::new(base)),
            open: false,
        };
        let Shape::Data { ty: it, .. } = self.lay.shape(ty)? else {
            return unsupported("an iterator without a struct layout");
        };
        let m = self.a.local(VT::r(mt.clone()));
        self.comp(map, 0, &VT::r(mt.clone()))?;
        self.a.set(m);
        self.a.ref_func(Sym::Helper(Helper::MapStep {
            env: env.clone(),
            comps: [key, val].concat(),
            tuple,
            opt: opt_form(o),
        }));
        self.a.get(m);
        self.a.i32(0);
        self.a.get(m);
        self.a.struct_get(&mt, M_USED);
        self.a.get(m);
        self.a.struct_get(&mt, M_LIVE);
        self.a.struct_new(&env);
        self.a.struct_new(&it);
        self.store(i)
    }
}

/// How a cursor builds its `T?` result.
fn opt_form(o: OptShape) -> OptForm {
    match o {
        OptShape::NullRef(_) => OptForm::NullRef,
        OptShape::NullStr => OptForm::NullStr,
        OptShape::Tagged(_) => OptForm::Tagged,
        OptShape::Boxed(b, _) => OptForm::Boxed(b),
    }
}

/// The Wasm signature of an instance: its parameters' values, then one
/// trait value per row key (codegen.md §12.4); a closure's code takes its
/// environment first.
pub fn signature(
    lay: &Lay<'_>,
    b: &Body,
    sub: u16,
    args: TyList,
    ret: Ty,
) -> StageResult<(Vec<VT>, Vec<VT>, Vec<u32>)> {
    let item = b.item;
    let s = |t: Ty| subst(lay.pool, lay.env, item, args, t);
    let mut params = Vec::new();
    let mut plocals = Vec::new();
    if sub == 0 {
        let mut in_sub = vec![false; b.local_ty.len()];
        for r in &b.sub_params[1..] {
            for &l in &b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize] {
                if let Some(x) = in_sub.get_mut(l as usize) {
                    *x = true;
                }
            }
        }
        for (l, sub_param) in in_sub.iter().enumerate() {
            if b.local_flags[l] & local_flags::PARAM != 0 && !sub_param {
                plocals.push(u32_of(l));
            }
        }
        for &l in &plocals {
            params.extend(lay.vts(s(b.local_ty[l as usize]))?);
        }
        for k in lay.env.row_keys(item, args) {
            params.push(VT::Eq);
            params.push(VT::r(lay.vtable(k, TyList::EMPTY)?));
        }
        let mut results = lay.vts(s(ret))?;
        if lay.env.suspends(item) {
            results = vec![VT::r(suspend_base(&results).0)];
        }
        Ok((params, results, plocals))
    } else {
        let r = b.sub_params[sub as usize];
        plocals = b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize].to_vec();
        params.push(VT::Eq);
        for &l in &plocals {
            params.extend(lay.vts(s(b.local_ty[l as usize]))?);
        }
        params.push(VT::rn(ctx_keys()));
        params.push(VT::rn(ctx_provs()));
        let ci = closure_of(b, sub)?;
        let Shape::Fn { code, .. } = lay.shape(s(b.ty[ci]))? else {
            return unsupported("a closure without a function type");
        };
        let WTy::Func(_, rs) = code else {
            return unsupported("a closure code type");
        };
        Ok((params, rs, plocals))
    }
}

/// The `Closure` instruction whose body is sub-body `sub`.
fn closure_of(b: &Body, sub: u16) -> StageResult<usize> {
    match (0..b.len()).find(|&i| b.tags[i] == Tag::Closure && b.data[i][0] == u32::from(sub)) {
        Some(ci) => Ok(ci),
        None => unsupported("a closure body without its closure"),
    }
}

fn is_point(t: Tag) -> bool {
    matches!(
        t,
        Tag::Await | Tag::AwaitValue | Tag::AwaitAll | Tag::AwaitRace
    )
}

/// The blocks an instruction owns, by its operand schema.
fn operand_blocks(b: &Body, i: u32) -> Vec<u32> {
    let [a, bw] = b.data[i as usize];
    let rec = |at: u32| b.record(at).to_vec();
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

type Plan = (HashMap<u32, u32>, HashMap<u32, (u32, u32)>);

/// Numbers the suspension points in structural order and records, for
/// every block and instruction that contains one, its range of states
/// (suspension.md §14.2): a subtree's states are contiguous.
fn plan(b: &Body, root: u32) -> StageResult<Plan> {
    fn join(r: Option<(u32, u32)>, x: Option<(u32, u32)>) -> Option<(u32, u32)> {
        match (r, x) {
            (Some(a), Some(c)) => Some((a.0.min(c.0), a.1.max(c.1))),
            (a, c) => a.or(c),
        }
    }
    fn block(b: &Body, blk: u32, p: &mut Plan, next: &mut u32) -> StageResult<Option<(u32, u32)>> {
        let mut r = None;
        for &i in b.record(b.data[blk as usize][0]) {
            if b.tags[i as usize] == Tag::Block {
                continue;
            }
            r = join(r, inst(b, i, p, next)?);
        }
        if let Some(x) = r {
            p.1.insert(blk, x);
        }
        Ok(r)
    }
    fn inst(b: &Body, i: u32, p: &mut Plan, next: &mut u32) -> StageResult<Option<(u32, u32)>> {
        let mut r = None;
        let blocks = operand_blocks(b, i);
        for (k, ob) in blocks.iter().enumerate() {
            let x = block(b, *ob, p, next)?;
            if x.is_some() && b.tags[i as usize] == Tag::Scope && k > 0 {
                return unsupported("a suspension point in a `defer` suite");
            }
            r = join(r, x);
        }
        if is_point(b.tags[i as usize]) {
            *next += 1;
            p.0.insert(i, *next);
            r = join(r, Some((*next, *next)));
        }
        if let Some(x) = r {
            p.1.insert(i, x);
        }
        Ok(r)
    }
    let mut p = (HashMap::new(), HashMap::new());
    let mut next = 0;
    block(b, root, &mut p, &mut next)?;
    Ok(p)
}

impl<'a> Em<'a> {
    fn new(
        lay: Lay<'a>,
        b: &'a Body,
        args: TyList,
        calls: &'a HashMap<u32, Target>,
        key: Hash128,
        a: Asm,
        results: Vec<VT>,
    ) -> Self {
        Em {
            lay,
            b,
            item: b.item,
            args,
            calls,
            a,
            locals: vec![None; b.local_ty.len()],
            cells: cell_locals(b),
            vals: HashMap::new(),
            ctrl: Vec::new(),
            deciding: Vec::new(),
            providers: Vec::new(),
            key,
            results,
            ret: None,
            scopes: HashMap::new(),
            defer_flags: HashMap::new(),
            susp: None,
        }
    }

    fn range(&self, x: u32) -> Option<(u32, u32)> {
        self.susp.as_ref().and_then(|s| s.ranges.get(&x).copied())
    }
    fn pc(&self) -> u32 {
        self.susp.as_ref().map_or(0, |s| s.pc)
    }
    /// Pushes whether `lo <= pc <= hi`.
    fn in_range(&mut self, lo: u32, hi: u32) {
        let pc = self.pc();
        let (lo, hi) = (lo.cast_signed(), hi.cast_signed());
        self.a.get(pc);
        self.a.i32(lo);
        if lo == hi {
            self.a.s().i32_eq();
            return;
        }
        self.a.s().i32_ge_u();
        self.a.get(pc);
        self.a.i32(hi);
        self.a.s().i32_le_u().i32_and();
    }
    /// `if pc == 0` around fresh-path-only code (no-op outside a machine).
    fn fresh_only(&mut self, f: impl FnOnce(&mut Self) -> StageResult<()>) -> StageResult<()> {
        if self.susp.is_none() {
            return f(self);
        }
        let pc = self.pc();
        self.a.get(pc);
        self.a.s().i32_eqz();
        self.a.if_();
        self.open(Ctl::Plain);
        f(self)?;
        self.ctrl.pop();
        self.a.end();
        Ok(())
    }

    /// A block list that contains a suspension point (suspension.md
    /// §14.2): on resume, the code before the point is skipped, and each
    /// construct on the way enters the branch that holds the point.
    fn resume_list(&mut self, items: &[u32]) -> StageResult<()> {
        let last = items.iter().rposition(|i| self.range(*i).is_some());
        let mut j = 0;
        while j < items.len() {
            let i = items[j];
            if let Some((lo, hi)) = self.range(i) {
                let pc = self.pc();
                self.a.get(pc);
                self.a.s().i32_eqz();
                self.in_range(lo, hi);
                self.a.s().i32_or();
                self.a.if_();
                self.open(Ctl::Plain);
                self.inst(i)?;
                self.ctrl.pop();
                self.a.end();
                j += 1;
                continue;
            }
            let start = j;
            while j < items.len() && self.range(items[j]).is_none() {
                j += 1;
            }
            let group = items[start..j].to_vec();
            if last.is_some_and(|l| start < l) {
                self.fresh_only(|em| {
                    for g in group {
                        em.inst(g)?;
                    }
                    Ok(())
                })?;
            } else {
                for g in group {
                    self.inst(g)?;
                }
            }
        }
        Ok(())
    }

    fn global_hash(&self, a: u32) -> Hash128 {
        let def = self.b.record(a).first().copied().unwrap_or(NONE);
        self.env().path_hash(DefId::from_raw(def))
    }

    /// `return`: through the exit ladders of the scopes it leaves.
    fn ret(&mut self, a: u32) -> StageResult<()> {
        let crossing = self.ctrl.iter().any(|c| matches!(c, Ctl::Scope(_)));
        let results = self.results.clone();
        if crossing {
            if a != NONE && !results.is_empty() {
                let ls = if let Some(l) = &self.ret {
                    l.clone()
                } else {
                    let l: Vec<u32> = results.iter().map(|v| self.a.local(v.clone())).collect();
                    self.ret = Some(l.clone());
                    l
                };
                self.load_as(a, &results)?;
                for l in ls.iter().rev() {
                    self.a.set(*l);
                }
            }
            return self.exit(Exit::Ret);
        }
        if a != NONE {
            self.load_as(a, &results)?;
        }
        self.finish_return();
        Ok(())
    }

    fn finish_return(&mut self) {
        if let Some(base) = self.susp.as_ref().map(|s| s.base.clone()) {
            self.a.ref_null(&base);
        }
        self.a.s().return_();
    }

    /// Leaves by `e`: to the innermost cleanup scope it crosses, which
    /// runs its suites and then takes `e` again; else directly.
    fn exit(&mut self, e: Exit) -> StageResult<()> {
        let floor = match e {
            Exit::Brk(t) => self.ctrl.iter().rposition(|c| *c == Ctl::Brk(t)),
            Exit::Cont(t) => self.ctrl.iter().rposition(|c| *c == Ctl::Cont(t)),
            Exit::Ret | Exit::Cancel => None,
        };
        let scope = self
            .ctrl
            .iter()
            .rposition(|c| matches!(c, Ctl::Scope(_)))
            .filter(|p| floor.is_none_or(|f| *p > f));
        if let Some(p) = scope {
            let Ctl::Scope(s) = self.ctrl[p] else {
                return unsupported("a cleanup scope marker");
            };
            let Some(info) = self.scopes.get_mut(&s) else {
                return unsupported("a cleanup scope without its ladder");
            };
            let code = if let Some(k) = info.exits.iter().position(|x| *x == e) {
                k + 1
            } else {
                info.exits.push(e);
                info.exits.len()
            };
            let local = info.code;
            self.a.i32(i32::try_from(code).unwrap_or(0));
            self.a.set(local);
            self.a.br(u32_of(self.ctrl.len() - 1 - p));
            return Ok(());
        }
        match e {
            Exit::Brk(t) => {
                let d = self.depth_of(Ctl::Brk(t))?;
                self.a.br(d);
            }
            Exit::Cont(t) => {
                let d = self.depth_of(Ctl::Cont(t))?;
                self.a.br(d);
            }
            Exit::Ret => {
                if let Some(ls) = self.ret.clone() {
                    for l in ls {
                        self.a.get(l);
                    }
                } else if !self.results.is_empty() {
                    self.a.s().unreachable();
                    return Ok(());
                }
                self.finish_return();
            }
            Exit::Cancel => {
                for v in self.results.clone() {
                    self.a.zero(&v);
                }
                self.finish_return();
            }
        }
        Ok(())
    }

    /// A `Scope` with `defer` suites: the exit ladder (codegen.md §12.2).
    /// Each suite has a "registered" flag; every exit stores its code and
    /// branches to the scope's end, which runs the registered suites last
    /// in, first out, then takes the exit.
    fn scope(&mut self, i: u32, body: u32, rec: u32) -> StageResult<()> {
        let suites = self.rec(rec);
        self.result(i)?;
        let code = self.a.local(VT::I32);
        let flags: Vec<u32> = suites.iter().map(|_| self.a.local(VT::I32)).collect();
        for (s, f) in suites.iter().zip(&flags) {
            self.defer_flags.insert(*s, *f);
        }
        self.scopes.insert(
            i,
            ScopeInfo {
                code,
                exits: Vec::new(),
            },
        );
        let fl = flags.clone();
        self.fresh_only(|em| {
            for f in fl {
                em.a.i32(0);
                em.a.set(f);
            }
            Ok(())
        })?;
        self.a.block();
        self.open(Ctl::Scope(i));
        self.block_into(body, Some(i))?;
        self.a.i32(0);
        self.a.set(code);
        self.ctrl.pop();
        self.a.end();
        for (s, f) in suites.iter().zip(&flags).rev() {
            self.a.get(*f);
            self.a.if_();
            self.open(Ctl::Plain);
            self.block_into(*s, None)?;
            self.ctrl.pop();
            self.a.end();
        }
        let exits = self
            .scopes
            .get(&i)
            .map(|s| s.exits.clone())
            .unwrap_or_default();
        for (k, e) in exits.into_iter().enumerate() {
            self.a.get(code);
            self.a.i32(i32::try_from(k + 1).unwrap_or(0));
            self.a.s().i32_eq();
            self.a.if_();
            self.open(Ctl::Plain);
            self.exit(e)?;
            self.ctrl.pop();
            self.a.end();
        }
        Ok(())
    }

    /// `$.with(K = p, ...): block`: each provider becomes a trait value
    /// that covers its key in the block.
    fn with(&mut self, i: u32, rec: u32, body: u32) -> StageResult<()> {
        let Some(Target::Withs(vts)) = self.calls.get(&i).cloned() else {
            return unsupported("a `$.with` that collection did not record");
        };
        let pairs = self.rec(rec);
        let mut pushed = 0;
        for ((k, v), (trait_, slots)) in pairs.chunks(2).map(|c| (c[0], c[1])).zip(vts) {
            let key_t = Ty(k);
            let TyData::TraitValue { args: targs, .. } = self.pool().get(key_t) else {
                return unsupported("a `$.with` key that is not a trait");
            };
            let vt = self.lay.vtable(trait_, targs)?;
            let ls = [self.a.local(VT::Eq), self.a.local(VT::r(vt.clone()))];
            let fv = self.vts(self.ty_of(v))?;
            self.erase(v, &fv)?;
            self.a.set(ls[0]);
            self.vtable_value(trait_, targs, &vt, &slots, &fv)?;
            self.a.set(ls[1]);
            self.providers.push((trait_, ls));
            pushed += 1;
        }
        self.result(i)?;
        self.a.block();
        self.open(Ctl::Plain);
        let r = self.block_into(body, Some(i));
        self.ctrl.pop();
        self.a.end();
        for _ in 0..pushed {
            self.providers.pop();
        }
        r
    }

    /// Pushes a vtable struct of adapters over the slots' targets.
    fn vtable_value(
        &mut self,
        trait_: DefId,
        targs: TyList,
        vt: &WTy,
        slots: &[CallTarget],
        fv: &[VT],
    ) -> StageResult<()> {
        let methods = self.env().trait_methods(trait_);
        for (m, t) in methods.iter().zip(slots) {
            let sig = self.lay.slot_sig(trait_, targs, *m)?;
            let target = Self::adapter_target(t)?;
            let mut ps = Vec::new();
            for p in self.env().params(t.item).unwrap_or_default() {
                ps.extend(self.vts(subst(self.pool(), self.env(), t.item, t.args, p))?);
            }
            let mut results = self.vts(t.ret)?;
            if self.env().suspends(*m) {
                results = vec![VT::r(suspend_base(&results).0)];
            }
            self.a.ref_func(Sym::Helper(Helper::Adapter {
                sig,
                self_vts: fv.to_vec(),
                target: Box::new(target),
                params: ps,
                results,
            }));
        }
        self.a.struct_new(vt);
        Ok(())
    }

    // ------------------------------------------------------ suspension

    fn susp_parts(&self) -> StageResult<(WTy, WTy, u32, u32)> {
        let Some(s) = &self.susp else {
            return unsupported("a suspension point outside a suspending body");
        };
        Ok((s.frame_ty.clone(), s.base.clone(), s.frame, s.pc))
    }

    /// Saves every saved local into the frame (suspension.md §14.3).
    fn save(&mut self) -> StageResult<()> {
        let (ft, _, frame, _) = self.susp_parts()?;
        let saved = self
            .susp
            .as_ref()
            .map(|s| s.saved.clone())
            .unwrap_or_default();
        for (j, (l, _)) in saved.iter().enumerate() {
            self.a.raw_get(frame);
            self.a.raw_get(*l);
            self.a.struct_set(&ft, F_SAVED + u32_of(j));
        }
        Ok(())
    }

    /// Reloads every saved local from the frame.
    fn reload(&mut self) -> StageResult<()> {
        let (ft, _, frame, _) = self.susp_parts()?;
        let saved = self
            .susp
            .as_ref()
            .map(|s| s.saved.clone())
            .unwrap_or_default();
        for (j, (l, _)) in saved.iter().enumerate() {
            self.a.raw_get(frame);
            self.a.struct_get(&ft, F_SAVED + u32_of(j));
            self.a.set(*l);
        }
        Ok(())
    }

    /// Pending at state `k` with the child on the stack: allocates the
    /// frame on the first real wait (§14.3), saves, and returns Pending.
    fn suspend(&mut self, k: u32, child: u32) -> StageResult<()> {
        let (ft, base, frame, _) = self.susp_parts()?;
        self.a.raw_get(frame);
        self.a.s().ref_is_null();
        self.a.if_();
        self.a.struct_new_default(&ft);
        self.a.set(frame);
        self.a.raw_get(frame);
        self.a.ref_func(Sym::Part(self.key, Part::Cancel));
        self.a.struct_set(&ft, F_CANCEL);
        self.a.raw_get(frame);
        self.a.ref_func(Sym::Part(self.key, Part::Poll));
        self.a.struct_set(&ft, F_POLL);
        self.a.end();
        self.save()?;
        self.a.raw_get(frame);
        self.a.raw_get(child);
        self.a.struct_set(&ft, F_CHILD);
        self.a.raw_get(frame);
        self.a.i32(k.cast_signed());
        self.a.struct_set(&ft, F_STATE);
        for v in self.results.clone() {
            self.a.zero(&v);
        }
        self.a.raw_get(frame);
        let _ = base;
        self.a.s().return_();
        Ok(())
    }

    /// Cancels the child task in local `c` (a `$Task`), when present.
    fn cancel_task(&mut self, c: u32) {
        let task = task_base();
        self.a.raw_get(c);
        self.a.s().ref_is_null();
        self.a.s().i32_eqz();
        self.a.if_();
        self.a.raw_get(c);
        self.a.raw_get(c);
        self.a.s().ref_as_non_null();
        self.a.struct_get(&task, F_CANCEL);
        self.a.call_ref(&cancel_fn());
        self.a.end();
    }

    /// A suspension point (suspension.md §14.2 to §14.6).
    fn point(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        let Some(k) = self.susp.as_ref().and_then(|s| s.states.get(&i).copied()) else {
            return unsupported("a suspension point outside a suspending body");
        };
        let (ft, _, frame, pc) = self.susp_parts()?;
        let tag = self.b.tags[i as usize];
        let task = task_base();
        let children = if tag == Tag::AwaitAll {
            self.rec(bw)
        } else {
            vec![]
        };
        if children.len() > 31 {
            return unsupported("`all!` of more than 31 suspensions");
        }
        let done = self.a.local(VT::I32);
        // Resumed for cancellation: cancel the child, then leave through
        // the enclosing scopes' ladders, which run the registered suites.
        self.a.get(pc);
        self.a.i32(k.cast_signed());
        self.a.s().i32_eq();
        self.a.if_();
        self.open(Ctl::Plain);
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_FLAGS);
        self.a.i32(CANCELLED);
        self.a.s().i32_and();
        self.a.if_();
        self.open(Ctl::Plain);
        let c = self.a.local(VT::rn(task.clone()));
        if tag == Tag::AwaitAll {
            for (j, ch) in children.iter().enumerate() {
                self.a.get(done);
                self.a.i32(1 << j);
                self.a.s().i32_and().i32_eqz();
                self.a.if_();
                self.comp(*ch, 0, &VT::rn(task.clone()))?;
                self.a.set(c);
                self.cancel_task(c);
                self.a.end();
            }
        } else {
            self.a.raw_get(frame);
            self.a.struct_get(&ft, F_CHILD);
            self.a.set(c);
            self.cancel_task(c);
        }
        self.exit(Exit::Cancel)?;
        self.ctrl.pop();
        self.a.end();
        self.ctrl.pop();
        self.a.end();
        let ty = self.sub(self.b.ty[i as usize]);
        let res = self.vts(ty)?;
        match tag {
            Tag::AwaitAll => self.await_all(i, k, &children, done, ty),
            Tag::AwaitValue => {
                self.await_stored(i, k, &res, |em, base| em.comp(a, 0, &VT::r(base.clone())))
            }
            _ => {
                let Some(c) = Callee::from_words(self.b.record(a)) else {
                    return unsupported("a malformed callee record");
                };
                let rec = self.rec(bw);
                let args = rec[..rec.len().saturating_sub(3)].to_vec();
                if let Callee::TraitMethod {
                    trait_,
                    method,
                    choice: (ChoiceKind::TraitValue, _),
                    ..
                } = c
                {
                    return self.await_stored(i, k, &res, |em, _| {
                        em.push_dyn(trait_, method, &args).map(|_| ())
                    });
                }
                let Some(Target::Call(t)) = self.calls.get(&i).cloned() else {
                    return unsupported("a bang call that collection did not resolve");
                };
                if t.kind != TargetKind::Instance || !self.env().suspends(t.item) {
                    return unsupported("a bang call of a compiler lowering");
                }
                self.await_direct(i, k, &t, &args, &res)
            }
        }
    }

    /// `g!(x)` of a known suspending instance: its body called directly,
    /// with the saved child frame on resume (§14.3).
    fn await_direct(
        &mut self,
        i: u32,
        k: u32,
        t: &CallTarget,
        args: &[u32],
        res: &[VT],
    ) -> StageResult<()> {
        let (ft, _, frame, pc) = self.susp_parts()?;
        let gres = self.vts(t.ret)?;
        let (gbase, _) = suspend_base(&gres);
        let ch = self.a.local(VT::rn(gbase.clone()));
        self.a.ref_null(&gbase);
        self.a.set(ch);
        self.a.get(pc);
        self.a.i32(k.cast_signed());
        self.a.s().i32_eq();
        self.a.if_();
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_CHILD);
        self.a.ref_cast(&gbase, true);
        self.a.set(ch);
        self.a.end();
        self.a.raw_get(ch);
        let pool = self.pool();
        let mut ps = Vec::new();
        for p in self.env().params(t.item).unwrap_or_default() {
            ps.push(subst(pool, self.env(), t.item, t.args, p));
        }
        for (r, p) in args.iter().zip(&ps) {
            let want: Vec<VT> = self.vts(*p)?.iter().map(VT::dflt).collect();
            self.load_as(*r, &want)?;
        }
        let keys = self.env().row_keys(t.item, t.args);
        self.push_providers(&keys)?;
        self.a.call(Sym::Part(t.key, Part::Body));
        let tmp: Vec<u32> = gres.iter().map(|v| self.a.local(v.dflt())).collect();
        let nc = self.a.local(VT::rn(gbase));
        self.a.set(nc);
        for l in tmp.iter().rev() {
            self.a.set(*l);
        }
        self.a.raw_get(nc);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.suspend(k, nc)?;
        self.a.end();
        self.a.i32(0);
        self.a.set(pc);
        for l in &tmp {
            self.a.raw_get(*l);
        }
        let dres: Vec<VT> = gres.iter().map(VT::dflt).collect();
        let _ = res;
        self.store_from(i, &dres)
    }

    /// A stored suspension (`s!()`, a trait-value bang call): polled
    /// through its vtable; `fresh` pushes it on the fresh path.
    fn await_stored(
        &mut self,
        i: u32,
        k: u32,
        res: &[VT],
        fresh: impl FnOnce(&mut Self, &WTy) -> StageResult<()>,
    ) -> StageResult<()> {
        let (ft, _, frame, pc) = self.susp_parts()?;
        let (base, poll) = suspend_base(res);
        let s = self.a.local(VT::rn(base.clone()));
        self.a.get(pc);
        self.a.i32(k.cast_signed());
        self.a.s().i32_eq();
        self.a.if_();
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_CHILD);
        self.a.ref_cast(&base, true);
        self.a.set(s);
        self.a.else_();
        fresh(self, &base)?;
        self.a.set(s);
        self.a.end();
        self.a.raw_get(s);
        self.a.s().ref_as_non_null();
        self.a.raw_get(s);
        self.a.s().ref_as_non_null();
        self.a.struct_get(&base, F_POLL);
        self.a.call_ref(&poll);
        let tmp: Vec<u32> = res.iter().map(|v| self.a.local(v.dflt())).collect();
        let ready = self.a.local(VT::I32);
        for l in tmp.iter().rev() {
            self.a.set(*l);
        }
        self.a.set(ready);
        self.a.get(ready);
        self.a.s().i32_eqz();
        self.a.if_();
        self.suspend(k, s)?;
        self.a.end();
        self.a.i32(0);
        self.a.set(pc);
        for l in &tmp {
            self.a.raw_get(*l);
        }
        let dres: Vec<VT> = res.iter().map(VT::dflt).collect();
        self.store_from(i, &dres)
    }

    /// `all!(a, b, ...)` (suspension.md §14.5): polls the unfinished
    /// children in argument order, keeps each result, and completes once
    /// every child has.
    fn await_all(
        &mut self,
        i: u32,
        k: u32,
        children: &[u32],
        done: u32,
        ty: Ty,
    ) -> StageResult<()> {
        let pc = self.pc();
        let Shape::Tuple { elems, boxed } = self.lay.shape(ty)? else {
            return unsupported("an `all!` result that is not a tuple");
        };
        self.fresh_only(|em| {
            em.a.i32(0);
            em.a.set(done);
            Ok(())
        })?;
        let mut keep = Vec::new();
        for (j, (ch, rv)) in children.iter().zip(&elems).enumerate() {
            let (base, poll) = suspend_base(rv);
            let ls: Vec<u32> = rv.iter().map(|v| self.a.local(v.dflt())).collect();
            let tmp: Vec<u32> = rv.iter().map(|v| self.a.local(v.dflt())).collect();
            let s = self.a.local(VT::r(base.clone()));
            let bit = 1i32 << j;
            self.a.get(done);
            self.a.i32(bit);
            self.a.s().i32_and().i32_eqz();
            self.a.if_();
            self.comp(*ch, 0, &VT::r(base.clone()))?;
            self.a.set(s);
            self.a.get(s);
            self.a.get(s);
            self.a.struct_get(&base, F_POLL);
            self.a.call_ref(&poll);
            for l in tmp.iter().rev() {
                self.a.set(*l);
            }
            self.a.if_();
            self.a.get(done);
            self.a.i32(bit);
            self.a.s().i32_or();
            self.a.set(done);
            for (d, t) in ls.iter().zip(&tmp) {
                self.a.raw_get(*t);
                self.a.set(*d);
            }
            self.a.end();
            self.a.end();
            keep.push(ls);
        }
        let all = (1i32 << children.len()) - 1;
        let none = self.a.local(VT::rn(task_base()));
        self.a.get(done);
        self.a.i32(all);
        self.a.s().i32_ne();
        self.a.if_();
        self.suspend(k, none)?;
        self.a.end();
        self.a.i32(0);
        self.a.set(pc);
        for (ls, rv) in keep.iter().zip(&elems) {
            for (l, v) in ls.iter().zip(rv) {
                self.a.raw_get(*l);
                self.a.conv(&v.dflt(), v);
            }
        }
        if let Some(b) = boxed {
            self.a.struct_new(&b);
        }
        self.store(i)
    }
}

/// `Emit(inst)`: walks the generic TIR of sub-body `sub` under the
/// instance's arguments. A suspending function's instance (suspension.md
/// §14.1) emits its cold constructor as the code entry and `f$body`,
/// `f$poll` and `f$cancel` as its parts.
pub fn emit(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    layouts: &Layouts,
    b: &Body,
    sub: u16,
    args: TyList,
    ret: Ty,
    calls: &HashMap<u32, Target>,
    key: Hash128,
) -> StageResult<Code> {
    let lay = Lay::new(pool, env, path, layouts);
    let s = |t: Ty| subst(pool, env, b.item, args, t);
    let suspends = if sub == 0 {
        env.suspends(b.item)
    } else {
        let ci = closure_of(b, sub)?;
        matches!(pool.get(s(b.ty[ci])), TyData::Fn { suspends: true, .. })
    };
    if suspends {
        return emit_suspending(&lay, b, sub, args, ret, calls, key);
    }
    let (params, results, plocals) = signature(&lay, b, sub, args, ret)?;
    let mut em = Em::new(
        Lay::new(pool, env, path, layouts),
        b,
        args,
        calls,
        key,
        Asm::new(params.clone()),
        results.clone(),
    );
    // Parameters are the first Wasm locals, in order.
    let mut next = u32::from(sub != 0);
    let mut copies = Vec::new();
    for &l in &plocals {
        let n = u32_of(em.lay.vts(s(b.local_ty[l as usize]))?.len());
        let ls: Vec<u32> = (next..next + n).collect();
        next += n;
        copies.push((l, ls));
    }
    for (l, ls) in copies {
        if em.cells.contains_key(&l) {
            // A captured, assigned parameter moves into its cell.
            let cell = em.cell_ty(l)?;
            let cl = em.local(l)?[0];
            for x in &ls {
                em.a.get(*x);
            }
            em.a.struct_new(&cell);
            em.a.set(cl);
        } else {
            em.locals[l as usize] = Some(ls);
        }
    }
    if sub == 0 {
        for k in env.row_keys(b.item, args) {
            em.providers.push((k, [next, next + 1]));
            next += 2;
        }
    } else {
        // The closure's environment: captured values into their locals.
        let ci = closure_of(b, sub)?;
        let (env_ty, cl) = em.closure_env(ci)?;
        let e = em.a.local(VT::r(env_ty.clone()));
        em.a.get(0);
        em.a.ref_cast(&env_ty, false);
        em.a.set(e);
        for (f, l) in cl {
            em.a.get(e);
            em.a.struct_get(&env_ty, f);
            em.a.set(l);
        }
        // The closure's own row: each key's provider from the context.
        let (keys_at, provs_at) = (next, next + 1);
        for k in em.fn_row_keys(s(b.ty[ci])) {
            let vt = em.lay.vtable(k, TyList::EMPTY)?;
            let ls = ctx_provider(&mut em.a, env, k, &vt, keys_at, provs_at);
            em.providers.push((k, ls));
        }
    }
    let root = b.sub_root[sub as usize];
    em.ctrl.push(Ctl::Plain);
    em.block_into(root, None)?;
    let tail = b.data[root as usize][1];
    if tail == NONE {
        if !results.is_empty() {
            em.a.s().unreachable();
        }
    } else {
        em.load_as(tail, &results)?;
    }
    Ok(em.a.finish(results))
}

/// The provider of key `k` from a closure call's context (the parameters
/// `keys_at` and `provs_at`, codegen.md §12.4): two fresh locals holding
/// its value and its vtable. The caller's row covers `k`, so the search
/// ends at its slot.
fn ctx_provider(
    a: &mut Asm,
    env: &dyn ProgramEnv,
    k: DefId,
    vt: &WTy,
    keys_at: u32,
    provs_at: u32,
) -> [u32; 2] {
    let pl = a.local(VT::Eq);
    let vl = a.local(VT::r(vt.clone()));
    let at = a.local(VT::I32);
    a.i32(0);
    a.set(at);
    a.block();
    a.loop_();
    a.raw_get(keys_at);
    a.raw_get(at);
    a.array_get(&ctx_keys());
    a.i64(key_id(env, k));
    a.s().i64_eq();
    a.br_if(1);
    a.raw_get(at);
    a.i32(1);
    a.s().i32_add();
    a.set(at);
    a.br(0);
    a.end();
    a.end();
    a.raw_get(provs_at);
    a.raw_get(at);
    a.i32(2);
    a.s().i32_mul();
    a.array_get(&ctx_provs());
    a.set(pl);
    a.raw_get(provs_at);
    a.raw_get(at);
    a.i32(2);
    a.s().i32_mul();
    a.i32(1);
    a.s().i32_add();
    a.array_get(&ctx_provs());
    a.ref_cast(vt, false);
    a.set(vl);
    [pl, vl]
}

/// A function reference's adapter instance (codegen.md §13.11): closure
/// code `(env, params..., keys, providers)` that forwards its parameters,
/// receiver first for a method (`fn.ref.unbound.receiver`), and its row's
/// providers from the context to one call of the item's instance at
/// `args`, which collection recorded as instruction 0. The context passes
/// through, so one adapter serves every caller row. A suspending item's
/// call is its cold constructor, which the adapter returns
/// (`fn.ref.suspending`).
pub fn emit_adapter(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    layouts: &Layouts,
    item: DefId,
    args: TyList,
    calls: &HashMap<u32, Target>,
) -> StageResult<Code> {
    let lay = Lay::new(pool, env, path, layouts);
    let Some(Target::Call(t)) = calls.get(&0) else {
        return unsupported("a function reference that collection did not resolve");
    };
    if t.kind != TargetKind::Instance {
        return unsupported("a function reference to a compiler lowering");
    }
    let s = |x: Ty| subst(pool, env, item, args, x);
    let params: Vec<Ty> = env
        .params(item)
        .unwrap_or_default()
        .into_iter()
        .map(s)
        .collect();
    let ft = pool.intern_ty(&TyData::Fn {
        params: pool.list(&params),
        result: s(env.ret(item).unwrap_or(Ty::VOID)),
        row: RowId::EMPTY,
        suspends: env.suspends(item),
    });
    let Shape::Fn { code, .. } = lay.shape(ft)? else {
        return unsupported("a function reference without a function type");
    };
    let WTy::Func(cps, crs) = code else {
        return unsupported("a closure code type");
    };
    let mut want = Vec::new();
    for p in env.params(t.item).unwrap_or_default() {
        want.extend(lay.vts(subst(pool, env, t.item, t.args, p))?);
    }
    let mut got = lay.vts(t.ret)?;
    if env.suspends(t.item) {
        got = vec![VT::r(suspend_base(&got).0)];
        // A frame over an erased result is another type, not a subtype.
        if got != crs {
            return unsupported("a reference to a suspending instance with an erased result");
        }
    }
    let (keys_at, provs_at) = (u32_of(cps.len() - 2), u32_of(cps.len() - 1));
    if want.len() + 3 != cps.len() || got.len() != crs.len() {
        return unsupported("a function reference whose instance has another shape");
    }
    let mut a = Asm::new(cps.clone());
    let mut provs = Vec::new();
    for k in env.row_keys(t.item, t.args) {
        let vt = lay.vtable(k, TyList::EMPTY)?;
        provs.push(ctx_provider(&mut a, env, k, &vt, keys_at, provs_at));
    }
    for (j, w) in want.iter().enumerate() {
        a.get(u32_of(j + 1));
        a.conv(&cps[j + 1], w);
    }
    for [pl, vl] in provs {
        a.get(pl);
        a.get(vl);
    }
    a.call(Sym::Inst(t.key));
    let tmp: Vec<u32> = got.iter().map(|v| a.local(v.clone())).collect();
    for l in tmp.iter().rev() {
        a.set(*l);
    }
    for (l, (have, w)) in tmp.iter().zip(got.iter().zip(&crs)) {
        a.get(*l);
        a.conv(have, w);
    }
    Ok(a.finish(crs))
}

/// What a suspending closure's cold constructor binds besides its
/// parameters: the environment struct with each captured value's field
/// and body local, and each row key's provider from the call's context
/// with its vtable type and two body locals.
struct ClosureBind {
    env: WTy,
    caps: Vec<(u32, u32)>,
    provs: Vec<(DefId, WTy, [u32; 2])>,
}

/// One emission pass of a suspending body: its code, every Wasm local it
/// saves with its type, and a closure's bindings.
type Pass = (Code, Vec<(u32, VT)>, Option<ClosureBind>);

/// A suspending function or closure body (suspension.md §14.1 to §14.6).
/// The body is emitted twice: the first pass learns its Wasm locals, which
/// the frame saves; the second writes the state machine over that frame.
///
/// A closure (`req.suspend.closure.marker`) takes the same path. Its code
/// is the cold constructor, with the closure ABI `(env, params..., keys,
/// providers)`; `f$body` takes only the parameters. The constructor
/// stores the parameters, the captured values and the providers of the
/// closure's row into the frame, so they are bound when the suspension is
/// constructed (`req.bind.construction`) and every poll reloads them as it
/// reloads any saved local. A closure value is only ever called through
/// `CallValue`, which builds the frame, so its body never takes the
/// null-frame path.
fn emit_suspending(
    lay: &Lay<'_>,
    b: &Body,
    sub: u16,
    args: TyList,
    ret: Ty,
    calls: &HashMap<u32, Target>,
    key: Hash128,
) -> StageResult<Code> {
    let (pool, env) = (lay.pool, lay.env);
    let s = |t: Ty| subst(pool, env, b.item, args, t);
    let (cold_params, _, plocals) = signature(lay, b, sub, args, ret)?;
    // The body's own parameters: a function's cold parameters, or a
    // closure's parameters without its environment and context.
    let (closure, result, own) = if sub == 0 {
        (None, s(ret), cold_params.clone())
    } else {
        let ci = closure_of(b, sub)?;
        let TyData::Fn { result, .. } = pool.get(s(b.ty[ci])) else {
            return unsupported("a closure without a function type");
        };
        let own = cold_params[1..cold_params.len() - 2].to_vec();
        (Some(ci), result, own)
    };
    let results = lay.vts(result)?;
    let (base, _) = suspend_base(&results);
    let dres: Vec<VT> = results.iter().map(VT::dflt).collect();
    let mut logical = vec![VT::rn(base.clone())];
    logical.extend(own.iter().cloned());
    let mut bres = dres.clone();
    bres.push(VT::rn(base.clone()));
    let root = b.sub_root[sub as usize];
    let (states, ranges) = plan(b, root)?;
    let frame_ty = |saved: &[(u32, VT)]| {
        let mut extra = vec![VT::rn(task_base())];
        extra.extend(saved.iter().map(|x| x.1.clone()));
        frame_of(&base, &extra)
    };
    let pass = |saved: Vec<(u32, VT)>| -> StageResult<Pass> {
        let ft = frame_ty(&saved);
        let mut a = Asm::new_dflt(logical.clone());
        let frame = a.local(VT::rn(ft.clone()));
        let pc = a.local(VT::I32);
        let mut em = Em::new(
            Lay::new(pool, env, lay.path, lay.shared),
            b,
            args,
            calls,
            key,
            a,
            dres.clone(),
        );
        em.susp = Some(Susp {
            frame_ty: ft.clone(),
            base: base.clone(),
            frame,
            pc,
            states: states.clone(),
            ranges: ranges.clone(),
            saved,
        });
        let mut next = 1;
        for &l in &plocals {
            if em.cells.contains_key(&l) {
                return unsupported("a mutably captured parameter of a suspending function");
            }
            let n = u32_of(em.lay.vts(s(b.local_ty[l as usize]))?.len());
            em.locals[l as usize] = Some((next..next + n).collect());
            next += n;
        }
        let bind = if let Some(ci) = closure {
            // Locals for the captured values and the row's providers,
            // which the cold constructor fills through the frame.
            let (env_ty, caps) = em.closure_env(ci)?;
            let mut provs = Vec::new();
            for k in em.fn_row_keys(s(b.ty[ci])) {
                let vt = em.lay.vtable(k, TyList::EMPTY)?;
                let ls = [em.a.local(VT::Eq), em.a.local(VT::r(vt.clone()))];
                em.providers.push((k, ls));
                provs.push((k, vt, ls));
            }
            Some(ClosureBind {
                env: env_ty,
                caps,
                provs,
            })
        } else {
            for k in env.row_keys(b.item, args) {
                em.providers.push((k, [next, next + 1]));
                next += 2;
            }
            None
        };
        // Prologue: a frame resumes from its state with its locals.
        em.a.raw_get(0);
        em.a.ref_cast(&ft, true);
        em.a.set(frame);
        em.a.raw_get(frame);
        em.a.s().ref_is_null().i32_eqz();
        em.a.if_();
        em.reload()?;
        em.a.raw_get(frame);
        em.a.struct_get(&ft, F_STATE);
        em.a.set(pc);
        em.a.end();
        em.ctrl.push(Ctl::Plain);
        em.block_into(root, None)?;
        let tail = b.data[root as usize][1];
        if tail == NONE {
            if dres.is_empty() {
                em.a.ref_null(&base);
            } else {
                em.a.s().unreachable();
            }
        } else {
            em.load_as(tail, &dres)?;
            em.a.ref_null(&base);
        }
        let np = em.a.nparams();
        let mut all = Vec::new();
        for l in 1..np {
            all.push((l, em.a.decl(l)));
        }
        for j in 0..u32_of(em.a.locals.len()) {
            let l = np + j;
            if l != frame && l != pc {
                all.push((l, em.a.decl(l)));
            }
        }
        Ok((em.a.finish(bres.clone()), all, bind))
    };
    let (_, saved, _) = pass(Vec::new())?;
    let (body, again, bind) = pass(saved.clone())?;
    if again != saved {
        return unsupported("a suspending body whose two emission passes differ");
    }
    let ft = frame_ty(&saved);
    let field = |l: u32| {
        saved
            .iter()
            .position(|x| x.0 == l)
            .map(|j| F_SAVED + u32_of(j))
    };
    // The cold constructor: a frame in state 0 holding the arguments.
    let mut a = Asm::new(cold_params.clone());
    let f = a.local(VT::r(ft.clone()));
    a.struct_new_default(&ft);
    a.set(f);
    a.get(f);
    a.ref_func(Sym::Part(key, Part::Cancel));
    a.struct_set(&ft, F_CANCEL);
    a.get(f);
    a.ref_func(Sym::Part(key, Part::Poll));
    a.struct_set(&ft, F_POLL);
    // Cold parameter `first + j` is the body's local `j + 1`.
    let first = u32::from(bind.is_some());
    let slot = |l: u32| match field(l) {
        Some(fi) => Ok(fi),
        None => unsupported("a suspending body's local without a frame field"),
    };
    for j in 0..u32_of(own.len()) {
        a.get(f);
        a.get(first + j);
        a.struct_set(&ft, slot(j + 1)?);
    }
    if let Some(bind) = bind {
        let e = a.local(VT::r(bind.env.clone()));
        a.get(0);
        a.ref_cast(&bind.env, false);
        a.set(e);
        for (fi, l) in bind.caps {
            a.get(f);
            a.get(e);
            a.struct_get(&bind.env, fi);
            a.struct_set(&ft, slot(l)?);
        }
        let (keys_at, provs_at) = (u32_of(cold_params.len() - 2), u32_of(cold_params.len() - 1));
        for (k, vt, ls) in bind.provs {
            let got = ctx_provider(&mut a, env, k, &vt, keys_at, provs_at);
            for (g, l) in got.into_iter().zip(ls) {
                a.get(f);
                a.get(g);
                a.struct_set(&ft, slot(l)?);
            }
        }
    }
    a.get(f);
    let cold = a.finish(vec![VT::r(base.clone())]);
    // f$poll: the runtime checks, then the body over the frame.
    let mut a = Asm::new(vec![VT::Eq]);
    let f = a.local(VT::r(ft.clone()));
    let tmp: Vec<u32> = dres.iter().map(|v| a.local(v.clone())).collect();
    let nf = a.local(VT::rn(base.clone()));
    a.get(0);
    a.ref_cast(&ft, false);
    a.set(f);
    flag_checks(&mut a, f, &ft, true);
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(ACTIVE);
    a.s().i32_or();
    a.struct_set(&ft, F_FLAGS);
    a.get(f);
    for v in &logical[1..] {
        a.zero(&v.dflt());
    }
    a.call(Sym::Part(key, Part::Body));
    a.set(nf);
    for l in tmp.iter().rev() {
        a.set(*l);
    }
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(!ACTIVE);
    a.s().i32_and();
    a.struct_set(&ft, F_FLAGS);
    a.raw_get(nf);
    a.s().ref_is_null();
    a.if_();
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(DONE);
    a.s().i32_or();
    a.struct_set(&ft, F_FLAGS);
    a.i32(1);
    for l in &tmp {
        a.raw_get(*l);
    }
    a.s().return_();
    a.end();
    a.i32(0);
    for v in &dres {
        a.zero(v);
    }
    let mut pres = vec![VT::I32];
    pres.extend(dres.iter().cloned());
    let poll_code = a.finish(pres);
    // f$cancel (§14.6): a frame that never waited only gets marked.
    let mut a = Asm::new(vec![VT::Eq]);
    let f = a.local(VT::r(ft.clone()));
    a.get(0);
    a.ref_cast(&ft, false);
    a.set(f);
    flag_checks(&mut a, f, &ft, false);
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(CANCELLED);
    a.s().i32_or();
    a.struct_set(&ft, F_FLAGS);
    a.get(f);
    a.struct_get(&ft, F_STATE);
    a.s().i32_eqz();
    a.if_();
    a.s().return_();
    a.end();
    a.get(f);
    for v in &logical[1..] {
        a.zero(&v.dflt());
    }
    a.call(Sym::Part(key, Part::Body));
    for _ in 0..bres.len() {
        a.s().drop();
    }
    let cancel = a.finish(vec![]);
    let mut code = cold;
    code.parts = vec![
        (Part::Body, body),
        (Part::Poll, poll_code),
        (Part::Cancel, cancel),
    ];
    Ok(code)
}

/// The runtime checks of `f$poll` and `f$cancel` (§14.3, §14.6): an
/// active frame is a re-entrant poll; a poll of a completed or cancelled
/// frame is invalid, and a cancel of one does nothing.
fn flag_checks(a: &mut Asm, f: u32, ft: &WTy, poll: bool) {
    a.get(f);
    a.struct_get(ft, F_FLAGS);
    a.i32(ACTIVE);
    a.s().i32_and();
    a.if_();
    a.call(Sym::Helper(Helper::Panic(
        "suspension-reentrant-poll: a suspension was polled or cancelled while active".into(),
    )));
    a.s().unreachable();
    a.end();
    a.get(f);
    a.struct_get(ft, F_FLAGS);
    a.i32(DONE | CANCELLED);
    a.s().i32_and();
    a.if_();
    if poll {
        a.call(Sym::Helper(Helper::Panic(
            "suspension-invalid-state: a completed or cancelled suspension was polled".into(),
        )));
        a.s().unreachable();
    } else {
        a.s().return_();
    }
    a.end();
}

/// The entry exports (codegen.md §13.1, suspension.md §14.4): `hd.init`
/// runs every reachable group's init in order; `hd.poll` provides one
/// default-profile provider per row key, each a vtable of host stubs
/// generated from `hd_host_abi::TABLE` (runtime-and-host.md §17.1), and
/// runs `main` or polls `main!`, then turns its result into the exit
/// status with `report` (a `std.rt` instance; `None` for `void`);
/// `hd.wake(n)` records completed handles.
pub fn entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    main: DefId,
    key: hd_base::Hash128,
    inits: &[hd_base::Hash128],
    report: Option<hd_base::Hash128>,
) -> StageResult<Vec<(String, Helper)>> {
    Ok(vec![
        (
            "hd.init".into(),
            Helper::EntryInit {
                inits: inits.to_vec(),
            },
        ),
        (
            "hd.poll".into(),
            root_poll(pool, env, path, main, key, report)?,
        ),
        ("hd.wake".into(), Helper::EntryWake),
    ])
}

/// The entry exports of a script (module.init.script): `hd.init` runs the
/// reachable groups' inits except the script's own, which is last, since
/// every reachable group is a dependency of it. `hd.poll` runs that init
/// with its entry providers, then exits with 0 as `main` with no result.
pub fn script_entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    init: DefId,
    key: hd_base::Hash128,
    inits: &[hd_base::Hash128],
) -> StageResult<Vec<(String, Helper)>> {
    let others: Vec<hd_base::Hash128> = inits.iter().copied().filter(|k| *k != key).collect();
    Ok(vec![
        ("hd.init".into(), Helper::EntryInit { inits: others }),
        (
            "hd.poll".into(),
            root_poll(pool, env, path, init, key, None)?,
        ),
        ("hd.wake".into(), Helper::EntryWake),
    ])
}

/// The exports of a test program (engines-and-test-runner.md §19.1):
/// `hd.init.j`, the init groups that test module `j` reaches; `hd.test.i`,
/// which polls case `i` as `hd.poll` polls `main`; and `hd.wake`.
pub fn test_entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    cases: &[(DefId, hd_base::Hash128, Option<hd_base::Hash128>)],
    inits: &[Vec<hd_base::Hash128>],
) -> StageResult<Vec<(String, Helper)>> {
    let mut out = Vec::new();
    for (j, ks) in inits.iter().enumerate() {
        out.push((
            format!("hd.init.{j}"),
            Helper::EntryInit { inits: ks.clone() },
        ));
    }
    for (i, (def, key, report)) in cases.iter().enumerate() {
        out.push((
            format!("hd.test.{i}"),
            root_poll(pool, env, path, *def, *key, *report)?,
        ));
    }
    out.push(("hd.wake".into(), Helper::EntryWake));
    Ok(out)
}

/// The poll export of one root: its row's default providers, then the
/// root's call or poll, then its report.
fn root_poll(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    main: DefId,
    key: hd_base::Hash128,
    report: Option<hd_base::Hash128>,
) -> StageResult<Helper> {
    let shared = Layouts::default();
    let lay = Lay::new(pool, env, path, &shared);
    let mut providers = Vec::new();
    for k in env.row_keys(main, TyList::EMPTY) {
        let p = path(k);
        let Some(host) = hd_host_abi::TABLE
            .iter()
            .find(|t| t.std_path.replace('.', "/") == p)
        else {
            return unsupported(format!("a default provider for `{p}`"));
        };
        let vt = lay.vtable(k, TyList::EMPTY)?;
        let mut slots = Vec::new();
        for m in env.trait_methods(k) {
            let sig = lay.slot_sig(k, TyList::EMPTY, m)?;
            let mp = path(m);
            let name = mp.rsplit(['.', '/']).next().unwrap_or("");
            // A method the table does not list runs its default body,
            // which for `write_error_line!` is the first method's call.
            let Some(hm) = host
                .methods
                .iter()
                .find(|x| x.name == name)
                .or_else(|| host.methods.first())
            else {
                return unsupported(format!("the host method `{name}`"));
            };
            slots.push(host_slot(&lay, host.key, hm, m, sig)?);
        }
        providers.push((vt, slots));
    }
    let ret = env.ret(main).unwrap_or(Ty::VOID);
    let results = lay.vts(ret)?;
    let bang = if env.suspends(main) {
        Some(suspend_base(&results))
    } else {
        None
    };
    Ok(Helper::EntryPoll {
        main: key,
        providers,
        results: u32_of(results.len()),
        bang,
        report: report.map(|k| (k, results)),
    })
}

/// One vtable slot of a default-profile provider: a host stub, or a
/// stub that panics for a method shape not lowered yet.
fn host_slot(
    lay: &Lay<'_>,
    key: &str,
    hm: &hd_host_abi::HostMethod,
    m: DefId,
    sig: WTy,
) -> StageResult<Helper> {
    let (pool, env) = (lay.pool, lay.env);
    let unlowered = |sig: WTy| {
        Ok(Helper::Unlowered {
            sig,
            what: format!("the host method `{key}.{}`", hm.name),
        })
    };
    if !env.suspends(m) || hm.wait != hd_host_abi::Wait::May {
        return unlowered(sig);
    }
    let full = pool.list(&[hd_mono::class_ref(pool)]);
    let mut args = Vec::new();
    let params: Vec<Ty> = env
        .params(m)
        .unwrap_or_default()
        .into_iter()
        .skip(1)
        .collect();
    for (p, c) in params.iter().zip(hm.params) {
        let pt = subst(pool, env, m, full, *p);
        let vts = lay.vts(pt)?;
        args.push(match (c, vts.as_slice()) {
            (hd_host_abi::Codec::Buffer("string"), _) => crate::rt::ArgCodec::Str,
            (hd_host_abi::Codec::Scalar(_), [VT::Ref(t, false)]) if t.fields() == [VT::I64] => {
                crate::rt::ArgCodec::DataI64((**t).clone())
            }
            (hd_host_abi::Codec::Scalar(_), [v @ (VT::I32 | VT::I64 | VT::F64)]) => {
                crate::rt::ArgCodec::Scalar(v.clone())
            }
            _ => return unlowered(sig),
        });
    }
    if params.len() != hm.params.len() {
        return unlowered(sig);
    }
    let ret = subst(pool, env, m, full, env.ret(m).unwrap_or(Ty::VOID));
    let result = lay.vts(ret)?;
    if !(result.is_empty() || result == [VT::I32, VT::I32]) {
        return unlowered(sig);
    }
    let (base, _) = suspend_base(&result);
    let mut extra: Vec<VT> = args
        .iter()
        .flat_map(crate::rt::ArgCodec::vts)
        .map(|v| v.dflt())
        .collect();
    extra.push(VT::I32);
    let frame = frame_of(&base, &extra);
    let poller = Helper::HostPoll {
        frame: frame.clone(),
        module: format!("hd:{key}"),
        method: hm.name.to_owned(),
        args,
        result,
    };
    Ok(Helper::HostCold {
        sig,
        frame: frame.clone(),
        poll: Box::new(poller),
        cancel: Box::new(Helper::HostCancel { frame }),
    })
}
