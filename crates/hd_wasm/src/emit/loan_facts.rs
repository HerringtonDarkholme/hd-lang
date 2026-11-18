//! Instance/layout facts for the link-time TIR pointer-loan analysis.
//! This adapter never reads the emitter's loan-answer tables.

use hd_base::{DefId, HashMap, HashSet, InstId};
use hd_mono::{Sub, Target, TargetKind};
use hd_tir::ir::{Body, Place, Proj, Tag};
use hd_tir::own::pointer::{self, Binding, Call, Context, Conversion, Holder, Reference};
use hd_types::{Ty, TyList};
use std::cell::RefCell;
use std::sync::Arc;

use super::borrow::{self, Roots};
use super::excl::{self, Bind, Held};
use crate::Input;
use crate::layout::{self, Kind};
use crate::module::Module;

/// Final holder facts retained from link analysis for emission.
#[derive(Default)]
pub(crate) struct HolderFacts {
    pub(super) shared: Arc<HashSet<usize>>,
    pub(super) pointers: Arc<HashSet<usize>>,
    pub(super) storage: Arc<borrow::StorageRoots>,
    pub(super) bindings: Vec<(usize, Held, Bind)>,
    pub(super) carried: Arc<HashMap<usize, Vec<u32>>>,
}

struct PreparedBody {
    rows: Vec<Arc<[usize]>>,
    shared: Arc<HashSet<usize>>,
    origins: Arc<excl::OriginIndex>,
    stores: Vec<Arc<[usize]>>,
    storage: Arc<borrow::StorageRoots>,
}

#[derive(Default)]
struct Carried {
    graph: excl::OriginGraph,
    answers: HashMap<usize, Vec<u32>>,
}

struct Facts<'a, 'b> {
    input: &'a Input<'b>,
    module: &'a Module,
    roots: HashMap<InstId, HolderFacts>,
    /// Each instance's body, by instance number, which every fact reads.
    bodies: Vec<Option<&'a Body>>,
    rows: Vec<Arc<[usize]>>,
    carried: RefCell<HashMap<InstId, Carried>>,
}

pub(super) fn decide(input: &Input<'_>, module: &mut Module) -> pointer::Decisions {
    let empty_origins = Arc::new(excl::OriginIndex::default());
    let mut origins = vec![empty_origins; input.collected.table.item.len()];
    let mut roots = HashMap::default();
    let empty_carried = Arc::new(HashMap::default());
    let mut bodies = vec![None; input.collected.table.item.len()];
    // The retained body references keep these identities live throughout
    // preparation; no pointer key survives in Module. A generic body's
    // instruction groups are built once, shared by all its instances.
    let mut grouped: HashMap<*const Body, PreparedBody> = HashMap::default();
    let mut rows: Vec<Arc<[usize]>> = vec![Arc::from([]); input.collected.table.item.len()];
    for &id in input.order {
        let Some(b) = (input.body)(id) else {
            continue;
        };
        bodies[id.idx()] = Some(b);
        let groups = grouped.entry(std::ptr::from_ref(b)).or_insert_with(|| {
            let origins = Arc::new(excl::OriginIndex::new(b));
            PreparedBody {
                rows: {
                    let mut rows = vec![Vec::new(); b.sub_root.len()];
                    for (i, sub) in b.inst_subs().into_iter().enumerate() {
                        rows[sub as usize].push(i);
                    }
                    crate::work::use_facts(b.len());
                    rows.into_iter().map(Arc::from).collect()
                },
                shared: Arc::new(super::closure::shared_locals(b)),
                origins,
                stores: excl::binding_stores(b),
                storage: Arc::new(borrow::StorageRoots::new(b)),
            }
        });
        origins[id.idx()] = Arc::clone(&groups.origins);
        let Sub::Body(scope) = input.collected.table.sub[id.idx()] else {
            continue;
        };
        rows[id.idx()] = Arc::clone(&groups.rows[usize::from(scope)]);
        let sites = Arc::clone(&groups.stores[usize::from(scope)]);
        roots.insert(
            id,
            HolderFacts {
                storage: Arc::clone(&groups.storage),
                shared: Arc::clone(&groups.shared),
                pointers: Arc::new(borrow::pointer_locals(input.pool, input.env, b, &sites)),
                bindings: excl::binds(input.pool, input.env, b, &groups.origins, &sites)
                    .into_iter()
                    .map(|(i, held, bind, _)| (i, held, bind))
                    .collect(),
                carried: Arc::clone(&empty_carried),
            },
        );
    }
    module.loan_origins = origins;
    let facts = Facts {
        input,
        module,
        roots,
        bodies,
        rows,
        carried: RefCell::default(),
    };
    let decisions = pointer::decide(&facts);
    let mut carried = facts.carried.into_inner();
    let empty = Arc::new(HolderFacts::default());
    let mut retained = vec![empty; input.collected.table.item.len()];
    for (id, mut holder) in facts.roots {
        if let Some(cache) = carried.remove(&id) {
            holder.carried = Arc::new(cache.answers);
        }
        retained[id.idx()] = Arc::new(holder);
    }
    module.loan_holders = retained;
    decisions
}

impl Facts<'_, '_> {
    fn origin_index(&self, id: InstId) -> &excl::OriginIndex {
        &self.module.loan_origins[id.idx()]
    }

    fn target(&self, id: InstId, instruction: u32) -> Option<&Target> {
        self.input.collected.calls[id.idx()].get(&instruction)
    }
}

impl Context for Facts<'_, '_> {
    fn order(&self) -> &[InstId] {
        self.input.order
    }

    fn body(&self, id: InstId) -> Option<&Body> {
        self.bodies[id.idx()].or_else(|| (self.input.body)(id))
    }

    fn instructions(&self, id: InstId) -> &[usize] {
        crate::work::use_facts(self.rows[id.idx()].len());
        &self.rows[id.idx()]
    }

    fn source(&self, id: InstId) -> Option<(DefId, TyList, u16)> {
        let t = &self.input.collected.table;
        match t.sub[id.idx()] {
            Sub::Body(k) => Some((t.item[id.idx()], t.args[id.idx()], k)),
            Sub::Adapter => None,
        }
    }

    fn reference(&self, ty: Ty) -> Option<Reference> {
        match layout::kind(self.input.pool, self.input.env, ty) {
            Kind::Ref { target, whole, fat } => Some(Reference {
                target,
                whole,
                fat,
                mutable: layout::ref_mutable(self.input.pool, self.input.env, ty),
            }),
            _ => None,
        }
    }

    fn strip(&self, ty: Ty) -> Ty {
        layout::strip(self.input.pool, self.input.env, ty)
    }

    fn place_crossings(&self, id: InstId, p: &Place) -> Option<Vec<(usize, Ty)>> {
        let b = self.body(id)?;
        let root = borrow::root_ty(b, p.root)?;
        let locals = &self.roots[&id];
        let roots = Roots {
            storage: &locals.storage,
            shared: &locals.shared,
            pointers: &locals.pointers,
            objects: &self.module.objects,
            env: self.input.env,
        };
        Some(borrow::place_crossings(self.input.pool, b, &roots, root, p))
    }

    fn type_crossings(&self, root: Ty, steps: &[Proj]) -> Vec<(usize, Ty)> {
        borrow::crossings(self.input.pool, self.input.env, root, steps)
    }

    fn call(&self, id: InstId, instruction: u32) -> Option<Call> {
        let target = self.target(id, instruction)?;
        let direct = |c: &hd_mono::CallTarget| match c.kind {
            TargetKind::Instance => vec![c.key],
            TargetKind::Intrinsic(_) | TargetKind::Builtin { .. } => Vec::new(),
        };
        let keys = match target {
            Target::Call(c) | Target::Default { call: c, .. } | Target::MapInline(c) => {
                Some(direct(c))
            }
            Target::MapKey { hash, eq } => {
                let mut keys = direct(hash);
                keys.extend(direct(eq));
                Some(keys)
            }
            Target::KnownClosure(k) => Some(vec![*k]),
            Target::Dyn => None,
            _ => Some(Vec::new()),
        };
        Some(Call {
            default: matches!(target, Target::Default { .. }),
            callees: keys.map(|keys| {
                keys.into_iter()
                    .map(|k| {
                        self.module.inst_of.get(&k).copied().filter(|id| {
                            matches!(self.input.collected.table.sub[id.idx()], Sub::Body(_))
                        })
                    })
                    .collect()
            }),
        })
    }

    fn receiver(&self, id: InstId, instruction: usize) -> Option<Option<bool>> {
        excl::call_info(
            self.input,
            self.body(id)?,
            instruction,
            self.target(id, u32::try_from(instruction).expect("insts")),
        )
    }

    fn params(&self, id: InstId, instruction: u32) -> Vec<Ty> {
        borrow::call_params(self.input, self.module, self.target(id, instruction))
    }

    fn dyn_receiver(&self, id: InstId, value: usize) -> Option<DefId> {
        borrow::dyn_receiver(self.input.pool, self.input.env, self.body(id)?, value)
    }

    fn bindings(&self, id: InstId) -> Vec<Holder> {
        let Some(holder) = self.roots.get(&id) else {
            return Vec::new();
        };
        holder
            .bindings
            .iter()
            .copied()
            .map(|(i, held, bind)| {
                (
                    i,
                    (held.local, held.slot),
                    match bind {
                        Bind::Begin(a) => Binding::Reference(a),
                        Bind::Call(a) => Binding::CallArgument(a),
                        Bind::CallParent(a) => Binding::CallParent(a),
                        Bind::Share(_) | Bind::Move(_) => Binding::Other,
                    },
                )
            })
            .collect()
    }

    fn held_mutable(&self, id: InstId, (local, slot): (usize, usize)) -> bool {
        self.body(id).is_some_and(|b| {
            excl::held_mutable(self.input.pool, self.input.env, b, Held { local, slot })
        })
    }

    fn carried_refs(&self, id: InstId, value: usize) -> Vec<u32> {
        self.body(id).map_or_else(Vec::new, |b| {
            let mut cache = self.carried.borrow_mut();
            let Carried { graph, answers } = cache.entry(id).or_default();
            answers
                .entry(value)
                .or_insert_with(|| {
                    excl::carried_refs(
                        self.input.pool,
                        self.input.env,
                        b,
                        self.origin_index(id),
                        graph,
                        value,
                    )
                })
                .clone()
        })
    }

    fn param_locals(&self, id: InstId) -> Vec<usize> {
        self.body(id).map_or_else(Vec::new, super::param_locals)
    }

    fn conversions(&self) -> Vec<Conversion> {
        let mut out = Vec::new();
        // A node's identity includes the concrete payload. Revisiting it at
        // another coercion cannot add a trait/object pair to dyn_types. Keep
        // first encounter order, but traverse each node/edge only once here.
        let mut seen = hd_base::HashSet::default();
        for &id in self.input.order {
            let Some(b) = self.body(id) else {
                continue;
            };
            for (&i, target) in &self.input.collected.calls[id.idx()] {
                let (Target::VTable(vt), Some(&Tag::Coerce)) = (target, b.tags.get(i as usize))
                else {
                    continue;
                };
                let a = b.data[i as usize][0];
                let Some(&from) = b.ty.get(a as usize) else {
                    continue;
                };
                let mut parents = Vec::new();
                let mut todo = vec![*vt];
                while let Some(id) = todo.pop() {
                    hd_mono::work::dyn_conversion(1);
                    if !seen.insert(id) {
                        continue;
                    }
                    let node = &self.input.collected.vtables[id.0];
                    parents.push(node.trait_);
                    todo.extend(node.parents.iter().rev().copied());
                }
                out.push(Conversion {
                    from,
                    traits: parents,
                });
            }
        }
        out
    }
}
