//! Generic requirement key collisions (spec/lang/
//! 11-requirements-and-suspension.md, "Generic Key Collisions"): two
//! keys that can become identical under some substitution of their type
//! parameters may not be visible together.

use hd_base::{DefId, HashMap};
use hd_diag::Code;
use hd_syntax::NodeRef;
use hd_types::{ParamRef, Ty, TyData, Types};

use crate::body::{Ck, RowFrame};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Head {
    #[default]
    Parameter,
    Adt(DefId),
    Trait(DefId),
    Tuple,
    Union,
    Rigid(Ty),
}

#[derive(Default)]
struct Description {
    head: Head,
    args: Vec<usize>,
    bindings: HashMap<DefId, usize>,
}

struct Classes {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl Classes {
    fn root(&mut self, mut id: usize) -> usize {
        let first = id;
        while self.parent[id] != id {
            hd_types::work::type_node();
            id = self.parent[id];
        }
        let mut at = first;
        while self.parent[at] != id {
            let next = self.parent[at];
            self.parent[at] = id;
            at = next;
        }
        id
    }
    fn join(&mut self, mut a: usize, mut b: usize) -> usize {
        if self.rank[a] < self.rank[b] {
            std::mem::swap(&mut a, &mut b);
        }
        self.parent[b] = a;
        if self.rank[a] == self.rank[b] {
            self.rank[a] += 1;
        }
        a
    }
}

/// Some finite parameter substitution makes these keys identical.
/// Compound classes share structural constraints; associated bindings are
/// open records whose shared fields agree. Check cycles once after solving,
/// rather than rewalking an expanding substitution at every variable bind.
fn can_coincide(pool: Types<'_>, a: Ty, b: Ty) -> bool {
    if a == b {
        return true;
    }
    let mut ids = HashMap::default();
    let mut types = Vec::new();
    let mut links = Vec::new();
    let mut children = Vec::new();
    hd_base::stack::all(None, |ty, stack| {
        let Some(ty) = ty else {
            stack.extend([Some(b), Some(a)]);
            return true;
        };
        let hd_base::hash::hash_map::Entry::Vacant(slot) = ids.entry(ty) else {
            return true;
        };
        hd_types::work::type_node();
        slot.insert(types.len());
        types.push(ty);
        children.clear();
        match pool.get(ty) {
            TyData::Fn {
                params,
                result,
                row,
                ..
            } => {
                children.extend_from_slice(pool.list_items(params));
                children.push(result);
                children.push(pool.intern_ty(&TyData::Row(row)));
            }
            TyData::Row(row) | TyData::Context(row) => {
                let row = pool.row_data(row);
                children.extend(row.keys);
                children.extend(row.params.into_iter().map(|p| {
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner: p.owner,
                        index: p.index,
                    }))
                }));
            }
            _ => pool.children(ty, &mut children),
        }
        links.push(children.clone());
        stack.extend(children.iter().rev().copied().map(Some));
        true
    });
    let mut descriptions = Vec::with_capacity(types.len());
    let mut initial = Vec::new();
    for &ty in &types {
        let (head, args, bindings) = match pool.get(ty) {
            TyData::Param(_) => (Head::Parameter, hd_types::TyList::EMPTY, Vec::new()),
            TyData::Adt { def, args } => (Head::Adt(def), args, Vec::new()),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => (Head::Trait(def), args, bindings),
            TyData::Tuple { elems, rest: None } => (Head::Tuple, elems, Vec::new()),
            TyData::Union(members) => (Head::Union, members, Vec::new()),
            _ => (Head::Rigid(ty), hd_types::TyList::EMPTY, Vec::new()),
        };
        let mut fields = HashMap::default();
        for (name, value) in bindings {
            hd_types::work::type_node();
            if let Some(old) = fields.insert(name, ids[&value]) {
                initial.push((old, ids[&value]));
            }
        }
        descriptions.push(Description {
            head,
            args: pool.list_items(args).iter().map(|t| ids[t]).collect(),
            bindings: fields,
        });
    }
    let mut classes = Classes {
        parent: (0..types.len()).collect(),
        rank: vec![0; types.len()],
    };
    let solved = hd_base::stack::all(None, |pair, stack| {
        let Some((a, b)) = pair else {
            stack.extend(initial.iter().copied().map(Some));
            stack.push(Some((ids[&a], ids[&b])));
            return true;
        };
        let (a, b) = (classes.root(a), classes.root(b));
        if a == b {
            return true;
        }
        hd_types::work::type_node();
        let mut x = std::mem::take(&mut descriptions[a]);
        let mut y = std::mem::take(&mut descriptions[b]);
        let done = if x.head == Head::Parameter {
            y
        } else if y.head == Head::Parameter {
            x
        } else {
            if x.head != y.head || x.args.len() != y.args.len() {
                return false;
            }
            let mut parts: Vec<_> = x.args.iter().zip(&y.args).map(|(&x, &y)| (x, y)).collect();
            if x.bindings.len() < y.bindings.len() {
                std::mem::swap(&mut x.bindings, &mut y.bindings);
            }
            for (name, value) in y.bindings {
                hd_types::work::type_node();
                match x.bindings.entry(name) {
                    hd_base::hash::hash_map::Entry::Occupied(entry) => {
                        parts.push((*entry.get(), value));
                    }
                    hd_base::hash::hash_map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            }
            stack.extend(parts.into_iter().rev().map(Some));
            x
        };
        let root = classes.join(a, b);
        descriptions[root] = done;
        true
    });
    if !solved {
        return false;
    }
    drop(descriptions);
    // A class graph cycle is exactly an infinite substitution. This also
    // checks parameters beneath rigid holders, not just matching arguments.
    let roots: Vec<_> = (0..types.len()).map(|i| classes.root(i)).collect();
    let mut edges = vec![Vec::new(); types.len()];
    let mut incoming = vec![0usize; types.len()];
    for (i, children) in links.into_iter().enumerate() {
        for child in children {
            hd_types::work::type_node();
            let child = roots[ids[&child]];
            edges[roots[i]].push(child);
            incoming[child] += 1;
        }
    }
    let mut visited = 0;
    hd_base::stack::all(None, |node, stack| {
        let Some(node) = node else {
            stack.extend(
                roots
                    .iter()
                    .enumerate()
                    .filter(|&(i, &r)| i == r && incoming[i] == 0)
                    .map(|(i, _)| Some(i)),
            );
            return true;
        };
        hd_types::work::type_node();
        visited += 1;
        for &child in &edges[node] {
            incoming[child] -= 1;
            if incoming[child] == 0 {
                stack.push(Some(child));
            }
        }
        true
    });
    visited == roots.iter().enumerate().filter(|&(i, &r)| i == r).count()
}

impl Ck<'_, '_> {
    /// The keys a lookup at this point can select from the enclosing
    /// scopes: the `$.with` blocks up to and including the enclosing
    /// function's or closure's declared row (`req.with.collision.compared`,
    /// `req.with.collision.closure`).
    pub(crate) fn visible_keys(&self) -> Vec<Ty> {
        let pool = self.pool();
        let mut keys = Vec::new();
        for frame in self.rows.iter().rev() {
            match frame {
                RowFrame::With(r) => keys.extend(pool.row_data(*r).keys),
                RowFrame::Declared(r)
                | RowFrame::Closure {
                    written: Some(r), ..
                } => {
                    keys.extend(pool.row_data(*r).keys);
                    break;
                }
                RowFrame::Any
                | RowFrame::Closure { written: None, .. }
                | RowFrame::Inferred { .. }
                | RowFrame::Profile { .. } => break,
            }
        }
        keys
    }

    /// One `$.with` or `$.context` entry binds the keys `new`: none may be
    /// able to become the same key as one of the `earlier` keys of the
    /// same expression or of the `visible` keys of the enclosing scopes
    /// (`req.with.collision`). Reported once per entry, on the entry.
    pub(crate) fn check_key_collision(
        &mut self,
        entry: NodeRef<'_>,
        new: &[Ty],
        earlier: &[Ty],
        visible: &[Ty],
    ) {
        let pool = self.pool();
        for &n in new {
            for &other in earlier.iter().chain(visible) {
                if n != other && can_coincide(pool, n, other) {
                    let msg = format!(
                        "the keys `{}` and `{}` can become the same key for some type arguments",
                        hd_resolve::show_ty_in(&self.cx.names, pool, n),
                        hd_resolve::show_ty_in(&self.cx.names, pool, other)
                    );
                    self.err(Code::GenericRequirementKeyCollision, entry, &msg);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_types::{InferTable, InternPool, LocalPool, TyList, VarKind};

    #[test]
    fn key_constraints_and_occurs_at_depth_1000_fit_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for generated constraint graphs.
            .spawn(|| {
                let pool = InternPool::new();
                let p = pool.types();
                let param = |owner, index| {
                    p.intern_ty(&TyData::Param(ParamRef {
                        owner: DefId::from_raw(owner),
                        index,
                    }))
                };
                let tuple = |xs: &[Ty]| {
                    p.intern_ty(&TyData::Tuple {
                        elems: p.list(xs),
                        rest: None,
                    })
                };
                for depth in [100, 1000] {
                    let x = param(1, 0);
                    let (mut a, mut b, mut bad) = (x, Ty::I32, Ty::BOOL);
                    for _ in 0..depth {
                        a = tuple(&[a, a]);
                        b = tuple(&[b, b]);
                        bad = tuple(&[bad, bad]);
                    }
                    let (answer, work) = hd_types::work::count_type_nodes(|| can_coincide(p, a, b));
                    assert!(answer);
                    assert!(
                        work < 20 * depth + 50,
                        "shared success stays linear: {work}"
                    );
                    let (answer, work) =
                        hd_types::work::count_type_nodes(|| can_coincide(p, b, bad));
                    assert!(!answer);
                    assert!(
                        work < 8 * depth + 50,
                        "shared mismatch stays linear: {work}"
                    );
                    let (answer, work) = hd_types::work::count_type_nodes(|| can_coincide(p, x, a));
                    assert!(!answer, "substitution must be finite");
                    assert!(work < 10 * depth + 50, "one final cycle walk: {work}");
                }
                let wrap = |t, i| {
                    p.intern_ty(&match i % 4 {
                        0 => TyData::Adt {
                            def: DefId::from_raw(9),
                            args: p.list(&[t, t]),
                        },
                        1 => TyData::TraitValue {
                            def: DefId::from_raw(9),
                            args: p.list(&[t, t]),
                            bindings: vec![(DefId::from_raw(10), t)],
                        },
                        2 => TyData::Union(p.list(&[t, t])),
                        _ => TyData::Tuple {
                            elems: p.list(&[t, t]),
                            rest: None,
                        },
                    })
                };
                let (mut a, mut b) = (param(1, 0), Ty::I32);
                for i in 0..1000 {
                    a = wrap(a, i);
                    b = wrap(b, i);
                }
                let (answer, work) = hd_types::work::count_type_nodes(|| can_coincide(p, a, b));
                assert!(answer);
                assert!(
                    work < 25_000,
                    "mixed compound classes do not unfold their shared paths: {work}"
                );
                let mut costs = Vec::new();
                for width in [100, 1000] {
                    let (mut a, mut b) = (param(1, 0), Ty::I32);
                    for _ in 0..20 {
                        a = tuple(&[a, a]);
                        b = tuple(&[b, b]);
                    }
                    let (mut left, mut right) = (Vec::new(), Vec::new());
                    for i in 0..width {
                        left.extend([a, param(2, i)]);
                        right.extend([b, Ty::I32]);
                    }
                    let (answer, work) = hd_types::work::count_type_nodes(|| {
                        can_coincide(p, tuple(&left), tuple(&right))
                    });
                    assert!(answer);
                    costs.push(work);
                }
                assert!(
                    costs[1] <= 11 * costs[0],
                    "width 100/1000, shared depth20: {costs:?}"
                );
                for kind in 0..8 {
                    let x = param(1, 0);
                    let mut rhs = x;
                    for _ in 0..1000 {
                        let row = p.row(&hd_types::RowData {
                            keys: vec![p.intern_ty(&TyData::TraitValue {
                                def: DefId::from_raw(3),
                                args: p.list(&[rhs]),
                                bindings: Vec::new(),
                            })],
                            params: Vec::new(),
                        });
                        rhs = p.intern_ty(&match kind {
                            0 => TyData::Ref {
                                inner: rhs,
                                mutable: false,
                                counted: true,
                            },
                            1 => TyData::Fn {
                                params: p.list(&[rhs]),
                                result: rhs,
                                row: hd_types::RowId::EMPTY,
                                suspends: false,
                                inputs: hd_types::Inputs::Fixed,
                                deps: hd_types::FnDeps::NONE,
                            },
                            2 => TyData::TraitValue {
                                def: DefId::from_raw(3),
                                args: p.list(&[rhs]),
                                bindings: Vec::new(),
                            },
                            3 => TyData::Opaque {
                                owner: DefId::from_raw(3),
                                site: 0,
                                borrows: true,
                                args: p.list(&[rhs]),
                                bound: rhs,
                            },
                            4 => TyData::Assoc {
                                assoc: DefId::from_raw(3),
                                trait_: DefId::from_raw(3),
                                self_ty: rhs,
                                args: TyList::EMPTY,
                            },
                            5 => TyData::Row(row),
                            6 => TyData::Context(row),
                            _ => TyData::Union(p.list(&[rhs, rhs])),
                        });
                    }
                    assert!(!can_coincide(p, x, rhs), "cycle through holder {kind}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn shared_binding_constraints_accumulate_and_existing_infer_ids_stay_rigid() {
        let (global, local) = (InternPool::new(), LocalPool::new());
        let p = Types::with_local(&global, &local);
        let x = p.intern_ty(&TyData::Param(ParamRef {
            owner: DefId::from_raw(1),
            index: 0,
        }));
        let record = |bindings| {
            p.intern_ty(&TyData::TraitValue {
                def: DefId::from_raw(2),
                args: TyList::EMPTY,
                bindings,
            })
        };
        let field = DefId::from_raw(3);
        let other = DefId::from_raw(4);
        let open = record(Vec::new());
        let number = record(vec![(field, Ty::I32)]);
        let boolean = record(vec![(field, Ty::BOOL)]);
        let different = record(vec![(other, Ty::BOOL)]);
        let tuple = |xs: &[Ty]| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(xs),
                rest: None,
            })
        };
        assert!(can_coincide(p, number, different));
        assert!(!can_coincide(p, number, boolean));
        assert!(can_coincide(p, tuple(&[x, x]), tuple(&[number, different])));
        for values in [[open, number, boolean], [number, open, boolean]] {
            assert!(
                !can_coincide(p, tuple(&[x, x, x]), tuple(&values)),
                "one substitution must meet every field constraint"
            );
        }
        let mut caller = InferTable::default();
        let (a, b) = (
            caller.fresh(p, VarKind::General),
            caller.fresh(p, VarKind::General),
        );
        assert!(!can_coincide(p, a, b));
        assert!(can_coincide(p, x, a));
        assert!(!can_coincide(p, tuple(&[x, x]), tuple(&[a, b])));
        assert_eq!(caller.shallow(p, a), a);
        assert_eq!(caller.shallow(p, b), b);
    }
}
