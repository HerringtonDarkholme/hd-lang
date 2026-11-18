//! Mandatory self references as monotone Boolean queries over raw declarations.
//! Like rustc's inhabited predicates, structs/tuples combine with OR and enums
//! with AND. Boolean argument contexts replace growing type substitutions.
use crate::body::BodyCx;
use hd_base::{DefId, Fuel, HashMap, NotImplemented, Stage, StageResult};
use hd_resolve::ItemData;
use hd_types::{Ty, TyData};
use std::collections::VecDeque;

// Ordinary generic arities fit in one word, like a small compiler bitset.
// Higher positions use shared persistent paths instead of copying wide keys.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct Bits {
    low: u64,
    high: usize,
}

type Context = Option<(DefId, Bits)>;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Type(Ty, Context),
    Declaration(DefId, Bits),
    Variant(DefId, Bits, usize),
}

#[derive(Clone, Copy)]
enum Use {
    Value,
    Argument(usize),
}

#[derive(Clone, Copy)]
enum Rule {
    Any,
    All(usize),
    Apply { def: DefId, bits: Bits },
}

struct Node {
    value: bool,
    users: Vec<(usize, Use)>,
    rule: Rule,
    queued: bool,
}

#[derive(Clone, Copy)]
enum Work {
    Build(usize, Key),
    True(usize),
    Notify(usize, Use),
    Apply(usize),
}

/// Each Boolean node becomes true once. An application changes context at most
/// once per argument; each context shares all raw type/declaration queries.
/// Distinct Boolean contexts can inherently be numerous. Their actual work is
/// charged to the existing item budget (`fn.limit.complexity`), never a new cap.
pub(crate) struct Needs {
    ids: HashMap<Key, usize>,
    nodes: Vec<Node>,
    todo: VecDeque<Work>,
    // Hash-consed persistent bit trie: 0 is empty, 1 is a true leaf. Updating
    // one argument costs a machine word's bits, independent of generic width.
    bits: Vec<(usize, usize)>,
    bit_ids: HashMap<(usize, usize), usize>,
    fuel: Fuel,
}

impl Needs {
    pub(crate) fn new(fuel: u64) -> Self {
        Self {
            ids: HashMap::default(),
            nodes: Vec::new(),
            todo: VecDeque::new(),
            bits: vec![(0, 0); 2],
            bit_ids: HashMap::default(),
            fuel: Fuel::new(fuel),
        }
    }

    fn tick(&mut self) -> StageResult<()> {
        hd_types::work::type_node();
        if self.fuel.charge(1) {
            Ok(())
        } else {
            Err(NotImplemented::out_of_fuel(
                Stage::Body,
                "checking mandatory self references exceeded the item's work budget",
            ))
        }
    }

    fn intern(&mut self, key: Key) -> StageResult<usize> {
        if let Some(&id) = self.ids.get(&key) {
            return Ok(id);
        }
        self.tick()?;
        let id = self.nodes.len();
        self.nodes.push(Node {
            value: false,
            users: Vec::new(),
            rule: Rule::Any,
            queued: false,
        });
        self.ids.insert(key, id);
        self.todo.push_back(Work::Build(id, key));
        Ok(id)
    }

    fn edge(&mut self, key: Key, parent: usize, use_: Use) -> StageResult<()> {
        self.tick()?;
        let child = self.intern(key)?;
        if self.nodes[child].value {
            self.todo.push_back(Work::Notify(parent, use_));
        } else {
            self.nodes[child].users.push((parent, use_));
        }
        Ok(())
    }

    fn has_bit(&self, bits: Bits, index: usize) -> bool {
        if index < u64::BITS as usize {
            return bits.low >> index & 1 != 0;
        }
        let mut root = bits.high;
        for bit in (0..usize::BITS).rev() {
            if root == 0 {
                return false;
            }
            let (left, right) = self.bits[root];
            root = if index >> bit & 1 == 0 { left } else { right };
        }
        root == 1
    }

    fn set_bit(&mut self, mut bits: Bits, index: usize) -> StageResult<Bits> {
        if index < u64::BITS as usize {
            bits.low |= 1 << index;
            return Ok(bits);
        }
        let mut root = bits.high;
        let mut path = Vec::with_capacity(usize::BITS as usize);
        for bit in (0..usize::BITS).rev() {
            self.tick()?;
            let pair = self.bits[root];
            path.push(pair);
            root = if index >> bit & 1 == 0 {
                pair.0
            } else {
                pair.1
            };
        }
        let mut result = 1;
        for (bit, (left, right)) in path.into_iter().rev().enumerate() {
            self.tick()?;
            let pair = if index >> bit & 1 == 0 {
                (result, right)
            } else {
                (left, result)
            };
            result = if let Some(&id) = self.bit_ids.get(&pair) {
                id
            } else {
                let id = self.bits.len();
                self.bits.push(pair);
                self.bit_ids.insert(pair, id);
                id
            };
        }
        bits.high = result;
        Ok(bits)
    }

    fn queue_apply(&mut self, id: usize) {
        if !self.nodes[id].queued {
            self.nodes[id].queued = true;
            self.todo.push_back(Work::Apply(id));
        }
    }

    fn build(&mut self, cx: &BodyCx<'_>, target: DefId, id: usize, key: Key) -> StageResult<()> {
        let pool = cx.names.pool.types();
        let known = cx.names.known;
        match key {
            Key::Type(ty, context) => match pool.get(ty) {
                TyData::Param(param) => {
                    if context.is_some_and(|(owner, bits)| {
                        owner == param.owner && self.has_bit(bits, usize::from(param.index))
                    }) {
                        self.todo.push_back(Work::True(id));
                    }
                }
                TyData::Ref { inner, .. } => {
                    self.edge(Key::Type(inner, context), id, Use::Value)?;
                }
                TyData::Tuple { elems, rest } => {
                    for &child in pool.list_items(elems) {
                        self.edge(Key::Type(child, context), id, Use::Value)?;
                    }
                    if let Some(rest) = rest {
                        self.edge(Key::Type(rest, context), id, Use::Value)?;
                    }
                }
                TyData::Adt { def, args } => {
                    if def == target {
                        self.todo.push_back(Work::True(id));
                    } else if def == known.list || def == known.map || def == known.option {
                        // `annot.self-ref.needs.stop`: arguments do not contribute.
                    } else if def == known.result {
                        if let Some(&first) = pool.list_items(args).first() {
                            self.edge(Key::Type(first, context), id, Use::Value)?;
                        }
                    } else {
                        self.nodes[id].rule = Rule::Apply {
                            def,
                            bits: Bits::default(),
                        };
                        for (index, &arg) in pool.list_items(args).iter().enumerate() {
                            self.edge(Key::Type(arg, context), id, Use::Argument(index))?;
                        }
                        self.queue_apply(id);
                    }
                }
                TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::Union(_)
                | TyData::Fn { .. }
                | TyData::TraitValue { .. }
                | TyData::Opaque { .. }
                | TyData::Const(_)
                | TyData::Assoc { .. }
                | TyData::Infer(_)
                | TyData::Canon(_)
                | TyData::Row(_)
                | TyData::Context(_) => {}
            },
            Key::Declaration(def, bits) => match cx.lookup.item(def).map(|item| &item.data) {
                Some(ItemData::Struct(fields, _)) => {
                    for field in fields {
                        self.edge(Key::Type(field.ty, Some((def, bits))), id, Use::Value)?;
                    }
                }
                Some(ItemData::Enum { variants, .. }) => {
                    self.nodes[id].rule = Rule::All(variants.len());
                    for index in 0..variants.len() {
                        self.edge(Key::Variant(def, bits, index), id, Use::Value)?;
                    }
                    // An empty enum has no self-containing variant.
                }
                Some(
                    ItemData::Fn(..)
                    | ItemData::Trait(_)
                    | ItemData::Impl { .. }
                    | ItemData::Method { .. }
                    | ItemData::AssocType { .. }
                    | ItemData::Alias { .. }
                    | ItemData::Newtype(..)
                    | ItemData::Builtin,
                )
                | None => {}
            },
            Key::Variant(def, bits, index) => {
                if let Some(ItemData::Enum { variants, .. }) =
                    cx.lookup.item(def).map(|it| &it.data)
                {
                    for field in &variants[index].fields {
                        self.edge(Key::Type(field.ty, Some((def, bits))), id, Use::Value)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn needs(&mut self, cx: &BodyCx<'_>, target: DefId, ty: Ty) -> StageResult<bool> {
        let root = self.intern(Key::Type(ty, None))?;
        while let Some(work) = self.todo.pop_front() {
            self.tick()?;
            match work {
                Work::Build(id, key) => self.build(cx, target, id, key)?,
                Work::True(id) => {
                    if !self.nodes[id].value {
                        self.nodes[id].value = true;
                        for (parent, use_) in std::mem::take(&mut self.nodes[id].users) {
                            self.todo.push_back(Work::Notify(parent, use_));
                        }
                    }
                }
                Work::Notify(id, use_) => {
                    if self.nodes[id].value {
                        continue;
                    }
                    match use_ {
                        Use::Argument(index) => {
                            if let Rule::Apply { def, bits } = self.nodes[id].rule {
                                let bits = self.set_bit(bits, index)?;
                                self.nodes[id].rule = Rule::Apply { def, bits };
                                self.queue_apply(id);
                            }
                        }
                        Use::Value => {
                            if let Rule::All(ref mut remaining) = self.nodes[id].rule {
                                if *remaining > 0 {
                                    *remaining -= 1;
                                    if *remaining == 0 {
                                        self.todo.push_back(Work::True(id));
                                    }
                                }
                            } else {
                                self.todo.push_back(Work::True(id));
                            }
                        }
                    }
                }
                Work::Apply(id) => {
                    self.nodes[id].queued = false;
                    if !self.nodes[id].value
                        && let Rule::Apply { def, bits } = self.nodes[id].rule
                    {
                        self.edge(Key::Declaration(def, bits), id, Use::Value)?;
                    }
                }
            }
        }
        Ok(self.nodes[root].value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::with_checker_items;
    use hd_resolve::{Field, Generic, Item, TypeAttrs, Variant};
    use hd_types::{ParamRef, TyList};

    fn field(names: hd_resolve::Names<'_>, ty: Ty) -> Field {
        Field {
            name: names.syms.intern("value"),
            ty,
            public: true,
            has_default: false,
            embedded: false,
            lifetimes: vec![],
            fn_lifetimes: vec![],
        }
    }

    #[test]
    fn needs_keeps_enum_conjunctions_and_closes_growing_generic_cycles() {
        with_checker_items(
            |names| {
                let p = names.pool.types();
                let target = p.applied(names.item("app.main", "Target"), Ty::I32);
                let mut items = Vec::new();
                for name in ["Either", "Both", "Maybe", "Empty", "Grow", "Reveal"] {
                    let def = names.item("app.main", name);
                    let x = p.intern_ty(&TyData::Param(ParamRef {
                        owner: def,
                        index: 0,
                    }));
                    let y = p.intern_ty(&TyData::Param(ParamRef {
                        owner: def,
                        index: 1,
                    }));
                    let data = match name {
                        "Either" => ItemData::Struct(
                            vec![field(names, x), field(names, y)],
                            TypeAttrs::default(),
                        ),
                        "Grow" => ItemData::Struct(
                            vec![field(names, p.applied(def, p.applied(names.known.list, x)))],
                            TypeAttrs::default(),
                        ),
                        "Reveal" => ItemData::Struct(
                            vec![field(names, p.applied(def, target)), field(names, x)],
                            TypeAttrs::default(),
                        ),
                        _ => {
                            let mut variants = vec![];
                            if name != "Empty" {
                                variants.push(Variant {
                                    name: names.syms.intern("A"),
                                    def: names.item("app.main", &format!("{name}.A")),
                                    fields: vec![field(names, x)],
                                });
                                variants.push(Variant {
                                    name: names.syms.intern("B"),
                                    def: names.item("app.main", &format!("{name}.B")),
                                    fields: if name == "Maybe" {
                                        vec![]
                                    } else {
                                        vec![field(names, y)]
                                    },
                                });
                            }
                            ItemData::Enum {
                                shared: vec![],
                                variants,
                                attrs: TypeAttrs::default(),
                            }
                        }
                    };
                    let mut item = Item::new(def, names.syms.intern(name), true, data);
                    for param in ["T", "U"].iter().take(match name {
                        "Empty" => 0,
                        "Grow" | "Reveal" | "Maybe" => 1,
                        _ => 2,
                    }) {
                        item.generics.push(Generic::plain(names.syms.intern(param)));
                    }
                    items.push(item);
                }
                items
            },
            |ck| {
                let p = ck.cx.names.pool.types();
                let target = ck.cx.names.item("app.main", "Target");
                let own = p.applied(target, Ty::I32);
                let adt = |name, args: &[Ty]| {
                    p.intern_ty(&TyData::Adt {
                        def: ck.cx.names.item("app.main", name),
                        args: p.list(args),
                    })
                };
                let mut graph = Needs::new(ck.cx.fuel);
                for (ty, expected) in [
                    (adt("Either", &[own, Ty::I32]), true),
                    (adt("Both", &[own, Ty::I32]), false),
                    (adt("Both", &[own, own]), true),
                    (adt("Both", &[adt("Either", &[Ty::I32, own]), own]), true),
                    (adt("Maybe", &[own, own]), false),
                    (adt("Empty", &[]), false),
                    (adt("Grow", &[own]), false),
                    (adt("Reveal", &[Ty::I32]), true),
                    (p.applied(ck.cx.names.known.option, own), false),
                    (p.applied(ck.cx.names.known.list, own), false),
                    (p.applied(ck.cx.names.known.map, own), false),
                    (
                        p.intern_ty(&TyData::Adt {
                            def: ck.cx.names.known.result,
                            args: p.list(&[Ty::I32, own]),
                        }),
                        false,
                    ),
                    (
                        p.intern_ty(&TyData::Adt {
                            def: ck.cx.names.known.result,
                            args: p.list(&[own, Ty::I32]),
                        }),
                        true,
                    ),
                ] {
                    assert_eq!(graph.needs(ck.cx, target, ty).unwrap(), expected);
                    assert_eq!(
                        hd_types::work::count_type_nodes(|| graph
                            .needs(ck.cx, target, ty)
                            .unwrap()),
                        (expected, 0)
                    );
                }
                for mutable in [false, true] {
                    for counted in [false, true] {
                        assert!(
                            graph
                                .needs(
                                    ck.cx,
                                    target,
                                    p.intern_ty(&TyData::Ref {
                                        inner: own,
                                        mutable,
                                        counted
                                    })
                                )
                                .unwrap()
                        );
                    }
                }
                let mut limited = Needs::new(1);
                let error = limited.needs(ck.cx, target, own).unwrap_err();
                assert!(error.out_of_fuel && !error.bug && !error.too_large);
            },
        );
    }

    #[test]
    fn needs_shares_depth_1000_and_width_1000_at_depth_20() {
        std::thread::Builder::new()
            .stack_size(96 << 10)
            // cost: one 96 KiB thread for these generated Boolean graphs.
            .spawn(|| {
                for width in [100, 1_000] {
                    with_checker_items(
                        |names| {
                            let p = names.pool.types();
                            let def = names.item("app.main", "Wide");
                            let fields = (0..width)
                                .map(|index| {
                                    field(
                                        names,
                                        p.intern_ty(&TyData::Param(ParamRef { owner: def, index })),
                                    )
                                })
                                .collect();
                            let mut item = Item::new(
                                def,
                                names.syms.intern("Wide"),
                                true,
                                ItemData::Struct(fields, TypeAttrs::default()),
                            );
                            for i in 0..width {
                                item.generics
                                    .push(Generic::plain(names.syms.intern(&format!("T{i}"))));
                            }
                            vec![item]
                        },
                        |ck| {
                            let p = ck.cx.names.pool.types();
                            let target = ck.cx.names.item("app.main", "Target");
                            let own = p.intern_ty(&TyData::Adt {
                                def: target,
                                args: TyList::EMPTY,
                            });
                            let mut graph = Needs::new(ck.cx.fuel);
                            for depth in [100, 1_000] {
                                let mut ty = own;
                                for _ in 0..depth {
                                    ty = p.intern_ty(&TyData::Tuple {
                                        elems: p.list(&[ty, ty]),
                                        rest: None,
                                    });
                                }
                                let (answer, work) = hd_types::work::count_type_nodes(|| {
                                    graph.needs(ck.cx, target, ty).unwrap()
                                });
                                assert!(answer);
                                assert!(work <= 12 * depth + 12, "shared depth {depth}: {work}");
                            }
                            let mut wide = p.intern_ty(&TyData::Adt {
                                def: ck.cx.names.item("app.main", "Wide"),
                                args: p.list(&vec![own; usize::from(width)]),
                            });
                            for _ in 0..20 {
                                wide = p.intern_ty(&TyData::Tuple {
                                    elems: p.list(&[wide, wide]),
                                    rest: None,
                                });
                            }
                            let (answer, work) = hd_types::work::count_type_nodes(|| {
                                graph.needs(ck.cx, target, wide).unwrap()
                            });
                            assert!(answer);
                            assert!(
                                work <= 160 * u64::from(width) + 1_000,
                                "width {width} at depth20: {work}"
                            );
                        },
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
