#![forbid(unsafe_code)]
//! `hd_structure`: the compiler-supplied `Structure` generator
//! (annotations, "The Structure Trait", "Walk, Describe, And Build",
//! "Members And Variants", "Tuple Structure", "Handles"; codegen.md §12.3
//! "Derives", §13.6). One generator writes the TIR bodies of `facts`,
//! `name`, `walk`, `describe` and `build` over a [`Target`]: a type and its
//! member list. It reads declarations only, through [`Cx`], and keeps no
//! checker state, so both of its callers use it:
//!
//! - **A derivation, at check time** ([`derivation`]): the checker builds
//!   the target from the data type or enum and the derivation's omitted
//!   members (`hd_check::structure`). Each derivation gets its own
//!   `Structure` (`annot.structure.per-derivation`): the bodies are hidden
//!   methods of the derived implementation `D` (`D`'s `walk` at the hidden
//!   path `walk`). The derive instance's `Structure` calls choose `D`
//!   itself (`Impl` choice), so collection calls these bodies directly.
//!   Each handle is one more hidden method of `D` with no parameter
//!   (`handle V`, `handle V M`), which the traversals call: its closures
//!   are instantiated once per derivation, not once per walker.
//! - **A tuple type, at codegen** ([`supplied`]): the trait solver's
//!   `Structure` row answers a concrete tuple with `Builtin(Structure)`,
//!   and collection asks for one method's body at that tuple type
//!   ([`tuple_target`], `annot.tuple.*`). The body is the trait method's
//!   instance at the tuple, keyed by the tuple type like any instance. A
//!   tuple has no implementation to hold hidden methods, so its handles are
//!   built where the traversal passes them.
//!
//! The handles and information values are values of `std.structure`'s
//! declarations (`lib/std/structure.hd`), built field by field; their
//! hidden fields (`hd_get`, `hd_holds`, `hd_default`) are closures over the
//! target. The witness field, which no std body reads, holds the member's
//! information as a `dyn Inspectable`. Doc comments are not carried: `doc`
//! is `.None`.

mod dbg;

pub use dbg::{show_body as dbg_show_body, values_body as dbg_values_body};

use hd_base::{DefId, LocalId, NodeIdx, NotImplemented, Stage, StageResult};
use hd_intern::PathKind;
use hd_resolve::{FnSig, Generic, Item, ItemData, Lookup, Names};
use hd_tir::Body;
use hd_tir::ir::{
    BodyKind, Callee, ChoiceKind, Coercion, NONE, Providers, Ref, Tag, TirBuilder, TirSink,
    local_flags,
};
use hd_types::{InternPool, ParamRef, Ty, TyData, TyList};

/// Where the generator reads declarations: `std.structure`'s data types
/// and protocol traits, `Structure` itself, `std.ops.panic_with`.
pub trait Items {
    fn item(&self, def: DefId) -> Option<&Item>;
}

impl Items for Lookup<'_> {
    fn item(&self, def: DefId) -> Option<&Item> {
        Lookup::item(self, def)
    }
}

/// The generator's context: the run's names and the declarations it sees.
/// `stage` labels what the generator cannot carry yet.
pub struct Cx<'a, 'n> {
    pub names: &'a Names<'n>,
    pub items: &'a dyn Items,
    pub stage: Stage,
}

impl Cx<'_, '_> {
    fn unsupported<T>(&self, what: impl Into<String>) -> StageResult<T> {
        Err(NotImplemented::new(self.stage, what))
    }
}

/// How a variant's member is read and the variant built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A data type's one variant: fields.
    Data,
    /// An enum's variant: payloads under a tag.
    Enum,
    /// A tuple's one variant (`annot.tuple.variant`): elements.
    Tuple,
}

/// One member of a variant, as a handle describes it.
#[derive(Clone, Debug)]
pub struct Mem {
    /// `name`, or `_0`, `_1`... for an unnamed payload parameter or a
    /// tuple element.
    pub name: String,
    /// The declared name, which names its default body.
    pub declared: String,
    pub position: u32,
    /// Its index among the variant's fields, payloads or elements.
    pub field: u32,
    /// The declared type, over the target's parameters.
    pub ty: Ty,
    pub embedded: bool,
    pub positional: bool,
    pub has_default: bool,
    /// Omitted by the derivation's member lines (`annot.omit.skip`).
    pub omitted: bool,
    /// A tuple's rest member (`annot.tuple.rest`): `walk` passes it to
    /// `rest` (`annot.walk.rest`).
    pub rest: bool,
    /// `SelfRef`'s variant index: 0 `Absent`, 1 `Optional`, 2 `Required`.
    pub self_ref: u32,
}

/// One variant; a data type or a tuple is one variant
/// (`annot.variant.data`, `annot.tuple.variant`).
#[derive(Clone, Debug)]
pub struct Var {
    pub name: String,
    pub index: u32,
    /// The variant's item, whose hidden members are its payload defaults;
    /// the data type for a data type, `DefId::NONE` for a tuple.
    pub def: DefId,
    pub shape: Shape,
    pub members: Vec<Mem>,
    pub self_ref: u32,
}

impl Var {
    fn of_data(&self) -> bool {
        self.shape != Shape::Enum
    }
}

/// What the generator traverses: a type, its name (`T::name()`) and its
/// variants with their members.
#[derive(Clone, Debug)]
pub struct Target {
    pub ty: Ty,
    pub name: String,
    pub vars: Vec<Var>,
}

/// The `Structure` target of a tuple type (`annot.tuple.*`): one variant
/// named `""` with index 0 and `of_data` true; its members are the
/// elements `_0`, `_1`..., positional, not embedded, with no facts, no doc
/// comment, no default and `self_ref` `.Absent`; a rest element
/// `List[T]...` is the last member, of type `List[T]`. `None` for a type
/// that is not a tuple.
#[must_use]
pub fn tuple_target(pool: &InternPool, ty: Ty) -> Option<Target> {
    let TyData::Tuple { elems, rest } = pool.get(ty) else {
        return None;
    };
    let elems = pool.list_items(elems);
    let member = |k: usize, t: Ty, rest: bool| {
        let position = u32::try_from(k).unwrap_or(u32::MAX);
        Mem {
            name: format!("_{k}"),
            declared: k.to_string(),
            position,
            field: position,
            ty: t,
            embedded: false,
            positional: true,
            has_default: false,
            omitted: false,
            rest,
            self_ref: 0,
        }
    };
    let mut members: Vec<Mem> = elems
        .iter()
        .enumerate()
        .map(|(k, t)| member(k, *t, false))
        .collect();
    if let Some(r) = rest {
        members.push(member(elems.len(), r, true));
    }
    Some(Target {
        ty,
        name: String::new(),
        vars: vec![Var {
            name: String::new(),
            index: 0,
            def: DefId::NONE,
            shape: Shape::Tuple,
            members,
            self_ref: 0,
        }],
    })
}

/// What a derivation adds to its module: hidden methods of the derived
/// implementation and their bodies.
#[derive(Default)]
pub struct Generated {
    pub items: Vec<Item>,
    pub bodies: Vec<Body>,
}

/// The `Structure` methods of `Structure` itself, by name.
fn structure_methods(cx: &Cx<'_, '_>) -> StageResult<Vec<(String, DefId, FnSig)>> {
    let names = cx.names;
    let st = names.known.structure;
    let Some(ItemData::Trait(t)) = cx.items.item(st).map(|i| &i.data) else {
        return cx.unsupported("a `Structure` without `std.structure`");
    };
    let mut out = Vec::new();
    for &(sym, sdef) in &t.methods {
        if let Some(sig) = cx.items.item(sdef).and_then(Item::sig) {
            out.push((names.text(sym).to_owned(), sdef, sig.clone()));
        }
    }
    Ok(out)
}

/// The body of `Structure`'s method `name` for `target`, with signature
/// `sig`; the handles it calls through methods.
fn method_body(
    g: Gen<'_, '_>,
    target: &Target,
    name: &str,
    sig: &FnSig,
) -> StageResult<Option<(Body, Vec<Handle>)>> {
    Ok(Some(match name {
        "facts" => g.facts_body(sig)?,
        "name" => g.const_body(sig, &target.name)?,
        "walk" => g.walk_body(sig, &target.vars)?,
        "describe" => g.describe_body(sig, &target.vars)?,
        "build" => g.build_body(sig, &target.vars)?,
        _ => return Ok(None),
    }))
}

/// The `Structure` bodies of a derivation `impl_` over `target`
/// (codegen.md §12.3): hidden methods of `impl_`, one per `Structure`
/// method and one per handle the traversals pass.
pub fn derivation(cx: &Cx<'_, '_>, target: &Target, impl_: DefId) -> StageResult<Generated> {
    let names = cx.names;
    let st = names.known.structure;
    let mut out = Generated::default();
    let mut handles: Vec<Handle> = Vec::new();
    for (name, sdef, trait_sig) in structure_methods(cx)? {
        let def = names.member(impl_, PathKind::Hidden, &name);
        let sig = subst_sig(names, &trait_sig, &|p: ParamRef| {
            if p.owner == st && p.index == 0 {
                Some(target.ty)
            } else if p.owner == sdef {
                Some(param(names, def, p.index))
            } else {
                None
            }
        });
        let g = Gen::new(cx, target.ty, def, Handles::Methods(impl_));
        let Some((body, wanted)) = method_body(g, target, &name, &sig)? else {
            continue;
        };
        out.items.push(Item::new(
            def,
            names.syms.intern(&name),
            false,
            ItemData::Method {
                owner: impl_,
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
        let mut g = Gen::new(cx, target.ty, h.def, Handles::Methods(impl_));
        let blk = g.b.open_block();
        let v = &target.vars[h.var];
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
                owner: impl_,
                sig: FnSig::simple(Vec::new(), Vec::new(), h.ty),
                has_body: true,
            },
        ));
        out.bodies.push(body);
    }
    Ok(out)
}

/// The body of `Structure`'s method `method` at `target`, a type the
/// compiler supplies `Structure` for (a tuple, codegen.md §13.6). The body
/// belongs to the trait method itself: its signature is the trait's with
/// `Self` replaced by the target, and the method's own parameters stay the
/// trait method's, so an instance's arguments are `[target, own...]`.
/// Handles are built where the traversal passes them. `None` for a method
/// the generator does not write.
pub fn supplied(cx: &Cx<'_, '_>, target: &Target, method: DefId) -> StageResult<Option<Body>> {
    let names = cx.names;
    let st = names.known.structure;
    let Some((name, _, trait_sig)) = structure_methods(cx)?.into_iter().find(|m| m.1 == method)
    else {
        return Ok(None);
    };
    let sig = subst_sig(names, &trait_sig, &|p: ParamRef| {
        (p.owner == st && p.index == 0).then_some(target.ty)
    });
    let g = Gen::new(cx, target.ty, method, Handles::Inline);
    Ok(method_body(g, target, &name, &sig)?.map(|(body, _)| body))
}

/// A type parameter of `owner`.
fn param(names: &Names<'_>, owner: DefId, index: u16) -> Ty {
    names
        .pool
        .intern_ty(&TyData::Param(ParamRef { owner, index }))
}

/// A signature with `f` applied to its types and bounds.
pub fn subst_sig(names: &Names<'_>, sig: &FnSig, f: &dyn Fn(ParamRef) -> Option<Ty>) -> FnSig {
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

/// `t` without an outer `mut`.
#[must_use]
pub fn strip_mut(names: &Names<'_>, t: Ty) -> Ty {
    match names.pool.get(t) {
        TyData::Mut(x) => x,
        _ => t,
    }
}

/// Where a traversal's handles come from.
#[derive(Clone, Copy)]
enum Handles {
    /// Calls of the derivation's hidden handle methods, whose bodies the
    /// derivation adds once.
    Methods(DefId),
    /// Built in place: the target has no implementation to hold them.
    Inline,
}

/// One generated body under construction.
struct Gen<'a, 'n> {
    cx: &'a Cx<'a, 'n>,
    names: &'a Names<'n>,
    b: TirBuilder,
    /// The target, over its parameters.
    target: Ty,
    handles: Handles,
    /// The handle methods this body calls, built by their own bodies.
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
    fn new(cx: &'a Cx<'a, 'n>, target: Ty, def: DefId, handles: Handles) -> Self {
        Gen {
            cx,
            names: cx.names,
            b: TirBuilder::new(def, BodyKind::DeriveInstance),
            target,
            handles,
            wanted: Vec::new(),
        }
    }

    /// A call of the derivation's handle method for `h`.
    fn handle_call(&mut self, impl_: DefId, h: Handle) -> Ref {
        let pool = self.names.pool;
        let n = self.cx.items.item(impl_).map_or(0, |i| i.generics.len());
        let own: Vec<Ty> = (0..n)
            .map(|i| param(self.names, impl_, u16::try_from(i).unwrap_or(u16::MAX)))
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

    /// The handle `Variant[S]` of `v`.
    fn variant_handle(&mut self, v: &Var) -> StageResult<Ref> {
        let Handles::Methods(impl_) = self.handles else {
            return self.new_variant_handle(v);
        };
        // A variant's index is its place in the variant list.
        let var = v.index as usize;
        let ty = self.adt(self.std("Variant"), &[self.target]);
        let def = self
            .names
            .member(impl_, PathKind::Hidden, &format!("handle {}", v.index));
        Ok(self.handle_call(
            impl_,
            Handle {
                var,
                mem: None,
                f: Ty::VOID,
                def,
                ty,
            },
        ))
    }

    /// The handle `Field[S, F]` of member `m` of `v`; a declared-type
    /// handle of a `mut` member is its own.
    fn field_handle(&mut self, v: &Var, m: &Mem, f: Ty) -> StageResult<Ref> {
        let Handles::Methods(impl_) = self.handles else {
            return self.new_field_handle(v, m, f);
        };
        // A member's position is its place in the variant's members.
        let (var, mem) = (v.index as usize, m.position as usize);
        let ty = self.adt(self.names.known.field, &[self.target, f]);
        let declared = if f == strip_mut(self.names, m.ty) {
            ""
        } else {
            " declared"
        };
        let def = self.names.member(
            impl_,
            PathKind::Hidden,
            &format!("handle {} {}{declared}", v.index, m.position),
        );
        Ok(self.handle_call(
            impl_,
            Handle {
                var,
                mem: Some(mem),
                f,
                def,
                ty,
            },
        ))
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
            vararg: false,
        })
    }

    /// The fields of a `std.structure` data type.
    fn std_fields(&self, def: DefId) -> StageResult<&'a [hd_resolve::Field]> {
        match self.cx.items.item(def).map(|i| &i.data) {
            Some(ItemData::Data(fs)) => Ok(fs),
            _ => self
                .cx
                .unsupported("a `std.structure` data type outside the module's view"),
        }
    }

    /// A field's declared type in a `std.structure` data type, its
    /// parameters replaced by `args`.
    fn field_ty(&self, def: DefId, field: &str, args: &[Ty]) -> StageResult<Ty> {
        let fs = self.std_fields(def)?;
        let Some(f) = fs.iter().find(|f| self.names.text(f.name) == field) else {
            return self
                .cx
                .unsupported(format!("the `std.structure` field `{field}`"));
        };
        Ok(self.names.pool.subst(f.ty, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        }))
    }

    /// A data value of `def[args]` from its fields' values by name, in
    /// declaration order.
    fn new_data(&mut self, def: DefId, args: &[Ty], vals: &[(&str, Ref)]) -> StageResult<Ref> {
        let fs = self.std_fields(def)?;
        let mut refs = Vec::new();
        for f in fs {
            let n = self.names.text(f.name);
            let Some((_, v)) = vals.iter().find(|(k, _)| *k == n) else {
                return self
                    .cx
                    .unsupported(format!("the `std.structure` field `{n}`"));
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
    /// a field of a data value, an element of a tuple, or a payload of an
    /// enum value that holds `v`.
    fn read(&mut self, s: Ref, v: &Var, m: &Mem, t: Ty) -> Ref {
        match v.shape {
            Shape::Data => self.b.emit(Tag::Field, s.0, m.field, t, SYN),
            Shape::Tuple => self.b.emit(Tag::TupleGet, s.0, m.field, t, SYN),
            Shape::Enum => {
                let rec = self.b.refs_record(&[Ref(v.index), Ref(m.field)]);
                self.b.emit(Tag::Payload, s.0, rec, t, SYN)
            }
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
        let of_data = self.bool_(v.of_data());
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
        let of_data = v.of_data();
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
            if v.of_data() {
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

    /// A call of a walker, describer or source method through the body's
    /// own parameter `w`: its result.
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
        let Some(ItemData::Trait(t)) = self.cx.items.item(trait_).map(|i| &i.data) else {
            return self
                .cx
                .unsupported("a structure protocol trait outside the module's view");
        };
        let Some(&(_, mdef)) = t.methods.iter().find(|(n, _)| names.text(*n) == method) else {
            return self
                .cx
                .unsupported(format!("the protocol method `{method}`"));
        };
        let Some(sig) = self.cx.items.item(mdef).and_then(Item::sig) else {
            return self
                .cx
                .unsupported(format!("the protocol method `{method}`"));
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
        let ret = hd_types::with_assoc_args(pool.types(), ret, trait_, pool.list(&[s]));
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

    /// The error type of a `Result` value `r`.
    fn result_args(&self, r: Ref) -> StageResult<(Ty, Ty)> {
        let pool = self.names.pool;
        let TyData::Adt { args, .. } = pool.get(strip_mut(self.names, self.b.ty_of(r))) else {
            return self
                .cx
                .unsupported("a protocol method whose result is not a `Result`");
        };
        let items = pool.list_items(args);
        Ok((
            items.first().copied().unwrap_or(Ty::POISON),
            items.get(1).copied().unwrap_or(Ty::POISON),
        ))
    }

    /// `r?` for a `Result[void, E]` in a body whose result type is `ret`.
    fn try_void(&mut self, r: Ref, ret: Ty) -> StageResult<()> {
        let (_, err) = self.result_args(r)?;
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
        let stage = self.cx.stage;
        let body = self.b.finish(root, &[]).map_err(|e| {
            NotImplemented::new(stage, format!("generated structure TIR verifier: {e:?}"))
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
    /// `annot.structure.name`, `annot.tuple.name`).
    fn const_body(mut self, sig: &FnSig, text: &str) -> StageResult<(Body, Vec<Handle>)> {
        let blk = self.b.open_block();
        let v = self.str_(text);
        let root = self.b.close_block(blk, Some(v), sig.ret, SYN);
        self.finish(root)
    }

    /// The traversal of one variant: `variant`, then each member it does
    /// not omit (`annot.walk.match`, `annot.walk.members`,
    /// `annot.describe.order`); `walk` passes a rest member to `rest`
    /// (`annot.walk.rest`). `value` is the walked value, or `None` for
    /// `describe`.
    fn visit(
        &mut self,
        trait_: DefId,
        w: (LocalId, Ty),
        v: &Var,
        value: Option<Ref>,
        ret: Ty,
    ) -> StageResult<()> {
        let h = self.variant_handle(v)?;
        let r = self.protocol_call(trait_, "variant", w, &[], &[h])?;
        self.try_void(r, ret)?;
        for m in v.members.iter().filter(|m| !m.omitted) {
            let f = strip_mut(self.names, m.ty);
            let h = self.field_handle(v, m, f)?;
            let mut args = vec![h];
            let (method, targ) = match value {
                Some(s) => {
                    args.push(self.read(s, v, m, f));
                    if m.rest {
                        ("rest", self.list_item(f)?)
                    } else {
                        ("member", f)
                    }
                }
                None => ("member", f),
            };
            let r = self.protocol_call(trait_, method, w, &[targ], &args)?;
            self.try_void(r, ret)?;
        }
        Ok(())
    }

    /// The item type `T` of a rest member's `List[T]`.
    fn list_item(&self, t: Ty) -> StageResult<Ty> {
        let pool = self.names.pool;
        match pool.get(t) {
            TyData::Adt { def, args } if def == self.names.known.list => {
                Ok(pool.list_items(args).first().copied().unwrap_or(Ty::POISON))
            }
            _ => self.cx.unsupported("a rest element that is not a `List`"),
        }
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
            return self.cx.unsupported("a `walk` signature");
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
            return self.cx.unsupported("a `describe` signature");
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
        let (ok_t, err) = self.result_args(r)?;
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
        let fs = self.std_fields(def)?;
        let Some(k) = fs.iter().position(|f| self.names.text(f.name) == name) else {
            return self
                .cx
                .unsupported(format!("the `std.structure` field `{name}`"));
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
            return self
                .cx
                .unsupported("a member default of a target that is not a data type or enum");
        };
        let a = self.b.refs_record(&[Ref(body.raw()), Ref(args.0)]);
        let bw = self.b.refs_record(&[]);
        Ok(self.b.emit(Tag::DefaultCall, a, bw, t, SYN))
    }

    /// Builds variant `v` from the source `src` (`annot.build.next` to
    /// `annot.build.construct`, `annot.tuple.build`): `next` until the end
    /// key, `member` per key with the value read so far, `missing` per
    /// member never named, then the value from one local per member.
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
            let h = self.field_handle(v, m, m.ty)?;
            let r = self.protocol_call(source, "missing", src, &[m.ty], &[h])?;
            let x = self.try_value(r, ret)?;
            let none = self.b.close_block(nb, Some(x), m.ty, SYN);
            values.push(self.switch_tag(cur, 1, some, none, m.ty));
        }
        let rec = self.b.refs_record(&values);
        Ok(match v.shape {
            Shape::Data => {
                let t = self.fresh(s);
                self.b.emit(Tag::NewData, NONE, rec, t, SYN)
            }
            Shape::Tuple => self.b.emit(Tag::NewTuple, NONE, rec, s, SYN),
            Shape::Enum => self.b.emit(Tag::NewVariant, v.index, rec, s, SYN),
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
        let h = self.field_handle(v, m, m.ty)?;
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
            return self.cx.unsupported("a `build` signature");
        };
        let names = self.names;
        let s = self.target;
        let ret = sig.ret;
        let blk = self.b.open_block();
        let mut hs = Vec::new();
        for v in vars {
            hs.push(self.variant_handle(v)?);
        }
        let vt = self.adt(self.std("Variant"), &[s]);
        let lt = self.fresh(self.adt(names.known.list, &[vt]));
        let rec = self.b.refs_record(&hs);
        let choices = self.b.emit(Tag::NewList, NONE, rec, lt, SYN);
        let r = self.protocol_call(names.known.source, "variant", src, &[], &[choices])?;
        let chosen = self.try_value(r, ret)?;
        let value = match vars {
            [v] if v.of_data() => self.build_variant(v, src, ret)?,
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
    /// type or a tuple, else in a chain of tag tests whose last arm is
    /// unreachable.
    fn dispatch(
        &mut self,
        vars: &[Var],
        x: Ref,
        each: &mut dyn FnMut(&mut Self, &Var) -> StageResult<()>,
    ) -> StageResult<()> {
        match vars {
            [v] if v.of_data() => each(self, v),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `annot.tuple.members`, `.rest`, `.variant`, `.name`.
    #[test]
    fn a_tuple_target_lists_its_elements_then_its_rest_member() {
        let pool = InternPool::new();
        let list = pool.intern_ty(&TyData::Adt {
            def: DefId::from_raw(7),
            args: pool.list(&[Ty::I32]),
        });
        let t = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[Ty::I32, Ty::BOOL]),
            rest: Some(list),
        });
        let target = tuple_target(&pool, t).expect("a tuple");
        assert_eq!(target.name, "");
        let [v] = &target.vars[..] else {
            panic!("one variant");
        };
        assert_eq!((v.name.as_str(), v.index, v.shape), ("", 0, Shape::Tuple));
        assert!(v.of_data());
        let shown: Vec<_> = v
            .members
            .iter()
            .map(|m| {
                (
                    m.name.as_str(),
                    m.position,
                    m.ty,
                    m.rest,
                    m.positional,
                    m.embedded,
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("_0", 0, Ty::I32, false, true, false),
                ("_1", 1, Ty::BOOL, false, true, false),
                ("_2", 2, list, true, true, false),
            ]
        );
        assert!(v.members.iter().all(|m| !m.has_default && m.self_ref == 0));
        assert!(tuple_target(&pool, Ty::I32).is_none());
    }
}
