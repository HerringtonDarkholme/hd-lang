//! Struct values (spec 08 "Struct Declarations", spec 04 "Value Types").
//! A struct value has no identity, so a field write changes only the
//! writable place it names: a `let mut` local or top-level binding, a
//! field of a data value with mutable access, what a `&mut` refers to, a
//! list element, a map value (`types.coll.map-value.place`), or a struct
//! field of one of these (`types.value.writable-places`).
//!
//! The written place comes from the syntax ([`Ck::place_of`]), not from
//! the instructions a read of it emitted: `a.b.x = v` is one
//! `Store [$a .b .x] v` (`docs/compiler/tir-v2.md`, "Field writes"), so
//! the other fields keep their values and no struct on the path is
//! rebuilt. A field through a reference is `Store [*%r .b .x] v`, a field
//! of a data value `Store [*%o .b .x] v`, a top-level binding's
//! `Store [@g .b .x] v`, a list element's `Store [%xs [%i] .b .x] v`, a
//! map value's `Store [%m [%e] .b .x] v` at the number `%e` of the key's
//! entry (`MapEntry`).

use hd_base::{DefId, Fuel, HashMap, HashSet, LocalId, StageResult};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Place as TirPlace, PlaceRoot, Proj, Ref, Tag, TirSink, local_flags};
use hd_types::{ParamRef, Ty, TyData};

use crate::body::{Ck, ExpectPosition};
use crate::refs::{Place, Root};

/// Concrete containment under one immutable impl view. Edges normalize
/// projections *after* substituting the containing instance's arguments
/// (`trait.assoc.projection`, `data.enum.no-recursion`). Declaration cycles
/// without a projection retain the existing field diagnostics and fix-its.
/// Each reachable instance and edge is expanded once; no type-pool scan.
/// Why an instance has no size: it holds itself through a projection's
/// payload, or one of its projections has no normal form within
/// `hd_base::limits::PROJECTION_DEPTH` bindings
/// (`trait.assoc.projection.depth`), the chain starting at the type kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unsized {
    Cycle,
    TooDeep(Ty),
}

/// `trait-resolution-depth` on a projection whose bindings chain past
/// the limit (`trait.assoc.projection.depth`); `shown` is the projection
/// the chain starts at.
pub(crate) fn projection_too_deep(shown: &str) -> String {
    format!(
        "normalizing `{shown}` needs more than {} nested associated-type bindings",
        hd_base::limits::PROJECTION_DEPTH
    )
}

/// The containment walks of one body or header stage. A body's walks
/// share `done`; with `shared`, closed types a clean walk
/// left are also published to the run's memo for the body's impl
/// universe, and a walk stops at a published type. So a wide type is
/// walked once per run and universe, not once per body that holds it:
/// O(distinct member types) per run plus one probe per member per body.
#[derive(Default)]
pub(crate) struct ProjectionCycles<'a> {
    done: HashMap<Ty, Option<Ty>>,
    reported: HashSet<Ty>,
    checked: HashSet<Ty>,
    normalized: HashMap<Ty, Ty>,
    /// The run's memo and the impl universe of a view with no hidden
    /// local impl: then a closed type's answer is the same in every body.
    shared: Option<(
        &'a hd_types::solver::GlobalMemo,
        hd_types::solver::ImplUniverseId,
    )>,
    /// A projection past the depth limit was met: the body's `done`
    /// holds answers that stopped there, so nothing more is published.
    tainted: bool,
}

impl<'a> ProjectionCycles<'a> {
    /// Walks that publish their clean closed types to `memo` for
    /// `universe`, and stop at the ones published.
    pub(crate) fn shared(
        memo: &'a hd_types::solver::GlobalMemo,
        universe: hd_types::solver::ImplUniverseId,
    ) -> Self {
        ProjectionCycles {
            shared: Some((memo, universe)),
            ..ProjectionCycles::default()
        }
    }

    /// An instance of a declaration that holds itself inline
    /// ([`holds_itself`]) is not entered: the value rule reports the
    /// declaration, and the instance's fields may grow without end
    /// (`struct G[T]: x: G[(T, T)]` holds `G[(i32, i32)]` in `G[i32]`).
    pub(crate) fn check(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        impls: hd_types::solver::Impls<'_>,
        global: &hd_types::solver::GlobalMemo,
        ty: Ty,
        fuel: &mut Fuel,
    ) -> Result<Option<Unsized>, ()> {
        // A rigid projection can acquire a concrete payload at a later
        // instantiation. Do not remember that open instance as acyclic.
        if pool.has_param(ty) || pool.has_infer(ty) {
            return Ok(None);
        }
        // A projection past the depth limit stays as it is in the search,
        // which then ends as for any other open projection; the first is
        // reported, once per head.
        let mut too_deep = None;
        let shared = self.shared.filter(|_| !self.tainted);
        let mut left = Vec::new();
        let cycle = Self::search(
            &mut self.done,
            ty,
            fuel,
            &mut |t| shared.is_some_and(|(memo, u)| !t.is_local() && memo.contained(u, t)),
            &mut left,
            &mut |t| {
                if let Some(&n) = self.normalized.get(&t) {
                    return n;
                }
                let n = match hd_types::solver::normalize_concrete(pool, impls, t) {
                    Ok(n) => n,
                    Err(e) => {
                        too_deep.get_or_insert(e.head);
                        t
                    }
                };
                self.normalized.insert(t, n);
                n
            },
            &mut |t| {
                let mut children = Vec::new();
                match pool.get(t) {
                    TyData::Adt { def, .. } if holds_itself(global, pool, lookup, def) => {}
                    TyData::Adt { def, args } => {
                        let argv = pool.list_items(args);
                        let sub = |f| {
                            pool.subst(f, &|p: ParamRef| {
                                (p.owner == def)
                                    .then(|| argv.get(usize::from(p.index)).copied())
                                    .flatten()
                            })
                        };
                        if let Some(item) = lookup.item(def) {
                            match &item.data {
                                hd_resolve::ItemData::Struct(fs, _) => {
                                    children.extend(fs.iter().map(|f| sub(f.ty)));
                                }
                                hd_resolve::ItemData::Enum {
                                    shared, variants, ..
                                } => {
                                    children.extend(
                                        shared
                                            .iter()
                                            .chain(variants.iter().flat_map(|v| &v.fields))
                                            .map(|f| sub(f.ty)),
                                    );
                                }
                                hd_resolve::ItemData::Newtype(base) => children.push(sub(*base)),
                                hd_resolve::ItemData::Fn(..)
                                | hd_resolve::ItemData::Trait(..)
                                | hd_resolve::ItemData::Impl { .. }
                                | hd_resolve::ItemData::Method { .. }
                                | hd_resolve::ItemData::AssocType { .. }
                                | hd_resolve::ItemData::Alias { .. }
                                | hd_resolve::ItemData::Builtin => {}
                            }
                        }
                    }
                    TyData::Tuple { elems, .. } => children.extend(pool.list_items(elems)),
                    TyData::Union(members) => children.extend(pool.list_items(members)),
                    // References, functions and trait values end containment;
                    // unresolved projections and parameters have no known fields.
                    TyData::Prim(_)
                    | TyData::Never
                    | TyData::Poison
                    | TyData::Fn { .. }
                    | TyData::TraitValue { .. }
                    | TyData::Opaque { .. }
                    | TyData::Param(_)
                    | TyData::Const(_)
                    | TyData::Assoc { .. }
                    | TyData::Ref { .. }
                    | TyData::Infer(_)
                    | TyData::Canon(_)
                    | TyData::Row(_)
                    | TyData::Context(_) => {}
                }
                crate::work::scan_work(1 + children.len());
                children
            },
        )?;
        if too_deep.is_some() {
            self.tainted = true;
        } else if cycle.is_none()
            && let Some((memo, u)) = shared
        {
            left.retain(|t| !t.is_local());
            memo.publish_contained(u, &left);
        }
        if let Some(head) = too_deep.filter(|&h| self.reported.insert(h)) {
            return Ok(Some(Unsized::TooDeep(head)));
        }
        Ok(cycle
            .is_some_and(|t| self.reported.insert(t))
            .then_some(Unsized::Cycle))
    }

    /// `known` answers a type published clean, where the walk stops;
    /// `left` collects each type the walk leaves with no cycle found.
    fn search(
        done: &mut HashMap<Ty, Option<Ty>>,
        root: Ty,
        fuel: &mut Fuel,
        known: &mut impl FnMut(Ty) -> bool,
        left: &mut Vec<Ty>,
        normalize: &mut impl FnMut(Ty) -> Ty,
        expand: &mut impl FnMut(Ty) -> Vec<Ty>,
    ) -> Result<Option<Ty>, ()> {
        // Enter/leave events keep both width and depth off the Rust stack.
        let mut todo = vec![(root, None)];
        let mut path: Vec<(Ty, bool)> = Vec::new();
        let mut active = HashMap::default();
        while let Some((raw, leave)) = todo.pop() {
            if let Some(t) = leave {
                active.remove(&t);
                path.pop();
                done.insert(raw, None);
                done.insert(t, None);
                left.extend([raw, t]);
                continue;
            }
            if let Some(&cycle) = done.get(&raw) {
                if cycle.is_some() {
                    for &(t, _) in &path {
                        done.insert(t, cycle);
                    }
                    return Ok(cycle);
                }
                continue;
            }
            if known(raw) {
                done.insert(raw, None);
                continue;
            }
            if !fuel.charge(1) {
                return Err(());
            }
            let t = normalize(raw);
            let projected = t != raw;
            if let Some(&at) = active.get(&t) {
                if projected || path[at + 1..].iter().any(|&(_, p)| p) {
                    for &(p, _) in &path {
                        done.insert(p, Some(t));
                    }
                    done.insert(raw, Some(t));
                    return Ok(Some(t));
                }
                continue;
            }
            if let Some(&cycle) = done.get(&t) {
                done.insert(raw, cycle);
                if cycle.is_some() {
                    for &(t, _) in &path {
                        done.insert(t, cycle);
                    }
                    return Ok(cycle);
                }
                continue;
            }
            active.insert(t, path.len());
            path.push((t, projected));
            todo.push((raw, Some(t)));
            todo.extend(expand(t).into_iter().rev().map(|c| (c, None)));
        }
        Ok(None)
    }

    /// Marks `t` and its parts as checked: the header's walk reported
    /// what they hold.
    pub(crate) fn checked_elsewhere(&mut self, t: Ty) {
        self.checked.insert(t);
    }

    /// Written type components, separate from containment edges: a reference
    /// ends containment but its referent must still be a well-formed type.
    /// Used after body inference settles, outside speculative trials.
    pub(crate) fn check_nested(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        impls: hd_types::solver::Impls<'_>,
        global: &hd_types::solver::GlobalMemo,
        ty: Ty,
        fuel: &mut Fuel,
    ) -> Result<Option<Unsized>, ()> {
        let mut todo = vec![ty];
        let mut bad = None;
        while let Some(t) = todo.pop() {
            if !self.checked.insert(t) {
                continue;
            }
            let found = self.check(pool, lookup, impls, global, t, fuel)?;
            bad = bad.or(found);
            match pool.get(t) {
                TyData::Adt { args, .. } => todo.extend(pool.list_items(args)),
                TyData::Union(members) => todo.extend(pool.list_items(members)),
                TyData::Ref { inner, .. } => todo.push(inner),
                TyData::Tuple { elems, rest } => {
                    todo.extend(pool.list_items(elems));
                    todo.extend(rest);
                }
                TyData::Fn { params, result, .. } => {
                    todo.extend(pool.list_items(params));
                    todo.push(result);
                }
                TyData::TraitValue { args, bindings, .. } => {
                    todo.extend(pool.list_items(args));
                    todo.extend(bindings.into_iter().map(|(_, t)| t));
                }
                TyData::Opaque { args, bound, .. } => {
                    todo.extend(pool.list_items(args));
                    todo.push(bound);
                }
                TyData::Assoc { self_ty, args, .. } => {
                    todo.push(self_ty);
                    todo.extend(pool.list_items(args));
                }
                TyData::Row(row) | TyData::Context(row) => todo.extend(pool.row_data(row).keys),
                TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::Param(_)
                | TyData::Const(_)
                | TyData::Infer(_)
                | TyData::Canon(_) => {}
            }
        }
        Ok(bad)
    }
}

impl Ck<'_, '_> {
    /// `base.name = rhs` or `base.name op= rhs` where `base` names the
    /// struct place `place` (`types.value.writable-places`,
    /// `types.value.field-write`).
    pub(crate) fn struct_field_assign(
        &mut self,
        mut place: Place,
        (base, lhs, rhs, s): (NodeRef<'_>, NodeRef<'_>, NodeRef<'_>, NodeRef<'_>),
        name: &str,
        compound: Option<TokenKind>,
    ) -> StageResult<()> {
        let bt = place.ty;
        // A tuple element is a place as a field is (`expr.place.tuple-field`).
        let elem = self.tuple_elem_of(bt, name);
        let Some((idx, ft)) = self.field_of(bt, name).or(elem) else {
            let msg = format!("no field `{name}` on {}", self.show(bt));
            self.err(Code::UnknownField, lhs, &msg);
            return Ok(());
        };
        if elem.is_none() {
            self.check_field_visible(bt, name, lhs);
        }
        if !self.place_writable_for_field(&place, (base, lhs)) {
            return Ok(());
        }
        self.push_field(&mut place, idx, ft);
        let ft = place.ty;
        let v = match compound {
            None => {
                let (r, t) = self.stored_expr(rhs, Some(ft), "in this field")?;
                self.coerce(r, t, ft, rhs, "assignment", ExpectPosition::General)
            }
            Some(k) => {
                let (cur, _) = self.read_place(&place, lhs);
                self.compound(k, cur, ft, rhs, s)?
            }
        };
        self.store_field(&place, v, s);
        Ok(())
    }

    /// Whether a struct field of the place `p`, which `base` names, may be
    /// written by a statement whose target is `at`; reports why not
    /// otherwise. A local root is read here, as a read of `base`
    /// would.
    fn place_writable_for_field(
        &mut self,
        p: &Place,
        (base, at): (NodeRef<'_>, NodeRef<'_>),
    ) -> bool {
        match p.root {
            Root::Local(l, d) => {
                let mut name = base;
                while matches!(name.kind(), SyntaxKind::FieldExpr | SyntaxKind::ParenExpr)
                    && let Some(inner) = name.children().next()
                {
                    name = inner;
                }
                self.note_read(l, d, name);
                self.local_writable(l, at)
            }
            Root::Global(s, ..) => {
                let Some(g) = self.global_binding(s) else {
                    return true;
                };
                if g.mutable {
                    return true;
                }
                let text = self.cx.names.text(s).to_owned();
                self.not_writable(&text, g.short, false, at);
                false
            }
            // `ref.write.field`: through `&mut` only.
            Root::Deref(_, rt) => {
                if self.ref_of(rt).is_some_and(|(_, m)| m) {
                    return true;
                }
                let msg = format!(
                    "this writes through a read-only reference {}; take the reference with `&mut` to write through it",
                    self.show(rt)
                );
                self.err(Code::NotWritable, at, &msg);
                false
            }
            // A list element is a place of a writable list
            // (`types.coll.element-place`).
            Root::Slot(list, _, lt, e) => {
                let index = self.cx.src.parse.tree.node(e);
                let list_node = index.children().next().unwrap_or(index);
                self.coll_change(list, lt, "this field write", list_node)
            }
            // A write names no pinned element (`PlaceUse::Write`).
            Root::Elem(..) => true,
        }
    }

    /// Whether the local `l` is writable: bound with `let mut`. Reports
    /// why not otherwise.
    fn local_writable(&mut self, l: LocalId, at: NodeRef<'_>) -> bool {
        if self.mut_locals.contains(&l) {
            return true;
        }
        let name = self
            .cx
            .names
            .text(self.b.body_mut().local_name[l.idx()])
            .to_owned();
        let flags = self.b.body_mut().local_flags[l.idx()];
        let short = flags & local_flags::SHORT != 0 || self.using_locals.contains(&l);
        let param = flags & local_flags::PARAM != 0;
        self.not_writable(&name, short, param, at);
        // `fn.method.self-struct-write`: the fix takes `&mut self`.
        if param && name == "self" {
            let syn = self.b.body_mut().local_syn[l.idx()];
            self.writable_binding_fix("self", false, true, syn);
        }
        false
    }

    /// Reports a field write through the binding `name`, which is not
    /// `let mut` (`types.value.write.using-split`, `types.value.write.plain-param`,
    /// `types.value.write.let`).
    fn not_writable(&mut self, name: &str, short: bool, param: bool, at: NodeRef<'_>) {
        if param {
            let msg = if name == "self" {
                "`self` is the method's own read-only copy of the receiver; take `&mut self` to change the caller's value".to_owned()
            } else {
                format!(
                    "parameter `{name}` is the function's own copy, which never changes; write `mut {name}` to change the copy"
                )
            };
            self.err(Code::NonReassignableParameterBinding, at, &msg);
        } else if short {
            let msg = format!("`{name}` never changes; bind it with `let mut` to change a field");
            self.err(Code::NonReassignableBinding, at, &msg);
        } else {
            let msg = format!(
                "`{name}` is not `let mut`, so it may only be reassigned as a whole; bind it with `let mut` to change a field"
            );
            self.err(Code::NotWritable, at, &msg);
        }
    }

    /// Stores `v` into the struct field that ends the path of `place`:
    /// one `Store` of the place; a list element's field is stored in the
    /// list's buffer, where the element is (#655).
    fn store_field(&mut self, place: &Place, v: Ref, s: NodeRef<'_>) {
        let at = s.index();
        let fields = place.path.iter().map(|&(k, _)| Proj::Field(k));
        let (root, proj): (PlaceRoot, Vec<Proj>) = match place.root {
            Root::Deref(r, _) => {
                self.store_ref_field(r, &place.path, v, s);
                return;
            }
            // `[%xs [%i] .p .x]`: the field of the list's slot; `[%m [%e]
            // .p .x]`, of the map's value at the key's entry, looked up
            // after the stored value is evaluated.
            Root::Slot(list, key, lt, _) => {
                let index = self.elem_index(list, key, lt, at);
                (
                    PlaceRoot::Value(list),
                    std::iter::once(Proj::Elem(index)).chain(fields).collect(),
                )
            }
            Root::Local(l, d) => {
                self.note_write(l, d);
                self.note_param_write(l, at);
                (PlaceRoot::Local(l), fields.collect())
            }
            Root::Global(_, def, ty) => (PlaceRoot::Global(def, ty), fields.collect()),
            // Reported by `place_writable_for_field`.
            Root::Elem(..) => return,
        };
        let word = self.b.place(&TirPlace { root, proj });
        self.b.emit(Tag::Store, word, v.0, Ty::VOID, at);
    }
}

impl Ck<'_, '_> {
    /// Marks the `mut` parameters of the function `f`: each is its own
    /// changeable copy, so it may be reassigned and, for a struct, its
    /// fields are writable places (`fn.param.mut`). So is `mut self`, for
    /// every kind of `Self` (`fn.method.mut-self-struct`).
    pub(crate) fn mark_mut_params(&mut self, f: NodeRef<'_>) {
        let Some(list) = hd_resolve::Src::child(f, SyntaxKind::ParameterList) else {
            return;
        };
        let toks = &self.cx.src.parse.tokens;
        let marked: Vec<_> = list
            .children()
            .filter(|p| p.kind() == SyntaxKind::Parameter)
            .filter(|p| {
                // `&mut self` and `*mut self` are references, not a
                // changeable copy.
                p.direct_token(toks, TokenKind::KwMut).is_some()
                    && p.direct_token(toks, TokenKind::Amp).is_none()
                    && p.direct_token(toks, TokenKind::Star).is_none()
            })
            .map(|p| {
                let name = if p.direct_token(toks, TokenKind::KwSelfValue).is_some() {
                    "self".to_owned()
                } else {
                    p.name(toks)
                        .map_or_else(String::new, |t| self.cx.src.text(t).to_owned())
                };
                (name, p.index())
            })
            .collect();
        for (name, parameter) in marked {
            let sym = self.cx.names.syms.intern(&name);
            let Some(&l) = self.scopes[0].get(&sym) else {
                continue;
            };
            self.mut_params.push((l, parameter));
            self.mut_locals.push(l);
        }
    }
}

/// The inline-containment walk of one stage-B pass (`data.struct.contains`,
/// `data.struct.no-recursion`, `data.enum.no-recursion`): a value of a type
/// holds the values of its struct fields, enum payloads, tuple elements,
/// union members, optionals and newtypes, but not through a reference, a
/// data type, a collection, a trait value or a function.
///
/// `finite` keeps, across the pass's items, each type whose inline closure
/// is finite: walked to the end, with no cycle and no type past
/// `hd_base::limits::TYPE_DEPTH`. Such a type never holds a declaration
/// that reaches it. If `D` reaches `u` and `u` held `D[a]`, then `D[a]`
/// would reach `u` substituted, and so on without end; the closure of `u`
/// would have a cycle, or types of growing depth, one past `TYPE_DEPTH`.
/// Both questions the header check asks are of that kind: whether a field
/// of `D` holds `D`, and whether a field of a declaration that `D` holds
/// holds `D`. So a walk skips a finite type, and each type is walked to
/// its end once per pass. Only a closure with a cycle or a type past the limit,
/// in a program that is already `recursive-value-type`, is walked again.
///
/// Cost: one visit per type held inline, per pass, in a program with no
/// recursive value type; a chain of `n` declarations held inline costs
/// `n`, not `n` per item that holds its head (#1828).
#[derive(Default)]
pub(crate) struct Containment {
    finite: HashSet<Ty>,
    /// For the run's shared set ([`holds_itself`]): a type that holds a
    /// declaration the walk's lookup lacks is not known finite, as
    /// another lookup may see into it. A header pass's walks share one
    /// lookup and keep such a type as finite.
    strict: bool,
    /// A walk met a declaration its lookup lacks, whose values it could
    /// not see into: an answer may differ under a lookup that holds it.
    blind: bool,
}

/// The types a declaration's value rule covers: a struct's fields, an
/// enum's shared and payload fields (`data.struct.no-recursion`,
/// `data.enum.no-recursion`); `None` for any other item.
pub(crate) fn value_fields(data: &hd_resolve::ItemData) -> Option<Vec<&hd_resolve::Field>> {
    match data {
        hd_resolve::ItemData::Struct(fs, _) => Some(fs.iter().collect()),
        hd_resolve::ItemData::Enum {
            shared, variants, ..
        } => Some(
            shared
                .iter()
                .chain(variants.iter().flat_map(|v| &v.fields))
                .collect(),
        ),
        _ => None,
    }
}

/// A field type `t` of `owner` with `owner`'s parameters renamed to the
/// same parameters of no declaration. A containment walk ends at a
/// parameter whoever owns it, so the answer is the same, but the walks of
/// two declarations meet at the types they share: the fields of
/// `struct C1[T]: next: C0[T]` and `struct C2[U]: next: C1[U]` both reach
/// `C0[T']`, which the first walk keeps as finite. With each owner's own
/// parameters, a chain of `n` generic declarations walked `n` types for
/// each, `n²` in all.
pub(crate) fn unowned(pool: hd_types::Types<'_>, owner: DefId, t: Ty) -> Ty {
    pool.subst(t, &|p: ParamRef| {
        (p.owner == owner).then(|| {
            pool.intern_ty(&TyData::Param(ParamRef {
                owner: DefId::NONE,
                index: p.index,
            }))
        })
    })
}

/// Whether `def` is a declaration the value rule rejects: one of its
/// [`value_fields`] holds a value of `def` inline. Each instance of it is
/// then unsized whatever its arguments, and a walk of an instance's
/// fields need not end: `struct G[T]: x: G[(T, T)]` holds `G[(i32, i32)]`
/// in `G[i32]`, and so on. rustc asks the same once per ADT
/// (`representability`), and layout stops at an ADT it rejected.
///
/// Cost: one answer per declaration per run, kept in `global`; the
/// deciding walks share the run's set of types with a finite inline
/// closure, so each such type is walked once per run, not once per
/// declaration that holds it. An answer decided under a lookup that
/// lacked a declaration the walk met is not kept.
pub(crate) fn holds_itself(
    global: &hd_types::solver::GlobalMemo,
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    def: DefId,
) -> bool {
    global.holds_itself(def, |finite| {
        let mut values = Containment {
            finite: std::mem::take(finite),
            strict: true,
            blind: false,
        };
        let held = lookup
            .item(def)
            .and_then(|it| value_fields(&it.data))
            .is_some_and(|fs| {
                fs.iter()
                    .any(|f| values.holds(pool, lookup, unowned(pool, def, f.ty), def))
            });
        *finite = values.finite;
        (held, !values.blind && lookup.item(def).is_some())
    })
}

impl Containment {
    /// Whether a value of type `t`, a field type of a declaration that
    /// reaches it, holds a value of the struct or enum `target` inline.
    pub(crate) fn holds(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        t: Ty,
        target: DefId,
    ) -> bool {
        self.walk(pool, lookup, t, &mut HashSet::default(), &mut |d| {
            d == target
        })
    }

    /// The declarations whose values a value of type `t` holds inline,
    /// each once, in the order first reached, leaving out those inside a
    /// type known finite: none of them holds a declaration that reaches
    /// it. `seen` holds the types already searched, across calls.
    pub(crate) fn held(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        t: Ty,
        seen: &mut HashSet<Ty>,
        out: &mut Vec<DefId>,
    ) {
        let mut held: HashSet<DefId> = out.iter().copied().collect();
        self.walk(pool, lookup, t, seen, &mut |d| {
            if held.insert(d) {
                out.push(d);
            }
            false
        });
    }

    /// Visits each declaration whose value `t` holds inline, outside the
    /// types known finite, stopping at the first for which `found` is
    /// true; whether one was. Declarations are reached in the order a
    /// left-to-right depth-first search takes.
    ///
    /// A depth-first search on a work stack, each type entered once per
    /// call (`seen`), with no cap on how many: a cycle of 300 declarations
    /// held inline was missed past a cap of 256 types (#1777). A type
    /// deeper than `TYPE_DEPTH` is not entered: only a declaration that
    /// holds itself with growing arguments reaches one
    /// (`struct S[T]: x: S[(T, T)]`), and it is reported. A type whose
    /// walk ends with no edge back into the search's path, no type past
    /// the limit and no such type below it is finite, and joins `finite`.
    fn walk(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        t: Ty,
        seen: &mut HashSet<Ty>,
        found: &mut dyn FnMut(DefId) -> bool,
    ) -> bool {
        /// A type on the search's path: its first kid in `kids`, and
        /// whether its closure is known not finite.
        struct Frame {
            ty: Ty,
            kids: usize,
            open: bool,
        }
        // The kids of the frames on the path, each frame's reversed, so
        // the next to visit is last.
        let mut kids = vec![t];
        let mut path: Vec<Frame> = Vec::new();
        loop {
            // The next kid of the last frame, or none when its kids are
            // done and the frame ends.
            let base = path.last().map_or(0, |f| f.kids);
            let Some(t) = (kids.len() > base).then(|| kids.pop()).flatten() else {
                let Some(done) = path.pop() else {
                    return false;
                };
                if !done.open {
                    self.finite.insert(done.ty);
                } else if let Some(parent) = path.last_mut() {
                    parent.open = true;
                }
                continue;
            };
            if self.finite.contains(&t) {
                continue;
            }
            // A type past the limit, one on the path (a cycle) or one
            // already walked in this call and found open: the parent's
            // closure is not finite.
            if pool.depth(t) > hd_base::limits::TYPE_DEPTH || !seen.insert(t) {
                if let Some(parent) = path.last_mut() {
                    parent.open = true;
                }
                continue;
            }
            crate::work::held_type();
            let start = kids.len();
            let mut open = false;
            match pool.get(t) {
                TyData::Adt { def, args } => {
                    if found(def) {
                        return true;
                    }
                    // A declaration the lookup lacks: its values are not
                    // seen.
                    let blind = !field_tys(pool, lookup, def, args, &mut kids);
                    self.blind |= blind;
                    open = blind && self.strict;
                }
                // A union holds its members inline, as an enum its payloads
                // (`types.union.representation`, `data.struct.contains`).
                TyData::Tuple { elems: parts, .. } | TyData::Union(parts) => {
                    kids.extend_from_slice(pool.list_items(parts));
                }
                // Containment ends at a reference, a trait value and a
                // function value (`data.struct.contains.ends`); the rest
                // hold no declaration's value: a parameter, a primitive, a
                // projection or opaque type, whose value the instance
                // decides, and a goal in error or still open.
                TyData::Ref { .. }
                | TyData::TraitValue { .. }
                | TyData::Fn { .. }
                | TyData::Param(_)
                | TyData::Prim(_)
                | TyData::Never
                | TyData::Poison
                | TyData::Opaque { .. }
                | TyData::Assoc { .. }
                | TyData::Const(_)
                | TyData::Infer(_)
                | TyData::Canon(_)
                | TyData::Row(_)
                | TyData::Context(_) => {}
            }
            kids[start..].reverse();
            path.push(Frame {
                ty: t,
                kids: start,
                open,
            });
        }
    }
}

/// The types of the fields and payloads of `def` applied to `args`: what
/// a value of it holds directly. False when `lookup` lacks `def`.
fn field_tys(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    def: DefId,
    args: hd_types::TyList,
    tys: &mut Vec<Ty>,
) -> bool {
    let arg_tys = pool.list_items(args);
    let sub = |f: Ty| {
        pool.subst(f, &|p: ParamRef| {
            (p.owner == def)
                .then(|| arg_tys.get(usize::from(p.index)).copied())
                .flatten()
        })
    };
    let Some(item) = lookup.item(def) else {
        return false;
    };
    match &item.data {
        hd_resolve::ItemData::Struct(fs, _) => {
            tys.extend(fs.iter().map(|f| sub(f.ty)));
        }
        hd_resolve::ItemData::Enum {
            shared, variants, ..
        } => {
            tys.extend(
                shared
                    .iter()
                    .chain(variants.iter().flat_map(|v| &v.fields))
                    .map(|f| sub(f.ty)),
            );
        }
        hd_resolve::ItemData::Newtype(base) => tys.push(sub(*base)),
        // `List` and `Map` hold their elements in heap storage and
        // `Suspend` its result in its task (hd_resolve seed.rs); an
        // alias is expanded when it is lowered, and the rest name no
        // type.
        hd_resolve::ItemData::Builtin
        | hd_resolve::ItemData::Alias { .. }
        | hd_resolve::ItemData::Fn(..)
        | hd_resolve::ItemData::Trait(_)
        | hd_resolve::ItemData::Impl { .. }
        | hd_resolve::ItemData::Method { .. }
        | hd_resolve::ItemData::AssocType { .. } => {}
    }
    true
}

#[cfg(test)]
mod containment_tests {
    use super::*;

    #[test]
    fn projection_graph_work_depends_only_on_reachable_width_and_depth() {
        // A thousand roots share a depth-20 suffix. A repeated check adds
        // no expansion, even with ten times as much unrelated memo state.
        for unrelated in [100, 1000] {
            let mut done = HashMap::default();
            for i in 0..unrelated {
                done.insert(Ty(100_000 + i), None);
            }
            let mut fuel = Fuel::new(100_000);
            let mut expanded = 0;
            let mut expand = |t: Ty| {
                expanded += 1;
                if t.0 < 1000 {
                    vec![Ty(5000 + t.0)]
                } else if t.0 < 1019 {
                    // Shared branches must not cause exponential retries.
                    vec![Ty(t.0 + 1), Ty(t.0 + 1)]
                } else {
                    vec![]
                }
            };
            for _ in 0..2 {
                for i in 0..1000 {
                    assert_eq!(
                        ProjectionCycles::search(
                            &mut done,
                            Ty(i),
                            &mut fuel,
                            &mut |_| false,
                            &mut Vec::new(),
                            &mut |t| if (5000..6000).contains(&t.0) {
                                Ty(1000)
                            } else {
                                t
                            },
                            &mut expand
                        ),
                        Ok(None)
                    );
                }
            }
            assert_eq!(expanded, 1020);
            // The first projection enters the suffix's root directly;
            // only its other nineteen nodes need separate enter events.
            assert_eq!(fuel.spent(), 2019);
        }
    }

    #[test]
    fn a_thousand_node_projected_ring_is_iterative_and_memoized() {
        let mut done = HashMap::default();
        let mut fuel = Fuel::new(2000);
        let mut expanded = 0;
        let mut expand = |t: Ty| {
            expanded += 1;
            vec![Ty(t.0 + 1)]
        };
        assert_eq!(
            ProjectionCycles::search(
                &mut done,
                Ty(0),
                &mut fuel,
                &mut |_| false,
                &mut Vec::new(),
                &mut |t| if t == Ty(1000) { Ty(0) } else { t },
                &mut expand
            ),
            Ok(Some(Ty(0)))
        );
        for i in 0..1000 {
            assert_eq!(
                ProjectionCycles::search(
                    &mut done,
                    Ty(i),
                    &mut fuel,
                    &mut |_| false,
                    &mut Vec::new(),
                    &mut |t| if t == Ty(1000) { Ty(0) } else { t },
                    &mut expand
                ),
                Ok(Some(Ty(0)))
            );
        }
        assert_eq!(expanded, 1000);
        assert_eq!(fuel.spent(), 1001);
    }

    #[test]
    fn exhaustion_does_not_cache_an_unfinished_graph_as_valid() {
        let mut done = HashMap::default();
        let mut expand = |t: Ty| vec![Ty(t.0 + 1)];
        let mut fuel = Fuel::new(20);
        assert_eq!(
            ProjectionCycles::search(
                &mut done,
                Ty(0),
                &mut fuel,
                &mut |_| false,
                &mut Vec::new(),
                &mut |t| t,
                &mut expand
            ),
            Err(())
        );
        assert!(done.is_empty());
    }
}
