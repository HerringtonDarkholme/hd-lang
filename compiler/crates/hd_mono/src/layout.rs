//! Instance keys, A1 classes and Wasm layouts over the full type
//! representation (codegen.md §13.2, §13.3; wasm-layout.md §15.1, §15.2;
//! data-structures.md §3.22).

use hd_base::{DefId, Hash128, InstId, NotImplemented, StableHasher, Stage, StageResult};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

/// The A1 class of a move-only type argument (§13.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum A1Class {
    /// The parameter needs its exact representation.
    Exact,
    /// One non-null reference.
    Ref,
    /// One nullable reference.
    RefNull,
    /// A scalar class, by its Wasm value type.
    Scalar(ValType),
    Void,
}

/// Wasm value types the layouts use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    /// `(ref $T)` or `(ref null $T)`.
    Ref {
        nullable: bool,
    },
    /// `eqref`: erased storage.
    EqRef,
}

/// Layout classes (§15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutClass {
    I32,
    I64,
    F32,
    F64,
    Ref,
    Multi,
    Erased,
    Void,
}

/// A layout: its class and its Wasm values in locals and results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub class: LayoutClass,
    pub values: Vec<ValType>,
    /// Packed storage in a field or array element (`i8` for `bool`, ...), in bits.
    pub packed_bits: Option<u8>,
}

/// The value bound (§15.1): Wasm values after slot sharing; default 4.
pub const VALUE_BOUND: usize = 4;

fn one(class: LayoutClass, v: ValType) -> Layout {
    Layout {
        class,
        values: vec![v],
        packed_bits: None,
    }
}

/// What layouts need to know about declared types (enum shapes).
pub trait LayoutEnv {
    /// Variant payload types of an enum, or `None` for a data type.
    fn enum_variants(&self, def: DefId, args: TyList) -> Option<Vec<Vec<Ty>>>;
}

/// The layout of a type (§15.1 and the value table of §15.2). Every type
/// form has a row; forms that need declared shapes ask `env`.
pub fn layout_of(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> StageResult<Layout> {
    let l = match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::Bool | Prim::I8 | Prim::U8 => Layout {
                packed_bits: Some(8),
                ..one(LayoutClass::I32, ValType::I32)
            },
            Prim::I16 | Prim::U16 => Layout {
                packed_bits: Some(16),
                ..one(LayoutClass::I32, ValType::I32)
            },
            Prim::I32 | Prim::U32 | Prim::Usize | Prim::Char => one(LayoutClass::I32, ValType::I32),
            Prim::I64 | Prim::U64 => one(LayoutClass::I64, ValType::I64),
            Prim::F32 => one(LayoutClass::F32, ValType::F32),
            Prim::F64 => one(LayoutClass::F64, ValType::F64),
            // A view: (ref $bytes, i64 span).
            Prim::String => Layout {
                class: LayoutClass::Multi,
                values: vec![ValType::Ref { nullable: false }, ValType::I64],
                packed_bits: None,
            },
            Prim::Void => Layout {
                class: LayoutClass::Void,
                values: vec![],
                packed_bits: None,
            },
        },
        TyData::Never => Layout {
            class: LayoutClass::Void,
            values: vec![],
            packed_bits: None,
        },
        TyData::Adt { def, args } => match env.enum_variants(def, args) {
            None => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
            Some(vs) if vs.iter().all(Vec::is_empty) => one(LayoutClass::I32, ValType::I32),
            Some(vs) => {
                // Slot sharing: a tag, then per value type the max count over variants.
                let mut slots: Vec<(ValType, usize)> = Vec::new();
                for v in &vs {
                    let mut here: Vec<(ValType, usize)> = Vec::new();
                    for f in v {
                        for val in layout_of(pool, env, *f)?.values {
                            let val = if matches!(val, ValType::Ref { .. }) {
                                ValType::EqRef
                            } else {
                                val
                            };
                            match here.iter_mut().find(|s| s.0 == val) {
                                Some(s) => s.1 += 1,
                                None => here.push((val, 1)),
                            }
                        }
                    }
                    for (val, n) in here {
                        match slots.iter_mut().find(|s| s.0 == val) {
                            Some(s) => s.1 = s.1.max(n),
                            None => slots.push((val, n)),
                        }
                    }
                }
                let mut values = vec![ValType::I32];
                for (val, n) in slots {
                    values.extend(std::iter::repeat_n(val, n));
                }
                if values.len() > VALUE_BOUND {
                    one(LayoutClass::Ref, ValType::Ref { nullable: false })
                } else {
                    Layout {
                        class: LayoutClass::Multi,
                        values,
                        packed_bits: None,
                    }
                }
            }
        },
        TyData::Tuple { elems, rest: None } => {
            let mut values = Vec::new();
            for e in pool.list_items(elems) {
                values.extend(layout_of(pool, env, e)?.values);
            }
            match values.len() {
                0 => Layout {
                    class: LayoutClass::Void,
                    values,
                    packed_bits: None,
                },
                n if n <= VALUE_BOUND => Layout {
                    class: LayoutClass::Multi,
                    values,
                    packed_bits: None,
                },
                _ => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
            }
        }
        TyData::Tuple { rest: Some(_), .. } => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "layout of an open tuple (rest element)",
            ));
        }
        TyData::Option(inner) => {
            let li = layout_of(pool, env, inner)?;
            match (li.class, li.values.as_slice()) {
                (LayoutClass::Ref, [ValType::Ref { .. }]) => {
                    one(LayoutClass::Ref, ValType::Ref { nullable: true })
                }
                (LayoutClass::Multi, [ValType::Ref { .. }, ValType::I64])
                    if inner == Ty::STRING =>
                {
                    Layout {
                        class: LayoutClass::Multi,
                        values: vec![ValType::Ref { nullable: true }, ValType::I64],
                        packed_bits: None,
                    }
                }
                _ => {
                    let mut values = vec![ValType::I32];
                    values.extend(li.values);
                    if values.len() > VALUE_BOUND {
                        one(LayoutClass::Ref, ValType::Ref { nullable: true })
                    } else {
                        Layout {
                            class: LayoutClass::Multi,
                            values,
                            packed_bits: None,
                        }
                    }
                }
            }
        }
        TyData::Fn { .. } => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
        // (eqref, (ref $VT)).
        TyData::TraitValue { .. } => Layout {
            class: LayoutClass::Multi,
            values: vec![ValType::EqRef, ValType::Ref { nullable: false }],
            packed_bits: None,
        },
        TyData::Mut(inner) => layout_of(pool, env, inner)?,
        TyData::Param(_)
        | TyData::Assoc { .. }
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Poison => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "layout of a non-concrete type (substitution missing)",
            ));
        }
    };
    Ok(l)
}

/// The A1 class of a concrete type argument.
pub fn a1_class(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> StageResult<A1Class> {
    let l = layout_of(pool, env, t)?;
    Ok(match (l.class, l.values.as_slice()) {
        (LayoutClass::Ref, [ValType::Ref { nullable: false }]) => A1Class::Ref,
        (LayoutClass::Ref, [ValType::Ref { nullable: true }]) => A1Class::RefNull,
        (LayoutClass::Void, _) => A1Class::Void,
        (_, [v]) => A1Class::Scalar(*v),
        _ => A1Class::Exact,
    })
}

/// `canon(T)`: the structural encoding over stable paths (§13.3). The
/// caller supplies each item's stable path hash.
pub fn canon(pool: &InternPool, path_hash: &dyn Fn(DefId) -> Hash128, t: Ty, h: &mut StableHasher) {
    match pool.get(t) {
        TyData::Prim(p) => {
            h.u8(0);
            h.u8(p as u8);
        }
        TyData::Never => h.u8(1),
        TyData::Adt { def, args } => {
            h.u8(2);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, args, h);
        }
        TyData::Tuple { elems, rest } => {
            h.u8(3);
            canon_list(pool, path_hash, elems, h);
            if let Some(r) = rest {
                canon(pool, path_hash, r, h);
            }
        }
        TyData::Option(i) => {
            h.u8(4);
            canon(pool, path_hash, i, h);
        }
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => {
            h.u8(5);
            canon_list(pool, path_hash, params, h);
            canon(pool, path_hash, result, h);
            let keys = pool.row_data(row).keys;
            h.u32(u32::try_from(keys.len()).expect("keys"));
            for k in keys {
                canon(pool, path_hash, k, h);
            }
            h.u8(u8::from(suspends));
        }
        TyData::TraitValue { def, args, .. } => {
            h.u8(6);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, args, h);
        }
        TyData::Mut(i) => {
            h.u8(7);
            canon(pool, path_hash, i, h);
        }
        other => {
            h.u8(255);
            h.str(&format!("{other:?}"));
        }
    }
}

fn canon_list(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    l: TyList,
    h: &mut StableHasher,
) {
    let items = pool.list_items(l);
    h.u32(u32::try_from(items.len()).expect("list"));
    for t in items {
        canon(pool, path_hash, t, h);
    }
}

/// A type argument as the instance key sees it: canon, or its class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyArg {
    Canon(Ty),
    Class(A1Class),
}

/// `instance_key = H("inst", item path, sub-body, [canon(arg) or class])` (§13.3).
pub fn instance_key(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    item: DefId,
    sub: u16,
    args: &[KeyArg],
) -> Hash128 {
    let mut h = StableHasher::new("inst");
    h.hash(path_hash(item));
    h.u16(sub);
    h.u32(u32::try_from(args.len()).expect("args"));
    for a in args {
        match a {
            KeyArg::Canon(t) => {
                h.u8(0);
                canon(pool, path_hash, *t, &mut h);
            }
            KeyArg::Class(c) => {
                h.u8(1);
                h.str(&format!("{c:?}"));
            }
        }
    }
    h.finish()
}

/// The per-program instance table (data-structures.md §3.22).
#[derive(Default, Debug)]
pub struct InstanceTable {
    pub item: Vec<DefId>,
    pub sub: Vec<u16>,
    pub args: Vec<TyList>,
    pub depth: Vec<u8>,
    pub parent: Vec<InstId>,
    pub key: Vec<Hash128>,
    index: std::collections::HashMap<(DefId, u16, TyList), InstId>,
}

/// The instantiation depth limits (§13.4).
pub const MAX_DEPTH: u8 = 32;
pub const MAX_CHAIN: u32 = 256;

impl InstanceTable {
    /// Pushes an instance once; returns its id and whether it was new.
    pub fn push(
        &mut self,
        item: DefId,
        sub: u16,
        args: TyList,
        depth: u8,
        parent: InstId,
        key: Hash128,
    ) -> StageResult<(InstId, bool)> {
        if let Some(&id) = self.index.get(&(item, sub, args)) {
            return Ok((id, false));
        }
        if depth > MAX_DEPTH {
            return Err(NotImplemented::new(
                Stage::Collect,
                "instantiation-too-deep diagnostic",
            ));
        }
        let id = InstId::from_raw(u32::try_from(self.item.len()).expect("instances"));
        self.item.push(item);
        self.sub.push(sub);
        self.args.push(args);
        self.depth.push(depth);
        self.parent.push(parent);
        self.key.push(key);
        self.index.insert((item, sub, args), id);
        Ok((id, true))
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.item.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item.is_empty()
    }
    /// Content order: by instance key (§6.5), never by id.
    #[must_use]
    pub fn content_order(&self) -> Vec<InstId> {
        let mut ids: Vec<InstId> = (0..self.len())
            .map(|i| InstId::from_raw(u32::try_from(i).expect("i")))
            .collect();
        ids.sort_by_key(|i| self.key[i.idx()]);
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::{
        A1Class, InstanceTable, KeyArg, LayoutClass, LayoutEnv, ValType, a1_class, instance_key,
        layout_of,
    };
    use hd_base::{DefId, Hash128, InstId};
    use hd_types::{InternPool, Ty, TyData, TyList};

    struct Env;
    impl LayoutEnv for Env {
        fn enum_variants(&self, def: DefId, _: TyList) -> Option<Vec<Vec<Ty>>> {
            match def.raw() {
                1 => Some(vec![vec![], vec![]]),
                2 => Some(vec![vec![Ty::I32], vec![Ty::STRING]]),
                _ => None,
            }
        }
    }

    #[test]
    fn layouts_cover_every_concrete_form() {
        let p = InternPool::new();
        let adt = |d: u32| {
            p.intern_ty(&TyData::Adt {
                def: DefId::from_raw(d),
                args: TyList::EMPTY,
            })
        };
        let data = adt(9);
        assert_eq!(
            layout_of(&p, &Env, Ty::I32).expect("i32").class,
            LayoutClass::I32
        );
        assert_eq!(
            layout_of(&p, &Env, Ty::BOOL).expect("bool").packed_bits,
            Some(8)
        );
        assert_eq!(
            layout_of(&p, &Env, Ty::STRING).expect("str").values.len(),
            2
        );
        assert_eq!(
            layout_of(&p, &Env, adt(1)).expect("tag enum").class,
            LayoutClass::I32
        );
        let value_enum = layout_of(&p, &Env, adt(2)).expect("value enum");
        assert_eq!(value_enum.class, LayoutClass::Multi);
        assert_eq!(
            value_enum.values,
            [ValType::I32, ValType::I32, ValType::EqRef, ValType::I64]
        );
        let opt_data = p.intern_ty(&TyData::Option(data));
        assert_eq!(
            layout_of(&p, &Env, opt_data).expect("opt").values,
            [ValType::Ref { nullable: true }]
        );
        let opt_i32 = p.intern_ty(&TyData::Option(Ty::I32));
        assert_eq!(
            layout_of(&p, &Env, opt_i32).expect("opt i32").values.len(),
            2
        );
        let tv = p.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(3),
            args: TyList::EMPTY,
            bindings: vec![],
        });
        assert_eq!(
            layout_of(&p, &Env, tv).expect("dyn").values[0],
            ValType::EqRef
        );
        let big = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::STRING, Ty::STRING, Ty::I32]),
            rest: None,
        });
        assert_eq!(
            layout_of(&p, &Env, big).expect("boxed tuple").class,
            LayoutClass::Ref
        );
        assert_eq!(a1_class(&p, &Env, data).expect("a1"), A1Class::Ref);
        assert_eq!(a1_class(&p, &Env, opt_data).expect("a1"), A1Class::RefNull);
        assert!(layout_of(&p, &Env, Ty::POISON).is_err());
    }

    #[test]
    fn class_instances_share_one_key() {
        let p = InternPool::new();
        let ph = |d: DefId| Hash128(u128::from(d.raw()) * 7919);
        let push = DefId::from_raw(4);
        let a = instance_key(&p, &ph, push, 0, &[KeyArg::Class(A1Class::Ref)]);
        let b = instance_key(&p, &ph, push, 0, &[KeyArg::Class(A1Class::Ref)]);
        let c = instance_key(&p, &ph, push, 0, &[KeyArg::Canon(Ty::I32)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut t = InstanceTable::default();
        assert!(
            t.push(push, 0, TyList::EMPTY, 0, InstId::NONE, a)
                .expect("push")
                .1
        );
        assert!(
            !t.push(push, 0, TyList::EMPTY, 0, InstId::NONE, a)
                .expect("push")
                .1
        );
        assert!(t.push(push, 1, TyList::EMPTY, 33, InstId::NONE, c).is_err());
    }
}
