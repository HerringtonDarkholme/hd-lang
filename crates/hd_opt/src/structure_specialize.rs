//! Closed-instance specialization of compiler-generated `Structure.walk`
//! traversals. The generator owns the traversal order, so copying one at a
//! closed call site preserves its semantics while making the concrete walker
//! and target visible to the ordinary instance pipeline.

use hd_base::{DefId, Hash128, StableHasher};
use hd_tir::inline::Site;
use hd_tir::ir::{Body, BodyKind, Callee, Place, PlaceRoot, PrimOp, Proj, ReadMode, Ref, Tag};
use hd_types::{Ty, TyData, TyList};

use crate::inline::InlineEnv;

/// The generator facts needed by this pass, supplied by the driver so
/// `hd_opt` remains independent of `hd_structure`.
pub trait StructureEnv: InlineEnv {
    /// Whether `def` is the hidden `walk` body emitted by `hd_structure`.
    fn generated_structure_walk(&self, def: DefId) -> bool;

    /// The compiler-known `std.structure.Walker` trait.
    fn walker_trait(&self) -> DefId;

    /// The data-only `Walker` hooks whose defaults bridge to the ordinary
    /// protocol.
    fn walker_data_variant(&self) -> DefId;
    fn walker_data_member(&self) -> DefId;

    /// The data-only format boundaries exposed by a specialized serde walk.
    fn data_format_boundaries(&self) -> [DefId; 2];

    /// A generated immutable handle factory, if `def` is one.
    fn generated_structure_handle(&self, def: DefId) -> Option<GeneratedHandle>;

    /// The known `std.structure.FieldRecord.get` method.
    fn field_get(&self) -> DefId;

    /// The known `std.structure.VariantRecord.holds` method.
    fn variant_holds(&self) -> DefId;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedHandle {
    Variant { variant: u32 },
    Field { variant: u32, field: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructureAccessor {
    Get,
    Holds,
}

/// Copies every closed generated `Structure.walk` call into `body`, after
/// substituting its concrete instance and reselecting trait implementations.
/// Open targets and custom `Structure` implementations never satisfy the
/// generator predicate and remain calls.
pub fn specialize_calls(env: &dyn StructureEnv, body: &mut Body, hash: Hash128) -> Hash128 {
    if env.memo().is_none() {
        return hash;
    }
    let mut sites = Vec::new();
    let mut hashes = Vec::new();
    for call in (0..body.len()).rev() {
        let Some(Callee::Item { def, targs }) = hd_tir::inline::known_call(body, call) else {
            continue;
        };
        if !env.generated_structure_walk(def) || !closed(env, targs) {
            continue;
        }
        let Some((mut copy, copy_hash)) = instance(env, def, targs) else {
            continue;
        };
        // A data walk first reaches Walker's default `data_*` bridge. Inline
        // those exact hooks before selecting the concrete legacy hooks they
        // expose. Two bounded passes avoid turning recursive user methods
        // into an inlining fixpoint.
        let mut nested_hashes = inline_data_walker_methods(env, &mut copy);
        nested_hashes.extend(inline_walker_methods(env, &mut copy));
        nested_hashes.extend(inline_data_format_boundaries(env, &mut copy));
        fold_handle_accessors(env, &mut copy);
        // `DeriveInstance` is a function for ownership but intentionally not
        // a general inline candidate. This pass has already applied the
        // narrower generated-walk and closed-instance guards.
        copy.kind = BodyKind::Fn;
        if !fits_call(env, body, call, &mut copy) {
            continue;
        }
        sites.push(Site { call, copy });
        let mut site_key = StableHasher::new("structure-walk-site-v1");
        site_key.hash(copy_hash);
        for nested in nested_hashes {
            site_key.hash(nested);
        }
        hashes.push(site_key.finish());
    }
    let copied = hd_tir::inline::inline_calls(body, sites).unwrap_or_default();
    if !copied.contains(&true) {
        return hash;
    }
    let mut key = StableHasher::new("structure-walk-specialize-v1");
    key.hash(hash);
    for (_, copied_hash) in copied.iter().zip(hashes).filter(|(yes, _)| **yes) {
        key.hash(copied_hash);
    }
    key.finish()
}

/// Replaces a data type's selected field handle `hd_get` with a direct
/// projection. Its sole variant is known to hold, so its `hd_holds` becomes
/// true as well. Enum accessors keep their checks until this pass proves the
/// active switch arm; projecting an enum field without that proof would skip
/// `structure-variant-mismatch`.
fn fold_handle_accessors(env: &dyn StructureEnv, body: &mut Body) {
    let stores = stored_values(body);
    let field_get = env.field_get();
    let variant_holds = env.variant_holds();
    for call in 0..body.len() {
        if body.tags[call] != Tag::Call {
            continue;
        }
        let Some(Callee::Item { def, .. }) = Callee::from_words(body.record(body.data[call][0]))
        else {
            continue;
        };
        let accessor = if def == field_get {
            StructureAccessor::Get
        } else if def == variant_holds {
            StructureAccessor::Holds
        } else {
            continue;
        };
        let record = body.record(body.data[call][1]).to_vec();
        let args = &record[..record.len().saturating_sub(3)];
        let Some(&handle_arg) = args.first() else {
            continue;
        };
        let Some(handle) = source_handle(env, body, &stores, handle_arg, 0) else {
            continue;
        };
        match (accessor, handle, args) {
            (StructureAccessor::Get, GeneratedHandle::Field { field, .. }, [_, target]) => {
                let Some((def, targs)) = adt_of_ref(body, *target, env) else {
                    continue;
                };
                if env.enum_variants(def, targs).is_some() {
                    // The handle's variant is not a proof about `target`:
                    // user walkers may call `get` with another enum value.
                    continue;
                }
                let Some(words) = (Place {
                    root: PlaceRoot::Value(Ref(*target)),
                    proj: vec![Proj::Field(field)],
                })
                .words() else {
                    continue;
                };
                let place = push_record(body, &words);
                body.tags[call] = Tag::Read;
                body.data[call] = [place, ReadMode::Use as u32];
            }
            (StructureAccessor::Holds, GeneratedHandle::Variant { .. }, [_, target]) => {
                let Some((def, targs)) = adt_of_ref(body, *target, env) else {
                    continue;
                };
                if env.enum_variants(def, targs).is_some() {
                    continue;
                }
                let constant = u32::try_from(body.consts.len()).expect("constants");
                body.consts.push((Ty::BOOL, 1));
                let value = Ref::konst(constant).0;
                let values = push_record(body, &[value, value]);
                body.tags[call] = Tag::Prim;
                body.data[call] = [PrimOp::Eq as u32, values];
            }
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum StoredValue {
    #[default]
    Missing,
    Unique(u32),
    Ambiguous,
}

fn stored_values(body: &Body) -> Vec<StoredValue> {
    let mut values = vec![StoredValue::Missing; body.local_ty.len()];
    for instruction in 0..body.len() {
        let Some((local, whole)) = body.stored_local(instruction) else {
            continue;
        };
        if !whole {
            values[local.idx()] = StoredValue::Ambiguous;
            continue;
        }
        values[local.idx()] = match values[local.idx()] {
            StoredValue::Missing => StoredValue::Unique(body.data[instruction][1]),
            StoredValue::Unique(_) | StoredValue::Ambiguous => StoredValue::Ambiguous,
        };
    }
    values
}

fn source_handle(
    env: &dyn StructureEnv,
    body: &Body,
    stores: &[StoredValue],
    value: u32,
    depth: u8,
) -> Option<GeneratedHandle> {
    if depth >= 8 || value & Ref::CONST_BIT != 0 {
        return None;
    }
    let instruction = value as usize;
    if body.tags.get(instruction) == Some(&Tag::Read)
        && let Some(local) = body.local_read(instruction)
    {
        let StoredValue::Unique(value) = stores.get(local.idx())? else {
            return None;
        };
        return source_handle(env, body, stores, *value, depth + 1);
    }
    // The default data hook weakens its handle reference before forwarding
    // it to the ordinary member hook. This coercion preserves the value, so
    // it preserves the generated-handle provenance too.
    if body.same_value_coercion(instruction) {
        return source_handle(env, body, stores, body.data[instruction][0], depth + 1);
    }
    if body.tags.get(instruction) != Some(&Tag::Call) {
        return None;
    }
    let Callee::Item { def, .. } =
        Callee::from_words(body.record(*body.data.get(instruction)?.first()?))?
    else {
        return None;
    };
    env.generated_structure_handle(def)
}

fn adt_of_ref(body: &Body, value: u32, env: &dyn StructureEnv) -> Option<(DefId, TyList)> {
    let ty = if value & Ref::CONST_BIT != 0 {
        body.consts
            .get((value & !Ref::CONST_BIT) as usize)
            .map(|constant| constant.0)?
    } else {
        *body.ty.get(value as usize)?
    };
    match env.pool().get(ty) {
        TyData::Adt { def, args } => Some((def, args)),
        _ => None,
    }
}

fn push_record(body: &mut Body, words: &[u32]) -> u32 {
    let at = u32::try_from(body.extra.len()).expect("records");
    body.extra
        .push(u32::try_from(words.len()).expect("record length"));
    body.extra.extend_from_slice(words);
    at
}

/// Copies the selected concrete walker methods into one specialized walk.
/// These methods take `&mut self`, which deliberately keeps them out of the
/// general inliner; here the generated traversal and closed receiver prove
/// the narrower copy safe.
fn inline_walker_methods(env: &dyn StructureEnv, body: &mut Body) -> Vec<Hash128> {
    inline_selected_methods(
        env,
        body,
        &|callee| matches!(callee, Callee::TraitMethod { trait_, .. } if *trait_ == env.walker_trait()),
    )
}

fn inline_data_walker_methods(env: &dyn StructureEnv, body: &mut Body) -> Vec<Hash128> {
    let variant = env.walker_data_variant();
    let member = env.walker_data_member();
    inline_selected_methods(
        env,
        body,
        &|callee| matches!(callee, Callee::TraitMethod { method, .. } if *method == variant || *method == member),
    )
}

fn inline_data_format_boundaries(env: &dyn StructureEnv, body: &mut Body) -> Vec<Hash128> {
    let boundaries = env.data_format_boundaries();
    inline_selected_methods(
        env,
        body,
        &|callee| matches!(callee, Callee::TraitMethod { method, .. } if boundaries.contains(method)),
    )
}

fn inline_selected_methods(
    env: &dyn StructureEnv,
    body: &mut Body,
    selected: &dyn Fn(&Callee) -> bool,
) -> Vec<Hash128> {
    let mut sites = Vec::new();
    let mut hashes = Vec::new();
    for call in (0..body.len()).rev() {
        let Some(callee @ Callee::TraitMethod { .. }) = hd_tir::inline::known_call(body, call)
        else {
            continue;
        };
        if !selected(&callee) {
            continue;
        }
        let Some((def, args)) = env.impl_target(&callee) else {
            continue;
        };
        let Some((mut copy, hash)) = instance(env, def, args) else {
            continue;
        };
        copy.kind = BodyKind::Fn;
        if !fits_call(env, body, call, &mut copy) {
            continue;
        }
        sites.push(Site { call, copy });
        hashes.push(hash);
    }
    let copied = hd_tir::inline::inline_calls(body, sites).unwrap_or_default();
    copied
        .iter()
        .zip(hashes)
        .filter_map(|(yes, hash)| (*yes).then_some(hash))
        .collect()
}

/// The general inliner's call-shape check, repeated here because this pass
/// deliberately bypasses only its candidate policy, never its type safety.
fn fits_call(env: &dyn StructureEnv, caller: &Body, call: usize, copy: &mut Body) -> bool {
    let pool = env.pool();
    let open = |ty: Ty| hd_mono::reveal(pool, env, ty);
    let fits = |got: Option<Ty>, want: Ty| {
        got.is_some_and(|got| got == want || got == Ty::NEVER || want == Ty::NEVER)
    };
    let record = caller.record(caller.data[call][1]);
    let args = &record[..record.len().saturating_sub(3)];
    let params: Vec<usize> = (0..copy.local_ty.len())
        .filter(|&local| copy.local_flags[local] & hd_tir::ir::local_flags::PARAM != 0)
        .collect();
    if params.len() != args.len() {
        return false;
    }
    for (&local, &arg) in params.iter().zip(args) {
        if arg == hd_tir::ir::NONE {
            continue;
        }
        let arg_ty = if arg & hd_tir::ir::Ref::CONST_BIT != 0 {
            caller
                .consts
                .get((arg & !hd_tir::ir::Ref::CONST_BIT) as usize)
                .map(|constant| constant.0)
        } else {
            caller.ty.get(arg as usize).copied()
        };
        let Some(arg_ty) = arg_ty.and_then(open) else {
            return false;
        };
        let want = copy.local_ty[local];
        if matches!(pool.get(want), TyData::Mut(inner) if inner == arg_ty)
            || crate::inline::lends_by_kind(pool, env.ref_cells(), arg_ty, want)
        {
            copy.local_ty[local] = arg_ty;
        } else if !fits(Some(arg_ty), want) {
            return false;
        }
    }
    let Some(&root) = copy.sub_root.first() else {
        return false;
    };
    let Some(&root_ty) = copy.ty.get(root as usize) else {
        return false;
    };
    fits(open(caller.ty[call]), root_ty)
}

fn closed(env: &dyn StructureEnv, args: TyList) -> bool {
    env.pool().list_items(args).iter().all(|&ty| {
        !env.pool().has_param(ty) && !env.pool().has_assoc(ty) && !env.pool().has_opaque(ty)
    })
}

fn instance(env: &dyn StructureEnv, def: DefId, args: TyList) -> Option<(Body, Hash128)> {
    let hash = env.checked_body(def)?.1;
    let mut copy = env.fresh_body(def)?;
    let pool = env.pool();
    let ty = |t: Ty| hd_mono::subst(pool, env, def, args, t);
    let list = |l| {
        pool.list(
            &pool
                .list_items(l)
                .iter()
                .map(|&t| ty(t))
                .collect::<Vec<_>>(),
        )
    };
    hd_tir::ir::map_body_types(&mut copy, &ty, &list);
    if copy
        .ty
        .iter()
        .chain(&copy.local_ty)
        .any(|&t| pool.has_param(t) || pool.has_assoc(t) || pool.has_opaque(t) || t == Ty::POISON)
    {
        return None;
    }
    hd_tir::inline::rechoose(&mut copy, &|trait_, self_ty, targs| {
        env.select_impl(trait_, self_ty, targs)
    });
    Some((copy, hash))
}
