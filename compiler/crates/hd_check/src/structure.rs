//! The compiler-supplied `Structure` of one derivation (annotations,
//! "The Structure Trait", "Walk, Describe, And Build", "Members And
//! Variants", "Self References", "Handles"; codegen.md §12.3 "Derives").
//!
//! Each derivation `D` (a derived implementation or a derivation block)
//! gets its own `Structure` for its target
//! (`annot.structure.per-derivation`): the bodies of `facts`, `name`,
//! `walk`, `describe` and `build`, as ordinary TIR bodies of hidden
//! methods of `D` (`D`'s `walk` at the hidden path `walk`). The derive
//! instance's `Structure` calls choose `D` itself (`Impl` choice), so
//! collection calls these bodies directly, and a `w.member(h, value)`
//! call is direct once the walker's type is known.
//!
//! Each handle is one more hidden method of `D` with no parameter
//! (`handle V`, `handle V M`), which the traversals call: its closures
//! are instantiated once per derivation, not once per walker. It becomes
//! a constant global when those exist (codegen.md §15.4). The handles and
//! information values are values of `std.structure`'s declarations
//! (`lib/std/structure.hd`), built field by field; their hidden fields
//! (`hd_get`, `hd_holds`, `hd_default`) are closures over the target.
//! The witness field, which no std body reads, holds the member's
//! information as a `dyn Inspectable`.
//!
//! Not generated yet, each a structured "not implemented" that leaves the
//! derivation's `Structure` without bodies (a call of it then stops at
//! collection): facts (declaration facts and member lines) and shared
//! constructor data. Doc comments are not carried: `doc` is `.None`.

use hd_base::{DefId, LocalId, NodeIdx, StageResult};
use hd_intern::PathKind;
use hd_resolve::{FnSig, Generic, Item, ItemData, Names};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, ChoiceKind, Coercion, NONE, Providers, Ref, Tag, TirBuilder, TirSink,
    local_flags,
};
use hd_types::{ParamRef, Ty, TyData, TyList};

use crate::body::{BodyCx, unsupported};
use crate::derive::OptIn;

/// One member of a variant, as a handle describes it.
struct Mem {
    /// `name`, or `_0`, `_1`... for an unnamed payload parameter.
    name: String,
    /// The declared name, which names its default body.
    declared: String,
    position: u32,
    /// Its index among the variant's (or data type's) fields.
    field: u32,
    /// The declared type, over the opt-in's parameters.
    ty: Ty,
    embedded: bool,
    positional: bool,
    has_default: bool,
    omitted: bool,
    /// `SelfRef`'s variant index: 0 `Absent`, 1 `Optional`, 2 `Required`.
    self_ref: u32,
}

/// One variant; a data type is one variant (`annot.variant.data`).
struct Var {
    name: String,
    index: u32,
    /// The variant's item, whose hidden members are its payload defaults;
    /// the data type for a data type.
    def: DefId,
    of_data: bool,
    members: Vec<Mem>,
    self_ref: u32,
}

/// What a derivation adds to its module: hidden methods of the derived
/// implementation and their bodies.
#[derive(Default)]
pub struct Generated {
    pub items: Vec<Item>,
    pub bodies: Vec<Body>,
}

/// The `Structure` bodies of `opt`. `cx` sees the target's module and
/// `std.structure`. `has_facts`: the derivation sees a fact (a decorator
/// on the target or a member, a member line, or a trait-less block).
pub fn structure_bodies(cx: &BodyCx<'_>, opt: &OptIn, has_facts: bool) -> StageResult<Generated> {
    if has_facts {
        return unsupported("the facts of a derivation's `Structure`");
    }
    let names = &cx.names;
    let vars = variants(cx, opt)?;
    let st = names.known.structure;
    let Some(ItemData::Trait(t)) = cx.lookup.item(st).map(|i| &i.data) else {
        return unsupported("a derivation without `std.structure`");
    };
    let mut out = Generated::default();
    let mut handles: Vec<Handle> = Vec::new();
    for &(sym, sdef) in &t.methods {
        let name = names.text(sym).to_owned();
        let Some(sitem) = cx.lookup.item(sdef) else {
            continue;
        };
        let Some(trait_sig) = sitem.sig() else {
            continue;
        };
        let def = names.member(opt.impl_, PathKind::Hidden, &name);
        let sig = subst_sig(names, trait_sig, &|p: ParamRef| {
            if p.owner == st && p.index == 0 {
                Some(opt.target)
            } else if p.owner == sdef {
                Some(param(names, def, p.index))
            } else {
                None
            }
        });
        let g = Gen::new(cx, opt, def);
        let (body, wanted) = match name.as_str() {
            "facts" => g.facts_body(&sig)?,
            "name" => {
                let n = cx
                    .lookup
                    .item(opt.data)
                    .map(|i| names.text(i.name).to_owned())
                    .unwrap_or_default();
                g.const_body(&sig, &n)?
            }
            "walk" => g.walk_body(&sig, &vars)?,
            "describe" => g.describe_body(&sig, &vars)?,
            "build" => g.build_body(&sig, &vars)?,
            _ => continue,
        };
        out.items.push(Item::new(
            def,
            sym,
            false,
            ItemData::Method {
                owner: opt.impl_,
                sig,
                has_body: true,
            },
        ));
        out.bodies.push(body);
        for h in wanted {
            if !handles.contains(&h) {
                handles.push(h);
            }
        }
    }
    // Each handle's own body.
    for h in handles {
        let mut g = Gen::new(cx, opt, h.def);
        let blk = g.b.open_block();
        let v = &vars[h.var];
        let x = match h.mem {
            None => g.new_variant_handle(v)?,
            Some(k) => g.new_field_handle(v, &v.members[k], h.f)?,
        };
        let root = g.b.close_block(blk, Some(x), h.ty, SYN);
        let (body, _) = g.finish(root)?;
        out.items.push(Item::new(
            h.def,
            names.syms.intern("handle"),
            false,
            ItemData::Method {
                owner: opt.impl_,
                sig: FnSig::simple(Vec::new(), Vec::new(), h.ty),
                has_body: true,
            },
        ));
        out.bodies.push(body);
    }
    Ok(out)
}

/// A type parameter of `owner`.
fn param(names: &Names<'_>, owner: DefId, index: u16) -> Ty {
    names
        .pool
        .intern_ty(&TyData::Param(ParamRef { owner, index }))
}

/// A signature with `f` applied to its types and bounds.
pub(crate) fn subst_sig(
    names: &Names<'_>,
    sig: &FnSig,
    f: &dyn Fn(ParamRef) -> Option<Ty>,
) -> FnSig {
    let pool = names.pool;
    let mut s = sig.clone();
    for (_, t) in &mut s.params {
        *t = pool.subst(*t, f);
    }
    s.ret = pool.subst(s.ret, f);
    s.row = pool.subst_row(s.row, f);
    s.generics = sig
        .generics
        .iter()
        .map(|g| Generic {
            bounds: g.bounds.iter().map(|b| pool.subst(*b, f)).collect(),
            default: g.default.map(|d| pool.subst(d, f)),
            ..g.clone()
        })
        .collect();
    s
}

fn strip_mut(names: &Names<'_>, t: Ty) -> Ty {
    match names.pool.get(t) {
        TyData::Mut(x) => x,
        _ => t,
    }
}

/// The target's variants and members, with the derivation's omitted
/// members marked (`annot.omit.skip`).
fn variants(cx: &BodyCx<'_>, opt: &OptIn) -> StageResult<Vec<Var>> {
    let names = &cx.names;
    let pool = names.pool;
    let Some(item) = cx.lookup.item(opt.data) else {
        return unsupported("a derivation target outside the module's view");
    };
    let TyData::Adt { args, .. } = pool.get(opt.target) else {
        return unsupported("a derivation target that is not a data type or enum");
    };
    let args = pool.list_items(args).to_vec();
    let data = opt.data;
    let ty = |t: Ty| {
        pool.subst(t, &|p: ParamRef| {
            (p.owner == data).then(|| args.get(usize::from(p.index)).copied())?
        })
    };
    let member = |i: usize, f: &hd_resolve::Field, omitted: bool| {
        let declared = names.text(f.name).to_owned();
        let positional = declared.starts_with(|c: char| c.is_ascii_digit());
        let t = ty(f.ty);
        Mem {
            name: if positional {
                format!("_{declared}")
            } else {
                declared.clone()
            },
            declared,
            position: u32::try_from(i).unwrap_or(0),
            field: u32::try_from(i).unwrap_or(0),
            ty: t,
            embedded: f.embedded,
            positional,
            has_default: f.has_default,
            omitted,
            self_ref: SelfRefs::new(cx, data).member(t),
        }
    };
    let mut out = Vec::new();
    match &item.data {
        ItemData::Data(fields) => {
            let members: Vec<Mem> = fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let n = names.text(f.name);
                    let label = if n.starts_with(|c: char| c.is_ascii_digit()) {
                        format!("_{n}")
                    } else {
                        n.to_owned()
                    };
                    member(i, f, opt.omitted.contains(&label))
                })
                .collect();
            out.push(Var {
                name: names.text(item.name).to_owned(),
                index: 0,
                def: data,
                of_data: true,
                self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                members,
            });
        }
        ItemData::Enum { shared, variants } => {
            if !shared.is_empty() {
                return unsupported("the shared constructor data of a derivation's variants");
            }
            for (k, v) in variants.iter().enumerate() {
                let members: Vec<Mem> = v
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| member(i, f, false))
                    .collect();
                out.push(Var {
                    name: names.text(v.name).to_owned(),
                    index: u32::try_from(k).unwrap_or(0),
                    def: v.def,
                    of_data: false,
                    self_ref: members.iter().map(|m| m.self_ref).max().unwrap_or(0),
                    members,
                });
            }
        }
        _ => return unsupported("a derivation target that is not a data type or enum"),
    }
    Ok(out)
}

/// Self references (`annot.self-ref.*`): whether a member's type refers
/// to, or needs, the enclosing type `data`.
struct SelfRefs<'a, 'c> {
    cx: &'a BodyCx<'c>,
    data: DefId,
}

impl<'a, 'c> SelfRefs<'a, 'c> {
    fn new(cx: &'a BodyCx<'c>, data: DefId) -> Self {
        Self { cx, data }
    }

    /// `SelfRef`'s index for a member of type `t`.
    fn member(&self, t: Ty) -> u32 {
        if self.needs(t, &mut Vec::new()) {
            2
        } else {
            u32::from(self.refers(t, &mut Vec::new()))
        }
    }

    /// The fields of a data type or each variant of an enum, with `args`
    /// substituted.
    fn fields(&self, def: DefId, args: TyList) -> Option<(bool, Vec<Vec<Ty>>)> {
        let pool = self.cx.names.pool;
        let args = pool.list_items(args).to_vec();
        let ty = |t: Ty| {
            pool.subst(t, &|p: ParamRef| {
                (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
            })
        };
        match &self.cx.lookup.item(def)?.data {
            ItemData::Data(fs) => Some((true, vec![fs.iter().map(|f| ty(f.ty)).collect()])),
            ItemData::Enum { variants, .. } => Some((
                false,
                variants
                    .iter()
                    .map(|v| v.fields.iter().map(|f| ty(f.ty)).collect())
                    .collect(),
            )),
            _ => None,
        }
    }

    /// `annot.self-ref.refers` and `.refers.members`.
    fn refers(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Mut(x) | TyData::Option(x) => self.refers(x, seen),
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.refers(*e, seen))
                    || rest.is_some_and(|r| self.refers(r, seen))
            }
            TyData::Adt { def, args } => {
                def == self.data
                    || pool.list_items(args).iter().any(|a| self.refers(*a, seen))
                    || self
                        .fields(def, args)
                        .is_some_and(|(_, vs)| vs.iter().flatten().any(|f| self.refers(*f, seen)))
            }
            _ => false,
        }
    }

    /// `annot.self-ref.needs.*`.
    fn needs(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.cx.names.pool;
        let k = &self.cx.names.known;
        if seen.contains(&t) {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Mut(x) => self.needs(x, seen),
            TyData::Tuple { elems, rest } => {
                pool.list_items(elems).iter().any(|e| self.needs(*e, seen))
                    || rest.is_some_and(|r| self.needs(r, seen))
            }
            TyData::Adt { def, args } => {
                if def == self.data {
                    return true;
                }
                if def == k.list || def == k.map {
                    return false;
                }
                if def == k.result {
                    let first = pool.list_items(args).first().copied();
                    return first.is_some_and(|x| self.needs(x, seen));
                }
                match self.fields(def, args) {
                    Some((true, vs)) => vs.iter().flatten().any(|f| self.needs(*f, seen)),
                    Some((false, vs)) => {
                        !vs.is_empty()
                            && vs
                                .iter()
                                .all(|v| v.iter().any(|f| self.needs(*f, &mut seen.clone())))
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }
}

/// One generated body under construction.
struct Gen<'a, 'n> {
    names: &'a Names<'n>,
    cx: &'a BodyCx<'n>,
    b: TirBuilder,
    /// The derivation's target, over its parameters.
    target: Ty,
    /// The derivation, whose hidden methods the handles are.
    impl_: DefId,
    /// The handles this body calls, built by their own bodies.
    wanted: Vec<Handle>,
}

/// One handle of a derivation (`annot.handle.constants`): a hidden method
/// of the derivation with no parameter that builds it, so a handle's
/// closures are instantiated once per derivation, not once per walker.
#[derive(Clone, Copy, PartialEq)]
struct Handle {
    var: usize,
    /// The member's index in the variant, or `None` for the variant's
    /// handle.
    mem: Option<usize>,
    /// The handle's `F`: the member's read or declared type.
    f: Ty,
    def: DefId,
    ty: Ty,
}

const SYN: NodeIdx = NodeIdx::NONE;

impl<'a, 'n> Gen<'a, 'n> {
    fn new(cx: &'a BodyCx<'n>, opt: &OptIn, def: DefId) -> Self {
        Gen {
            names: &cx.names,
            cx,
            b: TirBuilder::new(def, BodyKind::DeriveInstance),
            target: opt.target,
            impl_: opt.impl_,
            wanted: Vec::new(),
        }
    }

    /// A call of the derivation's handle method for `h`.
    fn handle_call(&mut self, h: Handle) -> Ref {
        let pool = self.names.pool;
        let n = self
            .cx
            .lookup
            .item(self.impl_)
            .map_or(0, |i| i.generics.len());
        let own: Vec<Ty> = (0..n)
            .map(|i| param(self.names, self.impl_, u16::try_from(i).unwrap_or(u16::MAX)))
            .collect();
        if !self.wanted.contains(&h) {
            self.wanted.push(h);
        }
        self.b.call(
            &Callee::Item {
                def: h.def,
                targs: pool.list(&own),
            },
            &[],
            Providers::None,
            h.ty,
            SYN,
        )
    }

    /// The handle `Variant[S]` of `v`, through its handle method.
    fn variant_handle(&mut self, v: &Var) -> Ref {
        // A variant's index is its place in the variant list.
        let var = v.index as usize;
        let ty = self.adt(self.std("Variant"), &[self.target]);
        let def = self
            .names
            .member(self.impl_, PathKind::Hidden, &format!("handle {}", v.index));
        self.handle_call(Handle {
            var,
            mem: None,
            f: Ty::VOID,
            def,
            ty,
        })
    }

    /// The handle `Field[S, F]` of member `m` of `v`, through its handle
    /// method; a declared-type handle of a `mut` member is its own.
    fn field_handle(&mut self, v: &Var, m: &Mem, f: Ty) -> Ref {
        // A member's position is its place in the variant's members.
        let (var, mem) = (v.index as usize, m.position as usize);
        let ty = self.adt(self.names.known.field, &[self.target, f]);
        let declared = if f == strip_mut(self.names, m.ty) {
            ""
        } else {
            " declared"
        };
        let def = self.names.member(
            self.impl_,
            PathKind::Hidden,
            &format!("handle {} {}{declared}", v.index, m.position),
        );
        self.handle_call(Handle {
            var,
            mem: Some(mem),
            f,
            def,
            ty,
        })
    }

    fn std(&self, name: &str) -> DefId {
        self.names.item("std.structure", name)
    }

    fn adt(&self, def: DefId, args: &[Ty]) -> Ty {
        let pool = self.names.pool;
        pool.intern_ty(&TyData::Adt {
            def,
            args: pool.list(args),
        })
    }

    fn fresh(&self, t: Ty) -> Ty {
        self.names.pool.intern_ty(&TyData::Mut(t))
    }

    fn fn_ty(&self, params: &[Ty], result: Ty) -> Ty {
        let pool = self.names.pool;
        pool.intern_ty(&TyData::Fn {
            params: pool.list(params),
            result,
            row: hd_types::RowId::EMPTY,
            suspends: false,
        })
    }

    /// A field's declared type in a `std.structure` data type, its
    /// parameters replaced by `args`.
    fn field_ty(&self, def: DefId, field: &str, args: &[Ty]) -> StageResult<Ty> {
        let Some(ItemData::Data(fs)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return unsupported("a `std.structure` data type outside the module's view");
        };
        let Some(f) = fs.iter().find(|f| self.names.text(f.name) == field) else {
            return unsupported(format!("the `std.structure` field `{field}`"));
        };
        Ok(self.names.pool.subst(f.ty, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        }))
    }

    /// A data value of `def[args]` from its fields' values by name, in
    /// declaration order.
    fn new_data(&mut self, def: DefId, args: &[Ty], vals: &[(&str, Ref)]) -> StageResult<Ref> {
        let Some(ItemData::Data(fs)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return unsupported("a `std.structure` data type outside the module's view");
        };
        let mut refs = Vec::new();
        for f in fs {
            let n = self.names.text(f.name);
            let Some((_, v)) = vals.iter().find(|(k, _)| *k == n) else {
                return unsupported(format!("the `std.structure` field `{n}`"));
            };
            refs.push(*v);
        }
        let rec = self.b.refs_record(&refs);
        let t = self.fresh(self.adt(def, args));
        Ok(self.b.emit(Tag::NewData, NONE, rec, t, SYN))
    }

    fn str_(&mut self, s: &str) -> Ref {
        self.b.const_str(s)
    }

    fn i32_(&mut self, n: u32) -> Ref {
        self.b.const_value(Ty::I32, u64::from(n))
    }

    fn bool_(&mut self, v: bool) -> Ref {
        self.b.const_value(Ty::BOOL, u64::from(v))
    }

    /// A payload-free variant `index` of `t` (`.None`, `SelfRef.Absent`).
    fn unit_variant(&mut self, index: u32, t: Ty) -> Ref {
        let rec = self.b.refs_record(&[]);
        self.b.emit(Tag::NewVariant, index, rec, t, SYN)
    }

    /// An empty list of type `t` (a `List[...]`).
    fn empty_list(&mut self, t: Ty) -> Ref {
        let rec = self.b.refs_record(&[]);
        let t = self.fresh(strip_mut(self.names, t));
        self.b.emit(Tag::NewList, NONE, rec, t, SYN)
    }

    /// `Facts { items: [] }`.
    fn no_facts(&mut self) -> StageResult<Ref> {
        let facts = self.std("Facts");
        let lt = self.field_ty(facts, "items", &[])?;
        let items = self.empty_list(lt);
        self.new_data(facts, &[], &[("items", items)])
    }

    fn self_ref(&mut self, k: u32) -> Ref {
        let t = self.adt(self.std("SelfRef"), &[]);
        self.unit_variant(k, t)
    }

    /// A closure `fn(params) -> ret` with no capture; `body` returns its
    /// value from the parameters' locals.
    fn closure(
        &mut self,
        params: &[Ty],
        ret: Ty,
        body: impl FnOnce(&mut Self, &[LocalId]) -> StageResult<Ref>,
    ) -> StageResult<Ref> {
        let syms = self.names.syms;
        let ls: Vec<LocalId> = params
            .iter()
            .enumerate()
            .map(|(i, t)| {
                self.b
                    .local(*t, syms.intern(&format!("p{i}")), local_flags::PARAM, SYN)
            })
            .collect();
        let m = self.b.open_sub(&ls);
        let blk = self.b.open_block();
        let v = body(self, &ls)?;
        let tail = (self.b.ty_of(v) != Ty::NEVER).then_some(v);
        let root = self.b.close_block(blk, tail, ret, SYN);
        let ft = self.fn_ty(params, ret);
        Ok(self.b.close_sub(m, root, ft, SYN))
    }

    /// Reads member `m` of variant `v` from `s` (a value of the target):
    /// a field of a data value, or a payload of an enum value that holds
    /// `v`.
    fn read(&mut self, s: Ref, v: &Var, m: &Mem, t: Ty) -> Ref {
        if v.of_data {
            self.b.emit(Tag::Field, s.0, m.field, t, SYN)
        } else {
            let rec = self.b.refs_record(&[Ref(v.index), Ref(m.field)]);
            self.b.emit(Tag::Payload, s.0, rec, t, SYN)
        }
    }

    /// `SwitchTag s [#index then else]` of type `t`.
    fn switch_tag(&mut self, s: Ref, index: u32, then: Ref, other: Ref, t: Ty) -> Ref {
        let rec = self.b.refs_record(&[Ref(index), then, other]);
        self.b.emit(Tag::SwitchTag, s.0, rec, t, SYN)
    }

    /// A categorized panic (`flow.panic.report`), through std's
    /// `panic_with` intrinsic: a `never` value.
    fn panic(&mut self, category: &str, message: &str) -> Ref {
        let def = self.names.item("std.ops", "panic_with");
        let c = self.str_(category);
        let msg = self.str_(message);
        self.b.call(
            &Callee::Item {
                def,
                targs: TyList::EMPTY,
            },
            &[c, msg],
            Providers::None,
            Ty::VOID,
            SYN,
        );
        self.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, SYN)
    }

    /// `VariantInfo` of `v`.
    fn variant_info(&mut self, v: &Var) -> StageResult<Ref> {
        let vi = self.std("VariantInfo");
        let name = self.str_(&v.name);
        let index = self.i32_(v.index);
        let facts = self.no_facts()?;
        let dt = self.field_ty(vi, "doc", &[])?;
        let doc = self.unit_variant(0, dt);
        let of_data = self.bool_(v.of_data);
        let st = self.field_ty(vi, "shared", &[])?;
        let shared = self.empty_list(st);
        let self_ref = self.self_ref(v.self_ref);
        self.new_data(
            vi,
            &[],
            &[
                ("name", name),
                ("index", index),
                ("facts", facts),
                ("doc", doc),
                ("of_data", of_data),
                ("shared", shared),
                ("self_ref", self_ref),
            ],
        )
    }

    /// The handle `Variant[S]` of `v` (`annot.handle.constants`), with
    /// `holds` (`annot.handle.holds`).
    fn new_variant_handle(&mut self, v: &Var) -> StageResult<Ref> {
        let info = self.variant_info(v)?;
        let s = self.target;
        let of_data = v.of_data;
        let index = v.index;
        let holds = self.closure(&[s], Ty::BOOL, |g, ps| {
            if of_data {
                return Ok(g.bool_(true));
            }
            let x = g.b.get(ps[0], s, SYN);
            let tb = g.b.open_block();
            let t = g.bool_(true);
            let then = g.b.close_block(tb, Some(t), Ty::BOOL, SYN);
            let eb = g.b.open_block();
            let f = g.bool_(false);
            let other = g.b.close_block(eb, Some(f), Ty::BOOL, SYN);
            Ok(g.switch_tag(x, index, then, other, Ty::BOOL))
        })?;
        let var = self.std("Variant");
        self.new_data(var, &[s], &[("info", info), ("hd_holds", holds)])
    }

    /// `Member` of `m`.
    fn member_info(&mut self, m: &Mem) -> StageResult<Ref> {
        let mi = self.std("Member");
        let name = self.str_(&m.name);
        let position = self.i32_(m.position);
        let facts = self.no_facts()?;
        let dt = self.field_ty(mi, "doc", &[])?;
        let doc = self.unit_variant(0, dt);
        let embedded = self.bool_(m.embedded);
        let positional = self.bool_(m.positional);
        let self_ref = self.self_ref(m.self_ref);
        self.new_data(
            mi,
            &[],
            &[
                ("name", name),
                ("position", position),
                ("facts", facts),
                ("doc", doc),
                ("embedded", embedded),
                ("positional", positional),
                ("self_ref", self_ref),
            ],
        )
    }

    /// The handle `Field[S, F]` of member `m` of `v`, `F` being `f`
    /// (`annot.handle.*`): `get` reads the member, panicking with
    /// `structure-variant-mismatch` on another variant; `default` runs
    /// the member's declared default.
    fn new_field_handle(&mut self, v: &Var, m: &Mem, f: Ty) -> StageResult<Ref> {
        let info = self.member_info(m)?;
        let s = self.target;
        let get = self.closure(&[s], f, |g, ps| {
            let x = g.b.get(ps[0], s, SYN);
            if v.of_data {
                return Ok(g.read(x, v, m, f));
            }
            let tb = g.b.open_block();
            let val = g.read(x, v, m, f);
            let then = g.b.close_block(tb, Some(val), f, SYN);
            let eb = g.b.open_block();
            g.panic(
                "structure-variant-mismatch",
                &format!("the value does not hold the variant `{}`", v.name),
            );
            let other = g.b.close_block(eb, None, Ty::NEVER, SYN);
            Ok(g.switch_tag(x, v.index, then, other, f))
        })?;
        let has_default = self.bool_(m.has_default);
        let opt = self.names.pool.intern_ty(&TyData::Option(f));
        let default = self.closure(&[], opt, |g, _| {
            if !m.has_default {
                return Ok(g.unit_variant(0, opt));
            }
            let d = g.default_of(v, m, f)?;
            let rec = g.b.refs_record(&[d]);
            Ok(g.b.emit(Tag::NewVariant, 1, rec, opt, SYN))
        })?;
        let inspectable = self.names.pool.intern_ty(&TyData::TraitValue {
            def: self.names.known.inspectable,
            args: TyList::EMPTY,
            bindings: vec![],
        });
        let witness = self
            .b
            .coerce(Coercion::ToTraitValue, NONE, info, inspectable, SYN);
        let field = self.names.known.field;
        self.new_data(
            field,
            &[s, f],
            &[
                ("info", info),
                ("hd_get", get),
                ("hd_has_default", has_default),
                ("hd_default", default),
                ("hd_witness", witness),
            ],
        )
    }

    /// A call of a walker or describer method (`member`, `variant`)
    /// through the body's own parameter `w`: its result type.
    fn protocol_call(
        &mut self,
        trait_: DefId,
        method: &str,
        w: (LocalId, Ty),
        method_args: &[Ty],
        args: &[Ref],
    ) -> StageResult<Ref> {
        let names = self.names;
        let pool = names.pool;
        let Some(ItemData::Trait(t)) = self.cx.lookup.item(trait_).map(|i| &i.data) else {
            return unsupported("a structure protocol trait outside the module's view");
        };
        let Some(&(_, mdef)) = t.methods.iter().find(|(n, _)| names.text(*n) == method) else {
            return unsupported(format!("the protocol method `{method}`"));
        };
        let Some(sig) = self.cx.lookup.item(mdef).and_then(Item::sig) else {
            return unsupported(format!("the protocol method `{method}`"));
        };
        let wt = strip_mut(names, w.1);
        let s = self.target;
        let ret = pool.subst(sig.ret, &|p: ParamRef| {
            if p.owner == trait_ {
                Some(if p.index == 0 { wt } else { s })
            } else if p.owner == mdef {
                method_args.get(usize::from(p.index)).copied()
            } else {
                None
            }
        });
        let ret = crate::call::with_assoc_args(pool.types(), ret, trait_, pool.list(&[s]));
        let recv = self.b.get(w.0, w.1, SYN);
        let mut all = vec![recv];
        all.extend_from_slice(args);
        let mut targs = vec![s];
        targs.extend_from_slice(method_args);
        Ok(self.b.call(
            &Callee::TraitMethod {
                trait_,
                method: mdef,
                self_ty: wt,
                targs: pool.list(&targs),
                choice: (ChoiceKind::Bound, 0),
            },
            &all,
            Providers::None,
            ret,
            SYN,
        ))
    }

    /// `r?` for a `Result[void, E]` in a body whose result type is `ret`.
    fn try_void(&mut self, r: Ref, ret: Ty) -> StageResult<()> {
        let pool = self.names.pool;
        let TyData::Adt { args, .. } = pool.get(strip_mut(self.names, self.b.ty_of(r))) else {
            return unsupported("a protocol method whose result is not a `Result`");
        };
        let err = pool.list_items(args).get(1).copied().unwrap_or(Ty::POISON);
        let ob = self.b.open_block();
        let ok = self.b.close_block(ob, None, Ty::VOID, SYN);
        let eb = self.b.open_block();
        let rec = self.b.refs_record(&[Ref(1), Ref(0)]);
        let e = self.b.emit(Tag::Payload, r.0, rec, err, SYN);
        let rec = self.b.refs_record(&[e]);
        let v = self.b.emit(Tag::NewVariant, 1, rec, ret, SYN);
        self.b.emit(Tag::Return, v.0, NONE, Ty::NEVER, SYN);
        let other = self.b.close_block(eb, None, Ty::NEVER, SYN);
        self.switch_tag(r, 0, ok, other, Ty::VOID);
        Ok(())
    }

    /// The parameters' locals of a body with signature `sig`.
    fn params(&mut self, sig: &FnSig) -> Vec<(LocalId, Ty)> {
        sig.params
            .iter()
            .map(|(n, t)| (self.b.local(*t, *n, local_flags::PARAM, SYN), *t))
            .collect()
    }

    fn finish(self, root: Ref) -> StageResult<(Body, Vec<Handle>)> {
        let wanted = self.wanted;
        let body = self.b.finish(root, &[]).map_err(|e| {
            hd_base::NotImplemented::new(
                hd_base::Stage::Body,
                format!("generated structure TIR verifier: {e:?}"),
            )
        })?;
        Ok((body, wanted))
    }

    /// `facts()`: the type-level facts (none here).
    fn facts_body(mut self, sig: &FnSig) -> StageResult<(Body, Vec<Handle>)> {
        let blk = self.b.open_block();
        let f = self.no_facts()?;
        let root = self.b.close_block(blk, Some(f), sig.ret, SYN);
        self.finish(root)
    }

    /// A body that returns one string constant (`name()`,
    /// `annot.structure.name`).
    fn const_body(mut self, sig: &FnSig, text: &str) -> StageResult<(Body, Vec<Handle>)> {
        let blk = self.b.open_block();
        let v = self.str_(text);
        let root = self.b.close_block(blk, Some(v), sig.ret, SYN);
        self.finish(root)
    }

    /// The traversal of one variant: `variant`, then each member it does
    /// not omit (`annot.walk.match`, `annot.walk.members`,
    /// `annot.describe.order`). `value` is the walked value, or `None`
    /// for `describe`.
    fn visit(
        &mut self,
        trait_: DefId,
        w: (LocalId, Ty),
        v: &Var,
        value: Option<Ref>,
        ret: Ty,
    ) -> StageResult<()> {
        let h = self.variant_handle(v);
        let r = self.protocol_call(trait_, "variant", w, &[], &[h])?;
        self.try_void(r, ret)?;
        for m in v.members.iter().filter(|m| !m.omitted) {
            let f = strip_mut(self.names, m.ty);
            let h = self.field_handle(v, m, f);
            let mut args = vec![h];
            if let Some(s) = value {
                args.push(self.read(s, v, m, f));
            }
            let r = self.protocol_call(trait_, "member", w, &[f], &args)?;
            self.try_void(r, ret)?;
        }
        Ok(())
    }

    /// `.Ok(())` of `ret`.
    fn ok_void(&mut self, ret: Ty) -> Ref {
        let rec = self.b.refs_record(&[Ref(NONE)]);
        self.b.emit(Tag::NewVariant, 0, rec, ret, SYN)
    }

    /// `walk(self, w)`: matches the value once, then visits its variant.
    fn walk_body(mut self, sig: &FnSig, vars: &[Var]) -> StageResult<(Body, Vec<Handle>)> {
        let ps = self.params(sig);
        let [(s, st), w] = ps[..] else {
            return unsupported("a `walk` signature");
        };
        let walker = self.names.known.walker;
        let blk = self.b.open_block();
        let x = self.b.get(s, st, SYN);
        self.dispatch(vars, x, &mut |g, v| g.visit(walker, w, v, Some(x), sig.ret))?;
        let ok = self.ok_void(sig.ret);
        let root = self.b.close_block(blk, Some(ok), sig.ret, SYN);
        self.finish(root)
    }

    /// `describe(d)`: every variant in order (`annot.describe.order`).
    fn describe_body(mut self, sig: &FnSig, vars: &[Var]) -> StageResult<(Body, Vec<Handle>)> {
        let ps = self.params(sig);
        let [d] = ps[..] else {
            return unsupported("a `describe` signature");
        };
        let describer = self.names.known.describer;
        let blk = self.b.open_block();
        for v in vars {
            self.visit(describer, d, v, None, sig.ret)?;
        }
        let ok = self.ok_void(sig.ret);
        let root = self.b.close_block(blk, Some(ok), sig.ret, SYN);
        self.finish(root)
    }

    /// `r?` for a `Result[T, E]`: its `T`, or a return of the `.Err`.
    fn try_value(&mut self, r: Ref, ret: Ty) -> StageResult<Ref> {
        let pool = self.names.pool;
        let TyData::Adt { args, .. } = pool.get(strip_mut(self.names, self.b.ty_of(r))) else {
            return unsupported("a protocol method whose result is not a `Result`");
        };
        let items = pool.list_items(args);
        let (ok_t, err) = (
            items.first().copied().unwrap_or(Ty::POISON),
            items.get(1).copied().unwrap_or(Ty::POISON),
        );
        let ob = self.b.open_block();
        let rec = self.b.refs_record(&[Ref(0), Ref(0)]);
        let v = self.b.emit(Tag::Payload, r.0, rec, ok_t, SYN);
        let ok = self.b.close_block(ob, Some(v), ok_t, SYN);
        let eb = self.b.open_block();
        let rec = self.b.refs_record(&[Ref(1), Ref(0)]);
        let e = self.b.emit(Tag::Payload, r.0, rec, err, SYN);
        let rec = self.b.refs_record(&[e]);
        let v = self.b.emit(Tag::NewVariant, 1, rec, ret, SYN);
        self.b.emit(Tag::Return, v.0, NONE, Ty::NEVER, SYN);
        let other = self.b.close_block(eb, None, Ty::NEVER, SYN);
        Ok(self.switch_tag(r, 0, ok, other, ok_t))
    }

    /// A field of a `std.structure` value by name.
    fn std_field(&mut self, v: Ref, def: DefId, name: &str, t: Ty) -> StageResult<Ref> {
        let Some(ItemData::Data(fs)) = self.cx.lookup.item(def).map(|i| &i.data) else {
            return unsupported("a `std.structure` data type outside the module's view");
        };
        let Some(k) = fs.iter().position(|f| self.names.text(f.name) == name) else {
            return unsupported(format!("the `std.structure` field `{name}`"));
        };
        let k = u32::try_from(k).unwrap_or(0);
        Ok(self.b.emit(Tag::Field, v.0, k, t, SYN))
    }

    /// `SwitchInt v [#value then else]` of type `t`.
    fn switch_int(&mut self, v: Ref, value: u32, then: Ref, other: Ref, t: Ty) -> Ref {
        let rec = self.b.refs_record(&[Ref(value), then, other]);
        self.b.emit(Tag::SwitchInt, v.0, rec, t, SYN)
    }

    /// The declared default of member `m` of `v`, as a value of type `t`
    /// (`annot.handle.default`, `annot.omit.default`).
    fn default_of(&mut self, v: &Var, m: &Mem, t: Ty) -> StageResult<Ref> {
        let body = self.names.member(v.def, PathKind::Hidden, &m.declared);
        let TyData::Adt { args, .. } = self.names.pool.get(self.target) else {
            return unsupported("a derivation target that is not a data type or enum");
        };
        let a = self.b.refs_record(&[Ref(body.raw()), Ref(args.0)]);
        let bw = self.b.refs_record(&[]);
        Ok(self.b.emit(Tag::DefaultCall, a, bw, t, SYN))
    }

    /// Builds variant `v` from the source `src` (`annot.build.next` to
    /// `annot.build.construct`): `next` until the end key, `member` per
    /// key with the value read so far, `missing` per member never named,
    /// then the value from one local per member.
    fn build_variant(&mut self, v: &Var, src: (LocalId, Ty), ret: Ty) -> StageResult<Ref> {
        let names = self.names;
        let pool = names.pool;
        let source = names.known.source;
        let s = self.target;
        let walked: Vec<&Mem> = v.members.iter().filter(|m| !m.omitted).collect();
        let member_t = self.adt(self.std("Member"), &[]);
        let mut infos = Vec::new();
        for m in &walked {
            infos.push(self.member_info(m)?);
        }
        let rec = self.b.refs_record(&infos);
        let lt = self.fresh(self.adt(names.known.list, &[member_t]));
        let infos = self.b.emit(Tag::NewList, NONE, rec, lt, SYN);
        let any = self.closure(&[s], Ty::BOOL, |g, _| Ok(g.bool_(true)))?;
        let members_def = self.std("Members");
        let members = self.new_data(members_def, &[s], &[("infos", infos), ("hd_type", any)])?;
        let slots: Vec<LocalId> = walked
            .iter()
            .map(|m| {
                let t = pool.intern_ty(&TyData::Option(m.ty));
                let l = self.b.local(
                    t,
                    names.syms.intern(&m.name),
                    local_flags::READ | local_flags::ASSIGNED,
                    SYN,
                );
                let none = self.unit_variant(0, t);
                self.b.set(l, none, SYN);
                l
            })
            .collect();
        let lp = self.b.open_loop();
        let lb = self.b.open_block();
        let r = self.protocol_call(source, "next", src, &[], &[members])?;
        let key = self.try_value(r, ret)?;
        let key_def = self.std("Key");
        let end = self.std_field(key, key_def, "is_end", Ty::BOOL)?;
        let tb = self.b.open_block();
        self.b.emit(Tag::Break, lp.0.raw(), NONE, Ty::NEVER, SYN);
        let then = self.b.close_block(tb, None, Ty::NEVER, SYN);
        let rec = self.b.refs_record(&[then, Ref(NONE)]);
        self.b.emit(Tag::If, end.0, rec, Ty::VOID, SYN);
        let info = self.std_field(key, key_def, "info", member_t)?;
        let pos = self.std_field(info, self.std("Member"), "position", Ty::I32)?;
        self.read_key(v, &walked, &slots, pos, src, ret)?;
        let body = self.b.close_block(lb, None, Ty::VOID, SYN);
        self.b.close_loop(lp, body, Ty::VOID, SYN);
        let mut values = Vec::new();
        let mut k = 0;
        for m in &v.members {
            if m.omitted {
                values.push(self.default_of(v, m, m.ty)?);
                continue;
            }
            let slot = slots[k];
            k += 1;
            let ot = pool.intern_ty(&TyData::Option(m.ty));
            let cur = self.b.get(slot, ot, SYN);
            let sb = self.b.open_block();
            let got = self.b.emit(Tag::Unwrap, cur.0, NONE, m.ty, SYN);
            let some = self.b.close_block(sb, Some(got), m.ty, SYN);
            let nb = self.b.open_block();
            let h = self.field_handle(v, m, m.ty);
            let r = self.protocol_call(source, "missing", src, &[m.ty], &[h])?;
            let x = self.try_value(r, ret)?;
            let none = self.b.close_block(nb, Some(x), m.ty, SYN);
            values.push(self.switch_tag(cur, 1, some, none, m.ty));
        }
        let rec = self.b.refs_record(&values);
        Ok(if v.of_data {
            let t = self.fresh(s);
            self.b.emit(Tag::NewData, NONE, rec, t, SYN)
        } else {
            self.b.emit(Tag::NewVariant, v.index, rec, s, SYN)
        })
    }

    /// One key of `build`'s loop: the member at position `pos` gets
    /// `s.member(h, previous)`; a key of no member of the variant panics
    /// (`annot.build.foreign-key`).
    fn read_key(
        &mut self,
        v: &Var,
        walked: &[&Mem],
        slots: &[LocalId],
        pos: Ref,
        src: (LocalId, Ty),
        ret: Ty,
    ) -> StageResult<()> {
        let Some((m, later)) = walked.split_first() else {
            self.panic(
                "structure-variant-mismatch",
                &format!("the key names no member of the variant `{}`", v.name),
            );
            return Ok(());
        };
        let pool = self.names.pool;
        let tb = self.b.open_block();
        let ot = pool.intern_ty(&TyData::Option(m.ty));
        let h = self.field_handle(v, m, m.ty);
        let prev = self.b.get(slots[0], ot, SYN);
        let r = self.protocol_call(self.names.known.source, "member", src, &[m.ty], &[h, prev])?;
        let x = self.try_value(r, ret)?;
        let rec = self.b.refs_record(&[x]);
        let some = self.b.emit(Tag::NewVariant, 1, rec, ot, SYN);
        self.b.set(slots[0], some, SYN);
        let then = self.b.close_block(tb, None, Ty::VOID, SYN);
        let eb = self.b.open_block();
        self.read_key(v, later, &slots[1..], pos, src, ret)?;
        let other = self.b.close_block(eb, None, Ty::VOID, SYN);
        self.switch_int(pos, m.position, then, other, Ty::VOID);
        Ok(())
    }

    /// `build(s)` (`annot.build.variant`): the source chooses among every
    /// variant, then the chosen one is built.
    fn build_body(mut self, sig: &FnSig, vars: &[Var]) -> StageResult<(Body, Vec<Handle>)> {
        let ps = self.params(sig);
        let [src] = ps[..] else {
            return unsupported("a `build` signature");
        };
        let names = self.names;
        let s = self.target;
        let ret = sig.ret;
        let blk = self.b.open_block();
        let mut hs = Vec::new();
        for v in vars {
            hs.push(self.variant_handle(v));
        }
        let vt = self.adt(self.std("Variant"), &[s]);
        let lt = self.fresh(self.adt(names.known.list, &[vt]));
        let rec = self.b.refs_record(&hs);
        let choices = self.b.emit(Tag::NewList, NONE, rec, lt, SYN);
        let r = self.protocol_call(names.known.source, "variant", src, &[], &[choices])?;
        let chosen = self.try_value(r, ret)?;
        let value = match vars {
            [v] if v.of_data => self.build_variant(v, src, ret)?,
            _ => {
                let vi = self.adt(self.std("VariantInfo"), &[]);
                let info = self.std_field(chosen, self.std("Variant"), "info", vi)?;
                let index = self.std_field(info, self.std("VariantInfo"), "index", Ty::I32)?;
                self.build_choice(vars, index, src, ret)?
            }
        };
        let rec = self.b.refs_record(&[value]);
        let ok = self.b.emit(Tag::NewVariant, 0, rec, ret, SYN);
        let root = self.b.close_block(blk, Some(ok), ret, SYN);
        self.finish(root)
    }

    /// The chosen variant, by its index, built.
    fn build_choice(
        &mut self,
        vars: &[Var],
        index: Ref,
        src: (LocalId, Ty),
        ret: Ty,
    ) -> StageResult<Ref> {
        let s = self.target;
        let Some((v, later)) = vars.split_first() else {
            return Ok(self.panic(
                "structure-variant-mismatch",
                "the source chose no variant of the type",
            ));
        };
        let tb = self.b.open_block();
        let x = self.build_variant(v, src, ret)?;
        let then = self.b.close_block(tb, Some(x), s, SYN);
        let eb = self.b.open_block();
        let y = self.build_choice(later, index, src, ret)?;
        let tail = (self.b.ty_of(y) != Ty::NEVER).then_some(y);
        let other = self
            .b
            .close_block(eb, tail, if tail.is_some() { s } else { Ty::NEVER }, SYN);
        Ok(self.switch_int(index, v.index, then, other, s))
    }

    /// Runs `each` for the variant that `x` holds: directly for a data
    /// type, else in a chain of tag tests whose last arm is unreachable.
    fn dispatch(
        &mut self,
        vars: &[Var],
        x: Ref,
        each: &mut dyn FnMut(&mut Self, &Var) -> StageResult<()>,
    ) -> StageResult<()> {
        match vars {
            [v] if v.of_data => each(self, v),
            [] => {
                self.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, SYN);
                Ok(())
            }
            [v, rest @ ..] => {
                let tb = self.b.open_block();
                each(self, v)?;
                let then = self.b.close_block(tb, None, Ty::VOID, SYN);
                let eb = self.b.open_block();
                if rest.is_empty() {
                    self.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, SYN);
                } else {
                    self.dispatch(rest, x, each)?;
                }
                let other = self.b.close_block(eb, None, Ty::VOID, SYN);
                self.switch_tag(x, v.index, then, other, Ty::VOID);
                Ok(())
            }
        }
    }
}
