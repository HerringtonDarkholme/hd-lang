//! Instance/layout facts for the link-time TIR pointer-loan analysis.
//! This adapter never reads the emitter's loan-answer tables.

use hd_base::{DefId, HashMap, InstId};
use hd_mono::{Sub, Target, TargetKind};
use hd_tir::ir::{Body, Proj, Tag};
use hd_tir::own::pointer::{self, Call, Context, Conversion, Reference};
use hd_types::{Ty, TyList};
use std::sync::Arc;

use super::{borrow, excl};
use crate::Input;
use crate::layout::{self, Kind};
use crate::module::Module;

struct Facts<'a, 'b> {
    input: &'a Input<'b>,
    module: &'a Module,
    /// Each instance's body, by instance number, which every fact reads.
    bodies: Vec<Option<&'a Body>>,
    rows: Vec<Arc<[usize]>>,
}

pub(super) fn decide(input: &Input<'_>, module: &mut Module) -> pointer::Decisions {
    let mut bodies = vec![None; input.collected.table.item.len()];
    // The retained body references keep these identities live throughout
    // preparation; no pointer key survives in Module. A generic body's
    // instruction groups are built once, shared by all its instances.
    let mut grouped: HashMap<*const Body, Vec<Arc<[usize]>>> = HashMap::default();
    let mut rows: Vec<Arc<[usize]>> = vec![Arc::from([]); input.collected.table.item.len()];
    for &id in input.order {
        let Some(b) = (input.body)(id) else {
            continue;
        };
        bodies[id.idx()] = Some(b);
        let groups = grouped.entry(std::ptr::from_ref(b)).or_insert_with(|| {
            let mut rows = vec![Vec::new(); b.sub_root.len()];
            for (i, sub) in b.inst_subs().into_iter().enumerate() {
                rows[sub as usize].push(i);
            }
            crate::work::use_facts(b.len());
            rows.into_iter().map(Arc::from).collect()
        });
        let Sub::Body(scope) = input.collected.table.sub[id.idx()] else {
            continue;
        };
        rows[id.idx()] = Arc::clone(&groups[usize::from(scope)]);
    }
    let facts = Facts {
        input,
        module,
        bodies,
        rows,
    };
    pointer::decide(&facts)
}

impl Facts<'_, '_> {
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
            Kind::Value => None,
        }
    }

    fn strip(&self, ty: Ty) -> Ty {
        layout::strip_newtype(self.input.pool, self.input.env, ty)
    }

    fn shared_census(&self) {
        crate::work::shared_census();
    }

    fn object(&self, def: DefId) -> bool {
        self.module.objects.contains(&self.input.env.path_hash(def))
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
            Target::Call(c)
            | Target::CrossingNumber { call: c, .. }
            | Target::Default { call: c, .. }
            | Target::MapInline(c) => Some(direct(c)),
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
                        self.module.inst(k).filter(|id| {
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
