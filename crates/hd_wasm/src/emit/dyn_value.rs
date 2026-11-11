//! Trait value references (`crates/hd_wasm/DESIGN.md` §23.8,
//! §23.19). Their `(data, vtable)` pair preserves the concrete referent's
//! address and names a static dispatch table. Local references borrow the
//! referent; counted references retain their owner. The vtable holds each
//! declared method's adapter slot, then direct supertraits' table addresses.
//! Adapters read the receiver through its address using the method's ABI.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::{SlotRecv, Target, TargetKind, VTable, subst};
use hd_types::{Ty, TyData};

use super::Em;
use super::value::Src;
use crate::layout::{Leaf, Part, mem as mem_of, walk};
use crate::module::{HEAP, Ins, Val, mem};
use crate::rt::{Helper, RefRead, TypeIdParts};

impl Em<'_, '_, '_> {
    fn is_dyn(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::TraitValue { .. })
    }

    pub(super) fn dyn_value_ty(&self, mut ty: Ty) -> Ty {
        while let TyData::Ref { inner, .. } = self.pool().get(ty) {
            ty = inner;
        }
        ty
    }

    /// The trait value that `value`, a reference to one, perhaps through
    /// more references, refers to: its type, locals holding it, and
    /// whether they were loaded from storage. A reference to a trait value is
    /// that trait value (`refs.rs`), lent; one read through an address is
    /// a copy whose caller provides its count.
    pub(super) fn read_dyn_value(&mut self, value: u32) -> StageResult<(Ty, Vec<u32>, bool)> {
        let mut ty = self.ty_of(value);
        // Both callers test that `dyn_value_ty` strips references from
        // `value`'s type to a trait value.
        let target = self.dyn_value_ty(ty);
        let mut ls = self.computed(value);
        let mut loaded = false;
        // Each thin reference is the address of the next one.
        while let Some((next_ty, false)) = self.ref_target(ty) {
            let m = mem_of(self.pool(), self.env(), next_ty)?;
            let next: Vec<u32> = m.leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, x) in m.leaves.iter().zip(&next) {
                self.f.get(ls[0]);
                self.f.op(l.load(0));
                self.f.set(*x);
            }
            ls = next;
            ty = next_ty;
            loaded = true;
        }
        Ok((target, ls, loaded))
    }

    /// `Coerce` of value `a` to the trait value `i` (`Supertrait`,
    /// `RefToTraitValue`): a value takes the vtable
    /// collection recorded at its type; a trait value, which collection
    /// gives none, widens.
    pub(super) fn trait_value(&mut self, i: u32, a: u32) -> StageResult<()> {
        let from = self.ty_of(a);
        let Some(Target::VTable(table)) = self.calls.get(&i).cloned() else {
            return self.widen(i, a);
        };
        let vs = self.vals(from)?;
        let direct = vs == [Val::I32]
            && walk(self.pool(), self.env(), from).and_then(|w| w.single()) == Some(0);
        if direct || vs.is_empty() {
            // A value of no Wasm values needs no box: its slots load
            // nothing. A value that is one counted address is the data
            // itself, never a copy, so
            // an element reference's identity base is its list's buffer.
            let leaves: Option<&[Leaf]> = if direct {
                self.comp(a, 0)?;
                None
            } else {
                self.f.i32(0);
                Some(&[])
            };
            let vt = self.vtable_of(&table, leaves, from)?;
            self.f.push(Ins::VTable(vt));
            return self.store_as(i, self.has_stated_retain(i));
        }
        let src = Src::Ref(a);
        let m = mem_of(self.pool(), self.env(), from)?;
        let w = self.counted(from);
        let cyclic = self.cyclic_holding(&[from]);
        let at = self.object(&m, w.as_ref(), cyclic, false, Some(from));
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, &src)?;
        let colls = self.copied(from);
        self.retain_fields(at, &m, w.as_ref(), &colls, colls.len());
        let vt = self.vtable_of(&table, Some(&m.leaves), from)?;
        self.f.get(at);
        self.f.push(Ins::VTable(vt));
        // A new box arrives with its count.
        self.store_owned(i)
    }

    /// The static vtable of `table` at payload type `from`, whose slots
    /// load the payload from a box of `leaves`, or pass the data itself
    /// when there is no box.
    fn vtable_of(
        &mut self,
        table: &VTable,
        leaves: Option<&[Leaf]>,
        from: Ty,
    ) -> StageResult<usize> {
        let mut parents = Vec::new();
        for p in &table.parents {
            parents.push(self.vtable_of(p, leaves, from)?);
        }
        // Collection reads through only a payload that is a reference.
        let read = match self.kind(from) {
            crate::layout::Kind::Ref { target, .. }
                if table.recv.contains(&SlotRecv::ReadThrough) =>
            {
                Some(RefRead {
                    value: self.ref_layout(target)?,
                })
            }
            _ => None,
        };
        let methods = hd_mono::slot_methods(self.env(), table.trait_);
        let mut slots = Vec::new();
        for ((m, t), recv) in methods.iter().zip(&table.slots).zip(&table.recv) {
            let env = self.env();
            let mut params = Vec::new();
            // Counts the target needs from the slot's lent receiver
            // and borrowed arguments. Owned cold parameters and inline
            // Drop values arrive with their counts.
            let mut own = Vec::new();
            // Value arguments the call hands in but its target only
            // borrows, released once it has returned, with Drop hooks.
            let mut given = Vec::new();
            let tys = super::callee_params(self.input, self.m, t.key, t.item, t.args);
            for (k, mut pt) in tys.into_iter().enumerate() {
                if k == 0 {
                    // The receiver: a reference, or the value the slot
                    // passes, the payload or what its reference reads.
                    if matches!(self.kind(pt), crate::layout::Kind::Ref { .. }) {
                        own.push((0, None));
                        continue;
                    }
                    pt = match (recv, self.kind(from)) {
                        (SlotRecv::ReadThrough, crate::layout::Kind::Ref { target, .. }) => target,
                        _ => from,
                    };
                }
                let cold = env.cold_transfers(t.item, pt);
                let target_pass = if cold {
                    self.pass(pt)
                } else {
                    self.pass_at(env.lends(t.item, t.args), k, pt)
                };
                let w = match target_pass {
                    crate::layout::Pass::Owned(w)
                        if !env.suspends(t.item)
                            || cold
                            || crate::layout::holds_drop(self.pool(), env, pt) =>
                    {
                        Some(w)
                    }
                    _ => None,
                };
                if k == 0 {
                    own.push((0, w));
                    continue;
                }
                let vs = self.vals(pt)?;
                let n = u32::try_from(vs.len()).expect("values");
                if matches!(self.pass(pt), crate::layout::Pass::Owned(_))
                    && (env.cold_transfers(*m, pt)
                        || crate::layout::holds_drop(self.pool(), env, pt))
                {
                    if w.is_none() {
                        given.push((u32::try_from(k).expect("params"), self.ref_layout(pt)?));
                    }
                    own.push((n, None));
                } else {
                    own.push((n, w));
                }
                params.extend(vs);
            }
            // A method with a row takes its caller's context last
            // (`ctx.rs`), in which the slot looks up its target's keys.
            let ctx = if super::ctx::slot_ctx(self.pool(), env, *m) {
                params.push(Val::I32);
                let keys = hd_mono::declared_row_keys(self.pool(), env, t.item, t.args);
                Some(crate::rt::SlotCtx {
                    ids: keys
                        .into_iter()
                        .map(|k| super::ctx::key_id(self.input, k))
                        .collect(),
                    forward: hd_mono::has_row_params(env, t.item),
                })
            } else {
                None
            };
            // A suspending method's slot builds its cold frame.
            let results = if env.suspends(t.item) {
                vec![Val::I32]
            } else {
                self.vals(t.ret)?
            };
            // A compiler-supplied method has no instance to call.
            let traps = t.kind != TargetKind::Instance;
            // The vtable's `runtime_type` answers the type the vtable is
            // built at (trait.inspect.dynamic).
            if let TargetKind::Builtin { self_ty, .. } = t.kind
                && (self.input.path)(*m).rsplit(['.', '/']).next() == Some("runtime_type")
            {
                let recorded = match self.pool().get(self_ty) {
                    TyData::Ref { inner, .. } => inner,
                    _ => self_ty,
                };
                let parts = self.type_id_parts(recorded);
                slots.push(Helper::RuntimeType(parts));
                continue;
            }
            slots.push(if traps {
                let mut all = vec![Val::I32];
                all.extend(params);
                Helper::DynTrap {
                    params: all,
                    results,
                }
            } else if let (SlotRecv::ReadThrough, Some(read)) = (recv, &read) {
                Helper::DynRead {
                    target: t.key,
                    read: read.clone(),
                    params,
                    results,
                    own,
                    given,
                    ctx,
                }
            } else {
                Helper::DynSlot {
                    target: t.key,
                    leaves: leaves.map(<[Leaf]>::to_vec),
                    params,
                    results,
                    own,
                    given,
                    ctx,
                }
            });
        }
        crate::drive::vtable(self.input, self.m, table.trait_, slots, parents)
    }

    /// `RefAsProvider` (`crates/hd_wasm/DESIGN.md` §15.2, "Providers of
    /// reference traits"): the trait value reference `a` installed as a
    /// provider. A reference to a trait value is that trait value
    /// (`refs.rs`), so the provider is `a` itself, which `ProviderAsRef`
    /// reads back. Checking coerces `a` to a reference to the key's trait
    /// value first.
    pub(super) fn ref_as_provider(&mut self, i: u32, a: u32) -> StageResult<()> {
        self.push(a)?;
        self.store_owned(i)
    }

    /// The canonical descriptor for `t`, addressed by the one-word `TypeId`
    /// (std's `struct TypeId: info: Ptr[TypeInfo]`).
    fn type_id_parts(&self, t: Ty) -> TypeIdParts {
        let (pool, env) = (self.pool(), self.env());
        TypeIdParts {
            name: hd_mono::type_name(pool, env, t),
            shown: hd_mono::printed_type_name(pool, env, t),
        }
    }

    /// The builtin `runtime_type` of a `self_ty` value `recv`: a concrete
    /// value's recorded type is its static type (trait.inspect.static); a
    /// trait value's is its vtable's `runtime_type` (trait.inspect.dynamic).
    pub(super) fn runtime_type(&mut self, i: u32, self_ty: Ty, recv: u32) -> StageResult<()> {
        if !self.is_dyn(self_ty) {
            let parts = self.type_id_parts(self_ty);
            self.f.helper(Helper::TypeIdOf(parts));
            return self.store(i);
        }
        // Checking gives a trait value `runtime_type` only when its trait
        // extends `Inspectable`.
        let steps = self.inspectable_steps(self_ty);
        self.comp(recv, 0)?;
        self.comp(recv, 1)?;
        for at in steps {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.call_indirect(vec![Val::I32], vec![Val::I32]);
        self.store(i)
    }

    /// `TypeId::of::[T]()` (`type_id_of`): a new `TypeId` of `T`.
    pub(super) fn type_id_of(&mut self, i: u32, targs: hd_types::TyList) -> StageResult<()> {
        // `TypeId::of[T]`.
        let t = self.pool().list_items(targs)[0];
        let parts = self.type_id_parts(self.sub(t));
        self.f.helper(Helper::TypeIdOf(parts));
        self.store(i)
    }

    /// Recover the original reference payload after an exact type test.
    /// `downcast_ref[T](&self) -> (&T)?` and `downcast_mut`, of an
    /// `impl dyn Inspectable` or `impl dyn Error` in std.
    pub(super) fn downcast(
        &mut self,
        i: u32,
        targs: hd_types::TyList,
        recv: u32,
    ) -> StageResult<()> {
        let Some(target) = self.pool().sole_arg(self.ty(i), self.env().option()) else {
            // ruled out: the intrinsic's shape (`Opt`) checks its result.
            unreachable!("a recovery intrinsic returns `(&T)?`");
        };
        let concrete = self.sub(self.pool().list_items(targs)[0]);
        let recv_t = self.ty_of(recv);
        // The receiver is a trait value or a reference to one.
        let value_t = self.ref_target(recv_t).map_or(recv_t, |(t, _)| t);
        if !self.is_dyn(value_t) {
            if hd_mono::type_name(self.pool(), self.env(), recv_t)
                == hd_mono::type_name(self.pool(), self.env(), target)
            {
                return self.new_variant(i, 1, &[Src::Ref(recv)]);
            }
            return self.new_variant(i, 0, &[]);
        }
        // A reference to a trait value is that trait value (`refs.rs`).
        let (data, table) = (self.f.local(Val::I32), self.f.local(Val::I32));
        self.comp(recv, 0)?;
        self.f.set(data);
        self.comp(recv, 1)?;
        self.f.set(table);
        // `Inspectable` and `Error < Inspectable` reach `Inspectable`.
        let steps = self.inspectable_steps(value_t);
        let want = self.type_id_parts(concrete);
        // The payload's recorded descriptor and T's canonical descriptor.
        self.f.get(data);
        self.f.get(table);
        for at in steps {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f.call_indirect(vec![Val::I32], vec![Val::I32]);
        self.f.helper(Helper::TypeInfo(want));
        self.f.op(I::I32Eq);
        let same = self.f.local(Val::I32);
        self.f.set(same);
        // The payload as `trait_value` stored it: the data itself, no
        // values, or a box of the target's layout.
        let vs = self.vals(target)?;
        let direct = vs == [Val::I32]
            && walk(self.pool(), self.env(), target).and_then(|w| w.single()) == Some(0);
        let m = mem_of(self.pool(), self.env(), target)?;
        self.f.get(same);
        self.f.op(I::If(wasm_encoder::BlockType::Empty));
        self.ctl.push(super::Ctl::Plain);
        let ls: Vec<u32> = if direct {
            vec![data]
        } else {
            let ls: Vec<u32> = m.leaves.iter().map(|l| self.f.local(l.val)).collect();
            for (l, x) in m.leaves.iter().zip(&ls) {
                self.f.get(data);
                self.f.op(l.load(0));
                self.f.set(*x);
            }
            ls
        };
        self.new_variant(i, 1, &[Src::Locals(ls)])?;
        self.f.op(I::Else);
        self.new_variant(i, 0, &[])?;
        self.close();
        Ok(())
    }

    /// Widening a trait value to a supertrait's (trait.dyn.widen): the
    /// data unchanged, and the supertrait's vtable read through the parent
    /// addresses of the value's.
    fn widen(&mut self, i: u32, a: u32) -> StageResult<()> {
        let source = self.ty_of(a);
        let read = if self.is_dyn(source) {
            None
        } else {
            Some(self.read_dyn_value(a)?)
        };
        if let Some((target, ls, true)) = &read {
            // ruled out: ownership states the result Retain when widening
            // loads an owned dynamic receiver; emission never invents a count.
            assert!(
                self.has_stated_retain(i),
                "loaded dyn widen has a stated Retain"
            );
            // Lower the adjacent Retain before the supertrait vtable loads.
            self.because(hd_tir::own::Reason::Stored, |em| {
                em.retain_locals(ls, *target)
            })?;
            self.retained_before_result.insert(i);
        }
        let from = read.as_ref().map_or(source, |v| v.0);
        let to = self.ty(i);
        let TyData::TraitValue { def, args, .. } = self.pool().get(to) else {
            unreachable!("a trait-value coercion is to a trait value");
        };
        // A `dyn Any` is laid out as a `dyn Inspectable` (`hd_mono`'s
        // vtable of a coercion to `Any`).
        let sealed = self.env().decls().sealed();
        let to = if def == sealed.any && sealed.inspectable != hd_base::DefId::NONE {
            (sealed.inspectable, hd_types::TyList::EMPTY)
        } else {
            (def, args)
        };
        // Checking widens only to a supertrait (trait.dyn.widen).
        let path = self.steps(from, to);
        if let Some((_, value, _)) = &read {
            self.f.get(value[0]);
            self.f.get(value[1]);
        } else {
            self.comp(a, 0)?;
            self.comp(a, 1)?;
        }
        for at in path {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        if read.as_ref().is_some_and(|r| r.2) {
            self.store_owned(i)
        } else {
            self.store_as(i, self.has_stated_retain(i))
        }
    }

    /// The loads from the vtable of a receiver of type `recv` to that of
    /// its trait `trait_` (itself or a supertrait), whose arguments lead
    /// `targs`: a call of a supertrait's method dispatches through the
    /// supertrait's vtable.
    pub(super) fn dyn_steps(
        &self,
        recv: Ty,
        trait_: hd_base::DefId,
        targs: hd_types::TyList,
    ) -> Vec<u32> {
        let pool = self.pool();
        let targs: Vec<Ty> = pool
            .list_items(targs)
            .iter()
            .map(|t| self.sub(*t))
            .collect();
        let n = self.env().trait_arity(trait_).min(targs.len());
        let want = pool.list(&targs[..n]);
        // Checking finds a trait value's methods in its trait and supertraits.
        self.steps(recv, (trait_, want))
    }

    /// The loads from the vtable of trait value type `at` to its
    /// `Inspectable` one.
    fn inspectable_steps(&self, at: Ty) -> Vec<u32> {
        let sealed = self.env().decls().sealed();
        self.steps(at, (sealed.inspectable, hd_types::TyList::EMPTY))
    }

    /// The loads from the vtable of trait value type `at` to that of `to`,
    /// a trait it extends: each caller's checked operation names one.
    fn steps(&self, at: Ty, to: (hd_base::DefId, hd_types::TyList)) -> Vec<u32> {
        let mut found = None;
        self.super_path(at, to, &mut Vec::new(), &mut found);
        found.expect("a trait value's trait extends the trait its operation names")
    }

    /// The offsets of the parent addresses from trait value type `at`'s
    /// vtable to the vtable of `to`: the first path whose arguments are
    /// `to`'s, else the first to its trait.
    fn super_path(
        &self,
        at: Ty,
        to: (hd_base::DefId, hd_types::TyList),
        path: &mut Vec<u32>,
        found: &mut Option<Vec<u32>>,
    ) -> bool {
        let (pool, env) = (self.pool(), self.env());
        let TyData::TraitValue { def, args, .. } = pool.get(at) else {
            return false;
        };
        if def == to.0 {
            if args == to.1 {
                *found = Some(path.clone());
                return true;
            }
            if found.is_none() {
                *found = Some(path.clone());
            }
        }
        // ruled out: a vtable fits Wasm32 memory, with four bytes per slot.
        let own = u32::try_from(hd_mono::slot_methods(env, def).len()).expect("slots");
        let mut full = vec![at];
        full.extend(pool.list_items(args).iter().copied());
        let full = pool.list(&full);
        for (k, sup) in env
            .decls()
            .supertraits(def)
            .to_vec()
            .into_iter()
            .enumerate()
        {
            let s = subst(pool, env, def, full, sup);
            path.push(4 * (own + u32::try_from(k).expect("supertraits")));
            let exact = self.super_path(s, to, path, found);
            path.pop();
            if exact {
                return true;
            }
        }
        false
    }
}
