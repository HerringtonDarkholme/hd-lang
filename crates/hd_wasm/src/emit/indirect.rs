//! Parameters and results passed by address (#2031, steps 1 and 2;
//! rustc's `PassMode::Indirect` with `rustc_codegen_ssa`'s
//! `OperandValue::Ref`, and its `sret` pointer for a result).
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
//! A result of such a type is written into a block its caller provides,
//! whose address the worker takes last; the worker returns nothing.
//! [`Em::place_result`] plans the worker's tail: a struct or tuple
//! literal there writes each field in place, and a field that is itself
//! a call to such a worker passes it the block's address plus the field's
//! offset, so building a value nested by value costs each function one
//! address whatever its size. A caller whose call is not written in place
//! reserves the block around the call; when the call right after takes
//! the result by address it passes the block on ([`Em::kept_results`]),
//! else it loads the leaves (the *load* conversion again).
//!
//! A parameter keeps the address form only when every use of it is a
//! read of its fields ([`params`]); any other parameter stays leaves. The
//! ownership passes write no `Drop` or `LoanEnd` for a type that counts
//! nothing and holds nothing to drop. Cost: [`params`] visits the
//! callee's body once, memoized per instance; [`Em::forwarded`] and
//! [`Em::kept_results`] visit only the sub-body being emitted, and the
//! value use counts they and [`Em::place_result`] read are counted once
//! per body, shared by its instances and closures ([`Em::slot_uses`]),
//! so a body with n closures costs O(its size), not n times that.

use std::rc::Rc;

use hd_base::{HashMap, HashSet, InstId};
use hd_mono::{Sub, subst};
use hd_tir::ir::{NONE, PlaceRoot, Proj, Ref, Tag, Word, slots};

use super::shape::u32_of;
use super::{Em, Input, param_locals};
use crate::layout::{Mem, indirect};
use crate::module::{Func, Global, Module, Part, STACK, Sym, Val};
use crate::op::Op as I;

/// What an instance's worker takes or gives by address: per parameter
/// local, in order, the layout of each one that passes by address, and
/// the layout of its result when it writes the result into a block its
/// caller provides (`sret`, step 2 of #2031), whose address is then the
/// worker's last parameter and the worker returns nothing.
pub(crate) struct Abi {
    pub(crate) params: Rc<[Option<Mem>]>,
    pub(crate) ret: Option<Mem>,
}

/// An instance's worker ABI, shared.
pub(crate) type Params = Rc<Abi>;

/// The worker ABI of instance `id`, or none when no parameter and not
/// the result passes by address and the instance keeps the leaf ABI
/// alone.
pub(crate) fn params(input: &Input<'_>, m: &mut Module, id: InstId) -> Option<Params> {
    if let Some(p) = m.indirect_params.get(&id) {
        return p.clone();
    }
    let p = compute(input, id);
    m.indirect_params.insert(id, p.clone());
    p
}

fn compute(input: &Input<'_>, id: InstId) -> Option<Params> {
    let t = &input.collected.table;
    if t.sub[id.idx()] != Sub::Body(0) {
        return None;
    }
    let item = t.item[id.idx()];
    if input.env.suspends(item) {
        return None;
    }
    let b = (input.body)(id)?;
    let ret = compute_ret(input, b, id);
    let params = compute_params(input, b, id);
    if params.is_none() && ret.is_none() {
        return None;
    }
    let params = params.unwrap_or_else(|| param_locals(b).iter().map(|_| None).collect());
    Some(Rc::new(Abi { params, ret }))
}

/// The result's layout when instance `id`, a plain function, returns a
/// value [`indirect`] passes by address. A facts, handle or init body
/// keeps its result in a cell, and a coinductive flag method returns
/// its flag early: those keep the leaf result.
fn compute_ret(input: &Input<'_>, b: &hd_tir::ir::Body, id: InstId) -> Option<Mem> {
    let t = &input.collected.table;
    if b.kind != hd_tir::ir::BodyKind::Fn || input.coinductive_flags.contains(&t.key[id.idx()]) {
        return None;
    }
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
    let ret = input.env.ret(item)?;
    indirect(
        input.pool,
        input.env,
        subst(input.pool, input.env, item, args, ret),
    )
    .ok()
    .flatten()
}

fn compute_params(
    input: &Input<'_>,
    b: &hd_tir::ir::Body,
    id: InstId,
) -> Option<Rc<[Option<Mem>]>> {
    let (pool, env) = (input.pool, input.env);
    let t = &input.collected.table;
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
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

/// The leaf-ABI function of an instance whose worker `worker` has the
/// ABI `abi`: it stores each address parameter's leaves in one
/// shadow-stack block, reserves the result's block after them when the
/// worker writes its result, calls the worker and loads the result's
/// leaves back. `leaves[k]` is parameter `k`'s values and `results` the
/// result's in the leaf ABI.
pub(crate) fn wrapper(
    m: &Module,
    key: hd_base::Hash128,
    worker: &Func,
    abi: &Abi,
    leaves: &[Vec<Val>],
    results: Vec<Val>,
) -> Func {
    let mut params = Vec::new();
    // Per worker parameter: the wrapper's first parameter for it, and the
    // block offset of an address one.
    let mut plan = Vec::new();
    let mut size = 0u32;
    for (p, vs) in abi.params.iter().zip(leaves) {
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
    // The result's block follows the parameters' in the same reservation.
    let result = abi.ret.as_ref().map(|mem| {
        let off = size;
        size = (size + mem.size).next_multiple_of(8);
        (off, mem)
    });
    let rest = worker.params.len() - plan.len() - usize::from(result.is_some());
    for k in 0..rest {
        let at = u32_of(params.len());
        params.push(worker.params[plan.len() + k]);
        plan.push((at, None));
    }
    let mut f = m.func(format!("{}#leaves", worker.name), params, results);
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
    if let Some((off, _)) = result {
        address(&mut f, base, off);
    }
    f.call(Sym::Part(key, Part::Indirect));
    if let Some((off, mem)) = result {
        load_leaves(&mut f, base, off, mem);
    }
    release(&mut f, base, size);
    f
}

/// Pushes every leaf of a `mem` value stored at `base + off`.
pub(crate) fn load_leaves(f: &mut Func, base: u32, off: u32, mem: &Mem) {
    for leaf in &mem.leaves {
        f.get(base);
        f.op(leaf.load_in(STACK, off));
    }
}

/// How many times each value of body `b` is used as a value operand: one
/// visit of the body, made once per body ([`Em::slot_uses`]).
fn value_uses(b: &hd_tir::ir::Body) -> HashMap<u32, u32> {
    let mut uses: HashMap<u32, u32> = HashMap::default();
    for i in 0..b.tags.len() {
        slots(b, i, |_, v, w| {
            if matches!(w, Word::Value(_)) {
                *uses.entry(v).or_default() += 1;
            }
        });
    }
    uses
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
    /// The body's value-operand use counts ([`value_uses`]), counted by the
    /// first of its emitters that asks: its instances and closure
    /// sub-bodies share one count.
    fn slot_uses(&self) -> std::sync::Arc<HashMap<u32, u32>> {
        std::sync::Arc::clone(
            self.slot_uses
                .get_or_init(|| std::sync::Arc::new(value_uses(self.b))),
        )
    }

    /// The parameters instance `key`'s worker takes by address, for a
    /// direct call of it.
    pub(super) fn worker_of(&mut self, key: hd_base::Hash128) -> Option<Params> {
        let id = *self.m.inst_of.get(&key)?;
        params(self.input, self.m, id)
    }

    /// The field reads of an address parameter that only pass on as an
    /// address argument of a direct call to a worker: each has one use,
    /// that argument. One visit of this sub-body's instructions.
    pub(super) fn forwarded(&mut self) -> HashSet<u32> {
        let b = self.b;
        let mut candidates: Vec<u32> = Vec::new();
        if self.mem_locals.is_empty() {
            return HashSet::default();
        }
        for &i in self.rows.clone().iter() {
            let i = u32_of(i);
            if b.tags[i as usize] != Tag::Call {
                continue;
            }
            let Some(super::Target::Call(ct)) = self.calls.get(&i).cloned() else {
                continue;
            };
            if !matches!(ct.kind, hd_mono::TargetKind::Instance) || self.env().suspends(ct.item) {
                continue;
            }
            let mut args = self.rec(self.b.data[i as usize][1]);
            args.truncate(args.len().saturating_sub(3));
            let params = self.param_tys(ct.key, ct.item, ct.args);
            let Some(abi) = self.worker_abi(ct.key, args.len(), params.as_deref()) else {
                continue;
            };
            for (r, w) in args.iter().zip(abi.params.iter()) {
                if w.is_some() && self.mem_field(*r).is_some() {
                    candidates.push(*r);
                }
            }
        }
        if candidates.is_empty() {
            return HashSet::default();
        }
        let uses = self.slot_uses();
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

/// Where a direct call to a worker that writes its result puts it.
pub(super) enum Dest {
    /// Into this worker's own result block, at an offset
    /// ([`Em::place_result`]).
    Placed(u32, u32),
    /// Into a block the call reserves: its address local, its size and
    /// the result's layout.
    Block(u32, i32, Mem),
}

impl Em<'_, '_, '_> {
    /// The worker ABI of instance `key` for a direct call with `n`
    /// arguments of types `params`, when the call can use it: the
    /// arguments line up with its parameters one to one.
    pub(super) fn worker_abi(
        &mut self,
        key: hd_base::Hash128,
        n: usize,
        params: Option<&[hd_types::Ty]>,
    ) -> Option<Params> {
        self.worker_of(key)
            .filter(|abi| abi.params.len() == n && params.is_some_and(|p| p.len() == n))
    }

    /// Plans which parts of `tail`, this worker's result, are written in
    /// place into its caller's block: the tail itself when it is a struct
    /// or tuple literal or a call to a worker that writes its result, and
    /// so on through each literal's fields that have that one use. One
    /// visit of the body for use counts, then one per planned value; a
    /// work list, not native recursion, walks nested literals.
    pub(super) fn place_result(&mut self, tail: u32) -> hd_base::StageResult<()> {
        let Some(base) = self.sret.as_ref().map(|s| s.0) else {
            return Ok(());
        };
        if tail == NONE || Ref(tail).as_inst().is_none() {
            return Ok(());
        }
        let b = self.b;
        let uses = self.slot_uses();
        if uses.get(&tail).copied().unwrap_or(0) > 1 {
            return Ok(());
        }
        let mut work = vec![(tail, 0u32)];
        while let Some((r, off)) = work.pop() {
            let tag = b.tags[r as usize];
            let members = match tag {
                Tag::Call => {
                    if self.writes_result(r) {
                        self.placed.insert(r, (base, off));
                    }
                    continue;
                }
                Tag::NewStruct if !self.newtype(self.ty(r)) => self.rec(b.data[r as usize][1]),
                Tag::NewTuple if b.data[r as usize][0] == NONE => self.rec(b.data[r as usize][1]),
                _ => continue,
            };
            self.placed.insert(r, (base, off));
            let t = self.ty(r);
            for (k, &f) in members.iter().enumerate() {
                if Ref(f).as_inst().is_some() && uses.get(&f) == Some(&1) {
                    let (_, part) = self.fold_path(t, &[u32_of(k)])?;
                    work.push((f, off + part.offset));
                }
            }
        }
        Ok(())
    }

    /// Whether `Call` `r` is a direct call to a worker that writes its
    /// result, as [`Em::call_instance`] makes it.
    fn writes_result(&mut self, r: u32) -> bool {
        let Some(super::Target::Call(ct)) = self.calls.get(&r).cloned() else {
            return false;
        };
        if !matches!(ct.kind, hd_mono::TargetKind::Instance) || self.env().suspends(ct.item) {
            return false;
        }
        let mut args = self.rec(self.b.data[r as usize][1]);
        args.truncate(args.len().saturating_sub(3));
        let params = self.param_tys(ct.key, ct.item, ct.args);
        self.worker_abi(ct.key, args.len(), params.as_deref())
            .is_some_and(|abi| abi.ret.is_some())
    }

    /// Writes literal `i`, planned in place, into the result block: each
    /// member `members[k]` not itself written there stores its leaves.
    pub(super) fn write_in_place(&mut self, i: u32, members: &[u32]) -> hd_base::StageResult<()> {
        let (base, off) = self.placed[&i];
        let t = self.ty(i);
        for (k, &r) in members.iter().enumerate() {
            if self.written.contains(&r) {
                continue;
            }
            // Only a field stored here lays out, so a literal costs the
            // leaves it stores, not the whole value's.
            let (ft, part) = self.fold_path(t, &[u32_of(k)])?;
            let mem = crate::layout::mem(self.pool(), self.env(), ft)?;
            for (j, leaf) in mem.leaves.iter().enumerate() {
                self.f.get(base);
                self.comp(r, j)?;
                self.f.op(leaf.store_in(STACK, off + part.offset));
            }
        }
        self.written.insert(i);
        Ok(())
    }

    /// Stores value `r` as this worker's result, laid out as `mem`, into
    /// the block at `base`, unless it was written in place.
    pub(super) fn store_result(
        &mut self,
        r: u32,
        base: u32,
        mem: &Mem,
    ) -> hd_base::StageResult<()> {
        if self.written.contains(&r) {
            return Ok(());
        }
        for (j, leaf) in mem.leaves.iter().enumerate() {
            self.f.get(base);
            self.comp(r, j)?;
            self.f.op(leaf.store_in(STACK, 0));
        }
        Ok(())
    }

    /// The block call `i`'s result goes to: its place in this worker's
    /// result block when planned, else one it reserves.
    pub(super) fn result_dest(&mut self, i: u32, mem: &Mem) -> Dest {
        if let Some(&(base, off)) = self.placed.get(&i) {
            return Dest::Placed(base, off);
        }
        let base = self.f.local(Val::I32);
        let size = mem.size.next_multiple_of(8).cast_signed();
        reserve(&mut self.f, base, size);
        Dest::Block(base, size, mem.clone())
    }

    /// Pushes the address of `d`.
    pub(super) fn push_dest(&mut self, d: &Dest) {
        match *d {
            Dest::Placed(base, off) => address(&mut self.f, base, off),
            Dest::Block(base, ..) => address(&mut self.f, base, 0),
        }
    }

    /// After call `i` wrote its result to `d`: whether it is written in
    /// place, so the call has no values; else the result's leaves are
    /// loaded from the block it reserved, which is given back.
    pub(super) fn dest_written(&mut self, i: u32, d: Dest) -> bool {
        match d {
            Dest::Placed(..) => {
                self.written.insert(i);
                true
            }
            Dest::Block(base, size, _) if self.keep.contains(&i) => {
                self.mem_vals.insert(i, (base, 0));
                self.kept_blocks.insert(i, (base, size));
                self.written.insert(i);
                true
            }
            Dest::Block(base, size, mem) => {
                load_leaves(&mut self.f, base, 0, &mem);
                release(&mut self.f, base, size);
                false
            }
        }
    }

    /// The calls whose result stays in its block for the next call to
    /// take by address, as rustc passes a temporary's place to an
    /// indirect argument: a call to a worker that writes its result,
    /// whose one use is an address argument of the direct call to a worker
    /// that comes right after it in the same block. Nothing runs between
    /// the two, so no exit, loop edge or suspension can leave the block
    /// reserved. One visit of this sub-body's instructions, and of each
    /// of its blocks' items; the use counts are the body's, shared.
    pub(super) fn kept_results(&mut self) -> HashSet<u32> {
        let b = self.b;
        let mut kept = HashSet::default();
        for &blk in self.rows.clone().iter() {
            let blk = u32_of(blk);
            if b.tags[blk as usize] != Tag::Block {
                continue;
            }
            let items = self.items(b.data[blk as usize][0]);
            for pair in items.windows(2) {
                let (r, c) = (pair[0], pair[1]);
                if b.tags[r as usize] != Tag::Call
                    || b.tags[c as usize] != Tag::Call
                    || !self.writes_result(r)
                {
                    continue;
                }
                let Some(super::Target::Call(ct)) = self.calls.get(&c).cloned() else {
                    continue;
                };
                if !matches!(ct.kind, hd_mono::TargetKind::Instance) || self.env().suspends(ct.item)
                {
                    continue;
                }
                let mut args = self.rec(b.data[c as usize][1]);
                args.truncate(args.len().saturating_sub(3));
                let params = self.param_tys(ct.key, ct.item, ct.args);
                let Some(abi) = self.worker_abi(ct.key, args.len(), params.as_deref()) else {
                    continue;
                };
                if !args
                    .iter()
                    .zip(abi.params.iter())
                    .any(|(a, w)| *a == r && w.is_some())
                {
                    continue;
                }
                if self.slot_uses().get(&r) == Some(&1) {
                    kept.insert(r);
                }
            }
        }
        kept
    }

    /// Gives back the block of a kept result among `args`, after call `i`
    /// took it ([`Em::kept_results`]). When `i`'s own result is kept too,
    /// its block lies below that one, so the earlier block passes to `i`:
    /// giving it back later gives back both.
    pub(super) fn release_kept(&mut self, i: u32, args: &[u32]) {
        // A call has at most one: the item right before it.
        for r in args {
            if let Some((base, size)) = self.kept_blocks.remove(r) {
                if let Some(own) = self.kept_blocks.get_mut(&i) {
                    *own = (base, size);
                } else {
                    release(&mut self.f, base, size);
                }
            }
        }
    }
}
