//! `Emit(inst)` (`crates/hd_mono/DESIGN.md` §12.1, §12.2): one walk of an instance's
//! generic TIR under its substitution. Every value instruction's result
//! lives in Wasm locals, one per value of its layout (`multi` layouts have
//! several), so structured control needs only empty block types. Calls go
//! to collection's recorded targets; nothing is selected here. A tag or
//! form this walk does not lower yet answers the structured `unsupported`.

use hd_base::HashMap;

use hd_base::{
    BuildProfile, DefId, Hash128, NodeIdx, NotImplemented, StableHasher, Stage, StageResult,
};
use hd_mono::layout::{CanonMemo, canon, inline_map_key};
use hd_mono::{
    CallTarget, INFO, ProgramEnv, SlotRecv, Target, TargetKind, VTable, class_ref,
    declared_row_keys, erased_args, has_row_params, key_order, row_args_open, row_is_open,
    row_param_keys, subst, thunk_capture_locals, thunk_operands, witness_args, witness_family,
};
use hd_tir::ir::{
    Body, BodyKind, Callee, ChoiceKind, Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, local_flags,
};
use hd_types::{Inputs, InternPool, Prim, RowId, Ty, TyData, TyList};

use crate::asm::Asm;
use crate::layout::{
    ABANDONED, ACTIVE, CANCELLED, DONE, DynSlot, EnumShape, F_CANCEL, F_CHILD, F_FLAGS, F_POLL,
    F_SAVED, F_STATE, Lay, Layouts, M_GROWN, M_HASHES, M_KEYS, M_LIVE, M_USED, OptShape, Shape,
    UNWINDING, box_of, cancel_fn, ctx_keys, ctx_provs, frame_of, key_id, storage, suspend_base,
    task_base, witness_ty,
};
use crate::lowering::{ClosureCode, Lowering};
use crate::rt::{Helper, KeyOps, OptForm, RefRead, block_import, unerase};
use crate::{Code, GSym, Part, Sym, VT, WTy, unsupported};

mod cow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctl {
    Plain,
    /// The block whose end starts arm `k` of match instruction `m`.
    Arm(u32, u32),
    Brk(u32),
    Cont(u32),
    /// The cleanup block of `Scope` instruction `s` (`crates/hd_mono/DESIGN.md` §12.2,
    /// "The exit ladder"): a branch to it runs the scope's suites.
    Scope(u32),
}

/// A way out of a cleanup scope: each is one rung of its exit ladder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exit {
    Ret,
    Brk(u32),
    Cont(u32),
    /// Cancellation at a suspension point (`crates/hd_wasm/DESIGN.md` §14.6): it runs
    /// the same suites, then the body returns.
    Cancel,
}

/// A cleanup scope's ladder: the exit-code local and the exits taken.
struct ScopeInfo {
    code: u32,
    exits: Vec<Exit>,
}

/// A suspending body's state machine (`crates/hd_wasm/DESIGN.md` §14.2, §14.3).
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
    /// Covering providers, innermost last: the whole key (the trait value
    /// type with its arguments, `crates/hd_mono/DESIGN.md` §13.18), its two locals.
    providers: Vec<(Ty, [u32; 2])>,
    /// The instance's own key (a suspending body names its parts).
    key: Hash128,
    /// The function's result values (defaultable in a suspending body).
    results: Vec<VT>,
    /// Locals holding a return value across an exit ladder.
    ret: Option<Vec<u32>>,
    scopes: HashMap<u32, ScopeInfo>,
    /// Each `defer` suite's "registered" flag local.
    defer_flags: HashMap<u32, u32>,
    /// While a `using` binding's dispose suite is emitted: its suspension
    /// points are shielded from cancellation (flow.using.cancel).
    shield: u32,
    /// While a point outside a dispose suite emits its await: the `ctrl`
    /// index of the block whose end starts the point's cancellation
    /// branch (`crates/hd_wasm/DESIGN.md` §14.6, "Awaiting a cancelled
    /// suspension").
    cancel_to: Option<usize>,
    susp: Option<Susp>,
    /// An erased body's thunks (`crates/hd_mono/DESIGN.md` §13.5.1): the local holding
    /// its witness field, that field's struct type, and each open
    /// instruction's thunk type.
    ops: Option<(u32, WTy, Vec<WTy>)>,
    /// The context of an erased body's own row parameters, or of the
    /// erased body a thunk serves (`crates/hd_mono/DESIGN.md` §13.5.1): its
    /// key ids and providers locals. A function value whose row lists
    /// an open row parameter gets it, and a key no provider covers is
    /// looked up in it.
    open_ctx: Option<[u32; 2]>,
    /// How many of `providers` the body had on entry: those after them
    /// are a `$.with` scope's, which a key of the open context may name
    /// too, so they go in front of it.
    open_base: usize,
    /// The instruction being emitted and the block depth it started at:
    /// its result is written at declaration when stored there (#274).
    here: Option<(u32, usize)>,
}

/// The locals a closure captures by shared cell, each with its first
/// `LocalSet` in the body that declares it (the declaration, which makes a
/// fresh cell). A `LocalSet` inside a closure that captures the local is
/// an assignment through the capture, never the declaration, however early
/// the closure's instructions come: a `for` pattern's bindings are set
/// after the loop body is built, so its closures' stores precede them. A
/// parameter moves into its cell on entry, so no store of it makes a fresh
/// one: a closure that writes a `mut` parameter, such as the `set` closure
/// of `&mut p` (`crates/hd_check/src/refs.rs`), writes that cell.
fn cell_locals(b: &Body) -> HashMap<u32, u32> {
    let mut cells = HashMap::default();
    for (c, l) in b.cap_local.iter().enumerate() {
        if b.cap_mode.get(c) == Some(&hd_tir::ir::CaptureMode::Shared) {
            cells.insert(l.raw(), u32::MAX);
        }
    }
    if cells.is_empty() {
        return cells;
    }
    let subs = b.inst_subs();
    let mut through_capture = hd_base::HashSet::default();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Closure {
            let rec = b.record(b.data[i][1]);
            let (start, len) = (rec[0] as usize, rec[1] as usize);
            for l in &b.cap_local[start..start + len] {
                through_capture.insert((b.data[i][0], l.raw()));
            }
        }
    }
    for (i, &sub) in subs.iter().enumerate() {
        if b.tags[i] == Tag::LocalSet
            && b.local_flags
                .get(b.data[i][0] as usize)
                .copied()
                .unwrap_or(0)
                & hd_tir::ir::local_flags::PARAM
                == 0
            && !through_capture.contains(&(sub, b.data[i][0]))
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

/// The low `bits` bits set, for a sub-word width.
fn width_mask(bits: u8) -> i32 {
    (1i32 << bits) - 1
}

fn u32_of(i: usize) -> u32 {
    u32::try_from(i).expect("index")
}

fn i32_of(i: usize) -> i32 {
    i32::try_from(i).expect("index")
}

/// The integer kind of a scalar type: bits, signed, float. Nominal
/// newtypes erase to their base, as values do.
fn num(lay: &Lay<'_>, t: Ty) -> (u8, bool, bool) {
    let t = lay.erase(t);
    let pool = lay.pool;
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
            Prim::String => (32, true, false),
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

    /// The cell type of a shared local: one mutable field per component,
    /// a nullable one for a reference. A cell can exist before its local's
    /// first value: a recursive local function captures its own name's
    /// cell before the closure is made (`fn.local.capture-rules`).
    fn cell_ty(&self, l: u32) -> StageResult<WTy> {
        let fields = self.cell_fields(l)?;
        Ok(WTy::Struct {
            fields,
            sup: None,
            open: false,
        })
    }

    /// The field types of [`Self::cell_ty`].
    fn cell_fields(&self, l: u32) -> StageResult<Vec<VT>> {
        Ok(self
            .vts(self.sub(self.b.local_ty[l as usize]))?
            .into_iter()
            .map(|v| match v {
                VT::Ref(t, _) => VT::Ref(t, true),
                v => v,
            })
            .collect())
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
                        self.a.lit(s.as_bytes());
                    } else {
                        self.a.i64(i64::try_from(s.len()).unwrap_or(0) << 32);
                    }
                }
                shape => self.scalar_const(&shape, bits)?,
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

    /// Pushes a constant's bits as a value of a scalar shape.
    fn scalar_const(&mut self, shape: &Shape, bits: u64) -> StageResult<()> {
        match shape {
            Shape::Scalar(VT::I64) => self.a.i64(bits.cast_signed()),
            Shape::Scalar(VT::F64) => {
                self.a.s().f64_const(f64::from_bits(bits).into());
            }
            // A float literal's constant holds its `f64` bits whatever
            // type the literal ends up with (`hd_check`'s literal).
            Shape::Scalar(VT::F32) => {
                self.a.s().f64_const(f64::from_bits(bits).into());
                self.a.s().f32_demote_f64();
            }
            Shape::Scalar(_) | Shape::Enum(EnumShape { boxed: None, .. }) => {
                let low = u32::try_from(bits & 0xffff_ffff).expect("32 bits");
                self.a.i32(low.cast_signed());
            }
            Shape::Void => {}
            _ => return unsupported("a constant of this type"),
        }
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
    /// Whether `i`'s result has no locals yet and a store now is in the
    /// block `i` started in, which holds its reads (`Asm` demotes the
    /// local when one is outside): its locals are then written at
    /// declaration (#274).
    fn fresh_here(&self, i: u32) -> bool {
        !self.vals.contains_key(&i) && self.here == Some((i, self.a.depth()))
    }
    /// Pops the instruction's values (pushed in its layout) into its locals.
    fn store(&mut self, i: u32) -> StageResult<()> {
        if self.fresh_here(i) {
            let vs = self.vts(self.sub(self.b.ty[i as usize]))?;
            let ls = self.a.locals_init(&vs);
            self.vals.insert(i, (ls, vs));
            return Ok(());
        }
        let ls = self.result(i)?;
        for l in ls.iter().rev() {
            self.a.set(*l);
        }
        Ok(())
    }
    /// Stores the values on the stack, in `have`, as `i`'s result.
    fn store_from(&mut self, i: u32, have: &[VT]) -> StageResult<()> {
        let want = match self.vals.get(&i) {
            Some((_, vs)) => vs.clone(),
            None => self.vts(self.sub(self.b.ty[i as usize]))?,
        };
        if have == want.as_slice() {
            return self.store(i);
        }
        let fresh = self.fresh_here(i) && have.len() == want.len();
        if !fresh {
            self.result(i)?;
        }
        let tmp = self.a.locals_init(have);
        let ls = if fresh {
            Vec::new()
        } else {
            self.vals[&i].0.clone()
        };
        for (k, (t, h)) in tmp.iter().zip(have).enumerate() {
            let Some(w) = want.get(k) else {
                break;
            };
            self.a.get(*t);
            self.a.conv(h, w);
            if let Some(l) = ls.get(k) {
                self.a.set(*l);
            }
        }
        if fresh {
            let ls = self.a.locals_init(&want);
            self.vals.insert(i, (ls, want));
        }
        Ok(())
    }

    /// Whether the value `r`, as checked, has a function type whose inputs
    /// are one tuple of an arity not known there, as in
    /// `f: Fn[Args, O, $ R]`: a call of it passes that tuple.
    fn tuple_inputs(&self, r: u32) -> bool {
        let pool = self.lay.pool;
        let Some(i) = Ref(r).as_inst() else {
            return false;
        };
        let t = self.lay.strip(self.b.ty[i.0 as usize]);
        matches!(
            pool.get(t),
            TyData::Fn {
                inputs: Inputs::Tuple,
                ..
            }
        )
    }

    /// Pushes the components of every element of the tuple `r`, as the
    /// leading value types of `want`.
    fn spread_elems(&mut self, r: u32, want: &[VT]) -> StageResult<()> {
        let Shape::Tuple { elems, boxed } = self.lay.shape(self.ty_of(r))? else {
            return unsupported("a spread of a non-tuple");
        };
        let have: Vec<VT> = elems.into_iter().flatten().collect();
        let Some(want) = want.get(..have.len()) else {
            return unsupported("a spread tuple that does not fit the callee's inputs");
        };
        for (k, (h, w)) in have.iter().zip(want).enumerate() {
            match &boxed {
                Some(b) => {
                    self.comp(r, 0, &VT::r(b.clone()))?;
                    self.a.struct_get(b, u32_of(k));
                    self.a.conv(h, w);
                }
                None => self.comp(r, k, w)?,
            }
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

    /// `If` `i`, and the `If`s nested in each `else` block after it, as an
    /// `else if` chain lowers (`flow.if.else-if`): a loop, not a recursion,
    /// so a long chain takes no stack per branch. Each step does what
    /// `inst` and `block_into` would for the `else` block's last `If`.
    fn if_chain(&mut self, first: u32) -> StageResult<()> {
        // An `else` block that continues into an `If`: the `If` it holds,
        // its items after that `If`, and the `inst` state to restore.
        struct Open {
            i: u32,
            els: u32,
            rest: Vec<u32>,
            at: Option<u32>,
            here: Option<(u32, usize)>,
        }
        let mut open: Vec<Open> = Vec::new();
        let mut i = first;
        let mut done = loop {
            let next = match self.if_head(i) {
                Ok(Some(els)) => self.else_chain(i, els).map(|n| n.map(|n| (els, n))),
                Ok(None) => {
                    self.ctrl.pop();
                    self.a.end();
                    Ok(None)
                }
                Err(e) => Err(e),
            };
            let (els, (inner, rest)) = match next {
                Ok(Some(n)) => n,
                Ok(None) => break Ok(()),
                Err(e) => break Err(e),
            };
            let at = self.a.at;
            if self
                .b
                .syn
                .get(inner as usize)
                .is_some_and(|n| *n != NodeIdx::NONE)
            {
                self.a.at = Some(inner);
            }
            let here = self.here.replace((inner, self.a.depth()));
            open.push(Open {
                i,
                els,
                rest,
                at,
                here,
            });
            i = inner;
        };
        while let Some(o) = open.pop() {
            self.a.at = o.at;
            self.here = o.here;
            if done.is_ok() {
                done = self.else_tail(o.i, o.els, &o.rest);
            }
        }
        done
    }

    /// The start of `If` `i` through its `then` block, and `else` if it
    /// has one: the `else` block, still to emit.
    fn if_head(&mut self, i: u32) -> StageResult<Option<u32>> {
        let [a, bw] = self.b.data[i as usize];
        let r = self.rec(bw);
        self.result(i)?;
        self.comp(a, 0, &VT::I32)?;
        self.resume_branch(i, r[0]);
        self.a.if_();
        self.open(Ctl::Plain);
        self.block_into(r[0], Some(i))?;
        let Some(&els) = r.get(1).filter(|e| **e != NONE) else {
            return Ok(None);
        };
        self.a.else_();
        Ok(Some(els))
    }

    /// Emits the items of `If` `i`'s `else` block `els` up to its last
    /// `If`, which is returned with the items after it for the chain to
    /// continue with. A block with no `If`, or one a resume enters, is
    /// emitted whole, closing `i`.
    fn else_chain(&mut self, i: u32, els: u32) -> StageResult<Option<(u32, Vec<u32>)>> {
        if self.b.tags[els as usize] != Tag::Block {
            return unsupported("a block operand that is not a block");
        }
        let items: Vec<u32> = self
            .rec(self.b.data[els as usize][0])
            .into_iter()
            .filter(|x| self.b.tags[*x as usize] != Tag::Block)
            .collect();
        let last_if = items
            .iter()
            .rposition(|x| self.b.tags[*x as usize] == Tag::If)
            .filter(|_| self.range(els).is_none());
        let Some(p) = last_if else {
            self.block_into(els, Some(i))?;
            self.ctrl.pop();
            self.a.end();
            return Ok(None);
        };
        for &x in &items[..p] {
            self.inst(x)?;
        }
        Ok(Some((items[p], items[p + 1..].to_vec())))
    }

    /// Finishes `If` `i` after the `If` its `else` block `els` continued
    /// into: the block's remaining items, its tail, and the `end`.
    fn else_tail(&mut self, i: u32, els: u32, rest: &[u32]) -> StageResult<()> {
        for &x in rest {
            self.inst(x)?;
        }
        let tail = self.b.data[els as usize][1];
        if tail != NONE {
            let want = self.vts(self.sub(self.b.ty[i as usize]))?;
            if !want.is_empty() {
                self.load_as(tail, &want)?;
                self.store(i)?;
            }
        }
        self.ctrl.pop();
        self.a.end();
        Ok(())
    }

    fn panic(&mut self, msg: &str) {
        self.a.call(Sym::Helper(Helper::Panic(msg.to_owned())));
        self.a.s().unreachable();
    }

    /// Whether this is a release build, whose integer arithmetic wraps and
    /// whose shift counts are masked (`types.arith.release`,
    /// `types.arith.shift-count.release`).
    fn release(&self) -> bool {
        self.lay.env.build_profile() == BuildProfile::Release
    }

    /// Emits instruction `i`, which anchors the calls it writes when it has
    /// a source position (`crates/hd_wasm/DESIGN.md` §15.9).
    fn inst(&mut self, i: u32) -> StageResult<()> {
        let outer = self.a.at;
        if self
            .b
            .syn
            .get(i as usize)
            .is_some_and(|n| *n != NodeIdx::NONE)
        {
            self.a.at = Some(i);
        }
        let here = self.here.replace((i, self.a.depth()));
        let r = self.inst_code(i);
        self.a.at = outer;
        self.here = here;
        r
    }

    fn inst_code(&mut self, i: u32) -> StageResult<()> {
        if let Some(&Target::Open(k)) = self.calls.get(&i)
            && self.b.tags[i as usize] != Tag::SwitchTag
        {
            let rs = self.open_call(i, k)?;
            return self.store_from(i, &rs);
        }
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
                    let fields = self.cell_fields(a)?;
                    let want = self.vts(self.sub(self.b.local_ty[a as usize]))?;
                    for (k, (f, w)) in fields.iter().zip(&want).enumerate() {
                        self.a.get(ls[0]);
                        self.a.struct_get(&cell, u32_of(k));
                        self.a.conv(f, w);
                    }
                } else {
                    for l in &ls {
                        self.a.get(*l);
                    }
                }
                self.store(i)?;
            }
            Tag::LocalSet => {
                let t = self.sub(self.b.local_ty[a as usize]);
                let want = self.vts(t)?;
                // A local's first write with a value declares its Wasm
                // locals there, written at declaration (#274).
                let first = !self.cells.contains_key(&a)
                    && bw != NONE
                    && !matches!(self.locals.get(a as usize), Some(Some(_)));
                if first {
                    self.load_as(bw, &want)?;
                    let ls = self.a.locals_init(&want);
                    if self.locals.len() <= a as usize {
                        self.locals.resize(a as usize + 1, None);
                    }
                    self.locals[a as usize] = Some(ls);
                    return Ok(());
                }
                let ls = self.local(a)?;
                if bw == NONE {
                    // No value yet: the components are written null.
                    for l in &ls {
                        self.a.demote(*l);
                    }
                }
                self.load_as(bw, &want)?;
                if let Some(first) = self.cells.get(&a).copied() {
                    // The cell's own field types: a declaration with no
                    // value yet (`NONE`) stores null references.
                    let fields = self.cell_fields(a)?;
                    let tmp: Vec<u32> = fields.into_iter().map(|v| self.a.local(v)).collect();
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
                let r = self.result(i)?;
                self.comp(a, 0, &VT::I32)?;
                if self.b.tags[i as usize] == Tag::Or {
                    self.a.s().i32_eqz();
                }
                self.resume_branch(i, bw);
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
                if let [tuple] = args[..]
                    && self.tuple_inputs(a)
                {
                    // Instantiation fixed the inputs tuple: its elements
                    // are the arguments (`fn.type.ctor.inputs`).
                    self.spread_elems(tuple, &ps[1..])?;
                } else {
                    let mut k = 1;
                    for r in args {
                        let n = self.vts(self.ty_of(r))?.len();
                        let want: Vec<VT> = ps[k..k + n].to_vec();
                        self.load_as(r, &want)?;
                        k += n;
                    }
                }
                let callee = self.ty_of(a);
                self.push_ctx(callee)?;
                // A closure collection resolved (the optimized pipeline's
                // A1) is a direct call of its code; any other reads the
                // code from the value, casting an abstract one to its final
                // type.
                if let Some(&Target::KnownClosure(key)) = self.calls.get(&i) {
                    self.a.call(Sym::Inst(key));
                } else {
                    self.a.get(f);
                    self.a.struct_get(&base, 0);
                    if self.lay.closure_code() == ClosureCode::Abstract {
                        self.a.ref_cast(&code, false);
                    }
                    self.a.call_ref(&code);
                }
                self.store_from(i, &rs.clone())?;
            }
            Tag::Interp => {
                let parts = self.rec(bw);
                self.concat(i, &parts)?;
            }
            Tag::Intrinsic => self.intrinsic(i, a, bw, ty)?,
            Tag::Coerce => self.coerce(i, a, bw, ty)?,
            Tag::NewData => {
                if self.lay.is_newtype(ty) {
                    // An erased newtype has no struct of its own:
                    // construction forwards the one base value.
                    let rec = self.rec(bw);
                    let [r] = rec.as_slice() else {
                        return unsupported("a newtype construction without one value");
                    };
                    let vs = self.vts(ty)?;
                    self.load_as(*r, &vs)?;
                    return self.store(i);
                }
                let Shape::Data { ty: st, fields } = self.lay.shape(ty)? else {
                    return unsupported("a data value without a struct layout");
                };
                if fields.is_empty() {
                    return self.canonical_data(i, ty, st);
                }
                let (ftys, in_data) = self.field_tys(ty);
                for (k, (r, (_, vs))) in self.rec(bw).into_iter().zip(fields).enumerate() {
                    self.load_as(r, &vs)?;
                    if let (1, Some(&ft)) = (vs.len(), ftys.get(k)) {
                        self.copy_slot(r, ft, in_data)?;
                    }
                }
                self.a.struct_new(&st);
                self.store(i)?;
            }
            Tag::Is => {
                // `ref.identity.same-place`: two references compare the
                // places they refer to, not the `Ref` objects.
                if matches!(self.lay.pool.get(self.lay.strip(self.ty_of(a))), TyData::Adt { def, .. } if def == self.env().ref_cell())
                {
                    return self.ref_identity_eq(i, a, bw);
                }
                // `expr.is.*`: the underlying references, never the
                // trait-value pair; component 0 of every operand layout
                // is the reference (the payload of a trait value).
                self.comp(a, 0, &VT::Eq)?;
                self.comp(bw, 0, &VT::Eq)?;
                self.a.s().ref_eq();
                self.store_from(i, &[VT::I32])?;
            }
            Tag::CopyData => {
                let Shape::Data { ty: st, fields } = self.lay.shape(ty)? else {
                    return unsupported("a copied value without a struct layout");
                };
                if fields.is_empty() {
                    return self.canonical_data(i, ty, st);
                }
                // Replacements are `(field, value)` pairs; every other field
                // is read from the source (`crates/hd_mono/DESIGN.md` `CopyData`).
                let words = self.rec(bw);
                let repl: Vec<(u32, u32)> = words.chunks(2).map(|w| (w[0], w[1])).collect();
                let (ftys, in_data) = self.field_tys(ty);
                for (idx, (start, vs)) in fields.iter().enumerate() {
                    if let Some((_, v)) = repl.iter().find(|(f, _)| *f as usize == idx) {
                        self.load_as(*v, vs)?;
                        if let (1, Some(&ft)) = (vs.len(), ftys.get(idx)) {
                            self.copy_slot(*v, ft, in_data)?;
                        }
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
                if self.lay.is_newtype(base_t) {
                    // Unwrapping an erased newtype forwards the value
                    // itself: there is no wrapper struct to read.
                    let vs = self.vts(ty)?;
                    if self.b.tags[i as usize] == Tag::Field {
                        self.load_as(a, &vs)?;
                    } else {
                        let r = self.rec(bw);
                        self.load_as(r[1], &vs)?;
                    }
                    return self.store_from(i, &vs);
                }
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
                        let (ftys, in_data) = self.field_tys(base_t);
                        for (k, w) in vs.iter().enumerate() {
                            self.comp(a, 0, &VT::r(st.clone()))?;
                            self.comp(v, k, w)?;
                            if let (1, Some(&ft)) = (vs.len(), ftys.get(idx as usize)) {
                                self.copy_slot(v, ft, in_data)?;
                            }
                            self.a.struct_set(&st, start + u32_of(k));
                        }
                    }
                }
            }
            Tag::NewTuple => {
                let Shape::Tuple { elems, boxed } = self.lay.shape(ty)? else {
                    return unsupported("a tuple without a tuple layout");
                };
                // The rest list, if any, is the last member.
                let mut members = self.rec(bw);
                if a != NONE {
                    members.push(a);
                }
                let mtys = self.member_tys(ty, None);
                for (k, (r, vs)) in members.into_iter().zip(elems).enumerate() {
                    self.load_as(r, &vs)?;
                    if let (1, Some(&mt)) = (vs.len(), mtys.get(k)) {
                        self.copy_slot(r, mt, false)?;
                    }
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
                if a != NONE {
                    let spreads = self.rec(a);
                    return self.spread_list(i, &items, &spreads, &elem, &lt);
                }
                self.a.i32(i32::try_from(items.len()).unwrap_or(0));
                let ety = self.coll_arg(ty, 0);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    for r in &items {
                        self.comp(*r, k, v)?;
                        if let (1, Some(et)) = (elem.len(), ety) {
                            self.copy_slot(*r, et, false)?;
                        }
                    }
                    self.a.array_new_fixed(&WTy::Array(st), u32_of(items.len()));
                }
                // `cow` and `pin`.
                self.a.i32(0);
                self.a.i32(0);
                self.a.struct_new(&lt);
                self.store(i)?;
            }
            Tag::NewMap => self.map_literal(i, &self.rec(bw), ty)?,
            // A generic local function's defining closure makes no value;
            // each use instantiates it (`sub_flags::GENERIC`).
            Tag::Closure
                if self.b.sub_is_generic(a as usize) && !self.b.is_instantiation(i as usize) => {}
            Tag::Closure => self.closure(i, bw, ty)?,
            // No captures: a bound reference is a `Closure` (§13.11).
            Tag::ItemRef => self.closure_value(i, 0..0, ty)?,
            Tag::ProviderGet => {
                let key = self.rec(a).first().copied().unwrap_or(NONE);
                let key_t = self.sub(Ty(key));
                if !matches!(self.pool().get(key_t), TyData::TraitValue { .. }) {
                    return unsupported("a provider key that is not a trait");
                }
                self.push_providers(&[key_t])?;
                self.store(i)?;
            }
            Tag::If => self.if_chain(i)?,
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
            Tag::ContextNew => self.context_new(i, a, ty)?,
            Tag::Guard => {
                let r = self.rec(bw);
                if self.range(i).is_some() {
                    // The condition runs once: fresh, or resuming into
                    // it; a resume into the failure branch skips it.
                    let c = self.a.local(VT::I32);
                    self.entered(a, |em| {
                        em.block_value(a)?;
                        em.a.set(c);
                        Ok(())
                    })?;
                    self.a.get(c);
                    self.resume_branch(i, r[0]);
                } else {
                    self.block_value(a)?;
                }
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
                // Over an open value, the tag read is a thunk (§13.5.1).
                if let Some(&Target::Open(k)) = self.calls.get(&i) {
                    self.open_call(i, k)?;
                } else {
                    self.tag_of(a)?;
                }
                self.a.i32(r[0].cast_signed());
                self.a.s().i32_eq();
                self.resume_branch(i, r[1]);
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
                self.resume_branch(i, r[1]);
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

    /// Pushes variant `variant` of the enum type `ty`, laid out as `e`.
    /// `field(self, f, k, exact)` pushes value `k` of payload field `f`
    /// as `exact`.
    fn push_variant(
        &mut self,
        ty: Ty,
        e: &EnumShape,
        variant: u32,
        field: &mut dyn FnMut(&mut Self, usize, usize, &VT) -> StageResult<()>,
    ) -> StageResult<()> {
        let fields = e.fields.get(variant as usize).cloned().unwrap_or_default();
        if let (Some(base), Some(sub)) = (&e.boxed, e.subtypes.get(variant as usize)) {
            // A subtype box: the tag, then the payload's exact values; a
            // payloadless variant is the base alone, one constant global
            // per enum type and tag.
            if let Some(sub) = sub {
                self.a.i32(variant.cast_signed());
                for (f, (_, vs)) in fields.iter().enumerate() {
                    for (k, x) in vs.iter().enumerate() {
                        field(self, f, k, x)?;
                    }
                }
                self.a.struct_new(sub);
            } else {
                let h = self.type_hash("payloadless-variant", ty);
                self.a
                    .global_get(GSym::Variant(h, variant, VT::r(base.clone())));
            }
            return Ok(());
        }
        self.a.i32(variant.cast_signed());
        for (s, st) in e.slots.iter().enumerate() {
            let src = fields.iter().enumerate().find_map(|(f, (slots, vs))| {
                slots
                    .iter()
                    .position(|x| *x == s)
                    .map(|k| (f, k, vs[k].clone()))
            });
            match src {
                Some((f, k, exact)) => {
                    field(self, f, k, &exact)?;
                    self.a.conv(&exact, st);
                }
                None => self.a.zero(st),
            }
        }
        if let Some(b) = &e.boxed {
            self.a.struct_new(b);
        }
        Ok(())
    }

    fn new_variant(&mut self, i: u32, variant: u32, vals: u32, ty: Ty) -> StageResult<()> {
        let args = self.rec(vals);
        match self.lay.shape(ty)? {
            Shape::Enum(e) => {
                let ptys = self.member_tys(ty, Some(variant));
                let mut single = Vec::new();
                for p in &ptys {
                    single.push(self.vts(*p)?.len() == 1);
                }
                self.push_variant(ty, &e, variant, &mut |s, f, k, x| match args.get(f) {
                    Some(r) => {
                        s.comp(*r, k, x)?;
                        match (single.get(f), ptys.get(f)) {
                            (Some(true), Some(&pt)) => s.copy_slot(*r, pt, false),
                            _ => Ok(()),
                        }
                    }
                    None => unsupported("a variant payload without its value"),
                })?;
                self.store(i)
            }
            Shape::Opt(o, inner) => {
                let some = variant == 1;
                match &o {
                    OptShape::NullRef(r) => {
                        if some {
                            self.comp(args[0], 0, r)?;
                            if let Some(&pt) = self.member_tys(ty, None).first() {
                                self.copy_slot(args[0], pt, false)?;
                            }
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
        let (bits, signed, float) = num(&self.lay, t0);
        let vt0 = self.vts(t0)?;
        if !matches!(vt0.as_slice(), [VT::I32 | VT::I64 | VT::F32 | VT::F64]) {
            return unsupported(format!("the operator {op:?} on this layout"));
        }
        let wide = vt0[0] == VT::I64;
        if float && op != PrimOp::Conv {
            return self.float_value(op, ops, &vt0[0]);
        }
        let s = |em: &mut Self| -> StageResult<()> {
            for r in ops {
                em.comp(*r, 0, &vt0[0])?;
            }
            Ok(())
        };
        // A shift's count is any unsigned type (`expr.shift.count-unsigned`).
        // It must be below the shifted value's bit width, checked in its own
        // type before it is brought to the value's width
        // (`types.arith.shift-count.debug`). A release build masks it to
        // the width instead (`types.arith.shift-count.release`): Wasm's
        // shifts mask at 32 and 64 bits, so only a sub-word width needs it.
        let shift = |em: &mut Self| -> StageResult<()> {
            em.comp(ops[0], 0, &vt0[0])?;
            let count = em.vts(em.ty_of(ops[1]))?;
            let Some(cv) = count.first().cloned() else {
                return unsupported("a shift count without a value");
            };
            let n = em.a.local(cv.clone());
            em.comp(ops[1], 0, &cv)?;
            em.a.set(n);
            em.a.get(n);
            if em.release() {
                if bits < 32 {
                    if cv == VT::I64 {
                        em.a.i64(i64::from(bits) - 1);
                        em.a.s().i64_and();
                    } else {
                        em.a.i32(i32::from(bits) - 1);
                        em.a.s().i32_and();
                    }
                }
            } else {
                if cv == VT::I64 {
                    em.a.i64(i64::from(bits));
                    em.a.s().i64_ge_u();
                } else {
                    em.a.i32(i32::from(bits));
                    em.a.s().i32_ge_u();
                }
                em.a.if_();
                em.panic("invalid-shift: shift count out of range");
                em.a.end();
                em.a.get(n);
            }
            match (&cv, wide) {
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
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Neg if self.release() => {
                self.wrapping(op, ops, bits, signed)?;
            }
            PrimOp::Add | PrimOp::Sub | PrimOp::Mul | PrimOp::Div | PrimOp::Rem | PrimOp::Neg => {
                self.checked(op, ops, bits, signed)?;
            }
            PrimOp::Pow => self.int_pow(ops, bits, signed)?,
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
            // Bits shifted past a sub-word width are dropped, as at 32 and
            // 64 bits, so the result is brought back to canonical form.
            PrimOp::Shl => {
                shift(self)?;
                if wide {
                    self.a.s().i64_shl();
                } else {
                    self.a.s().i32_shl();
                    self.wrap_to(bits, signed);
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
                    // A sign-extended value stays so under `~`; a
                    // zero-extended one flips only its own bits.
                    self.a.i32(if signed || bits >= 32 {
                        -1
                    } else {
                        width_mask(bits)
                    });
                    self.a.s().i32_xor();
                }
            }
            PrimOp::Conv => self.convert(ops[0], &vt0[0], (signed, float), ty)?,
        }
        Ok(())
    }

    /// A numeric cast of `ops[0]` to the type `ty` (`types.cast.*`). The
    /// source's representation is `from`, with its `signed` and `float`
    /// kind. No cast panics (`types.cast.no-panic`).
    fn convert(
        &mut self,
        op: u32,
        from: &VT,
        (signed, float): (bool, bool),
        ty: Ty,
    ) -> StageResult<()> {
        let to = self.vts(ty)?;
        let Some(to) = to.first().cloned() else {
            return unsupported("a numeric cast to a type without a value");
        };
        let (tbits, tsigned, tfloat) = num(&self.lay, ty);
        self.comp(op, 0, from)?;
        let mut x = self.a.s();
        match (from, &to) {
            (VT::I32, VT::I64) => {
                if signed {
                    x.i64_extend_i32_s()
                } else {
                    x.i64_extend_i32_u()
                };
            }
            (VT::I64, VT::I32) => {
                x.i32_wrap_i64();
            }
            // An integer converts to the nearest float, a tie to the even
            // significand (`types.cast.int-float`): one rounding, by the
            // instruction.
            (VT::I32, VT::F32) => {
                if signed {
                    x.f32_convert_i32_s()
                } else {
                    x.f32_convert_i32_u()
                };
            }
            (VT::I64, VT::F32) => {
                if signed {
                    x.f32_convert_i64_s()
                } else {
                    x.f32_convert_i64_u()
                };
            }
            (VT::I32, VT::F64) => {
                if signed {
                    x.f64_convert_i32_s()
                } else {
                    x.f64_convert_i32_u()
                };
            }
            (VT::I64, VT::F64) => {
                if signed {
                    x.f64_convert_i64_s()
                } else {
                    x.f64_convert_i64_u()
                };
            }
            // `f32` widens exactly; `f64` narrows to the nearest `f32`, a
            // tie to the even significand (`types.cast.f32-f64`,
            // `types.cast.f64-f32`).
            (VT::F32, VT::F64) => {
                x.f64_promote_f32();
            }
            (VT::F64, VT::F32) => {
                x.f32_demote_f64();
            }
            (VT::F32 | VT::F64, VT::I32 | VT::I64) => {
                self.float_to_int(from, tbits, tsigned);
            }
            (a, b) if a == b => {}
            _ => return unsupported("this numeric conversion"),
        }
        // An integer cast keeps the target's low bits
        // (`types.cast.wrap`): `u8(300)` is 44, `i8(200)` is -56. A float
        // source was clamped to the target's range already.
        if !tfloat && !float {
            self.wrap_to(tbits, tsigned);
        }
        Ok(())
    }

    /// Truncates the float on the stack toward zero and saturates it to the
    /// integer type of `bits` and `signed` (`types.cast.saturate`): the
    /// target's bounds for the infinities and out-of-range values, and zero
    /// for NaN. The `trunc_sat` instructions do this at 32 and 64 bits. A
    /// sub-word target saturates in `i32` first, then clamps: that is
    /// monotone, so the bound the clamp meets is the right one.
    fn float_to_int(&mut self, from: &VT, bits: u8, signed: bool) {
        let single = *from == VT::F32;
        let mut x = self.a.s();
        match (bits, single, signed) {
            (64, true, true) => x.i64_trunc_sat_f32_s(),
            (64, true, false) => x.i64_trunc_sat_f32_u(),
            (64, false, true) => x.i64_trunc_sat_f64_s(),
            (64, false, false) => x.i64_trunc_sat_f64_u(),
            (32, true, true) => x.i32_trunc_sat_f32_s(),
            (32, true, false) => x.i32_trunc_sat_f32_u(),
            (32, false, true) => x.i32_trunc_sat_f64_s(),
            (32, false, false) => x.i32_trunc_sat_f64_u(),
            (_, true, _) => x.i32_trunc_sat_f32_s(),
            (_, false, _) => x.i32_trunc_sat_f64_s(),
        };
        if bits >= 32 {
            return;
        }
        let (lo, hi) = if signed {
            (-(1i32 << (bits - 1)), (1i32 << (bits - 1)) - 1)
        } else {
            (0, width_mask(bits))
        };
        let v = self.a.local(VT::I32);
        self.a.set(v);
        // `lo` where `v < lo`, then `hi` where that is above `hi`.
        self.a.i32(lo);
        self.a.get(v);
        self.a.get(v);
        self.a.i32(lo);
        self.a.s().i32_lt_s().select();
        self.a.set(v);
        self.a.i32(hi);
        self.a.get(v);
        self.a.get(v);
        self.a.i32(hi);
        self.a.s().i32_gt_s().select();
    }

    /// Brings an `i32` holding a `bits`-wide integer's low bits back to
    /// canonical form (`crates/hd_mono/DESIGN.md` §13.15): sign-extended when `signed`,
    /// zero-extended otherwise. Every producer of a sub-word value ends
    /// with it, so consumers use the native 32-bit operations. Nothing at
    /// 32 and 64 bits.
    fn wrap_to(&mut self, bits: u8, signed: bool) {
        match (bits, signed) {
            (8, true) => {
                self.a.s().i32_extend8_s();
            }
            (16, true) => {
                self.a.s().i32_extend16_s();
            }
            (8 | 16, false) => {
                self.a.i32(width_mask(bits));
                self.a.s().i32_and();
            }
            _ => {}
        }
    }

    fn float_value(&mut self, op: PrimOp, ops: &[u32], vt: &VT) -> StageResult<()> {
        // `expr.power.float.pow`: the IEEE `pow` helper takes and gives
        // `f64`, so an `f32` operand is widened exactly and the result is
        // rounded back to `f32`. Floating `%` is the same shape: the `fmod`
        // helper works on `f64`, and the remainder of two `f32` values is
        // exactly an `f32` value, so narrowing it loses nothing
        // (`expr.float.remainder-truncated`).
        let helper = match op {
            PrimOp::Pow => Some(Helper::PowF64),
            PrimOp::Rem => Some(Helper::RemF64),
            _ => None,
        };
        if let Some(helper) = helper {
            for r in ops {
                self.comp(*r, 0, vt)?;
                if *vt == VT::F32 {
                    self.a.s().f64_promote_f32();
                }
            }
            self.a.call(Sym::Helper(helper));
            if *vt == VT::F32 {
                self.a.s().f32_demote_f64();
            }
            return Ok(());
        }
        // The basic operations and comparisons are the Wasm instructions of
        // the operand's own width (`expr.float.basic`, `expr.eq.float`):
        // IEEE 754 single or double, with its NaN, infinity and signed zero.
        let single = *vt == VT::F32;
        if op == PrimOp::Neg {
            self.comp(ops[0], 0, vt)?;
            if single {
                self.a.s().f32_neg();
            } else {
                self.a.s().f64_neg();
            }
            return Ok(());
        }
        for r in ops {
            self.comp(*r, 0, vt)?;
        }
        let mut x = self.a.s();
        match (op, single) {
            (PrimOp::Add, false) => x.f64_add(),
            (PrimOp::Sub, false) => x.f64_sub(),
            (PrimOp::Mul, false) => x.f64_mul(),
            (PrimOp::Div, false) => x.f64_div(),
            (PrimOp::Eq, false) => x.f64_eq(),
            (PrimOp::Ne, false) => x.f64_ne(),
            (PrimOp::Lt, false) => x.f64_lt(),
            (PrimOp::Le, false) => x.f64_le(),
            (PrimOp::Gt, false) => x.f64_gt(),
            (PrimOp::Ge, false) => x.f64_ge(),
            (PrimOp::Add, true) => x.f32_add(),
            (PrimOp::Sub, true) => x.f32_sub(),
            (PrimOp::Mul, true) => x.f32_mul(),
            (PrimOp::Div, true) => x.f32_div(),
            (PrimOp::Eq, true) => x.f32_eq(),
            (PrimOp::Ne, true) => x.f32_ne(),
            (PrimOp::Lt, true) => x.f32_lt(),
            (PrimOp::Le, true) => x.f32_le(),
            (PrimOp::Gt, true) => x.f32_gt(),
            (PrimOp::Ge, true) => x.f32_ge(),
            _ => return unsupported(format!("the float operator {op:?}")),
        };
        Ok(())
    }

    /// Release-build `+`, `-`, `*` and unary `-` (`types.arith.release`):
    /// the native operation, which keeps the low 32 or 64 bits, brought back
    /// to a sub-word type's canonical form by `wrap_to`. Division and
    /// remainder stay `checked` in every build (`types.arith.always`).
    fn wrapping(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let wide = bits == 64;
        let vt = if wide { VT::I64 } else { VT::I32 };
        if op == PrimOp::Neg {
            self.a.zero(&vt);
            self.comp(ops[0], 0, &vt)?;
        } else {
            self.comp(ops[0], 0, &vt)?;
            self.comp(ops[1], 0, &vt)?;
        }
        {
            let mut s = self.a.s();
            match (op, wide) {
                (PrimOp::Add, true) => s.i64_add(),
                (PrimOp::Mul, true) => s.i64_mul(),
                (_, true) => s.i64_sub(),
                (PrimOp::Add, false) => s.i32_add(),
                (PrimOp::Mul, false) => s.i32_mul(),
                (_, false) => s.i32_sub(),
            };
        }
        self.wrap_to(bits, signed);
        Ok(())
    }

    /// Checked integer arithmetic (`crates/hd_mono/DESIGN.md` §13.15, `types.arith.checked`).
    /// Sub-word widths compute exactly in `i32` and 32-bit widths in `i64`,
    /// each range-checked against the type; 64-bit widths test the native
    /// result. The operands are canonical, and an in-range result is too.
    /// A release build takes `wrapping` instead, except for division and
    /// remainder.
    fn checked(&mut self, op: PrimOp, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let wide = bits == 64;
        // The operands' representation, and the type the exact result fits.
        let vt = if wide { VT::I64 } else { VT::I32 };
        let exact = if bits == 32 { VT::I64 } else { vt.clone() };
        let x = self.a.local(exact.clone());
        let y = self.a.local(exact.clone());
        let r = self.a.local(exact.clone());
        let (lhs, rhs) = if op == PrimOp::Neg {
            (None, ops[0])
        } else {
            (Some(ops[0]), ops[1])
        };
        for (src, dst) in [(lhs, x), (Some(rhs), y)] {
            match src {
                Some(v) => {
                    self.comp(v, 0, &vt)?;
                    if bits == 32 {
                        if signed {
                            self.a.s().i64_extend_i32_s();
                        } else {
                            self.a.s().i64_extend_i32_u();
                        }
                    }
                }
                None => self.a.zero(&exact),
            }
            self.a.set(dst);
        }
        self.checked_arith(op, bits, signed, (x, y, r));
        self.a.get(r);
        if bits == 32 {
            self.a.s().i32_wrap_i64();
        }
        Ok(())
    }

    /// `r = x op y` on locals of the exact type (`checked`'s), with the
    /// overflow and division tests; it leaves nothing on the stack.
    fn checked_arith(&mut self, op: PrimOp, bits: u8, signed: bool, (x, y, r): (u32, u32, u32)) {
        let wide = bits == 64;
        let in64 = bits >= 32;
        if matches!(op, PrimOp::Div | PrimOp::Rem) {
            self.a.get(y);
            if in64 {
                self.a.s().i64_eqz();
            } else {
                self.a.s().i32_eqz();
            }
            self.a.if_();
            self.panic("integer-division-by-zero: division by zero");
            self.a.end();
        }
        // `MIN / -1` overflows in every build (`types.arith.min-division`).
        // Narrower widths see it in the range check; at 64 bits it must be
        // tested before `i64.div_s`, which would trap.
        if wide && signed && op == PrimOp::Div {
            self.a.get(x);
            self.a.i64(i64::MIN);
            self.a.s().i64_eq();
            self.a.get(y);
            self.a.i64(-1);
            self.a.s().i64_eq().i32_and();
            self.a.if_();
            self.panic("integer-overflow: integer overflow");
            self.a.end();
        }
        self.a.get(x);
        self.a.get(y);
        {
            let mut s = self.a.s();
            match (op, signed, in64) {
                (PrimOp::Add, _, true) => s.i64_add(),
                (PrimOp::Sub | PrimOp::Neg, _, true) => s.i64_sub(),
                (PrimOp::Mul, _, true) => s.i64_mul(),
                (PrimOp::Div, true, true) => s.i64_div_s(),
                (PrimOp::Div, false, true) => s.i64_div_u(),
                (PrimOp::Rem, true, true) => s.i64_rem_s(),
                (_, _, true) => s.i64_rem_u(),
                (PrimOp::Add, _, false) => s.i32_add(),
                (PrimOp::Sub | PrimOp::Neg, _, false) => s.i32_sub(),
                (PrimOp::Mul, _, false) => s.i32_mul(),
                (PrimOp::Div, true, false) => s.i32_div_s(),
                (PrimOp::Div, false, false) => s.i32_div_u(),
                (PrimOp::Rem, true, false) => s.i32_rem_s(),
                (_, _, false) => s.i32_rem_u(),
            };
        }
        self.a.set(r);
        if wide {
            self.wide_overflow(op, signed, x, y, r);
        } else {
            // The exact result against the type's range. An unsigned
            // result below zero reads as a large unsigned value.
            let (lo, hi): (i64, i64) = if signed {
                (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
            } else {
                (0, (1i64 << bits) - 1)
            };
            let push = |em: &mut Self, v: i64| {
                if in64 {
                    em.a.i64(v);
                } else {
                    em.a.i32(i32::try_from(v).expect("a sub-word bound"));
                }
            };
            self.a.get(r);
            push(self, hi);
            match (signed, in64) {
                (true, true) => self.a.s().i64_gt_s(),
                (false, true) => self.a.s().i64_gt_u(),
                (true, false) => self.a.s().i32_gt_s(),
                (false, false) => self.a.s().i32_gt_u(),
            };
            if signed {
                self.a.get(r);
                push(self, lo);
                if in64 {
                    self.a.s().i64_lt_s();
                } else {
                    self.a.s().i32_lt_s();
                }
                self.a.s().i32_or();
            }
        }
        self.a.if_();
        self.panic("integer-overflow: integer overflow");
        self.a.end();
    }

    /// Integer power (`expr.power.int.*`, `expr.power.checked`):
    /// `ops[0] ** ops[1]` by repeated squaring, each product checked in
    /// the base's type. A square is made only while exponent bits remain,
    /// so no checked product exceeds the final magnitude, and a result in
    /// range never panics. An exponent of zero gives one. A release build
    /// multiplies natively, keeping the low bits, and wraps the result to
    /// the type (`types.arith.release`).
    fn int_pow(&mut self, ops: &[u32], bits: u8, signed: bool) -> StageResult<()> {
        let vt = if bits == 64 { VT::I64 } else { VT::I32 };
        let exact = if bits == 32 { VT::I64 } else { vt.clone() };
        let in64 = exact == VT::I64;
        let (x, y, p) = (
            self.a.local(exact.clone()),
            self.a.local(exact.clone()),
            self.a.local(exact.clone()),
        );
        let (base, acc) = (self.a.local(exact.clone()), self.a.local(exact.clone()));
        let e = self.a.local(VT::I64);
        self.comp(ops[0], 0, &vt)?;
        if bits == 32 {
            if signed {
                self.a.s().i64_extend_i32_s();
            } else {
                self.a.s().i64_extend_i32_u();
            }
        }
        self.a.set(base);
        // The exponent is any unsigned width, extended to 64 bits.
        let evt = self.vts(self.ty_of(ops[1]))?;
        let Some(ev) = evt.first().cloned() else {
            return unsupported("an exponent without a value");
        };
        self.comp(ops[1], 0, &ev)?;
        if ev == VT::I32 {
            self.a.s().i64_extend_i32_u();
        }
        self.a.set(e);
        if in64 {
            self.a.i64(1);
        } else {
            self.a.i32(1);
        }
        self.a.set(acc);
        self.a.block();
        self.a.loop_();
        self.a.get(e);
        self.a.i64(1);
        self.a.s().i64_and().i64_eqz().i32_eqz();
        self.a.if_();
        for (src, dst) in [(acc, x), (base, y)] {
            self.a.get(src);
            self.a.set(dst);
        }
        self.pow_mul(bits, signed, (x, y, p));
        self.a.get(p);
        self.a.set(acc);
        self.a.end();
        self.a.get(e);
        self.a.i64(1);
        self.a.s().i64_shr_u();
        self.a.set(e);
        self.a.get(e);
        self.a.s().i64_eqz();
        self.a.br_if(1);
        for dst in [x, y] {
            self.a.get(base);
            self.a.set(dst);
        }
        self.pow_mul(bits, signed, (x, y, p));
        self.a.get(p);
        self.a.set(base);
        self.a.br(0);
        self.a.end();
        self.a.end();
        self.a.get(acc);
        if bits == 32 {
            self.a.s().i32_wrap_i64();
        }
        if self.release() {
            self.wrap_to(bits, signed);
        }
        Ok(())
    }

    /// `r = x * y` on `int_pow`'s locals: checked, or in a release build
    /// the native product, whose low bits are the wrapped product's.
    fn pow_mul(&mut self, bits: u8, signed: bool, (x, y, r): (u32, u32, u32)) {
        if !self.release() {
            self.checked_arith(PrimOp::Mul, bits, signed, (x, y, r));
            return;
        }
        self.a.get(x);
        self.a.get(y);
        if bits >= 32 {
            self.a.s().i64_mul();
        } else {
            self.a.s().i32_mul();
        }
        self.a.set(r);
    }

    /// Pushes whether the 64-bit `r = x op y` overflowed: add and subtract
    /// by sign tests, multiply by dividing the product back.
    fn wide_overflow(&mut self, op: PrimOp, signed: bool, x: u32, y: u32, r: u32) {
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
            // The product overflowed when `x` is not zero and `r / x` is not
            // `y`. `x = -1` is tested apart, since `MIN / -1` traps: it
            // overflows exactly when `y` is `MIN`.
            (PrimOp::Mul, _) => {
                let o = self.a.local(VT::I32);
                self.a.i32(0);
                self.a.set(o);
                self.a.get(x);
                self.a.s().i64_eqz().i32_eqz();
                self.a.if_();
                if signed {
                    self.a.get(x);
                    self.a.i64(-1);
                    self.a.s().i64_eq();
                    self.a.if_();
                    self.a.get(y);
                    self.a.i64(i64::MIN);
                    self.a.s().i64_eq();
                    self.a.set(o);
                    self.a.else_();
                }
                self.a.get(r);
                self.a.get(x);
                if signed {
                    self.a.s().i64_div_s();
                } else {
                    self.a.s().i64_div_u();
                }
                self.a.get(y);
                self.a.s().i64_ne();
                self.a.set(o);
                if signed {
                    self.a.end();
                }
                self.a.end();
                self.a.get(o);
            }
            // Division and remainder overflow only at `MIN / -1`, tested
            // before the division.
            _ => self.a.i32(0),
        }
    }

    /// The current providers for a callee's row keys, in key order: per
    /// key, the innermost provider of that whole key, so `Repo[User]`
    /// never matches `Repo[Post]` (`req.row.entail.generic`, `crates/hd_mono/DESIGN.md`
    /// §13.18). Check covers every key, so a miss is internal.
    fn push_providers(&mut self, keys: &[Ty]) -> StageResult<()> {
        for k in keys {
            let ls = self.provider_locals(*k)?;
            self.a.get(ls[0]);
            self.a.get(ls[1]);
        }
        Ok(())
    }

    /// The locals of the innermost provider of the whole key `k`. In a
    /// thunk of an erased body with row parameters, a key no provider
    /// covers is one of those rows' keys, read from the open context
    /// (`crates/hd_mono/DESIGN.md` §13.5.1).
    fn provider_locals(&mut self, k: Ty) -> StageResult<[u32; 2]> {
        if let Some((_, ls)) = self.providers.iter().rev().find(|p| p.0 == k) {
            return Ok(*ls);
        }
        let Some([keys_at, provs_at]) = self.open_ctx else {
            return unsupported(format!(
                "a provider of `{}`, which this function's row does not cover",
                self.pool().display(k)
            ));
        };
        let vt = self.lay.key_vtable(k)?;
        let (pool, env) = (self.lay.pool, self.lay.env);
        Ok(ctx_provider(
            &mut self.a,
            pool,
            env,
            k,
            &vt,
            keys_at,
            provs_at,
        ))
    }

    /// The keys of a function type's row, whole and in key order.
    fn fn_row_keys(&self, t: Ty) -> Vec<Ty> {
        let Some(row) = self.fn_row(t) else {
            return Vec::new();
        };
        let (pool, env) = (self.pool(), self.env());
        key_order(pool, &|d| env.path_hash(d), pool.row_data(row).keys)
    }

    /// A function type's row.
    fn fn_row(&self, t: Ty) -> Option<RowId> {
        let pool = self.pool();
        let t = self.lay.strip(t);
        match pool.get(t) {
            TyData::Fn { row, .. } => Some(row),
            _ => None,
        }
    }

    /// The keys of a context type's row, whole and in key order: the
    /// order of its providers array (`crates/hd_mono/DESIGN.md` §12.4, "Context values").
    fn context_keys(&self, t: Ty) -> StageResult<Vec<Ty>> {
        let pool = self.pool();
        let TyData::Context(row) = pool.get(self.lay.strip(t)) else {
            return unsupported("a context entry of a type that is not a context");
        };
        let env = self.env();
        Ok(key_order(
            pool,
            &|d| env.path_hash(d),
            pool.row_data(row).keys,
        ))
    }

    /// `$.context(...)`: one providers array, per key of the context's
    /// row in key order, the provider of the last pair that binds it
    /// (`req.context.order`).
    fn context_new(&mut self, i: u32, rec: u32, ty: Ty) -> StageResult<()> {
        let keys = self.context_keys(ty)?;
        let pairs = self.rec(rec);
        for k in &keys {
            let mut from = None;
            for c in pairs.chunks(2).rev() {
                let kt = self.sub(Ty(c[0]));
                if kt == *k {
                    from = Some((c[1], None));
                    break;
                }
                if matches!(self.pool().get(kt), TyData::Context(_))
                    && let Some(at) = self.context_keys(kt)?.iter().position(|x| x == k)
                {
                    from = Some((c[1], Some(at)));
                    break;
                }
            }
            match from {
                Some((v, None)) => {
                    let vs = self.vts(*k)?;
                    self.load_as(v, &vs)?;
                }
                // A spread context's payload and vtable, kept erased.
                Some((v, Some(at))) => {
                    for slot in [2 * at, 2 * at + 1] {
                        self.comp(v, 0, &VT::r(ctx_provs()))?;
                        self.a.i32(i32_of(slot));
                        self.a.array_get(&ctx_provs());
                    }
                }
                None => return unsupported("a context key that no entry binds"),
            }
        }
        self.a.array_new_fixed(&ctx_provs(), u32_of(2 * keys.len()));
        self.store(i)
    }

    /// Pushes the providers of a spread context value `v` of type `ct`,
    /// key by key in key order, as provider locals: the payload, and the
    /// vtable cast back to the key's (`req.context.spread`). Returns how
    /// many it pushed.
    fn push_context(&mut self, v: u32, ct: Ty) -> StageResult<usize> {
        let keys = self.context_keys(ct)?;
        let c = self.a.local(VT::r(ctx_provs()));
        self.comp(v, 0, &VT::r(ctx_provs()))?;
        self.a.set(c);
        for (at, k) in keys.iter().enumerate() {
            let vt = self.lay.key_vtable(*k)?;
            let ls = [self.a.local(VT::Eq), self.a.local(VT::r(vt.clone()))];
            self.a.get(c);
            self.a.i32(i32_of(2 * at));
            self.a.array_get(&ctx_provs());
            self.a.set(ls[0]);
            self.a.get(c);
            self.a.i32(i32_of(2 * at + 1));
            self.a.array_get(&ctx_provs());
            self.a.ref_cast(&vt, false);
            self.a.set(ls[1]);
            self.providers.push((*k, ls));
        }
        Ok(keys.len())
    }

    /// The context a call of a function value of type `callee` passes:
    /// the key ids and this body's providers of the callee type's row
    /// (`crates/hd_mono/DESIGN.md` §12.4), or two nulls for the empty row.
    /// In an erased body, a row that lists the method's own row parameter
    /// also passes the open context, which needs no copy when the row
    /// has no other key (§13.5.1).
    fn push_ctx(&mut self, callee: Ty) -> StageResult<()> {
        let mut keys = self.fn_row_keys(callee);
        let open = self
            .fn_row(callee)
            .is_some_and(|r| row_is_open(self.pool(), r));
        if open {
            self.scope_keys(&mut keys);
        }
        self.push_ctx_of(&keys, open)
    }

    /// Adds the keys of the `$.with` scopes open in a body with an open
    /// context: one may be a key of the open row too, and the nearer
    /// provider wins (`req.with.nearest.row-parameter`).
    fn scope_keys(&self, keys: &mut Vec<Ty>) {
        if self.open_ctx.is_none() {
            return;
        }
        for at in (self.open_base..self.providers.len()).rev() {
            let k = self.providers[at].0;
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }

    /// The providers a call of `item` at `args` passes: one per key its
    /// row lists itself, then, for its own row parameters, one context of
    /// their arguments' keys (`crates/hd_mono/DESIGN.md` §12.4).
    fn push_call_providers(&mut self, item: DefId, args: TyList) -> StageResult<()> {
        let (pool, env) = (self.pool(), self.env());
        if !has_row_params(env, item) {
            return self.push_providers(&env.row_keys(item, args));
        }
        self.push_providers(&declared_row_keys(pool, env, item, args))?;
        self.push_row_ctx(item, args)
    }

    /// The context of `item`'s own row parameters at `args`: the keys of
    /// their arguments, in front of this body's open context when an
    /// argument lists the open row, so forwarding `R` costs nothing.
    fn push_row_ctx(&mut self, item: DefId, args: TyList) -> StageResult<()> {
        let (pool, env) = (self.pool(), self.env());
        let mut keys = row_param_keys(pool, env, item, args);
        let open = row_args_open(pool, env, item, args);
        if open {
            self.scope_keys(&mut keys);
        }
        self.push_ctx_of(&keys, open)
    }

    /// Marks the locals `at` as the open context, after the body's own
    /// providers (`crates/hd_mono/DESIGN.md` §12.4, §13.5.1).
    fn set_open_ctx(&mut self, at: [u32; 2]) {
        self.open_ctx = Some(at);
        self.open_base = self.providers.len();
    }

    /// A context of `keys`, each with its innermost provider, in front of
    /// the open context when `open` (`crates/hd_mono/DESIGN.md` §13.5.1),
    /// so a key both name resolves to the nearer provider
    /// (`req.with.nearest.row-parameter`); two nulls for no key.
    fn push_ctx_of(&mut self, keys: &[Ty], open: bool) -> StageResult<()> {
        let base = if open { self.open_ctx } else { None };
        let Some([ok, op]) = base else {
            if keys.is_empty() {
                self.a.ref_null(&ctx_keys());
                self.a.ref_null(&ctx_provs());
                return Ok(());
            }
            for k in keys {
                self.a.i64(key_id(self.pool(), self.env(), *k));
            }
            self.a.array_new_fixed(&ctx_keys(), u32_of(keys.len()));
            self.push_providers(keys)?;
            self.a.array_new_fixed(&ctx_provs(), u32_of(2 * keys.len()));
            return Ok(());
        };
        if keys.is_empty() {
            self.a.raw_get(ok);
            self.a.raw_get(op);
            return Ok(());
        }
        let n = i32_of(keys.len());
        let mut provs = Vec::new();
        for k in keys {
            provs.push(self.provider_locals(*k)?);
        }
        // The open context's length; a null one is empty.
        let len = self.a.local(VT::I32);
        self.a.i32(0);
        self.a.set(len);
        self.a.raw_get(ok);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.a.raw_get(ok);
        self.a.array_len();
        self.a.set(len);
        self.a.end();
        self.a.i32(n);
        self.a.get(len);
        self.a.s().i32_add();
        self.a.array_new_default(&ctx_keys());
        let nk = self.a.local_init(VT::r(ctx_keys()));
        self.a.i32(2 * n);
        self.a.get(len);
        self.a.i32(2);
        self.a.s().i32_mul();
        self.a.s().i32_add();
        self.a.array_new_default(&ctx_provs());
        let np = self.a.local_init(VT::r(ctx_provs()));
        for (j, (k, [pl, vl])) in keys.iter().zip(provs).enumerate() {
            let j = i32_of(j);
            self.a.get(nk);
            self.a.i32(j);
            self.a.i64(key_id(self.pool(), self.env(), *k));
            self.a.array_set(&ctx_keys());
            self.a.get(np);
            self.a.i32(2 * j);
            self.a.get(pl);
            self.a.array_set(&ctx_provs());
            self.a.get(np);
            self.a.i32(2 * j + 1);
            self.a.get(vl);
            self.a.array_set(&ctx_provs());
        }
        self.a.get(len);
        self.a.if_();
        self.a.get(nk);
        self.a.i32(n);
        self.a.raw_get(ok);
        self.a.i32(0);
        self.a.get(len);
        self.a.array_copy(&ctx_keys(), &ctx_keys());
        self.a.get(np);
        self.a.i32(2 * n);
        self.a.raw_get(op);
        self.a.i32(0);
        self.a.get(len);
        self.a.i32(2);
        self.a.s().i32_mul();
        self.a.array_copy(&ctx_provs(), &ctx_provs());
        self.a.end();
        self.a.get(nk);
        self.a.get(np);
        Ok(())
    }

    fn call(&mut self, i: u32, callee_at: u32, args_at: u32, ty: Ty) -> StageResult<()> {
        let rec = self.rec(args_at);
        let args = &rec[..rec.len().saturating_sub(3)];
        let Some(c) = Callee::from_words(self.b.record(callee_at)) else {
            return Err(NotImplemented::bug(
                Stage::Emit,
                "a malformed callee record",
            ));
        };
        let target = self.calls.get(&i).cloned();
        // A member the compiler lowers at each call goes there at any
        // receiver, a trait value included (`Inspectable.downcast`).
        if let Some(Target::Call(t)) = &target
            && let TargetKind::Intrinsic(key) = &t.kind
        {
            return self.call_intrinsic(i, (t.item, t.args), key, args, ty);
        }
        if let (
            Some(Target::Dyn),
            Callee::TraitMethod {
                trait_,
                method,
                targs,
                ..
            },
        ) = (&target, c)
        {
            let rs = self.push_dyn(trait_, method, targs, args, ty)?;
            return self.store_from(i, &rs);
        }
        let Some(Target::Call(t)) = target else {
            return unsupported("a call that collection did not resolve");
        };
        match &t.kind {
            TargetKind::Instance => self.call_instance(i, &t, args, ty, false),
            TargetKind::Intrinsic(key) => self.call_intrinsic(i, (t.item, t.args), key, args, ty),
            TargetKind::Builtin { self_ty, .. } => {
                let name = (self.lay.path)(t.item);
                let name = name.rsplit(['.', '/']).next().unwrap_or("").to_owned();
                self.builtin(i, &name, *self_ty, args, ty)
            }
        }
    }

    /// A direct `call` of a collected instance: the arguments at the
    /// callee's parameter types, then its providers. `bracket` counts the
    /// call as a forbidden context (`crates/hd_wasm/DESIGN.md` §14.9), so `block_on`
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
        self.push_call_providers(t.item, t.args)?;
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
        crate::rt::forbid_by(&mut self.a, by);
    }

    /// An omitted argument (`fn.default.eval`, `crates/hd_mono/DESIGN.md` §13.13): a
    /// direct `call` of its default body's instance with the earlier
    /// argument values, bracketed when the body makes a call (§12.3).
    fn default_call(&mut self, i: u32, args_at: u32, ty: Ty) -> StageResult<()> {
        // A constant default is inlined (`crates/hd_mono/DESIGN.md` §13.13).
        if let Some(Target::ConstDefault(bits)) = self.calls.get(&i).cloned() {
            let shape = self.lay.shape(self.sub(ty))?;
            self.scalar_const(&shape, bits)?;
            return self.store(i);
        }
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
        // A concrete value's recorded type is its static type
        // (trait.inspect.static).
        if name == "runtime_type" {
            self.push_type_id(self_ty, ty)?;
            return self.store(i);
        }
        // Erased newtypes share their base's intrinsic lowerings: the
        // values are the base's values.
        let self_ty = self.lay.erase(self_ty);
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
                self.int_text(i, args[0], self_ty, v)
            }
            // A float's text is std's `f64_text` and `f32_text`, the same
            // host primitives its `Display` implementation calls.
            ("to_string", Shape::Scalar(VT::F64)) => self.host_primitive(i, "format_f64", args, ty),
            ("to_string", Shape::Scalar(VT::F32)) => self.host_primitive(i, "format_f32", args, ty),
            // `f64.sqrt` and `f32.sqrt` (std-num.float-sqrt): the one
            // correctly rounded IEEE 754 square-root instruction.
            ("sqrt", Shape::Scalar(VT::F64)) => {
                self.comp(args[0], 0, &VT::F64)?;
                self.a.s().f64_sqrt();
                self.store(i)
            }
            ("sqrt", Shape::Scalar(VT::F32)) => {
                self.comp(args[0], 0, &VT::F32)?;
                self.a.s().f32_sqrt();
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
        let (_, _, float) = num(&self.lay, self.ty_of(ops[0]));
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

    /// An integer's decimal text through the runtime's `IntToStr`
    /// helper, which writes the digits into one buffer: the built-in
    /// `to_string` and std's `format_int` primitive.
    fn int_text(&mut self, i: u32, arg: u32, t: Ty, v: &VT) -> StageResult<()> {
        let (_, signed, _) = num(&self.lay, t);
        self.comp(arg, 0, v)?;
        self.a.call(Sym::Helper(Helper::IntToStr {
            wide: *v == VT::I64,
            signed,
        }));
        self.store(i)
    }

    fn lit_into(&mut self, i: u32, s: &str) -> StageResult<()> {
        self.a.lit(s.as_bytes());
        self.a.i64(i64::try_from(s.len()).unwrap_or(0) << 32);
        self.store(i)
    }

    fn call_intrinsic(
        &mut self,
        i: u32,
        (item, targs): (DefId, TyList),
        key: &str,
        args: &[u32],
        ty: Ty,
    ) -> StageResult<()> {
        match key {
            // `TypeId::of::[T]()` (trait.typeid.of): the same `TypeId`
            // `runtime_type` builds for a value of type `T`.
            "type_id_of" => {
                let Some(&t) = self.pool().list_items(targs).last() else {
                    return unsupported("a `TypeId::of` without its type argument");
                };
                self.push_type_id(t, ty)?;
                self.store(i)
            }
            "downcast_val" | "downcast" | "downcast_mut" => self.downcast(i, item, args[0], ty),
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
                self.a.call(crate::rt::panic_import());
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
            // on standard error (`module.entry.err-stream`).
            // `dbg`'s line, to the program's debug output, which is standard
            // error (`module.dbg.stream`); the same write as an entry point's
            // error report.
            "entry_write" | "dbg_write" => {
                self.load(args[0])?;
                self.a.call(Sym::Helper(Helper::StrToBuf));
                self.a.call(crate::rt::stderr_import());
                crate::rt::write_lit(&mut self.a, "\n");
                Ok(())
            }
            // The REPL's value line (`cli.repl.value.stream`), to standard
            // output through the runtime, which needs no `Console`.
            "repl_write" | "entry_write_out" => {
                self.load(args[0])?;
                self.a.call(Sym::Helper(Helper::StrToBuf));
                self.a.call(crate::rt::stdout_import());
                crate::rt::write_lit_to(&mut self.a, "\n", crate::rt::stdout_import());
                Ok(())
            }
            // std's `list_truncate(list, len)`: the list keeps its first
            // `len` items. Its callers checked the bounds.
            // A pinned list panics when the length changes
            // (`ref.elem.pinned`).
            "list_truncate" => {
                let (elem, lt) = self.list_parts(self.ty_of(args[0]))?;
                let (l, n) = (self.a.local(VT::r(lt.clone())), self.a.local(VT::I32));
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.comp(args[1], 0, &VT::I32)?;
                self.a.set(n);
                self.pin_check(l, &lt, elem.len(), Some(n));
                self.a.get(l);
                self.a.get(n);
                self.a.struct_set(&lt, 0);
                Ok(())
            }
            "block_on" => {
                // `crates/hd_wasm/DESIGN.md` §14.9: poll; when Pending, wait in the host.
                let Shape::Suspend { base, poll, result } = self.lay.shape(self.ty_of(args[0]))?
                else {
                    return unsupported("`block_on` of a value that is not a suspension");
                };
                let Shape::Enum(end) = self.lay.shape(ty)? else {
                    return unsupported("`block_on` whose result is not a `BlockOn`");
                };
                let s = self.a.local(VT::r(base.clone()));
                self.comp(args[0], 0, &VT::r(base.clone()))?;
                self.a.set(s);
                // The indirect ban (`req.drive.block-on.indirect`): reached
                // while a forbidden context runs, it panics.
                self.a.call(Sym::Helper(Helper::ForbidCheck));
                let (ready, flags, seen) = (
                    self.a.local(VT::I32),
                    self.a.local(VT::I32),
                    self.a.local(VT::I32),
                );
                let tmp: Vec<u32> = result.iter().map(|v| self.a.local(v.dflt())).collect();
                // The polled subtree is its own root: its host leaves sit
                // under no `all!` waker node (`crates/hd_wasm/DESIGN.md`
                // §14.4) until `block_on` returns.
                let (node, slot) = (self.a.local(VT::Eq), self.a.local(VT::I32));
                self.a.global_get(crate::rt::wake_cur_node());
                self.a.set(node);
                self.a.global_get(crate::rt::wake_cur_slot());
                self.a.set(slot);
                self.a.ref_null_eq();
                self.a.global_set(crate::rt::wake_cur_node());
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
                // A cancelled suspension never completes; each poll
                // continues its cleanup, and the drive ends once that has
                // ended (`req.drive.block-on.cancelled`).
                self.a.get(s);
                self.a.struct_get(&base, F_FLAGS);
                self.a.set(flags);
                self.a.get(flags);
                self.a.i32(CANCELLED | UNWINDING);
                self.a.s().i32_and();
                self.a.i32(CANCELLED);
                self.a.s().i32_eq();
                self.a.br_if(1);
                // An `abandon()` in the drive may have ended a cleanup that
                // a party waits on with no wake, as for the entry
                // (`crates/hd_wasm/DESIGN.md` §14.6, "Abandoning"): poll
                // again, and hand the request on to the outer driver.
                self.a.global_get(crate::rt::rewake_global());
                self.a.if_();
                self.a.i32(0);
                self.a.global_set(crate::rt::rewake_global());
                self.a.i32(1);
                self.a.set(seen);
                self.a.br(1);
                self.a.end();
                // The host waits for at least one completion and writes
                // the handles; they go to the wake table.
                self.a.call(block_import());
                self.a.call(Sym::Helper(Helper::WakeMark));
                self.a.br(0);
                self.a.end();
                self.a.end();
                self.a.raw_get(node);
                self.a.global_set(crate::rt::wake_cur_node());
                self.a.get(slot);
                self.a.global_set(crate::rt::wake_cur_slot());
                self.a.get(seen);
                self.a.global_get(crate::rt::rewake_global());
                self.a.s().i32_or();
                self.a.global_set(crate::rt::rewake_global());
                // `.Completed(value)`, or how the cleanup ended:
                // `.Abandoned` when the frame carries `ABANDONED`, else
                // `.Cancelled` (`std-task.block-on.decl` order).
                self.result(i)?;
                self.a.get(ready);
                self.a.if_();
                self.push_variant(ty, &end, 0, &mut |em, _, k, x| {
                    em.a.get(tmp[k]);
                    em.a.conv(&result[k].dflt(), x);
                    Ok(())
                })?;
                self.store(i)?;
                self.a.else_();
                self.a.get(flags);
                self.a.i32(ABANDONED);
                self.a.s().i32_and();
                self.a.if_();
                self.push_variant(ty, &end, 2, &mut |_, _, _, _| Ok(()))?;
                self.store(i)?;
                self.a.else_();
                self.push_variant(ty, &end, 1, &mut |_, _, _, _| Ok(()))?;
                self.store(i)?;
                self.a.end();
                self.a.end();
                Ok(())
            }
            // `Suspend[T].cancel!`'s cleanup frame (`req.cancel.wait`): a
            // `Suspend[bool]` that starts or continues the task's one
            // cleanup and waits it out (`crates/hd_wasm/DESIGN.md` §14.6).
            "task_cleanup_frame" => {
                let Shape::Suspend { base, .. } = self.lay.shape(ty)? else {
                    return unsupported("a `cancel!` frame of a type that is not a suspension");
                };
                let Shape::Suspend { base: task, .. } = self.lay.shape(self.ty_of(args[0]))? else {
                    return unsupported("`cancel!` of a value that is not a suspension");
                };
                self.comp(args[0], 0, &VT::r(task))?;
                self.a
                    .call(Sym::Helper(Helper::CleanupCold { base: base.clone() }));
                self.store_from(i, &[VT::r(base)])
            }
            // A suspension that never completes, for std's `timeout!`: its
            // cleanup bound waits in a `dispose!` of a `race!` child that
            // must stay live until the race cancels it
            // (`crates/hd_wasm/DESIGN.md` §14.6, "A frame that never
            // completes").
            "task_pending_frame" => {
                let Shape::Suspend { base, result, .. } = self.lay.shape(ty)? else {
                    return unsupported("a pending frame of a type that is not a suspension");
                };
                self.a.call(Sym::Helper(Helper::PendingCold {
                    base: base.clone(),
                    result,
                }));
                self.store_from(i, &[VT::r(base)])
            }
            // `Suspend[T].abandon` (`req.abandon.unwind`): the task's own
            // cancel, run with the `abandon()` depth raised, so it unwinds
            // at once and kills every shielded `dispose!` it reaches
            // (`crates/hd_wasm/DESIGN.md` §14.6, "Abandoning").
            "task_abandon" => {
                let Shape::Suspend { base, .. } = self.lay.shape(self.ty_of(args[0]))? else {
                    return unsupported("`abandon` of a value that is not a suspension");
                };
                let s = self.a.local(VT::r(base.clone()));
                let before = self.a.local(VT::I32);
                self.comp(args[0], 0, &VT::r(base.clone()))?;
                self.a.set(s);
                self.a.get(s);
                self.a.struct_get(&base, F_FLAGS);
                self.a.set(before);
                abandon_by(&mut self.a, 1);
                self.a.get(s);
                self.a.get(s);
                self.a.struct_get(&base, F_CANCEL);
                self.a.call_ref(&cancel_fn());
                abandon_by(&mut self.a, -1);
                // An abandon that did something may have ended a cleanup
                // that parties wait on with no wake: the entry polls again.
                self.a.get(s);
                self.a.struct_get(&base, F_FLAGS);
                self.a.get(before);
                self.a.s().i32_ne();
                self.a.if_();
                self.a.i32(1);
                self.a.global_set(crate::rt::rewake_global());
                self.a.end();
                Ok(())
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
            "concat_all" => self.concat_all(i, args[0]),
            "char_scalar" => {
                self.comp(args[0], 0, &VT::I32)?;
                self.store(i)
            }
            "char_from_scalar" => self.char_from_scalar(i, args[0]),
            // std's `format_int[N < Integer](value: N)`: the helper of the
            // argument's width and signedness.
            "format_int" => {
                let t = self.lay.erase(self.ty_of(args[0]));
                let Shape::Scalar(v @ (VT::I32 | VT::I64)) = self.lay.shape(t)? else {
                    return unsupported("`format_int` of a value that is not an integer");
                };
                self.int_text(i, args[0], t, &v)
            }
            // The run's map hash key: the runtime global, filled from the
            // host's `hash_seed` import on first use. A host that draws 0
            // is asked again, and answers 0 again.
            "map_hash_seed" => {
                let g = crate::rt::hash_seed_global();
                let seed = self.a.local(VT::I64);
                self.a.global_get(g.clone());
                self.a.set(seed);
                self.a.get(seed);
                self.a.s().i64_eqz();
                self.a.if_();
                self.a.call(crate::rt::hash_seed_import());
                self.a.set(seed);
                self.a.get(seed);
                self.a.global_set(g);
                self.a.end();
                self.a.get(seed);
                self.store(i)
            }
            // The next serial number of a local or new object that a
            // reference refers to (`crates/hd_wasm/DESIGN.md` §15.2,
            // "References"): a runtime global counter, so serials are
            // unique within a run. The first is 1; 0 marks a top-level
            // binding or a data field, whose identity needs no serial.
            "ref_serial" => {
                let g = crate::rt::ref_serial_global();
                self.a.global_get(g.clone());
                self.a.i64(1);
                self.a.s().i64_add();
                self.a.global_set(g.clone());
                self.a.global_get(g);
                self.store(i)
            }
            // The bit casts between a float and the integer of its width:
            // one reinterpret instruction, which keeps every bit, a NaN's
            // sign and payload included.
            "f64_to_bits" => {
                self.comp(args[0], 0, &VT::F64)?;
                self.a.s().i64_reinterpret_f64();
                self.store(i)
            }
            "f64_from_bits" => {
                self.comp(args[0], 0, &VT::I64)?;
                self.a.s().f64_reinterpret_i64();
                self.store(i)
            }
            "f32_to_bits" => {
                self.comp(args[0], 0, &VT::F32)?;
                self.a.s().i32_reinterpret_f32();
                self.store(i)
            }
            "f32_from_bits" => {
                self.comp(args[0], 0, &VT::I32)?;
                self.a.s().f32_reinterpret_i32();
                self.store(i)
            }
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
            // The float text primitives, which the host supplies as
            // `hd:prim` imports (`std` README, "Host"): the scalars go
            // in as parameters and the string comes back in the exchange
            // buffer.
            "format_f64" | "format_f32" | "format_f64_fixed" | "string_lower" | "string_upper"
            | "parse_f64" | "snapshot_update" => self.host_primitive(i, key, args, ty),
            other => unsupported(format!("the intrinsic `{other}`")),
        }
    }

    /// A call of a host primitive: the `hd:prim` import named `name`. A
    /// scalar argument goes in as itself and a string argument in the
    /// exchange buffer (its length is the import's parameter). A string
    /// result is decoded from the exchange buffer and a scalar result is
    /// the import's own.
    fn host_primitive(&mut self, i: u32, name: &str, args: &[u32], ty: Ty) -> StageResult<()> {
        let mut vts = Vec::new();
        let mut codecs = Vec::new();
        for a in args {
            let v = self.vts(self.ty_of(*a))?;
            if matches!(self.lay.shape(self.ty_of(*a))?, Shape::Str) {
                codecs.push(crate::rt::ArgCodec::Str);
            } else if let [one @ (VT::I32 | VT::I64 | VT::F32 | VT::F64)] = v.as_slice() {
                codecs.push(crate::rt::ArgCodec::Scalar(one.clone()));
            } else {
                return unsupported("a host primitive argument that is neither scalar nor string");
            }
            vts.extend(v);
        }
        let result = self.vts(ty)?;
        let res = match (self.lay.shape(ty)?, result.as_slice()) {
            (Shape::Str, _) => crate::rt::ResCodec::Buf(crate::boundary::BTy::of(&self.lay, ty)?),
            (_, [one @ (VT::I32 | VT::I64 | VT::F32 | VT::F64)]) => {
                crate::rt::ResCodec::Scalar(one.clone())
            }
            _ => return unsupported("a host primitive result that is neither scalar nor string"),
        };
        let helper = Helper::HostPrim {
            sig: WTy::Func(vts, result.clone()),
            name: name.to_owned(),
            args: codecs,
            result: res,
        };
        for a in args {
            self.load(*a)?;
        }
        self.a.call(Sym::Helper(helper));
        self.store_from(i, &result)
    }

    /// The `Inspectable` trait and its `runtime_type` method, from the
    /// item of a recovery intrinsic: `downcast_val`'s parameter is a
    /// `dyn Inspectable`, and `downcast` and `downcast_mut` are its members.
    fn inspectable_of(&self, item: DefId) -> StageResult<(DefId, DefId)> {
        let env = self.env();
        let insp = match env.parent(item) {
            Some((owner, _)) => owner,
            None => match env
                .params(item)
                .and_then(|ps| ps.first().map(|p| self.pool().get(*p)))
            {
                Some(TyData::TraitValue { def, .. }) => def,
                _ => return unsupported("a recovery intrinsic without `Inspectable`"),
            },
        };
        // The trait's dictionary holds `runtime_type` alone.
        let Some(&rt) = env.trait_methods(insp).first() else {
            return unsupported("an `Inspectable` without `runtime_type`");
        };
        Ok((insp, rt))
    }

    /// Pushes the `TypeId` of `t` (trait.typeid.*): the std data type
    /// `TypeId` (`ty`, `runtime_type`'s result) over `t`'s printable name,
    /// read from its global after the first build.
    fn push_type_id(&mut self, t: Ty, ty: Ty) -> StageResult<()> {
        let (st, key, shown) = self.type_id_parts(t, ty)?;
        self.a.call(Sym::Helper(Helper::TypeIdOf {
            ty: st,
            name: key,
            shown,
        }));
        Ok(())
    }

    /// The struct of the `TypeId` type `ty`, the key of `t`'s identity,
    /// and its printable name (trait.typeid.name). The two strings are one
    /// data segment when they are equal.
    fn type_id_parts(&self, t: Ty, ty: Ty) -> StageResult<(WTy, String, String)> {
        let ty = match self.pool().get(ty) {
            TyData::Mut(x) => x,
            _ => ty,
        };
        let Shape::Data { ty: st, fields } = self.lay.shape(ty)? else {
            return unsupported("a `TypeId` that is not a data value");
        };
        let string = [VT::r(WTy::Bytes), VT::I64];
        if !matches!(&fields[..], [(0, k), (2, n)] if *k == string && *n == string) {
            return unsupported("a `TypeId` that is not its key and name strings");
        }
        Ok((
            st,
            hd_mono::type_name(self.pool(), self.env(), t),
            hd_mono::printed_type_name(self.pool(), self.env(), t),
        ))
    }

    /// `downcast_val::[T](value)`, `value.downcast::[T]()` and
    /// `value.downcast_mut::[T]()` (trait.downcast.*): `.Some` of the
    /// value exactly when its recorded type has `T`'s runtime identity,
    /// `T` matched exactly; a reference comes back as itself and a boxed
    /// value unboxed. A trait value's recorded type is read through its
    /// `Inspectable` dictionary's `runtime_type`; a concrete receiver's is
    /// its static type, so the test is decided here.
    fn downcast(&mut self, i: u32, item: DefId, recv: u32, ty: Ty) -> StageResult<()> {
        let pool = self.pool();
        let strip = |t: Ty| match pool.get(t) {
            TyData::Mut(x) => x,
            _ => t,
        };
        let TyData::Option(target) = pool.get(strip(ty)) else {
            return unsupported("a downcast whose result is not optional");
        };
        let recv_t = strip(self.ty_of(recv));
        let Shape::Opt(o, inner) = self.lay.shape(strip(ty))? else {
            return unsupported("a downcast whose result is not optional");
        };
        let want = hd_mono::type_name(pool, self.env(), target);
        let TyData::TraitValue { .. } = pool.get(recv_t) else {
            if hd_mono::type_name(pool, self.env(), recv_t) == want {
                if matches!(o, OptShape::Tagged(_)) {
                    self.a.i32(1);
                }
                self.load_as(recv, &inner)?;
                if let OptShape::Boxed(b, _) = &o {
                    self.a.struct_new(b);
                }
                return self.store(i);
            }
            return self.none_into(i);
        };
        let Shape::Dyn { vt, key: from, .. } = self.lay.shape(recv_t)? else {
            return unsupported("a downcast of a trait value without a vtable");
        };
        let (insp, rt) = self.inspectable_of(item)?;
        let Some((steps, ikey)) = self.lay.super_path(from, insp, TyList::EMPTY)? else {
            return unsupported("a downcast of a trait value that is not `Inspectable`");
        };
        let ivt = self.lay.vtable(ikey)?;
        let sig = self.lay.slot_sig(ikey, rt)?;
        let Some(id_ty) = self.env().ret(rt) else {
            return unsupported("a `runtime_type` without its result");
        };
        let (st, _, _) = self.type_id_parts(target, id_ty)?;
        // The payload, then its recorded type's key against `T`'s.
        let p = self.a.local(VT::Eq);
        self.comp(recv, 0, &VT::Eq)?;
        self.a.set(p);
        self.a.get(p);
        self.comp(recv, 1, &VT::r(vt))?;
        for (svt, field) in &steps {
            self.a.struct_get(svt, *field);
        }
        self.a.struct_get(&ivt, 0);
        self.a.call_ref(&sig);
        let id = self.a.local(VT::r(st.clone()));
        self.a.set(id);
        self.a.get(id);
        self.a.struct_get(&st, 0);
        self.a.get(id);
        self.a.struct_get(&st, 1);
        self.a.lit(want.as_bytes());
        self.a.i64(i64::try_from(want.len()).unwrap_or(0) << 32);
        self.a.call(Sym::Helper(Helper::StrEq));
        self.a.if_();
        if matches!(o, OptShape::Tagged(_)) {
            self.a.i32(1);
        }
        crate::rt::unerase(&mut self.a, p, &inner);
        if let OptShape::Boxed(b, _) = &o {
            self.a.struct_new(b);
        }
        self.store(i)?;
        self.a.else_();
        self.none_into(i)?;
        self.a.end();
        Ok(())
    }

    /// Sets an optional instruction's values to `.None`: every value zero.
    fn none_into(&mut self, i: u32) -> StageResult<()> {
        let res = self.result(i)?;
        let want = self.vals[&i].1.clone();
        for (l, v) in res.iter().zip(&want) {
            self.a.zero(&v.dflt());
            self.a.set(*l);
        }
        Ok(())
    }

    /// A trait-value call, its results left on the stack: the method's
    /// slot in the vtable of its declaring trait `trait_`, reached from
    /// the receiver's vtable through the parent fields when `trait_` is a
    /// supertrait (trait.dyn.value-methods, `crates/hd_mono/DESIGN.md` §13.16). `targs`
    /// are the callee's type arguments, the trait's first. A generic
    /// method's slot takes its open arguments as `eqref` and the witness
    /// of the call's method type arguments, and its open result, unboxed
    /// here, has the type `ret` (`crates/hd_mono/DESIGN.md` §13.5.1).
    fn push_dyn(
        &mut self,
        trait_: DefId,
        method: DefId,
        targs: TyList,
        args: &[u32],
        ret: Ty,
    ) -> StageResult<Vec<VT>> {
        let targs: Vec<Ty> = self
            .pool()
            .list_items(targs)
            .iter()
            .map(|t| self.sub(*t))
            .collect();
        let DynSlot {
            value_vt,
            steps,
            vt,
            slot,
            sig,
            keys,
            open,
        } = self
            .lay
            .dyn_slot(self.ty_of(args[0]), trait_, &targs, method)?;
        let WTy::Func(ps, rs) = &sig else {
            return unsupported("a vtable slot type");
        };
        let rl = self.a.local(VT::Eq);
        let vl = self.a.local(VT::r(vt.clone()));
        self.comp(args[0], 0, &VT::Eq)?;
        self.a.set(rl);
        self.comp(args[0], 1, &VT::r(value_vt))?;
        for (svt, field) in &steps {
            self.a.struct_get(svt, *field);
        }
        self.a.set(vl);
        self.a.get(rl);
        let mut k = 1;
        for (j, r) in args[1..].iter().enumerate() {
            let have = self.vts(self.ty_of(*r))?;
            if open
                .as_ref()
                .is_some_and(|o| o.params.get(j) == Some(&true))
            {
                self.erase(*r, &have)?;
                k += 1;
                continue;
            }
            let n = have.len();
            let want = ps[k..k + n].to_vec();
            self.load_as(*r, &want)?;
            k += n;
        }
        if open.is_some() {
            let n = self.env().trait_arity(trait_).min(targs.len());
            let env = self.env();
            let ph = |d: DefId| env.path_hash(d);
            let mut memo = CanonMemo::default();
            let family = witness_family(self.pool(), &ph, &mut memo, method, &targs[..n]);
            let name = witness_args(self.pool(), &ph, &mut memo, &targs[n..]);
            self.a
                .global_get(GSym::Witness(family, name, VT::r(witness_ty())));
        }
        self.push_providers(&keys)?;
        // The method's own row parameters: one context of the keys their
        // arguments add to its row (`crates/hd_mono/DESIGN.md` §13.5.1).
        if has_row_params(self.env(), method) {
            let mut all = vec![class_ref(self.pool())];
            all.extend_from_slice(&targs);
            let all = self.pool().list(&all);
            self.push_row_ctx(method, all)?;
        }
        self.a.get(vl);
        self.a.struct_get(&vt, slot);
        self.a.call_ref(&sig);
        if open.is_some_and(|o| o.ret) && !self.env().suspends(method) {
            let want = self.vts(ret)?;
            let tmp = self.a.local(VT::Eq);
            self.a.set(tmp);
            unerase(&mut self.a, tmp, &want);
            return Ok(want);
        }
        Ok(rs.clone())
    }

    /// An erased body's prologue (`crates/hd_mono/DESIGN.md` §13.5.1): its field of the
    /// witness parameter `w`, cast to the struct of its thunks.
    fn read_ops(&mut self, w: u32, family: Hash128) -> StageResult<()> {
        let mut opens: Vec<(u32, u32)> = self
            .calls
            .iter()
            .filter_map(|(ix, t)| match t {
                Target::Open(k) => Some((*k, *ix)),
                _ => None,
            })
            .collect();
        if opens.is_empty() {
            return Ok(());
        }
        opens.sort_unstable();
        let erased = self.args;
        let mut sigs = Vec::new();
        for (_, ix) in opens {
            sigs.push(thunk_sig(&self.lay, self.b, erased, ix)?);
        }
        let ops_ty = WTy::Struct {
            fields: sigs.iter().map(|s| VT::r(s.clone())).collect(),
            sup: None,
            open: false,
        };
        let l = self.a.local(VT::r(ops_ty.clone()));
        self.a.get(w);
        self.a
            .global_get(GSym::WitnessField(family, self.key, VT::I32));
        self.a.array_get(&witness_ty());
        self.a.ref_cast(&ops_ty, false);
        self.a.set(l);
        self.ops = Some((l, ops_ty, sigs));
        Ok(())
    }

    /// An open instruction of an erased body (`crates/hd_mono/DESIGN.md` §13.5.1): its
    /// operands, open ones as `eqref`, and the body's providers go to
    /// its thunk in the witness field the prologue read, whose results
    /// are left on the stack.
    fn open_call(&mut self, i: u32, k: u32) -> StageResult<Vec<VT>> {
        let Some((ops, ops_ty, sigs)) = self.ops.clone() else {
            return unsupported("an open instruction outside an erased body");
        };
        let Some(sig) = sigs.get(k as usize).cloned() else {
            return unsupported("an open instruction past its body's thunks");
        };
        let WTy::Func(_, rs) = &sig else {
            return unsupported("a thunk type");
        };
        for r in thunk_operands(self.b, i) {
            self.load(r)?;
        }
        for (l, _) in thunk_capture_locals(self.b, i) {
            for l in self.local(l)? {
                self.a.get(l);
            }
        }
        let keys = self.env().row_keys(self.item, self.args);
        self.push_providers(&keys)?;
        // With row parameters, the thunk also gets a context: each other
        // key a provider covers here, in front of the open context, so
        // its instruction finds every key it may need at concrete types.
        if has_row_params(self.env(), self.item) {
            let mut extra: Vec<Ty> = Vec::new();
            for (key, _) in self.providers.iter().rev() {
                if !keys.contains(key) && !extra.contains(key) {
                    extra.push(*key);
                }
            }
            self.push_ctx_of(&extra, true)?;
        }
        self.a.get(ops);
        self.a.struct_get(&ops_ty, k);
        self.a.call_ref(&sig);
        Ok(rs.clone())
    }

    fn closure(&mut self, i: u32, rec: u32, ty: Ty) -> StageResult<()> {
        let caps = self.rec(rec);
        let (start, len) = (caps[0] as usize, caps[1] as usize);
        self.closure_value(i, start..start + len, ty)
    }

    /// A closure value: its code, the instance collection recorded (a
    /// closure body, or a function reference's adapter, `crates/hd_mono/DESIGN.md`
    /// §13.11), and the captured locals `caps` (indices into
    /// `cap_local`). A closure whose environment holds no value is the
    /// constant global of its code (`GSym::Closure`), read without
    /// allocating.
    fn closure_value(&mut self, i: u32, caps: std::ops::Range<usize>, ty: Ty) -> StageResult<()> {
        let Some(Target::Closure(key)) = self.calls.get(&i).cloned() else {
            return unsupported("a closure that collection did not record");
        };
        let (env, fields) = self.env_of(caps, ty)?;
        if fields.is_empty() {
            self.a.global_get(GSym::Closure(key, VT::r(env)));
            return self.store(i);
        }
        self.a.ref_func(Sym::Inst(key));
        for (_, l) in fields {
            self.a.get(l);
        }
        self.a.struct_new(&env);
        self.store(i)
    }

    /// The environment struct of a closure value of type `ty` that
    /// captures `caps` (`crates/hd_mono/DESIGN.md`, the `Closure` row): a subtype of the
    /// function type's base holding the code, then each captured local's
    /// values (a `Shared` one's cell). Each captured value comes with its
    /// field and the Wasm local holding it, in field order.
    fn env_of(
        &mut self,
        caps: std::ops::Range<usize>,
        ty: Ty,
    ) -> StageResult<(WTy, Vec<(u32, u32)>)> {
        let Shape::Fn { base, .. } = self.lay.shape(ty)? else {
            return unsupported("a closure without a function type");
        };
        let mut fields = base.fields();
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
        // A trait value reaches a trait-value conversion when a type
        // parameter is instantiated with one: it widens, keeping its
        // payload and recorded type (trait.erase.keeps).
        if kind == Coercion::Supertrait as u32
            || (kind == Coercion::ToTraitValue as u32
                && matches!(self.lay.shape(self.ty_of(v))?, Shape::Dyn { .. }))
        {
            return self.widen(i, v, ty);
        }
        // A reference payload boxes as any value does; only the vtable's
        // adapters differ (`hd_mono::SlotRecv`).
        if kind == Coercion::ToTraitValue as u32 || kind == Coercion::RefToTraitValue as u32 {
            let Some(Target::VTable(table)) = self.calls.get(&i).cloned() else {
                return unsupported("a trait-value coercion that collection did not record");
            };
            let Shape::Dyn { key, .. } = self.lay.shape(ty)? else {
                return unsupported("a coercion to a non-trait type");
            };
            let from = self.ty_of(v);
            let fv = self.vts(from)?;
            self.erase(v, &fv)?;
            self.vtable_value(&table, key, from, &fv)?;
            return self.store(i);
        }
        // A trait value reference installed as a provider ("Providers of
        // reference traits"): the payload is the reference itself, which
        // `ProviderAsRef` reads back; the vtable is the one its `get`
        // returns, there only to fill the provider's layout.
        if kind == Coercion::RefAsProvider as u32 {
            let from = self.ty_of(v);
            let rd = self.ref_read(from)?;
            let fv = self.vts(from)?;
            let ([rv], [_, vt_want], [_, vt_have]) =
                (fv.as_slice(), want.as_slice(), rd.results.as_slice())
            else {
                return unsupported("a provider reference of an unexpected layout");
            };
            let (rv, vt_want, vt_have) = (rv.clone(), vt_want.clone(), vt_have.clone());
            self.comp(v, 0, &VT::Eq)?;
            self.comp(v, 0, &rv)?;
            rd.emit(&mut self.a);
            let l = self.a.local(vt_have.clone());
            self.a.set(l);
            self.a.s().drop();
            self.a.get(l);
            self.a.conv(&vt_have, &vt_want);
            return self.store(i);
        }
        if kind == Coercion::ProviderAsRef as u32 {
            let [w] = want.as_slice() else {
                return unsupported("a provider reference of several values");
            };
            // `comp` casts the `eqref` payload to the reference's struct.
            self.comp(v, 0, &w.clone())?;
            return self.store(i);
        }
        // A suspending function value's code already returns its cold
        // `mut Suspend[T]` (`Lay::code_ty`), so its closure is the
        // lowered constructor's closure unchanged.
        if kind == Coercion::Weaken as u32
            || kind == Coercion::Variance as u32
            || kind == Coercion::RowSubsume as u32
            || kind == Coercion::SuspendFnToCtor as u32
            || kind == Coercion::Identity as u32
        {
            self.load_as(v, &want)?;
            return self.store(i);
        }
        unsupported(format!("the coercion kind {kind}"))
    }

    /// Widening to a supertrait value (trait.dyn.widen, `crates/hd_mono/DESIGN.md`
    /// §13.16): the payload unchanged, and the supertrait's vtable read
    /// through the parent fields of the child's.
    fn widen(&mut self, i: u32, v: u32, ty: Ty) -> StageResult<()> {
        let (
            Shape::Dyn { vt, key: from, .. },
            Shape::Dyn {
                trait_: def, args, ..
            },
        ) = (self.lay.shape(self.ty_of(v))?, self.lay.shape(ty)?)
        else {
            return unsupported("a supertrait coercion between non-trait types");
        };
        let Some((steps, _)) = self.lay.super_path(from, def, args)? else {
            return unsupported("a supertrait coercion to a trait the value does not extend");
        };
        self.comp(v, 0, &VT::Eq)?;
        self.comp(v, 1, &VT::r(vt))?;
        for (svt, field) in &steps {
            self.a.struct_get(svt, *field);
        }
        self.store(i)
    }

    fn adapter_target(t: &CallTarget) -> StageResult<Sym> {
        match t.kind {
            TargetKind::Instance => Ok(Sym::Inst(t.key)),
            _ => unsupported("a vtable slot filled by a compiler lowering"),
        }
    }

    /// Pushes a value as `eqref` (§13.5.1, "As an open value"): an `i32`
    /// as an `i31ref` when it fits (`Helper::EraseI32`).
    fn erase(&mut self, v: u32, vts: &[VT]) -> StageResult<()> {
        if let [x @ (VT::Eq | VT::Ref(..))] = vts {
            self.comp(v, 0, x)
        } else if vts == [VT::I32] {
            self.load_as(v, vts)?;
            self.a.call(Sym::Helper(Helper::EraseI32));
            Ok(())
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
            IntrinsicOp::ListIndex | IntrinsicOp::ListIndexMut => {
                let lty = self.ty_of(args[0]);
                let (elem, lt) = self.list_parts(lty)?;
                let l = self.a.local(VT::r(lt.clone()));
                let ix = self.a.local(VT::I32);
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.comp(args[1], 0, &VT::I32)?;
                self.a.set(ix);
                self.bounds(l, &lt, ix);
                if op == IntrinsicOp::ListIndexMut {
                    let n = self.a.local(VT::I32);
                    self.a.get(l);
                    self.a.struct_get(&lt, 0);
                    self.a.set(n);
                    self.list_reserve(l, lty, n, false)?;
                }
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
                let lty = self.ty_of(args[0]);
                let (elem, lt) = self.list_parts(lty)?;
                let l = self.a.local(VT::r(lt.clone()));
                let ix = self.a.local(VT::I32);
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.comp(args[1], 0, &VT::I32)?;
                self.a.set(ix);
                self.bounds(l, &lt, ix);
                let n = self.a.local(VT::I32);
                self.a.get(l);
                self.a.struct_get(&lt, 0);
                self.a.set(n);
                self.list_reserve(l, lty, n, false)?;
                let ety = self.coll_arg(lty, 0);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    self.a.get(l);
                    self.a.struct_get(&lt, 1 + u32_of(k));
                    self.a.get(ix);
                    self.comp(args[2], k, v)?;
                    if let (1, Some(et)) = (elem.len(), ety) {
                        self.copy_slot(args[2], et, false)?;
                    }
                    self.a.array_set(&WTy::Array(st));
                }
                Ok(())
            }
            IntrinsicOp::ListPush => {
                let lty = self.ty_of(args[0]);
                let (elem, lt) = self.list_parts(lty)?;
                let l = self.a.local(VT::r(lt.clone()));
                self.comp(args[0], 0, &VT::r(lt.clone()))?;
                self.a.set(l);
                self.pin_check(l, &lt, elem.len(), None);
                let n = self.a.local(VT::I32);
                self.a.get(l);
                self.a.struct_get(&lt, 0);
                self.a.set(n);
                self.list_reserve(l, lty, n, true)?;
                let ety = self.coll_arg(lty, 0);
                for (k, v) in elem.iter().enumerate() {
                    let st = storage(v).dflt();
                    self.a.get(l);
                    self.a.struct_get(&lt, 1 + u32_of(k));
                    self.a.get(n);
                    self.comp(args[1], k, v)?;
                    if let (1, Some(et)) = (elem.len(), ety) {
                        self.copy_slot(args[1], et, false)?;
                    }
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
            | IntrinsicOp::MapIndexMut
            | IntrinsicOp::MapGet
            | IntrinsicOp::MapSet
            | IntrinsicOp::MapRemove => self.map_op(i, op, &args, ty),
            IntrinsicOp::ValueCopy => {
                // The copy's own type says what it copies: a legacy `mut`
                // view of the operand is no copy's type (`types.coll.legacy`).
                let vts = self.load(args[0])?;
                self.copy_top(self.sub(ty))?;
                self.store_from(i, &vts)
            }
            IntrinsicOp::SnapBegin => self.snap_begin(i, args[0]),
            IntrinsicOp::SnapEnd => self.snap_end(args[0], args[1]),
            IntrinsicOp::ListPin => self.list_pin(i, &args),
            IntrinsicOp::ListUnpin => self.list_unpin(args[0]),
            IntrinsicOp::MapIter => self.map_iter(i, args[0], ty),
            IntrinsicOp::Item => unsupported("the intrinsic operation Item"),
            IntrinsicOp::MapSlots => {
                let (_, _, mt, _) = self.map_parts(self.ty_of(args[0]))?;
                self.comp(args[0], 0, &VT::r(mt.clone()))?;
                self.a.struct_get(&mt, M_USED);
                self.store(i)
            }
            IntrinsicOp::MapSlotNext => self.map_slot_next(i, &args),
            IntrinsicOp::MapSlotEntry => self.map_slot_entry(i, args[0], args[1], ty),
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

    /// `concat_all(parts)`: every part's bytes in order, in one new array
    /// whose length is summed first, so each byte is copied once.
    fn concat_all(&mut self, i: u32, list: u32) -> StageResult<()> {
        let (elem, lt) = self.list_parts(self.ty_of(list))?;
        if elem != [VT::r(WTy::Bytes), VT::I64] {
            return unsupported("a `concat_all` whose parts are not strings");
        }
        let (bytes, spans) = (
            WTy::Array(storage(&elem[0]).dflt()),
            WTy::Array(storage(&elem[1]).dflt()),
        );
        let l = self.a.local(VT::r(lt.clone()));
        let (n, k, total, off, span) = (
            self.a.local(VT::I32),
            self.a.local(VT::I32),
            self.a.local(VT::I32),
            self.a.local(VT::I32),
            self.a.local(VT::I64),
        );
        let arr = self.a.local(VT::r(WTy::Bytes));
        self.comp(list, 0, &VT::r(lt.clone()))?;
        self.a.set(l);
        self.a.get(l);
        self.a.struct_get(&lt, 0);
        self.a.set(n);
        // Each pass walks the parts with `k`; the first sums the lengths,
        // the second copies.
        let walk = |em: &mut Self, body: &dyn Fn(&mut Self)| {
            em.a.i32(0);
            em.a.set(k);
            em.a.block();
            em.a.loop_();
            em.a.get(k);
            em.a.get(n);
            em.a.s().i32_ge_u();
            em.a.br_if(1);
            em.a.get(l);
            em.a.struct_get(&lt, 2);
            em.a.get(k);
            em.a.array_get(&spans);
            em.a.set(span);
            body(em);
            em.a.get(k);
            em.a.i32(1);
            em.a.s().i32_add();
            em.a.set(k);
            em.a.br(0);
            em.a.end();
            em.a.end();
        };
        let len = |em: &mut Self| {
            em.a.get(span);
            em.a.i64(32);
            em.a.s().i64_shr_u().i32_wrap_i64();
        };
        self.a.i32(0);
        self.a.set(total);
        walk(self, &|em| {
            em.a.get(total);
            len(em);
            em.a.s().i32_add();
            em.a.set(total);
        });
        self.a.get(total);
        self.a.array_new_default(&WTy::Bytes);
        self.a.set(arr);
        self.a.i32(0);
        self.a.set(off);
        walk(self, &|em| {
            em.a.get(arr);
            em.a.get(off);
            em.a.get(l);
            em.a.struct_get(&lt, 1);
            em.a.get(k);
            em.a.array_get(&bytes);
            em.a.conv(&storage(&elem[0]).dflt(), &VT::r(WTy::Bytes));
            em.a.get(span);
            em.a.s().i32_wrap_i64();
            len(em);
            em.a.array_copy(&WTy::Bytes, &WTy::Bytes);
            em.a.get(off);
            len(em);
            em.a.s().i32_add();
            em.a.set(off);
        });
        self.a.get(arr);
        self.a.get(total);
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

    /// A list literal with spreads (`expr.list.spread.insert`): the
    /// operands are already evaluated, once, in element order. The new list
    /// is sized exactly, then each component array is filled by `array.set`
    /// for an element and `array.copy` for a spread.
    fn spread_list(
        &mut self,
        i: u32,
        items: &[u32],
        spreads: &[u32],
        elem: &[VT],
        lt: &WTy,
    ) -> StageResult<()> {
        let is_spread = |k: usize| spreads.iter().any(|s| *s as usize == k);
        let n = self.a.local(VT::I32);
        self.a
            .i32(i32::try_from(items.len() - spreads.len()).unwrap_or(0));
        self.a.set(n);
        let mut sources = Vec::new();
        for (k, r) in items.iter().enumerate() {
            if !is_spread(k) {
                continue;
            }
            let (_, st) = self.list_parts(self.ty_of(*r))?;
            if st != *lt {
                return unsupported("a list spread with a different element layout");
            }
            let l = self.a.local(VT::r(st.clone()));
            self.comp(*r, 0, &VT::r(st.clone()))?;
            self.a.set(l);
            self.a.get(n);
            self.a.get(l);
            self.a.struct_get(&st, 0);
            self.a.s().i32_add();
            self.a.set(n);
            sources.push(l);
        }
        let off = self.a.local(VT::I32);
        let ety = self.coll_arg(self.ty_of(i), 0);
        self.a.get(n);
        for (c, v) in elem.iter().enumerate() {
            let arr = WTy::Array(storage(v).dflt());
            let dst = self.a.local(VT::r(arr.clone()));
            self.a.get(n);
            self.a.array_new_default(&arr);
            self.a.set(dst);
            self.a.i32(0);
            self.a.set(off);
            let mut next = sources.iter();
            for (k, r) in items.iter().enumerate() {
                if is_spread(k) {
                    let Some(l) = next.next() else {
                        return unsupported("a list spread without its list");
                    };
                    self.a.get(dst);
                    self.a.get(off);
                    self.a.get(*l);
                    self.a.struct_get(lt, 1 + u32_of(c));
                    self.a.i32(0);
                    self.a.get(*l);
                    self.a.struct_get(lt, 0);
                    self.a.array_copy(&arr, &arr);
                    let end = self.a.local(VT::I32);
                    self.a.get(off);
                    self.a.get(*l);
                    self.a.struct_get(lt, 0);
                    self.a.s().i32_add();
                    self.a.set(end);
                    if let (1, Some(et)) = (elem.len(), ety) {
                        self.copy_range(dst, &arr, (off, end), v, et)?;
                    }
                    self.a.get(end);
                    self.a.set(off);
                } else {
                    self.a.get(dst);
                    self.a.get(off);
                    self.comp(*r, c, v)?;
                    if let (1, Some(et)) = (elem.len(), ety) {
                        self.copy_slot(*r, et, false)?;
                    }
                    self.a.array_set(&arr);
                    self.a.get(off);
                    self.a.i32(1);
                    self.a.s().i32_add();
                    self.a.set(off);
                }
            }
            self.a.get(dst);
        }
        // `cow` and `pin`.
        self.a.i32(0);
        self.a.i32(0);
        self.a.struct_new(lt);
        self.store(i)
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
        let base = crate::layout::closure_base(self.lay.closure_code(), &code);
        let env = WTy::Struct {
            fields: [base.fields(), vec![VT::r(lt.clone()), VT::I32, VT::I32]].concat(),
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
        self.comp(list, 0, &VT::r(lt.clone()))?;
        self.a.i32(0);
        // The length at creation (`flow.for.length`).
        self.comp(list, 0, &VT::r(lt.clone()))?;
        self.a.struct_get(&lt, 0);
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
        let t = self.lay.strip(t);
        let TyData::Adt { args, .. } = pool.get(t) else {
            return unsupported("a map type");
        };
        let Some(&k) = pool.list_items(args).first() else {
            return unsupported("a map type without a key");
        };
        Ok((key, val, ty, k))
    }

    /// How instruction `i`'s map hashes and compares keys of type `kt`
    /// (`crates/hd_mono/DESIGN.md` §13.12): for an integer, `bool`, `char` or
    /// `string`, through the keyed bucket hash collection selected and an
    /// inline comparison; else through the `key_bucket_hash` and `Eq.eq`
    /// instances that collection selected at the key type.
    fn key_ops(&self, i: u32, kt: Ty, key: &[VT]) -> StageResult<KeyOps> {
        if let Some(Target::MapInline(hash)) = self.calls.get(&i) {
            if !inline_map_key(self.pool(), self.env(), kt) || self.vts(hash.ret)? != [VT::I32] {
                return unsupported("an inline map key's bucket hash that returns other values");
            }
            let params = self.key_params(hash, 0)?;
            let sym = Box::new(Sym::Inst(hash.key));
            return Ok(match key {
                [VT::I32] if params == [VT::I64] => KeyOps::I32(sym),
                [VT::I64] if params == [VT::I64] => KeyOps::I64(sym),
                [VT::Ref(t, false), VT::I64] if **t == WTy::Bytes && params == key => {
                    KeyOps::Str(sym)
                }
                _ => return unsupported("an inline map key of another layout"),
            });
        }
        if let Some(Target::MapKey { hash, eq }) = self.calls.get(&i) {
            let params = self.key_params(hash, 0)?;
            if params.len() != key.len()
                || self.key_params(eq, 0)? != params
                || self.key_params(eq, 1)? != params
                || self.vts(hash.ret)? != [VT::I32]
                || self.vts(eq.ret)? != [VT::I32]
            {
                return unsupported("a map key whose bucket hash or `eq` takes other values");
            }
            return Ok(KeyOps::Call {
                hash: Box::new(Sym::Inst(hash.key)),
                eq: Box::new(Sym::Inst(eq.key)),
                params,
            });
        }
        unsupported("a map key whose operations collection did not select")
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
        self.a.i32(0);
        self.a.i32(cap * 2);
        self.a.array_new_default(&ints);
        self.a.i32(cap);
        self.a.array_new_default(&ints);
        for v in comps {
            self.a.i32(cap);
            self.a.array_new_default(&WTy::Array(storage(v).dflt()));
        }
        self.a.i32(0);
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
        (v, vty): (u32, Option<Ty>),
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
        if let (1, Some(vty)) = (val.len(), vty) {
            self.copy_slot(v, vty, false)?;
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
                let vty = self.coll_arg(ty, 1);
                self.map_put(m, (&mt, &key, &val), &ops, &kl, h, (items[2 * e + 1], vty))?;
            }
            self.a.get(m);
        }
        self.store(i)
    }

    /// `m[k]`, `m.get(k)`, `m[k] = v` and `m.remove(k)` (`crates/hd_mono/DESIGN.md`
    /// §13.12): hash the key, then probe for its entry.
    fn map_op(&mut self, i: u32, op: IntrinsicOp, args: &[u32], ty: Ty) -> StageResult<()> {
        let (key, val, mt, kt) = self.map_parts(self.ty_of(args[0]))?;
        let ops = self.key_ops(i, kt, &key)?;
        let m = self.a.local(VT::r(mt.clone()));
        self.comp(args[0], 0, &VT::r(mt.clone()))?;
        self.a.set(m);
        if matches!(
            op,
            IntrinsicOp::MapSet | IntrinsicOp::MapRemove | IntrinsicOp::MapIndexMut
        ) {
            self.map_unique(m, self.ty_of(args[0]))?;
        }
        let (kl, h) = self.map_key(args[1], &key, &ops)?;
        if op == IntrinsicOp::MapSet {
            let vty = self.coll_arg(self.ty_of(args[0]), 1);
            return self.map_put(m, (&mt, &key, &val), &ops, &kl, h, (args[2], vty));
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
        if matches!(op, IntrinsicOp::MapIndex | IntrinsicOp::MapIndexMut) {
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

    /// `(map, pos, end)`: the first live entry slot in `pos..end` that the
    /// map has written, or `end`. A `for` loop checks for growth first
    /// (`flow.for.invalidate.map-rebuild`), so every slot before `end` is
    /// still written; a drop walk has no check, so the scan still stops at
    /// what is written now.
    fn map_slot_next(&mut self, i: u32, args: &[u32]) -> StageResult<()> {
        let (_, _, mt, _) = self.map_parts(self.ty_of(args[0]))?;
        let m = self.a.local(VT::r(mt.clone()));
        let (pos, end, lim) = (
            self.a.local(VT::I32),
            self.a.local(VT::I32),
            self.a.local(VT::I32),
        );
        self.comp(args[0], 0, &VT::r(mt.clone()))?;
        self.a.set(m);
        self.comp(args[1], 0, &VT::I32)?;
        self.a.set(pos);
        self.comp(args[2], 0, &VT::I32)?;
        self.a.set(end);
        self.a.get(end);
        self.a.set(lim);
        self.a.get(m);
        self.a.struct_get(&mt, M_USED);
        self.a.get(lim);
        self.a.s().i32_lt_u();
        self.a.if_();
        self.a.get(m);
        self.a.struct_get(&mt, M_USED);
        self.a.set(lim);
        self.a.end();
        self.a.block();
        self.a.loop_();
        self.a.get(pos);
        self.a.get(lim);
        self.a.s().i32_ge_u();
        self.a.if_();
        self.a.get(end);
        self.a.set(pos);
        self.a.br(2);
        self.a.end();
        self.a.get(m);
        self.a.struct_get(&mt, M_HASHES);
        self.a.get(pos);
        self.a.array_get(&WTy::Array(VT::I32));
        self.a.i32(0);
        self.a.s().i32_ge_s();
        self.a.br_if(1);
        self.a.get(pos);
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.set(pos);
        self.a.br(0);
        self.a.end();
        self.a.end();
        self.a.get(pos);
        self.store(i)
    }

    /// `(map, slot)`: the `(K, V)` entry in a live slot.
    fn map_slot_entry(&mut self, i: u32, map: u32, slot: u32, ty: Ty) -> StageResult<()> {
        let (key, val, mt, _) = self.map_parts(self.ty_of(map))?;
        let Shape::Tuple { boxed, .. } = self.lay.shape(ty)? else {
            return unsupported("a map entry that is not a tuple");
        };
        let m = self.a.local(VT::r(mt.clone()));
        let ix = self.a.local(VT::I32);
        self.comp(map, 0, &VT::r(mt.clone()))?;
        self.a.set(m);
        self.comp(slot, 0, &VT::I32)?;
        self.a.set(ix);
        let comps = [key, val].concat();
        for (k, v) in comps.iter().enumerate() {
            let st = storage(v).dflt();
            self.a.get(m);
            self.a.struct_get(&mt, M_KEYS + u32_of(k));
            self.a.get(ix);
            self.a.array_get(&WTy::Array(st.clone()));
            self.a.conv(&st, v);
        }
        match boxed {
            Some(b) => {
                self.a.struct_new(&b);
                self.store(i)
            }
            None => self.store_from(i, &comps),
        }
    }

    /// `Map.iter()`: a cursor closure over the entries (`crates/hd_mono/DESIGN.md`
    /// §13.12), capturing the written and live entry counts and the
    /// rebuild count.
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
        let base = crate::layout::closure_base(self.lay.closure_code(), &code);
        let env = WTy::Struct {
            fields: [
                base.fields(),
                vec![VT::r(mt.clone()), VT::I32, VT::I32, VT::I32, VT::I32],
            ]
            .concat(),
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
            code,
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
        self.a.get(m);
        self.a.struct_get(&mt, M_GROWN);
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
/// trait value per row key (`crates/hd_mono/DESIGN.md` §12.4); a closure's code takes its
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
            params.push(VT::r(lay.key_vtable(k)?));
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

/// The defining `Closure` instruction of sub-body `sub`, not an
/// instantiation of a generic local function.
fn closure_of(b: &Body, sub: u16) -> StageResult<usize> {
    let defining = |i: usize| {
        b.tags[i] == Tag::Closure && b.data[i][0] == u32::from(sub) && !b.is_instantiation(i)
    };
    match (0..b.len()).find(|&i| defining(i)) {
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
/// (`crates/hd_wasm/DESIGN.md` §14.2): a subtree's states are contiguous.
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
        for ob in &blocks {
            let x = block(b, *ob, p, next)?;
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
    let mut p = (HashMap::default(), HashMap::default());
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
            vals: HashMap::default(),
            ctrl: Vec::new(),
            deciding: Vec::new(),
            providers: Vec::new(),
            key,
            results,
            ret: None,
            scopes: HashMap::default(),
            defer_flags: HashMap::default(),
            shield: 0,
            cancel_to: None,
            susp: None,
            ops: None,
            open_ctx: None,
            open_base: 0,
            here: None,
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

    /// `if pc == 0 || lo <= pc <= hi` around code that holds the states
    /// `x` covers: it runs fresh or when resuming into it, never when
    /// resuming past it. Code without a state is fresh-path-only.
    fn entered(&mut self, x: u32, f: impl FnOnce(&mut Self) -> StageResult<()>) -> StageResult<()> {
        let Some((lo, hi)) = self.range(x) else {
            return self.fresh_only(f);
        };
        let pc = self.pc();
        self.a.get(pc);
        self.a.s().i32_eqz();
        self.in_range(lo, hi);
        self.a.s().i32_or();
        self.a.if_();
        self.open(Ctl::Plain);
        f(self)?;
        self.ctrl.pop();
        self.a.end();
        Ok(())
    }

    /// With the branch condition of `i` on the stack: on resume, replaces
    /// it by whether the resumed state is in `then`, so a resumed branch
    /// enters the side that holds the point (`crates/hd_wasm/DESIGN.md` §14.2). A
    /// `select`, since structured blocks here carry no values.
    fn resume_branch(&mut self, i: u32, then: u32) {
        if self.range(i).is_none() {
            return;
        }
        match self.range(then) {
            Some((lo, hi)) => self.in_range(lo, hi),
            None => self.a.i32(0),
        }
        let pc = self.pc();
        self.a.get(pc);
        self.a.s().i32_eqz();
        self.a.s().select();
    }

    /// A block list that contains a suspension point (`crates/hd_wasm/DESIGN.md`
    /// §14.2): one `br_table` on `pc` enters it. The list is cut into
    /// segments, each starting at an item that holds a state, after a
    /// fresh-only prefix. Each segment follows the `end` of its own block,
    /// so fresh code falls through every segment, and a resume branches
    /// out of the block before the segment that holds its state.
    fn resume_list(&mut self, items: &[u32]) -> StageResult<()> {
        let points: Vec<usize> = (0..items.len())
            .filter(|&j| self.range(items[j]).is_some())
            .collect();
        let Some(&first) = points.first() else {
            for &i in items {
                self.inst(i)?;
            }
            return Ok(());
        };
        // One item holds every state: a resume always enters it, so only
        // the prefix needs a guard.
        if points.len() == 1 {
            if first > 0 {
                self.fresh_only(|em| {
                    for &i in &items[..first] {
                        em.inst(i)?;
                    }
                    Ok(())
                })?;
            }
            for &i in &items[first..] {
                self.inst(i)?;
            }
            return Ok(());
        }
        let ranges: Vec<(u32, u32)> = points
            .iter()
            .filter_map(|&j| self.range(items[j]))
            .collect();
        let (lo, hi) = (ranges[0].0, ranges[ranges.len() - 1].1);
        // Block `k` ends before segment `k`; segment 0 is the prefix.
        for _ in 0..=points.len() {
            self.a.block();
            self.open(Ctl::Plain);
        }
        // `pc - lo`: states start at 1, so a fresh `pc` of 0 wraps past
        // the table and takes the default, the prefix.
        let targets: Vec<u32> = (lo..=hi)
            .map(|pc| {
                ranges
                    .iter()
                    .position(|&(a, b)| a <= pc && pc <= b)
                    .map_or(0, |k| u32_of(k + 1))
            })
            .collect();
        let pc = self.pc();
        self.a.get(pc);
        self.a.i32(lo.cast_signed());
        self.a.s().i32_sub();
        self.a.s().br_table(targets, 0);
        let close = |em: &mut Self| {
            em.ctrl.pop();
            em.a.end();
        };
        close(self);
        for &i in &items[..first] {
            self.inst(i)?;
        }
        for (k, &p) in points.iter().enumerate() {
            close(self);
            let next = points.get(k + 1).copied().unwrap_or(items.len());
            for &i in &items[p..next] {
                self.inst(i)?;
            }
        }
        Ok(())
    }

    /// A fieldless data value is canonical for its data type
    /// (`expr.is.canonical-data`): one constant global per type, read here
    /// without allocating (`expr.is.canonical.same`).
    /// `a is b` on two references (`crates/hd_wasm/DESIGN.md` §15.2,
    /// "References"): their identity fields are equal, the bases by
    /// `ref.eq` on the payloads, the serials by `i64.eq`, and the keys as
    /// strings.
    fn ref_identity_eq(&mut self, i: u32, a: u32, b: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        let Shape::Data { ty: st, fields } = self.lay.shape(t)? else {
            return unsupported("a reference without a struct layout");
        };
        let (Some(base), Some(serial), Some(key)) = (fields.get(2), fields.get(3), fields.get(4))
        else {
            return unsupported("a reference without its identity fields");
        };
        let (base, serial, key) = (base.0, serial.0, key.0);
        let ra = self.a.local(VT::r(st.clone()));
        let rb = self.a.local(VT::r(st.clone()));
        self.comp(a, 0, &VT::r(st.clone()))?;
        self.a.set(ra);
        self.comp(b, 0, &VT::r(st.clone()))?;
        self.a.set(rb);
        for r in [ra, rb] {
            self.a.get(r);
            self.a.struct_get(&st, base);
        }
        self.a.s().ref_eq();
        for r in [ra, rb] {
            self.a.get(r);
            self.a.struct_get(&st, serial);
        }
        self.a.s().i64_eq().i32_and();
        for r in [ra, rb] {
            self.a.get(r);
            self.a.struct_get(&st, key);
            self.a.get(r);
            self.a.struct_get(&st, key + 1);
        }
        self.a.call(Sym::Helper(Helper::StrEq));
        self.a.s().i32_and();
        self.store_from(i, &[VT::I32])
    }

    fn canonical_data(&mut self, i: u32, ty: Ty, st: WTy) -> StageResult<()> {
        let h = self.type_hash("canonical-data", ty);
        self.a.global_get(GSym::Canon(h, VT::r(st)));
        self.store(i)
    }

    /// The canonical encoding of `ty`, newtypes stripped, hashed under
    /// `domain`: a global per type names it.
    fn type_hash(&self, domain: &str, ty: Ty) -> Hash128 {
        let ty = self.lay.strip(ty);
        let mut h = StableHasher::new(domain);
        let path_hash = |d: DefId| self.env().path_hash(d);
        h.hash(canon(
            self.pool(),
            &path_hash,
            &mut CanonMemo::default(),
            ty,
        ));
        h.finish()
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
            // The unwinding ended: the body hands back its frame, still
            // marked `CANCELLED`, so a direct caller sees that no value
            // came (`crates/hd_wasm/DESIGN.md` §14.6).
            Exit::Cancel => {
                let (ft, _, frame, _) = self.susp_parts()?;
                for v in self.results.clone() {
                    self.a.zero(&v);
                }
                self.a.raw_get(frame);
                self.a.raw_get(frame);
                self.a.struct_get(&ft, F_FLAGS);
                self.a.i32(!UNWINDING);
                self.a.s().i32_and();
                self.a.struct_set(&ft, F_FLAGS);
                self.a.raw_get(frame);
                self.a.s().return_();
            }
        }
        Ok(())
    }

    /// A `Scope` with `defer` suites: the exit ladder
    /// (`crates/hd_mono/DESIGN.md` §12.2). Each suite has a "registered"
    /// flag; every exit stores its code and branches to the scope's end,
    /// which runs the registered suites, last in, first out, then takes
    /// the exit.
    fn scope(&mut self, i: u32, body: u32, rec: u32) -> StageResult<()> {
        let entries = self.rec(rec);
        self.result(i)?;
        let code = self.a.local(VT::I32);
        let flags: Vec<u32> = entries
            .iter()
            .map(|&s| {
                let f = self.a.local(VT::I32);
                self.defer_flags.insert(s, f);
                f
            })
            .collect();
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
        // A suite that holds a suspension point: a `using` binding's
        // `dispose!` (flow.using.suite-suspends).
        let waits: Vec<Option<(u32, u32)>> = entries.iter().map(|&s| self.range(s)).collect();
        if waits.iter().any(Option::is_some) {
            return self.waiting_scope(i, body, code, &entries, &flags, &waits);
        }
        self.a.block();
        self.open(Ctl::Scope(i));
        self.block_into(body, Some(i))?;
        self.a.i32(0);
        self.a.set(code);
        self.ctrl.pop();
        self.a.end();
        for (e, f) in entries.iter().zip(&flags).rev() {
            self.a.get(*f);
            self.a.if_();
            self.open(Ctl::Plain);
            self.run_suite(*e)?;
            self.ctrl.pop();
            self.a.end();
        }
        self.scope_exits(i, code)
    }

    /// The exit ladder's rung for `defer` suite `s`, whose flag is set.
    fn run_suite(&mut self, s: u32) -> StageResult<()> {
        if self.range(s).is_some() {
            // A suite that waits is a `using` binding's `dispose!`, which
            // runs under the driver (flow.using.not-forbidden), so it is no
            // forbidden context; a count held across its wait would leak to
            // others.
            self.shield += 1;
            let r = self.block_into(s, None);
            self.shield -= 1;
            return r;
        }
        // A `defer` suite is a forbidden context
        // (`req.drive.block-on.indirect`): `block_on` reached from it
        // panics. The suite cannot leave early, so the count balances.
        self.forbid(1);
        self.block_into(s, None)?;
        self.forbid(-1);
        Ok(())
    }

    /// The exit ladder of a scope with a suspending entry, a `using`
    /// binding's `dispose!` (`crates/hd_wasm/DESIGN.md` §14.6). A resume
    /// into an entry skips the body and the entries that already ran,
    /// which cleared their flags; `code` holds the saved exit. A dispose
    /// runs to its end even when the task is cancelled (flow.using.cancel).
    fn waiting_scope(
        &mut self,
        i: u32,
        body: u32,
        code: u32,
        entries: &[u32],
        flags: &[u32],
        waits: &[Option<(u32, u32)>],
    ) -> StageResult<()> {
        self.a.block();
        self.open(Ctl::Scope(i));
        self.entered(body, |em| {
            em.block_into(body, Some(i))?;
            em.a.i32(0);
            em.a.set(code);
            Ok(())
        })?;
        self.ctrl.pop();
        self.a.end();
        let (ft, _, frame, _) = self.susp_parts()?;
        for ((e, f), w) in entries.iter().zip(flags).zip(waits).rev() {
            // Registered, or resumed into.
            let live = self.a.local(VT::I32);
            self.a.get(*f);
            if let Some((lo, hi)) = *w {
                self.in_range(lo, hi);
                self.a.s().i32_or();
            }
            self.a.set(live);
            self.a.get(live);
            self.a.if_();
            self.open(Ctl::Plain);
            self.a.i32(0);
            self.a.set(*f);
            self.run_suite(*e)?;
            self.ctrl.pop();
            self.a.end();
        }
        // Cancelled while a shielded `dispose!` waited: once the ladder
        // ran, the scope leaves by `Cancel`, whatever exit it was taking.
        let cancel = {
            let Some(info) = self.scopes.get_mut(&i) else {
                return unsupported("a cleanup scope without its ladder");
            };
            if let Some(k) = info.exits.iter().position(|x| *x == Exit::Cancel) {
                k + 1
            } else {
                info.exits.push(Exit::Cancel);
                info.exits.len()
            }
        };
        self.a.raw_get(frame);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_FLAGS);
        self.a.i32(CANCELLED);
        self.a.s().i32_and();
        self.a.if_();
        self.a.i32(i32::try_from(cancel).unwrap_or(0));
        self.a.set(code);
        self.a.end();
        self.a.end();
        self.scope_exits(i, code)
    }

    /// The exits a scope's ladder ends in: each taken by its code.
    fn scope_exits(&mut self, i: u32, code: u32) -> StageResult<()> {
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

    /// `$.with(K = p, ctx..., ...): block`: each provider, a trait value
    /// of its key since checking, and each spread context's providers
    /// cover their keys in the block, later entries innermost
    /// (`crates/hd_mono/DESIGN.md` §12.4, "Context values").
    fn with(&mut self, i: u32, rec: u32, body: u32) -> StageResult<()> {
        let pairs = self.rec(rec);
        let mut pushed = 0;
        for (k, v) in pairs.chunks(2).map(|c| (c[0], c[1])) {
            let key_t = self.sub(Ty(k));
            if matches!(self.pool().get(key_t), TyData::Context(_)) {
                pushed += self.push_context(v, key_t)?;
                continue;
            }
            if !matches!(self.pool().get(key_t), TyData::TraitValue { .. }) {
                return unsupported("a `$.with` key that is not a trait");
            }
            let vs = self.vts(key_t)?;
            let [pv, vv] = vs.as_slice() else {
                return unsupported("a trait value that is not two values");
            };
            let ls = [self.a.local(pv.clone()), self.a.local(vv.clone())];
            self.load_as(v, &vs)?;
            self.a.set(ls[1]);
            self.a.set(ls[0]);
            self.providers.push((key_t, ls));
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

    /// Pushes a vtable (`crates/hd_mono/DESIGN.md` §13.5, §13.16) of adapters over the
    /// slots' targets, for a payload of values `fv` erased to `eqref`: its
    /// method slots, then its direct supertraits' vtables. A supertrait
    /// reached twice, as `Display` is through two paths of a diamond, is
    /// built once.
    /// A trait value's vtable tree is constant, `ref.func` of its adapters
    /// under `struct.new`, so it is one immutable global, read here with
    /// no allocation (`GSym::Vtable`). The tree is written into its own
    /// constant expression; a tree that repeats a subtable repeats its
    /// `struct.new`, which runs once, when the module is instantiated.
    fn vtable_value(&mut self, table: &VTable, key: Ty, from: Ty, fv: &[VT]) -> StageResult<()> {
        let mut uses: HashMap<Ty, u32> = HashMap::default();
        self.count_tables(table, key, &mut uses)?;
        // Collection records a repeated subtable's slots at its first
        // occurrence only, so each later occurrence is that one again.
        let table = self.first_tables(table, key, &mut HashMap::default());
        let outer = std::mem::replace(&mut self.a, Asm::new(vec![]));
        let read = if table.has_read() {
            Some(self.ref_read(from)?)
        } else {
            None
        };
        let pushed = self.push_table(
            &table,
            key,
            (fv, read.as_ref()),
            &HashMap::default(),
            &mut HashMap::default(),
        );
        let tree = std::mem::replace(&mut self.a, outer);
        pushed?;
        let init = tree.into_const();
        let vt = self.lay.vtable(key)?;
        self.a
            .global_get(GSym::Vtable(init.hash(), VT::r(vt), Box::new(init)));
        Ok(())
    }

    /// A vtable tree in which each repeated key is its first occurrence,
    /// in the order [`Self::push_table`] visits them.
    fn first_tables(&self, table: &VTable, key: Ty, firsts: &mut HashMap<Ty, VTable>) -> VTable {
        if let Some(first) = firsts.get(&key) {
            return first.clone();
        }
        let supers = self.lay.dyn_supers(key);
        let parents = table
            .parents
            .iter()
            .zip(supers)
            .map(|(p, s)| self.first_tables(p, s, firsts))
            .collect();
        let whole = VTable {
            trait_: table.trait_,
            slots: table.slots.clone(),
            recv: table.recv.clone(),
            parents,
        };
        firsts.insert(key, whole.clone());
        whole
    }

    /// How many times each vtable key occurs in a vtable tree.
    fn count_tables(
        &self,
        table: &VTable,
        key: Ty,
        uses: &mut HashMap<Ty, u32>,
    ) -> StageResult<()> {
        *uses.entry(key).or_default() += 1;
        let supers = self.lay.dyn_supers(key);
        if supers.len() != table.parents.len() {
            return unsupported("a vtable whose parents collection did not record");
        }
        for (p, s) in table.parents.iter().zip(supers) {
            self.count_tables(p, s, uses)?;
        }
        Ok(())
    }

    /// How an adapter reads the value a reference payload of type `from`
    /// (a lowered `&T`, `std.core.Ref[T]`) refers to: a call of its `get`
    /// closure ("Trait value references").
    fn ref_read(&self, from: Ty) -> StageResult<RefRead> {
        let Shape::Data { ty: data, fields } = self.lay.shape(from)? else {
            return unsupported("a reference payload without a struct layout");
        };
        let (pool, env) = (self.pool(), self.env());
        let TyData::Adt { def, args } = pool.get(from) else {
            return unsupported("a reference payload that is no reference");
        };
        let Some(get_ty) = env.data_fields(def).and_then(|f| f.first().copied()) else {
            return unsupported("a reference type without its `get` field");
        };
        let get_ty = subst(pool, env, def, args, get_ty);
        let Shape::Fn { base, code } = self.lay.shape(get_ty)? else {
            return unsupported("a reference's `get` that is no closure");
        };
        let WTy::Func(_, results) = &code else {
            return unsupported("a closure code type");
        };
        let Some((start, vs)) = fields.first() else {
            return unsupported("a reference without fields");
        };
        let [field_vt] = vs.as_slice() else {
            return unsupported("a reference's `get` field of several values");
        };
        Ok(RefRead {
            data,
            field: *start,
            field_vt: field_vt.clone(),
            results: results.clone(),
            cast: self.lay.closure_code() == ClosureCode::Abstract,
            base,
            code,
        })
    }

    fn push_table(
        &mut self,
        table: &VTable,
        key: Ty,
        (fv, read): (&[VT], Option<&RefRead>),
        uses: &HashMap<Ty, u32>,
        built: &mut HashMap<Ty, u32>,
    ) -> StageResult<()> {
        let trait_ = table.trait_;
        if let Some(&l) = built.get(&key) {
            self.a.get(l);
            return Ok(());
        }
        let vt = self.lay.vtable(key)?;
        let methods = self.env().trait_methods(trait_);
        for ((m, t), recv) in methods.iter().zip(&table.slots).zip(&table.recv) {
            let sig = self.lay.slot_sig(key, *m)?;
            if *recv == SlotRecv::Unavailable {
                self.a.ref_func(Sym::Helper(Helper::Unlowered {
                    sig,
                    what: "a `&self` or `&mut self` method through a trait value that holds no reference".into(),
                }));
                continue;
            }
            // A method the compiler supplies for every type: the vtable's
            // `runtime_type` answers the recorded type, the concrete type
            // the vtable is built at (trait.inspect.dynamic). Any other
            // has no lowering yet: its slot panics when called, as a host
            // slot does.
            if let TargetKind::Builtin { self_ty, .. } = t.kind {
                let path = (self.lay.path)(*m);
                if path.rsplit(['.', '/']).next() == Some("runtime_type") {
                    let (ty, name, shown) = self.type_id_parts(self_ty, t.ret)?;
                    self.a.ref_func(Sym::Helper(Helper::RuntimeType {
                        sig,
                        ty,
                        name,
                        shown,
                    }));
                } else {
                    self.a.ref_func(Sym::Helper(Helper::Unlowered {
                        sig,
                        what: format!("the compiler-supplied `{path}`"),
                    }));
                }
                continue;
            }
            let target = Self::adapter_target(t)?;
            let mut ps = Vec::new();
            for p in self.env().params(t.item).unwrap_or_default() {
                ps.extend(self.vts(subst(self.pool(), self.env(), t.item, t.args, p))?);
            }
            // An erased body takes its witness after its parameters.
            if self.lay.slot_open(key, *m).is_some() {
                ps.push(VT::r(witness_ty()));
            }
            let mut results = self.vts(t.ret)?;
            if self.env().suspends(*m) {
                results = vec![VT::r(suspend_base(&results).0)];
            }
            // The impl's row keys among the slot's: the impl method's row
            // is the trait method's (trait.impl.signature).
            let keys = self.lay.slot_keys(key, *m);
            let mut provs = Vec::new();
            for k in self.env().row_keys(t.item, t.args) {
                let Some(at) = keys.iter().position(|x| *x == k) else {
                    return unsupported(format!(
                        "a vtable slot whose implementation's row has `{}`, which the trait method's lacks",
                        self.pool().display(k)
                    ));
                };
                provs.push(u32_of(at));
            }
            // The context of the method's own row parameters is the
            // slot's last two parameters, which the erased body takes last
            // too: one more pair, passed on as it is.
            let mut pairs = u32_of(keys.len());
            if has_row_params(self.env(), *m) {
                provs.push(pairs);
                pairs += 1;
            }
            let read = match (recv, read) {
                (SlotRecv::ReadThrough, Some(rd)) => Some(Box::new(rd.clone())),
                (SlotRecv::ReadThrough, None) => {
                    return unsupported(
                        "a vtable slot that reads through a payload of no reference",
                    );
                }
                _ => None,
            };
            self.a.ref_func(Sym::Helper(Helper::Adapter {
                sig,
                self_vts: fv.to_vec(),
                target: Box::new(target),
                params: ps,
                results,
                keys: pairs,
                provs,
                read,
            }));
        }
        let supers = self.lay.dyn_supers(key);
        for (p, s) in table.parents.iter().zip(supers) {
            self.push_table(p, s, (fv, read), uses, built)?;
        }
        self.a.struct_new(&vt);
        if uses.get(&key).is_some_and(|n| *n > 1) {
            let l = self.a.local(VT::r(vt));
            self.a.set(l);
            self.a.get(l);
            built.insert(key, l);
        }
        Ok(())
    }

    // ------------------------------------------------------ suspension

    fn susp_parts(&self) -> StageResult<(WTy, WTy, u32, u32)> {
        let Some(s) = &self.susp else {
            return unsupported("a suspension point outside a suspending body");
        };
        Ok((s.frame_ty.clone(), s.base.clone(), s.frame, s.pc))
    }

    /// Saves every saved local into the frame (`crates/hd_wasm/DESIGN.md` §14.3).
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

    /// Allocates the frame when it is still null (§14.3), with its own
    /// cancel and poll functions.
    fn materialize(&mut self) -> StageResult<()> {
        let (ft, _, frame, _) = self.susp_parts()?;
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
        Ok(())
    }

    /// The awaited child `child` (a local; `None` for `all!`, whose
    /// children are its arguments) was cancelled by another task, so this
    /// task is cancelled at the point (`req.cancel.await`): the frame is
    /// marked `CANCELLED` and `UNWINDING`, and the point's cancellation
    /// branch runs, which folds the child's cleanup into this task's
    /// (`crates/hd_wasm/DESIGN.md` §14.6).
    fn cancel_here(&mut self, child: Option<u32>) -> StageResult<()> {
        let Some(at) = self.cancel_to else {
            return unsupported("a cancelled await outside a point");
        };
        let (ft, _, frame, _) = self.susp_parts()?;
        self.materialize()?;
        if let Some(c) = child {
            self.a.raw_get(frame);
            self.a.raw_get(c);
            self.a.struct_set(&ft, F_CHILD);
        }
        self.a.raw_get(frame);
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_FLAGS);
        self.a.i32(CANCELLED | UNWINDING);
        self.a.s().i32_or();
        self.a.struct_set(&ft, F_FLAGS);
        self.a.br(u32_of(self.ctrl.len() - 1 - at));
        Ok(())
    }

    /// Pushes whether the task in local `c`, of struct type `ty`, is
    /// marked `CANCELLED`; `c` is not null.
    fn is_cancelled(&mut self, c: u32, ty: &WTy) {
        self.a.raw_get(c);
        self.a.s().ref_as_non_null();
        self.a.struct_get(ty, F_FLAGS);
        self.a.i32(CANCELLED);
        self.a.s().i32_and();
    }

    /// Pending at state `k` with the child on the stack: allocates the
    /// frame on the first real wait (§14.3), saves, and returns Pending.
    fn suspend(&mut self, k: u32, child: u32) -> StageResult<()> {
        let (ft, base, frame, _) = self.susp_parts()?;
        self.materialize()?;
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

    /// Sets local `wait` when the child task in local `c` is unwinding.
    fn unwinding_into(&mut self, c: u32, wait: u32) {
        let task = task_base();
        self.a.raw_get(c);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.a.raw_get(c);
        self.a.s().ref_as_non_null();
        self.a.struct_get(&task, F_FLAGS);
        self.a.i32(UNWINDING);
        self.a.s().i32_and();
        self.a.if_();
        self.a.i32(1);
        self.a.set(wait);
        self.a.end();
        self.a.end();
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

    /// Opens a shielded point's block (`crates/hd_wasm/DESIGN.md` §14.6,
    /// "Abandoning"). Resumed into under `abandon()`, the point kills its
    /// waiting child, the `dispose!`, with no step: it leaves the block
    /// with `out` null and the results zero, so the suite goes on as if
    /// the dispose returned.
    fn shield_enter(&mut self, k: u32, child: u32, tmp: &[u32], out: u32) -> StageResult<()> {
        let (_, _, _, pc) = self.susp_parts()?;
        self.a.block();
        self.open(Ctl::Plain);
        self.a.get(pc);
        self.a.i32(k.cast_signed());
        self.a.s().i32_eq();
        self.a.global_get(crate::rt::abandoning_global());
        self.a.s().i32_eqz().i32_eqz().i32_and();
        self.a.if_();
        self.cancel_task(child);
        let out_ty = self.a.decl(out);
        self.a.zero(&out_ty);
        self.a.set(out);
        for l in tmp {
            let v = self.a.decl(*l);
            self.a.zero(&v);
            self.a.set(*l);
        }
        self.a.br(1);
        self.a.end();
        Ok(())
    }

    /// Closes a shielded point's block after the child step left its
    /// pending child in `out` (null when it returned). A child left
    /// pending under `abandon()`, or one that was cancelled because it
    /// awaited a suspension cancelled elsewhere (a failed dependency,
    /// `req.abandon.dependency`), is abandoned, and `out` is cleared, so
    /// the suite goes on; this frame's cleanup is then `ABANDONED`.
    fn shield_leave(&mut self, out: u32) {
        let task = task_base();
        let frame = self.susp.as_ref().map(|s| (s.frame_ty.clone(), s.frame));
        self.a.raw_get(out);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.a.global_get(crate::rt::abandoning_global());
        self.is_cancelled(out, &task);
        self.a.s().i32_or();
        self.a.if_();
        if let Some((ft, f)) = frame {
            self.a.raw_get(f);
            self.a.s().ref_is_null().i32_eqz();
            self.a.if_();
            self.a.raw_get(f);
            self.a.raw_get(f);
            self.a.struct_get(&ft, F_FLAGS);
            self.a.i32(ABANDONED);
            self.a.s().i32_or();
            self.a.struct_set(&ft, F_FLAGS);
            self.a.end();
        }
        abandon_by(&mut self.a, 1);
        self.cancel_task(out);
        abandon_by(&mut self.a, -1);
        let out_ty = self.a.decl(out);
        self.a.zero(&out_ty);
        self.a.set(out);
        self.a.end();
        self.a.end();
        self.ctrl.pop();
        self.a.end();
    }

    /// A suspension point (`crates/hd_wasm/DESIGN.md` §14.2 to §14.6).
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
        // A point in a `using` binding's dispose suite is shielded: it has
        // no cancellation branch, and waits on.
        if self.shield > 0 {
            return self.await_point(i, k, a, bw, &children, done);
        }
        // The cancellation branch follows the end of block `cancel`: it is
        // entered on a resume for cancellation, or when the awaited child
        // turns out to be cancelled (`Em::cancel_here`). A Ready await
        // leaves block `out`.
        self.a.block();
        self.open(Ctl::Plain);
        self.a.block();
        self.open(Ctl::Plain);
        let at = self.ctrl.len() - 1;
        self.a.get(pc);
        self.a.i32(k.cast_signed());
        self.a.s().i32_eq();
        self.a.if_();
        self.open(Ctl::Plain);
        self.a.raw_get(frame);
        self.a.struct_get(&ft, F_FLAGS);
        self.a.i32(CANCELLED);
        self.a.s().i32_and();
        self.a.br_if(1);
        self.ctrl.pop();
        self.a.end();
        let outer = self.cancel_to.replace(at);
        let r = self.await_point(i, k, a, bw, &children, done);
        self.cancel_to = outer;
        r?;
        self.a.br(1);
        self.ctrl.pop();
        self.a.end();
        // Cancel the child, then leave through the enclosing scopes'
        // ladders, which run the registered suites. A child left unwinding
        // (its shielded `dispose!` waits) is waited for here first: each
        // later cancel call continues it. The child cancels are bracketed
        // as a frame's cancel brackets its work, so a folded cleanup that
        // ended `.Abandoned` marks this frame also when the branch runs
        // from an await, outside any cancel of this frame
        // (`req.abandon.carry`, `req.cancel.await.unwind`).
        let c = self.a.local(VT::rn(task.clone()));
        let wait = self.a.local(VT::I32);
        let seen = self.a.local(VT::I32);
        self.a.i32(0);
        self.a.set(wait);
        self.a.call(Sym::Helper(Helper::CancelEnter));
        self.a.set(seen);
        if tag == Tag::AwaitAll {
            for (j, ch) in children.iter().enumerate() {
                self.a.get(done);
                self.a.i32(1 << j);
                self.a.s().i32_and().i32_eqz();
                self.a.if_();
                self.comp(*ch, 0, &VT::rn(task.clone()))?;
                self.a.set(c);
                self.cancel_task(c);
                self.unwinding_into(c, wait);
                self.a.end();
            }
            self.a.ref_null(&task);
            self.a.set(c);
        } else {
            self.a.raw_get(frame);
            self.a.struct_get(&ft, F_CHILD);
            self.a.set(c);
            self.cancel_task(c);
            self.unwinding_into(c, wait);
        }
        self.a.raw_get(frame);
        self.a.s().ref_as_non_null();
        self.a.get(seen);
        self.a.call(Sym::Helper(Helper::CancelLeave));
        self.a.get(wait);
        self.a.if_();
        self.suspend(k, c)?;
        self.a.end();
        // The ladders run as fresh code: a dispose suite's guards test
        // `pc` (flow.using.cancel).
        self.a.i32(0);
        self.a.set(pc);
        self.exit(Exit::Cancel)?;
        self.ctrl.pop();
        self.a.end();
        Ok(())
    }

    /// The await of point `i` at state `k`: polls or steps its child, or
    /// its children for `all!`.
    fn await_point(
        &mut self,
        i: u32,
        k: u32,
        a: u32,
        bw: u32,
        children: &[u32],
        done: u32,
    ) -> StageResult<()> {
        let tag = self.b.tags[i as usize];
        let ty = self.sub(self.b.ty[i as usize]);
        let res = self.vts(ty)?;
        match tag {
            Tag::AwaitAll => self.await_all(i, k, children, done, ty),
            Tag::AwaitValue => self.await_stored(i, k, &res, &res, |em, base| {
                em.comp(a, 0, &VT::r(base.clone()))
            }),
            _ => {
                let Some(c) = Callee::from_words(self.b.record(a)) else {
                    return Err(NotImplemented::bug(
                        Stage::Emit,
                        "a malformed callee record",
                    ));
                };
                let rec = self.rec(bw);
                let args = rec[..rec.len().saturating_sub(3)].to_vec();
                if let (
                    Some(Target::Dyn),
                    Callee::TraitMethod {
                        trait_,
                        method,
                        targs,
                        ..
                    },
                ) = (self.calls.get(&i), c)
                {
                    let targs_sub: Vec<Ty> = self
                        .pool()
                        .list_items(targs)
                        .iter()
                        .map(|t| self.sub(*t))
                        .collect();
                    let slot =
                        self.lay
                            .dyn_slot(self.ty_of(args[0]), trait_, &targs_sub, method)?;
                    let stored = if slot.open.is_some_and(|o| o.ret) {
                        // The cold frame is one reference either way; its
                        // poll returns the open result as `eqref`.
                        vec![VT::Eq]
                    } else {
                        res.clone()
                    };
                    return self.await_stored(i, k, &stored, &res, |em, _| {
                        em.push_dyn(trait_, method, targs, &args, ty).map(|_| ())
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
        let tmp: Vec<u32> = gres.iter().map(|v| self.a.local(v.dflt())).collect();
        let nc = self.a.local(VT::rn(gbase.clone()));
        // A point in a `using` binding's dispose suite is shielded: under
        // `abandon()`, or when its `dispose!` loses a dependency, it kills
        // the `dispose!` instead of waiting (`crates/hd_wasm/DESIGN.md`
        // §14.6, "Abandoning").
        let shielded = self.shield > 0;
        if shielded {
            self.shield_enter(k, ch, &tmp, nc)?;
        }
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
        self.push_call_providers(t.item, t.args)?;
        self.a.call(Sym::Part(t.key, Part::Body));
        self.a.set(nc);
        for l in tmp.iter().rev() {
            self.a.set(*l);
        }
        if shielded {
            self.shield_leave(nc);
        }
        self.a.raw_get(nc);
        self.a.s().ref_is_null().i32_eqz();
        self.a.if_();
        self.open(Ctl::Plain);
        // A child that ended cancelled hands back its frame: this task is
        // cancelled too (`req.cancel.await`).
        if self.cancel_to.is_some() {
            self.is_cancelled(nc, &gbase);
            self.a.if_();
            self.open(Ctl::Plain);
            self.cancel_here(Some(nc))?;
            self.ctrl.pop();
            self.a.end();
        }
        self.suspend(k, nc)?;
        self.ctrl.pop();
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
        stored: &[VT],
        res: &[VT],
        fresh: impl FnOnce(&mut Self, &WTy) -> StageResult<()>,
    ) -> StageResult<()> {
        let (ft, _, frame, pc) = self.susp_parts()?;
        let (base, poll) = suspend_base(stored);
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
        let tmp: Vec<u32> = stored.iter().map(|v| self.a.local(v.dflt())).collect();
        let ready = self.a.local(VT::I32);
        // A point in a `using` binding's dispose suite is shielded: under
        // `abandon()`, or when its `dispose!` loses a dependency, it kills
        // the `dispose!` instead of waiting (`crates/hd_wasm/DESIGN.md`
        // §14.6, "Abandoning"). `gone` is the child left to wait on.
        let shielded = self.shield > 0;
        let gone = self.a.local(VT::rn(base.clone()));
        if shielded {
            self.shield_enter(k, s, &tmp, gone)?;
        }
        self.a.raw_get(s);
        self.a.s().ref_as_non_null();
        self.a.raw_get(s);
        self.a.s().ref_as_non_null();
        self.a.struct_get(&base, F_POLL);
        self.a.call_ref(&poll);
        for l in tmp.iter().rev() {
            self.a.set(*l);
        }
        self.a.set(ready);
        if shielded {
            self.a.ref_null(&base);
            self.a.set(gone);
            self.a.get(ready);
            self.a.s().i32_eqz();
            self.a.if_();
            self.a.raw_get(s);
            self.a.set(gone);
            self.a.end();
            self.shield_leave(gone);
            self.a.raw_get(gone);
            self.a.s().ref_is_null();
            self.a.set(ready);
        }
        self.a.get(ready);
        self.a.s().i32_eqz();
        self.a.if_();
        self.open(Ctl::Plain);
        // A stored suspension that another task cancelled answers Pending
        // and stays `CANCELLED`: this task is cancelled too
        // (`req.cancel.await`).
        if self.cancel_to.is_some() {
            self.is_cancelled(s, &base);
            self.a.if_();
            self.open(Ctl::Plain);
            self.cancel_here(Some(s))?;
            self.ctrl.pop();
            self.a.end();
        }
        self.suspend(k, s)?;
        self.ctrl.pop();
        self.a.end();
        self.a.i32(0);
        self.a.set(pc);
        let dres: Vec<VT> = res.iter().map(VT::dflt).collect();
        if stored == res {
            for l in &tmp {
                self.a.raw_get(*l);
            }
        } else {
            // An erased generic method's poll returns its open result;
            // only now, at the concrete await, unbox or cast it.
            let [one] = stored else {
                return unsupported("an erased result with several values");
            };
            if *one != VT::Eq {
                return unsupported("an erased result that is not one reference");
            }
            unerase(&mut self.a, tmp[0], &dres);
        }
        self.store_from(i, &dres)
    }

    /// `all!(a, b, ...)` (`crates/hd_wasm/DESIGN.md` §14.5): polls the unfinished
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
        // The `all!`'s waker node (`crates/hd_wasm/DESIGN.md` §14.4): made on
        // the first poll with every bit set, so each child is polled
        // once; later polls visit only the children whose bit a wake set.
        let waker = crate::rt::wake_node();
        let wn = self.a.local(VT::rn(waker.clone()));
        let (outer, outer_slot, regs) = (
            self.a.local(VT::Eq),
            self.a.local(VT::I32),
            self.a.local(VT::I32),
        );
        self.fresh_only(|em| {
            em.a.i32(0);
            em.a.set(done);
            em.a.i32(-1);
            em.a.global_get(crate::rt::wake_cur_node());
            em.a.global_get(crate::rt::wake_cur_slot());
            em.a.struct_new(&waker);
            em.a.set(wn);
            Ok(())
        })?;
        self.a.global_get(crate::rt::wake_cur_node());
        self.a.set(outer);
        self.a.global_get(crate::rt::wake_cur_slot());
        self.a.set(outer_slot);
        // Whether a child polled now turned out cancelled by another task
        // (`req.cancel.await`).
        let gone = self.a.local(VT::I32);
        self.a.i32(0);
        self.a.set(gone);
        let mut keep = Vec::new();
        for (j, (ch, rv)) in children.iter().zip(&elems).enumerate() {
            let (base, poll) = suspend_base(rv);
            let ls: Vec<u32> = rv.iter().map(|v| self.a.local(v.dflt())).collect();
            let tmp: Vec<u32> = rv.iter().map(|v| self.a.local(v.dflt())).collect();
            let s = self.a.local(VT::r(base.clone()));
            let bit = 1i32 << j;
            // Unfinished and woken: clear the bit, then poll under it.
            self.a.get(done);
            self.a.i32(bit);
            self.a.s().i32_and().i32_eqz();
            self.a.get(wn);
            self.a.struct_get(&waker, 0);
            self.a.i32(bit);
            self.a.s().i32_and().i32_const(0).i32_ne().i32_and();
            self.a.if_();
            self.a.get(wn);
            self.a.get(wn);
            self.a.struct_get(&waker, 0);
            self.a.i32(!bit);
            self.a.s().i32_and();
            self.a.struct_set(&waker, 0);
            self.a.get(wn);
            self.a.global_set(crate::rt::wake_cur_node());
            self.a.i32(i32::try_from(j).expect("slot"));
            self.a.global_set(crate::rt::wake_cur_slot());
            self.a.global_get(crate::rt::wake_regs());
            self.a.set(regs);
            self.comp(*ch, 0, &VT::r(base.clone()))?;
            self.a.set(s);
            self.a.global_get(crate::rt::all_polls());
            self.a.i32(1);
            self.a.s().i32_add();
            self.a.global_set(crate::rt::all_polls());
            self.a.get(s);
            self.a.get(s);
            self.a.struct_get(&base, F_POLL);
            self.a.call_ref(&poll);
            for l in tmp.iter().rev() {
                self.a.set(*l);
            }
            self.a.raw_get(outer);
            self.a.global_set(crate::rt::wake_cur_node());
            self.a.get(outer_slot);
            self.a.global_set(crate::rt::wake_cur_slot());
            self.a.if_();
            self.a.get(done);
            self.a.i32(bit);
            self.a.s().i32_or();
            self.a.set(done);
            for (d, t) in ls.iter().zip(&tmp) {
                self.a.raw_get(*t);
                self.a.set(*d);
            }
            self.a.else_();
            self.a.get(gone);
            self.is_cancelled(s, &base);
            self.a.s().i32_or();
            self.a.set(gone);
            // A Pending child that registered no host leaf has nothing
            // that would wake it: it stays woken, and so does this
            // `all!` in its parents.
            self.a.global_get(crate::rt::wake_regs());
            self.a.get(regs);
            self.a.s().i32_eq();
            self.a.if_();
            self.a.get(wn);
            self.a.i32(i32::try_from(j).expect("slot"));
            self.a.call(Sym::Helper(Helper::WakeUp));
            self.a.end();
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
        self.open(Ctl::Plain);
        // A cancelled child cancels this task: the cancellation branch
        // cancels every unfinished child, in argument order, before it
        // waits for any (`req.combinator.cancel-order`).
        if self.cancel_to.is_some() {
            self.a.get(gone);
            self.a.if_();
            self.open(Ctl::Plain);
            self.cancel_here(None)?;
            self.ctrl.pop();
            self.a.end();
        }
        // Pending with every unfinished child waiting on a wake: to its
        // parent, this `all!` waits on a registered wake too, even when
        // this poll polled none of those children.
        self.a.get(wn);
        self.a.struct_get(&waker, 0);
        self.a.get(done);
        self.a.i32(-1);
        self.a.s().i32_xor().i32_and();
        self.a.i32(all);
        self.a.s().i32_and().i32_eqz();
        self.a.if_();
        self.a.global_get(crate::rt::wake_regs());
        self.a.i32(1);
        self.a.s().i32_add();
        self.a.global_set(crate::rt::wake_regs());
        self.a.end();
        self.suspend(k, none)?;
        self.ctrl.pop();
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

/// How a scenario hold stubs a suspending call's poll (issue #56): the
/// stub answers pending without running the body, so the driver owns an
/// unfinished root. `Active` additionally leaves the frame flagged
/// active, so the next poll reports `suspension-reentrant-poll` through
/// the real flag check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldStub {
    /// Answer pending with the flags untouched.
    Pending,
    /// Answer pending with the frame flagged active.
    Active,
}

/// `Emit(inst)`: walks the generic TIR of sub-body `sub` under the
/// instance's arguments. A suspending function's instance (`crates/hd_wasm/DESIGN.md`
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
    held: Option<HoldStub>,
) -> StageResult<Code> {
    emit_body(
        pool, env, path, layouts, b, sub, args, ret, calls, key, None, held,
    )
}

/// The erased body of a generic method a vtable slot holds (`crates/hd_mono/DESIGN.md`
/// §13.5.1): the item's body with its own type parameters open, its
/// witness after its parameters, its open instructions calling the
/// thunks of the witness field of the body's family.
pub fn emit_erased(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    layouts: &Layouts,
    b: &Body,
    args: TyList,
    ret: Ty,
    calls: &HashMap<u32, Target>,
    key: Hash128,
) -> StageResult<Code> {
    let Some(Target::Erased { family }) = calls.get(&INFO) else {
        return unsupported("an erased body that collection did not record");
    };
    emit_body(
        pool,
        env,
        path,
        layouts,
        b,
        0,
        args,
        ret,
        calls,
        key,
        Some(*family),
        None,
    )
}

/// The values a captured local crosses with, at `args`: its components,
/// or one shared cell of them, matching closure emission.
fn captured_local_vts(
    lay: &Lay,
    b: &Body,
    args: TyList,
    local: u32,
    shared: bool,
) -> StageResult<Vec<VT>> {
    let t = subst(lay.pool, lay.env, b.item, args, b.local_ty[local as usize]);
    let fields = lay.vts(t)?;
    if !shared {
        return Ok(fields);
    }
    let fields = fields
        .into_iter()
        .map(|v| match v {
            VT::Ref(t, _) => VT::Ref(t, true),
            v => v,
        })
        .collect();
    Ok(vec![VT::r(WTy::Struct {
        fields,
        sup: None,
        open: false,
    })])
}

/// The type of thunk `at` of the erased body of `item` at `erased`
/// (`crates/hd_mono/DESIGN.md` §13.5.1): its operands as the erased body holds them,
/// open ones as `eqref`, then the body's providers; its result likewise.
fn thunk_sig(lay: &Lay, b: &Body, erased: TyList, at: u32) -> StageResult<WTy> {
    let (pool, env) = (lay.pool, lay.env);
    let s = |t: Ty| subst(pool, env, b.item, erased, t);
    let mut ps = Vec::new();
    for r in thunk_operands(b, at) {
        ps.extend(lay.vts(s(b.ty[r as usize]))?);
    }
    for (local, shared) in thunk_capture_locals(b, at) {
        ps.extend(captured_local_vts(lay, b, erased, local, shared)?);
    }
    for k in env.row_keys(b.item, erased) {
        ps.push(VT::Eq);
        ps.push(VT::r(lay.key_vtable(k)?));
    }
    if has_row_params(env, b.item) {
        ps.push(VT::rn(ctx_keys()));
        ps.push(VT::rn(ctx_provs()));
    }
    // A tag switch's thunk is its scrutinee's tag read.
    if b.tags[at as usize] == Tag::SwitchTag {
        return Ok(WTy::Func(ps, vec![VT::I32]));
    }
    Ok(WTy::Func(ps, lay.vts(s(b.ty[at as usize]))?))
}

/// Thunk `k` of an erased body at concrete method type arguments
/// (`crates/hd_mono/DESIGN.md` §13.5.1): the one open instruction collection recorded,
/// emitted at the caller's types, its open operands unboxed or cast on
/// the way in and its open result boxed or upcast on the way out.
pub fn emit_thunk(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    layouts: &Layouts,
    b: &Body,
    args: TyList,
    calls: &HashMap<u32, Target>,
    key: Hash128,
) -> StageResult<Code> {
    let Some(&Target::Thunk { at }) = calls.get(&INFO) else {
        return unsupported("a thunk that collection did not record");
    };
    let lay = Lay::new(pool, env, path, layouts);
    let erased = erased_args(pool, env, b.item, args);
    let WTy::Func(ps, rs) = thunk_sig(&lay, b, erased, at)? else {
        return unsupported("a thunk type");
    };
    let mut em = Em::new(lay, b, args, calls, key, Asm::new(ps), rs.clone());
    let mut next = 0;
    for r in thunk_operands(b, at) {
        let crossing = em
            .lay
            .vts(subst(pool, env, b.item, erased, b.ty[r as usize]))?;
        let have = em.vts(em.ty_of(r))?;
        let params: Vec<u32> = (next..next + u32_of(crossing.len())).collect();
        next += u32_of(crossing.len());
        if crossing == have {
            em.vals.insert(r, (params, have));
            continue;
        }
        let ls: Vec<u32> = have.iter().map(|v| em.a.local(v.clone())).collect();
        unerase(&mut em.a, params[0], &have);
        for l in ls.iter().rev() {
            em.a.set(*l);
        }
        em.vals.insert(r, (ls, have));
    }
    for (local, shared) in thunk_capture_locals(b, at) {
        let crossing = captured_local_vts(&em.lay, b, erased, local, shared)?;
        let have = captured_local_vts(&em.lay, b, args, local, shared)?;
        let params: Vec<u32> = (next..next + u32_of(crossing.len())).collect();
        next += u32_of(crossing.len());
        if crossing == have {
            em.locals[local as usize] = Some(params);
            continue;
        }
        let ls: Vec<u32> = have.iter().map(|v| em.a.local(v.clone())).collect();
        unerase(&mut em.a, params[0], &have);
        for l in ls.iter().rev() {
            em.a.set(*l);
        }
        em.locals[local as usize] = Some(ls);
    }
    // The erased body's own providers, at concrete types; the keys of its
    // row parameters' arguments are read from the context.
    for k in declared_row_keys(pool, env, b.item, args) {
        em.providers.push((k, [next, next + 1]));
        next += 2;
    }
    if has_row_params(env, b.item) {
        em.set_open_ctx([next, next + 1]);
    }
    em.ctrl.push(Ctl::Plain);
    if b.tags[at as usize] == Tag::SwitchTag {
        em.tag_of(b.data[at as usize][0])?;
        return Ok(em.a.finish(rs));
    }
    em.inst(at)?;
    let have = em.vts(em.sub(b.ty[at as usize]))?;
    if have == rs {
        em.load_as(at, &rs)?;
    } else {
        em.erase(at, &have)?;
    }
    Ok(em.a.finish(rs))
}

/// An item or closure body; `erased` is the witness family of an erased
/// body (`crates/hd_mono/DESIGN.md` §13.5.1).
fn emit_body(
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
    erased: Option<Hash128>,
    held: Option<HoldStub>,
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
        return emit_suspending(&lay, b, sub, args, ret, calls, key, erased, held);
    }
    let (mut params, results, plocals) = signature(&lay, b, sub, args, ret)?;
    let row_ctx = sub == 0 && has_row_params(env, b.item);
    if erased.is_some() {
        let at = params.len() - 2 * env.row_keys(b.item, args).len();
        params.insert(at, VT::r(witness_ty()));
    }
    // An item's own row parameters, in any instance of it: their keys'
    // context, last (`crates/hd_mono/DESIGN.md` §12.4).
    if row_ctx {
        params.push(VT::rn(ctx_keys()));
        params.push(VT::rn(ctx_provs()));
    }
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
        let witness = next;
        if erased.is_some() {
            next += 1;
        }
        for k in env.row_keys(b.item, args) {
            em.providers.push((k, [next, next + 1]));
            next += 2;
        }
        if row_ctx {
            em.set_open_ctx([next, next + 1]);
        }
        if let Some(family) = erased {
            em.read_ops(witness, family)?;
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
            let vt = em.lay.key_vtable(k)?;
            let ls = ctx_provider(&mut em.a, pool, env, k, &vt, keys_at, provs_at);
            em.providers.push((k, ls));
        }
        // A row that lists the open row: the context also holds its keys.
        if em.fn_row(s(b.ty[ci])).is_some_and(|r| row_is_open(pool, r)) {
            em.set_open_ctx([keys_at, provs_at]);
        }
    }
    // A facts body and a structure handle's body build their value once
    // per instance (`annot.fact.eval.lazy`, `annot.handle.constants`).
    let fact = match b.kind {
        BodyKind::Fact => Some(Some(&*b.label)),
        BodyKind::Handle => Some(None),
        _ => None,
    };
    let once = fact
        .filter(|_| sub == 0 && erased.is_none())
        .map(|fact| OnceCell {
            key,
            results: results.clone(),
            fact,
        });
    let entered = once.as_ref().map(|cell| cell.enter(&mut em.a));
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
        if let (Some(cell), Some(fact)) = (once, entered) {
            cell.leave(&mut em.a, fact);
        }
    }
    Ok(em.a.finish(results))
}

/// The storage of a body whose value is built once per instance: a
/// global per Wasm value of the result, and a flag that says the body has
/// run. The body runs on its first call; every later call returns the
/// stored value. That is a facts body (`annot.fact.eval.lazy`), whose
/// `fact` labels its panics, or a structure handle's body
/// (`annot.handle.constants`), whose `fact` is `None`.
struct OnceCell<'b> {
    key: Hash128,
    results: Vec<VT>,
    fact: Option<&'b str>,
}

impl OnceCell<'_> {
    /// The global holding result `k`, or the flag past the last one.
    fn global(&self, k: usize) -> GSym {
        match self.results.get(k) {
            Some(v) => GSym::Binding(self.key, u32_of(k), v.dflt()),
            None => GSym::Binding(self.key, u32_of(k), VT::I32),
        }
    }

    /// The start of the body: a call after the first returns the stored
    /// value. Otherwise, for a facts body, the host's panic reports name
    /// its label until the body ends (`annot.fact.eval.panic-runtime`),
    /// and the result is `true`.
    fn enter(&self, a: &mut Asm) -> bool {
        let flag = self.global(self.results.len());
        a.global_get(flag.clone());
        if self.fact.is_some() {
            // The flag is 0 before the body, 2 while it runs, 1 after.
            a.i32(1);
            a.s().i32_eq();
        }
        a.if_();
        for (k, v) in self.results.iter().enumerate() {
            a.global_get(self.global(k));
            a.conv(&v.dflt(), v);
        }
        a.s().return_();
        a.end();
        let Some(label) = self.fact else {
            return false;
        };
        crate::rt::write_lit_to(a, label, crate::rt::fact_import());
        // A call while the body runs reads the fact from its own
        // evaluation (`annot.fact.eval.reentrant`); the report names it.
        a.global_get(flag.clone());
        a.if_();
        a.call(Sym::Helper(Helper::Panic(
            "reentrant-evaluation: the fact reads itself".into(),
        )));
        a.s().unreachable();
        a.end();
        a.i32(2);
        a.global_set(flag);
        // A facts body is a forbidden context (`req.drive.block-on.fact-contexts`).
        crate::rt::forbid_by(a, 1);
        true
    }

    /// The end of the body, with its result on the stack: stores it, and
    /// for a facts body (`fact`) tells the host the body has ended, so
    /// its reports name the facts being evaluated around this one, if any.
    fn leave(&self, a: &mut Asm, fact: bool) {
        let held: Vec<u32> = self.results.iter().map(|v| a.local(v.clone())).collect();
        for l in held.iter().rev() {
            a.set(*l);
        }
        for (k, l) in held.iter().enumerate() {
            a.get(*l);
            a.global_set(self.global(k));
        }
        a.i32(1);
        a.global_set(self.global(self.results.len()));
        if fact {
            a.i32(-1);
            a.call(crate::rt::fact_import());
            crate::rt::forbid_by(a, -1);
        }
        for l in &held {
            a.get(*l);
        }
    }
}

/// The provider of key `k` from a closure call's context (the parameters
/// `keys_at` and `provs_at`, `crates/hd_mono/DESIGN.md` §12.4): two fresh locals holding
/// its value and its vtable. The caller's row covers `k`, so the search
/// ends at its slot.
fn ctx_provider(
    a: &mut Asm,
    pool: &InternPool,
    env: &dyn ProgramEnv,
    k: Ty,
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
    a.i64(key_id(pool, env, k));
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

/// A function reference's adapter instance (`crates/hd_mono/DESIGN.md` §13.11): closure
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
    let t = match calls.get(&0) {
        Some(Target::Call(t)) => t,
        Some(Target::Dyn) => return emit_dyn_adapter(&lay, item, args),
        _ => return unsupported("a function reference that collection did not resolve"),
    };
    if t.kind != TargetKind::Instance {
        let callee = Callee::Item {
            def: item,
            targs: args,
        };
        let keys = env.row_keys(item, args);
        return emit_call_adapter(&lay, item, args, Target::Call(t.clone()), &callee, &keys);
    }
    let (cps, crs) = adapter_code(&lay, item, args)?;
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
    for k in declared_row_keys(pool, env, t.item, t.args) {
        let vt = lay.key_vtable(k)?;
        provs.push(ctx_provider(&mut a, pool, env, k, &vt, keys_at, provs_at));
    }
    for (j, w) in want.iter().enumerate() {
        a.get(u32_of(j + 1));
        a.conv(&cps[j + 1], w);
    }
    for [pl, vl] in provs {
        a.get(pl);
        a.get(vl);
    }
    // The item's own row parameters read their keys from the context the
    // value's caller passed, which holds the value's row: it passes on.
    if has_row_params(env, t.item) {
        a.raw_get(keys_at);
        a.raw_get(provs_at);
    }
    a.call(Sym::Inst(t.key));
    convert_results(&mut a, &got, &crs);
    Ok(a.finish(crs))
}

/// A function reference's adapter to a target the compiler lowers at each
/// call: an intrinsic, a built-in family's method, or a trait method at a
/// trait-value `Self`. The adapter is the call itself, as a body of one
/// `Call` whose operands are the parameters, so the reference lowers
/// exactly as a direct call at the same types does (`crates/hd_mono/DESIGN.md` §13.11).
/// The closure's parameters are the call's operands, receiver first;
/// `keys` are the row keys the call's providers come from, which the
/// adapter takes from the context its caller passed.
fn emit_call_adapter(
    lay: &Lay,
    item: DefId,
    args: TyList,
    target: Target,
    callee: &Callee,
    keys: &[Ty],
) -> StageResult<Code> {
    let (pool, env) = (lay.pool, lay.env);
    let (cps, crs) = adapter_code(lay, item, args)?;
    let ptys = env.params(item).unwrap_or_default();
    let n = u32_of(ptys.len());
    let mut b = Body::new(item, BodyKind::Fn);
    // One placeholder instruction per parameter, then the call.
    for p in &ptys {
        b.tags.push(Tag::Poison);
        b.data.push([NONE, NONE]);
        b.ty.push(*p);
        b.syn.push(NodeIdx::NONE);
    }
    let callee = callee.words();
    let callee_at = u32_of(b.extra.len());
    b.extra.push(u32_of(callee.len()));
    b.extra.extend(callee);
    // The operands, then the three words of a call's providers.
    let args_at = u32_of(b.extra.len());
    b.extra.push(n + 3);
    b.extra.extend(0..n);
    b.extra.extend([0, 0, 0]);
    let ret = env.ret(item).unwrap_or(Ty::VOID);
    b.tags.push(Tag::Call);
    b.data.push([callee_at, args_at]);
    b.ty.push(ret);
    b.syn.push(NodeIdx::NONE);
    let calls = HashMap::from_iter([(n, target)]);
    let mut em = Em::new(
        Lay::new(pool, env, lay.path, lay.shared),
        &b,
        args,
        &calls,
        Hash128(0),
        Asm::new(cps.clone()),
        crs.clone(),
    );
    let mut next = 1;
    for (j, p) in ptys.iter().enumerate() {
        let vts = em.vts(em.sub(*p))?;
        let ls: Vec<u32> = (next..next + u32_of(vts.len())).collect();
        next += u32_of(vts.len());
        em.vals.insert(u32_of(j), (ls, vts));
    }
    if next + 2 != u32_of(cps.len()) {
        return unsupported("a function reference whose call has another shape");
    }
    let (keys_at, provs_at) = (next, next + 1);
    for k in keys {
        let vt = em.lay.key_vtable(*k)?;
        let ls = ctx_provider(&mut em.a, pool, env, *k, &vt, keys_at, provs_at);
        em.providers.push((*k, ls));
    }
    em.ctrl.push(Ctl::Plain);
    // The call's result is the closure's: a suspending item's call is its
    // cold constructor, whose frame the closure returns
    // (`fn.ref.suspending`), not a value of the declared result type.
    let ls: Vec<u32> = crs.iter().map(|v| em.a.local(v.clone())).collect();
    em.vals.insert(n, (ls, crs.clone()));
    em.call(n, callee_at, args_at, em.sub(ret))?;
    em.load_as(n, &crs)?;
    Ok(em.a.finish(crs))
}

/// The closure code type of a function reference to `item` at `args`:
/// its parameters `(env, params..., keys, providers)` and results.
fn adapter_code(lay: &Lay, item: DefId, args: TyList) -> StageResult<(Vec<VT>, Vec<VT>)> {
    let (pool, env) = (lay.pool, lay.env);
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
        inputs: hd_types::Inputs::Fixed,
    });
    let Shape::Fn { code, .. } = lay.shape(ft)? else {
        return unsupported("a function reference without a function type");
    };
    let WTy::Func(cps, crs) = code else {
        return unsupported("a closure code type");
    };
    Ok((cps, crs))
}

/// Converts the results `got` on the stack to the closure results `crs`.
fn convert_results(a: &mut Asm, got: &[VT], crs: &[VT]) {
    let tmp: Vec<u32> = got.iter().map(|v| a.local(v.clone())).collect();
    for l in tmp.iter().rev() {
        a.set(*l);
    }
    for (l, (have, w)) in tmp.iter().zip(got.iter().zip(crs)) {
        a.get(*l);
        a.conv(have, w);
    }
}

/// A function reference to a trait method at a trait-value `Self`, as
/// `T::name` at `T = dyn Named` (trait.dyn.bound.dispatch): the call
/// goes through the method's slot in the receiver's vtable.
fn emit_dyn_adapter(lay: &Lay, item: DefId, args: TyList) -> StageResult<Code> {
    let (pool, env) = (lay.pool, lay.env);
    let Some((trait_, _)) = env.parent(item) else {
        return unsupported("a dynamic function reference to a method outside a trait");
    };
    let all = pool.list_items(args);
    let Some((&recv, targs)) = all.split_first() else {
        return unsupported("a trait member reference without `Self`");
    };
    let slot = lay.dyn_slot(recv, trait_, targs, item)?;
    let callee = Callee::TraitMethod {
        trait_,
        method: item,
        self_ty: recv,
        targs: pool.list(targs),
        choice: (ChoiceKind::TraitValue, 0),
    };
    emit_call_adapter(lay, item, args, Target::Dyn, &callee, &slot.keys)
}

/// What a suspending closure's cold constructor binds besides its
/// parameters: the environment struct with each captured value's field
/// and body local, and each row key's provider from the call's context
/// with its vtable type and two body locals.
struct ClosureBind {
    env: WTy,
    caps: Vec<(u32, u32)>,
    provs: Vec<(Ty, WTy, [u32; 2])>,
    /// The two body locals of the call's context, when the closure's row
    /// lists the open row.
    ctx: Option<[u32; 2]>,
}

/// One emission pass of a suspending body: its code, every Wasm local it
/// saves with its type, and a closure's bindings.
type Pass = (Code, Vec<(u32, VT)>, Option<ClosureBind>);

/// A suspending function or closure body (`crates/hd_wasm/DESIGN.md` §14.1 to §14.6).
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
    erased: Option<Hash128>,
    held: Option<HoldStub>,
) -> StageResult<Code> {
    let (pool, env) = (lay.pool, lay.env);
    let s = |t: Ty| subst(pool, env, b.item, args, t);
    let (mut cold_params, _, plocals) = signature(lay, b, sub, args, ret)?;
    let row_ctx = sub == 0 && has_row_params(env, b.item);
    if erased.is_some() {
        let at = cold_params.len() - 2 * env.row_keys(b.item, args).len();
        cold_params.insert(at, VT::r(witness_ty()));
    }
    // An item's own row parameters: their keys' context, last, saved in
    // the frame as a parameter is.
    if row_ctx {
        cold_params.push(VT::rn(ctx_keys()));
        cold_params.push(VT::rn(ctx_provs()));
    }
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
        let witness = next;
        if erased.is_some() {
            next += 1;
        }
        let bind = if let Some(ci) = closure {
            // Locals for the captured values and the row's providers,
            // which the cold constructor fills through the frame.
            let (env_ty, caps) = em.closure_env(ci)?;
            let mut provs = Vec::new();
            for k in em.fn_row_keys(s(b.ty[ci])) {
                let vt = em.lay.key_vtable(k)?;
                let ls = [em.a.local(VT::Eq), em.a.local(VT::r(vt.clone()))];
                em.providers.push((k, ls));
                provs.push((k, vt, ls));
            }
            // A row that lists the open row keeps the call's context,
            // bound at construction as the providers are.
            let ctx = if em.fn_row(s(b.ty[ci])).is_some_and(|r| row_is_open(pool, r)) {
                let ls = [
                    em.a.local(VT::rn(ctx_keys())),
                    em.a.local(VT::rn(ctx_provs())),
                ];
                em.set_open_ctx(ls);
                Some(ls)
            } else {
                None
            };
            Some(ClosureBind {
                env: env_ty,
                caps,
                provs,
                ctx,
            })
        } else {
            for k in env.row_keys(b.item, args) {
                em.providers.push((k, [next, next + 1]));
                next += 2;
            }
            if row_ctx {
                em.set_open_ctx([next, next + 1]);
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
        // An erased body reads its thunks after the frame restored the
        // witness parameter, so a resumed frame has them too.
        if let Some(family) = erased {
            em.read_ops(witness, family)?;
        }
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
    // The first pass, with no save or reload, learns the body's locals;
    // those live at its entry are the ones a resume reads, so only they
    // get frame fields (`crate::live`). The second pass saves them.
    let (first, all, _) = pass(Vec::new())?;
    let saved: Vec<(u32, VT)> = match crate::live::live_at_entry(&first.body, u32_of(logical.len()))
    {
        Some(live) => all
            .iter()
            .filter(|(l, _)| live.get(*l as usize) == Some(&true))
            .cloned()
            .collect(),
        None => all.clone(),
    };
    let (body, again, bind) = pass(saved.clone())?;
    if again != all {
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
    // Cold parameter `first + j` is the body's local `j + 1`. A local the
    // body never reads before writing has no field, and takes nothing.
    let first = u32::from(bind.is_some());
    for j in 0..u32_of(own.len()) {
        if let Some(fi) = field(j + 1) {
            a.get(f);
            a.get(first + j);
            a.struct_set(&ft, fi);
        }
    }
    if let Some(bind) = bind {
        let e = a.local(VT::r(bind.env.clone()));
        a.get(0);
        a.ref_cast(&bind.env, false);
        a.set(e);
        for (fi, l) in bind.caps {
            if let Some(to) = field(l) {
                a.get(f);
                a.get(e);
                a.struct_get(&bind.env, fi);
                a.struct_set(&ft, to);
            }
        }
        let (keys_at, provs_at) = (u32_of(cold_params.len() - 2), u32_of(cold_params.len() - 1));
        for (k, vt, ls) in bind.provs {
            let got = ctx_provider(&mut a, pool, env, k, &vt, keys_at, provs_at);
            for (g, l) in got.into_iter().zip(ls) {
                if let Some(to) = field(l) {
                    a.get(f);
                    a.get(g);
                    a.struct_set(&ft, to);
                }
            }
        }
        if let Some(ls) = bind.ctx {
            for (g, l) in [keys_at, provs_at].into_iter().zip(ls) {
                if let Some(to) = field(l) {
                    a.get(f);
                    a.raw_get(g);
                    a.struct_set(&ft, to);
                }
            }
        }
    }
    a.get(f);
    let cold = a.finish(vec![VT::r(base.clone())]);
    // f$poll: the runtime checks, then the body over the frame. A held
    // call answers every poll with pending, without running its body; an
    // active hold leaves the frame flagged active, so the next poll
    // reports re-entry through the real flag check.
    let poll_code = if let Some(kind) = held {
        let mut a = Asm::new(vec![VT::Eq]);
        if kind == HoldStub::Active {
            let f = a.local(VT::r(ft.clone()));
            a.get(0);
            a.ref_cast(&ft, false);
            a.set(f);
            a.get(f);
            a.get(f);
            a.struct_get(&ft, F_FLAGS);
            a.i32(ACTIVE);
            a.s().i32_or();
            a.struct_set(&ft, F_FLAGS);
        }
        a.i32(0);
        for v in &dres {
            a.zero(v);
        }
        let mut pres = vec![VT::I32];
        pres.extend(dres.iter().cloned());
        a.finish(pres)
    } else {
        let mut a = Asm::new(vec![VT::Eq]);
        let f = a.local(VT::r(ft.clone()));
        let tmp: Vec<u32> = dres.iter().map(|v| a.local(v.clone())).collect();
        let nf = a.local(VT::rn(base.clone()));
        a.get(0);
        a.ref_cast(&ft, false);
        a.set(f);
        // The runtime checks (`rt::Helper::PollCheck`); a failed
        // dependency of a dispose answers Pending.
        a.get(f);
        a.call(Sym::Helper(Helper::PollCheck));
        a.if_();
        a.i32(0);
        for v in &dres {
            a.zero(v);
        }
        a.s().return_();
        a.end();
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
        a.finish(pres)
    };
    // f$cancel (§14.6): a frame that never waited only gets marked. A
    // started frame is `UNWINDING` until its body leaves by `Cancel`,
    // which clears the flag; a frame that still unwinds (its shielded
    // `dispose!` waits) continues. Under `abandon()` the frame is marked
    // `ABANDONED`, and the `ABANDONED` bits of the cancels it makes carry
    // up into it (`req.abandon.carry`, `crate::rt::abandon_seen_global`).
    let mut a = Asm::new(vec![VT::Eq]);
    let f = a.local(VT::r(ft.clone()));
    let back = a.local(VT::rn(base.clone()));
    let seen = a.local(VT::I32);
    a.get(0);
    a.ref_cast(&ft, false);
    a.set(f);
    a.call(Sym::Helper(Helper::CancelEnter));
    a.set(seen);
    a.block();
    a.get(f);
    a.call(Sym::Helper(Helper::CancelCheck));
    a.s().i32_eqz();
    a.br_if(0);
    a.get(f);
    a.struct_get(&ft, F_STATE);
    a.s().i32_eqz();
    a.br_if(0);
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(UNWINDING);
    a.s().i32_or();
    a.struct_set(&ft, F_FLAGS);
    a.get(f);
    for v in &logical[1..] {
        a.zero(&v.dflt());
    }
    a.call(Sym::Part(key, Part::Body));
    a.set(back);
    for _ in 1..bres.len() {
        a.s().drop();
    }
    // A body that completed after all leaves nothing to unwind.
    a.raw_get(back);
    a.s().ref_is_null();
    a.if_();
    a.get(f);
    a.get(f);
    a.struct_get(&ft, F_FLAGS);
    a.i32(!UNWINDING);
    a.s().i32_and();
    a.struct_set(&ft, F_FLAGS);
    a.end();
    a.end();
    a.get(f);
    a.get(seen);
    a.call(Sym::Helper(Helper::CancelLeave));
    let cancel = a.finish(vec![]);
    // A held body never runs: a direct caller passes a null frame on the
    // fresh path, so build the frame through the cold constructor and
    // hand it back as the pending child; on resume hand the same frame
    // back again. Stored and root calls reach the held poll stub above.
    let body = if held.is_some() && sub == 0 {
        let mut a = Asm::new_dflt(logical.clone());
        let f = a.local(VT::rn(ft.clone()));
        a.get(0);
        a.s().ref_is_null();
        a.if_();
        for j in 1..u32_of(logical.len()) {
            a.get(j);
        }
        a.call(Sym::Inst(key));
        a.ref_cast(&ft, false);
        a.set(f);
        a.else_();
        a.get(0);
        a.ref_cast(&ft, false);
        a.set(f);
        a.end();
        for v in &dres {
            a.zero(v);
        }
        a.raw_get(f);
        a.finish(bres.clone())
    } else {
        body
    };
    let mut code = cold;
    code.parts = vec![
        (Part::Body, body),
        (Part::Poll, poll_code),
        (Part::Cancel, cancel),
    ];
    Ok(code)
}

/// Adds `by` to the `abandon()` depth (`crate::rt::abandoning_global`).
fn abandon_by(a: &mut Asm, by: i32) {
    a.global_get(crate::rt::abandoning_global());
    a.i32(by);
    a.s().i32_add();
    a.global_set(crate::rt::abandoning_global());
}

/// The entry exports (`crates/hd_mono/DESIGN.md` §13.1, `crates/hd_wasm/DESIGN.md` §14.4): `hd.init`
/// runs every reachable group's init in order; `hd.poll` provides one
/// default-profile provider per row key, each a vtable of host stubs
/// generated from `hd_host_abi::TABLE` (`crates/hd_run_wasmtime/DESIGN.md` §17.1), and
/// runs `main` or polls `main!`, then turns its result into the exit
/// status with `report` (a `std.rt` instance; `None` for `void`);
/// `hd.wake(n)` records completed handles.
pub fn entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    lowering: Lowering,
    main: DefId,
    key: hd_base::Hash128,
    inits: &[hd_base::Hash128],
    entry_inits: &[hd_base::Hash128],
    report: Option<hd_base::Hash128>,
    boundary: &hd_mono::BoundaryMap,
) -> StageResult<Vec<(String, Helper)>> {
    // Only the entry module's own init bodies may use `block_on`
    // (`req.drive.block-on.forbidden-contexts`).
    let inits = inits
        .iter()
        .map(|k| (*k, !entry_inits.contains(k)))
        .collect();
    Ok(vec![
        ("hd.init".into(), Helper::EntryInit { inits }),
        (
            "hd.poll".into(),
            root_poll(
                pool,
                env,
                path,
                lowering,
                main,
                key,
                report,
                Vec::new(),
                boundary,
            )?,
        ),
        ("hd.wake".into(), Helper::EntryWake),
    ])
}

/// The entry exports of a script (module.init.script): `hd.init` runs the
/// reachable groups' inits before the script's first own init body `key`.
/// `hd.poll` runs that body with its entry providers, then the inits after
/// it in order: the script's own bodies `own` (its runs, when its module
/// cycle's statements split it) with the entry providers, the others as
/// forbidden contexts. It then exits with 0 as `main` with no result.
pub fn script_entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    lowering: Lowering,
    init: DefId,
    key: hd_base::Hash128,
    inits: &[hd_base::Hash128],
    own: &[hd_base::Hash128],
    report: Option<hd_base::Hash128>,
    boundary: &hd_mono::BoundaryMap,
) -> StageResult<Vec<(String, Helper)>> {
    let (before, after): (Vec<_>, Vec<_>) = match inits.iter().position(|k| *k == key) {
        Some(at) => (inits[..at].to_vec(), inits[at + 1..].to_vec()),
        // An init this build does not run, as an earlier REPL input's.
        None => (
            inits.iter().copied().filter(|k| !own.contains(k)).collect(),
            own.iter().copied().filter(|k| *k != key).collect(),
        ),
    };
    // A `?` at the top level returns from the whole init, which a module
    // cycle that splits the statements into runs cannot honor.
    if report.is_some() && after.iter().any(|k| own.contains(k)) {
        return unsupported("a script using `?` whose module cycle runs after its first statement");
    }
    let before = before.into_iter().map(|k| (k, true)).collect();
    let after = after.into_iter().map(|k| (k, !own.contains(&k))).collect();
    Ok(vec![
        ("hd.init".into(), Helper::EntryInit { inits: before }),
        (
            "hd.poll".into(),
            root_poll(
                pool, env, path, lowering, init, key, report, after, boundary,
            )?,
        ),
        ("hd.wake".into(), Helper::EntryWake),
    ])
}

/// The exports of a test program (`docs/compiler/engines-and-test-runner.md` §19.1):
/// `hd.init.j`, the init groups that test module `j` reaches; `hd.test.i`,
/// which polls case `i` as `hd.poll` polls `main`; and `hd.wake`.
pub fn test_entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    lowering: Lowering,
    cases: &[(DefId, hd_base::Hash128, Option<hd_base::Hash128>)],
    inits: &[Vec<hd_base::Hash128>],
    boundary: &hd_mono::BoundaryMap,
) -> StageResult<Vec<(String, Helper)>> {
    let mut out = Vec::new();
    for (j, ks) in inits.iter().enumerate() {
        out.push((
            format!("hd.init.{j}"),
            // The module under test is not an entry module
            // (`module.init.tests.no-entry`), so every init is forbidden.
            Helper::EntryInit {
                inits: ks.iter().map(|k| (*k, true)).collect(),
            },
        ));
    }
    for (i, (def, key, report)) in cases.iter().enumerate() {
        out.push((
            format!("hd.test.{i}"),
            root_poll(
                pool,
                env,
                path,
                lowering,
                *def,
                *key,
                *report,
                Vec::new(),
                boundary,
            )?,
        ));
    }
    out.push(("hd.wake".into(), Helper::EntryWake));
    Ok(out)
}

/// The exports of a scenario program: like [`entry`], whose suspending
/// root `link` gives `hd.cancel` and `hd.abandon`, plus, when the entry
/// module declares it, `hd.scenario.cleanup`, which calls the top-level
/// cleanup function `cleanup` names. Only a `Goal::Scenario` build emits
/// it, so normal builds never carry it.
pub fn scenario_entry(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    lowering: Lowering,
    main: DefId,
    key: hd_base::Hash128,
    inits: &[hd_base::Hash128],
    entry_inits: &[hd_base::Hash128],
    report: Option<hd_base::Hash128>,
    cleanup: Option<hd_base::Hash128>,
    boundary: &hd_mono::BoundaryMap,
) -> StageResult<Vec<(String, Helper)>> {
    let mut out = entry(
        pool,
        env,
        path,
        lowering,
        main,
        key,
        inits,
        entry_inits,
        report,
        boundary,
    )?;
    if let Some(func) = cleanup {
        out.push(("hd.scenario.cleanup".into(), Helper::EntryCall { func }));
    }
    Ok(out)
}

/// The poll export of one root: its row's default providers, then the
/// root's call or poll, then the inits `then` that follow a script's first
/// own body (flagged when forbidden, else run with the same providers),
/// then its report.
fn root_poll(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    path: &dyn Fn(DefId) -> String,
    lowering: Lowering,
    main: DefId,
    key: hd_base::Hash128,
    report: Option<hd_base::Hash128>,
    then: Vec<(hd_base::Hash128, bool)>,
    boundary: &hd_mono::BoundaryMap,
) -> StageResult<Helper> {
    let shared = Layouts::new(lowering);
    let lay = Lay::new(pool, env, path, &shared);
    let mut providers = Vec::new();
    for key in env.row_keys(main, TyList::EMPTY) {
        // A host capability is never generic (`req.use.entry-row`), so the
        // host table is keyed by the trait alone (`crates/hd_mono/DESIGN.md` §13.18).
        let k = match pool.get(key) {
            TyData::TraitValue {
                def,
                args,
                bindings,
            } if args == TyList::EMPTY && bindings.is_empty() => def,
            _ => {
                return unsupported(format!("a default provider for `{}`", pool.display(key)));
            }
        };
        let p = path(k);
        let host = hd_host_abi::host_trait(&p.replace('/', "."));
        let tk = lay.trait_key(k, TyList::EMPTY);
        if !lay.dyn_supers(tk).is_empty() {
            return unsupported(format!(
                "a default provider for `{p}`, which has supertraits"
            ));
        }
        let vt = lay.vtable(tk)?;
        let mut slots = Vec::new();
        for m in env.trait_methods(k) {
            let sig = lay.slot_sig(tk, m)?;
            let mp = path(m);
            let name = mp.rsplit(['.', '/']).next().unwrap_or("");
            // A trait the ABI table does not list is a runtime profile's
            // own, whose row comes from its declaration.
            let Some(host) = host else {
                let key = p.rsplit(['.', '/']).next().unwrap_or("");
                let at = crate::boundary::AtProfile {
                    codecs: boundary,
                    out: false,
                    handles: &[],
                };
                slots.push(profile_slot(&lay, key, name, m, sig, false, at)?);
                continue;
            };
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
            slots.push(host_slot(
                &lay,
                host.key,
                &HostRow::of(hm),
                m,
                sig,
                false,
                None,
            )?);
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
    // A script's top level is a suspension driver
    // (`module.init.script-driver`), which one poll export runs; a module
    // cycle that splits the statements into runs has no single poll.
    if bang.is_some() && !then.is_empty() {
        return unsupported(
            "a suspending script whose module cycle runs after its first statement",
        );
    }
    Ok(Helper::EntryPoll {
        main: key,
        providers,
        results: u32_of(results.len()),
        bang,
        report: report.map(|k| (k, results)),
        then,
    })
}

/// The vtable slots of a host handle trait, in the trait's method order:
/// each reads the handle number from its receiver and calls the host
/// method with it first (`boundary::BTy::Handle`). A trait in
/// `hd_host_abi::HANDLES` names its methods `<Trait>.<method>` there; a
/// runtime profile's own trait, at the profile's boundary `at`, is
/// imported from `hd:<Trait>` by each method's name (`profile_slot`).
/// Then the vtables of its direct supertraits, which have none of their
/// own, as `std.ops.Disposable`: each method a host method of the handle
/// trait, as `TcpStream.dispose` (flow.using.disposable).
pub(crate) fn handle_slots(
    lay: &Lay<'_>,
    trait_: DefId,
    host: Option<&hd_host_abi::HostTrait>,
    at: Option<crate::boundary::AtProfile<'_>>,
) -> StageResult<(Vec<Helper>, Vec<crate::boundary::HandleSuper>)> {
    let tp = (lay.path)(trait_);
    let tname = tp.rsplit(['.', '/']).next().unwrap_or("");
    let key = lay.trait_key(trait_, TyList::EMPTY);
    let supers = lay.dyn_supers(key);
    if !supers.is_empty() && host.is_none() {
        return unsupported(format!(
            "the host handle trait `{tp}`, which has supertraits"
        ));
    }
    let slots = handle_trait_slots(lay, key, trait_, tname, host, at)?;
    let mut tables = Vec::new();
    for s in supers {
        let TyData::TraitValue { def, .. } = lay.pool.get(s) else {
            return unsupported("a supertrait that is not a trait");
        };
        if !lay.dyn_supers(s).is_empty() {
            return unsupported(format!(
                "the host handle trait `{tp}`, whose supertrait has supertraits"
            ));
        }
        let vt = lay.vtable(s)?;
        tables.push((vt, handle_trait_slots(lay, s, def, tname, host, at)?));
    }
    Ok((slots, tables))
}

/// The slots of the vtable of `trait_`, at trait value key `key`, for a
/// host handle whose trait is named `tname` (`handle_slots`).
fn handle_trait_slots(
    lay: &Lay<'_>,
    key: Ty,
    trait_: DefId,
    tname: &str,
    host: Option<&hd_host_abi::HostTrait>,
    at: Option<crate::boundary::AtProfile<'_>>,
) -> StageResult<Vec<Helper>> {
    let tp = (lay.path)(trait_);
    let mut slots = Vec::new();
    for m in lay.env.trait_methods(trait_) {
        let sig = lay.slot_sig(key, m)?;
        let mp = (lay.path)(m);
        let method = mp.rsplit(['.', '/']).next().unwrap_or("");
        let WTy::Func(params, results) = &sig else {
            return unsupported("a handle slot without a signature");
        };
        // The inner call takes the receiver, then the handle number.
        let mut inner = params.clone();
        inner.insert(1, VT::I32);
        let inner = WTy::Func(inner, results.clone());
        let inner = match (host, at) {
            (Some(host), _) => {
                let name = format!("{tname}.{method}");
                let Some(hm) = host.methods.iter().find(|x| x.name == name) else {
                    return unsupported(format!("the host method `{name}`"));
                };
                host_slot(lay, host.key, &HostRow::of(hm), m, inner, true, None)?
            }
            (None, Some(at)) => profile_slot(lay, tname, method, m, inner, true, at)?,
            (None, None) => return unsupported(format!("the host handle trait `{tp}`")),
        };
        slots.push(Helper::HandleSlot {
            sig,
            payload: crate::boundary::handle_payload(),
            inner: Box::new(inner),
        });
    }
    Ok(slots)
}

/// One host method's row as `host_slot` lowers it: from the ABI table, or
/// derived from a runtime profile's own trait (`profile_slot`).
struct HostRow<'a> {
    name: &'a str,
    params: &'a [hd_host_abi::Codec],
    result: hd_host_abi::Codec,
    wait: hd_host_abi::Wait,
}

impl HostRow<'_> {
    fn of(hm: &hd_host_abi::HostMethod) -> HostRow<'_> {
        HostRow {
            name: hm.name,
            params: hm.params,
            result: hm.result,
            wait: hm.wait,
        }
    }
}

/// A vtable slot of a host trait that a runtime profile declares itself
/// (spec/conformance/README.md "Runtime Profiles"), which the ABI table
/// does not list: it is imported from `hd:<Trait>` by the method's name,
/// waits when the method suspends, and every argument and result crosses
/// in the exchange buffer, so a result gets each check of its declared
/// type (`module.profile.host-result.*`). A data type with a field that
/// is not `pub` crosses as its JSON text through the codecs collection
/// gathered for it (`module.boundary.serialize.exact`,
/// `.deserialize.exact`). With `handle`, the trait is a profile handle's
/// and the handle number crosses first, as an `i32` (`handle_slots`).
fn profile_slot(
    lay: &Lay<'_>,
    key: &str,
    name: &str,
    m: DefId,
    sig: WTy,
    handle: bool,
    at: crate::boundary::AtProfile<'_>,
) -> StageResult<Helper> {
    use hd_host_abi::{Codec, Wait};
    let (pool, env) = (lay.pool, lay.env);
    let full = pool.list(&[hd_mono::class_ref(pool)]);
    let params: Vec<Ty> = env
        .params(m)
        .unwrap_or_default()
        .into_iter()
        .skip(1)
        .map(|p| subst(pool, env, m, full, p))
        .collect();
    let ret = subst(pool, env, m, full, env.ret(m).unwrap_or(Ty::VOID));
    let mut codecs = vec![Codec::Buffer(""); params.len()];
    if handle {
        codecs.insert(0, Codec::Scalar(hd_host_abi::Scalar::I32));
    }
    let row = HostRow {
        name,
        params: &codecs,
        result: if lay.vts(ret)?.is_empty() {
            Codec::Void
        } else {
            Codec::Buffer("")
        },
        wait: if env.suspends(m) {
            Wait::May
        } else {
            Wait::Never
        },
    };
    host_slot(lay, key, &row, m, sig, handle, Some(at))
}

/// One vtable slot of a default-profile provider (`crates/hd_run_wasmtime/DESIGN.md`
/// §17.2): a host call for a method that never waits, a host leaf frame
/// for one that may, or a stub that panics for a method whose declaration
/// and ABI row disagree. With `handle`, the method's first ABI argument
/// is a host handle's number, the slot's first parameter after the
/// receiver (`handle_slots`).
fn host_slot(
    lay: &Lay<'_>,
    key: &str,
    hm: &HostRow<'_>,
    m: DefId,
    sig: WTy,
    handle: bool,
    at: Option<crate::boundary::AtProfile<'_>>,
) -> StageResult<Helper> {
    use crate::boundary::{AtProfile, BTy};
    use crate::rt::{ArgCodec, ResCodec};
    use hd_host_abi::Codec;
    let (pool, env) = (lay.pool, lay.env);
    let unlowered = |sig: WTy| {
        Ok(Helper::Unlowered {
            sig,
            what: format!("the host method `{key}.{}`", hm.name),
        })
    };
    let waits = hm.wait == hd_host_abi::Wait::May;
    if env.suspends(m) != waits {
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
    let codecs = if handle {
        args.push(ArgCodec::Scalar(VT::I32));
        hm.params.get(1..).unwrap_or_default()
    } else {
        hm.params
    };
    if params.len() != codecs.len() {
        return unlowered(sig);
    }
    for (p, c) in params.iter().zip(codecs) {
        let pt = subst(pool, env, m, full, *p);
        let vts = lay.vts(pt)?;
        args.push(match (c, vts.as_slice()) {
            // A lone string crosses as its bytes alone (§17.4).
            (Codec::Buffer(_), _) if hm.params.len() == 1 && lay.strip(pt) == Ty::STRING => {
                ArgCodec::Str
            }
            (Codec::Buffer(_), _) => ArgCodec::Buf(match at {
                Some(at) => BTy::of_json(lay, pt, AtProfile { out: true, ..at })?,
                None => BTy::of(lay, pt)?,
            }),
            (Codec::Scalar(_), [VT::Ref(t, false)]) if t.fields() == [VT::I64] => {
                ArgCodec::DataI64((**t).clone())
            }
            (Codec::Scalar(_), [v @ (VT::I32 | VT::I64 | VT::F64)]) => ArgCodec::Scalar(v.clone()),
            _ => return unlowered(sig),
        });
    }
    let ret = subst(pool, env, m, full, env.ret(m).unwrap_or(Ty::VOID));
    let rv = lay.vts(ret)?;
    let result = match (hm.result, rv.as_slice()) {
        (Codec::Void, []) => ResCodec::Void,
        (Codec::Scalar(_), [VT::Ref(t, false)]) if !waits && t.fields() == [VT::I64] => {
            ResCodec::DataI64((**t).clone())
        }
        (Codec::Scalar(_), [v @ (VT::I32 | VT::I64 | VT::F64)]) if !waits => {
            ResCodec::Scalar(v.clone())
        }
        (Codec::Buffer(_), _) => {
            let b = match at {
                Some(at) => BTy::of_json(lay, ret, AtProfile { out: false, ..at })?,
                None => BTy::of(lay, ret)?,
            };
            if waits && b.is_tags() && rv == [VT::I32, VT::I32] {
                ResCodec::Tags
            } else {
                ResCodec::Buf(b)
            }
        }
        _ => return unlowered(sig),
    };
    if !waits {
        return Ok(Helper::HostCall {
            sig,
            module: format!("hd:{key}"),
            method: hm.name.to_owned(),
            args,
            result,
        });
    }
    let (base, _) = suspend_base(&result.vts());
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
