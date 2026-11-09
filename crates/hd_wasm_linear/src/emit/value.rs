//! Structs, tuples and enums (`crates/hd_wasm_linear/DESIGN.md` §23.3,
//! job 6). A struct, a tuple and a non-recursive enum are their values in
//! locals ([`crate::layout::mem`]): construction pushes them, a field read
//! picks some. A self-recursive enum is the address of a cell in the heap,
//! which holds the enum's values at their offsets.

use crate::op::Op as I;
use hd_base::StageResult;
use hd_mono::layout::{CanonMemo, canon};
use hd_types::{Ty, TyData};
use std::rc::Rc;

use super::Em;
use crate::layout::{
    Discr, Flat, Is, Kind, Leaf, Mem, Part, Step, Test, Walk, kind, struct_mem, variant_steps,
    variants, walk,
};
use crate::module::{HEAP, Sym, Val, mem};
use crate::rt::{DropCall, Helper};
use crate::unsupported;

/// Where a payload field's values come from: a TIR value, or locals.
#[derive(Clone)]
pub enum Src {
    Ref(u32),
    Locals(Vec<u32>),
}

/// Where [`Em::replace`] writes a value.
#[derive(Clone, Copy)]
pub(super) enum Dst<'a> {
    /// The Wasm locals that hold it.
    Locals(&'a [u32]),
    /// Top-level binding `h`'s Wasm globals from value `first` on, of the
    /// types `vals`: `(h, first, vals)`.
    Globals(hd_base::Hash128, usize, &'a [Val]),
    /// The leaves of the memory at the address in a local, some bytes
    /// before them: `(at, base, leaves)`.
    Mem(u32, u32, &'a [Leaf]),
}

/// Wasm global `k` of top-level binding `h`, of type `v`.
fn storage(h: hd_base::Hash128, k: usize, v: Val) -> crate::module::Global {
    crate::module::Global::Storage(h, u32::try_from(k).expect("k"), v)
}

impl Em<'_, '_, '_> {
    pub(super) fn kind(&self, t: Ty) -> Kind {
        kind(self.pool(), self.env(), t)
    }

    /// Whether `t` is a nominal newtype, which erases to its base.
    pub(super) fn newtype(&self, t: Ty) -> bool {
        matches!(self.pool().get(t), TyData::Adt { def, .. }
            if self.env().newtype_base(def).is_some())
    }

    /// An enum's (or an optional's) layout as values, whether or not it is
    /// boxed, and where each payload field lies in it; `None` for a type
    /// that is neither.
    pub(super) fn flat(&self, t: Ty) -> StageResult<Option<Rc<Flat>>> {
        if let Some(f) = self.flats.borrow().get(&t) {
            return Ok(Some(f.clone()));
        }
        let f = crate::layout::flat(self.pool(), self.env(), t)?;
        if let Some(f) = &f {
            self.flats.borrow_mut().insert(t, f.clone());
        }
        Ok(f)
    }

    /// [`Em::flat`] of a type its caller knows is an enum or an optional:
    /// a value a `SwitchTag`, a payload read, a variant's construction or a
    /// place's `as .v .f` step reaches, each of which the checker types
    /// only on an enum or an optional (each caller names its own).
    pub(super) fn enum_flat(&self, t: Ty) -> StageResult<Rc<Flat>> {
        // Every caller decides on, reads a payload of, builds or steps
        // into a variant of a value the checker typed as an enum or an
        // optional (`Tag::SwitchTag`, `Tag::Payload`, `Tag::NewVariant`,
        // `Proj::Variant`), or names an optional type itself.
        // ruled out: a type that is no enum, by the checker's typing above.
        Ok(self.flat(t)?.expect("an enum or an optional"))
    }

    /// Carries out the steps `steps` ([`Step`]): `leaf` pushes field
    /// `f`'s leaf `k`.
    pub(super) fn run_steps(
        &mut self,
        steps: &[Step],
        leaf: &mut dyn FnMut(&mut Self, usize, usize) -> StageResult<()>,
    ) -> StageResult<()> {
        for s in steps {
            match *s {
                Step::Leaf(f, k) => leaf(self, f, k)?,
                Step::Op(op) => self.f.op(op),
            }
        }
        Ok(())
    }

    /// The payload types of each variant of enum (or optional) `t`.
    pub(super) fn variant_tys(&self, t: Ty) -> StageResult<Rc<Vec<Vec<Ty>>>> {
        if let Some(v) = self.variant_tys.borrow().get(&t) {
            return Ok(v.clone());
        }
        let Some(vs) = variants(self.pool(), self.env(), t) else {
            return unsupported("a variant of a type that is not an enum");
        };
        let vs = Rc::new(vs);
        self.variant_tys.borrow_mut().insert(t, vs.clone());
        Ok(vs)
    }

    /// The counted references of a boxed enum's cell: its value's, as the
    /// enum lays it out unboxed.
    fn cell_walk(&self, t: Ty) -> Option<Walk> {
        self.input.runtime?;
        crate::layout::switch_walk(self.pool(), self.env(), t)
    }

    /// Pushes a zero of `v`.
    pub(super) fn zero(&mut self, v: Val) {
        self.f.op(match v {
            Val::I32 => I::I32Const(0),
            Val::I64 => I::I64Const(0),
            Val::F32 => I::F32Const(wasm_encoder::Ieee32::new(0)),
            Val::F64 => I::F64Const(wasm_encoder::Ieee64::new(0)),
        });
    }

    /// Pushes value `k` of a source.
    pub(super) fn src(&mut self, s: &Src, k: usize) -> StageResult<()> {
        match s {
            Src::Ref(r) => self.comp(*r, k),
            Src::Locals(ls) => {
                self.f.get(ls[k]);
                Ok(())
            }
        }
    }

    /// Pushes the value of leaf `l` at `address + base`.
    fn load(
        &mut self,
        address: &dyn Fn(&mut Self) -> StageResult<()>,
        base: u32,
        l: Leaf,
    ) -> StageResult<()> {
        address(self)?;
        self.f.op(l.load(base));
        Ok(())
    }

    /// Stores into leaf `l` at `address + base` the value that `value`
    /// pushes.
    pub(super) fn store_leaf(
        &mut self,
        address: &dyn Fn(&mut Self) -> StageResult<()>,
        base: u32,
        l: Leaf,
        value: &mut dyn FnMut(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        address(self)?;
        value(self)?;
        self.f.op(l.store(base));
        Ok(())
    }

    /// A new counted object whose payload is laid out as `m`, with the
    /// counted references `walk` (§23.4): a local holding its payload's
    /// address. Its descriptor names the release glue of its payload, and
    /// says whether it takes part in cycle collection (`cyclic`, from
    /// `crate::cycle`).
    pub(super) fn object(
        &mut self,
        m: &Mem,
        walk: Option<&Walk>,
        cyclic: bool,
        recursive: bool,
        target: Option<Ty>,
    ) -> u32 {
        let drops = target.map_or_else(Vec::new, |ty| self.drop_calls(ty));
        self.object_drops(m, walk, cyclic, recursive, target, drops)
    }

    pub(super) fn object_drops(
        &mut self,
        m: &Mem,
        walk: Option<&Walk>,
        cyclic: bool,
        recursive: bool,
        target: Option<Ty>,
        drops: Vec<DropCall>,
    ) -> u32 {
        let at = self.f.local(Val::I32);
        self.f.u32(m.size);
        let glue = walk
            .map(|w| {
                Sym::Helper(Helper::DropFields {
                    leaves: m.leaves.clone(),
                    walk: w.clone(),
                    drops: drops.clone(),
                })
            })
            .or_else(|| {
                (!drops.is_empty()).then(|| {
                    Sym::Helper(Helper::DropFields {
                        leaves: Vec::new(),
                        walk: Walk::Seq(Vec::new()),
                        drops,
                    })
                })
            });
        let weak_key = target.and_then(|ty| {
            let key = canon(
                self.pool(),
                &|d| self.env().path_hash(d),
                &mut CanonMemo::default(),
                ty,
            );
            (self.m.open_world || self.m.weak_types.contains(&key)).then_some(key)
        });
        if let Some(key) = weak_key {
            crate::rt::push_typed_desc_recursive(&mut self.f, glue, cyclic, recursive, key);
        } else {
            crate::rt::push_desc_recursive(&mut self.f, glue, cyclic, recursive);
        }
        if self.input.runtime.is_some_and(|r| r.checked.is_none())
            && m.size
                .checked_add(crate::module::HEADER)
                .is_some_and(|n| n <= 2048)
        {
            self.f.helper(Helper::NewFixed { size: m.size });
        } else {
            self.f.helper(Helper::New);
        }
        self.f.set(at);
        at
    }

    /// The `std.rt.Drop` calls in a concrete value's structural release,
    /// in pre-order: the value itself, then each inline field. A field that
    /// is its own counted data object runs its hook in that object's
    /// descriptor glue instead.
    pub(super) fn drop_calls(&self, t: Ty) -> Vec<DropCall> {
        self.drop_paths(t)
            .into_iter()
            .map(|(call, _)| call)
            .collect()
    }

    pub(super) fn drop_paths(&self, t: Ty) -> Vec<(DropCall, Vec<hd_tir::ir::Proj>)> {
        fn add(
            em: &Em<'_, '_, '_>,
            t: Ty,
            base: u32,
            root: bool,
            guards: &[(Leaf, Is)],
            path: &[hd_tir::ir::Proj],
            out: &mut Vec<(DropCall, Vec<hd_tir::ir::Proj>)>,
        ) {
            if (root || matches!(em.kind(t), Kind::Value))
                && let Some(&key) = em.input.collected.drop_glue.get(&t)
            {
                out.push((
                    DropCall {
                        offset: base,
                        target: key,
                        guards: guards.to_vec(),
                    },
                    path.to_vec(),
                ));
            }
            if let Ok(variants) = em.variant_tys(t)
                && let Ok(Some(flat)) = em.flat(t)
                && variants.len() == flat.positions.len()
            {
                for (variant, (fields, offsets)) in (0u32..).zip(variants.iter().zip(&flat.offsets))
                {
                    // The test that the discriminant says `variant`: none
                    // for the one variant, and no call for a variant no
                    // value takes.
                    let mut nested = guards.to_vec();
                    match flat.test(variant) {
                        Test::Const(true) => {}
                        Test::Const(false) | Test::Read(_, Is::Never) => continue,
                        Test::Read(d, is) => nested.push((
                            Leaf {
                                offset: base + d.leaf.offset,
                                ..d.leaf
                            },
                            is,
                        )),
                    }
                    for (index, (&field, &offset)) in fields.iter().zip(offsets).enumerate() {
                        if !matches!(em.kind(field), Kind::Value) {
                            continue;
                        }
                        let mut child = path.to_vec();
                        child.extend([
                            hd_tir::ir::Proj::Variant(variant),
                            hd_tir::ir::Proj::Field(u32::try_from(index).expect("field")),
                        ]);
                        add(em, field, base + offset, false, &nested, &child, out);
                    }
                }
                return;
            }
            let (fields, parts) = match em.kind(t) {
                Kind::Value => {
                    let stripped = crate::layout::strip(em.pool(), em.env(), t);
                    let fields = match em.pool().get(stripped) {
                        TyData::Tuple { .. } => crate::layout::tuple_members(em.pool(), stripped),
                        TyData::Adt { def, .. } if !em.env().is_enum(def) => em.struct_fields(t),
                        _ => return,
                    };
                    let Ok((_, parts)) = struct_mem(em.pool(), em.env(), t) else {
                        return;
                    };
                    (fields, parts)
                }
                _ => return,
            };
            for (index, (&field, part)) in fields.iter().zip(parts).enumerate() {
                if matches!(em.kind(field), Kind::Value) {
                    let mut child = path.to_vec();
                    child.push(hd_tir::ir::Proj::Field(
                        u32::try_from(index).expect("field"),
                    ));
                    add(em, field, base + part.offset, false, guards, &child, out);
                }
            }
        }

        let mut out = Vec::new();
        add(self, t, 0, true, &[], &[], &mut out);
        out
    }

    /// Moves an inline value's Wasm locals into a local-stack frame for
    /// its Drop hooks and the subsequent release of its fields.
    pub(super) fn drop_inline(&mut self, ls: &[u32], t: Ty) -> StageResult<bool> {
        self.drop_inline_flags(ls, t, &[])
    }

    pub(super) fn drop_inline_flags(
        &mut self,
        ls: &[u32],
        t: Ty,
        flags: &[Option<u32>],
    ) -> StageResult<bool> {
        if !matches!(self.kind(t), Kind::Value)
            || !crate::layout::holds_drop(self.pool(), self.env(), t)
        {
            return Ok(false);
        }
        let mut drops = self.drop_calls(t);
        if drops.is_empty() {
            return Ok(false);
        }
        let m = crate::layout::mem(self.pool(), self.env(), t)?;
        let mut storage = m.clone();
        let flag_base = m.size.next_multiple_of(4);
        if !flags.is_empty() {
            storage.size = (flag_base + u32::try_from(flags.len()).expect("flags") * 4)
                .next_multiple_of(m.align.max(4));
        }
        for (index, (call, flag)) in drops.iter_mut().zip(flags).enumerate() {
            if flag.is_some() {
                call.guards.push((
                    Leaf {
                        offset: flag_base + u32::try_from(index).expect("flag") * 4,
                        val: Val::I32,
                        bytes: 4,
                        signed: false,
                    },
                    Is::Eq(1),
                ));
            }
        }
        let walk = walk(self.pool(), self.env(), t).unwrap_or_else(|| Walk::Seq(Vec::new()));
        for &local in ls {
            self.f.get(local);
        }
        for (index, flag) in flags.iter().enumerate() {
            if let Some(flag) = flag {
                self.f.get(*flag);
                storage.leaves.push(Leaf {
                    offset: flag_base + u32::try_from(index).expect("flag") * 4,
                    val: Val::I32,
                    bytes: 4,
                    signed: false,
                });
            }
        }
        self.f.helper(Helper::DropValue {
            size: storage.size,
            leaves: storage.leaves,
            walk,
            drops,
        });
        Ok(true)
    }

    /// The leaves of a payload of `tys`, laid out as `parts`, that hold a
    /// collection a count copies ([`Em::copied`]).
    pub(super) fn part_colls(&self, tys: &[Ty], parts: &[Part]) -> Vec<u32> {
        let mut out = Vec::new();
        for (t, p) in tys.iter().zip(parts) {
            let first = u32::try_from(p.first).expect("leaves");
            out.extend(self.copied(*t).into_iter().map(|c| first + c));
        }
        out
    }

    /// One more count on each counted reference that a new object's
    /// construction copied into its payload: each collection, at the
    /// first `n` leaves of `skip`, the copy glue's result stored back. The
    /// other leaves of `skip` take no count (a move put them there).
    pub(super) fn retain_fields(
        &mut self,
        at: u32,
        m: &Mem,
        walk: Option<&Walk>,
        skip: &[u32],
        n: usize,
    ) {
        for &c in &skip[..n] {
            let l = m.leaves[c as usize];
            self.f.get(at);
            self.f.get(at);
            self.f.op(l.load(0));
            self.f.helper(Helper::CollCopy);
            self.f.op(I::I32Store(mem(HEAP, u64::from(l.offset), 2)));
        }
        if let Some(w) = walk.and_then(|w| w.without(skip)) {
            self.f.get(at);
            self.f.helper(Helper::RetainFields {
                leaves: m.leaves.clone(),
                walk: w,
            });
        }
    }

    /// `NewStruct`: a struct pushes its fields' values; a newtype is its
    /// one value.
    pub(super) fn new_data(&mut self, i: u32, bw: u32) -> StageResult<()> {
        let fields = self.rec(bw);
        let t = self.ty(i);
        if self.newtype(t) {
            // A newtype is constructed from its one base value.
            let r = fields[0];
            let moved = self.take(r);
            self.push(r)?;
            return self.store_as(i, moved);
        }
        // A struct literal names a struct, whose values are in locals.
        let tys = self.struct_fields(t);
        // Construction consumes its parts (§23.25).
        let srcs = self.count_refs(&fields, &tys)?;
        for (s, ft) in srcs.iter().zip(&tys) {
            for k in 0..self.vals(*ft)?.len() {
                self.src(s, k)?;
            }
        }
        // The parts' counts are the new value's.
        self.store_owned(i)
    }

    /// The parts `srcs`, of types `tys`, of a new value, each moved in or a
    /// copy with counts of its own ([`Em::retain_src`]).
    pub(super) fn count_parts(&mut self, srcs: &[Src], tys: &[Ty]) -> StageResult<Vec<Src>> {
        srcs.iter()
            .zip(tys)
            .map(|(s, t)| self.retain_src(s, *t))
            .collect()
    }

    /// The parts `rs`, of types `tys`, of a new value ([`Em::count_parts`]).
    fn count_refs(&mut self, rs: &[u32], tys: &[Ty]) -> StageResult<Vec<Src>> {
        let srcs: Vec<Src> = rs.iter().map(|r| Src::Ref(*r)).collect();
        self.count_parts(&srcs, tys)
    }

    /// Pushes every value of `s`.
    pub(super) fn push_src(&mut self, s: &Src) -> StageResult<()> {
        match s {
            Src::Ref(r) => self.push(*r),
            Src::Locals(ls) => {
                for l in ls {
                    self.f.get(*l);
                }
                Ok(())
            }
        }
    }

    /// Stores the values of `s` as the part `p` of the object at local `at`
    /// whose payload is laid out as `m`.
    pub(super) fn store_part(&mut self, at: u32, m: &Mem, p: Part, s: &Src) -> StageResult<()> {
        let leaves = &m.leaves[p.first..p.first + p.count];
        self.write_new(Dst::Mem(at, 0, leaves), s)
    }

    /// Replaces the `t` value at `d` with value `v`: a counted place takes
    /// a count of the new value ([`Em::consumed`]; a copy of each
    /// collection a place holds, `types.coll.value`) and releases the old
    /// one's before the write.
    pub(super) fn replace(&mut self, d: Dst, t: Ty, v: u32) -> StageResult<()> {
        if !self.needs_drop(t) {
            self.write_new(d, &Src::Ref(v))?;
            self.stored_drop();
            return Ok(());
        }
        let s = self.consumed(v)?;
        if self.releases_old() {
            let old = self.old_locals(d);
            self.load_old(d, &old);
            self.release_old_drop(&old, t)?;
        }
        self.write_new(d, &s)?;
        self.stored_drop();
        Ok(())
    }

    /// Replaces the `t` value at `d` with `s`, which holds its count
    /// already, as [`Em::replace`] does, but releases the old value, read
    /// into `old` ([`Em::old_locals`]), after the write.
    pub(super) fn replace_late(&mut self, d: Dst, t: Ty, s: &Src, old: &[u32]) -> StageResult<()> {
        let drops = self.needs_drop(t) && self.releases_old();
        if drops {
            self.load_old(d, old);
        }
        self.write_new(d, s)?;
        if drops {
            self.release_old_drop(old, t)?;
        }
        self.stored_drop();
        Ok(())
    }

    /// The locals the value at `d` is read into before a write: `d`'s own
    /// when it is locals.
    pub(super) fn old_locals(&mut self, d: Dst) -> Vec<u32> {
        match d {
            Dst::Locals(ls) => ls.to_vec(),
            Dst::Globals(_, _, vals) => vals.iter().map(|v| self.f.local(*v)).collect(),
            Dst::Mem(_, _, leaves) => leaves.iter().map(|l| self.f.local(l.val)).collect(),
        }
    }

    /// Reads the values at `d` into the locals `old`.
    fn load_old(&mut self, d: Dst, old: &[u32]) {
        for (k, o) in old.iter().enumerate() {
            match d {
                Dst::Locals(_) => return,
                Dst::Globals(h, first, vals) => self.f.global_get(storage(h, first + k, vals[k])),
                Dst::Mem(at, base, leaves) => {
                    self.f.get(at);
                    self.f.op(leaves[k].load(base));
                }
            }
            self.f.set(*o);
        }
    }

    /// Writes the values of `s` to `d`.
    pub(super) fn write_new(&mut self, d: Dst, s: &Src) -> StageResult<()> {
        match d {
            Dst::Locals(ls) => {
                self.push_src(s)?;
                for x in ls.iter().rev() {
                    self.f.set(*x);
                }
            }
            Dst::Globals(h, first, vals) => {
                self.push_src(s)?;
                for (k, v) in vals.iter().enumerate().rev() {
                    self.f.global_set(storage(h, first + k, *v));
                }
            }
            Dst::Mem(at, base, leaves) => {
                for (k, l) in leaves.iter().enumerate() {
                    self.f.get(at);
                    self.src(s, k)?;
                    self.f.op(l.store(base));
                }
            }
        }
        Ok(())
    }

    /// `CopyStruct`: a copy of `a` with the fields of the `(field, value)`
    /// pairs `bw` replaced.
    pub(super) fn copy_data(&mut self, i: u32, a: u32, bw: u32) -> StageResult<()> {
        let words = self.rec(bw);
        let repl: Vec<(u32, u32)> = words.chunks(2).map(|w| (w[0], w[1])).collect();
        let t = self.ty(i);
        let replaced = |f: usize| repl.iter().find(|(k, _)| *k as usize == f).map(|(_, v)| *v);
        if self.newtype(t) {
            match replaced(0) {
                Some(v) => self.push(v)?,
                None => self.push(a)?,
            }
            return self.store(i);
        }
        // A copy-update copies a struct, whose values are in locals.
        let (_, parts) = struct_mem(self.pool(), self.env(), t)?;
        let tys = self.struct_fields(t);
        let copy = self.operation_count(
            i,
            hd_tir::ir::Tag::Retain,
            hd_tir::ir::count::Part::Operand(0),
        );
        // The copy takes a count of each kept field, and each
        // replacement moves its count in or takes one (§23.25).
        let from = self.vals.get(&a).cloned();
        let mut srcs = Vec::new();
        for (f, (ft, p)) in tys.iter().zip(&parts).enumerate() {
            srcs.push(match (replaced(f), &from) {
                (Some(v), _) => Some(self.consumed(v)?),
                (None, Some(ls)) => {
                    let kept = &ls[p.first..p.first + p.count];
                    Some(Src::Locals(match copy {
                        Some(copy) => self.because(copy, |em| em.copy_locals(kept, *ft))?,
                        None => kept.to_vec(),
                    }))
                }
                (None, None) => None,
            });
        }
        for (f, p) in parts.iter().enumerate() {
            match &srcs[f] {
                Some(s) => {
                    for k in 0..p.count {
                        self.src(s, k)?;
                    }
                }
                None => {
                    for k in p.first..p.first + p.count {
                        self.comp(a, k)?;
                    }
                }
            }
        }
        self.store_owned(i)
    }

    /// A field read: field `idx` of `a`, picked from a struct's values.
    pub(super) fn field(&mut self, i: u32, a: u32, idx: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        if self.newtype(t) {
            self.push(a)?;
            return self.store(i);
        }
        // A field read of a value reads a struct's field; one through a
        // reference is a `DerefField` step.
        let (_, parts) = struct_mem(self.pool(), self.env(), t)?;
        let p = parts[idx as usize];
        for k in p.first..p.first + p.count {
            self.comp(a, k)?;
        }
        self.store(i)
    }

    /// The field types of struct type `t`, substituted.
    pub(super) fn struct_fields(&self, t: Ty) -> Vec<Ty> {
        let (pool, env) = (self.pool(), self.env());
        let TyData::Adt { def, args } = pool.get(crate::layout::strip(pool, env, t)) else {
            return Vec::new();
        };
        env.data_fields(def)
            .unwrap_or_default()
            .into_iter()
            .map(|f| hd_mono::subst(pool, env, def, args, f))
            .collect()
    }

    /// A tuple element read: element `idx`'s values.
    pub(super) fn tuple_get(&mut self, i: u32, a: u32, idx: u32) -> StageResult<()> {
        let (_, parts) = struct_mem(self.pool(), self.env(), self.ty_of(a))?;
        let p = parts[idx as usize];
        for k in p.first..p.first + p.count {
            self.comp(a, k)?;
        }
        self.store(i)
    }

    /// `a is b` (`expr.is.*`): two values refer to the same object.
    pub(super) fn is(&mut self, i: u32, a: u32, b: u32) -> StageResult<()> {
        // `ref.identity.same-place`: two references compare the places
        // they refer to, which are their addresses; two references to
        // trait values compare their data.
        let t = self.ty_of(a);
        if matches!(self.kind(t), Kind::Ref { .. }) {
            self.comp(a, 0)?;
            self.comp(b, 0)?;
            self.f.op(I::I32Eq);
            return self.store(i);
        }
        // A boxed enum, a list, a map and a trait value's
        // data are each one counted address first; a trait value that
        // boxed a value compares its boxes (`ref.identity.same-place`).
        let t = self.ty_of(a);
        if !matches!(self.kind(t), Kind::Boxed)
            && walk(self.pool(), self.env(), t).and_then(|w| w.single()) != Some(0)
        {
            return unsupported("identity of a value that is no object");
        }
        self.comp(a, 0)?;
        self.comp(b, 0)?;
        self.f.op(I::I32Eq);
        self.store(i)
    }

    /// Pushes the cell that says enum value `a`'s variant, a tag or a
    /// niche ([`Flat::cell`]): from its locals, from its cell when it is
    /// boxed, or as a borrow through a reference read it in place
    /// ([`Em::lazy_borrow`]). An enum of one variant has no cell; its
    /// tests need no read ([`Em::is_variant`]).
    pub(super) fn discr_cell(&mut self, a: u32, d: Discr) -> StageResult<()> {
        if let Some(&(_, word)) = self.lazy.get(&a) {
            self.f.get(word);
            return Ok(());
        }
        if matches!(self.kind(self.ty_of(a)), Kind::Boxed) {
            return self.load(&|em: &mut Self| em.comp(a, 0), 0, d.leaf);
        }
        self.comp(a, d.value as usize)
    }

    /// Pushes an `i32` that is nonzero exactly when enum value `a` is
    /// variant `v` (`read_discr` against `v`): a compare of its cell, or a
    /// constant for an enum of one variant.
    pub(super) fn is_variant(&mut self, a: u32, v: u32) -> StageResult<()> {
        // A `SwitchTag`'s scrutinee, an enum or an optional.
        let flat = self.enum_flat(self.ty_of(a))?;
        match flat.test(v) {
            Test::Const(b) => self.f.i32(i32::from(b)),
            Test::Read(d, is) => {
                self.discr_cell(a, d)?;
                for op in is.ops() {
                    self.f.op(op);
                }
            }
        }
        Ok(())
    }

    /// A payload read: field `field` of variant `variant` of enum value `a`.
    pub(super) fn payload(&mut self, i: u32, a: u32, variant: u32, field: u32) -> StageResult<()> {
        let t = self.ty_of(a);
        // A payload read's operand, an enum or an optional.
        let flat = self.enum_flat(t)?;
        // A checked payload step names a field of its variant.
        let (v, f) = (variant as usize, field as usize);
        if let Some(&(at, _)) = self.lazy.get(&a) {
            // A borrow through a reference reads the field in place.
            for l in &flat.leaves[v][f] {
                self.f.get(at);
                self.f.op(l.load(0));
            }
        } else if matches!(self.kind(t), Kind::Boxed) {
            // The cell holds the payload at its offset.
            for l in &flat.leaves[v][f] {
                self.load(&|em: &mut Self| em.comp(a, 0), 0, *l)?;
            }
        } else {
            for k in flat.selection(v, f) {
                self.comp(a, k)?;
            }
        }
        self.store(i)
    }

    /// [`Em::variant`] of copies of `fields`.
    pub(super) fn new_variant(&mut self, i: u32, variant: u32, fields: &[Src]) -> StageResult<()> {
        self.variant(i, variant, fields, false)
    }

    /// Variant `v` of `i`'s enum with the payload `fs`, moved in if `moved`:
    /// its cells ([`variant_steps`]), in a counted cell if boxed.
    pub(super) fn variant(&mut self, i: u32, v: u32, fs: &[Src], moved: bool) -> StageResult<()> {
        let t = self.ty(i);
        // A variant's construction, of an enum or an optional.
        let flat = self.enum_flat(t)?;
        let tys = self
            .variant_tys(t)
            .ok()
            .and_then(|vs| vs.get(v as usize).cloned())
            .unwrap_or_default();
        // Construction consumes its parts (§23.25).
        let fields = if moved || self.short_origins.contains(&i) {
            fs.to_vec()
        } else {
            self.count_parts(fs, &tys)?
        };
        // The checker builds a variant of its enum with each of its
        // fields, as the backend's own constructions do.
        let mut leaf = |em: &mut Self, f: usize, k: usize| em.src(&fields[f], k);
        let m = &flat.mem;
        if matches!(self.kind(t), Kind::Boxed) {
            // The cell is memory: the discriminant and the variant's leaves
            // at their offsets.
            let walk = self.cell_walk(t);
            let cyclic = self.cyclic_object(t);
            let recursive = self.recursive_object(t);
            let cell = self.object(m, walk.as_ref(), cyclic, recursive, Some(t));
            let at = |em: &mut Self| {
                em.f.get(cell);
                Ok(())
            };
            // `write_discr`: the tag, or a niche value, when `v` writes one.
            if let Some(d) = flat.cell()
                && let Some(raw) = d.enc.raw(v)
            {
                self.store_leaf(&at, 0, d.leaf, &mut |em: &mut Self| {
                    em.f.u32(raw);
                    Ok(())
                })?;
            }
            for (fi, ls) in flat.leaves[v as usize].iter().enumerate() {
                for (k, l) in ls.iter().enumerate() {
                    self.store_leaf(&at, 0, *l, &mut |em: &mut Self| leaf(em, fi, k))?;
                }
            }
            self.f.get(cell);
            return self.store_owned(i);
        }
        let values = variant_steps(&flat, v);
        for (c, steps) in values.iter().enumerate() {
            let steps = crate::layout::value_or_zero(&m.leaves, c, steps.as_ref());
            self.run_steps(&steps, &mut leaf)?;
        }
        self.store_owned(i)
    }
}
