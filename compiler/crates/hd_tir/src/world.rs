//! Run-scoped tables: the `DefId` table over stable paths, the type pool
//! (data-structures.md §3.9.2, simplest form), and the global constant pool
//! that TIR's constant `Ref`s name (checking-and-tir.md §4.13.11). Every ID
//! here is a run ID: it never reaches a cache key or an entry.

use std::collections::HashMap;

use hd_base::{Hash128, Symbol};
use hd_iface::{CItem, CSig, CTy, HeaderItem, KeyHasher, encode_item};
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
    Impl {
        trait_: DefId,
        target: Ty,
        methods: Vec<(Symbol, DefId)>,
    },
    /// A method of an impl: the impl and the trait method's index.
    ImplMethod {
        impl_: DefId,
        index: u32,
        sig: FnSig,
    },
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
    #[must_use]
    pub fn lookup(&self, path: &str) -> Option<DefId> {
        self.path_ids.get(path).copied()
    }
    #[must_use]
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
    #[must_use]
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
    #[must_use]
    pub fn const_value(&self, c: u32) -> (Ty, u64) {
        self.consts[c as usize]
    }
    pub fn sym(&mut self, s: &str) -> Symbol {
        self.syms.intern(s)
    }
    #[must_use]
    pub fn text(&self, s: Symbol) -> &str {
        self.syms.resolve(s).unwrap_or("?")
    }

    #[must_use]
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
    #[must_use]
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

// ------------------------------------------------------------ world loading

fn lower_sig(w: &mut World, s: &CSig, self_ty: Option<&CTy>) -> FnSig {
    let fix = |c: &CTy| match (c, self_ty) {
        (CTy::SelfTy, Some(t)) => t.clone(),
        _ => c.clone(),
    };
    FnSig {
        generics: s
            .generics
            .iter()
            .map(|(g, b)| (w.sym(g), b.as_ref().map(|b| w.def(b))))
            .collect(),
        params: s
            .params
            .iter()
            .map(|(p, t)| (w.sym(p), w.intern_canon(&fix(t))))
            .collect(),
        ret: w.intern_canon(&fix(&s.ret)),
    }
}

/// Loads header items into the run's tables. Traits before impls, since an
/// impl method's signature is its trait method's with `Self` replaced.
pub fn load_items(w: &mut World, items: &[HeaderItem], hashes: Option<&[Hash128]>) {
    let mut traits: HashMap<String, Vec<(String, CSig)>> = HashMap::new();
    for (i, h) in items.iter().enumerate() {
        let id = w.def(&h.path);
        if let Some(hs) = hashes {
            w.item_hash.insert(id, hs[i]);
        } else {
            let mut b = Vec::new();
            encode_item(&mut b, h);
            w.item_hash
                .insert(id, KeyHasher::new("item").bytes(&b).finish());
        }
        match &h.item {
            CItem::Fn(s) => {
                let sig = lower_sig(w, s, None);
                w.defs.insert(id, DefKind::Fn(sig));
            }
            CItem::Data(fs) => {
                let fields = fs
                    .iter()
                    .map(|(f, t)| (w.sym(f), w.intern_canon(t)))
                    .collect();
                w.defs.insert(id, DefKind::Data(fields));
            }
            CItem::Trait(ms) => {
                traits.insert(h.path.clone(), ms.clone());
                let methods = ms
                    .iter()
                    .map(|(m, s)| (w.sym(m), lower_sig(w, s, None)))
                    .collect();
                w.defs.insert(id, DefKind::Trait(methods));
            }
            CItem::Impl { .. } => {}
        }
    }
    for h in items {
        let CItem::Impl {
            trait_,
            target,
            methods,
        } = &h.item
        else {
            continue;
        };
        let id = w.def(&h.path);
        let tid = w.def(trait_);
        let target_ty = w.intern_canon(target);
        let trait_methods: Vec<(String, CSig)> =
            traits
                .get(trait_)
                .cloned()
                .unwrap_or_else(|| match w.defs.get(&tid) {
                    Some(DefKind::Trait(ms)) => ms
                        .iter()
                        .map(|(m, _)| {
                            (
                                w.text(*m).to_owned(),
                                CSig {
                                    generics: vec![],
                                    params: vec![],
                                    ret: CTy::Void,
                                },
                            )
                        })
                        .collect(),
                    _ => Vec::new(),
                });
        let mut ms = Vec::new();
        for m in methods {
            let mid = w.def(&format!("{}.{m}", h.path));
            let index = trait_methods.iter().position(|(n, _)| n == m).unwrap_or(0);
            // The method's signature: the trait's, with Self as the target.
            let sig = match w.defs.get(&tid) {
                Some(DefKind::Trait(tms)) => tms.get(index).map(|(_, s)| s.clone()),
                _ => None,
            };
            let mut sig = sig.unwrap_or(FnSig {
                generics: vec![],
                params: vec![],
                ret: target_ty,
            });
            for p in &mut sig.params {
                p.1 = w.subst(p.1, &[], Some(target_ty));
            }
            sig.ret = w.subst(sig.ret, &[], Some(target_ty));
            let sym = w.sym(m);
            w.defs.insert(
                mid,
                DefKind::ImplMethod {
                    impl_: id,
                    index: u32::try_from(index).expect("i"),
                    sig,
                },
            );
            ms.push((sym, mid));
        }
        w.defs.insert(
            id,
            DefKind::Impl {
                trait_: tid,
                target: target_ty,
                methods: ms,
            },
        );
        let list = w.impls.entry(tid).or_default();
        if !list.contains(&id) {
            list.push(id);
        }
    }
}
