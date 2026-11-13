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
#[derive(Default)]
pub(crate) struct ProjectionCycles {
    done: HashMap<Ty, Option<Ty>>,
    reported: HashSet<Ty>,
    checked: HashSet<Ty>,
    normalized: HashMap<Ty, Ty>,
}

impl ProjectionCycles {
    pub(crate) fn check(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        impls: hd_types::solver::Impls<'_>,
        ty: Ty,
        fuel: &mut Fuel,
    ) -> Result<bool, ()> {
        // A rigid projection can acquire a concrete payload at a later
        // instantiation. Do not remember that open instance as acyclic.
        if pool.has_param(ty) || pool.has_infer(ty) {
            return Ok(false);
        }
        let cycle = Self::search(
            &mut self.done,
            ty,
            fuel,
            &mut |t| {
                *self
                    .normalized
                    .entry(t)
                    .or_insert_with(|| hd_types::solver::normalize_concrete(pool, impls, t))
            },
            &mut |t| {
                let mut children = Vec::new();
                match pool.get(t) {
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
                children
            },
        )?;
        Ok(cycle.is_some_and(|t| self.reported.insert(t)))
    }

    fn search(
        done: &mut HashMap<Ty, Option<Ty>>,
        root: Ty,
        fuel: &mut Fuel,
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

    /// Written type components, separate from containment edges: a reference
    /// ends containment but its referent must still be a well-formed type.
    /// Used after body inference settles, outside speculative trials.
    pub(crate) fn check_nested(
        &mut self,
        pool: hd_types::Types<'_>,
        lookup: &hd_resolve::Lookup<'_>,
        impls: hd_types::solver::Impls<'_>,
        ty: Ty,
        fuel: &mut Fuel,
    ) -> Result<bool, ()> {
        let mut todo = vec![ty];
        let mut bad = false;
        while let Some(t) = todo.pop() {
            if !self.checked.insert(t) {
                continue;
            }
            bad |= self.check(pool, lookup, impls, t, fuel)?;
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
            let base = path.last().map_or(0, |f| f.kids);
            if kids.len() == base {
                let Some(done) = path.pop() else {
                    return false;
                };
                if !done.open {
                    self.finite.insert(done.ty);
                } else if let Some(parent) = path.last_mut() {
                    parent.open = true;
                }
                continue;
            }
            let Some(t) = kids.pop() else {
                return false;
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
            match pool.get(t) {
                TyData::Adt { def, args } => {
                    if found(def) {
                        return true;
                    }
                    field_tys(pool, lookup, def, args, &mut kids);
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
                open: false,
            });
        }
    }
}

/// The types of the fields and payloads of `def` applied to `args`: what
/// a value of it holds directly.
fn field_tys(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    def: DefId,
    args: hd_types::TyList,
    tys: &mut Vec<Ty>,
) {
    let arg_tys = pool.list_items(args);
    let sub = |f: Ty| {
        pool.subst(f, &|p: ParamRef| {
            (p.owner == def)
                .then(|| arg_tys.get(usize::from(p.index)).copied())
                .flatten()
        })
    };
    let Some(item) = lookup.item(def) else {
        return;
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
            ProjectionCycles::search(&mut done, Ty(0), &mut fuel, &mut |t| t, &mut expand),
            Err(())
        );
        assert!(done.is_empty());
    }
}
