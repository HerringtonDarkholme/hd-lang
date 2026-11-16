//! Parameters passed by address (#2031, step 1; rustc's
//! `PassMode::Indirect` with `rustc_codegen_ssa`'s `OperandValue::Ref`).
//!
//! A struct or tuple of more than [`crate::layout::INDIRECT`] values that
//! counts nothing ([`crate::layout::indirect`]) passes to an instance's
//! *worker*, `Sym::Part(key, Part::Indirect)`, as one `i32`: the address of
//! its bits in the shadow stack (memory [`STACK`]). The worker keeps the
//! parameter there, as rustc keeps an indirect argument as its place: a
//! field read loads only the leaves it reads, and a field that is itself
//! indirect and only passed on to another worker passes as the address
//! plus its offset, with nothing loaded. So forwarding a value costs one
//! address whatever its size.
//!
//! Two conversions keep every other site on the leaf form, as GHC's
//! worker/wrapper split does:
//!
//! - *spill*: a direct call whose argument is leaves stores them into a
//!   block it reserves on the shadow stack around the call;
//! - *load*: a field read that is not only passed on loads its leaves.
//!
//! The instance's own symbol, `Sym::Inst(key)`, stays the leaf ABI: a
//! wrapper that spills its parameters and calls the worker. Tables,
//! vtables, adapters, closures, drives and host rows keep calling it
//! unchanged; final linking drops it when only direct calls remain.
//!
//! A parameter keeps the address form only when every use of it is a
//! read of its fields ([`params`]); any other parameter stays leaves. The
//! ownership passes write no `Drop` or `LoanEnd` for a type that counts
//! nothing and holds nothing to drop. Cost: [`params`] visits the callee's body once, memoized per
//! instance; [`Em::forwarded`] visits a worker's body once, and no other.

use std::rc::Rc;

use hd_base::{HashMap, HashSet, InstId};
use hd_mono::{Sub, subst};
use hd_tir::ir::{PlaceRoot, Proj, Tag, Word, slots};

use super::shape::u32_of;
use super::{Em, Input, param_locals};
use crate::layout::{Mem, indirect};
use crate::module::{Func, Global, Module, Part, STACK, Sym, Val};
use crate::op::Op as I;

/// Per parameter local, in order, the layout of each one that passes by
/// address.
pub(crate) type Params = Rc<[Option<Mem>]>;

/// The parameters of instance `id` that pass by address, or none when no
/// parameter does and the instance keeps the leaf ABI alone.
pub(crate) fn params(input: &Input<'_>, m: &mut Module, id: InstId) -> Option<Params> {
    if let Some(p) = m.indirect_params.get(&id) {
        return p.clone();
    }
    let p = compute(input, id);
    m.indirect_params.insert(id, p.clone());
    p
}

fn compute(input: &Input<'_>, id: InstId) -> Option<Params> {
    let (pool, env) = (input.pool, input.env);
    let t = &input.collected.table;
    if t.sub[id.idx()] != Sub::Body(0) {
        return None;
    }
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
    if env.suspends(item) {
        return None;
    }
    let b = (input.body)(id)?;
    let locals = param_locals(b);
    let mut found: HashMap<usize, Mem> = HashMap::default();
    for &l in &locals {
        let ty = subst(pool, env, item, args, b.local_ty[l]);
        if let Ok(Some(m)) = indirect(pool, env, ty) {
            found.insert(l, m);
        }
    }
    if found.is_empty() {
        return None;
    }
    for c in &b.cap_local {
        found.remove(&c.idx());
    }
    for i in 0..b.tags.len() {
        if found.is_empty() {
            return None;
        }
        let tag = b.tags[i];
        if !matches!(
            tag,
            Tag::Read
                | Tag::Store
                | Tag::Ref
                | Tag::Drop
                | Tag::IsUnique
                | Tag::FlagSet
                | Tag::FlagClear
                | Tag::LoanEnd
        ) {
            continue;
        }
        let Some(p) = b.decode_place(b.data[i][0]) else {
            continue;
        };
        let PlaceRoot::Local(l) = p.root else {
            continue;
        };
        // A whole read takes other paths (`same_value`, moves into
        // locals), which read the parameter's locals: it stays leaves.
        let fields = !p.proj.is_empty() && p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
        if !(tag == Tag::Read && fields) {
            found.remove(&l.idx());
        }
    }
    if found.is_empty() {
        return None;
    }
    Some(locals.iter().map(|l| found.remove(l)).collect())
}

/// The leaf-ABI function of an instance whose worker `worker` takes the
/// parameters `ps` by address: it stores each such parameter's leaves in
/// one shadow-stack block and calls the worker. `leaves[k]` is parameter
/// `k`'s values in the leaf ABI.
pub(crate) fn wrapper(
    m: &Module,
    key: hd_base::Hash128,
    worker: &Func,
    ps: &[Option<Mem>],
    leaves: &[Vec<Val>],
) -> Func {
    let mut params = Vec::new();
    // Per worker parameter: the wrapper's first parameter for it, and the
    // block offset of an address one.
    let mut plan = Vec::new();
    let mut size = 0u32;
    for (p, vs) in ps.iter().zip(leaves) {
        let at = u32_of(params.len());
        params.extend(vs.iter().copied());
        match p {
            Some(mem) => {
                let off = size;
                size = (size + mem.size).next_multiple_of(8);
                plan.push((at, Some((off, mem))));
            }
            None => {
                for k in 0..vs.len() {
                    plan.push((at + u32_of(k), None));
                }
            }
        }
    }
    let rest = worker.params.len() - plan.len();
    for k in 0..rest {
        let at = u32_of(params.len());
        params.push(worker.params[plan.len() + k]);
        plan.push((at, None));
    }
    let mut f = m.func(
        format!("{}#leaves", worker.name),
        params,
        worker.results.clone(),
    );
    let base = f.local(Val::I32);
    let size = size.cast_signed();
    reserve(&mut f, base, size);
    for &(at, addr) in &plan {
        if let Some((off, mem)) = addr {
            for (k, leaf) in mem.leaves.iter().enumerate() {
                f.get(base);
                f.get(at + u32_of(k));
                f.op(leaf.store_in(STACK, off));
            }
        }
    }
    for &(at, addr) in &plan {
        match addr {
            Some((off, _)) => address(&mut f, base, off),
            None => f.get(at),
        }
    }
    f.call(Sym::Part(key, Part::Indirect));
    release(&mut f, base, size);
    f
}

/// Reserves `size` bytes of the shadow stack, its address in `base`.
pub(crate) fn reserve(f: &mut Func, base: u32, size: i32) {
    f.global_get(Global::Stack);
    f.op(I::I32Const(size));
    f.op(I::I32Sub);
    f.op(I::LocalTee(base));
    f.global_set(Global::Stack);
}

/// Gives back what [`reserve`] took.
pub(crate) fn release(f: &mut Func, base: u32, size: i32) {
    f.get(base);
    f.op(I::I32Const(size));
    f.op(I::I32Add);
    f.global_set(Global::Stack);
}

/// Pushes `base + off`.
fn address(f: &mut Func, base: u32, off: u32) {
    f.get(base);
    if off != 0 {
        f.op(I::I32Const(off.cast_signed()));
        f.op(I::I32Add);
    }
}

impl Em<'_, '_, '_> {
    /// The parameters instance `key`'s worker takes by address, for a
    /// direct call of it.
    pub(super) fn worker_of(&mut self, key: hd_base::Hash128) -> Option<Params> {
        let id = *self.m.inst_of.get(&key)?;
        params(self.input, self.m, id)
    }

    /// The field reads of an address parameter that only pass on as an
    /// address argument of a direct call to a worker: each has one use,
    /// that argument. One visit of the body.
    pub(super) fn forwarded(&mut self) -> HashSet<u32> {
        let b = self.b;
        let mut candidates: Vec<u32> = Vec::new();
        if self.mem_locals.is_empty() {
            return HashSet::default();
        }
        for i in 0..u32_of(b.tags.len()) {
            if b.tags[i as usize] != Tag::Call {
                continue;
            }
            let Some(super::Target::Call(ct)) = self.calls.get(&i).cloned() else {
                continue;
            };
            if !matches!(ct.kind, hd_mono::TargetKind::Instance) || self.env().suspends(ct.item) {
                continue;
            }
            let Some(ws) = self.worker_of(ct.key) else {
                continue;
            };
            let mut args = self.rec(self.b.data[i as usize][1]);
            args.truncate(args.len().saturating_sub(3));
            let params = self.param_tys(ct.key, ct.item, ct.args);
            if args.len() != ws.len() || params.is_none_or(|p| p.len() != args.len()) {
                continue;
            }
            for (r, w) in args.iter().zip(ws.iter()) {
                if w.is_some() && self.mem_field(*r).is_some() {
                    candidates.push(*r);
                }
            }
        }
        if candidates.is_empty() {
            return HashSet::default();
        }
        let mut uses: HashMap<u32, u32> = HashMap::default();
        for i in 0..b.tags.len() {
            slots(b, i, |_, v, w| {
                if matches!(w, Word::Value(_)) {
                    *uses.entry(v).or_default() += 1;
                }
            });
        }
        candidates
            .into_iter()
            .filter(|r| uses.get(r) == Some(&1))
            .collect()
    }

    /// For a `Read` `r` of fields of an address parameter: the parameter's
    /// address local, its layout and the field path.
    fn mem_field(&self, r: u32) -> Option<(u32, Mem, Vec<u32>, usize)> {
        if self.b.tags.get(r as usize) != Some(&Tag::Read) {
            return None;
        }
        let p = self.b.decode_place(self.b.data[r as usize][0])?;
        let PlaceRoot::Local(l) = p.root else {
            return None;
        };
        let (at, mem) = self.mem_locals.get(&l.idx())?.clone();
        let fields = p
            .proj
            .iter()
            .map(|s| match s {
                Proj::Field(f) => Some(*f),
                _ => None,
            })
            .collect::<Option<Vec<u32>>>()?;
        Some((at, mem, fields, l.idx()))
    }

    /// Emits `Read` `i` when it reads fields of an address parameter: the
    /// address of the field when it only passes on ([`Em::forwarded`]),
    /// else the field's leaves loaded. Whether it did.
    pub(super) fn mem_read(&mut self, i: u32) -> hd_base::StageResult<bool> {
        let Some((at, mem, fields, l)) = self.mem_field(i) else {
            return Ok(false);
        };
        let declared = self.sub(self.b.local_ty[l]);
        let (_, part) = self.fold_path(declared, &fields)?;
        if self.forward.contains(&i) {
            self.mem_vals.insert(i, (at, part.offset));
            return Ok(true);
        }
        for leaf in &mem.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load_in(STACK, 0));
        }
        self.store(i)?;
        Ok(true)
    }

    /// Pushes the arguments of direct call `i` to a worker taking `ws` by
    /// address: an address argument's field address, or its leaves stored
    /// in a block of the shadow stack. The block, to give back after the
    /// call.
    pub(super) fn push_worker_args(
        &mut self,
        i: u32,
        args: &[u32],
        params: &[hd_types::Ty],
        lends: u64,
        ws: &[Option<Mem>],
    ) -> hd_base::StageResult<Option<(u32, i32)>> {
        let mut size = 0u32;
        let mut offs = Vec::with_capacity(args.len());
        for (r, w) in args.iter().zip(ws) {
            match w {
                Some(mem) if !self.mem_vals.contains_key(r) => {
                    offs.push(size);
                    size = (size + mem.size).next_multiple_of(8);
                }
                _ => offs.push(0),
            }
        }
        let block = (size != 0).then(|| {
            let base = self.f.local(Val::I32);
            reserve(&mut self.f, base, size.cast_signed());
            (base, size.cast_signed())
        });
        for (k, (&r, w)) in args.iter().zip(ws).enumerate() {
            // An argument that spills has a nonempty block: a struct of
            // more than 16 values has a nonzero size.
            match (w, self.mem_vals.get(&r).copied(), block) {
                (Some(_), Some((at, off)), _) => address(&mut self.f, at, off),
                (Some(mem), None, Some((base, _))) => {
                    for (j, leaf) in mem.leaves.iter().enumerate() {
                        self.f.get(base);
                        self.comp(r, j)?;
                        self.f.op(leaf.store_in(STACK, offs[k]));
                    }
                    address(&mut self.f, base, offs[k]);
                }
                _ => self.push_arg(i, k, r, params[k], Some(lends))?,
            }
        }
        Ok(block)
    }

    /// Gives back a block [`Em::push_worker_args`] reserved.
    pub(super) fn release_block(&mut self, block: Option<(u32, i32)>) {
        if let Some((base, size)) = block {
            release(&mut self.f, base, size);
        }
    }
}
