//! The `InternPool` (data-structures.md §3.4, §3.9.2): every type, row and
//! type list is one hash-consed item, so global type equality is one
//! integer compare. Storage is a one-byte tag, a `u32` data word, a `u32`
//! meta word (flags and node count) and variable parts in `extra`.

use std::collections::HashMap;
use std::sync::Mutex;

use hd_base::{AppendVec, DefId, InferVar};

/// A type: an `InternPool` index. Bit 31 set: a body-local index (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ty(pub u32);

/// An interned list of types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TyList(pub u32);

/// An interned row (§3.4 "Rows").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Prim {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Usize,
    F32,
    F64,
    Bool,
    Char,
    String,
    Void,
}

impl Prim {
    pub const ALL: [Prim; 15] = [
        Prim::I8,
        Prim::I16,
        Prim::I32,
        Prim::I64,
        Prim::U8,
        Prim::U16,
        Prim::U32,
        Prim::U64,
        Prim::Usize,
        Prim::F32,
        Prim::F64,
        Prim::Bool,
        Prim::Char,
        Prim::String,
        Prim::Void,
    ];
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Prim::I8 => "i8",
            Prim::I16 => "i16",
            Prim::I32 => "i32",
            Prim::I64 => "i64",
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::Usize => "usize",
            Prim::F32 => "f32",
            Prim::F64 => "f64",
            Prim::Bool => "bool",
            Prim::Char => "char",
            Prim::String => "string",
            Prim::Void => "void",
        }
    }
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(
            self,
            Prim::I8
                | Prim::I16
                | Prim::I32
                | Prim::I64
                | Prim::U8
                | Prim::U16
                | Prim::U32
                | Prim::U64
                | Prim::Usize
        )
    }
    #[must_use]
    pub const fn is_float(self) -> bool {
        matches!(self, Prim::F32 | Prim::F64)
    }
}

/// A declared type parameter: (owner item, index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParamRef {
    pub owner: DefId,
    pub index: u16,
}

/// A declared row parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowParamRef {
    pub owner: DefId,
    pub index: u16,
}

/// A row: sorted keys and declared row parameters. Pending private rows
/// (`PendingPart`) live only in the body and module tiers, never here.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct RowData {
    pub keys: Vec<Ty>,
    pub params: Vec<RowParamRef>,
}

/// The decoded view of one type (§3.4 `TyView`), owned for simplicity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyData {
    Prim(Prim),
    Never,
    Poison,
    Adt {
        def: DefId,
        args: TyList,
    },
    Tuple {
        elems: TyList,
        rest: Option<Ty>,
    },
    Option(Ty),
    Fn {
        params: TyList,
        result: Ty,
        row: RowId,
        suspends: bool,
    },
    TraitValue {
        def: DefId,
        args: TyList,
        bindings: Vec<(DefId, Ty)>,
    },
    Param(ParamRef),
    /// An unnormalized projection: the associated item and its trait reference.
    Assoc {
        assoc: DefId,
        trait_: DefId,
        self_ty: Ty,
        args: TyList,
    },
    Mut(Ty),
    Infer(InferVar),
    /// A canonical placeholder of the solver (trait-solver.md §2.2).
    Canon(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
enum PoolTag {
    Prim,
    Never,
    Poison,
    Adt,
    Tuple,
    TupleRest,
    Option,
    Fn,
    TraitValue,
    Param,
    Assoc,
    Mut,
    Infer,
    Canon,
    List,
    Row,
}

/// Meta flags (§3.9.2).
pub mod meta {
    pub const HAS_INFER: u32 = 1;
    pub const HAS_POISON: u32 = 2;
    pub const HAS_PARAM: u32 = 4;
    pub const HAS_ASSOC: u32 = 8;
    pub const HAS_CANON: u32 = 16;
}

type Key = (PoolTag, u32, Vec<u32>);

/// The global pool. Columns are append-only; a mutex guards only the dedup
/// index and appends (the design's 64 shards come with the pool scheduler).
pub struct InternPool {
    tag: AppendVec<PoolTag>,
    data: AppendVec<u32>,
    meta: AppendVec<u32>,
    extra: AppendVec<Box<[u32]>>,
    index: Mutex<HashMap<Key, u32>>,
}

impl Default for InternPool {
    fn default() -> Self {
        let p = Self {
            tag: AppendVec::new(),
            data: AppendVec::new(),
            meta: AppendVec::new(),
            extra: AppendVec::new(),
            index: Mutex::new(HashMap::new()),
        };
        // Pre-seeding (§3.3 item 5): primitives first, so `Ty::I32` etc. are constants.
        for prim in Prim::ALL {
            p.intern_ty(&TyData::Prim(prim));
        }
        p.intern_ty(&TyData::Never);
        p.intern_ty(&TyData::Poison);
        p.list(&[]);
        p.row(&RowData::default());
        p
    }
}

impl Ty {
    #[must_use]
    pub const fn prim(p: Prim) -> Ty {
        Ty(p as u32)
    }
    pub const I32: Ty = Ty::prim(Prim::I32);
    pub const BOOL: Ty = Ty::prim(Prim::Bool);
    pub const STRING: Ty = Ty::prim(Prim::String);
    pub const VOID: Ty = Ty::prim(Prim::Void);
    pub const NEVER: Ty = Ty(15);
    pub const POISON: Ty = Ty(16);
}

impl TyList {
    pub const EMPTY: TyList = TyList(17);
}

impl RowId {
    pub const EMPTY: RowId = RowId(18);
}

impl InternPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn intern(&self, tag: PoolTag, data: u32, extra: Vec<u32>, meta: u32) -> u32 {
        let key = (tag, data, extra);
        let mut idx = self.index.lock().expect("pool index");
        if let Some(&i) = idx.get(&key) {
            return i;
        }
        let i = self.tag.push(tag);
        self.data.push(data);
        self.meta.push(meta);
        self.extra.push(key.2.clone().into_boxed_slice());
        idx.insert(key, i);
        i
    }

    fn meta_of(&self, t: Ty) -> u32 {
        self.meta[t.0]
    }

    fn list_meta(&self, l: TyList) -> u32 {
        self.meta[l.0]
    }

    pub fn list(&self, tys: &[Ty]) -> TyList {
        let meta = tys.iter().fold(0, |m, t| m | self.meta_of(*t));
        TyList(self.intern(PoolTag::List, 0, tys.iter().map(|t| t.0).collect(), meta))
    }

    #[must_use]
    pub fn list_items(&self, l: TyList) -> Vec<Ty> {
        self.extra[l.0].iter().map(|&w| Ty(w)).collect()
    }

    pub fn row(&self, row: &RowData) -> RowId {
        let mut keys = row.keys.clone();
        keys.sort_by_key(|t| t.0);
        keys.dedup();
        let mut words: Vec<u32> = vec![u32::try_from(keys.len()).expect("row keys")];
        words.extend(keys.iter().map(|t| t.0));
        for p in &row.params {
            words.push(p.owner.raw());
            words.push(u32::from(p.index));
        }
        RowId(self.intern(PoolTag::Row, 0, words, 0))
    }

    #[must_use]
    pub fn row_data(&self, r: RowId) -> RowData {
        let w = &self.extra[r.0];
        let n = w[0] as usize;
        let keys = w[1..=n].iter().map(|&k| Ty(k)).collect();
        let params = w[n + 1..]
            .chunks(2)
            .map(|c| RowParamRef {
                owner: DefId::from_raw(c[0]),
                index: u16::try_from(c[1]).expect("row param"),
            })
            .collect();
        RowData { keys, params }
    }

    /// Interns a type. Body-local types (with `Infer`) are interned too in
    /// this skeleton; the design keeps them in a body-local pool (§3.4).
    pub fn intern_ty(&self, t: &TyData) -> Ty {
        use PoolTag as T;
        let (tag, data, extra, meta) = match t {
            TyData::Prim(p) => (T::Prim, *p as u32, vec![], 0),
            TyData::Never => (T::Never, 0, vec![], 0),
            TyData::Poison => (T::Poison, 0, vec![], meta::HAS_POISON),
            TyData::Adt { def, args } => (T::Adt, def.raw(), vec![args.0], self.list_meta(*args)),
            TyData::Tuple { elems, rest: None } => {
                (T::Tuple, elems.0, vec![], self.list_meta(*elems))
            }
            TyData::Tuple {
                elems,
                rest: Some(r),
            } => (
                T::TupleRest,
                elems.0,
                vec![r.0],
                self.list_meta(*elems) | self.meta_of(*r),
            ),
            TyData::Option(inner) => (T::Option, inner.0, vec![], self.meta_of(*inner)),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => (
                T::Fn,
                params.0,
                vec![result.0, row.0, u32::from(*suspends)],
                self.list_meta(*params) | self.meta_of(*result),
            ),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let mut w = vec![args.0];
                let mut m = self.list_meta(*args);
                let mut b = bindings.clone();
                b.sort_by_key(|(d, _)| d.raw());
                for (d, t) in b {
                    w.push(d.raw());
                    w.push(t.0);
                    m |= self.meta_of(t);
                }
                (T::TraitValue, def.raw(), w, m)
            }
            TyData::Param(p) => (
                T::Param,
                p.owner.raw(),
                vec![u32::from(p.index)],
                meta::HAS_PARAM,
            ),
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => (
                T::Assoc,
                assoc.raw(),
                vec![trait_.raw(), self_ty.0, args.0],
                meta::HAS_ASSOC | self.meta_of(*self_ty) | self.list_meta(*args),
            ),
            TyData::Mut(inner) => (T::Mut, inner.0, vec![], self.meta_of(*inner)),
            TyData::Infer(v) => (T::Infer, v.raw(), vec![], meta::HAS_INFER),
            TyData::Canon(i) => (T::Canon, u32::from(*i), vec![], meta::HAS_CANON),
        };
        Ty(self.intern(tag, data, extra, meta))
    }

    /// Decodes a type (§3.4 `TyView`).
    #[must_use]
    pub fn get(&self, t: Ty) -> TyData {
        let d = self.data[t.0];
        let x = &self.extra[t.0];
        match self.tag[t.0] {
            PoolTag::Prim => TyData::Prim(Prim::ALL[d as usize]),
            PoolTag::Never => TyData::Never,
            PoolTag::Poison => TyData::Poison,
            PoolTag::Adt => TyData::Adt {
                def: DefId::from_raw(d),
                args: TyList(x[0]),
            },
            PoolTag::Tuple => TyData::Tuple {
                elems: TyList(d),
                rest: None,
            },
            PoolTag::TupleRest => TyData::Tuple {
                elems: TyList(d),
                rest: Some(Ty(x[0])),
            },
            PoolTag::Option => TyData::Option(Ty(d)),
            PoolTag::Fn => TyData::Fn {
                params: TyList(d),
                result: Ty(x[0]),
                row: RowId(x[1]),
                suspends: x[2] != 0,
            },
            PoolTag::TraitValue => TyData::TraitValue {
                def: DefId::from_raw(d),
                args: TyList(x[0]),
                bindings: x[1..]
                    .chunks(2)
                    .map(|c| (DefId::from_raw(c[0]), Ty(c[1])))
                    .collect(),
            },
            PoolTag::Param => TyData::Param(ParamRef {
                owner: DefId::from_raw(d),
                index: u16::try_from(x[0]).expect("param"),
            }),
            PoolTag::Assoc => TyData::Assoc {
                assoc: DefId::from_raw(d),
                trait_: DefId::from_raw(x[0]),
                self_ty: Ty(x[1]),
                args: TyList(x[2]),
            },
            PoolTag::Mut => TyData::Mut(Ty(d)),
            PoolTag::Infer => TyData::Infer(InferVar::from_raw(d)),
            PoolTag::Canon => TyData::Canon(u8::try_from(d).expect("canon")),
            PoolTag::List | PoolTag::Row => unreachable!("not a type index"),
        }
    }

    #[must_use]
    pub fn has_infer(&self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_INFER != 0
    }
    #[must_use]
    pub fn has_poison(&self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_POISON != 0
    }
    #[must_use]
    pub fn has_param(&self, t: Ty) -> bool {
        self.meta_of(t) & meta::HAS_PARAM != 0
    }
    #[must_use]
    pub fn len(&self) -> u32 {
        self.tag.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tag.is_empty()
    }

    /// Prints a type with run IDs for items (`#n`); output that a user
    /// sees goes through stable paths instead (§6.5).
    #[must_use]
    pub fn display(&self, t: Ty) -> String {
        let list = |l: TyList| {
            self.list_items(l)
                .iter()
                .map(|x| self.display(*x))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self.get(t) {
            TyData::Prim(p) => p.name().to_owned(),
            TyData::Never => "never".into(),
            TyData::Poison => "{poison}".into(),
            TyData::Adt { def, args } if args == TyList::EMPTY => format!("#{}", def.raw()),
            TyData::Adt { def, args } => format!("#{}[{}]", def.raw(), list(args)),
            TyData::Tuple { elems, rest } => {
                format!(
                    "({}{})",
                    list(elems),
                    rest.map_or(String::new(), |r| format!(", ...{}", self.display(r)))
                )
            }
            TyData::Option(i) => format!("{}?", self.display(i)),
            TyData::Fn {
                params,
                result,
                suspends,
                ..
            } => {
                format!(
                    "fn{}({}) -> {}",
                    if suspends { "!" } else { "" },
                    list(params),
                    self.display(result)
                )
            }
            TyData::TraitValue { def, .. } => format!("dyn #{}", def.raw()),
            TyData::Param(p) => format!("T{}@{}", p.index, p.owner.raw()),
            TyData::Assoc { assoc, self_ty, .. } => {
                format!("<{}>::#{}", self.display(self_ty), assoc.raw())
            }
            TyData::Mut(i) => format!("mut {}", self.display(i)),
            TyData::Infer(v) => format!("?{}", v.raw()),
            TyData::Canon(i) => format!("^{i}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InternPool, Prim, RowData, RowId, Ty, TyData, TyList};
    use hd_base::DefId;

    #[test]
    fn preseeded_constants() {
        let p = InternPool::new();
        assert_eq!(p.get(Ty::I32), TyData::Prim(Prim::I32));
        assert_eq!(p.get(Ty::NEVER), TyData::Never);
        assert_eq!(p.get(Ty::POISON), TyData::Poison);
        assert_eq!(p.list(&[]), TyList::EMPTY);
        assert_eq!(p.row(&RowData::default()), RowId::EMPTY);
    }

    #[test]
    fn every_form_round_trips_and_dedups() {
        let p = InternPool::new();
        let list = p.list(&[Ty::I32, Ty::STRING]);
        let def = DefId::from_raw(7);
        let row = p.row(&RowData {
            keys: vec![Ty::STRING, Ty::I32],
            params: vec![],
        });
        let forms = [
            TyData::Adt { def, args: list },
            TyData::Tuple {
                elems: list,
                rest: None,
            },
            TyData::Tuple {
                elems: list,
                rest: Some(Ty::BOOL),
            },
            TyData::Option(Ty::I32),
            TyData::Fn {
                params: list,
                result: Ty::VOID,
                row,
                suspends: true,
            },
            TyData::TraitValue {
                def,
                args: TyList::EMPTY,
                bindings: vec![(DefId::from_raw(9), Ty::I32)],
            },
            TyData::Param(super::ParamRef {
                owner: def,
                index: 1,
            }),
            TyData::Assoc {
                assoc: DefId::from_raw(3),
                trait_: def,
                self_ty: Ty::I32,
                args: list,
            },
            TyData::Mut(Ty::STRING),
            TyData::Infer(hd_base::InferVar::from_raw(0)),
            TyData::Canon(2),
        ];
        for f in forms {
            let t = p.intern_ty(&f);
            assert_eq!(p.get(t), f);
            assert_eq!(p.intern_ty(&f), t, "hash-consed");
        }
        assert_eq!(p.row_data(row).keys.len(), 2);
        let opt = p.intern_ty(&TyData::Option(Ty::POISON));
        assert!(p.has_poison(opt));
    }
}
