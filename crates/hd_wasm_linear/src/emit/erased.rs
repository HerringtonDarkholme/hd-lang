//! The erased method ABI on the linear backend (`crates/hd_mono/DESIGN.md`
//! §13.5.1, `crates/hd_wasm_linear/DESIGN.md` §23.27). A generic method called
//! through a trait value has one erased body per implementation, whose own
//! type parameters stay open, and a witness per call's type arguments.
//!
//! - **Open values.** A value of an open type is one `i32`, a counted
//!   address that owns a count like any other: the caller's value itself
//!   when its layout is one counted address (a list, a map, a data value,
//!   a reference), 0 when it has no values, and otherwise a new counted
//!   box of its layout, as a trait value boxes its data (§23.19). So the
//!   erased body moves, stores, counts and releases open values with no
//!   knowledge of their types.
//! - **Open instructions** are outlined: instruction `k` calls the `k`th
//!   thunk of the body's table, which the witness holds, with its
//!   operands and captures, each with a count the thunk owns, then the
//!   body's providers. A thunk is that one instruction emitted at the
//!   caller's types; it unboxes its open operands and boxes its open
//!   result, which it returns with its own count.
//! - **Witnesses** are static: per erased body of the method's family, the
//!   address of a table of its thunks' function-table slots. A body reads
//!   its own table at its family field, which link knows.
//! - **Cells.** A local that closures share in a generic method keeps its
//!   erased view in every instance, since the closures that the erased
//!   body's thunks build are the concrete instances' own: a concrete read
//!   unboxes, a concrete write boxes.
//! - **References** to an open value are the address of a word holding
//!   it, which no concrete place is when the value crosses boxed. Such a
//!   reference crosses as a counted representation view retaining its
//!   original place. Typed getters and setters convert at each access;
//!   aliases keep following replacements after the call has returned.
//! - **Kept suspensions.** An erased frame kept as a `Suspend[T]` whose
//!   `T` crosses boxed is wrapped in a frame whose poll unboxes the
//!   result (`Helper::OpenFrame`).

use hd_base::{DefId, Hash128, InstId, StageResult};
use hd_mono::layout::CanonMemo;
use hd_mono::{
    INFO, Target, declared_row_keys, erased_args, has_row_params, is_open, open_param, own_count,
    subst, thunk_capture_locals, thunk_operands, witness_args, witness_family,
};
use hd_tir::ir::{Callee, NONE, Ref, Tag};
use hd_types::{Ty, TyList};
use wasm_encoder::Instruction as I;

use super::value::Src;
use super::{Ctl, Em};
use crate::layout::{Kind, Mem, Part, Walk, kind, mem as mem_of, walk};
use crate::module::{Func, HEAP, Module, Val, mem};
use crate::rt::Helper;
use crate::{Input, unsupported};

/// The generic parts of a trait method's vtable slot: each parameter
/// after the receiver's erased type when it is open, the erased result
/// type and whether it is open, and the static witness of the call's
/// type arguments.
pub(super) struct DynOpen {
    pub params: Vec<Option<Ty>>,
    pub ret: bool,
    pub ret_ty: Ty,
    pub witness: usize,
}

/// How a concrete value crosses to an open one.
enum Cross {
    /// Both sides are open, or the value is one counted address.
    Same,
    /// The value has no Wasm values: the open value is 0.
    Empty,
    /// A counted box of this layout holds the value.
    Box(Mem),
    /// A thin reference whose concrete value crosses boxed or empty.
    /// A representation view forwards each access to the source place.
    RefBox { target: Ty },
}

/// A caller-owned argument view released when a synchronous call returns.
pub(super) struct Back {
    held: u32,
    ty: Ty,
}

/// The witness family of erased body `id`.
pub(super) fn family(input: &Input<'_>, id: InstId) -> StageResult<Hash128> {
    match input.collected.calls[id.idx()].get(&INFO) {
        Some(&Target::Erased { family }) => Ok(family),
        _ => unsupported("an erased body that collection did not record"),
    }
}

/// Whether `t` is a suspension, a cold call's frame.
fn is_suspension(input: &Input<'_>, t: Ty) -> bool {
    matches!(input.pool.get(t), hd_types::TyData::Adt { def, .. }
        if input.env.std_kind(def) == hd_mono::layout::StdKind::Suspend)
}

/// The type of instruction value `r` of `b`, unsubstituted.
fn raw_ty(b: &hd_tir::ir::Body, r: u32) -> Ty {
    b.ty[r as usize]
}

/// Thunk `id`: one open instruction of an erased body at the caller's
/// type arguments. It takes the erased body's view of the instruction's
/// operands and captures, each with a count of its own, then the body's
/// providers and context, and returns the erased view of the result with
/// its own count; a tag switch's thunk returns the scrutinee's tag.
pub(super) fn thunk(input: &Input<'_>, m: &mut Module, id: InstId) -> StageResult<Func> {
    let t = &input.collected.table;
    let (item, args) = (t.item[id.idx()], t.args[id.idx()]);
    let calls = &input.collected.calls[id.idx()];
    let Some(&Target::Thunk { at }) = calls.get(&INFO) else {
        return unsupported("a thunk that collection did not record");
    };
    let Some(b) = (input.body)(id) else {
        return unsupported("a thunk without TIR");
    };
    let (pool, env) = (input.pool, input.env);
    let erased = erased_args(pool, env, item, args);
    let se = |t: Ty| subst(pool, env, item, erased, t);
    let caps = thunk_capture_locals(b, at);
    // A shared capture is its cell's address; any other capture is a
    // value of the thunk's own.
    let shared_cells = caps
        .iter()
        .filter(|c| c.1)
        .map(|c| (c.0 as usize, u32::MAX))
        .collect();
    let mut em = Em::new(input, m, b, (item, args), calls, shared_cells);
    em.origin_root = at;
    let mut params = Vec::new();
    let mut ops = Vec::new();
    for r in thunk_operands(b, at) {
        let et = se(raw_ty(b, r));
        let n = em.vals(et)?.len();
        ops.push((r, params.len(), n, et));
        params.extend(em.vals(et)?);
    }
    let mut held = Vec::new();
    let mut backs = Vec::new();
    for (l, shared) in &caps {
        let et = se(b.local_ty[*l as usize]);
        let vs = if *shared {
            vec![Val::I32]
        } else {
            em.vals(et)?
        };
        held.push((*l as usize, *shared, params.len(), vs.len(), et));
        params.extend(vs);
    }
    for key in declared_row_keys(pool, env, item, args) {
        let p = u32::try_from(params.len()).expect("params");
        em.providers.push((key, [p, p + 1]));
        params.extend([Val::I32, Val::I32]);
    }
    if has_row_params(env, item) {
        em.open_ctx = Some(u32::try_from(params.len()).expect("params"));
        params.push(Val::I32);
    }
    em.open_base = em.providers.len();
    let switch = b.tags[at as usize] == Tag::SwitchTag;
    let rty = se(b.ty[at as usize]);
    let results = if switch {
        vec![Val::I32]
    } else {
        em.vals(rty)?
    };
    em.results.clone_from(&results);
    em.f = Func::new(
        format!("{}#thunk{at}", (input.path)(item)),
        params,
        results.clone(),
    );
    em.inherit_origin_source();
    em.counting = em.counts_anything();
    if em.counting {
        em.ret = results.iter().map(|v| em.f.local(*v)).collect();
        em.open(Ctl::Exit);
    }
    let range = |p: usize, n: usize| -> Vec<u32> {
        (p..p + n)
            .map(|x| u32::try_from(x).expect("params"))
            .collect()
    };
    // Synchronous callers may hold an argument view until return; cold
    // arguments use ordinary ownership instead.
    let returns = matches!(
        b.tags[at as usize],
        Tag::Call | Tag::CallValue | Tag::CallDyn | Tag::DefaultCall
    ) && !is_suspension(input, em.strip_mut(em.ty(at)));
    for (r, p, n, et) in ops {
        let ct = em.ty_of(r);
        let ls = if is_open(pool, et) {
            let o = u32::try_from(p).expect("params");
            em.unerase_arg(o, et, ct, returns.then_some(&mut backs))?
        } else {
            range(p, n)
        };
        em.vals.insert(r, ls);
    }
    for (l, shared, p, n, et) in held {
        let ct = em.sub(b.local_ty[l]);
        let ls = if !shared && is_open(pool, et) {
            em.unerase(u32::try_from(p).expect("params"), et, ct, true)?
        } else {
            range(p, n)
        };
        em.locals.insert(l, ls);
    }
    let out: Vec<u32> = if switch {
        em.tag(b.data[at as usize][0])?;
        let x = em.f.local(Val::I32);
        em.f.set(x);
        em.write_back(backs)?;
        vec![x]
    } else {
        em.inst(at)?;
        em.write_back(backs)?;
        match em.vals.get(&at).cloned() {
            None => Vec::new(),
            Some(_) if em.diverges(at) => Vec::new(),
            Some(ls) if is_open(pool, rty) => vec![em.erase(&ls, rty, em.ty(at), false)?],
            Some(ls) => {
                em.retain_locals(&ls, em.ty(at))?;
                ls
            }
        }
    };
    if out.len() == results.len() {
        for l in &out {
            em.f.get(*l);
        }
    } else {
        em.f.op(I::Unreachable);
    }
    if em.counting {
        for l in em.ret.clone().iter().rev() {
            em.f.set(*l);
        }
        em.close();
        em.release_all()?;
        for l in em.ret.clone() {
            em.f.get(l);
        }
    }
    Ok(em.f)
}

impl Em<'_, '_, '_> {
    /// An erased body's prologue: the address of its thunks' table, read
    /// from its family's field of the witness in parameter `w`.
    pub(super) fn read_ops(&mut self, family: Hash128, key: Hash128, w: u32) -> StageResult<()> {
        let Some(field) = self
            .input
            .collected
            .witnesses
            .families
            .iter()
            .find(|f| f.0 == family)
            .and_then(|f| f.1.iter().position(|k| *k == key))
        else {
            return unsupported("an erased body outside its family's witnesses");
        };
        let ops = self.f.local(Val::I32);
        self.f.get(w);
        self.f.op(I::I32Load(mem(
            HEAP,
            4 * u64::try_from(field).expect("field"),
            2,
        )));
        self.f.set(ops);
        self.ops = Some(ops);
        Ok(())
    }

    /// Open instruction `i`, its thunk `k`'s result stored as its own.
    pub(super) fn open_inst(&mut self, i: u32, k: u32) -> StageResult<()> {
        self.open_call(i, k)?;
        if self.diverges(i) {
            self.f.op(I::Unreachable);
            return Ok(());
        }
        self.store_owned(i)
    }

    /// Calls thunk `k` of open instruction `i`, its results left on the
    /// stack: its operands and captures, each with a count for the thunk,
    /// then the body's providers, and, with row parameters, a context of
    /// every other key in scope in front of the open one, so the thunk
    /// finds each key its instruction needs at concrete types.
    pub(super) fn open_call(&mut self, i: u32, k: u32) -> StageResult<()> {
        let Some(ops) = self.ops else {
            return unsupported("an open instruction outside an erased body");
        };
        let mut params = Vec::new();
        for r in thunk_operands(self.b, i) {
            params.extend(self.vals(self.ty_of(r))?);
            if !self.take(r) {
                self.retain_ref(r)?;
            }
            self.push(r)?;
        }
        for (l, shared) in thunk_capture_locals(self.b, i) {
            let l = l as usize;
            if shared {
                let c = self.cell_of(l)?;
                self.retain_addr(c);
                self.f.get(c);
                params.push(Val::I32);
                continue;
            }
            let t = self.sub(self.b.local_ty[l]);
            let ls = self.local_value(u32::try_from(l).expect("locals"))?;
            self.retain_locals(&ls, t)?;
            for x in &ls {
                self.f.get(*x);
            }
            params.extend(self.vals(t)?);
        }
        let (pool, env) = (self.input.pool, self.input.env);
        let keys = declared_row_keys(pool, env, self.item, self.args);
        for key in &keys {
            let [d, v] = self.provider(*key)?;
            self.f.get(d);
            self.f.get(v);
            params.extend([Val::I32, Val::I32]);
        }
        let c = if has_row_params(env, self.item) {
            let mut extra: Vec<Ty> = Vec::new();
            for (key, _) in self.providers.iter().rev() {
                if !keys.contains(key) && !extra.contains(key) {
                    extra.push(*key);
                }
            }
            params.push(Val::I32);
            self.push_ctx_of(&extra, true)?
        } else {
            None
        };
        let results = if self.b.tags[i as usize] == Tag::SwitchTag {
            vec![Val::I32]
        } else {
            self.vals(self.ty(i))?
        };
        self.f.get(ops);
        self.f.op(I::I32Load(mem(HEAP, 4 * u64::from(k), 2)));
        self.f.call_indirect(params, results);
        self.release_ctx(c);
        Ok(())
    }

    /// One more count on the counted address in local `x`.
    fn retain_addr(&mut self, x: u32) {
        if let Some(r) = self.input.runtime {
            self.f.get(x);
            self.f.call(crate::module::Sym::Inst(r.retain));
        }
    }

    /// One count less on the open value in local `o`.
    pub(super) fn release_open(&mut self, o: u32) {
        if self.input.runtime.is_some() {
            self.f.get(o);
            self.f.helper(Helper::ReleaseRef);
        }
    }

    /// How a concrete `ct` value crosses as the open type `et`. A
    /// reference whose target is open is the address of an open value, so
    /// a reference to a value that is one counted address crosses as it
    /// is (its place holds the same word), and any other thin one is
    /// represented by a permanent view over its original storage.
    fn crossing(&self, et: Ty, ct: Ty) -> StageResult<Cross> {
        let (pool, env) = (self.pool(), self.env());
        if is_open(pool, ct) {
            return Ok(Cross::Same);
        }
        let one = |t: Ty| -> StageResult<bool> {
            Ok(self.vals(t)? == [Val::I32] && walk(pool, env, t) == Some(Walk::Ref(0)))
        };
        if let Kind::Ref { .. } = kind(pool, env, et) {
            return match kind(pool, env, ct) {
                Kind::Ref { target, fat: false } if one(target)? => Ok(Cross::Same),
                Kind::Ref { target, fat: false } => Ok(Cross::RefBox { target }),
                _ => unsupported("a reference to a trait value as an open value"),
            };
        }
        if self.vals(ct)?.is_empty() {
            return Ok(Cross::Empty);
        }
        if one(ct)? {
            return Ok(Cross::Same);
        }
        Ok(Cross::Box(mem_of(pool, env, ct)?))
    }

    /// A local holding the concrete `ct` value in locals `ls` as an open
    /// value of type `et`, with a count of its own: the value's count
    /// moves in when `owned`, else the open value takes a new one.
    pub(super) fn erase(&mut self, ls: &[u32], et: Ty, ct: Ty, owned: bool) -> StageResult<u32> {
        let out = self.f.local(Val::I32);
        match self.crossing(et, ct)? {
            Cross::Same => {
                self.f.get(ls[0]);
                self.f.set(out);
                if !owned {
                    self.retain_locals(&[out], et)?;
                }
            }
            Cross::Empty => {
                self.f.i32(0);
                self.f.set(out);
            }
            Cross::Box(m) => {
                let w = self.counted(ct);
                let cyclic = self.cyclic_holding(&[ct]);
                let at = self.object(&m, w.as_ref(), cyclic);
                let part = Part {
                    first: 0,
                    count: m.leaves.len(),
                    offset: 0,
                };
                self.store_part(at, &m, part, &Src::Locals(ls.to_vec()))?;
                if !owned {
                    self.retain_fields(at, &m, w.as_ref());
                }
                self.f.get(at);
                self.f.set(out);
            }
            Cross::RefBox { target, .. } => {
                let view = self.ref_view(ls[0], target)?;
                if owned {
                    self.release_open(ls[0]);
                }
                self.f.get(view);
                self.f.set(out);
            }
        }
        Ok(out)
    }

    /// Locals holding the open value of type `et` in local `o` as a
    /// concrete `ct` value. When `owned`, the open value's count is used
    /// up and the result holds counts of its own; otherwise the result is
    /// a view that lives as long as the open value.
    pub(super) fn unerase(&mut self, o: u32, et: Ty, ct: Ty, owned: bool) -> StageResult<Vec<u32>> {
        match self.crossing(et, ct)? {
            Cross::Same => {
                let x = self.f.local(Val::I32);
                self.f.get(o);
                self.f.set(x);
                Ok(vec![x])
            }
            Cross::Empty => Ok(Vec::new()),
            Cross::Box(m) => {
                let ls: Vec<u32> = m.leaves.iter().map(|l| self.f.local(l.val)).collect();
                for (leaf, x) in m.leaves.iter().zip(&ls) {
                    self.f.get(o);
                    self.f.op(leaf.load(0));
                    self.f.set(*x);
                }
                if owned {
                    self.retain_locals(&ls, ct)?;
                    self.release_open(o);
                }
                Ok(ls)
            }
            Cross::RefBox { .. } => {
                let Kind::Ref { target: source, .. } = self.kind(et) else {
                    return unsupported("an open reference without its target");
                };
                let view = self.ref_view(o, source)?;
                if owned {
                    self.release_open(o);
                }
                Ok(vec![view])
            }
        }
    }

    /// A thunk takes an owned view; it remains live if the call keeps it.
    fn unerase_arg(
        &mut self,
        o: u32,
        et: Ty,
        ct: Ty,
        _backs: Option<&mut Vec<Back>>,
    ) -> StageResult<Vec<u32>> {
        self.unerase(o, et, ct, true)
    }

    /// Turns the erased frame on the stack, a dynamic call's suspension
    /// whose open result is of type `et`, into the frame of the
    /// `Suspend[T]` value `t` that keeps it: the same frame when a `T`
    /// value is its open value, else a frame over it whose poll unboxes
    /// the result ([`Helper::OpenFrame`]).
    pub(super) fn keep_open(&mut self, et: Ty, t: Ty) -> StageResult<()> {
        let ct = self.susp_inner(t)?;
        let (leaves, reference) = match self.crossing(et, ct)? {
            Cross::Same => return Ok(()),
            Cross::Empty => (Vec::new(), None),
            Cross::Box(m) => (m.leaves, None),
            Cross::RefBox { .. } => {
                let Kind::Ref { target, .. } = self.kind(et) else {
                    return unsupported("a kept open reference without its target");
                };
                (
                    mem_of(self.pool(), self.env(), ct)?.leaves,
                    Some(self.ref_layout(target)?),
                )
            }
        };
        self.f.helper(Helper::OpenFrame {
            part: 0,
            reference,
            leaves,
            walk: self.counted(ct),
            counts: self.input.runtime.is_some(),
        });
        Ok(())
    }

    /// Synchronous callers keep an argument view until return. Cold calls
    /// instead use `erase`, whose ordinary argument ownership keeps it live.
    pub(super) fn erase_arg(
        &mut self,
        r: &[u32],
        et: Ty,
        ct: Ty,
        backs: Option<&mut Vec<Back>>,
    ) -> StageResult<Option<u32>> {
        let Some(backs) = backs else {
            return Ok(None);
        };
        let Cross::RefBox { target } = self.crossing(et, ct)? else {
            return Ok(None);
        };
        let Some(&address) = r.first() else {
            return unsupported("a reference argument without its address");
        };
        let held = self.ref_view(address, target)?;
        backs.push(Back { held, ty: et });
        Ok(Some(held))
    }

    /// Views continuously forward writes; only the caller's count ends here.
    pub(super) fn write_back(&mut self, backs: Vec<Back>) -> StageResult<()> {
        for view in backs {
            self.release_locals(&[view.held], view.ty)?;
        }
        Ok(())
    }

    /// Locals holding value `r`: its instruction's, or a constant's,
    /// pushed into fresh ones.
    pub(super) fn local_value_of(&mut self, r: u32) -> StageResult<Vec<u32>> {
        if r != NONE && Ref(r).as_inst().is_some() {
            return match self.vals.get(&r) {
                Some(ls) => Ok(ls.clone()),
                None => unsupported("a value its instruction did not compute"),
            };
        }
        let vs = self.vals(self.ty_of(r))?;
        let ls: Vec<u32> = vs.into_iter().map(|v| self.f.local(v)).collect();
        self.push(r)?;
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        Ok(ls)
    }

    /// The generic parts of a dynamic call of `method` of `trait_` at
    /// `targs` (the trait's arguments, then the method's own), or `None`
    /// for a method with no parameters of its own.
    pub(super) fn dyn_open(
        &mut self,
        trait_: DefId,
        method: DefId,
        targs: TyList,
    ) -> StageResult<Option<DynOpen>> {
        let Some((params, ret_ty)) = self.erased_sig(trait_, method, targs) else {
            return Ok(None);
        };
        let (pool, env) = (self.input.pool, self.input.env);
        let targs: Vec<Ty> = pool
            .list_items(targs)
            .iter()
            .map(|t| self.sub(*t))
            .collect();
        let n = env.trait_arity(trait_).min(targs.len());
        let ph = |d: DefId| env.path_hash(d);
        let mut memo = CanonMemo::default();
        let family = witness_family(pool, &ph, &mut memo, method, &targs[..n]);
        let name = witness_args(pool, &ph, &mut memo, &targs[n..]);
        let Some((_, _, fields)) = self
            .input
            .collected
            .witnesses
            .witnesses
            .iter()
            .find(|w| w.0 == family && w.1 == name)
        else {
            return unsupported("a dynamic call whose witness collection did not build");
        };
        let mut tables = Vec::new();
        for thunks in fields {
            let slots = thunks
                .iter()
                .map(|k| crate::module::Sym::Inst(*k))
                .collect();
            tables.push(self.m.vtable(slots));
        }
        let witness = self.m.vtable_with(Vec::new(), tables);
        Ok(Some(DynOpen {
            params,
            ret: is_open(pool, ret_ty),
            ret_ty,
            witness,
        }))
    }

    /// The erased result type of the dynamic call in callee record `a`,
    /// when it is open.
    pub(super) fn dyn_open_ret(&self, a: u32) -> StageResult<Option<Ty>> {
        let Some(Callee::TraitMethod {
            trait_,
            method,
            targs,
            ..
        }) = Callee::from_words(self.b.record(a))
        else {
            return unsupported("a dynamic call without a trait method");
        };
        Ok(self
            .erased_sig(trait_, method, targs)
            .map(|s| s.1)
            .filter(|t| is_open(self.pool(), *t)))
    }

    /// A generic method's slot as its erased bodies see it: each
    /// parameter after the receiver's type when it is open, and the
    /// result type. `None` when the method has no type parameters of its
    /// own.
    fn erased_sig(
        &self,
        trait_: DefId,
        method: DefId,
        targs: TyList,
    ) -> Option<(Vec<Option<Ty>>, Ty)> {
        let (pool, env) = (self.input.pool, self.input.env);
        if own_count(env, method) == 0 {
            return None;
        }
        let targs = pool.list_items(targs);
        let n = env.trait_arity(trait_).min(targs.len());
        let mut all = vec![hd_mono::class_ref(pool)];
        all.extend(targs[..n].iter().map(|t| self.sub(*t)));
        all.extend((0..own_count(env, method)).map(|k| open_param(pool, k)));
        let all = pool.list(&all);
        let params = env
            .params(method)
            .unwrap_or_default()
            .into_iter()
            .skip(1)
            .map(|p| {
                let t = subst(pool, env, method, all, p);
                is_open(pool, t).then_some(t)
            })
            .collect();
        let ret = subst(pool, env, method, all, env.ret(method).unwrap_or(Ty::VOID));
        Some((params, ret))
    }

    /// Whether this body's shared cells hold their erased view: those of
    /// a method with type parameters of its own, of a trait or an
    /// implementation, whose erased body builds closures through thunks.
    fn erased_cells(&self) -> bool {
        let env = self.env();
        if own_count(env, self.item) == 0 {
            return false;
        }
        let owner = env.generics_owner(self.item);
        env.parent(owner).is_some_and(|(parent, _)| {
            env.impl_row(parent).is_some() || env.trait_methods(parent).contains(&owner)
        })
    }

    /// The type local `l`'s cell holds: its erased view for a local that
    /// a closure shares in a generic method ([`Em::erased_cells`]), else
    /// its own type.
    pub(super) fn cell_ty(&self, l: usize) -> Ty {
        let t = self.b.local_ty[l];
        let shared = || {
            (0..self.b.len()).any(|ci| {
                self.b.tags[ci] == Tag::Closure
                    && super::closure::captures(self.b, ci)
                        .iter()
                        .any(|c| c.0 == l && c.1)
            })
        };
        if self.erased_cells() && shared() {
            let (pool, env) = (self.input.pool, self.input.env);
            let e = erased_args(pool, env, self.item, self.args);
            return subst(pool, env, self.item, e, t);
        }
        self.sub(t)
    }

    /// Whether local `l`'s cell holds an open value for a concrete one,
    /// so reads unbox and writes box.
    pub(super) fn cell_converts(&self, l: usize) -> bool {
        let pool = self.pool();
        is_open(pool, self.cell_ty(l)) && !is_open(pool, self.sub(self.b.local_ty[l]))
    }

    /// The values `src` pushes, of local `l`'s type, as its cell holds
    /// them: boxed into an open value when the cell converts. The cell
    /// takes over the count `src` holds.
    pub(super) fn cell_src(&mut self, l: usize, src: Src) -> StageResult<Src> {
        if !self.cell_converts(l) {
            return Ok(src);
        }
        let t = self.sub(self.b.local_ty[l]);
        let ls = match src {
            Src::Locals(ls) => ls,
            Src::Ref(r) => self.local_value_of(r)?,
        };
        let o = self.erase(&ls, self.cell_ty(l), t, true)?;
        Ok(Src::Locals(vec![o]))
    }
}
