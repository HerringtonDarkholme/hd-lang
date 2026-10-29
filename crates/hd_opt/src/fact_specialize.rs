//! Closed declaration-fact lookups. A generated `Structure.facts` body is
//! fixed for one monomorphized declaration, even when the ordinary std
//! helper that asks it is deliberately cold.

use hd_base::{DefId, Hash128, LocalId, StableHasher};
use hd_tir::ir::{Body, Callee, IntrinsicOp, PlaceRoot, Proj, Ref, Tag};
use hd_types::{Ty, TyList};

use crate::structure_specialize::StructureEnv;

/// Facts about one declaration-fact query which the driver can answer from
/// generator metadata and checked bodies.
pub trait FactEnv: StructureEnv {
    /// The queried fact type when `def[args]` is either `Facts.find` itself
    /// or a pure one-argument wrapper around it.
    fn fact_lookup_key(&self, def: DefId, args: TyList) -> Option<FactLookup>;

    /// Whether `def` is the ordinary `List.is_empty` helper.
    fn list_is_empty(&self, def: DefId) -> bool;

    /// Whether `ty` is the nominal `Facts` value. A literal list alone is not
    /// declaration-fact provenance, even when its elements look like facts.
    fn is_facts(&self, ty: Ty) -> bool;

    /// The concrete types in a fixed, literal, no-spread fact list and the
    /// checked bodies that prove them. `def` is either a generated
    /// `Structure.facts` factory or a fact body's `DefaultCall` target.
    /// `projected` is the path from that value to the queried `Facts`.
    fn known_facts(&self, def: DefId, args: TyList, projected: &[Proj]) -> Option<KnownFacts>;

    /// A concrete generated member or variant handle body. Its root block
    /// is entered explicitly: the caller's handle call remains in the suite,
    /// so inspecting this compiler-owned body cannot discard caller effects.
    fn fact_handle_body(&self, def: DefId, args: TyList) -> Option<(Body, Hash128)>;

    /// A stable structural digest of one closed concrete fact type.
    fn fact_type_hash(&self, ty: Ty) -> Hash128;

    /// The target of either a checker reference or its lowered `Ref[T]`.
    fn fact_ref_inner(&self, ty: Ty) -> Option<Ty>;
}

/// One declaration's fixed fact types. The values still evaluate normally;
/// this is only enough to prove that a different type is absent.
#[derive(Clone, Debug)]
pub struct KnownFacts {
    pub types: Vec<Ty>,
    pub digest: Hash128,
}

/// The requested type and the path from a recognized wrapper's first
/// parameter to the `Facts` receiver it passes to `Facts.find`.
#[derive(Clone, Debug)]
pub struct FactLookup {
    pub key: Ty,
    pub projection: Vec<Proj>,
}

#[derive(Clone, Copy, Debug, Default)]
enum StoredValue {
    #[default]
    Missing,
    Unique(u32),
    Ambiguous,
}

/// Replaces queries proven absent from fixed, literal facts by `None`. The
/// returned hash names both the transform and every checked body it relied on.
pub fn specialize_calls(env: &dyn FactEnv, body: &mut Body, hash: Hash128) -> Hash128 {
    if env.memo().is_none() {
        return hash;
    }
    let stores = stored_values(body);
    let mut dependencies = Vec::new();
    for call in (0..body.len()).rev() {
        let Some(Callee::Item { def, targs }) = hd_tir::inline::plain_call(body, call) else {
            continue;
        };
        if env.list_is_empty(def) {
            let record = body.record(body.data[call][1]);
            let args = &record[..record.len().saturating_sub(3)];
            if let Some(&items) = args.first()
                && let Some(facts) = source_facts(env, body, &stores, items, &[], 0)
            {
                let input = u32::try_from(body.consts.len()).expect("constants");
                body.consts
                    .push((Ty::BOOL, u64::from(!facts.types.is_empty())));
                let not_input = push_record(body, &[Ref::konst(input).0]);
                body.tags[call] = Tag::Prim;
                body.data[call] = [hd_tir::ir::PrimOp::Not as u32, not_input];
                dependencies.push(facts.digest);
                continue;
            }
        }
        let Some(lookup) = env.fact_lookup_key(def, targs) else {
            continue;
        };
        let record = body.record(body.data[call][1]);
        let args = &record[..record.len().saturating_sub(3)];
        let Some(&facts) = args.first() else {
            continue;
        };
        let Some(facts) = source_facts(env, body, &stores, facts, &lookup.projection, 0) else {
            continue;
        };
        if facts.types.contains(&lookup.key) {
            continue;
        }
        let empty = push_record(body, &[]);
        body.tags[call] = Tag::NewVariant;
        body.data[call] = [0, empty];
        dependencies.push(facts.digest);
    }
    if dependencies.is_empty() {
        return hash;
    }
    let mut key = StableHasher::new("declaration-fact-specialize-v1");
    key.hash(hash);
    for dependency in dependencies {
        key.hash(dependency);
    }
    key.finish()
}

fn stored_values(body: &Body) -> Vec<StoredValue> {
    let mut values = vec![StoredValue::Missing; body.local_ty.len()];
    for instruction in 0..body.len() {
        if body.tags[instruction] == Tag::Intrinsic
            && matches!(
                IntrinsicOp::from_u32(body.data[instruction][0]),
                Some(
                    IntrinsicOp::ListPush
                        | IntrinsicOp::ListReserve
                        | IntrinsicOp::MapSet
                        | IntrinsicOp::MapRemove
                )
            )
            && let Some(&receiver) = body.record(body.data[instruction][1]).first()
            && let Some(local) = changed_local(body, receiver, 0)
        {
            // Collection intrinsics mutate through an ordinary value read,
            // not a `Ref` to the local. The initializer is stale afterwards.
            values[local.idx()] = StoredValue::Ambiguous;
            continue;
        }
        if body.tags[instruction] == Tag::Ref
            && body.data[instruction][1] & hd_tir::ir::ref_word::MUT != 0
            && let Some(local) = body.local_in_place(body.data[instruction][0])
        {
            values[local.idx()] = StoredValue::Ambiguous;
            continue;
        }
        let Some((local, whole)) = body.stored_local(instruction) else {
            continue;
        };
        values[local.idx()] = if whole {
            match values[local.idx()] {
                StoredValue::Missing => StoredValue::Unique(body.data[instruction][1]),
                StoredValue::Unique(_) | StoredValue::Ambiguous => StoredValue::Ambiguous,
            }
        } else {
            StoredValue::Ambiguous
        };
    }
    values
}

fn changed_local(body: &Body, value: u32, depth: u8) -> Option<LocalId> {
    if depth >= 16 || value & Ref::CONST_BIT != 0 {
        return None;
    }
    let instruction = value as usize;
    match body.tags.get(instruction)? {
        Tag::Read => match body.decode_place(body.data[instruction][0])?.root {
            PlaceRoot::Local(local) => Some(local),
            PlaceRoot::Value(value) => changed_local(body, value.0, depth + 1),
            PlaceRoot::Global(..) | PlaceRoot::Frame(_) | PlaceRoot::Ptr(_) => None,
        },
        Tag::Block => changed_local(body, body.data[instruction][1], depth + 1),
        Tag::Coerce if body.same_value_coercion(instruction) => {
            changed_local(body, body.data[instruction][0], depth + 1)
        }
        _ => None,
    }
}

fn source_facts(
    env: &dyn FactEnv,
    body: &Body,
    stores: &[StoredValue],
    value: u32,
    projected: &[Proj],
    depth: u8,
) -> Option<KnownFacts> {
    if depth >= 16 || value & Ref::CONST_BIT != 0 {
        return None;
    }
    let instruction = value as usize;
    match body.tags.get(instruction)? {
        Tag::Read => {
            let place = body.decode_place(body.data[instruction][0])?;
            let mut projection = place.proj;
            projection.extend_from_slice(projected);
            source_place(env, body, stores, place.root, &projection, depth + 1)
        }
        Tag::Ref => {
            let [Proj::Deref, projected @ ..] = projected else {
                return None;
            };
            let place = body.decode_place(body.data[instruction][0])?;
            let mut projection = place.proj;
            projection.extend_from_slice(projected);
            source_place(env, body, stores, place.root, &projection, depth + 1)
        }
        // Replacing the lookup can make a receiver block unreachable. Its
        // suite may have effects, so ordinary caller provenance never looks
        // through a block. Generated bodies are inspected separately by the
        // environment and keep their call instruction in the caller suite.
        Tag::Block => None,
        Tag::Coerce if body.same_value_coercion(instruction) => source_facts(
            env,
            body,
            stores,
            body.data[instruction][0],
            projected,
            depth + 1,
        ),
        Tag::NewStruct => {
            if (projected.is_empty() || matches!(projected, [Proj::Field(0)]))
                && env.is_facts(*body.ty.get(instruction)?)
            {
                return literal_facts(env, body, stores, instruction, depth + 1);
            }
            let [Proj::Field(field), rest @ ..] = projected else {
                return None;
            };
            let value = *body
                .record(body.data[instruction][1])
                .get(*field as usize)?;
            source_facts(env, body, stores, value, rest, depth + 1)
        }
        Tag::Call => {
            let Callee::Item { def, targs } = hd_tir::inline::plain_call(body, instruction)? else {
                return None;
            };
            if let Some(facts) = env.known_facts(def, targs, projected) {
                return Some(facts);
            }
            let (handle, handle_hash) = env.fact_handle_body(def, targs)?;
            let root = *handle.sub_root.first()? as usize;
            if handle.tags.get(root) != Some(&Tag::Block) {
                return None;
            }
            let handle_stores = stored_values(&handle);
            let mut facts = source_facts(
                env,
                &handle,
                &handle_stores,
                handle.data[root][1],
                projected,
                depth + 1,
            )?;
            let mut digest = StableHasher::new("generated-handle-facts-v1");
            digest.hash(handle_hash);
            digest.hash(facts.digest);
            facts.digest = digest.finish();
            Some(facts)
        }
        Tag::DefaultCall => {
            let record = body.record(body.data[instruction][0]);
            let (&def, &targs) = (record.first()?, record.get(1)?);
            env.known_facts(DefId::from_raw(def), TyList(targs), projected)
        }
        _ => None,
    }
}

fn source_place(
    env: &dyn FactEnv,
    body: &Body,
    stores: &[StoredValue],
    root: PlaceRoot,
    projected: &[Proj],
    depth: u8,
) -> Option<KnownFacts> {
    match root {
        PlaceRoot::Local(local) => {
            let StoredValue::Unique(value) = stores.get(local.idx())? else {
                return None;
            };
            source_facts(env, body, stores, *value, projected, depth)
        }
        PlaceRoot::Value(value) => source_facts(env, body, stores, value.0, projected, depth),
        _ => None,
    }
}

fn literal_facts(
    env: &dyn FactEnv,
    body: &Body,
    stores: &[StoredValue],
    value: usize,
    depth: u8,
) -> Option<KnownFacts> {
    let items = *body.record(body.data[value][1]).first()?;
    let items = literal_list(body, stores, items, depth)?;
    literal_fact_list(env, body, items)
}

fn literal_fact_list(env: &dyn FactEnv, body: &Body, items: usize) -> Option<KnownFacts> {
    if body.tags.get(items) != Some(&Tag::NewList) {
        return None;
    }
    if body.data[items][0] != hd_tir::ir::NONE {
        return None;
    }
    let pool = env.pool();
    let mut types = Vec::new();
    for &item in body.record(body.data[items][1]) {
        let item = item as usize;
        if body.tags.get(item) != Some(&Tag::RefDyn) {
            return None;
        }
        let address = body.data[item][0] as usize;
        if body.tags.get(address) != Some(&Tag::Block) {
            return None;
        }
        let coercion = body.data[address][1] as usize;
        if body.tags.get(coercion) != Some(&Tag::Coerce)
            || body.record(body.data[coercion][1]).first()
                != Some(&(hd_tir::ir::Coercion::RefToTraitValue as u32))
        {
            return None;
        }
        let source = body.data[coercion][0] as usize;
        let inner = env.fact_ref_inner(*body.ty.get(source)?)?;
        if inner == Ty::POISON
            || pool.has_param(inner)
            || pool.has_assoc(inner)
            || pool.has_opaque(inner)
        {
            return None;
        }
        types.push(inner);
    }
    let mut digest = StableHasher::new("literal-instance-facts-v1");
    for &ty in &types {
        digest.hash(env.fact_type_hash(ty));
    }
    Some(KnownFacts {
        types,
        digest: digest.finish(),
    })
}

fn literal_list(body: &Body, stores: &[StoredValue], value: u32, depth: u8) -> Option<usize> {
    if depth >= 16 || value & Ref::CONST_BIT != 0 {
        return None;
    }
    let instruction = value as usize;
    match body.tags.get(instruction)? {
        Tag::NewList => Some(instruction),
        Tag::Block => literal_list(body, stores, body.data[instruction][1], depth + 1),
        Tag::Coerce if body.same_value_coercion(instruction) => {
            literal_list(body, stores, body.data[instruction][0], depth + 1)
        }
        Tag::Read => {
            let place = body.decode_place(body.data[instruction][0])?;
            if !place.proj.is_empty() {
                return None;
            }
            let PlaceRoot::Local(local) = place.root else {
                return None;
            };
            let StoredValue::Unique(value) = stores.get(local.idx())? else {
                return None;
            };
            literal_list(body, stores, *value, depth + 1)
        }
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
