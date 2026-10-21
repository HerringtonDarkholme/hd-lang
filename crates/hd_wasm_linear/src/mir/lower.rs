//! Closure-stripped TIR to MIR, for the subset the prototype covers:
//! scalars, structs, enums and their matches, locals and cells, places
//! through references, list elements, calls, `if` and loops. Anything else
//! leaves the function `unsupported`.
//!
//! Counts are conservative: a value that a statement consumes (a local's
//! store, an owned argument, a field store, a returned value) is a fresh
//! copy with a `retain`; a value read from memory is retained too; every
//! counted variable is released at the exit. [`super::own`] then places
//! the releases at last use and removes the counts it can prove needless.
//!
//! Lists are laid out as the study proposes (`docs/compiler/memory-ir.md`,
//! "The list representation"): a list is the address of its counted
//! buffer, whose payload holds the capacity at 0, the length at 4 and the
//! elements from 8. A change writes the buffer back to the list's place.
//!
//! Field writes. The checker writes `l.f = v` on a local as
//! `LocalSet l (CopyData (LocalGet l) [f = v])`. The lowering stores the
//! field in place when the `CopyData` is that store's only use: it is the
//! TIR `Write(place)` the migration adds, read from today's shape.

use hd_base::{DefId, HashMap, HashSet, InstId};
use hd_mono::{Sub, Target, TargetKind, printed_type_name, subst};
use hd_tir::ir::{Body, Coercion, IntrinsicOp, NONE, PrimOp, Ref, Tag, local_flags, place_root};
use hd_types::{Prim as P, Ty};
use hd_types::{TyData, TyList};

use super::{Callee, Count, Func, Kind, Label, Mode, Operand, Proj, Rvalue, Stmt, VarId, VarKind};
use crate::Input;
use crate::layout::{self, Kind as LKind, Pass};

/// The list buffer's payload offsets (§23.7, without the header).
const LEN: u32 = 4;
const ELEMS: u32 = 8;

/// A TIR value as the lowering holds it: an operand, or a place not read
/// yet, so that a field of a place reads only that field.
#[derive(Clone, Debug)]
enum V {
    Op(Operand),
    Place(Place),
    /// A `CopyData` not built yet: a field store may take its place.
    Update,
}

#[derive(Clone, Debug)]
struct Place {
    base: Base,
    ty: Ty,
}

#[derive(Clone, Debug)]
enum Base {
    /// Some values of a variable.
    Reg(VarId, Option<Proj>),
    /// Memory at an address plus an offset.
    Mem(Operand, u32),
}

/// What a statement about to be lowered may change, so that place values
/// read before it are read first ([`Lw::flush_for`]).
#[derive(Clone, Copy)]
enum Hazard {
    /// Anything: control flow.
    All,
    /// Memory: a store or a call.
    Memory,
    /// One variable: its assignment.
    Var(VarId),
}

/// Where a `br` to a TIR label goes, and the variable its value goes to.
#[derive(Clone, Copy)]
struct Target2 {
    brk: Label,
    cont: Option<Label>,
    result: Option<VarId>,
}

struct Lw<'i, 'a> {
    input: &'i Input<'a>,
    b: &'a Body,
    item: DefId,
    args: TyList,
    calls: &'i HashMap<u32, Target>,
    f: Func,
    out: Vec<Vec<Stmt>>,
    vals: HashMap<u32, V>,
    /// How many reads of each instruction's value are still to come.
    uses: Vec<u32>,
    /// The values held as places that a later read may still want.
    pending: Vec<u32>,
    locals: HashMap<u32, VarId>,
    cells: HashSet<u32>,
    /// The cell locals whose declaring store the lowering has passed.
    made: HashSet<VarId>,
    targets: HashMap<u32, Target2>,
    /// The arm labels of each `Match` being lowered.
    arms: Vec<(u32, Vec<Label>)>,
    /// Each open cleanup scope's suites and the depth of its body's list.
    suites: Vec<(Vec<u32>, usize)>,
    origin: u32,
    counts: bool,
}

type R<T> = Result<T, String>;

/// The MIR of instance `id`, its passes run and checked.
#[must_use]
pub fn function(input: &Input<'_>, id: InstId) -> Func {
    let t = &input.collected.table;
    let item = t.item[id.idx()];
    let mut f = Func::new((input.path)(item));
    if t.sub[id.idx()] != Sub::Body(0) {
        f.unsupported = Some("a closure, adapter or erased body".into());
        return f;
    }
    let Some(b) = (input.body)(id) else {
        f.unsupported = Some("an instance without TIR".into());
        return f;
    };
    f.tir_len = u32::try_from(b.len()).expect("insts");
    if input.env.suspends(item) {
        f.unsupported = Some("a suspending body (the emitter keeps its state machine)".into());
        return f;
    }
    let mut lw = Lw {
        input,
        b,
        item,
        args: t.args[id.idx()],
        calls: &input.collected.calls[id.idx()],
        f,
        out: vec![Vec::new()],
        vals: HashMap::default(),
        uses: uses(b),
        pending: Vec::new(),
        locals: HashMap::default(),
        cells: cells(b),
        made: HashSet::default(),
        targets: HashMap::default(),
        arms: Vec::new(),
        suites: Vec::new(),
        origin: NONE,
        counts: input.runtime.is_some(),
    };
    match lw.body() {
        Ok(()) => {
            let mut f = lw.f;
            super::own::run(&mut f);
            f.errors = super::verify::verify(&f);
            f
        }
        Err(what) => {
            let mut f = lw.f;
            f.unsupported = Some(what);
            f.body.clear();
            f.exit.clear();
            f
        }
    }
}

/// How many times each instruction's value is read, by the operand
/// schema of its readers.
fn uses(b: &Body) -> Vec<u32> {
    // Only listed instructions run: one a later lowering made unneeded
    // was taken out of its block (`TirBuilder::unlist`).
    let mut listed = vec![false; b.len()];
    for i in 0..b.len() {
        if b.tags[i] == Tag::Block {
            for &k in b.record(b.data[i][0]) {
                if let Some(x) = listed.get_mut(k as usize) {
                    *x = true;
                }
            }
        }
    }
    let mut n = vec![0u32; b.len()];
    for i in (0..b.len()).filter(|i| listed[*i]) {
        for r in reads(b, i) {
            if let Some(k) = Ref(r).as_inst()
                && let Some(c) = n.get_mut(k.0 as usize)
            {
                *c += 1;
            }
        }
    }
    n
}

/// The values instruction `i` reads, by its tag's operand schema.
fn reads(b: &Body, i: usize) -> Vec<u32> {
    use hd_tir::ir::Op;
    let [a, w] = b.data[i];
    let tag = b.tags[i];
    let mut out = Vec::new();
    match tag {
        Tag::Call | Tag::Await | Tag::DefaultCall => {
            let r = b.record(w);
            out.extend_from_slice(&r[..r.len().saturating_sub(3)]);
            return out;
        }
        Tag::CopyData => {
            out.push(a);
            out.extend(b.record(w).iter().skip(1).step_by(2));
            return out;
        }
        Tag::FieldSet => {
            out.push(a);
            out.extend(b.record(w).get(1));
            return out;
        }
        Tag::Intrinsic | Tag::Interp | Tag::NewData | Tag::NewVariant | Tag::NewMap => {
            out.extend_from_slice(b.record(w));
            return out;
        }
        Tag::NewList if a == NONE => {
            out.extend_from_slice(b.record(w));
            return out;
        }
        Tag::RefWrite => {
            out.extend_from_slice(b.record(a));
            return out;
        }
        _ => {}
    }
    for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
        match op {
            Op::Value if word != NONE => out.push(word),
            Op::Values => out.extend_from_slice(b.record(word)),
            _ => {}
        }
    }
    out
}

/// The locals some reference points at: each lives in a cell.
fn cells(b: &Body) -> HashSet<u32> {
    let mut out = HashSet::default();
    for i in 0..b.len() {
        if let Some(p) = b.place(i)
            && p.kind == place_root::LOCAL
        {
            out.insert(p.x);
        }
    }
    out
}

impl Lw<'_, '_> {
    fn pool(&self) -> &hd_types::InternPool {
        self.input.pool
    }

    fn env(&self) -> &dyn hd_mono::ProgramEnv {
        self.input.env
    }

    fn sub(&self, t: Ty) -> Ty {
        subst(self.pool(), self.env(), self.item, self.args, t)
    }

    fn ty_of(&self, r: u32) -> Ty {
        if r == NONE {
            return Ty::VOID;
        }
        self.sub(match Ref(r).as_inst() {
            Some(i) => self.b.ty[i.0 as usize],
            None => self.b.consts[(r & !Ref::CONST_BIT) as usize].0,
        })
    }

    fn name(&self, t: Ty) -> String {
        printed_type_name(self.pool(), self.env(), t)
            .replace("app.main.", "")
            .replace("std.core.", "")
    }

    fn void(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::Never)
            || layout::mem(self.pool(), self.env(), t).is_ok_and(|m| m.leaves.is_empty())
    }

    fn never(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::Never)
    }

    fn counted(&self, t: Ty) -> bool {
        self.counts && layout::walk(self.pool(), self.env(), t).is_some()
    }

    fn mem(&self, t: Ty) -> R<layout::Mem> {
        layout::mem(self.pool(), self.env(), t).map_err(|e| e.what.clone())
    }

    fn var(&mut self, name: String, tir_local: Option<u32>, t: Ty, kind: VarKind) -> R<VarId> {
        let vals = self.mem(t)?.vals();
        let id = VarId::try_from(self.f.vars.len()).expect("vars");
        let refs = if self.counts {
            layout::walk(self.pool(), self.env(), t).map_or(0, |w| refs(&w))
        } else {
            0
        };
        self.f.vars.push(super::Var {
            name,
            tir_local,
            ty: self.name(t),
            vals,
            counted: self.counted(t),
            refs,
            kind,
            cell: false,
        });
        Ok(id)
    }

    fn temp(&mut self, t: Ty) -> R<VarId> {
        let n = self.f.vars.len();
        self.var(format!("t{n}"), None, t, VarKind::Temp)
    }

    fn emit(&mut self, kind: Kind) {
        let s = self.f.stmt(self.origin, kind);
        self.out.last_mut().expect("an open list").push(s);
    }

    fn open(&mut self) {
        self.out.push(Vec::new());
    }

    fn close(&mut self) -> Vec<Stmt> {
        self.out.pop().expect("an open list")
    }

    fn count(&mut self, op: Count, var: VarId) {
        if self.f.counted(var) {
            let ty = self.f.vars[var as usize].ty.clone();
            self.emit(Kind::Count { op, var, ty });
        }
    }

    // ---- the function ----

    fn body(&mut self) -> R<()> {
        let b = self.b;
        let mut in_sub = vec![false; b.local_ty.len()];
        for r in &b.sub_params[1..] {
            for &l in &b.extra[r.start as usize + 1..(r.start + 1 + r.len) as usize] {
                if let Some(x) = in_sub.get_mut(l as usize) {
                    *x = true;
                }
            }
        }
        for (l, &inner) in in_sub.iter().enumerate() {
            if b.local_flags[l] & local_flags::PARAM == 0 || inner {
                continue;
            }
            let t = self.sub(b.local_ty[l]);
            let mode = match layout::pass(self.pool(), self.env(), self.counts, t) {
                Pass::Uncounted => Mode::Plain,
                Pass::Owned(_) => Mode::Owned,
                Pass::Borrowed => Mode::Borrowed,
            };
            let l32 = u32::try_from(l).expect("locals");
            let v = self.var(format!("${l}"), Some(l32), t, VarKind::Param(mode))?;
            self.f.params.push(v);
            self.locals.insert(l32, v);
            if self.cells.contains(&l32) {
                return Err("a parameter some reference points at".into());
            }
        }
        let ret = self.sub(self.env().ret(self.item).unwrap_or(Ty::VOID));
        if !self.void(ret) {
            let r = self.var("result".into(), None, ret, VarKind::Result)?;
            self.f.result = Some(r);
        }
        let root = b.sub_root[0];
        let tail = self.block(root)?;
        if let Some(t) = tail
            && Ref(t).as_inst().is_some()
        {
            self.origin = t;
        }
        if let (Some(r), Some(t)) = (self.f.result, tail) {
            let o = self.consume(t)?;
            self.emit(Kind::Assign {
                dst: r,
                proj: None,
                rv: Rvalue::Use(o.clone()),
            });
            self.consumed(&o);
        }
        self.f.body = self.close();
        // The exit releases every counted variable that may own a value.
        self.origin = NONE;
        self.open();
        for v in 0..self.f.vars.len() {
            let v = VarId::try_from(v).expect("vars");
            let var = &self.f.vars[v as usize];
            let owns = matches!(
                var.kind,
                VarKind::Param(Mode::Owned) | VarKind::Local | VarKind::Temp
            );
            if owns && var.counted {
                self.count(Count::Release, v);
            }
        }
        self.f.exit = self.close();
        Ok(())
    }

    /// Lowers block `blk`'s statements into the open list; its tail value.
    /// A block that a list names is run by the instruction that owns it,
    /// never where the list names it. A block some `Break` targets is a
    /// labelled MIR block whose value is its result variable.
    fn block(&mut self, blk: u32) -> R<Option<u32>> {
        let b = self.b;
        if b.tags[blk as usize] != Tag::Block {
            return Err(format!(
                "a {} where a block belongs",
                b.tags[blk as usize].name()
            ));
        }
        let target = self.labelled(blk)?;
        if target.is_some() {
            self.flush();
            self.open();
        }
        for &i in b.record(b.data[blk as usize][0]) {
            if b.tags[i as usize] != Tag::Block {
                self.inst(i)?;
            }
        }
        let tail = b.data[blk as usize][1];
        let tail = (tail != NONE).then_some(tail);
        let Some(target) = target else {
            return Ok(tail);
        };
        if let (Some(t), Some(res)) = (tail, target.result)
            && !self.never(self.ty_of(t))
        {
            let o = self.consume(t)?;
            self.emit(Kind::Assign {
                dst: res,
                proj: None,
                rv: Rvalue::Use(o.clone()),
            });
            self.consumed(&o);
        }
        self.flush();
        let body = self.close();
        self.emit(Kind::Block {
            label: target.brk,
            body,
        });
        Ok(target.result.map(|r| {
            self.vals.insert(blk, V::Op(Operand::Var(r, None)));
            self.uses[blk as usize] += 1;
            blk
        }))
    }

    /// Lowers a child block into a list of its own; its tail value.
    fn arm(&mut self, blk: u32) -> R<(Vec<Stmt>, Option<u32>)> {
        self.flush();
        self.open();
        let tail = self.block(blk)?;
        Ok((self.close(), tail))
    }

    // ---- values ----

    fn konst(&self, r: u32) -> Operand {
        let (ty, bits) = self.b.consts[(r & !Ref::CONST_BIT) as usize];
        let t = hd_mono::layout::strip_newtype(self.pool(), self.env(), self.sub(ty));
        let text = match self.pool().get(t) {
            TyData::Prim(P::Bool) => (bits != 0).to_string(),
            TyData::Prim(P::F64) => format!("{:?}", f64::from_bits(bits)),
            TyData::Prim(P::F32) => format!(
                "{:?}",
                f32::from_bits(u32::try_from(bits & 0xffff_ffff).expect("low"))
            ),
            TyData::Prim(P::String) => {
                format!(
                    "{:?}",
                    self.b.strings[usize::try_from(bits).expect("string")]
                )
            }
            TyData::Prim(P::I8 | P::I16 | P::I32 | P::I64) => {
                let (w, _, _) = layout::num(self.pool(), t);
                let shift = 64 - u32::from(w);
                ((bits.cast_signed() << shift) >> shift).to_string()
            }
            _ => bits.to_string(),
        };
        Operand::Const(text)
    }

    /// Builds `CopyData` `i`: every field from the source but the
    /// replaced ones.
    fn update(&mut self, i: u32) -> R<Operand> {
        let b = self.b;
        let [a, w] = b.data[i as usize];
        let t = self.ty_of(i);
        let rec = b.record(w).to_vec();
        let src = self.read(a)?;
        let s = layout::strip(self.pool(), self.env(), t);
        let n = match self.pool().get(s) {
            TyData::Adt { def, .. } => self.env().data_fields(def).unwrap_or_default().len(),
            _ => return Err("a copy-update of a value that is no struct".into()),
        };
        let mut parts = Vec::new();
        for k in 0..n {
            let k32 = u32::try_from(k).expect("fields");
            if let Some(p) = rec.chunks(2).find(|p| p[0] == k32) {
                parts.push((self.read(p[1])?, self.ty_of(p[1])));
            } else {
                let (ft, _, proj) = self.path(t, &[k32])?;
                parts.push((project(&src, &proj), ft));
            }
        }
        let ty = self.name(t);
        let outer = self.origin;
        self.origin = i;
        let v = self.build(i, parts, |parts| Rvalue::Make { ty, parts })?;
        self.origin = outer;
        Ok(Operand::Var(v, None))
    }

    /// Instruction `i`'s new value from `make` over its parts: each counted
    /// part is a copy of its own, which the value takes over.
    fn build(
        &mut self,
        i: u32,
        parts: Vec<(Operand, Ty)>,
        make: impl FnOnce(Vec<Operand>) -> Rvalue,
    ) -> R<VarId> {
        let mut ops = Vec::new();
        for (o, t) in parts {
            ops.push(self.copy(o, t)?);
        }
        let v = self.assign(i, make(ops.clone()))?;
        for o in &ops {
            self.consumed(o);
        }
        Ok(v)
    }

    /// Takes one read of value `r`.
    fn take(&mut self, r: u32) {
        if let Some(i) = Ref(r).as_inst()
            && let Some(c) = self.uses.get_mut(i.0 as usize)
        {
            *c = c.saturating_sub(1);
        }
    }

    /// Value `r` for a read that consumes nothing: a variable's own
    /// values where it is one, else a temporary that holds it, retained
    /// when it came from memory.
    fn read(&mut self, r: u32) -> R<Operand> {
        if Ref(r).as_inst().is_none() {
            return Ok(self.konst(r));
        }
        self.take(r);
        match self.vals.get(&r).cloned() {
            Some(V::Op(o)) => Ok(o),
            Some(V::Place(p)) => match p.base {
                Base::Reg(v, proj) => Ok(Operand::Var(v, proj)),
                Base::Mem(..) => self.materialize(r, &p),
            },
            Some(V::Update) => self.update(r),
            None => Err(format!("a value read before it is computed (%{r})")),
        }
    }

    /// Value `r` for a statement that consumes it: a copy with a count of
    /// its own, which the caller zeroes ([`Self::consumed`]) after the
    /// consuming statement.
    fn consume(&mut self, r: u32) -> R<Operand> {
        let o = self.read(r)?;
        let t = self.ty_of(r);
        self.copy(o, t)
    }

    fn copy(&mut self, o: Operand, t: Ty) -> R<Operand> {
        if !self.counted(t) || matches!(o, Operand::Const(_)) {
            return Ok(o);
        }
        let u = self.temp(t)?;
        self.emit(Kind::Assign {
            dst: u,
            proj: None,
            rv: Rvalue::Use(o),
        });
        self.count(Count::Retain, u);
        Ok(Operand::Var(u, None))
    }

    /// After a statement consumed `o`'s count: `o` holds none.
    fn consumed(&mut self, o: &Operand) {
        if let Operand::Var(v, None) = o
            && self.f.counted(*v)
            && self.f.vars[*v as usize].kind == VarKind::Temp
        {
            self.emit(Kind::Kill {
                var: *v,
                proj: None,
            });
        }
    }

    /// Reads place value `r` from memory into a temporary.
    fn materialize(&mut self, r: u32, p: &Place) -> R<Operand> {
        let Base::Mem(addr, off) = &p.base else {
            return Err("a register place read as memory".into());
        };
        let t = self.temp(p.ty)?;
        let ty = self.name(p.ty);
        self.emit(Kind::Assign {
            dst: t,
            proj: None,
            rv: Rvalue::Load {
                addr: addr.clone(),
                off: *off,
                ty,
            },
        });
        self.count(Count::Retain, t);
        let o = Operand::Var(t, None);
        self.vals.insert(r, V::Op(o.clone()));
        Ok(o)
    }

    /// Reads every place value that a later instruction may still read,
    /// before control flow: an arm may change what it names.
    fn flush(&mut self) {
        self.flush_for(Hazard::All);
    }

    /// Reads every place value that a later instruction may still read
    /// and that hazard `h` may change.
    fn flush_for(&mut self, h: Hazard) {
        let pending = std::mem::take(&mut self.pending);
        for r in pending {
            if self.uses[r as usize] == 0 {
                continue;
            }
            let p = match self.vals.get(&r).cloned() {
                Some(V::Place(p)) => p,
                Some(V::Update) => {
                    let src = self.b.data[r as usize][0];
                    let hit = match (h, self.vals.get(&src)) {
                        (Hazard::All, _) | (_, None | Some(V::Op(_) | V::Update)) => true,
                        (Hazard::Memory, Some(V::Place(q))) => matches!(q.base, Base::Mem(..)),
                        (Hazard::Var(v), Some(V::Place(q))) => match &q.base {
                            Base::Reg(b, _) => *b == v,
                            Base::Mem(a, _) => a.var() == Some(v),
                        },
                    };
                    if hit {
                        // Built where it stands: its reads count as uses.
                        self.uses[r as usize] += 1;
                        if self.update(r).is_err() {
                            self.pending.push(r);
                        }
                        self.uses[r as usize] -= 1;
                    } else {
                        self.pending.push(r);
                    }
                    continue;
                }
                _ => continue,
            };
            let hit = match (h, &p.base) {
                (Hazard::All, _) | (Hazard::Memory, Base::Mem(..)) => true,
                (Hazard::Var(v), Base::Reg(b, _)) => *b == v,
                (Hazard::Var(v), Base::Mem(a, _)) => a.var() == Some(v),
                _ => false,
            };
            if !hit {
                self.pending.push(r);
                continue;
            }
            let got = match &p.base {
                Base::Mem(..) => self.materialize(r, &p).map(|_| ()),
                Base::Reg(v, proj) => {
                    let o = Operand::Var(*v, proj.clone());
                    self.temp(p.ty).map(|t| {
                        self.emit(Kind::Assign {
                            dst: t,
                            proj: None,
                            rv: Rvalue::Use(o),
                        });
                        self.count(Count::Retain, t);
                        self.vals.insert(r, V::Op(Operand::Var(t, None)));
                    })
                }
            };
            if got.is_err() {
                self.pending.push(r);
            }
        }
    }

    fn set_val(&mut self, i: u32, v: V) {
        if matches!(v, V::Place(_)) {
            self.pending.push(i);
        }
        self.vals.insert(i, v);
    }

    /// Instruction `i`'s value in a new temporary.
    fn assign(&mut self, i: u32, rv: Rvalue) -> R<VarId> {
        let t = self.temp(self.ty_of(i))?;
        self.emit(Kind::Assign {
            dst: t,
            proj: None,
            rv,
        });
        self.set_val(i, V::Op(Operand::Var(t, None)));
        Ok(t)
    }

    /// The TIR local `l`'s variable, made on first use.
    fn local(&mut self, l: u32) -> R<VarId> {
        if let Some(&v) = self.locals.get(&l) {
            return Ok(v);
        }
        let t = self.sub(self.b.local_ty[l as usize]);
        let cell = self.cells.contains(&l);
        let v = if cell {
            // The variable holds the cell's address; the cell holds `t`.
            let v = self.var(format!("${l}"), Some(l), t, VarKind::Local)?;
            let var = &mut self.f.vars[v as usize];
            var.vals = vec![crate::module::Val::I32];
            var.counted = self.counts;
            var.cell = true;
            v
        } else {
            self.var(format!("${l}"), Some(l), t, VarKind::Local)?
        };
        self.locals.insert(l, v);
        Ok(v)
    }

    /// The field path `fields` below type `t`: its type, the offset of its
    /// values in memory, and which of `t`'s values it holds.
    fn path(&self, mut t: Ty, fields: &[u32]) -> R<(Ty, u32, Proj)> {
        let (mut off, mut first) = (0u32, 0usize);
        let mut count = self.mem(t)?.leaves.len();
        let mut text = String::new();
        for &f in fields {
            let s = layout::strip(self.pool(), self.env(), t);
            let (_, parts) =
                layout::struct_mem(self.pool(), self.env(), s).map_err(|e| e.what.clone())?;
            let (fts, names) = match self.pool().get(s) {
                TyData::Adt { def, args } => (
                    self.env()
                        .data_fields(def)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|x| subst(self.pool(), self.env(), def, args, x))
                        .collect::<Vec<_>>(),
                    self.env().data_field_names(def).unwrap_or_default(),
                ),
                TyData::Tuple { .. } => (layout::tuple_members(self.pool(), s), Vec::new()),
                _ => return Err("a field of a value that is no struct".into()),
            };
            let (Some(part), Some(&ft)) = (parts.get(f as usize), fts.get(f as usize)) else {
                return Err("a field the struct does not have".into());
            };
            off += part.offset;
            first += part.first;
            count = part.count;
            t = ft;
            let _ = std::fmt::Write::write_fmt(
                &mut text,
                format_args!(
                    ".{}",
                    names
                        .get(f as usize)
                        .cloned()
                        .unwrap_or_else(|| f.to_string())
                ),
            );
        }
        let leaves = (first..first + count)
            .map(|k| u32::try_from(k).expect("leaves"))
            .collect();
        Ok((t, off, Proj { leaves, path: text }))
    }

    /// A place's field `f`.
    fn field_of(&self, p: &Place, f: u32) -> R<Place> {
        let (ft, off, proj) = self.path(p.ty, &[f])?;
        Ok(Place {
            ty: ft,
            base: match &p.base {
                Base::Reg(v, outer) => Base::Reg(*v, Some(join(outer.as_ref(), &proj))),
                Base::Mem(a, o) => Base::Mem(a.clone(), o + off),
            },
        })
    }

    /// The element type of list type `t` and its layout.
    fn elem(&self, t: Ty) -> R<(Ty, layout::Mem)> {
        let TyData::Adt { args, .. } = self.pool().get(layout::strip(self.pool(), self.env(), t))
        else {
            return Err("a list operation on a value that is not a list".into());
        };
        let et = *self
            .pool()
            .list_items(args)
            .first()
            .ok_or("a list without its element type")?;
        Ok((et, self.mem(et)?))
    }

    /// The place list value `r` was read from, for a change that writes
    /// the buffer back: a variable, or memory.
    fn list_place(&mut self, r: u32) -> R<Base> {
        match self.vals.get(&r).cloned() {
            Some(V::Place(p)) => {
                self.take(r);
                Ok(p.base)
            }
            _ => Err("a list change through a value that names no place".into()),
        }
    }

    /// Before a change to the list at place `base`: memory and the
    /// variable that holds it may change.
    fn flush_change(&mut self, base: &Base) {
        self.flush_for(Hazard::Memory);
        match base {
            Base::Reg(v, _) => self.flush_for(Hazard::Var(*v)),
            Base::Mem(a, _) => {
                if let Some(v) = a.var() {
                    self.flush_for(Hazard::Var(v));
                }
            }
        }
    }

    /// The list buffer at place `base`, made unique: the variable's own,
    /// or a temporary that borrows from memory.
    fn unique(&mut self, base: &Base, t: Ty) -> R<Operand> {
        let ty = self.name(t);
        match base {
            Base::Reg(v, proj) => {
                let o = Operand::Var(*v, proj.clone());
                self.emit(Kind::Assign {
                    dst: *v,
                    proj: proj.clone(),
                    rv: Rvalue::Unique {
                        addr: o.clone(),
                        ty,
                    },
                });
                Ok(o)
            }
            Base::Mem(a, off) => {
                let addr = self.offset(a.clone(), *off)?;
                let u = self.temp(t)?;
                self.f.vars[u as usize].counted = false;
                self.emit(Kind::Assign {
                    dst: u,
                    proj: None,
                    rv: Rvalue::Unique { addr, ty },
                });
                Ok(Operand::Var(u, None))
            }
        }
    }

    /// `a + off` as an operand.
    fn offset(&mut self, a: Operand, off: u32) -> R<Operand> {
        if off == 0 {
            return Ok(a);
        }
        let u = self.temp(Ty::prim(P::Usize))?;
        self.emit(Kind::Assign {
            dst: u,
            proj: None,
            rv: Rvalue::Offset { base: a, off },
        });
        Ok(Operand::Var(u, None))
    }

    /// The bounds-checked address of element `index` of the list buffer
    /// `buf`.
    fn element(&mut self, buf: &Operand, index: &Operand, size: u32) -> R<Operand> {
        let n = self.temp(Ty::prim(P::Usize))?;
        self.emit(Kind::Assign {
            dst: n,
            proj: None,
            rv: Rvalue::Load {
                addr: buf.clone(),
                off: LEN,
                ty: "usize".into(),
            },
        });
        self.emit(Kind::Check {
            index: index.clone(),
            len: Operand::Var(n, None),
        });
        let a = self.temp(Ty::prim(P::Usize))?;
        self.emit(Kind::Assign {
            dst: a,
            proj: None,
            rv: Rvalue::IndexAddr {
                base: buf.clone(),
                index: index.clone(),
                size,
                off: ELEMS,
            },
        });
        Ok(Operand::Var(a, None))
    }

    /// Stores value `r` of type `t` at `addr + off`: the old value is
    /// released after the new one is stored.
    fn store(&mut self, addr: Operand, off: u32, t: Ty, r: u32) -> R<()> {
        let ty = self.name(t);
        let val = self.consume(r)?;
        let old = if self.counted(t) {
            let o = self.temp(t)?;
            self.emit(Kind::Assign {
                dst: o,
                proj: None,
                rv: Rvalue::Take {
                    addr: addr.clone(),
                    off,
                    ty: ty.clone(),
                },
            });
            Some(o)
        } else {
            None
        };
        // The old value: owned by the place until the store replaces it.
        self.emit(Kind::Store {
            addr,
            off,
            ty,
            val: val.clone(),
        });
        self.consumed(&val);
        if let Some(o) = old {
            self.emit(Kind::Count {
                op: Count::Release,
                var: o,
                ty: self.f.vars[o as usize].ty.clone(),
            });
            self.emit(Kind::Kill { var: o, proj: None });
        }
        Ok(())
    }

    // ---- instructions ----

    fn inst(&mut self, i: u32) -> R<()> {
        let outer = self.origin;
        // An instruction with no source position (a decision tree's jump)
        // keeps the enclosing one's.
        if self.b.syn[i as usize] != hd_base::NodeIdx::NONE || outer == NONE {
            self.origin = i;
        }
        let r = self.inst_body(i);
        self.origin = outer;
        r
    }

    #[allow(clippy::too_many_lines)]
    fn inst_body(&mut self, i: u32) -> R<()> {
        let b = self.b;
        let [a, w] = b.data[i as usize];
        let tag = b.tags[i as usize];
        match tag {
            Tag::LocalGet => {
                let v = self.local(a)?;
                let t = self.ty_of(i);
                let base = if self.f.vars[v as usize].cell {
                    Base::Mem(Operand::Var(v, None), 0)
                } else {
                    Base::Reg(v, None)
                };
                self.set_val(i, V::Place(Place { base, ty: t }));
            }
            Tag::LocalSet => {
                if w == NONE {
                    return Ok(());
                }
                if self.never(self.ty_of(w)) {
                    self.emit(Kind::Unreachable);
                    return Ok(());
                }
                let v = self.local(a)?;
                let t = self.sub(b.local_ty[a as usize]);
                if !self.f.vars[v as usize].cell && self.field_write(v, a, w)? {
                    return Ok(());
                }
                self.flush_for(if self.f.vars[v as usize].cell {
                    Hazard::Memory
                } else {
                    Hazard::Var(v)
                });
                if self.f.vars[v as usize].cell {
                    return self.cell_set(v, t, w);
                }
                let val = self.read(w)?;
                self.count(Count::Release, v);
                self.emit(Kind::Assign {
                    dst: v,
                    proj: None,
                    rv: Rvalue::Use(val),
                });
                self.count(Count::Retain, v);
            }
            Tag::Prim => {
                let op = PrimOp::from_u32(a).ok_or("an unknown operator")?;
                let mut args = Vec::new();
                for r in b.record(w).to_vec() {
                    args.push(self.read(r)?);
                }
                let name = format!("{op:?}").to_lowercase();
                self.assign(i, Rvalue::Prim { op: name, args })?;
            }
            Tag::And | Tag::Or => {
                let res = self.temp(Ty::BOOL)?;
                let c = self.read(a)?;
                self.emit(Kind::Assign {
                    dst: res,
                    proj: None,
                    rv: Rvalue::Use(c.clone()),
                });
                let (stmts, tail) = self.arm(w)?;
                self.open();
                self.out.last_mut().expect("open").extend(stmts);
                if let Some(t) = tail {
                    let o = self.read(t)?;
                    self.emit(Kind::Assign {
                        dst: res,
                        proj: None,
                        rv: Rvalue::Use(o),
                    });
                }
                let inner = self.close();
                let (then, els) = if tag == Tag::And {
                    (inner, Vec::new())
                } else {
                    (Vec::new(), inner)
                };
                self.emit(Kind::If {
                    cond: Operand::Var(res, None),
                    then,
                    els,
                });
                self.set_val(i, V::Op(Operand::Var(res, None)));
            }
            Tag::Call | Tag::DefaultCall => self.call(i, w)?,
            Tag::CallValue => {
                self.flush_for(Hazard::Memory);
                let fv = self.read(a)?;
                let mut args = Vec::new();
                let rec = b.record(w).to_vec();
                // The record holds the arguments, then the context.
                let n = match self.pool().get(self.ty_of(a)) {
                    TyData::Fn { params, .. } => self.pool().list_items(params).len(),
                    _ => rec.len(),
                };
                for r in rec.into_iter().take(n) {
                    let t = self.ty_of(r);
                    let mode = self.mode(t);
                    let o = match mode {
                        Mode::Owned => self.consume(r)?,
                        _ => self.read(r)?,
                    };
                    args.push((o, mode));
                }
                let dst = self.call_result(i)?;
                self.emit(Kind::Call {
                    dst,
                    callee: Callee::Value(fv),
                    args: args.clone(),
                });
                for (o, m) in &args {
                    if *m == Mode::Owned {
                        self.consumed(o);
                    }
                }
            }
            Tag::Coerce => {
                let kind = b.record(w).first().copied().unwrap_or(NONE);
                let same = [
                    Coercion::Weaken,
                    Coercion::Variance,
                    Coercion::RowSubsume,
                    Coercion::Identity,
                ]
                .iter()
                .any(|c| *c as u32 == kind);
                if same {
                    let v = match self.vals.get(&a).cloned() {
                        Some(v) => {
                            self.take(a);
                            v
                        }
                        None => V::Op(self.read(a)?),
                    };
                    self.set_val(i, v);
                } else if kind == Coercion::WrapSome as u32 {
                    let o = self.read(a)?;
                    let at = self.ty_of(a);
                    let ty = self.name(self.ty_of(i));
                    self.build(i, vec![(o, at)], |parts| Rvalue::Variant {
                        ty,
                        variant: "Some".into(),
                        parts,
                    })?;
                } else if kind == Coercion::Never as u32 {
                    self.emit(Kind::Unreachable);
                } else {
                    return Err("a coercion to a trait value or `Any`".into());
                }
            }
            Tag::NewData => {
                let t = self.ty_of(i);
                if !matches!(layout::kind(self.pool(), self.env(), t), LKind::Value) {
                    return Err("a compiler-supplied data object".into());
                }
                let mut parts = Vec::new();
                for r in b.record(w).to_vec() {
                    parts.push((self.read(r)?, self.ty_of(r)));
                }
                let ty = self.name(t);
                self.build(i, parts, |parts| Rvalue::Make { ty, parts })?;
            }
            Tag::NewTuple if a == NONE => {
                let mut parts = Vec::new();
                for r in b.record(w).to_vec() {
                    parts.push((self.read(r)?, self.ty_of(r)));
                }
                let ty = self.name(self.ty_of(i));
                self.build(i, parts, |parts| Rvalue::Make { ty, parts })?;
            }
            Tag::NewVariant => {
                let mut parts = Vec::new();
                for r in b.record(w).to_vec() {
                    parts.push((self.read(r)?, self.ty_of(r)));
                }
                let t = self.ty_of(i);
                let ty = self.name(t);
                let variant = self.variant_name(t, a);
                self.build(i, parts, |parts| Rvalue::Variant { ty, variant, parts })?;
            }
            Tag::NewList if a == NONE => {
                let t = self.ty_of(i);
                let (et, m) = self.elem(t)?;
                let elems = b.record(w).to_vec();
                if elems.is_empty() {
                    // The one static empty buffer: immortal.
                    self.set_val(i, V::Op(Operand::Const("[]".into())));
                    return Ok(());
                }
                let n = Operand::Const(elems.len().to_string());
                let ty = self.name(t);
                let v = self.assign(i, Rvalue::Alloc { ty })?;
                let buf = Operand::Var(v, None);
                for off in [0, LEN] {
                    self.emit(Kind::Store {
                        addr: buf.clone(),
                        off,
                        ty: "usize".into(),
                        val: n.clone(),
                    });
                }
                for (k, r) in elems.into_iter().enumerate() {
                    let at = ELEMS + m.size * u32::try_from(k).expect("elements");
                    let val = self.consume(r)?;
                    self.emit(Kind::Store {
                        addr: buf.clone(),
                        off: at,
                        ty: self.name(et),
                        val: val.clone(),
                    });
                    self.consumed(&val);
                }
            }
            Tag::CopyData => {
                self.vals.insert(i, V::Update);
                self.pending.push(i);
            }
            Tag::Field | Tag::TupleGet => {
                let t = self.ty_of(a);
                if !matches!(layout::kind(self.pool(), self.env(), t), LKind::Value) {
                    return Err("a field of a compiler-supplied data object".into());
                }
                let place = if let Some(V::Place(p)) = self.vals.get(&a).cloned() {
                    self.take(a);
                    p
                } else {
                    let o = self.read(a)?;
                    let Operand::Var(v, proj) = o else {
                        return Err("a field of a constant".into());
                    };
                    Place {
                        base: Base::Reg(v, proj),
                        ty: t,
                    }
                };
                let p = self.field_of(&place, w)?;
                self.set_val(i, V::Place(p));
            }
            Tag::RefPlace => self.ref_place(i)?,
            Tag::RefRead => {
                let r = self.read(a)?;
                let t = self.ty_of(i);
                self.set_val(
                    i,
                    V::Place(Place {
                        base: Base::Mem(r, 0),
                        ty: t,
                    }),
                );
            }
            Tag::RefWrite => {
                self.flush_for(Hazard::Memory);
                let rec = b.record(a).to_vec();
                let [rf, v] = rec[..] else {
                    return Err("a write through a reference without its operands".into());
                };
                let addr = self.read(rf)?;
                let t = self.ty_of(v);
                self.store(addr, 0, t, v)?;
            }
            Tag::Intrinsic => self.intrinsic(i, a, w)?,
            // Run by the instruction that owns it ([`Self::block`]).
            Tag::Block => {}
            Tag::Scope => {
                let suites = b.record(w).to_vec();
                let depth = self.out.len();
                self.suites.push((suites, depth));
                let tail = self.block(a)?;
                let (suites, _) = self.suites.pop().unwrap_or_default();
                // The suites run at the scope's end, last registered first.
                // An exit that leaves the scope early is not covered.
                for s in suites.iter().rev() {
                    self.flush();
                    let _ = self.block(*s)?;
                }
                if let Some(t) = tail {
                    let v = match self.vals.get(&t).cloned() {
                        Some(v) => {
                            self.take(t);
                            v
                        }
                        None => V::Op(self.read(t)?),
                    };
                    self.set_val(i, v);
                }
            }
            Tag::Defer => {
                if self.suites.last().is_none_or(|(_, d)| *d != self.out.len()) {
                    return Err("a `defer` registered under a condition".into());
                }
            }
            Tag::If => self.if_(i, a, w)?,
            Tag::Loop => {
                self.flush();
                let t = self.ty_of(i);
                let result = if self.void(t) || self.never(t) {
                    None
                } else {
                    Some(self.temp(t)?)
                };
                let brk = self.f.label();
                let cont = self.f.label();
                self.targets.insert(
                    i,
                    Target2 {
                        brk,
                        cont: Some(cont),
                        result,
                    },
                );
                self.open();
                let _ = self.block(a)?;
                self.flush();
                let body = self.close();
                let lp = self.f.stmt(i, Kind::Loop { label: cont, body });
                self.emit(Kind::Block {
                    label: brk,
                    body: vec![lp],
                });
                if let Some(r) = result {
                    self.set_val(i, V::Op(Operand::Var(r, None)));
                }
            }
            Tag::Break | Tag::Continue => {
                if self.suites.iter().any(|(s, _)| !s.is_empty()) {
                    return Err("an exit that leaves a cleanup scope".into());
                }
                let target = *b
                    .label_inst
                    .get(a as usize)
                    .ok_or("a break with an unknown label")?;
                let Some(&t) = self.targets.get(&target) else {
                    return Err(format!(
                        "a break to {} %{target}, which the prototype does not label",
                        self.b.tags.get(target as usize).map_or("?", |t| t.name())
                    ));
                };
                if tag == Tag::Break {
                    if w != NONE
                        && let Some(res) = t.result
                    {
                        let o = self.consume(w)?;
                        self.emit(Kind::Assign {
                            dst: res,
                            proj: None,
                            rv: Rvalue::Use(o.clone()),
                        });
                        self.consumed(&o);
                    }
                    self.flush();
                    self.emit(Kind::Br { label: t.brk });
                } else {
                    self.flush();
                    self.emit(Kind::Br {
                        label: t.cont.ok_or("a continue to a block")?,
                    });
                }
            }
            Tag::Return => {
                if self.suites.iter().any(|(s, _)| !s.is_empty()) {
                    return Err("an exit that leaves a cleanup scope".into());
                }
                if let (Some(r), true) = (self.f.result, a != NONE) {
                    let o = self.consume(a)?;
                    self.emit(Kind::Assign {
                        dst: r,
                        proj: None,
                        rv: Rvalue::Use(o.clone()),
                    });
                    self.consumed(&o);
                }
                self.flush();
                self.emit(Kind::Return);
            }
            Tag::Unreachable => self.emit(Kind::Unreachable),
            Tag::Match => self.match_(i, a, w)?,
            Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar => {
                let rec = b.record(w).to_vec();
                let [case, then, other] = rec[..] else {
                    return Err("a switch that is not one case and a default".into());
                };
                let x = self.read(a)?;
                let x = if tag == Tag::SwitchTag {
                    let t = self.temp(Ty::prim(P::U32))?;
                    self.emit(Kind::Assign {
                        dst: t,
                        proj: None,
                        rv: Rvalue::Tag(x),
                    });
                    Operand::Var(t, None)
                } else {
                    x
                };
                let c = self.temp(Ty::BOOL)?;
                self.emit(Kind::Assign {
                    dst: c,
                    proj: None,
                    rv: Rvalue::Prim {
                        op: "eq".into(),
                        args: vec![x, Operand::Const(case.to_string())],
                    },
                });
                let (then, _) = self.arm(then)?;
                let els = if other == NONE {
                    vec![self.f.stmt(i, Kind::Unreachable)]
                } else {
                    self.arm(other)?.0
                };
                self.emit(Kind::If {
                    cond: Operand::Var(c, None),
                    then,
                    els,
                });
            }
            Tag::Payload | Tag::Unwrap => {
                let (variant, field) = if tag == Tag::Unwrap {
                    (1, 0)
                } else {
                    let r = b.record(w);
                    (r[0], r[1])
                };
                let t = self.ty_of(a);
                if !matches!(layout::kind(self.pool(), self.env(), t), LKind::Value) {
                    return Err("a payload of a boxed enum".into());
                }
                let x = self.read(a)?;
                let Operand::Var(v, outer) = x else {
                    return Err("a payload of a constant".into());
                };
                let vs = layout::variants(self.pool(), self.env(), t).ok_or("no enum")?;
                let (_, positions) =
                    layout::enum_mem(self.pool(), self.env(), &vs).map_err(|e| e.what)?;
                let at = positions
                    .get(variant as usize)
                    .and_then(|fs| fs.get(field as usize))
                    .ok_or("a payload field the enum does not have")?;
                let base = outer.as_ref().map(|p| p.leaves.clone());
                let leaves = at
                    .iter()
                    .map(|k| {
                        let k = u32::try_from(*k).expect("leaves");
                        base.as_ref().map_or(k, |b| b[k as usize])
                    })
                    .collect();
                let path = format!(
                    "{}.{}.{field}",
                    outer.as_ref().map_or("", |p| p.path.as_str()),
                    self.variant_name(t, variant)
                );
                let ft = self.ty_of(i);
                self.set_val(
                    i,
                    V::Place(Place {
                        base: Base::Reg(v, Some(Proj { leaves, path })),
                        ty: ft,
                    }),
                );
            }
            Tag::ToArm => {
                let (_, labels) = self
                    .arms
                    .last()
                    .ok_or_else(|| format!("an arm jump outside a match (%{i})"))?;
                let l = *labels
                    .get(a as usize)
                    .ok_or("an arm the match does not have")?;
                self.flush();
                self.emit(Kind::Br { label: l });
            }
            Tag::Hook => {}
            other => return Err(format!("`{}`", other.name())),
        }
        Ok(())
    }

    fn variant_name(&self, t: Ty, k: u32) -> String {
        let name = self.name(t);
        if matches!(self.pool().get(t), TyData::Option(_)) || name.ends_with('?') {
            return ["None", "Some"]
                .get(k as usize)
                .map_or_else(|| k.to_string(), |s| (*s).to_owned());
        }
        if name.starts_with("Result[") {
            return ["Ok", "Err"]
                .get(k as usize)
                .map_or_else(|| k.to_string(), |s| (*s).to_owned());
        }
        format!("#{k}")
    }

    fn mode(&self, t: Ty) -> Mode {
        match layout::pass(self.pool(), self.env(), self.counts, t) {
            Pass::Uncounted => Mode::Plain,
            Pass::Owned(_) => Mode::Owned,
            Pass::Borrowed => Mode::Borrowed,
        }
    }

    fn call_result(&mut self, i: u32) -> R<Option<VarId>> {
        let t = self.ty_of(i);
        if self.void(t) || self.never(t) {
            return Ok(None);
        }
        let v = self.temp(t)?;
        self.set_val(i, V::Op(Operand::Var(v, None)));
        Ok(Some(v))
    }

    fn call(&mut self, i: u32, w: u32) -> R<()> {
        self.flush_for(Hazard::Memory);
        let target = self.calls.get(&i).cloned();
        let (callee, params) = match &target {
            Some(Target::Call(ct) | Target::Default { call: ct, .. }) => {
                let mut name = (self.input.path)(ct.item);
                let targs: Vec<String> = self
                    .pool()
                    .list_items(ct.args)
                    .iter()
                    .map(|t| self.name(*t))
                    .collect();
                if !targs.is_empty() {
                    name = format!("{name}[{}]", targs.join(", "));
                }
                if let TargetKind::Intrinsic(k) = &ct.kind {
                    name = format!("intrinsic {k}");
                }
                let params: Vec<Ty> = self
                    .env()
                    .params(ct.item)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| subst(self.pool(), self.env(), ct.item, ct.args, p))
                    .collect();
                (name, params)
            }
            Some(Target::ConstDefault(_)) => {
                return Err("a constant default argument".into());
            }
            _ => return Err("a call without a direct target".into()),
        };
        let rec = self.b.record(w);
        let rec: Vec<u32> = rec[..rec.len().saturating_sub(3)].to_vec();
        let mut args = Vec::new();
        for (k, r) in rec.into_iter().enumerate() {
            let t = params.get(k).copied().unwrap_or_else(|| self.ty_of(r));
            let mode = self.mode(t);
            let o = match mode {
                Mode::Owned => self.consume(r)?,
                _ => self.read(r)?,
            };
            args.push((o, mode));
        }
        let dst = self.call_result(i)?;
        self.emit(Kind::Call {
            dst,
            callee: Callee::Direct(callee),
            args: args.clone(),
        });
        for (o, m) in &args {
            if *m == Mode::Owned {
                self.consumed(o);
            }
        }
        Ok(())
    }

    /// The label a `br` to block `i` takes, when some `Break` targets it.
    fn labelled(&mut self, i: u32) -> R<Option<Target2>> {
        if !self.b.label_inst.contains(&i) {
            return Ok(None);
        }
        let t = self.ty_of(i);
        let result = if self.void(t) || self.never(t) {
            None
        } else {
            Some(self.temp(t)?)
        };
        let target = Target2 {
            brk: self.f.label(),
            cont: None,
            result,
        };
        self.targets.insert(i, target);
        Ok(Some(target))
    }

    fn if_(&mut self, i: u32, a: u32, w: u32) -> R<()> {
        let cond = self.read(a)?;
        let rec = self.b.record(w).to_vec();
        let t = self.ty_of(i);
        let result = if self.void(t) || self.never(t) {
            None
        } else {
            Some(self.temp(t)?)
        };
        let mut arms = Vec::new();
        for &blk in &rec {
            if blk == NONE {
                arms.push(Vec::new());
                continue;
            }
            let (mut stmts, tail) = self.arm(blk)?;
            if let (Some(res), Some(tl)) = (result, tail)
                && !self.never(self.ty_of(tl))
            {
                self.open();
                let o = self.consume(tl)?;
                self.emit(Kind::Assign {
                    dst: res,
                    proj: None,
                    rv: Rvalue::Use(o.clone()),
                });
                self.consumed(&o);
                self.flush();
                stmts.extend(self.close());
            }
            arms.push(stmts);
        }
        let els = if arms.len() > 1 {
            arms.pop().expect("else")
        } else {
            Vec::new()
        };
        let then = arms.pop().unwrap_or_default();
        self.emit(Kind::If { cond, then, els });
        if let Some(r) = result {
            self.set_val(i, V::Op(Operand::Var(r, None)));
        }
        Ok(())
    }

    fn match_(&mut self, i: u32, a: u32, w: u32) -> R<()> {
        let blocks = self.b.record(w).to_vec();
        let n = blocks.len() - 1;
        let t = self.ty_of(i);
        let result = if self.void(t) || self.never(t) {
            None
        } else {
            Some(self.temp(t)?)
        };
        // The scrutinee: a variable's values, or a temporary holding it.
        if let Some(V::Place(p)) = self.vals.get(&a).cloned()
            && matches!(p.base, Base::Mem(..))
        {
            self.take(a);
            self.materialize(a, &p)?;
            self.uses[a as usize] += 1;
        }
        self.flush();
        let end = self.f.label();
        let labels: Vec<Label> = (0..n).map(|_| self.f.label()).collect();
        self.arms.push((i, labels.clone()));
        self.open();
        let _ = self.block(blocks[0])?;
        self.emit(Kind::Unreachable);
        let mut cur = self.close();
        self.arms.pop();
        for (k, &blk) in blocks[1..].iter().enumerate() {
            let inner = self.f.stmt(
                i,
                Kind::Block {
                    label: labels[k],
                    body: cur,
                },
            );
            self.open();
            self.out.last_mut().expect("open").push(inner);
            let tail = self.block(blk)?;
            if let (Some(res), Some(tl)) = (result, tail)
                && !self.never(self.ty_of(tl))
            {
                let o = self.consume(tl)?;
                self.emit(Kind::Assign {
                    dst: res,
                    proj: None,
                    rv: Rvalue::Use(o.clone()),
                });
                self.consumed(&o);
            }
            self.flush();
            self.emit(Kind::Br { label: end });
            cur = self.close();
        }
        self.emit(Kind::Block {
            label: end,
            body: cur,
        });
        if let Some(r) = result {
            self.set_val(i, V::Op(Operand::Var(r, None)));
        }
        Ok(())
    }

    /// `l.f... = v` written as `LocalSet l (CopyData (LocalGet l) ...)`:
    /// the field store, when that is the `CopyData`'s only use. A nested
    /// path `l.a.b = v` is `CopyData (LocalGet l) [a = CopyData (Field
    /// (LocalGet l) a) [b = v]]`.
    fn field_write(&mut self, v: VarId, l: u32, w: u32) -> R<bool> {
        let b = self.b;
        let is_local = |k: usize| b.tags.get(k) == Some(&Tag::LocalGet) && b.data[k][0] == l;
        let mut path = Vec::new();
        let mut copies = Vec::new();
        let mut at = w;
        // The instruction whose value the current `CopyData` must copy:
        // the local, then each field of it on the path.
        let mut expect: Option<(usize, u32)> = None;
        while let Some(k) = Ref(at).as_inst() {
            let k = k.0 as usize;
            if b.tags[k] != Tag::CopyData || self.uses[k] != 1 {
                break;
            }
            let rec = b.record(b.data[k][1]);
            let [f, val] = rec[..] else { break };
            let src = b.data[k][0] as usize;
            let fits = match expect {
                None => is_local(src),
                Some((parent, fld)) => {
                    b.tags[src] == Tag::Field
                        && b.data[src][1] == fld
                        && (b.data[src][0] as usize == parent || is_local(b.data[src][0] as usize))
                }
            };
            if !fits {
                break;
            }
            path.push(f);
            copies.push(k);
            expect = Some((src, f));
            at = val;
        }
        if path.is_empty() {
            return Ok(false);
        }
        // The copies are this store; anything else that reads the local
        // reads it first.
        for k in &copies {
            let src = b.data[*k][0];
            self.take(src);
            let k = u32::try_from(*k).expect("insts");
            self.vals.insert(k, V::Op(Operand::Const("()".into())));
            self.uses[k as usize] = 0;
        }
        self.flush_for(Hazard::Var(v));
        self.write_path(v, &path, at).map(|()| true)
    }

    fn write_path(&mut self, v: VarId, path: &[u32], val: u32) -> R<()> {
        let t = self.f.vars[v as usize].clone();
        let lt =
            self.sub(self.b.local_ty[t.tir_local.ok_or("a field write into no local")? as usize]);
        let (ft, _, proj) = self.path(lt, path)?;
        if self.counted(ft) {
            // The old value takes the local's count of it.
            let old = self.temp(ft)?;
            self.emit(Kind::Assign {
                dst: old,
                proj: None,
                rv: Rvalue::Use(Operand::Var(v, Some(proj.clone()))),
            });
            self.emit(Kind::Kill {
                var: v,
                proj: Some(proj.clone()),
            });
            let o = self.consume(val)?;
            self.emit(Kind::Assign {
                dst: v,
                proj: Some(proj),
                rv: Rvalue::Use(o.clone()),
            });
            self.consumed(&o);
            self.count(Count::Release, old);
            self.emit(Kind::Kill {
                var: old,
                proj: None,
            });
        } else {
            let o = self.read(val)?;
            self.emit(Kind::Assign {
                dst: v,
                proj: Some(proj),
                rv: Rvalue::Use(o),
            });
        }
        Ok(())
    }

    /// A store into a cell local: the first makes the cell.
    fn cell_set(&mut self, v: VarId, t: Ty, w: u32) -> R<()> {
        let ty = self.name(t);
        if self.made.insert(v) {
            self.emit(Kind::Count {
                op: Count::Release,
                var: v,
                ty: format!("cell {ty}"),
            });
            self.emit(Kind::Assign {
                dst: v,
                proj: None,
                rv: Rvalue::Alloc { ty: ty.clone() },
            });
            let val = self.consume(w)?;
            self.emit(Kind::Store {
                addr: Operand::Var(v, None),
                off: 0,
                ty,
                val: val.clone(),
            });
            self.consumed(&val);
            return Ok(());
        }
        self.store(Operand::Var(v, None), 0, t, w)
    }

    fn ref_place(&mut self, i: u32) -> R<()> {
        let p = self
            .b
            .place(i as usize)
            .ok_or("a reference without its place")?;
        let path = p.path.to_vec();
        let (base, ty) = match p.kind {
            place_root::LOCAL => {
                let v = self.local(p.x)?;
                if !self.f.vars[v as usize].cell {
                    return Err("a reference to a local without a cell".into());
                }
                (
                    Operand::Var(v, None),
                    self.sub(self.b.local_ty[p.x as usize]),
                )
            }
            place_root::DEREF => {
                let v = self.local(p.x)?;
                let rt = self.sub(self.b.local_ty[p.x as usize]);
                let LKind::Ref { target, fat: false } = layout::kind(self.pool(), self.env(), rt)
                else {
                    return Err("a place through a trait value reference".into());
                };
                let o = if self.f.vars[v as usize].cell {
                    let t = self.temp(rt)?;
                    self.emit(Kind::Assign {
                        dst: t,
                        proj: None,
                        rv: Rvalue::Load {
                            addr: Operand::Var(v, None),
                            off: 0,
                            ty: self.name(rt),
                        },
                    });
                    Operand::Var(t, None)
                } else {
                    Operand::Var(v, None)
                };
                (o, target)
            }
            place_root::ELEM => {
                let lv = self.local(p.x)?;
                let iv = self.local(p.y)?;
                let lt = self.sub(self.b.local_ty[p.x as usize]);
                let (et, m) = self.elem(lt)?;
                let a = self.temp(Ty::prim(P::Usize))?;
                self.emit(Kind::Assign {
                    dst: a,
                    proj: None,
                    rv: Rvalue::IndexAddr {
                        base: Operand::Var(lv, None),
                        index: Operand::Var(iv, None),
                        size: m.size,
                        off: ELEMS,
                    },
                });
                (Operand::Var(a, None), et)
            }
            _ => return Err("a reference to a global or a data field".into()),
        };
        let (_, off, _) = self.path(ty, &path)?;
        let rt = self.ty_of(i);
        let r = self.temp(rt)?;
        self.emit(Kind::Assign {
            dst: r,
            proj: None,
            rv: if off == 0 {
                Rvalue::Use(base)
            } else {
                Rvalue::Offset { base, off }
            },
        });
        self.count(Count::Retain, r);
        self.set_val(i, V::Op(Operand::Var(r, None)));
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn intrinsic(&mut self, i: u32, a: u32, w: u32) -> R<()> {
        let op = IntrinsicOp::from_u32(a).ok_or("an unknown intrinsic")?;
        let args = self.b.record(w).to_vec();
        let list_ty = |me: &Self| me.ty_of(args[0]);
        match op {
            IntrinsicOp::ListLen => {
                let buf = self.read(args[0])?;
                self.assign(
                    i,
                    Rvalue::Load {
                        addr: buf,
                        off: LEN,
                        ty: "usize".into(),
                    },
                )?;
            }
            IntrinsicOp::ListIndex => {
                let (et, m) = self.elem(list_ty(self))?;
                let buf = self.read(args[0])?;
                let ix = self.read(args[1])?;
                let at = self.element(&buf, &ix, m.size)?;
                self.set_val(
                    i,
                    V::Place(Place {
                        base: Base::Mem(at, 0),
                        ty: et,
                    }),
                );
            }
            IntrinsicOp::ListIndexMut | IntrinsicOp::ListSet | IntrinsicOp::ListFieldSet => {
                let lt = list_ty(self);
                let (et, m) = self.elem(lt)?;
                let place = self.list_place(args[0])?;
                self.flush_change(&place);
                let ix = self.read(args[1])?;
                let buf = self.unique(&place, lt)?;
                let at = self.element(&buf, &ix, m.size)?;
                match op {
                    IntrinsicOp::ListIndexMut => self.set_val(
                        i,
                        V::Place(Place {
                            base: Base::Mem(at, 0),
                            ty: et,
                        }),
                    ),
                    IntrinsicOp::ListSet => self.store(at, 0, et, args[2])?,
                    _ => {
                        let fields: Vec<u32> = args[3..]
                            .iter()
                            .map(|r| {
                                let c = self.b.consts[(r & !Ref::CONST_BIT) as usize].1;
                                u32::try_from(c).expect("field index")
                            })
                            .collect();
                        let (ft, off, _) = self.path(et, &fields)?;
                        self.store(at, off, ft, args[2])?;
                    }
                }
            }
            IntrinsicOp::ListPush => {
                let lt = list_ty(self);
                let place = self.list_place(args[0])?;
                self.flush_change(&place);
                let et = self.elem(lt)?.0;
                let v = self.consume(args[1])?;
                let mode = if self.counted(et) {
                    Mode::Owned
                } else {
                    Mode::Plain
                };
                match place {
                    Base::Reg(var, None) => {
                        // Go's `xs = append(xs, v)`: the buffer moves through
                        // the call, which may grow or copy it.
                        self.emit(Kind::Call {
                            dst: Some(var),
                            callee: Callee::Direct(format!("std.List.append[{}]", self.name(et))),
                            args: vec![(Operand::Var(var, None), Mode::Owned), (v.clone(), mode)],
                        });
                    }
                    Base::Mem(addr, off) => {
                        let addr = self.offset(addr, off)?;
                        self.emit(Kind::Call {
                            dst: None,
                            callee: Callee::Direct(format!("std.List.push[{}]", self.name(et))),
                            args: vec![(addr, Mode::Borrowed), (v.clone(), mode)],
                        });
                    }
                    Base::Reg(..) => return Err("a push to a list field of a local".into()),
                }
                if mode == Mode::Owned {
                    self.consumed(&v);
                }
            }
            IntrinsicOp::ValueCopy | IntrinsicOp::SnapBegin => {
                // A copy shares the buffers; a change copies one first
                // when another owner holds it (`unique`).
                let o = self.read(args[0])?;
                let t = self.ty_of(i);
                let v = self.assign(i, Rvalue::Use(o))?;
                if self.counted(t) {
                    self.count(Count::Retain, v);
                }
            }
            IntrinsicOp::SnapEnd => {
                let s = self.read(args[1])?;
                if let Operand::Var(v, None) = s {
                    self.count(Count::Release, v);
                    if self.f.counted(v) {
                        self.emit(Kind::Kill { var: v, proj: None });
                    }
                }
            }
            IntrinsicOp::ListPin => {
                let lt = list_ty(self);
                let place = self.list_place(args[0])?;
                self.flush_change(&place);
                let buf = self.unique(&place, lt)?;
                if let Some(&ix) = args.get(1) {
                    let ix = self.read(ix)?;
                    let n = self.temp(Ty::prim(P::Usize))?;
                    self.emit(Kind::Assign {
                        dst: n,
                        proj: None,
                        rv: Rvalue::Load {
                            addr: buf.clone(),
                            off: LEN,
                            ty: "usize".into(),
                        },
                    });
                    self.emit(Kind::Check {
                        index: ix,
                        len: Operand::Var(n, None),
                    });
                }
                let v = self.assign(i, Rvalue::Use(buf))?;
                self.emit(Kind::Pin { var: v });
                self.count(Count::Retain, v);
            }
            IntrinsicOp::ListUnpin => {
                self.flush_for(Hazard::Memory);
                let x = self.read(args[0])?;
                let Operand::Var(v, None) = x else {
                    return Err("an unpin of no variable".into());
                };
                self.count(Count::Release, v);
                self.emit(Kind::Kill { var: v, proj: None });
            }
            IntrinsicOp::StrConcat
            | IntrinsicOp::StrEq
            | IntrinsicOp::MapGet
            | IntrinsicOp::MapLen => {
                let name = match op {
                    IntrinsicOp::StrConcat => "std.string.concat",
                    IntrinsicOp::StrEq => "std.string.eq",
                    IntrinsicOp::MapGet => "std.Map.get",
                    _ => "std.Map.len",
                };
                let mut ops = Vec::new();
                for &r in &args {
                    let o = self.read(r)?;
                    let m = if self.counted(self.ty_of(r)) {
                        Mode::Borrowed
                    } else {
                        Mode::Plain
                    };
                    ops.push((o, m));
                }
                let dst = self.call_result(i)?;
                self.emit(Kind::Call {
                    dst,
                    callee: Callee::Direct(name.into()),
                    args: ops,
                });
            }
            other => return Err(format!("the intrinsic {other:?}")),
        }
        Ok(())
    }
}

fn join(outer: Option<&Proj>, inner: &Proj) -> Proj {
    match outer {
        None => inner.clone(),
        Some(o) => Proj {
            leaves: inner.leaves.iter().map(|k| o.leaves[*k as usize]).collect(),
            path: format!("{}{}", o.path, inner.path),
        },
    }
}

fn project(o: &Operand, p: &Proj) -> Operand {
    match o {
        Operand::Var(v, outer) => Operand::Var(*v, Some(join(outer.as_ref(), p))),
        Operand::Const(c) => Operand::Const(format!("{c}{}", p.path)),
    }
}

/// How many values a walk counts.
fn refs(w: &layout::Walk) -> u32 {
    fn go(w: &layout::Walk, seen: &mut HashSet<u32>) {
        match w {
            layout::Walk::Ref(k) => {
                seen.insert(*k);
            }
            layout::Walk::Seq(ws) => ws.iter().for_each(|w| go(w, seen)),
            layout::Walk::Switch(_, arms) => arms.iter().flatten().for_each(|w| go(w, seen)),
        }
    }
    let mut seen = HashSet::default();
    go(w, &mut seen);
    u32::try_from(seen.len()).expect("refs")
}
