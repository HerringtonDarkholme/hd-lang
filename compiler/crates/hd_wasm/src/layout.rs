//! Value layouts at concrete types (wasm-layout.md §15.1, §15.2): the Wasm
//! values of every hd type in locals, parameters and results, and the
//! shapes emission needs to build and read them. The classes agree with
//! `hd_mono::layout::layout_of` (the value bound, slot sharing, `T?`
//! forms), and add the struct descriptors.

use hd_base::{DefId, StageResult};
use hd_mono::{ProgramEnv, class_ref, is_class_ref, subst};
use hd_types::{InternPool, Prim, Ty, TyData, TyList};

use crate::{VT, WTy, unsupported};

/// The value bound (§15.1).
pub const BOUND: usize = 4;

/// A function value's provider context (codegen.md §12.4, one callable
/// ABI): every closure code takes, after its arguments, the key ids and
/// the providers of the caller's row for it, so a value of any narrower
/// row looks its own keys up and row subsumption is no instruction.
#[must_use]
pub fn ctx_keys() -> WTy {
    WTy::Array(VT::I64)
}

/// The providers of a context: per key, its value and its vtable.
#[must_use]
pub fn ctx_provs() -> WTy {
    WTy::Array(VT::Eq)
}

/// The id a context gives a key: the low bits of its stable path hash.
#[must_use]
pub fn key_id(env: &dyn ProgramEnv, k: DefId) -> i64 {
    let bytes = env.path_hash(k).0.to_le_bytes();
    i64::from_le_bytes(bytes[..8].try_into().unwrap_or_default())
}

/// What emission reads about the program's types.
pub struct Lay<'a> {
    pub pool: &'a InternPool,
    pub env: &'a dyn ProgramEnv,
    pub path: &'a dyn Fn(DefId) -> String,
}

/// An enum's value layout: a tag, then shared payload slots (§15.1).
#[derive(Clone, Debug)]
pub struct EnumShape {
    /// Storage of each shared slot after the tag (references as `eqref`).
    pub slots: Vec<VT>,
    /// Per variant, per payload field: its slot indices and exact values.
    pub fields: Vec<Vec<(Vec<usize>, Vec<VT>)>>,
    /// The box past the bound: tag and slots as one immutable struct.
    pub boxed: Option<WTy>,
}

/// The `T?` forms (§15.2 rule 1).
#[derive(Clone, Debug)]
pub enum OptShape {
    /// A nullable reference; null is `.None`.
    NullRef(VT),
    /// `string?`: `(ref null $bytes, i64)`.
    NullStr,
    /// `(i32 tag, dflt(T)...)`.
    Tagged(Vec<VT>),
    /// Past the bound: a nullable box of `dflt(T)`.
    Boxed(WTy, Vec<VT>),
}

#[derive(Clone, Debug)]
pub enum Shape {
    Void,
    Scalar(VT),
    Str,
    ClassRef,
    Data {
        ty: WTy,
        /// Per field: its first struct field and its values.
        fields: Vec<(u32, Vec<VT>)>,
    },
    Enum(EnumShape),
    Opt(OptShape, Vec<VT>),
    Tuple {
        elems: Vec<Vec<VT>>,
        boxed: Option<WTy>,
    },
    List {
        elem: Vec<VT>,
        ty: WTy,
    },
    Map {
        key: Vec<VT>,
        val: Vec<VT>,
        ty: WTy,
    },
    Fn {
        base: WTy,
        code: WTy,
    },
    Dyn {
        trait_: DefId,
        args: TyList,
        vt: WTy,
    },
    Suspend {
        base: WTy,
        poll: WTy,
        result: Vec<VT>,
    },
}

/// Storage of a value in a shared slot or an array element: references
/// erased to `eqref` (A1, §15.2 "Arrays"), string bytes kept nullable.
#[must_use]
pub fn storage(v: &VT) -> VT {
    match v {
        VT::Ref(t, _) if **t == WTy::Bytes => VT::rn(WTy::Bytes),
        VT::Ref(..) => VT::Eq,
        x => x.clone(),
    }
}

/// The closure base struct of a code signature (§15.2 "closure").
#[must_use]
pub fn closure_base(code: &WTy) -> WTy {
    WTy::Struct {
        fields: vec![VT::r(code.clone())],
        sup: None,
        open: true,
    }
}

/// Fields of every suspension (suspension.md §14.1): the cancel function,
/// the resume state, the flags; then, per result layout, the poll
/// function; then, in a generated frame, the awaited child.
pub const F_CANCEL: u32 = 0;
pub const F_STATE: u32 = 1;
pub const F_FLAGS: u32 = 2;
pub const F_POLL: u32 = 3;
pub const F_CHILD: u32 = 4;
/// The first saved field of a generated frame.
pub const F_SAVED: u32 = 5;

/// Frame flags (suspension.md §14.1).
pub const ACTIVE: i32 = 1;
pub const DONE: i32 = 4;
pub const CANCELLED: i32 = 8;

/// The cancel function's type: `(frame) -> ()`, one for every layout.
#[must_use]
pub fn cancel_fn() -> WTy {
    WTy::Func(vec![VT::Eq], vec![])
}

/// `$Task`: the layout-independent prefix of every suspension, through
/// which a parent cancels a child of any result layout.
#[must_use]
pub fn task_base() -> WTy {
    WTy::Struct {
        fields: vec![VT::rn(cancel_fn()), VT::I32, VT::I32],
        sup: None,
        open: true,
    }
}

/// `$Suspend_L` (suspension.md §14.1) and its poll function's type
/// `(frame) -> (i32 ready, dflt(L)...)`. The competing-driver field is
/// not laid out yet.
#[must_use]
pub fn suspend_base(result: &[VT]) -> (WTy, WTy) {
    let mut res = vec![VT::I32];
    res.extend(result.iter().map(VT::dflt));
    let poll = WTy::Func(vec![VT::Eq], res);
    (
        WTy::Struct {
            fields: vec![VT::rn(cancel_fn()), VT::I32, VT::I32, VT::rn(poll.clone())],
            sup: Some(Box::new(task_base())),
            open: true,
        },
        poll,
    )
}

/// A suspension type over a base, with extra fields (a generated frame,
/// a host leaf, a combinator frame).
#[must_use]
pub fn frame_of(base: &WTy, extra: &[VT]) -> WTy {
    let mut fields = base.fields().to_vec();
    fields.extend_from_slice(extra);
    WTy::Struct {
        fields,
        sup: Some(Box::new(base.clone())),
        open: false,
    }
}

/// The struct for erased values that are not one reference: an immutable
/// box of their values (§13.5.1, "As an open value").
#[must_use]
pub fn box_of(vts: &[VT]) -> WTy {
    WTy::Struct {
        fields: vts.to_vec(),
        sup: None,
        open: false,
    }
}

/// A list: `{len, one array per element value}` (structure of arrays).
#[must_use]
pub fn list_ty(elem: &[VT]) -> WTy {
    let mut fields = vec![VT::I32];
    fields.extend(elem.iter().map(|v| VT::r(WTy::Array(storage(v).dflt()))));
    WTy::Struct {
        fields,
        sup: None,
        open: false,
    }
}

/// A map: `{len, key arrays, value arrays}` in insertion order.
#[must_use]
pub fn map_ty(key: &[VT], val: &[VT]) -> WTy {
    let mut fields = vec![VT::I32];
    for v in key.iter().chain(val) {
        fields.push(VT::r(WTy::Array(storage(v).dflt())));
    }
    WTy::Struct {
        fields,
        sup: None,
        open: false,
    }
}

impl Lay<'_> {
    fn strip(&self, t: Ty) -> Ty {
        match self.pool.get(t) {
            TyData::Mut(i) => self.strip(i),
            _ => t,
        }
    }

    pub fn vts(&self, t: Ty) -> StageResult<Vec<VT>> {
        self.vts_at(t, 0)
    }

    fn vts_at(&self, t: Ty, depth: u8) -> StageResult<Vec<VT>> {
        Ok(match self.shape_at(t, depth)? {
            Shape::Void => vec![],
            Shape::Scalar(v) => vec![v],
            Shape::Str => vec![VT::r(WTy::Bytes), VT::I64],
            Shape::ClassRef => vec![VT::Eq],
            Shape::Data { ty, .. } | Shape::List { ty, .. } | Shape::Map { ty, .. } => {
                vec![VT::r(ty)]
            }
            Shape::Enum(e) => match e.boxed {
                Some(b) => vec![VT::r(b)],
                None if e.slots.is_empty() => vec![VT::I32],
                None => [vec![VT::I32], e.slots].concat(),
            },
            Shape::Opt(o, _) => match o {
                OptShape::NullRef(v) => vec![v.dflt()],
                OptShape::NullStr => vec![VT::rn(WTy::Bytes), VT::I64],
                OptShape::Tagged(vs) => [vec![VT::I32], vs].concat(),
                OptShape::Boxed(b, _) => vec![VT::rn(b)],
            },
            Shape::Tuple { elems, boxed } => match boxed {
                Some(b) => vec![VT::r(b)],
                None => elems.concat(),
            },
            Shape::Fn { base, .. } | Shape::Suspend { base, .. } => vec![VT::r(base)],
            Shape::Dyn { vt, .. } => vec![VT::Eq, VT::r(vt)],
        })
    }

    pub fn shape(&self, t: Ty) -> StageResult<Shape> {
        self.shape_at(t, 0)
    }

    fn shape_at(&self, t: Ty, depth: u8) -> StageResult<Shape> {
        if depth > 12 {
            return unsupported("a recursive type layout (subtype enum boxes)");
        }
        let d = depth + 1;
        let pool = self.pool;
        let t = self.strip(t);
        if is_class_ref(pool, t) {
            return Ok(Shape::ClassRef);
        }
        Ok(match pool.get(t) {
            TyData::Prim(p) => match p {
                Prim::I64 | Prim::U64 => Shape::Scalar(VT::I64),
                Prim::F32 => Shape::Scalar(VT::F32),
                Prim::F64 => Shape::Scalar(VT::F64),
                Prim::String => Shape::Str,
                Prim::Void => Shape::Void,
                _ => Shape::Scalar(VT::I32),
            },
            TyData::Never => Shape::Void,
            TyData::Adt { def, args } => {
                let path = (self.path)(def);
                match path.as_str() {
                    "std/core/List" => {
                        let [e] = pool.list_items(args)[..] else {
                            return unsupported("a List without one argument");
                        };
                        let elem = self.vts_at(e, d)?;
                        Shape::List {
                            ty: list_ty(&elem),
                            elem,
                        }
                    }
                    "std/core/Map" => {
                        let [k, v] = pool.list_items(args)[..] else {
                            return unsupported("a Map without two arguments");
                        };
                        let (key, val) = (self.vts_at(k, d)?, self.vts_at(v, d)?);
                        Shape::Map {
                            ty: map_ty(&key, &val),
                            key,
                            val,
                        }
                    }
                    "std/task/Suspend" => {
                        let [r] = pool.list_items(args)[..] else {
                            return unsupported("a Suspend without one argument");
                        };
                        let result = self.vts_at(r, d)?;
                        let (base, poll_ty) = suspend_base(&result);
                        Shape::Suspend {
                            base,
                            poll: poll_ty,
                            result,
                        }
                    }
                    _ => {
                        if let Some(vs) = self.env.enum_variants(def, args) {
                            return Ok(Shape::Enum(self.enum_shape(&vs, d)?));
                        }
                        let Some(fs) = self.env.data_fields(def) else {
                            return unsupported(format!("the layout of `{path}`"));
                        };
                        let mut fields = Vec::new();
                        let mut all = Vec::new();
                        for f in fs {
                            let vs = self.vts_at(subst(pool, self.env, def, args, f), d)?;
                            fields.push((u32::try_from(all.len()).expect("fields"), vs.clone()));
                            all.extend(vs);
                        }
                        Shape::Data {
                            ty: WTy::Struct {
                                fields: all,
                                sup: None,
                                open: false,
                            },
                            fields,
                        }
                    }
                }
            }
            TyData::Tuple { elems, rest: None } => {
                let mut es = Vec::new();
                for e in pool.list_items(elems) {
                    es.push(self.vts_at(e, d)?);
                }
                let n: usize = es.iter().map(Vec::len).sum();
                Shape::Tuple {
                    boxed: (n > BOUND).then(|| box_of(&es.concat())),
                    elems: es,
                }
            }
            TyData::Option(inner) => {
                let iv = self.vts_at(inner, d)?;
                let shape = match iv.as_slice() {
                    [v @ (VT::Ref(_, false) | VT::Eq)] => OptShape::NullRef(v.clone()),
                    _ if self.strip(inner) == Ty::STRING => OptShape::NullStr,
                    _ => {
                        let dv: Vec<VT> = iv.iter().map(VT::dflt).collect();
                        if dv.len() < BOUND {
                            OptShape::Tagged(dv)
                        } else {
                            OptShape::Boxed(box_of(&dv), dv)
                        }
                    }
                };
                Shape::Opt(shape, iv)
            }
            TyData::Fn { params, result, .. } => {
                let mut ps = vec![VT::Eq];
                for p in pool.list_items(params) {
                    ps.extend(self.vts_at(p, d)?);
                }
                ps.push(VT::rn(ctx_keys()));
                ps.push(VT::rn(ctx_provs()));
                let code = WTy::Func(ps, self.vts_at(result, d)?);
                Shape::Fn {
                    base: closure_base(&code),
                    code,
                }
            }
            TyData::TraitValue { def, args, .. } => Shape::Dyn {
                trait_: def,
                args,
                vt: self.vtable_at(def, args, d)?,
            },
            _ => {
                return unsupported(format!(
                    "the layout of the non-concrete type {}",
                    pool.display(t)
                ));
            }
        })
    }

    fn enum_shape(&self, variants: &[Vec<Ty>], d: u8) -> StageResult<EnumShape> {
        let mut slots: Vec<VT> = Vec::new();
        let mut fields = Vec::new();
        for v in variants {
            let mut used = vec![false; slots.len()];
            let mut fs = Vec::new();
            for f in v {
                let vs = self.vts_at(*f, d)?;
                let mut at = Vec::new();
                for x in &vs {
                    let s = storage(x).dflt();
                    let k = (0..slots.len())
                        .find(|&k| !used[k] && slots[k] == s)
                        .unwrap_or_else(|| {
                            slots.push(s.clone());
                            used.push(false);
                            slots.len() - 1
                        });
                    used[k] = true;
                    at.push(k);
                }
                fs.push((at, vs));
            }
            fields.push(fs);
        }
        let boxed =
            (1 + slots.len() > BOUND).then(|| box_of(&[vec![VT::I32], slots.clone()].concat()));
        Ok(EnumShape {
            slots,
            fields,
            boxed,
        })
    }

    /// A trait's vtable (codegen.md §13.5): one code reference per method of
    /// the trait itself, in declaration order.
    pub fn vtable(&self, trait_: DefId, args: TyList) -> StageResult<WTy> {
        self.vtable_at(trait_, args, 0)
    }

    fn vtable_at(&self, trait_: DefId, args: TyList, d: u8) -> StageResult<WTy> {
        let mut fields = Vec::new();
        for m in self.env.trait_methods(trait_) {
            fields.push(VT::r(self.slot_sig_at(trait_, args, m, d + 1)?));
        }
        Ok(WTy::Struct {
            fields,
            sup: None,
            open: false,
        })
    }

    /// A vtable slot's signature: `(eqref self, params...) -> results`; a
    /// suspending method's slot returns its cold `mut Suspend[T]`.
    pub fn slot_sig(&self, trait_: DefId, args: TyList, m: DefId) -> StageResult<WTy> {
        self.slot_sig_at(trait_, args, m, 0)
    }

    fn slot_sig_at(&self, trait_: DefId, args: TyList, m: DefId, d: u8) -> StageResult<WTy> {
        let pool = self.pool;
        let mut all = vec![class_ref(pool)];
        all.extend(pool.list_items(args));
        let full = pool.list(&all);
        let _ = trait_;
        let mut ps = vec![VT::Eq];
        for p in self.env.params(m).unwrap_or_default().into_iter().skip(1) {
            ps.extend(self.vts_at(subst(pool, self.env, m, full, p), d)?);
        }
        let ret = subst(pool, self.env, m, full, self.env.ret(m).unwrap_or(Ty::VOID));
        let mut rs = self.vts_at(ret, d)?;
        if self.env.suspends(m) {
            rs = vec![VT::r(suspend_base(&rs).0)];
        }
        Ok(WTy::Func(ps, rs))
    }
}
