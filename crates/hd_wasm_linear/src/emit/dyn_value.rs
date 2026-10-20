//! Trait values (`crates/hd_wasm/DESIGN.md` §23.8, §23.19, job 11). A
//! trait value is `(data, vtable)`. The data is the value itself when its
//! layout is one counted address (a list, a map, a data value); any other
//! value is boxed in a counted object of its layout, whose descriptor
//! releases it. Either way the data is one counted address, so a trait
//! value's release and trace are its data's, and the vtable is static: one
//! function-table slot per method, then the addresses of its direct
//! supertraits' vtables.

use hd_base::StageResult;
use hd_mono::{SlotRecv, Target, TargetKind, VTable, erased, subst};
use hd_types::{Ty, TyData};
use wasm_encoder::Instruction as I;

use super::Em;
use super::value::Src;
use crate::layout::{Leaf, Part, Walk, mem as mem_of, payload_mem, walk};
use crate::module::{HEAP, Ins, Sym, Val, mem};
use crate::rt::{Helper, RefRead, TypeIdParts};
use crate::unsupported;

impl Em<'_, '_, '_> {
    fn is_dyn(&self, t: Ty) -> bool {
        matches!(
            self.pool().get(self.strip_mut(t)),
            TyData::TraitValue { .. }
        )
    }

    pub(super) fn dyn_value_ty(&self, mut ty: Ty) -> Ty {
        loop {
            match self.pool().get(ty) {
                TyData::Mut(inner) => ty = inner,
                TyData::Adt { def, args } if def == self.env().ref_cell() => {
                    ty = self.pool().list_items(args)[0];
                }
                _ => return ty,
            }
        }
    }

    /// Reads reference getters, releasing each owned intermediate reference.
    pub(super) fn read_dyn_value(&mut self, value: u32) -> StageResult<(Ty, Vec<u32>)> {
        let mut ty = self.strip_mut(self.ty_of(value));
        let read = self.ref_read(ty)?;
        let mut locals = self.read_through(value, &read)?;
        loop {
            let TyData::Adt { args, .. } = self.pool().get(ty) else {
                return unsupported("a trait reference without a Ref layout");
            };
            ty = self.pool().list_items(args)[0];
            if let TyData::Mut(inner) = self.pool().get(ty) {
                ty = inner;
            }
            if self.is_dyn(ty) {
                return Ok((ty, locals));
            }
            let read = self.ref_read(ty)?;
            self.f.get(locals[0]);
            self.f.op(I::I32Load(mem(HEAP, u64::from(read.env), 2)));
            self.f.get(locals[0]);
            self.f.op(I::I32Load(mem(HEAP, u64::from(read.code), 2)));
            self.f
                .body
                .push(Ins::CallIndirect(vec![Val::I32], read.value.clone()));
            let next: Vec<u32> = read.value.iter().map(|v| self.f.local(*v)).collect();
            for l in next.iter().rev() {
                self.f.set(*l);
            }
            self.release_locals(&locals, ty)?;
            locals = next;
        }
    }

    /// `Coerce` of value `a` to the trait value `i` (`ToTraitValue`,
    /// `Supertrait`): a trait value widens; any other value takes the
    /// vtable collection recorded at its type.
    pub(super) fn trait_value(&mut self, i: u32, a: u32) -> StageResult<()> {
        let from = self.strip_mut(self.ty_of(a));
        let sealed = self.env().decls().sealed();
        let sealed_identity = matches!(self.pool().get(self.strip_mut(self.ty(i))), TyData::TraitValue { def, .. } if def == sealed.inspectable || def == sealed.any);
        if self.is_dyn(from)
            || ((!sealed_identity
                || hd_mono::trait_reference_widens(
                    self.pool(),
                    self.env(),
                    from,
                    self.strip_mut(self.ty(i)),
                ))
                && self.is_dyn(self.dyn_value_ty(from)))
        {
            return self.widen(i, a);
        }
        let Some(Target::VTable(table)) = self.calls.get(&i).cloned() else {
            return unsupported("a trait-value coercion that collection did not record");
        };
        let vs = self.vals(from)?;
        let direct = vs == [Val::I32] && walk(self.pool(), self.env(), from) == Some(Walk::Ref(0));
        if direct || vs.is_empty() {
            // A value of no Wasm values needs no box: its slots load
            // nothing. A value that is one counted address is the data
            // itself, never a copy, as the Wasm GC backend erases it, so
            // an element reference's identity base is its list's header.
            let leaves: Option<&[Leaf]> = if direct {
                if self.is_pin(a) {
                    self.list_addr(a)?;
                } else {
                    self.comp(a, 0)?;
                }
                None
            } else {
                self.f.i32(0);
                Some(&[])
            };
            let vt = self.vtable_of(&table, leaves, from)?;
            self.f.body.push(Ins::VTable(vt));
            return self.store(i);
        }
        let src = self.slot_src(a, from, false)?;
        let m = mem_of(self.pool(), self.env(), from)?;
        let w = self.counted(from);
        let at = self.object(&m, w.as_ref());
        let part = Part {
            first: 0,
            count: m.leaves.len(),
            offset: 0,
        };
        self.store_part(at, &m, part, &src)?;
        self.retain_fields(at, &m, w.as_ref());
        let vt = self.vtable_of(&table, Some(&m.leaves), from)?;
        self.f.get(at);
        self.f.body.push(Ins::VTable(vt));
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
        let read = if table.recv.contains(&SlotRecv::ReadThrough) {
            Some(self.ref_read(from)?)
        } else {
            None
        };
        let methods = self.env().trait_methods(table.trait_);
        let mut slots = Vec::new();
        for ((m, t), recv) in methods.iter().zip(&table.slots).zip(&table.recv) {
            let (pool, env) = (self.pool(), self.env());
            let mut params = Vec::new();
            for p in env.params(t.item).unwrap_or_default().into_iter().skip(1) {
                params.extend(self.vals(subst(pool, env, t.item, t.args, p))?);
            }
            for _ in env.row_keys(t.item, t.args) {
                params.extend([Val::I32, Val::I32]);
            }
            // A suspending method's slot builds its cold frame.
            let results = if env.suspends(t.item) {
                vec![Val::I32]
            } else {
                self.vals(t.ret)?
            };
            // Generic erased methods and compiler-supplied methods still
            // use their existing dynamic-call lowering restrictions.
            let traps = erased::own_count(env, *m) > 0 || t.kind != TargetKind::Instance;
            // The vtable's `runtime_type` answers the type the vtable is
            // built at (trait.inspect.dynamic).
            if let TargetKind::Builtin { self_ty, .. } = t.kind
                && (self.input.path)(*m).rsplit(['.', '/']).next() == Some("runtime_type")
            {
                let recorded = match self.pool().get(self_ty) {
                    TyData::Adt { def, args } if def == self.env().ref_cell() => {
                        self.pool().list_items(args)[0]
                    }
                    _ => self_ty,
                };
                let parts = self.type_id_parts(recorded, t.ret)?;
                slots.push(Sym::Helper(Helper::RuntimeType(parts)));
                continue;
            }
            slots.push(Sym::Helper(if traps {
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
                }
            } else {
                Helper::DynSlot {
                    target: t.key,
                    leaves: leaves.map(<[Leaf]>::to_vec),
                    params,
                    results,
                }
            }));
        }
        Ok(self.m.vtable_with(slots, parents))
    }

    /// How a slot reads the value behind the reference payload of type
    /// `from` (`SlotRecv::ReadThrough`): where the `get` closure lies in
    /// the `Ref` object, and the value's layout and counted references.
    pub(super) fn ref_read(&self, from: Ty) -> StageResult<RefRead> {
        let (pool, env) = (self.pool(), self.env());
        let TyData::Adt { def, args } = pool.get(from) else {
            return unsupported("a reference payload that is not a `Ref`");
        };
        let fields: Vec<Ty> = env
            .data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| subst(pool, env, def, args, f))
            .collect();
        let (Some(&value), Some(&get)) = (pool.list_items(args).first(), fields.first()) else {
            return unsupported("a `Ref` without its `get` field");
        };
        let (m, parts) = payload_mem(pool, env, &fields)?;
        let p = parts[0];
        if self.vals(get)? != [Val::I32, Val::I32] {
            return unsupported("a `Ref` whose `get` is not a function value");
        }
        Ok(RefRead {
            code: m.leaves[p.first].offset,
            env: m.leaves[p.first + 1].offset,
            value: self.vals(value)?,
            walk: self.counted(value),
        })
    }

    /// The value behind reference `r`, read through its `get` closure as
    /// [`Self::ref_read`] describes: locals holding it, with a count of
    /// its own.
    pub(super) fn read_through(&mut self, r: u32, read: &RefRead) -> StageResult<Vec<u32>> {
        self.comp(r, 0)?;
        self.f.op(I::I32Load(mem(HEAP, u64::from(read.env), 2)));
        self.comp(r, 0)?;
        self.f.op(I::I32Load(mem(HEAP, u64::from(read.code), 2)));
        self.f
            .body
            .push(Ins::CallIndirect(vec![Val::I32], read.value.clone()));
        let ls: Vec<u32> = read.value.iter().map(|v| self.f.local(*v)).collect();
        for l in ls.iter().rev() {
            self.f.set(*l);
        }
        Ok(ls)
    }

    /// `RefAsProvider` (`crates/hd_wasm/DESIGN.md` §15.2, "Providers of
    /// reference traits"): the trait value reference `a` installed as a
    /// provider. Its data is the `Ref` itself, which `ProviderAsRef` reads
    /// back; its vtable is the one `get` returns, there to fill the
    /// provider's layout, and the trait value `get` built is let go.
    pub(super) fn ref_as_provider(&mut self, i: u32, a: u32) -> StageResult<()> {
        let read = self.ref_read(self.strip_mut(self.ty_of(a)))?;
        if read.value != [Val::I32, Val::I32] {
            return unsupported("a provider reference of an unexpected layout");
        }
        let ls = self.read_through(a, &read)?;
        if self.input.runtime.is_some() {
            self.f.get(ls[0]);
            self.f.helper(Helper::ReleaseRef);
        }
        self.comp(a, 0)?;
        self.f.get(ls[1]);
        self.store(i)
    }

    /// The `TypeId` struct `id` (`runtime_type`'s result) of `t`: its key
    /// and its printable name (trait.typeid.name), static strings.
    fn type_id_parts(&self, t: Ty, id: Ty) -> StageResult<TypeIdParts> {
        if self.vals(id)? != [Val::I32; 4] {
            return unsupported("a `TypeId` that is not its key and name strings");
        }
        let (pool, env) = (self.pool(), self.env());
        Ok(TypeIdParts {
            name: hd_mono::type_name(pool, env, t),
            shown: hd_mono::printed_type_name(pool, env, t),
        })
    }

    /// The builtin `runtime_type` of a `self_ty` value `recv`: a concrete
    /// value's recorded type is its static type (trait.inspect.static); a
    /// trait value's is its vtable's `runtime_type` (trait.inspect.dynamic).
    pub(super) fn runtime_type(&mut self, i: u32, self_ty: Ty, recv: u32) -> StageResult<()> {
        if !self.is_dyn(self_ty) {
            let parts = self.type_id_parts(self_ty, self.ty(i))?;
            self.f.helper(Helper::TypeIdOf(parts));
            return self.store(i);
        }
        let sealed = self.env().decls().sealed();
        let mut path = Vec::new();
        let mut found = None;
        self.super_path(
            self_ty,
            (sealed.inspectable, hd_types::TyList::EMPTY),
            &mut path,
            &mut found,
        );
        let Some(steps) = found else {
            return unsupported("a `runtime_type` of a trait value that is not `Inspectable`");
        };
        self.comp(recv, 0)?;
        self.comp(recv, 1)?;
        for at in steps {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f
            .body
            .push(Ins::CallIndirect(vec![Val::I32], vec![Val::I32; 4]));
        self.store(i)
    }

    /// `TypeId::of::[T]()` (`type_id_of`): a new `TypeId` of `T`.
    pub(super) fn type_id_of(&mut self, i: u32, targs: hd_types::TyList) -> StageResult<()> {
        let Some(&t) = self.pool().list_items(targs).last() else {
            return unsupported("a `TypeId::of` without its type argument");
        };
        let parts = self.type_id_parts(self.sub(t), self.ty(i))?;
        self.f.helper(Helper::TypeIdOf(parts));
        self.store(i)
    }

    /// The `Inspectable` trait of recovery intrinsic `item` and its one
    /// method, `runtime_type`.
    fn inspectable_of(
        &self,
        item: hd_base::DefId,
    ) -> StageResult<(hd_base::DefId, hd_base::DefId)> {
        let env = self.env();
        let Some((owner, _)) = env.parent(item) else {
            return unsupported("a recovery intrinsic without an inherent impl");
        };
        let Some((target, _, _)) = env.impl_head(owner) else {
            return unsupported("a recovery intrinsic outside an inherent impl");
        };
        let mut todo = vec![target];
        let mut seen = Vec::new();
        while let Some(t) = todo.pop() {
            let TyData::TraitValue { def, .. } = self.pool().get(t) else {
                continue;
            };
            if seen.contains(&def) {
                continue;
            }
            seen.push(def);
            for method in env.trait_methods(def) {
                if (self.input.path)(method).rsplit(['.', '/']).next() == Some("runtime_type") {
                    return Ok((def, method));
                }
            }
            todo.extend_from_slice(env.decls().supertraits(def));
        }
        unsupported("a recovery intrinsic without the Inspectable dictionary")
    }

    /// Recover the original reference payload after an exact type test.
    pub(super) fn downcast(&mut self, i: u32, item: hd_base::DefId, recv: u32) -> StageResult<()> {
        let TyData::Option(target) = self.pool().get(self.strip_mut(self.ty(i))) else {
            return unsupported("a downcast whose result is not optional");
        };
        let TyData::Adt { def, args } = self.pool().get(target) else {
            return unsupported("a recovered target that is not a reference");
        };
        if def != self.env().ref_cell() {
            return unsupported("a recovered target that is not a reference");
        }
        let concrete = self.pool().list_items(args)[0];
        let want = hd_mono::type_name(self.pool(), self.env(), concrete);
        let recv_t = self.strip_mut(self.ty_of(recv));
        let value_t = match self.pool().get(recv_t) {
            TyData::TraitValue { .. } => recv_t,
            TyData::Adt { def, args } if def == self.env().ref_cell() => {
                let Some(&inner) = self.pool().list_items(args).first() else {
                    return unsupported("a reference without its target type");
                };
                self.strip_mut(inner)
            }
            _ => return unsupported("a downcast receiver that is not a reference"),
        };
        if !self.is_dyn(value_t) {
            if hd_mono::type_name(self.pool(), self.env(), recv_t)
                == hd_mono::type_name(self.pool(), self.env(), target)
            {
                return self.new_variant(i, 1, &[Src::Ref(recv)]);
            }
            return self.new_variant(i, 0, &[]);
        }
        let (data, table) = if value_t == recv_t {
            let (data, table) = (self.f.local(Val::I32), self.f.local(Val::I32));
            self.comp(recv, 0)?;
            self.f.set(data);
            self.comp(recv, 1)?;
            self.f.set(table);
            (data, table)
        } else {
            let read = self.ref_read(recv_t)?;
            let payload = self.read_through(recv, &read)?;
            let [data, table] = payload.as_slice() else {
                return unsupported("a trait reference getter without payload and vtable");
            };
            (*data, *table)
        };
        let (insp, rt) = self.inspectable_of(item)?;
        let mut path = Vec::new();
        let mut found = None;
        self.super_path(
            value_t,
            (insp, hd_types::TyList::EMPTY),
            &mut path,
            &mut found,
        );
        let Some(steps) = found else {
            return unsupported("a downcast of a trait value that is not `Inspectable`");
        };
        let Some(id_ty) = self.env().ret(rt) else {
            return unsupported("a `runtime_type` without its result");
        };
        self.type_id_parts(concrete, id_ty)?;
        // The payload's recorded type, then its key against `T`'s; the
        // name is dropped, and neither is counted.
        self.f.get(data);
        self.f.get(table);
        for at in steps {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        self.f.op(I::I32Load(mem(HEAP, 0, 2)));
        self.f
            .body
            .push(Ins::CallIndirect(vec![Val::I32], vec![Val::I32; 4]));
        self.f.op(I::Drop);
        self.f.op(I::Drop);
        let at = self.m.static_bytes(want.as_bytes(), 1);
        self.f.u32(at);
        self.f.u32(u32::try_from(want.len()).expect("type name"));
        self.f.helper(Helper::StrEq);
        let same = self.f.local(Val::I32);
        self.f.set(same);
        // The payload as `trait_value` stored it: the data itself, no
        // values, or a box of the target's layout.
        let vs = self.vals(target)?;
        let direct =
            vs == [Val::I32] && walk(self.pool(), self.env(), target) == Some(Walk::Ref(0));
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
        // The getter returned an owned trait payload; the optional result
        // retained its reference on success, so release this temporary in both arms.
        if value_t != recv_t && self.input.runtime.is_some() {
            self.f.get(data);
            self.f.helper(Helper::ReleaseRef);
        }
        Ok(())
    }

    /// Widening a trait value to a supertrait's (trait.dyn.widen): the
    /// data unchanged, and the supertrait's vtable read through the parent
    /// addresses of the value's.
    fn widen(&mut self, i: u32, a: u32) -> StageResult<()> {
        let source = self.strip_mut(self.ty_of(a));
        let owned = if self.is_dyn(source) {
            None
        } else {
            Some(self.read_dyn_value(a)?)
        };
        let from = owned.as_ref().map_or(source, |v| v.0);
        let to = self.strip_mut(self.ty(i));
        let TyData::TraitValue { def, args, .. } = self.pool().get(to) else {
            return unsupported("a supertrait coercion to a type that is not a trait value");
        };
        // A `dyn Any` is laid out as a `dyn Inspectable` (`hd_mono`'s
        // vtable of a coercion to `Any`).
        let sealed = self.env().decls().sealed();
        let to = if def == sealed.any && sealed.inspectable != hd_base::DefId::NONE {
            (sealed.inspectable, hd_types::TyList::EMPTY)
        } else {
            (def, args)
        };
        let mut path = Vec::new();
        let mut found = None;
        self.super_path(from, to, &mut path, &mut found);
        let Some(path) = found else {
            return unsupported("a supertrait coercion to a trait the value does not extend");
        };
        if let Some((_, value)) = &owned {
            self.f.get(value[0]);
            self.f.get(value[1]);
        } else {
            self.comp(a, 0)?;
            self.comp(a, 1)?;
        }
        for at in path {
            self.f.op(I::I32Load(mem(HEAP, u64::from(at), 2)));
        }
        if owned.is_some() {
            self.store_owned(i)
        } else {
            self.store(i)
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
    ) -> StageResult<Vec<u32>> {
        let pool = self.pool();
        let targs: Vec<Ty> = pool
            .list_items(targs)
            .iter()
            .map(|t| self.sub(*t))
            .collect();
        let n = self.env().trait_arity(trait_).min(targs.len());
        let want = pool.list(&targs[..n]);
        let mut path = Vec::new();
        let mut found = None;
        self.super_path(self.strip_mut(recv), (trait_, want), &mut path, &mut found);
        match found {
            Some(steps) => Ok(steps),
            None => {
                unsupported("a trait-value call of a method of a trait the value does not extend")
            }
        }
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
        let own = u32::try_from(env.trait_methods(def).len()).expect("slots");
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
