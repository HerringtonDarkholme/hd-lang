//! Parameters and results passed by address (#2031, steps 1 to 4;
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
//! A local of such a type kept in memory (step 3, rustc's
//! `LocalRef::Place`) lives in a slot of one *frame block* on the shadow
//! stack, reserved at the function's entry and given back at every exit
//! ([`Em::frame_locals`], [`stack_frame`]). Its store writes a literal
//! or a worker's result in place, copies a value read from memory with
//! one `memory.copy`, or stores leaves; a read of it, whole or of fields,
//! is an address like a parameter's.
//!
//! A parameter keeps the address form only when every use of it is a
//! read of its fields ([`params`]); any other parameter stays leaves. The
//! ownership passes write no `Drop` or `LoanEnd` for a type that counts
//! nothing and holds nothing to drop.
//!
//! A parameter whose type holds counted references (step 4,
//! [`crate::layout::by_address`]) passes by address too when the instance
//! owns it: its counts move with its bits, and its `Drop` or exit release
//! runs its type's release witness from the address
//! ([`crate::layout::ReleasePlan`], Swift's value-witness `destroy`),
//! skipping a field a move took out under that field's flag (rustc's drop
//! elaboration, [`Em::mem_moves`], [`Em::mem_release`]). Results and
//! frame locals of such a type still take the leaf form. Cost: [`params`] visits the
//! callee's body once, memoized per instance; [`Em::forwarded`] and
//! [`Em::kept_results`] visit only the sub-body being emitted, and the
//! value use counts they and [`Em::place_result`] read are counted once
//! per body, shared by its instances and closures ([`Em::slot_uses`]),
//! so a body with n closures costs O(its size), not n times that.

use std::rc::Rc;

use hd_base::{HashMap, HashSet, InstId, SliceSort as _};
use hd_mono::{Sub, subst};
use hd_tir::ir::{NONE, PlaceRoot, Proj, ReadMode, Ref, Tag, Word, slots};

use super::shape::u32_of;
use super::{Em, Input, param_locals};
use crate::layout::{Mem, ReleasePlan, ReleaseStep, by_address, indirect};
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
    by_address(
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
    // A counted parameter passes by address only when the callee owns it
    // (`layout::Pass::Owned`): its count arrives with its bits, and its
    // drop releases them from the address (`Em::mem_release`). A lent one
    // keeps leaves.
    let lends = env.lends(item, args) | hd_mono::defaults::promoted_mask(&t.defaults[id.idx()]);
    let mut found: HashMap<usize, Mem> = HashMap::default();
    let mut counted: HashMap<usize, hd_types::Ty> = HashMap::default();
    for (k, &l) in locals.iter().enumerate() {
        let ty = subst(pool, env, item, args, b.local_ty[l]);
        if let Ok(Some(m)) = by_address(pool, env, ty) {
            if crate::layout::walk(pool, env, ty).is_some() {
                if k < 64 && lends & (1 << k) != 0 {
                    continue;
                }
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
        let kept = match (tag, counted.get(&l.idx())) {
            (Tag::Read, None) => fields,
            // Its whole drop releases from the address.
            (Tag::Drop, Some(_)) => p.proj.is_empty(),
            // A move out of a counted one is skipped by its drop, which
            // elaborates only fields passed by address: every proper
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
        if !matches!(by_address(pool, env, u), Ok(Some(_))) {
            return false;
        }
        t = u;
    }
    true
}

/// The release plan of a type passed by address ([`ReleasePlan`]), made
/// once per type in `memo`, nested types first, with a stack of its own:
/// a chain of K nested types takes no native stack depth and O(K) work.
pub(crate) fn release_plan(
    pool: &hd_types::InternPool,
    env: &dyn hd_mono::ProgramEnv,
    memo: &mut HashMap<hd_types::Ty, Option<std::sync::Arc<ReleasePlan>>>,
    t: hd_types::Ty,
) -> hd_base::StageResult<Option<std::sync::Arc<ReleasePlan>>> {
    let strip = |u| crate::layout::strip(pool, env, u);
    let t = strip(t);
    let mut stack = vec![(t, false)];
    while let Some((u, ready)) = stack.pop() {
        if memo.contains_key(&u) {
            continue;
        }
        let ms = crate::layout::members(pool, env, u)?;
        if !ready {
            let mut pending = Vec::new();
            for &f in &ms.tys {
                let f = strip(f);
                if !memo.contains_key(&f) && by_address(pool, env, f)?.is_some() {
                    pending.push((f, false));
                }
            }
            if !pending.is_empty() {
                stack.push((u, true));
                stack.extend(pending);
                continue;
            }
        }
        let mem = crate::layout::mem(pool, env, u)?;
        let mut steps = Vec::new();
        for (field, (&f, part)) in ms.tys.iter().zip(&ms.parts).enumerate() {
            let field = u32_of(field);
            if by_address(pool, env, f)?.is_some() {
                if let Some(plan) = memo.get(&strip(f)).cloned().flatten() {
                    steps.push(ReleaseStep::Nested {
                        field,
                        offset: part.offset,
                        plan,
                    });
                }
            } else if let Some(walk) = crate::layout::count_walk(pool, env, f) {
                steps.push(ReleaseStep::Inline {
                    field,
                    leaves: mem.leaves[part.first..part.first + part.count].to_vec(),
                    walk,
                });
            }
        }
        memo.insert(
            u,
            (!steps.is_empty()).then(|| std::sync::Arc::new(ReleasePlan::new(steps))),
        );
    }
    Ok(memo.get(&t).cloned().flatten())
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
        let b = self.b;
        if self.mem_locals.is_empty() {
            return HashSet::default();
        }
        // A read of memory stored whole into a local kept in memory is
        // copied there with one `memory.copy`.
        let mut candidates: Vec<u32> = std::mem::take(&mut self.frame_copies)
            .into_iter()
            .filter(|&v| self.mem_field(v).is_some())
            .collect();
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
                // A copy of a counted field takes counts of its own, so
                // only a move hands its address on.
                if w.is_some()
                    && self.mem_field(*r).is_some()
                    && (b.data[*r as usize][1] == ReadMode::Move as u32
                        || !self.tracked(self.ty_of(*r)))
                {
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

    /// For a `Read` `r` of a local kept in memory, whole or of fields: the
    /// local's address local and offset, its layout and the field path.
    pub(super) fn mem_field(&self, r: u32) -> Option<(u32, u32, Mem, Vec<u32>, usize)> {
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
        let declared = self.sub(self.b.local_ty[l]);
        let (_, part) = self.fold_path(declared, &fields)?;
        // A move out of a counted parameter: its drop skips the field.
        let moved = self.b.data[i as usize][1] == ReadMode::Move as u32;
        if moved
            && let Some(&(_, flag)) = self
                .mem_moved
                .get(&l)
                .and_then(|ms| ms.iter().find(|(p, _)| *p == fields))
        {
            self.f.i32(1);
            self.f.set(flag);
        }
        if self.forward.contains(&i) {
            self.mem_vals.insert(i, (at, off + part.offset));
            return Ok(true);
        }
        for leaf in &mem.leaves[part.first..part.first + part.count] {
            self.f.get(at);
            self.f.op(leaf.load_in(STACK, off));
        }
        self.store_as(i, moved)?;
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
                    // An owned counted argument moves in or takes counts
                    // of its own, as a leaf argument does (`Em::push_arg`).
                    let src = if self.tracked(params[k]) {
                        self.arg_src(i, k, r, params[k], Some(lends))?
                    } else {
                        None
                    };
                    for (j, leaf) in mem.leaves.iter().enumerate() {
                        self.f.get(base);
                        match &src {
                            Some(super::value::Src::Locals(ls)) => self.f.get(ls[j]),
                            Some(super::value::Src::Ref(r)) => self.comp(*r, j)?,
                            None => self.comp(r, j)?,
                        }
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
            let tag = b.tags[r as usize];
            let members = match tag {
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
                    Tag::Call | Tag::NewStruct | Tag::NewTuple
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
            let src = self.retain_src(&super::value::Src::Ref(r), ft)?;
            for (j, leaf) in mem.leaves.iter().enumerate() {
                self.f.get(base);
                self.src(&src, j)?;
                self.f.op(leaf.store_in(STACK, off + part.offset));
            }
        }
        self.written.insert(i);
        Ok(())
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
        if self.written.contains(&r) {
            return Ok(());
        }
        if let Some(&(at, from)) = self.mem_vals.get(&r) {
            self.copy_in(base, 0, at, from, mem.size);
            return Ok(());
        }
        let counted = self.counting && self.tracked(self.ty_of(r));
        let (src, moved) = match ret {
            _ if !counted => (super::value::Src::Ref(r), false),
            None => {
                let root = self.b.sub_root[self.sub_id as usize];
                (super::value::Src::Ref(r), self.take_from(r, root))
            }
            Some(borrowed) => {
                let moved = !borrowed && self.take(r);
                let s = if moved || borrowed {
                    super::value::Src::Ref(r)
                } else {
                    self.retain_ref(r)?
                };
                (s, moved)
            }
        };
        for (j, leaf) in mem.leaves.iter().enumerate() {
            self.f.get(base);
            self.src(&src, j)?;
            self.f.op(leaf.store_in(STACK, 0));
        }
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
        self.f.push(crate::bulk::copy(STACK, STACK));
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

/// Reserves the frame block of `size` bytes at the entry of `f`, its
/// address in local `base`, and gives it back at every exit: a `return`
/// and every branch out of the function's own label, as
/// [`crate::wide::locals_frame`] does for the local stack. A `?` or an
/// early `return` is such an exit, and a loop reuses the block each
/// round, so no path leaves it reserved.
pub(crate) fn stack_frame(f: &mut Func, base: u32, size: u32) {
    use crate::module::Ins;
    let size = size.cast_signed();
    let selector = f.local(Val::I32);
    let stack = f.intern_global(Global::Stack);
    let mut out = vec![
        Ins::GlobalGet(stack),
        Ins::Op(I::I32Const(size)),
        Ins::Op(I::I32Sub),
        Ins::Op(I::LocalTee(base)),
        Ins::GlobalSet(stack),
    ];
    let restore = |out: &mut Vec<Ins>| {
        out.extend([
            Ins::Op(I::LocalGet(base)),
            Ins::Op(I::I32Const(size)),
            Ins::Op(I::I32Add),
            Ins::GlobalSet(stack),
        ]);
    };
    let mut origins = crate::origins::Remap::new(f.origins.as_deref());
    origins.repeat(f.origin_root, out.len());
    let body = std::mem::take(&mut f.body);
    let mut depth = 0;
    for (position, ins) in body.into_iter().enumerate() {
        let start = out.len();
        crate::wide::exit(f, ins, &mut depth, selector, &restore, &mut out);
        origins.merged(position..position + 1, out.len() - start);
    }
    let start = out.len();
    restore(&mut out);
    origins.repeat(f.origin_root, out.len() - start);
    f.body = out;
    origins.finish(f);
}

impl Em<'_, '_, '_> {
    /// Plans the locals this function keeps in memory (step 3 of #2031,
    /// rustc's `LocalRef::Place` for a local whose layout is not an
    /// immediate): a local, not a parameter, a capture, a cell or a slot,
    /// of a type [`indirect`] passes by address, that the body stores
    /// whole once and otherwise only reads, whole or by fields. Each gets
    /// an 8-byte aligned slot in the frame block ([`stack_frame`]); its
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
        let mut seen: HashMap<usize, Option<(u32, u32)>> = HashMap::default();
        for &i in self.rows.clone().iter() {
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
            let entry = seen.entry(l.idx()).or_insert(Some((0, NONE)));
            let Some((stores, value)) = entry.as_mut() else {
                continue;
            };
            let fields = p.proj.iter().all(|s| matches!(s, Proj::Field(_)));
            match tag {
                Tag::Read if fields => {}
                Tag::Store if p.proj.is_empty() && b.data[i][1] != NONE => {
                    *stores += 1;
                    *value = b.data[i][1];
                }
                _ => *entry = None,
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
            let ty = self.sub(b.local_ty[l]);
            if let Ok(Some(m)) = indirect(self.pool(), self.env(), ty) {
                kept.push((l, m, v));
            }
        }
        if kept.is_empty() {
            return Ok(());
        }
        kept.sort_stable_by_key(|k| k.0);
        let base = self.f.local(Val::I32);
        let uses = self.slot_uses();
        let mut size = 0u32;
        for (l, mem, v) in kept {
            // A read copies only from memory: an address parameter or a
            // local kept before this one (a copy's source comes first).
            if b.tags[v as usize] == Tag::Read && self.mem_source(v).is_none() {
                continue;
            }
            let off = size;
            size = (size + mem.size).next_multiple_of(8);
            self.mem_locals.insert(l, (base, off, mem));
            if Ref(v).as_inst().is_some() && uses.get(&v) == Some(&1) {
                self.frame_copies.push(v);
                self.plan_in_place(v, base, off, &uses)?;
            }
        }
        self.stack_frame = Some((base, size));
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
        if self.written.contains(&v) {
            return Ok(true);
        }
        if let Some(&(at, from)) = self.mem_vals.get(&v) {
            self.copy_in(base, off, at, from, mem.size);
            return Ok(true);
        }
        for (j, leaf) in mem.leaves.iter().enumerate() {
            self.f.get(base);
            self.comp(v, j)?;
            self.f.op(leaf.store_in(STACK, off));
        }
        Ok(true)
    }
}

impl Em<'_, '_, '_> {
    /// Notes each field path a `Read move` takes out of a counted address
    /// parameter, with a flag local its drop tests (rustc's drop
    /// elaboration with drop flags; a Wasm local starts at 0, "not
    /// moved"). One visit of this sub-body's instructions, only when it
    /// has such a parameter.
    pub(super) fn mem_moves(&mut self) {
        let b = self.b;
        let counted: Vec<usize> = self
            .mem_locals
            .keys()
            .copied()
            .filter(|l| {
                self.parameter_positions.contains_key(l) && self.tracked(self.sub(b.local_ty[*l]))
            })
            .collect();
        if counted.is_empty() {
            return;
        }
        for &i in self.rows.clone().iter() {
            if b.tags[i] != Tag::Read || b.data[i][1] != ReadMode::Move as u32 {
                continue;
            }
            let Some((_, _, _, fields, l)) = self.mem_field(u32_of(i)) else {
                continue;
            };
            if !counted.contains(&l) || !self.tracked(self.ty(u32_of(i))) {
                continue;
            }
            let known = self
                .mem_moved
                .get(&l)
                .is_some_and(|ms| ms.iter().any(|(p, _)| *p == fields));
            if !known {
                let flag = self.f.local(Val::I32);
                self.mem_moved.entry(l).or_default().push((fields, flag));
            }
        }
    }

    /// Releases counted address parameter `l` from its address, once: its
    /// type's release plan ([`release_plan`]), with each field a move took
    /// out skipped under its flag. A later release (the exit's, after a
    /// `Drop`) finds the dead flag set. Whether `l` is such a parameter.
    /// O(the fields on the moved paths), the rest one witness call each.
    pub(super) fn mem_release(&mut self, l: usize) -> hd_base::StageResult<bool> {
        enum Task {
            Level(u32, std::sync::Arc<ReleasePlan>, Vec<(Vec<u32>, u32)>),
            Step(u32, ReleaseStep),
            Guard(u32),
            End,
        }
        let Some((at, off, _)) = self.mem_locals.get(&l).cloned() else {
            return Ok(false);
        };
        if !self.parameter_positions.contains_key(&l) {
            return Ok(false);
        }
        let ty = self.sub(self.b.local_ty[l]);
        let (pool, env) = (self.input.pool, self.input.env);
        let plan = release_plan(pool, env, &mut self.m.release_plans, ty)?;
        let Some(plan) = plan else {
            return Ok(true);
        };
        let dead = if let Some(&d) = self.mem_dead.get(&l) {
            d
        } else {
            let d = self.f.local(Val::I32);
            self.mem_dead.insert(l, d);
            d
        };
        self.f.get(dead);
        self.f.op(I::I32Eqz);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        let moved = self.mem_moved.get(&l).cloned().unwrap_or_default();
        let mut todo = vec![Task::Level(off, plan, moved)];
        while let Some(task) = todo.pop() {
            match task {
                Task::End => self.f.op(I::End),
                Task::Guard(flag) => {
                    self.f.get(flag);
                    self.f.op(I::I32Eqz);
                    self.f.op(I::If(wasm_encoder::BlockType::Empty));
                }
                Task::Step(off, step) => crate::rt::release_step(&mut self.f, at, off, &step),
                Task::Level(off, plan, moved) => {
                    let mut seq = Vec::new();
                    for step in &plan.steps {
                        let here: Vec<(Vec<u32>, u32)> = moved
                            .iter()
                            .filter(|(p, _)| p.first() == Some(&step.field()))
                            .map(|(p, flag)| (p[1..].to_vec(), *flag))
                            .collect();
                        let guards: Vec<u32> = here
                            .iter()
                            .filter(|(p, _)| p.is_empty())
                            .map(|(_, flag)| *flag)
                            .collect();
                        let deeper: Vec<(Vec<u32>, u32)> =
                            here.into_iter().filter(|(p, _)| !p.is_empty()).collect();
                        seq.extend(guards.iter().map(|&g| Task::Guard(g)));
                        // `indirect::params` keeps a move only through
                        // fields passed by address: a deeper path is one
                        // into a nested plan.
                        match step {
                            ReleaseStep::Nested { offset, plan, .. } if !deeper.is_empty() => {
                                seq.push(Task::Level(off + offset, plan.clone(), deeper));
                            }
                            _ => seq.push(Task::Step(off, step.clone())),
                        }
                        seq.extend(guards.iter().map(|_| Task::End));
                    }
                    todo.extend(seq.into_iter().rev());
                }
            }
        }
        self.f.i32(1);
        self.f.set(dead);
        self.f.op(I::End);
        Ok(true)
    }
}
