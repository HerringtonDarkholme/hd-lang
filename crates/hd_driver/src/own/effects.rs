//! Conditional generated-handle effects over the ordinary semantic call graph.
//! No body copy, ownership layout, or separate context engine is needed.

use super::{
    Body, CallMemory, Callee, ChoiceKind, DefId, Env, HashMap, InlineEnv, Node, OwnCx, Pass,
    ProgramEnv, RefCell, StructureEnv, Tri, Ty, TyData, TyList, lock,
};
use hd_tir::ir::Ref;
use hd_tir::own::EffectIndex;
use hd_tir::own::memory_effects::{BodyEffects, HandleOrigin, Transfer, ValueSource, Writes};
use std::sync::Arc;

struct View<'a, 'e> {
    env: &'a Env<'e>,
    instance: Option<Node>,
    types: RefCell<HashMap<Ty, Ty>>,
    callees: RefCell<HashMap<Callee, Callee>>,
}

impl<'a, 'e> View<'a, 'e> {
    fn new(env: &'a Env<'e>, instance: Option<Node>) -> Self {
        Self {
            env,
            instance,
            types: RefCell::default(),
            callees: RefCell::default(),
        }
    }
    fn ty(&self, ty: Ty) -> Ty {
        let Some((def, args)) = self.instance else {
            return ty;
        };
        if !self.env.is_open(ty) {
            return ty;
        }
        if let Some(&value) = self.types.borrow().get(&ty) {
            return value;
        }
        let value = hd_mono::subst(&self.env.run.pool, self.env, def, args, ty);
        self.types.borrow_mut().insert(ty, value);
        value
    }
    fn list(&self, args: TyList) -> TyList {
        let pool = &self.env.run.pool;
        pool.list(
            &pool
                .list_items(args)
                .iter()
                .map(|&ty| self.ty(ty))
                .collect::<Vec<_>>(),
        )
    }
    fn callee(&self, callee: &Callee) -> Callee {
        if self.instance.is_none() {
            return callee.clone();
        }
        if let Some(value) = self.callees.borrow().get(callee) {
            return value.clone();
        }
        let value = match *callee {
            Callee::Item { def, targs } => Callee::Item {
                def,
                targs: self.list(targs),
            },
            Callee::TraitMethod {
                trait_,
                method,
                self_ty,
                targs,
                choice,
            } => {
                let self_ty = self.ty(self_ty);
                let targs = self.list(targs);
                let choice = if choice.0 == ChoiceKind::Bound {
                    InlineEnv::select_impl(self.env, trait_, self_ty, targs)
                        .map_or(choice, |def| (ChoiceKind::Impl, def.raw()))
                } else {
                    choice
                };
                Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice,
                }
            }
        };
        self.callees
            .borrow_mut()
            .insert(callee.clone(), value.clone());
        value
    }
    fn word_ty(&self, body: &Body, word: u32) -> Ty {
        let ty = Ref(word).as_inst().map_or_else(
            || {
                body.consts
                    .get((word & !Ref::CONST_BIT) as usize)
                    .map_or(Ty::POISON, |v| v.0)
            },
            |v| body.ty[v.0 as usize],
        );
        self.ty(ty)
    }
}

// Only the structural effect index reads this view. The ownership passes
// continue to use Env and their existing physical-layout admission.
impl OwnCx for View<'_, '_> {
    fn temporary_owner_name(&self) -> hd_base::Symbol {
        OwnCx::temporary_owner_name(self.env)
    }
    fn counted(&self, ty: Ty) -> bool {
        let ty = self.ty(ty);
        self.env.is_open(ty) || OwnCx::counted(self.env, ty)
    }
    fn holds_drop(&self, ty: Ty) -> Tri {
        let ty = self.ty(ty);
        if self.env.is_open(ty) {
            Tri::Maybe
        } else if hd_wasm::holds_drop_type(&self.env.run.pool, self.env, ty) {
            Tri::Yes
        } else {
            Tri::No
        }
    }
    fn holds_ref(&self, ty: Ty) -> bool {
        OwnCx::holds_ref(self.env, self.ty(ty))
    }
    fn counted_refs_only(&self, ty: Ty) -> bool {
        OwnCx::counted_refs_only(self.env, self.ty(ty))
    }
    fn pass(&self, callee: &Callee, k: usize, n: usize) -> Pass {
        // Raw allocation freshness reads the intrinsic's existing ownership
        // contract, without consulting or recording an ownership summary.
        let callee = self.callee(callee);
        let def = match callee {
            Callee::Item { def, .. } => def,
            Callee::TraitMethod { method, .. } => method,
        };
        if self.env.intrinsic(def).is_some() {
            self.env.declared_pass(def, k, n)
        } else {
            Pass::Owned
        }
    }
    fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
        Pass::Owned
    }
    fn received(&self, _: DefId, _: usize, _: usize) -> Pass {
        Pass::Owned
    }
    fn may_write(&self, _: &Callee) -> bool {
        true
    }
    fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
        false
    }
    fn holds_coll(&self, _: Ty) -> bool {
        false
    }
    fn call_memory(&self, callee: &Callee) -> CallMemory {
        OwnCx::call_memory(self.env, &self.callee(callee))
    }
}

fn requirement(origin: HandleOrigin) -> Writes {
    match origin {
        HandleOrigin::Trusted => Writes::default(),
        HandleOrigin::Parameter(index) => Writes::handle(index),
        HandleOrigin::Unknown => Writes::EXTERNAL,
    }
}

fn value(facts: &[HandleOrigin], word: u32) -> HandleOrigin {
    Ref(word)
        .as_inst()
        .and_then(|v| facts.get(v.0 as usize))
        .copied()
        .unwrap_or_default()
}

fn equation(
    env: &Env<'_>,
    body: &Body,
    index: &EffectIndex,
    instance: Option<Node>,
) -> BodyEffects {
    let view = View::new(env, instance);
    let mut handles = vec![HandleOrigin::Unknown; body.len()];
    for (i, source) in index.value_sources(body).iter().enumerate() {
        handles[i] = match *source {
            ValueSource::Parameter(parameter) => HandleOrigin::Parameter(parameter),
            ValueSource::Value(word) => value(&handles, word),
            ValueSource::Producer => hd_tir::inline::known_call(body, i)
                .map(|callee| view.callee(&callee))
                .and_then(|callee| env.callee_node(&callee))
                .and_then(|node| StructureEnv::generated_structure_handle(env, node.0))
                .map_or(HandleOrigin::Unknown, |_| HandleOrigin::Trusted),
        };
    }
    let mut quiet = vec![false; body.len()];
    let mut known = vec![None; body.len()];
    let mut required = Writes::default();
    for i in 0..body.len() {
        let Some(callee) = hd_tir::inline::known_call(body, i) else {
            continue;
        };
        let Some(node) = env.callee_node(&view.callee(&callee)) else {
            continue;
        };
        let args = hd_tir::inline::arguments(body, i);
        let [handle, target, ..] = args else { continue };
        let origin = value(&handles, *handle);
        if origin == HandleOrigin::Unknown {
            continue;
        }
        let TyData::Ref { inner, .. } = env.run.pool.get(view.word_ty(body, *target)) else {
            continue;
        };
        let Some(proof) =
            hd_opt::structure_specialize::generated_accessor_proof(env, node.0, inner)
        else {
            continue;
        };
        quiet[i] = true;
        required.join(requirement(origin));
        if proof == hd_opt::structure_specialize::AccessorProof::True {
            known[i] = Some(hd_mono::defaults::Value::Scalar(1));
        }
    }
    let active = if known.iter().any(Option::is_some) {
        hd_mono::defaults::reachable_given(body, &|i| known[i])
    } else {
        Vec::new()
    };
    let mut out =
        hd_tir::own::WriteEffects::indexed(body, index).equation_in(&view, &active, &quiet);
    out.direct.join(required);
    for call in &mut out.calls {
        call.callee = view.callee(&call.callee);
        if let Transfer::Arguments(references) = &mut call.transfer {
            let args = hd_tir::inline::arguments(body, call.instruction as usize);
            let origins = args
                .iter()
                .take(64)
                .map(|&word| value(&handles, word))
                .collect();
            call.transfer = Transfer::WithHandles {
                references: std::mem::take(references),
                handles: origins,
            };
        }
    }
    out
}

pub(super) fn writes_after_ownership(env: &Env<'_>, body: &Body, node: Node) -> bool {
    let index = EffectIndex::new(body);
    equation(env, body, &index, Some(node))
        .evaluate(|callee| env.call_writes(callee))
        .any()
}

impl Env<'_> {
    pub(super) fn borrowed_effect<T>(
        &self,
        node: Node,
        f: impl FnOnce(&Body, &EffectIndex) -> T,
    ) -> Option<T> {
        let all = self.run.summaries.as_ref()?;
        {
            let mut depths = lock(&all.effect_depths);
            if self.run.pool.list_items(node.1).iter().any(|&ty| {
                hd_mono::type_argument_depth(&self.run.pool, &mut depths, ty)
                    > u32::from(hd_mono::layout::MAX_DEPTH)
            }) {
                // Emission rejects these same type arguments before requesting
                // a body. This does not exclude an accepted wide instance.
                return None;
            }
        }
        let supplied = self.supplied_walk_family(node);
        let (body, key) = if let Some(family) = &supplied {
            (
                &family.bodies[0],
                (node.0, *self.run.pool.list_items(node.1).first()?),
            )
        } else {
            let (body, _) = self
                .run
                .lazy_body(self.p, node.0)
                .and_then(|b| self.run.decode(b))?;
            (body, (node.0, Ty::POISON))
        };
        let index = lock(&all.effect_indices)
            .entry(key)
            .or_insert_with(|| Arc::new(EffectIndex::new(body)))
            .clone();
        Some(f(body, &index))
    }

    pub(super) fn borrowed_effect_callees(&self, node: Node) -> Vec<Node> {
        self.borrowed_effect(node, |body, _| {
            let view = View::new(self, Some(node));
            let mut seen = hd_base::HashSet::default();
            (0..body.len())
                .filter_map(|i| hd_tir::inline::known_call(body, i))
                .filter_map(|callee| self.callee_node(&view.callee(&callee)))
                .filter(|node| {
                    self.intrinsic(node.0).is_none()
                        && (self.run.lazy_body(self.p, node.0).is_some()
                            || self.supplied_walk_family(*node).is_some())
                        && !self
                            .run
                            .pool
                            .list_items(node.1)
                            .iter()
                            .any(|&ty| self.is_open(ty))
                        && seen.insert(*node)
                })
                .collect()
        })
        .unwrap_or_default()
    }

    pub(super) fn borrowed_equation(&self, node: Node) -> Option<BodyEffects> {
        self.borrowed_effect(node, |body, index| {
            hd_wasm::with_layout_cache(|| equation(self, body, index, Some(node)))
        })
    }

    pub(super) fn conditional_equation(&self, body: &Body) -> BodyEffects {
        hd_wasm::with_layout_cache(|| equation(self, body, &EffectIndex::new(body), None))
    }
}
