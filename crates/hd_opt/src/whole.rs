//! Whole-program scans of every instance's TIR before emission, whose
//! results a release link's backend consumes: [`mutation_targets`], the
//! concrete types whose existing heap objects the program can write
//! (`crates/hd_wasm_linear/DESIGN.md` §23.28). The scan reads the
//! backend's layout through [`ObjectLayout`].

use hd_base::{DefId, HashSet, InstId};
use hd_mono::{InstanceTable, ProgramEnv, Sub, subst};
use hd_tir::ir::{Body, PlaceRoot, Tag, ref_word};
use hd_types::{InternPool, Ty, TyData, TyList};

/// What the scan reads of the backend's type layout.
pub trait ObjectLayout {
    /// `t` with newtypes erased and `mut` dropped: what its layout reads.
    fn strip(&self, t: Ty) -> Ty;
    /// The stripped target of a mutable counted reference type, or none.
    fn mutable_counted_ref_target(&self, t: Ty) -> Option<Ty>;
}

/// The concrete types whose existing heap objects the program can write.
/// `None` means an instance body was unavailable, so every reference edge
/// must remain cycle-capable.
pub fn mutation_targets(
    pool: &InternPool,
    env: &dyn ProgramEnv,
    layout: &dyn ObjectLayout,
    instances: &Instances<'_>,
) -> Option<HashSet<Ty>> {
    let table = instances.table;
    let mut targets = HashSet::default();
    let mut seen = HashSet::default();
    for id in instances.order {
        if matches!(table.sub[id.idx()], Sub::Adapter) {
            continue;
        }
        let b = (instances.body)(*id)?;
        let (item, args) = (table.item[id.idx()], table.args[id.idx()]);
        if !seen.insert((std::ptr::from_ref(b) as usize, item, args)) {
            continue;
        }
        let scan = Scan {
            pool,
            env,
            layout,
            item,
            args,
            b,
        };
        scan.mutation_targets(&mut targets);
    }
    Some(targets)
}

/// The program's instances as the backend emits them: collection's table,
/// the content order, and each instance's TIR body.
pub struct Instances<'a> {
    pub table: &'a InstanceTable,
    pub order: &'a [InstId],
    pub body: &'a dyn Fn(InstId) -> Option<&'a Body>,
}

struct Scan<'a> {
    pool: &'a InternPool,
    env: &'a dyn ProgramEnv,
    layout: &'a dyn ObjectLayout,
    item: DefId,
    args: TyList,
    b: &'a Body,
}

impl Scan<'_> {
    fn sub(&self, t: Ty) -> Ty {
        subst(self.pool, self.env, self.item, self.args, t)
    }

    /// Adds every type whose existing heap object this body can write: a
    /// mutable counted reference's target, and a reassignable local whose
    /// cell a counted reference can keep.
    fn mutation_targets(&self, out: &mut HashSet<Ty>) {
        let b = self.b;
        let mut writes = vec![0u32; b.local_ty.len()];
        for i in 0..b.len() {
            if b.tags[i] == Tag::Store
                && let Some((local, true)) = b.stored_local(i)
            {
                writes[local.idx()] += 1;
            }
        }
        let mut seen = HashSet::default();
        for &t in b.ty.iter().chain(&b.local_ty) {
            self.mutation_type(self.sub(t), &mut seen, out);
        }
        for (i, (&tag, &[place, word])) in b.tags.iter().zip(&b.data).enumerate() {
            if tag != Tag::Ref
                || word & (ref_word::LOCAL | ref_word::LENT) != 0
                || self
                    .layout
                    .mutable_counted_ref_target(self.sub(b.ty[i]))
                    .is_some()
            {
                continue;
            }
            let Some(PlaceRoot::Local(local)) = b.decode_place(place).map(|p| p.root) else {
                continue;
            };
            let flags = b.local_flags[local.idx()];
            let ty = self.sub(b.local_ty[local.idx()]);
            let parameter = flags & hd_tir::ir::local_flags::PARAM != 0;
            let reassignable = if parameter {
                matches!(self.pool.get(ty), TyData::Mut(_))
            } else {
                flags & hd_tir::ir::local_flags::MUTATED != 0 || writes[local.idx()] > 1
            };
            if reassignable {
                out.insert(self.layout.strip(ty));
            }
        }
    }

    /// Adds mutable counted references nested anywhere in concrete `t`.
    fn mutation_type(&self, t: Ty, seen: &mut HashSet<Ty>, out: &mut HashSet<Ty>) {
        if !seen.insert(t) {
            return;
        }
        if let Some(target) = self.layout.mutable_counted_ref_target(t) {
            out.insert(target);
        }
        let mut children = Vec::new();
        match self.pool.get(t) {
            TyData::Adt { def, args } => {
                children.extend(self.pool.list_items(args).iter().copied());
                if let Some(fields) = self.env.data_fields(def) {
                    children.extend(
                        fields
                            .into_iter()
                            .map(|field| subst(self.pool, self.env, def, args, field)),
                    );
                }
            }
            TyData::Tuple { elems, rest } => {
                children.extend(self.pool.list_items(elems).iter().copied());
                children.extend(rest);
            }
            TyData::Union(members) => {
                children.extend(self.pool.list_items(members).iter().copied());
            }
            TyData::Option(inner) | TyData::Mut(inner) | TyData::Ref { inner, .. } => {
                children.push(inner);
            }
            TyData::Fn { params, result, .. } => {
                children.extend(self.pool.list_items(params).iter().copied());
                children.push(result);
            }
            TyData::TraitValue { args, bindings, .. } => {
                children.extend(self.pool.list_items(args).iter().copied());
                children.extend(bindings.into_iter().map(|(_, ty)| ty));
            }
            TyData::Opaque { args, bound, .. } => {
                children.extend(self.pool.list_items(args).iter().copied());
                children.push(bound);
            }
            TyData::Assoc { self_ty, args, .. } => {
                children.push(self_ty);
                children.extend(self.pool.list_items(args).iter().copied());
            }
            TyData::Prim(_)
            | TyData::Never
            | TyData::Poison
            | TyData::Param(_)
            | TyData::Const(_)
            | TyData::Infer(_)
            | TyData::Canon(_)
            | TyData::Row(_)
            | TyData::Context(_) => {}
        }
        for child in children {
            self.mutation_type(child, seen, out);
        }
    }
}
