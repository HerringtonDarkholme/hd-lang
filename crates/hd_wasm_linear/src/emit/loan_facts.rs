//! Instance/layout facts for the link-time TIR pointer-loan analysis.
//! This adapter never reads the emitter's loan-answer tables.

use hd_base::{DefId, HashMap, HashSet, InstId};
use hd_mono::{Sub, Target, TargetKind, subst};
use hd_tir::ir::{Body, Place, Proj, Tag};
use hd_tir::own::pointer::{self, Binding, Call, Context, Conversion, Holder, Reference};
use hd_types::{Ty, TyList};

use super::borrow::{self, Roots};
use super::excl::{self, Bind, Held};
use crate::Input;
use crate::layout::{self, Kind};
use crate::module::Module;

struct LocalRoots {
    shared: HashSet<usize>,
    pointers: HashSet<usize>,
}

struct Facts<'a, 'b> {
    input: &'a Input<'b>,
    module: &'a Module,
    roots: HashMap<InstId, LocalRoots>,
    /// Each instance's body, by instance number, which every fact reads.
    bodies: Vec<Option<&'a Body>>,
}

pub(super) fn decide(input: &Input<'_>, module: &Module) -> pointer::Decisions {
    let mut roots = HashMap::default();
    let mut bodies = vec![None; input.collected.table.item.len()];
    for &id in input.order {
        let Some(b) = (input.body)(id) else {
            continue;
        };
        bodies[id.idx()] = Some(b);
        if !matches!(input.collected.table.sub[id.idx()], Sub::Body(_)) {
            continue;
        }
        let t = &input.collected.table;
        let sub = |ty| {
            subst(
                input.pool,
                input.env,
                t.item[id.idx()],
                t.args[id.idx()],
                ty,
            )
        };
        roots.insert(
            id,
            LocalRoots {
                shared: super::closure::shared_locals(b),
                pointers: borrow::pointer_locals(input.pool, input.env, b, &sub),
            },
        );
    }
    pointer::decide(&Facts {
        input,
        module,
        roots,
        bodies,
    })
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

    fn source(&self, id: InstId) -> Option<(DefId, TyList, u16)> {
        let t = &self.input.collected.table;
        match t.sub[id.idx()] {
            Sub::Body(k) => Some((t.item[id.idx()], t.args[id.idx()], k)),
            Sub::Adapter => None,
        }
    }

    fn substitute(&self, id: InstId, ty: Ty) -> Ty {
        let t = &self.input.collected.table;
        subst(
            self.input.pool,
            self.input.env,
            t.item[id.idx()],
            t.args[id.idx()],
            ty,
        )
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
        let sub = |ty| self.substitute(id, ty);
        let root = borrow::root_ty(b, &sub, p.root)?;
        let locals = &self.roots[&id];
        let roots = Roots {
            shared: &locals.shared,
            pointers: &locals.pointers,
            objects: &self.module.objects,
            env: self.input.env,
        };
        Some(borrow::place_crossings(
            self.input.pool,
            (b, &sub),
            &roots,
            root,
            p,
        ))
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
        borrow::dyn_receiver(
            self.input.pool,
            self.input.env,
            (self.body(id)?, &|ty| self.substitute(id, ty)),
            value,
        )
    }

    fn bindings(&self, id: InstId) -> Vec<Holder> {
        let Some(b) = self.body(id) else {
            return Vec::new();
        };
        excl::binds(
            self.input.pool,
            self.input.env,
            b,
            &|ty| self.substitute(id, ty),
            true,
        )
        .into_iter()
        .map(|(i, held, bind, _)| {
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
            excl::held_mutable(
                self.input.pool,
                self.input.env,
                b,
                &|ty| self.substitute(id, ty),
                Held { local, slot },
            )
        })
    }

    fn carried_refs(&self, id: InstId, value: usize) -> Vec<u32> {
        self.body(id).map_or_else(Vec::new, |b| {
            excl::carried_refs(
                self.input.pool,
                self.input.env,
                b,
                &|ty| self.substitute(id, ty),
                value,
            )
        })
    }

    fn param_locals(&self, id: InstId) -> Vec<usize> {
        self.body(id).map_or_else(Vec::new, super::param_locals)
    }

    fn conversions(&self) -> Vec<Conversion> {
        fn traits(vt: &hd_mono::VTable, out: &mut Vec<DefId>) {
            out.push(vt.trait_);
            for parent in &vt.parents {
                traits(parent, out);
            }
        }
        let mut out = Vec::new();
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
                traits(vt, &mut parents);
                out.push(Conversion {
                    from: self.substitute(id, from),
                    traits: parents,
                });
            }
        }
        out
    }
}
