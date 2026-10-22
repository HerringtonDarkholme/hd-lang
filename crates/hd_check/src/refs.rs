//! References (spec/lang/13-values-and-references.md): the types `&T` and
//! `&mut T`, `&e` and `&mut e` on places and on other values, `*e` and
//! writes through a reference, automatic dereference on field access and
//! method calls, and the read-only rules (`&mut T` converts to `&T`;
//! reads keep declared field types, and `&T` protects its immediate referent).
//!
//! A reference is a `Ref` of its place (`docs/compiler/tir-v2.md`,
//! "`&` and `&mut`"), which the backend lowers to the place's address
//! (`crates/hd_wasm_linear/DESIGN.md` §23.20); a read through one is a
//! `Read` of a place through it, `[*%r]` or with the struct fields it
//! follows, `[*%r .x]`, a write a `Store` into such a place, and its
//! identity `RefIdentity`. One builder, [`Ck::place_of`], gives the
//! place of `&`, `&mut`, an automatic address and a write: `&x` on a
//! local is `Ref [$x]`, so the backend keeps `x` in a cell; a place
//! through a reference or in a data value starts at that value, as
//! `[*%r .x]` or `[*%o .k .x]`; `&e` on any other value binds it to a
//! fresh local, which is the new object. A field write is one `Store` of
//! its place (`value.rs`). The checker lowers `&T` and
//! `&mut T` to one type, `Ref[T]`, when it finishes a body
//! (`Types::lower_refs`), so the layout pass recognizes the reference
//! carrier and `&mut T` to `&T` costs nothing.
//!
//! Identity (`ref.identity.*`): two references are the same place when
//! they hold the same address, which `r is s` and `identity_of` compare
//! in the backend.

use hd_base::{DefId, LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::{Code, FixSafety, Severity};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{
    Coercion, NONE, Place as TirPlace, PlaceRoot, Proj, ReadMode, Ref, Tag, TirSink, local_flags,
};
use hd_types::{Ty, TyData};

use crate::body::{Ck, ExpectPosition, LetMutKind, unsupported};
use crate::expr::Indexed;

/// Where a place starts (`docs/compiler/tir-v2.md`, "Places").
#[derive(Clone, Copy)]
pub(crate) enum Root {
    /// A local, with its scope depth.
    Local(LocalId, usize),
    /// A top-level binding: its name, id and type.
    Global(Symbol, DefId, Ty),
    /// The place the reference value `r` refers to, `[*%r]`, with `r`'s
    /// type.
    Deref(Ref, Ty),
    /// A field of the data value `o`, `[*%o .k]`: the field's index, the
    /// field's type, and the data value's type.
    DataField(Ref, u32, Ty, Ty),
    /// A slot of a list's buffer (`ref.elem.place`): the list as the
    /// source read it (for the writable check), the local holding the
    /// pinned list and its depth, the index value, and the list's value
    /// type. The place is `[$pinned [%i]]`; on the ownership passes' path,
    /// where insertion pins the list (`docs/compiler/tir-v2.md`, stage 2
    /// commit 12b), there is no pinned local and it is `[%xs [%i]]`.
    Elem(Ref, Option<(LocalId, usize)>, Ref, Ty),
    /// A list element that a write names, `xs[i]`, which no reference
    /// pins: the list value, the index value, the list's type, and the
    /// index expression.
    Slot(Ref, Ref, Ty, NodeIdx),
    /// A value of a map, which is no place (`types.coll.map-value`): the
    /// value read, and the index expression. Only a read reaches it; a
    /// write through it is reported.
    MapValue(Ref, NodeIdx),
}

/// What a place is built for ([`Ck::place_of`]): a reference to it (`&`,
/// `&mut`, an automatic address), which pins a list element, or a write
/// into it, which names the element where it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceUse {
    Ref,
    Write,
}

/// A place a reference refers to (`ref.place.forms`): its root, the
/// struct fields read from it in order (each with the type it reads), and
/// the place's type.
pub(crate) struct Place {
    pub(crate) root: Root,
    pub(crate) path: Vec<(u32, Ty)>,
    pub(crate) ty: Ty,
}

/// The operand of `&` or `&mut`: a place, or another value, already
/// evaluated.
pub(crate) enum Operand {
    Place(Place),
    Value(Ref, Ty),
}

impl Ck<'_, '_> {
    /// `(T, mutable)` when `t` is a reference type `&T` or `&mut T`.
    pub(crate) fn ref_of(&self, t: Ty) -> Option<(Ty, bool)> {
        let pool = self.pool();
        match pool.get(self.infer.resolve(pool, t)) {
            TyData::Ref { inner, mutable } => Some((inner, mutable)),
            TyData::Mut(i) => self.ref_of(i),
            _ => None,
        }
    }

    /// A read keeps its declared type; `&mut` also grants legacy view access.
    pub(crate) fn deref_ty(&self, inner: Ty, mutable: bool) -> Ty {
        let pool = self.pool();
        let inner = self.infer.resolve(pool, inner);
        if mutable && self.is_composite(inner) && !matches!(pool.get(inner), TyData::Mut(_)) {
            pool.intern_ty(&TyData::Mut(inner))
        } else {
            inner
        }
    }

    /// `ref.convert.readonly-covariant`, `ref.convert.readonly-option`:
    /// representation-preserving permission weakening through a readonly
    /// container view. A mutable reference and an invariant argument keep
    /// their exact operands. The caller rolls back a failed inference trial.
    pub(crate) fn readonly_view_fits(&mut self, from: Ty, to: Ty, depth: u32) -> bool {
        if depth > 32 {
            return false;
        }
        let pool = self.pool();
        let from = self.infer.resolve(pool, from);
        let to = self.infer.resolve(pool, to);
        if from == to {
            return true;
        }
        let d = depth + 1;
        match (pool.get(from), pool.get(to)) {
            (
                TyData::Ref { inner: a, .. },
                TyData::Ref {
                    inner: b,
                    mutable: false,
                },
            ) => self.readonly_view_fits(a, b, d),
            (TyData::Option(a), TyData::Option(b)) => self.readonly_view_fits(a, b, d),
            (
                TyData::Adt { def, args: a },
                TyData::Adt {
                    def: other,
                    args: b,
                },
            ) if def == other => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                let variance = self.declared_variances(def);
                a.len() == b.len()
                    && a.iter().zip(b).enumerate().all(|(i, (&a, &b))| {
                        if variance.get(i) == Some(&1) {
                            self.readonly_view_fits(a, b, d)
                        } else {
                            self.infer.unify(pool, a, b).is_ok() && self.perm_fits(a, b, false, d)
                        }
                    })
            }
            (TyData::Tuple { elems: a, rest: ar }, TyData::Tuple { elems: b, rest: br }) => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|(&a, &b)| self.readonly_view_fits(a, b, d))
                    && match (ar, br) {
                        (Some(a), Some(b)) => self.readonly_view_fits(a, b, d),
                        (None, None) => true,
                        _ => false,
                    }
            }
            // Function inputs reverse the conversion; results preserve it.
            // Nested rows stay invariant (`types.variance.function`). A
            // top-level function value fits its row after this trial.
            (
                TyData::Fn {
                    params: a,
                    result: ar,
                    row: aw,
                    suspends: ac,
                    inputs: ai,
                },
                TyData::Fn {
                    params: b,
                    result: br,
                    row: bw,
                    suspends: bc,
                    inputs: bi,
                },
            ) if ac == bc && ai == bi => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                a.len() == b.len()
                    && (depth == 0 || self.rows_equal(aw, bw))
                    && a.iter()
                        .zip(b)
                        .all(|(&a, &b)| self.variance_view_fits(b, a, d))
                    && self.variance_view_fits(ar, br, d)
            }
            _ => self.infer.unify(pool, from, to).is_ok() && self.perm_fits(from, to, false, d),
        }
    }

    /// Representation-preserving variance inside a function signature.
    /// Unlike a readonly reference view, an ordinary optional or invariant
    /// argument cannot weaken permissions (`types.option.invariant`).
    pub(crate) fn variance_view_fits(&mut self, from: Ty, to: Ty, depth: u32) -> bool {
        if depth > 32 {
            return false;
        }
        let pool = self.pool();
        let from = self.infer.resolve(pool, from);
        let to = self.infer.resolve(pool, to);
        if from == to {
            return true;
        }
        let d = depth + 1;
        match (pool.get(from), pool.get(to)) {
            (TyData::Fn { .. }, TyData::Fn { .. })
            | (TyData::Ref { .. }, TyData::Ref { mutable: false, .. }) => {
                self.readonly_view_fits(from, to, depth)
            }
            (
                TyData::Adt { def, args: a },
                TyData::Adt {
                    def: other,
                    args: b,
                },
            ) if def == other => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                let variance = self.declared_variances(def);
                a.len() == b.len()
                    && a.iter().zip(b).enumerate().all(|(i, (&a, &b))| {
                        match variance.get(i).copied().unwrap_or(0) {
                            1 => self.variance_view_fits(a, b, d),
                            -1 => self.variance_view_fits(b, a, d),
                            _ => {
                                self.infer.unify(pool, a, b).is_ok()
                                    && self.perm_fits(a, b, false, d)
                            }
                        }
                    })
            }
            (TyData::Tuple { elems: a, rest: ar }, TyData::Tuple { elems: b, rest: br }) => {
                let (a, b) = (pool.list_items(a), pool.list_items(b));
                a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|(&a, &b)| self.variance_view_fits(a, b, d))
                    && match (ar, br) {
                        (Some(a), Some(b)) => self.variance_view_fits(a, b, d),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => {
                self.infer.unify(pool, from, to).is_ok()
                    && self.perm_fits(from, to, !matches!(pool.get(from), TyData::Option(_)), d)
            }
        }
    }

    /// What the value `v` reads when it is a `Read` through a reference
    /// value, `[*%r .f .g]`: the reference `%r`, whether it is `&mut`, and
    /// the struct fields the read follows from the referent.
    pub(crate) fn read_through(&mut self, v: Ref) -> Option<(Ref, bool, Vec<u32>)> {
        let i = v.as_inst()?;
        let body = self.b.body_mut();
        if body.tags[i.0 as usize] != Tag::Read {
            return None;
        }
        let p = body.decode_place(body.data[i.0 as usize][0])?;
        let (PlaceRoot::Value(r), [Proj::Deref, fields @ ..]) = (p.root, p.proj.as_slice()) else {
            return None;
        };
        let path = fields
            .iter()
            .map(|f| match f {
                Proj::Field(k) => Some(*k),
                _ => None,
            })
            .collect::<Option<Vec<u32>>>()?;
        let (_, m) = self.ref_of(self.b.ty_of(r))?;
        Some((r, m, path))
    }

    /// Reads through the reference `r` of type `rt` (`*r`, or an automatic
    /// dereference): `Read [*%r]`. A field step on it extends the place
    /// ([`Ck::field_read`]).
    pub(crate) fn emit_deref(&mut self, r: Ref, rt: Ty, at: NodeIdx) -> (Ref, Ty) {
        let Some((inner, mutable)) = self.ref_of(rt) else {
            return (r, rt);
        };
        let t = self.deref_ty(inner, mutable);
        if r.0 == NONE {
            return (self.b.emit(Tag::Poison, NONE, NONE, t, at), t);
        }
        let word = self.b.place(&TirPlace {
            root: PlaceRoot::Value(r),
            proj: vec![Proj::Deref],
        });
        (self.b.emit(Tag::Read, word, ReadMode::Use as u32, t, at), t)
    }

    /// Field `idx`, of type `ft`, of the struct value `base`. When `base`
    /// is a `Read` through a reference that nothing uses yet (the last
    /// instruction), the field extends its place, so `p.x` through `&Point`
    /// is one `Read [*%r .x]` that loads only `x`; else a read of the
    /// field ([`Ck::field_value`]).
    pub(crate) fn field_read(&mut self, base: Ref, idx: u32, ft: Ty, at: NodeIdx) -> Ref {
        let last = base
            .as_inst()
            .is_some_and(|i| i.0 as usize + 1 == self.b.body_mut().len());
        if last
            && self.is_struct_ty(self.b.ty_of(base))
            && let Some((r, _, mut path)) = self.read_through(base)
        {
            path.push(idx);
            let mut proj = vec![Proj::Deref];
            proj.extend(path.into_iter().map(Proj::Field));
            let i = base.0 as usize;
            // The old place's record, made just before the read, goes
            // with it.
            let body = self.b.body_mut();
            let old = body.data[i][0] as usize;
            if body
                .extra
                .get(old)
                .is_some_and(|&n| old + 1 + n as usize == body.extra.len())
            {
                body.extra.truncate(old);
            }
            let word = self.b.place(&TirPlace {
                root: PlaceRoot::Value(r),
                proj,
            });
            let body = self.b.body_mut();
            body.data[i][0] = word;
            body.ty[i] = ft;
            return base;
        }
        self.field_value(base, idx, ft, at)
    }

    /// The value `r` of type `t` as an index base (`ref.read.index.declared`):
    /// a reference to a list, map or string is read through, so the index
    /// names an element of the referenced value; any other value is
    /// returned unchanged.
    pub(crate) fn deref_index_base(&mut self, r: Ref, t: Ty, at: NodeIdx) -> (Ref, Ty) {
        let Some((inner, _)) = self.ref_of(t) else {
            return (r, t);
        };
        if self.index_kind(inner).is_none() {
            return (r, t);
        }
        self.emit_deref(r, t, at)
    }

    /// Replaces the value the reference `r` of type `rt` refers to with
    /// `v` (`*r = v`): `Store [*%r] v`.
    pub(crate) fn emit_ref_store(&mut self, r: Ref, rt: Ty, v: Ref, at: NodeIdx) {
        let Some((inner, _)) = self.ref_of(rt) else {
            return;
        };
        let v = self.retype(v, inner, at);
        self.store_through(r, &[], v, at);
    }

    /// `Store [*%r .f ..] v`: replaces the value at the struct field path
    /// `fields` in what the reference `r` refers to (all of it for no
    /// path). A poisoned reference stores nothing.
    fn store_through(&mut self, r: Ref, fields: &[u32], v: Ref, at: NodeIdx) {
        if r.0 == NONE {
            return;
        }
        let mut proj = vec![Proj::Deref];
        proj.extend(fields.iter().map(|k| Proj::Field(*k)));
        let word = self.b.place(&TirPlace {
            root: PlaceRoot::Value(r),
            proj,
        });
        self.b.emit(Tag::Store, word, v.0, Ty::VOID, at);
    }

    /// `v` with the type `to`, which differs from `v`'s at most in views
    /// and reference permissions, so the change is a static one.
    pub(crate) fn retype(&mut self, v: Ref, to: Ty, at: NodeIdx) -> Ref {
        let from = self.b.ty_of(v);
        if v.0 == NONE || from == to || matches!(from, Ty::NEVER | Ty::POISON) {
            return v;
        }
        self.b.coerce(Coercion::Weaken, NONE, v, to, at)
    }

    /// `&e` and `&mut e` (`ref.expr.place`, `ref.expr.allocate`).
    pub(crate) fn ref_expr(&mut self, n: NodeRef<'_>, want: Option<Ty>) -> StageResult<(Ref, Ty)> {
        let pool = self.pool();
        let mutable = n
            .direct_token(&self.cx.src.parse.tokens, TokenKind::KwMut)
            .is_some();
        let Some(e) = n.children().next() else {
            return unsupported("a reference expression without an operand");
        };
        // A trait value reference converts the whole reference
        // (`trait.dyn.ref.convert`), so its trait value type is no expected
        // type for the operand.
        let want_inner = want
            .filter(|w| self.dyn_ref_of(*w).is_none())
            .and_then(|w| self.ref_of(w))
            .map(|(i, _)| i);
        // An element reference that initializes a simple binding belongs
        // to it, and lasts until its last use (`ref.elem.release`).
        let owner = self
            .pin_owner
            .filter(|(node, _)| *node == n.index())
            .map(|(_, s)| s);
        if owner.is_some() {
            self.pin_owner = None;
        }
        let saved = std::mem::replace(&mut self.pin_next_owner, owner);
        let op = self.place_of(e, want_inner, PlaceUse::Ref);
        self.pin_next_owner = saved;
        let Some(op) = op? else {
            return Ok(self.ref_poison(n));
        };
        let place = match op {
            Operand::Place(p) => {
                if mutable && !self.place_writable(&p, e) {
                    return Ok(self.ref_poison(n));
                }
                if mutable && let Root::Local(l, _) = p.root {
                    self.ref_taken.push(l);
                }
                p
            }
            Operand::Value(v, t) => {
                if matches!(self.infer.resolve(pool, t), Ty::POISON | Ty::NEVER) || v.0 == NONE {
                    return Ok(self.ref_poison(n));
                }
                // A new object holds a copy of the value (`ref.expr.allocate`);
                // `&mut` on a composite value without mutable access, such as
                // a readonly `Suspend[T]`, would upgrade it
                // (`types.mut.no-upgrade`).
                // A list or map value is no view: the new object holds a
                // copy of it (`ref.place.writable.collection`).
                let coll = self.is_coll_value(t);
                if mutable && !coll && self.is_composite(t) && !self.has_mut_access(t) {
                    let msg = format!(
                        "`&mut` needs mutable access, but the value is a readonly {}",
                        self.show(t)
                    );
                    self.err(Code::MutableUpgrade, e, &msg);
                    return Ok(self.ref_poison(n));
                }
                let v = if coll {
                    self.coll_copy(v, t, e.index())
                } else {
                    v
                };
                let (l, d) = self.hold(v, t, e);
                // The closures share the new local (a cell); the `set`
                // closure assigns it.
                self.add_flags(l, local_flags::CAPTURED_ASSIGNED);
                Place {
                    root: Root::Local(l, d),
                    path: vec![],
                    ty: t,
                }
            }
        };
        let inner = self.strip_mut(self.infer.resolve(pool, place.ty));
        let rt = pool.intern_ty(&TyData::Ref { inner, mutable });
        let r = self.make_ref(&place, mutable, rt, n);
        Ok((r, rt))
    }

    /// Writes a field path of an already evaluated reference `r`:
    /// `Store [*%r .f ..] v`. The RHS is already evaluated too; only this
    /// field is replaced.
    pub(crate) fn store_ref_field(&mut self, r: Ref, path: &[(u32, Ty)], v: Ref, n: NodeRef<'_>) {
        let ty = path.last().expect("a field path").1;
        let inner = self.strip_mut(self.infer.resolve(self.pool(), ty));
        let v = self.retype(v, inner, n.index());
        let fields: Vec<u32> = path.iter().map(|(k, _)| *k).collect();
        self.store_through(r, &fields, v, n.index());
    }

    /// A new object holding `v` of type `t`, as `&e` or `&mut e` on a value
    /// that is no place makes one (`ref.expr.allocate`).
    pub(crate) fn new_object_ref(
        &mut self,
        v: Ref,
        t: Ty,
        mutable: bool,
        n: NodeRef<'_>,
    ) -> (Ref, Ty) {
        let (l, d) = self.hold(v, t, n);
        self.add_flags(l, local_flags::CAPTURED_ASSIGNED);
        let place = Place {
            root: Root::Local(l, d),
            path: vec![],
            ty: t,
        };
        let pool = self.pool();
        let inner = self.strip_mut(t);
        let rt = pool.intern_ty(&TyData::Ref { inner, mutable });
        (self.make_ref(&place, mutable, rt, n), rt)
    }

    fn ref_poison(&mut self, n: NodeRef<'_>) -> (Ref, Ty) {
        (
            self.b.emit(Tag::Poison, NONE, NONE, Ty::POISON, n.index()),
            Ty::POISON,
        )
    }

    /// Binds `v` of type `t` to a new local the body cannot name; returns
    /// it with its scope depth.
    pub(crate) fn hold(&mut self, v: Ref, t: Ty, n: NodeRef<'_>) -> (LocalId, usize) {
        let sym = self.cx.names.syms.intern("$ref");
        let l = self.b.local(t, sym, local_flags::ASSIGNED, n.index());
        self.b.set(l, v, n.index());
        (l, self.scopes.len() - 1)
    }

    /// The place `e` names (`ref.place.forms`), as `use_` needs it: the
    /// operand of `&` or `&mut`, or the target of a write; any other
    /// expression, evaluated. A place's root is evaluated once: a local or
    /// a top-level binding is not read, and a reference, a data value, a
    /// list or an index is a value. `None` after an error.
    pub(crate) fn place_of(
        &mut self,
        e: NodeRef<'_>,
        want: Option<Ty>,
        use_: PlaceUse,
    ) -> StageResult<Option<Operand>> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        match e.kind() {
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [inner] => self.place_of(*inner, want, use_),
                _ => unsupported("a parenthesized expression shape"),
            },
            SyntaxKind::NameExpr => {
                let s = self.sym_of(e);
                if let Some((l, d)) = self.find_local(s)
                    && !self.local_generics.contains_key(&l)
                {
                    return Ok(Some(Operand::Place(Place {
                        root: Root::Local(l, d),
                        path: vec![],
                        ty: self.b.local_ty(l),
                    })));
                }
                if self.find_local(s).is_none()
                    && let Some(g) = self.global_binding(s)
                {
                    // Reports what a read reports (initialization order); a
                    // write reads no value.
                    if use_ == PlaceUse::Write {
                        if matches!(self.global_read(s, e), crate::init::GlobalRead::Error) {
                            let v = self.poison_value(e);
                            return Ok(Some(Operand::Value(v.0, v.1)));
                        }
                    } else if let Some((r, _)) = self.global_get(s, e)
                        && self.b.ty_of(r) == Ty::POISON
                    {
                        return Ok(None);
                    }
                    return Ok(Some(Operand::Place(Place {
                        root: Root::Global(s, g.def, g.ty),
                        path: vec![],
                        ty: g.ty,
                    })));
                }
                let (r, t) = self.expr(e, want)?;
                Ok(Some(Operand::Value(r, t)))
            }
            SyntaxKind::FieldExpr => {
                let Some(base) = kids.first().copied() else {
                    return unsupported("a field without a base");
                };
                let name = self.cx.src.text(self.cx.src.last(e)).to_owned();
                let global = base.kind() == SyntaxKind::NameExpr
                    && self.find_local(self.sym_of(base)).is_none()
                    && self.global_binding(self.sym_of(base)).is_some();
                if (base.kind() == SyntaxKind::NameExpr
                    && self.find_local(self.sym_of(base)).is_none()
                    && !global
                    || base.kind() == SyntaxKind::FieldExpr)
                    && let Some(v) = self.static_member_value(base, &name, want, e)?
                {
                    return Ok(Some(Operand::Value(v.0, v.1)));
                }
                let Some(b) = self.place_of(base, None, use_)? else {
                    return Ok(None);
                };
                self.field_step(b, &name, (base, e), use_)
            }
            SyntaxKind::DerefExpr => {
                let Some(inner) = kids.first().copied() else {
                    return unsupported("a dereference without an operand");
                };
                let (r, t) = self.expr(inner, None)?;
                let Some((it, m)) = self.ref_of(t) else {
                    self.not_a_reference(t, inner);
                    return Ok(None);
                };
                Ok(Some(Operand::Place(Place {
                    root: Root::Deref(r, t),
                    path: vec![],
                    ty: self.deref_ty(it, m),
                })))
            }
            // A list element names a slot of the list's buffer, which a
            // reference pins (`ref.elem.place`).
            SyntaxKind::IndexExpr if use_ == PlaceUse::Write => self.slot_operand(e),
            SyntaxKind::IndexExpr => {
                let owner = self.pin_next_owner.take();
                self.elem_operand(e, owner)
            }
            _ => {
                let (r, t) = self.expr(e, want)?;
                Ok(Some(Operand::Value(r, t)))
            }
        }
    }

    /// The field `name` of the operand `b`: a struct field extends a
    /// place; a field of a referenced value or of a data value starts a
    /// place there; a field of any other value is a value.
    /// The place of the embedded part that `part`, a path of embedded
    /// fields, reaches from `op`, a struct place or a reference to one
    /// (`names.promoted.path`). Any other operand is returned as it is.
    pub(crate) fn part_place(
        &mut self,
        op: Operand,
        part: Vec<(u32, Ty)>,
        e: NodeRef<'_>,
    ) -> Operand {
        let Some(&(_, ty)) = part.last() else {
            return op;
        };
        let op = match op {
            Operand::Place(p) if self.ref_of(p.ty).is_some() => {
                let (r, t) = self.read_place(&p, e);
                Operand::Value(r, t)
            }
            op => op,
        };
        match op {
            Operand::Place(mut p) if self.is_struct_ty(p.ty) => {
                p.path.extend(part);
                p.ty = ty;
                Operand::Place(p)
            }
            Operand::Value(r, t) if self.ref_of(t).is_some() => Operand::Place(Place {
                root: Root::Deref(r, t),
                path: part,
                ty,
            }),
            op => op,
        }
    }

    /// The place `p` of a struct, extended to the embedded part that
    /// promotes the field `name` when the struct has no field `name`
    /// itself (`names.promoted.path`). A part reached through a value that
    /// is no struct is not followed.
    pub(crate) fn promote_place(&mut self, mut p: Place, name: &str) -> Place {
        if self.field_of(p.ty, name).is_some() {
            return p;
        }
        let Some(path) = self.promoted_field_path(p.ty, name) else {
            return p;
        };
        let saved = (p.path.len(), p.ty);
        for step in &path {
            let Some((idx, ft)) = self.field_of(p.ty, step) else {
                break;
            };
            if !self.is_struct_ty(p.ty) {
                p.path.truncate(saved.0);
                p.ty = saved.1;
                return p;
            }
            let ft = self.field_access(p.ty, step, ft);
            p.path.push((idx, ft));
            p.ty = ft;
        }
        p
    }

    fn field_step(
        &mut self,
        b: Operand,
        name: &str,
        (base, e): (NodeRef<'_>, NodeRef<'_>),
        use_: PlaceUse,
    ) -> StageResult<Option<Operand>> {
        let b = match b {
            Operand::Place(p) if !self.is_struct_ty(p.ty) => {
                let (r, t) = self.read_place(&p, e);
                Operand::Value(r, t)
            }
            other => other,
        };
        match b {
            Operand::Place(mut p) => {
                if self.field_of(p.ty, name).is_none()
                    && let Some(path) = self.promoted_field_path(p.ty, name)
                {
                    // A promoted field is its explicit path
                    // (`names.promoted.path`).
                    for step in &path {
                        let Some((idx, ft)) = self.field_of(p.ty, step) else {
                            break;
                        };
                        p.path.push((idx, ft));
                        p.ty = ft;
                    }
                }
                let Some((idx, ft)) = self.field_of(p.ty, name) else {
                    self.no_field(p.ty, name, e);
                    return Ok(None);
                };
                self.check_field_visible(p.ty, name, e);
                p.path.push((idx, ft));
                p.ty = ft;
                Ok(Some(Operand::Place(p)))
            }
            Operand::Value(r, t) => {
                if matches!(self.infer.resolve(self.pool(), t), Ty::POISON | Ty::NEVER) {
                    return Ok(None);
                }
                if let Some((inner, m)) = self.ref_of(t) {
                    let p = Place {
                        root: Root::Deref(r, t),
                        path: vec![],
                        ty: self.deref_ty(inner, m),
                    };
                    return self.field_step(Operand::Place(p), name, (base, e), use_);
                }
                let pool = self.pool();
                let inner = self.strip_mut(self.infer.resolve(pool, t));
                // A write reads any other field as an expression does; only
                // a data value's field is a place it names.
                if use_ == PlaceUse::Write
                    && !(matches!(pool.get(inner), TyData::Adt { .. })
                        && !self.is_struct_ty(t)
                        && self.field_of(t, name).is_some())
                {
                    let (v, vt) = self.field_of_value((r, t), name, (base, e))?;
                    return Ok(Some(Operand::Value(v, vt)));
                }
                if let TyData::Tuple { .. } = pool.get(inner) {
                    // `ref.check.not-yet-non-list-elements`
                    self.err(
                        Code::ReferencesNotYetChecked,
                        e,
                        "a reference to a tuple element is not checked yet",
                    );
                    return Ok(None);
                }
                let (r, t) = self.promote_base(r, t, name, e);
                let Some((idx, ft)) = self.field_of(t, name) else {
                    // A shared enum field is no place, so a reference to it
                    // holds a copy (`expr.place.enum-shared-field`).
                    let inner = self.strip_mut(self.infer.resolve(pool, t));
                    if let Some((v, vt)) = self.shared_field(r, inner, name, e) {
                        return Ok(Some(Operand::Value(v, vt)));
                    }
                    self.no_field(t, name, e);
                    return Ok(None);
                };
                self.check_field_visible(t, name, e);
                if self.is_struct_ty(t) {
                    // A field of a struct that is no place: a value.
                    let v = self.b.field(r, idx, ft, e.index());
                    return Ok(Some(Operand::Value(v, ft)));
                }
                let ft = self.field_access(t, name, ft);
                Ok(Some(Operand::Place(Place {
                    root: Root::DataField(r, idx, ft, t),
                    path: vec![],
                    ty: ft,
                })))
            }
        }
    }

    /// The element `e`, `xs[k]`, that a write names: a slot of the list
    /// `xs` evaluates to, at the evaluated key; a value of a map, which is
    /// no place; else the value of the index expression.
    fn slot_operand(&mut self, e: NodeRef<'_>) -> StageResult<Option<Operand>> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        let [base, key] = kids.as_slice() else {
            return unsupported("an index shape");
        };
        let (br, bt) = self.expr(*base, None)?;
        if br.0 == NONE && matches!(bt, Ty::NEVER | Ty::POISON) {
            return Ok(Some(Operand::Value(br, bt)));
        }
        let (br, bt) = self.deref_index_base(br, bt, base.index());
        let builtin = self.index_kind(bt);
        let (kr, kt) = self.index_key_value(*key, builtin)?;
        match builtin {
            Some((get @ (Indexed::List | Indexed::Map), bkt, vt))
                if get == Indexed::Map || !self.is_range_ty(kt) =>
            {
                let kr = self.index_key((kr, kt), *key, bkt);
                let root = if get == Indexed::Map {
                    let v = self.index_read(get, (br, kr), vt, e.index());
                    Root::MapValue(v, e.index())
                } else {
                    Root::Slot(br, kr, bt, e.index())
                };
                Ok(Some(Operand::Place(Place {
                    root,
                    path: vec![],
                    ty: vt,
                })))
            }
            _ => {
                let (v, vt) = self.index_of_value((br, bt), builtin, (kr, kt), (*key, e))?;
                Ok(Some(Operand::Value(v, vt)))
            }
        }
    }

    fn no_field(&mut self, t: Ty, name: &str, e: NodeRef<'_>) {
        if matches!(self.infer.resolve(self.pool(), t), Ty::POISON | Ty::NEVER) {
            return;
        }
        let msg = format!("no field `{name}` on {}", self.show(t));
        self.err(Code::UnknownField, e, &msg);
    }

    fn not_a_reference(&mut self, t: Ty, n: NodeRef<'_>) {
        if matches!(self.infer.resolve(self.pool(), t), Ty::POISON | Ty::NEVER) {
            return;
        }
        let msg = format!(
            "`*` reads through a reference, but this is a {}",
            self.show(t)
        );
        self.err(Code::TypeMismatch, n, &msg);
    }

    /// A top-level binding named `s`: its own module's, else an earlier
    /// REPL input's.
    pub(crate) fn global_binding(&self, s: Symbol) -> Option<crate::init::InputBinding> {
        let init = self.cx.init.borrow();
        init.globals
            .get(&s)
            .map(crate::init::Global::binding)
            .or_else(|| init.earlier.get(&s).copied())
    }

    /// Whether `&mut` may refer to the place `p` (`ref.place.writable`);
    /// reports why not otherwise.
    pub(crate) fn place_writable(&mut self, p: &Place, at: NodeRef<'_>) -> bool {
        let ok = match p.root {
            Root::Local(l, _) => self.local_ref_writable(l, at),
            Root::Global(s, _, _) => {
                let g = self.global_binding(s);
                if g.is_some_and(|g| g.mutable) {
                    true
                } else {
                    let name = self.cx.names.text(s).to_owned();
                    let short = g.is_some_and(|g| g.short);
                    self.ref_not_writable(&name, short, false, None, at);
                    false
                }
            }
            Root::Deref(_, rt) => {
                if self.ref_of(rt).is_some_and(|(_, m)| m) {
                    true
                } else {
                    let msg = format!(
                        "`&mut` refers through a read-only reference {}; take the reference with `&mut` to write through it",
                        self.show(rt)
                    );
                    self.err(Code::NotWritable, at, &msg);
                    false
                }
            }
            // `ref.elem.writable`: as an element store needs.
            Root::Elem(list, _, _, lt) | Root::Slot(list, _, lt, _) => {
                self.coll_change(list, lt, "a `&mut` element reference", at)
            }
            // A reference names no map value (`elem_operand`).
            Root::MapValue(..) => false,
            Root::DataField(_, _, _, dt) => {
                if self.has_mut_access(dt) {
                    true
                } else {
                    let msg = format!(
                        "`&mut` refers to a field of a readonly {}, which grants no writes",
                        self.show(dt)
                    );
                    self.err(Code::NotWritable, at, &msg);
                    false
                }
            }
        };
        if !ok {
            return false;
        }
        // A composite value without mutable access in the place, such as a
        // readonly `Suspend[T]`, grants no writes, so `&mut` to it would
        // upgrade it (`types.mut.no-upgrade`). A collection value is
        // no view: a writable place holding one is writable
        // (`types.coll.writable`).
        let coll_value = self.is_coll_value(p.ty)
            && (!matches!(p.root, Root::DataField(..)) || !p.path.is_empty());
        if self.is_composite(p.ty) && !self.has_mut_access(p.ty) && !coll_value {
            let msg = format!(
                "`&mut` needs mutable access, but this place holds a readonly {}",
                self.show(p.ty)
            );
            self.err(Code::MutableUpgrade, at, &msg);
            return false;
        }
        true
    }

    /// Whether `&mut` may refer to the local `l`: a `let mut` binding or a
    /// `mut` parameter (`ref.place.writable`). Reports why not otherwise.
    fn local_ref_writable(&mut self, l: LocalId, at: NodeRef<'_>) -> bool {
        if self.mut_locals.contains(&l) {
            return true;
        }
        let (name, flags, syn) = {
            let body = self.b.body_mut();
            (
                body.local_name[l.idx()],
                body.local_flags[l.idx()],
                body.local_syn[l.idx()],
            )
        };
        let name = self.cx.names.text(name).to_owned();
        let short = flags & local_flags::SHORT != 0 || self.using_locals.contains(&l);
        self.ref_not_writable(&name, short, flags & local_flags::PARAM != 0, Some(syn), at);
        false
    }

    /// Reports `&mut` of the binding `name`, which is not writable
    /// (`ref.place.not-writable.*`), with the fix-it that marks the
    /// binding when the binding's declaration `decl` takes one.
    fn ref_not_writable(
        &mut self,
        name: &str,
        short: bool,
        param: bool,
        decl: Option<NodeIdx>,
        at: NodeRef<'_>,
    ) {
        let (code, msg) = if param {
            let msg = if name == "self" {
                "`&mut self` needs a writable receiver, but `self` is the method's own copy, which never changes".to_owned()
            } else {
                format!(
                    "`&mut` needs a writable place, but parameter `{name}` is the function's own copy, which never changes; write `mut {name}` to change the copy"
                )
            };
            (Code::NonReassignableParameterBinding, msg)
        } else if short {
            (
                Code::NonReassignableBinding,
                format!(
                    "`&mut` needs a writable place, but `{name}` never changes; bind it with `let mut`"
                ),
            )
        } else {
            (
                Code::NotWritable,
                format!(
                    "`&mut` needs a writable place, but `{name}` is not `let mut`; bind it with `let mut`"
                ),
            )
        };
        self.err(code, at, &msg);
        // The fix-it marks the binding: `let x` becomes `let mut x`, and a
        // parameter `x: T` becomes `mut x: T`. A `:=` binding is left to
        // the author, who may want a copy instead.
        let Some(decl) = decl.filter(|d| *d != NodeIdx::NONE && !short) else {
            return;
        };
        let node = self.cx.src.parse.tree.node(decl);
        let kind_ok = match node.kind() {
            SyntaxKind::BindingPattern => true,
            SyntaxKind::Parameter => name != "self",
            _ => false,
        };
        if !kind_ok {
            return;
        }
        let Some(tok) = self.cx.src.tokens(node).find(|t| {
            matches!(
                self.cx.src.tkind(*t),
                Some(TokenKind::Ident | TokenKind::RawIdent)
            )
        }) else {
            return;
        };
        let lo = self.cx.src.token_span(tok).lo;
        let span = hd_base::Span {
            file: self.cx.src.file,
            lo,
            hi: lo,
        };
        let title = format!("mark `{name}` with `mut`");
        self.diags
            .add_fix(&title, FixSafety::Suggestion, &[(span, "mut ")]);
    }

    /// Reads the place `p`: its root, then each struct field on its path.
    /// Inside a closure the reads capture what they name.
    pub(crate) fn read_place(&mut self, p: &Place, n: NodeRef<'_>) -> (Ref, Ty) {
        let (mut v, mut t) = self.read_root(p.root, n);
        for &(idx, ft) in &p.path {
            v = self.field_read(v, idx, ft, n.index());
            t = ft;
        }
        (v, t)
    }

    fn read_root(&mut self, root: Root, n: NodeRef<'_>) -> (Ref, Ty) {
        match root {
            Root::Local(l, d) => self.read_local(l, d, n),
            Root::Global(s, def, ty) => self
                .global_get(s, n)
                .unwrap_or_else(|| (self.b.read_global(def, ty, n.index()), ty)),
            Root::Deref(r, rt) => self.emit_deref(r, rt, n.index()),
            Root::DataField(o, idx, ft, _) => (self.b.data_field(o, idx, ft, n.index()), ft),
            Root::Slot(list, index, lt, _) => {
                let et = self.elem_ty(lt);
                (self.b.element(list, index, et, n.index()), et)
            }
            Root::MapValue(v, _) => (v, self.b.ty_of(v)),
            Root::Elem(list, pinned, i, lt) => {
                let et = self.elem_ty(lt);
                let l = match pinned {
                    Some((buf, bd)) => self.read_local(buf, bd, n).0,
                    None => list,
                };
                (self.b.element(l, i, et, n.index()), et)
            }
        }
    }

    /// The element type of the list type `lt`.
    fn elem_ty(&mut self, lt: Ty) -> Ty {
        self.index_kind(lt).map_or(Ty::POISON, |(.., vt)| vt)
    }

    /// The reference to the place `p`: a `Ref` of the place
    /// (`docs/compiler/tir-v2.md`, "`&` and `&mut`"), the address the
    /// backend makes of it. An element's index, which a local holds, is
    /// read for the place's element projection.
    pub(crate) fn make_ref(&mut self, p: &Place, mutable: bool, rt: Ty, n: NodeRef<'_>) -> Ref {
        self.ref_reads(p, mutable, n);
        let mut proj = Vec::with_capacity(p.path.len() + 2);
        let root = match p.root {
            Root::Local(l, _) => PlaceRoot::Local(l),
            Root::Global(_, def, ty) => PlaceRoot::Global(def, ty),
            Root::Deref(r, _) => {
                proj.push(Proj::Deref);
                PlaceRoot::Value(r)
            }
            Root::DataField(o, idx, _, _) => {
                proj.extend([Proj::Deref, Proj::Field(idx)]);
                PlaceRoot::Value(o)
            }
            // A write's places; no reference takes them.
            Root::Slot(list, index, ..) => {
                proj.push(Proj::Elem(index));
                PlaceRoot::Value(list)
            }
            Root::MapValue(v, _) => PlaceRoot::Value(v),
            Root::Elem(list, pinned, i, _) => {
                proj.push(Proj::Elem(i));
                match pinned {
                    Some((buf, _)) => PlaceRoot::Local(buf),
                    None => PlaceRoot::Value(list),
                }
            }
        };
        proj.extend(p.path.iter().map(|(idx, _)| Proj::Field(*idx)));
        let word = self.b.place(&TirPlace { root, proj });
        self.b
            .emit(Tag::Ref, word, u32::from(mutable), rt, n.index())
    }

    /// The reads and writes of the locals and top-level binding that the
    /// place `p` starts from, as the reference's former closure form made
    /// them (its `get` read the place, a `&mut`'s `set` replaced it): the
    /// checks of a read, a capture into each open closure the reference
    /// is in, and the local flags. The flags stay as that form left them,
    /// `CAPTURED` and, for a `&mut` to a local, `CAPTURED_ASSIGNED`,
    /// since capture modes, collection copies and the backend's loans read
    /// them (stage 2 moves those decisions into the ownership passes).
    fn ref_reads(&mut self, p: &Place, mutable: bool, n: NodeRef<'_>) {
        let read = |ck: &mut Self, l: LocalId, d: usize| {
            if ck.note_read(l, d, n) {
                ck.add_flags(l, local_flags::CAPTURED);
            }
        };
        match p.root {
            Root::Local(l, d) => {
                read(self, l, d);
                if mutable {
                    self.add_flags(
                        l,
                        local_flags::CAPTURED
                            | local_flags::CAPTURED_ASSIGNED
                            | local_flags::ASSIGNED,
                    );
                }
            }
            Root::Global(s, _, _) => {
                let _ = self.global_read(s, n);
            }
            // A value root was read when it was evaluated.
            Root::Deref(..) | Root::DataField(..) | Root::Slot(..) | Root::MapValue(..) => {}
            Root::Elem(_, Some((buf, bd)), ..) => read(self, buf, bd),
            Root::Elem(_, None, ..) => {}
        }
    }

    /// The identity of the reference `r` as a value of std's
    /// `RefIdentity` (`ty`), what std's `identity_of` returns: the backend
    /// makes it from the reference's address.
    pub(crate) fn ref_identity_value(&mut self, r: Ref, ty: Ty, n: NodeRef<'_>) -> Ref {
        self.b.emit(Tag::RefIdentity, r.0, NONE, ty, n.index())
    }

    /// `r is s` where an operand is a reference (`ref.identity.*`): both
    /// must be references to compatible types; the result compares their
    /// identities.
    pub(crate) fn ref_is(
        &mut self,
        (a, at): (Ref, Ty),
        (c, ct): (Ref, Ty),
        n: NodeRef<'_>,
        rn: NodeRef<'_>,
    ) -> StageResult<Ref> {
        let pool = self.pool();
        let syn = n.index();
        match (self.ref_of(at), self.ref_of(ct)) {
            (Some((ai, _)), Some((ci, _))) => {
                let (ai, ci) = (self.strip_mut(ai), self.strip_mut(ci));
                let snap = self.infer.snapshot();
                if self.infer.unify(pool, ai, ci).is_err() {
                    self.infer.rollback(snap);
                    let msg = format!(
                        "`is` compares {} with {}, which refer to different types",
                        self.show(at),
                        self.show(ct)
                    );
                    self.err(Code::IncompatibleIdentityOperands, rn, &msg);
                }
            }
            (Some(_), None) | (None, Some(_)) => {
                let (other, other_ty, ref_ty) = if self.ref_of(at).is_some() {
                    (rn, ct, at)
                } else {
                    (n, at, ct)
                };
                // An operand of type `never` diverged before the
                // comparison, which is then unreachable.
                if self.infer.resolve(pool, other_ty) == Ty::NEVER {
                    return Ok(if self.ref_of(at).is_some() { c } else { a });
                }
                if self.infer.resolve(pool, other_ty) == Ty::POISON {
                    return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::BOOL, syn));
                }
                // `expr.is.value-operand`: a value has no identity at all.
                if !self.has_identity(other_ty)? {
                    let msg = format!(
                        "{} has no identity to compare with `is`",
                        self.show(other_ty)
                    );
                    self.err(Code::IdentityRequiresReferences, other, &msg);
                    return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::BOOL, syn));
                }
                let msg = format!(
                    "`is` compares the reference {} with {}, which is not a reference; `is` on a reference needs a reference on both sides",
                    self.show(ref_ty),
                    self.show(other_ty)
                );
                self.err(Code::IncompatibleIdentityOperands, other, &msg);
                return Ok(self.b.emit(Tag::Poison, NONE, NONE, Ty::BOOL, syn));
            }
            (None, None) => return unsupported("`is` without a reference operand"),
        }
        // The backend compares the identity fields of the two `Ref`s
        // (`crates/hd_wasm_linear/DESIGN.md` §15.2, "References"), so a generic
        // `is` over `T < AnyRef` compares places too.
        Ok(self.b.emit(Tag::Is, a.0, c.0, Ty::BOOL, syn))
    }

    /// `*e` as a value (`ref.deref.read`): the whole referenced value, a
    /// copy.
    pub(crate) fn deref_expr(&mut self, n: NodeRef<'_>) -> StageResult<(Ref, Ty)> {
        let Some(e) = n.children().next() else {
            return unsupported("a dereference without an operand");
        };
        let (r, t) = self.expr(e, None)?;
        if self.dyn_ref_of(t).is_some() {
            self.err(
                Code::TraitUsedAsType,
                n,
                "a trait value referent cannot be read as a bare value",
            );
            return Ok(self.ref_poison(n));
        }
        let Some(_) = self.ref_of(t) else {
            self.not_a_reference(t, e);
            return Ok(self.ref_poison(n));
        };
        let (v, vt) = self.emit_deref(r, t, n.index());
        Ok((v, vt))
    }

    /// `*e = v` and `*e op= v` (`ref.deref.write`): replaces the value the
    /// reference `e` refers to.
    pub(crate) fn deref_assign(
        &mut self,
        lhs: NodeRef<'_>,
        rhs: NodeRef<'_>,
        compound: Option<TokenKind>,
        s: NodeRef<'_>,
    ) -> StageResult<()> {
        let Some(e) = lhs.children().next() else {
            return unsupported("a dereference without an operand");
        };
        let (r, t) = self.expr(e, None)?;
        let Some((inner, mutable)) = self.ref_of(t) else {
            self.not_a_reference(t, e);
            return Ok(());
        };
        if !mutable {
            let msg = format!(
                "this writes through a read-only reference {}; take the reference with `&mut` to write through it",
                self.show(t)
            );
            self.err(Code::NotWritable, lhs, &msg);
            return Ok(());
        }
        // `trait.dyn.ref.no-store`: what a trait value reference refers to
        // has a type it does not know, so no whole value replaces it.
        if self.dyn_ref_of(t).is_some() {
            self.expr(rhs, None)?;
            let msg = format!(
                "this replaces the whole value behind a trait value reference {}, whose type the reference does not know; change it through a `&mut self` method instead",
                self.show(t)
            );
            self.err(Code::TypeMismatch, lhs, &msg);
            return Ok(());
        }
        // A value with views is stored with mutable access, which a read
        // through the reference grants (`ref.read.type`).
        let want = self.deref_ty(inner, true);
        let v = match compound {
            None => {
                let (vr, vt) = self.expr(rhs, Some(want))?;
                let v = self.coerce(vr, vt, want, rhs, "assignment", ExpectPosition::General);
                // The referenced place takes its own copy (`types.coll.value`).
                self.coll_copy(v, inner, rhs.index())
            }
            Some(k) => {
                let (cur, _) = self.emit_deref(r, t, lhs.index());
                let cur = self.retype(cur, inner, lhs.index());
                self.compound(k, cur, inner, rhs, s)?
            }
        };
        self.emit_ref_store(r, t, v, s.index());
        Ok(())
    }

    /// `redundant-let-mut` (`types.bind.let-mut-primitive-unused`): a
    /// `let mut` local of a primitive type, or with a `mut Suspend[T]`
    /// annotation, whose address no
    /// `&mut` takes changes nothing that the binding would not without it.
    pub(crate) fn redundant_primitive_let_mut(&mut self) {
        for (l, pat, kind) in std::mem::take(&mut self.prim_let_mut) {
            if self.ref_taken.contains(&l) {
                continue;
            }
            let node = self.cx.src.parse.tree.node(pat);
            let span = self.cx.src.span(node);
            let msg = match kind {
                LetMutKind::Primitive => {
                    "`let mut` on a primitive is needed only when `&mut` of it is taken; a plain `let` is already reassignable"
                }
                LetMutKind::Annotated => {
                    "the annotation already grants `mut`, and no `&mut` of this binding is taken; remove the `mut` before the name"
                }
            };
            self.diags
                .push(Code::RedundantLetMut, Severity::Warning, span, msg, None);
            let Some(tok) = node.direct_token(&self.cx.src.parse.tokens, TokenKind::KwMut) else {
                continue;
            };
            let lo = self.cx.src.token_span(tok).lo;
            let next = hd_base::TokenIdx::from_raw(tok.raw() + 1);
            let hi = self.cx.src.token_span(next).lo.max(lo);
            let span = hd_base::Span {
                file: self.cx.src.file,
                lo,
                hi,
            };
            self.diags
                .add_fix("remove `mut`", FixSafety::Exact, &[(span, "")]);
        }
    }

    /// Lowers every reference type the finished body records to `Ref[T]`,
    /// the data type that represents `&T` and `&mut T` on the backend
    /// (`Types::lower_refs`).
    pub(crate) fn lower_body_refs(&mut self) {
        let pool = self.pool();
        let cell = self.cx.names.known.ref_cell;
        hd_tir::ir::map_body_types(self.b.body_mut(), &|t| pool.lower_refs(cell, t), &|l| {
            pool.lower_refs_list(cell, l)
        });
    }
}
