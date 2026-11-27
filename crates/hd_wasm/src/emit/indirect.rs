//! Parameters and results passed by address (#2031, steps 1 to 4;
//! rustc's `PassMode::Indirect` with `rustc_codegen_ssa`'s
//! `OperandValue::Ref`, and its `sret` pointer for a result).
//!
//! A struct, tuple or enum of more than [`crate::layout::INDIRECT`] values
//! ([`crate::layout::by_address`]) passes to an instance's
//! *worker*, `Sym::Part(key, Part::Indirect)`, as one `i32`: the address of
//! its bits in the shadow stack (memory [`HEAP`]). The worker keeps the
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
//! - *load*: a read, whole or of a field, that is neither passed on nor
//!   copied into a block loads its leaves.
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
//! A local of such a type kept in memory (step 3, rustc's
//! `LocalRef::Place`) lives in a slot of one *frame block* on the shadow
//! stack, reserved at the function's entry and given back at every exit
//! ([`Em::frame_locals`], [`crate::wide::locals_frame`]). Its store writes a literal
//! or a worker's result in place, copies a value read from memory with
//! one `memory.copy`, or stores leaves; a read of it, whole or of fields,
//! is an address like a parameter's.
//!
//! A parameter keeps the address form when every use of it is a read,
//! whole or of its fields ([`params`]): a whole read that is the result
//! or a worker's argument is one `memory.copy` or the address itself
//! (rustc's `OperandValue::Ref`, Go's `Move`). A parameter stored to,
//! borrowed or captured stays leaves. The ownership passes write no
//! `Drop` or `LoanEnd` for a type that counts nothing and holds nothing
//! to drop.
//!
//! A parameter whose type holds counted references (step 4,
//! [`crate::layout::by_address`]) passes by address too when the instance
//! owns it: its counts move with its bits, and its `Drop` or exit release
//! counts it from the address: the open drop over its fields
//! (`hd_layout::shims`, Swift's value-witness `destroy`), skipping a field
//! a move took out under that field's flag (rustc's drop elaboration; the
//! flags are TIR's, `hd_tir::own::drop_flags`, read by
//! [`Em::mem_release`]). A result of such
//! a type is written into its caller's block, and a frame local of one
//! kept in memory is released from its slot while its whole path's flag
//! is set (step 4b); a whole move of it clears the flag, and a whole copy to a
//! callee that owns it is one `memory.copy` into the call's block, whose
//! counts the type's retain glue takes from the address
//! (`crate::rt::Helper::RetainFields`, Swift's `initializeWithCopy`).
//!
//! A type that holds a `Drop` hook is released the same way (#2031,
//! [`Em::released`]): its in-place release shim runs each hook on the
//! value where it lives, and the open drop skips a hook a moved field
//! rules out under that field's flag. Cost:
//! [`params`] visits the
//! callee's body once, memoized per instance; [`Em::forwarded`] and
//! [`Em::kept_results`] visit only the sub-body being emitted, and the
//! value use counts they and [`Em::place_result`] read are counted once
//! per body, shared by its instances and closures ([`Em::slot_uses`]),
//! so a body with n closures costs O(its size), not n times that.

mod constant;

use std::rc::Rc;
use std::sync::Arc;

use hd_base::{HashMap, HashSet, InstId, SliceSort as _};
use hd_mono::{Sub, subst};
use hd_tir::ir::{NONE, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};

use super::shape::u32_of;
use super::{Em, Input, param_locals};
use crate::layout::{LeafOps as _, Mem, by_address};
use crate::module::{Func, Global, HEAP, Module, Part, Sym, Val};
use crate::op::Op as I;

/// What an instance's worker takes or gives by address: per parameter
/// local, in order, the layout of each one that passes by address, and
/// the layout of its result when it writes the result into a block its
/// caller provides (`sret`, step 2 of #2031), whose address is then the
/// worker's last parameter and the worker returns nothing.
pub(crate) struct Abi {
    pub(crate) params: Arc<[Option<Mem>]>,
    pub(crate) ret: Option<Mem>,
}

/// An instance's worker ABI, shared.
pub(crate) type Params = Arc<Abi>;

/// The worker ABI of instance `id`, or none when no parameter and not
/// the result passes by address and the instance keeps the leaf ABI
/// alone: [`prepare`]'s, for an instance the link emits.
pub(crate) fn params(input: &Input<'_>, m: &Module, id: InstId) -> Option<Params> {
    match m.indirect_params.get(&id) {
        Some(p) => p.clone(),
        None => compute(input, id),
    }
}

/// Each instance's worker ABI, which its emission and its callers' read,
/// computed on the task's threads before any is emitted. Cost: one
/// computation per instance, as each instance's emission made before.
pub(crate) fn prepare(input: &Input<'_>, m: &mut Module) {
    let all = hd_sched::par_map(input.order, |&id| (id, compute(input, id)));
    m.facts_mut().indirect_params = all.into_iter().collect();
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
    let params = compute_params(input, b);
    if params.is_none() && ret.is_none() {
        return None;
    }
    let params = params.unwrap_or_else(|| param_locals(b).iter().map(|_| None).collect());
    Some(Arc::new(Abi { params, ret }))
}

/// The result's layout when instance `id`, a plain function, returns a
/// value [`by_address`] keeps. A facts, handle or init body
/// keeps its result in a cell, and a coinductive flag method returns
/// its flag early: those keep the leaf result.
fn compute_ret(input: &Input<'_>, b: &hd_tir::ir::Body, id: InstId) -> Option<Mem> {
    let t = &input.collected.table;
    if b.kind != hd_tir::ir::BodyKind::Fn || input.coinductive_flags.contains(&t.key[id.idx()]) {
        return None;
    }
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
    let ret = subst(input.pool, input.env, item, args, input.env.ret(item)?);
    if !by_address(input.pool, input.env, ret).ok()? {
        return None;
    }
    crate::layout::mem(input.pool, input.env, ret).ok()
}

fn compute_params(input: &Input<'_>, b: &hd_tir::ir::Body) -> Option<Arc<[Option<Mem>]>> {
    let (pool, env) = (input.pool, input.env);
    let locals = param_locals(b);
    // A counted parameter passes by address whether the callee owns it
    // or not (rustc's `PassMode::Indirect` for any by-value argument
    // over two pointers). An owned one's count arrives with its bits, and
    // its drop releases them from the address (`Em::mem_release`); a lent
    // one (`layout::Pass::Borrowed`) is the caller's place, which neither
    // side counts, and its drops stand under `ParamOwned`, false for it.
    // Leaves would cost each call O(the value's size) (#2399).
    let mut found: HashMap<usize, Mem> = HashMap::default();
    let mut counted: HashMap<usize, hd_types::Ty> = HashMap::default();
    for &l in &locals {
        let ty = b.local_ty[l];
        if matches!(by_address(pool, env, ty), Ok(true))
            && let Ok(m) = crate::layout::mem(pool, env, ty)
        {
            if crate::layout::tracked(pool, env, ty) || crate::layout::holds_drop(pool, env, ty) {
                counted.insert(l, ty);
            }
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
            Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::LoanEnd
        ) {
            continue;
        }
        let Some(p) = b.decode_place(b.data[i][0]) else {
            continue;
        };
        let PlaceRoot::Local(l) = p.root else {
            continue;
        };
        // A read, whole or of fields, is the address plus the path's
        // offset (`Em::mem_read`): a whole one passes on as the address
        // or is copied with one `memory.copy`.
        let fields = p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
        let kept = match (tag, counted.get(&l.idx())) {
            (Tag::Read, None) => fields,
            // Its drops, whole or of fields (`hd_tir::own::elaborate`),
            // release from the address.
            (Tag::Drop, Some(_)) => fields,
            // A move out of a counted one is skipped by its drop: a whole
            // move clears its TIR flag, and a field move is elaborated
            // only through fields passed by address, so every proper
            // prefix of the path must be one.
            (Tag::Read, Some(&ty)) if b.data[i][1] == ReadMode::Move as u32 => {
                fields && moves_through_addresses(pool, env, ty, &p.proj)
            }
            (Tag::Read, Some(_)) => fields,
            _ => false,
        };
        if !kept {
            found.remove(&l.idx());
        }
    }
    if found.is_empty() {
        return None;
    }
    Some(locals.iter().map(|l| found.remove(l)).collect())
}

/// Whether each proper prefix of field path `proj` into a value of `ty`
/// names a field passed by address ([`by_address`]), so a drop that skips
/// the moved field elaborates only address fields' plans. O(the path).
fn moves_through_addresses(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    ty: hd_types::Ty,
    proj: &[Proj],
) -> bool {
    let mut t = ty;
    for (n, s) in proj.iter().enumerate() {
        let Proj::Field(f) = s else {
            return false;
        };
        if n + 1 == proj.len() {
            return true;
        }
        let Ok(ms) = crate::layout::members(pool, env, t) else {
            return false;
        };
        let Some(&u) = ms.tys.get(*f as usize) else {
            return false;
        };
        if !matches!(by_address(pool, env, u), Ok(true)) {
            return false;
        }
        t = u;
    }
    true
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
    debug_assert_eq!(
        abi.params.len(),
        leaves.len(),
        "ABI logical parameter groups"
    );
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
    // Freeze the worker suffix before appending to plan: plan.len() grows
    // once per forwarded value and cannot also be the suffix's base index.
    let end = worker.params.len() - usize::from(result.is_some());
    for &value in &worker.params[plan.len()..end] {
        let at = u32_of(params.len());
        params.push(value);
        plan.push((at, None));
    }
    debug_assert_eq!(
        plan.len() + usize::from(result.is_some()),
        worker.params.len()
    );
    let mut f = m.func(format!("{}#leaves", worker.name), params, results);
    let base = f.local(Val::I32);
    let size = size.cast_signed();
    reserve(&mut f, base, size);
    for &(at, addr) in &plan {
        if let Some((off, mem)) = addr {
            for (k, leaf) in mem.leaves.iter().enumerate() {
                f.get(base);
                f.get(at + u32_of(k));
                f.op(leaf.store_in(HEAP, off));
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
        f.op(leaf.load_in(HEAP, off));
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
    let (code, _) = crate::wide::frame_enter(f, base, size.cast_unsigned());
    for ins in code {
        f.push(ins);
    }
}

/// Gives back what [`reserve`] took.
pub(crate) fn release(f: &mut Func, base: u32, size: i32) {
    f.get(base);
    f.op(I::I32Const(size));
    f.op(I::I32Add);
    f.global_set(Global::Stack);
}

/// Pushes `base + off`.
pub(super) fn address(f: &mut Func, base: u32, off: u32) {
    f.get(base);
    if off != 0 {
        f.op(I::I32Const(off.cast_signed()));
        f.op(I::I32Add);
    }
}

impl Em<'_, '_, '_> {
    /// The members of struct or tuple type `t`, a literal planned in
    /// place ([`Em::plan_in_place`]): one probe of the link's cache.
    fn member_parts(&self, t: hd_types::Ty) -> hd_base::StageResult<Rc<crate::layout::Members>> {
        crate::layout::members(self.pool(), self.env(), t)
    }

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
        if self.mem_locals.is_empty() {
            return HashSet::default();
        }
        // A read of memory stored whole into a local kept in memory is
        // copied there with one `memory.copy`.
        let mut candidates: Vec<u32> = std::mem::take(&mut self.frame_copies)
            .into_iter()
            .filter(|&v| self.mem_field(v).is_some())
            .collect();
        let passed = self.worker_reads();
        candidates.extend(passed.into_iter().filter(|&r| self.mem_field(r).is_some()));
        if self.sret.is_some() {
            candidates.extend(self.rows.iter().filter_map(|&i| {
                let r = self.b.data[i][0];
                (self.b.tags[i] == Tag::Return && self.mem_field(r).is_some()).then_some(r)
            }));
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

    /// The reads of a local this sub-body's direct calls pass to a worker
    /// parameter taken by address, that can pass as an address: a move,
    /// a read of a type that counts nothing, or a copy to a callee that
    /// owns it (copied into the call's block, where the retain shim takes
    /// its counts from the address, [`Em::push_worker_args`]). One visit
    /// of the sub-body's rows and each call's arguments.
    fn worker_reads(&mut self) -> Vec<u32> {
        let b = self.b;
        let mut out = Vec::new();
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
            let lends = self.env().lends(ct.item, ct.args)
                | super::promote::promoted_params(self.input, self.m, ct.key);
            for (k, (&r, w)) in args.iter().zip(abi.params.iter()).enumerate() {
                if w.is_none()
                    || b.tags.get(r as usize) != Some(&Tag::Read)
                    || !b
                        .decode_place(b.data[r as usize][0])
                        .is_some_and(|p| matches!(p.root, PlaceRoot::Local(_)))
                {
                    continue;
                }
                let mode = b.data[r as usize][1];
                let t = self.ty_of(r);
                let pass = self.argument_pass(Some(lends), k, t);
                let owned_copy =
                    mode == ReadMode::Copy as u32 && matches!(pass, crate::layout::Pass::Owned);
                // A lent argument is the caller's place: its address.
                let lent =
                    mode != ReadMode::Move as u32 && matches!(pass, crate::layout::Pass::Borrowed);
                if mode == ReadMode::Move as u32 || !self.cx().tracked(t) || owned_copy || lent {
                    out.push(r);
                }
            }
        }
        out
    }

    /// For a `Read` `r` of a local kept in memory, whole or of fields: the
    /// local's address local and offset, its layout and the field path.
    pub(super) fn mem_field(&self, r: u32) -> Option<MemField> {
        if self.b.tags.get(r as usize) != Some(&Tag::Read) {
            return None;
        }
        let p = self.b.decode_place(self.b.data[r as usize][0])?;
        let PlaceRoot::Local(l) = p.root else {
            return None;
        };
        let (at, off, mem) = self.mem_locals.get(&l.idx())?.clone();
        let fields = p
            .proj
            .iter()
            .map(|s| match s {
                Proj::Field(f) => Some(*f),
                _ => None,
            })
            .collect::<Option<Vec<u32>>>()?;
        Some((at, off, mem, fields, l.idx()))
    }

    /// Emits `Read` `i` when it reads a local kept in memory: the address
    /// of what it reads when that only passes on ([`Em::forwarded`]), else
    /// the leaves loaded. Whether it did.
    pub(super) fn mem_read(&mut self, i: u32) -> hd_base::StageResult<bool> {
        let Some((at, off, mem, fields, l)) = self.mem_field(i) else {
            return Ok(false);
        };
        let declared = self.b.local_ty[l];
        let (_, part) = self.fold_path(declared, &fields)?;
        if self.forward.contains(&i) {
            self.mem_vals.insert(i, (at, off + part.offset));
            return Ok(true);
        }
        for leaf in &mem.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load_in(HEAP, off));
        }
        self.store(i)?;
        Ok(true)
    }

    /// The stated `Retain` of copy read `value` that only passes its
    /// address on ([`Em::forwarded`]): one count more on each counted
    /// reference at that address, as Swift lowers `copy_addr`. Whether
    /// `value` is one.
    pub(super) fn mem_retain(&mut self, value: u32) -> hd_base::StageResult<bool> {
        let Some(&(at, off)) = self.mem_vals.get(&value) else {
            return Ok(false);
        };
        let t = self.ty_of(value);
        let counted = crate::rt::counted_of(self.input, self.m, t)?;
        if counted.counts() {
            // The type's retain glue on the address, one helper per layout.
            let mem = crate::layout::mem(self.input.pool, self.input.env, t)?;
            address(&mut self.f, at, off);
            self.f.helper(crate::rt::Helper::RetainFields {
                leaves: mem.leaves,
                parts: vec![(0, counted)],
            });
        }
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
        // A counted copy read from memory ([`Em::forwarded`]) is copied
        // into the block, then takes its counts there.
        let copies: Vec<bool> = args
            .iter()
            .zip(params)
            .map(|(&r, &t)| {
                self.mem_vals.contains_key(&r)
                    && self.b.data[r as usize][1] == ReadMode::Copy as u32
                    && self.cx().tracked(t)
            })
            .collect();
        let mut size = 0u32;
        let mut offs = Vec::with_capacity(args.len());
        for (k, (r, w)) in args.iter().zip(ws).enumerate() {
            match w {
                Some(mem) if !self.mem_vals.contains_key(r) || copies[k] => {
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
                // The copy's count is its stated `Retain` (`Em::own_retain`).
                (Some(mem), Some((at, off)), Some((base, _))) if copies[k] => {
                    self.copy_in(base, offs[k], at, off, mem.size);
                    address(&mut self.f, base, offs[k]);
                }
                (Some(_), Some((at, off)), _) => address(&mut self.f, at, off),
                (Some(mem), None, Some((base, _))) => {
                    // An owned counted argument moves in or takes counts
                    // of its own, as a leaf argument does (`Em::push_arg`).
                    let src = if self.cx().tracked(params[k]) {
                        self.arg_src(i, k, r, params[k], Some(lends))?
                    } else {
                        None
                    };
                    let src = src.unwrap_or(super::value::Src::Ref(r));
                    self.store_src(HEAP, base, offs[k], &mem.leaves, &src)?;
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
        let uses = self.slot_uses();
        if uses.get(&tail).copied().unwrap_or(0) > 1 {
            return Ok(());
        }
        self.plan_in_place(tail, base, 0, &uses)
    }

    /// Plans `root`, headed for the block at `base` plus `off`, written in
    /// place: a call to a worker that writes its result gets that address,
    /// and a struct or tuple literal writes each field there, and so on
    /// through each literal's fields with one use. A work list, not native
    /// recursion, walks nested literals; each planned value is visited
    /// once.
    fn plan_in_place(
        &mut self,
        root: u32,
        base: u32,
        off: u32,
        uses: &HashMap<u32, u32>,
    ) -> hd_base::StageResult<()> {
        let b = self.b;
        let mut work = vec![(root, off)];
        while let Some((r, off)) = work.pop() {
            if matches!(b.tags[r as usize], Tag::NewStruct | Tag::NewTuple)
                && by_address(self.pool(), self.env(), self.ty(r))?
                && constant::allocation(self, r)?.is_some()
            {
                self.placed.insert(r, (base, off));
                continue;
            }
            let tag = b.tags[r as usize];
            let members = match tag {
                // An inlined call's one-exit wrapper preserves its value.
                // The shared census proves no other Break can reach it.
                Tag::Scope => {
                    if let Some(value) = b.wrapper_value(r as usize) {
                        let block = b.data[r as usize][0];
                        let label = self.topo.label_of[block as usize];
                        if self.break_counts.get(label as usize) == Some(&1)
                            && uses.get(&value) == Some(&1)
                        {
                            self.placed.insert(r, (base, off));
                            self.placed.insert(block, (base, off));
                            // The block tail still writes parameters and constants
                            // through `placed_tail`; only instructions have more
                            // construction to plan at this destination.
                            work.extend(Ref(value).as_inst().map(|i| (i.0, off)));
                        }
                    }
                    continue;
                }
                // A read of memory headed here is one `memory.copy`
                // ([`Em::forwarded`]).
                Tag::Read => {
                    self.frame_copies.push(r);
                    continue;
                }
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
            // Only a member that can itself be written in place needs its
            // offset: the type's members are read once, if at all.
            let mut parts = None;
            for (k, &f) in members.iter().enumerate() {
                if Ref(f).as_inst().is_none() || uses.get(&f) != Some(&1) {
                    continue;
                }
                // Its offset is read where the literal is written.
                if b.tags[f as usize] == Tag::Read {
                    self.frame_copies.push(f);
                    continue;
                }
                if !matches!(
                    b.tags[f as usize],
                    Tag::Call | Tag::NewStruct | Tag::NewTuple | Tag::Scope
                ) {
                    continue;
                }
                let parts = match &parts {
                    Some(p) => p,
                    None => parts.insert(self.member_parts(self.ty(r))?),
                };
                work.push((f, off + parts.parts[k].offset));
            }
        }
        Ok(())
    }

    /// A block tail headed for an existing destination writes there rather
    /// than materializing the wrapper's locals. The one-use proof preserves
    /// the old tail's move/copy decision and cleanup order.
    pub(super) fn placed_tail(
        &mut self,
        r: u32,
        dest: u32,
        user: u32,
    ) -> hd_base::StageResult<bool> {
        let Some(&(base, off)) = self.placed.get(&dest) else {
            return Ok(false);
        };
        if !self.written.contains(&r) {
            let mem = crate::layout::mem(self.pool(), self.env(), self.ty(dest))?;
            if let Some(&(at, from)) = self.mem_vals.get(&r) {
                self.copy_in(base, off, at, from, mem.size);
            } else {
                // A tail that is not moved has its stated `Retain`.
                let moved = self.take_from(r, user);
                self.store_src(HEAP, base, off, &mem.leaves, &super::value::Src::Ref(r))?;
                if moved {
                    self.settle_value(r);
                }
            }
        }
        self.written.insert(dest);
        Ok(true)
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
        if self.write_constant(i, base, off)? {
            return Ok(());
        }
        let t = self.ty(i);
        let borrowed = self.b.is_borrowed_coercion(i);
        let mut parts = None;
        for (k, &r) in members.iter().enumerate() {
            if self.written.contains(&r) {
                continue;
            }
            // Only a field stored here lays out, so a literal costs the
            // leaves it stores, not the whole value's; the type's members
            // are read once.
            let parts = match &parts {
                Some(p) => p,
                None => parts.insert(self.member_parts(t)?),
            };
            let (ft, part) = (parts.tys[k], parts.parts[k]);
            let mem = crate::layout::mem(self.pool(), self.env(), ft)?;
            if let Some(&(at, from)) = self.mem_vals.get(&r) {
                self.copy_in(base, off + part.offset, at, from, mem.size);
                continue;
            }
            // Construction consumes its parts (`Em::new_data`): a counted
            // one moves in or takes counts of its own.
            let src = if borrowed {
                super::value::Src::Ref(r)
            } else {
                self.retain_src(&super::value::Src::Ref(r), ft)?
            };
            self.store_src(HEAP, base, off + part.offset, &mem.leaves, &src)?;
        }
        self.written.insert(i);
        Ok(())
    }

    /// Copies a literal once at its existing Memory boundary.
    fn write_constant(&mut self, r: u32, base: u32, off: u32) -> hd_base::StageResult<bool> {
        let layout = crate::layout::layout(self.pool(), self.env(), self.ty(r))?;
        if matches!(layout.repr, crate::layout::Repr::Memory)
            && let Some(at) = constant::allocation(self, r)?
        {
            address(&mut self.f, base, off);
            self.f.static_addr(at);
            self.f.u32(layout.size);
            self.f.push(crate::bulk::copy(crate::module::HEAP, HEAP));
            self.written.insert(r);
            return Ok(true);
        }
        Ok(false)
    }

    /// Stores value `r` as this worker's result, laid out as `mem`, into
    /// the block at `base`, unless it was written in place.
    /// A counted result leaves with its count, as the leaf ABI's does:
    /// the tail (`ret` none) moves out to the root, a `return` (`ret` is
    /// whether it is borrowed) moves out at its only use or takes a count
    /// for the caller.
    pub(super) fn store_result(
        &mut self,
        r: u32,
        base: u32,
        mem: &Mem,
        ret: Option<bool>,
    ) -> hd_base::StageResult<()> {
        if self.written.contains(&r) || self.write_constant(r, base, 0)? {
            return Ok(());
        }
        // A copy's count is its stated `Retain` (`Em::own_retain`).
        if let Some(&(at, from)) = self.mem_vals.get(&r) {
            self.copy_in(base, 0, at, from, mem.size);
            return Ok(());
        }
        let counted = self.counting && self.cx().tracked(self.ty_of(r));
        let (src, moved) = match ret {
            _ if !counted => (super::value::Src::Ref(r), false),
            None => {
                let root = self.b.sub_root[self.sub_id as usize];
                (super::value::Src::Ref(r), self.take_from(r, root))
            }
            // A returned value took its count before the exit set
            // (`hd_tir::own::elaborate`).
            Some(borrowed) => (super::value::Src::Ref(r), !borrowed && self.take(r)),
        };
        self.store_src(HEAP, base, 0, &mem.leaves, &src)?;
        if moved {
            self.settle_value(r);
        }
        Ok(())
    }

    /// Copies `size` bytes of the shadow stack from `at + from` to
    /// `base + off`: one `memory.copy`, whatever the value's size.
    fn copy_in(&mut self, base: u32, off: u32, at: u32, from: u32, size: u32) {
        address(&mut self.f, base, off);
        address(&mut self.f, at, from);
        self.f.op(I::I32Const(size.cast_signed()));
        self.f.push(crate::bulk::copy(HEAP, HEAP));
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

impl Em<'_, '_, '_> {
    /// Plans the locals this function keeps in memory (step 3 of #2031,
    /// rustc's `LocalRef::Place` for a local whose layout is not an
    /// immediate): a local, not a parameter, a capture, a cell or a slot,
    /// of a type [`by_address`] keeps, that the body stores
    /// whole once and otherwise only reads, whole or by fields. A local
    /// of a counted type [`by_address`] keeps (#2031 step 4b) is kept as
    /// a counted parameter is: read only by fields, a move only through
    /// fields passed by address, dropped whole; its one store writes its
    /// value in place or moves it in from memory, and its TIR `FlagSet`
    /// sets its flags; its `Drop` or exit release runs its type's release
    /// witness while its whole path's flag is set ([`Em::mem_release`]).
    /// Each gets
    /// an 8-byte aligned slot in the frame block ([`crate::wide::locals_frame`]); its
    /// stored value, when that store is its one use, is planned in place
    /// there ([`Em::plan_in_place`]). One stored value means no read of
    /// the local can see a later store before the use it passes on to,
    /// so a read passes the slot's address. A captured local stays
    /// leaves: its closure's environment copies them. Cost: one visit of
    /// this sub-body's instructions; the body's captures and use counts
    /// are counted once per body ([`Em::slot_uses`]), so a body with n
    /// closures costs O(its size) in all.
    pub(super) fn frame_locals(&mut self) -> hd_base::StageResult<()> {
        let b = self.b;
        // Per local of this sub-body: its whole stores and the last value
        // stored, or none once a use other than a read by fields or whole
        // rules it out.
        let uses = self.slot_uses();
        let mut seen: HashMap<usize, Option<(u32, u32)>> = HashMap::default();
        // Per local, what keeping a counted value in memory depends on: a
        // move not through fields passed by address (`far`, below) rules
        // it out; a whole drop rules out only a local that counts nothing.
        // A whole read keeps it only when it passes by address to a worker
        // (`whole`, below): a move clears its TIR flag, and a copy to a
        // callee that owns it is copied into the call's block, where the
        // retain shim takes its counts from the address.
        let mut moves: Vec<(usize, u32)> = Vec::new();
        let mut moved_from: HashSet<usize> = HashSet::default();
        let mut whole_reads: Vec<(usize, u32)> = Vec::new();
        let mut dropped: HashSet<usize> = HashSet::default();
        for &i in self.rows.clone().iter() {
            let tag = b.tags[i];
            if !matches!(
                tag,
                Tag::Read | Tag::Store | Tag::Ref | Tag::Drop | Tag::IsUnique | Tag::LoanEnd
            ) {
                continue;
            }
            let Some(p) = b.decode_place(b.data[i][0]) else {
                continue;
            };
            let PlaceRoot::Local(l) = p.root else {
                continue;
            };
            let entry = seen.entry(l.idx()).or_insert(Some((0, NONE)));
            let Some((stores, value)) = entry.as_mut() else {
                continue;
            };
            let fields = p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
            match tag {
                Tag::Read if fields => {
                    if p.proj.is_empty() {
                        whole_reads.push((l.idx(), u32_of(i)));
                    } else if b.data[i][1] == ReadMode::Move as u32 {
                        moves.push((l.idx(), u32_of(i)));
                    }
                    if b.data[i][1] == ReadMode::Move as u32 {
                        moved_from.insert(l.idx());
                    }
                }
                Tag::Store if p.proj.is_empty() && b.data[i][1] != NONE => {
                    *stores += 1;
                    *value = b.data[i][1];
                    if let Some(&off) = self.slots.get(&l.idx())
                        && self.cells.get(&l.idx()) == Some(&u32_of(i))
                        && uses.get(value) == Some(&1)
                        && !self.store_releases(u32_of(i))
                        && !self.loan_locals.contains_key(&l.idx())
                        && Ref(*value).as_inst().is_some()
                        && matches!(
                            b.tags[*value as usize],
                            Tag::Call | Tag::NewStruct | Tag::NewTuple | Tag::Scope
                        )
                        && by_address(self.pool(), self.env(), b.local_ty[l.idx()])?
                        && let Some(base) = self.frame_base()
                    {
                        self.frame_copies.push(*value);
                        self.plan_in_place(*value, base, off, &uses)?;
                    }
                }
                Tag::Drop if fields => {
                    dropped.insert(l.idx());
                }
                _ => *entry = None,
            }
        }
        // A move out of a counted local is skipped by its drop, which
        // elaborates only fields passed by address (as `compute_params`).
        let mut far: HashSet<usize> = HashSet::default();
        for (l, i) in moves {
            let ty = b.local_ty[l];
            if far.contains(&l) || !self.moved_field_flagged(ty, self.ty(i)) {
                continue;
            }
            let p = b.decode_place(b.data[i as usize][0]);
            if !p.is_some_and(|p| moves_through_addresses(self.pool(), self.env(), ty, &p.proj)) {
                far.insert(l);
            }
        }
        // A whole read of a counted local that would load its leaves (one
        // not passed by address to a worker, at its one use) rules it out:
        // the slot would cost a load per leaf where locals cost none.
        let mut whole: HashSet<usize> = HashSet::default();
        if !whole_reads.is_empty() {
            let passed: HashSet<u32> = self.worker_reads().into_iter().collect();
            for (l, r) in whole_reads {
                if !passed.contains(&r) || uses.get(&r) != Some(&1) {
                    whole.insert(l);
                }
            }
        }
        let mut kept: Vec<(usize, Mem, u32)> = Vec::new();
        for (l, got) in seen {
            let Some((1, v)) = got else {
                continue;
            };
            // Memory pays when the value lands there without its leaves: a
            // literal or a worker's result written in place, or a read of
            // memory copied (checked below, in local order). A value
            // computed as leaves (a match, a struct update, a read of
            // leaves) would be stored and loaded again: it stays leaves.
            if Ref(v).as_inst().is_none() {
                continue;
            }
            let lands = matches!(
                b.tags[v as usize],
                Tag::Read | Tag::NewStruct | Tag::NewTuple
            ) || b.tags[v as usize] == Tag::Call && self.writes_result(v);
            if !lands {
                continue;
            }
            if self.parameter_positions.contains_key(&l)
                || self.mem_locals.contains_key(&l)
                || self.captured.contains(&l)
                || self.is_cell(l)
                || self.slots.contains_key(&l)
            {
                continue;
            }
            let ty = b.local_ty[l];
            if !matches!(by_address(self.pool(), self.env(), ty), Ok(true)) {
                continue;
            }
            let m = crate::layout::mem(self.pool(), self.env(), ty)?;
            let counted = self.released(ty);
            // A counted value lands with its counts: written in place, or
            // moved in from memory, at its one use. A copy read stored
            // whole stays leaves; the optimizer leaves none to keep (tried
            // `let b = a` with both read after, with `b` or `a` then moved
            // to an owning callee, a parameter's field the same way, and
            // each in a loop: the copy moves to the use).
            let lands_counted = uses.get(&v) == Some(&1)
                && (b.tags[v as usize] != Tag::Read
                    || b.data[v as usize][1] == ReadMode::Move as u32);
            // A borrowed read of memory stored whole, at its one use, into
            // a local never moved from, dropped, or read whole but as a
            // worker's address: the local names the place it reads (an
            // inlined lent parameter), so it costs no slot and no copy.
            let alias = b.tags[v as usize] == Tag::Read
                && b.data[v as usize][1] == ReadMode::Borrow as u32
                && uses.get(&v) == Some(&1)
                && !moved_from.contains(&l)
                && !dropped.contains(&l)
                && !whole.contains(&l);
            if alias {
                kept.push((l, m, v));
                continue;
            }
            let ruled_out = if counted {
                whole.contains(&l) || far.contains(&l) || !lands_counted
            } else {
                dropped.contains(&l)
            };
            if ruled_out {
                continue;
            }
            kept.push((l, m, v));
        }
        if kept.is_empty() {
            return Ok(());
        }
        kept.sort_stable_by_key(|k| k.0);
        let base = self.f.local(Val::I32);
        let mut size = 0u32;
        for (l, mem, v) in kept {
            // A read copies only from memory: an address parameter or a
            // local kept before this one (a copy's source comes first).
            if b.tags[v as usize] == Tag::Read && self.mem_source(v).is_none() {
                continue;
            }
            if b.tags[v as usize] == Tag::Read
                && b.data[v as usize][1] == ReadMode::Borrow as u32
                && let Some((at, off, _, fields, src)) = self.mem_field(v)
            {
                // An alias: the read passes on its address and the store
                // writes nothing ([`Em::mem_store`]).
                let (_, part) = self.fold_path(b.local_ty[src], &fields)?;
                self.mem_locals
                    .insert(l, (at, off + part.offset, Rc::new(mem)));
                self.frame_copies.push(v);
                self.aliased.insert(v);
                continue;
            }
            let off = size;
            size = (size + mem.size).next_multiple_of(8);
            self.mem_locals.insert(l, (base, off, Rc::new(mem)));
            ledger!(if self.released(b.local_ty[l]) => "memlocal/counted" else "memlocal/uncounted");
            if Ref(v).as_inst().is_some() && uses.get(&v) == Some(&1) {
                self.frame_copies.push(v);
                self.plan_in_place(v, base, off, &uses)?;
            }
        }
        if size != 0 {
            self.stack_frame = Some((base, size));
        }
        Ok(())
    }

    /// The local kept in memory that `Read` `r` reads, whole or by fields.
    fn mem_source(&self, r: u32) -> Option<usize> {
        let p = self.b.decode_place(self.b.data[r as usize][0])?;
        let PlaceRoot::Local(l) = p.root else {
            return None;
        };
        let fields = p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
        (fields && self.mem_locals.contains_key(&l.idx())).then_some(l.idx())
    }

    /// Emits `Store` `i` when it stores a local kept in memory whole
    /// ([`Em::frame_locals`]): nothing when its value was written in place,
    /// one `memory.copy` of the layout's size when the value is a read
    /// that passes on its address, else its leaves. Whether it did.
    pub(super) fn mem_store(&mut self, i: u32) -> hd_base::StageResult<bool> {
        let Some(p) = self.b.decode_place(self.b.data[i as usize][0]) else {
            return Ok(false);
        };
        let PlaceRoot::Local(l) = p.root else {
            return Ok(false);
        };
        if !p.proj.is_empty() {
            return Ok(false);
        }
        let Some((base, off, mem)) = self.mem_locals.get(&l.idx()).cloned() else {
            return Ok(false);
        };
        // ruled out: a diverging value reaches no `Store` (tried `let a:
        // Big = panic(..)` and a match whose other arm panics; TIR ends the
        // path before the store), so `v` has its values or its address.
        let v = self.b.data[i as usize][1];
        if self.written.contains(&v) || self.aliased.contains(&v) {
            return Ok(true);
        }
        if let Some(&(at, from)) = self.mem_vals.get(&v) {
            self.copy_in(base, off, at, from, mem.size);
            return Ok(true);
        }
        self.store_src(HEAP, base, off, &mem.leaves, &super::value::Src::Ref(v))?;
        Ok(true)
    }
}

/// A `Read` of a local kept in memory ([`Em::mem_field`]): its address
/// local, offset and layout, the field path and the local.
pub(super) type MemField = (u32, u32, Rc<Mem>, Vec<u32>, usize);

impl Em<'_, '_, '_> {
    /// Whether a value of `t` kept by address is released from it at its
    /// drop ([`Em::mem_release`]): it holds a counted reference or a
    /// `Drop` hook.
    pub(super) fn released(&self, t: hd_types::Ty) -> bool {
        self.cx().tracked(t) || crate::layout::holds_drop(self.pool(), self.env(), t)
    }

    /// Whether a move of a field of type `field` out of a local of type
    /// `local` kept by address needs a move flag its drop tests: the
    /// field holds a counted reference, or the local holds a `Drop` hook,
    /// whose run any moved field rules out for its enclosing values
    /// (`ref.move.field`, as the leaf path's drop-state flags).
    fn moved_field_flagged(&self, local: hd_types::Ty, field: hd_types::Ty) -> bool {
        self.cx().tracked(field) || crate::layout::holds_drop(self.pool(), self.env(), local)
    }

    /// Releases local `l` kept in memory from its address: the open drop
    /// over its leaves under the TIR flags of its paths
    /// (`hd_tir::own::drop_flags`, which flags a local kept by address:
    /// set by its store or at its sub-body's entry, cleared by a whole
    /// move and by each `Drop`), each whole node released where it lives
    /// (its type's in-place shim when it holds a hook). Whether `l` is a
    /// counted address parameter or a counted frame local. O(the flags
    /// and the fields on their paths).
    pub(super) fn mem_release(&mut self, l: usize) -> hd_base::StageResult<bool> {
        let Some((at, off, mem)) = self.mem_locals.get(&l).cloned() else {
            return Ok(false);
        };
        let ty = self.b.local_ty[l];
        if !self.parameter_positions.contains_key(&l) && !self.released(ty) {
            return Ok(false);
        }
        let counted = crate::rt::counted_of(self.input, self.m, ty)?;
        if !counted.counts() {
            return Ok(true);
        }
        // Released from its address by the type's object glue, one helper
        // per layout, in place; its flags guard this (`hd_tir::own::elaborate`).
        address(&mut self.f, at, off);
        self.f.helper(crate::rt::Helper::DropParts {
            leaves: mem.leaves.clone(),
            parts: vec![(0, counted)],
        });
        Ok(true)
    }
}
