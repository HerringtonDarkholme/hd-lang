//! Value-relative runtime reference fragments. The checker layout supplies
//! logical origin slots; instance layout later supplies physical positions.
use super::{BodyCx, HashMap, HashSet, ItemData, Layouts, Rc, Ty, TyData, w};
use hd_tir::ir::LoanField;

#[derive(Clone, Debug)]
pub(super) struct Shape {
    pub path: Vec<LoanField>,
    /// None denotes the value's whole origin set, as for an open T.
    pub slots: Option<Vec<usize>>,
    pub expand: bool,
}

#[derive(Default)]
pub(super) struct Shapes {
    done: HashMap<Ty, Rc<Vec<Shape>>>,
}

struct Field {
    ty: Ty,
    path: Vec<LoanField>,
    slots: Option<Vec<usize>>,
}

enum Task {
    Enter(Ty),
    Exit(Ty, Vec<Field>),
}

impl Shapes {
    pub fn of(&mut self, ls: &Layouts<'_>, cx: &BodyCx<'_>, root: Ty) -> Rc<Vec<Shape>> {
        let mut pending = vec![Task::Enter(root)];
        let mut active = HashSet::default();
        while let Some(task) = pending.pop() {
            match task {
                Task::Enter(ty) => {
                    if self.done.contains_key(&ty) {
                        continue;
                    }
                    let leaf = match ls.pool.get(ty) {
                        TyData::Ref { counted: false, .. } => Some(vec![Shape {
                            path: Vec::new(),
                            slots: Some(vec![0]),
                            expand: false,
                        }]),
                        TyData::Param(_) | TyData::Assoc { .. } | TyData::Opaque { .. }
                            if ls.holds(ty) =>
                        {
                            Some(vec![Shape {
                                path: Vec::new(),
                                slots: None,
                                expand: true,
                            }])
                        }
                        // These values contain no inline reference holder.
                        TyData::Ref { counted: true, .. } | TyData::Fn { .. } => Some(Vec::new()),
                        _ if !ls.holds(ty) => Some(Vec::new()),
                        _ => None,
                    };
                    if let Some(leaf) = leaf {
                        self.done.insert(ty, Rc::new(leaf));
                        continue;
                    }
                    if !active.insert(ty) {
                        // A by-value layout cycle is boxed. Its contents are
                        // not flattened into this value's holder locals.
                        self.done.insert(ty, Rc::new(Vec::new()));
                        continue;
                    }
                    let groups: Vec<_> = match ls.pool.get(ty) {
                        TyData::Tuple { elems, rest } => vec![(
                            None,
                            ls.pool.list_items(elems).len() + usize::from(rest.is_some()),
                        )],
                        TyData::Adt { def, .. } if def.get().is_some() => cx
                            .lookup
                            .item(def)
                            .map_or_else(Vec::new, |item| match &item.data {
                                ItemData::Struct(fields, _) => vec![(None, fields.len())],
                                ItemData::Enum {
                                    shared, variants, ..
                                } if shared.is_empty() => variants
                                    .iter()
                                    .enumerate()
                                    .map(|(variant, fields)| {
                                        (Some(w(variant)), fields.fields.len())
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            }),
                        _ => Vec::new(),
                    };
                    let mut fields = Vec::new();
                    for (variant, count) in groups {
                        for index in 0..count {
                            let Some((field, slots)) = ls.field_slots(cx, ty, variant, w(index))
                            else {
                                continue;
                            };
                            let path = variant
                                .into_iter()
                                .map(LoanField::Variant)
                                .chain([LoanField::Field(w(index))])
                                .collect();
                            fields.push(Field {
                                ty: field,
                                path,
                                slots,
                            });
                        }
                    }
                    let children: Vec<_> = fields.iter().map(|f| f.ty).collect();
                    pending.push(Task::Exit(ty, fields));
                    pending.extend(children.into_iter().rev().map(Task::Enter));
                }
                Task::Exit(ty, fields) => {
                    active.remove(&ty);
                    let mut shapes = Vec::new();
                    for field in fields {
                        for child in self.done.get(&field.ty).map_or(&[][..], |s| s.as_slice()) {
                            let mut path = field.path.clone();
                            path.extend_from_slice(&child.path);
                            let slots = child
                                .slots
                                .as_ref()
                                .and_then(|slots| {
                                    slots
                                        .iter()
                                        .map(|&slot| field.slots.as_ref()?.get(slot).copied())
                                        .collect::<Option<Vec<_>>>()
                                })
                                .filter(|slots| slots.iter().all(|&s| s < ls.of(ty).slots.len()));
                            shapes.push(Shape {
                                path,
                                slots,
                                expand: child.expand,
                            });
                        }
                    }
                    self.done.insert(ty, Rc::new(shapes));
                }
            }
        }
        Rc::clone(&self.done[&root])
    }
}
