//! References (spec/lang/13-values-and-references.md): the types `&T` and
//! `&mut T`, `&e` and `&mut e` on places and on other values, `*e` and
//! writes through a reference, automatic dereference on field access and
//! method calls, and the read-only rules (`&mut T` converts to `&T`;
//! reading through `&T` weakens every `&mut` inside).
//!
//! The Wasm GC emulation (`crates/hd_wasm/DESIGN.md` §15.2,
//! "References"): a reference is a `std.core.Ref[T]` object holding two
//! closures, `get` and `set`, that read and replace the referenced place.
//! `&x` on a local makes closures that capture `x`, so the backend keeps
//! `x` in a shared cell; `&e` on any other value binds it to a fresh local
//! that only the closures hold, which is the new object. A place reached
//! through struct fields is rebuilt on a write, as `value.rs` rebuilds a
//! struct field write. The checker lowers `&T` and `&mut T` to one type,
//! `Ref[T]`, when it finishes a body (`Types::lower_refs`), so the
//! backend sees an ordinary data type and `&mut T` to `&T` costs nothing.
//!
//! Identity (`ref.identity.*`): a `Ref` also holds the place's identity,
//! which `r is s` compares field by field in the backend. A local some `&` in the file
//! may name draws a serial number from the `ref_serial` intrinsic where it
//! is declared, so two references to one execution of its declaration
//! share it; a new object draws one per evaluation. A data field's place
//! is named by its data object; a top-level binding's by its path.

use hd_base::{DefId, LocalId, NodeIdx, StageResult, Symbol};
use hd_diag::{Code, FixSafety, Severity};
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};
use hd_tir::ir::{Callee, Coercion, IntrinsicOp, NONE, Providers, Ref, Tag, TirSink, local_flags};
use hd_types::{ParamRef, Prim, RowId, Ty, TyData, TyList};

use crate::body::{Ck, ExpectPosition, LetMutKind, OpenSub, unsupported};

/// Where a referenced place starts.
#[derive(Clone, Copy)]
pub(crate) enum Root {
    /// A local, with its scope depth.
    Local(LocalId, usize),
    /// A top-level binding: its name, id and type.
    Global(Symbol, DefId, Ty),
    /// The place a reference refers to. The reference, evaluated once, is
    /// held by a local made for it (with its depth); its type follows.
    Deref(LocalId, usize, Ty),
    /// A field of a data value, evaluated once and held by a local made
    /// for it (with its depth): the field's index, the field's type, and
    /// the data value's type.
    DataField(LocalId, usize, u32, Ty, Ty),
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

    /// The type a read through `&T` or `&mut T` yields (`ref.read.type`,
    /// `ref.read.weaken`): through `&mut T`, `T`, with mutable access when
    /// `T` has views; through `&T`, `T` with every `&mut` and `mut` view
    /// inside it weakened.
    pub(crate) fn deref_ty(&self, inner: Ty, mutable: bool) -> Ty {
        let pool = self.pool();
        if !mutable {
            return self.weaken(inner);
        }
        let inner = self.infer.resolve(pool, inner);
        if self.is_composite(inner) && !matches!(pool.get(inner), TyData::Mut(_)) {
            pool.intern_ty(&TyData::Mut(inner))
        } else {
            inner
        }
    }

    /// `t` as a read through a read-only reference sees it
    /// (`ref.read.weaken`): each `&mut U` inside becomes `&U` and each
    /// `mut U` view becomes `U`, at any depth, except inside a function
    /// type.
    pub(crate) fn weaken(&self, t: Ty) -> Ty {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        let list = |l: TyList| {
            pool.list(
                &pool
                    .list_items(l)
                    .iter()
                    .map(|e| self.weaken(*e))
                    .collect::<Vec<_>>(),
            )
        };
        let d = match pool.get(t) {
            TyData::Ref { inner, .. } => TyData::Ref {
                inner: self.weaken(inner),
                mutable: false,
            },
            TyData::Mut(i) => return self.weaken(i),
            TyData::Adt { def, args } => TyData::Adt {
                def,
                args: list(args),
            },
            TyData::Option(i) => TyData::Option(self.weaken(i)),
            TyData::Tuple { elems, rest } => TyData::Tuple {
                elems: list(elems),
                rest: rest.map(|r| self.weaken(r)),
            },
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => TyData::TraitValue {
                def,
                args: list(args),
                bindings,
            },
            _ => return t,
        };
        pool.intern_ty(&d)
    }

    /// Whether a value of type `t` holds a `&mut` at any depth: through
    /// the fields of structs, data types and enums, tuple elements,
    /// optionals, newtypes and type arguments (`ref.read.copy-out`).
    fn holds_mut_ref(&self, t: Ty, seen: &mut Vec<Ty>) -> bool {
        let pool = self.pool();
        let t = self.infer.resolve(pool, t);
        if seen.contains(&t) || seen.len() > 256 {
            return false;
        }
        seen.push(t);
        match pool.get(t) {
            TyData::Ref { mutable, .. } => mutable,
            TyData::Mut(i) | TyData::Option(i) => self.holds_mut_ref(i, seen),
            TyData::Tuple { elems, rest } => pool
                .list_items(elems)
                .iter()
                .copied()
                .chain(rest)
                .any(|e| self.holds_mut_ref(e, seen)),
            TyData::Adt { def, args } => {
                let argv = pool.list_items(args);
                if argv.iter().any(|a| self.holds_mut_ref(*a, seen)) {
                    return true;
                }
                let sub = |f: Ty| {
                    pool.subst(f, &|p: ParamRef| {
                        (p.owner == def)
                            .then(|| argv.get(usize::from(p.index)).copied())
                            .flatten()
                    })
                };
                let Some(item) = self.cx.lookup.item(def) else {
                    return false;
                };
                let fields: Vec<Ty> = match &item.data {
                    hd_resolve::ItemData::Data(fs) => fs.iter().map(|f| sub(f.ty)).collect(),
                    hd_resolve::ItemData::Enum { shared, variants } => shared
                        .iter()
                        .chain(variants.iter().flat_map(|v| &v.fields))
                        .map(|f| sub(f.ty))
                        .collect(),
                    hd_resolve::ItemData::Newtype(base) => vec![sub(*base)],
                    _ => vec![],
                };
                fields.into_iter().any(|f| self.holds_mut_ref(f, seen))
            }
            _ => false,
        }
    }

    /// `ref.read.copy-out`: a whole value of type `t` read through a
    /// read-only reference must not hold a `&mut`, which would be writable
    /// again in the copy.
    pub(crate) fn check_copy_out(&mut self, t: Ty, n: NodeRef<'_>) {
        if self.holds_mut_ref(t, &mut Vec::new()) {
            let msg = format!(
                "this copies a value of type {} out of a read-only reference, and it holds a `&mut`, which the copy would make writable again; read the fields you need instead",
                self.show(t)
            );
            self.err(Code::MutableUpgrade, n, &msg);
        }
    }

    /// The type of a field or element of type `t` read from a value that
    /// `shared` says was read through a read-only reference: weakened
    /// (`ref.read.weaken`), and no copy of a `&mut` unless the read is
    /// itself the base of another field read (`projected`).
    pub(crate) fn shared_read(
        &mut self,
        t: Ty,
        shared: bool,
        projected: bool,
        n: NodeRef<'_>,
    ) -> Ty {
        if !shared {
            return t;
        }
        let w = self.weaken(t);
        if !projected {
            self.check_copy_out(w, n);
        }
        w
    }

    /// Whether the value `r` is read through a read-only reference: a
    /// field, or a field of a field, of a value read through `&T`
    /// (`ref.read.weaken`).
    pub(crate) fn via_shared_ref(&mut self, r: Ref) -> bool {
        let mut cur = r;
        while let Some(i) = cur.as_inst() {
            let (tag, data) = {
                let body = self.b.body_mut();
                (body.tags[i.0 as usize], body.data[i.0 as usize])
            };
            match tag {
                Tag::Field | Tag::TupleGet => cur = Ref(data[0]),
                Tag::CallValue => return self.deref_source(Ref(data[0])).is_some_and(|(_, m)| !m),
                _ => return false,
            }
        }
        false
    }

    /// When `callee`, the function value of a `CallValue`, is the `get`
    /// closure of a reference (as [`Self::emit_deref`] reads it): the
    /// reference value and whether it is `&mut`.
    pub(crate) fn deref_source(&mut self, callee: Ref) -> Option<(Ref, bool)> {
        let i = callee.as_inst()?;
        let (tag, data) = {
            let body = self.b.body_mut();
            (body.tags[i.0 as usize], body.data[i.0 as usize])
        };
        if tag != Tag::Field || data[1] != 0 {
            return None;
        }
        let base = Ref(data[0]);
        let (_, m) = self.ref_of(self.b.ty_of(base))?;
        Some((base, m))
    }

    /// Reads through the reference `r` of type `rt` (`*r`, or an automatic
    /// dereference): calls its `get` closure.
    pub(crate) fn emit_deref(&mut self, r: Ref, rt: Ty, at: NodeIdx) -> (Ref, Ty) {
        let pool = self.pool();
        let Some((inner, mutable)) = self.ref_of(rt) else {
            return (r, rt);
        };
        let get = pool.intern_ty(&fn_ty(pool, &[], inner));
        let f = self.b.emit(Tag::Field, r.0, 0, get, at);
        let rec = self.b.refs_record(&[]);
        let t = self.deref_ty(inner, mutable);
        (self.b.emit(Tag::CallValue, f.0, rec, t, at), t)
    }

    /// Replaces the value the reference `r` of type `rt` refers to with
    /// `v` (`*r = v`): calls its `set` closure.
    pub(crate) fn emit_ref_store(&mut self, r: Ref, rt: Ty, v: Ref, at: NodeIdx) {
        let pool = self.pool();
        let Some((inner, _)) = self.ref_of(rt) else {
            return;
        };
        let set = pool.intern_ty(&fn_ty(pool, &[inner], Ty::VOID));
        let f = self.b.emit(Tag::Field, r.0, 1, set, at);
        let v = self.retype(v, inner, at);
        let rec = self.b.refs_record(&[v]);
        self.b.emit(Tag::CallValue, f.0, rec, Ty::VOID, at);
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
        let want_inner = want.and_then(|w| self.ref_of(w)).map(|(i, _)| i);
        let Some(op) = self.operand(e, want_inner)? else {
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
                // A new object holds a copy of the value; `&mut` on a
                // readonly view would grant writes through it
                // (`ref.expr.allocate.view`).
                if mutable && self.is_composite(t) && !self.has_mut_access(t) {
                    let msg = format!(
                        "`&mut` needs mutable access, but the value is a readonly {}",
                        self.show(t)
                    );
                    self.err(Code::MutableUpgrade, e, &msg);
                    return Ok(self.ref_poison(n));
                }
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
        let r = self.make_ref(&place, inner, mutable, rt, n);
        Ok((r, rt))
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

    /// The operand of `&` or `&mut`: a place (`ref.place.forms`), or any
    /// other expression, evaluated. `None` after an error.
    pub(crate) fn operand(
        &mut self,
        e: NodeRef<'_>,
        want: Option<Ty>,
    ) -> StageResult<Option<Operand>> {
        let kids: Vec<NodeRef<'_>> = e.children().collect();
        match e.kind() {
            SyntaxKind::ParenExpr => match kids.as_slice() {
                [inner] => self.operand(*inner, want),
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
                    // Reports what a read reports (initialization order).
                    if let Some((r, _)) = self.global_get(s, e)
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
                let Some(b) = self.operand(base, None)? else {
                    return Ok(None);
                };
                self.field_step(b, &name, e)
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
                let (l, d) = self.hold(r, t, inner);
                Ok(Some(Operand::Place(Place {
                    root: Root::Deref(l, d, t),
                    path: vec![],
                    ty: self.deref_ty(it, m),
                })))
            }
            SyntaxKind::IndexExpr => {
                // `ref.check.not-yet-elements`: element references pin their list,
                // which comes with a later job.
                self.err(
                    Code::ReferencesNotYetChecked,
                    e,
                    "a reference to a list element or a map value is not checked yet",
                );
                Ok(None)
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
    fn field_step(
        &mut self,
        b: Operand,
        name: &str,
        e: NodeRef<'_>,
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
                    let (l, d) = self.hold(r, t, e);
                    let p = Place {
                        root: Root::Deref(l, d, t),
                        path: vec![],
                        ty: self.deref_ty(inner, m),
                    };
                    return self.field_step(Operand::Place(p), name, e);
                }
                let pool = self.pool();
                if let TyData::Tuple { .. } = pool.get(self.strip_mut(self.infer.resolve(pool, t)))
                {
                    // `ref.check.not-yet-elements`
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
                    let v = self.b.emit(Tag::Field, r.0, idx, ft, e.index());
                    return Ok(Some(Operand::Value(v, ft)));
                }
                let ft = self.field_access(t, name, ft);
                let (l, d) = self.hold(r, t, e);
                Ok(Some(Operand::Place(Place {
                    root: Root::DataField(l, d, idx, ft, t),
                    path: vec![],
                    ty: ft,
                })))
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
    fn place_writable(&mut self, p: &Place, at: NodeRef<'_>) -> bool {
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
            Root::Deref(_, _, rt) => {
                if self.ref_of(rt).is_some_and(|(_, m)| m) {
                    true
                } else {
                    let msg = format!(
                        "`&mut` refers through a read-only reference {}; take the reference with `&mut` to write through it",
                        self.show(rt)
                    );
                    self.err(Code::ReadonlyRoot, at, &msg);
                    false
                }
            }
            Root::DataField(_, _, _, _, dt) => {
                if self.has_mut_access(dt) {
                    true
                } else {
                    let msg = format!(
                        "`&mut` refers to a field of a readonly {}, which grants no writes",
                        self.show(dt)
                    );
                    self.err(Code::ReadonlyRoot, at, &msg);
                    false
                }
            }
        };
        if !ok {
            return false;
        }
        // `ref.place.writable.view`: a readonly view in the place grants no
        // writes, so `&mut` to it would upgrade it.
        if self.is_composite(p.ty) && !self.has_mut_access(p.ty) {
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
                Code::ReadonlyRoot,
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
            v = self.b.emit(Tag::Field, v.0, idx, ft, n.index());
            t = ft;
        }
        (v, t)
    }

    fn read_root(&mut self, root: Root, n: NodeRef<'_>) -> (Ref, Ty) {
        match root {
            Root::Local(l, d) => self.read_local(l, d, n),
            Root::Global(s, def, ty) => self.global_get(s, n).unwrap_or_else(|| {
                let a = self.b.refs_record(&[Ref(def.raw())]);
                (self.b.emit(Tag::GlobalGet, a, NONE, ty, n.index()), ty)
            }),
            Root::Deref(l, d, rt) => {
                let (r, _) = self.read_local(l, d, n);
                self.emit_deref(r, rt, n.index())
            }
            Root::DataField(l, d, idx, ft, _) => {
                let (o, _) = self.read_local(l, d, n);
                (self.b.emit(Tag::Field, o.0, idx, ft, n.index()), ft)
            }
        }
    }

    /// Stores `v` of type `vt` into the place `p`: a root is replaced; a
    /// struct field is stored by rebuilding each struct on the path and
    /// replacing the root (`types.value.write-once`).
    fn store_place(&mut self, p: &Place, v: Ref, n: NodeRef<'_>) {
        let at = n.index();
        let Some((&(last, _), init)) = p.path.split_last() else {
            self.store_root(p.root, v, n);
            return;
        };
        let (root, _) = self.read_root(p.root, n);
        let mut vals = vec![root];
        for &(i, ty) in init {
            let prev = vals[vals.len() - 1];
            vals.push(self.b.emit(Tag::Field, prev.0, i, ty, at));
        }
        let mut new = v;
        let mut field = last;
        for (k, val) in vals.iter().enumerate().rev() {
            let ty = self.b.ty_of(*val);
            let rec = self.b.refs_record(&[Ref(field), new]);
            new = self.b.emit(Tag::CopyData, val.0, rec, ty, at);
            if k > 0 {
                field = init[k - 1].0;
            }
        }
        self.store_root(p.root, new, n);
    }

    fn store_root(&mut self, root: Root, v: Ref, n: NodeRef<'_>) {
        let at = n.index();
        match root {
            Root::Local(l, d) => {
                self.note_write(l, d);
                let lt = self.b.local_ty(l);
                let v = self.retype(v, lt, at);
                self.b.set(l, v, at);
            }
            Root::Global(_, def, ty) => {
                let v = self.retype(v, ty, at);
                let a = self.b.refs_record(&[Ref(def.raw())]);
                self.b.emit(Tag::GlobalSet, a, v.0, Ty::VOID, at);
            }
            Root::Deref(l, d, rt) => {
                let (r, _) = self.read_local(l, d, n);
                self.emit_ref_store(r, rt, v, at);
            }
            Root::DataField(l, d, idx, ft, _) => {
                let (o, _) = self.read_local(l, d, n);
                let v = self.retype(v, ft, at);
                let rec = self.b.refs_record(&[Ref(idx), v]);
                self.b.emit(Tag::FieldSet, o.0, rec, Ty::VOID, at);
            }
        }
    }

    /// The reference to the place `p`: a `Ref[T]` object whose `get`
    /// closure reads the place and whose `set` closure replaces it. A
    /// read-only reference's `set` is never called.
    pub(crate) fn make_ref(
        &mut self,
        p: &Place,
        inner: Ty,
        mutable: bool,
        rt: Ty,
        n: NodeRef<'_>,
    ) -> Ref {
        let pool = self.pool();
        let syn = n.index();
        let get_ty = pool.intern_ty(&fn_ty(pool, &[], inner));
        let set_ty = pool.intern_ty(&fn_ty(pool, &[inner], Ty::VOID));
        // `get`: reads the place.
        let mark = self.b.open_sub(&[]);
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len(),
        });
        let blk = self.b.open_block();
        let (v, _) = self.read_place(p, n);
        let v = self.retype(v, inner, syn);
        let root = self.b.close_block(blk, Some(v), inner, syn);
        self.subs.pop();
        let get = self.b.close_sub(mark, root, get_ty, syn);
        // `set`: replaces it.
        let sym = self.cx.names.syms.intern("$value");
        let param = self.b.local(inner, sym, local_flags::PARAM, syn);
        let mark = self.b.open_sub(&[param]);
        self.subs.push(OpenSub {
            mark,
            depth: self.scopes.len(),
        });
        let blk = self.b.open_block();
        if mutable {
            let v = self.b.get(param, inner, syn);
            self.store_place(p, v, n);
        } else {
            self.b.emit(Tag::Unreachable, NONE, NONE, Ty::NEVER, syn);
        }
        let root = self.b.close_block(blk, None, Ty::VOID, syn);
        self.subs.pop();
        let set = self.b.close_sub(mark, root, set_ty, syn);
        let (base, serial, key) = self.place_identity(p, n);
        let rec = self.b.refs_record(&[get, set, base, serial, key]);
        self.b.emit(Tag::NewData, NONE, rec, rt, syn)
    }

    /// The type of a reference's identity base, `dyn Any`.
    fn any_ty(&self) -> Ty {
        self.pool().intern_ty(&TyData::TraitValue {
            def: self.cx.names.known.any,
            args: TyList::EMPTY,
            bindings: vec![],
        })
    }

    /// The identity of the place `p`, evaluated where the reference is
    /// taken (`ref.identity.same-place`): the data object that holds it as
    /// `dyn Any` (`RefRoot`'s one value when no data object does), the
    /// serial number of its local or new object (0 for a top-level binding
    /// or a data field), and the path of fields from there.
    fn place_identity(&mut self, p: &Place, n: NodeRef<'_>) -> (Ref, Ref, Ref) {
        let at = n.index();
        let i64_ty = Ty::prim(Prim::I64);
        let mut suffix = String::new();
        for (idx, _) in &p.path {
            suffix.push('.');
            suffix.push_str(&idx.to_string());
        }
        match p.root {
            Root::Local(l, d) => {
                let base = self.root_base(n);
                let serial = match self.ref_serials.get(&l).copied() {
                    Some((sl, _)) => self.read_local(sl, d, n).0,
                    None => self.new_ref_serial(at),
                };
                (base, serial, self.b.const_str(&suffix))
            }
            Root::Global(_, def, _) => {
                let base = self.root_base(n);
                let serial = self.b.const_value(i64_ty, 0);
                let key = format!("g:{}{suffix}", self.cx.names.path(def));
                (base, serial, self.b.const_str(&key))
            }
            Root::Deref(l, d, _) => {
                let (r, _) = self.read_local(l, d, n);
                let any = self.any_ty();
                let base = self.b.emit(Tag::Field, r.0, 2, any, at);
                let serial = self.b.emit(Tag::Field, r.0, 3, i64_ty, at);
                let mut key = self.b.emit(Tag::Field, r.0, 4, Ty::STRING, at);
                if !suffix.is_empty() {
                    let tail = self.b.const_str(&suffix);
                    let rec = self.b.refs_record(&[key, tail]);
                    key = self.b.emit(
                        Tag::Intrinsic,
                        IntrinsicOp::StrConcat as u32,
                        rec,
                        Ty::STRING,
                        at,
                    );
                }
                (base, serial, key)
            }
            Root::DataField(l, d, idx, _, dt) => {
                let (o, _) = self.read_local(l, d, n);
                let any = self.any_ty();
                let o = self.retype(o, self.strip_mut(dt), at);
                let base = self.coerce(
                    o,
                    self.strip_mut(dt),
                    any,
                    n,
                    "reference",
                    ExpectPosition::General,
                );
                let serial = self.b.const_value(i64_ty, 0);
                (base, serial, self.b.const_str(&format!("f{idx}{suffix}")))
            }
        }
    }

    /// The identity of the reference `r` as a value of std's
    /// `RefIdentity` (`ty`), whose fields are the reference's identity
    /// fields in order: what std's `identity_of` returns.
    pub(crate) fn ref_identity_value(&mut self, r: Ref, ty: Ty, n: NodeRef<'_>) -> Ref {
        let at = n.index();
        let any = self.any_ty();
        let base = self.b.emit(Tag::Field, r.0, 2, any, at);
        let serial = self.b.emit(Tag::Field, r.0, 3, Ty::prim(Prim::I64), at);
        let key = self.b.emit(Tag::Field, r.0, 4, Ty::STRING, at);
        let rec = self.b.refs_record(&[base, serial, key]);
        self.b.emit(Tag::NewData, NONE, rec, ty, at)
    }

    /// `RefRoot`'s one value as `dyn Any`: the identity base of a place no
    /// data object holds.
    fn root_base(&mut self, n: NodeRef<'_>) -> Ref {
        let pool = self.pool();
        let root_ty = pool.intern_ty(&TyData::Adt {
            def: self.cx.names.known.ref_root,
            args: TyList::EMPTY,
        });
        let rec = self.b.refs_record(&[]);
        let v = self.b.emit(Tag::NewData, NONE, rec, root_ty, n.index());
        let any = self.any_ty();
        self.coerce(v, root_ty, any, n, "reference", ExpectPosition::General)
    }

    /// A fresh serial number (`ref_serial`).
    fn new_ref_serial(&mut self, at: NodeIdx) -> Ref {
        let c = Callee::Item {
            def: self.cx.names.known.ref_serial,
            targs: TyList::EMPTY,
        };
        self.b
            .call(&c, &[], Providers::None, Ty::prim(Prim::I64), at)
    }

    /// The names some `&` or `&mut` in the module's file refers to through
    /// a chain of fields (`&x`, `&mut x.pos.y`): a local of one of these
    /// names draws a serial number where it is declared.
    fn ref_names(&self) -> &hd_base::HashSet<Symbol> {
        self.cx.ref_names.get_or_init(|| {
            let tree = &self.cx.src.parse.tree;
            let mut names = hd_base::HashSet::default();
            for i in 0..tree.len() {
                let node = tree.node(NodeIdx::from_raw(u32::try_from(i).expect("nodes")));
                if node.kind() != SyntaxKind::RefExpr {
                    continue;
                }
                let mut e = node.children().next();
                while let Some(x) = e {
                    match x.kind() {
                        SyntaxKind::ParenExpr | SyntaxKind::FieldExpr => e = x.children().next(),
                        SyntaxKind::NameExpr => {
                            names.insert(self.sym_of(x));
                            break;
                        }
                        _ => break,
                    }
                }
            }
            names
        })
    }

    /// Names the local that will hold the serial number of the local `l`
    /// when some `&` in the file names it: from here on, a reference to
    /// `l` reads that local, which `l`'s declaration fills
    /// ([`Self::draw_ref_serial`]). A pattern's binding is named before
    /// the arm that uses it is checked, and stored after, in the decision
    /// that runs first.
    pub(crate) fn reserve_ref_serial(&mut self, l: LocalId, at: NodeIdx) {
        if self.ref_serials.contains_key(&l) {
            return;
        }
        let name = self.b.body_mut().local_name[l.idx()];
        if !self.ref_names().contains(&name) {
            return;
        }
        let sym = self.cx.names.syms.intern("$serial");
        let sl = self
            .b
            .local(Ty::prim(Prim::I64), sym, local_flags::ASSIGNED, at);
        self.ref_serials.insert(l, (sl, false));
    }

    /// Draws the serial number of the local `l` where it is declared: its
    /// first store, or a parameter's entry (`ref.identity.places`), when
    /// some `&` in the file names it. Each execution of the declaration
    /// draws a new one, as each makes a new place.
    pub(crate) fn draw_ref_serial(&mut self, l: LocalId, at: NodeIdx) {
        self.reserve_ref_serial(l, at);
        let Some(&(sl, false)) = self.ref_serials.get(&l) else {
            return;
        };
        let v = self.new_ref_serial(at);
        self.b.set(sl, v, at);
        self.ref_serials.insert(l, (sl, true));
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
                if matches!(self.infer.resolve(pool, other_ty), Ty::POISON | Ty::NEVER) {
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
        // (`crates/hd_wasm/DESIGN.md` §15.2, "References"), so a generic
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
        let Some((_, mutable)) = self.ref_of(t) else {
            self.not_a_reference(t, e);
            return Ok(self.ref_poison(n));
        };
        let (v, vt) = self.emit_deref(r, t, n.index());
        if !mutable {
            self.check_copy_out(vt, n);
        }
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
            self.err(Code::ReadonlyRoot, lhs, &msg);
            return Ok(());
        }
        // A value with views is stored with mutable access, which a read
        // through the reference grants (`ref.read.type`).
        let want = self.deref_ty(inner, true);
        let v = match compound {
            None => {
                let (vr, vt) = self.expr(rhs, Some(want))?;
                self.coerce(vr, vt, want, rhs, "assignment", ExpectPosition::General)
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

    /// `redundant-let-mut` (`types.bind.let-mut-primitive-unused`,
    /// `types.bind.let-mut-annotated.unused`): a `let mut` local of a
    /// primitive type, or with a `mut T` annotation, whose address no
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

/// The type `fn(params) -> result` with no requirements.
fn fn_ty(pool: hd_types::Types<'_>, params: &[Ty], result: Ty) -> TyData {
    TyData::Fn {
        params: pool.list(params),
        result,
        row: RowId::EMPTY,
        suspends: false,
        inputs: hd_types::Inputs::Fixed,
    }
}
