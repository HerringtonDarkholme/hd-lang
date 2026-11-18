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
    /// The normal forms of the parts of the types normalized so far, so a
    /// part shared by many visited types is normalized once.
    norm: hd_types::solver::ConcreteNorm,
    /// The run's memo and the impl universe of a view with no hidden
    /// local impl: then a closed type's answer is the same in every body.
    shared: Option<(
        &'a hd_types::solver::GlobalMemo,
        hd_types::solver::ImplUniverseId,
    )>,
    /// A projection past the depth limit was met: the body's `done`
    /// holds answers that stopped there, so nothing more is published.
    tainted: bool,
    /// The value rule's decisions under this walk's lookup alone
    /// ([`holds_itself`]).
    inline: InlineDecls,
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
                let n = match self.norm.normalize(pool, impls, t) {
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
                    TyData::Adt { def, .. }
                        if holds_itself(global, &mut self.inline, pool, lookup, def) => {}
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
                                hd_resolve::ItemData::Newtype(base, _) => children.push(sub(*base)),
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
                self.store_ref_field(r, &place.path, place.ty, v, s);
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
            let Some(&l) = self.scopes.base().get(&sym) else {
                continue;
            };
            self.mut_params.push((l, parameter));
            self.mut_locals.push(l);
        }
    }
}

/// The value rule's decisions under one lookup ([`InlineDecl`],
/// `data.struct.contains`, `data.struct.no-recursion`,
/// `data.enum.no-recursion`): a value of a type holds the values of its
/// struct fields, enum payloads, tuple elements, union members, optionals
/// and newtypes, but not through a reference, a data type, a collection, a
/// trait value or a function.
///
/// Each declaration is decided once, as rustc decides `params_in_repr` and
/// `representability` once per ADT, never per instance: an instance's
/// fields may grow without end (`struct G[T]: x: G[(T, T)]` holds
/// `G[(i32, i32)]` in `G[i32]`, and with a second growing field `2^k`
/// instances at depth `k`, #2011), but its declaration's fields are
/// written once. A decision made under a lookup that lacks a declaration it
/// reaches (another module's private struct) may differ under a lookup that
/// sees it, so it stays in `local`, the asker's own memo; the rest go to the
/// run's memo, shared by every header and body.
///
/// Cost, per run: each declaration a value can hold is decided once, with
/// two walks of its field types, each entering a type once per
/// declaration, plus a strongly connected components pass over the
/// declarations and over the edges of each component. A decision under a
/// lacking lookup costs the same once per asker's memo.
pub(crate) type InlineDecls = hd_types::solver::InlineDecls;

/// The decisions one header pass reads: its lookup's own, and which member
/// of each group of declarations that hold one another reports the group
/// (`data.enum.no-recursion.group`), by the group's first member.
#[derive(Default)]
pub(crate) struct InlinePass {
    pub(crate) local: InlineDecls,
    reporters: HashMap<DefId, Option<DefId>>,
}

impl InlinePass {
    /// [`with_decided`] with this pass's own decisions.
    pub(crate) fn decided<R>(
        &mut self,
        global: &hd_types::solver::GlobalMemo,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        def: DefId,
        f: impl FnOnce(Decided<'_>) -> R,
    ) -> R {
        with_decided(global, &mut self.local, pool, lookup, def, f)
    }

    /// The member of `group` that reports it: of its structs and enums,
    /// the one whose `key` (module path, then name) sorts first
    /// (`data.enum.no-recursion.group`); in a group of newtypes alone, the
    /// newtype whose `key` sorts first (`types.newtype.no-recursion.group`).
    /// Chosen once per group per pass, in one pass over the group.
    pub(crate) fn reporter<K: Ord>(
        &mut self,
        lookup: &hd_resolve::Lookup<'_>,
        group: &[DefId],
        key: impl Fn(DefId) -> K,
    ) -> Option<DefId> {
        let head = *group.first()?;
        *self.reporters.entry(head).or_insert_with(|| {
            group
                .iter()
                .copied()
                .filter(|&d| {
                    matches!(
                        lookup.item(d).map(|i| &i.data),
                        Some(
                            hd_resolve::ItemData::Struct(..)
                                | hd_resolve::ItemData::Enum { .. }
                                | hd_resolve::ItemData::Newtype(..)
                        )
                    )
                })
                .min_by_key(|&d| {
                    let newtype = matches!(
                        lookup.item(d).map(|i| &i.data),
                        Some(hd_resolve::ItemData::Newtype(..))
                    );
                    (newtype, key(d))
                })
        })
    }
}

/// The types a declaration's value rule covers, in written order: a
/// struct's fields, an enum's shared and payload fields, a newtype's base
/// (`data.struct.no-recursion`, `data.enum.no-recursion`,
/// `types.newtype.no-recursion`); `None` for any other item. They are
/// the ones [`inline_tys`] gives, so a declaration the value rule reports
/// is one its decision finds cyclic.
pub(crate) fn value_fields(data: &hd_resolve::ItemData) -> Option<Vec<Ty>> {
    let mut out = Vec::new();
    match data {
        hd_resolve::ItemData::Struct(..)
        | hd_resolve::ItemData::Enum { .. }
        | hd_resolve::ItemData::Newtype(..) => inline_tys(data, &mut out),
        _ => return None,
    }
    Some(out)
}

/// The types a value of a declaration holds directly: a struct's fields,
/// an enum's shared and payload fields, a newtype's base. `List` and `Map`
/// hold their elements in heap storage and `Suspend` its result in its
/// task (`hd_resolve` `seed.rs`); an alias is expanded when it is lowered, and
/// the rest name no type.
fn inline_tys(data: &hd_resolve::ItemData, out: &mut Vec<Ty>) {
    match data {
        hd_resolve::ItemData::Struct(fs, _) => out.extend(fs.iter().map(|f| f.ty)),
        hd_resolve::ItemData::Enum {
            shared, variants, ..
        } => out.extend(
            shared
                .iter()
                .chain(variants.iter().flat_map(|v| &v.fields))
                .map(|f| f.ty),
        ),
        hd_resolve::ItemData::Newtype(base, _) => out.push(*base),
        hd_resolve::ItemData::Builtin
        | hd_resolve::ItemData::Alias { .. }
        | hd_resolve::ItemData::Fn(..)
        | hd_resolve::ItemData::Trait(_)
        | hd_resolve::ItemData::Impl { .. }
        | hd_resolve::ItemData::Method { .. }
        | hd_resolve::ItemData::AssocType { .. } => {}
    }
}

/// The types a value of type `t` holds directly that may hold a
/// declaration: a tuple's elements, a union's members, and the arguments of
/// an instance whose declaration holds that parameter (`repr`, or every
/// argument when `repr` is not known yet). Containment ends at a
/// reference, a trait value and a function value
/// (`data.struct.contains.ends`); the rest hold no declaration's value: a
/// parameter, a primitive, a projection or opaque type, whose value the
/// instance decides, and a goal in error or still open.
fn inline_kids(pool: hd_types::Types<'_>, t: Ty, repr: Option<&[bool]>, out: &mut Vec<Ty>) {
    match pool.get(t) {
        TyData::Adt { args, .. } => {
            let args = pool.list_items(args);
            match repr {
                Some(repr) => out.extend(
                    args.iter()
                        .zip(repr)
                        .filter(|&(_, &held)| held)
                        .map(|(&a, _)| a),
                ),
                None => out.extend_from_slice(args),
            }
        }
        TyData::Tuple { elems: parts, .. } | TyData::Union(parts) => {
            out.extend_from_slice(pool.list_items(parts));
        }
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
}

/// The decided declarations an asker reads: the run's, then its own.
#[derive(Clone, Copy)]
pub(crate) struct Decided<'m> {
    run: &'m InlineDecls,
    local: &'m InlineDecls,
}

impl<'m> Decided<'m> {
    pub(crate) fn get(self, d: DefId) -> Option<&'m hd_types::solver::InlineDecl> {
        self.run.get(&d).or_else(|| self.local.get(&d))
    }

    /// Whether a value of type `t` holds a value of a member of the group
    /// named `head` ([`hd_types::solver::InlineDecl::group`]), when every
    /// declaration `t` holds is decided: one walk of `t`, each type entered
    /// once.
    pub(crate) fn holds_group(self, pool: hd_types::Types<'_>, t: Ty, head: DefId) -> bool {
        let mut seen = HashSet::default();
        let mut work = vec![t];
        while let Some(t) = work.pop() {
            if !seen.insert(t) {
                continue;
            }
            crate::work::held_type();
            let repr = match pool.get(t) {
                TyData::Adt { def, .. } => {
                    let Some(d) = self.get(def) else { continue };
                    if d.group.first() == Some(&head) {
                        return true;
                    }
                    Some(&*d.repr)
                }
                _ => None,
            };
            inline_kids(pool, t, repr, &mut work);
        }
        false
    }
}

/// Runs `f` on the decisions, with `def` and every declaration a value of
/// it holds decided ([`decide`]). The run's memo is locked meanwhile.
pub(crate) fn with_decided<R>(
    global: &hd_types::solver::GlobalMemo,
    local: &mut InlineDecls,
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    def: DefId,
    f: impl FnOnce(Decided<'_>) -> R,
) -> R {
    global.inline_decls(|run| {
        decide(pool, lookup, run, local, def);
        f(Decided { run, local })
    })
}

/// Whether `def` is a struct, enum or newtype the value rule rejects: a
/// value of it holds a value of it inline (`data.struct.no-recursion`,
/// `data.enum.no-recursion`, `types.newtype.no-recursion`). Each
/// instance of it is then unsized whatever its arguments, and a walk of
/// an instance's members need not end (`type G[T](G[(T, T)])` holds
/// `G[(i32, i32)]` in `G[i32]`), so the containment and `Copy` walks stop
/// there. One probe of the run's decisions, each decided once per run.
pub(crate) fn holds_itself(
    global: &hd_types::solver::GlobalMemo,
    local: &mut InlineDecls,
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    def: DefId,
) -> bool {
    matches!(
        lookup.item(def).map(|i| &i.data),
        Some(
            hd_resolve::ItemData::Struct(..)
                | hd_resolve::ItemData::Enum { .. }
                | hd_resolve::ItemData::Newtype(..)
        )
    ) && with_decided(global, local, pool, lookup, def, |d| {
        d.get(def).is_some_and(|d| d.cyclic)
    })
}

/// The declarations the checker's solver reads: its lookup's, except that
/// a declaration the value rule rejects ([`holds_itself`]) has no members
/// for the `Copy` row, so the row's walk stops at its instance, as the
/// ownership walk does (`crate::moves`). Its fields may grow without end
/// (`struct G[T]: x: G[(T, T)]` holds `G[(i32, i32)]` in `G[i32]`, #2013),
/// and the declaration is reported. Cost: one lock and probe of the run's
/// decisions per declaration the row meets, each decided once per run.
pub(crate) struct SolverDecls<'d, 'm> {
    pub(crate) decls: hd_resolve::LookupDecls<'d>,
    pub(crate) global: &'d hd_types::solver::GlobalMemo,
    /// The asker's own decisions ([`with_decided`]).
    pub(crate) local: std::cell::RefCell<&'m mut InlineDecls>,
}

impl hd_types::solver::Declarations for SolverDecls<'_, '_> {
    fn sealed(&self) -> &hd_types::solver::SealedTraits {
        self.decls.sealed()
    }
    fn type_decl(&self, def: DefId) -> hd_types::solver::TypeDecl {
        self.decls.type_decl(def)
    }
    fn supertraits(&self, trait_: DefId) -> &[Ty] {
        self.decls.supertraits(trait_)
    }
    fn has_unavailable_member(&self, trait_: DefId) -> bool {
        self.decls.has_unavailable_member(trait_)
    }
    fn has_unavailable_reference_member(&self, trait_: DefId, mutable: bool) -> bool {
        self.decls.has_unavailable_reference_member(trait_, mutable)
    }
    fn is_local(&self, def: DefId) -> bool {
        self.decls.is_local(def)
    }
    fn is_const_slot(&self, def: DefId, index: usize) -> bool {
        self.decls.is_const_slot(def, index)
    }
    fn copy_decl(&self, def: DefId) -> hd_types::solver::CopyDecl {
        match self.decls.copy_decl(def) {
            hd_types::solver::CopyDecl::Members(_)
                if holds_itself(
                    self.global,
                    &mut self.local.borrow_mut(),
                    self.decls.pool.types(),
                    self.decls.lookup,
                    def,
                ) =>
            {
                hd_types::solver::CopyDecl::Copies
            }
            d => d,
        }
    }
    fn trait_requires_copy(&self, trait_: DefId) -> bool {
        self.decls.trait_requires_copy(trait_)
    }
}

/// Decides `root` and every undecided declaration a value of it may hold.
///
/// The declarations are found by one walk each of their field types
/// ([`inline_kids`], every argument of an undecided instance), and their
/// strongly connected components ([`components`]) are decided sinks
/// first, so each component reads only decided declarations outside it.
/// Within a component, which parameters each member holds is a least
/// fixpoint, solved as Horn clauses by one worklist walk of the members'
/// field types: an argument of a member's instance waits on that member's
/// parameter, and is walked when the parameter is found held. The edges the
/// walk meets, from a member to the members its fields hold, give the
/// groups, by a second components pass. A component is decided under this
/// lookup alone when a member is one the lookup lacks, or one it reaches
/// was decided so.
fn decide(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    run: &mut InlineDecls,
    local: &mut InlineDecls,
    root: DefId,
) {
    if run.contains_key(&root) || local.contains_key(&root) {
        return;
    }
    // The undecided declarations, by number, each with the numbers of the
    // undecided ones its fields may hold, and whether it is known only to
    // this lookup: one it lacks, or one that holds a declaration decided
    // under this lookup alone.
    let mut defs = vec![root];
    let mut ids: HashMap<DefId, usize> = HashMap::default();
    ids.insert(root, 0);
    let mut edges: Vec<Vec<usize>> = Vec::new();
    let mut blind: Vec<bool> = Vec::new();
    let mut tys = Vec::new();
    let mut seen = HashSet::default();
    let mut mentioned = HashSet::default();
    while let Some(&d) = defs.get(edges.len()) {
        let mut out = Vec::new();
        let mut lacking = false;
        match lookup.item(d) {
            Some(item) => inline_tys(&item.data, &mut tys),
            None => lacking = true,
        }
        seen.clear();
        mentioned.clear();
        while let Some(t) = tys.pop() {
            if !seen.insert(t) {
                continue;
            }
            crate::work::held_type();
            let mut repr = None;
            if let TyData::Adt { def: e, .. } = pool.get(t) {
                if let Some(known) = run.get(&e) {
                    repr = Some(&*known.repr);
                } else if let Some(known) = local.get(&e) {
                    lacking = true;
                    repr = Some(&*known.repr);
                } else {
                    let next = defs.len();
                    let id = *ids.entry(e).or_insert(next);
                    if id == next {
                        defs.push(e);
                    }
                    if mentioned.insert(id) {
                        out.push(id);
                    }
                }
            }
            inline_kids(pool, t, repr, &mut tys);
        }
        edges.push(out);
        blind.push(lacking);
    }
    let comps = components(&edges);
    // Each declaration's component, and its place in it.
    let mut comp = vec![(0, 0); defs.len()];
    for (c, members) in comps.iter().enumerate() {
        for (i, &m) in members.iter().enumerate() {
            comp[m] = (c, i);
        }
    }
    for (c, members) in comps.iter().enumerate() {
        let lacking = members.iter().any(|&m| {
            blind[m]
                || edges[m]
                    .iter()
                    .any(|&w| comp[w].0 != c && local.contains_key(&defs[w]))
        });
        let decided = decide_component(
            pool,
            lookup,
            Decided { run, local },
            &defs,
            &ids,
            &comp,
            c,
            members,
        );
        let into = if lacking { &mut *local } else { &mut *run };
        into.extend(decided);
    }
}

/// The decisions of the members of component `c` (`members`, numbers in
/// `defs`), every declaration they hold outside it decided in `known`.
#[allow(clippy::too_many_arguments)]
fn decide_component(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    known: Decided<'_>,
    defs: &[DefId],
    ids: &HashMap<DefId, usize>,
    comp: &[(usize, usize)],
    c: usize,
    members: &[usize],
) -> Vec<(DefId, hd_types::solver::InlineDecl)> {
    // The member a declaration is, by place.
    let member = |e: DefId| {
        ids.get(&e)
            .map(|&id| comp[id])
            .and_then(|(ec, i)| (ec == c).then_some(i))
    };
    let n = members.len();
    let mut repr: Vec<Vec<bool>> = members
        .iter()
        .map(|&m| vec![false; lookup.item(defs[m]).map_or(0, |it| it.generics.len())])
        .collect();
    let mut holds: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut waiting: HashMap<(usize, usize), Vec<(usize, Ty)>> = HashMap::default();
    let mut work: Vec<(usize, Ty)> = Vec::new();
    let mut tys = Vec::new();
    for (i, &m) in members.iter().enumerate() {
        if let Some(item) = lookup.item(defs[m]) {
            inline_tys(&item.data, &mut tys);
            work.extend(tys.drain(..).map(|t| (i, t)));
        }
    }
    let mut seen = HashSet::default();
    while let Some((i, t)) = work.pop() {
        if !seen.insert((i, t)) {
            continue;
        }
        crate::work::held_type();
        match pool.get(t) {
            TyData::Adt { def: e, args } => {
                if let Some(j) = member(e) {
                    holds[i].push(j);
                    for (k, &a) in pool.list_items(args).iter().enumerate() {
                        if repr[j].get(k) == Some(&true) {
                            work.push((i, a));
                        } else {
                            waiting.entry((j, k)).or_default().push((i, a));
                        }
                    }
                    continue;
                }
                inline_kids(
                    pool,
                    t,
                    Some(known.get(e).map_or(&[][..], |d| &d.repr)),
                    &mut tys,
                );
            }
            TyData::Param(p) if p.owner == defs[members[i]] => {
                let k = usize::from(p.index);
                if let Some(held) = repr[i].get_mut(k)
                    && !*held
                {
                    *held = true;
                    work.extend(waiting.remove(&(i, k)).unwrap_or_default());
                }
                continue;
            }
            _ => inline_kids(pool, t, None, &mut tys),
        }
        work.extend(tys.drain(..).map(|u| (i, u)));
    }
    let mut out = Vec::with_capacity(n);
    for group in components(&holds) {
        let names: std::sync::Arc<[DefId]> = group.iter().map(|&i| defs[members[i]]).collect();
        for &i in &group {
            out.push((
                defs[members[i]],
                hd_types::solver::InlineDecl {
                    repr: std::mem::take(&mut repr[i]).into(),
                    group: names.clone(),
                    cyclic: group.len() > 1 || holds[i].contains(&i),
                },
            ));
        }
    }
    out
}

/// The strongly connected components of the graph with `edges`, each a
/// list of node numbers, every component after the ones it reaches
/// ([`hd_base::scc`]).
fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let sccs = hd_base::scc::sccs_of(edges);
    (sccs.iter())
        .map(|m| m.iter().map(|&x| x as usize).collect())
        .collect()
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
