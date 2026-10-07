//! Run-scoped tables: the `DefId` table over stable paths, the type pool
//! (data-structures.md §3.9.2, simplest form), and the global constant pool
//! that TIR's constant `Ref`s name (checking-and-tir.md §4.13.11). Every ID
//! here is a run ID: it never reaches a cache key or an entry.

use std::collections::HashMap;

use hd_base::{Hash128, Symbol};
use hd_intern::Interner;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ty(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyKind {
    Void,
    Never,
    Bool,
    I32,
    /// A type parameter of the enclosing generic item, by index.
    Param(u32),
    /// `Self` inside a trait.
    SelfTy,
    Adt(DefId),
    /// The `REF` class: a type argument erased to "one non-null reference"
    /// in a move-only generic instance (codegen.md §13.2, A1).
    ClassRef,
}

/// The content form of a type: stable paths, no IDs. Interfaces, entries and
/// keys use it (`canon(T)`, codegen.md §13.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CTy {
    Void,
    Never,
    Bool,
    I32,
    Param(u32),
    SelfTy,
    Adt(String),
    ClassRef,
}

impl CTy {
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            CTy::Void => out.push(0),
            CTy::Never => out.push(1),
            CTy::Bool => out.push(2),
            CTy::I32 => out.push(3),
            CTy::Param(i) => {
                out.push(4);
                out.extend_from_slice(&i.to_le_bytes());
            }
            CTy::SelfTy => out.push(5),
            CTy::Adt(path) => {
                out.push(6);
                put_str(out, path);
            }
            CTy::ClassRef => out.push(7),
        }
    }
    pub fn decode(r: &mut Reader<'_>) -> CTy {
        match r.u8() {
            0 => CTy::Void,
            1 => CTy::Never,
            2 => CTy::Bool,
            3 => CTy::I32,
            4 => CTy::Param(r.u32()),
            5 => CTy::SelfTy,
            6 => CTy::Adt(r.str()),
            7 => CTy::ClassRef,
            t => panic!("bad type tag {t}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FnSig {
    /// Type parameters, each with an optional trait bound.
    pub generics: Vec<(Symbol, Option<DefId>)>,
    pub params: Vec<(Symbol, Ty)>,
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub enum DefKind {
    Fn(FnSig),
    Data(Vec<(Symbol, Ty)>),
    Trait(Vec<(Symbol, FnSig)>),
    Impl { trait_: DefId, target: Ty, methods: Vec<(Symbol, DefId)> },
    /// A method of an impl: the impl and the trait method's index.
    ImplMethod { impl_: DefId, index: u32, sig: FnSig },
}

#[derive(Default)]
pub struct World {
    pub syms: Interner,
    paths: Vec<String>,
    path_ids: HashMap<String, DefId>,
    pub defs: HashMap<DefId, DefKind>,
    tys: Vec<TyKind>,
    ty_ids: HashMap<TyKind, Ty>,
    consts: Vec<(Ty, u64)>,
    const_ids: HashMap<(Ty, u64), u32>,
    /// Per-item interface hashes (resolution-and-interfaces.md §4.11.2).
    pub item_hash: HashMap<DefId, Hash128>,
    /// Impl tables: every impl visible in this run, by trait.
    pub impls: HashMap<DefId, Vec<DefId>>,
}

impl World {
    pub fn def(&mut self, path: &str) -> DefId {
        if let Some(&id) = self.path_ids.get(path) {
            return id;
        }
        let id = DefId(u32::try_from(self.paths.len()).expect("defs"));
        self.paths.push(path.to_owned());
        self.path_ids.insert(path.to_owned(), id);
        id
    }
    pub fn lookup(&self, path: &str) -> Option<DefId> {
        self.path_ids.get(path).copied()
    }
    pub fn path(&self, id: DefId) -> &str {
        &self.paths[id.0 as usize]
    }
    pub fn ty(&mut self, kind: TyKind) -> Ty {
        if let Some(&t) = self.ty_ids.get(&kind) {
            return t;
        }
        let t = Ty(u32::try_from(self.tys.len()).expect("types"));
        self.tys.push(kind.clone());
        self.ty_ids.insert(kind, t);
        t
    }
    pub fn kind(&self, t: Ty) -> &TyKind {
        &self.tys[t.0 as usize]
    }
    pub fn konst(&mut self, ty: Ty, bits: u64) -> u32 {
        if let Some(&c) = self.const_ids.get(&(ty, bits)) {
            return c;
        }
        let c = u32::try_from(self.consts.len()).expect("consts");
        assert!(c < 1 << 31, "constant pool exceeds 31 bits");
        self.consts.push((ty, bits));
        self.const_ids.insert((ty, bits), c);
        c
    }
    pub fn const_value(&self, c: u32) -> (Ty, u64) {
        self.consts[c as usize]
    }
    pub fn sym(&mut self, s: &str) -> Symbol {
        self.syms.intern(s)
    }
    pub fn text(&self, s: Symbol) -> &str {
        self.syms.resolve(s).unwrap_or("?")
    }

    pub fn canon(&self, t: Ty) -> CTy {
        match self.kind(t) {
            TyKind::Void => CTy::Void,
            TyKind::Never => CTy::Never,
            TyKind::Bool => CTy::Bool,
            TyKind::I32 => CTy::I32,
            TyKind::Param(i) => CTy::Param(*i),
            TyKind::SelfTy => CTy::SelfTy,
            TyKind::Adt(d) => CTy::Adt(self.path(*d).to_owned()),
            TyKind::ClassRef => CTy::ClassRef,
        }
    }
    pub fn intern_canon(&mut self, c: &CTy) -> Ty {
        let kind = match c {
            CTy::Void => TyKind::Void,
            CTy::Never => TyKind::Never,
            CTy::Bool => TyKind::Bool,
            CTy::I32 => TyKind::I32,
            CTy::Param(i) => TyKind::Param(*i),
            CTy::SelfTy => TyKind::SelfTy,
            CTy::Adt(p) => TyKind::Adt(self.def(p)),
            CTy::ClassRef => TyKind::ClassRef,
        };
        self.ty(kind)
    }
    /// Substitutes type parameters (and `Self`) in `t`.
    pub fn subst(&mut self, t: Ty, args: &[Ty], self_ty: Option<Ty>) -> Ty {
        match *self.kind(t) {
            TyKind::Param(i) => args.get(i as usize).copied().unwrap_or(t),
            TyKind::SelfTy => self_ty.unwrap_or(t),
            _ => t,
        }
    }
    pub fn display(&self, t: Ty) -> String {
        match self.kind(t) {
            TyKind::Void => "void".into(),
            TyKind::Never => "never".into(),
            TyKind::Bool => "bool".into(),
            TyKind::I32 => "i32".into(),
            TyKind::Param(i) => format!("T{i}"),
            TyKind::SelfTy => "Self".into(),
            TyKind::Adt(d) => self.path(*d).to_owned(),
            TyKind::ClassRef => "REF".into(),
        }
    }
}

// ---- canonical bytes and hashing (cache.md §5.3: H(kind tag, fields...)) ----

pub fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&u32::try_from(s.len()).expect("len").to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}
pub fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
pub fn put_hash(out: &mut Vec<u8>, h: Hash128) {
    out.extend_from_slice(&h.0.to_le_bytes());
}

pub struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn u8(&mut self) -> u8 {
        let v = self.bytes[self.pos];
        self.pos += 1;
        v
    }
    pub fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().expect("u32"));
        self.pos += 4;
        v
    }
    pub fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.bytes[self.pos..self.pos + 8].try_into().expect("u64"));
        self.pos += 8;
        v
    }
    pub fn hash(&mut self) -> Hash128 {
        let v = u128::from_le_bytes(self.bytes[self.pos..self.pos + 16].try_into().expect("h"));
        self.pos += 16;
        Hash128(v)
    }
    pub fn str(&mut self) -> String {
        let n = self.u32() as usize;
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + n]).expect("utf8").to_owned();
        self.pos += n;
        s
    }
    pub fn done(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

/// `H(tag, fields...)`: the hasher every key uses.
pub struct KeyHasher(Vec<u8>);
impl KeyHasher {
    pub fn new(tag: &str) -> Self {
        let mut v = Vec::new();
        put_str(&mut v, tag);
        Self(v)
    }
    pub fn str(mut self, s: &str) -> Self {
        put_str(&mut self.0, s);
        self
    }
    pub fn hash(mut self, h: Hash128) -> Self {
        put_hash(&mut self.0, h);
        self
    }
    pub fn bytes(mut self, b: &[u8]) -> Self {
        put_u32(&mut self.0, u32::try_from(b.len()).expect("len"));
        self.0.extend_from_slice(b);
        self
    }
    pub fn finish(self) -> Hash128 {
        hd_base::hash128(&self.0)
    }
}
