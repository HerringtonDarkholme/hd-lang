//! Struct values (spec 08 "Struct Declarations", spec 04 "Value Types").
//! A struct value has no identity, so a field write changes only the
//! writable place it names: a `let mut` local or top-level binding, a
//! field of a data value with mutable access, what a `&mut` refers to, a
//! list element, a map value (`types.coll.map-value.place`), or a struct
//! field of one of these (`types.value.writable-forms`).
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

use hd_base::{DefId, LocalId, StageResult};
use hd_diag::Code;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Place as TirPlace, PlaceRoot, Proj, Ref, Tag, TirSink, local_flags};
use hd_types::{ParamRef, Ty, TyData};

use crate::body::{Ck, ExpectPosition};
use crate::refs::{Place, Root};

impl Ck<'_, '_> {
    /// `base.name = rhs` or `base.name op= rhs` where `base` names the
    /// struct place `place` (`types.value.writable-forms`,
    /// `types.value.field-write`).
    pub(crate) fn struct_field_assign(
        &mut self,
        mut place: Place,
        (base, lhs, rhs, s): (NodeRef<'_>, NodeRef<'_>, NodeRef<'_>, NodeRef<'_>),
        name: &str,
        compound: Option<TokenKind>,
    ) -> StageResult<()> {
        let bt = place.ty;
        let Some((idx, ft)) = self.field_of(bt, name) else {
            let msg = format!("no field `{name}` on {}", self.show(bt));
            self.err(Code::UnknownField, lhs, &msg);
            return Ok(());
        };
        self.check_field_visible(bt, name, lhs);
        if !self.place_writable_for_field(&place, (base, lhs)) {
            return Ok(());
        }
        place.path.push((idx, ft));
        place.ty = ft;
        let v = match compound {
            None => {
                let (r, t) = self.expr(rhs, Some(ft))?;
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
            // A field of a data value: the store needs mutable access to
            // that value (`types.path.store.not-writable`).
            Root::DataField(o, _, _, dt) => {
                if self.has_mut_access(dt) {
                    return true;
                }
                self.check_store_target((o, dt), at);
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
    /// `let mut` (`types.value.write.short`, `types.value.write.plain-param`,
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
                (PlaceRoot::Local(l), fields.collect())
            }
            Root::Global(_, def, ty) => (PlaceRoot::Global(def, ty), fields.collect()),
            Root::DataField(o, k, ..) => (
                PlaceRoot::Value(o),
                [Proj::Deref, Proj::Field(k)]
                    .into_iter()
                    .chain(fields)
                    .collect(),
            ),
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
    /// fields are writable places (`fn.param.mut`). `mut self` on a data
    /// type keeps its `mut` type and is not marked; on a struct, a list or
    /// a map it is (`fn.method.mut-self-struct`).
    pub(crate) fn mark_mut_params(&mut self, f: NodeRef<'_>) {
        let Some(list) = hd_resolve::Src::child(f, SyntaxKind::ParameterList) else {
            return;
        };
        let toks = &self.cx.src.parse.tokens;
        let marked: Vec<_> = list
            .children()
            .filter(|p| p.kind() == SyntaxKind::Parameter)
            .filter(|p| {
                // `&mut self` is a reference, not a changeable copy.
                p.direct_token(toks, TokenKind::KwMut).is_some()
                    && p.direct_token(toks, TokenKind::Amp).is_none()
            })
            .map(|p| {
                if p.direct_token(toks, TokenKind::KwSelfValue).is_some() {
                    "self".to_owned()
                } else {
                    p.name(toks)
                        .map_or_else(String::new, |t| self.cx.src.text(t).to_owned())
                }
            })
            .collect();
        for name in marked {
            let sym = self.cx.names.syms.intern(&name);
            let Some(&l) = self.scopes[0].get(&sym) else {
                continue;
            };
            // A data type's `mut self` keeps its `mut` type. A rejected
            // `mut` view of a struct is marked, so one error is reported.
            let t = self.b.local_ty(l);
            if matches!(self.pool().get(t), TyData::Mut(_)) {
                if self.is_coll_ty(t) {
                    // A list's or a map's own copy has the value type.
                    self.b.body_mut().local_ty[l.idx()] = self.strip_mut(t);
                } else if !self.is_struct_ty(t) {
                    continue;
                }
            }
            self.mut_params.push(l);
            self.mut_locals.push(l);
        }
    }
}

/// Whether a value of type `t` holds a value of the struct or enum
/// `target` inline: through struct fields, enum payloads, tuple elements,
/// optionals and newtypes, but not through a reference, a data type, a
/// collection, a trait value or a function (`data.struct.contains`,
/// `data.struct.no-recursion`, `data.enum.no-recursion`). `seen` holds the
/// types already searched.
pub(crate) fn holds_value(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    target: DefId,
    seen: &mut Vec<Ty>,
) -> bool {
    walk_values(pool, lookup, t, seen, &mut |d| d == target)
}

/// The declarations whose values a value of type `t` holds inline, as
/// [`holds_value`] searches them, each once, in the order first reached.
pub(crate) fn held_values(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    seen: &mut Vec<Ty>,
    out: &mut Vec<DefId>,
) {
    walk_values(pool, lookup, t, seen, &mut |d| {
        if !out.contains(&d) {
            out.push(d);
        }
        false
    });
}

/// Visits each declaration whose value `t` holds inline, stopping at the
/// first for which `found` is true; whether one was.
fn walk_values(
    pool: hd_types::Types<'_>,
    lookup: &hd_resolve::Lookup<'_>,
    t: Ty,
    seen: &mut Vec<Ty>,
    found: &mut dyn FnMut(DefId) -> bool,
) -> bool {
    if seen.contains(&t) || seen.len() > 256 {
        return false;
    }
    seen.push(t);
    let field_tys = |def: DefId, args: hd_types::TyList, tys: &mut Vec<Ty>| {
        let argv = pool.list_items(args);
        let sub = |f: Ty| {
            pool.subst(f, &|p: ParamRef| {
                (p.owner == def)
                    .then(|| argv.get(usize::from(p.index)).copied())
                    .flatten()
            })
        };
        let Some(item) = lookup.item(def) else {
            return;
        };
        match &item.data {
            hd_resolve::ItemData::Data(fs) if item.is_struct => {
                tys.extend(fs.iter().map(|f| sub(f.ty)));
            }
            hd_resolve::ItemData::Enum { shared, variants } => {
                tys.extend(
                    shared
                        .iter()
                        .chain(variants.iter().flat_map(|v| &v.fields))
                        .map(|f| sub(f.ty)),
                );
            }
            hd_resolve::ItemData::Newtype(base) => tys.push(sub(*base)),
            _ => {}
        }
    };
    let mut inner = Vec::new();
    match pool.get(t) {
        TyData::Adt { def, args } => {
            if found(def) {
                return true;
            }
            field_tys(def, args, &mut inner);
        }
        TyData::Option(i) | TyData::Mut(i) => inner.push(i),
        TyData::Tuple { elems, .. } => inner.extend(pool.list_items(elems).iter().copied()),
        _ => {}
    }
    inner
        .into_iter()
        .any(|i| walk_values(pool, lookup, i, seen, found))
}
